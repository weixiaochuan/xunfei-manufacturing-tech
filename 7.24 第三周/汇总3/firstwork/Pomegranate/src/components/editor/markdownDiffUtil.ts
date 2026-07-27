/**
 * 对比/合并工具的辅助函数。
 *
 * 提取自 CompareClipboardButton，供两组件共享。
 */
import type { Editor } from "@tiptap/react";

/**
 * 从 Tiptap editor 提取 markdown 源码。
 * 依赖 `@tiptap/extension-markdown` 的 `editor.storage.markdown.getMarkdown()`。
 */
export function getNoteMarkdown(editor: Editor): string {
  const storage = editor.storage as { markdown?: { getMarkdown: () => string } };
  if (storage.markdown?.getMarkdown) {
    return storage.markdown.getMarkdown();
  }
  // 兜底：拿纯文本
  return editor.getText({ blockSeparator: "\n\n" });
}

/**
 * 从 Tiptap editor 提取可见纯文本（去除 markdown 标记）。
 */
export function getNotePlainText(editor: Editor): string {
  return editor.getText({ blockSeparator: "\n" });
}

/**
 * 启发式判断剪贴板文本"看起来像不像 markdown"。
 */
export function looksLikeMarkdown(text: string): boolean {
  if (!text) return false;

  if (/^#{1,6}\s/m.test(text)) return true;
  if (/\*\*[^*]+\*\*/.test(text)) return true;
  if (/__[^_]+__/.test(text)) return true;
  if (/`[^`]+`/.test(text)) return true;
  if (/\[.+\]\(https?:\/\/.+\)/.test(text)) return true;
  if (/!\[.*\]\(.+\)/.test(text)) return true;
  if (/^[-*+]\s/m.test(text)) return true;
  if (/^\d+\.\s/m.test(text)) return true;
  if (/^>\s/m.test(text)) return true;
  if (/```/.test(text)) return true;
  if (/<(\w+)[^>]*>/.test(text)) return true;
  if (/^---\s*$/m.test(text)) return true;

  return false;
}
