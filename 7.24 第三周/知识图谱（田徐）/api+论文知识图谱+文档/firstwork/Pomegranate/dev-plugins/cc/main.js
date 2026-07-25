/**
 * cc 示例插件 v2.1 —— 适配 AppAPI 1.1.0
 *
 * 演示新 API 的全部域：
 *   - commands   注册命令面板项
 *   - sidebar    注册侧边栏图标
 *   - panelViews 注册面板视图
 *   - ribbon     注册功能区按钮（1.1.0 新增）
 *   - notes      调用受控笔记 API
 *   - settings   读写插件自身设置
 */
module.exports = {
  async onLoad(ctx) {
    const { app, meta } = ctx;

    console.log(`[cc] 激活中... API version=${app.version}, plugin=${meta.id}`);

    // ===== 1. 注册命令 =====
    app.commands.addCommand({
      id: "say-hello",
      name: "CC: 打招呼",
      group: "示例",
      callback: () => {
        app.notices.show(`Hello from ${meta.name}!`);
      },
    });

    app.commands.addCommand({
      id: "count-notes",
      name: "CC: 统计笔记数量",
      group: "示例",
      callback: async () => {
        try {
          const notes = await app.notes.list();
          app.notices.show(`当前共有 ${notes.length} 篇笔记`);
        } catch (e) {
          app.notices.error(`查询失败: ${e}`);
        }
      },
    });

    // ===== 2. 注册侧边栏图标 =====
    app.sidebar.addItem({
      id: "cc-panel",
      icon: "Sparkles",
      label: "CC 示例",
      viewId: "cc-main-view",
    });

    // ===== 2.5 注册 Ribbon 功能区按钮（1.1.0 新增）=====
    // 存在性守卫：宿主未升到 1.1.0（或 HMR 后旧 PluginManager 单例残留）时跳过
    if (app.ribbon && typeof app.ribbon.addItem === "function") {
      app.ribbon.addItem({
        id: "cc-ribbon-stat",
        icon: "Zap",
        tooltip: "CC: 一键统计笔记数",
        onClick: async () => {
          try {
            const notes = await app.notes.list();
            app.notices.show(`📊 当前共 ${notes.length} 篇笔记`);
          } catch (e) {
            app.notices.error(`统计失败: ${e}`);
          }
        },
      });
    } else {
      console.warn("[cc] app.ribbon 不可用（宿主 < 1.1.0 或需重启 dev 服务）");
    }

    // ===== 2.6 注册编辑器右键菜单项（1.1.0 新增）=====
    if (app.editor && typeof app.editor.addContextMenuItem === "function") {
      app.editor.addContextMenuItem({
        id: "cc-uppercase",
        label: "CC: 选区转大写",
        icon: "Pencil",
        when: "has-selection",
        callback: (ctx) => {
          if (!ctx.selection) return;
          ctx.replaceSelection(ctx.selection.toUpperCase());
        },
      });

      app.editor.addContextMenuItem({
        id: "cc-insert-stamp",
        label: "CC: 插入时间戳",
        icon: "Calendar",
        when: "cursor",
        callback: (ctx) => {
          ctx.insertText(`\n[${new Date().toLocaleString()}] `);
        },
      });

      app.editor.addContextMenuItem({
        id: "cc-word-count",
        label: "CC: 显示全文字数",
        icon: "Hash",
        when: "always",
        callback: (ctx) => {
          const text = ctx.getContent();
          app.notices.show(`📝 全文 ${text.length} 字符`);
        },
      });
    } else {
      console.warn("[cc] app.editor 不可用（宿主 < 1.1.0 或需重启 dev 服务）");
    }

    // ===== 2.7 注册编辑器工具栏按钮（1.1.0 新增）=====
    if (app.editor && typeof app.editor.addToolbarButton === "function") {
      app.editor.addToolbarButton({
        id: "cc-tb-uppercase",
        icon: "Pencil",
        tooltip: "CC: 选区转大写",
        callback: (ctx) => {
          if (!ctx.selection) {
            app.notices.show("请先选中文字");
            return;
          }
          ctx.replaceSelection(ctx.selection.toUpperCase());
        },
      });
    }

    // ===== 3. 注册设置 Tab（1.1.0 新增）=====
    app.settings.registerTab({
      id: "cc-settings",
      title: "CC 插件设置",
      render: (container) => {
        const cleanup = app.settings.createForm(container, {
          fields: [
            {
              kind: "text",
              key: "apiKey",
              label: "API Key",
              placeholder: "输入你的 API Key",
              default: "",
            },
            {
              kind: "number",
              key: "maxResults",
              label: "最大结果数",
              default: 20,
              min: 1,
              max: 100,
            },
            {
              kind: "boolean",
              key: "autoSync",
              label: "自动同步",
              default: true,
            },
            {
              kind: "select",
              key: "theme",
              label: "主题",
              default: "auto",
              options: [
                { value: "auto", label: "跟随系统" },
                { value: "light", label: "浅色" },
                { value: "dark", label: "深色" },
              ],
            },
          ],
        });
        return cleanup;
      },
    });

    // ===== 4. 注册面板视图 =====
    app.panelViews.register({
      id: "cc-main-view",
      title: "CC 控制面板",
      render: (container) => {
        container.innerHTML = `
          <div style="padding: 16px; font-family: system-ui, sans-serif;">
            <h3>CC 示例插件 v2.0.0</h3>
            <p>这是一个演示 AppAPI 1.0.0 的示例插件</p>
            <button id="cc-btn-stat" style="padding: 8px 16px; cursor: pointer;">统计笔记</button>
            <pre id="cc-output" style="margin-top: 16px; background: #f0f0f0; padding: 8px; border-radius: 4px;"></pre>
          </div>
        `;
        const btn = container.querySelector("#cc-btn-stat");
        const out = container.querySelector("#cc-output");
        const handler = async () => {
          try {
            const notes = await app.notes.list();
            out.textContent = `笔记数量: ${notes.length}\n最近一篇: ${notes[0]?.title ?? "无"}`;
          } catch (e) {
            out.textContent = `错误: ${e}`;
          }
        };
        btn.addEventListener("click", handler);

        // 返回 cleanup
        return () => {
          btn.removeEventListener("click", handler);
        };
      },
    });

    // ===== 4. 读写插件自身设置 =====
    const lastRun = await app.settings.get("lastRun");
    console.log(`[cc] 上次激活: ${lastRun ?? "首次"}`);
    await app.settings.set("lastRun", new Date().toISOString());

    // ===== 5. 监听事件（存在性检查）=====
    if (app.events && typeof app.events.on === "function") {
      const off = app.events.on("workspace:active-note-changed", ({ noteId }) => {
        console.log(`[cc] 当前笔记切换到: ${noteId}`);
      });
      ctx._ccUnsubscribe = off;
    }

    console.log(`[cc] 激活完成`);
  },

  async onUnload(ctx) {
    console.log(`[cc] 停用中...`);
    if (ctx._ccUnsubscribe) ctx._ccUnsubscribe();
    // commands / sidebar / panelViews 由 PluginManager 自动按 pluginId 前缀清理
  },
};
