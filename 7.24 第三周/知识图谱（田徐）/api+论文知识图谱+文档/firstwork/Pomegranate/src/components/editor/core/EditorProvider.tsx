import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";
import type { EditorCore, EditorProviderProps } from "@/types/editor";

export interface TiptapEditorLike {
  commands?: {
    setContent?: (content: string, options?: { emitUpdate?: boolean }) => void;
    focus?: () => void;
    insertContent?: (content: string) => void;
  };
  getHTML?: () => string;
  getText?: () => string;
  isDestroyed?: boolean;
  destroy?: () => void;
}

interface EditorContextValue {
  editor: EditorCore;
  noteId: number | null;
  readOnly: boolean;
  setEditorInstance: (editor: TiptapEditorLike | null) => void;
  getEditorInstance: () => TiptapEditorLike | null;
  setContent: (content: string) => void;
  getContent: () => string;
}

const EditorContext = createContext<EditorContextValue | null>(null);

function isTiptapEditorLike(value: unknown): value is TiptapEditorLike {
  return typeof value === "object" && value !== null;
}

export function EditorProvider({
  editor,
  children,
  noteId,
  readOnly = false,
  initialContent = "",
}: EditorProviderProps) {
  const [content, setContentState] = useState(initialContent);
  const editorInstanceRef = useRef<TiptapEditorLike | null>(null);

  const setEditorInstance = useCallback((instance: TiptapEditorLike | null) => {
    editorInstanceRef.current = instance;
    if ("setEditorInstance" in editor) {
      const setter = editor.setEditorInstance;
      if (typeof setter === "function") {
        setter.call(editor, instance);
      }
    }
  }, [editor]);

  const getEditorInstance = useCallback(() => editorInstanceRef.current, []);

  const setContent = useCallback(
    (nextContent: string) => {
      setContentState(nextContent);
      const instance = editorInstanceRef.current;
      if (instance?.commands?.setContent) {
        instance.commands.setContent(nextContent, { emitUpdate: false });
        return;
      }
      editor.setContent(nextContent);
    },
    [editor],
  );

  const getContent = useCallback(() => {
    const instance = editorInstanceRef.current;
    if (instance?.getHTML) return instance.getHTML();
    if (instance?.getText) return instance.getText();
    if (isTiptapEditorLike(instance)) return "";
    const editorContent = editor.getContent();
    return editorContent || content;
  }, [content, editor]);

  const value = useMemo<EditorContextValue>(
    () => ({
      editor,
      noteId: noteId ?? null,
      readOnly,
      setEditorInstance,
      getEditorInstance,
      setContent,
      getContent,
    }),
    [
      editor,
      noteId,
      readOnly,
      setEditorInstance,
      getEditorInstance,
      setContent,
      getContent,
    ],
  );

  return (
    <EditorContext.Provider value={value}>{children}</EditorContext.Provider>
  );
}

export function useEditor(): EditorContextValue {
  const context = useContext(EditorContext);
  if (!context) {
    throw new Error("useEditor must be used within EditorProvider");
  }
  return context;
}

