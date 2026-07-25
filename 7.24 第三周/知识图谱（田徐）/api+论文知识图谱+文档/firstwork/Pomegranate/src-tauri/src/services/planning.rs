use chrono::Local;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

use crate::database::Database;
use crate::error::AppError;
use crate::models::{PlanningFileView, PlanningSessionKind, PlanningWorkspace};

pub const PLANNING_PLUGIN_ID: &str = "official-planning-with-files";
const WORKSPACE_DIR: &str = "planning-workspaces";
const MAX_FILE_BYTES: usize = 256 * 1024;
const CONTEXT_CHAR_BUDGET: usize = 3600;
const FILES: &[&str] = &["plan.md", "findings.md", "progress.md"];

pub struct PlanningService;

#[derive(Debug, Clone)]
pub struct PlanningContext {
    pub enabled: bool,
    pub content: String,
    pub chars: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum PlanningProviderMode {
    SystemMessage,
    TextPrefix,
}

impl PlanningService {
    pub fn get_workspace(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
    ) -> Result<PlanningWorkspace, AppError> {
        let session_key = session_key(&kind, session_id)?;
        let (enabled, auto_apply, pending_update, last_updated_at) =
            load_state(db, &session_key)?.unwrap_or((false, false, None, None));
        let (plugin_ready, blocked_reason) = plugin_ready(db);
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        if enabled && plugin_ready {
            ensure_workspace_files(&workspace, session_id)?;
        }
        let files = read_files_if_present(&workspace)?;
        let (current_stage, progress_percent, blockers) = summarize_workspace_v2(&files);
        Ok(PlanningWorkspace {
            plugin_id: PLANNING_PLUGIN_ID.to_string(),
            plugin_ready,
            enabled,
            auto_apply,
            session_kind: kind,
            session_id: session_id.to_string(),
            workspace_path: workspace.to_string_lossy().to_string(),
            files,
            pending_update,
            current_stage,
            progress_percent,
            blockers,
            last_updated_at,
            blocked_reason,
        })
    }

    pub fn set_enabled(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        enabled: bool,
    ) -> Result<PlanningWorkspace, AppError> {
        let session_key = session_key(&kind, session_id)?;
        if enabled {
            ensure_plugin_ready(db)?;
            ensure_workspace_files(&workspace_path(data_dir, &kind, session_id)?, session_id)?;
        }
        upsert_state(db, &session_key, &kind, session_id, enabled)?;
        db.write_audit_log(
            PLANNING_PLUGIN_ID,
            if enabled {
                "planning_enabled"
            } else {
                "planning_disabled"
            },
            Some(&session_key),
        )
        .ok();
        Self::get_workspace(db, data_dir, kind, session_id)
    }

    pub fn save_file(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        file_name: &str,
        content: &str,
    ) -> Result<PlanningWorkspace, AppError> {
        ensure_plugin_ready(db)?;
        ensure_session_enabled(db, &kind, session_id)?;
        let name = validate_file_name(file_name)?;
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        ensure_workspace_files(&workspace, session_id)?;
        atomic_write(&workspace.join(name), &redact_sensitive(content)?)?;
        touch_state(db, &session_key(&kind, session_id)?)?;
        db.write_audit_log(PLANNING_PLUGIN_ID, "planning_file_edited", Some(name))
            .ok();
        Self::get_workspace(db, data_dir, kind, session_id)
    }

    pub fn clear(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        confirm: bool,
    ) -> Result<PlanningWorkspace, AppError> {
        if !confirm {
            return Err(AppError::InvalidInput("清空规划需要二次确认".into()));
        }
        ensure_plugin_ready(db)?;
        ensure_session_enabled(db, &kind, session_id)?;
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        fs::create_dir_all(&workspace)?;
        for file in FILES {
            atomic_write(
                &workspace.join(file),
                &default_file_content(file, session_id),
            )?;
        }
        set_pending_update(db, &session_key(&kind, session_id)?, None)?;
        db.write_audit_log(
            PLANNING_PLUGIN_ID,
            "planning_cleared",
            Some(&session_key(&kind, session_id)?),
        )
        .ok();
        Self::get_workspace(db, data_dir, kind, session_id)
    }

    pub fn export(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        target_dir: &Path,
    ) -> Result<(), AppError> {
        ensure_plugin_ready(db)?;
        ensure_session_enabled(db, &kind, session_id)?;
        if !target_dir.exists() || !target_dir.is_dir() {
            return Err(AppError::InvalidInput("导出目录不存在".into()));
        }
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        let export_dir =
            target_dir.join(format!("planning-{}", safe_session_dir(&kind, session_id)?));
        fs::create_dir_all(&export_dir)?;
        for file in FILES {
            let src = workspace.join(file);
            if src.exists() {
                fs::copy(&src, export_dir.join(file))?;
            }
        }
        db.write_audit_log(
            PLANNING_PLUGIN_ID,
            "planning_exported",
            Some(&session_key(&kind, session_id)?),
        )
        .ok();
        Ok(())
    }

    pub fn build_context(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        user_input: &str,
        mode: PlanningProviderMode,
    ) -> Result<PlanningContext, AppError> {
        if !is_session_enabled(db, &kind, session_id)? {
            return Ok(PlanningContext {
                enabled: false,
                content: user_input.to_string(),
                chars: 0,
            });
        }
        ensure_plugin_ready(db)?;
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        ensure_workspace_files(&workspace, session_id)?;
        let files = read_files_if_present(&workspace)?;
        let mut context = compact_context(&files, CONTEXT_CHAR_BUDGET);
        context = context.replace("planningUpdate", "planning_update tool");
        let chars = context.chars().count();
        db.write_audit_log(
            PLANNING_PLUGIN_ID,
            "planning_context_injected",
            Some(&format!("{} chars", chars)),
        )
        .ok();
        let content = match mode {
            PlanningProviderMode::SystemMessage => context,
            PlanningProviderMode::TextPrefix => format!(
                "[PLANNING CONTEXT]\n{}\n[/PLANNING CONTEXT]\n\n[USER REQUEST]\n{}\n[/USER REQUEST]",
                context, user_input
            ),
        };
        Ok(PlanningContext {
            enabled: true,
            content,
            chars,
        })
    }

    pub fn planning_system_message(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
    ) -> Result<Option<String>, AppError> {
        if !is_session_enabled(db, &kind, session_id)? {
            return Ok(None);
        }
        let context = Self::build_context(
            db,
            data_dir,
            kind,
            session_id,
            "",
            PlanningProviderMode::SystemMessage,
        )?;
        Ok(Some(format!(
            "{}\n\nUse planning_update for planning changes. Do not append JSON planning updates to the final answer. Never write keys, tokens, secrets, or Authorization headers into planning files.",
            context.content
        )))
    }

    pub fn planning_tool_instruction() -> &'static str {
        "Maintain Planning with Files through the planning_update tool/protocol. Do not show Planning Context, DSML, tool_calls, or planningUpdate JSON to the user. Call planning_update when the goal, acceptance criteria, stage, todo list, findings, decisions, blockers, progress, or next step changes. Keep the natural-language answer separate from planning updates."
    }

    pub fn sanitize_visible_response(text: &str) -> String {
        sanitize_visible_response(text)
    }

    pub fn emit_workspace_updated(
        app: &AppHandle,
        workspace: &PlanningWorkspace,
        changed_sections: Vec<String>,
    ) {
        emit_workspace_updated(app, workspace, changed_sections);
    }

    pub fn record_completion_and_emit(
        app: &AppHandle,
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        status: &str,
        user_input: &str,
        assistant_output: &str,
    ) -> Result<(), AppError> {
        Self::record_completion(
            db,
            data_dir,
            kind.clone(),
            session_id,
            status,
            user_input,
            assistant_output,
        )?;
        let workspace = Self::get_workspace(db, data_dir, kind, session_id)?;
        emit_workspace_updated(
            app,
            &workspace,
            vec!["plan".into(), "findings".into(), "progress".into()],
        );
        Ok(())
    }

    pub fn apply_tool_update_and_emit(
        app: &AppHandle,
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        args_json: &str,
    ) -> Result<PlanningWorkspace, AppError> {
        ensure_plugin_ready(db)?;
        ensure_session_enabled(db, &kind, session_id)?;
        let parsed: Value = serde_json::from_str(args_json).map_err(|_| {
            AppError::InvalidInput("planning_update arguments are not valid JSON".into())
        })?;
        let update = normalize_planning_update_value(parsed)?;
        let key = session_key(&kind, session_id)?;
        let workspace_path = workspace_path(data_dir, &kind, session_id)?;
        ensure_workspace_files(&workspace_path, session_id)?;
        apply_update_to_files(&workspace_path, &update)?;
        touch_state(db, &key)?;
        db.write_audit_log(PLANNING_PLUGIN_ID, "planning_update_applied", Some(&key))
            .ok();
        let workspace = Self::get_workspace(db, data_dir, kind, session_id)?;
        emit_workspace_updated(
            app,
            &workspace,
            vec!["plan".into(), "findings".into(), "progress".into()],
        );
        Ok(workspace)
    }

    pub fn record_activity_and_emit(
        app: &AppHandle,
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        status: &str,
        note: &str,
    ) -> Result<(), AppError> {
        if !is_session_enabled(db, &kind, session_id)? {
            return Ok(());
        }
        ensure_plugin_ready(db)?;
        let key = session_key(&kind, session_id)?;
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        ensure_workspace_files(&workspace, session_id)?;
        append_activity_to_files(&workspace, status, note)?;
        touch_state(db, &key)?;
        db.write_audit_log(PLANNING_PLUGIN_ID, "planning_progress_synced", Some(&key))
            .ok();
        let workspace_view = Self::get_workspace(db, data_dir, kind, session_id)?;
        emit_workspace_updated(app, &workspace_view, vec!["plan".into(), "progress".into()]);
        Ok(())
    }

    pub fn record_completion(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        status: &str,
        user_input: &str,
        assistant_output: &str,
    ) -> Result<(), AppError> {
        if !is_session_enabled(db, &kind, session_id)? {
            return Ok(());
        }
        ensure_plugin_ready(db)?;
        let key = session_key(&kind, session_id)?;
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        ensure_workspace_files(&workspace, session_id)?;
        let safe_user = truncate_chars(&redact_sensitive(user_input)?, 240);
        let progress = format!(
            "\n\n## {}\n- 状态：{}\n- 用户请求：{}\n- 结果：{}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            status,
            safe_user,
            match status {
                "completed" => "AI 回复完成，等待用户检查是否应用规划更新。",
                "cancelled" => "用户取消生成，未将本轮输出标记为完成。",
                "failed" | "error" => "Provider 调用失败，保留原规划文件。",
                _ => "本轮已记录。",
            }
        );
        append_file(&workspace.join("progress.md"), &progress)?;
        append_activity_to_files(&workspace, status, "AI 本轮调用状态已同步")?;

        if status == "completed" {
            let tool_updates = extract_planning_tool_updates(assistant_output)?;
            if !tool_updates.is_empty() {
                for update in tool_updates {
                    apply_update_to_files(&workspace, &update)?;
                }
                db.write_audit_log(PLANNING_PLUGIN_ID, "planning_update_applied", Some(&key))
                    .ok();
            } else if let Some(update) = extract_planning_update(assistant_output)? {
                set_pending_update(db, &key, Some(update.to_string()))?;
                db.write_audit_log(PLANNING_PLUGIN_ID, "planning_update_proposed", Some(&key))
                    .ok();
            }
        } else if status == "cancelled" {
            db.write_audit_log(PLANNING_PLUGIN_ID, "planning_cancelled", Some(&key))
                .ok();
        } else if status == "failed" || status == "error" {
            db.write_audit_log(PLANNING_PLUGIN_ID, "planning_failed", Some(&key))
                .ok();
        }
        touch_state(db, &key)?;
        Ok(())
    }

    pub fn apply_update(
        db: &Database,
        data_dir: &Path,
        kind: PlanningSessionKind,
        session_id: &str,
        accept: bool,
    ) -> Result<PlanningWorkspace, AppError> {
        ensure_plugin_ready(db)?;
        ensure_session_enabled(db, &kind, session_id)?;
        let key = session_key(&kind, session_id)?;
        let pending = load_state(db, &key)?
            .and_then(|(_, _, p, _)| p)
            .ok_or_else(|| AppError::InvalidInput("当前没有待应用的规划更新".into()))?;
        if !accept {
            set_pending_update(db, &key, None)?;
            db.write_audit_log(PLANNING_PLUGIN_ID, "planning_update_rejected", Some(&key))
                .ok();
            return Self::get_workspace(db, data_dir, kind, session_id);
        }
        let value: Value = serde_json::from_str(&pending)
            .map_err(|_| AppError::InvalidInput("待应用规划更新不是合法 JSON".into()))?;
        let workspace = workspace_path(data_dir, &kind, session_id)?;
        ensure_workspace_files(&workspace, session_id)?;
        apply_update_to_files(&workspace, &value)?;
        set_pending_update(db, &key, None)?;
        db.write_audit_log(PLANNING_PLUGIN_ID, "planning_update_applied", Some(&key))
            .ok();
        Self::get_workspace(db, data_dir, kind, session_id)
    }
}

fn plugin_ready(db: &Database) -> (bool, Option<String>) {
    match ensure_plugin_ready(db) {
        Ok(()) => (true, None),
        Err(err) => (false, Some(err.to_string())),
    }
}

fn ensure_plugin_ready(db: &Database) -> Result<(), AppError> {
    let plugin = db.get_plugin(PLANNING_PLUGIN_ID)?;
    if !plugin.enabled || plugin.status != "installed" {
        return Err(AppError::InvalidInput(
            "Planning with Files 插件尚未安装并启用".into(),
        ));
    }
    for permission in [
        "ai.context.read",
        "ai.context.augment",
        "ai.session.read",
        "planning.files.read",
        "planning.files.write",
        "ui.chat.toolbar",
        "ui.chat.panel",
    ] {
        if !db.has_plugin_permission(PLANNING_PLUGIN_ID, permission)? {
            return Err(AppError::InvalidInput(format!(
                "Planning with Files 缺少权限：{}",
                permission
            )));
        }
    }
    Ok(())
}

fn kind_to_db(kind: &PlanningSessionKind) -> &'static str {
    match kind {
        PlanningSessionKind::Ai => "ai",
        PlanningSessionKind::Agent => "agent",
    }
}

fn session_key(kind: &PlanningSessionKind, session_id: &str) -> Result<String, AppError> {
    validate_session_id(kind, session_id)?;
    Ok(format!("{}:{}", kind_to_db(kind), session_id))
}

fn validate_session_id(kind: &PlanningSessionKind, session_id: &str) -> Result<(), AppError> {
    let ok = match kind {
        PlanningSessionKind::Ai => session_id.chars().all(|c| c.is_ascii_digit()),
        PlanningSessionKind::Agent => {
            session_id.starts_with("sess-")
                && session_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
        }
    };
    if ok && session_id.len() <= 96 {
        Ok(())
    } else {
        Err(AppError::InvalidInput("非法 planning sessionId".into()))
    }
}

fn safe_session_dir(kind: &PlanningSessionKind, session_id: &str) -> Result<String, AppError> {
    validate_session_id(kind, session_id)?;
    Ok(format!("{}-{}", kind_to_db(kind), session_id))
}

fn workspace_path(
    data_dir: &Path,
    kind: &PlanningSessionKind,
    session_id: &str,
) -> Result<PathBuf, AppError> {
    let base = data_dir.join(WORKSPACE_DIR);
    let path = base.join(safe_session_dir(kind, session_id)?);
    let normalized = path.components().collect::<PathBuf>();
    if !normalized.starts_with(&base) {
        return Err(AppError::InvalidInput("planning workspace 越界".into()));
    }
    Ok(normalized)
}

fn validate_file_name(file_name: &str) -> Result<&'static str, AppError> {
    FILES
        .iter()
        .copied()
        .find(|name| *name == file_name)
        .ok_or_else(|| AppError::InvalidInput("非法 planning 文件名".into()))
}

type PlanningState = (bool, bool, Option<String>, Option<String>);

fn load_state(db: &Database, key: &str) -> Result<Option<PlanningState>, AppError> {
    let conn = db.conn_lock()?;
    conn.query_row(
        "SELECT enabled, auto_apply, pending_update_json, last_updated_at
         FROM planning_sessions WHERE session_key = ?1",
        params![key],
        |row| {
            Ok((
                row.get::<_, i64>(0)? != 0,
                row.get::<_, i64>(1)? != 0,
                row.get(2)?,
                row.get(3)?,
            ))
        },
    )
    .optional()
    .map_err(AppError::from)
}

fn upsert_state(
    db: &Database,
    key: &str,
    kind: &PlanningSessionKind,
    session_id: &str,
    enabled: bool,
) -> Result<(), AppError> {
    let conn = db.conn_lock()?;
    conn.execute(
        "INSERT INTO planning_sessions
            (session_key, session_kind, session_id, enabled, last_updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now','localtime'))
         ON CONFLICT(session_key) DO UPDATE SET
            enabled = excluded.enabled,
            last_updated_at = excluded.last_updated_at",
        params![key, kind_to_db(kind), session_id, enabled as i64],
    )?;
    Ok(())
}

fn touch_state(db: &Database, key: &str) -> Result<(), AppError> {
    let conn = db.conn_lock()?;
    conn.execute(
        "UPDATE planning_sessions SET last_updated_at = datetime('now','localtime')
         WHERE session_key = ?1",
        params![key],
    )?;
    Ok(())
}

fn set_pending_update(db: &Database, key: &str, pending: Option<String>) -> Result<(), AppError> {
    let conn = db.conn_lock()?;
    conn.execute(
        "UPDATE planning_sessions
         SET pending_update_json = ?2, last_updated_at = datetime('now','localtime')
         WHERE session_key = ?1",
        params![key, pending],
    )?;
    Ok(())
}

fn is_session_enabled(
    db: &Database,
    kind: &PlanningSessionKind,
    session_id: &str,
) -> Result<bool, AppError> {
    let key = session_key(kind, session_id)?;
    Ok(load_state(db, &key)?.map(|s| s.0).unwrap_or(false))
}

fn ensure_session_enabled(
    db: &Database,
    kind: &PlanningSessionKind,
    session_id: &str,
) -> Result<(), AppError> {
    if is_session_enabled(db, kind, session_id)? {
        Ok(())
    } else {
        Err(AppError::InvalidInput(
            "当前会话未开启 Planning with Files".into(),
        ))
    }
}

fn ensure_workspace_files(workspace: &Path, session_id: &str) -> Result<(), AppError> {
    fs::create_dir_all(workspace)?;
    for file in FILES {
        let path = workspace.join(file);
        if !path.exists() {
            atomic_write(&path, &default_file_content(file, session_id))?;
        }
    }
    Ok(())
}

fn default_file_content(file: &str, session_id: &str) -> String {
    match file {
        "plan.md" => format!(
            "# Plan\n\n- 当前任务目标：待用户明确\n- 验收标准：待补充\n- 当前阶段：初始化\n\n## 待办事项\n- [ ] 明确本会话目标\n\n## 阻塞问题\n- 暂无\n\n## 下一步\n- 继续对话并更新规划\n\n<!-- session:{} -->\n",
            session_id
        ),
        "findings.md" => "# Findings\n\n## 已确认事实\n- 暂无\n\n## 用户决策\n- 暂无\n\n## 技术约束\n- 暂无\n".into(),
        "progress.md" => format!(
            "# Progress\n\n## {}\n- 已创建 Planning with Files 工作区\n- 下一步：等待用户请求\n",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        ),
        _ => String::new(),
    }
}

fn read_files_if_present(workspace: &Path) -> Result<Vec<PlanningFileView>, AppError> {
    let mut out = Vec::new();
    for file in FILES {
        let path = workspace.join(file);
        let content = if path.exists() {
            read_limited_utf8(&path)?
        } else {
            String::new()
        };
        let updated_at = path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|_| Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        out.push(PlanningFileView {
            name: (*file).to_string(),
            content,
            updated_at,
        });
    }
    Ok(out)
}

fn read_limited_utf8(path: &Path) -> Result<String, AppError> {
    let meta = path.metadata()?;
    if meta.len() as usize > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput("planning 文件超过大小限制".into()));
    }
    fs::read_to_string(path).map_err(AppError::from)
}

fn atomic_write(path: &Path, content: &str) -> Result<(), AppError> {
    if content.as_bytes().len() > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput("planning 内容超过大小限制".into()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn append_file(path: &Path, content: &str) -> Result<(), AppError> {
    let mut current = if path.exists() {
        read_limited_utf8(path)?
    } else {
        String::new()
    };
    current.push_str(content);
    atomic_write(path, &current)
}

fn append_activity_to_files(workspace: &Path, status: &str, note: &str) -> Result<(), AppError> {
    let safe_status = redact_sensitive(status)?;
    let safe_note = truncate_chars(&redact_sensitive(note)?, 320);
    let progress = format!(
        "\n\n## {}\n- 状态：{}\n- 进度：{}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        safe_status,
        safe_note
    );
    append_file(&workspace.join("progress.md"), &progress)?;

    let plan_path = workspace.join("plan.md");
    let current = read_limited_utf8(&plan_path)?;
    let stage = match status {
        "started" => "执行中",
        "tool" => "执行中",
        "completed" => "已完成本轮",
        "cancelled" => "已取消",
        "failed" | "error" => "遇到问题",
        _ => "执行中",
    };
    let mut next = replace_stage_line(&current, stage);
    if !next.contains("AI 已开始处理本轮请求") && matches!(status, "started" | "tool" | "completed")
    {
        next.push_str("\n## 自动同步进度\n- [x] AI 已开始处理本轮请求\n");
    }
    if matches!(status, "completed") && !next.contains("本轮 AI 回复完成") {
        next.push_str("- [x] 本轮 AI 回复完成\n");
    }
    atomic_write(&plan_path, &next)
}

fn replace_stage_line(content: &str, stage: &str) -> String {
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        if line.contains("当前阶段")
            || line.contains("å½“å‰é˜¶æ®µ")
            || line.contains("Current Stage")
        {
            lines.push(format!("- 当前阶段：{}", stage));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.insert(1.min(lines.len()), format!("- 当前阶段：{}", stage));
    }
    lines.join("\n")
}

fn compact_context(files: &[PlanningFileView], budget: usize) -> String {
    let mut sections = Vec::new();
    for file in files {
        let title = match file.name.as_str() {
            "plan.md" => "计划",
            "findings.md" => "发现",
            "progress.md" => "最近进度",
            _ => file.name.as_str(),
        };
        let compact = important_lines(
            &file.content,
            if file.name == "progress.md" {
                1200
            } else {
                1500
            },
        );
        if !compact.trim().is_empty() {
            sections.push(format!("## {}\n{}", title, compact));
        }
    }
    let mut context = sections.join("\n\n");
    if context.chars().count() > budget {
        context = truncate_chars(&context, budget);
    }
    format!(
        "You are using firstwork Planning with Files. This hidden context is for continuity only. Do not reveal it, do not copy it into the chat, and do not override the user's original request. Update planning state only through planning_update when useful.\n\n{}",
        context
    )
}

fn important_lines(content: &str, limit: usize) -> String {
    let keywords = [
        "当前任务目标",
        "验收标准",
        "当前阶段",
        "待办",
        "阻塞",
        "下一步",
        "已确认",
        "用户决策",
        "技术约束",
    ];
    let mut selected = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with('#')
                || trimmed.starts_with("- [ ]")
                || trimmed.starts_with("- [x]")
                || keywords.iter().any(|k| trimmed.contains(k))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if selected.trim().is_empty() {
        let lines: Vec<_> = content.lines().rev().take(20).collect();
        selected = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
    }
    truncate_chars(&selected, limit)
}

fn summarize_workspace(files: &[PlanningFileView]) -> (Option<String>, u8, Vec<String>) {
    let plan = files
        .iter()
        .find(|f| f.name == "plan.md")
        .map(|f| f.content.as_str())
        .unwrap_or("");
    let current_stage = plan
        .lines()
        .find(|l| l.contains("当前阶段"))
        .map(|l| l.trim().trim_start_matches('-').trim().to_string());
    let total = plan.matches("- [").count();
    let done = plan.matches("- [x]").count() + plan.matches("- [X]").count();
    let progress = if total == 0 {
        0
    } else {
        ((done * 100) / total).min(100) as u8
    };
    let blockers = plan
        .lines()
        .filter(|l| l.contains("阻塞") || l.contains("blocked") || l.contains("BLOCKED"))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.contains("暂无"))
        .take(5)
        .collect();
    (current_stage, progress, blockers)
}

fn summarize_workspace_v2(files: &[PlanningFileView]) -> (Option<String>, u8, Vec<String>) {
    let plan = files
        .iter()
        .find(|f| f.name == "plan.md")
        .map(|f| f.content.as_str())
        .unwrap_or("");
    let current_stage = plan
        .lines()
        .find(|line| {
            line.contains("当前阶段")
                || line.contains("å½“å‰é˜¶æ®µ")
                || line.contains("Current Stage")
        })
        .map(|line| line.trim().trim_start_matches('-').trim().to_string());
    let total = plan.matches("- [").count();
    let done = plan.matches("- [x]").count() + plan.matches("- [X]").count();
    let progress = if total == 0 {
        0
    } else {
        ((done * 100) / total).min(100) as u8
    };
    let blockers = plan
        .lines()
        .filter(|line| {
            line.contains("阻塞")
                || line.contains("é˜»å¡ž")
                || line.contains("blocked")
                || line.contains("BLOCKED")
        })
        .map(|line| line.trim().to_string())
        .filter(|line| !line.contains("暂无") && !line.contains("æš‚æ— "))
        .take(5)
        .collect();
    (current_stage, progress, blockers)
}

fn redact_sensitive(input: &str) -> Result<String, AppError> {
    let patterns = [
        (r"(?i)(api[_ -]?key\s*[:=]\s*)([^\s,;]+)", "$1[REDACTED]"),
        (r"(?i)(api[_ -]?secret\s*[:=]\s*)([^\s,;]+)", "$1[REDACTED]"),
        (r"(?i)(authorization\s*[:=]\s*)([^\n]+)", "$1[REDACTED]"),
        (r"(?i)(bearer\s+)([A-Za-z0-9._:\-]+)", "$1[REDACTED]"),
        (r"(?i)(token\s*[:=]\s*)([^\s,;]+)", "$1[REDACTED]"),
    ];
    let mut out = input.to_string();
    for (pattern, replacement) in patterns {
        let re = regex::Regex::new(pattern)
            .map_err(|e| AppError::Custom(format!("redaction regex error: {}", e)))?;
        out = re.replace_all(&out, replacement).to_string();
    }
    Ok(out)
}

fn extract_planning_update(text: &str) -> Result<Option<Value>, AppError> {
    let Some(idx) = text.rfind("\"planningUpdate\"") else {
        return Ok(None);
    };
    let bytes = text.as_bytes();
    let mut start = None;
    for i in (0..idx).rev() {
        if bytes[i] == b'{' {
            start = Some(i);
            break;
        }
    }
    let Some(start) = start else {
        return Ok(None);
    };
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let end = start + offset + ch.len_utf8();
                    let value: Value = serde_json::from_str(&text[start..end]).map_err(|_| {
                        AppError::InvalidInput("planningUpdate JSON 解析失败，已保留原规划".into())
                    })?;
                    validate_planning_update(&value)?;
                    return Ok(Some(value));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

fn extract_planning_tool_updates(text: &str) -> Result<Vec<Value>, AppError> {
    let mut updates = Vec::new();
    for raw in extract_json_objects_after(text, "planning_update") {
        let parsed: Value = serde_json::from_str(&raw).map_err(|_| {
            AppError::InvalidInput("planning_update arguments are not valid JSON".into())
        })?;
        updates.push(normalize_planning_update_value(parsed)?);
    }

    for block in extract_between_all(text, "<tool_calls>", "</tool_calls>") {
        for update in parse_tool_calls_block(&block)? {
            updates.push(update);
        }
    }

    if text.contains("<|DSML|>") {
        for block in extract_between_all(text, "<tool_call>", "</tool_call>") {
            for update in parse_tool_calls_block(&block)? {
                updates.push(update);
            }
        }
    }

    Ok(updates)
}

fn normalize_planning_update_value(value: Value) -> Result<Value, AppError> {
    if value.get("planningUpdate").is_some() {
        validate_planning_update(&value)?;
        return Ok(value);
    }
    let mut plan = serde_json::Map::new();
    if let Some(v) = value.get("goal") {
        plan.insert("goal".into(), v.clone());
    }
    if let Some(v) = value.get("acceptanceCriteria") {
        plan.insert("acceptanceCriteria".into(), v.clone());
    }
    if let Some(v) = value.get("stage") {
        plan.insert("currentStage".into(), v.clone());
    }
    if let Some(v) = value.get("todoUpdates") {
        plan.insert("pending".into(), v.clone());
    }
    if let Some(v) = value.get("blockers") {
        plan.insert("blockers".into(), v.clone());
    }
    if let Some(v) = value.get("nextStep") {
        plan.insert("nextStep".into(), v.clone());
    }
    let progress = json!({
        "status": value.get("status").and_then(Value::as_str).unwrap_or("partial"),
        "summary": value.get("progressNote").and_then(Value::as_str).unwrap_or("Planning updated by AI tool call.")
    });
    let normalized = json!({
        "planningUpdate": {
            "plan": Value::Object(plan),
            "findings": value.get("findings").cloned().unwrap_or_else(|| json!([])),
            "decisions": value.get("decisions").cloned().unwrap_or_else(|| json!([])),
            "progress": progress
        }
    });
    validate_planning_update(&normalized)?;
    Ok(normalized)
}

fn parse_tool_calls_block(block: &str) -> Result<Vec<Value>, AppError> {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return parse_tool_calls_value(&value);
    }
    if trimmed.contains("planning_update") {
        let mut out = Vec::new();
        for raw in extract_json_objects_after(trimmed, "planning_update") {
            let parsed: Value = serde_json::from_str(&raw).map_err(|_| {
                AppError::InvalidInput("planning_update arguments are not valid JSON".into())
            })?;
            out.push(normalize_planning_update_value(parsed)?);
        }
        return Ok(out);
    }
    Ok(Vec::new())
}

fn parse_tool_calls_value(value: &Value) -> Result<Vec<Value>, AppError> {
    let calls: Vec<&Value> = if let Some(items) = value.as_array() {
        items.iter().collect()
    } else if let Some(items) = value.get("tool_calls").and_then(Value::as_array) {
        items.iter().collect()
    } else {
        vec![value]
    };
    let mut out = Vec::new();
    for call in calls {
        let name = call
            .get("name")
            .or_else(|| call.get("function").and_then(|f| f.get("name")))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name != "planning_update" {
            continue;
        }
        let args = call
            .get("arguments")
            .or_else(|| call.get("args"))
            .or_else(|| call.get("function").and_then(|f| f.get("arguments")))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let args_value = if let Some(s) = args.as_str() {
            serde_json::from_str::<Value>(s).map_err(|_| {
                AppError::InvalidInput("planning_update arguments are not valid JSON".into())
            })?
        } else {
            args
        };
        out.push(normalize_planning_update_value(args_value)?);
    }
    Ok(out)
}

fn extract_json_objects_after(text: &str, marker: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while let Some(rel) = text[offset..].find(marker) {
        let marker_pos = offset + rel;
        let Some(start_rel) = text[marker_pos..].find('{') else {
            break;
        };
        let start = marker_pos + start_rel;
        if let Some(end) = find_matching_brace(text, start) {
            out.push(text[start..end].to_string());
            offset = end;
        } else {
            break;
        }
    }
    out
}

fn find_matching_brace(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_between_all(text: &str, start_tag: &str, end_tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(start_tag) {
        let after = &rest[start + start_tag.len()..];
        let Some(end) = after.find(end_tag) else {
            break;
        };
        out.push(after[..end].to_string());
        rest = &after[end + end_tag.len()..];
    }
    out
}

fn sanitize_visible_response(text: &str) -> String {
    let mut out = strip_dsml_protocol_blocks(text);
    out = out.replace("<|DSML|>", "");
    out = remove_between_all(&out, "<tool_calls>", "</tool_calls>");
    out = remove_between_all(&out, "<tool_call>", "</tool_call>");
    for raw in extract_json_objects_after(&out, "planning_update") {
        out = out.replace(&raw, "");
    }
    if let Ok(Some(update)) = extract_planning_update(&out) {
        out = out.replace(&update.to_string(), "");
    }
    out = out.replace("planning_update()", "");
    out.lines()
        .filter(|line| {
            let lowered = line.to_ascii_lowercase();
            !(lowered.contains("dsml")
                || lowered.contains("tool_calls")
                || lowered.contains("<tool_call")
                || lowered.contains("</tool_call")
                || lowered.contains("<invoke")
                || lowered.contains("</invoke")
                || lowered.contains("<parameter")
                || lowered.contains("</parameter"))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn strip_dsml_protocol_blocks(text: &str) -> String {
    use std::sync::OnceLock;
    static DSML_TOOL_CALLS_RE: OnceLock<regex::Regex> = OnceLock::new();
    static DSML_BLOCK_RE: OnceLock<regex::Regex> = OnceLock::new();
    static DSML_TAG_RE: OnceLock<regex::Regex> = OnceLock::new();
    let tool_calls_re = DSML_TOOL_CALLS_RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?is)<\s*\|+\s*DSML\s*\|+\s*tool_calls\b[^>]*>.*?<\s*/?\s*\|+\s*DSML\s*\|+\s*/?\s*tool_calls\s*>"#,
        )
        .expect("DSML tool_calls block regex must compile")
    });
    let block_re = DSML_BLOCK_RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?is)<\s*\|+\s*DSML\s*\|+\s*(?:tool_calls|tool_call|invoke|parameter)\b[^>]*>.*?<\s*/?\s*\|+\s*DSML\s*\|+\s*/?\s*(?:tool_calls|tool_call|invoke|parameter)\s*>"#,
        )
        .expect("DSML pseudo-tool block regex must compile")
    });
    let tag_re = DSML_TAG_RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?is)<\s*/?\s*\|+\s*DSML\s*\|+\s*/?\s*(?:tool_calls|tool_call|invoke|parameter)\b[^>]*>"#,
        )
        .expect("DSML pseudo-tool tag regex must compile")
    });
    let without_tool_calls = tool_calls_re.replace_all(text, "");
    let without_blocks = block_re.replace_all(&without_tool_calls, "");
    tag_re.replace_all(&without_blocks, "").to_string()
}

fn remove_between_all(text: &str, start_tag: &str, end_tag: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(start_tag) {
        out.push_str(&rest[..start]);
        let after = &rest[start + start_tag.len()..];
        let Some(end) = after.find(end_tag) else {
            return out.trim().to_string();
        };
        rest = &after[end + end_tag.len()..];
    }
    out.push_str(rest);
    out
}

fn emit_workspace_updated(
    app: &AppHandle,
    workspace: &PlanningWorkspace,
    changed_sections: Vec<String>,
) {
    let payload = json!({
        "workspaceId": workspace.workspace_path,
        "conversationId": workspace.session_id,
        "sessionKind": workspace.session_kind,
        "sessionId": workspace.session_id,
        "revision": Local::now().timestamp_millis(),
        "updatedAt": workspace.last_updated_at,
        "changedSections": changed_sections,
        "currentStage": workspace.current_stage,
        "progressPercent": workspace.progress_percent,
    });
    let _ = app.emit("planning://updated", payload.clone());
    let _ = app.emit("planning:updated", payload);
}

fn validate_planning_update(value: &Value) -> Result<(), AppError> {
    let update = value
        .get("planningUpdate")
        .ok_or_else(|| AppError::InvalidInput("缺少 planningUpdate 字段".into()))?;
    if !update.is_object() {
        return Err(AppError::InvalidInput("planningUpdate 必须是对象".into()));
    }
    if let Some(plan) = update.get("plan") {
        if !plan.is_object() {
            return Err(AppError::InvalidInput(
                "planningUpdate.plan 必须是对象".into(),
            ));
        }
    }
    if let Some(findings) = update.get("findings") {
        if !findings.is_array() {
            return Err(AppError::InvalidInput(
                "planningUpdate.findings 必须是数组".into(),
            ));
        }
    }
    if let Some(progress) = update.get("progress") {
        if !progress.is_object() {
            return Err(AppError::InvalidInput(
                "planningUpdate.progress 必须是对象".into(),
            ));
        }
    }
    Ok(())
}

fn apply_update_to_files(workspace: &Path, value: &Value) -> Result<(), AppError> {
    let update = value
        .get("planningUpdate")
        .ok_or_else(|| AppError::InvalidInput("缺少 planningUpdate 字段".into()))?;
    if let Some(plan) = update.get("plan") {
        let current = read_limited_utf8(&workspace.join("plan.md"))?;
        let mut next = String::new();
        next.push_str("# Plan\n\n");
        if let Some(v) = plan.get("goal").and_then(|v| v.as_str()) {
            next.push_str(&format!("- Goal: {}\n", redact_sensitive(v)?));
        }
        if let Some(v) = plan.get("acceptanceCriteria") {
            next.push_str("\n## Acceptance Criteria\n");
            append_plain_array(&mut next, Some(v))?;
        }
        if let Some(v) = plan.get("currentStage").and_then(|v| v.as_str()) {
            next.push_str(&format!("- 当前阶段：{}\n", redact_sensitive(v)?));
        }
        if let Some(v) = plan.get("nextStep").and_then(|v| v.as_str()) {
            next.push_str(&format!("- 下一步：{}\n", redact_sensitive(v)?));
        }
        next.push_str("\n## 已完成事项\n");
        append_string_array(&mut next, plan.get("completed"), true)?;
        next.push_str("\n## 待办事项\n");
        append_string_array(&mut next, plan.get("pending"), false)?;
        next.push_str("\n## 阻塞问题\n");
        append_plain_array(&mut next, plan.get("blockers"))?;
        next.push_str("\n\n---\n\n## 上一版摘录\n");
        next.push_str(&truncate_chars(&current, 1200));
        atomic_write(&workspace.join("plan.md"), &next)?;
    }
    if let Some(findings) = update.get("findings") {
        let mut chunk = format!("\n\n## {}\n", Local::now().format("%Y-%m-%d %H:%M:%S"));
        append_plain_array(&mut chunk, Some(findings))?;
        append_file(&workspace.join("findings.md"), &chunk)?;
    }
    if let Some(decisions) = update.get("decisions") {
        let mut chunk = format!(
            "\n\n## Decisions {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        append_plain_array(&mut chunk, Some(decisions))?;
        append_file(&workspace.join("findings.md"), &chunk)?;
    }
    if let Some(progress) = update.get("progress") {
        let status = progress
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("partial");
        let summary = progress
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("AI 提出了规划更新。");
        let chunk = format!(
            "\n\n## {}\n- 状态：{}\n- 摘要：{}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            redact_sensitive(status)?,
            redact_sensitive(summary)?
        );
        append_file(&workspace.join("progress.md"), &chunk)?;
    }
    Ok(())
}

fn append_string_array(
    out: &mut String,
    value: Option<&Value>,
    done: bool,
) -> Result<(), AppError> {
    if let Some(items) = value.and_then(|v| v.as_array()) {
        for item in items.iter().filter_map(|v| v.as_str()) {
            out.push_str(if done { "- [x] " } else { "- [ ] " });
            out.push_str(&redact_sensitive(item)?);
            out.push('\n');
        }
    } else {
        out.push_str("- 暂无\n");
    }
    Ok(())
}

fn append_plain_array(out: &mut String, value: Option<&Value>) -> Result<(), AppError> {
    if let Some(items) = value.and_then(|v| v.as_array()) {
        for item in items.iter().filter_map(|v| v.as_str()) {
            out.push_str("- ");
            out.push_str(&redact_sensitive(item)?);
            out.push('\n');
        }
    } else {
        out.push_str("- 暂无\n");
    }
    Ok(())
}

fn truncate_chars(input: &str, max: usize) -> String {
    let mut out: String = input.chars().take(max).collect();
    if input.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use uuid::Uuid;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("firstwork-planning-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn temp_db() -> (Database, TestDir) {
        let dir = TestDir::new();
        let db_path = dir.path().join("dev.db").to_string_lossy().to_string();
        let db = Database::init(&db_path).unwrap();
        (db, dir)
    }

    fn install_ready_plugin(db: &Database) {
        let conn = db.conn_lock().unwrap();
        let manifest_json = format!(
            r#"{{"id":"{}","name":"Planning with Files","version":"1.0.0","description":"Official planning workspace plugin","author":"firstwork","main":"main.js","permissions":["ai.context.read","ai.context.augment","ai.session.read","planning.files.read","planning.files.write","ui.chat.toolbar","ui.chat.panel"],"contributes":{{}}}}"#,
            PLANNING_PLUGIN_ID
        );
        conn.execute(
            "INSERT OR REPLACE INTO plugins
                (id, name, version, path, main, manifest_json, enabled, status, content_hash)
             VALUES (?1, 'Planning', '1.0.0', 'builtin', 'main.js', ?2, 1, 'installed', 'hash')",
            params![PLANNING_PLUGIN_ID, manifest_json],
        )
        .unwrap();
        for permission in [
            "ai.context.read",
            "ai.context.augment",
            "ai.session.read",
            "planning.files.read",
            "planning.files.write",
            "ui.chat.toolbar",
            "ui.chat.panel",
        ] {
            conn.execute(
                "INSERT OR REPLACE INTO plugin_permissions (plugin_id, permission, granted)
                 VALUES (?1, ?2, 1)",
                params![PLANNING_PLUGIN_ID, permission],
            )
            .unwrap();
        }
    }

    #[test]
    fn new_session_defaults_disabled() {
        let (db, dir) = temp_db();
        let ws =
            PlanningService::get_workspace(&db, dir.path(), PlanningSessionKind::Ai, "1").unwrap();
        assert!(!ws.enabled);
    }

    #[test]
    fn enable_creates_three_files_and_restores() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        let ws = PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true)
            .unwrap();
        assert!(ws.enabled);
        assert_eq!(ws.files.len(), 3);
        assert!(Path::new(&ws.workspace_path).join("plan.md").exists());
        let again =
            PlanningService::get_workspace(&db, dir.path(), PlanningSessionKind::Ai, "1").unwrap();
        assert!(again.enabled);
    }

    #[test]
    fn session_isolation_and_path_traversal_rejected() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true).unwrap();
        assert!(PlanningService::set_enabled(
            &db,
            dir.path(),
            PlanningSessionKind::Agent,
            "../bad",
            true
        )
        .is_err());
        let other =
            PlanningService::get_workspace(&db, dir.path(), PlanningSessionKind::Ai, "2").unwrap();
        assert!(!other.enabled);
    }

    #[test]
    fn oversize_and_secret_redaction() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true).unwrap();
        let huge = "x".repeat(MAX_FILE_BYTES + 1);
        assert!(PlanningService::save_file(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "plan.md",
            &huge
        )
        .is_err());
        let ws = PlanningService::save_file(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "findings.md",
            "api_key=secret123 Authorization: Bearer abc",
        )
        .unwrap();
        let findings = ws.files.iter().find(|f| f.name == "findings.md").unwrap();
        assert!(!findings.content.contains("secret123"));
        assert!(!findings.content.contains("Bearer abc"));
    }

    #[test]
    fn structured_update_requires_confirmation() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true).unwrap();
        let answer = r#"正常回答
{"planningUpdate":{"plan":{"currentStage":"开发","completed":["分析"],"pending":["测试"],"blockers":[],"nextStep":"运行验证"},"findings":["已确认可复用"],"progress":{"status":"partial","summary":"完成分析"}}}"#;
        PlanningService::record_completion(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "completed",
            "做计划",
            answer,
        )
        .unwrap();
        let ws =
            PlanningService::get_workspace(&db, dir.path(), PlanningSessionKind::Ai, "1").unwrap();
        assert!(ws.pending_update.is_some());
        let before = ws
            .files
            .iter()
            .find(|f| f.name == "plan.md")
            .unwrap()
            .content
            .clone();
        PlanningService::apply_update(&db, dir.path(), PlanningSessionKind::Ai, "1", false)
            .unwrap();
        let rejected =
            PlanningService::get_workspace(&db, dir.path(), PlanningSessionKind::Ai, "1").unwrap();
        assert_eq!(
            before,
            rejected
                .files
                .iter()
                .find(|f| f.name == "plan.md")
                .unwrap()
                .content
        );
        PlanningService::record_completion(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "completed",
            "做计划",
            answer,
        )
        .unwrap();
        let applied =
            PlanningService::apply_update(&db, dir.path(), PlanningSessionKind::Ai, "1", true)
                .unwrap();
        assert!(applied
            .files
            .iter()
            .find(|f| f.name == "plan.md")
            .unwrap()
            .content
            .contains("运行验证"));
    }

    #[test]
    fn planning_update_tool_call_applies_without_visible_protocol() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true).unwrap();
        let answer = r#"Here is the plan.
<|DSML|><tool_calls>[{"name":"planning_update","arguments":{"goal":"Robot rescue plan","stage":"Week 1 setup","todoUpdates":["Define milestones","Build vision-to-motion MVP"],"findings":["Deadline is week 2"],"decisions":["Use staged validation"],"progressNote":"Initial plan created","nextStep":"Confirm resources"}}]</tool_calls>"#;
        PlanningService::record_completion(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "completed",
            "make a plan",
            answer,
        )
        .unwrap();
        let ws =
            PlanningService::get_workspace(&db, dir.path(), PlanningSessionKind::Ai, "1").unwrap();
        assert!(ws.pending_update.is_none());
        let plan = &ws
            .files
            .iter()
            .find(|f| f.name == "plan.md")
            .unwrap()
            .content;
        assert!(plan.contains("Robot rescue plan"));
        assert!(plan.contains("Week 1 setup"));
        let findings = &ws
            .files
            .iter()
            .find(|f| f.name == "findings.md")
            .unwrap()
            .content;
        assert!(findings.contains("Deadline is week 2"));
        assert_eq!(
            PlanningService::sanitize_visible_response(answer),
            "Here is the plan."
        );
    }

    #[test]
    fn planning_update_function_syntax_is_hidden() {
        let text = r#"Natural reply.
planning_update({"goal":"Hidden","progressNote":"Do not render"})"#;
        let updates = extract_planning_tool_updates(text).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            PlanningService::sanitize_visible_response(text),
            "Natural reply."
        );
    }

    #[test]
    fn compact_dsml_tool_protocol_is_hidden() {
        let text = r#"Before
<||DSML||tool_calls><||DSML||invoke name="mcp_kb_mcp_list_notes"><||DSML||parameter name="limit" string="false">100</||DSML||parameter></||DSML||invoke></||DSML||tool_calls>
After"#;
        let cleaned = PlanningService::sanitize_visible_response(text);
        assert!(cleaned.contains("Before"));
        assert!(cleaned.contains("After"));
        assert!(!cleaned.contains("DSML"));
        assert!(!cleaned.contains("mcp_kb_mcp_list_notes"));
    }

    #[test]
    fn spaced_dsml_tool_protocol_is_hidden() {
        let text = r#"找到了资源和预算笔记。让我读取它。
< | DSML | tool_calls>< | DSML | invoke name="get_note">
< | DSML | parameter name="id" string="false">11</ | DSML | parameter>
</ | DSML | invoke></ | DSML | tool_calls>"#;
        let cleaned = PlanningService::sanitize_visible_response(text);
        assert_eq!(cleaned, "找到了资源和预算笔记。让我读取它。");
        assert!(!cleaned.contains("DSML"));
        assert!(!cleaned.contains("get_note"));
    }

    #[test]
    fn activity_sync_advances_stage_and_progress() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true).unwrap();
        let workspace = workspace_path(dir.path(), &PlanningSessionKind::Ai, "1").unwrap();
        append_activity_to_files(&workspace, "started", "AI started working").unwrap();
        let ws =
            PlanningService::get_workspace(&db, dir.path(), PlanningSessionKind::Ai, "1").unwrap();
        assert!(ws.current_stage.unwrap_or_default().contains("执行中"));
        assert!(ws.progress_percent > 0);
    }

    #[test]
    fn disabled_session_does_not_inject_context() {
        let (db, dir) = temp_db();
        let ctx = PlanningService::build_context(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "hello",
            PlanningProviderMode::TextPrefix,
        )
        .unwrap();
        assert!(!ctx.enabled);
        assert_eq!(ctx.content, "hello");
    }

    #[test]
    fn enabled_text_prefix_preserves_user_request() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true).unwrap();
        PlanningService::save_file(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "plan.md",
            "## 当前任务目标\n完成规划插件\n\n## 下一步\n运行测试",
        )
        .unwrap();
        let ctx = PlanningService::build_context(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "继续推进",
            PlanningProviderMode::TextPrefix,
        )
        .unwrap();
        assert!(ctx.enabled);
        assert!(ctx.content.contains("[PLANNING CONTEXT]"));
        assert!(ctx.content.contains("[USER REQUEST]\n继续推进"));
    }

    #[test]
    fn plugin_disabled_cannot_write() {
        let (db, dir) = temp_db();
        install_ready_plugin(&db);
        PlanningService::set_enabled(&db, dir.path(), PlanningSessionKind::Ai, "1", true).unwrap();
        {
            let conn = db.conn_lock().unwrap();
            conn.execute(
                "UPDATE plugins SET enabled = 0 WHERE id = ?1",
                params![PLANNING_PLUGIN_ID],
            )
            .unwrap();
        }
        assert!(PlanningService::save_file(
            &db,
            dir.path(),
            PlanningSessionKind::Ai,
            "1",
            "plan.md",
            "should fail",
        )
        .is_err());
    }
}
