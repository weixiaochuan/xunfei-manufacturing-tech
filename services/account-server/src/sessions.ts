import { createHash, randomBytes, randomUUID } from "node:crypto";
import type { Pool } from "pg";

export interface SessionUser {
  platformUserId: string;
  accountNumber: string;
  username: string;
  displayName: string | null;
  email: string | null;
}

export interface CreatedSession {
  token: string;
  user: SessionUser;
}

export interface SessionService {
  create(user: SessionUser, deviceLabel?: string | null): Promise<CreatedSession>;
  findActive(token: string): Promise<SessionUser | null>;
  revoke(token: string): Promise<void>;
}

interface SessionUserRow {
  id: string;
  account_number: string;
  casdoor_name: string;
  display_name: string | null;
  email: string | null;
}

const SESSION_TOKEN_BYTES = 32;

export function hashSessionToken(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

function mapUser(row: SessionUserRow): SessionUser {
  return {
    platformUserId: row.id,
    accountNumber: row.account_number,
    username: row.casdoor_name,
    displayName: row.display_name,
    email: row.email,
  };
}

export function createSessionService(pool: Pool, ttlSeconds: number): SessionService {
  return {
    async create(user, deviceLabel = null) {
      const token = randomBytes(SESSION_TOKEN_BYTES).toString("base64url");
      const tokenHash = hashSessionToken(token);
      const expiresAt = new Date(Date.now() + ttlSeconds * 1_000);

      await pool.query(
        `INSERT INTO user_sessions (
           id, platform_user_id, token_hash, expires_at, device_label
         ) VALUES ($1, $2, $3, $4, $5)`,
        [randomUUID(), user.platformUserId, tokenHash, expiresAt, deviceLabel],
      );

      return { token, user };
    },

    async findActive(token) {
      const result = await pool.query<SessionUserRow>(
        `UPDATE user_sessions AS session
         SET last_used_at = CURRENT_TIMESTAMP
         FROM platform_users AS platform_user
         WHERE session.token_hash = $1
           AND session.platform_user_id = platform_user.id
           AND session.revoked_at IS NULL
           AND session.expires_at > CURRENT_TIMESTAMP
         RETURNING
           platform_user.id,
           platform_user.account_number,
           platform_user.casdoor_name,
           platform_user.display_name,
           platform_user.email`,
        [hashSessionToken(token)],
      );

      return result.rowCount === 1 && result.rows[0] ? mapUser(result.rows[0]) : null;
    },

    async revoke(token) {
      await pool.query(
        `UPDATE user_sessions
         SET revoked_at = COALESCE(revoked_at, CURRENT_TIMESTAMP)
         WHERE token_hash = $1`,
        [hashSessionToken(token)],
      );
    },
  };
}
