use tauri::State;

use crate::models::{
    CourseGraphAiAnalysis, CourseGraphAiRelation, ReviewCourseGraphAiRelationInput,
};
use crate::services::course_graph_ai::CourseGraphAiService;
use crate::state::AppState;

#[tauri::command]
pub async fn course_graph_ai_analyze(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> Result<CourseGraphAiAnalysis, String> {
    CourseGraphAiService::analyze(&app, &state.db, node_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn course_graph_ai_get(
    state: State<'_, AppState>,
    node_id: String,
) -> Result<Option<CourseGraphAiAnalysis>, String> {
    CourseGraphAiService::get(&state.db, &node_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn course_graph_ai_review_relation(
    state: State<'_, AppState>,
    input: ReviewCourseGraphAiRelationInput,
) -> Result<CourseGraphAiRelation, String> {
    CourseGraphAiService::review_relation(&state.db, input.relation_id, &input.status)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn course_graph_ai_accepted_graph(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> Result<serde_json::Value, String> {
    CourseGraphAiService::accepted_graph(&app, &state.db, &node_id).map_err(|e| e.to_string())
}
