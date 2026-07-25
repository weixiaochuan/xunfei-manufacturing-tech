import type { Pool, PoolClient } from "pg";

export type LearningProjectDocumentRole =
  | "material"
  | "syllabus"
  | "note"
  | "exercise"
  | "reference"
  | "other";

export type LearningProjectDocumentImportance = "normal" | "important" | "core";

export interface PublicLearningProjectDocument {
  documentId: string;
  title: string;
  documentType: string;
  role: LearningProjectDocumentRole;
  importance: LearningProjectDocumentImportance;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
  status: "available" | "deleted";
}

export type LearningProjectDocumentListResult =
  | { status: "ok"; projectRevision: number; documents: PublicLearningProjectDocument[] }
  | { status: "not_found" };

export type LearningProjectDocumentWriteResult =
  | { status: "updated"; projectRevision: number; document?: PublicLearningProjectDocument }
  | { status: "not_found" }
  | { status: "conflict" }
  | { status: "exists" }
  | { status: "invalid_order" };

export interface LearningProjectDocumentService {
  listProjectDocuments(ownerUserId: string, projectId: string): Promise<LearningProjectDocumentListResult>;
  addProjectDocument(ownerUserId: string, projectId: string, input: unknown): Promise<LearningProjectDocumentWriteResult>;
  updateProjectDocument(ownerUserId: string, projectId: string, documentId: string, input: unknown): Promise<LearningProjectDocumentWriteResult>;
  removeProjectDocument(ownerUserId: string, projectId: string, documentId: string, input: unknown): Promise<LearningProjectDocumentWriteResult>;
  reorderProjectDocuments(ownerUserId: string, projectId: string, input: unknown): Promise<LearningProjectDocumentWriteResult>;
  copyProjectDocumentRelations(ownerUserId: string, sourceProjectId: string, targetProjectId: string): Promise<number>;
}

export interface LearningProjectDocumentRepository {
  list(ownerUserId: string, projectId: string): Promise<LearningProjectDocumentListResult>;
  add(ownerUserId: string, projectId: string, input: AddProjectDocumentRecord): Promise<LearningProjectDocumentWriteResult>;
  update(ownerUserId: string, projectId: string, documentId: string, input: UpdateProjectDocumentRecord): Promise<LearningProjectDocumentWriteResult>;
  remove(ownerUserId: string, projectId: string, documentId: string, expectedRevision: number): Promise<LearningProjectDocumentWriteResult>;
  reorder(ownerUserId: string, projectId: string, input: ReorderProjectDocumentsRecord): Promise<LearningProjectDocumentWriteResult>;
  copy(ownerUserId: string, sourceProjectId: string, targetProjectId: string): Promise<number>;
}

export interface AddProjectDocumentRecord {
  expectedRevision: number;
  documentId: string;
  role: LearningProjectDocumentRole;
  importance: LearningProjectDocumentImportance;
  sortOrder: number | null;
}

export interface UpdateProjectDocumentRecord {
  expectedRevision: number;
  role?: LearningProjectDocumentRole;
  importance?: LearningProjectDocumentImportance;
  sortOrder?: number;
}

export interface ReorderProjectDocumentsRecord {
  expectedRevision: number;
  documentIds: string[];
}

export class LearningProjectDocumentValidationError extends Error {}

interface ProjectRevisionRow {
  revision: string | number;
}

interface ProjectDocumentRow {
  document_id: string;
  title: string;
  document_type: string;
  role: LearningProjectDocumentRole;
  importance: LearningProjectDocumentImportance;
  sort_order: number;
  created_at: Date | string;
  updated_at: Date | string;
  document_deleted_at: Date | string | null;
}

const ROLES = new Set<LearningProjectDocumentRole>([
  "material",
  "syllabus",
  "note",
  "exercise",
  "reference",
  "other",
]);

const IMPORTANCE = new Set<LearningProjectDocumentImportance>([
  "normal",
  "important",
  "core",
]);

function iso(value: Date | string): string {
  return new Date(value).toISOString();
}

function nullableIso(value: Date | string | null): string | null {
  return value === null ? null : iso(value);
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function bodyObject(value: unknown): Record<string, unknown> {
  if (value === undefined || value === null) return {};
  if (!isPlainObject(value)) throw new LearningProjectDocumentValidationError("invalid_learning_project_document_payload");
  return value;
}

function isUuid(value: unknown): value is string {
  return typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
}

function validUuid(value: unknown, code: string): string {
  if (!isUuid(value)) throw new LearningProjectDocumentValidationError(code);
  return value;
}

function validExpectedRevision(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1) {
    throw new LearningProjectDocumentValidationError("invalid_expected_revision");
  }
  return value;
}

function validRole(value: unknown): LearningProjectDocumentRole {
  if (value === undefined || value === null) return "material";
  if (typeof value !== "string" || !ROLES.has(value as LearningProjectDocumentRole)) {
    throw new LearningProjectDocumentValidationError("invalid_learning_project_document_role");
  }
  return value as LearningProjectDocumentRole;
}

function validImportance(value: unknown): LearningProjectDocumentImportance {
  if (value === undefined || value === null) return "normal";
  if (typeof value !== "string" || !IMPORTANCE.has(value as LearningProjectDocumentImportance)) {
    throw new LearningProjectDocumentValidationError("invalid_learning_project_document_importance");
  }
  return value as LearningProjectDocumentImportance;
}

function validSortOrder(value: unknown): number | null {
  if (value === undefined || value === null) return null;
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new LearningProjectDocumentValidationError("invalid_learning_project_document_sort_order");
  }
  return value;
}

function addInput(rawInput: unknown): AddProjectDocumentRecord {
  const input = bodyObject(rawInput);
  return {
    expectedRevision: validExpectedRevision(input.expectedRevision),
    documentId: validUuid(input.documentId, "invalid_document_id"),
    role: validRole(input.role),
    importance: validImportance(input.importance),
    sortOrder: validSortOrder(input.sortOrder),
  };
}

function updateInput(rawInput: unknown): UpdateProjectDocumentRecord {
  const input = bodyObject(rawInput);
  const parsed: UpdateProjectDocumentRecord = {
    expectedRevision: validExpectedRevision(input.expectedRevision),
  };
  if (input.role !== undefined) parsed.role = validRole(input.role);
  if (input.importance !== undefined) parsed.importance = validImportance(input.importance);
  if (input.sortOrder !== undefined) {
    const sortOrder = validSortOrder(input.sortOrder);
    if (sortOrder === null) throw new LearningProjectDocumentValidationError("invalid_learning_project_document_sort_order");
    parsed.sortOrder = sortOrder;
  }
  if (parsed.role === undefined && parsed.importance === undefined && parsed.sortOrder === undefined) {
    throw new LearningProjectDocumentValidationError("empty_learning_project_document_update");
  }
  return parsed;
}

function deleteInput(rawInput: unknown): number {
  return validExpectedRevision(bodyObject(rawInput).expectedRevision);
}

function reorderInput(rawInput: unknown): ReorderProjectDocumentsRecord {
  const input = bodyObject(rawInput);
  if (!Array.isArray(input.documentIds)) {
    throw new LearningProjectDocumentValidationError("invalid_learning_project_document_order");
  }
  const seen = new Set<string>();
  const documentIds = input.documentIds.map((value) => {
    const documentId = validUuid(value, "invalid_document_id");
    if (seen.has(documentId)) {
      throw new LearningProjectDocumentValidationError("invalid_learning_project_document_order");
    }
    seen.add(documentId);
    return documentId;
  });
  return {
    expectedRevision: validExpectedRevision(input.expectedRevision),
    documentIds,
  };
}

function mapProjectDocument(row: ProjectDocumentRow): PublicLearningProjectDocument {
  const deletedAt = nullableIso(row.document_deleted_at);
  return {
    documentId: row.document_id,
    title: row.title,
    documentType: row.document_type,
    role: row.role,
    importance: row.importance,
    sortOrder: Number(row.sort_order),
    createdAt: iso(row.created_at),
    updatedAt: iso(row.updated_at),
    deletedAt,
    status: deletedAt === null ? "available" : "deleted",
  };
}

async function transaction<T>(pool: Pool, fn: (client: PoolClient) => Promise<T>): Promise<T> {
  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    const result = await fn(client);
    await client.query("COMMIT");
    return result;
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

async function lockProject(
  client: PoolClient,
  ownerUserId: string,
  projectId: string,
  expectedRevision: number,
): Promise<"ok" | "not_found" | "conflict"> {
  const project = await client.query<ProjectRevisionRow>(
    `SELECT revision
     FROM learning_projects
     WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL
     FOR UPDATE`,
    [projectId, ownerUserId],
  );
  if (!project.rows[0]) return "not_found";
  return Number(project.rows[0].revision) === expectedRevision ? "ok" : "conflict";
}

async function bumpProjectRevision(
  client: PoolClient,
  ownerUserId: string,
  projectId: string,
  expectedRevision: number,
): Promise<number> {
  const result = await client.query<ProjectRevisionRow>(
    `UPDATE learning_projects
     SET revision = revision + 1,
         updated_at = CURRENT_TIMESTAMP
     WHERE id = $1 AND owner_user_id = $2 AND revision = $3 AND deleted_at IS NULL
     RETURNING revision`,
    [projectId, ownerUserId, expectedRevision],
  );
  return Number(result.rows[0]!.revision);
}

async function relationExists(
  client: PoolClient,
  ownerUserId: string,
  projectId: string,
  documentId: string,
): Promise<boolean> {
  const result = await client.query(
    `SELECT 1
     FROM learning_project_documents
     WHERE owner_user_id = $1 AND project_id = $2 AND document_id = $3`,
    [ownerUserId, projectId, documentId],
  );
  return result.rowCount === 1;
}

async function activeDocumentExists(
  client: PoolClient,
  ownerUserId: string,
  documentId: string,
): Promise<boolean> {
  const result = await client.query(
    `SELECT 1
     FROM documents
     WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL`,
    [documentId, ownerUserId],
  );
  return result.rowCount === 1;
}

async function selectProjectDocument(
  client: PoolClient,
  ownerUserId: string,
  projectId: string,
  documentId: string,
): Promise<PublicLearningProjectDocument | null> {
  const result = await client.query<ProjectDocumentRow>(
    `SELECT lpd.document_id,
            d.title,
            d.document_kind AS document_type,
            lpd.role,
            lpd.importance,
            lpd.sort_order,
            lpd.created_at,
            lpd.updated_at,
            d.deleted_at AS document_deleted_at
     FROM learning_project_documents lpd
     JOIN documents d
       ON d.id = lpd.document_id
      AND d.owner_user_id = lpd.owner_user_id
     WHERE lpd.owner_user_id = $1
       AND lpd.project_id = $2
       AND lpd.document_id = $3`,
    [ownerUserId, projectId, documentId],
  );
  return result.rows[0] ? mapProjectDocument(result.rows[0]) : null;
}

async function nextSortOrder(client: PoolClient, ownerUserId: string, projectId: string): Promise<number> {
  const result = await client.query<{ next_sort_order: number | string | null }>(
    `SELECT COALESCE(MAX(sort_order) + 1, 0) AS next_sort_order
     FROM learning_project_documents
     WHERE owner_user_id = $1 AND project_id = $2`,
    [ownerUserId, projectId],
  );
  return Number(result.rows[0]?.next_sort_order ?? 0);
}

async function currentRelationIds(client: PoolClient, ownerUserId: string, projectId: string): Promise<string[]> {
  const result = await client.query<{ document_id: string }>(
    `SELECT document_id
     FROM learning_project_documents
     WHERE owner_user_id = $1 AND project_id = $2
     ORDER BY sort_order ASC, created_at ASC, document_id ASC`,
    [ownerUserId, projectId],
  );
  return result.rows.map((row) => row.document_id);
}

export async function copyProjectDocumentRelationsInTransaction(
  client: PoolClient,
  ownerUserId: string,
  sourceProjectId: string,
  targetProjectId: string,
): Promise<number> {
  const result = await client.query(
    `INSERT INTO learning_project_documents (
       project_id, owner_user_id, document_id, role, importance, sort_order, created_at, updated_at
     )
     SELECT $3, owner_user_id, document_id, role, importance, sort_order, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
     FROM learning_project_documents
     WHERE owner_user_id = $1 AND project_id = $2`,
    [ownerUserId, sourceProjectId, targetProjectId],
  );
  return result.rowCount ?? 0;
}

export function createPostgresLearningProjectDocumentRepository(pool: Pool): LearningProjectDocumentRepository {
  return {
    async list(ownerUserId, projectId) {
      const project = await pool.query<ProjectRevisionRow>(
        `SELECT revision
         FROM learning_projects
         WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL`,
        [projectId, ownerUserId],
      );
      if (!project.rows[0]) return { status: "not_found" };
      const documents = await pool.query<ProjectDocumentRow>(
        `SELECT lpd.document_id,
                d.title,
                d.document_kind AS document_type,
                lpd.role,
                lpd.importance,
                lpd.sort_order,
                lpd.created_at,
                lpd.updated_at,
                d.deleted_at AS document_deleted_at
         FROM learning_project_documents lpd
         JOIN documents d
           ON d.id = lpd.document_id
          AND d.owner_user_id = lpd.owner_user_id
         WHERE lpd.owner_user_id = $1 AND lpd.project_id = $2
         ORDER BY lpd.sort_order ASC, lpd.created_at ASC, lpd.document_id ASC`,
        [ownerUserId, projectId],
      );
      return {
        status: "ok",
        projectRevision: Number(project.rows[0].revision),
        documents: documents.rows.map(mapProjectDocument),
      };
    },

    async add(ownerUserId, projectId, input) {
      return transaction(pool, async (client) => {
        const projectStatus = await lockProject(client, ownerUserId, projectId, input.expectedRevision);
        if (projectStatus !== "ok") return { status: projectStatus };
        if (await relationExists(client, ownerUserId, projectId, input.documentId)) {
          return { status: "exists" };
        }
        if (!(await activeDocumentExists(client, ownerUserId, input.documentId))) {
          return { status: "not_found" };
        }
        const sortOrder = input.sortOrder ?? await nextSortOrder(client, ownerUserId, projectId);
        const projectRevision = await bumpProjectRevision(client, ownerUserId, projectId, input.expectedRevision);
        await client.query(
          `INSERT INTO learning_project_documents (
             project_id, owner_user_id, document_id, role, importance, sort_order
           ) VALUES ($1, $2, $3, $4, $5, $6)`,
          [projectId, ownerUserId, input.documentId, input.role, input.importance, sortOrder],
        );
        const document = await selectProjectDocument(client, ownerUserId, projectId, input.documentId);
        if (!document) throw new Error("learning_project_document_insert_failed");
        return {
          status: "updated",
          projectRevision,
          document,
        };
      });
    },

    async update(ownerUserId, projectId, documentId, input) {
      return transaction(pool, async (client) => {
        const projectStatus = await lockProject(client, ownerUserId, projectId, input.expectedRevision);
        if (projectStatus !== "ok") return { status: projectStatus };
        if (!(await relationExists(client, ownerUserId, projectId, documentId))) {
          return { status: "not_found" };
        }
        const existing = await selectProjectDocument(client, ownerUserId, projectId, documentId);
        const projectRevision = await bumpProjectRevision(client, ownerUserId, projectId, input.expectedRevision);
        await client.query(
          `UPDATE learning_project_documents
           SET role = $4,
               importance = $5,
               sort_order = $6,
               updated_at = CURRENT_TIMESTAMP
           WHERE owner_user_id = $1 AND project_id = $2 AND document_id = $3`,
          [
            ownerUserId,
            projectId,
            documentId,
            input.role ?? existing!.role,
            input.importance ?? existing!.importance,
            input.sortOrder ?? existing!.sortOrder,
          ],
        );
        const document = await selectProjectDocument(client, ownerUserId, projectId, documentId);
        if (!document) throw new Error("learning_project_document_update_failed");
        return {
          status: "updated",
          projectRevision,
          document,
        };
      });
    },

    async remove(ownerUserId, projectId, documentId, expectedRevision) {
      return transaction(pool, async (client) => {
        const projectStatus = await lockProject(client, ownerUserId, projectId, expectedRevision);
        if (projectStatus !== "ok") return { status: projectStatus };
        if (!(await relationExists(client, ownerUserId, projectId, documentId))) {
          return { status: "not_found" };
        }
        const projectRevision = await bumpProjectRevision(client, ownerUserId, projectId, expectedRevision);
        await client.query(
          `DELETE FROM learning_project_documents
           WHERE owner_user_id = $1 AND project_id = $2 AND document_id = $3`,
          [ownerUserId, projectId, documentId],
        );
        return { status: "updated", projectRevision };
      });
    },

    async reorder(ownerUserId, projectId, input) {
      return transaction(pool, async (client) => {
        const projectStatus = await lockProject(client, ownerUserId, projectId, input.expectedRevision);
        if (projectStatus !== "ok") return { status: projectStatus };
        const currentIds = await currentRelationIds(client, ownerUserId, projectId);
        if (currentIds.length !== input.documentIds.length) {
          return { status: "invalid_order" };
        }
        const currentSet = new Set(currentIds);
        if (input.documentIds.some((documentId) => !currentSet.has(documentId))) {
          return { status: "invalid_order" };
        }
        const projectRevision = await bumpProjectRevision(client, ownerUserId, projectId, input.expectedRevision);
        for (const [sortOrder, documentId] of input.documentIds.entries()) {
          await client.query(
            `UPDATE learning_project_documents
             SET sort_order = $4,
                 updated_at = CURRENT_TIMESTAMP
             WHERE owner_user_id = $1 AND project_id = $2 AND document_id = $3`,
            [ownerUserId, projectId, documentId, sortOrder],
          );
        }
        return { status: "updated", projectRevision };
      });
    },

    async copy(ownerUserId, sourceProjectId, targetProjectId) {
      return transaction(pool, (client) =>
        copyProjectDocumentRelationsInTransaction(client, ownerUserId, sourceProjectId, targetProjectId));
    },
  };
}

export function createLearningProjectDocumentService(
  repository: LearningProjectDocumentRepository,
): LearningProjectDocumentService {
  return {
    listProjectDocuments(ownerUserId, projectId) {
      return repository.list(ownerUserId, validUuid(projectId, "invalid_project_id"));
    },
    addProjectDocument(ownerUserId, projectId, input) {
      return repository.add(ownerUserId, validUuid(projectId, "invalid_project_id"), addInput(input));
    },
    updateProjectDocument(ownerUserId, projectId, documentId, input) {
      return repository.update(
        ownerUserId,
        validUuid(projectId, "invalid_project_id"),
        validUuid(documentId, "invalid_document_id"),
        updateInput(input),
      );
    },
    removeProjectDocument(ownerUserId, projectId, documentId, input) {
      return repository.remove(
        ownerUserId,
        validUuid(projectId, "invalid_project_id"),
        validUuid(documentId, "invalid_document_id"),
        deleteInput(input),
      );
    },
    reorderProjectDocuments(ownerUserId, projectId, input) {
      return repository.reorder(ownerUserId, validUuid(projectId, "invalid_project_id"), reorderInput(input));
    },
    copyProjectDocumentRelations(ownerUserId, sourceProjectId, targetProjectId) {
      return repository.copy(
        ownerUserId,
        validUuid(sourceProjectId, "invalid_project_id"),
        validUuid(targetProjectId, "invalid_project_id"),
      );
    },
  };
}
