//! 插件系统 Commands
//!
//! 薄 IPC 包装：接收参数 → 调用 PluginService → 转换错误。

use std::collections::HashMap;

use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::models::{
    AgentSendMessageInput, AgentSessionCreateInput, NormalizedPluginManifest, PermissionDiff,
    PluginCompatibility, PluginDocumentSummaryAgentFinalizeInput,
    PluginDocumentSummaryAgentStartInput, PluginDocumentSummaryAgentStartResult,
    PluginDocumentSummaryCancelInput, PluginDocumentSummaryConfig,
    PluginDocumentSummaryConfigInput, PluginDocumentSummaryInput, PluginDocumentSummaryInsertInput,
    PluginDocumentSummaryResult, PluginDocumentToolbarButton, PluginInfo, PluginInstallationInfo,
    PluginIntegrityCheck, PluginManifest, PluginPackageInspection, PluginRuntimePolicy,
    PluginSummaryAgentOption,
};
use crate::services::plugins::PluginService;
use crate::services::xingchen_agent::XingchenAgentService;
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

#[tauri::command]
pub fn parse_plugin_manifest(path: String) -> Result<NormalizedPluginManifest, String> {
    PluginService::parse_manifest(&PathBuf::from(path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn validate_plugin_manifest(path: String) -> Result<NormalizedPluginManifest, String> {
    PluginService::validate_manifest_path(&PathBuf::from(path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn inspect_plugin_package(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<PluginPackageInspection, String> {
    let app_version = app.package_info().version.to_string();
    PluginService::inspect_package(&state.db, &PathBuf::from(path), &app_version)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn calculate_plugin_integrity(path: String) -> Result<String, String> {
    PluginService::calculate_integrity_for_path(&PathBuf::from(path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn compare_plugin_permissions(
    current: Vec<String>,
    next: Vec<String>,
) -> Result<PermissionDiff, String> {
    Ok(PluginService::compare_permissions(current, next))
}

#[tauri::command]
pub fn check_plugin_compatibility(
    app: AppHandle,
    min_app_version: Option<String>,
) -> Result<PluginCompatibility, String> {
    let app_version = app.package_info().version.to_string();
    Ok(PluginService::check_compatibility(
        min_app_version,
        &app_version,
    ))
}

#[tauri::command]
pub fn get_plugin_installation(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Option<PluginInstallationInfo>, String> {
    PluginService::get_installation(&state.db, &plugin_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_plugin_installations(
    state: State<'_, AppState>,
) -> Result<Vec<PluginInstallationInfo>, String> {
    PluginService::list_installations(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn verify_plugin_installation(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<PluginIntegrityCheck, String> {
    PluginService::verify_installation(&state.db, &plugin_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn can_execute_plugin_runtime(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<PluginRuntimePolicy, String> {
    PluginService::can_execute_runtime(&state.db, &plugin_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_summary_toolbar_buttons(
    state: State<'_, AppState>,
) -> Result<Vec<PluginDocumentToolbarButton>, String> {
    PluginService::document_summary_toolbar_buttons(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_mock_summary(
    state: State<'_, AppState>,
    input: PluginDocumentSummaryInput,
) -> Result<PluginDocumentSummaryResult, String> {
    PluginService::mock_document_summary(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_summary_insert(
    state: State<'_, AppState>,
    input: PluginDocumentSummaryInsertInput,
) -> Result<(), String> {
    PluginService::record_document_summary_insert(&state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_summary_agents(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<PluginSummaryAgentOption>, String> {
    PluginService::document_summary_agents(&state.db, &state.data_dir, &plugin_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_summary_config_get(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<PluginDocumentSummaryConfig, String> {
    PluginService::get_document_summary_config(&state.db, &state.data_dir, &plugin_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_summary_config_set(
    state: State<'_, AppState>,
    input: PluginDocumentSummaryConfigInput,
) -> Result<PluginDocumentSummaryConfig, String> {
    PluginService::set_document_summary_config(&state.db, &state.data_dir, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_document_summary_agent_start(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PluginDocumentSummaryAgentStartInput,
) -> Result<PluginDocumentSummaryAgentStartResult, String> {
    let plugin_id = input.plugin_id.clone();
    let (agent, title, prompt) =
        PluginService::prepare_document_summary_agent_start(&state.db, &state.data_dir, input)
            .map_err(|e| e.to_string())?;
    let session = XingchenAgentService::create_session(
        &state.db,
        AgentSessionCreateInput {
            external_agent_id: agent.id.clone(),
            title: Some(format!("文档摘要 - {}", title)),
        },
    )
    .map_err(|e| {
        PluginService::finalize_document_summary_agent(
            &state.db,
            PluginDocumentSummaryAgentFinalizeInput {
                plugin_id: plugin_id.clone(),
                external_agent_id: agent.id.clone(),
                session_id: String::new(),
                request_id: String::new(),
                status: "failed".into(),
                error_code: Some(e.to_string()),
            },
        )
        .ok();
        e.to_string()
    })?;
    let send = XingchenAgentService::send_message(
        app,
        AgentSendMessageInput {
            session_id: session.id.clone(),
            content: prompt,
            scenario: None,
            source_plugin_id: Some(plugin_id.clone()),
            source_feature: Some("document-summary".into()),
        },
    )
    .await
    .map_err(|e| {
        PluginService::finalize_document_summary_agent(
            &state.db,
            PluginDocumentSummaryAgentFinalizeInput {
                plugin_id: plugin_id.clone(),
                external_agent_id: agent.id.clone(),
                session_id: session.id.clone(),
                request_id: String::new(),
                status: "failed".into(),
                error_code: Some(e.to_string()),
            },
        )
        .ok();
        e.to_string()
    })?;
    PluginService::record_document_summary_agent_started(
        &state.db,
        &plugin_id,
        &agent.id,
        &session.id,
        &send.request_id,
    );
    Ok(PluginDocumentSummaryAgentStartResult {
        plugin_id,
        external_agent_id: agent.id,
        session_id: session.id,
        request_id: send.request_id,
        mock: send.mock,
    })
}

#[tauri::command]
pub fn plugin_document_summary_cancel(
    state: State<'_, AppState>,
    input: PluginDocumentSummaryCancelInput,
) -> Result<(), String> {
    PluginService::finalize_document_summary_agent(
        &state.db,
        PluginDocumentSummaryAgentFinalizeInput {
            plugin_id: input.plugin_id.clone(),
            external_agent_id: String::new(),
            session_id: String::new(),
            request_id: input.request_id.clone(),
            status: "cancelled".into(),
            error_code: None,
        },
    )
    .map_err(|e| e.to_string())?;
    XingchenAgentService::cancel_request(&state.agent_cancel, &input.request_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_summary_agent_finalize(
    state: State<'_, AppState>,
    input: PluginDocumentSummaryAgentFinalizeInput,
) -> Result<(), String> {
    PluginService::finalize_document_summary_agent(&state.db, input).map_err(|e| e.to_string())
}
