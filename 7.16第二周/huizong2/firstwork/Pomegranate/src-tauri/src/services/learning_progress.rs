use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::database::Database;
use crate::error::AppError;

const LATEST_PROGRESS_KEY: &str = "learning_assistant.progress.latest";
const PROJECT_INDEX_KEY: &str = "learning_assistant.projects.index";
const PROJECT_KEY_PREFIX: &str = "learning_assistant.projects.project.";
const PROJECT_DATA_VERSION: &str = "2";

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProgressSaveInput {
    pub record: Value,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProgressSaveResult {
    pub saved_at: String,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProgressLoadResult {
    pub record: Option<Value>,
    pub message: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProgressClearResult {
    pub cleared: bool,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectIndex {
    pub version: String,
    pub current_project_id: Option<String>,
    pub migrated_latest: bool,
    pub projects: Vec<LearningProjectSummary>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectSummary {
    pub project_id: String,
    pub project_name: String,
    pub course_name: String,
    pub learning_goal: String,
    pub current_stage: String,
    pub progress_percent: u32,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectListResult {
    pub projects: Vec<LearningProjectSummary>,
    pub current_project_id: Option<String>,
    pub migrated_latest: bool,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectLoadInput {
    pub project_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectLoadResult {
    pub project: Option<Value>,
    pub summary: Option<LearningProjectSummary>,
    pub message: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectCreateInput {
    pub project_name: String,
    #[serde(default)]
    pub record: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectSaveInput {
    pub project_id: String,
    pub record: Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectRenameInput {
    pub project_id: String,
    pub project_name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectDeleteInput {
    pub project_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectDuplicateInput {
    pub project_id: String,
    pub project_name: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectSaveResult {
    pub project_id: String,
    pub saved_at: String,
    pub summary: LearningProjectSummary,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningProjectDeleteResult {
    pub deleted: bool,
    pub current_project_id: Option<String>,
    pub message: String,
}

pub struct LearningProgressService;

impl LearningProgressService {
    pub fn save_latest(
        db: &Database,
        input: LearningProgressSaveInput,
    ) -> Result<LearningProgressSaveResult, AppError> {
        let saved_at = now_string();
        let mut record = input.record;
        sanitize_secret_fields(&mut record);

        if let Value::Object(map) = &mut record {
            map.insert("updatedAt".to_string(), Value::String(saved_at.clone()));
            map.entry("version".to_string())
                .or_insert_with(|| Value::String("1".to_string()));
        }

        let text = serde_json::to_string_pretty(&record)?;
        db.set_config(LATEST_PROGRESS_KEY, &text)?;

        Ok(LearningProgressSaveResult {
            saved_at,
            message: "学习记录已保存".to_string(),
        })
    }

    pub fn load_latest(db: &Database) -> Result<LearningProgressLoadResult, AppError> {
        let Some(text) = db.get_config(LATEST_PROGRESS_KEY)? else {
            return Ok(LearningProgressLoadResult {
                record: None,
                message: "暂无历史学习记录".to_string(),
                error: None,
            });
        };

        match serde_json::from_str::<Value>(&text) {
            Ok(record) => Ok(LearningProgressLoadResult {
                record: Some(record),
                message: "已读取最近一次学习记录".to_string(),
                error: None,
            }),
            Err(error) => Ok(LearningProgressLoadResult {
                record: None,
                message: "学习记录已损坏，可清除后重新开始。".to_string(),
                error: Some(format!("学习记录 JSON 解析失败：{error}")),
            }),
        }
    }

    pub fn clear_latest(db: &Database) -> Result<LearningProgressClearResult, AppError> {
        let cleared = db.delete_config(LATEST_PROGRESS_KEY)?;
        Ok(LearningProgressClearResult {
            cleared,
            message: if cleared {
                "当前学习记录已清除".to_string()
            } else {
                "当前没有可清除的学习记录".to_string()
            },
        })
    }

    pub fn list_projects(db: &Database) -> Result<LearningProjectListResult, AppError> {
        let index = load_or_repair_index(db)?;
        Ok(LearningProjectListResult {
            projects: sorted_projects(index.projects),
            current_project_id: index.current_project_id,
            migrated_latest: index.migrated_latest,
            message: "已读取学习项目列表".to_string(),
        })
    }

    pub fn create_project(
        db: &Database,
        input: LearningProjectCreateInput,
    ) -> Result<LearningProjectSaveResult, AppError> {
        let project_name = input.project_name.trim();
        if project_name.is_empty() {
            return Err(AppError::InvalidInput("项目名称不能为空".to_string()));
        }

        let now = now_string();
        let project_id = new_project_id();
        let mut record = input.record.unwrap_or_else(|| Value::Object(Map::new()));
        sanitize_secret_fields(&mut record);
        set_project_record_field(&mut record, "projectId", project_id.clone());
        set_project_record_field(&mut record, "projectName", project_name.to_string());
        set_project_record_field(&mut record, "version", PROJECT_DATA_VERSION.to_string());
        set_project_record_field(&mut record, "createdAt", now.clone());
        set_project_record_field(&mut record, "updatedAt", now.clone());
        set_project_record_field(&mut record, "lastOpenedAt", now.clone());

        let summary = summary_from_record(&record, &project_id, project_name, &now);
        save_project_record(db, &project_id, &record)?;
        let mut index = load_or_repair_index(db)?;
        upsert_summary(&mut index, summary.clone());
        index.current_project_id = Some(project_id.clone());
        save_index(db, &index)?;

        Ok(LearningProjectSaveResult {
            project_id,
            saved_at: now,
            summary,
            message: "学习项目已创建".to_string(),
        })
    }

    pub fn load_project(
        db: &Database,
        input: LearningProjectLoadInput,
    ) -> Result<LearningProjectLoadResult, AppError> {
        let project_id = validate_project_id(&input.project_id)?;
        let Some(text) = db.get_config(&project_key(&project_id))? else {
            return Ok(LearningProjectLoadResult {
                project: None,
                summary: None,
                message: "学习项目不存在".to_string(),
                error: Some("project not found".to_string()),
            });
        };

        let mut record = match serde_json::from_str::<Value>(&text) {
            Ok(record) => record,
            Err(error) => {
                return Ok(LearningProjectLoadResult {
                    project: None,
                    summary: None,
                    message: "学习项目数据已损坏".to_string(),
                    error: Some(format!("项目 JSON 解析失败：{error}")),
                });
            }
        };

        let now = now_string();
        set_project_record_field(&mut record, "lastOpenedAt", now.clone());
        set_project_record_field(&mut record, "projectId", project_id.clone());
        save_project_record(db, &project_id, &record)?;

        let mut index = load_or_repair_index(db)?;
        let summary = summary_from_record(&record, &project_id, "", &now);
        upsert_summary(&mut index, summary.clone());
        index.current_project_id = Some(project_id);
        save_index(db, &index)?;

        Ok(LearningProjectLoadResult {
            project: Some(record),
            summary: Some(summary),
            message: "学习项目已打开".to_string(),
            error: None,
        })
    }

    pub fn save_project(
        db: &Database,
        input: LearningProjectSaveInput,
    ) -> Result<LearningProjectSaveResult, AppError> {
        let project_id = validate_project_id(&input.project_id)?;
        let now = now_string();
        let existing =
            load_project_value(db, &project_id)?.unwrap_or_else(|| Value::Object(Map::new()));
        let mut record = input.record;
        sanitize_secret_fields(&mut record);
        let project_name = value_string(&record, "projectName")
            .or_else(|| value_string(&existing, "projectName"))
            .unwrap_or_else(|| "未命名学习项目".to_string());
        let created_at = value_string(&existing, "createdAt")
            .or_else(|| value_string(&record, "createdAt"))
            .unwrap_or_else(|| now.clone());

        set_project_record_field(&mut record, "projectId", project_id.clone());
        set_project_record_field(&mut record, "projectName", project_name.clone());
        set_project_record_field(&mut record, "version", PROJECT_DATA_VERSION.to_string());
        set_project_record_field(&mut record, "createdAt", created_at);
        set_project_record_field(&mut record, "updatedAt", now.clone());
        set_project_record_field(&mut record, "lastOpenedAt", now.clone());

        let summary = summary_from_record(&record, &project_id, &project_name, &now);
        save_project_record(db, &project_id, &record)?;
        // Keep the old single-record key as a compatibility copy of the last saved project.
        db.set_config(LATEST_PROGRESS_KEY, &serde_json::to_string_pretty(&record)?)?;

        let mut index = load_or_repair_index(db)?;
        upsert_summary(&mut index, summary.clone());
        index.current_project_id = Some(project_id.clone());
        save_index(db, &index)?;

        Ok(LearningProjectSaveResult {
            project_id,
            saved_at: now,
            summary,
            message: "学习项目已保存".to_string(),
        })
    }

    pub fn rename_project(
        db: &Database,
        input: LearningProjectRenameInput,
    ) -> Result<LearningProjectSaveResult, AppError> {
        let project_id = validate_project_id(&input.project_id)?;
        let project_name = input.project_name.trim();
        if project_name.is_empty() {
            return Err(AppError::InvalidInput("项目名称不能为空".to_string()));
        }
        let mut record = load_project_value(db, &project_id)?
            .ok_or_else(|| AppError::Custom("学习项目不存在".to_string()))?;
        set_project_record_field(&mut record, "projectName", project_name.to_string());
        Self::save_project(db, LearningProjectSaveInput { project_id, record })
    }

    pub fn delete_project(
        db: &Database,
        input: LearningProjectDeleteInput,
    ) -> Result<LearningProjectDeleteResult, AppError> {
        let project_id = validate_project_id(&input.project_id)?;
        let deleted = db.delete_config(&project_key(&project_id))?;
        let mut index = load_or_repair_index(db)?;
        index.projects.retain(|item| item.project_id != project_id);
        if index.current_project_id.as_deref() == Some(&project_id) {
            index.current_project_id = sorted_projects(index.projects.clone())
                .first()
                .map(|item| item.project_id.clone());
        }
        save_index(db, &index)?;
        Ok(LearningProjectDeleteResult {
            deleted,
            current_project_id: index.current_project_id,
            message: if deleted {
                "学习项目已删除".to_string()
            } else {
                "学习项目不存在或已删除".to_string()
            },
        })
    }

    pub fn duplicate_project(
        db: &Database,
        input: LearningProjectDuplicateInput,
    ) -> Result<LearningProjectSaveResult, AppError> {
        let source_id = validate_project_id(&input.project_id)?;
        let project_name = input.project_name.trim();
        if project_name.is_empty() {
            return Err(AppError::InvalidInput("项目名称不能为空".to_string()));
        }
        let mut record = load_project_value(db, &source_id)?
            .ok_or_else(|| AppError::Custom("源学习项目不存在".to_string()))?;
        let now = now_string();
        let project_id = new_project_id();
        sanitize_secret_fields(&mut record);
        set_project_record_field(&mut record, "projectId", project_id.clone());
        set_project_record_field(&mut record, "projectName", project_name.to_string());
        set_project_record_field(&mut record, "version", PROJECT_DATA_VERSION.to_string());
        set_project_record_field(&mut record, "createdAt", now.clone());
        set_project_record_field(&mut record, "updatedAt", now.clone());
        set_project_record_field(&mut record, "lastOpenedAt", now.clone());

        let summary = summary_from_record(&record, &project_id, project_name, &now);
        save_project_record(db, &project_id, &record)?;
        let mut index = load_or_repair_index(db)?;
        upsert_summary(&mut index, summary.clone());
        index.current_project_id = Some(project_id.clone());
        save_index(db, &index)?;

        Ok(LearningProjectSaveResult {
            project_id,
            saved_at: now,
            summary,
            message: "学习项目已复制".to_string(),
        })
    }
}

fn load_or_repair_index(db: &Database) -> Result<LearningProjectIndex, AppError> {
    let mut index = match db.get_config(PROJECT_INDEX_KEY)? {
        Some(text) => serde_json::from_str::<LearningProjectIndex>(&text)
            .unwrap_or_else(|_| rebuild_index_from_project_records(db).unwrap_or_default()),
        None => LearningProjectIndex::default(),
    };

    if index.version.is_empty() {
        index.version = PROJECT_DATA_VERSION.to_string();
    }

    if !index.migrated_latest {
        migrate_latest_record(db, &mut index)?;
    }

    index.projects = sorted_projects(index.projects);
    save_index(db, &index)?;
    Ok(index)
}

fn rebuild_index_from_project_records(db: &Database) -> Result<LearningProjectIndex, AppError> {
    let mut projects = Vec::new();
    for config in db.get_all_config()? {
        if let Some(project_id) = config.key.strip_prefix(PROJECT_KEY_PREFIX) {
            if let Ok(record) = serde_json::from_str::<Value>(&config.value) {
                projects.push(summary_from_record(&record, project_id, "", &now_string()));
            }
        }
    }
    Ok(LearningProjectIndex {
        version: PROJECT_DATA_VERSION.to_string(),
        current_project_id: projects.first().map(|item| item.project_id.clone()),
        migrated_latest: false,
        projects,
    })
}

fn migrate_latest_record(db: &Database, index: &mut LearningProjectIndex) -> Result<(), AppError> {
    if let Some(text) = db.get_config(LATEST_PROGRESS_KEY)? {
        if let Ok(mut record) = serde_json::from_str::<Value>(&text) {
            let already_migrated = value_string(&record, "projectId")
                .map(|id| index.projects.iter().any(|item| item.project_id == id))
                .unwrap_or(false);
            if !already_migrated {
                let now = now_string();
                let project_id = new_project_id();
                let project_name = infer_project_name(&record);
                sanitize_secret_fields(&mut record);
                set_project_record_field(&mut record, "projectId", project_id.clone());
                set_project_record_field(&mut record, "projectName", project_name.clone());
                set_project_record_field(&mut record, "version", PROJECT_DATA_VERSION.to_string());
                if value_string(&record, "createdAt").is_none() {
                    set_project_record_field(&mut record, "createdAt", now.clone());
                }
                set_project_record_field(&mut record, "updatedAt", now.clone());
                set_project_record_field(&mut record, "lastOpenedAt", now.clone());
                set_project_record_field(&mut record, "migratedFromLatest", "true".to_string());
                save_project_record(db, &project_id, &record)?;
                upsert_summary(
                    index,
                    summary_from_record(&record, &project_id, &project_name, &now),
                );
                index.current_project_id = Some(project_id);
            }
        }
    }
    index.migrated_latest = true;
    Ok(())
}

fn save_index(db: &Database, index: &LearningProjectIndex) -> Result<(), AppError> {
    db.set_config(PROJECT_INDEX_KEY, &serde_json::to_string_pretty(index)?)?;
    Ok(())
}

fn save_project_record(db: &Database, project_id: &str, record: &Value) -> Result<(), AppError> {
    db.set_config(
        &project_key(project_id),
        &serde_json::to_string_pretty(record)?,
    )?;
    Ok(())
}

fn load_project_value(db: &Database, project_id: &str) -> Result<Option<Value>, AppError> {
    let Some(text) = db.get_config(&project_key(project_id))? else {
        return Ok(None);
    };
    serde_json::from_str::<Value>(&text)
        .map(Some)
        .map_err(AppError::from)
}

fn project_key(project_id: &str) -> String {
    format!("{PROJECT_KEY_PREFIX}{project_id}")
}

fn validate_project_id(project_id: &str) -> Result<String, AppError> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err(AppError::InvalidInput(
            "projectId cannot be empty".to_string(),
        ));
    }
    if !project_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(AppError::InvalidInput(
            "projectId contains invalid characters".to_string(),
        ));
    }
    Ok(project_id.to_string())
}

fn upsert_summary(index: &mut LearningProjectIndex, summary: LearningProjectSummary) {
    if let Some(existing) = index
        .projects
        .iter_mut()
        .find(|item| item.project_id == summary.project_id)
    {
        *existing = summary;
    } else {
        index.projects.push(summary);
    }
    index.projects = sorted_projects(index.projects.clone());
}

fn sorted_projects(mut projects: Vec<LearningProjectSummary>) -> Vec<LearningProjectSummary> {
    projects.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.last_opened_at.cmp(&a.last_opened_at))
    });
    projects
}

fn summary_from_record(
    record: &Value,
    project_id: &str,
    fallback_name: &str,
    fallback_time: &str,
) -> LearningProjectSummary {
    let project_name = value_string(record, "projectName")
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            if fallback_name.trim().is_empty() {
                infer_project_name(record)
            } else {
                fallback_name.to_string()
            }
        });
    let course_name = nested_string(record, &["goal", "course"])
        .or_else(|| value_string(record, "courseName"))
        .unwrap_or_else(|| "机械制造工艺学".to_string());
    let learning_goal = nested_string(record, &["goal", "learningGoal"])
        .or_else(|| value_string(record, "learningGoal"))
        .unwrap_or_default();
    let current_stage = current_stage_name(record);
    let progress_percent = progress_percent(record);
    LearningProjectSummary {
        project_id: project_id.to_string(),
        project_name,
        course_name,
        learning_goal,
        current_stage,
        progress_percent,
        created_at: value_string(record, "createdAt").unwrap_or_else(|| fallback_time.to_string()),
        updated_at: value_string(record, "updatedAt").unwrap_or_else(|| fallback_time.to_string()),
        last_opened_at: value_string(record, "lastOpenedAt")
            .unwrap_or_else(|| fallback_time.to_string()),
    }
}

fn infer_project_name(record: &Value) -> String {
    let course = nested_string(record, &["goal", "course"])
        .or_else(|| value_string(record, "courseName"))
        .unwrap_or_else(|| "机械制造工艺学".to_string());
    let goal = nested_string(record, &["goal", "learningGoal"])
        .or_else(|| value_string(record, "learningGoal"))
        .unwrap_or_default();
    if goal.trim().is_empty() {
        format!("{course}学习项目")
    } else {
        format!("{course}-{goal}")
    }
}

fn current_stage_name(record: &Value) -> String {
    let index = value_u64(record, "currentStageIndex").unwrap_or(0) as usize;
    record
        .get("plan")
        .and_then(|plan| plan.get("stages"))
        .and_then(Value::as_array)
        .and_then(|stages| stages.get(index))
        .and_then(|stage| stage.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "未开始".to_string())
}

fn progress_percent(record: &Value) -> u32 {
    let Some(statuses) = record.get("stageStatuses").and_then(Value::as_array) else {
        return 0;
    };
    if statuses.is_empty() {
        return 0;
    }
    let completed = statuses
        .iter()
        .filter(|status| {
            status
                .get("status")
                .and_then(Value::as_str)
                .map(|value| value == "completed")
                .unwrap_or(false)
        })
        .count();
    ((completed as f64 / statuses.len() as f64) * 100.0).round() as u32
}

fn set_project_record_field(record: &mut Value, key: &str, value: String) {
    if !record.is_object() {
        *record = Value::Object(Map::new());
    }
    if let Value::Object(map) = record {
        map.insert(key.to_string(), Value::String(value));
    }
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn value_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn nested_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn new_project_id() -> String {
    let now = Local::now();
    let nanos = now
        .timestamp_nanos_opt()
        .unwrap_or_else(|| now.timestamp_millis() * 1_000_000);
    format!("project-{nanos}")
}

fn now_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn sanitize_secret_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let normalized = key.to_lowercase().replace(['_', '-'], "");
                if normalized.contains("apikey")
                    || normalized.contains("password")
                    || normalized.contains("apisecret")
                    || normalized.contains("authorization")
                    || normalized.contains("bearer")
                    || normalized.contains("token")
                    || normalized.contains("credential")
                {
                    map.remove(&key);
                    continue;
                }
                if let Some(child) = map.get_mut(&key) {
                    sanitize_secret_fields(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                sanitize_secret_fields(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod project_isolation_tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn projects_are_isolated_and_secrets_are_removed_before_persistence() {
        let data_dir = std::env::temp_dir().join(format!(
            "firstwork-learning-projects-{}",
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&data_dir).expect("test data directory should be created");
        let db_path = data_dir.join("learning-projects.db");
        let db = Database::init(db_path.to_string_lossy().as_ref())
            .expect("test database should initialize");

        let first = LearningProgressService::create_project(
            &db,
            LearningProjectCreateInput {
                project_name: "项目 A".to_string(),
                record: Some(json!({
                    "goal": {"course": "机械制造工艺学", "learningGoal": "目标 A"},
                    "stageQuizzes": {"0": {"scoreResult": {"totalScore": 80}}},
                    "wrongQuestionReviewPrompts": [{"id": "wrong-a"}],
                    "runtime": {"apiPassword": "TEST_SECRET_MUST_NOT_PERSIST"}
                })),
            },
        )
        .expect("project A should be created");
        let second = LearningProgressService::create_project(
            &db,
            LearningProjectCreateInput {
                project_name: "项目 B".to_string(),
                record: Some(json!({
                    "goal": {"course": "机械制造工艺学", "learningGoal": "目标 B"},
                    "stageQuizzes": {"0": {"scoreResult": {"totalScore": 35}}},
                    "wrongQuestionReviewPrompts": [{"id": "wrong-b"}]
                })),
            },
        )
        .expect("project B should be created");

        let loaded_first = LearningProgressService::load_project(
            &db,
            LearningProjectLoadInput {
                project_id: first.project_id,
            },
        )
        .expect("project A should load")
        .project
        .expect("project A should exist");
        let loaded_second = LearningProgressService::load_project(
            &db,
            LearningProjectLoadInput {
                project_id: second.project_id,
            },
        )
        .expect("project B should load")
        .project
        .expect("project B should exist");

        assert_eq!(
            nested_string(&loaded_first, &["goal", "learningGoal"]).as_deref(),
            Some("目标 A")
        );
        assert_eq!(
            nested_string(&loaded_second, &["goal", "learningGoal"]).as_deref(),
            Some("目标 B")
        );
        assert!(loaded_first.to_string().contains("wrong-a"));
        assert!(!loaded_first.to_string().contains("wrong-b"));
        assert!(!loaded_first
            .to_string()
            .contains("TEST_SECRET_MUST_NOT_PERSIST"));

        drop(db);
        let _ = fs::remove_dir_all(data_dir);
    }
}
