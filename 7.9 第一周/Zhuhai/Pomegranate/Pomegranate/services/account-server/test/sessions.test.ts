import assert from "node:assert/strict";
import test from "node:test";
import type { Pool } from "pg";
import type { AccountServerConfig } from "../src/config.js";
import type { OidcClient } from "../src/oidc.js";
import { buildServer } from "../src/server.js";
import {
  createSessionService,
  type SessionService,
  type SessionUser,
} from "../src/sessions.js";

interface StoredSession {
  id: string;
  platformUserId: string;
  tokenHash: string;
  expiresAt: Date;
  lastUsedAt: Date;
  revokedAt: Date | null;
  deviceLabel: string | null;
}

const USER: SessionUser = {
  platformUserId: "9c01a82c-7260-4780-95bf-4c16e26b046e",
  accountNumber: "POME-000001",
  username: "alice",
  displayName: "Alice",
  email: "alice@example.test",
};

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

const UNUSED_OIDC = {} as OidcClient;

function createFakeDatabase() {
  const sessions = new Map<string, StoredSession>();
  const pool = {
    query: async (sql: string, values?: unknown[]) => {
      if (sql.includes("INSERT INTO user_sessions")) {
        const [id, platformUserId, tokenHash, expiresAt, deviceLabel] = values as [
          string,
          string,
          string,
          Date,
          string | null,
        ];
        sessions.set(tokenHash, {
          id,
          platformUserId,
          tokenHash,
          expiresAt,
          lastUsedAt: new Date(),
          revokedAt: null,
          deviceLabel,
        });
        return { rows: [], rowCount: 1 };
      }

      const tokenHash = values?.[0] as string;
      const stored = sessions.get(tokenHash);
      if (sql.includes("FROM platform_users")) {
        if (!stored || stored.revokedAt || stored.expiresAt.getTime() <= Date.now()) {
          return { rows: [], rowCount: 0 };
        }
        stored.lastUsedAt = new Date();
        return {
          rows: [{
            id: USER.platformUserId,
            account_number: USER.accountNumber,
            casdoor_name: USER.username,
            display_name: USER.displayName,
            email: USER.email,
          }],
          rowCount: 1,
        };
      }
      if (sql.includes("SET revoked_at")) {
        if (stored && !stored.revokedAt) {
          stored.revokedAt = new Date();
        }
        return { rows: [], rowCount: stored ? 1 : 0 };
      }
      return { rows: [{ value: 1 }], rowCount: 1 };
    },
  } as unknown as Pool;
  return { pool, sessions };
}

function createServer(pool: Pool, sessionService: SessionService) {
  return buildServer({
    pool,
    config: TEST_CONFIG,
    oidcClient: UNUSED_OIDC,
    sessionService,
    logger: false,
  });
}

test("session creation returns a 32-byte random token and stores only its hash", async () => {
  const database = createFakeDatabase();
  const service = createSessionService(database.pool, TEST_CONFIG.session.ttlSeconds);
  const first = await service.create(USER, "test-device");
  const second = await service.create(USER, "test-device");

  assert.match(first.token, /^[A-Za-z0-9_-]{43}$/);
  assert.notEqual(first.token, second.token);
  assert.equal(database.sessions.size, 2);
  for (const stored of database.sessions.values()) {
    assert.match(stored.tokenHash, /^[a-f0-9]{64}$/);
    assert.notEqual(stored.tokenHash, first.token);
    assert.notEqual(stored.tokenHash, second.token);
  }
});

test("valid, expired, and revoked sessions are handled safely", async () => {
  const database = createFakeDatabase();
  const service = createSessionService(database.pool, TEST_CONFIG.session.ttlSeconds);
  const active = await service.create(USER);
  assert.deepEqual(await service.findActive(active.token), USER);

  const expired = await service.create(USER);
  const expiredRow = [...database.sessions.values()].find(
    (row) => row.tokenHash !== [...database.sessions.values()][0]?.tokenHash,
  );
  assert.ok(expiredRow);
  expiredRow.expiresAt = new Date(0);
  assert.equal(await service.findActive(expired.token), null);

  await service.revoke(active.token);
  assert.equal(await service.findActive(active.token), null);
  assert.equal(await service.findActive("x".repeat(43)), null);
});

test("logout revokes only the presented device session and is idempotent", async (t) => {
  const database = createFakeDatabase();
  const service = createSessionService(database.pool, TEST_CONFIG.session.ttlSeconds);
  const first = await service.create(USER, "device-one");
  const second = await service.create(USER, "device-two");
  const server = createServer(database.pool, service);
  t.after(() => server.close());

  const active = await server.inject({
    method: "GET",
    url: "/auth/session",
    headers: { authorization: `Bearer ${first.token}` },
  });
  assert.equal(active.statusCode, 200);
  assert.deepEqual(active.json(), { status: "ok", user: USER });

  for (let attempt = 0; attempt < 2; attempt += 1) {
    const logout = await server.inject({
      method: "POST",
      url: "/auth/logout",
      headers: { authorization: `Bearer ${first.token}` },
    });
    assert.equal(logout.statusCode, 200);
    assert.deepEqual(logout.json(), { status: "ok" });
  }

  const revoked = await server.inject({
    method: "GET",
    url: "/auth/session",
    headers: { authorization: `Bearer ${first.token}` },
  });
  assert.equal(revoked.statusCode, 401);
  const otherDevice = await server.inject({
    method: "GET",
    url: "/auth/session",
    headers: { authorization: `Bearer ${second.token}` },
  });
  assert.equal(otherDevice.statusCode, 200);
});

test("missing and unknown bearer tokens return a minimal 401", async (t) => {
  const database = createFakeDatabase();
  const service = createSessionService(database.pool, TEST_CONFIG.session.ttlSeconds);
  const server = createServer(database.pool, service);
  t.after(() => server.close());

  for (const authorization of [undefined, `Bearer ${"z".repeat(43)}`]) {
    const response = await server.inject({
      method: "GET",
      url: "/auth/session",
      headers: authorization ? { authorization } : {},
    });
    assert.equal(response.statusCode, 401);
    assert.deepEqual(response.json(), { status: "error", error: "invalid_session" });
  }
});

test("session database errors do not leak tokens, SQL, credentials, or stacks", async (t) => {
  const sensitiveToken = "q".repeat(43);
  const failingService: SessionService = {
    create: async () => { throw new Error("SELECT password=test-password"); },
    findActive: async () => { throw new Error(`${sensitiveToken} internal-stack`); },
    revoke: async () => { throw new Error("database connection string"); },
  };
  const database = createFakeDatabase();
  const server = createServer(database.pool, failingService);
  t.after(() => server.close());

  const response = await server.inject({
    method: "GET",
    url: "/auth/session",
    headers: { authorization: `Bearer ${sensitiveToken}` },
  });
  assert.equal(response.statusCode, 503);
  assert.deepEqual(response.json(), { status: "error", error: "session_unavailable" });
  assert.doesNotMatch(response.body, /SELECT|password|internal-stack|connection|string/);
  assert.doesNotMatch(response.body, new RegExp(sensitiveToken));
});
