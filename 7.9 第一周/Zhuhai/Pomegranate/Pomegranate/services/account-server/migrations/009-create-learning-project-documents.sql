ALTER TABLE learning_projects
  ADD CONSTRAINT learning_projects_id_owner_user_id_unique UNIQUE (id, owner_user_id);

ALTER TABLE documents
  ADD CONSTRAINT documents_id_owner_user_id_unique UNIQUE (id, owner_user_id);

CREATE TABLE learning_project_documents (
  project_id UUID NOT NULL,
  owner_user_id UUID NOT NULL REFERENCES platform_users(id) ON DELETE RESTRICT,
  document_id UUID NOT NULL,
  role TEXT NOT NULL DEFAULT 'material',
  importance TEXT NOT NULL DEFAULT 'normal',
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (project_id, document_id),
  CONSTRAINT learning_project_documents_project_owner_fk
    FOREIGN KEY (project_id, owner_user_id)
    REFERENCES learning_projects(id, owner_user_id)
    ON DELETE CASCADE,
  CONSTRAINT learning_project_documents_document_owner_fk
    FOREIGN KEY (document_id, owner_user_id)
    REFERENCES documents(id, owner_user_id)
    ON DELETE RESTRICT,
  CONSTRAINT learning_project_documents_role_check
    CHECK (role IN ('material', 'syllabus', 'note', 'exercise', 'reference', 'other')),
  CONSTRAINT learning_project_documents_importance_check
    CHECK (importance IN ('normal', 'important', 'core')),
  CONSTRAINT learning_project_documents_sort_order_check
    CHECK (sort_order >= 0)
);

CREATE INDEX learning_project_documents_project_order_idx
  ON learning_project_documents(owner_user_id, project_id, sort_order, created_at);

CREATE INDEX learning_project_documents_document_idx
  ON learning_project_documents(owner_user_id, document_id);
