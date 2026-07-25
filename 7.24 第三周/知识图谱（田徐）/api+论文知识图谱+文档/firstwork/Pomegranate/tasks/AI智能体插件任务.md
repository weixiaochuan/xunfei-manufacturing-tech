# AI 智能体插件 — 开发任务

> 基于 `AI智能体插件集成.md` 方案，按两阶段递进拆分。

---

## 总览

| 阶段 | 任务数 | 预估时间 | Rust 改动 | 依赖 |
|------|--------|---------|-----------|------|
| Phase 1: 轻量复用 | 5 | 0.5-1 天 | 零 | 无 |
| Phase 2: 安全代理 | 8 | 2-2.5 天 | 4 个 Command | Phase 1 验证通过 |
| **合计** | **13** | **2.5-3.5 天** | | |

---

## 任务依赖图

```
P1-1 创建插件骨架
  └→ P1-2 实现 API 封装层
       └→ P1-3 实现终端 UI 渲染引擎
            └→ P1-4 实现插件入口 + 注册扩展点
                 └→ P1-5 样式 + 快捷键 + 集成测试
                      │
                      ▼ (验证通过：插件可调用 AI)
P2-1 Rust 数据模型 + 权限常量
  └→ P2-2 plugin_proxy_ai_chat (流式)
       ├→ P2-3 plugin_proxy_ai_chat_sync (非流式)
       ├→ P2-4 plugin_proxy_ai_models
       └→ P2-5 plugin_proxy_ai_cancel
            └→ P2-6 注册 Command + cargo check
                 └→ P2-7 AppAPI ai 域实现
                      └→ P2-8 编程助手插件 + 提示词模板
```

---

## Phase 1: Deepseek-TUI 插件（零 Rust 改动）

### P1-1 创建插件骨架

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 15 分钟 |
| **文件** | `dev-plugins/deepseek-tui/plugin.json` |

**产出**：
```json
{
  "id": "deepseek-tui",
  "name": "Deepseek TUI",
  "version": "1.0.0",
  "description": "Deepseek 终端风格 AI 对话面板，复用现有 AI 模型配置",
  "main": "main.js",
  "styles": "styles.css",
  "minAppVersion": "1.0.0",
  "permissions": ["settings:read"],
  "contributes": {
    "commands": [
      { "id": "deepseek.open", "title": "打开 Deepseek 终端" },
      { "id": "deepseek.new-session", "title": "新建 Deepseek 会话" }
    ]
  }
}
```

**验证**：插件能被 `PluginManager.scan()` 发现并加载

---

### P1-2 实现 API 封装层

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 20 分钟 |
| **文件** | `dev-plugins/deepseek-tui/api.js` |
| **依赖** | P1-1 |

**产出**：
- `DeepseekAPI.send(conversationId, message)` — 调用 `invoke("send_ai_message")`
- `DeepseekAPI.listConversations()` — 调用 `invoke("list_ai_conversations")`
- `DeepseekAPI.createConversation(title)` — 调用 `invoke("create_ai_conversation")`
- `DeepseekAPI.deleteConversation(id)` — 调用 `invoke("delete_ai_conversation")`
- `DeepseekAPI.getMessages(conversationId)` — 调用 `invoke("get_ai_messages")`（如有）

**核心代码**：
```js
var DeepseekAPI = (function () {
  function send(conversationId, message) {
    return invoke("send_ai_message", {
      conversationId: conversationId,
      content: message,
    });
  }
  function listConversations() {
    return invoke("list_ai_conversations");
  }
  function createConversation(title) {
    return invoke("create_ai_conversation", { title: title || "Deepseek 对话" });
  }
  return { send, listConversations, createConversation };
})();
```

**验证**：在终端执行 `DeepseekAPI.createConversation("test")` 返回 conversation id

---

### P1-3 实现终端 UI 渲染引擎

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 1.5 小时 |
| **文件** | `dev-plugins/deepseek-tui/terminal.js` |
| **依赖** | P1-2 |

**产出** — `DeepseekTerminal` 模块：

| 功能 | 实现 |
|------|------|
| **DOM 结构** | `el.root` > `el.output` + `el.inputLine` + `el.statusBar` |
| **消息渲染** | 用户消息绿色、AI 消息白色、代码块 `<pre><code>` |
| **输入处理** | `<textarea>` + `keydown` 事件 |
| **流式追加** | `listen("ai:token")` → 最后一个 AI 块逐字追加 + 光标闪烁 |
| **自动滚动** | 每次内容更新后 `scrollTop = scrollHeight` |
| **销毁清理** | 移除 DOM + 取消事件监听 |

**函数签名**：
```js
var DeepseekTerminal = (function () {
  function create(container, app) { /* → { destroy, newSession, focus } */ }
  return { create: create };
})();
```

**验证**：
- [ ] 容器中正确渲染终端 DOM
- [ ] 输入文字后按 Enter 触发发送
- [ ] Shift+Enter 换行
- [ ] AI 流式回复逐字显示

---

### P1-4 实现插件入口 + 注册扩展点

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 30 分钟 |
| **文件** | `dev-plugins/deepseek-tui/main.js` |
| **依赖** | P1-3 |

**产出** — `onLoad / onUnload` 生命周期：

| 扩展点 | API | 说明 |
|------|------|------|
| PanelView | `app.panelViews.register()` | 终端面板容器 |
| Ribbon | `app.ribbon.addItem()` | 右下角常驻按钮 |
| Command | `app.commands.addCommand()` | `deepseek.open` / `deepseek.new-session` |

```js
var terminalInstance = null;
var unregisterItems = [];

function onLoad(ctx) {
  var app = ctx.app;

  // 1. 注册面板视图
  unregisterItems.push(app.panelViews.register({
    id: "deepseek-terminal",
    title: "Deepseek 终端",
    icon: "Terminal",
    render: function (container) {
      terminalInstance = DeepseekTerminal.create(container, app);
    },
  }));

  // 2. Ribbon 按钮
  unregisterItems.push(app.ribbon.addItem({
    id: "deepseek-toggle",
    icon: "BrainCircuit",
    tooltip: "Deepseek 终端",
    onClick: function () { app.panelViews.toggle("deepseek-terminal"); },
  }));

  // 3. 命令面板
  unregisterItems.push(app.commands.addCommand({
    id: "deepseek.open",
    title: "打开 Deepseek 终端",
    callback: function () { app.panelViews.open("deepseek-terminal"); },
  }));
}

function onUnload(ctx) {
  if (terminalInstance) { terminalInstance.destroy(); terminalInstance = null; }
  unregisterItems.forEach(function (fn) { if (typeof fn === "function") fn(); });
  unregisterItems = [];
}

module.exports = { onLoad: onLoad, onUnload: onUnload };
```

**验证**：
- [ ] 插件加载后 Ribbon 出现 Terminal 图标
- [ ] 点击图标打开终端面板
- [ ] Ctrl+Shift+P 搜索 "Deepseek" 能找到命令
- [ ] 插件卸载后 UI 元素全部清理

---

### P1-5 样式 + 快捷键 + 集成测试

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 30 分钟 |
| **文件** | `dev-plugins/deepseek-tui/styles.css` |
| **依赖** | P1-4 |

**产出**：
- [ ] CSS 变量定义（`--dt-bg`, `--dt-fg`, `--dt-prompt`, `--dt-user`, `--dt-ai` 等）
- [ ] GitHub Dark 配色方案（`#0d1117` 背景 / `#3fb950` 提示符）
- [ ] 等宽字体栈（Cascadia Code → Fira Code → JetBrains Mono → Consolas）
- [ ] 代码块语法高亮基础样式
- [ ] 快捷键：
  - `Enter` — 发送消息
  - `Shift+Enter` — 换行
  - `Ctrl+N` — 新建会话
  - `Ctrl+L` — 清屏
  - `Escape` — 取消当前 AI 生成

**集成测试清单**：
- [ ] 新建会话 → 发送消息 → 流式回复 → 正确渲染
- [ ] 代码块 Markdown 渲染（\`\`\`rust ... \`\`\`）
- [ ] 多轮对话上下文保持
- [ ] 清屏后重新发送
- [ ] 插件停用/启用/卸载全生命周期
- [ ] 与现有 AI 抽屉（NoteAiDrawer）共存无冲突

---

## Phase 2: 安全代理（增强安全控制）

### P2-1 Rust 数据模型 + 权限常量

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 20 分钟 |
| **文件** | `src-tauri/src/models/mod.rs`, `src-tauri/src/services/plugins.rs` |
| **依赖** | Phase 1 验证通过 |

**model 新增**：
```rust
#[derive(Debug, Deserialize)]
pub struct PluginAiChatInput {
    pub messages: Vec<PluginAiMessage>,
    pub model_id: Option<i64>,
    pub conversation_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PluginAiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginAiTokenPayload {
    pub token: String,
    pub full_text: Option<String>,
    pub done: bool,
    pub error: Option<String>,
}
```

**权限常量新增**：
```rust
// VALID_PERMISSIONS 数组追加
"ai:chat",
"ai:models",
```

**验证**：`cargo check -p w-notebook` 编译通过

---

### P2-2 plugin_proxy_ai_chat（流式）

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 1 小时 |
| **文件** | `src-tauri/src/commands/plugin_proxy.rs` |
| **依赖** | P2-1 |

**实现要点**：
1. Token 验证 → `PluginTokenRegistry.lookup()`
2. 权限检查 → `db.has_plugin_permission(plugin_id, "ai:chat")`
3. 速率限制 → `PluginRateLimiter.check(plugin_id, "ai", 10, 60)` (10次/分钟)
4. 审计日志 → `db.write_audit_log(plugin_id, "ai_chat", "")`
5. 调用 `AiService::chat_stream_for_plugin(app, input, event_name)`
6. 事件发射 → `app.emit(&event_name, payload)` 其中 `event_name = "plugin:ai-token-{token}"`

**需要新增的 AiService 方法**（最小化改动）：
```rust
// services/ai.rs
impl AiService {
    /// 供插件代理调用的流式对话（使用默认模型）
    pub async fn chat_stream_for_plugin(
        app: &AppHandle,
        input: PluginAiChatInput,
        event_name: &str,
    ) -> Result<(), AppError> {
        // 1. 获取默认 AI 模型
        // 2. 构建请求（复用现有 build_chat_request 逻辑）
        // 3. 流式处理 + emit
        // 4. 自动保存到 ai_conversations / ai_messages
    }
}
```

**验证**：
- [ ] 有效 token → 正常流式返回
- [ ] 无效 token → 返回错误
- [ ] 无 `ai:chat` 权限 → 返回权限错误
- [ ] 超速率限制 → 返回限速错误
- [ ] 审计日志正确写入

---

### P2-3 plugin_proxy_ai_chat_sync（非流式）

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 30 分钟 |
| **文件** | `src-tauri/src/commands/plugin_proxy.rs` |
| **依赖** | P2-2 |

**实现要点**：
- 与 P2-2 相同的验证流程
- 收集完整回复后返回 `Result<String, String>`
- 复用 `AiService::chat_sync()` 或等待流式完成后聚合

```rust
#[tauri::command]
pub async fn plugin_proxy_ai_chat_sync(
    state: State<'_, AppState>,
    token: String,
    input: PluginAiChatInput,
) -> Result<String, String> {
    let plugin_id = verify(&state, &token)?;
    check_perm(&state, &plugin_id, "ai:chat")?;
    check_rate(&state, &plugin_id, "ai", 10, 60)?;
    audit(&state, &plugin_id, "ai_chat_sync", "");
    AiService::chat_sync_for_plugin(&state, input).await.map_err(|e| e.to_string())
}
```

**验证**：`cargo check` + 单元测试

---

### P2-4 plugin_proxy_ai_models

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 15 分钟 |
| **文件** | `src-tauri/src/commands/plugin_proxy.rs` |
| **依赖** | P2-1 |

**实现要点**：
```rust
#[tauri::command]
pub fn plugin_proxy_ai_models(
    state: State<'_, AppState>,
    token: String,
) -> Result<Vec<AiModel>, String> {
    let plugin_id = verify(&state, &token)?;
    check_perm(&state, &plugin_id, "ai:models")?;
    state.db.list_ai_models().map_err(|e| e.to_string())
}
```

**验证**：返回模型列表，不包含 api_key 敏感字段

---

### P2-5 plugin_proxy_ai_cancel

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 20 分钟 |
| **文件** | `src-tauri/src/commands/plugin_proxy.rs` |
| **依赖** | P2-2 |

**实现要点**：
- 通过 token 查找对应的取消信号
- 触发 `cancel_tx.send(true)` 中断流
- 需要维护 `HashMap<String, watch::Sender<bool>>` 映射

```rust
// state.rs 或 services 中新增
pub struct AiCancelRegistry {
    pub senders: Mutex<HashMap<String, watch::Sender<bool>>>,
}
```

**验证**：流式对话中途调用 cancel → 流立即停止

---

### P2-6 注册 Command + cargo check

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 15 分钟 |
| **文件** | `src-tauri/src/lib.rs` |
| **依赖** | P2-2 ~ P2-5 |

**修改 `generate_handler![]`**：
```rust
.invoke_handler(tauri::generate_handler![
    // ... 现有 commands ...
    commands::plugin_proxy::plugin_proxy_ai_chat,
    commands::plugin_proxy::plugin_proxy_ai_chat_sync,
    commands::plugin_proxy::plugin_proxy_ai_models,
    commands::plugin_proxy::plugin_proxy_ai_cancel,
])
```

**验证**：
- [ ] `cargo check` 通过
- [ ] `cargo clippy` 无新增警告
- [ ] `cargo test` 通过

---

### P2-7 AppAPI ai 域实现

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 1 小时 |
| **文件** | `src/services/pluginApi.ts`, `src/types/index.ts` |
| **依赖** | P2-6 |

**TypeScript 类型新增**：
```typescript
// src/types/index.ts
export interface PluginAiMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface PluginAiChatCallbacks {
  onToken: (token: string) => void;
  onDone: (fullText: string) => void;
  onError: (error: string) => void;
}

export interface PluginAiChatOptions {
  modelId?: number;
  conversationId?: number;
}
```

**PluginAppAPI 新增**：
```typescript
// createAiDomain(token) 工厂函数
ai: {
  chat: (messages, callbacks, opts?) => () => void,
  chatSync: (messages, opts?) => Promise<string>,
  listModels: () => Promise<AiModelInfo[]>,
}
```

**关键实现细节**：
- `chat()` 返回取消函数（调用 `plugin_proxy_ai_cancel` + 清理 unlisten）
- 事件名 `plugin:ai-token-{token}` 绑定令牌前缀（隔离）
- done / error 后自动清理 unlisten

**验证**：
- [ ] 在 cc 示例插件中新增 AI 调用按钮，测试完整链路
- [ ] 两个插件同时调用 AI → 事件不碰撞
- [ ] 取消功能正常

---

### P2-8 编程助手插件 + 提示词模板

| 项 | 内容 |
|------|------|
| **状态** | ✅ completed |
| **预估** | 1 小时 |
| **文件** | `dev-plugins/code-assistant/` (4 个文件) |
| **依赖** | P2-7 |

**文件结构**：
```
dev-plugins/code-assistant/
├── plugin.json          # permissions: [ai:chat, editor:read, editor:write]
├── main.js              # 注册 5 个命令 + 编辑器右键菜单
├── prompts.js           # 编程专用提示词模板
└── styles.css           # 输出面板样式
```

**功能清单**：

| 命令 ID | 功能 | 触发方式 | 提示词方法 |
|------|------|---------|-----------|
| `ai.explain-code` | 代码解释 | 右键菜单 + 命令面板 | `Prompts.explain(code)` |
| `ai.review-code` | 代码审查 | 右键菜单 + 命令面板 | `Prompts.review(code)` |
| `ai.refactor-code` | 代码重构 | 右键菜单 + 命令面板 | `Prompts.refactor(code, goal)` |
| `ai.gen-test` | 测试生成 | 命令面板 | `Prompts.genTest(code, lang)` |
| `ai.fix-bug` | Bug 分析 | 命令面板 | `Prompts.fixBug(code, error)` |

**编辑器右键菜单注册**：
```js
// main.js
app.editor.addContextMenuItem({
  id: "ai-explain",
  label: "AI: 解释代码",
  onClick: function () {
    var selection = app.editor.getSelection();
    if (selection) handleAiAction("explain", selection);
  },
});
```

**提示词模板设计**（`prompts.js`）：
```js
var Prompts = {
  explain: function(code) {
    return [
      { role: "system", content: "你是资深编程导师，用中文详细解释代码..." },
      { role: "user", content: "```\n" + code + "\n```" }
    ];
  },
  // ... review, refactor, genTest, fixBug
};
```

**验证**：
- [ ] 编辑器中选中代码 → 右键 → "AI: 解释代码" → 弹窗显示结果
- [ ] 命令面板搜索 "AI:" 显示全部 5 个命令
- [ ] 每种功能流式输出正常
- [ ] 结果面板可以复制/关闭

---

## 进度汇总

| 编号 | 任务 | 状态 | 预估 | 实际 |
|------|------|------|------|------|
| | | **Phase 1: 轻量复用** | | **3h** | |
| P1-0 | invoke + onTauriEvent 桥接（前置） | ✅ | 15m | 插件无法直接 import invoke/listen，需在 PluginAppAPI 中桥接 |
| P1-1 | 创建插件骨架 | ✅ | 15m | dev-plugins/deepseek-tui/plugin.json |
| P1-2 | 实现 API 封装层 | ✅ | 20m | 合并到 main.js — 插件系统只加载单文件 |
| P1-3 | 实现终端 UI 渲染引擎 | ✅ | 1.5h | 合并到 main.js — 纯 DOM 渲染 + 会话列表 |
| P1-4 | 实现插件入口 + 注册扩展点 | ✅ | 30m | PanelView + Sidebar + Ribbon + Command |
| P1-5 | 样式 + 快捷键 + 集成测试 | ✅ | 30m | 初始 DeepSeek 暗色主题，后改为 --kb-* 变量跟随主窗口 |
| | **Phase 2: 安全代理** | | **5.5h** | |
| P2-1 | Rust 数据模型 + 权限常量 | ✅ | 20m | PluginAiChatInput/Message/TokenPayload/ModelInfo + ai:chat/ai:models |
| P2-2 | plugin_proxy_ai_chat（流式） | ✅ | 1h | Token+权限+限速+审计 → PluginAiEmitter → stream_*_generic |
| P2-3 | plugin_proxy_ai_chat_sync（非流式） | ✅ | 30m | NoopAiEmitter 不发事件，直接返回文本 |
| P2-4 | plugin_proxy_ai_models | ✅ | 15m | 返回 PluginAiModelInfo，去除 api_key |
| P2-5 | plugin_proxy_ai_cancel | ✅ | 20m | token:requestId 复合 key，watch channel |
| P2-6 | 注册 Command + cargo check | ✅ | 15m | 4 个 Command 注册到 generate_handler! |
| P2-7 | AppAPI ai 域实现 | ✅ | 1h | ctx.app.ai.chat/chatSync/listModels + PluginAiAPI 类型 |
| P2-8 | 编程助手插件 + 提示词模板 | ✅ | 1h | code-assistant: 代码解释/审查/重构/测试生成 + 右键菜单 |
| | **总计** | | **~8.5h** | |

---

## 验证里程碑

| 里程碑 | 触发条件 | 验证内容 |
|------|------|------|
| **M1: AI 可调用** | ✅ P1-5 + 修复 | 插件 invoke send_ai_message → 流式回复 → 终端渲染 |
| **M2: 安全隔离** | ✅ P2-6 完成 | Token + 权限 + 限速 + 审计全链路 |
| **M3: 编程辅助** | ✅ P2-8 完成 | 代码解释/审查/重构/测试生成 4 个场景 |
| **M4: 多插件共存** | ✅ Phase 2 + 并发修复 | requestId 隔离事件通道，两插件无冲突 |

---

## 不能改动的部分（检查清单）

- [x] `services/ai.rs` — AI 核心逻辑不变（新增 PluginAiEmitter + plugin_chat_stream/sync，不改现有逻辑）
- [x] `database/schema.rs` — 表结构零改动
- [x] `commands/ai.rs` — 现有 AI Command 零改动
- [x] `tauri.conf.json` — 配置零改动
- [x] `capabilities/default.json` — 权限声明零改动
- [x] `src/pages/ai/index.tsx` — 现有 AI 聊天页面零改动

---

## 计划外修复（开发过程中发现并解决）

| 问题 | 根因 | 修复 |
|------|------|------|
| `DeepseekTerminal is not defined` | 插件系统只加载 main.js，api.js/terminal.js 未被加载 | 合并三文件为单文件 main.js |
| app data 与 dev-plugins 不一致 | 插件运行时从 `app data/plugins/` 加载，非 dev-plugins | 每次修改后同步到 app data 目录 |
| `send_ai_message` 参数名错误 | 插件传 `content`，Rust 期望 `message` | 改为 `message: message` |
| AI 回复无内容 | Rust `emit("ai:token", content)` 发的是纯字符串，插件按对象取 `.token` | 兼容字符串和对象两种 payload 格式 |
| `ai:done` 后 state=null 崩溃 | done/error 监听器未保存，清理不完整；state 无空值防护 | `_listeners[]` 数组统一管理；`if(!state)return` 防护 |
| 无会话管理 | 终端只有纯聊天区 | 添加左侧 200px 会话列表 + 历史加载 + 切换 + 新建 |
| 深色主题与主窗口不兼容 | 硬编码 DeepSeek 配色 | 全部改用 `var(--kb-*)` CSS 变量，自动跟随亮/暗主题 |
| 并发 AI 调用串扰 | 同插件多请求共用事件名和取消 key | requestId 隔离事件通道 + 取消 map |
| `chatSync` 事件泄露 | 复用流式方法向固定事件名发 token | NoopAiEmitter 不发事件 |
| invoke resolve 后未兜底 | done 事件丢失时监听器泄漏 | `.then()` 兜底调用 `onDone` |
