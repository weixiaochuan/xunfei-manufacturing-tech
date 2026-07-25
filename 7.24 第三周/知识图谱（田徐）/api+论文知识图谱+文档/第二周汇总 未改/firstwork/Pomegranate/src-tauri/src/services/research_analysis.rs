use std::collections::{HashMap, HashSet};
use std::path::Path;

use tokio::sync::watch;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    PluginAiChatInput, PluginAiMessage, ResearchAnalysisInput, ResearchAnalysisResult,
    ResearchGraphEdge, ResearchGraphNode, ResearchKeywordOverlap,
};

use super::ai::AiService;
use super::pdf::PdfService;

const MAX_PAPERS: usize = 5;
const MAX_CHARS_PER_PAPER: usize = 32_000;

pub struct ResearchAnalysisService;

impl ResearchAnalysisService {
    pub async fn analyze(
        db: &Database,
        input: ResearchAnalysisInput,
    ) -> Result<ResearchAnalysisResult, AppError> {
        validate_input(&input)?;
        let mut paper_sections = Vec::new();
        let mut warnings = Vec::new();
        for (index, path_text) in input.file_paths.iter().enumerate() {
            let path = Path::new(path_text);
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("paper.pdf")
                .to_string();
            let extracted = PdfService::extract_text_only(path)?;
            if extracted.trim().is_empty() {
                return Err(AppError::Custom(format!("论文《{file_name}》没有可提取的文字层")));
            }
            let total_chars = extracted.chars().count();
            let content: String = extracted.chars().take(MAX_CHARS_PER_PAPER).collect();
            if total_chars > MAX_CHARS_PER_PAPER {
                warnings.push(format!(
                    "《{file_name}》正文较长，本次分析截取前 {MAX_CHARS_PER_PAPER} 个字符"
                ));
            }
            paper_sections.push(format!(
                "\n## PAPER_{}\n文件名：{}\n正文：\n{}",
                index + 1,
                file_name,
                content
            ));
        }

        let model = db.get_default_ai_model()?;
        if !model.provider.eq_ignore_ascii_case("deepseek") {
            return Err(AppError::InvalidInput(
                "论文分析当前只使用 DeepSeek。请到“设置 → AI 模型”添加 DeepSeek（API 地址 https://api.deepseek.com，模型 deepseek-v4-flash）并设为默认模型。".into(),
            ));
        }
        let system_prompt = r#"你是严谨的科研论文比较分析器。根据用户提供的多篇论文正文和当前项目背景，逐篇提取标题、摘要、关键词、研究问题、方法、数据/实验、指标、结论、创新与局限，再进行跨论文比较并给当前项目可执行建议。摘要应忠实概括论文，不得虚构；keywords 优先提取论文作者给出的关键词，未明确列出时可从摘要和正文归纳 3～8 个规范化关键词。必须重点识别多篇论文的共同关键词：共同关键词不只列出名称，还要分析各论文在该主题下的方法、实验和结论有何一致、差异或可组合之处，并把结论写入 keywordOverlaps.analysis 和 comparisons。所有判断必须来自给定正文；证据 quote 应短小并标注章节线索，无法确定页码时 location 写“PDF文字层，页码不可用”。只返回 JSON，不要 Markdown。graphNodes 和 graphEdges 应形成“当前项目—论文—关键词—方法/结论/局限”的知识图谱。关系只能使用 SIMILAR_TO、SUPPORTS、CONTRADICTS、EXTENDS、USES_METHOD、HAS_KEYWORD、HAS_CONCLUSION、HAS_LIMITATION、APPLICABLE_TO_PROJECT。"#;
        let user_prompt = format!(
            r#"当前项目背景：
{}

论文正文：
{}

严格返回：
{{
  "projectSummary":"对当前项目的简要理解",
  "papers":[{{"paperId":"PAPER_1","fileName":"原文件名","title":"论文标题","abstractText":"论文摘要或忠实概括","keywords":["关键词1","关键词2"],"researchQuestion":"研究问题","methods":[],"dataAndExperiments":[],"metrics":[],"conclusions":[],"innovations":[],"limitations":[],"evidence":[{{"paperId":"PAPER_1","quote":"短证据","location":"章节或位置"}}]}}],
  "keywordOverlaps":[{{"keyword":"共同关键词","paperIds":["PAPER_1","PAPER_2"],"analysis":"围绕该关键词重点比较方法、实验、结论和可组合点"}}],
  "comparisons":[{{"dimension":"比较维度","commonPoints":[],"differences":[],"conflicts":[]}}],
  "recommendations":[{{"title":"建议标题","action":"具体怎么改","rationale":"为什么","supportingPaperIds":["PAPER_1"],"confidence":0.8}}],
  "graphNodes":[{{"id":"project","label":"当前项目","nodeType":"Project","paperId":null}}],
  "graphEdges":[{{"id":"edge-1","source":"PAPER_1","target":"project","relationType":"APPLICABLE_TO_PROJECT","reason":"理由"}}],
  "warnings":[]
}}"#,
            input.project_context.trim(),
            paper_sections.join("\n")
        );
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let raw = AiService::plugin_chat_sync(
            db,
            PluginAiChatInput {
                messages: vec![
                    PluginAiMessage { role: "system".into(), content: system_prompt.into() },
                    PluginAiMessage { role: "user".into(), content: user_prompt },
                ],
                request_id: format!("research-analysis-{}", chrono::Utc::now().timestamp_millis()),
                model_id: Some(model.id),
            },
            cancel_rx,
        )
        .await?;
        let mut result: ResearchAnalysisResult = parse_json_object(&raw)?;
        result.model_id = model.id;
        result.warnings.extend(warnings);
        normalize_result(&mut result, input.file_paths.len());
        Ok(result)
    }
}

fn validate_input(input: &ResearchAnalysisInput) -> Result<(), AppError> {
    if !(2..=MAX_PAPERS).contains(&input.file_paths.len()) {
        return Err(AppError::InvalidInput(format!("请上传 2 到 {MAX_PAPERS} 篇 PDF 论文")));
    }
    if input.project_context.trim().chars().count() < 20 {
        return Err(AppError::InvalidInput("请至少用 20 个字描述当前项目方向、方法或困难".into()));
    }
    for path_text in &input.file_paths {
        let path = Path::new(path_text);
        if !path.exists() {
            return Err(AppError::NotFound(format!("论文文件不存在：{path_text}")));
        }
        let is_pdf = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false);
        if !is_pdf {
            return Err(AppError::InvalidInput(format!("目前仅支持 PDF：{path_text}")));
        }
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
    let start = raw.find('{').ok_or_else(|| AppError::Custom("AI 未返回 JSON 对象".into()))?;
    let end = raw.rfind('}').ok_or_else(|| AppError::Custom("AI 返回的 JSON 不完整".into()))?;
    serde_json::from_str(&raw[start..=end])
        .map_err(|e| AppError::Custom(format!("论文分析结果无法解析：{e}")))
}

fn normalize_result(result: &mut ResearchAnalysisResult, expected_papers: usize) {
    let allowed_paper_ids: HashSet<String> = (1..=expected_papers)
        .map(|index| format!("PAPER_{index}"))
        .collect();
    result.papers.retain(|paper| allowed_paper_ids.contains(&paper.paper_id));
    for paper in &mut result.papers {
        normalize_keywords(&mut paper.keywords);
    }
    result.keyword_overlaps = build_keyword_overlaps(&result.papers, &result.keyword_overlaps);
    for recommendation in &mut result.recommendations {
        recommendation.confidence = recommendation.confidence.clamp(0.0, 1.0);
        recommendation.supporting_paper_ids.retain(|id| allowed_paper_ids.contains(id));
    }
    enrich_keyword_graph(result);
    let node_ids: HashSet<String> = result.graph_nodes.iter().map(|node| node.id.clone()).collect();
    result.graph_edges.retain(|edge| {
        node_ids.contains(&edge.source) && node_ids.contains(&edge.target)
    });
}

fn normalize_keywords(keywords: &mut Vec<String>) {
    let mut seen = HashSet::new();
    keywords.retain_mut(|keyword| {
        *keyword = keyword.trim().to_string();
        !keyword.is_empty() && seen.insert(keyword.to_lowercase())
    });
    keywords.truncate(8);
}

fn build_keyword_overlaps(
    papers: &[crate::models::ResearchPaperAnalysis],
    ai_overlaps: &[ResearchKeywordOverlap],
) -> Vec<ResearchKeywordOverlap> {
    let mut occurrences: HashMap<String, (String, Vec<String>)> = HashMap::new();
    for paper in papers {
        for keyword in &paper.keywords {
            let key = keyword.to_lowercase();
            let entry = occurrences.entry(key).or_insert_with(|| (keyword.clone(), Vec::new()));
            if !entry.1.contains(&paper.paper_id) {
                entry.1.push(paper.paper_id.clone());
            }
        }
    }

    let ai_analysis: HashMap<String, String> = ai_overlaps
        .iter()
        .map(|item| (item.keyword.trim().to_lowercase(), item.analysis.trim().to_string()))
        .collect();
    let mut overlaps: Vec<ResearchKeywordOverlap> = occurrences
        .into_iter()
        .filter_map(|(key, (keyword, paper_ids))| {
            (paper_ids.len() >= 2).then(|| ResearchKeywordOverlap {
                keyword,
                analysis: ai_analysis.get(&key).cloned().filter(|text| !text.is_empty()).unwrap_or_else(|| {
                    format!("{} 均涉及该关键词，建议重点对比相关方法、实验设置与结论。", paper_ids.join("、"))
                }),
                paper_ids,
            })
        })
        .collect();
    overlaps.sort_by(|left, right| {
        right.paper_ids.len().cmp(&left.paper_ids.len()).then_with(|| left.keyword.cmp(&right.keyword))
    });
    overlaps
}

fn enrich_keyword_graph(result: &mut ResearchAnalysisResult) {
    let mut node_ids: HashSet<String> = result.graph_nodes.iter().map(|node| node.id.clone()).collect();
    let mut edge_ids: HashSet<String> = result.graph_edges.iter().map(|edge| edge.id.clone()).collect();
    for (index, overlap) in result.keyword_overlaps.iter().enumerate() {
        let keyword_node_id = format!("shared-keyword-{}", index + 1);
        if node_ids.insert(keyword_node_id.clone()) {
            result.graph_nodes.push(ResearchGraphNode {
                id: keyword_node_id.clone(),
                label: overlap.keyword.clone(),
                node_type: "Keyword".into(),
                paper_id: None,
            });
        }
        for paper_id in &overlap.paper_ids {
            if !node_ids.contains(paper_id) {
                node_ids.insert(paper_id.clone());
                result.graph_nodes.push(ResearchGraphNode {
                    id: paper_id.clone(),
                    label: paper_id.clone(),
                    node_type: "Paper".into(),
                    paper_id: Some(paper_id.clone()),
                });
            }
            let edge_id = format!("keyword-edge-{}-{}", index + 1, paper_id.to_lowercase());
            if edge_ids.insert(edge_id.clone()) {
                result.graph_edges.push(ResearchGraphEdge {
                    id: edge_id,
                    source: paper_id.clone(),
                    target: keyword_node_id.clone(),
                    relation_type: "HAS_KEYWORD".into(),
                    reason: overlap.analysis.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_inside_fence() {
        let raw = r#"```json
        {"projectSummary":"x","papers":[],"keywordOverlaps":[],"comparisons":[],"recommendations":[],"graphNodes":[],"graphEdges":[],"warnings":[],"modelId":0}
        ```"#;
        let result: ResearchAnalysisResult = parse_json_object(raw).expect("应能解析 JSON");
        assert_eq!(result.project_summary, "x");
    }

    #[test]
    fn shared_keywords_are_normalized_and_added_to_graph() {
        let raw = r#"{
          "projectSummary":"x",
          "papers":[
            {"paperId":"PAPER_1","fileName":"a.pdf","title":"A","abstractText":"摘要A","keywords":["智能制造","故障诊断"],"researchQuestion":"q","methods":[],"dataAndExperiments":[],"metrics":[],"conclusions":[],"innovations":[],"limitations":[],"evidence":[]},
            {"paperId":"PAPER_2","fileName":"b.pdf","title":"B","abstractText":"摘要B","keywords":[" 智能制造 ","视觉"],"researchQuestion":"q","methods":[],"dataAndExperiments":[],"metrics":[],"conclusions":[],"innovations":[],"limitations":[],"evidence":[]}
          ],
          "keywordOverlaps":[{"keyword":"智能制造","paperIds":["PAPER_1","PAPER_2"],"analysis":"共同主题的深入比较"}],
          "comparisons":[],"recommendations":[],
          "graphNodes":[],"graphEdges":[],"warnings":[],"modelId":0
        }"#;
        let mut result: ResearchAnalysisResult = parse_json_object(raw).expect("应能解析 JSON");
        normalize_result(&mut result, 2);
        assert_eq!(result.keyword_overlaps.len(), 1);
        assert_eq!(result.keyword_overlaps[0].analysis, "共同主题的深入比较");
        assert!(result.graph_nodes.iter().any(|node| node.node_type == "Keyword"));
        assert_eq!(result.graph_edges.iter().filter(|edge| edge.relation_type == "HAS_KEYWORD").count(), 2);
    }
}
