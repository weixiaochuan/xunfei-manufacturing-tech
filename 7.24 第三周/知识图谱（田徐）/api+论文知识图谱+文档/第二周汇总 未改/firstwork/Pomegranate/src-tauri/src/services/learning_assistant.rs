use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::database::Database;
use crate::error::AppError;
use crate::services::http_client;
use crate::services::learning_kb::{LearningKbSearchInput, LearningKbService};

const LEARNING_SKILL_MD: &str = "skills/learning-assistant/SKILL.md";
const GENERATE_PLAN_WORKFLOW: &str =
    "skills/learning-assistant/workflows/generate-learning-plan.md";
const PLANNING_RULES: &str = "skills/learning-assistant/references/planning-rules.md";
const SCORING_RULES: &str = "skills/learning-assistant/references/scoring-rules.md";
const PLAN_TEMPLATE_JSON: &str = "templates/plan_template.json";
const DEFAULT_SPARK_API_BASE: &str = "https://spark-api-open.xf-yun.com/v1";
const DEFAULT_SPARK_MODEL: &str = "4.0Ultra";
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
    pub current_level: String,
    pub final_goal: String,
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
    pub learning_tasks: Vec<String>,
    pub resource_tasks: Vec<String>,
    pub practice_tasks: Vec<String>,
    pub check_tasks: Vec<String>,
    pub completion_criteria: Vec<String>,
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
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct LearningAiConfig {
    api_base: String,
    api_key: String,
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
}

pub struct LearningAssistantService;

impl LearningAssistantService {
    pub fn get_ai_config() -> Result<LearningAssistantAiConfigStatus, AppError> {
        let runtime_config = {
            let guard = runtime_learning_ai_config()
                .read()
                .map_err(|_| AppError::Custom("learning ai config lock poisoned".to_string()))?;
            guard.clone()
        };

        if let Some(config) = runtime_config {
            return Ok(LearningAssistantAiConfigStatus {
                api_base: config.api_base,
                model: config.model,
                has_api_key: true,
                source: "runtime".to_string(),
            });
        }

        let api_base =
            env_trim("SPARK_API_BASE").unwrap_or_else(|| DEFAULT_SPARK_API_BASE.to_string());
        let model = env_trim("SPARK_MODEL").unwrap_or_else(|| DEFAULT_SPARK_MODEL.to_string());
        let has_api_key = env_trim("SPARK_API_PASSWORD").is_some();

        Ok(LearningAssistantAiConfigStatus {
            api_base,
            model,
            has_api_key,
            source: if has_api_key { "env" } else { "notConfigured" }.to_string(),
        })
    }

    pub fn save_ai_config(
        input: LearningAssistantAiConfigInput,
    ) -> Result<LearningAssistantAiConfigStatus, AppError> {
        let api_base = clean_or(&input.api_base, DEFAULT_SPARK_API_BASE);
        let model = clean_or(&input.model, DEFAULT_SPARK_MODEL);
        let api_key = input.api_key.trim().to_string();

        if api_key.is_empty() {
            return Err(AppError::InvalidInput(
                "API Key cannot be empty".to_string(),
            ));
        }

        let status = LearningAssistantAiConfigStatus {
            api_base: api_base.clone(),
            model: model.clone(),
            has_api_key: true,
            source: "runtime".to_string(),
        };

        {
            let mut guard = runtime_learning_ai_config()
                .write()
                .map_err(|_| AppError::Custom("learning ai config lock poisoned".to_string()))?;
            *guard = Some(LearningAiConfig {
                api_base,
                api_key,
                model,
            });
        }

        Ok(status)
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
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, AppError> {
        let root = validate_engine(&input.learning_assistant_root)?;
        let (understanding, message) = match build_ai_understanding(&input).await {
            Ok(understanding) => (
                understanding,
                Some("已使用讯飞星火完成学习目标解析".to_string()),
            ),
            Err(error) => {
                let fallback_message = format!("讯飞星火目标解析使用模板 fallback：{error}");
                log::warn!("[learning_assistant] {fallback_message}");
                (build_understanding(&input), Some(fallback_message))
            }
        };

        Ok(LearningAssistantPlanResult {
            success: true,
            engine_root: root.to_string_lossy().to_string(),
            skill_path: root.join(LEARNING_SKILL_MD).to_string_lossy().to_string(),
            template_path: root.join(PLAN_TEMPLATE_JSON).to_string_lossy().to_string(),
            understanding,
            stages: Vec::new(),
            message,
            error: None,
        })
    }

    pub async fn generate_plan(
        db: &Database,
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, AppError> {
        let root = validate_engine(&input.learning_assistant_root)?;
        let kb_context = build_plan_kb_context(db, &input);
        let (understanding, stages, message) =
            match build_ai_plan(&input, kb_context.as_deref()).await {
                Ok((understanding, stages)) => (
                    understanding,
                    stages,
                    Some("已使用讯飞星火生成学习计划".to_string()),
                ),
                Err(error) => {
                    let fallback_message = format!("讯飞星火计划生成使用模板 fallback：{error}");
                    log::warn!("[learning_assistant] {fallback_message}");
                    (
                        build_understanding(&input),
                        build_stage_plan(&input),
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
            stages,
            message,
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
    };

    match LearningKbService::search(db, search_input) {
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

async fn build_ai_understanding(
    input: &LearningAssistantPlanInput,
) -> Result<LearningAssistantUnderstanding, AppError> {
    let config = LearningAiConfig::from_runtime_or_env()?;
    let prompt = build_goal_understanding_prompt(input);
    let content = call_learning_ai(&config, &prompt).await?;
    let parsed = parse_ai_goal_understanding(&content)?;
    Ok(map_ai_understanding(parsed, input))
}

async fn build_ai_plan(
    input: &LearningAssistantPlanInput,
    kb_context: Option<&str>,
) -> Result<(LearningAssistantUnderstanding, Vec<LearningAssistantStage>), AppError> {
    let config = LearningAiConfig::from_runtime_or_env()?;
    let prompt = build_plan_prompt(input, kb_context);
    let content = call_learning_ai(&config, &prompt).await?;
    let parsed = parse_ai_plan(&content)?;
    let mut understanding = parsed
        .understanding
        .map(|understanding| map_ai_understanding(understanding, input))
        .unwrap_or_else(|| build_understanding(input));
    understanding.source = Some(parsed.source.unwrap_or_else(|| "spark".to_string()));

    let stages = parsed
        .stages
        .into_iter()
        .enumerate()
        .map(|(index, stage)| map_ai_stage(index, stage))
        .collect::<Vec<_>>();

    if stages.is_empty() {
        return Err(AppError::Custom(
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

fn build_goal_understanding_prompt(input: &LearningAssistantPlanInput) -> String {
    format!(
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
    )
}

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

fn build_plan_kb_context(db: &Database, input: &LearningAssistantPlanInput) -> Option<String> {
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
    };

    match LearningKbService::search(db, search_input) {
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
