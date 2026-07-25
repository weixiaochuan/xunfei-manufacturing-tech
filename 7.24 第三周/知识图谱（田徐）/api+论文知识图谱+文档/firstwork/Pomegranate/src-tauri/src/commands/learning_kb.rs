use crate::services::learning_kb::{
    LearningKbSearchInput, LearningKbSearchResult, LearningKbService,
};
use crate::state::AppState;

#[tauri::command]
pub fn learning_kb_search(
    state: tauri::State<'_, AppState>,
    input: LearningKbSearchInput,
) -> Result<LearningKbSearchResult, String> {
    LearningKbService::search(&state.db, input).map_err(|e| e.to_string())
}
