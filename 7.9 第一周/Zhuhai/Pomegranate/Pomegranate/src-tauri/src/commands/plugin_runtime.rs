//! Plugin runtime token lifecycle commands.

use tauri::State;

use crate::services::plugins::PluginService;
use crate::state::AppState;

#[tauri::command]
pub fn plugin_acquire_token(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<String, String> {
    let plugin = state.db.get_plugin(&plugin_id).map_err(|e| e.to_string())?;

    if plugin.status != "installed" {
        return Err(format!("插件 {} 状态非 installed", plugin_id));
    }
    if !plugin.enabled {
        return Err(format!("插件 {} 已禁用", plugin_id));
    }

    let policy =
        PluginService::can_execute_runtime(&state.db, &plugin_id).map_err(|e| e.to_string())?;
    if !policy.can_execute {
        return Err(policy
            .blocked_reason
            .unwrap_or_else(|| "插件运行时被安全策略阻止".into()));
    }

    let integrity =
        PluginService::verify_installation(&state.db, &plugin_id).map_err(|e| e.to_string())?;
    if !integrity.ok {
        return Err(integrity
            .message
            .unwrap_or_else(|| "插件内容已改变，拒绝激活".into()));
    }

    state.plugin_tokens.acquire(&plugin_id)
}

#[tauri::command]
pub fn plugin_revoke_token(state: State<'_, AppState>, plugin_id: String) -> Result<(), String> {
    state.plugin_tokens.revoke(&plugin_id)
}
