import type { FastifyInstance, FastifyReply } from "fastify";
import { InvalidSessionError, requirePlatformUser } from "./authentication.js";
import {
  DocumentValidationError,
  type DocumentKind,
  type DocumentService,
} from "./documents.js";
import type { SessionService } from "./sessions.js";
import type { UserFileService } from "./user-files.js";
import {
  DocumentLibraryValidationError,
  type DocumentLibraryService,
  type MarkdownMutation,
} from "./document-library.js";

interface ListQuery {
  kind?: string;
  folderId?: string;
  tagId?: string;
  diaryDate?: string;
  hidden?: string;
  deleted?: string;
  limit?: string;
  offset?: string;
}

interface DocumentParams {
  documentId: string;
}

interface MarkdownBody {
  expectedRevision?: unknown;
  title?: unknown;
  markdownContent?: unknown;
  folderId?: unknown;
  diaryDate?: unknown;
  isPinned?: unknown;
  isHidden?: unknown;
  sortOrder?: unknown;
  tagIds?: unknown;
}

interface NamedBody { name?: unknown; parentId?: unknown }
interface FolderParams { folderId: string }
interface TagParams { tagId: string }

interface ImportBody {
  documents?: unknown;
  folders?: unknown;
  tags?: unknown;
  tagLinks?: unknown;
}

function sendError(reply: FastifyReply, statusCode: number, error: string) {
  return reply.code(statusCode).send({ status: "error", error });
}

function isUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
}

function parseInteger(value: string | undefined, fallback: number, min: number, max: number) {
  if (value === undefined) return fallback;
  if (!/^\d+$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= min && parsed <= max ? parsed : null;
}

function parseBoolean(value: string | undefined, fallback: boolean): boolean | null {
  if (value === undefined) return fallback;
  if (value === "true") return true;
  if (value === "false") return false;
  return null;
}

function mutation(body: MarkdownBody | undefined): MarkdownMutation {
  return {
    ...(body?.title !== undefined ? { title: body.title as string } : {}),
    ...(body?.markdownContent !== undefined ? { markdownContent: body.markdownContent as string } : {}),
    ...(body?.folderId !== undefined ? { folderId: body.folderId as string | null } : {}),
    ...(body?.diaryDate !== undefined ? { diaryDate: body.diaryDate as string | null } : {}),
    ...(body?.isPinned !== undefined ? { isPinned: body.isPinned as boolean } : {}),
    ...(body?.isHidden !== undefined ? { isHidden: body.isHidden as boolean } : {}),
    ...(body?.sortOrder !== undefined ? { sortOrder: body.sortOrder as number } : {}),
    ...(body?.tagIds !== undefined ? { tagIds: body.tagIds as string[] } : {}),
  };
}

async function authenticate(
  server: FastifyInstance,
  authorization: unknown,
  sessions: SessionService,
  reply: FastifyReply,
) {
  try {
    return await requirePlatformUser(authorization, sessions);
  } catch (error) {
    if (error instanceof InvalidSessionError) {
      sendError(reply, 401, "invalid_session");
      return null;
    }
    server.log.warn("统一文档接口 session 查询失败");
    sendError(reply, 503, "session_unavailable");
    return null;
  }
}

export function registerDocumentRoutes(
  server: FastifyInstance,
  sessions: SessionService,
  documents: DocumentService,
  library: DocumentLibraryService,
  userFiles: UserFileService,
): void {
  server.get<{ Querystring: ListQuery }>("/documents", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    const kind = request.query.kind;
    if (kind !== undefined && kind !== "markdown" && kind !== "uploaded_file") {
      return sendError(reply, 400, "invalid_document_kind");
    }
    const limit = parseInteger(request.query.limit, 50, 1, 100);
    const offset = parseInteger(request.query.offset, 0, 0, Number.MAX_SAFE_INTEGER);
    const hidden = parseBoolean(request.query.hidden, false);
    const deleted = parseBoolean(request.query.deleted, false);
    if (limit === null || offset === null) return sendError(reply, 400, "invalid_pagination");
    if (hidden === null || deleted === null) return sendError(reply, 400, "invalid_document_filter");
    if (request.query.folderId !== undefined && !isUuid(request.query.folderId)) return sendError(reply, 400, "invalid_folder");
    if (request.query.tagId !== undefined && !isUuid(request.query.tagId)) return sendError(reply, 400, "invalid_tag");
    if (request.query.diaryDate !== undefined && !/^\d{4}-\d{2}-\d{2}$/.test(request.query.diaryDate)) return sendError(reply, 400, "invalid_diary_date");
    try {
      const result = await library.list(user.platformUserId, {
        kind: (kind as DocumentKind | undefined) ?? null,
        folderId: request.query.folderId ?? null,
        tagId: request.query.tagId ?? null,
        diaryDate: request.query.diaryDate ?? null,
        hidden,
        deleted,
        limit,
        offset,
      });
      return reply.code(200).send({ status: "ok", documents: result, limit, offset });
    } catch {
      server.log.warn("统一文档列表查询失败");
      return sendError(reply, 503, "documents_unavailable");
    }
  });

  server.post<{ Body: MarkdownBody }>("/documents/markdown", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    try {
      const document = await library.createMarkdown(user.platformUserId, mutation(request.body));
      return reply.code(201).send({ status: "ok", document });
    } catch (error) {
      if (error instanceof DocumentValidationError || error instanceof DocumentLibraryValidationError) {
        return sendError(reply, 400, error.message);
      }
      server.log.warn("Markdown 文档创建失败");
      return sendError(reply, 503, "document_create_unavailable");
    }
  });

  server.patch<{ Params: DocumentParams; Body: MarkdownBody }>(
    "/documents/:documentId",
    async (request, reply) => {
      const user = await authenticate(server, request.headers.authorization, sessions, reply);
      if (!user) return reply;
      if (!isUuid(request.params.documentId)) return sendError(reply, 404, "document_not_found");
      try {
        const result = await library.updateMarkdown(user.platformUserId, request.params.documentId, request.body?.expectedRevision, mutation(request.body));
        if (result.status === "not_found") return sendError(reply, 404, "document_not_found");
        if (result.status === "conflict") return sendError(reply, 409, "document_conflict");
        return reply.code(200).send({ status: "ok", document: result.document });
      } catch (error) {
        if (error instanceof DocumentValidationError || error instanceof DocumentLibraryValidationError) {
          return sendError(reply, 400, error.message);
        }
        server.log.warn("Markdown 文档更新失败");
        return sendError(reply, 503, "document_update_unavailable");
      }
    },
  );

  server.post<{ Params: DocumentParams }>("/documents/:documentId/restore", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    if (!isUuid(request.params.documentId)) return sendError(reply, 404, "document_not_found");
    try {
      const result = await library.restore(user.platformUserId, request.params.documentId, async (fileId) => {
        try {
          const downloaded = await userFiles.download(fileId, user.platformUserId);
          if (!downloaded) return false;
          downloaded.content.destroy();
          return true;
        } catch {
          return false;
        }
      });
      if (result.status === "not_found") return sendError(reply, 404, "document_not_found");
      if (result.status === "file_content_unavailable") return sendError(reply, 409, "file_content_unavailable");
      return reply.code(200).send({ status: "ok", document: result.document });
    } catch {
      server.log.warn("统一文档恢复失败");
      return sendError(reply, 503, "document_restore_unavailable");
    }
  });

  server.get("/document-folders", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    try { return reply.code(200).send({ status: "ok", folders: await library.listFolders(user.platformUserId) }); }
    catch { server.log.warn("文档文件夹列表查询失败"); return sendError(reply, 503, "document_folders_unavailable"); }
  });

  server.post<{ Body: NamedBody }>("/document-folders", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply); if (!user) return reply;
    try { return reply.code(201).send({ status: "ok", folder: await library.createFolder(user.platformUserId, request.body?.name, request.body?.parentId) }); }
    catch (error) { if (error instanceof DocumentLibraryValidationError) return sendError(reply, 400, error.message); server.log.warn("文档文件夹创建失败"); return sendError(reply, 503, "document_folder_create_unavailable"); }
  });

  server.patch<{ Params: FolderParams; Body: NamedBody }>("/document-folders/:folderId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply); if (!user) return reply;
    if (!isUuid(request.params.folderId)) return sendError(reply, 404, "document_folder_not_found");
    try { const folder = await library.updateFolder(user.platformUserId, request.params.folderId, request.body?.name, request.body?.parentId); return folder ? reply.code(200).send({ status: "ok", folder }) : sendError(reply, 404, "document_folder_not_found"); }
    catch (error) { if (error instanceof DocumentLibraryValidationError) return sendError(reply, 400, error.message); server.log.warn("文档文件夹更新失败"); return sendError(reply, 503, "document_folder_update_unavailable"); }
  });

  server.delete<{ Params: FolderParams }>("/document-folders/:folderId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply); if (!user) return reply;
    if (!isUuid(request.params.folderId)) return sendError(reply, 404, "document_folder_not_found");
    try { return await library.deleteFolder(user.platformUserId, request.params.folderId) ? reply.code(200).send({ status: "ok" }) : sendError(reply, 404, "document_folder_not_found"); }
    catch { server.log.warn("文档文件夹删除失败"); return sendError(reply, 503, "document_folder_delete_unavailable"); }
  });

  server.get("/document-tags", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply); if (!user) return reply;
    try { return reply.code(200).send({ status: "ok", tags: await library.listTags(user.platformUserId) }); }
    catch { server.log.warn("文档标签列表查询失败"); return sendError(reply, 503, "document_tags_unavailable"); }
  });

  server.post<{ Body: NamedBody }>("/document-tags", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply); if (!user) return reply;
    try { return reply.code(201).send({ status: "ok", tag: await library.createTag(user.platformUserId, request.body?.name) }); }
    catch (error) { if (error instanceof DocumentLibraryValidationError) return sendError(reply, 400, error.message); server.log.warn("文档标签创建失败"); return sendError(reply, 503, "document_tag_create_unavailable"); }
  });

  server.patch<{ Params: TagParams; Body: NamedBody }>("/document-tags/:tagId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply); if (!user) return reply;
    if (!isUuid(request.params.tagId)) return sendError(reply, 404, "document_tag_not_found");
    try { const tag = await library.updateTag(user.platformUserId, request.params.tagId, request.body?.name); return tag ? reply.code(200).send({ status: "ok", tag }) : sendError(reply, 404, "document_tag_not_found"); }
    catch (error) { if (error instanceof DocumentLibraryValidationError) return sendError(reply, 400, error.message); server.log.warn("文档标签更新失败"); return sendError(reply, 503, "document_tag_update_unavailable"); }
  });

  server.delete<{ Params: TagParams }>("/document-tags/:tagId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply); if (!user) return reply;
    if (!isUuid(request.params.tagId)) return sendError(reply, 404, "document_tag_not_found");
    try { return await library.deleteTag(user.platformUserId, request.params.tagId) ? reply.code(200).send({ status: "ok" }) : sendError(reply, 404, "document_tag_not_found"); }
    catch { server.log.warn("文档标签删除失败"); return sendError(reply, 503, "document_tag_delete_unavailable"); }
  });

  server.delete<{ Params: DocumentParams }>(
    "/documents/:documentId",
    async (request, reply) => {
      const user = await authenticate(server, request.headers.authorization, sessions, reply);
      if (!user) return reply;
      if (!isUuid(request.params.documentId)) return sendError(reply, 404, "document_not_found");
      try {
        const deleted = await documents.deleteOwned(request.params.documentId, user.platformUserId);
        if (!deleted) return sendError(reply, 404, "document_not_found");
        if (deleted.storageKey) await userFiles.removeStoredContent(deleted.storageKey);
        return reply.code(200).send({ status: "ok" });
      } catch {
        server.log.warn("统一文档删除失败");
        return sendError(reply, 503, "document_delete_unavailable");
      }
    },
  );

  server.post<{ Body: ImportBody }>(
    "/documents/import-local-markdown",
    async (request, reply) => {
      const user = await authenticate(server, request.headers.authorization, sessions, reply);
      if (!user) return reply;
      try {
        const result = await documents.importLocalMarkdown(
          user.platformUserId,
          request.body?.documents,
        );
        let metadata: { folders: number; tags: number; links: number } | undefined;
        const includesMetadata = request.body?.folders !== undefined || request.body?.tags !== undefined || request.body?.tagLinks !== undefined;
        if (includesMetadata) {
          if (user.accountNumber !== "POME-000001") return sendError(reply, 403, "local_metadata_import_forbidden");
          metadata = await library.importLocalMetadata(user.platformUserId, {
            folders: request.body?.folders,
            tags: request.body?.tags,
            tagLinks: request.body?.tagLinks,
          });
        }
        return reply.code(200).send({ status: "ok", ...result, ...(metadata ? { metadata } : {}) });
      } catch (error) {
        if (error instanceof DocumentValidationError || error instanceof DocumentLibraryValidationError) {
          return sendError(reply, 400, error.message);
        }
        server.log.warn("本地 Markdown 批量导入失败");
        return sendError(reply, 503, "document_import_unavailable");
      }
    },
  );
}
