//! AI Provider 管理 Commands
//!
//! 暴露 ProviderRegistry 的查询和切换功能给前端。

use crate::models::{
    ActiveProviderInfo, AiCapabilities, AiProviderConfig, AiProviderMetadata, SwitchProviderInput,
};
use crate::services::provider_registry::{get_provider_registry, register_builtin_providers};
use crate::state::AppState;
use std::sync::OnceLock;
use tauri::State;

/// 内置 Provider 初始化标志（进程生命周期内只注册一次）
static BUILTIN_INIT: OnceLock<()> = OnceLock::new();

/// 确保内置 Provider 已注册
fn ensure_builtin_providers() {
    BUILTIN_INIT.get_or_init(|| {
        let registry = get_provider_registry();
        register_builtin_providers(registry);
    });
}

// ─── Provider 列表 ────────────────────────────

/// 列出所有已注册的 AI Provider
#[tauri::command]
pub fn list_ai_providers() -> Result<Vec<AiProviderMetadata>, String> {
    ensure_builtin_providers();
    Ok(get_provider_registry().get_all_metadata())
}

/// 获取当前活跃的 AI Provider 信息（不含 api_key）
#[tauri::command]
pub fn get_active_provider() -> Result<Option<ActiveProviderInfo>, String> {
    ensure_builtin_providers();
    let registry = get_provider_registry();
    let active = registry.get_active();
    match active {
        Some(provider) => {
            let metadata = provider.metadata();
            Ok(Some(ActiveProviderInfo {
                provider_type: metadata.provider_type.clone(),
                display_name: metadata.display_name.clone(),
                model_id: metadata.default_model.clone(),
                capabilities: metadata.capabilities.clone(),
            }))
        }
        None => Ok(None),
    }
}

/// 获取当前活跃 Provider 的能力
#[tauri::command]
pub fn get_provider_capabilities() -> Result<Option<AiCapabilities>, String> {
    ensure_builtin_providers();
    let registry = get_provider_registry();
    let active = registry.get_active();
    Ok(active.map(|p| p.capabilities().clone()))
}

// ─── Provider 切换 ────────────────────────────

/// 切换到指定 AI Provider
///
/// 根据 AiModel 记录 ID 从数据库加载完整配置，然后触发注册表切换。
#[tauri::command]
pub async fn switch_ai_provider(
    state: State<'_, AppState>,
    input: SwitchProviderInput,
) -> Result<ActiveProviderInfo, String> {
    ensure_builtin_providers();

    // 1. 从数据库加载模型配置（含 api_key）
    let model = state
        .db
        .get_ai_model(input.model_id)
        .map_err(|e| format!("加载 AI 模型失败: {}", e))?;

    // 2. 构造 ProviderConfig
    let config = AiProviderConfig {
        provider_type: input.provider_type,
        api_url: model.api_url,
        api_key: model.api_key,
        model_id: model.model_id,
        max_context: Some(model.max_context),
    };

    // 3. 切换
    let registry = get_provider_registry();
    registry
        .switch(config)
        .await
        .map_err(|e| format!("切换 Provider 失败: {}", e))?;

    // 4. 返回新活跃 Provider 信息
    let active = registry.get_active().ok_or("切换后 Provider 未就绪")?;
    let metadata = active.metadata();
    Ok(ActiveProviderInfo {
        provider_type: metadata.provider_type.clone(),
        display_name: metadata.display_name.clone(),
        model_id: metadata.default_model.clone(),
        capabilities: metadata.capabilities.clone(),
    })
}

// ─── Provider 初始化 ──────────────────────────

/// 使用默认 DeepSeek 模型初始化 Provider
///
/// 在应用启动或首次使用时调用，从数据库查找默认模型并激活对应 Provider。
#[tauri::command]
pub async fn init_default_provider(state: State<'_, AppState>) -> Result<ActiveProviderInfo, String> {
    ensure_builtin_providers();

    // 查找默认模型（数据库层直接返回错误而非 Option，我们用 match 区分）
    let model = match state.db.get_default_ai_model() {
        Ok(m) => m,
        Err(_) => {
            return Err("未找到默认 AI 模型，请先在设置中添加 AI 模型".to_string());
        }
    };

    // 构造配置
    let config = AiProviderConfig {
        provider_type: if model.provider == "deepseek" {
            "deepseek".to_string()
        } else {
            "openai_compatible".to_string()
        },
        api_url: model.api_url,
        api_key: model.api_key,
        model_id: model.model_id,
        max_context: Some(model.max_context),
    };

    let registry = get_provider_registry();
    registry
        .switch(config)
        .await
        .map_err(|e| format!("初始化 Provider 失败: {}", e))?;

    let active = registry.get_active().ok_or("初始化后 Provider 未就绪")?;
    let metadata = active.metadata();
    Ok(ActiveProviderInfo {
        provider_type: metadata.provider_type.clone(),
        display_name: metadata.display_name.clone(),
        model_id: metadata.default_model.clone(),
        capabilities: metadata.capabilities.clone(),
    })
}
