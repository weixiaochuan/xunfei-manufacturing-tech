import assert from "node:assert/strict";
import test from "node:test";
import type { Pool } from "pg";
import type { AccountServerConfig } from "../src/config.js";
import {
  createLearningProjectService,
  type JsonArray,
  type JsonObject,
  type LearningProjectCreateRecord,
  type LearningProjectPatchRecord,
  type LearningProjectRecord,
  type LearningProjectRepository,
} from "../src/learning-projects.js";
import type { OidcClient } from "../src/oidc.js";
import { buildServer } from "../src/server.js";
import type { SessionService, SessionUser } from "../src/sessions.js";

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

class MemoryLearningProjectRepository implements LearningProjectRepository {
  readonly records = new Map<string, LearningProjectRecord>();
  private clock = Date.parse("2026-07-25T00:00:00.000Z");

  private now(): string {
    this.clock += 1_000;
    return new Date(this.clock).toISOString();
  }

  async create(ownerUserId: string, input: LearningProjectCreateRecord) {
    const now = this.now();
    const record: LearningProjectRecord = {
      id: input.id,
      ownerUserId,
      name: input.name,
      learningType: input.learningType,
      courseName: input.courseName,
      goalSummary: input.goalSummary,
      learningGoal: structuredClone(input.learningGoal),
      understanding: structuredClone(input.understanding),
      currentPlan: structuredClone(input.currentPlan),
      progress: structuredClone(input.progress),
      planAdjustments: structuredClone(input.planAdjustments),
      dataSchemaVersion: input.dataSchemaVersion,
      revision: 1,
      lastOpenedAt: null,
      createdAt: now,
      updatedAt: now,
      deletedAt: null,
    };
    this.records.set(record.id, record);
    return structuredClone(record);
  }

  async list(ownerUserId: string, options: { sort: "updated" | "recent"; limit: number; offset: number }) {
    const timestamp = (record: LearningProjectRecord) =>
      options.sort === "recent" ? record.lastOpenedAt ?? record.updatedAt : record.updatedAt;
    return [...this.records.values()]
      .filter((record) => record.ownerUserId === ownerUserId && record.deletedAt === null)
      .sort((left, right) => timestamp(right).localeCompare(timestamp(left)) || right.id.localeCompare(left.id))
      .slice(options.offset, options.offset + options.limit)
      .map((record) => structuredClone(record));
  }

  async findActive(ownerUserId: string, projectId: string) {
    const record = this.records.get(projectId);
    return record && record.ownerUserId === ownerUserId && record.deletedAt === null
      ? structuredClone(record)
      : null;
  }

  async update(ownerUserId: string, projectId: string, expectedRevision: number, patch: LearningProjectPatchRecord) {
    const record = this.records.get(projectId);
    if (!record || record.ownerUserId !== ownerUserId || record.deletedAt !== null) return { status: "not_found" as const };
    if (record.revision !== expectedRevision) return { status: "conflict" as const };
    if (patch.name !== undefined) record.name = patch.name;
    if (patch.learningType !== undefined) record.learningType = patch.learningType;
    if (patch.courseName !== undefined) record.courseName = patch.courseName;
    if (patch.goalSummary !== undefined) record.goalSummary = patch.goalSummary;
    if (patch.learningGoal !== undefined) record.learningGoal = structuredClone(patch.learningGoal);
    if (patch.understanding !== undefined) record.understanding = structuredClone(patch.understanding);
    if (patch.currentPlan !== undefined) record.currentPlan = structuredClone(patch.currentPlan);
    if (patch.progress !== undefined) record.progress = structuredClone(patch.progress);
    if (patch.planAdjustments !== undefined) record.planAdjustments = structuredClone(patch.planAdjustments);
    record.revision += 1;
    record.updatedAt = this.now();
    return { status: "updated" as const, record: structuredClone(record) };
  }

  async open(ownerUserId: string, projectId: string) {
    const record = this.records.get(projectId);
    if (!record || record.ownerUserId !== ownerUserId || record.deletedAt !== null) return null;
    record.lastOpenedAt = this.now();
    return structuredClone(record);
  }

  async delete(ownerUserId: string, projectId: string, expectedRevision: number) {
    const record = this.records.get(projectId);
    if (!record || record.ownerUserId !== ownerUserId || record.deletedAt !== null) return { status: "not_found" as const };
    if (record.revision !== expectedRevision) return { status: "conflict" as const };
    record.revision += 1;
    record.updatedAt = this.now();
    record.deletedAt = record.updatedAt;
    return { status: "deleted" as const };
  }
}

function sessions(): SessionService {
  return {
    create: async (user) => ({ token: TOKEN_A, user }),
    findActive: async (token) => token === TOKEN_A ? USER_A : token === TOKEN_B ? USER_B : null,
    revoke: async () => undefined,
  };
}

function config(): AccountServerConfig {
  return {
    deploymentProfile: "local",
    server: { host: "127.0.0.1", port: 3010, publicUrl: "http://127.0.0.1:3010" },
    database: { host: "127.0.0.1", port: 5432, database: "test", user: "test", password: "test", connectionTimeoutMillis: 5000 },
    oidc: { baseUrl: "http://127.0.0.1:8000", clientId: "test", clientSecret: "test", redirectUri: "http://127.0.0.1:3010/auth/callback", organization: "pomegranate", application: "app-pomegranate" },
    session: { ttlSeconds: 60 },
    userFiles: { backend: "filesystem", root: "unused", maxBytes: 1024 },
    nodeEnv: "test",
  };
}

const UNUSED_POOL = { query: async () => ({ rows: [{ value: 1 }], rowCount: 1 }) } as unknown as Pool;
const UNUSED_OIDC = {} as OidcClient;

function fixture() {
  const repository = new MemoryLearningProjectRepository();
  const server = buildServer({
    pool: UNUSED_POOL,
    config: config(),
    oidcClient: UNUSED_OIDC,
    sessionService: sessions(),
    learningProjectService: createLearningProjectService(repository),
    logger: false,
  });
  return { repository, server, close: async () => { await server.close(); } };
}

function auth(token = TOKEN_A) {
  return { authorization: `Bearer ${token}` };
}

async function createProject(
  server: ReturnType<typeof buildServer>,
  token = TOKEN_A,
  payload: Record<string, unknown> = {},
) {
  const response = await server.inject({
    method: "POST",
    url: "/learning/projects",
    headers: auth(token),
    payload: {
      name: "机械制造学习",
      learningType: "systematic",
      courseName: "机械制造工艺学",
      goalSummary: "三周完成系统学习",
      learningGoal: { target: "通过考试" },
      understanding: { gap: "基础薄弱" },
      currentPlan: { stages: [{ title: "基础" }] },
      progress: { currentStageIndex: 0 },
      planAdjustments: [],
      ...payload,
    },
  });
  assert.equal(response.statusCode, 201, response.body);
  return response.json().project;
}

test("learning project endpoints require authentication", async (t) => {
  const f = fixture(); t.after(f.close);
  for (const request of [
    { method: "GET", url: "/learning/projects" },
    { method: "POST", url: "/learning/projects", payload: { name: "x" } },
    { method: "GET", url: "/learning/projects/11111111-1111-4111-8111-111111111111" },
    { method: "PATCH", url: "/learning/projects/11111111-1111-4111-8111-111111111111", payload: { expectedRevision: 1, name: "x" } },
    { method: "PATCH", url: "/learning/projects/11111111-1111-4111-8111-111111111111/name", payload: { expectedRevision: 1, name: "x" } },
    { method: "POST", url: "/learning/projects/11111111-1111-4111-8111-111111111111/open" },
    { method: "DELETE", url: "/learning/projects/11111111-1111-4111-8111-111111111111", payload: { expectedRevision: 1 } },
    { method: "POST", url: "/learning/projects/11111111-1111-4111-8111-111111111111/duplicate", payload: { name: "copy" } },
  ] as const) {
    const response = await f.server.inject(request);
    assert.equal(response.statusCode, 401, `${request.method} ${request.url}`);
    assert.equal(response.json().error, "invalid_session");
  }
});

test("creation uses session owner, rejects forged owners, and allows duplicate names", async (t) => {
  const f = fixture(); t.after(f.close);
  const forged = await f.server.inject({
    method: "POST",
    url: "/learning/projects",
    headers: auth(),
    payload: { name: "x", ownerId: USER_B.platformUserId },
  });
  assert.equal(forged.statusCode, 400);
  assert.equal(forged.json().error, "unsafe_learning_project_payload");

  const first = await createProject(f.server);
  const second = await createProject(f.server);
  assert.notEqual(first.id, second.id);
  assert.equal(first.revision, 1);
  assert.equal(first.dataSchemaVersion, 1);
  assert.equal(f.repository.records.get(first.id)?.ownerUserId, USER_A.platformUserId);
  assert.doesNotMatch(JSON.stringify(first), /ownerUserId|platformUserId|token|apiKey/i);

  const list = await f.server.inject({ method: "GET", url: "/learning/projects", headers: auth() });
  assert.equal(list.statusCode, 200);
  assert.equal(list.json().projects.length, 2);
});

test("lists are owner scoped and summaries omit large JSON payloads", async (t) => {
  const f = fixture(); t.after(f.close);
  await createProject(f.server, TOKEN_A, { currentPlan: { stages: Array.from({ length: 20 }, (_, index) => ({ title: `阶段${index}` })) } });
  await createProject(f.server, TOKEN_B, { name: "Bob project" });

  const listA = await f.server.inject({ method: "GET", url: "/learning/projects", headers: auth(TOKEN_A) });
  assert.equal(listA.statusCode, 200);
  assert.equal(listA.json().projects.length, 1);
  assert.equal(listA.json().projects[0].name, "机械制造学习");
  assert.equal(listA.json().projects[0].currentPlan, undefined);
  assert.equal(listA.json().projects[0].learningGoal, undefined);

  const listB = await f.server.inject({ method: "GET", url: "/learning/projects", headers: auth(TOKEN_B) });
  assert.equal(listB.json().projects.length, 1);
  assert.equal(listB.json().projects[0].name, "Bob project");
});

test("cross-account get, update, rename, delete, and duplicate return not found", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = await createProject(f.server, TOKEN_A);
  for (const request of [
    { method: "GET", url: `/learning/projects/${project.id}` },
    { method: "PATCH", url: `/learning/projects/${project.id}`, payload: { expectedRevision: 1, progress: { stolen: true } } },
    { method: "PATCH", url: `/learning/projects/${project.id}/name`, payload: { expectedRevision: 1, name: "stolen" } },
    { method: "DELETE", url: `/learning/projects/${project.id}`, payload: { expectedRevision: 1 } },
    { method: "POST", url: `/learning/projects/${project.id}/duplicate`, payload: { name: "stolen" } },
  ] as const) {
    const response = await f.server.inject({ ...request, headers: auth(TOKEN_B) });
    assert.equal(response.statusCode, 404, `${request.method} ${request.url}`);
    assert.equal(response.json().error, "learning_project_not_found");
  }
});

test("revision controls updates, renames, opens, and deletes", async (t) => {
  const f = fixture(); t.after(f.close);
  const created = await createProject(f.server);

  const update = await f.server.inject({
    method: "PATCH",
    url: `/learning/projects/${created.id}`,
    headers: auth(),
    payload: { expectedRevision: 1, progress: { currentStageIndex: 1 } },
  });
  assert.equal(update.statusCode, 200);
  assert.equal(update.json().project.revision, 2);
  assert.equal(update.json().project.progress.currentStageIndex, 1);

  const stale = await f.server.inject({
    method: "PATCH",
    url: `/learning/projects/${created.id}`,
    headers: auth(),
    payload: { expectedRevision: 1, progress: { currentStageIndex: 99 } },
  });
  assert.equal(stale.statusCode, 409);
  assert.equal(stale.json().error, "learning_project_conflict");
  assert.doesNotMatch(stale.body, /99/);

  const renamed = await f.server.inject({
    method: "PATCH",
    url: `/learning/projects/${created.id}/name`,
    headers: auth(),
    payload: { expectedRevision: 2, name: "新项目名" },
  });
  assert.equal(renamed.statusCode, 200);
  assert.equal(renamed.json().project.revision, 3);

  const opened = await f.server.inject({ method: "POST", url: `/learning/projects/${created.id}/open`, headers: auth() });
  assert.equal(opened.statusCode, 200);
  assert.equal(opened.json().project.revision, 3);
  assert.match(opened.json().project.lastOpenedAt, /^\d{4}-\d{2}-\d{2}T/);

  const staleDelete = await f.server.inject({
    method: "DELETE",
    url: `/learning/projects/${created.id}`,
    headers: auth(),
    payload: { expectedRevision: 2 },
  });
  assert.equal(staleDelete.statusCode, 409);

  const deleted = await f.server.inject({
    method: "DELETE",
    url: `/learning/projects/${created.id}`,
    headers: auth(),
    payload: { expectedRevision: 3 },
  });
  assert.equal(deleted.statusCode, 200);
  assert.equal((await f.server.inject({ method: "GET", url: "/learning/projects", headers: auth() })).json().projects.length, 0);
  assert.equal((await f.server.inject({ method: "GET", url: `/learning/projects/${created.id}`, headers: auth() })).statusCode, 404);
});

test("concurrent updates with the same revision allow only one writer", async (t) => {
  const f = fixture(); t.after(f.close);
  const created = await createProject(f.server);

  const [left, right] = await Promise.all([
    f.server.inject({
      method: "PATCH",
      url: `/learning/projects/${created.id}`,
      headers: auth(),
      payload: { expectedRevision: 1, progress: { writer: "left" } },
    }),
    f.server.inject({
      method: "PATCH",
      url: `/learning/projects/${created.id}`,
      headers: auth(),
      payload: { expectedRevision: 1, progress: { writer: "right" } },
    }),
  ]);

  assert.deepEqual([left.statusCode, right.statusCode].sort(), [200, 409]);
  const conflict = left.statusCode === 409 ? left : right;
  assert.equal(conflict.json().error, "learning_project_conflict");

  const detail = await f.server.inject({ method: "GET", url: `/learning/projects/${created.id}`, headers: auth() });
  assert.equal(detail.statusCode, 200);
  assert.equal(detail.json().project.revision, 2);
  assert.match(detail.json().project.progress.writer, /^(left|right)$/);
});

test("duplicate creates an independent revision-1 copy without mutating the source", async (t) => {
  const f = fixture(); t.after(f.close);
  const source = await createProject(f.server, TOKEN_A, { progress: { score: 80 }, planAdjustments: [{ reason: "mock" }] });
  const duplicate = await f.server.inject({
    method: "POST",
    url: `/learning/projects/${source.id}/duplicate`,
    headers: auth(),
    payload: {},
  });
  assert.equal(duplicate.statusCode, 201);
  const copy = duplicate.json().project;
  assert.notEqual(copy.id, source.id);
  assert.equal(copy.name, "机械制造学习 副本");
  assert.equal(copy.revision, 1);
  assert.deepEqual(copy.progress, { score: 80 });
  assert.deepEqual(copy.planAdjustments, [{ reason: "mock" }]);

  const original = await f.server.inject({ method: "GET", url: `/learning/projects/${source.id}`, headers: auth() });
  assert.equal(original.json().project.revision, 1);
  assert.equal(original.json().project.name, source.name);
});

test("field validation rejects unsafe payloads, invalid JSON shapes, and large JSON", async (t) => {
  const f = fixture(); t.after(f.close);
  const invalidPayloads: Array<{ payload: Record<string, unknown>; error: string }> = [
    { payload: { name: "   " }, error: "invalid_learning_project_name" },
    { payload: { name: "x", learningGoal: [] }, error: "invalid_learning_goal" },
    { payload: { name: "x", planAdjustments: {} }, error: "invalid_plan_adjustments" },
    { payload: { name: "x", dataSchemaVersion: 0 }, error: "invalid_data_schema_version" },
    { payload: { name: "x", currentPlan: { sourcePath: "D:\\ag\\private.md" } }, error: "unsafe_learning_project_payload" },
    { payload: { name: "x", currentPlan: { path: "C:\\Users\\Alice\\private.md" } }, error: "unsafe_learning_project_payload" },
    { payload: { name: "x", progress: { apiKey: "secret" } }, error: "unsafe_learning_project_payload" },
    { payload: { name: "x", learningGoal: { text: "x".repeat(520 * 1024) } }, error: "learning_project_json_too_large" },
  ];
  for (const item of invalidPayloads) {
    const response = await f.server.inject({ method: "POST", url: "/learning/projects", headers: auth(), payload: item.payload });
    assert.equal(response.statusCode, 400, JSON.stringify(item.payload).slice(0, 80));
    assert.equal(response.json().error, item.error);
  }

  const created = await createProject(f.server);
  const invalidRevision = await f.server.inject({
    method: "PATCH",
    url: `/learning/projects/${created.id}`,
    headers: auth(),
    payload: { expectedRevision: "1", progress: {} },
  });
  assert.equal(invalidRevision.statusCode, 400);
  assert.equal(invalidRevision.json().error, "invalid_expected_revision");
});

test("learning text can mention security words and path examples without false positives", async (t) => {
  const f = fixture(); t.after(f.close);
  const ordinaryText = [
    "This is course text, not a secret.",
    "It mentions token, API, password, authorization, and Bearer as concepts.",
    "It also shows a Windows path example: C:\\Users\\Student\\notes.md.",
    "It may mention a Unix path example: /home/student/notes.md.",
  ].join(" ");

  const project = await createProject(f.server, TOKEN_A, {
    learningGoal: { text: ordinaryText },
    currentPlan: { notes: ordinaryText },
  });

  assert.equal(project.learningGoal.text, ordinaryText);
  assert.equal(project.currentPlan.notes, ordinaryText);
});

test("field and global body size limits return distinct errors", async (t) => {
  const f = fixture(); t.after(f.close);
  const fieldTooLarge = "中".repeat(180_000);
  assert(Buffer.byteLength(JSON.stringify({ text: fieldTooLarge }), "utf8") > 512 * 1024);
  assert(Buffer.byteLength(JSON.stringify({ name: "x", learningGoal: { text: fieldTooLarge } }), "utf8") < 1024 * 1024);

  const fieldResponse = await f.server.inject({
    method: "POST",
    url: "/learning/projects",
    headers: auth(),
    payload: { name: "x", learningGoal: { text: fieldTooLarge } },
  });
  assert.equal(fieldResponse.statusCode, 400);
  assert.equal(fieldResponse.json().error, "learning_project_json_too_large");

  const globalTooLarge = JSON.stringify({
    name: "x",
    learningGoal: { text: "中".repeat(360_000) },
  });
  assert(Buffer.byteLength(globalTooLarge, "utf8") > 1024 * 1024);
  const globalResponse = await f.server.inject({
    method: "POST",
    url: "/learning/projects",
    headers: { ...auth(), "content-type": "application/json" },
    payload: globalTooLarge,
  });
  assert.equal(globalResponse.statusCode, 413);
});

test("recent sorting is online and owner scoped", async (t) => {
  const f = fixture(); t.after(f.close);
  const first = await createProject(f.server, TOKEN_A, { name: "first" });
  const second = await createProject(f.server, TOKEN_A, { name: "second" });
  const other = await createProject(f.server, TOKEN_B, { name: "other" });

  await f.server.inject({ method: "POST", url: `/learning/projects/${first.id}/open`, headers: auth(TOKEN_A) });
  await f.server.inject({ method: "POST", url: `/learning/projects/${other.id}/open`, headers: auth(TOKEN_B) });

  const recentA = await f.server.inject({ method: "GET", url: "/learning/projects?sort=recent", headers: auth(TOKEN_A) });
  assert.equal(recentA.statusCode, 200);
  assert.equal(recentA.json().projects[0].id, first.id);
  assert.equal(recentA.json().projects.some((item: { id: string }) => item.id === other.id), false);

  const updatedA = await f.server.inject({ method: "GET", url: "/learning/projects?sort=updated", headers: auth(TOKEN_A) });
  assert.equal(updatedA.json().projects.some((item: { id: string }) => item.id === second.id), true);
});
