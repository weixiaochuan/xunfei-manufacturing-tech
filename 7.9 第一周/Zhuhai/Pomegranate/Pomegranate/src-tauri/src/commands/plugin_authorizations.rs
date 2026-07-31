use tauri::State;

use crate::account::AccountState;
use crate::models::{CurrentPluginCapabilityAuthorization, PluginCapabilityAuthorization};
use crate::services::plugin_authorizations;
use crate::state::AppState;

/// 查询当前可信账号与宿主安装上下文下的正式 capability 授权事实。
#[tauri::command]
pub async fn list_current_formal_plugin_capability_authorizations(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
) -> Result<Vec<CurrentPluginCapabilityAuthorization>, String> {
    plugin_authorizations::list_current_formal_plugin_capability_authorizations(
        &state.db, &account, &plugin_id,
    )
    .await
    .map_err(Into::into)
}

/// 创建 pending 授权请求；主体、context、scope 和 semanticVersion 均由后端确定。
#[tauri::command]
pub async fn request_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::request_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
        expires_at,
    )
    .await
    .map_err(Into::into)
}

/// 明确同意当前 capability。
#[tauri::command]
pub async fn grant_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::grant_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
        expires_at,
    )
    .await
    .map_err(Into::into)
}

/// 明确拒绝当前 capability。
#[tauri::command]
pub async fn deny_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::deny_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
    )
    .await
    .map_err(Into::into)
}

/// 撤销当前 capability 的 granted 记录，不删除历史事实。
#[tauri::command]
pub async fn revoke_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::revoke_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
    )
    .await
    .map_err(Into::into)
}

/// 显式写回已经到期的 pending/granted 记录。
#[tauri::command]
pub async fn expire_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::expire_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
    )
    .await
    .map_err(Into::into)
}
