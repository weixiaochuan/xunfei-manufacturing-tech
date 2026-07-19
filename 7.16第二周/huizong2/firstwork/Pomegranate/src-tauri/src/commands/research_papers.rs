use crate::models::{ResearchPaperSearchInput, ResearchPaperSearchResult};
use crate::services::research_papers::ResearchPaperService;

/// Search recent research papers for the AI research assistant.
#[tauri::command]
pub async fn research_search_papers(
    input: ResearchPaperSearchInput,
) -> Result<ResearchPaperSearchResult, String> {
    ResearchPaperService::search(input)
        .await
        .map_err(|error| error.to_string())
}
