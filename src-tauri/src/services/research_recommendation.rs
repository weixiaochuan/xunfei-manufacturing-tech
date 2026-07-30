use std::collections::HashSet;

use chrono::Datelike;
use serde::Deserialize;
use tokio::sync::watch;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    PluginAiChatInput, PluginAiMessage, ResearchPaperKnowledgeRecommendation,
    ResearchPaperKnowledgeRecommendationInput,
};

use super::ai::AiService;

const MAX_REASON_CHARS: usize = 1_000;
const MAX_TAGS: usize = 5;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiRecommendationPayload {
    decision: String,
    reason: String,
    confidence: f64,
    #[serde(default)]
    suggested_tags: Vec<String>,
}

pub struct ResearchRecommendationService;

impl ResearchRecommendationService {
    pub async fn recommend(
        db: &Database,
        input: ResearchPaperKnowledgeRecommendationInput,
    ) -> Result<ResearchPaperKnowledgeRecommendation, AppError> {
        validate_input(&input)?;
        let model = match db.get_default_ai_model() {
            Ok(model) => model,
            Err(_) => return Ok(local_fallback_recommendation(&input, None)),
        };
        let paper = &input.paper;
        let source_names = paper
            .sources
            .iter()
            .map(|source| source.name.as_str())
            .collect::<Vec<_>>()
            .join("、");
        let abstract_text = paper
            .abstract_text
            .as_deref()
            .unwrap_or("未收录摘要，只能依据题录信息进行保守判断");
        let system_prompt = r#"你是科研知识库的论文入库顾问。你的任务只是提供建议，不能代替用户决定，也不能声称已经把论文加入知识库。

请从以下方面综合判断：
1. 主题匹配：说明与检索主题的具体交集，不要只说“相关”；
2. 研究价值：依据摘要说明方法、数据、结论可能具有的复用、引用或对比价值；摘要缺失时必须明确无法判断；
3. 可靠性与完整度：结合年份、引用量、出版载体、多论文库交叉收录、DOI、作者和摘要完整度判断；
4. 风险与核验建议：指出预印本、引用尚未积累、元数据缺失、仅凭摘要无法验证结论等风险，并告诉用户入库前应核验什么。

只返回一个 JSON 对象，不要返回 Markdown。decision 只能是 recommended、consider、not_recommended。reason 必须用中文按“主题匹配：”“研究价值：”“可靠性与完整度：”“风险与核验建议：”“综合结论：”五行输出，每项必须包含当前论文的具体证据，总长度控制在 300 到 600 个汉字；JSON 字符串中的换行使用 \n。无法从题录或摘要确认的内容必须明确写“无法确认”，不得臆测。confidence 是 0 到 1。suggestedTags 最多 5 个，不得包含论文标题。"#;
        let user_prompt = format!(
            r#"检索主题：{}

论文题录：
- 标题：{}
- 作者：{}
- 发表日期：{}
- 出版载体：{}
- 类型：{}
- 引用量：{}
- DOI：{}
- 论文库来源：{}
- 系统筛选依据：{}
- 摘要：{}

严格返回：
{{"decision":"recommended|consider|not_recommended","reason":"主题匹配：具体证据\n研究价值：具体证据\n可靠性与完整度：具体证据\n风险与核验建议：具体建议\n综合结论：明确结论","confidence":0.0,"suggestedTags":["标签"]}}"#,
            input.query.trim(),
            paper.title.trim(),
            if paper.authors.is_empty() {
                "未收录".to_string()
            } else {
                paper.authors.join("、")
            },
            paper
                .publication_date
                .clone()
                .unwrap_or_else(|| paper.publication_year.to_string()),
            paper
                .venue
                .as_deref()
                .or(paper.publisher.as_deref())
                .unwrap_or("未收录"),
            paper.work_type,
            paper.cited_by_count,
            paper.doi.as_deref().unwrap_or("未收录"),
            if source_names.is_empty() {
                "未收录"
            } else {
                source_names.as_str()
            },
            paper.rank_reason,
            abstract_text,
        );
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let raw = match AiService::plugin_chat_sync(
            db,
            PluginAiChatInput {
                messages: vec![
                    PluginAiMessage {
                        role: "system".into(),
                        content: system_prompt.into(),
                    },
                    PluginAiMessage {
                        role: "user".into(),
                        content: user_prompt,
                    },
                ],
                request_id: format!(
                    "research-kb-recommendation-{}",
                    chrono::Utc::now().timestamp_millis()
                ),
                model_id: Some(model.id),
            },
            cancel_rx,
        )
        .await
        {
            Ok(raw) => raw,
            Err(_) => return Ok(local_fallback_recommendation(&input, Some(model.id))),
        };

        match parse_json_object::<AiRecommendationPayload>(&raw) {
            Ok(payload) => Ok(normalize_recommendation(payload, model.id)),
            Err(_) => Ok(local_fallback_recommendation(&input, Some(model.id))),
        }
    }
}

fn validate_input(input: &ResearchPaperKnowledgeRecommendationInput) -> Result<(), AppError> {
    if input.query.trim().is_empty() {
        return Err(AppError::InvalidInput("缺少本次论文检索主题".into()));
    }
    if input.paper.title.trim().is_empty() {
        return Err(AppError::InvalidInput("论文标题不能为空".into()));
    }
    Ok(())
}

fn parse_json_object<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, AppError> {
    let trimmed = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }
    let start = raw
        .find('{')
        .ok_or_else(|| AppError::Custom("AI 未返回 JSON 对象".into()))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| AppError::Custom("AI 返回的 JSON 不完整".into()))?;
    serde_json::from_str(&raw[start..=end])
        .map_err(|error| AppError::Custom(format!("AI 入库建议无法解析：{error}")))
}

fn normalize_recommendation(
    payload: AiRecommendationPayload,
    model_id: i64,
) -> ResearchPaperKnowledgeRecommendation {
    let decision = match payload.decision.trim().to_ascii_lowercase().as_str() {
        "recommended" => "recommended",
        "not_recommended" | "not-recommended" => "not_recommended",
        _ => "consider",
    }
    .to_string();
    let reason = truncate_chars(payload.reason.trim(), MAX_REASON_CHARS);
    let reason = if reason.is_empty() {
        "AI 未给出充分理由，建议用户阅读摘要和方法后再决定。".to_string()
    } else {
        reason
    };
    let mut seen = HashSet::new();
    let mut suggested_tags = Vec::new();
    for tag in payload.suggested_tags {
        let normalized = truncate_chars(tag.trim(), 24);
        let key = normalized.to_lowercase();
        if !normalized.is_empty() && seen.insert(key) {
            suggested_tags.push(normalized);
        }
        if suggested_tags.len() >= MAX_TAGS {
            break;
        }
    }
    ResearchPaperKnowledgeRecommendation {
        decision,
        reason,
        confidence: if payload.confidence.is_finite() {
            payload.confidence.clamp(0.0, 1.0)
        } else {
            0.0
        },
        suggested_tags,
        evaluation_mode: "ai".into(),
        warning: None,
        model_id,
    }
}

fn local_fallback_recommendation(
    input: &ResearchPaperKnowledgeRecommendationInput,
    model_id: Option<i64>,
) -> ResearchPaperKnowledgeRecommendation {
    let paper = &input.paper;
    let current_year = chrono::Utc::now().year();
    let age = (current_year - paper.publication_year).max(0);
    let mut adjusted_score = paper.frontier_score as f64;

    if paper
        .abstract_text
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        adjusted_score += 5.0;
    } else {
        adjusted_score -= 8.0;
    }
    if paper
        .doi
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        adjusted_score += 3.0;
    }
    if paper.sources.len() >= 2 {
        adjusted_score += 5.0;
    }
    if paper.cited_by_count >= 20 {
        adjusted_score += 3.0;
    }
    if age > 4 {
        adjusted_score -= 5.0;
    }
    adjusted_score = adjusted_score.clamp(0.0, 100.0);

    let (decision, conclusion) = if adjusted_score >= 78.0 {
        ("recommended", "建议加入，便于后续引用和对比")
    } else if adjusted_score >= 55.0 {
        ("consider", "建议先核验摘要、方法和数据后再决定")
    } else {
        ("not_recommended", "当前证据不足，暂不建议加入")
    };
    let has_abstract = paper
        .abstract_text
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let source_names = paper
        .sources
        .iter()
        .map(|source| source.name.as_str())
        .collect::<Vec<_>>()
        .join("、");
    let theme_evidence = if paper.rank_reason.trim().is_empty() {
        format!(
            "系统前沿分为 {}，但没有更细的关键词匹配说明，需要打开原文确认与“{}”的实际关联。",
            paper.frontier_score,
            input.query.trim()
        )
    } else {
        truncate_chars(paper.rank_reason.trim(), 220)
    };
    let research_value = if has_abstract {
        format!(
            "已收录摘要，并生成 {} 条阅读提示，可用于初步判断研究问题、方法和适用边界；但本地规则不会理解全文语义，不能确认方法创新性、实验质量或结论是否成立。",
            paper.highlights.len()
        )
    } else {
        "摘要缺失，当前只能依据题录信息判断，无法确认研究问题、方法、数据、实验设计和结论是否具有复用价值。".to_string()
    };
    let annualized_citations = paper.cited_by_count as f64 / (age.max(0) as f64 + 1.0);
    let impact_signal = if age <= 1 && paper.cited_by_count < 10 {
        "发表时间较近，引用尚未充分积累，低引用量不能直接视为低质量"
    } else if paper.cited_by_count >= 50 {
        "已有较明显的学术关注度，但引用量不等同于研究质量"
    } else if paper.cited_by_count == 0 {
        "暂未发现引用记录，需要重点核验出版状态和研究质量"
    } else {
        "已有一定引用记录，仍需结合领域规模和论文年龄理解"
    };
    let mut present_metadata = Vec::new();
    let mut missing_metadata = Vec::new();
    if paper.authors.is_empty() {
        missing_metadata.push("作者");
    } else {
        present_metadata.push("作者");
    }
    if has_abstract {
        present_metadata.push("摘要");
    } else {
        missing_metadata.push("摘要");
    }
    if paper
        .doi
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        present_metadata.push("DOI");
    } else {
        missing_metadata.push("DOI");
    }
    if paper
        .venue
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || paper
            .publisher
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        present_metadata.push("出版载体");
    } else {
        missing_metadata.push("出版载体");
    }
    let metadata_evidence = format!(
        "发表于 {} 年（距今约 {} 年），累计引用 {} 次、折合约 {:.1} 次/年，{}。题录 4 项中已具备 {} 项（{}）{}。",
        paper.publication_year,
        age,
        paper.cited_by_count,
        annualized_citations,
        impact_signal,
        present_metadata.len(),
        present_metadata.join("、"),
        if missing_metadata.is_empty() {
            "，核心元数据较完整".to_string()
        } else {
            format!("，仍缺少 {}", missing_metadata.join("、"))
        }
    );
    let source_evidence = if paper.sources.len() >= 2 {
        format!(
            "由 {} 个论文库交叉收录（{}），同一题录获得多来源印证；但跨库收录只能提高题录可信度，不能替代同行评议或全文核验。",
            paper.sources.len(),
            source_names
        )
    } else {
        format!(
            "目前只在 {} 个论文库中收录（{}），缺少跨库交叉印证，应进一步核对原始出版页面。",
            paper.sources.len(),
            if source_names.is_empty() {
                "来源未记录"
            } else {
                source_names.as_str()
            }
        )
    };
    let mut risks = vec!["本地规则未读取论文全文，无法验证方法、实验和结论".to_string()];
    if paper.work_type.eq_ignore_ascii_case("posted-content")
        || paper.work_type.to_ascii_lowercase().contains("preprint")
    {
        risks.push("该记录可能是预印本，需要确认是否经过同行评议及是否已有正式版本".into());
    }
    if !has_abstract {
        risks.push("缺少摘要，无法进行内容层面的初筛".into());
    }
    if paper.sources.len() <= 1 {
        risks.push("只有单一论文库来源，需要核对出版页面".into());
    }
    if paper
        .doi
        .as_deref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        risks.push("缺少 DOI，后续去重和版本追踪可能不稳定".into());
    }
    let reason = format!(
        "主题匹配：{}\n研究价值：{}\n可靠性与完整度：{}\n来源交叉验证：{}\n风险与核验建议：{}；入库前建议打开原文，优先核验研究问题、样本或数据来源、评价指标、基线对比、局限性和最终出版状态。\n综合结论：综合校正分 {:.0}/100，{}。",
        theme_evidence,
        research_value,
        metadata_evidence,
        source_evidence,
        risks.join("；"),
        adjusted_score,
        conclusion
    );
    let completeness = [
        !paper.authors.is_empty(),
        has_abstract,
        paper
            .doi
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        paper
            .venue
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || paper
                .publisher
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
    ]
    .into_iter()
    .filter(|value| *value)
    .count() as f64;
    let confidence =
        (0.5 + completeness * 0.06 + (paper.sources.len().min(3) as f64) * 0.03).clamp(0.0, 0.82);

    ResearchPaperKnowledgeRecommendation {
        decision: decision.into(),
        reason,
        confidence,
        suggested_tags: local_suggested_tags(&input.query),
        evaluation_mode: "local_fallback".into(),
        warning: Some(
            "默认 AI 模型当前不可用，已改用可解释的本地规则评估。本结果仅依据检索主题、系统相关度、发表年份、引用量、题录完整度和跨库收录情况，不会理解论文全文，也不能判断方法创新性、实验质量或结论真实性；请把它作为初筛依据，并在最终入库前核验原文。".into(),
        ),
        model_id: model_id.unwrap_or(0),
    }
}

fn local_suggested_tags(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    query
        .split(|character: char| character.is_whitespace() || "，,；;、/|".contains(character))
        .map(str::trim)
        .filter(|tag| (2..=24).contains(&tag.chars().count()))
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .take(MAX_TAGS)
        .map(str::to_string)
        .collect()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_ai_json() {
        let raw = r#"```json
{"decision":"recommended","reason":"主题高度相关，方法可复用。","confidence":0.87,"suggestedTags":["知识图谱","RAG"]}
```"#;
        let parsed: AiRecommendationPayload = parse_json_object(raw).expect("JSON 应可解析");
        let recommendation = normalize_recommendation(parsed, 7);
        assert_eq!(recommendation.decision, "recommended");
        assert_eq!(recommendation.model_id, 7);
        assert_eq!(recommendation.suggested_tags, vec!["知识图谱", "RAG"]);
        assert_eq!(recommendation.evaluation_mode, "ai");
        assert!(recommendation.warning.is_none());
    }

    #[test]
    fn normalizes_untrusted_ai_fields() {
        let payload = AiRecommendationPayload {
            decision: "unknown".into(),
            reason: "  ".into(),
            confidence: 2.4,
            suggested_tags: vec!["AI".into(), "ai".into(), " ".into(), "论文".into()],
        };
        let recommendation = normalize_recommendation(payload, 3);
        assert_eq!(recommendation.decision, "consider");
        assert_eq!(recommendation.confidence, 1.0);
        assert_eq!(recommendation.suggested_tags, vec!["AI", "论文"]);
        assert!(!recommendation.reason.is_empty());
    }

    #[test]
    fn falls_back_to_local_recommendation_when_ai_is_unavailable() {
        let input = ResearchPaperKnowledgeRecommendationInput {
            query: "知识图谱 RAG".into(),
            paper: crate::models::ResearchPaper {
                id: "paper-1".into(),
                title: "A useful paper".into(),
                authors: vec!["Alice".into()],
                publication_year: chrono::Utc::now().year(),
                publication_date: None,
                venue: Some("Test Journal".into()),
                publisher: None,
                work_type: "journal-article".into(),
                cited_by_count: 30,
                doi: Some("10.1000/test".into()),
                url: "https://example.com/paper".into(),
                frontier_score: 82,
                rank_reason: "与主题高度相关".into(),
                sources: vec![
                    crate::models::ResearchPaperSource {
                        name: "Crossref".into(),
                        url: "https://example.com/crossref".into(),
                    },
                    crate::models::ResearchPaperSource {
                        name: "DBLP".into(),
                        url: "https://example.com/dblp".into(),
                    },
                ],
                abstract_text: Some("摘要".into()),
                highlights: vec![],
            },
        };
        let recommendation = local_fallback_recommendation(&input, Some(1));
        assert_eq!(recommendation.decision, "recommended");
        assert_eq!(recommendation.evaluation_mode, "local_fallback");
        assert!(recommendation.warning.is_some());
        assert_eq!(recommendation.suggested_tags, vec!["知识图谱", "RAG"]);
        for section in [
            "主题匹配：",
            "研究价值：",
            "可靠性与完整度：",
            "来源交叉验证：",
            "风险与核验建议：",
            "综合结论：",
        ] {
            assert!(
                recommendation.reason.contains(section),
                "评估理由缺少分项：{section}"
            );
        }
        let warning = recommendation.warning.expect("本地兜底必须说明局限");
        assert!(warning.contains("不会理解论文全文"));
        assert!(warning.contains("最终入库前核验原文"));
    }
}
