use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const DEMO_QUESTIONS_JSON: &str =
    include_str!("../../../../learning-assistant/templates/demo_questions.json");
const QUESTION_TABLE: &str = "learning_quiz_questions";

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
    pub question_id: String,
    pub course: String,
    pub knowledge_point: String,
    #[serde(rename = "type")]
    pub question_type: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
    pub standard_answer: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub score: u32,
    pub difficulty: String,
    pub explanation: String,
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
}

pub struct LearningQuizService;

impl LearningQuizService {
    pub async fn get_questions(
        input: LearningQuizGetQuestionsInput,
    ) -> Result<LearningQuizQuestionsResult, AppError> {
        let input = normalize_input(input)?;
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

    pub fn score(input: LearningQuizScoreInput) -> Result<LearningQuizScoreResult, AppError> {
        if input.questions.is_empty() {
            return Err(AppError::InvalidInput("questions cannot be empty".to_string()));
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

        let percent = if max_score == 0 {
            0
        } else {
            total_score * 100 / max_score
        };
        let (level, feedback) = score_band(percent);
        let can_go_next = percent >= 70;
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
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
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

    let mut scored = questions
        .into_iter()
        .filter(|question| question.course.to_lowercase() == course)
        .map(|question| {
            let mut score = 0;
            let knowledge = question.knowledge_point.to_lowercase();
            let text = question.question.to_lowercase();
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

fn score_question(
    question: &LearningQuizQuestion,
    user_answer: &str,
) -> LearningQuizDetailResult {
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
    answer.trim().to_lowercase().replace(char::is_whitespace, "")
}

fn score_band(percent: u32) -> (String, String) {
    if percent >= 85 {
        (
            "掌握良好".to_string(),
            "掌握良好，可以进入下一阶段。".to_string(),
        )
    } else if percent >= 70 {
        (
            "基本掌握".to_string(),
            "基本掌握，可以进入下一阶段，但需要补强薄弱点。".to_string(),
        )
    } else if percent >= 60 {
        (
            "掌握不稳".to_string(),
            "掌握不稳，建议减少新内容并补强本阶段知识点。".to_string(),
        )
    } else {
        (
            "建议重学".to_string(),
            "建议重学本阶段，降低难度并再次测试。".to_string(),
        )
    }
}

fn build_suggestions(percent: u32, weak_points: &[String]) -> Vec<String> {
    let weak_text = if weak_points.is_empty() {
        "本阶段核心知识点".to_string()
    } else {
        weak_points.join("、")
    };
    if percent >= 85 {
        vec!["保持当前节奏，可补充提高题或综合案例。".to_string()]
    } else if percent >= 70 {
        vec![format!("进入下一阶段前复习：{weak_text}。")]
    } else if percent >= 60 {
        vec![
            format!("先补强：{weak_text}。"),
            "减少新内容输入，增加基础练习和错题复盘。".to_string(),
        ]
    } else {
        vec![
            format!("重新学习：{weak_text}。"),
            "完成基础概念复述后再进行阶段测试。".to_string(),
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

fn default_limit() -> usize {
    5
}
