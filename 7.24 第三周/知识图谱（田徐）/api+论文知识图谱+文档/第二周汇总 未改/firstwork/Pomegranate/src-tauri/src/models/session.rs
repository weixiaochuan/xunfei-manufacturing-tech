use serde::{Deserialize, Serialize};

// ─── 任务执行会话 ─────────────────────────────────

/// 会话状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Running,
    WaitingConfirm,
    Paused,
    Completed,
}

/// Phase 状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    Pending,
    Running,
    WaitingConfirm,
    Completed,
    Skipped,
    Failed,
}

/// 任务执行会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSession {
    pub id: String,
    pub plan_path: String,
    pub plan_name: String,
    pub status: SessionStatus,
    pub current_phase_index: i32,
    pub total_phases: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// 执行阶段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPhase {
    pub id: String,
    pub session_id: String,
    pub index_num: i32,
    pub name: String,
    pub description: String,
    pub status: PhaseStatus,
    pub files_modified: Option<String>,
    pub result_summary: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// 执行日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLog {
    pub id: i64,
    pub session_id: String,
    pub phase_id: Option<String>,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

/// 创建日志入参
#[derive(Debug, Clone, Deserialize)]
pub struct CreateExecutionLogInput {
    pub session_id: String,
    pub phase_id: Option<String>,
    pub level: String,
    pub message: String,
}

/// 计划文件解析结果（不存库，仅用于传输）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedPlan {
    pub name: String,
    pub phases: Vec<PhaseDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseDraft {
    pub id: String,
    pub name: String,
    pub description: String,
}

// ─── 项目文件夹会话 ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSession {
    pub id: String,
    pub project_name: String,
    pub project_path: String,
    pub status: String,
    pub git_branch: Option<String>,
    pub is_open: bool,
    pub last_active_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSessionContext {
    pub session_id: String,
    pub project_path: String,
    pub git_branch: Option<String>,
    pub changed_files: Vec<String>,
    pub pinned_files: Vec<String>,
    pub recent_files: Vec<String>,
    pub current_task: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSessionMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendProjectSessionMessageInput {
    pub session_id: String,
    pub role: String,
    pub content: String,
}
