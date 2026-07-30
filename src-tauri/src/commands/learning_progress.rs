use crate::services::learning_progress::{
    LearningProgressClearResult, LearningProgressLoadResult, LearningProgressSaveInput,
    LearningProgressSaveResult, LearningProgressService, LearningProjectCreateInput,
    LearningProjectDeleteInput, LearningProjectDeleteResult, LearningProjectDuplicateInput,
    LearningProjectListResult, LearningProjectLoadInput, LearningProjectLoadResult,
    LearningProjectRenameInput, LearningProjectSaveInput, LearningProjectSaveResult,
};
use crate::state::AppState;

#[tauri::command]
pub fn learning_progress_save_latest(
    state: tauri::State<'_, AppState>,
    input: LearningProgressSaveInput,
) -> Result<LearningProgressSaveResult, String> {
    LearningProgressService::save_latest(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_progress_load_latest(
    state: tauri::State<'_, AppState>,
) -> Result<LearningProgressLoadResult, String> {
    LearningProgressService::load_latest(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_progress_clear_latest(
    state: tauri::State<'_, AppState>,
) -> Result<LearningProgressClearResult, String> {
    LearningProgressService::clear_latest(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_project_list(
    state: tauri::State<'_, AppState>,
) -> Result<LearningProjectListResult, String> {
    LearningProgressService::list_projects(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_project_create(
    state: tauri::State<'_, AppState>,
    input: LearningProjectCreateInput,
) -> Result<LearningProjectSaveResult, String> {
    LearningProgressService::create_project(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_project_load(
    state: tauri::State<'_, AppState>,
    input: LearningProjectLoadInput,
) -> Result<LearningProjectLoadResult, String> {
    LearningProgressService::load_project(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_project_save(
    state: tauri::State<'_, AppState>,
    input: LearningProjectSaveInput,
) -> Result<LearningProjectSaveResult, String> {
    LearningProgressService::save_project(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_project_rename(
    state: tauri::State<'_, AppState>,
    input: LearningProjectRenameInput,
) -> Result<LearningProjectSaveResult, String> {
    LearningProgressService::rename_project(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_project_delete(
    state: tauri::State<'_, AppState>,
    input: LearningProjectDeleteInput,
) -> Result<LearningProjectDeleteResult, String> {
    LearningProgressService::delete_project(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_project_duplicate(
    state: tauri::State<'_, AppState>,
    input: LearningProjectDuplicateInput,
) -> Result<LearningProjectSaveResult, String> {
    LearningProgressService::duplicate_project(&state.db, input).map_err(|e| e.to_string())
}
