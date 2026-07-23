import {
  folderApi as localFolderApi,
  dailyApi as localDailyApi,
  hiddenApi as localHiddenApi,
  noteApi as localNoteApi,
  tagApi as localTagApi,
  trashApi as localTrashApi,
} from "@/lib/api";
import type { Folder, Note, NoteInput, NoteQuery, PageResult, SearchResult, Tag } from "@/types";
import {
  accountDocumentsApi,
  type AccountDocument,
  type AccountDocumentFile,
  type AccountDocumentFolder,
  type AccountDocumentTag,
} from "./accountDocumentsApi";
import { normalizeCreateMarkdownInput } from "./createMarkdownInput";
import { documentSource, isAccountDocumentSource } from "./documentSource";
import {
  assertCurrentDocumentRequest,
  captureDocumentRequest,
  getDocumentAccountKey,
  subscribeDocumentAccountReset,
} from "./documentSession";

export interface AccountBackedNote extends Note {
  document_kind: "markdown" | "uploaded_file";
  account_document_id: string;
  revision: number;
  content_sha256: string | null;
  deleted_at: string | null;
  account_folder_id: string | null;
  account_tag_ids: string[];
  account_file: AccountDocumentFile | null;
}

type IdMaps = {
  forward: Map<string, number>;
  reverse: Map<number, string>;
};

const documentIds: IdMaps = { forward: new Map(), reverse: new Map() };
const folderIds: IdMaps = { forward: new Map(), reverse: new Map() };
const tagIds: IdMaps = { forward: new Map(), reverse: new Map() };
const documentCache = new Map<string, AccountDocument>();

function clearAccountMappings(): void {
  for (const maps of [documentIds, folderIds, tagIds]) {
    maps.forward.clear();
    maps.reverse.clear();
  }
  documentCache.clear();
}

subscribeDocumentAccountReset(clearAccountMappings);

function fnv1a(value: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

function registerId(uuid: string, maps: IdMaps): number {
  const existing = maps.forward.get(uuid);
  if (existing !== undefined) return existing;
  let candidate = -((fnv1a(uuid) & 0x3fffffff) + 1);
  while (maps.reverse.has(candidate) && maps.reverse.get(candidate) !== uuid) candidate -= 1;
  maps.forward.set(uuid, candidate);
  maps.reverse.set(candidate, uuid);
  return candidate;
}

function requireMappedId(id: number, maps: IdMaps, kind: string): string {
  const value = maps.reverse.get(id);
  if (!value) throw { code: "notFound", message: `${kind}不存在或无权访问` };
  return value;
}

function assertSignedIn(): void {
  if (!getDocumentAccountKey()) {
    throw { code: "signedOut", message: "请先登录" };
  }
}

async function guarded<T>(request: () => Promise<T>): Promise<T> {
  assertSignedIn();
  const identity = captureDocumentRequest();
  const result = await request();
  assertCurrentDocumentRequest(identity);
  return result;
}

function fileType(file: AccountDocumentFile | null): string | null {
  if (!file) return null;
  const extension = file.originalName.split(".").pop()?.toLowerCase();
  return extension && extension !== file.originalName.toLowerCase()
    ? extension
    : file.mimeType?.split("/").pop() || "file";
}

export function mapAccountDocument(document: AccountDocument): AccountBackedNote {
  documentCache.set(document.id, document);
  const folderId = document.folder ? registerId(document.folder.id, folderIds) : null;
  for (const tag of document.tags) registerId(tag.id, tagIds);
  return {
    id: registerId(document.id, documentIds),
    title: document.title,
    content: document.markdownContent ?? "",
    folder_id: folderId,
    is_daily: document.diaryDate !== null,
    daily_date: document.diaryDate,
    is_pinned: document.isPinned,
    is_hidden: document.isHidden,
    is_encrypted: false,
    word_count: document.wordCount,
    created_at: document.createdAt,
    updated_at: document.updatedAt,
    source_file_path: null,
    source_file_type: fileType(document.file),
    sort_order: document.sortOrder,
    document_kind: document.kind,
    account_document_id: document.id,
    revision: document.revision,
    content_sha256: document.contentSha256,
    deleted_at: document.deletedAt,
    account_folder_id: document.folder?.id ?? null,
    account_tag_ids: document.tags.map((tag) => tag.id),
    account_file: document.file,
  };
}

export async function importEditableMarkdownFile(): Promise<AccountBackedNote | null> {
  if (!isAccountDocumentSource) {
    throw { code: "unsupported", message: "当前文档来源不支持账号 Markdown 导入" };
  }
  const result = await guarded(() => accountDocumentsApi.importEditableMarkdown());
  return result.status === "cancelled" ? null : mapAccountDocument(result.document);
}

function mapFolder(folder: AccountDocumentFolder): Folder {
  return {
    id: registerId(folder.id, folderIds),
    name: folder.name,
    parent_id: folder.parentId ? registerId(folder.parentId, folderIds) : null,
    sort_order: 0,
    children: [],
    note_count: 0,
  };
}

function buildFolderTree(items: AccountDocumentFolder[]): Folder[] {
  const nodes = new Map<string, Folder>();
  for (const item of items) nodes.set(item.id, mapFolder(item));
  const roots: Folder[] = [];
  for (const item of items) {
    const node = nodes.get(item.id)!;
    const parent = item.parentId ? nodes.get(item.parentId) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
  }
  return roots;
}

function mapTag(tag: AccountDocumentTag, noteCount = 0): Tag {
  return {
    id: registerId(tag.id, tagIds),
    name: tag.name,
    color: null,
    note_count: noteCount,
  };
}

async function listAccountDocuments(filters: {
  hidden?: boolean;
  deleted?: boolean;
  folderId?: number | null;
  tagId?: number | null;
  diaryDate?: string | null;
} = {}): Promise<AccountDocument[]> {
  return guarded(() =>
    accountDocumentsApi.list({
      hidden: filters.hidden ?? false,
      deleted: filters.deleted ?? false,
      folderId:
        filters.folderId == null ? undefined : requireMappedId(filters.folderId, folderIds, "文件夹"),
      tagId: filters.tagId == null ? undefined : requireMappedId(filters.tagId, tagIds, "标签"),
      diaryDate: filters.diaryDate ?? undefined,
      limit: 100,
      offset: 0,
    }),
  );
}

async function findAccountDocument(id: number): Promise<AccountDocument> {
  const uuid = documentIds.reverse.get(id);
  if (uuid && documentCache.has(uuid)) return documentCache.get(uuid)!;
  const pages = await Promise.all([
    listAccountDocuments({ hidden: false, deleted: false }),
    listAccountDocuments({ hidden: true, deleted: false }),
    listAccountDocuments({ hidden: false, deleted: true }),
    listAccountDocuments({ hidden: true, deleted: true }),
  ]);
  for (const item of pages.flat()) mapAccountDocument(item);
  const resolved = documentIds.reverse.get(id);
  const document = resolved ? documentCache.get(resolved) : undefined;
  if (!document) throw { code: "notFound", message: "文档不存在或无权访问" };
  return document;
}

function accountUpdateInput(document: AccountDocument, input: NoteInput) {
  return {
    expectedRevision: document.revision,
    title: input.title,
    markdownContent: input.content,
    folderId:
      input.folder_id === undefined
        ? document.folder?.id ?? null
        : input.folder_id === null
          ? null
          : requireMappedId(input.folder_id, folderIds, "文件夹"),
  };
}

export function isAccountUploadedNote(note: Note): note is AccountBackedNote {
  return (note as Partial<AccountBackedNote>).document_kind === "uploaded_file";
}

export function isAccountMarkdownNote(note: Note): note is AccountBackedNote {
  return (note as Partial<AccountBackedNote>).document_kind === "markdown";
}

export async function openAccountUploadedNote(note: Note, allowUnknown = false) {
  if (!isAccountUploadedNote(note) || !note.account_file) {
    throw { code: "notFound", message: "文件不存在或无权访问" };
  }
  return guarded(() => accountDocumentsApi.openUploadedFile(note.account_file!, allowUnknown));
}

export async function beginAccountUploadedEdit(note: Note) {
  if (!isAccountUploadedNote(note) || !note.account_file) {
    throw { code: "notFound", message: "文件不存在或无权访问" };
  }
  return guarded(() => accountDocumentsApi.beginUploadedEdit(note.account_document_id, note.account_file!));
}

export async function checkAccountUploadedEdit(workspaceId: string) {
  return guarded(() => accountDocumentsApi.checkUploadedEdit(workspaceId));
}

export async function syncAccountUploadedEdit(workspaceId: string) {
  return guarded(() => accountDocumentsApi.syncUploadedEdit(workspaceId));
}

export async function discardAccountUploadedEdit(workspaceId: string) {
  return guarded(() => accountDocumentsApi.discardUploadedEdit(workspaceId));
}

export async function prepareAccountUploadedMaterial(note: Note): Promise<string> {
  if (!isAccountUploadedNote(note) || !note.account_file) {
    throw { code: "notFound", message: "文件不存在或无权访问" };
  }
  const result = await guarded(() => accountDocumentsApi.prepareUploadedMaterial(note.account_file!));
  return result.content;
}

export async function searchInternalDocuments(query: string, limit = 20): Promise<SearchResult[]> {
  if (documentSource === "local") {
    const { searchApi } = await import("@/lib/api");
    return searchApi.search(query, limit);
  }
  const result = await noteApi.list({ page: 1, page_size: limit, keyword: query });
  return result.items.map((note) => ({
    id: note.id,
    title: note.title,
    snippet: note.content.slice(0, 180),
    updated_at: note.updated_at,
    folder_id: note.folder_id,
  }));
}

export const noteApi = {
  create: async (input: NoteInput): Promise<Note> => {
    if (documentSource === "local") return localNoteApi.create(input);
    let folderId: string | null = null;
    if (input.folder_id != null) {
      // A folder id left in the URL by the previous account is not a valid
      // identity in the new account. Create at the root instead of leaking or
      // reusing the previous account's selection.
      folderId = folderIds.reverse.get(input.folder_id) ?? null;
    }
    const document = await guarded(() =>
      accountDocumentsApi.createMarkdown(normalizeCreateMarkdownInput({
        title: input.title,
        markdownContent: input.content,
        folderId,
      })),
    );
    return mapAccountDocument(document);
  },
  update: async (id: number, input: NoteInput): Promise<Note> => {
    if (documentSource === "local") return localNoteApi.update(id, input);
    const current = await findAccountDocument(id);
    if (current.kind !== "markdown") throw { code: "invalidKind", message: "文件文档不能进入 Markdown 编辑器" };
    const updated = await guarded(() =>
      accountDocumentsApi.updateMarkdown(current.id, accountUpdateInput(current, input)),
    );
    return mapAccountDocument(updated);
  },
  delete: async (id: number): Promise<void> => {
    if (documentSource === "local") return localNoteApi.delete(id);
    const current = await findAccountDocument(id);
    await guarded(() => accountDocumentsApi.delete(current.id));
    documentCache.delete(current.id);
  },
  get: async (id: number): Promise<Note> => {
    if (documentSource === "local") return localNoteApi.get(id);
    return mapAccountDocument(await findAccountDocument(id));
  },
  list: async (query: NoteQuery): Promise<PageResult<Note>> => {
    if (documentSource === "local") return localNoteApi.list(query);
    const documents = await listAccountDocuments({
      hidden: false,
      deleted: false,
      folderId: query.uncategorized ? null : query.folder_id,
    });
    const keyword = query.keyword?.trim().toLocaleLowerCase();
    const inFolder = query.uncategorized
      ? documents.filter((item) => item.folder === null)
      : documents;
    const filtered = keyword
      ? inFolder.filter((item) =>
          `${item.title}\n${item.markdownContent ?? ""}`.toLocaleLowerCase().includes(keyword),
        )
      : inFolder;
    const sorted = [...filtered].sort((left, right) => {
      if (query.sort_by === "title") return left.title.localeCompare(right.title);
      if (query.sort_by === "created") return right.createdAt.localeCompare(left.createdAt);
      if (query.sort_by === "custom") return left.sortOrder - right.sortOrder;
      return Number(right.isPinned) - Number(left.isPinned) || right.updatedAt.localeCompare(left.updatedAt);
    });
    const page = query.page ?? 1;
    const pageSize = query.page_size ?? 20;
    const offset = (page - 1) * pageSize;
    return {
      items: sorted.slice(offset, offset + pageSize).map(mapAccountDocument),
      total: sorted.length,
      page,
      page_size: pageSize,
    };
  },
  moveToFolder: async (id: number, folderId?: number | null): Promise<void> => {
    if (documentSource === "local") return localNoteApi.moveToFolder(id, folderId);
    const current = await findAccountDocument(id);
    if (current.kind !== "markdown") throw { code: "invalidKind", message: "暂不能移动文件文档" };
    mapAccountDocument(
      await guarded(() =>
        accountDocumentsApi.updateMarkdown(current.id, {
          expectedRevision: current.revision,
          folderId: folderId == null ? null : requireMappedId(folderId, folderIds, "文件夹"),
        }),
      ),
    );
  },
  reorder: async (orderedIds: number[]): Promise<void> => {
    if (documentSource === "local") return localNoteApi.reorder(orderedIds);
    for (const [sortOrder, id] of orderedIds.entries()) {
      const current = await findAccountDocument(id);
      if (current.kind === "markdown") {
        mapAccountDocument(await guarded(() => accountDocumentsApi.updateMarkdown(current.id, {
          expectedRevision: current.revision,
          sortOrder,
        })));
      }
    }
  },
  moveBatch: async (ids: number[], folderId: number | null): Promise<number> => {
    if (documentSource === "local") return localNoteApi.moveBatch(ids, folderId);
    await Promise.all(ids.map((id) => noteApi.moveToFolder(id, folderId)));
    return ids.length;
  },
  trashBatch: async (ids: number[]): Promise<number> => {
    if (documentSource === "local") return localNoteApi.trashBatch(ids);
    await Promise.all(ids.map((id) => noteApi.delete(id)));
    return ids.length;
  },
  trashAll: async (): Promise<number> => {
    if (documentSource === "local") return localNoteApi.trashAll();
    const all = await listAccountDocuments();
    await Promise.all(all.map((item) => guarded(() => accountDocumentsApi.delete(item.id))));
    return all.length;
  },
  addTagsBatch: async (noteIds: number[], ids: number[]): Promise<number> => {
    if (documentSource === "local") return localNoteApi.addTagsBatch(noteIds, ids);
    await Promise.all(noteIds.flatMap((noteId) => ids.map((tagId) => tagApi.addToNote(noteId, tagId))));
    return noteIds.length;
  },
  togglePin: async (id: number): Promise<boolean> => {
    if (documentSource === "local") return localNoteApi.togglePin(id);
    const current = await findAccountDocument(id);
    if (current.kind !== "markdown") throw { code: "invalidKind", message: "暂不能置顶文件文档" };
    const updated = await guarded(() => accountDocumentsApi.updateMarkdown(current.id, {
      expectedRevision: current.revision,
      isPinned: !current.isPinned,
    }));
    mapAccountDocument(updated);
    return updated.isPinned;
  },
  setHidden: async (id: number, hidden: boolean): Promise<boolean> => {
    if (documentSource === "local") return localNoteApi.setHidden(id, hidden);
    const current = await findAccountDocument(id);
    if (current.kind !== "markdown") throw { code: "invalidKind", message: "暂不能隐藏文件文档" };
    const updated = await guarded(() => accountDocumentsApi.updateMarkdown(current.id, {
      expectedRevision: current.revision,
      isHidden: hidden,
    }));
    mapAccountDocument(updated);
    return updated.isHidden;
  },
  openInNewWindow: async (id: number): Promise<void> => {
    if (documentSource === "local") return localNoteApi.openInNewWindow(id);
    throw { code: "unsupported", message: "账号文档暂不支持独立编辑窗口" };
  },
};

export const folderApi = {
  list: async (): Promise<Folder[]> => {
    if (documentSource === "local") return localFolderApi.list();
    return buildFolderTree(await guarded(() => accountDocumentsApi.listFolders()));
  },
  create: async (name: string, parentId?: number | null): Promise<Folder> => {
    if (documentSource === "local") return localFolderApi.create(name, parentId ?? undefined);
    return mapFolder(await guarded(() => accountDocumentsApi.createFolder(
      name,
      parentId == null ? null : requireMappedId(parentId, folderIds, "文件夹"),
    )));
  },
  rename: async (id: number, name: string): Promise<void> => {
    if (documentSource === "local") return localFolderApi.rename(id, name);
    await guarded(() => accountDocumentsApi.updateFolder(requireMappedId(id, folderIds, "文件夹"), name));
  },
  delete: async (id: number): Promise<void> => {
    if (documentSource === "local") return localFolderApi.delete(id);
    await guarded(() => accountDocumentsApi.deleteFolder(requireMappedId(id, folderIds, "文件夹")));
  },
  move: async (id: number, parentId: number | null): Promise<void> => {
    if (documentSource === "local") return localFolderApi.move(id, parentId);
    await guarded(() => accountDocumentsApi.updateFolder(
      requireMappedId(id, folderIds, "文件夹"),
      undefined,
      parentId == null ? null : requireMappedId(parentId, folderIds, "文件夹"),
    ));
  },
  reorder: async (orderedIds: number[]): Promise<void> => {
    if (documentSource === "local") return localFolderApi.reorder(orderedIds);
  },
  ensurePath: async (path: string): Promise<number | null> => {
    if (documentSource === "local") return localFolderApi.ensurePath(path);
    throw { code: "unsupported", message: "账号文档暂不支持按路径批量创建文件夹" };
  },
};

export const tagApi = {
  create: async (name: string, color?: string | null): Promise<Tag> => {
    if (documentSource === "local") return localTagApi.create(name, color ?? undefined);
    return mapTag(await guarded(() => accountDocumentsApi.createTag(name)));
  },
  list: async (): Promise<Tag[]> => {
    if (documentSource === "local") return localTagApi.list();
    const [tags, documents] = await Promise.all([
      guarded(() => accountDocumentsApi.listTags()),
      listAccountDocuments(),
    ]);
    return tags.map((tag) => mapTag(tag, documents.filter((item) => item.tags.some((itemTag) => itemTag.id === tag.id)).length));
  },
  rename: async (id: number, name: string): Promise<void> => {
    if (documentSource === "local") return localTagApi.rename(id, name);
    await guarded(() => accountDocumentsApi.updateTag(requireMappedId(id, tagIds, "标签"), name));
  },
  setColor: async (id: number, color: string | null): Promise<void> => {
    if (documentSource === "local") return localTagApi.setColor(id, color);
  },
  delete: async (id: number): Promise<void> => {
    if (documentSource === "local") return localTagApi.delete(id);
    await guarded(() => accountDocumentsApi.deleteTag(requireMappedId(id, tagIds, "标签")));
  },
  addToNote: async (noteId: number, tagId: number): Promise<void> => {
    if (documentSource === "local") return localTagApi.addToNote(noteId, tagId);
    const current = await findAccountDocument(noteId);
    if (current.kind !== "markdown") throw { code: "invalidKind", message: "暂不能给文件文档添加标签" };
    const next = new Set(current.tags.map((tag) => tag.id));
    next.add(requireMappedId(tagId, tagIds, "标签"));
    mapAccountDocument(await guarded(() => accountDocumentsApi.updateMarkdown(current.id, {
      expectedRevision: current.revision,
      tagIds: [...next],
    })));
  },
  removeFromNote: async (noteId: number, tagId: number): Promise<void> => {
    if (documentSource === "local") return localTagApi.removeFromNote(noteId, tagId);
    const current = await findAccountDocument(noteId);
    if (current.kind !== "markdown") throw { code: "invalidKind", message: "暂不能修改文件文档标签" };
    const removed = requireMappedId(tagId, tagIds, "标签");
    mapAccountDocument(await guarded(() => accountDocumentsApi.updateMarkdown(current.id, {
      expectedRevision: current.revision,
      tagIds: current.tags.map((tag) => tag.id).filter((id) => id !== removed),
    })));
  },
  getNoteTags: async (noteId: number): Promise<Tag[]> => {
    if (documentSource === "local") return localTagApi.getNoteTags(noteId);
    return (await findAccountDocument(noteId)).tags.map((tag) => mapTag(tag));
  },
  listNotesByTag: async (tagId: number, page = 1, pageSize = 20): Promise<PageResult<Note>> => {
    if (documentSource === "local") return localTagApi.listNotesByTag(tagId, page, pageSize);
    const documents = await guarded(() => accountDocumentsApi.list({
      tagId: requireMappedId(tagId, tagIds, "标签"), hidden: false, deleted: false, limit: 100, offset: 0,
    }));
    const offset = (page - 1) * pageSize;
    return { items: documents.slice(offset, offset + pageSize).map(mapAccountDocument), total: documents.length, page, page_size: pageSize };
  },
};

function permanentDeleteUnsupported(): never {
  throw { code: "unsupported", message: "账号文档暂不支持从客户端永久删除" };
}

export const trashApi = {
  softDelete: (id: number) => noteApi.delete(id),
  restore: async (id: number): Promise<boolean> => {
    if (documentSource === "local") return localTrashApi.restore(id);
    const current = await findAccountDocument(id);
    mapAccountDocument(await guarded(() => accountDocumentsApi.restore(current.id)));
    return current.folder !== null;
  },
  permanentDelete: async (id: number): Promise<void> => {
    if (documentSource === "local") return localTrashApi.permanentDelete(id);
    permanentDeleteUnsupported();
  },
  list: async (page = 1, pageSize = 20): Promise<PageResult<Note>> => {
    if (documentSource === "local") return localTrashApi.list(page, pageSize);
    const documents = await listAccountDocuments({ deleted: true, hidden: false });
    const offset = (page - 1) * pageSize;
    return { items: documents.slice(offset, offset + pageSize).map(mapAccountDocument), total: documents.length, page, page_size: pageSize };
  },
  empty: async (): Promise<number> => {
    if (documentSource === "local") return localTrashApi.empty();
    permanentDeleteUnsupported();
  },
  restoreBatch: async (ids: number[]): Promise<{ restored: number; toRoot: number }> => {
    if (documentSource === "local") return localTrashApi.restoreBatch(ids);
    let toRoot = 0;
    for (const id of ids) if (!(await trashApi.restore(id))) toRoot += 1;
    return { restored: ids.length, toRoot };
  },
  permanentDeleteBatch: async (ids: number[]): Promise<number> => {
    if (documentSource === "local") return localTrashApi.permanentDeleteBatch(ids);
    permanentDeleteUnsupported();
  },
};

export const hiddenApi = {
  list: async (opts?: {
    page?: number;
    pageSize?: number;
    folderId?: number | null;
    uncategorized?: boolean;
  }): Promise<PageResult<Note>> => {
    if (documentSource === "local") return localHiddenApi.list(opts);
    let documents = await listAccountDocuments({
      hidden: true,
      deleted: false,
      folderId: opts?.folderId,
    });
    if (opts?.uncategorized) documents = documents.filter((item) => item.folder === null);
    const page = opts?.page ?? 1;
    const pageSize = opts?.pageSize ?? 20;
    const offset = (page - 1) * pageSize;
    return { items: documents.slice(offset, offset + pageSize).map(mapAccountDocument), total: documents.length, page, page_size: pageSize };
  },
  listFolderIds: async (): Promise<(number | null)[]> => {
    if (documentSource === "local") return localHiddenApi.listFolderIds();
    const documents = await listAccountDocuments({ hidden: true, deleted: false });
    return [...new Set(documents.map((item) => item.folder ? registerId(item.folder.id, folderIds) : null))];
  },
};

export const dailyApi = {
  get: async (date: string): Promise<Note | null> => {
    if (documentSource === "local") return localDailyApi.get(date);
    const documents = await listAccountDocuments({ diaryDate: date, hidden: false, deleted: false });
    return documents[0] ? mapAccountDocument(documents[0]) : null;
  },
  getOrCreate: async (date: string): Promise<Note> => {
    if (documentSource === "local") return localDailyApi.getOrCreate(date);
    const existing = await dailyApi.get(date);
    if (existing) return existing;
    return mapAccountDocument(await guarded(() => accountDocumentsApi.createMarkdown(normalizeCreateMarkdownInput({
      title: date,
      markdownContent: "",
      diaryDate: date,
    }))));
  },
  listDates: async (year: number, month: number): Promise<string[]> => {
    if (documentSource === "local") return localDailyApi.listDates(year, month);
    const prefix = `${year}-${String(month).padStart(2, "0")}-`;
    const documents = await listAccountDocuments({ hidden: false, deleted: false });
    return documents
      .map((item) => item.diaryDate)
      .filter((date): date is string => Boolean(date?.startsWith(prefix)))
      .sort();
  },
  getNeighbors: async (date: string): Promise<[string | null, string | null]> => {
    if (documentSource === "local") return localDailyApi.getNeighbors(date);
    const dates = (await listAccountDocuments({ hidden: false, deleted: false }))
      .map((item) => item.diaryDate)
      .filter((item): item is string => Boolean(item))
      .sort();
    return [dates.filter((item) => item < date).at(-1) ?? null, dates.find((item) => item > date) ?? null];
  },
};

export { documentSource, isAccountDocumentSource };
