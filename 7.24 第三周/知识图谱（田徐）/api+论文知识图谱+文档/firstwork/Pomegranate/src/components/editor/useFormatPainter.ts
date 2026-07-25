import { useCallback, useEffect, useRef, useState } from "react";
import type { Editor } from "@tiptap/react";

export type FormatPainterMode = "idle" | "once" | "persist";

/**
 * 格式刷只搬运"格式相关" marks，不动 link / annotation 这类语义 mark。
 * textStyle 覆盖了字体颜色 (color) 与字号 (fontSize)。
 */
const FORMAT_MARK_NAMES = [
  "bold",
  "italic",
  "underline",
  "strike",
  "highlight",
  "code",
  "superscript",
  "subscript",
  "textStyle",
];

interface PickedMark {
  name: string;
  attrs: Record<string, unknown>;
}

/**
 * 格式刷 Hook（仿 Office 行为）：
 * - 单击按钮：吸取当前光标/选区处的格式 → 下次选中目标文本应用一次后自动退出（once 模式）
 * - 双击按钮：进入持续模式，每次选中文本都会应用，直到 Esc 或再次点击退出（persist 模式）
 * - 应用时只清除/设置白名单内的 marks，避免误清 link
 *
 * Why：双击的实现依赖浏览器 click + dblclick 同时触发的天然顺序：
 *   click1 → idle 切到 once，click2 → once 被 cancel 回 idle，dblclick → 重新 pick 进 persist。
 *   终态 persist 正确，中间一帧抖动肉眼不可见，避免 setTimeout 防抖带来的延迟感。
 */
export function useFormatPainter(editor: Editor | null) {
  const [mode, setMode] = useState<FormatPainterMode>("idle");
  const pickedRef = useRef<PickedMark[]>([]);

  const pick = useCallback((): boolean => {
    if (!editor) return false;
    const { state } = editor;
    const { from, to, $from } = state.selection;

    // 空选区取 storedMarks（输入下一个字符会带上的 marks），否则取选区起点处的 marks
    // 与 Office 一致：跨段选区也只看起点的格式
    const marksAtPos =
      from === to ? (state.storedMarks ?? $from.marks()) : $from.marks();

    pickedRef.current = marksAtPos
      .filter((m) => FORMAT_MARK_NAMES.includes(m.type.name))
      .map((m) => ({ name: m.type.name, attrs: { ...m.attrs } }));
    return true;
  }, [editor]);

  const apply = useCallback((): boolean => {
    if (!editor) return false;
    const { from, to } = editor.state.selection;
    if (from === to) return false;

    const chain = editor.chain().focus();
    for (const name of FORMAT_MARK_NAMES) {
      chain.unsetMark(name);
    }
    for (const m of pickedRef.current) {
      chain.setMark(m.name, m.attrs);
    }
    chain.run();
    return true;
  }, [editor]);

  const cancel = useCallback(() => {
    setMode("idle");
    pickedRef.current = [];
  }, []);

  const pickOnce = useCallback(() => {
    if (mode !== "idle") {
      cancel();
      return;
    }
    if (pick()) setMode("once");
  }, [mode, pick, cancel]);

  const pickPersist = useCallback(() => {
    if (pick()) setMode("persist");
  }, [pick]);

  useEffect(() => {
    if (!editor || mode === "idle") return;

    const onSelectionUpdate = () => {
      const { from, to } = editor.state.selection;
      if (from === to) return;
      apply();
      if (mode === "once") cancel();
    };

    editor.on("selectionUpdate", onSelectionUpdate);
    return () => {
      editor.off("selectionUpdate", onSelectionUpdate);
    };
  }, [editor, mode, apply, cancel]);

  useEffect(() => {
    if (mode === "idle") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [mode, cancel]);

  return { mode, pickOnce, pickPersist, cancel };
}
