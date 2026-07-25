//! Claude Code Agent Runner — Command 层（IPC 入口）
//!
//! 薄包装：接收前端参数 → 调用 Service → 转换错误。

use tauri::{AppHandle, State};

use crate::models::{ClaudeAgentEvent, ClaudeAgentSession, StartClaudeAgentInput};
use crate::services::claude_agent;
use crate::state::AppState;

/// 检测 Claude Code CLI 是否可用
#[tauri::command]
pub async fn claude_agent_check_cli() -> Result<String, String> {
    claude_agent::check_cli().await.map_err(|e| e.to_string())
}

/// 启动 Agent 会话
#[tauri::command]
pub async fn start_claude_agent_session(
    app: AppHandle,
    state: State<'_, AppState>,
    input: StartClaudeAgentInput,
) -> Result<ClaudeAgentSession, String> {
    claude_agent::start_session(
        &app,
        &state.db,
        input,
        state.data_dir.clone(),
        state.claude_agent_processes.clone(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// 停止 Agent 会话
#[tauri::command]
pub async fn stop_claude_agent_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    claude_agent::stop_session(state.claude_agent_processes.clone(), &session_id)
        .await
        .map_err(|e| e.to_string())
}

/// 列出 Agent 会话
#[tauri::command]
pub fn list_claude_agent_sessions(
    state: State<'_, AppState>,
    project_path: Option<String>,
) -> Result<Vec<ClaudeAgentSession>, String> {
    state
        .db
        .list_agent_sessions(project_path.as_deref())
        .map_err(|e| e.to_string())
}

/// 获取 Agent 会话详情
#[tauri::command]
pub fn get_claude_agent_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<ClaudeAgentSession, String> {
    state
        .db
        .get_agent_session(&session_id)
        .map_err(|e| e.to_string())
}

/// 列出 Agent 事件
#[tauri::command]
pub fn list_claude_agent_events(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<ClaudeAgentEvent>, String> {
    state
        .db
        .list_agent_events(&session_id)
        .map_err(|e| e.to_string())
}
