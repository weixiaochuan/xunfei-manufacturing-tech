
/**
 * 注册 Tiptap Editor 到编辑器注册表
 */

import { editorRegistry } from "./core/EditorRegistry";
import { TiptapEditorCore } from "./core/TiptapEditorCore";
import { EditorConfig } from "@/types/editor";

/**
 * 注册 Tiptap Editor
 */
export function registerTiptapEditor() {
  editorRegistry.register({
    id: "tiptap",
    name: "Tiptap Editor",
    description: "功能丰富的富文本编辑器，支持 Markdown",
    version: "1.0.0",
    supportedFileTypes: ["md", "markdown", "txt", "text"],
    features: [
      "rich-text",
      "markdown",
      "images",
      "videos",
      "tables",
      "links",
      "tasks",
      "math",
      "diagrams",
    ],
    factory: (config: EditorConfig) => new TiptapEditorCore(config),
    defaultConfig: {
      spellcheck: true,
      syntaxHighlight: true,
      autoSave: true,
      autoSaveInterval: 30000,
      theme: "light",
    },
  });

  console.log("[EditorRegistry] Tiptap Editor registered");
}

/**
 * 初始化编辑器注册表
 * 注册所有内置编辑器
 */
export function initEditorRegistry() {
  registerTiptapEditor();
  // 可以在这里注册其他编辑器
}
