use chrono::Local;
use uuid::Uuid;

use crate::database::Database;
use crate::error::AppError;
use crate::models::session::{
    ExecutionPhase, PhaseStatus, ProjectSession, ProjectSessionContext, SessionStatus, TaskSession,
};
use crate::services::session_plan::SessionPlanService;

/// 会话管理服务 — 会话生命周期 + Phase 状态机
pub struct SessionManagerService;

impl SessionManagerService {
    /// 创建新会话：解析计划文件 → 创建 session + phases 入 DB
    pub fn create_session(db: &Database, plan_path: &str) -> Result<TaskSession, AppError> {
        // 1. 解析计划
        let plan = SessionPlanService::parse_plan_file(plan_path)?;

        let session_id = Uuid::new_v4().to_string();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let total_phases = plan.phases.len() as i32;

        // 2. 创建 session
        let session = TaskSession {
            id: session_id.clone(),
            plan_path: plan_path.to_string(),
            plan_name: plan.name,
            status: SessionStatus::Idle,
            current_phase_index: 0,
            total_phases: total_phases,
            created_at: now.clone(),
            updated_at: now,
        };
        db.create_session(&session)?;

        // 3. 批量创建 phases
        let phases: Vec<ExecutionPhase> = plan
            .phases
            .into_iter()
            .map(|p| ExecutionPhase {
                id: format!("{}:{}", session_id, p.id),
                session_id: session_id.clone(),
                index_num: p
                    .id
                    .strip_prefix("phase_")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                name: p.name,
                description: p.description,
                status: PhaseStatus::Pending,
                files_modified: None,
                result_summary: None,
                started_at: None,
                finished_at: None,
            })
            .collect();
        db.create_phases_batch(&phases)?;

        Ok(session)
    }

    /// 开始执行指定 Phase
    pub fn start_phase(
        db: &Database,
        session_id: &str,
        phase_index: i32,
    ) -> Result<ExecutionPhase, AppError> {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let phase_id = format!("{}:phase_{}", session_id, phase_index);

        // 更新 session 状态
        db.update_session_status(session_id, "running", phase_index)?;

        // 更新 phase
        db.update_phase_status(&phase_id, "running", None, None)?;
        db.update_phase_timestamps(&phase_id, Some(&now), None)?;

        // 返回更新后的 phase
        let phases = db.get_phases_by_session(session_id)?;
        phases
            .into_iter()
            .find(|p| p.index_num == phase_index)
            .ok_or_else(|| AppError::NotFound(format!("Phase {} 不存在", phase_index)))
    }

    /// 完成当前 Phase（标记为等待确认）
    pub fn complete_phase(
        db: &Database,
        session_id: &str,
        phase_index: i32,
        result_summary: Option<&str>,
        files_modified: Option<&str>,
    ) -> Result<(), AppError> {
        let phase_id = format!("{}:phase_{}", session_id, phase_index);
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        db.update_phase_status(&phase_id, "waiting_confirm", result_summary, files_modified)?;
        db.update_phase_timestamps(&phase_id, None, Some(&now))?;
        db.update_session_status(session_id, "waiting_confirm", phase_index)?;

        Ok(())
    }

    /// 确认当前 Phase → 已完成，推进到下一 Phase
    pub fn confirm_phase(db: &Database, session_id: &str) -> Result<(), AppError> {
        let session = db
            .get_session(session_id)?
            .ok_or_else(|| AppError::NotFound("会话不存在".into()))?;

        let current_index = session.current_phase_index;
        let phase_id = format!("{}:phase_{}", session_id, current_index);

        // 标记当前 phase 为 completed
        db.update_phase_status(&phase_id, "completed", None, None)?;

        let next_index = current_index + 1;
        if next_index >= session.total_phases {
            // 全部完成
            db.update_session_status(session_id, "completed", current_index)?;
        } else {
            // 推进到下一 phase（但不自动启动）
            db.update_session_status(session_id, "idle", next_index)?;
        }

        Ok(())
    }

    /// 跳过当前 Phase
    pub fn skip_phase(db: &Database, session_id: &str, phase_index: i32) -> Result<(), AppError> {
        let phase_id = format!("{}:phase_{}", session_id, phase_index);
        db.update_phase_status(&phase_id, "skipped", None, None)?;

        let session = db
            .get_session(session_id)?
            .ok_or_else(|| AppError::NotFound("会话不存在".into()))?;

        let next_index = phase_index + 1;
        if next_index >= session.total_phases {
            db.update_session_status(session_id, "completed", phase_index)?;
        } else {
            db.update_session_status(session_id, "idle", next_index)?;
        }

        Ok(())
    }

    /// 重试当前 Phase（重置状态）
    pub fn retry_phase(db: &Database, session_id: &str, phase_index: i32) -> Result<(), AppError> {
        let phase_id = format!("{}:phase_{}", session_id, phase_index);
        db.update_phase_status(&phase_id, "pending", None, None)?;
        db.update_phase_timestamps(&phase_id, None, None)?;
        db.update_session_status(session_id, "idle", phase_index)?;
        Ok(())
    }

    /// 暂停会话
    pub fn pause_session(db: &Database, session_id: &str) -> Result<(), AppError> {
        let session = db
            .get_session(session_id)?
            .ok_or_else(|| AppError::NotFound("会话不存在".into()))?;
        db.update_session_status(session_id, "paused", session.current_phase_index)?;
        Ok(())
    }

    /// 恢复会话
    pub fn resume_session(db: &Database, session_id: &str) -> Result<(), AppError> {
        let session = db
            .get_session(session_id)?
            .ok_or_else(|| AppError::NotFound("会话不存在".into()))?;
        db.update_session_status(session_id, "idle", session.current_phase_index)?;
        Ok(())
    }

    /// 获取会话详情（含 phases）
    pub fn get_session_with_phases(
        db: &Database,
        session_id: &str,
    ) -> Result<Option<(TaskSession, Vec<ExecutionPhase>)>, AppError> {
        let session = match db.get_session(session_id)? {
            Some(s) => s,
            None => return Ok(None),
        };
        let phases = db.get_phases_by_session(session_id)?;
        Ok(Some((session, phases)))
    }

    // ─── 项目文件夹会话 ─────────────────────────

    /// 打开或恢复项目会话
    pub fn open_project_session(
        db: &Database,
        project_path: &str,
        project_name: Option<&str>,
    ) -> Result<ProjectSession, AppError> {
        let path = std::path::Path::new(project_path);
        if !path.exists() || !path.is_dir() {
            return Err(AppError::InvalidInput("项目路径不存在或不是文件夹".into()));
        }
        let project_name = project_name
            .filter(|n| !n.trim().is_empty())
            .map(|n| n.trim().to_string())
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| project_path.to_string())
            });
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let git_branch = detect_git_branch(path);

        // 尝试按路径查找已有会话
        let existing = db.get_project_session_by_path(project_path)?;
        let session = match existing {
            Some(mut s) => {
                s.status = "active".into();
                s.is_open = true;
                s.git_branch = git_branch;
                s.last_active_at = now.clone();
                db.upsert_project_session(&s)?;
                s
            }
            None => {
                let s = ProjectSession {
                    id: Uuid::new_v4().to_string(),
                    project_name,
                    project_path: project_path.to_string(),
                    status: "active".into(),
                    git_branch,
                    is_open: true,
                    last_active_at: now.clone(),
                    created_at: now,
                };
                db.upsert_project_session(&s)?;
                s
            }
        };

        // 初始化或更新上下文
        let ctx = ProjectSessionContext {
            session_id: session.id.clone(),
            project_path: session.project_path.clone(),
            git_branch: session.git_branch.clone(),
            changed_files: vec![],
            pinned_files: vec![],
            recent_files: vec![],
            current_task: None,
            updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        db.upsert_project_session_context(&ctx)?;

        Ok(session)
    }

    /// 关闭项目会话 Tab（不删除历史数据）
    pub fn close_project_session(db: &Database, session_id: &str) -> Result<(), AppError> {
        let session = db
            .get_project_session_by_id(session_id)?
            .ok_or_else(|| AppError::NotFound("项目会话不存在".into()))?;
        if session.is_open {
            db.close_project_session(session_id)?;
        }
        Ok(())
    }
}

/// 探测项目 Git 分支（失败静默返回 None）
fn detect_git_branch(project_path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(project_path)
        .output()
        .ok()?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    None
}
