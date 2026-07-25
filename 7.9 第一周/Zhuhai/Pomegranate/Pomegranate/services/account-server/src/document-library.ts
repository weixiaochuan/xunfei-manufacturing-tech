import { createHash, randomUUID } from "node:crypto";
import type { Pool, PoolClient } from "pg";
import type { DocumentKind, PublicDocumentFile } from "./documents.js";

export const LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND = "learning_assistant_upload";
export const LEARNING_ASSISTANT_UPLOAD_FOLDER_NAME = "助学模块上传";

export type DocumentFolderKind = typeof LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND;

export interface PublicDocumentFolder {
  id: string;
  name: string;
  parentId: string | null;
  folderKind: DocumentFolderKind | null;
  createdAt: string;
  updatedAt: string;
}

export interface PublicDocumentTag {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
}

export interface CatalogDocument {
  id: string;
  kind: DocumentKind;
  title: string;
  markdownContent: string | null;
  file: PublicDocumentFile | null;
  sourceLocalDocumentId: string | null;
  folder: PublicDocumentFolder | null;
  tags: PublicDocumentTag[];
  diaryDate: string | null;
  isPinned: boolean;
  isHidden: boolean;
  sortOrder: number;
  wordCount: number;
  contentSha256: string | null;
  revision: number;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface DocumentFilters {
  kind: DocumentKind | null;
  folderId: string | null;
  tagId: string | null;
  diaryDate: string | null;
  hidden: boolean;
  deleted: boolean;
  limit: number;
  offset: number;
}

export interface MarkdownMutation {
  title?: string;
  markdownContent?: string;
  folderId?: string | null;
  diaryDate?: string | null;
  isPinned?: boolean;
  isHidden?: boolean;
  sortOrder?: number;
  tagIds?: string[];
}

export type DocumentUpdateResult =
  | { status: "updated"; document: CatalogDocument }
  | { status: "not_found" }
  | { status: "conflict" };

export type DocumentRestoreResult =
  | { status: "restored"; document: CatalogDocument }
  | { status: "not_found" }
  | { status: "file_content_unavailable" };

export interface DocumentLibraryService {
  list(ownerUserId: string, filters: DocumentFilters): Promise<CatalogDocument[]>;
  listFolders(ownerUserId: string): Promise<PublicDocumentFolder[]>;
  createFolder(ownerUserId: string, name: unknown, parentId: unknown): Promise<PublicDocumentFolder>;
  getOrCreateLearningAssistantUploadFolder(ownerUserId: string): Promise<PublicDocumentFolder>;
  updateFolder(ownerUserId: string, folderId: string, name: unknown, parentId: unknown): Promise<PublicDocumentFolder | null>;
  deleteFolder(ownerUserId: string, folderId: string): Promise<boolean>;
  listTags(ownerUserId: string): Promise<PublicDocumentTag[]>;
  createTag(ownerUserId: string, name: unknown): Promise<PublicDocumentTag>;
  updateTag(ownerUserId: string, tagId: string, name: unknown): Promise<PublicDocumentTag | null>;
  deleteTag(ownerUserId: string, tagId: string): Promise<boolean>;
  createMarkdown(ownerUserId: string, input: MarkdownMutation): Promise<CatalogDocument>;
  updateMarkdown(ownerUserId: string, documentId: string, expectedRevision: unknown, input: MarkdownMutation): Promise<DocumentUpdateResult>;
  restore(ownerUserId: string, documentId: string, verifyStoredFile?: (fileId: string) => Promise<boolean>): Promise<DocumentRestoreResult>;
  importLocalMetadata(ownerUserId: string, input: unknown): Promise<{ folders: number; tags: number; links: number }>;
}

export class DocumentLibraryValidationError extends Error {}

interface DocumentRow {
  id: string;
  document_kind: DocumentKind;
  title: string;
  markdown_content: string | null;
  source_local_document_id: string | null;
  folder_id: string | null;
  folder_name: string | null;
  folder_parent_id: string | null;
  folder_kind: DocumentFolderKind | null;
  folder_created_at: Date | string | null;
  folder_updated_at: Date | string | null;
  tags: Array<{ id: string; name: string; createdAt: string; updatedAt: string }> | null;
  diary_date: Date | string | null;
  is_pinned: boolean;
  is_hidden: boolean;
  sort_order: number;
  word_count: number;
  content_sha256: string | null;
  revision: string | number;
  created_at: Date | string;
  updated_at: Date | string;
  deleted_at: Date | string | null;
  file_id: string | null;
  original_name: string | null;
  mime_type: string | null;
  size_bytes: string | number | null;
  sha256: string | null;
}

interface FolderRow {
  id: string;
  name: string;
  parent_id: string | null;
  folder_kind: DocumentFolderKind | null;
  created_at: Date | string;
  updated_at: Date | string;
}

interface TagRow {
  id: string;
  name: string;
  created_at: Date | string;
  updated_at: Date | string;
}

const MAX_TITLE_BYTES = 2_000;
const MAX_MARKDOWN_BYTES = 2 * 1024 * 1024;
const MAX_NAME_BYTES = 512;

const DOCUMENT_SELECT = `
  SELECT d.id, d.document_kind, d.title, d.markdown_content,
    d.source_local_document_id, d.folder_id, d.diary_date, d.is_pinned,
    d.is_hidden, d.sort_order, d.word_count, d.content_sha256, d.revision,
    d.created_at, d.updated_at, d.deleted_at,
    folder.name AS folder_name, folder.parent_id AS folder_parent_id,
    folder.folder_kind,
    folder.created_at AS folder_created_at, folder.updated_at AS folder_updated_at,
    COALESCE(tag_data.tags, '[]'::json) AS tags,
    uf.id AS file_id, uf.original_name, uf.mime_type, uf.size_bytes, uf.sha256
  FROM documents d
  LEFT JOIN document_folders folder
    ON folder.id = d.folder_id AND folder.deleted_at IS NULL
  LEFT JOIN LATERAL (
    SELECT json_agg(json_build_object(
      'id', tag.id,
      'name', tag.name,
      'createdAt', tag.created_at,
      'updatedAt', tag.updated_at
    ) ORDER BY lower(tag.name), tag.id) AS tags
    FROM document_tag_links link
    JOIN document_tags tag ON tag.id = link.tag_id AND tag.deleted_at IS NULL
    WHERE link.document_id = d.id
  ) tag_data ON TRUE
  LEFT JOIN user_files uf ON uf.id = d.user_file_id`;

function iso(value: Date | string): string {
  return new Date(value).toISOString();
}

function dateOnly(value: Date | string | null): string | null {
  if (value === null) return null;
  if (typeof value === "string" && /^\d{4}-\d{2}-\d{2}$/.test(value)) return value;
  return new Date(value).toISOString().slice(0, 10);
}

function mapFolder(row: FolderRow): PublicDocumentFolder {
  return { id: row.id, name: row.name, parentId: row.parent_id, folderKind: row.folder_kind, createdAt: iso(row.created_at), updatedAt: iso(row.updated_at) };
}

function mapTag(row: TagRow): PublicDocumentTag {
  return { id: row.id, name: row.name, createdAt: iso(row.created_at), updatedAt: iso(row.updated_at) };
}

function mapDocument(row: DocumentRow): CatalogDocument {
  const revision = Number(row.revision);
  const wordCount = Number(row.word_count);
  if (!Number.isSafeInteger(revision) || revision < 1 || !Number.isSafeInteger(wordCount) || wordCount < 0) {
    throw new Error("invalid_document_metadata");
  }
  let file: PublicDocumentFile | null = null;
  if (row.file_id !== null) {
    const sizeBytes = Number(row.size_bytes);
    if (!row.original_name || !row.sha256 || !Number.isSafeInteger(sizeBytes) || sizeBytes < 0) {
      throw new Error("invalid_document_file_metadata");
    }
    file = { id: row.file_id, originalName: row.original_name, mimeType: row.mime_type, sizeBytes, sha256: row.sha256 };
  }
  return {
    id: row.id,
    kind: row.document_kind,
    title: row.title,
    markdownContent: row.markdown_content,
    file,
    sourceLocalDocumentId: row.source_local_document_id,
    folder: row.folder_id && row.folder_name && row.folder_created_at && row.folder_updated_at
      ? { id: row.folder_id, name: row.folder_name, parentId: row.folder_parent_id, folderKind: row.folder_kind, createdAt: iso(row.folder_created_at), updatedAt: iso(row.folder_updated_at) }
      : null,
    tags: (row.tags ?? []).map((tag) => ({ ...tag, createdAt: iso(tag.createdAt), updatedAt: iso(tag.updatedAt) })),
    diaryDate: dateOnly(row.diary_date),
    isPinned: row.is_pinned,
    isHidden: row.is_hidden,
    sortOrder: row.sort_order,
    wordCount,
    contentSha256: row.content_sha256,
    revision,
    createdAt: iso(row.created_at),
    updatedAt: iso(row.updated_at),
    deletedAt: row.deleted_at === null ? null : iso(row.deleted_at),
  };
}

function validName(value: unknown, code: string): string {
  if (typeof value !== "string") throw new DocumentLibraryValidationError(code);
  const name = value.trim();
  if (!name || Buffer.byteLength(name, "utf8") > MAX_NAME_BYTES) throw new DocumentLibraryValidationError(code);
  return name;
}

function validTitle(value: unknown): string {
  if (typeof value !== "string") throw new DocumentLibraryValidationError("invalid_title");
  const title = value.trim();
  if (!title || Buffer.byteLength(title, "utf8") > MAX_TITLE_BYTES) throw new DocumentLibraryValidationError("invalid_title");
  return title;
}

function validMarkdown(value: unknown): string {
  if (typeof value !== "string" || Buffer.byteLength(value, "utf8") > MAX_MARKDOWN_BYTES) {
    throw new DocumentLibraryValidationError("invalid_markdown_content");
  }
  return value;
}

function validUuidOrNull(value: unknown, code: string): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string" || !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)) {
    throw new DocumentLibraryValidationError(code);
  }
  return value;
}

function validDate(value: unknown): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value)) throw new DocumentLibraryValidationError("invalid_diary_date");
  const date = new Date(`${value}T00:00:00.000Z`);
  if (!Number.isFinite(date.getTime()) || date.toISOString().slice(0, 10) !== value) throw new DocumentLibraryValidationError("invalid_diary_date");
  return value;
}

function validBoolean(value: unknown, code: string): boolean {
  if (typeof value !== "boolean") throw new DocumentLibraryValidationError(code);
  return value;
}

function validSortOrder(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) throw new DocumentLibraryValidationError("invalid_sort_order");
  return value;
}

function validTagIds(value: unknown): string[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > 100) throw new DocumentLibraryValidationError("invalid_tag_ids");
  const values = value.map((item) => validUuidOrNull(item, "invalid_tag_ids")!);
  return [...new Set(values)];
}

interface LocalMetadataPayload {
  folders: Array<{ sourceLocalFolderId: string; name: string; parentSourceLocalFolderId: string | null }>;
  tags: Array<{ sourceLocalTagId: string; name: string }>;
  tagLinks: Array<{ sourceLocalDocumentId: string; sourceLocalTagId: string }>;
}

function validSourceId(value: unknown, prefix: string): string {
  if (typeof value !== "string" || !value.startsWith(prefix) || value.length > 128 || /[\u0000-\u001f\u007f]/.test(value)) {
    throw new DocumentLibraryValidationError("invalid_local_metadata");
  }
  return value;
}

function validLocalMetadata(value: unknown): LocalMetadataPayload {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new DocumentLibraryValidationError("invalid_local_metadata");
  const raw = value as Record<string, unknown>;
  if (!Array.isArray(raw.folders) || !Array.isArray(raw.tags) || !Array.isArray(raw.tagLinks) || raw.folders.length > 1_000 || raw.tags.length > 1_000 || raw.tagLinks.length > 10_000) {
    throw new DocumentLibraryValidationError("invalid_local_metadata");
  }
  return {
    folders: raw.folders.map((item) => {
      if (typeof item !== "object" || item === null || Array.isArray(item)) throw new DocumentLibraryValidationError("invalid_local_metadata");
      const entry = item as Record<string, unknown>;
      return {
        sourceLocalFolderId: validSourceId(entry.sourceLocalFolderId, "sqlite-folder:"),
        name: validName(entry.name, "invalid_local_metadata"),
        parentSourceLocalFolderId: entry.parentSourceLocalFolderId === null ? null : validSourceId(entry.parentSourceLocalFolderId, "sqlite-folder:"),
      };
    }),
    tags: raw.tags.map((item) => {
      if (typeof item !== "object" || item === null || Array.isArray(item)) throw new DocumentLibraryValidationError("invalid_local_metadata");
      const entry = item as Record<string, unknown>;
      return { sourceLocalTagId: validSourceId(entry.sourceLocalTagId, "sqlite-tag:"), name: validName(entry.name, "invalid_local_metadata") };
    }),
    tagLinks: raw.tagLinks.map((item) => {
      if (typeof item !== "object" || item === null || Array.isArray(item)) throw new DocumentLibraryValidationError("invalid_local_metadata");
      const entry = item as Record<string, unknown>;
      return { sourceLocalDocumentId: validSourceId(entry.sourceLocalDocumentId, "sqlite-note:"), sourceLocalTagId: validSourceId(entry.sourceLocalTagId, "sqlite-tag:") };
    }),
  };
}

function stats(content: string): { wordCount: number; contentSha256: string } {
  return {
    // 与现有 SQLite 触发器保持一致：只移除普通 ASCII 空格后计算 Unicode 字符数。
    wordCount: [...content.replaceAll(" ", "")].length,
    contentSha256: createHash("sha256").update(content, "utf8").digest("hex"),
  };
}

async function transaction<T>(pool: Pool, run: (client: PoolClient) => Promise<T>): Promise<T> {
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    const result = await run(client);
    await client.query("COMMIT");
    return result;
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

async function findDocument(client: Pool | PoolClient, id: string): Promise<CatalogDocument> {
  const result = await client.query<DocumentRow>(`${DOCUMENT_SELECT} WHERE d.id = $1`, [id]);
  if (result.rowCount !== 1 || !result.rows[0]) throw new Error("document_write_incomplete");
  return mapDocument(result.rows[0]);
}

async function assertFolder(client: PoolClient, ownerUserId: string, folderId: string | null): Promise<void> {
  if (folderId === null) return;
  const result = await client.query("SELECT 1 FROM document_folders WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL", [folderId, ownerUserId]);
  if (result.rowCount !== 1) throw new DocumentLibraryValidationError("invalid_folder");
}

async function assertTags(client: PoolClient, ownerUserId: string, tagIds: string[]): Promise<void> {
  if (tagIds.length === 0) return;
  const result = await client.query<{ count: string }>("SELECT count(*)::text AS count FROM document_tags WHERE owner_user_id = $1 AND id = ANY($2::uuid[]) AND deleted_at IS NULL", [ownerUserId, tagIds]);
  if (Number(result.rows[0]?.count ?? 0) !== tagIds.length) throw new DocumentLibraryValidationError("invalid_tags");
}

async function replaceTags(client: PoolClient, documentId: string, tagIds: string[]): Promise<void> {
  await client.query("DELETE FROM document_tag_links WHERE document_id = $1", [documentId]);
  if (tagIds.length > 0) {
    await client.query("INSERT INTO document_tag_links (document_id, tag_id) SELECT $1, unnest($2::uuid[])", [documentId, tagIds]);
  }
}

export function createPostgresDocumentLibraryService(pool: Pool): DocumentLibraryService {
  return {
    async list(ownerUserId, filters) {
      const params: unknown[] = [ownerUserId];
      const clauses = ["d.owner_user_id = $1"];
      const add = (clause: string, value: unknown) => { params.push(value); clauses.push(clause.replace("?", `$${params.length}`)); };
      if (filters.kind) add("d.document_kind = ?", filters.kind);
      if (filters.folderId) add("d.folder_id = ?", filters.folderId);
      if (filters.tagId) add("EXISTS (SELECT 1 FROM document_tag_links filter_link WHERE filter_link.document_id = d.id AND filter_link.tag_id = ?)", filters.tagId);
      if (filters.diaryDate) add("d.diary_date = ?::date", filters.diaryDate);
      add("d.is_hidden = ?", filters.hidden);
      clauses.push(filters.deleted ? "d.deleted_at IS NOT NULL" : "d.deleted_at IS NULL");
      params.push(filters.limit, filters.offset);
      const result = await pool.query<DocumentRow>(`${DOCUMENT_SELECT} WHERE ${clauses.join(" AND ")} ORDER BY d.is_pinned DESC, d.sort_order ASC, d.updated_at DESC, d.id DESC LIMIT $${params.length - 1} OFFSET $${params.length}`, params);
      return result.rows.map(mapDocument);
    },

    async listFolders(ownerUserId) {
      const result = await pool.query<FolderRow>("SELECT id, name, parent_id, folder_kind, created_at, updated_at FROM document_folders WHERE owner_user_id = $1 AND deleted_at IS NULL ORDER BY lower(name), id", [ownerUserId]);
      return result.rows.map(mapFolder);
    },

    async createFolder(ownerUserId, rawName, rawParentId) {
      const name = validName(rawName, "invalid_folder_name");
      const parentId = validUuidOrNull(rawParentId, "invalid_parent_folder");
      return transaction(pool, async (client) => {
        await assertFolder(client, ownerUserId, parentId);
        const result = await client.query<FolderRow>("INSERT INTO document_folders (id, owner_user_id, name, parent_id) VALUES ($1, $2, $3, $4) RETURNING id, name, parent_id, folder_kind, created_at, updated_at", [randomUUID(), ownerUserId, name, parentId]);
        return mapFolder(result.rows[0]!);
      });
    },

    async getOrCreateLearningAssistantUploadFolder(ownerUserId) {
      return transaction(pool, async (client) => {
        const result = await client.query<FolderRow>(
          `INSERT INTO document_folders (id, owner_user_id, name, parent_id, folder_kind)
           VALUES ($1, $2, $3, NULL, $4)
           ON CONFLICT (owner_user_id, folder_kind)
             WHERE folder_kind = 'learning_assistant_upload' AND deleted_at IS NULL
           DO UPDATE SET
             name = EXCLUDED.name,
             updated_at = CASE
               WHEN document_folders.name IS DISTINCT FROM EXCLUDED.name THEN CURRENT_TIMESTAMP
               ELSE document_folders.updated_at
             END
           RETURNING id, name, parent_id, folder_kind, created_at, updated_at`,
          [randomUUID(), ownerUserId, LEARNING_ASSISTANT_UPLOAD_FOLDER_NAME, LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND],
        );
        return mapFolder(result.rows[0]!);
      });
    },

    async updateFolder(ownerUserId, folderId, rawName, rawParentId) {
      const name = rawName === undefined ? undefined : validName(rawName, "invalid_folder_name");
      const parentId = rawParentId === undefined ? undefined : validUuidOrNull(rawParentId, "invalid_parent_folder");
      if (name === undefined && parentId === undefined) throw new DocumentLibraryValidationError("empty_update");
      return transaction(pool, async (client) => {
        const current = await client.query<FolderRow>("SELECT id, name, parent_id, folder_kind, created_at, updated_at FROM document_folders WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL FOR UPDATE", [folderId, ownerUserId]);
        if (!current.rows[0]) return null;
        if (current.rows[0].folder_kind === LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND && name !== undefined && name !== current.rows[0].name) {
          throw new DocumentLibraryValidationError("learning_assistant_upload_folder_name_locked");
        }
        const nextParent = parentId === undefined ? current.rows[0].parent_id : parentId;
        if (nextParent === folderId) throw new DocumentLibraryValidationError("folder_cycle");
        await assertFolder(client, ownerUserId, nextParent);
        try {
          const result = await client.query<FolderRow>("UPDATE document_folders SET name = $3, parent_id = $4, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL RETURNING id, name, parent_id, folder_kind, created_at, updated_at", [folderId, ownerUserId, name ?? current.rows[0].name, nextParent]);
          return mapFolder(result.rows[0]!);
        } catch (error) {
          if (error instanceof Error && /document_folder_cycle/.test(error.message)) throw new DocumentLibraryValidationError("folder_cycle");
          throw error;
        }
      });
    },

    async deleteFolder(ownerUserId, folderId) {
      return transaction(pool, async (client) => {
        const found = await client.query("SELECT 1 FROM document_folders WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL FOR UPDATE", [folderId, ownerUserId]);
        if (found.rowCount !== 1) return false;
        await client.query("UPDATE documents SET folder_id = NULL, updated_at = CURRENT_TIMESTAMP WHERE owner_user_id = $1 AND folder_id = $2", [ownerUserId, folderId]);
        await client.query("UPDATE document_folders SET parent_id = NULL, updated_at = CURRENT_TIMESTAMP WHERE owner_user_id = $1 AND parent_id = $2 AND deleted_at IS NULL", [ownerUserId, folderId]);
        await client.query("UPDATE document_folders SET parent_id = NULL, deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND owner_user_id = $2", [folderId, ownerUserId]);
        return true;
      });
    },

    async listTags(ownerUserId) {
      const result = await pool.query<TagRow>("SELECT id, name, created_at, updated_at FROM document_tags WHERE owner_user_id = $1 AND deleted_at IS NULL ORDER BY lower(name), id", [ownerUserId]);
      return result.rows.map(mapTag);
    },

    async createTag(ownerUserId, rawName) {
      const name = validName(rawName, "invalid_tag_name");
      try {
        const result = await pool.query<TagRow>("INSERT INTO document_tags (id, owner_user_id, name) VALUES ($1, $2, $3) RETURNING id, name, created_at, updated_at", [randomUUID(), ownerUserId, name]);
        return mapTag(result.rows[0]!);
      } catch (error) {
        if (typeof error === "object" && error !== null && "code" in error && error.code === "23505") throw new DocumentLibraryValidationError("tag_name_exists");
        throw error;
      }
    },

    async updateTag(ownerUserId, tagId, rawName) {
      const name = validName(rawName, "invalid_tag_name");
      try {
        const result = await pool.query<TagRow>("UPDATE document_tags SET name = $3, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL RETURNING id, name, created_at, updated_at", [tagId, ownerUserId, name]);
        return result.rows[0] ? mapTag(result.rows[0]) : null;
      } catch (error) {
        if (typeof error === "object" && error !== null && "code" in error && error.code === "23505") throw new DocumentLibraryValidationError("tag_name_exists");
        throw error;
      }
    },

    async deleteTag(ownerUserId, tagId) {
      return transaction(pool, async (client) => {
        const found = await client.query("SELECT 1 FROM document_tags WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL FOR UPDATE", [tagId, ownerUserId]);
        if (found.rowCount !== 1) return false;
        await client.query("DELETE FROM document_tag_links WHERE tag_id = $1", [tagId]);
        await client.query("UPDATE document_tags SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = $1", [tagId]);
        return true;
      });
    },

    async createMarkdown(ownerUserId, rawInput) {
      const title = validTitle(rawInput.title);
      const markdownContent = validMarkdown(rawInput.markdownContent);
      const folderId = validUuidOrNull(rawInput.folderId, "invalid_folder");
      const diaryDate = validDate(rawInput.diaryDate);
      const isPinned = rawInput.isPinned === undefined ? false : validBoolean(rawInput.isPinned, "invalid_is_pinned");
      const isHidden = rawInput.isHidden === undefined ? false : validBoolean(rawInput.isHidden, "invalid_is_hidden");
      const sortOrder = rawInput.sortOrder === undefined ? 0 : validSortOrder(rawInput.sortOrder);
      const tagIds = validTagIds(rawInput.tagIds);
      const computed = stats(markdownContent);
      return transaction(pool, async (client) => {
        await assertFolder(client, ownerUserId, folderId);
        await assertTags(client, ownerUserId, tagIds);
        const id = randomUUID();
        await client.query(`INSERT INTO documents (id, owner_user_id, document_kind, title, markdown_content, folder_id, diary_date, is_pinned, is_hidden, sort_order, word_count, content_sha256, revision) VALUES ($1, $2, 'markdown', $3, $4, $5, $6, $7, $8, $9, $10, $11, 1)`, [id, ownerUserId, title, markdownContent, folderId, diaryDate, isPinned, isHidden, sortOrder, computed.wordCount, computed.contentSha256]);
        await replaceTags(client, id, tagIds);
        return findDocument(client, id);
      });
    },

    async updateMarkdown(ownerUserId, documentId, rawExpectedRevision, rawInput) {
      if (typeof rawExpectedRevision !== "number" || !Number.isSafeInteger(rawExpectedRevision) || rawExpectedRevision < 1) throw new DocumentLibraryValidationError("invalid_expected_revision");
      const hasChanges = Object.values(rawInput).some((value) => value !== undefined);
      if (!hasChanges) throw new DocumentLibraryValidationError("empty_update");
      return transaction(pool, async (client) => {
        const currentResult = await client.query<DocumentRow>(`${DOCUMENT_SELECT} WHERE d.id = $1 AND d.owner_user_id = $2 AND d.document_kind = 'markdown' AND d.deleted_at IS NULL FOR UPDATE OF d`, [documentId, ownerUserId]);
        const currentRow = currentResult.rows[0];
        if (!currentRow) return { status: "not_found" as const };
        const current = mapDocument(currentRow);
        if (current.revision !== rawExpectedRevision) return { status: "conflict" as const };
        const title = rawInput.title === undefined ? current.title : validTitle(rawInput.title);
        const content = rawInput.markdownContent === undefined ? current.markdownContent! : validMarkdown(rawInput.markdownContent);
        const folderId = rawInput.folderId === undefined ? current.folder?.id ?? null : validUuidOrNull(rawInput.folderId, "invalid_folder");
        const diaryDate = rawInput.diaryDate === undefined ? current.diaryDate : validDate(rawInput.diaryDate);
        const isPinned = rawInput.isPinned === undefined ? current.isPinned : validBoolean(rawInput.isPinned, "invalid_is_pinned");
        const isHidden = rawInput.isHidden === undefined ? current.isHidden : validBoolean(rawInput.isHidden, "invalid_is_hidden");
        const sortOrder = rawInput.sortOrder === undefined ? current.sortOrder : validSortOrder(rawInput.sortOrder);
        const tagIds = rawInput.tagIds === undefined ? current.tags.map((tag) => tag.id) : validTagIds(rawInput.tagIds);
        await assertFolder(client, ownerUserId, folderId);
        await assertTags(client, ownerUserId, tagIds);
        const computed = stats(content);
        const updated = await client.query("UPDATE documents SET title = $3, markdown_content = $4, folder_id = $5, diary_date = $6, is_pinned = $7, is_hidden = $8, sort_order = $9, word_count = $10, content_sha256 = $11, revision = revision + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND owner_user_id = $2 AND revision = $12 AND document_kind = 'markdown' AND deleted_at IS NULL RETURNING id", [documentId, ownerUserId, title, content, folderId, diaryDate, isPinned, isHidden, sortOrder, computed.wordCount, computed.contentSha256, rawExpectedRevision]);
        if (updated.rowCount !== 1) return { status: "conflict" as const };
        if (rawInput.tagIds !== undefined) await replaceTags(client, documentId, tagIds);
        return { status: "updated" as const, document: await findDocument(client, documentId) };
      });
    },

    async restore(ownerUserId, documentId, verifyStoredFile) {
      return transaction(pool, async (client) => {
        const found = await client.query<{ document_kind: DocumentKind; user_file_id: string | null; file_deleted_at: Date | null }>("SELECT d.document_kind, d.user_file_id, uf.deleted_at AS file_deleted_at FROM documents d LEFT JOIN user_files uf ON uf.id = d.user_file_id WHERE d.id = $1 AND d.owner_user_id = $2 AND d.deleted_at IS NOT NULL FOR UPDATE OF d", [documentId, ownerUserId]);
        const row = found.rows[0];
        if (!row) return { status: "not_found" as const };
        if (row.document_kind === "uploaded_file") {
          if (!row.user_file_id || row.file_deleted_at !== null || (verifyStoredFile && !(await verifyStoredFile(row.user_file_id)))) {
            return { status: "file_content_unavailable" as const };
          }
        }
        await client.query("UPDATE documents SET deleted_at = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND owner_user_id = $2", [documentId, ownerUserId]);
        return { status: "restored" as const, document: await findDocument(client, documentId) };
      });
    },

    async importLocalMetadata(ownerUserId, rawInput) {
      const input = validLocalMetadata(rawInput);
      return transaction(pool, async (client) => {
        for (const folder of input.folders) {
          await client.query(`INSERT INTO document_folders (id, owner_user_id, name, source_local_folder_id) VALUES ($1, $2, $3, $4) ON CONFLICT (owner_user_id, source_local_folder_id) WHERE source_local_folder_id IS NOT NULL DO UPDATE SET name = EXCLUDED.name, updated_at = CURRENT_TIMESTAMP, deleted_at = NULL`, [randomUUID(), ownerUserId, folder.name, folder.sourceLocalFolderId]);
        }
        for (const folder of input.folders) {
          const parent = folder.parentSourceLocalFolderId === null ? null : await client.query<{ id: string }>("SELECT id FROM document_folders WHERE owner_user_id = $1 AND source_local_folder_id = $2 AND deleted_at IS NULL", [ownerUserId, folder.parentSourceLocalFolderId]);
          if (folder.parentSourceLocalFolderId !== null && !parent?.rows[0]) throw new DocumentLibraryValidationError("invalid_local_metadata");
          await client.query("UPDATE document_folders SET parent_id = $3, updated_at = CURRENT_TIMESTAMP WHERE owner_user_id = $1 AND source_local_folder_id = $2 AND deleted_at IS NULL", [ownerUserId, folder.sourceLocalFolderId, parent?.rows[0]?.id ?? null]);
        }
        for (const tag of input.tags) {
          const found = await client.query<{ id: string }>("SELECT id FROM document_tags WHERE owner_user_id = $1 AND deleted_at IS NULL AND (source_local_tag_id = $2 OR lower(name) = lower($3)) FOR UPDATE", [ownerUserId, tag.sourceLocalTagId, tag.name]);
          if (found.rows[0]) {
            await client.query("UPDATE document_tags SET name = $3, source_local_tag_id = COALESCE(source_local_tag_id, $2), updated_at = CURRENT_TIMESTAMP WHERE id = $1", [found.rows[0].id, tag.sourceLocalTagId, tag.name]);
          } else {
            await client.query("INSERT INTO document_tags (id, owner_user_id, name, source_local_tag_id) VALUES ($1, $2, $3, $4)", [randomUUID(), ownerUserId, tag.name, tag.sourceLocalTagId]);
          }
        }
        await client.query(`UPDATE documents d SET folder_id = folder.id FROM document_folders folder WHERE d.owner_user_id = $1 AND d.owner_user_id = folder.owner_user_id AND d.document_kind = 'markdown' AND d.source_local_document_id IS NOT NULL AND folder.source_local_folder_id = 'sqlite-folder:' || (d.legacy_metadata->'folder'->>'id')`, [ownerUserId]);
        for (const link of input.tagLinks) {
          await client.query(`INSERT INTO document_tag_links (document_id, tag_id) SELECT d.id, tag.id FROM documents d JOIN document_tags tag ON tag.owner_user_id = d.owner_user_id AND tag.source_local_tag_id = $3 AND tag.deleted_at IS NULL WHERE d.owner_user_id = $1 AND d.source_local_document_id = $2 ON CONFLICT DO NOTHING`, [ownerUserId, link.sourceLocalDocumentId, link.sourceLocalTagId]);
        }
        const counts = await client.query<{ folders: number; tags: number; links: number }>(`SELECT (SELECT count(*)::int FROM document_folders WHERE owner_user_id = $1 AND source_local_folder_id IS NOT NULL AND deleted_at IS NULL) AS folders, (SELECT count(*)::int FROM document_tags WHERE owner_user_id = $1 AND source_local_tag_id IS NOT NULL AND deleted_at IS NULL) AS tags, (SELECT count(*)::int FROM document_tag_links link JOIN documents d ON d.id = link.document_id WHERE d.owner_user_id = $1 AND d.source_local_document_id IS NOT NULL) AS links`, [ownerUserId]);
        return counts.rows[0]!;
      });
    },
  };
}
