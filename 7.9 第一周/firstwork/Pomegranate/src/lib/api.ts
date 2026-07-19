import { invoke } from "@tauri-apps/api/core";
import type {
  AiConversation,
  AiMessage,
  AiModel,
  AiModelInput,
  AiModelTestResult,
  CreateTaskInput,
  DailyWritingStat,
  DashboardStats,
  DocConverter,
  Folder,
  Note,
  NoteInput,
  NoteQuery,
  PageResult,
  PdfImportResult,
  PptMasterCheckInput,
  PptMasterCheckResult,
  PptMasterExportInput,
  PptMasterExportResult,
  PptMasterGenerateInput,
  PptMasterGenerateResult,
  PluginAuditLogEntry,
  PluginInfo,
  PluginManifest,
  PromptTemplate,
  PromptTemplateInput,
  ScannedFile,
  RestoreBatchResult,
  SearchResult,
  ShortcutBinding,
  SyncBackend,
  SyncBackendInput,
  SyncPullResult,
  SyncPushResult,
  SystemInfo,
  Tag,
  Task,
  TaskLinkInput,
  TaskQuery,
  TaskSearchHit,
  TaskSession,
  TaskSessionDetail,
  TaskStats,
  UpdateTaskInput,
} from "@/types";

type DynamicApi = Record<string, (...args: any[]) => Promise<any>>;
type ApiShape<T extends object> = T & DynamicApi;
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
  createConversation(title?: string): Promise<AiConversation>;
  setAttachedNotes(conversationId: number, noteIds: number[]): Promise<void>;
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

interface SearchApi {
  search(keyword: string, limit?: number): Promise<SearchResult[]>;
}

interface LinkApi {
  searchTargets(keyword: string, limit?: number): Promise<Array<[number, string]>>;
}

interface ImportApi {
  scan(rootPath: string): Promise<ScannedFile[]>;
}

interface ConfigApi {
  get(key: string): Promise<string>;
  set(key: string, value: string): Promise<void>;
  delete(key: string): Promise<void>;
}

interface SourceFileApi {
  getConverterStatus(): Promise<DocConverter>;
  readFileAsBase64(path: string): Promise<string>;
  convertDocToDocxBase64(path: string): Promise<string>;
  attachSourceFile(noteId: number, sourcePath: string, fileType: string): Promise<string>;
}

interface ImageApi {
  getBlob(path: string): Promise<Uint8Array>;
}

interface PdfApi {
  importPdfs(paths: string[], folderId?: number | null): Promise<PdfImportResult[]>;
}

interface SyncV1Api {
  listBackends(): Promise<SyncBackend[]>;
  createBackend(input: SyncBackendInput): Promise<number>;
  updateBackend(id: number, input: SyncBackendInput): Promise<void>;
  deleteBackend(id: number): Promise<boolean>;
  testConnection(id: number): Promise<void>;
  push(id: number): Promise<SyncPushResult>;
  pull(id: number): Promise<SyncPullResult>;
}

interface ShortcutsApi {
  list(): Promise<ShortcutBinding[]>;
  set(id: string, accel: string): Promise<void>;
  reset(id: string): Promise<void>;
  disable(id: string): Promise<void>;
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

interface SessionApi {
  list(): Promise<TaskSession[]>;
  get(id: string): Promise<TaskSessionDetail>;
}

function toSnakeCase(value: string) {
  return value.replace(/[A-Z]/g, (m) => `_${m.toLowerCase()}`);
}

function createApi<T extends object>(prefix: string): ApiShape<T> {
  return new Proxy(
    {},
    {
      get(_target, prop) {
        if (typeof prop !== "string") return undefined;
        return async (...args: any[]) => {
          const command = `${prefix}_${toSnakeCase(prop)}`;
          if (args.length === 0) return invoke(command);
          if (args.length === 1 && typeof args[0] === "object" && !Array.isArray(args[0])) {
            return invoke(command, args[0]);
          }
          return invoke(command, { args });
        };
      },
    },
  ) as ApiShape<T>;
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
export const updaterApi = createApi("updater");
export const aiChatApi = createApi<AiChatApi>("ai");
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
    invoke("ai_write_assist", { action, selected_text: selectedText, context }),
  cancel: () => invoke("cancel_ai_write_assist"),
  suggestPrompt: (selectedText, context) =>
    invoke("ai_suggest_prompt", { selected_text: selectedText, context }),
  understandPpt: ({ prompt, modelId }) => invoke("ai_ppt_understand", { input: { prompt, modelId } }),
};
export const aiPlanApi = createApi("ai_plan");
export const aiAttachmentApi = createApi("ai");
export const noteApi: NoteApi = {
  list: (query) => invoke("list_notes", { query: query ?? {} }),
  get: (id) => invoke("get_note", { id }),
  create: (input) => invoke("create_note", { input }),
  update: (id, input) => invoke("update_note", { id, input }),
  delete: (id) => invoke("delete_note", { id }),
  togglePin: (id) => invoke("toggle_pin", { id }),
  moveToFolder: (noteId, folderId = null) =>
    invoke("move_note_to_folder", { note_id: noteId, folder_id: folderId }),
  reorder: (orderedIds) => invoke("reorder_notes", { ordered_ids: orderedIds }),
  moveBatch: (ids, folderId = null) => invoke("move_notes_batch", { ids, folder_id: folderId }),
  addTagsBatch: (noteIds, tagIds) =>
    invoke("add_tags_to_notes_batch", { note_ids: noteIds, tag_ids: tagIds }),
  trashBatch: (ids) => invoke("trash_notes_batch", { ids }),
  trashAll: () => invoke("trash_all_notes"),
  setHidden: (id, hidden) => invoke("set_note_hidden", { id, hidden }),
  clipUrl: (url, folderId = null) => invoke("clip_url_to_note", { url, folder_id: folderId }),
  openInNewWindow: (noteId) => invoke("open_note_in_new_window", { note_id: noteId }),
};
export const folderApi: FolderApi = {
  list: () => invoke("list_folders"),
  create: (name, parentId = null) => invoke("create_folder", { name, parent_id: parentId }),
  rename: (id, name) => invoke("rename_folder", { id, name }),
  delete: (id) => invoke("delete_folder", { id }),
  move: (id, newParentId = null) => invoke("move_folder", { id, new_parent_id: newParentId }),
  reorder: (orderedIds) => invoke("reorder_folders", { ordered_ids: orderedIds }),
  ensurePath: (path) => invoke("ensure_folder_path", { path }),
};
export const tagApi: TagApi = {
  list: () => invoke("list_tags"),
  create: (name, color = null) => invoke("create_tag", { name, color }),
  rename: (id, name) => invoke("rename_tag", { id, name }),
  setColor: (id, color = null) => invoke("set_tag_color", { id, color }),
  delete: (id) => invoke("delete_tag", { id }),
  addToNote: (noteId, tagId) => invoke("add_tag_to_note", { note_id: noteId, tag_id: tagId }),
  removeFromNote: (noteId, tagId) =>
    invoke("remove_tag_from_note", { note_id: noteId, tag_id: tagId }),
  getNoteTags: (noteId) => invoke("get_note_tags", { note_id: noteId }),
  listNotesByTag: (tagId, page, pageSize) =>
    invoke("list_notes_by_tag", { tag_id: tagId, page, page_size: pageSize }),
};
export const taskApi: TaskApi = {
  list: (query) => (query === undefined ? invoke("list_tasks") : invoke("list_tasks", { query })),
  get: (id) => invoke("get_task", { id }),
  search: (query, limit) =>
    limit === undefined ? invoke("search_tasks", { query }) : invoke("search_tasks", { query, limit }),
  stats: () => invoke("get_task_stats"),
  listSubtasks: (parentTaskId) => invoke("list_subtasks", { parent_id: parentTaskId }),
  create: (input) => invoke("create_task", { input }),
  update: (id, input) => invoke("update_task", { id, input }),
  delete: (id) => invoke("delete_task", { id }),
  deleteBatch: (ids) => invoke("delete_tasks_batch", { ids }),
  completeBatch: (ids) => invoke("complete_tasks_batch", { ids }),
  toggleStatus: (id) => invoke("toggle_task_status", { id }),
  addLink: (taskId, input) => invoke("add_task_link", { task_id: taskId, input }),
  removeLink: (linkId) => invoke("remove_task_link", { link_id: linkId }),
  snooze: (id, minutes) => invoke("snooze_task_reminder", { id, minutes }),
  completeOccurrence: (id) => invoke("complete_task_occurrence", { id }),
};
export const taskCategoryApi = createApi("task_category");
export const cardApi = createApi("card");
export const dailyApi: DailyApi = {
  get: (date) => invoke("get_daily", { date }),
  getOrCreate: (date) => invoke("get_or_create_daily", { date }),
  listDates: (year, month) => invoke("list_daily_dates", { year, month }),
  getNeighbors: (date) => invoke("get_daily_neighbors", { date }),
};
export const searchApi = createApi<SearchApi>("search");
export const linkApi = createApi<LinkApi>("link");
export const importApi = createApi<ImportApi>("import");
export const exportApi = createApi("export");
export const trashApi: TrashApi = {
  softDelete: (id) => invoke("soft_delete_note", { id }),
  list: (page, pageSize) => invoke("list_trash", { page, page_size: pageSize }),
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
      page_size: options?.pageSize,
      folder_id: options?.folderId,
      uncategorized: options?.uncategorized,
    }),
  listFolderIds: () => invoke("list_hidden_folder_ids"),
};
export const hiddenPinApi: HiddenPinApi = {
  isSet: () => invoke("is_hidden_pin_set"),
  getHint: () => invoke("get_hidden_pin_hint"),
  verify: (pin) => invoke("verify_hidden_pin", { pin }),
  set: (oldPin, newPin, hint = null) =>
    invoke("set_hidden_pin", { old_pin: oldPin, new_pin: newPin, hint }),
  clear: (currentPin) => invoke("clear_hidden_pin", { current_pin: currentPin }),
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
export const dataDirApi = createApi("data_dir");
export const sourceFileApi = createApi<SourceFileApi>("source_file");
export const sourceWritebackApi = createApi("source_writeback");
export const vaultApi = createApi("vault");
export const attachmentApi = createApi("attachment");
export const imageApi = createApi<ImageApi>("image");
export const videoApi = createApi("video");
export const templateApi = createApi("template");
export const pdfApi = createApi<PdfApi>("pdf");
export const autostartApi = createApi("autostart");
export const syncApi = createApi("sync");
export const syncV1Api = createApi<SyncV1Api>("sync_v1");
export const shortcutsApi = createApi<ShortcutsApi>("shortcut");
export const orphanAssetApi = createApi("orphan");
export const asrApi = createApi("asr");
export const promptApi: PromptApi = {
  list: (enabledOnly) => invoke("list_prompts", { only_enabled: enabledOnly }),
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
  enable: (pluginId) => invoke("enable_plugin", { plugin_id: pluginId }),
  disable: (pluginId) => invoke("disable_plugin", { plugin_id: pluginId }),
  uninstall: (pluginId) => invoke("uninstall_plugin", { plugin_id: pluginId }),
  getManifest: (pluginId) => invoke("get_plugin_manifest", { plugin_id: pluginId }),
  grantPermissions: (pluginId, permissions) =>
    invoke("grant_plugin_permissions", { plugin_id: pluginId, permissions }),
  revokePermissions: (pluginId, permissions) =>
    invoke("revoke_plugin_permissions", { plugin_id: pluginId, permissions }),
  getSettings: (pluginId) => invoke("get_plugin_settings", { plugin_id: pluginId }),
  setSettings: (pluginId, settings) => invoke("set_plugin_settings", { plugin_id: pluginId, settings }),
  readAsset: (pluginId, relativePath) =>
    invoke("read_plugin_asset", { plugin_id: pluginId, relative_path: relativePath }),
  getAuditLog: (pluginId, limit) => invoke("plugin_audit_log_list", { plugin_id: pluginId, limit }),
};
export const sessionApi = createApi<SessionApi>("session");
export const projectSessionApi = createApi("project_session");
