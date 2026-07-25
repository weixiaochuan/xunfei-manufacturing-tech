# 编程开发 AI 插件 — 实现方案设计

## 问题分析

| 维度 | 现状 | 目标 |
|------|------|------|
| AI 能力 | Rust 端完整的 AI 问答系统（流式/RAG/Skills） | 插件也能调用 AI 进行编程辅助 |
| 插件 API | notes/tasks/editor/settings 域已暴露 | 缺少 `ai` 域的 API |
| 编程场景 | 编辑器有代码块，但无 AI 编程辅助 | AI 代码补全、解释、审查、重构 |
| 安全边界 | 插件通过 Token + 权限代理访问后端 | AI 调用同样需要权限控制 |

---

## 方案对比

### 方案 A：通用 AI 域暴露

在插件 AppAPI 中新增 `ai` 域，让插件可以直接发起 AI 对话（流式）。

| 评分维度 | 得分 | 说明 |
|----------|------|------|
| 复用度 | 5/5 | 完全复用现有 AI 服务 |
| 可行性 | 5/5 | 只需新增 plugin_proxy Command |
| 开发量 | 4/5 | 约 1-2 天 |
| 安全性 | 4/5 | 令牌验证 + 权限控制 + 速率限制 |

**优点**：通用性强，不限于编程场景；后续其他插件也能用 AI 能力
**缺点**：插件开发者需要自己管理提示词和上下文

### 方案 B：编程专用 AI Command

在 Rust 端预置编程场景的专用 Command（代码解释、代码审查、代码补全等）。

| 评分维度 | 得分 | 说明 |
|----------|------|------|
| 复用度 | 2/5 | 需要新建专用提示词和流程 |
| 可行性 | 4/5 | 可行但工作量大 |
| 开发量 | 2/5 | 约 3-5 天 |

**优点**：开箱即用，提示词质量可控
**缺点**：灵活性差，扩展需改 Rust 代码

### 方案 C：混合方案（推荐最终采用）

方案 A 为基础 + 插件侧封装编程专用提示词模板。Rust 端暴露通用 AI 调用能力，编程逻辑在插件 JS 中实现。

| 评分维度 | 得分 |
|----------|------|
| 复用度 | 5/5 |
| 可行性 | 5/5 |
| 开发量 | 4/5 |
| 安全性 | 4/5 |

---

## 推荐：方案 C（混合方案）

**核心思路**：Rust 端增加 `plugin_proxy_ai_chat`（流式）和 `plugin_proxy_ai_chat_sync`（非流式）两个代理 Command，前端 PluginAppAPI 新增 `ai` 域，插件开发者可以自由组合 AI 能力实现编程辅助功能。

---

## 架构设计

### 整体数据流

```
┌─────────────────────────────────────────────────────────────────┐
│ 编程开发插件 (JS, 在前端沙箱执行)                                   │
│                                                                  │
│  const ctx = { app, logger }                                    │
│  ctx.app.ai.chat({ messages, onToken, onDone, onError })        │
│       │                                                          │
│       │  token 隐含在闭包中，插件 JS 不可见                        │
│       ▼                                                          │
│  PluginAppAPI.ai.chat()                                          │
│       │                                                          │
│       │  listen("plugin:ai-token-{token}")                      │
│       │  invoke("plugin_proxy_ai_chat", { token, messages })    │
│       ▼                                                          │
├─────────────────────────────────────────────────────────────────┤
│ Rust 后端                                                        │
│                                                                  │
│  commands/plugin_proxy.rs :: plugin_proxy_ai_chat()             │
│    ├─ verify(token) → plugin_id                                 │
│    ├─ 检查权限: "ai:chat"                                        │
│    ├─ 速率限制                                                    │
│    ├─ 写审计日志                                                  │
│    ├─ AiService::chat_stream() → 使用插件所属的模型配置           │
│    └─ emit("plugin:ai-token-{token_uuid}", payload)             │
│                                                                  │
│  需要新增:                                                        │
│    - plugin_proxy_ai_chat (流式)                                 │
│    - plugin_proxy_ai_chat_sync (非流式)                          │
│    - plugin_proxy_ai_models (列出可用模型)                       │
│    - plugin_proxy_ai_cancel (取消生成)                           │
└──────────────────────────────────────────────────────────────────┘
```

### 新增/修改文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src-tauri/src/commands/plugin_proxy.rs` | 修改 | 新增 4 个 AI 代理 Command |
| `src-tauri/src/services/plugins.rs` | 修改 | 新增 AI 权限常量 |
| `src-tauri/src/models/mod.rs` | 修改 | 新增 PluginAiChatInput/Output 模型 |
| `src/services/pluginApi.ts` | 修改 | 新增 `ai` 域 API |
| `src/types/index.ts` | 修改 | 新增 AI 域 TypeScript 类型 |
| `dev-plugins/code-assistant/plugin.json` | 新建 | 编程助手插件 manifest |
| `dev-plugins/code-assistant/main.js` | 新建 | 编程助手插件主逻辑 |

---

## 详细设计

### 1. 数据模型（Rust）

```rust
// models/mod.rs 新增

/// 插件 AI 对话输入
#[derive(Debug, Deserialize)]
pub struct PluginAiChatInput {
    pub messages: Vec<PluginAiMessage>,
    pub model_id: Option<i64>,    // 可选指定模型，不指定用默认
}

#[derive(Debug, Deserialize)]
pub struct PluginAiMessage {
    pub role: String,  // "system" | "user" | "assistant"
    pub content: String,
}

/// 插件 AI Token 事件负载
#[derive(Debug, Clone, Serialize)]
pub struct PluginAiTokenPayload {
    pub token: String,
    pub done: bool,
    pub error: Option<String>,
}
```

### 2. Rust Command 层

```rust
// commands/plugin_proxy.rs 新增

/// 插件流式 AI 对话
#[tauri::command]
pub async fn plugin_proxy_ai_chat(
    state: State<'_, AppState>,
    app: AppHandle,
    token: String,
    input: PluginAiChatInput,
) -> Result<(), String> {
    // 1. 令牌验证
    let plugin_id = verify_token(&state, &token)?;
    // 2. 权限检查: "ai:chat"
    check_permission(&state, &plugin_id, "ai:chat")?;
    // 3. 速率限制
    check_rate_limit(&state, &plugin_id)?;
    // 4. 审计日志
    audit_log(&state, &plugin_id, "ai_chat", "");
    // 5. 调用 AI 服务（流式）
    let event_name = format!("plugin:ai-token-{}", token);
    AiService::chat_stream_for_plugin(app, input, event_name).await
}
```

### 3. 前端 PluginAppAPI 新增

```typescript
// src/services/pluginApi.ts 新增 ai 域

ai: {
  /**
   * 流式 AI 对话
   * @param messages - 对话消息数组
   * @param callbacks - 回调 { onToken, onDone, onError }
   * @returns 取消函数
   */
  chat: (messages: AiMessage[], callbacks: {
    onToken: (token: string) => void;
    onDone: (fullText: string) => void;
    onError: (error: string) => void;
  }) => {
    // 1. invoke("plugin_proxy_ai_chat", { token, messages })
    // 2. listen(`plugin:ai-token-${token}`, ...) 接收流
    // 3. 返回取消函数 → invoke("plugin_proxy_ai_cancel", { token })
  },

  /** 非流式 AI 对话 */
  chatSync: (messages: AiMessage[]) => Promise<string>,

  /** 获取可用模型列表 */
  listModels: () => Promise<AiModelInfo[]>,

  /** 取消当前生成 */
  cancel: () => Promise<void>,
}
```

### 4. 编程助手插件设计

插件功能模块：

```
编程开发插件 (code-assistant)
├── 代码解释 (Explain)
│   └── 选中代码 → 右键菜单/命令面板 → AI 解释
├── 代码审查 (Review)
│   └── 选中代码 → AI 审查建议
├── 代码补全 (Complete)
│   └── 编辑器内联补全 / 侧边栏建议
├── 代码重构 (Refactor)
│   └── 选中代码 → 指定重构目标 → AI 生成重构版本
├── 测试生成 (GenTest)
│   └── 选中函数 → AI 生成单元测试
└── 命令面板集成
    └── 统一入口: "AI: 解释代码" / "AI: 审查代码" 等
```

### 5. 权限声明

```json
// plugin.json 新增权限声明
{
  "permissions": [
    "ai:chat",        // 调用 AI 对话
    "editor:read",    // 读取编辑器选区
    "editor:write",   // 写入编辑器内容
    "workspace:read", // 读取当前笔记
    "notes:read"      // 读取笔记（上下文）
  ]
}
```

### 6. 新增有效的 AI 权限

```rust
// services/plugins.rs 的 VALID_PERMISSIONS 新增
pub const VALID_PERMISSIONS: &[&str] = &[
    // ... 现有权限 ...
    "ai:chat",    // 新增: AI 对话
    "ai:models",  // 新增: 查看模型列表
];
```

---

## 安全模型

```
插件调用 ai.chat()
  → PluginAppAPI 闭包内 token（插件 JS 不可见）
  → invoke("plugin_proxy_ai_chat", { token, ... })
  → Rust verify(token):
     ├─ token 有效？→ 反查 plugin_id
     ├─ 插件已启用？→ DB 查询
     ├─ 有 "ai:chat" 权限？→ DB 校验
     ├─ 速率限制？→ 10次/分钟（AI 调用较贵）
     └─ 写审计日志 → 可追溯
  → 事件发射目标: "plugin:ai-token-{token_uuid}"
     (token_uuid 仅 Rust 和 PluginAppAPI 知道，插件 JS 无法伪造监听)
```

---

## 关键设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| AI 域放哪层 | Rust 代理 | 安全（API Key 不泄露前端沙箱） |
| 流式方案 | Tauri Event | 复用现有 `ai:token` 模式 |
| 上下文来源 | 插件自行传入 | 灵活性最大，插件决定传什么 |
| 模型选择 | 插件可选指定 | 使用应用已配置的模型 |
| API Key 安全 | 插件不接触 | 使用应用已保存的 API Key |

---

## 实施步骤

| 步骤 | 任务 | 涉及文件 | 说明 |
|------|------|---------|------|
| **1** | Rust 端新增 AI 代理 Command | `commands/plugin_proxy.rs` | 核心：4 个新 Command |
| **2** | 新增数据模型和权限常量 | `models/mod.rs`, `services/plugins.rs` | 支撑：输入输出模型 + 权限定义 |
| **3** | 前端 PluginAppAPI 扩展 `ai` 域 | `services/pluginApi.ts`, `types/index.ts` | 核心：插件可调用的 AI API |
| **4** | 创建编程助手示例插件 | `dev-plugins/code-assistant/` | 验证：plugin.json + main.js |
| **5** | 事件监听清理机制 | `services/pluginApi.ts` | 完善：取消 + 资源回收 |
