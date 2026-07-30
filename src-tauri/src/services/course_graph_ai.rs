use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::sync::watch;

use crate::database::Database;
use crate::error::AppError;
use crate::models::course_graph_ai::{CourseGraphAiModelOutput, CourseGraphAiRelation};
use crate::models::{CourseGraphAiAnalysis, PluginAiChatInput, PluginAiMessage};

use super::ai::AiService;
use super::course_graph::CourseGraphService;

const ALLOWED_RELATION_TYPES: &[&str] = &[
    "PREREQUISITE_OF",
    "PART_OF",
    "SIMILAR_TO",
    "CONTRASTS_WITH",
    "APPLIED_IN",
    "DERIVED_FROM",
    "AI_RELATED_TO",
];

pub struct CourseGraphAiService;

impl CourseGraphAiService {
    pub async fn analyze(
        app: &AppHandle,
        db: &Database,
        node_id: String,
    ) -> Result<CourseGraphAiAnalysis, AppError> {
        let context = CourseGraphService::analysis_context(app, node_id.clone(), Some(16))
            .map_err(AppError::Custom)?;
        let node = context
            .get("node")
            .and_then(Value::as_object)
            .ok_or_else(|| AppError::Custom("课程知识点上下文缺少 node".into()))?;
        let node_name = node
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let node_content = node
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if node_name.is_empty() {
            return Err(AppError::InvalidInput("知识点名称为空".into()));
        }

        let candidates = context
            .get("candidates")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let candidate_names: HashMap<String, String> = candidates
            .iter()
            .filter_map(|item| {
                Some((
                    item.get("id")?.as_str()?.to_string(),
                    item.get("name")?.as_str()?.to_string(),
                ))
            })
            .collect();
        let candidate_text = candidates
            .iter()
            .filter_map(|item| {
                Some(format!(
                    "- id={}；名称={}；内容={}",
                    item.get("id")?.as_str()?,
                    item.get("name")?.as_str()?,
                    item.get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                ))
            })
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = r#"你是机械制造工艺课程的知识图谱分析器。请基于给定原文解释知识点，并从候选知识点中判断有教学价值的关系。不得编造候选列表以外的 targetNodeId。只返回一个 JSON 对象，不要 Markdown 代码块或解释。关系类型只能使用：PREREQUISITE_OF、PART_OF、SIMILAR_TO、CONTRASTS_WITH、APPLIED_IN、DERIVED_FROM、AI_RELATED_TO。confidence 必须是 0 到 1 的数字。如果证据不足，relations 返回空数组。"#;
        let user_prompt = format!(
            r#"当前知识点：
id={node_id}
名称={node_name}
原文={node_content}

候选知识点：
{candidate_text}

返回格式：
{{
  "definition": "准确的具体含义",
  "summary": "一句话概括",
  "aliases": ["别名或近义说法"],
  "prerequisites": ["理解它所需的前置知识"],
  "applications": ["典型应用"],
  "misconceptions": ["常见误区"],
  "relations": [
    {{"targetNodeId":"候选 id","relationType":"关系类型","reason":"判断依据","confidence":0.85}}
  ]
}}"#,
        );

        let model = db.get_default_ai_model()?;
        if !model.provider.eq_ignore_ascii_case("deepseek") {
            return Err(AppError::InvalidInput(
                "知识图谱 AI 分析当前只使用 DeepSeek。请到“设置 → AI 模型”添加 DeepSeek（API 地址 https://api.deepseek.com，模型 deepseek-v4-flash）并设为默认模型。".into(),
            ));
        }
        let request_id = format!(
            "course-graph-ai-{}-{}",
            node_id,
            chrono::Utc::now().timestamp_millis()
        );
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let raw = AiService::plugin_chat_sync(
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
                request_id,
                model_id: Some(model.id),
            },
            cancel_rx,
        )
        .await?;
        let parsed = parse_model_output(&raw)?;
        let source_kind = context
            .get("sourceKind")
            .and_then(Value::as_str)
            .unwrap_or("bundled-course-graph")
            .to_string();
        let source_revision = context
            .get("sourceRevision")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let mut seen = HashSet::new();
        let relations = parsed
            .relations
            .into_iter()
            .filter_map(|relation| {
                let relation_type = relation.relation_type.trim().to_ascii_uppercase();
                let target_name = candidate_names.get(&relation.target_node_id)?.clone();
                if !ALLOWED_RELATION_TYPES.contains(&relation_type.as_str())
                    || relation.reason.trim().is_empty()
                    || !seen.insert((relation.target_node_id.clone(), relation_type.clone()))
                {
                    return None;
                }
                Some(CourseGraphAiRelation {
                    id: 0,
                    source_node_id: node_id.clone(),
                    source_node_name: node_name.clone(),
                    target_node_id: relation.target_node_id,
                    target_node_name: target_name,
                    relation_type,
                    reason: relation.reason.trim().to_string(),
                    confidence: relation.confidence.clamp(0.0, 1.0),
                    status: "pending".into(),
                    source_kind: source_kind.clone(),
                    source_revision: source_revision.clone(),
                    model_id: model.id,
                    created_at: String::new(),
                    updated_at: String::new(),
                })
            })
            .collect();

        let analysis = CourseGraphAiAnalysis {
            node_id,
            node_name,
            source_kind,
            source_revision,
            definition: require_text(parsed.definition, "definition")?,
            summary: require_text(parsed.summary, "summary")?,
            aliases: clean_list(parsed.aliases),
            prerequisites: clean_list(parsed.prerequisites),
            applications: clean_list(parsed.applications),
            misconceptions: clean_list(parsed.misconceptions),
            model_id: model.id,
            relations,
            created_at: String::new(),
            updated_at: String::new(),
        };
        db.save_course_graph_ai_analysis(&analysis, &raw)
    }

    pub fn get(db: &Database, node_id: &str) -> Result<Option<CourseGraphAiAnalysis>, AppError> {
        db.get_course_graph_ai_analysis(node_id, "bundled-course-graph")
    }

    pub fn review_relation(
        db: &Database,
        relation_id: i64,
        status: &str,
    ) -> Result<CourseGraphAiRelation, AppError> {
        if !matches!(status, "accepted" | "rejected" | "pending") {
            return Err(AppError::InvalidInput(
                "关系状态只能是 pending、accepted 或 rejected".into(),
            ));
        }
        db.review_course_graph_ai_relation(relation_id, status)
    }

    pub fn accepted_graph(
        app: &AppHandle,
        db: &Database,
        node_id: &str,
    ) -> Result<Value, AppError> {
        let relations = db.list_course_graph_ai_relations(node_id, Some("accepted"))?;
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        if let Ok(source) = CourseGraphService::node_detail(app, node_id.to_string()) {
            nodes.push(source);
        }
        for relation in relations {
            if let Ok(target) =
                CourseGraphService::node_detail(app, relation.target_node_id.clone())
            {
                nodes.push(target);
                edges.push(json!({
                    "id": format!("ai:{}", relation.id),
                    "elementId": format!("ai:{}", relation.id),
                    "source": relation.source_node_id,
                    "target": relation.target_node_id,
                    "startNodeElementId": relation.source_node_id,
                    "endNodeElementId": relation.target_node_id,
                    "type": relation.relation_type,
                    "metadata": {
                        "aiGenerated": true,
                        "confidence": relation.confidence,
                        "reason": relation.reason,
                        "status": relation.status,
                    }
                }));
            }
        }
        Ok(json!({ "nodes": nodes, "relationships": edges }))
    }
}

fn parse_model_output(raw: &str) -> Result<CourseGraphAiModelOutput, AppError> {
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
        .map_err(|e| AppError::Custom(format!("AI 分析结果无法解析：{e}")))
}

fn require_text(value: String, field: &str) -> Result<String, AppError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(AppError::Custom(format!("AI 分析结果缺少 {field}")))
    } else {
        Ok(value)
    }
}

fn clean_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .take(12)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_json() {
        let raw = r#"```json
        {"definition":"定义","summary":"概括","aliases":[],"prerequisites":[],"applications":[],"misconceptions":[],"relations":[]}
        ```"#;
        let parsed = parse_model_output(raw).expect("应能解析 fenced JSON");
        assert_eq!(parsed.definition, "定义");
    }
}
