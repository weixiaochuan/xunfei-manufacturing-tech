use crate::services::learning_assistant::{
    LearningAssistantCheckInput, LearningAssistantCheckResult, LearningAssistantPlanInput,
    LearningAssistantPlanResult, LearningAssistantService,
};

#[tauri::command]
pub fn learning_assistant_check(
    app: tauri::AppHandle,
    input: LearningAssistantCheckInput,
) -> Result<LearningAssistantCheckResult, String> {
    LearningAssistantService::check(&app, input)
}

#[tauri::command]
pub fn learning_assistant_understand(
    input: LearningAssistantPlanInput,
) -> Result<LearningAssistantPlanResult, String> {
    LearningAssistantService::understand(input)
}

#[tauri::command]
pub fn learning_assistant_generate_plan(
    app: tauri::AppHandle,
    input: LearningAssistantPlanInput,
) -> Result<LearningAssistantPlanResult, String> {
    LearningAssistantService::generate_plan(&app, input)
}
