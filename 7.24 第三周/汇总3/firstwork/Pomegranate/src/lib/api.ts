import { invoke } from "@tauri-apps/api/core";
import { isEnabled as isAutostartEnabled, enable as enableAutostart, disable as disableAutostart } from "@tauri-apps/plugin-autostart";
import { check as checkForUpdate } from "@tauri-apps/plugin-updater";
import type {
  AttachmentPreview,
  AiConversation,
  AiMessage,
  AiModel,
  AiModelInput,
  AiModelTestResult,
  AgentMessageInfo,
  AgentSendMessageInput,
  AgentSendMessageResult,
  AgentSessionCreateInput,
  AgentSessionInfo,
  AgentTestResult,
  AgentUsageEvent,
  AgentWorkflowInvokeInput,
  AgentWorkflowInvokeResult,
  CreateTaskInput,
  CredentialCreateInput,
  CredentialInfo,
  CredentialUpdateInput,
  CredentialUsage,
  CourseGraphConfig,
  CourseGraphAiAnalysis,
  CourseGraphAiRelation,
  CourseGraphAiRelationStatus,
  CourseGraphHealth,
  CourseGraphStats,
  DailyWritingStat,
  DashboardStats,
  DocConverter,
  Folder,
  GraphData,
  MessageAttachment,
  MigrationMarker,
  MarketplaceAcquireInput,
  MarketplaceActionResult,
  MarketplaceEntitlement,
  MarketplaceExternalAuthorizationInput,
  MarketplaceInstallInput,
  MarketplaceLedgerEntry,
  MarketplaceMockTestResult,
  MarketplacePermissionRejectionInput,
  MarketplaceProductDetail,
  MarketplaceProductQuery,
  MarketplaceProductSummary,
  MarketplaceRefundInput,
  MarketplaceReviewInfo,
  MarketplaceReviewInput,
  MarketplaceServiceConfigurationInput,
  MarketplaceUpdateInfo,
  MarketplaceUpdateInput,
  LocalAccountProfile,
  LocalAccountUpdateInput,
  MarketplaceMockRole,
  MarketplaceMockSession,
  MarketplaceOrder,
  DeveloperProduct,
  DeveloperProductInput,
  DeveloperProductVersion,
  DeveloperUploadPackageInput,
  DeveloperVersionInput,
  DeveloperSubmitInput,
  MarketplacePackageReport,
  MarketplaceReviewStatus,
  MarketplaceSubmission,
  AdminReviewInput,
  AdminProductModerationInput,
  AdminVersionModerationInput,
  DeveloperDashboard,
  DeveloperEarning,
  ExternalAgentConfig,
  ExternalAgentInput,
  ImportResult,
  Note,
  NoteTemplate,
  NoteTemplateInput,
  NoteInput,
  NoteQuery,
  NoteLink,
  OpenMarkdownResult,
  OrphanAssetClean,
  OrphanAssetScan,
  PageResult,
  ParsedPlan,
  ProjectSession,
  ProjectSessionMessage,
  PdfImportResult,
  PptMasterCheckInput,
  PptMasterCheckResult,
  PptMasterExportInput,
  PptMasterExportResult,
  PptMasterGenerateInput,
  PptMasterGenerateResult,
  ResearchAnalysisInput,
  ResearchAnalysisResult,
  ResearchPaperKnowledgeRecommendation,
  ResearchPaperKnowledgeRecommendationInput,
  ResearchPaperSearchInput,
  ResearchPaperSearchResult,
  PluginAuditLogEntry,
  PluginDocumentSummaryInput,
  PluginDocumentSummaryAgentFinalizeInput,
  PluginDocumentSummaryAgentStartInput,
  PluginDocumentSummaryAgentStartResult,
  PluginDocumentSummaryCancelInput,
  PluginDocumentSummaryConfig,
  PluginDocumentSummaryConfigInput,
  PluginDocumentSummaryInsertInput,
  PluginDocumentSummaryResult,
  PluginDocumentToolbarButton,
  PluginSummaryAgentOption,
  NormalizedPluginManifest,
  PermissionDiff,
  PluginCompatibility,
  PluginInstallationInfo,
  PluginIntegrityCheck,
  PluginInfo,
  PluginManifest,
  PluginPackageInspection,
  PlanningSessionKind,
  PlanningWorkspace,
  PluginRuntimePolicy,
  PluginActivationRule,
  PluginArchiveInspection,
  PluginExecutionContext,
  PluginExecutionLogInput,
  PluginFeatureInvokeInput,
  PluginFeatureInvokeResult,
  PluginInstallArchiveInput,
  PluginInstallResult,
  PluginVersionInfo,
  ResolvedPluginContributions,
  PromptTemplate,
  PromptTemplateInput,
  ResolvedDataDir,
  ScannedFile,
  RestoreBatchResult,
  RuntimeDataDirectory,
  SearchResult,
  ShortcutBinding,
  SyncBackend,
  SyncBackendInput,
  SyncHistoryItem,
  SyncPullResult,
  SyncPushResult,
  SystemInfo,
  AsrConfig,
  AsrTestResult,
  BindableXingchenProduct,
  Tag,
  Task,
  TaskCategory,
  TaskLinkInput,
  TaskQuery,
  TaskSearchHit,
  TranscribeResult,
  TaskSession,
  TaskSessionDetail,
  TaskStats,
  UpdateTaskInput,
} from "@/types";

type NoteListResult = {
  items: Note[];
  total: number;
  page: number;
  page_size: number;
};

interface SystemApi {
  getDashboardStats(): Promise<DashboardStats>;
  getSystemInfo(): Promise<SystemInfo>;
  getMultiInstanceEnabled(): Promise<boolean>;
  getWritingTrend(days: number): Promise<DailyWritingStat[]>;
  resolveAssetAbsolute(rel: string): Promise<string>;
  writeTextFile(path: string, content: string): Promise<void>;
  setMultiInstanceEnabled(enabled: boolean): Promise<void>;
}

interface AiChatApi {
  listConversations(): Promise<AiConversation[]>;
  listMessages(conversationId: number): Promise<AiMessage[]>;
  createConversation(title?: string, modelId?: number | null): Promise<AiConversation>;
  deleteConversation(id: number): Promise<void>;
  deleteConversationsBefore(olderThanDays?: number): Promise<number>;
  renameConversation(id: number, title: string): Promise<void>;
  updateConversationModel(id: number, modelId: number): Promise<void>;
  setAttachedNotes(conversationId: number, noteIds: number[]): Promise<void>;
  sendMessage(
    conversationId: number,
    message: string,
    useRag?: boolean,
    useSkills?: boolean,
    attachments?: MessageAttachment[],
    effectiveMessage?: string,
    pluginSystemContext?: string,
  ): Promise<void>;
  finalizePluginOutput(
    conversationId: number,
    expectedOutput: string,
    finalOutput: string,
  ): Promise<void>;
  cancelGeneration(conversationId: number): Promise<void>;
  getOrCreateCompanionConversation(noteId: number): Promise<AiConversation>;
  archiveToNote(
    conversationId: number,
    title?: string,
    folderId?: number | null,
  ): Promise<Note>;
}

interface AiModelApi {
  list(): Promise<AiModel[]>;
  create(input: AiModelInput): Promise<AiModel>;
  update(id: number, input: AiModelInput): Promise<AiModel>;
  delete(id: number): Promise<void>;
  setDefault(id: number): Promise<void>;
  test(input: AiModelInput): Promise<AiModelTestResult>;
}

interface AiWriteApi {
  assist(action: string, selectedText: string, context?: string): Promise<string>;
  cancel(): Promise<void>;
  suggestPrompt(selectedText: string, context?: string): Promise<string>;
  understandPpt(input: { prompt: string; modelId?: number | null }): Promise<string>;
  understandPptChunk(input: { prompt: string; modelId: number }): Promise<string>;
  mergePptUnderstanding(input: { prompt: string; modelId: number }): Promise<string>;
}

interface PptMasterApi {
  check(input: PptMasterCheckInput): Promise<PptMasterCheckResult>;
  export(input: PptMasterExportInput): Promise<PptMasterExportResult>;
  generateFromPrompt(input: PptMasterGenerateInput): Promise<PptMasterGenerateResult>;
}

interface NoteApi {
  list(query?: NoteQuery): Promise<NoteListResult>;
  get(id: number): Promise<Note>;
  create(input: NoteInput): Promise<Note>;
  update(id: number, input: Partial<NoteInput> & Record<string, unknown>): Promise<Note>;
  delete(id: number): Promise<void>;
  togglePin(id: number): Promise<boolean>;
  moveToFolder(noteId: number, folderId?: number | null): Promise<void>;
  reorder(orderedIds: number[]): Promise<void>;
  moveBatch(ids: number[], folderId?: number | null): Promise<number>;
  addTagsBatch(noteIds: number[], tagIds: number[]): Promise<number>;
  trashBatch(ids: number[]): Promise<number>;
  trashAll(): Promise<number>;
  setHidden(id: number, hidden: boolean): Promise<boolean>;
  clipUrl(url: string, folderId?: number | null): Promise<Note>;
  openInNewWindow(noteId: number): Promise<void>;
}

interface FolderApi {
  list(): Promise<Folder[]>;
  create(name: string, parentId?: number | null): Promise<Folder>;
  rename(id: number, name: string): Promise<void>;
  delete(id: number): Promise<void>;
  move(id: number, newParentId?: number | null): Promise<void>;
  reorder(orderedIds: number[]): Promise<void>;
  ensurePath(path: string): Promise<number | null>;
}

interface TaskApi {
  list(query?: TaskQuery): Promise<Task[]>;
  get(id: number): Promise<Task>;
  search(keyword: string, limit?: number): Promise<TaskSearchHit[]>;
  stats(): Promise<TaskStats>;
  listSubtasks(parentTaskId: number): Promise<Task[]>;
  create(input: CreateTaskInput): Promise<number>;
  update(id: number, input: UpdateTaskInput): Promise<boolean>;
  delete(id: number): Promise<boolean>;
  deleteBatch(ids: number[]): Promise<number>;
  completeBatch(ids: number[]): Promise<number>;
  toggleStatus(id: number): Promise<number>;
  addLink(taskId: number, input: TaskLinkInput): Promise<number>;
  removeLink(linkId: number): Promise<boolean>;
  snooze(id: number, minutes: number): Promise<boolean>;
  completeOccurrence(id: number): Promise<void>;
}

interface DailyApi {
  get(date: string): Promise<Note | null>;
  getOrCreate(date: string): Promise<Note>;
  listDates(year: number, month: number): Promise<string[]>;
  getNeighbors(date: string): Promise<[string | null, string | null]>;
}

interface TagApi {
  list(): Promise<Tag[]>;
  create(name: string, color?: string | null): Promise<Tag>;
  rename(id: number, name: string): Promise<void>;
  setColor(id: number, color?: string | null): Promise<void>;
  delete(id: number): Promise<void>;
  addToNote(noteId: number, tagId: number): Promise<void>;
  removeFromNote(noteId: number, tagId: number): Promise<void>;
  getNoteTags(noteId: number): Promise<Tag[]>;
  listNotesByTag(tagId: number, page?: number, pageSize?: number): Promise<PageResult<Note>>;
}

interface ConfigApi {
  get(key: string): Promise<string>;
  set(key: string, value: string): Promise<void>;
  delete(key: string): Promise<void>;
}

interface PromptApi {
  list(enabledOnly?: boolean): Promise<PromptTemplate[]>;
  get(id: number): Promise<PromptTemplate>;
  create(input: PromptTemplateInput): Promise<PromptTemplate>;
  update(id: number, input: PromptTemplateInput): Promise<PromptTemplate>;
  delete(id: number): Promise<boolean>;
  setEnabled(id: number, enabled: boolean): Promise<void>;
}

interface PluginApi {
  list(): Promise<PluginInfo[]>;
  scan(): Promise<PluginInfo[]>;
  installFromDir(path: string): Promise<PluginInfo>;
  enable(pluginId: string): Promise<void>;
  disable(pluginId: string): Promise<void>;
  uninstall(pluginId: string): Promise<boolean>;
  getManifest(pluginId: string): Promise<PluginManifest>;
  grantPermissions(pluginId: string, permissions: string[]): Promise<number>;
  revokePermissions(pluginId: string, permissions: string[]): Promise<number>;
  getSettings(pluginId: string): Promise<Record<string, unknown>>;
  setSettings(pluginId: string, settings: unknown): Promise<void>;
  readAsset(pluginId: string, relativePath: string): Promise<string>;
  getAuditLog(pluginId: string, limit?: number): Promise<PluginAuditLogEntry[]>;
  parseManifest(path: string): Promise<NormalizedPluginManifest>;
  validateManifest(path: string): Promise<NormalizedPluginManifest>;
  inspectPackage(path: string): Promise<PluginPackageInspection>;
  calculateIntegrity(path: string): Promise<string>;
  comparePermissions(current: string[], next: string[]): Promise<PermissionDiff>;
  checkCompatibility(minAppVersion?: string | null): Promise<PluginCompatibility>;
  getInstallation(pluginId: string): Promise<PluginInstallationInfo | null>;
  listInstallations(): Promise<PluginInstallationInfo[]>;
  verifyInstallation(pluginId: string): Promise<PluginIntegrityCheck>;
  canExecuteRuntime(pluginId: string): Promise<PluginRuntimePolicy>;
  inspectArchive(path: string): Promise<PluginArchiveInspection>;
  installArchive(input: PluginInstallArchiveInput): Promise<PluginInstallResult>;
  updateArchive(input: PluginInstallArchiveInput): Promise<PluginInstallResult>;
  rollback(pluginId: string, version: string): Promise<PluginInstallResult>;
  listVersions(pluginId: string): Promise<PluginVersionInfo[]>;
  getActivationSettings(pluginId: string): Promise<PluginActivationRule[]>;
  setActivationSetting(
    pluginId: string,
    scopeType: "global" | "scene" | "feature",
    scopeKey: string,
    enabled: boolean,
  ): Promise<void>;
  resolveEnabledContributions(context: PluginExecutionContext): Promise<ResolvedPluginContributions>;
  recordExecution(input: PluginExecutionLogInput): Promise<void>;
  invokeXingchenFeature(input: PluginFeatureInvokeInput): Promise<PluginFeatureInvokeResult>;
  listDocumentSummaryToolbarButtons(): Promise<PluginDocumentToolbarButton[]>;
  mockDocumentSummary(input: PluginDocumentSummaryInput): Promise<PluginDocumentSummaryResult>;
  recordDocumentSummaryInsert(input: PluginDocumentSummaryInsertInput): Promise<void>;
  listDocumentSummaryAgents(pluginId: string): Promise<PluginSummaryAgentOption[]>;
  getDocumentSummaryConfig(pluginId: string): Promise<PluginDocumentSummaryConfig>;
  setDocumentSummaryConfig(input: PluginDocumentSummaryConfigInput): Promise<PluginDocumentSummaryConfig>;
  startDocumentSummaryAgent(input: PluginDocumentSummaryAgentStartInput): Promise<PluginDocumentSummaryAgentStartResult>;
  cancelDocumentSummary(input: PluginDocumentSummaryCancelInput): Promise<void>;
  finalizeDocumentSummaryAgent(input: PluginDocumentSummaryAgentFinalizeInput): Promise<void>;
}

interface PlanningApi {
  getWorkspace(sessionKind: PlanningSessionKind, sessionId: string): Promise<PlanningWorkspace>;
  setEnabled(
    sessionKind: PlanningSessionKind,
    sessionId: string,
    enabled: boolean,
  ): Promise<PlanningWorkspace>;
  saveFile(
    sessionKind: PlanningSessionKind,
    sessionId: string,
    fileName: string,
    content: string,
  ): Promise<PlanningWorkspace>;
  applyUpdate(
    sessionKind: PlanningSessionKind,
    sessionId: string,
    accept: boolean,
  ): Promise<PlanningWorkspace>;
  clear(sessionKind: PlanningSessionKind, sessionId: string, confirm: boolean): Promise<PlanningWorkspace>;
  export(sessionKind: PlanningSessionKind, sessionId: string, targetDir: string): Promise<void>;
}

interface MarketplaceApi {
  listAccounts(): Promise<LocalAccountProfile[]>;
  switchAccount(userId: string): Promise<MarketplaceMockSession>;
  updateAccount(input: LocalAccountUpdateInput): Promise<MarketplaceMockSession>;
  applyDeveloper(): Promise<MarketplaceMockSession>;
  listProducts(query?: MarketplaceProductQuery): Promise<MarketplaceProductSummary[]>;
  searchProducts(query: MarketplaceProductQuery): Promise<MarketplaceProductSummary[]>;
  getProduct(productId: string): Promise<MarketplaceProductDetail>;
  getProductVersion(productId: string, version?: string | null): Promise<NormalizedPluginManifest>;
  acquireProduct(input: MarketplaceAcquireInput): Promise<MarketplaceActionResult>;
  bindExternalAuthorization(input: MarketplaceExternalAuthorizationInput): Promise<MarketplaceActionResult>;
  listEntitlements(): Promise<MarketplaceEntitlement[]>;
  installProduct(input: MarketplaceInstallInput): Promise<MarketplaceActionResult>;
  updateProduct(input: MarketplaceUpdateInput): Promise<MarketplaceActionResult>;
  uninstallProduct(productId: string): Promise<MarketplaceActionResult>;
  enableProduct(productId: string): Promise<MarketplaceActionResult>;
  disableProduct(productId: string): Promise<MarketplaceActionResult>;
  recordPermissionRejection(input: MarketplacePermissionRejectionInput): Promise<MarketplaceActionResult>;
  configureService(input: MarketplaceServiceConfigurationInput): Promise<MarketplaceActionResult>;
  listInstalled(): Promise<MarketplaceProductSummary[]>;
  checkUpdates(): Promise<MarketplaceUpdateInfo[]>;
  verifyInstallation(productId: string): Promise<MarketplaceActionResult>;
  devRevokeProductVersion(productId: string, version?: string | null): Promise<MarketplaceActionResult>;
  devRestoreProductVersion(productId: string, version?: string | null): Promise<MarketplaceActionResult>;
  mockTestProduct(productId: string): Promise<MarketplaceMockTestResult>;
  getMockSession(): Promise<MarketplaceMockSession>;
  switchMockRole(role: MarketplaceMockRole): Promise<MarketplaceMockSession>;
  listOrders(): Promise<MarketplaceOrder[]>;
  listLedger(): Promise<MarketplaceLedgerEntry[]>;
  requestRefund(input: MarketplaceRefundInput): Promise<MarketplaceActionResult>;
  listReviews(productId: string): Promise<MarketplaceReviewInfo[]>;
  submitReview(input: MarketplaceReviewInput): Promise<MarketplaceReviewInfo>;
}

interface DeveloperApi {
  listProducts(): Promise<DeveloperProduct[]>;
  createProduct(input: DeveloperProductInput): Promise<DeveloperProduct>;
  updateProduct(productId: string, input: DeveloperProductInput): Promise<DeveloperProduct>;
  createVersion(input: DeveloperVersionInput): Promise<DeveloperProductVersion>;
  uploadPackage(input: DeveloperUploadPackageInput): Promise<MarketplacePackageReport>;
  getPackageReport(productId: string, version: string): Promise<MarketplacePackageReport>;
  submitProduct(input: DeveloperSubmitInput): Promise<MarketplaceActionResult>;
  submitVersion(input: DeveloperSubmitInput): Promise<MarketplaceActionResult>;
  listEarnings(): Promise<DeveloperEarning[]>;
  getDashboard(): Promise<DeveloperDashboard>;
}

interface AdminMarketplaceApi {
  listSubmissions(status?: MarketplaceReviewStatus | null): Promise<MarketplaceSubmission[]>;
  getSubmission(submissionId: number): Promise<MarketplaceSubmission>;
  startReview(input: AdminReviewInput): Promise<MarketplaceActionResult>;
  approveSubmission(input: AdminReviewInput): Promise<MarketplaceActionResult>;
  rejectSubmission(input: AdminReviewInput): Promise<MarketplaceActionResult>;
  suspendProduct(input: AdminProductModerationInput): Promise<MarketplaceActionResult>;
  restoreProduct(input: AdminProductModerationInput): Promise<MarketplaceActionResult>;
  delistProduct(input: AdminProductModerationInput): Promise<MarketplaceActionResult>;
  revokeVersion(input: AdminVersionModerationInput): Promise<MarketplaceActionResult>;
}

interface CredentialApi {
  list(): Promise<CredentialInfo[]>;
  create(input: CredentialCreateInput): Promise<CredentialInfo>;
  update(id: string, input: CredentialUpdateInput): Promise<CredentialInfo>;
  delete(id: string, force?: boolean): Promise<void>;
  getUsage(id: string): Promise<CredentialUsage[]>;
}

interface ExternalAgentApi {
  list(): Promise<ExternalAgentConfig[]>;
  listBindableProducts(): Promise<BindableXingchenProduct[]>;
  create(input: ExternalAgentInput): Promise<ExternalAgentConfig>;
  update(id: string, input: ExternalAgentInput): Promise<ExternalAgentConfig>;
  delete(id: string): Promise<void>;
  testConnection(id: string): Promise<AgentTestResult>;
  healthCheck(id: string): Promise<AgentTestResult>;
  listSessions(externalAgentId?: string | null): Promise<AgentSessionInfo[]>;
  createSession(input: AgentSessionCreateInput): Promise<AgentSessionInfo>;
  deleteSession(id: string): Promise<void>;
  listMessages(sessionId: string): Promise<AgentMessageInfo[]>;
  sendMessage(input: AgentSendMessageInput): Promise<AgentSendMessageResult>;
  finalizePluginOutput(
    sessionId: string,
    requestId: string,
    expectedOutput: string,
    finalOutput: string,
  ): Promise<void>;
  invokeWorkflow(input: AgentWorkflowInvokeInput): Promise<AgentWorkflowInvokeResult>;
  cancelRequest(requestId: string): Promise<void>;
  listUsage(externalAgentId?: string | null): Promise<AgentUsageEvent[]>;
  clearUsage(externalAgentId?: string | null): Promise<void>;
}

interface TrashApi {
  softDelete(id: number): Promise<void>;
  list(page?: number, pageSize?: number): Promise<PageResult<Note>>;
  restore(id: number): Promise<boolean>;
  permanentDelete(id: number): Promise<void>;
  empty(): Promise<number>;
  restoreBatch(ids: number[]): Promise<RestoreBatchResult>;
  permanentDeleteBatch(ids: number[]): Promise<number>;
}

interface HiddenApi {
  list(options?: {
    page?: number;
    pageSize?: number;
    folderId?: number | null;
    uncategorized?: boolean;
  }): Promise<PageResult<Note>>;
  listFolderIds(): Promise<Array<number | null>>;
}

interface HiddenPinApi {
  isSet(): Promise<boolean>;
  getHint(): Promise<string | null>;
  verify(pin: string): Promise<void>;
  set(oldPin: string | null, newPin: string, hint?: string | null): Promise<void>;
  clear(currentPin: string): Promise<void>;
}


export const systemApi: SystemApi = {
  getDashboardStats: () => invoke("get_dashboard_stats"),
  getSystemInfo: () => invoke("get_system_info"),
  getMultiInstanceEnabled: () => invoke("get_multi_instance_enabled"),
  getWritingTrend: (days) => invoke("get_writing_trend", { days }),
  resolveAssetAbsolute: (rel) => invoke("resolve_asset_absolute_path", { rel }),
  writeTextFile: (path, content) => invoke("write_text_file", { path, content }),
  setMultiInstanceEnabled: (enabled) => invoke("set_multi_instance_enabled", { enabled }),
};
export const updaterApi: any = {
  checkUpdate: () => checkForUpdate(),
};
export const aiChatApi: AiChatApi = {
  listConversations: () => invoke("list_ai_conversations"),
  listMessages: (conversationId) => invoke("list_ai_messages", { conversationId }),
  createConversation: (title, modelId = null) =>
    invoke("create_ai_conversation", { title, modelId }),
  deleteConversation: (id) => invoke("delete_ai_conversation", { id }),
  deleteConversationsBefore: (olderThanDays) =>
    invoke("delete_ai_conversations_before", { olderThanDays: olderThanDays ?? null }),
  renameConversation: (id, title) => invoke("rename_ai_conversation", { id, title }),
  updateConversationModel: (id, modelId) =>
    invoke("update_ai_conversation_model", { id, modelId }),
  setAttachedNotes: (conversationId, noteIds) =>
    invoke("set_ai_conversation_attached_notes", {
      conversationId,
      noteIds,
    }),
  sendMessage: (
    conversationId,
    message,
    useRag,
    useSkills,
    attachments,
    effectiveMessage,
    pluginSystemContext,
  ) =>
    invoke("send_ai_message", {
      conversationId,
      message,
      effectiveMessage,
      pluginSystemContext,
      useRag,
      useSkills,
      attachments,
    }),
  finalizePluginOutput: (conversationId, expectedOutput, finalOutput) =>
    invoke("finalize_ai_plugin_output", {
      conversationId,
      expectedOutput,
      finalOutput,
    }),
  cancelGeneration: (conversationId) =>
    invoke("cancel_ai_generation", { conversationId }),
  getOrCreateCompanionConversation: (noteId) =>
    invoke("get_or_create_companion_conversation", { noteId }),
  archiveToNote: (conversationId, title, folderId = null) =>
    invoke("archive_ai_conversation_to_note", {
      conversationId,
      title,
      folderId,
    }),
};
export const aiModelApi: AiModelApi = {
  list: () => invoke("list_ai_models"),
  create: (input) => invoke("create_ai_model", { input }),
  update: (id, input) => invoke("update_ai_model", { id, input }),
  delete: (id) => invoke("delete_ai_model", { id }),
  setDefault: (id) => invoke("set_default_ai_model", { id }),
  test: (input) => invoke("test_ai_model", { input }),
};
export const aiWriteApi: AiWriteApi = {
  assist: (action, selectedText, context) =>
    invoke("ai_write_assist", { action, selectedText, context }),
  cancel: () => invoke("cancel_ai_write_assist"),
  suggestPrompt: (selectedText, context) =>
    invoke("ai_suggest_prompt", { selectedText, context }),
  understandPpt: ({ prompt, modelId }) =>
    invoke("ai_ppt_understand", { input: { prompt, modelId, requestKind: "direct" } }),
  understandPptChunk: ({ prompt, modelId }) =>
    invoke("ai_ppt_understand", { input: { prompt, modelId, requestKind: "chunk" } }),
  mergePptUnderstanding: ({ prompt, modelId }) =>
    invoke("ai_ppt_understand", { input: { prompt, modelId, requestKind: "merge" } }),
};
export const aiPlanApi: any = {
  planToday: (request: unknown) => invoke("ai_plan_today", { request }),
  extractTaskFromText: (text: string) => invoke("ai_extract_task_from_text", { text }),
  planFromGoal: (request: unknown) => invoke("ai_plan_from_goal", { request }),
  undoBatch: (batchId: string) => invoke("undo_task_batch", { batchId }),
  planFromExcel: (request: unknown) => invoke("ai_plan_from_excel", { request }),
  draftNote: (request: unknown) => invoke("ai_draft_note", { request }),
};
export const aiAttachmentApi: any = {
  parseAttachment: (filePath: string): Promise<AttachmentPreview> =>
    invoke("ai_parse_attachment", { filePath }),
  parseExcel: (filePath: string) => invoke("ai_parse_excel", { filePath }),
};
export const noteApi: NoteApi = {
  list: (query) => invoke("list_notes", { query: query ?? {} }),
  get: (id) => invoke("get_note", { id }),
  create: (input) => invoke("create_note", { input }),
  update: (id, input) => invoke("update_note", { id, input }),
  delete: (id) => invoke("delete_note", { id }),
  togglePin: (id) => invoke("toggle_pin", { id }),
  moveToFolder: (noteId, folderId = null) =>
    invoke("move_note_to_folder", { noteId, folderId }),
  reorder: (orderedIds) => invoke("reorder_notes", { orderedIds }),
  moveBatch: (ids, folderId = null) => invoke("move_notes_batch", { ids, folderId }),
  addTagsBatch: (noteIds, tagIds) =>
    invoke("add_tags_to_notes_batch", { noteIds, tagIds }),
  trashBatch: (ids) => invoke("trash_notes_batch", { ids }),
  trashAll: () => invoke("trash_all_notes"),
  setHidden: (id, hidden) => invoke("set_note_hidden", { id, hidden }),
  clipUrl: (url, folderId = null) => invoke("clip_url_to_note", { url, folderId }),
  openInNewWindow: (noteId) => invoke("open_note_in_new_window", { noteId }),
};
export const folderApi: FolderApi = {
  list: () => invoke("list_folders"),
  create: (name, parentId = null) => invoke("create_folder", { name, parentId }),
  rename: (id, name) => invoke("rename_folder", { id, name }),
  delete: (id) => invoke("delete_folder", { id }),
  move: (id, newParentId = null) => invoke("move_folder", { id, newParentId }),
  reorder: (orderedIds) => invoke("reorder_folders", { orderedIds }),
  ensurePath: (path) => invoke("ensure_folder_path", { path }),
};
export const tagApi: TagApi = {
  list: () => invoke("list_tags"),
  create: (name, color = null) => invoke("create_tag", { name, color }),
  rename: (id, name) => invoke("rename_tag", { id, name }),
  setColor: (id, color = null) => invoke("set_tag_color", { id, color }),
  delete: (id) => invoke("delete_tag", { id }),
  addToNote: (noteId, tagId) => invoke("add_tag_to_note", { noteId, tagId }),
  removeFromNote: (noteId, tagId) =>
    invoke("remove_tag_from_note", { noteId, tagId }),
  getNoteTags: (noteId) => invoke("get_note_tags", { noteId }),
  listNotesByTag: (tagId, page, pageSize) =>
    invoke("list_notes_by_tag", { tagId, page, pageSize }),
};
export const taskApi: TaskApi = {
  list: (query) => (query === undefined ? invoke("list_tasks") : invoke("list_tasks", { query })),
  get: (id) => invoke("get_task", { id }),
  search: (query, limit) =>
    limit === undefined ? invoke("search_tasks", { query }) : invoke("search_tasks", { query, limit }),
  stats: () => invoke("get_task_stats"),
  listSubtasks: (parentTaskId) => invoke("list_subtasks", { parentId: parentTaskId }),
  create: (input) => invoke("create_task", { input }),
  update: (id, input) => invoke("update_task", { id, input }),
  delete: (id) => invoke("delete_task", { id }),
  deleteBatch: (ids) => invoke("delete_tasks_batch", { ids }),
  completeBatch: (ids) => invoke("complete_tasks_batch", { ids }),
  toggleStatus: (id) => invoke("toggle_task_status", { id }),
  addLink: (taskId, input) => invoke("add_task_link", { taskId, input }),
  removeLink: (linkId) => invoke("remove_task_link", { linkId }),
  snooze: (id, minutes) => invoke("snooze_task_reminder", { id, minutes }),
  completeOccurrence: (id) => invoke("complete_task_occurrence", { id }),
};
export const taskCategoryApi: any = {
  list: (): Promise<TaskCategory[]> => invoke("list_task_categories"),
  create: (input: unknown) => invoke("create_task_category", { input }),
  update: (id: number, input: unknown) => invoke("update_task_category", { id, input }),
  delete: (id: number) => invoke("delete_task_category", { id }),
};
export const cardApi: any = {
  list: (deck?: string) => invoke("list_cards", { deck }),
  get: (id: number) => invoke("get_card", { id }),
  listDue: (limit?: number) => invoke("list_due_cards", { limit }),
  create: (input: unknown) => invoke("create_card", { input }),
  updateContent: (id: number, front: string, back: string) =>
    invoke("update_card_content", { id, front, back }),
  delete: (id: number) => invoke("delete_card", { id }),
  review: (input: unknown) => invoke("review_card", { input }),
  stats: () => invoke("get_card_stats"),
  listReviewLogs: (cardId: number, limit?: number) =>
    invoke("list_card_review_logs", { cardId, limit }),
};
export const dailyApi: DailyApi = {
  get: (date) => invoke("get_daily", { date }),
  getOrCreate: (date) => invoke("get_or_create_daily", { date }),
  listDates: (year, month) => invoke("list_daily_dates", { year, month }),
  getNeighbors: (date) => invoke("get_daily_neighbors", { date }),
};
export const searchApi = {
  search: (keyword: string, limit?: number): Promise<SearchResult[]> =>
    invoke("search_notes", { query: keyword, limit }),
};
export const linkApi = {
  syncLinks: (noteId: number, targetIds: number[]) =>
    invoke("sync_note_links", { sourceId: noteId, targetIds }),
  getBacklinks: (noteId: number): Promise<NoteLink[]> => invoke("get_backlinks", { noteId }),
  searchTargets: (keyword: string, limit?: number): Promise<Array<[number, string]>> =>
    invoke("search_link_targets", { keyword, limit }),
  findIdByTitle: (title: string): Promise<number | null> =>
    invoke("find_note_id_by_title_loose", { title }),
  getGraphData: (): Promise<GraphData> => invoke("get_graph_data"),
};
export const courseGraphApi = {
  getConfig: (): Promise<CourseGraphConfig> => invoke("course_graph_get_config"),
  health: (): Promise<CourseGraphHealth> => invoke("course_graph_health"),
  stats: (): Promise<CourseGraphStats> => invoke("course_graph_stats"),
  chapters: (): Promise<unknown> => invoke("course_graph_chapters"),
  expand: (elementId: string): Promise<unknown> =>
    invoke("course_graph_expand", { elementId }),
  search: (query: string, limit = 20): Promise<unknown> =>
    invoke("course_graph_search", { query, limit }),
  nodeDetail: (nodeId: string): Promise<unknown> =>
    invoke("course_graph_node_detail", { nodeId }),
  knowledge: (knowledgeId: string): Promise<unknown> =>
    invoke("course_graph_knowledge", { knowledgeId }),
  related: (nodeId: string): Promise<unknown> =>
    invoke("course_graph_related", { nodeId }),
  analyzeWithAi: (nodeId: string): Promise<CourseGraphAiAnalysis> =>
    invoke("course_graph_ai_analyze", { nodeId }),
  getAiAnalysis: (nodeId: string): Promise<CourseGraphAiAnalysis | null> =>
    invoke("course_graph_ai_get", { nodeId }),
  reviewAiRelation: (
    relationId: number,
    status: CourseGraphAiRelationStatus,
  ): Promise<CourseGraphAiRelation> =>
    invoke("course_graph_ai_review_relation", { input: { relationId, status } }),
  acceptedAiGraph: (nodeId: string): Promise<unknown> =>
    invoke("course_graph_ai_accepted_graph", { nodeId }),
};

export const researchApi = {
  searchPapers: (input: ResearchPaperSearchInput): Promise<ResearchPaperSearchResult> =>
    invoke("research_search_papers", { input }),
  recommendForKnowledgeBase: (
    input: ResearchPaperKnowledgeRecommendationInput,
  ): Promise<ResearchPaperKnowledgeRecommendation> =>
    invoke("research_recommend_for_knowledge_base", { input }),
  analyzePapers: (input: ResearchAnalysisInput): Promise<ResearchAnalysisResult> =>
    invoke("research_analyze_papers", { input }),
};
export const importApi = {
  scan: (path: string): Promise<ScannedFile[]> => invoke("scan_markdown_folder", { path }),
  scanSupportedFolder: (path: string): Promise<ScannedFile[]> =>
    invoke("scan_supported_import_folder", { path }),
  importSelected: (
    filePaths: string[],
    folderId?: number | null,
    rootPath?: string | null,
    preserveRoot?: boolean,
    policy?: unknown,
  ): Promise<ImportResult> =>
    invoke("import_selected_files", {
      filePaths,
      folderId: folderId ?? null,
      rootPath: rootPath ?? null,
      preserveRoot,
      policy,
    }),
  importMixed: (
    filePaths: string[],
    folderId?: number | null,
    rootPath?: string | null,
    preserveRoot?: boolean,
    policy?: unknown,
  ): Promise<ImportResult> =>
    invoke("import_mixed_files", {
      filePaths,
      folderId: folderId ?? null,
      rootPath: rootPath ?? null,
      preserveRoot,
      policy,
    }),
  openMarkdownFile: (filePath: string): Promise<OpenMarkdownResult> =>
    invoke("open_markdown_file", { filePath }),
  takePendingOpenMdPath: (): Promise<string | null> => invoke("take_pending_open_md_path"),
};
export const exportApi: any = {
  exportNotes: (outputDir: string, folderId?: number | null) =>
    invoke("export_notes", { outputDir, folderId: folderId ?? null }),
  exportSingle: (id: number, parentDir: string) =>
    invoke("export_single_note", { id, parentDir }),
  exportSingleToWord: (id: number, targetPath: string) =>
    invoke("export_single_note_to_word", { id, targetPath }),
  exportSingleToHtml: (id: number, targetPath: string) =>
    invoke("export_single_note_to_html", { id, targetPath }),
};
export const trashApi: TrashApi = {
  softDelete: (id) => invoke("soft_delete_note", { id }),
  list: (page, pageSize) => invoke("list_trash", { page, pageSize }),
  restore: (id) => invoke("restore_note", { id }),
  permanentDelete: (id) => invoke("permanent_delete_note", { id }),
  empty: () => invoke("empty_trash"),
  restoreBatch: (ids) => invoke("restore_notes_batch", { ids }),
  permanentDeleteBatch: (ids) => invoke("permanent_delete_notes_batch", { ids }),
};
export const hiddenApi: HiddenApi = {
  list: (options) =>
    invoke("list_hidden_notes", {
      page: options?.page,
      pageSize: options?.pageSize,
      folderId: options?.folderId,
      uncategorized: options?.uncategorized,
    }),
  listFolderIds: () => invoke("list_hidden_folder_ids"),
};
export const hiddenPinApi: HiddenPinApi = {
  isSet: () => invoke("is_hidden_pin_set"),
  getHint: () => invoke("get_hidden_pin_hint"),
  verify: (pin) => invoke("verify_hidden_pin", { pin }),
  set: (oldPin, newPin, hint = null) =>
    invoke("set_hidden_pin", { oldPin, newPin, hint }),
  clear: (currentPin) => invoke("clear_hidden_pin", { currentPin }),
};
export const configApi: ConfigApi = {
  get: (key) => invoke("get_config", { key }),
  set: (key, value) => invoke("set_config", { key, value }),
  delete: (key) => invoke("delete_config", { key }),
};
export const pptMasterApi: PptMasterApi = {
  check: (input) => invoke("ppt_master_check", { input }),
  export: (input) => invoke("ppt_master_export", { input }),
  generateFromPrompt: (input) => invoke("ppt_master_generate_from_prompt", { input }),
};
export const dataDirApi = {
  getInfo: (): Promise<ResolvedDataDir> => invoke("get_data_dir_info"),
  setPending: (newPath: string) => invoke("set_pending_data_dir", { newPath }),
  clearPending: () => invoke("clear_pending_data_dir"),
  setPendingWithMigration: (newPath: string) =>
    invoke("set_pending_data_dir_with_migration", { newPath }),
  cancelPendingMigration: () => invoke("cancel_pending_migration"),
  getMigrationMarker: (): Promise<MigrationMarker | null> => invoke("get_migration_marker"),
};
export const runtimeApi = {
  getDataDirectory: (): Promise<RuntimeDataDirectory> => invoke("get_runtime_data_directory"),
};
export const sourceFileApi = {
  getConverterStatus: (): Promise<DocConverter> => invoke("get_converter_status"),
  diagnoseConverter: () => invoke("diagnose_doc_converter"),
  readFileAsBase64: (path: string): Promise<string> => invoke("read_file_as_base64", { path }),
  convertDocToDocxBase64: (path: string): Promise<string> =>
    invoke("convert_doc_to_docx_base64", { path }),
  attachSourceFile: (noteId: number, sourcePath: string, fileType: string) =>
    invoke("attach_source_file", {
      noteId,
      sourcePath,
      fileType,
    }),
  getAbsolutePath: (noteId: number): Promise<string | null> =>
    invoke("get_source_file_absolute_path", { noteId }),
};
export const sourceWritebackApi: any = {
  writeBack: (noteId: number, force: boolean) =>
    invoke("write_back_source_md", { noteId, force }),
};
export const vaultApi: any = {
  status: () => invoke("vault_status"),
  setup: (password: string) => invoke("vault_setup", { password }),
  unlock: (password: string) => invoke("vault_unlock", { password }),
  lock: () => invoke("vault_lock"),
  encryptNote: (id: number) => invoke("encrypt_note", { id }),
  decryptNote: (id: number) => invoke("decrypt_note", { id }),
  disableEncrypt: (id: number) => invoke("disable_note_encrypt", { id }),
};
export const attachmentApi: any = {
  save: (noteId: number, fileName: string, base64Data: string) =>
    invoke("save_note_attachment", {
      noteId,
      fileName,
      base64Data,
    }),
  saveFromPath: (noteId: number, sourcePath: string) =>
    invoke("save_note_attachment_from_path", { noteId, sourcePath }),
  deleteAll: (noteId: number) => invoke("delete_note_attachments", { noteId }),
  getDir: () => invoke("get_attachments_dir"),
};
export const imageApi: any = {
  save: (noteId: number, fileName: string, base64Data: string) =>
    invoke("save_note_image", {
      noteId,
      fileName,
      base64Data,
    }),
  saveFromPath: (noteId: number, sourcePath: string) =>
    invoke("save_note_image_from_path", { noteId, sourcePath }),
  downloadFromUrl: (noteId: number, url: string, referer?: string) =>
    invoke("download_image_to_assets", { noteId, url, referer }),
  deleteAll: (noteId: number) => invoke("delete_note_images", { noteId }),
  getDir: () => invoke("get_images_dir"),
  getBlob: (path: string) => invoke("get_image_blob", { path }),
};
export const videoApi: any = {
  save: (noteId: number, fileName: string, data: Uint8Array) =>
    invoke("save_video", { noteId, fileName, data }),
  saveFromPath: (noteId: number, sourcePath: string) =>
    invoke("save_video_from_path", { noteId, sourcePath }),
  deleteAll: (noteId: number) => invoke("delete_note_videos", { noteId }),
  getDir: () => invoke("get_videos_dir"),
};
export const templateApi = {
  list: (): Promise<NoteTemplate[]> => invoke("list_templates"),
  get: (id: number) => invoke("get_template", { id }),
  create: (input: NoteTemplateInput) => invoke("create_template", { input }),
  update: (id: number, input: NoteTemplateInput) => invoke("update_template", { id, input }),
  delete: (id: number) => invoke("delete_template", { id }),
  createNoteFromTemplate: (templateId: number, title?: string, folderId?: number | null): Promise<Note> =>
    invoke("create_note_from_template", {
      templateId,
      title,
      folderId: folderId ?? null,
    }),
};
export const pdfApi = {
  importPdfs: (paths: string[], folderId?: number | null): Promise<PdfImportResult[]> =>
    invoke("import_pdfs", { paths, folderId: folderId ?? null }),
  getAbsolutePath: (noteId: number) => invoke("get_pdf_absolute_path", { noteId }),
  rebuildEditableNote: (noteId: number): Promise<Note> =>
    invoke("rebuild_pdf_note_as_editable", { noteId }),
};
export const autostartApi: any = {
  isEnabled: () => isAutostartEnabled(),
  enable: () => enableAutostart(),
  disable: () => disableAutostart(),
};
export const syncApi: any = {
  exportToFile: (scope: unknown, targetPath: string) =>
    invoke("sync_export_to_file", { scope, targetPath }),
  importFromFile: (sourcePath: string, mode: string) =>
    invoke("sync_import_from_file", { sourcePath, mode }),
  webdavTest: (url: string, username: string, password: string) =>
    invoke("sync_webdav_test", { url, username, password }),
  webdavPush: (scope: unknown, config: unknown) => invoke("sync_webdav_push", { scope, config }),
  webdavPull: (mode: string, config: unknown, filename?: string) =>
    invoke("sync_webdav_pull", { mode, config, filename }),
  webdavPreview: (config: unknown, filename?: string) =>
    invoke("sync_webdav_preview", { config, filename }),
  webdavListSnapshots: (config: unknown) => invoke("sync_webdav_list_snapshots", { config }),
  savePassword: (username: string, password: string) =>
    invoke("sync_save_webdav_password", { username, password }),
  hasPassword: (username: string) => invoke("sync_has_webdav_password", { username }),
  getPassword: (username: string) => invoke("sync_get_webdav_password", { username }),
  deletePassword: (username: string) => invoke("sync_delete_webdav_password", { username }),
  listHistory: (limit?: number): Promise<SyncHistoryItem[]> => invoke("sync_list_history", { limit }),
  schedulerReload: () => invoke("sync_scheduler_reload"),
};
export const syncV1Api = {
  listBackends: (): Promise<SyncBackend[]> => invoke("sync_v1_list_backends"),
  getBackend: (id: number) => invoke("sync_v1_get_backend", { id }),
  createBackend: (input: SyncBackendInput) => invoke("sync_v1_create_backend", { input }),
  updateBackend: (id: number, input: SyncBackendInput) => invoke("sync_v1_update_backend", { id, input }),
  deleteBackend: (id: number) => invoke("sync_v1_delete_backend", { id }),
  testConnection: (id: number) => invoke("sync_v1_test_connection", { id }),
  readRemoteManifest: (id: number) => invoke("sync_v1_read_remote_manifest", { id }),
  push: (id: number): Promise<SyncPushResult> => invoke("sync_v1_push", { id }),
  pull: (id: number): Promise<SyncPullResult> => invoke("sync_v1_pull", { id }),
  getLocalManifest: () => invoke("sync_v1_get_local_manifest"),
};
export const shortcutsApi = {
  list: (): Promise<ShortcutBinding[]> => invoke("list_shortcut_bindings"),
  set: (id: string, accel: string) => invoke("set_shortcut_binding", { id, accel }),
  reset: (id: string) => invoke("reset_shortcut_binding", { id }),
  disable: (id: string) => invoke("disable_shortcut_binding", { id }),
};
export const orphanAssetApi: any = {
  scanAll: (): Promise<OrphanAssetScan> => invoke("scan_orphan_assets"),
  clean: (items: unknown[]): Promise<OrphanAssetClean> => invoke("clean_orphan_assets", { items }),
};
export const asrApi = {
  getConfig: (): Promise<AsrConfig> => invoke("asr_get_config"),
  saveConfig: (config: unknown) => invoke("asr_save_config", { config }),
  testConnection: (config: unknown): Promise<AsrTestResult> => invoke("asr_test_connection", { config }),
  transcribe: (request: unknown): Promise<TranscribeResult> => invoke("asr_transcribe_audio", { request }),
};
export const promptApi: PromptApi = {
  list: (enabledOnly) => invoke("list_prompts", { onlyEnabled: enabledOnly }),
  get: (id) => invoke("get_prompt", { id }),
  create: (input) => invoke("create_prompt", { input }),
  update: (id, input) => invoke("update_prompt", { id, input }),
  delete: (id) => invoke("delete_prompt", { id }),
  setEnabled: (id, enabled) => invoke("set_prompt_enabled", { id, enabled }),
};
export const pluginApi: PluginApi = {
  list: () => invoke("list_plugins"),
  scan: () => invoke("scan_plugins"),
  installFromDir: (path) => invoke("install_plugin_from_dir", { path }),
  enable: (pluginId) => invoke("enable_plugin", { pluginId }),
  disable: (pluginId) => invoke("disable_plugin", { pluginId }),
  uninstall: (pluginId) => invoke("uninstall_plugin", { pluginId }),
  getManifest: (pluginId) => invoke("get_plugin_manifest", { pluginId }),
  grantPermissions: (pluginId, permissions) =>
    invoke("grant_plugin_permissions", { pluginId, permissions }),
  revokePermissions: (pluginId, permissions) =>
    invoke("revoke_plugin_permissions", { pluginId, permissions }),
  getSettings: (pluginId) => invoke("get_plugin_settings", { pluginId }),
  setSettings: (pluginId, settings) => invoke("set_plugin_settings", { pluginId, settings }),
  readAsset: (pluginId, relativePath) =>
    invoke("read_plugin_asset", { pluginId, relativePath }),
  getAuditLog: (pluginId, limit) => invoke("plugin_audit_log_list", { pluginId, limit }),
  parseManifest: (path) => invoke("parse_plugin_manifest", { path }),
  validateManifest: (path) => invoke("validate_plugin_manifest", { path }),
  inspectPackage: (path) => invoke("inspect_plugin_package", { path }),
  calculateIntegrity: (path) => invoke("calculate_plugin_integrity", { path }),
  comparePermissions: (current, next) =>
    invoke("compare_plugin_permissions", { current, next }),
  checkCompatibility: (minAppVersion) =>
    invoke("check_plugin_compatibility", { minAppVersion: minAppVersion ?? null }),
  getInstallation: (pluginId) => invoke("get_plugin_installation", { pluginId }),
  listInstallations: () => invoke("list_plugin_installations"),
  verifyInstallation: (pluginId) => invoke("verify_plugin_installation", { pluginId }),
  canExecuteRuntime: (pluginId) => invoke("can_execute_plugin_runtime", { pluginId }),
  inspectArchive: (path) => invoke("plugin_inspect_archive", { path }),
  installArchive: (input) => invoke("plugin_install_archive", { input }),
  updateArchive: (input) => invoke("plugin_update_archive", { input }),
  rollback: (pluginId, version) => invoke("plugin_rollback", { pluginId, version }),
  listVersions: (pluginId) => invoke("plugin_list_versions", { pluginId }),
  getActivationSettings: (pluginId) =>
    invoke("plugin_get_activation_settings", { pluginId }),
  setActivationSetting: (pluginId, scopeType, scopeKey, enabled) =>
    invoke("plugin_set_activation_setting", { pluginId, scopeType, scopeKey, enabled }),
  resolveEnabledContributions: (context) =>
    invoke("plugin_resolve_enabled_contributions", { context }),
  recordExecution: (input) => invoke("plugin_record_execution", { input }),
  invokeXingchenFeature: (input) => invoke("plugin_feature_invoke_xingchen", { input }),
  listDocumentSummaryToolbarButtons: () => invoke("plugin_document_summary_toolbar_buttons"),
  mockDocumentSummary: (input) => invoke("plugin_document_mock_summary", { input }),
  recordDocumentSummaryInsert: (input) => invoke("plugin_document_summary_insert", { input }),
  listDocumentSummaryAgents: (pluginId) => invoke("plugin_document_summary_agents", { pluginId }),
  getDocumentSummaryConfig: (pluginId) => invoke("plugin_document_summary_config_get", { pluginId }),
  setDocumentSummaryConfig: (input) => invoke("plugin_document_summary_config_set", { input }),
  startDocumentSummaryAgent: (input) => invoke("plugin_document_summary_agent_start", { input }),
  cancelDocumentSummary: (input) => invoke("plugin_document_summary_cancel", { input }),
  finalizeDocumentSummaryAgent: (input) => invoke("plugin_document_summary_agent_finalize", { input }),
};

export const planningApi: PlanningApi = {
  getWorkspace: (sessionKind, sessionId) =>
    invoke("planning_get_workspace", { input: { sessionKind, sessionId } }),
  setEnabled: (sessionKind, sessionId, enabled) =>
    invoke("planning_set_enabled", { input: { sessionKind, sessionId, enabled } }),
  saveFile: (sessionKind, sessionId, fileName, content) =>
    invoke("planning_save_file", { input: { sessionKind, sessionId, fileName, content } }),
  applyUpdate: (sessionKind, sessionId, accept) =>
    invoke("planning_apply_update", { input: { sessionKind, sessionId, accept } }),
  clear: (sessionKind, sessionId, confirm) =>
    invoke("planning_clear", { input: { sessionKind, sessionId, confirm } }),
  export: (sessionKind, sessionId, targetDir) =>
    invoke("planning_export", { input: { sessionKind, sessionId, targetDir } }),
};

export const marketplaceApi: MarketplaceApi = {
  listAccounts: () => invoke("marketplace_list_accounts"),
  switchAccount: (userId) => invoke("marketplace_switch_account", { userId }),
  updateAccount: (input) => invoke("marketplace_update_account", { input }),
  applyDeveloper: () => invoke("marketplace_apply_developer"),
  listProducts: (query) => invoke("marketplace_list_products", { query: query ?? null }),
  searchProducts: (query) => invoke("marketplace_search_products", { query }),
  getProduct: (productId) => invoke("marketplace_get_product", { productId }),
  getProductVersion: (productId, version = null) =>
    invoke("marketplace_get_product_version", { productId, version }),
  acquireProduct: (input) => invoke("marketplace_acquire_product", { input }),
  bindExternalAuthorization: (input) =>
    invoke("marketplace_bind_external_authorization", { input }),
  listEntitlements: () => invoke("marketplace_list_entitlements"),
  installProduct: (input) => invoke("marketplace_install_product", { input }),
  updateProduct: (input) => invoke("marketplace_update_product", { input }),
  uninstallProduct: (productId) => invoke("marketplace_uninstall_product", { productId }),
  enableProduct: (productId) => invoke("marketplace_enable_product", { productId }),
  disableProduct: (productId) => invoke("marketplace_disable_product", { productId }),
  recordPermissionRejection: (input) => invoke("marketplace_record_permission_rejection", { input }),
  configureService: (input) => invoke("marketplace_configure_service", { input }),
  listInstalled: () => invoke("marketplace_list_installed"),
  checkUpdates: () => invoke("marketplace_check_updates"),
  verifyInstallation: (productId) => invoke("marketplace_verify_installation", { productId }),
  devRevokeProductVersion: (productId, version = null) =>
    invoke("marketplace_dev_revoke_product_version", { productId, version }),
  devRestoreProductVersion: (productId, version = null) =>
    invoke("marketplace_dev_restore_product_version", { productId, version }),
  mockTestProduct: (productId) => invoke("marketplace_mock_test_product", { productId }),
  getMockSession: () => invoke("marketplace_get_mock_session"),
  switchMockRole: (role) => invoke("marketplace_switch_mock_role", { role }),
  listOrders: () => invoke("marketplace_list_orders"),
  listLedger: () => invoke("marketplace_list_ledger"),
  requestRefund: (input) => invoke("marketplace_request_refund", { input }),
  listReviews: (productId) => invoke("marketplace_list_reviews", { productId }),
  submitReview: (input) => invoke("marketplace_submit_review", { input }),
};

export const developerApi: DeveloperApi = {
  listProducts: () => invoke("developer_list_products"),
  createProduct: (input) => invoke("developer_create_product", { input }),
  updateProduct: (productId, input) => invoke("developer_update_product", { productId, input }),
  createVersion: (input) => invoke("developer_create_version", { input }),
  uploadPackage: (input) => invoke("developer_upload_package", { input }),
  getPackageReport: (productId, version) =>
    invoke("developer_get_package_report", { productId, version }),
  submitProduct: (input) => invoke("developer_submit_product", { input }),
  submitVersion: (input) => invoke("developer_submit_version", { input }),
  listEarnings: () => invoke("developer_list_earnings"),
  getDashboard: () => invoke("developer_get_dashboard"),
};

export const adminMarketplaceApi: AdminMarketplaceApi = {
  listSubmissions: (status = null) => invoke("admin_list_submissions", { status }),
  getSubmission: (submissionId) => invoke("admin_get_submission", { submissionId }),
  startReview: (input) => invoke("admin_start_review", { input }),
  approveSubmission: (input) => invoke("admin_approve_submission", { input }),
  rejectSubmission: (input) => invoke("admin_reject_submission", { input }),
  suspendProduct: (input) => invoke("admin_suspend_product", { input }),
  restoreProduct: (input) => invoke("admin_restore_product", { input }),
  delistProduct: (input) => invoke("admin_delist_product", { input }),
  revokeVersion: (input) => invoke("admin_revoke_version", { input }),
};

export const credentialApi: CredentialApi = {
  list: () => invoke("credential_list"),
  create: (input) => invoke("credential_create", { input }),
  update: (id, input) => invoke("credential_update", { id, input }),
  delete: (id, force = false) => invoke("credential_delete", { id, force }),
  getUsage: (id) => invoke("credential_get_usage", { id }),
};

export const externalAgentApi: ExternalAgentApi = {
  list: () => invoke("external_agent_list"),
  listBindableProducts: () => invoke("external_agent_list_bindable_products"),
  create: (input) => invoke("external_agent_create", { input }),
  update: (id, input) => invoke("external_agent_update", { id, input }),
  delete: (id) => invoke("external_agent_delete", { id }),
  testConnection: (id) => invoke("external_agent_test_connection", { id }),
  healthCheck: (id) => invoke("external_agent_health_check", { id }),
  listSessions: (externalAgentId = null) =>
    invoke("agent_session_list", { externalAgentId }),
  createSession: (input) => invoke("agent_session_create", { input }),
  deleteSession: (id) => invoke("agent_session_delete", { id }),
  listMessages: (sessionId) => invoke("agent_message_list", { sessionId }),
  sendMessage: (input) => invoke("agent_send_message", { input }),
  finalizePluginOutput: (sessionId, requestId, expectedOutput, finalOutput) =>
    invoke("agent_finalize_plugin_output", {
      sessionId,
      requestId,
      expectedOutput,
      finalOutput,
    }),
  invokeWorkflow: (input) => invoke("agent_workflow_invoke", { input }),
  cancelRequest: (requestId) => invoke("agent_cancel_request", { requestId }),
  listUsage: (externalAgentId = null) =>
    invoke("agent_usage_list", { externalAgentId }),
  clearUsage: (externalAgentId = null) =>
    invoke("agent_usage_clear", { externalAgentId }),
};

export interface DocumentSourceInfo {
  id: number;
  displayName: string;
  originalFileName: string;
  fileExtension: string;
  mimeType: string;
  category: string;
  sourceModule: string;
  isBuiltin: boolean;
  isEnabled: boolean;
  isAvailable: boolean;
  fileSize: number;
  createdAt: string;
  updatedAt: string;
}

export interface DocumentSourceListResult {
  sources: DocumentSourceInfo[];
  warnings: string[];
}

export interface DocumentTreeNode {
  id: string;
  parentId: string | null;
  name: string;
  nodeType: "folder" | "file";
  sourceType: "localKnowledgeBase" | "learningUpload" | "userDocument";
  systemFolder: boolean;
  folderId: number | null;
  documentSourceId: number | null;
  fileType: string | null;
  mimeType: string | null;
  size: number | null;
  parseStatus: "ready" | "parsing" | "failed" | "unsupported" | null;
  parseMessage: string | null;
  canUseAsLearningSource: boolean;
  childCount: number;
  children: DocumentTreeNode[];
}

export interface DocumentTreeResult {
  roots: DocumentTreeNode[];
  warnings: string[];
}

export const documentSourceApi = {
  list: (input?: { category?: string; sourceModule?: string; fileExtension?: string }) =>
    invoke<DocumentSourceListResult>("document_source_list", { input: input ?? null }),
  importLearning: (sourcePath: string) =>
    invoke<DocumentSourceInfo>("document_source_import_learning", { sourcePath }),
  delete: (id: number) => invoke<void>("document_source_delete", { id }),
};

export const documentTreeApi = {
  list: (forceRefresh = false) =>
    invoke<DocumentTreeResult>("document_tree_list", { forceRefresh }),
};

export const sessionApi = {
  parsePlan: (path: string): Promise<ParsedPlan> => invoke("parse_plan_file", { path }),
  create: (path: string): Promise<TaskSession> => invoke("create_task_session", { planPath: path }),
  get: (sessionId: string): Promise<TaskSessionDetail> => invoke("get_task_session", { sessionId }),
  list: (): Promise<TaskSession[]> => invoke("list_task_sessions"),
  startPhase: (sessionId: string, phaseIndex: number) =>
    invoke("start_session_phase", { sessionId, phaseIndex }),
  confirmPhase: (sessionId: string) => invoke("confirm_session_phase", { sessionId }),
  skipPhase: (sessionId: string, phaseIndex: number) =>
    invoke("skip_session_phase", { sessionId, phaseIndex }),
  retryPhase: (sessionId: string, phaseIndex: number) =>
    invoke("retry_session_phase", { sessionId, phaseIndex }),
  pause: (sessionId: string) => invoke("pause_task_session", { sessionId }),
  resume: (sessionId: string) => invoke("resume_task_session", { sessionId }),
  delete: (sessionId: string) => invoke("delete_task_session", { sessionId }),
  exportLogs: (sessionId: string) => invoke("export_execution_logs", { sessionId }),
};
export const projectSessionApi = {
  open: (projectPath: string, projectName?: string): Promise<ProjectSession> =>
    invoke("open_project_session", { projectPath, projectName }),
  listOpen: (): Promise<ProjectSession[]> => invoke("list_open_project_sessions"),
  listRecent: () => invoke("list_recent_project_sessions"),
  setActive: (sessionId: string) => invoke("set_active_project_session", { sessionId }),
  close: (sessionId: string) => invoke("close_project_session", { sessionId }),
  getContext: (sessionId: string) => invoke("get_project_session_context", { sessionId }),
  appendMessage: (sessionId: string, role: string, content: string): Promise<ProjectSessionMessage> =>
    invoke("append_project_session_message", {
      input: { sessionId, role, content },
    }),
  listMessages: (sessionId: string): Promise<ProjectSessionMessage[]> =>
    invoke("list_project_session_messages", { sessionId }),
};
