use crate::services::learning_assistant::{
    LearningAssistantAiConfigInput, LearningAssistantAiConfigStatus, LearningAssistantCheckInput,
    LearningAssistantCheckResult, LearningAssistantPlanInput, LearningAssistantPlanResult,
    LearningAssistantService, LearningPlanAdjustInput, LearningPlanAdjustResult,
};
use crate::state::AppState;

#[tauri::command]
pub fn learning_assistant_check(
    input: LearningAssistantCheckInput,
) -> Result<LearningAssistantCheckResult, String> {
    LearningAssistantService::check(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_assistant_ai_get_config(
    state: tauri::State<'_, AppState>,
) -> Result<LearningAssistantAiConfigStatus, String> {
    LearningAssistantService::get_ai_config(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_assistant_ai_save_config(
    state: tauri::State<'_, AppState>,
    input: LearningAssistantAiConfigInput,
) -> Result<LearningAssistantAiConfigStatus, String> {
    LearningAssistantService::save_ai_config(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_assistant_ai_clear_config(
    state: tauri::State<'_, AppState>,
) -> Result<LearningAssistantAiConfigStatus, String> {
    LearningAssistantService::clear_ai_config(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn learning_assistant_understand(
    state: tauri::State<'_, AppState>,
    input: LearningAssistantPlanInput,
) -> Result<LearningAssistantPlanResult, String> {
    LearningAssistantService::understand(&state.db, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn learning_assistant_generate_plan(
    state: tauri::State<'_, AppState>,
    input: LearningAssistantPlanInput,
) -> Result<LearningAssistantPlanResult, String> {
    LearningAssistantService::generate_plan(&state.db, &state.data_dir, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn learning_assistant_adjust_plan(
    state: tauri::State<'_, AppState>,
    input: LearningPlanAdjustInput,
) -> Result<LearningPlanAdjustResult, String> {
    LearningAssistantService::adjust_plan(&state.db, input)
        .await
        .map_err(|e| e.to_string())
}
