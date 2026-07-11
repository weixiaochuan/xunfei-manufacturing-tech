# Tiptap 插件化开发任务规划

## 概述

本规划将现有的 Tiptap 编辑器核心模块重构为可插拔的编辑器插件系统，同时保持完整功能和 API 兼容性。

## 目标

1. **架构解耦**：将编辑器核心与业务逻辑分离
2. **可插拔**：支持热插拔不同的编辑器实现
3. **向后兼容**：保持现有 API 不变
4. **多编辑器支持**：为未来支持其他编辑器（如 CodeMirror、Monaco）奠定基础

## 阶段规划

### 阶段 1：设计编辑器抽象接口 (1-2天)

#### 任务
- [ ] 定义 `EditorCore` 接口
- [ ] 定义 `EditorExtension` 接口（扩展点）
- [ ] 定义 `EditorContext` 上下文类型
- [ ] 定义事件系统（如 `onContentChange`、`onSelectionChange`）

#### 输出文件
```
src/types/editor.ts
```

#### 技术要点
- 保持与现有 `TiptapEditor` props 兼容
- 提供必要的抽象方法（如 `getContent()`、`setContent()`、`getSelection()`）
- 支持扩展注册机制（工具栏、菜单、快捷键等）

---

### 阶段 2：抽离 Tiptap 具体实现 (2-3天)

#### 任务
- [ ] 创建 `TiptapEditorCore` 类，实现 `EditorCore` 接口
- [ ] 将现有 `TiptapEditor.tsx` 重构为适配器
- [ ] 抽离通用编辑器组件（如 `EditorToolbar`、`EditorContextMenu`）
- [ ] 保持图片、视频、数学公式等功能工作正常

#### 输出文件
```
src/components/editor/core/
  ├─ TiptapEditorCore.tsx
  ├─ EditorToolbar.tsx
  ├─ EditorContextMenu.tsx
  └─ extensions/
      ├─ FigureExtension.tsx
      ├─ VideoExtension.tsx
      └─ MathExtension.tsx
```

#### 技术要点
- **向后兼容**：保持现有 `TiptapEditor.tsx` API 不变
- **增量迁移**：旧版可继续工作，新功能用插件架构
- **扩展注册**：通过 `registerToolbarButton()`、`registerContextMenu()` 等方法注册扩展点

---

### 阶段 3：设计插件化架构 (2天)

#### 任务
- [ ] 定义 EditorPlugin 接口
- [ ] 实现插件管理器 `PluginManager.editors` 子系统
- [ ] 设计插件清单格式 `plugin.manifest.editors`
- [ ] 实现插件加载和卸载机制

#### 输出文件
```
src/services/pluginApi.ts (扩展 editor API)
src/components/editor/PluginEditorAdapter.tsx
```

#### 插件接口设计
```typescript
// 插件可提供的编辑器能力
interface EditorPlugin {
  id: string;
  name: string;
  provides: {
    editor: {
      component: React.ComponentType<EditorProps>;
      fileTypes: string[]; // 处理的文件类型
      features: string[]; // ['rich-text', 'markdown', 'code', ...]
    }
  }
}
```

---

### 阶段 4：实现编辑器扩展点 (2-3天)

#### 任务
- [ ] 实现工具栏扩展点（插件可添加按钮）
- [ ] 实现右键菜单扩展点（插件可添加菜单项）
- [ ] 实现快捷键扩展点
- [ ] 实现编辑器钩子（如 `beforeSave`、`afterLoad`）

#### 技术要点
- 复用现有的 `PluginToolbarButtons` 架构
- 保持与现有插件 API 的一致性
- 支持条件显示（如 `when: 'hasSelection'`）

---

### 阶段 5：Rust 侧支持 (1-2天)

#### 任务
- [ ] 添加插件能力清单到 `models/mod.rs`
- [ ] 添加编辑器插件权限（如 `editor:read`、`editor:write`）
- [ ] 扩展 `commands/plugin_proxy.rs`，支持编辑器代理命令

#### 数据库变更
无需变更现有表，可复用 `plugin_config`。

---

### 阶段 6：测试和文档 (1-2天)

#### 任务
- [ ] 编写集成测试
- [ ] 编写迁移指南
- [ ] 更新开发者文档
- [ ] 测试现有插件（如 pomodoro 等）是否继续正常工作

#### 兼容性测试清单
- [ ] 普通笔记编辑保存正常
- [ ] WikiLink 解析和跳转正常
- [ ] 图片/视频插入正常
- [ ] AI 上下文菜单正常
- [ ] 任务提醒正常
- [ ] 现有插件（如 pomodoro）正常工作
- [ ] 移动端编辑正常工作

---

### 阶段 7：示例插件 (可选，+2天)

#### 任务
- [ ] 创建 Markdown 源码编辑器插件
- [ ] 创建代码编辑器插件（基于 CodeMirror）
- [ ] 完整文档

---

## 文件清单（预计新增）

### TypeScript
```
src/types/editor.ts              # 编辑器接口定义
src/components/editor/core/      # 编辑器核心实现
src/components/editor/EditorRegistry.tsx  # 编辑器注册表
```

### Rust
- [ ] 扩展 `models/mod.rs` - 添加编辑器插件能力声明
- [ ] 扩展 `commands/plugin_proxy.rs` - 添加编辑器代理命令

### 文档
```
docs/编辑器插件开发.md
docs/迁移-Tiptap到插件架构.md
```

## 风险与缓解

### 风险 1：破坏现有功能
**缓解**：
- 保持现有 `TiptapEditor.tsx` API 完全不变
- 内部实现重构时保持对外接口稳定
- 详尽回归测试

### 风险 2：性能下降
**缓解**：
- 避免过度抽象
- 保持增量渲染
- 进行性能基准测试

### 风险 3：复杂度过高
**缓解**：
- 保持插件 API 简洁
- 提供清晰文档和示例
- 先支持核心功能，逐步开放高级特性

## 依赖关系树

```
阶段 1 (接口设计)
    ↓
阶段 2 (Tiptap 实现重构)
    ↓
阶段 3 (插件架构)
    ↓
阶段 4 (扩展点实现)
    ↓
阶段 5 (Rust 侧支持)
    ↓
阶段 6 (测试和文档)
```

## 资源需求

### 开发者
- 熟悉 React/TypeScript
- 了解 Tiptap/ProseMirror
- 了解现有项目架构

### 测试环境
- Windows/macOS/Linux
- 现有笔记库用于测试迁移

## 成功标准

1. 现有所有功能正常工作（无回归）
2. 插件 API 可以被正确调用
3. 文档完整，示例工作正常
4. 性能不低于当前版本

## 总工期估算

- **基础目标**：阶段 1-6 = 约 10-14个工作日
- **完整目标**：阶段 1-7 = 约 12-16个工作日

## 验收条件

- [ ] 现有测试通过（无回归）
- [ ] 文档完整可用
- [ ] 代码通过 Code Review
- [ ] 编辑器插件 API 可以被正确调用（可以用 demo 插件验证）

---

## 下一步

计划审批通过后，先执行阶段 1：设计编辑器抽象接口，这是所有后续工作的基础。

---

*本文档最后更新时间：2026-06-10*
