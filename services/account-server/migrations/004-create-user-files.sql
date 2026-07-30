CREATE TABLE user_files (
  id UUID PRIMARY KEY,
  owner_user_id UUID NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  original_name TEXT NOT NULL,
  storage_key TEXT NOT NULL UNIQUE,
  mime_type TEXT,
  size_bytes BIGINT NOT NULL,
  sha256 TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMPTZ,
  CONSTRAINT user_files_original_name_not_blank CHECK (btrim(original_name) <> ''),
  CONSTRAINT user_files_size_nonnegative CHECK (size_bytes >= 0),
  CONSTRAINT user_files_sha256_format CHECK (sha256 ~ '^[0-9a-f]{64}$')
);

CREATE INDEX user_files_owner_user_id_idx
  ON user_files (owner_user_id);

CREATE INDEX user_files_owner_created_at_idx
  ON user_files (owner_user_id, created_at DESC);

CREATE INDEX user_files_storage_key_idx
  ON user_files (storage_key);
