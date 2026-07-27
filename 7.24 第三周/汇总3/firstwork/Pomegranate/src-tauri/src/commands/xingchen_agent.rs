use tauri::{AppHandle, State};

use crate::models::{
    AgentMessageInfo, AgentSendMessageInput, AgentSendMessageResult, AgentSessionCreateInput,
    AgentSessionInfo, AgentTestResult, AgentUsageEvent, AgentWorkflowInvokeInput,
    AgentWorkflowInvokeResult, BindableXingchenProduct, ExternalAgentConfig, ExternalAgentInput,
};
use crate::services::xingchen_agent::XingchenAgentService;
use crate::state::AppState;

#[tauri::command]
pub fn external_agent_list(state: State<'_, AppState>) -> Result<Vec<ExternalAgentConfig>, String> {
    XingchenAgentService::list_agents(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn external_agent_list_bindable_products(
    state: State<'_, AppState>,
) -> Result<Vec<BindableXingchenProduct>, String> {
    XingchenAgentService::list_bindable_products(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn external_agent_create(
    state: State<'_, AppState>,
    input: ExternalAgentInput,
) -> Result<ExternalAgentConfig, String> {
    XingchenAgentService::create_agent(&state.db, &state.data_dir, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn external_agent_update(
    state: State<'_, AppState>,
    id: String,
    input: ExternalAgentInput,
) -> Result<ExternalAgentConfig, String> {
    XingchenAgentService::update_agent(&state.db, &state.data_dir, &id, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn external_agent_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    XingchenAgentService::delete_agent(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn external_agent_test_connection(
    state: State<'_, AppState>,
    id: String,
) -> Result<AgentTestResult, String> {
    XingchenAgentService::test_connection(&state.db, &state.data_dir, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn external_agent_health_check(
    state: State<'_, AppState>,
    id: String,
) -> Result<AgentTestResult, String> {
    XingchenAgentService::health_check(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_session_list(
    state: State<'_, AppState>,
    external_agent_id: Option<String>,
) -> Result<Vec<AgentSessionInfo>, String> {
    XingchenAgentService::list_sessions(&state.db, external_agent_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_session_create(
    state: State<'_, AppState>,
    input: AgentSessionCreateInput,
) -> Result<AgentSessionInfo, String> {
    XingchenAgentService::create_session(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_session_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    XingchenAgentService::delete_session(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_message_list(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<AgentMessageInfo>, String> {
    XingchenAgentService::list_messages(&state.db, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_send_message(
    app: AppHandle,
    input: AgentSendMessageInput,
) -> Result<AgentSendMessageResult, String> {
    XingchenAgentService::send_message(app, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_finalize_plugin_output(
    state: State<'_, AppState>,
    session_id: String,
    request_id: String,
    expected_output: String,
    final_output: String,
) -> Result<(), String> {
    XingchenAgentService::finalize_plugin_output(
        &state.db,
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
    input: AgentWorkflowInvokeInput,
) -> Result<AgentWorkflowInvokeResult, String> {
    XingchenAgentService::invoke_workflow(app, input)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_cancel_request(state: State<'_, AppState>, request_id: String) -> Result<(), String> {
    XingchenAgentService::cancel_request(&state.agent_cancel, &request_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_usage_list(
    state: State<'_, AppState>,
    external_agent_id: Option<String>,
) -> Result<Vec<AgentUsageEvent>, String> {
    XingchenAgentService::list_usage(&state.db, external_agent_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_usage_clear(
    state: State<'_, AppState>,
    external_agent_id: Option<String>,
) -> Result<(), String> {
    XingchenAgentService::clear_usage(&state.db, external_agent_id).map_err(|e| e.to_string())
}
