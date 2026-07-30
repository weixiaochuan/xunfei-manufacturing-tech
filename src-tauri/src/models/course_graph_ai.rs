use serde::{Deserialize, Serialize};

/// AI 对一个课程知识点的结构化解释。
///
/// `source_kind` 与 `source_revision` 现在指向内置课程库；后续切换为侧边栏文档时，
/// 同一套分析与审核链路可以继续复用，不需要把 AI 逻辑绑定到某一种知识来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseGraphAiAnalysis {
    pub node_id: String,
    pub node_name: String,
    pub source_kind: String,
    pub source_revision: String,
    pub definition: String,
    pub summary: String,
    pub aliases: Vec<String>,
    pub prerequisites: Vec<String>,
    pub applications: Vec<String>,
    pub misconceptions: Vec<String>,
    pub model_id: i64,
    pub relations: Vec<CourseGraphAiRelation>,
    pub created_at: String,
    pub updated_at: String,
}

/// AI 推断的知识点关系。关系默认进入 pending，必须经用户确认后才能作为正式增强边展示。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseGraphAiRelation {
    pub id: i64,
    pub source_node_id: String,
    pub source_node_name: String,
    pub target_node_id: String,
    pub target_node_name: String,
    pub relation_type: String,
    pub reason: String,
    pub confidence: f64,
    pub status: String,
    pub source_kind: String,
    pub source_revision: String,
    pub model_id: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCourseGraphAiRelationInput {
    pub relation_id: i64,
    pub status: String,
}

/// 仅用于解析模型返回；节点名与来源信息均由后端可信数据补齐。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseGraphAiModelOutput {
    pub definition: String,
    pub summary: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub applications: Vec<String>,
    #[serde(default)]
    pub misconceptions: Vec<String>,
    #[serde(default)]
    pub relations: Vec<CourseGraphAiModelRelation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseGraphAiModelRelation {
    pub target_node_id: String,
    pub relation_type: String,
    pub reason: String,
    pub confidence: f64,
}
