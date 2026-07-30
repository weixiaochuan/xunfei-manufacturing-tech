import assert from "node:assert/strict";
import test from "node:test";
import type { Pool } from "pg";
import {
  PlatformUserIdentityError,
  findOrCreatePlatformUser,
  type VerifiedPlatformIdentity,
} from "../src/platform-users.js";

interface FakeRow {
  id: string;
  account_number: string;
  casdoor_subject: string;
  casdoor_owner: string;
  casdoor_name: string;
  display_name: string | null;
  email: string | null;
}

function createIdentity(
  overrides: Partial<VerifiedPlatformIdentity> = {},
): VerifiedPlatformIdentity {
  return {
    subject: "subject-001",
    organization: "pomegranate",
    username: "alice",
    displayName: "Alice",
    email: "alice@example.test",
    ...overrides,
  };
}

function createFakePool() {
  const rows = new Map<string, FakeRow>();
  const statements: string[] = [];
  let sequence = 0;
  let connectCount = 0;

  const pool = {
    connect: async () => {
      connectCount += 1;
      return {
        query: async (sql: string, values?: unknown[]) => {
          statements.push(sql);
          if (!sql.includes("INSERT INTO platform_users")) {
            return { rows: [], rowCount: null };
          }

          await Promise.resolve();
          const [newId, subject, organization, username, displayName, email] =
            values as [string, string, string, string, string | null, string | null];
          const existing = rows.get(subject);
          if (existing) {
            existing.casdoor_owner = organization;
            existing.casdoor_name = username;
            existing.display_name = displayName;
            existing.email = email;
            return { rows: [{ ...existing }], rowCount: 1 };
          }

          sequence += 1;
          const row: FakeRow = {
            id: newId,
            account_number: `POME-${String(sequence).padStart(6, "0")}`,
            casdoor_subject: subject,
            casdoor_owner: organization,
            casdoor_name: username,
            display_name: displayName,
            email,
          };
          rows.set(subject, row);
          return { rows: [{ ...row }], rowCount: 1 };
        },
        release: () => undefined,
      };
    },
  } as unknown as Pool;

  return {
    pool,
    rows,
    statements,
    getConnectCount: () => connectCount,
  };
}

test("first login creates a platform user with a sequence-backed account number", async () => {
  const database = createFakePool();
  const user = await findOrCreatePlatformUser(database.pool, createIdentity());

  assert.equal(user.accountNumber, "POME-000001");
  assert.equal(database.rows.size, 1);
  const upsertSql = database.statements.find((sql) => sql.includes("INSERT INTO platform_users"));
  assert.match(upsertSql ?? "", /nextval\('platform_account_number_seq'\)/);
  assert.match(upsertSql ?? "", /ON CONFLICT \(casdoor_subject\) DO UPDATE/);
});

test("same subject reuses the same UUID and account number", async () => {
  const database = createFakePool();
  const first = await findOrCreatePlatformUser(database.pool, createIdentity());
  const second = await findOrCreatePlatformUser(database.pool, createIdentity());

  assert.equal(second.id, first.id);
  assert.equal(second.accountNumber, first.accountNumber);
  assert.equal(database.rows.size, 1);
});

test("profile changes update mutable fields without changing platform identity", async () => {
  const database = createFakePool();
  const first = await findOrCreatePlatformUser(database.pool, createIdentity());
  const second = await findOrCreatePlatformUser(
    database.pool,
    createIdentity({ username: "alice-renamed", displayName: "Alice Updated", email: null }),
  );

  assert.equal(second.id, first.id);
  assert.equal(second.accountNumber, first.accountNumber);
  assert.equal(second.username, "alice-renamed");
  assert.equal(second.displayName, "Alice Updated");
  assert.equal(second.email, null);
});

test("different subjects create different platform users", async () => {
  const database = createFakePool();
  const first = await findOrCreatePlatformUser(database.pool, createIdentity());
  const second = await findOrCreatePlatformUser(
    database.pool,
    createIdentity({ subject: "subject-002", username: "bob" }),
  );

  assert.notEqual(second.id, first.id);
  assert.notEqual(second.accountNumber, first.accountNumber);
  assert.equal(second.accountNumber, "POME-000002");
  assert.equal(database.rows.size, 2);
});

test("concurrent first logins for the same subject produce one row", async () => {
  const database = createFakePool();
  const [first, second] = await Promise.all([
    findOrCreatePlatformUser(database.pool, createIdentity()),
    findOrCreatePlatformUser(database.pool, createIdentity()),
  ]);

  assert.equal(first.id, second.id);
  assert.equal(first.accountNumber, second.accountNumber);
  assert.equal(database.rows.size, 1);
});

test("wrong organization is rejected before opening a database connection", async () => {
  const database = createFakePool();
  await assert.rejects(
    () =>
      findOrCreatePlatformUser(
        database.pool,
        createIdentity({ organization: "another-organization" }),
      ),
    PlatformUserIdentityError,
  );
  assert.equal(database.rows.size, 0);
  assert.equal(database.getConnectCount(), 0);
});

test("configured test organization can create platform users", async () => {
  const database = createFakePool();
  const user = await findOrCreatePlatformUser(
    database.pool,
    createIdentity({ organization: "pomegranate-test" }),
    "pomegranate-test",
  );

  assert.equal(user.organization, "pomegranate-test");
  assert.equal(user.accountNumber, "POME-000001");
  assert.equal(database.rows.size, 1);
});

test("blank stable subject is rejected before writing", async () => {
  const database = createFakePool();
  await assert.rejects(
    () => findOrCreatePlatformUser(database.pool, createIdentity({ subject: " " })),
    PlatformUserIdentityError,
  );
  assert.equal(database.rows.size, 0);
});
