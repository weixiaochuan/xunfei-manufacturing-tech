var CodeAssistantPrompts = {
  explain: function (code) {
    return [
      { role: "system", content: "你是资深编程导师。请用中文解释代码，结构包括：整体功能、关键逻辑、重要边界条件。不要改写代码。" },
      { role: "user", content: "请解释以下代码：\n```\n" + code + "\n```" },
    ];
  },
  review: function (code) {
    return [
      { role: "system", content: "你是代码审查专家。请用中文审查代码，重点检查：潜在 Bug、安全风险、性能问题、可读性改进。输出分条建议。" },
      { role: "user", content: "请审查以下代码：\n```\n" + code + "\n```" },
    ];
  },
  refactor: function (code) {
    return [
      { role: "system", content: "你是代码重构专家。请在保持功能不变的前提下重构代码，优先提高可读性、可维护性和安全性。输出重构后的代码，并简述改动点。" },
      { role: "user", content: "请重构以下代码：\n```\n" + code + "\n```" },
    ];
  },
  genTest: function (code) {
    return [
      { role: "system", content: "你是测试工程师。请为给定代码生成单元测试，覆盖正常路径、边界条件和错误输入。若语言不明确，请根据代码判断。" },
      { role: "user", content: "请为以下代码生成测试：\n```\n" + code + "\n```" },
    ];
  },
};

var CodeAssistantUI = (function () {
  var modal = null;
  var contentEl = null;
  var cancelFn = null;

  function open(title) {
    close();
    modal = document.createElement("div");
    modal.className = "ca-overlay";
    modal.innerHTML = '<div class="ca-modal"><div class="ca-header"><span class="ca-title"></span><button class="ca-close">×</button></div><pre class="ca-content"></pre><div class="ca-footer"><button class="ca-copy">复制</button><button class="ca-cancel">取消生成</button></div></div>';
    modal.querySelector(".ca-title").textContent = title;
    contentEl = modal.querySelector(".ca-content");
    modal.querySelector(".ca-close").addEventListener("click", close);
    modal.querySelector(".ca-copy").addEventListener("click", function () {
      navigator.clipboard && navigator.clipboard.writeText(contentEl.textContent || "");
    });
    modal.querySelector(".ca-cancel").addEventListener("click", function () {
      if (cancelFn) cancelFn();
    });
    document.body.appendChild(modal);
  }

  function append(text) {
    if (!contentEl) return;
    contentEl.textContent += text;
    contentEl.scrollTop = contentEl.scrollHeight;
  }

  function setCancel(fn) {
    cancelFn = fn;
  }

  function close() {
    if (cancelFn) {
      try { cancelFn(); } catch (e) {}
      cancelFn = null;
    }
    if (modal && modal.parentNode) modal.parentNode.removeChild(modal);
    modal = null;
    contentEl = null;
  }

  return { open: open, append: append, setCancel: setCancel, close: close };
})();

module.exports = {
  async onLoad(ctx) {
    var app = ctx.app;

    async function runAction(kind, title) {
      var selection = app.editor && app.editor.getCurrentSelection ? app.editor.getCurrentSelection() : "";
      if (!selection || !selection.trim()) {
        app.notices.show("请先在编辑器中选中代码");
        return;
      }
      var promptFactory = CodeAssistantPrompts[kind];
      if (!promptFactory) return;
      CodeAssistantUI.open(title);
      try {
        var cancel = await app.ai.chat(promptFactory(selection), {
          onToken: function (token) { CodeAssistantUI.append(token); },
          onDone: function () { CodeAssistantUI.setCancel(null); },
          onError: function (err) {
            CodeAssistantUI.append("\n\n[错误] " + err);
            CodeAssistantUI.setCancel(null);
          },
        });
        CodeAssistantUI.setCancel(cancel);
      } catch (e) {
        CodeAssistantUI.append("\n\n[错误] " + e);
      }
    }

    app.commands.addCommand({ id: "ai.explain-code", name: "AI: 解释代码", group: "AI", callback: function () { runAction("explain", "AI: 解释代码"); } });
    app.commands.addCommand({ id: "ai.review-code", name: "AI: 审查代码", group: "AI", callback: function () { runAction("review", "AI: 审查代码"); } });
    app.commands.addCommand({ id: "ai.refactor-code", name: "AI: 重构代码", group: "AI", callback: function () { runAction("refactor", "AI: 重构代码"); } });
    app.commands.addCommand({ id: "ai.gen-test", name: "AI: 生成测试", group: "AI", callback: function () { runAction("genTest", "AI: 生成测试"); } });

    if (app.editor && app.editor.addContextMenuItem) {
      app.editor.addContextMenuItem({ id: "ca-explain", label: "AI: 解释代码", icon: "Sparkles", when: "has-selection", callback: function () { runAction("explain", "AI: 解释代码"); } });
      app.editor.addContextMenuItem({ id: "ca-review", label: "AI: 审查代码", icon: "SearchCheck", when: "has-selection", callback: function () { runAction("review", "AI: 审查代码"); } });
      app.editor.addContextMenuItem({ id: "ca-refactor", label: "AI: 重构代码", icon: "Wand2", when: "has-selection", callback: function () { runAction("refactor", "AI: 重构代码"); } });
    }

    console.log("[code-assistant] 激活完成");
  },

  async onUnload() {
    CodeAssistantUI.close();
  },
};
