use tauri::{Manager, State};

use crate::database::Database;
use crate::models::session::{
    AppendProjectSessionMessageInput, CreateExecutionLogInput, ExecutionLog, ExecutionPhase,
    ParsedPlan, ProjectSession, ProjectSessionContext, ProjectSessionMessage, TaskSession,
};
use crate::services::session_manager::SessionManagerService;
use crate::services::session_plan::SessionPlanService;

/// 解析计划文件（不创建会话，仅预览）
#[tauri::command]
pub fn parse_plan_file(path: String) -> Result<ParsedPlan, String> {
    SessionPlanService::parse_plan_file(&path).map_err(|e| e.to_string())
}

/// 创建任务执行会话
#[tauri::command]
pub fn create_task_session(
    db: State<'_, Database>,
    plan_path: String,
) -> Result<TaskSession, String> {
    SessionManagerService::create_session(&db, &plan_path).map_err(|e| e.to_string())
}

/// 获取会话详情（含 phases）
#[tauri::command]
pub fn get_task_session(
    db: State<'_, Database>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    let result = SessionManagerService::get_session_with_phases(&db, &session_id)
        .map_err(|e| e.to_string())?;
    match result {
        Some((session, phases)) => Ok(serde_json::json!({
            "session": session,
            "phases": phases,
        })),
        None => Err("会话不存在".into()),
    }
}

/// 列出所有会话
#[tauri::command]
pub fn list_task_sessions(db: State<'_, Database>) -> Result<Vec<TaskSession>, String> {
    db.list_sessions().map_err(|e| e.to_string())
}

/// 开始执行指定 Phase
#[tauri::command]
pub fn start_session_phase(
    db: State<'_, Database>,
    session_id: String,
    phase_index: i32,
) -> Result<ExecutionPhase, String> {
    SessionManagerService::start_phase(&db, &session_id, phase_index).map_err(|e| e.to_string())
}

/// 确认当前 Phase（完成后推进到下一 Phase）
#[tauri::command]
pub fn confirm_session_phase(db: State<'_, Database>, session_id: String) -> Result<(), String> {
    SessionManagerService::confirm_phase(&db, &session_id).map_err(|e| e.to_string())
}

/// 跳过当前 Phase
#[tauri::command]
pub fn skip_session_phase(
    db: State<'_, Database>,
    session_id: String,
    phase_index: i32,
) -> Result<(), String> {
    SessionManagerService::skip_phase(&db, &session_id, phase_index).map_err(|e| e.to_string())
}

/// 重试当前 Phase
#[tauri::command]
pub fn retry_session_phase(
    db: State<'_, Database>,
    session_id: String,
    phase_index: i32,
) -> Result<(), String> {
    SessionManagerService::retry_phase(&db, &session_id, phase_index).map_err(|e| e.to_string())
}

/// 暂停会话
#[tauri::command]
pub fn pause_task_session(db: State<'_, Database>, session_id: String) -> Result<(), String> {
    SessionManagerService::pause_session(&db, &session_id).map_err(|e| e.to_string())
}

/// 恢复会话
#[tauri::command]
pub fn resume_task_session(db: State<'_, Database>, session_id: String) -> Result<(), String> {
    SessionManagerService::resume_session(&db, &session_id).map_err(|e| e.to_string())
}

/// 删除会话
#[tauri::command]
pub fn delete_task_session(db: State<'_, Database>, session_id: String) -> Result<(), String> {
    db.delete_session(&session_id).map_err(|e| e.to_string())
}

/// 添加执行日志
#[tauri::command]
pub fn add_execution_log(
    db: State<'_, Database>,
    log: CreateExecutionLogInput,
) -> Result<(), String> {
    db.add_execution_log(&log).map_err(|e| e.to_string())
}

/// 获取执行日志
#[tauri::command]
pub fn get_execution_logs(
    db: State<'_, Database>,
    session_id: String,
    phase_id: Option<String>,
) -> Result<Vec<ExecutionLog>, String> {
    match phase_id {
        Some(pid) => db.get_logs_by_phase(&pid).map_err(|e| e.to_string()),
        None => db
            .get_logs_by_session(&session_id)
            .map_err(|e| e.to_string()),
    }
}

/// 导出执行日志为 JSON 文件
#[tauri::command]
pub fn export_execution_logs(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    session_id: String,
) -> Result<String, String> {
    let logs = db
        .get_logs_by_session(&session_id)
        .map_err(|e| e.to_string())?;
    let session = db
        .get_session(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or("会话不存在")?;

    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let exports_dir = data_dir.join("exports");
    std::fs::create_dir_all(&exports_dir).map_err(|e| e.to_string())?;

    let filename = format!(
        "session_{}_{}.json",
        &session.plan_name,
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let file_path = exports_dir.join(&filename);

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "session": session,
        "logs": logs,
    }))
    .map_err(|e| e.to_string())?;

    std::fs::write(&file_path, json).map_err(|e| e.to_string())?;

    Ok(file_path.to_string_lossy().to_string())
}

/// 保存粘贴的计划内容为临时 .md 文件（用于 SessionInitModal 粘贴模式）
#[tauri::command]
pub fn save_temp_plan(app: tauri::AppHandle, content: String) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let tmp_dir = data_dir.join("temp_plans");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let filename = format!("plan_{}.md", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let file_path = tmp_dir.join(&filename);
    std::fs::write(&file_path, &content).map_err(|e| e.to_string())?;
    Ok(file_path.to_string_lossy().to_string())
}

// ─── 项目文件夹会话 ─────────────────────────────

/// 打开或恢复项目会话
#[tauri::command]
pub fn open_project_session(
    db: State<'_, Database>,
    project_path: String,
    project_name: Option<String>,
) -> Result<ProjectSession, String> {
    SessionManagerService::open_project_session(&db, &project_path, project_name.as_deref())
        .map_err(|e| e.to_string())
}

/// 列出已打开的项目会话
#[tauri::command]
pub fn list_open_project_sessions(db: State<'_, Database>) -> Result<Vec<ProjectSession>, String> {
    db.list_open_project_sessions().map_err(|e| e.to_string())
}

/// 列出最近的项目会话
#[tauri::command]
pub fn list_recent_project_sessions(
    db: State<'_, Database>,
) -> Result<Vec<ProjectSession>, String> {
    db.list_recent_project_sessions().map_err(|e| e.to_string())
}

/// 设置活跃会话
#[tauri::command]
pub fn set_active_project_session(
    db: State<'_, Database>,
    session_id: String,
) -> Result<(), String> {
    db.set_project_session_active(&session_id)
        .map_err(|e| e.to_string())
}

/// 关闭项目会话 Tab（不删除数据）
#[tauri::command]
pub fn close_project_session(db: State<'_, Database>, session_id: String) -> Result<(), String> {
    SessionManagerService::close_project_session(&db, &session_id).map_err(|e| e.to_string())
}

/// 获取项目会话上下文
#[tauri::command]
pub fn get_project_session_context(
    db: State<'_, Database>,
    session_id: String,
) -> Result<Option<ProjectSessionContext>, String> {
    db.get_project_session_context(&session_id)
        .map_err(|e| e.to_string())
}

/// 追加项目会话消息
#[tauri::command]
pub fn append_project_session_message(
    db: State<'_, Database>,
    input: AppendProjectSessionMessageInput,
) -> Result<ProjectSessionMessage, String> {
    db.create_project_session_message(&input)
        .map_err(|e| e.to_string())
}

/// 列出项目会话消息
#[tauri::command]
pub fn list_project_session_messages(
    db: State<'_, Database>,
    session_id: String,
) -> Result<Vec<ProjectSessionMessage>, String> {
    db.list_project_session_messages(&session_id)
        .map_err(|e| e.to_string())
}
