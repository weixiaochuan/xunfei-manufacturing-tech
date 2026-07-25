//! AI Provider 抽象层 — 数据模型与 Trait 定义
//!
//! 借鉴 WorkAny BaseProviderRegistry 的泛型注册表设计模式，
//! 在 Rust 侧实现多 AI 提供商（DeepSeek / OpenAI / Ollama）的统一抽象。

use async_trait::async_trait;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::error::AppError;

// ─── 能力描述 ───────────────────────────────────

/// AI 提供商能力描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCapabilities {
    /// 是否支持流式输出
    pub supports_streaming: bool,
    /// 是否支持视觉输入（图片理解）
    pub supports_vision: bool,
    /// 是否支持 Function Calling / Tool Use
    pub supports_function_calling: bool,
    /// 最大输出 token 数（None = 未知/模型决定）
    pub max_output_tokens: Option<u32>,
}

// ─── 元数据 ─────────────────────────────────────

/// AI 提供商元数据（暴露给前端的选择列表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProviderMetadata {
    /// 唯一标识: "deepseek" | "openai" | "ollama"
    pub provider_type: String,
    /// 显示名称: "DeepSeek" | "OpenAI" | "Ollama"
    pub display_name: String,
    /// 描述文本
    pub description: String,
    /// 默认 API 地址（用户可覆盖）
    pub default_base_url: String,
    /// 默认模型标识
    pub default_model: String,
    /// 此 Provider 的能力
    pub capabilities: AiCapabilities,
}

// ─── Provider 配置 ──────────────────────────────

/// AI Provider 运行时配置（从 AiModel 持久化记录派生）
#[derive(Debug, Clone, Deserialize)]
pub struct AiProviderConfig {
    /// 提供商类型: "deepseek" | "openai" | "ollama"
    pub provider_type: String,
    /// API 基础 URL
    pub api_url: String,
    /// API Key（可为空，如本地 Ollama）
    pub api_key: Option<String>,
    /// 模型标识
    pub model_id: String,
    /// 模型最大上下文 token 数
    pub max_context: Option<i64>,
}

// ─── 对话消息 ───────────────────────────────────

/// 聊天消息（OpenAI 兼容格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// 角色: "system" | "user" | "assistant"
    pub role: String,
    /// 消息文本
    pub content: String,
}

/// 聊天选项
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatOptions {
    /// 温度 (0.0-2.0)
    pub temperature: Option<f32>,
    /// 最大生成 token 数
    pub max_tokens: Option<u32>,
    /// 是否流式返回
    pub stream: bool,
}

// ─── 流式 Token ────────────────────────────────

/// 流式响应的单次 Token
#[derive(Debug, Clone)]
pub enum StreamToken {
    /// 文本片段
    Content(String),
    /// 流结束
    Done,
    /// 流错误
    Error(String),
}

// ─── AI Provider Trait ──────────────────────────

/// AI 提供商统一接口
///
/// 每个具体的 AI Provider（DeepSeek / OpenAI / Ollama）必须实现此 trait。
/// 使用 `#[async_trait]` 以支持 async fn 在 trait 中。
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// 返回此 Provider 的元数据
    fn metadata(&self) -> &AiProviderMetadata;

    /// 返回此 Provider 的能力
    fn capabilities(&self) -> &AiCapabilities;

    /// 检查此 Provider 在当前环境是否可用
    async fn is_available(&self) -> Result<bool, AppError> {
        // 默认实现：总是可用
        Ok(true)
    }

    /// 初始化 Provider（验证配置、预热连接等）
    async fn init(&self) -> Result<(), AppError> {
        Ok(())
    }

    /// 流式聊天
    ///
    /// 返回一个 Stream，每个元素是一段文本或错误。
    /// 使用 `Pin<Box<dyn Stream>>` 以支持动态分发。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        options: ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send>>, AppError>;

    /// 非流式聊天（默认实现：收集 chat_stream 的全部输出）
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: ChatOptions,
    ) -> Result<String, AppError> {
        use futures::StreamExt;

        let mut stream = self.chat_stream(messages, options).await?;
        let mut result = String::new();
        while let Some(chunk) = stream.next().await {
            result.push_str(&chunk?);
        }
        Ok(result)
    }

    /// 关闭 Provider，释放资源
    async fn shutdown(&self) -> Result<(), AppError> {
        Ok(())
    }
}

// ─── Provider 插件（内部用，不序列化）───────────

/// AI Provider 插件定义
///
/// 由注册表持有，包含元数据和工厂函数。
/// 工厂闭包需要 `Send + Sync` 以支持跨线程注册。
pub struct AiProviderPlugin {
    /// 插件元数据
    pub metadata: AiProviderMetadata,
    /// 工厂函数：根据配置创建 AiProvider 实例
    /// 使用 `Arc<dyn AiProvider>` 方便注册表中共享引用
    pub factory: Box<
        dyn Fn(AiProviderConfig) -> Result<std::sync::Arc<dyn AiProvider>, AppError> + Send + Sync,
    >,
}

// ─── 工具函数 ──────────────────────────────────

/// 切换 Provider 入参（从 AiModel ID 懒加载配置）
#[derive(Debug, Clone, Deserialize)]
pub struct SwitchProviderInput {
    /// 目标 Provider 类型: "deepseek" | "openai_compatible"
    pub provider_type: String,
    /// 使用的 AiModel 记录 ID（从数据库加载完整配置）
    pub model_id: i64,
}

/// 活跃 Provider 信息（返回给前端，不包含 api_key）
#[derive(Debug, Clone, Serialize)]
pub struct ActiveProviderInfo {
    pub provider_type: String,
    pub display_name: String,
    pub model_id: String,
    pub capabilities: AiCapabilities,
}

/// 从现有 AiModel 记录构造 AiProviderConfig
///
/// 当用户选择"使用已保存的模型"时，从 DB 读取并转为注册表可用的配置。
impl AiProviderConfig {
    pub fn from_ai_model(provider: &super::AiModel) -> Self {
        Self {
            provider_type: provider.provider.clone(),
            api_url: provider.api_url.clone(),
            api_key: provider.api_key.clone(),
            model_id: provider.model_id.clone(),
            max_context: Some(provider.max_context),
        }
    }
}
