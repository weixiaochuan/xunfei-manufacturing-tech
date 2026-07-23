import { randomUUID } from "node:crypto";
import type { Readable } from "node:stream";
import { extname } from "node:path";
import type { Pool } from "pg";
import { FileStorageLimitError, type FileStorage } from "./storage/file-storage.js";

export interface PublicUserFile {
  id: string;
  originalName: string;
  mimeType: string | null;
  sizeBytes: number;
  sha256: string;
  createdAt: string;
}

export interface StoredUserFile extends PublicUserFile {
  storageKey: string;
}

export interface UploadInput {
  ownerUserId: string;
  originalName: string;
  mimeType: string | null;
  content: AsyncIterable<Buffer | string> | Iterable<Buffer | string>;
}

export interface DownloadedUserFile {
  file: PublicUserFile;
  content: Readable;
}

export interface ReplacedUserFileContent {
  file: PublicUserFile;
  documentId: string;
  revision: number;
  updatedAt: string;
}

export type ReplaceUserFileContentResult =
  | { status: "updated"; value: ReplacedUserFileContent }
  | { status: "not_found" }
  | { status: "conflict" }
  | { status: "file_type_mismatch" };

interface ReplacementMetadata {
  storageKey: string;
  mimeType: string | null;
  sizeBytes: number;
  sha256: string;
}

type RepositoryReplacementResult =
  | ({ status: "updated"; oldStorageKey: string; documentId: string; revision: number; updatedAt: string } & StoredUserFile)
  | { status: "not_found" }
  | { status: "conflict" };

export interface UserFileRepository {
  create(record: StoredUserFile & { ownerUserId: string }): Promise<void>;
  list(ownerUserId: string, limit: number, offset: number): Promise<StoredUserFile[]>;
  findOwnedActive(fileId: string, ownerUserId: string): Promise<StoredUserFile | null>;
  softDeleteOwned(fileId: string, ownerUserId: string): Promise<StoredUserFile | null>;
  hardDeleteOwned(fileId: string, ownerUserId: string): Promise<StoredUserFile | null>;
  replaceOwnedContent(
    fileId: string,
    ownerUserId: string,
    expectedSha256: string,
    replacement: ReplacementMetadata,
  ): Promise<RepositoryReplacementResult>;
}

export interface UserFileService {
  upload(input: UploadInput): Promise<PublicUserFile>;
  list(ownerUserId: string, limit: number, offset: number): Promise<PublicUserFile[]>;
  download(fileId: string, ownerUserId: string): Promise<DownloadedUserFile | null>;
  delete(fileId: string, ownerUserId: string): Promise<boolean>;
  rollbackUpload(fileId: string, ownerUserId: string): Promise<void>;
  removeStoredContent(storageKey: string): Promise<void>;
  replaceContent(input: {
    fileId: string;
    ownerUserId: string;
    expectedSha256: string;
    originalName: string;
    mimeType: string | null;
    content: AsyncIterable<Buffer | string> | Iterable<Buffer | string>;
  }): Promise<ReplaceUserFileContentResult>;
}

export class FileTooLargeError extends Error {}
export class FileContentUnavailableError extends Error {}

interface UserFileRow {
  id: string;
  original_name: string;
  storage_key: string;
  mime_type: string | null;
  size_bytes: string | number;
  sha256: string;
  created_at: Date | string;
}

function mapRow(row: UserFileRow): StoredUserFile {
  const sizeBytes = Number(row.size_bytes);
  if (!Number.isSafeInteger(sizeBytes) || sizeBytes < 0) {
    throw new Error("invalid_file_size");
  }
  return {
    id: row.id,
    originalName: row.original_name,
    storageKey: row.storage_key,
    mimeType: row.mime_type,
    sizeBytes,
    sha256: row.sha256,
    createdAt: new Date(row.created_at).toISOString(),
  };
}

function toPublicFile(file: StoredUserFile): PublicUserFile {
  return {
    id: file.id,
    originalName: file.originalName,
    mimeType: file.mimeType,
    sizeBytes: file.sizeBytes,
    sha256: file.sha256,
    createdAt: file.createdAt,
  };
}

export function sanitizeOriginalName(filename: string): string {
  const leaf = filename.replace(/\\/g, "/").split("/").pop() ?? "";
  const cleaned = leaf
    .replace(/[\u0000-\u001f\u007f]/g, "")
    .replace(/["'`;]/g, "_")
    .trim()
    .slice(0, 255);
  return cleaned || "unnamed-file";
}

export function sanitizeMimeType(mimeType: string | undefined): string | null {
  if (!mimeType) {
    return null;
  }
  const normalized = mimeType.trim().toLowerCase();
  return /^[a-z0-9][a-z0-9!#$&^_.+-]*\/[a-z0-9][a-z0-9!#$&^_.+-]*$/.test(normalized)
    ? normalized.slice(0, 127)
    : null;
}

export function createPostgresUserFileRepository(pool: Pool): UserFileRepository {
  return {
    async create(record) {
      await pool.query(
        `INSERT INTO user_files (
           id, owner_user_id, original_name, storage_key, mime_type, size_bytes, sha256, created_at
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`,
        [
          record.id,
          record.ownerUserId,
          record.originalName,
          record.storageKey,
          record.mimeType,
          record.sizeBytes,
          record.sha256,
          record.createdAt,
        ],
      );
    },

    async list(ownerUserId, limit, offset) {
      const result = await pool.query<UserFileRow>(
        `SELECT id, original_name, storage_key, mime_type, size_bytes, sha256, created_at
         FROM user_files
         WHERE owner_user_id = $1 AND deleted_at IS NULL
         ORDER BY created_at DESC, id DESC
         LIMIT $2 OFFSET $3`,
        [ownerUserId, limit, offset],
      );
      return result.rows.map(mapRow);
    },

    async findOwnedActive(fileId, ownerUserId) {
      const result = await pool.query<UserFileRow>(
        `SELECT id, original_name, storage_key, mime_type, size_bytes, sha256, created_at
         FROM user_files
         WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL`,
        [fileId, ownerUserId],
      );
      return result.rowCount === 1 && result.rows[0] ? mapRow(result.rows[0]) : null;
    },

    async softDeleteOwned(fileId, ownerUserId) {
      const result = await pool.query<UserFileRow>(
        `UPDATE user_files
         SET deleted_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL
         RETURNING id, original_name, storage_key, mime_type, size_bytes, sha256, created_at`,
        [fileId, ownerUserId],
      );
      return result.rowCount === 1 && result.rows[0] ? mapRow(result.rows[0]) : null;
    },

    async hardDeleteOwned(fileId, ownerUserId) {
      const result = await pool.query<UserFileRow>(
        `DELETE FROM user_files
         WHERE id = $1 AND owner_user_id = $2
         RETURNING id, original_name, storage_key, mime_type, size_bytes, sha256, created_at`,
        [fileId, ownerUserId],
      );
      return result.rowCount === 1 && result.rows[0] ? mapRow(result.rows[0]) : null;
    },

    async replaceOwnedContent(fileId, ownerUserId, expectedSha256, replacement) {
      const client = await pool.connect();
      try {
        await client.query("BEGIN");
        const found = await client.query<UserFileRow & { document_id: string; revision: string | number }>(
          `SELECT uf.id, uf.original_name, uf.storage_key, uf.mime_type, uf.size_bytes,
                  uf.sha256, uf.created_at, d.id AS document_id, d.revision
           FROM user_files uf
           JOIN documents d ON d.user_file_id = uf.id
           WHERE uf.id = $1 AND uf.owner_user_id = $2 AND uf.deleted_at IS NULL
             AND d.owner_user_id = $2 AND d.document_kind = 'uploaded_file' AND d.deleted_at IS NULL
           FOR UPDATE OF uf, d`,
          [fileId, ownerUserId],
        );
        const current = found.rows[0];
        if (!current) {
          await client.query("ROLLBACK");
          return { status: "not_found" } as const;
        }
        if (current.sha256 !== expectedSha256) {
          await client.query("ROLLBACK");
          return { status: "conflict" } as const;
        }
        const fileUpdate = await client.query<UserFileRow>(
          `UPDATE user_files
           SET storage_key = $3, mime_type = $4, size_bytes = $5, sha256 = $6
           WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL AND sha256 = $7
           RETURNING id, original_name, storage_key, mime_type, size_bytes, sha256, created_at`,
          [fileId, ownerUserId, replacement.storageKey, replacement.mimeType,
            replacement.sizeBytes, replacement.sha256, expectedSha256],
        );
        if (fileUpdate.rowCount !== 1 || !fileUpdate.rows[0]) {
          await client.query("ROLLBACK");
          return { status: "conflict" } as const;
        }
        const documentUpdate = await client.query<{ revision: string | number; updated_at: Date | string }>(
          `UPDATE documents
           SET revision = revision + 1, updated_at = CURRENT_TIMESTAMP
           WHERE id = $1 AND owner_user_id = $2 AND document_kind = 'uploaded_file'
             AND deleted_at IS NULL
           RETURNING revision, updated_at`,
          [current.document_id, ownerUserId],
        );
        if (documentUpdate.rowCount !== 1 || !documentUpdate.rows[0]) {
          throw new Error("uploaded_document_update_incomplete");
        }
        await client.query("COMMIT");
        const updated = mapRow(fileUpdate.rows[0]);
        return {
          status: "updated" as const,
          ...updated,
          oldStorageKey: current.storage_key,
          documentId: current.document_id,
          revision: Number(documentUpdate.rows[0].revision),
          updatedAt: new Date(documentUpdate.rows[0].updated_at).toISOString(),
        };
      } catch (error) {
        await client.query("ROLLBACK").catch(() => undefined);
        throw error;
      } finally {
        client.release();
      }
    },
  };
}

export function createUserFileService(
  repository: UserFileRepository,
  storage: FileStorage,
  maxBytes: number,
): UserFileService {
  return {
    async upload(input) {
      let stored;
      try {
        stored = await storage.writeFile(input.content, maxBytes);
        const record: StoredUserFile & { ownerUserId: string } = {
          id: randomUUID(),
          ownerUserId: input.ownerUserId,
          originalName: sanitizeOriginalName(input.originalName),
          storageKey: stored.storageKey,
          mimeType: input.mimeType,
          sizeBytes: stored.sizeBytes,
          sha256: stored.sha256,
          createdAt: new Date().toISOString(),
        };

        try {
          await repository.create(record);
        } catch (error) {
          await storage.deleteFile(record.storageKey).catch(() => undefined);
          throw error;
        }
        return toPublicFile(record);
      } catch (error) {
        if (error instanceof FileStorageLimitError) throw new FileTooLargeError();
        throw error;
      }
    },

    async list(ownerUserId, limit, offset) {
      return (await repository.list(ownerUserId, limit, offset)).map(toPublicFile);
    },

    async download(fileId, ownerUserId) {
      const file = await repository.findOwnedActive(fileId, ownerUserId);
      if (!file) {
        return null;
      }
      try {
        const verification = await storage.verifyFile(file.storageKey, file.sizeBytes, file.sha256);
        if (!verification.exists || !verification.sizeMatches || !verification.sha256Matches) {
          throw new Error("stored_file_verification_failed");
        }
        return {
          file: toPublicFile(file),
          content: await storage.createReadStream(file.storageKey),
        };
      } catch {
        throw new FileContentUnavailableError();
      }
    },

    async delete(fileId, ownerUserId) {
      const file = await repository.softDeleteOwned(fileId, ownerUserId);
      if (!file) {
        return false;
      }
      await storage.deleteFile(file.storageKey);
      return true;
    },

    async rollbackUpload(fileId, ownerUserId) {
      const file = await repository.hardDeleteOwned(fileId, ownerUserId);
      if (file) {
        await storage.deleteFile(file.storageKey);
      }
    },

    async removeStoredContent(storageKey) {
      await storage.deleteFile(storageKey);
    },

    async replaceContent(input) {
      const current = await repository.findOwnedActive(input.fileId, input.ownerUserId);
      if (!current) return { status: "not_found" };
      if (current.sha256 !== input.expectedSha256) return { status: "conflict" };
      if (extname(current.originalName).toLowerCase() !== extname(sanitizeOriginalName(input.originalName)).toLowerCase()) {
        return { status: "file_type_mismatch" };
      }

      let replacement;
      try {
        replacement = await storage.writeFile(input.content, maxBytes);
      } catch (error) {
        if (error instanceof FileStorageLimitError) throw new FileTooLargeError();
        throw error;
      }
      try {
        const result = await repository.replaceOwnedContent(
          input.fileId,
          input.ownerUserId,
          input.expectedSha256,
          { ...replacement, mimeType: input.mimeType },
        );
        if (result.status !== "updated") {
          await storage.deleteFile(replacement.storageKey).catch(() => undefined);
          return result;
        }
        await storage.deleteFile(result.oldStorageKey).catch(() => undefined);
        return {
          status: "updated",
          value: {
            file: toPublicFile(result),
            documentId: result.documentId,
            revision: result.revision,
            updatedAt: result.updatedAt,
          },
        };
      } catch (error) {
        await storage.deleteFile(replacement.storageKey).catch(() => undefined);
        throw error;
      }
    },
  };
}
