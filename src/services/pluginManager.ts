/**
 * PluginManager — 插件运行时核心 v1.0.0
 *
 * 职责：
 * 1. 启动时从后端拉取所有已启用插件列表
 * 2. 读取每个插件的 main.js / styles.css 并执行
 * 3. 管理插件生命周期（activate / deactivate / reload）
 * 4. 单插件崩溃不影响其他插件和主应用
 *
 * 安全模型（R1 修订后）：
 * - 激活时通过 plugin_acquire_token 向 Rust 申领令牌
 * - 停用时通过 plugin_revoke_token 作废令牌
 * - 令牌闭包注入 createAppAPI，插件 JS 无法直接获取
 *
 * 注册表（R4 修订后）：
 * - 内部 Map 存储，key = "<pluginId>:<id>"
 * - 不再使用 window.__plugin* 全局键
 * - PluginManager extends EventTarget 发送 registry 变化事件
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { pluginApi } from "@/lib/api";
import { createAppAPI } from "@/services/pluginApi";
import type { Task } from "@/types";
import type {
  PluginInfo,
  PluginContext,
  PluginModule,
  PluginCommandDef,
  PluginSidebarItemDef,
  PluginPanelViewDef,
  PluginRibbonItemDef,
  PluginEditorContextMenuItemDef,
  PluginEditorToolbarButtonDef,
  PluginSettingsTabDef,
  TaskViewProps,
} from "@/types";

// ───────── 类型 ─────────

interface ActivePlugin {
  info: PluginInfo;
  module: PluginModule;
  /** 插件运行时令牌（Rust 侧签发） */
  token: string;
  /** 注入到 <head> 的样式标签 */
  styleEl: HTMLStyleElement | null;
}

type EventHandler = (...args: unknown[]) => void;

// ───────── 内部工具 ─────────

function executePluginJS(code: string, pluginId: string): PluginModule {
  const wrapped = `
return (function() {
  const module = { exports: {} };
  const exports = module.exports;
  try {
    ${code}
  } catch (e) {
    module.exports = { onLoad: function() { throw e; } };
  }
  return module.exports;
})();
`;

  try {
    // eslint-disable-next-line @typescript-eslint/no-implied-eval
    const fn = new Function(wrapped);
    const exports = fn() as PluginModule;
    if (typeof exports !== "object" || exports === null) {
      console.warn(`[PluginManager] 插件 ${pluginId}: main.js 未导出 module.exports 对象，跳过`);
      return {};
    }
    return exports;
  } catch (e) {
    console.error(`[PluginManager] 插件 ${pluginId} 脚本解析失败:`, e);
    return {
      onLoad: () => {
        throw new Error(`插件 ${pluginId} 脚本解析失败: ${e}`);
      },
    };
  }
}

function injectStyle(pluginId: string, css: string): HTMLStyleElement {
  const el = document.createElement("style");
  el.setAttribute("data-plugin-id", pluginId);
  el.textContent = css;
  document.head.appendChild(el);
  return el;
}

function removeStyle(pluginId: string) {
  const el = document.querySelector(`style[data-plugin-id="${pluginId}"]`);
  if (el) el.remove();
}

// ───────── PluginManager ─────────

/** 注册表变化事件类型 */
type RegistryEvent =
  | "commands" | "sidebar" | "views" | "ribbon"
  | "editor-menus" | "editor-toolbar" | "settings-tabs";

export class PluginManager {
  // ─── 注册表（key = "<pluginId>:<id>"）───
  private registry = {
    commands: new Map<string, PluginCommandDef & { pluginId: string }>(),
    sidebar: new Map<string, PluginSidebarItemDef & { pluginId: string }>(),
    panelViews: new Map<string, PluginPanelViewDef>(),
    taskViews: new Map<string, { id: string; label: string; icon: string; pluginId: string; render: (container: HTMLElement, props: TaskViewProps) => void | (() => void) }>(),
    ribbon: new Map<string, PluginRibbonItemDef & { pluginId: string }>(),
    editorMenus: new Map<
      string,
      PluginEditorContextMenuItemDef & { pluginId: string }
    >(),
    editorToolbar: new Map<
      string,
      PluginEditorToolbarButtonDef & { pluginId: string }
    >(),
    settingsTabs: new Map<string, PluginSettingsTabDef & { pluginId: string }>(),
  };

  // ─── 注册表订阅者（简单回调列表，不用 EventTarget）───
  // EventTarget 在 Vite HMR + Tauri WebView 下有时不可靠：旧实例的 listener
  // 注册到了同一个单例上但 React 没重渲染。改用纯回调数组，每次 emit 都遍历调用。
  private subscribers: Record<RegistryEvent, Set<() => void>> = {
    commands: new Set(),
    sidebar: new Set(),
    views: new Set(),
    ribbon: new Set(),
    "editor-menus": new Set(),
    "editor-toolbar": new Set(),
    "settings-tabs": new Set(),
  };

  /** 订阅注册表变化；返回取消订阅函数 */
  subscribe(event: RegistryEvent, handler: () => void): () => void {
    this.subscribers[event].add(handler);
    return () => {
      this.subscribers[event].delete(handler);
    };
  }

  private emit(event: RegistryEvent) {
    for (const fn of this.subscribers[event]) {
      try {
        fn();
      } catch (e) {
        console.error(`[PluginManager] subscriber for ${event} threw:`, e);
      }
    }
  }

  // ─── 错误日志（环形缓冲，最多 100 条）──────────
  private errorLog: Array<{
    pluginId: string;
    time: string;
    kind: string;
    message: string;
  }> = [];

  /** 写入一条插件运行时错误日志（供内部 try-catch 调用） */
  _logError(pluginId: string, kind: string, message: string) {
    this.errorLog.push({
      pluginId,
      time: new Date().toISOString(),
      kind,
      message,
    });
    if (this.errorLog.length > 100) {
      this.errorLog = this.errorLog.slice(-100);
    }
  }

  /** 获取错误日志副本（最近 100 条，按时间升序） */
  getErrorLog(): ReadonlyArray<{
    pluginId: string;
    time: string;
    kind: string;
    message: string;
  }> {
    return [...this.errorLog];
  }

  // ─── 事件监听器订阅 ───
  private eventSubscriptions = new Map<string, Map<string, Set<EventHandler>>>();

  /** 当前已激活的插件 */
  private active = new Map<string, ActivePlugin>();

  /** 初始化是否完成 */
  private initialized = false;

  /** Tauri 事件监听器清理句柄 */
  private tauriUnlistens: UnlistenFn[] = [];

  /**
   * 搭建 Tauri 事件 → 插件事件桥接。
   * Rust 侧 emit `plugin:task:*`，桥接后转发为 `task:*` 到注册的插件处理器。
   */
  private async setupEventBridge(): Promise<void> {
    const bridge = (tauriEvent: string, pluginEvent: string) => {
      listen<Task>(tauriEvent, (event) => {
        // 遍历所有插件订阅，派发给匹配的 handler
        for (const [, pluginEvents] of this.eventSubscriptions) {
          const handlers = pluginEvents.get(pluginEvent);
          if (handlers) {
            for (const fn of handlers) {
              try {
                fn(event.payload);
              } catch (e) {
                console.error(`[PluginManager] 事件 ${pluginEvent} 派发异常:`, e);
              }
            }
          }
        }
      }).then((unlisten) => {
        this.tauriUnlistens.push(unlisten);
      });
    };

    await bridge("plugin:task:created", "task:created");
    await bridge("plugin:task:updated", "task:updated");
    await bridge("plugin:task:completed", "task:completed");
    await bridge("plugin:task:deleted", "task:deleted");
    await bridge("plugin:task:reminded", "task:reminded");
    console.log("[PluginManager] 任务事件桥接已就绪");
  }

  /** 启动时初始化：加载所有已启用插件 */
  async init(): Promise<void> {
    if (this.initialized) {
      console.warn("[PluginManager] 已初始化，跳过重复 init");
      return;
    }

    console.log("[PluginManager] 开始初始化插件运行时...");

    try {
      await pluginApi.scan();
    } catch (e) {
      console.warn("[PluginManager] 扫描插件目录失败（非致命）:", e);
    }

    let plugins: PluginInfo[];
    try {
      plugins = await pluginApi.list();
    } catch (e) {
      console.error("[PluginManager] 获取插件列表失败:", e);
      return;
    }

    const enabled = plugins.filter((p) => p.enabled && p.status === "installed");
    console.log(`[PluginManager] 发现 ${enabled.length} 个已启用插件`);

    for (const plugin of enabled) {
      try {
        await this.activatePlugin(plugin);
      } catch (e) {
        console.error(`[PluginManager] 插件 ${plugin.id} 启动失败:`, e);
      }
    }

    // 搭建任务事件桥接（Tauri emit → 插件 handler）
    await this.setupEventBridge();

    this.initialized = true;
    console.log(`[PluginManager] 初始化完成，${this.active.size} 个插件已激活`);
  }

  /** 激活单个插件 */
  async activatePlugin(info: PluginInfo): Promise<void> {
    if (this.active.has(info.id)) {
      console.warn(`[PluginManager] 插件 ${info.id} 已激活，先停用再重新激活`);
      await this.deactivatePlugin(info.id);
    }

    console.log(`[PluginManager] 激活插件: ${info.id} v${info.version}`);

    // T4: 1. 向 Rust 申领令牌
    let token: string;
    try {
      token = await invoke<string>("plugin_acquire_token", { pluginId: info.id });
    } catch (e) {
      throw new Error(`申领令牌失败: ${e}`);
    }

    // 2. 读取 main.js
    let mainJS: string;
    try {
      mainJS = await pluginApi.readAsset(info.id, info.main);
    } catch (e) {
      // 令牌回滚
      await invoke("plugin_revoke_token", { pluginId: info.id }).catch(() => {});
      throw new Error(`读取 main.js (${info.main}) 失败: ${e}`);
    }

    // 3. 执行 JS 代码
    const pluginModule = executePluginJS(mainJS, info.id);

    // 4. 构造 PluginContext（无 register* 顶层方法）
    const appAPI = createAppAPI(info.id, token, this);

    const ctx: PluginContext = {
      app: appAPI,
      meta: Object.freeze({
        id: info.id,
        name: info.name,
        version: info.version,
        dir: info.path,
      }),
    };

    // 5. 调用 onLoad
    if (typeof pluginModule.onLoad === "function") {
      try {
        await pluginModule.onLoad(ctx);
      } catch (e) {
        // 令牌回滚 + 注册表回滚：onLoad 可能已注册了 sidebar/views/commands
        await invoke("plugin_revoke_token", { pluginId: info.id }).catch(() => {});
        this.cleanupPluginRegistry(info.id);
        throw new Error(`onLoad() 执行失败: ${e}`);
      }
    }

    // 6. 注入 CSS
    let styleEl: HTMLStyleElement | null = null;
    if (info.styles) {
      try {
        const css = await pluginApi.readAsset(info.id, info.styles);
        styleEl = injectStyle(info.id, css);
        console.log(`[PluginManager] 插件 ${info.id} 样式已注入`);
      } catch (e) {
        console.warn(`[PluginManager] 插件 ${info.id} 读取样式失败（非致命）:`, e);
      }
    }

    // 7. 记录激活状态
    this.active.set(info.id, {
      info,
      module: pluginModule,
      token,
      styleEl,
    });

    console.log(`[PluginManager] 插件 ${info.id} 激活成功`);
  }

  /** 停用单个插件
   *
   * 关键顺序：先同步清理注册表 + 派发事件（UI 立即刷新），再异步走 onUnload / 令牌作废。
   * 这样即便 onUnload / Rust 端令牌作废抛错或卡住，左侧侧边栏图标也已经消失，
   * 用户不会看到"已停用但图标还在"的脏状态。
   *
   * 同时无论 active 表里是否有记录都强制清理，覆盖 activatePlugin 半失败留下的
   * "幽灵注册"（onLoad 抛错前已经 addItem，但 this.active.set 未执行）。
   */
  async deactivatePlugin(pluginId: string): Promise<void> {
    const active = this.active.get(pluginId);
    console.log(`[PluginManager] 停用插件: ${pluginId} (active=${!!active})`);

    // 1. ★ 优先同步清理注册表 + 派发事件 —— UI 立即刷新，不依赖后续 await
    this.cleanupPluginRegistry(pluginId);
    removeStyle(pluginId);
    this.active.delete(pluginId);

    // 2. 异步调用 onUnload（仅 active 有记录时；用 active 快照避免上面 delete 后丢失）
    if (active && typeof active.module.onUnload === "function") {
      const appAPI = createAppAPI(pluginId, active.token, this);
      const ctx: PluginContext = {
        app: appAPI,
        meta: Object.freeze({
          id: active.info.id,
          name: active.info.name,
          version: active.info.version,
          dir: active.info.path,
        }),
      };
      try {
        await active.module.onUnload(ctx);
      } catch (e) {
        console.warn(`[PluginManager] 插件 ${pluginId} onUnload 失败（忽略）:`, e);
      }
    }

    // 3. 异步作废令牌（即使失败也不阻塞 UI；Rust 侧无令牌引用最终会过期）
    await invoke("plugin_revoke_token", { pluginId }).catch((e) => {
      console.warn(`[PluginManager] 作废令牌失败（忽略）:`, e);
    });

    console.log(`[PluginManager] 插件 ${pluginId} 已停用`);
  }

  /** 重载插件 */
  async reloadPlugin(pluginId: string): Promise<void> {
    const active = this.active.get(pluginId);
    if (!active) {
      console.warn(`[PluginManager] 插件 ${pluginId} 未激活，无法重载`);
      return;
    }
    await this.deactivatePlugin(pluginId);
    try {
      const updated = await pluginApi.list();
      const info = updated.find((p) => p.id === pluginId);
      if (info && info.enabled) {
        await this.activatePlugin(info);
      } else {
        console.warn(`[PluginManager] 插件 ${pluginId} 已卸载或被禁用，取消重载`);
      }
    } catch (e) {
      console.error(`[PluginManager] 重载插件 ${pluginId} 失败:`, e);
    }
  }

  /** 停用所有已激活插件 */
  async deactivateAll(): Promise<void> {
    const ids = Array.from(this.active.keys());
    for (const id of ids) {
      try {
        await this.deactivatePlugin(id);
      } catch (e) {
        console.error(`[PluginManager] 停用插件 ${id} 失败:`, e);
      }
    }
  }

  // ═══════════════════════════════════════════════════
  // 公共查询方法
  // ═══════════════════════════════════════════════════

  getActiveIds(): string[] {
    return Array.from(this.active.keys());
  }

  isActive(pluginId: string): boolean {
    return this.active.has(pluginId);
  }

  getRegisteredCommands(): Array<PluginCommandDef & { pluginId: string }> {
    return Array.from(this.registry.commands.values());
  }

  getRegisteredSidebarItems(): Array<PluginSidebarItemDef & { pluginId: string }> {
    return Array.from(this.registry.sidebar.values());
  }

  getRegisteredPanelViews(): PluginPanelViewDef[] {
    return Array.from(this.registry.panelViews.values());
  }

  getRegisteredRibbonItems(): Array<PluginRibbonItemDef & { pluginId: string }> {
    return Array.from(this.registry.ribbon.values());
  }

  getRegisteredEditorMenuItems(): Array<
    PluginEditorContextMenuItemDef & { pluginId: string }
  > {
    return Array.from(this.registry.editorMenus.values());
  }

  /** 按 pluginId 查激活态插件的展示名（用于 tooltip 等场景） */
  getPluginName(pluginId: string): string | null {
    return this.active.get(pluginId)?.info.name ?? null;
  }

  getPanelView(viewId: string): PluginPanelViewDef | null {
    // viewId 可能是裸 id（如 "cc-main-view"）或 "<pluginId>:<id>" 全 key。
    // 内部 Map 用全 key 存储，所以先尝试全 key，再按裸 id 在 values 里查找。
    const direct = this.registry.panelViews.get(viewId);
    if (direct) return direct;
    for (const def of this.registry.panelViews.values()) {
      if (def.id === viewId) return def;
    }
    return null;
  }

  // ═══════════════════════════════════════════════════
  // 内部方法（给 createAppAPI 的子 API 调用）
  // ═══════════════════════════════════════════════════

  _addCommand(pluginId: string, def: PluginCommandDef): () => void {
    const key = `${pluginId}:${def.id}`;
    this.registry.commands.set(key, { ...def, pluginId });
    this.emit("commands");
    return () => {
      this.registry.commands.delete(key);
      this.emit("commands");
    };
  }

  _removeCommand(pluginId: string, id: string) {
    this.registry.commands.delete(`${pluginId}:${id}`);
    this.emit("commands");
  }

  _executeCommand(fullId: string): void | Promise<void> {
    const cmd = this.registry.commands.get(fullId);
    if (!cmd) throw new Error(`命令 ${fullId} 未注册`);
    return cmd.callback();
  }

  _addSidebarItem(pluginId: string, def: PluginSidebarItemDef) {
    const key = `${pluginId}:${def.id}`;
    this.registry.sidebar.set(key, { ...def, pluginId });
    this.emit("sidebar");
  }

  _removeSidebarItem(pluginId: string, id: string) {
    this.registry.sidebar.delete(`${pluginId}:${id}`);
    this.emit("sidebar");
  }

  _registerPanelView(pluginId: string, def: PluginPanelViewDef) {
    const key = `${pluginId}:${def.id}`;
    this.registry.panelViews.set(key, { ...def, pluginId });
    this.emit("views");
  }

  _unregisterPanelView(pluginId: string, id: string) {
    this.registry.panelViews.delete(`${pluginId}:${id}`);
    this.emit("views");
  }

  /** 注册插件任务视图（单插件最多 5 个） */
  _registerTaskView(
    pluginId: string,
    def: { id: string; label: string; icon: string; render: (container: HTMLElement, props: TaskViewProps) => void | (() => void) },
  ): () => void {
    const key = `${pluginId}:${def.id}`;
    const count = Array.from(this.registry.taskViews.keys()).filter((k) =>
      k.startsWith(`${pluginId}:`),
    ).length;
    if (count >= 5) {
      console.warn(`[PluginManager] 插件 ${pluginId} 注册视图超过 5 个上限`);
      return () => {};
    }
    this.registry.taskViews.set(key, { ...def, pluginId });
    this.emit("views");
    return () => {
      this.registry.taskViews.delete(key);
      this.emit("views");
    };
  }

  _unregisterTaskView(pluginId: string, id: string) {
    this.registry.taskViews.delete(`${pluginId}:${id}`);
    this.emit("views");
  }

  getAllTaskViews(): Array<{
    id: string;
    label: string;
    icon: string;
    pluginId: string;
    render: (container: HTMLElement, props: TaskViewProps) => void | (() => void);
  }> {
    return Array.from(this.registry.taskViews.values());
  }

  _addRibbonItem(pluginId: string, def: PluginRibbonItemDef) {
    const key = `${pluginId}:${def.id}`;
    this.registry.ribbon.set(key, { ...def, pluginId });
    this.emit("ribbon");
  }

  _removeRibbonItem(pluginId: string, id: string) {
    this.registry.ribbon.delete(`${pluginId}:${id}`);
    this.emit("ribbon");
  }

  _addEditorMenuItem(pluginId: string, def: PluginEditorContextMenuItemDef) {
    const key = `${pluginId}:${def.id}`;
    this.registry.editorMenus.set(key, { ...def, pluginId });
    this.emit("editor-menus");
  }

  _removeEditorMenuItem(pluginId: string, id: string) {
    this.registry.editorMenus.delete(`${pluginId}:${id}`);
    this.emit("editor-menus");
  }

  _addEditorToolbarButton(
    pluginId: string,
    def: PluginEditorToolbarButtonDef,
  ) {
    const key = `${pluginId}:${def.id}`;
    this.registry.editorToolbar.set(key, { ...def, pluginId });
    this.emit("editor-toolbar");
  }

  _removeEditorToolbarButton(pluginId: string, id: string) {
    this.registry.editorToolbar.delete(`${pluginId}:${id}`);
    this.emit("editor-toolbar");
  }

  getRegisteredEditorToolbarButtons(): Array<
    PluginEditorToolbarButtonDef & { pluginId: string }
  > {
    return Array.from(this.registry.editorToolbar.values());
  }

  _registerSettingsTab(pluginId: string, def: PluginSettingsTabDef) {
    const key = `${pluginId}:${def.id}`;
    this.registry.settingsTabs.set(key, { ...def, pluginId });
    this.emit("settings-tabs");
  }

  _unregisterSettingsTab(pluginId: string, id: string) {
    this.registry.settingsTabs.delete(`${pluginId}:${id}`);
    this.emit("settings-tabs");
  }

  getRegisteredSettingsTabs(): Array<
    PluginSettingsTabDef & { pluginId: string }
  > {
    return Array.from(this.registry.settingsTabs.values());
  }

  _addEventListener<K extends string>(
    pluginId: string,
    event: K,
    handler: (data: unknown) => void,
  ): () => void {
    if (!this.eventSubscriptions.has(pluginId)) {
      this.eventSubscriptions.set(pluginId, new Map());
    }
    const pluginEvents = this.eventSubscriptions.get(pluginId)!;
    if (!pluginEvents.has(event)) {
      pluginEvents.set(event, new Set());
    }
    pluginEvents.get(event)!.add(handler as EventHandler);
    return () => {
      pluginEvents.get(event)?.delete(handler as EventHandler);
    };
  }

  // ═══════════════════════════════════════════════════
  // 私有方法
  // ═══════════════════════════════════════════════════

  /** 按插件前缀批量清理注册表 */
  private cleanupPluginRegistry(pluginId: string) {
    const prefix = `${pluginId}:`;
    for (const key of [...this.registry.commands.keys()]) {
      if (key.startsWith(prefix)) this.registry.commands.delete(key);
    }
    for (const key of [...this.registry.sidebar.keys()]) {
      if (key.startsWith(prefix)) this.registry.sidebar.delete(key);
    }
    for (const key of [...this.registry.panelViews.keys()]) {
      if (key.startsWith(prefix)) this.registry.panelViews.delete(key);
    }
    for (const key of [...this.registry.taskViews.keys()]) {
      if (key.startsWith(prefix)) this.registry.taskViews.delete(key);
    }
    for (const key of [...this.registry.ribbon.keys()]) {
      if (key.startsWith(prefix)) this.registry.ribbon.delete(key);
    }
    for (const key of [...this.registry.editorMenus.keys()]) {
      if (key.startsWith(prefix)) this.registry.editorMenus.delete(key);
    }
    for (const key of [...this.registry.editorToolbar.keys()]) {
      if (key.startsWith(prefix)) this.registry.editorToolbar.delete(key);
    }
    for (const key of [...this.registry.settingsTabs.keys()]) {
      if (key.startsWith(prefix)) this.registry.settingsTabs.delete(key);
    }
    // 清理事件订阅
    this.eventSubscriptions.delete(pluginId);
    // 清理错误日志
    this.errorLog = this.errorLog.filter((e) => e.pluginId !== pluginId);

    this.emit("commands");
    this.emit("sidebar");
    this.emit("views");
    this.emit("ribbon");
    this.emit("editor-menus");
    this.emit("editor-toolbar");
    this.emit("settings-tabs");
  }
}

/** 全局单例 */
export const pluginManager = new PluginManager();
