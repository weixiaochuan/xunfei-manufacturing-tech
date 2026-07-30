import { invoke } from "@tauri-apps/api/core";

import { captureDocumentRequest } from "../documents/documentSession.ts";
import {
  type AccountLearningEnvelope,
  type AddAccountLearningProjectDocumentInput,
  type LearningProjectDocumentData,
  type LearningProjectDocumentsListData,
  type LearningProjectDocumentsRevisionData,
  type RemoveAccountLearningProjectDocumentInput,
  type ReorderAccountLearningProjectDocumentsInput,
  type UpdateAccountLearningProjectDocumentInput,
  createAccountLearningValidationError,
  finalizeAccountLearningEnvelope,
  parseAccountLearningEnvelope,
  parseLearningProjectDocumentData,
  parseLearningProjectDocumentsListData,
  parseLearningProjectDocumentsRevisionData,
  sanitizeDocumentImportance,
  sanitizeDocumentRole,
  sanitizeExpectedRevision,
  sanitizeOptionalSortOrder,
  sanitizeUuidInput,
} from "./accountLearningTypes.ts";

export async function listAccountLearningProjectDocuments(
  projectId: string,
): Promise<AccountLearningEnvelope<LearningProjectDocumentsListData>> {
  return invokeAccountLearningProjectDocumentsCommand(
    "account_learning_project_documents_list",
    { projectId: sanitizeUuidInput(projectId) },
    parseLearningProjectDocumentsListData,
  );
}

export async function addAccountLearningProjectDocument(
  input: AddAccountLearningProjectDocumentInput,
): Promise<AccountLearningEnvelope<LearningProjectDocumentData>> {
  return invokeAccountLearningProjectDocumentsCommand(
    "account_learning_project_document_add",
    sanitizeDocumentAddInput(input),
    parseLearningProjectDocumentData,
  );
}

export async function updateAccountLearningProjectDocument(
  input: UpdateAccountLearningProjectDocumentInput,
): Promise<AccountLearningEnvelope<LearningProjectDocumentData>> {
  return invokeAccountLearningProjectDocumentsCommand(
    "account_learning_project_document_update",
    sanitizeDocumentUpdateInput(input),
    parseLearningProjectDocumentData,
  );
}

export async function removeAccountLearningProjectDocument(
  input: RemoveAccountLearningProjectDocumentInput,
): Promise<AccountLearningEnvelope<LearningProjectDocumentsRevisionData>> {
  return invokeAccountLearningProjectDocumentsCommand(
    "account_learning_project_document_remove",
    {
      projectId: sanitizeUuidInput(input.projectId),
      documentId: sanitizeUuidInput(input.documentId),
      expectedRevision: sanitizeExpectedRevision(input.expectedRevision),
    },
    parseLearningProjectDocumentsRevisionData,
  );
}

export async function reorderAccountLearningProjectDocuments(
  input: ReorderAccountLearningProjectDocumentsInput,
): Promise<AccountLearningEnvelope<LearningProjectDocumentsRevisionData>> {
  return invokeAccountLearningProjectDocumentsCommand(
    "account_learning_project_documents_reorder",
    sanitizeDocumentsReorderInput(input),
    parseLearningProjectDocumentsRevisionData,
  );
}

async function invokeAccountLearningProjectDocumentsCommand<T>(
  command: string,
  input: Record<string, unknown>,
  parseData: (rawData: unknown) => T,
): Promise<AccountLearningEnvelope<T>> {
  const requestContext = captureDocumentRequest();
  const raw = await invoke(command, { input });
  const envelope = parseAccountLearningEnvelope(raw, parseData);
  return finalizeAccountLearningEnvelope(envelope, requestContext);
}

function sanitizeDocumentAddInput(
  input: AddAccountLearningProjectDocumentInput,
): Record<string, unknown> {
  const sanitized: Record<string, unknown> = {
    projectId: sanitizeUuidInput(input.projectId),
    expectedRevision: sanitizeExpectedRevision(input.expectedRevision),
    documentId: sanitizeUuidInput(input.documentId),
  };

  insertOptionalRole(sanitized, input);
  insertOptionalImportance(sanitized, input);
  insertOptionalSortOrder(sanitized, input);

  return sanitized;
}

function sanitizeDocumentUpdateInput(
  input: UpdateAccountLearningProjectDocumentInput,
): Record<string, unknown> {
  const sanitized: Record<string, unknown> = {
    projectId: sanitizeUuidInput(input.projectId),
    documentId: sanitizeUuidInput(input.documentId),
    expectedRevision: sanitizeExpectedRevision(input.expectedRevision),
  };

  let hasUpdate = false;
  hasUpdate = insertOptionalRole(sanitized, input) || hasUpdate;
  hasUpdate = insertOptionalImportance(sanitized, input) || hasUpdate;
  hasUpdate = insertOptionalSortOrder(sanitized, input) || hasUpdate;

  if (!hasUpdate) {
    throw createAccountLearningValidationError();
  }

  return sanitized;
}

function sanitizeDocumentsReorderInput(
  input: ReorderAccountLearningProjectDocumentsInput,
): Record<string, unknown> {
  if (!Array.isArray(input.documentIds)) {
    throw createAccountLearningValidationError();
  }
  const seen = new Set<string>();
  const documentIds = input.documentIds.map((documentId) => {
    const sanitized = sanitizeUuidInput(documentId);
    if (seen.has(sanitized)) {
      throw createAccountLearningValidationError();
    }
    seen.add(sanitized);
    return sanitized;
  });

  return {
    projectId: sanitizeUuidInput(input.projectId),
    expectedRevision: sanitizeExpectedRevision(input.expectedRevision),
    documentIds,
  };
}

function insertOptionalRole(
  sanitized: Record<string, unknown>,
  input: { role?: unknown },
): boolean {
  if (!Object.prototype.hasOwnProperty.call(input, "role")) {
    return false;
  }
  const role = sanitizeDocumentRole(input.role);
  if (role !== undefined) {
    sanitized.role = role;
    return true;
  }
  return false;
}

function insertOptionalImportance(
  sanitized: Record<string, unknown>,
  input: { importance?: unknown },
): boolean {
  if (!Object.prototype.hasOwnProperty.call(input, "importance")) {
    return false;
  }
  const importance = sanitizeDocumentImportance(input.importance);
  if (importance !== undefined) {
    sanitized.importance = importance;
    return true;
  }
  return false;
}

function insertOptionalSortOrder(
  sanitized: Record<string, unknown>,
  input: { sortOrder?: unknown },
): boolean {
  if (!Object.prototype.hasOwnProperty.call(input, "sortOrder")) {
    return false;
  }
  const sortOrder = sanitizeOptionalSortOrder(input.sortOrder);
  if (sortOrder !== undefined) {
    sanitized.sortOrder = sortOrder;
    return true;
  }
  return false;
}
