/**
 * PluginToolbarButtons — 编辑器工具栏插件按钮组
 *
 * 从 pluginManager.subscribe("editor-toolbar") 拉取已激活插件注册的工具栏按钮，
 * 在编辑器顶栏末尾渲染（与内置按钮以细分隔线隔开）。
 *
 * 行为：点击执行 callback(EditorActionCtx)；错误隔离走 message.error。
 * 无插件时渲染 null，对 Toolbar 零开销。
 */

import { useEffect, useState } from "react";
import { Button, Tooltip, message } from "antd";
import { pluginManager } from "@/services/pluginManager";
import { getActiveEditor } from "@/services/editorBridge";
import { resolvePluginIconComponent } from "@/components/plugin/pluginIcons";
import type {
  PluginEditorToolbarButtonDef,
  EditorActionCtx,
} from "@/types";
import type { Editor } from "@tiptap/react";

type TBEntry = PluginEditorToolbarButtonDef & { pluginId: string };

interface Props {
  editor: Editor | null;
  noteId?: number | null;
}

export function PluginToolbarButtons({ editor, noteId }: Props) {
  const [items, setItems] = useState<TBEntry[]>([]);

  useEffect(() => {
    const refresh = () =>
      setItems(pluginManager.getRegisteredEditorToolbarButtons());
    refresh();
    return pluginManager.subscribe("editor-toolbar", refresh);
  }, []);

  if (items.length === 0) return null;

  return (
    <>
      {/* 细分隔线：与内置最后一个按钮组区分 */}
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
      {items.map((item) => {
        const Icon = resolvePluginIconComponent(item.icon);
        const pluginName =
          pluginManager.getPluginName(item.pluginId) ?? item.pluginId;
        return (
          <Tooltip
            key={`${item.pluginId}:${item.id}`}
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
              icon={<Icon size={14} />}
              onClick={async () => {
                const ed = editor ?? getActiveEditor();
                if (!ed) {
                  message.warning("编辑器尚未就绪");
                  return;
                }
                const sel = ed.state.selection;
                const selText = sel.empty
                  ? ""
                  : ed.state.doc.textBetween(sel.from, sel.to, "\n");
                const ctx: EditorActionCtx = {
                  noteId: noteId ?? null,
                  selection: selText,
                  replaceSelection: (text: string) => {
                    const s = ed.state.selection;
                    if (s.empty) {
                      ed.chain().focus().insertContent(text).run();
                    } else {
                      ed
                        .chain()
                        .focus()
                        .deleteSelection()
                        .insertContent(text)
                        .run();
                    }
                  },
                  insertText: (text: string) => {
                    ed.chain().focus().insertContent(text).run();
                  },
                  getContent: () => {
                    // eslint-disable-next-line @typescript-eslint/no-explicit-any
                    const md = (ed.storage as any).markdown?.getMarkdown?.();
                    return typeof md === "string" ? md : ed.getText();
                  },
                };
                try {
                  await item.callback(ctx);
                } catch (e) {
                  console.error(
                    `[PluginToolbar] ${item.pluginId}:${item.id} 抛错:`,
                    e,
                  );
                  pluginManager._logError(item.pluginId, "editor:toolbar", String(e));
                  message.error(`插件「${item.pluginId}」执行失败：${e}`);
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
