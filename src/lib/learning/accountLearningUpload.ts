import { invoke } from "@tauri-apps/api/core";
import type { AccountUserFile } from "../api";
import {
  assertCurrentDocumentRequest,
  captureDocumentRequest,
  type DocumentRequestIdentity,
} from "../documents/documentSession.ts";

const LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND = "learning_assistant_upload" as const;

const INVALID_RESPONSE_MESSAGE =
  "上传结果无法确认，服务端可能已经保存文件，请刷新“助学模块上传”目录后再决定是否重试";

type LearningUploadStatus =
  | "cancelled"
  | "accountChanged"
  | "uploaded"
  | "uploadedAccountChanged";

export type SafeLearningMaterialFile = Pick<
  AccountUserFile,
  "id" | "originalName" | "mimeType" | "sizeBytes" | "sha256" | "createdAt"
>;

export type LearningMaterialUploadResult =
  | { status: "cancelled" }
  | { status: "accountChanged" }
  | {
      status: "uploaded";
      file: SafeLearningMaterialFile;
      documentId: string;
      folderId: string;
      folderKind: typeof LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND;
    }
  | {
      status: "uploadedAccountChanged";
      file: SafeLearningMaterialFile;
      documentId: string;
      folderId: string;
      folderKind: typeof LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND;
    };

type InvokeCommand = (command: string) => Promise<unknown>;

interface LearningUploadDependencies {
  invokeCommand: InvokeCommand;
  captureRequest: () => DocumentRequestIdentity;
  assertCurrentRequest: (identity: DocumentRequestIdentity) => void;
}

interface LearningMaterialUploadApi {
  pickAndUploadLearningMaterial(): Promise<LearningMaterialUploadResult>;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function invalidResponse(): { code: "invalidResponse"; message: string } {
  return { code: "invalidResponse", message: INVALID_RESPONSE_MESSAGE };
}

function nonEmptyString(value: unknown): string {
  if (typeof value !== "string" || value.trim() === "") throw invalidResponse();
  return value;
}

function statusOf(value: unknown): LearningUploadStatus {
  const status = nonEmptyString(value);
  if (
    status === "cancelled" ||
    status === "accountChanged" ||
    status === "uploaded" ||
    status === "uploadedAccountChanged"
  ) {
    return status;
  }
  throw invalidResponse();
}

function safeFile(value: unknown): SafeLearningMaterialFile {
  if (!isRecord(value)) throw invalidResponse();
  const sizeBytes = value.sizeBytes;
  if (typeof sizeBytes !== "number" || !Number.isFinite(sizeBytes) || sizeBytes < 0) {
    throw invalidResponse();
  }
  const mimeType = value.mimeType;
  if (mimeType !== null && typeof mimeType !== "string") throw invalidResponse();
  return {
    id: nonEmptyString(value.id),
    originalName: nonEmptyString(value.originalName),
    mimeType,
    sizeBytes,
    sha256: nonEmptyString(value.sha256),
    createdAt: nonEmptyString(value.createdAt),
  };
}

function rejectSuccessPayload(value: Record<string, unknown>): void {
  for (const key of ["file", "documentId", "folderId", "folderKind"]) {
    if (Object.prototype.hasOwnProperty.call(value, key)) throw invalidResponse();
  }
}

function parseLearningMaterialUploadResult(raw: unknown): LearningMaterialUploadResult {
  if (!isRecord(raw)) throw invalidResponse();
  const status = statusOf(raw.status);

  if (status === "cancelled" || status === "accountChanged") {
    rejectSuccessPayload(raw);
    return { status };
  }

  const folderKind = nonEmptyString(raw.folderKind);
  if (folderKind !== LEARNING_ASSISTANT_UPLOAD_FOLDER_KIND) throw invalidResponse();

  return {
    status,
    file: safeFile(raw.file),
    documentId: nonEmptyString(raw.documentId),
    folderId: nonEmptyString(raw.folderId),
    folderKind,
  };
}

function isStaleRequestError(error: unknown): boolean {
  return isRecord(error) && error.code === "staleRequest";
}

function accountChangedAfterUpload(
  result: Extract<LearningMaterialUploadResult, { status: "uploaded" }>,
): Extract<LearningMaterialUploadResult, { status: "uploadedAccountChanged" }> {
  return { ...result, status: "uploadedAccountChanged" };
}

function applyLearningUploadRequestGuard(
  result: LearningMaterialUploadResult,
  requestContext: DocumentRequestIdentity,
  assertCurrentRequest: (identity: DocumentRequestIdentity) => void = assertCurrentDocumentRequest,
): LearningMaterialUploadResult {
  if (result.status !== "uploaded") return result;
  try {
    assertCurrentRequest(requestContext);
    return result;
  } catch (error) {
    if (isStaleRequestError(error)) return accountChangedAfterUpload(result);
    throw error;
  }
}

function createLearningMaterialUploadApi(
  dependencies: Partial<LearningUploadDependencies> = {},
): LearningMaterialUploadApi {
  const invokeCommand = dependencies.invokeCommand ?? ((command) => invoke(command));
  const captureRequest = dependencies.captureRequest ?? captureDocumentRequest;
  const assertCurrentRequest = dependencies.assertCurrentRequest ?? assertCurrentDocumentRequest;

  return {
    async pickAndUploadLearningMaterial() {
      const requestContext = captureRequest();
      const raw = await invokeCommand("account_pick_and_upload_learning_material");
      const result = parseLearningMaterialUploadResult(raw);
      return applyLearningUploadRequestGuard(result, requestContext, assertCurrentRequest);
    },
  };
}

export const { pickAndUploadLearningMaterial } = createLearningMaterialUploadApi();
