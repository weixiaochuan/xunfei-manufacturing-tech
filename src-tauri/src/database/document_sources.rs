use rusqlite::{params, params_from_iter, OptionalExtension, Transaction};

use crate::error::AppError;
use crate::services::document_sources::{
    DocumentSource, DocumentSourceListInput, NewDocumentSource, CATEGORY_LEARNING_UPLOAD,
    MODULE_LEARNING_ASSISTANT,
};

use super::Database;

impl Database {
    pub fn list_document_sources(
        &self,
        input: &DocumentSourceListInput,
    ) -> Result<Vec<DocumentSource>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|error| AppError::Custom(error.to_string()))?;
        let mut statement = conn.prepare(
            "SELECT id, display_name, original_file_name, stored_relative_path, file_extension,
                    mime_type, category, source_module, is_builtin, is_enabled, file_size,
                    checksum, created_at, updated_at
             FROM document_sources
             WHERE (?1 IS NULL OR category = ?1)
               AND (?2 IS NULL OR source_module = ?2)
               AND (?3 IS NULL OR file_extension = ?3)
             ORDER BY is_builtin DESC, created_at ASC, id ASC",
        )?;
        let rows = statement.query_map(
            params![input.category, input.source_module, input.file_extension],
            map_document_source,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn get_document_source(&self, id: i64) -> Result<Option<DocumentSource>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|error| AppError::Custom(error.to_string()))?;
        conn.query_row(
            "SELECT id, display_name, original_file_name, stored_relative_path, file_extension,
                    mime_type, category, source_module, is_builtin, is_enabled, file_size,
                    checksum, created_at, updated_at FROM document_sources WHERE id = ?1",
            [id],
            map_document_source,
        )
        .optional()
        .map_err(AppError::from)
    }

    pub fn get_document_sources_by_ids(
        &self,
        ids: &[i64],
    ) -> Result<Vec<DocumentSource>, AppError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|error| AppError::Custom(error.to_string()))?;
        let placeholders = (1..=ids.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, display_name, original_file_name, stored_relative_path, file_extension,
                    mime_type, category, source_module, is_builtin, is_enabled, file_size,
                    checksum, created_at, updated_at FROM document_sources WHERE id IN ({placeholders})"
        );
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(ids.iter()), map_document_source)?;
        let mut result = rows.collect::<Result<Vec<_>, _>>()?;
        result.sort_by_key(|source| {
            ids.iter()
                .position(|id| *id == source.id)
                .unwrap_or(usize::MAX)
        });
        Ok(result)
    }

    pub fn upsert_document_source(
        &self,
        input: &NewDocumentSource,
    ) -> Result<DocumentSource, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|error| AppError::Custom(error.to_string()))?;
        conn.execute(
            "INSERT INTO document_sources
                (display_name, original_file_name, stored_relative_path, file_extension, mime_type,
                 category, source_module, is_builtin, is_enabled, file_size, checksum)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10)
             ON CONFLICT(source_module, category, original_file_name, checksum) DO UPDATE SET
                display_name=excluded.display_name, stored_relative_path=excluded.stored_relative_path,
                file_extension=excluded.file_extension, mime_type=excluded.mime_type,
                file_size=excluded.file_size, is_enabled=1, updated_at=datetime('now','localtime')",
            params![input.display_name, input.original_file_name, input.stored_relative_path,
                input.file_extension, input.mime_type, input.category, input.source_module,
                input.is_builtin, input.file_size, input.checksum],
        )?;
        conn.query_row(
            "SELECT id, display_name, original_file_name, stored_relative_path, file_extension,
                    mime_type, category, source_module, is_builtin, is_enabled, file_size,
                    checksum, created_at, updated_at FROM document_sources
             WHERE source_module=?1 AND category=?2 AND original_file_name=?3 AND checksum=?4",
            params![
                input.source_module,
                input.category,
                input.original_file_name,
                input.checksum
            ],
            map_document_source,
        )
        .map_err(AppError::from)
    }

    pub fn upsert_learning_upload_document_source(
        &self,
        input: &NewDocumentSource,
    ) -> Result<DocumentSource, AppError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|error| AppError::Custom(error.to_string()))?;
        let tx = conn.transaction()?;
        let source = upsert_document_source_tx(&tx, input)?;
        ensure_learning_upload_note_tx(&tx, input)?;
        tx.commit()?;
        Ok(source)
    }

    pub fn repair_learning_upload_document_notes(&self) -> Result<usize, AppError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|error| AppError::Custom(error.to_string()))?;
        let tx = conn.transaction()?;
        let sources = {
            let mut statement = tx.prepare(
                "SELECT display_name, original_file_name, stored_relative_path, file_extension,
                        mime_type, category, source_module, is_builtin, file_size, checksum
                 FROM document_sources
                 WHERE source_module = ?1 AND category = ?2 AND is_enabled = 1",
            )?;
            let rows = statement.query_map(
                params![MODULE_LEARNING_ASSISTANT, CATEGORY_LEARNING_UPLOAD],
                |row| {
                    Ok(NewDocumentSource {
                        display_name: row.get(0)?,
                        original_file_name: row.get(1)?,
                        stored_relative_path: row.get(2)?,
                        file_extension: row.get(3)?,
                        mime_type: row.get(4)?,
                        category: row.get(5)?,
                        source_module: row.get(6)?,
                        is_builtin: row.get(7)?,
                        file_size: row.get(8)?,
                        checksum: row.get(9)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for source in &sources {
            ensure_learning_upload_note_tx(&tx, source)?;
        }
        tx.commit()?;
        Ok(sources.len())
    }

    pub fn delete_document_source_record(&self, id: i64) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|error| AppError::Custom(error.to_string()))?;
        conn.execute("DELETE FROM document_sources WHERE id=?1", [id])?;
        Ok(())
    }
}

fn map_document_source(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentSource> {
    Ok(DocumentSource {
        id: row.get(0)?,
        display_name: row.get(1)?,
        original_file_name: row.get(2)?,
        stored_relative_path: row.get(3)?,
        file_extension: row.get(4)?,
        mime_type: row.get(5)?,
        category: row.get(6)?,
        source_module: row.get(7)?,
        is_builtin: row.get(8)?,
        is_enabled: row.get(9)?,
        file_size: row.get(10)?,
        checksum: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        is_available: false,
    })
}

fn upsert_document_source_tx(
    tx: &Transaction<'_>,
    input: &NewDocumentSource,
) -> Result<DocumentSource, AppError> {
    tx.execute(
        "INSERT INTO document_sources
            (display_name, original_file_name, stored_relative_path, file_extension, mime_type,
             category, source_module, is_builtin, is_enabled, file_size, checksum)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10)
         ON CONFLICT(source_module, category, original_file_name, checksum) DO UPDATE SET
            display_name=excluded.display_name, stored_relative_path=excluded.stored_relative_path,
            file_extension=excluded.file_extension, mime_type=excluded.mime_type,
            file_size=excluded.file_size, is_enabled=1, updated_at=datetime('now','localtime')",
        params![
            input.display_name,
            input.original_file_name,
            input.stored_relative_path,
            input.file_extension,
            input.mime_type,
            input.category,
            input.source_module,
            input.is_builtin,
            input.file_size,
            input.checksum
        ],
    )?;
    tx.query_row(
        "SELECT id, display_name, original_file_name, stored_relative_path, file_extension,
                mime_type, category, source_module, is_builtin, is_enabled, file_size,
                checksum, created_at, updated_at FROM document_sources
         WHERE source_module=?1 AND category=?2 AND original_file_name=?3 AND checksum=?4",
        params![
            input.source_module,
            input.category,
            input.original_file_name,
            input.checksum
        ],
        map_document_source,
    )
    .map_err(AppError::from)
}

fn ensure_learning_upload_note_tx(
    tx: &Transaction<'_>,
    input: &NewDocumentSource,
) -> Result<i64, AppError> {
    let folder_id = ensure_root_folder_tx(tx, CATEGORY_LEARNING_UPLOAD)?;
    let source_type = input.file_extension.to_ascii_lowercase();
    let existing_id: Option<i64> = tx
        .query_row(
            "SELECT id FROM notes
             WHERE source_file_path = ?1 AND is_deleted = 0
             LIMIT 1",
            params![input.stored_relative_path],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(note_id) = existing_id {
        tx.execute(
            "UPDATE notes
             SET title = ?1, folder_id = ?2, source_file_type = ?3,
                 updated_at = datetime('now','localtime')
             WHERE id = ?4",
            params![input.display_name, folder_id, source_type, note_id],
        )?;
        return Ok(note_id);
    }

    let title_normalized = crate::database::links::normalize_title(&input.display_name);
    let content = "";
    let content_hash = crate::services::hash::sha256_hex(content);
    tx.execute(
        "INSERT INTO notes
            (title, content, folder_id, title_normalized, content_hash,
             source_file_path, source_file_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            input.display_name,
            content,
            folder_id,
            title_normalized,
            content_hash,
            input.stored_relative_path,
            source_type
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

fn ensure_root_folder_tx(tx: &Transaction<'_>, name: &str) -> Result<i64, AppError> {
    if let Some(id) = tx
        .query_row(
            "SELECT id FROM folders WHERE parent_id IS NULL AND name = ?1 ORDER BY id LIMIT 1",
            params![name],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    tx.execute(
        "INSERT INTO folders (name, parent_id) VALUES (?1, NULL)",
        params![name],
    )?;
    Ok(tx.last_insert_rowid())
}
