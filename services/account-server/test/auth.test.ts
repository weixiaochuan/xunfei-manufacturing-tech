import assert from "node:assert/strict";
import test from "node:test";
import type { Pool } from "pg";
import type { AccountServerConfig } from "../src/config.js";
import {
  OidcOrganizationError,
  type OidcClient,
} from "../src/oidc.js";
import type { FindOrCreatePlatformUser } from "../src/platform-users.js";
import type { SessionService } from "../src/sessions.js";
import { DesktopLoginTicketStore } from "../src/auth.js";
import { buildServer } from "../src/server.js";

const TEST_CONFIG: AccountServerConfig = {
  deploymentProfile: "local",
  server: { host: "127.0.0.1", port: 3010, publicUrl: "http://127.0.0.1:3010" },
  database: {
    host: "127.0.0.1",
    port: 5432,
    database: "pomegranate_account",
    user: "test-user",
    password: "test-password",
    connectionTimeoutMillis: 5_000,
  },
  oidc: {
    baseUrl: "http://127.0.0.1:8000",
    clientId: "test-client-id",
    clientSecret: "test-client-secret",
    redirectUri: "http://127.0.0.1:3010/auth/callback",
    organization: "pomegranate",
    application: "app-pomegranate",
  },
  session: { ttlSeconds: 7 * 24 * 60 * 60 },
  userFiles: { backend: "filesystem", root: "test-user-files", maxBytes: 1_024 },
  nodeEnv: "test",
};

const TEST_POOL = {
  query: async () => ({ rows: [{ value: 1 }] }),
} as unknown as Pool;

const TEST_PLATFORM_USER_SERVICE: FindOrCreatePlatformUser = async (identity) => ({
  id: "9c01a82c-7260-4780-95bf-4c16e26b046e",
  accountNumber: "POME-000001",
  casdoorSubject: identity.subject,
  organization: identity.organization,
  username: identity.username,
  displayName: identity.displayName,
  email: identity.email,
});

const TEST_SESSION_TOKEN = "s".repeat(43);
const TEST_SESSION_SERVICE: SessionService = {
  create: async (user) => ({ token: TEST_SESSION_TOKEN, user }),
  findActive: async () => null,
  revoke: async () => undefined,
};

function createFakeOidcClient(overrides: Partial<OidcClient> = {}): OidcClient {
  return {
    discover:
      overrides.discover ??
      (async () => ({
        issuer: "http://localhost:8000",
        authorizationEndpoint: "http://localhost:8000/login/oauth/authorize",
        tokenEndpoint: "http://localhost:8000/api/login/oauth/access_token",
        userinfoEndpoint: "http://localhost:8000/api/userinfo",
        jwksUri: "http://localhost:8000/.well-known/jwks",
      })),
    getAuthorizationUrl:
      overrides.getAuthorizationUrl ??
      (async (state) => {
        const url = new URL("http://localhost:8000/login/oauth/authorize");
        url.searchParams.set("client_id", TEST_CONFIG.oidc.clientId);
        url.searchParams.set("response_type", "code");
        url.searchParams.set("redirect_uri", TEST_CONFIG.oidc.redirectUri);
        url.searchParams.set("scope", "openid profile email");
        url.searchParams.set("state", state);
        return url;
      }),
    exchangeCode:
      overrides.exchangeCode ??
      (async () => ({
        accessToken: "test-access-token",
        idToken: "test-id-token",
      })),
    verifyIdToken:
      overrides.verifyIdToken ??
      (async () => ({
        subject: "test-subject",
        organization: "pomegranate",
        organizationClaim: "owner",
        username: "alice",
        displayName: "Alice",
        email: "alice@example.test",
        claimTypes: { sub: "string", owner: "string" },
      })),
    getUserInfo:
      overrides.getUserInfo ??
      (async () => ({
        sub: "test-subject",
        preferred_username: "alice",
        name: "Alice",
        email: "alice@example.test",
      })),
  };
}

function getCookieAndState(response: Awaited<ReturnType<ReturnType<typeof buildServer>["inject"]>>) {
  const setCookie = response.headers["set-cookie"];
  if (typeof setCookie !== "string") {
    throw new Error("测试响应缺少 state Cookie");
  }
  const cookieHeader = setCookie.split(";", 1)[0];
  assert.ok(cookieHeader);
  const location = response.headers.location;
  if (typeof location !== "string") {
    throw new Error("测试响应缺少重定向地址");
  }
  const state = new URL(location).searchParams.get("state");
  if (!state) {
    throw new Error("测试重定向地址缺少 state");
  }
  return { setCookie, cookieHeader, state, location };
}

async function login(server: ReturnType<typeof buildServer>, url = "/auth/login") {
  const response = await server.inject({ method: "GET", url });
  assert.equal(response.statusCode, 302);
  return getCookieAndState(response);
}

test("GET /auth/login redirects and sets a short-lived HttpOnly state cookie", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    platformUserService: TEST_PLATFORM_USER_SERVICE,
    logger: false,
  });
  t.after(() => server.close());

  const response = await server.inject({ method: "GET", url: "/auth/login" });
  assert.equal(response.statusCode, 302);
  const { setCookie, state, location } = getCookieAndState(response);
  assert.match(setCookie, /HttpOnly/i);
  assert.match(setCookie, /SameSite=Lax/i);
  assert.match(setCookie, /Path=\/auth\/callback/i);
  assert.match(setCookie, /Max-Age=300/i);
  assert.doesNotMatch(setCookie, /Secure/i);
  assert.ok(state.length >= 40);

  const redirect = new URL(location);
  assert.equal(redirect.origin, "http://localhost:8000");
  assert.equal(redirect.pathname, "/login/oauth/authorize");
  assert.equal(redirect.searchParams.get("response_type"), "code");
  assert.equal(redirect.searchParams.get("scope"), "openid profile email");
  assert.equal(redirect.searchParams.get("redirect_uri"), TEST_CONFIG.oidc.redirectUri);
});

test("callback without state returns 400", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    platformUserService: TEST_PLATFORM_USER_SERVICE,
    logger: false,
  });
  t.after(() => server.close());

  const response = await server.inject({
    method: "GET",
    url: "/auth/callback?code=temporary-code",
  });
  assert.equal(response.statusCode, 400);
  assert.deepEqual(response.json(), { status: "error", error: "invalid_state" });
});

test("callback with mismatched state returns 400", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader } = await login(server);

  const response = await server.inject({
    method: "GET",
    url: "/auth/callback?code=temporary-code&state=wrong-state",
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 400);
  assert.deepEqual(response.json(), { status: "error", error: "invalid_state" });
});

test("Casdoor authorization error is handled without echoing provider details", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server);

  const response = await server.inject({
    method: "GET",
    url: `/auth/callback?error=access_denied&error_description=private-detail&state=${state}`,
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 400);
  assert.deepEqual(response.json(), { status: "error", error: "authorization_failed" });
  assert.doesNotMatch(response.body, /private-detail|access_denied/);
});

test("token exchange failure does not leak the underlying error", async (t) => {
  const oidcClient = createFakeOidcClient({
    exchangeCode: async () => {
      throw new Error("test-client-secret temporary-code");
    },
  });
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient,
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server);

  const response = await server.inject({
    method: "GET",
    url: `/auth/callback?code=temporary-code&state=${state}`,
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 502);
  assert.deepEqual(response.json(), { status: "error", error: "token_exchange_failed" });
  assert.doesNotMatch(response.body, /test-client-secret|temporary-code/);
});

test("userinfo failure does not leak access tokens or internal errors", async (t) => {
  const oidcClient = createFakeOidcClient({
    getUserInfo: async () => {
      throw new Error("test-access-token internal-stack");
    },
  });
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient,
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server);

  const response = await server.inject({
    method: "GET",
    url: `/auth/callback?code=temporary-code&state=${state}`,
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 502);
  assert.deepEqual(response.json(), { status: "error", error: "userinfo_failed" });
  assert.doesNotMatch(response.body, /test-access-token|internal-stack/);
});

test("users outside pomegranate are rejected", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient({
      verifyIdToken: async () => {
        throw new OidcOrganizationError({ sub: "string", owner: "string" });
      },
    }),
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server);

  const response = await server.inject({
    method: "GET",
    url: `/auth/callback?code=temporary-code&state=${state}`,
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 403);
  assert.deepEqual(response.json(), { status: "error", error: "organization_forbidden" });
});

test("successful callback returns minimal profile and consumes state once", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    platformUserService: TEST_PLATFORM_USER_SERVICE,
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server);
  const callbackUrl = `/auth/callback?code=temporary-code&state=${state}`;

  const response = await server.inject({
    method: "GET",
    url: callbackUrl,
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 200);
  assert.deepEqual(response.json(), {
    status: "ok",
    organization: "pomegranate",
    subject: "test-subject",
    username: "alice",
    displayName: "Alice",
    email: "alice@example.test",
    platformUserId: "9c01a82c-7260-4780-95bf-4c16e26b046e",
    accountNumber: "POME-000001",
  });
  assert.doesNotMatch(response.body, /access|refresh|id_token|temporary-code/);

  const replay = await server.inject({
    method: "GET",
    url: callbackUrl,
    headers: { cookie: cookieHeader },
  });
  assert.equal(replay.statusCode, 400);
  assert.deepEqual(replay.json(), { status: "error", error: "invalid_state" });
});

test("ID Token verification failure does not leak tokens or credentials", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient({
      verifyIdToken: async () => {
        throw new Error("test-id-token test-client-secret internal-stack");
      },
    }),
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server);

  const response = await server.inject({
    method: "GET",
    url: `/auth/callback?code=temporary-code&state=${state}`,
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 401);
  assert.deepEqual(response.json(), {
    status: "error",
    error: "invalid_id_token",
  });
  assert.doesNotMatch(
    response.body,
    /test-id-token|test-client-secret|internal-stack|temporary-code/,
  );
});

test("desktop login redirects with a one-time ticket bound to the platform user", async (t) => {
  const ticketStore = new DesktopLoginTicketStore();
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    platformUserService: TEST_PLATFORM_USER_SERVICE,
    ticketStore,
    sessionService: TEST_SESSION_SERVICE,
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server, "/auth/login?client=desktop");

  const callback = await server.inject({
    method: "GET",
    url: `/auth/callback?code=temporary-code&state=${state}`,
    headers: { cookie: cookieHeader },
  });
  assert.equal(callback.statusCode, 302);
  const location = callback.headers.location;
  assert.equal(typeof location, "string");
  const redirect = new URL(location as string);
  assert.equal(redirect.protocol, "pomegranate:");
  assert.equal(redirect.hostname, "auth");
  assert.equal(redirect.pathname, "/callback");
  assert.deepEqual([...redirect.searchParams.keys()], ["ticket"]);
  const ticket = redirect.searchParams.get("ticket");
  assert.ok(ticket);
  assert.ok(ticket.length >= 43);

  const exchange = await server.inject({
    method: "POST",
    url: "/auth/desktop/exchange",
    payload: { ticket },
  });
  assert.equal(exchange.statusCode, 200);
  assert.deepEqual(exchange.json(), {
    status: "ok",
    sessionToken: TEST_SESSION_TOKEN,
    user: {
      platformUserId: "9c01a82c-7260-4780-95bf-4c16e26b046e",
      accountNumber: "POME-000001",
      username: "alice",
      displayName: "Alice",
      email: "alice@example.test",
    },
  });
  assert.doesNotMatch(exchange.body, /access_token|refresh_token|id_token|subject/);

  const replay = await server.inject({
    method: "POST",
    url: "/auth/desktop/exchange",
    payload: { ticket },
  });
  assert.equal(replay.statusCode, 400);
  assert.deepEqual(replay.json(), { status: "error", error: "invalid_ticket" });
  assert.doesNotMatch(replay.body, new RegExp(ticket));
});

test("desktop tickets use at least 32 random bytes", () => {
  const ticketStore = new DesktopLoginTicketStore();
  const first = ticketStore.issue({
    platformUserId: "platform-user-1",
    accountNumber: "POME-000001",
    username: "alice",
    displayName: null,
    email: null,
  });
  const second = ticketStore.issue({
    platformUserId: "platform-user-1",
    accountNumber: "POME-000001",
    username: "alice",
    displayName: null,
    email: null,
  });
  assert.notEqual(first, second);
  assert.match(first, /^[A-Za-z0-9_-]{43}$/);
  assert.match(second, /^[A-Za-z0-9_-]{43}$/);
});

test("expired and unknown desktop tickets are rejected", async (t) => {
  const ticketStore = new DesktopLoginTicketStore();
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    ticketStore,
    logger: false,
  });
  t.after(() => server.close());
  const expired = ticketStore.issue({
    platformUserId: "platform-user-1",
    accountNumber: "POME-000001",
    username: "alice",
    displayName: null,
    email: null,
  }, 0);

  assert.equal(ticketStore.consume(expired, 60_001), null);
  const unknown = await server.inject({
    method: "POST",
    url: "/auth/desktop/exchange",
    payload: { ticket: "unknown-ticket-value" },
  });
  assert.equal(unknown.statusCode, 400);
  assert.deepEqual(unknown.json(), { status: "error", error: "invalid_ticket" });
  assert.doesNotMatch(unknown.body, /unknown-ticket-value/);
});

test("desktop exchange validation errors do not expose internal details", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    logger: false,
  });
  t.after(() => server.close());

  const response = await server.inject({
    method: "POST",
    url: "/auth/desktop/exchange",
    payload: { ticket: "test-access-token SELECT password=test-password" },
  });
  assert.equal(response.statusCode, 400);
  assert.deepEqual(response.json(), { status: "error", error: "invalid_ticket" });
  assert.doesNotMatch(response.body, /access-token|SELECT|test-password/);
});

test("login rejects unsupported client modes without accepting a callback URL", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    logger: false,
  });
  t.after(() => server.close());

  const response = await server.inject({
    method: "GET",
    url: "/auth/login?client=desktop-other&callback=https://untrusted.example",
  });
  assert.equal(response.statusCode, 400);
  assert.deepEqual(response.json(), { status: "error", error: "invalid_client" });
  assert.doesNotMatch(response.body, /untrusted/);
});

test("platform user database failure returns a safe error", async (t) => {
  const server = buildServer({
    pool: TEST_POOL,
    config: TEST_CONFIG,
    oidcClient: createFakeOidcClient(),
    platformUserService: async () => {
      throw new Error("SELECT secret_column password=test-password internal-stack");
    },
    logger: false,
  });
  t.after(() => server.close());
  const { cookieHeader, state } = await login(server);

  const response = await server.inject({
    method: "GET",
    url: `/auth/callback?code=temporary-code&state=${state}`,
    headers: { cookie: cookieHeader },
  });
  assert.equal(response.statusCode, 503);
  assert.deepEqual(response.json(), {
    status: "error",
    error: "platform_user_unavailable",
  });
  assert.doesNotMatch(
    response.body,
    /SELECT|secret_column|test-password|internal-stack|temporary-code/,
  );
});
