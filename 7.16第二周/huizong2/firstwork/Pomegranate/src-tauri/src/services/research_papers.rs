use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{Datelike, Utc};
use reqwest::header::USER_AGENT;
use serde::Deserialize;

use crate::error::AppError;
use crate::models::{ResearchPaper, ResearchPaperSearchInput, ResearchPaperSearchResult};
use crate::services::http_client;

const CROSSREF_WORKS_URL: &str = "https://api.crossref.org/works";
const CROSSREF_SOURCE: &str = "Crossref";
const CANDIDATE_ROWS: usize = 60;
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

static SEARCH_CACHE: OnceLock<Mutex<HashMap<String, CachedSearch>>> = OnceLock::new();

#[derive(Clone)]
struct CachedSearch {
    cached_at: Instant,
    result: ResearchPaperSearchResult,
}

pub struct ResearchPaperService;

impl ResearchPaperService {
    pub async fn search(
        input: ResearchPaperSearchInput,
    ) -> Result<ResearchPaperSearchResult, AppError> {
        let query = input.query.trim();
        if query.is_empty() {
            return Err(AppError::InvalidInput("请输入要检索的研究主题".to_string()));
        }
        if query.chars().count() > 300 {
            return Err(AppError::InvalidInput(
                "研究主题过长，请精简到 300 个字符以内".to_string(),
            ));
        }

        let limit = input.limit.unwrap_or(12).clamp(5, 20);
        let today = Utc::now().date_naive();
        let to_year = today.year();
        let from_year = to_year - 4;
        let cache_key = format!("{}|{}|{}", query.to_lowercase(), from_year, limit);

        if let Some(result) = read_cache(&cache_key) {
            return Ok(result);
        }

        let date_filter = format!(
            "from-pub-date:{from_year}-01-01,until-pub-date:{}",
            today.format("%Y-%m-%d")
        );
        let rows = CANDIDATE_ROWS.to_string();
        let mut request = http_client::shared()
            .get(CROSSREF_WORKS_URL)
            .header(
                USER_AGENT,
                "Pomegranate-AI-Research/1.0 (Crossref metadata search)",
            )
            .timeout(Duration::from_secs(25))
            .query(&[
                ("query.bibliographic", query),
                ("filter", date_filter.as_str()),
                ("rows", rows.as_str()),
            ]);

        // Crossref recommends an optional contact mailbox for the polite pool.
        // It is read from the environment only and is never persisted by firstwork.
        if let Ok(mailto) = std::env::var("CROSSREF_MAILTO") {
            let mailto = mailto.trim();
            if !mailto.is_empty() {
                request = request.query(&[("mailto", mailto)]);
            }
        }

        let response = request.send().await.map_err(|error| {
            AppError::Custom(format!("论文检索服务连接失败，请检查网络后重试：{error}"))
        })?;
        let status = response.status();
        if !status.is_success() {
            let message = match status.as_u16() {
                429 => "论文检索请求过于频繁，请稍后再试".to_string(),
                403 => "论文检索服务暂时拒绝访问，请稍后再试".to_string(),
                _ => format!("论文检索服务返回异常状态：{status}"),
            };
            return Err(AppError::Custom(message));
        }

        let payload = response.json::<CrossrefEnvelope>().await.map_err(|error| {
            AppError::Custom(format!("论文检索结果解析失败，请稍后重试：{error}"))
        })?;

        let total_results = payload.message.total_results;
        let papers = rank_papers(payload.message.items, from_year, to_year, limit);
        let result = ResearchPaperSearchResult {
            query: query.to_string(),
            from_year,
            to_year,
            total_results,
            papers,
            source: CROSSREF_SOURCE.to_string(),
        };

        write_cache(cache_key, result.clone());
        Ok(result)
    }
}

fn rank_papers(
    items: Vec<CrossrefWork>,
    from_year: i32,
    to_year: i32,
    limit: usize,
) -> Vec<ResearchPaper> {
    let mut seen = HashSet::new();
    let mut candidates = items
        .into_iter()
        .filter(|item| is_paper_type(&item.work_type))
        .filter_map(|item| to_candidate(item, from_year, to_year))
        .filter(|candidate| seen.insert(candidate.paper.id.to_lowercase()))
        .collect::<Vec<_>>();

    let max_relevance = candidates
        .iter()
        .map(|candidate| candidate.relevance.max(0.0))
        .fold(0.0_f64, f64::max);
    let max_citations = candidates
        .iter()
        .map(|candidate| candidate.paper.cited_by_count)
        .max()
        .unwrap_or(0);
    let year_span = (to_year - from_year + 1).max(1) as f64;

    for candidate in &mut candidates {
        let relevance = if max_relevance > 0.0 {
            (candidate.relevance.max(0.0) / max_relevance).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let recency =
            ((candidate.paper.publication_year - from_year + 1) as f64 / year_span).clamp(0.0, 1.0);
        let impact = if max_citations > 0 {
            (candidate.paper.cited_by_count as f64).ln_1p() / (max_citations as f64).ln_1p()
        } else {
            0.0
        };

        let score = relevance * 0.55 + recency * 0.30 + impact * 0.15;
        candidate.paper.frontier_score = (score * 100.0).round() as u32;
        candidate.paper.rank_reason = rank_reason(
            candidate.paper.publication_year,
            to_year,
            candidate.paper.cited_by_count,
            relevance,
        );
    }

    candidates.sort_by(|left, right| {
        right
            .paper
            .frontier_score
            .cmp(&left.paper.frontier_score)
            .then_with(|| {
                right
                    .paper
                    .publication_year
                    .cmp(&left.paper.publication_year)
            })
            .then_with(|| right.paper.cited_by_count.cmp(&left.paper.cited_by_count))
    });

    candidates
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.paper)
        .collect()
}

struct PaperCandidate {
    paper: ResearchPaper,
    relevance: f64,
}

fn to_candidate(item: CrossrefWork, from_year: i32, to_year: i32) -> Option<PaperCandidate> {
    let (publication_year, publication_date) = work_date(&item)?;
    let title = item
        .title
        .into_iter()
        .find(|title| !title.trim().is_empty())?
        .trim()
        .to_string();
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let doi = item
        .doi
        .map(|doi| doi.trim().to_string())
        .filter(|doi| !doi.is_empty());
    let url = doi
        .as_deref()
        .map(doi_url)
        .or_else(|| item.url.filter(|url| url.starts_with("http")))?;
    let id = doi.clone().unwrap_or_else(|| url.clone());
    let authors = item
        .authors
        .into_iter()
        .filter_map(author_name)
        .take(8)
        .collect::<Vec<_>>();
    let venue = item
        .container_titles
        .into_iter()
        .find(|venue| !venue.trim().is_empty())
        .map(|venue| venue.trim().to_string());

    Some(PaperCandidate {
        relevance: item.score.unwrap_or_default(),
        paper: ResearchPaper {
            id,
            title,
            authors,
            publication_year,
            publication_date,
            venue,
            publisher: item
                .publisher
                .filter(|publisher| !publisher.trim().is_empty()),
            work_type: item.work_type,
            cited_by_count: item.cited_by_count,
            doi,
            url,
            frontier_score: 0,
            rank_reason: String::new(),
        },
    })
}

fn work_date(item: &CrossrefWork) -> Option<(i32, Option<String>)> {
    item.published
        .as_ref()
        .or(item.published_online.as_ref())
        .or(item.published_print.as_ref())
        .and_then(crossref_date)
}

fn crossref_date(date: &CrossrefDate) -> Option<(i32, Option<String>)> {
    let parts = date.date_parts.first()?;
    let year = *parts.first()?;
    let formatted = match parts.as_slice() {
        [year, month, day, ..] => Some(format!("{year:04}-{month:02}-{day:02}")),
        [year, month] => Some(format!("{year:04}-{month:02}")),
        [year] => Some(format!("{year:04}")),
        _ => None,
    };
    Some((year, formatted))
}

fn author_name(author: CrossrefAuthor) -> Option<String> {
    if let Some(name) = author.name.filter(|name| !name.trim().is_empty()) {
        return Some(name.trim().to_string());
    }
    let name = [
        author.given.unwrap_or_default(),
        author.family.unwrap_or_default(),
    ]
    .into_iter()
    .map(|part| part.trim().to_string())
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ");
    (!name.is_empty()).then_some(name)
}

fn doi_url(doi: &str) -> String {
    if doi.starts_with("http://") || doi.starts_with("https://") {
        doi.to_string()
    } else {
        format!("https://doi.org/{doi}")
    }
}

fn is_paper_type(work_type: &str) -> bool {
    matches!(
        work_type,
        "journal-article"
            | "proceedings-article"
            | "posted-content"
            | "book-chapter"
            | "dissertation"
            | "report"
    )
}

fn rank_reason(year: i32, current_year: i32, citations: u64, relevance: f64) -> String {
    let mut reasons = Vec::new();
    if year >= current_year - 1 {
        reasons.push("近两年发表");
    } else {
        reasons.push("近五年研究");
    }
    if relevance >= 0.75 {
        reasons.push("主题匹配度高");
    } else {
        reasons.push("与主题相关");
    }
    if citations >= 100 {
        reasons.push("高被引");
    } else if citations >= 20 {
        reasons.push("已有较多引用");
    }
    reasons.join(" · ")
}

fn read_cache(key: &str) -> Option<ResearchPaperSearchResult> {
    let cache = SEARCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().ok()?;
    cache.retain(|_, entry| entry.cached_at.elapsed() < CACHE_TTL);
    cache.get(key).map(|entry| entry.result.clone())
}

fn write_cache(key: String, result: ResearchPaperSearchResult) {
    let cache = SEARCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.insert(
            key,
            CachedSearch {
                cached_at: Instant::now(),
                result,
            },
        );
    }
}

#[derive(Debug, Deserialize)]
struct CrossrefEnvelope {
    message: CrossrefMessage,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CrossrefMessage {
    #[serde(default)]
    total_results: u64,
    #[serde(default)]
    items: Vec<CrossrefWork>,
}

#[derive(Debug, Deserialize)]
struct CrossrefWork {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default, rename = "author")]
    authors: Vec<CrossrefAuthor>,
    #[serde(default, rename = "container-title")]
    container_titles: Vec<String>,
    publisher: Option<String>,
    #[serde(default, rename = "type")]
    work_type: String,
    #[serde(default, rename = "is-referenced-by-count")]
    cited_by_count: u64,
    #[serde(default)]
    score: Option<f64>,
    published: Option<CrossrefDate>,
    #[serde(rename = "published-online")]
    published_online: Option<CrossrefDate>,
    #[serde(rename = "published-print")]
    published_print: Option<CrossrefDate>,
}

#[derive(Debug, Deserialize)]
struct CrossrefAuthor {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CrossrefDate {
    #[serde(default)]
    date_parts: Vec<Vec<i32>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_doi_link() {
        assert_eq!(
            doi_url("10.1234/example"),
            "https://doi.org/10.1234/example"
        );
        assert_eq!(
            doi_url("https://doi.org/10.1234/example"),
            "https://doi.org/10.1234/example"
        );
    }

    #[test]
    fn parses_crossref_date() {
        let date = CrossrefDate {
            date_parts: vec![vec![2025, 7, 3]],
        };
        assert_eq!(
            crossref_date(&date),
            Some((2025, Some("2025-07-03".to_string())))
        );
    }

    #[test]
    fn ranks_recent_relevant_papers_without_network() {
        let items = vec![
            sample_work("10.1/old", 2022, 300, 20.0),
            sample_work("10.1/new", 2026, 10, 25.0),
            sample_work("10.1/book", 2026, 10, 25.0).with_type("book"),
        ];

        let papers = rank_papers(items, 2022, 2026, 10);

        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].doi.as_deref(), Some("10.1/new"));
        assert!(papers[0].rank_reason.contains("近两年发表"));
    }

    #[test]
    fn filters_duplicates_and_out_of_range_items() {
        let items = vec![
            sample_work("10.1/same", 2025, 1, 10.0),
            sample_work("10.1/same", 2025, 200, 99.0),
            sample_work("10.1/too-old", 2020, 1, 10.0),
        ];

        let papers = rank_papers(items, 2022, 2026, 10);

        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].doi.as_deref(), Some("10.1/same"));
    }

    trait WithType {
        fn with_type(self, work_type: &str) -> Self;
    }

    impl WithType for CrossrefWork {
        fn with_type(mut self, work_type: &str) -> Self {
            self.work_type = work_type.to_string();
            self
        }
    }

    fn sample_work(doi: &str, year: i32, citations: u64, score: f64) -> CrossrefWork {
        CrossrefWork {
            doi: Some(doi.to_string()),
            url: None,
            title: vec![format!("Paper {doi}")],
            authors: vec![CrossrefAuthor {
                given: Some("Ada".to_string()),
                family: Some("Lovelace".to_string()),
                name: None,
            }],
            container_titles: vec!["Journal".to_string()],
            publisher: Some("Publisher".to_string()),
            work_type: "journal-article".to_string(),
            cited_by_count: citations,
            score: Some(score),
            published: Some(CrossrefDate {
                date_parts: vec![vec![year, 1, 1]],
            }),
            published_online: None,
            published_print: None,
        }
    }
}
