use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchAnalysisInput {
    pub file_paths: Vec<String>,
    pub project_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchPaperAnalysis {
    pub paper_id: String,
    pub file_name: String,
    pub title: String,
    #[serde(default)]
    pub abstract_text: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub research_question: String,
    pub methods: Vec<String>,
    pub data_and_experiments: Vec<String>,
    pub metrics: Vec<String>,
    pub conclusions: Vec<String>,
    pub innovations: Vec<String>,
    pub limitations: Vec<String>,
    pub evidence: Vec<ResearchEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchKeywordOverlap {
    pub keyword: String,
    pub paper_ids: Vec<String>,
    #[serde(default)]
    pub analysis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEvidence {
    pub paper_id: String,
    pub quote: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchComparison {
    pub dimension: String,
    pub common_points: Vec<String>,
    pub differences: Vec<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchProjectRecommendation {
    pub title: String,
    pub action: String,
    pub rationale: String,
    pub supporting_paper_ids: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGraphNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub paper_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchGraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub relation_type: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchAnalysisResult {
    pub project_summary: String,
    pub papers: Vec<ResearchPaperAnalysis>,
    #[serde(default)]
    pub keyword_overlaps: Vec<ResearchKeywordOverlap>,
    pub comparisons: Vec<ResearchComparison>,
    pub recommendations: Vec<ResearchProjectRecommendation>,
    pub graph_nodes: Vec<ResearchGraphNode>,
    pub graph_edges: Vec<ResearchGraphEdge>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub model_id: i64,
}
