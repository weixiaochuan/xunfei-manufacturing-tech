use serde_json::Value;

use crate::services::course_graph::{
    CourseGraphConfig, CourseGraphHealth, CourseGraphService, CourseGraphStats,
};

#[tauri::command]
pub fn course_graph_get_config(app: tauri::AppHandle) -> Result<CourseGraphConfig, String> {
    CourseGraphService::get_config(&app)
}

#[tauri::command]
pub fn course_graph_health(app: tauri::AppHandle) -> Result<CourseGraphHealth, String> {
    CourseGraphService::health(&app)
}

#[tauri::command]
pub fn course_graph_stats(app: tauri::AppHandle) -> Result<CourseGraphStats, String> {
    CourseGraphService::stats(&app)
}

#[tauri::command]
pub fn course_graph_chapters(app: tauri::AppHandle) -> Result<Value, String> {
    CourseGraphService::chapters(&app)
}

#[tauri::command]
pub fn course_graph_expand(app: tauri::AppHandle, element_id: String) -> Result<Value, String> {
    CourseGraphService::expand(&app, element_id)
}

#[tauri::command]
pub fn course_graph_search(
    app: tauri::AppHandle,
    query: String,
    limit: Option<u32>,
) -> Result<Value, String> {
    CourseGraphService::search(&app, query, limit)
}

#[tauri::command]
pub fn course_graph_node_detail(app: tauri::AppHandle, node_id: String) -> Result<Value, String> {
    CourseGraphService::node_detail(&app, node_id)
}

#[tauri::command]
pub fn course_graph_knowledge(
    app: tauri::AppHandle,
    knowledge_id: String,
) -> Result<Value, String> {
    CourseGraphService::knowledge(&app, knowledge_id)
}

#[tauri::command]
pub fn course_graph_related(app: tauri::AppHandle, node_id: String) -> Result<Value, String> {
    CourseGraphService::related(&app, node_id)
}
