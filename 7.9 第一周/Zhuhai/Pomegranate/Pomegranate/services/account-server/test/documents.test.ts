import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import type { Pool } from "pg";
import type { AccountServerConfig } from "../src/config.js";
import {
  createDocumentService,
  type DeletedDocument,
  type DocumentKind,
  type DocumentRepository,
  type LocalMarkdownImportItem,
  type PublicDocument,
} from "../src/documents.js";
import type { OidcClient } from "../src/oidc.js";
import { buildServer } from "../src/server.js";
import type { SessionService, SessionUser } from "../src/sessions.js";
import { LocalFilesystemStorage } from "../src/storage/local-filesystem-storage.js";
import {
  DocumentLibraryValidationError,
  type CatalogDocument,
  type DocumentFilters,
  type DocumentLibraryService,
  type MarkdownMutation,
  type PublicDocumentFolder,
  type PublicDocumentTag,
} from "../src/document-library.js";
import {
  createUserFileService,
  type StoredUserFile,
  type UserFileRepository,
} from "../src/user-files.js";

const TOKEN_A = "a".repeat(43);
const TOKEN_B = "b".repeat(43);
const USER_A: SessionUser = {
  platformUserId: "11111111-1111-4111-8111-111111111111",
  accountNumber: "POME-000001",
  username: "alice",
  displayName: "Alice",
  email: null,
};
const USER_B: SessionUser = {
  platformUserId: "22222222-2222-4222-8222-222222222222",
  accountNumber: "POME-000004",
  username: "bob",
  displayName: "Bob",
  email: null,
};

interface MemoryDocument {
  ownerUserId: string;
  public: PublicDocument;
  legacyMetadata: Record<string, unknown> | null;
}

class MemoryDocuments implements DocumentRepository {
  readonly records = new Map<string, MemoryDocument>();

  constructor(private readonly onFileDelete: (fileId: string, ownerUserId: string) => Promise<void>) {}

  async list(ownerUserId: string, kind: DocumentKind | null, limit: number, offset: number) {
    return [...this.records.values()]
      .filter((record) => record.ownerUserId === ownerUserId && record.public.deletedAt === null)
      .filter((record) => kind === null || record.public.kind === kind)
      .sort((a, b) => b.public.updatedAt.localeCompare(a.public.updatedAt))
      .slice(offset, offset + limit)
      .map((record) => record.public);
  }

  async createMarkdown(ownerUserId: string, title: string, markdownContent: string) {
    const now = new Date().toISOString();
    const document: PublicDocument = {
      id: randomUUID(), kind: "markdown", title, markdownContent, file: null,
      sourceLocalDocumentId: null, createdAt: now, updatedAt: now, deletedAt: null,
    };
    this.records.set(document.id, { ownerUserId, public: document, legacyMetadata: null });
    return document;
  }

  async createUploadedFile(ownerUserId: string, file: StoredUserFile) {
    if ([...this.records.values()].some((record) => record.public.file?.id === file.id)) {
      throw new Error("duplicate_file_document");
    }
    const document: PublicDocument = {
      id: randomUUID(), kind: "uploaded_file", title: file.originalName,
      markdownContent: null, file, sourceLocalDocumentId: null,
      createdAt: file.createdAt, updatedAt: file.createdAt, deletedAt: null,
    };
    this.records.set(document.id, { ownerUserId, public: document, legacyMetadata: null });
    return document;
  }

  async updateMarkdown(documentId: string, ownerUserId: string, updates: { title?: string; markdownContent?: string }) {
    const record = this.records.get(documentId);
    if (!record || record.ownerUserId !== ownerUserId || record.public.kind !== "markdown" || record.public.deletedAt) return null;
    record.public = { ...record.public, ...updates, updatedAt: new Date().toISOString() };
    return record.public;
  }

  async deleteOwned(documentId: string, ownerUserId: string) {
    const record = this.records.get(documentId);
    if (!record || record.ownerUserId !== ownerUserId || record.public.deletedAt) return null;
    const now = new Date().toISOString();
    record.public = { ...record.public, updatedAt: now, deletedAt: now };
    return { documentId, fileId: record.public.file?.id ?? null, storageKey: null };
  }

  async deleteByFileId(fileId: string, ownerUserId: string): Promise<DeletedDocument | null> {
    const entry = [...this.records.entries()].find(([, record]) =>
      record.ownerUserId === ownerUserId && record.public.file?.id === fileId && !record.public.deletedAt);
    if (!entry) return null;
    await this.onFileDelete(fileId, ownerUserId);
    return this.deleteOwned(entry[0], ownerUserId);
  }

  async upsertLocalMarkdown(ownerUserId: string, item: LocalMarkdownImportItem) {
    const existing = [...this.records.values()].find((record) =>
      record.ownerUserId === ownerUserId && record.public.sourceLocalDocumentId === item.sourceLocalDocumentId);
    if (!existing) {
      const document: PublicDocument = {
        id: randomUUID(), kind: "markdown", title: item.title,
        markdownContent: item.markdownContent, file: null,
        sourceLocalDocumentId: item.sourceLocalDocumentId,
        createdAt: item.createdAt, updatedAt: item.updatedAt, deletedAt: item.deletedAt ?? null,
      };
      this.records.set(document.id, { ownerUserId, public: document, legacyMetadata: item.legacyMetadata });
      return { status: "imported" as const, document };
    }
    const unchanged = existing.public.title === item.title &&
      existing.public.markdownContent === item.markdownContent &&
      existing.public.createdAt === item.createdAt &&
      existing.public.updatedAt === item.updatedAt &&
      existing.public.deletedAt === (item.deletedAt ?? null) &&
      JSON.stringify(existing.legacyMetadata) === JSON.stringify(item.legacyMetadata);
    if (unchanged) return { status: "skipped" as const, document: existing.public };
    existing.public = {
      ...existing.public, title: item.title, markdownContent: item.markdownContent,
      createdAt: item.createdAt, updatedAt: item.updatedAt, deletedAt: item.deletedAt ?? null,
    };
    existing.legacyMetadata = item.legacyMetadata;
    return { status: "updated" as const, document: existing.public };
  }
}

class MemoryLibrary implements DocumentLibraryService {
  readonly catalog = new Map<string, CatalogDocument>();
  readonly folders = new Map<string, PublicDocumentFolder & { ownerUserId: string }>();
  readonly tags = new Map<string, PublicDocumentTag & { ownerUserId: string }>();

  constructor(private readonly documents: MemoryDocuments) {}

  private view(record: MemoryDocument): CatalogDocument {
    const existing = this.catalog.get(record.public.id);
    if (existing) return { ...existing, deletedAt: record.public.deletedAt, updatedAt: record.public.updatedAt };
    return {
      ...record.public, folder: null, tags: [], diaryDate: null, isPinned: false, isHidden: false,
      sortOrder: 0, wordCount: 0, contentSha256: null, revision: 1,
    };
  }

  async list(ownerUserId: string, filters: DocumentFilters) {
    return [...this.documents.records.values()].filter((record) => record.ownerUserId === ownerUserId)
      .map((record) => this.view(record))
      .filter((document) => filters.kind === null || document.kind === filters.kind)
      .filter((document) => filters.folderId === null || document.folder?.id === filters.folderId)
      .filter((document) => filters.tagId === null || document.tags.some((tag) => tag.id === filters.tagId))
      .filter((document) => filters.diaryDate === null || document.diaryDate === filters.diaryDate)
      .filter((document) => document.isHidden === filters.hidden)
      .filter((document) => filters.deleted ? document.deletedAt !== null : document.deletedAt === null)
      .sort((a, b) => Number(b.isPinned) - Number(a.isPinned) || a.sortOrder - b.sortOrder || b.updatedAt.localeCompare(a.updatedAt))
      .slice(filters.offset, filters.offset + filters.limit);
  }

  async listFolders(ownerUserId: string) { return [...this.folders.values()].filter((folder) => folder.ownerUserId === ownerUserId).map(({ ownerUserId: _, ...folder }) => folder); }
  async createFolder(ownerUserId: string, rawName: unknown, rawParentId: unknown) {
    if (typeof rawName !== "string" || !rawName.trim()) throw new Error("invalid_folder_name");
    const parentId = rawParentId === null || rawParentId === undefined ? null : String(rawParentId);
    if (parentId && this.folders.get(parentId)?.ownerUserId !== ownerUserId) throw new DocumentLibraryValidationError("invalid_folder");
    const now = new Date().toISOString(); const folder = { id: randomUUID(), name: rawName.trim(), parentId, createdAt: now, updatedAt: now, ownerUserId };
    this.folders.set(folder.id, folder); const { ownerUserId: _, ...result } = folder; return result;
  }
  async updateFolder(ownerUserId: string, folderId: string, rawName: unknown, rawParentId: unknown) {
    const folder = this.folders.get(folderId); if (!folder || folder.ownerUserId !== ownerUserId) return null;
    const parentId = rawParentId === undefined ? folder.parentId : rawParentId === null ? null : String(rawParentId);
    if (parentId === folderId) throw new DocumentLibraryValidationError("folder_cycle");
    let cursor = parentId;
    while (cursor) { const parent = this.folders.get(cursor); if (!parent || parent.ownerUserId !== ownerUserId) throw new DocumentLibraryValidationError("invalid_folder"); if (parent.parentId === folderId) throw new DocumentLibraryValidationError("folder_cycle"); cursor = parent.parentId; }
    if (rawName !== undefined) { if (typeof rawName !== "string" || !rawName.trim()) throw new Error("invalid_folder_name"); folder.name = rawName.trim(); }
    folder.parentId = parentId; folder.updatedAt = new Date().toISOString(); const { ownerUserId: _, ...result } = folder; return result;
  }
  async deleteFolder(ownerUserId: string, folderId: string) {
    const folder = this.folders.get(folderId); if (!folder || folder.ownerUserId !== ownerUserId) return false;
    this.folders.delete(folderId); for (const item of this.catalog.values()) if (item.folder?.id === folderId) item.folder = null;
    for (const child of this.folders.values()) if (child.ownerUserId === ownerUserId && child.parentId === folderId) child.parentId = null; return true;
  }
  async listTags(ownerUserId: string) { return [...this.tags.values()].filter((tag) => tag.ownerUserId === ownerUserId).map(({ ownerUserId: _, ...tag }) => tag); }
  async createTag(ownerUserId: string, rawName: unknown) {
    if (typeof rawName !== "string" || !rawName.trim()) throw new Error("invalid_tag_name");
    if ([...this.tags.values()].some((tag) => tag.ownerUserId === ownerUserId && tag.name.toLowerCase() === rawName.trim().toLowerCase())) throw new Error("tag_name_exists");
    const now = new Date().toISOString(); const tag = { id: randomUUID(), name: rawName.trim(), createdAt: now, updatedAt: now, ownerUserId }; this.tags.set(tag.id, tag); const { ownerUserId: _, ...result } = tag; return result;
  }
  async updateTag(ownerUserId: string, tagId: string, rawName: unknown) { const tag = this.tags.get(tagId); if (!tag || tag.ownerUserId !== ownerUserId) return null; if (typeof rawName !== "string" || !rawName.trim()) throw new Error("invalid_tag_name"); tag.name = rawName.trim(); tag.updatedAt = new Date().toISOString(); const { ownerUserId: _, ...result } = tag; return result; }
  async deleteTag(ownerUserId: string, tagId: string) { const tag = this.tags.get(tagId); if (!tag || tag.ownerUserId !== ownerUserId) return false; this.tags.delete(tagId); for (const item of this.catalog.values()) item.tags = item.tags.filter((entry) => entry.id !== tagId); return true; }
  async createMarkdown(ownerUserId: string, input: MarkdownMutation) {
    const content = input.markdownContent!; const folder = input.folderId ? this.folders.get(input.folderId) : null;
    if (input.folderId && folder?.ownerUserId !== ownerUserId) throw new DocumentLibraryValidationError("invalid_folder");
    const tags = (input.tagIds ?? []).map((id) => this.tags.get(id)).filter((tag) => tag?.ownerUserId === ownerUserId) as Array<PublicDocumentTag & { ownerUserId: string }>;
    if (tags.length !== (input.tagIds ?? []).length) throw new DocumentLibraryValidationError("invalid_tags");
    const base = await this.documents.createMarkdown(ownerUserId, input.title!, input.markdownContent!);
    const document: CatalogDocument = { ...base, folder: folder ? (({ ownerUserId: _, ...value }) => value)(folder) : null, tags: tags.map(({ ownerUserId: _, ...tag }) => tag), diaryDate: input.diaryDate ?? null, isPinned: input.isPinned ?? false, isHidden: input.isHidden ?? false, sortOrder: input.sortOrder ?? 0, wordCount: [...content.replaceAll(" ", "")].length, contentSha256: await import("node:crypto").then(({ createHash }) => createHash("sha256").update(content).digest("hex")), revision: 1 };
    this.catalog.set(document.id, document); return document;
  }
  async updateMarkdown(ownerUserId: string, documentId: string, expectedRevision: unknown, input: MarkdownMutation) {
    const record = this.documents.records.get(documentId); const current = this.catalog.get(documentId);
    if (!record || record.ownerUserId !== ownerUserId || !current || current.deletedAt) return { status: "not_found" as const };
    if (current.revision !== expectedRevision) return { status: "conflict" as const };
    const content = input.markdownContent ?? current.markdownContent!; const updatedAt = new Date().toISOString();
    const next: CatalogDocument = { ...current, ...input, markdownContent: content, folder: input.folderId === undefined ? current.folder : input.folderId === null ? null : (({ ownerUserId: _, ...folder }) => folder)(this.folders.get(input.folderId)!), tags: input.tagIds === undefined ? current.tags : input.tagIds.map((id) => (({ ownerUserId: _, ...tag }) => tag)(this.tags.get(id)!)), wordCount: [...content.replaceAll(" ", "")].length, contentSha256: await import("node:crypto").then(({ createHash }) => createHash("sha256").update(content).digest("hex")), revision: current.revision + 1, updatedAt };
    delete (next as unknown as Record<string, unknown>).folderId; delete (next as unknown as Record<string, unknown>).tagIds;
    this.catalog.set(documentId, next); record.public = { ...record.public, title: next.title, markdownContent: content, updatedAt }; return { status: "updated" as const, document: next };
  }
  async restore(ownerUserId: string, documentId: string) { const record = this.documents.records.get(documentId); if (!record || record.ownerUserId !== ownerUserId || !record.public.deletedAt) return { status: "not_found" as const }; if (record.public.kind === "uploaded_file") return { status: "file_content_unavailable" as const }; record.public = { ...record.public, deletedAt: null, updatedAt: new Date().toISOString() }; return { status: "restored" as const, document: this.view(record) }; }
  async importLocalMetadata(ownerUserId: string, input: unknown) {
    const value = input as { folders: Array<{ sourceLocalFolderId: string }>; tags: Array<{ sourceLocalTagId: string }>; tagLinks: unknown[] };
    return { folders: value.folders.length, tags: value.tags.length, links: value.tagLinks.length };
  }
}

class MemoryFiles implements UserFileRepository {
  readonly records = new Map<string, StoredUserFile & { ownerUserId: string; deleted: boolean }>();
  async create(record: StoredUserFile & { ownerUserId: string }) { this.records.set(record.id, { ...record, deleted: false }); }
  async list(ownerUserId: string, limit: number, offset: number) {
    return [...this.records.values()].filter((r) => r.ownerUserId === ownerUserId && !r.deleted).slice(offset, offset + limit);
  }
  async findOwnedActive(fileId: string, ownerUserId: string) {
    const record = this.records.get(fileId); return record?.ownerUserId === ownerUserId && !record.deleted ? record : null;
  }
  async softDeleteOwned(fileId: string, ownerUserId: string) {
    const record = await this.findOwnedActive(fileId, ownerUserId); if (!record) return null; record.deleted = true; return record;
  }
  async hardDeleteOwned(fileId: string, ownerUserId: string) {
    const record = this.records.get(fileId); if (!record || record.ownerUserId !== ownerUserId) return null;
    this.records.delete(fileId); return record;
  }
  async replaceOwnedContent(fileId: string, ownerUserId: string, expectedSha256: string, replacement: { storageKey: string; mimeType: string | null; sizeBytes: number; sha256: string }) {
    const record = await this.findOwnedActive(fileId, ownerUserId);
    if (!record) return { status: "not_found" as const };
    if (record.sha256 !== expectedSha256) return { status: "conflict" as const };
    const oldStorageKey = record.storageKey; Object.assign(record, replacement);
    return { status: "updated" as const, ...record, oldStorageKey, documentId: fileId, revision: 2, updatedAt: new Date().toISOString() };
  }
}

function sessions(): SessionService {
  return {
    create: async (user) => ({ token: TOKEN_A, user }),
    findActive: async (token) => token === TOKEN_A ? USER_A : token === TOKEN_B ? USER_B : null,
    revoke: async () => undefined,
  };
}

function config(root: string): AccountServerConfig {
  return {
    deploymentProfile: "local",
    server: { host: "127.0.0.1", port: 3010, publicUrl: "http://127.0.0.1:3010" },
    database: { host: "127.0.0.1", port: 5432, database: "test", user: "test", password: "test", connectionTimeoutMillis: 5000 },
    oidc: { baseUrl: "http://127.0.0.1:8000", clientId: "test", clientSecret: "test", redirectUri: "http://127.0.0.1:3010/auth/callback", organization: "pomegranate", application: "app-pomegranate" },
    session: { ttlSeconds: 60 }, userFiles: { backend: "filesystem", root, maxBytes: 1024 }, nodeEnv: "test",
  };
}

const UNUSED_POOL = { query: async () => ({ rows: [{ value: 1 }], rowCount: 1 }) } as unknown as Pool;
const UNUSED_OIDC = {} as OidcClient;

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "pomegranate-documents-test-"));
  const fileRepository = new MemoryFiles();
  const documentRepository = new MemoryDocuments(async (fileId, ownerUserId) => {
    await fileRepository.softDeleteOwned(fileId, ownerUserId);
  });
  const documents = createDocumentService(documentRepository);
  const library = new MemoryLibrary(documentRepository);
  const storage = new LocalFilesystemStorage(root);
  await storage.initialize();
  const userFiles = createUserFileService(fileRepository, storage, 1024);
  const server = buildServer({ pool: UNUSED_POOL, config: config(root), oidcClient: UNUSED_OIDC, sessionService: sessions(), documentService: documents, documentLibraryService: library, userFileService: userFiles, logger: false });
  return { root, documentRepository, documents, library, fileRepository, server, close: async () => { await server.close(); await rm(root, { recursive: true, force: true }); } };
}

function auth(token = TOKEN_A) { return { authorization: `Bearer ${token}` }; }

test("markdown create, list, update, soft delete, and owner isolation", async (t) => {
  const f = await fixture(); t.after(f.close);
  const created = await f.server.inject({ method: "POST", url: "/documents/markdown", headers: auth(), payload: { title: "First", markdownContent: "# body" } });
  assert.equal(created.statusCode, 201);
  const id = created.json().document.id;
  assert.equal((await f.server.inject({ method: "GET", url: "/documents", headers: auth() })).json().documents.length, 1);
  assert.equal((await f.server.inject({ method: "GET", url: "/documents", headers: auth(TOKEN_B) })).json().documents.length, 0);
  for (const method of ["PATCH", "DELETE"] as const) {
    const response = await f.server.inject({ method, url: `/documents/${id}`, headers: auth(TOKEN_B), ...(method === "PATCH" ? { payload: { expectedRevision: 1, title: "stolen" } } : {}) });
    assert.equal(response.statusCode, 404);
  }
  const updated = await f.server.inject({ method: "PATCH", url: `/documents/${id}`, headers: auth(), payload: { expectedRevision: 1, title: "Updated", markdownContent: "new" } });
  assert.equal(updated.statusCode, 200); assert.equal(updated.json().document.id, id);
  assert.equal((await f.server.inject({ method: "DELETE", url: `/documents/${id}`, headers: auth() })).statusCode, 200);
  assert.equal((await f.server.inject({ method: "GET", url: "/documents", headers: auth() })).json().documents.length, 0);
});

test("uploaded files appear in documents and file deletion hides both records", async (t) => {
  const f = await fixture(); t.after(f.close);
  const boundary = "----documents-file";
  const payload = Buffer.from(`--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="sample.pdf"\r\nContent-Type: application/pdf\r\n\r\n%PDF harmless\r\n--${boundary}--\r\n`);
  const upload = await f.server.inject({ method: "POST", url: "/files", headers: { ...auth(), "content-type": `multipart/form-data; boundary=${boundary}` }, payload });
  assert.equal(upload.statusCode, 201);
  const fileId = upload.json().file.id;
  const listed = await f.server.inject({ method: "GET", url: "/documents", headers: auth() });
  assert.equal(listed.json().documents.length, 1); assert.equal(listed.json().documents[0].kind, "uploaded_file");
  assert.equal((await f.server.inject({ method: "GET", url: "/documents", headers: auth(TOKEN_B) })).json().documents.length, 0);
  assert.equal((await f.server.inject({ method: "DELETE", url: `/files/${fileId}`, headers: auth(TOKEN_B) })).statusCode, 404);
  assert.equal((await f.server.inject({ method: "DELETE", url: `/files/${fileId}`, headers: auth() })).statusCode, 200);
  assert.equal((await f.server.inject({ method: "GET", url: "/documents", headers: auth() })).json().documents.length, 0);
  assert.equal((await f.server.inject({ method: "GET", url: "/files", headers: auth() })).json().files.length, 0);
});

test("local Markdown import is idempotent, updates in place, and is owner-scoped", async (t) => {
  const f = await fixture(); t.after(f.close);
  const item = { sourceLocalDocumentId: "sqlite-note:1", title: "Legacy", markdownContent: "one", createdAt: "2026-07-20T00:00:00.000Z", updatedAt: "2026-07-20T01:00:00.000Z", legacyMetadata: { pinned: false } };
  const first = await f.server.inject({ method: "POST", url: "/documents/import-local-markdown", headers: auth(), payload: { documents: [item] } });
  assert.equal(first.statusCode, 200); assert.equal(first.json().imported, 1);
  const id = first.json().outcomes[0].documentId;
  const second = await f.server.inject({ method: "POST", url: "/documents/import-local-markdown", headers: auth(), payload: { documents: [item] } });
  assert.equal(second.json().skipped, 1); assert.equal(second.json().outcomes[0].documentId, id);
  const third = await f.server.inject({ method: "POST", url: "/documents/import-local-markdown", headers: auth(), payload: { documents: [{ ...item, markdownContent: "two" }] } });
  assert.equal(third.json().updated, 1); assert.equal(third.json().outcomes[0].documentId, id);
  const other = await f.server.inject({ method: "POST", url: "/documents/import-local-markdown", headers: auth(TOKEN_B), payload: { documents: [item] } });
  assert.equal(other.json().imported, 1); assert.notEqual(other.json().outcomes[0].documentId, id);
});

test("invalid imports and repository failures return safe errors without Markdown or secrets", async (t) => {
  const f = await fixture(); t.after(f.close);
  const invalid = await f.server.inject({ method: "POST", url: "/documents/import-local-markdown", headers: auth(), payload: { documents: [{ sourceLocalDocumentId: "x", title: "x", markdownContent: "private body", createdAt: "bad", updatedAt: "bad", legacyMetadata: {} }] } });
  assert.equal(invalid.statusCode, 200); assert.equal(invalid.json().failed, 1); assert.doesNotMatch(invalid.body, /private body/);
  f.documentRepository.upsertLocalMarkdown = async () => { throw new Error("SQL password token private body stack"); };
  const failure = await f.server.inject({ method: "POST", url: "/documents/import-local-markdown", headers: auth(), payload: { documents: [{ sourceLocalDocumentId: "x", title: "x", markdownContent: "private body", createdAt: "2026-07-20T00:00:00.000Z", updatedAt: "2026-07-20T00:00:00.000Z", legacyMetadata: {} }] } });
  assert.equal(failure.statusCode, 503); assert.doesNotMatch(failure.body, /SQL|password|token|private body|stack/i);
});

test("folder CRUD is owner scoped, prevents cycles, and safely unclassifies documents", async (t) => {
  const f = await fixture(); t.after(f.close);
  const parentResponse = await f.server.inject({ method: "POST", url: "/document-folders", headers: auth(), payload: { name: "课程" } });
  assert.equal(parentResponse.statusCode, 201);
  const parentId = parentResponse.json().folder.id;
  const childResponse = await f.server.inject({ method: "POST", url: "/document-folders", headers: auth(), payload: { name: "第一章", parentId } });
  assert.equal(childResponse.statusCode, 201);
  const childId = childResponse.json().folder.id;
  assert.equal((await f.server.inject({ method: "GET", url: "/document-folders", headers: auth(TOKEN_B) })).json().folders.length, 0);
  assert.equal((await f.server.inject({ method: "PATCH", url: `/document-folders/${parentId}`, headers: auth(TOKEN_B), payload: { name: "越权" } })).statusCode, 404);
  const cycle = await f.server.inject({ method: "PATCH", url: `/document-folders/${parentId}`, headers: auth(), payload: { parentId: childId } });
  assert.equal(cycle.statusCode, 400); assert.equal(cycle.json().error, "folder_cycle");
  const renamed = await f.server.inject({ method: "PATCH", url: `/document-folders/${childId}`, headers: auth(), payload: { name: "第二章" } });
  assert.equal(renamed.statusCode, 200); assert.equal(renamed.json().folder.name, "第二章");
  const document = await f.server.inject({ method: "POST", url: "/documents/markdown", headers: auth(), payload: { title: "归档测试", markdownContent: "内容", folderId: childId } });
  assert.equal(document.statusCode, 201);
  assert.equal((await f.server.inject({ method: "DELETE", url: `/document-folders/${childId}`, headers: auth() })).statusCode, 200);
  const listed = await f.server.inject({ method: "GET", url: "/documents", headers: auth() });
  assert.equal(listed.json().documents[0].folder, null);
});

test("tags, metadata filters, revision conflicts, trash, and restore behave safely", async (t) => {
  const f = await fixture(); t.after(f.close);
  const folderId = (await f.server.inject({ method: "POST", url: "/document-folders", headers: auth(), payload: { name: "日记" } })).json().folder.id;
  const tagId = (await f.server.inject({ method: "POST", url: "/document-tags", headers: auth(), payload: { name: "重点" } })).json().tag.id;
  const otherTagId = (await f.server.inject({ method: "POST", url: "/document-tags", headers: auth(TOKEN_B), payload: { name: "私有" } })).json().tag.id;
  const forbidden = await f.server.inject({ method: "POST", url: "/documents/markdown", headers: auth(), payload: { title: "越权", markdownContent: "x", tagIds: [otherTagId] } });
  assert.equal(forbidden.statusCode, 400); assert.equal(forbidden.json().error, "invalid_tags");
  const created = await f.server.inject({ method: "POST", url: "/documents/markdown", headers: auth(), payload: { title: "自动保存", markdownContent: "a b文", folderId, diaryDate: "2026-07-23", isPinned: true, isHidden: true, sortOrder: 7, tagIds: [tagId] } });
  assert.equal(created.statusCode, 201);
  const initial = created.json().document;
  assert.equal(initial.wordCount, 3); assert.match(initial.contentSha256, /^[0-9a-f]{64}$/); assert.equal(initial.revision, 1);
  assert.equal((await f.server.inject({ method: "GET", url: "/documents", headers: auth() })).json().documents.length, 0);
  const filtered = await f.server.inject({ method: "GET", url: `/documents?hidden=true&folderId=${folderId}&tagId=${tagId}&diaryDate=2026-07-23`, headers: auth() });
  assert.equal(filtered.statusCode, 200); assert.equal(filtered.json().documents.length, 1);
  const firstUpdate = await f.server.inject({ method: "PATCH", url: `/documents/${initial.id}`, headers: auth(), payload: { expectedRevision: 1, markdownContent: "first update", isHidden: false } });
  assert.equal(firstUpdate.statusCode, 200); assert.equal(firstUpdate.json().document.revision, 2);
  const stale = await f.server.inject({ method: "PATCH", url: `/documents/${initial.id}`, headers: auth(), payload: { expectedRevision: 1, markdownContent: "stale overwrite" } });
  assert.equal(stale.statusCode, 409); assert.equal(stale.json().error, "document_conflict"); assert.doesNotMatch(stale.body, /stale overwrite/);
  const afterConflict = await f.server.inject({ method: "GET", url: `/documents?tagId=${tagId}`, headers: auth() });
  assert.equal(afterConflict.json().documents[0].markdownContent, "first update");
  assert.equal((await f.server.inject({ method: "DELETE", url: `/documents/${initial.id}`, headers: auth() })).statusCode, 200);
  const trash = await f.server.inject({ method: "GET", url: "/documents?deleted=true", headers: auth() });
  assert.equal(trash.json().documents.length, 1);
  assert.equal((await f.server.inject({ method: "POST", url: `/documents/${initial.id}/restore`, headers: auth() })).statusCode, 200);
  const renamedTag = await f.server.inject({ method: "PATCH", url: `/document-tags/${tagId}`, headers: auth(), payload: { name: "核心" } });
  assert.equal(renamedTag.statusCode, 200);
  assert.equal((await f.server.inject({ method: "DELETE", url: `/document-tags/${tagId}`, headers: auth() })).statusCode, 200);
  assert.equal((await f.server.inject({ method: "GET", url: "/document-tags", headers: auth() })).json().tags.length, 0);
});

test("uploaded_file cannot be restored after its stored content was removed", async (t) => {
  const f = await fixture(); t.after(f.close);
  const boundary = "----documents-restore";
  const payload = Buffer.from(`--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="gone.pdf"\r\nContent-Type: application/pdf\r\n\r\n%PDF harmless\r\n--${boundary}--\r\n`);
  const upload = await f.server.inject({ method: "POST", url: "/files", headers: { ...auth(), "content-type": `multipart/form-data; boundary=${boundary}` }, payload });
  const fileId = upload.json().file.id;
  const documentId = [...f.documentRepository.records.values()].find((record) => record.public.file?.id === fileId)!.public.id;
  assert.equal((await f.server.inject({ method: "DELETE", url: `/documents/${documentId}`, headers: auth() })).statusCode, 200);
  const restored = await f.server.inject({ method: "POST", url: `/documents/${documentId}/restore`, headers: auth() });
  assert.equal(restored.statusCode, 409); assert.equal(restored.json().error, "file_content_unavailable");
});
