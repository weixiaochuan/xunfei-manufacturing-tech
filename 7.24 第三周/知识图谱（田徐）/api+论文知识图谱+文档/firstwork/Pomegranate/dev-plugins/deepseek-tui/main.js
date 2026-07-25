/**
 * Deepseek-TUI 插件 (单文件)
 *
 * 通过 ctx.app.invoke / ctx.app.onTauriEvent 桥接调用现有 AI Command，
 * 零 Rust 改动。终端 UI 纯 DOM 渲染，参考 DeepSeek TUI 暗色配色。
 *
 * v1.1 — 输入框优化：模型选择器 + 权限模式 + 上下文进度条 + Git 信息
 */

/* ================================================================
 * DeepseekAPI — AI API 封装层 + 状态管理
 * ================================================================ */
var DeepseekAPI = (function () {
  var _app = null;
  var _conversationId = null;
  var _listeners = [];  // 所有事件监听器清理句柄
  var _creatingPromise = null;  // 防止并发创建对话的 Promise 锁

  // ─── v1.1 新增状态 ───
  var _models = [];              // 模型列表缓存 [{id, name, provider, model_id, is_default, max_context}]
  var _selectedModelId = null;    // 当前选中的模型 ID
  var _permissionMode = "ask";   // ask | auto-edit | plan | skip

  function init(app) { if (!_app) { _app = app; } }

  // ─── v1.1 模型管理 ───
  async function loadModels() {
    try {
      var list = await _app.invoke("list_ai_models");
      _models = list || [];
      if (_models.length > 0) {
        // 选默认模型，没有默认则选第一个
        var def = null;
        for (var i = 0; i < _models.length; i++) {
          if (_models[i].is_default) { def = _models[i]; break; }
        }
        _selectedModelId = def ? def.id : _models[0].id;
      }
    } catch (e) {
      console.warn("[deepseek-tui] 加载模型列表失败:", e);
      _models = [];
    }
    return _models;
  }

  function getModels() { return _models; }
  function getSelectedModelId() { return _selectedModelId; }
  function setSelectedModelId(id) { _selectedModelId = id; }

  // ─── v1.1 权限模式 ───
  function getPermissionMode() { return _permissionMode; }
  function setPermissionMode(mode) { _permissionMode = mode; }

  /** 返回当前权限模式对应的系统提示前缀 */
  function getPermissionPrompt() {
    switch (_permissionMode) {
      case "auto-edit":
        return "[模式: 自动编辑] 请直接进行文件编辑操作，无需询问确认。\n\n";
      case "plan":
        return "[模式: 计划模式] 请先制定执行计划，说明每一步要做什么，等我确认后再执行。\n\n";
      case "skip":
        return "[模式: 跳过权限] 请自由执行所有操作，无需任何权限检查。\n\n";
      case "ask":
      default:
        return "";
    }
  }

  // ─── v1.1 Git 信息 ───
  async function loadGitInfo() {
    try {
      return await _app.invoke("get_git_info");
    } catch (e) {
      console.warn("[deepseek-tui] 获取 git 信息失败:", e);
      return null;
    }
  }

  // ─── v1.1 上下文计算 ───
  async function calcContextUsage() {
    try {
      var msgs = await getHistory();
      if (!msgs || !msgs.length) return 0;
      var totalChars = 0;
      for (var i = 0; i < msgs.length; i++) {
        totalChars += (msgs[i].content || "").length;
      }
      // 找当前选中模型的 max_context
      var maxCtx = 32000;
      for (var j = 0; j < _models.length; j++) {
        if (_models[j].id === _selectedModelId) {
          maxCtx = _models[j].max_context || 32000;
          break;
        }
      }
      return Math.min(Math.round((totalChars / maxCtx) * 100), 99);
    } catch (e) {
      return 0;
    }
  }

  async function ensureConversation() {
    if (_conversationId) return _conversationId;
    // 防止并发调用创建多个对话
    if (_creatingPromise) return _creatingPromise;
    var args = { title: "Deepseek 终端对话" };
    if (_selectedModelId) args.model_id = _selectedModelId;
    _creatingPromise = _app.invoke("create_ai_conversation", args).then(function (conv) {
      _conversationId = conv.id;
      _creatingPromise = null;
      return _conversationId;
    }).catch(function (e) {
      _creatingPromise = null;
      throw e;
    });
    return _creatingPromise;
  }

  async function send(message, callbacks) {
    var cid = await ensureConversation();
    cleanup();

    // v1.1: 拼接权限模式前缀
    var prompt = getPermissionPrompt();
    var finalMessage = prompt ? prompt + message : message;

    var fullText = "";

    _listeners.push(await _app.onTauriEvent("ai:token", function (payload) {
      var token = typeof payload === "string" ? payload : (payload && payload.token);
      if (token) {
        fullText += token;
        if (callbacks && callbacks.onToken) {
          callbacks.onToken(token, fullText);
        }
      }
    }));
    _listeners.push(await _app.onTauriEvent("ai:done", function () {
      cleanup();
      if (callbacks && callbacks.onDone) callbacks.onDone(fullText);
    }));
    _listeners.push(await _app.onTauriEvent("ai:error", function (payload) {
      cleanup();
      var err = typeof payload === "string" ? payload : (payload && payload.error) || "未知错误";
      if (callbacks && callbacks.onError) callbacks.onError(err);
    }));

    await _app.invoke("send_ai_message", { conversationId: cid, message: finalMessage });
    return function cancel() {
      cleanup();
      _app.invoke("cancel_ai_generation", { conversationId: cid }).catch(function () {});
    };
  }

  async function newSession(title) {
    _conversationId = null;
    cleanup();
    var args = { title: title || "Deepseek 终端对话" };
    if (_selectedModelId) args.model_id = _selectedModelId;
    var conv = await _app.invoke("create_ai_conversation", args);
    _conversationId = conv.id;
    return conv;
  }

  async function getHistory() {
    var cid = await ensureConversation();
    return _app.invoke("list_ai_messages", { conversationId: cid });
  }

  async function listConversations() {
    return _app.invoke("list_ai_conversations");
  }

  function switchConversation(id) {
    cleanup();
    _conversationId = id;
  }

  function getConversationId() { return _conversationId; }

  function cleanup() {
    for (var i = 0; i < _listeners.length; i++) {
      try { _listeners[i](); } catch (e) {}
    }
    _listeners = [];
  }

  function destroy() {
    cleanup();
    _conversationId = null;
    _app = null;
    _models = [];
    _selectedModelId = null;
  }

  return {
    init: init, send: send, newSession: newSession, getHistory: getHistory,
    listConversations: listConversations, switchConversation: switchConversation,
    getConversationId: getConversationId, cleanup: cleanup, destroy: destroy,
    // v1.1
    loadModels: loadModels, getModels: getModels,
    getSelectedModelId: getSelectedModelId, setSelectedModelId: setSelectedModelId,
    getPermissionMode: getPermissionMode, setPermissionMode: setPermissionMode,
    getPermissionPrompt: getPermissionPrompt,
    loadGitInfo: loadGitInfo, calcContextUsage: calcContextUsage,
    getActiveProvider: function () {
      return _app.invoke("get_active_provider").catch(function () { return null; });
    },
  };
})();

/* ================================================================
 * DeepseekTerminal — 终端 UI 渲染引擎 (纯 DOM)
 * ================================================================ */
var DeepseekTerminal = (function () {
  var PROMPT_SYMBOL = "\u25B6";
  var USER_PREFIX = "$ ";
  var AI_PREFIX = "\u2500\u2500 ";

  function create(container, app) {
    var el = {};
    var state = { streaming: false, streamingBlock: null, conversations: [] };

    // ─── 根容器 flex row ───
    el.root = document.createElement("div");
    el.root.className = "dt-root";

    // ─── 左侧会话列表面板 ───
    var sidebar = document.createElement("div");
    sidebar.className = "dt-sidebar";

    var sidebarHeader = document.createElement("div");
    sidebarHeader.className = "dt-sidebar-header";
    sidebarHeader.innerHTML = '<span class="dt-sidebar-title">会话</span>';

    var newBtn = document.createElement("button");
    newBtn.className = "dt-sidebar-new-btn";
    newBtn.textContent = "+";
    newBtn.title = "新建会话";
    newBtn.addEventListener("click", function () { handleNewSession(); });
    sidebarHeader.appendChild(newBtn);
    sidebar.appendChild(sidebarHeader);

    var sidebarList = document.createElement("div");
    sidebarList.className = "dt-sidebar-list";
    sidebar.appendChild(sidebarList);

    el.root.appendChild(sidebar);

    // ─── 右侧主区域 ───
    var mainArea = document.createElement("div");
    mainArea.className = "dt-main";

    el.output = document.createElement("div");
    el.output.className = "dt-output";
    mainArea.appendChild(el.output);

    // ═══════════════════════════════════════════════════════
    // v1.1 工具栏行 — 模型选择器 | 权限选择器 | 上下文进度条
    // ═══════════════════════════════════════════════════════
    el.toolbar = document.createElement("div");
    el.toolbar.className = "dt-toolbar";

    // A) 模型选择器
    var modelGroup = document.createElement("div");
    modelGroup.className = "dt-toolbar-group";

    var modelLabel = document.createElement("span");
    modelLabel.className = "dt-toolbar-label";
    modelLabel.textContent = "模型";
    modelGroup.appendChild(modelLabel);

    el.modelSelect = document.createElement("select");
    el.modelSelect.className = "dt-select dt-select-model";
    modelGroup.appendChild(el.modelSelect);
    el.toolbar.appendChild(modelGroup);

    // B) 权限模式选择器
    var permGroup = document.createElement("div");
    permGroup.className = "dt-toolbar-group";

    var permLabel = document.createElement("span");
    permLabel.className = "dt-toolbar-label";
    permLabel.textContent = "权限";
    permGroup.appendChild(permLabel);

    el.permSelect = document.createElement("select");
    el.permSelect.className = "dt-select dt-select-perm";
    var PERM_OPTIONS = [
      { value: "ask", label: "询问" },
      { value: "auto-edit", label: "自动编辑" },
      { value: "plan", label: "计划模式" },
      { value: "skip", label: "跳过权限" },
    ];
    for (var p = 0; p < PERM_OPTIONS.length; p++) {
      var opt = document.createElement("option");
      opt.value = PERM_OPTIONS[p].value;
      opt.textContent = PERM_OPTIONS[p].label;
      if (PERM_OPTIONS[p].value === DeepseekAPI.getPermissionMode()) opt.selected = true;
      el.permSelect.appendChild(opt);
    }
    permGroup.appendChild(el.permSelect);
    el.toolbar.appendChild(permGroup);

    // C) 上下文进度条
    el.contextGroup = document.createElement("div");
    el.contextGroup.className = "dt-toolbar-group dt-context-group";

    el.contextLabel = document.createElement("span");
    el.contextLabel.className = "dt-context-label";
    el.contextLabel.textContent = "上下文 0%";
    el.contextGroup.appendChild(el.contextLabel);

    el.contextBarWrap = document.createElement("div");
    el.contextBarWrap.className = "dt-context-bar";
    el.contextFill = document.createElement("div");
    el.contextFill.className = "dt-context-fill";
    el.contextBarWrap.appendChild(el.contextFill);
    el.contextGroup.appendChild(el.contextBarWrap);

    el.toolbar.appendChild(el.contextGroup);
    mainArea.appendChild(el.toolbar);

    // ─── 工具栏事件 ───
    el.modelSelect.addEventListener("change", function () {
      var newModelId = parseInt(el.modelSelect.value, 10);
      if (isNaN(newModelId)) return;
      DeepseekAPI.setSelectedModelId(newModelId);
      var cid = DeepseekAPI.getConversationId();
      if (cid) {
        app.invoke("update_ai_conversation_model", { id: cid, modelId: newModelId })
          .then(function () { refreshContext(); })
          .catch(function (e) { console.warn("[deepseek-tui] 切换模型失败:", e); });
      } else {
        refreshContext();
      }
    });

    el.permSelect.addEventListener("change", function () {
      DeepseekAPI.setPermissionMode(el.permSelect.value);
    });

    // ─── 输入区 ───
    el.inputWrapper = document.createElement("div");
    el.inputWrapper.className = "dt-input-wrapper";
    el.promptSpan = document.createElement("span");
    el.promptSpan.className = "dt-prompt";
    el.promptSpan.textContent = PROMPT_SYMBOL + " ";
    el.input = document.createElement("textarea");
    el.input.className = "dt-input";
    el.input.rows = 1;
    el.input.placeholder = "输入消息，Enter 发送，Shift+Enter 换行...";
    el.inputWrapper.appendChild(el.promptSpan);
    el.inputWrapper.appendChild(el.input);
    mainArea.appendChild(el.inputWrapper);

    // v1.1 状态栏 — 快捷键 + Git 信息
    el.statusBar = document.createElement("div");
    el.statusBar.className = "dt-statusbar";

    var shortcutsSpan = document.createElement("span");
    shortcutsSpan.className = "dt-shortcuts";
    shortcutsSpan.textContent = "Ctrl+N 新会话 | Ctrl+L 清屏 | Enter 发送 | Escape 取消";
    el.statusBar.appendChild(shortcutsSpan);

    el.gitSpan = document.createElement("span");
    el.gitSpan.className = "dt-git-info";
    el.statusBar.appendChild(el.gitSpan);

    var providerSep = document.createElement("span");
    providerSep.className = "dt-status-sep";
    providerSep.textContent = " \u2502 ";
    el.statusBar.appendChild(providerSep);

    el.providerSpan = document.createElement("span");
    el.providerSpan.className = "dt-provider-info";
    el.statusBar.appendChild(el.providerSpan);

    mainArea.appendChild(el.statusBar);

    el.root.appendChild(mainArea);
    container.appendChild(el.root);

    // ─── 输入事件 ───
    el.input.addEventListener("keydown", function (e) {
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); }
      else if (e.ctrlKey && e.key === "l") { e.preventDefault(); clearOutput(); }
      else if (e.ctrlKey && e.key === "n") { e.preventDefault(); handleNewSession(); }
      else if (e.key === "Escape") { e.preventDefault(); handleCancel(); }
    });
    el.input.addEventListener("input", function () {
      el.input.style.height = "auto";
      el.input.style.height = Math.min(el.input.scrollHeight, 200) + "px";
    });

    function scrollBottom() { el.output.scrollTop = el.output.scrollHeight; }
    function handleCancel() { if (state.streaming) api.cancel(); }

    // ─── v1.1 模型选择器渲染 ───
    function renderModelSelect() {
      var models = DeepseekAPI.getModels();
      var selectedId = DeepseekAPI.getSelectedModelId();
      el.modelSelect.innerHTML = "";
      for (var i = 0; i < models.length; i++) {
        var m = models[i];
        var opt = document.createElement("option");
        opt.value = m.id;
        opt.textContent = m.provider + ": " + m.model_id + (m.is_default ? " (默认)" : "");
        if (m.id === selectedId) opt.selected = true;
        el.modelSelect.appendChild(opt);
      }
    }

    // ─── v1.1 Git 信息渲染 ───
    async function updateGitInfo() {
      try {
        var info = await DeepseekAPI.loadGitInfo();
        if (!info || !info.branch) {
          el.gitSpan.style.display = "none";
          return;
        }
        el.gitSpan.style.display = "";
        var parts = [];
        parts.push("\uD83C\uDF3F " + info.branch);
        if (info.changed > 0) parts.push("\u25CF" + info.changed);
        if (info.staged > 0) parts.push("+" + info.staged);
        if (info.untracked > 0) parts.push("~" + info.untracked);
        if (info.ahead > 0) parts.push("\u2191" + info.ahead);
        if (info.behind > 0) parts.push("\u2193" + info.behind);
        el.gitSpan.textContent = parts.join(" ");
      } catch (e) {
        el.gitSpan.style.display = "none";
      }
    }

    // ─── v1.1 Provider 信息渲染 ───
    async function updateProviderInfo() {
      try {
        var info = await DeepseekAPI.getActiveProvider();
        if (!info || !info.provider_type) {
          el.providerSpan.style.display = "none";
          return;
        }
        el.providerSpan.style.display = "";
        el.providerSpan.textContent = info.provider_type + ": " + info.model_id;
      } catch (e) {
        el.providerSpan.style.display = "none";
      }
    }

    // ─── v1.1 上下文刷新 ───
    async function refreshContext() {
      try {
        var pct = await DeepseekAPI.calcContextUsage();
        el.contextLabel.textContent = "上下文 " + pct + "%";
        el.contextFill.style.width = pct + "%";
        // 三档颜色
        var cls = "dt-context-fill";
        if (pct > 80) cls += " high";
        else if (pct > 50) cls += " medium";
        else cls += " low";
        el.contextFill.className = cls;
      } catch (e) {
        // 静默失败
      }
    }

    // ─── 会话列表渲染 ───
    function renderConversationList() {
      sidebarList.innerHTML = "";
      var activeId = DeepseekAPI.getConversationId();
      for (var i = 0; i < state.conversations.length; i++) {
        var conv = state.conversations[parseInt(i,10)];
        var item = document.createElement("div");
        item.className = "dt-conv-item" + (conv.id === activeId ? " dt-conv-active" : "");
        item.title = conv.title || ("对话 #" + conv.id);
        item.textContent = conv.title || ("对话 #" + conv.id);
        item.addEventListener("click", (function (c) {
          return function () {
            DeepseekAPI.switchConversation(c.id);
            clearOutput();
            loadHistory();
            renderConversationList();
            refreshContext();
          };
        })(conv));
        sidebarList.appendChild(item);
      }
    }

    async function loadConversations() {
      try {
        var list = await DeepseekAPI.listConversations();
        state.conversations = list || [];
        renderConversationList();
      } catch (e) { console.warn("[deepseek-tui] 加载会话列表失败:", e); }
    }

    async function loadHistory() {
      try {
        var msgs = await DeepseekAPI.getHistory();
        if (!msgs || !msgs.length) return;
        for (var i = 0; i < msgs.length; i++) {
          var m = msgs[parseInt(i,10)];
          if (m.role === "user") {
            var block = document.createElement("div");
            block.className = "dt-msg dt-msg-user";
            block.innerHTML = '<span class="dt-user-prefix">' + USER_PREFIX + '</span><span class="dt-user-text"></span>';
            block.querySelector(".dt-user-text").textContent = m.content || "";
            el.output.appendChild(block);
          } else if (m.role === "assistant") {
            var ablock = document.createElement("div");
            ablock.className = "dt-msg dt-msg-ai";
            ablock.innerHTML = '<div class="dt-ai-prefix">' + AI_PREFIX + 'Deepseek</div><div class="dt-ai-text"></div>';
            var textDiv = ablock.querySelector(".dt-ai-text");
            textDiv.textContent = m.content || "";
            renderMarkdownBlocks(textDiv, m.content || "");
            el.output.appendChild(ablock);
          }
        }
        scrollBottom();
      } catch (e) { console.warn("[deepseek-tui] 加载历史失败:", e); }
    }

    var api = {
      addUserMessage: function (text) {
        var block = document.createElement("div");
        block.className = "dt-msg dt-msg-user";
        var prefix = document.createElement("span");
        prefix.className = "dt-user-prefix";
        prefix.textContent = USER_PREFIX;
        var content = document.createElement("span");
        content.className = "dt-user-text";
        content.textContent = text;
        block.appendChild(prefix);
        block.appendChild(content);
        el.output.appendChild(block);
        scrollBottom();
      },
      startAiMessage: function () {
        state.streaming = true;
        var block = document.createElement("div");
        block.className = "dt-msg dt-msg-ai";
        var prefix = document.createElement("div");
        prefix.className = "dt-ai-prefix";
        prefix.textContent = AI_PREFIX + "Deepseek";
        var content = document.createElement("div");
        content.className = "dt-ai-text";
        content.innerHTML = '<span class="dt-cursor">\u258B</span>';
        block.appendChild(prefix);
        block.appendChild(content);
        el.output.appendChild(block);
        state.streamingBlock = block;
        state.streamingContent = content;
        scrollBottom();
      },
      appendAiToken: function (token) {
        if (!state || !state.streamingContent) return;
        var cursor = state.streamingContent.querySelector(".dt-cursor");
        if (cursor) cursor.remove();
        var span = document.createElement("span");
        span.textContent = token;
        state.streamingContent.appendChild(span);
        var newCursor = document.createElement("span");
        newCursor.className = "dt-cursor";
        newCursor.textContent = "\u258B";
        state.streamingContent.appendChild(newCursor);
        scrollBottom();
      },
      finishAiMessage: function (fullText) {
        if (!state) return;
        state.streaming = false;
        if (state.streamingContent) {
          var cursor = state.streamingContent.querySelector(".dt-cursor");
          if (cursor) cursor.remove();
          renderMarkdownBlocks(state.streamingContent, fullText);
        }
        state.streamingBlock = null;
        state.streamingContent = null;
        loadConversations();
        refreshContext();
      },
      showAiError: function (error) {
        if (!state) return;
        state.streaming = false;
        if (state.streamingContent) {
          var cursor = state.streamingContent.querySelector(".dt-cursor");
          if (cursor) cursor.remove();
          var errSpan = document.createElement("span");
          errSpan.className = "dt-error";
          errSpan.textContent = " [错误: " + (error || "未知") + "]";
          state.streamingContent.appendChild(errSpan);
        }
        state.streamingBlock = null;
        state.streamingContent = null;
      },
      getInput: function () {
        var text = el.input.value.trim();
        el.input.value = "";
        el.input.style.height = "auto";
        return text;
      },
      focus: function () { el.input.focus(); },
      clear: function () { clearOutput(); },
      newSession: function () { handleNewSession(); },
      destroy: function () {
        if (el.root && el.root.parentNode) el.root.parentNode.removeChild(el.root);
        state = null; el = {};
      },
    };

    // v1.1: 修改发送逻辑，完成后刷新上下文
    async function handleSend() {
      var text = api.getInput();
      if (!text || state.streaming) return;
      // 权限前缀在 DeepseekAPI.send 内部拼接，这里直接传原始文本用于展示
      api.addUserMessage(text);
      api.startAiMessage();
      try {
        var cancel = await DeepseekAPI.send(text, {
          onToken: function (token) { api.appendAiToken(token); },
          onDone: function (fullText) { api.finishAiMessage(fullText); },
          onError: function (err) { api.showAiError(err); },
        });
        state.cancelFn = cancel;
      } catch (e) { api.showAiError(String(e)); }
    }

    async function handleNewSession() {
      try {
        await DeepseekAPI.newSession();
        clearOutput();
        loadConversations();
        refreshContext();
        var msg = document.createElement("div");
        msg.className = "dt-system-msg";
        msg.textContent = "\u2500\u2500 新会话已创建 \u2500\u2500";
        el.output.appendChild(msg);
        scrollBottom();
      } catch (e) { app.notices.error("新建会话失败: " + e); }
    }

    function clearOutput() { el.output.innerHTML = ""; }

    function renderMarkdownBlocks(container) {
      var html = container.innerHTML;
      html = html.replace(/```(\w*)\n?([\s\S]*?)```/g, function (_, lang, code) {
        var langLabel = lang ? '<span class="dt-code-lang">' + lang + '</span>' : '';
        return '<div class="dt-code-block">' + langLabel + '<pre><code>' + escapeHtml(code) + '</code></pre></div>';
      });
      html = html.replace(/`([^`]+)`/g, '<code class="dt-inline-code">$1</code>');
      container.innerHTML = html;
    }

    function escapeHtml(text) {
      return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    }

    DeepseekAPI.init(app);

    // ─── v1.1 初始化：加载模型列表 → 渲染选择器 → 加载 Git 信息 → 上下文 ───
    DeepseekAPI.loadModels().then(function () {
      renderModelSelect();
      refreshContext();
    });
    loadConversations();
    updateGitInfo();
    updateProviderInfo();
    refreshContext();
    api.focus();
    return api;
  }

  return { create: create };
})();

/* ================================================================
 * 插件入口 — onLoad / onUnload
 * ================================================================ */
var terminalInstance = null;

module.exports = {
  onLoad: function (ctx) {
    var app = ctx.app;

    app.panelViews.register({
      id: "deepseek-terminal",
      title: "Deepseek 终端",
      layout: "fullscreen",
      icon: "Terminal",
      render: function (container) {
        if (terminalInstance) terminalInstance.destroy();
        terminalInstance = DeepseekTerminal.create(container, app);
        return function cleanup() {
          if (terminalInstance) { terminalInstance.destroy(); terminalInstance = null; }
        };
      },
    });

    if (app.sidebar && typeof app.sidebar.addItem === "function") {
      app.sidebar.addItem({ id: "deepseek-terminal-sidebar", icon: "Terminal", label: "Deepseek", viewId: "deepseek-terminal" });
    }

    if (app.ribbon && typeof app.ribbon.addItem === "function") {
      app.ribbon.addItem({ id: "deepseek-toggle", icon: "BrainCircuit", tooltip: "Deepseek 终端",
        onClick: function () { app.notices.show("请通过侧边栏打开 Deepseek 终端"); } });
    }

    app.commands.addCommand({ id: "deepseek.open", name: "Deepseek: 打开终端", group: "AI",
      callback: function () { app.notices.show("请通过侧边栏打开 Deepseek 终端"); } });

    app.commands.addCommand({ id: "deepseek.new-session", name: "Deepseek: 新建会话", group: "AI",
      callback: function () {
        DeepseekAPI.newSession().then(function () {
          if (terminalInstance) terminalInstance.focus();
          app.notices.show("Deepseek 新会话已创建");
        }).catch(function (e) { app.notices.error("新建会话失败: " + e); });
      } });

    console.log("[deepseek-tui] v1.1 激活完成");
  },

  onUnload: function () {
    if (terminalInstance) { terminalInstance.destroy(); terminalInstance = null; }
    DeepseekAPI.destroy();
  },
};
