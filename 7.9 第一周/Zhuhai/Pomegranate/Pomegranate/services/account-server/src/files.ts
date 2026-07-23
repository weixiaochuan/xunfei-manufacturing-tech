import type { FastifyInstance, FastifyReply } from "fastify";
import { InvalidSessionError, requirePlatformUser } from "./authentication.js";
import type { SessionService } from "./sessions.js";
import type { DocumentService } from "./documents.js";
import { isAllowedUploadFilename } from "./file-types.js";
import {
  FileContentUnavailableError,
  FileTooLargeError,
  sanitizeMimeType,
  type UserFileService,
} from "./user-files.js";

interface ListQuery {
  limit?: string;
  offset?: string;
}

interface FileParams {
  fileId: string;
}

function sendError(reply: FastifyReply, statusCode: number, error: string) {
  return reply.code(statusCode).send({ status: "error", error });
}

function parseInteger(value: string | undefined, fallback: number, min: number, max: number) {
  if (value === undefined) {
    return fallback;
  }
  if (!/^\d+$/.test(value)) {
    return null;
  }
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= min && parsed <= max ? parsed : null;
}

function isUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value);
}

function contentDisposition(filename: string): string {
  const ascii = filename.replace(/[^\x20-\x7e]/g, "_").replace(/["\\]/g, "_");
  return `attachment; filename="${ascii || "download"}"; filename*=UTF-8''${encodeURIComponent(filename)}`;
}

async function authenticate(
  server: FastifyInstance,
  authorization: unknown,
  sessionService: SessionService,
  reply: FastifyReply,
) {
  try {
    return await requirePlatformUser(authorization, sessionService);
  } catch (error) {
    if (error instanceof InvalidSessionError) {
      sendError(reply, 401, "invalid_session");
      return null;
    }
    server.log.warn("文件接口 session 查询失败");
    sendError(reply, 503, "session_unavailable");
    return null;
  }
}

export function registerFileRoutes(
  server: FastifyInstance,
  sessionService: SessionService,
  userFiles: UserFileService,
  documents: DocumentService,
  maxBytes: number,
): void {
  server.post("/files", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessionService, reply);
    if (!user) return reply;

    let storedFile;
    try {
      if (!request.isMultipart()) {
        return sendError(reply, 400, "multipart_required");
      }
      let upload: { filename: string; mimetype: string; chunks: Buffer[] } | null = null;
      for await (const part of request.parts({
        limits: { fileSize: maxBytes + 1, files: 2, fields: 1, parts: 2 },
      })) {
        if (part.type !== "file" || part.fieldname !== "file" || upload) {
          if (part.type === "file") part.file.resume();
          return sendError(reply, 400, "invalid_file_field");
        }
        if (!isAllowedUploadFilename(part.filename)) {
          part.file.resume();
          return sendError(reply, 400, "file_type_rejected");
        }
        const chunks: Buffer[] = [];
        let size = 0;
        for await (const value of part.file) {
          const chunk = Buffer.isBuffer(value) ? value : Buffer.from(value);
          size += chunk.length;
          if (size > maxBytes) throw new FileTooLargeError();
          chunks.push(chunk);
        }
        upload = { filename: part.filename, mimetype: part.mimetype, chunks };
      }
      if (!upload) {
        return sendError(reply, 400, "invalid_file_field");
      }
      storedFile = await userFiles.upload({
        ownerUserId: user.platformUserId,
        originalName: upload.filename,
        mimeType: sanitizeMimeType(upload.mimetype),
        content: upload.chunks,
      });
      try {
        await documents.createUploadedFile(user.platformUserId, storedFile);
      } catch (error) {
        await userFiles.rollbackUpload(storedFile.id, user.platformUserId).catch(() => undefined);
        throw error;
      }
    } catch (error) {
      if (error instanceof FileTooLargeError || error instanceof server.multipartErrors.RequestFileTooLargeError) {
        return sendError(reply, 413, "file_too_large");
      }
      server.log.warn("文件上传失败");
      return sendError(reply, 503, "file_upload_unavailable");
    }

    return reply.code(201).send({ status: "ok", file: storedFile });
  });

  server.get<{ Querystring: ListQuery }>("/files", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessionService, reply);
    if (!user) return reply;
    const limit = parseInteger(request.query.limit, 50, 1, 100);
    const offset = parseInteger(request.query.offset, 0, 0, Number.MAX_SAFE_INTEGER);
    if (limit === null || offset === null) {
      return sendError(reply, 400, "invalid_pagination");
    }
    try {
      return reply.code(200).send({
        status: "ok",
        files: await userFiles.list(user.platformUserId, limit, offset),
        limit,
        offset,
      });
    } catch {
      server.log.warn("文件列表查询失败");
      return sendError(reply, 503, "files_unavailable");
    }
  });

  server.get<{ Params: FileParams }>("/files/:fileId/download", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessionService, reply);
    if (!user) return reply;
    if (!isUuid(request.params.fileId)) {
      return sendError(reply, 404, "file_not_found");
    }
    try {
      const download = await userFiles.download(request.params.fileId, user.platformUserId);
      if (!download) {
        return sendError(reply, 404, "file_not_found");
      }
      reply.header("Content-Type", "application/octet-stream");
      reply.header("Content-Disposition", contentDisposition(download.file.originalName));
      reply.header("X-Pomegranate-Content-Sha256", download.file.sha256);
      reply.header("X-Content-Type-Options", "nosniff");
      return reply.send(download.content);
    } catch (error) {
      if (error instanceof FileContentUnavailableError) {
        server.log.warn("文件内容在磁盘中不可用");
      } else {
        server.log.warn("文件下载失败");
      }
      return sendError(reply, 503, "file_content_unavailable");
    }
  });

  server.put<{ Params: FileParams }>("/files/:fileId/content", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessionService, reply);
    if (!user) return reply;
    if (!isUuid(request.params.fileId)) return sendError(reply, 404, "file_not_found");
    if (!request.isMultipart()) return sendError(reply, 400, "multipart_required");

    try {
      let upload: { filename: string; mimetype: string; chunks: Buffer[] } | null = null;
      let expectedSha256: string | null = null;
      for await (const part of request.parts({
        limits: { fileSize: maxBytes + 1, files: 1, fields: 1, parts: 2 },
      })) {
        if (part.type === "field") {
          if (part.fieldname !== "expectedSha256" || expectedSha256 !== null || typeof part.value !== "string") {
            return sendError(reply, 400, "invalid_replacement_request");
          }
          expectedSha256 = part.value.toLowerCase();
          continue;
        }
        if (part.fieldname !== "file" || upload) {
          part.file.resume();
          return sendError(reply, 400, "invalid_replacement_request");
        }
        if (!isAllowedUploadFilename(part.filename)) {
          part.file.resume();
          return sendError(reply, 400, "file_type_rejected");
        }
        const chunks: Buffer[] = [];
        let size = 0;
        for await (const value of part.file) {
          const chunk = Buffer.isBuffer(value) ? value : Buffer.from(value);
          size += chunk.length;
          if (size > maxBytes) throw new FileTooLargeError();
          chunks.push(chunk);
        }
        upload = { filename: part.filename, mimetype: part.mimetype, chunks };
      }
      if (!upload || !expectedSha256 || !/^[0-9a-f]{64}$/.test(expectedSha256)) {
        return sendError(reply, 400, "invalid_replacement_request");
      }
      const result = await userFiles.replaceContent({
        fileId: request.params.fileId,
        ownerUserId: user.platformUserId,
        expectedSha256,
        originalName: upload.filename,
        mimeType: sanitizeMimeType(upload.mimetype),
        content: upload.chunks,
      });
      if (result.status === "not_found") return sendError(reply, 404, "file_not_found");
      if (result.status === "conflict") return sendError(reply, 409, "file_conflict");
      if (result.status === "file_type_mismatch") return sendError(reply, 400, "file_type_mismatch");
      return reply.code(200).send({ status: "ok", ...result.value });
    } catch (error) {
      if (error instanceof FileTooLargeError || error instanceof server.multipartErrors.RequestFileTooLargeError) {
        return sendError(reply, 413, "file_too_large");
      }
      server.log.warn("uploaded_file 内容替换失败");
      return sendError(reply, 503, "file_replace_unavailable");
    }
  });

  server.delete<{ Params: FileParams }>("/files/:fileId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessionService, reply);
    if (!user) return reply;
    if (!isUuid(request.params.fileId)) {
      return sendError(reply, 404, "file_not_found");
    }
    try {
      const linked = await documents.deleteByFileId(request.params.fileId, user.platformUserId);
      if (linked) {
        if (linked.storageKey) await userFiles.removeStoredContent(linked.storageKey);
        return reply.code(200).send({ status: "ok" });
      }
      const deleted = await userFiles.delete(request.params.fileId, user.platformUserId);
      return deleted ? reply.code(200).send({ status: "ok" }) : sendError(reply, 404, "file_not_found");
    } catch {
      server.log.warn("文件删除失败");
      return sendError(reply, 503, "file_delete_unavailable");
    }
  });
}
