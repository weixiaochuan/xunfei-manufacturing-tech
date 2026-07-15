use crate::services::learning_assistant::{
    LearningAssistantCheckInput, LearningAssistantCheckResult, LearningAssistantPlanInput,
    LearningAssistantPlanResult, LearningAssistantService,
};

#[tauri::command]
pub fn learning_assistant_check(
    input: LearningAssistantCheckInput,
) -> Result<LearningAssistantCheckResult, String> {
    LearningAssistantService::check(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_assistant_understand(
    input: LearningAssistantPlanInput,
) -> Result<LearningAssistantPlanResult, String> {
    LearningAssistantService::understand(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_assistant_generate_plan(
    input: LearningAssistantPlanInput,
) -> Result<LearningAssistantPlanResult, String> {
    LearningAssistantService::generate_plan(input).map_err(|e| e.to_string())
}
