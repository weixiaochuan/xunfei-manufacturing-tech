use tauri::{AppHandle, State};

use crate::account::AccountState;
use crate::models::{
    AgentMessageInfo, AgentSendMessageInput, AgentSendMessageResult, AgentSessionCreateInput,
    AgentSessionInfo, AgentTestResult, AgentUsageEvent, AgentWorkflowInvokeInput,
    AgentWorkflowInvokeResult, BindableXingchenProduct, ExternalAgentConfig, ExternalAgentInput,
};
use crate::services::resource_ownership::resolve_resource_owner;
use crate::services::xingchen_agent::XingchenAgentService;
use crate::state::AppState;

#[tauri::command]
pub async fn external_agent_list(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
) -> Result<Vec<ExternalAgentConfig>, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::list_agents(&state.db, &owner).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn external_agent_list_bindable_products(
    state: State<'_, AppState>,
) -> Result<Vec<BindableXingchenProduct>, String> {
    XingchenAgentService::list_bindable_products(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn external_agent_create(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: ExternalAgentInput,
) -> Result<ExternalAgentConfig, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::create_agent(&state.db, &state.data_dir, &owner, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn external_agent_update(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
    input: ExternalAgentInput,
) -> Result<ExternalAgentConfig, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::update_agent(&state.db, &state.data_dir, &owner, &id, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn external_agent_delete(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
) -> Result<(), String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::delete_agent(&state.db, &owner, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn external_agent_test_connection(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
) -> Result<AgentTestResult, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::test_connection(&state.db, &state.data_dir, &owner, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn external_agent_health_check(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
) -> Result<AgentTestResult, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::health_check(&state.db, &owner, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_session_list(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    external_agent_id: Option<String>,
) -> Result<Vec<AgentSessionInfo>, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::list_sessions(&state.db, &owner, external_agent_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_session_create(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: AgentSessionCreateInput,
) -> Result<AgentSessionInfo, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::create_session(&state.db, &owner, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_session_delete(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    id: String,
) -> Result<(), String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::delete_session(&state.db, &owner, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_message_list(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    session_id: String,
) -> Result<Vec<AgentMessageInfo>, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::list_messages(&state.db, &owner, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: AgentSendMessageInput,
) -> Result<AgentSendMessageResult, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::send_message(app, owner, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_finalize_plugin_output(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    session_id: String,
    request_id: String,
    expected_output: String,
    final_output: String,
) -> Result<(), String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::finalize_plugin_output(
        &state.db,
        &owner,
        &session_id,
        &request_id,
        &expected_output,
        &final_output,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_workflow_invoke(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: AgentWorkflowInvokeInput,
) -> Result<AgentWorkflowInvokeResult, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::invoke_workflow(app, owner, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_cancel_request(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request_id: String,
) -> Result<(), String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::cancel_request(&state.db, &owner, &state.agent_cancel, &request_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_usage_list(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    external_agent_id: Option<String>,
) -> Result<Vec<AgentUsageEvent>, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::list_usage(&state.db, &owner, external_agent_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_usage_clear(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    external_agent_id: Option<String>,
) -> Result<(), String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|e| e.to_string())?;
    XingchenAgentService::clear_usage(&state.db, &owner, external_agent_id)
        .map_err(|e| e.to_string())
}
