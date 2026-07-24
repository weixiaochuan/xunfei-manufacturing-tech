use crate::services::learning_kb::{
    LearningKbInventory, LearningKbSearchInput, LearningKbSearchResult, LearningKbService,
};

#[tauri::command]
pub fn learning_kb_inventory(app: tauri::AppHandle) -> Result<LearningKbInventory, String> {
    LearningKbService::inventory(&app)
}

#[tauri::command]
pub fn learning_kb_search(
    app: tauri::AppHandle,
    input: LearningKbSearchInput,
) -> Result<LearningKbSearchResult, String> {
    LearningKbService::search(&app, input)
}
