use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use crate::account::{
    account_authorization_header, account_server_endpoint, atomic_replace,
    clear_account_credential, emit_account_signed_out, load_account_document_session, AccountState,
};

const MAX_LIST_LIMIT: u32 = 100;
const MAX_IMPORTED_MARKDOWN_BYTES: u64 = 2 * 1024 * 1024;
const CACHE_DIR_NAME: &str = "account-document-cache";
const WORKSPACE_DIR_NAME: &str = "account-document-workspaces";
const WORKSPACE_METADATA_NAME: &str = ".pomegranate-workspace.json";

#[derive(Clone)]
pub struct AccountDocumentState {
    client: Client,
}

impl Default for AccountDocumentState {
    fn default() -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("固定的账号文档 HTTP 客户端配置应有效");
        Self { client }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDocumentError {
    pub code: String,
    pub message: String,
}

impl AccountDocumentError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub folder_kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicTag {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicDocumentFile {
    pub id: String,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicAccountDocument {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub markdown_content: Option<String>,
    pub file: Option<PublicDocumentFile>,
    pub folder: Option<PublicFolder>,
    pub tags: Vec<PublicTag>,
    pub diary_date: Option<String>,
    pub is_pinned: bool,
    pub is_hidden: bool,
    pub sort_order: i64,
    pub word_count: i64,
    pub content_sha256: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentsResponse {
    status: String,
    documents: Vec<PublicAccountDocument>,
}

#[derive(Clone, Debug, Deserialize)]
struct DocumentResponse {
    status: String,
    document: PublicAccountDocument,
}

#[derive(Clone, Debug, Deserialize)]
struct FoldersResponse {
    status: String,
    folders: Vec<PublicFolder>,
}

#[derive(Clone, Debug, Deserialize)]
struct FolderResponse {
    status: String,
    folder: PublicFolder,
}

#[derive(Clone, Debug, Deserialize)]
struct TagsResponse {
    status: String,
    tags: Vec<PublicTag>,
}

#[derive(Clone, Debug, Deserialize)]
struct TagResponse {
    status: String,
    tag: PublicTag,
}

#[derive(Clone, Debug, Deserialize)]
struct ErrorResponse {
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentListInput {
    pub kind: Option<String>,
    pub folder_id: Option<String>,
    pub tag_id: Option<String>,
    pub diary_date: Option<String>,
    pub hidden: Option<bool>,
    pub deleted: Option<bool>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkdownMutation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diary_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_hidden: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_ids: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderMutation {
    pub name: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TagMutation {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum OpenUploadedDocumentResult {
    Opened,
    ConfirmationRequired { file_name: String },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ImportMarkdownResult {
    Success { document: PublicAccountDocument },
    Cancelled,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedAccountDocumentMaterial {
    pub content: String,
    pub display_name: String,
    pub kind: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadedWorkspaceMetadata {
    document_id: String,
    file_id: String,
    original_name: String,
    mime_type: Option<String>,
    baseline_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedWorkspaceResult {
    pub status: String,
    pub workspace_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaceFileResponse {
    status: String,
    file: PublicDocumentFile,
    document_id: String,
    revision: i64,
    updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadedSyncResult {
    pub status: String,
    pub workspace_id: String,
    pub file: Option<PublicDocumentFile>,
    pub document_id: Option<String>,
    pub revision: Option<i64>,
    pub updated_at: Option<String>,
}

fn validate_uuid(value: &str, code: &str) -> Result<(), AccountDocumentError> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| AccountDocumentError::new(code, "文档不存在或无权访问"))
}

fn validate_sha256(value: &str) -> Result<String, AccountDocumentError> {
    let normalized = value.to_ascii_lowercase();
    if normalized.len() == 64 && normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(normalized)
    } else {
        Err(AccountDocumentError::new(
            "invalidFileMetadata",
            "文件校验信息无效",
        ))
    }
}

fn safe_file_name(value: &str) -> String {
    let name = Path::new(value)
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("document");
    let cleaned: String = name
        .chars()
        .filter(|ch| {
            !ch.is_control() && !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
        .take(180)
        .collect();
    if cleaned.trim_matches(['.', ' ']).is_empty() {
        "document".to_string()
    } else {
        cleaned.trim_matches(['.', ' ']).to_string()
    }
}

fn is_editable_markdown_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "markdown")
    )
}

fn imported_markdown_title(path: &Path) -> String {
    let title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .take(200)
        .collect::<String>();
    let title = title.trim();
    if title.is_empty() {
        "未命名文档".to_string()
    } else {
        title.to_string()
    }
}

fn decode_imported_markdown(mut bytes: Vec<u8>) -> Result<String, AccountDocumentError> {
    if bytes.len() as u64 > MAX_IMPORTED_MARKDOWN_BYTES {
        return Err(AccountDocumentError::new(
            "markdownTooLarge",
            "Markdown 文件超过允许大小",
        ));
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes.drain(..3);
    }
    String::from_utf8(bytes)
        .map_err(|_| AccountDocumentError::new("markdownEncodingUnsupported", "暂不支持该文件编码"))
}

fn hash_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn is_safe_office_type(name: &str, _mime_type: Option<&str>) -> bool {
    let extension = Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let allowed = [
        "doc", "docx", "xls", "xlsx", "csv", "ppt", "pptx", "pdf", "md", "markdown", "mdx", "mdxl",
        "txt", "rtf", "json", "xml", "yaml", "yml", "png", "jpg", "jpeg", "gif", "webp", "bmp",
        "svg",
    ];
    let executable = [
        "exe", "msi", "com", "scr", "bat", "cmd", "ps1", "vbs", "js", "jar", "reg", "lnk", "hta",
    ];
    if executable.contains(&extension.as_str()) {
        return false;
    }
    allowed.contains(&extension.as_str())
}

fn user_message(status: StatusCode, error: Option<&str>) -> AccountDocumentError {
    match status {
        StatusCode::UNAUTHORIZED => {
            AccountDocumentError::new("signedOut", "登录已失效，请重新登录")
        }
        StatusCode::BAD_REQUEST => match error {
            Some("invalid_title") => AccountDocumentError::new("titleInvalid", "文档标题缺失"),
            Some("invalid_markdown_content") => {
                AccountDocumentError::new("markdownContentInvalid", "文档正文格式无效")
            }
            Some("invalid_folder") => AccountDocumentError::new("folderInvalid", "文件夹信息无效"),
            Some("invalid_tag_ids" | "invalid_tags") => {
                AccountDocumentError::new("tagsInvalid", "标签信息无效")
            }
            _ => AccountDocumentError::new("requestShapeInvalid", "新建参数格式错误"),
        },
        StatusCode::NOT_FOUND => AccountDocumentError::new("notFound", "文档不存在或无权访问"),
        StatusCode::CONFLICT if error == Some("document_conflict") => {
            AccountDocumentError::new("documentConflict", "文档已在其他位置更新")
        }
        StatusCode::CONFLICT if error == Some("file_content_unavailable") => {
            AccountDocumentError::new("fileContentMissing", "文件内容已不存在，无法恢复")
        }
        StatusCode::CONFLICT if error == Some("file_conflict") => {
            AccountDocumentError::new("fileConflict", "此文件已在其他设备更新，无法直接覆盖")
        }
        StatusCode::BAD_REQUEST
            if error == Some("file_type_mismatch") || error == Some("file_type_rejected") =>
        {
            AccountDocumentError::new("fileTypeRejected", "此文件类型不允许同步")
        }
        _ => AccountDocumentError::new("unavailable", "账号文档服务暂不可用"),
    }
}

async fn parse_response<T: DeserializeOwned>(
    app: &AppHandle,
    account: &AccountState,
    response: reqwest::Response,
) -> Result<T, AccountDocumentError> {
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .map_err(|_| AccountDocumentError::new("invalidResponse", "账号文档服务返回无效数据"));
    }
    let error = response
        .json::<ErrorResponse>()
        .await
        .ok()
        .and_then(|body| body.error);
    if status == StatusCode::UNAUTHORIZED {
        clear_account_credential(account);
        emit_account_signed_out(app);
    }
    Err(user_message(status, error.as_deref()))
}

async fn request(
    _app: &AppHandle,
    documents: &AccountDocumentState,
    account: &AccountState,
    method: Method,
    path: &str,
) -> Result<reqwest::RequestBuilder, AccountDocumentError> {
    let (token, _) = load_account_document_session(account)
        .await
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let authorization = account_authorization_header(&token)
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    Ok(documents
        .client
        .request(method, account_server_endpoint(path))
        .header(reqwest::header::AUTHORIZATION, authorization))
}

#[tauri::command]
pub async fn account_list_documents(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    input: DocumentListInput,
) -> Result<Vec<PublicAccountDocument>, AccountDocumentError> {
    let limit = input.limit.unwrap_or(50).clamp(1, MAX_LIST_LIMIT);
    let offset = input.offset.unwrap_or(0);
    let mut query = vec![
        ("hidden", input.hidden.unwrap_or(false).to_string()),
        ("deleted", input.deleted.unwrap_or(false).to_string()),
        ("limit", limit.to_string()),
        ("offset", offset.to_string()),
    ];
    if let Some(kind) = input.kind {
        if kind != "markdown" && kind != "uploaded_file" {
            return Err(AccountDocumentError::new(
                "invalidFilter",
                "文档类型筛选无效",
            ));
        }
        query.push(("kind", kind));
    }
    for (key, value) in [("folderId", input.folder_id), ("tagId", input.tag_id)] {
        if let Some(value) = value {
            validate_uuid(&value, "invalidFilter")?;
            query.push((key, value));
        }
    }
    if let Some(value) = input.diary_date {
        query.push(("diaryDate", value));
    }
    let response = request(&app, &documents, &account, Method::GET, "/documents")
        .await?
        .query(&query)
        .send()
        .await
        .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    let body: DocumentsResponse = parse_response(&app, &account, response).await?;
    if body.status != "ok" {
        return Err(AccountDocumentError::new(
            "invalidResponse",
            "账号文档服务返回无效数据",
        ));
    }
    Ok(body.documents)
}

#[tauri::command]
pub async fn account_create_markdown_document(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    input: MarkdownMutation,
) -> Result<PublicAccountDocument, AccountDocumentError> {
    #[cfg(debug_assertions)]
    eprintln!(
        "[account-documents] create fields title={} markdownContent={} folderId={} diaryDate={} isPinned={} isHidden={} sortOrder={} tagIds={}",
        if input.title.is_some() { "string" } else { "missing" },
        if input.markdown_content.is_some() { "string" } else { "missing" },
        if input.folder_id.is_some() { "string" } else { "missing" },
        if input.diary_date.is_some() { "string" } else { "missing" },
        if input.is_pinned.is_some() { "boolean" } else { "missing" },
        if input.is_hidden.is_some() { "boolean" } else { "missing" },
        if input.sort_order.is_some() { "integer" } else { "missing" },
        input.tag_ids.as_ref().map_or("missing".to_string(), |values| format!("array({})", values.len())),
    );
    let response = request(
        &app,
        &documents,
        &account,
        Method::POST,
        "/documents/markdown",
    )
    .await?
    .json(&input)
    .send()
    .await
    .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    let body: DocumentResponse = parse_response(&app, &account, response).await?;
    if body.status != "ok" || body.document.revision != 1 || body.document.kind != "markdown" {
        return Err(AccountDocumentError::new(
            "invalidResponse",
            "账号文档服务返回无效数据",
        ));
    }
    Ok(body.document)
}

#[tauri::command]
pub async fn account_import_markdown_file(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
) -> Result<ImportMarkdownResult, AccountDocumentError> {
    let Some(selected) = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown"])
        .blocking_pick_file()
    else {
        return Ok(ImportMarkdownResult::Cancelled);
    };
    let path = selected
        .into_path()
        .map_err(|_| AccountDocumentError::new("markdownReadFailed", "无法读取所选文件"))?;
    if !is_editable_markdown_path(&path) {
        return Err(AccountDocumentError::new(
            "fileTypeRejected",
            "仅支持导入 .md 或 .markdown 文件",
        ));
    }
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|_| AccountDocumentError::new("markdownReadFailed", "无法读取所选文件"))?;
    if !metadata.is_file() {
        return Err(AccountDocumentError::new(
            "markdownReadFailed",
            "请选择一个 Markdown 文件",
        ));
    }
    if metadata.len() > MAX_IMPORTED_MARKDOWN_BYTES {
        return Err(AccountDocumentError::new(
            "markdownTooLarge",
            "Markdown 文件超过允许大小",
        ));
    }
    let title = imported_markdown_title(&path);
    let markdown_content = decode_imported_markdown(
        tokio::fs::read(&path)
            .await
            .map_err(|_| AccountDocumentError::new("markdownReadFailed", "无法读取所选文件"))?,
    )?;
    let document = account_create_markdown_document(
        app,
        documents,
        account,
        MarkdownMutation {
            expected_revision: None,
            title: Some(title),
            markdown_content: Some(markdown_content),
            folder_id: None,
            diary_date: None,
            is_pinned: Some(false),
            is_hidden: Some(false),
            sort_order: Some(0),
            tag_ids: Some(Vec::new()),
        },
    )
    .await?;
    Ok(ImportMarkdownResult::Success { document })
}

#[tauri::command]
pub async fn account_update_markdown_document(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    document_id: String,
    input: MarkdownMutation,
) -> Result<PublicAccountDocument, AccountDocumentError> {
    validate_uuid(&document_id, "notFound")?;
    let response = request(
        &app,
        &documents,
        &account,
        Method::PATCH,
        &format!("/documents/{document_id}"),
    )
    .await?
    .json(&input)
    .send()
    .await
    .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    let body: DocumentResponse = parse_response(&app, &account, response).await?;
    Ok(body.document)
}

async fn document_action(
    app: &AppHandle,
    documents: &AccountDocumentState,
    account: &AccountState,
    method: Method,
    path: &str,
) -> Result<Option<PublicAccountDocument>, AccountDocumentError> {
    let response = request(app, documents, account, method, path)
        .await?
        .send()
        .await
        .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    if response.status().is_success() && response.status() == StatusCode::OK {
        let bytes = response.bytes().await.map_err(|_| {
            AccountDocumentError::new("invalidResponse", "账号文档服务返回无效数据")
        })?;
        if let Ok(body) = serde_json::from_slice::<DocumentResponse>(&bytes) {
            return Ok(Some(body.document));
        }
        return Ok(None);
    }
    parse_response::<serde_json::Value>(app, account, response).await?;
    Ok(None)
}

#[tauri::command]
pub async fn account_delete_document(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    document_id: String,
) -> Result<(), AccountDocumentError> {
    validate_uuid(&document_id, "notFound")?;
    document_action(
        &app,
        &documents,
        &account,
        Method::DELETE,
        &format!("/documents/{document_id}"),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn account_restore_document(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    document_id: String,
) -> Result<PublicAccountDocument, AccountDocumentError> {
    validate_uuid(&document_id, "notFound")?;
    document_action(
        &app,
        &documents,
        &account,
        Method::POST,
        &format!("/documents/{document_id}/restore"),
    )
    .await?
    .ok_or_else(|| AccountDocumentError::new("invalidResponse", "账号文档服务返回无效数据"))
}

#[tauri::command]
pub async fn account_list_document_folders(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
) -> Result<Vec<PublicFolder>, AccountDocumentError> {
    let response = request(&app, &documents, &account, Method::GET, "/document-folders")
        .await?
        .send()
        .await
        .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    Ok(parse_response::<FoldersResponse>(&app, &account, response)
        .await?
        .folders)
}

#[tauri::command]
pub async fn account_get_or_create_learning_assistant_upload_folder(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
) -> Result<PublicFolder, AccountDocumentError> {
    let response = request(
        &app,
        &documents,
        &account,
        Method::POST,
        "/document-folders/learning-assistant-upload",
    )
    .await?
    .send()
    .await
    .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    Ok(parse_response::<FolderResponse>(&app, &account, response)
        .await?
        .folder)
}

#[tauri::command]
pub async fn account_create_document_folder(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    input: FolderMutation,
) -> Result<PublicFolder, AccountDocumentError> {
    let response = request(
        &app,
        &documents,
        &account,
        Method::POST,
        "/document-folders",
    )
    .await?
    .json(&input)
    .send()
    .await
    .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    Ok(parse_response::<FolderResponse>(&app, &account, response)
        .await?
        .folder)
}

#[tauri::command]
pub async fn account_update_document_folder(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    folder_id: String,
    input: FolderMutation,
) -> Result<PublicFolder, AccountDocumentError> {
    validate_uuid(&folder_id, "notFound")?;
    let response = request(
        &app,
        &documents,
        &account,
        Method::PATCH,
        &format!("/document-folders/{folder_id}"),
    )
    .await?
    .json(&input)
    .send()
    .await
    .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    Ok(parse_response::<FolderResponse>(&app, &account, response)
        .await?
        .folder)
}

#[tauri::command]
pub async fn account_delete_document_folder(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    folder_id: String,
) -> Result<(), AccountDocumentError> {
    validate_uuid(&folder_id, "notFound")?;
    document_action(
        &app,
        &documents,
        &account,
        Method::DELETE,
        &format!("/document-folders/{folder_id}"),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn account_list_document_tags(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
) -> Result<Vec<PublicTag>, AccountDocumentError> {
    let response = request(&app, &documents, &account, Method::GET, "/document-tags")
        .await?
        .send()
        .await
        .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    Ok(parse_response::<TagsResponse>(&app, &account, response)
        .await?
        .tags)
}

#[tauri::command]
pub async fn account_create_document_tag(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    input: TagMutation,
) -> Result<PublicTag, AccountDocumentError> {
    let response = request(&app, &documents, &account, Method::POST, "/document-tags")
        .await?
        .json(&input)
        .send()
        .await
        .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    Ok(parse_response::<TagResponse>(&app, &account, response)
        .await?
        .tag)
}

#[tauri::command]
pub async fn account_update_document_tag(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    tag_id: String,
    input: TagMutation,
) -> Result<PublicTag, AccountDocumentError> {
    validate_uuid(&tag_id, "notFound")?;
    let response = request(
        &app,
        &documents,
        &account,
        Method::PATCH,
        &format!("/document-tags/{tag_id}"),
    )
    .await?
    .json(&input)
    .send()
    .await
    .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    Ok(parse_response::<TagResponse>(&app, &account, response)
        .await?
        .tag)
}

#[tauri::command]
pub async fn account_delete_document_tag(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    tag_id: String,
) -> Result<(), AccountDocumentError> {
    validate_uuid(&tag_id, "notFound")?;
    document_action(
        &app,
        &documents,
        &account,
        Method::DELETE,
        &format!("/document-tags/{tag_id}"),
    )
    .await?;
    Ok(())
}

async fn write_cache_file(path: &Path, content: &[u8]) -> Result<(), AccountDocumentError> {
    let parent = path
        .parent()
        .ok_or_else(|| AccountDocumentError::new("cacheFailed", "无法创建账号文件缓存"))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|_| AccountDocumentError::new("cacheFailed", "无法创建账号文件缓存"))?;
    let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
    tokio::fs::write(&temporary, content)
        .await
        .map_err(|_| AccountDocumentError::new("cacheFailed", "无法写入账号文件缓存"))?;
    atomic_replace(&temporary, path)
        .await
        .map_err(|_| AccountDocumentError::new("cacheFailed", "无法更新账号文件缓存"))?;
    Ok(())
}

fn cache_path(
    app: &AppHandle,
    platform_user_id: &str,
    file_id: &str,
    sha256: &str,
    name: &str,
) -> Result<PathBuf, AccountDocumentError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| AccountDocumentError::new("cacheFailed", "无法访问应用数据目录"))?;
    let user_key = hash_hex(platform_user_id.as_bytes());
    let safe_name = safe_file_name(name);
    let extension = Path::new(&safe_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    Ok(data_dir
        .join(CACHE_DIR_NAME)
        .join(user_key)
        .join(format!("{file_id}-{sha256}{extension}")))
}

async fn ensure_cached_upload(
    app: &AppHandle,
    documents: &AccountDocumentState,
    account: &AccountState,
    file_id: &str,
    original_name: &str,
    sha256: &str,
) -> Result<(PathBuf, String), AccountDocumentError> {
    validate_uuid(file_id, "notFound")?;
    let expected_sha = validate_sha256(sha256)?;
    let safe_name = safe_file_name(original_name);
    let (token, user) = load_account_document_session(account)
        .await
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let target = cache_path(
        app,
        &user.platform_user_id,
        file_id,
        &expected_sha,
        &safe_name,
    )?;

    let mut valid_cache = false;
    if let Ok(bytes) = tokio::fs::read(&target).await {
        valid_cache = hash_hex(&bytes) == expected_sha;
        if !valid_cache {
            let _ = tokio::fs::remove_file(&target).await;
        }
    }
    if !valid_cache {
        let authorization = account_authorization_header(&token)
            .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
        let response = documents
            .client
            .get(account_server_endpoint(&format!(
                "/files/{file_id}/download"
            )))
            .header(reqwest::header::AUTHORIZATION, authorization)
            .send()
            .await
            .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
        if !response.status().is_success() {
            parse_response::<serde_json::Value>(app, account, response).await?;
            return Err(AccountDocumentError::new("downloadFailed", "文件下载失败"));
        }
        let content = response
            .bytes()
            .await
            .map_err(|_| AccountDocumentError::new("downloadFailed", "文件下载失败"))?;
        if hash_hex(&content) != expected_sha {
            return Err(AccountDocumentError::new(
                "hashMismatch",
                "文件下载校验失败",
            ));
        }
        write_cache_file(&target, &content).await?;
        let written = tokio::fs::read(&target)
            .await
            .map_err(|_| AccountDocumentError::new("cacheFailed", "无法校验账号文件缓存"))?;
        if hash_hex(&written) != expected_sha {
            let _ = tokio::fs::remove_file(&target).await;
            return Err(AccountDocumentError::new(
                "hashMismatch",
                "文件缓存校验失败",
            ));
        }
    }
    Ok((target, safe_name))
}

fn is_editable_uploaded_type(name: &str) -> bool {
    matches!(
        Path::new(name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "md" | "markdown"
            | "mdx"
            | "mdxl"
            | "txt"
            | "rtf"
            | "json"
            | "xml"
            | "yaml"
            | "yml"
            | "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "csv"
            | "ppt"
            | "pptx"
    )
}

fn workspace_paths(
    app: &AppHandle,
    platform_user_id: &str,
    workspace_id: &str,
    original_name: &str,
) -> Result<(PathBuf, PathBuf), AccountDocumentError> {
    if workspace_id.len() != 64 || !workspace_id.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(AccountDocumentError::new(
            "workspaceInvalid",
            "编辑工作副本无效",
        ));
    }
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| AccountDocumentError::new("workspaceFailed", "无法访问应用数据目录"))?;
    let directory = data_dir
        .join(WORKSPACE_DIR_NAME)
        .join(hash_hex(platform_user_id.as_bytes()))
        .join(workspace_id);
    Ok((
        directory.join(safe_file_name(original_name)),
        directory.join(WORKSPACE_METADATA_NAME),
    ))
}

async fn read_workspace(
    app: &AppHandle,
    account: &AccountState,
    workspace_id: &str,
) -> Result<(UploadedWorkspaceMetadata, PathBuf, PathBuf), AccountDocumentError> {
    let (_, user) = load_account_document_session(account)
        .await
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| AccountDocumentError::new("workspaceFailed", "无法访问应用数据目录"))?;
    let metadata_path = data_dir
        .join(WORKSPACE_DIR_NAME)
        .join(hash_hex(user.platform_user_id.as_bytes()))
        .join(workspace_id)
        .join(WORKSPACE_METADATA_NAME);
    let metadata_bytes = tokio::fs::read(&metadata_path)
        .await
        .map_err(|_| AccountDocumentError::new("workspaceNotFound", "编辑工作副本不存在"))?;
    let metadata: UploadedWorkspaceMetadata = serde_json::from_slice(&metadata_bytes)
        .map_err(|_| AccountDocumentError::new("workspaceInvalid", "编辑工作副本元数据无效"))?;
    validate_uuid(&metadata.document_id, "workspaceInvalid")?;
    validate_uuid(&metadata.file_id, "workspaceInvalid")?;
    validate_sha256(&metadata.baseline_sha256)?;
    let (file_path, expected_metadata_path) = workspace_paths(
        app,
        &user.platform_user_id,
        workspace_id,
        &metadata.original_name,
    )?;
    if expected_metadata_path != metadata_path {
        return Err(AccountDocumentError::new(
            "workspaceInvalid",
            "编辑工作副本归属无效",
        ));
    }
    Ok((metadata, file_path, metadata_path))
}

async fn write_workspace_metadata(
    path: &Path,
    metadata: &UploadedWorkspaceMetadata,
) -> Result<(), AccountDocumentError> {
    let content = serde_json::to_vec(metadata)
        .map_err(|_| AccountDocumentError::new("workspaceFailed", "无法保存编辑工作副本元数据"))?;
    write_cache_file(path, &content).await
}

#[tauri::command]
pub async fn account_begin_uploaded_document_edit(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    document_id: String,
    file_id: String,
    original_name: String,
    mime_type: Option<String>,
    sha256: String,
) -> Result<UploadedWorkspaceResult, AccountDocumentError> {
    validate_uuid(&document_id, "notFound")?;
    let baseline_sha256 = validate_sha256(&sha256)?;
    let safe_name = safe_file_name(&original_name);
    if !is_editable_uploaded_type(&safe_name) {
        return Err(AccountDocumentError::new(
            "fileTypeRejected",
            "此文件类型仅支持打开查看",
        ));
    }
    let (_, user) = load_account_document_session(&account)
        .await
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let workspace_id = hash_hex(format!("{document_id}:{file_id}").as_bytes());
    let (workspace_file, metadata_path) =
        workspace_paths(&app, &user.platform_user_id, &workspace_id, &safe_name)?;
    if let (Ok(existing), Ok(bytes)) = (
        tokio::fs::read(&metadata_path).await,
        tokio::fs::read(&workspace_file).await,
    ) {
        if let Ok(metadata) = serde_json::from_slice::<UploadedWorkspaceMetadata>(&existing) {
            if metadata.document_id == document_id
                && metadata.file_id == file_id
                && hash_hex(&bytes) != metadata.baseline_sha256
            {
                app.opener()
                    .open_path(workspace_file.to_string_lossy().into_owned(), None::<&str>)
                    .map_err(|_| AccountDocumentError::new("openFailed", "无法打开编辑工作副本"))?;
                return Ok(UploadedWorkspaceResult {
                    status: "modified".into(),
                    workspace_id,
                });
            }
        }
    }
    let (cached, _) = ensure_cached_upload(
        &app,
        &documents,
        &account,
        &file_id,
        &safe_name,
        &baseline_sha256,
    )
    .await?;
    let bytes = tokio::fs::read(cached)
        .await
        .map_err(|_| AccountDocumentError::new("workspaceFailed", "无法准备编辑工作副本"))?;
    write_cache_file(&workspace_file, &bytes).await?;
    write_workspace_metadata(
        &metadata_path,
        &UploadedWorkspaceMetadata {
            document_id,
            file_id,
            original_name: safe_name,
            mime_type,
            baseline_sha256,
        },
    )
    .await?;
    app.opener()
        .open_path(workspace_file.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|_| AccountDocumentError::new("openFailed", "无法打开编辑工作副本"))?;
    Ok(UploadedWorkspaceResult {
        status: "monitoring".into(),
        workspace_id,
    })
}

#[tauri::command]
pub async fn account_check_uploaded_document_edit(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    workspace_id: String,
) -> Result<UploadedWorkspaceResult, AccountDocumentError> {
    let (metadata, file_path, _) = read_workspace(&app, &account, &workspace_id).await?;
    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|_| AccountDocumentError::new("workspaceNotFound", "编辑工作副本不存在"))?;
    let status = if hash_hex(&bytes) == metadata.baseline_sha256 {
        "monitoring"
    } else {
        "modified"
    };
    Ok(UploadedWorkspaceResult {
        status: status.into(),
        workspace_id,
    })
}

#[tauri::command]
pub async fn account_sync_uploaded_document_edit(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    workspace_id: String,
) -> Result<UploadedSyncResult, AccountDocumentError> {
    let (mut metadata, file_path, metadata_path) =
        read_workspace(&app, &account, &workspace_id).await?;
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|_| AccountDocumentError::new("workspaceNotFound", "编辑工作副本不存在"))?;
    if hash_hex(&bytes) == metadata.baseline_sha256 {
        return Ok(UploadedSyncResult {
            status: "unchanged".into(),
            workspace_id,
            file: None,
            document_id: None,
            revision: None,
            updated_at: None,
        });
    }
    let (token, _) = load_account_document_session(&account)
        .await
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let authorization = account_authorization_header(&token)
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let mut file_part =
        reqwest::multipart::Part::bytes(bytes).file_name(metadata.original_name.clone());
    if let Some(mime) = &metadata.mime_type {
        file_part = file_part
            .mime_str(mime)
            .map_err(|_| AccountDocumentError::new("fileTypeRejected", "文件类型信息无效"))?;
    }
    let form = reqwest::multipart::Form::new()
        .text("expectedSha256", metadata.baseline_sha256.clone())
        .part("file", file_part);
    let response = documents
        .client
        .put(account_server_endpoint(&format!(
            "/files/{}/content",
            metadata.file_id
        )))
        .header(reqwest::header::AUTHORIZATION, authorization)
        .multipart(form)
        .send()
        .await
        .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    let body: ReplaceFileResponse = parse_response(&app, &account, response).await?;
    if body.status != "ok"
        || body.document_id != metadata.document_id
        || body.file.id != metadata.file_id
    {
        return Err(AccountDocumentError::new(
            "invalidResponse",
            "账号文档服务返回无效数据",
        ));
    }
    metadata.baseline_sha256 = body.file.sha256.clone();
    write_workspace_metadata(&metadata_path, &metadata).await?;
    Ok(UploadedSyncResult {
        status: "synced".into(),
        workspace_id,
        file: Some(body.file),
        document_id: Some(body.document_id),
        revision: Some(body.revision),
        updated_at: Some(body.updated_at),
    })
}

#[tauri::command]
pub async fn account_discard_uploaded_document_edit(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    workspace_id: String,
) -> Result<UploadedWorkspaceResult, AccountDocumentError> {
    let (mut metadata, file_path, metadata_path) =
        read_workspace(&app, &account, &workspace_id).await?;
    let (token, _) = load_account_document_session(&account)
        .await
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let authorization = account_authorization_header(&token)
        .map_err(|error| AccountDocumentError::new(error.code, error.message))?;
    let response = documents
        .client
        .get(account_server_endpoint(&format!(
            "/files/{}/download",
            metadata.file_id
        )))
        .header(reqwest::header::AUTHORIZATION, authorization)
        .send()
        .await
        .map_err(|_| AccountDocumentError::new("unavailable", "账号文档服务暂不可用"))?;
    if !response.status().is_success() {
        parse_response::<serde_json::Value>(&app, &account, response).await?;
        return Err(AccountDocumentError::new(
            "downloadFailed",
            "服务端版本下载失败",
        ));
    }
    let sha = response
        .headers()
        .get("x-pomegranate-content-sha256")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AccountDocumentError::new("invalidResponse", "服务端文件校验信息缺失"))?;
    let expected = validate_sha256(sha)?;
    let bytes = response
        .bytes()
        .await
        .map_err(|_| AccountDocumentError::new("downloadFailed", "服务端版本下载失败"))?;
    if hash_hex(&bytes) != expected {
        return Err(AccountDocumentError::new(
            "hashMismatch",
            "服务端版本校验失败",
        ));
    }
    write_cache_file(&file_path, &bytes).await?;
    metadata.baseline_sha256 = expected;
    write_workspace_metadata(&metadata_path, &metadata).await?;
    app.opener()
        .open_path(file_path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|_| AccountDocumentError::new("openFailed", "无法打开编辑工作副本"))?;
    Ok(UploadedWorkspaceResult {
        status: "monitoring".into(),
        workspace_id,
    })
}

fn xml_visible_text(xml: &str) -> String {
    let normalized = xml
        .replace("</w:p>", "\n")
        .replace("</a:p>", "\n")
        .replace("<a:br/>", "\n");
    let mut output = String::new();
    let mut in_tag = false;
    for ch in normalized.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn extract_ooxml_text(path: &Path, extension: &str) -> Result<String, AccountDocumentError> {
    let file = std::fs::File::open(path)
        .map_err(|_| AccountDocumentError::new("materialReadFailed", "无法读取账号文档素材"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| AccountDocumentError::new("materialUnsupported", "该 Office 文件无法解析"))?;
    let names: Vec<String> = if extension == "docx" {
        vec!["word/document.xml".to_string()]
    } else {
        let mut names = archive
            .file_names()
            .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
            .map(str::to_string)
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let mut output = String::new();
    for name in names {
        let mut entry = archive.by_name(&name).map_err(|_| {
            AccountDocumentError::new("materialUnsupported", "该 Office 文件缺少可读取正文")
        })?;
        let mut xml = String::new();
        entry.read_to_string(&mut xml).map_err(|_| {
            AccountDocumentError::new("materialUnsupported", "该 Office 文件正文编码无法读取")
        })?;
        output.push_str(&xml_visible_text(&xml));
        output.push('\n');
    }
    if output.trim().is_empty() {
        return Err(AccountDocumentError::new(
            "materialUnsupported",
            "该 Office 文件没有可提取正文",
        ));
    }
    Ok(output)
}

#[tauri::command]
pub async fn account_open_uploaded_document(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    file_id: String,
    original_name: String,
    mime_type: Option<String>,
    sha256: String,
    allow_unknown: bool,
) -> Result<OpenUploadedDocumentResult, AccountDocumentError> {
    let safe_name = safe_file_name(&original_name);
    if !allow_unknown && !is_safe_office_type(&safe_name, mime_type.as_deref()) {
        return Ok(OpenUploadedDocumentResult::ConfirmationRequired {
            file_name: safe_name,
        });
    }
    let (target, _) = ensure_cached_upload(
        &app,
        &documents,
        &account,
        &file_id,
        &original_name,
        &sha256,
    )
    .await?;

    app.opener()
        .open_path(target.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|_| AccountDocumentError::new("openFailed", "无法使用系统默认程序打开文件"))?;
    Ok(OpenUploadedDocumentResult::Opened)
}

#[tauri::command]
pub async fn account_prepare_uploaded_document_material(
    app: AppHandle,
    documents: tauri::State<'_, AccountDocumentState>,
    account: tauri::State<'_, AccountState>,
    file_id: String,
    original_name: String,
    mime_type: Option<String>,
    sha256: String,
) -> Result<PreparedAccountDocumentMaterial, AccountDocumentError> {
    let (target, safe_name) = ensure_cached_upload(
        &app,
        &documents,
        &account,
        &file_id,
        &original_name,
        &sha256,
    )
    .await?;
    let extension = Path::new(&safe_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let parsed = tokio::task::spawn_blocking(move || {
        if matches!(extension.as_str(), "docx" | "pptx") {
            let text = extract_ooxml_text(&target, &extension)?;
            let truncated = text.chars().count() > 60_000;
            return Ok(PreparedAccountDocumentMaterial {
                content: text.chars().take(60_000).collect(),
                display_name: safe_name,
                kind: "office".to_string(),
                truncated,
            });
        }
        let path = target.to_string_lossy().into_owned();
        let preview =
            crate::services::ai::AiService::parse_attachment_auto(&path).map_err(|_| {
                AccountDocumentError::new(
                    "materialUnsupported",
                    "该文件类型暂不能作为 PPT 文字素材",
                )
            })?;
        let result = match preview {
            crate::models::AttachmentPreview::Excel(value) => PreparedAccountDocumentMaterial {
                content: value.markdown,
                display_name: safe_name,
                kind: "excel".to_string(),
                truncated: !value.truncated_sheets.is_empty(),
            },
            crate::models::AttachmentPreview::Pdf(value) => PreparedAccountDocumentMaterial {
                content: value.content,
                display_name: safe_name,
                kind: "pdf".to_string(),
                truncated: value.truncated,
            },
            crate::models::AttachmentPreview::Text(value) => PreparedAccountDocumentMaterial {
                content: value.content,
                display_name: safe_name,
                kind: "text".to_string(),
                truncated: value.truncated,
            },
        };
        Ok(result)
    })
    .await
    .map_err(|_| AccountDocumentError::new("materialReadFailed", "账号文档素材处理失败"))??;
    let _ = mime_type;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_file_names_cannot_escape_the_cache_directory() {
        assert_eq!(safe_file_name("../../secret.pdf"), "secret.pdf");
        assert_eq!(safe_file_name("..\\..\\evil.exe"), "evil.exe");
        assert_eq!(safe_file_name("bad\r\nname.txt"), "badname.txt");
    }

    #[test]
    fn cache_identity_changes_for_every_user_file_or_hash() {
        let key =
            |user: &str, file: &str, sha: &str| hash_hex(format!("{user}:{file}:{sha}").as_bytes());
        assert_ne!(key("a", "f", "1"), key("b", "f", "1"));
        assert_ne!(key("a", "f", "1"), key("a", "g", "1"));
        assert_ne!(key("a", "f", "1"), key("a", "f", "2"));
    }

    #[test]
    fn public_folder_keeps_learning_assistant_upload_kind() {
        let folder: PublicFolder = serde_json::from_value(serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "name": "learning upload",
            "parentId": null,
            "folderKind": "learning_assistant_upload",
            "createdAt": "2026-07-27T00:00:00.000Z",
            "updatedAt": "2026-07-27T00:00:00.000Z"
        }))
        .expect("folder should deserialize");
        assert_eq!(
            folder.folder_kind.as_deref(),
            Some("learning_assistant_upload")
        );
    }

    #[test]
    fn executable_extensions_require_explicit_confirmation() {
        assert!(is_safe_office_type("report.pdf", None));
        assert!(is_safe_office_type("uploaded.md", Some("text/markdown")));
        assert!(is_safe_office_type("component.mdx", Some("text/plain")));
        assert!(!is_safe_office_type("script.exe", None));
        assert!(!is_safe_office_type("command.ps1", Some("text/plain")));
        assert!(!is_safe_office_type("unknown.data", Some("text/plain")));
    }

    #[test]
    fn external_edit_allowlist_accepts_documents_and_rejects_executables() {
        for allowed in [
            "a.md", "a.txt", "a.docx", "a.xlsx", "a.pptx", "a.csv", "a.yaml",
        ] {
            assert!(is_editable_uploaded_type(allowed), "{allowed}");
        }
        for rejected in [
            "a.exe", "a.msi", "a.dll", "a.bat", "a.cmd", "a.ps1", "a.js", "a.jar", "a.lnk", "a.pdf",
        ] {
            assert!(!is_editable_uploaded_type(rejected), "{rejected}");
        }
    }

    #[test]
    fn workspace_identity_is_stable_per_document_and_file() {
        let identity =
            |document: &str, file: &str| hash_hex(format!("{document}:{file}").as_bytes());
        assert_eq!(identity("d1", "f1"), identity("d1", "f1"));
        assert_ne!(identity("d1", "f1"), identity("d2", "f1"));
        assert_ne!(identity("d1", "f1"), identity("d1", "f2"));
    }

    #[test]
    fn editable_markdown_import_accepts_only_md_and_markdown() {
        assert!(is_editable_markdown_path(Path::new("note.md")));
        assert!(is_editable_markdown_path(Path::new("note.MARKDOWN")));
        for rejected in ["note.mdx", "note.mdxl", "note.txt", "note.exe", "note"] {
            assert!(
                !is_editable_markdown_path(Path::new(rejected)),
                "{rejected}"
            );
        }
    }

    #[test]
    fn editable_markdown_import_is_strict_utf8_and_strips_a_bom() {
        assert_eq!(decode_imported_markdown(Vec::new()).unwrap(), "");
        assert_eq!(
            decode_imported_markdown([&[0xef, 0xbb, 0xbf][..], "正文".as_bytes()].concat())
                .unwrap(),
            "正文",
        );
        let error = decode_imported_markdown(vec![0xff, 0xfe, 0x41]).unwrap_err();
        assert_eq!(error.code, "markdownEncodingUnsupported");
    }

    #[test]
    fn editable_markdown_import_rejects_content_over_the_server_limit() {
        let error = decode_imported_markdown(vec![b'a'; MAX_IMPORTED_MARKDOWN_BYTES as usize + 1])
            .unwrap_err();
        assert_eq!(error.code, "markdownTooLarge");
    }

    #[test]
    fn ooxml_visible_text_drops_markup_and_decodes_safe_entities() {
        let text = xml_visible_text("<w:p><w:t>A &amp; B</w:t></w:p><w:p><w:t>C</w:t></w:p>");
        assert_eq!(text, "A & B\nC\n");
    }

    #[test]
    fn markdown_mutation_omits_absent_optional_fields_from_http_json() {
        let input = MarkdownMutation {
            expected_revision: None,
            title: Some("未命名文档".to_string()),
            markdown_content: Some(String::new()),
            folder_id: None,
            diary_date: None,
            is_pinned: None,
            is_hidden: None,
            sort_order: None,
            tag_ids: None,
        };
        let json = serde_json::to_value(input).expect("mutation should serialize");
        assert_eq!(
            json.get("title").and_then(|value| value.as_str()),
            Some("未命名文档")
        );
        assert_eq!(
            json.get("markdownContent").and_then(|value| value.as_str()),
            Some("")
        );
        for absent in [
            "expectedRevision",
            "folderId",
            "diaryDate",
            "isPinned",
            "isHidden",
            "sortOrder",
            "tagIds",
        ] {
            assert!(
                !json.as_object().unwrap().contains_key(absent),
                "{absent} must be omitted"
            );
        }
    }
}
