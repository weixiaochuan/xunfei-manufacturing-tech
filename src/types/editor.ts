
/**
 * 编辑器插件化架构 - 类型定义
 */

import React from "react";

// ─── 编辑器核心接口 ─────────────────────────────────────

/**
 * 编辑器核心接口
 * 所有编辑器实现（TiptapEditor、CodeMirrorEditor等）都需要实现此接口
 */
export interface EditorCore {
  /** 编辑器唯一标识 */
  readonly id: string;

  /** 编辑器名称（用于UI展示） */
  readonly name: string;

  /** 编辑器版本 */
  readonly version: string;

  /** 支持的文件类型（扩展名数组，如 ['md', 'txt']） */
  readonly supportedFileTypes: string[];

  /** 支持的功能特性列表 */
  readonly features: string[];

  /** 获取当前编辑器内容 */
  getContent(): string;

  /** 设置编辑器内容 */
  setContent(content: string): void;

  /** 获取当前选中文本 */
  getSelection(): string;

  /** 替换选中内容 */
  replaceSelection(text: string): void;

  /** 在光标位置插入文本 */
  insertText(text: string): void;

  /** 聚焦编辑器 */
  focus(): void;

  /** 编辑器是否已挂载/就绪 */
  isReady(): boolean;

  /** 销毁编辑器，清理资源 */
  destroy(): void;
}

// ─── 编辑器上下文 ─────────────────────────────────────

/**
 * 编辑器上下文
 * 传递给编辑器扩展和插件的上下文信息
 */
export interface EditorContext {
  /** 当前笔记 ID（如果有） */
  noteId: number | null;

  /** 当前笔记内容（只读） */
  content: string;

  /** 编辑器核心实例 */
  editor: EditorCore;

  /** App API（如果在插件环境中） */
  appApi?: any;

  /** 访问当前状态的方法 */
  getState: () => EditorState;

  /** 更新编辑器状态的方法 */
  setState: (newState: Partial<EditorState>) => void;
}

// ─── 编辑器状态 ───────────────────────────────────────

/**
 * 编辑器状态
 */
export interface EditorState {
  /** 当前内容 */
  content: string;

  /** 是否正在编辑（有未保存的变更） */
  isDirty: boolean;

  /** 是否只读模式 */
  isReadOnly: boolean;

  /** 是否正在加载 */
  isLoading: boolean;

  /** 当前编辑器实例 */
  editorInstance: EditorCore | null;
}

// ─── 编辑器扩展接口 ───────────────────────────────────

/**
 * 编辑器扩展接口
 * 插件可以通过扩展点增强编辑器功能
 */
export interface EditorExtension {
  /** 扩展唯一 ID */
  id: string;

  /** 扩展名称 */
  name: string;

  /** 扩展描述 */
  description?: string;

  /** 扩展版本 */
  version?: string;

  /**
   * 扩展初始化时调用
   */
  onInit?: (context: EditorContext) => void | Promise<void>;

  /**
   * 编辑器内容变更时调用
   */
  onContentChange?: (content: string, context: EditorContext) => void;

  /**
   * 编辑器选区变更时调用
   */
  onSelectionChange?: (selection: string, context: EditorContext) => void;

  /**
   * 编辑器获得焦点时调用
   */
  onFocus?: (context: EditorContext) => void;

  /**
   * 编辑器失去焦点时调用
   */
  onBlur?: (context: EditorContext) => void;

  /**
   * 编辑器销毁时调用
   */
  onDestroy?: (context: EditorContext) => void;

  /**
   * 自定义工具栏按钮（可选）
   */
  toolbarButtons?: ToolbarButton[];

  /**
   * 自定义右键菜单（可选）
   */
  contextMenuItems?: ContextMenuItem[];

  /**
   * 自定义快捷键（可选）
   */
  shortcuts?: KeyboardShortcut[];
}

// ─── 工具栏按钮 ───────────────────────────────────────

/**
 * 工具栏按钮定义
 */
export interface ToolbarButton {
  /** 按钮 ID */
  id: string;

  /** 按钮图标（Lucide 图标名称） */
  icon: string;

  /** 按钮标签 */
  label: string;

  /** 按钮提示文字 */
  tooltip?: string;

  /** 按钮分组 */
  group?: string;

  /** 按钮是否禁用 */
  disabled?: boolean | ((context: EditorContext) => boolean);

  /** 按钮是否显示 */
  visible?: boolean | ((context: EditorContext) => boolean);

  /** 按钮点击回调 */
  onClick: (context: EditorContext) => void | Promise<void>;

  /** 按钮排序权重（越小越靠前） */
  order?: number;
}

// ─── 右键菜单项 ─────────────────────────────────────

/**
 * 右键菜单项定义
 */
export interface ContextMenuItem {
  /** 菜单项 ID */
  id: string;

  /** 菜单项图标（可选） */
  icon?: string;

  /** 菜单项标签 */
  label: string;

  /** 菜单项分组 */
  group?: string;

  /** 菜单项是否禁用 */
  disabled?: boolean | ((context: EditorContext) => boolean);

  /** 菜单项是否显示 */
  visible?: boolean | ((context: EditorContext) => boolean);

  /** 菜单项点击回调 */
  onClick: (context: EditorContext) => void | Promise<void>;

  /** 菜单项排序权重（越小越靠前） */
  order?: number;
}

// ─── 键盘快捷键 ───────────────────────────────────────

/**
 * 键盘快捷键定义
 */
export interface KeyboardShortcut {
  /** 快捷键 ID */
  id: string;

  /** 快捷键组合（如 'Ctrl+S'） */
  key: string;

  /** 快捷键描述 */
  description?: string;

  /** 快捷键回调 */
  handler: (context: EditorContext, event: KeyboardEvent) => void | Promise<void>;

  /** 是否阻止默认行为（默认 true） */
  preventDefault?: boolean;

  /** 是否阻止事件冒泡（默认 false） */
  stopPropagation?: boolean;
}

// ─── 编辑器事件系统 ───────────────────────────────────

/**
 * 编辑器事件类型
 */
export type EditorEventType =
  | "contentChange"
  | "selectionChange"
  | "focus"
  | "blur"
  | "init"
  | "destroy"
  | "save"
  | "toolbarAction";

/**
 * 编辑器事件回调
 */
export type EditorEventCallback = (event: EditorEvent) => void;

/**
 * 编辑器事件
 */
export interface EditorEvent {
  /** 事件类型 */
  type: EditorEventType;

  /** 事件来源编辑器 ID */
  editorId: string;

  /** 事件时间戳 */
  timestamp: number;

  /** 事件数据（根据类型不同而不同） */
  data?: any;
}

/**
 * 编辑器事件监听器
 */
export interface EditorEventListener {
  /** 事件类型 */
  type: EditorEventType;

  /** 回调函数 */
  callback: EditorEventCallback;

  /** 监听器 ID（用于注销） */
  id: string;
}

// ─── 编辑器 Provider 组件 props ─────────────────────

/**
 * 编辑器 Provider 组件 props
 */
export interface EditorProviderProps {
  /** 编辑器实例 */
  editor: EditorCore;

  /** 子组件 */
  children: React.ReactNode;

  /** 笔记 ID（可选） */
  noteId?: number;

  /** 是否只读（默认 false） */
  readOnly?: boolean;

  /** 初始内容 */
  initialContent?: string;

  /** 内容变更回调 */
  onContentChange?: (content: string) => void;

  /** 保存回调 */
  onSave?: (content: string) => void;
}

// ─── 编辑器注册表 ─────────────────────────────────────

/**
 * 编辑器注册表项
 */
export interface EditorRegistryEntry {
  /** 编辑器唯一 ID */
  id: string;

  /** 编辑器名称 */
  name: string;

  /** 编辑器描述 */
  description?: string;

  /** 编辑器版本 */
  version?: string;

  /** 支持的文件类型 */
  supportedFileTypes: string[];

  /** 支持的功能特性 */
  features: string[];

  /** 编辑器工厂函数 */
  factory: (config: EditorConfig) => EditorCore;

  /** 默认配置 */
  defaultConfig?: EditorConfig;
}

/**
 * 编辑器配置
 */
export interface EditorConfig {
  /** 编辑器 ID */
  id?: string;

  /** 是否启用拼写检查 */
  spellcheck?: boolean;

  /** 是否启用语法高亮 */
  syntaxHighlight?: boolean;

  /** 是否自动保存 */
  autoSave?: boolean;

  /** 自动保存间隔（毫秒） */
  autoSaveInterval?: number;

  /** 占位符文字 */
  placeholder?: string;

  /** 主题（light/dark） */
  theme?: "light" | "dark";

  /** 额外配置项 */
  [key: string]: any;
}

/**
 * 编辑器注册表
 */
export interface EditorRegistry {
  /** 注册编辑器 */
  register(editor: EditorRegistryEntry): void;

  /** 注销编辑器 */
  unregister(id: string): void;

  /** 获取已注册的编辑器列表 */
  getEditors(): EditorRegistryEntry[];

  /** 根据 ID 获取编辑器 */
  getEditor(id: string): EditorRegistryEntry | undefined;

  /** 根据文件类型获取合适的编辑器 */
  getEditorForFileType(fileType: string): EditorRegistryEntry | undefined;

  /** 根据功能特性获取编辑器列表 */
  getEditorsByFeature(feature: string): EditorRegistryEntry[];
}

// ─── 编辑器插件接口 ───────────────────────────────────

/**
 * 编辑器插件接口（与现有插件系统集成）
 */
export interface EditorPlugin {
  /** 插件 ID */
  id: string;

  /** 插件名称 */
  name: string;

  /** 插件描述 */
  description?: string;

  /** 插件版本 */
  version?: string;

  /** 提供的编辑器 */
  providesEditor?: {
    /** 编辑器组件类型 */
    component: React.ComponentType<any>;

    /** 支持的文件类型 */
    fileTypes: string[];

    /** 支持的功能特性 */
    features: string[];
  };

  /** 提供的编辑器扩展 */
  providesExtensions?: EditorExtension[];
}

// ─── 编辑器钩子返回值 ─────────────────────────────────

/**
 * useEditor 钩子返回值
 */
export interface UseEditorReturn {
  /** 编辑器实例 */
  editor: EditorCore | null;

  /** 编辑器状态 */
  state: EditorState;

  /** 编辑器上下文 */
  context: EditorContext | null;

  /** 更新编辑器状态 */
  setState: (newState: Partial<EditorState>) => void;

  /** 获取内容 */
  getContent: () => string;

  /** 设置内容 */
  setContent: (content: string) => void;

  /** 发送事件 */
  emit: (event: EditorEvent) => void;

  /** 监听事件 */
  on: (type: EditorEventType, callback: EditorEventCallback) => () => void;

  /** 注册扩展 */
  registerExtension: (extension: EditorExtension) => () => void;
}

// ─── 工具类型 ───────────────────────────────────────

/**
 * 编辑器功能特性常量
 */
export const EDITOR_FEATURES = {
  RICH_TEXT: "rich-text",
  MARKDOWN: "markdown",
  CODE: "code",
  OUTLINE: "outline",
  COLLABORATION: "collaboration",
  COMMENTS: "comments",
  HISTORY: "history",
  SPELLCHECK: "spellcheck",
  SYNTAX_HIGHLIGHT: "syntax-highlight",
  MATH: "math",
  DIAGRAMS: "diagrams",
  IMAGES: "images",
  VIDEOS: "videos",
  TABLES: "tables",
  LINKS: "links",
  TASKS: "tasks",
} as const;

export type EditorFeature = typeof EDITOR_FEATURES[keyof typeof EDITOR_FEATURES];
