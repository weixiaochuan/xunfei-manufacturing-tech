import { invoke } from "@tauri-apps/api/core";

import { captureDocumentRequest } from "../documents/documentSession.ts";
import {
  ACCOUNT_LEARNING_JS_SAFE_INTEGER_MAX,
  type AccountLearningEnvelope,
  type AccountLearningProjectSort,
  type CreateAccountLearningProjectInput,
  type DuplicateAccountLearningProjectInput,
  type JsonArray,
  type JsonObject,
  type LearningProjectDeleteData,
  type LearningProjectDetail,
  type LearningProjectListData,
  type LearningProjectRevisionInput,
  type ListAccountLearningProjectsInput,
  type RenameAccountLearningProjectInput,
  type UpdateAccountLearningProjectInput,
  createAccountLearningValidationError,
  finalizeAccountLearningEnvelope,
  parseAccountLearningEnvelope,
  parseLearningProjectDeleteData,
  parseLearningProjectDetail,
  parseLearningProjectListData,
  sanitizeExpectedRevision,
  sanitizeOptionalJsonArray,
  sanitizeOptionalJsonObject,
  sanitizeOptionalString,
  sanitizeRequiredString,
  sanitizeUuidInput,
} from "./accountLearningTypes.ts";

export async function listAccountLearningProjects(
  input: ListAccountLearningProjectsInput = {},
): Promise<AccountLearningEnvelope<LearningProjectListData>> {
  return invokeAccountLearningProjectCommand(
    "account_learning_projects_list",
    sanitizeListInput(input),
    parseLearningProjectListData,
  );
}

export async function createAccountLearningProject(
  input: CreateAccountLearningProjectInput,
): Promise<AccountLearningEnvelope<LearningProjectDetail>> {
  return invokeAccountLearningProjectCommand(
    "account_learning_project_create",
    sanitizeCreateInput(input),
    parseLearningProjectDetail,
  );
}

export async function getAccountLearningProject(
  projectId: string,
): Promise<AccountLearningEnvelope<LearningProjectDetail>> {
  return invokeAccountLearningProjectCommand(
    "account_learning_project_get",
    { projectId: sanitizeUuidInput(projectId) },
    parseLearningProjectDetail,
  );
}

export async function updateAccountLearningProject(
  input: UpdateAccountLearningProjectInput,
): Promise<AccountLearningEnvelope<LearningProjectDetail>> {
  return invokeAccountLearningProjectCommand(
    "account_learning_project_update",
    sanitizeUpdateInput(input),
    parseLearningProjectDetail,
  );
}

export async function renameAccountLearningProject(
  input: RenameAccountLearningProjectInput,
): Promise<AccountLearningEnvelope<LearningProjectDetail>> {
  return invokeAccountLearningProjectCommand(
    "account_learning_project_rename",
    {
      projectId: sanitizeUuidInput(input.projectId),
      expectedRevision: sanitizeExpectedRevision(input.expectedRevision),
      name: sanitizeRequiredString(input.name),
    },
    parseLearningProjectDetail,
  );
}

export async function openAccountLearningProject(
  projectId: string,
): Promise<AccountLearningEnvelope<LearningProjectDetail>> {
  return invokeAccountLearningProjectCommand(
    "account_learning_project_open",
    { projectId: sanitizeUuidInput(projectId) },
    parseLearningProjectDetail,
  );
}

export async function deleteAccountLearningProject(
  input: LearningProjectRevisionInput,
): Promise<AccountLearningEnvelope<LearningProjectDeleteData>> {
  return invokeAccountLearningProjectCommand(
    "account_learning_project_delete",
    {
      projectId: sanitizeUuidInput(input.projectId),
      expectedRevision: sanitizeExpectedRevision(input.expectedRevision),
    },
    parseLearningProjectDeleteData,
  );
}

export async function duplicateAccountLearningProject(
  input: DuplicateAccountLearningProjectInput,
): Promise<AccountLearningEnvelope<LearningProjectDetail>> {
  const sanitized: Record<string, unknown> = {
    projectId: sanitizeUuidInput(input.projectId),
  };

  if (Object.prototype.hasOwnProperty.call(input, "name")) {
    const name = sanitizeOptionalString(input.name);
    if (name !== undefined && name !== null) {
      sanitized.name = sanitizeRequiredString(name);
    }
  }

  return invokeAccountLearningProjectCommand(
    "account_learning_project_duplicate",
    sanitized,
    parseLearningProjectDetail,
  );
}

async function invokeAccountLearningProjectCommand<T>(
  command: string,
  input: Record<string, unknown>,
  parseData: (rawData: unknown) => T,
): Promise<AccountLearningEnvelope<T>> {
  const requestContext = captureDocumentRequest();
  const raw = await invoke(command, { input });
  const envelope = parseAccountLearningEnvelope(raw, parseData);
  return finalizeAccountLearningEnvelope(envelope, requestContext);
}

function sanitizeListInput(
  input: ListAccountLearningProjectsInput,
): Record<string, unknown> {
  const sanitized: Record<string, unknown> = {};

  if (input.sort !== undefined) {
    sanitized.sort = sanitizeProjectSort(input.sort);
  }
  if (input.limit !== undefined) {
    sanitized.limit = sanitizeIntegerRange(input.limit, 1, 100);
  }
  if (input.offset !== undefined) {
    sanitized.offset = sanitizeIntegerRange(
      input.offset,
      0,
      ACCOUNT_LEARNING_JS_SAFE_INTEGER_MAX,
    );
  }

  return sanitized;
}

function sanitizeCreateInput(
  input: CreateAccountLearningProjectInput,
): Record<string, unknown> {
  const sanitized: Record<string, unknown> = {
    name: sanitizeRequiredString(input.name),
  };

  insertOptionalText(sanitized, input, "learningType");
  insertOptionalText(sanitized, input, "courseName");
  insertOptionalText(sanitized, input, "goalSummary");
  insertOptionalObject(sanitized, input, "learningGoal");
  insertOptionalObject(sanitized, input, "understanding");
  insertOptionalObject(sanitized, input, "currentPlan");
  insertOptionalObject(sanitized, input, "progress");
  insertOptionalArray(sanitized, input, "planAdjustments");

  return sanitized;
}

function sanitizeUpdateInput(
  input: UpdateAccountLearningProjectInput,
): Record<string, unknown> {
  const sanitized: Record<string, unknown> = {
    projectId: sanitizeUuidInput(input.projectId),
    expectedRevision: sanitizeExpectedRevision(input.expectedRevision),
  };

  if (Object.prototype.hasOwnProperty.call(input, "name")) {
    const name = sanitizeOptionalString(input.name);
    if (name !== undefined) {
      if (name === null) {
        throw createAccountLearningValidationError();
      }
      sanitized.name = sanitizeRequiredString(name);
    }
  }
  insertOptionalText(sanitized, input, "learningType");
  insertOptionalText(sanitized, input, "courseName");
  insertOptionalText(sanitized, input, "goalSummary");
  insertOptionalObject(sanitized, input, "learningGoal");
  insertOptionalObject(sanitized, input, "understanding");
  insertOptionalObject(sanitized, input, "currentPlan");
  insertOptionalObject(sanitized, input, "progress");
  insertOptionalArray(sanitized, input, "planAdjustments");

  return sanitized;
}

function insertOptionalText(
  sanitized: Record<string, unknown>,
  input: object,
  key: string,
): void {
  if (!Object.prototype.hasOwnProperty.call(input, key)) {
    return;
  }
  const value = sanitizeOptionalString(
    (input as Record<string, unknown>)[key],
  );
  if (value !== undefined) {
    sanitized[key] = value;
  }
}

function insertOptionalObject(
  sanitized: Record<string, unknown>,
  input: object,
  key: string,
): void {
  if (!Object.prototype.hasOwnProperty.call(input, key)) {
    return;
  }
  const value = sanitizeOptionalJsonObject(
    (input as Record<string, unknown>)[key],
  ) as JsonObject | undefined;
  if (value !== undefined) {
    sanitized[key] = value;
  }
}

function insertOptionalArray(
  sanitized: Record<string, unknown>,
  input: object,
  key: string,
): void {
  if (!Object.prototype.hasOwnProperty.call(input, key)) {
    return;
  }
  const value = sanitizeOptionalJsonArray(
    (input as Record<string, unknown>)[key],
  ) as JsonArray | undefined;
  if (value !== undefined) {
    sanitized[key] = value;
  }
}

function sanitizeProjectSort(sort: unknown): AccountLearningProjectSort {
  if (sort === "updated" || sort === "recent") {
    return sort;
  }
  throw createAccountLearningValidationError();
}

function sanitizeIntegerRange(
  value: unknown,
  min: number,
  max: number,
): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < min ||
    value > max
  ) {
    throw createAccountLearningValidationError();
  }
  return value;
}
