/**
 * 编辑器工具栏插件按钮组。
 *
 * legacy JavaScript 插件继续读取 PluginManager 注册表；v2 声明式插件只读取
 * Rust 权威接口返回的按钮。两类按钮在同一工具栏并存，互不覆盖。
 */

import { useEffect, useMemo, useState } from "react";
import { Button, Tooltip, message } from "antd";
import type { Editor } from "@tiptap/react";
import { resolvePluginIconComponent } from "@/components/plugin/pluginIcons";
import { noteApi, pluginApi } from "@/lib/api";
import { getActiveEditor } from "@/services/editorBridge";
import { subscribeDeclarativePluginToolbarChanged } from "@/services/declarativePluginEvents";
import { pluginManager } from "@/services/pluginManager";
import type {
  EditorActionCtx,
  PluginDocumentToolbarButton,
  PluginEditorToolbarButtonDef,
} from "@/types";
import {
  combineToolbarEntries,
  executeDeclarativeDocumentAction,
  subscribeDeclarativeToolbar,
} from "./declarativeDocumentToolbar";

type LegacyToolbarEntry = PluginEditorToolbarButtonDef & { pluginId: string };

interface Props {
  editor: Editor | null;
  noteId?: number | null;
}

export function PluginToolbarButtons({ editor, noteId }: Props) {
  const [legacyItems, setLegacyItems] = useState<LegacyToolbarEntry[]>([]);
  const [declarativeItems, setDeclarativeItems] = useState<
    PluginDocumentToolbarButton[]
  >([]);
  const [runningAction, setRunningAction] = useState<string | null>(null);

  useEffect(() => {
    const refresh = () =>
      setLegacyItems(pluginManager.getRegisteredEditorToolbarButtons());
    refresh();
    return pluginManager.subscribe("editor-toolbar", refresh);
  }, []);

  useEffect(
    () =>
      subscribeDeclarativeToolbar({
        load: () => pluginApi.listDocumentSummaryToolbarButtons(),
        subscribe: subscribeDeclarativePluginToolbarChanged,
        onItems: setDeclarativeItems,
        onError: (error) => {
          // 加载失败不能影响内置或 legacy 工具栏，只保留低干扰诊断。
          console.warn("[PluginToolbar] 加载声明式工具栏按钮失败", error);
        },
      }),
    [],
  );

  const items = useMemo(
    () => combineToolbarEntries(legacyItems, declarativeItems),
    [legacyItems, declarativeItems],
  );

  const createActionContext = (): EditorActionCtx | null => {
    const activeEditor = editor ?? getActiveEditor();
    if (!activeEditor) {
      message.warning("编辑器尚未就绪");
      return null;
    }

    const selection = activeEditor.state.selection;
    const selectionText = selection.empty
      ? ""
      : activeEditor.state.doc.textBetween(
          selection.from,
          selection.to,
          "\n",
        );
    return {
      noteId: noteId ?? null,
      selection: selectionText,
      replaceSelection: (text: string) => {
        const currentSelection = activeEditor.state.selection;
        if (currentSelection.empty) {
          activeEditor.chain().focus().insertContent(text).run();
        } else {
          activeEditor
            .chain()
            .focus()
            .deleteSelection()
            .insertContent(text)
            .run();
        }
      },
      insertText: (text: string) => {
        activeEditor.chain().focus().insertContent(text).run();
      },
      getContent: () => {
        // markdown 扩展存在时优先发送 Markdown，否则退回纯文本。
        const storage = activeEditor.storage as {
          markdown?: { getMarkdown?: () => unknown };
        };
        const markdown = storage.markdown?.getMarkdown?.();
        return typeof markdown === "string" ? markdown : activeEditor.getText();
      },
    };
  };

  const runDeclarativeAction = async (
    item: PluginDocumentToolbarButton,
  ) => {
    const context = createActionContext();
    if (!context) return;

    const actionKey = `${item.pluginId}:${item.id}`;
    setRunningAction(actionKey);
    try {
      const title =
        noteId == null
          ? "当前文档"
          : await noteApi
              .get(noteId)
              .then((note) => note.title)
              .catch(() => "当前文档");
      await executeDeclarativeDocumentAction(item, title, context, {
        mockSummary: (input) => pluginApi.mockDocumentSummary(input),
        authorizeInsert: (input) =>
          pluginApi.recordDocumentSummaryInsert(input),
      });
      message.success("AI 摘要已插入当前文档");
    } catch (error) {
      console.error(
        `[PluginToolbar] ${item.pluginId}:${item.id} 执行失败`,
        error,
      );
      message.error(`AI 摘要执行失败：${String(error)}`);
    } finally {
      setRunningAction((current) => (current === actionKey ? null : current));
    }
  };

  if (items.length === 0) return null;

  return (
    <>
      <span
        aria-hidden
        style={{
          display: "inline-block",
          width: 1,
          height: 18,
          margin: "0 3px",
          background: "var(--ant-color-border-secondary, #f0f0f0)",
          verticalAlign: "middle",
        }}
      />
      {items.map((entry) => {
        const item = entry.item;
        const Icon = resolvePluginIconComponent(item.icon);
        const pluginName =
          entry.kind === "legacy"
            ? pluginManager.getPluginName(entry.item.pluginId) ??
              entry.item.pluginId
            : entry.item.pluginName;
        const buttonLabel =
          entry.kind === "declarative"
            ? entry.item.label
            : entry.item.tooltip;
        const actionKey = `${item.pluginId}:${item.id}`;
        return (
          <Tooltip
            key={`${entry.kind}:${actionKey}`}
            title={
              <span>
                <span style={{ opacity: 0.7, fontSize: 11 }}>{pluginName}</span>
                <br />
                {item.tooltip}
              </span>
            }
          >
            <Button
              type="text"
              size="small"
              aria-label={buttonLabel}
              icon={<Icon size={14} />}
              loading={
                entry.kind === "declarative" && runningAction === actionKey
              }
              onClick={async () => {
                if (entry.kind === "declarative") {
                  await runDeclarativeAction(entry.item);
                  return;
                }

                const context = createActionContext();
                if (!context) return;
                try {
                  await entry.item.callback(context);
                } catch (error) {
                  console.error(
                    `[PluginToolbar] ${item.pluginId}:${item.id} 执行失败`,
                    error,
                  );
                  pluginManager._logError(
                    item.pluginId,
                    "editor:toolbar",
                    String(error),
                  );
                  message.error(
                    `插件「${item.pluginId}」执行失败：${String(error)}`,
                  );
                }
              }}
              style={{ minWidth: 26, height: 26, padding: 0 }}
            />
          </Tooltip>
        );
      })}
    </>
  );
}
