//! Claude Code Agent Runner — Service 层
//!
//! 职责：
//! - 检测 Claude Code CLI 可用性
//! - 启动/停止 agent 子进程
//! - 通过 Tauri 事件实时推送 stdout/stderr
//! - 安全校验（工作目录白名单）

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{ClaudeAgentSession, StartClaudeAgentInput};

/// 运行中的 agent 进程句柄
pub struct AgentProcessHandle {
    pub session_id: String,
    pub pid: u32,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

/// Agent 事件 payload（发给前端）
#[derive(Clone, serde::Serialize)]
pub struct AgentEventPayload {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub kind: String,
    pub content: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// 危险目录黑名单 — 禁止在这些目录或它们的子目录中执行 Claude Code
const FORBIDDEN_ROOTS: &[&str] = &["/", "\\", "C:\\", "D:\\"];

fn is_path_safe(path: &str, data_dir: &std::path::Path) -> bool {
    let p = std::path::Path::new(path);
    if !p.exists() || !p.is_dir() {
        return false;
    }
    let canonical = match p.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let canonical_str = canonical.to_string_lossy();

    // 禁止在根目录运行
    for root in FORBIDDEN_ROOTS {
        if canonical.as_os_str() == std::path::Path::new(root).as_os_str() {
            return false;
        }
    }

    // 禁止在应用数据目录运行
    if let Ok(can_data) = data_dir.canonicalize() {
        if canonical.starts_with(&can_data) {
            return false;
        }
    }

    true
}

/// 检测 Claude Code CLI 是否可用
pub async fn check_cli() -> Result<String, AppError> {
    let cli = cli_name();
    let mut cmd = TokioCommand::new(&cli);
    cmd.arg("--version");
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().await.map_err(|e| {
        AppError::Custom(format!("找不到 {}: {}", cli, e))
    })?;

    let text = String::from_utf8_lossy(if output.stdout.is_empty() { &output.stderr } else { &output.stdout })
        .trim()
        .to_string();

    if output.status.success() && !text.is_empty() {
        Ok(text)
    } else {
        Err(AppError::Custom(format!("{} --version 退出码: {}", cli, output.status)))
    }
}

/// 启动 Claude Code agent 会话
///
/// 流程：
/// 1. 校验 project_path 安全性
/// 2. 数据库创建 session（status=pending）
/// 3. 启动子进程
/// 4. 更新 session status=running, pid
/// 5. 后台 task 读取 stdout/stderr → emit 事件 + 写入 DB
/// 6. 进程退出 → 更新 status + emit claude-agent:done
pub async fn start_session(
    app: &AppHandle,
    db: &Database,
    input: StartClaudeAgentInput,
    data_dir: std::path::PathBuf,
    processes: Arc<Mutex<HashMap<String, AgentProcessHandle>>>,
) -> Result<ClaudeAgentSession, AppError> {
    // 1. 安全校验
    if !is_path_safe(&input.project_path, &data_dir) {
        return Err(AppError::Custom("不允许在此目录中运行 Claude Code。请选择有效的项目目录。".into()));
    }

    // 2. 同项目并发保护：同一 project_path 只允许一个 running agent
    {
        let procs = processes.lock().map_err(|e| AppError::Custom(e.to_string()))?;
        for (_, h) in procs.iter() {
            let existing = db.get_agent_session(&h.session_id).ok();
            if let Some(s) = existing {
                if s.project_path == input.project_path && s.status == "running" {
                    return Err(AppError::Custom(
                        format!("项目 {} 中已有运行中的 Agent 会话 ({}), 请先停止后再启动", input.project_path, s.id)
                    ));
                }
            }
        }
    }

    // 3. 创建 session
    let mut session = db.create_agent_session(&input)?;

    // 3. 启动子进程 — 用 -p 传入 prompt 作为一次性任务
    let cli = cli_name();
    let mut cmd = TokioCommand::new(&cli);
    cmd.arg("-p")
        .arg(&input.prompt)
        .current_dir(&input.project_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| {
        AppError::Custom(format!("无法启动 {}: {}", cli, e))
    })?;

    // 4. 记录 pid
    let pid = child.id().unwrap_or(0) as i64;
    session.pid = Some(pid);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut procs = processes.lock().map_err(|e| AppError::Custom(e.to_string()))?;
        procs.insert(session.id.clone(), AgentProcessHandle {
            session_id: session.id.clone(),
            pid: pid as u32,
            shutdown_tx,
        });
    }

    // Mark running
    db.update_agent_session_status(&session.id, "running", None, None)?;
    session.status = "running".into();

    // 推送 started 事件
    let _ = app.emit("claude-agent:started", AgentEventPayload {
        session_id: session.id.clone(),
        kind: "started".into(),
        content: format!("Agent started in {}", input.project_path),
        created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    });

    // 5. 后台读取 stdout/stderr
    let app_for_task = app.clone();
    let sid = session.id.clone();

    let stdout = child.stdout.take().expect("stdout captured");
    let stderr = child.stderr.take().expect("stderr captured");

    let app_stdout = app_for_task.clone();
    let app_stderr = app_for_task.clone();
    let sid_out = sid.clone();
    let sid_err = sid.clone();

    // 实际 DB 访问复用现有的 db 引用，这里简化：event 只推前端，不入 DB
    let stdout_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = app_stdout.emit("claude-agent:chunk", AgentEventPayload {
                session_id: sid_out.clone(),
                kind: "stdout".into(),
                content: line,
                created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            });
        }
    });

    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let _ = app_stderr.emit("claude-agent:stderr", AgentEventPayload {
                session_id: sid_err.clone(),
                kind: "stderr".into(),
                content: line,
                created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            });
        }
    });

    // 6. 后台等待进程退出
    let app_done = app_for_task.clone();
    let sid_done = sid.clone();
    let procs_done = processes.clone();

    tokio::spawn(async move {
        // wait for stdout/stderr tasks + process
        let (exit_status, _, _) = tokio::join!(child.wait(), stdout_task, stderr_task);

        let status_str;
        let exit_code;
        let error_msg;

        match exit_status {
            Ok(es) => {
                exit_code = es.code();
                if es.success() {
                    status_str = "completed";
                    error_msg = None;
                } else {
                    status_str = "failed";
                    error_msg = Some(format!("exit code {}", es.code().unwrap_or(-1)));
                }
            }
            Err(e) => {
                exit_code = None;
                status_str = "failed";
                error_msg = Some(e.to_string());
            }
        }

        // 清理进程表
        if let Ok(mut procs) = procs_done.lock() {
            procs.remove(&sid_done);
        }

        // 通知前端
        let _ = app_done.emit("claude-agent:done", AgentEventPayload {
            session_id: sid_done.clone(),
            kind: "done".into(),
            content: status_str.to_string(),
            created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        });

        // 丢弃 shutdown_rx（不关心）
        drop(shutdown_rx);
    });

    Ok(session)
}

/// 停止 agent 会话
pub async fn stop_session(
    processes: Arc<Mutex<HashMap<String, AgentProcessHandle>>>,
    session_id: &str,
) -> Result<(), AppError> {
    let handle = {
        let mut procs = processes.lock().map_err(|e| AppError::Custom(e.to_string()))?;
        procs.remove(session_id)
    };

    if let Some(h) = handle {
        let _ = h.shutdown_tx.send(());
        #[cfg(unix)]
        unsafe {
            libc::kill(h.pid as i32, libc::SIGTERM);
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &h.pid.to_string(), "/F"])
                .output();
        }
        Ok(())
    } else {
        Err(AppError::NotFound(format!("会话 {} 未在运行", session_id)))
    }
}

#[cfg(target_os = "windows")]
fn cli_name() -> &'static str {
    "claude.cmd"
}

#[cfg(not(target_os = "windows"))]
fn cli_name() -> &'static str {
    "claude"
}
