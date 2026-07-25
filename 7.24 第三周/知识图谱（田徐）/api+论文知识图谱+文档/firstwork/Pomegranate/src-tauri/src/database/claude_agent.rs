use crate::error::AppError;
use crate::models::{ClaudeAgentEvent, ClaudeAgentSession, StartClaudeAgentInput};
use uuid::Uuid;

use super::Database;

/// 把一行 claude_agent_sessions 转成 ClaudeAgentSession
fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<ClaudeAgentSession> {
    Ok(ClaudeAgentSession {
        id: row.get(0)?,
        project_path: row.get(1)?,
        prompt: row.get(2)?,
        session_name: row.get(3)?,
        permission_mode: row.get(4)?,
        status: row.get(5)?,
        pid: row.get(6)?,
        exit_code: row.get(7)?,
        error_message: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        started_at: row.get(11)?,
        finished_at: row.get(12)?,
    })
}

/// 把一行 claude_agent_events 转成 ClaudeAgentEvent
fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<ClaudeAgentEvent> {
    Ok(ClaudeAgentEvent {
        id: row.get(0)?,
        session_id: row.get(1)?,
        kind: row.get(2)?,
        content: row.get(3)?,
        created_at: row.get(4)?,
    })
}

impl Database {
    // ─── Agent 会话 ─────────────────────────────

    pub fn create_agent_session(
        &self,
        input: &StartClaudeAgentInput,
    ) -> Result<ClaudeAgentSession, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO claude_agent_sessions (id, project_path, prompt, session_name, permission_mode)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id,
                input.project_path,
                input.prompt,
                input.session_name,
                input.permission_mode,
            ],
        )?;

        drop(conn);
        self.get_agent_session(&id)
    }

    pub fn get_agent_session(&self, id: &str) -> Result<ClaudeAgentSession, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        let s = conn.query_row(
            "SELECT id, project_path, prompt, session_name, permission_mode,
                    status, pid, exit_code, error_message,
                    created_at, updated_at, started_at, finished_at
             FROM claude_agent_sessions WHERE id = ?1",
            [id],
            row_to_session,
        )?;
        Ok(s)
    }

    pub fn list_agent_sessions(
        &self,
        project_path: Option<&str>,
    ) -> Result<Vec<ClaudeAgentSession>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match project_path {
            Some(path) => (
                "SELECT id, project_path, prompt, session_name, permission_mode,
                        status, pid, exit_code, error_message,
                        created_at, updated_at, started_at, finished_at
                 FROM claude_agent_sessions WHERE project_path = ?1
                 ORDER BY created_at DESC"
                    .into(),
                vec![Box::new(path.to_string())],
            ),
            None => (
                "SELECT id, project_path, prompt, session_name, permission_mode,
                        status, pid, exit_code, error_message,
                        created_at, updated_at, started_at, finished_at
                 FROM claude_agent_sessions
                 ORDER BY created_at DESC"
                    .into(),
                vec![],
            ),
        };

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let sessions = stmt
            .query_map(param_refs.as_slice(), row_to_session)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    pub fn update_agent_session_status(
        &self,
        id: &str,
        status: &str,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let finished = matches!(status, "completed" | "failed" | "cancelled");

        conn.execute(
            "UPDATE claude_agent_sessions SET status = ?1, exit_code = ?2,
                 error_message = COALESCE(?3, error_message),
                 started_at = CASE WHEN ?4 AND started_at IS NULL THEN datetime('now','localtime') ELSE started_at END,
                 finished_at = CASE WHEN ?5 THEN datetime('now','localtime') ELSE finished_at END,
                 updated_at = datetime('now','localtime')
             WHERE id = ?6",
            rusqlite::params![
                status,
                exit_code,
                error,
                status == "running",
                finished,
                id
            ],
        )?;

        Ok(())
    }

    pub fn set_agent_pid(&self, id: &str, pid: i64) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;
        conn.execute(
            "UPDATE claude_agent_sessions SET pid = ?1, updated_at = datetime('now','localtime') WHERE id = ?2",
            rusqlite::params![pid, id],
        )?;
        Ok(())
    }

    // ─── Agent 事件 ─────────────────────────────

    pub fn add_agent_event(
        &self,
        session_id: &str,
        kind: &str,
        content: &str,
    ) -> Result<ClaudeAgentEvent, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        conn.execute(
            "INSERT INTO claude_agent_events (session_id, kind, content)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![session_id, kind, content],
        )?;

        let id = conn.last_insert_rowid();

        drop(conn);
        // 简单重建
        Ok(ClaudeAgentEvent {
            id,
            session_id: session_id.to_string(),
            kind: kind.to_string(),
            content: content.to_string(),
            created_at: String::new(),
        })
    }

    pub fn list_agent_events(&self, session_id: &str) -> Result<Vec<ClaudeAgentEvent>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Custom(e.to_string()))?;

        let mut stmt = conn.prepare(
            "SELECT id, session_id, kind, content, created_at
             FROM claude_agent_events WHERE session_id = ?1
             ORDER BY id ASC",
        )?;

        let events = stmt
            .query_map([session_id], row_to_event)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_input() -> StartClaudeAgentInput {
        StartClaudeAgentInput {
            project_path: "/tmp/test-project".into(),
            prompt: "Hello".into(),
            permission_mode: "readonly".into(),
            session_name: Some("test session".into()),
        }
    }

    fn setup_db() -> Database {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("kb_agent_test_{}_{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        Database::init(dir.join("test.db").to_str().unwrap()).expect("init test db")
    }

    #[test]
    fn create_and_get_session() {
        let db = setup_db();
        let input = mk_input();
        let s = db.create_agent_session(&input).unwrap();
        assert_eq!(s.project_path, "/tmp/test-project");
        assert_eq!(s.status, "pending");
        assert_eq!(s.prompt, "Hello");

        let got = db.get_agent_session(&s.id).unwrap();
        assert_eq!(got.id, s.id);
    }

    #[test]
    fn update_status_flow() {
        let db = setup_db();
        let s = db.create_agent_session(&mk_input()).unwrap();

        db.update_agent_session_status(&s.id, "running", Some(42), None)
            .unwrap();
        let s = db.get_agent_session(&s.id).unwrap();
        assert_eq!(s.status, "running");
        assert_eq!(s.exit_code, Some(42));

        db.update_agent_session_status(&s.id, "completed", Some(0), None)
            .unwrap();
        let s = db.get_agent_session(&s.id).unwrap();
        assert_eq!(s.status, "completed");
    }

    #[test]
    fn list_by_project() {
        let db = setup_db();
        let a = db.create_agent_session(&mk_input()).unwrap();
        let mut b_input = mk_input();
        b_input.project_path = "/tmp/other".into();
        db.create_agent_session(&b_input).unwrap();

        let list = db.list_agent_sessions(Some("/tmp/test-project")).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, a.id);
    }

    #[test]
    fn add_and_list_events() {
        let db = setup_db();
        let s = db.create_agent_session(&mk_input()).unwrap();

        db.add_agent_event(&s.id, "stdout", "line 1").unwrap();
        db.add_agent_event(&s.id, "stderr", "error").unwrap();

        let e = db.list_agent_events(&s.id).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].content, "line 1");
        assert_eq!(e[1].kind, "stderr");
    }
}
