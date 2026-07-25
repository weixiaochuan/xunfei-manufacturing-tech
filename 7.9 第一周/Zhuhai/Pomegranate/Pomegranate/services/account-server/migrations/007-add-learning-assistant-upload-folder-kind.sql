ALTER TABLE document_folders
  ADD COLUMN folder_kind TEXT,
  ADD CONSTRAINT document_folders_folder_kind_valid CHECK (
    folder_kind IS NULL OR folder_kind = 'learning_assistant_upload'
  );

CREATE UNIQUE INDEX document_folders_owner_learning_assistant_upload_kind_unique_idx
  ON document_folders (owner_user_id, folder_kind)
  WHERE folder_kind = 'learning_assistant_upload' AND deleted_at IS NULL;
