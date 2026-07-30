import {
  assertCurrentDocumentRequest,
  type DocumentRequestIdentity,
} from "../documents/documentSession.ts";

export const ACCOUNT_LEARNING_JS_SAFE_INTEGER_MAX = 9_007_199_254_740_991;
export const ACCOUNT_LEARNING_SORT_ORDER_MAX = 2_147_483_647;

export const LEARNING_DOCUMENT_ROLES = [
  "material",
  "syllabus",
  "note",
  "exercise",
  "reference",
  "other",
] as const;

export const LEARNING_DOCUMENT_IMPORTANCE = [
  "normal",
  "important",
  "core",
] as const;

export type JsonObject = Record<string, unknown>;
export type JsonArray = unknown[];

export type LearningProjectDocumentRole =
  (typeof LEARNING_DOCUMENT_ROLES)[number];
export type LearningProjectDocumentImportance =
  (typeof LEARNING_DOCUMENT_IMPORTANCE)[number];
export type LearningProjectDocumentStatus = "available" | "deleted";

export interface LearningProjectSummary {
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

export interface LearningProjectDetail extends LearningProjectSummary {
  learningGoal: JsonObject;
  understanding: JsonObject;
  currentPlan: JsonObject;
  progress: JsonObject;
  planAdjustments: JsonArray;
  dataSchemaVersion: number;
}

export interface LearningProjectListData {
  projects: LearningProjectSummary[];
  limit: number;
  offset: number;
}

export interface LearningProjectDeleteData {
  projectId: string;
}

export interface LearningProjectDocument {
  documentId: string;
  title: string;
  documentType: string;
  role: LearningProjectDocumentRole;
  importance: LearningProjectDocumentImportance;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
  status: LearningProjectDocumentStatus;
}

export interface LearningProjectDocumentsListData {
  projectRevision: number;
  documents: LearningProjectDocument[];
}

export interface LearningProjectDocumentData {
  projectRevision: number;
  document: LearningProjectDocument;
}

export interface LearningProjectDocumentsRevisionData {
  projectRevision: number;
}

export type AccountLearningEnvelope<T> =
  | {
      status: "completed";
      data: T;
    }
  | {
      status: "completedAccountChanged";
      data: T;
    }
  | {
      status: "accountChanged";
    };

export type AccountLearningProjectSort = "updated" | "recent";

export interface ListAccountLearningProjectsInput {
  sort?: AccountLearningProjectSort;
  limit?: number;
  offset?: number;
}

export interface CreateAccountLearningProjectInput {
  name: string;
  learningType?: string | null;
  courseName?: string | null;
  goalSummary?: string | null;
  learningGoal?: JsonObject;
  understanding?: JsonObject;
  currentPlan?: JsonObject;
  progress?: JsonObject;
  planAdjustments?: JsonArray;
}

export interface UpdateAccountLearningProjectInput {
  projectId: string;
  expectedRevision: number;
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

export interface RenameAccountLearningProjectInput {
  projectId: string;
  expectedRevision: number;
  name: string;
}

export interface LearningProjectRevisionInput {
  projectId: string;
  expectedRevision: number;
}

export interface DuplicateAccountLearningProjectInput {
  projectId: string;
  name?: string;
}

export interface AddAccountLearningProjectDocumentInput {
  projectId: string;
  expectedRevision: number;
  documentId: string;
  role?: LearningProjectDocumentRole;
  importance?: LearningProjectDocumentImportance;
  sortOrder?: number;
}

export interface UpdateAccountLearningProjectDocumentInput {
  projectId: string;
  documentId: string;
  expectedRevision: number;
  role?: LearningProjectDocumentRole;
  importance?: LearningProjectDocumentImportance;
  sortOrder?: number;
}

export interface RemoveAccountLearningProjectDocumentInput {
  projectId: string;
  documentId: string;
  expectedRevision: number;
}

export interface ReorderAccountLearningProjectDocumentsInput {
  projectId: string;
  expectedRevision: number;
  documentIds: string[];
}

export interface AccountLearningClientError {
  code: "invalidResponse" | "validation";
  message: string;
}

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function createAccountLearningInvalidResponseError(): AccountLearningClientError {
  return {
    code: "invalidResponse",
    message:
      "Account learning result could not be verified. Refresh the account learning data before deciding whether to retry.",
  };
}

export function createAccountLearningValidationError(): AccountLearningClientError {
  return {
    code: "validation",
    message: "Account learning request is invalid.",
  };
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

export function parseAccountLearningEnvelope<T>(
  raw: unknown,
  parseData: (rawData: unknown) => T,
): AccountLearningEnvelope<T> {
  const response = expectRecord(raw);
  const status = expectString(response.status);

  if (status === "accountChanged") {
    if (Object.prototype.hasOwnProperty.call(response, "data")) {
      throw createAccountLearningInvalidResponseError();
    }
    return { status: "accountChanged" };
  }

  if (status !== "completed" && status !== "completedAccountChanged") {
    throw createAccountLearningInvalidResponseError();
  }

  if (!Object.prototype.hasOwnProperty.call(response, "data")) {
    throw createAccountLearningInvalidResponseError();
  }

  return {
    status,
    data: parseData(response.data),
  };
}

export function finalizeAccountLearningEnvelope<T>(
  envelope: AccountLearningEnvelope<T>,
  requestContext: DocumentRequestIdentity,
): AccountLearningEnvelope<T> {
  if (envelope.status !== "completed") {
    return envelope;
  }

  try {
    assertCurrentDocumentRequest(requestContext);
    return envelope;
  } catch (error) {
    if (isStaleRequestError(error)) {
      return {
        status: "completedAccountChanged",
        data: envelope.data,
      };
    }
    throw error;
  }
}

export function parseLearningProjectListData(
  raw: unknown,
): LearningProjectListData {
  const data = expectRecord(raw);
  const projects = expectArray(data.projects).map(parseLearningProjectSummary);

  return {
    projects,
    limit: parseNonNegativeSafeInteger(data.limit),
    offset: parseNonNegativeSafeInteger(data.offset),
  };
}

export function parseLearningProjectSummary(
  raw: unknown,
): LearningProjectSummary {
  const project = expectRecord(raw);

  return {
    id: parseUuid(project.id),
    name: expectNonEmptyString(project.name),
    learningType: parseNullableString(project.learningType),
    courseName: parseNullableString(project.courseName),
    goalSummary: parseNullableString(project.goalSummary),
    revision: parseRevision(project.revision),
    lastOpenedAt: parseNullableString(project.lastOpenedAt),
    createdAt: expectNonEmptyString(project.createdAt),
    updatedAt: expectNonEmptyString(project.updatedAt),
  };
}

export function parseLearningProjectDetail(
  raw: unknown,
): LearningProjectDetail {
  const project = expectRecord(raw);
  const summary = parseLearningProjectSummary(project);

  return {
    ...summary,
    learningGoal: parseJsonObject(project.learningGoal),
    understanding: parseJsonObject(project.understanding),
    currentPlan: parseJsonObject(project.currentPlan),
    progress: parseJsonObject(project.progress),
    planAdjustments: parseJsonArray(project.planAdjustments),
    dataSchemaVersion: parsePositiveSafeInteger(project.dataSchemaVersion),
  };
}

export function parseLearningProjectDeleteData(
  raw: unknown,
): LearningProjectDeleteData {
  const data = expectRecord(raw);

  return {
    projectId: parseUuid(data.projectId),
  };
}

export function parseLearningProjectDocumentsListData(
  raw: unknown,
): LearningProjectDocumentsListData {
  const data = expectRecord(raw);

  return {
    projectRevision: parseRevision(data.projectRevision),
    documents: expectArray(data.documents).map(parseLearningProjectDocument),
  };
}

export function parseLearningProjectDocumentData(
  raw: unknown,
): LearningProjectDocumentData {
  const data = expectRecord(raw);

  return {
    projectRevision: parseRevision(data.projectRevision),
    document: parseLearningProjectDocument(data.document),
  };
}

export function parseLearningProjectDocumentsRevisionData(
  raw: unknown,
): LearningProjectDocumentsRevisionData {
  const data = expectRecord(raw);

  return {
    projectRevision: parseRevision(data.projectRevision),
  };
}

export function parseLearningProjectDocument(
  raw: unknown,
): LearningProjectDocument {
  const document = expectRecord(raw);

  return {
    documentId: parseUuid(document.documentId),
    title: expectNonEmptyString(document.title),
    documentType: expectNonEmptyString(document.documentType),
    role: parseDocumentRole(document.role),
    importance: parseDocumentImportance(document.importance),
    sortOrder: parseSortOrder(document.sortOrder),
    createdAt: expectNonEmptyString(document.createdAt),
    updatedAt: expectNonEmptyString(document.updatedAt),
    deletedAt: parseNullableString(document.deletedAt),
    status: parseDocumentStatus(document.status),
  };
}

export function sanitizeUuidInput(value: unknown): string {
  if (typeof value !== "string" || !UUID_PATTERN.test(value)) {
    throw createAccountLearningValidationError();
  }
  return value;
}

export function sanitizeExpectedRevision(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 1 ||
    value > ACCOUNT_LEARNING_JS_SAFE_INTEGER_MAX
  ) {
    throw createAccountLearningValidationError();
  }
  return value;
}

export function sanitizeOptionalSortOrder(value: unknown): number | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > ACCOUNT_LEARNING_SORT_ORDER_MAX
  ) {
    throw createAccountLearningValidationError();
  }
  return value;
}

export function sanitizeOptionalString(
  value: unknown,
): string | null | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (value === null) {
    return null;
  }
  if (typeof value !== "string") {
    throw createAccountLearningValidationError();
  }
  return value;
}

export function sanitizeRequiredString(value: unknown): string {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw createAccountLearningValidationError();
  }
  return value;
}

export function sanitizeOptionalJsonObject(
  value: unknown,
): JsonObject | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!isRecord(value)) {
    throw createAccountLearningValidationError();
  }
  return value;
}

export function sanitizeOptionalJsonArray(
  value: unknown,
): JsonArray | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!Array.isArray(value)) {
    throw createAccountLearningValidationError();
  }
  return value;
}

export function sanitizeDocumentRole(
  value: unknown,
): LearningProjectDocumentRole | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (isIncluded(LEARNING_DOCUMENT_ROLES, value)) {
    return value;
  }
  throw createAccountLearningValidationError();
}

export function sanitizeDocumentImportance(
  value: unknown,
): LearningProjectDocumentImportance | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (isIncluded(LEARNING_DOCUMENT_IMPORTANCE, value)) {
    return value;
  }
  throw createAccountLearningValidationError();
}

function isStaleRequestError(error: unknown): boolean {
  return (
    isRecord(error) &&
    typeof error.code === "string" &&
    error.code === "staleRequest"
  );
}

function parseDocumentRole(value: unknown): LearningProjectDocumentRole {
  if (isIncluded(LEARNING_DOCUMENT_ROLES, value)) {
    return value;
  }
  throw createAccountLearningInvalidResponseError();
}

function parseDocumentImportance(
  value: unknown,
): LearningProjectDocumentImportance {
  if (isIncluded(LEARNING_DOCUMENT_IMPORTANCE, value)) {
    return value;
  }
  throw createAccountLearningInvalidResponseError();
}

function parseDocumentStatus(value: unknown): LearningProjectDocumentStatus {
  if (value === "available" || value === "deleted") {
    return value;
  }
  throw createAccountLearningInvalidResponseError();
}

function isIncluded<const T extends readonly string[]>(
  values: T,
  value: unknown,
): value is T[number] {
  return typeof value === "string" && values.includes(value);
}

function parseUuid(value: unknown): string {
  if (typeof value !== "string" || !UUID_PATTERN.test(value)) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function parseRevision(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 1 ||
    value > ACCOUNT_LEARNING_JS_SAFE_INTEGER_MAX
  ) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function parseSortOrder(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > ACCOUNT_LEARNING_SORT_ORDER_MAX
  ) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function parsePositiveSafeInteger(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 1 ||
    value > ACCOUNT_LEARNING_JS_SAFE_INTEGER_MAX
  ) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function parseNonNegativeSafeInteger(value: unknown): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0 ||
    value > ACCOUNT_LEARNING_JS_SAFE_INTEGER_MAX
  ) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function parseJsonObject(value: unknown): JsonObject {
  if (!isRecord(value)) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function parseJsonArray(value: unknown): JsonArray {
  if (!Array.isArray(value)) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function parseNullableString(value: unknown): string | null {
  if (value === null) {
    return null;
  }
  if (typeof value === "string") {
    return value;
  }
  throw createAccountLearningInvalidResponseError();
}

function expectRecord(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function expectArray(value: unknown): unknown[] {
  if (!Array.isArray(value)) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function expectString(value: unknown): string {
  if (typeof value !== "string") {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}

function expectNonEmptyString(value: unknown): string {
  if (typeof value !== "string" || value.length === 0) {
    throw createAccountLearningInvalidResponseError();
  }
  return value;
}
