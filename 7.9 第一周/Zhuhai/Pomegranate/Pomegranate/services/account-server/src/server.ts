import cookie from "@fastify/cookie";
import multipart from "@fastify/multipart";
import Fastify, { LogController } from "fastify";
import type { Pool } from "pg";
import type { FileStorage } from "./storage/file-storage.js";
import { LocalFilesystemStorage } from "./storage/local-filesystem-storage.js";
import {
  DesktopLoginTicketStore,
  OidcStateStore,
  registerAuthRoutes,
} from "./auth.js";
import type { AccountServerConfig } from "./config.js";
import { createOidcClient } from "./oidc.js";
import type { OidcClient } from "./oidc.js";
import {
  createPlatformUserService,
  type FindOrCreatePlatformUser,
} from "./platform-users.js";
import { createSessionService, type SessionService } from "./sessions.js";
import { registerFileRoutes } from "./files.js";
import {
  createPostgresUserFileRepository,
  createUserFileService,
  type UserFileService,
} from "./user-files.js";
import { registerDocumentRoutes } from "./document-routes.js";
import {
  createDocumentService,
  createPostgresDocumentRepository,
  type DocumentService,
} from "./documents.js";
import {
  createPostgresDocumentLibraryService,
  type DocumentLibraryService,
} from "./document-library.js";
import {
  registerLearningProjectDocumentRoutes,
  registerLearningProjectRoutes,
} from "./learning-routes.js";
import {
  createLearningProjectService,
  createPostgresLearningProjectRepository,
  type LearningProjectService,
} from "./learning-projects.js";
import {
  createLearningProjectDocumentService,
  createPostgresLearningProjectDocumentRepository,
  type LearningProjectDocumentService,
} from "./learning-project-documents.js";

const SERVICE_NAME = "pomegranate-account-server";

export interface ServerDependencies {
  pool: Pool;
  config: AccountServerConfig;
  oidcClient?: OidcClient;
  stateStore?: OidcStateStore;
  ticketStore?: DesktopLoginTicketStore;
  platformUserService?: FindOrCreatePlatformUser;
  sessionService?: SessionService;
  userFileService?: UserFileService;
  fileStorage?: FileStorage;
  documentService?: DocumentService;
  documentLibraryService?: DocumentLibraryService;
  learningProjectService?: LearningProjectService;
  learningProjectDocumentService?: LearningProjectDocumentService;
  logger?: boolean;
}

export function buildServer(dependencies: ServerDependencies) {
  const server = Fastify({
    logger: dependencies.logger === false ? false : { level: "info" },
    logController: new LogController({
      disableRequestLogging: true,
    }),
  });

  void server.register(cookie);
  void server.register(multipart, {
    limits: {
      fileSize: dependencies.config.userFiles.maxBytes + 1,
      files: 2,
      fields: 1,
    },
  });

  server.get("/health/live", async (_request, reply) => {
    server.log.info("存活检查通过");
    return reply.code(200).send({
      status: "ok",
      service: SERVICE_NAME,
    });
  });

  server.get("/health/ready", async (_request, reply) => {
    try {
      await dependencies.pool.query("SELECT 1");
      server.log.info("就绪检查通过");
      return reply.code(200).send({
        status: "ok",
        service: SERVICE_NAME,
        database: "ready",
      });
    } catch {
      server.log.warn("就绪检查失败：数据库不可用");
      return reply.code(503).send({
        status: "unavailable",
        service: SERVICE_NAME,
        database: "unavailable",
      });
    }
  });

  const sessionService = dependencies.sessionService ??
    createSessionService(dependencies.pool, dependencies.config.session.ttlSeconds);

  registerAuthRoutes(
    server,
    dependencies.config.oidc,
    dependencies.oidcClient ?? createOidcClient(dependencies.config.oidc),
    dependencies.stateStore ?? new OidcStateStore(),
    dependencies.ticketStore ?? new DesktopLoginTicketStore(),
    dependencies.platformUserService ?? createPlatformUserService(dependencies.pool),
    sessionService,
    dependencies.config.nodeEnv === "development" &&
      process.env.OIDC_DEBUG_CLAIM_TYPES === "true",
  );

  const userFileService = dependencies.userFileService ??
    createUserFileService(
      createPostgresUserFileRepository(dependencies.pool),
      dependencies.fileStorage ?? new LocalFilesystemStorage(dependencies.config.userFiles.root),
      dependencies.config.userFiles.maxBytes,
    );
  const documentService = dependencies.documentService ??
    createDocumentService(createPostgresDocumentRepository(dependencies.pool));
  const documentLibraryService = dependencies.documentLibraryService ??
    createPostgresDocumentLibraryService(dependencies.pool);
  const learningProjectService = dependencies.learningProjectService ??
    createLearningProjectService(createPostgresLearningProjectRepository(dependencies.pool));
  const learningProjectDocumentService = dependencies.learningProjectDocumentService ??
    createLearningProjectDocumentService(createPostgresLearningProjectDocumentRepository(dependencies.pool));

  registerFileRoutes(
    server,
    sessionService,
    userFileService,
    documentService,
    dependencies.config.userFiles.maxBytes,
  );
  registerDocumentRoutes(server, sessionService, documentService, documentLibraryService, userFileService);
  registerLearningProjectRoutes(server, sessionService, learningProjectService);
  registerLearningProjectDocumentRoutes(server, sessionService, learningProjectDocumentService);

  return server;
}
