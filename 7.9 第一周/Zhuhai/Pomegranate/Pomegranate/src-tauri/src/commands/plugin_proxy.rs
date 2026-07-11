//! 插件 proxy Command —— 受控的 IPC 入口
//!
//! 安全模型：
//! 1. 所有调用必须带 token
//! 2. token 反查 plugin_id（无效 → 拒绝）
//! 3. 查询 plugin_permissions 表确认权限（缺失 → 拒绝）
//! 4. 通过 → 委托给对应 Service

use std::collections::HashMap;

use serde_json::Value;
use tauri::State;
use tokio::sync::watch;

use crate::error::AppError;
use crate::models::{
    Note, NoteInput, NoteQuery, PluginAiChatInput, PluginAiModelInfo, PluginAuditLogEntry,
    PluginCreateTaskInput, PluginTaskFilter, PluginTaskView, PluginUpdateTaskInput, SearchResult,
};
use crate::services::ai::AiService;
use crate::services::note::NoteService;
use crate::services::search::SearchService;
use crate::services::tasks::TaskService;
use crate::state::AppState;

/// 统一令牌+权限校验
///
/// 返回值：通过校验的 plugin_id
fn verify(
    state: &AppState,
    token: &str,
    required_permission: Option<&str>,
) -> Result<String, AppError> {
    // 1. 令牌反查
    let plugin_id = state
        .plugin_tokens
        .lookup(token)
        .map_err(AppError::Custom)?
        .ok_or(AppError::PluginPermissionDenied {
            plugin_id: None,
            required_permission: None,
        })?;

    // 2. 权限校验
    if let Some(perm) = required_permission {
        let granted = state
            .db
            .has_plugin_permission(&plugin_id, perm)
            .map_err(|e| AppError::Custom(e.to_string()))?;
        if !granted {
            return Err(AppError::PluginPermissionDenied {
                plugin_id: Some(plugin_id),
                required_permission: Some(perm.to_string()),
            });
        }
    }

    // 3. T25 审计日志（失败不阻业务）
    let _ = state.db.write_audit_log(
        &plugin_id,
        required_permission.unwrap_or("settings"),
        None,
    );

    Ok(plugin_id)
}

// ─── notes 域 6 个 Command ─────────────────────────────────

#[tauri::command]
pub fn plugin_proxy_notes_list(
    state: State<'_, AppState>,
    token: String,
    query: NoteQuery,
) -> Result<Vec<Note>, String> {
    verify(&state, &token, Some("notes:read")).map_err(|e| e.to_string())?;
    NoteService::list(&state.db, &query)
        .map(|page| page.items)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_notes_get(
    state: State<'_, AppState>,
    token: String,
    id: i64,
) -> Result<Note, String> {
    verify(&state, &token, Some("notes:read")).map_err(|e| e.to_string())?;
    NoteService::get(&state.db, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_notes_search(
    state: State<'_, AppState>,
    token: String,
    keyword: String,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    verify(&state, &token, Some("notes:read")).map_err(|e| e.to_string())?;
    SearchService::search(&state.db, &keyword, limit)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_notes_create(
    state: State<'_, AppState>,
    token: String,
    input: NoteInput,
) -> Result<Note, String> {
    verify(&state, &token, Some("notes:write")).map_err(|e| e.to_string())?;
    NoteService::create(&state.db, &input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_notes_update(
    state: State<'_, AppState>,
    token: String,
    id: i64,
    input: NoteInput,
) -> Result<Note, String> {
    verify(&state, &token, Some("notes:write")).map_err(|e| e.to_string())?;
    NoteService::update(&state.db, id, &input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_notes_delete(
    state: State<'_, AppState>,
    token: String,
    id: i64,
) -> Result<(), String> {
    verify(&state, &token, Some("notes:write")).map_err(|e| e.to_string())?;
    NoteService::delete(&state.db, id).map_err(|e| e.to_string())
}

// ─── settings 域 3 个 Command ──────────────────────────────
//
// T23: 键名纵深防御 — 所有 key 自动加 plugin:<id>: 前缀。
// DB 层已有 plugin_id 列做隔离（主防线），前缀是第二道保险：
// 即使 token 校验被绕过，跨插件 key 碰撞也不会泄漏数据。

fn scoped_key(plugin_id: &str, key: &str) -> String {
    format!("plugin:{}:{}", plugin_id, key)
}

#[tauri::command]
pub fn plugin_proxy_settings_get(
    state: State<'_, AppState>,
    token: String,
    key: String,
) -> Result<Option<Value>, String> {
    let plugin_id = verify(&state, &token, None).map_err(|e| e.to_string())?;
    let scoped = scoped_key(&plugin_id, &key);
    state
        .db
        .get_plugin_setting(&plugin_id, &scoped)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_settings_set(
    state: State<'_, AppState>,
    token: String,
    key: String,
    value: Value,
) -> Result<(), String> {
    let plugin_id = verify(&state, &token, None).map_err(|e| e.to_string())?;
    let scoped = scoped_key(&plugin_id, &key);
    state
        .db
        .set_plugin_setting(&plugin_id, &scoped, &value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_settings_get_all(
    state: State<'_, AppState>,
    token: String,
) -> Result<HashMap<String, Value>, String> {
    let plugin_id = verify(&state, &token, None).map_err(|e| e.to_string())?;
    let all = state
        .db
        .get_plugin_settings(&plugin_id)
        .map_err(|e| e.to_string())?;
    // 去掉前缀，插件视角 key 无感知
    let prefix = format!("plugin:{}:", plugin_id);
    Ok(all
        .into_iter()
        .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|s| (s.to_string(), v)))
        .collect())
}

// ─── audit 域（T25：管理 UI 调用，不走 token 校验）────────

#[tauri::command]
pub fn plugin_audit_log_list(
    state: State<'_, AppState>,
    plugin_id: String,
    limit: Option<u32>,
) -> Result<Vec<PluginAuditLogEntry>, String> {
    let limit = limit.unwrap_or(50).min(200);
    state
        .db
        .get_plugin_audit_log(&plugin_id, limit)
        .map(|rows| {
            rows.into_iter()
                .map(|(id, pid, op, target, ts)| PluginAuditLogEntry {
                    id,
                    plugin_id: pid,
                    operation: op,
                    target,
                    timestamp: ts,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

// ─── AI 域 4 个 Command（插件 AI 能力）────────────────────

#[tauri::command]
pub async fn plugin_proxy_ai_chat(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    input: PluginAiChatInput,
) -> Result<(), String> {
    let plugin_id = verify(&state, &token, Some("ai:chat")).map_err(|e| e.to_string())?;
    state
        .plugin_rate_limiter
        .check_ai(&plugin_id)
        .map_err(|e| e.to_string())?;

    let request_key = format!("{}:{}", token, input.request_id);
    let event_name = format!("plugin:ai-token-{}", request_key);
    let (cancel_tx, cancel_rx) = watch::channel(false);
    {
        let mut cancel_map = state.plugin_ai_cancel.lock().map_err(|e| e.to_string())?;
        cancel_map.insert(request_key.clone(), cancel_tx);
    }

    let result = AiService::plugin_chat_stream(app, &state.db, input, event_name, cancel_rx).await;

    {
        let mut cancel_map = state.plugin_ai_cancel.lock().map_err(|e| e.to_string())?;
        cancel_map.remove(&request_key);
    }

    result.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_proxy_ai_chat_sync(
    state: State<'_, AppState>,
    token: String,
    input: PluginAiChatInput,
) -> Result<String, String> {
    let plugin_id = verify(&state, &token, Some("ai:chat")).map_err(|e| e.to_string())?;
    state
        .plugin_rate_limiter
        .check_ai(&plugin_id)
        .map_err(|e| e.to_string())?;

    let (_cancel_tx, cancel_rx) = watch::channel(false);
    AiService::plugin_chat_sync(&state.db, input, cancel_rx)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_ai_models(
    state: State<'_, AppState>,
    token: String,
) -> Result<Vec<PluginAiModelInfo>, String> {
    verify(&state, &token, Some("ai:models")).map_err(|e| e.to_string())?;
    let models = state.db.list_ai_models().map_err(|e| e.to_string())?;
    Ok(models
        .into_iter()
        .map(|m| PluginAiModelInfo {
            id: m.id,
            name: m.name,
            provider: m.provider,
            protocol: m.protocol,
            api_url: m.api_url,
            model_id: m.model_id,
            is_default: m.is_default,
            max_context: m.max_context,
            supports_tools: m.supports_tools,
            supports_vision: m.supports_vision,
            max_output_tokens: m.max_output_tokens,
        })
        .collect())
}

#[tauri::command]
pub fn plugin_proxy_ai_cancel(
    state: State<'_, AppState>,
    token: String,
    request_id: String,
) -> Result<(), String> {
    let _plugin_id = verify(&state, &token, Some("ai:chat")).map_err(|e| e.to_string())?;
    let request_key = format!("{}:{}", token, request_id);
    let cancel_map = state.plugin_ai_cancel.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = cancel_map.get(&request_key) {
        let _ = tx.send(true);
    }
    Ok(())
}

// ─── tasks 域 6 个 Command（阶段 2：待办插件化）────────────

#[tauri::command]
pub fn plugin_proxy_tasks_list(
    state: State<'_, AppState>,
    token: String,
    filter: Option<PluginTaskFilter>,
) -> Result<Vec<PluginTaskView>, String> {
    let _plugin_id = verify(&state, &token, Some("tasks.read")).map_err(|e| e.to_string())?;
    TaskService::list_for_plugin(&state.db, filter.unwrap_or_default())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_tasks_get(
    state: State<'_, AppState>,
    token: String,
    id: i64,
) -> Result<PluginTaskView, String> {
    let _plugin_id = verify(&state, &token, Some("tasks.read")).map_err(|e| e.to_string())?;
    TaskService::get_for_plugin(&state.db, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("任务 {} 不存在", id))
}

#[tauri::command]
pub fn plugin_proxy_tasks_create(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    input: PluginCreateTaskInput,
) -> Result<PluginTaskView, String> {
    let plugin_id = verify(&state, &token, Some("tasks.write")).map_err(|e| e.to_string())?;
    state
        .plugin_rate_limiter
        .check_write(&plugin_id)
        .map_err(|e| e.to_string())?;
    TaskService::create_from_plugin(&app, &state.db, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_tasks_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    id: i64,
    input: PluginUpdateTaskInput,
) -> Result<(), String> {
    let plugin_id = verify(&state, &token, Some("tasks.write")).map_err(|e| e.to_string())?;
    state
        .plugin_rate_limiter
        .check_write(&plugin_id)
        .map_err(|e| e.to_string())?;
    TaskService::update_from_plugin(&app, &state.db, id, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_tasks_complete(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    id: i64,
) -> Result<(), String> {
    let plugin_id = verify(&state, &token, Some("tasks.write")).map_err(|e| e.to_string())?;
    state
        .plugin_rate_limiter
        .check_write(&plugin_id)
        .map_err(|e| e.to_string())?;
    TaskService::complete_from_plugin(&app, &state.db, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn plugin_proxy_tasks_delete(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
    id: i64,
) -> Result<(), String> {
    let plugin_id = verify(&state, &token, Some("tasks.write")).map_err(|e| e.to_string())?;
    state
        .plugin_rate_limiter
        .check_write(&plugin_id)
        .map_err(|e| e.to_string())?;
    TaskService::delete_from_plugin(&app, &state.db, id).map_err(|e| e.to_string())
}
