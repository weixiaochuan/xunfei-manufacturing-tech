
# Tiptap 插件化架构迁移指南

## 概述

本文档介绍如何将现有的 Tiptap 编辑器代码迁移到新的插件化架构。

## 迁移策略

我们采用**向后兼容**的迁移策略：

1. **保持现有代码不变**：`TiptapEditor` 组件继续正常工作
2. **增量采用新功能**：新代码可以使用 `UnifiedEditor` 和扩展系统
3. **平滑过渡**：逐步重构，不破坏现有功能

## 已完成的工作

### 1. 编辑器抽象接口

创建了 `EditorCore` 接口，定义了编辑器必须实现的方法：

- `getContent()` / `setContent()`
- `getSelection()` / `replaceSelection()` / `insertText()`
- `focus()` / `isReady()` / `destroy()`

文件位置：`src/types/editor.ts`

### 2. Tiptap 具体实现

- `TiptapEditorCore`：封装 Tiptap 编辑器，实现 `EditorCore` 接口
- `TiptapEditorAdapter`：适配器组件，保持向后兼容
- 编辑器注册表：管理所有可用的编辑器实现

文件位置：
- `src/components/editor/core/TiptapEditorCore.ts`
- `src/components/editor/TiptapEditorAdapter.tsx`
- `src/components/editor/core/EditorRegistry.tsx`

### 3. 编辑器 Provider 和 Hooks

- `EditorProvider`：管理编辑器状态和生命周期
- `useEditor`：访问编辑器上下文
- `useEditorState` / `useEditorContent`：便捷 Hooks

文件位置：`src/components/editor/core/EditorProvider.tsx`

### 4. 统一编辑器组件

- `UnifiedEditor`：支持多种编辑器实现的选择和切换
- 自动根据文件类型选择合适的编辑器

文件位置：`src/components/editor/UnifiedEditor.tsx`

## 现有代码的兼容性

### 继续使用 TiptapEditor

现有代码不需要任何修改，可以继续使用：

```typescript
// ✅ 继续工作
import { TiptapEditor } from "@/components/editor";

function MyComponent() {
  return (
    <TiptapEditor
      content={content}
      onChange={onChange}
      noteId={noteId}
      // ... 其他 props
    />
  );
}
```

### 逐步采用新功能

新代码可以使用 `UnifiedEditor`：

```typescript
// ✅ 新代码使用
import { UnifiedEditor } from "@/components/editor";

function NewComponent() {
  return (
    <UnifiedEditor
      content={content}
      onChange={onChange}
      noteId={noteId}
      editorId="tiptap" // 可选，指定编辑器
      fileType="md" // 可选，根据文件类型选择
    />
  );
}
```

## 待完成的工作

### 阶段 4：实现编辑器扩展点

- [ ] 工具栏扩展点集成
- [ ] 右键菜单扩展点集成
- [ ] 快捷键扩展点集成
- [ ] 编辑器钩子系统（beforeSave / afterLoad）

### 阶段 5：Rust 侧支持

- [ ] 添加插件能力清单到 models
- [ ] 添加编辑器插件权限
- [ ] 扩展 plugin_proxy 命令支持

### 阶段 6：测试和文档

- [ ] 编写集成测试
- [ ] 完整测试现有功能
- [ ] 更新开发者文档
- [ ] 测试现有插件是否正常工作

### 阶段 7：示例插件（可选）

- [ ] 创建 Markdown 源码编辑器插件
- [ ] 创建代码编辑器插件（基于 CodeMirror）
- [ ] 完整文档

## 如何添加新的编辑器实现

### 1. 创建 EditorCore 实现

```typescript
// src/components/editor/editors/MyEditorCore.ts
import { EditorCore, EditorConfig } from "@/types/editor";

export class MyEditorCore implements EditorCore {
  // 实现接口方法
}
```

### 2. 注册编辑器

```typescript
// 在 App 初始化时
import { editorRegistry } from "@/components/editor";

editorRegistry.register({
  id: "my-editor",
  name: "My Editor",
  supportedFileTypes: ["ext"],
  features: [/* 功能列表 */],
  factory: (config) => new MyEditorCore(config),
  defaultConfig: { /* 配置 */ },
});
```

### 3. 使用编辑器

```typescript
import { UnifiedEditor } from "@/components/editor";

<UnifiedEditor
  content={content}
  onChange={onChange}
  editorId="my-editor"
/>
```

## 架构变化对比

### 之前

```
TiptapEditor (单体组件)
  ├── 直接使用 Tiptap
  ├── 内置所有功能
  └── 难以扩展
```

### 现在

```
UnifiedEditor (统一入口)
  ├── EditorRegistry (编辑器注册表)
  │     ├── TiptapEditorCore (Tiptap 实现)
  │     └── 其他编辑器实现...
  ├── EditorProvider (状态管理)
  └── EditorExtension (扩展系统)
        ├── ToolbarButtons
        ├── ContextMenuItems
        └── Shortcuts
```

## 迁移检查清单

- [x] 设计编辑器抽象接口
- [x] 抽离 Tiptap 具体实现
- [x] 保持向后兼容
- [x] 创建编辑器注册表
- [x] 实现 EditorProvider 和 Hooks
- [ ] 实现编辑器扩展点
- [ ] 添加编辑器插件权限
- [ ] 完整测试现有功能
- [ ] 编写迁移指南
- [ ] 更新开发者文档

## 常见问题

### Q: 现有代码需要修改吗？

A: 不需要。现有代码可以继续使用 `TiptapEditor`，保持完全兼容。

### Q: 如何测试新架构？

A: 可以先在新功能中使用 `UnifiedEditor`，验证正常工作后再逐步迁移。

### Q: 插件系统如何集成？

A: 编辑器插件将通过现有的插件系统集成，使用 `PluginEditorAdapter` 组件。

## 下一步

1. 继续实现阶段 4-7
2. 编写完整的测试
3. 更新文档
4. 发布第一个版本
