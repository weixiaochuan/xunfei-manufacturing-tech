import type {
  EditorActionCtx,
  PluginDocumentSummaryResult,
  PluginDocumentToolbarButton,
  PluginEditorToolbarButtonDef,
} from "@/types";

export type LegacyToolbarEntry = PluginEditorToolbarButtonDef & {
  pluginId: string;
};

export type CombinedToolbarEntry =
  | { kind: "legacy"; item: LegacyToolbarEntry }
  | { kind: "declarative"; item: PluginDocumentToolbarButton };

export function combineToolbarEntries(
  legacyItems: LegacyToolbarEntry[],
  declarativeItems: PluginDocumentToolbarButton[],
): CombinedToolbarEntry[] {
  return [
    ...legacyItems.map((item) => ({ kind: "legacy" as const, item })),
    ...declarativeItems.map((item) => ({
      kind: "declarative" as const,
      item,
    })),
  ];
}

interface DeclarativeToolbarSubscriptionOptions {
  load: () => Promise<PluginDocumentToolbarButton[]>;
  subscribe: (handler: () => void) => () => void;
  onItems: (items: PluginDocumentToolbarButton[]) => void;
  onError: (error: unknown) => void;
}

/**
 * 加载后端权威按钮列表，并用递增请求号丢弃过期响应。
 * 返回的清理函数同时取消事件订阅，并阻止卸载后的状态回写。
 */
export function subscribeDeclarativeToolbar({
  load,
  subscribe,
  onItems,
  onError,
}: DeclarativeToolbarSubscriptionOptions): () => void {
  let active = true;
  let requestId = 0;

  const refresh = async () => {
    const currentRequestId = ++requestId;
    try {
      const items = await load();
      if (active && currentRequestId === requestId) {
        onItems(items);
      }
    } catch (error) {
      if (active && currentRequestId === requestId) {
        onError(error);
      }
    }
  };

  const unsubscribe = subscribe(() => {
    void refresh();
  });
  void refresh();

  return () => {
    active = false;
    requestId += 1;
    unsubscribe();
  };
}

interface ExecuteMockSummaryDependencies {
  mockSummary: (input: {
    pluginId: string;
    title: string;
    content: string;
  }) => Promise<PluginDocumentSummaryResult>;
  authorizeInsert: (input: {
    pluginId: string;
    title: string;
  }) => Promise<void>;
}

export async function executeDeclarativeDocumentAction(
  button: PluginDocumentToolbarButton,
  title: string,
  context: EditorActionCtx,
  dependencies: ExecuteMockSummaryDependencies,
): Promise<PluginDocumentSummaryResult> {
  if (button.action !== "mock-document-summary") {
    throw new Error(`不支持的声明式编辑器动作：${String(button.action)}`);
  }

  const summary = await dependencies.mockSummary({
    pluginId: button.pluginId,
    title,
    content: context.getContent(),
  });

  // 写入前再次经过 Rust 权限与启用状态校验，避免状态变化后绕过后端。
  await dependencies.authorizeInsert({
    pluginId: button.pluginId,
    title: summary.title,
  });
  context.insertText(`\n\n## AI 摘要\n\n${summary.summary}\n`);
  return summary;
}
