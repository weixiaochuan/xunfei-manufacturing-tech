use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

const LEARNING_SKILL_MD: &str = "skills/learning-assistant/SKILL.md";
const GENERATE_PLAN_WORKFLOW: &str =
    "skills/learning-assistant/workflows/generate-learning-plan.md";
const PLANNING_RULES: &str = "skills/learning-assistant/references/planning-rules.md";
const SCORING_RULES: &str = "skills/learning-assistant/references/scoring-rules.md";
const PLAN_TEMPLATE_JSON: &str = "templates/plan_template.json";

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantUnderstanding {
    pub summary: String,
    pub current_gap: String,
    pub strategy: String,
    pub closed_loop: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantStage {
    pub name: String,
    pub time_range: String,
    pub goal: String,
    pub learning_tasks: Vec<String>,
    pub resource_tasks: Vec<String>,
    pub practice_tasks: Vec<String>,
    pub check_tasks: Vec<String>,
    pub completion_criteria: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantPlanResult {
    pub success: bool,
    pub engine_root: String,
    pub skill_path: String,
    pub template_path: String,
    pub understanding: LearningAssistantUnderstanding,
    pub stages: Vec<LearningAssistantStage>,
    pub error: Option<String>,
}

pub struct LearningAssistantService;

impl LearningAssistantService {
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
            errors.push(format!("Missing plan template: {}", template_path.display()));
        }
        for required in [
            GENERATE_PLAN_WORKFLOW,
            PLANNING_RULES,
            SCORING_RULES,
        ] {
            let required_path = root.join(required);
            if !required_path.is_file() {
                errors.push(format!("Missing skill reference: {}", required_path.display()));
            }
        }

        Ok(LearningAssistantCheckResult {
            ok: errors.is_empty(),
            skill_path: skill_path.to_string_lossy().to_string(),
            template_path: template_path.to_string_lossy().to_string(),
            errors,
        })
    }

    pub fn understand(
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, AppError> {
        let root = validate_engine(&input.learning_assistant_root)?;
        let understanding = build_understanding(&input);

        Ok(LearningAssistantPlanResult {
            success: true,
            engine_root: root.to_string_lossy().to_string(),
            skill_path: root.join(LEARNING_SKILL_MD).to_string_lossy().to_string(),
            template_path: root.join(PLAN_TEMPLATE_JSON).to_string_lossy().to_string(),
            understanding,
            stages: Vec::new(),
            error: None,
        })
    }

    pub fn generate_plan(
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, AppError> {
        let root = validate_engine(&input.learning_assistant_root)?;
        let understanding = build_understanding(&input);
        let stages = build_stage_plan(&input);

        Ok(LearningAssistantPlanResult {
            success: true,
            engine_root: root.to_string_lossy().to_string(),
            skill_path: root.join(LEARNING_SKILL_MD).to_string_lossy().to_string(),
            template_path: root.join(PLAN_TEMPLATE_JSON).to_string_lossy().to_string(),
            understanding,
            stages,
            error: None,
        })
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
    for required in [
        GENERATE_PLAN_WORKFLOW,
        PLANNING_RULES,
        SCORING_RULES,
    ] {
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
        closed_loop: "目标解析 -> 计划生成 -> 阶段任务 -> 资源推荐 -> 成果检查 -> 进度记录 -> 计划调整".to_string(),
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
            learning_tasks: vec![
                "整理课程大纲、考试要求或项目要求".to_string(),
                "列出必须掌握的核心概念和先修知识".to_string(),
                format!("按{daily_time}安排固定学习时段"),
            ],
            resource_tasks: vec![
                "收集 1 套主教材或系统课程".to_string(),
                "准备 1 份权威参考资料和 1 个练习来源".to_string(),
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
            learning_tasks: vec![
                "按模块学习核心章节".to_string(),
                "每个模块输出 5-10 条关键结论".to_string(),
                "把不懂的问题记录到待解决列表".to_string(),
            ],
            resource_tasks: vec![
                "围绕难点补充视频、讲义或案例".to_string(),
                "优先使用同一体系资料，避免频繁换源".to_string(),
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
            learning_tasks: vec![
                "按薄弱专题安排专项突破".to_string(),
                "整理高频错误和常见陷阱".to_string(),
            ],
            resource_tasks: vec![
                "选择分层练习题或项目案例".to_string(),
                "为每个薄弱点匹配一个补救资源".to_string(),
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
            learning_tasks: vec![
                "回顾全周期知识结构".to_string(),
                "补齐最后的薄弱模块".to_string(),
            ],
            resource_tasks: vec![
                "整理最终复习清单或作品参考".to_string(),
                "保留后续进阶资源入口".to_string(),
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

fn clean_or(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}
