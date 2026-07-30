CREATE TABLE learning_projects (
  id UUID PRIMARY KEY,
  owner_user_id UUID NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  name TEXT NOT NULL,
  learning_type TEXT,
  course_name TEXT,
  goal_summary TEXT,
  learning_goal JSONB NOT NULL DEFAULT '{}'::jsonb,
  understanding JSONB NOT NULL DEFAULT '{}'::jsonb,
  current_plan JSONB NOT NULL DEFAULT '{}'::jsonb,
  progress JSONB NOT NULL DEFAULT '{}'::jsonb,
  plan_adjustments JSONB NOT NULL DEFAULT '[]'::jsonb,
  data_schema_version INTEGER NOT NULL DEFAULT 1,
  revision BIGINT NOT NULL DEFAULT 1,
  last_opened_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMPTZ,
  CONSTRAINT learning_projects_name_not_blank CHECK (btrim(name) <> ''),
  CONSTRAINT learning_projects_revision_positive CHECK (revision >= 1),
  CONSTRAINT learning_projects_schema_version_positive CHECK (data_schema_version >= 1),
  CONSTRAINT learning_projects_learning_goal_object CHECK (jsonb_typeof(learning_goal) = 'object'),
  CONSTRAINT learning_projects_understanding_object CHECK (jsonb_typeof(understanding) = 'object'),
  CONSTRAINT learning_projects_current_plan_object CHECK (jsonb_typeof(current_plan) = 'object'),
  CONSTRAINT learning_projects_progress_object CHECK (jsonb_typeof(progress) = 'object'),
  CONSTRAINT learning_projects_plan_adjustments_array CHECK (jsonb_typeof(plan_adjustments) = 'array')
);

CREATE INDEX learning_projects_owner_updated_idx
  ON learning_projects (owner_user_id, updated_at DESC, id DESC)
  WHERE deleted_at IS NULL;

CREATE INDEX learning_projects_owner_recent_idx
  ON learning_projects (owner_user_id, (COALESCE(last_opened_at, updated_at)) DESC, updated_at DESC, id DESC)
  WHERE deleted_at IS NULL;
