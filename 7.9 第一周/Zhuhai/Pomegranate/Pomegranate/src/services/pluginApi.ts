/**
 * 插件 AppAPI 工厂 v1.0.0
 *
 * 安全模型（R1 修订后）：
 * - 所有能力调用通过 plugin_proxy_* Rust Command，Rust 侧做令牌+权限校验
 * - 前端 hasPermission() 降级为 UX 前置提示（仅 console.warn，非安全边界）
 * - token 闭包捕获，插件 JS 无法直接拿到或伪造
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { message } from "antd";
import {
  PLUGIN_API_VERSION,
} from "@/types";
import type {
  PluginAppAPI,
  PluginNotesAPI,
  PluginTasksAPI,
  PluginTaskViewsAPI,
  PluginSettingsAPI,
  PluginCommandsAPI,
  PluginWorkspaceAPI,
  PluginSidebarAPI,
  PluginPanelViewsAPI,
  PluginEventBus,
  PluginRibbonAPI,
  PluginEditorAPI,
  PluginAiAPI,
  PluginAiTokenPayload,
  PluginInfo,
  PluginTaskView,
} from "@/types";
import { PluginManager } from "@/services/pluginManager";
import { getActiveSelectionText } from "@/services/editorBridge";
import { createSettingsForm } from "@/components/plugin/SettingsFormRenderer";

/** 错误转译表 */
function translateError(raw: string): string {
  if (raw.includes("插件权限拒绝") && raw.includes("plugin=None")) {
    return "插件会话已过期，请重新启用插件";
  }
  if (raw.includes("插件权限拒绝")) {
    return raw;
  }
  if (raw.includes("不存在")) return "插件已卸载，请刷新页面";
  if (raw.includes("已禁用")) return "插件已禁用";
  if (raw.includes("状态非 installed")) return "插件状态异常，请稍后重试";
  return raw;
}

/** UX 权限前置检查（仅 console.warn，非安全边界） */
function preCheck(pluginId: string, perm: string): void {
  console.warn(`[plugin:${pluginId}] 调用需要权限 ${perm}，请确认已授权`);
}

/**
 * 为指定插件创建受限的 AppAPI 实例。
 *
 * @param pluginId  插件 ID
 * @param token     插件运行时令牌（由 Rust 侧 plugin_acquire_token 签发）
 * @param manager   PluginManager 实例（用于 commands/sidebar/panelViews 注册）
 */
export function createAppAPI(
  info: PluginInfo,
  token: string,
  manager: PluginManager,
): PluginAppAPI {
  const pluginId = info.id;
  // ─── notes 子 API ───────────────────────────
  const notes: PluginNotesAPI = {
    list: async (query) => {
      preCheck(pluginId, "notes:read");
      try {
        return await invoke("plugin_proxy_notes_list", { token, query: query ?? {} });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    get: async (id) => {
      preCheck(pluginId, "notes:read");
      try {
        return await invoke("plugin_proxy_notes_get", { token, id });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    search: async (keyword, limit) => {
      preCheck(pluginId, "notes:read");
      try {
        return await invoke("plugin_proxy_notes_search", { token, keyword, limit });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    create: async (input) => {
      preCheck(pluginId, "notes:write");
      try {
        return await invoke("plugin_proxy_notes_create", { token, input });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    update: async (id, input) => {
      preCheck(pluginId, "notes:write");
      try {
        return await invoke("plugin_proxy_notes_update", { token, id, input });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    delete: async (id) => {
      preCheck(pluginId, "notes:write");
      try {
        return await invoke("plugin_proxy_notes_delete", { token, id });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
  };

  // ─── settings 子 API ────────────────────────
  const settings: PluginSettingsAPI = {
    get: async (key) => {
      try {
        return (await invoke("plugin_proxy_settings_get", { token, key })) ?? undefined;
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    set: async (key, value) => {
      try {
        await invoke("plugin_proxy_settings_set", { token, key, value });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    getAll: async () => {
      try {
        return await invoke("plugin_proxy_settings_get_all", { token });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    registerTab: (def) => manager._registerSettingsTab(pluginId, def),
    unregisterTab: (id) => manager._unregisterSettingsTab(pluginId, id),
    createForm: (container, schema) =>
      createSettingsForm(container, schema, {
        get: (key) => settings.get(key),
        set: (key, value) => settings.set(key, value),
      }),
  };

  // ─── commands 子 API ────────────────────────
  const commands: PluginCommandsAPI = {
    addCommand: (def) => manager._addCommand(pluginId, def),
    removeCommand: (id) => manager._removeCommand(pluginId, id),
    executeCommand: (fullId) => manager._executeCommand(fullId),
  };

  // ─── sidebar 子 API ─────────────────────────
  const sidebar: PluginSidebarAPI = {
    addItem: (def) => manager._addSidebarItem(pluginId, def),
    removeItem: (id) => manager._removeSidebarItem(pluginId, id),
  };

  // ─── panelViews 子 API ──────────────────────
  const panelViews: PluginPanelViewsAPI = {
    register: (def) => manager._registerPanelView(pluginId, def),
    unregister: (id) => manager._unregisterPanelView(pluginId, id),
  };

  // ─── ribbon 子 API ──────────────────────────
  const ribbon: PluginRibbonAPI = {
    addItem: (def) => manager._addRibbonItem(pluginId, def),
    removeItem: (id) => manager._removeRibbonItem(pluginId, id),
  };

  // ─── editor 子 API ──────────────────────────
  const editor: PluginEditorAPI = {
    addContextMenuItem: (def) => manager._addEditorMenuItem(pluginId, def),
    removeContextMenuItem: (id) => manager._removeEditorMenuItem(pluginId, id),
    addToolbarButton: (def) => manager._addEditorToolbarButton(pluginId, def),
    removeToolbarButton: (id) => manager._removeEditorToolbarButton(pluginId, id),
    getCurrentSelection: () => getActiveSelectionText(),
  };

  // ─── workspace 子 API ───────────────────────
  const workspace: PluginWorkspaceAPI = {
    getActiveNoteId: () => {
      try {
        const match = window.location.hash.match(/\/notes\/(\d+)/);
        return match ? Number(match[1]) : null;
      } catch {
        return null;
      }
    },
    getActiveNote: async () => {
      preCheck(pluginId, "notes:read");
      const id = (() => {
        try {
          const m = window.location.hash.match(/\/notes\/(\d+)/);
          return m ? Number(m[1]) : null;
        } catch {
          return null;
        }
      })();
      if (id == null) return null;
      try {
        return await invoke("plugin_proxy_notes_get", { token, id });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
  };

  // ─── tasks 子 API（阶段 2：待办插件化）─────────
  const tasks: PluginTasksAPI = {
    list: async (filter?) => {
      preCheck(pluginId, "tasks.read");
      try {
        return await invoke<PluginTaskView[]>("plugin_proxy_tasks_list", {
          token,
          filter: filter ?? null,
        });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    get: async (id) => {
      preCheck(pluginId, "tasks.read");
      try {
        return await invoke<PluginTaskView | null>("plugin_proxy_tasks_get", {
          token,
          id,
        });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    create: async (input) => {
      preCheck(pluginId, "tasks.write");
      try {
        return await invoke<PluginTaskView>("plugin_proxy_tasks_create", {
          token,
          input,
        });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    update: async (id, patch) => {
      preCheck(pluginId, "tasks.write");
      try {
        await invoke("plugin_proxy_tasks_update", { token, id, input: patch });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    complete: async (id) => {
      preCheck(pluginId, "tasks.write");
      try {
        await invoke("plugin_proxy_tasks_complete", { token, id });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    delete: async (id) => {
      preCheck(pluginId, "tasks.write");
      try {
        await invoke("plugin_proxy_tasks_delete", { token, id });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
  };

  // ─── taskViews 子 API（阶段 3：自定义视图）─────
  const taskViews: PluginTaskViewsAPI = {
    register: (def) => {
      preCheck(pluginId, "taskViews.register");
      return manager._registerTaskView(pluginId, def);
    },
  };

  // ─── notices 子 API ─────────────────────────
  const notices = {
    show: (msg: string, duration?: number) => {
      message.success(msg, duration ?? 3);
    },
    error: (msg: string) => {
      message.error(msg);
    },
  };

  // ─── events 子 API ──────────────────────────
  const events: PluginEventBus = {
    on: <K extends keyof import("@/types").PluginEvents>(
      event: K,
      handler: (data: import("@/types").PluginEvents[K]) => void,
    ): (() => void) => {
      // 任务类事件需要 tasks.subscribe 权限
      if (String(event).startsWith("task:")) {
        preCheck(pluginId, "tasks.subscribe");
      }
      const off = manager._addEventListener(pluginId, event, handler as (data: unknown) => void);
      return off;
    },
    emit: <K extends keyof import("@/types").PluginEvents>(
      _event: K,
      _data: import("@/types").PluginEvents[K],
    ): void => {
      // emit 保留但暂不实现（阶段 2 末对接）
      console.warn("[pluginApi] events.emit 暂未实现");
    },
  };

  // ─── ai 子 API（Phase 2：受控插件 AI 能力）────────────────
  const ai: PluginAiAPI = {
    chat: async (messages, callbacks, options) => {
      preCheck(pluginId, "ai:chat");
      const requestId = typeof crypto !== "undefined" && "randomUUID" in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
      const eventName = `plugin:ai-token-${token}:${requestId}`;
      let fullText = "";
      let finished = false;
      let unlisten: UnlistenFn | null = null;
      const cleanup = () => {
        if (unlisten) {
          unlisten();
          unlisten = null;
        }
      };
      unlisten = await listen<PluginAiTokenPayload>(eventName, (event) => {
        const payload = event.payload;
        if (payload.error) {
          finished = true;
          cleanup();
          callbacks.onError?.(payload.error);
          return;
        }
        if (payload.done) {
          finished = true;
          cleanup();
          callbacks.onDone?.(payload.fullText ?? fullText);
          return;
        }
        if (payload.token) {
          fullText += payload.token;
          callbacks.onToken?.(payload.token, fullText);
        }
      });
      invoke("plugin_proxy_ai_chat", {
        token,
        input: {
          messages,
          requestId,
          modelId: options?.modelId,
          conversationId: options?.conversationId,
        },
      })
        .then(() => {
          if (!finished) {
            finished = true;
            cleanup();
            callbacks.onDone?.(fullText);
          }
        })
        .catch((e) => {
          finished = true;
          cleanup();
          callbacks.onError?.(translateError(String(e)));
        });
      return () => {
        invoke("plugin_proxy_ai_cancel", { token, requestId }).catch(() => {});
        cleanup();
      };
    },
    chatSync: async (messages, options) => {
      preCheck(pluginId, "ai:chat");
      try {
        return await invoke<string>("plugin_proxy_ai_chat_sync", {
          token,
          input: {
            messages,
            requestId: `sync-${Date.now()}-${Math.random().toString(16).slice(2)}`,
            modelId: options?.modelId,
            conversationId: options?.conversationId,
          },
        });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
    listModels: async () => {
      preCheck(pluginId, "ai:models");
      try {
        return await invoke("plugin_proxy_ai_models", { token });
      } catch (e) {
        throw new Error(translateError(String(e)));
      }
    },
  };

  // ─── invoke 桥接（Phase 1：直接透传，供插件调用现有 Rust Command）───
  const rawInvoke = async <T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    try {
      return await invoke<T>(cmd, args);
    } catch (e) {
      throw new Error(translateError(String(e)));
    }
  };

  // ─── Tauri 事件桥接（Phase 1：供插件监听全局事件如 ai:token）───
  const rawOnTauriEvent = async <T = unknown>(
    event: string,
    handler: (payload: T) => void,
  ): Promise<() => void> => {
    const unlisten: UnlistenFn = await listen<T>(event, (e) => {
      try {
        handler(e.payload);
      } catch (err) {
        console.error(`[plugin:${pluginId}] 事件 ${event} 处理异常:`, err);
      }
    });
    return () => { unlisten(); };
  };

  const api: PluginAppAPI = {
    version: PLUGIN_API_VERSION,
    workspace,
    notes,
    tasks,
    taskViews,
    settings,
    notices,
    commands,
    sidebar,
    panelViews,
    events,
    ribbon,
    editor,
    ai,
  };

  if (info.rawInvokeAllowed && info.canExecute && info.runtimeKind === "legacy-js") {
    api.invoke = rawInvoke;
    api.onTauriEvent = rawOnTauriEvent;
  }

  return api;
}
