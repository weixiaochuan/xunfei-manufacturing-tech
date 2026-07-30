CREATE TABLE documents (
  id UUID PRIMARY KEY,
  owner_user_id UUID NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  document_kind TEXT NOT NULL,
  title TEXT NOT NULL,
  markdown_content TEXT,
  user_file_id UUID REFERENCES user_files(id) ON DELETE RESTRICT,
  source_local_document_id TEXT,
  legacy_metadata JSONB,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMPTZ,
  CONSTRAINT documents_kind_valid
    CHECK (document_kind IN ('markdown', 'uploaded_file')),
  CONSTRAINT documents_title_not_blank
    CHECK (btrim(title) <> ''),
  CONSTRAINT documents_source_local_id_not_blank
    CHECK (source_local_document_id IS NULL OR btrim(source_local_document_id) <> ''),
  CONSTRAINT documents_kind_payload_valid CHECK (
    (document_kind = 'markdown' AND markdown_content IS NOT NULL AND user_file_id IS NULL)
    OR
    (document_kind = 'uploaded_file' AND markdown_content IS NULL AND user_file_id IS NOT NULL)
  )
);

CREATE INDEX documents_owner_user_id_idx
  ON documents (owner_user_id);

CREATE INDEX documents_owner_updated_at_idx
  ON documents (owner_user_id, updated_at DESC);

CREATE INDEX documents_owner_kind_idx
  ON documents (owner_user_id, document_kind);

CREATE UNIQUE INDEX documents_user_file_id_unique_idx
  ON documents (user_file_id)
  WHERE user_file_id IS NOT NULL;

CREATE UNIQUE INDEX documents_owner_source_local_unique_idx
  ON documents (owner_user_id, source_local_document_id)
  WHERE source_local_document_id IS NOT NULL;

INSERT INTO documents (
  id,
  owner_user_id,
  document_kind,
  title,
  markdown_content,
  user_file_id,
  created_at,
  updated_at,
  deleted_at
)
SELECT
  id,
  owner_user_id,
  'uploaded_file',
  original_name,
  NULL,
  id,
  created_at,
  created_at,
  deleted_at
FROM user_files
ON CONFLICT (user_file_id) WHERE user_file_id IS NOT NULL DO NOTHING;
