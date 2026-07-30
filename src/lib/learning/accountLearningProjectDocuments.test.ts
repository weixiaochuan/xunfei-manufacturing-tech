import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test, { afterEach } from "node:test";

import { changeDocumentAccount } from "../documents/documentSession.ts";
import {
  addAccountLearningProjectDocument,
  listAccountLearningProjectDocuments,
  removeAccountLearningProjectDocument,
  reorderAccountLearningProjectDocuments,
  updateAccountLearningProjectDocument,
} from "./accountLearningProjectDocuments.ts";

(globalThis as unknown as Record<string, unknown>).window = globalThis;

const { clearMocks, mockIPC } = await import("@tauri-apps/api/mocks");

const PROJECT_ID = "4e87b0d3-2c0f-4878-9d49-75f2c5b89973";
const DOCUMENT_ID = "72f1a6a8-788e-4058-93ed-8c617783b828";
const SECOND_DOCUMENT_ID = "e830cc29-e68c-4fe6-843c-5b65d4125807";
const MAX_SORT_ORDER = 2_147_483_647;

function documentRaw(overrides: Record<string, unknown> = {}) {
  return {
    documentId: DOCUMENT_ID,
    title: "Learning material",
    documentType: "uploaded_file",
    role: "material",
    importance: "normal",
    sortOrder: 0,
    createdAt: "2026-07-25T00:00:00.000Z",
    updatedAt: "2026-07-25T00:00:01.000Z",
    deletedAt: null,
    status: "available",
    ownerUserId: "must-not-enter-react",
    storageKey: "internal-storage",
    path: "C:\\secret\\material.pdf",
    token: "secret-token",
    ...overrides,
  };
}

function completedEnvelope(data: unknown) {
  return {
    status: "completed",
    data,
    ownerUserId: "must-not-enter-react",
  };
}

function mockResponses(
  responses: unknown[],
): Array<{ command: string; payload: unknown }> {
  const calls: Array<{ command: string; payload: unknown }> = [];
  mockIPC((command, payload) => {
    calls.push({ command, payload });
    return responses.shift();
  });
  return calls;
}

async function assertInvalidResponse(action: () => Promise<unknown>): Promise<void> {
  await assert.rejects(action(), (error) => {
    const safeError = asErrorRecord(error);
    assert.equal(safeError.code, "invalidResponse");
    assert.doesNotMatch(String(safeError.message), /secret-token|C:\\secret|storageKey|ownerUserId/);
    return true;
  });
}

function asErrorRecord(error: unknown): Record<string, unknown> {
  assert.equal(typeof error, "object");
  assert.notEqual(error, null);
  return error as Record<string, unknown>;
}

afterEach(() => {
  clearMocks();
  changeDocumentAccount(null);
});

test("project document APIs call the fixed Tauri commands with whitelisted payloads", async () => {
  const calls = mockResponses([
    completedEnvelope({
      projectRevision: 3,
      documents: [
        documentRaw(),
        documentRaw({
          documentId: SECOND_DOCUMENT_ID,
          sortOrder: 1,
          deletedAt: "2026-07-25T00:01:00.000Z",
          status: "deleted",
        }),
      ],
    }),
    completedEnvelope({ projectRevision: 4, document: documentRaw() }),
    completedEnvelope({
      projectRevision: 5,
      document: documentRaw({ role: "reference", importance: "core", sortOrder: MAX_SORT_ORDER }),
    }),
    completedEnvelope({ projectRevision: 6 }),
    completedEnvelope({ projectRevision: 7 }),
  ]);

  await listAccountLearningProjectDocuments(PROJECT_ID);
  await addAccountLearningProjectDocument({
    projectId: PROJECT_ID,
    expectedRevision: 3,
    documentId: DOCUMENT_ID,
    role: "material",
    importance: "important",
    sortOrder: MAX_SORT_ORDER,
  });
  await updateAccountLearningProjectDocument({
    projectId: PROJECT_ID,
    documentId: DOCUMENT_ID,
    expectedRevision: 4,
    role: "reference",
    importance: "core",
    sortOrder: MAX_SORT_ORDER,
  });
  await removeAccountLearningProjectDocument({
    projectId: PROJECT_ID,
    documentId: DOCUMENT_ID,
    expectedRevision: 5,
  });
  await reorderAccountLearningProjectDocuments({
    projectId: PROJECT_ID,
    expectedRevision: 6,
    documentIds: [SECOND_DOCUMENT_ID, DOCUMENT_ID],
  });

  assert.deepEqual(calls.map((call) => call.command), [
    "account_learning_project_documents_list",
    "account_learning_project_document_add",
    "account_learning_project_document_update",
    "account_learning_project_document_remove",
    "account_learning_project_documents_reorder",
  ]);
  assert.deepEqual(calls[1]?.payload, {
    input: {
      projectId: PROJECT_ID,
      expectedRevision: 3,
      documentId: DOCUMENT_ID,
      role: "material",
      importance: "important",
      sortOrder: MAX_SORT_ORDER,
    },
  });
  assert.deepEqual(calls[4]?.payload, {
    input: {
      projectId: PROJECT_ID,
      expectedRevision: 6,
      documentIds: [SECOND_DOCUMENT_ID, DOCUMENT_ID],
    },
  });
});

test("project document responses keep available and deleted states without leaking internals", async () => {
  mockResponses([
    completedEnvelope({
      projectRevision: 3,
      documents: [
        documentRaw(),
        documentRaw({
          documentId: SECOND_DOCUMENT_ID,
          deletedAt: "2026-07-25T00:01:00.000Z",
          status: "deleted",
        }),
      ],
    }),
  ]);

  const result = await listAccountLearningProjectDocuments(PROJECT_ID);

  assert.equal(result.status, "completed");
  assert.equal(result.data.projectRevision, 3);
  assert.equal(result.data.documents[0]?.status, "available");
  assert.equal(result.data.documents[1]?.status, "deleted");
  assert.deepEqual(Object.keys(result.data.documents[0] ?? {}).sort(), [
    "createdAt",
    "deletedAt",
    "documentId",
    "documentType",
    "importance",
    "role",
    "sortOrder",
    "status",
    "title",
    "updatedAt",
  ]);
  assert.equal("ownerUserId" in (result.data.documents[0] ?? {}), false);
  assert.equal("storageKey" in (result.data.documents[0] ?? {}), false);
  assert.equal("path" in (result.data.documents[0] ?? {}), false);
});

test("project document envelopes preserve accountChanged and convert stale completed results", async () => {
  mockResponses([{ status: "accountChanged" }]);
  assert.deepEqual(await listAccountLearningProjectDocuments(PROJECT_ID), {
    status: "accountChanged",
  });

  clearMocks();
  changeDocumentAccount("platform-user-a");
  mockIPC(() => {
    changeDocumentAccount("platform-user-b");
    return completedEnvelope({ projectRevision: 8 });
  });

  const result = await reorderAccountLearningProjectDocuments({
    projectId: PROJECT_ID,
    expectedRevision: 7,
    documentIds: [DOCUMENT_ID],
  });

  assert.equal(result.status, "completedAccountChanged");
  assert.equal(result.data.projectRevision, 8);
});

test("project document parsers reject malformed data without leaking raw fields or retrying", async () => {
  const cases = [
    completedEnvelope({ projectRevision: 1, documents: [documentRaw({ role: "bad" })] }),
    completedEnvelope({ projectRevision: 1, documents: [documentRaw({ importance: "bad" })] }),
    completedEnvelope({ projectRevision: 1, documents: [documentRaw({ sortOrder: MAX_SORT_ORDER + 1 })] }),
    completedEnvelope({ projectRevision: 1, documents: [documentRaw({ status: "gone" })] }),
    { status: "completed" },
  ];

  for (const value of cases) {
    clearMocks();
    let calls = 0;
    mockIPC(() => {
      calls += 1;
      return value;
    });
    await assertInvalidResponse(() => listAccountLearningProjectDocuments(PROJECT_ID));
    assert.equal(calls, 1);
  }
});

test("project document input validation rejects unsafe IDs and sort orders before invoke", async () => {
  const calls = mockResponses([completedEnvelope({ projectRevision: 1 })]);

  await assert.rejects(
    () => listAccountLearningProjectDocuments("../not-a-uuid"),
    { code: "validation" },
  );
  await assert.rejects(
    () =>
      addAccountLearningProjectDocument({
        projectId: PROJECT_ID,
        expectedRevision: 1,
        documentId: DOCUMENT_ID,
        sortOrder: MAX_SORT_ORDER + 1,
      }),
    { code: "validation" },
  );
  await assert.rejects(
    () =>
      updateAccountLearningProjectDocument({
        projectId: PROJECT_ID,
        documentId: DOCUMENT_ID,
        expectedRevision: 1,
      }),
    { code: "validation" },
  );
  await assert.rejects(
    () =>
      reorderAccountLearningProjectDocuments({
        projectId: PROJECT_ID,
        expectedRevision: 1,
        documentIds: [DOCUMENT_ID, DOCUMENT_ID],
      }),
    { code: "validation" },
  );
  assert.equal(calls.length, 0);
});

test("Tauri project document errors propagate safely and are not retried", async () => {
  const exists = {
    code: "learningProjectDocumentExists",
    message: "The document is already linked.",
  };
  let calls = 0;
  mockIPC(() => {
    calls += 1;
    return Promise.reject(exists);
  });

  await assert.rejects(
    () =>
      addAccountLearningProjectDocument({
        projectId: PROJECT_ID,
        expectedRevision: 1,
        documentId: DOCUMENT_ID,
      }),
    (error) => {
      assert.equal(error, exists);
      return true;
    },
  );
  assert.equal(calls, 1);
});

test("project document APIs do not contain UI, storage, network, or proxy side effects", async () => {
  const source = await readFile(
    new URL("./accountLearningProjectDocuments.ts", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(source, /message\.(?:error|success|warning)/);
  assert.doesNotMatch(source, /\blocalStorage\b|\bsessionStorage\b|\bindexedDB\b/);
  assert.doesNotMatch(source, /\bzustand\b|use[A-Z]\w*Store/);
  assert.doesNotMatch(source, /\bfetch\b|\baxios\b|\bsetTimeout\b/);
  assert.doesNotMatch(source, /ownerId|ownerUserId|Authorization|authorization|accountServerUrl/);
  assert.doesNotMatch(source, /invokeLearning|method\s*,\s*path|rawUrl|url:/);
});
