import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test, { afterEach } from "node:test";

import { changeDocumentAccount } from "../documents/documentSession.ts";
import {
  createAccountLearningProject,
  deleteAccountLearningProject,
  duplicateAccountLearningProject,
  getAccountLearningProject,
  listAccountLearningProjects,
  openAccountLearningProject,
  renameAccountLearningProject,
  updateAccountLearningProject,
} from "./accountLearningProjects.ts";

(globalThis as unknown as Record<string, unknown>).window = globalThis;

const { clearMocks, mockIPC } = await import("@tauri-apps/api/mocks");

const PROJECT_ID = "4e87b0d3-2c0f-4878-9d49-75f2c5b89973";
const OTHER_PROJECT_ID = "6b8d13e5-6577-4f0f-9411-7f3021c61716";
const MAX_SAFE_REVISION = 9_007_199_254_740_991;

function projectSummaryRaw() {
  return {
    id: PROJECT_ID,
    name: "Process planning",
    learningType: "course",
    courseName: "Manufacturing",
    goalSummary: "Read token/password/path as normal course words",
    revision: 3,
    lastOpenedAt: null,
    createdAt: "2026-07-25T00:00:00.000Z",
    updatedAt: "2026-07-25T00:00:01.000Z",
    ownerUserId: "must-not-enter-react",
    token: "secret-token",
    storageKey: "internal-storage",
    path: "C:\\secret\\project.json",
  };
}

function projectDetailRaw(overrides: Record<string, unknown> = {}) {
  return {
    ...projectSummaryRaw(),
    learningGoal: {
      text: "Show Windows path examples and password/token words safely.",
    },
    understanding: {},
    currentPlan: {},
    progress: {},
    planAdjustments: [],
    dataSchemaVersion: 1,
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
    const response = responses.shift();
    if (response instanceof Error) {
      throw response;
    }
    return response;
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

test("project CRUD APIs call the fixed Tauri commands with whitelisted payloads", async () => {
  const calls = mockResponses([
    completedEnvelope({ projects: [projectSummaryRaw()], limit: 20, offset: 2 }),
    completedEnvelope(projectDetailRaw()),
    completedEnvelope(projectDetailRaw()),
    completedEnvelope(projectDetailRaw()),
    completedEnvelope(projectDetailRaw()),
    completedEnvelope(projectDetailRaw()),
    completedEnvelope({ projectId: PROJECT_ID }),
    completedEnvelope(projectDetailRaw({ id: OTHER_PROJECT_ID, revision: 1 })),
  ]);

  await listAccountLearningProjects({ sort: "recent", limit: 20, offset: 2 });
  await createAccountLearningProject({
    name: "Process planning",
    learningGoal: { text: "token and C:\\example are course text" },
    planAdjustments: [],
  });
  await getAccountLearningProject(PROJECT_ID);
  await updateAccountLearningProject({
    projectId: PROJECT_ID,
    expectedRevision: 3,
    learningType: null,
    courseName: "Manufacturing",
    goalSummary: undefined,
    progress: { done: true },
  });
  await renameAccountLearningProject({
    projectId: PROJECT_ID,
    expectedRevision: 4,
    name: "Renamed",
  });
  await openAccountLearningProject(PROJECT_ID);
  await deleteAccountLearningProject({
    projectId: PROJECT_ID,
    expectedRevision: 5,
  });
  await duplicateAccountLearningProject({
    projectId: PROJECT_ID,
    name: "Process planning copy",
  });

  assert.deepEqual(calls.map((call) => call.command), [
    "account_learning_projects_list",
    "account_learning_project_create",
    "account_learning_project_get",
    "account_learning_project_update",
    "account_learning_project_rename",
    "account_learning_project_open",
    "account_learning_project_delete",
    "account_learning_project_duplicate",
  ]);
  assert.deepEqual(calls[0]?.payload, {
    input: { sort: "recent", limit: 20, offset: 2 },
  });
  assert.deepEqual(calls[1]?.payload, {
    input: {
      name: "Process planning",
      learningGoal: { text: "token and C:\\example are course text" },
      planAdjustments: [],
    },
  });
  assert.deepEqual(calls[3]?.payload, {
    input: {
      projectId: PROJECT_ID,
      expectedRevision: 3,
      learningType: null,
      courseName: "Manufacturing",
      progress: { done: true },
    },
  });
  assert.deepEqual(calls[6]?.payload, {
    input: { projectId: PROJECT_ID, expectedRevision: 5 },
  });
  assert.deepEqual(calls[7]?.payload, {
    input: { projectId: PROJECT_ID, name: "Process planning copy" },
  });
});

test("project responses are rebuilt from safe whitelist fields", async () => {
  mockResponses([completedEnvelope(projectDetailRaw())]);

  const result = await getAccountLearningProject(PROJECT_ID);

  assert.equal(result.status, "completed");
  assert.deepEqual(Object.keys(result.data).sort(), [
    "courseName",
    "createdAt",
    "currentPlan",
    "dataSchemaVersion",
    "goalSummary",
    "id",
    "lastOpenedAt",
    "learningGoal",
    "learningType",
    "name",
    "planAdjustments",
    "progress",
    "revision",
    "understanding",
    "updatedAt",
  ]);
  assert.equal("ownerUserId" in result.data, false);
  assert.equal("storageKey" in result.data, false);
  assert.equal("path" in result.data, false);
  assert.equal(result.data.goalSummary?.includes("token"), true);
});

test("project envelopes preserve accountChanged and convert stale completed results", async () => {
  mockResponses([{ status: "accountChanged" }]);
  assert.deepEqual(await listAccountLearningProjects(), { status: "accountChanged" });

  clearMocks();
  changeDocumentAccount("platform-user-a");
  mockIPC(() => {
    changeDocumentAccount("platform-user-b");
    return completedEnvelope(projectDetailRaw());
  });

  const result = await getAccountLearningProject(PROJECT_ID);

  assert.equal(result.status, "completedAccountChanged");
  assert.equal(result.data.id, PROJECT_ID);
});

test("project parsers reject malformed envelopes without leaking raw data or retrying", async () => {
  const cases = [
    { status: "completed" },
    completedEnvelope(projectDetailRaw({ revision: MAX_SAFE_REVISION + 1 })),
    completedEnvelope(projectDetailRaw({ learningGoal: [] })),
    completedEnvelope(projectDetailRaw({ planAdjustments: {} })),
    { status: "unknown", data: projectDetailRaw() },
  ];

  for (const value of cases) {
    clearMocks();
    let calls = 0;
    mockIPC(() => {
      calls += 1;
      return value;
    });
    await assertInvalidResponse(() => getAccountLearningProject(PROJECT_ID));
    assert.equal(calls, 1);
  }
});

test("project input validation blocks path injection and unsafe numeric values before invoke", async () => {
  const calls = mockResponses([completedEnvelope(projectDetailRaw())]);

  await assert.rejects(
    () => getAccountLearningProject("../not-a-uuid"),
    { code: "validation" },
  );
  await assert.rejects(
    () => listAccountLearningProjects({ sort: "owner" as never }),
    { code: "validation" },
  );
  await assert.rejects(
    () =>
      updateAccountLearningProject({
        projectId: PROJECT_ID,
        expectedRevision: MAX_SAFE_REVISION + 1,
      }),
    { code: "validation" },
  );
  assert.equal(calls.length, 0);
});

test("Tauri project errors propagate as safe objects without stringify wrapping", async () => {
  const conflict = {
    code: "learningProjectConflict",
    message: "Refresh the project before saving.",
  };
  mockIPC(() => Promise.reject(conflict));

  await assert.rejects(
    () => updateAccountLearningProject({ projectId: PROJECT_ID, expectedRevision: 1 }),
    (error) => {
      assert.equal(error, conflict);
      return true;
    },
  );
});

test("project APIs do not contain UI, storage, network, or account-server proxy side effects", async () => {
  const source = await readFile(
    new URL("./accountLearningProjects.ts", import.meta.url),
    "utf8",
  );
  assert.doesNotMatch(source, /message\.(?:error|success|warning)/);
  assert.doesNotMatch(source, /\blocalStorage\b|\bsessionStorage\b|\bindexedDB\b/);
  assert.doesNotMatch(source, /\bzustand\b|use[A-Z]\w*Store/);
  assert.doesNotMatch(source, /\bfetch\b|\baxios\b|\bsetTimeout\b/);
  assert.doesNotMatch(source, /ownerId|ownerUserId|Authorization|authorization|accountServerUrl/);
  assert.doesNotMatch(source, /invokeLearning|method\s*,\s*path|rawUrl|url:/);
});
