use crate::services::learning_resources::{
    LearningResourcesRecommendInput, LearningResourcesRecommendResult, LearningResourcesService,
};

#[tauri::command]
pub async fn learning_resources_recommend(
    input: LearningResourcesRecommendInput,
) -> Result<LearningResourcesRecommendResult, String> {
    LearningResourcesService::recommend(input)
        .await
        .map_err(|e| e.to_string())
}
