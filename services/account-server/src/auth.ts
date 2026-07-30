import { randomBytes } from "node:crypto";
import type { FastifyInstance, FastifyReply } from "fastify";
import type { OidcConfig } from "./config.js";
import {
  OidcOrganizationError,
  type OidcClient,
  type OidcUserInfo,
  type VerifiedIdToken,
} from "./oidc.js";
import {
  PlatformUserIdentityError,
  type FindOrCreatePlatformUser,
} from "./platform-users.js";
import type { SessionService, SessionUser } from "./sessions.js";
import {
  InvalidSessionError,
  readBearerToken,
  requirePlatformUser,
} from "./authentication.js";

const STATE_COOKIE_NAME = "pomegranate_oidc_state";
const STATE_TTL_SECONDS = 5 * 60;
const MAX_PENDING_STATES = 1_000;
const DESKTOP_CALLBACK_URL = "pomegranate://auth/callback";
const LOGIN_TICKET_TTL_MS = 60_000;
const MAX_PENDING_TICKETS = 1_000;

type AuthClient = "browser" | "desktop";

export type DesktopLoginUser = SessionUser;

interface PendingState {
  expiresAt: number;
  client: AuthClient;
}

interface PendingTicket {
  expiresAt: number;
  user: DesktopLoginUser;
  cleanupTimer: NodeJS.Timeout;
}

export class OidcStateStore {
  private readonly states = new Map<string, PendingState>();

  issue(client: AuthClient = "browser", now = Date.now()): string {
    this.removeExpired(now);
    if (this.states.size >= MAX_PENDING_STATES) {
      const oldestState = this.states.keys().next().value;
      if (typeof oldestState === "string") {
        this.states.delete(oldestState);
      }
    }

    const state = randomBytes(32).toString("base64url");
    this.states.set(state, {
      expiresAt: now + STATE_TTL_SECONDS * 1_000,
      client,
    });
    return state;
  }

  consume(cookieState: string, returnedState: string, now = Date.now()): AuthClient | null {
    const pending = this.states.get(cookieState);
    this.states.delete(cookieState);
    return cookieState === returnedState && pending !== undefined && pending.expiresAt >= now
      ? pending.client
      : null;
  }

  private removeExpired(now: number): void {
    for (const [state, pending] of this.states) {
      if (pending.expiresAt < now) {
        this.states.delete(state);
      }
    }
  }
}

export class DesktopLoginTicketStore {
  private readonly tickets = new Map<string, PendingTicket>();

  issue(user: DesktopLoginUser, now = Date.now()): string {
    if (this.tickets.size >= MAX_PENDING_TICKETS) {
      const oldestTicket = this.tickets.keys().next().value;
      if (typeof oldestTicket === "string") {
        this.remove(oldestTicket);
      }
    }

    const ticket = randomBytes(32).toString("base64url");
    const expiresAt = now + LOGIN_TICKET_TTL_MS;
    const cleanupTimer = setTimeout(() => this.remove(ticket), LOGIN_TICKET_TTL_MS);
    cleanupTimer.unref();
    this.tickets.set(ticket, { expiresAt, user, cleanupTimer });
    return ticket;
  }

  consume(ticket: string, now = Date.now()): DesktopLoginUser | null {
    const pending = this.tickets.get(ticket);
    this.remove(ticket);
    return pending && pending.expiresAt >= now ? pending.user : null;
  }

  private remove(ticket: string): void {
    const pending = this.tickets.get(ticket);
    if (pending) {
      clearTimeout(pending.cleanupTimer);
      this.tickets.delete(ticket);
    }
  }
}

interface CallbackQuery {
  code?: string;
  state?: string;
  error?: string;
  error_description?: string;
}

interface LoginQuery {
  client?: string;
}

interface DesktopExchangeBody {
  ticket?: unknown;
}

function sendAuthError(reply: FastifyReply, statusCode: number, error: string) {
  return reply.code(statusCode).send({ status: "error", error });
}

function clearStateCookie(reply: FastifyReply): void {
  reply.clearCookie(STATE_COOKIE_NAME, { path: "/auth/callback" });
}

function readOptionalString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

export function registerAuthRoutes(
  server: FastifyInstance,
  config: OidcConfig,
  oidcClient: OidcClient,
  stateStore: OidcStateStore,
  ticketStore: DesktopLoginTicketStore,
  findOrCreatePlatformUser: FindOrCreatePlatformUser,
  sessionService: SessionService,
  claimTypeDebug: boolean,
): void {
  server.get<{ Querystring: LoginQuery }>("/auth/login", async (request, reply) => {
    if (request.query.client !== undefined && request.query.client !== "desktop") {
      return sendAuthError(reply, 400, "invalid_client");
    }
    const authClient: AuthClient = request.query.client === "desktop" ? "desktop" : "browser";
    try {
      const state = stateStore.issue(authClient);
      const authorizationUrl = await oidcClient.getAuthorizationUrl(state);
      reply.setCookie(STATE_COOKIE_NAME, state, {
        httpOnly: true,
        sameSite: "lax",
        secure: false,
        path: "/auth/callback",
        maxAge: STATE_TTL_SECONDS,
      });
      return reply.redirect(authorizationUrl.toString());
    } catch {
      server.log.warn("OIDC 登录初始化失败");
      return sendAuthError(reply, 503, "oidc_unavailable");
    }
  });

  server.get<{ Querystring: CallbackQuery }>("/auth/callback", async (request, reply) => {
    const cookieState = request.cookies[STATE_COOKIE_NAME];
    const returnedState = request.query.state;
    clearStateCookie(reply);

    const authClient =
      typeof cookieState === "string" && typeof returnedState === "string"
        ? stateStore.consume(cookieState, returnedState)
        : null;
    if (!authClient) {
      return sendAuthError(reply, 400, "invalid_state");
    }

    if (request.query.error) {
      server.log.warn("Casdoor 拒绝了授权请求");
      return sendAuthError(reply, 400, "authorization_failed");
    }

    const code = request.query.code;
    if (typeof code !== "string" || code.length === 0) {
      return sendAuthError(reply, 400, "missing_code");
    }

    let accessToken: string;
    let idToken: string;
    try {
      ({ accessToken, idToken } = await oidcClient.exchangeCode(code));
    } catch {
      server.log.warn("OIDC 令牌交换失败");
      return sendAuthError(reply, 502, "token_exchange_failed");
    }

    let identity: VerifiedIdToken;
    try {
      identity = await oidcClient.verifyIdToken(idToken);
    } catch (error) {
      if (error instanceof OidcOrganizationError) {
        if (claimTypeDebug) {
          server.log.info(
            { idTokenClaimTypes: error.claimTypes },
            "OIDC ID Token claim 名称和类型调试",
          );
        }
        server.log.warn("OIDC 用户组织不符合要求");
        return sendAuthError(reply, 403, "organization_forbidden");
      }
      server.log.warn("OIDC ID Token 验证失败");
      return sendAuthError(reply, 401, "invalid_id_token");
    }

    if (claimTypeDebug) {
      server.log.info(
        { idTokenClaimTypes: identity.claimTypes },
        "OIDC ID Token claim 名称和类型调试",
      );
    }

    let userInfo: OidcUserInfo;
    try {
      userInfo = await oidcClient.getUserInfo(accessToken);
    } catch {
      server.log.warn("OIDC UserInfo 请求失败");
      return sendAuthError(reply, 502, "userinfo_failed");
    }

    const userInfoSubject = readOptionalString(userInfo.sub);
    if (userInfoSubject !== identity.subject) {
      server.log.warn("OIDC UserInfo subject 与 ID Token 不一致");
      return sendAuthError(reply, 502, "invalid_userinfo");
    }

    let platformUser;
    try {
      platformUser = await findOrCreatePlatformUser({
        subject: identity.subject,
        organization: identity.organization,
        username: identity.username,
        displayName: identity.displayName,
        email: identity.email,
      });
    } catch (error) {
      if (error instanceof PlatformUserIdentityError) {
        server.log.warn("平台用户身份不符合要求");
        return sendAuthError(reply, 403, "organization_forbidden");
      }
      server.log.warn("平台用户数据库操作失败");
      return sendAuthError(reply, 503, "platform_user_unavailable");
    }

    const responseUser: DesktopLoginUser = {
      platformUserId: platformUser.id,
      accountNumber: platformUser.accountNumber,
      username: identity.username,
      displayName: identity.displayName,
      email: identity.email,
    };

    if (authClient === "desktop") {
      const ticket = ticketStore.issue(responseUser);
      const redirectUrl = new URL(DESKTOP_CALLBACK_URL);
      redirectUrl.searchParams.set("ticket", ticket);
      return reply.redirect(redirectUrl.toString());
    }

    return reply.code(200).send({
      status: "ok",
      organization: identity.organization,
      subject: identity.subject,
      username: responseUser.username,
      displayName: responseUser.displayName,
      email: responseUser.email,
      platformUserId: responseUser.platformUserId,
      accountNumber: responseUser.accountNumber,
    });
  });

  server.post<{ Body: DesktopExchangeBody }>("/auth/desktop/exchange", async (request, reply) => {
    const ticket = request.body?.ticket;
    if (typeof ticket !== "string" || ticket.length === 0 || ticket.length > 512) {
      return sendAuthError(reply, 400, "invalid_ticket");
    }

    const user = ticketStore.consume(ticket);
    if (!user) {
      return sendAuthError(reply, 400, "invalid_ticket");
    }

    try {
      const session = await sessionService.create(user, "Pomegranate Desktop");
      return reply.code(200).send({
        status: "ok",
        sessionToken: session.token,
        user: session.user,
      });
    } catch {
      server.log.warn("平台 session 创建失败");
      return sendAuthError(reply, 503, "session_unavailable");
    }
  });

  server.get("/auth/session", async (request, reply) => {
    try {
      const user = await requirePlatformUser(request.headers.authorization, sessionService);
      return reply.code(200).send({ status: "ok", user });
    } catch (error) {
      if (error instanceof InvalidSessionError) {
        return sendAuthError(reply, 401, "invalid_session");
      }
      server.log.warn("平台 session 查询失败");
      return sendAuthError(reply, 503, "session_unavailable");
    }
  });

  server.post("/auth/logout", async (request, reply) => {
    const token = readBearerToken(request.headers.authorization);
    if (!token) {
      return sendAuthError(reply, 401, "invalid_session");
    }

    try {
      await sessionService.revoke(token);
      return reply.code(200).send({ status: "ok" });
    } catch {
      server.log.warn("平台 session 撤销失败");
      return sendAuthError(reply, 503, "session_unavailable");
    }
  });
}
