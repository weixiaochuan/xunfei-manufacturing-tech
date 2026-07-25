
/**
 * 统一编辑器组件
 * 支持多种编辑器实现的选择和切换
 */

import { useMemo } from "react";
import { useEditorRegistry } from "./core/EditorRegistry";
import { EditorProvider } from "./core/EditorProvider";
import { TiptapEditorAdapter } from "./TiptapEditorAdapter";
import { TiptapEditorCore } from "./core/TiptapEditorCore";
import type { EditorCore } from "@/types/editor";

interface UnifiedEditorProps {
  /** 编辑器内容 */
  content: string;

  /** 内容变更回调 */
  onChange: (content: string) => void;

  /** 占位符文字 */
  placeholder?: string;

  /** 笔记 ID */
  noteId?: number;

  /** 确保笔记 ID 的回调 */
  ensureNoteId?: () => Promise<number>;

  /** Wiki 链接点击回调 */
  onWikiLinkClick?: (title: string) => void;

  /** AI 询问回调 */
  onAskAi?: (selectedText: string) => void;

  /** 编辑器就绪回调 */
  onEditorReady?: (editor: any) => void;

  /** 是否显示底部统计 */
  showFooterStats?: boolean;

  /** 强制使用的编辑器 ID */
  editorId?: string;

  /** 文件类型（用于自动选择编辑器） */
  fileType?: string;
}

/**
 * 统一编辑器组件
 * 根据配置或文件类型自动选择合适的编辑器
 */
export function UnifiedEditor({
  content,
  onChange,
  placeholder,
  noteId,
  ensureNoteId,
  onWikiLinkClick,
  onAskAi,
  onEditorReady,
  showFooterStats = true,
  editorId,
  fileType,
}: UnifiedEditorProps) {
  const registry = useEditorRegistry();

  // 选择编辑器
  const selectedEditor = useMemo(() => {
    // 如果指定了编辑器 ID，优先使用
    if (editorId) {
      const editor = registry.getEditor(editorId);
      if (editor) return editor;
    }

    // 根据文件类型选择
    if (fileType) {
      const editor = registry.getEditorForFileType(fileType);
      if (editor) return editor;
    }

    // 默认使用 Tiptap
    return registry.getEditor("tiptap");
  }, [registry, editorId, fileType]);

  // 创建编辑器实例
  const editorCore = useMemo<EditorCore | null>(() => {
    if (!selectedEditor) return null;

    const config = {
      id: `${selectedEditor.id}-${noteId || "new"}`,
      initialContent: content,
      placeholder,
    };

    return selectedEditor.factory(config);
  }, [selectedEditor, content, placeholder, noteId]);

  // 如果没有可用的编辑器，显示错误
  if (!selectedEditor || !editorCore) {
    return (
      <div className="p-4 text-center text-red-500">
        没有可用的编辑器
      </div>
    );
  }

  // 根据编辑器类型渲染相应的组件
  const renderEditor = () => {
    // Tiptap 编辑器
    if (selectedEditor.id === "tiptap" || editorCore instanceof TiptapEditorCore) {
      return (
        <TiptapEditorAdapter
          content={content}
          onChange={onChange}
          placeholder={placeholder}
          noteId={noteId}
          ensureNoteId={ensureNoteId}
          onWikiLinkClick={onWikiLinkClick}
          onAskAi={onAskAi}
          onEditorReady={onEditorReady}
          showFooterStats={showFooterStats}
        />
      );
    }

    // 其他编辑器可以在这里添加

    // 降级方案
    return (
      <div className="p-4 text-center">
        编辑器 {selectedEditor.name} 暂未实现
      </div>
    );
  };

  // 使用 EditorProvider 包装
  return (
    <EditorProvider
      editor={editorCore}
      noteId={noteId}
      initialContent={content}
      onContentChange={onChange}
    >
      {renderEditor()}
    </EditorProvider>
  );
}

/**
 * 兼容层：保持 TiptapEditor 的接口
 * 向后兼容旧代码
 */
export function TiptapEditor(props: UnifiedEditorProps) {
  return <UnifiedEditor {...props} editorId="tiptap" />;
}
