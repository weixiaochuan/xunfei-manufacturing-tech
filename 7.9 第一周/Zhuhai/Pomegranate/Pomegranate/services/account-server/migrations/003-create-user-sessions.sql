CREATE TABLE user_sessions (
  id UUID PRIMARY KEY,
  platform_user_id UUID NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  token_hash TEXT NOT NULL UNIQUE CHECK (btrim(token_hash) <> ''),
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  expires_at TIMESTAMPTZ NOT NULL,
  last_used_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  revoked_at TIMESTAMPTZ,
  device_label TEXT,
  CONSTRAINT user_sessions_expires_after_creation CHECK (expires_at > created_at)
);

CREATE INDEX user_sessions_platform_user_id_idx
  ON user_sessions (platform_user_id);

CREATE INDEX user_sessions_active_expires_idx
  ON user_sessions (expires_at)
  WHERE revoked_at IS NULL;
