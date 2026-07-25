use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{header::AUTHORIZATION, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::AppHandle;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::account::{
    account_authorization_header, account_server_endpoint, clear_account_credential,
    emit_account_signed_out, load_account_document_session, AccountFileError, AccountState,
};

const JS_SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;
const MAX_LIST_LIMIT: u32 = 100;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountLearningError {
    pub code: String,
    pub message: String,
}

impl AccountLearningError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    fn invalid_response() -> Self {
        Self::new(
            "invalidResponse",
            "Account service returned an invalid learning project response",
        )
    }

    fn unavailable(is_write: bool) -> Self {
        if is_write {
            Self::new(
                "unavailable",
                "Account service request did not complete. The result may be unknown; refresh learning projects before retrying.",
            )
        } else {
            Self::new("unavailable", "Account service is unavailable")
        }
    }
}

impl From<AccountFileError> for AccountLearningError {
    fn from(error: AccountFileError) -> Self {
        Self::new(error.code, error.message)
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountLearningEnvelopeStatus {
    Completed,
    AccountChanged,
    CompletedAccountChanged,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountLearningEnvelope<T> {
    pub status: AccountLearningEnvelopeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> AccountLearningEnvelope<T> {
    fn completed(data: T) -> Self {
        Self {
            status: AccountLearningEnvelopeStatus::Completed,
            data: Some(data),
        }
    }

    fn account_changed() -> Self {
        Self {
            status: AccountLearningEnvelopeStatus::AccountChanged,
            data: None,
        }
    }

    fn completed_account_changed(data: T) -> Self {
        Self {
            status: AccountLearningEnvelopeStatus::CompletedAccountChanged,
            data: Some(data),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectSummary {
    pub id: String,
    pub name: String,
    pub learning_type: Option<String>,
    pub course_name: Option<String>,
    pub goal_summary: Option<String>,
    pub revision: u64,
    pub last_opened_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectDetail {
    pub id: String,
    pub name: String,
    pub learning_type: Option<String>,
    pub course_name: Option<String>,
    pub goal_summary: Option<String>,
    pub learning_goal: Value,
    pub understanding: Value,
    pub current_plan: Value,
    pub progress: Value,
    pub plan_adjustments: Value,
    pub data_schema_version: u64,
    pub revision: u64,
    pub last_opened_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectListData {
    pub projects: Vec<LearningProjectSummary>,
    pub limit: u32,
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectDeleteData {
    pub project_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LearningProjectListSort {
    Updated,
    Recent,
}

impl LearningProjectListSort {
    fn as_query_value(&self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Recent => "recent",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningProjectListInput {
    pub sort: Option<LearningProjectListSort>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningProjectCreateInput {
    pub name: String,
    pub learning_type: Option<String>,
    pub course_name: Option<String>,
    pub goal_summary: Option<String>,
    pub learning_goal: Option<Value>,
    pub understanding: Option<Value>,
    pub current_plan: Option<Value>,
    pub progress: Option<Value>,
    pub plan_adjustments: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningProjectIdInput {
    pub project_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningProjectUpdateInput {
    pub project_id: String,
    pub expected_revision: u64,
    pub name: Option<String>,
    pub learning_type: Option<Option<String>>,
    pub course_name: Option<Option<String>>,
    pub goal_summary: Option<Option<String>>,
    pub learning_goal: Option<Value>,
    pub understanding: Option<Value>,
    pub current_plan: Option<Value>,
    pub progress: Option<Value>,
    pub plan_adjustments: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningProjectRenameInput {
    pub project_id: String,
    pub expected_revision: u64,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningProjectRevisionInput {
    pub project_id: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LearningProjectDuplicateInput {
    pub project_id: String,
    pub name: Option<String>,
}

#[derive(Clone)]
struct LearningSession {
    token: Zeroizing<Vec<u8>>,
    platform_user_id: String,
}

#[async_trait]
trait LearningSessionProvider {
    async fn load_session(&self) -> Result<LearningSession, AccountLearningError>;
}

struct AccountLearningSessionProvider<'a> {
    account: &'a AccountState,
}

#[async_trait]
impl LearningSessionProvider for AccountLearningSessionProvider<'_> {
    async fn load_session(&self) -> Result<LearningSession, AccountLearningError> {
        let (token, user) = load_account_document_session(self.account).await?;
        Ok(LearningSession {
            token,
            platform_user_id: user.platform_user_id,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct LearningProjectRequest {
    method: Method,
    path: String,
    query: Vec<(String, String)>,
    body: Option<Value>,
    is_write: bool,
}

#[async_trait]
trait LearningProjectRemote {
    async fn send(
        &self,
        token: &[u8],
        request: &LearningProjectRequest,
    ) -> Result<Value, AccountLearningError>;
}

struct HttpLearningProjectRemote;

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("fixed account learning HTTP client configuration should be valid")
    })
}

#[async_trait]
impl LearningProjectRemote for HttpLearningProjectRemote {
    async fn send(
        &self,
        token: &[u8],
        request: &LearningProjectRequest,
    ) -> Result<Value, AccountLearningError> {
        let authorization = account_authorization_header(token)?;
        let mut builder = http_client()
            .request(
                request.method.clone(),
                account_server_endpoint(&request.path),
            )
            .header(AUTHORIZATION, authorization);
        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }
        let response = builder
            .send()
            .await
            .map_err(|_| AccountLearningError::unavailable(request.is_write))?;
        parse_http_response(response, request.is_write).await
    }
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: Option<String>,
}

async fn parse_http_response(
    response: reqwest::Response,
    is_write: bool,
) -> Result<Value, AccountLearningError> {
    let status = response.status();
    if status.is_success() {
        return response
            .json::<Value>()
            .await
            .map_err(|_| AccountLearningError::invalid_response());
    }

    let error_code = response
        .json::<ErrorResponse>()
        .await
        .ok()
        .and_then(|body| body.error);
    Err(map_server_error(status, error_code.as_deref(), is_write))
}

fn map_server_error(
    status: StatusCode,
    error_code: Option<&str>,
    is_write: bool,
) -> AccountLearningError {
    match status {
        StatusCode::UNAUTHORIZED => AccountLearningError::new("signedOut", "Please sign in first"),
        StatusCode::NOT_FOUND => {
            AccountLearningError::new("learningProjectNotFound", "Learning project was not found")
        }
        StatusCode::CONFLICT if error_code == Some("learning_project_conflict") => {
            AccountLearningError::new(
                "learningProjectConflict",
                "Learning project was updated on another device",
            )
        }
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            AccountLearningError::new("validation", "Learning project request is invalid")
        }
        status if status.is_server_error() => AccountLearningError::unavailable(is_write),
        _ => AccountLearningError::unavailable(is_write),
    }
}

fn uuid_path_segment(value: &str) -> Result<String, AccountLearningError> {
    Uuid::parse_str(value)
        .map(|uuid| uuid.to_string())
        .map_err(|_| {
            AccountLearningError::new("learningProjectNotFound", "Learning project was not found")
        })
}

fn ensure_js_safe_integer(value: u64) -> Result<u64, AccountLearningError> {
    if value > JS_SAFE_INTEGER_MAX {
        return Err(AccountLearningError::invalid_response());
    }
    Ok(value)
}

fn ensure_positive_js_safe_integer(value: u64) -> Result<u64, AccountLearningError> {
    if value == 0 {
        return Err(AccountLearningError::invalid_response());
    }
    ensure_js_safe_integer(value)
}

fn ensure_input_revision(value: u64) -> Result<u64, AccountLearningError> {
    if value == 0 || value > JS_SAFE_INTEGER_MAX {
        return Err(AccountLearningError::new(
            "validation",
            "Learning project revision is invalid",
        ));
    }
    Ok(value)
}

fn ensure_list_limit(value: Option<u32>) -> Result<u32, AccountLearningError> {
    let limit = value.unwrap_or(50);
    if limit == 0 || limit > MAX_LIST_LIMIT {
        return Err(AccountLearningError::new(
            "validation",
            "Learning project list limit is invalid",
        ));
    }
    Ok(limit)
}

fn ensure_list_offset(value: Option<u64>) -> Result<u64, AccountLearningError> {
    let offset = value.unwrap_or(0);
    if offset > JS_SAFE_INTEGER_MAX {
        return Err(AccountLearningError::new(
            "validation",
            "Learning project list offset is invalid",
        ));
    }
    Ok(offset)
}

fn ensure_object(value: &Value) -> Result<(), AccountLearningError> {
    if value.as_object().is_none() {
        return Err(AccountLearningError::invalid_response());
    }
    Ok(())
}

fn ensure_array(value: &Value) -> Result<(), AccountLearningError> {
    if value.as_array().is_none() {
        return Err(AccountLearningError::invalid_response());
    }
    Ok(())
}

fn safe_string(value: String) -> Result<String, AccountLearningError> {
    if value.trim().is_empty() {
        return Err(AccountLearningError::invalid_response());
    }
    Ok(value)
}

fn optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProjectSummary {
    id: String,
    name: String,
    learning_type: Option<String>,
    course_name: Option<String>,
    goal_summary: Option<String>,
    revision: u64,
    last_opened_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProjectDetail {
    id: String,
    name: String,
    learning_type: Option<String>,
    course_name: Option<String>,
    goal_summary: Option<String>,
    learning_goal: Value,
    understanding: Value,
    current_plan: Value,
    progress: Value,
    plan_adjustments: Value,
    data_schema_version: u64,
    revision: u64,
    last_opened_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct RawProjectResponse {
    status: String,
    project: RawProjectDetail,
}

#[derive(Deserialize)]
struct RawProjectListResponse {
    status: String,
    projects: Vec<RawProjectSummary>,
    limit: u32,
    offset: u64,
}

#[derive(Deserialize)]
struct RawStatusResponse {
    status: String,
}

fn validate_project_summary(
    raw: RawProjectSummary,
) -> Result<LearningProjectSummary, AccountLearningError> {
    let id = uuid_path_segment(&raw.id)?;
    Ok(LearningProjectSummary {
        id,
        name: safe_string(raw.name)?,
        learning_type: raw.learning_type,
        course_name: raw.course_name,
        goal_summary: raw.goal_summary,
        revision: ensure_positive_js_safe_integer(raw.revision)?,
        last_opened_at: raw.last_opened_at,
        created_at: safe_string(raw.created_at)?,
        updated_at: safe_string(raw.updated_at)?,
    })
}

fn validate_project_detail(
    raw: RawProjectDetail,
) -> Result<LearningProjectDetail, AccountLearningError> {
    ensure_object(&raw.learning_goal)?;
    ensure_object(&raw.understanding)?;
    ensure_object(&raw.current_plan)?;
    ensure_object(&raw.progress)?;
    ensure_array(&raw.plan_adjustments)?;
    if raw.data_schema_version == 0 || raw.data_schema_version > JS_SAFE_INTEGER_MAX {
        return Err(AccountLearningError::invalid_response());
    }
    Ok(LearningProjectDetail {
        id: uuid_path_segment(&raw.id)?,
        name: safe_string(raw.name)?,
        learning_type: raw.learning_type,
        course_name: raw.course_name,
        goal_summary: raw.goal_summary,
        learning_goal: raw.learning_goal,
        understanding: raw.understanding,
        current_plan: raw.current_plan,
        progress: raw.progress,
        plan_adjustments: raw.plan_adjustments,
        data_schema_version: raw.data_schema_version,
        revision: ensure_positive_js_safe_integer(raw.revision)?,
        last_opened_at: raw.last_opened_at,
        created_at: safe_string(raw.created_at)?,
        updated_at: safe_string(raw.updated_at)?,
    })
}

fn parse_project_response(raw: Value) -> Result<LearningProjectDetail, AccountLearningError> {
    let response: RawProjectResponse =
        serde_json::from_value(raw).map_err(|_| AccountLearningError::invalid_response())?;
    if response.status != "ok" {
        return Err(AccountLearningError::invalid_response());
    }
    validate_project_detail(response.project)
}

fn parse_project_list_response(
    raw: Value,
) -> Result<LearningProjectListData, AccountLearningError> {
    let response: RawProjectListResponse =
        serde_json::from_value(raw).map_err(|_| AccountLearningError::invalid_response())?;
    if response.status != "ok" || response.limit == 0 || response.limit > MAX_LIST_LIMIT {
        return Err(AccountLearningError::invalid_response());
    }
    let offset = ensure_js_safe_integer(response.offset)?;
    let projects = response
        .projects
        .into_iter()
        .map(validate_project_summary)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LearningProjectListData {
        projects,
        limit: response.limit,
        offset,
    })
}

fn parse_delete_response(
    raw: Value,
    project_id: String,
) -> Result<LearningProjectDeleteData, AccountLearningError> {
    let response: RawStatusResponse =
        serde_json::from_value(raw).map_err(|_| AccountLearningError::invalid_response())?;
    if response.status != "ok" {
        return Err(AccountLearningError::invalid_response());
    }
    Ok(LearningProjectDeleteData { project_id })
}

fn body_from_map(map: Map<String, Value>) -> Value {
    Value::Object(map)
}

fn insert_optional_text(map: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        map.insert(key.to_string(), Value::String(value.clone()));
    }
}

fn insert_optional_json(map: &mut Map<String, Value>, key: &str, value: &Option<Value>) {
    if let Some(value) = value {
        map.insert(key.to_string(), value.clone());
    }
}

fn insert_patch_text(map: &mut Map<String, Value>, key: &str, value: &Option<Option<String>>) {
    if let Some(value) = value {
        map.insert(
            key.to_string(),
            value
                .as_ref()
                .map(|text| Value::String(text.clone()))
                .unwrap_or(Value::Null),
        );
    }
}

fn list_request(
    input: LearningProjectListInput,
) -> Result<LearningProjectRequest, AccountLearningError> {
    let sort = input.sort.unwrap_or(LearningProjectListSort::Updated);
    let limit = ensure_list_limit(input.limit)?;
    let offset = ensure_list_offset(input.offset)?;
    Ok(LearningProjectRequest {
        method: Method::GET,
        path: "/learning/projects".to_string(),
        query: vec![
            ("sort".to_string(), sort.as_query_value().to_string()),
            ("limit".to_string(), limit.to_string()),
            ("offset".to_string(), offset.to_string()),
        ],
        body: None,
        is_write: false,
    })
}

fn create_request(input: LearningProjectCreateInput) -> LearningProjectRequest {
    let mut body = Map::new();
    body.insert("name".to_string(), Value::String(input.name));
    insert_optional_text(&mut body, "learningType", &input.learning_type);
    insert_optional_text(&mut body, "courseName", &input.course_name);
    insert_optional_text(&mut body, "goalSummary", &input.goal_summary);
    insert_optional_json(&mut body, "learningGoal", &input.learning_goal);
    insert_optional_json(&mut body, "understanding", &input.understanding);
    insert_optional_json(&mut body, "currentPlan", &input.current_plan);
    insert_optional_json(&mut body, "progress", &input.progress);
    insert_optional_json(&mut body, "planAdjustments", &input.plan_adjustments);
    LearningProjectRequest {
        method: Method::POST,
        path: "/learning/projects".to_string(),
        query: Vec::new(),
        body: Some(body_from_map(body)),
        is_write: true,
    }
}

fn get_request(
    input: LearningProjectIdInput,
) -> Result<LearningProjectRequest, AccountLearningError> {
    let project_id = uuid_path_segment(&input.project_id)?;
    Ok(LearningProjectRequest {
        method: Method::GET,
        path: format!("/learning/projects/{project_id}"),
        query: Vec::new(),
        body: None,
        is_write: false,
    })
}

fn update_request(
    input: LearningProjectUpdateInput,
) -> Result<LearningProjectRequest, AccountLearningError> {
    let project_id = uuid_path_segment(&input.project_id)?;
    let mut body = Map::new();
    body.insert(
        "expectedRevision".to_string(),
        Value::Number(ensure_input_revision(input.expected_revision)?.into()),
    );
    if let Some(name) = input.name {
        body.insert("name".to_string(), Value::String(name));
    }
    insert_patch_text(&mut body, "learningType", &input.learning_type);
    insert_patch_text(&mut body, "courseName", &input.course_name);
    insert_patch_text(&mut body, "goalSummary", &input.goal_summary);
    insert_optional_json(&mut body, "learningGoal", &input.learning_goal);
    insert_optional_json(&mut body, "understanding", &input.understanding);
    insert_optional_json(&mut body, "currentPlan", &input.current_plan);
    insert_optional_json(&mut body, "progress", &input.progress);
    insert_optional_json(&mut body, "planAdjustments", &input.plan_adjustments);
    Ok(LearningProjectRequest {
        method: Method::PATCH,
        path: format!("/learning/projects/{project_id}"),
        query: Vec::new(),
        body: Some(body_from_map(body)),
        is_write: true,
    })
}

fn rename_request(
    input: LearningProjectRenameInput,
) -> Result<LearningProjectRequest, AccountLearningError> {
    let project_id = uuid_path_segment(&input.project_id)?;
    Ok(LearningProjectRequest {
        method: Method::PATCH,
        path: format!("/learning/projects/{project_id}/name"),
        query: Vec::new(),
        body: Some(serde_json::json!({
            "expectedRevision": ensure_input_revision(input.expected_revision)?,
            "name": input.name,
        })),
        is_write: true,
    })
}

fn open_request(
    input: LearningProjectIdInput,
) -> Result<LearningProjectRequest, AccountLearningError> {
    let project_id = uuid_path_segment(&input.project_id)?;
    Ok(LearningProjectRequest {
        method: Method::POST,
        path: format!("/learning/projects/{project_id}/open"),
        query: Vec::new(),
        body: None,
        is_write: true,
    })
}

fn delete_request(
    input: LearningProjectRevisionInput,
) -> Result<(LearningProjectRequest, String), AccountLearningError> {
    let project_id = uuid_path_segment(&input.project_id)?;
    Ok((
        LearningProjectRequest {
            method: Method::DELETE,
            path: format!("/learning/projects/{project_id}"),
            query: Vec::new(),
            body: Some(serde_json::json!({
                "expectedRevision": ensure_input_revision(input.expected_revision)?,
            })),
            is_write: true,
        },
        project_id,
    ))
}

fn duplicate_request(
    input: LearningProjectDuplicateInput,
) -> Result<LearningProjectRequest, AccountLearningError> {
    let project_id = uuid_path_segment(&input.project_id)?;
    let mut body = Map::new();
    if let Some(name) = optional_string(input.name) {
        body.insert("name".to_string(), Value::String(name));
    }
    Ok(LearningProjectRequest {
        method: Method::POST,
        path: format!("/learning/projects/{project_id}/duplicate"),
        query: Vec::new(),
        body: Some(body_from_map(body)),
        is_write: true,
    })
}

async fn session_matches<S: LearningSessionProvider + Sync>(
    sessions: &S,
    expected_platform_user_id: &str,
) -> bool {
    sessions
        .load_session()
        .await
        .map(|current| current.platform_user_id == expected_platform_user_id)
        .unwrap_or(false)
}

async fn execute_learning_request<S, R, T, F>(
    sessions: &S,
    remote: &R,
    request: LearningProjectRequest,
    parser: F,
) -> Result<AccountLearningEnvelope<T>, AccountLearningError>
where
    S: LearningSessionProvider + Sync,
    R: LearningProjectRemote + Sync,
    F: FnOnce(Value) -> Result<T, AccountLearningError> + Send,
{
    let initial = sessions.load_session().await?;
    if !session_matches(sessions, &initial.platform_user_id).await {
        return Ok(AccountLearningEnvelope::account_changed());
    }

    let raw = remote.send(&initial.token, &request).await?;
    let data = parser(raw)?;
    if session_matches(sessions, &initial.platform_user_id).await {
        Ok(AccountLearningEnvelope::completed(data))
    } else {
        Ok(AccountLearningEnvelope::completed_account_changed(data))
    }
}

async fn execute_account_learning_command<T, F>(
    app: &AppHandle,
    account: &AccountState,
    request: LearningProjectRequest,
    parser: F,
) -> Result<AccountLearningEnvelope<T>, AccountLearningError>
where
    F: FnOnce(Value) -> Result<T, AccountLearningError> + Send,
{
    let sessions = AccountLearningSessionProvider { account };
    let remote = HttpLearningProjectRemote;
    let result = execute_learning_request(&sessions, &remote, request, parser).await;
    if let Err(error) = &result {
        if error.code == "signedOut" {
            clear_account_credential(account);
            emit_account_signed_out(app);
        }
    }
    result
}

#[tauri::command]
pub async fn account_learning_projects_list(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectListInput,
) -> Result<AccountLearningEnvelope<LearningProjectListData>, AccountLearningError> {
    let request = list_request(input)?;
    execute_account_learning_command(&app, &account, request, parse_project_list_response).await
}

#[tauri::command]
pub async fn account_learning_project_create(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectCreateInput,
) -> Result<AccountLearningEnvelope<LearningProjectDetail>, AccountLearningError> {
    let request = create_request(input);
    execute_account_learning_command(&app, &account, request, parse_project_response).await
}

#[tauri::command]
pub async fn account_learning_project_get(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectIdInput,
) -> Result<AccountLearningEnvelope<LearningProjectDetail>, AccountLearningError> {
    let request = get_request(input)?;
    execute_account_learning_command(&app, &account, request, parse_project_response).await
}

#[tauri::command]
pub async fn account_learning_project_update(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectUpdateInput,
) -> Result<AccountLearningEnvelope<LearningProjectDetail>, AccountLearningError> {
    let request = update_request(input)?;
    execute_account_learning_command(&app, &account, request, parse_project_response).await
}

#[tauri::command]
pub async fn account_learning_project_rename(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectRenameInput,
) -> Result<AccountLearningEnvelope<LearningProjectDetail>, AccountLearningError> {
    let request = rename_request(input)?;
    execute_account_learning_command(&app, &account, request, parse_project_response).await
}

#[tauri::command]
pub async fn account_learning_project_open(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectIdInput,
) -> Result<AccountLearningEnvelope<LearningProjectDetail>, AccountLearningError> {
    let request = open_request(input)?;
    execute_account_learning_command(&app, &account, request, parse_project_response).await
}

#[tauri::command]
pub async fn account_learning_project_delete(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectRevisionInput,
) -> Result<AccountLearningEnvelope<LearningProjectDeleteData>, AccountLearningError> {
    let (request, project_id) = delete_request(input)?;
    execute_account_learning_command(&app, &account, request, |raw| {
        parse_delete_response(raw, project_id)
    })
    .await
}

#[tauri::command]
pub async fn account_learning_project_duplicate(
    app: AppHandle,
    account: tauri::State<'_, AccountState>,
    input: LearningProjectDuplicateInput,
) -> Result<AccountLearningEnvelope<LearningProjectDetail>, AccountLearningError> {
    let request = duplicate_request(input)?;
    execute_account_learning_command(&app, &account, request, parse_project_response).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    const PROJECT_ID: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER_PROJECT_ID: &str = "22222222-2222-4222-8222-222222222222";

    struct FakeSessionProvider {
        sessions: Mutex<VecDeque<Result<LearningSession, AccountLearningError>>>,
    }

    #[async_trait]
    impl LearningSessionProvider for FakeSessionProvider {
        async fn load_session(&self) -> Result<LearningSession, AccountLearningError> {
            self.sessions
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(test_session("user-a")))
        }
    }

    struct FakeRemote {
        response: Mutex<Result<Value, AccountLearningError>>,
        requests: Mutex<Vec<LearningProjectRequest>>,
        tokens: Mutex<Vec<Vec<u8>>>,
    }

    #[async_trait]
    impl LearningProjectRemote for FakeRemote {
        async fn send(
            &self,
            token: &[u8],
            request: &LearningProjectRequest,
        ) -> Result<Value, AccountLearningError> {
            self.requests.lock().unwrap().push(request.clone());
            self.tokens.lock().unwrap().push(token.to_vec());
            self.response.lock().unwrap().clone()
        }
    }

    fn test_session(platform_user_id: &str) -> LearningSession {
        LearningSession {
            token: Zeroizing::new(b"session-token".to_vec()),
            platform_user_id: platform_user_id.to_string(),
        }
    }

    fn sessions(values: Vec<Result<LearningSession, AccountLearningError>>) -> FakeSessionProvider {
        FakeSessionProvider {
            sessions: Mutex::new(VecDeque::from(values)),
        }
    }

    fn remote(response: Value) -> Arc<FakeRemote> {
        Arc::new(FakeRemote {
            response: Mutex::new(Ok(response)),
            requests: Mutex::new(Vec::new()),
            tokens: Mutex::new(Vec::new()),
        })
    }

    fn project_json(id: &str, revision: u64) -> Value {
        serde_json::json!({
            "id": id,
            "name": "学习项目",
            "learningType": "exam",
            "courseName": "机械制造",
            "goalSummary": "掌握重点",
            "learningGoal": {"text": "目标"},
            "understanding": {},
            "currentPlan": {"steps": []},
            "progress": {},
            "planAdjustments": [],
            "dataSchemaVersion": 1,
            "revision": revision,
            "lastOpenedAt": null,
            "createdAt": "2026-07-25T00:00:00.000Z",
            "updatedAt": "2026-07-25T00:00:00.000Z",
            "ownerUserId": "must-not-leak",
            "storageKey": "must-not-leak",
            "path": "must-not-leak"
        })
    }

    fn project_response() -> Value {
        serde_json::json!({
            "status": "ok",
            "project": project_json(PROJECT_ID, 1)
        })
    }

    fn list_response() -> Value {
        serde_json::json!({
            "status": "ok",
            "projects": [{
                "id": PROJECT_ID,
                "name": "学习项目",
                "learningType": null,
                "courseName": null,
                "goalSummary": null,
                "revision": 1,
                "lastOpenedAt": null,
                "createdAt": "2026-07-25T00:00:00.000Z",
                "updatedAt": "2026-07-25T00:00:00.000Z",
                "ownerUserId": "must-not-leak"
            }],
            "limit": 50,
            "offset": 0
        })
    }

    async fn execute_test_request<T, F>(
        session_values: Vec<Result<LearningSession, AccountLearningError>>,
        response: Value,
        request: LearningProjectRequest,
        parser: F,
    ) -> (
        Result<AccountLearningEnvelope<T>, AccountLearningError>,
        Arc<FakeRemote>,
    )
    where
        F: FnOnce(Value) -> Result<T, AccountLearningError> + Send,
    {
        let sessions = sessions(session_values);
        let remote = remote(response);
        let result = execute_learning_request(&sessions, remote.as_ref(), request, parser).await;
        (result, remote)
    }

    #[test]
    fn list_request_uses_whitelisted_query() {
        let request = list_request(LearningProjectListInput {
            sort: Some(LearningProjectListSort::Recent),
            limit: Some(20),
            offset: Some(5),
        })
        .unwrap();
        assert_eq!(request.method, Method::GET);
        assert_eq!(request.path, "/learning/projects");
        assert_eq!(
            request.query,
            vec![
                ("sort".to_string(), "recent".to_string()),
                ("limit".to_string(), "20".to_string()),
                ("offset".to_string(), "5".to_string()),
            ]
        );
        assert!(request.body.is_none());
        assert!(!request.is_write);
    }

    #[test]
    fn list_rejects_unsafe_ranges() {
        assert_eq!(
            list_request(LearningProjectListInput {
                sort: None,
                limit: Some(0),
                offset: None,
            })
            .unwrap_err()
            .code,
            "validation"
        );
        assert_eq!(
            list_request(LearningProjectListInput {
                sort: None,
                limit: None,
                offset: Some(JS_SAFE_INTEGER_MAX + 1),
            })
            .unwrap_err()
            .code,
            "validation"
        );
    }

    #[test]
    fn project_id_injection_is_rejected_before_url_construction() {
        let error = get_request(LearningProjectIdInput {
            project_id: format!("{PROJECT_ID}/documents"),
        })
        .unwrap_err();
        assert_eq!(error.code, "learningProjectNotFound");
    }

    #[test]
    fn create_request_contains_only_allowed_fields() {
        let request = create_request(LearningProjectCreateInput {
            name: "项目".to_string(),
            learning_type: Some("exam".to_string()),
            course_name: Some("课程".to_string()),
            goal_summary: None,
            learning_goal: Some(serde_json::json!({"text":"目标"})),
            understanding: None,
            current_plan: None,
            progress: None,
            plan_adjustments: None,
        });
        let body = request.body.unwrap();
        assert_eq!(request.method, Method::POST);
        assert_eq!(request.path, "/learning/projects");
        assert_eq!(body["name"], "项目");
        assert_eq!(body["learningType"], "exam");
        assert!(body.get("ownerId").is_none());
        assert!(body.get("token").is_none());
        assert!(body.get("url").is_none());
    }

    #[test]
    fn update_request_preserves_missing_null_and_value_semantics() {
        let input: LearningProjectUpdateInput = serde_json::from_value(serde_json::json!({
            "projectId": PROJECT_ID,
            "expectedRevision": 3,
            "learningType": null,
            "courseName": "课程 B",
            "currentPlan": {"items": []}
        }))
        .unwrap();
        let request = update_request(input).unwrap();
        let body = request.body.unwrap();
        assert_eq!(request.method, Method::PATCH);
        assert_eq!(request.path, format!("/learning/projects/{PROJECT_ID}"));
        assert_eq!(body["expectedRevision"], 3);
        assert!(body.get("name").is_none());
        assert!(body["learningType"].is_null());
        assert_eq!(body["courseName"], "课程 B");
        assert_eq!(body["currentPlan"], serde_json::json!({"items": []}));
    }

    #[test]
    fn update_rejects_unsafe_revision() {
        let error = update_request(LearningProjectUpdateInput {
            project_id: PROJECT_ID.to_string(),
            expected_revision: JS_SAFE_INTEGER_MAX + 1,
            name: Some("项目".to_string()),
            learning_type: None,
            course_name: None,
            goal_summary: None,
            learning_goal: None,
            understanding: None,
            current_plan: None,
            progress: None,
            plan_adjustments: None,
        })
        .unwrap_err();
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn rename_delete_duplicate_and_open_use_fixed_paths_and_bodies() {
        let rename = rename_request(LearningProjectRenameInput {
            project_id: PROJECT_ID.to_string(),
            expected_revision: 2,
            name: "新名称".to_string(),
        })
        .unwrap();
        assert_eq!(rename.method, Method::PATCH);
        assert_eq!(rename.path, format!("/learning/projects/{PROJECT_ID}/name"));
        assert_eq!(rename.body.unwrap()["expectedRevision"], 2);

        let (delete, deleted_id) = delete_request(LearningProjectRevisionInput {
            project_id: PROJECT_ID.to_string(),
            expected_revision: 4,
        })
        .unwrap();
        assert_eq!(delete.method, Method::DELETE);
        assert_eq!(delete.path, format!("/learning/projects/{PROJECT_ID}"));
        assert_eq!(delete.body.unwrap()["expectedRevision"], 4);
        assert_eq!(deleted_id, PROJECT_ID);

        let duplicate = duplicate_request(LearningProjectDuplicateInput {
            project_id: PROJECT_ID.to_string(),
            name: Some("副本".to_string()),
        })
        .unwrap();
        assert_eq!(duplicate.method, Method::POST);
        assert_eq!(
            duplicate.path,
            format!("/learning/projects/{PROJECT_ID}/duplicate")
        );
        assert_eq!(duplicate.body.unwrap()["name"], "副本");

        let open = open_request(LearningProjectIdInput {
            project_id: PROJECT_ID.to_string(),
        })
        .unwrap();
        assert_eq!(open.method, Method::POST);
        assert_eq!(open.path, format!("/learning/projects/{PROJECT_ID}/open"));
        assert!(open.body.is_none());
    }

    #[test]
    fn deny_unknown_fields_blocks_owner_token_url_and_path() {
        for key in ["ownerId", "token", "accountServerUrl", "path"] {
            let value = serde_json::json!({
                "projectId": PROJECT_ID,
                "expectedRevision": 1,
                key: "unsafe"
            });
            assert!(
                serde_json::from_value::<LearningProjectRevisionInput>(value).is_err(),
                "{key} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn unauthenticated_session_does_not_send_request() {
        let request = get_request(LearningProjectIdInput {
            project_id: PROJECT_ID.to_string(),
        })
        .unwrap();
        let (result, remote) = execute_test_request(
            vec![Err(AccountLearningError::new(
                "signedOut",
                "Please sign in first",
            ))],
            project_response(),
            request,
            parse_project_response,
        )
        .await;
        assert_eq!(result.unwrap_err().code, "signedOut");
        assert!(remote.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn account_change_before_http_returns_account_changed_without_request() {
        let request = get_request(LearningProjectIdInput {
            project_id: PROJECT_ID.to_string(),
        })
        .unwrap();
        let (result, remote) = execute_test_request(
            vec![Ok(test_session("user-a")), Ok(test_session("user-b"))],
            project_response(),
            request,
            parse_project_response,
        )
        .await;
        assert_eq!(
            result.unwrap().status,
            AccountLearningEnvelopeStatus::AccountChanged
        );
        assert!(remote.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn successful_request_returns_completed_and_uses_initial_token() {
        let request = get_request(LearningProjectIdInput {
            project_id: PROJECT_ID.to_string(),
        })
        .unwrap();
        let (result, remote) = execute_test_request(
            vec![
                Ok(test_session("user-a")),
                Ok(test_session("user-a")),
                Ok(test_session("user-a")),
            ],
            project_response(),
            request,
            parse_project_response,
        )
        .await;
        let result = result.unwrap();
        assert_eq!(result.status, AccountLearningEnvelopeStatus::Completed);
        assert_eq!(result.data.unwrap().id, PROJECT_ID);
        assert_eq!(remote.requests.lock().unwrap().len(), 1);
        assert_eq!(remote.tokens.lock().unwrap()[0], b"session-token");
    }

    #[tokio::test]
    async fn get_request_uses_completed_account_changed_after_successful_account_switch() {
        let request = get_request(LearningProjectIdInput {
            project_id: PROJECT_ID.to_string(),
        })
        .unwrap();
        let (result, remote) = execute_test_request(
            vec![
                Ok(test_session("user-a")),
                Ok(test_session("user-a")),
                Ok(test_session("user-b")),
            ],
            project_response(),
            request,
            parse_project_response,
        )
        .await;
        let result = result.unwrap();
        assert_eq!(
            result.status,
            AccountLearningEnvelopeStatus::CompletedAccountChanged
        );
        assert_eq!(result.data.unwrap().id, PROJECT_ID);
        assert_eq!(remote.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn writes_also_return_completed_account_changed_after_successful_account_switch() {
        let request = create_request(LearningProjectCreateInput {
            name: "项目".to_string(),
            learning_type: None,
            course_name: None,
            goal_summary: None,
            learning_goal: None,
            understanding: None,
            current_plan: None,
            progress: None,
            plan_adjustments: None,
        });
        let (result, remote) = execute_test_request(
            vec![
                Ok(test_session("user-a")),
                Ok(test_session("user-a")),
                Ok(test_session("user-b")),
            ],
            project_response(),
            request,
            parse_project_response,
        )
        .await;
        assert_eq!(
            result.unwrap().status,
            AccountLearningEnvelopeStatus::CompletedAccountChanged
        );
        assert_eq!(remote.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn project_response_is_whitelisted_and_preserves_jsonb() {
        let project = parse_project_response(project_response()).unwrap();
        assert_eq!(project.id, PROJECT_ID);
        assert_eq!(project.revision, 1);
        assert_eq!(project.learning_goal, serde_json::json!({"text": "目标"}));
        let serialized = serde_json::to_value(project).unwrap();
        assert!(serialized.get("ownerUserId").is_none());
        assert!(serialized.get("storageKey").is_none());
        assert!(serialized.get("path").is_none());
    }

    #[test]
    fn project_list_response_is_whitelisted() {
        let list = parse_project_list_response(list_response()).unwrap();
        assert_eq!(list.projects.len(), 1);
        assert_eq!(list.projects[0].id, PROJECT_ID);
        let serialized = serde_json::to_value(list).unwrap();
        assert!(serialized["projects"][0].get("ownerUserId").is_none());
    }

    #[test]
    fn invalid_response_rejects_unsafe_revision_and_json_shapes() {
        let too_large_revision = serde_json::json!({
            "status": "ok",
            "project": project_json(PROJECT_ID, JS_SAFE_INTEGER_MAX + 1)
        });
        assert_eq!(
            parse_project_response(too_large_revision).unwrap_err().code,
            "invalidResponse"
        );

        let mut invalid_json = project_response();
        invalid_json["project"]["currentPlan"] = serde_json::json!([]);
        assert_eq!(
            parse_project_response(invalid_json).unwrap_err().code,
            "invalidResponse"
        );
    }

    #[test]
    fn delete_response_returns_only_project_id() {
        let deleted =
            parse_delete_response(serde_json::json!({"status": "ok"}), PROJECT_ID.to_string())
                .unwrap();
        assert_eq!(deleted.project_id, PROJECT_ID);
    }

    #[test]
    fn server_errors_map_to_stable_safe_codes() {
        assert_eq!(
            map_server_error(StatusCode::UNAUTHORIZED, Some("invalid_session"), false).code,
            "signedOut"
        );
        assert_eq!(
            map_server_error(
                StatusCode::NOT_FOUND,
                Some("learning_project_not_found"),
                false
            )
            .code,
            "learningProjectNotFound"
        );
        assert_eq!(
            map_server_error(
                StatusCode::CONFLICT,
                Some("learning_project_conflict"),
                true
            )
            .code,
            "learningProjectConflict"
        );
        assert_eq!(
            map_server_error(
                StatusCode::BAD_REQUEST,
                Some("invalid_learning_project_name"),
                true
            )
            .code,
            "validation"
        );
        let unavailable = map_server_error(StatusCode::INTERNAL_SERVER_ERROR, None, true);
        assert_eq!(unavailable.code, "unavailable");
        assert!(unavailable.message.contains("unknown"));
    }

    #[test]
    fn duplicate_omits_absent_name_and_trims_blank_optional_name() {
        let no_name = duplicate_request(LearningProjectDuplicateInput {
            project_id: OTHER_PROJECT_ID.to_string(),
            name: None,
        })
        .unwrap();
        assert_eq!(no_name.body.unwrap(), serde_json::json!({}));

        let blank_name = duplicate_request(LearningProjectDuplicateInput {
            project_id: OTHER_PROJECT_ID.to_string(),
            name: Some("   ".to_string()),
        })
        .unwrap();
        assert_eq!(blank_name.body.unwrap(), serde_json::json!({}));
    }
}
