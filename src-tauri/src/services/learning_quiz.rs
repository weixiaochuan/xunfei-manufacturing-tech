use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;

const DEMO_QUESTIONS_JSON: &str =
    include_str!("../../resources/learning-assistant/full/templates/demo_questions.json");
const QUESTION_TABLE: &str = "learning_quiz_questions";
const FORMAL_QUESTION_BANK_RELATIVE_PATH: &str =
    "files.v21_最终/question_bank_system/db/question_bank.db";
const MAX_FORMAL_ASSET_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningQuizGetQuestionsInput {
    pub course: String,
    #[serde(alias = "stage_name")]
    pub stage_name: String,
    #[serde(alias = "stage_index")]
    pub stage_index: usize,
    #[serde(default, alias = "knowledge_points")]
    pub knowledge_points: Vec<String>,
    pub level: String,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningQuizQuestion {
    #[serde(alias = "question_id")]
    pub question_id: String,
    pub course: String,
    #[serde(alias = "knowledge_point")]
    pub knowledge_point: String,
    #[serde(rename = "type")]
    pub question_type: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(alias = "standard_answer")]
    pub standard_answer: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub score: u32,
    pub difficulty: String,
    pub explanation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_image: Option<String>,
    // The answer image is populated only after the backend reloads an approved answer.
    #[serde(skip)]
    pub answer_image: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningQuizQuestionsResult {
    pub questions: Vec<LearningQuizQuestion>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningQuizAnswer {
    pub question_id: String,
    pub user_answer: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningQuizScoreInput {
    pub stage_name: String,
    pub stage_index: usize,
    #[serde(default)]
    pub student_id: Option<String>,
    pub questions: Vec<LearningQuizQuestion>,
    pub answers: Vec<LearningQuizAnswer>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningQuizScoreResult {
    pub total_score: u32,
    pub max_score: u32,
    pub level: String,
    pub weak_points: Vec<String>,
    pub missing_keywords: Vec<String>,
    pub feedback: String,
    pub suggestions: Vec<String>,
    pub can_go_next: bool,
    pub detail_results: Vec<LearningQuizDetailResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningQuizDetailResult {
    pub question_id: String,
    pub user_answer: String,
    pub standard_answer: String,
    pub score: u32,
    pub max_score: u32,
    pub correct: bool,
    pub missing_keywords: Vec<String>,
    pub comment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_image: Option<String>,
}

pub struct LearningQuizService;

impl LearningQuizService {
    pub async fn get_questions(
        input: LearningQuizGetQuestionsInput,
    ) -> Result<LearningQuizQuestionsResult, AppError> {
        let input = normalize_input(input)?;
        if let Some(api_url) = question_bank_api_url() {
            match query_external_question_bank(&api_url, &input).await {
                Ok(questions) => {
                    let questions = rank_and_limit(questions, &input);
                    if !questions.is_empty() {
                        return Ok(LearningQuizQuestionsResult {
                            questions,
                            message: "已从外部出题系统返回阶段测试题目".to_string(),
                            source: "question_bank_api".to_string(),
                        });
                    }
                    log::warn!(
                        "[learning_quiz] external question bank returned no matched questions, fallback"
                    );
                }
                Err(error) => {
                    log::warn!("[learning_quiz] external question bank failed, fallback: {error}");
                }
            }
        }

        if let Some(db_path) = question_bank_db_path_from_env()? {
            let questions = query_formal_sqlite_questions(&db_path, &input)?;
            let questions = rank_and_limit(questions, &input);
            let message = if questions.is_empty() {
                "正式《机械制造工艺学》题库中暂无与本阶段匹配的题目".to_string()
            } else {
                "已从正式《机械制造工艺学》SQLite 题库返回阶段测试题目".to_string()
            };
            return Ok(LearningQuizQuestionsResult {
                questions,
                message,
                source: "formal_sqlite".to_string(),
            });
        }

        let db_config = LearningQuizDbConfig::from_env();

        if let Some(config) = db_config {
            match query_configured_database(&config, &input).await {
                Ok(questions) => {
                    let questions = rank_and_limit(questions, &input);
                    let message = if questions.is_empty() {
                        "当前题库暂无匹配题目".to_string()
                    } else {
                        "已从学习题库数据库返回阶段测试题目".to_string()
                    };
                    return Ok(LearningQuizQuestionsResult {
                        questions,
                        message,
                        source: config.db_type,
                    });
                }
                Err(error) => {
                    log::warn!("[learning_quiz] database query failed, fallback to demo: {error}");
                }
            }
        }

        if let Some(db_path) = bundled_formal_question_bank_path()? {
            let questions = query_formal_sqlite_questions(&db_path, &input)?;
            let questions = rank_and_limit(questions, &input);
            let message = if questions.is_empty() {
                "正式《机械制造工艺学》题库中暂无与本阶段匹配的题目".to_string()
            } else {
                "已从项目内正式《机械制造工艺学》SQLite 题库返回阶段测试题目".to_string()
            };
            return Ok(LearningQuizQuestionsResult {
                questions,
                message,
                source: "formal_sqlite".to_string(),
            });
        }

        if !allow_demo_quiz_fallback() {
            return Err(AppError::Custom(
                "未找到正式《机械制造工艺学》题库 question_bank.db；桌面版默认不再使用 demo 题库，请检查 files.v21_最终/question_bank_system/db/question_bank.db 是否随项目一起复制。"
                    .to_string(),
            ));
        }

        let questions = load_demo_questions()?;
        let questions = rank_and_limit(questions, &input);
        let message = if questions.is_empty() {
            "当前题库暂无匹配题目，demo 题库中也没有可匹配题目".to_string()
        } else {
            "未配置或无法连接学习题库数据库，已使用 demo_questions.json 演示题目".to_string()
        };

        Ok(LearningQuizQuestionsResult {
            questions,
            message,
            source: "demo".to_string(),
        })
    }

    pub async fn score(
        mut input: LearningQuizScoreInput,
    ) -> Result<LearningQuizScoreResult, AppError> {
        if let Some(api_url) = question_bank_api_url() {
            match score_with_external_question_bank(&api_url, &input).await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    log::warn!("[learning_quiz] external scoring failed, fallback: {error}");
                }
            }
        }

        if let Some(db_path) = formal_question_bank_path_for_scoring()? {
            hydrate_formal_answers_for_scoring(&db_path, &mut input)?;
        }
        score_locally(input)
    }
}

fn score_locally(input: LearningQuizScoreInput) -> Result<LearningQuizScoreResult, AppError> {
    if input.questions.is_empty() {
        return Err(AppError::InvalidInput(
            "questions cannot be empty".to_string(),
        ));
    }

    let answers: HashMap<String, String> = input
        .answers
        .into_iter()
        .map(|answer| (answer.question_id, answer.user_answer))
        .collect();

    let mut total_score = 0;
    let mut max_score = 0;
    let mut weak_points = Vec::new();
    let mut missing_keywords = Vec::new();
    let mut detail_results = Vec::new();

    for question in input.questions {
        let user_answer = answers
            .get(&question.question_id)
            .cloned()
            .unwrap_or_default();
        let detail = score_question(&question, &user_answer);
        max_score += detail.max_score;
        total_score += detail.score;

        if detail.score < detail.max_score {
            push_unique(&mut weak_points, question.knowledge_point.clone());
        }
        for keyword in &detail.missing_keywords {
            push_unique(&mut missing_keywords, keyword.clone());
        }

        detail_results.push(detail);
    }

    let percent = score_percentage(total_score, max_score);
    let (level, feedback) = score_band(percent);
    let can_go_next = percent >= 60;
    let suggestions = build_suggestions(percent, &weak_points);

    Ok(LearningQuizScoreResult {
        total_score,
        max_score,
        level,
        weak_points,
        missing_keywords,
        feedback,
        suggestions,
        can_go_next,
        detail_results,
    })
}

#[derive(Debug, Clone)]
struct LearningQuizDbConfig {
    db_type: String,
    url: String,
    user: Option<String>,
    password: Option<String>,
}

impl LearningQuizDbConfig {
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
    config: &LearningQuizDbConfig,
    input: &LearningQuizGetQuestionsInput,
) -> Result<Vec<LearningQuizQuestion>, AppError> {
    match config.db_type.as_str() {
        "sqlite" | "sqlite3" => query_sqlite_questions(&config.url, input),
        "http" | "https" | "api" => query_http_questions(config, input).await,
        other => Err(AppError::Custom(format!(
            "Unsupported LEARNING_DB_TYPE: {other}"
        ))),
    }
}

async fn query_external_question_bank(
    api_url: &str,
    input: &LearningQuizGetQuestionsInput,
) -> Result<Vec<LearningQuizQuestion>, AppError> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/questions", api_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("external question request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Custom(format!(
            "external question service returned {status}"
        )));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|e| AppError::Custom(format!("external question response parse failed: {e}")))?;
    let items = external_items_from_value(&value)?;
    let questions = items
        .into_iter()
        .filter_map(|item| map_external_question(item, input))
        .collect();
    Ok(questions)
}

async fn score_with_external_question_bank(
    api_url: &str,
    input: &LearningQuizScoreInput,
) -> Result<LearningQuizScoreResult, AppError> {
    if input.questions.is_empty() {
        return Err(AppError::InvalidInput(
            "questions cannot be empty".to_string(),
        ));
    }

    let client = reqwest::Client::new();
    let url = format!("{}/api/answer", api_url.trim_end_matches('/'));
    let student_id = required_external_student_id(input)?;
    let answers: HashMap<String, String> = input
        .answers
        .iter()
        .map(|answer| (answer.question_id.clone(), answer.user_answer.clone()))
        .collect();

    let mut total_score = 0;
    let mut max_score = 0;
    let mut weak_points = Vec::new();
    let mut missing_keywords = Vec::new();
    let mut detail_results = Vec::new();

    for question in &input.questions {
        let user_answer = answers
            .get(&question.question_id)
            .cloned()
            .unwrap_or_default();
        let payload = serde_json::json!({
            "question_id": question.question_id,
            "answer": user_answer,
            "student_id": student_id,
            "mode": "测试"
        });
        let response = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Custom(format!("external answer request failed: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(AppError::Custom(format!(
                "external answer service returned {status}"
            )));
        }

        let value = response
            .json::<Value>()
            .await
            .map_err(|e| AppError::Custom(format!("external answer response parse failed: {e}")))?;
        let detail = map_external_answer_result(question, &user_answer, &value);
        if detail.score < detail.max_score {
            push_unique(&mut weak_points, question.knowledge_point.clone());
        }
        for keyword in &detail.missing_keywords {
            push_unique(&mut missing_keywords, keyword.clone());
        }
        total_score += detail.score;
        max_score += detail.max_score;
        detail_results.push(detail);
    }

    let percent = score_percentage(total_score, max_score);
    let (level, feedback) = score_band(percent);
    let suggestions = build_suggestions(percent, &weak_points);

    Ok(LearningQuizScoreResult {
        total_score,
        max_score,
        level,
        weak_points,
        missing_keywords,
        feedback,
        suggestions,
        can_go_next: percent >= 60,
        detail_results,
    })
}

fn required_external_student_id(input: &LearningQuizScoreInput) -> Result<&str, AppError> {
    input
        .student_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::InvalidInput(
                "外部题库评分需要当前学习项目 ID，避免不同项目共用答题记录".to_string(),
            )
        })
}

fn query_sqlite_questions(
    db_url: &str,
    input: &LearningQuizGetQuestionsInput,
) -> Result<Vec<LearningQuizQuestion>, AppError> {
    let db_path = db_url
        .strip_prefix("sqlite://")
        .or_else(|| db_url.strip_prefix("sqlite:"))
        .unwrap_or(db_url);
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT
            question_id,
            course,
            knowledge_point,
            type,
            question,
            options,
            standard_answer,
            keywords,
            score,
            difficulty,
            explanation
         FROM {QUESTION_TABLE}
         WHERE course = ?1
         LIMIT 100"
    ))?;

    let rows = stmt.query_map([input.course.as_str()], |row| {
        let options_text: Option<String> = row.get(5)?;
        let keywords_text: Option<String> = row.get(7)?;
        Ok(LearningQuizQuestion {
            question_id: row.get(0)?,
            course: row.get(1)?,
            knowledge_point: row.get(2)?,
            question_type: row.get(3)?,
            question: row.get(4)?,
            options: options_text
                .as_deref()
                .map(parse_string_list)
                .unwrap_or_else(Vec::new),
            standard_answer: row.get(6)?,
            keywords: keywords_text
                .as_deref()
                .map(parse_string_list)
                .unwrap_or_else(Vec::new),
            score: row.get::<_, i64>(8).unwrap_or(0).max(0) as u32,
            difficulty: row.get(9)?,
            explanation: row.get(10)?,
            question_image: None,
            answer_image: None,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn query_formal_sqlite_questions(
    db_path: &Path,
    input: &LearningQuizGetQuestionsInput,
) -> Result<Vec<LearningQuizQuestion>, AppError> {
    if !is_mechanical_manufacturing_course(&input.course) {
        return Ok(Vec::new());
    }

    let conn = Connection::open(db_path)?;
    ensure_formal_question_bank_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT
            q.question_id,
            q.course_chapter,
            q.source_node_id,
            q.question_type,
            q.stem,
            q.options_json,
            q.bloom_level,
            q.subjective_difficulty,
            q.total_score,
            q.source,
            q.review_status,
            GROUP_CONCAT(k.knowledge_title, '；') AS knowledge_titles,
            q.image_path
         FROM questions q
         LEFT JOIN question_knowledge_map m ON m.question_id = q.question_id
         LEFT JOIN knowledge_points k ON k.knowledge_id = m.knowledge_id
         WHERE q.review_status = '已通过'
           AND COALESCE(q.usage_scope, '学生练习') = '学生练习'
           AND q.answer IS NOT NULL
           AND trim(q.answer) <> ''
           AND (q.no_answer_reason IS NULL OR trim(q.no_answer_reason) = '')
           AND q.question_type <> '多选'
         GROUP BY q.question_id",
    )?;

    let rows = stmt.query_map([], |row| {
        let raw_type: String = row.get(3)?;
        let options_json: Option<String> = row.get(5)?;
        let bloom_level: Option<String> = row.get(6)?;
        let subjective_difficulty: Option<String> = row.get(7)?;
        let total_score: Option<i64> = row.get(8)?;
        let course_chapter: Option<String> = row.get(1)?;
        let source_node_id: Option<String> = row.get(2)?;
        let source: Option<String> = row.get(9)?;
        let review_status: Option<String> = row.get(10)?;
        let knowledge_titles: Option<String> = row.get(11)?;
        let image_path: Option<String> = row.get(12)?;

        let question_type = map_formal_question_type(&raw_type);
        let options = parse_formal_options(options_json.as_deref(), &question_type);
        let knowledge_point = build_formal_knowledge_point(
            course_chapter.as_deref(),
            source_node_id.as_deref(),
            knowledge_titles.as_deref(),
        );
        let difficulty = map_formal_difficulty(
            subjective_difficulty.as_deref(),
            bloom_level.as_deref(),
            input,
        );

        Ok((
            LearningQuizQuestion {
                question_id: row.get(0)?,
                course: input.course.clone(),
                knowledge_point,
                question_type: question_type.clone(),
                question: row.get(4)?,
                options,
                // Approved answers and scoring keywords stay in Rust until submission.
                standard_answer: String::new(),
                keywords: Vec::new(),
                score: total_score
                    .unwrap_or_else(|| default_formal_score(&question_type) as i64)
                    .max(1) as u32,
                difficulty,
                explanation: build_formal_explanation(
                    course_chapter.as_deref(),
                    None,
                    source.as_deref(),
                    review_status.as_deref(),
                ),
                question_image: None,
                answer_image: None,
            },
            image_path,
        ))
    })?;

    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(mut question, image_path)| {
            question.question_image = image_path
                .as_deref()
                .map(|path| read_formal_asset_data_url(db_path, path))
                .transpose()?;
            Ok(question)
        })
        .collect()
}

fn formal_question_bank_path_for_scoring() -> Result<Option<PathBuf>, AppError> {
    if let Some(path) = question_bank_db_path_from_env()? {
        return Ok(Some(path));
    }
    bundled_formal_question_bank_path()
}

fn hydrate_formal_answers_for_scoring(
    db_path: &Path,
    input: &mut LearningQuizScoreInput,
) -> Result<(), AppError> {
    let conn = Connection::open(db_path)?;
    ensure_formal_question_bank_schema(&conn)?;
    let mut stmt = conn.prepare(
        "SELECT
            q.answer,
            q.rubric_json,
            q.total_score,
            q.question_type,
            q.answer_image_path,
            GROUP_CONCAT(k.key_concepts, '；') AS key_concepts,
            GROUP_CONCAT(k.tags, '；') AS tags,
            GROUP_CONCAT(k.knowledge_title, '；') AS knowledge_titles
         FROM questions q
         LEFT JOIN question_knowledge_map m ON m.question_id = q.question_id
         LEFT JOIN knowledge_points k ON k.knowledge_id = m.knowledge_id
         WHERE q.question_id = ?1
           AND q.review_status = '已通过'
           AND COALESCE(q.usage_scope, '学生练习') = '学生练习'
           AND q.answer IS NOT NULL
           AND trim(q.answer) <> ''
           AND (q.no_answer_reason IS NULL OR trim(q.no_answer_reason) = '')
           AND q.question_type <> '多选'
         GROUP BY q.question_id",
    )?;

    for question in &mut input.questions {
        let row = stmt
            .query_row([question.question_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })
            .optional()?;

        let Some((
            answer,
            rubric_json,
            total_score,
            raw_type,
            answer_image_path,
            key_concepts,
            tags,
            knowledge_titles,
        )) = row
        else {
            continue;
        };

        let question_type = map_formal_question_type(&raw_type);
        question.standard_answer = answer.clone();
        question.keywords = extract_formal_keywords(
            rubric_json.as_deref(),
            key_concepts.as_deref(),
            tags.as_deref(),
            knowledge_titles.as_deref(),
            &answer,
            &question_type,
        );
        question.score = total_score
            .unwrap_or_else(|| default_formal_score(&question_type) as i64)
            .max(1) as u32;
        question.answer_image = answer_image_path
            .as_deref()
            .map(|path| read_formal_asset_data_url(db_path, path))
            .transpose()?;
    }
    Ok(())
}

fn read_formal_asset_data_url(db_path: &Path, relative_path: &str) -> Result<String, AppError> {
    let relative = Path::new(relative_path.trim());
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(AppError::InvalidInput("题库图片路径不合法".to_string()));
    }

    let root = db_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| AppError::Custom("无法定位题库资源根目录".to_string()))?
        .canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(AppError::InvalidInput(
            "题库图片不在受控资源目录内".to_string(),
        ));
    }

    let metadata = fs::metadata(&path)?;
    if metadata.len() > MAX_FORMAL_ASSET_BYTES {
        return Err(AppError::InvalidInput(
            "题库图片超过 5 MiB 限制".to_string(),
        ));
    }
    let mime = match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => {
            return Err(AppError::InvalidInput(
                "题库图片格式仅支持 PNG/JPEG".to_string(),
            ))
        }
    };
    let bytes = fs::read(path)?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn ensure_formal_question_bank_schema(conn: &Connection) -> Result<(), AppError> {
    let has_questions: i64 = conn.query_row(
        "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'questions'",
        [],
        |row| row.get(0),
    )?;
    if has_questions == 0 {
        return Err(AppError::Custom(
            "正式题库 SQLite 中缺少 questions 表".to_string(),
        ));
    }

    let mut stmt = conn.prepare("PRAGMA table_info(questions)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    for required in ["question_id", "question_type", "stem", "answer"] {
        if !columns.iter().any(|column| column == required) {
            return Err(AppError::Custom(format!(
                "正式题库 questions 表缺少必要字段 {required}"
            )));
        }
    }
    Ok(())
}

fn question_bank_db_path_from_env() -> Result<Option<PathBuf>, AppError> {
    let Some(raw_path) = env_trim("QUESTION_BANK_DB_PATH") else {
        return Ok(None);
    };
    let path = PathBuf::from(raw_path);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    if path.is_file() {
        Ok(Some(path))
    } else {
        Err(AppError::Custom(
            "QUESTION_BANK_DB_PATH 指定的正式题库文件不存在".to_string(),
        ))
    }
}

fn bundled_formal_question_bank_path() -> Result<Option<PathBuf>, AppError> {
    let mut roots = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        push_path_ancestors(&mut roots, &current_dir);
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            push_path_ancestors(&mut roots, exe_dir);
        }
    }

    for root in roots {
        let candidate = root.join(FORMAL_QUESTION_BANK_RELATIVE_PATH);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn push_path_ancestors(roots: &mut Vec<PathBuf>, path: &Path) {
    for ancestor in path.ancestors() {
        let ancestor = ancestor.to_path_buf();
        if !roots.iter().any(|root| root == &ancestor) {
            roots.push(ancestor);
        }
    }
}

fn is_mechanical_manufacturing_course(course: &str) -> bool {
    let course = course.trim();
    course.is_empty()
        || course.contains("机械制造工艺学")
        || course.contains("机械制造")
        || course.to_lowercase().contains("manufacturing")
}

fn map_formal_question_type(raw_type: &str) -> String {
    if raw_type.contains("单选") || raw_type.contains("多选") {
        "choice".to_string()
    } else if raw_type.contains("判断") {
        "judgment".to_string()
    } else {
        "short_answer".to_string()
    }
}

fn parse_formal_options(options_json: Option<&str>, question_type: &str) -> Vec<String> {
    if question_type != "choice" && question_type != "judgment" {
        return Vec::new();
    }

    let Some(text) = options_json.map(str::trim).filter(|text| !text.is_empty()) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return parse_string_list(text);
    };

    match value {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text),
                Value::Object(map) => map
                    .get("text")
                    .or_else(|| map.get("label"))
                    .or_else(|| map.get("content"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                _ => None,
            })
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn build_formal_knowledge_point(
    course_chapter: Option<&str>,
    source_node_id: Option<&str>,
    knowledge_titles: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    for value in [course_chapter, knowledge_titles, source_node_id]
        .into_iter()
        .flatten()
    {
        for item in split_text_terms(value) {
            push_unique(&mut parts, item);
        }
    }
    if parts.is_empty() {
        "机械制造工艺学".to_string()
    } else {
        parts.join("；")
    }
}

fn extract_formal_keywords(
    rubric_json: Option<&str>,
    key_concepts: Option<&str>,
    tags: Option<&str>,
    knowledge_titles: Option<&str>,
    answer: &str,
    question_type: &str,
) -> Vec<String> {
    if question_type != "short_answer" {
        return Vec::new();
    }

    let mut keywords = Vec::new();
    if let Some(text) = rubric_json.map(str::trim).filter(|text| !text.is_empty()) {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(text) {
            for item in items {
                match item {
                    Value::String(text) => push_unique(&mut keywords, text.trim().to_string()),
                    Value::Object(map) => {
                        if let Some(point) = map.get("point").and_then(Value::as_str) {
                            push_unique(&mut keywords, point.trim().to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    for value in [key_concepts, tags, knowledge_titles].into_iter().flatten() {
        for item in split_text_terms(value) {
            push_unique(&mut keywords, item);
        }
    }

    if keywords.is_empty() {
        for item in split_text_terms(answer).into_iter().take(6) {
            push_unique(&mut keywords, item);
        }
    }

    keywords.into_iter().take(10).collect()
}

fn split_text_terms(text: &str) -> Vec<String> {
    text.split(|c: char| {
        matches!(
            c,
            ',' | ';' | '，' | '；' | '、' | '\n' | '\r' | '\t' | '|' | '/'
        )
    })
    .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_string())
    .filter(|item| !item.is_empty() && item.len() <= 80)
    .collect()
}

fn map_formal_difficulty(
    subjective_difficulty: Option<&str>,
    bloom_level: Option<&str>,
    input: &LearningQuizGetQuestionsInput,
) -> String {
    let raw = subjective_difficulty
        .or(bloom_level)
        .unwrap_or_else(|| preferred_difficulty(&input.level));
    if raw.contains("基础") || raw.contains("记忆") || raw.contains("easy") {
        "easy".to_string()
    } else if raw.contains("进阶")
        || raw.contains("困难")
        || raw.contains("应用")
        || raw.contains("分析")
        || raw.contains("hard")
    {
        "hard".to_string()
    } else {
        "medium".to_string()
    }
}

fn default_formal_score(question_type: &str) -> u32 {
    match question_type {
        "choice" | "judgment" => 10,
        _ => 10,
    }
}

fn build_formal_explanation(
    chapter: Option<&str>,
    explanation: Option<&str>,
    source: Option<&str>,
    review_status: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(chapter) = chapter.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("章节：{chapter}"));
    }
    if let Some(explanation) = explanation.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("解析：{explanation}"));
    }
    if let Some(source) = source.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("来源：{source}"));
    }
    if let Some(status) = review_status
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("审核状态：{status}"));
    }
    parts.join("；")
}

fn external_items_from_value(value: &Value) -> Result<Vec<Value>, AppError> {
    if let Some(items) = value.as_array() {
        return Ok(items.clone());
    }
    for key in ["questions", "items", "data", "results"] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            return Ok(items.clone());
        }
    }
    Err(AppError::Custom(
        "external question response is not an array".to_string(),
    ))
}

fn map_external_question(
    item: Value,
    input: &LearningQuizGetQuestionsInput,
) -> Option<LearningQuizQuestion> {
    let id = value_string(&item, &["id", "question_id", "questionId"])?;
    let external_type = value_string(&item, &["type", "question_type", "questionType"])
        .unwrap_or_else(|| "short_answer".to_string());
    let question_type = map_external_question_type(&external_type);
    let options = item
        .get("options")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|value| value.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let score = value_f64(&item, &["total_score", "totalScore", "score"])
        .unwrap_or_else(|| default_external_score(&question_type))
        .round()
        .max(1.0) as u32;
    let chapter = value_string(&item, &["chapter"]).unwrap_or_default();
    let node = value_string(&item, &["node", "knowledge_id", "knowledgeId"])
        .or_else(|| value_string(&item, &["knowledge_title", "knowledgeTitle"]))
        .unwrap_or_else(|| {
            if chapter.is_empty() {
                input.stage_name.clone()
            } else {
                chapter.clone()
            }
        });
    let stem = value_string(&item, &["stem", "question", "title"])?;
    let bloom = value_string(&item, &["bloom", "bloom_level", "bloomLevel"]).unwrap_or_default();
    let src = value_string(&item, &["src", "source"]).unwrap_or_default();
    let scan = item.get("scan").and_then(Value::as_bool).unwrap_or(false);
    let image = value_string(&item, &["image", "image_url", "imageUrl"]).unwrap_or_default();
    let explanation = build_external_question_note(&chapter, &bloom, &src, scan, &image);

    Some(LearningQuizQuestion {
        question_id: id,
        course: input.course.clone(),
        knowledge_point: node,
        question_type,
        question: stem,
        options,
        standard_answer: String::new(),
        keywords: Vec::new(),
        score,
        difficulty: map_external_difficulty(&bloom, input),
        explanation,
        question_image: None,
        answer_image: None,
    })
}

fn map_external_answer_result(
    question: &LearningQuizQuestion,
    user_answer: &str,
    value: &Value,
) -> LearningQuizDetailResult {
    let max_score = value_f64(value, &["total_score", "totalScore"])
        .unwrap_or(question.score as f64)
        .round()
        .max(1.0) as u32;
    let score = value_f64(value, &["score"])
        .map(|score| score.round().clamp(0.0, max_score as f64) as u32)
        .or_else(|| {
            value
                .get("is_correct")
                .or_else(|| value.get("isCorrect"))
                .and_then(Value::as_bool)
                .map(|correct| if correct { max_score } else { 0 })
        })
        .unwrap_or(0);
    let correct = value
        .get("is_correct")
        .or_else(|| value.get("isCorrect"))
        .and_then(Value::as_bool)
        .unwrap_or(score >= max_score);
    let standard_answer = value_string(
        value,
        &[
            "correct_answer",
            "correctAnswer",
            "reference_answer",
            "referenceAnswer",
            "standard_answer",
            "standardAnswer",
        ],
    )
    .or_else(|| value_string(value, &["answer_image", "answerImage"]))
    .unwrap_or_else(|| question.standard_answer.clone());
    let mut missing_keywords = missing_keywords(question, user_answer);
    if !correct && missing_keywords.is_empty() && !question.knowledge_point.is_empty() {
        missing_keywords.push(question.knowledge_point.clone());
    }
    let comment = build_external_answer_comment(value, correct);

    LearningQuizDetailResult {
        question_id: question.question_id.clone(),
        user_answer: user_answer.trim().to_string(),
        standard_answer,
        score,
        max_score,
        correct,
        missing_keywords,
        comment,
        answer_image: None,
    }
}

fn map_external_question_type(question_type: &str) -> String {
    let value = question_type.to_lowercase();
    if value.contains("multi") || question_type.contains("多选") {
        "short_answer".to_string()
    } else if value.contains("choice") || question_type.contains('选') {
        "choice".to_string()
    } else if value.contains("judgment") || question_type.contains('判') {
        "judgment".to_string()
    } else {
        "short_answer".to_string()
    }
}

fn map_external_difficulty(bloom: &str, input: &LearningQuizGetQuestionsInput) -> String {
    if let Some(difficulty) = &input.difficulty {
        return difficulty.clone();
    }
    if ["应用", "分析", "hard", "advanced"]
        .iter()
        .any(|keyword| bloom.contains(keyword))
    {
        "hard".to_string()
    } else if ["记忆", "easy", "基础"]
        .iter()
        .any(|keyword| bloom.contains(keyword))
    {
        "easy".to_string()
    } else {
        preferred_difficulty(&input.level).to_string()
    }
}

fn default_external_score(question_type: &str) -> f64 {
    match question_type {
        "choice" | "judgment" => 2.0,
        _ => 10.0,
    }
}

fn build_external_question_note(
    chapter: &str,
    bloom: &str,
    src: &str,
    scan: bool,
    image: &str,
) -> String {
    let mut parts = Vec::new();
    if !chapter.is_empty() {
        parts.push(format!("章节：{chapter}"));
    }
    if !bloom.is_empty() {
        parts.push(format!("Bloom：{bloom}"));
    }
    if !src.is_empty() {
        parts.push(format!("来源：{src}"));
    }
    if scan {
        parts.push("原卷截图题，请以题图为准".to_string());
    }
    if !image.is_empty() {
        parts.push(format!("题图：{image}"));
    }
    parts.join("；")
}

fn build_external_answer_comment(value: &Value, correct: bool) -> String {
    let mut parts = Vec::new();
    for key in [
        "grade_note",
        "gradeNote",
        "process_feedback",
        "processFeedback",
        "explanation",
    ] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        if correct {
            "外部出题系统判定为正确".to_string()
        } else {
            "外部出题系统已返回反馈，请对照标准答案和解析复盘".to_string()
        }
    } else {
        parts.join("；")
    }
}

fn missing_keywords(question: &LearningQuizQuestion, user_answer: &str) -> Vec<String> {
    let answer = user_answer.to_lowercase();
    question
        .keywords
        .iter()
        .filter(|keyword| !answer.contains(&keyword.to_lowercase()))
        .cloned()
        .collect()
}

fn value_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|item| {
            item.as_str()
                .map(|text| text.trim().to_string())
                .or_else(|| item.as_i64().map(|number| number.to_string()))
                .or_else(|| item.as_u64().map(|number| number.to_string()))
        })
        .filter(|text| !text.is_empty())
}

fn value_f64(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|item| {
            item.as_f64()
                .or_else(|| item.as_i64().map(|number| number as f64))
                .or_else(|| item.as_u64().map(|number| number as f64))
                .or_else(|| {
                    item.as_str()
                        .and_then(|text| text.trim().parse::<f64>().ok())
                })
        })
}

async fn query_http_questions(
    config: &LearningQuizDbConfig,
    input: &LearningQuizGetQuestionsInput,
) -> Result<Vec<LearningQuizQuestion>, AppError> {
    let client = reqwest::Client::new();
    let mut request = client.post(&config.url).json(input);
    if let (Some(user), Some(password)) = (&config.user, &config.password) {
        request = request.basic_auth(user, Some(password));
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("学习题库接口请求失败: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Custom(format!(
            "学习题库接口返回非成功状态: {status}"
        )));
    }

    let result = response
        .json::<LearningQuizQuestionsResult>()
        .await
        .map_err(|e| AppError::Custom(format!("学习题库接口响应解析失败: {e}")))?;

    Ok(result.questions)
}

fn normalize_input(
    mut input: LearningQuizGetQuestionsInput,
) -> Result<LearningQuizGetQuestionsInput, AppError> {
    input.course = input.course.trim().to_string();
    input.stage_name = input.stage_name.trim().to_string();
    input.level = input.level.trim().to_string();
    input.limit = input.limit.clamp(1, 10);
    input.knowledge_points = input
        .knowledge_points
        .into_iter()
        .map(|point| point.trim().to_string())
        .filter(|point| !point.is_empty())
        .collect();
    input.difficulty = input
        .difficulty
        .map(|difficulty| difficulty.trim().to_lowercase())
        .filter(|difficulty| !difficulty.is_empty());

    if input.course.is_empty() {
        return Err(AppError::InvalidInput("course cannot be empty".to_string()));
    }
    if input.stage_name.is_empty() {
        input.stage_name = format!("阶段 {}", input.stage_index);
    }
    Ok(input)
}

fn rank_and_limit(
    questions: Vec<LearningQuizQuestion>,
    input: &LearningQuizGetQuestionsInput,
) -> Vec<LearningQuizQuestion> {
    let course = input.course.to_lowercase();
    let preferred_difficulty = input
        .difficulty
        .as_deref()
        .unwrap_or_else(|| preferred_difficulty(&input.level));
    let knowledge_points: Vec<String> = input
        .knowledge_points
        .iter()
        .map(|point| point.to_lowercase())
        .collect();
    let stage_name = input.stage_name.to_lowercase();

    let mut scored = questions
        .into_iter()
        .filter(|question| question.course.to_lowercase() == course)
        .map(|question| {
            let mut score = 0;
            let knowledge = question.knowledge_point.to_lowercase();
            let text = question.question.to_lowercase();
            if !stage_name.is_empty()
                && (knowledge.contains(&stage_name)
                    || text.contains(&stage_name)
                    || stage_name.contains(&knowledge))
            {
                score += 30;
            }
            if knowledge_points.iter().any(|point| {
                !point.is_empty()
                    && (knowledge.contains(point)
                        || point.contains(&knowledge)
                        || text.contains(point))
            }) {
                score += 40;
            }
            if normalize_difficulty(&question.difficulty) == preferred_difficulty {
                score += 20;
            }
            score += match question.question_type.as_str() {
                "choice" => 12,
                "judgment" => 10,
                "short_answer" => 8,
                _ => 4,
            };
            (score, question)
        })
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.question_id.cmp(&b.1.question_id))
    });
    scored
        .into_iter()
        .take(input.limit)
        .map(|(_, question)| question)
        .collect()
}

fn score_question(question: &LearningQuizQuestion, user_answer: &str) -> LearningQuizDetailResult {
    let normalized_user_answer = normalize_answer(user_answer);
    let normalized_standard_answer = normalize_answer(&question.standard_answer);
    let max_score = question.score;

    let (score, missing_keywords, correct, comment) = match question.question_type.as_str() {
        "choice" | "judgment" => {
            let correct = normalized_user_answer == normalized_standard_answer;
            (
                if correct { max_score } else { 0 },
                Vec::new(),
                correct,
                if correct {
                    "答案正确".to_string()
                } else {
                    "答案与标准答案不一致".to_string()
                },
            )
        }
        "short_answer" => {
            if question.keywords.is_empty() {
                let correct = normalized_user_answer == normalized_standard_answer;
                (
                    if correct { max_score } else { 0 },
                    Vec::new(),
                    correct,
                    "未配置关键词，按标准答案完全匹配评分".to_string(),
                )
            } else {
                let answer_lower = user_answer.to_lowercase();
                let matched = question
                    .keywords
                    .iter()
                    .filter(|keyword| answer_lower.contains(&keyword.to_lowercase()))
                    .count();
                let missing = question
                    .keywords
                    .iter()
                    .filter(|keyword| !answer_lower.contains(&keyword.to_lowercase()))
                    .cloned()
                    .collect::<Vec<_>>();
                let score = ((max_score as f64) * (matched as f64)
                    / (question.keywords.len() as f64))
                    .round() as u32;
                (
                    score,
                    missing,
                    score == max_score,
                    format!("匹配到 {matched}/{} 个关键词", question.keywords.len()),
                )
            }
        }
        _ => (0, Vec::new(), false, "暂不支持该题型评分".to_string()),
    };

    LearningQuizDetailResult {
        question_id: question.question_id.clone(),
        user_answer: user_answer.trim().to_string(),
        standard_answer: question.standard_answer.clone(),
        score,
        max_score,
        correct,
        missing_keywords,
        comment,
        answer_image: question.answer_image.clone(),
    }
}

fn load_demo_questions() -> Result<Vec<LearningQuizQuestion>, AppError> {
    if let Some(path) = demo_questions_path() {
        if let Ok(text) = fs::read_to_string(path) {
            return parse_question_list(&text);
        }
    }
    parse_question_list(DEMO_QUESTIONS_JSON)
}

fn demo_questions_path() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    [
        current.join("learning-assistant/templates/demo_questions.json"),
        current.join("../learning-assistant/templates/demo_questions.json"),
        current.join("../../learning-assistant/templates/demo_questions.json"),
        current.join("../../../learning-assistant/templates/demo_questions.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn parse_question_list(text: &str) -> Result<Vec<LearningQuizQuestion>, AppError> {
    #[derive(Deserialize)]
    struct QuestionList {
        questions: Vec<LearningQuizQuestion>,
    }

    if let Ok(list) = serde_json::from_str::<QuestionList>(text) {
        return Ok(list.questions);
    }
    serde_json::from_str::<Vec<LearningQuizQuestion>>(text).map_err(AppError::from)
}

fn parse_string_list(text: &str) -> Vec<String> {
    if let Ok(list) = serde_json::from_str::<Vec<String>>(text) {
        return list;
    }
    text.split([',', '，', ';', '；'])
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn preferred_difficulty(level: &str) -> &'static str {
    let level = level.to_lowercase();
    if ["零基础", "基础较差", "较差", "入门", "beginner", "weak"]
        .iter()
        .any(|keyword| level.contains(keyword))
    {
        "easy"
    } else if ["较好", "提高", "hard", "advanced"]
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

fn normalize_answer(answer: &str) -> String {
    answer
        .trim()
        .to_lowercase()
        .replace(char::is_whitespace, "")
}

fn score_percentage(total_score: u32, max_score: u32) -> u32 {
    if max_score == 0 {
        return 0;
    }
    (((total_score as f64 / max_score as f64) * 100.0).round() as u32).min(100)
}

fn score_band(percent: u32) -> (String, String) {
    if percent >= 80 {
        (
            "优秀".to_string(),
            "优秀，当前阶段掌握较好，可以正常进入后续阶段。".to_string(),
        )
    } else if percent >= 60 {
        (
            "基本掌握".to_string(),
            "基本掌握，可以继续后续学习，建议复习薄弱知识点。".to_string(),
        )
    } else {
        (
            "需要重学".to_string(),
            "本阶段测试结果低于60分，建议重新学习当前阶段薄弱知识点。".to_string(),
        )
    }
}

fn build_suggestions(percent: u32, weak_points: &[String]) -> Vec<String> {
    let weak_text = if weak_points.is_empty() {
        "本阶段核心知识点".to_string()
    } else {
        weak_points.join("、")
    };
    if percent >= 80 {
        vec!["保持当前节奏，可补充提高题或综合案例。".to_string()]
    } else if percent >= 60 {
        vec![format!("进入下一阶段前复习：{weak_text}。")]
    } else {
        vec![
            format!("重新学习：{weak_text}。"),
            "本阶段测试结果低于60分，建议先完成薄弱点补学后再重新测试。".to_string(),
        ]
    }
}

fn push_unique(items: &mut Vec<String>, value: String) {
    if !value.is_empty() && !items.iter().any(|item| item == &value) {
        items.push(value);
    }
}

fn env_trim(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn question_bank_api_url() -> Option<String> {
    env_trim("QUESTION_BANK_API_URL").map(|url| url.trim_end_matches('/').to_string())
}

fn allow_demo_quiz_fallback() -> bool {
    env_trim("LEARNING_QUIZ_ALLOW_DEMO")
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn default_limit() -> usize {
    5
}

#[cfg(test)]
mod bundled_data_tests {
    use super::*;

    fn bundled_path() -> PathBuf {
        bundled_formal_question_bank_path()
            .expect("question-bank path lookup should not fail")
            .expect("firstwork must include the formal question-bank database")
    }

    fn test_input() -> LearningQuizGetQuestionsInput {
        LearningQuizGetQuestionsInput {
            course: "机械制造工艺学".to_string(),
            stage_name: "机械加工精度".to_string(),
            stage_index: 3,
            knowledge_points: vec!["加工精度".to_string()],
            level: "基础一般".to_string(),
            difficulty: None,
            limit: 5,
        }
    }

    #[test]
    fn bundled_formal_question_bank_is_discoverable_and_readable() {
        let path = bundled_path();
        let questions = query_formal_sqlite_questions(&path, &test_input())
            .expect("bundled formal question bank should be readable");
        assert!(!questions.is_empty());
        assert!(questions
            .iter()
            .all(|question| question.standard_answer.is_empty()));
        assert!(questions
            .iter()
            .all(|question| question.keywords.is_empty()));
        assert!(questions
            .iter()
            .all(|question| question.answer_image.is_none()));
        assert!(questions.iter().any(|question| question
            .question_image
            .as_deref()
            .is_some_and(|image| image.starts_with("data:image/"))));
    }

    #[test]
    fn approved_answer_and_answer_image_are_loaded_only_for_scoring() {
        let path = bundled_path();
        let questions = query_formal_sqlite_questions(&path, &test_input())
            .expect("bundled formal question bank should be readable");
        let question = questions
            .into_iter()
            .find(|question| question.question_id.starts_with("P_"))
            .expect("the formal bank should contain an approved student PDF question");
        assert!(question.standard_answer.is_empty());
        assert!(question.answer_image.is_none());

        let mut score_input = LearningQuizScoreInput {
            stage_name: "机械加工精度".to_string(),
            stage_index: 3,
            student_id: Some("project-test-a".to_string()),
            questions: vec![question],
            answers: vec![LearningQuizAnswer {
                question_id: "P_24372c7bf1".to_string(),
                user_answer: "测试作答".to_string(),
            }],
        };
        // Use the selected ID rather than relying on a fixed record ordering.
        score_input.answers[0].question_id = score_input.questions[0].question_id.clone();
        hydrate_formal_answers_for_scoring(&path, &mut score_input)
            .expect("approved answers should be available to the Rust scorer");
        assert!(!score_input.questions[0].standard_answer.is_empty());
        assert!(score_input.questions[0]
            .answer_image
            .as_deref()
            .is_some_and(|image| image.starts_with("data:image/")));

        let result = score_locally(score_input).expect("local scoring should succeed");
        assert!(result.detail_results[0]
            .answer_image
            .as_deref()
            .is_some_and(|image| image.starts_with("data:image/")));
    }

    #[test]
    fn formal_asset_reader_rejects_path_traversal() {
        let error = read_formal_asset_data_url(&bundled_path(), "../db/question_bank.db")
            .expect_err("path traversal must be rejected");
        assert!(error.to_string().contains("路径不合法"));
    }

    #[test]
    fn external_scoring_requires_a_project_specific_student_id() {
        let missing = LearningQuizScoreInput {
            stage_name: "阶段".to_string(),
            stage_index: 1,
            student_id: None,
            questions: Vec::new(),
            answers: Vec::new(),
        };
        assert!(required_external_student_id(&missing).is_err());

        let present = LearningQuizScoreInput {
            student_id: Some("project-isolated-1".to_string()),
            ..missing
        };
        assert_eq!(
            required_external_student_id(&present).expect("project ID should be accepted"),
            "project-isolated-1"
        );
    }
}
