//! T-013 自定义数据目录 — Tauri Commands
//!
//! 暴露给前端：
//! - `get_data_dir_info` 读当前/默认/指针/来源（设置页 UI 显示用）
//! - `set_pending_data_dir` 写指针文件（重启生效）
//! - `clear_pending_data_dir` 清指针文件（恢复默认；重启生效）
//!
//! 所有 Command 都通过 `crate::framework_app_data_dir` 获取 framework 根目录，
//! 保证 dev 模式走 `-dev` 隔离目录、不污染 prod 的指针/迁移 marker。

use serde::Serialize;

use crate::services::data_dir::{DataDirResolver, MigrationMarker, ResolvedDataDir};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDataDirectory {
    pub path: String,
    pub source: String,
    pub exists: bool,
    pub database_path: String,
    pub writable: bool,
}

#[tauri::command]
pub fn get_data_dir_info(app: tauri::AppHandle) -> Result<ResolvedDataDir, String> {
    let app_data_dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    DataDirResolver::resolve(&app_data_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_runtime_data_directory(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RuntimeDataDirectory, String> {
    let data_dir = state.data_dir.clone();
    let database_name = if cfg!(debug_assertions) {
        "dev-app.db"
    } else {
        "app.db"
    };
    let database_path = data_dir.join(database_name);
    let exists = data_dir.is_dir();
    let writable = test_writable(&data_dir).map_err(|e| e.to_string())?;
    let source = runtime_data_dir_source(&app, &data_dir);

    Ok(RuntimeDataDirectory {
        path: data_dir.to_string_lossy().to_string(),
        source,
        exists,
        database_path: database_path.to_string_lossy().to_string(),
        writable,
    })
}

#[tauri::command]
pub fn set_pending_data_dir(app: tauri::AppHandle, new_path: String) -> Result<(), String> {
    let app_data_dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    DataDirResolver::set_pending(&app_data_dir, &new_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_pending_data_dir(app: tauri::AppHandle) -> Result<(), String> {
    let app_data_dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    DataDirResolver::clear_pending(&app_data_dir).map_err(|e| e.to_string())
}

/// T-013 完整版：写指针 + 写迁移 marker，让重启时自动迁移
///
/// `from_dir` 是当前使用的数据目录（即 AppState.data_dir），由前端从 get_data_dir_info 得到。
/// **不在 from_dir 默认实例情况下使用 AppState.data_dir 作为 from**：因为多开实例的
/// data_dir 是 instance-N 子目录，迁移整库还是要从父目录复制。所以前端传 framework
/// 默认 app_data_dir 还是用户当前自定义路径作为 from——保持简单，由前端控制。
#[tauri::command]
pub fn set_pending_data_dir_with_migration(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    new_path: String,
) -> Result<(), String> {
    let app_data_dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    // from = 当前实际数据根（多开实例情况下也是当前实例所在）
    let from_dir = state.data_dir.clone();
    DataDirResolver::set_pending_with_migration(&app_data_dir, &from_dir, &new_path)
        .map_err(|e| e.to_string())
}

/// 取消未执行的迁移（用户在重启前后悔了；删指针 + 删 marker）
#[tauri::command]
pub fn cancel_pending_migration(app: tauri::AppHandle) -> Result<(), String> {
    let app_data_dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    DataDirResolver::cancel_migration(&app_data_dir).map_err(|e| e.to_string())
}

/// 读迁移 marker（splash 窗口启动时查初始状态用）
#[tauri::command]
pub fn get_migration_marker(app: tauri::AppHandle) -> Result<Option<MigrationMarker>, String> {
    let app_data_dir = crate::framework_app_data_dir(&app).map_err(|e| e.to_string())?;
    DataDirResolver::read_migration_marker(&app_data_dir).map_err(|e| e.to_string())
}

fn runtime_data_dir_source(app: &tauri::AppHandle, data_dir: &std::path::Path) -> String {
    if std::env::var("KB_DATA_DIR")
        .ok()
        .map(|value| std::path::PathBuf::from(value.trim()) == data_dir)
        .unwrap_or(false)
    {
        return "KB_DATA_DIR".to_string();
    }

    if let Ok(framework_dir) = crate::framework_app_data_dir(app) {
        if let Ok(Some(scope)) =
            crate::services::data_dir::DataDirResolver::read_active_account_scope(&framework_dir)
        {
            if let Ok(expected) = crate::services::data_dir::DataDirResolver::account_data_dir(
                &framework_dir,
                &scope.platform_user_id,
            ) {
                if expected == data_dir {
                    return "account".to_string();
                }
            }
        }
        if data_dir == framework_dir {
            return "default".to_string();
        }
        if framework_dir
            .join(crate::services::data_dir::POINTER_FILE)
            .is_file()
        {
            return "data_dir.txt".to_string();
        }
    }

    "AppState".to_string()
}

fn test_writable(data_dir: &std::path::Path) -> Result<bool, std::io::Error> {
    if !data_dir.is_dir() {
        return Ok(false);
    }
    let probe = data_dir.join(format!(
        ".runtime-data-write-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&probe, b"ok")?;
    let _ = std::fs::remove_file(probe);
    Ok(true)
}
