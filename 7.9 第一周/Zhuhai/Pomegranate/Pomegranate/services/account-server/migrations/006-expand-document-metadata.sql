CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE document_folders (
  id UUID PRIMARY KEY,
  owner_user_id UUID NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  name TEXT NOT NULL,
  parent_id UUID REFERENCES document_folders(id) ON DELETE SET NULL,
  source_local_folder_id TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMPTZ,
  CONSTRAINT document_folders_name_not_blank CHECK (btrim(name) <> ''),
  CONSTRAINT document_folders_source_not_blank CHECK (
    source_local_folder_id IS NULL OR btrim(source_local_folder_id) <> ''
  ),
  CONSTRAINT document_folders_not_self_parent CHECK (parent_id IS NULL OR parent_id <> id)
);

CREATE UNIQUE INDEX document_folders_owner_source_unique_idx
  ON document_folders (owner_user_id, source_local_folder_id)
  WHERE source_local_folder_id IS NOT NULL;
CREATE INDEX document_folders_owner_parent_idx
  ON document_folders (owner_user_id, parent_id)
  WHERE deleted_at IS NULL;

CREATE OR REPLACE FUNCTION prevent_document_folder_cycle()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  parent_owner UUID;
  cycle_found BOOLEAN;
BEGIN
  IF NEW.parent_id IS NULL THEN
    RETURN NEW;
  END IF;

  SELECT owner_user_id INTO parent_owner
  FROM document_folders
  WHERE id = NEW.parent_id AND deleted_at IS NULL;

  IF parent_owner IS NULL OR parent_owner <> NEW.owner_user_id THEN
    RAISE EXCEPTION 'invalid_document_folder_parent';
  END IF;

  WITH RECURSIVE ancestors(id, parent_id) AS (
    SELECT id, parent_id FROM document_folders WHERE id = NEW.parent_id
    UNION ALL
    SELECT folder.id, folder.parent_id
    FROM document_folders folder
    JOIN ancestors ON folder.id = ancestors.parent_id
  )
  SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = NEW.id) INTO cycle_found;

  IF cycle_found THEN
    RAISE EXCEPTION 'document_folder_cycle';
  END IF;
  RETURN NEW;
END;
$$;

CREATE TRIGGER document_folders_prevent_cycle
BEFORE INSERT OR UPDATE OF parent_id, owner_user_id ON document_folders
FOR EACH ROW EXECUTE FUNCTION prevent_document_folder_cycle();

CREATE TABLE document_tags (
  id UUID PRIMARY KEY,
  owner_user_id UUID NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  name TEXT NOT NULL,
  source_local_tag_id TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMPTZ,
  CONSTRAINT document_tags_name_not_blank CHECK (btrim(name) <> ''),
  CONSTRAINT document_tags_source_not_blank CHECK (
    source_local_tag_id IS NULL OR btrim(source_local_tag_id) <> ''
  )
);

CREATE UNIQUE INDEX document_tags_owner_name_unique_idx
  ON document_tags (owner_user_id, lower(name))
  WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX document_tags_owner_source_unique_idx
  ON document_tags (owner_user_id, source_local_tag_id)
  WHERE source_local_tag_id IS NOT NULL;
CREATE INDEX document_tags_owner_updated_idx
  ON document_tags (owner_user_id, updated_at DESC)
  WHERE deleted_at IS NULL;

ALTER TABLE documents
  ADD COLUMN folder_id UUID REFERENCES document_folders(id) ON DELETE SET NULL,
  ADD COLUMN diary_date DATE,
  ADD COLUMN is_pinned BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN is_hidden BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN content_sha256 TEXT,
  ADD COLUMN revision BIGINT NOT NULL DEFAULT 1;

CREATE TABLE document_tag_links (
  document_id UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  tag_id UUID NOT NULL REFERENCES document_tags(id) ON DELETE CASCADE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (document_id, tag_id)
);

CREATE INDEX document_tag_links_tag_document_idx
  ON document_tag_links (tag_id, document_id);

UPDATE documents
SET
  diary_date = CASE
    WHEN legacy_metadata->>'isDaily' = 'true'
      AND legacy_metadata->>'dailyDate' ~ '^\d{4}-\d{2}-\d{2}$'
    THEN (legacy_metadata->>'dailyDate')::date
    ELSE NULL
  END,
  is_pinned = COALESCE((legacy_metadata->>'isPinned')::boolean, FALSE),
  is_hidden = COALESCE((legacy_metadata->>'isHidden')::boolean, FALSE),
  sort_order = CASE
    WHEN legacy_metadata->>'sortOrder' ~ '^-?\d+$'
    THEN (legacy_metadata->>'sortOrder')::integer
    ELSE 0
  END,
  word_count = char_length(replace(markdown_content, ' ', '')),
  content_sha256 = encode(digest(markdown_content, 'sha256'), 'hex'),
  revision = 1
WHERE document_kind = 'markdown';

WITH target_owner AS (
  SELECT id FROM platform_users WHERE account_number = 'POME-000001'
), local_folders AS (
  SELECT DISTINCT
    d.owner_user_id,
    'sqlite-folder:' || (d.legacy_metadata->'folder'->>'id') AS source_id,
    btrim(d.legacy_metadata->'folder'->>'name') AS name
  FROM documents d
  JOIN target_owner owner ON owner.id = d.owner_user_id
  WHERE d.document_kind = 'markdown'
    AND d.source_local_document_id IS NOT NULL
    AND jsonb_typeof(d.legacy_metadata->'folder') = 'object'
    AND d.legacy_metadata->'folder'->>'id' ~ '^\d+$'
    AND btrim(COALESCE(d.legacy_metadata->'folder'->>'name', '')) <> ''
)
INSERT INTO document_folders (id, owner_user_id, name, source_local_folder_id)
SELECT gen_random_uuid(), owner_user_id, name, source_id
FROM local_folders
ON CONFLICT (owner_user_id, source_local_folder_id)
  WHERE source_local_folder_id IS NOT NULL
DO UPDATE SET name = EXCLUDED.name, updated_at = CURRENT_TIMESTAMP, deleted_at = NULL;

WITH target_owner AS (
  SELECT id FROM platform_users WHERE account_number = 'POME-000001'
)
UPDATE documents d
SET folder_id = folder.id
FROM target_owner owner, document_folders folder
WHERE d.owner_user_id = owner.id
  AND d.document_kind = 'markdown'
  AND d.source_local_document_id IS NOT NULL
  AND folder.owner_user_id = d.owner_user_id
  AND folder.source_local_folder_id = 'sqlite-folder:' || (d.legacy_metadata->'folder'->>'id');

WITH target_owner AS (
  SELECT id FROM platform_users WHERE account_number = 'POME-000001'
), local_tags AS (
  SELECT DISTINCT d.owner_user_id, btrim(tag->>'name') AS name
  FROM documents d
  JOIN target_owner owner ON owner.id = d.owner_user_id
  CROSS JOIN LATERAL jsonb_array_elements(
    CASE WHEN jsonb_typeof(d.legacy_metadata->'tags') = 'array'
      THEN d.legacy_metadata->'tags' ELSE '[]'::jsonb END
  ) tag
  WHERE d.document_kind = 'markdown'
    AND d.source_local_document_id IS NOT NULL
    AND btrim(COALESCE(tag->>'name', '')) <> ''
)
INSERT INTO document_tags (id, owner_user_id, name)
SELECT gen_random_uuid(), owner_user_id, name FROM local_tags
ON CONFLICT (owner_user_id, lower(name)) WHERE deleted_at IS NULL
DO UPDATE SET updated_at = CURRENT_TIMESTAMP;

WITH target_owner AS (
  SELECT id FROM platform_users WHERE account_number = 'POME-000001'
), local_links AS (
  SELECT d.id AS document_id, d.owner_user_id, lower(btrim(tag->>'name')) AS tag_name
  FROM documents d
  JOIN target_owner owner ON owner.id = d.owner_user_id
  CROSS JOIN LATERAL jsonb_array_elements(
    CASE WHEN jsonb_typeof(d.legacy_metadata->'tags') = 'array'
      THEN d.legacy_metadata->'tags' ELSE '[]'::jsonb END
  ) tag
  WHERE d.document_kind = 'markdown'
    AND d.source_local_document_id IS NOT NULL
    AND btrim(COALESCE(tag->>'name', '')) <> ''
)
INSERT INTO document_tag_links (document_id, tag_id)
SELECT link.document_id, tag.id
FROM local_links link
JOIN document_tags tag
  ON tag.owner_user_id = link.owner_user_id
  AND lower(tag.name) = link.tag_name
  AND tag.deleted_at IS NULL
ON CONFLICT DO NOTHING;

ALTER TABLE documents
  ADD CONSTRAINT documents_word_count_nonnegative CHECK (word_count >= 0),
  ADD CONSTRAINT documents_revision_positive CHECK (revision > 0),
  ADD CONSTRAINT documents_content_sha256_valid CHECK (
    (document_kind = 'markdown' AND content_sha256 ~ '^[0-9a-f]{64}$')
    OR (document_kind = 'uploaded_file' AND content_sha256 IS NULL)
  );

CREATE INDEX documents_owner_folder_idx
  ON documents (owner_user_id, folder_id, updated_at DESC);
CREATE INDEX documents_owner_diary_idx
  ON documents (owner_user_id, diary_date, updated_at DESC)
  WHERE diary_date IS NOT NULL;
CREATE INDEX documents_owner_hidden_idx
  ON documents (owner_user_id, is_hidden, updated_at DESC);
CREATE INDEX documents_owner_deleted_idx
  ON documents (owner_user_id, deleted_at, updated_at DESC);
CREATE INDEX documents_owner_catalog_order_idx
  ON documents (owner_user_id, is_pinned DESC, sort_order, updated_at DESC);
