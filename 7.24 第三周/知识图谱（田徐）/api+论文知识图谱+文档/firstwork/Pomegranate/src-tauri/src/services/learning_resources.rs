use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const DEMO_RESOURCES_JSON: &str =
    include_str!("../../../../learning-assistant/templates/demo_resources.json");
const RESOURCE_TABLE: &str = "learning_resources";

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningResourcesRecommendInput {
    pub course: String,
    #[serde(alias = "stage_name")]
    pub stage_name: String,
    #[serde(alias = "stage_index")]
    pub stage_index: usize,
    #[serde(default)]
    #[serde(alias = "knowledge_points")]
    pub knowledge_points: Vec<String>,
    pub level: String,
    #[serde(default = "default_task_type")]
    #[serde(alias = "task_type")]
    pub task_type: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningResource {
    #[serde(alias = "resource_id")]
    pub resource_id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub course: String,
    #[serde(alias = "knowledge_point")]
    pub knowledge_point: String,
    pub difficulty: String,
    pub url: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub duration: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningResourcesRecommendResult {
    pub resources: Vec<LearningResource>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub source: String,
}

pub struct LearningResourcesService;

impl LearningResourcesService {
    pub async fn recommend(
        input: LearningResourcesRecommendInput,
    ) -> Result<LearningResourcesRecommendResult, AppError> {
        let input = normalize_input(input)?;
        let db_config = LearningResourceDbConfig::from_env();

        if let Some(config) = db_config {
            match query_configured_database(&config, &input).await {
                Ok(resources) => {
                    let resources = rank_and_limit(resources, &input);
                    let message = if resources.is_empty() {
                        "当前数据库暂无匹配资源".to_string()
                    } else {
                        "已从学习资源数据库返回推荐资源".to_string()
                    };
                    return Ok(LearningResourcesRecommendResult {
                        resources,
                        message,
                        source: config.db_type,
                    });
                }
                Err(error) => {
                    log::warn!(
                        "[learning_resources] database query failed, fallback to demo: {error}"
                    );
                }
            }
        }

        let resources = load_demo_resources()?;
        let resources = rank_and_limit(resources, &input);
        let message = if resources.is_empty() {
            "当前数据库暂无匹配资源，demo 数据中也没有可匹配资源".to_string()
        } else {
            "未配置或无法连接学习资源数据库，已使用 demo_resources.json 演示资源".to_string()
        };

        Ok(LearningResourcesRecommendResult {
            resources,
            message,
            source: "demo".to_string(),
        })
    }
}

#[derive(Debug, Clone)]
struct LearningResourceDbConfig {
    db_type: String,
    url: String,
    user: Option<String>,
    password: Option<String>,
}

impl LearningResourceDbConfig {
    fn from_env() -> Option<Self> {
        let db_type = env_trim("LEARNING_DB_TYPE");
        let url = env_trim("LEARNING_DB_URL");

        match (db_type, url) {
            (Some(db_type), Some(url)) => Some(Self {
                db_type: db_type.to_lowercase(),
                url,
                user: env_trim("LEARNING_DB_USER"),
                password: env_trim("LEARNING_DB_PASSWORD"),
            }),
            _ => None,
        }
    }
}

async fn query_configured_database(
    config: &LearningResourceDbConfig,
    input: &LearningResourcesRecommendInput,
) -> Result<Vec<LearningResource>, AppError> {
    match config.db_type.as_str() {
        "sqlite" | "sqlite3" => query_sqlite_resources(&config.url, input),
        "http" | "https" | "api" => query_http_resources(config, input).await,
        other => Err(AppError::Custom(format!(
            "Unsupported LEARNING_DB_TYPE: {other}"
        ))),
    }
}

fn query_sqlite_resources(
    db_url: &str,
    input: &LearningResourcesRecommendInput,
) -> Result<Vec<LearningResource>, AppError> {
    let db_path = db_url
        .strip_prefix("sqlite://")
        .or_else(|| db_url.strip_prefix("sqlite:"))
        .unwrap_or(db_url);
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT
            resource_id,
            title,
            type,
            course,
            knowledge_point,
            difficulty,
            url,
            summary,
            tags,
            duration
         FROM {RESOURCE_TABLE}
         WHERE course = ?1
         LIMIT 100"
    ))?;

    let rows = stmt.query_map([input.course.as_str()], |row| {
        let url: Option<String> = row.get(6)?;
        let tags_text: Option<String> = row.get(8)?;
        let duration: Option<String> = row.get(9)?;
        Ok(LearningResource {
            resource_id: row.get(0)?,
            title: row.get(1)?,
            resource_type: row.get(2)?,
            course: row.get(3)?,
            knowledge_point: row.get(4)?,
            difficulty: row.get(5)?,
            url: url.unwrap_or_default(),
            summary: row.get(7)?,
            tags: tags_text
                .as_deref()
                .map(parse_tags)
                .unwrap_or_else(Vec::new),
            duration: duration.unwrap_or_default(),
            reason: String::new(),
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

async fn query_http_resources(
    config: &LearningResourceDbConfig,
    input: &LearningResourcesRecommendInput,
) -> Result<Vec<LearningResource>, AppError> {
    let client = reqwest::Client::new();
    let mut request = client.post(&config.url).json(input);
    if let (Some(user), Some(password)) = (&config.user, &config.password) {
        request = request.basic_auth(user, Some(password));
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("学习资源接口请求失败: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Custom(format!(
            "学习资源接口返回非成功状态: {status}"
        )));
    }

    let result = response
        .json::<LearningResourcesRecommendResult>()
        .await
        .map_err(|e| AppError::Custom(format!("学习资源接口响应解析失败: {e}")))?;

    Ok(result.resources)
}

fn normalize_input(
    mut input: LearningResourcesRecommendInput,
) -> Result<LearningResourcesRecommendInput, AppError> {
    input.course = input.course.trim().to_string();
    input.stage_name = input.stage_name.trim().to_string();
    input.level = input.level.trim().to_string();
    input.task_type = clean_or(&input.task_type, "resource");
    input.limit = input.limit.clamp(1, 3);
    input.knowledge_points = input
        .knowledge_points
        .into_iter()
        .map(|point| point.trim().to_string())
        .filter(|point| !point.is_empty())
        .collect();

    if input.course.is_empty() {
        return Err(AppError::InvalidInput("course cannot be empty".to_string()));
    }
    if input.stage_name.is_empty() {
        input.stage_name = format!("阶段 {}", input.stage_index);
    }

    Ok(input)
}

fn rank_and_limit(
    resources: Vec<LearningResource>,
    input: &LearningResourcesRecommendInput,
) -> Vec<LearningResource> {
    let course = input.course.to_lowercase();
    let preferred_difficulty = preferred_difficulty(&input.level);
    let knowledge_points: Vec<String> = input
        .knowledge_points
        .iter()
        .map(|point| point.to_lowercase())
        .collect();

    let mut scored = resources
        .into_iter()
        .filter(|resource| resource.course.to_lowercase() == course)
        .map(|mut resource| {
            let mut score = 0;
            let knowledge = resource.knowledge_point.to_lowercase();
            let title = resource.title.to_lowercase();
            let summary = resource.summary.to_lowercase();
            let difficulty = normalize_difficulty(&resource.difficulty);

            if knowledge_points.iter().any(|point| {
                !point.is_empty()
                    && (knowledge.contains(point)
                        || point.contains(&knowledge)
                        || title.contains(point)
                        || summary.contains(point))
            }) {
                score += 40;
            }
            if difficulty == preferred_difficulty {
                score += 30;
            }
            score += match resource.resource_type.as_str() {
                "video" => 12,
                "article" => 10,
                "courseware" => 9,
                "question_bank" => 8,
                _ => 4,
            };
            if resource.stage_matches(input.stage_index, &input.stage_name) {
                score += 8;
            }

            resource.reason = build_reason(&resource, input, difficulty == preferred_difficulty);
            (score, resource)
        })
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    scored
        .into_iter()
        .take(input.limit)
        .map(|(_, resource)| resource)
        .collect()
}

fn load_demo_resources() -> Result<Vec<LearningResource>, AppError> {
    if let Some(path) = demo_resources_path() {
        if let Ok(text) = fs::read_to_string(path) {
            return parse_resource_list(&text);
        }
    }
    parse_resource_list(DEMO_RESOURCES_JSON)
}

fn demo_resources_path() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    [
        current.join("learning-assistant/templates/demo_resources.json"),
        current.join("../learning-assistant/templates/demo_resources.json"),
        current.join("../../learning-assistant/templates/demo_resources.json"),
        current.join("../../../learning-assistant/templates/demo_resources.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn parse_resource_list(text: &str) -> Result<Vec<LearningResource>, AppError> {
    #[derive(Deserialize)]
    struct ResourceList {
        resources: Vec<LearningResource>,
    }

    if let Ok(list) = serde_json::from_str::<ResourceList>(text) {
        return Ok(list.resources);
    }
    serde_json::from_str::<Vec<LearningResource>>(text).map_err(AppError::from)
}

fn build_reason(
    resource: &LearningResource,
    input: &LearningResourcesRecommendInput,
    difficulty_matched: bool,
) -> String {
    let difficulty_text = if difficulty_matched {
        "难度适合当前基础"
    } else {
        "可作为当前阶段的补充材料"
    };
    format!(
        "该资源匹配「{}」阶段的知识点「{}」，{}。",
        input.stage_name, resource.knowledge_point, difficulty_text
    )
}

fn preferred_difficulty(level: &str) -> &'static str {
    let level = level.to_lowercase();
    if ["零基础", "基础较差", "较差", "入门", "beginner", "weak"]
        .iter()
        .any(|keyword| level.contains(keyword))
    {
        "easy"
    } else if ["较好", "好", "提高", "hard", "advanced"]
        .iter()
        .any(|keyword| level.contains(keyword))
    {
        "hard"
    } else {
        "medium"
    }
}

fn normalize_difficulty(difficulty: &str) -> &'static str {
    let difficulty = difficulty.to_lowercase();
    if ["easy", "入门", "基础", "简单"]
        .iter()
        .any(|keyword| difficulty.contains(keyword))
    {
        "easy"
    } else if ["hard", "提高", "进阶", "困难", "综合"]
        .iter()
        .any(|keyword| difficulty.contains(keyword))
    {
        "hard"
    } else {
        "medium"
    }
}

fn parse_tags(tags_text: &str) -> Vec<String> {
    if let Ok(tags) = serde_json::from_str::<Vec<String>>(tags_text) {
        return tags;
    }
    tags_text
        .split([',', '，', ';', '；'])
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect()
}

fn clean_or(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn env_trim(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_task_type() -> String {
    "resource".to_string()
}

fn default_limit() -> usize {
    3
}

trait StageMatch {
    fn stage_matches(&self, stage_index: usize, stage_name: &str) -> bool;
}

impl StageMatch for LearningResource {
    fn stage_matches(&self, stage_index: usize, stage_name: &str) -> bool {
        let stage_index_text = stage_index.to_string();
        let stage_name = stage_name.to_lowercase();
        self.tags.iter().any(|tag| {
            let tag = tag.to_lowercase();
            tag.contains(&stage_index_text) || stage_name.contains(&tag)
        })
    }
}
