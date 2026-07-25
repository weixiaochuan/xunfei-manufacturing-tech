use tauri::State;

use crate::models::{ResearchAnalysisInput, ResearchAnalysisResult};
use crate::services::research_analysis::ResearchAnalysisService;
use crate::state::AppState;

#[tauri::command]
pub async fn research_analyze_papers(
    state: State<'_, AppState>,
    input: ResearchAnalysisInput,
) -> Result<ResearchAnalysisResult, String> {
    ResearchAnalysisService::analyze(&state.db, input)
        .await
        .map_err(|e| e.to_string())
}
