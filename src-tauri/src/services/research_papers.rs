use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{Datelike, Utc};
use reqwest::header::USER_AGENT;
use serde::Deserialize;

use crate::error::AppError;
use crate::models::{
    ResearchPaper, ResearchPaperSearchInput, ResearchPaperSearchResult, ResearchPaperSource,
    ResearchPaperSourceStatus,
};
use crate::services::http_client;

const CROSSREF_WORKS_URL: &str = "https://api.crossref.org/works";
const SEMANTIC_SCHOLAR_SEARCH_URL: &str = "https://api.semanticscholar.org/graph/v1/paper/search";
const SEMANTIC_SCHOLAR_BULK_URL: &str =
    "https://api.semanticscholar.org/graph/v1/paper/search/bulk";
const ARXIV_SEARCH_URL: &str = "https://export.arxiv.org/api/query";
const OPENALEX_WORKS_URL: &str = "https://api.openalex.org/works";
const OPENALEX_ARXIV_SOURCE_ID: &str = "S4306400194";
const EUROPE_PMC_SEARCH_URL: &str = "https://www.ebi.ac.uk/europepmc/webservices/rest/search";
const PUBMED_SEARCH_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi";
const PUBMED_SUMMARY_URL: &str = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi";
const DBLP_SEARCH_URL: &str = "https://dblp.org/search/publ/api";
const CROSSREF_SOURCE: &str = "Crossref";
const SEMANTIC_SCHOLAR_SOURCE: &str = "Semantic Scholar";
const ARXIV_SOURCE: &str = "arXiv";
const EUROPE_PMC_SOURCE: &str = "Europe PMC";
const PUBMED_SOURCE: &str = "PubMed";
const DBLP_SOURCE: &str = "DBLP";
const CANDIDATE_ROWS_PER_SOURCE: usize = 40;
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

static SEARCH_CACHE: OnceLock<Mutex<HashMap<String, CachedSearch>>> = OnceLock::new();

#[derive(Clone)]
struct CachedSearch {
    cached_at: Instant,
    result: ResearchPaperSearchResult,
}

struct PlatformSearch {
    source: &'static str,
    total_results: u64,
    candidates: Vec<PaperCandidate>,
}

struct PaperCandidate {
    paper: ResearchPaper,
    relevance: f64,
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
        let cache_key = format!("v4|{}|{}|{}", query.to_lowercase(), from_year, limit);

        if let Some(result) = read_cache(&cache_key) {
            return Ok(result);
        }

        // 六个平台独立请求；单个平台不可用时保留其余平台的结果，避免一次限流拖垮整次检索。
        let (crossref, semantic_scholar, arxiv, europe_pmc, pubmed, dblp) = futures::join!(
            search_crossref(query, from_year, to_year),
            search_semantic_scholar(query, from_year, to_year),
            search_arxiv(query, from_year, to_year),
            search_europe_pmc(query, from_year, to_year),
            search_pubmed(query, from_year, to_year),
            search_dblp(query, from_year, to_year),
        );

        let platform_results = [
            (CROSSREF_SOURCE, crossref),
            (SEMANTIC_SCHOLAR_SOURCE, semantic_scholar),
            (ARXIV_SOURCE, arxiv),
            (EUROPE_PMC_SOURCE, europe_pmc),
            (PUBMED_SOURCE, pubmed),
            (DBLP_SOURCE, dblp),
        ];
        let mut candidates = Vec::new();
        let mut source_statuses = Vec::new();
        let mut warnings = Vec::new();
        let mut available_sources = Vec::new();
        let mut total_results = 0_u64;

        for (source_name, platform_result) in platform_results {
            match platform_result {
                Ok(mut result) => {
                    let result_count = result.candidates.len();
                    total_results = total_results.saturating_add(result.total_results);
                    available_sources.push(result.source);
                    candidates.append(&mut result.candidates);
                    source_statuses.push(ResearchPaperSourceStatus {
                        name: result.source.to_string(),
                        available: true,
                        result_count,
                        message: None,
                    });
                }
                Err(error) => {
                    let warning = format!("{source_name} 暂时不可用：{error}");
                    warnings.push(warning.clone());
                    source_statuses.push(ResearchPaperSourceStatus {
                        name: source_name.to_string(),
                        available: false,
                        result_count: 0,
                        message: Some(error),
                    });
                }
            }
        }

        if available_sources.is_empty() {
            return Err(AppError::Custom(format!(
                "多个论文检索平台均连接失败，请检查网络后重试。{}",
                warnings.join("；")
            )));
        }

        let papers = merge_and_rank_papers(candidates, query, from_year, to_year, limit);
        let result = ResearchPaperSearchResult {
            query: query.to_string(),
            from_year,
            to_year,
            total_results,
            papers,
            source: available_sources.join("、"),
            sources: source_statuses,
            warnings,
        };

        write_cache(cache_key, result.clone());
        Ok(result)
    }
}

async fn search_crossref(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    let date_filter = format!(
        "from-pub-date:{from_year}-01-01,until-pub-date:{}-12-31",
        to_year
    );
    let rows = CANDIDATE_ROWS_PER_SOURCE.to_string();
    let mut request = http_client::shared()
        .get(CROSSREF_WORKS_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/2.0 (multi-source academic search)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[
            ("query.bibliographic", query),
            ("filter", date_filter.as_str()),
            ("rows", rows.as_str()),
        ]);

    // Crossref 推荐提供联系邮箱进入 polite pool；只读环境变量，不持久化。
    if let Ok(mailto) = std::env::var("CROSSREF_MAILTO") {
        let mailto = mailto.trim();
        if !mailto.is_empty() {
            request = request.query(&[("mailto", mailto)]);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(platform_status_message(status.as_u16()));
    }

    let payload = response
        .json::<CrossrefEnvelope>()
        .await
        .map_err(|error| format!("结果解析失败：{error}"))?;
    let mut candidates = payload
        .message
        .items
        .into_iter()
        .filter(|item| is_paper_type(&item.work_type))
        .filter_map(|item| crossref_candidate(item, from_year, to_year))
        .collect::<Vec<_>>();
    normalize_relevance(&mut candidates);

    Ok(PlatformSearch {
        source: CROSSREF_SOURCE,
        total_results: payload.message.total_results,
        candidates,
    })
}

async fn search_semantic_scholar(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    match search_semantic_scholar_relevance(query, from_year, to_year).await {
        Ok(result) => Ok(result),
        Err(primary_error) => search_semantic_scholar_bulk(query, from_year, to_year)
            .await
            .map_err(|fallback_error| {
                format!("普通检索不可用（{primary_error}）；备用检索也失败（{fallback_error}）")
            }),
    }
}

async fn search_semantic_scholar_relevance(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    let limit = CANDIDATE_ROWS_PER_SOURCE.to_string();
    let year_filter = format!("{from_year}-{to_year}");
    let fields = [
        "title",
        "authors",
        "year",
        "publicationDate",
        "venue",
        "publicationTypes",
        "citationCount",
        "externalIds",
        "url",
        "abstract",
        "openAccessPdf",
    ]
    .join(",");
    let mut request = http_client::shared()
        .get(SEMANTIC_SCHOLAR_SEARCH_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/2.0 (multi-source academic search)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[
            ("query", query),
            ("year", year_filter.as_str()),
            ("limit", limit.as_str()),
            ("fields", fields.as_str()),
        ]);

    // 未配置密钥时仍使用公开接口；如用户提供密钥，则自动获得更稳定的额度。
    if let Ok(api_key) = std::env::var("SEMANTIC_SCHOLAR_API_KEY") {
        let api_key = api_key.trim();
        if !api_key.is_empty() {
            request = request.header("x-api-key", api_key);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(platform_status_message(status.as_u16()));
    }

    let payload = response
        .json::<SemanticScholarEnvelope>()
        .await
        .map_err(|error| format!("结果解析失败：{error}"))?;
    let returned_count = payload.data.len().max(1);
    let candidates = payload
        .data
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            semantic_scholar_candidate(
                item,
                from_year,
                to_year,
                ordered_relevance(index, returned_count),
            )
        })
        .collect::<Vec<_>>();

    Ok(PlatformSearch {
        source: SEMANTIC_SCHOLAR_SOURCE,
        total_results: payload.total,
        candidates,
    })
}

async fn search_semantic_scholar_bulk(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    let year_filter = format!("{from_year}-{to_year}");
    let fields = [
        "title",
        "authors",
        "year",
        "publicationDate",
        "venue",
        "publicationTypes",
        "citationCount",
        "externalIds",
        "url",
    ]
    .join(",");
    let mut request = http_client::shared()
        .get(SEMANTIC_SCHOLAR_BULK_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/3.1 (Semantic Scholar bulk fallback)",
        )
        .timeout(Duration::from_secs(35))
        .query(&[
            ("query", query),
            ("year", year_filter.as_str()),
            ("fields", fields.as_str()),
            ("sort", "publicationDate:desc"),
        ]);

    if let Ok(api_key) = std::env::var("SEMANTIC_SCHOLAR_API_KEY") {
        let api_key = api_key.trim();
        if !api_key.is_empty() {
            request = request.header("x-api-key", api_key);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(platform_status_message(status.as_u16()));
    }

    let payload = response
        .json::<SemanticScholarEnvelope>()
        .await
        .map_err(|error| format!("结果解析失败：{error}"))?;
    let total_results = payload.total;
    let returned_count = payload.data.len().max(1);
    let mut candidates = payload
        .data
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let relevance = semantic_bulk_relevance(&item, query, index, returned_count);
            semantic_scholar_candidate(item, from_year, to_year, relevance)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .relevance
            .partial_cmp(&left.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates.truncate(CANDIDATE_ROWS_PER_SOURCE);
    normalize_relevance(&mut candidates);

    Ok(PlatformSearch {
        source: SEMANTIC_SCHOLAR_SOURCE,
        total_results,
        candidates,
    })
}

async fn search_arxiv(query: &str, from_year: i32, to_year: i32) -> Result<PlatformSearch, String> {
    match search_arxiv_direct(query, from_year, to_year).await {
        Ok(result) => Ok(result),
        Err(primary_error) => search_arxiv_via_openalex(query, from_year, to_year)
            .await
            .map_err(|fallback_error| {
                format!(
                    "arXiv 直连接口不可用（{primary_error}）；备用元数据检索也失败（{fallback_error}）"
                )
            }),
    }
}

async fn search_arxiv_direct(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    let clean_query = query.replace('"', " ").replace('\\', " ");
    let search_query = format!(
        "all:\"{}\" AND submittedDate:[{}01010000 TO {}12312359]",
        clean_query.trim(),
        from_year,
        to_year
    );
    let max_results = CANDIDATE_ROWS_PER_SOURCE.to_string();
    let response = http_client::shared()
        .get(ARXIV_SEARCH_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/2.0 (multi-source academic search)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[
            ("search_query", search_query.as_str()),
            ("start", "0"),
            ("max_results", max_results.as_str()),
            ("sortBy", "relevance"),
            ("sortOrder", "descending"),
        ])
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(platform_status_message(status.as_u16()));
    }

    let body = response
        .text()
        .await
        .map_err(|error| format!("读取结果失败：{error}"))?;
    let payload = quick_xml::de::from_str::<ArxivFeed>(&body)
        .map_err(|error| format!("结果解析失败：{error}"))?;
    let returned_count = payload.entries.len().max(1);
    let candidates = payload
        .entries
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            arxiv_candidate(
                item,
                from_year,
                to_year,
                ordered_relevance(index, returned_count),
            )
        })
        .collect::<Vec<_>>();

    Ok(PlatformSearch {
        source: ARXIV_SOURCE,
        total_results: payload.total_results,
        candidates,
    })
}

async fn search_arxiv_via_openalex(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    let filter = format!(
        "from_publication_date:{from_year}-01-01,to_publication_date:{to_year}-12-31,\
         locations.source.id:{OPENALEX_ARXIV_SOURCE_ID}"
    )
    .replace(' ', "");
    let per_page = CANDIDATE_ROWS_PER_SOURCE.to_string();
    let fields = [
        "doi",
        "title",
        "display_name",
        "publication_year",
        "publication_date",
        "cited_by_count",
        "authorships",
        "primary_location",
        "locations",
        "abstract_inverted_index",
    ]
    .join(",");
    let response = http_client::shared()
        .get(OPENALEX_WORKS_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/3.1 (arXiv metadata fallback)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[
            ("search", query),
            ("filter", filter.as_str()),
            ("per-page", per_page.as_str()),
            ("select", fields.as_str()),
        ])
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(platform_status_message(status.as_u16()));
    }

    let payload = response
        .json::<OpenAlexEnvelope>()
        .await
        .map_err(|error| format!("结果解析失败：{error}"))?;
    let returned_count = payload.results.len().max(1);
    let candidates = payload
        .results
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            openalex_arxiv_candidate(
                item,
                from_year,
                to_year,
                ordered_relevance(index, returned_count),
            )
        })
        .collect::<Vec<_>>();

    Ok(PlatformSearch {
        source: ARXIV_SOURCE,
        total_results: payload.meta.count,
        candidates,
    })
}

async fn search_europe_pmc(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    let query_with_date =
        format!("({query}) AND FIRST_PDATE:[{from_year}-01-01 TO {to_year}-12-31]");
    let page_size = CANDIDATE_ROWS_PER_SOURCE.to_string();
    let response = http_client::shared()
        .get(EUROPE_PMC_SEARCH_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/3.0 (multi-source academic search)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[
            ("query", query_with_date.as_str()),
            ("format", "json"),
            ("resultType", "core"),
            ("pageSize", page_size.as_str()),
        ])
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(platform_status_message(status.as_u16()));
    }

    let payload = response
        .json::<EuropePmcEnvelope>()
        .await
        .map_err(|error| format!("结果解析失败：{error}"))?;
    let returned_count = payload.result_list.results.len().max(1);
    let candidates = payload
        .result_list
        .results
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            europe_pmc_candidate(
                item,
                from_year,
                to_year,
                ordered_relevance(index, returned_count),
            )
        })
        .collect::<Vec<_>>();

    Ok(PlatformSearch {
        source: EUROPE_PMC_SOURCE,
        total_results: payload.hit_count,
        candidates,
    })
}

async fn search_pubmed(
    query: &str,
    from_year: i32,
    to_year: i32,
) -> Result<PlatformSearch, String> {
    let retmax = CANDIDATE_ROWS_PER_SOURCE.to_string();
    let mindate = format!("{from_year}/01/01");
    let maxdate = format!("{to_year}/12/31");
    let mut search_request = http_client::shared()
        .get(PUBMED_SEARCH_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/3.0 (multi-source academic search)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[
            ("db", "pubmed"),
            ("term", query),
            ("retmax", retmax.as_str()),
            ("retmode", "json"),
            ("sort", "relevance"),
            ("datetype", "pdat"),
            ("mindate", mindate.as_str()),
            ("maxdate", maxdate.as_str()),
            ("tool", "pomegranate_ai_research"),
        ]);

    // NCBI 的 API key 与联系邮箱均为可选环境变量，只用于提升公开接口稳定性。
    if let Ok(api_key) = std::env::var("NCBI_API_KEY") {
        let api_key = api_key.trim();
        if !api_key.is_empty() {
            search_request = search_request.query(&[("api_key", api_key)]);
        }
    }
    if let Ok(email) = std::env::var("NCBI_EMAIL") {
        let email = email.trim();
        if !email.is_empty() {
            search_request = search_request.query(&[("email", email)]);
        }
    }

    let search_response = search_request
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let search_status = search_response.status();
    if !search_status.is_success() {
        return Err(platform_status_message(search_status.as_u16()));
    }
    let search_payload = search_response
        .json::<PubMedSearchEnvelope>()
        .await
        .map_err(|error| format!("检索结果解析失败：{error}"))?;
    let total_results = search_payload
        .search_result
        .count
        .parse::<u64>()
        .unwrap_or_default();
    if search_payload.search_result.id_list.is_empty() {
        return Ok(PlatformSearch {
            source: PUBMED_SOURCE,
            total_results,
            candidates: Vec::new(),
        });
    }

    let ids = search_payload.search_result.id_list.join(",");
    let mut summary_request = http_client::shared()
        .get(PUBMED_SUMMARY_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/3.0 (multi-source academic search)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[
            ("db", "pubmed"),
            ("id", ids.as_str()),
            ("retmode", "json"),
            ("tool", "pomegranate_ai_research"),
        ]);
    if let Ok(api_key) = std::env::var("NCBI_API_KEY") {
        let api_key = api_key.trim();
        if !api_key.is_empty() {
            summary_request = summary_request.query(&[("api_key", api_key)]);
        }
    }

    let summary_response = summary_request
        .send()
        .await
        .map_err(|error| format!("摘要信息连接失败：{error}"))?;
    let summary_status = summary_response.status();
    if !summary_status.is_success() {
        return Err(platform_status_message(summary_status.as_u16()));
    }
    let summary_payload = summary_response
        .json::<PubMedSummaryEnvelope>()
        .await
        .map_err(|error| format!("摘要信息解析失败：{error}"))?;
    let returned_count = summary_payload.result.uids.len().max(1);
    let candidates = summary_payload
        .result
        .uids
        .iter()
        .enumerate()
        .filter_map(|(index, uid)| {
            summary_payload.result.items.get(uid).and_then(|item| {
                pubmed_candidate(
                    uid,
                    item,
                    from_year,
                    to_year,
                    ordered_relevance(index, returned_count),
                )
            })
        })
        .collect::<Vec<_>>();

    Ok(PlatformSearch {
        source: PUBMED_SOURCE,
        total_results,
        candidates,
    })
}

async fn search_dblp(query: &str, from_year: i32, to_year: i32) -> Result<PlatformSearch, String> {
    let rows = CANDIDATE_ROWS_PER_SOURCE.to_string();
    let response = http_client::shared()
        .get(DBLP_SEARCH_URL)
        .header(
            USER_AGENT,
            "Pomegranate-AI-Research/3.0 (multi-source academic search)",
        )
        .timeout(Duration::from_secs(25))
        .query(&[("q", query), ("h", rows.as_str()), ("format", "json")])
        .send()
        .await
        .map_err(|error| format!("连接失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(platform_status_message(status.as_u16()));
    }

    let payload = response
        .json::<DblpEnvelope>()
        .await
        .map_err(|error| format!("结果解析失败：{error}"))?;
    let total_results = payload.result.hits.total.parse::<u64>().unwrap_or_default();
    let returned_count = payload.result.hits.items.len().max(1);
    let candidates = payload
        .result
        .hits
        .items
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            dblp_candidate(
                item.info,
                from_year,
                to_year,
                ordered_relevance(index, returned_count),
            )
        })
        .collect::<Vec<_>>();

    Ok(PlatformSearch {
        source: DBLP_SOURCE,
        total_results,
        candidates,
    })
}

fn platform_status_message(status: u16) -> String {
    match status {
        429 => "请求过于频繁，请稍后再试".to_string(),
        401 | 403 => "平台暂时拒绝访问或需要更高访问额度".to_string(),
        _ => format!("平台返回异常状态 HTTP {status}"),
    }
}

fn crossref_candidate(item: CrossrefWork, from_year: i32, to_year: i32) -> Option<PaperCandidate> {
    let (publication_year, publication_date) = crossref_work_date(&item)?;
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let title = item
        .title
        .into_iter()
        .find(|title| !title.trim().is_empty())?
        .trim()
        .to_string();
    let doi = item
        .doi
        .and_then(|doi| normalized_nonempty(doi))
        .map(|doi| normalize_doi(&doi));
    let url = doi
        .as_deref()
        .map(doi_url)
        .or_else(|| item.url.and_then(valid_http_url))?;
    let id = doi.clone().unwrap_or_else(|| url.clone());
    let authors = item
        .authors
        .into_iter()
        .filter_map(crossref_author_name)
        .take(8)
        .collect::<Vec<_>>();
    let venue = item
        .container_titles
        .into_iter()
        .find_map(normalized_nonempty);
    let abstract_text = item
        .abstract_text
        .map(|abstract_text| clean_text(&abstract_text))
        .filter(|abstract_text| !abstract_text.is_empty());

    Some(PaperCandidate {
        relevance: item.score.unwrap_or_default(),
        paper: ResearchPaper {
            id,
            title,
            authors,
            publication_year,
            publication_date,
            venue,
            publisher: item.publisher.and_then(normalized_nonempty),
            work_type: item.work_type,
            cited_by_count: item.cited_by_count,
            doi,
            url: url.clone(),
            frontier_score: 0,
            rank_reason: String::new(),
            sources: vec![ResearchPaperSource {
                name: CROSSREF_SOURCE.to_string(),
                url,
            }],
            abstract_text,
            highlights: Vec::new(),
        },
    })
}

fn semantic_scholar_candidate(
    item: SemanticScholarPaper,
    from_year: i32,
    to_year: i32,
    relevance: f64,
) -> Option<PaperCandidate> {
    let publication_year = item.year.or_else(|| {
        item.publication_date
            .as_deref()
            .and_then(|date| date.get(0..4))
            .and_then(|year| year.parse().ok())
    })?;
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let title = normalized_nonempty(item.title)?;
    let doi = item
        .external_ids
        .and_then(|ids| ids.doi)
        .and_then(normalized_nonempty)
        .map(|doi| normalize_doi(&doi));
    let semantic_url = item
        .url
        .and_then(valid_http_url)
        .unwrap_or_else(|| format!("https://www.semanticscholar.org/paper/{}", item.paper_id));
    let open_access_url = item
        .open_access_pdf
        .and_then(|pdf| pdf.url)
        .and_then(valid_http_url);
    let url = doi
        .as_deref()
        .map(doi_url)
        .or(open_access_url)
        .unwrap_or_else(|| semantic_url.clone());
    let id = doi
        .clone()
        .unwrap_or_else(|| format!("s2:{}", item.paper_id));
    let work_type = semantic_work_type(item.publication_types.as_deref().unwrap_or_default());
    let authors = item
        .authors
        .into_iter()
        .filter_map(|author| normalized_nonempty(author.name))
        .take(8)
        .collect::<Vec<_>>();
    let abstract_text = item
        .abstract_text
        .map(|abstract_text| clean_text(&abstract_text))
        .filter(|abstract_text| !abstract_text.is_empty());

    Some(PaperCandidate {
        relevance,
        paper: ResearchPaper {
            id,
            title,
            authors,
            publication_year,
            publication_date: item.publication_date.and_then(normalized_nonempty),
            venue: item.venue.and_then(normalized_nonempty),
            publisher: None,
            work_type,
            cited_by_count: item.citation_count.unwrap_or_default(),
            doi,
            url,
            frontier_score: 0,
            rank_reason: String::new(),
            sources: vec![ResearchPaperSource {
                name: SEMANTIC_SCHOLAR_SOURCE.to_string(),
                url: semantic_url,
            }],
            abstract_text,
            highlights: Vec::new(),
        },
    })
}

fn semantic_bulk_relevance(
    item: &SemanticScholarPaper,
    query: &str,
    index: usize,
    total: usize,
) -> f64 {
    let terms = query
        .split(|character: char| {
            character.is_whitespace()
                || matches!(character, ',' | '，' | ';' | '；' | ':' | '：' | '/' | '\\')
        })
        .map(str::trim)
        .filter(|term| term.chars().count() >= 2)
        .map(|term| term.to_lowercase())
        .collect::<HashSet<_>>();
    if terms.is_empty() {
        return ordered_relevance(index, total);
    }

    let title = item.title.to_lowercase();
    let abstract_text = item
        .abstract_text
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let title_matches = terms
        .iter()
        .filter(|term| title.contains(term.as_str()))
        .count() as f64;
    let abstract_matches = terms
        .iter()
        .filter(|term| abstract_text.contains(term.as_str()))
        .count() as f64;
    let lexical =
        ((title_matches * 3.0 + abstract_matches) / (terms.len() as f64 * 4.0)).clamp(0.0, 1.0);
    (lexical * 0.85 + ordered_relevance(index, total) * 0.15).clamp(0.0, 1.0)
}

fn arxiv_candidate(
    item: ArxivEntry,
    from_year: i32,
    to_year: i32,
    relevance: f64,
) -> Option<PaperCandidate> {
    let publication_year = item
        .published
        .get(0..4)
        .and_then(|year| year.parse::<i32>().ok())?;
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let title = normalized_nonempty(clean_text(&item.title))?;
    let arxiv_url = valid_http_url(item.id)?.replace("http://", "https://");
    let doi = item
        .doi
        .and_then(normalized_nonempty)
        .map(|doi| normalize_doi(&doi));
    let url = doi
        .as_deref()
        .map(doi_url)
        .unwrap_or_else(|| arxiv_url.clone());
    let id = doi.clone().unwrap_or_else(|| arxiv_url.clone());
    let abstract_text = normalized_nonempty(clean_text(&item.summary));
    let publication_date = item
        .published
        .get(0..10)
        .map(ToString::to_string)
        .or_else(|| Some(publication_year.to_string()));
    let authors = item
        .authors
        .into_iter()
        .filter_map(|author| normalized_nonempty(author.name))
        .take(8)
        .collect::<Vec<_>>();

    Some(PaperCandidate {
        relevance,
        paper: ResearchPaper {
            id,
            title,
            authors,
            publication_year,
            publication_date,
            venue: item
                .journal_reference
                .and_then(normalized_nonempty)
                .or_else(|| Some("arXiv 预印本".to_string())),
            publisher: Some("arXiv".to_string()),
            work_type: "posted-content".to_string(),
            cited_by_count: 0,
            doi,
            url,
            frontier_score: 0,
            rank_reason: String::new(),
            sources: vec![ResearchPaperSource {
                name: ARXIV_SOURCE.to_string(),
                url: arxiv_url,
            }],
            abstract_text,
            highlights: Vec::new(),
        },
    })
}

fn openalex_arxiv_candidate(
    item: OpenAlexWork,
    from_year: i32,
    to_year: i32,
    relevance: f64,
) -> Option<PaperCandidate> {
    let publication_year = item.publication_year?;
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let title = item
        .title
        .or(item.display_name)
        .and_then(normalized_nonempty)?;
    let arxiv_url = openalex_arxiv_url(&item.locations, item.doi.as_deref())?;
    let doi = item
        .doi
        .and_then(normalized_nonempty)
        .map(|doi| normalize_doi(&doi));
    let id = doi.clone().unwrap_or_else(|| arxiv_url.clone());
    let authors = item
        .authorships
        .into_iter()
        .filter_map(|authorship| normalized_nonempty(authorship.author.display_name))
        .take(8)
        .collect::<Vec<_>>();
    let venue = item
        .primary_location
        .and_then(|location| location.source)
        .and_then(|source| source.display_name)
        .and_then(normalized_nonempty)
        .or_else(|| Some("arXiv 预印本".to_string()));
    let abstract_text = rebuild_openalex_abstract(item.abstract_inverted_index);

    Some(PaperCandidate {
        relevance,
        paper: ResearchPaper {
            id,
            title,
            authors,
            publication_year,
            publication_date: item.publication_date.and_then(normalized_nonempty),
            venue,
            publisher: Some("arXiv".to_string()),
            work_type: "posted-content".to_string(),
            cited_by_count: item.cited_by_count,
            doi,
            url: arxiv_url.clone(),
            frontier_score: 0,
            rank_reason: String::new(),
            sources: vec![ResearchPaperSource {
                name: ARXIV_SOURCE.to_string(),
                url: arxiv_url,
            }],
            abstract_text,
            highlights: Vec::new(),
        },
    })
}

fn openalex_arxiv_url(locations: &[OpenAlexLocation], doi: Option<&str>) -> Option<String> {
    locations
        .iter()
        .filter_map(|location| location.landing_page_url.as_deref())
        .find_map(arxiv_url_from_value)
        .or_else(|| doi.and_then(arxiv_url_from_value))
}

fn arxiv_url_from_value(value: &str) -> Option<String> {
    let lower = value.to_lowercase();
    let identifier = if let Some(position) = lower.find("arxiv.org/abs/") {
        value.get(position + "arxiv.org/abs/".len()..)
    } else if let Some(position) = lower.find("arxiv.org/pdf/") {
        value.get(position + "arxiv.org/pdf/".len()..)
    } else if let Some(position) = lower.find("10.48550/arxiv.") {
        value.get(position + "10.48550/arxiv.".len()..)
    } else {
        None
    }?;
    let identifier = identifier
        .split(['?', '#'])
        .next()
        .unwrap_or(identifier)
        .trim_end_matches(".pdf")
        .trim_matches('/');
    (!identifier.is_empty()).then(|| format!("https://arxiv.org/abs/{identifier}"))
}

fn rebuild_openalex_abstract(
    inverted_index: Option<HashMap<String, Vec<usize>>>,
) -> Option<String> {
    let mut positioned_words = inverted_index?
        .into_iter()
        .flat_map(|(word, positions)| {
            positions
                .into_iter()
                .map(move |position| (position, word.clone()))
        })
        .collect::<Vec<_>>();
    positioned_words.sort_by_key(|(position, _)| *position);
    let abstract_text = positioned_words
        .into_iter()
        .map(|(_, word)| word)
        .collect::<Vec<_>>()
        .join(" ");
    normalized_nonempty(clean_text(&abstract_text))
}

fn europe_pmc_candidate(
    item: EuropePmcResult,
    from_year: i32,
    to_year: i32,
    relevance: f64,
) -> Option<PaperCandidate> {
    let publication_year = item
        .first_publication_date
        .as_deref()
        .and_then(extract_year)
        .or_else(|| item.pub_year.as_deref().and_then(extract_year))?;
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let title = item.title.and_then(normalized_nonempty)?;
    let source_id = normalized_nonempty(item.id)?;
    let source_code = item
        .source
        .and_then(normalized_nonempty)
        .unwrap_or_else(|| "MED".to_string());
    let europe_pmc_url = format!(
        "https://europepmc.org/article/{}/{}",
        source_code.to_uppercase(),
        source_id
    );
    let doi = item
        .doi
        .and_then(normalized_nonempty)
        .map(|doi| normalize_doi(&doi));
    let url = doi
        .as_deref()
        .map(doi_url)
        .unwrap_or_else(|| europe_pmc_url.clone());
    let authors = item
        .author_string
        .unwrap_or_default()
        .split(',')
        .filter_map(|author| normalized_nonempty(author.to_string()))
        .take(8)
        .collect::<Vec<_>>();
    let abstract_text = item
        .abstract_text
        .map(|abstract_text| clean_text(&abstract_text))
        .filter(|abstract_text| !abstract_text.is_empty());
    let publication_date = item
        .first_publication_date
        .and_then(normalized_nonempty)
        .or_else(|| Some(publication_year.to_string()));

    Some(PaperCandidate {
        relevance,
        paper: ResearchPaper {
            id: doi
                .clone()
                .unwrap_or_else(|| format!("europe-pmc:{source_code}:{source_id}")),
            title,
            authors,
            publication_year,
            publication_date,
            venue: item.journal_title.and_then(normalized_nonempty),
            publisher: Some("Europe PMC".to_string()),
            work_type: "journal-article".to_string(),
            cited_by_count: item.cited_by_count.unwrap_or_default(),
            doi,
            url,
            frontier_score: 0,
            rank_reason: String::new(),
            sources: vec![ResearchPaperSource {
                name: EUROPE_PMC_SOURCE.to_string(),
                url: europe_pmc_url,
            }],
            abstract_text,
            highlights: Vec::new(),
        },
    })
}

fn pubmed_candidate(
    uid: &str,
    item: &PubMedSummaryItem,
    from_year: i32,
    to_year: i32,
    relevance: f64,
) -> Option<PaperCandidate> {
    let publication_year = item
        .sort_pub_date
        .as_deref()
        .and_then(extract_year)
        .or_else(|| item.pub_date.as_deref().and_then(extract_year))?;
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let title = item
        .title
        .as_ref()
        .map(|title| clean_text(title))
        .and_then(normalized_nonempty)?;
    let pubmed_url = format!("https://pubmed.ncbi.nlm.nih.gov/{uid}/");
    let doi = item
        .article_ids
        .iter()
        .find(|article_id| article_id.id_type.eq_ignore_ascii_case("doi"))
        .and_then(|article_id| normalized_nonempty(article_id.value.clone()))
        .map(|doi| normalize_doi(&doi));
    let url = doi
        .as_deref()
        .map(doi_url)
        .unwrap_or_else(|| pubmed_url.clone());
    let authors = item
        .authors
        .iter()
        .filter_map(|author| normalized_nonempty(author.name.clone()))
        .take(8)
        .collect::<Vec<_>>();
    let work_type = if item
        .publication_types
        .iter()
        .any(|publication_type| publication_type.to_lowercase().contains("conference"))
    {
        "proceedings-article".to_string()
    } else {
        "journal-article".to_string()
    };

    Some(PaperCandidate {
        relevance,
        paper: ResearchPaper {
            id: doi.clone().unwrap_or_else(|| format!("pubmed:{uid}")),
            title,
            authors,
            publication_year,
            publication_date: item
                .pub_date
                .clone()
                .and_then(normalized_nonempty)
                .or_else(|| Some(publication_year.to_string())),
            venue: item
                .full_journal_name
                .clone()
                .and_then(normalized_nonempty)
                .or_else(|| item.source.clone().and_then(normalized_nonempty)),
            publisher: Some("NCBI".to_string()),
            work_type,
            cited_by_count: 0,
            doi,
            url,
            frontier_score: 0,
            rank_reason: String::new(),
            sources: vec![ResearchPaperSource {
                name: PUBMED_SOURCE.to_string(),
                url: pubmed_url,
            }],
            abstract_text: None,
            highlights: Vec::new(),
        },
    })
}

fn dblp_candidate(
    info: DblpInfo,
    from_year: i32,
    to_year: i32,
    relevance: f64,
) -> Option<PaperCandidate> {
    let publication_year = info.year.as_deref().and_then(extract_year)?;
    if !(from_year..=to_year).contains(&publication_year) {
        return None;
    }

    let title = normalized_nonempty(clean_text(&info.title))?;
    let doi = info
        .doi
        .and_then(normalized_nonempty)
        .map(|doi| normalize_doi(&doi));
    let source_url = info
        .url
        .and_then(valid_http_url)
        .unwrap_or_else(|| format!("https://dblp.org/rec/{}.html", info.key));
    let external_url = info
        .electronic_edition
        .map(OneOrMany::into_vec)
        .unwrap_or_default()
        .into_iter()
        .find_map(DblpText::into_text)
        .and_then(valid_http_url);
    let url = doi
        .as_deref()
        .map(doi_url)
        .or(external_url)
        .unwrap_or_else(|| source_url.clone());
    let authors = info
        .authors
        .map(|authors| authors.authors.into_vec())
        .unwrap_or_default()
        .into_iter()
        .filter_map(DblpText::into_text)
        .filter_map(normalized_nonempty)
        .take(8)
        .collect::<Vec<_>>();
    let work_type = match info
        .publication_type
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        value if value.contains("conference") || value.contains("workshop") => {
            "proceedings-article"
        }
        value if value.contains("book") => "book-chapter",
        _ => "journal-article",
    }
    .to_string();

    Some(PaperCandidate {
        relevance,
        paper: ResearchPaper {
            id: doi.clone().unwrap_or_else(|| format!("dblp:{}", info.key)),
            title,
            authors,
            publication_year,
            publication_date: Some(publication_year.to_string()),
            venue: info.venue.and_then(normalized_nonempty),
            publisher: Some("DBLP".to_string()),
            work_type,
            cited_by_count: 0,
            doi,
            url,
            frontier_score: 0,
            rank_reason: String::new(),
            sources: vec![ResearchPaperSource {
                name: DBLP_SOURCE.to_string(),
                url: source_url,
            }],
            abstract_text: None,
            highlights: Vec::new(),
        },
    })
}

fn merge_and_rank_papers(
    candidates: Vec<PaperCandidate>,
    query: &str,
    from_year: i32,
    to_year: i32,
    limit: usize,
) -> Vec<ResearchPaper> {
    let mut merged: Vec<PaperCandidate> = Vec::new();
    for candidate in candidates {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| papers_match(&existing.paper, &candidate.paper))
        {
            merge_candidate(existing, candidate);
        } else {
            merged.push(candidate);
        }
    }

    let max_citations = merged
        .iter()
        .map(|candidate| candidate.paper.cited_by_count)
        .max()
        .unwrap_or(0);
    let year_span = (to_year - from_year + 1).max(1) as f64;

    for candidate in &mut merged {
        let recency =
            ((candidate.paper.publication_year - from_year + 1) as f64 / year_span).clamp(0.0, 1.0);
        let impact = if max_citations > 0 {
            (candidate.paper.cited_by_count as f64).ln_1p() / (max_citations as f64).ln_1p()
        } else {
            0.0
        };
        let source_confidence = (candidate.paper.sources.len() as f64 / 3.0).clamp(0.0, 1.0);
        let score = candidate.relevance.clamp(0.0, 1.0) * 0.45
            + recency * 0.25
            + impact * 0.15
            + source_confidence * 0.15;

        candidate.paper.frontier_score = (score * 100.0).round() as u32;
        candidate.paper.rank_reason = rank_reason(
            candidate.paper.publication_year,
            to_year,
            candidate.paper.cited_by_count,
            candidate.relevance,
            &candidate.paper.sources,
        );
        candidate.paper.highlights = build_highlights(
            candidate.paper.abstract_text.as_deref(),
            &candidate.paper.title,
            query,
            &candidate.paper.rank_reason,
        );
    }

    merged.sort_by(|left, right| {
        right
            .paper
            .frontier_score
            .cmp(&left.paper.frontier_score)
            .then_with(|| right.paper.sources.len().cmp(&left.paper.sources.len()))
            .then_with(|| {
                right
                    .paper
                    .publication_year
                    .cmp(&left.paper.publication_year)
            })
            .then_with(|| right.paper.cited_by_count.cmp(&left.paper.cited_by_count))
    });

    merged
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.paper)
        .collect()
}

fn papers_match(left: &ResearchPaper, right: &ResearchPaper) -> bool {
    match (&left.doi, &right.doi) {
        (Some(left_doi), Some(right_doi)) if left_doi.eq_ignore_ascii_case(right_doi) => true,
        _ => normalize_title(&left.title) == normalize_title(&right.title),
    }
}

fn merge_candidate(existing: &mut PaperCandidate, mut incoming: PaperCandidate) {
    existing.relevance = existing.relevance.max(incoming.relevance);
    existing.paper.cited_by_count = existing
        .paper
        .cited_by_count
        .max(incoming.paper.cited_by_count);

    if existing.paper.doi.is_none() {
        if let Some(doi) = incoming.paper.doi.take() {
            existing.paper.id = doi.clone();
            existing.paper.url = doi_url(&doi);
            existing.paper.doi = Some(doi);
        }
    }
    if existing.paper.authors.len() < incoming.paper.authors.len() {
        existing.paper.authors = incoming.paper.authors;
    }
    if existing.paper.venue.is_none() {
        existing.paper.venue = incoming.paper.venue;
    }
    if existing.paper.publisher.is_none() {
        existing.paper.publisher = incoming.paper.publisher;
    }
    if existing
        .paper
        .publication_date
        .as_ref()
        .map(|date| date.len())
        .unwrap_or(0)
        < incoming
            .paper
            .publication_date
            .as_ref()
            .map(|date| date.len())
            .unwrap_or(0)
    {
        existing.paper.publication_date = incoming.paper.publication_date;
    }
    if incoming
        .paper
        .abstract_text
        .as_ref()
        .map(|text| text.chars().count())
        .unwrap_or(0)
        > existing
            .paper
            .abstract_text
            .as_ref()
            .map(|text| text.chars().count())
            .unwrap_or(0)
    {
        existing.paper.abstract_text = incoming.paper.abstract_text;
    }

    for source in incoming.paper.sources {
        if !existing
            .paper
            .sources
            .iter()
            .any(|current| current.name == source.name)
        {
            existing.paper.sources.push(source);
        }
    }
}

fn build_highlights(
    abstract_text: Option<&str>,
    title: &str,
    query: &str,
    rank_reason: &str,
) -> Vec<String> {
    let Some(abstract_text) = abstract_text else {
        return fallback_highlights(query, rank_reason);
    };

    let sentences = split_sentences(abstract_text);
    if sentences.is_empty() {
        let highlight = truncate_chars(abstract_text, 180);
        return if normalize_title(&highlight) == normalize_title(title) {
            fallback_highlights(query, rank_reason)
        } else {
            vec![highlight]
        };
    }

    let query_terms = query
        .split(|character: char| {
            character.is_whitespace()
                || matches!(character, ',' | '，' | ';' | '；' | ':' | '：' | '/' | '\\')
        })
        .map(str::trim)
        .filter(|term| term.chars().count() >= 2)
        .map(|term| term.to_lowercase())
        .collect::<HashSet<_>>();
    let signal_terms = [
        "we propose",
        "we present",
        "results show",
        "demonstrate",
        "method",
        "experiment",
        "本文提出",
        "研究提出",
        "结果表明",
        "实验表明",
        "方法",
        "性能",
        "结论",
    ];
    let mut scored = sentences
        .into_iter()
        .enumerate()
        .map(|(index, sentence)| {
            let lower = sentence.to_lowercase();
            let query_score = query_terms
                .iter()
                .filter(|term| lower.contains(term.as_str()))
                .count() as i32
                * 4;
            let signal_score = signal_terms
                .iter()
                .filter(|term| lower.contains(**term))
                .count() as i32
                * 2;
            let position_score = if index == 0 {
                2
            } else if index == 1 {
                1
            } else {
                0
            };
            (index, query_score + signal_score + position_score, sentence)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    scored.retain(|(_, _, sentence)| normalize_title(sentence) != normalize_title(title));
    scored.truncate(2);
    scored.sort_by_key(|item| item.0);
    let highlights = scored
        .into_iter()
        .map(|(_, _, sentence)| truncate_chars(&sentence, 180))
        .filter(|highlight| normalize_title(highlight) != normalize_title(title))
        .collect::<Vec<_>>();
    if highlights.is_empty() {
        fallback_highlights(query, rank_reason)
    } else {
        highlights
    }
}

fn fallback_highlights(query: &str, rank_reason: &str) -> Vec<String> {
    let topic = if query.trim().is_empty() {
        "该研究主题".to_string()
    } else {
        format!("“{}”", truncate_chars(query.trim(), 80))
    };
    vec![
        format!("阅读重点：围绕{topic}，优先核对研究方法、数据来源与评价指标。"),
        format!("筛选依据：{rank_reason}；平台未提供摘要，具体结论请以原文为准。"),
    ]
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        current.push(character);
        if matches!(character, '.' | '!' | '?' | '。' | '！' | '？' | '；')
            && current.chars().count() >= 24
        {
            if let Some(sentence) = normalized_nonempty(current.clone()) {
                sentences.push(sentence);
            }
            current.clear();
        }
    }
    if let Some(sentence) = normalized_nonempty(current) {
        sentences.push(sentence);
    }
    sentences
}

fn rank_reason(
    year: i32,
    current_year: i32,
    citations: u64,
    relevance: f64,
    sources: &[ResearchPaperSource],
) -> String {
    let relevance_percent = (relevance.clamp(0.0, 1.0) * 100.0).round() as u32;
    let age = (current_year - year).max(0);
    let recency = if age == 0 {
        format!("{year} 年发表（当年研究）")
    } else if age == 1 {
        format!("{year} 年发表（近 1 年）")
    } else {
        format!("{year} 年发表（距今 {age} 年）")
    };
    let citation_reason = if citations == 0 {
        "引用：0 次或暂未收录".to_string()
    } else {
        format!("引用：{citations} 次")
    };
    let source_names = sources
        .iter()
        .map(|source| source.name.as_str())
        .collect::<Vec<_>>()
        .join("、");
    let source_reason = if sources.len() >= 2 {
        format!(
            "多平台交叉收录：{} 个论文库（{source_names}）",
            sources.len()
        )
    } else {
        format!("来源论文库：{source_names}")
    };
    let reasons = [
        format!("主题相关度：{relevance_percent}%"),
        recency,
        citation_reason,
        source_reason,
    ];
    reasons.join(" · ")
}

fn normalize_relevance(candidates: &mut [PaperCandidate]) {
    let max_relevance = candidates
        .iter()
        .map(|candidate| candidate.relevance.max(0.0))
        .fold(0.0_f64, f64::max);
    if max_relevance <= 0.0 {
        for (index, candidate) in candidates.iter_mut().enumerate() {
            candidate.relevance = ordered_relevance(index, CANDIDATE_ROWS_PER_SOURCE);
        }
        return;
    }
    for candidate in candidates {
        candidate.relevance = (candidate.relevance.max(0.0) / max_relevance).clamp(0.0, 1.0);
    }
}

fn ordered_relevance(index: usize, total: usize) -> f64 {
    let denominator = total.max(1) as f64;
    (1.0 - index as f64 / denominator * 0.75).clamp(0.25, 1.0)
}

fn semantic_work_type(publication_types: &[String]) -> String {
    let normalized = publication_types
        .iter()
        .map(|value| value.to_lowercase())
        .collect::<Vec<_>>();
    if normalized.iter().any(|value| value.contains("conference")) {
        "proceedings-article".to_string()
    } else if normalized.iter().any(|value| value.contains("preprint")) {
        "posted-content".to_string()
    } else if normalized.iter().any(|value| value.contains("book")) {
        "book-chapter".to_string()
    } else {
        "journal-article".to_string()
    }
}

fn crossref_work_date(item: &CrossrefWork) -> Option<(i32, Option<String>)> {
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

fn crossref_author_name(author: CrossrefAuthor) -> Option<String> {
    if let Some(name) = author.name.and_then(normalized_nonempty) {
        return Some(name);
    }
    let name = [
        author.given.unwrap_or_default(),
        author.family.unwrap_or_default(),
    ]
    .into_iter()
    .filter_map(normalized_nonempty)
    .collect::<Vec<_>>()
    .join(" ");
    normalized_nonempty(name)
}

fn normalize_doi(doi: &str) -> String {
    doi.trim()
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("doi:")
        .trim()
        .to_lowercase()
}

fn doi_url(doi: &str) -> String {
    format!("https://doi.org/{}", normalize_doi(doi))
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalized_nonempty(value: String) -> Option<String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

fn valid_http_url(url: String) -> Option<String> {
    let url = url.trim().to_string();
    (url.starts_with("http://") || url.starts_with("https://")).then_some(url)
}

fn extract_year(value: &str) -> Option<i32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| part.len() >= 4)
        .filter_map(|part| part.get(..4)?.parse::<i32>().ok())
        .find(|year| (1800..=2200).contains(year))
}

fn clean_text(text: &str) -> String {
    let mut without_markup = String::with_capacity(text.len());
    let mut inside_tag = false;
    for character in text.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => without_markup.push(character),
            _ => {}
        }
    }
    without_markup
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
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
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
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

#[derive(Debug, Deserialize)]
struct SemanticScholarEnvelope {
    #[serde(default)]
    total: u64,
    #[serde(default)]
    data: Vec<SemanticScholarPaper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SemanticScholarPaper {
    paper_id: String,
    title: String,
    #[serde(default)]
    authors: Vec<SemanticScholarAuthor>,
    year: Option<i32>,
    publication_date: Option<String>,
    venue: Option<String>,
    publication_types: Option<Vec<String>>,
    citation_count: Option<u64>,
    external_ids: Option<SemanticScholarExternalIds>,
    url: Option<String>,
    #[serde(rename = "abstract")]
    abstract_text: Option<String>,
    open_access_pdf: Option<SemanticScholarOpenAccessPdf>,
}

#[derive(Debug, Deserialize)]
struct SemanticScholarAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SemanticScholarExternalIds {
    #[serde(rename = "DOI")]
    doi: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SemanticScholarOpenAccessPdf {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArxivFeed {
    #[serde(rename = "totalResults", default)]
    total_results: u64,
    #[serde(rename = "entry", default)]
    entries: Vec<ArxivEntry>,
}

#[derive(Debug, Deserialize)]
struct ArxivEntry {
    id: String,
    title: String,
    summary: String,
    published: String,
    #[serde(rename = "author", default)]
    authors: Vec<ArxivAuthor>,
    #[serde(rename = "doi")]
    doi: Option<String>,
    #[serde(rename = "journal_ref")]
    journal_reference: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArxivAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OpenAlexEnvelope {
    meta: OpenAlexMeta,
    #[serde(default)]
    results: Vec<OpenAlexWork>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexMeta {
    #[serde(default)]
    count: u64,
}

#[derive(Debug, Deserialize)]
struct OpenAlexWork {
    doi: Option<String>,
    title: Option<String>,
    display_name: Option<String>,
    publication_year: Option<i32>,
    publication_date: Option<String>,
    #[serde(default)]
    cited_by_count: u64,
    #[serde(default)]
    authorships: Vec<OpenAlexAuthorship>,
    primary_location: Option<OpenAlexLocation>,
    #[serde(default)]
    locations: Vec<OpenAlexLocation>,
    abstract_inverted_index: Option<HashMap<String, Vec<usize>>>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthorship {
    author: OpenAlexAuthor,
}

#[derive(Debug, Deserialize)]
struct OpenAlexAuthor {
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct OpenAlexLocation {
    landing_page_url: Option<String>,
    source: Option<OpenAlexSource>,
}

#[derive(Debug, Deserialize)]
struct OpenAlexSource {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EuropePmcEnvelope {
    #[serde(default)]
    hit_count: u64,
    #[serde(default)]
    result_list: EuropePmcResultList,
}

#[derive(Debug, Default, Deserialize)]
struct EuropePmcResultList {
    #[serde(default, rename = "result")]
    results: Vec<EuropePmcResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EuropePmcResult {
    id: String,
    source: Option<String>,
    title: Option<String>,
    author_string: Option<String>,
    journal_title: Option<String>,
    pub_year: Option<String>,
    first_publication_date: Option<String>,
    cited_by_count: Option<u64>,
    doi: Option<String>,
    abstract_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PubMedSearchEnvelope {
    #[serde(rename = "esearchresult")]
    search_result: PubMedSearchResult,
}

#[derive(Debug, Deserialize)]
struct PubMedSearchResult {
    #[serde(default)]
    count: String,
    #[serde(default, rename = "idlist")]
    id_list: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PubMedSummaryEnvelope {
    result: PubMedSummaryResult,
}

#[derive(Debug, Deserialize)]
struct PubMedSummaryResult {
    #[serde(default)]
    uids: Vec<String>,
    #[serde(flatten)]
    items: HashMap<String, PubMedSummaryItem>,
}

#[derive(Debug, Deserialize)]
struct PubMedSummaryItem {
    title: Option<String>,
    #[serde(rename = "pubdate")]
    pub_date: Option<String>,
    #[serde(rename = "sortpubdate")]
    sort_pub_date: Option<String>,
    source: Option<String>,
    #[serde(rename = "fulljournalname")]
    full_journal_name: Option<String>,
    #[serde(default)]
    authors: Vec<PubMedAuthor>,
    #[serde(default, rename = "articleids")]
    article_ids: Vec<PubMedArticleId>,
    #[serde(default, rename = "pubtype")]
    publication_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PubMedAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PubMedArticleId {
    #[serde(rename = "idtype")]
    id_type: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct DblpEnvelope {
    result: DblpResult,
}

#[derive(Debug, Deserialize)]
struct DblpResult {
    hits: DblpHits,
}

#[derive(Debug, Deserialize)]
struct DblpHits {
    #[serde(default, rename = "@total")]
    total: String,
    #[serde(default, rename = "hit")]
    items: Vec<DblpHit>,
}

#[derive(Debug, Deserialize)]
struct DblpHit {
    info: DblpInfo,
}

#[derive(Debug, Deserialize)]
struct DblpInfo {
    authors: Option<DblpAuthors>,
    title: String,
    venue: Option<String>,
    year: Option<String>,
    #[serde(rename = "type")]
    publication_type: Option<String>,
    key: String,
    doi: Option<String>,
    #[serde(rename = "ee")]
    electronic_edition: Option<OneOrMany<DblpText>>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DblpAuthors {
    #[serde(rename = "author")]
    authors: OneOrMany<DblpText>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DblpText {
    Plain(String),
    Detailed { text: String },
}

impl DblpText {
    fn into_text(self) -> Option<String> {
        match self {
            Self::Plain(text) | Self::Detailed { text } => normalized_nonempty(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_doi_link() {
        assert_eq!(
            doi_url("10.1234/Example"),
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
    fn merges_same_paper_and_preserves_all_sources() {
        let crossref = sample_candidate(
            "A practical paper",
            Some("10.1/same"),
            CROSSREF_SOURCE,
            2025,
            12,
            Some("We propose a practical method. Results show a clear improvement."),
        );
        let semantic = sample_candidate(
            "A practical paper",
            Some("10.1/same"),
            SEMANTIC_SCHOLAR_SOURCE,
            2025,
            20,
            Some("We propose a practical method. Results show a clear improvement."),
        );

        let papers =
            merge_and_rank_papers(vec![crossref, semantic], "practical method", 2022, 2026, 10);

        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].sources.len(), 2);
        assert_eq!(papers[0].cited_by_count, 20);
        assert!(papers[0].rank_reason.contains("多平台交叉收录"));
        assert!(papers[0].rank_reason.contains("主题相关度："));
        assert!(papers[0].rank_reason.contains("引用：20 次"));
        assert!(papers[0].rank_reason.contains("Crossref、Semantic Scholar"));
        assert!(!papers[0].highlights.is_empty());
    }

    #[test]
    fn builds_fallback_highlights_without_abstract() {
        let title = "机器人视觉伺服研究";
        let highlights = build_highlights(None, title, "机器人", "近两年发表");
        assert_eq!(highlights.len(), 2);
        assert!(!highlights.iter().any(|highlight| highlight.contains(title)));
        assert!(highlights[0].contains("研究方法、数据来源与评价指标"));
        assert!(highlights[1].contains("平台未提供摘要"));
    }

    #[test]
    fn removes_highlight_that_repeats_the_paper_title() {
        let title = "A sufficiently long paper title about robot visual control";
        let highlights = build_highlights(
            Some(
                "A sufficiently long paper title about robot visual control. \
                 We propose an adaptive controller that improves tracking accuracy in experiments.",
            ),
            title,
            "robot visual control",
            "主题匹配度高",
        );

        assert!(!highlights
            .iter()
            .any(|highlight| normalize_title(highlight) == normalize_title(title)));
        assert!(highlights
            .iter()
            .any(|highlight| highlight.contains("adaptive controller")));
    }

    #[test]
    fn parses_new_platform_payloads_and_builds_https_source_links() {
        let europe_payload = r#"{
            "hitCount": 1,
            "resultList": {
                "result": [{
                    "id": "12345678",
                    "source": "MED",
                    "title": "Example biomedical paper",
                    "authorString": "Ada Lovelace, Alan Turing",
                    "journalTitle": "Example Journal",
                    "pubYear": "2025",
                    "firstPublicationDate": "2025-04-02",
                    "citedByCount": 7,
                    "doi": "10.1000/example",
                    "abstractText": "We report a useful biomedical result."
                }]
            }
        }"#;
        let europe = serde_json::from_str::<EuropePmcEnvelope>(europe_payload)
            .expect("Europe PMC JSON should parse");
        let europe_candidate = europe_pmc_candidate(
            europe.result_list.results.into_iter().next().unwrap(),
            2024,
            2026,
            0.9,
        )
        .expect("Europe PMC candidate should build");
        assert!(europe_candidate.paper.sources[0]
            .url
            .starts_with("https://europepmc.org/article/"));

        let pubmed_payload = r#"{
            "result": {
                "uids": ["12345678"],
                "12345678": {
                    "title": "Example PubMed paper",
                    "pubdate": "2025 Apr",
                    "sortpubdate": "2025/04/02 00:00",
                    "source": "Example J",
                    "fulljournalname": "Example Journal",
                    "authors": [{"name": "Lovelace A"}],
                    "articleids": [{"idtype": "doi", "value": "10.1000/example"}],
                    "pubtype": ["Journal Article"]
                }
            }
        }"#;
        let pubmed = serde_json::from_str::<PubMedSummaryEnvelope>(pubmed_payload)
            .expect("PubMed JSON should parse");
        let pubmed_item = pubmed.result.items.get("12345678").unwrap();
        let pubmed_candidate = pubmed_candidate("12345678", pubmed_item, 2024, 2026, 0.9)
            .expect("PubMed candidate should build");
        assert_eq!(
            pubmed_candidate.paper.sources[0].url,
            "https://pubmed.ncbi.nlm.nih.gov/12345678/"
        );

        let dblp_payload = r#"{
            "result": {
                "hits": {
                    "@total": "1",
                    "hit": [{
                        "info": {
                            "authors": {
                                "author": [
                                    {"@pid": "1", "text": "Ada Lovelace"},
                                    "Alan Turing"
                                ]
                            },
                            "title": "Example computer science paper",
                            "venue": "ExampleConf",
                            "year": "2025",
                            "type": "Conference and Workshop Papers",
                            "key": "conf/example/paper",
                            "doi": "10.1000/example",
                            "ee": "https://doi.org/10.1000/example",
                            "url": "https://dblp.org/rec/conf/example/paper"
                        }
                    }]
                }
            }
        }"#;
        let dblp =
            serde_json::from_str::<DblpEnvelope>(dblp_payload).expect("DBLP JSON should parse");
        let dblp_candidate = dblp_candidate(
            dblp.result.hits.items.into_iter().next().unwrap().info,
            2024,
            2026,
            0.9,
        )
        .expect("DBLP candidate should build");
        assert!(dblp_candidate.paper.sources[0]
            .url
            .starts_with("https://dblp.org/rec/"));
    }

    #[test]
    fn builds_arxiv_candidate_from_openalex_fallback_payload() {
        let payload = r#"{
            "meta": {"count": 1},
            "results": [{
                "id": "https://openalex.org/W123",
                "doi": "https://doi.org/10.48550/arxiv.2501.00001",
                "title": "Fallback arXiv paper",
                "display_name": "Fallback arXiv paper",
                "publication_year": 2025,
                "publication_date": "2025-01-02",
                "type": "preprint",
                "cited_by_count": 9,
                "authorships": [
                    {"author": {"display_name": "Ada Lovelace"}}
                ],
                "primary_location": {
                    "landing_page_url": "http://arxiv.org/abs/2501.00001",
                    "source": {"display_name": "arXiv (Cornell University)"}
                },
                "locations": [{
                    "landing_page_url": "http://arxiv.org/abs/2501.00001",
                    "source": {"display_name": "arXiv (Cornell University)"}
                }],
                "abstract_inverted_index": {
                    "We": [0],
                    "present": [1],
                    "a": [2],
                    "fallback": [3],
                    "method.": [4]
                }
            }]
        }"#;
        let parsed =
            serde_json::from_str::<OpenAlexEnvelope>(payload).expect("OpenAlex JSON should parse");
        let candidate =
            openalex_arxiv_candidate(parsed.results.into_iter().next().unwrap(), 2024, 2026, 0.9)
                .expect("OpenAlex arXiv candidate should build");

        assert_eq!(candidate.paper.sources[0].name, ARXIV_SOURCE);
        assert_eq!(
            candidate.paper.sources[0].url,
            "https://arxiv.org/abs/2501.00001"
        );
        assert_eq!(candidate.paper.cited_by_count, 9);
        assert!(candidate
            .paper
            .abstract_text
            .as_deref()
            .unwrap_or_default()
            .contains("fallback method"));
    }

    #[test]
    fn parses_arxiv_atom_feed() {
        let feed = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/"
      xmlns:arxiv="http://arxiv.org/schemas/atom">
  <opensearch:totalResults>1</opensearch:totalResults>
  <entry>
    <id>http://arxiv.org/abs/2501.00001v1</id>
    <title>Example robotics paper</title>
    <summary>We propose a robot control method. Results show improved accuracy.</summary>
    <published>2025-01-01T00:00:00Z</published>
    <author><name>Ada Lovelace</name></author>
    <arxiv:doi>10.1000/example</arxiv:doi>
    <arxiv:journal_ref>Robotics Journal</arxiv:journal_ref>
  </entry>
</feed>"#;

        let parsed = quick_xml::de::from_str::<ArxivFeed>(feed).expect("Atom feed should parse");
        assert_eq!(parsed.total_results, 1);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].doi.as_deref(), Some("10.1000/example"));
    }

    fn sample_candidate(
        title: &str,
        doi: Option<&str>,
        source: &str,
        year: i32,
        citations: u64,
        abstract_text: Option<&str>,
    ) -> PaperCandidate {
        let doi = doi.map(ToString::to_string);
        let url = doi
            .as_deref()
            .map(doi_url)
            .unwrap_or_else(|| "https://example.com/paper".to_string());
        PaperCandidate {
            relevance: 0.9,
            paper: ResearchPaper {
                id: doi.clone().unwrap_or_else(|| url.clone()),
                title: title.to_string(),
                authors: vec!["Ada Lovelace".to_string()],
                publication_year: year,
                publication_date: Some(year.to_string()),
                venue: Some("Journal".to_string()),
                publisher: None,
                work_type: "journal-article".to_string(),
                cited_by_count: citations,
                doi,
                url: url.clone(),
                frontier_score: 0,
                rank_reason: String::new(),
                sources: vec![ResearchPaperSource {
                    name: source.to_string(),
                    url,
                }],
                abstract_text: abstract_text.map(ToString::to_string),
                highlights: Vec::new(),
            },
        }
    }
}
