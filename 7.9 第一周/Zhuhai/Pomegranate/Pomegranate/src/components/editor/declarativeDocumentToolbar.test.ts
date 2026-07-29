import assert from "node:assert/strict";
import test from "node:test";
import {
  combineToolbarEntries,
  executeDeclarativeDocumentAction,
  subscribeDeclarativeToolbar,
} from "./declarativeDocumentToolbar.ts";
import type {
  EditorActionCtx,
  PluginDocumentToolbarButton,
  PluginEditorToolbarButtonDef,
} from "@/types";

const button: PluginDocumentToolbarButton = {
  pluginId: "official-ai-document-summary-plugin",
  pluginName: "AI 文档摘要插件",
  id: "ai-document-summary",
  label: "AI 摘要",
  tooltip: "生成摘要",
  icon: "Sparkles",
  action: "mock-document-summary",
};

function flushPromises() {
  return new Promise<void>((resolve) => setImmediate(resolve));
}

test("接口返回声明式摘要按钮时交付给渲染状态", async () => {
  const snapshots: PluginDocumentToolbarButton[][] = [];
  const dispose = subscribeDeclarativeToolbar({
    load: async () => [button],
    subscribe: () => () => undefined,
    onItems: (items) => snapshots.push(items),
    onError: (error) => assert.fail(String(error)),
  });

  await flushPromises();
  assert.deepEqual(snapshots, [[button]]);
  dispose();
});

test("接口返回空列表时保持无声明式按钮", async () => {
  const snapshots: PluginDocumentToolbarButton[][] = [];
  const dispose = subscribeDeclarativeToolbar({
    load: async () => [],
    subscribe: () => () => undefined,
    onItems: (items) => snapshots.push(items),
    onError: (error) => assert.fail(String(error)),
  });

  await flushPromises();
  assert.deepEqual(snapshots, [[]]);
  dispose();
});

test("legacy 与声明式按钮合并后同时保留", () => {
  const legacy: PluginEditorToolbarButtonDef & { pluginId: string } = {
    pluginId: "legacy-plugin",
    id: "legacy-action",
    tooltip: "Legacy action",
    icon: "Puzzle",
    callback: async () => undefined,
  };

  const combined = combineToolbarEntries([legacy], [button]);
  assert.deepEqual(
    combined.map((entry) => [entry.kind, entry.item.id]),
    [
      ["legacy", "legacy-action"],
      ["declarative", "ai-document-summary"],
    ],
  );
});

test("声明式工具栏刷新事件会重新查询", async () => {
  let eventHandler: (() => void) | undefined;
  let calls = 0;
  const snapshots: PluginDocumentToolbarButton[][] = [];
  const dispose = subscribeDeclarativeToolbar({
    load: async () => (++calls === 1 ? [] : [button]),
    subscribe: (handler) => {
      eventHandler = handler;
      return () => undefined;
    },
    onItems: (items) => snapshots.push(items),
    onError: (error) => assert.fail(String(error)),
  });

  await flushPromises();
  eventHandler?.();
  await flushPromises();
  assert.equal(calls, 2);
  assert.deepEqual(snapshots, [[], [button]]);
  dispose();
});

test("禁用或撤权后接口返回空列表会移除按钮", async () => {
  let eventHandler: (() => void) | undefined;
  let enabled = true;
  const snapshots: PluginDocumentToolbarButton[][] = [];
  const dispose = subscribeDeclarativeToolbar({
    load: async () => (enabled ? [button] : []),
    subscribe: (handler) => {
      eventHandler = handler;
      return () => undefined;
    },
    onItems: (items) => snapshots.push(items),
    onError: (error) => assert.fail(String(error)),
  });

  await flushPromises();
  enabled = false;
  eventHandler?.();
  await flushPromises();
  assert.deepEqual(snapshots, [[button], []]);
  dispose();
});

test("卸载组件会取消订阅并忽略未完成请求", async () => {
  let unsubscribed = false;
  let resolveLoad: ((items: PluginDocumentToolbarButton[]) => void) | undefined;
  const snapshots: PluginDocumentToolbarButton[][] = [];
  const dispose = subscribeDeclarativeToolbar({
    load: () =>
      new Promise((resolve) => {
        resolveLoad = resolve;
      }),
    subscribe: () => () => {
      unsubscribed = true;
    },
    onItems: (items) => snapshots.push(items),
    onError: (error) => assert.fail(String(error)),
  });

  dispose();
  resolveLoad?.([button]);
  await flushPromises();
  assert.equal(unsubscribed, true);
  assert.deepEqual(snapshots, []);
});

test("接口失败只进入诊断回调且不清空现有工具栏", async () => {
  const snapshots: PluginDocumentToolbarButton[][] = [];
  const errors: unknown[] = [];
  const dispose = subscribeDeclarativeToolbar({
    load: async () => {
      throw new Error("backend unavailable");
    },
    subscribe: () => () => undefined,
    onItems: (items) => snapshots.push(items),
    onError: (error) => errors.push(error),
  });

  await flushPromises();
  assert.deepEqual(snapshots, []);
  assert.equal(errors.length, 1);
  dispose();
});

test("mock 摘要只能经后端生成和写入授权后插入编辑器", async () => {
  const calls: string[] = [];
  const inserted: string[] = [];
  const context: EditorActionCtx = {
    noteId: 1,
    selection: "",
    replaceSelection: () => undefined,
    insertText: (text) => {
      calls.push("insert");
      inserted.push(text);
    },
    getContent: () => "正文",
  };

  await executeDeclarativeDocumentAction(button, "测试文档", context, {
    mockSummary: async (input) => {
      calls.push("mock");
      assert.equal(input.content, "正文");
      return {
        pluginId: button.pluginId,
        title: input.title,
        summary: "受控摘要",
        mock: true,
        providerLabel: "Mock Provider",
        wordCount: 2,
        generatedAt: "2026-07-29 00:00:00",
      };
    },
    authorizeInsert: async () => {
      calls.push("authorize");
    },
  });

  assert.deepEqual(calls, ["mock", "authorize", "insert"]);
  assert.match(inserted[0], /受控摘要/);

  await assert.rejects(
    executeDeclarativeDocumentAction(button, "测试文档", context, {
      mockSummary: async () => ({
        pluginId: button.pluginId,
        title: "测试文档",
        summary: "不可写入",
        mock: true,
        providerLabel: "Mock Provider",
        wordCount: 2,
        generatedAt: "2026-07-29 00:00:00",
      }),
      authorizeInsert: async () => {
        throw new Error("permission revoked");
      },
    }),
    /permission revoked/,
  );
  assert.equal(inserted.length, 1);
});
