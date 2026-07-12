use crate::services::learning_quiz::{
    LearningQuizGetQuestionsInput, LearningQuizQuestionsResult, LearningQuizScoreInput,
    LearningQuizScoreResult, LearningQuizService,
};

#[tauri::command]
pub async fn learning_quiz_get_questions(
    input: LearningQuizGetQuestionsInput,
) -> Result<LearningQuizQuestionsResult, String> {
    LearningQuizService::get_questions(input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn learning_quiz_score(input: LearningQuizScoreInput) -> Result<LearningQuizScoreResult, String> {
    LearningQuizService::score(input).map_err(|e| e.to_string())
}
