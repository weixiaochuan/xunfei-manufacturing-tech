import { createHash, randomUUID } from "node:crypto";
import type { Pool, PoolClient } from "pg";
import type { PublicUserFile } from "./user-files.js";

export type DocumentKind = "markdown" | "uploaded_file";

export interface PublicDocumentFile {
  id: string;
  originalName: string;
  mimeType: string | null;
  sizeBytes: number;
  sha256: string;
}

export interface PublicDocument {
  id: string;
  kind: DocumentKind;
  title: string;
  markdownContent: string | null;
  file: PublicDocumentFile | null;
  sourceLocalDocumentId: string | null;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface LocalMarkdownImportItem {
  sourceLocalDocumentId: string;
  title: string;
  markdownContent: string;
  createdAt: string;
  updatedAt: string;
  deletedAt?: string | null;
  legacyMetadata: Record<string, unknown>;
}

export interface ImportOutcome {
  sourceLocalDocumentId: string;
  documentId: string;
  status: "imported" | "updated" | "skipped";
  contentSha256: string;
}

export interface ImportResult {
  imported: number;
  updated: number;
  skipped: number;
  failed: number;
  outcomes: ImportOutcome[];
}

export interface DeletedDocument {
  documentId: string;
  fileId: string | null;
  storageKey: string | null;
}

export interface DocumentRepository {
  list(ownerUserId: string, kind: DocumentKind | null, limit: number, offset: number): Promise<PublicDocument[]>;
  createMarkdown(ownerUserId: string, title: string, markdownContent: string): Promise<PublicDocument>;
  createUploadedFile(ownerUserId: string, file: PublicUserFile): Promise<PublicDocument>;
  updateMarkdown(
    documentId: string,
    ownerUserId: string,
    updates: { title?: string; markdownContent?: string },
  ): Promise<PublicDocument | null>;
  deleteOwned(documentId: string, ownerUserId: string): Promise<DeletedDocument | null>;
  deleteByFileId(fileId: string, ownerUserId: string): Promise<DeletedDocument | null>;
  upsertLocalMarkdown(ownerUserId: string, item: LocalMarkdownImportItem): Promise<{
    status: "imported" | "updated" | "skipped";
    document: PublicDocument;
  }>;
}

export interface DocumentService extends DocumentRepository {
  importLocalMarkdown(ownerUserId: string, items: unknown): Promise<ImportResult>;
}

interface DocumentRow {
  id: string;
  document_kind: DocumentKind;
  title: string;
  markdown_content: string | null;
  source_local_document_id: string | null;
  created_at: Date | string;
  updated_at: Date | string;
  deleted_at: Date | string | null;
  file_id: string | null;
  original_name: string | null;
  mime_type: string | null;
  size_bytes: string | number | null;
  sha256: string | null;
}

interface ExistingImportRow extends DocumentRow {
  legacy_metadata: Record<string, unknown> | null;
}

const DOCUMENT_SELECT = `
  SELECT
    d.id,
    d.document_kind,
    d.title,
    d.markdown_content,
    d.source_local_document_id,
    d.legacy_metadata,
    d.created_at,
    d.updated_at,
    d.deleted_at,
    uf.id AS file_id,
    uf.original_name,
    uf.mime_type,
    uf.size_bytes,
    uf.sha256
  FROM documents d
  LEFT JOIN user_files uf ON uf.id = d.user_file_id`;

function iso(value: Date | string): string {
  return new Date(value).toISOString();
}

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, entry]) => `${JSON.stringify(key)}:${canonicalJson(entry)}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function mapDocument(row: DocumentRow): PublicDocument {
  let file: PublicDocumentFile | null = null;
  if (row.file_id !== null) {
    const sizeBytes = Number(row.size_bytes);
    if (
      !row.original_name ||
      !Number.isSafeInteger(sizeBytes) ||
      sizeBytes < 0 ||
      !row.sha256
    ) {
      throw new Error("invalid_document_file_metadata");
    }
    file = {
      id: row.file_id,
      originalName: row.original_name,
      mimeType: row.mime_type,
      sizeBytes,
      sha256: row.sha256,
    };
  }
  return {
    id: row.id,
    kind: row.document_kind,
    title: row.title,
    markdownContent: row.markdown_content,
    file,
    sourceLocalDocumentId: row.source_local_document_id,
    createdAt: iso(row.created_at),
    updatedAt: iso(row.updated_at),
    deletedAt: row.deleted_at === null ? null : iso(row.deleted_at),
  };
}

async function findDocument(client: Pool | PoolClient, id: string): Promise<PublicDocument> {
  const result = await client.query<DocumentRow>(`${DOCUMENT_SELECT} WHERE d.id = $1`, [id]);
  if (result.rowCount !== 1 || !result.rows[0]) throw new Error("document_write_incomplete");
  return mapDocument(result.rows[0]);
}

async function softDelete(
  pool: Pool,
  predicate: "document" | "file",
  id: string,
  ownerUserId: string,
): Promise<DeletedDocument | null> {
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    const column = predicate === "document" ? "d.id" : "d.user_file_id";
    const found = await client.query<{ document_id: string; file_id: string | null; storage_key: string | null }>(
      `SELECT d.id AS document_id, d.user_file_id AS file_id, uf.storage_key
       FROM documents d
       LEFT JOIN user_files uf ON uf.id = d.user_file_id
       WHERE ${column} = $1 AND d.owner_user_id = $2 AND d.deleted_at IS NULL
       FOR UPDATE OF d`,
      [id, ownerUserId],
    );
    const row = found.rows[0];
    if (!row) {
      await client.query("ROLLBACK");
      return null;
    }
    await client.query(
      "UPDATE documents SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
      [row.document_id],
    );
    if (row.file_id) {
      await client.query(
        `UPDATE user_files SET deleted_at = COALESCE(deleted_at, CURRENT_TIMESTAMP)
         WHERE id = $1 AND owner_user_id = $2`,
        [row.file_id, ownerUserId],
      );
    }
    await client.query("COMMIT");
    return {
      documentId: row.document_id,
      fileId: row.file_id,
      storageKey: row.storage_key,
    };
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

export function createPostgresDocumentRepository(pool: Pool): DocumentRepository {
  return {
    async list(ownerUserId, kind, limit, offset) {
      const params: unknown[] = [ownerUserId];
      let kindClause = "";
      if (kind) {
        params.push(kind);
        kindClause = ` AND d.document_kind = $${params.length}`;
      }
      params.push(limit, offset);
      const result = await pool.query<DocumentRow>(
        `${DOCUMENT_SELECT}
         WHERE d.owner_user_id = $1 AND d.deleted_at IS NULL${kindClause}
         ORDER BY d.updated_at DESC, d.id DESC
         LIMIT $${params.length - 1} OFFSET $${params.length}`,
        params,
      );
      return result.rows.map(mapDocument);
    },

    async createMarkdown(ownerUserId, title, markdownContent) {
      const id = randomUUID();
      await pool.query(
        `INSERT INTO documents (
           id, owner_user_id, document_kind, title, markdown_content, created_at, updated_at
         ) VALUES ($1, $2, 'markdown', $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)`,
        [id, ownerUserId, title, markdownContent],
      );
      return findDocument(pool, id);
    },

    async createUploadedFile(ownerUserId, file) {
      const id = randomUUID();
      await pool.query(
        `INSERT INTO documents (
           id, owner_user_id, document_kind, title, markdown_content, user_file_id,
           created_at, updated_at
         ) VALUES ($1, $2, 'uploaded_file', $3, NULL, $4, $5, $5)`,
        [id, ownerUserId, file.originalName, file.id, file.createdAt],
      );
      return findDocument(pool, id);
    },

    async updateMarkdown(documentId, ownerUserId, updates) {
      const result = await pool.query<{ id: string }>(
        `UPDATE documents
         SET title = COALESCE($3, title),
             markdown_content = COALESCE($4, markdown_content),
             updated_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND owner_user_id = $2
           AND document_kind = 'markdown' AND deleted_at IS NULL
         RETURNING id`,
        [documentId, ownerUserId, updates.title ?? null, updates.markdownContent ?? null],
      );
      return result.rowCount === 1 ? findDocument(pool, documentId) : null;
    },

    deleteOwned(documentId, ownerUserId) {
      return softDelete(pool, "document", documentId, ownerUserId);
    },

    deleteByFileId(fileId, ownerUserId) {
      return softDelete(pool, "file", fileId, ownerUserId);
    },

    async upsertLocalMarkdown(ownerUserId, item) {
      const existing = await pool.query<ExistingImportRow>(
        `${DOCUMENT_SELECT}
         WHERE d.owner_user_id = $1 AND d.source_local_document_id = $2`,
        [ownerUserId, item.sourceLocalDocumentId],
      );
      const row = existing.rows[0];
      if (row) {
        const desiredDeletedAt = item.deletedAt ?? null;
        const unchanged =
          row.title === item.title &&
          row.markdown_content === item.markdownContent &&
          iso(row.created_at) === item.createdAt &&
          iso(row.updated_at) === item.updatedAt &&
          (row.deleted_at === null ? null : iso(row.deleted_at)) === desiredDeletedAt &&
          canonicalJson(row.legacy_metadata ?? {}) === canonicalJson(item.legacyMetadata);
        if (unchanged) {
          return { status: "skipped" as const, document: mapDocument(row) };
        }
        await pool.query(
          `UPDATE documents
           SET title = $3, markdown_content = $4, legacy_metadata = $5,
               created_at = $6, updated_at = $7, deleted_at = $8
           WHERE id = $1 AND owner_user_id = $2 AND document_kind = 'markdown'`,
          [
            row.id,
            ownerUserId,
            item.title,
            item.markdownContent,
            item.legacyMetadata,
            item.createdAt,
            item.updatedAt,
            desiredDeletedAt,
          ],
        );
        return { status: "updated" as const, document: await findDocument(pool, row.id) };
      }

      const id = randomUUID();
      await pool.query(
        `INSERT INTO documents (
           id, owner_user_id, document_kind, title, markdown_content,
           source_local_document_id, legacy_metadata, created_at, updated_at, deleted_at
         ) VALUES ($1, $2, 'markdown', $3, $4, $5, $6, $7, $8, $9)`,
        [
          id,
          ownerUserId,
          item.title,
          item.markdownContent,
          item.sourceLocalDocumentId,
          item.legacyMetadata,
          item.createdAt,
          item.updatedAt,
          item.deletedAt ?? null,
        ],
      );
      return { status: "imported" as const, document: await findDocument(pool, id) };
    },
  };
}

const MAX_TITLE_BYTES = 2_000;
const MAX_MARKDOWN_BYTES = 2 * 1024 * 1024;
const MAX_LEGACY_METADATA_BYTES = 64 * 1024;
const MAX_IMPORT_BATCH = 100;
const MAX_IMPORT_BATCH_BYTES = 20 * 1024 * 1024;

export class DocumentValidationError extends Error {}

function validTitle(value: unknown): string {
  if (typeof value !== "string") throw new DocumentValidationError("invalid_title");
  const title = value.trim();
  if (!title || Buffer.byteLength(title, "utf8") > MAX_TITLE_BYTES) {
    throw new DocumentValidationError("invalid_title");
  }
  return title;
}

function validMarkdown(value: unknown): string {
  if (typeof value !== "string" || Buffer.byteLength(value, "utf8") > MAX_MARKDOWN_BYTES) {
    throw new DocumentValidationError("invalid_markdown_content");
  }
  return value;
}

function validTimestamp(value: unknown, nullable = false): string | null {
  if (nullable && (value === null || value === undefined)) return null;
  if (typeof value !== "string") throw new DocumentValidationError("invalid_timestamp");
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) throw new DocumentValidationError("invalid_timestamp");
  return date.toISOString();
}

function containsAbsolutePath(value: unknown): boolean {
  if (typeof value === "string") {
    return /^[a-zA-Z]:[\\/]/.test(value) || /^\\\\/.test(value) || value.startsWith("/");
  }
  if (Array.isArray(value)) return value.some(containsAbsolutePath);
  if (typeof value === "object" && value !== null) {
    return Object.values(value).some(containsAbsolutePath);
  }
  return false;
}

function validLegacyMetadata(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new DocumentValidationError("invalid_legacy_metadata");
  }
  const serialized = JSON.stringify(value);
  if (
    Buffer.byteLength(serialized, "utf8") > MAX_LEGACY_METADATA_BYTES ||
    containsAbsolutePath(value)
  ) {
    throw new DocumentValidationError("invalid_legacy_metadata");
  }
  return value as Record<string, unknown>;
}

function validImportItem(value: unknown): LocalMarkdownImportItem {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new DocumentValidationError("invalid_import_item");
  }
  const item = value as Record<string, unknown>;
  if (
    typeof item.sourceLocalDocumentId !== "string" ||
    !item.sourceLocalDocumentId.trim() ||
    item.sourceLocalDocumentId.length > 128 ||
    /[\u0000-\u001f\u007f]/.test(item.sourceLocalDocumentId)
  ) {
    throw new DocumentValidationError("invalid_source_local_document_id");
  }
  return {
    sourceLocalDocumentId: item.sourceLocalDocumentId,
    title: validTitle(item.title),
    markdownContent: validMarkdown(item.markdownContent),
    createdAt: validTimestamp(item.createdAt)!,
    updatedAt: validTimestamp(item.updatedAt)!,
    deletedAt: validTimestamp(item.deletedAt, true),
    legacyMetadata: validLegacyMetadata(item.legacyMetadata),
  };
}

export function createDocumentService(repository: DocumentRepository): DocumentService {
  return {
    list(ownerUserId, kind, limit, offset) {
      return repository.list(ownerUserId, kind, limit, offset);
    },
    createMarkdown(ownerUserId, title, markdownContent) {
      return repository.createMarkdown(ownerUserId, validTitle(title), validMarkdown(markdownContent));
    },
    updateMarkdown(documentId, ownerUserId, updates) {
      const validated: { title?: string; markdownContent?: string } = {};
      if (updates.title !== undefined) validated.title = validTitle(updates.title);
      if (updates.markdownContent !== undefined) {
        validated.markdownContent = validMarkdown(updates.markdownContent);
      }
      if (validated.title === undefined && validated.markdownContent === undefined) {
        throw new DocumentValidationError("empty_update");
      }
      return repository.updateMarkdown(documentId, ownerUserId, validated);
    },
    createUploadedFile(ownerUserId, file) {
      return repository.createUploadedFile(ownerUserId, file);
    },
    deleteOwned(documentId, ownerUserId) {
      return repository.deleteOwned(documentId, ownerUserId);
    },
    deleteByFileId(fileId, ownerUserId) {
      return repository.deleteByFileId(fileId, ownerUserId);
    },
    upsertLocalMarkdown(ownerUserId, item) {
      return repository.upsertLocalMarkdown(ownerUserId, item);
    },
    async importLocalMarkdown(ownerUserId, rawItems) {
      if (!Array.isArray(rawItems) || rawItems.length === 0 || rawItems.length > MAX_IMPORT_BATCH) {
        throw new DocumentValidationError("invalid_import_batch");
      }
      let batchBytes = 0;
      const items: LocalMarkdownImportItem[] = [];
      let failed = 0;
      for (const rawItem of rawItems) {
        try {
          const item = validImportItem(rawItem);
          batchBytes += Buffer.byteLength(item.markdownContent, "utf8");
          if (batchBytes > MAX_IMPORT_BATCH_BYTES) {
            throw new DocumentValidationError("import_batch_too_large");
          }
          items.push(item);
        } catch {
          failed += 1;
        }
      }

      const result: ImportResult = {
        imported: 0,
        updated: 0,
        skipped: 0,
        failed,
        outcomes: [],
      };
      for (const item of items) {
        const outcome = await repository.upsertLocalMarkdown(ownerUserId, item);
        result[outcome.status] += 1;
        result.outcomes.push({
          sourceLocalDocumentId: item.sourceLocalDocumentId,
          documentId: outcome.document.id,
          status: outcome.status,
          contentSha256: createHash("sha256").update(outcome.document.markdownContent ?? "", "utf8").digest("hex"),
        });
      }
      return result;
    },
  };
}
