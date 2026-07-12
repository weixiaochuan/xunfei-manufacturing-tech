import type { EditorConfig, EditorCore } from "@/types/editor";
import type { TiptapEditorLike } from "./EditorProvider";

export class TiptapEditorCore implements EditorCore {
  readonly type = "tiptap";
  readonly id: string;
  readonly name = "Tiptap Editor";
  readonly version = "1.0.0";
  readonly supportedFileTypes = ["md", "markdown", "txt", "text"];
  readonly features = [
    "rich-text",
    "markdown",
    "images",
    "videos",
    "tables",
    "links",
    "tasks",
    "math",
    "diagrams",
  ];

  private editorInstance: TiptapEditorLike | null = null;
  private content: string;

  constructor(config: EditorConfig = {}) {
    this.id = config.id ?? "tiptap";
    this.content =
      typeof config.initialContent === "string" ? config.initialContent : "";
  }

  setEditorInstance(editor: TiptapEditorLike | null): void {
    this.editorInstance = editor;
  }

  getEditorInstance(): TiptapEditorLike | null {
    return this.editorInstance;
  }

  getContent(): string {
    if (this.editorInstance?.getHTML) return this.editorInstance.getHTML();
    if (this.editorInstance?.getText) return this.editorInstance.getText();
    return this.content;
  }

  setContent(content: string): void {
    this.content = content;
    if (this.editorInstance?.commands?.setContent) {
      this.editorInstance.commands.setContent(content, { emitUpdate: false });
    }
  }

  getSelection(): string {
    return "";
  }

  replaceSelection(text: string): void {
    this.insertText(text);
  }

  insertText(text: string): void {
    if (this.editorInstance?.commands?.insertContent) {
      this.editorInstance.commands.insertContent(text);
      return;
    }
    this.content += text;
  }

  focus(): void {
    this.editorInstance?.commands?.focus?.();
  }

  isReady(): boolean {
    return Boolean(this.editorInstance && !this.editorInstance.isDestroyed);
  }

  destroy(): void {
    if (this.editorInstance && !this.editorInstance.isDestroyed) {
      this.editorInstance.destroy?.();
    }
    this.editorInstance = null;
  }
}

