
// 向后兼容：保持现有代码继续使用 TiptapEditor
export { TiptapEditor } from "./TiptapEditor";

// 新的插件化架构导出
export * from "./core";
export * from "./registerTiptapEditor";
export { TiptapEditorAdapter } from "./TiptapEditorAdapter";
export { UnifiedEditor } from "./UnifiedEditor";
export { PluginEditorAdapter } from "./PluginEditorAdapter";

