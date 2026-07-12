//! 插件系统 Commands
//!
//! 薄 IPC 包装：接收参数 → 调用 PluginService → 转换错误。

use std::collections::HashMap;

use tauri::State;

use crate::models::{PluginInfo, PluginManifest};
use crate::services::plugins::PluginService;
use crate::state::AppState;

/// 列出已安装插件
#[tauri::command]
pub fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    PluginService::list(&state.db).map_err(|e| e.to_string())
}

/// 扫描应用数据目录下的 plugins 文件夹，并同步 manifest 到数据库
#[tauri::command]
pub fn scan_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    PluginService::scan(&state.db, &state.data_dir).map_err(|e| e.to_string())
}

/// 从本地目录安装插件
#[tauri::command]
pub fn install_plugin_from_dir(
    state: State<'_, AppState>,
    path: String,
) -> Result<PluginInfo, String> {
    PluginService::install_from_dir(&state.db, &state.data_dir, &path).map_err(|e| e.to_string())
}

/// 启用插件
#[tauri::command]
pub fn enable_plugin(state: State<'_, AppState>, plugin_id: String) -> Result<(), String> {
    PluginService::enable(&state.db, &plugin_id).map_err(|e| e.to_string())
}

/// 禁用插件
#[tauri::command]
pub fn disable_plugin(state: State<'_, AppState>, plugin_id: String) -> Result<(), String> {
    PluginService::disable(&state.db, &plugin_id).map_err(|e| e.to_string())
}

/// 卸载插件
#[tauri::command]
pub fn uninstall_plugin(state: State<'_, AppState>, plugin_id: String) -> Result<bool, String> {
    PluginService::uninstall(&state.db, &state.data_dir, &plugin_id).map_err(|e| e.to_string())
}

/// 获取插件 manifest
#[tauri::command]
pub fn get_plugin_manifest(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<PluginManifest, String> {
    PluginService::get_manifest(&state.db, &plugin_id).map_err(|e| e.to_string())
}

/// 授权插件权限
#[tauri::command]
pub fn grant_plugin_permissions(
    state: State<'_, AppState>,
    plugin_id: String,
    permissions: Vec<String>,
) -> Result<usize, String> {
    PluginService::grant_permissions(&state.db, &plugin_id, permissions).map_err(|e| e.to_string())
}

/// 撤销插件权限
#[tauri::command]
pub fn revoke_plugin_permissions(
    state: State<'_, AppState>,
    plugin_id: String,
    permissions: Vec<String>,
) -> Result<usize, String> {
    PluginService::revoke_permissions(&state.db, &plugin_id, permissions).map_err(|e| e.to_string())
}

/// 获取插件设置
#[tauri::command]
pub fn get_plugin_settings(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<HashMap<String, serde_json::Value>, String> {
    PluginService::get_settings(&state.db, &plugin_id).map_err(|e| e.to_string())
}

/// 全量保存插件设置
#[tauri::command]
pub fn set_plugin_settings(
    state: State<'_, AppState>,
    plugin_id: String,
    settings: serde_json::Value,
) -> Result<(), String> {
    PluginService::set_settings(&state.db, &plugin_id, settings).map_err(|e| e.to_string())
}

/// 读取插件目录内的文本资源
#[tauri::command]
pub fn read_plugin_asset(
    state: State<'_, AppState>,
    plugin_id: String,
    relative_path: String,
) -> Result<String, String> {
    PluginService::read_asset(&state.db, &state.data_dir, &plugin_id, &relative_path)
        .map_err(|e| e.to_string())
}
