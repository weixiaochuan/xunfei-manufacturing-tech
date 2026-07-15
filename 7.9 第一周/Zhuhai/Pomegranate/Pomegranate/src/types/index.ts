/** 应用配置 */
export interface AppConfig {
  key: string;
  value: string;
}

export interface PptMasterCheckInput {
  pptMasterRoot: string;
  pythonPath: string;
}

export interface PptMasterCheckResult {
  ok: boolean;
  scriptPath: string;
  pythonVersion: string | null;
  errors: string[];
}

export interface PptMasterExportInput {
  pptMasterRoot: string;
  pythonPath: string;
  projectPath: string;
}

export interface PptMasterExportResult {
  success: boolean;
  outputPath: string | null;
  exitCode: number | null;
  stdout: string;
  stderr: string;
  durationMs: number;
  error: string | null;
}

export interface PptUnderstandingDraft {
  understandingSummary: string;
  keyPriorities: string;
  narrativeMainline: string;
  suggestedPageStructure: string;
  visualExpressionAdvice: string;
  openQuestions: string;
}

export type PptMaterialSourceType = "document" | "diary";

export interface PptMaterialSourceRef {
  id: number;
  sourceType: PptMaterialSourceType;
  title: string;
  updatedAt?: string;
  wordCount?: number;
  folderId?: number | null;
  dailyDate?: string | null;
  hasUnsavedChanges?: boolean;
}

export interface ResolvedPptMaterialSource extends PptMaterialSourceRef {
  plainText: string;
}

export interface PptMaterialSelection {
  sources: ResolvedPptMaterialSource[];
  mergedText: string;
  totalCharacters: number;
}

export type PptMaterialProcessingMode = "direct" | "chunked" | null;

export type PptMaterialAnalysisStatus =
  | "idle"
  | "planning"
  | "analyzing"
  | "merging"
  | "success"
  | "error";

export type PptMaterialAnalysisStage = "planning" | "analyzing" | "merging";

export interface PptMaterialAnalysisProgress {
  current: number;
  total: number;
  stage: PptMaterialAnalysisStage;
}

export interface PptMaterialChunk {
  id: string;
  index: number;
  total: number;
  text: string;
  sourceTitles: string[];
  headingContext: string[];
  startCharacter: number;
  endCharacter: number;
  estimatedTokens: number;
}

export interface PptMaterialChunkPlan {
  chunks: PptMaterialChunk[];
  totalCharacters: number;
  totalEstimatedTokens: number;
  chunkTokenBudget: number;
  promptOverheadTokens: number;
  metadataReserveTokens: number;
  outputReserveTokens: number;
  modelMaxContextTokens: number;
}

export interface PptChunkUnderstandingDraft extends PptUnderstandingDraft {
  chunkId: string;
  chunkIndex: number;
}

export interface PptMaterialSourceMetadata {
  id: number;
  sourceType: PptMaterialSourceType;
  title: string;
}

export interface PptMasterGenerateInput {
  pptMasterRoot: string;
  pythonPath: string;
  /** @deprecated Compatibility input for older backends. Structured understanding is authoritative. */
  prompt?: string | null;
  planningContext?: string | null;
  aiUnderstandingResult?: PptUnderstandingDraft | string | null;
  understandingSummary?: string | null;
  keyPriorities?: string | null;
  suggestedPageStructure?: string | null;
  narrativeMainline?: string | null;
  visualExpressionAdvice?: string | null;
  openQuestions?: string | null;
  rawMaterial?: string | null;
  materialSources?: PptMaterialSourceMetadata[];
  extraRequirements?: string | null;
  modelId?: number | null;
  title?: string;
  audience?: string;
  slideCount?: number;
  style?: string;
  generationEngine?: "ppt_master_native" | "legacy_fallback";
  mode?: string;
  visualStyle?: string;
  layoutBias?: string[];
  chartBias?: string[];
  outputDir?: string | null;
  generationMode?: "agent" | "template";
}

export interface PptMasterGenerateResult {
  success: boolean;
  projectPath: string | null;
  pptxPath: string | null;
  finalPptxPath: string | null;
  slidePlanPath: string | null;
  designSpecPath: string | null;
  qualityCheckPassed: boolean | null;
  generationMode: string;
  exitCode: number | null;
  stdout: string;
  stderr: string;
  durationMs: number;
  error: string | null;
  generationEngine?: string;
}

// ─── 插件系统 ───────────────────────────────────

export interface PluginCommandContribution {
  id: string;
  title: string;
}

export interface PluginViewContribution {
  id: string;
  title: string;
}

export interface PluginContributes {
  commands: PluginCommandContribution[];
  sidebarViews: PluginViewContribution[];
  settings: boolean;
}

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description: string | null;
  author: string | null;
  main: string;
  styles: string | null;
  minAppVersion: string | null;
  permissions: string[];
  contributes: PluginContributes;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string | null;
  author: string | null;
  path: string;
  main: string;
  styles: string | null;
  minAppVersion: string | null;
  enabled: boolean;
  status: string;
  permissions: string[];
  grantedPermissions: string[];
  manifest: PluginManifest;
  installedAt: string;
  updatedAt: string;
}

/** 插件审计日志（T25） */
export interface PluginAuditLogEntry {
  id: number;
  pluginId: string;
  operation: string;
  target: string | null;
  timestamp: string;
}

// ═══════════════════════════════════════════════════════
// 插件运行时 — AppAPI v1.0.0（T11: TypeScript 类型草案）
// ═══════════════════════════════════════════════════════

/** AppAPI 版本号（与 Rust 侧固定字符串保持一致） */
export const PLUGIN_API_VERSION = "1.2.0" as const;

/** 插件暴露的模块约定：module.exports = { onLoad, onUnload } */
export interface PluginModule {
  onLoad?: (ctx: PluginContext) => void | Promise<void>;
  onUnload?: (ctx: PluginContext) => void | Promise<void>;
}

/**
 * 插件 onLoad / onUnload 回调的上下文
 *
 * v1.0.0: 移除顶层 register* 方法，统一通过 ctx.app.* 子 API 操作。
 */
export interface PluginContext {
  app: PluginAppAPI;
  meta: Readonly<PluginMeta>;
}

/** 插件元信息（精简版） */
export interface PluginMeta {
  id: string;
  name: string;
  version: string;
  dir: string;
}

/** 插件命令面板项 */
export interface PluginCommandDef {
  /** 命令 ID（不含 pluginId 前缀，PluginManager 自动加） */
  id: string;
  name: string;
  group?: string;
  hotkey?: string;
  callback: () => void | Promise<void>;
}

/** 插件侧边栏条目 */
export interface PluginSidebarItemDef {
  id: string;
  icon: string;
  label: string;
  viewId?: string;
  onClick?: () => void;
  group?: "ai" | "workflow" | "search" | "bottom";
  /** PluginManager 填充，插件不应设置 */
  pluginId?: string;
}

/** 插件面板视图 */
export interface PluginPanelViewDef {
  id: string;
  title: string;
  layout?: "default" | "fullscreen";
  /** 返回 cleanup 函数（可选） */
  render: (container: HTMLElement) => void | (() => void);
  /** PluginManager 填充，插件不应设置 */
  pluginId?: string;
}

/**
 * 插件功能区（Ribbon）按钮 — 1.1.0 新增
 *
 * 渲染在主窗 Header 右侧，常用于"全局快捷动作"（例如"快速捕获"、"导出当前笔记"）。
 * 与 sidebar 的区别：Ribbon 是动作触发器（点完就走），不持有视图路由。
 */
export interface PluginRibbonItemDef {
  id: string;
  /** Lucide 图标名（不区分大小写、忽略 -_ 空格） */
  icon: string;
  tooltip: string;
  onClick: () => void | Promise<void>;
  /** PluginManager 填充，插件不应设置 */
  pluginId?: string;
}

/**
 * 编辑器右键菜单项 — 1.1.0 新增
 *
 * 触发：用户在 Tiptap 编辑器内右键时检查注册表，按 `when` 过滤后追加到菜单末尾。
 *
 * `when` 决定显示条件：
 * - `"always"`        - 任何位置右键都显示（默认）
 * - `"has-selection"` - 仅当有非空选区时显示
 * - `"cursor"`        - 仅当光标无选区时显示
 *
 * 触发 callback 时会传入 EditorActionCtx，提供对当前编辑器的受控操作（不暴露 Tiptap editor 实例）。
 */
export interface PluginEditorContextMenuItemDef {
  id: string;
  label: string;
  /** Lucide 图标名（可选） */
  icon?: string;
  when?: "always" | "has-selection" | "cursor";
  callback: (ctx: EditorActionCtx) => void | Promise<void>;
  /** PluginManager 填充，插件不应设置 */
  pluginId?: string;
}

/**
 * 编辑器动作上下文 — 1.1.0 新增
 *
 * 传给插件 callback 的受控接口。不暴露 Tiptap editor 实例，避免插件直接调内部命令。
 */
export interface EditorActionCtx {
  /** 当前笔记 ID；新建笔记 / 未保存时为 null */
  noteId: number | null;
  /** 当前选中的纯文本（无选区时为空字符串） */
  selection: string;
  /** 用选中文本替换；无选区时插入到光标位置 */
  replaceSelection(text: string): void;
  /** 在光标位置插入文本（保留选区，不删除） */
  insertText(text: string): void;
  /** 取整篇内容为 markdown 文本（只读） */
  getContent(): string;
}

// ─── 子 API 接口 ─────────────────────────────────

/** 命令注册子 API */
export interface PluginCommandsAPI {
  addCommand(def: PluginCommandDef): () => void;
  removeCommand(id: string): void;
  executeCommand(fullId: string): void | Promise<void>;
}

/** 侧边栏注册子 API */
export interface PluginSidebarAPI {
  addItem(def: PluginSidebarItemDef): void;
  removeItem(id: string): void;
}

/** 面板视图注册子 API */
export interface PluginPanelViewsAPI {
  register(def: PluginPanelViewDef): void;
  unregister(id: string): void;
}

/** Ribbon 功能区注册子 API — 1.1.0 新增 */
export interface PluginRibbonAPI {
  addItem(def: PluginRibbonItemDef): void;
  removeItem(id: string): void;
}

/**
 * 编辑器工具栏按钮 — 1.1.0 新增
 *
 * 渲染在编辑器顶部工具栏末尾，与内置按钮以细分隔线隔开。
 * callback 收到 EditorActionCtx，与右键菜单项共用同一受控接口。
 */
export interface PluginEditorToolbarButtonDef {
  id: string;
  /** Lucide 图标名 */
  icon: string;
  tooltip: string;
  callback: (ctx: EditorActionCtx) => void | Promise<void>;
  /** PluginManager 填充，插件不应设置 */
  pluginId?: string;
}

/** 编辑器扩展点子 API — 1.1.0 新增 */
export interface PluginEditorAPI {
  addContextMenuItem(def: PluginEditorContextMenuItemDef): void;
  removeContextMenuItem(id: string): void;
  addToolbarButton(def: PluginEditorToolbarButtonDef): void;
  removeToolbarButton(id: string): void;
  /**
   * 同步查询当前激活编辑器的选区文本。
   * 返回 null = 当前没有激活的编辑器（用户不在笔记页 / 编辑器未就绪）。
   * 返回空字符串 = 编辑器存在但无选区。
   */
  getCurrentSelection(): string | null;
}

/** 插件任务 CRUD API */
export interface PluginTasksAPI {
  list(filter?: PluginTaskFilter): Promise<PluginTaskView[]>;
  get(id: number): Promise<PluginTaskView | null>;
  create(input: PluginCreateTaskInput): Promise<PluginTaskView>;
  update(id: number, patch: PluginUpdateTaskInput): Promise<void>;
  complete(id: number): Promise<void>;
  delete(id: number): Promise<void>;
}

/** 插件任务视图注册 API */
export interface PluginTaskViewsAPI {
  register(def: {
    id: string;
    label: string;
    icon: string;
    render: (container: HTMLElement, props: TaskViewProps) => void | (() => void);
  }): () => void;
}

/** 插件事件类型清单 */
export interface PluginEvents {
  "workspace:active-note-changed": { noteId: number | null };
  "workspace:editor-saved": { noteId: number };
  "settings:changed": { key: string };
  // ─── 待办事件（阶段 1 事件总线）─────────────
  "task:created": Task;
  "task:updated": Task;
  "task:completed": Task;
  "task:deleted": Task;
  "task:reminded": Task;
}

/** 插件事件总线 */
export interface PluginEventBus {
  on<K extends keyof PluginEvents>(
    event: K,
    handler: (data: PluginEvents[K]) => void,
  ): () => void;
  emit<K extends keyof PluginEvents>(event: K, data: PluginEvents[K]): void;
}

export interface PluginAiMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface PluginAiChatCallbacks {
  onToken?: (token: string, fullText: string) => void;
  onDone?: (fullText: string) => void;
  onError?: (error: string) => void;
}

export interface PluginAiChatOptions {
  modelId?: number;
  conversationId?: number;
}

export interface PluginAiModelInfo {
  id: number;
  name: string;
  provider: string;
  protocol: string;
  apiUrl: string;
  modelId: string;
  isDefault: boolean;
  maxContext: number;
  supportsTools: boolean;
  supportsVision: boolean;
  maxOutputTokens: number | null;
}

export interface PluginAiTokenPayload {
  token: string;
  fullText: string | null;
  done: boolean;
  error: string | null;
}

export interface PluginAiAPI {
  chat(
    messages: PluginAiMessage[],
    callbacks: PluginAiChatCallbacks,
    options?: PluginAiChatOptions,
  ): Promise<() => void>;
  chatSync(messages: PluginAiMessage[], options?: PluginAiChatOptions): Promise<string>;
  listModels(): Promise<PluginAiModelInfo[]>;
}

/** 工作区子 API */
export interface PluginWorkspaceAPI {
  getActiveNoteId(): number | null;
  getActiveNote(): Promise<Note | null>;
}

/** 通知子 API */
export interface PluginNoticesAPI {
  show(message: string, duration?: number): void;
  error(message: string): void;
}

/** 设置子 API */
export interface PluginSettingsTabDef {
  id: string;
  title: string;
  /** 返回 cleanup 函数（可选） */
  render: (container: HTMLElement) => void | (() => void);
  /** PluginManager 填充，插件不应设置 */
  pluginId?: string;
}

/** 设置表单字段 schema（createForm 用） — 1.1.0 新增 */
export type SettingsFormField =
  | {
      kind: "text";
      key: string;
      label: string;
      default?: string;
      placeholder?: string;
    }
  | {
      kind: "number";
      key: string;
      label: string;
      default?: number;
      min?: number;
      max?: number;
    }
  | {
      kind: "boolean";
      key: string;
      label: string;
      default?: boolean;
    }
  | {
      kind: "select";
      key: string;
      label: string;
      options: Array<{ value: string; label: string }>;
      default?: string;
    };

export interface PluginSettingsFormSchema {
  fields: SettingsFormField[];
}

export interface PluginSettingsAPI {
  get<T = unknown>(key: string): Promise<T | undefined>;
  set(key: string, value: unknown): Promise<void>;
  getAll(): Promise<Record<string, unknown>>;
  /** 在应用设置页注册一个独立 Tab — 1.1.0 新增 */
  registerTab(def: PluginSettingsTabDef): void;
  unregisterTab(id: string): void;
  /**
   * 基于 schema 自动生成 antd Form，自动 get/set 到插件自身设置。
   * 返回 cleanup 函数（调用即卸载 React 树 + 清理 DOM）。
   * — 1.1.0 新增
   */
  createForm(container: HTMLElement, schema: PluginSettingsFormSchema): () => void;
}

/** 笔记子 API */
export interface PluginNotesAPI {
  list(query?: NoteQuery): Promise<Note[]>;
  get(id: number): Promise<Note | null>;
  search(keyword: string, limit?: number): Promise<SearchResult[]>;
  create(input: NoteInput): Promise<Note>;
  update(id: number, input: NoteInput): Promise<Note>;
  delete(id: number): Promise<void>;
}

/** AppAPI v1.0.0 */
export interface PluginAppAPI {
  readonly version: typeof PLUGIN_API_VERSION;
  workspace: PluginWorkspaceAPI;
  notes: PluginNotesAPI;
  tasks: PluginTasksAPI;
  taskViews: PluginTaskViewsAPI;
  settings: PluginSettingsAPI;
  notices: PluginNoticesAPI;
  commands: PluginCommandsAPI;
  sidebar: PluginSidebarAPI;
  panelViews: PluginPanelViewsAPI;
  events: PluginEventBus;
  /** Ribbon 功能区 — 1.1.0 新增 */
  ribbon: PluginRibbonAPI;
  /** 编辑器扩展点 — 1.1.0 新增 */
  editor: PluginEditorAPI;
  /** AI 对话能力 — 1.2.0 新增 */
  ai: PluginAiAPI;
  /**
   * 通用 invoke 桥接（Phase 1 轻量方案，供插件调用现有 Rust Command）。
   * 直接透传 Tauri invoke，不做令牌/权限校验。
   */
  invoke: <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  /**
   * 监听 Tauri 全局事件（Phase 1 轻量方案）。
   * 返回取消监听函数。
   */
  onTauriEvent: <T = unknown>(event: string, handler: (payload: T) => void) => Promise<() => void>;
}

// ═══════════════════════════════════════════════════════
// 旧版兼容类型（T4 完成后可移除）
// ═══════════════════════════════════════════════════════

/** @deprecated 使用 PluginContext（无 register* 方法） */
export interface PluginContextLegacy {
  app: PluginAppAPI;
  meta: Readonly<PluginMeta>;
  registerCommand?: (cmd: PluginCommandDef) => void;
  registerSidebarItem?: (item: PluginSidebarItemDef) => void;
  registerPanelView?: (def: PluginPanelViewDef) => void;
  registerSettings?: (render: () => void) => void;
}

/** 全局快捷键绑定（仅 global scope，由 Rust 侧 global-shortcut 插件管理） */
export interface ShortcutBinding {
  /** 内部唯一 ID（如 'global.quickCapture'） */
  id: string;
  /** 当前生效的 accelerator；空字符串 = 已禁用 */
  accel: string;
  /** 默认 accelerator */
  defaultAccel: string;
  /** 用户已改键 / 已禁用 */
  isCustom: boolean;
  /** 是否被禁用（accel 为空） */
  disabled: boolean;
}

/** 系统信息 */
export interface SystemInfo {
  os: string;
  arch: string;
  appVersion: string;
  /** 当前实例的数据根目录（多开实例 = app_data_dir/instance-N，不是公共 app_data_dir） */
  dataDir: string;
  imagesDir: string;
  /** 多开实例编号；null = 默认实例 */
  instanceId: number | null;
  /** 是否运行在 dev build 下（前端徽章追加 [DEV] 标识） */
  isDev: boolean;
}

// ─── 笔记 ─────────────────────────────────────

/** 笔记 */
export interface Note {
  id: number;
  title: string;
  content: string;
  folder_id: number | null;
  is_daily: boolean;
  daily_date: string | null;
  is_pinned: boolean;
  /** T-003: 是否"隐藏"（主列表/搜索/反链/图谱/RAG 全部过滤；wiki 跳转仍可访问）*/
  is_hidden: boolean;
  /** T-007: 是否加密。content 字段会是占位符，真实内容需调 decryptNote */
  is_encrypted: boolean;
  word_count: number;
  created_at: string;
  updated_at: string;
  /** 关联的原始文件相对路径（相对 app_data_dir），纯笔记为 null */
  source_file_path: string | null;
  /** 原始文件类型："pdf" / "docx" / "doc" / null */
  source_file_type: string | null;
  /** 同 folder 内的自定义排序值，越小越靠前；只在 NoteQuery.sort_by="custom" 时生效 */
  sort_order: number;
}

/** PDF 导入结果（单个文件） */
export interface PdfImportResult {
  sourcePath: string;
  noteId: number | null;
  title: string | null;
  error: string | null;
}

/** .doc 转换器探测结果（serde kebab-case） */
export type DocConverter = "libre-office" | "windows-com" | "none";

/** 单个 ProgId 的实测结果 */
export interface ComProgIdAttempt {
  progid: string;
  ok: boolean;
  error: string | null;
}

/** 转换器完整诊断报告 */
export interface ConverterDiagnostic {
  libreOfficePath: string | null;
  comAttempts: ComProgIdAttempt[];
  active: DocConverter;
}

/** 创建/更新笔记入参 */
export interface NoteInput {
  title: string;
  content: string;
  folder_id?: number | null;
}

/** 笔记列表查询参数 */
export interface NoteQuery {
  folder_id?: number | null;
  keyword?: string | null;
  page?: number;
  page_size?: number;
  /** true 时只返回 folder_id IS NULL 的笔记（"未分类"虚拟文件夹） */
  uncategorized?: boolean;
  /** true 时点父文件夹连同所有子孙文件夹的笔记一起返回（默认 true，符合"文件夹=容器"直觉） */
  include_descendants?: boolean;
  /** 排序模式：默认按 is_pinned DESC, updated_at DESC */
  sort_by?: "default" | "custom" | "created" | "title";
}

// ─── 文件夹 ───────────────────────────────────

/** 文件夹（树形结构） */
export interface Folder {
  id: number;
  name: string;
  parent_id: number | null;
  sort_order: number;
  children: Folder[];
  note_count: number;
}

// ─── 标签 ─────────────────────────────────────

/** 标签 */
export interface Tag {
  id: number;
  name: string;
  color: string | null;
  note_count: number;
}

/** 创建/更新标签入参 */
export interface TagInput {
  name: string;
  color?: string | null;
}

// ─── 搜索 ─────────────────────────────────────

/** 全文搜索结果 */
export interface SearchResult {
  id: number;
  title: string;
  snippet: string;
  updated_at: string;
  folder_id: number | null;
}

// ─── 回收站 ───────────────────────────────────

/** 回收站笔记查询参数 */
export interface TrashQuery {
  page?: number;
  page_size?: number;
}

// ─── 笔记链接 ─────────────────────────────────

/** 反向链接 */
export interface NoteLink {
  source_id: number;
  source_title: string;
  context: string | null;
  updated_at: string;
}

// ─── 知识图谱 ─────────────────────────────────

/** 图谱节点 */
export interface GraphNode {
  id: number;
  title: string;
  is_daily: boolean;
  is_pinned: boolean;
  tag_count: number;
  link_count: number;
}

/** 图谱边 */
export interface GraphEdge {
  source: number;
  target: number;
}

/** 知识图谱数据 */
export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

// ─── AI 知识问答 ─────────────────────────────

/** AI 模型配置 */
export interface AiModel {
  id: number;
  name: string;
  provider: string;
  protocol: string;
  api_url: string;
  api_key: string | null;
  model_id: string;
  is_default: boolean;
  max_context: number;
  supports_tools: boolean;
  supports_vision: boolean;
  max_output_tokens: number | null;
  created_at: string;
}

/** 创建/更新 AI 模型入参 */
export interface AiModelInput {
  name: string;
  provider: string;
  protocol?: string;
  api_url: string;
  api_key?: string | null;
  model_id: string;
  max_context?: number;
  supports_tools?: boolean;
  supports_vision?: boolean;
  max_output_tokens?: number | null;
}

/** AI 模型连通性测试结果 */
export interface AiModelTestResult {
  ok: boolean;
  /** 端到端往返耗时（毫秒） */
  latency_ms: number;
  /** 服务端样本：成功时为模型回复前 40 字 */
  sample: string | null;
}

// ─── AI Provider 注册表 ───────────────────────

/** AI Provider 能力描述 */
export interface AiCapabilities {
  supports_streaming: boolean;
  supports_vision: boolean;
  supports_function_calling: boolean;
  max_output_tokens: number | null;
}

/** AI Provider 元数据（前端选择列表用） */
export interface AiProviderMetadata {
  provider_type: string;
  display_name: string;
  description: string;
  default_base_url: string;
  default_model: string;
  capabilities: AiCapabilities;
}

/** 切换 Provider 入参 */
export interface SwitchProviderInput {
  provider_type: string;
  model_id: number;
}

/** 活跃 Provider 信息（不含 api_key） */
export interface ActiveProviderInfo {
  provider_type: string;
  display_name: string;
  model_id: string;
  capabilities: AiCapabilities;
}

/** AI 对话 */
export interface AiConversation {
  id: number;
  title: string;
  model_id: number;
  /** 附加给本对话的笔记 ID 列表，整个对话共享 */
  attached_note_ids: number[];
  created_at: string;
  updated_at: string;
}

/** AI 消息 */
export interface AiMessage {
  id: number;
  conversation_id: number;
  /** 角色: user / assistant */
  role: string;
  content: string;
  /** 引用的笔记 ID 列表 (JSON 字符串) */
  references: string | null;
  /** T-004: Skills 框架下 AI 调用的工具记录（JSON 字符串，反序列化后是 SkillCall[]）*/
  skill_calls: string | null;
  created_at: string;
}

/** AI Skill 调用记录（T-004）*/
export interface SkillCall {
  id: string;
  name: string;
  /** 参数 JSON 字符串（展示给用户看） */
  argsJson: string;
  /** Skill 执行结果文本（可能是 JSON 或截断的文本） */
  result: string;
  /** 'running' | 'ok' | 'error' */
  status: "running" | "ok" | "error";
}

// ─── AI 规划今日待办（T-005） ────────────

export interface PlanTodayRequest {
  /** 用户输入的"今日目标"，可留空 */
  goal?: string | null;
  /** 是否把昨日未完成/过期任务一起带进 AI 的上下文；默认 true */
  includeYesterdayUnfinished?: boolean;
}

/** AI 对一条待办的建议；尚未写入 DB */
export interface TaskSuggestion {
  title: string;
  /** 0=紧急 / 1=普通 / 2=低 */
  priority?: number | null;
  important?: boolean | null;
  /** 'YYYY-MM-DD' 或 'YYYY-MM-DD HH:MM:SS' */
  dueDate?: string | null;
  /** AI 自动判断的提醒时间（分钟）：null=不提醒；0=准时；正数=提前 N 分钟。
   *  推荐值：null / 0 / 15 / 30 / 60 / 180 / 1440 / 10080 */
  remindBefore?: number | null;
  /** AI 给出的推荐理由，UI 折叠展示 */
  reason?: string | null;
}

export interface PlanTodayResponse {
  tasks: TaskSuggestion[];
  summary?: string | null;
}

// ─── AI 智能规划（目标驱动）────────────

export interface PlanFromGoalRequest {
  /** 用户描述的目标，例如"180 天减肥到 55 公斤" */
  goal: string;
  /** 计划周期天数，默认 30，最大 365 */
  horizonDays?: number;
  /** 起始日期 'YYYY-MM-DD'；缺省=今天 */
  startDate?: string | null;
  /** 用户额外补充信息（作息/兴趣/约束） */
  profileHint?: string | null;
}

export interface MilestoneDraft {
  title: string;
  /** 自然语言日期范围，如 "5月1日-5月31日" */
  dateRange?: string | null;
  description?: string | null;
}

export interface PlanFromGoalResponse {
  tasks: TaskSuggestion[];
  milestones: MilestoneDraft[];
  summary?: string | null;
  /** 服务端生成的批次 ID，前端落库时每条任务都要透传到 source_batch_id */
  batchId: string;
  /** 服务端友好警告（如 Excel 体积大被截断）；UI 在预览页顶部展示 */
  warnings?: string[];
}

export interface PlanFromExcelRequest {
  /** Excel/ODS 文件绝对路径（来自 Tauri dialog） */
  filePath: string;
  /** 计划周期天数，默认 30，最大 365 */
  horizonDays?: number;
  /** 起始日期 'YYYY-MM-DD'；缺省=今天 */
  startDate?: string | null;
  /** 用户对 Excel 内容的额外说明（可选），如"重点关注健身部分" */
  extraGoal?: string | null;
}

// ─── AI 会话附件（路线 A） ────────────

/** Excel/ODS 附件解析预览。 */
export interface ExcelPreview {
  filePath: string;
  displayName: string;
  /** calamine 解析后的多 sheet markdown 全文 */
  markdown: string;
  totalRows: number;
  /** 因体积过大被截断的 sheet 名（空数组=未截断） */
  truncatedSheets: string[];
  /** markdown 字符数（用于 UI 提示"占用约 12k 上下文"） */
  charsEstimated: number;
}

/** 文本类附件（md / txt / json / 代码等）解析预览。 */
export interface TextPreview {
  filePath: string;
  displayName: string;
  content: string;
  totalLines: number;
  charsEstimated: number;
  /** 单文件超 60k 字符时尾部被截断 */
  truncated: boolean;
}

/** PDF 附件解析预览（仅文字层抽取，扫描件会报错）。 */
export interface PdfPreview {
  filePath: string;
  displayName: string;
  content: string;
  charsEstimated: number;
  truncated: boolean;
}

/**
 * 统一附件预览：后端按扩展名自动分发到 Excel/Text/PDF 解析器返回。
 * tagged union 用 kind 区分，对应后端 AttachmentPreview enum。
 */
export type AttachmentPreview =
  | ({ kind: "excel" } & ExcelPreview)
  | ({ kind: "text" } & TextPreview)
  | ({ kind: "pdf" } & PdfPreview);

/**
 * 发送给 AI 的消息附件。每种 kind 字段不同（参考后端 MessageAttachment）。
 */
export type MessageAttachment =
  | {
      kind: "excel";
      filePath: string;
      displayName: string;
      markdown: string;
      totalRows: number;
      truncatedSheets: string[];
    }
  | {
      kind: "text";
      filePath: string;
      displayName: string;
      content: string;
      truncated: boolean;
    }
  | {
      kind: "pdf";
      filePath: string;
      displayName: string;
      content: string;
      truncated: boolean;
    };

// ─── AI 写笔记并归档（T-006） ────────────

export type TargetLength = "short" | "medium" | "long";

export interface DraftNoteRequest {
  topic: string;
  reference?: string | null;
  /** short=100~300 / medium=300~800 / long=800~2000 */
  targetLength?: TargetLength;
}

/** AI 生成的笔记草稿（未落库，Modal 里用户确认后才写入）*/
export interface DraftNoteResponse {
  title: string;
  /** Markdown 正文 */
  content: string;
  /** 建议目录路径，如 "工作/周报"；空串 = 根目录 */
  folderPath: string;
  reason?: string | null;
}

// ─── T-007 笔记加密 / Vault ─────────────────────

/** Vault 整体状态（三态机）*/
export type VaultStatus = "notset" | "locked" | "unlocked";

// ─── 导入 ─────────────────────────────────────

/** 扫描到的文件在库中的匹配类型（后端扫描时判定） */
export type ImportMatchKind = "new" | "path" | "fuzzy";

/** 扫描到的文件条目 */
export interface ScannedFile {
  path: string;
  /** 相对扫描根的父目录，斜杠统一；根层文件为空串 */
  relative_dir: string;
  name: string;
  size: number;
  /**
   * 去重匹配结果：
   * - "new"   全新文件
   * - "path"  按 source_file_path 精确命中（已导入过）
   * - "fuzzy" 按 (title, content_hash) 兜底命中（用户可能搬动过源文件）
   */
  match_kind: ImportMatchKind;
  /** match_kind 非 "new" 时，指向已存在笔记的 id */
  existing_note_id: number | null;
}

/**
 * 导入冲突策略：遇到已存在的文件怎么处理
 * - "skip"      跳过（默认，最安全）
 * - "duplicate" 创建副本（标题加 " (2)"）
 */
export type ImportConflictPolicy = "skip" | "duplicate";

/** 导入结果 */
export interface ImportResult {
  imported: number;
  skipped: number;
  duplicated: number;
  errors: string[];
  /** T-009: 自动关联的 frontmatter tag 条数（笔记 × 标签 笛卡尔次数） */
  tags_attached?: number;
  /** T-009: 解析到 frontmatter 的笔记数 */
  frontmatter_parsed?: number;
  /** T-009 Commit 2: 复制到 kb_assets/images 的图片张数 */
  attachments_copied?: number;
  /** T-009 Commit 2: 缺失的图片清单（"笔记标题: 原始引用"，已去重） */
  attachments_missing?: string[];
  /** 本次新建的笔记 ID（按文件参数顺序，含 Duplicate 副本） */
  noteIds?: number[];
  /** 命中已有笔记并按 Skip 策略跳过时记录的现有笔记 ID */
  existingNoteIds?: number[];
}

/** 导入进度 */
export interface ImportProgress {
  current: number;
  total: number;
  file_name: string;
}

/** "打开单个 md 文件"返回结果 */
export interface OpenMarkdownResult {
  noteId: number;
  /** 检测到源文件有变化并已同步回笔记 */
  wasSynced: boolean;
}

/** 孤儿素材类型 */
export type OrphanKind = "image" | "video" | "attachment" | "pdf" | "source";

/** 孤儿原因 */
export type OrphanReason = "notePurged" | "unreferenced";

/** 单条孤儿素材 */
export interface OrphanItem {
  kind: OrphanKind;
  /** 绝对路径 */
  path: string;
  /** 按 note_id 分目录的素材有；纯平铺的 images 没有 */
  noteId: number | null;
  size: number;
  reason: OrphanReason;
}

/** 单类素材的孤儿组（UI Tab 内） */
export interface OrphanGroup {
  count: number;
  totalBytes: number;
  /** 实际明细（最多 500 条）*/
  items: OrphanItem[];
  truncated: boolean;
}

/** 全量孤儿扫描结果 */
export interface OrphanAssetScan {
  images: OrphanGroup;
  videos: OrphanGroup;
  attachments: OrphanGroup;
  pdfs: OrphanGroup;
  sources: OrphanGroup;
}

/** 孤儿素材清理结果 */
export interface OrphanAssetClean {
  deleted: number;
  freedBytes: number;
  failed: string[];
}

/** 附件信息（save_note_attachment 返回） */
export interface AttachmentInfo {
  /** 绝对路径（前端用来构造 file:// 链接给 opener 打开） */
  path: string;
  /** 原始文件名（展示用） */
  fileName: string;
  /** 字节数（用于显示 "1.2 MB"） */
  size: number;
  /** MIME 类型 */
  mime: string;
}

// ─── 外部 .md 双向同步（写回原文件） ─────────────

/**
 * 笔记保存后写回原 .md 文件的结果。
 *
 * - `ok`：写回成功；`assets_copied` = 用户在编辑器里新插入的图被复制到 `<basename>.assets/` 的张数
 * - `skipped`：笔记不是从外部 .md 打开的，没什么可写回（默认情况）
 * - `conflict`：原文件 mtime 与上次写回时不一致 = 外部编辑器（VSCode 等）改过文件，
 *   前端应弹 Modal 让用户选「覆盖外部 / 取消」（"覆盖外部" 后端再调一次 `force=true`）
 * - `missing`：原文件被删/移走/挂网盘断开
 */
export type WriteBackResult =
  | { kind: "ok"; assets_copied: number; file_path: string }
  | { kind: "skipped"; reason: string }
  | { kind: "conflict"; external_mtime: number; last_known_mtime: number | null; file_path: string }
  | { kind: "missing"; file_path: string };

// ─── 导出 ─────────────────────────────────────

/** 导出结果 */
export interface ExportResult {
  exported: number;
  errors: string[];
  /** 用户选择的父目录 */
  output_dir: string;
  /** 实际创建的导出根目录（在 output_dir 下自动包一层 Pomegranate导出_YYYYMMDD_HHmmss） */
  root_dir: string;
  /** 拷贝到 .assets/ 的资产文件总数（图片+附件，按物理文件去重） */
  assets_copied: number;
}

/** 单篇导出结果 */
export interface SingleExportResult {
  /** 实际创建的笔记根目录（含 .md 与 assets/） */
  root_dir: string;
  /** .md 文件绝对路径 */
  file_path: string;
  /** 拷贝到 assets/ 的资产文件数 */
  assets_copied: number;
}

/** T-020 Word 导出结果 */
export interface WordExportResult {
  filePath: string;
  imagesEmbedded: number;
  imagesMissing: number;
}

/** T-020 HTML 导出结果（单文件，图片内嵌） */
export interface HtmlExportResult {
  filePath: string;
  imagesInlined: number;
  imagesMissing: number;
}

/** 导出进度 */
export interface ExportProgress {
  current: number;
  total: number;
  file_name: string;
}

// ─── 首页统计 ─────────────────────────────────

/** 首页统计数据 */
export interface DashboardStats {
  total_notes: number;
  total_folders: number;
  total_tags: number;
  total_links: number;
  today_updated: number;
  total_words: number;
}

// ─── 写作趋势 ─────────────────────────────────

/** 每日写作统计 */
export interface DailyWritingStat {
  date: string;
  note_count: number;
  word_count: number;
}

// ─── 笔记模板 ─────────────────────────────────

/** 笔记模板 */
export interface NoteTemplate {
  id: number;
  name: string;
  description: string;
  content: string;
  created_at: string;
}

/** 创建/更新模板入参 */
export interface NoteTemplateInput {
  name: string;
  description: string;
  content: string;
}

// ─── 通用 ─────────────────────────────────────

/** 分页响应 */
export interface PageResult<T> {
  items: T[];
  total: number;
  page: number;
  page_size: number;
}

/**
 * 批量恢复笔记的结果（与后端 RestoreBatchResult 对齐，serde camelCase）
 *
 * `toRoot` = 其中因原文件夹已被删除而落到根目录的条数
 */
export interface RestoreBatchResult {
  restored: number;
  toRoot: number;
}

// ─── 同步 ─────────────────────────────────────

/** 同步范围：控制本次同步包含哪些数据 */
export interface SyncScope {
  notes: boolean;
  images: boolean;
  pdfs: boolean;
  sources: boolean;
  settings: boolean;
}

export const DEFAULT_SYNC_SCOPE: SyncScope = {
  notes: true,
  images: true,
  pdfs: true,
  sources: true,
  settings: true,
};

/** 导入模式 */
export type SyncImportMode = "merge" | "overwrite";

/** WebDAV 配置 */
export interface WebDavConfig {
  url: string;
  username: string;
  /** 前端传入时使用；后端读取时从 keyring 取 */
  password?: string;
}

/** 同步数据统计 */
export interface SyncStats {
  notesCount: number;
  foldersCount: number;
  tagsCount: number;
  imagesCount: number;
  pdfsCount: number;
  sourcesCount: number;
  /** 资产总大小（字节）*/
  assetsSize: number;
}

/** 云端快照条目（多设备场景，一台一个 kb-sync-<device>.zip） */
export interface RemoteSnapshot {
  filename: string;
  device: string;
}

/** 云端 manifest（快照元信息） */
export interface SyncManifest {
  schemaVersion: number;
  device: string;
  exportedAt: string;
  appVersion: string;
  scope: SyncScope;
  stats: SyncStats;
}

/** 同步操作结果 */
export interface SyncResult {
  stats: SyncStats;
  finishedAt: string;
}

/** 同步历史记录 */
export interface SyncHistoryItem {
  id: number;
  direction: string;
  startedAt: string;
  finishedAt: string | null;
  success: boolean;
  error: string | null;
  statsJson: string;
}

// ─── 待办任务 ───────────────────────────────────

export type TaskPriority = 0 | 1 | 2; // 0=urgent / 1=normal / 2=low
export type TaskStatus = 0 | 1;       // 0=todo / 1=done
export type TaskLinkKind = "note" | "path" | "url";
/** 循环规则：不循环 / 每天 / 每周 / 每月 */
export type TaskRepeatKind = "none" | "daily" | "weekly" | "monthly";

export interface TaskLink {
  id: number;
  task_id: number;
  kind: TaskLinkKind;
  target: string;
  label: string | null;
}

export interface Task {
  id: number;
  title: string;
  description: string | null;
  priority: TaskPriority;
  important: boolean;
  status: TaskStatus;
  /** 'YYYY-MM-DD' 或 'YYYY-MM-DD HH:MM:SS'；前者视作当天 23:59:59 */
  due_date: string | null;
  completed_at: string | null;
  created_at: string;
  updated_at: string;
  /** 提前 N 分钟提醒；null=不提醒 */
  remind_before_minutes: number | null;
  /** 上次已触发提醒的时刻，去重用 */
  reminded_at: string | null;
  /** 循环规则 */
  repeat_kind: TaskRepeatKind;
  /** 每 N 个单位 */
  repeat_interval: number;
  /** ISO 1..7（Mon..Sun）逗号分隔；仅 weekly 有意义 */
  repeat_weekdays: string | null;
  /** 循环截止日期 'YYYY-MM-DD' */
  repeat_until: string | null;
  /** 总触发次数上限（含首次） */
  repeat_count: number | null;
  /** 已触发次数 */
  repeat_done_count: number;
  /** AI 智能规划批次 ID（手动创建为 null） */
  source_batch_id: string | null;
  /** 一级分类 ID；null = 未分类 */
  category_id: number | null;
  /** 父任务 ID；null = 主任务，非 null = 子任务 */
  parent_task_id: number | null;
  /** 已完成子任务数（仅主任务有意义；子任务恒为 0） */
  subtask_done: number;
  /** 总子任务数 */
  subtask_total: number;
  links: TaskLink[];
}

export interface TaskLinkInput {
  kind: TaskLinkKind;
  target: string;
  label?: string | null;
}

export interface CreateTaskInput {
  title: string;
  description?: string | null;
  priority?: TaskPriority;
  important?: boolean;
  due_date?: string | null;
  remind_before_minutes?: number | null;
  links?: TaskLinkInput[];
  repeat_kind?: TaskRepeatKind;
  repeat_interval?: number;
  repeat_weekdays?: string | null;
  repeat_until?: string | null;
  repeat_count?: number | null;
  /** AI 智能规划批次 ID（同次生成共享一个 UUID，可一键撤销整批） */
  source_batch_id?: string | null;
  /** 一级分类 ID；不传或 null = 未分类 */
  category_id?: number | null;
  /** 父任务 ID；传则创建为该任务的子任务，不传 = 创建主任务 */
  parent_task_id?: number | null;
}

export interface UpdateTaskInput {
  title?: string;
  description?: string | null;
  priority?: TaskPriority;
  important?: boolean;
  due_date?: string | null;
  clear_due_date?: boolean;
  remind_before_minutes?: number | null;
  clear_remind_before_minutes?: boolean;
  repeat_kind?: TaskRepeatKind;
  repeat_interval?: number;
  repeat_weekdays?: string | null;
  clear_repeat_weekdays?: boolean;
  repeat_until?: string | null;
  clear_repeat_until?: boolean;
  repeat_count?: number | null;
  clear_repeat_count?: boolean;
  /** 改分类；不传 = 不动 */
  category_id?: number | null;
  /** 传 true 显式清空，落到"未分类" */
  clear_category_id?: boolean;
}

export interface TaskQuery {
  status?: TaskStatus;
  keyword?: string;
  priority?: TaskPriority;
  /** 某个分类（与 uncategorized 互斥，category_id 优先） */
  category_id?: number | null;
  /** true 时只看 category_id IS NULL 的任务 */
  uncategorized?: boolean;
}

// ─── 待办分类 ─────────────────────────────────

export interface TaskCategory {
  id: number;
  name: string;
  /** 圆点颜色，如 "#1677ff" */
  color: string;
  /** 可选 emoji 或 lucide 图标名 */
  icon: string | null;
  sort_order: number;
  created_at: string;
}

export interface CreateTaskCategoryInput {
  name: string;
  color?: string | null;
  icon?: string | null;
  sort_order?: number | null;
}

export interface UpdateTaskCategoryInput {
  name?: string;
  color?: string;
  icon?: string | null;
  clear_icon?: boolean;
  sort_order?: number;
}

export interface TaskStats {
  totalTodo: number;
  totalDone: number;
  urgentTodo: number;
  overdue: number;
  dueToday: number;
}

/** 顶栏 Ctrl+K 搜索的待办命中（轻量结果，与 SearchResult 对齐风格） */
export interface TaskSearchHit {
  id: number;
  title: string;
  /** description 截断 */
  snippet: string;
  /** 0=todo / 1=done */
  status: TaskStatus;
  /** 0=urgent / 1=normal / 2=low */
  priority: TaskPriority;
  dueDate: string | null;
}

// ─── AI 提示词库 ───────────────────────────────

/**
 * AI 写作结果插入模式
 * - `replace`：用结果替换选中的文本（默认；改写/扩展/翻译等）
 * - `append`：在选区末尾追加（续写场景）
 * - `popup`：只展示给用户看，不自动插入（总结场景）
 */
export type PromptOutputMode = "replace" | "append" | "popup";

export interface PromptTemplate {
  id: number;
  title: string;
  description: string;
  prompt: string;
  outputMode: PromptOutputMode;
  icon: string | null;
  isBuiltin: boolean;
  builtinCode: string | null;
  sortOrder: number;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface PromptTemplateInput {
  title: string;
  description?: string | null;
  prompt: string;
  outputMode?: PromptOutputMode;
  icon?: string | null;
  sortOrder?: number | null;
  enabled?: boolean | null;
}

// ─── T-024 同步 V1 ─────────────────────────────

export type SyncBackendKind = "local" | "webdav" | "s3";

export interface SyncBackend {
  id: number;
  kind: SyncBackendKind;
  name: string;
  configJson: string;
  enabled: boolean;
  autoSync: boolean;
  syncIntervalMin: number;
  lastPushTs: string | null;
  lastPullTs: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface SyncBackendInput {
  kind: SyncBackendKind;
  name: string;
  configJson: string;
  enabled?: boolean | null;
  autoSync?: boolean | null;
  syncIntervalMin?: number | null;
}

export interface ManifestEntry {
  stableId: string;
  title: string;
  contentHash: string;
  updatedAt: string;
  remotePath: string;
  tombstone: boolean;
  folderPath: string;
}

export interface SyncManifestV1 {
  manifestVersion: number;
  appVersion: string;
  device: string;
  generatedAt: string;
  entries: ManifestEntry[];
}

export interface SyncPushResult {
  uploaded: number;
  deletedRemote: number;
  skipped: number;
  errors: string[];
}

export interface SyncPullResult {
  downloaded: number;
  deletedLocal: number;
  conflicts: number;
  errors: string[];
}

export interface SyncV1ProgressEvent {
  backendId: number;
  /** "compute" | "diff" | "upload" | "download" | "manifest" | "apply" | "done" */
  phase: string;
  current: number;
  total: number;
  message: string;
}

// ─── T-013 自定义数据目录 ──────────────────────

/** 数据目录来源 */
export type DataDirSource = "env" | "pointer" | "default";

/** 当前数据目录解析结果 */
export interface ResolvedDataDir {
  /** 框架默认 app_data_dir（OS 给的固定位置）*/
  defaultDir: string;
  /** 当前生效的数据根目录 */
  currentDir: string;
  /** 来源（env / pointer / default）*/
  source: DataDirSource;
  /** 指针文件里写的路径（可能与 current 不一致：env 临时覆盖时；为 null 表示无指针） */
  pendingDir: string | null;
}

/** 迁移 marker 状态 */
export type MigrationStatus = "pending" | "in_progress" | "crashed" | "done";

/** 迁移 marker（启动期检测） */
export interface MigrationMarker {
  from: string;
  to: string;
  status: MigrationStatus;
  /** rust 是 snake_case，serde 默认序列化保持 snake；这里用相同字段名 */
  started_at: string;
  updated_at: string;
  completed_items: string[];
}

/** 迁移进度事件（splash 窗口监听用）*/
export interface MigrationProgress {
  /** "scan" | "copy_file" | "copy_dir" | "verify" | "done" | "error" */
  phase: string;
  currentFile: string;
  itemIndex: number;
  itemTotal: number;
  bytesDone: number;
  bytesTotal: number;
  message: string;
}

// ─── M5-2: 外部 MCP Server ─────────────────────────────────────

export interface McpServer {
  id: number;
  name: string;
  transport: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface McpServerInput {
  name: string;
  transport?: string;
  command: string;
  args?: string[];
  env?: Record<string, string>;
  enabled?: boolean;
}

// ─── 语音识别（ASR）─────────────────────────────

/** ASR 服务商。当前只支持阿里云 DashScope；后续接入新厂商时在这里加值。 */
export type AsrProviderKind = "dashscope";

/**
 * ASR 配置（与 Rust `AsrConfig` 对齐，camelCase 序列化）。
 *
 * 配置存到后端 `app_config` 表 `asr.*` KV，不走独立表。
 * `apiKey` 明文保存（与 ai_models.api_key 风格一致）。
 */
export interface AsrConfig {
  provider: AsrProviderKind;
  apiKey: string;
  /** 模型 ID，如 `qwen3-asr-flash-filetrans` / `paraformer-v2` */
  model: string;
  /** 区域："beijing"（默认）/ "singapore" */
  region: string;
  enabled: boolean;
}

/** 转录请求：把录音 base64 + MIME 提交给后端 */
export interface TranscribeRequest {
  /** 音频 base64（不含 data:xxx;base64, 前缀） */
  audioBase64: string;
  /** 音频 MIME，如 "audio/wav" / "audio/webm;codecs=opus" */
  mime: string;
  /** 语言提示（zh / en / auto），缺省让模型自动检测 */
  language?: string;
}

/** 转录结果 */
export interface TranscribeResult {
  text: string;
  latencyMs: number;
  model: string;
}

/** 「测试连接」结果 */
export interface AsrTestResult {
  ok: boolean;
  latencyMs: number;
  /** 失败时的简短中文原因；成功时为 null */
  message: string | null;
}

// ─── 闪卡 + FSRS 复习 ────────────────────────────────────────

/**
 * 闪卡。
 *
 * FSRS state：0=New, 1=Learning, 2=Review, 3=Relearning
 *   与 ts-fsrs 的 State 枚举值一致；写入/读出都用整数。
 *
 * Rust 侧字段名用 snake_case，serde 默认不改名，所以这里 TS 也保持 snake_case
 * （与 Note 等老类型风格一致）。
 */
export interface Card {
  id: number;
  note_id: number | null;
  front: string;
  back: string;
  deck: string;

  // FSRS 调度状态
  due: string;
  stability: number;
  difficulty: number;
  elapsed_days: number;
  scheduled_days: number;
  reps: number;
  lapses: number;
  state: number;
  last_review: string | null;

  is_deleted: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateCardInput {
  front: string;
  back: string;
  deck?: string;
  noteId?: number | null;
}

/** ts-fsrs 算好的新调度状态 + 用户评分，提交给后端 review_card */
export interface ReviewCardInput {
  cardId: number;
  /** 1=Again, 2=Hard, 3=Good, 4=Easy（与 ts-fsrs Rating 枚举一致） */
  rating: number;
  state: number;
  due: string;
  stability: number;
  difficulty: number;
  elapsedDays: number;
  lastElapsedDays: number;
  scheduledDays: number;
}

export interface CardReviewLog {
  id: number;
  cardId: number;
  rating: number;
  state: number;
  due: string;
  stability: number;
  difficulty: number;
  elapsedDays: number;
  lastElapsedDays: number;
  scheduledDays: number;
  review: string;
}

export interface CardStats {
  dueToday: number;
  learning: number;
  review: number;
  newCards: number;
  total: number;
}

// ─── 插件化待办（阶段 2）─────────────────────────

/** 插件可见的任务过滤条件 */
export interface PluginTaskFilter {
  categoryId?: number | null;
  status?: 'pending' | 'completed' | 'archived';
  parentTaskId?: number | null;
  dueBefore?: string;
  dueAfter?: string;
  limit?: number;
  offset?: number;
}

/** 插件可见的任务对象（脱敏，不含 sourceBatchId 等内部字段） */
export interface PluginTaskView {
  id: number;
  title: string;
  description: string | null;
  status: 'pending' | 'completed' | 'archived';
  priority: number;
  important: boolean;
  dueAt: string | null;
  remindBeforeMinutes: number | null;
  categoryId: number | null;
  parentTaskId: number | null;
  createdAt: string;
  updatedAt: string;
}

/** 插件视图 render 函数接收的 props */
export interface TaskViewProps {
  tasks: PluginTaskView[];
  filter: PluginTaskFilter;
  api: {
    complete(id: number): Promise<void>;
    openDetail(id: number): void;
    update(id: number, patch: Partial<PluginTaskView>): Promise<void>;
  };
}

/** 插件创建任务入参 */
export interface PluginCreateTaskInput {
  title: string;
  description?: string | null;
  priority?: number;
  important?: boolean;
  dueAt?: string | null;
  remindBeforeMinutes?: number | null;
  categoryId?: number | null;
  parentTaskId?: number | null;
}

// ─── 任务执行会话 ─────────────────────────────────

export type SessionStatus = "idle" | "running" | "waiting_confirm" | "paused" | "completed";
export type PhaseStatus = "pending" | "running" | "waiting_confirm" | "completed" | "skipped" | "failed";

export interface TaskSession {
  id: string;
  plan_path: string;
  plan_name: string;
  status: SessionStatus;
  current_phase_index: number;
  total_phases: number;
  created_at: string;
  updated_at: string;
}

export interface ExecutionPhase {
  id: string;
  session_id: string;
  index_num: number;
  name: string;
  description: string;
  status: PhaseStatus;
  files_modified?: string | null;
  result_summary?: string | null;
  started_at?: string | null;
  finished_at?: string | null;
}

export interface ExecutionLog {
  id: number;
  session_id: string;
  phase_id?: string | null;
  level: string;
  message: string;
  created_at: string;
}

export interface CreateExecutionLogInput {
  session_id: string;
  phase_id?: string | null;
  level: string;
  message: string;
}

export interface ParsedPlan {
  name: string;
  phases: PhaseDraft[];
}

export interface PhaseDraft {
  id: string;
  name: string;
  description: string;
}

export interface TaskSessionDetail {
  session: TaskSession;
  phases: ExecutionPhase[];
}

/** 插件更新任务入参 */
export interface PluginUpdateTaskInput {
  title?: string;
  description?: string;
  priority?: number;
  important?: boolean;
  dueAt?: string | null;
  remindBeforeMinutes?: number | null;
  categoryId?: number | null;
  clearDueAt?: boolean;
  clearRemindBeforeMinutes?: boolean;
  clearCategoryId?: boolean;
}

// ─── 项目文件夹会话 ─────────────────────────────────

export interface ProjectSession {
  id: string;
  projectName: string;
  projectPath: string;
  status: string;
  gitBranch?: string | null;
  isOpen: boolean;
  lastActiveAt: string;
  createdAt: string;
}

// ─── Claude Code Agent Runner ────────────────────

export type ClaudeAgentStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export type ClaudeAgentPermissionMode =
  | "readonly"
  | "ask"
  | "workspace_write";

export interface StartClaudeAgentInput {
  project_path: string;
  prompt: string;
  permission_mode: string;
  session_name?: string;
}

export interface ClaudeAgentSession {
  id: string;
  project_path: string;
  prompt: string;
  session_name?: string | null;
  permission_mode: string;
  status: string;
  pid?: number | null;
  exit_code?: number | null;
  error_message?: string | null;
  created_at: string;
  updated_at: string;
  started_at?: string | null;
  finished_at?: string | null;
}

export interface ClaudeAgentEvent {
  id: number;
  session_id: string;
  kind: string;
  content: string;
  created_at: string;
}

export interface ClaudeAgentEventPayload {
  sessionId: string;
  kind: string;
  content: string;
  createdAt: string;
}

export interface ProjectSessionContext {
  sessionId: string;
  projectPath: string;
  gitBranch?: string | null;
  changedFiles: string[];
  pinnedFiles: string[];
  recentFiles: string[];
  currentTask?: string | null;
  updatedAt: string;
}

export interface ProjectSessionMessage {
  id: string;
  sessionId: string;
  role: string;
  content: string;
  createdAt: string;
}
