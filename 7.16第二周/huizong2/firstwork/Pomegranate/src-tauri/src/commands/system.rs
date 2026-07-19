use tauri::State;

use crate::models::{DailyWritingStat, DashboardStats, GitInfo, SystemInfo};
use crate::services::asset_path;
use crate::services::image::ImageService;
use crate::state::AppState;

/// 把笔记里的相对资产路径（kb-asset:// 后那段）还原成绝对路径。
///
/// 用途：附件链接需要走 OS opener 打开（必须传绝对路径）；其它素材渲染走 asset 协议
/// 不需要这个 Command，前端 `convertFileSrc` 自己拼即可。
///
/// 安全：拒绝含 `..` 或绝对前缀的输入，强制限定在 data_dir 之内。
#[tauri::command]
pub fn resolve_asset_absolute_path(
    state: State<'_, AppState>,
    rel: String,
) -> Result<String, String> {
    let abs = asset_path::rel_to_abs(&rel, &state.data_dir)?;
    Ok(abs.to_string_lossy().into_owned())
}

/// 获取系统信息
///
/// data_dir / images_dir 都从 state 取，保证多开实例下返回的是当前实例自己的目录
/// （而不是被所有实例共享的 app_data_dir 根）。
#[tauri::command]
pub fn get_system_info(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SystemInfo, String> {
    let data_dir = state.data_dir.to_string_lossy().into_owned();
    let images_dir = ImageService::images_dir(&state.data_dir)
        .to_string_lossy()
        .into_owned();

    Ok(SystemInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        app_version: app.package_info().version.to_string(),
        data_dir,
        images_dir,
        instance_id: state.instance_id,
        is_dev: cfg!(debug_assertions),
    })
}

/// 获取首页统计数据
#[tauri::command]
pub fn get_dashboard_stats(state: State<'_, AppState>) -> Result<DashboardStats, String> {
    state.db.get_dashboard_stats().map_err(|e| e.to_string())
}

/// 获取写作趋势（最近 N 天）
#[tauri::command]
pub fn get_writing_trend(
    state: State<'_, AppState>,
    days: Option<i32>,
) -> Result<Vec<DailyWritingStat>, String> {
    state
        .db
        .get_writing_trend(days.unwrap_or(30))
        .map_err(|e| e.to_string())
}

/// 简单的 greet 命令（保留为示例）
#[tauri::command]
pub fn greet(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("名称不能为空".into());
    }
    Ok(format!("Hello, {}! 来自 Rust 的问候!", name))
}

/// 查询是否允许多开实例。
/// flag 文件位于 framework_app_data_dir 根（与单实例锁同目录），
/// 在 Tauri Builder 启动前由 lib.rs 读取以决定是否拒绝第二个进程。
/// dev 模式下走 `-dev` 隔离目录，避免污染 prod 设置。
#[tauri::command]
pub fn get_multi_instance_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    let dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    Ok(crate::is_multi_instance_enabled(&dir))
}

/// 切换"允许多开实例"开关。下次启动生效（当前进程的实例锁不会变）。
#[tauri::command]
pub fn set_multi_instance_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    crate::set_multi_instance_enabled(&dir, enabled).map_err(|e| e.to_string())
}

/// 把任意文本写入指定路径（UTF-8）。前端"导出 SVG"等小工具用。
///
/// Tauri 2 的 WebView 默认拦截 `<a download>` 触发的下载，所以只读视图里的
/// "导出"按钮无法走纯前端方案，必须经 Rust 写盘。前端先调 `tauri-plugin-dialog`
/// 的 `save()` 获取目标路径，再把内容传到这里。
///
/// 安全：路径由用户在原生 Save 对话框中选定，不接受相对路径或拼接；调用方传啥写啥。
#[tauri::command]
pub fn write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败 {}: {}", path, e))
}

/// 获取当前工作目录的 Git 仓库状态快照。
///
/// 通过 shell out `git` CLI 实现，不引入额外 crate。
/// 非 git 仓库时返回 Default（branch: None, is_clean: true）。
#[tauri::command]
pub fn get_git_info() -> Result<GitInfo, String> {
    fn run_git(args: &[&str]) -> Option<String> {
        let mut cmd = std::process::Command::new("git");
        cmd.args(args);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let output = cmd.output().ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    // 1) 当前分支
    let branch = run_git(&["branch", "--show-current"]);

    // 无分支 = 非 git 仓库（或 detached HEAD）
    let branch = branch.filter(|b| !b.is_empty());

    // 2) git status --porcelain 解析文件状态
    let mut changed = 0i32;
    let mut staged = 0i32;
    let mut untracked = 0i32;
    if let Some(porcelain) = run_git(&["status", "--porcelain"]) {
        for line in porcelain.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let bytes = line.as_bytes();
            if bytes.len() < 2 {
                continue;
            }
            let x = bytes[0]; // 暂存区状态
            let y = bytes[1]; // 工作区状态
                              // 未跟踪
            if x == b'?' && y == b'?' {
                untracked += 1;
                continue;
            }
            // 暂存区有变更
            if x != b' ' {
                staged += 1;
            }
            // 工作区有变更
            if y != b' ' {
                changed += 1;
            }
        }
    }

    // 3) 领先/落后远程提交数
    let (ahead, behind) = if let Some(counts) =
        run_git(&["rev-list", "--count", "--left-right", "@{upstream}...HEAD"])
    {
        let parts: Vec<&str> = counts.split_whitespace().collect();
        let a = parts
            .first()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);
        let b = parts
            .get(1)
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);
        (a, b)
    } else {
        (0, 0)
    };

    let is_clean = changed == 0 && staged == 0 && untracked == 0;

    Ok(GitInfo {
        branch,
        is_clean,
        changed,
        staged,
        untracked,
        ahead,
        behind,
    })
}
