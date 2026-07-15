# AI 泛型提供者注册表 — 开发任务

> 基于 [WorkAny BaseProviderRegistry 分析报告](#) 的建议，在 Rust 侧实现泛型 AI Provider 注册表，支撑多 AI 提供商热切换。

---

## 背景与动机

### 当前状态
- AI 调用链路：`JS 插件 → invoke("send_ai_message") → Rust AiService → HTTP(DeepSeek API)`
- AI 模型配置：通过 `AiModel` 表 CRUD（API Key / Base URL / Model）
- 只有一个硬编码的 AI 调用路径，无法在运行时切换不同的 AI 提供商（OpenAI / Ollama / Anthropic）

### 目标
- 借鉴 WorkAny `BaseProviderRegistry<TProvider, TConfig>` 的泛型注册表设计模式
- 在 Rust 侧实现 `AiProviderRegistry`，支持：
  1. 运行时注册/注销 AI 提供商插件
  2. 单例管理（同类型 Provider 只初始化一次）
  3. 热切换（运行时切换 DeepSeek → OpenAI → Ollama）
  4. 能力查询（supportsStreaming / supportsVision 等）

### 技术约束
- 所有 AI 提供商通过统一的 HTTP API 调用（OpenAI 兼容格式）
- 不依赖外部 Node.js SDK（如 `@codeany/open-agent-sdk`）
- 遵循本项目 Rust 三层架构（models → services → commands）

---

## 架构设计

```
┌─────────────────────────────────────────────────────────┐
│                     前端 (React)                         │
│  invoke("list_ai_providers")                            │
│  invoke("switch_ai_provider", { type: "openai" })       │
│  invoke("get_active_provider")                          │
└────────────────────────┬────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────┐
│               Commands 层 (ai_provider.rs)               │
│  list_ai_providers / switch_ai_provider / ...           │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│            Services 层 (provider_registry.rs)            │
│  AiProviderRegistry {                                    │
│    plugins: HashMap<Type, AiProviderPlugin>              │
│    instances: HashMap<Type, Arc<dyn AiProvider>>         │
│  }                                                       │
│  ┌──────────────────────────────────────────────────┐   │
│  │ trait AiProvider {                                │   │
│  │   fn chat_stream(&self, ...) -> Stream<...>       │   │
│  │   fn get_capabilities() -> AiCapabilities         │   │
│  │ }                                                  │   │
│  │ impl AiProvider for DeepSeekProvider { ... }       │   │
│  │ impl AiProvider for OpenAIProvider { ... }         │   │
│  │ impl AiProvider for OllamaProvider { ... }         │   │
│  └──────────────────────────────────────────────────┘   │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│            Database 层 (复用现有 AiModel 表)              │
│  ai_models 表: id, name, provider_type, api_key,        │
│                base_url, model, is_default, ...         │
└─────────────────────────────────────────────────────────┘
```

### 核心 Trait 设计

```rust
/// AI 提供商能力描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCapabilities {
    pub supports_streaming: bool,
    pub supports_vision: bool,
    pub supports_function_calling: bool,
    pub max_tokens: Option<u32>,
}

/// AI 提供商元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProviderMetadata {
    pub provider_type: String,   // "deepseek" | "openai" | "ollama"
    pub display_name: String,    // "DeepSeek" | "OpenAI" | "Ollama"
    pub description: String,
    pub default_base_url: String,
    pub default_model: String,
    pub capabilities: AiCapabilities,
}

/// AI 提供商插件
pub struct AiProviderPlugin {
    pub metadata: AiProviderMetadata,
    pub factory: Box<dyn Fn(&AiProviderConfig) -> Box<dyn AiProvider> + Send + Sync>,
}

/// AI 提供商统一接口
#[async_trait]
pub trait AiProvider: Send + Sync {
    fn metadata(&self) -> &AiProviderMetadata;
    fn capabilities(&self) -> &AiCapabilities;
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        options: ChatOptions,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send>>, AppError>;
}

/// 注册表
pub struct AiProviderRegistry {
    plugins: HashMap<String, AiProviderPlugin>,
    active_type: RwLock<String>,
    active_instance: RwLock<Option<Arc<dyn AiProvider>>>,
    config: RwLock<AiProviderConfig>,
}
```

### 注册表 API（借鉴 BaseProviderRegistry）

| 方法 | 签名 | 对应 WorkAny |
|------|------|-------------|
| `register` | `fn register(&mut self, plugin: AiProviderPlugin)` | `BaseProviderRegistry.register()` |
| `unregister` | `fn unregister(&mut self, type: &str)` | `BaseProviderRegistry.unregister()` |
| `has` | `fn has(&self, type: &str) -> bool` | `BaseProviderRegistry.has()` |
| `get_metadata` | `fn get_metadata(&self, type: &str) -> Option<&AiProviderMetadata>` | `BaseProviderRegistry.getMetadata()` |
| `get_all_metadata` | `fn get_all_metadata(&self) -> Vec<&AiProviderMetadata>` | `BaseProviderRegistry.getAllMetadata()` |
| `list_registered` | `fn list_registered(&self) -> Vec<String>` | `BaseProviderRegistry.getRegistered()` |
| `switch` | `async fn switch(&self, type: &str) -> Result<(), AppError>` | `ProviderManagerImpl.switchAgentProvider()` |
| `get_active` | `fn get_active(&self) -> Option<Arc<dyn AiProvider>>` | `ProviderManagerImpl.getAgentProvider()` |
| `get_active_type` | `fn get_active_type(&self) -> String` | 新增 |

---

## 开发任务分解

### Phase 1: 数据模型与 Trait 定义 ✅

- [x] **1.1 定义核心 Trait** (`models/ai_provider.rs`)
  - [ ] `AiCapabilities` struct（describe + Serialize/Deserialize）
  - [ ] `AiProviderMetadata` struct
  - [ ] `AiProviderConfig` struct（api_key, base_url, model）
  - [ ] `ChatMessage` struct（role + content）
  - [ ] `ChatOptions` struct（temperature, max_tokens, stream）
  - [ ] `AiProvider` trait（async_trait）
    - [ ] `fn metadata()` → 元数据
    - [ ] `fn capabilities()` → 能力
    - [ ] `async fn chat_stream()` → 流式聊天
    - [ ] `async fn chat()` → 非流式聊天（默认实现调 chat_stream）
  - [ ] `AiProviderPlugin` struct（metadata + factory 闭包）

- [ ] **1.2 在 models/mod.rs 注册模块**
  - [ ] `pub mod ai_provider;`
  - [ ] 确认与现有 `AiModel` / `AiModelInput` 的关系（Provider 是运行时抽象，AiModel 是持久化配置）

### Phase 2: 注册表实现 ✅

- [x] **2.1 实现 AiProviderRegistry** (`services/provider_registry.rs`)
  - [ ] `struct AiProviderRegistry { plugins, active_type, active_instance, config }`
  - [ ] `fn new(default_type: &str) -> Self`
  - [ ] `fn register(&mut self, plugin: AiProviderPlugin)` — 注册 Provider
  - [ ] `fn unregister(&mut self, provider_type: &str)` — 注销
  - [ ] `fn has(&self, provider_type: &str) -> bool` — 检查是否存在
  - [ ] `fn get_metadata(&self, provider_type: &str) -> Option<&AiProviderMetadata>`
  - [ ] `fn get_all_metadata(&self) -> Vec<&AiProviderMetadata>`
  - [ ] `fn list_registered(&self) -> Vec<String>`
  - [ ] `async fn switch(&self, provider_type: &str, config: AiProviderConfig) -> Result<(), AppError>` — 热切换
  - [ ] `fn get_active(&self) -> Option<Arc<dyn AiProvider>>`
  - [ ] `fn get_active_type(&self) -> String`

- [ ] **2.2 注册内置 Provider** (`services/provider_registry.rs` 或 `services/builtin_providers.rs`)
  - [ ] `DeepSeekProvider` — 实现 `AiProvider` trait（复用现有 `AiService::stream_chat` 逻辑）
  - [ ] OpenAI 兼容 Provider（OpenRouter / 自定义端点均走此路径）

- [ ] **2.3 在 services/mod.rs 注册模块**
  - [ ] `pub mod provider_registry;`

### Phase 3: Command 层 ✅

- [x] **3.1 创建 AI Provider Command** (`commands/ai_provider.rs`)
  - [ ] `list_ai_providers` — 返回所有已注册 Provider 的元数据列表
  - [ ] `get_active_provider` — 返回当前活跃 Provider 的类型和配置（隐藏 api_key）
  - [ ] `switch_ai_provider` — 切换 Provider（参数：provider_type + 可选 config）
  - [ ] `get_provider_capabilities` — 返回当前活跃 Provider 的能力

- [ ] **3.2 在 commands/mod.rs 注册模块**
  - [ ] `pub mod ai_provider;`

- [ ] **3.3 在 lib.rs 注册 Command**
  - [ ] 添加 `commands::ai_provider::list_ai_providers` 等

- [ ] **3.4 在 AppState 中注册 ProviderRegistry**
  - [ ] `AppState` 添加 `provider_registry: Arc<AiProviderRegistry>`
  - [ ] `AppState::new()` 中初始化 + 注册内置 Provider

### Phase 4: 前端集成 🚧

- [x] **4.1 TypeScript 类型定义** (`src/types/index.ts`)
  - [ ] `AiCapabilities` interface
  - [ ] `AiProviderMetadata` interface
  - [ ] `AiProviderConfig` interface

- [ ] **4.2 API 封装** (`src/lib/api/index.ts` 或 `src/lib/api/aiProvider.ts`)
  - [ ] `aiProviderApi.listProviders()`
  - [ ] `aiProviderApi.getActiveProvider()`
  - [ ] `aiProviderApi.switchProvider()`
  - [ ] `aiProviderApi.getCapabilities()`

- [ ] **4.3 设置页 UI** — AI 模型设置页增加 Provider 选择
  - [ ] 下拉选择框（Ant Design Select）列出所有已注册 Provider
  - [ ] 切换后显示对应 Provider 的配置项（API Key / Base URL / Model）
  - [ ] 保存后调用 `switch_ai_provider`

- [ ] **4.4 DeepSeek 插件适配**
  - [ ] 不再硬编码 `send_ai_message`，改为使用活跃 Provider
  - [ ] （可选）插件 UI 显示当前使用的 Provider 名称

### Phase 5: 验证与测试

- [ ] **5.1 Rust 构建验证**
  - [ ] `cargo check` 无错误
  - [ ] `cargo clippy` 无警告

- [ ] **5.2 基本功能验证**
  - [ ] 启动后默认 Provider 为 DeepSeek
  - [ ] 通过 Command 切换到 OpenAI 兼容端点
  - [ ] 确认 `chat_stream` 在两个 Provider 间均正常工作
  - [ ] 确认注册/注销不影响活跃 Provider

---

## 预估工作量

| Phase | 内容 | 预估时间 |
|-------|------|---------|
| Phase 1 | 数据模型与 Trait | 1-2h |
| Phase 2 | 注册表 + 内置 Provider | 3-4h |
| Phase 3 | Command 层 | 1h |
| Phase 4 | 前端集成 | 2-3h |
| Phase 5 | 验证测试 | 1h |
| **合计** | | **8-11h** |

---

## 不做的内容（明确排除）

以下 WorkAny 模块**不**集成，原因已在上期分析报告中详述：

| 模块 | 排除原因 |
|------|---------|
| BaseAgent (TypeScript) | 核心能力（流式/会话/计划）已在本项目 Rust 侧实现，移植无增量价值 |
| AgentRegistry (TypeScript) | 深层依赖 `@codeany/open-agent-sdk`，WebView IIFE 无法运行 |
| ProviderManagerImpl | 依赖 Node.js 动态 import()、Sandbox 体系，架构范式不兼容 |

---

## 风险与注意事项

1. **async_trait 依赖**：需在 `Cargo.toml` 添加 `async-trait` crate（如尚未添加）
2. **Provider 工厂模式**：使用 `Box<dyn Fn()>` 闭包，注意 `Send + Sync` 约束
3. **AiService 重构**：DeepSeek 的流式调用逻辑需要从 140KB 的 `ai.rs` 中提取到 `DeepSeekProvider`
4. **向后兼容**：现有 `send_ai_message` Command 需保持可用，内部委托给活跃 Provider
5. **API Key 安全**：API Key 不通过 Command 返回值泄露，仅允许写入不允许读取
