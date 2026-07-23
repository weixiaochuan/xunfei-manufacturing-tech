use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_DISPOSITION};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use zeroize::Zeroizing;

const DESKTOP_LOGIN_PATH: &str = "/auth/login?client=desktop";
const DESKTOP_EXCHANGE_PATH: &str = "/auth/desktop/exchange";
const SESSION_PATH: &str = "/auth/session";
const LOGOUT_PATH: &str = "/auth/logout";
const FILES_PATH: &str = "/files";
const ACCOUNT_EVENT_NAME: &str = "account:login-result";
const USER_FILE_MAX_BYTES: u64 = 20 * 1024 * 1024;
const ALLOWED_UPLOAD_EXTENSIONS: &[&str] = &[
    "doc", "docx", "xls", "xlsx", "csv", "ppt", "pptx", "pdf", "md", "markdown",
    "mdx", "mdxl", "txt", "rtf", "json", "xml", "yaml", "yml", "png", "jpg", "jpeg",
    "gif", "webp", "bmp", "svg",
];
const BLOCKED_UPLOAD_EXTENSIONS: &[&str] = &[
    "exe", "msi", "dll", "bat", "cmd", "com", "scr", "ps1", "vbs", "js", "jse", "jar",
    "reg", "lnk",
];
const MAX_TICKET_LENGTH: usize = 512;
const SESSION_TOKEN_MIN_LENGTH: usize = 43;
const SESSION_TOKEN_MAX_LENGTH: usize = 512;
const CREDENTIAL_SERVICE: &str = "cn.edu.pomegranate.account";
const CREDENTIAL_USERNAME: &str = "desktop-session";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountUser {
    pub platform_user_id: String,
    pub account_number: String,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum AccountLoginResult {
    Success { user: AccountUser },
    SignedOut,
    Unavailable { message: String },
    Error { message: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserFile {
    pub id: String,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
    pub sha256: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserFileList {
    pub files: Vec<UserFile>,
    pub limit: u32,
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum FilePickerResult {
    Success { file: UserFile },
    Cancelled,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum FileDownloadResult {
    Success { file_name: String },
    Cancelled,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountFileError {
    pub code: &'static str,
    pub message: String,
}

impl AccountFileError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

trait SessionCredentialStore: Send + Sync {
    fn save(&self, token: &[u8]) -> Result<(), CredentialError>;
    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError>;
    fn delete(&self) -> Result<(), CredentialError>;
}

struct WindowsCredentialStore;

#[derive(Debug)]
struct CredentialError;

impl WindowsCredentialStore {
    fn entry() -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USERNAME).map_err(|_| CredentialError)
    }
}

impl SessionCredentialStore for WindowsCredentialStore {
    fn save(&self, token: &[u8]) -> Result<(), CredentialError> {
        Self::entry()?
            .set_secret(token)
            .map_err(|_| CredentialError)
    }

    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError> {
        match Self::entry()?.get_secret() {
            Ok(token) => Ok(Some(Zeroizing::new(token))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CredentialError),
        }
    }

    fn delete(&self) -> Result<(), CredentialError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialError),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RemoteError {
    ServiceUnavailable,
    Rejected,
    InvalidResponse,
    NotFound,
    TooLarge,
    UnsupportedType,
}

#[derive(Clone)]
struct DownloadPayload {
    file_name: String,
    content: Vec<u8>,
}

#[async_trait]
trait AccountRemote: Send + Sync {
    async fn exchange_ticket(
        &self,
        ticket: &str,
    ) -> Result<(Zeroizing<String>, AccountUser), RemoteError>;
    async fn get_session(&self, token: &[u8]) -> Result<AccountUser, RemoteError>;
    async fn logout(&self, token: &[u8]) -> Result<(), RemoteError>;
    async fn list_files(
        &self,
        _token: &[u8],
        _limit: u32,
        _offset: u64,
    ) -> Result<UserFileList, RemoteError> {
        Err(RemoteError::ServiceUnavailable)
    }
    async fn upload_file(
        &self,
        _token: &[u8],
        _file_name: String,
        _content: Vec<u8>,
    ) -> Result<UserFile, RemoteError> {
        Err(RemoteError::ServiceUnavailable)
    }
    async fn download_file(
        &self,
        _token: &[u8],
        _file_id: Uuid,
    ) -> Result<DownloadPayload, RemoteError> {
        Err(RemoteError::ServiceUnavailable)
    }
    async fn delete_file(&self, _token: &[u8], _file_id: Uuid) -> Result<(), RemoteError> {
        Err(RemoteError::ServiceUnavailable)
    }
}

struct HttpAccountRemote {
    client: reqwest::Client,
}

impl HttpAccountRemote {
    fn new() -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("固定的 Account Server HTTP 客户端配置应有效");
        Self { client }
    }

    fn authorization_header(token: &[u8]) -> Result<HeaderValue, RemoteError> {
        if token.len() < SESSION_TOKEN_MIN_LENGTH
            || token.len() > SESSION_TOKEN_MAX_LENGTH
            || !token
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
        {
            return Err(RemoteError::Rejected);
        }
        let mut value = Zeroizing::new(Vec::with_capacity(7 + token.len()));
        value.extend_from_slice(b"Bearer ");
        value.extend_from_slice(token);
        let mut header = HeaderValue::from_bytes(&value).map_err(|_| RemoteError::Rejected)?;
        header.set_sensitive(true);
        Ok(header)
    }
}

#[derive(Serialize)]
struct ExchangeRequest<'a> {
    ticket: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeResponse {
    status: String,
    session_token: Option<String>,
    user: Option<AccountUser>,
}

#[derive(Deserialize)]
struct SessionResponse {
    status: String,
    user: Option<AccountUser>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileListResponse {
    status: String,
    files: Vec<UserFile>,
    limit: u32,
    offset: u64,
}

#[derive(Deserialize)]
struct FileUploadResponse {
    status: String,
    file: Option<UserFile>,
}

#[async_trait]
impl AccountRemote for HttpAccountRemote {
    async fn exchange_ticket(
        &self,
        ticket: &str,
    ) -> Result<(Zeroizing<String>, AccountUser), RemoteError> {
        let response = self
            .client
            .post(account_server_url(DESKTOP_EXCHANGE_PATH))
            .json(&ExchangeRequest { ticket })
            .send()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?;
        if !response.status().is_success() {
            return Err(if response.status().is_client_error() {
                RemoteError::Rejected
            } else {
                RemoteError::ServiceUnavailable
            });
        }
        let payload = response
            .json::<ExchangeResponse>()
            .await
            .map_err(|_| RemoteError::InvalidResponse)?;
        if payload.status != "ok" {
            return Err(RemoteError::InvalidResponse);
        }
        let token = Zeroizing::new(payload.session_token.ok_or(RemoteError::InvalidResponse)?);
        validate_session_token(token.as_bytes())?;
        let user = validate_user(payload.user.ok_or(RemoteError::InvalidResponse)?)?;
        Ok((token, user))
    }

    async fn get_session(&self, token: &[u8]) -> Result<AccountUser, RemoteError> {
        let response = self
            .client
            .get(account_server_url(SESSION_PATH))
            .header(AUTHORIZATION, Self::authorization_header(token)?)
            .send()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RemoteError::Rejected);
        }
        if !response.status().is_success() {
            return Err(RemoteError::ServiceUnavailable);
        }
        let payload = response
            .json::<SessionResponse>()
            .await
            .map_err(|_| RemoteError::InvalidResponse)?;
        if payload.status != "ok" {
            return Err(RemoteError::InvalidResponse);
        }
        validate_user(payload.user.ok_or(RemoteError::InvalidResponse)?)
    }

    async fn logout(&self, token: &[u8]) -> Result<(), RemoteError> {
        let response = self
            .client
            .post(account_server_url(LOGOUT_PATH))
            .header(AUTHORIZATION, Self::authorization_header(token)?)
            .send()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            Ok(())
        } else {
            Err(RemoteError::ServiceUnavailable)
        }
    }

    async fn list_files(
        &self,
        token: &[u8],
        limit: u32,
        offset: u64,
    ) -> Result<UserFileList, RemoteError> {
        let response = self
            .client
            .get(account_server_url(FILES_PATH))
            .header(AUTHORIZATION, Self::authorization_header(token)?)
            .query(&[("limit", limit.to_string()), ("offset", offset.to_string())])
            .send()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RemoteError::Rejected);
        }
        if !response.status().is_success() {
            return Err(RemoteError::ServiceUnavailable);
        }
        let payload = response
            .json::<FileListResponse>()
            .await
            .map_err(|_| RemoteError::InvalidResponse)?;
        if payload.status != "ok" || payload.limit != limit || payload.offset != offset {
            return Err(RemoteError::InvalidResponse);
        }
        let files = payload
            .files
            .into_iter()
            .map(validate_user_file)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(UserFileList {
            files,
            limit,
            offset,
        })
    }

    async fn upload_file(
        &self,
        token: &[u8],
        file_name: String,
        content: Vec<u8>,
    ) -> Result<UserFile, RemoteError> {
        let part = reqwest::multipart::Part::bytes(content).file_name(file_name);
        let response = self
            .client
            .post(account_server_url(FILES_PATH))
            .header(AUTHORIZATION, Self::authorization_header(token)?)
            .multipart(reqwest::multipart::Form::new().part("file", part))
            .send()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RemoteError::Rejected);
        }
        if response.status() == reqwest::StatusCode::PAYLOAD_TOO_LARGE {
            return Err(RemoteError::TooLarge);
        }
        if response.status() == reqwest::StatusCode::BAD_REQUEST {
            return Err(RemoteError::UnsupportedType);
        }
        if !response.status().is_success() {
            return Err(RemoteError::ServiceUnavailable);
        }
        let payload = response
            .json::<FileUploadResponse>()
            .await
            .map_err(|_| RemoteError::InvalidResponse)?;
        if payload.status != "ok" {
            return Err(RemoteError::InvalidResponse);
        }
        validate_user_file(payload.file.ok_or(RemoteError::InvalidResponse)?)
    }

    async fn download_file(
        &self,
        token: &[u8],
        file_id: Uuid,
    ) -> Result<DownloadPayload, RemoteError> {
        let response = self
            .client
            .get(account_server_url(&format!(
                "{FILES_PATH}/{file_id}/download"
            )))
            .header(AUTHORIZATION, Self::authorization_header(token)?)
            .send()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RemoteError::Rejected);
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(RemoteError::NotFound);
        }
        if !response.status().is_success() {
            return Err(RemoteError::ServiceUnavailable);
        }
        if response
            .content_length()
            .is_some_and(|size| size > USER_FILE_MAX_BYTES)
        {
            return Err(RemoteError::InvalidResponse);
        }
        let file_name = response
            .headers()
            .get(CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_disposition_file_name)
            .unwrap_or_else(|| "download".to_string());
        let content = response
            .bytes()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?
            .to_vec();
        if content.len() as u64 > USER_FILE_MAX_BYTES {
            return Err(RemoteError::InvalidResponse);
        }
        Ok(DownloadPayload { file_name, content })
    }

    async fn delete_file(&self, token: &[u8], file_id: Uuid) -> Result<(), RemoteError> {
        let response = self
            .client
            .delete(account_server_url(&format!("{FILES_PATH}/{file_id}")))
            .header(AUTHORIZATION, Self::authorization_header(token)?)
            .send()
            .await
            .map_err(|_| RemoteError::ServiceUnavailable)?;
        match response.status() {
            status if status.is_success() => Ok(()),
            reqwest::StatusCode::UNAUTHORIZED => Err(RemoteError::Rejected),
            reqwest::StatusCode::NOT_FOUND => Err(RemoteError::NotFound),
            _ => Err(RemoteError::ServiceUnavailable),
        }
    }
}

pub struct AccountState {
    pending_result: Mutex<Option<AccountLoginResult>>,
    credentials: Arc<dyn SessionCredentialStore>,
    remote: Arc<dyn AccountRemote>,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            pending_result: Mutex::new(None),
            credentials: Arc::new(WindowsCredentialStore),
            remote: Arc::new(HttpAccountRemote::new()),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CallbackError {
    InvalidUri,
    InvalidTicket,
}

fn account_server_url(path: &str) -> String {
    crate::account_network::account_server_endpoint(path)
}

fn extract_ticket(raw_uri: &str) -> Result<String, CallbackError> {
    let url = reqwest::Url::parse(raw_uri).map_err(|_| CallbackError::InvalidUri)?;
    if url.scheme() != "pomegranate"
        || url.host_str() != Some("auth")
        || url.path() != "/callback"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(CallbackError::InvalidUri);
    }

    let mut pairs = url.query_pairs();
    let Some((name, value)) = pairs.next() else {
        return Err(CallbackError::InvalidTicket);
    };
    if name != "ticket"
        || value.is_empty()
        || value.len() > MAX_TICKET_LENGTH
        || pairs.next().is_some()
    {
        return Err(CallbackError::InvalidTicket);
    }
    Ok(value.into_owned())
}

pub fn is_account_callback_uri(raw_uri: &str) -> bool {
    extract_ticket(raw_uri).is_ok()
}

fn validate_user(user: AccountUser) -> Result<AccountUser, RemoteError> {
    if user.platform_user_id.trim().is_empty()
        || !user.account_number.starts_with("POME-")
        || user.username.trim().is_empty()
    {
        return Err(RemoteError::InvalidResponse);
    }
    Ok(user)
}

fn validate_user_file(file: UserFile) -> Result<UserFile, RemoteError> {
    if Uuid::parse_str(&file.id).is_err()
        || file.original_name.trim().is_empty()
        || file.original_name.len() > 255
        || file.size_bytes > USER_FILE_MAX_BYTES
        || file.sha256.len() != 64
        || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || file.created_at.trim().is_empty()
    {
        return Err(RemoteError::InvalidResponse);
    }
    Ok(file)
}

fn safe_display_file_name(value: &str) -> String {
    let leaf = value
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    let trimmed = leaf.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "download".to_string()
    } else {
        trimmed.chars().take(255).collect()
    }
}

fn upload_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn is_allowed_upload_path(path: &Path) -> bool {
    upload_extension(path).is_some_and(|extension| {
        !BLOCKED_UPLOAD_EXTENSIONS.contains(&extension.as_str())
            && ALLOWED_UPLOAD_EXTENSIONS.contains(&extension.as_str())
    })
}

fn parse_content_disposition_file_name(value: &str) -> Option<String> {
    for part in value.split(';').map(str::trim) {
        if let Some(encoded) = part.strip_prefix("filename*=UTF-8''") {
            if let Ok(decoded) = urlencoding::decode(encoded) {
                return Some(safe_display_file_name(&decoded));
            }
        }
    }
    value
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("filename="))
        .map(|name| safe_display_file_name(name.trim_matches('"')))
}

fn validate_session_token(token: &[u8]) -> Result<(), RemoteError> {
    HttpAccountRemote::authorization_header(token).map(|_| ())
}

fn publish_result(app: &AppHandle, result: AccountLoginResult) {
    if let Ok(mut pending) = app.state::<AccountState>().pending_result.lock() {
        *pending = Some(result.clone());
    }
    let _ = app.emit(ACCOUNT_EVENT_NAME, result);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

async fn restore_session_inner(state: &AccountState) -> AccountLoginResult {
    let token = match state.credentials.load() {
        Ok(Some(token)) => token,
        Ok(None) => return AccountLoginResult::SignedOut,
        Err(_) => {
            return AccountLoginResult::Unavailable {
                message: "无法读取系统安全凭据，账号状态暂不可用".to_string(),
            }
        }
    };

    match state.remote.get_session(&token).await {
        Ok(user) => AccountLoginResult::Success { user },
        Err(RemoteError::Rejected) => {
            if state.credentials.delete().is_err() {
                return AccountLoginResult::Error {
                    message: "登录已失效，但无法清除本机安全凭据".to_string(),
                };
            }
            AccountLoginResult::SignedOut
        }
        Err(
            RemoteError::ServiceUnavailable
            | RemoteError::InvalidResponse
            | RemoteError::NotFound
            | RemoteError::TooLarge
            | RemoteError::UnsupportedType,
        ) => AccountLoginResult::Unavailable {
            message: "账号服务暂不可用，本地功能仍可正常使用".to_string(),
        },
    }
}

async fn logout_inner(state: &AccountState) -> AccountLoginResult {
    let token = match state.credentials.load() {
        Ok(token) => token,
        Err(_) => {
            return AccountLoginResult::Error {
                message: "无法读取系统安全凭据，退出失败".to_string(),
            }
        }
    };

    if let Some(token) = token.as_deref() {
        if state.remote.logout(token).await.is_err() {
            log::warn!("账号服务未确认 session 撤销，将继续清除本机凭据");
        }
    }

    if state.credentials.delete().is_err() {
        return AccountLoginResult::Error {
            message: "无法清除本机安全凭据，退出失败".to_string(),
        };
    }
    AccountLoginResult::SignedOut
}

pub fn handle_deep_link(app: AppHandle, raw_uri: String) {
    let ticket = match extract_ticket(&raw_uri) {
        Ok(ticket) => ticket,
        Err(_) => {
            log::warn!("账号登录回调格式无效");
            publish_result(
                &app,
                AccountLoginResult::Error {
                    message: "收到无效的账号登录回调，请重新登录".to_string(),
                },
            );
            return;
        }
    };

    log::info!("收到账号登录回调，开始交换一次性凭证");
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AccountState>();
        match state.remote.exchange_ticket(&ticket).await {
            Ok((token, user)) => {
                if state.credentials.save(token.as_bytes()).is_err() {
                    let _ = state.remote.logout(token.as_bytes()).await;
                    log::warn!("无法保存桌面账号安全凭据");
                    publish_result(
                        &app,
                        AccountLoginResult::Error {
                            message: "无法保存系统安全凭据，登录未完成".to_string(),
                        },
                    );
                    return;
                }
                log::info!("桌面账号登录成功，session 已保存至系统安全凭据存储");
                publish_result(&app, AccountLoginResult::Success { user });
            }
            Err(
                RemoteError::ServiceUnavailable
                | RemoteError::NotFound
                | RemoteError::TooLarge
                | RemoteError::UnsupportedType,
            ) => publish_result(
                &app,
                AccountLoginResult::Error {
                    message: "无法连接账号服务，请确认本地 Account Server 已启动".to_string(),
                },
            ),
            Err(RemoteError::Rejected | RemoteError::InvalidResponse) => publish_result(
                &app,
                AccountLoginResult::Error {
                    message: "登录凭证无效或已过期，请重新登录".to_string(),
                },
            ),
        }
    });
}

pub(crate) fn load_file_session(state: &AccountState) -> Result<Zeroizing<Vec<u8>>, AccountFileError> {
    match state.credentials.load() {
        Ok(Some(token)) => Ok(token),
        Ok(None) => Err(AccountFileError::new("signedOut", "请先登录")),
        Err(_) => Err(AccountFileError::new(
            "unavailable",
            "无法读取系统安全凭据，账号状态暂不可用",
        )),
    }
}

pub(crate) async fn load_account_document_session(
    state: &AccountState,
) -> Result<(Zeroizing<Vec<u8>>, AccountUser), AccountFileError> {
    let token = load_file_session(state)?;
    let user = state
        .remote
        .get_session(&token)
        .await
        .map_err(|error| map_file_remote_error(state, error))?;
    Ok((token, user))
}

pub(crate) fn account_authorization_header(
    token: &[u8],
) -> Result<HeaderValue, AccountFileError> {
    HttpAccountRemote::authorization_header(token)
        .map_err(|_| AccountFileError::new("signedOut", "登录已失效，请重新登录"))
}

pub(crate) fn account_server_endpoint(path: &str) -> String {
    crate::account_network::account_server_endpoint(path)
}

pub(crate) fn clear_account_credential(state: &AccountState) {
    let _ = state.credentials.delete();
}

pub(crate) fn emit_account_signed_out(app: &AppHandle) {
    let _ = app.emit(ACCOUNT_EVENT_NAME, AccountLoginResult::SignedOut);
}

pub(crate) async fn load_verified_document_migration_session(
    state: &AccountState,
) -> Result<(Zeroizing<Vec<u8>>, AccountUser), String> {
    let token = load_file_session(state).map_err(|_| "未找到有效的桌面登录凭据".to_string())?;
    let user = state
        .remote
        .get_session(&token)
        .await
        .map_err(|_| "无法验证当前桌面登录账号".to_string())?;
    if user.account_number != "POME-000001" {
        return Err("当前账号不是 POME-000001，已拒绝导入".to_string());
    }
    Ok((token, user))
}

pub(crate) fn document_migration_authorization_header(
    token: &[u8],
) -> Result<HeaderValue, String> {
    HttpAccountRemote::authorization_header(token)
        .map_err(|_| "桌面登录凭据无效".to_string())
}

pub(crate) fn document_migration_server_url(path: &str) -> String {
    account_server_url(path)
}

fn map_file_remote_error(state: &AccountState, error: RemoteError) -> AccountFileError {
    match error {
        RemoteError::Rejected => {
            let _ = state.credentials.delete();
            AccountFileError::new("signedOut", "登录已失效，请重新登录")
        }
        RemoteError::NotFound => AccountFileError::new("notFound", "文件不存在或无权访问"),
        RemoteError::TooLarge => AccountFileError::new("tooLarge", "文件超过 20 MiB"),
        RemoteError::UnsupportedType => {
            AccountFileError::new("fileTypeRejected", "不支持上传此文件类型")
        }
        RemoteError::ServiceUnavailable => AccountFileError::new("unavailable", "账号服务暂不可用"),
        RemoteError::InvalidResponse => {
            AccountFileError::new("invalidResponse", "账号服务返回了无效的文件信息")
        }
    }
}

fn notify_signed_out(app: &AppHandle, error: &AccountFileError) {
    if error.code == "signedOut" {
        let _ = app.emit(ACCOUNT_EVENT_NAME, AccountLoginResult::SignedOut);
    }
}

async fn list_files_inner(
    state: &AccountState,
    limit: Option<u32>,
    offset: Option<u64>,
) -> Result<UserFileList, AccountFileError> {
    let limit = limit.unwrap_or(50).clamp(1, 100);
    let offset = offset.unwrap_or(0);
    let token = load_file_session(state)?;
    state
        .remote
        .list_files(&token, limit, offset)
        .await
        .map_err(|error| map_file_remote_error(state, error))
}

async fn upload_file_inner(
    state: &AccountState,
    path: &Path,
) -> Result<UserFile, AccountFileError> {
    if !is_allowed_upload_path(path) {
        return Err(AccountFileError::new(
            "fileTypeRejected",
            "不支持上传此文件类型",
        ));
    }
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| AccountFileError::new("uploadFailed", "无法读取所选文件"))?;
    if !metadata.is_file() {
        return Err(AccountFileError::new("uploadFailed", "请选择一个普通文件"));
    }
    if metadata.len() > USER_FILE_MAX_BYTES {
        return Err(AccountFileError::new("tooLarge", "文件超过 20 MiB"));
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(safe_display_file_name)
        .ok_or_else(|| AccountFileError::new("uploadFailed", "无法识别文件名"))?;
    let content = tokio::fs::read(path)
        .await
        .map_err(|_| AccountFileError::new("uploadFailed", "无法读取所选文件"))?;
    if content.len() as u64 > USER_FILE_MAX_BYTES {
        return Err(AccountFileError::new("tooLarge", "文件超过 20 MiB"));
    }
    let token = load_file_session(state)?;
    state
        .remote
        .upload_file(&token, file_name, content)
        .await
        .map_err(|error| map_file_remote_error(state, error))
}

fn parse_file_id(file_id: &str) -> Result<Uuid, AccountFileError> {
    Uuid::parse_str(file_id).map_err(|_| AccountFileError::new("notFound", "文件不存在或无权访问"))
}

async fn download_file_inner(
    state: &AccountState,
    file_id: &str,
) -> Result<DownloadPayload, AccountFileError> {
    let file_id = parse_file_id(file_id)?;
    let token = load_file_session(state)?;
    state
        .remote
        .download_file(&token, file_id)
        .await
        .map_err(|error| map_file_remote_error(state, error))
}

async fn find_download_name_inner(
    state: &AccountState,
    file_id: Uuid,
) -> Result<String, AccountFileError> {
    let token = load_file_session(state)?;
    let mut offset = 0_u64;
    loop {
        let page = state
            .remote
            .list_files(&token, 100, offset)
            .await
            .map_err(|error| map_file_remote_error(state, error))?;
        if let Some(file) = page
            .files
            .iter()
            .find(|file| file.id.eq_ignore_ascii_case(&file_id.to_string()))
        {
            return Ok(safe_display_file_name(&file.original_name));
        }
        if page.files.len() < 100 {
            return Err(AccountFileError::new("notFound", "文件不存在或无权访问"));
        }
        offset = offset
            .checked_add(100)
            .ok_or_else(|| AccountFileError::new("notFound", "文件不存在或无权访问"))?;
    }
}

async fn delete_file_inner(state: &AccountState, file_id: &str) -> Result<(), AccountFileError> {
    let file_id = parse_file_id(file_id)?;
    let token = load_file_session(state)?;
    state
        .remote
        .delete_file(&token, file_id)
        .await
        .map_err(|error| map_file_remote_error(state, error))
}

async fn write_download_atomically(target: &Path, content: &[u8]) -> Result<(), AccountFileError> {
    let parent = target
        .parent()
        .ok_or_else(|| AccountFileError::new("downloadFailed", "无法使用所选保存位置"))?;
    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    let temporary = parent.join(format!(
        ".{}.pomegranate-{}.tmp",
        safe_display_file_name(target_name),
        Uuid::new_v4()
    ));

    let write_result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await?;
        file.write_all(content).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        atomic_replace(&temporary, target).await
    }
    .await;

    if write_result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(AccountFileError::new(
            "downloadFailed",
            "保存文件失败，请检查目标位置是否可写",
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) async fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source.to_path_buf();
    let target = target.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let source_wide = source
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let target_wide = target
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let succeeded = unsafe {
            MoveFileExW(
                source_wide.as_ptr(),
                target_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if succeeded == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
    .await
    .map_err(std::io::Error::other)?
}

#[cfg(not(windows))]
pub(crate) async fn atomic_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    tokio::fs::rename(source, target).await
}

#[tauri::command]
pub fn begin_account_login(app: AppHandle) -> Result<(), String> {
    log::info!("开始桌面账号登录流程");
    app.opener()
        .open_url(account_server_url(DESKTOP_LOGIN_PATH), None::<&str>)
        .map_err(|_| "无法打开系统浏览器，请稍后重试".to_string())
}

#[tauri::command]
pub async fn restore_account_session(
    state: tauri::State<'_, AccountState>,
) -> Result<AccountLoginResult, String> {
    Ok(restore_session_inner(&state).await)
}

#[tauri::command]
pub async fn logout_account(
    state: tauri::State<'_, AccountState>,
) -> Result<AccountLoginResult, String> {
    Ok(logout_inner(&state).await)
}

#[tauri::command]
pub fn take_pending_account_login_result(
    state: tauri::State<'_, AccountState>,
) -> Option<AccountLoginResult> {
    state.pending_result.lock().ok()?.take()
}

#[tauri::command]
pub async fn account_list_files(
    app: AppHandle,
    state: tauri::State<'_, AccountState>,
    limit: Option<u32>,
    offset: Option<u64>,
) -> Result<UserFileList, AccountFileError> {
    let result = list_files_inner(&state, limit, offset).await;
    if let Err(error) = &result {
        notify_signed_out(&app, error);
    }
    result
}

#[tauri::command]
pub async fn account_pick_and_upload_file(
    app: AppHandle,
    state: tauri::State<'_, AccountState>,
) -> Result<FilePickerResult, AccountFileError> {
    let Some(selected) = app
        .dialog()
        .file()
        .add_filter("支持的文件", ALLOWED_UPLOAD_EXTENSIONS)
        .blocking_pick_file()
    else {
        return Ok(FilePickerResult::Cancelled);
    };
    let path = selected
        .into_path()
        .map_err(|_| AccountFileError::new("uploadFailed", "无法读取所选文件"))?;
    let result = upload_file_inner(&state, &path).await;
    if let Err(error) = &result {
        notify_signed_out(&app, error);
    }
    result.map(|file| FilePickerResult::Success { file })
}

#[tauri::command]
pub async fn account_download_file(
    app: AppHandle,
    state: tauri::State<'_, AccountState>,
    file_id: String,
) -> Result<FileDownloadResult, AccountFileError> {
    let parsed_file_id = parse_file_id(&file_id)?;
    let suggested_name = match find_download_name_inner(&state, parsed_file_id).await {
        Ok(file_name) => file_name,
        Err(error) => {
            notify_signed_out(&app, &error);
            return Err(error);
        }
    };
    let Some(selected) = app
        .dialog()
        .file()
        .set_file_name(&suggested_name)
        .blocking_save_file()
    else {
        return Ok(FileDownloadResult::Cancelled);
    };
    let target = selected
        .into_path()
        .map_err(|_| AccountFileError::new("downloadFailed", "无法使用所选保存位置"))?;
    let payload = match download_file_inner(&state, &file_id).await {
        Ok(payload) => payload,
        Err(error) => {
            notify_signed_out(&app, &error);
            return Err(error);
        }
    };
    write_download_atomically(&target, &payload.content).await?;
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .map(safe_display_file_name)
        .unwrap_or_else(|| safe_display_file_name(&payload.file_name));
    Ok(FileDownloadResult::Success { file_name })
}

#[tauri::command]
pub async fn account_delete_file(
    app: AppHandle,
    state: tauri::State<'_, AccountState>,
    file_id: String,
) -> Result<(), AccountFileError> {
    let result = delete_file_inner(&state, &file_id).await;
    if let Err(error) = &result {
        notify_signed_out(&app, error);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryCredentialStore {
        token: Mutex<Option<Vec<u8>>>,
        delete_count: Mutex<usize>,
    }

    impl SessionCredentialStore for MemoryCredentialStore {
        fn save(&self, token: &[u8]) -> Result<(), CredentialError> {
            *self.token.lock().unwrap() = Some(token.to_vec());
            Ok(())
        }

        fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, CredentialError> {
            Ok(self.token.lock().unwrap().clone().map(Zeroizing::new))
        }

        fn delete(&self) -> Result<(), CredentialError> {
            *self.token.lock().unwrap() = None;
            *self.delete_count.lock().unwrap() += 1;
            Ok(())
        }
    }

    struct FakeRemote {
        session_result: Mutex<Result<AccountUser, RemoteError>>,
        logout_count: Mutex<usize>,
    }

    #[async_trait]
    impl AccountRemote for FakeRemote {
        async fn exchange_ticket(
            &self,
            _ticket: &str,
        ) -> Result<(Zeroizing<String>, AccountUser), RemoteError> {
            unreachable!()
        }

        async fn get_session(&self, _token: &[u8]) -> Result<AccountUser, RemoteError> {
            self.session_result.lock().unwrap().clone()
        }

        async fn logout(&self, _token: &[u8]) -> Result<(), RemoteError> {
            *self.logout_count.lock().unwrap() += 1;
            Ok(())
        }
    }

    fn test_user() -> AccountUser {
        AccountUser {
            platform_user_id: "platform-user-id".to_string(),
            account_number: "POME-000001".to_string(),
            username: "alice".to_string(),
            display_name: Some("Alice".to_string()),
            email: None,
        }
    }

    fn test_state(
        credentials: Arc<MemoryCredentialStore>,
        session_result: Result<AccountUser, RemoteError>,
    ) -> (AccountState, Arc<FakeRemote>) {
        let remote = Arc::new(FakeRemote {
            session_result: Mutex::new(session_result),
            logout_count: Mutex::new(0),
        });
        (
            AccountState {
                pending_result: Mutex::new(None),
                credentials,
                remote: remote.clone(),
            },
            remote,
        )
    }

    struct FakeFileRemote {
        list_result: Mutex<Result<UserFileList, RemoteError>>,
        delete_result: Mutex<Result<(), RemoteError>>,
    }

    #[async_trait]
    impl AccountRemote for FakeFileRemote {
        async fn exchange_ticket(
            &self,
            _ticket: &str,
        ) -> Result<(Zeroizing<String>, AccountUser), RemoteError> {
            unreachable!()
        }

        async fn get_session(&self, _token: &[u8]) -> Result<AccountUser, RemoteError> {
            unreachable!()
        }

        async fn logout(&self, _token: &[u8]) -> Result<(), RemoteError> {
            unreachable!()
        }

        async fn list_files(
            &self,
            _token: &[u8],
            _limit: u32,
            _offset: u64,
        ) -> Result<UserFileList, RemoteError> {
            self.list_result.lock().unwrap().clone()
        }

        async fn delete_file(&self, _token: &[u8], _file_id: Uuid) -> Result<(), RemoteError> {
            self.delete_result.lock().unwrap().clone()
        }
    }

    fn test_file() -> UserFile {
        UserFile {
            id: "11111111-1111-4111-8111-111111111111".to_string(),
            original_name: "notes.txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            size_bytes: 12,
            sha256: "a".repeat(64),
            created_at: "2026-07-23T00:00:00.000Z".to_string(),
        }
    }

    fn file_test_state(
        credentials: Arc<MemoryCredentialStore>,
        list_result: Result<UserFileList, RemoteError>,
        delete_result: Result<(), RemoteError>,
    ) -> AccountState {
        AccountState {
            pending_result: Mutex::new(None),
            credentials,
            remote: Arc::new(FakeFileRemote {
                list_result: Mutex::new(list_result),
                delete_result: Mutex::new(delete_result),
            }),
        }
    }

    #[test]
    fn accepts_only_the_exact_account_callback_shape() {
        let ticket = extract_ticket("pomegranate://auth/callback?ticket=abc_DEF-123").unwrap();
        assert_eq!(ticket, "abc_DEF-123");
    }

    #[test]
    fn rejects_other_schemes_hosts_paths_and_extra_fields() {
        for uri in [
            "https://auth/callback?ticket=value",
            "pomegranate://other/callback?ticket=value",
            "pomegranate://auth/other?ticket=value",
            "pomegranate://auth/callback",
            "pomegranate://auth/callback?ticket=",
            "pomegranate://auth/callback?ticket=one&subject=two",
            "pomegranate://auth/callback?ticket=one#fragment",
            "pomegranate://user@auth/callback?ticket=one",
            "pomegranate://auth:123/callback?ticket=one",
        ] {
            assert!(extract_ticket(uri).is_err(), "URI should be rejected");
        }
    }

    #[test]
    fn rejects_duplicate_ticket_parameters() {
        assert_eq!(
            extract_ticket("pomegranate://auth/callback?ticket=one&ticket=two"),
            Err(CallbackError::InvalidTicket)
        );
    }

    #[test]
    fn credential_wrapper_saves_reads_and_deletes_without_serializing_the_token() {
        let store = MemoryCredentialStore::default();
        let token = b"a-43-character-session-token-value-123456789";
        store.save(token).unwrap();
        assert_eq!(
            store.load().unwrap().map(|value| value.to_vec()),
            Some(token.to_vec())
        );
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());

        let serialized =
            serde_json::to_string(&AccountLoginResult::Success { user: test_user() }).unwrap();
        assert!(!serialized.contains(std::str::from_utf8(token).unwrap()));
        assert!(!serialized.contains("sessionToken"));
    }

    #[tokio::test]
    async fn startup_restore_success_returns_only_the_user() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        credentials.save(b"a".repeat(43).as_slice()).unwrap();
        let user = test_user();
        let (state, _) = test_state(credentials, Ok(user.clone()));
        assert_eq!(
            restore_session_inner(&state).await,
            AccountLoginResult::Success { user }
        );
    }

    #[tokio::test]
    async fn unauthorized_restore_deletes_the_invalid_credential() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        credentials.save(b"b".repeat(43).as_slice()).unwrap();
        let (state, _) = test_state(credentials.clone(), Err(RemoteError::Rejected));
        assert_eq!(
            restore_session_inner(&state).await,
            AccountLoginResult::SignedOut
        );
        assert!(credentials.load().unwrap().is_none());
    }

    #[tokio::test]
    async fn network_error_preserves_the_existing_credential() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        let token = b"c".repeat(43);
        credentials.save(&token).unwrap();
        let (state, _) = test_state(credentials.clone(), Err(RemoteError::ServiceUnavailable));
        assert!(matches!(
            restore_session_inner(&state).await,
            AccountLoginResult::Unavailable { .. }
        ));
        assert_eq!(
            credentials.load().unwrap().map(|value| value.to_vec()),
            Some(token)
        );
    }

    #[tokio::test]
    async fn logout_revokes_remotely_and_always_clears_the_local_credential() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        credentials.save(b"d".repeat(43).as_slice()).unwrap();
        let (state, remote) = test_state(credentials.clone(), Ok(test_user()));
        assert_eq!(logout_inner(&state).await, AccountLoginResult::SignedOut);
        assert!(credentials.load().unwrap().is_none());
        assert_eq!(*remote.logout_count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn file_list_requires_a_desktop_session() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        let state = file_test_state(
            credentials,
            Ok(UserFileList {
                files: vec![],
                limit: 50,
                offset: 0,
            }),
            Ok(()),
        );
        let error = list_files_inner(&state, None, None).await.unwrap_err();
        assert_eq!(error.code, "signedOut");
    }

    #[tokio::test]
    async fn file_list_returns_only_validated_public_metadata() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        credentials.save(b"f".repeat(43).as_slice()).unwrap();
        let expected = UserFileList {
            files: vec![test_file()],
            limit: 50,
            offset: 0,
        };
        let state = file_test_state(credentials, Ok(expected.clone()), Ok(()));
        assert_eq!(
            list_files_inner(&state, None, None).await.unwrap(),
            expected
        );

        let serialized = serde_json::to_string(&expected).unwrap();
        for forbidden in ["ownerUserId", "owner_user_id", "storageKey", "sessionToken"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn file_list_response_parses_camel_case_public_metadata() {
        let payload: FileListResponse = serde_json::from_str(
            r#"{
              "status":"ok",
              "files":[{
                "id":"11111111-1111-4111-8111-111111111111",
                "originalName":"notes.txt",
                "mimeType":"text/plain",
                "sizeBytes":12,
                "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "createdAt":"2026-07-23T00:00:00.000Z"
              }],
              "limit":50,
              "offset":0
            }"#,
        )
        .unwrap();
        assert_eq!(payload.status, "ok");
        assert_eq!(
            validate_user_file(payload.files[0].clone()).unwrap(),
            test_file()
        );
    }

    #[test]
    fn upload_result_cannot_expose_local_paths_or_private_storage_fields() {
        let serialized =
            serde_json::to_string(&FilePickerResult::Success { file: test_file() }).unwrap();
        for forbidden in ["localPath", "ownerUserId", "storageKey", "sessionToken"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn upload_type_policy_accepts_only_the_explicit_allowlist() {
        for extension in ALLOWED_UPLOAD_EXTENSIONS {
            assert!(is_allowed_upload_path(Path::new(&format!("sample.{extension}"))), "{extension}");
        }
        for extension in BLOCKED_UPLOAD_EXTENSIONS {
            assert!(!is_allowed_upload_path(Path::new(&format!("sample.{extension}"))), "{extension}");
        }
        assert!(!is_allowed_upload_path(Path::new("sample.html")));
        assert!(!is_allowed_upload_path(Path::new("sample")));
        assert!(is_allowed_upload_path(Path::new("REPORT.PDF")));
    }

    #[tokio::test]
    async fn unauthorized_file_request_clears_the_invalid_session() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        credentials.save(b"g".repeat(43).as_slice()).unwrap();
        let state = file_test_state(credentials.clone(), Err(RemoteError::Rejected), Ok(()));
        let error = list_files_inner(&state, None, None).await.unwrap_err();
        assert_eq!(error.code, "signedOut");
        assert!(credentials.load().unwrap().is_none());
    }

    #[tokio::test]
    async fn file_network_error_preserves_the_session() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        let token = b"h".repeat(43);
        credentials.save(&token).unwrap();
        let state = file_test_state(
            credentials.clone(),
            Err(RemoteError::ServiceUnavailable),
            Ok(()),
        );
        let error = list_files_inner(&state, None, None).await.unwrap_err();
        assert_eq!(error.code, "unavailable");
        assert_eq!(
            credentials.load().unwrap().unwrap().as_slice(),
            token.as_slice()
        );
    }

    #[test]
    fn file_ids_must_be_uuids_before_becoming_url_segments() {
        assert!(parse_file_id("11111111-1111-4111-8111-111111111111").is_ok());
        assert!(parse_file_id("../casdoor").is_err());
        assert!(parse_file_id("id?owner=another-user").is_err());
    }

    #[test]
    fn download_errors_and_names_do_not_leak_paths() {
        let name = parse_content_disposition_file_name(
            "attachment; filename=ignored; filename*=UTF-8''..%2Fserver%2Fsecret.txt",
        )
        .unwrap();
        assert_eq!(name, "secret.txt");
        let error = map_file_remote_error(
            &file_test_state(
                Arc::new(MemoryCredentialStore::default()),
                Err(RemoteError::ServiceUnavailable),
                Ok(()),
            ),
            RemoteError::ServiceUnavailable,
        );
        assert!(!error.message.contains("server"));
        assert!(!error.message.contains('\\'));
    }

    #[tokio::test]
    async fn delete_not_found_has_one_safe_message() {
        let credentials = Arc::new(MemoryCredentialStore::default());
        credentials.save(b"i".repeat(43).as_slice()).unwrap();
        let state = file_test_state(
            credentials,
            Ok(UserFileList {
                files: vec![],
                limit: 50,
                offset: 0,
            }),
            Err(RemoteError::NotFound),
        );
        let error = delete_file_inner(&state, "11111111-1111-4111-8111-111111111111")
            .await
            .unwrap_err();
        assert_eq!(error.message, "文件不存在或无权访问");
    }
}
