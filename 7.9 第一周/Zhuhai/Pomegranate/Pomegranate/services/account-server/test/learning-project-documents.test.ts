import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import type { Pool } from "pg";
import type { AccountServerConfig } from "../src/config.js";
import {
  createLearningProjectDocumentService,
  type AddProjectDocumentRecord,
  type LearningProjectDocumentImportance,
  type LearningProjectDocumentRepository,
  type LearningProjectDocumentRole,
  type LearningProjectDocumentWriteResult,
  type PublicLearningProjectDocument,
  type ReorderProjectDocumentsRecord,
  type UpdateProjectDocumentRecord,
} from "../src/learning-project-documents.js";
import type {
  LearningProjectDeleteResult,
  LearningProjectService,
  LearningProjectUpdateResult,
  PublicLearningProject,
  PublicLearningProjectSummary,
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

interface ProjectState {
  id: string;
  ownerUserId: string;
  name: string;
  revision: number;
  deletedAt: string | null;
}

interface DocumentState {
  id: string;
  ownerUserId: string;
  title: string;
  documentType: string;
  deletedAt: string | null;
  storageKey: string;
  localPath: string;
}

interface RelationState {
  projectId: string;
  ownerUserId: string;
  documentId: string;
  role: LearningProjectDocumentRole;
  importance: LearningProjectDocumentImportance;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

class MemoryLearningProjectDocumentRepository implements LearningProjectDocumentRepository {
  readonly projects = new Map<string, ProjectState>();
  readonly documents = new Map<string, DocumentState>();
  readonly relations = new Map<string, RelationState>();
  private clock = Date.parse("2026-07-25T00:00:00.000Z");

  now(): string {
    this.clock += 1_000;
    return new Date(this.clock).toISOString();
  }

  createProject(ownerUserId = USER_A.platformUserId, name = "study project") {
    const project: ProjectState = {
      id: randomUUID(),
      ownerUserId,
      name,
      revision: 1,
      deletedAt: null,
    };
    this.projects.set(project.id, project);
    return project;
  }

  createDocument(ownerUserId = USER_A.platformUserId, title = "material") {
    const document: DocumentState = {
      id: randomUUID(),
      ownerUserId,
      title,
      documentType: "uploaded_file",
      deletedAt: null,
      storageKey: `private/${randomUUID()}`,
      localPath: `PRIVATE_LOCAL_PATH/${title}.pdf`,
    };
    this.documents.set(document.id, document);
    return document;
  }

  softDeleteDocument(documentId: string) {
    const document = this.documents.get(documentId);
    assert(document);
    document.deletedAt = this.now();
  }

  private key(projectId: string, documentId: string) {
    return `${projectId}:${documentId}`;
  }

  private activeProject(ownerUserId: string, projectId: string) {
    const project = this.projects.get(projectId);
    return project && project.ownerUserId === ownerUserId && project.deletedAt === null ? project : null;
  }

  private mapRelation(relation: RelationState): PublicLearningProjectDocument {
    const document = this.documents.get(relation.documentId);
    assert(document);
    return {
      documentId: relation.documentId,
      title: document.title,
      documentType: document.documentType,
      role: relation.role,
      importance: relation.importance,
      sortOrder: relation.sortOrder,
      createdAt: relation.createdAt,
      updatedAt: relation.updatedAt,
      deletedAt: document.deletedAt,
      status: document.deletedAt === null ? "available" : "deleted",
    };
  }

  private projectConflictOrNotFound(ownerUserId: string, projectId: string, expectedRevision: number) {
    const project = this.activeProject(ownerUserId, projectId);
    if (!project) return { status: "not_found" as const };
    if (project.revision !== expectedRevision) return { status: "conflict" as const };
    return { status: "ok" as const, project };
  }

  private bump(project: ProjectState) {
    project.revision += 1;
    return project.revision;
  }

  async list(ownerUserId: string, projectId: string) {
    const project = this.activeProject(ownerUserId, projectId);
    if (!project) return { status: "not_found" as const };
    const documents = [...this.relations.values()]
      .filter((relation) => relation.ownerUserId === ownerUserId && relation.projectId === projectId)
      .sort((left, right) => left.sortOrder - right.sortOrder || left.createdAt.localeCompare(right.createdAt))
      .map((relation) => this.mapRelation(relation));
    return { status: "ok" as const, projectRevision: project.revision, documents };
  }

  async add(ownerUserId: string, projectId: string, input: AddProjectDocumentRecord) {
    const projectStatus = this.projectConflictOrNotFound(ownerUserId, projectId, input.expectedRevision);
    if (projectStatus.status !== "ok") return projectStatus;
    const relationKey = this.key(projectId, input.documentId);
    if (this.relations.has(relationKey)) return { status: "exists" as const };
    const document = this.documents.get(input.documentId);
    if (!document || document.ownerUserId !== ownerUserId || document.deletedAt !== null) {
      return { status: "not_found" as const };
    }
    const sortOrder = input.sortOrder ?? [...this.relations.values()]
      .filter((relation) => relation.ownerUserId === ownerUserId && relation.projectId === projectId)
      .reduce((next, relation) => Math.max(next, relation.sortOrder + 1), 0);
    const now = this.now();
    this.relations.set(relationKey, {
      projectId,
      ownerUserId,
      documentId: input.documentId,
      role: input.role,
      importance: input.importance,
      sortOrder,
      createdAt: now,
      updatedAt: now,
    });
    const relation = this.relations.get(relationKey)!;
    return {
      status: "updated" as const,
      projectRevision: this.bump(projectStatus.project),
      document: this.mapRelation(relation),
    };
  }

  async update(ownerUserId: string, projectId: string, documentId: string, input: UpdateProjectDocumentRecord) {
    const projectStatus = this.projectConflictOrNotFound(ownerUserId, projectId, input.expectedRevision);
    if (projectStatus.status !== "ok") return projectStatus;
    const relation = this.relations.get(this.key(projectId, documentId));
    if (!relation || relation.ownerUserId !== ownerUserId) return { status: "not_found" as const };
    if (input.role !== undefined) relation.role = input.role;
    if (input.importance !== undefined) relation.importance = input.importance;
    if (input.sortOrder !== undefined) relation.sortOrder = input.sortOrder;
    relation.updatedAt = this.now();
    return {
      status: "updated" as const,
      projectRevision: this.bump(projectStatus.project),
      document: this.mapRelation(relation),
    };
  }

  async remove(ownerUserId: string, projectId: string, documentId: string, expectedRevision: number) {
    const projectStatus = this.projectConflictOrNotFound(ownerUserId, projectId, expectedRevision);
    if (projectStatus.status !== "ok") return projectStatus;
    const relationKey = this.key(projectId, documentId);
    const relation = this.relations.get(relationKey);
    if (!relation || relation.ownerUserId !== ownerUserId) return { status: "not_found" as const };
    this.relations.delete(relationKey);
    return { status: "updated" as const, projectRevision: this.bump(projectStatus.project) };
  }

  async reorder(ownerUserId: string, projectId: string, input: ReorderProjectDocumentsRecord): Promise<LearningProjectDocumentWriteResult> {
    const projectStatus = this.projectConflictOrNotFound(ownerUserId, projectId, input.expectedRevision);
    if (projectStatus.status !== "ok") return projectStatus;
    const current = [...this.relations.values()]
      .filter((relation) => relation.ownerUserId === ownerUserId && relation.projectId === projectId)
      .map((relation) => relation.documentId);
    if (current.length !== input.documentIds.length) return { status: "invalid_order" };
    const currentSet = new Set(current);
    if (input.documentIds.some((documentId) => !currentSet.has(documentId))) return { status: "invalid_order" };
    for (const [sortOrder, documentId] of input.documentIds.entries()) {
      const relation = this.relations.get(this.key(projectId, documentId));
      assert(relation);
      relation.sortOrder = sortOrder;
      relation.updatedAt = this.now();
    }
    return { status: "updated", projectRevision: this.bump(projectStatus.project) };
  }

  async copy(ownerUserId: string, sourceProjectId: string, targetProjectId: string) {
    let copied = 0;
    for (const relation of [...this.relations.values()]) {
      if (relation.ownerUserId !== ownerUserId || relation.projectId !== sourceProjectId) continue;
      const now = this.now();
      this.relations.set(this.key(targetProjectId, relation.documentId), {
        ...structuredClone(relation),
        projectId: targetProjectId,
        createdAt: now,
        updatedAt: now,
      });
      copied += 1;
    }
    return copied;
  }
}

class MemoryLearningProjectService implements LearningProjectService {
  constructor(private readonly repository: MemoryLearningProjectDocumentRepository) {}

  private project(projectState: ProjectState): PublicLearningProject {
    const timestamp = new Date("2026-07-25T00:00:00.000Z").toISOString();
    return {
      id: projectState.id,
      name: projectState.name,
      learningType: null,
      courseName: null,
      goalSummary: null,
      learningGoal: {},
      understanding: {},
      currentPlan: {},
      progress: {},
      planAdjustments: [],
      dataSchemaVersion: 1,
      revision: projectState.revision,
      lastOpenedAt: null,
      createdAt: timestamp,
      updatedAt: timestamp,
    };
  }

  async create(ownerUserId: string, input: { name?: unknown }) {
    return this.project(this.repository.createProject(ownerUserId, typeof input.name === "string" ? input.name : "study project"));
  }

  async list(): Promise<PublicLearningProjectSummary[]> {
    return [];
  }

  async get(ownerUserId: string, projectId: string) {
    const project = this.repository.projects.get(projectId);
    return project && project.ownerUserId === ownerUserId && project.deletedAt === null ? this.project(project) : null;
  }

  async update(): Promise<LearningProjectUpdateResult> {
    throw new Error("not used");
  }

  async rename(): Promise<LearningProjectUpdateResult> {
    throw new Error("not used");
  }

  async open(ownerUserId: string, projectId: string) {
    return this.get(ownerUserId, projectId);
  }

  async delete(): Promise<LearningProjectDeleteResult> {
    throw new Error("not used");
  }

  async duplicate(ownerUserId: string, projectId: string, input: { name?: unknown }) {
    const source = this.repository.projects.get(projectId);
    if (!source || source.ownerUserId !== ownerUserId || source.deletedAt !== null) return null;
    const copy = this.repository.createProject(ownerUserId, typeof input.name === "string" ? input.name : `${source.name} copy`);
    await this.repository.copy(ownerUserId, source.id, copy.id);
    return this.project(copy);
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
  const repository = new MemoryLearningProjectDocumentRepository();
  const server = buildServer({
    pool: UNUSED_POOL,
    config: config(),
    oidcClient: UNUSED_OIDC,
    sessionService: sessions(),
    learningProjectService: new MemoryLearningProjectService(repository),
    learningProjectDocumentService: createLearningProjectDocumentService(repository),
    logger: false,
  });
  return { repository, server, close: async () => { await server.close(); } };
}

function auth(token = TOKEN_A) {
  return { authorization: `Bearer ${token}` };
}

async function addDocument(
  server: ReturnType<typeof buildServer>,
  projectId: string,
  documentId: string,
  expectedRevision: number,
  payload: Record<string, unknown> = {},
) {
  const response = await server.inject({
    method: "POST",
    url: `/learning/projects/${projectId}/documents`,
    headers: auth(),
    payload: { expectedRevision, documentId, ...payload },
  });
  assert.equal(response.statusCode, 201, response.body);
  return response.json();
}

test("learning project document endpoints require authentication", async (t) => {
  const f = fixture(); t.after(f.close);
  const projectId = randomUUID();
  const documentId = randomUUID();
  for (const request of [
    { method: "GET", url: `/learning/projects/${projectId}/documents` },
    { method: "POST", url: `/learning/projects/${projectId}/documents`, payload: { expectedRevision: 1, documentId } },
    { method: "PATCH", url: `/learning/projects/${projectId}/documents/${documentId}`, payload: { expectedRevision: 1, role: "note" } },
    { method: "DELETE", url: `/learning/projects/${projectId}/documents/${documentId}`, payload: { expectedRevision: 1 } },
    { method: "PUT", url: `/learning/projects/${projectId}/documents/order`, payload: { expectedRevision: 1, documentIds: [] } },
  ] as const) {
    const response = await f.server.inject(request);
    assert.equal(response.statusCode, 401, `${request.method} ${request.url}`);
    assert.equal(response.json().error, "invalid_session");
  }
});

test("documents can be associated many-to-many without leaking internal file data", async (t) => {
  const f = fixture(); t.after(f.close);
  const firstProject = f.repository.createProject();
  const secondProject = f.repository.createProject();
  const firstDocument = f.repository.createDocument(USER_A.platformUserId, "syllabus.pdf");
  const secondDocument = f.repository.createDocument(USER_A.platformUserId, "notes.md");

  const first = await addDocument(f.server, firstProject.id, firstDocument.id, 1);
  assert.equal(first.projectRevision, 2);
  assert.equal(first.document.role, "material");
  assert.equal(first.document.importance, "normal");
  assert.equal(first.document.sortOrder, 0);
  assert.equal(first.document.status, "available");
  assert.doesNotMatch(JSON.stringify(first), /ownerUserId|storageKey|localPath|PRIVATE_LOCAL_PATH/i);

  const second = await addDocument(f.server, firstProject.id, secondDocument.id, 2, { role: "note", importance: "important" });
  assert.equal(second.projectRevision, 3);
  assert.equal(second.document.sortOrder, 1);

  const sameDocumentOtherProject = await addDocument(f.server, secondProject.id, firstDocument.id, 1);
  assert.equal(sameDocumentOtherProject.projectRevision, 2);

  const list = await f.server.inject({ method: "GET", url: `/learning/projects/${firstProject.id}/documents`, headers: auth() });
  assert.equal(list.statusCode, 200);
  assert.equal(list.json().projectRevision, 3);
  assert.deepEqual(list.json().documents.map((item: { documentId: string }) => item.documentId), [firstDocument.id, secondDocument.id]);
});

test("duplicate association and invalid documents fail without consuming revision", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject();
  const document = f.repository.createDocument();
  const deletedDocument = f.repository.createDocument();
  const otherOwnerDocument = f.repository.createDocument(USER_B.platformUserId);
  f.repository.softDeleteDocument(deletedDocument.id);

  await addDocument(f.server, project.id, document.id, 1);
  const duplicate = await f.server.inject({
    method: "POST",
    url: `/learning/projects/${project.id}/documents`,
    headers: auth(),
    payload: { expectedRevision: 2, documentId: document.id },
  });
  assert.equal(duplicate.statusCode, 409);
  assert.equal(duplicate.json().error, "learning_project_document_exists");
  assert.equal(f.repository.projects.get(project.id)?.revision, 2);

  for (const documentId of [deletedDocument.id, otherOwnerDocument.id]) {
    const response = await f.server.inject({
      method: "POST",
      url: `/learning/projects/${project.id}/documents`,
      headers: auth(),
      payload: { expectedRevision: 2, documentId },
    });
    assert.equal(response.statusCode, 404);
    assert.equal(response.json().error, "learning_project_document_not_found");
    assert.equal(f.repository.projects.get(project.id)?.revision, 2);
  }
});

test("cross-account project access returns not found", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject(USER_A.platformUserId);
  const document = f.repository.createDocument(USER_A.platformUserId);

  for (const request of [
    { method: "GET", url: `/learning/projects/${project.id}/documents` },
    { method: "POST", url: `/learning/projects/${project.id}/documents`, payload: { expectedRevision: 1, documentId: document.id } },
    { method: "PATCH", url: `/learning/projects/${project.id}/documents/${document.id}`, payload: { expectedRevision: 1, role: "note" } },
    { method: "DELETE", url: `/learning/projects/${project.id}/documents/${document.id}`, payload: { expectedRevision: 1 } },
  ] as const) {
    const response = await f.server.inject({ ...request, headers: auth(TOKEN_B) });
    assert.equal(response.statusCode, 404, `${request.method} ${request.url}`);
  }
});

test("soft-deleted documents remain listed as deleted after association", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject();
  const document = f.repository.createDocument(USER_A.platformUserId, "old.pdf");
  await addDocument(f.server, project.id, document.id, 1);
  f.repository.softDeleteDocument(document.id);

  const list = await f.server.inject({ method: "GET", url: `/learning/projects/${project.id}/documents`, headers: auth() });
  assert.equal(list.statusCode, 200);
  assert.equal(list.json().documents[0].status, "deleted");
  assert.match(list.json().documents[0].deletedAt, /^\d{4}-\d{2}-\d{2}T/);
});

test("metadata updates validate enums and increment revision", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject();
  const document = f.repository.createDocument();
  await addDocument(f.server, project.id, document.id, 1);

  const updated = await f.server.inject({
    method: "PATCH",
    url: `/learning/projects/${project.id}/documents/${document.id}`,
    headers: auth(),
    payload: { expectedRevision: 2, role: "exercise", importance: "core", sortOrder: 4 },
  });
  assert.equal(updated.statusCode, 200);
  assert.equal(updated.json().projectRevision, 3);
  assert.equal(updated.json().document.role, "exercise");
  assert.equal(updated.json().document.importance, "core");
  assert.equal(updated.json().document.sortOrder, 4);

  for (const payload of [
    { expectedRevision: 3, role: "bad" },
    { expectedRevision: 3, importance: "bad" },
    { expectedRevision: 3, sortOrder: -1 },
    { expectedRevision: 3 },
  ]) {
    const response = await f.server.inject({
      method: "PATCH",
      url: `/learning/projects/${project.id}/documents/${document.id}`,
      headers: auth(),
      payload,
    });
    assert.equal(response.statusCode, 400);
    assert.equal(f.repository.projects.get(project.id)?.revision, 3);
  }
});

test("complete reorder validates exact document set and increments revision once", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject();
  const first = f.repository.createDocument(USER_A.platformUserId, "first");
  const second = f.repository.createDocument(USER_A.platformUserId, "second");
  const extra = f.repository.createDocument(USER_A.platformUserId, "extra");
  await addDocument(f.server, project.id, first.id, 1);
  await addDocument(f.server, project.id, second.id, 2);

  const reordered = await f.server.inject({
    method: "PUT",
    url: `/learning/projects/${project.id}/documents/order`,
    headers: auth(),
    payload: { expectedRevision: 3, documentIds: [second.id, first.id] },
  });
  assert.equal(reordered.statusCode, 200);
  assert.equal(reordered.json().projectRevision, 4);

  const list = await f.server.inject({ method: "GET", url: `/learning/projects/${project.id}/documents`, headers: auth() });
  assert.deepEqual(list.json().documents.map((item: { documentId: string }) => item.documentId), [second.id, first.id]);

  for (const documentIds of [[first.id], [first.id, first.id], [second.id, first.id, extra.id]]) {
    const response = await f.server.inject({
      method: "PUT",
      url: `/learning/projects/${project.id}/documents/order`,
      headers: auth(),
      payload: { expectedRevision: 4, documentIds },
    });
    assert.equal(response.statusCode, 400);
    assert.equal(response.json().error, "invalid_learning_project_document_order");
    assert.equal(f.repository.projects.get(project.id)?.revision, 4);
  }
});

test("remove deletes only the relation and not the document", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject();
  const document = f.repository.createDocument();
  await addDocument(f.server, project.id, document.id, 1);

  const removed = await f.server.inject({
    method: "DELETE",
    url: `/learning/projects/${project.id}/documents/${document.id}`,
    headers: auth(),
    payload: { expectedRevision: 2 },
  });
  assert.equal(removed.statusCode, 200);
  assert.equal(removed.json().projectRevision, 3);
  assert.equal(f.repository.documents.has(document.id), true);
  assert.equal(f.repository.relations.size, 0);
});

test("concurrent association writes with one revision allow only one writer", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject();
  const leftDocument = f.repository.createDocument(USER_A.platformUserId, "left");
  const rightDocument = f.repository.createDocument(USER_A.platformUserId, "right");

  const [left, right] = await Promise.all([
    f.server.inject({
      method: "POST",
      url: `/learning/projects/${project.id}/documents`,
      headers: auth(),
      payload: { expectedRevision: 1, documentId: leftDocument.id },
    }),
    f.server.inject({
      method: "POST",
      url: `/learning/projects/${project.id}/documents`,
      headers: auth(),
      payload: { expectedRevision: 1, documentId: rightDocument.id },
    }),
  ]);

  assert.deepEqual([left.statusCode, right.statusCode].sort(), [201, 409]);
  const conflict = left.statusCode === 409 ? left : right;
  assert.equal(conflict.json().error, "learning_project_conflict");
  assert.equal(f.repository.projects.get(project.id)?.revision, 2);
  assert.equal(f.repository.relations.size, 1);
});

test("duplicate project copies document relations without copying file bodies or mutating source", async (t) => {
  const f = fixture(); t.after(f.close);
  const project = f.repository.createProject(USER_A.platformUserId, "source");
  const available = f.repository.createDocument(USER_A.platformUserId, "available");
  const deleted = f.repository.createDocument(USER_A.platformUserId, "deleted");
  await addDocument(f.server, project.id, available.id, 1, { importance: "core" });
  await addDocument(f.server, project.id, deleted.id, 2, { role: "reference" });
  f.repository.softDeleteDocument(deleted.id);

  const duplicate = await f.server.inject({
    method: "POST",
    url: `/learning/projects/${project.id}/duplicate`,
    headers: auth(),
    payload: { name: "copy" },
  });
  assert.equal(duplicate.statusCode, 201);
  const copyId = duplicate.json().project.id;
  assert.notEqual(copyId, project.id);
  assert.equal(duplicate.json().project.revision, 1);
  assert.equal(f.repository.projects.get(project.id)?.revision, 3);
  assert.equal(f.repository.documents.size, 2);

  const copiedRelations = await f.server.inject({ method: "GET", url: `/learning/projects/${copyId}/documents`, headers: auth() });
  assert.equal(copiedRelations.statusCode, 200);
  assert.equal(copiedRelations.json().documents.length, 2);
  assert.equal(copiedRelations.json().documents.find((item: { documentId: string }) => item.documentId === deleted.id).status, "deleted");
});
