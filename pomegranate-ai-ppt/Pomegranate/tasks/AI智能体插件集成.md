# AI 智能体插件集成方案

> 整合了「AI编程插件方案」和「Web TUI集成方案」，形成两阶段递进式 AI 插件架构。

---

## 1. 背景

项目已具备完整的 AI 基础设施和插件运行时：

| 能力 | 实现 | 位置 |
|------|------|------|
| AI 流式对话 | `send_ai_message` + SSE + `emit("ai:token")` | `commands/ai.rs` + `services/ai.rs` |
| 对话管理 | `list/create/delete_ai_conversation` | `commands/ai.rs` |
| AI 模型管理 | `list/create/update/delete_ai_model` | `commands/ai.rs` |
| 插件面板 | `app.panelViews.register()` | `services/pluginManager.ts` |
| 插件权限 | Token + 权限校验 + 审计日志 | `commands/plugin_proxy.rs` |
| 编辑器桥接 | `app.editor.addContextMenuItem()` 等 | `services/pluginApi.ts` |

**核心问题**：插件系统缺少 AI 调用能力，无法让插件成为"AI 智能体"。

---

## 2. 两阶段递进架构

```
                         ┌───────────────────────────────────┐
                         │         AI 智能体插件层             │
                         │                                   │
  ┌──────────────────────┼───────────────────────────────────┤
  │  编程助手插件          │   Deepseek-TUI 插件              │
  │  (代码解释/审查/补全)   │   (终端风格 AI 对话面板)          │
  │                       │                                   │
  │  使用: Phase 2 API    │  使用: Phase 1 API               │
  └───────┬───────────────┴─────────────┬─────────────────────┘
          │                             │
          ▼                             ▼
  ┌───────────────────┐    ┌───────────────────────────────┐
  │  Phase 2: 安全代理  │    │  Phase 1: 轻量复用              │
  │  (需 Rust 改动)    │    │  (零 Rust 改动，立即可用)        │
  │                   │    │                               │
  │  AppAPI.ai.chat() │    │  invoke("send_ai_message")    │
  │  → plugin_proxy   │    │  listen("ai:token")           │
  │  → Token验证      │    │  → 直接到 AI Service          │
  │  → 权限检查       │    │                               │
  │  → 速率限制       │    │  优点: 快速、零成本             │
  │  → 审计日志       │    │  缺点: 无插件级安全隔离         │
  └───────────────────┘    └───────────────────────────────┘
          │                             │
          ▼                             ▼
  ┌─────────────────────────────────────────────────────────┐
  │              Rust 后端 (三层架构 - 已有)                   │
  │                                                         │
  │  commands/ai.rs                                         │
  │  ├── send_ai_message          → 流式调用 AI API         │
  │  ├── list_ai_conversations    → 对话列表                │
  │  ├── create_ai_conversation   → 新建对话                │
  │  └── delete_ai_conversation   → 删除对话                │
  │                                                         │
  │  services/ai.rs                                         │
  │  ├── stream_chat              → reqwest SSE 流式处理    │
  │  ├── chat_stream_with_skills  → Skills 工具调用         │
  │  └── emit("ai:token")         → 前端实时渲染           │
  │                                                         │
  │  database/ai.rs                                         │
  │  └── ai_models / ai_conversations / ai_messages 表      │
  └─────────────────────────────────────────────────────────┘
```

### 两阶段定位

| | Phase 1: 轻量复用 | Phase 2: 安全代理 |
|------|------|------|
| **Rust 改动** | 零 | 4 个新 proxy Command |
| **开发成本** | 0.5 天 | 1-2 天 |
| **安全隔离** | 依赖 Command 内置安全 | Token + 权限 + 速率 + 审计 |
| **适用场景** | 快速原型、独立对话面板 | 编辑器集成、多插件并发 |
| **推荐时机** | 现在就开始 | Phase 1 验证后实施 |

---

## 3. Phase 1：轻量复用（零 Rust 改动）

### 3.1 原理

插件的 main.js 在 PluginAppAPI 沙箱中执行，**可以直接调用 `invoke()` 访问已注册的全局 Command**。

### 3.2 调用链路

```
插件 main.js
  │
  ├── invoke("create_ai_conversation", { title: "新对话" })
  │     → commands/ai.rs → DB 插入 → 返回 conversation_id
  │
  ├── invoke("send_ai_message", { conversationId, content })
  │     → commands/ai.rs → services/ai.rs → HTTP Stream → emit("ai:token")
  │
  └── listen("ai:token", callback)
        → 接收流式 token → 渲染到插件 DOM
```

### 3.3 示例：Deepseek-TUI 插件（终端风格 AI 面板）

```
dev-plugins/deepseek-tui/
├── plugin.json          # 插件清单 (permissions: ["settings:read"])
├── main.js              # 插件入口 (onLoad: 注册 PanelView + Ribbon + Command)
├── terminal.js          # 终端 UI 渲染引擎 (纯 DOM/CSS，不依赖 React)
├── api.js               # AI API 封装 (invoke 现有 Command)
└── styles.css           # 终端风格样式 (黑底绿字，等宽字体)
```

**核心代码**：

```js
// api.js — 直接 invoke 现有 AI Command
var DeepseekAPI = {
  send: function(conversationId, message) {
    return invoke("send_ai_message", { conversationId, content: message });
  },
  listConversations: function() {
    return invoke("list_ai_conversations");
  },
  createConversation: function(title) {
    return invoke("create_ai_conversation", { title: title || "AI 对话" });
  },
};
```

```js
// terminal.js — 监听流式 Token
app.events.on("ai:token", function(data) {
  appendToOutput(data); // 逐字追加到终端 DOM
  scrollToBottom();
});
```

### 3.4 UI 设计

```
┌──────────────────────────────────────────┐
│  🤖 Deepseek · 新对话 1           [✕]   │
├──────────────────────────────────────────┤
│                                          │
│  $ 你好，请介绍一下你自己                 │  ← 绿色 Prompt
│                                          │
│  ── Deepseek ────────────────────────── │
│  你好！我是 DeepSeek，由深度求索公司      │  ← 白色 AI 回复
│  创造的 AI 助手...                       │
│                                          │
│  $ 帮我写一个 Rust 快速排序              │
│                                          │
│  ── Deepseek ────────────────────────── │
│  ```rust                                 │  ← 代码块渲染
│  fn quicksort<T: Ord>(arr: &mut [T]) {  │
│      ...                                │
│  }                                      │
│  ```                                     │
│                                          │
│  ▋ $ _                                   │  ← 输入行
├──────────────────────────────────────────┤
│  Ctrl+N 新会话 | Ctrl+L 清屏 | Enter 发送 │
└──────────────────────────────────────────┘
```

### 3.5 Phase 1 插件开发的通用模式

```js
// 任何插件都可以用这个模式接入 AI
function onLoad(ctx) {
  var conversationId = null;

  // 1. 创建或复用对话
  async function ensureConversation() {
    if (!conversationId) {
      conversationId = await invoke("create_ai_conversation", { title: "插件对话" });
    }
    return conversationId;
  }

  // 2. 发送消息
  async function askAI(question) {
    var cid = await ensureConversation();
    // 注意: send_ai_message 本身不返回值，结果通过事件推送
    await invoke("send_ai_message", { conversationId: cid, content: question });
  }

  // 3. 监听回复
  var unlisten = null;
  function onToken(data) {
    // data.token — 单个 token 文本
    // data.done — 是否结束
    appendOutput(data.token);
  }

  // ⚠️ 注意: "ai:token" 是全局事件，多个插件时会互相收到
  // Phase 1 的限制，Phase 2 通过独立事件名解决
  ctx.app.events.on("ai:token", onToken);
}
```

### 3.6 Phase 1 限制

| 限制 | 影响 | Phase 2 解决 |
|------|------|-------------|
| 全局事件碰撞 | 多个插件同时用 AI 时 token 事件混乱 | 独立事件名 |
| 无速率限制 | 插件可无限调用 AI（消耗 API 额度） | Token 级别限速 |
| 无审计日志 | 无法追踪哪个插件做了什么 AI 调用 | 审计日志 |
| 权限粒度过粗 | 插件有 invoke 权限就能调所有 AI 命令 | 声明式权限 |

---

## 4. Phase 2：安全代理（增强安全控制）

> Phase 1 验证可行后，按需实施 Phase 2。

### 4.1 新增文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src-tauri/src/commands/plugin_proxy.rs` | 修改 | 新增 4 个 AI 代理 Command |
| `src-tauri/src/services/plugins.rs` | 修改 | `VALID_PERMISSIONS` 新增 `ai:chat`、`ai:models` |
| `src-tauri/src/models/mod.rs` | 修改 | 新增 `PluginAiChatInput`、`PluginAiMessage` |
| `src/services/pluginApi.ts` | 修改 | `AppAPI` 新增 `ai` 域 |
| `src/types/index.ts` | 修改 | 新增 `PluginAiDomain` 等 TS 类型 |
| `dev-plugins/code-assistant/` | 新建 | 编程助手示例插件 |

### 4.2 数据模型

```rust
// models/mod.rs 新增

/// 插件 AI 对话输入
#[derive(Debug, Deserialize)]
pub struct PluginAiChatInput {
    pub messages: Vec<PluginAiMessage>,
    pub model_id: Option<i64>,
    pub conversation_id: Option<i64>,  // 可选：复用已有对话
}

#[derive(Debug, Deserialize)]
pub struct PluginAiMessage {
    pub role: String,    // "system" | "user" | "assistant"
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginAiTokenPayload {
    pub token: String,
    pub done: bool,
    pub error: Option<String>,
}
```

### 4.3 Rust 代理 Command

```rust
// commands/plugin_proxy.rs 新增 4 个 Command

/// 插件流式 AI 对话
#[tauri::command]
pub async fn plugin_proxy_ai_chat(
    state: State<'_, AppState>,
    app: AppHandle,
    token: String,
    input: PluginAiChatInput,
) -> Result<(), String> {
    let plugin_id = verify_token(&state, &token)?;
    check_permission(&state, &plugin_id, "ai:chat")?;
    check_rate_limit(&state, &plugin_id, "ai", 10, 60)?; // 10次/分钟
    audit_log(&state, &plugin_id, "ai_chat", "");

    // 事件名绑定 token 前缀，隔离不同插件的流
    let event_name = format!("plugin:ai-token-{}", token);
    AiService::chat_stream_for_plugin(&app, input, &event_name).await
}

/// 插件非流式 AI 对话
#[tauri::command]
pub async fn plugin_proxy_ai_chat_sync(
    state: State<'_, AppState>,
    token: String,
    input: PluginAiChatInput,
) -> Result<String, String> {
    let plugin_id = verify_token(&state, &token)?;
    check_permission(&state, &plugin_id, "ai:chat")?;
    check_rate_limit(&state, &plugin_id, "ai", 10, 60)?;
    audit_log(&state, &plugin_id, "ai_chat_sync", "");

    AiService::chat_sync(input).await     // 阻塞等待完整回复
}

/// 获取可用模型列表
#[tauri::command]
pub fn plugin_proxy_ai_models(
    state: State<'_, AppState>,
    token: String,
) -> Result<Vec<AiModel>, String> {
    let plugin_id = verify_token(&state, &token)?;
    check_permission(&state, &plugin_id, "ai:models")?;

    state.db.list_ai_models().map_err(|e| e.to_string())
}

/// 取消插件 AI 生成
#[tauri::command]
pub fn plugin_proxy_ai_cancel(
    state: State<'_, AppState>,
    token: String,
) -> Result<(), String> {
    let plugin_id = verify_token(&state, &token)?;
    // 通过 token 查找取消信号并触发
    AiService::cancel_by_token(&token).map_err(|e| e.to_string())
}
```

### 4.4 前端 AppAPI ai 域

```typescript
// src/services/pluginApi.ts 新增

interface PluginAiAPI {
  /** 流式 AI 对话，返回取消函数 */
  chat(
    messages: { role: string; content: string }[],
    callbacks: {
      onToken: (token: string) => void;
      onDone: (fullText: string) => void;
      onError: (err: string) => void;
    },
    opts?: { modelId?: number; conversationId?: number }
  ): () => void;  // 返回取消函数

  /** 非流式 AI 对话 */
  chatSync(
    messages: { role: string; content: string }[],
    opts?: { modelId?: number }
  ): Promise<string>;

  /** 获取可用模型列表 */
  listModels(): Promise<AiModelInfo[]>;
}

// 在 createAppAPI 工厂中实现：
function createAiDomain(token: string): PluginAiAPI {
  var unlisten = null;

  return {
    chat: function(messages, callbacks, opts) {
      var eventName = `plugin:ai-token-${token}`;

      // 监听专属事件流
      unlisten = listen(eventName, function(event) {
        var payload = event.payload;
        if (payload.error) {
          callbacks.onError(payload.error);
          cleanup();
        } else if (payload.done) {
          callbacks.onDone(payload.fullText || "");
          cleanup();
        } else {
          callbacks.onToken(payload.token);
        }
      });

      // 发起请求
      invoke("plugin_proxy_ai_chat", {
        token: token,
        input: { messages: messages, modelId: opts?.modelId, conversationId: opts?.conversationId }
      }).catch(function(e) { callbacks.onError(String(e)); });

      // 返回取消函数
      return function cancel() {
        invoke("plugin_proxy_ai_cancel", { token: token });
        cleanup();
      };

      function cleanup() {
        if (unlisten) { unlisten(); unlisten = null; }
      }
    },

    chatSync: function(messages, opts) {
      return invoke("plugin_proxy_ai_chat_sync", {
        token: token,
        input: { messages: messages, modelId: opts?.modelId }
      });
    },

    listModels: function() {
      return invoke("plugin_proxy_ai_models", { token: token });
    },
  };
}
```

### 4.5 Phase 2 安全模型

```
插件调用 ctx.app.ai.chat(messages, callbacks)
  │
  │ token 在闭包中，插件 JS 不可见
  ▼
createAiDomain(token).chat()
  │
  ├── invoke("plugin_proxy_ai_chat", { token, input })
  │     └── Rust verify():
  │          ├─ token 有效？       → PluginTokenRegistry.lookup()
  │          ├─ 插件已启用？        → DB plugins 表
  │          ├─ 有 "ai:chat" 权限？ → DB plugin_permissions 表
  │          ├─ 速率限制？         → PluginRateLimiter (ai: 10次/分钟)
  │          └─ 审计日志           → DB plugin_audit_log 表
  │
  └── listen("plugin:ai-token-{token}", callback)
        └── 事件名绑定 token UUID，其他插件无法伪造监听
```

### 4.6 Phase 1 vs Phase 2 调用对比

| | Phase 1 | Phase 2 |
|------|------|------|
| **调用方式** | `invoke("send_ai_message", ...)` | `ctx.app.ai.chat(messages, ...)` |
| **事件隔离** | 全局 `ai:token`，多插件碰撞 | `plugin:ai-token-{uuid}`，独立隔离 |
| **权限控制** | 无（有 invoke 权限即能调） | 声明式 `ai:chat` 权限 |
| **速率限制** | 无 | 10次/分钟/插件 |
| **审计日志** | 无 | 完整记录谁在何时做了什么 |
| **取消机制** | `invoke("cancel_ai_generation")` | `ctx.app.ai.chat()` 返回的 `cancel()` |

---

## 5. AI 智能体插件示例

### 5.1 Deepseek-TUI（Phase 1 实现）

终端风格的 AI 对话面板，纯 DOM/CSS 渲染，快捷键驱动。

| 文件 | 行数 | 说明 |
|------|------|------|
| `plugin.json` | ~15 | manifest |
| `main.js` | ~80 | PanelView + Ribbon + Command 注册 |
| `terminal.js` | ~250 | 终端 UI 渲染（黑底绿字，等宽字体） |
| `api.js` | ~60 | invoke 现有 AI Command |
| `styles.css` | ~100 | 终端风格 CSS 变量 |

### 5.2 编程助手（Phase 2 实现）

基于编辑器集成的 AI 编程辅助，右键菜单 + 命令面板入口。

```
dev-plugins/code-assistant/
├── plugin.json          # permissions: [ai:chat, editor:read, editor:write]
├── main.js              # onLoad: 注册命令 + 编辑器菜单
├── prompts.js           # 编程专用提示词模板
└── styles.css           # 结果面板样式
```

**功能模块**：

| 功能 | 触发方式 | 提示词策略 |
|------|---------|-----------|
| 代码解释 | 右键菜单 / 命令面板 | "请详细解释以下代码..." + 选区 |
| 代码审查 | 右键菜单 / 命令面板 | "请审查以下代码，指出潜在问题..." + 选区 |
| 代码重构 | 右键菜单 / 命令面板 | "请重构以下代码，提高可读性和性能..." + 选区 |
| 测试生成 | 命令面板 | "请为以下函数生成单元测试..." + 选区 |
| Bug 分析 | 命令面板 | "以下代码报错，请分析原因..." + 选区 + 错误信息 |

```js
// prompts.js — 编程提示词模板
var Prompts = {
  explain: function(code) {
    return [
      { role: "system", content: "你是一个资深编程导师，请用中文详细解释代码。包括：1) 整体功能 2) 关键逻辑 3) 使用的技术/模式。" },
      { role: "user", content: "请解释以下代码：\n```\n" + code + "\n```" }
    ];
  },
  review: function(code) {
    return [
      { role: "system", content: "你是一个代码审查专家。请指出代码中的：1) 潜在 Bug 2) 安全问题 3) 性能问题 4) 可读性改进。" },
      { role: "user", content: "请审查以下代码：\n```\n" + code + "\n```" }
    ];
  },
  refactor: function(code, goal) {
    return [
      { role: "system", content: "你是一个代码重构专家。请根据目标重构代码，保持功能不变。" },
      { role: "user", content: "重构目标：" + (goal || "提高可读性和性能") + "\n```\n" + code + "\n```" }
    ];
  },
  genTest: function(code, lang) {
    return [
      { role: "system", content: "你是一个测试工程师。请为给定函数生成全面的单元测试。" },
      { role: "user", content: "请为以下" + (lang || "") + "函数生成单元测试：\n```\n" + code + "\n```" }
    ];
  },
};
```

---

## 6. 整合后的实施路线

```
Phase 1 (第 1-2 天)              Phase 2 (第 3-4 天)
┌─────────────────────┐         ┌─────────────────────────┐
│ Deepseek-TUI 插件    │         │ Rust AI 代理 Command     │
│ • 零 Rust 改动        │         │ • plugin_proxy_ai_chat  │
│ • 终端风格 AI 面板    │  ────►  │ • plugin_proxy_ai_chat  │
│ • 快捷键驱动          │         │   _sync                  │
│ • 验证 AI 接入可行性   │         │ • plugin_proxy_ai_models│
│ • 约 535 行代码       │         │ • plugin_proxy_ai_cancel│
└─────────────────────┘         └───────────┬─────────────┘
                                            │
                                            ▼
                               ┌─────────────────────────┐
                               │ AppAPI ai 域 + 编程助手   │
                               │ • createAiDomain()       │
                               │ • 编程助手插件            │
                               │ • 安全隔离 + 审计         │
                               └─────────────────────────┘
```

| 阶段 | 任务 | 依赖 | 产出 |
|------|------|------|------|
| **P1-1** | 创建 Deepseek-TUI 插件 | 无（纯前端） | 终端风格 AI 面板 |
| **P1-2** | 验证 PluginAppAPI 中 invoke/listen 可行性 | P1-1 | 确认方案可行 |
| **P2-1** | Rust 端新增 4 个 AI 代理 Command | P1-2 验证通过 | plugin_proxy_ai_* |
| **P2-2** | AppAPI 新增 ai 域 | P2-1 | ctx.app.ai.* |
| **P2-3** | 编程助手插件 + 提示词模板 | P2-2 | 代码解释/审查/重构 |
| **P2-4** | Deepseek-TUI 迁移到 Phase 2 API（可选） | P2-2 | 安全隔离增强 |

---

## 7. 关键设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 分阶段还是一步到位 | 两阶段递进 | Phase 1 零成本验证，Phase 2 按需增强 |
| Phase 2 是否替换 Phase 1 | 共存 | Phase 1 适合独立面板，Phase 2 适合编辑器集成 |
| API Key 安全 | 插件不可见 | 使用应用已保存的模型配置（Rust 侧持有） |
| 流式方案 | Tauri Event | 复用现有 `ai:token` 模式，Phase 2 加 token 前缀隔离 |
| 提示词位置 | 插件 JS 侧 | 灵活可变，不需要改 Rust 代码 |
| 数据库 | 零改动 | 100% 复用 `ai_conversations` / `ai_messages` 表 |

---

## 8. 不得改动的部分

- ✅ `services/ai.rs` — AI 核心逻辑零改动
- ✅ `database/schema.rs` — 表结构零改动
- ✅ `commands/ai.rs` — 现有 AI Command 零改动
- ✅ `tauri.conf.json` — 配置零改动
- ✅ `capabilities/` — 权限声明零改动

---

## 9. 开发量估算

| 阶段 | 内容 | 文件 | 代码行数 | 时间 |
|------|------|------|---------|------|
| P1 | Deepseek-TUI 插件 | 5 个文件 | ~535 行 | 0.5-1 天 |
| P2-1 | Rust AI 代理 Command | `plugin_proxy.rs` + 模型 | ~150 行 | 1 天 |
| P2-2 | AppAPI ai 域 | `pluginApi.ts` + 类型 | ~100 行 | 0.5 天 |
| P2-3 | 编程助手插件 | `code-assistant/` 4 文件 | ~300 行 | 0.5-1 天 |
| **总计** | | **~12 文件** | **~1085 行** | **2.5-3.5 天** |
