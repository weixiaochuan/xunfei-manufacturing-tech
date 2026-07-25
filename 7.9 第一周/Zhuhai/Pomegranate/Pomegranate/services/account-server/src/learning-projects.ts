import { randomUUID } from "node:crypto";
import type { Pool } from "pg";

type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };
export type JsonArray = JsonValue[];

export interface PublicLearningProjectSummary {
  id: string;
  name: string;
  learningType: string | null;
  courseName: string | null;
  goalSummary: string | null;
  revision: number;
  lastOpenedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface PublicLearningProject extends PublicLearningProjectSummary {
  learningGoal: JsonObject;
  understanding: JsonObject;
  currentPlan: JsonObject;
  progress: JsonObject;
  planAdjustments: JsonArray;
  dataSchemaVersion: number;
}

export type LearningProjectUpdateResult =
  | { status: "updated"; project: PublicLearningProject }
  | { status: "not_found" }
  | { status: "conflict" };

export type LearningProjectDeleteResult =
  | { status: "deleted" }
  | { status: "not_found" }
  | { status: "conflict" };

export interface LearningProjectService {
  create(ownerUserId: string, input: unknown): Promise<PublicLearningProject>;
  list(ownerUserId: string, options: { sort: "updated" | "recent"; limit: number; offset: number }): Promise<PublicLearningProjectSummary[]>;
  get(ownerUserId: string, projectId: string): Promise<PublicLearningProject | null>;
  update(ownerUserId: string, projectId: string, input: unknown): Promise<LearningProjectUpdateResult>;
  rename(ownerUserId: string, projectId: string, input: unknown): Promise<LearningProjectUpdateResult>;
  open(ownerUserId: string, projectId: string): Promise<PublicLearningProject | null>;
  delete(ownerUserId: string, projectId: string, input: unknown): Promise<LearningProjectDeleteResult>;
  duplicate(ownerUserId: string, projectId: string, input: unknown): Promise<PublicLearningProject | null>;
}

export class LearningProjectValidationError extends Error {}

export interface LearningProjectRecord {
  id: string;
  ownerUserId: string;
  name: string;
  learningType: string | null;
  courseName: string | null;
  goalSummary: string | null;
  learningGoal: JsonObject;
  understanding: JsonObject;
  currentPlan: JsonObject;
  progress: JsonObject;
  planAdjustments: JsonArray;
  dataSchemaVersion: number;
  revision: number;
  lastOpenedAt: string | null;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface LearningProjectCreateRecord {
  id: string;
  name: string;
  learningType: string | null;
  courseName: string | null;
  goalSummary: string | null;
  learningGoal: JsonObject;
  understanding: JsonObject;
  currentPlan: JsonObject;
  progress: JsonObject;
  planAdjustments: JsonArray;
  dataSchemaVersion: number;
}

export interface LearningProjectPatchRecord {
  name?: string;
  learningType?: string | null;
  courseName?: string | null;
  goalSummary?: string | null;
  learningGoal?: JsonObject;
  understanding?: JsonObject;
  currentPlan?: JsonObject;
  progress?: JsonObject;
  planAdjustments?: JsonArray;
}

type RepositoryUpdateResult =
  | { status: "updated"; record: LearningProjectRecord }
  | { status: "not_found" }
  | { status: "conflict" };

type RepositoryDeleteResult =
  | { status: "deleted" }
  | { status: "not_found" }
  | { status: "conflict" };

export interface LearningProjectRepository {
  create(ownerUserId: string, input: LearningProjectCreateRecord): Promise<LearningProjectRecord>;
  list(ownerUserId: string, options: { sort: "updated" | "recent"; limit: number; offset: number }): Promise<LearningProjectRecord[]>;
  findActive(ownerUserId: string, projectId: string): Promise<LearningProjectRecord | null>;
  update(ownerUserId: string, projectId: string, expectedRevision: number, patch: LearningProjectPatchRecord): Promise<RepositoryUpdateResult>;
  open(ownerUserId: string, projectId: string): Promise<LearningProjectRecord | null>;
  delete(ownerUserId: string, projectId: string, expectedRevision: number): Promise<RepositoryDeleteResult>;
}

interface LearningProjectRow {
  id: string;
  owner_user_id: string;
  name: string;
  learning_type: string | null;
  course_name: string | null;
  goal_summary: string | null;
  learning_goal: JsonObject;
  understanding: JsonObject;
  current_plan: JsonObject;
  progress: JsonObject;
  plan_adjustments: JsonArray;
  data_schema_version: string | number;
  revision: string | number;
  last_opened_at: Date | string | null;
  created_at: Date | string;
  updated_at: Date | string;
  deleted_at: Date | string | null;
}

const MAX_NAME_BYTES = 512;
const MAX_SHORT_TEXT_BYTES = 2_000;
const MAX_JSON_FIELD_BYTES = 512 * 1024;
const FORBIDDEN_TOP_LEVEL_KEYS = new Set([
  "id",
  "ownerid",
  "owneruserid",
  "owneruserid",
  "owneruser_id",
  "owner_user_id",
  "platformuserid",
  "platformuser_id",
  "platform_user_id",
  "revision",
  "createdat",
  "created_at",
  "updatedat",
  "updated_at",
  "deletedat",
  "deleted_at",
]);
const FORBIDDEN_NESTED_KEY_PARTS = [
  "apikey",
  "api_key",
  "token",
  "secret",
  "password",
  "authorization",
  "bearer",
  "credential",
];
const FORBIDDEN_PATH_KEYS = new Set([
  "path",
  "localpath",
  "local_path",
  "sourcepath",
  "source_path",
  "filepath",
  "file_path",
]);

function iso(value: Date | string): string {
  return new Date(value).toISOString();
}

function nullableIso(value: Date | string | null): string | null {
  return value === null ? null : iso(value);
}

function normalizeKey(key: string): string {
  return key.toLowerCase().replace(/[^a-z0-9_]/g, "");
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function assertSafeJsonValue(value: unknown, code: string): asserts value is JsonValue {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    if (typeof value === "number" && !Number.isFinite(value)) {
      throw new LearningProjectValidationError(code);
    }
    return;
  }
  if (Array.isArray(value)) {
    for (const item of value) assertSafeJsonValue(item, code);
    return;
  }
  if (isPlainObject(value)) {
    for (const [key, item] of Object.entries(value)) {
      const normalized = normalizeKey(key);
      if (
        FORBIDDEN_PATH_KEYS.has(normalized) ||
        FORBIDDEN_NESTED_KEY_PARTS.some((part) => normalized.includes(part))
      ) {
        throw new LearningProjectValidationError("unsafe_learning_project_payload");
      }
      assertSafeJsonValue(item, code);
    }
    return;
  }
  throw new LearningProjectValidationError(code);
}

function assertNoForbiddenTopLevelKeys(input: Record<string, unknown>): void {
  for (const key of Object.keys(input)) {
    const normalized = normalizeKey(key);
    if (
      FORBIDDEN_TOP_LEVEL_KEYS.has(normalized) ||
      FORBIDDEN_PATH_KEYS.has(normalized) ||
      FORBIDDEN_NESTED_KEY_PARTS.some((part) => normalized.includes(part))
    ) {
      throw new LearningProjectValidationError("unsafe_learning_project_payload");
    }
  }
}

function jsonBytes(value: JsonValue): number {
  return Buffer.byteLength(JSON.stringify(value), "utf8");
}

function validName(value: unknown): string {
  if (typeof value !== "string") throw new LearningProjectValidationError("invalid_learning_project_name");
  const name = value.trim();
  if (!name || Buffer.byteLength(name, "utf8") > MAX_NAME_BYTES) {
    throw new LearningProjectValidationError("invalid_learning_project_name");
  }
  return name;
}

function validOptionalText(value: unknown, code: string): string | null {
  if (value === undefined || value === null) return null;
  if (typeof value !== "string") throw new LearningProjectValidationError(code);
  const text = value.trim();
  if (Buffer.byteLength(text, "utf8") > MAX_SHORT_TEXT_BYTES) {
    throw new LearningProjectValidationError(code);
  }
  return text || null;
}

function validExpectedRevision(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1) {
    throw new LearningProjectValidationError("invalid_expected_revision");
  }
  return value;
}

function validJsonObject(value: unknown, code: string): JsonObject {
  if (!isPlainObject(value)) throw new LearningProjectValidationError(code);
  assertSafeJsonValue(value, code);
  const object = value as JsonObject;
  if (jsonBytes(object) > MAX_JSON_FIELD_BYTES) {
    throw new LearningProjectValidationError("learning_project_json_too_large");
  }
  return object;
}

function validJsonArray(value: unknown, code: string): JsonArray {
  if (!Array.isArray(value)) throw new LearningProjectValidationError(code);
  assertSafeJsonValue(value, code);
  const array = value as JsonArray;
  if (jsonBytes(array) > MAX_JSON_FIELD_BYTES) {
    throw new LearningProjectValidationError("learning_project_json_too_large");
  }
  return array;
}

function bodyObject(value: unknown): Record<string, unknown> {
  if (value === undefined || value === null) return {};
  if (!isPlainObject(value)) throw new LearningProjectValidationError("invalid_learning_project_payload");
  assertNoForbiddenTopLevelKeys(value);
  return value;
}

function createInput(rawInput: unknown): LearningProjectCreateRecord {
  const input = bodyObject(rawInput);
  if (input.dataSchemaVersion !== undefined) {
    throw new LearningProjectValidationError("invalid_data_schema_version");
  }
  return {
    id: randomUUID(),
    name: validName(input.name),
    learningType: validOptionalText(input.learningType, "invalid_learning_type"),
    courseName: validOptionalText(input.courseName, "invalid_course_name"),
    goalSummary: validOptionalText(input.goalSummary, "invalid_goal_summary"),
    learningGoal: input.learningGoal === undefined ? {} : validJsonObject(input.learningGoal, "invalid_learning_goal"),
    understanding: input.understanding === undefined ? {} : validJsonObject(input.understanding, "invalid_understanding"),
    currentPlan: input.currentPlan === undefined ? {} : validJsonObject(input.currentPlan, "invalid_current_plan"),
    progress: input.progress === undefined ? {} : validJsonObject(input.progress, "invalid_progress"),
    planAdjustments: input.planAdjustments === undefined ? [] : validJsonArray(input.planAdjustments, "invalid_plan_adjustments"),
    dataSchemaVersion: 1,
  };
}

function patchInput(rawInput: unknown): { expectedRevision: number; patch: LearningProjectPatchRecord } {
  const input = bodyObject(rawInput);
  if (input.dataSchemaVersion !== undefined) {
    throw new LearningProjectValidationError("invalid_data_schema_version");
  }
  const expectedRevision = validExpectedRevision(input.expectedRevision);
  const patch: LearningProjectPatchRecord = {};
  if (input.name !== undefined) patch.name = validName(input.name);
  if (input.learningType !== undefined) patch.learningType = validOptionalText(input.learningType, "invalid_learning_type");
  if (input.courseName !== undefined) patch.courseName = validOptionalText(input.courseName, "invalid_course_name");
  if (input.goalSummary !== undefined) patch.goalSummary = validOptionalText(input.goalSummary, "invalid_goal_summary");
  if (input.learningGoal !== undefined) patch.learningGoal = validJsonObject(input.learningGoal, "invalid_learning_goal");
  if (input.understanding !== undefined) patch.understanding = validJsonObject(input.understanding, "invalid_understanding");
  if (input.currentPlan !== undefined) patch.currentPlan = validJsonObject(input.currentPlan, "invalid_current_plan");
  if (input.progress !== undefined) patch.progress = validJsonObject(input.progress, "invalid_progress");
  if (input.planAdjustments !== undefined) patch.planAdjustments = validJsonArray(input.planAdjustments, "invalid_plan_adjustments");
  if (Object.keys(patch).length === 0) throw new LearningProjectValidationError("empty_update");
  return { expectedRevision, patch };
}

function renameInput(rawInput: unknown): { expectedRevision: number; name: string } {
  const input = bodyObject(rawInput);
  return { expectedRevision: validExpectedRevision(input.expectedRevision), name: validName(input.name) };
}

function deleteInput(rawInput: unknown): number {
  return validExpectedRevision(bodyObject(rawInput).expectedRevision);
}

function duplicateName(rawInput: unknown, sourceName: string): string {
  const input = bodyObject(rawInput);
  if (input.name === undefined || input.name === null) return validName(`${sourceName} 副本`);
  return validName(input.name);
}

function mapRecord(row: LearningProjectRow): LearningProjectRecord {
  return {
    id: row.id,
    ownerUserId: row.owner_user_id,
    name: row.name,
    learningType: row.learning_type,
    courseName: row.course_name,
    goalSummary: row.goal_summary,
    learningGoal: row.learning_goal,
    understanding: row.understanding,
    currentPlan: row.current_plan,
    progress: row.progress,
    planAdjustments: row.plan_adjustments,
    dataSchemaVersion: Number(row.data_schema_version),
    revision: Number(row.revision),
    lastOpenedAt: nullableIso(row.last_opened_at),
    createdAt: iso(row.created_at),
    updatedAt: iso(row.updated_at),
    deletedAt: nullableIso(row.deleted_at),
  };
}

function summary(record: LearningProjectRecord): PublicLearningProjectSummary {
  return {
    id: record.id,
    name: record.name,
    learningType: record.learningType,
    courseName: record.courseName,
    goalSummary: record.goalSummary,
    revision: record.revision,
    lastOpenedAt: record.lastOpenedAt,
    createdAt: record.createdAt,
    updatedAt: record.updatedAt,
  };
}

function project(record: LearningProjectRecord): PublicLearningProject {
  return {
    ...summary(record),
    learningGoal: record.learningGoal,
    understanding: record.understanding,
    currentPlan: record.currentPlan,
    progress: record.progress,
    planAdjustments: record.planAdjustments,
    dataSchemaVersion: record.dataSchemaVersion,
  };
}

const PROJECT_SELECT = `
  SELECT id, owner_user_id, name, learning_type, course_name, goal_summary,
    learning_goal, understanding, current_plan, progress, plan_adjustments,
    data_schema_version, revision, last_opened_at, created_at, updated_at, deleted_at
  FROM learning_projects`;

export function createPostgresLearningProjectRepository(pool: Pool): LearningProjectRepository {
  return {
    async create(ownerUserId, input) {
      const result = await pool.query<LearningProjectRow>(
        `WITH inserted AS (
           INSERT INTO learning_projects (
             id, owner_user_id, name, learning_type, course_name, goal_summary,
             learning_goal, understanding, current_plan, progress, plan_adjustments,
             data_schema_version, revision
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 1)
           RETURNING id, owner_user_id, name, learning_type, course_name, goal_summary,
             learning_goal, understanding, current_plan, progress, plan_adjustments,
             data_schema_version, revision, last_opened_at, created_at, updated_at, deleted_at
         )
         SELECT id, owner_user_id, name, learning_type, course_name, goal_summary,
           learning_goal, understanding, current_plan, progress, plan_adjustments,
           data_schema_version, revision, last_opened_at, created_at, updated_at, deleted_at
         FROM inserted`,
        [
          input.id,
          ownerUserId,
          input.name,
          input.learningType,
          input.courseName,
          input.goalSummary,
          input.learningGoal,
          input.understanding,
          input.currentPlan,
          input.progress,
          input.planAdjustments,
          input.dataSchemaVersion,
        ],
      );
      return mapRecord(result.rows[0]!);
    },

    async list(ownerUserId, options) {
      const order = options.sort === "recent"
        ? "COALESCE(last_opened_at, updated_at) DESC, updated_at DESC, id DESC"
        : "updated_at DESC, id DESC";
      const result = await pool.query<LearningProjectRow>(
        `${PROJECT_SELECT}
         WHERE owner_user_id = $1 AND deleted_at IS NULL
         ORDER BY ${order}
         LIMIT $2 OFFSET $3`,
        [ownerUserId, options.limit, options.offset],
      );
      return result.rows.map(mapRecord);
    },

    async findActive(ownerUserId, projectId) {
      const result = await pool.query<LearningProjectRow>(
        `${PROJECT_SELECT} WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL`,
        [projectId, ownerUserId],
      );
      return result.rows[0] ? mapRecord(result.rows[0]) : null;
    },

    async update(ownerUserId, projectId, expectedRevision, patch) {
      const existing = await this.findActive(ownerUserId, projectId);
      if (!existing) return { status: "not_found" };
      if (existing.revision !== expectedRevision) return { status: "conflict" };
      const next = {
        name: patch.name ?? existing.name,
        learningType: patch.learningType === undefined ? existing.learningType : patch.learningType,
        courseName: patch.courseName === undefined ? existing.courseName : patch.courseName,
        goalSummary: patch.goalSummary === undefined ? existing.goalSummary : patch.goalSummary,
        learningGoal: patch.learningGoal ?? existing.learningGoal,
        understanding: patch.understanding ?? existing.understanding,
        currentPlan: patch.currentPlan ?? existing.currentPlan,
        progress: patch.progress ?? existing.progress,
        planAdjustments: patch.planAdjustments ?? existing.planAdjustments,
      };
      const result = await pool.query<LearningProjectRow>(
        `UPDATE learning_projects
         SET name = $3,
             learning_type = $4,
             course_name = $5,
             goal_summary = $6,
             learning_goal = $7,
             understanding = $8,
             current_plan = $9,
             progress = $10,
             plan_adjustments = $11,
             revision = revision + 1,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND owner_user_id = $2 AND revision = $12 AND deleted_at IS NULL
         RETURNING id, owner_user_id, name, learning_type, course_name, goal_summary,
           learning_goal, understanding, current_plan, progress, plan_adjustments,
           data_schema_version, revision, last_opened_at, created_at, updated_at, deleted_at`,
        [
          projectId,
          ownerUserId,
          next.name,
          next.learningType,
          next.courseName,
          next.goalSummary,
          next.learningGoal,
          next.understanding,
          next.currentPlan,
          next.progress,
          next.planAdjustments,
          expectedRevision,
        ],
      );
      return result.rows[0] ? { status: "updated", record: mapRecord(result.rows[0]) } : { status: "conflict" };
    },

    async open(ownerUserId, projectId) {
      const result = await pool.query<LearningProjectRow>(
        `UPDATE learning_projects
         SET last_opened_at = CURRENT_TIMESTAMP
         WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL
         RETURNING id, owner_user_id, name, learning_type, course_name, goal_summary,
           learning_goal, understanding, current_plan, progress, plan_adjustments,
           data_schema_version, revision, last_opened_at, created_at, updated_at, deleted_at`,
        [projectId, ownerUserId],
      );
      return result.rows[0] ? mapRecord(result.rows[0]) : null;
    },

    async delete(ownerUserId, projectId, expectedRevision) {
      const existing = await this.findActive(ownerUserId, projectId);
      if (!existing) return { status: "not_found" };
      if (existing.revision !== expectedRevision) return { status: "conflict" };
      const result = await pool.query(
        `UPDATE learning_projects
         SET deleted_at = CURRENT_TIMESTAMP,
             updated_at = CURRENT_TIMESTAMP,
             revision = revision + 1
         WHERE id = $1 AND owner_user_id = $2 AND revision = $3 AND deleted_at IS NULL`,
        [projectId, ownerUserId, expectedRevision],
      );
      return result.rowCount === 1 ? { status: "deleted" } : { status: "conflict" };
    },
  };
}

export function createLearningProjectService(repository: LearningProjectRepository): LearningProjectService {
  return {
    async create(ownerUserId, input) {
      return project(await repository.create(ownerUserId, createInput(input)));
    },

    async list(ownerUserId, options) {
      return (await repository.list(ownerUserId, options)).map(summary);
    },

    async get(ownerUserId, projectId) {
      const record = await repository.findActive(ownerUserId, projectId);
      return record ? project(record) : null;
    },

    async update(ownerUserId, projectId, input) {
      const { expectedRevision, patch } = patchInput(input);
      const result = await repository.update(ownerUserId, projectId, expectedRevision, patch);
      return result.status === "updated" ? { status: "updated", project: project(result.record) } : result;
    },

    async rename(ownerUserId, projectId, input) {
      const parsed = renameInput(input);
      const result = await repository.update(ownerUserId, projectId, parsed.expectedRevision, { name: parsed.name });
      return result.status === "updated" ? { status: "updated", project: project(result.record) } : result;
    },

    async open(ownerUserId, projectId) {
      const record = await repository.open(ownerUserId, projectId);
      return record ? project(record) : null;
    },

    delete(ownerUserId, projectId, input) {
      return repository.delete(ownerUserId, projectId, deleteInput(input));
    },

    async duplicate(ownerUserId, projectId, input) {
      const source = await repository.findActive(ownerUserId, projectId);
      if (!source) return null;
      const record = await repository.create(ownerUserId, {
        id: randomUUID(),
        name: duplicateName(input, source.name),
        learningType: source.learningType,
        courseName: source.courseName,
        goalSummary: source.goalSummary,
        learningGoal: structuredClone(source.learningGoal),
        understanding: structuredClone(source.understanding),
        currentPlan: structuredClone(source.currentPlan),
        progress: structuredClone(source.progress),
        planAdjustments: structuredClone(source.planAdjustments),
        dataSchemaVersion: source.dataSchemaVersion,
      });
      return project(record);
    },
  };
}
