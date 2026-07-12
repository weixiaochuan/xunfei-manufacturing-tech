use rusqlite::params;

use crate::database::Database;
use crate::error::AppError;
use crate::models::session::{
    AppendProjectSessionMessageInput, CreateExecutionLogInput, ExecutionLog, ExecutionPhase,
    ProjectSession, ProjectSessionContext, ProjectSessionMessage, TaskSession,
};

impl Database {
    // ─── ProjectSession ────────────────────────

    pub fn upsert_project_session(&self, session: &ProjectSession) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "INSERT INTO project_sessions
                (id, project_name, project_path, status, git_branch, is_open, last_active_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(project_path) DO UPDATE SET
                project_name = excluded.project_name,
                status = excluded.status,
                git_branch = excluded.git_branch,
                is_open = excluded.is_open,
                last_active_at = excluded.last_active_at",
            params![
                session.id,
                session.project_name,
                session.project_path,
                session.status,
                session.git_branch,
                if session.is_open { 1 } else { 0 },
                session.last_active_at,
                session.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_project_session_by_path(
        &self,
        project_path: &str,
    ) -> Result<Option<ProjectSession>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, project_name, project_path, status, git_branch, is_open, last_active_at, created_at
             FROM project_sessions WHERE project_path = ?1",
        )?;
        let result = stmt.query_row(params![project_path], map_project_session).ok();
        Ok(result)
    }

    pub fn get_project_session_by_id(
        &self,
        session_id: &str,
    ) -> Result<Option<ProjectSession>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, project_name, project_path, status, git_branch, is_open, last_active_at, created_at
             FROM project_sessions WHERE id = ?1",
        )?;
        let result = stmt.query_row(params![session_id], map_project_session).ok();
        Ok(result)
    }

    pub fn list_open_project_sessions(&self) -> Result<Vec<ProjectSession>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, project_name, project_path, status, git_branch, is_open, last_active_at, created_at
             FROM project_sessions WHERE is_open = 1 ORDER BY last_active_at DESC",
        )?;
        let sessions = stmt
            .query_map([], map_project_session)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn list_recent_project_sessions(&self) -> Result<Vec<ProjectSession>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, project_name, project_path, status, git_branch, is_open, last_active_at, created_at
             FROM project_sessions ORDER BY last_active_at DESC LIMIT 20",
        )?;
        let sessions = stmt
            .query_map([], map_project_session)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn set_project_session_active(&self, session_id: &str) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "UPDATE project_sessions
             SET is_open = 1, status = 'active', last_active_at = datetime('now', 'localtime')
             WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub fn close_project_session(&self, session_id: &str) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "UPDATE project_sessions SET is_open = 0, status = 'idle', last_active_at = datetime('now', 'localtime') WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub fn upsert_project_session_context(
        &self,
        context: &ProjectSessionContext,
    ) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "INSERT INTO project_session_contexts
                (session_id, project_path, git_branch, changed_files_json, pinned_files_json, recent_files_json, current_task, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(session_id) DO UPDATE SET
                project_path = excluded.project_path,
                git_branch = excluded.git_branch,
                changed_files_json = excluded.changed_files_json,
                pinned_files_json = excluded.pinned_files_json,
                recent_files_json = excluded.recent_files_json,
                current_task = excluded.current_task,
                updated_at = excluded.updated_at",
            params![
                context.session_id,
                context.project_path,
                context.git_branch,
                serde_json::to_string(&context.changed_files)?,
                serde_json::to_string(&context.pinned_files)?,
                serde_json::to_string(&context.recent_files)?,
                context.current_task,
                context.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_project_session_context(
        &self,
        session_id: &str,
    ) -> Result<Option<ProjectSessionContext>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT session_id, project_path, git_branch, changed_files_json, pinned_files_json, recent_files_json, current_task, updated_at
             FROM project_session_contexts WHERE session_id = ?1",
        )?;
        let result = stmt
            .query_row(params![session_id], |row| {
                Ok(ProjectSessionContext {
                    session_id: row.get(0)?,
                    project_path: row.get(1)?,
                    git_branch: row.get(2)?,
                    changed_files: parse_json_vec(row.get::<_, String>(3)?),
                    pinned_files: parse_json_vec(row.get::<_, String>(4)?),
                    recent_files: parse_json_vec(row.get::<_, String>(5)?),
                    current_task: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .ok();
        Ok(result)
    }

    pub fn insert_project_session_message(
        &self,
        message: &ProjectSessionMessage,
    ) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "INSERT INTO project_session_messages (id, session_id, role, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                message.id,
                message.session_id,
                message.role,
                message.content,
                message.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_project_session_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<ProjectSessionMessage>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, created_at
             FROM project_session_messages WHERE session_id = ?1 ORDER BY created_at ASC, id ASC",
        )?;
        let messages = stmt
            .query_map(params![session_id], |row| {
                Ok(ProjectSessionMessage {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn create_project_session_message(
        &self,
        input: &AppendProjectSessionMessageInput,
    ) -> Result<ProjectSessionMessage, AppError> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let message = ProjectSessionMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: input.session_id.clone(),
            role: input.role.clone(),
            content: input.content.clone(),
            created_at: now,
        };
        self.insert_project_session_message(&message)?;
        Ok(message)
    }

    // ─── TaskSession ───────────────────────────

    pub fn create_session(&self, session: &TaskSession) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "INSERT INTO task_sessions (id, plan_path, plan_name, status, current_phase_index, total_phases, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session.id,
                session.plan_path,
                session.plan_name,
                format_session_status(&session.status),
                session.current_phase_index,
                session.total_phases,
                session.created_at,
                session.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> Result<Option<TaskSession>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, plan_path, plan_name, status, current_phase_index, total_phases, created_at, updated_at
             FROM task_sessions WHERE id = ?1",
        )?;
        let result = stmt
            .query_row(params![id], |row| {
                Ok(TaskSession {
                    id: row.get(0)?,
                    plan_path: row.get(1)?,
                    plan_name: row.get(2)?,
                    status: parse_session_status(&row.get::<_, String>(3)?),
                    current_phase_index: row.get(4)?,
                    total_phases: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .ok();
        Ok(result)
    }

    pub fn list_sessions(&self) -> Result<Vec<TaskSession>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, plan_path, plan_name, status, current_phase_index, total_phases, created_at, updated_at
             FROM task_sessions ORDER BY updated_at DESC",
        )?;
        let sessions = stmt
            .query_map([], |row| {
                Ok(TaskSession {
                    id: row.get(0)?,
                    plan_path: row.get(1)?,
                    plan_name: row.get(2)?,
                    status: parse_session_status(&row.get::<_, String>(3)?),
                    current_phase_index: row.get(4)?,
                    total_phases: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn update_session_status(
        &self,
        id: &str,
        status: &str,
        current_phase_index: i32,
    ) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "UPDATE task_sessions SET status = ?1, current_phase_index = ?2, updated_at = datetime('now', 'localtime') WHERE id = ?3",
            params![status, current_phase_index, id],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: &str) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute("DELETE FROM execution_logs WHERE session_id = ?1", params![id])?;
        conn.execute("DELETE FROM execution_phases WHERE session_id = ?1", params![id])?;
        conn.execute("DELETE FROM task_sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ─── ExecutionPhase ────────────────────────

    pub fn create_phases_batch(&self, phases: &[ExecutionPhase]) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        for p in phases {
            conn.execute(
                "INSERT INTO execution_phases (id, session_id, index_num, name, description, status, files_modified, result_summary, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    p.id,
                    p.session_id,
                    p.index_num,
                    p.name,
                    p.description,
                    format_phase_status(&p.status),
                    p.files_modified,
                    p.result_summary,
                    p.started_at,
                    p.finished_at,
                ],
            )?;
        }
        Ok(())
    }

    pub fn get_phases_by_session(&self, session_id: &str) -> Result<Vec<ExecutionPhase>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, index_num, name, description, status, files_modified, result_summary, started_at, finished_at
             FROM execution_phases WHERE session_id = ?1 ORDER BY index_num ASC",
        )?;
        let phases = stmt
            .query_map(params![session_id], |row| {
                Ok(ExecutionPhase {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    index_num: row.get(2)?,
                    name: row.get(3)?,
                    description: row.get(4)?,
                    status: parse_phase_status(&row.get::<_, String>(5)?),
                    files_modified: row.get(6)?,
                    result_summary: row.get(7)?,
                    started_at: row.get(8)?,
                    finished_at: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(phases)
    }

    pub fn update_phase_status(
        &self,
        id: &str,
        status: &str,
        result_summary: Option<&str>,
        files_modified: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "UPDATE execution_phases SET status = ?1, result_summary = ?2, files_modified = ?3 WHERE id = ?4",
            params![status, result_summary, files_modified, id],
        )?;
        Ok(())
    }

    pub fn update_phase_timestamps(
        &self,
        id: &str,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "UPDATE execution_phases SET started_at = ?1, finished_at = ?2 WHERE id = ?3",
            params![started_at, finished_at, id],
        )?;
        Ok(())
    }

    // ─── ExecutionLog ──────────────────────────

    pub fn add_execution_log(&self, log: &CreateExecutionLogInput) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "INSERT INTO execution_logs (session_id, phase_id, level, message, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now', 'localtime'))",
            params![log.session_id, log.phase_id, log.level, log.message],
        )?;
        Ok(())
    }

    pub fn get_logs_by_session(&self, session_id: &str) -> Result<Vec<ExecutionLog>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, phase_id, level, message, created_at
             FROM execution_logs WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let logs = stmt
            .query_map(params![session_id], |row| {
                Ok(ExecutionLog {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    phase_id: row.get(2)?,
                    level: row.get(3)?,
                    message: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    pub fn get_logs_by_phase(&self, phase_id: &str) -> Result<Vec<ExecutionLog>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, phase_id, level, message, created_at
             FROM execution_logs WHERE phase_id = ?1 ORDER BY id ASC",
        )?;
        let logs = stmt
            .query_map(params![phase_id], |row| {
                Ok(ExecutionLog {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    phase_id: row.get(2)?,
                    level: row.get(3)?,
                    message: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }
}

// ─── 辅助函数 ─────────────────────────────────

fn map_project_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSession> {
    Ok(ProjectSession {
        id: row.get(0)?,
        project_name: row.get(1)?,
        project_path: row.get(2)?,
        status: row.get(3)?,
        git_branch: row.get(4)?,
        is_open: row.get::<_, i32>(5)? != 0,
        last_active_at: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn parse_json_vec(value: String) -> Vec<String> {
    serde_json::from_str(&value).unwrap_or_default()
}

fn format_session_status(s: &crate::models::session::SessionStatus) -> String {
    match s {
        crate::models::session::SessionStatus::Idle => "idle".into(),
        crate::models::session::SessionStatus::Running => "running".into(),
        crate::models::session::SessionStatus::WaitingConfirm => "waiting_confirm".into(),
        crate::models::session::SessionStatus::Paused => "paused".into(),
        crate::models::session::SessionStatus::Completed => "completed".into(),
    }
}

fn parse_session_status(s: &str) -> crate::models::session::SessionStatus {
    match s {
        "idle" => crate::models::session::SessionStatus::Idle,
        "running" => crate::models::session::SessionStatus::Running,
        "waiting_confirm" => crate::models::session::SessionStatus::WaitingConfirm,
        "paused" => crate::models::session::SessionStatus::Paused,
        "completed" => crate::models::session::SessionStatus::Completed,
        _ => crate::models::session::SessionStatus::Idle,
    }
}

fn format_phase_status(s: &crate::models::session::PhaseStatus) -> String {
    match s {
        crate::models::session::PhaseStatus::Pending => "pending".into(),
        crate::models::session::PhaseStatus::Running => "running".into(),
        crate::models::session::PhaseStatus::WaitingConfirm => "waiting_confirm".into(),
        crate::models::session::PhaseStatus::Completed => "completed".into(),
        crate::models::session::PhaseStatus::Skipped => "skipped".into(),
        crate::models::session::PhaseStatus::Failed => "failed".into(),
    }
}

fn parse_phase_status(s: &str) -> crate::models::session::PhaseStatus {
    match s {
        "pending" => crate::models::session::PhaseStatus::Pending,
        "running" => crate::models::session::PhaseStatus::Running,
        "waiting_confirm" => crate::models::session::PhaseStatus::WaitingConfirm,
        "completed" => crate::models::session::PhaseStatus::Completed,
        "skipped" => crate::models::session::PhaseStatus::Skipped,
        "failed" => crate::models::session::PhaseStatus::Failed,
        _ => crate::models::session::PhaseStatus::Pending,
    }
}
