import type { FastifyInstance, FastifyReply } from "fastify";
import { InvalidSessionError, requirePlatformUser } from "./authentication.js";
import type { SessionService } from "./sessions.js";
import {
  LearningProjectValidationError,
  type LearningProjectService,
} from "./learning-projects.js";

interface ProjectParams {
  projectId: string;
}

interface ListQuery {
  sort?: string;
  limit?: string;
  offset?: string;
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
    server.log.warn("learning project session lookup failed");
    sendError(reply, 503, "session_unavailable");
    return null;
  }
}

function validationError(reply: FastifyReply, error: unknown) {
  if (error instanceof LearningProjectValidationError) {
    return sendError(reply, 400, error.message);
  }
  return null;
}

export function registerLearningProjectRoutes(
  server: FastifyInstance,
  sessions: SessionService,
  projects: LearningProjectService,
): void {
  server.get<{ Querystring: ListQuery }>("/learning/projects", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    const limit = parseInteger(request.query.limit, 50, 1, 100);
    const offset = parseInteger(request.query.offset, 0, 0, Number.MAX_SAFE_INTEGER);
    if (limit === null || offset === null) return sendError(reply, 400, "invalid_pagination");
    const sort = request.query.sort ?? "updated";
    if (sort !== "updated" && sort !== "recent") return sendError(reply, 400, "invalid_learning_project_sort");
    try {
      return reply.code(200).send({
        status: "ok",
        projects: await projects.list(user.platformUserId, { sort, limit, offset }),
        limit,
        offset,
      });
    } catch {
      server.log.warn("learning project list failed");
      return sendError(reply, 503, "learning_projects_unavailable");
    }
  });

  server.post("/learning/projects", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    try {
      return reply.code(201).send({
        status: "ok",
        project: await projects.create(user.platformUserId, request.body),
      });
    } catch (error) {
      const response = validationError(reply, error);
      if (response) return response;
      server.log.warn("learning project create failed");
      return sendError(reply, 503, "learning_project_create_unavailable");
    }
  });

  server.get<{ Params: ProjectParams }>("/learning/projects/:projectId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    if (!isUuid(request.params.projectId)) return sendError(reply, 404, "learning_project_not_found");
    try {
      const project = await projects.get(user.platformUserId, request.params.projectId);
      return project ? reply.code(200).send({ status: "ok", project }) : sendError(reply, 404, "learning_project_not_found");
    } catch {
      server.log.warn("learning project detail failed");
      return sendError(reply, 503, "learning_project_unavailable");
    }
  });

  server.patch<{ Params: ProjectParams }>("/learning/projects/:projectId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    if (!isUuid(request.params.projectId)) return sendError(reply, 404, "learning_project_not_found");
    try {
      const result = await projects.update(user.platformUserId, request.params.projectId, request.body);
      if (result.status === "not_found") return sendError(reply, 404, "learning_project_not_found");
      if (result.status === "conflict") return sendError(reply, 409, "learning_project_conflict");
      return reply.code(200).send({ status: "ok", project: result.project });
    } catch (error) {
      const response = validationError(reply, error);
      if (response) return response;
      server.log.warn("learning project update failed");
      return sendError(reply, 503, "learning_project_update_unavailable");
    }
  });

  server.patch<{ Params: ProjectParams }>("/learning/projects/:projectId/name", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    if (!isUuid(request.params.projectId)) return sendError(reply, 404, "learning_project_not_found");
    try {
      const result = await projects.rename(user.platformUserId, request.params.projectId, request.body);
      if (result.status === "not_found") return sendError(reply, 404, "learning_project_not_found");
      if (result.status === "conflict") return sendError(reply, 409, "learning_project_conflict");
      return reply.code(200).send({ status: "ok", project: result.project });
    } catch (error) {
      const response = validationError(reply, error);
      if (response) return response;
      server.log.warn("learning project rename failed");
      return sendError(reply, 503, "learning_project_rename_unavailable");
    }
  });

  server.post<{ Params: ProjectParams }>("/learning/projects/:projectId/open", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    if (!isUuid(request.params.projectId)) return sendError(reply, 404, "learning_project_not_found");
    try {
      const project = await projects.open(user.platformUserId, request.params.projectId);
      return project ? reply.code(200).send({ status: "ok", project }) : sendError(reply, 404, "learning_project_not_found");
    } catch {
      server.log.warn("learning project open failed");
      return sendError(reply, 503, "learning_project_open_unavailable");
    }
  });

  server.delete<{ Params: ProjectParams }>("/learning/projects/:projectId", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    if (!isUuid(request.params.projectId)) return sendError(reply, 404, "learning_project_not_found");
    try {
      const result = await projects.delete(user.platformUserId, request.params.projectId, request.body);
      if (result.status === "not_found") return sendError(reply, 404, "learning_project_not_found");
      if (result.status === "conflict") return sendError(reply, 409, "learning_project_conflict");
      return reply.code(200).send({ status: "ok" });
    } catch (error) {
      const response = validationError(reply, error);
      if (response) return response;
      server.log.warn("learning project delete failed");
      return sendError(reply, 503, "learning_project_delete_unavailable");
    }
  });

  server.post<{ Params: ProjectParams }>("/learning/projects/:projectId/duplicate", async (request, reply) => {
    const user = await authenticate(server, request.headers.authorization, sessions, reply);
    if (!user) return reply;
    if (!isUuid(request.params.projectId)) return sendError(reply, 404, "learning_project_not_found");
    try {
      const project = await projects.duplicate(user.platformUserId, request.params.projectId, request.body);
      return project ? reply.code(201).send({ status: "ok", project }) : sendError(reply, 404, "learning_project_not_found");
    } catch (error) {
      const response = validationError(reply, error);
      if (response) return response;
      server.log.warn("learning project duplicate failed");
      return sendError(reply, 503, "learning_project_duplicate_unavailable");
    }
  });
}
