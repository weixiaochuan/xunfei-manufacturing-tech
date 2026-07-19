/**
 * EditorBridge —— 在 Tiptap 编辑器与插件 API 之间架的轻量桥。
 *
 * 问题：插件 API（pluginApi.ts）需要被插件随时调用（onLoad、命令回调等），
 * 但 Tiptap 的 editor 实例存活在某个 React 组件的 state / useEditor 里，
 * 没有现成的全局入口。
 *
 * 方案：editor 组件 mount 时调 setActiveEditor 注册自己，unmount 时清空。
 * 同一时刻只允许一个 active editor（笔记应用本来就是单 active 编辑）。
 *
 * 不放到 Zustand 是因为 editor 实例的引用变化非常频繁（每次内容变更都可能
 * 触发新引用），走 store 会让所有订阅者无谓重渲染。这里只暴露同步 getter，
 * 谁需要谁主动调，零订阅成本。
 */
import type { Editor } from "@tiptap/react";

interface EditorBridgeState {
  editor: Editor | null;
  noteId: number | null;
}

const state: EditorBridgeState = {
  editor: null,
  noteId: null,
};

export function setActiveEditor(editor: Editor | null, noteId: number | null) {
  state.editor = editor;
  state.noteId = noteId;
}

export function getActiveEditor(): Editor | null {
  return state.editor;
}

export function getActiveNoteId(): number | null {
  return state.noteId;
}

/** 同步取当前编辑器的纯文本选区；无激活编辑器返回 null */
export function getActiveSelectionText(): string | null {
  const ed = state.editor;
  if (!ed) return null;
  const { from, to } = ed.state.selection;
  if (from === to) return "";
  return ed.state.doc.textBetween(from, to, "\n");
}

/** 在光标位置插入文本（保留选区，不删除） */
export function insertTextAtCursor(text: string): boolean {
  const ed = state.editor;
  if (!ed) return false;
  ed.chain().focus().insertContent(text).run();
  return true;
}

/** 用选中文本替换；无选区时插入到光标位置（Tiptap 的 insertContent 行为本身就这样） */
export function replaceSelectionWithText(text: string): boolean {
  const ed = state.editor;
  if (!ed) return false;
  const { from, to } = ed.state.selection;
  if (from === to) {
    ed.chain().focus().insertContent(text).run();
  } else {
    ed.chain().focus().deleteSelection().insertContent(text).run();
  }
  return true;
}

/** 取整篇内容为 markdown 文本（只读）*/
export function getActiveContentMarkdown(): string {
  const ed = state.editor;
  if (!ed) return "";
  // Tiptap 默认的 getText 是纯文本；若需要真 markdown 需 storage.markdown 扩展。
  // 当前项目编辑器是否注册 markdown storage 由 TiptapEditor 决定，这里先用 getText 兜底，
  // 避免插件拿到空字符串误以为编辑器没就绪。
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const md = (ed.storage as any).markdown?.getMarkdown?.();
  if (typeof md === "string") return md;
  return ed.getText();
}
