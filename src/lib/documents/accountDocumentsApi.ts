import { invoke } from "@tauri-apps/api/core";
import {
  safeCreateMarkdownShape,
  type NormalizedCreateMarkdownInput,
} from "./createMarkdownInput";

export interface AccountDocumentFolder {
  id: string;
  name: string;
  parentId: string | null;
  folderKind: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AccountDocumentTag {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
}

export interface AccountDocumentFile {
  id: string;
  originalName: string;
  mimeType: string | null;
  sizeBytes: number;
  sha256: string;
}

export interface AccountDocument {
  id: string;
  kind: "markdown" | "uploaded_file";
  title: string;
  markdownContent: string | null;
  file: AccountDocumentFile | null;
  folder: AccountDocumentFolder | null;
  tags: AccountDocumentTag[];
  diaryDate: string | null;
  isPinned: boolean;
  isHidden: boolean;
  sortOrder: number;
  wordCount: number;
  contentSha256: string | null;
  revision: number;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface AccountDocumentFilters {
  kind?: "markdown" | "uploaded_file";
  folderId?: string;
  tagId?: string;
  diaryDate?: string;
  hidden?: boolean;
  deleted?: boolean;
  limit?: number;
  offset?: number;
}

export interface AccountMarkdownMutation {
  expectedRevision?: number;
  title?: string;
  markdownContent?: string;
  folderId?: string | null;
  diaryDate?: string | null;
  isPinned?: boolean;
  isHidden?: boolean;
  sortOrder?: number;
  tagIds?: string[];
}

export type OpenUploadedDocumentResult =
  | { status: "opened" }
  | { status: "confirmationRequired"; fileName: string };

export type ImportEditableMarkdownResult =
  | { status: "success"; document: AccountDocument }
  | { status: "cancelled" };

export interface PreparedAccountDocumentMaterial {
  content: string;
  displayName: string;
  kind: "text" | "pdf" | "excel" | "office";
  truncated: boolean;
}

export interface UploadedWorkspaceResult {
  status: "monitoring" | "modified";
  workspaceId: string;
}

export interface UploadedSyncResult {
  status: "unchanged" | "synced";
  workspaceId: string;
  file: AccountDocumentFile | null;
  documentId: string | null;
  revision: number | null;
  updatedAt: string | null;
}

export const accountDocumentsApi = {
  list: (input: AccountDocumentFilters) =>
    invoke<AccountDocument[]>("account_list_documents", { input }),
  createMarkdown: (input: NormalizedCreateMarkdownInput) => {
    if (import.meta.env.DEV) {
      console.debug("[account-documents] create shape", safeCreateMarkdownShape(input));
    }
    return invoke<AccountDocument>("account_create_markdown_document", { input });
  },
  importEditableMarkdown: () =>
    invoke<ImportEditableMarkdownResult>("account_import_markdown_file"),
  updateMarkdown: (documentId: string, input: AccountMarkdownMutation) =>
    invoke<AccountDocument>("account_update_markdown_document", { documentId, input }),
  delete: (documentId: string) => invoke<void>("account_delete_document", { documentId }),
  restore: (documentId: string) =>
    invoke<AccountDocument>("account_restore_document", { documentId }),
  listFolders: () => invoke<AccountDocumentFolder[]>("account_list_document_folders"),
  ensureLearningAssistantUploadFolder: () =>
    invoke<AccountDocumentFolder>("account_get_or_create_learning_assistant_upload_folder"),
  createFolder: (name: string, parentId: string | null) =>
    invoke<AccountDocumentFolder>("account_create_document_folder", {
      input: { name, parentId },
    }),
  updateFolder: (folderId: string, name?: string, parentId?: string | null) =>
    invoke<AccountDocumentFolder>("account_update_document_folder", {
      folderId,
      input: { name, parentId },
    }),
  deleteFolder: (folderId: string) =>
    invoke<void>("account_delete_document_folder", { folderId }),
  listTags: () => invoke<AccountDocumentTag[]>("account_list_document_tags"),
  createTag: (name: string) =>
    invoke<AccountDocumentTag>("account_create_document_tag", { input: { name } }),
  updateTag: (tagId: string, name: string) =>
    invoke<AccountDocumentTag>("account_update_document_tag", { tagId, input: { name } }),
  deleteTag: (tagId: string) => invoke<void>("account_delete_document_tag", { tagId }),
  openUploadedFile: (file: AccountDocumentFile, allowUnknown = false) =>
    invoke<OpenUploadedDocumentResult>("account_open_uploaded_document", {
      fileId: file.id,
      originalName: file.originalName,
      mimeType: file.mimeType,
      sha256: file.sha256,
      allowUnknown,
    }),
  beginUploadedEdit: (documentId: string, file: AccountDocumentFile) =>
    invoke<UploadedWorkspaceResult>("account_begin_uploaded_document_edit", {
      documentId,
      fileId: file.id,
      originalName: file.originalName,
      mimeType: file.mimeType,
      sha256: file.sha256,
    }),
  checkUploadedEdit: (workspaceId: string) =>
    invoke<UploadedWorkspaceResult>("account_check_uploaded_document_edit", { workspaceId }),
  syncUploadedEdit: (workspaceId: string) =>
    invoke<UploadedSyncResult>("account_sync_uploaded_document_edit", { workspaceId }),
  discardUploadedEdit: (workspaceId: string) =>
    invoke<UploadedWorkspaceResult>("account_discard_uploaded_document_edit", { workspaceId }),
  prepareUploadedMaterial: (file: AccountDocumentFile) =>
    invoke<PreparedAccountDocumentMaterial>("account_prepare_uploaded_document_material", {
      fileId: file.id,
      originalName: file.originalName,
      mimeType: file.mimeType,
      sha256: file.sha256,
    }),
};
