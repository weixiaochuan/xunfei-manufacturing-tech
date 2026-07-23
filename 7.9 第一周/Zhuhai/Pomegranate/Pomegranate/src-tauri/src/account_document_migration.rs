use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use reqwest::header::AUTHORIZATION;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::account::{
    document_migration_authorization_header, document_migration_server_url,
    load_verified_document_migration_session, AccountState,
};
use crate::state::AppState;

const IMPORT_PATH: &str = "/documents/import-local-markdown";
const DOCUMENTS_PATH: &str = "/documents?kind=markdown&limit=100&offset=0";
const MAX_IMPORT_DOCUMENTS: usize = 100;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportDocument {
    source_local_document_id: String,
    title: String,
    markdown_content: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    legacy_metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportFolder {
    source_local_folder_id: String,
    name: String,
    parent_source_local_folder_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportTag {
    source_local_tag_id: String,
    name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportTagLink {
    source_local_document_id: String,
    source_local_tag_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LocalDigest {
    source_id: String,
    title: String,
    content_sha256: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
}

#[derive(Debug)]
struct LocalSnapshot {
    documents: Vec<ImportDocument>,
    folders: Vec<ImportFolder>,
    tags: Vec<ImportTag>,
    tag_links: Vec<ImportTagLink>,
    digests: Vec<LocalDigest>,
    database_size: u64,
    aggregate_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportOutcome {
    source_local_document_id: String,
    document_id: String,
    content_sha256: String,
}

#[derive(Debug, Deserialize)]
struct ImportResponse {
    status: String,
    imported: usize,
    updated: usize,
    skipped: usize,
    failed: usize,
    outcomes: Vec<ImportOutcome>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicDocument {
    id: String,
    kind: String,
    title: String,
    markdown_content: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    file: Option<PublicDocumentFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicDocumentFile {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DocumentListResponse {
    status: String,
    documents: Vec<PublicDocument>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMigrationReport {
    pub backup_created: bool,
    pub local_total: usize,
    pub local_active: usize,
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
    pub server_active_markdown: usize,
    pub hashes_match: bool,
    pub source_database_unchanged: bool,
}

fn migration_error(message: &str) -> String {
    format!("文档迁移失败：{message}")
}

fn local_timestamp(value: &str) -> Result<String, String> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.to_rfc3339());
    }
    let naive = ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
        .ok_or_else(|| migration_error("SQLite 文档时间格式无效"))?;
    FixedOffset::east_opt(8 * 60 * 60)
        .and_then(|offset| offset.from_local_datetime(&naive).single())
        .map(|date| date.to_rfc3339())
        .ok_or_else(|| migration_error("SQLite 文档时间无法转换"))
}

fn optional_timestamp(value: Option<String>) -> Result<Option<String>, String> {
    value.map(|raw| local_timestamp(&raw)).transpose()
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn read_tags(connection: &Connection, note_id: i64) -> Result<Vec<Value>, String> {
    let mut statement = connection
        .prepare(
            "SELECT t.name, t.color FROM tags t INNER JOIN note_tags nt ON nt.tag_id = t.id WHERE nt.note_id = ?1 ORDER BY t.id",
        )
        .map_err(|_| migration_error("无法读取 SQLite 标签结构"))?;
    let tags = statement
        .query_map([note_id], |row| {
            Ok(json!({ "name": row.get::<_, String>(0)?, "color": row.get::<_, Option<String>>(1)? }))
        })
        .map_err(|_| migration_error("无法读取 SQLite 标签"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| migration_error("无法读取 SQLite 标签"))?;
    Ok(tags)
}

fn read_folder(connection: &Connection, folder_id: Option<i64>) -> Result<Value, String> {
    let Some(folder_id) = folder_id else { return Ok(Value::Null); };
    connection
        .query_row(
            "SELECT name, parent_id, sort_order FROM folders WHERE id = ?1",
            [folder_id],
            |row| Ok(json!({
                "id": folder_id,
                "name": row.get::<_, String>(0)?,
                "parentId": row.get::<_, Option<i64>>(1)?,
                "sortOrder": row.get::<_, i64>(2)?,
            })),
        )
        .or_else(|error| if error == rusqlite::Error::QueryReturnedNoRows { Ok(Value::Null) } else { Err(error) })
        .map_err(|_| migration_error("无法读取 SQLite 文件夹元数据"))
}

fn open_read_only(database_path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| migration_error("无法以只读方式打开 SQLite 数据库"))
}

fn read_snapshot(database_path: &Path) -> Result<LocalSnapshot, String> {
    let database_size = fs::metadata(database_path)
        .map_err(|_| migration_error("无法读取 SQLite 数据库信息"))?
        .len();
    let connection = open_read_only(database_path)?;
    let mut statement = connection
        .prepare(
            "SELECT id, title, content, folder_id, is_daily, daily_date, created_at, updated_at, is_pinned, is_deleted, deleted_at, word_count, source_file_path, source_file_type, content_html, title_normalized, is_hidden, content_hash, is_encrypted, from_ai_conversation_id, companion_conversation_id, last_writeback_mtime, sort_order FROM notes ORDER BY id",
        )
        .map_err(|_| migration_error("SQLite notes 结构与预期不一致"))?;
    let mut rows = statement
        .query([])
        .map_err(|_| migration_error("无法读取 SQLite 文档"))?;
    let mut documents = Vec::new();
    let mut digests = Vec::new();
    let mut aggregate = Sha256::new();

    while let Some(row) = rows.next().map_err(|_| migration_error("无法读取 SQLite 文档"))? {
        let id: i64 = row.get(0).map_err(|_| migration_error("SQLite 文档 ID 无效"))?;
        let title: String = row.get(1).map_err(|_| migration_error("SQLite 文档标题无效"))?;
        let content: String = row.get(2).map_err(|_| migration_error("SQLite 文档正文无效"))?;
        let folder_id: Option<i64> = row.get(3).map_err(|_| migration_error("SQLite 文件夹字段无效"))?;
        let is_encrypted: i64 = row.get(18).map_err(|_| migration_error("SQLite 加密字段无效"))?;
        if is_encrypted != 0 {
            return Err(migration_error("存在加密文档，安全起见未执行任何导入"));
        }
        let created_at = local_timestamp(&row.get::<_, String>(6).map_err(|_| migration_error("SQLite 创建时间无效"))?)?;
        let updated_at = local_timestamp(&row.get::<_, String>(7).map_err(|_| migration_error("SQLite 更新时间无效"))?)?;
        let deleted_at = optional_timestamp(row.get(10).map_err(|_| migration_error("SQLite 删除时间无效"))?)?;
        let source_path: Option<String> = row.get(12).map_err(|_| migration_error("SQLite 来源字段无效"))?;
        let safe_source_name = source_path.as_ref().and_then(|path| Path::new(path).file_name()).and_then(|name| name.to_str());
        let legacy_metadata = json!({
            "folder": read_folder(&connection, folder_id)?,
            "tags": read_tags(&connection, id)?,
            "isDaily": row.get::<_, i64>(4).unwrap_or(0) != 0,
            "dailyDate": row.get::<_, Option<String>>(5).unwrap_or(None),
            "isPinned": row.get::<_, i64>(8).unwrap_or(0) != 0,
            "isDeleted": row.get::<_, i64>(9).unwrap_or(0) != 0,
            "wordCount": row.get::<_, i64>(11).unwrap_or(0),
            "sourceFileName": safe_source_name,
            "sourceFileType": row.get::<_, Option<String>>(13).unwrap_or(None),
            "contentHtml": row.get::<_, Option<String>>(14).unwrap_or(None),
            "titleNormalized": row.get::<_, Option<String>>(15).unwrap_or(None),
            "isHidden": row.get::<_, i64>(16).unwrap_or(0) != 0,
            "legacyContentHash": row.get::<_, Option<String>>(17).unwrap_or(None),
            "isEncrypted": false,
            "fromAiConversationId": row.get::<_, Option<String>>(19).unwrap_or(None),
            "companionConversationId": row.get::<_, Option<String>>(20).unwrap_or(None),
            "lastWritebackMtime": row.get::<_, Option<i64>>(21).unwrap_or(None),
            "sortOrder": row.get::<_, i64>(22).unwrap_or(0),
        });
        let source_id = format!("sqlite-note:{id}");
        let content_sha256 = sha256(&content);
        aggregate.update(source_id.as_bytes());
        aggregate.update(title.as_bytes());
        aggregate.update(content_sha256.as_bytes());
        documents.push(ImportDocument {
            source_local_document_id: source_id.clone(), title: title.clone(), markdown_content: content,
            created_at: created_at.clone(), updated_at: updated_at.clone(), deleted_at: deleted_at.clone(), legacy_metadata,
        });
        digests.push(LocalDigest { source_id, title, content_sha256, created_at, updated_at, deleted_at });
    }
    drop(rows);
    drop(statement);

    let mut folder_statement = connection
        .prepare("SELECT id, name, parent_id FROM folders ORDER BY id")
        .map_err(|_| migration_error("无法读取 SQLite 文件夹目录"))?;
    let folders = folder_statement
        .query_map([], |row| {
            let id = row.get::<_, i64>(0)?;
            Ok(ImportFolder {
                source_local_folder_id: format!("sqlite-folder:{id}"),
                name: row.get(1)?,
                parent_source_local_folder_id: row
                    .get::<_, Option<i64>>(2)?
                    .map(|parent_id| format!("sqlite-folder:{parent_id}")),
            })
        })
        .map_err(|_| migration_error("无法读取 SQLite 文件夹目录"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| migration_error("SQLite 文件夹目录无效"))?;

    let mut tag_statement = connection
        .prepare("SELECT id, name FROM tags ORDER BY id")
        .map_err(|_| migration_error("无法读取 SQLite 标签目录"))?;
    let tags = tag_statement
        .query_map([], |row| {
            let id = row.get::<_, i64>(0)?;
            Ok(ImportTag { source_local_tag_id: format!("sqlite-tag:{id}"), name: row.get(1)? })
        })
        .map_err(|_| migration_error("无法读取 SQLite 标签目录"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| migration_error("SQLite 标签目录无效"))?;

    let mut link_statement = connection
        .prepare("SELECT note_id, tag_id FROM note_tags ORDER BY note_id, tag_id")
        .map_err(|_| migration_error("无法读取 SQLite 文档标签关联"))?;
    let tag_links = link_statement
        .query_map([], |row| {
            Ok(ImportTagLink {
                source_local_document_id: format!("sqlite-note:{}", row.get::<_, i64>(0)?),
                source_local_tag_id: format!("sqlite-tag:{}", row.get::<_, i64>(1)?),
            })
        })
        .map_err(|_| migration_error("无法读取 SQLite 文档标签关联"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| migration_error("SQLite 文档标签关联无效"))?;

    Ok(LocalSnapshot {
        documents,
        folders,
        tags,
        tag_links,
        digests,
        database_size,
        aggregate_sha256: format!("{:x}", aggregate.finalize()),
    })
}

fn create_backup(database_path: &Path, backup_directory: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(backup_directory).map_err(|_| migration_error("无法创建 SQLite 备份目录"))?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup_path = backup_directory.join(format!("markdown-before-import-{stamp}-{}.db", Uuid::new_v4()));
    let connection = open_read_only(database_path)?;
    connection.execute("VACUUM INTO ?1", params![backup_path.to_string_lossy().as_ref()])
        .map_err(|_| migration_error("SQLite 在线备份失败"))?;
    if !backup_path.is_file() { return Err(migration_error("SQLite 备份文件未生成")); }
    Ok(backup_path)
}

async fn run_migration(database_path: &Path, backup_directory: &Path, account_state: &AccountState) -> Result<DocumentMigrationReport, String> {
    let (token, _user) = load_verified_document_migration_session(account_state).await?;
    let before = read_snapshot(database_path)?;
    if before.documents.len() > MAX_IMPORT_DOCUMENTS { return Err(migration_error("本地文档超过单批导入上限")); }
    let _backup_path = create_backup(database_path, backup_directory)?;
    let client = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(5)).timeout(std::time::Duration::from_secs(30)).build().map_err(|_| migration_error("无法创建迁移网络客户端"))?;
    let response = client.post(document_migration_server_url(IMPORT_PATH))
        .header(AUTHORIZATION, document_migration_authorization_header(&token)?)
        .json(&json!({
            "documents": before.documents,
            "folders": before.folders,
            "tags": before.tags,
            "tagLinks": before.tag_links,
        }))
        .send().await.map_err(|_| migration_error("Account Server 暂不可用"))?;
    if !response.status().is_success() { return Err(migration_error("Account Server 拒绝导入")); }
    let imported: ImportResponse = response.json().await.map_err(|_| migration_error("导入响应无效"))?;
    if imported.status != "ok" || imported.failed != 0 || imported.outcomes.len() != before.digests.len() {
        return Err(migration_error("导入结果不完整"));
    }
    let expected = before.digests.iter().map(|item| (item.source_id.as_str(), item.content_sha256.as_str())).collect::<BTreeMap<_, _>>();
    let hashes_match = imported.outcomes.iter().all(|item| {
        !item.document_id.is_empty() && expected.get(item.source_local_document_id.as_str()).is_some_and(|hash| *hash == item.content_sha256)
    });
    if !hashes_match { return Err(migration_error("导入后正文摘要不一致")); }
    let list_response = client.get(document_migration_server_url(DOCUMENTS_PATH))
        .header(AUTHORIZATION, document_migration_authorization_header(&token)?)
        .send().await.map_err(|_| migration_error("无法核对服务端文档目录"))?;
    if !list_response.status().is_success() { return Err(migration_error("服务端文档目录核对失败")); }
    let listed: DocumentListResponse = list_response.json().await.map_err(|_| migration_error("服务端文档目录响应无效"))?;
    if listed.status != "ok" { return Err(migration_error("服务端文档目录响应无效")); }
    let server_active_markdown = listed.documents.iter().filter(|item| {
        item.kind == "markdown" && item.deleted_at.is_none() && !item.id.is_empty() && !item.title.is_empty() && item.markdown_content.is_some() && !item.created_at.is_empty() && !item.updated_at.is_empty()
    }).count();
    let after = read_snapshot(database_path)?;
    let source_database_unchanged = before.database_size == after.database_size && before.aggregate_sha256 == after.aggregate_sha256 && before.documents.len() == after.documents.len();
    if !source_database_unchanged { return Err(migration_error("SQLite 原始数据发生变化，已停止验收")); }
    Ok(DocumentMigrationReport {
        backup_created: true,
        local_total: before.documents.len(),
        local_active: before.digests.iter().filter(|item| item.deleted_at.is_none()).count(),
        imported: imported.imported,
        updated: imported.updated,
        skipped: imported.skipped,
        server_active_markdown,
        hashes_match,
        source_database_unchanged,
    })
}

#[tauri::command]
pub async fn account_import_local_markdown_documents(
    app_state: State<'_, AppState>,
    account_state: State<'_, AccountState>,
) -> Result<DocumentMigrationReport, String> {
    let database_path = app_state.data_dir.join(if cfg!(debug_assertions) { "dev-app.db" } else { "app.db" });
    let backup_directory = app_state.data_dir.join("account-document-backups");
    run_migration(&database_path, &backup_directory, &account_state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_conversion_uses_explicit_local_offset() {
        assert_eq!(local_timestamp("2026-07-22 12:34:56").unwrap(), "2026-07-22T12:34:56+08:00");
    }

    #[test]
    fn source_ids_and_hashes_are_stable() {
        assert_eq!(sha256("same"), sha256("same"));
        assert_ne!(sha256("same"), sha256("different"));
        assert_eq!(format!("sqlite-note:{}", 42), "sqlite-note:42");
    }

    #[tokio::test]
    #[ignore = "requires the real local POME-000001 desktop session and Account Server"]
    async fn real_dev_database_import_runner() {
        assert_eq!(
            std::env::var("POME_RUN_REAL_DOCUMENT_IMPORT").as_deref(),
            Ok("1"),
            "real import requires an explicit opt-in environment variable"
        );
        let app_data = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA is required"));
        let data_dir = app_data.join("edu.bit.inb-dev");
        let report = run_migration(
            &data_dir.join("dev-app.db"),
            &data_dir.join("account-document-backups"),
            &AccountState::default(),
        )
        .await
        .expect("real local Markdown import failed safely");
        println!("{}", serde_json::to_string(&report).expect("safe report serialization"));
    }

    #[tokio::test]
    #[ignore = "requires the real local POME-000001 desktop session and Account Server"]
    async fn real_uploaded_file_document_linkage_runner() {
        assert_eq!(
            std::env::var("POME_RUN_REAL_DOCUMENT_FILE_TEST").as_deref(),
            Ok("1"),
            "real file test requires an explicit opt-in environment variable"
        );
        let account_state = AccountState::default();
        let (token, _) = load_verified_document_migration_session(&account_state)
            .await
            .expect("POME-000001 session is required");
        let client = reqwest::Client::new();
        let part = reqwest::multipart::Part::bytes(b"%PDF-1.4\n% harmless local linkage test\n%%EOF\n".to_vec())
            .file_name("document-linkage-smoke.pdf")
            .mime_str("application/pdf")
            .expect("fixed MIME type");
        let upload = client
            .post(document_migration_server_url("/files"))
            .header(AUTHORIZATION, document_migration_authorization_header(&token).unwrap())
            .multipart(reqwest::multipart::Form::new().part("file", part))
            .send()
            .await
            .expect("file upload request failed");
        assert_eq!(upload.status(), reqwest::StatusCode::CREATED);
        let payload: Value = upload.json().await.expect("safe upload response");
        let file_id = payload.pointer("/file/id").and_then(Value::as_str).expect("file id").to_string();
        let listed: DocumentListResponse = client
            .get(document_migration_server_url("/documents?limit=100&offset=0"))
            .header(AUTHORIZATION, document_migration_authorization_header(&token).unwrap())
            .send().await.expect("document list request failed")
            .json().await.expect("document list response");
        assert!(listed.documents.iter().any(|document| document.kind == "uploaded_file" && document.file.as_ref().is_some_and(|file| file.id == file_id)));
        let deleted = client
            .delete(document_migration_server_url(&format!("/files/{file_id}")))
            .header(AUTHORIZATION, document_migration_authorization_header(&token).unwrap())
            .send().await.expect("file delete request failed");
        assert!(deleted.status().is_success());
        let after: DocumentListResponse = client
            .get(document_migration_server_url("/documents?limit=100&offset=0"))
            .header(AUTHORIZATION, document_migration_authorization_header(&token).unwrap())
            .send().await.expect("post-delete document list failed")
            .json().await.expect("post-delete document list response");
        assert!(!after.documents.iter().any(|document| document.file.as_ref().is_some_and(|file| file.id == file_id)));
        println!("{{\"uploadedDocumentLinked\":true,\"deletedFromActiveCatalog\":true}}");
    }

    #[tokio::test]
    #[ignore = "requires the real local POME-000001 desktop session and Account Server"]
    async fn real_unified_document_entry_runner() {
        use std::collections::HashSet;

        assert_eq!(
            std::env::var("POME_RUN_REAL_DOCUMENT_ENTRY_TEST").as_deref(),
            Ok("1"),
            "real entry test requires an explicit opt-in environment variable"
        );
        fn files(root: &Path) -> HashSet<PathBuf> {
            std::fs::read_dir(root)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|entry| entry.file_type().ok().filter(|kind| kind.is_file()).map(|_| entry.path()))
                .collect()
        }

        let account_state = AccountState::default();
        let (token, _) = load_verified_document_migration_session(&account_state)
            .await
            .expect("POME-000001 session is required");
        let authorization = document_migration_authorization_header(&token).unwrap();
        let client = reqwest::Client::new();
        let marker = Uuid::new_v4().to_string();
        let storage_root = PathBuf::from(r"D:\PomegranateServer\data\user-files");
        let legacy_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../services/account-server/.data/user-files");
        let storage_before = files(&storage_root);
        let legacy_before = files(&legacy_root);

        let create_markdown = |title: String, markdown_content: String| {
            client
                .post(document_migration_server_url("/documents/markdown"))
                .header(AUTHORIZATION, &authorization)
                .json(&json!({
                    "title": title,
                    "markdownContent": markdown_content,
                    "folderId": null,
                    "diaryDate": null,
                    "isPinned": false,
                    "isHidden": false,
                    "sortOrder": 0,
                    "tagIds": [],
                }))
                .send()
        };

        let internal = create_markdown(format!("entry-new-{marker}"), String::new())
            .await.expect("internal Markdown create request failed");
        assert_eq!(internal.status(), reqwest::StatusCode::CREATED);
        let internal: Value = internal.json().await.expect("internal Markdown response");
        assert_eq!(internal.pointer("/document/kind").and_then(Value::as_str), Some("markdown"));
        assert_eq!(internal.pointer("/document/revision").and_then(Value::as_i64), Some(1));
        let internal_id = internal.pointer("/document/id").and_then(Value::as_str).unwrap().to_string();
        assert_eq!(files(&storage_root), storage_before);

        let source_bytes = format!("# harmless uploaded markdown {marker}\n").into_bytes();
        let upload = client
            .post(document_migration_server_url("/files"))
            .header(AUTHORIZATION, &authorization)
            .multipart(reqwest::multipart::Form::new().part(
                "file",
                reqwest::multipart::Part::bytes(source_bytes.clone())
                    .file_name(format!("entry-upload-{marker}.md"))
                    .mime_str("text/markdown").unwrap(),
            ))
            .send().await.expect("Markdown upload request failed");
        assert_eq!(upload.status(), reqwest::StatusCode::CREATED);
        let upload: Value = upload.json().await.expect("Markdown upload response");
        let file_id = upload.pointer("/file/id").and_then(Value::as_str).unwrap().to_string();
        let storage_after_upload = files(&storage_root);
        let added = storage_after_upload.difference(&storage_before).collect::<Vec<_>>();
        assert_eq!(added.len(), 1);
        assert_eq!(std::fs::read(added[0]).expect("stored upload must be readable"), source_bytes);

        let imported = create_markdown(
            format!("entry-import-{marker}"),
            format!("# harmless editable import {marker}\n"),
        ).await.expect("editable Markdown create request failed");
        assert_eq!(imported.status(), reqwest::StatusCode::CREATED);
        let imported: Value = imported.json().await.expect("editable Markdown response");
        assert_eq!(imported.pointer("/document/kind").and_then(Value::as_str), Some("markdown"));
        assert_eq!(imported.pointer("/document/revision").and_then(Value::as_i64), Some(1));
        let imported_id = imported.pointer("/document/id").and_then(Value::as_str).unwrap().to_string();
        assert_eq!(files(&storage_root), storage_after_upload);
        assert_eq!(files(&legacy_root), legacy_before);

        let listed: DocumentListResponse = client
            .get(document_migration_server_url("/documents?limit=100&offset=0"))
            .header(AUTHORIZATION, &authorization)
            .send().await.expect("document list failed")
            .json().await.expect("document list response");
        let uploaded = listed.documents.iter().find(|document| {
            document.file.as_ref().is_some_and(|file| file.id == file_id)
        }).expect("uploaded Markdown must appear in unified documents");
        assert_eq!(uploaded.kind, "uploaded_file");

        for document_id in [&internal_id, &imported_id] {
            let deleted = client
                .delete(document_migration_server_url(&format!("/documents/{document_id}")))
                .header(AUTHORIZATION, &authorization)
                .send().await.expect("Markdown cleanup failed");
            assert!(deleted.status().is_success());
        }
        let deleted_file = client
            .delete(document_migration_server_url(&format!("/files/{file_id}")))
            .header(AUTHORIZATION, &authorization)
            .send().await.expect("uploaded file cleanup failed");
        assert!(deleted_file.status().is_success());
        assert_eq!(files(&storage_root), storage_before);
        assert_eq!(files(&legacy_root), legacy_before);
        println!("{{\"internalMarkdownRevision\":1,\"uploadedMarkdownKind\":\"uploaded_file\",\"importedMarkdownRevision\":1,\"newStorageDeltaAfterCleanup\":0,\"legacyStorageDelta\":0}}");
    }

    #[tokio::test]
    #[ignore = "requires the real local POME-000001 desktop session and Account Server"]
    async fn real_document_revision_conflict_runner() {
        assert_eq!(
            std::env::var("POME_RUN_REAL_DOCUMENT_REVISION_TEST").as_deref(),
            Ok("1"),
            "real revision test requires an explicit opt-in environment variable"
        );
        let account_state = AccountState::default();
        let (token, _) = load_verified_document_migration_session(&account_state)
            .await
            .expect("POME-000001 session is required");
        let authorization = document_migration_authorization_header(&token).unwrap();
        let client = reqwest::Client::new();
        let marker = format!("stage2-smoke-{}", Uuid::new_v4());

        let folder_response = client
            .post(document_migration_server_url("/document-folders"))
            .header(AUTHORIZATION, &authorization)
            .json(&json!({ "name": marker }))
            .send().await.expect("folder request failed");
        assert_eq!(folder_response.status(), reqwest::StatusCode::CREATED);
        let folder: Value = folder_response.json().await.expect("folder response");
        let folder_id = folder.pointer("/folder/id").and_then(Value::as_str).expect("folder id");

        let tag_response = client
            .post(document_migration_server_url("/document-tags"))
            .header(AUTHORIZATION, &authorization)
            .json(&json!({ "name": marker }))
            .send().await.expect("tag request failed");
        assert_eq!(tag_response.status(), reqwest::StatusCode::CREATED);
        let tag: Value = tag_response.json().await.expect("tag response");
        let tag_id = tag.pointer("/tag/id").and_then(Value::as_str).expect("tag id");

        let create_response = client
            .post(document_migration_server_url("/documents/markdown"))
            .header(AUTHORIZATION, &authorization)
            .json(&json!({
                "title": marker,
                "markdownContent": "harmless initial content",
                "folderId": folder_id,
                "diaryDate": "2026-07-23",
                "isPinned": true,
                "sortOrder": 9,
                "tagIds": [tag_id],
            }))
            .send().await.expect("document create request failed");
        assert_eq!(create_response.status(), reqwest::StatusCode::CREATED);
        let created: Value = create_response.json().await.expect("document create response");
        let document_id = created.pointer("/document/id").and_then(Value::as_str).expect("document id");
        assert_eq!(created.pointer("/document/revision").and_then(Value::as_i64), Some(1));

        let first_update = client
            .patch(document_migration_server_url(&format!("/documents/{document_id}")))
            .header(AUTHORIZATION, &authorization)
            .json(&json!({ "expectedRevision": 1, "markdownContent": "harmless first successful update" }))
            .send().await.expect("first update failed");
        assert_eq!(first_update.status(), reqwest::StatusCode::OK);
        let first_payload: Value = first_update.json().await.expect("first update response");
        assert_eq!(first_payload.pointer("/document/revision").and_then(Value::as_i64), Some(2));

        let stale_update = client
            .patch(document_migration_server_url(&format!("/documents/{document_id}")))
            .header(AUTHORIZATION, &authorization)
            .json(&json!({ "expectedRevision": 1, "markdownContent": "stale content must not win" }))
            .send().await.expect("stale update request failed");
        assert_eq!(stale_update.status(), reqwest::StatusCode::CONFLICT);
        let stale_payload: Value = stale_update.json().await.expect("stale update response");
        assert_eq!(stale_payload.get("error").and_then(Value::as_str), Some("document_conflict"));

        let list_response = client
            .get(document_migration_server_url("/documents?limit=100&offset=0"))
            .header(AUTHORIZATION, &authorization)
            .send().await.expect("document list failed");
        assert!(list_response.status().is_success());
        let listed: Value = list_response.json().await.expect("document list response");
        let saved_content = listed.get("documents").and_then(Value::as_array)
            .and_then(|documents| documents.iter().find(|document| document.get("id").and_then(Value::as_str) == Some(document_id)))
            .and_then(|document| document.get("markdownContent")).and_then(Value::as_str);
        assert_eq!(saved_content, Some("harmless first successful update"));

        let deleted = client.delete(document_migration_server_url(&format!("/documents/{document_id}"))).header(AUTHORIZATION, &authorization).send().await.expect("delete failed");
        assert!(deleted.status().is_success());
        let restored = client.post(document_migration_server_url(&format!("/documents/{document_id}/restore"))).header(AUTHORIZATION, &authorization).send().await.expect("restore failed");
        assert!(restored.status().is_success());
        let deleted_again = client.delete(document_migration_server_url(&format!("/documents/{document_id}"))).header(AUTHORIZATION, &authorization).send().await.expect("second delete failed");
        assert!(deleted_again.status().is_success());
        let tag_deleted = client.delete(document_migration_server_url(&format!("/document-tags/{tag_id}"))).header(AUTHORIZATION, &authorization).send().await.expect("tag delete failed");
        assert!(tag_deleted.status().is_success());
        let folder_deleted = client.delete(document_migration_server_url(&format!("/document-folders/{folder_id}"))).header(AUTHORIZATION, &authorization).send().await.expect("folder delete failed");
        assert!(folder_deleted.status().is_success());
        println!("{{\"created\":true,\"firstRevision\":2,\"staleStatus\":409,\"newerContentPreserved\":true,\"restoreVerified\":true}}");
    }
}
