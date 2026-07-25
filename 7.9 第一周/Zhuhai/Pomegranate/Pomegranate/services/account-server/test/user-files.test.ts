import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import type { Pool } from "pg";
import type { AccountServerConfig } from "../src/config.js";
import type { OidcClient } from "../src/oidc.js";
import { buildServer } from "../src/server.js";
import type { DocumentService, PublicDocument } from "../src/documents.js";
import {
  LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND,
  LEARNING_ASSISTANT_UPLOAD_FOLDER_NAME,
  type DocumentLibraryService,
  type PublicDocumentFolder,
} from "../src/document-library.js";
import type { SessionService, SessionUser } from "../src/sessions.js";
import { LocalFilesystemStorage } from "../src/storage/local-filesystem-storage.js";
import {
  createUserFileService,
  sanitizeOriginalName,
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
  accountNumber: "POME-000002",
  username: "bob",
  displayName: "Bob",
  email: null,
};

class MemoryRepository implements UserFileRepository {
  readonly records = new Map<string, StoredUserFile & { ownerUserId: string; deleted: boolean }>();
  failCreate = false;

  async create(record: StoredUserFile & { ownerUserId: string }) {
    if (this.failCreate) throw new Error("INSERT secret SQL path=C:\\private token=hidden");
    this.records.set(record.id, { ...record, deleted: false });
  }

  async list(ownerUserId: string, limit: number, offset: number) {
    return [...this.records.values()]
      .filter((record) => record.ownerUserId === ownerUserId && !record.deleted)
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt) || b.id.localeCompare(a.id))
      .slice(offset, offset + limit);
  }

  async findOwnedActive(fileId: string, ownerUserId: string) {
    const record = this.records.get(fileId);
    return record && record.ownerUserId === ownerUserId && !record.deleted ? record : null;
  }

  async softDeleteOwned(fileId: string, ownerUserId: string) {
    const record = this.records.get(fileId);
    if (!record || record.ownerUserId !== ownerUserId || record.deleted) return null;
    record.deleted = true;
    return record;
  }

  async hardDeleteOwned(fileId: string, ownerUserId: string) {
    const record = this.records.get(fileId);
    if (!record || record.ownerUserId !== ownerUserId) return null;
    this.records.delete(fileId);
    return record;
  }

  async replaceOwnedContent(fileId: string, ownerUserId: string, expectedSha256: string, replacement: { storageKey: string; mimeType: string | null; sizeBytes: number; sha256: string }) {
    const record = await this.findOwnedActive(fileId, ownerUserId);
    if (!record) return { status: "not_found" as const };
    if (record.sha256 !== expectedSha256) return { status: "conflict" as const };
    const oldStorageKey = record.storageKey;
    Object.assign(record, replacement);
    return { status: "updated" as const, ...record, oldStorageKey, documentId: `00000000-0000-4000-8000-${fileId.replaceAll("-", "").slice(0, 12)}`, revision: 2, updatedAt: new Date().toISOString() };
  }
}

function config(root: string, maxBytes = 1_024): AccountServerConfig {
  return {
    deploymentProfile: "local",
    server: { host: "127.0.0.1", port: 3010, publicUrl: "http://127.0.0.1:3010" },
    database: {
      host: "127.0.0.1", port: 5432, database: "test", user: "test",
      password: "test-password", connectionTimeoutMillis: 5_000,
    },
    oidc: {
      baseUrl: "http://127.0.0.1:8000", clientId: "test-client",
      clientSecret: "test-secret", redirectUri: "http://127.0.0.1:3010/auth/callback",
      organization: "pomegranate", application: "app-pomegranate",
    },
    session: { ttlSeconds: 60 },
    userFiles: { backend: "filesystem", root, maxBytes },
    nodeEnv: "test",
  };
}

function sessions(): SessionService {
  return {
    create: async (user) => ({ token: TOKEN_A, user }),
    findActive: async (token) => token === TOKEN_A ? USER_A : token === TOKEN_B ? USER_B : null,
    revoke: async () => undefined,
  };
}

const UNUSED_POOL = { query: async () => ({ rows: [{ value: 1 }], rowCount: 1 }) } as unknown as Pool;
const UNUSED_OIDC = {} as OidcClient;

function multipart(filename: string, content: Buffer, field = "file") {
  const boundary = "----pomegranate-test-boundary";
  const head = Buffer.from(
    `--${boundary}\r\nContent-Disposition: form-data; name="${field}"; filename="${filename}"\r\n` +
      "Content-Type: text/plain\r\n\r\n",
  );
  const tail = Buffer.from(`\r\n--${boundary}--\r\n`);
  return {
    payload: Buffer.concat([head, content, tail]),
    headers: { "content-type": `multipart/form-data; boundary=${boundary}` },
  };
}

function multipartTwoFiles() {
  const boundary = "----pomegranate-two-files";
  const part = (name: string, content: string) =>
    `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${name}"\r\n` +
    `Content-Type: text/plain\r\n\r\n${content}\r\n`;
  return {
    payload: Buffer.from(`${part("one.txt", "one")}${part("two.txt", "two")}--${boundary}--\r\n`),
    headers: { "content-type": `multipart/form-data; boundary=${boundary}` },
  };
}

type TestMultipartPart =
  | { type: "field"; name: string; value: string }
  | { type: "file"; name: string; filename: string; content: Buffer; contentType?: string };

function multipartParts(parts: TestMultipartPart[], boundary = `----pomegranate-${randomUUID()}`) {
  const buffers: Buffer[] = [];
  for (const part of parts) {
    if (part.type === "field") {
      buffers.push(Buffer.from(
        `--${boundary}\r\nContent-Disposition: form-data; name="${part.name}"\r\n\r\n${part.value}\r\n`,
      ));
      continue;
    }
    buffers.push(Buffer.from(
      `--${boundary}\r\nContent-Disposition: form-data; name="${part.name}"; filename="${part.filename}"\r\n` +
        `Content-Type: ${part.contentType ?? "text/plain"}\r\n\r\n`,
    ));
    buffers.push(part.content);
    buffers.push(Buffer.from("\r\n"));
  }
  buffers.push(Buffer.from(`--${boundary}--\r\n`));
  return { payload: Buffer.concat(buffers), headers: { "content-type": `multipart/form-data; boundary=${boundary}` } };
}

function multipartWithFolderKind(filename: string, content: Buffer, order: "folder-first" | "file-first" = "folder-first") {
  const file = { type: "file" as const, name: "file", filename, content };
  const folder = { type: "field" as const, name: "folderKind", value: LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND };
  return multipartParts(order === "folder-first" ? [folder, file] : [file, folder]);
}

function replacementMultipart(filename: string, content: Buffer, expectedSha256: string) {
  const boundary = "----pomegranate-replacement";
  const payload = Buffer.concat([
    Buffer.from(`--${boundary}\r\nContent-Disposition: form-data; name="expectedSha256"\r\n\r\n${expectedSha256}\r\n`),
    Buffer.from(`--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${filename}"\r\nContent-Type: text/plain\r\n\r\n`),
    content,
    Buffer.from(`\r\n--${boundary}--\r\n`),
  ]);
  return { payload, headers: { "content-type": `multipart/form-data; boundary=${boundary}` } };
}

interface MemoryFolder extends PublicDocumentFolder {
  ownerUserId: string;
  deleted: boolean;
}

function createMemoryDocumentLibrary() {
  const folders = new Map<string, MemoryFolder>();
  let deleteNextResolvedLearningFolder = false;
  const toPublic = (folder: MemoryFolder): PublicDocumentFolder => ({
    id: folder.id,
    name: folder.name,
    parentId: folder.parentId,
    folderKind: folder.folderKind,
    createdAt: folder.createdAt,
    updatedAt: folder.updatedAt,
  });
  const createFolder = (
    ownerUserId: string,
    name: string,
    folderKind: PublicDocumentFolder["folderKind"],
  ) => {
    const now = new Date().toISOString();
    const folder: MemoryFolder = {
      id: randomUUID(),
      name,
      parentId: null,
      folderKind,
      createdAt: now,
      updatedAt: now,
      ownerUserId,
      deleted: false,
    };
    folders.set(folder.id, folder);
    return folder;
  };
  const service = {
    async getOrCreateLearningAssistantUploadFolder(ownerUserId: string) {
      const existing = [...folders.values()].find((folder) =>
        folder.ownerUserId === ownerUserId &&
        folder.folderKind === LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND &&
        !folder.deleted);
      const folder = existing ??
        createFolder(ownerUserId, LEARNING_ASSISTANT_UPLOAD_FOLDER_NAME, LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND);
      const result = toPublic(folder);
      if (deleteNextResolvedLearningFolder) {
        folder.deleted = true;
        folder.updatedAt = new Date().toISOString();
        deleteNextResolvedLearningFolder = false;
      }
      return result;
    },
  } as DocumentLibraryService;
  return {
    service,
    folders,
    createOrdinarySameNameFolder(ownerUserId: string) {
      return createFolder(ownerUserId, LEARNING_ASSISTANT_UPLOAD_FOLDER_NAME, null);
    },
    softDeleteFolder(folderId: string) {
      const folder = folders.get(folderId);
      if (folder) {
        folder.deleted = true;
        folder.updatedAt = new Date().toISOString();
      }
    },
    deleteNextLearningFolderAfterResolve() {
      deleteNextResolvedLearningFolder = true;
    },
    activeLearningFolders(ownerUserId: string) {
      return [...folders.values()].filter((folder) =>
        folder.ownerUserId === ownerUserId &&
        folder.folderKind === LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND &&
        !folder.deleted);
    },
  };
}

async function fixture(maxBytes = 1_024, failDocumentCreate = false) {
  const root = await mkdtemp(join(tmpdir(), "pomegranate-files-test-"));
  const repository = new MemoryRepository();
  const storage = new LocalFilesystemStorage(root);
  await storage.initialize();
  const service = createUserFileService(repository, storage, maxBytes);
  const linkedFiles = new Map<string, PublicDocument>();
  const documentLibrary = createMemoryDocumentLibrary();
  const documents: DocumentService = {
    list: async () => [...linkedFiles.values()],
    createMarkdown: async () => { throw new Error("unused"); },
    async createUploadedFile(ownerUserId, file, options) {
      if (failDocumentCreate) throw new Error("document INSERT failed with private details");
      if (options?.folderId) {
        const folder = documentLibrary.folders.get(options.folderId);
        if (!folder || folder.ownerUserId !== ownerUserId || folder.deleted) {
          throw new Error("invalid_uploaded_file_folder");
        }
      }
      const document: PublicDocument = {
        id: `00000000-0000-4000-8000-${file.id.replaceAll("-", "").slice(0, 12)}`,
        kind: "uploaded_file",
        title: file.originalName,
        markdownContent: null,
        file,
        folderId: options?.folderId ?? null,
        sourceLocalDocumentId: null,
        createdAt: file.createdAt,
        updatedAt: file.createdAt,
        deletedAt: null,
      };
      linkedFiles.set(file.id, document);
      return document;
    },
    updateMarkdown: async () => null,
    deleteOwned: async () => null,
    async deleteByFileId(fileId, ownerUserId) {
      const document = linkedFiles.get(fileId);
      if (!document) return null;
      const deleted = await service.delete(fileId, ownerUserId);
      if (!deleted) return null;
      linkedFiles.delete(fileId);
      return { documentId: document.id, fileId, storageKey: null };
    },
    upsertLocalMarkdown: async () => { throw new Error("unused"); },
    importLocalMarkdown: async () => ({ imported: 0, updated: 0, skipped: 0, failed: 0, outcomes: [] }),
  };
  const server = buildServer({
    pool: UNUSED_POOL,
    config: config(root, maxBytes),
    oidcClient: UNUSED_OIDC,
    sessionService: sessions(),
    userFileService: service,
    documentService: documents,
    documentLibraryService: documentLibrary.service,
    logger: false,
  });
  return {
    root,
    repository,
    service,
    linkedFiles,
    documentLibrary,
    server,
    close: async () => { await server.close(); await rm(root, { recursive: true, force: true }); },
  };
}

test("upload requires an active session, including unknown, expired, or revoked tokens", async (t) => {
  const f = await fixture(); t.after(f.close);
  for (const token of [undefined, "x".repeat(43), "e".repeat(43), "r".repeat(43)]) {
    const body = multipart("a.txt", Buffer.from("a"));
    const response = await f.server.inject({
      method: "POST", url: "/files", payload: body.payload,
      headers: { ...body.headers, ...(token ? { authorization: `Bearer ${token}` } : {}) },
    });
    assert.equal(response.statusCode, 401);
    assert.deepEqual(response.json(), { status: "error", error: "invalid_session" });
  }
});

test("content replacement keeps IDs, updates bytes, and rejects stale hashes and other owners", async (t) => {
  const f = await fixture(); t.after(f.close);
  const uploaded = multipart("editable.txt", Buffer.from("version one"));
  const created = await f.server.inject({ method: "POST", url: "/files", headers: { ...uploaded.headers, authorization: `Bearer ${TOKEN_A}` }, payload: uploaded.payload });
  assert.equal(created.statusCode, 201);
  const original = created.json().file as { id: string; sha256: string };

  const replacement = replacementMultipart("editable.txt", Buffer.from("version two"), original.sha256);
  const updated = await f.server.inject({ method: "PUT", url: `/files/${original.id}/content`, headers: { ...replacement.headers, authorization: `Bearer ${TOKEN_A}` }, payload: replacement.payload });
  assert.equal(updated.statusCode, 200);
  assert.equal(updated.json().file.id, original.id);
  assert.equal(updated.json().revision, 2);
  assert.notEqual(updated.json().file.sha256, original.sha256);
  const downloaded = await f.server.inject({ method: "GET", url: `/files/${original.id}/download`, headers: { authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(downloaded.body, "version two");

  const stale = replacementMultipart("editable.txt", Buffer.from("stale overwrite"), original.sha256);
  const conflict = await f.server.inject({ method: "PUT", url: `/files/${original.id}/content`, headers: { ...stale.headers, authorization: `Bearer ${TOKEN_A}` }, payload: stale.payload });
  assert.equal(conflict.statusCode, 409);
  assert.equal(conflict.json().error, "file_conflict");
  const foreign = await f.server.inject({ method: "PUT", url: `/files/${original.id}/content`, headers: { ...stale.headers, authorization: `Bearer ${TOKEN_B}` }, payload: stale.payload });
  assert.equal(foreign.statusCode, 404);
  const unchanged = await f.server.inject({ method: "GET", url: `/files/${original.id}/download`, headers: { authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(unchanged.body, "version two");
});

test("upload writes exact content and safe metadata with SHA-256", async (t) => {
  const f = await fixture(); t.after(f.close);
  const content = Buffer.from("harmless account file\n", "utf8");
  const body = multipart("notes.txt", content);
  const response = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(response.statusCode, 201);
  const result = response.json();
  assert.equal(result.file.originalName, "notes.txt");
  assert.equal(result.file.sizeBytes, content.length);
  assert.equal(result.file.sha256, createHash("sha256").update(content).digest("hex"));
  assert.equal(result.file.mimeType, "text/plain");
  assert.equal("storageKey" in result.file, false);
  assert.equal("ownerUserId" in result.file, false);
  assert.equal(typeof result.documentId, "string");
  assert.equal(result.folderId, null);
  assert.equal(result.folderKind, null);
  const linked = f.linkedFiles.get(result.file.id);
  assert.equal(linked?.id, result.documentId);
  assert.equal(linked?.folderId, null);
  const record = [...f.repository.records.values()][0];
  assert.ok(record);
  assert.deepEqual(await readFile(join(f.root, record.storageKey)), content);
});

test("folderKind uploads create uploaded_file documents in the current account learning folder", async (t) => {
  const f = await fixture(); t.after(f.close);

  const firstBody = multipartWithFolderKind("learning-a.txt", Buffer.from("A"), "folder-first");
  const first = await f.server.inject({
    method: "POST",
    url: "/files",
    payload: firstBody.payload,
    headers: { ...firstBody.headers, authorization: `Bearer ${TOKEN_A}` },
  });
  assert.equal(first.statusCode, 201);
  const firstResult = first.json();
  assert.equal(firstResult.folderKind, LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND);
  assert.equal(typeof firstResult.folderId, "string");
  assert.equal(f.linkedFiles.get(firstResult.file.id)?.folderId, firstResult.folderId);
  assert.equal(firstResult.documentId, f.linkedFiles.get(firstResult.file.id)?.id);
  assert.doesNotMatch(JSON.stringify(firstResult), new RegExp(USER_A.platformUserId));
  assert.doesNotMatch(JSON.stringify(firstResult), /ownerUserId|storageKey|path|token|authorization/i);

  const secondBody = multipartWithFolderKind("learning-b.txt", Buffer.from("B"), "file-first");
  const second = await f.server.inject({
    method: "POST",
    url: "/files",
    payload: secondBody.payload,
    headers: { ...secondBody.headers, authorization: `Bearer ${TOKEN_A}` },
  });
  assert.equal(second.statusCode, 201);
  assert.equal(second.json().folderId, firstResult.folderId);
  assert.equal(f.documentLibrary.activeLearningFolders(USER_A.platformUserId).length, 1);

  const otherAccount = await f.server.inject({
    method: "POST",
    url: "/files",
    payload: firstBody.payload,
    headers: { ...firstBody.headers, authorization: `Bearer ${TOKEN_B}` },
  });
  assert.equal(otherAccount.statusCode, 201);
  assert.notEqual(otherAccount.json().folderId, firstResult.folderId);
  assert.equal(f.documentLibrary.activeLearningFolders(USER_B.platformUserId).length, 1);
});

test("folderKind uploads ignore ordinary same-name folders and recreate deleted learning folders", async (t) => {
  const f = await fixture(); t.after(f.close);
  const ordinary = f.documentLibrary.createOrdinarySameNameFolder(USER_A.platformUserId);

  const firstBody = multipartWithFolderKind("first.txt", Buffer.from("first"));
  const first = await f.server.inject({
    method: "POST",
    url: "/files",
    payload: firstBody.payload,
    headers: { ...firstBody.headers, authorization: `Bearer ${TOKEN_A}` },
  });
  assert.equal(first.statusCode, 201);
  const firstFolderId = first.json().folderId;
  assert.notEqual(firstFolderId, ordinary.id);
  assert.equal(f.documentLibrary.activeLearningFolders(USER_A.platformUserId).length, 1);

  f.documentLibrary.softDeleteFolder(firstFolderId);
  const secondBody = multipartWithFolderKind("second.txt", Buffer.from("second"));
  const second = await f.server.inject({
    method: "POST",
    url: "/files",
    payload: secondBody.payload,
    headers: { ...secondBody.headers, authorization: `Bearer ${TOKEN_A}` },
  });
  assert.equal(second.statusCode, 201);
  assert.notEqual(second.json().folderId, firstFolderId);
  assert.notEqual(second.json().folderId, ordinary.id);
  assert.equal(f.documentLibrary.activeLearningFolders(USER_A.platformUserId).length, 1);
  assert.equal(f.linkedFiles.get(second.json().file.id)?.folderId, second.json().folderId);
});

test("invalid upload fields and folderKind injection are rejected before storage", async (t) => {
  const f = await fixture(); t.after(f.close);
  const file = { type: "file" as const, name: "file", filename: "safe.txt", content: Buffer.from("not stored") };
  const cases = [
    {
      body: multipartParts([{ type: "field", name: "folderKind", value: "input" }, file]),
      error: "invalid_folder_kind",
    },
    {
      body: multipartParts([{ type: "field", name: "folderKind", value: LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND }, { type: "field", name: "folderKind", value: LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND }, file]),
      error: "invalid_file_field",
    },
    {
      body: multipartParts([{ type: "field", name: "ownerId", value: USER_B.platformUserId }, file]),
      error: "invalid_file_field",
    },
    {
      body: multipartParts([{ type: "field", name: "ownerUserId", value: USER_B.platformUserId }, file]),
      error: "invalid_file_field",
    },
    {
      body: multipartParts([file, { type: "field", name: "folderId", value: randomUUID() }]),
      error: "invalid_file_field",
    },
    {
      body: multipartParts([{ type: "field", name: "path", value: "C:\\private\\notes.txt" }, file]),
      error: "invalid_file_field",
    },
    {
      body: multipartParts([{ type: "field", name: "sourcePath", value: "C:\\private\\notes.txt" }, file]),
      error: "invalid_file_field",
    },
    {
      body: multipartParts([{ type: "file", name: "folderKind", filename: "folderKind.txt", content: Buffer.from(LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND) }, file]),
      error: "invalid_file_field",
    },
  ];
  for (const item of cases) {
    const response = await f.server.inject({
      method: "POST",
      url: "/files",
      payload: item.body.payload,
      headers: { ...item.body.headers, authorization: `Bearer ${TOKEN_A}` },
    });
    assert.equal(response.statusCode, 400);
    assert.equal(response.json().error, item.error);
  }
  assert.equal(f.repository.records.size, 0);
  assert.equal(f.linkedFiles.size, 0);
  assert.equal(f.documentLibrary.folders.size, 0);
  assert.deepEqual(await readdir(f.root), []);
});

test("empty files are accepted with size zero and the correct digest", async (t) => {
  const f = await fixture(); t.after(f.close);
  const body = multipart("empty.txt", Buffer.alloc(0));
  const response = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(response.statusCode, 201);
  assert.equal(response.json().file.sizeBytes, 0);
  assert.equal(response.json().file.sha256, createHash("sha256").update(Buffer.alloc(0)).digest("hex"));
});

test("an uploaded Markdown file remains an uploaded_file with exact bytes and no Markdown body", async (t) => {
  const f = await fixture(); t.after(f.close);
  const content = Buffer.from("# uploaded, not imported\n", "utf8");
  const body = multipart("source.md", content);
  const response = await f.server.inject({
    method: "POST", url: "/files", payload: body.payload,
    headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` },
  });
  assert.equal(response.statusCode, 201);
  const fileId = response.json().file.id;
  const document = f.linkedFiles.get(fileId);
  assert.equal(document?.kind, "uploaded_file");
  assert.equal(document?.markdownContent, null);
  const record = f.repository.records.get(fileId);
  assert.ok(record);
  assert.deepEqual(await readFile(join(f.root, record.storageKey)), content);
});

test("the upload allowlist accepts every supported document, text, data, and image extension", async (t) => {
  const f = await fixture(); t.after(f.close);
  const extensions = [
    "doc", "docx", "xls", "xlsx", "csv", "ppt", "pptx", "pdf",
    "md", "markdown", "mdx", "mdxl", "txt", "rtf", "json", "xml", "yaml", "yml",
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg",
  ];
  for (const extension of extensions) {
    const body = multipart(`sample.${extension}`, Buffer.from(extension));
    const response = await f.server.inject({
      method: "POST", url: "/files", payload: body.payload,
      headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` },
    });
    assert.equal(response.statusCode, 201, extension);
  }
  assert.equal(f.repository.records.size, extensions.length);
});

test("dangerous, script, missing, and unknown extensions are rejected before storage", async (t) => {
  const f = await fixture(); t.after(f.close);
  const rejected = [
    "exe", "msi", "dll", "bat", "cmd", "com", "scr", "ps1", "vbs", "js", "jse",
    "jar", "reg", "lnk", "html", "unknown", "",
  ];
  for (const extension of rejected) {
    const filename = extension ? `sample.${extension}` : "sample";
    const body = multipart(filename, Buffer.from("not stored"));
    const response = await f.server.inject({
      method: "POST", url: "/files", payload: body.payload,
      headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` },
    });
    assert.equal(response.statusCode, 400, extension || "no extension");
    assert.deepEqual(response.json(), { status: "error", error: "file_type_rejected" });
  }
  assert.equal(f.repository.records.size, 0);
  assert.deepEqual(await readdir(f.root), []);
});

test("oversized uploads return 413 without metadata or orphaned files", async (t) => {
  const f = await fixture(4); t.after(f.close);
  const body = multipart("large.txt", Buffer.from("12345"));
  const response = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(response.statusCode, 413);
  assert.equal(f.repository.records.size, 0);
  assert.deepEqual(await readdir(f.root), []);
});

test("upload accepts exactly one file field named file", async (t) => {
  const f = await fixture(); t.after(f.close);
  const wrong = multipart("a.txt", Buffer.from("a"), "attachment");
  const two = multipartTwoFiles();
  for (const body of [wrong, two]) {
    const response = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
    assert.equal(response.statusCode, 400);
  }
  assert.equal(f.repository.records.size, 0);
  assert.deepEqual(await readdir(f.root), []);
});

test("path traversal names are display-only and cannot escape storage root", async (t) => {
  const f = await fixture(); t.after(f.close);
  assert.equal(sanitizeOriginalName("../../outside\r\nInjected.txt"), "outsideInjected.txt");
  const body = multipart("../../outside.txt", Buffer.from("safe"));
  const response = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(response.statusCode, 201);
  const record = [...f.repository.records.values()][0]; assert.ok(record);
  assert.match(record.storageKey, /^[a-f0-9]{64}$/);
  assert.deepEqual(await readdir(f.root), [record.storageKey]);
});

test("strict owner isolation applies to list, download, and delete", async (t) => {
  const f = await fixture(); t.after(f.close);
  const body = multipart("private.txt", Buffer.from("only A"));
  const upload = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
  const fileId = upload.json().file.id;
  const listA = await f.server.inject({ method: "GET", url: "/files", headers: { authorization: `Bearer ${TOKEN_A}` } });
  const listB = await f.server.inject({ method: "GET", url: "/files", headers: { authorization: `Bearer ${TOKEN_B}` } });
  assert.equal(listA.json().files.length, 1);
  assert.equal(listB.json().files.length, 0);
  for (const method of ["GET", "DELETE"] as const) {
    const suffix = method === "GET" ? "/download" : "";
    const response = await f.server.inject({ method, url: `/files/${fileId}${suffix}`, headers: { authorization: `Bearer ${TOKEN_B}` } });
    assert.equal(response.statusCode, 404);
  }
  const download = await f.server.inject({ method: "GET", url: `/files/${fileId}/download`, headers: { authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(download.statusCode, 200);
  assert.equal(download.body, "only A");
  assert.equal(download.headers["content-type"], "application/octet-stream");
});

test("soft delete hides the file, removes content, is repeatably 404, and does not affect B", async (t) => {
  const f = await fixture(); t.after(f.close);
  const a = await f.service.upload({ ownerUserId: USER_A.platformUserId, originalName: "same.txt", mimeType: "text/plain", content: [Buffer.from("A")] });
  const b = await f.service.upload({ ownerUserId: USER_B.platformUserId, originalName: "same.txt", mimeType: "text/plain", content: [Buffer.from("B")] });
  const first = await f.server.inject({ method: "DELETE", url: `/files/${a.id}`, headers: { authorization: `Bearer ${TOKEN_A}` } });
  const second = await f.server.inject({ method: "DELETE", url: `/files/${a.id}`, headers: { authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(first.statusCode, 200);
  assert.equal(second.statusCode, 404);
  assert.equal((await f.service.list(USER_A.platformUserId, 50, 0)).length, 0);
  assert.equal((await f.service.list(USER_B.platformUserId, 50, 0))[0]?.id, b.id);
  const bDownload = await f.service.download(b.id, USER_B.platformUserId);
  assert.ok(bDownload);
  bDownload.content.destroy();
});

test("concurrent same-name uploads use distinct storage keys and never overwrite", async (t) => {
  const f = await fixture(); t.after(f.close);
  const [first, second] = await Promise.all([
    f.service.upload({ ownerUserId: USER_A.platformUserId, originalName: "same.txt", mimeType: null, content: [Buffer.from("one")] }),
    f.service.upload({ ownerUserId: USER_A.platformUserId, originalName: "same.txt", mimeType: null, content: [Buffer.from("two")] }),
  ]);
  assert.notEqual(first.id, second.id);
  const keys = [...f.repository.records.values()].map((record) => record.storageKey);
  assert.equal(new Set(keys).size, 2);
  assert.deepEqual(new Set(await Promise.all(keys.map((key) => readFile(join(f.root, key), "utf8")))), new Set(["one", "two"]));
});

test("database failure cleans the completed file and returns no sensitive details", async (t) => {
  const f = await fixture(); t.after(f.close); f.repository.failCreate = true;
  const body = multipart("failure.txt", Buffer.from("data"));
  const response = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(response.statusCode, 503);
  assert.deepEqual(await readdir(f.root), []);
  assert.doesNotMatch(response.body, /INSERT|private|token|path|password|stack/i);
});

test("document catalog failure rolls back user_files metadata and disk content", async (t) => {
  const f = await fixture(1_024, true); t.after(f.close);
  const body = multipartWithFolderKind("rollback.txt", Buffer.from("temporary"));
  const response = await f.server.inject({ method: "POST", url: "/files", payload: body.payload, headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(response.statusCode, 503);
  assert.equal(f.repository.records.size, 0);
  assert.equal(f.documentLibrary.activeLearningFolders(USER_A.platformUserId).length, 1);
  assert.deepEqual(await readdir(f.root), []);
  assert.doesNotMatch(response.body, /INSERT|private|temporary|stack/i);
});

test("folder removal after folderKind resolution rolls back uploaded bytes instead of falling back to root", async (t) => {
  const f = await fixture(); t.after(f.close);
  f.documentLibrary.deleteNextLearningFolderAfterResolve();
  const body = multipartWithFolderKind("race.txt", Buffer.from("race"));
  const response = await f.server.inject({
    method: "POST",
    url: "/files",
    payload: body.payload,
    headers: { ...body.headers, authorization: `Bearer ${TOKEN_A}` },
  });
  assert.equal(response.statusCode, 503);
  assert.equal(f.repository.records.size, 0);
  assert.equal(f.linkedFiles.size, 0);
  assert.equal(f.documentLibrary.activeLearningFolders(USER_A.platformUserId).length, 0);
  assert.deepEqual(await readdir(f.root), []);
});

test("disk write failure creates no metadata record", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "pomegranate-files-fail-"));
  const invalidRoot = join(parent, "not-a-directory");
  await writeFile(invalidRoot, "blocking file");
  const repository = new MemoryRepository();
  const storage = new LocalFilesystemStorage(invalidRoot);
  await assert.rejects(() => storage.initialize());
  assert.equal(repository.records.size, 0);
  await rm(parent, { recursive: true, force: true });
});

test("pagination is bounded and invalid file ids reveal no ownership information", async (t) => {
  const f = await fixture(); t.after(f.close);
  const invalidPage = await f.server.inject({ method: "GET", url: "/files?limit=101", headers: { authorization: `Bearer ${TOKEN_A}` } });
  const invalidId = await f.server.inject({ method: "GET", url: "/files/not-a-uuid/download", headers: { authorization: `Bearer ${TOKEN_A}` } });
  assert.equal(invalidPage.statusCode, 400);
  assert.equal(invalidId.statusCode, 404);
  assert.deepEqual(invalidId.json(), { status: "error", error: "file_not_found" });
});
