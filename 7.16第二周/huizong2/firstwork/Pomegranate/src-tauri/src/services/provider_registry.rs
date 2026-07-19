//! AI Provider 注册表 — 借鉴 WorkAny BaseProviderRegistry 的泛型注册表模式
//!
//! 管理多个 AI 提供商（DeepSeek / OpenAI 兼容 / Ollama）的注册、切换和生命周期。
//!
//! API 对照 WorkAny:
//! - register()              → BaseProviderRegistry.register()
//! - unregister()            → BaseProviderRegistry.unregister()
//! - has()                   → BaseProviderRegistry.has()
//! - get_metadata()          → BaseProviderRegistry.getMetadata()
//! - get_all_metadata()      → BaseProviderRegistry.getAllMetadata()
//! - list_registered()       → BaseProviderRegistry.getRegistered()
//! - switch()                → ProviderManagerImpl.switchAgentProvider()
//! - get_active()            → ProviderManagerImpl.getAgentProvider()
//! - get_active_type()       → 新增

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use futures::stream::Stream;
use reqwest::Client;
use serde_json::{json, Value};
use std::pin::Pin;

use crate::error::AppError;
use crate::models::ai_provider::AiProvider;
use crate::models::ai_provider::{
    AiCapabilities, AiProviderConfig, AiProviderMetadata, AiProviderPlugin, ChatMessage,
    ChatOptions,
};
use crate::services::http_client;

// ─── 全局单例 ──────────────────────────────────

static REGISTRY: std::sync::LazyLock<AiProviderRegistry> =
    std::sync::LazyLock::new(AiProviderRegistry::new);

/// 获取全局 AI Provider 注册表单例
pub fn get_provider_registry() -> &'static AiProviderRegistry {
    &REGISTRY
}

// ─── 注册表实现 ─────────────────────────────────

/// AI Provider 注册表
///
/// 线程安全：plugins 在初始化后只读，active 通过 RwLock 保护。
pub struct AiProviderRegistry {
    /// 已注册的 Provider 插件（按 provider_type 索引）
    plugins: RwLock<HashMap<String, AiProviderPlugin>>,
    /// 当前活跃的 Provider 实例
    active: RwLock<Option<Arc<dyn AiProvider>>>,
    /// 当前活跃 Provider 的类型
    active_type: RwLock<String>,
}

impl AiProviderRegistry {
    /// 创建空的注册表
    fn new() -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            active: RwLock::new(None),
            active_type: RwLock::new(String::new()),
        }
    }

    // ─── 注册 / 注销 ──────────────────────────

    /// 注册一个 Provider 插件
    ///
    /// 如果同类型已存在，会覆盖（并打印警告日志）。
    pub fn register(&self, plugin: AiProviderPlugin) {
        let provider_type = plugin.metadata.provider_type.clone();
        let mut plugins = self.plugins.write().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] plugins lock poisoned: {}", e);
            e.into_inner()
        });

        if plugins.contains_key(&provider_type) {
            log::warn!(
                "[ProviderRegistry] Overwriting existing provider: {}",
                provider_type
            );
        }

        log::info!(
            "[ProviderRegistry] Registered provider: {} ({})",
            provider_type,
            plugin.metadata.display_name
        );

        plugins.insert(provider_type, plugin);
    }

    /// 注销一个 Provider
    pub fn unregister(&self, provider_type: &str) {
        let mut plugins = self.plugins.write().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] plugins lock poisoned: {}", e);
            e.into_inner()
        });

        if plugins.remove(provider_type).is_some() {
            log::info!(
                "[ProviderRegistry] Unregistered provider: {}",
                provider_type
            );
        }
    }

    /// 检查 Provider 类型是否已注册
    pub fn has(&self, provider_type: &str) -> bool {
        let plugins = self.plugins.read().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] plugins lock poisoned: {}", e);
            e.into_inner()
        });
        plugins.contains_key(provider_type)
    }

    // ─── 元数据查询 ────────────────────────────

    /// 获取指定 Provider 的元数据
    pub fn get_metadata(&self, provider_type: &str) -> Option<AiProviderMetadata> {
        let plugins = self.plugins.read().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] plugins lock poisoned: {}", e);
            e.into_inner()
        });
        plugins.get(provider_type).map(|p| p.metadata.clone())
    }

    /// 获取所有已注册 Provider 的元数据
    pub fn get_all_metadata(&self) -> Vec<AiProviderMetadata> {
        let plugins = self.plugins.read().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] plugins lock poisoned: {}", e);
            e.into_inner()
        });
        plugins.values().map(|p| p.metadata.clone()).collect()
    }

    /// 列出所有已注册的 Provider 类型
    pub fn list_registered(&self) -> Vec<String> {
        let plugins = self.plugins.read().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] plugins lock poisoned: {}", e);
            e.into_inner()
        });
        plugins.keys().cloned().collect()
    }

    // ─── 活跃 Provider 管理 ─────────────────────

    /// 切换到指定 Provider
    ///
    /// 根据 provider_type 查找已注册的插件，调用工厂创建实例，
    /// 并设为活跃。旧的活跃 Provider 会被 shutdown。
    pub async fn switch(&self, config: AiProviderConfig) -> Result<(), AppError> {
        let provider_type = config.provider_type.clone();

        // 1. 在锁内创建 Provider 实例（工厂调用不耗时）
        let new_provider: Arc<dyn AiProvider> = {
            let plugins = self.plugins.read().unwrap_or_else(|e| {
                log::error!("[ProviderRegistry] plugins lock poisoned: {}", e);
                e.into_inner()
            });

            let plugin = plugins.get(&provider_type).ok_or_else(|| {
                AppError::NotFound(format!(
                    "Provider 类型未注册: {}。已注册: {:?}",
                    provider_type,
                    plugins.keys().collect::<Vec<_>>()
                ))
            })?;

            (plugin.factory)(config)?
        };

        // 2. 初始化新 Provider
        new_provider.init().await?;

        // 3. 关闭旧 Provider
        let old = {
            let mut active = self.active.write().unwrap_or_else(|e| {
                log::error!("[ProviderRegistry] active lock poisoned: {}", e);
                e.into_inner()
            });
            let old = active.take();
            *active = Some(new_provider);
            old
        };
        {
            let mut at = self.active_type.write().unwrap_or_else(|e| {
                log::error!("[ProviderRegistry] active_type lock poisoned: {}", e);
                e.into_inner()
            });
            *at = provider_type.clone();
        }

        if let Some(old) = old {
            if let Err(e) = old.shutdown().await {
                log::warn!("[ProviderRegistry] 关闭旧 Provider 失败: {}", e);
            }
        }

        log::info!("[ProviderRegistry] 已切换到 Provider: {}", provider_type);
        Ok(())
    }

    /// 获取当前活跃的 Provider 实例
    pub fn get_active(&self) -> Option<Arc<dyn AiProvider>> {
        let active = self.active.read().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] active lock poisoned: {}", e);
            e.into_inner()
        });
        active.clone()
    }

    /// 获取当前活跃 Provider 的类型标识
    pub fn get_active_type(&self) -> String {
        let active_type = self.active_type.read().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] active_type lock poisoned: {}", e);
            e.into_inner()
        });
        active_type.clone()
    }

    /// 关闭所有 Provider
    pub async fn shutdown_all(&self) {
        let mut active = self.active.write().unwrap_or_else(|e| {
            log::error!("[ProviderRegistry] active lock poisoned: {}", e);
            e.into_inner()
        });
        if let Some(provider) = active.take() {
            if let Err(e) = provider.shutdown().await {
                log::warn!("[ProviderRegistry] shutdown 失败: {}", e);
            }
        }
        log::info!("[ProviderRegistry] 所有 Provider 已关闭");
    }
}

// ─── 内置 Provider: OpenAI 兼容（DeepSeek / OpenAI / Ollama 通用）───

/// 默认能力：支持流式，不支持视觉和 function calling
const DEFAULT_CAPABILITIES: AiCapabilities = AiCapabilities {
    supports_streaming: true,
    supports_vision: false,
    supports_function_calling: false,
    max_output_tokens: None,
};

/// DeepSeek 默认元数据
const DEEPSEEK_METADATA: AiProviderMetadata = AiProviderMetadata {
    provider_type: String::new(), // 运行时填充
    display_name: String::new(),
    description: String::new(),
    default_base_url: String::new(),
    default_model: String::new(),
    capabilities: DEFAULT_CAPABILITIES,
};

/// OpenAI 兼容 Provider — 通用 HTTP 流式客户端
///
/// 通过调整 base_url + model 可兼容 DeepSeek / OpenAI / Ollama / OpenRouter 等
/// 所有提供 OpenAI 兼容 API 的服务。
pub struct OpenAiCompatibleProvider {
    metadata: AiProviderMetadata,
    config: RwLock<AiProviderConfig>,
    http_client: &'static Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(metadata: AiProviderMetadata, config: AiProviderConfig) -> Self {
        Self {
            metadata,
            config: RwLock::new(config),
            http_client: http_client::shared(),
        }
    }

    /// 构建 OpenAI 兼容的 chat completions URL
    fn build_chat_url(base_url: &str) -> String {
        let trimmed = base_url.trim_end_matches('/');
        if trimmed.ends_with("/chat/completions") {
            trimmed.to_string()
        } else if trimmed.ends_with("/v1") {
            format!("{}/chat/completions", trimmed)
        } else {
            format!("{}/v1/chat/completions", trimmed)
        }
    }

    /// 将消息构建为 OpenAI API 格式的 Value 数组
    fn build_messages_json(messages: &[ChatMessage]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| {
                json!({
                    "role": m.role,
                    "content": m.content
                })
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    fn metadata(&self) -> &AiProviderMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &AiCapabilities {
        &self.metadata.capabilities
    }

    async fn is_available(&self) -> Result<bool, AppError> {
        // HTTP Provider 总是可用（网络问题在 chat_stream 时报错）
        Ok(true)
    }

    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        options: ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send>>, AppError> {
        // 提取配置值后立即 drop 锁守卫（RwLockReadGuard 不是 Send，不能跨 await）
        let (api_url, model_id, api_key) = {
            let config = self
                .config
                .read()
                .map_err(|e| AppError::Custom(format!("Config lock error: {}", e)))?;
            (
                config.api_url.clone(),
                config.model_id.clone(),
                config.api_key.clone(),
            )
        };

        let url = Self::build_chat_url(&api_url);
        let api_messages = Self::build_messages_json(&messages);

        let mut body = json!({
            "model": model_id,
            "messages": api_messages,
            "stream": options.stream,
        });

        if let Some(t) = options.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(mt) = options.max_tokens {
            body["max_tokens"] = json!(mt);
        }

        let mut request = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body);

        if let Some(ref key) = api_key {
            if !key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", key));
            }
        }

        if !options.stream {
            // 非流式：简单请求-响应
            let response = request
                .send()
                .await
                .map_err(|e| AppError::Custom(format!("API 请求失败: {}", e)))?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(AppError::Custom(format!("API 错误 {}: {}", status, body)));
            }

            let data: Value = response
                .json()
                .await
                .map_err(|e| AppError::Custom(format!("JSON 解析失败: {}", e)))?;

            let content = data["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();

            // 返回一个立即产生单个值的流
            let stream = futures::stream::once(async move { Ok(content) });
            Ok(Box::pin(stream))
        } else {
            // 流式：SSE 解析
            let response = request
                .send()
                .await
                .map_err(|e| AppError::Custom(format!("API 请求失败: {}", e)))?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(AppError::Custom(format!("API 错误 {}: {}", status, body)));
            }

            let byte_stream = response.bytes_stream();
            let stream = sse_parser(byte_stream);
            Ok(Box::pin(stream))
        }
    }
}

/// SSE 流解析器：将 reqwest 字节流转换为文本 token 流
///
/// 解析标准 SSE 格式：
/// - `data: {"choices":[{"delta":{"content":"hello"}}]}\n\n`
/// - `data: [DONE]\n\n`
fn sse_parser<S>(byte_stream: S) -> impl Stream<Item = Result<String, AppError>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    byte_stream
        .map(|chunk| -> Vec<Result<String, AppError>> {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => return vec![Err(AppError::Custom(format!("流读取错误: {}", e)))],
            };

            let text = String::from_utf8_lossy(&bytes);
            let mut results = Vec::new();

            for line in text.lines() {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                if line == "data: [DONE]" {
                    continue;
                }

                if let Some(json_str) = line.strip_prefix("data: ") {
                    match serde_json::from_str::<Value>(json_str) {
                        Ok(data) => {
                            if let Some(content) = data["choices"][0]["delta"]["content"].as_str() {
                                if !content.is_empty() {
                                    results.push(Ok(content.to_string()));
                                }
                            }
                        }
                        Err(_) => {
                            // 忽略无法解析的行
                        }
                    }
                }
            }

            results
        })
        .flat_map(futures::stream::iter)
}

// ─── 内置 Provider 工厂 ─────────────────────────

/// DeepSeek Provider 元数据
fn deepseek_metadata() -> AiProviderMetadata {
    AiProviderMetadata {
        provider_type: "deepseek".to_string(),
        display_name: "DeepSeek".to_string(),
        description: "DeepSeek API（OpenAI 兼容），支持 Chat V3 / Reasoner".to_string(),
        default_base_url: "https://api.deepseek.com".to_string(),
        default_model: "deepseek-chat".to_string(),
        capabilities: AiCapabilities {
            supports_streaming: true,
            supports_vision: false,
            supports_function_calling: false,
            max_output_tokens: Some(8192),
        },
    }
}

/// OpenAI Provider 元数据（可复用为 OpenAI / OpenRouter / 自定义端点）
fn openai_compatible_metadata() -> AiProviderMetadata {
    AiProviderMetadata {
        provider_type: "openai_compatible".to_string(),
        display_name: "OpenAI 兼容".to_string(),
        description: "通用 OpenAI 兼容 API（支持 OpenAI / OpenRouter / 自定义端点）".to_string(),
        default_base_url: "https://api.openai.com".to_string(),
        default_model: "gpt-4o-mini".to_string(),
        capabilities: AiCapabilities {
            supports_streaming: true,
            supports_vision: true,
            supports_function_calling: true,
            max_output_tokens: Some(16384),
        },
    }
}

/// 注册所有内置 Provider
pub fn register_builtin_providers(registry: &AiProviderRegistry) {
    // DeepSeek
    registry.register(AiProviderPlugin {
        metadata: deepseek_metadata(),
        factory: Box::new(|config: AiProviderConfig| {
            let metadata = AiProviderMetadata {
                provider_type: config.provider_type.clone(),
                display_name: "DeepSeek".to_string(),
                description: format!("DeepSeek API — {} @ {}", config.model_id, config.api_url),
                default_base_url: config.api_url.clone(),
                default_model: config.model_id.clone(),
                capabilities: AiCapabilities {
                    supports_streaming: true,
                    supports_vision: false,
                    supports_function_calling: false,
                    max_output_tokens: Some(8192),
                },
            };
            Ok(Arc::new(OpenAiCompatibleProvider::new(metadata, config)) as Arc<dyn AiProvider>)
        }),
    });

    // OpenAI 兼容
    registry.register(AiProviderPlugin {
        metadata: openai_compatible_metadata(),
        factory: Box::new(|config: AiProviderConfig| {
            let metadata = AiProviderMetadata {
                provider_type: config.provider_type.clone(),
                display_name: format!("OpenAI 兼容 — {}", config.model_id),
                description: format!("OpenAI 兼容 API — {} @ {}", config.model_id, config.api_url),
                default_base_url: config.api_url.clone(),
                default_model: config.model_id.clone(),
                capabilities: AiCapabilities {
                    supports_streaming: true,
                    supports_vision: true,
                    supports_function_calling: true,
                    max_output_tokens: Some(16384),
                },
            };
            Ok(Arc::new(OpenAiCompatibleProvider::new(metadata, config)) as Arc<dyn AiProvider>)
        }),
    });

    log::info!(
        "[ProviderRegistry] 内置 Provider 已注册: {}",
        registry.list_registered().join(", ")
    );
}
