
/**
 * Tiptap Editor 适配器组件
 * 保持与现有 TiptapEditor 相同的接口，但使用新的插件化架构
 */

import { useEffect, useRef } from "react";
import { TiptapEditorCore } from "./core/TiptapEditorCore";
import { EditorProvider, useEditor } from "./core/EditorProvider";
import { TiptapEditor } from "./TiptapEditor";

interface TiptapEditorAdapterProps {
  content: string;
  onChange: (content: string) => void;
  placeholder?: string;
  noteId?: number;
  documentTitle?: string;
  ensureNoteId?: () => Promise<number>;
  onWikiLinkClick?: (title: string) => void;
  onAskAi?: (selectedText: string) => void;
  onEditorReady?: (editor: any) => void;
  showFooterStats?: boolean;
}

/**
 * 内部包装组件
 */
function TiptapEditorWrapper({
  content,
  onChange,
  placeholder,
  noteId,
  documentTitle,
  ensureNoteId,
  onWikiLinkClick,
  onAskAi,
  onEditorReady,
  showFooterStats,
}: TiptapEditorAdapterProps) {
  const { editor: coreEditor, setContent } = useEditor();
  const tiptapEditorRef = useRef<any>(null);

  // 当外部 content 变化时，更新编辑器
  useEffect(() => {
    setContent(content);
  }, [content, setContent]);

  // 处理编辑器内容变更
  const handleChange = (newContent: string) => {
    onChange(newContent);
  };

  // 处理编辑器就绪
  const handleEditorReady = (editor: any) => {
    // 将 Tiptap 实例设置到 Core
    if (coreEditor && coreEditor instanceof TiptapEditorCore) {
      coreEditor.setEditorInstance(editor);
    }
    tiptapEditorRef.current = editor;
    onEditorReady?.(editor);
  };

  return (
    <TiptapEditor
      content={content}
      onChange={handleChange}
      placeholder={placeholder}
      noteId={noteId}
      documentTitle={documentTitle}
      ensureNoteId={ensureNoteId}
      onWikiLinkClick={onWikiLinkClick}
      onAskAi={onAskAi}
      onEditorReady={handleEditorReady}
      showFooterStats={showFooterStats}
    />
  );
}

/**
 * Tiptap Editor 适配器组件
 * 包装现有 TiptapEditor，使用新的插件化架构
 */
export function TiptapEditorAdapter(props: TiptapEditorAdapterProps) {
  // 创建编辑器实例
  const editorCoreRef = useRef<TiptapEditorCore | null>(null);

  if (!editorCoreRef.current) {
    editorCoreRef.current = new TiptapEditorCore({
      id: `tiptap-adapter-${Date.now()}`,
      initialContent: props.content,
      placeholder: props.placeholder,
    });
  }

  // 当 content 变化时更新
  useEffect(() => {
    if (editorCoreRef.current) {
      // 不在这里直接设置，因为 Provider 会处理
    }
  }, [props.content]);

  return (
    <EditorProvider
      editor={editorCoreRef.current}
      noteId={props.noteId}
      initialContent={props.content}
      onContentChange={props.onChange}
    >
      <TiptapEditorWrapper {...props} />
    </EditorProvider>
  );
}
