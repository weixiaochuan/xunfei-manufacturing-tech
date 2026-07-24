use serde::{Deserialize, Serialize};

pub const COURSE_BASELINE_HOURS: f64 = 64.0;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LearningGoal {
    FinalSprint,
    GapFilling,
    SystematicLearning,
    ComprehensiveImprovement,
}

impl LearningGoal {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "期末冲刺" => Ok(Self::FinalSprint),
            "查漏补缺" => Ok(Self::GapFilling),
            "系统学习" => Ok(Self::SystematicLearning),
            "综合提升" => Ok(Self::ComprehensiveImprovement),
            _ => Err(format!("不支持的学习目标：{value}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::FinalSprint => "期末冲刺",
            Self::GapFilling => "查漏补缺",
            Self::SystematicLearning => "系统学习",
            Self::ComprehensiveImprovement => "综合提升",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StageRatioConfig {
    pub stage_key: &'static str,
    pub stage_name: &'static str,
    pub ratio: f64,
}

#[derive(Debug, Clone)]
pub struct LearningGoalConfig {
    pub total_days: u32,
    pub coverage_ratio: f64,
    pub stage_ratios: Vec<StageRatioConfig>,
}

pub fn learning_goal_config(goal: LearningGoal) -> LearningGoalConfig {
    let (days, coverage, stages) = match goal {
        LearningGoal::FinalSprint => (
            3,
            0.25,
            vec![
                ("examFramework", "高频考点与框架梳理", 0.20),
                ("coreReinforcement", "核心知识强化", 0.25),
                ("examPractice", "真题与专项练习", 0.40),
                ("mockReview", "模拟测试与错题复盘", 0.15),
            ],
        ),
        LearningGoal::GapFilling => (
            14,
            0.50,
            vec![
                ("diagnosis", "基础诊断与薄弱点定位", 0.15),
                ("gapLearning", "薄弱知识补学", 0.40),
                ("correctionPractice", "专项练习与订正", 0.30),
                ("retestReview", "复测与错题回顾", 0.15),
            ],
        ),
        LearningGoal::SystematicLearning => (
            21,
            0.75,
            vec![
                ("framework", "课程框架建立", 0.10),
                ("coreLearning", "核心知识系统学习", 0.50),
                ("chapterPractice", "章节练习与应用", 0.25),
                ("stageReview", "复习与阶段测试", 0.15),
            ],
        ),
        LearningGoal::ComprehensiveImprovement => (
            28,
            1.0,
            vec![
                ("foundationReview", "基础知识回顾", 0.10),
                ("advancedLearning", "重点难点深化", 0.30),
                ("integratedApplication", "综合题与案例应用", 0.40),
                ("assessmentSummary", "综合测评与总结", 0.20),
            ],
        ),
    };
    LearningGoalConfig {
        total_days: days,
        coverage_ratio: coverage,
        stage_ratios: stages
            .into_iter()
            .map(|(stage_key, stage_name, ratio)| StageRatioConfig {
                stage_key,
                stage_name,
                ratio,
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceImportanceLevel {
    Reference,
    Normal,
    Important,
    Core,
}

impl SourceImportanceLevel {
    pub fn weight(self) -> f64 {
        match self {
            Self::Reference => 0.0,
            Self::Normal => 1.0,
            Self::Important => 1.5,
            Self::Core => 2.0,
        }
    }

    pub fn included_in_plan(self) -> bool {
        !matches!(self, Self::Reference)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedLearningSource {
    pub source_id: i64,
    pub importance_level: SourceImportanceLevel,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningPlanSource {
    pub id: i64,
    pub display_name: String,
    pub category: String,
    pub file_extension: String,
    pub is_enabled: bool,
    pub is_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningTimeSummary {
    pub baseline_course_hours: f64,
    pub target_hours: f64,
    pub available_hours: f64,
    pub planned_hours: f64,
    pub missing_hours: f64,
    pub extra_available_hours: f64,
    pub target_coverage_rate: f64,
    pub recommended_daily_hours: f64,
    pub total_days: u32,
    pub daily_study_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageHourAllocation {
    pub stage_key: String,
    pub stage_name: String,
    pub ratio: f64,
    pub allocated_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHourAllocation {
    pub source_id: i64,
    pub display_name: String,
    pub category: String,
    pub importance_level: SourceImportanceLevel,
    pub importance_weight: f64,
    pub allocated_hours: f64,
    pub included_in_plan: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStageAllocation {
    pub stage_key: String,
    pub hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyHourAllocation {
    pub day_index: u32,
    pub planned_hours: f64,
    pub remaining_capacity_hours: f64,
    pub stage_allocations: Vec<DailyStageAllocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageSourceAllocation {
    pub stage_key: String,
    pub source_id: i64,
    pub allocated_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLearningPlanAllocation {
    pub time_summary: LearningTimeSummary,
    pub stage_allocations: Vec<StageHourAllocation>,
    pub source_allocations: Vec<SourceHourAllocation>,
    pub daily_allocations: Vec<DailyHourAllocation>,
    pub stage_source_allocations: Vec<StageSourceAllocation>,
    pub warnings: Vec<String>,
}

pub struct LocalLearningPlanInput {
    pub learning_goal: LearningGoal,
    pub daily_study_hours: f64,
    pub selected_learning_sources: Vec<SelectedLearningSource>,
}

pub struct LocalLearningPlanService;

impl LocalLearningPlanService {
    pub fn calculate(
        input: LocalLearningPlanInput,
        sources: Vec<LearningPlanSource>,
    ) -> Result<LocalLearningPlanAllocation, String> {
        validate_daily_study_hours(input.daily_study_hours)?;
        if input.selected_learning_sources.is_empty() {
            return Err("请至少选择一个数据文件。".into());
        }
        let config = learning_goal_config(input.learning_goal);
        validate_stage_ratios(&config.stage_ratios)?;

        let mut selected = Vec::new();
        for item in input.selected_learning_sources {
            let source = sources
                .iter()
                .find(|source| source.id == item.source_id)
                .ok_or_else(|| format!("数据文件 ID {} 不存在。", item.source_id))?;
            if !source.is_enabled || !source.is_available {
                return Err(format!("数据文件“{}”当前不可用。", source.display_name));
            }
            if !source.file_extension.eq_ignore_ascii_case("xlsx") {
                return Err(format!(
                    "数据文件“{}”暂不支持参与自动学习计划。",
                    source.display_name
                ));
            }
            selected.push((item, source.clone()));
        }

        if !selected
            .iter()
            .any(|(item, _)| item.importance_level.included_in_plan())
        {
            return Err("请至少将一个数据文件设置为常规、重点或核心资料。".into());
        }

        let target = round_to_half_hour(COURSE_BASELINE_HOURS * config.coverage_ratio);
        let available = round_to_half_hour(config.total_days as f64 * input.daily_study_hours);
        let planned = target.min(available);
        let summary = LearningTimeSummary {
            baseline_course_hours: COURSE_BASELINE_HOURS,
            target_hours: target,
            available_hours: available,
            planned_hours: planned,
            missing_hours: round_to_half_hour((target - available).max(0.0)),
            extra_available_hours: round_to_half_hour((available - target).max(0.0)),
            target_coverage_rate: if target > 0.0 { planned / target } else { 0.0 },
            recommended_daily_hours: ceil_to_half_hour(target / config.total_days as f64),
            total_days: config.total_days,
            daily_study_hours: input.daily_study_hours,
        };

        let stage_hours = allocate_half_hour_blocks(
            planned,
            &config
                .stage_ratios
                .iter()
                .map(|stage| stage.ratio)
                .collect::<Vec<_>>(),
        )?;
        let stages = config
            .stage_ratios
            .iter()
            .zip(stage_hours)
            .map(|(stage, hours)| StageHourAllocation {
                stage_key: stage.stage_key.into(),
                stage_name: stage.stage_name.into(),
                ratio: stage.ratio,
                allocated_hours: hours,
            })
            .collect::<Vec<_>>();
        let source_hours = allocate_half_hour_blocks(
            planned,
            &selected
                .iter()
                .map(|(item, _)| item.importance_level.weight())
                .collect::<Vec<_>>(),
        )?;
        let source_allocations = selected
            .into_iter()
            .zip(source_hours)
            .map(|((item, source), hours)| SourceHourAllocation {
                source_id: source.id,
                display_name: source.display_name,
                category: source.category,
                importance_level: item.importance_level,
                importance_weight: item.importance_level.weight(),
                allocated_hours: hours,
                included_in_plan: item.importance_level.included_in_plan(),
            })
            .collect();
        let daily_allocations = allocate_days(config.total_days, input.daily_study_hours, &stages)?;
        let mut warnings = Vec::new();
        if summary.missing_hours > 0.0 {
            warnings.push(format!(
                "当前可用时间为{}小时，低于当前学习目标建议的{}小时。本计划将优先安排权重较高的资料。",
                summary.available_hours, summary.target_hours
            ));
        }
        if summary.extra_available_hours > 0.0 {
            warnings.push(format!(
                "超出目标的{}小时作为可选拓展容量，不会自动填入重复任务。",
                summary.extra_available_hours
            ));
        }

        Ok(LocalLearningPlanAllocation {
            time_summary: summary,
            stage_allocations: stages,
            source_allocations,
            daily_allocations,
            stage_source_allocations: Vec::new(),
            warnings,
        })
    }
}

pub fn validate_daily_study_hours(value: f64) -> Result<(), String> {
    if !value.is_finite() || !(0.5..=24.0).contains(&value) {
        return Err("每日学习时间必须是0.5至24之间的有限小时数。".into());
    }
    if ((value * 2.0).round() - value * 2.0).abs() > 1e-9 {
        return Err("每日学习时间必须是0.5小时的整数倍。".into());
    }
    Ok(())
}

pub fn resolve_knowledge_point_weight(
    label: Option<&str>,
    numeric: Option<f64>,
) -> (f64, Option<String>) {
    if let Some(value) = numeric {
        if value.is_finite() && value >= 0.0 {
            return (value, None);
        }
        return (1.0, Some("知识点权重无效，已回退为1.0。".into()));
    }
    let value = match label.unwrap_or("").trim() {
        "重要" => 1.3,
        "非常重要" | "核心" => 1.6,
        _ => 1.0,
    };
    (value, None)
}

pub fn round_to_half_hour(value: f64) -> f64 {
    (value * 2.0).round() / 2.0
}

fn ceil_to_half_hour(value: f64) -> f64 {
    (value * 2.0).ceil() / 2.0
}

fn validate_stage_ratios(stages: &[StageRatioConfig]) -> Result<(), String> {
    let sum: f64 = stages.iter().map(|stage| stage.ratio).sum();
    if (sum - 1.0).abs() > 1e-9 {
        Err("阶段比例总和必须等于1。".into())
    } else {
        Ok(())
    }
}

fn allocate_half_hour_blocks(total: f64, weights: &[f64]) -> Result<Vec<f64>, String> {
    if !total.is_finite() || total < 0.0 || ((total * 2.0).round() - total * 2.0).abs() > 1e-9 {
        return Err("待分配时间必须是非负的0.5小时整数倍。".into());
    }
    if weights
        .iter()
        .any(|weight| !weight.is_finite() || *weight < 0.0)
    {
        return Err("分配权重必须是非负有限数值。".into());
    }
    let sum: f64 = weights.iter().sum();
    if sum <= 0.0 {
        return Err("参与自动计划的数据总权重不能为0。".into());
    }
    let blocks = (total * 2.0).round() as usize;
    let theoretical = weights
        .iter()
        .map(|weight| blocks as f64 * weight / sum)
        .collect::<Vec<_>>();
    let mut allocated = theoretical
        .iter()
        .map(|value| value.floor() as usize)
        .collect::<Vec<_>>();
    let assigned: usize = allocated.iter().sum();
    let mut order = (0..weights.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| {
        (theoretical[*b] - theoretical[*b].floor())
            .partial_cmp(&(theoretical[*a] - theoretical[*a].floor()))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(b))
    });
    for index in order.into_iter().take(blocks.saturating_sub(assigned)) {
        allocated[index] += 1;
    }
    Ok(allocated
        .into_iter()
        .map(|blocks| blocks as f64 / 2.0)
        .collect())
}

fn allocate_days(
    days: u32,
    daily: f64,
    stages: &[StageHourAllocation],
) -> Result<Vec<DailyHourAllocation>, String> {
    let capacity = (daily * 2.0).round() as usize;
    let mut remaining = stages
        .iter()
        .map(|stage| (stage.allocated_hours * 2.0).round() as usize)
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for day in 1..=days {
        let mut available = capacity;
        let mut items = Vec::new();
        for (index, stage) in stages.iter().enumerate() {
            if available == 0 {
                break;
            }
            let used = available.min(remaining[index]);
            if used > 0 {
                items.push(DailyStageAllocation {
                    stage_key: stage.stage_key.clone(),
                    hours: used as f64 / 2.0,
                });
                remaining[index] -= used;
                available -= used;
            }
        }
        result.push(DailyHourAllocation {
            day_index: day,
            planned_hours: (capacity - available) as f64 / 2.0,
            remaining_capacity_hours: available as f64 / 2.0,
            stage_allocations: items,
        });
    }
    if remaining.iter().sum::<usize>() > 0 {
        return Err("每日容量不足，无法装入全部计划时间。".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: i64) -> LearningPlanSource {
        LearningPlanSource {
            id,
            display_name: format!("{id}.xlsx"),
            category: "内置知识点".into(),
            file_extension: "xlsx".into(),
            is_enabled: true,
            is_available: true,
        }
    }

    fn make(
        goal: LearningGoal,
        daily: f64,
        levels: &[SourceImportanceLevel],
    ) -> (LocalLearningPlanInput, Vec<LearningPlanSource>) {
        (
            LocalLearningPlanInput {
                learning_goal: goal,
                daily_study_hours: daily,
                selected_learning_sources: levels
                    .iter()
                    .enumerate()
                    .map(|(i, level)| SelectedLearningSource {
                        source_id: i as i64 + 1,
                        importance_level: *level,
                    })
                    .collect(),
            },
            (1..=levels.len()).map(|id| source(id as i64)).collect(),
        )
    }

    #[test]
    fn valid_and_invalid_daily_hours() {
        for value in [0.5, 1.0, 1.5, 2.0, 24.0] {
            assert!(validate_daily_study_hours(value).is_ok());
        }
        for value in [0.0, -0.5, 0.25, 1.3, 24.5, f64::NAN, f64::INFINITY] {
            assert!(validate_daily_study_hours(value).is_err());
        }
    }

    #[test]
    fn targets_and_ratios_are_goal_specific() {
        for (goal, target) in [
            (LearningGoal::FinalSprint, 16.0),
            (LearningGoal::GapFilling, 32.0),
            (LearningGoal::SystematicLearning, 48.0),
            (LearningGoal::ComprehensiveImprovement, 64.0),
        ] {
            let config = learning_goal_config(goal);
            assert_eq!(COURSE_BASELINE_HOURS * config.coverage_ratio, target);
            assert!((config.stage_ratios.iter().map(|s| s.ratio).sum::<f64>() - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn capacity_examples_preserve_source_semantics() {
        let (input, sources) = make(
            LearningGoal::SystematicLearning,
            1.0,
            &[SourceImportanceLevel::Normal],
        );
        let result = LocalLearningPlanService::calculate(input, sources).unwrap();
        assert_eq!(
            (
                result.time_summary.target_hours,
                result.time_summary.available_hours,
                result.time_summary.planned_hours,
                result.time_summary.missing_hours
            ),
            (48.0, 21.0, 21.0, 27.0)
        );

        let (input, sources) = make(
            LearningGoal::ComprehensiveImprovement,
            3.0,
            &[SourceImportanceLevel::Normal],
        );
        let result = LocalLearningPlanService::calculate(input, sources).unwrap();
        assert_eq!(result.time_summary.extra_available_hours, 20.0);
    }

    #[test]
    fn allocations_preserve_totals() {
        let (input, sources) = make(
            LearningGoal::ComprehensiveImprovement,
            3.0,
            &[
                SourceImportanceLevel::Normal,
                SourceImportanceLevel::Important,
                SourceImportanceLevel::Core,
                SourceImportanceLevel::Reference,
            ],
        );
        let result = LocalLearningPlanService::calculate(input, sources).unwrap();
        assert_eq!(
            result
                .source_allocations
                .iter()
                .map(|x| x.allocated_hours)
                .sum::<f64>(),
            64.0
        );
        assert_eq!(result.source_allocations[3].allocated_hours, 0.0);
        assert_eq!(
            result
                .stage_allocations
                .iter()
                .map(|x| x.allocated_hours)
                .sum::<f64>(),
            64.0
        );
        assert_eq!(
            result
                .daily_allocations
                .iter()
                .map(|x| x.planned_hours)
                .sum::<f64>(),
            64.0
        );
        assert!(result
            .daily_allocations
            .iter()
            .all(|x| x.planned_hours <= 3.0));
    }

    #[test]
    fn all_reference_rejected() {
        let (input, sources) = make(
            LearningGoal::FinalSprint,
            1.0,
            &[SourceImportanceLevel::Reference],
        );
        assert!(LocalLearningPlanService::calculate(input, sources).is_err());
    }

    #[test]
    fn knowledge_weight_fallback() {
        assert_eq!(resolve_knowledge_point_weight(None, None).0, 1.0);
        assert_eq!(resolve_knowledge_point_weight(Some("重要"), None).0, 1.3);
        assert_eq!(resolve_knowledge_point_weight(Some("核心"), None).0, 1.6);
        assert!(resolve_knowledge_point_weight(None, Some(-1.0)).1.is_some());
    }
}
