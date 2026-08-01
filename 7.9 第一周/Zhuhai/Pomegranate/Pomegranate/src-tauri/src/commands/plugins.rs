//! 插件系统 Commands
//!
//! 薄 IPC 包装：接收参数 → 调用 PluginService → 转换错误。

use std::collections::HashMap;

use std::path::PathBuf;
use std::time::Instant;

use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::account::AccountState;
use crate::models::{
    AgentSendMessageInput, AgentSessionCreateInput, AgentWorkflowInvokeInput,
    NormalizedPluginManifest, PermissionDiff, PluginActivationRule, PluginArchiveInspection,
    PluginCompatibility, PluginDocumentSummaryAgentFinalizeInput,
    PluginDocumentSummaryAgentStartInput, PluginDocumentSummaryAgentStartResult,
    PluginDocumentSummaryCancelInput, PluginDocumentSummaryConfig,
    PluginDocumentSummaryConfigInput, PluginDocumentSummaryInput, PluginDocumentSummaryInsertInput,
    PluginDocumentSummaryResult, PluginDocumentToolbarButton, PluginExecutionContext,
    PluginExecutionLogInput, PluginFeatureInvokeInput, PluginFeatureInvokeResult, PluginInfo,
    PluginInstallArchiveInput, PluginInstallResult, PluginInstallationInfo, PluginIntegrityCheck,
    PluginManifest, PluginPackageInspection, PluginRuntimePolicy, PluginSummaryAgentOption,
    PluginVersionInfo, ResolvedPluginContributions,
};
use crate::services::plugin_file_exports::{PluginFileExportService, SelectedFileExportView};
use crate::services::plugin_platform::PluginPlatformService;
use crate::services::plugins::PluginService;
use crate::services::resource_ownership::resolve_resource_owner;
use crate::services::xingchen_agent::XingchenAgentService;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginFileExportSelector {
    plugin_id: String,
    feature_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginFileExportAuthorizationInput {
    plugin_id: String,
    feature_id: String,
    selection_handle: String,
}

#[tauri::command]
pub async fn plugin_select_feature_export_target(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: PluginFileExportSelector,
) -> Result<Option<SelectedFileExportView>, String> {
    let suggested = PluginFileExportService::suggested_file_name(
        &state.db,
        &input.plugin_id,
        &input.feature_id,
    )
    .map_err(|error| error.to_string())?;
    let Some(selected) = app
        .dialog()
        .file()
        .set_file_name(&suggested)
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let target = selected
        .into_path()
        .map_err(|_| "无法使用所选保存位置".to_string())?;
    PluginFileExportService::issue_selection(
        &state.selected_file_exports,
        &state.db,
        &account,
        &input.plugin_id,
        &input.feature_id,
        &target,
    )
    .await
    .map(Some)
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn plugin_grant_feature_export(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: PluginFileExportAuthorizationInput,
) -> Result<(), String> {
    PluginFileExportService::grant(
        &state.selected_file_exports,
        &state.db,
        &account,
        &input.plugin_id,
        &input.feature_id,
        &input.selection_handle,
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn plugin_revoke_feature_export(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: PluginFileExportAuthorizationInput,
) -> Result<(), String> {
    PluginFileExportService::revoke(
        &state.selected_file_exports,
        &state.db,
        &account,
        &input.plugin_id,
        &input.feature_id,
        &input.selection_handle,
    )
    .await
    .map_err(|error| error.to_string())
}

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
pub async fn plugin_read_enhancement_resource(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    contribution_id: String,
) -> Result<String, String> {
    PluginPlatformService::read_enhancement_resource(
        &state.db,
        &account,
        &state.plugin_rate_limiter,
        &state.data_dir,
        &plugin_id,
        &contribution_id,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_read_feature_ui_schema(
    state: State<'_, AppState>,
    plugin_id: String,
    feature_id: String,
) -> Result<String, String> {
    PluginPlatformService::read_feature_ui_schema(
        &state.db,
        &state.data_dir,
        &plugin_id,
        &feature_id,
    )
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

/// 预检正式 `.firstwork-plugin` 压缩包；不会修改安装状态。
#[tauri::command]
pub fn plugin_inspect_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<PluginArchiveInspection, String> {
    PluginPlatformService::inspect_archive(
        &state.db,
        &state.data_dir,
        &PathBuf::from(path),
        &app.package_info().version.to_string(),
    )
    .map_err(|error| error.to_string())
}

/// 用户确认预检信息后执行原子安装。
#[tauri::command]
pub fn plugin_install_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PluginInstallArchiveInput,
) -> Result<PluginInstallResult, String> {
    PluginPlatformService::install_archive(
        &state.db,
        &state.data_dir,
        input,
        &app.package_info().version.to_string(),
    )
    .map_err(|error| error.to_string())
}

/// 更新与安装共享同一条安全流水线；版本历史不会被覆盖。
#[tauri::command]
pub fn plugin_update_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    input: PluginInstallArchiveInput,
) -> Result<PluginInstallResult, String> {
    plugin_install_archive(app, state, input)
}

#[tauri::command]
pub fn plugin_rollback(
    state: State<'_, AppState>,
    plugin_id: String,
    version: String,
) -> Result<PluginInstallResult, String> {
    PluginPlatformService::rollback(&state.db, &state.data_dir, &plugin_id, &version)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn plugin_list_versions(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<PluginVersionInfo>, String> {
    PluginPlatformService::list_versions(&state.db, &plugin_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn plugin_get_activation_settings(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<PluginActivationRule>, String> {
    let mut rules = state
        .db
        .get_plugin_activation_settings(&plugin_id)
        .map_err(|error| error.to_string())?;
    let current = state
        .db
        .current_plugin_version(&plugin_id)
        .map_err(|error| error.to_string())?;
    if let Some(version) = current {
        if let Some((manifest, _, _)) = state
            .db
            .plugin_version_manifest(&plugin_id, &version)
            .map_err(|error| error.to_string())?
        {
            if !rules.iter().any(|rule| rule.scope_type == "global") {
                rules.push(PluginActivationRule {
                    plugin_id: plugin_id.clone(),
                    scope_type: "global".into(),
                    scope_key: String::new(),
                    enabled: manifest.default_activation.global,
                    source: "manifest".into(),
                });
            }
            for scene in manifest.supported_scenes {
                let key = scene.as_str().to_string();
                if rules
                    .iter()
                    .any(|rule| rule.scope_type == "scene" && rule.scope_key == key)
                {
                    continue;
                }
                let enabled = manifest
                    .default_activation
                    .scenes
                    .get(&key)
                    .copied()
                    .unwrap_or(manifest.default_activation.global);
                rules.push(PluginActivationRule {
                    plugin_id: plugin_id.clone(),
                    scope_type: "scene".into(),
                    scope_key: key,
                    enabled,
                    source: "manifest".into(),
                });
            }
        }
    }
    Ok(rules)
}

#[tauri::command]
pub fn plugin_set_activation_setting(
    state: State<'_, AppState>,
    plugin_id: String,
    scope_type: String,
    scope_key: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .set_plugin_activation_setting(&plugin_id, &scope_type, &scope_key, enabled)
        .map_err(|error| error.to_string())?;
    if scope_type == "global" {
        state
            .db
            .set_plugin_enabled(&plugin_id, enabled)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn plugin_resolve_enabled_contributions(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    context: PluginExecutionContext,
) -> Result<ResolvedPluginContributions, String> {
    PluginPlatformService::resolve_enabled_contributions(
        &state.db,
        &account,
        &state.plugin_rate_limiter,
        context,
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn plugin_record_execution(
    state: State<'_, AppState>,
    input: PluginExecutionLogInput,
) -> Result<(), String> {
    PluginPlatformService::record_execution(&state.db, input).map_err(|error| error.to_string())
}

/// 正式 feature 插件只能借助当前已配置的 ExternalAgent 调用星辰。
/// 插件不会获得 credential 明文、Endpoint 或通用 Tauri invoke。
#[tauri::command]
pub async fn plugin_feature_invoke_xingchen(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: PluginFeatureInvokeInput,
) -> Result<PluginFeatureInvokeResult, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let audit_request_id = format!("plugin-feature-{}", uuid::Uuid::new_v4());
    let spec = PluginPlatformService::prepare_feature_invocation(
        &state.db,
        &account,
        &state.plugin_rate_limiter,
        &owner,
        &input.plugin_id,
        &input.feature_id,
        &input.external_agent_id,
    )
    .await
    .map_err(|error| error.to_string())?;
    let plugin_id = input.plugin_id.clone();
    let feature_id = input.feature_id.clone();
    let selection_handle = input.selection_handle.clone();
    if matches!(spec.output_kind.as_str(), "docx-base64" | "file-base64") {
        let handle = selection_handle
            .as_deref()
            .ok_or_else(|| "文件输出需要先选择并授权保存目标".to_string())?;
        PluginFileExportService::preflight_authorized(
            &state.selected_file_exports,
            &state.db,
            &account,
            &state.plugin_rate_limiter,
            &plugin_id,
            &feature_id,
            handle,
        )
        .await
        .map_err(|error| error.to_string())?;
    } else if selection_handle.is_some() {
        return Err("当前功能不接受文件选择句柄".into());
    }
    let result = XingchenAgentService::invoke_workflow(
        app,
        owner,
        AgentWorkflowInvokeInput {
            external_agent_id: input.external_agent_id,
            parameters: input.parameters,
            file_paths: input.file_paths,
            source_plugin_id: Some(plugin_id.clone()),
            source_feature: Some(feature_id.clone()),
            plugin_system_context: input.plugin_system_context,
            plugin_contribution_ids: input.plugin_contribution_ids,
        },
    )
    .await;
    match result {
        Ok(mut workflow_result) => {
            if workflow_result.ok
                && matches!(spec.output_kind.as_str(), "docx-base64" | "file-base64")
            {
                if workflow_result.deferred_output_files.len() != 1 {
                    return Err("Workflow 未返回唯一可验证的文件输出".into());
                }
                let deferred = workflow_result.deferred_output_files.remove(0);
                let saved = PluginFileExportService::write_authorized(
                    &state.selected_file_exports,
                    &state.db,
                    &account,
                    &state.plugin_rate_limiter,
                    &plugin_id,
                    &feature_id,
                    selection_handle
                        .as_deref()
                        .ok_or_else(|| "文件输出需要先选择并授权保存目标".to_string())?,
                    &deferred.file_name,
                    &deferred.content_type,
                    &deferred.bytes,
                )
                .await
                .map_err(|error| error.to_string())?;
                workflow_result.content = format!(
                    "{}\n\n已生成文件：{}\n\n保存位置：{}",
                    workflow_result.content, saved.file_name, saved.path
                );
                workflow_result.output_files.push(saved);
            }
            let status = if workflow_result.ok {
                "success"
            } else {
                "failed"
            };
            let error = (!workflow_result.ok).then_some(workflow_result.message.as_str());
            PluginPlatformService::record_feature_invocation(
                &state.db,
                &plugin_id,
                &feature_id,
                &workflow_result.request_id,
                status,
                started.elapsed().as_millis() as i64,
                error,
            );
            PluginPlatformService::finish_feature_invocation(spec, workflow_result)
                .map_err(|error| error.to_string())
        }
        Err(error) => {
            PluginPlatformService::record_feature_invocation(
                &state.db,
                &plugin_id,
                &feature_id,
                &audit_request_id,
                "failed",
                started.elapsed().as_millis() as i64,
                Some(&error.to_string()),
            );
            Err(error.to_string())
        }
    }
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
pub async fn plugin_document_summary_agents(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
) -> Result<Vec<PluginSummaryAgentOption>, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|error| error.to_string())?;
    PluginService::document_summary_agents(&state.db, &state.data_dir, &owner, &plugin_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_document_summary_config_get(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
) -> Result<PluginDocumentSummaryConfig, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|error| error.to_string())?;
    PluginService::get_document_summary_config(&state.db, &state.data_dir, &owner, &plugin_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_document_summary_config_set(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: PluginDocumentSummaryConfigInput,
) -> Result<PluginDocumentSummaryConfig, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|error| error.to_string())?;
    PluginService::set_document_summary_config(&state.db, &state.data_dir, &owner, input)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_document_summary_agent_start(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: PluginDocumentSummaryAgentStartInput,
) -> Result<PluginDocumentSummaryAgentStartResult, String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|error| error.to_string())?;
    let plugin_id = input.plugin_id.clone();
    let plugin_system_context = input.plugin_system_context.clone();
    let plugin_contribution_ids = input.plugin_contribution_ids.clone();
    let (agent, title, prompt) = PluginService::prepare_document_summary_agent_start(
        &state.db,
        &state.data_dir,
        &owner,
        input,
    )
    .map_err(|e| e.to_string())?;
    let session = XingchenAgentService::create_session(
        &state.db,
        &owner,
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
        owner,
        AgentSendMessageInput {
            session_id: session.id.clone(),
            content: prompt,
            effective_content: None,
            plugin_system_context,
            plugin_contribution_ids,
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
pub async fn plugin_document_summary_cancel(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    input: PluginDocumentSummaryCancelInput,
) -> Result<(), String> {
    let owner = resolve_resource_owner(&state.db, &account)
        .await
        .map_err(|error| error.to_string())?;
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
    XingchenAgentService::cancel_request(&state.db, &owner, &state.agent_cancel, &input.request_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_document_summary_agent_finalize(
    state: State<'_, AppState>,
    input: PluginDocumentSummaryAgentFinalizeInput,
) -> Result<(), String> {
    PluginService::finalize_document_summary_agent(&state.db, input).map_err(|e| e.to_string())
}

#[cfg(test)]
mod selected_file_export_dto_tests {
    use super::*;

    #[test]
    fn selected_file_requests_reject_paths_scope_and_actor_fields() {
        assert!(
            serde_json::from_value::<PluginFileExportSelector>(serde_json::json!({
                "pluginId": "plugin",
                "featureId": "feature",
                "path": "C:/forged.txt"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<PluginFileExportAuthorizationInput>(serde_json::json!({
                "pluginId": "plugin",
                "featureId": "feature",
                "selectionHandle": "opaque",
                "scopeKey": "v1:forged",
                "subject": "forged",
                "installation": "forged"
            }))
            .is_err()
        );
    }
}
