import { randomUUID } from "node:crypto";
import type { Pool, PoolClient } from "pg";

const PLATFORM_ORGANIZATION = "pomegranate";

export interface VerifiedPlatformIdentity {
  subject: string;
  organization: string;
  username: string;
  displayName: string | null;
  email: string | null;
}

export interface PlatformUser {
  id: string;
  accountNumber: string;
  casdoorSubject: string;
  organization: string;
  username: string;
  displayName: string | null;
  email: string | null;
}

interface PlatformUserRow {
  id: string;
  account_number: string;
  casdoor_subject: string;
  casdoor_owner: string;
  casdoor_name: string;
  display_name: string | null;
  email: string | null;
}

export type FindOrCreatePlatformUser = (
  identity: VerifiedPlatformIdentity,
) => Promise<PlatformUser>;

export class PlatformUserIdentityError extends Error {
  constructor() {
    super("平台用户身份无效");
    this.name = "PlatformUserIdentityError";
  }
}

function requireNonBlank(value: string): string {
  if (value.trim().length === 0) {
    throw new PlatformUserIdentityError();
  }
  return value;
}

function mapRow(row: PlatformUserRow): PlatformUser {
  return {
    id: row.id,
    accountNumber: row.account_number,
    casdoorSubject: row.casdoor_subject,
    organization: row.casdoor_owner,
    username: row.casdoor_name,
    displayName: row.display_name,
    email: row.email,
  };
}

async function upsertPlatformUser(
  client: PoolClient,
  identity: VerifiedPlatformIdentity,
): Promise<PlatformUser> {
  const result = await client.query<PlatformUserRow>(
    `
      INSERT INTO platform_users (
        id,
        casdoor_subject,
        casdoor_owner,
        casdoor_name,
        account_number,
        display_name,
        email
      )
      VALUES (
        $1,
        $2,
        $3,
        $4,
        'POME-' || lpad(nextval('platform_account_number_seq')::text, 6, '0'),
        $5,
        $6
      )
      ON CONFLICT (casdoor_subject) DO UPDATE SET
        casdoor_owner = EXCLUDED.casdoor_owner,
        casdoor_name = EXCLUDED.casdoor_name,
        display_name = EXCLUDED.display_name,
        email = EXCLUDED.email,
        updated_at = CURRENT_TIMESTAMP
      RETURNING
        id,
        account_number,
        casdoor_subject,
        casdoor_owner,
        casdoor_name,
        display_name,
        email
    `,
    [
      randomUUID(),
      identity.subject,
      identity.organization,
      identity.username,
      identity.displayName,
      identity.email,
    ],
  );

  const row = result.rows[0];
  if (!row) {
    throw new Error("平台用户写入未返回结果");
  }
  return mapRow(row);
}

export async function findOrCreatePlatformUser(
  pool: Pool,
  identity: VerifiedPlatformIdentity,
): Promise<PlatformUser> {
  if (identity.organization !== PLATFORM_ORGANIZATION) {
    throw new PlatformUserIdentityError();
  }

  const validatedIdentity: VerifiedPlatformIdentity = {
    ...identity,
    subject: requireNonBlank(identity.subject),
    username: requireNonBlank(identity.username),
  };

  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    const platformUser = await upsertPlatformUser(client, validatedIdentity);
    await client.query("COMMIT");
    return platformUser;
  } catch (error) {
    await client.query("ROLLBACK").catch(() => undefined);
    throw error;
  } finally {
    client.release();
  }
}

export function createPlatformUserService(pool: Pool): FindOrCreatePlatformUser {
  return (identity) => findOrCreatePlatformUser(pool, identity);
}
