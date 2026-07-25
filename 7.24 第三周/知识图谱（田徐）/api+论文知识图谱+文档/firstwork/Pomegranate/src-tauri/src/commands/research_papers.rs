use crate::models::{
    ResearchPaperKnowledgeRecommendation, ResearchPaperKnowledgeRecommendationInput,
    ResearchPaperSearchInput, ResearchPaperSearchResult,
};
use crate::services::research_papers::ResearchPaperService;
use crate::services::research_recommendation::ResearchRecommendationService;
use crate::state::AppState;

/// Search recent research papers for the AI research assistant.
#[tauri::command]
pub async fn research_search_papers(
    input: ResearchPaperSearchInput,
) -> Result<ResearchPaperSearchResult, String> {
    ResearchPaperService::search(input)
        .await
        .map_err(|error| error.to_string())
}

/// Ask AI whether a paper is worth adding to the knowledge base.
///
/// This command is advisory only and never writes the paper to the database.
#[tauri::command]
pub async fn research_recommend_for_knowledge_base(
    state: tauri::State<'_, AppState>,
    input: ResearchPaperKnowledgeRecommendationInput,
) -> Result<ResearchPaperKnowledgeRecommendation, String> {
    ResearchRecommendationService::recommend(&state.db, input)
        .await
        .map_err(|error| error.to_string())
}
