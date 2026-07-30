use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::database::Database;
use crate::error::AppError;
use crate::services::credentials::CredentialService;
use crate::services::http_client;
use crate::services::learning_kb::{
    LearningKbResultItem, LearningKbSearchInput, LearningKbService,
};
use crate::services::local_learning_plan::{
    learning_goal_config, LearningGoal, LocalLearningPlanAllocation, LocalLearningPlanInput,
    LocalLearningPlanService, SelectedLearningSource, SourceImportanceLevel,
};

const LEARNING_SKILL_MD: &str = "skills/learning-assistant/SKILL.md";
const GENERATE_PLAN_WORKFLOW: &str =
    "skills/learning-assistant/workflows/generate-learning-plan.md";
const PLANNING_RULES: &str = "skills/learning-assistant/references/planning-rules.md";
const SCORING_RULES: &str = "skills/learning-assistant/references/scoring-rules.md";
const PLAN_TEMPLATE_JSON: &str = "templates/plan_template.json";
const DEFAULT_SPARK_API_BASE: &str = "https://spark-api-open.xf-yun.com/v1";
const DEFAULT_SPARK_MODEL: &str = "4.0Ultra";
const LEARNING_AI_CONFIG_KEY: &str = "learning_assistant.ai_config";
const LEARNING_AI_CREDENTIAL_ID: &str = "learning-assistant-ai-api-key";
const LEARNING_AI_CREDENTIAL_PROVIDER: &str = "learning-assistant";
const LEARNING_AI_CREDENTIAL_LABEL: &str = "AI 助学模型 API Key";
const CLOSED_LOOP_TEXT: &str =
    "目标解析 -> 计划生成 -> 阶段任务 -> 资源推荐 -> 成果检查 -> 进度记录 -> 计划调整";

static RUNTIME_LEARNING_AI_CONFIG: OnceLock<RwLock<Option<LearningAiConfig>>> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantCheckInput {
    pub learning_assistant_root: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantCheckResult {
    pub ok: bool,
    pub skill_path: String,
    pub template_path: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantPlanInput {
    pub learning_assistant_root: String,
    pub learning_goal: String,
    pub course_name: String,
    pub learning_cycle: String,
    pub daily_time: String,
    #[serde(default)]
    pub daily_study_hours: f64,
    pub current_level: String,
    pub final_goal: String,
    #[serde(default)]
    pub selected_document_source_ids: Vec<i64>,
    #[serde(default)]
    pub selected_learning_sources: Vec<SelectedLearningSource>,
    #[serde(default)]
    pub plugin_prompt_context: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantUnderstanding {
    pub summary: String,
    pub current_gap: String,
    pub strategy: String,
    pub closed_loop: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_analysis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_points: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_points: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantStage {
    pub name: String,
    pub time_range: String,
    pub goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge_points: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learning_entries: Option<Vec<LearningPlanEntry>>,
    pub learning_tasks: Vec<String>,
    pub resource_tasks: Vec<String>,
    pub practice_tasks: Vec<String>,
    pub check_tasks: Vec<String>,
    pub completion_criteria: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningPlanEntry {
    pub entry_id: String,
    pub title: String,
    pub section: String,
    pub entry_type: String,
    pub mastery_level: String,
    pub study_action: String,
    pub practice_action: String,
    pub check_method: String,
    pub expected_output: String,
    pub estimated_minutes: u32,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    pub source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prerequisite: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weak_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_task: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantPlanResult {
    pub success: bool,
    pub engine_root: String,
    pub skill_path: String,
    pub template_path: String,
    pub understanding: LearningAssistantUnderstanding,
    pub stages: Vec<LearningAssistantStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_profile_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_allocation: Option<LocalLearningPlanAllocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct LearningAiConfig {
    api_base: String,
    api_key: String,
    model: String,
    source: String,
    credential_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PersistedLearningAiConfig {
    api_base: String,
    #[serde(default)]
    credential_id: Option<String>,
    // Legacy field from early builds. If present, it is migrated into
    // secure-credentials before being removed from app_config.
    #[serde(default)]
    api_key: Option<String>,
    model: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantAiConfigInput {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantAiConfigStatus {
    pub api_base: String,
    pub model: String,
    pub has_api_key: bool,
    pub source: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningPlanAdjustInput {
    pub course_name: String,
    pub current_level: String,
    pub final_goal: String,
    pub daily_time: String,
    pub learning_cycle: String,
    pub stage_index: usize,
    pub stages: Vec<LearningAssistantStage>,
    pub score: u32,
    pub max_score: u32,
    pub mastery_level: String,
    #[serde(default)]
    pub weak_points: Vec<String>,
    #[serde(default)]
    pub missing_keywords: Vec<String>,
    #[serde(default)]
    pub wrong_knowledge_points: Vec<String>,
    #[serde(default)]
    pub feedback: String,
    #[serde(default)]
    pub review_suggestions: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningPlanAdjustResult {
    pub stages: Vec<LearningAssistantStage>,
    pub conclusion: String,
    pub reason: String,
    pub source: String,
    pub rule_band: String,
    pub current_stage_status: String,
    pub can_advance: bool,
    pub need_retest: bool,
    pub weak_points: Vec<String>,
    pub added_tasks: Vec<String>,
    pub delayed_tasks: Vec<String>,
    pub locked_stage_indexes: Vec<usize>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AiPlanAdjustment {
    reason: Option<String>,
    added_tasks: Option<Vec<String>>,
    review_tasks: Option<Vec<String>>,
    practice_tasks: Option<Vec<String>>,
    check_tasks: Option<Vec<String>>,
    improvement_tasks: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AiGoalUnderstanding {
    #[serde(alias = "courseName")]
    course: Option<String>,
    #[serde(alias = "goal_type")]
    goal_type: Option<String>,
    days: Option<Value>,
    cycle: Option<Value>,
    #[serde(alias = "dailyTime")]
    daily_time: Option<String>,
    #[serde(alias = "levelAnalysis")]
    level: Option<String>,
    #[serde(alias = "finalGoal")]
    final_goal: Option<String>,
    expected_outputs: Option<Value>,
    #[serde(alias = "focusPoints")]
    knowledge_scope: Option<Value>,
    #[serde(alias = "riskPoints")]
    risk_points: Option<Value>,
    #[serde(alias = "suggestions")]
    suggestions: Option<Value>,
    #[serde(alias = "learningStrategy")]
    learning_strategy: Option<String>,
    stage_count_suggestion: Option<Value>,
    summary: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AiPlanResponse {
    understanding: Option<AiGoalUnderstanding>,
    stages: Vec<AiPlanStage>,
    source: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AiPlanStage {
    #[serde(alias = "stageName", alias = "name")]
    stage_name: Option<String>,
    #[serde(alias = "duration", alias = "timeRange")]
    duration: Option<String>,
    #[serde(alias = "stageGoal", alias = "goal")]
    stage_goal: Option<String>,
    knowledge_points: Option<Value>,
    learning_tasks: Option<Value>,
    resource_tasks: Option<Value>,
    practice_tasks: Option<Value>,
    check_tasks: Option<Value>,
    completion_criteria: Option<Value>,
    learning_entries: Option<Vec<LearningPlanEntry>>,
}

#[derive(Debug, Clone)]
struct GoalPlanProfile {
    goal_type: LearningGoal,
    cycle: &'static str,
    recommended_stage_count: usize,
    coverage_mode: &'static str,
    depth_mode: &'static str,
    preferred_mastery_levels: Vec<&'static str>,
    task_focus: Vec<&'static str>,
    entry_count_range: (usize, usize),
    assessment_mode: &'static str,
    prompt_instruction: &'static str,
    plan_strategy: &'static str,
    goal_profile_summary: String,
    stage_templates: Vec<&'static str>,
}

#[derive(Debug, Clone)]
struct LearningEntryCandidate {
    title: String,
    section: String,
    content: String,
    source_file: String,
    score: f64,
}

#[derive(Debug, Clone, Default)]
struct PlanKnowledgeContext {
    text: Option<String>,
    candidates: Vec<LearningEntryCandidate>,
}

#[derive(Debug, Clone)]
enum LearningAiFailure {
    NotConfigured(String),
    Http { status: u16, message: String },
    InvalidModel(String),
    Network(String),
    EmptyContent,
    InvalidUnderstandingJson(String),
    InvalidPlanJson(String),
    Other(String),
}

impl LearningAiFailure {
    fn fallback_reason(&self) -> String {
        match self {
            Self::NotConfigured(message) => format!("未配置 API：{message}"),
            Self::Http {
                status: 401,
                message,
            } => append_http_detail("HTTP 401：API Key 或 APIPassword 无效", message),
            Self::Http {
                status: 403,
                message,
            } => append_http_detail("HTTP 403：当前账号没有该模型权限", message),
            Self::Http {
                status: 404,
                message,
            } => append_http_detail("HTTP 404：API Base 或请求路径错误", message),
            Self::InvalidModel(message) => format!("模型名称无效：{message}"),
            Self::Network(message) => format!("网络请求失败：{message}"),
            Self::EmptyContent => "接口返回空内容".to_string(),
            Self::InvalidUnderstandingJson(message) => {
                format!("模型返回内容无法解析为目标理解 JSON：{message}")
            }
            Self::InvalidPlanJson(message) => {
                format!("模型返回内容无法解析为计划 JSON：{message}")
            }
            Self::Http { status, message } => {
                format!("其他 HTTP 错误（状态码 {status}）：{message}")
            }
            Self::Other(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for LearningAiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.fallback_reason())
    }
}

fn append_http_detail(base: &str, message: &str) -> String {
    let detail = message.trim();
    if detail.is_empty() || detail == "接口未返回错误详情" {
        base.to_string()
    } else {
        format!("{base}；接口消息：{detail}")
    }
}

pub struct LearningAssistantService;

impl LearningAssistantService {
    pub fn get_ai_config(db: &Database) -> Result<LearningAssistantAiConfigStatus, AppError> {
        Ok(resolve_learning_ai_config(db)
            .map(config_status_from_resolved)
            .unwrap_or_else(|_| fallback_config_status()))
    }

    pub fn save_ai_config(
        db: &Database,
        input: LearningAssistantAiConfigInput,
    ) -> Result<LearningAssistantAiConfigStatus, AppError> {
        let api_base = clean_or(&input.api_base, DEFAULT_SPARK_API_BASE);
        let model = clean_or(&input.model, DEFAULT_SPARK_MODEL);
        let new_api_key = input.api_key.trim().to_string();
        let existing = read_persisted_learning_ai_config(db)?;
        let credential_id = existing
            .as_ref()
            .and_then(|config| config.credential_id.clone())
            .unwrap_or_else(|| LEARNING_AI_CREDENTIAL_ID.to_string());

        let legacy_api_key = existing
            .as_ref()
            .and_then(|config| config.api_key.clone())
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty());

        if !new_api_key.is_empty() {
            CredentialService::upsert_api_key(
                db,
                db.data_dir(),
                &credential_id,
                LEARNING_AI_CREDENTIAL_PROVIDER,
                LEARNING_AI_CREDENTIAL_LABEL,
                &new_api_key,
            )?;
        } else if let Some(legacy_key) = legacy_api_key {
            CredentialService::upsert_api_key(
                db,
                db.data_dir(),
                &credential_id,
                LEARNING_AI_CREDENTIAL_PROVIDER,
                LEARNING_AI_CREDENTIAL_LABEL,
                &legacy_key,
            )?;
        } else if CredentialService::load_api_key(db, db.data_dir(), &credential_id)
            .ok()
            .flatten()
            .is_none()
            && env_trim("SPARK_API_PASSWORD").is_none()
        {
            return Err(AppError::InvalidInput(
                "API Key cannot be empty".to_string(),
            ));
        }

        let persisted = PersistedLearningAiConfig {
            api_base: api_base.clone(),
            credential_id: Some(credential_id.clone()),
            api_key: None,
            model: model.clone(),
        };
        let value = serde_json::to_string(&persisted)
            .map_err(|e| AppError::Custom(format!("serialize learning ai config failed: {e}")))?;
        db.set_config(LEARNING_AI_CONFIG_KEY, &value)?;

        let status = LearningAssistantAiConfigStatus {
            api_base: api_base.clone(),
            model: model.clone(),
            has_api_key: true,
            source: "user".to_string(),
        };

        {
            let mut guard = runtime_learning_ai_config()
                .write()
                .map_err(|_| AppError::Custom("learning ai config lock poisoned".to_string()))?;
            *guard = Some(LearningAiConfig {
                api_base,
                api_key: CredentialService::load_api_key(db, db.data_dir(), &credential_id)
                    .ok()
                    .flatten()
                    .or_else(|| env_trim("SPARK_API_PASSWORD"))
                    .unwrap_or_default(),
                model,
                source: "user".to_string(),
                credential_id: Some(credential_id),
            });
        }

        Ok(status)
    }

    pub fn clear_ai_config(db: &Database) -> Result<LearningAssistantAiConfigStatus, AppError> {
        if let Some(existing) = read_persisted_learning_ai_config(db)? {
            if let Some(credential_id) = existing.credential_id {
                let _ = CredentialService::delete(db, db.data_dir(), &credential_id, true);
            }
        }
        let _ = db.delete_config(LEARNING_AI_CONFIG_KEY)?;
        {
            let mut guard = runtime_learning_ai_config()
                .write()
                .map_err(|_| AppError::Custom("learning ai config lock poisoned".to_string()))?;
            *guard = None;
        }
        Self::get_ai_config(db)
    }

    pub fn check(
        input: LearningAssistantCheckInput,
    ) -> Result<LearningAssistantCheckResult, AppError> {
        let root = parse_dir("learning-assistant root", &input.learning_assistant_root)?;
        let skill_path = root.join(LEARNING_SKILL_MD);
        let template_path = root.join(PLAN_TEMPLATE_JSON);
        let mut errors = Vec::new();

        if !skill_path.is_file() {
            errors.push(format!("Missing skill file: {}", skill_path.display()));
        }
        if !template_path.is_file() {
            errors.push(format!(
                "Missing plan template: {}",
                template_path.display()
            ));
        }
        for required in [GENERATE_PLAN_WORKFLOW, PLANNING_RULES, SCORING_RULES] {
            let required_path = root.join(required);
            if !required_path.is_file() {
                errors.push(format!(
                    "Missing skill reference: {}",
                    required_path.display()
                ));
            }
        }

        Ok(LearningAssistantCheckResult {
            ok: errors.is_empty(),
            skill_path: skill_path.to_string_lossy().to_string(),
            template_path: template_path.to_string_lossy().to_string(),
            errors,
        })
    }

    pub async fn understand(
        db: &Database,
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, AppError> {
        let root = validate_engine(&input.learning_assistant_root)?;
        let (understanding, fallback_reason, message) =
            match build_ai_understanding(db, &input).await {
                Ok(understanding) => (
                    understanding,
                    None,
                    Some("已使用讯飞星火完成学习目标解析".to_string()),
                ),
                Err(error) => {
                    let reason = error.fallback_reason();
                    let fallback_message = format!("讯飞星火目标解析使用模板 fallback：{error}");
                    log::warn!("[learning_assistant] {fallback_message}");
                    (
                        build_understanding(&input),
                        Some(reason),
                        Some(fallback_message),
                    )
                }
            };

        Ok(LearningAssistantPlanResult {
            success: true,
            engine_root: root.to_string_lossy().to_string(),
            skill_path: root.join(LEARNING_SKILL_MD).to_string_lossy().to_string(),
            template_path: root.join(PLAN_TEMPLATE_JSON).to_string_lossy().to_string(),
            understanding,
            stages: Vec::new(),
            plan_strategy: None,
            goal_profile_summary: None,
            local_allocation: None,
            message,
            fallback_reason,
            error: None,
        })
    }

    pub async fn generate_plan(
        db: &Database,
        data_dir: &std::path::Path,
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, AppError> {
        let root = validate_engine(&input.learning_assistant_root)?;
        let local_allocation = calculate_local_allocation(db, data_dir, &input)?;
        let profile = build_goal_plan_profile(&input);
        let kb_context = build_plan_knowledge_context(db, data_dir, &input, &profile);
        let (understanding, stages, fallback_reason, message) =
            match build_ai_plan(db, &input, &profile, &kb_context).await {
                Ok((understanding, stages)) => (
                    understanding,
                    normalize_goal_specific_plan(stages, &input, &profile, &kb_context.candidates),
                    None,
                    Some("已使用讯飞星火生成学习计划".to_string()),
                ),
                Err(error) => {
                    let reason = error.fallback_reason();
                    let fallback_message = format!("讯飞星火计划生成使用模板 fallback：{error}");
                    log::warn!("[learning_assistant] {fallback_message}");
                    (
                        build_understanding(&input),
                        build_goal_stage_plan(&input, &profile, &kb_context.candidates),
                        Some(reason),
                        Some(fallback_message),
                    )
                }
            };

        let stages = apply_local_stage_allocation(stages, &local_allocation);
        Ok(LearningAssistantPlanResult {
            success: true,
            engine_root: root.to_string_lossy().to_string(),
            skill_path: root.join(LEARNING_SKILL_MD).to_string_lossy().to_string(),
            template_path: root.join(PLAN_TEMPLATE_JSON).to_string_lossy().to_string(),
            understanding,
            stages,
            plan_strategy: Some(profile.plan_strategy.to_string()),
            goal_profile_summary: Some(profile.goal_profile_summary.clone()),
            local_allocation: Some(local_allocation),
            message,
            fallback_reason,
            error: None,
        })
    }

    pub async fn adjust_plan(
        db: &Database,
        input: LearningPlanAdjustInput,
    ) -> Result<LearningPlanAdjustResult, AppError> {
        adjust_learning_plan(db, input).await
    }
}

fn parse_dir(label: &str, value: &str) -> Result<PathBuf, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(format!("{label} cannot be empty")));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_dir() {
        if path.is_relative() {
            if let Ok(current) = std::env::current_dir() {
                let candidates = [
                    current.join(&path),
                    current.parent().map(|p| p.join(&path)).unwrap_or_default(),
                    current
                        .parent()
                        .and_then(|p| p.parent())
                        .map(|p| p.join(&path))
                        .unwrap_or_default(),
                ];
                for candidate in candidates {
                    if candidate.is_dir() {
                        return Ok(candidate);
                    }
                }
            }
        }
        return Err(AppError::NotFound(format!(
            "{label} does not exist or is not a directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn validate_engine(root_value: &str) -> Result<PathBuf, AppError> {
    let root = parse_dir("learning-assistant root", root_value)?;
    let skill_path = root.join(LEARNING_SKILL_MD);
    let template_path = root.join(PLAN_TEMPLATE_JSON);

    if !skill_path.is_file() {
        return Err(AppError::NotFound(format!(
            "Missing learning assistant skill file: {}",
            skill_path.display()
        )));
    }
    if !template_path.is_file() {
        return Err(AppError::NotFound(format!(
            "Missing learning plan template: {}",
            template_path.display()
        )));
    }

    let _skill_text = fs::read_to_string(&skill_path)?;
    let _template_text = fs::read_to_string(&template_path)?;
    for required in [GENERATE_PLAN_WORKFLOW, PLANNING_RULES, SCORING_RULES] {
        let required_path = root.join(required);
        if !required_path.is_file() {
            return Err(AppError::NotFound(format!(
                "Missing learning assistant skill reference: {}",
                required_path.display()
            )));
        }
        let _ = fs::read_to_string(required_path)?;
    }
    Ok(root)
}

async fn adjust_learning_plan(
    db: &Database,
    input: LearningPlanAdjustInput,
) -> Result<LearningPlanAdjustResult, AppError> {
    if input.stages.is_empty() {
        return Err(AppError::InvalidInput(
            "learning plan stages cannot be empty".to_string(),
        ));
    }
    if input.stage_index >= input.stages.len() {
        return Err(AppError::InvalidInput(format!(
            "stageIndex {} is outside current plan",
            input.stage_index
        )));
    }

    let percentage = score_percentage(input.score, input.max_score);
    let rule_band = adjustment_rule_band(percentage);
    let ai_adjustment = match build_ai_plan_adjustment(&input, rule_band, percentage).await {
        Ok(adjustment) => Some(adjustment),
        Err(error) => {
            log::warn!("[learning_assistant] dynamic plan adjustment uses local fallback: {error}");
            None
        }
    };
    let kb_resource_tasks = build_adjustment_kb_resource_tasks(db, &input);

    Ok(apply_adjustment_rule(
        &input,
        rule_band,
        percentage,
        ai_adjustment,
        kb_resource_tasks,
    ))
}

async fn build_ai_plan_adjustment(
    input: &LearningPlanAdjustInput,
    rule_band: &str,
    percentage: u32,
) -> Result<AiPlanAdjustment, AppError> {
    let config = LearningAiConfig::from_runtime_or_env()?;
    let prompt = build_plan_adjustment_prompt(input, rule_band, percentage);
    let content = call_learning_ai(&config, &prompt).await?;
    parse_ai_plan_adjustment(&content)
}

fn build_plan_adjustment_prompt(
    input: &LearningPlanAdjustInput,
    rule_band: &str,
    percentage: u32,
) -> String {
    let stage = &input.stages[input.stage_index];
    let next_stage = input
        .stages
        .get(input.stage_index + 1)
        .map(|stage| stage.name.as_str())
        .unwrap_or("无后续阶段");
    let weak_points = fallback_weak_points(&collect_adjustment_weak_points(input)).join("、");
    let current_tasks = [
        stage.learning_tasks.join("；"),
        stage.practice_tasks.join("；"),
        stage.check_tasks.join("；"),
    ]
    .join("\n");

    format!(
        r#"你是 Pomegranate AI 助学模块中的计划调整助手。请只根据用户当前课程、阶段测试结果和薄弱点，生成用于“修改学习计划”的具体任务文字和原因。

本地代码已经决定调整规则，你不得改变分数档位、是否推进、是否重学、是否锁定后续阶段。
不要编造外部资源、URL、视频、论文或跨课程内容。资源任务只能写“本地 knowledge_points 资料、教材章节、课堂笔记、例题、错题复盘”等可验证来源。

请只输出 JSON，不要 Markdown。
JSON 字段：
{{
  "reason": "本次调整原因，说明分数、薄弱点和推进方式",
  "addedTasks": ["新增任务总览"],
  "reviewTasks": ["薄弱点复习任务"],
  "practiceTasks": ["针对性基础练习或综合练习"],
  "checkTasks": ["重新检验或阶段验收任务"],
  "improvementTasks": ["提高任务或综合应用题"]
}}

课程：{course}
当前基础：{level}
最终目标：{final_goal}
每日学习时间：{daily_time}
学习周期：{cycle}
当前阶段序号：{stage_index}
当前阶段：{stage_name}
下一阶段：{next_stage}
得分：{score}/{max_score}（{percentage}%）
掌握等级：{mastery}
本地规则档位：{rule_band}
薄弱点与缺失关键词：{weak_points}
评分反馈：{feedback}
复习建议：{suggestions}
当前阶段任务：
{current_tasks}"#,
        course = clean_or(&input.course_name, "机械制造工艺学"),
        level = clean_or(&input.current_level, "未填写"),
        final_goal = clean_or(&input.final_goal, "未填写"),
        daily_time = clean_or(&input.daily_time, "未填写"),
        cycle = clean_or(&input.learning_cycle, "未填写"),
        stage_index = input.stage_index + 1,
        stage_name = stage.name,
        next_stage = next_stage,
        score = input.score,
        max_score = input.max_score,
        percentage = percentage,
        mastery = clean_or(&input.mastery_level, "未给出"),
        rule_band = rule_band,
        weak_points = weak_points,
        feedback = clean_or(&input.feedback, "无"),
        suggestions = input.review_suggestions.join("；"),
        current_tasks = current_tasks,
    )
}

fn parse_ai_plan_adjustment(content: &str) -> Result<AiPlanAdjustment, AppError> {
    let json_text = extract_json_object(content)
        .ok_or_else(|| AppError::Custom("AI plan adjustment response is not JSON".to_string()))?;
    let parsed = serde_json::from_str::<AiPlanAdjustment>(json_text)
        .map_err(|e| AppError::Custom(format!("AI plan adjustment JSON parse failed: {e}")))?;

    let has_tasks = parsed
        .reason
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || parsed
            .added_tasks
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        || parsed
            .review_tasks
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        || parsed
            .practice_tasks
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        || parsed
            .check_tasks
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        || parsed
            .improvement_tasks
            .as_ref()
            .is_some_and(|items| !items.is_empty());

    if !has_tasks {
        return Err(AppError::Custom(
            "AI plan adjustment JSON has no usable adjustment fields".to_string(),
        ));
    }

    Ok(parsed)
}

fn apply_adjustment_rule(
    input: &LearningPlanAdjustInput,
    rule_band: &str,
    percentage: u32,
    ai_adjustment: Option<AiPlanAdjustment>,
    kb_resource_tasks: Vec<String>,
) -> LearningPlanAdjustResult {
    let mut stages = input.stages.clone();
    let current = input.stage_index;
    let weak_points = fallback_weak_points(&collect_adjustment_weak_points(input));
    let source = if ai_adjustment.is_some() {
        "spark"
    } else {
        "fallback"
    }
    .to_string();
    let ai = ai_adjustment.unwrap_or_default();
    let mut added_tasks = Vec::new();
    let delayed_tasks = Vec::new();
    let locked_stage_indexes = Vec::new();

    let review_tasks = adjustment_tasks(
        ai.review_tasks,
        &weak_points,
        "补弱任务",
        "复习并口头说明",
        3,
    );
    let practice_tasks = adjustment_tasks(
        ai.practice_tasks,
        &weak_points,
        "针对性练习",
        "完成基础例题并记录错因",
        3,
    );
    let check_tasks = non_empty_or(
        clean_task_list(ai.check_tasks.unwrap_or_default(), 3),
        vec![format!(
            "重新检验：围绕{}完成阶段小测并复盘",
            weak_points.join("、")
        )],
    );
    let improvement_tasks = non_empty_or(
        clean_task_list(ai.improvement_tasks.unwrap_or_default(), 2),
        vec![
            "提高任务：完成 1 道综合应用题并说明解题路径".to_string(),
            "提高任务：把本阶段知识点整理成可复用检查清单".to_string(),
        ],
    );
    let ai_added_tasks = clean_task_list(ai.added_tasks.unwrap_or_default(), 4);

    match rule_band {
        "excellent" => {
            mark_stage(&mut stages[current], "已调整：当前阶段已完成，正常推进。");
            if let Some(next) = stages.get_mut(current + 1) {
                append_tasks(&mut next.practice_tasks, &improvement_tasks, 8);
                append_tasks(
                    &mut next.check_tasks,
                    &["完成新增提高任务后做一次综合验收。".to_string()],
                    8,
                );
            } else {
                append_tasks(&mut stages[current].practice_tasks, &improvement_tasks, 8);
            }
            added_tasks.extend(improvement_tasks);
            added_tasks.extend(ai_added_tasks);
            LearningPlanAdjustResult {
                stages,
                conclusion: "正常推进，增加提高任务。".to_string(),
                reason: adjustment_reason(
                    ai.reason,
                    percentage,
                    &weak_points,
                    "得分达到 80 分及以上，当前阶段掌握较好，可以正常进入后续阶段；如需调整，可少量增加提高任务。",
                ),
                source,
                rule_band: rule_band.to_string(),
                current_stage_status: "completed".to_string(),
                can_advance: true,
                need_retest: false,
                weak_points,
                added_tasks: dedupe_non_empty(added_tasks, 6),
                delayed_tasks,
                locked_stage_indexes,
            }
        }
        "basic" => {
            mark_stage(
                &mut stages[current],
                "已调整：当前阶段基本掌握，可以继续后续学习。",
            );
            if let Some(next) = stages.get_mut(current + 1) {
                prepend_tasks(&mut next.learning_tasks, &review_tasks, 8);
                prepend_tasks(&mut next.resource_tasks, &kb_resource_tasks, 8);
                prepend_tasks(&mut next.practice_tasks, &practice_tasks, 8);
                prepend_tasks(
                    &mut next.check_tasks,
                    &["继续后续学习前，先完成薄弱点复盘检查。".to_string()],
                    8,
                );
            }
            added_tasks.extend(review_tasks);
            added_tasks.extend(practice_tasks);
            added_tasks.extend(ai_added_tasks);
            LearningPlanAdjustResult {
                stages,
                conclusion: "基本掌握，可以继续后续学习，并建议复习薄弱知识点。".to_string(),
                reason: adjustment_reason(
                    ai.reason,
                    percentage,
                    &weak_points,
                    "得分处于 60 分至 80 分以下，当前阶段达到基本掌握；可以继续后续学习，但建议复习薄弱知识点。",
                ),
                source,
                rule_band: rule_band.to_string(),
                current_stage_status: "completed".to_string(),
                can_advance: true,
                need_retest: false,
                weak_points,
                added_tasks: dedupe_non_empty(added_tasks, 8),
                delayed_tasks,
                locked_stage_indexes,
            }
        }
        _ => {
            mark_stage(
                &mut stages[current],
                "重新学习：建议优先补齐当前阶段薄弱知识点。",
            );
            stages[current].learning_tasks = limited(
                [
                    review_tasks.clone(),
                    vec![
                        "重新学习：回到本阶段基础概念、基本步骤和典型例题。".to_string(),
                        "重新学习：用自己的话重述每个核心概念并补齐笔记。".to_string(),
                    ],
                ]
                .concat(),
                8,
            );
            stages[current].resource_tasks =
                limited(local_resource_tasks(&weak_points, &kb_resource_tasks), 6);
            stages[current].practice_tasks = limited(
                [
                    practice_tasks.clone(),
                    vec!["重新学习：先做简单练习，再做原错题同类题。".to_string()],
                ]
                .concat(),
                8,
            );
            stages[current].check_tasks = limited(
                [
                    check_tasks.clone(),
                    vec!["重新测试：完成重学任务后再次开始本阶段测试。".to_string()],
                ]
                .concat(),
                8,
            );
            append_tasks(
                &mut stages[current].completion_criteria,
                &["建议复测达到 60 分以上后再继续推进。".to_string()],
                8,
            );
            added_tasks.extend(review_tasks);
            added_tasks.extend(practice_tasks);
            added_tasks.extend(check_tasks);
            added_tasks.extend(ai_added_tasks);
            LearningPlanAdjustResult {
                stages,
                conclusion: "建议重新学习本阶段并重新测试，后续阶段不锁定。".to_string(),
                reason: adjustment_reason(
                    ai.reason,
                    percentage,
                    &weak_points,
                    "得分低于 60 分，系统根据薄弱知识点生成重新学习建议；是否采用该调整由用户决定，不采用也不会锁定后续阶段。",
                ),
                source,
                rule_band: rule_band.to_string(),
                current_stage_status: "重新学习".to_string(),
                can_advance: false,
                need_retest: true,
                weak_points,
                added_tasks: dedupe_non_empty(added_tasks, 10),
                delayed_tasks,
                locked_stage_indexes,
            }
        }
    }
}

fn score_percentage(score: u32, max_score: u32) -> u32 {
    if max_score == 0 {
        return 0;
    }
    (((score as f64 / max_score as f64) * 100.0).round() as u32).min(100)
}

fn adjustment_rule_band(percentage: u32) -> &'static str {
    if percentage >= 80 {
        "excellent"
    } else if percentage >= 60 {
        "basic"
    } else {
        "relearn"
    }
}

fn collect_adjustment_weak_points(input: &LearningPlanAdjustInput) -> Vec<String> {
    let mut values = Vec::new();
    values.extend(input.weak_points.clone());
    values.extend(input.wrong_knowledge_points.clone());
    values.extend(
        input
            .missing_keywords
            .iter()
            .map(|keyword| format!("缺失关键词：{keyword}")),
    );
    dedupe_non_empty(values, 12)
}

fn fallback_weak_points(points: &[String]) -> Vec<String> {
    if points.is_empty() {
        vec!["本阶段核心知识点".to_string()]
    } else {
        points.to_vec()
    }
}

fn adjustment_tasks(
    ai_tasks: Option<Vec<String>>,
    weak_points: &[String],
    label: &str,
    action: &str,
    limit_count: usize,
) -> Vec<String> {
    let tasks = clean_task_list(ai_tasks.unwrap_or_default(), limit_count);
    if !tasks.is_empty() {
        return tasks;
    }
    weak_points
        .iter()
        .take(limit_count)
        .map(|point| format!("{label}：围绕“{point}”{action}。"))
        .collect()
}

fn build_adjustment_kb_resource_tasks(
    db: &Database,
    input: &LearningPlanAdjustInput,
) -> Vec<String> {
    let weak_points = collect_adjustment_weak_points(input);
    let stage = &input.stages[input.stage_index];
    let query = fallback_weak_points(&weak_points).join(" ");
    let search_input = LearningKbSearchInput {
        course: clean_or(&input.course_name, "机械制造工艺学"),
        query,
        stage_name: stage.name.clone(),
        stage_index: Some(input.stage_index + 1),
        stage_goal: stage.goal.clone(),
        learning_tasks: stage.learning_tasks.clone(),
        resource_tasks: stage.resource_tasks.clone(),
        practice_tasks: stage.practice_tasks.clone(),
        check_tasks: stage.check_tasks.clone(),
        knowledge_points: fallback_weak_points(&weak_points),
        top_k: 3,
        document_source_ids: Vec::new(),
        selected_learning_sources: Vec::new(),
    };

    match LearningKbService::search(db, db.data_dir(), search_input) {
        Ok(result) => result
            .results
            .into_iter()
            .take(3)
            .map(|item| {
                format!(
                    "补弱资料：阅读本地知识库《{}》/{} 中“{}”相关内容。",
                    item.source_file, item.section, item.title
                )
            })
            .collect(),
        Err(error) => {
            log::warn!("[learning_assistant] dynamic adjustment kb search failed: {error}");
            Vec::new()
        }
    }
}

fn local_resource_tasks(weak_points: &[String], kb_resource_tasks: &[String]) -> Vec<String> {
    let mut tasks = kb_resource_tasks.to_vec();
    tasks.extend(weak_points.iter().take(3).map(|point| {
        format!("补弱资料：查阅本地 knowledge_points、教材章节和课堂笔记中“{point}”相关内容。")
    }));
    dedupe_non_empty(tasks, 5)
}

fn adjustment_reason(
    ai_reason: Option<String>,
    percentage: u32,
    weak_points: &[String],
    fallback: &str,
) -> String {
    let cleaned = ai_reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    cleaned.unwrap_or_else(|| {
        format!(
            "{fallback} 本次得分为 {percentage}%，主要薄弱点：{}。",
            weak_points.join("、")
        )
    })
}

fn mark_stage(stage: &mut LearningAssistantStage, marker: &str) {
    if !stage.goal.contains(marker) {
        stage.goal = format!("{marker} {}", stage.goal);
    }
}

fn prepend_tasks(target: &mut Vec<String>, tasks: &[String], max_count: usize) {
    let mut next = tasks.to_vec();
    next.extend(target.iter().cloned());
    *target = dedupe_non_empty(next, max_count);
}

fn append_tasks(target: &mut Vec<String>, tasks: &[String], max_count: usize) {
    target.extend(tasks.iter().cloned());
    *target = dedupe_non_empty(target.clone(), max_count);
}

fn clean_task_list(tasks: Vec<String>, max_count: usize) -> Vec<String> {
    dedupe_non_empty(tasks, max_count)
}

fn limited(tasks: Vec<String>, max_count: usize) -> Vec<String> {
    dedupe_non_empty(tasks, max_count)
}

fn dedupe_non_empty(tasks: Vec<String>, max_count: usize) -> Vec<String> {
    let mut output = Vec::new();
    for task in tasks {
        let text = task.trim();
        if text.is_empty() || output.iter().any(|item: &String| item == text) {
            continue;
        }
        output.push(text.to_string());
        if output.len() >= max_count {
            break;
        }
    }
    output
}

fn build_goal_plan_profile(input: &LearningAssistantPlanInput) -> GoalPlanProfile {
    let goal_type =
        LearningGoal::parse(&input.learning_goal).unwrap_or(LearningGoal::SystematicLearning);
    let config = learning_goal_config(goal_type);
    let (recommended_stage_count, coverage_mode, depth_mode, mastery, task_focus, entry_count_range, assessment_mode, prompt_instruction, plan_strategy, stage_templates) =
        match goal_type {
            LearningGoal::FinalSprint => (
                3,
                "短期冲刺，只覆盖高频、基础、典型题和易错点",
                "紧凑提分，减少背景阅读",
                vec!["掌握", "熟练应用"],
                vec!["公式原则", "判断方法", "典型题", "易错点", "限时检查"],
                (3, 10),
                "每日限时自测与错题复盘",
                "生成 3 天冲刺计划，每个词条必须对应复习动作、题型练习和检查方法，不安排大段背景阅读。",
                "当前采用短期冲刺策略，优先安排考试高频词条、典型题和易错点。",
                vec![
                    "第 1 天：核心概念与基本原则速查",
                    "第 2 天：计算方法、设计方法和典型题",
                    "第 3 天：易错点、综合题和模拟检查",
                ],
            ),
            LearningGoal::GapFilling => (
                4,
                "薄弱项诊断和前置知识补齐",
                "先诊断再补弱，强调原因和复测",
                vec!["理解", "掌握"],
                vec!["诊断", "前置知识", "纠正误区", "再验证"],
                (3, 8),
                "诊断任务、补学任务和再次检查",
                "生成查漏补缺计划，必须说明 weakReason、prerequisite 和 retryTask；没有真实测试数据时标明依据当前基础和依赖关系生成待检查词条。",
                "当前采用薄弱项诊断策略，优先安排前置知识、易错词条和再次检查。",
                vec![
                    "阶段 1：诊断现状并定位薄弱知识点",
                    "阶段 2：补齐基础概念和前置知识",
                    "阶段 3：纠正常见错误并加强典型题",
                    "阶段 4：再次检查并确认薄弱项已补齐",
                ],
            ),
            LearningGoal::SystematicLearning => (
                5,
                "按章节结构和 learning_order 尽量覆盖主要章节",
                "理解到掌握递进，保持知识依赖顺序",
                vec!["了解", "理解", "掌握"],
                vec!["章节顺序", "前置关系", "结构化笔记", "阶段练习"],
                (4, 12),
                "阶段测验与知识结构输出",
                "生成系统递进计划，按章节和知识依赖安排词条，体现了解、理解、掌握的层次递进。",
                "当前采用系统递进策略，按章节和知识依赖关系安排词条。",
                vec![
                    "阶段 1：课程框架与基础概念",
                    "阶段 2：机械加工工艺规程设计",
                    "阶段 3：机床夹具设计",
                    "阶段 4：加工精度与表面质量控制",
                    "阶段 5：装配工艺与课程综合复盘",
                ],
            ),
            LearningGoal::ComprehensiveImprovement => (
                5,
                "跨章节综合应用和方案比较",
                "高阶应用，减少简单定义重复",
                vec!["掌握", "熟练应用"],
                vec!["综合分析", "方案比较", "误差控制", "工程应用", "成果输出"],
                (3, 10),
                "综合案例、方案评价和成果输出",
                "生成综合提升计划，必须包含跨章节综合分析、方案比较和熟练应用任务，不能只是系统学习延长版。",
                "当前采用综合应用策略，重点安排跨章节分析、方案比较和综合问题解决。",
                vec![
                    "阶段 1：核心知识快速诊断与整合",
                    "阶段 2：工艺路线与夹具方案综合分析",
                    "阶段 3：精度、表面质量和误差控制",
                    "阶段 4：装配工艺与跨章节综合应用",
                    "阶段 5：综合案例、方案评价和成果输出",
                ],
            ),
        };

    GoalPlanProfile {
        goal_type,
        cycle: match goal_type {
            LearningGoal::FinalSprint => "3天",
            LearningGoal::GapFilling => "2周",
            LearningGoal::SystematicLearning => "3周",
            LearningGoal::ComprehensiveImprovement => "4周",
        },
        recommended_stage_count,
        coverage_mode,
        depth_mode,
        preferred_mastery_levels: mastery,
        task_focus,
        entry_count_range,
        assessment_mode,
        prompt_instruction,
        plan_strategy,
        goal_profile_summary: format!(
            "goalType={:?}; cycle={}; recommendedStageCount={}; coverageMode={}; depthMode={}; assessmentMode={}; configuredDays={}",
            goal_type,
            match goal_type {
                LearningGoal::FinalSprint => "3天",
                LearningGoal::GapFilling => "2周",
                LearningGoal::SystematicLearning => "3周",
                LearningGoal::ComprehensiveImprovement => "4周",
            },
            recommended_stage_count,
            coverage_mode,
            depth_mode,
            assessment_mode,
            config.total_days
        ),
        stage_templates,
    }
}

fn normalize_goal_specific_plan(
    mut stages: Vec<LearningAssistantStage>,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    candidates: &[LearningEntryCandidate],
) -> Vec<LearningAssistantStage> {
    if stages.is_empty() {
        return build_goal_stage_plan(input, profile, candidates);
    }
    let fallback = build_goal_stage_plan(input, profile, candidates);
    if stages.len() < profile.recommended_stage_count {
        stages.extend(fallback.iter().skip(stages.len()).cloned());
    }
    stages.truncate(profile.recommended_stage_count);

    for (index, stage) in stages.iter_mut().enumerate() {
        if stage.name.trim().is_empty() {
            stage.name = profile.stage_templates[index % profile.stage_templates.len()].to_string();
        }
        let existing = stage.learning_entries.take().unwrap_or_default();
        let normalized = normalize_learning_entries(existing, index, input, profile, candidates);
        stage.knowledge_points = Some(normalized.iter().map(|entry| entry.title.clone()).collect());
        stage.learning_tasks = non_empty_or(
            stage.learning_tasks.clone(),
            normalized
                .iter()
                .map(|entry| entry.study_action.clone())
                .collect(),
        );
        stage.practice_tasks = non_empty_or(
            stage.practice_tasks.clone(),
            normalized
                .iter()
                .map(|entry| entry.practice_action.clone())
                .collect(),
        );
        stage.check_tasks = non_empty_or(
            stage.check_tasks.clone(),
            normalized
                .iter()
                .map(|entry| entry.check_method.clone())
                .collect(),
        );
        stage.completion_criteria = non_empty_or(
            stage.completion_criteria.clone(),
            normalized
                .iter()
                .map(|entry| entry.expected_output.clone())
                .collect(),
        );
        stage.learning_entries = Some(normalized);
    }
    validate_goal_specific_plan(stages, input, profile, candidates)
}

fn build_goal_stage_plan(
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    candidates: &[LearningEntryCandidate],
) -> Vec<LearningAssistantStage> {
    (0..profile.recommended_stage_count)
        .map(|index| {
            let entries = build_stage_entries(index, input, profile, candidates);
            let titles = entries
                .iter()
                .map(|entry| entry.title.clone())
                .collect::<Vec<_>>();
            LearningAssistantStage {
                name: profile.stage_templates[index].to_string(),
                time_range: format!("{}，阶段 {}", profile.cycle, index + 1),
                goal: stage_goal_text(index, profile),
                knowledge_points: Some(titles),
                learning_entries: Some(entries.clone()),
                learning_tasks: entries
                    .iter()
                    .map(|entry| entry.study_action.clone())
                    .collect(),
                resource_tasks: entries
                    .iter()
                    .map(|entry| match entry.source_file.as_deref() {
                        Some(file) => {
                            format!("阅读本地知识库来源 {} 中的“{}”。", file, entry.title)
                        }
                        None => format!(
                            "当前词条未匹配到本地知识库来源，按课程模板核对“{}”。",
                            entry.title
                        ),
                    })
                    .collect(),
                practice_tasks: entries
                    .iter()
                    .map(|entry| entry.practice_action.clone())
                    .collect(),
                check_tasks: entries
                    .iter()
                    .map(|entry| entry.check_method.clone())
                    .collect(),
                completion_criteria: entries
                    .iter()
                    .map(|entry| entry.expected_output.clone())
                    .collect(),
            }
        })
        .collect()
}

fn normalize_learning_entries(
    entries: Vec<LearningPlanEntry>,
    stage_index: usize,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    candidates: &[LearningEntryCandidate],
) -> Vec<LearningPlanEntry> {
    let allowed_sources = candidates
        .iter()
        .map(|item| item.source_file.as_str())
        .collect::<Vec<_>>();
    let mut seen = Vec::<String>::new();
    let mut normalized = entries
        .into_iter()
        .filter_map(|entry| {
            let title = entry.title.trim().to_string();
            if title.is_empty() || is_generic_entry_title(&title) {
                return None;
            }
            if seen.iter().any(|item| item == &title) {
                return None;
            }
            seen.push(title.clone());
            Some(fill_missing_entry_fields(
                entry,
                stage_index,
                input,
                profile,
                &allowed_sources,
            ))
        })
        .collect::<Vec<_>>();

    let target_count = target_entry_count(input, profile);
    if normalized.len() < target_count {
        let mut generated = build_stage_entries(stage_index, input, profile, candidates);
        generated.retain(|entry| !seen.iter().any(|title| title == &entry.title));
        normalized.extend(
            generated
                .into_iter()
                .take(target_count.saturating_sub(normalized.len())),
        );
    }
    normalized.truncate(profile.entry_count_range.1);
    if normalized.is_empty() {
        normalized = build_stage_entries(stage_index, input, profile, candidates)
            .into_iter()
            .take(1)
            .collect();
    }
    normalized
}

fn fill_missing_entry_fields(
    mut entry: LearningPlanEntry,
    stage_index: usize,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    allowed_sources: &[&str],
) -> LearningPlanEntry {
    entry.mastery_level = normalize_mastery_level(&entry.mastery_level, stage_index, profile);
    if entry.entry_id.trim().is_empty() {
        entry.entry_id = format!(
            "stage{}-{}",
            stage_index + 1,
            stable_entry_slug(&entry.title)
        );
    }
    entry.entry_type = clean_or(
        &entry.entry_type,
        infer_entry_type(&entry.title, &entry.reason),
    );
    entry.study_action = clean_or(
        &entry.study_action,
        &study_action_for(&entry.title, &entry.mastery_level, profile),
    );
    entry.practice_action = clean_or(
        &entry.practice_action,
        &practice_action_for(&entry.title, profile),
    );
    entry.check_method = clean_or(
        &entry.check_method,
        &check_method_for(&entry.title, profile),
    );
    entry.expected_output = clean_or(
        &entry.expected_output,
        &format!("形成“{}”的可检查笔记、例题记录或判断清单。", entry.title),
    );
    entry.reason = clean_or(&entry.reason, &reason_for(&entry.title, input, profile));
    if entry.estimated_minutes == 0 {
        entry.estimated_minutes = entry_minutes(input, profile);
    }
    entry.estimated_minutes = entry.estimated_minutes.clamp(10, 35);
    if entry
        .source_file
        .as_deref()
        .is_some_and(|source| !allowed_sources.iter().any(|allowed| allowed == &source))
    {
        entry.source_file = None;
        entry.source_type = "modelFallback".to_string();
    }
    if entry.source_file.is_none() && entry.source_type.trim().is_empty() {
        entry.source_type = "modelFallback".to_string();
    }
    if entry.source_file.is_some() {
        entry.source_type = "knowledgeBase".to_string();
    }
    if matches!(profile.goal_type, LearningGoal::GapFilling) {
        if entry.weak_reason.as_deref().unwrap_or("").trim().is_empty() {
            entry.weak_reason = Some("当前依据用户基础和知识依赖关系生成待检查词条。".to_string());
        }
        if entry.retry_task.as_deref().unwrap_or("").trim().is_empty() {
            entry.retry_task = Some(format!(
                "补学后重新解释“{}”，并完成 2 道对应检查题。",
                entry.title
            ));
        }
    }
    entry
}

fn validate_goal_specific_plan(
    mut stages: Vec<LearningAssistantStage>,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    candidates: &[LearningEntryCandidate],
) -> Vec<LearningAssistantStage> {
    for (index, stage) in stages.iter_mut().enumerate() {
        let entries = stage.learning_entries.take().unwrap_or_default();
        stage.learning_entries = Some(normalize_learning_entries(
            entries, index, input, profile, candidates,
        ));
    }
    stages
}

fn build_stage_entries(
    stage_index: usize,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    candidates: &[LearningEntryCandidate],
) -> Vec<LearningPlanEntry> {
    let target = target_entry_count(input, profile);
    let mut pool = if candidates.is_empty() {
        fallback_candidates(profile)
    } else {
        candidates.to_vec()
    };
    rank_candidates(&mut pool, stage_index, input, profile);
    let start = if pool.len() > target {
        (stage_index * target / 2).min(pool.len().saturating_sub(1))
    } else {
        0
    };
    pool.into_iter()
        .cycle()
        .skip(start)
        .take(target)
        .enumerate()
        .map(|(entry_index, candidate)| {
            candidate_to_entry(candidate, stage_index, entry_index, input, profile)
        })
        .collect()
}

fn candidate_to_entry(
    candidate: LearningEntryCandidate,
    stage_index: usize,
    entry_index: usize,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
) -> LearningPlanEntry {
    let mastery = mastery_for_stage(stage_index, profile);
    let source_file = (!candidate.source_file.is_empty()).then_some(candidate.source_file.clone());
    LearningPlanEntry {
        entry_id: format!(
            "s{}e{}-{}",
            stage_index + 1,
            entry_index + 1,
            stable_entry_slug(&candidate.title)
        ),
        title: candidate.title.clone(),
        section: clean_or(
            &candidate.section,
            &profile.stage_templates[stage_index % profile.stage_templates.len()],
        ),
        entry_type: infer_entry_type(&candidate.title, &candidate.content).to_string(),
        mastery_level: mastery.to_string(),
        study_action: study_action_for(&candidate.title, mastery, profile),
        practice_action: practice_action_for(&candidate.title, profile),
        check_method: check_method_for(&candidate.title, profile),
        expected_output: format!(
            "输出“{}”的要点、适用条件和一条可检查例题记录。",
            candidate.title
        ),
        estimated_minutes: entry_minutes(input, profile),
        reason: reason_for(&candidate.title, input, profile),
        source_file,
        source_type: if candidate.source_file.is_empty() {
            "modelFallback".to_string()
        } else {
            "knowledgeBase".to_string()
        },
        prerequisite: Some(prerequisites_from_text(&candidate.content)),
        weak_reason: matches!(profile.goal_type, LearningGoal::GapFilling)
            .then(|| "当前依据用户基础和知识依赖关系生成待检查词条。".to_string()),
        retry_task: matches!(profile.goal_type, LearningGoal::GapFilling).then(|| {
            format!(
                "补学后重新完成“{}”的口头解释和 2 道基础检查题。",
                candidate.title
            )
        }),
    }
}

fn fallback_candidates(profile: &GoalPlanProfile) -> Vec<LearningEntryCandidate> {
    let titles = match profile.goal_type {
        LearningGoal::FinalSprint => vec![
            "生产纲领与生产类型的判定",
            "粗基准选择原则",
            "精基准选择原则",
            "工序集中与工序分散",
            "加工余量的概念及确定方法",
            "六点定位原理",
            "工艺尺寸链的建立与计算",
            "完全定位、不完全定位、欠定位和过定位",
            "工艺系统几何误差",
            "工艺系统受力变形",
        ],
        LearningGoal::GapFilling => vec![
            "机械加工工艺过程与工艺规程的基本概念",
            "定位基准与设计基准的区别",
            "粗基准选择原则",
            "精基准选择原则",
            "六点定位原理",
            "定位误差的组成",
            "工艺尺寸链的建立与计算",
            "加工误差的统计分析",
        ],
        LearningGoal::SystematicLearning => vec![
            "机械制造工艺学的研究对象",
            "机械加工工艺过程与工艺规程的基本概念",
            "生产纲领与生产类型的判定",
            "零件结构工艺性分析",
            "粗基准选择原则",
            "精基准选择原则",
            "工序集中与工序分散",
            "六点定位原理",
            "夹紧力方向和作用点选择",
            "工艺系统几何误差",
            "机械加工表面质量",
            "装配尺寸链",
        ],
        LearningGoal::ComprehensiveImprovement => vec![
            "工艺路线方案比较",
            "定位方案与夹具方案综合分析",
            "工艺尺寸链的建立与计算",
            "加工误差综合分析",
            "工艺系统受力变形控制",
            "表面质量影响因素与控制措施",
            "装配尺寸链与装配精度保证",
            "典型零件工艺方案评价",
            "跨章节综合工艺设计",
        ],
    };
    titles
        .into_iter()
        .map(|title| LearningEntryCandidate {
            title: title.to_string(),
            section: "本地模板候选".to_string(),
            content: String::new(),
            source_file: String::new(),
            score: 0.0,
        })
        .collect()
}

fn rank_candidates(
    candidates: &mut [LearningEntryCandidate],
    stage_index: usize,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
) {
    candidates.sort_by(|left, right| {
        let left_score = candidate_goal_score(left, stage_index, input, profile);
        let right_score = candidate_goal_score(right, stage_index, input, profile);
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn candidate_goal_score(
    candidate: &LearningEntryCandidate,
    stage_index: usize,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
) -> f64 {
    let text = format!(
        "{} {} {}",
        candidate.title, candidate.section, candidate.content
    );
    let mut score = candidate.score;
    for keyword in &profile.task_focus {
        if text.contains(keyword) {
            score += 8.0;
        }
    }
    if text.contains(&input.final_goal) {
        score += 5.0;
    }
    match profile.goal_type {
        LearningGoal::FinalSprint => {
            for keyword in ["原则", "方法", "计算", "误差", "定位", "尺寸链", "典型"]
            {
                if text.contains(keyword) {
                    score += 6.0;
                }
            }
            for keyword in ["发展", "历史", "概述"] {
                if text.contains(keyword) {
                    score -= 8.0;
                }
            }
        }
        LearningGoal::GapFilling => {
            for keyword in ["基础", "概念", "基准", "定位", "误差", "前置"] {
                if text.contains(keyword) {
                    score += 5.0;
                }
            }
        }
        LearningGoal::SystematicLearning => {
            score += stage_index as f64;
        }
        LearningGoal::ComprehensiveImprovement => {
            for keyword in ["综合", "方案", "比较", "分析", "控制", "应用", "尺寸链"]
            {
                if text.contains(keyword) {
                    score += 7.0;
                }
            }
        }
    }
    score
}

fn target_entry_count(input: &LearningAssistantPlanInput, profile: &GoalPlanProfile) -> usize {
    let daily_hours = if input.daily_study_hours > 0.0 {
        input.daily_study_hours
    } else {
        parse_legacy_daily_hours(&input.daily_time).unwrap_or(1.0)
    };
    let days = learning_goal_config(profile.goal_type).total_days.max(1) as f64;
    let stage_budget = daily_hours * 60.0 * days / profile.recommended_stage_count.max(1) as f64;
    let base_minutes = match profile.goal_type {
        LearningGoal::FinalSprint => 18.0,
        LearningGoal::GapFilling => 22.0,
        LearningGoal::SystematicLearning => 24.0,
        LearningGoal::ComprehensiveImprovement => 28.0,
    };
    let by_time = (stage_budget / base_minutes).floor().max(1.0) as usize;
    let soft_min = if stage_budget >= profile.entry_count_range.0 as f64 * 10.0 {
        profile.entry_count_range.0
    } else {
        1
    };
    by_time.clamp(soft_min, profile.entry_count_range.1)
}

fn entry_minutes(input: &LearningAssistantPlanInput, profile: &GoalPlanProfile) -> u32 {
    let daily_hours = if input.daily_study_hours > 0.0 {
        input.daily_study_hours
    } else {
        parse_legacy_daily_hours(&input.daily_time).unwrap_or(1.0)
    };
    let days = learning_goal_config(profile.goal_type).total_days.max(1) as f64;
    let budget = daily_hours * 60.0 * days / profile.recommended_stage_count.max(1) as f64;
    let count = target_entry_count(input, profile).max(1) as f64;
    (budget / count).round().clamp(10.0, 35.0) as u32
}

fn mastery_for_stage(stage_index: usize, profile: &GoalPlanProfile) -> &'static str {
    match profile.goal_type {
        LearningGoal::FinalSprint => {
            if stage_index >= 1 {
                "熟练应用"
            } else {
                "掌握"
            }
        }
        LearningGoal::GapFilling => {
            if stage_index == 0 {
                "理解"
            } else {
                "掌握"
            }
        }
        LearningGoal::SystematicLearning => match stage_index {
            0 => "了解",
            1 | 2 => "理解",
            _ => "掌握",
        },
        LearningGoal::ComprehensiveImprovement => {
            if stage_index >= 1 {
                "熟练应用"
            } else {
                "掌握"
            }
        }
    }
}

fn normalize_mastery_level(value: &str, stage_index: usize, profile: &GoalPlanProfile) -> String {
    let value = value.trim();
    if value.contains("熟练") || value.contains("综合") || value.contains("应用") {
        "熟练应用".to_string()
    } else if value.contains("掌握") || value.contains("计算") || value.contains("判断") {
        "掌握".to_string()
    } else if value.contains("理解") || value.contains("解释") {
        "理解".to_string()
    } else if value.contains("了解") || value.contains("识记") {
        "了解".to_string()
    } else {
        mastery_for_stage(stage_index, profile).to_string()
    }
}

fn infer_entry_type(title: &str, content: &str) -> &'static str {
    let text = format!("{title} {content}");
    if text.contains("计算") || text.contains("尺寸链") {
        "计算"
    } else if text.contains("设计") || text.contains("方案") {
        "设计"
    } else if text.contains("原则") || text.contains("规则") {
        "规则"
    } else if text.contains("误差") || text.contains("分析") {
        "原理"
    } else if text.contains("综合") || text.contains("应用") {
        "综合应用"
    } else {
        "概念"
    }
}

fn study_action_for(title: &str, mastery: &str, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => {
            format!("快速复盘“{title}”的定义、判定步骤和常见易错点，整理成 3 条速记结论。")
        }
        LearningGoal::GapFilling => {
            format!("重新理解“{title}”，先补前置概念，再写出自己容易混淆的地方。")
        }
        LearningGoal::SystematicLearning => {
            format!("按“{mastery}”要求学习“{title}”，记录概念、适用条件和与前后知识点的关系。")
        }
        LearningGoal::ComprehensiveImprovement => {
            format!("围绕“{title}”做跨章节关联分析，说明它如何影响工艺方案或工程结果。")
        }
    }
}

fn practice_action_for(title: &str, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => {
            format!("完成“{title}”对应的 2-3 道典型题或判断题，并标注解题入口。")
        }
        LearningGoal::GapFilling => format!("用 1 道基础题和 1 道易错题验证“{title}”是否补齐。"),
        LearningGoal::SystematicLearning => {
            format!("围绕“{title}”完成例题或章节练习，并补充到知识结构图中。")
        }
        LearningGoal::ComprehensiveImprovement => {
            format!("把“{title}”放入综合案例，比较至少两种处理方案的差异。")
        }
    }
}

fn check_method_for(title: &str, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => format!("限时说明“{title}”的判断依据，并完成一题复算或复判。"),
        LearningGoal::GapFilling => {
            format!("闭卷复述“{title}”的前置关系，并重新做补学后的检查题。")
        }
        LearningGoal::SystematicLearning => {
            format!("能把“{title}”放回章节脉络中，并说明它与前后知识点的关系。")
        }
        LearningGoal::ComprehensiveImprovement => {
            format!("能用“{title}”解释综合方案选择，并说出取舍原因。")
        }
    }
}

fn reason_for(
    title: &str,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
) -> String {
    format!(
        "“{}”匹配当前学习目标“{}”和最终目标“{}”，用于{}。",
        title, input.learning_goal, input.final_goal, profile.depth_mode
    )
}

fn stage_goal_text(index: usize, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => format!(
            "围绕短期提分完成第 {} 天高频词条、典型题和易错点检查。",
            index + 1
        ),
        LearningGoal::GapFilling => format!(
            "完成第 {} 阶段薄弱项诊断、补齐或复测任务，明确每个词条为何需要补。",
            index + 1
        ),
        LearningGoal::SystematicLearning => format!(
            "按章节顺序完成第 {} 阶段知识结构学习，形成递进式理解。",
            index + 1
        ),
        LearningGoal::ComprehensiveImprovement => format!(
            "完成第 {} 阶段跨章节综合分析和方案比较，提升工程应用能力。",
            index + 1
        ),
    }
}

fn prerequisites_from_text(content: &str) -> Vec<String> {
    content
        .split(['；', ';', '，', ',', '、'])
        .map(str::trim)
        .filter(|item| item.chars().count() >= 3)
        .take(3)
        .map(ToString::to_string)
        .collect()
}

fn stable_entry_slug(title: &str) -> String {
    let slug = title
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    if slug.is_empty() {
        format!(
            "{:x}",
            title.bytes().fold(0_u32, |acc, byte| acc
                .wrapping_mul(31)
                .wrapping_add(byte as u32))
        )
    } else {
        slug
    }
}

fn is_generic_entry_title(title: &str) -> bool {
    let text = title.trim();
    text.is_empty()
        || matches!(
            text,
            "学习重点知识" | "复习重点知识" | "完成相关练习" | "查看本地资料"
        )
        || text.ends_with("章")
        || text.contains("学习第")
}

#[cfg(test)]
mod learning_assistant_goal_profile_tests {
    use super::*;

    fn input_for(goal: &str, cycle: &str) -> LearningAssistantPlanInput {
        LearningAssistantPlanInput {
            learning_assistant_root: "../learning-assistant".to_string(),
            learning_goal: goal.to_string(),
            course_name: "机械制造工艺学".to_string(),
            learning_cycle: cycle.to_string(),
            daily_time: "每天1小时".to_string(),
            daily_study_hours: 1.0,
            current_level: "基础一般".to_string(),
            final_goal: "期末成绩达到80分以上".to_string(),
            selected_document_source_ids: Vec::new(),
            selected_learning_sources: Vec::new(),
            plugin_prompt_context: None,
        }
    }

    fn candidates() -> Vec<LearningEntryCandidate> {
        [
            "机械加工工艺过程与工艺规程的基本概念",
            "生产纲领与生产类型的判定",
            "粗基准选择原则",
            "精基准选择原则",
            "工序集中与工序分散",
            "加工余量的概念及确定方法",
            "六点定位原理",
            "完全定位、不完全定位、欠定位和过定位",
            "工艺系统几何误差",
            "工艺系统受力变形",
            "工艺尺寸链的建立与计算",
            "工艺路线方案比较",
        ]
        .iter()
        .enumerate()
        .map(|(index, title)| LearningEntryCandidate {
            title: (*title).to_string(),
            section: format!("测试章节 {}", index + 1),
            content: "原则 方法 计算 误差 综合 分析".to_string(),
            source_file: "knowledge_base_test.xlsx".to_string(),
            score: 10.0 - index as f64 * 0.1,
        })
        .collect()
    }

    #[test]
    fn fallback_plans_are_goal_specific() {
        let cases = [
            ("期末冲刺", "3天", 3),
            ("查漏补缺", "2周", 4),
            ("系统学习", "3周", 5),
            ("综合提升", "4周", 5),
        ];
        let candidates = candidates();
        let mut signatures = Vec::new();

        for (goal, cycle, expected_stage_count) in cases {
            let input = input_for(goal, cycle);
            let profile = build_goal_plan_profile(&input);
            let stages = build_goal_stage_plan(&input, &profile, &candidates);
            assert_eq!(stages.len(), expected_stage_count, "{goal}");
            assert!(stages.iter().all(|stage| stage
                .learning_entries
                .as_ref()
                .is_some_and(|entries| !entries.is_empty())));
            let mastery = stages
                .iter()
                .flat_map(|stage| stage.learning_entries.as_ref().into_iter().flatten())
                .map(|entry| entry.mastery_level.clone())
                .collect::<Vec<_>>();
            signatures.push(format!("{}:{}:{}", goal, stages.len(), mastery.join("/")));
        }

        signatures.sort();
        signatures.dedup();
        assert_eq!(signatures.len(), 4);
    }
}

async fn build_ai_understanding(
    db: &Database,
    input: &LearningAssistantPlanInput,
) -> Result<LearningAssistantUnderstanding, LearningAiFailure> {
    let config = resolve_learning_ai_config_for_call(db)?;
    let prompt = build_goal_understanding_prompt(input);
    let content = call_learning_ai_checked(&config, &prompt).await?;
    let parsed = parse_ai_goal_understanding(&content).map_err(|error| {
        LearningAiFailure::InvalidUnderstandingJson(sanitize_error_message(
            &error.to_string(),
            Some(&config),
        ))
    })?;
    let mut understanding = map_ai_understanding(parsed, input);
    understanding.source = Some(config.source);
    Ok(understanding)
}

async fn build_ai_plan(
    db: &Database,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    kb_context: &PlanKnowledgeContext,
) -> Result<(LearningAssistantUnderstanding, Vec<LearningAssistantStage>), LearningAiFailure> {
    let config = resolve_learning_ai_config_for_call(db)?;
    let prompt = build_goal_specific_plan_prompt(input, profile, kb_context);
    let content = call_learning_ai_checked(&config, &prompt).await?;
    let parsed = parse_ai_plan(&content).map_err(|error| {
        LearningAiFailure::InvalidPlanJson(sanitize_error_message(
            &error.to_string(),
            Some(&config),
        ))
    })?;
    let mut understanding = parsed
        .understanding
        .map(|understanding| map_ai_understanding(understanding, input))
        .unwrap_or_else(|| build_understanding(input));
    understanding.source = Some(config.source);

    let stages = parsed
        .stages
        .into_iter()
        .enumerate()
        .map(|(index, stage)| map_ai_stage(index, stage))
        .collect::<Vec<_>>();

    if stages.is_empty() {
        return Err(LearningAiFailure::InvalidPlanJson(
            "讯飞星火返回的学习计划缺少阶段列表".to_string(),
        ));
    }

    Ok((understanding, stages))
}

async fn call_learning_ai(config: &LearningAiConfig, prompt: &str) -> Result<String, AppError> {
    let url = build_chat_completions_url(&config.api_base);
    let body = json!({
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": "你是一个目标驱动型 AI 助学智能体。请严格按用户要求输出 JSON。"
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "temperature": 0.2,
        "stream": false
    });

    let response = http_client::shared()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", config.api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Custom(format!("学习目标 AI API 请求失败: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Custom(format!(
            "学习目标 AI API 返回非成功状态 {status}: {body}"
        )));
    }

    let data = response
        .json::<Value>()
        .await
        .map_err(|e| AppError::Custom(format!("学习目标 AI API 响应 JSON 解析失败: {e}")))?;

    data["choices"][0]["message"]["content"]
        .as_str()
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| AppError::Custom("学习目标 AI API 响应缺少 message.content".to_string()))
}

async fn call_learning_ai_checked(
    config: &LearningAiConfig,
    prompt: &str,
) -> Result<String, LearningAiFailure> {
    let url = build_chat_completions_url(&config.api_base);
    let body = json!({
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": "你是一个目标驱动型 AI 助学智能体。请严格按用户要求输出 JSON。"
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "temperature": 0.2,
        "stream": false
    });

    let response = http_client::shared()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", config.api_key))
        .json(&body)
        .send()
        .await
        .map_err(|error| {
            LearningAiFailure::Network(sanitize_error_message(&error.to_string(), Some(config)))
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let message = sanitize_error_message(&body, Some(config));
        if !matches!(status.as_u16(), 401 | 403 | 404) && looks_like_invalid_model(&message) {
            return Err(LearningAiFailure::InvalidModel(message));
        }
        return Err(LearningAiFailure::Http {
            status: status.as_u16(),
            message,
        });
    }

    let data = response.json::<Value>().await.map_err(|error| {
        LearningAiFailure::Other(format!(
            "接口响应 JSON 解析失败：{}",
            sanitize_error_message(&error.to_string(), Some(config))
        ))
    })?;

    data["choices"][0]["message"]["content"]
        .as_str()
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
        .ok_or(LearningAiFailure::EmptyContent)
}

fn resolve_learning_ai_config_for_call(
    db: &Database,
) -> Result<LearningAiConfig, LearningAiFailure> {
    resolve_learning_ai_config(db).map_err(|error| {
        let message = sanitize_error_message(&error.to_string(), None);
        let normalized = message.to_lowercase();
        if normalized.contains("api")
            || normalized.contains("spark_api_password")
            || message.contains("未配置")
        {
            LearningAiFailure::NotConfigured(message)
        } else {
            LearningAiFailure::Other(format!("读取模型 API 配置失败：{message}"))
        }
    })
}

fn looks_like_invalid_model(message: &str) -> bool {
    let normalized = message.to_lowercase();
    normalized.contains("invalid model")
        || normalized.contains("model not")
        || normalized.contains("model does not")
        || normalized.contains("unsupported model")
        || normalized.contains("model_not_found")
        || normalized.contains("model_not_exist")
        || normalized.contains("模型不存在")
        || normalized.contains("模型名称")
        || normalized.contains("模型名")
}

fn sanitize_error_message(raw: &str, config: Option<&LearningAiConfig>) -> String {
    let mut sanitized = raw.to_string();
    if let Some(config) = config {
        let key = config.api_key.trim();
        if !key.is_empty() {
            sanitized = sanitized.replace(key, "[REDACTED_API_KEY]");
        }
    }

    sanitized = redact_bearer_tokens(&sanitized);
    sanitized = redact_authorization_lines(&sanitized);

    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "接口未返回错误详情".to_string()
    } else {
        trimmed.to_string()
    }
}

fn redact_bearer_tokens(value: &str) -> String {
    let mut output = Vec::new();
    for token in value.split_whitespace() {
        if token.eq_ignore_ascii_case("bearer") {
            output.push(token.to_string());
            continue;
        }
        if output
            .last()
            .is_some_and(|previous| previous.eq_ignore_ascii_case("bearer"))
        {
            output.push("[REDACTED_API_KEY]".to_string());
        } else {
            output.push(token.to_string());
        }
    }
    output.join(" ")
}

fn redact_authorization_lines(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            if line.to_lowercase().contains("authorization") {
                "Authorization: [REDACTED]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_plugin_prompt_context(prompt: &mut String, input: &LearningAssistantPlanInput) {
    if let Some(context) = input
        .plugin_prompt_context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str("\n\n[PLUGIN AUGMENTED CONTEXT]\n");
        prompt.push_str(context);
        prompt.push_str("\n[/PLUGIN AUGMENTED CONTEXT]");
    }
}

fn build_goal_understanding_prompt(input: &LearningAssistantPlanInput) -> String {
    let mut prompt = format!(
        r#"你是一个目标驱动型 AI 助学智能体。请根据用户输入解析学习目标，输出结构化 JSON。

需要识别：
1. 课程名称
2. 目标类型
3. 学习周期
4. 每日学习时间
5. 当前基础
6. 最终成果要求
7. 需要覆盖的知识点范围
8. 推荐学习策略
9. 建议阶段数
10. 一句话目标摘要

要求：
- 只输出 JSON；
- 不要输出解释性散文；
- 不要使用 Markdown 代码块；
- 不要编造不存在的课程资源；
- 如果信息缺失，用合理默认值，并在 summary 中说明。

JSON 字段：
summary
goalType
courseName
cycle
dailyTime
levelAnalysis
finalGoal
focusPoints
riskPoints
suggestions
source

用户输入：
学习目标：{learning_goal}
课程名称：{course_name}
学习周期：{learning_cycle}
每日学习时间：{daily_time}
当前基础：{current_level}
最终目标：{final_goal}"#,
        learning_goal = clean_or(&input.learning_goal, "未填写"),
        course_name = clean_or(&input.course_name, "未填写"),
        learning_cycle = clean_or(&input.learning_cycle, "未填写"),
        daily_time = clean_or(&input.daily_time, "未填写"),
        current_level = clean_or(&input.current_level, "未填写"),
        final_goal = clean_or(&input.final_goal, "未填写"),
    );
    append_plugin_prompt_context(&mut prompt, input);
    prompt
}

#[allow(dead_code)]
fn build_plan_prompt(input: &LearningAssistantPlanInput, kb_context: Option<&str>) -> String {
    let kb_context = kb_context
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("当前未检索到可用的本地知识库内容，请基于机械制造工艺学课程常识生成计划，但不要编造资源链接。");

    format!(
        r#"你是 Pomegranate / 石榴软件中的 AI 助学智能体。请围绕“机械制造工艺学”为学生生成阶段学习计划，输出结构化 JSON。

必须遵守：
- 只输出 JSON；
- 不要输出 Markdown 代码块；
- 不要编造视频链接、课件链接、文章链接或外部 URL；
- 当前没有 resources 表，resourceTasks 只能写本地知识点资料学习、教材章节学习、例题学习、课堂笔记复习等任务；
- 计划必须结合学习目标、学习周期、每日学习时间、当前基础、最终目标和本地 knowledge_points 知识库上下文；
- 每个阶段至少包含 stageName、duration、stageGoal、knowledgePoints、learningTasks、resourceTasks、practiceTasks、checkTasks、completionCriteria。

输出 JSON 结构：
{{
  "source": "spark",
  "understanding": {{
    "summary": "...",
    "goalType": "...",
    "courseName": "...",
    "cycle": "...",
    "dailyTime": "...",
    "levelAnalysis": "...",
    "finalGoal": "...",
    "focusPoints": ["..."],
    "riskPoints": ["..."],
    "suggestions": ["..."],
    "learningStrategy": "..."
  }},
  "stages": [
    {{
      "stageName": "阶段 1：...",
      "duration": "...",
      "stageGoal": "...",
      "knowledgePoints": ["..."],
      "learningTasks": ["..."],
      "resourceTasks": ["..."],
      "practiceTasks": ["..."],
      "checkTasks": ["..."],
      "completionCriteria": ["..."]
    }}
  ]
}}

用户输入：
学习目标：{learning_goal}
课程名称：{course_name}
学习周期：{learning_cycle}
每日学习时间：{daily_time}
当前基础：{current_level}
最终目标：{final_goal}

本地 knowledge_points 知识库上下文：
{kb_context}"#,
        learning_goal = clean_or(&input.learning_goal, "未填写"),
        course_name = clean_or(&input.course_name, "机械制造工艺学"),
        learning_cycle = clean_or(&input.learning_cycle, "未填写"),
        daily_time = clean_or(&input.daily_time, "未填写"),
        current_level = clean_or(&input.current_level, "未填写"),
        final_goal = clean_or(&input.final_goal, "未填写"),
    )
}

fn build_goal_specific_plan_prompt(
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    kb_context: &PlanKnowledgeContext,
) -> String {
    let kb_text = kb_context
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("当前没有匹配到本地知识库候选词条；如必须补齐词条，sourceType 使用 modelFallback，sourceFile 留空。");
    let candidate_json = serde_json::to_string(
        &kb_context
            .candidates
            .iter()
            .take(50)
            .map(|item| {
                json!({
                    "title": item.title.clone(),
                    "section": item.section.clone(),
                    "content": item.content.clone(),
                    "sourceFile": item.source_file.clone(),
                    "score": item.score,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());
    let profile_json = json!({
        "goalType": format!("{:?}", profile.goal_type),
        "cycle": profile.cycle,
        "recommendedStageCount": profile.recommended_stage_count,
        "coverageMode": profile.coverage_mode,
        "depthMode": profile.depth_mode,
        "preferredMasteryLevels": profile.preferred_mastery_levels,
        "taskFocus": profile.task_focus,
        "entryCountRange": {
            "min": profile.entry_count_range.0,
            "max": profile.entry_count_range.1,
        },
        "assessmentMode": profile.assessment_mode,
        "promptInstruction": profile.prompt_instruction,
        "stageTemplates": profile.stage_templates,
    });

    let mut prompt = format!(
        r#"你是 Pomegranate 石榴软件的 AI 助学计划生成器。必须生成“机械制造工艺学”的结构化学习计划。

硬性要求：
1. 四类 learningGoal 必须生成明显不同的阶段、词条、掌握程度、学习动作和检查方式。
2. 不得只输出章节名或笼统任务，必须输出具体知识词条。
3. learningEntries 优先来自本地知识库候选词条；使用候选词条时 sourceType=knowledgeBase，sourceFile 必须等于候选中的 sourceFile。
4. 如果候选不足，可以补 modelFallback 词条，但 sourceType=modelFallback 且不要伪造 sourceFile。
5. 每个阶段至少 1 个 learningEntry；每个 learningEntry 必须有 masteryLevel、studyAction、practiceAction、checkMethod、expectedOutput、estimatedMinutes、reason。
6. masteryLevel 只能使用：了解、理解、掌握、熟练应用。
7. estimatedMinutes 建议 10-35 分钟；不要明显超过阶段可用时间。
8. 不得编造外部 URL、视频、论文或资源链接。
9. 只输出 JSON，不要 Markdown。

输出 JSON 结构：
{{
  "source": "spark",
  "understanding": {{
    "summary": "...",
    "goalType": "...",
    "courseName": "...",
    "cycle": "...",
    "dailyTime": "...",
    "levelAnalysis": "...",
    "finalGoal": "...",
    "focusPoints": ["..."],
    "riskPoints": ["..."],
    "suggestions": ["..."],
    "learningStrategy": "..."
  }},
  "stages": [
    {{
      "stageName": "...",
      "duration": "...",
      "stageGoal": "...",
      "knowledgePoints": ["具体词条"],
      "learningEntries": [
        {{
          "entryId": "s1e1",
          "title": "具体知识词条",
          "section": "所属章节",
          "entryType": "概念/原理/规则/方法/计算/设计/综合应用",
          "masteryLevel": "了解/理解/掌握/熟练应用",
          "studyAction": "具体怎么学",
          "practiceAction": "具体怎么练",
          "checkMethod": "如何检查是否掌握",
          "expectedOutput": "学习后形成什么成果",
          "estimatedMinutes": 20,
          "reason": "为什么安排该词条",
          "sourceFile": "真实候选来源文件，可选",
          "sourceType": "knowledgeBase",
          "prerequisite": ["前置知识，可选"],
          "weakReason": "查漏补缺模式使用，可选",
          "retryTask": "查漏补缺模式使用，可选"
        }}
      ],
      "learningTasks": ["..."],
      "resourceTasks": ["..."],
      "practiceTasks": ["..."],
      "checkTasks": ["..."],
      "completionCriteria": ["..."]
    }}
  ]
}}

用户输入：
courseName={course_name}
learningGoal={learning_goal}
learningCycle={learning_cycle}
dailyTime={daily_time}
currentLevel={current_level}
finalGoal={final_goal}

goalPlanProfile={profile_json}

本地知识库候选词条 JSON={candidate_json}

本地知识库文本上下文：
{kb_text}

掌握程度规则：
- 了解：能说出基本含义，知道作用和适用范围。
- 理解：能解释原理，区分相近概念，说明原因。
- 掌握：能独立完成判断、计算或常规题目。
- 熟练应用：能解决综合题、比较方案并处理工程问题。

currentLevel 约束：
- 零基础：增加基础概念和前置知识，任务更小，少安排综合题。
- 基础较弱：增加概念解释和基础例题，重点处理易混内容。
- 基础一般：减少过多基础介绍，加入典型题和应用任务。
- 基础较好：增加方案比较、综合题和工程应用。

finalGoal 约束：
- 通过考试/70分以上：优先基础概念、常规题和主要中等题。
- 80分以上：增加计算题、分析题和易错点。
- 90分以上/综合题/解决综合问题：增加跨章节关联、综合题、方案比较和高要求检查。

时间约束：dailyTime={daily_time}，请控制每阶段 learningEntries 数量和 estimatedMinutes 总量。
"#,
        course_name = clean_or(&input.course_name, "机械制造工艺学"),
        learning_goal = clean_or(&input.learning_goal, "系统学习"),
        learning_cycle = clean_or(&input.learning_cycle, profile.cycle),
        daily_time = clean_or(&input.daily_time, "每天1小时"),
        current_level = clean_or(&input.current_level, "基础一般"),
        final_goal = clean_or(&input.final_goal, "掌握课程核心知识"),
        profile_json = profile_json,
        candidate_json = candidate_json,
        kb_text = kb_text,
    );
    append_plugin_prompt_context(&mut prompt, input);
    prompt
}

fn parse_ai_goal_understanding(content: &str) -> Result<AiGoalUnderstanding, AppError> {
    let json_text = extract_json_object(content)
        .ok_or_else(|| AppError::Custom("学习目标 AI 返回内容不是 JSON".to_string()))?;
    let parsed = serde_json::from_str::<AiGoalUnderstanding>(json_text)
        .map_err(|e| AppError::Custom(format!("学习目标 AI JSON 字段解析失败: {e}")))?;

    let has_usable_signal = parsed
        .summary
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty())
        || parsed
            .learning_strategy
            .as_deref()
            .is_some_and(|v| !v.trim().is_empty())
        || !value_to_list(parsed.knowledge_scope.as_ref()).is_empty()
        || !value_to_list(parsed.expected_outputs.as_ref()).is_empty();

    if !has_usable_signal {
        return Err(AppError::Custom(
            "学习目标 AI JSON 缺少核心字段".to_string(),
        ));
    }

    Ok(parsed)
}

fn parse_ai_plan(content: &str) -> Result<AiPlanResponse, AppError> {
    let json_text = extract_json_object(content)
        .ok_or_else(|| AppError::Custom("讯飞星火返回的计划内容不是 JSON".to_string()))?;
    let parsed = serde_json::from_str::<AiPlanResponse>(json_text)
        .map_err(|e| AppError::Custom(format!("讯飞星火计划 JSON 字段解析失败: {e}")))?;
    if parsed.stages.is_empty() {
        return Err(AppError::Custom(
            "讯飞星火计划 JSON 缺少 stages".to_string(),
        ));
    }
    Ok(parsed)
}

fn map_ai_understanding(
    parsed: AiGoalUnderstanding,
    input: &LearningAssistantPlanInput,
) -> LearningAssistantUnderstanding {
    let course = clean_or(
        parsed.course.as_deref().unwrap_or(""),
        &clean_or(&input.course_name, "当前课程"),
    );
    let goal_type = clean_or(parsed.goal_type.as_deref().unwrap_or(""), "能力提升目标");
    let days = parsed
        .days
        .as_ref()
        .or(parsed.cycle.as_ref())
        .map(value_to_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| clean_or(&input.learning_cycle, "一个学习周期"));
    let daily_time = clean_or(
        parsed.daily_time.as_deref().unwrap_or(""),
        &clean_or(&input.daily_time, "每天固定时间"),
    );
    let level = clean_or(
        parsed.level.as_deref().unwrap_or(""),
        &clean_or(&input.current_level, "基础待评估"),
    );
    let final_goal = clean_or(
        parsed.final_goal.as_deref().unwrap_or(""),
        &clean_or(&input.final_goal, "形成可检验的学习成果"),
    );
    let expected_outputs = value_to_list(parsed.expected_outputs.as_ref());
    let knowledge_scope = value_to_list(parsed.knowledge_scope.as_ref());
    let risk_points = value_to_list(parsed.risk_points.as_ref());
    let suggestions = value_to_list(parsed.suggestions.as_ref());
    let strategy = clean_or(
        parsed.learning_strategy.as_deref().unwrap_or(""),
        "建议按目标拆解、阶段学习、练习验证和复盘调整推进。",
    );
    let stage_count = parsed
        .stage_count_suggestion
        .as_ref()
        .and_then(value_to_usize)
        .unwrap_or(4)
        .clamp(3, 5);
    let summary = clean_or(
        parsed.summary.as_deref().unwrap_or(""),
        &format!("围绕「{course}」在{days}内完成{goal_type}，每天投入{daily_time}。"),
    );

    let expected_text = if expected_outputs.is_empty() {
        final_goal.clone()
    } else {
        expected_outputs.join("、")
    };
    let scope_text = if knowledge_scope.is_empty() {
        "结合课程大纲和目标要求补齐核心知识点".to_string()
    } else {
        knowledge_scope.join("、")
    };

    LearningAssistantUnderstanding {
        summary,
        current_gap: format!(
            "当前基础：{level}。目标类型：{goal_type}。最终成果要求：{expected_text}。需要覆盖：{scope_text}。"
        ),
        strategy,
        closed_loop: format!("{CLOSED_LOOP_TEXT}。建议拆分为 {stage_count} 个阶段推进。"),
        goal_type: Some(goal_type),
        course_name: Some(course),
        cycle: Some(days),
        daily_time: Some(daily_time),
        level_analysis: Some(level),
        final_goal: Some(final_goal),
        focus_points: (!knowledge_scope.is_empty()).then_some(knowledge_scope),
        risk_points: (!risk_points.is_empty()).then_some(risk_points),
        suggestions: (!suggestions.is_empty()).then_some(suggestions),
        source: Some(parsed.source.unwrap_or_else(|| "spark".to_string())),
    }
}

fn map_ai_stage(index: usize, stage: AiPlanStage) -> LearningAssistantStage {
    let knowledge_points = value_to_list(stage.knowledge_points.as_ref());

    LearningAssistantStage {
        name: clean_or(
            stage.stage_name.as_deref().unwrap_or(""),
            &format!("阶段 {}：学习推进", index + 1),
        ),
        time_range: clean_or(
            stage.duration.as_deref().unwrap_or(""),
            &format!("第 {} 阶段", index + 1),
        ),
        goal: clean_or(
            stage.stage_goal.as_deref().unwrap_or(""),
            "完成本阶段知识学习、练习验证和成果检查。",
        ),
        knowledge_points: (!knowledge_points.is_empty()).then_some(knowledge_points),
        learning_entries: stage.learning_entries,
        learning_tasks: non_empty_or(
            value_to_list(stage.learning_tasks.as_ref()),
            vec!["学习本阶段对应教材章节和课堂笔记".to_string()],
        ),
        resource_tasks: non_empty_or(
            value_to_list(stage.resource_tasks.as_ref()),
            vec![
                "阅读本地 knowledge_points 知识点资料".to_string(),
                "结合教材例题和课堂笔记复习".to_string(),
            ],
        ),
        practice_tasks: non_empty_or(
            value_to_list(stage.practice_tasks.as_ref()),
            vec!["完成本阶段知识点对应例题和课后练习".to_string()],
        ),
        check_tasks: non_empty_or(
            value_to_list(stage.check_tasks.as_ref()),
            vec!["对照阶段目标进行自测和错题复盘".to_string()],
        ),
        completion_criteria: non_empty_or(
            value_to_list(stage.completion_criteria.as_ref()),
            vec!["能够独立说明本阶段核心知识点并完成对应练习".to_string()],
        ),
    }
}

fn extract_json_object(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    (start < end).then_some(&trimmed[start..=end])
}

fn build_chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn value_to_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .flat_map(|item| match item {
                Value::String(text) => text
                    .split(['、', ',', '，', ';', '；'])
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                other => vec![value_to_text(other)],
            })
            .filter(|item| !item.is_empty())
            .collect(),
        Some(Value::String(text)) => text
            .split(['、', ',', '，', ';', '；'])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect(),
        Some(other) => {
            let text = value_to_text(other);
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text]
            }
        }
        None => Vec::new(),
    }
}

fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Array(items) => items
            .iter()
            .map(value_to_text)
            .collect::<Vec<_>>()
            .join("、"),
        Value::Object(_) | Value::Null => String::new(),
    }
}

fn value_to_usize(value: &Value) -> Option<usize> {
    match value {
        Value::Number(number) => number.as_u64().map(|value| value as usize),
        Value::String(text) => text.trim().parse::<usize>().ok(),
        _ => None,
    }
}

fn read_persisted_learning_ai_config(
    db: &Database,
) -> Result<Option<PersistedLearningAiConfig>, AppError> {
    let Some(value) = db.get_config(LEARNING_AI_CONFIG_KEY)? else {
        return Ok(None);
    };
    match serde_json::from_str::<PersistedLearningAiConfig>(&value) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(error) => {
            log::warn!("[learning_assistant] ignored invalid persisted model API config: {error}");
            Ok(None)
        }
    }
}

fn resolve_learning_ai_config(db: &Database) -> Result<LearningAiConfig, AppError> {
    if let Some(config) = read_persisted_learning_ai_config(db)? {
        let credential_id = config
            .credential_id
            .clone()
            .unwrap_or_else(|| LEARNING_AI_CREDENTIAL_ID.to_string());
        if let Some(legacy_key) = config
            .api_key
            .as_ref()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
        {
            CredentialService::upsert_api_key(
                db,
                db.data_dir(),
                &credential_id,
                LEARNING_AI_CREDENTIAL_PROVIDER,
                LEARNING_AI_CREDENTIAL_LABEL,
                &legacy_key,
            )?;
            let migrated = PersistedLearningAiConfig {
                api_base: config.api_base.clone(),
                credential_id: Some(credential_id.clone()),
                api_key: None,
                model: config.model.clone(),
            };
            let value = serde_json::to_string(&migrated).map_err(|e| {
                AppError::Custom(format!("serialize migrated learning ai config failed: {e}"))
            })?;
            db.set_config(LEARNING_AI_CONFIG_KEY, &value)?;
        }

        let mut api_key = CredentialService::load_api_key(db, db.data_dir(), &credential_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        if api_key.is_empty() {
            api_key = env_trim("SPARK_API_PASSWORD").unwrap_or_default();
        }
        if !api_key.is_empty() {
            let resolved = LearningAiConfig {
                api_base: clean_or(&config.api_base, DEFAULT_SPARK_API_BASE),
                api_key,
                model: clean_or(&config.model, DEFAULT_SPARK_MODEL),
                source: "user".to_string(),
                credential_id: Some(credential_id),
            };
            let mut guard = runtime_learning_ai_config()
                .write()
                .map_err(|_| AppError::Custom("learning ai config lock poisoned".to_string()))?;
            *guard = Some(resolved.clone());
            return Ok(resolved);
        }
    }

    if let Some(api_key) = env_trim("SPARK_API_PASSWORD") {
        return Ok(LearningAiConfig {
            api_base: env_trim("SPARK_API_BASE")
                .unwrap_or_else(|| DEFAULT_SPARK_API_BASE.to_string()),
            api_key,
            model: env_trim("SPARK_MODEL").unwrap_or_else(|| DEFAULT_SPARK_MODEL.to_string()),
            source: "env".to_string(),
            credential_id: None,
        });
    }

    Err(AppError::Custom(
        "未配置模型 API，将使用本地模板".to_string(),
    ))
}

fn config_status_from_resolved(config: LearningAiConfig) -> LearningAssistantAiConfigStatus {
    LearningAssistantAiConfigStatus {
        api_base: config.api_base,
        model: config.model,
        has_api_key: true,
        source: config.source,
    }
}

fn fallback_config_status() -> LearningAssistantAiConfigStatus {
    LearningAssistantAiConfigStatus {
        api_base: DEFAULT_SPARK_API_BASE.to_string(),
        model: DEFAULT_SPARK_MODEL.to_string(),
        has_api_key: false,
        source: "fallback".to_string(),
    }
}

impl LearningAiConfig {
    fn from_runtime_or_env() -> Result<Self, AppError> {
        if let Some(config) = runtime_learning_ai_config()
            .read()
            .ok()
            .and_then(|guard| guard.clone())
        {
            return Ok(config);
        }

        let api_key = env_trim("SPARK_API_PASSWORD")
            .ok_or_else(|| AppError::Custom("未配置 SPARK_API_PASSWORD".to_string()))?;

        Ok(Self {
            api_base: env_trim("SPARK_API_BASE")
                .unwrap_or_else(|| DEFAULT_SPARK_API_BASE.to_string()),
            api_key,
            model: env_trim("SPARK_MODEL").unwrap_or_else(|| DEFAULT_SPARK_MODEL.to_string()),
            source: "env".to_string(),
            credential_id: None,
        })
    }
}

fn runtime_learning_ai_config() -> &'static RwLock<Option<LearningAiConfig>> {
    RUNTIME_LEARNING_AI_CONFIG.get_or_init(|| RwLock::new(None))
}

#[cfg(test)]
mod bundled_engine_tests {
    use super::*;

    #[test]
    fn bundled_learning_engine_is_resolvable_from_the_app_directory() {
        let checked = LearningAssistantService::check(LearningAssistantCheckInput {
            learning_assistant_root: "../learning-assistant".to_string(),
        })
        .expect("the bundled learning assistant path should resolve");

        assert!(checked.ok, "missing learning assets: {:?}", checked.errors);
        assert!(checked.skill_path.ends_with("SKILL.md"));
        assert!(checked.template_path.ends_with("plan_template.json"));
    }
}

fn build_understanding(input: &LearningAssistantPlanInput) -> LearningAssistantUnderstanding {
    let course = clean_or(&input.course_name, "当前课程");
    let goal = clean_or(&input.learning_goal, "完成本阶段学习目标");
    let cycle = clean_or(&input.learning_cycle, "一个学习周期");
    let daily_time = clean_or(&input.daily_time, "每天固定时间");
    let current_level = clean_or(&input.current_level, "基础待评估");
    let final_goal = clean_or(&input.final_goal, "形成可检验的学习成果");

    LearningAssistantUnderstanding {
        summary: format!(
            "你希望围绕「{course}」在{cycle}内完成「{goal}」，每天投入{daily_time}，最终达到「{final_goal}」。"
        ),
        current_gap: format!(
            "当前基础是「{current_level}」，主要差距通常在知识框架、稳定练习量和阶段性检验标准之间。"
        ),
        strategy: "建议采用“先搭框架、再做专题突破、随后集中练习、最后复盘输出”的节奏，把每个阶段都落到任务、练习和检查标准上。".to_string(),
        closed_loop: CLOSED_LOOP_TEXT.to_string(),
        goal_type: Some(goal),
        course_name: Some(course),
        cycle: Some(cycle),
        daily_time: Some(daily_time),
        level_analysis: Some(current_level),
        final_goal: Some(final_goal),
        focus_points: None,
        risk_points: None,
        suggestions: None,
        source: Some("fallback".to_string()),
    }
}

#[allow(dead_code)]
fn build_stage_plan(input: &LearningAssistantPlanInput) -> Vec<LearningAssistantStage> {
    let course = clean_or(&input.course_name, "课程");
    let daily_time = clean_or(&input.daily_time, "每日学习时间");
    let final_goal = clean_or(&input.final_goal, "最终目标");

    vec![
        LearningAssistantStage {
            name: "阶段 1：目标拆解与知识框架".to_string(),
            time_range: "第 1 阶段，建议占总周期 20%".to_string(),
            goal: format!("建立「{course}」的知识地图，明确从当前基础到目标成果的路径。"),
            knowledge_points: None,
            learning_entries: None,
            learning_tasks: vec![
                "整理课程大纲、考试要求或项目要求".to_string(),
                "列出必须掌握的核心概念和先修知识".to_string(),
                format!("按{daily_time}安排固定学习时段"),
            ],
            resource_tasks: vec![
                "阅读本地 knowledge_points 中的课程基础知识点资料".to_string(),
                "准备主教材、课堂笔记和对应章节例题".to_string(),
            ],
            practice_tasks: vec![
                "完成基础概念自测".to_string(),
                "用自己的话输出一页知识框架".to_string(),
            ],
            check_tasks: vec![
                "检查是否能解释核心概念之间的关系".to_string(),
                "标记仍不理解的薄弱点".to_string(),
            ],
            completion_criteria: vec![
                "形成清晰的学习清单".to_string(),
                "能说清本课程的重点、难点和评价方式".to_string(),
            ],
        },
        LearningAssistantStage {
            name: "阶段 2：核心知识学习".to_string(),
            time_range: "第 2 阶段，建议占总周期 30%".to_string(),
            goal: "完成核心知识输入，建立可复述、可迁移的理解。".to_string(),
            knowledge_points: None,
            learning_entries: None,
            learning_tasks: vec![
                "按模块学习核心章节".to_string(),
                "每个模块输出 5-10 条关键结论".to_string(),
                "把不懂的问题记录到待解决列表".to_string(),
            ],
            resource_tasks: vec![
                "围绕难点阅读本地知识点资料和教材章节".to_string(),
                "结合课堂笔记、教材例题保持同一知识体系".to_string(),
            ],
            practice_tasks: vec![
                "每学完一个模块完成对应练习".to_string(),
                "对错题或卡点做原因归类".to_string(),
            ],
            check_tasks: vec![
                "闭卷复述模块框架".to_string(),
                "用例题或小任务验证理解".to_string(),
            ],
            completion_criteria: vec![
                "核心模块均有笔记和练习记录".to_string(),
                "高频概念可以独立解释并举例".to_string(),
            ],
        },
        LearningAssistantStage {
            name: "阶段 3：专题练习与能力巩固".to_string(),
            time_range: "第 3 阶段，建议占总周期 30%".to_string(),
            goal: "通过集中练习把知识转化为稳定解题或应用能力。".to_string(),
            knowledge_points: None,
            learning_entries: None,
            learning_tasks: vec![
                "按薄弱专题安排专项突破".to_string(),
                "整理高频错误和常见陷阱".to_string(),
            ],
            resource_tasks: vec![
                "选择教材例题、课后练习和本地知识点资料".to_string(),
                "为每个薄弱点匹配对应章节知识点和例题".to_string(),
            ],
            practice_tasks: vec![
                "完成专题练习并记录正确率".to_string(),
                "对难题进行二次讲解或重做".to_string(),
            ],
            check_tasks: vec![
                "做一次阶段测验".to_string(),
                "检查薄弱点是否明显减少".to_string(),
            ],
            completion_criteria: vec![
                "主要专题能在限定时间内完成".to_string(),
                "错题原因能归类并给出修正方法".to_string(),
            ],
        },
        LearningAssistantStage {
            name: "阶段 4：综合输出与目标验收".to_string(),
            time_range: "最后阶段，建议占总周期 20%".to_string(),
            goal: format!("围绕「{final_goal}」完成最终验收，形成可展示或可评分成果。"),
            knowledge_points: None,
            learning_entries: None,
            learning_tasks: vec![
                "回顾全周期知识结构".to_string(),
                "补齐最后的薄弱模块".to_string(),
            ],
            resource_tasks: vec![
                "整理最终复习清单、教材章节和本地知识点资料".to_string(),
                "保留后续需要二次复习的薄弱知识点清单".to_string(),
            ],
            practice_tasks: vec![
                "完成综合模拟、项目作品或总结报告".to_string(),
                "按真实标准进行限时演练".to_string(),
            ],
            check_tasks: vec![
                "对照最终目标逐项验收".to_string(),
                "记录未达标项并生成下一轮调整建议".to_string(),
            ],
            completion_criteria: vec![
                "能够独立完成综合任务".to_string(),
                "输出结果达到预设目标或明确下一轮补强方向".to_string(),
            ],
        },
    ]
}

fn calculate_local_allocation(
    db: &Database,
    data_dir: &std::path::Path,
    input: &LearningAssistantPlanInput,
) -> Result<LocalLearningPlanAllocation, AppError> {
    let selected = if input.selected_learning_sources.is_empty() {
        input
            .selected_document_source_ids
            .iter()
            .map(|id| SelectedLearningSource {
                document_source_id: *id,
                importance_level: SourceImportanceLevel::Normal,
            })
            .collect()
    } else {
        input.selected_learning_sources.clone()
    };
    let selected_ids = selected
        .iter()
        .map(|item| item.document_source_id)
        .collect::<Vec<_>>();
    let sources = crate::services::document_tree::DocumentTreeService::selectable_sources(
        db,
        data_dir,
        &selected_ids,
    )?;
    let daily = if input.daily_study_hours > 0.0 {
        input.daily_study_hours
    } else {
        parse_legacy_daily_hours(&input.daily_time)
            .ok_or_else(|| AppError::InvalidInput("每日学习时间必须使用数值小时。".into()))?
    };
    let goal = LearningGoal::parse(&input.learning_goal).map_err(AppError::InvalidInput)?;
    LocalLearningPlanService::calculate(
        LocalLearningPlanInput {
            learning_goal: goal,
            daily_study_hours: daily,
            selected_learning_sources: selected,
        },
        sources,
    )
    .map_err(AppError::InvalidInput)
}

fn parse_legacy_daily_hours(value: &str) -> Option<f64> {
    let compact = value.replace(' ', "");
    let hour = compact
        .split("小时")
        .next()
        .and_then(|prefix| {
            prefix
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
                .parse::<f64>()
                .ok()
        })
        .unwrap_or(0.0);
    let minutes = compact
        .split("小时")
        .nth(1)
        .unwrap_or(&compact)
        .split("分钟")
        .next()
        .and_then(|prefix| {
            prefix
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
                .parse::<f64>()
                .ok()
        })
        .unwrap_or(0.0);
    let result = hour + minutes / 60.0;
    (result > 0.0).then_some(result)
}

fn selected_document_source_ids(input: &LearningAssistantPlanInput) -> Vec<i64> {
    if input.selected_learning_sources.is_empty() {
        input.selected_document_source_ids.clone()
    } else {
        input
            .selected_learning_sources
            .iter()
            .map(|item| item.document_source_id)
            .collect()
    }
}

fn apply_local_stage_allocation(
    mut stages: Vec<LearningAssistantStage>,
    allocation: &LocalLearningPlanAllocation,
) -> Vec<LearningAssistantStage> {
    for (stage, local) in stages.iter_mut().zip(&allocation.stage_allocations) {
        stage.time_range = format!(
            "{}小时（占本次计划{}%）",
            local.allocated_hours,
            (local.ratio * 100.0).round()
        );
    }
    stages
}

fn build_plan_knowledge_context(
    db: &Database,
    data_dir: &std::path::Path,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
) -> PlanKnowledgeContext {
    let query = [
        input.learning_goal.as_str(),
        input.learning_cycle.as_str(),
        input.daily_time.as_str(),
        input.current_level.as_str(),
        input.final_goal.as_str(),
        profile.coverage_mode,
        profile.depth_mode,
        &profile.task_focus.join(" "),
        "机械制造工艺学",
    ]
    .join(" ");

    let search_input = LearningKbSearchInput {
        course: clean_or(&input.course_name, "机械制造工艺学"),
        query,
        stage_name: "生成学习计划".to_string(),
        stage_index: Some(0),
        stage_goal: clean_or(&input.final_goal, "生成阶段学习计划"),
        learning_tasks: Vec::new(),
        resource_tasks: Vec::new(),
        practice_tasks: Vec::new(),
        check_tasks: Vec::new(),
        knowledge_points: vec![
            clean_or(&input.learning_goal, "学习目标"),
            clean_or(&input.current_level, "当前基础"),
            clean_or(&input.final_goal, "最终目标"),
        ],
        top_k: 50,
        document_source_ids: selected_document_source_ids(input),
        selected_learning_sources: input.selected_learning_sources.clone(),
    };

    match LearningKbService::search(db, data_dir, search_input) {
        Ok(result) => {
            if result.results.is_empty() {
                return PlanKnowledgeContext {
                    text: Some(result.message),
                    candidates: Vec::new(),
                };
            }

            let selected_items = select_context_items(&result.results, 30_000, 6_000);
            let candidates = selected_items
                .iter()
                .map(|item| LearningEntryCandidate {
                    title: item.title.clone(),
                    section: item.section.clone(),
                    content: item.content.clone(),
                    source_file: item.source_file.clone(),
                    score: item.score,
                })
                .collect::<Vec<_>>();
            let text = selected_items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "{}. documentId={} sourceFile={} sourceFolder={} sourceType={} fileType={} weight={} pageOrSheet={} chunkIndex={} section={} title={} score={} content={}",
                        index + 1,
                        item.document_id,
                        item.source_file,
                        item.source_folder,
                        item.source_type,
                        item.file_type,
                        item.weight,
                        item.sheet_name,
                        item.chunk_index,
                        item.section,
                        item.title,
                        item.score,
                        item.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            PlanKnowledgeContext {
                text: Some(text),
                candidates,
            }
        }
        Err(error) => {
            log::warn!("[learning_assistant] plan knowledge context search failed: {error}");
            PlanKnowledgeContext {
                text: Some(format!(
                    "本地知识库检索失败：{error}。如果使用 fallback 词条，sourceType 必须标记为 modelFallback。"
                )),
                candidates: Vec::new(),
            }
        }
    }
}

fn select_context_items<'a>(
    items: &'a [LearningKbResultItem],
    total_char_limit: usize,
    per_document_char_limit: usize,
) -> Vec<&'a LearningKbResultItem> {
    let mut selected = Vec::new();
    let mut total_chars = 0usize;
    let mut per_document = HashMap::<i64, usize>::new();
    let mut seen = HashSet::<(i64, usize)>::new();
    for item in items {
        if !seen.insert((item.document_id, item.chunk_index)) {
            continue;
        }
        let content_chars = item.content.chars().count();
        let used_for_document = per_document.get(&item.document_id).copied().unwrap_or(0);
        if used_for_document + content_chars > per_document_char_limit {
            continue;
        }
        if total_chars + content_chars > total_char_limit {
            break;
        }
        per_document.insert(item.document_id, used_for_document + content_chars);
        total_chars += content_chars;
        selected.push(item);
    }
    selected
}

#[allow(dead_code)]
fn build_plan_kb_context(
    db: &Database,
    data_dir: &std::path::Path,
    input: &LearningAssistantPlanInput,
) -> Option<String> {
    let query = [
        input.learning_goal.as_str(),
        input.learning_cycle.as_str(),
        input.daily_time.as_str(),
        input.current_level.as_str(),
        input.final_goal.as_str(),
        "机械制造工艺学",
    ]
    .join(" ");

    let search_input = LearningKbSearchInput {
        course: clean_or(&input.course_name, "机械制造工艺学"),
        query,
        stage_name: "生成学习计划".to_string(),
        stage_index: Some(0),
        stage_goal: clean_or(&input.final_goal, "生成阶段学习计划"),
        learning_tasks: Vec::new(),
        resource_tasks: Vec::new(),
        practice_tasks: Vec::new(),
        check_tasks: Vec::new(),
        knowledge_points: vec![
            clean_or(&input.learning_goal, "学习目标"),
            clean_or(&input.current_level, "当前基础"),
            clean_or(&input.final_goal, "最终目标"),
        ],
        top_k: 10,
        document_source_ids: selected_document_source_ids(input),
        selected_learning_sources: input.selected_learning_sources.clone(),
    };

    match LearningKbService::search(db, data_dir, search_input) {
        Ok(result) => {
            if result.results.is_empty() {
                return Some(result.message);
            }

            let context = result
                .results
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "{}. 来源：{} / {}；章节：{}；知识点：{}；内容：{}",
                        index + 1,
                        item.source_file,
                        item.sheet_name,
                        item.section,
                        item.title,
                        item.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some(context)
        }
        Err(error) => {
            log::warn!("[learning_assistant] 计划生成知识库上下文检索失败：{error}");
            Some(format!("本地知识库检索失败：{error}。请不要编造资源链接。"))
        }
    }
}

fn non_empty_or(values: Vec<String>, fallback: Vec<String>) -> Vec<String> {
    if values.is_empty() {
        fallback
    } else {
        values
    }
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
