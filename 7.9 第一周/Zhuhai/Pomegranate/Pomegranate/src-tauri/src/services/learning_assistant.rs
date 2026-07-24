use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::services::learning_kb::{
    inventory_from_dir, search_in_dir, LearningKbInventory, LearningKbResultItem,
    LearningKbSearchInput, LearningKbService,
};
use crate::services::local_learning_plan::{
    learning_goal_config, LearningGoal, LearningPlanSource, LocalLearningPlanAllocation,
    LocalLearningPlanInput, LocalLearningPlanService, SelectedLearningSource,
    SourceImportanceLevel,
};

const LOCAL_FALLBACK_REASON: &str = "本阶段未启用模型调用，使用本地模板 fallback。";
const CLOSED_LOOP_TEXT: &str =
    "目标解析 -> 计划生成 -> 阶段任务 -> 资源推荐 -> 成果检查 -> 进度记录 -> 计划调整";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantCheckInput {
    #[serde(default)]
    pub learning_assistant_root: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantCheckResult {
    pub ok: bool,
    pub knowledge_points_path: String,
    pub workbook_count: usize,
    pub knowledge_point_count: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningAssistantPlanInput {
    #[serde(default)]
    pub learning_assistant_root: Option<String>,
    #[serde(default)]
    pub learning_goal: String,
    #[serde(default)]
    pub course_name: String,
    #[serde(default)]
    pub learning_cycle: String,
    #[serde(default)]
    pub daily_time: String,
    #[serde(default)]
    pub daily_study_hours: f64,
    #[serde(default)]
    pub current_level: String,
    #[serde(default)]
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
struct GoalPlanProfile {
    goal_type: LearningGoal,
    cycle: &'static str,
    recommended_stage_count: usize,
    plan_strategy: &'static str,
    stage_templates: Vec<&'static str>,
    entry_count_range: (usize, usize),
    task_focus: Vec<&'static str>,
}

pub struct LearningAssistantService;

impl LearningAssistantService {
    pub fn check(
        app: &AppHandle,
        input: LearningAssistantCheckInput,
    ) -> Result<LearningAssistantCheckResult, String> {
        let _compat_root_hint = input.learning_assistant_root.as_deref();
        let inventory = LearningKbService::inventory(app)?;
        Ok(check_result_from_inventory(inventory))
    }

    pub fn understand(
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, String> {
        let understanding = build_understanding(&input);
        Ok(LearningAssistantPlanResult {
            success: true,
            engine_root: "bundled-learning-assistant".to_string(),
            skill_path: String::new(),
            template_path: String::new(),
            understanding,
            stages: Vec::new(),
            plan_strategy: None,
            goal_profile_summary: None,
            local_allocation: None,
            message: Some("本阶段仅启用本地目标理解 fallback。".to_string()),
            fallback_reason: Some(LOCAL_FALLBACK_REASON.to_string()),
            error: None,
        })
    }

    pub fn generate_plan(
        app: &AppHandle,
        input: LearningAssistantPlanInput,
    ) -> Result<LearningAssistantPlanResult, String> {
        let inventory = LearningKbService::inventory(app)?;
        let knowledge_points_dir = Path::new(&inventory.directory);
        generate_plan_from_dir(knowledge_points_dir, input)
    }
}

pub fn generate_plan_from_dir(
    knowledge_points_dir: &Path,
    input: LearningAssistantPlanInput,
) -> Result<LearningAssistantPlanResult, String> {
    let _compat_root_hint = input.learning_assistant_root.as_deref();
    let inventory = inventory_from_dir(knowledge_points_dir)?;
    if !inventory.ok {
        return Ok(LearningAssistantPlanResult {
            success: false,
            engine_root: knowledge_points_dir.to_string_lossy().to_string(),
            skill_path: String::new(),
            template_path: String::new(),
            understanding: build_understanding(&input),
            stages: Vec::new(),
            plan_strategy: None,
            goal_profile_summary: None,
            local_allocation: None,
            message: Some("本地知识点资源检查未通过。".to_string()),
            fallback_reason: Some(LOCAL_FALLBACK_REASON.to_string()),
            error: Some(inventory.warnings.join("；")),
        });
    }

    let profile = build_goal_plan_profile(&input);
    let understanding = build_understanding(&input);
    let local_allocation = calculate_local_allocation(&input, &inventory, profile.goal_type)?;
    let candidates = search_candidates(knowledge_points_dir, &input, &profile)?;
    let stages = build_goal_stage_plan(&input, &profile, &candidates);

    Ok(LearningAssistantPlanResult {
        success: true,
        engine_root: knowledge_points_dir.to_string_lossy().to_string(),
        skill_path: String::new(),
        template_path: String::new(),
        understanding,
        stages,
        plan_strategy: Some(profile.plan_strategy.to_string()),
        goal_profile_summary: Some(format!(
            "goalType={:?}; cycle={}; recommendedStageCount={}; bundledKnowledgePoints={}",
            profile.goal_type,
            profile.cycle,
            profile.recommended_stage_count,
            inventory.knowledge_point_count
        )),
        local_allocation: Some(local_allocation),
        message: Some("当前使用本地模板生成，未调用模型 API。".to_string()),
        fallback_reason: Some(LOCAL_FALLBACK_REASON.to_string()),
        error: None,
    })
}

fn check_result_from_inventory(inventory: LearningKbInventory) -> LearningAssistantCheckResult {
    LearningAssistantCheckResult {
        ok: inventory.ok,
        knowledge_points_path: inventory.directory,
        workbook_count: inventory.workbook_count,
        knowledge_point_count: inventory.knowledge_point_count,
        errors: if inventory.ok {
            Vec::new()
        } else {
            inventory.warnings.clone()
        },
        warnings: inventory.warnings,
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
        source: Some("localFallback".to_string()),
    }
}

fn build_goal_plan_profile(input: &LearningAssistantPlanInput) -> GoalPlanProfile {
    let goal_type =
        LearningGoal::parse(&input.learning_goal).unwrap_or(LearningGoal::SystematicLearning);
    match goal_type {
        LearningGoal::FinalSprint => GoalPlanProfile {
            goal_type,
            cycle: "3天",
            recommended_stage_count: 3,
            plan_strategy: "当前采用短期冲刺策略，优先安排考试高频词条、典型题和易错点。",
            stage_templates: vec![
                "第 1 天：核心概念与基本原则速查",
                "第 2 天：计算方法、设计方法和典型题",
                "第 3 天：易错点、综合题和模拟检查",
            ],
            entry_count_range: (3, 10),
            task_focus: vec!["原则", "方法", "计算", "误差", "定位", "尺寸链", "典型"],
        },
        LearningGoal::GapFilling => GoalPlanProfile {
            goal_type,
            cycle: "2周",
            recommended_stage_count: 4,
            plan_strategy: "当前采用薄弱项诊断策略，优先安排前置知识、易错词条和再次检查。",
            stage_templates: vec![
                "阶段 1：诊断现状并定位薄弱知识点",
                "阶段 2：补齐基础概念和前置知识",
                "阶段 3：纠正常见错误并加强典型题",
                "阶段 4：再次检查并确认薄弱项已补齐",
            ],
            entry_count_range: (3, 8),
            task_focus: vec!["基础", "概念", "基准", "定位", "误差", "前置"],
        },
        LearningGoal::SystematicLearning => GoalPlanProfile {
            goal_type,
            cycle: "3周",
            recommended_stage_count: 5,
            plan_strategy: "当前采用系统递进策略，按章节和知识依赖关系安排词条。",
            stage_templates: vec![
                "阶段 1：课程框架与基础概念",
                "阶段 2：机械加工工艺规程设计",
                "阶段 3：机床夹具设计",
                "阶段 4：加工精度与表面质量控制",
                "阶段 5：装配工艺与课程综合复盘",
            ],
            entry_count_range: (4, 12),
            task_focus: vec!["章节顺序", "前置关系", "结构化笔记", "阶段练习"],
        },
        LearningGoal::ComprehensiveImprovement => GoalPlanProfile {
            goal_type,
            cycle: "4周",
            recommended_stage_count: 5,
            plan_strategy: "当前采用综合应用策略，重点安排跨章节分析、方案比较和综合问题解决。",
            stage_templates: vec![
                "阶段 1：核心知识快速诊断与整合",
                "阶段 2：工艺路线与夹具方案综合分析",
                "阶段 3：精度、表面质量和误差控制",
                "阶段 4：装配工艺与跨章节综合应用",
                "阶段 5：综合案例、方案评价和成果输出",
            ],
            entry_count_range: (3, 10),
            task_focus: vec!["综合", "方案", "比较", "分析", "控制", "应用", "尺寸链"],
        },
    }
}

fn search_candidates(
    knowledge_points_dir: &Path,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
) -> Result<Vec<LearningKbResultItem>, String> {
    let mut candidates = Vec::new();
    for (index, stage_name) in profile.stage_templates.iter().enumerate() {
        let result = search_in_dir(
            knowledge_points_dir,
            LearningKbSearchInput {
                course: input.course_name.clone(),
                query: stage_name.to_string(),
                stage_name: stage_name.to_string(),
                stage_index: Some(index + 1),
                stage_goal: input.final_goal.clone(),
                knowledge_points: profile
                    .task_focus
                    .iter()
                    .map(|item| item.to_string())
                    .collect(),
                top_k: profile.entry_count_range.1,
                ..LearningKbSearchInput::default()
            },
        )?;
        candidates.extend(result.results);
    }
    if candidates.is_empty() {
        let result = search_in_dir(
            knowledge_points_dir,
            LearningKbSearchInput {
                query: input.final_goal.clone(),
                top_k: profile.entry_count_range.1,
                ..LearningKbSearchInput::default()
            },
        )?;
        candidates.extend(result.results);
    }
    Ok(candidates)
}

fn build_goal_stage_plan(
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    candidates: &[LearningKbResultItem],
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
                        None => format!("按本地模板核对“{}”。", entry.title),
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

fn build_stage_entries(
    stage_index: usize,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
    candidates: &[LearningKbResultItem],
) -> Vec<LearningPlanEntry> {
    let target = target_entry_count(input, profile);
    let mut pool = if candidates.is_empty() {
        fallback_candidates(profile)
    } else {
        candidates.to_vec()
    };
    rank_candidates(&mut pool, stage_index, profile);
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
    candidate: LearningKbResultItem,
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
            profile.stage_templates[stage_index % profile.stage_templates.len()],
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
            "localTemplate".to_string()
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

fn fallback_candidates(profile: &GoalPlanProfile) -> Vec<LearningKbResultItem> {
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
        .map(|title| LearningKbResultItem {
            source_file: String::new(),
            sheet_name: String::new(),
            section: "本地模板候选".to_string(),
            title: title.to_string(),
            content: String::new(),
            importance: None,
            importance_weight: None,
            matched_keywords: Vec::new(),
            score: 0.0,
            reason: "本地模板候选词条。".to_string(),
        })
        .collect()
}

fn calculate_local_allocation(
    input: &LearningAssistantPlanInput,
    inventory: &LearningKbInventory,
    goal: LearningGoal,
) -> Result<LocalLearningPlanAllocation, String> {
    let sources = inventory
        .workbooks
        .iter()
        .enumerate()
        .map(|(index, workbook)| LearningPlanSource {
            id: index as i64 + 1,
            display_name: workbook.file_name.clone(),
            category: "内置知识点".to_string(),
            file_extension: "xlsx".to_string(),
            is_enabled: true,
            is_available: true,
        })
        .collect::<Vec<_>>();
    let selected = sources
        .iter()
        .map(|source| SelectedLearningSource {
            source_id: source.id,
            importance_level: SourceImportanceLevel::Normal,
        })
        .collect::<Vec<_>>();
    let daily = if input.daily_study_hours > 0.0 {
        input.daily_study_hours
    } else {
        parse_legacy_daily_hours(&input.daily_time).unwrap_or(1.0)
    };
    LocalLearningPlanService::calculate(
        LocalLearningPlanInput {
            learning_goal: goal,
            daily_study_hours: daily,
            selected_learning_sources: selected,
        },
        sources,
    )
}

fn rank_candidates(
    candidates: &mut [LearningKbResultItem],
    stage_index: usize,
    profile: &GoalPlanProfile,
) {
    candidates.sort_by(|left, right| {
        let left_score = candidate_goal_score(left, stage_index, profile);
        let right_score = candidate_goal_score(right, stage_index, profile);
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn candidate_goal_score(
    candidate: &LearningKbResultItem,
    stage_index: usize,
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

fn stage_goal_text(index: usize, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => [
            "快速锁定考试高频概念和基本原则。",
            "集中处理计算、设计和典型题方法。",
            "通过易错点复盘和模拟检查完成冲刺验收。",
        ][index.min(2)]
        .to_string(),
        LearningGoal::GapFilling => [
            "诊断当前薄弱点并明确补学优先级。",
            "补齐基础概念、前置知识和依赖关系。",
            "围绕薄弱点做专项练习并纠正常见错误。",
            "复测薄弱项，形成下一轮调整清单。",
        ][index.min(3)]
        .to_string(),
        LearningGoal::SystematicLearning => [
            "建立课程整体框架和基础概念地图。",
            "系统学习机械加工工艺规程设计。",
            "理解机床夹具设计的定位与夹紧逻辑。",
            "掌握加工精度、误差和表面质量控制方法。",
            "整合装配工艺并完成课程综合复盘。",
        ][index.min(4)]
        .to_string(),
        LearningGoal::ComprehensiveImprovement => [
            "快速诊断核心知识并建立综合问题清单。",
            "综合分析工艺路线与夹具方案。",
            "深化精度、表面质量和误差控制能力。",
            "处理装配工艺与跨章节综合应用。",
            "完成综合案例、方案评价和成果输出。",
        ][index.min(4)]
        .to_string(),
    }
}

fn mastery_for_stage(stage_index: usize, profile: &GoalPlanProfile) -> &'static str {
    match profile.goal_type {
        LearningGoal::FinalSprint => {
            if stage_index == 0 {
                "掌握"
            } else {
                "熟练应用"
            }
        }
        LearningGoal::GapFilling => {
            if stage_index <= 1 {
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
            if stage_index <= 1 {
                "掌握"
            } else {
                "熟练应用"
            }
        }
    }
}

fn infer_entry_type(title: &str, content: &str) -> &'static str {
    let text = format!("{title} {content}");
    if text.contains("计算") || text.contains("尺寸链") || text.contains("公式") {
        "calculation"
    } else if text.contains("方案") || text.contains("设计") || text.contains("工艺") {
        "application"
    } else if text.contains("误差") || text.contains("质量") || text.contains("控制") {
        "analysis"
    } else {
        "concept"
    }
}

fn study_action_for(title: &str, mastery: &str, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => {
            format!("用 10 分钟速查“{title}”，整理必须记住的判断点和例题入口。")
        }
        LearningGoal::GapFilling => {
            format!("补学“{title}”的前置概念，写出自己不稳的原因和纠正方式。")
        }
        LearningGoal::SystematicLearning => {
            format!("按章节顺序学习“{title}”，形成达到“{mastery}”层级的结构化笔记。")
        }
        LearningGoal::ComprehensiveImprovement => {
            format!("围绕“{title}”做跨章节联系，比较至少两种方案或应用条件。")
        }
    }
}

fn practice_action_for(title: &str, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => format!("完成“{title}”相关的 2 道典型题或判断题。"),
        LearningGoal::GapFilling => format!("针对“{title}”做一次错因复盘和 2 道补弱练习。"),
        LearningGoal::SystematicLearning => {
            format!("用“{title}”解释一个课本例子，并完成对应章节练习。")
        }
        LearningGoal::ComprehensiveImprovement => {
            format!("把“{title}”放入综合案例中，说明其与其他章节知识的关系。")
        }
    }
}

fn check_method_for(title: &str, profile: &GoalPlanProfile) -> String {
    match profile.goal_type {
        LearningGoal::FinalSprint => format!("限时口述“{title}”的核心结论，并记录卡顿点。"),
        LearningGoal::GapFilling => format!("重新解释“{title}”，确认薄弱点是否已经消除。"),
        LearningGoal::SystematicLearning => format!("闭卷画出“{title}”的要点关系图。"),
        LearningGoal::ComprehensiveImprovement => {
            format!("用“{title}”完成一次方案判断或案例说明。")
        }
    }
}

fn reason_for(
    title: &str,
    input: &LearningAssistantPlanInput,
    profile: &GoalPlanProfile,
) -> String {
    format!(
        "“{title}”匹配{}目标，服务于最终目标：{}。",
        profile.goal_type.label(),
        clean_or(&input.final_goal, "形成可检验的学习成果")
    )
}

fn prerequisites_from_text(content: &str) -> Vec<String> {
    let mut output = Vec::new();
    for marker in ["前置", "基础", "依赖", "关联"] {
        if content.contains(marker) {
            output.push(marker.to_string());
        }
    }
    if output.is_empty() {
        output.push("按课程章节顺序复习前置概念".to_string());
    }
    output
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
    let total = hour + minutes / 60.0;
    (total > 0.0).then_some((total * 2.0).round() / 2.0)
}

fn stable_entry_slug(value: &str) -> String {
    let slug = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(ch))
        .take(16)
        .collect::<String>();
    if slug.is_empty() {
        "entry".to_string()
    } else {
        slug
    }
}

fn clean_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::learning_kb::manifest_knowledge_points_dir;

    fn input_for(goal: &str) -> LearningAssistantPlanInput {
        LearningAssistantPlanInput {
            learning_goal: goal.to_string(),
            course_name: "机械制造工艺学".to_string(),
            learning_cycle: String::new(),
            daily_time: "每天1小时".to_string(),
            daily_study_hours: 1.0,
            current_level: "基础一般".to_string(),
            final_goal: "梳理完整课程知识框架".to_string(),
            learning_assistant_root: None,
        }
    }

    #[test]
    fn four_fallback_goals_are_distinct() {
        let dir = manifest_knowledge_points_dir();
        let goals = ["期末冲刺", "查漏补缺", "系统学习", "综合提升"];
        let mut first_stage_names = Vec::new();
        for goal in goals {
            let result = generate_plan_from_dir(&dir, input_for(goal)).unwrap();
            assert!(result.success);
            assert_eq!(
                result.fallback_reason.as_deref(),
                Some(LOCAL_FALLBACK_REASON)
            );
            assert!(!result.stages.is_empty());
            first_stage_names.push(result.stages[0].name.clone());
        }
        first_stage_names.sort();
        first_stage_names.dedup();
        assert_eq!(first_stage_names.len(), 4);
    }

    #[test]
    fn generated_plan_contains_specific_entries() {
        let result =
            generate_plan_from_dir(&manifest_knowledge_points_dir(), input_for("系统学习"))
                .unwrap();
        let entries = result.stages[0].learning_entries.as_ref().unwrap();
        assert!(!entries.is_empty());
        assert!(entries
            .iter()
            .any(|entry| entry.source_type == "knowledgeBase"));
        assert!(result.local_allocation.is_some());
    }

    #[test]
    fn check_reports_complete_inventory() {
        let inventory = inventory_from_dir(&manifest_knowledge_points_dir()).unwrap();
        let result = check_result_from_inventory(inventory);
        assert!(result.ok, "{:?}", result.errors);
        assert_eq!(result.workbook_count, 7);
        assert_eq!(result.knowledge_point_count, 283);
    }
}
