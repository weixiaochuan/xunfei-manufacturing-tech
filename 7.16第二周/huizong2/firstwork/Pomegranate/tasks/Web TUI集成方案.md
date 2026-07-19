# Deepseek-TUI 插件集成方案 — Web TUI 面板

> 方案 B：创建 Deepseek 插件，在面板中用纯 DOM/CSS 模拟终端风格界面，复用现有 AI 三层架构。

---

## 1. 背景

项目已具备完整的 Deepseek API 支持基础设施：

| 层级 | 文件 | 能力 |
|------|------|------|
| Command | `src-tauri/src/commands/ai.rs` | 对话管理、流式消息、事件推送 |
| Service | `src-tauri/src/services/ai.rs` | `build_openai_chat_url` 支持 Deepseek OpenAPI 兼容 |
| 前端 UI | `src/components/ai/NoteAiDrawer.tsx` | 现有 AI 聊天抽屉 |
| 插件系统 | `src/services/pluginManager.ts` | registerPanelView / registerRibbonItem / registerCommand |

**无需新增 Rust Command，100% 复用现有 AI 后端。**

---

## 2. 架构设计

```
┌──────────────────────────────────────────────────────────────────┐
│  Deepseek-TUI 插件 (dev-plugins/deepseek-tui/)                    │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  main.js (插件入口)                                           │ │
│  │  ├── onLoad: 注册 PanelView + Ribbon 按钮 + 命令              │ │
│  │  │   ├── panelView "deepseek-terminal"                        │ │
│  │  │   ├── ribbon 按钮 (右下角常驻)                              │ │
│  │  │   └── command "deepseek.open" (命令面板可调)               │ │
│  │  └── onUnload: 清理注册                                       │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  terminal.js (终端 UI 渲染引擎)                                │ │
│  │  ├── 终端风格渲染 (黑色背景、绿色输入、白色输出)               │ │
│  │  ├── 多行输入框 (Shift+Enter 换行, Enter 发送)                │ │
│  │  ├── 流式输出 (Tauri Event listen → 逐字追加)                  │ │
│  │  ├── Markdown 渲染 (代码块高亮、表格、列表)                    │ │
│  │  └── 会话切换 (Ctrl+N 新建, Ctrl+L 清屏)                      │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                    │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  api.js (API 封装 — 复用现有 Rust Command)                     │ │
│  │  ├── chat.send(message) → invoke("send_ai_message", ...)      │ │
│  │  ├── chat.listen()       → listen("ai:token", ...)            │ │
│  │  ├── chat.history()      → invoke("list_ai_conversations")    │ │
│  │  └── chat.newSession()   → invoke("create_ai_conversation")   │ │
│  └──────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
         │                                    │
         │ AppAPI (受限 token)                │ plugin.json
         │ invoke() / listen()                │ permissions
         ▼                                    ▼
┌──────────────────────────────────────────────────────────────────┐
│  Rust 后端 (三层架构 — 全复用，零改动)                            │
│                                                                    │
│  commands/ai.rs (已有)                                            │
│  ├── send_ai_message          → 流式调用 Deepseek API             │
│  ├── list_ai_conversations    → 获取对话列表                      │
│  ├── create_ai_conversation   → 新建对话                          │
│  └── delete_ai_conversation   → 删除对话                          │
│                                                                    │
│  services/ai.rs (已有)                                            │
│  ├── build_openai_chat_url    → Deepseek API 兼容                 │
│  ├── stream_chat              → reqwest SSE 流式处理              │
│  └── emit("ai:token")         → 前端实时渲染                     │
└──────────────────────────────────────────────────────────────────┘
```

---

## 3. 文件结构

```
dev-plugins/deepseek-tui/
├── plugin.json          # 插件清单
├── main.js              # 插件入口 (onLoad/onUnload)
├── terminal.js          # 终端 UI 渲染引擎
├── api.js               # Deepseek API 封装
├── styles.css           # 终端风格样式
└── README.md            # 使用文档
```

---

## 4. plugin.json

```json
{
  "id": "deepseek-tui",
  "name": "Deepseek TUI",
  "version": "1.0.0",
  "description": "Deepseek 终端风格 AI 对话面板，复用现有 AI 模型配置",
  "author": "Intelligent-NoteBook",
  "main": "main.js",
  "styles": "styles.css",
  "minAppVersion": "1.0.0",
  "permissions": [
    "settings:read"
  ],
  "contributes": {
    "commands": [
      { "id": "deepseek.open", "title": "打开 Deepseek 终端" },
      { "id": "deepseek.new-session", "title": "新建 Deepseek 会话" }
    ]
  }
}
```

---

## 5. 核心代码设计

### 5.1 main.js — 插件入口

```js
// 模块级变量（onUnload 清理）
var terminalInstance = null;
var offCommandOpen = null;
var offCommandNew = null;
var unregisterView = null;
var unregisterRibbon = null;

function onLoad(ctx) {
  var app = ctx.app;

  // 注册终端面板视图
  unregisterView = app.panelViews.register({
    id: "deepseek-terminal",
    title: "Deepseek 终端",
    icon: "Terminal",
    render: function (container) {
      // 动态加载 terminal.js 并初始化
      var script = document.createElement("script");
      script.textContent = TERMINAL_JS_SOURCE; // 内联或通过 app.assets 读取
      document.head.appendChild(script);
      terminalInstance = DeepseekTerminal.create(container, app);
    },
  });

  // 右下角 Ribbon 按钮
  unregisterRibbon = app.ribbon.addItem({
    id: "deepseek-toggle",
    icon: "BrainCircuit",
    tooltip: "Deepseek 终端",
    onClick: function () {
      app.panelViews.toggle("deepseek-terminal");
    },
  });

  // 命令面板注册
  offCommandOpen = app.commands.addCommand({
    id: "deepseek.open",
    title: "打开 Deepseek 终端",
    callback: function () {
      app.panelViews.open("deepseek-terminal");
    },
  });

  offCommandNew = app.commands.addCommand({
    id: "deepseek.new-session",
    title: "新建 Deepseek 会话",
    callback: function () {
      if (terminalInstance) terminalInstance.newSession();
    },
  });
}

function onUnload(ctx) {
  if (terminalInstance) { terminalInstance.destroy(); terminalInstance = null; }
  if (offCommandOpen) { offCommandOpen(); offCommandOpen = null; }
  if (offCommandNew) { offCommandNew(); offCommandNew = null; }
  if (unregisterView) { unregisterView(); unregisterView = null; }
  if (unregisterRibbon) { unregisterRibbon(); unregisterRibbon = null; }
}

module.exports = { onLoad: onLoad, onUnload: onUnload };
```

### 5.2 terminal.js — 终端 UI 渲染引擎

核心能力：

- **DOM 渲染**: 纯 `document.createElement` / `innerHTML`，不依赖 React
- **输入处理**: 监听 `keydown`，支持 Enter(发送) / Shift+Enter(换行) / Ctrl+N(新建) / Ctrl+L(清屏)
- **流式追加**: 最后一次 AI 回复用 `<span id="cursor">` 标记，收到 `ai:token` 后向前插入字符
- **Markdown 渲染**: 代码块用 `<pre><code>`，行内代码用 `<code>`，表格用 `<table>`
- **自动滚动**: 每次内容更新后 `scrollTop = scrollHeight`

```js
var DeepseekTerminal = (function () {
  function create(container, app) {
    var el = {};

    // 创建 DOM 结构
    el.root = document.createElement("div");
    el.root.className = "deepseek-terminal";

    el.output = document.createElement("div");
    el.output.className = "deepseek-terminal-output";

    el.inputLine = document.createElement("div");
    el.inputLine.className = "deepseek-terminal-input-line";
    el.inputLine.innerHTML =
      '<span class="prompt">$ </span>' +
      '<textarea class="input-area" rows="1" placeholder="输入消息..."></textarea>';

    el.statusBar = document.createElement("div");
    el.statusBar.className = "deepseek-terminal-status";
    el.statusBar.textContent = "Ctrl+N 新会话 | Ctrl+L 清屏 | Enter 发送 | Shift+Enter 换行";

    el.root.append(el.output, el.inputLine, el.statusBar);
    container.appendChild(el.root);

    // 事件绑定
    el.input = el.inputLine.querySelector(".input-area");
    el.input.addEventListener("keydown", handleKeydown);

    // 监听流式 Token
    var unlisten = null;
    app.events.on("ai:token", function (data) { appendToken(el, data); });

    // ... 更多实现

    return { destroy: function () { /* cleanup */ }, newSession: function () { /* ... */ } };
  }

  return { create: create };
})();
```

### 5.3 api.js — API 封装

```js
var DeepseekAPI = (function () {
  // 复用现有 Rust AI Command
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

  function deleteConversation(id) {
    return invoke("delete_ai_conversation", { id: id });
  }

  return {
    send: send,
    listConversations: listConversations,
    createConversation: createConversation,
    deleteConversation: deleteConversation,
  };
})();
```

---

## 6. UI 设计草图

```
┌──────────────────────────────────────────┐
│  🤖 Deepseek · 新对话 1           [✕]   │  ← 标题栏 (由 PanelView 框架提供)
├──────────────────────────────────────────┤
│                                          │
│  $ 你好，请介绍一下你自己                 │  ← 用户输入 (绿色)
│                                          │
│  ── Deepseek ────────────────────────── │
│  你好！我是 DeepSeek，由深度求索公司      │  ← AI 回复 (白色/灰色)
│  创造的 AI 助手，我可以回答问题、         │
│  编写代码、分析文档...                   │
│                                          │
│  $ 帮我写一个 Rust 快速排序              │
│                                          │
│  ── Deepseek ────────────────────────── │  ← 代码块渲染
│  \`\`\`rust                              │
│  fn quicksort<T: Ord>(arr: &mut [T]) {  │
│      if arr.len() <= 1 { return; }      │
│      let pivot = partition(arr);        │
│      quicksort(&mut arr[..pivot]);      │
│      quicksort(&mut arr[pivot+1..]);    │
│  }                                      │
│  \`\`\`                                  │
│                                          │
│  ▋ $ _                                   │  ← 输入行 (光标闪烁)
├──────────────────────────────────────────┤
│  Ctrl+N 新会话 | Ctrl+L 清屏 | Enter 发送 │  ← 状态栏快捷键提示
└──────────────────────────────────────────┘
```

---

## 7. styles.css — 终端风格

核心设计令牌：

```css
.deepseek-terminal {
  --dt-bg: #0d1117;           /* GitHub Dark 背景 */
  --dt-fg: #c9d1d9;           /* 主文字 */
  --dt-prompt: #3fb950;       /* 绿色提示符 */
  --dt-user: #58a6ff;         /* 用户输入 */
  --dt-ai: #8b949e;           /* AI 回复 */
  --dt-code-bg: #161b22;      /* 代码块背景 */
  --dt-code-fg: #c9d1d9;      /* 代码块文字 */
  --dt-border: #30363d;        /* 边框 */
  --dt-status: #484f58;       /* 状态栏 */

  background: var(--dt-bg);
  color: var(--dt-fg);
  font-family: "Cascadia Code", "Fira Code", "JetBrains Mono", "Consolas", monospace;
  font-size: 14px;
  line-height: 1.6;
  height: 100%;
  display: flex;
  flex-direction: column;
}

.deepseek-terminal-output {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
  scroll-behavior: smooth;
}

.deepseek-terminal-input-line {
  display: flex;
  align-items: flex-start;
  padding: 8px 16px;
  border-top: 1px solid var(--dt-border);
}

.deepseek-terminal-input-line .prompt {
  color: var(--dt-prompt);
  font-weight: bold;
  margin-right: 8px;
}

.deepseek-terminal-input-line .input-area {
  flex: 1;
  background: transparent;
  border: none;
  color: var(--dt-fg);
  font-family: inherit;
  font-size: inherit;
  resize: none;
  outline: none;
}

.deepseek-terminal-status {
  padding: 4px 16px;
  font-size: 11px;
  color: var(--dt-status);
  border-top: 1px solid var(--dt-border);
}
```

---

## 8. 调用链路

```
用户输入 "你好" → Enter
  │
  ▼
terminal.js: handleKeydown
  │
  ▼
api.js: invoke("send_ai_message", { conversationId: 1, content: "你好" })
  │
  ▼
commands/ai.rs: send_ai_message()
  │
  ▼
services/ai.rs: AiService::stream_chat()
  │  ├── reqwest POST → https://api.deepseek.com/v1/chat/completions
  │  └── SSE stream → 逐行读取 JSON chunks
  │
  ▼
emit("ai:token", token_content)  ← 每个 token 发射一次
  │
  ▼
terminal.js: listen("ai:token") → appendToken()
  │
  ▼
DOM 更新：向前追加字符 + 自动滚动到底部
```

---

## 9. 与现有系统关系

| 现有系统 | 关系 | 说明 |
|---------|------|------|
| AI 模型管理（设置页） | 复用 | 用户在设置中配置 Deepseek API Key 和 URL，插件自动使用默认模型 |
| AI 对话系统 | 复用 | 对话存储、历史消息全部走现有 `ai_conversations` 表 |
| AI 抽屉（NoteAiDrawer） | 互补 | 插件是独立的终端面板，与现有 AI 抽屉共存，互不干扰 |
| 插件系统 | 寄生 | 作为标准插件运行，通过 AppAPI 令牌+权限机制安全隔离 |
| 命令面板 | 集成 | 注册 `deepseek.open` 命令，用户可通过 Ctrl+Shift+P 打开 |

---

## 10. 开发量估算

| 文件 | 内容 | 行数 |
|------|------|------|
| `plugin.json` | 插件清单 | ~15 |
| `main.js` | 插件生命周期 + 注册 | ~80 |
| `terminal.js` | 终端 UI 渲染引擎 | ~250 |
| `api.js` | API 封装层 | ~60 |
| `styles.css` | 终端样式 | ~100 |
| `README.md` | 使用说明 | ~30 |
| **总计** | | **~535 行，1-2 天** |

---

## 11. 无需改动的部分

- ✅ **Rust 后端**: 零改动，100% 复用现有 AI Command
- ✅ **数据库 Schema**: 零改动，复用 `ai_conversations` / `ai_messages` 表
- ✅ **Capabilities**: 零改动，插件通过 AppAPI 受限访问
- ✅ **前端路由**: 零改动，作为 PanelView 渲染在现有布局中
- ✅ **前端构建**: 零改动，插件 JS 在运行时动态加载
