import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test, { afterEach } from "node:test";

import { changeDocumentAccount } from "../documents/documentSession.ts";
import { pickAndUploadLearningMaterial } from "./accountLearningUpload.ts";

(globalThis as unknown as Record<string, unknown>).window = globalThis;

const { clearMocks, mockIPC } = await import("@tauri-apps/api/mocks");

const FOLDER_KIND = "learning_assistant_upload";

const uploadedRaw = () => ({
  status: "uploaded",
  file: {
    id: "6e3e21d9-0bc7-4f3b-bc88-8d989ea17f74",
    originalName: "learning-token-path-material.pdf",
    mimeType: "application/pdf",
    sizeBytes: 128,
    sha256: "a".repeat(64),
    createdAt: "2026-07-25T00:00:00.000Z",
    ownerUserId: "must-not-enter-react",
    storageKey: "must-not-enter-react",
    path: "C:\\secret\\learning-material.pdf",
    token: "secret-token",
  },
  documentId: "3a41639c-4172-4811-99c8-ec3887b50e89",
  folderId: "d0342b91-1ca8-4665-8c29-6f77c1831aef",
  folderKind: FOLDER_KIND,
  ownerUserId: "must-not-enter-react",
  storageKey: "must-not-enter-react",
  path: "C:\\secret\\learning-material.pdf",
  token: "secret-token",
});

function asErrorRecord(error: unknown): Record<string, unknown> {
  assert.equal(typeof error, "object");
  assert.notEqual(error, null);
  return error as Record<string, unknown>;
}

async function assertInvalidResponse(action: () => Promise<unknown>): Promise<void> {
  await assert.rejects(action(), (error) => {
    const safeError = asErrorRecord(error);
    assert.equal(safeError.code, "invalidResponse");
    assert.match(String(safeError.message), /上传结果无法确认/);
    assert.match(String(safeError.message), /刷新“助学模块上传”目录/);
    assert.doesNotMatch(String(safeError.message), /secret-token|C:\\secret|storageKey|ownerUserId/);
    return true;
  });
}

function mockUploadResponse(response: unknown): Array<{ command: string; payload: unknown }> {
  const calls: Array<{ command: string; payload: unknown }> = [];
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    return response;
  });
  return calls;
}

afterEach(() => {
  clearMocks();
  changeDocumentAccount(null);
});

test("pickAndUploadLearningMaterial invokes the fixed command without business arguments", async () => {
  const calls = mockUploadResponse(uploadedRaw());

  const result = await pickAndUploadLearningMaterial();

  assert.deepEqual(calls, [
    { command: "account_pick_and_upload_learning_material", payload: {} },
  ]);
  assert.equal(result.status, "uploaded");
  assert.equal(result.documentId, uploadedRaw().documentId);
});

test("uploaded results are runtime-validated and rebuilt from safe whitelist fields", async () => {
  mockUploadResponse(uploadedRaw());

  const result = await pickAndUploadLearningMaterial();

  assert.equal(result.status, "uploaded");
  assert.equal(result.folderKind, FOLDER_KIND);
  assert.deepEqual(Object.keys(result.file).sort(), [
    "createdAt",
    "id",
    "mimeType",
    "originalName",
    "sha256",
    "sizeBytes",
  ]);
  assert.equal(result.file.originalName, "learning-token-path-material.pdf");
  assert.equal("ownerUserId" in result, false);
  assert.equal("storageKey" in result, false);
  assert.equal("path" in result.file, false);
  assert.equal("token" in result.file, false);
});

test("cancelled and accountChanged return without success payloads", async () => {
  mockUploadResponse({ status: "cancelled" });
  assert.deepEqual(await pickAndUploadLearningMaterial(), { status: "cancelled" });

  clearMocks();
  mockUploadResponse({ status: "accountChanged" });
  assert.deepEqual(await pickAndUploadLearningMaterial(), { status: "accountChanged" });

  clearMocks();
  mockUploadResponse({ status: "cancelled", documentId: "fake" });
  await assertInvalidResponse(pickAndUploadLearningMaterial);
});

test("uploadedAccountChanged keeps successful upload identifiers", async () => {
  mockUploadResponse({ ...uploadedRaw(), status: "uploadedAccountChanged" });

  const result = await pickAndUploadLearningMaterial();

  assert.equal(result.status, "uploadedAccountChanged");
  assert.equal(result.documentId, uploadedRaw().documentId);
  assert.equal(result.folderId, uploadedRaw().folderId);
  assert.equal(result.folderKind, FOLDER_KIND);
});

test("uploaded stays associable when the document generation is unchanged", async () => {
  changeDocumentAccount("platform-user-a");
  mockUploadResponse(uploadedRaw());

  const result = await pickAndUploadLearningMaterial();

  assert.equal(result.status, "uploaded");
});

test("uploaded converts to uploadedAccountChanged when React generation changes after invoke", async () => {
  changeDocumentAccount("platform-user-a");
  mockIPC((command, payload) => {
    assert.equal(command, "account_pick_and_upload_learning_material");
    assert.deepEqual(payload, {});
    changeDocumentAccount("platform-user-b");
    return uploadedRaw();
  });

  const result = await pickAndUploadLearningMaterial();

  assert.equal(result.status, "uploadedAccountChanged");
  assert.equal(result.documentId, uploadedRaw().documentId);
  assert.equal(result.folderId, uploadedRaw().folderId);
  assert.equal(result.folderKind, FOLDER_KIND);
});

test("critical malformed responses are rejected with safe invalidResponse", async () => {
  const cases = [
    { ...uploadedRaw(), documentId: undefined },
    { ...uploadedRaw(), folderId: undefined },
    { ...uploadedRaw(), folderKind: "other" },
    { ...uploadedRaw(), file: undefined },
    { ...uploadedRaw(), file: { ...uploadedRaw().file, sizeBytes: -1 } },
    { ...uploadedRaw(), file: { ...uploadedRaw().file, originalName: "" } },
    { ...uploadedRaw(), status: "mystery" },
  ];

  for (const value of cases) {
    clearMocks();
    mockUploadResponse(value);
    await assertInvalidResponse(pickAndUploadLearningMaterial);
  }
});

test("invalid responses are not retried and do not leak raw sensitive fields", async () => {
  let calls = 0;
  mockIPC(() => {
    calls += 1;
    return {
      ...uploadedRaw(),
      documentId: undefined,
      token: "secret-token",
      path: "C:\\secret\\learning-material.pdf",
    };
  });

  await assertInvalidResponse(pickAndUploadLearningMaterial);
  assert.equal(calls, 1);
});

test("learning upload API has no UI, storage, Zustand, or ordinary upload side effects", async () => {
  const source = await readFile(new URL("./accountLearningUpload.ts", import.meta.url), "utf8");
  assert.doesNotMatch(source, /export\s+(?:const\s+LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND|function\s+(?:parseLearningMaterialUploadResult|createLearningMaterialUploadApi|applyLearningUploadRequestGuard))/);
  assert.doesNotMatch(source, /message\.(?:error|success|warning)/);
  assert.doesNotMatch(source, /\blocalStorage\b|\bsessionStorage\b|\bindexedDB\b/);
  assert.doesNotMatch(source, /\bzustand\b|use[A-Z]\w*Store/);
  assert.doesNotMatch(source, /\bfetch\b|\baxios\b|\bsetTimeout\b/);
  assert.doesNotMatch(source, /projectId|expectedRevision|ownerId|folderId:\s*["']/);

  const apiSource = await readFile(new URL("../api/index.ts", import.meta.url), "utf8");
  assert.match(apiSource, /account_pick_and_upload_file/);
  assert.doesNotMatch(apiSource, /account_pick_and_upload_learning_material/);
});

test("Tauri errors propagate without being stringified into unsafe UI text", async () => {
  const error = { code: "unavailable", message: "账号服务暂不可用" };
  mockIPC(() => Promise.reject(error));

  await assert.rejects(pickAndUploadLearningMaterial(), (caught) => {
    assert.equal(caught, error);
    return true;
  });
});
