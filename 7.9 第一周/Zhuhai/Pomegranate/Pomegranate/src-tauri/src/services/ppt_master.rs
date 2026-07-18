use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Instant;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::time::{timeout, Duration};

use crate::database::Database;
use crate::error::AppError;
use crate::models::{PluginAiChatInput, PluginAiMessage};
use crate::services::ai::{AiRequestProgress, AiService};

#[path = "ppt_master_native_density.rs"]
mod native_density;
#[path = "ppt_master_native_planning.rs"]
mod native_planning;
#[path = "ppt_master_native_state.rs"]
mod native_state;
#[path = "ppt_master_native_theme.rs"]
mod native_theme;
use native_density::{run_space_utilization_check, NativePageDensityContract};
use native_planning::{
    assemble_slide_plan, deck_outline_schema, load_or_create_checkpoint, parse_deck_outline,
    parse_slide_spec, persist_checkpoint as persist_native_planning_checkpoint, read_outline,
    read_slide_spec, slide_spec_schema, write_outline, write_slide_spec, DeckOutline,
    NativeMaterialIndex, NativePlanningCheckpoint, NativePlanningContractError,
    NativePlanningErrorKind, NativePlanningRequestMetric, SlideSpec,
    NATIVE_PLANNING_CHECKPOINT_FILE, NATIVE_PLANNING_MAX_ATTEMPTS,
};
use native_state::{
    find_matching_resume_project, invalidate_downstream, now as native_state_now, read_state,
    write_state_atomic, NativeFingerprintInput, NativeGenerationState, NativeStateModel,
    NativeTextGeometryState, NATIVE_CANVAS, NATIVE_GENERATION_SPEC_VERSION, NATIVE_STATE_FILE,
};
use native_theme::{
    persist_theme_spec, validate_svg_theme, validate_visible_text_integrity, NativeThemeSpec,
    NATIVE_THEME_SPEC_FILE,
};

const SVG_TO_PPTX_SCRIPT: &str = "skills/ppt-master/scripts/svg_to_pptx.py";
const SVG_QUALITY_CHECKER_SCRIPT: &str = "skills/ppt-master/scripts/svg_quality_checker.py";
const PROJECT_MANAGER_SCRIPT: &str = "skills/ppt-master/scripts/project_manager.py";
const TOTAL_MD_SPLIT_SCRIPT: &str = "skills/ppt-master/scripts/total_md_split.py";
const FINALIZE_SVG_SCRIPT: &str = "skills/ppt-master/scripts/finalize_svg.py";
const PPT_MASTER_SKILL_MD: &str = "skills/ppt-master/SKILL.md";
const PPT_MASTER_EXECUTOR_BASE: &str = "skills/ppt-master/references/executor-base.md";
const PPT_MASTER_SHARED_STANDARDS: &str = "skills/ppt-master/references/shared-standards.md";
const PPT_MASTER_MODES_DIR: &str = "skills/ppt-master/references/modes";
const PPT_MASTER_VISUAL_STYLES_DIR: &str = "skills/ppt-master/references/visual-styles";
const PPT_MASTER_LAYOUTS_DIR: &str = "skills/ppt-master/templates/layouts";
const PPT_MASTER_CHARTS_DIR: &str = "skills/ppt-master/templates/charts";
const AI_PPT_TIMEOUT_SECS: u64 = 120;
const NATIVE_AI_TIMEOUT_SECS: u64 = 300;
const NATIVE_GENERATION_MAX_OUTPUT_TOKENS: i64 = 16_384;
#[cfg(test)]
const NATIVE_PLAN_JSON_MAX_ATTEMPTS: usize = 2;
const NATIVE_SVG_REPAIR_TIMEOUT_CONFIG_KEY: &str = "ppt.native_svg_repair_timeout_seconds";
const NATIVE_SVG_REPAIR_TIMEOUT_DEFAULT_SECS: u64 = 300;
const NATIVE_SVG_REPAIR_TIMEOUT_MIN_SECS: u64 = 60;
const NATIVE_SVG_REPAIR_TIMEOUT_MAX_SECS: u64 = 1_200;
const NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE: usize = 2;
const NATIVE_SVG_REPAIR_MAX_OUTPUT_TOKENS: i64 = 8_192;
const NATIVE_TEXT_GEOMETRY_CHECKER_SOURCE: &str =
    include_str!("../../scripts/ppt_native_text_geometry.py");
const NATIVE_TEXT_GEOMETRY_CHECKER_FILE: &str = "ppt_native_text_geometry_v1.py";
const NATIVE_TEXT_GEOMETRY_MAX_AI_ATTEMPTS_PER_PAGE: usize = 2;
const NATIVE_EXECUTOR_CLIP_PATH_RULE: &str =
    "Never generate <clipPath> or a clip-path attribute on any <g> or shape. There are no Executor exceptions: draw the final circle, ellipse, rect, or path geometry directly instead of clipping. This includes portraits, avatars, cards, rounded corners, maps, and decorations.";
const NATIVE_POWERPOINT_GEOMETRY_CHECKER_SOURCE: &str =
    include_str!("../../scripts/ppt_native_powerpoint_geometry.ps1");
const NATIVE_POWERPOINT_GEOMETRY_CHECKER_FILE: &str = "ppt_native_powerpoint_geometry_v1.ps1";
const NATIVE_POWERPOINT_GEOMETRY_MAX_AI_ATTEMPTS_PER_PAGE: usize = 3;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PptMasterCheckInput {
    pub ppt_master_root: String,
    pub python_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PptMasterCheckResult {
    pub ok: bool,
    pub script_path: String,
    pub python_version: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PptMasterExportInput {
    pub ppt_master_root: String,
    pub python_path: String,
    pub project_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PptMasterExportResult {
    pub success: bool,
    pub output_path: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PptUnderstandingDraftInput {
    #[serde(default)]
    pub understanding_summary: String,
    #[serde(default)]
    pub key_priorities: String,
    #[serde(default)]
    pub narrative_mainline: String,
    #[serde(default)]
    pub suggested_page_structure: String,
    #[serde(default)]
    pub visual_expression_advice: String,
    #[serde(default)]
    pub open_questions: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PptUnderstandingInput {
    Structured(PptUnderstandingDraftInput),
    Legacy(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PptMaterialSourceInput {
    pub id: i64,
    pub source_type: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PptMasterGenerateInput {
    pub ppt_master_root: String,
    pub python_path: String,
    #[serde(default)]
    pub prompt: String,
    pub planning_context: Option<String>,
    pub ai_understanding_result: Option<PptUnderstandingInput>,
    pub understanding_summary: Option<String>,
    pub key_priorities: Option<String>,
    pub suggested_page_structure: Option<String>,
    pub narrative_mainline: Option<String>,
    pub visual_expression_advice: Option<String>,
    #[serde(default)]
    pub visual_suggestions: Option<String>,
    pub open_questions: Option<String>,
    pub raw_material: Option<String>,
    #[serde(default)]
    pub material_sources: Vec<PptMaterialSourceInput>,
    pub extra_requirements: Option<String>,
    pub model_id: Option<i64>,
    pub title: Option<String>,
    pub audience: Option<String>,
    pub slide_count: Option<usize>,
    pub style: Option<String>,
    #[serde(default)]
    pub custom_style: Option<String>,
    pub generation_engine: Option<String>,
    pub mode: Option<String>,
    pub visual_style: Option<String>,
    #[serde(default)]
    pub layout_bias: Vec<String>,
    #[serde(default)]
    pub chart_bias: Vec<String>,
    pub output_dir: Option<String>,
    pub generation_mode: Option<String>,
    #[serde(default)]
    pub block_on_quality_failure: Option<bool>,
}

impl PptMasterGenerateInput {
    pub(crate) fn block_on_quality_failure(&self) -> bool {
        self.block_on_quality_failure.unwrap_or(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PptGenerationRoute {
    LegacyFallback,
    PptMasterNative,
}

impl PptGenerationRoute {
    pub(crate) fn generation_mode(self) -> &'static str {
        match self {
            Self::LegacyFallback => "template",
            Self::PptMasterNative => "agent",
        }
    }

    pub(crate) fn generation_engine(self) -> &'static str {
        match self {
            Self::LegacyFallback => "legacy_fallback",
            Self::PptMasterNative => "ppt_master_native",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PptMasterGenerateResult {
    pub success: bool,
    pub project_path: Option<String>,
    pub pptx_path: Option<String>,
    pub final_pptx_path: Option<String>,
    pub slide_plan_path: Option<String>,
    pub design_spec_path: Option<String>,
    pub quality_check_passed: Option<bool>,
    pub generation_mode: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub error: Option<String>,
    pub generation_engine: String,
    pub failure_stage: Option<String>,
    pub failure_type: Option<String>,
    pub failed_page: Option<usize>,
    pub timed_out_after_seconds: Option<u64>,
    pub failed_svg_file: Option<String>,
    pub stage: Option<String>,
    pub page_number: Option<usize>,
    pub svg_path: Option<String>,
    pub violated_rule: Option<String>,
    pub checker_summary: Option<String>,
    pub intermediate_artifact_paths: Vec<String>,
}

#[derive(Debug, Default)]
struct PptGenerationFailureMetadata {
    failure_stage: String,
    failure_type: String,
    failed_page: Option<usize>,
    timed_out_after_seconds: Option<u64>,
    failed_svg_file: Option<String>,
}

impl PptMasterGenerateResult {
    pub fn failure(
        error: String,
        generation_mode: String,
        generation_engine: String,
        duration_ms: u128,
    ) -> Self {
        let metadata = classify_ppt_generation_failure(&error);
        Self {
            success: false,
            project_path: None,
            pptx_path: None,
            final_pptx_path: None,
            slide_plan_path: None,
            design_spec_path: None,
            quality_check_passed: None,
            generation_mode,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms,
            error: Some(error),
            generation_engine,
            failure_stage: Some(metadata.failure_stage),
            failure_type: Some(metadata.failure_type),
            failed_page: metadata.failed_page,
            timed_out_after_seconds: metadata.timed_out_after_seconds,
            failed_svg_file: metadata.failed_svg_file,
            stage: None,
            page_number: None,
            svg_path: None,
            violated_rule: None,
            checker_summary: None,
            intermediate_artifact_paths: Vec::new(),
        }
    }

    fn with_generated_artifacts(
        mut self,
        project: &Path,
        slide_plan_path: &Path,
        design_spec_path: &Path,
        quality_check_passed: Option<bool>,
        stdout: String,
        stderr: String,
    ) -> Self {
        self.project_path = Some(project.to_string_lossy().to_string());
        self.slide_plan_path = Some(slide_plan_path.to_string_lossy().to_string());
        self.design_spec_path = Some(design_spec_path.to_string_lossy().to_string());
        self.quality_check_passed = quality_check_passed;
        self.stdout = stdout;
        self.stderr = stderr;
        self.intermediate_artifact_paths = native_intermediate_artifact_paths(project);
        self
    }

    fn with_partial_artifacts(
        mut self,
        project: Option<&Path>,
        slide_plan_path: Option<&Path>,
        design_spec_path: Option<&Path>,
        quality_check_passed: Option<bool>,
        stdout: String,
        stderr: String,
    ) -> Self {
        self.project_path = project.map(|path| path.to_string_lossy().to_string());
        self.slide_plan_path = slide_plan_path.map(|path| path.to_string_lossy().to_string());
        self.design_spec_path = design_spec_path.map(|path| path.to_string_lossy().to_string());
        self.quality_check_passed = quality_check_passed;
        self.stdout = stdout;
        self.stderr = stderr;
        if let Some(project) = project {
            self.intermediate_artifact_paths = native_intermediate_artifact_paths(project);
        }
        self
    }

    fn with_classified_failure(mut self) -> Self {
        if self.success {
            return self;
        }
        let metadata = classify_ppt_generation_failure(self.error.as_deref().unwrap_or(""));
        self.failure_stage = Some(metadata.failure_stage);
        self.failure_type = Some(metadata.failure_type);
        self.failed_page = metadata.failed_page;
        self.timed_out_after_seconds = metadata.timed_out_after_seconds;
        self.failed_svg_file = metadata.failed_svg_file;
        self.stage = self.failure_stage.clone();
        self.page_number = self.failed_page;
        self
    }
}

fn native_intermediate_artifact_paths(project: &Path) -> Vec<String> {
    [
        project.join(NATIVE_STATE_FILE),
        project.join(NATIVE_THEME_SPEC_FILE),
        project.join("design_spec.md"),
        project.join("spec_lock.md"),
        project.join("slide_plan.json"),
        project.join("svg_output"),
        project.join("notes"),
        project.join("svg_final"),
        project.join("exports"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .map(|path| path.to_string_lossy().to_string())
    .collect()
}

fn normalize_generation_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub(crate) fn resolve_generation_route(
    generation_engine: Option<&str>,
    generation_mode: Option<&str>,
) -> Result<PptGenerationRoute, AppError> {
    let engine = normalize_generation_value(generation_engine);
    let mode = normalize_generation_value(generation_mode);
    match (engine, mode) {
        (None, None)
        | (Some("legacy_fallback"), None)
        | (None, Some("template"))
        | (Some("legacy_fallback"), Some("template")) => Ok(PptGenerationRoute::LegacyFallback),
        (Some("ppt_master_native"), None)
        | (None, Some("agent"))
        | (Some("ppt_master_native"), Some("agent")) => Ok(PptGenerationRoute::PptMasterNative),
        (Some(engine), Some(mode))
            if matches!(engine, "legacy_fallback" | "ppt_master_native")
                && matches!(mode, "template" | "agent") =>
        {
            Err(AppError::InvalidInput(format!(
                "generationMode 与 generationEngine 不一致: mode={mode}, engine={engine}"
            )))
        }
        (Some(engine), _) if !matches!(engine, "legacy_fallback" | "ppt_master_native") => Err(
            AppError::InvalidInput(format!("generationEngine 无效: {engine}")),
        ),
        (_, Some(mode)) if !matches!(mode, "template" | "agent") => Err(AppError::InvalidInput(
            format!("generationMode 无效: {mode}"),
        )),
        _ => Err(AppError::InvalidInput("无法解析 PPT 生成模式".to_string())),
    }
}

fn requested_generation_route(
    input: &PptMasterGenerateInput,
) -> Result<PptGenerationRoute, AppError> {
    resolve_generation_route(
        input.generation_engine.as_deref(),
        input.generation_mode.as_deref(),
    )
}

fn generation_identity_for_error(input: &PptMasterGenerateInput) -> PptGenerationRoute {
    requested_generation_route(input).unwrap_or_else(|_| {
        if normalize_generation_value(input.generation_engine.as_deref())
            == Some("ppt_master_native")
            || normalize_generation_value(input.generation_mode.as_deref()) == Some("agent")
        {
            PptGenerationRoute::PptMasterNative
        } else {
            PptGenerationRoute::LegacyFallback
        }
    })
}

fn native_stage_start(log_lines: &mut Vec<String>, stage: &str, input: &str) -> Instant {
    log_lines.push(format!(
        "[Native Stage] stage={stage} status=start input={} output=-",
        single_line_log_value(input)
    ));
    Instant::now()
}

fn native_stage_success(log_lines: &mut Vec<String>, stage: &str, started: Instant, output: &str) {
    log_lines.push(format!(
        "[Native Stage] stage={stage} status=success durationMs={} input=- output={} error=-",
        started.elapsed().as_millis(),
        single_line_log_value(output)
    ));
}

fn native_stage_failure(log_lines: &mut Vec<String>, stage: &str, started: Instant, error: &str) {
    log_lines.push(format!(
        "[Native Stage] stage={stage} status=failed durationMs={} input=- output=- error={}",
        started.elapsed().as_millis(),
        single_line_log_value(error)
    ));
}

fn single_line_log_value(value: &str) -> String {
    let flattened = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_for_log(&flattened, 360)
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut shortened = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    shortened.push('…');
    shortened
}

fn native_failure_type(stage: &str, error: &str) -> &'static str {
    if error.contains("native_planning_json_syntax") {
        return "native_planning_json_syntax";
    }
    if error.contains("native_planning_schema_validation") {
        return "native_planning_schema_validation";
    }
    if error.contains("native_planning_finish_reason") {
        return "native_planning_finish_reason";
    }
    if error.contains("native_planning_network") {
        return "native_planning_network";
    }
    if error.contains("超时") || error.contains("timeout") {
        return "native_ai_timeout";
    }
    match stage {
        "resolve_config" => "native_configuration_error",
        "prepare_project" => "native_project_prepare_failed",
        "build_design_spec" | "build_spec_lock" | "build_slide_plan" => "native_planning_failed",
        "execute_slides" if error.contains("缺少") => "native_svg_missing",
        "execute_slides" => "native_slide_execution_failed",
        "validate_svgs" => "native_svg_validation_failed",
        "validate_text_geometry" => "native_text_geometry_validation_failed",
        "validate_powerpoint_text_geometry" => "native_powerpoint_text_geometry_validation_failed",
        "generate_notes" => "native_notes_failed",
        "export_pptx" => "native_export_failed",
        _ => "native_generation_failed",
    }
}

#[allow(clippy::too_many_arguments)]
fn native_failure_result(
    error: AppError,
    stage: &str,
    pipeline_started: Instant,
    stage_started: Instant,
    log_lines: &mut Vec<String>,
    project: Option<&Path>,
    slide_plan_path: Option<&Path>,
    design_spec_path: Option<&Path>,
    quality_check_passed: Option<bool>,
    extra_stdout: &[String],
    extra_stderr: &[String],
) -> PptMasterGenerateResult {
    let error_text = error.to_string();
    if let Some(project) = project {
        if project.join(NATIVE_STATE_FILE).is_file() {
            if let Ok(mut state) = read_state(project) {
                state.current_stage = stage.to_string();
                state.set_status("failed");
                let _ = write_state_atomic(project, &state);
            }
        }
    }
    native_stage_failure(log_lines, stage, stage_started, &error_text);
    let mut result = PptMasterGenerateResult::failure(
        error_text.clone(),
        "agent".to_string(),
        "ppt_master_native".to_string(),
        pipeline_started.elapsed().as_millis(),
    )
    .with_partial_artifacts(
        project,
        slide_plan_path,
        design_spec_path,
        quality_check_passed,
        join_outputs(log_lines, extra_stdout),
        join_outputs(&[], extra_stderr),
    );
    result.failure_stage = Some(stage.to_string());
    result.failure_type = Some(native_failure_type(stage, &error_text).to_string());
    if let Some(captures) = native_failed_svg_regex().captures(&error_text) {
        result.failed_svg_file = captures.get(1).map(|value| value.as_str().to_string());
    }
    if let Some(captures) = native_failed_page_regex().captures(&error_text) {
        result.failed_page = captures
            .get(1)
            .and_then(|value| value.as_str().parse::<usize>().ok());
    }
    result.timed_out_after_seconds = error_text
        .split_once("超过 ")
        .and_then(|(_, suffix)| suffix.split_whitespace().next())
        .and_then(|seconds| seconds.parse::<u64>().ok());
    result
}

fn with_native_quality_failure(
    mut result: PptMasterGenerateResult,
    stage: &str,
    project: &Path,
    page_number: Option<usize>,
    file_name: &str,
    violated_rule: &str,
    checker_summary: &str,
) -> PptMasterGenerateResult {
    let svg_path = project.join("svg_output").join(file_name);
    result.stage = Some(stage.to_string());
    result.failure_stage = Some(stage.to_string());
    result.page_number = page_number;
    result.failed_page = page_number;
    result.failed_svg_file = Some(file_name.to_string());
    result.svg_path = Some(svg_path.to_string_lossy().to_string());
    result.violated_rule = Some(violated_rule.to_string());
    result.checker_summary = Some(checker_summary.to_string());
    result.intermediate_artifact_paths = native_intermediate_artifact_paths(project);
    result
}

fn native_page_validation_error_message(
    project: &Path,
    stage: &str,
    failure: &NativeQualityFailure,
) -> String {
    if stage == "validate_svgs" {
        return native_quality_error_message(project, failure);
    }
    let checker_name = match stage {
        "validate_powerpoint_text_geometry" => "PowerPoint 导出后文本几何检查",
        "validate_visible_text_integrity" => "原生 SVG 可见文字完整性检查",
        "validate_theme_consistency" => "原生 SVG 全局主题一致性检查",
        "validate_space_utilization" => "原生 SVG 页面空间利用检查",
        _ => "原生 SVG 文本几何检查",
    };
    format!(
        "ppt-master {}失败；strict native 禁止自动回退或全页缩放；stage={}；page_number={}；svg_path={}；violated_rule={}；checker_summary={}；intermediate_artifacts={}",
        checker_name,
        stage,
        failure
            .page_number
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        project
            .join("svg_output")
            .join(&failure.file_name)
            .display(),
        failure.violated_rule,
        single_line_log_value(&failure.checker_summary),
        project.display()
    )
}

fn native_page_validation_may_retry(stage: &str, attempts: usize) -> bool {
    let limit = match stage {
        "validate_svgs"
        | "validate_visible_text_integrity"
        | "validate_theme_consistency"
        | "validate_space_utilization" => NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE,
        "validate_text_geometry" => NATIVE_TEXT_GEOMETRY_MAX_AI_ATTEMPTS_PER_PAGE,
        _ => return false,
    };
    attempts < limit
}

fn native_page_relayout_preserved_visible_text(expected: &[String], actual: &[String]) -> bool {
    expected == actual
}

fn native_quality_error_message(project: &Path, failure: &NativeQualityFailure) -> String {
    format!(
        "ppt-master 原生 SVG 质量检查失败；strict native 禁止自动回退或稳定模式修复；stage=validate_svgs；page_number={}；svg_path={}；violated_rule={}；checker_summary={}；intermediate_artifacts={}",
        failure
            .page_number
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        project
            .join("svg_output")
            .join(&failure.file_name)
            .display(),
        failure.violated_rule,
        single_line_log_value(&failure.checker_summary),
        project.display()
    )
}

fn native_failed_svg_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)([0-9]{2}_[a-z0-9_-]+\.svg)").expect("valid regex"))
}

fn native_failed_page_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"P([0-9]{1,3})\b").expect("valid regex"))
}

fn classify_ppt_generation_failure(error: &str) -> PptGenerationFailureMetadata {
    let failed_svg_file = [
        "AI 修复 native 兼容 SVG：",
        "AI 修复 SVG：",
        "AI 修复 final text leakage:",
    ]
    .into_iter()
    .find_map(|marker| error.split_once(marker).map(|(_, suffix)| suffix))
    .and_then(|suffix| suffix.split_whitespace().next())
    .map(|value| {
        value
            .trim_matches(|ch: char| ch == '，' || ch == '。')
            .to_string()
    });
    let failed_page = failed_svg_file
        .as_deref()
        .and_then(|file| file.split_once('_').map(|(prefix, _)| prefix))
        .and_then(|prefix| prefix.parse::<usize>().ok());
    let timed_out_after_seconds = error
        .split_once("超过 ")
        .and_then(|(_, suffix)| suffix.split_whitespace().next())
        .and_then(|seconds| seconds.parse::<u64>().ok());

    if (error.contains("native_svg_repair_timeout") || error.contains("AI 修复 native 兼容 SVG"))
        && timed_out_after_seconds.is_some()
    {
        return PptGenerationFailureMetadata {
            failure_stage: if error.contains("AI 修复 native 兼容 SVG") {
                "native_svg_compat_repair"
            } else {
                "native_svg_repair"
            }
            .to_string(),
            failure_type: "native_svg_repair_timeout".to_string(),
            failed_page,
            timed_out_after_seconds,
            failed_svg_file,
        };
    }
    if error.contains("ppt-master 根目录") || error.contains("找不到 svg_to_pptx.py 脚本") {
        return PptGenerationFailureMetadata {
            failure_stage: "configuration".to_string(),
            failure_type: "ppt_master_root_invalid".to_string(),
            ..Default::default()
        };
    }
    if error.contains("Python") {
        return PptGenerationFailureMetadata {
            failure_stage: "configuration".to_string(),
            failure_type: "python_configuration_error".to_string(),
            ..Default::default()
        };
    }
    if error.contains("导出目录")
        || error.contains("输出目录")
        || error.contains("导出文件夹")
        || error.contains("复制到")
    {
        return PptGenerationFailureMetadata {
            failure_stage: "output".to_string(),
            failure_type: "output_path_error".to_string(),
            ..Default::default()
        };
    }
    PptGenerationFailureMetadata {
        failure_stage: "generation".to_string(),
        failure_type: "generation_failed".to_string(),
        failed_page,
        timed_out_after_seconds,
        failed_svg_file,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SlidePlan {
    title: String,
    subtitle: String,
    audience: String,
    style: String,
    #[serde(default = "default_theme")]
    theme: Theme,
    #[serde(default)]
    theme_allocation: Vec<ThemeAllocation>,
    slides: Vec<Slide>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeAllocation {
    page_id: String,
    assigned_theme: String,
    exclusive_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ContentBlock {
    #[serde(default)]
    label: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Theme {
    name: String,
    primary: String,
    secondary: String,
    accent: String,
    background: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Slide {
    page: usize,
    #[serde(default)]
    page_index: usize,
    #[serde(default)]
    page_id: String,
    #[serde(rename = "type")]
    slide_type: String,
    #[serde(default)]
    layout: String,
    title: String,
    subtitle: String,
    bullets: Vec<String>,
    #[serde(default)]
    visual_hint: String,
    #[serde(default)]
    page_theme: String,
    #[serde(default)]
    main_claim: String,
    #[serde(default)]
    core_message: String,
    #[serde(default)]
    content_scope: String,
    #[serde(default)]
    content_blocks: Vec<ContentBlock>,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    relation: String,
    #[serde(default)]
    density: String,
    #[serde(default)]
    visual_intent: String,
    #[serde(default)]
    must_include: Vec<String>,
    #[serde(default)]
    must_avoid: Vec<String>,
    #[serde(default)]
    page_rhythm: String,
    #[serde(default)]
    chart_ref: String,
    #[serde(default)]
    chart_type: String,
    #[serde(default)]
    file_stem: String,
    speaker_note: String,
}

#[derive(Debug, Clone)]
struct PptMasterStyleMapping {
    user_style: String,
    mode: String,
    visual_style: String,
    mode_reference: String,
    visual_style_reference: String,
    layout_bias: Vec<String>,
    chart_bias: Vec<String>,
    template_provenance: Vec<String>,
}

#[derive(Debug)]
struct PptMasterResources {
    modes_index: String,
    visual_styles_index: String,
    layouts_index: String,
    charts_index: String,
    executor_base: String,
    shared_standards: String,
}

#[derive(Debug, Clone)]
struct ChartCatalog {
    keys: std::collections::HashSet<String>,
}

#[derive(Debug, Clone)]
struct NativeSvgIssue {
    file_name: String,
    issue_type: String,
    unsupported_elements: Vec<String>,
    detail: String,
}

#[derive(Debug, Clone)]
struct FinalTextIssue {
    file_name: String,
    leaked_terms: Vec<String>,
}

pub struct PptMasterService;

impl PptMasterService {
    pub fn check(input: PptMasterCheckInput) -> Result<PptMasterCheckResult, AppError> {
        let root = parse_dir("ppt-master 根目录", &input.ppt_master_root)?;
        let script_path = root.join(SVG_TO_PPTX_SCRIPT);
        let mut errors = Vec::new();

        if !script_path.is_file() {
            errors.push(format!("找不到脚本: {}", script_path.display()));
        }

        let python_version = match python_version(&root, &input.python_path) {
            Ok(version) => Some(version),
            Err(e) => {
                errors.push(e);
                None
            }
        };

        Ok(PptMasterCheckResult {
            ok: errors.is_empty(),
            script_path: script_path.to_string_lossy().to_string(),
            python_version,
            errors,
        })
    }

    pub fn export(input: PptMasterExportInput) -> Result<PptMasterExportResult, AppError> {
        let root = parse_dir("ppt-master 根目录", &input.ppt_master_root)?;
        let project = parse_dir("ppt-master 项目目录", &input.project_path)?;
        ensure_python_available(&root, &input.python_path)?;

        let script_path = root.join(SVG_TO_PPTX_SCRIPT);
        if !script_path.is_file() {
            return Err(AppError::NotFound(format!(
                "找不到 svg_to_pptx.py 脚本: {}",
                script_path.display()
            )));
        }

        export_project(&root, &input.python_path, &project, Instant::now())
    }

    pub async fn generate_from_prompt(
        db: &Database,
        input: PptMasterGenerateInput,
    ) -> Result<PptMasterGenerateResult, AppError> {
        let route = match requested_generation_route(&input) {
            Ok(route) => route,
            Err(error) => {
                let identity = generation_identity_for_error(&input);
                let mut result = PptMasterGenerateResult::failure(
                    error.to_string(),
                    identity.generation_mode().to_string(),
                    identity.generation_engine().to_string(),
                    0,
                );
                result.failure_stage = Some("resolve_config".to_string());
                result.failure_type = Some("generation_route_invalid".to_string());
                result.stdout = format!(
                    "[Native Stage] stage=resolve_config status=failed durationMs=0 input=generationMode,generationEngine output=- error={}",
                    single_line_log_value(&error.to_string())
                );
                return Ok(result);
            }
        };
        println!(
            "[Engine] generation_mode={} generation_engine={}",
            route.generation_mode(),
            route.generation_engine()
        );
        match route {
            PptGenerationRoute::LegacyFallback => {
                Self::generate_from_prompt_template(db, input).await
            }
            PptGenerationRoute::PptMasterNative => {
                Self::generate_from_prompt_ppt_master_native(db, input).await
            }
        }
    }

    async fn generate_from_prompt_template(
        db: &Database,
        input: PptMasterGenerateInput,
    ) -> Result<PptMasterGenerateResult, AppError> {
        let started = Instant::now();
        let root = parse_dir("ppt-master 根目录", &input.ppt_master_root)?;
        ensure_python_available(&root, &input.python_path)?;

        let script_path = root.join(SVG_TO_PPTX_SCRIPT);
        if !script_path.is_file() {
            return Err(AppError::NotFound(format!(
                "找不到 svg_to_pptx.py 脚本: {}",
                script_path.display()
            )));
        }

        let prompt = input.prompt.trim();
        let planning_context = build_generation_planning_context(&input, prompt);
        let visible_material = build_stable_visible_material(&input, prompt);
        if prompt.is_empty() && !has_authoritative_generation_input(&input) {
            return Err(AppError::InvalidInput(
                "缺少结构化需求理解、规划上下文或原始语料".into(),
            ));
        }

        let requested_count = input.slide_count.unwrap_or(6).clamp(1, 30);
        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("AI PPT")
            .to_string();
        let style = input
            .style
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("简约商务")
            .to_string();

        let mut log_lines = Vec::new();
        log_lines.push("[Stable Mode]".to_string());
        log_lines.push("[Stable Planning Input]".to_string());
        log_lines.push(format!(
            "rawMaterialLength={}",
            input.raw_material.as_deref().unwrap_or("").chars().count()
        ));
        log_lines.push(format!(
            "materialSourceCount={}",
            input.material_sources.len()
        ));
        log_lines.push(format!(
            "hasPlanningContext={}",
            !planning_context.trim().is_empty()
        ));
        log_lines.push(format!("hasConfirmedPrompt={}", !prompt.trim().is_empty()));
        log_lines.push(format!("slideCount={}", requested_count));

        let mut plan = match generate_slide_plan_with_ai(
            db,
            &planning_context,
            input.model_id,
            &title,
            requested_count,
            &style,
        )
        .await
        {
            Ok(plan) => {
                log_lines.push("path=ai_slide_plan".to_string());
                normalize_slide_plan(plan, &title, requested_count, &style)
            }
            Err(e) => {
                log::warn!("AI slide_plan 生成失败，使用默认方案: {}", e);
                log_lines.push(format!("path=default_fallback"));
                log_lines.push(format!("[Stable Mode] fallback used, reason={}", e));
                if extract_material_units(&visible_material).is_empty() {
                    return Err(AppError::Custom(
                        "AI slide_plan 生成失败，且未能从用户语料中提取可用内容，已停止生成，避免输出占位 PPT。".into(),
                    ));
                }
                default_slide_plan(&title, requested_count, &style, &visible_material)
            }
        };
        prepare_stable_plan_for_render(&mut plan, &visible_material);
        log_stable_content_check(&plan, "[Stable Content Check]", &mut log_lines);
        if let Some(report) = validate_stable_content_plan(&plan) {
            log_lines.push(format!("[Stable Content Check] failed reason={}", report));
            log_lines.push("[Stable Content Repair] start".to_string());
            match repair_stable_slide_plan_with_ai(
                db,
                &planning_context,
                &plan,
                &report,
                input.model_id,
                &title,
                requested_count,
                &style,
            )
            .await
            {
                Ok(repaired) => {
                    let mut repaired =
                        normalize_slide_plan(repaired, &title, requested_count, &style);
                    prepare_stable_plan_for_render(&mut repaired, &visible_material);
                    if validate_stable_content_plan(&repaired).is_none() {
                        plan = repaired;
                        log_lines.push("[Stable Content Repair] done".to_string());
                    } else {
                        enrich_plan_from_material(&mut plan, &visible_material);
                        prepare_stable_plan_for_render(&mut plan, &visible_material);
                        log_lines.push("[Stable Content Repair] ai output still thin; fallback enrichment applied".to_string());
                    }
                }
                Err(e) => {
                    enrich_plan_from_material(&mut plan, &visible_material);
                    prepare_stable_plan_for_render(&mut plan, &visible_material);
                    log_lines.push(format!(
                        "[Stable Content Repair] ai failed; fallback enrichment applied: {}",
                        e
                    ));
                }
            }
        }
        prepare_stable_plan_for_render(&mut plan, &visible_material);
        if let Some(report) = validate_stable_content_plan(&plan) {
            return Err(AppError::Custom(format!(
                "Stable slide_plan content is still too thin after repair: {}",
                report
            )));
        }
        log_stable_content_check(&plan, "[Stable Content Check Final]", &mut log_lines);
        log_lines.push("[Stable Slide Plan]".to_string());
        for slide in &plan.slides {
            log_lines.push(format!(
                "P{:02} title={} coreMessage={} blocks={} evidence={}",
                slide.page,
                slide.title,
                compact_log_text(&stable_core_message(slide), ""),
                slide.content_blocks.len(),
                slide.evidence.len()
            ));
        }

        let project = create_project_dir(&root)?;
        let sources = project.join("sources");
        let notes = project.join("notes");
        let svg_output = project.join("svg_output");
        for dir_name in [
            "sources",
            "notes",
            "svg_output",
            "svg_final",
            "images",
            "icons",
            "templates",
            "analysis",
            "exports",
        ] {
            create_dir_all(&project.join(dir_name))?;
        }

        write_file(&sources.join("confirmed_prompt.md"), prompt)?;
        write_file(
            &project.join("design_spec.md"),
            &build_stable_design_spec(&plan),
        )?;
        let plan_json = serde_json::to_string_pretty(&plan)
            .map_err(|e| AppError::Custom(format!("序列化 slide_plan 失败: {}", e)))?;
        let slide_plan_path = project.join("slide_plan.json");
        write_file(&slide_plan_path, &plan_json)?;

        let render_profile = StableRenderProfile::load(&root, &plan);
        let mut stable_degradations =
            std::collections::HashMap::<usize, Vec<StableContentDegradation>>::new();
        log_lines.push(format!(
            "[Stable Visual] style={} source={} charts_index={}",
            render_profile.visual_style_id,
            render_profile.visual_style_source,
            if render_profile.chart_catalog_loaded {
                "loaded"
            } else {
                "unavailable"
            }
        ));
        for slide in &plan.slides {
            let filename = svg_filename_for_slide(slide);
            let rendered = render_slide_svg_with_profile(&plan, slide, &render_profile)?;
            for rejection in &rendered.motif_gate_rejections {
                log_lines.push(format!(
                    "[Stable Motif Gate] P{:02} {}",
                    slide.page, rejection
                ));
            }
            if let Some(reason) = &rendered.motif_fallback_reason {
                log_lines.push(format!(
                    "[Stable Motif Fallback] P{:02} {}",
                    slide.page, reason
                ));
            }
            log_lines.push(format!(
                "[Stable Motif Gate] P{:02} selected={}",
                slide.page, rendered.motif
            ));
            log_lines.push(format!(
                "[Stable Visual Selection] P{:02} layout={} motif={}",
                slide.page, rendered.layout, rendered.motif
            ));
            log_lines.push(format!(
                "[Stable Visual Diversity] duplicate_signature={} motif_reuse_count={} signature={} structure_fingerprint={}",
                rendered.duplicate_signature,
                rendered.motif_reuse_count,
                rendered.visual_signature,
                rendered.structure_fingerprint
            ));
            log_lines.extend(rendered.local_repair_logs.iter().cloned());
            for degradation in &rendered.degradations {
                log_lines.push(format!(
                    "[Stable Content Degradation] P{:02} block={} field={} action={} severity=warning",
                    slide.page,
                    degradation.block_id,
                    degradation.field,
                    degradation.action
                ));
            }
            if !rendered.degradations.is_empty() {
                stable_degradations
                    .entry(slide.page)
                    .or_default()
                    .extend(rendered.degradations.clone());
            }
            log_lines.push(format!(
                "[Stable Layout QA] P{:02} passed layout={} reflow={} warnings={}",
                slide.page,
                rendered.layout,
                rendered.reflow_attempts,
                if rendered.warnings.is_empty() {
                    "none".to_string()
                } else {
                    rendered.warnings.join(" | ")
                }
            ));
            write_file(&svg_output.join(filename), &rendered.svg)?;
        }
        write_file(
            &notes.join("total.md"),
            &build_notes_with_degradations(&plan, &stable_degradations),
        )?;

        let output_dir = input.output_dir.clone();
        let export = export_project(&root, &input.python_path, &project, started)?;
        let mut success = export.success;
        let mut error = export.error;
        let mut final_pptx_path = None;

        if export.success {
            if let (Some(dir), Some(pptx)) = (output_dir.as_deref(), export.output_path.as_deref())
            {
                match copy_final_pptx(Path::new(pptx), dir, &plan.title) {
                    Ok(path) => final_pptx_path = Some(path.to_string_lossy().to_string()),
                    Err(e) => {
                        success = false;
                        error = Some(format!("PPTX 已生成，但复制到导出文件夹失败: {}", e));
                    }
                }
            }
        }

        Ok(PptMasterGenerateResult {
            success,
            project_path: Some(project.to_string_lossy().to_string()),
            pptx_path: export.output_path,
            final_pptx_path,
            slide_plan_path: Some(slide_plan_path.to_string_lossy().to_string()),
            design_spec_path: Some(project.join("design_spec.md").to_string_lossy().to_string()),
            quality_check_passed: None,
            generation_mode: "template".to_string(),
            exit_code: export.exit_code,
            stdout: join_outputs(&log_lines, &[export.stdout]),
            stderr: export.stderr,
            duration_ms: export.duration_ms,
            error,
            generation_engine: "legacy_fallback".to_string(),
            failure_stage: None,
            failure_type: None,
            failed_page: None,
            timed_out_after_seconds: None,
            failed_svg_file: None,
            stage: None,
            page_number: None,
            svg_path: None,
            violated_rule: None,
            checker_summary: None,
            intermediate_artifact_paths: Vec::new(),
        }
        .with_classified_failure())
    }

    async fn generate_from_prompt_ppt_master_native(
        db: &Database,
        input: PptMasterGenerateInput,
    ) -> Result<PptMasterGenerateResult, AppError> {
        Self::generate_from_prompt_ppt_master_native_with_project(db, input, None).await
    }

    async fn generate_from_prompt_ppt_master_native_with_project(
        db: &Database,
        input: PptMasterGenerateInput,
        forced_project: Option<PathBuf>,
    ) -> Result<PptMasterGenerateResult, AppError> {
        let started = Instant::now();
        let mut log_lines = Vec::new();
        let block_on_quality_failure = input.block_on_quality_failure();
        let mut quality_check_passed = true;
        println!("[PPT Pipeline] service entered");
        let resolve_started = native_stage_start(
            &mut log_lines,
            "resolve_config",
            "generationMode=agent,generationEngine=ppt_master_native,pptMasterRoot,pythonPath",
        );
        let root = match parse_dir("ppt-master 根目录", &input.ppt_master_root) {
            Ok(root) => root,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "resolve_config",
                    started,
                    resolve_started,
                    &mut log_lines,
                    None,
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }
        };
        println!("[Config] pptMasterRoot={}", root.display());
        println!(
            "[Config] pythonPath={}",
            resolve_python_program(&root, &input.python_path).display()
        );
        println!("[Engine] generation_engine=ppt_master_native");
        println!(
            "[Quality Check] blockOnQualityFailure={}",
            block_on_quality_failure
        );
        log_lines.push(format!(
            "[Quality Check] blockOnQualityFailure={} strictNative=true checksSkipped=false fallback=false",
            block_on_quality_failure
        ));
        if let Err(error) = ensure_python_available(&root, &input.python_path) {
            return Ok(native_failure_result(
                error,
                "resolve_config",
                started,
                resolve_started,
                &mut log_lines,
                None,
                None,
                None,
                None,
                &[],
                &[],
            ));
        }

        let export_script = root.join(SVG_TO_PPTX_SCRIPT);
        if !export_script.is_file() {
            return Ok(native_failure_result(
                AppError::NotFound(format!(
                    "找不到 svg_to_pptx.py 脚本: {}",
                    export_script.display()
                )),
                "resolve_config",
                started,
                resolve_started,
                &mut log_lines,
                None,
                None,
                None,
                None,
                &[],
                &[],
            ));
        }
        for script in [
            PROJECT_MANAGER_SCRIPT,
            TOTAL_MD_SPLIT_SCRIPT,
            FINALIZE_SVG_SCRIPT,
        ] {
            let script_path = root.join(script);
            if !script_path.is_file() {
                return Ok(native_failure_result(
                    AppError::NotFound(format!(
                        "找不到 ppt-master 脚本: {}",
                        script_path.display()
                    )),
                    "resolve_config",
                    started,
                    resolve_started,
                    &mut log_lines,
                    None,
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }
        }
        let checker_script = root.join(SVG_QUALITY_CHECKER_SCRIPT);
        if !checker_script.is_file() {
            return Ok(native_failure_result(
                AppError::NotFound(format!(
                    "找不到 svg_quality_checker.py 脚本: {}",
                    checker_script.display()
                )),
                "resolve_config",
                started,
                resolve_started,
                &mut log_lines,
                None,
                None,
                None,
                None,
                &[],
                &[],
            ));
        }
        native_stage_success(
            &mut log_lines,
            "resolve_config",
            resolve_started,
            &format!(
                "root={},python={},scripts=project_manager|quality_checker|total_md_split|finalize_svg|svg_to_pptx",
                root.display(),
                resolve_python_program(&root, &input.python_path).display()
            ),
        );

        let prompt = input.prompt.trim();
        let planning_context = build_generation_planning_context(&input, prompt);
        if prompt.is_empty() && !has_authoritative_generation_input(&input) {
            return Err(AppError::InvalidInput(
                "缺少结构化需求理解、规划上下文或原始语料".into(),
            ));
        }

        let requested_count = input.slide_count.unwrap_or(6).clamp(1, 30);
        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("AI PPT")
            .to_string();
        let style = input
            .style
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("科技蓝")
            .to_string();
        let understanding_for_theme = effective_understanding_draft(&input);
        let visual_suggestions = trimmed_option(&input.visual_suggestions)
            .or_else(|| trimmed_option(&input.visual_expression_advice))
            .or_else(|| {
                let value = understanding_for_theme.visual_expression_advice.trim();
                (!value.is_empty()).then_some(value)
            });
        let theme_spec = NativeThemeSpec::from_inputs(
            &style,
            input.custom_style.as_deref(),
            input.extra_requirements.as_deref(),
            visual_suggestions,
        );
        let style_mapping = resolve_style_mapping(&root, &style, &input, &theme_spec);
        let (input_fingerprint, state_model) = match build_native_input_fingerprint(
            db,
            &input,
            &planning_context,
            &title,
            requested_count,
            &style_mapping,
            &theme_spec,
        ) {
            Ok(value) => value,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "resolve_config",
                    started,
                    resolve_started,
                    &mut log_lines,
                    None,
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }
        };

        log_lines.push("[PPT Pipeline]".to_string());
        log_lines.push("engine=ppt_master_native".to_string());
        log_lines.push("[Style Mapping]".to_string());
        log_lines.push(format!("user_style={}", style_mapping.user_style));
        log_lines.push(format!("visual_style={}", style_mapping.visual_style));
        log_lines.push(format!("mode={}", style_mapping.mode));
        log_lines.push(format!(
            "layout_bias={}",
            style_mapping.layout_bias.join(",")
        ));
        log_lines.push(format!("chart_bias={}", style_mapping.chart_bias.join(",")));
        log_lines.push(format!(
            "[Native Theme] name={} primary={} secondary={} accent={} customStylePresent={} extraRequirementsPresent={}",
            theme_spec.theme_name,
            theme_spec.primary_color,
            theme_spec.secondary_color,
            theme_spec.accent_color,
            !theme_spec.source_custom_style.is_empty(),
            !theme_spec.source_extra_requirements.is_empty()
        ));
        log_planning_input(
            &input,
            prompt,
            &planning_context,
            requested_count,
            &style,
            &mut log_lines,
        );
        log_lines.push("[Domain Neutrality]".to_string());
        log_lines.push("hardcoded_domain_template=false".to_string());
        log_lines.push("page_count_template=false".to_string());
        log_lines.push("roadshow_template=false".to_string());

        println!("[Project] init/resume start");
        let prepare_started = native_stage_start(
            &mut log_lines,
            "prepare_project",
            &format!(
                "slideCount={requested_count},planningContextChars={}",
                planning_context.chars().count()
            ),
        );
        let (project, mut native_state, resumed) = match select_native_project(
            &root,
            &input.python_path,
            &title,
            requested_count,
            &input_fingerprint,
            state_model,
            forced_project,
            &mut log_lines,
        ) {
            Ok(value) => value,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "prepare_project",
                    started,
                    prepare_started,
                    &mut log_lines,
                    None,
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }
        };
        println!(
            "[Project] {} done: {}",
            if resumed { "resume" } else { "init" },
            project.display()
        );
        let sources = project.join("sources");
        let notes = project.join("notes");
        let svg_output = project.join("svg_output");
        let theme_spec_path = match persist_theme_spec(&project, &theme_spec) {
            Ok(path) => path,
            Err(error) => {
                native_state.set_status("failed");
                let _ = write_state_atomic(&project, &native_state);
                return Ok(native_failure_result(
                    AppError::Custom(error),
                    "prepare_project",
                    started,
                    prepare_started,
                    &mut log_lines,
                    Some(&project),
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }
        };
        if !resumed {
            for (path, content) in [
                (sources.join("confirmed_prompt.md"), prompt),
                (
                    sources.join("planning_context.md"),
                    planning_context.as_str(),
                ),
            ] {
                if let Err(error) = write_file(&path, content) {
                    native_state.set_status("failed");
                    let _ = write_state_atomic(&project, &native_state);
                    return Ok(native_failure_result(
                        error,
                        "prepare_project",
                        started,
                        prepare_started,
                        &mut log_lines,
                        Some(&project),
                        None,
                        None,
                        None,
                        &[],
                        &[],
                    ));
                }
            }
            if let Some(raw_material) = input.raw_material.as_deref() {
                if let Err(error) = write_file(&sources.join("raw_material.txt"), raw_material) {
                    native_state.set_status("failed");
                    let _ = write_state_atomic(&project, &native_state);
                    return Ok(native_failure_result(
                        error,
                        "prepare_project",
                        started,
                        prepare_started,
                        &mut log_lines,
                        Some(&project),
                        None,
                        None,
                        None,
                        &[],
                        &[],
                    ));
                }
            }
        }
        native_state.set_stage(if resumed {
            "resume_validate"
        } else {
            "build_design_spec"
        });
        if let Err(error) = persist_native_state(&project, &native_state) {
            return Ok(native_failure_result(
                error,
                "prepare_project",
                started,
                prepare_started,
                &mut log_lines,
                Some(&project),
                None,
                None,
                None,
                &[],
                &[],
            ));
        }
        native_stage_success(
            &mut log_lines,
            "prepare_project",
            prepare_started,
            &format!(
                "project={},resumed={},state={},themeSpec={}",
                project.display(),
                resumed,
                project.join(NATIVE_STATE_FILE).display(),
                theme_spec_path.display()
            ),
        );

        let design_started = native_stage_start(
            &mut log_lines,
            "build_design_spec",
            "SKILL.md,executor-base.md,shared-standards.md,planning_context.md",
        );
        let skill_text = match read_ppt_master_skill(&root) {
            Ok(value) => value,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "build_design_spec",
                    started,
                    design_started,
                    &mut log_lines,
                    Some(&project),
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }
        };
        let resources = match read_ppt_master_resources(&root) {
            Ok(value) => value,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "build_design_spec",
                    started,
                    design_started,
                    &mut log_lines,
                    Some(&project),
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }
        };
        let chart_catalog = load_chart_catalog(&root);

        let (plan, design_spec, design_spec_path, spec_lock_path, slide_plan_path) = if resumed
            && native_planning_artifacts_present(&project)
        {
            log_lines.push(format!(
                "[Native Resume] planningArtifacts=reuse fingerprint={} project={}",
                input_fingerprint,
                project.display()
            ));
            match load_native_planning_artifacts(&project, requested_count) {
                Ok(artifacts) => artifacts,
                Err(error) => {
                    native_state.set_status("failed");
                    let _ = persist_native_state(&project, &native_state);
                    return Ok(native_failure_result(
                        error,
                        "build_slide_plan",
                        started,
                        design_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&project.join("slide_plan.json")),
                        Some(&project.join("design_spec.md")),
                        None,
                        &[],
                        &[],
                    ));
                }
            }
        } else {
            println!("[AI] slide_plan start");
            log_lines.push("[AI] slide_plan start".to_string());
            let mut plan = match generate_native_structured_slide_plan(
                db,
                &input,
                &project,
                &input_fingerprint,
                &title,
                input.audience.as_deref().unwrap_or(""),
                requested_count,
                &style,
                &theme_spec,
                input.model_id,
                &mut log_lines,
            )
            .await
            {
                Ok(plan) => normalize_slide_plan(plan, &title, requested_count, &style),
                Err(error) => {
                    record_native_pipeline_failure(
                        &project,
                        &mut native_state,
                        "build_design_spec",
                    );
                    return Ok(native_failure_result(
                        error,
                        "build_design_spec",
                        started,
                        design_started,
                        &mut log_lines,
                        Some(&project),
                        None,
                        None,
                        None,
                        &[],
                        &[],
                    ));
                }
            };
            log_lines.push("[Fallback Plan] used=false strictNative=true".to_string());
            log_lines.push("[Slide Plan Source] source=ppt_master_native_ai".to_string());
            println!("[AI] slide_plan done");
            log_lines.push("[AI] slide_plan done".to_string());
            ensure_layout_variety(&mut plan);
            enrich_slide_execution_plan(&mut plan, &style_mapping, &chart_catalog);
            if let Some(duplicate_report) = detect_slide_plan_duplicates(&plan) {
                log_lines.push(format!(
                    "[Slide Plan] duplicate warning after per-page planning: {}; global replan=false",
                    duplicate_report
                ));
            }
            plan.style = style_mapping.user_style.clone();
            plan.theme = Theme {
                name: theme_spec.theme_name.clone(),
                primary: theme_spec.primary_color.clone(),
                secondary: theme_spec.secondary_color.clone(),
                accent: theme_spec.accent_color.clone(),
                background: theme_spec.background_color.clone(),
            };
            log_slide_plan_summary(&plan, &mut log_lines);

            if let Err(error) =
                copy_layout_templates(&root, &project, &style_mapping, &mut log_lines)
            {
                record_native_pipeline_failure(&project, &mut native_state, "build_design_spec");
                return Ok(native_failure_result(
                    error,
                    "build_design_spec",
                    started,
                    design_started,
                    &mut log_lines,
                    Some(&project),
                    None,
                    None,
                    None,
                    &[],
                    &[],
                ));
            }

            println!("[Spec] write design_spec/spec_lock");
            log_design_spec_pages(&plan, &style_mapping, &mut log_lines);
            let design_spec =
                build_ppt_master_design_spec(&plan, &planning_context, &style_mapping, &theme_spec);
            let design_spec_path = project.join("design_spec.md");
            if let Err(error) = write_file(&design_spec_path, &design_spec) {
                record_native_pipeline_failure(&project, &mut native_state, "build_design_spec");
                return Ok(native_failure_result(
                    error,
                    "build_design_spec",
                    started,
                    design_started,
                    &mut log_lines,
                    Some(&project),
                    None,
                    Some(&design_spec_path),
                    None,
                    &[],
                    &[],
                ));
            }
            native_stage_success(
                &mut log_lines,
                "build_design_spec",
                design_started,
                &format!("designSpec={}", design_spec_path.display()),
            );

            let spec_started = native_stage_start(
                &mut log_lines,
                "build_spec_lock",
                "design_spec.md,normalized native slide plan",
            );
            let spec_lock = build_ppt_master_spec_lock(&plan, &style_mapping, &theme_spec);
            let spec_lock_path = project.join("spec_lock.md");
            if let Err(error) = write_file(&spec_lock_path, &spec_lock) {
                record_native_pipeline_failure(&project, &mut native_state, "build_spec_lock");
                return Ok(native_failure_result(
                    error,
                    "build_spec_lock",
                    started,
                    spec_started,
                    &mut log_lines,
                    Some(&project),
                    None,
                    Some(&design_spec_path),
                    None,
                    &[],
                    &[],
                ));
            }
            native_stage_success(
                &mut log_lines,
                "build_spec_lock",
                spec_started,
                &format!("specLock={}", spec_lock_path.display()),
            );

            let plan_started = native_stage_start(
                &mut log_lines,
                "build_slide_plan",
                "normalized AI plan,design_spec.md,spec_lock.md",
            );
            let slide_plan_path = project.join("slide_plan.json");
            let plan_json = match serde_json::to_string_pretty(&plan) {
                Ok(value) => value,
                Err(error) => {
                    record_native_pipeline_failure(&project, &mut native_state, "build_slide_plan");
                    return Ok(native_failure_result(
                        AppError::Custom(format!("序列化 slide_plan 失败: {error}")),
                        "build_slide_plan",
                        started,
                        plan_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        None,
                        &[],
                        &[],
                    ));
                }
            };
            if let Err(error) = write_file(&slide_plan_path, &plan_json) {
                record_native_pipeline_failure(&project, &mut native_state, "build_slide_plan");
                return Ok(native_failure_result(
                    error,
                    "build_slide_plan",
                    started,
                    plan_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    None,
                    &[],
                    &[],
                ));
            }
            native_stage_success(
                &mut log_lines,
                "build_slide_plan",
                plan_started,
                &format!(
                    "slidePlan={},slides={}",
                    slide_plan_path.display(),
                    plan.slides.len()
                ),
            );
            (
                plan,
                design_spec,
                design_spec_path,
                spec_lock_path,
                slide_plan_path,
            )
        };

        update_native_state_plan_paths(&mut native_state, &project, &plan);
        #[cfg(debug_assertions)]
        if std::env::var("POME_NATIVE_DEBUG_PLANNING_ONLY").as_deref() == Ok("1") {
            native_state.set_stage("planning_validated");
            native_state.set_status("planning_validated");
            if let Err(error) = persist_native_state(&project, &native_state) {
                return Ok(native_failure_result(
                    error,
                    "build_slide_plan",
                    started,
                    design_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    None,
                    &[],
                    &[],
                ));
            }
            log_lines.push(
                "[Native Debug] planningOnly=true planningValidated=true svgGeneration=false fallback=false"
                    .to_string(),
            );
            return Ok(PptMasterGenerateResult {
                success: true,
                project_path: Some(project.to_string_lossy().to_string()),
                pptx_path: None,
                final_pptx_path: None,
                slide_plan_path: Some(slide_plan_path.to_string_lossy().to_string()),
                design_spec_path: Some(design_spec_path.to_string_lossy().to_string()),
                quality_check_passed: None,
                generation_mode: "agent".to_string(),
                exit_code: Some(0),
                stdout: log_lines.join("\n"),
                stderr: String::new(),
                duration_ms: started.elapsed().as_millis(),
                error: None,
                generation_engine: "ppt_master_native".to_string(),
                failure_stage: None,
                failure_type: None,
                failed_page: None,
                timed_out_after_seconds: None,
                failed_svg_file: None,
                stage: Some("planning_validated".to_string()),
                page_number: None,
                svg_path: None,
                violated_rule: None,
                checker_summary: None,
                intermediate_artifact_paths: native_intermediate_artifact_paths(&project),
            });
        }
        native_state.set_stage("execute_slides");
        if let Err(error) = persist_native_state(&project, &native_state) {
            return Ok(native_failure_result(
                error,
                "build_slide_plan",
                started,
                design_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                None,
                &[],
                &[],
            ));
        }

        println!("[SVG] generate start");
        let execute_started = native_stage_start(
            &mut log_lines,
            "execute_slides",
            &format!(
                "designSpec={},specLock={},slidePlan={},slides={}",
                design_spec_path.display(),
                spec_lock_path.display(),
                slide_plan_path.display(),
                plan.slides.len()
            ),
        );
        let reuse_preparation = match prepare_existing_native_pages(
            &root,
            &input.python_path,
            &project,
            &plan,
            &theme_spec,
            &mut native_state,
            &mut log_lines,
            started,
        ) {
            Ok(value) => value,
            Err(error) => {
                native_state.set_status("failed");
                let _ = persist_native_state(&project, &native_state);
                return Ok(native_failure_result(
                    error,
                    "execute_slides",
                    started,
                    execute_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    None,
                    &[],
                    &[],
                ));
            }
        };
        let mut downstream_invalidated = reuse_preparation.upstream_changed;
        let pages_to_generate: HashSet<usize> =
            native_pages_requiring_generation(&plan, &reuse_preparation.reusable_pages)
                .into_iter()
                .collect();
        log_lines.push(format!(
            "[Native Resume] reusablePages={} generatePages={}",
            sorted_page_list(&reuse_preparation.reusable_pages),
            sorted_page_list(&pages_to_generate)
        ));
        for idx in 0..plan.slides.len() {
            let slide = &plan.slides[idx];
            if !pages_to_generate.contains(&slide.page) {
                continue;
            }
            let prev_title = idx
                .checked_sub(1)
                .and_then(|i| plan.slides.get(i))
                .map(|s| s.title.as_str())
                .unwrap_or("");
            let next_title = plan
                .slides
                .get(idx + 1)
                .map(|s| s.title.as_str())
                .unwrap_or("");
            log_svg_page_task(slide, &style_mapping, &mut log_lines);
            let filename = svg_filename_for_slide(slide);
            let density_contract = NativePageDensityContract::for_slide(slide);
            let mut page_retry_feedback = native_state
                .pages
                .get(&slide.page.to_string())
                .and_then(|page| {
                    page.checker_summary.as_deref().map(|summary| {
                        format!(
                            "Previous strict validation failed. Repair only this page. violatedRule={}; detail={}",
                            page.violated_rule.as_deref().unwrap_or("unknown"),
                            summary
                        )
                    })
                });
            let mut geometry_repair_svg: Option<String> = None;
            let mut geometry_must_keep_text: Option<Vec<String>> = None;
            let mut density_relayout_svg: Option<String> = None;
            let mut density_must_keep_text: Option<Vec<String>> = None;
            if injected_native_failure_page() == Some(slide.page) {
                let error_text = format!(
                    "P{:02} 原生断点续跑验收注入失败；未调用 AI；strict native fallback=false",
                    slide.page
                );
                set_native_page_state(
                    &mut native_state,
                    slide.page,
                    "failed",
                    Some(error_text.clone()),
                    Some("test-injected interruption".to_string()),
                    None,
                    false,
                );
                native_state.set_status("failed");
                let _ = persist_native_state(&project, &native_state);
                return Ok(native_failure_result(
                    AppError::Custom(error_text),
                    "execute_slides",
                    started,
                    execute_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    None,
                    &[],
                    &[],
                ));
            }
            if !downstream_invalidated {
                if let Err(error) = invalidate_downstream(&project).map_err(AppError::Custom) {
                    native_state.set_status("failed");
                    let _ = persist_native_state(&project, &native_state);
                    return Ok(native_failure_result(
                        error,
                        "execute_slides",
                        started,
                        execute_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        None,
                        &[],
                        &[],
                    ));
                }
                native_state.artifacts.final_pptx_path = None;
                downstream_invalidated = true;
            }
            'page_attempt: loop {
                {
                    let page_state = native_state.page_mut(slide.page);
                    page_state.status = "generating".to_string();
                    page_state.attempts += 1;
                    page_state.last_error = None;
                    page_state.violated_rule = None;
                    page_state.checker_summary = None;
                    page_state.text_geometry = None;
                    page_state.reused = false;
                    page_state.updated_at = native_state_now();
                }
                if let Err(error) = persist_native_state(&project, &native_state) {
                    return Ok(native_failure_result(
                        error,
                        "execute_slides",
                        started,
                        execute_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        None,
                        &[],
                        &[],
                    ));
                }
                log_lines.push(format!(
                    "[Native Resume] page=P{:02} action={} aiCalled=true attempt={}",
                    slide.page,
                    if geometry_repair_svg.is_some() {
                        "repair-current-svg"
                    } else if density_relayout_svg.is_some() {
                        "relayout-current-svg"
                    } else {
                        "generate"
                    },
                    native_state.page_mut(slide.page).attempts
                ));
                let svg = match generate_ppt_master_driven_slide_svg(
                    db,
                    &skill_text,
                    &resources,
                    &design_spec,
                    &spec_lock_path,
                    &style_mapping,
                    &theme_spec,
                    &plan,
                    slide,
                    prev_title,
                    next_title,
                    input.model_id,
                    page_retry_feedback.as_deref(),
                    &density_contract,
                    geometry_repair_svg.as_deref(),
                    geometry_must_keep_text.as_deref(),
                    density_relayout_svg.as_deref(),
                    density_must_keep_text.as_deref(),
                )
                .await
                {
                    Ok(svg) => svg,
                    Err(error) => {
                        let error_text = format!("P{:02} 原生 SVG 生成失败: {error}", slide.page);
                        set_native_page_state(
                            &mut native_state,
                            slide.page,
                            "failed",
                            Some(error_text.clone()),
                            Some("AI SVG generation".to_string()),
                            None,
                            false,
                        );
                        native_state.set_status("failed");
                        let _ = persist_native_state(&project, &native_state);
                        return Ok(native_failure_result(
                            AppError::Custom(error_text),
                            "execute_slides",
                            started,
                            execute_started,
                            &mut log_lines,
                            Some(&project),
                            Some(&slide_plan_path),
                            Some(&design_spec_path),
                            None,
                            &[],
                            &[],
                        ));
                    }
                };
                let (svg, normalization) = normalize_native_svg_compatibility(&svg);
                if normalization != NativeSvgNormalizationReport::default() {
                    log_lines.push(format!(
                    "[SVG Compatibility] file={} rgbaColorsNormalized={} groupOpacityNormalized={} filtersRemoved={} malformedClosingTagsRepaired={} duplicateLineCoordinatesRepaired={} fallback=false",
                    filename,
                    normalization.rgba_colors_normalized,
                    normalization.group_opacity_normalized,
                    normalization.filters_removed,
                    normalization.malformed_closing_tags_repaired,
                    normalization.duplicate_line_coordinates_repaired
                ));
                }
                if let Err(error) = validate_native_svg_text(&filename, &svg) {
                    set_native_page_state(
                        &mut native_state,
                        slide.page,
                        "failed",
                        Some(error.to_string()),
                        Some("native SVG completeness/canvas".to_string()),
                        None,
                        false,
                    );
                    native_state.set_status("failed");
                    let _ = persist_native_state(&project, &native_state);
                    return Ok(native_failure_result(
                        error,
                        "execute_slides",
                        started,
                        execute_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        None,
                        &[],
                        &[],
                    ));
                }
                if let Err(error) = write_file(&svg_output.join(&filename), &svg) {
                    set_native_page_state(
                        &mut native_state,
                        slide.page,
                        "failed",
                        Some(error.to_string()),
                        Some("write native SVG".to_string()),
                        None,
                        false,
                    );
                    native_state.set_status("failed");
                    let _ = persist_native_state(&project, &native_state);
                    return Ok(native_failure_result(
                        error,
                        "execute_slides",
                        started,
                        execute_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        None,
                        &[],
                        &[],
                    ));
                }
                set_native_page_state(
                    &mut native_state,
                    slide.page,
                    "generated",
                    None,
                    None,
                    None,
                    false,
                );
                if let Err(error) = persist_native_state(&project, &native_state) {
                    return Ok(native_failure_result(
                        error,
                        "execute_slides",
                        started,
                        execute_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        None,
                        &[],
                        &[],
                    ));
                }
                let (mut page_failure, page_quality, page_geometry) =
                    match validate_generated_native_page(
                        &root,
                        &input.python_path,
                        &project,
                        &filename,
                        slide.page,
                        &theme_spec,
                        &density_contract,
                        &mut log_lines,
                        started,
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            let failure_stage = if error.to_string().contains("空间占用") {
                                "validate_space_utilization"
                            } else if error.to_string().contains("文本几何") {
                                "validate_text_geometry"
                            } else {
                                "validate_svgs"
                            };
                            set_native_page_state(
                                &mut native_state,
                                slide.page,
                                "failed",
                                Some(error.to_string()),
                                Some(format!("strict native page validation ({failure_stage})")),
                                None,
                                false,
                            );
                            native_state.set_status("failed");
                            let _ = persist_native_state(&project, &native_state);
                            return Ok(native_failure_result(
                                error,
                                failure_stage,
                                started,
                                execute_started,
                                &mut log_lines,
                                Some(&project),
                                Some(&slide_plan_path),
                                Some(&design_spec_path),
                                Some(false),
                                &[],
                                &[],
                            ));
                        }
                    };
                if page_failure.is_none() {
                    if let (Some(expected), Some(geometry)) =
                        (geometry_must_keep_text.as_ref(), page_geometry.as_ref())
                    {
                        let actual = geometry.visible_texts();
                        if !native_page_relayout_preserved_visible_text(expected, &actual) {
                            page_failure = Some(NativePageValidationFailure {
                                stage: "validate_text_geometry".to_string(),
                                violated_rule: "visible_text_changed_during_geometry_repair"
                                    .to_string(),
                                checker_summary: format!(
                                    "AI local geometry repair changed visible text; expected={}; actual={}",
                                    serde_json::to_string(expected).unwrap_or_default(),
                                    serde_json::to_string(&actual).unwrap_or_default()
                                ),
                                text_geometry: Some(geometry.state()),
                            });
                        }
                    }
                }
                if page_failure.is_none() {
                    if let (Some(expected), Some(geometry)) =
                        (density_must_keep_text.as_ref(), page_geometry.as_ref())
                    {
                        let actual = geometry.visible_texts();
                        if !native_page_relayout_preserved_visible_text(expected, &actual) {
                            page_failure = Some(NativePageValidationFailure {
                                stage: "validate_space_utilization".to_string(),
                                violated_rule: "visible_text_changed_during_density_relayout"
                                    .to_string(),
                                checker_summary: format!(
                                    "AI local density relayout changed visible text; expected={}; actual={}",
                                    serde_json::to_string(expected).unwrap_or_default(),
                                    serde_json::to_string(&actual).unwrap_or_default()
                                ),
                                text_geometry: Some(geometry.state()),
                            });
                        }
                    }
                }
                if let Some(geometry) = page_geometry.as_ref() {
                    set_native_text_geometry_state(&mut native_state, slide.page, geometry.state());
                    log_lines.push(format!(
                    "[Text Geometry] page=P{:02} passed={} hardErrors={} warnings={} autoFixedBlocks={} svg={}",
                    slide.page,
                    geometry.passed,
                    geometry.hard_errors.len(),
                    geometry.warnings.len(),
                    geometry.auto_fix_applied.len(),
                    filename
                ));
                }
                if let Some(failure) = page_failure {
                    let failure_stage = failure.stage.clone();
                    let violated_rule = failure.violated_rule.clone();
                    let checker_summary = failure.checker_summary.clone();
                    if let Some(geometry) = failure.text_geometry.clone() {
                        set_native_text_geometry_state(&mut native_state, slide.page, geometry);
                    }
                    set_native_page_state(
                        &mut native_state,
                        slide.page,
                        "failed",
                        Some(failure.checker_summary.clone()),
                        Some(failure.violated_rule.clone()),
                        Some(failure.checker_summary.clone()),
                        false,
                    );
                    native_state.current_stage = failure_stage.clone();
                    let page_attempts = native_state
                        .pages
                        .get(&slide.page.to_string())
                        .map(|page| page.attempts)
                        .unwrap_or_default();
                    if native_page_validation_may_retry(&failure_stage, page_attempts) {
                        native_state.set_stage("execute_slides");
                        persist_native_state(&project, &native_state)?;
                        page_retry_feedback = Some(if failure_stage == "validate_svgs" {
                            format!(
                                "Page P{:02} strict SVG compatibility hard error. violatedRule={}; detail={}. Repair only this page. {} Do not remove core content and do not use fallback.",
                                slide.page,
                                violated_rule,
                                checker_summary,
                                NATIVE_EXECUTOR_CLIP_PATH_RULE
                            )
                        } else if failure_stage == "validate_theme_consistency" {
                            format!(
                                "Page P{:02} does not follow the immutable deck-wide NativeThemeSpec. {} Regenerate only this page using the exact allowed palette and shared decoration/shape language. Do not mechanically recolor all elements; preserve readable text contrast. Do not use any forbidden color or fallback.",
                                slide.page, checker_summary
                            )
                        } else if failure_stage == "validate_visible_text_integrity" {
                            format!(
                                "Page P{:02} contains damaged visible text. {} Regenerate only this page. Preserve the SlideSpec meaning, emit valid UTF-8 Chinese, and do not expose Markdown heading markers. Do not use fallback.",
                                slide.page, checker_summary
                            )
                        } else if failure_stage == "validate_space_utilization" {
                            density_relayout_svg =
                                fs::read_to_string(project.join("svg_output").join(&filename)).ok();
                            density_must_keep_text = page_geometry
                                .as_ref()
                                .map(NativeTextGeometryReport::visible_texts);
                            geometry_repair_svg = None;
                            geometry_must_keep_text = None;
                            format!(
                                "Page P{:02} has theme-independent dead whitespace or insufficient functional occupancy. {} Relayout only this page once. Keep every current visible text string and every mustInclude fact, redistribute them across the effective canvas, and use semantic supporting structure without adding facts, repeating text, empty cards, or changing the NativeThemeSpec.",
                                slide.page, checker_summary
                            )
                        } else if let Some(geometry) = page_geometry.as_ref() {
                            geometry_repair_svg =
                                fs::read_to_string(project.join("svg_output").join(&filename)).ok();
                            geometry_must_keep_text = Some(geometry.visible_texts());
                            density_relayout_svg = None;
                            density_must_keep_text = None;
                            serde_json::to_string(&geometry.repair_context())
                                .unwrap_or_else(|_| checker_summary.clone())
                        } else {
                            checker_summary.clone()
                        });
                        if (failure_stage != "validate_text_geometry"
                            || geometry_repair_svg.is_some())
                            && (failure_stage != "validate_space_utilization"
                                || density_relayout_svg.is_some())
                        {
                            log_lines.push(format!(
                                "[Native Page Retry] page=P{:02} stage={} action={} nextAttempt={} otherPagesRegenerated=false fallback=false violatedRule={}",
                                slide.page,
                                failure_stage,
                                if failure_stage == "validate_text_geometry" {
                                    "repair-current-svg-only"
                                } else if failure_stage == "validate_space_utilization" {
                                    "relayout-current-svg-only"
                                } else {
                                    "regenerate-page-only"
                                },
                                page_attempts + 1,
                                violated_rule
                            ));
                            continue 'page_attempt;
                        }
                        log_lines.push(format!(
                            "[Native Page Retry] page=P{:02} stage={} action=abort-local-repair svgRead=false fallback=false",
                            slide.page, failure_stage
                        ));
                    }
                    let last_svg_path = svg_output.join(&filename);
                    if should_continue_after_quality_failure(
                        block_on_quality_failure,
                        &last_svg_path,
                    ) {
                        quality_check_passed = false;
                        set_native_page_state(
                            &mut native_state,
                            slide.page,
                            "generated",
                            Some(failure.checker_summary.clone()),
                            Some(failure.violated_rule.clone()),
                            Some(failure.checker_summary.clone()),
                            false,
                        );
                        native_state.set_stage("execute_slides");
                        persist_native_state(&project, &native_state)?;
                        log_lines.push(format!(
                            "[Quality Check] page=P{:02} passed=false blockOnQualityFailure=false action=keep-last-svg-and-continue svg={} fallback=false",
                            slide.page,
                            last_svg_path.display()
                        ));
                        break 'page_attempt;
                    }
                    native_state.set_status("failed");
                    let _ = persist_native_state(&project, &native_state);
                    let quality_failure = NativeQualityFailure {
                        page_number: Some(slide.page),
                        file_name: filename.clone(),
                        violated_rule: failure.violated_rule,
                        checker_summary: failure.checker_summary,
                    };
                    let result = native_failure_result(
                        AppError::Custom(native_page_validation_error_message(
                            &project,
                            &failure_stage,
                            &quality_failure,
                        )),
                        &failure_stage,
                        started,
                        execute_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        Some(false),
                        std::slice::from_ref(&page_quality.stdout),
                        std::slice::from_ref(&page_quality.stderr),
                    );
                    return Ok(with_native_quality_failure(
                        result,
                        &failure_stage,
                        &project,
                        Some(slide.page),
                        &filename,
                        &quality_failure.violated_rule,
                        &quality_failure.checker_summary,
                    ));
                }
                set_native_page_state(
                    &mut native_state,
                    slide.page,
                    "validated",
                    None,
                    None,
                    None,
                    false,
                );
                persist_native_state(&project, &native_state)?;
                log_lines.push(format!("写入 SVG: svg_output/{}", filename));
                break 'page_attempt;
            }
        }
        let svg_set_validation = if block_on_quality_failure {
            validate_native_svg_set(&plan, &svg_output)
        } else {
            ensure_native_svg_files_exist(&plan, &svg_output)
        };
        if let Err(error) = svg_set_validation {
            native_state.set_status("failed");
            let _ = persist_native_state(&project, &native_state);
            return Ok(native_failure_result(
                error,
                "execute_slides",
                started,
                execute_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                None,
                &[],
                &[],
            ));
        }
        native_stage_success(
            &mut log_lines,
            "execute_slides",
            execute_started,
            &format!(
                "svgOutput={},pages={}",
                svg_output.display(),
                plan.slides.len()
            ),
        );

        let validate_started = native_stage_start(
            &mut log_lines,
            "validate_svgs",
            &format!("svgOutput={}", svg_output.display()),
        );
        native_state.set_stage("validate_svgs");
        if let Err(error) = persist_native_state(&project, &native_state) {
            return Ok(native_failure_result(
                error,
                "validate_svgs",
                started,
                validate_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                None,
                &[],
                &[],
            ));
        }
        log_lines.push("[Check] svg_quality_checker start".to_string());
        let quality = match run_quality_check(&root, &input.python_path, &project, started) {
            Ok(result) => result,
            Err(error) if !block_on_quality_failure => {
                quality_check_passed = false;
                log_lines.push(format!(
                    "[Quality Check] stage=validate_svgs passed=false blockOnQualityFailure=false action=continue-to-export checkerError={} fallback=false",
                    single_line_log_value(&error.to_string())
                ));
                PptMasterExportResult {
                    success: false,
                    output_path: None,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                    duration_ms: started.elapsed().as_millis(),
                    error: Some(error.to_string()),
                }
            }
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "validate_svgs",
                    started,
                    validate_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    None,
                    &[],
                    &[],
                ));
            }
        };
        log_lines.push(format!(
            "[Check] svg_quality_checker done: success={}",
            quality.success
        ));
        if !quality.success {
            quality_check_passed = false;
            let failures = parse_native_quality_failures(&quality.stdout, &quality.stderr);
            let first_failure = failures
                .first()
                .cloned()
                .unwrap_or_else(|| NativeQualityFailure {
                    page_number: None,
                    file_name: "unknown.svg".to_string(),
                    violated_rule: "SVG Quality Checker hard error".to_string(),
                    checker_summary: single_line_log_value(&quality.stdout),
                });
            if block_on_quality_failure {
                for failure in &failures {
                    if let Some(page) = failure.page_number {
                        set_native_page_state(
                            &mut native_state,
                            page,
                            "failed",
                            Some(failure.checker_summary.clone()),
                            Some(failure.violated_rule.clone()),
                            Some(failure.checker_summary.clone()),
                            false,
                        );
                    }
                }
                native_state.set_status("failed");
                let _ = persist_native_state(&project, &native_state);
                let result = native_failure_result(
                    AppError::Custom(native_quality_error_message(&project, &first_failure)),
                    "validate_svgs",
                    started,
                    validate_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(false),
                    std::slice::from_ref(&quality.stdout),
                    std::slice::from_ref(&quality.stderr),
                );
                return Ok(with_native_quality_failure(
                    result,
                    "validate_svgs",
                    &project,
                    first_failure.page_number,
                    &first_failure.file_name,
                    &first_failure.violated_rule,
                    &first_failure.checker_summary,
                ));
            }
            for failure in &failures {
                if let Some(page) = failure.page_number {
                    set_native_page_state(
                        &mut native_state,
                        page,
                        "generated",
                        Some(failure.checker_summary.clone()),
                        Some(failure.violated_rule.clone()),
                        Some(failure.checker_summary.clone()),
                        false,
                    );
                }
            }
            persist_native_state(&project, &native_state)?;
            log_lines.push(format!(
                "[Quality Check] stage=validate_svgs passed=false blockOnQualityFailure=false action=continue-to-export firstRule={} fallback=false",
                first_failure.violated_rule
            ));
        } else {
            log_lines.push("[Repair] skipped: strict native validation passed".to_string());
        }

        let native_issues = match scan_native_incompatible_svgs(&svg_output) {
            Ok(issues) => issues,
            Err(error) if !block_on_quality_failure => {
                quality_check_passed = false;
                log_lines.push(format!(
                    "[Quality Check] stage=scan_native_compatibility passed=false blockOnQualityFailure=false action=continue-to-export error={} fallback=false",
                    single_line_log_value(&error.to_string())
                ));
                Vec::new()
            }
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "validate_svgs",
                    started,
                    validate_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    std::slice::from_ref(&quality.stdout),
                    std::slice::from_ref(&quality.stderr),
                ));
            }
        };
        if !native_issues.is_empty() {
            quality_check_passed = false;
            if !block_on_quality_failure {
                log_lines.push(format!(
                    "[Quality Check] stage=scan_native_compatibility passed=false blockOnQualityFailure=false action=continue-to-export issues={} fallback=false",
                    single_line_log_value(&summarize_native_issues(&native_issues))
                ));
            } else {
                return Ok(native_failure_result(
                    AppError::Custom(format!(
                        "原生 SVG 包含 DrawingML 导出不支持的元素: {}",
                        summarize_native_issues(&native_issues)
                    )),
                    "validate_svgs",
                    started,
                    validate_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    std::slice::from_ref(&quality.stdout),
                    std::slice::from_ref(&quality.stderr),
                ));
            }
        }
        let text_issues = match scan_final_text_leaks(&svg_output) {
            Ok(issues) => issues,
            Err(error) if !block_on_quality_failure => {
                quality_check_passed = false;
                log_lines.push(format!(
                    "[Quality Check] stage=scan_visible_text passed=false blockOnQualityFailure=false action=continue-to-export error={} fallback=false",
                    single_line_log_value(&error.to_string())
                ));
                Vec::new()
            }
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "validate_svgs",
                    started,
                    validate_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    std::slice::from_ref(&quality.stdout),
                    std::slice::from_ref(&quality.stderr),
                ));
            }
        };
        if !text_issues.is_empty() {
            quality_check_passed = false;
            if !block_on_quality_failure {
                log_lines.push(format!(
                    "[Quality Check] stage=scan_visible_text passed=false blockOnQualityFailure=false action=continue-to-export issues={} fallback=false",
                    single_line_log_value(&summarize_final_text_issues(&text_issues))
                ));
            } else {
                return Ok(native_failure_result(
                    AppError::Custom(format!(
                        "原生 SVG 检测到内部字段或模板词泄漏: {}",
                        summarize_final_text_issues(&text_issues)
                    )),
                    "validate_svgs",
                    started,
                    validate_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    std::slice::from_ref(&quality.stdout),
                    std::slice::from_ref(&quality.stderr),
                ));
            }
        }
        if quality_check_passed {
            for slide in &plan.slides {
                let reused = native_state
                    .pages
                    .get(&slide.page.to_string())
                    .is_some_and(|page| page.reused);
                set_native_page_state(
                    &mut native_state,
                    slide.page,
                    "validated",
                    None,
                    None,
                    None,
                    reused,
                );
            }
        }
        native_state.set_stage("generate_notes");
        if let Err(error) = persist_native_state(&project, &native_state) {
            return Ok(native_failure_result(
                error,
                "validate_svgs",
                started,
                validate_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                Some(true),
                std::slice::from_ref(&quality.stdout),
                std::slice::from_ref(&quality.stderr),
            ));
        }
        native_stage_success(
            &mut log_lines,
            "validate_svgs",
            validate_started,
            &format!(
                "qualityPassed={},svgOutput={}",
                quality_check_passed,
                svg_output.display()
            ),
        );

        let notes_started = native_stage_start(
            &mut log_lines,
            "generate_notes",
            &format!("slidePlan={}", slide_plan_path.display()),
        );
        let total_notes_path = notes.join("total.md");
        if let Err(error) = write_file(&total_notes_path, &build_notes(&plan)) {
            return Ok(native_failure_result(
                error,
                "generate_notes",
                started,
                notes_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                Some(true),
                std::slice::from_ref(&quality.stdout),
                std::slice::from_ref(&quality.stderr),
            ));
        }
        let split = match run_total_md_split(&root, &input.python_path, &project, started) {
            Ok(result) => result,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "generate_notes",
                    started,
                    notes_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    std::slice::from_ref(&quality.stdout),
                    std::slice::from_ref(&quality.stderr),
                ));
            }
        };
        if !split.success {
            return Ok(native_failure_result(
                AppError::Custom(
                    split
                        .error
                        .clone()
                        .unwrap_or_else(|| "total_md_split.py 执行失败".to_string()),
                ),
                "generate_notes",
                started,
                notes_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                Some(true),
                &[quality.stdout.clone(), split.stdout.clone()],
                &[quality.stderr.clone(), split.stderr.clone()],
            ));
        }
        native_stage_success(
            &mut log_lines,
            "generate_notes",
            notes_started,
            &format!("notes={}", total_notes_path.display()),
        );
        native_state.set_stage("export_pptx");
        native_state.artifacts.notes_path = notes.to_string_lossy().to_string();
        if let Err(error) = persist_native_state(&project, &native_state) {
            return Ok(native_failure_result(
                error,
                "generate_notes",
                started,
                notes_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                Some(true),
                &[quality.stdout.clone(), split.stdout.clone()],
                &[quality.stderr.clone(), split.stderr.clone()],
            ));
        }

        println!("[Export] start");
        let export_started = native_stage_start(
            &mut log_lines,
            "export_pptx",
            &format!("project={},svgSource=svg_output", project.display()),
        );
        log_lines.push("[Finalize] command: finalize_svg.py <project>".to_string());
        let finalize = match run_finalize_svg(&root, &input.python_path, &project, started) {
            Ok(result) => result,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "export_pptx",
                    started,
                    export_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    &[quality.stdout.clone(), split.stdout.clone()],
                    &[quality.stderr.clone(), split.stderr.clone()],
                ));
            }
        };
        if !finalize.success {
            return Ok(native_failure_result(
                AppError::Custom(
                    finalize
                        .error
                        .clone()
                        .unwrap_or_else(|| "finalize_svg.py 执行失败".to_string()),
                ),
                "export_pptx",
                started,
                export_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                Some(true),
                &[
                    quality.stdout.clone(),
                    split.stdout.clone(),
                    finalize.stdout.clone(),
                ],
                &[
                    quality.stderr.clone(),
                    split.stderr.clone(),
                    finalize.stderr.clone(),
                ],
            ));
        }

        log_lines.push("[Export] svg source: svg_output".to_string());
        log_lines.push(
            "[Export] command: svg_to_pptx.py <project> (native default source=svg_output)"
                .to_string(),
        );
        let export = match export_project(&root, &input.python_path, &project, started) {
            Ok(result) => result,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "export_pptx",
                    started,
                    export_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    &[
                        quality.stdout.clone(),
                        split.stdout.clone(),
                        finalize.stdout.clone(),
                    ],
                    &[
                        quality.stderr.clone(),
                        split.stderr.clone(),
                        finalize.stderr.clone(),
                    ],
                ));
            }
        };
        let pptx_path = match validate_native_export_result(&export) {
            Ok(path) => path,
            Err(error) => {
                return Ok(native_failure_result(
                    error,
                    "export_pptx",
                    started,
                    export_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    &[
                        quality.stdout.clone(),
                        split.stdout.clone(),
                        finalize.stdout.clone(),
                        export.stdout.clone(),
                    ],
                    &[
                        quality.stderr.clone(),
                        split.stderr.clone(),
                        finalize.stderr.clone(),
                        export.stderr.clone(),
                    ],
                ));
            }
        };

        let powerpoint_geometry_started = native_stage_start(
            &mut log_lines,
            "validate_powerpoint_text_geometry",
            &format!("pptx={},svgSource=svg_output", pptx_path.display()),
        );
        native_state.set_stage("validate_powerpoint_text_geometry");
        persist_native_state(&project, &native_state)?;
        let powerpoint_geometry = match run_native_powerpoint_geometry_check(&project, &pptx_path) {
            Ok(report) => Some(report),
            Err(error) if !block_on_quality_failure => {
                quality_check_passed = false;
                log_lines.push(format!(
                    "[Quality Check] stage=validate_powerpoint_text_geometry passed=false blockOnQualityFailure=false action=keep-exported-pptx checkerError={} fallback=false",
                    single_line_log_value(&error.to_string())
                ));
                None
            }
            Err(error) => {
                native_state.set_status("failed");
                let _ = persist_native_state(&project, &native_state);
                return Ok(native_failure_result(
                    error,
                    "validate_powerpoint_text_geometry",
                    started,
                    powerpoint_geometry_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(true),
                    &[export.stdout.clone()],
                    &[export.stderr.clone()],
                ));
            }
        };
        if let Some(powerpoint_geometry) = powerpoint_geometry.as_ref() {
            log_lines.push(format!(
            "[PowerPoint Text Geometry] passed={} hardErrors={} warnings={} safeRegionFixes={} renderDir={}",
            powerpoint_geometry.passed,
            powerpoint_geometry.hard_errors.len(),
            powerpoint_geometry.warnings.len(),
            powerpoint_geometry.safe_fixes.len(),
            powerpoint_geometry
                .render_dir
                .as_deref()
                .unwrap_or("unknown")
        ));
            if !powerpoint_geometry.passed {
                quality_check_passed = false;
            }
            if !powerpoint_geometry.passed && block_on_quality_failure {
                for slide in &plan.slides {
                    let issues = powerpoint_geometry.issues_for_page(slide.page);
                    if issues.is_empty() {
                        continue;
                    }
                    let rule = issues
                        .first()
                        .and_then(|issue| issue.get("rule"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("powerpoint_text_geometry_failed")
                        .to_string();
                    let summary = format!(
                        "PowerPoint actual text bounds failed: {}",
                        serde_json::Value::Array(issues)
                    );
                    set_native_page_state(
                        &mut native_state,
                        slide.page,
                        "failed",
                        Some(summary.clone()),
                        Some(rule),
                        Some(summary),
                        false,
                    );
                }
                native_state.current_stage = "validate_powerpoint_text_geometry".to_string();
                native_state.set_status("failed");
                native_state.artifacts.final_pptx_path = None;
                invalidate_downstream(&project).map_err(AppError::Custom)?;
                persist_native_state(&project, &native_state)?;
                let failed_page = powerpoint_geometry.first_page();
                let failed_file = failed_page
                    .and_then(|page| plan.slides.iter().find(|slide| slide.page == page))
                    .map(svg_filename_for_slide)
                    .unwrap_or_else(|| "unknown.svg".to_string());
                let quality_failure = NativeQualityFailure {
                    page_number: failed_page,
                    file_name: failed_file.clone(),
                    violated_rule: powerpoint_geometry.first_rule(),
                    checker_summary: powerpoint_geometry.summary(),
                };
                let result = native_failure_result(
                    AppError::Custom(native_page_validation_error_message(
                        &project,
                        "validate_powerpoint_text_geometry",
                        &quality_failure,
                    )),
                    "validate_powerpoint_text_geometry",
                    started,
                    powerpoint_geometry_started,
                    &mut log_lines,
                    Some(&project),
                    Some(&slide_plan_path),
                    Some(&design_spec_path),
                    Some(false),
                    &[export.stdout.clone()],
                    &[export.stderr.clone()],
                );
                return Ok(with_native_quality_failure(
                    result,
                    "validate_powerpoint_text_geometry",
                    &project,
                    failed_page,
                    &failed_file,
                    &quality_failure.violated_rule,
                    &quality_failure.checker_summary,
                ));
            }
            if !powerpoint_geometry.passed {
                log_lines.push(
                "[Quality Check] stage=validate_powerpoint_text_geometry passed=false blockOnQualityFailure=false action=keep-exported-pptx fallback=false"
                    .to_string(),
            );
            }
        }
        let powerpoint_render_dir = powerpoint_geometry
            .as_ref()
            .and_then(|report| report.render_dir.as_deref())
            .unwrap_or("unknown");
        native_stage_success(
            &mut log_lines,
            "validate_powerpoint_text_geometry",
            powerpoint_geometry_started,
            &format!(
                "pptx={},renderDir={powerpoint_render_dir}",
                pptx_path.display()
            ),
        );

        let export_title = plan.title.clone();
        let final_pptx_path = match input.output_dir.as_deref() {
            Some(dir) => match copy_final_pptx(&pptx_path, dir, &export_title) {
                Ok(path) => Some(path.to_string_lossy().to_string()),
                Err(error) => {
                    return Ok(native_failure_result(
                        AppError::Custom(format!("PPTX 已生成，但复制到导出文件夹失败: {error}")),
                        "export_pptx",
                        started,
                        export_started,
                        &mut log_lines,
                        Some(&project),
                        Some(&slide_plan_path),
                        Some(&design_spec_path),
                        Some(true),
                        &[
                            quality.stdout.clone(),
                            split.stdout.clone(),
                            finalize.stdout.clone(),
                            export.stdout.clone(),
                        ],
                        &[
                            quality.stderr.clone(),
                            split.stderr.clone(),
                            finalize.stderr.clone(),
                            export.stderr.clone(),
                        ],
                    ));
                }
            },
            None => None,
        };
        native_stage_success(
            &mut log_lines,
            "export_pptx",
            export_started,
            &format!("pptx={}", pptx_path.display()),
        );
        native_state.current_stage = "completed".to_string();
        native_state.artifacts.final_pptx_path = Some(pptx_path.to_string_lossy().to_string());
        native_state.set_status("completed");
        if let Err(error) = persist_native_state(&project, &native_state) {
            return Ok(native_failure_result(
                error,
                "completed",
                started,
                export_started,
                &mut log_lines,
                Some(&project),
                Some(&slide_plan_path),
                Some(&design_spec_path),
                Some(true),
                &[
                    quality.stdout.clone(),
                    split.stdout.clone(),
                    finalize.stdout.clone(),
                ],
                &[
                    quality.stderr.clone(),
                    split.stderr.clone(),
                    finalize.stderr.clone(),
                ],
            ));
        }

        let completed_started = native_stage_start(
            &mut log_lines,
            "completed",
            &format!("project={},pptx={}", project.display(), pptx_path.display()),
        );
        native_stage_success(
            &mut log_lines,
            "completed",
            completed_started,
            &format!(
                "strictNative=true,blockOnQualityFailure={},qualityCheckPassed={},fallbackUsed=false",
                block_on_quality_failure, quality_check_passed
            ),
        );
        println!("[Done] pptx={}", pptx_path.display());

        Ok(PptMasterGenerateResult {
            success: true,
            project_path: Some(project.to_string_lossy().to_string()),
            pptx_path: Some(pptx_path.to_string_lossy().to_string()),
            final_pptx_path,
            slide_plan_path: Some(slide_plan_path.to_string_lossy().to_string()),
            design_spec_path: Some(design_spec_path.to_string_lossy().to_string()),
            quality_check_passed: Some(quality_check_passed),
            generation_mode: "agent".to_string(),
            exit_code: export.exit_code,
            stdout: join_outputs(
                &log_lines,
                &[quality.stdout, split.stdout, finalize.stdout, export.stdout],
            ),
            stderr: join_outputs(
                &[],
                &[quality.stderr, split.stderr, finalize.stderr, export.stderr],
            ),
            duration_ms: started.elapsed().as_millis(),
            error: None,
            generation_engine: "ppt_master_native".to_string(),
            failure_stage: None,
            failure_type: None,
            failed_page: None,
            timed_out_after_seconds: None,
            failed_svg_file: None,
            stage: None,
            page_number: None,
            svg_path: None,
            violated_rule: None,
            checker_summary: None,
            intermediate_artifact_paths: native_intermediate_artifact_paths(&project),
        })
    }
}

fn build_native_input_fingerprint(
    db: &Database,
    input: &PptMasterGenerateInput,
    planning_context: &str,
    title: &str,
    slide_count: usize,
    style_mapping: &PptMasterStyleMapping,
    theme_spec: &NativeThemeSpec,
) -> Result<(String, NativeStateModel), AppError> {
    let model = match input.model_id {
        Some(id) => db.get_ai_model(id)?,
        None => db.get_default_ai_model()?,
    };
    let mut understanding =
        format_understanding_draft(&effective_understanding_draft(input)).unwrap_or_default();
    if let Some(PptUnderstandingInput::Legacy(value)) = input.ai_understanding_result.as_ref() {
        understanding.push_str("\nlegacyUnderstanding:\n");
        understanding.push_str(value);
    }
    if !input.material_sources.is_empty() {
        understanding.push_str("\nmaterialSources:\n");
        for source in &input.material_sources {
            understanding.push_str(&format!(
                "{}|{}|{}\n",
                source.id, source.source_type, source.title
            ));
        }
    }
    let effective_max_output_tokens = model
        .max_output_tokens
        .filter(|value| *value > 0)
        .map(|value| value.min(NATIVE_GENERATION_MAX_OUTPUT_TOKENS))
        .unwrap_or(NATIVE_GENERATION_MAX_OUTPUT_TOKENS);
    let fingerprint_input = NativeFingerprintInput {
        topic: title.to_string(),
        prompt: input.prompt.clone(),
        planning_context: planning_context.to_string(),
        raw_material: input.raw_material.clone().unwrap_or_default(),
        understanding,
        extra_requirements: input.extra_requirements.clone().unwrap_or_default(),
        audience: input.audience.clone().unwrap_or_default(),
        slide_count,
        style: style_mapping.user_style.clone(),
        custom_style: input.custom_style.clone().unwrap_or_default(),
        visual_suggestions: theme_spec.source_visual_suggestions.clone(),
        theme_spec: theme_spec.prompt_contract(),
        mode: style_mapping.mode.clone(),
        visual_style: style_mapping.visual_style.clone(),
        layout_bias: style_mapping.layout_bias.clone(),
        chart_bias: style_mapping.chart_bias.clone(),
        model_database_id: model.id,
        model_provider: model.provider.clone(),
        model_id: model.model_id.clone(),
        generation_mode: "agent".to_string(),
        generation_engine: "ppt_master_native".to_string(),
        generation_spec_version: NATIVE_GENERATION_SPEC_VERSION.to_string(),
        canvas: NATIVE_CANVAS.to_string(),
        max_output_tokens: effective_max_output_tokens,
        timeout_seconds: NATIVE_AI_TIMEOUT_SECS,
    };
    let fingerprint = fingerprint_input.fingerprint().map_err(AppError::Custom)?;
    Ok((
        fingerprint,
        NativeStateModel {
            database_id: model.id,
            provider: model.provider,
            model_id: model.model_id,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn runtime_path_without_windows_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(local) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(local);
        }
    }
    path.to_path_buf()
}

fn select_native_project(
    root: &Path,
    python_path: &str,
    title: &str,
    slide_count: usize,
    input_fingerprint: &str,
    model: NativeStateModel,
    forced_project: Option<PathBuf>,
    log_lines: &mut Vec<String>,
) -> Result<(PathBuf, NativeGenerationState, bool), AppError> {
    if let Some(project) = forced_project {
        let canonical_project = project.canonicalize().map_err(|error| {
            AppError::Custom(format!(
                "显式续跑项目不存在: {} ({error})",
                project.display()
            ))
        })?;
        let projects_root = root.join("projects").canonicalize().map_err(|error| {
            AppError::Custom(format!(
                "ppt-master projects 目录不可用: {} ({error})",
                root.join("projects").display()
            ))
        })?;
        if !canonical_project.starts_with(&projects_root) {
            return Err(AppError::InvalidInput(format!(
                "显式续跑项目不在 ppt-master/projects 内: {}",
                canonical_project.display()
            )));
        }
        let runtime_project = runtime_path_without_windows_verbatim_prefix(&canonical_project);
        let mut state = if runtime_project.join(NATIVE_STATE_FILE).is_file() {
            let state = read_state(&runtime_project).map_err(AppError::Custom)?;
            if state.input_fingerprint != input_fingerprint {
                return Err(AppError::InvalidInput(format!(
                    "原生断点输入指纹不一致，禁止混用旧页面: project={}, cached={}, current={}",
                    runtime_project.display(),
                    state.input_fingerprint,
                    input_fingerprint
                )));
            }
            state
        } else {
            log_lines.push(format!(
                "[Native Resume] bootstrapState=true project={} reason=pre-state-project",
                runtime_project.display()
            ));
            NativeGenerationState::new(
                input_fingerprint.to_string(),
                title.to_string(),
                slide_count,
                model,
                &runtime_project,
            )
        };
        state.set_stage("resume_validate");
        persist_native_state(&runtime_project, &state)?;
        return Ok((runtime_project, state, true));
    }

    let force_new_debug_project = cfg!(debug_assertions)
        && std::env::var("POME_NATIVE_DEBUG_FORCE_NEW_PROJECT").as_deref() == Ok("1");
    if force_new_debug_project {
        log_lines.push(
            "[Native Debug] forceNewProject=true resumeScanSkipped=true fallback=false".to_string(),
        );
        let project = init_project_with_project_manager(root, python_path, title, log_lines)?;
        let state = NativeGenerationState::new(
            input_fingerprint.to_string(),
            title.to_string(),
            slide_count,
            model,
            &project,
        );
        persist_native_state(&project, &state)?;
        log_lines.push(format!(
            "[Native Resume] matched=false fingerprint={} action=create-new-debug-project",
            input_fingerprint
        ));
        return Ok((project, state, false));
    }

    let (matching, warnings) =
        find_matching_resume_project(root, input_fingerprint).map_err(AppError::Custom)?;
    log_lines.extend(warnings);
    if let Some((project, mut state)) = matching {
        if state.slide_count != slide_count || state.model != model {
            return Err(AppError::InvalidInput(format!(
                "原生断点元数据与当前输入不一致，禁止复用: {}",
                project.display()
            )));
        }
        if native_planning_artifacts_present(&project)
            || native_planning_checkpoint_present(&project)
        {
            state.set_stage("resume_validate");
            persist_native_state(&project, &state)?;
            log_lines.push(format!(
                "[Native Resume] matched=true fingerprint={} project={}",
                input_fingerprint,
                project.display()
            ));
            return Ok((project, state, true));
        }
        state.set_status("failed");
        let _ = persist_native_state(&project, &state);
        log_lines.push(format!(
            "[Native Resume] matched=true reusable=false reason=incomplete-planning-artifacts project={} action=create-new-project",
            project.display()
        ));
    }

    let project = init_project_with_project_manager(root, python_path, title, log_lines)?;
    let state = NativeGenerationState::new(
        input_fingerprint.to_string(),
        title.to_string(),
        slide_count,
        model,
        &project,
    );
    persist_native_state(&project, &state)?;
    log_lines.push(format!(
        "[Native Resume] matched=false fingerprint={} action=create-new-project",
        input_fingerprint
    ));
    Ok((project, state, false))
}

fn native_planning_artifacts_present(project: &Path) -> bool {
    ["design_spec.md", "spec_lock.md", "slide_plan.json"]
        .into_iter()
        .map(|name| project.join(name))
        .all(|path| path.is_file() && fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0))
}

fn native_planning_checkpoint_present(project: &Path) -> bool {
    let path = project.join(NATIVE_PLANNING_CHECKPOINT_FILE);
    path.is_file() && fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

fn persist_native_state(project: &Path, state: &NativeGenerationState) -> Result<(), AppError> {
    write_state_atomic(project, state)
        .map(|_| ())
        .map_err(AppError::Custom)
}

fn load_native_planning_artifacts(
    project: &Path,
    expected_slide_count: usize,
) -> Result<(SlidePlan, String, PathBuf, PathBuf, PathBuf), AppError> {
    let design_spec_path = project.join("design_spec.md");
    let spec_lock_path = project.join("spec_lock.md");
    let slide_plan_path = project.join("slide_plan.json");
    for (label, path) in [
        ("design_spec.md", &design_spec_path),
        ("spec_lock.md", &spec_lock_path),
        ("slide_plan.json", &slide_plan_path),
    ] {
        if !path.is_file() {
            return Err(AppError::NotFound(format!(
                "原生断点缺少可复用规划产物 {label}: {}",
                path.display()
            )));
        }
        if fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            == 0
        {
            return Err(AppError::Custom(format!(
                "原生断点规划产物为空 {label}: {}",
                path.display()
            )));
        }
    }
    let plan_raw = fs::read_to_string(&slide_plan_path).map_err(|error| {
        AppError::Custom(format!(
            "读取原生断点 slide_plan.json 失败: {} ({error})",
            slide_plan_path.display()
        ))
    })?;
    let plan: SlidePlan = serde_json::from_str(&plan_raw).map_err(|error| {
        AppError::Custom(format!(
            "解析原生断点 slide_plan.json 失败: {} ({error})",
            slide_plan_path.display()
        ))
    })?;
    if plan.slides.len() != expected_slide_count {
        return Err(AppError::InvalidInput(format!(
            "原生断点页数与当前输入不一致: expected={}, actual={}, path={}",
            expected_slide_count,
            plan.slides.len(),
            slide_plan_path.display()
        )));
    }
    for (index, slide) in plan.slides.iter().enumerate() {
        if slide.page != index + 1 {
            return Err(AppError::InvalidInput(format!(
                "原生断点 slide_plan 页码不连续: index={}, page={}, path={}",
                index + 1,
                slide.page,
                slide_plan_path.display()
            )));
        }
    }
    let design_spec = fs::read_to_string(&design_spec_path).map_err(|error| {
        AppError::Custom(format!(
            "读取原生断点 design_spec.md 失败: {} ({error})",
            design_spec_path.display()
        ))
    })?;
    Ok((
        plan,
        design_spec,
        design_spec_path,
        spec_lock_path,
        slide_plan_path,
    ))
}

fn update_native_state_plan_paths(
    state: &mut NativeGenerationState,
    project: &Path,
    plan: &SlidePlan,
) {
    state.artifacts.design_spec_path = project.join("design_spec.md").to_string_lossy().to_string();
    state.artifacts.spec_lock_path = project.join("spec_lock.md").to_string_lossy().to_string();
    state.artifacts.slide_plan_path = project
        .join("slide_plan.json")
        .to_string_lossy()
        .to_string();
    for slide in &plan.slides {
        let page_state = state.page_mut(slide.page);
        page_state.svg_path = project
            .join("svg_output")
            .join(svg_filename_for_slide(slide))
            .to_string_lossy()
            .to_string();
        page_state.updated_at = native_state_now();
    }
}

#[derive(Debug, Default)]
struct NativeReusePreparation {
    reusable_pages: HashSet<usize>,
    upstream_changed: bool,
}

#[derive(Debug, Clone)]
struct NativePageValidationFailure {
    stage: String,
    violated_rule: String,
    checker_summary: String,
    text_geometry: Option<NativeTextGeometryState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeTextGeometryReport {
    schema_version: u32,
    #[serde(default)]
    svg_path: Option<String>,
    passed: bool,
    #[serde(default)]
    hard_errors: Vec<serde_json::Value>,
    #[serde(default)]
    warnings: Vec<serde_json::Value>,
    #[serde(default)]
    checker_error: Option<String>,
    #[serde(default)]
    auto_fix_applied: Vec<serde_json::Value>,
    #[serde(default)]
    text_blocks: Vec<serde_json::Value>,
    #[serde(default)]
    failure_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativePowerPointGeometryReport {
    schema_version: u32,
    passed: bool,
    #[serde(default)]
    pptx_path: Option<String>,
    #[serde(default)]
    render_dir: Option<String>,
    #[serde(default)]
    hard_errors: Vec<serde_json::Value>,
    #[serde(default)]
    warnings: Vec<serde_json::Value>,
    #[serde(default)]
    safe_fixes: Vec<serde_json::Value>,
    #[serde(default)]
    pages: Vec<serde_json::Value>,
    #[serde(default)]
    checker_error: Option<String>,
}

impl NativePowerPointGeometryReport {
    fn first_page(&self) -> Option<usize> {
        self.hard_errors
            .first()
            .and_then(|issue| issue.get("pageNumber"))
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as usize)
    }

    fn first_rule(&self) -> String {
        self.hard_errors
            .first()
            .and_then(|issue| issue.get("rule"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("powerpoint_text_geometry_checker_failed")
            .to_string()
    }

    fn summary(&self) -> String {
        if let Some(error) = self.checker_error.as_deref() {
            return format!("PowerPoint geometry checker error: {error}");
        }
        format!(
            "hardErrors={},warnings={},renderDir={},firstIssue={}",
            self.hard_errors.len(),
            self.warnings.len(),
            self.render_dir.as_deref().unwrap_or("unknown"),
            self.hard_errors
                .first()
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        )
    }

    fn issues_for_page(&self, page: usize) -> Vec<serde_json::Value> {
        self.hard_errors
            .iter()
            .filter(|issue| {
                issue.get("pageNumber").and_then(serde_json::Value::as_u64) == Some(page as u64)
            })
            .cloned()
            .collect()
    }
}

impl NativeTextGeometryReport {
    fn state(&self) -> NativeTextGeometryState {
        NativeTextGeometryState {
            passed: self.passed,
            hard_errors: self.hard_errors.clone(),
            warnings: self.warnings.clone(),
            checked_at: native_state_now(),
        }
    }

    fn violated_rule(&self) -> String {
        const RULE_PRIORITY: [&str; 5] = [
            "text_outside_canvas",
            "text_text_overlap",
            "text_obstacle_overlap",
            "text_exceeds_max_lines",
            "text_outside_declared_region",
        ];
        RULE_PRIORITY
            .iter()
            .find_map(|preferred| {
                self.hard_errors.iter().find(|issue| {
                    issue.get("rule").and_then(serde_json::Value::as_str) == Some(*preferred)
                })
            })
            .or_else(|| self.hard_errors.first())
            .and_then(|issue| issue.get("rule"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("text_geometry_checker_failed")
            .to_string()
    }

    fn actionable_issues(&self) -> serde_json::Value {
        const MAX_TARGETS: usize = 24;
        let mut target_indexes = HashMap::<String, usize>::new();
        let mut omitted_targets = HashSet::<String>::new();
        let mut targets = Vec::<serde_json::Value>::new();

        for issue in &self.hard_errors {
            let key = issue
                .get("domIndex")
                .map(|value| format!("dom:{value}"))
                .or_else(|| {
                    issue
                        .get("regionId")
                        .and_then(serde_json::Value::as_str)
                        .map(|value| format!("region:{value}"))
                })
                .unwrap_or_else(|| format!("issue:{}", targets.len()));
            let rule = issue
                .get("rule")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("text_geometry_checker_failed");
            if let Some(index) = target_indexes.get(&key).copied() {
                if let Some(rules) = targets[index]
                    .get_mut("rules")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    let rule_value = serde_json::Value::String(rule.to_string());
                    if !rules.contains(&rule_value) {
                        rules.push(rule_value);
                    }
                }
                continue;
            }
            if targets.len() >= MAX_TARGETS {
                omitted_targets.insert(key);
                continue;
            }

            let mut target = serde_json::Map::new();
            target.insert(
                "rules".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::String(rule.to_string())]),
            );
            for field in [
                "domIndex",
                "regionId",
                "role",
                "text",
                "actualBounds",
                "allowedBounds",
                "overflow",
                "collision",
            ] {
                if let Some(value) = issue.get(field) {
                    target.insert(field.to_string(), value.clone());
                }
            }
            target_indexes.insert(key, targets.len());
            targets.push(serde_json::Value::Object(target));
        }

        serde_json::json!({
            "targets": targets,
            "omittedTargets": omitted_targets.len(),
        })
    }

    fn visible_texts(&self) -> Vec<String> {
        self.text_blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect()
    }

    fn repair_context(&self) -> serde_json::Value {
        let regions = self
            .text_blocks
            .iter()
            .filter_map(|block| {
                let region_id = block.get("regionId")?.clone();
                let region = block.get("region")?.clone();
                Some(serde_json::json!({
                    "regionId": region_id,
                    "role": block.get("role").cloned().unwrap_or(serde_json::Value::Null),
                    "text": block.get("text").cloned().unwrap_or(serde_json::Value::Null),
                    "region": region,
                    "textAnchor": block.get("textAnchor").cloned().unwrap_or(serde_json::Value::Null),
                    "minFontSize": block.get("minFontSize").cloned().unwrap_or(serde_json::Value::Null),
                    "maxLines": block.get("maxLines").cloned().unwrap_or(serde_json::Value::Null),
                    "wrap": block.get("wrap").cloned().unwrap_or(serde_json::Value::Null),
                }))
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "failureKind": self.failure_kind.clone(),
            "mustKeepVisibleText": self.visible_texts(),
            "issues": self.actionable_issues(),
            "allowedRegions": regions,
        })
    }

    fn summary(&self) -> String {
        if let Some(error) = self.checker_error.as_deref() {
            return format!("text geometry checker error: {error}");
        }
        format!(
            "hardErrors={},warnings={},actionableIssues={}",
            self.hard_errors.len(),
            self.warnings.len(),
            self.actionable_issues()
        )
    }
}

fn materialize_native_text_geometry_checker() -> Result<PathBuf, AppError> {
    let directory = std::env::temp_dir().join("pomegranate-native-tools");
    fs::create_dir_all(&directory).map_err(|error| {
        AppError::Custom(format!(
            "创建原生文本几何检查器目录失败: {} ({error})",
            directory.display()
        ))
    })?;
    let path = directory.join(NATIVE_TEXT_GEOMETRY_CHECKER_FILE);
    let needs_write = fs::read_to_string(&path)
        .map(|current| current != NATIVE_TEXT_GEOMETRY_CHECKER_SOURCE)
        .unwrap_or(true);
    if needs_write {
        fs::write(&path, NATIVE_TEXT_GEOMETRY_CHECKER_SOURCE).map_err(|error| {
            AppError::Custom(format!(
                "写入原生文本几何检查器失败: {} ({error})",
                path.display()
            ))
        })?;
    }
    Ok(path)
}

fn materialize_native_powerpoint_geometry_checker() -> Result<PathBuf, AppError> {
    let directory = std::env::temp_dir().join("pomegranate-native-tools");
    fs::create_dir_all(&directory).map_err(|error| {
        AppError::Custom(format!(
            "创建 PowerPoint 文本几何检查器目录失败: {} ({error})",
            directory.display()
        ))
    })?;
    let path = directory.join(NATIVE_POWERPOINT_GEOMETRY_CHECKER_FILE);
    let needs_write = fs::read_to_string(&path)
        .map(|current| current != NATIVE_POWERPOINT_GEOMETRY_CHECKER_SOURCE)
        .unwrap_or(true);
    if needs_write {
        fs::write(&path, NATIVE_POWERPOINT_GEOMETRY_CHECKER_SOURCE).map_err(|error| {
            AppError::Custom(format!(
                "写入 PowerPoint 文本几何检查器失败: {} ({error})",
                path.display()
            ))
        })?;
    }
    Ok(path)
}

fn run_native_text_geometry_check(
    python_path: &str,
    svg_path: &Path,
) -> Result<NativeTextGeometryReport, AppError> {
    let checker = materialize_native_text_geometry_checker()?;
    let mut command = Command::new(python_path);
    command
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .arg(&checker)
        .arg("--svg")
        .arg(svg_path)
        .arg("--auto-fix");
    add_no_window(&mut command);
    let output = command.output().map_err(|error| {
        AppError::Custom(format!(
            "启动原生文本几何检查器失败: python={}, checker={}, svg={} ({error})",
            python_path,
            checker.display(),
            svg_path.display()
        ))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let report: NativeTextGeometryReport = serde_json::from_str(&stdout).map_err(|error| {
        AppError::Custom(format!(
            "解析原生文本几何检查结果失败: svg={}, exitCode={:?}, stdout={}, stderr={} ({error})",
            svg_path.display(),
            output.status.code(),
            single_line_log_value(&stdout),
            single_line_log_value(&stderr)
        ))
    })?;
    if let Some(checker_error) = report.checker_error.as_deref() {
        return Err(AppError::Custom(format!(
            "原生文本几何检查器执行失败: svg={}, error={}, stderr={}",
            svg_path.display(),
            checker_error,
            single_line_log_value(&stderr)
        )));
    }
    if !matches!(output.status.code(), Some(0 | 2)) {
        return Err(AppError::Custom(format!(
            "原生文本几何检查器异常退出: svg={}, exitCode={:?}, stderr={}",
            svg_path.display(),
            output.status.code(),
            single_line_log_value(&stderr)
        )));
    }
    Ok(report)
}

fn run_native_powerpoint_geometry_check(
    project: &Path,
    pptx_path: &Path,
) -> Result<NativePowerPointGeometryReport, AppError> {
    let checker = materialize_native_powerpoint_geometry_checker()?;
    let render_dir = project
        .join("analysis")
        .join("powerpoint_text_geometry_render");
    fs::create_dir_all(&render_dir).map_err(|error| {
        AppError::Custom(format!(
            "创建 PowerPoint 文本几何预览目录失败: {} ({error})",
            render_dir.display()
        ))
    })?;
    let mut command = Command::new("powershell");
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&checker)
        .arg("-PptxPath")
        .arg(pptx_path)
        .arg("-SvgDir")
        .arg(project.join("svg_output"))
        .arg("-RenderDir")
        .arg(&render_dir)
        .arg("-ApplySafeRegionFixes");
    add_no_window(&mut command);
    let output = command.output().map_err(|error| {
        AppError::Custom(format!(
            "启动 PowerPoint 文本几何检查器失败: checker={}, pptx={} ({error})",
            checker.display(),
            pptx_path.display()
        ))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let report: NativePowerPointGeometryReport =
        serde_json::from_str(&stdout).map_err(|error| {
            AppError::Custom(format!(
                "解析 PowerPoint 文本几何检查结果失败: exitCode={:?}, stdout={}, stderr={} ({error})",
                output.status.code(),
                single_line_log_value(&stdout),
                single_line_log_value(&stderr)
            ))
        })?;
    if let Some(checker_error) = report.checker_error.as_deref() {
        return Err(AppError::Custom(format!(
            "PowerPoint 文本几何检查器执行失败: {checker_error}"
        )));
    }
    if !matches!(output.status.code(), Some(0 | 2)) {
        return Err(AppError::Custom(format!(
            "PowerPoint 文本几何检查器异常退出: exitCode={:?}, stderr={}",
            output.status.code(),
            single_line_log_value(&stderr)
        )));
    }
    Ok(report)
}

fn native_pages_requiring_generation(
    plan: &SlidePlan,
    reusable_pages: &HashSet<usize>,
) -> Vec<usize> {
    plan.slides
        .iter()
        .filter(|slide| !reusable_pages.contains(&slide.page))
        .map(|slide| slide.page)
        .collect()
}

fn sorted_page_list(pages: &HashSet<usize>) -> String {
    let mut pages = pages.iter().copied().collect::<Vec<_>>();
    pages.sort_unstable();
    pages
        .into_iter()
        .map(|page| format!("P{page:02}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn set_native_page_state(
    state: &mut NativeGenerationState,
    page: usize,
    status: &str,
    last_error: Option<String>,
    violated_rule: Option<String>,
    checker_summary: Option<String>,
    reused: bool,
) {
    let page_state = state.page_mut(page);
    page_state.status = status.to_string();
    page_state.last_error = last_error;
    page_state.violated_rule = violated_rule;
    page_state.checker_summary = checker_summary;
    page_state.reused = reused;
    page_state.updated_at = native_state_now();
}

fn set_native_text_geometry_state(
    state: &mut NativeGenerationState,
    page: usize,
    geometry: NativeTextGeometryState,
) {
    let page_state = state.page_mut(page);
    page_state.text_geometry = Some(geometry);
    page_state.updated_at = native_state_now();
}

fn record_native_pipeline_failure(project: &Path, state: &mut NativeGenerationState, stage: &str) {
    state.current_stage = stage.to_string();
    state.set_status("failed");
    let _ = persist_native_state(project, state);
}

fn consume_native_powerpoint_repair_marker(svg: &str) -> Option<String> {
    const MARKERS: [&str; 2] = [
        " data-pome-powerpoint-repair-ready=\"true\"",
        " data-pome-powerpoint-repair-ready='true'",
    ];
    MARKERS
        .iter()
        .find(|marker| svg.contains(**marker))
        .map(|marker| svg.replacen(marker, "", 1))
}

fn stored_powerpoint_region_drift_is_safely_recheckable(summary: Option<&str>) -> bool {
    const PREFIX: &str = "PowerPoint actual text bounds failed: ";
    let Some(raw) = summary.and_then(|value| value.strip_prefix(PREFIX)) else {
        return false;
    };
    let Ok(issues) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return false;
    };
    if issues.is_empty() {
        return false;
    }
    issues.iter().all(|issue| {
        if issue.get("rule").and_then(serde_json::Value::as_str)
            != Some("powerpoint_text_outside_declared_region")
        {
            return false;
        }
        let Some(allowed) = issue.get("allowedBounds") else {
            return false;
        };
        let Some(width) = allowed.get("width").and_then(serde_json::Value::as_f64) else {
            return false;
        };
        let Some(height) = allowed.get("height").and_then(serde_json::Value::as_f64) else {
            return false;
        };
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return false;
        }
        let Some(overflow) = issue.get("overflow") else {
            return false;
        };
        let amounts = ["left", "top", "right", "bottom"]
            .into_iter()
            .map(|key| overflow.get(key).and_then(serde_json::Value::as_f64))
            .collect::<Option<Vec<_>>>();
        let Some(amounts) = amounts else {
            return false;
        };
        if amounts
            .iter()
            .any(|amount| !amount.is_finite() || *amount < 0.0)
        {
            return false;
        }
        let max_overflow = amounts.into_iter().fold(0.0_f64, f64::max);
        let safe_limit = (width.max(height) * 0.2).clamp(3.0, 12.0);
        max_overflow > 0.0 && max_overflow <= safe_limit
    })
}

fn prepare_existing_native_pages(
    root: &Path,
    python_path: &str,
    project: &Path,
    plan: &SlidePlan,
    theme_spec: &NativeThemeSpec,
    state: &mut NativeGenerationState,
    log_lines: &mut Vec<String>,
    started: Instant,
) -> Result<NativeReusePreparation, AppError> {
    let svg_output = project.join("svg_output");
    let mut preparation = NativeReusePreparation::default();
    let mut structurally_valid = HashSet::new();
    let mut powerpoint_repair_ready_pages = HashSet::new();
    for slide in &plan.slides {
        let file_name = svg_filename_for_slide(slide);
        let path = svg_output.join(&file_name);
        state.page_mut(slide.page).svg_path = path.to_string_lossy().to_string();
        if !path.is_file() {
            set_native_page_state(
                state,
                slide.page,
                "pending",
                Some(format!("缺少原生 SVG: {}", path.display())),
                None,
                None,
                false,
            );
            persist_native_state(project, state)?;
            continue;
        }
        let mut original = fs::read_to_string(&path).map_err(|error| {
            AppError::Custom(format!(
                "读取原生断点 SVG 失败: {} ({error})",
                path.display()
            ))
        })?;
        if let Some(cleaned) = consume_native_powerpoint_repair_marker(&original) {
            write_file(&path, &cleaned)?;
            original = cleaned;
            powerpoint_repair_ready_pages.insert(slide.page);
            preparation.upstream_changed = true;
            log_lines.push(format!(
                "[Native Resume] page=P{:02} action=consume-powerpoint-repair-marker repairReady=true fallback=false file={}",
                slide.page, file_name
            ));
        }
        let (normalized, report) = normalize_native_svg_compatibility(&original);
        if normalized != original {
            write_file(&path, &normalized)?;
            preparation.upstream_changed = true;
            log_lines.push(format!(
                "[Native Resume] page=P{:02} action=mechanical-repair file={} rgbaColorsNormalized={} groupOpacityNormalized={} filtersRemoved={} malformedClosingTagsRepaired={} duplicateLineCoordinatesRepaired={} fallback=false",
                slide.page,
                file_name,
                report.rgba_colors_normalized,
                report.group_opacity_normalized,
                report.filters_removed,
                report.malformed_closing_tags_repaired,
                report.duplicate_line_coordinates_repaired
            ));
        }
        match validate_native_svg_text(&file_name, &normalized).and_then(|_| {
            validate_visible_text_integrity(&normalized).map_err(|error| {
                AppError::Custom(format!(
                    "原生 SVG 可见文字完整性检查失败: {file_name} ({error})"
                ))
            })
        }) {
            Ok(()) => {
                let theme_validation = validate_svg_theme(&normalized, theme_spec);
                log_lines.push(format!(
                    "[Native Theme Check] page=P{:02} passed={} {}",
                    slide.page, theme_validation.passed, theme_validation.summary
                ));
                if theme_validation.passed {
                    structurally_valid.insert(file_name);
                } else {
                    set_native_page_state(
                        state,
                        slide.page,
                        "failed",
                        Some(theme_validation.summary.clone()),
                        Some("native_theme_consistency".to_string()),
                        Some(theme_validation.summary),
                        false,
                    );
                    persist_native_state(project, state)?;
                }
            }
            Err(error) => {
                set_native_page_state(
                    state,
                    slide.page,
                    "failed",
                    Some(error.to_string()),
                    Some("native SVG completeness/canvas".to_string()),
                    None,
                    false,
                );
                persist_native_state(project, state)?;
            }
        }
    }

    let quality = if structurally_valid.is_empty() {
        None
    } else {
        Some(run_quality_check(root, python_path, project, started)?)
    };
    let quality_failures = quality
        .as_ref()
        .map(|result| parse_native_quality_failures(&result.stdout, &result.stderr))
        .unwrap_or_default();
    let native_issues = scan_native_incompatible_svgs(&svg_output)?;
    let text_issues = scan_final_text_leaks(&svg_output)?;

    for slide in &plan.slides {
        let file_name = svg_filename_for_slide(slide);
        let geometry_path = svg_output.join(&file_name);
        let prior_powerpoint_failure =
            state
                .pages
                .get(&slide.page.to_string())
                .is_some_and(|page| {
                    page.status == "failed"
                        && page
                            .violated_rule
                            .as_deref()
                            .is_some_and(|rule| rule.starts_with("powerpoint_"))
                });
        let repaired_source_ready =
            prior_powerpoint_failure && powerpoint_repair_ready_pages.contains(&slide.page);
        let stored_region_drift_recheckable = prior_powerpoint_failure
            && state
                .pages
                .get(&slide.page.to_string())
                .is_some_and(|page| {
                    stored_powerpoint_region_drift_is_safely_recheckable(
                        page.last_error
                            .as_deref()
                            .or(page.checker_summary.as_deref()),
                    )
                });
        let powerpoint_retry_without_ai_ready =
            repaired_source_ready || stored_region_drift_recheckable;
        if prior_powerpoint_failure
            && !powerpoint_retry_without_ai_ready
            && state
                .pages
                .get(&slide.page.to_string())
                .is_some_and(|page| {
                    page.attempts >= NATIVE_POWERPOINT_GEOMETRY_MAX_AI_ATTEMPTS_PER_PAGE
                })
        {
            return Err(AppError::Custom(format!(
                "P{:02} PowerPoint 文本几何页内重试已达到上限 {}；保留失败页和中间产物，不调用 fallback",
                slide.page, NATIVE_POWERPOINT_GEOMETRY_MAX_AI_ATTEMPTS_PER_PAGE
            )));
        }
        if !structurally_valid.contains(&file_name) {
            continue;
        }
        if let Some(failure) = quality_failures
            .iter()
            .find(|failure| failure.file_name == file_name)
        {
            set_native_page_state(
                state,
                slide.page,
                "failed",
                Some(failure.checker_summary.clone()),
                Some(failure.violated_rule.clone()),
                Some(failure.checker_summary.clone()),
                false,
            );
            log_lines.push(format!(
                "[Native Resume] page=P{:02} reusable=false violatedRule={} file={}",
                slide.page, failure.violated_rule, file_name
            ));
            persist_native_state(project, state)?;
            continue;
        }
        if quality.as_ref().is_some_and(|result| !result.success) && quality_failures.is_empty() {
            let summary = quality
                .as_ref()
                .map(|result| single_line_log_value(&result.stdout))
                .unwrap_or_else(|| "SVG Quality Checker failed".to_string());
            set_native_page_state(
                state,
                slide.page,
                "failed",
                Some(summary.clone()),
                Some("SVG Quality Checker hard error".to_string()),
                Some(summary),
                false,
            );
            persist_native_state(project, state)?;
            continue;
        }
        if let Some(issue) = native_issues
            .iter()
            .find(|issue| issue.file_name == file_name)
        {
            let detail = summarize_native_issues(std::slice::from_ref(issue));
            set_native_page_state(
                state,
                slide.page,
                "failed",
                Some(detail.clone()),
                Some("DrawingML unsupported SVG element".to_string()),
                Some(detail),
                false,
            );
            persist_native_state(project, state)?;
            continue;
        }
        if let Some(issue) = text_issues
            .iter()
            .find(|issue| issue.file_name == file_name)
        {
            let detail = summarize_final_text_issues(std::slice::from_ref(issue));
            set_native_page_state(
                state,
                slide.page,
                "failed",
                Some(detail.clone()),
                Some("visible internal/template text leakage".to_string()),
                Some(detail),
                false,
            );
            persist_native_state(project, state)?;
            continue;
        }
        let before_geometry = fs::read_to_string(&geometry_path).map_err(|error| {
            AppError::Custom(format!(
                "读取文本几何检查前 SVG 失败: {} ({error})",
                geometry_path.display()
            ))
        })?;
        let geometry = run_native_text_geometry_check(python_path, &geometry_path)?;
        let geometry_changed = fs::read_to_string(&geometry_path)
            .map(|current| current != before_geometry)
            .unwrap_or(false);
        if geometry_changed {
            preparation.upstream_changed = true;
        }
        set_native_text_geometry_state(state, slide.page, geometry.state());
        log_lines.push(format!(
            "[Native Resume] page=P{:02} textGeometryPassed={} hardErrors={} warnings={} autoFixedBlocks={} file={}",
            slide.page,
            geometry.passed,
            geometry.hard_errors.len(),
            geometry.warnings.len(),
            geometry.auto_fix_applied.len(),
            file_name
        ));
        if !geometry.passed {
            let summary = geometry.summary();
            set_native_page_state(
                state,
                slide.page,
                "failed",
                Some(summary.clone()),
                Some(geometry.violated_rule()),
                Some(summary),
                false,
            );
            state.current_stage = "validate_text_geometry".to_string();
            persist_native_state(project, state)?;
            continue;
        }
        let density_contract = NativePageDensityContract::for_slide(slide);
        let density_report_path = project
            .join("analysis")
            .join("native_space_utilization")
            .join(format!("P{:02}.json", slide.page));
        let density = run_space_utilization_check(
            python_path,
            &geometry_path,
            &density_contract,
            &density_report_path,
        )?;
        log_lines.push(format!(
            "[Native Resume] page=P{:02} spaceUtilizationPassed={} rhythm={} informationOccupancy={:.4} combinedOccupancy={:.4} occupiedZones={} file={}",
            slide.page,
            density.passed,
            density.page_rhythm,
            density.information_occupancy_ratio,
            density.combined_occupancy_ratio,
            density.occupied_zone_count,
            file_name
        ));
        if !density.passed {
            let summary = density.summary();
            set_native_page_state(
                state,
                slide.page,
                "failed",
                Some(summary.clone()),
                Some(density.violated_rule()),
                Some(summary),
                false,
            );
            state.current_stage = "validate_space_utilization".to_string();
            persist_native_state(project, state)?;
            continue;
        }
        if prior_powerpoint_failure && !powerpoint_retry_without_ai_ready {
            log_lines.push(format!(
                "[Native Resume] page=P{:02} reusable=false reason=previous-powerpoint-text-geometry-failure action=regenerate-page-only fallback=false file={}",
                slide.page, file_name
            ));
            persist_native_state(project, state)?;
            continue;
        }
        if repaired_source_ready {
            log_lines.push(format!(
                "[Native Resume] page=P{:02} action=verify-repaired-source previousFailure=powerpoint-text-geometry oneTimeRepairMarker=true fallback=false file={}",
                slide.page, file_name
            ));
        }
        if stored_region_drift_recheckable {
            log_lines.push(format!(
                "[Native Resume] page=P{:02} action=reuse-for-powerpoint-region-recheck previousFailure=bounded-font-engine-drift aiCalled=false fallback=false file={}",
                slide.page, file_name
            ));
        }
        preparation.reusable_pages.insert(slide.page);
        set_native_page_state(state, slide.page, "validated", None, None, None, true);
        log_lines.push(format!(
            "[Native Resume] page=P{:02} action=reuse status=validated aiCalled=false file={}",
            slide.page, file_name
        ));
        persist_native_state(project, state)?;
    }

    if preparation.upstream_changed {
        invalidate_downstream(project).map_err(AppError::Custom)?;
        state.artifacts.final_pptx_path = None;
        persist_native_state(project, state)?;
    }
    Ok(preparation)
}

fn validate_generated_native_page(
    root: &Path,
    python_path: &str,
    project: &Path,
    file_name: &str,
    page_number: usize,
    theme_spec: &NativeThemeSpec,
    density_contract: &NativePageDensityContract,
    log_lines: &mut Vec<String>,
    started: Instant,
) -> Result<
    (
        Option<NativePageValidationFailure>,
        PptMasterExportResult,
        Option<NativeTextGeometryReport>,
    ),
    AppError,
> {
    let quality = run_quality_check(root, python_path, project, started)?;
    if let Some(failure) =
        native_page_quality_failure(file_name, quality.success, &quality.stdout, &quality.stderr)
    {
        return Ok((Some(failure), quality, None));
    }
    let svg_output = project.join("svg_output");
    if let Some(issue) = scan_native_incompatible_svgs(&svg_output)?
        .into_iter()
        .find(|issue| issue.file_name == file_name)
    {
        return Ok((
            Some(NativePageValidationFailure {
                stage: "validate_svgs".to_string(),
                violated_rule: "DrawingML unsupported SVG element".to_string(),
                checker_summary: summarize_native_issues(std::slice::from_ref(&issue)),
                text_geometry: None,
            }),
            quality,
            None,
        ));
    }
    let svg_path = svg_output.join(file_name);
    let svg = fs::read_to_string(&svg_path).map_err(|error| {
        AppError::Custom(format!(
            "读取原生 SVG 做主题与文字检查失败: {} ({error})",
            svg_path.display()
        ))
    })?;
    if let Err(error) = validate_visible_text_integrity(&svg) {
        return Ok((
            Some(NativePageValidationFailure {
                stage: "validate_visible_text_integrity".to_string(),
                violated_rule: error.clone(),
                checker_summary: format!("file={file_name},integrityError={error}"),
                text_geometry: None,
            }),
            quality,
            None,
        ));
    }
    let theme_validation = validate_svg_theme(&svg, theme_spec);
    log_lines.push(format!(
        "[Native Theme Check] file={} passed={} {}",
        file_name, theme_validation.passed, theme_validation.summary
    ));
    if !theme_validation.passed {
        return Ok((
            Some(NativePageValidationFailure {
                stage: "validate_theme_consistency".to_string(),
                violated_rule: "page_does_not_follow_native_theme_spec".to_string(),
                checker_summary: theme_validation.summary,
                text_geometry: None,
            }),
            quality,
            None,
        ));
    }
    let geometry = run_native_text_geometry_check(python_path, &svg_path)?;
    if !geometry.passed {
        let state = geometry.state();
        return Ok((
            Some(NativePageValidationFailure {
                stage: "validate_text_geometry".to_string(),
                violated_rule: geometry.violated_rule(),
                checker_summary: geometry.summary(),
                text_geometry: Some(state),
            }),
            quality,
            Some(geometry),
        ));
    }
    if let Some(issue) = scan_final_text_leaks(&svg_output)?
        .into_iter()
        .find(|issue| issue.file_name == file_name)
    {
        return Ok((
            Some(NativePageValidationFailure {
                stage: "validate_svgs".to_string(),
                violated_rule: "visible internal/template text leakage".to_string(),
                checker_summary: summarize_final_text_issues(std::slice::from_ref(&issue)),
                text_geometry: Some(geometry.state()),
            }),
            quality,
            Some(geometry),
        ));
    }
    let report_path = project
        .join("analysis")
        .join("native_space_utilization")
        .join(format!("P{page_number:02}.json"));
    let space =
        run_space_utilization_check(python_path, &svg_path, density_contract, &report_path)?;
    log_lines.push(format!(
        "[Space Utilization] file={} passed={} rhythm={} informationOccupancy={:.4} combinedOccupancy={:.4} occupiedZones={} report={}",
        file_name,
        space.passed,
        space.page_rhythm,
        space.information_occupancy_ratio,
        space.combined_occupancy_ratio,
        space.occupied_zone_count,
        report_path.display()
    ));
    if !space.passed {
        return Ok((
            Some(NativePageValidationFailure {
                stage: "validate_space_utilization".to_string(),
                violated_rule: space.violated_rule(),
                checker_summary: space.summary(),
                text_geometry: Some(geometry.state()),
            }),
            quality,
            Some(geometry),
        ));
    }
    Ok((None, quality, Some(geometry)))
}

fn native_page_quality_failure(
    file_name: &str,
    quality_success: bool,
    stdout: &str,
    stderr: &str,
) -> Option<NativePageValidationFailure> {
    let failures = parse_native_quality_failures(stdout, stderr);
    if let Some(failure) = failures
        .iter()
        .find(|failure| failure.file_name == file_name)
    {
        return Some(NativePageValidationFailure {
            stage: "validate_svgs".to_string(),
            violated_rule: failure.violated_rule.clone(),
            checker_summary: failure.checker_summary.clone(),
            text_geometry: None,
        });
    }
    if !quality_success && failures.is_empty() {
        return Some(NativePageValidationFailure {
            stage: "validate_svgs".to_string(),
            violated_rule: "SVG Quality Checker hard error".to_string(),
            checker_summary: single_line_log_value(stdout),
            text_geometry: None,
        });
    }
    None
}

#[cfg(test)]
fn injected_native_failure_page() -> Option<usize> {
    std::env::var("POME_NATIVE_TEST_FAIL_BEFORE_PAGE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
}

#[cfg(not(test))]
fn injected_native_failure_page() -> Option<usize> {
    None
}

async fn generate_slide_plan_with_ai(
    db: &Database,
    prompt: &str,
    model_id: Option<i64>,
    title: &str,
    slide_count: usize,
    style: &str,
) -> Result<SlidePlan, AppError> {
    let _legacy_ai_prompt = format!(
        "你是稳定模式 PPT 内容策划助手。请只输出严格 JSON，不要 markdown，不要代码块，不要解释。\n\
         目标：根据用户原始语料和 AI 理解结果，生成一个简单但内容真实的 slide_plan，用于 Pomegranate 稳定 SVG 渲染。\n\n\
         必须输出 JSON：\n\
         {{\"title\":\"...\",\"subtitle\":\"...\",\"audience\":\"...\",\"style\":\"...\",\"slides\":[{{\"page\":1,\"type\":\"cover\",\"layout\":\"cover\",\"title\":\"...\",\"subtitle\":\"...\",\"bullets\":[],\"visualHint\":\"...\",\"speakerNote\":\"...\"}}]}}\n\n\
         规则：\n\
         - slides 数量必须等于 {slide_count}。\n\
         - title、subtitle、bullets 必须来自用户语料、AI 理解结果，或对用户材料的合理概括。\n\
         - 每个内容页 bullets 必须有 2-4 条真实信息，不能使用占位句。\n\
         - 禁止输出这些占位话术：提炼用户材料中的关键信息、围绕当前主题组织重点表达、使用短句和结构化表达、概括本主题的核心内容。\n\
         - 不要编造材料中不存在的数字、排名、奖项、年份或确定事实。\n\
         - 不要套项目路演、工科、技术方案模板，除非用户材料明确要求。\n\
         - layout 只能从 cover, cards, timeline, compare, process, matrix, highlight, image_text, summary 中选择。\n\
         - 封面用 cover，最后一页可用 summary；其他页根据材料内容选择。\n\n\
         【建议标题】{title}\n\
         【建议风格】{style}\n\
         【用户材料与 AI 理解】\n{prompt}"
    );
    let ai_prompt = stable_slide_plan_prompt(prompt, title, slide_count, style);
    let input = PluginAiChatInput {
        request_id: "ppt_master_generate_slide_plan".to_string(),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let raw = ppt_ai_chat_with_timeout(db, input, "AI 生成 slide_plan").await?;
    parse_slide_plan_json(&raw)
}

fn stable_slide_plan_prompt(prompt: &str, title: &str, slide_count: usize, style: &str) -> String {
    format!(
        "You are the STABLE MODE PPT content planner. Return strict JSON only; no markdown.\n\
         Goal: create a content-contract-driven slide_plan for exactly {slide_count} slides.\n\
         This is NOT ppt_master_native. Do not output spec_lock/native workflow fields.\n\n\
         Required JSON shape:\n\
         {{\"title\":\"...\",\"subtitle\":\"...\",\"audience\":\"...\",\"style\":\"...\",\"slides\":[{{\
         \"page\":1,\"pageIndex\":1,\"type\":\"cover\",\"layout\":\"cover\",\
         \"title\":\"...\",\"subtitle\":\"...\",\"pageTheme\":\"...\",\
         \"coreMessage\":\"...\",\"mainClaim\":\"...\",\"contentScope\":\"...\",\
         \"contentBlocks\":[{{\"label\":\"...\",\"text\":\"...\",\"detail\":\"...\"}}],\
         \"evidence\":[\"specific fact/concept/example/relation from source\"],\
         \"relation\":\"timeline|category|compare|cause|process|none\",\
         \"chartType\":\"cards|timeline|process|matrix|compare|highlight|summary\",\
         \"density\":\"anchor|dense|breathing\",\
         \"visualIntent\":\"...\",\
         \"bullets\":[\"...\"],\"visualHint\":\"...\",\"speakerNote\":\"2-5 natural sentences\"\
         }}]}}\n\n\
         Hard rules:\n\
         - slides length MUST equal {slide_count}.\n\
         - Every non-cover slide must have 3-6 contentBlocks. Cover/summary should still have at least 2 concrete blocks when possible.\n\
         - evidence must come from user raw material, AI understanding, or a faithful summary of that material.\n\
         - Each page must have one pageTheme and one coreMessage; do not repeat the same theme on adjacent pages.\n\
         - bullets are short display summaries derived from contentBlocks, not placeholders.\n\
         - relation chooses the content organization: timeline/category/compare/cause/process/none.\n\
         - chartType chooses a stable renderer: cards/timeline/process/matrix/compare/highlight/summary.\n\
         - density controls amount: anchor = visual focus, dense = more blocks/details, breathing = more whitespace but still concrete.\n\
         - For humanities such as history/literature/philosophy, contentBlocks should come from people, events, concepts, periods, relationships, influence, viewpoints, or examples.\n\
         - For science/engineering, contentBlocks may come from concepts, principles, structures, processes, data, experiments, or applications.\n\
         - Style only changes expression and visual tone. It must not change the academic/domain content.\n\
         - Do not fabricate numbers, rankings, awards, years, counts, or named facts not present in the material.\n\
         - Forbidden placeholder text: 提炼用户材料中的关键信息; 围绕当前主题组织重点表达; 使用短句和结构化表达; 概括本主题的核心内容; 主题与一句话价值主张; 呈现主题的阶段、结构或层次.\n\
         - If material is thin, merge related themes inside exactly {slide_count} pages and use faithful high-level concepts. Do not create empty cards.\n\n\
         Suggested title: {title}\n\
         Selected style: {style}\n\
         User material and AI understanding:\n{prompt}"
    )
}

async fn repair_stable_slide_plan_with_ai(
    db: &Database,
    prompt: &str,
    plan: &SlidePlan,
    report: &str,
    model_id: Option<i64>,
    title: &str,
    slide_count: usize,
    style: &str,
) -> Result<SlidePlan, AppError> {
    let plan_json = serde_json::to_string(plan)
        .map_err(|e| AppError::Custom(format!("Serialize stable slide_plan failed: {}", e)))?;
    let ai_prompt = format!(
        "Repair this STABLE MODE slide_plan. Return strict JSON only.\n\
         Keep exactly {slide_count} slides and preserve the user's subject.\n\
         Repair reason: {report}\n\n\
         Requirements:\n\
         - Each slide needs title, pageTheme, coreMessage, contentScope.\n\
         - Each non-cover slide needs 3-6 concrete contentBlocks with label/text/detail.\n\
         - Each slide needs evidence from the user material or faithful summary.\n\
         - No placeholder phrases, no invented numbers/rankings/awards/years.\n\
         - Use chartType/layout from cards/timeline/process/matrix/compare/highlight/summary.\n\n\
         Suggested title: {title}\nSelected style: {style}\n\n\
         User material and AI understanding:\n{prompt}\n\n\
         Current JSON:\n{plan_json}"
    );
    let input = PluginAiChatInput {
        request_id: "ppt_master_repair_stable_slide_plan".to_string(),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let raw = ppt_ai_chat_with_timeout(db, input, "AI repair stable slide_plan").await?;
    parse_slide_plan_json(&raw)
}

fn read_ppt_master_skill(root: &Path) -> Result<String, AppError> {
    let path = root.join(PPT_MASTER_SKILL_MD);
    if !path.is_file() {
        return Err(AppError::NotFound(format!(
            "找不到 ppt-master SKILL.md: {}",
            path.display()
        )));
    }
    fs::read_to_string(&path)
        .map_err(|e| AppError::Custom(format!("读取 SKILL.md 失败: {} ({})", path.display(), e)))
}

fn read_text_required(path: &Path, label: &str) -> Result<String, AppError> {
    if !path.is_file() {
        return Err(AppError::NotFound(format!(
            "找不到 {}: {}",
            label,
            path.display()
        )));
    }
    fs::read_to_string(path)
        .map_err(|e| AppError::Custom(format!("读取 {} 失败: {} ({})", label, path.display(), e)))
}

fn read_ppt_master_resources(root: &Path) -> Result<PptMasterResources, AppError> {
    Ok(PptMasterResources {
        modes_index: read_text_required(
            &root.join(PPT_MASTER_MODES_DIR).join("_index.md"),
            "modes/_index.md",
        )?,
        visual_styles_index: read_text_required(
            &root.join(PPT_MASTER_VISUAL_STYLES_DIR).join("_index.md"),
            "visual-styles/_index.md",
        )?,
        layouts_index: read_text_required(
            &root.join(PPT_MASTER_LAYOUTS_DIR).join("layouts_index.json"),
            "layouts_index.json",
        )?,
        charts_index: read_text_required(
            &root.join(PPT_MASTER_CHARTS_DIR).join("charts_index.json"),
            "charts_index.json",
        )?,
        executor_base: read_text_required(
            &root.join(PPT_MASTER_EXECUTOR_BASE),
            "executor-base.md",
        )?,
        shared_standards: read_text_required(
            &root.join(PPT_MASTER_SHARED_STANDARDS),
            "shared-standards.md",
        )?,
    })
}

fn load_chart_catalog(root: &Path) -> ChartCatalog {
    let mut keys = std::collections::HashSet::new();
    let path = root.join(PPT_MASTER_CHARTS_DIR).join("charts_index.json");
    if let Ok(text) = fs::read_to_string(path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(charts) = value.get("charts").and_then(|item| item.as_object()) {
                for key in charts.keys() {
                    keys.insert(key.to_string());
                }
            }
        }
    }
    ChartCatalog { keys }
}

fn resolve_style_mapping(
    root: &Path,
    style: &str,
    input: &PptMasterGenerateInput,
    theme_spec: &NativeThemeSpec,
) -> PptMasterStyleMapping {
    let user_style = style.trim().to_string();
    let (default_mode, default_visual, default_layouts, default_charts): (
        &str,
        &str,
        Vec<&str>,
        Vec<&str>,
    ) = if style.contains("科技") || style.contains("蓝") {
        (
            "showcase",
            "dark-tech",
            vec!["ai_ops"],
            vec![
                "pipeline_with_stages",
                "process_flow",
                "layered_architecture",
                "kpi_cards",
            ],
        )
    } else if style.contains("竞赛") || style.contains("路演") {
        (
            "narrative",
            "glassmorphism",
            vec!["ai_ops"],
            vec![
                "kpi_cards",
                "process_flow",
                "timeline",
                "comparison_columns",
            ],
        )
    } else if style.contains("学术") {
        (
            "instructional",
            "data-journalism",
            vec!["academic_defense"],
            vec!["line_chart", "bar_chart", "basic_table", "timeline"],
        )
    } else if style.contains("图文") {
        (
            "showcase",
            "photo-editorial",
            Vec::new(),
            vec!["vertical_list", "journey_map", "kpi_cards"],
        )
    } else if theme_spec.theme_name == "red-heritage" {
        (
            "narrative",
            "vintage-poster",
            Vec::new(),
            vec![
                "timeline",
                "process_flow",
                "comparison_columns",
                "vertical_list",
            ],
        )
    } else {
        (
            theme_spec.preferred_mode(),
            theme_spec.preferred_visual_style(),
            Vec::new(),
            vec!["kpi_cards", "comparison_columns", "process_flow"],
        )
    };

    let mode = input
        .mode
        .as_deref()
        .map(str::trim)
        .filter(|value| ppt_master_mode_exists(root, value))
        .unwrap_or(default_mode)
        .to_string();
    let visual_style = input
        .visual_style
        .as_deref()
        .map(str::trim)
        .filter(|value| ppt_master_visual_style_exists(root, value))
        .unwrap_or(default_visual)
        .to_string();
    let layout_bias: Vec<String> = if input.layout_bias.is_empty() {
        default_layouts
            .into_iter()
            .map(ToString::to_string)
            .collect()
    } else {
        input
            .layout_bias
            .iter()
            .map(|value| value.trim())
            .filter(|value| ppt_master_layout_exists(root, value))
            .map(ToString::to_string)
            .collect()
    };
    let chart_bias: Vec<String> = if input.chart_bias.is_empty() {
        default_charts
            .into_iter()
            .map(ToString::to_string)
            .collect()
    } else {
        input
            .chart_bias
            .iter()
            .map(|value| value.trim())
            .filter(|value| ppt_master_chart_exists(root, value))
            .map(ToString::to_string)
            .collect()
    };
    let template_provenance = layout_bias
        .iter()
        .map(|layout| format!("layout: skills/ppt-master/templates/layouts/{}", layout))
        .collect();
    let mode_reference =
        fs::read_to_string(root.join(PPT_MASTER_MODES_DIR).join(format!("{}.md", mode)))
            .unwrap_or_default();
    let visual_style_reference = fs::read_to_string(
        root.join(PPT_MASTER_VISUAL_STYLES_DIR)
            .join(format!("{}.md", visual_style)),
    )
    .unwrap_or_default();

    PptMasterStyleMapping {
        user_style,
        mode,
        visual_style,
        mode_reference,
        visual_style_reference,
        layout_bias,
        chart_bias,
        template_provenance,
    }
}

fn ppt_master_mode_exists(root: &Path, mode: &str) -> bool {
    root.join(PPT_MASTER_MODES_DIR)
        .join(format!("{}.md", mode))
        .is_file()
}

fn ppt_master_visual_style_exists(root: &Path, visual_style: &str) -> bool {
    root.join(PPT_MASTER_VISUAL_STYLES_DIR)
        .join(format!("{}.md", visual_style))
        .is_file()
}

fn ppt_master_layout_exists(root: &Path, layout: &str) -> bool {
    root.join(PPT_MASTER_LAYOUTS_DIR)
        .join(layout)
        .join("design_spec.md")
        .is_file()
}

fn ppt_master_chart_exists(root: &Path, chart: &str) -> bool {
    root.join(PPT_MASTER_CHARTS_DIR)
        .join(format!("{}.svg", chart))
        .is_file()
}

fn init_project_with_project_manager(
    root: &Path,
    python_path: &str,
    title: &str,
    log_lines: &mut Vec<String>,
) -> Result<PathBuf, AppError> {
    let projects_dir = root.join("projects");
    create_dir_all(&projects_dir)?;
    let base_name = format!(
        "pome_ppt_{}_{}",
        chrono::Local::now().format("%H%M%S"),
        safe_filename(title, "deck")
    );
    let project_name = safe_filename(&base_name, "pome_ppt");
    let project_path = projects_dir.join(format!(
        "{}_ppt169_{}",
        project_name,
        chrono::Local::now().format("%Y%m%d")
    ));
    let script_path = root.join(PROJECT_MANAGER_SCRIPT);
    let python = resolve_python_program(root, python_path);
    let mut cmd = Command::new(&python);
    cmd.current_dir(root)
        .arg(&script_path)
        .arg("init")
        .arg(&project_name)
        .arg("--format")
        .arg("ppt169")
        .arg("--dir")
        .arg(&projects_dir);
    add_no_window(&mut cmd);
    let output = cmd.output().map_err(|e| {
        AppError::Custom(format!(
            "无法启动 project_manager.py: {} ({})",
            python.display(),
            e
        ))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    log_lines.push("project_manager initialized".to_string());
    if !stdout.trim().is_empty() {
        log_lines.push(stdout.trim().to_string());
    }
    if !output.status.success() {
        return Err(AppError::Custom(format!(
            "project_manager.py init 失败: {}",
            stderr.trim()
        )));
    }
    if !project_path.is_dir() {
        return Err(AppError::NotFound(format!(
            "project_manager.py 未创建预期项目目录: {}",
            project_path.display()
        )));
    }
    Ok(project_path)
}

fn copy_layout_templates(
    root: &Path,
    project: &Path,
    mapping: &PptMasterStyleMapping,
    log_lines: &mut Vec<String>,
) -> Result<(), AppError> {
    if mapping.layout_bias.is_empty() {
        return Ok(());
    }
    let templates = project.join("templates");
    create_dir_all(&templates)?;
    for layout in &mapping.layout_bias {
        let layout_dir = root.join(PPT_MASTER_LAYOUTS_DIR).join(layout);
        if !layout_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&layout_dir)? {
            let entry = entry?;
            let path = entry.path();
            let is_svg = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("svg"))
                .unwrap_or(false);
            if is_svg {
                let dest = templates.join(entry.file_name());
                fs::copy(&path, &dest).map_err(|e| {
                    AppError::Custom(format!("复制 layout 模板失败: {} ({})", path.display(), e))
                })?;
            }
        }
        log_lines.push(format!("layout template copied: {}", layout));
    }
    Ok(())
}

async fn ppt_ai_chat_with_timeout(
    db: &Database,
    input: PluginAiChatInput,
    context: &str,
) -> Result<String, AppError> {
    // Keep this guard in the generic entry point so a newly added native SVG repair
    // cannot silently fall back to the legacy 120-second PPT AI timeout.
    if is_native_svg_repair_request_id(&input.request_id) {
        let prompt_chars = input
            .messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum();
        let svg_file = context
            .rsplit_once(['：', ':'])
            .map(|(_, value)| value.trim())
            .filter(|value| value.to_ascii_lowercase().ends_with(".svg"))
            .unwrap_or("unknown.svg");
        return ppt_native_svg_repair_chat_with_timeout(
            db,
            input,
            context,
            svg_file,
            0,
            prompt_chars,
        )
        .await;
    }
    ppt_ai_chat_with_policy(
        db,
        input,
        context,
        PptAiRequestPolicy {
            timeout_secs: AI_PPT_TIMEOUT_SECS,
            max_output_tokens: None,
            svg_chars: None,
            prompt_chars: None,
            failure_type: None,
            timeout_source: "hardcoded",
            svg_file: None,
            disable_thinking: false,
            force_json_output: false,
        },
    )
    .await
}

async fn ppt_native_generation_chat_with_timeout(
    db: &Database,
    input: PluginAiChatInput,
    context: &str,
) -> Result<String, AppError> {
    let force_json_output = input.request_id.starts_with("ppt_master_agent_design_plan");
    let prompt_chars = input
        .messages
        .iter()
        .map(|message| message.content.chars().count())
        .sum();
    ppt_ai_chat_with_policy(
        db,
        input,
        context,
        PptAiRequestPolicy {
            timeout_secs: NATIVE_AI_TIMEOUT_SECS,
            max_output_tokens: Some(NATIVE_GENERATION_MAX_OUTPUT_TOKENS),
            svg_chars: None,
            prompt_chars: Some(prompt_chars),
            failure_type: Some("native_ai_timeout"),
            timeout_source: "ppt_master_native",
            svg_file: None,
            disable_thinking: true,
            force_json_output,
        },
    )
    .await
}

async fn ppt_native_structured_chat_with_timeout(
    db: &Database,
    input: PluginAiChatInput,
    context: &str,
    max_output_tokens: i64,
) -> Result<PptAiResponse, AppError> {
    let prompt_chars = input
        .messages
        .iter()
        .map(|message| message.content.chars().count())
        .sum();
    ppt_ai_chat_with_policy_detailed(
        db,
        input,
        context,
        PptAiRequestPolicy {
            timeout_secs: NATIVE_AI_TIMEOUT_SECS,
            max_output_tokens: Some(max_output_tokens),
            svg_chars: None,
            prompt_chars: Some(prompt_chars),
            failure_type: Some("native_planning_ai_failed"),
            timeout_source: "ppt_master_native_structured_planning",
            svg_file: None,
            disable_thinking: true,
            force_json_output: true,
        },
    )
    .await
}

#[derive(Debug, Clone)]
struct PptAiRequestPolicy {
    timeout_secs: u64,
    max_output_tokens: Option<i64>,
    svg_chars: Option<usize>,
    prompt_chars: Option<usize>,
    failure_type: Option<&'static str>,
    timeout_source: &'static str,
    svg_file: Option<String>,
    disable_thinking: bool,
    force_json_output: bool,
}

#[derive(Debug, Clone)]
struct PptAiResponse {
    content: String,
    input_characters: usize,
    estimated_input_tokens: usize,
    output_characters: usize,
    elapsed_ms: u128,
    finish_reason: Option<String>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

async fn ppt_ai_chat_with_policy(
    db: &Database,
    input: PluginAiChatInput,
    context: &str,
    policy: PptAiRequestPolicy,
) -> Result<String, AppError> {
    let response = ppt_ai_chat_with_policy_detailed(db, input, context, policy).await?;
    if response.finish_reason.as_deref() == Some("length") {
        return Err(AppError::Custom(format!(
            "{context} stopped with finish_reason=length; outputCharacters={}",
            response.output_characters
        )));
    }
    Ok(response.content)
}

async fn ppt_ai_chat_with_policy_detailed(
    db: &Database,
    input: PluginAiChatInput,
    context: &str,
    policy: PptAiRequestPolicy,
) -> Result<PptAiResponse, AppError> {
    let model = match input.model_id {
        Some(id) => db.get_ai_model(id)?,
        None => db.get_default_ai_model()?,
    };
    let max_output_tokens = policy.max_output_tokens.map(|requested| {
        model
            .max_output_tokens
            .filter(|configured| *configured > 0)
            .map(|configured| requested.min(configured))
            .unwrap_or(requested)
    });
    let request_chars = input
        .messages
        .iter()
        .map(|message| message.role.chars().count() + message.content.chars().count() + 16)
        .sum::<usize>();
    let estimated_tokens = input
        .messages
        .iter()
        .map(|message| estimate_mixed_text_tokens(&message.content))
        .sum::<usize>();
    let progress = AiRequestProgress::default();
    let started = Instant::now();
    println!(
        "[PPT AI Request] context={} resolvedTimeoutSeconds={} timeoutSource={} svgFile={} thinkingMode={} responseFormat={} stage=request_start requestCharacters={} modelId={} model_db_id={} provider={} api_url={} started_at={} estimated_tokens={} svg_chars={} prompt_chars={} max_output_tokens={} stream={}",
        context,
        policy.timeout_secs,
        policy.timeout_source,
        policy.svg_file.as_deref().unwrap_or("n/a"),
        if policy.disable_thinking { "disabled" } else { "provider_default" },
        if policy.force_json_output { "json_object" } else { "text" },
        request_chars,
        model.model_id,
        model.id,
        model.provider,
        sanitize_api_url_for_log(&model.api_url),
        chrono::Utc::now().to_rfc3339(),
        estimated_tokens,
        policy
            .svg_chars
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        policy
            .prompt_chars
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        max_output_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "model_default".to_string()),
        !policy.force_json_output,
    );
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let result = await_ppt_ai_network(
        AiService::plugin_chat_sync_with_options(
            db,
            input,
            cancel_rx,
            max_output_tokens,
            Some(&progress),
            policy.disable_thinking,
            policy.force_json_output,
        ),
        policy.timeout_secs,
        context,
        policy.failure_type,
        &progress,
    )
    .await;
    let snapshot = progress.snapshot();
    let elapsed_ms = started.elapsed().as_millis();
    let output_characters = result
        .as_ref()
        .map(|content| content.chars().count())
        .unwrap_or(0);
    println!(
        "[PPT AI Response] context={} elapsed_ms={} response_headers_received={} first_response_received={} partial_response_received={} stream_completed={} first_response_after_ms={} partial_response_after_ms={} finish_reason={} prompt_tokens={} completion_tokens={} total_tokens={} response_characters={} result={}",
        context,
        elapsed_ms,
        snapshot.response_headers_received,
        snapshot.first_response_received,
        snapshot.partial_response_received,
        snapshot.stream_completed,
        snapshot
            .first_response_after_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        snapshot
            .partial_response_after_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        snapshot.finish_reason.as_deref().unwrap_or("n/a"),
        snapshot
            .prompt_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        snapshot
            .completion_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        snapshot
            .total_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
        snapshot.response_characters.unwrap_or(output_characters),
        if result.is_ok() { "ok" } else { "error" },
    );
    result.map(|content| PptAiResponse {
        content,
        input_characters: request_chars,
        estimated_input_tokens: estimated_tokens,
        output_characters,
        elapsed_ms,
        finish_reason: snapshot.finish_reason,
        prompt_tokens: snapshot.prompt_tokens,
        completion_tokens: snapshot.completion_tokens,
        total_tokens: snapshot.total_tokens,
    })
}

async fn await_ppt_ai_network<F>(
    future: F,
    timeout_secs: u64,
    context: &str,
    failure_type: Option<&str>,
    progress: &AiRequestProgress,
) -> Result<String, AppError>
where
    F: std::future::Future<Output = Result<String, AppError>>,
{
    match timeout(Duration::from_secs(timeout_secs), future).await {
        Ok(result) => result,
        Err(_) => {
            let snapshot = progress.snapshot();
            let stage = if !snapshot.response_headers_received {
                "connect_or_wait_response_headers"
            } else if !snapshot.first_response_received {
                "wait_first_body_chunk"
            } else if snapshot.partial_response_received && !snapshot.stream_completed {
                "read_stream_completion"
            } else {
                "read_response"
            };
            let prefix = failure_type
                .map(|value| format!("{}: ", value))
                .unwrap_or_default();
            Err(AppError::Custom(format!(
                "{}{} 超时：超过 {} 秒，stage={}，已停止生成。",
                prefix, context, timeout_secs, stage
            )))
        }
    }
}

fn estimate_mixed_text_tokens(value: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for character in value.chars() {
        if ('\u{3400}'..='\u{9fff}').contains(&character) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + (other + 3) / 4
}

fn sanitize_api_url_for_log(value: &str) -> String {
    value
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_string()
}

fn effective_understanding_draft(input: &PptMasterGenerateInput) -> PptUnderstandingDraftInput {
    let mut draft = match input.ai_understanding_result.as_ref() {
        Some(PptUnderstandingInput::Structured(value)) => value.clone(),
        _ => PptUnderstandingDraftInput::default(),
    };
    let overwrite = |target: &mut String, value: &Option<String>| {
        if target.trim().is_empty() {
            if let Some(value) = trimmed_option(value) {
                *target = value.to_string();
            }
        }
    };
    overwrite(
        &mut draft.understanding_summary,
        &input.understanding_summary,
    );
    overwrite(&mut draft.key_priorities, &input.key_priorities);
    overwrite(&mut draft.narrative_mainline, &input.narrative_mainline);
    overwrite(
        &mut draft.suggested_page_structure,
        &input.suggested_page_structure,
    );
    overwrite(
        &mut draft.visual_expression_advice,
        &input.visual_expression_advice,
    );
    overwrite(&mut draft.open_questions, &input.open_questions);
    draft
}

fn format_understanding_draft(draft: &PptUnderstandingDraftInput) -> Option<String> {
    let sections = [
        ("AI 理解摘要", draft.understanding_summary.trim()),
        ("重点取舍", draft.key_priorities.trim()),
        ("叙事主线", draft.narrative_mainline.trim()),
        ("建议页面结构", draft.suggested_page_structure.trim()),
        ("视觉与表达建议", draft.visual_expression_advice.trim()),
        ("仍需确认的问题", draft.open_questions.trim()),
    ];
    if sections.iter().all(|(_, value)| value.is_empty()) {
        return None;
    }
    Some(
        sections
            .into_iter()
            .map(|(title, value)| format!("## {}\n{}", title, value))
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}

fn legacy_understanding_text(input: &PptMasterGenerateInput) -> Option<&str> {
    match input.ai_understanding_result.as_ref() {
        Some(PptUnderstandingInput::Legacy(value)) => {
            let value = value.trim();
            (!value.is_empty()).then_some(value)
        }
        _ => None,
    }
}

fn has_authoritative_generation_input(input: &PptMasterGenerateInput) -> bool {
    format_understanding_draft(&effective_understanding_draft(input)).is_some()
        || has_text(&input.planning_context)
        || has_text(&input.raw_material)
        || has_text(&input.extra_requirements)
}

fn build_generation_planning_context(
    input: &PptMasterGenerateInput,
    compatibility_prompt: &str,
) -> String {
    let mut parts = Vec::new();
    let understanding = format_understanding_draft(&effective_understanding_draft(input));
    if let Some(value) = understanding.as_deref() {
        parts.push(format!(
            "[User-Edited Structured AI Understanding]\n{}",
            value
        ));
    }
    if let Some(value) = trimmed_option(&input.planning_context) {
        if understanding
            .as_deref()
            .is_none_or(|draft| draft.trim() != value)
        {
            parts.push(format!("[Planning Context Mirror]\n{}", value));
        }
    }
    if let Some(value) = legacy_understanding_text(input) {
        parts.push(format!("[Legacy AI Understanding Result]\n{}", value));
    }
    if let Some(value) = trimmed_option(&input.audience) {
        parts.push(format!("[Audience]\n{}", value));
    }
    if let Some(value) = trimmed_option(&input.raw_material) {
        parts.push(format!(
            "[Raw Material - authoritative source, do not invent facts outside this]\n{}",
            value
        ));
    }
    if let Some(value) = trimmed_option(&input.extra_requirements) {
        parts.push(format!("[Extra Requirements]\n{}", value));
    }
    if !compatibility_prompt.trim().is_empty() {
        parts.push(format!(
            "[Legacy Prompt - compatibility only, never override structured understanding]\n{}",
            compatibility_prompt.trim()
        ));
    }
    parts.push(
        "[Fact Safety Rules]\n\
         - Do not invent exact numbers, rankings, awards, academic ratings, fellow counts, laboratory counts, or years.\n\
         - If the raw material does not explicitly provide a number/ranking/award count, write it as illustrative, replaceable, representative, or long-term accumulation.\n\
         - Forbidden definite claims unless explicitly sourced: 全国唯一, 全国第一, 连续三年, 连续五年, 20+, 6个国家级, 国家科技一等奖.\n\
         - Do not repeat any domain-specific keyword as a selling point unless it is explicitly central to the raw material.\n"
            .to_string(),
    );
    parts.join("\n\n")
}

fn build_stable_visible_material(
    input: &PptMasterGenerateInput,
    compatibility_prompt: &str,
) -> String {
    let mut parts = Vec::new();
    if let Some(value) = trimmed_option(&input.raw_material) {
        parts.push(value.to_string());
    } else {
        let draft = effective_understanding_draft(input);
        for value in [
            draft.understanding_summary,
            draft.key_priorities,
            draft.narrative_mainline,
        ] {
            if !value.trim().is_empty() {
                parts.push(value);
            }
        }
        if let Some(value) = trimmed_option(&input.planning_context) {
            parts.push(value.to_string());
        }
        if let Some(value) = legacy_understanding_text(input) {
            parts.push(value.to_string());
        }
    }
    if let Some(value) = trimmed_option(&input.extra_requirements) {
        parts.push(value.to_string());
    }
    if parts.is_empty() && !compatibility_prompt.trim().is_empty() {
        parts.push(compatibility_prompt.trim().to_string());
    }
    parts
        .into_iter()
        .map(|value| sanitize_visible_text(&value))
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn trimmed_option(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|item| !item.is_empty())
}

fn has_text(value: &Option<String>) -> bool {
    trimmed_option(value).is_some()
}

fn planning_context_has(context: &str, needle: &str) -> bool {
    context.contains(needle)
}

fn log_planning_input(
    input: &PptMasterGenerateInput,
    compatibility_prompt: &str,
    planning_context: &str,
    slide_count: usize,
    style: &str,
    log_lines: &mut Vec<String>,
) {
    let understanding = effective_understanding_draft(input);
    let raw_material_length = input
        .raw_material
        .as_deref()
        .map(|value| value.chars().count())
        .unwrap_or(0);
    let rows = [
        "[Planning Input]".to_string(),
        format!("topic={}", input.title.as_deref().unwrap_or("")),
        format!("audience={}", input.audience.as_deref().unwrap_or("")),
        format!("style={}", style),
        format!("slideCount={}", slide_count),
        format!("rawMaterialLength={}", raw_material_length),
        format!("materialSourceCount={}", input.material_sources.len()),
        format!(
            "hasStructuredUnderstanding={}",
            format_understanding_draft(&understanding).is_some()
        ),
        format!(
            "hasLegacyPrompt={}",
            !compatibility_prompt.trim().is_empty()
        ),
        format!("hasPlanningContext={}", has_text(&input.planning_context)),
        format!(
            "hasUnderstandingSummary={}",
            !understanding.understanding_summary.trim().is_empty()
        ),
        format!(
            "hasKeyPriorities={}",
            !understanding.key_priorities.trim().is_empty()
        ),
        format!(
            "hasSuggestedPageStructure={}",
            !understanding.suggested_page_structure.trim().is_empty()
                || has_text(&input.suggested_page_structure)
                || planning_context_has(planning_context, "Suggested Page Structure")
                || planning_context_has(planning_context, "建议页面结构")
        ),
        format!(
            "hasNarrativeMainline={}",
            !understanding.narrative_mainline.trim().is_empty()
                || has_text(&input.narrative_mainline)
                || planning_context_has(planning_context, "Narrative Mainline")
                || planning_context_has(planning_context, "叙事主线")
        ),
        format!(
            "hasVisualAdvice={}",
            !understanding.visual_expression_advice.trim().is_empty()
                || has_text(&input.visual_expression_advice)
                || planning_context_has(planning_context, "Visual Expression Advice")
                || planning_context_has(planning_context, "视觉")
        ),
    ];
    for row in rows {
        println!("{}", row);
        log_lines.push(row);
    }
}

#[allow(clippy::too_many_arguments)]
async fn generate_native_structured_slide_plan(
    db: &Database,
    input: &PptMasterGenerateInput,
    project: &Path,
    input_fingerprint: &str,
    title: &str,
    audience: &str,
    slide_count: usize,
    style: &str,
    theme_spec: &NativeThemeSpec,
    model_id: Option<i64>,
    log_lines: &mut Vec<String>,
) -> Result<SlidePlan, AppError> {
    let mut checkpoint = load_or_create_checkpoint(project, input_fingerprint, slide_count)
        .map_err(AppError::Custom)?;
    if let Some(existing) = checkpoint.theme_spec.as_ref() {
        if existing != theme_spec {
            return Err(AppError::Custom(
                "native planning checkpoint theme does not match current input fingerprint"
                    .to_string(),
            ));
        }
    } else {
        checkpoint.theme_spec = Some(theme_spec.clone());
        persist_native_planning_checkpoint(project, &checkpoint).map_err(AppError::Custom)?;
    }
    let raw_material = input.raw_material.as_deref().unwrap_or("");
    let material_index = NativeMaterialIndex::new(raw_material);
    let deck_context =
        build_native_deck_outline_context(input, title, audience, slide_count, style, theme_spec);
    log_lines.push(format!(
        "[Native Planning Context] phase=deck_outline rawMaterialCharacters={} materialUnits={} contextCharacters={} fullRawMaterialInjected=false",
        material_index.raw_characters(),
        material_index.unit_count(),
        deck_context.chars().count()
    ));

    let mut outline_was_generated = false;
    let outline = if checkpoint.outline.status == "validated" {
        match read_outline(project, slide_count) {
            Ok(outline) => {
                log_lines.push(format!(
                    "[Native Planning Resume] phase=deck_outline reused=true path={}",
                    checkpoint.outline.path
                ));
                outline
            }
            Err(error) => {
                checkpoint.outline.status = "pending".to_string();
                checkpoint.outline.attempts = 0;
                checkpoint.outline.last_error_kind = Some(
                    NativePlanningErrorKind::SchemaValidation
                        .as_str()
                        .to_string(),
                );
                checkpoint.outline.last_error = Some(error);
                checkpoint.outline.updated_at = native_state_now();
                persist_native_planning_checkpoint(project, &checkpoint)
                    .map_err(AppError::Custom)?;
                outline_was_generated = true;
                generate_native_deck_outline(
                    db,
                    project,
                    slide_count,
                    model_id,
                    &deck_context,
                    &mut checkpoint,
                )
                .await?
            }
        }
    } else {
        outline_was_generated = true;
        generate_native_deck_outline(
            db,
            project,
            slide_count,
            model_id,
            &deck_context,
            &mut checkpoint,
        )
        .await?
    };

    if outline_was_generated {
        for index in 1..=slide_count {
            let state = checkpoint.slide_mut(index, project);
            state.status = "pending".to_string();
            state.attempts = 0;
            state.last_error_kind = None;
            state.last_error = None;
            state.updated_at = native_state_now();
            let path = native_planning::slide_spec_path(project, index);
            if path.is_file() {
                fs::remove_file(&path).map_err(|error| {
                    AppError::Custom(format!(
                        "invalidate stale SlideSpec failed: {} ({error})",
                        path.display()
                    ))
                })?;
            }
        }
        persist_native_planning_checkpoint(project, &checkpoint).map_err(AppError::Custom)?;
    }

    let outline_json = serde_json::to_string(&outline)
        .map_err(|error| AppError::Custom(format!("serialize DeckOutline failed: {error}")))?;
    let mut specs = Vec::with_capacity(slide_count);
    for outline_slide in &outline.slides {
        let page_index = outline_slide.index;
        let reusable = checkpoint
            .slide_specs
            .get(&page_index.to_string())
            .is_some_and(|state| state.status == "validated");
        if reusable {
            match read_slide_spec(project, page_index) {
                Ok(spec) => {
                    log_lines.push(format!(
                        "[Native Planning Resume] phase=slide_spec page=P{page_index:02} reused=true"
                    ));
                    specs.push(spec);
                    continue;
                }
                Err(error) => {
                    let state = checkpoint.slide_mut(page_index, project);
                    state.status = "pending".to_string();
                    state.attempts = 0;
                    state.last_error_kind = Some(
                        NativePlanningErrorKind::SchemaValidation
                            .as_str()
                            .to_string(),
                    );
                    state.last_error = Some(error);
                    state.updated_at = native_state_now();
                    persist_native_planning_checkpoint(project, &checkpoint)
                        .map_err(AppError::Custom)?;
                }
            }
        }

        let evidence_query = format!(
            "{} {} {}",
            outline_slide.evidence_query, outline_slide.title, outline_slide.core_message
        );
        let evidence = material_index.retrieve(&evidence_query, page_index, slide_count);
        let spec_context = build_native_slide_spec_context(
            &outline_json,
            outline_slide,
            &evidence,
            audience,
            style,
            theme_spec,
        );
        log_lines.push(format!(
            "[Native Planning Context] phase=slide_spec page=P{page_index:02} retrievedUnits={} retrievedCharacters={} contextCharacters={} fullRawMaterialInjected=false",
            evidence.len(),
            evidence.iter().map(|item| item.chars().count()).sum::<usize>(),
            spec_context.chars().count()
        ));
        let spec = generate_native_slide_spec(
            db,
            project,
            page_index,
            model_id,
            &spec_context,
            &mut checkpoint,
        )
        .await?;
        specs.push(spec);
    }

    checkpoint.status = "validated".to_string();
    checkpoint.updated_at = native_state_now();
    persist_native_planning_checkpoint(project, &checkpoint).map_err(AppError::Custom)?;
    Ok(assemble_slide_plan(&outline, &specs, audience, style))
}

async fn generate_native_deck_outline(
    db: &Database,
    project: &Path,
    slide_count: usize,
    model_id: Option<i64>,
    original_prompt: &str,
    checkpoint: &mut NativePlanningCheckpoint,
) -> Result<DeckOutline, AppError> {
    let start_attempt = checkpoint.outline.attempts.saturating_add(1);
    let mut previous_error = checkpoint.outline.last_error.clone();
    for attempt in start_attempt..=NATIVE_PLANNING_MAX_ATTEMPTS {
        let prompt = native_planning_attempt_prompt(original_prompt, previous_error.as_deref());
        checkpoint.outline.status = "generating".to_string();
        checkpoint.outline.attempts = attempt;
        checkpoint.outline.updated_at = native_state_now();
        persist_native_planning_checkpoint(project, checkpoint).map_err(AppError::Custom)?;
        let request_id = format!("ppt_master_agent_design_plan_outline_{attempt}");
        let input = PluginAiChatInput {
            request_id: request_id.clone(),
            model_id,
            messages: vec![PluginAiMessage {
                role: "user".to_string(),
                content: prompt.clone(),
            }],
        };
        let request_started = Instant::now();
        let response = ppt_native_structured_chat_with_timeout(
            db,
            input,
            &format!("DeckOutline attempt {attempt}/{NATIVE_PLANNING_MAX_ATTEMPTS}"),
            4_096,
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let contract_error = NativePlanningContractError {
                    kind: NativePlanningErrorKind::Network,
                    summary: error.to_string(),
                };
                checkpoint.record_metric(native_planning_error_metric(
                    request_id,
                    "deck_outline",
                    None,
                    attempt,
                    &prompt,
                    request_started.elapsed().as_millis(),
                    &contract_error,
                ));
                record_outline_contract_failure(checkpoint, &contract_error);
                persist_native_planning_checkpoint(project, checkpoint)
                    .map_err(AppError::Custom)?;
                previous_error = Some(contract_error.summary.clone());
                if attempt == NATIVE_PLANNING_MAX_ATTEMPTS {
                    return Err(native_planning_contract_app_error(
                        "deck_outline",
                        None,
                        &contract_error,
                    ));
                }
                continue;
            }
        };
        if let Some(contract_error) = planning_finish_reason_error(&response) {
            checkpoint.record_metric(native_planning_response_metric(
                request_id,
                "deck_outline",
                None,
                attempt,
                &response,
                Some(&contract_error),
            ));
            record_outline_contract_failure(checkpoint, &contract_error);
            persist_native_planning_checkpoint(project, checkpoint).map_err(AppError::Custom)?;
            previous_error = Some(contract_error.summary.clone());
            if attempt == NATIVE_PLANNING_MAX_ATTEMPTS {
                return Err(native_planning_contract_app_error(
                    "deck_outline",
                    None,
                    &contract_error,
                ));
            }
            continue;
        }
        match parse_deck_outline(&response.content, slide_count) {
            Ok(outline) => {
                write_outline(project, &outline).map_err(AppError::Custom)?;
                checkpoint.record_metric(native_planning_response_metric(
                    request_id,
                    "deck_outline",
                    None,
                    attempt,
                    &response,
                    None,
                ));
                checkpoint.outline.status = "validated".to_string();
                checkpoint.outline.last_error_kind = None;
                checkpoint.outline.last_error = None;
                checkpoint.outline.updated_at = native_state_now();
                persist_native_planning_checkpoint(project, checkpoint)
                    .map_err(AppError::Custom)?;
                return Ok(outline);
            }
            Err(contract_error) => {
                checkpoint.record_metric(native_planning_response_metric(
                    request_id,
                    "deck_outline",
                    None,
                    attempt,
                    &response,
                    Some(&contract_error),
                ));
                record_outline_contract_failure(checkpoint, &contract_error);
                persist_native_planning_checkpoint(project, checkpoint)
                    .map_err(AppError::Custom)?;
                previous_error = Some(contract_error.summary.clone());
                if attempt == NATIVE_PLANNING_MAX_ATTEMPTS {
                    return Err(native_planning_contract_app_error(
                        "deck_outline",
                        None,
                        &contract_error,
                    ));
                }
            }
        }
    }
    Err(AppError::Custom(
        "native planning DeckOutline retry budget exhausted".to_string(),
    ))
}

async fn generate_native_slide_spec(
    db: &Database,
    project: &Path,
    page_index: usize,
    model_id: Option<i64>,
    original_prompt: &str,
    checkpoint: &mut NativePlanningCheckpoint,
) -> Result<SlideSpec, AppError> {
    let start_attempt = checkpoint
        .slide_specs
        .get(&page_index.to_string())
        .map(|state| state.attempts.saturating_add(1))
        .unwrap_or(1);
    let mut previous_error = checkpoint
        .slide_specs
        .get(&page_index.to_string())
        .and_then(|state| state.last_error.clone());
    for attempt in start_attempt..=NATIVE_PLANNING_MAX_ATTEMPTS {
        let prompt = native_planning_attempt_prompt(original_prompt, previous_error.as_deref());
        {
            let state = checkpoint.slide_mut(page_index, project);
            state.status = "generating".to_string();
            state.attempts = attempt;
            state.updated_at = native_state_now();
        }
        persist_native_planning_checkpoint(project, checkpoint).map_err(AppError::Custom)?;
        let request_id = format!("ppt_master_agent_design_plan_slide_{page_index:02}_{attempt}");
        let input = PluginAiChatInput {
            request_id: request_id.clone(),
            model_id,
            messages: vec![PluginAiMessage {
                role: "user".to_string(),
                content: prompt.clone(),
            }],
        };
        let request_started = Instant::now();
        let response = ppt_native_structured_chat_with_timeout(
            db,
            input,
            &format!("SlideSpec P{page_index:02} attempt {attempt}/{NATIVE_PLANNING_MAX_ATTEMPTS}"),
            4_096,
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let contract_error = NativePlanningContractError {
                    kind: NativePlanningErrorKind::Network,
                    summary: error.to_string(),
                };
                checkpoint.record_metric(native_planning_error_metric(
                    request_id,
                    "slide_spec",
                    Some(page_index),
                    attempt,
                    &prompt,
                    request_started.elapsed().as_millis(),
                    &contract_error,
                ));
                record_slide_contract_failure(checkpoint, project, page_index, &contract_error);
                persist_native_planning_checkpoint(project, checkpoint)
                    .map_err(AppError::Custom)?;
                previous_error = Some(contract_error.summary.clone());
                if attempt == NATIVE_PLANNING_MAX_ATTEMPTS {
                    return Err(native_planning_contract_app_error(
                        "slide_spec",
                        Some(page_index),
                        &contract_error,
                    ));
                }
                continue;
            }
        };
        if let Some(contract_error) = planning_finish_reason_error(&response) {
            checkpoint.record_metric(native_planning_response_metric(
                request_id,
                "slide_spec",
                Some(page_index),
                attempt,
                &response,
                Some(&contract_error),
            ));
            record_slide_contract_failure(checkpoint, project, page_index, &contract_error);
            persist_native_planning_checkpoint(project, checkpoint).map_err(AppError::Custom)?;
            previous_error = Some(contract_error.summary.clone());
            if attempt == NATIVE_PLANNING_MAX_ATTEMPTS {
                return Err(native_planning_contract_app_error(
                    "slide_spec",
                    Some(page_index),
                    &contract_error,
                ));
            }
            continue;
        }
        match parse_slide_spec(&response.content, page_index) {
            Ok(spec) => {
                write_slide_spec(project, page_index, &spec).map_err(AppError::Custom)?;
                checkpoint.record_metric(native_planning_response_metric(
                    request_id,
                    "slide_spec",
                    Some(page_index),
                    attempt,
                    &response,
                    None,
                ));
                {
                    let state = checkpoint.slide_mut(page_index, project);
                    state.status = "validated".to_string();
                    state.last_error_kind = None;
                    state.last_error = None;
                    state.updated_at = native_state_now();
                }
                persist_native_planning_checkpoint(project, checkpoint)
                    .map_err(AppError::Custom)?;
                return Ok(spec);
            }
            Err(contract_error) => {
                checkpoint.record_metric(native_planning_response_metric(
                    request_id,
                    "slide_spec",
                    Some(page_index),
                    attempt,
                    &response,
                    Some(&contract_error),
                ));
                record_slide_contract_failure(checkpoint, project, page_index, &contract_error);
                persist_native_planning_checkpoint(project, checkpoint)
                    .map_err(AppError::Custom)?;
                previous_error = Some(contract_error.summary.clone());
                if attempt == NATIVE_PLANNING_MAX_ATTEMPTS {
                    return Err(native_planning_contract_app_error(
                        "slide_spec",
                        Some(page_index),
                        &contract_error,
                    ));
                }
            }
        }
    }
    Err(AppError::Custom(format!(
        "native planning SlideSpec P{page_index:02} retry budget exhausted"
    )))
}

fn build_native_deck_outline_context(
    input: &PptMasterGenerateInput,
    title: &str,
    audience: &str,
    slide_count: usize,
    style: &str,
    theme_spec: &NativeThemeSpec,
) -> String {
    let understanding = format_understanding_draft(&effective_understanding_draft(input))
        .unwrap_or_else(|| "No structured understanding was provided.".to_string());
    let extra_requirements = trimmed_option(&input.extra_requirements).unwrap_or("none");
    let schema = deck_outline_schema().to_string();
    format!(
        "Return JSON only for a DeckOutline. Do not output markdown, SVG, long body copy, coordinates, or full source material.\n\
         Required JSON Schema (enforced again by Rust; unknown fields are rejected):\n{schema}\n\n\
         Planning rules:\n\
         - page_count must equal {slide_count}; indices must be unique and continuous from 1.\n\
         - slide 1 must be cover; later slides must not be cover.\n\
         - Every title and core_message must be distinct.\n\
         - evidence_query is a concise retrieval query for local source selection, not visible copy.\n\
         - Build a coherent narrative for the requested subject and audience.\n\
         - Do not invent facts, numbers, dates, rankings, or quotations.\n\
         - Use only these slide_type enum values: cover, section, overview, timeline, process, comparison, data, quote, image, profile, content, summary.\n\n\
         deck_title: {title}\nAudience: {audience}\nStyle: {style}\nExtra requirements: {extra_requirements}\n\
         Deck-wide NativeThemeSpec (planning context only; never render field names as visible copy):\n{theme_contract}\n\n\
         Structured material summary:\n{understanding}"
        ,
        theme_contract = theme_spec.prompt_contract()
    )
}

fn build_native_slide_spec_context(
    outline_json: &str,
    outline_slide: &native_planning::DeckOutlineSlide,
    evidence: &[String],
    audience: &str,
    style: &str,
    theme_spec: &NativeThemeSpec,
) -> String {
    let slide_json = serde_json::to_string(outline_slide).unwrap_or_else(|_| "{}".to_string());
    let evidence_json = serde_json::to_string(evidence).unwrap_or_else(|_| "[]".to_string());
    let schema = slide_spec_schema().to_string();
    format!(
        "Return JSON only for exactly one SlideSpec. Do not output markdown, SVG, coordinates, or fields outside the Schema.\n\
         Required JSON Schema (enforced again by Rust; unknown fields are rejected):\n{schema}\n\n\
         Rules:\n\
         - index must match the requested page.\n\
         - visible_content contains 1-6 concise, presentation-ready strings grounded in retrieved evidence.\n\
         - evidence contains 1-8 faithful source facts or paraphrases; do not invent details.\n\
         - speaker_notes may expand context but must stay within the Schema limit.\n\
         - Use one layout_intent enum value: hero, section, editorial_split, timeline, process, comparison, data_focus, quote_focus, image_focus, profile, card_grid, summary.\n\
         - Preserve the current page's role and avoid taking over adjacent pages.\n\n\
         Audience: {audience}\nStyle: {style}\nDeck-wide NativeThemeSpec (use as visual guidance only; do not output it as fields or visible text): {theme_contract}\nDeckOutline: {outline_json}\nCurrent outline slide: {slide_json}\nRetrieved local source units: {evidence_json}",
        theme_contract = theme_spec.prompt_contract()
    )
}

fn native_planning_attempt_prompt(original_prompt: &str, previous_error: Option<&str>) -> String {
    match previous_error {
        Some(error) => format!(
            "{original_prompt}\n\nSTRUCTURED RETRY (final attempt): The prior output failed validation: {}. Return a complete JSON object matching the Schema. Do not include or reconstruct the previous response.",
            truncate_for_log(error, 480)
        ),
        None => original_prompt.to_string(),
    }
}

fn planning_finish_reason_error(response: &PptAiResponse) -> Option<NativePlanningContractError> {
    match response.finish_reason.as_deref() {
        Some("stop") | None => None,
        Some(reason) => Some(NativePlanningContractError {
            kind: NativePlanningErrorKind::FinishReason,
            summary: format!(
                "finish_reason={reason}; outputCharacters={}",
                response.output_characters
            ),
        }),
    }
}

fn native_planning_response_metric(
    request_id: String,
    phase: &str,
    page_index: Option<usize>,
    attempt: usize,
    response: &PptAiResponse,
    error: Option<&NativePlanningContractError>,
) -> NativePlanningRequestMetric {
    let (json_parse_result, schema_validation_result) = match error.map(|error| error.kind) {
        None => ("passed", "passed"),
        Some(NativePlanningErrorKind::JsonSyntax) => ("failed", "not_run"),
        Some(NativePlanningErrorKind::SchemaValidation) => ("passed", "failed"),
        Some(NativePlanningErrorKind::FinishReason | NativePlanningErrorKind::Network) => {
            ("not_run", "not_run")
        }
    };
    NativePlanningRequestMetric {
        request_id,
        phase: phase.to_string(),
        page_index,
        attempt,
        input_characters: response.input_characters,
        estimated_input_tokens: response.estimated_input_tokens,
        output_characters: response.output_characters,
        elapsed_ms: response.elapsed_ms,
        finish_reason: response.finish_reason.clone(),
        prompt_tokens: response.prompt_tokens,
        completion_tokens: response.completion_tokens,
        total_tokens: response.total_tokens,
        json_parse_result: json_parse_result.to_string(),
        schema_validation_result: schema_validation_result.to_string(),
        error_kind: error.map(|error| error.kind.as_str().to_string()),
        error_summary: error.map(|error| error.summary.clone()),
    }
}

fn native_planning_error_metric(
    request_id: String,
    phase: &str,
    page_index: Option<usize>,
    attempt: usize,
    prompt: &str,
    elapsed_ms: u128,
    error: &NativePlanningContractError,
) -> NativePlanningRequestMetric {
    NativePlanningRequestMetric {
        request_id,
        phase: phase.to_string(),
        page_index,
        attempt,
        input_characters: prompt.chars().count(),
        estimated_input_tokens: estimate_mixed_text_tokens(prompt),
        output_characters: 0,
        elapsed_ms,
        finish_reason: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        json_parse_result: "not_run".to_string(),
        schema_validation_result: "not_run".to_string(),
        error_kind: Some(error.kind.as_str().to_string()),
        error_summary: Some(error.summary.clone()),
    }
}

fn record_outline_contract_failure(
    checkpoint: &mut NativePlanningCheckpoint,
    error: &NativePlanningContractError,
) {
    checkpoint.outline.status = "failed".to_string();
    checkpoint.outline.last_error_kind = Some(error.kind.as_str().to_string());
    checkpoint.outline.last_error = Some(error.summary.clone());
    checkpoint.outline.updated_at = native_state_now();
    checkpoint.status = "failed".to_string();
    checkpoint.updated_at = native_state_now();
}

fn record_slide_contract_failure(
    checkpoint: &mut NativePlanningCheckpoint,
    project: &Path,
    page_index: usize,
    error: &NativePlanningContractError,
) {
    let state = checkpoint.slide_mut(page_index, project);
    state.status = "failed".to_string();
    state.last_error_kind = Some(error.kind.as_str().to_string());
    state.last_error = Some(error.summary.clone());
    state.updated_at = native_state_now();
    checkpoint.status = "failed".to_string();
    checkpoint.updated_at = native_state_now();
}

fn native_planning_contract_app_error(
    phase: &str,
    page_index: Option<usize>,
    error: &NativePlanningContractError,
) -> AppError {
    AppError::Custom(format!(
        "native_planning_{}; stage={phase}; page={}; error={}",
        error.kind.as_str(),
        page_index
            .map(|index| format!("P{index:02}"))
            .unwrap_or_else(|| "deck".to_string()),
        error.summary
    ))
}

#[cfg(test)]
#[allow(dead_code)]
async fn generate_agent_design_plan(
    db: &Database,
    skill_text: &str,
    prompt: &str,
    model_id: Option<i64>,
    title: &str,
    slide_count: usize,
    style: &str,
) -> Result<SlidePlan, AppError> {
    let ai_prompt = format!(
        "You are the ppt-master Strategist. Return strict JSON only, no markdown.\n\
         The following prompt includes Pomegranate AI understanding sections. Treat storyline and recommended page structure as the primary planning source.\n\n\
         Required top-level JSON fields: title, subtitle, audience, style, theme, themeAllocation, slides.\n\
         theme MUST be an object with exactly these string fields: {{\"name\":\"...\",\"primary\":\"#RRGGBB\",\"secondary\":\"#RRGGBB\",\"accent\":\"#RRGGBB\",\"background\":\"#RRGGBB\"}}. Never return theme as prose or a plain string.\n\
         themeAllocation must be an array of {{pageId, assignedTheme, exclusiveScope}}. Each assignedTheme must be unique; exclusiveScope must say what this page owns and what it does not cover.\n\
         Required slide JSON fields per page: page, pageIndex, pageId, type, layout, title, subtitle, pageTheme, mainClaim, contentScope, mustInclude, mustAvoid, bullets, visualHint, pageRhythm, chartRef, chartType, fileStem, speakerNote.\n\n\
         page and pageIndex MUST be JSON integers (1, 2, 3...), never labels such as \"封面\" or \"第二页\".\n\
         mustInclude, mustAvoid, and bullets MUST always be JSON arrays of strings, including when empty or containing only one item. Never return those fields as plain strings.\n\n\
         Dynamic page planning algorithm:\n\
         - Do not treat any example structure as a fixed template.\n\
         - Infer a themePool from user topic, raw material, AI understanding, audience, style, extra requirements, and requested slideCount.\n\
         - Possible themePool items are neutral categories such as overview/core idea, background/context, concept definition, development/history, structure/relationship, key evidence, method/process, example/case, comparison, implication/value, practice/exercise, and final synthesis. Select only categories that fit the user's material.\n\
         - If slideCount <= 3, merge related themes into broad non-overlapping theme groups.\n\
         - If 4 <= slideCount <= 8, usually assign one major theme per page with clear narrative progression.\n\
         - If slideCount > 8, split into finer subthemes and add case/diagram/data/transition pages only when supported by the source.\n\
         - Fewer pages means reasonable merging; more pages means finer splitting. Never duplicate the same assignedTheme.\n\n\
         Hard content de-duplication rules:\n\
         - Each slide must have one unique pageTheme and one unique mainClaim.\n\
         - Follow recommended page structure when provided; do not collapse multiple suggested pages into the same topic.\n\
         - Build narrative progression, not parallel repetition.\n\
         - A fact point must not be repeated as the main content on adjacent pages.\n\
         - If a keyword is the focus of one page, other pages may only lightly echo it.\n\
         - Any domain-specific keyword may be the focus of only the page whose assignedTheme owns that topic. Other pages may only lightly echo it.\n\
         - Do not use product-roadshow, engineering, startup, school-promotion, research-defense, or teaching-lesson structures unless the user's material or explicit requirement calls for that genre.\n\
         - Fact safety: do not invent exact numbers, rankings, award counts, fellow counts, laboratory counts, or years. Use 示意数据/可替换数据位/代表性方向/长期积累 when the source does not provide exact data.\n\
         - Forbidden definite claims unless explicitly present in raw material: 全国唯一, 全国第一, 连续三年, 连续五年, 20+, 6个国家级, 国家科技一等奖.\n\n\
         pageRhythm must be anchor, dense, or breathing. chartRef/chartType may be timeline, vertical_pillars, pipeline_with_stages, labeled_card, hub_spoke, kpi_cards, process_flow, or none. fileStem must be ASCII only.\n\n\
         Original planning request follows:\n\nRequested slideCount: {slide_count}\nSuggested title: {title}\nSelected style: {style}\n\nppt-master rules excerpt:\n{skill_excerpt}\n\nUser planning context:\n{prompt}",
        skill_excerpt = skill_excerpt(skill_text)
    );
    request_native_slide_plan_with_json_retry(
        db,
        model_id,
        "ppt_master_agent_design_plan",
        "AI generate slide_plan",
        ai_prompt,
    )
    .await
}

#[cfg(test)]
#[allow(dead_code)]
async fn regenerate_agent_design_plan_with_dedup(
    db: &Database,
    skill_text: &str,
    planning_context: &str,
    current_plan: &SlidePlan,
    duplicate_report: &str,
    model_id: Option<i64>,
    title: &str,
    slide_count: usize,
    style: &str,
) -> Result<SlidePlan, AppError> {
    let current_json = serde_json::to_string_pretty(current_plan)
        .map_err(|e| AppError::Custom(format!("serialize slide_plan for de-dup failed: {}", e)))?;
    let ai_prompt = format!(
        "You are the ppt-master Strategist. The current slide_plan repeats content too much.\n\
         Return a corrected strict JSON slide_plan only, no markdown.\n\n\
         Duplicate report:\n{duplicate_report}\n\n\
         Mandatory fix:\n\
         - Keep {slide_count} slides.\n\
         - Preserve the user's recommended page structure from planning_context, but adapt it dynamically to the requested slideCount.\n\
         - Rebuild themeAllocation first: exactly one unique assignedTheme per page, with a clear exclusiveScope.\n\
         - If slideCount is small, merge related themes into broad non-overlapping groups; if slideCount is large, split themes into finer subthemes.\n\
         - Give every page a distinct pageTheme and mainClaim derived from its assignedTheme.\n\
         - Every mustAvoid must name concrete themes owned by other pages, not a generic warning.\n\
         - Restrict any high-frequency topic to the page whose assignedTheme owns it; other pages may only lightly echo it.\n\
         - Remove unsupported exact numbers/rankings/award counts unless the raw material explicitly provides them.\n\
         - Fill themeAllocation plus pageIndex, pageId, pageTheme, mainClaim, contentScope, mustInclude, mustAvoid, pageRhythm, chartRef, chartType, fileStem for every slide.\n\n\
         Suggested title: {title}\n\
         Suggested style: {style}\n\n\
         ppt-master rules excerpt:\n{skill_excerpt}\n\n\
         Planning context:\n{planning_context}\n\n\
         Current plan to repair:\n{current_json}",
        skill_excerpt = skill_excerpt(skill_text)
    );
    request_native_slide_plan_with_json_retry(
        db,
        model_id,
        "ppt_master_agent_design_plan_dedup",
        "AI repair slide_plan de-duplication",
        ai_prompt,
    )
    .await
}

#[cfg(test)]
fn native_plan_json_request_id(base_request_id: &str, attempt: usize) -> String {
    if attempt <= 1 {
        base_request_id.to_string()
    } else {
        format!("{base_request_id}_json_retry_{attempt}")
    }
}

#[cfg(test)]
fn build_native_plan_json_retry_prompt(original_prompt: &str, parse_error: &str) -> String {
    let parse_error = truncate_for_prompt(parse_error, 400);
    format!(
        "{original_prompt}\n\n\
         JSON CORRECTION RETRY (final attempt):\n\
         - The previous response was not valid JSON. Parser error: {parse_error}\n\
         - Return one complete JSON object only. Do not use markdown fences or commentary.\n\
         - Check every array/object separator, comma, quote, escape sequence, and closing bracket.\n\
         - Preserve all required fields and exactly the requested slide count.\n\
         - Do not abbreviate, truncate, or append text after the closing brace."
    )
}

#[cfg(test)]
#[allow(dead_code)]
async fn request_native_slide_plan_with_json_retry(
    db: &Database,
    model_id: Option<i64>,
    base_request_id: &str,
    context: &str,
    original_prompt: String,
) -> Result<SlidePlan, AppError> {
    let mut parse_errors = Vec::new();

    for attempt in 1..=NATIVE_PLAN_JSON_MAX_ATTEMPTS {
        let prompt = if attempt == 1 {
            original_prompt.clone()
        } else {
            build_native_plan_json_retry_prompt(
                &original_prompt,
                parse_errors
                    .last()
                    .map(String::as_str)
                    .unwrap_or("unknown JSON parse error"),
            )
        };
        let input = PluginAiChatInput {
            request_id: native_plan_json_request_id(base_request_id, attempt),
            model_id,
            messages: vec![PluginAiMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };
        let attempt_context =
            format!("{context} (JSON attempt {attempt}/{NATIVE_PLAN_JSON_MAX_ATTEMPTS})");
        let raw = ppt_native_generation_chat_with_timeout(db, input, &attempt_context).await?;

        match parse_native_slide_plan_json(&raw) {
            Ok(plan) => {
                println!(
                    "[Native Plan Parse] context={} attempt={}/{} status=valid responseChars={}",
                    context,
                    attempt,
                    NATIVE_PLAN_JSON_MAX_ATTEMPTS,
                    raw.chars().count()
                );
                return Ok(plan);
            }
            Err(error) => {
                let parse_error = error.to_string();
                println!(
                    "[Native Plan Parse] context={} attempt={}/{} status=invalid responseChars={} error={} action={}",
                    context,
                    attempt,
                    NATIVE_PLAN_JSON_MAX_ATTEMPTS,
                    raw.chars().count(),
                    parse_error,
                    if attempt < NATIVE_PLAN_JSON_MAX_ATTEMPTS {
                        "retry-json-only"
                    } else {
                        "fail-strict-native"
                    }
                );
                parse_errors.push(parse_error);
            }
        }
    }

    Err(AppError::Custom(format!(
        "{context} returned malformed slide_plan JSON after {NATIVE_PLAN_JSON_MAX_ATTEMPTS} attempts; no fallback was used; parse_errors={}",
        parse_errors.join(" | ")
    )))
}

async fn generate_ppt_master_driven_slide_svg(
    db: &Database,
    skill_text: &str,
    resources: &PptMasterResources,
    design_spec: &str,
    spec_lock_path: &Path,
    mapping: &PptMasterStyleMapping,
    theme_spec: &NativeThemeSpec,
    plan: &SlidePlan,
    slide: &Slide,
    prev_title: &str,
    next_title: &str,
    model_id: Option<i64>,
    retry_feedback: Option<&str>,
    density_contract: &NativePageDensityContract,
    geometry_repair_svg: Option<&str>,
    geometry_must_keep_text: Option<&[String]>,
    density_relayout_svg: Option<&str>,
    density_must_keep_text: Option<&[String]>,
) -> Result<String, AppError> {
    let spec_lock = fs::read_to_string(spec_lock_path).map_err(|e| {
        AppError::Custom(format!(
            "璇诲彇 spec_lock.md 澶辫触: {} ({})",
            spec_lock_path.display(),
            e
        ))
    })?;
    let locked_page_context = format!(
        "Current page execution lock:\n\
         - page: P{page:02}\n\
         - file_name: {file_name}\n\
         - page_rhythm: {page_rhythm}\n\
         - page_chart: {page_chart}\n\
         - mode: {mode}\n\
         - visual_style: {visual_style}\n\n\
         Mandatory per-page rules:\n\
         - Pomegranate reloaded spec_lock.md from disk immediately before this page.\n\
         - Use ONLY colors, font families, and font sizes declared in spec_lock.md.\n\
         - Do not introduce colors or font sizes outside the lock.\n\
         - Apply page_rhythm strictly: anchor = large visual center and few words; breathing = purposeful whitespace plus one strong claim and support; balanced = a main region plus supporting information across the effective canvas; dense = structured proof/chart/matrix without crowding.\n\
         - If page_chart is not none, borrow that ppt-master chart type's information structure.\n\
         - Forbidden SVG: <use>, <symbol>, <pattern>, visual defs + use references, <foreignObject>, <style>, class, filter, mask, every <clipPath> and every clip-path attribute, textPath, animation, script, iframe, external href image, rgba(), group opacity, HTML named entities such as &nbsp; &mdash; &copy;.\n\
         - {clip_path_rule}\n\
         - Repeated graphics must be expanded as real rect/path/text/circle/line/polyline/polygon elements. Never use <use href=\"#...\">.\n",
        page = slide.page,
        file_name = svg_filename_for_slide(slide),
        page_rhythm = page_rhythm_for_slide(slide),
        page_chart = chart_reference_for_slide(slide, mapping).unwrap_or_else(|| "none".to_string()),
        mode = mapping.mode,
        visual_style = mapping.visual_style,
        clip_path_rule = NATIVE_EXECUTOR_CLIP_PATH_RULE,
    );
    let text_geometry_contract = format!(
        "Native text geometry contract:\n\
         - Allocate a non-overlapping text region before writing every visible text block. Never rely on SVG automatic wrapping.\n\
         - Every visible <text> MUST declare text-anchor plus data-pome-role, data-pome-region-id, data-pome-region-x, data-pome-region-y, data-pome-region-width, data-pome-region-height, data-pome-min-font-size, data-pome-wrap, data-pome-max-lines, data-pome-line-height, and data-pome-safe-padding. Missing metadata is a hard page error.\n\
         - Allowed roles: title, subtitle, body, metric, unit, caption, label, footer. Region coordinates are absolute coordinates in the 1280×720 viewBox.\n\
         - data-pome-region-x/y/width/height describe the OUTER allocated rectangle, not the text baseline. Place region-y above the glyph ascender and reserve enough height for the full measured glyph bbox; reserve the real measured width for end-anchored footer/page-number text.\n\
         - data-pome-safe-padding is space inside that outer rectangle. A bbox outside the outer rectangle is a hard error; a bbox inside it but short of safe padding is a warning.\n\
         - Chinese multiline body text MUST use explicit tspans. The first tspan MUST have absolute x and y; later lines MUST have explicit x and dy. A first line with dy only is invalid because SVG resolves it from the implicit baseline y=0.\n\
         - data-pome-line-height MUST match the actual tspan baseline interval. For mixed-size rich text, size the region from the final aggregate browser bbox, not only the parent font size.\n\
         - Metrics, units, and explanations MUST use separate regions and must not share a baseline area.\n\
         - Keep each measured text bbox inside its declared region and keep the declared safe padding from borders. Region coordinates are in canvas space; text-anchor=start/middle/end and transform/translate must be evaluated by their final canvas bbox.\n\
         - Keep font size at or above data-pome-min-font-size. If content does not fit, shorten non-core wording before reducing only that block; never shrink the whole page.\n\
         - Mark icons or graphics that text must avoid with data-pome-obstacle=\"true\" and the matching data-pome-region-id. Use data-pome-allow-overlap=\"true\" only for intentional decorative text overlap.\n\
         - Before submitting, verify every text/tspan aggregate bbox fits its declared region and that title/body/metric/unit blocks do not overlap.\n\
         - Example: <text text-anchor=\"start\" data-pome-role=\"body\" data-pome-region-id=\"card-1-body\" data-pome-region-x=\"120\" data-pome-region-y=\"220\" data-pome-region-width=\"280\" data-pome-region-height=\"72\" data-pome-min-font-size=\"14\" data-pome-wrap=\"true\" data-pome-max-lines=\"3\" data-pome-line-height=\"20\" data-pome-safe-padding=\"10\"><tspan x=\"130\" y=\"242\">first line</tspan><tspan x=\"130\" dy=\"20\">second line</tspan></text>\n\
         {retry_feedback}",
        retry_feedback = retry_feedback
            .map(|value| format!("\nPage-only retry feedback (must be fixed):\n{}", value))
            .unwrap_or_default()
    );
    let current_page_task = format!(
        "Current page task contract:\n\
         - title: {title}\n\
         - pageTheme: {page_theme}\n\
         - mainClaim: {main_claim}\n\
         - contentScope: {content_scope}\n\
         - mustInclude: {must_include}\n\
         - mustAvoid: {must_avoid}\n\
         - pageRhythm: {page_rhythm}\n\
         - chartType: {chart_type}\n\n\
         [Universal page density contract]\n{density_contract}\n\n\
         Execution rule: render ONLY this page contract. Do not re-plan the deck. Do not pull another page's theme into this page. Use the global storyline only as background.\n\
         Final visible text boundary: you are generating the final user-visible PPT page. Do not render internal field names, prompt words, template labels, developer terminology, agent workflow terminology, or product names.\n\
         Never render these visible terms: Prompt, confirmedPrompt, MVP, Demo, Pomegranate, PPT Master, Executor, Agent, Workflow, fallback, legacy fallback, legacy_fallback, legacy mode, native, spec_lock, design_spec, slide_plan, pageTheme, contentScope, chartType, chartRef, background pain point, core solution, technical flow, closed-loop validation. The ordinary semantic noun legacy is allowed only when it clearly describes the user's subject, such as historical or cultural legacy.\n\
         Visible page text must come from user material, AI understanding, current slide pageTheme/mainClaim/mustInclude, or reasonable non-fabricated summary. If content is insufficient, use a more general topic summary; never fill with template placeholders.\n\
         Fact safety: do not invent exact numbers, rankings, award counts, fellow counts, laboratory counts, or years. If a precise data point is not explicitly sourced, write 示意数据, 可替换数据位, 代表性方向, or 长期积累 instead.\n",
        title = slide.title,
        page_theme = slide.page_theme,
        main_claim = slide.main_claim,
        content_scope = slide.content_scope,
        must_include = if slide.must_include.is_empty() {
            "none".to_string()
        } else {
            slide.must_include.join("; ")
        },
        must_avoid = if slide.must_avoid.is_empty() {
            "none".to_string()
        } else {
            slide.must_avoid.join("; ")
        },
        page_rhythm = page_rhythm_for_slide(slide),
        chart_type = chart_reference_for_slide(slide, mapping).unwrap_or_else(|| "none".to_string()),
        density_contract = density_contract.prompt_contract(),
    );
    let full_ai_prompt = format!(
        "你是 ppt-master Executor。请按照 ppt-master 的设计体系逐页手写 SVG，而不是生成简单占位框。\n\
         只输出完整 SVG，不要 markdown，不要解释。\n\n\
         硬性要求：\n\
         1. SVG viewBox 必须是 \"0 0 1280 720\"，width=\"1280\" height=\"720\"。\n\
         2. 必须遵守 spec_lock.md 中的 mode、visual_style、colors、typography、page_rhythm、page_layouts、page_charts、forbidden。\n\
         3. 视觉设计由 ppt-master reference 驱动：读取 locked mode 与 locked visual_style 的语义，不要退化成普通卡片模板。\n\
         4. 如本页有 page_charts，应借鉴对应 charts 模板的信息结构；如本页有 page_layouts，应继承对应 layout 的结构精神。\n\
         5. 不要使用外部网络图片；不要使用 forbidden 中禁止的 SVG 元素。\n\
         6. 每页要有明确视觉层级、留白节奏和页面角色差异。\n\
         7. 禁止生成任何 <clipPath> 或 clip-path 属性，包括普通图形、剪影、头像、卡片和装饰；直接绘制最终边界，不得依赖裁剪。\n\
         8. NativeThemeSpec 是整套页面唯一且不可重新解释的全局主题合同；每页必须使用 primary/secondary/accent 至少一种，并遵守背景、线条、装饰与 forbidden_colors。不要把主题字段渲染成可见文字。\n\n\
         【NativeThemeSpec — deck-wide immutable】\n{theme_contract}\n\n\
         【Native text geometry contract】\n{text_geometry_contract}\n\n\
         [Universal page density contract — theme-independent]\n{density_contract}\n\n\
         【ppt-master SKILL 摘要】\n{skill_excerpt}\n\n\
         【Executor Base 摘要】\n{executor_excerpt}\n\n\
         【Shared Standards 摘要】\n{standards_excerpt}\n\n\
         【Modes Index 摘要】\n{modes_index}\n\n\
         【Visual Styles Index 摘要】\n{visual_styles_index}\n\n\
         【Locked Mode Reference: {mode}】\n{mode_reference}\n\n\
         【Locked Visual Style Reference: {visual_style}】\n{visual_reference}\n\n\
         【Layouts Index 摘要】\n{layouts_index}\n\n\
         【Charts Index 摘要】\n{charts_index}\n\n\
         【Per-page spec_lock execution context】\n{locked_page_context}\n\n\
         【design_spec.md】\n{design_spec}\n\n\
         【spec_lock.md】\n{spec_lock}\n\n\
         【全局标题】{deck_title}\n【总页数】{total}\n【上一页标题】{prev_title}\n【下一页标题】{next_title}\n\n\
         【当前页 JSON】\n{slide_json}\n\n\
         请输出完整 SVG：",
        skill_excerpt = truncate_for_prompt(&skill_excerpt(skill_text), 1200),
        theme_contract = theme_spec.prompt_contract(),
        executor_excerpt = truncate_for_prompt(&resources.executor_base, 3000),
        standards_excerpt = truncate_for_prompt(&resources.shared_standards, 2400),
        modes_index = truncate_for_prompt(&resources.modes_index, 400),
        visual_styles_index = truncate_for_prompt(&resources.visual_styles_index, 450),
        mode = mapping.mode,
        mode_reference = truncate_for_prompt(&mapping.mode_reference, 1600),
        visual_style = mapping.visual_style,
        visual_reference = truncate_for_prompt(&mapping.visual_style_reference, 1800),
        layouts_index = truncate_for_prompt(&resources.layouts_index, 350),
        charts_index = truncate_for_prompt(&resources.charts_index, 500),
        locked_page_context = locked_page_context,
        text_geometry_contract = text_geometry_contract,
        density_contract = density_contract.prompt_contract(),
        design_spec = format!(
            "{}\n\n{}",
            current_page_task,
            truncate_for_prompt(design_spec, 1800)
        ),
        spec_lock = truncate_for_prompt(&spec_lock, 4000),
        deck_title = plan.title,
        total = plan.slides.len(),
        prev_title = prev_title,
        next_title = next_title,
        slide_json = serde_json::to_string_pretty(slide).unwrap_or_default()
    );
    let ai_prompt = if let Some(current_svg) = geometry_repair_svg {
        let slide_spec = spec_lock_path
            .parent()
            .map(|project| {
                project
                    .join("slide_specs")
                    .join(format!("slide-{:02}.json", slide.page))
            })
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_else(|| serde_json::to_string_pretty(slide).unwrap_or_default());
        format!(
            "You are performing one page-local SVG text-geometry repair.\n\
             Return only one complete SVG. Do not re-plan the deck and do not use any fallback.\n\
             Preserve every visible text string exactly, in the same DOM order. Do not delete, summarize, rewrite, or add visible content.\n\
             You may only adjust the failed text elements inside their declared regions: x/y, text-anchor, tspan line breaks/dy, line height, local font size down to data-pome-min-font-size, and a small local region correction inside the 1280x720 canvas.\n\
             Keep all non-failed graphics, colors, hierarchy, and page structure unchanged. The same deck-wide NativeThemeSpec remains mandatory.\n\
             Re-check text-to-text and text-to-obstacle collisions after the repair.\n\n\
             [NativeThemeSpec — deck-wide immutable]\n{theme_contract}\n\n\
             [Current SlideSpec]\n{slide_spec}\n\n\
             [Must keep visible text]\n{must_keep}\n\n\
             [Geometry failure elements and allowed regions]\n{repair_context}\n\n\
             [Current SVG]\n{current_svg}\n",
            theme_contract = theme_spec.prompt_contract(),
            slide_spec = slide_spec,
            must_keep = serde_json::to_string_pretty(
                &geometry_must_keep_text.unwrap_or_default()
            )
            .unwrap_or_default(),
            repair_context = retry_feedback.unwrap_or("{}"),
            current_svg = current_svg,
        )
    } else if let Some(current_svg) = density_relayout_svg {
        let slide_spec = spec_lock_path
            .parent()
            .map(|project| {
                project
                    .join("slide_specs")
                    .join(format!("slide-{:02}.json", slide.page))
            })
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_else(|| serde_json::to_string_pretty(slide).unwrap_or_default());
        format!(
            "You are performing exactly one page-local SVG space-utilization relayout.\n\
             Return only one complete 1280x720 SVG. Do not re-plan the deck and do not use any fallback.\n\
             Preserve every current visible text string and every mustInclude fact. Do not delete, rewrite, repeat, or invent facts.\n\
             Keep the immutable NativeThemeSpec, font minimums, text-region metadata, and all geometry rules.\n\
             Redistribute existing information to remove the reported non-functional dead whitespace. Use a semantic main region and supporting regions. You may change local structure, positions, scale of the true focal point, relationship lines, stage labels, data bars, quote emphasis, or section backgrounds.\n\
             Do not add empty cards or meaningless rectangles, and do not turn the page into a card grid unless the current facts are genuinely parallel.\n\
             Re-check text regions, collisions, and canvas bounds before returning.\n\n\
             [NativeThemeSpec — deck-wide immutable]\n{theme_contract}\n\n\
             [Universal density contract]\n{density_contract}\n\n\
             [Current SlideSpec]\n{slide_spec}\n\n\
             [Must include facts]\n{must_include}\n\n\
             [Must keep current visible text]\n{must_keep}\n\n\
             [Measured dead-whitespace report]\n{repair_context}\n\n\
             [Current SVG]\n{current_svg}\n",
            theme_contract = theme_spec.prompt_contract(),
            density_contract = density_contract.prompt_contract(),
            slide_spec = slide_spec,
            must_include = serde_json::to_string_pretty(&slide.must_include).unwrap_or_default(),
            must_keep = serde_json::to_string_pretty(
                &density_must_keep_text.unwrap_or_default()
            )
            .unwrap_or_default(),
            repair_context = retry_feedback.unwrap_or("{}"),
            current_svg = current_svg,
        )
    } else {
        full_ai_prompt
    };
    if let Some(project) = spec_lock_path.parent() {
        let input_dir = project.join("analysis").join("native_executor_inputs");
        let _ = fs::create_dir_all(&input_dir);
        let suffix = if geometry_repair_svg.is_some() {
            "geometry-repair"
        } else if density_relayout_svg.is_some() {
            "density-relayout"
        } else {
            "generate"
        };
        let _ = write_file(
            &input_dir.join(format!("P{:02}-{suffix}.txt", slide.page)),
            &ai_prompt,
        );
    }
    let input = PluginAiChatInput {
        request_id: format!("ppt_master_native_svg_{:02}", slide.page),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let raw = ppt_native_generation_chat_with_timeout(
        db,
        input,
        &format!("AI 生成第 {} 页 SVG", slide.page),
    )
    .await?;
    let has_svg_start = raw.contains("<svg");
    let has_svg_end = raw.contains("</svg>");
    println!(
        "[Native Executor Response] page=P{:02} responseChars={} hasSvgStart={} hasSvgEnd={}",
        slide.page,
        raw.chars().count(),
        has_svg_start,
        has_svg_end
    );
    extract_svg(&raw).ok_or_else(|| {
        AppError::Custom(format!(
            "第 {} 页 AI 未返回完整 SVG：responseChars={}，hasSvgStart={}，hasSvgEnd={}",
            slide.page,
            raw.chars().count(),
            has_svg_start,
            has_svg_end
        ))
    })
}

async fn generate_agent_slide_svg(
    db: &Database,
    skill_text: &str,
    design_spec: &str,
    spec_lock: &str,
    plan: &SlidePlan,
    slide: &Slide,
    prev_title: &str,
    next_title: &str,
    model_id: Option<i64>,
) -> Result<String, AppError> {
    let ai_prompt = format!(
        "你是 ppt-master Executor。请逐页手写 SVG。只输出完整 SVG，不要 markdown，不要解释。\n\
         必须遵守：viewBox=\"0 0 1600 900\"；合法 XML/SVG；中文清晰；不要依赖外部图片；文字短句；视觉层级丰富；本页版式必须服从 layout 和 visualHint。\n\
         允许使用 rect/circle/line/path/text/g/defs/linearGradient/pattern 等基础元素。不要把所有页做成同一个卡片模板。\n\n\
         【ppt-master SVG 规则摘要】\n{skill_excerpt}\n\n\
         【design_spec.md】\n{design_spec}\n\n\
         【spec_lock.md】\n{spec_lock}\n\n\
         【全局标题】{deck_title}\n【总页数】{total}\n【上一页标题】{prev_title}\n【下一页标题】{next_title}\n\n\
         【当前页 JSON】\n{slide_json}\n\n\
         请输出完整 SVG：",
        skill_excerpt = skill_excerpt(skill_text),
        deck_title = plan.title,
        total = plan.slides.len(),
        slide_json = serde_json::to_string_pretty(slide).unwrap_or_default()
    );
    let input = PluginAiChatInput {
        request_id: format!("ppt_master_agent_svg_{:02}", slide.page),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let raw =
        ppt_ai_chat_with_timeout(db, input, &format!("AI 生成第 {} 页 SVG", slide.page)).await?;
    extract_svg(&raw)
        .ok_or_else(|| AppError::Custom(format!("第 {} 页 AI 输出中找不到完整 SVG", slide.page)))
}

async fn repair_agent_svgs_once(
    db: &Database,
    skill_text: &str,
    design_spec: &str,
    spec_lock: &str,
    plan: &SlidePlan,
    svg_output: &Path,
    quality_error: &str,
    model_id: Option<i64>,
    attempts_by_file: &mut HashMap<String, usize>,
    log_lines: &mut Vec<String>,
) -> Result<(), AppError> {
    for slide in &plan.slides {
        let filename = svg_filename_for_slide(slide);
        let path = svg_output.join(&filename);
        if !path.is_file() {
            continue;
        }
        if !reserve_native_svg_repair_attempt(attempts_by_file, &filename) {
            log_lines.push(format!(
                "[SVG Repair] skipped, max attempts reached: file={} max_attempts={}",
                filename, NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE
            ));
            continue;
        }
        let old_svg = fs::read_to_string(&path).unwrap_or_default();
        let prompt = format!(
            "你是 ppt-master SVG 修复器。只输出修复后的完整 SVG，不要 markdown。\n\
             修复目标：通过 svg_quality_checker.py。保持页面设计意图，不要改成通用卡片模板。\n\n\
             【质量检查错误】\n{quality_error}\n\n【design_spec.md】\n{design_spec}\n\n【spec_lock.md】\n{spec_lock}\n\n\
             【当前页】\n{slide_json}\n\n【原 SVG】\n{old_svg}",
            slide_json = serde_json::to_string_pretty(slide).unwrap_or_default(),
            quality_error = quality_error,
            design_spec = design_spec,
            spec_lock = spec_lock,
            old_svg = old_svg,
        );
        let input = PluginAiChatInput {
            request_id: format!("ppt_master_agent_svg_repair_{:02}", slide.page),
            model_id,
            messages: vec![
                PluginAiMessage {
                    role: "system".to_string(),
                    content: skill_excerpt(skill_text).to_string(),
                },
                PluginAiMessage {
                    role: "user".to_string(),
                    content: prompt,
                },
            ],
        };
        let svg_chars = old_svg.chars().count();
        let prompt_chars = input
            .messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum();
        let raw = ppt_native_svg_repair_chat_with_timeout(
            db,
            input,
            &format!("AI 修复 SVG：{}", filename),
            &filename,
            svg_chars,
            prompt_chars,
        )
        .await?;
        if let Some(svg) = extract_svg(&raw) {
            write_file(&path, &svg)?;
        }
    }
    Ok(())
}

async fn repair_native_svg_issues_once(
    db: &Database,
    svg_output: &Path,
    issues: &[NativeSvgIssue],
    model_id: Option<i64>,
    attempts_by_file: &mut HashMap<String, usize>,
    log_lines: &mut Vec<String>,
) -> Result<(), AppError> {
    let mut processed_this_pass = Vec::new();
    for issue in issues {
        if processed_this_pass
            .iter()
            .any(|name: &String| name == &issue.file_name)
        {
            continue;
        }
        processed_this_pass.push(issue.file_name.clone());
        let path = svg_output.join(&issue.file_name);
        if !path.is_file() {
            log_lines.push(format!(
                "[Native Compat] repair skipped, source SVG not found: svg_output/{}",
                issue.file_name
            ));
            continue;
        }
        if !reserve_native_svg_repair_attempt(attempts_by_file, &issue.file_name) {
            log_lines.push(format!(
                "[Native Compat] repair skipped, max attempts reached: file={} max_attempts={}",
                issue.file_name, NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE
            ));
            continue;
        }
        let old_svg = fs::read_to_string(&path).unwrap_or_default();
        let prompt = build_native_svg_repair_prompt(issue, &old_svg);
        let svg_chars = old_svg.chars().count();
        let prompt_chars = prompt.chars().count();
        let max_output_tokens = native_svg_repair_output_tokens(svg_chars);
        let resolved_timeout = native_svg_repair_timeout(db);
        let attempt = attempts_by_file
            .get(&issue.file_name)
            .copied()
            .unwrap_or_default();
        log_lines.push(format!(
            "[Native Compat AI] file={} attempt={}/{} svg_chars={} prompt_chars={} estimated_tokens={} max_output_tokens={} timeout_secs={}",
            issue.file_name,
            attempt,
            NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE,
            svg_chars,
            prompt_chars,
            estimate_mixed_text_tokens(&prompt),
            max_output_tokens,
            resolved_timeout.seconds,
        ));
        let input = PluginAiChatInput {
            request_id: format!("ppt_master_native_svg_compat_repair_{}", issue.file_name),
            model_id,
            messages: vec![PluginAiMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };
        let raw = ppt_native_svg_repair_chat_with_timeout(
            db,
            input,
            &format!("AI 修复 native 兼容 SVG：{}", issue.file_name),
            &issue.file_name,
            svg_chars,
            prompt_chars,
        )
        .await?;
        if let Some(svg) = extract_svg(&raw) {
            write_file(&path, &svg)?;
            log_lines.push(format!(
                "[Native Compat] repaired unsupported SVG elements in svg_output/{}",
                issue.file_name
            ));
        } else {
            log_lines.push(format!(
                "[Native Compat] repair failed, AI did not return SVG for {}",
                issue.file_name
            ));
        }
    }
    Ok(())
}

fn build_native_svg_repair_prompt(issue: &NativeSvgIssue, old_svg: &str) -> String {
    format!(
        "Repair this SVG for native DrawingML PPTX export. Output only the complete fixed SVG, no markdown or explanation.\n\n\
         Failed file: {file_name}\n\
         Error type: {issue_type}\n\
         Unsupported elements: {unsupported}\n\
         Converter detail: {detail}\n\n\
         Change only the detected incompatibility. Preserve all text, geometry, colors, dimensions, and visual hierarchy that are unrelated to the issue.\n\
         Replace unsupported nodes with equivalent native-safe svg/g/rect/circle/ellipse/line/polyline/polygon/path/text/tspan elements.\n\
         Do not add use, symbol, foreignObject, external images, filter, mask, clipPath, or unsupported pattern elements.\n\n\
         SVG to repair:\n{old_svg}",
        file_name = issue.file_name,
        issue_type = issue.issue_type,
        unsupported = issue.unsupported_elements.join(", "),
        detail = issue.detail,
        old_svg = old_svg,
    )
}

fn reserve_native_svg_repair_attempt(
    attempts_by_file: &mut HashMap<String, usize>,
    file_name: &str,
) -> bool {
    let attempts = attempts_by_file.entry(file_name.to_string()).or_default();
    if *attempts >= NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE {
        return false;
    }
    *attempts += 1;
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedNativeSvgRepairTimeout {
    seconds: u64,
    source: &'static str,
}

fn native_svg_repair_timeout(db: &Database) -> ResolvedNativeSvgRepairTimeout {
    match db.get_config(NATIVE_SVG_REPAIR_TIMEOUT_CONFIG_KEY) {
        Ok(configured) => resolve_native_svg_repair_timeout(configured.as_deref()),
        Err(error) => {
            println!(
                "[PPT Native SVG Repair Config] key={} read_failed={} fallbackSeconds={}",
                NATIVE_SVG_REPAIR_TIMEOUT_CONFIG_KEY, error, NATIVE_SVG_REPAIR_TIMEOUT_DEFAULT_SECS,
            );
            ResolvedNativeSvgRepairTimeout {
                seconds: NATIVE_SVG_REPAIR_TIMEOUT_DEFAULT_SECS,
                source: "default",
            }
        }
    }
}

fn resolve_native_svg_repair_timeout(configured: Option<&str>) -> ResolvedNativeSvgRepairTimeout {
    match configured.and_then(|value| value.trim().parse::<u64>().ok()) {
        Some(seconds) => ResolvedNativeSvgRepairTimeout {
            seconds: seconds.clamp(
                NATIVE_SVG_REPAIR_TIMEOUT_MIN_SECS,
                NATIVE_SVG_REPAIR_TIMEOUT_MAX_SECS,
            ),
            source: "config",
        },
        None => ResolvedNativeSvgRepairTimeout {
            seconds: NATIVE_SVG_REPAIR_TIMEOUT_DEFAULT_SECS,
            source: "default",
        },
    }
}

fn is_native_svg_repair_request_id(request_id: &str) -> bool {
    request_id.starts_with("ppt_master_agent_svg_repair_")
        || request_id.starts_with("ppt_master_native_svg_compat_repair_")
        || request_id.starts_with("ppt_master_final_text_guard_repair_")
}

async fn ppt_native_svg_repair_chat_with_timeout(
    db: &Database,
    input: PluginAiChatInput,
    context: &str,
    svg_file: &str,
    svg_chars: usize,
    prompt_chars: usize,
) -> Result<String, AppError> {
    let resolved_timeout = native_svg_repair_timeout(db);
    let max_output_tokens = native_svg_repair_output_tokens(svg_chars.max(1));
    ppt_ai_chat_with_policy(
        db,
        input,
        context,
        PptAiRequestPolicy {
            timeout_secs: resolved_timeout.seconds,
            max_output_tokens: Some(max_output_tokens),
            svg_chars: Some(svg_chars),
            prompt_chars: Some(prompt_chars),
            failure_type: Some("native_svg_repair_timeout"),
            timeout_source: resolved_timeout.source,
            svg_file: Some(svg_file.to_string()),
            disable_thinking: true,
            force_json_output: false,
        },
    )
    .await
}

fn native_svg_repair_output_tokens(svg_chars: usize) -> i64 {
    let proportional_limit = (svg_chars as i64 + 2) / 3 + 1_024;
    proportional_limit.clamp(2_048, NATIVE_SVG_REPAIR_MAX_OUTPUT_TOKENS)
}

async fn repair_final_text_leaks_once(
    db: &Database,
    design_spec: &str,
    spec_lock: &str,
    plan: &SlidePlan,
    svg_output: &Path,
    issues: &[FinalTextIssue],
    model_id: Option<i64>,
    attempts_by_file: &mut HashMap<String, usize>,
    log_lines: &mut Vec<String>,
) -> Result<(), AppError> {
    let mut repaired = Vec::new();
    for issue in issues {
        if repaired
            .iter()
            .any(|name: &String| name == &issue.file_name)
        {
            continue;
        }
        let path = svg_output.join(&issue.file_name);
        if !path.is_file() {
            continue;
        }
        if !reserve_native_svg_repair_attempt(attempts_by_file, &issue.file_name) {
            log_lines.push(format!(
                "[Final Text Guard] {} repair skipped: max attempts reached ({})",
                issue.file_name, NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE
            ));
            continue;
        }
        let old_svg = fs::read_to_string(&path).unwrap_or_default();
        let slide_json = slide_json_for_svg_file(plan, &issue.file_name);
        let prompt = format!(
            "You are repairing a final user-visible PPT SVG page. Output only the complete fixed SVG, no markdown.\n\n\
             File: {file_name}\n\
             Leaked internal/template terms: {terms}\n\n\
             Requirements:\n\
             - Remove all internal template terms, agent workflow terms, placeholders, and developer words from visible page text.\n\
             - Replace them with content relevant to the user's topic and this slide plan.\n\
             - Preserve layout, geometry, viewBox, native DrawingML compatibility, and visual style.\n\
             - Do not introduce new unsupported SVG elements.\n\
             - Final visible page text must come only from user material, AI understanding, current slide pageTheme/mainClaim/mustInclude, or reasonable non-fabricated summary.\n\
             - Forbidden visible terms include: {banned_terms}.\n\n\
             design_spec.md:\n{design_spec}\n\n\
             spec_lock.md:\n{spec_lock}\n\n\
             Slide JSON:\n{slide_json}\n\n\
             Original SVG:\n{old_svg}",
            file_name = issue.file_name,
            terms = issue.leaked_terms.join(", "),
            banned_terms = banned_final_text_terms().join(", "),
            design_spec = design_spec,
            spec_lock = spec_lock,
            slide_json = slide_json,
            old_svg = old_svg,
        );
        let input = PluginAiChatInput {
            request_id: format!("ppt_master_final_text_guard_repair_{}", issue.file_name),
            model_id,
            messages: vec![PluginAiMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };
        let svg_chars = old_svg.chars().count();
        let prompt_chars = input
            .messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum();
        let raw = ppt_native_svg_repair_chat_with_timeout(
            db,
            input,
            &format!("AI 修复 final text leakage: {}", issue.file_name),
            &issue.file_name,
            svg_chars,
            prompt_chars,
        )
        .await?;
        if let Some(svg) = extract_svg(&raw) {
            write_file(&path, &svg)?;
            log_lines.push(format!(
                "[Final Text Guard] {} repair done",
                issue.file_name
            ));
            repaired.push(issue.file_name.clone());
        } else {
            log_lines.push(format!(
                "[Final Text Guard] {} repair failed: AI did not return SVG",
                issue.file_name
            ));
        }
    }
    Ok(())
}

async fn enforce_final_text_guard(
    db: &Database,
    design_spec: &str,
    spec_lock: &str,
    plan: &SlidePlan,
    svg_output: &Path,
    model_id: Option<i64>,
    attempts_by_file: &mut HashMap<String, usize>,
    log_lines: &mut Vec<String>,
) -> Result<(), AppError> {
    log_lines.push("[Final Text Guard] scan start".to_string());
    let mut issues = scan_final_text_leaks(svg_output)?;
    if issues.is_empty() {
        for slide in &plan.slides {
            log_lines.push(format!("[Final Text Guard] P{:02} passed", slide.page));
        }
        return Ok(());
    }
    for issue in &issues {
        log_lines.push(format!(
            "[Final Text Guard] {} failed: leaked terms = {}",
            issue.file_name,
            issue.leaked_terms.join(", ")
        ));
        log_lines.push(format!(
            "[Final Text Guard] {} repair start",
            issue.file_name
        ));
    }
    repair_final_text_leaks_once(
        db,
        design_spec,
        spec_lock,
        plan,
        svg_output,
        &issues,
        model_id,
        attempts_by_file,
        log_lines,
    )
    .await?;
    issues = scan_final_text_leaks(svg_output)?;
    if !issues.is_empty() {
        return Err(AppError::Custom(format!(
            "检测到内部模板词泄漏，已停止导出: {}",
            summarize_final_text_issues(&issues)
        )));
    }
    for slide in &plan.slides {
        log_lines.push(format!("[Final Text Guard] P{:02} passed", slide.page));
    }
    Ok(())
}

fn slide_json_for_svg_file(plan: &SlidePlan, file_name: &str) -> String {
    let page = file_name
        .split('_')
        .next()
        .and_then(|value| value.parse::<usize>().ok());
    if let Some(page) = page {
        if let Some(slide) = plan.slides.iter().find(|slide| slide.page == page) {
            return serde_json::to_string_pretty(slide).unwrap_or_default();
        }
    }
    String::new()
}

fn scan_native_incompatible_svgs(svg_dir: &Path) -> Result<Vec<NativeSvgIssue>, AppError> {
    let mut issues = Vec::new();
    if !svg_dir.is_dir() {
        return Ok(issues);
    }
    for entry in fs::read_dir(svg_dir).map_err(|e| {
        AppError::Custom(format!("读取 SVG 目录失败: {} ({})", svg_dir.display(), e))
    })? {
        let entry = entry.map_err(|e| AppError::Custom(format!("读取 SVG 文件项失败: {}", e)))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("svg") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| AppError::Custom(format!("读取 SVG 失败: {} ({})", path.display(), e)))?;
        let unsupported = detect_native_unsupported_elements(&text);
        if !unsupported.is_empty() {
            issues.push(NativeSvgIssue {
                file_name: path
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string()),
                issue_type: "unsupported visual SVG element".to_string(),
                unsupported_elements: unsupported,
                detail: "Pomegranate native preflight found SVG elements unsupported by ppt-master DrawingML export".to_string(),
            });
        }
    }
    Ok(issues)
}

fn detect_native_unsupported_elements(svg: &str) -> Vec<String> {
    let mut elements = Vec::new();
    let lower = svg.to_lowercase();
    for (needle, label) in [
        ("<use", "use"),
        ("<symbol", "symbol"),
        ("<foreignobject", "foreignObject"),
        ("<style", "style"),
        ("<filter", "filter"),
        ("<mask", "mask"),
        ("<clippath", "clipPath"),
        ("<textpath", "textPath"),
        ("<animate", "animation"),
        ("<set", "animation"),
        ("<script", "script"),
        ("<iframe", "iframe"),
    ] {
        if lower.contains(needle) && !elements.iter().any(|item| item == label) {
            elements.push(label.to_string());
        }
    }
    for (needle, label) in [
        (" class=", "class"),
        (" xlink:href=\"http", "external href image"),
        (" xlink:href='http", "external href image"),
        ("rgba(", "rgba"),
        ("<g opacity=", "group opacity"),
        ("&nbsp;", "HTML named entity"),
        ("&mdash;", "HTML named entity"),
        ("&copy;", "HTML named entity"),
        ("&ndash;", "HTML named entity"),
        ("&reg;", "HTML named entity"),
        ("&hellip;", "HTML named entity"),
        ("&bull;", "HTML named entity"),
    ] {
        if lower.contains(needle) && !elements.iter().any(|item| item == label) {
            elements.push(label.to_string());
        }
    }
    if lower.contains("<image") && (lower.contains("href=\"http") || lower.contains("href='http")) {
        elements.push("external href image".to_string());
    }
    if lower.contains("<pattern") && !lower.contains("data-pptx-pattern") {
        elements.push("unsupported pattern".to_string());
    }
    elements
}

fn banned_final_text_terms() -> &'static [&'static str] {
    &[
        "背景痛点",
        "核心方案",
        "技术流程",
        "Demo 展示",
        "Demo",
        "MVP",
        "闭环验证",
        "最小闭环",
        "Prompt",
        "confirmedPrompt",
        "Pomegranate",
        "PPT Master",
        "PPT-MASTER",
        "EXECUTOR",
        "Executor",
        "Agent",
        "Workflow",
        "fallback",
        "legacy fallback",
        "legacy_fallback",
        "legacy mode",
        "native",
        "spec_lock",
        "design_spec",
        "slide_plan",
        "从确认 Prompt 中获取",
        "听众最关心的核心",
        "说明最小闭环",
        "说明方案如何解决问题",
    ]
}

fn scan_final_text_leaks(svg_dir: &Path) -> Result<Vec<FinalTextIssue>, AppError> {
    let mut issues = Vec::new();
    if !svg_dir.is_dir() {
        return Ok(issues);
    }
    for entry in fs::read_dir(svg_dir)? {
        let entry = entry?;
        let path = entry.path();
        let is_svg = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("svg"))
            .unwrap_or(false);
        if !is_svg {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        let visible_text = visible_svg_text(&text);
        let visible_lower = visible_text.to_lowercase();
        let mut leaked_terms = Vec::new();
        for term in banned_final_text_terms() {
            let term_lower = term.to_lowercase();
            if visible_lower.contains(&term_lower) && !leaked_terms.iter().any(|item| item == term)
            {
                leaked_terms.push((*term).to_string());
            }
        }
        if !leaked_terms.is_empty() {
            issues.push(FinalTextIssue {
                file_name: entry.file_name().to_string_lossy().to_string(),
                leaked_terms,
            });
        }
    }
    Ok(issues)
}

fn visible_svg_text(svg: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in svg.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn summarize_final_text_issues(issues: &[FinalTextIssue]) -> String {
    issues
        .iter()
        .map(|issue| {
            format!(
                "{} leaked terms = {}",
                issue.file_name,
                issue.leaked_terms.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn parse_native_export_issues(output: &str) -> Vec<NativeSvgIssue> {
    let mut issues = Vec::new();
    for line in output.lines() {
        if !line.contains(".svg:") || !line.contains("unsupported visual SVG element") {
            continue;
        }
        let Some(idx) = line.find(".svg:") else {
            continue;
        };
        let file_name = format!("{}.svg", &line[..idx]);
        let unsupported = detect_unsupported_elements_from_error(line);
        issues.push(NativeSvgIssue {
            file_name,
            issue_type: "unsupported visual SVG element".to_string(),
            unsupported_elements: unsupported,
            detail: line.trim().to_string(),
        });
    }
    issues
}

fn detect_unsupported_elements_from_error(line: &str) -> Vec<String> {
    let lower = line.to_lowercase();
    let mut elements = Vec::new();
    for label in [
        "use",
        "symbol",
        "foreignobject",
        "filter",
        "mask",
        "clippath",
        "pattern",
    ] {
        let slash = format!("/{}", label);
        let tag = format!("<{}", label);
        if lower.contains(&slash) || lower.contains(&tag) {
            elements.push(match label {
                "foreignobject" => "foreignObject".to_string(),
                "clippath" => "clipPath".to_string(),
                _ => label.to_string(),
            });
        }
    }
    if elements.is_empty() {
        elements.push("unknown unsupported element".to_string());
    }
    elements
}

fn summarize_native_issues(issues: &[NativeSvgIssue]) -> String {
    if issues.is_empty() {
        return String::new();
    }
    let first = &issues[0];
    format!(
        "失败页: {}; 错误类型: {}; 不支持元素: {}; 建议: AI修复该页 SVG 或切换 legacy fallback。{}",
        first.file_name,
        first.issue_type,
        first.unsupported_elements.join(", "),
        first.detail
    )
}

fn extract_svg(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with("<svg") || trimmed.starts_with("<?xml") {
        return Some(trimmed.to_string());
    }
    let start = raw.find("<svg").or_else(|| raw.find("<?xml"))?;
    let end = raw.rfind("</svg>")?;
    if end <= start {
        return None;
    }
    Some(raw[start..end + "</svg>".len()].trim().to_string())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct NativeSvgNormalizationReport {
    rgba_colors_normalized: usize,
    group_opacity_normalized: usize,
    filters_removed: usize,
    malformed_closing_tags_repaired: usize,
    duplicate_line_coordinates_repaired: usize,
}

fn normalize_native_svg_compatibility(svg: &str) -> (String, NativeSvgNormalizationReport) {
    let mut rgba_colors_normalized = 0;
    let rgba_normalized = native_opening_tag_regex()
        .replace_all(svg, |captures: &regex::Captures<'_>| {
            let tag = captures.get(0).map_or("", |value| value.as_str());
            let (normalized, count) = normalize_native_rgba_tag(tag);
            rgba_colors_normalized += count;
            normalized
        })
        .into_owned();
    let (line_coordinate_normalized, duplicate_line_coordinates_repaired) =
        normalize_duplicate_native_line_coordinate_attributes(&rgba_normalized);
    let mut normalized_count = 0;
    let group_normalized = native_group_opacity_regex()
        .replace_all(
            &line_coordinate_normalized,
            |captures: &regex::Captures<'_>| {
                normalized_count += 1;
                let before = captures.name("before").map_or("", |value| value.as_str());
                let after = captures.name("after").map_or("", |value| value.as_str());
                let opacity = captures.name("opacity").map_or("1", |value| value.as_str());
                let remaining_attributes = format!("{before}{after}");
                let lower = remaining_attributes.to_ascii_lowercase();
                let fill_opacity = if lower.contains("fill-opacity") {
                    String::new()
                } else {
                    format!(" fill-opacity=\"{opacity}\"")
                };
                let stroke_opacity = if lower.contains("stroke-opacity") {
                    String::new()
                } else {
                    format!(" stroke-opacity=\"{opacity}\"")
                };
                format!("<g{before}{fill_opacity}{stroke_opacity}{after}>")
            },
        )
        .into_owned();
    let filter_definition_count = native_filter_definition_regex()
        .find_iter(&group_normalized)
        .count();
    let without_filter_definitions = native_filter_definition_regex()
        .replace_all(&group_normalized, "")
        .into_owned();
    let filter_reference_count = native_filter_reference_regex()
        .find_iter(&without_filter_definitions)
        .count();
    let without_filter_references = native_filter_reference_regex()
        .replace_all(&without_filter_definitions, "")
        .into_owned();
    let (normalized, malformed_closing_tags_repaired) =
        normalize_duplicate_native_svg_closing_tags(&without_filter_references);
    (
        normalized,
        NativeSvgNormalizationReport {
            rgba_colors_normalized,
            group_opacity_normalized: normalized_count,
            filters_removed: filter_definition_count + filter_reference_count,
            malformed_closing_tags_repaired,
            duplicate_line_coordinates_repaired,
        },
    )
}

fn normalize_duplicate_native_line_coordinate_attributes(svg: &str) -> (String, usize) {
    let mut repaired = 0;
    let normalized = native_line_opening_tag_regex()
        .replace_all(svg, |captures: &regex::Captures<'_>| {
            let tag = captures.get(0).map_or("", |value| value.as_str());
            let attributes = native_line_coordinate_attribute_regex()
                .captures_iter(tag)
                .filter_map(|attribute| {
                    let name = attribute.name("name")?;
                    let value = attribute.name("value")?;
                    Some((
                        name.start(),
                        name.end(),
                        name.as_str().to_ascii_lowercase(),
                        value.as_str().trim().to_string(),
                    ))
                })
                .collect::<Vec<_>>();
            if attributes.len() != 4 {
                return tag.to_string();
            }

            let repair_candidates = [
                ("x1", "x2", "y1", "y2"),
                ("x2", "x1", "y1", "y2"),
                ("y1", "y2", "x1", "x2"),
                ("y2", "y1", "x1", "x2"),
            ];
            for (duplicate, missing, other_start, other_end) in repair_candidates {
                let count = |name: &str| {
                    attributes
                        .iter()
                        .filter(|(_, _, attribute, _)| attribute == name)
                        .count()
                };
                if count(duplicate) != 2
                    || count(missing) != 0
                    || count(other_start) != 1
                    || count(other_end) != 1
                {
                    continue;
                }
                let duplicate_values = attributes
                    .iter()
                    .filter(|(_, _, attribute, _)| attribute == duplicate)
                    .map(|(_, _, _, value)| value)
                    .collect::<Vec<_>>();
                if duplicate_values.len() != 2 || duplicate_values[0] != duplicate_values[1] {
                    continue;
                }
                let Some((start, end, _, _)) = attributes
                    .iter()
                    .find(|(_, _, attribute, _)| attribute == duplicate)
                else {
                    continue;
                };
                let mut fixed = tag.to_string();
                fixed.replace_range(*start..*end, missing);
                repaired += 1;
                return fixed;
            }
            tag.to_string()
        })
        .into_owned();
    (normalized, repaired)
}

fn normalize_native_rgba_tag(tag: &str) -> (String, usize) {
    let mut normalized = tag.to_string();
    let mut count = 0;

    loop {
        let Some(captures) = native_rgba_color_attribute_regex().captures(&normalized) else {
            break;
        };
        let Some(full) = captures.get(0) else {
            break;
        };
        let attribute = captures
            .name("attribute")
            .map_or("", |value| value.as_str())
            .to_ascii_lowercase();
        let quote = captures.name("quote").map_or("\"", |value| value.as_str());
        let Some(red) = captures
            .name("red")
            .and_then(|value| value.as_str().parse::<u16>().ok())
        else {
            break;
        };
        let Some(green) = captures
            .name("green")
            .and_then(|value| value.as_str().parse::<u16>().ok())
        else {
            break;
        };
        let Some(blue) = captures
            .name("blue")
            .and_then(|value| value.as_str().parse::<u16>().ok())
        else {
            break;
        };
        let Some(alpha) = captures
            .name("alpha")
            .and_then(|value| value.as_str().parse::<f64>().ok())
        else {
            break;
        };
        if red > 255 || green > 255 || blue > 255 || !(0.0..=1.0).contains(&alpha) {
            break;
        }

        let opacity_attribute = match attribute.as_str() {
            "fill" => "fill-opacity",
            "stroke" => "stroke-opacity",
            "stop-color" => "stop-opacity",
            _ => break,
        };
        let hex_attribute = format!("{attribute}={quote}#{red:02X}{green:02X}{blue:02X}{quote}");

        let existing_opacity = native_opacity_attribute_regex()
            .captures_iter(&normalized)
            .find(|candidate| {
                candidate
                    .name("attribute")
                    .is_some_and(|value| value.as_str().eq_ignore_ascii_case(opacity_attribute))
            })
            .and_then(|candidate| {
                let full = candidate.get(0)?;
                let value = candidate.name("value")?.as_str().parse::<f64>().ok()?;
                Some((
                    full.range(),
                    value,
                    candidate.name("quote")?.as_str().to_string(),
                ))
            });

        if let Some((opacity_range, existing_value, opacity_quote)) = existing_opacity {
            let combined = format_svg_opacity(existing_value * alpha);
            normalized.replace_range(
                opacity_range,
                &format!("{opacity_attribute}={opacity_quote}{combined}{opacity_quote}"),
            );
            let Some(updated_rgba) = native_rgba_color_attribute_regex().find(&normalized) else {
                break;
            };
            normalized.replace_range(updated_rgba.range(), &hex_attribute);
        } else {
            normalized.replace_range(
                full.range(),
                &format!(
                    "{hex_attribute} {opacity_attribute}={quote}{}{quote}",
                    format_svg_opacity(alpha)
                ),
            );
        }
        count += 1;
    }

    (normalized, count)
}

fn format_svg_opacity(value: f64) -> String {
    let value = value.clamp(0.0, 1.0);
    let formatted = format!("{value:.6}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_duplicate_native_svg_closing_tags(svg: &str) -> (String, usize) {
    let mut normalized = svg.to_string();
    let mut repaired = 0;
    for (invalid, valid) in [
        ("</texttext>", "</text>"),
        ("</tspantspan>", "</tspan>"),
        ("</gg>", "</g>"),
        ("</defsdefs>", "</defs>"),
        ("</svgsvg>", "</svg>"),
    ] {
        let count = normalized.matches(invalid).count();
        if count > 0 {
            normalized = normalized.replace(invalid, valid);
            repaired += count;
        }
    }
    (normalized, repaired)
}

fn native_opening_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?s)<[A-Za-z][^<>]*>"#).expect("valid regex"))
}

fn native_rgba_color_attribute_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)(?P<attribute>fill|stroke|stop-color)\s*=\s*(?P<quote>[\"'])rgba\(\s*(?P<red>[0-9]{1,3})\s*,\s*(?P<green>[0-9]{1,3})\s*,\s*(?P<blue>[0-9]{1,3})\s*,\s*(?P<alpha>(?:0(?:\.[0-9]+)?|1(?:\.0+)?))\s*\)[\"']"#,
        )
        .expect("valid regex")
    })
}

fn native_opacity_attribute_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)(?P<attribute>fill-opacity|stroke-opacity|stop-opacity)\s*=\s*(?P<quote>[\"'])(?P<value>(?:0(?:\.[0-9]+)?|1(?:\.0+)?))[\"']"#,
        )
        .expect("valid regex")
    })
}

fn native_group_opacity_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)<g(?P<before>[^>]*)\sopacity\s*=\s*[\"'](?P<opacity>[0-9]*\.?[0-9]+)[\"'](?P<after>[^>]*)>"#,
        )
        .expect("valid regex")
    })
}

fn native_line_opening_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?is)<line\b[^>]*>"#).expect("valid regex"))
}

fn native_line_coordinate_attribute_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(?P<name>x1|x2|y1|y2)\s*=\s*(?P<quote>[\"'])(?P<value>[^\"']*)[\"']"#)
            .expect("valid regex")
    })
}

fn native_filter_definition_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?is)<filter\b[^>]*>.*?</filter\s*>"#).expect("valid regex"))
}

fn native_filter_reference_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\sfilter\s*=\s*[\"'][^\"']*[\"']"#).expect("valid regex"))
}

fn validate_native_svg_text(file_name: &str, svg: &str) -> Result<(), AppError> {
    let trimmed = svg.trim();
    if trimmed.is_empty() {
        return Err(AppError::Custom(format!("原生 SVG 为空: {file_name}")));
    }
    if trimmed.contains("```") {
        return Err(AppError::Custom(format!(
            "原生 SVG 混入 Markdown 代码块标记: {file_name}"
        )));
    }
    if !trimmed.contains("<svg") || !trimmed.ends_with("</svg>") {
        return Err(AppError::Custom(format!(
            "原生 SVG 不完整或混入说明文字: {file_name}"
        )));
    }
    if !native_view_box_regex().is_match(trimmed) {
        return Err(AppError::Custom(format!(
            "原生 SVG viewBox 必须为 0 0 1280 720: {file_name}"
        )));
    }
    if !native_width_regex().is_match(trimmed) || !native_height_regex().is_match(trimmed) {
        return Err(AppError::Custom(format!(
            "原生 SVG width/height 必须为 1280/720: {file_name}"
        )));
    }
    Ok(())
}

fn native_view_box_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\bviewBox\s*=\s*["']\s*0\s+0\s+1280\s+720\s*["']"#).expect("valid regex")
    })
}

fn native_width_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\bwidth\s*=\s*["']\s*1280(?:\.0+)?\s*["']"#).expect("valid regex")
    })
}

fn native_height_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\bheight\s*=\s*["']\s*720(?:\.0+)?\s*["']"#).expect("valid regex")
    })
}

fn validate_native_svg_set(plan: &SlidePlan, svg_output: &Path) -> Result<(), AppError> {
    if !svg_output.is_dir() {
        return Err(AppError::NotFound(format!(
            "原生 SVG 目录不存在: {}",
            svg_output.display()
        )));
    }

    for slide in &plan.slides {
        let file_name = svg_filename_for_slide(slide);
        let path = svg_output.join(&file_name);
        if !path.is_file() {
            return Err(AppError::NotFound(format!(
                "原生模式缺少 SVG: {file_name} ({})",
                path.display()
            )));
        }
        let svg = fs::read_to_string(&path).map_err(|error| {
            AppError::Custom(format!("读取原生 SVG 失败: {file_name} ({error})"))
        })?;
        validate_native_svg_text(&file_name, &svg)?;
    }

    let actual_svg_count = fs::read_dir(svg_output)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        })
        .count();
    if actual_svg_count != plan.slides.len() {
        return Err(AppError::Custom(format!(
            "原生 SVG 页数不符合 slide_plan: expected={}, actual={}, dir={}",
            plan.slides.len(),
            actual_svg_count,
            svg_output.display()
        )));
    }
    Ok(())
}

fn ensure_native_svg_files_exist(plan: &SlidePlan, svg_output: &Path) -> Result<(), AppError> {
    if !svg_output.is_dir() {
        return Err(AppError::NotFound(format!(
            "原生 SVG 目录不存在: {}",
            svg_output.display()
        )));
    }
    for slide in &plan.slides {
        let file_name = svg_filename_for_slide(slide);
        let path = svg_output.join(&file_name);
        if !path.is_file() {
            return Err(AppError::NotFound(format!(
                "原生模式缺少 SVG: {file_name} ({})",
                path.display()
            )));
        }
    }
    Ok(())
}

fn skill_excerpt(skill_text: &str) -> String {
    skill_text
        .lines()
        .filter(|line| {
            line.contains("SVG")
                || line.contains("spec_lock")
                || line.contains("design_spec")
                || line.contains("Quality")
                || line.contains("viewBox")
                || line.contains("SEQUENTIAL")
                || line.contains("Executor")
        })
        .take(80)
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_for_prompt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("\n...[truncated]\n");
    out
}

#[cfg(test)]
fn parse_native_slide_plan_json(raw: &str) -> Result<SlidePlan, AppError> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let json_text = if serde_json::from_str::<serde_json::Value>(stripped).is_ok() {
        stripped
    } else {
        let start = raw
            .find('{')
            .ok_or_else(|| AppError::Custom("AI 返回中找不到 JSON 起点".into()))?;
        let end = raw
            .rfind('}')
            .ok_or_else(|| AppError::Custom("AI 返回中找不到 JSON 终点".into()))?;
        if end <= start {
            return Err(AppError::Custom("AI 返回 JSON 范围无效".into()));
        }
        &raw[start..=end]
    };
    let mut value = serde_json::from_str::<serde_json::Value>(json_text).map_err(|error| {
        AppError::Custom(format!("AI 返回的 slide_plan JSON 无法解析: {error}"))
    })?;

    // Native rendering always applies the locked Pomegranate/ppt-master theme after planning.
    // Some single-turn models return a prose theme description even when asked for an object;
    // normalize only that non-executed field while keeping every page contract strict.
    value["theme"] = serde_json::to_value(default_theme())
        .map_err(|error| AppError::Custom(format!("序列化 native 默认主题失败: {error}")))?;
    for (field, fallback) in [
        ("title", ""),
        ("subtitle", ""),
        ("audience", ""),
        ("style", ""),
    ] {
        normalize_native_string_field(&mut value, field, fallback);
    }
    normalize_native_object_array_field(&mut value, "themeAllocation");
    if let Some(allocations) = value
        .get_mut("themeAllocation")
        .and_then(serde_json::Value::as_array_mut)
    {
        for allocation in allocations {
            for field in ["pageId", "assignedTheme", "exclusiveScope"] {
                normalize_native_string_field(allocation, field, "");
            }
        }
    }
    if let Some(slides) = value
        .get_mut("slides")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (index, slide) in slides.iter_mut().enumerate() {
            if let Some(object) = slide.as_object_mut() {
                let page_number = serde_json::Value::Number(serde_json::Number::from(index + 1));
                object.insert("page".to_string(), page_number.clone());
                object.insert("pageIndex".to_string(), page_number);
            }
            let page_title = format!("第 {} 页", index + 1);
            let page_id = format!("P{:02}", index + 1);
            let file_stem = format!("slide_{}", index + 1);
            for (field, fallback) in [
                ("pageId", page_id.as_str()),
                ("type", if index == 0 { "cover" } else { "content" }),
                ("layout", ""),
                ("title", page_title.as_str()),
                ("subtitle", ""),
                ("visualHint", ""),
                ("pageTheme", ""),
                ("mainClaim", ""),
                ("coreMessage", ""),
                ("contentScope", ""),
                ("relation", ""),
                ("density", ""),
                ("visualIntent", ""),
                ("pageRhythm", ""),
                ("chartRef", ""),
                ("chartType", ""),
                ("fileStem", file_stem.as_str()),
                ("speakerNote", ""),
            ] {
                normalize_native_string_field(slide, field, fallback);
            }
            for field in ["bullets", "evidence", "mustInclude", "mustAvoid"] {
                normalize_native_string_array_field(slide, field);
            }
            normalize_native_object_array_field(slide, "contentBlocks");
        }
    }

    serde_json::from_value::<SlidePlan>(value)
        .map_err(|error| AppError::Custom(format!("AI 返回的 slide_plan JSON 无法解析: {error}")))
}

#[cfg(test)]
fn normalize_native_string_field(value: &mut serde_json::Value, field: &str, fallback: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if !object.get(field).is_some_and(serde_json::Value::is_string) {
        object.insert(
            field.to_string(),
            serde_json::Value::String(fallback.to_string()),
        );
    }
}

#[cfg(test)]
fn normalize_native_string_array_field(value: &mut serde_json::Value, field: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let current = object
        .entry(field.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if current.is_null() {
        *current = serde_json::Value::Array(Vec::new());
        return;
    }
    if let Some(text) = current.as_str() {
        let trimmed = text.trim();
        *current = if trimmed.is_empty() {
            serde_json::Value::Array(Vec::new())
        } else {
            serde_json::Value::Array(vec![serde_json::Value::String(trimmed.to_string())])
        };
    }
}

#[cfg(test)]
fn normalize_native_object_array_field(value: &mut serde_json::Value, field: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if !object.get(field).is_some_and(serde_json::Value::is_array) {
        object.insert(field.to_string(), serde_json::Value::Array(Vec::new()));
    }
}

fn parse_slide_plan_json(raw: &str) -> Result<SlidePlan, AppError> {
    let trimmed = raw.trim();
    if let Ok(plan) = serde_json::from_str::<SlidePlan>(trimmed) {
        return Ok(plan);
    }
    let stripped = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(plan) = serde_json::from_str::<SlidePlan>(stripped) {
        return Ok(plan);
    }
    let start = raw
        .find('{')
        .ok_or_else(|| AppError::Custom("AI 返回中找不到 JSON 起点".into()))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| AppError::Custom("AI 返回中找不到 JSON 终点".into()))?;
    if end <= start {
        return Err(AppError::Custom("AI 返回 JSON 范围无效".into()));
    }
    serde_json::from_str::<SlidePlan>(&raw[start..=end])
        .map_err(|e| AppError::Custom(format!("AI 返回的 slide_plan JSON 无法解析: {}", e)))
}

fn detect_slide_plan_duplicates(plan: &SlidePlan) -> Option<String> {
    let mut issues = Vec::new();
    let mut allocation_pages = std::collections::HashMap::<String, Vec<String>>::new();
    for allocation in &plan.theme_allocation {
        let key = normalize_topic_key(&allocation.assigned_theme);
        if !key.is_empty() {
            allocation_pages
                .entry(key)
                .or_default()
                .push(allocation.page_id.clone());
        }
    }
    for (_key, pages) in allocation_pages {
        if pages.len() > 1 {
            issues.push(format!(
                "[Slide Plan] duplicate detected pages={} reason=duplicated assignedTheme action=regenerate_with_theme_allocation",
                pages.join(",")
            ));
        }
    }
    for pair in plan.slides.windows(2) {
        let left = slide_topic_text(&pair[0]);
        let right = slide_topic_text(&pair[1]);
        let overlap = token_overlap_score(&left, &right);
        if overlap >= 0.62 {
            issues.push(format!(
                "P{:02}/P{:02} adjacent topic overlap {:.2}: {} / {}",
                pair[0].page, pair[1].page, overlap, pair[0].title, pair[1].title
            ));
        }
        let include_overlap = token_overlap_score(
            &pair[0].must_include.join(" "),
            &pair[1].must_include.join(" "),
        );
        if include_overlap >= 0.58 {
            issues.push(format!(
                "[Slide Plan] duplicate detected pages=P{:02},P{:02} reason=overlapping mustInclude {:.2} action=regenerate_with_theme_allocation",
                pair[0].page, pair[1].page, include_overlap
            ));
        }
        let scope_overlap = token_overlap_score(&pair[0].content_scope, &pair[1].content_scope);
        if scope_overlap >= 0.58 {
            issues.push(format!(
                "[Slide Plan] duplicate detected pages=P{:02},P{:02} reason=overlapping contentScope {:.2} action=regenerate_with_theme_allocation",
                pair[0].page, pair[1].page, scope_overlap
            ));
        }
    }

    for i in 0..plan.slides.len() {
        for j in (i + 1)..plan.slides.len() {
            let left = &plan.slides[i];
            let right = &plan.slides[j];
            let theme_overlap = token_overlap_score(&left.page_theme, &right.page_theme);
            if theme_overlap >= 0.72 {
                issues.push(format!(
                    "[Slide Plan] duplicate detected pages=P{:02},P{:02} reason=overlapping theme {:.2}: {} / {} action=regenerate_with_theme_allocation",
                    left.page, right.page, theme_overlap, left.page_theme, right.page_theme
                ));
            }
            let claim_overlap = token_overlap_score(&left.main_claim, &right.main_claim);
            if claim_overlap >= 0.72 {
                issues.push(format!(
                    "[Slide Plan] duplicate detected pages=P{:02},P{:02} reason=overlapping mainClaim {:.2} action=regenerate_with_theme_allocation",
                    left.page, right.page, claim_overlap
                ));
            }
        }
    }

    for (keyword, pages) in repeated_content_keywords(plan) {
        if pages.len() >= 3 {
            issues.push(format!(
                "[Slide Plan] duplicate detected pages={} reason=high-frequency keyword `{}` action=regenerate_with_theme_allocation",
                pages.join(","),
                keyword
            ));
        }
    }
    for pair in plan.slides.windows(2) {
        let left_terms = content_keywords(&slide_full_text(&pair[0]));
        let right_terms = content_keywords(&slide_full_text(&pair[1]));
        let overlap = left_terms
            .iter()
            .filter(|term| right_terms.contains(*term))
            .take(2)
            .count();
        if overlap >= 2 {
            issues.push(format!(
                "[Slide Plan] duplicate detected pages=P{:02},P{:02} reason=consecutive repeated content keywords action=regenerate_with_theme_allocation",
                pair[0].page, pair[1].page
            ));
        }
    }

    for claim in [
        "全国唯一",
        "全国第一",
        "连续三年",
        "连续五年",
        "20+",
        "6个国家级",
        "国家科技一等奖",
        "两院院士",
    ] {
        let pages: Vec<String> = plan
            .slides
            .iter()
            .filter(|slide| slide_full_text(slide).contains(claim))
            .map(|slide| format!("P{:02}", slide.page))
            .collect();
        if !pages.is_empty() {
            issues.push(format!(
                "unsupported definite claim `{}` appears on {}",
                claim,
                pages.join(", ")
            ));
        }
    }

    if issues.is_empty() {
        None
    } else {
        Some(issues.join("; "))
    }
}

fn slide_topic_text(slide: &Slide) -> String {
    [
        slide.title.as_str(),
        slide.subtitle.as_str(),
        slide.page_theme.as_str(),
        slide.main_claim.as_str(),
        slide.content_scope.as_str(),
    ]
    .join(" ")
}

fn normalize_topic_key(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !ch.is_ascii()
            && !ch.is_whitespace()
            && !"，。；、：:（）()《》“”\"'!?！？-_ ".contains(ch)
        {
            out.push(ch);
        }
        if out.chars().count() >= 24 {
            break;
        }
    }
    out
}

fn slide_full_text(slide: &Slide) -> String {
    let block_text = slide
        .content_blocks
        .iter()
        .map(content_block_display)
        .collect::<Vec<_>>()
        .join(" ");
    [
        slide_topic_text(slide),
        stable_core_message(slide),
        block_text,
        slide.evidence.join(" "),
        slide.bullets.join(" "),
        slide.must_include.join(" "),
        slide.must_avoid.join(" "),
        slide.speaker_note.clone(),
    ]
    .join(" ")
}

fn token_overlap_score(left: &str, right: &str) -> f32 {
    let left_tokens = content_tokens(left);
    let right_tokens = content_tokens(right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }
    let overlap = left_tokens.intersection(&right_tokens).count() as f32;
    overlap / left_tokens.len().min(right_tokens.len()) as f32
}

fn content_tokens(text: &str) -> std::collections::HashSet<String> {
    let mut tokens = std::collections::HashSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else {
            if current.len() >= 2 {
                tokens.insert(current.clone());
            }
            current.clear();
            if !ch.is_ascii()
                && !ch.is_whitespace()
                && !"，。；、：:（）()《》“”\"'!?！？".contains(ch)
            {
                tokens.insert(ch.to_string());
            }
        }
    }
    if current.len() >= 2 {
        tokens.insert(current);
    }
    tokens
}

fn content_keywords(text: &str) -> std::collections::HashSet<String> {
    content_tokens(text)
        .into_iter()
        .filter(|token| {
            let chars = token.chars().count();
            chars >= 2 && !generic_content_stopwords().contains(&token.as_str())
        })
        .collect()
}

fn repeated_content_keywords(plan: &SlidePlan) -> Vec<(String, Vec<String>)> {
    let mut by_keyword: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for slide in &plan.slides {
        for keyword in content_keywords(&slide_topic_text(slide)) {
            by_keyword
                .entry(keyword)
                .or_default()
                .push(format!("P{:02}", slide.page));
        }
    }
    by_keyword
        .into_iter()
        .filter(|(_, pages)| pages.len() >= 3)
        .collect()
}

fn generic_content_stopwords() -> &'static [&'static str] {
    &[
        "ppt", "page", "slide", "title", "theme", "claim", "scope", "main", "with", "from", "this",
        "that", "内容", "主题", "页面", "核心", "观点", "材料", "用户", "当前", "本页", "讲解",
        "说明", "展示", "总结", "归纳", "分析", "结构", "要点", "重点", "信息", "表达",
    ]
}

fn log_slide_plan_summary(plan: &SlidePlan, log_lines: &mut Vec<String>) {
    let header = "[Slide Plan Generated]".to_string();
    println!("{}", header);
    log_lines.push(header);
    for slide in &plan.slides {
        let row = format!(
            "P{:02}: title={} / pageTheme={} / mainClaim={} / contentScope={}",
            slide.page,
            compact_log_text(&slide.title, ""),
            compact_log_text(&slide.page_theme, &slide.title),
            compact_log_text(&slide.main_claim, &slide.subtitle),
            compact_log_text(&slide.content_scope, "")
        );
        println!("{}", row);
        log_lines.push(row);
    }
}

fn log_design_spec_pages(
    plan: &SlidePlan,
    mapping: &PptMasterStyleMapping,
    log_lines: &mut Vec<String>,
) {
    let header = "[Design Spec Pages]".to_string();
    println!("{}", header);
    log_lines.push(header);
    for slide in &plan.slides {
        let row = format!(
            "P{:02}: pageTheme={} / mainClaim={} / chartType={} / pageRhythm={}",
            slide.page,
            compact_log_text(&slide.page_theme, &slide.title),
            compact_log_text(&slide.main_claim, &slide.subtitle),
            chart_reference_for_slide(slide, mapping).unwrap_or_else(|| "none".to_string()),
            page_rhythm_for_slide(slide)
        );
        println!("{}", row);
        log_lines.push(row);
    }
}

fn log_svg_page_task(slide: &Slide, mapping: &PptMasterStyleMapping, log_lines: &mut Vec<String>) {
    let row = format!(
        "[SVG Page Task] P{:02}: title={} / pageTheme={} / mainClaim={} / mustAvoid={} / chartType={} / pageRhythm={}",
        slide.page,
        compact_log_text(&slide.title, ""),
        compact_log_text(&slide.page_theme, &slide.title),
        compact_log_text(&slide.main_claim, &slide.subtitle),
        if slide.must_avoid.is_empty() {
            "none".to_string()
        } else {
            compact_log_text(&slide.must_avoid.join("; "), "")
        },
        chart_reference_for_slide(slide, mapping).unwrap_or_else(|| "none".to_string()),
        page_rhythm_for_slide(slide)
    );
    println!("{}", row);
    log_lines.push(row);
}

fn compact_log_text(value: &str, fallback: &str) -> String {
    let text = if value.trim().is_empty() {
        fallback
    } else {
        value
    }
    .trim();
    let mut out: String = text.chars().take(32).collect();
    if text.chars().count() > 32 {
        out.push_str("...");
    }
    out
}

fn normalize_slide_plan(
    mut plan: SlidePlan,
    title: &str,
    slide_count: usize,
    style: &str,
) -> SlidePlan {
    if plan.title.trim().is_empty() {
        plan.title = title.to_string();
    }
    if plan.style.trim().is_empty() {
        plan.style = style.to_string();
    }
    plan.theme = theme_for_style(&plan.style);
    if plan.subtitle.trim().is_empty() {
        plan.subtitle = "结构清晰、重点突出的演示文稿".to_string();
    }
    if plan.audience.trim().is_empty() {
        plan.audience = "目标听众".to_string();
    }
    if plan.slides.is_empty() {
        let mut fallback = default_slide_plan(title, slide_count, style, "");
        refresh_theme_allocation_and_must_avoid(&mut fallback);
        return fallback;
    }
    if plan.slides.len() > slide_count {
        plan.slides.truncate(slide_count);
    }
    let total_slides = plan.slides.len();
    for (idx, slide) in plan.slides.iter_mut().enumerate() {
        slide.page = idx + 1;
        slide.page_index = idx + 1;
        if slide.page_id.trim().is_empty() {
            slide.page_id = format!("P{:02}", idx + 1);
        }
        if slide.slide_type.trim().is_empty() {
            slide.slide_type = if idx == 0 { "cover" } else { "content" }.to_string();
        }
        if slide.layout.trim().is_empty() {
            slide.layout = choose_layout(
                idx,
                total_slides,
                &slide.title,
                &slide.subtitle,
                &slide.bullets,
            );
        }
        slide.layout = normalize_layout(&slide.layout, idx, total_slides);
        if slide.title.trim().is_empty() {
            slide.title = if idx == 0 {
                plan.title.clone()
            } else {
                format!("第 {} 页", idx + 1)
            };
        }
        if slide.subtitle.trim().is_empty() {
            slide.subtitle = "本页核心观点".to_string();
        }
        if slide.page_theme.trim().is_empty() {
            slide.page_theme = slide.title.clone();
        }
        if slide.main_claim.trim().is_empty() {
            slide.main_claim = slide.subtitle.clone();
        }
        if slide.core_message.trim().is_empty() {
            slide.core_message = slide.main_claim.clone();
        }
        if slide.content_scope.trim().is_empty() {
            slide.content_scope = if slide.bullets.is_empty() {
                slide.subtitle.clone()
            } else {
                slide.bullets.join("; ")
            };
        }
        if slide.evidence.is_empty() {
            slide.evidence = evidence_from_slide_text(slide);
        }
        if slide.content_blocks.is_empty() {
            slide.content_blocks = content_blocks_from_slide(slide);
        }
        if idx > 0 && slide.content_blocks.len() < 2 {
            let mut more = content_blocks_from_units(&slide.evidence, &slide.title);
            slide.content_blocks.append(&mut more);
            dedup_content_blocks(&mut slide.content_blocks);
        }
        if slide.relation.trim().is_empty() {
            slide.relation = relation_for_layout(&slide.layout).to_string();
        }
        if slide.chart_type.trim().is_empty() {
            slide.chart_type = chart_type_for_layout(&slide.layout).to_string();
        }
        if slide.density.trim().is_empty() {
            slide.density = density_for_layout(&slide.layout, idx, total_slides).to_string();
        }
        if slide.visual_intent.trim().is_empty() {
            slide.visual_intent = slide.visual_hint.clone();
        }
        if idx > 0 && slide.bullets.is_empty() {
            slide.bullets = bullets_from_blocks_or_evidence(slide);
        }
        if slide.must_include.is_empty() {
            slide.must_include = slide
                .content_blocks
                .iter()
                .map(content_block_display)
                .chain(slide.bullets.iter().cloned())
                .take(3)
                .collect();
        }
        if slide.must_avoid.is_empty() {
            slide.must_avoid = vec![
                "Do not repeat other pages as the main topic".to_string(),
                "Do not re-plan the whole deck on this page".to_string(),
            ];
        }
        if slide.bullets.len() > 5 {
            slide.bullets.truncate(5);
        }
        if slide.visual_hint.trim().is_empty() {
            slide.visual_hint = visual_hint_for_layout(&slide.layout).to_string();
        }
        if slide.speaker_note.trim().is_empty() {
            slide.speaker_note = format!("讲解本页：{}", slide.subtitle);
        }
    }
    refresh_theme_allocation_and_must_avoid(&mut plan);
    plan
}

fn stable_markdown_link_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?i)\[([^\]\r\n]{1,160})\]\(\s*(?:https?://|www\.)[^)\r\n]*\)")
            .expect("valid stable markdown link regex")
    })
}

fn stable_raw_url_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?i)(?:https?://|www\.)[^\s<>{}\[\]，。；;！!？?]+")
            .expect("valid stable raw URL regex")
    })
}

fn stable_url_fragment_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:[a-z0-9-]+\.)?(?:wikipedia(?:\.org)?|org/wiki|wiki/|cite_note)[^\s，。；;！!？?]*",
        )
        .expect("valid stable URL fragment regex")
    })
}

fn stable_percent_encoded_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?i)[^\s，。；;！!？?]*%[0-9a-f]{2}[^\s，。；;！!？?]*")
            .expect("valid stable percent-encoded URL regex")
    })
}

fn stable_malformed_citation_prefix_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?m)(^|[\s，。；;！？!?])\d{1,3}\s*[（(]\s*\[\s*(?:\\+\s*)?\[\s*(?:注\s*)?\d{1,3}(?:\s*[-–—]\s*\d{1,3})?\s*\]?\s*[（(]?",
        )
        .expect("valid stable malformed citation prefix regex")
    })
}

fn stable_escaped_citation_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"\[\s*(?:\\+\s*)?\[\s*(?:注\s*)?\d{1,3}(?:\s*[-–—]\s*\d{1,3})?\s*\]?")
            .expect("valid stable escaped citation regex")
    })
}

fn stable_bracketed_citation_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(
            r"(?:\[\s*(?:注\s*)?\d{1,3}(?:\s*[-–—]\s*\d{1,3})?\s*\]|【\s*(?:注\s*)?\d{1,3}(?:\s*[-–—]\s*\d{1,3})?\s*】|\\+\]\])",
        )
        .expect("valid stable bracketed citation regex")
    })
}

fn stable_orphan_note_prefix_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?m)(^|[\s，。；;：:])注\s*\d{1,3}\s*[（(]")
            .expect("valid stable orphan note prefix regex")
    })
}

fn stable_leading_reference_cluster_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?m)^\s*\d{1,3}\s*[（(]\s*\d{1,4}\s*([，。；;：:\p{Han}A-Za-z])")
            .expect("valid stable leading reference cluster regex")
    })
}

fn stable_trailing_reference_cluster_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?m)(^|[\s，。；;])(?:注\s*)?\d{1,3}\s*[（(]\s*\d{1,3}\s*$")
            .expect("valid stable trailing reference cluster regex")
    })
}

fn stable_isolated_reference_number_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:注\s*)?[1-9]\d{0,2}\s*$")
            .expect("valid stable isolated reference number regex")
    })
}

fn stable_leading_reference_number_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:注\s*)?[1-9]\d{0,2}\s*[，,]\s*")
            .expect("valid stable leading reference number regex")
    })
}

fn stable_empty_parentheses_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?:\(\s*\)|（\s*）)").expect("valid stable empty parentheses regex")
    })
}

fn stable_orphan_parenthesized_reference_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?m)\s*[（(]\s*\d{1,4}\s*(?:$|([，。；;]))")
            .expect("valid stable orphan parenthesized reference regex")
    })
}

fn stable_orphan_opening_parenthesis_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"\s*[（(]\s*([，。；;])")
            .expect("valid stable orphan opening parenthesis regex")
    })
}

fn stable_html_tag_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"<[^>\r\n]+>").expect("valid stable HTML tag regex"))
}

fn split_stable_visible_clauses(value: &str) -> Vec<String> {
    let mut clauses = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if matches!(
            ch,
            '\n' | '\r' | '。' | '！' | '？' | '!' | '?' | '；' | ';'
        ) {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                clauses.push(trimmed.to_string());
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        clauses.push(current.trim().to_string());
    }
    clauses
}

fn looks_like_stable_design_instruction(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    let page_directive = normalized.starts_with("slide ")
        || normalized.starts_with("page ")
        || value.trim_start().starts_with("本页")
        || value.trim_start().starts_with("该页")
        || (value.trim_start().starts_with('第')
            && value.chars().take(12).collect::<String>().contains('页'));
    let design_action = [
        "建议",
        "可用",
        "采用",
        "布局",
        "排版",
        "视觉",
        "图表",
        "卡片",
        "配图",
        "半透明",
        "标题",
        "副标题",
        "页脚",
        "横线",
        "居中",
        "左侧",
        "右侧",
        "背景",
        "呈现方式",
        "visual advice",
        "design advice",
        "layout intent",
        "visual intent",
    ]
    .iter()
    .any(|token| normalized.contains(&token.to_ascii_lowercase()));
    let planning_heading = [
        "visual expression advice",
        "suggested page structure",
        "page recommendation",
        "diversity reason",
        "视觉与表达建议",
        "建议页面结构",
    ]
    .iter()
    .any(|token| normalized.contains(&token.to_ascii_lowercase()));
    let cover_directive = (value.trim_start().starts_with("封面")
        || normalized.starts_with("cover"))
        && design_action;
    let outline_marker = value.trim().chars().count() <= 16
        && value.trim_start().starts_with('第')
        && value.contains("部分");
    let cover_heading = value.trim().starts_with("封面·总览")
        || value.trim().starts_with("封面总览")
        || value.trim().eq_ignore_ascii_case("cover overview");
    let question_residue = value.trim().chars().count() <= 20
        && value.trim().ends_with("部分")
        && value.contains("评价");
    page_directive && design_action
        || cover_directive
        || planning_heading
        || outline_marker
        || cover_heading
        || question_residue
}

fn looks_like_stable_internal_question(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    let question_form = value.contains("是否")
        || value.contains("请说明")
        || value.contains("希望用何种")
        || value.contains("如需展示")
        || normalized.contains("should this")
        || normalized.contains("please confirm");
    let production_topic = [
        "ppt",
        "汇报",
        "页面",
        "展示",
        "措辞",
        "视觉处理",
        "呈现方式",
        "平衡度",
        "图表",
        "评价部分",
        "不同视角",
        "价值引导",
        "展开",
        "陈述",
        "采用",
        "保留",
        "需要",
    ]
    .iter()
    .any(|token| normalized.contains(&token.to_ascii_lowercase()));
    question_form && production_topic || value.trim().starts_with("若有倾向")
}

fn looks_like_stable_internal_metadata(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    [
        "user-edited structured ai understanding",
        "user-edited struct",
        "structured ai understanding",
        "planning context mirror",
        "legacy ai understanding result",
        "legacy prompt - compatibility only",
        "fact safety rules",
        "pomegranate planning context",
        "fallback reason",
        "ai understanding result",
        "本材料源自",
        "材料来源",
        "素材来源",
        "文中提到",
    ]
    .iter()
    .any(|token| normalized.contains(token))
}

fn remove_unmatched_stable_square_brackets(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut keep = vec![true; chars.len()];
    let mut ascii_open = Vec::new();
    let mut wide_open = Vec::new();
    for (index, ch) in chars.iter().enumerate() {
        match ch {
            '[' => ascii_open.push(index),
            ']' => {
                if ascii_open.pop().is_none() {
                    keep[index] = false;
                }
            }
            '【' => wide_open.push(index),
            '】' => {
                if wide_open.pop().is_none() {
                    keep[index] = false;
                }
            }
            _ => {}
        }
    }
    for index in ascii_open.into_iter().chain(wide_open) {
        keep[index] = false;
    }
    chars
        .into_iter()
        .zip(keep)
        .filter_map(|(ch, keep)| keep.then_some(ch))
        .collect()
}

fn remove_redundant_stable_backslashes(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(value.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '\\'
            && chars
                .get(index + 1)
                .is_none_or(|next| matches!(next, '[' | ']' | '*' | '#' | '_' | '`' | '\\'))
        {
            continue;
        }
        output.push(*ch);
    }
    output
}

fn clean_stable_reference_artifacts(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }
    let mut cleaned = stable_markdown_link_regex()
        .replace_all(value, "$1")
        .into_owned();
    cleaned = stable_html_tag_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = stable_raw_url_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = stable_url_fragment_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = stable_percent_encoded_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = stable_malformed_citation_prefix_regex()
        .replace_all(&cleaned, "$1")
        .into_owned();
    cleaned = stable_escaped_citation_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = stable_bracketed_citation_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = stable_orphan_note_prefix_regex()
        .replace_all(&cleaned, "$1")
        .into_owned();
    cleaned = stable_leading_reference_cluster_regex()
        .replace_all(&cleaned, "$1")
        .into_owned();
    cleaned = stable_trailing_reference_cluster_regex()
        .replace_all(&cleaned, "$1")
        .into_owned();
    cleaned = stable_isolated_reference_number_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = stable_leading_reference_number_regex()
        .replace_all(&cleaned, "")
        .into_owned();
    cleaned = stable_orphan_parenthesized_reference_regex()
        .replace_all(&cleaned, "$1")
        .into_owned();
    cleaned = stable_orphan_opening_parenthesis_regex()
        .replace_all(&cleaned, "$1")
        .into_owned();
    cleaned = stable_empty_parentheses_regex()
        .replace_all(&cleaned, " ")
        .into_owned();
    cleaned = remove_redundant_stable_backslashes(&cleaned);
    cleaned = remove_unmatched_stable_square_brackets(&cleaned);
    let decoded = cleaned
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'");
    decoded
        .trim_start_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '，' | ',' | '；' | ';' | '：' | ':')
        })
        .to_string()
}

fn stable_markdown_heading_prefix(value: &str) -> Option<(usize, &str)> {
    let trimmed = value.trim_start();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let remainder = &trimmed[hashes..];
    remainder
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
        .then(|| (hashes, remainder.trim_start()))
}

fn strip_stable_list_prefix(value: &str) -> &str {
    let trimmed = value.trim_start();
    for marker in ["- ", "* ", "> ", "• ", "· "] {
        if let Some(remainder) = trimmed.strip_prefix(marker) {
            return remainder.trim_start();
        }
    }
    trimmed
}

fn sanitize_visible_text(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }
    let cleaned = clean_stable_reference_artifacts(value);

    let mut visible = Vec::new();
    for raw_line in cleaned.lines() {
        let (heading_level, line) = stable_markdown_heading_prefix(raw_line).map_or(
            (None, strip_stable_list_prefix(raw_line)),
            |(level, body)| (Some(level), body),
        );
        let mut line_clauses = Vec::new();
        for clause in split_stable_visible_clauses(line) {
            let clause = clause.trim();
            if clause.is_empty()
                || looks_like_stable_internal_metadata(clause)
                || looks_like_stable_design_instruction(clause)
                || looks_like_stable_internal_question(clause)
            {
                continue;
            }
            let lower = clause.to_ascii_lowercase();
            if lower.contains("http")
                || lower.contains("cite_note")
                || lower.contains("org/wiki")
                || lower.contains("wikipedia")
                || lower.contains("](")
            {
                continue;
            }
            let collapsed = clause.split_whitespace().collect::<Vec<_>>().join(" ");
            if collapsed
                .chars()
                .any(|ch| ch.is_alphanumeric() || ('\u{3400}'..='\u{9fff}').contains(&ch))
            {
                line_clauses.push(collapsed);
            }
        }
        if !line_clauses.is_empty() {
            let mut line = line_clauses.join("；");
            if let Some(level) = heading_level {
                line = format!("{} {}", "#".repeat(level), line);
            }
            visible.push(line);
        }
    }
    visible.join("\n")
}

fn extract_material_units(text: &str) -> Vec<String> {
    let text = sanitize_visible_text(text);
    let mut units = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if matches!(
            ch,
            '。' | '！' | '？' | '；' | '\n' | '\r' | '.' | '!' | '?' | ';'
        ) {
            push_material_unit(&mut units, &current);
            current.clear();
        }
    }
    push_material_unit(&mut units, &current);
    units
        .into_iter()
        .filter(|unit| !is_internal_or_placeholder_unit(unit))
        .take(80)
        .collect()
}

fn push_material_unit(units: &mut Vec<String>, value: &str) {
    let cleaned = value
        .trim()
        .trim_matches(|ch: char| {
            ch.is_ascii_punctuation() || "，。；：、！？（）()[]【】".contains(ch)
        })
        .trim();
    if cleaned.chars().count() >= 8 {
        units.push(cleaned.chars().take(90).collect());
    }
}

fn is_internal_or_placeholder_unit(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if looks_like_stable_internal_metadata(value)
        || looks_like_stable_design_instruction(value)
        || looks_like_stable_internal_question(value)
        || lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("cite_note")
        || lower.contains("org/wiki")
        || lower.contains("wikipedia")
    {
        return true;
    }
    let banned = [
        "[Confirmed Prompt]",
        "[Pomegranate Planning Context]",
        "[AI Understanding Result]",
        "[User-Edited Structured AI Understanding]",
        "[Planning Context Mirror]",
        "[Legacy AI Understanding Result]",
        "[Legacy Prompt - compatibility only",
        "[Audience]",
        "[Suggested Page Structure]",
        "[Narrative Mainline]",
        "[Visual Expression Advice]",
        "[Extra Requirements]",
        "[Raw Material",
        "[Fact Safety Rules]",
        "提炼用户材料中的关键信息",
        "围绕当前主题组织重点表达",
        "使用短句和结构化表达",
        "概括本主题的核心内容",
    ];
    banned.iter().any(|item| value.contains(item))
}

fn select_units_for_slide(units: &[String], idx: usize, total: usize) -> Vec<String> {
    if units.is_empty() {
        return Vec::new();
    }
    if idx == 0 {
        return units.iter().take(2).cloned().collect();
    }
    let content_pages = total.saturating_sub(1).max(1);
    let page = idx.saturating_sub(1);
    let chunk = ((units.len() as f32) / (content_pages as f32))
        .ceil()
        .max(2.0) as usize;
    let start = page
        .saturating_mul(chunk)
        .min(units.len().saturating_sub(1));
    let end = (start + chunk).min(units.len());
    let mut selected: Vec<String> = units[start..end].iter().take(4).cloned().collect();
    if selected.len() < 2 {
        selected.extend(
            units
                .iter()
                .skip(start + selected.len())
                .take(2 - selected.len())
                .cloned(),
        );
    }
    selected
}

fn title_from_material_unit(unit: &str, idx: usize) -> String {
    let mut title = unit
        .split(['，', '。', '；', '：', ',', '.', ';', ':'])
        .next()
        .unwrap_or(unit)
        .trim()
        .chars()
        .take(18)
        .collect::<String>();
    if title.chars().count() < 4 {
        title = format!("第 {} 部分", idx + 1);
    }
    title
}

fn fallback_bullets_from_text(text: &str) -> Vec<String> {
    let mut bullets = extract_material_units(text);
    if bullets.is_empty() {
        let cleaned = text.trim();
        if !cleaned.is_empty() {
            bullets.push(cleaned.chars().take(60).collect());
        }
    }
    bullets.truncate(4);
    bullets
}

fn content_blocks_from_units(units: &[String], fallback_label: &str) -> Vec<ContentBlock> {
    units
        .iter()
        .filter(|unit| !is_internal_or_placeholder_unit(unit))
        .take(6)
        .enumerate()
        .filter_map(|(idx, unit)| {
            let label = title_from_material_unit(unit, idx);
            let block = ContentBlock {
                label: if label.trim().is_empty() {
                    fallback_label.chars().take(12).collect()
                } else {
                    label
                },
                text: unit.chars().take(52).collect(),
                detail: unit.chars().skip(52).take(90).collect(),
            };
            sanitize_visible_block(&block, idx)
        })
        .collect()
}

fn content_blocks_from_slide(slide: &Slide) -> Vec<ContentBlock> {
    let mut units = Vec::new();
    units.extend(slide.bullets.iter().cloned());
    units.extend(slide.evidence.iter().cloned());
    if units.is_empty() {
        units.extend(extract_material_units(&format!(
            "{} {} {} {}",
            slide.title, slide.subtitle, slide.main_claim, slide.content_scope
        )));
    }
    content_blocks_from_units(&units, &slide.title)
}

fn evidence_from_slide_text(slide: &Slide) -> Vec<String> {
    let mut units = Vec::new();
    units.extend(slide.bullets.iter().cloned());
    units.extend(extract_material_units(&format!(
        "{} {} {}",
        slide.subtitle, slide.main_claim, slide.content_scope
    )));
    units
        .into_iter()
        .filter(|unit| !is_internal_or_placeholder_unit(unit))
        .take(6)
        .collect()
}

fn dedup_content_blocks(blocks: &mut Vec<ContentBlock>) {
    let mut seen = std::collections::HashSet::new();
    blocks.retain(|block| {
        let key = format!("{} {}", block.label.trim(), block.text.trim());
        !key.trim().is_empty() && seen.insert(key)
    });
    if blocks.len() > 6 {
        blocks.truncate(6);
    }
}

fn content_block_display(block: &ContentBlock) -> String {
    let label = block.label.trim();
    let text = block.text.trim();
    let detail = block.detail.trim();
    if !label.is_empty() && !text.is_empty() {
        format!("{}: {}", label, text)
    } else if !text.is_empty() {
        text.to_string()
    } else {
        detail.to_string()
    }
}

fn bullets_from_blocks_or_evidence(slide: &Slide) -> Vec<String> {
    let mut bullets: Vec<String> = slide
        .content_blocks
        .iter()
        .map(content_block_display)
        .filter(|item| !item.trim().is_empty())
        .take(4)
        .collect();
    if bullets.len() < 2 {
        bullets.extend(slide.evidence.iter().take(4 - bullets.len()).cloned());
    }
    if bullets.len() < 2 {
        bullets.extend(fallback_bullets_from_text(&format!(
            "{} {} {}",
            slide.title, slide.subtitle, slide.content_scope
        )));
    }
    bullets.truncate(5);
    bullets
}

fn relation_for_layout(layout: &str) -> &'static str {
    match layout {
        "timeline" => "timeline",
        "compare" | "matrix" => "compare",
        "process" => "process",
        "cards" | "image_text" => "category",
        "highlight" => "cause",
        _ => "none",
    }
}

fn chart_type_for_layout(layout: &str) -> &'static str {
    match layout {
        "timeline" => "timeline",
        "compare" => "compare",
        "process" => "process",
        "matrix" => "matrix",
        "highlight" => "highlight",
        "summary" => "summary",
        _ => "cards",
    }
}

fn density_for_layout(layout: &str, idx: usize, total: usize) -> &'static str {
    if idx == 0 || idx + 1 == total {
        "anchor"
    } else {
        match layout {
            "highlight" | "image_text" => "breathing",
            "cards" | "timeline" | "compare" | "process" | "matrix" => "dense",
            _ => "dense",
        }
    }
}

fn stable_core_message(slide: &Slide) -> String {
    if !slide.core_message.trim().is_empty() {
        slide.core_message.trim().to_string()
    } else if !slide.main_claim.trim().is_empty() {
        slide.main_claim.trim().to_string()
    } else {
        slide.subtitle.trim().to_string()
    }
}

fn validate_stable_content_plan(plan: &SlidePlan) -> Option<String> {
    let mut issues = Vec::new();
    for slide in &plan.slides {
        let core = stable_core_message(slide);
        if slide.title.trim().is_empty() {
            issues.push(format!("P{:02} title empty", slide.page));
        }
        if core.trim().is_empty() {
            issues.push(format!("P{:02} coreMessage empty", slide.page));
        }
        let min_blocks = if slide.page == 1 { 1 } else { 2 };
        if slide.content_blocks.len() < min_blocks {
            issues.push(format!(
                "P{:02} contentBlocks too few ({})",
                slide.page,
                slide.content_blocks.len()
            ));
        }
        if slide.page > 1 && slide.bullets.len().max(slide.evidence.len()) < 2 {
            issues.push(format!("P{:02} bullets/evidence too few", slide.page));
        }
        if contains_placeholder_phrase(&slide_full_text(slide)) {
            issues.push(format!("P{:02} contains placeholder phrase", slide.page));
        }
    }
    if issues.is_empty() {
        None
    } else {
        Some(issues.join("; "))
    }
}

fn log_stable_content_check(plan: &SlidePlan, label: &str, log_lines: &mut Vec<String>) {
    log_lines.push(label.to_string());
    for slide in &plan.slides {
        let status = if slide.title.trim().is_empty()
            || stable_core_message(slide).trim().is_empty()
            || contains_placeholder_phrase(&slide_full_text(slide))
        {
            "failed"
        } else {
            "passed"
        };
        log_lines.push(format!(
            "P{:02} {} blocks={} evidence={}",
            slide.page,
            status,
            slide.content_blocks.len(),
            slide.evidence.len()
        ));
    }
}

fn enrich_plan_from_material(plan: &mut SlidePlan, material: &str) {
    let units = extract_material_units(material);
    if units.is_empty() {
        return;
    }
    let total = plan.slides.len();
    for (idx, slide) in plan.slides.iter_mut().enumerate() {
        let selected = select_units_for_slide(&units, idx, total);
        if slide.evidence.len() < 2 {
            slide.evidence.extend(selected.iter().cloned());
            slide.evidence.truncate(6);
        }
        if slide.content_blocks.len() < 2 {
            slide
                .content_blocks
                .extend(content_blocks_from_units(&selected, &slide.title));
            dedup_content_blocks(&mut slide.content_blocks);
        }
        if slide.bullets.len() < 2 {
            slide.bullets = bullets_from_blocks_or_evidence(slide);
        }
        if slide.core_message.trim().is_empty() {
            slide.core_message = slide
                .evidence
                .first()
                .cloned()
                .unwrap_or_else(|| slide.subtitle.clone());
        }
    }
}

fn sanitize_visible_list(values: &[String], limit: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .iter()
        .map(|value| sanitize_visible_text(value))
        .filter(|value| !value.trim().is_empty() && seen.insert(value.to_ascii_lowercase()))
        .take(limit)
        .collect()
}

fn strip_stable_label_prefixes(value: &str, label: &str) -> String {
    let mut remaining = value.trim();
    let prefixes = [format!("{}:", label.trim()), format!("{}：", label.trim())];
    loop {
        let next = prefixes
            .iter()
            .find_map(|prefix| remaining.strip_prefix(prefix))
            .map(str::trim);
        match next {
            Some(value) if value != remaining => remaining = value,
            _ => break,
        }
    }
    remaining.to_string()
}

fn clean_stable_block_label(value: &str) -> String {
    let trimmed = value.trim();
    for (open, close) in [('（', '）'), ('(', ')')] {
        if trimmed.contains(open) && !trimmed.contains(close) {
            return trimmed
                .split(open)
                .next()
                .unwrap_or(trimmed)
                .trim()
                .to_string();
        }
    }
    trimmed.to_string()
}

fn repair_stable_split_numeric_boundary(text: &mut String, detail: &mut String) {
    if !detail
        .trim_start()
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        return;
    }
    let trimmed_text = text.trim_end();
    let ends_in_split_number = trimmed_text
        .chars()
        .last()
        .is_some_and(|ch| ch.is_ascii_digit());
    let ends_in_numeric_connector = ["达到", "约为", "总计", "共计"]
        .iter()
        .any(|suffix| trimmed_text.ends_with(suffix));
    if !ends_in_split_number && !ends_in_numeric_connector {
        return;
    }
    *text = format!("{}{}", trimmed_text, detail.trim_start());
    detail.clear();
}

fn sanitize_visible_block(block: &ContentBlock, index: usize) -> Option<ContentBlock> {
    let mut label = sanitize_visible_text(&block.label);
    let mut text = sanitize_visible_text(&block.text);
    let mut detail = sanitize_visible_text(&block.detail);
    text = strip_stable_label_prefixes(&text, &label);
    repair_stable_split_numeric_boundary(&mut text, &mut detail);
    if label.is_empty() && text.is_empty() && detail.is_empty() {
        return None;
    }
    if text.is_empty() {
        text = if detail.is_empty() {
            label.clone()
        } else {
            detail.clone()
        };
    }
    if text.chars().count() < 4 && detail.is_empty() {
        text = label.clone();
    }
    if label.is_empty() {
        label = title_from_material_unit(&text, index);
    }
    label = clean_stable_block_label(&label);
    if detail == text || detail == label || (!detail.is_empty() && text.contains(&detail)) {
        detail.clear();
    }
    Some(ContentBlock {
        label: label.chars().take(28).collect(),
        text: text.chars().take(120).collect(),
        detail: detail.chars().take(140).collect(),
    })
}

fn stable_plan_visible_pool(plan: &SlidePlan, material: &str) -> Vec<String> {
    let mut units = extract_material_units(material);
    for slide in &plan.slides {
        for value in [
            &slide.title,
            &slide.subtitle,
            &slide.main_claim,
            &slide.core_message,
            &slide.content_scope,
        ] {
            units.extend(extract_material_units(value));
        }
        for block in &slide.content_blocks {
            units.extend(extract_material_units(&format!(
                "{}；{}；{}",
                block.label, block.text, block.detail
            )));
        }
        for value in slide.bullets.iter().chain(slide.evidence.iter()) {
            units.extend(extract_material_units(value));
        }
    }
    let mut seen = std::collections::HashSet::new();
    units.retain(|unit| {
        !unit.trim().is_empty()
            && !is_internal_or_placeholder_unit(unit)
            && seen.insert(unit.to_ascii_lowercase())
    });
    units
}

fn stable_semantic_terms(value: &str, deck_title: &str) -> std::collections::HashSet<String> {
    fn collect_terms(value: &str) -> std::collections::HashSet<String> {
        let mut terms = std::collections::HashSet::new();
        let mut cjk_run = Vec::new();
        let mut ascii = String::new();
        let flush_ascii = |ascii: &mut String, terms: &mut std::collections::HashSet<String>| {
            if ascii.len() >= 2 {
                terms.insert(ascii.to_ascii_lowercase());
            }
            ascii.clear();
        };
        let flush_cjk = |run: &mut Vec<char>, terms: &mut std::collections::HashSet<String>| {
            if run.len() == 1 {
                terms.insert(run[0].to_string());
            } else {
                for pair in run.windows(2) {
                    terms.insert(pair.iter().collect());
                }
            }
            run.clear();
        };
        for ch in value.chars().chain(std::iter::once(' ')) {
            if ('\u{3400}'..='\u{9fff}').contains(&ch) {
                flush_ascii(&mut ascii, &mut terms);
                cjk_run.push(ch);
            } else if ch.is_ascii_alphanumeric() {
                flush_cjk(&mut cjk_run, &mut terms);
                ascii.push(ch);
            } else {
                flush_ascii(&mut ascii, &mut terms);
                flush_cjk(&mut cjk_run, &mut terms);
            }
        }
        terms
    }

    let mut terms = collect_terms(value);
    for generic in collect_terms(deck_title) {
        terms.remove(&generic);
    }
    for generic in [
        "页面", "内容", "核心", "主题", "分析", "观点", "情况", "相关", "主要", "展示", "page",
        "slide", "content", "theme",
    ] {
        terms.remove(generic);
    }
    terms
}

fn stable_semantic_similarity(anchor: &str, candidate: &str, deck_title: &str) -> f32 {
    let anchor_terms = stable_semantic_terms(anchor, deck_title);
    let candidate_terms = stable_semantic_terms(candidate, deck_title);
    if anchor_terms.is_empty() || candidate_terms.is_empty() {
        return 0.0;
    }
    let overlap = anchor_terms.intersection(&candidate_terms).count() as f32;
    overlap / anchor_terms.len().min(candidate_terms.len()) as f32
}

fn stable_block_semantic_text(block: &ContentBlock) -> String {
    format!("{} {} {}", block.label, block.text, block.detail)
        .trim()
        .to_string()
}

fn stable_block_message(block: &ContentBlock) -> String {
    let value = if block.text.trim().is_empty() {
        stable_block_semantic_text(block)
    } else if block.detail.trim().is_empty() {
        block.text.trim().to_string()
    } else {
        format!("{}{}", block.text.trim(), block.detail.trim())
    };
    value.chars().take(160).collect()
}

fn stable_short_label(value: &str, limit: usize) -> String {
    value
        .split(['（', '(', '：', ':'])
        .next()
        .unwrap_or(value)
        .trim()
        .chars()
        .take(limit)
        .collect()
}

fn stable_semantic_block_pool(plan: &SlidePlan) -> Vec<ContentBlock> {
    let mut pool = Vec::new();
    for slide in &plan.slides {
        pool.extend(slide.content_blocks.iter().cloned());
        let mut visible_units = slide.bullets.clone();
        visible_units.extend(slide.evidence.iter().cloned());
        pool.extend(content_blocks_from_units(&visible_units, &slide.title));
    }
    let mut seen = std::collections::HashSet::new();
    pool.retain(|block| {
        let key = stable_block_semantic_text(block).to_ascii_lowercase();
        !key.is_empty() && seen.insert(key)
    });
    pool
}

fn prefer_complete_stable_blocks(blocks: &mut [ContentBlock], pool: &[ContentBlock]) {
    for block in blocks {
        let topic = normalize_topic_key(&block.label);
        if topic.is_empty() {
            continue;
        }
        let current_length = stable_block_semantic_text(block).chars().count();
        if let Some(candidate) = pool
            .iter()
            .filter(|candidate| normalize_topic_key(&candidate.label) == topic)
            .max_by_key(|candidate| stable_block_semantic_text(candidate).chars().count())
        {
            if stable_block_semantic_text(candidate).chars().count() > current_length + 8 {
                *block = candidate.clone();
            }
        }
    }
}

fn repair_stable_semantic_consistency(plan: &mut SlidePlan) {
    let deck_title = plan.title.clone();
    let pool = stable_semantic_block_pool(plan);

    for slide in &mut plan.slides {
        let signals = [
            slide.relation.trim().to_ascii_lowercase(),
            slide.chart_type.trim().to_ascii_lowercase(),
            slide.layout.trim().to_ascii_lowercase(),
        ];
        let structured_semantic = signals.iter().any(|value| {
            [
                StableLayoutKind::Timeline,
                StableLayoutKind::Process,
                StableLayoutKind::Comparison,
            ]
            .iter()
            .any(|layout| stable_layout_aliases(*layout).contains(&value.as_str()))
        });
        let anchor = format!("{} {}", slide.title, slide.page_theme);
        let anchor_terms = stable_semantic_terms(&anchor, &deck_title);
        if !structured_semantic && !anchor_terms.is_empty() && slide.page > 1 {
            let mut used = std::collections::HashSet::new();
            let mut used_topics = std::collections::HashSet::new();
            for block in &mut slide.content_blocks {
                let current = stable_block_semantic_text(block);
                let current_score = stable_semantic_similarity(&anchor, &current, &deck_title);
                if let Some((replacement, score)) = pool
                    .iter()
                    .map(|candidate| {
                        let display = stable_block_semantic_text(candidate);
                        let score = stable_semantic_similarity(&anchor, &display, &deck_title);
                        (candidate, display, score)
                    })
                    .filter(|(_, display, score)| {
                        *score >= 0.16 && !used.contains(&display.to_ascii_lowercase())
                    })
                    .filter(|(candidate, _, _)| {
                        let topic = normalize_topic_key(&candidate.label);
                        topic.is_empty() || !used_topics.contains(&topic)
                    })
                    .max_by(|left, right| {
                        left.2
                            .total_cmp(&right.2)
                            .then_with(|| left.1.chars().count().cmp(&right.1.chars().count()))
                    })
                    .map(|(candidate, _, score)| (candidate.clone(), score))
                {
                    let replacement_text = stable_block_semantic_text(&replacement);
                    let richer_same_subject = replacement_text.chars().count()
                        > current.chars().count() + 18
                        && score + 0.02 >= current_score
                        && stable_semantic_similarity(&current, &replacement_text, &deck_title)
                            >= 0.45;
                    if score > current_score + 0.06 || richer_same_subject {
                        *block = replacement;
                    }
                }
                used.insert(stable_block_semantic_text(block).to_ascii_lowercase());
                let topic = normalize_topic_key(&block.label);
                if !topic.is_empty() {
                    used_topics.insert(topic);
                }
            }

            let core = stable_core_message(slide);
            if stable_semantic_similarity(&anchor, &core, &deck_title) < 0.15 {
                if let Some(best) = slide.content_blocks.iter().max_by(|left, right| {
                    stable_semantic_similarity(
                        &anchor,
                        &stable_block_semantic_text(left),
                        &deck_title,
                    )
                    .total_cmp(&stable_semantic_similarity(
                        &anchor,
                        &stable_block_semantic_text(right),
                        &deck_title,
                    ))
                }) {
                    let replacement = stable_block_message(best);
                    if stable_semantic_similarity(&anchor, &replacement, &deck_title) >= 0.18 {
                        slide.core_message = replacement.clone();
                        slide.main_claim = replacement.clone();
                        slide.subtitle = replacement;
                    }
                }
            } else if stable_semantic_similarity(&anchor, &slide.subtitle, &deck_title) < 0.12 {
                slide.subtitle = core;
            }
        }

        prefer_complete_stable_blocks(&mut slide.content_blocks, &pool);
        let comparison = signals.iter().any(|value| {
            stable_layout_aliases(StableLayoutKind::Comparison).contains(&value.as_str())
        });
        if comparison && slide.content_blocks.len() >= 2 {
            let mut labels = std::collections::HashSet::new();
            slide.content_blocks.retain(|block| {
                let key = normalize_topic_key(&block.label);
                !key.is_empty() && labels.insert(key)
            });
            slide.content_blocks.truncate(2);
        }
        if comparison && slide.content_blocks.len() >= 2 {
            let left = slide.content_blocks[0].label.trim();
            let right = slide.content_blocks[1].label.trim();
            if !left.is_empty() && !right.is_empty() && left != right {
                let left_short = stable_short_label(left, 16);
                let right_short = stable_short_label(right, 16);
                slide.title = format!("{} vs {}", left_short, right_short);
                slide.page_theme = slide.title.clone();
                slide.core_message = format!("对照{}与{}的关键差异", left_short, right_short);
                slide.main_claim = slide.core_message.clone();
                slide.subtitle = slide.core_message.clone();
            }
        }
        let category = signals.iter().any(|value| {
            stable_layout_aliases(StableLayoutKind::CategoryGrid).contains(&value.as_str())
        });
        if category && !comparison && slide.content_blocks.len() >= 2 {
            let mut labels = std::collections::HashSet::new();
            slide.content_blocks.retain(|block| {
                let key = normalize_topic_key(&block.label);
                !key.is_empty() && labels.insert(key)
            });
        }
        if category && !comparison && slide.content_blocks.len() >= 2 {
            let current_anchor = format!("{} {}", slide.title, slide.page_theme);
            let matching = slide
                .content_blocks
                .iter()
                .filter(|block| {
                    stable_semantic_similarity(
                        &current_anchor,
                        &content_block_display(block),
                        &deck_title,
                    ) >= 0.08
                })
                .count();
            let core_matches = stable_semantic_similarity(
                &current_anchor,
                &stable_core_message(slide),
                &deck_title,
            ) >= 0.18;
            if matching < 2 {
                let first = stable_short_label(&slide.content_blocks[0].label, 14);
                let second = stable_short_label(&slide.content_blocks[1].label, 14);
                slide.title = format!("{}：多维观察", deck_title);
                slide.page_theme = slide.title.clone();
                slide.core_message = if core_matches {
                    format!(
                        "{}；并从{}与{}等维度展开",
                        stable_core_message(slide),
                        first,
                        second
                    )
                } else {
                    format!("从{}与{}等维度展开", first, second)
                };
                slide.main_claim = slide.core_message.clone();
                slide.subtitle = slide.core_message.clone();
            }
        }

        let timeline = signals.iter().any(|value| {
            stable_layout_aliases(StableLayoutKind::Timeline).contains(&value.as_str())
        });
        if timeline {
            let with_year = slide
                .content_blocks
                .iter()
                .filter(|block| extract_year_token(block).is_some())
                .count();
            if with_year >= 2 {
                slide
                    .content_blocks
                    .retain(|block| extract_year_token(block).is_some());
                slide
                    .content_blocks
                    .sort_by_key(|block| extract_year_token(block));
                if let (Some(first), Some(last)) =
                    (slide.content_blocks.first(), slide.content_blocks.last())
                {
                    let first_label = extract_year_token(first)
                        .unwrap_or_else(|| stable_short_label(&first.label, 20));
                    let last_label = extract_year_token(last)
                        .unwrap_or_else(|| stable_short_label(&last.label, 20));
                    slide.core_message =
                        format!("从{}到{}，关键事件沿时间推进", first_label, last_label);
                    slide.main_claim = slide.core_message.clone();
                    slide.subtitle = slide.core_message.clone();
                }
            }
        }
        let process = signals.iter().any(|value| {
            stable_layout_aliases(StableLayoutKind::Process).contains(&value.as_str())
        });
        if process {
            let mut labels = std::collections::HashSet::new();
            slide.content_blocks.retain(|block| {
                let key = normalize_topic_key(&block.label);
                !key.is_empty() && labels.insert(key)
            });
            if slide.content_blocks.len() < 3 {
                let process_anchor = format!("{} {}", slide.title, slide.page_theme);
                let mut candidates: Vec<(ContentBlock, f32, usize)> = pool
                    .iter()
                    .filter_map(|candidate| {
                        let label_key = normalize_topic_key(&candidate.label);
                        if label_key.is_empty() || labels.contains(&label_key) {
                            return None;
                        }
                        let semantic_text = stable_block_semantic_text(candidate);
                        let score = stable_semantic_similarity(
                            &process_anchor,
                            &semantic_text,
                            &deck_title,
                        );
                        (score >= 0.08).then_some((
                            candidate.clone(),
                            score,
                            semantic_text.chars().count(),
                        ))
                    })
                    .collect();
                candidates.sort_by(|left, right| {
                    right
                        .1
                        .total_cmp(&left.1)
                        .then_with(|| right.2.cmp(&left.2))
                });
                for (candidate, _, _) in candidates {
                    let label_key = normalize_topic_key(&candidate.label);
                    if labels.insert(label_key) {
                        slide.content_blocks.push(candidate);
                    }
                    if slide.content_blocks.len() >= 3 {
                        break;
                    }
                }
            }
            let with_year = slide
                .content_blocks
                .iter()
                .filter(|block| extract_year_token(block).is_some())
                .count();
            if with_year >= 2 {
                slide
                    .content_blocks
                    .sort_by_key(|block| extract_year_token(block));
                if let (Some(first), Some(last)) =
                    (slide.content_blocks.first(), slide.content_blocks.last())
                {
                    let first_short = stable_short_label(&first.label, 12);
                    let last_short = stable_short_label(&last.label, 12);
                    slide.title = format!("{}到{}的阶段演进", first_short, last_short);
                    slide.page_theme = slide.title.clone();
                    slide.core_message =
                        format!("从{}到{}，阶段关系依次展开", first_short, last_short);
                    slide.main_claim = slide.core_message.clone();
                    slide.subtitle = slide.core_message.clone();
                }
            }
        }
        dedup_content_blocks(&mut slide.content_blocks);
    }
}

fn repair_stable_summary_page(plan: &mut SlidePlan) {
    if plan.slides.len() < 2 {
        return;
    }
    let deck_title = plan.title.clone();
    let mut representatives = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for slide in plan
        .slides
        .iter()
        .skip(1)
        .take(plan.slides.len().saturating_sub(2))
    {
        if let Some(block) = slide.content_blocks.iter().find(|block| {
            let display = content_block_display(block);
            !display.trim().is_empty()
                && !is_internal_or_placeholder_unit(&display)
                && seen.insert(normalize_topic_key(&display))
        }) {
            representatives.push(block.clone());
        }
        if representatives.len() == 4 {
            break;
        }
    }
    if representatives.len() < 2 {
        return;
    }
    let summary = plan
        .slides
        .last_mut()
        .expect("plan has at least two slides");
    summary.title = format!("{}：关键脉络与核心结论", deck_title);
    summary.page_theme = "总结与回望".to_string();
    summary.core_message = format!("沿关键阶段与主要事件回看{}", deck_title);
    summary.main_claim = summary.core_message.clone();
    summary.subtitle = summary.core_message.clone();
    summary.content_scope = summary.core_message.clone();
    summary.content_blocks = representatives;
    summary.bullets = summary
        .content_blocks
        .iter()
        .map(content_block_display)
        .take(4)
        .collect();
    summary.evidence = summary.bullets.clone();
    summary.relation = "summary".to_string();
    summary.chart_type = "summary".to_string();
    summary.layout = "summary".to_string();
}

fn assign_stable_page_rhythm(plan: &mut SlidePlan) {
    let total = plan.slides.len();
    for (index, slide) in plan.slides.iter_mut().enumerate() {
        let explicit = slide.page_rhythm.trim().to_ascii_lowercase();
        let rhythm = if index == 0 || index + 1 == total {
            "anchor"
        } else if matches!(
            explicit.as_str(),
            "anchor" | "breathing" | "balanced" | "dense"
        ) {
            explicit.as_str()
        } else {
            let semantic = format!(
                "{} {} {}",
                slide.relation.to_ascii_lowercase(),
                slide.chart_type.to_ascii_lowercase(),
                slide.layout.to_ascii_lowercase()
            );
            if semantic.contains("highlight")
                || semantic.contains("quote")
                || slide.content_blocks.len() <= 2
            {
                "breathing"
            } else if semantic.contains("timeline")
                || semantic.contains("process")
                || semantic.contains("compare")
                || semantic.contains("matrix")
                || semantic.contains("cause")
                || semantic.contains("hierarchy")
            {
                "dense"
            } else {
                "balanced"
            }
        };
        slide.page_rhythm = rhythm.to_string();
        slide.density = rhythm.to_string();
    }
}

fn prepare_stable_plan_for_render(plan: &mut SlidePlan, visible_material: &str) {
    plan.title = sanitize_visible_text(&plan.title);
    plan.subtitle = sanitize_visible_text(&plan.subtitle);
    plan.audience = sanitize_visible_text(&plan.audience);
    let pool = stable_plan_visible_pool(plan, visible_material);
    let total = plan.slides.len();

    for (index, slide) in plan.slides.iter_mut().enumerate() {
        slide.title = sanitize_visible_text(&slide.title);
        slide.subtitle = sanitize_visible_text(&slide.subtitle);
        slide.page_theme = sanitize_visible_text(&slide.page_theme);
        slide.main_claim = sanitize_visible_text(&slide.main_claim);
        slide.core_message = sanitize_visible_text(&slide.core_message);
        slide.content_scope = sanitize_visible_text(&slide.content_scope);
        slide.bullets = sanitize_visible_list(&slide.bullets, 6);
        slide.evidence = sanitize_visible_list(&slide.evidence, 6);
        slide.content_blocks = slide
            .content_blocks
            .iter()
            .enumerate()
            .filter_map(|(block_index, block)| sanitize_visible_block(block, block_index))
            .collect();
        if index > 0 {
            let local_blocks = content_blocks_from_slide(slide);
            if !local_blocks.is_empty() {
                let mut merged = std::mem::take(&mut slide.content_blocks);
                merged.extend(local_blocks);
                slide.content_blocks = merged;
            }
        }
        dedup_content_blocks(&mut slide.content_blocks);
        if index == 0 && !slide.core_message.trim().is_empty() {
            if let Some(primary) = slide.content_blocks.first_mut() {
                primary.text = slide.core_message.clone();
                primary.detail.clear();
            }
        }
        if !slide.core_message.trim().is_empty() {
            let title_key = normalize_topic_key(&slide.title);
            for block in &mut slide.content_blocks {
                if !title_key.is_empty() && normalize_topic_key(&block.label) == title_key {
                    block.text = slide.core_message.clone();
                    block.detail.clear();
                }
            }
        }

        let selected = select_units_for_slide(&pool, index, total);
        if slide.title.is_empty() {
            slide.title = if index == 0 {
                plan.title.clone()
            } else {
                selected
                    .first()
                    .map(|unit| title_from_material_unit(unit, index))
                    .unwrap_or_else(|| plan.title.clone())
            };
        }
        if slide.page_theme.is_empty() {
            slide.page_theme = slide.title.clone();
        }
        if slide.content_blocks.len() < if index == 0 { 1 } else { 2 } {
            slide
                .content_blocks
                .extend(content_blocks_from_units(&selected, &slide.title));
            dedup_content_blocks(&mut slide.content_blocks);
        }
        if slide.content_blocks.is_empty() {
            slide.content_blocks.push(ContentBlock {
                label: slide.title.clone(),
                text: selected
                    .first()
                    .cloned()
                    .unwrap_or_else(|| plan.title.clone()),
                detail: String::new(),
            });
        }
        let fallback_core = slide
            .content_blocks
            .first()
            .map(|block| {
                if block.text.trim().is_empty() {
                    content_block_display(block)
                } else {
                    block.text.clone()
                }
            })
            .unwrap_or_else(|| slide.title.clone());
        if slide.core_message.is_empty() {
            slide.core_message = fallback_core.clone();
        }
        if slide.main_claim.is_empty() {
            slide.main_claim = slide.core_message.clone();
        }
        if slide.subtitle.is_empty() {
            slide.subtitle = slide.core_message.clone();
        }
        if slide.content_scope.is_empty() {
            slide.content_scope = slide.core_message.clone();
        }
        if slide.bullets.len() < 2 {
            slide.bullets = slide
                .content_blocks
                .iter()
                .map(content_block_display)
                .filter(|value| !value.trim().is_empty())
                .take(5)
                .collect();
        }
        if slide.evidence.is_empty() {
            slide.evidence = slide.bullets.clone();
        }
        slide.speaker_note = sanitize_visible_text(&slide.speaker_note);
        if slide.speaker_note.is_empty() {
            slide.speaker_note = slide.core_message.clone();
        }
    }
    repair_stable_semantic_consistency(plan);
    repair_stable_summary_page(plan);
    assign_stable_page_rhythm(plan);
    refresh_theme_allocation_and_must_avoid(plan);
}

fn contains_placeholder_phrase(text: &str) -> bool {
    [
        "提炼用户材料中的关键信息",
        "围绕当前主题组织重点表达",
        "使用短句和结构化表达",
        "概括本主题的核心内容",
        "主题与一句话价值主张",
        "呈现主题的阶段、结构或层次",
        "鎻愮偧鐢ㄦ埛鏉愭枡涓殑鍏抽敭淇℃伅",
        "鍥寸粫褰撳墠涓婚缁勭粐閲嶇偣琛ㄨ揪",
        "浣跨敤鐭彞鍜岀粨鏋勫寲琛ㄨ揪",
        "姒傛嫭鏈富棰樼殑鏍稿績鍐呭",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
}

fn default_slide_plan(title: &str, slide_count: usize, style: &str, prompt: &str) -> SlidePlan {
    let count = slide_count.clamp(1, 30);
    let material_units = extract_material_units(prompt);
    let templates = [
        ("cover", "cover"),
        ("content", "cards"),
        ("content", "timeline"),
        ("content", "compare"),
        ("content", "process"),
        ("content", "highlight"),
        ("content", "matrix"),
        ("content", "summary"),
    ];
    let mut slides = Vec::with_capacity(count);
    for idx in 0..count {
        let (slide_type, layout) = templates.get(idx).copied().unwrap_or(("content", "cards"));
        let layout = normalize_layout(layout, idx, count);
        let visual_hint = visual_hint_for_layout(&layout).to_string();
        let units = select_units_for_slide(&material_units, idx, count);
        let first_unit = units.first().cloned().unwrap_or_else(|| title.to_string());
        let fallback_title = if idx == 0 {
            title.to_string()
        } else {
            title_from_material_unit(&first_unit, idx)
        };
        let subtitle = if idx == 0 {
            first_unit.clone()
        } else {
            units.get(1).cloned().unwrap_or_else(|| first_unit.clone())
        };
        let page_title = if idx == 0 {
            title.to_string()
        } else {
            fallback_title.clone()
        };
        let page_theme = if idx == 0 {
            format!("{}：总体认识", title)
        } else {
            fallback_title.clone()
        };
        let bullets = if idx == 0 {
            Vec::new()
        } else {
            let mut bullets = units;
            if bullets.len() < 2 {
                bullets.extend(fallback_bullets_from_text(&subtitle));
            }
            bullets.truncate(4);
            bullets
        };
        let evidence = if idx == 0 {
            select_units_for_slide(&material_units, idx, count)
        } else {
            bullets.clone()
        };
        let content_blocks = content_blocks_from_units(&evidence, &fallback_title);
        slides.push(Slide {
            page: idx + 1,
            page_index: idx + 1,
            page_id: format!("P{:02}", idx + 1),
            slide_type: slide_type.to_string(),
            layout: layout.clone(),
            title: page_title,
            subtitle: subtitle.to_string(),
            bullets,
            visual_hint,
            page_theme,
            main_claim: subtitle.to_string(),
            core_message: subtitle.to_string(),
            content_scope: if idx == 0 {
                title.to_string()
            } else {
                first_unit.clone()
            },
            content_blocks,
            evidence,
            relation: relation_for_layout(&layout).to_string(),
            density: density_for_layout(&layout, idx, count).to_string(),
            visual_intent: visual_hint_for_layout(&layout).to_string(),
            must_include: Vec::new(),
            must_avoid: vec![
                "Do not repeat other pages as the main topic".to_string(),
                "Do not re-plan the whole deck on this page".to_string(),
            ],
            page_rhythm: String::new(),
            chart_ref: String::new(),
            chart_type: chart_type_for_layout(&layout).to_string(),
            file_stem: String::new(),
            speaker_note: if prompt.trim().is_empty() {
                subtitle.to_string()
            } else {
                format!("讲解本页材料重点：{}", subtitle)
            },
        });
    }
    SlidePlan {
        title: title.to_string(),
        subtitle: "基于确认需求自动生成的演示文稿".to_string(),
        audience: "目标听众".to_string(),
        style: style.to_string(),
        theme: theme_for_style(style),
        theme_allocation: Vec::new(),
        slides,
    }
}

fn create_project_dir(root: &Path) -> Result<PathBuf, AppError> {
    let projects = root.join("projects");
    create_dir_all(&projects)?;
    let base = format!("pome_ppt_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    for index in 0..100 {
        let name = if index == 0 {
            base.clone()
        } else {
            format!("{}_{}", base, index)
        };
        let path = projects.join(name);
        if !path.exists() {
            create_dir_all(&path)?;
            return Ok(path);
        }
    }
    Err(AppError::Custom(
        "无法创建唯一的 ppt-master 项目目录".into(),
    ))
}

fn copy_final_pptx(source: &Path, output_dir: &str, title: &str) -> Result<PathBuf, AppError> {
    let trimmed = output_dir.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("导出文件夹不能为空".into()));
    }
    let dir = PathBuf::from(trimmed);
    if !dir.exists() {
        return Err(AppError::NotFound(format!(
            "导出文件夹不存在: {}",
            dir.display()
        )));
    }
    if !dir.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "导出路径不是文件夹: {}",
            dir.display()
        )));
    }
    let filename = format!(
        "{}_{}.pptx",
        safe_filename(title, "AI_PPT"),
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let target = dir.join(filename);
    fs::copy(source, &target).map_err(|e| {
        AppError::Custom(format!(
            "复制 PPTX 失败: {} -> {} ({})",
            source.display(),
            target.display(),
            e
        ))
    })?;
    Ok(target)
}

fn build_ppt_master_design_spec(
    plan: &SlidePlan,
    prompt: &str,
    mapping: &PptMasterStyleMapping,
    theme_spec: &NativeThemeSpec,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} - Design Spec\n\n", plan.title));
    out.push_str("> Generated by Pomegranate as a ppt-master-compatible planning artifact. Pomegranate owns user interaction and slide_plan; ppt-master owns design resources, SVG constraints, and PPTX export.\n\n");
    out.push_str("## Deck-wide NativeThemeSpec\n\n```json\n");
    out.push_str(&theme_spec.prompt_contract());
    out.push_str("\n```\n\nThis contract is visual guidance only. Never render its field names or rule text as visible slide content.\n\n");
    out.push_str("## I. Project Information\n\n");
    out.push_str("| Item | Value |\n| ---- | ----- |\n");
    out.push_str(&format!("| **Project Name** | {} |\n", plan.title));
    out.push_str("| **Canvas Format** | PPT 16:9 (1280x720) |\n");
    out.push_str(&format!("| **Page Count** | {} |\n", plan.slides.len()));
    out.push_str(&format!("| **Design Style** | {} |\n", mapping.user_style));
    out.push_str(&format!("| **Target Audience** | {} |\n", plan.audience));
    out.push_str("| **Use Case** | AI-generated presentation from confirmed user prompt |\n");
    out.push_str("| **Delivery Purpose** | balanced |\n");
    out.push_str("| **Content Strategy** | Restructure user material into a concise, presentation-ready outline while preserving supplied facts. |\n\n");
    out.push_str("## II. Canvas Specification\n\n");
    out.push_str("| Property | Value |\n| -------- | ----- |\n");
    out.push_str("| **Format** | PPT 16:9 |\n");
    out.push_str("| **Dimensions** | 1280x720 |\n");
    out.push_str("| **viewBox** | `0 0 1280 720` |\n");
    out.push_str("| **Margins** | left/right 56px, top/bottom 44px |\n");
    out.push_str("| **Content Area** | x=56, y=44, width=1168, height=632 |\n\n");
    out.push_str("## III. Visual Theme\n\n");
    out.push_str("### Theme Style\n\n");
    out.push_str(&format!("- **Mode**: {}\n", mapping.mode));
    out.push_str(&format!("- **Visual style**: {}\n", mapping.visual_style));
    out.push_str("- **Theme**: Derived from selected ppt-master visual style and Pomegranate confirmed prompt\n");
    out.push_str(&format!("- **Tone**: {}\n\n", plan.style));
    out.push_str("### Template Provenance\n\n");
    if mapping.template_provenance.is_empty() {
        out.push_str(
            "- No layout template copied; free design under locked mode and visual_style.\n\n",
        );
    } else {
        for item in &mapping.template_provenance {
            out.push_str(&format!("- {}\n", item));
        }
        out.push('\n');
    }
    out.push_str("### Color Scheme\n\n");
    out.push_str("| Role | HEX | Purpose |\n| ---- | --- | ------- |\n");
    out.push_str(&format!(
        "| **Background** | `{}` | Page background |\n",
        theme_spec.background_color
    ));
    out.push_str(&format!(
        "| **Secondary bg** | `{}` | Secondary page background |\n",
        theme_spec.secondary_background_color
    ));
    out.push_str(&format!(
        "| **Primary** | `{}` | Primary emphasis |\n",
        theme_spec.primary_color
    ));
    out.push_str(&format!(
        "| **Accent** | `{}` | Data highlights and key information |\n",
        theme_spec.accent_color
    ));
    out.push_str(&format!(
        "| **Secondary accent** | `{}` | Secondary emphasis |\n",
        theme_spec.secondary_color
    ));
    out.push_str(&format!(
        "| **Body text** | `{}` | Main text |\n",
        theme_spec.text_primary
    ));
    out.push_str(&format!(
        "| **Secondary text** | `{}` | Captions and notes |\n",
        theme_spec.text_secondary
    ));
    out.push_str(&format!(
        "| **Border/divider** | `{}` | Lines and separators |\n\n",
        theme_spec.border_color
    ));
    out.push_str("### Image Strategy\n\n");
    out.push_str("- **Image Usage**: none in P0 unless the user-provided prompt explicitly names image assets.\n");
    out.push_str("- **Image Rendering**: inferred from visual_style by ppt-master references when images are added.\n");
    out.push_str(
        "- **Image Palette**: use locked color scheme; do not invent new HEX values in SVG.\n\n",
    );
    out.push_str("## IV. Typography System\n\n");
    out.push_str(
        "- Typography direction: PPT-safe CJK sans, clean business presentation baseline.\n",
    );
    out.push_str("- Title: `Microsoft YaHei, Arial, sans-serif`\n");
    out.push_str("- Body: `Microsoft YaHei, Arial, sans-serif`\n");
    out.push_str("- Emphasis: `Georgia, SimSun, serif`\n");
    out.push_str("- Code: `Consolas, Courier New, monospace`\n");
    out.push_str("- Baseline body size: 24\n\n");
    out.push_str("## V. Layout Principles\n\n");
    out.push_str("- Use ppt-master page rhythm: anchor / dense / breathing.\n");
    out.push_str("- Avoid repeating identical card grids across pages.\n");
    out.push_str("- Prefer chart/template references when content shape matches `page_charts` or copied layout templates.\n\n");
    out.push_str("## V-A. Theme Allocation\n\n");
    out.push_str("| Page | Assigned Theme | Exclusive Scope |\n| ---- | -------------- | --------------- |\n");
    for allocation in &plan.theme_allocation {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            allocation.page_id, allocation.assigned_theme, allocation.exclusive_scope
        ));
    }
    out.push('\n');
    out.push_str("## VI. Page Roster / Layout Planning\n\n");
    out.push_str("| Page | Title | Page Theme | Main Claim | Rhythm | Chart | Layout reference |\n| ---- | ----- | ---------- | ---------- | ------ | ----- | ---------------- |\n");
    for slide in &plan.slides {
        let layout_ref =
            page_layout_reference(slide, mapping).unwrap_or_else(|| "free-design".to_string());
        let chart_ref =
            chart_reference_for_slide(slide, mapping).unwrap_or_else(|| "none".to_string());
        out.push_str(&format!(
            "| P{:02} | {} | {} | {} | {} | {} | {} |\n",
            slide.page,
            slide.title,
            slide.page_theme,
            slide.main_claim,
            page_rhythm_for_slide(slide),
            chart_ref,
            layout_ref
        ));
    }
    out.push_str("\n## VII. Visualization Reference List\n\n");
    out.push_str("| Page | Content Shape | Reference template path | Rationale |\n| ---- | ------------- | ----------------------- | --------- |\n");
    for slide in &plan.slides {
        if let Some(chart) = chart_reference_for_slide(slide, mapping) {
            out.push_str(&format!(
                "| P{:02} | {} | templates/charts/{}.svg | Matched from slide layout `{}` and style chart bias |\n",
                slide.page, slide.visual_hint, chart, slide.layout
            ));
        }
    }
    out.push_str("\n## VIII. Image Resource List\n\n- No image rows in P0 native pipeline unless future source import provides assets.\n\n");
    out.push_str("## IX. Content Outline\n\n");
    for slide in &plan.slides {
        out.push_str(&format!("### P{:02}. {}\n\n", slide.page, slide.title));
        out.push_str(&format!("- Role: {}\n", slide.slide_type));
        out.push_str(&format!("- Subtitle: {}\n", slide.subtitle));
        out.push_str(&format!("- Page theme: {}\n", slide.page_theme));
        out.push_str(&format!("- Main claim: {}\n", slide.main_claim));
        out.push_str(&format!("- Content scope: {}\n", slide.content_scope));
        out.push_str(&format!(
            "- Page rhythm: {}\n",
            page_rhythm_for_slide(slide)
        ));
        out.push_str(&format!(
            "- Chart type: {}\n",
            chart_reference_for_slide(slide, mapping).unwrap_or_else(|| "none".to_string())
        ));
        if !slide.must_include.is_empty() {
            out.push_str("- Must include:\n");
            for item in &slide.must_include {
                out.push_str(&format!("  - {}\n", item));
            }
        }
        if !slide.must_avoid.is_empty() {
            out.push_str("- Must avoid:\n");
            for item in &slide.must_avoid {
                out.push_str(&format!("  - {}\n", item));
            }
        }
        out.push_str(&format!("- Visual focus: {}\n", slide.visual_hint));
        if !slide.bullets.is_empty() {
            out.push_str("- Content:\n");
            for bullet in &slide.bullets {
                out.push_str(&format!("  - {}\n", bullet));
            }
        }
        out.push_str(&format!("- Speaker note: {}\n\n", slide.speaker_note));
    }
    out.push_str("## X. Confirmed Prompt\n\n");
    out.push_str(prompt);
    out.push('\n');
    out
}

fn build_ppt_master_spec_lock(
    plan: &SlidePlan,
    mapping: &PptMasterStyleMapping,
    theme_spec: &NativeThemeSpec,
) -> String {
    let mut out = String::new();
    out.push_str("# Execution Lock\n\n");
    out.push_str("## canvas\n- viewBox: 0 0 1280 720\n- format: PPT 16:9\n\n");
    out.push_str(&format!("## mode\n- mode: {}\n\n", mapping.mode));
    out.push_str(&format!(
        "## visual_style\n- visual_style: {}\n\n",
        mapping.visual_style
    ));
    out.push_str("## template_provenance\n");
    if mapping.template_provenance.is_empty() {
        out.push_str("- source: free-design\n\n");
    } else {
        for item in &mapping.template_provenance {
            out.push_str(&format!("- {}\n", item));
        }
        out.push('\n');
    }
    out.push_str("## native_theme_spec\n```json\n");
    out.push_str(&theme_spec.prompt_contract());
    out.push_str("\n```\n\n");
    out.push_str("## colors\n");
    out.push_str(&format!("- bg: {}\n", theme_spec.background_color));
    out.push_str(&format!(
        "- secondary_bg: {}\n",
        theme_spec.secondary_background_color
    ));
    out.push_str(&format!("- surface: {}\n", theme_spec.surface_color));
    out.push_str(&format!("- panel: {}\n", theme_spec.panel_color));
    out.push_str(&format!("- primary: {}\n", theme_spec.primary_color));
    out.push_str(&format!("- accent: {}\n", theme_spec.accent_color));
    out.push_str(&format!(
        "- secondary_accent: {}\n",
        theme_spec.secondary_color
    ));
    out.push_str(&format!("- text: {}\n", theme_spec.text_primary));
    out.push_str(&format!(
        "- text_secondary: {}\n",
        theme_spec.text_secondary
    ));
    out.push_str(&format!("- muted: {}\n", theme_spec.text_secondary));
    out.push_str(&format!("- border: {}\n", theme_spec.border_color));
    out.push_str(&format!("- grid: {}\n\n", theme_spec.border_color));
    out.push_str("## typography\n");
    out.push_str("- font_family: Microsoft YaHei, Arial, sans-serif\n");
    out.push_str("- title_family: Microsoft YaHei, Arial, sans-serif\n");
    out.push_str("- body_family: Microsoft YaHei, Arial, sans-serif\n");
    out.push_str("- emphasis_family: Georgia, SimSun, serif\n");
    out.push_str("- code_family: Consolas, Courier New, monospace\n");
    out.push_str("- body: 24\n- title: 42\n- subtitle: 30\n- lead: 28\n- subheading: 26\n- annotation: 18\n- footnote: 14\n- cover_title: 76\n- hero_word: 54\n\n");
    out.push_str("## icons\n- library: tabler-outline\n- stroke_width: 2\n- inventory: presentation, chart-bar, route, bulb, timeline, database, target, users, trophy, report-analytics\n\n");
    out.push_str("## images\n- strategy: none\n\n");
    out.push_str("## page_rhythm\n");
    for slide in &plan.slides {
        out.push_str(&format!(
            "- P{:02}: {}\n",
            slide.page,
            page_rhythm_for_slide(slide)
        ));
    }
    out.push('\n');
    let layout_rows: Vec<String> = plan
        .slides
        .iter()
        .filter_map(|slide| {
            page_layout_reference(slide, mapping)
                .map(|layout| format!("- P{:02}: {}", slide.page, layout))
        })
        .collect();
    if !layout_rows.is_empty() {
        out.push_str("## page_layouts\n");
        out.push_str(&layout_rows.join("\n"));
        out.push_str("\n\n");
    }
    let chart_rows: Vec<String> = plan
        .slides
        .iter()
        .filter_map(|slide| {
            chart_reference_for_slide(slide, mapping)
                .map(|chart| format!("- P{:02}: {}", slide.page, chart))
        })
        .collect();
    if !chart_rows.is_empty() {
        out.push_str("## page_charts\n");
        out.push_str(&chart_rows.join("\n"));
        out.push_str("\n\n");
    }
    out.push_str("## theme_allocation\n");
    for allocation in &plan.theme_allocation {
        out.push_str(&format!(
            "- {}: assignedTheme={} | exclusiveScope={}\n",
            allocation.page_id, allocation.assigned_theme, allocation.exclusive_scope
        ));
    }
    out.push('\n');
    out.push_str("## page_content_contract\n");
    for slide in &plan.slides {
        out.push_str(&format!(
            "- P{:02}: theme={} | claim={} | scope={} | avoid={}\n",
            slide.page,
            slide.page_theme,
            slide.main_claim,
            slide.content_scope,
            if slide.must_avoid.is_empty() {
                "none".to_string()
            } else {
                slide.must_avoid.join("; ")
            }
        ));
    }
    out.push('\n');
    out.push_str("## forbidden\n");
    out.push_str("- Mixing icon libraries\n");
    out.push_str("- rgba()\n");
    out.push_str("- <style>, class, <foreignObject>, textPath, @font-face, <animate*>, <script>, <iframe>, <symbol>+<use>\n");
    out.push_str("- <g opacity>\n");
    out.push_str("- HTML named entities in text\n");
    out.push_str(
        "- <use> in any form, including <use href=\"#...\"> and <use xlink:href=\"#...\">\n",
    );
    out.push_str("- <symbol> and visual <defs> + <use> reuse patterns; repeat shapes must be expanded as real rect/path/text/circle/line elements\n");
    out.push_str("- <foreignObject> and HTML inside SVG\n");
    out.push_str("- external href images or network image references\n");
    out.push_str("- <filter>, <mask>, <clipPath>\n");
    out.push_str("- unsupported <pattern>; only ppt-master data-pptx-pattern-compliant patterns may be used\n");
    out.push_str("- fabricated exact numbers, rankings, award counts, fellow counts, laboratory counts, or years\n");
    out.push_str("- definite claims such as 全国唯一, 全国第一, 连续三年, 连续五年, 20+, 6个国家级, 国家科技一等奖 unless explicitly present in raw material\n");
    out
}

fn page_rhythm_for_slide(slide: &Slide) -> String {
    let explicit = slide.page_rhythm.trim();
    if matches!(explicit, "anchor" | "dense" | "breathing" | "balanced") {
        return explicit.to_string();
    }
    match slide.layout.as_str() {
        "cover" | "section" => "anchor",
        "summary" => "balanced",
        "highlight" => "breathing",
        "matrix" | "cards" | "compare" | "timeline" | "process" => "dense",
        _ => "dense",
    }
    .to_string()
}

fn page_layout_reference(slide: &Slide, mapping: &PptMasterStyleMapping) -> Option<String> {
    if mapping.layout_bias.is_empty() {
        return None;
    }
    let basename = match slide.layout.as_str() {
        "cover" => "01_cover",
        "section" => "02_chapter",
        "summary" => "04_ending",
        _ => "03_content",
    };
    Some(basename.to_string())
}

fn chart_reference_for_slide(slide: &Slide, mapping: &PptMasterStyleMapping) -> Option<String> {
    let explicit = if slide.chart_ref.trim().is_empty() {
        slide.chart_type.trim()
    } else {
        slide.chart_ref.trim()
    };
    if !explicit.is_empty() {
        return if explicit == "none" {
            None
        } else {
            Some(explicit.to_string())
        };
    }
    let preferred: &[&str] = match slide.layout.as_str() {
        "timeline" => &["timeline", "roadmap_vertical", "gantt_chart"],
        "process" => &["pipeline_with_stages", "process_flow", "chevron_process"],
        "compare" => &["comparison_columns", "comparison_table", "pros_cons_chart"],
        "matrix" => &[
            "matrix_2x2",
            "quadrant_text_bullets",
            "feature_matrix_table",
        ],
        "highlight" => &["kpi_cards", "gauge_chart", "bullet_chart"],
        "cards" => &["kpi_cards", "icon_grid", "vertical_list"],
        _ => return None,
    };
    for chart in preferred {
        if mapping.chart_bias.iter().any(|item| item == chart) {
            return Some((*chart).to_string());
        }
    }
    mapping.chart_bias.first().cloned()
}

fn build_stable_design_spec(plan: &SlidePlan) -> String {
    let mut out = String::new();
    out.push_str("# Stable Mode Design Spec\n\n");
    out.push_str(&format!("- Title: {}\n", plan.title));
    out.push_str(&format!("- Subtitle: {}\n", plan.subtitle));
    out.push_str(&format!("- Audience: {}\n", plan.audience));
    out.push_str(&format!("- Style: {}\n", plan.style));
    out.push_str("- Canvas: 16:9, 1280x720 SVG (ppt-master ppt169)\n");
    out.push_str("- Generation: Pomegranate stable content-contract SVG rendering\n\n");
    out.push_str("## Page Content Contracts\n\n");
    for slide in &plan.slides {
        out.push_str(&format!("### P{:02} {}\n\n", slide.page, slide.title));
        out.push_str(&format!("- Page theme: {}\n", slide.page_theme));
        out.push_str(&format!("- Core message: {}\n", stable_core_message(slide)));
        out.push_str(&format!("- Content scope: {}\n", slide.content_scope));
        out.push_str(&format!("- Relation: {}\n", slide.relation));
        out.push_str(&format!("- Chart type: {}\n", slide.chart_type));
        out.push_str(&format!("- Density: {}\n", slide.density));
        out.push_str(&format!("- Page rhythm: {}\n", slide.page_rhythm));
        out.push_str("- Content blocks:\n");
        for block in &slide.content_blocks {
            out.push_str(&format!(
                "  - {} | {} | {}\n",
                block.label, block.text, block.detail
            ));
        }
        if !slide.evidence.is_empty() {
            out.push_str("- Evidence:\n");
            for item in &slide.evidence {
                out.push_str(&format!("  - {}\n", item));
            }
        }
        out.push('\n');
    }
    out
}

fn build_design_spec(plan: &SlidePlan) -> String {
    format!(
        "# Design Spec\n\n- 标题：{}\n- 副标题：{}\n- 汇报对象：{}\n- 风格：{}\n- 画布：16:9, 1600x900 SVG\n- 生成方式：Pomegranate 模板式 SVG 生成\n",
        plan.title, plan.subtitle, plan.audience, plan.style
    )
}

fn build_agent_design_spec(plan: &SlidePlan, prompt: &str, skill_text: &str) -> String {
    let mut out = String::new();
    out.push_str("---\nkind: deck\ncanvas: ppt169-custom\n---\n\n");
    out.push_str("# Design Spec\n\n");
    out.push_str("## Template Overview\n");
    out.push_str(&format!("- Title: {}\n", plan.title));
    out.push_str(&format!("- Subtitle: {}\n", plan.subtitle));
    out.push_str(&format!("- Audience: {}\n", plan.audience));
    out.push_str(&format!("- Style: {}\n", plan.style));
    out.push_str("- Canvas: 16:9, SVG viewBox 0 0 1600 900\n");
    out.push_str(
        "- Goal: Generate visually varied, editable PPTX pages through ppt-master SVG export.\n\n",
    );
    out.push_str("## Confirmed Prompt\n\n");
    out.push_str(prompt);
    out.push_str("\n\n## Color Scheme\n");
    out.push_str(&format!(
        "- Primary: {}\n- Secondary: {}\n- Accent: {}\n- Background: {}\n\n",
        plan.theme.primary, plan.theme.secondary, plan.theme.accent, plan.theme.background
    ));
    out.push_str("## Typography\n");
    out.push_str("- Font family: Microsoft YaHei, PingFang SC, SimSun, Arial, sans-serif\n");
    out.push_str(
        "- Hierarchy: cover title 64-84, page title 40-52, subtitle 22-28, body 18-26.\n\n",
    );
    out.push_str("## Page Structure\n");
    for slide in &plan.slides {
        out.push_str(&format!(
            "{}. {} — layout: {}; visual focus: {}; subtitle: {}\n",
            slide.page, slide.title, slide.layout, slide.visual_hint, slide.subtitle
        ));
    }
    out.push_str("\n## Executor Rules From ppt-master\n");
    out.push_str(&skill_excerpt(skill_text));
    out.push('\n');
    out
}

fn build_agent_spec_lock(plan: &SlidePlan) -> String {
    let palette = palette_for_style(&plan.style);
    let mut out = String::new();
    out.push_str("# Spec Lock\n\n");
    out.push_str("canvas:\n");
    out.push_str("  width: 1600\n  height: 900\n  viewBox: 0 0 1600 900\n\n");
    out.push_str("colors:\n");
    out.push_str(&format!(
        "  primary: {}\n  secondary: {}\n  accent: {}\n  highlight: {}\n  background: {}\n  surface: {}\n  text: {}\n  muted: {}\n  line: {}\n\n",
        palette.accent, palette.accent2, palette.highlight, plan.theme.accent, palette.bg1, palette.surface, palette.text, palette.muted, palette.line
    ));
    out.push_str("typography:\n");
    out.push_str("  font_family: Microsoft YaHei, PingFang SC, SimSun, Arial, sans-serif\n");
    out.push_str("  title: 44\n  subtitle: 24\n  body: 22\n\n");
    out.push_str("page_layouts:\n");
    for slide in &plan.slides {
        out.push_str(&format!("  {:02}: {}\n", slide.page, slide.layout));
    }
    out.push_str("\npage_rhythm:\n");
    for slide in &plan.slides {
        let rhythm = match slide.layout.as_str() {
            "cover" | "section" | "highlight" | "summary" => "breathing",
            "matrix" | "cards" => "dense",
            _ => "anchor",
        };
        out.push_str(&format!("  {:02}: {}\n", slide.page, rhythm));
    }
    out
}

fn ensure_layout_variety(plan: &mut SlidePlan) {
    let total = plan.slides.len();
    for (idx, slide) in plan.slides.iter_mut().enumerate() {
        slide.layout = normalize_layout(&slide.layout, idx, total);
    }
    if total < 4 {
        return;
    }
    let unique = plan
        .slides
        .iter()
        .map(|slide| slide.layout.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();
    if unique >= 4 {
        return;
    }
    let layouts = [
        "cover",
        "cards",
        "timeline",
        "compare",
        "process",
        "matrix",
        "highlight",
        "image_text",
        "summary",
    ];
    for (idx, slide) in plan.slides.iter_mut().enumerate() {
        slide.layout = normalize_layout(layouts[idx % layouts.len()], idx, total);
        slide.visual_hint = visual_hint_for_layout(&slide.layout).to_string();
    }
}

fn enrich_slide_execution_plan(
    plan: &mut SlidePlan,
    mapping: &PptMasterStyleMapping,
    chart_catalog: &ChartCatalog,
) {
    let total = plan.slides.len();
    for idx in 0..total {
        let slide = &mut plan.slides[idx];
        if slide.page_rhythm.trim().is_empty() {
            slide.page_rhythm = planned_page_rhythm(idx, total, slide).to_string();
        }
        if slide.file_stem.trim().is_empty() {
            slide.file_stem = planned_file_stem(idx, total, slide);
        } else {
            slide.file_stem = ascii_stem(&slide.file_stem, &planned_file_stem(idx, total, slide));
        }
        if slide.chart_ref.trim().is_empty() {
            slide.chart_ref = match_chart_for_slide(slide, mapping, chart_catalog)
                .unwrap_or_else(|| "none".to_string());
        }
        if slide.chart_type.trim().is_empty() {
            slide.chart_type = slide.chart_ref.clone();
        } else if slide.chart_ref.trim().is_empty() || slide.chart_ref == "none" {
            slide.chart_ref = slide.chart_type.clone();
        }
    }
    refresh_theme_allocation_and_must_avoid(plan);
}

fn refresh_theme_allocation_and_must_avoid(plan: &mut SlidePlan) {
    let mut seen = std::collections::HashSet::new();
    let mut allocations = Vec::with_capacity(plan.slides.len());
    for slide in &mut plan.slides {
        if slide.page_index == 0 {
            slide.page_index = slide.page;
        }
        if slide.page_id.trim().is_empty() {
            slide.page_id = format!("P{:02}", slide.page);
        }
        let mut assigned_theme = if slide.page_theme.trim().is_empty() {
            slide.title.trim().to_string()
        } else {
            slide.page_theme.trim().to_string()
        };
        if assigned_theme.is_empty() {
            assigned_theme = format!("Page {}", slide.page);
        }
        let key = normalize_topic_key(&assigned_theme);
        if !key.is_empty() && seen.contains(&key) {
            assigned_theme = format!("{} - {}", assigned_theme, slide.page_id);
        }
        seen.insert(normalize_topic_key(&assigned_theme));
        slide.page_theme = assigned_theme.clone();

        let exclusive_scope = if slide.content_scope.trim().is_empty() {
            slide.subtitle.trim().to_string()
        } else {
            slide.content_scope.trim().to_string()
        };
        let exclusive_scope = if exclusive_scope.is_empty() {
            format!("Only cover {}", assigned_theme)
        } else {
            exclusive_scope
        };
        slide.content_scope = exclusive_scope.clone();
        allocations.push(ThemeAllocation {
            page_id: slide.page_id.clone(),
            assigned_theme,
            exclusive_scope,
        });
    }

    plan.theme_allocation = allocations;
    let allocations = plan.theme_allocation.clone();
    for slide in &mut plan.slides {
        let mut avoid = Vec::new();
        for allocation in &allocations {
            if allocation.page_id == slide.page_id {
                continue;
            }
            avoid.push(format!(
                "Do not cover {}; owned by {}",
                allocation.assigned_theme, allocation.page_id
            ));
        }
        avoid.push("Do not use unsupported exact numbers, rankings, award counts, fellow counts, laboratory counts, or years".to_string());
        slide.must_avoid = avoid;
    }
}

fn planned_page_rhythm(idx: usize, total: usize, slide: &Slide) -> &'static str {
    if idx == 0 || idx + 1 == total {
        return "anchor";
    }
    let text = format!(
        "{} {} {} {}",
        slide.layout,
        slide.title,
        slide.subtitle,
        slide.bullets.join(" ")
    );
    if slide.layout == "highlight"
        || contains_any(
            &text,
            &["人物", "人才", "价值", "观点", "气质", "转场", "理念"],
        )
    {
        return "breathing";
    }
    if total >= 8 && matches!(idx, 3 | 6 | 8) {
        return "breathing";
    }
    "dense"
}

fn planned_file_stem(idx: usize, total: usize, slide: &Slide) -> String {
    if idx == 0 {
        return "cover".to_string();
    }
    if idx + 1 == total {
        return "summary".to_string();
    }
    let text = format!(
        "{} {} {} {}",
        slide.layout,
        slide.title,
        slide.subtitle,
        slide.bullets.join(" ")
    );
    let stem = if contains_any(&text, &["使命", "历史", "起源", "发展", "时间", "血脉"])
    {
        "mission"
    } else if contains_any(&text, &["实力", "能力", "优势", "学科", "支柱", "硬核"]) {
        "strength"
    } else if contains_any(
        &text,
        &["流程", "链条", "过程", "步骤", "方法", "机制", "阶段"],
    ) {
        "process"
    } else if contains_any(&text, &["人才", "人物", "特质", "学生", "培养", "做事"]) {
        "people"
    } else if contains_any(&text, &["背景", "现状", "问题", "语境"]) {
        "context"
    } else if contains_any(&text, &["方案", "解决", "方法", "策略"]) {
        "solution"
    } else if contains_any(&text, &["价值", "收益", "影响", "未来"]) {
        "value"
    } else {
        match slide.layout.as_str() {
            "timeline" => "timeline",
            "process" => "process",
            "matrix" => "matrix",
            "compare" => "compare",
            "highlight" => "value",
            _ => "content",
        }
    };
    format!("{}_{}", idx + 1, stem)
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '_')
        .to_string()
}

fn match_chart_for_slide(
    slide: &Slide,
    mapping: &PptMasterStyleMapping,
    chart_catalog: &ChartCatalog,
) -> Option<String> {
    if matches!(slide.layout.as_str(), "cover" | "section") {
        return None;
    }
    let text = format!(
        "{} {} {} {}",
        slide.layout,
        slide.title,
        slide.subtitle,
        slide.bullets.join(" ")
    );
    let candidates: &[&str] =
        if contains_any(&text, &["历史", "发展", "时间", "起源", "历程", "路径"])
            || slide.layout == "timeline"
        {
            &["timeline"]
        } else if contains_any(
            &text,
            &["能力", "优势", "学科", "支柱", "方向", "赛道", "实力"],
        ) || slide.layout == "matrix"
        {
            &["vertical_pillars", "labeled_card"]
        } else if contains_any(
            &text,
            &["流程", "链条", "过程", "步骤", "方法", "机制", "阶段"],
        ) || slide.layout == "process"
        {
            &["pipeline_with_stages", "process_flow"]
        } else if contains_any(&text, &["人才", "特质", "标签", "特点", "气质", "培养"])
        {
            &["labeled_card"]
        } else if contains_any(&text, &["总结", "四个", "关键词", "中心", "归纳"])
            || slide.layout == "summary"
        {
            &["hub_spoke"]
        } else if contains_any(&text, &["指标", "成果", "数字", "数据", "%", "倍"]) {
            &["kpi_cards"]
        } else {
            &[]
        };

    for candidate in candidates {
        if chart_catalog.keys.contains(*candidate) {
            return Some((*candidate).to_string());
        }
    }
    if !candidates.is_empty() {
        for bias in &mapping.chart_bias {
            if chart_catalog.keys.contains(bias) {
                return Some(bias.clone());
            }
        }
    }
    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn svg_filename_for_slide(slide: &Slide) -> String {
    format!(
        "{:02}_{}.svg",
        slide.page,
        ascii_stem(&slide.file_stem, "slide")
    )
}

fn ascii_stem(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | ' ' | '/' | '\\' | ':' | '：') && !out.ends_with('_') {
            out.push('_');
        }
        if out.len() >= 24 {
            break;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_notes(plan: &SlidePlan) -> String {
    let mut out = String::new();
    for slide in &plan.slides {
        out.push_str(&format!(
            "# {:02}_{}\n\n{}\n\n---\n\n",
            slide.page,
            ascii_stem(&slide.file_stem, "slide"),
            slide.speaker_note
        ));
    }
    out
}

fn build_notes_with_degradations(
    plan: &SlidePlan,
    degradations: &std::collections::HashMap<usize, Vec<StableContentDegradation>>,
) -> String {
    let mut out = String::new();
    for slide in &plan.slides {
        out.push_str(&format!(
            "# {:02}_{}\n\n{}\n",
            slide.page,
            ascii_stem(&slide.file_stem, "slide"),
            slide.speaker_note
        ));
        if let Some(items) = degradations.get(&slide.page) {
            let mut seen = std::collections::HashSet::new();
            let visible = items
                .iter()
                .filter(|item| item.priority != StableContentPriority::P0Required)
                .filter(|item| seen.insert(format!("{}:{}", item.field, item.original)))
                .collect::<Vec<_>>();
            if !visible.is_empty() {
                out.push_str("\n\n补充说明：\n");
                for item in visible {
                    let field_name = match item.field {
                        "detail" => "补充细节",
                        "evidence" => "材料依据",
                        _ => "补充内容",
                    };
                    out.push_str(&format!(
                        "- {}（{}）：{}\n",
                        if item.block_label.trim().is_empty() {
                            &item.block_id
                        } else {
                            &item.block_label
                        },
                        field_name,
                        item.original.trim()
                    ));
                }
            }
        }
        out.push_str("\n---\n\n");
    }
    out
}

const STABLE_CANVAS_WIDTH: f32 = 1280.0;
const STABLE_CANVAS_HEIGHT: f32 = 720.0;
const STABLE_SAFE_LEFT: f32 = 56.0;
const STABLE_SAFE_RIGHT: f32 = 1224.0;
const STABLE_CONTENT_TOP: f32 = 176.0;
const STABLE_CONTENT_BOTTOM: f32 = 646.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct StableRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl StableRect {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }

    fn inset(self, amount: f32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - amount * 2.0).max(0.0),
            height: (self.height - amount * 2.0).max(0.0),
        }
    }

    fn contains(self, other: Self, tolerance: f32) -> bool {
        other.x >= self.x - tolerance
            && other.y >= self.y - tolerance
            && other.right() <= self.right() + tolerance
            && other.bottom() <= self.bottom() + tolerance
    }

    fn overlaps(self, other: Self, tolerance: f32) -> bool {
        self.x + tolerance < other.right()
            && self.right() > other.x + tolerance
            && self.y + tolerance < other.bottom()
            && self.bottom() > other.y + tolerance
    }

    fn area(self) -> f32 {
        self.width.max(0.0) * self.height.max(0.0)
    }

    fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableElementKind {
    Header,
    Text,
    Card,
    Footer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableTextEmphasis {
    Normal,
    Strong,
    Heading1,
    Heading2,
    Heading3,
}

impl StableTextEmphasis {
    fn heading_level(self) -> Option<usize> {
        match self {
            Self::Heading1 => Some(1),
            Self::Heading2 => Some(2),
            Self::Heading3 => Some(3),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct StableTextRun {
    text: String,
    bold: bool,
    font_scale: f32,
    emphasis: StableTextEmphasis,
}

#[derive(Debug, Clone, PartialEq)]
struct StableTextParagraph {
    runs: Vec<StableTextRun>,
}

#[derive(Debug, Clone, PartialEq)]
struct StableTextLine {
    runs: Vec<StableTextRun>,
}

impl StableTextLine {
    fn plain_text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct StableTextRenderPolicy {
    allow_strong: bool,
    allow_heading_scale: bool,
}

impl StableTextRenderPolicy {
    fn for_element(id: &str, kind: StableElementKind) -> Self {
        let source_like = id.contains("footer") || id.contains("evidence") || id.contains("source");
        Self {
            allow_strong: kind != StableElementKind::Footer && !source_like,
            allow_heading_scale: kind == StableElementKind::Text && !source_like,
        }
    }
}

#[derive(Debug, Clone)]
struct StableLayoutElement {
    id: String,
    rect: StableRect,
    kind: StableElementKind,
    container: Option<StableRect>,
}

#[derive(Debug, Clone)]
struct StableTextFit {
    lines: Vec<String>,
    rich_lines: Vec<StableTextLine>,
    font_size: f32,
    line_height: f32,
    used_height: f32,
    max_line_width: f32,
    required_width: f32,
    required_height: f32,
    overflowed: bool,
}

#[derive(Debug, Clone)]
struct StableTextBoxRecord {
    id: String,
    requested_rect: StableRect,
    fit: StableTextFit,
}

#[derive(Debug, Clone)]
struct StableVisualTokens {
    background: String,
    surface: String,
    panel: String,
    primary: String,
    accent: String,
    text: String,
    muted: String,
    border: String,
    subtle: String,
    font_family: String,
    corner_radius: f32,
    dark: bool,
}

#[derive(Debug, Clone)]
struct StableRenderProfile {
    visual_style_id: String,
    visual_style_source: String,
    chart_catalog_loaded: bool,
    chart_patterns: std::collections::HashSet<String>,
    tokens: StableVisualTokens,
    local_repair: Option<StableLocalRepairPlan>,
}

fn normalize_stable_chart_pattern(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if matches!(ch, '-' | ' ') { '_' } else { ch })
        .collect()
}

fn parse_stable_chart_patterns(text: &str) -> std::collections::HashSet<String> {
    let Ok(serde_json::Value::Object(root)) = serde_json::from_str::<serde_json::Value>(text)
    else {
        return std::collections::HashSet::new();
    };
    let mut patterns = std::collections::HashSet::new();

    // 新版目录把图表定义放在 charts 子对象；同时保留根级 key 兼容旧目录。
    if let Some(charts) = root.get("charts").and_then(serde_json::Value::as_object) {
        patterns.extend(charts.keys().map(|key| normalize_stable_chart_pattern(key)));
    }
    patterns.extend(
        root.iter()
            .filter(|(key, value)| {
                !matches!(key.as_str(), "meta" | "_meta" | "charts") && value.is_object()
            })
            .map(|(key, _)| normalize_stable_chart_pattern(key)),
    );
    patterns.retain(|key| !key.is_empty());
    patterns
}

fn load_stable_chart_patterns(path: &Path) -> std::collections::HashSet<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| parse_stable_chart_patterns(&text))
        .unwrap_or_default()
}

impl StableRenderProfile {
    fn load(root: &Path, plan: &SlidePlan) -> Self {
        let visual_style_id = stable_visual_style_id(&plan.style).to_string();
        let style_path = root
            .join(PPT_MASTER_VISUAL_STYLES_DIR)
            .join(format!("{}.md", visual_style_id));
        let visual_style_source = if style_path.is_file() {
            style_path.to_string_lossy().to_string()
        } else {
            "Pomegranate stable fallback tokens".to_string()
        };
        let chart_index_path = root.join(PPT_MASTER_CHARTS_DIR).join("charts_index.json");
        let chart_patterns = load_stable_chart_patterns(&chart_index_path);
        let chart_catalog_loaded = !chart_patterns.is_empty();
        Self {
            tokens: stable_visual_tokens(plan, &visual_style_id),
            visual_style_id,
            visual_style_source,
            chart_catalog_loaded,
            chart_patterns,
            local_repair: None,
        }
    }

    fn from_plan(plan: &SlidePlan) -> Self {
        let visual_style_id = stable_visual_style_id(&plan.style).to_string();
        Self {
            tokens: stable_visual_tokens(plan, &visual_style_id),
            visual_style_id,
            visual_style_source: "Pomegranate stable compatibility profile".to_string(),
            chart_catalog_loaded: false,
            chart_patterns: std::collections::HashSet::new(),
            local_repair: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StableLayoutKind {
    Anchor,
    EditorialSplit,
    Timeline,
    CategoryGrid,
    Comparison,
    CauseEffect,
    Process,
    Hierarchy,
    Matrix,
    Quote,
    EvidenceLed,
    Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StableMotif {
    PlainEditorial,
    TopBandCard,
    NumberedBadge,
    BigNumber,
    QuoteStatement,
    SplitPanel,
    TimelineNode,
    StepBlock,
    HubSpoke,
    BracketGroup,
    EvidenceStrip,
    ImagePlaceholderEditorial,
    MatrixCell,
    ComparisonColumn,
    SectionBanner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableMotifStatus {
    ProductionReady,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableDensity {
    Anchor,
    Breathing,
    Balanced,
    Dense,
}

#[derive(Debug, Clone, Copy)]
struct StableMotifRequirements {
    min_blocks: usize,
    max_blocks: usize,
    requires_number: bool,
    requires_year_or_metric: bool,
    requires_quote: bool,
    requires_comparison: bool,
    requires_sequence: bool,
    requires_center_concept: bool,
    requires_image: bool,
    min_evidence: usize,
    allowed_density: &'static [StableDensity],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableDecorationPurpose {
    Divider,
    Connector,
    Emphasis,
    Grouping,
    TimelineAxis,
    DataMarker,
}

#[derive(Debug, Clone)]
struct StableDecoration {
    id: String,
    purpose: StableDecorationPurpose,
    rect: StableRect,
    associated_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct StableRenderedBlock {
    id: String,
    rect: StableRect,
    label_complete: bool,
    text_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableContentPriority {
    P0Required,
    P1Optional,
    P2Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableFailureType {
    TextOverflow,
    TextOverlap,
    ContainerOverflow,
    RequiredContentMissing,
    DecorationCollision,
    FooterCollision,
    OutOfSafeBounds,
    MotifIncomplete,
}

impl StableFailureType {
    fn as_str(self) -> &'static str {
        match self {
            Self::TextOverflow => "TextOverflow",
            Self::TextOverlap => "TextOverlap",
            Self::ContainerOverflow => "ContainerOverflow",
            Self::RequiredContentMissing => "RequiredContentMissing",
            Self::DecorationCollision => "DecorationCollision",
            Self::FooterCollision => "FooterCollision",
            Self::OutOfSafeBounds => "OutOfSafeBounds",
            Self::MotifIncomplete => "MotifIncomplete",
        }
    }
}

#[derive(Debug, Clone)]
struct StableLayoutFailure {
    page_index: usize,
    block_id: Option<String>,
    text_role: Option<String>,
    failure_type: StableFailureType,
    bounds: Option<StableRect>,
    required_width: Option<f32>,
    required_height: Option<f32>,
    attempted_strategy: Vec<String>,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableRepairLevel {
    TextBox,
    ContentBlock,
    Motif,
    AlternateMotif,
    Page,
}

impl StableRepairLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::TextBox => "TextBox",
            Self::ContentBlock => "ContentBlock",
            Self::Motif => "Motif",
            Self::AlternateMotif => "AlternateMotif",
            Self::Page => "Page",
        }
    }
}

#[derive(Debug, Clone)]
struct StableLocalRepairPlan {
    block_id: Option<String>,
    text_role: Option<String>,
    failure_type: StableFailureType,
    level: StableRepairLevel,
    strategy: &'static str,
}

#[derive(Debug, Clone)]
struct StableContentDegradation {
    block_id: String,
    block_label: String,
    field: &'static str,
    action: &'static str,
    priority: StableContentPriority,
    original: String,
}

fn stable_content_priority(field: &str) -> StableContentPriority {
    match field {
        "title" | "coreMessage" | "label" | "text" => StableContentPriority::P0Required,
        "detail" => StableContentPriority::P1Optional,
        _ => StableContentPriority::P2Optional,
    }
}

impl StableMotif {
    fn as_str(self) -> &'static str {
        match self {
            Self::PlainEditorial => "plain_editorial",
            Self::TopBandCard => "top_band_card",
            Self::NumberedBadge => "numbered_badge_card",
            Self::BigNumber => "big_number_block",
            Self::QuoteStatement => "quote_statement",
            Self::SplitPanel => "split_panel",
            Self::TimelineNode => "timeline_node",
            Self::StepBlock => "step_block",
            Self::HubSpoke => "hub_spoke",
            Self::BracketGroup => "bracket_group",
            Self::EvidenceStrip => "evidence_strip",
            Self::ImagePlaceholderEditorial => "image_placeholder_editorial",
            Self::MatrixCell => "matrix_cell",
            Self::ComparisonColumn => "comparison_column",
            Self::SectionBanner => "section_banner",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StableAccentPosition {
    None,
    Top,
    Bottom,
    Center,
    Split,
    Bracket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StableContainerStyle {
    Borderless,
    PartialRule,
    FilledPanel,
    SplitField,
    Node,
    Band,
    Matrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StableNumberStyle {
    None,
    Circle,
    Square,
    Text,
    Roman,
    Hero,
    Year,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StableVisualSignature {
    layout_family: StableLayoutKind,
    motif_family: StableMotif,
    accent_position: StableAccentPosition,
    container_style: StableContainerStyle,
    number_style: StableNumberStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StableFocalRegion {
    Left,
    Center,
    Split,
    Grid,
    Axis,
    Radial,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StableStructureFingerprint {
    layout_family: StableLayoutKind,
    focal_region: StableFocalRegion,
    rows: u8,
    columns: u8,
    card_count: u8,
    has_axis: bool,
    has_connectors: bool,
    has_center_node: bool,
    has_radial_structure: bool,
    has_big_number: bool,
    has_quote: bool,
    has_chart_structure: bool,
    asymmetric: bool,
    element_count_band: u8,
    occupancy_band: u8,
}

impl StableStructureFingerprint {
    fn describe(self) -> String {
        format!(
            "{}:{:?}:{}x{}:cards={}:axis={}:links={}:center={}:radial={}:number={}:quote={}:chart={}:asym={}:elements={}:occupancy={}",
            self.layout_family.as_str(),
            self.focal_region,
            self.rows,
            self.columns,
            self.card_count,
            self.has_axis,
            self.has_connectors,
            self.has_center_node,
            self.has_radial_structure,
            self.has_big_number,
            self.has_quote,
            self.has_chart_structure,
            self.asymmetric,
            self.element_count_band,
            self.occupancy_band,
        )
    }
}

impl StableVisualSignature {
    fn describe(self) -> String {
        format!(
            "{}:{}:{:?}:{:?}:{:?}",
            self.layout_family.as_str(),
            self.motif_family.as_str(),
            self.accent_position,
            self.container_style,
            self.number_style
        )
    }
}

#[derive(Debug, Clone)]
struct StableVisualSelection {
    signature: StableVisualSignature,
    structure_fingerprint: StableStructureFingerprint,
    motif_reuse_count: usize,
    duplicate_signature: bool,
    rejected: Vec<String>,
    fallback_reason: Option<String>,
}

impl StableLayoutKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Anchor => "anchor",
            Self::EditorialSplit => "editorial_split",
            Self::Timeline => "timeline",
            Self::CategoryGrid => "category_grid",
            Self::Comparison => "comparison",
            Self::CauseEffect => "cause_effect",
            Self::Process => "process",
            Self::Hierarchy => "hierarchy",
            Self::Matrix => "matrix",
            Self::Quote => "quote",
            Self::EvidenceLed => "evidence_led",
            Self::Summary => "summary",
        }
    }
}

fn stable_layout_aliases(layout: StableLayoutKind) -> &'static [&'static str] {
    match layout {
        StableLayoutKind::Anchor => &["anchor", "cover"],
        StableLayoutKind::EditorialSplit => &["editorial_split", "image_text"],
        StableLayoutKind::Timeline => &["timeline", "roadmap_vertical", "gantt_chart"],
        StableLayoutKind::CategoryGrid => &[
            "category",
            "cards",
            "category_grid",
            "labeled_card",
            "icon_grid",
        ],
        StableLayoutKind::Comparison => &[
            "compare",
            "comparison",
            "comparison_columns",
            "comparison_table",
            "pros_cons_chart",
        ],
        StableLayoutKind::CauseEffect => &[
            "cause",
            "cause_effect",
            "cause_analysis",
            "fishbone",
            "fishbone_diagram",
        ],
        StableLayoutKind::Process => &[
            "process",
            "process_flow",
            "pipeline_with_stages",
            "chevron_process",
            "chevron_chain_with_tail",
            "numbered_steps",
            "circular_stages",
        ],
        StableLayoutKind::Hierarchy => &[
            "hierarchy",
            "pyramid",
            "pyramid_chart",
            "pyramid_isometric",
            "layered_architecture",
            "top_down_tree",
        ],
        StableLayoutKind::Matrix => &[
            "matrix",
            "matrix_2x2",
            "feature_matrix_table",
            "quadrant_text_bullets",
        ],
        StableLayoutKind::Quote => &["quote"],
        StableLayoutKind::EvidenceLed => &["highlight", "evidence_led", "kpi_cards"],
        StableLayoutKind::Summary => &["summary"],
    }
}

fn stable_layout_has_internal_renderer(layout: StableLayoutKind) -> bool {
    // Every StableLayoutKind is backed by a Rust SVG renderer. The external catalog is semantic
    // enrichment only and must never disable an internal production capability.
    matches!(
        layout,
        StableLayoutKind::Anchor
            | StableLayoutKind::EditorialSplit
            | StableLayoutKind::Timeline
            | StableLayoutKind::CategoryGrid
            | StableLayoutKind::Comparison
            | StableLayoutKind::CauseEffect
            | StableLayoutKind::Process
            | StableLayoutKind::Hierarchy
            | StableLayoutKind::Matrix
            | StableLayoutKind::Quote
            | StableLayoutKind::EvidenceLed
            | StableLayoutKind::Summary
    )
}

fn stable_layout_is_available(
    layout: StableLayoutKind,
    chart_patterns: &std::collections::HashSet<String>,
) -> bool {
    stable_layout_has_internal_renderer(layout)
        || stable_layout_aliases(layout)
            .iter()
            .any(|alias| chart_patterns.contains(*alias))
}

fn stable_layout_matches_signal(layout: StableLayoutKind, signals: &[&str]) -> bool {
    signals.iter().any(|signal| {
        let normalized = normalize_stable_chart_pattern(signal);
        stable_layout_aliases(layout).contains(&normalized.as_str())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StableDetailLevel {
    Full,
    Reduced,
    Essential,
}

#[derive(Debug)]
struct StablePageDraft {
    body: String,
    elements: Vec<StableLayoutElement>,
    text_boxes: Vec<StableTextBoxRecord>,
    decorations: Vec<StableDecoration>,
    rendered_blocks: Vec<StableRenderedBlock>,
    degradations: Vec<StableContentDegradation>,
    warnings: Vec<String>,
    hard_failures: Vec<String>,
}

impl StablePageDraft {
    fn new() -> Self {
        Self {
            body: String::new(),
            elements: Vec::new(),
            text_boxes: Vec::new(),
            decorations: Vec::new(),
            rendered_blocks: Vec::new(),
            degradations: Vec::new(),
            warnings: Vec::new(),
            hard_failures: Vec::new(),
        }
    }

    fn push_rect(&mut self, id: &str, rect: StableRect, kind: StableElementKind) {
        self.elements.push(StableLayoutElement {
            id: id.to_string(),
            rect,
            kind,
            container: None,
        });
    }

    fn push_decoration(
        &mut self,
        id: &str,
        purpose: StableDecorationPurpose,
        rect: StableRect,
        associated_ids: &[&str],
    ) {
        self.decorations.push(StableDecoration {
            id: id.to_string(),
            purpose,
            rect,
            associated_ids: associated_ids
                .iter()
                .map(|value| value.to_string())
                .collect(),
        });
    }

    fn push_degradation(
        &mut self,
        block_id: &str,
        block_label: &str,
        field: &'static str,
        action: &'static str,
        original: &str,
    ) {
        if original.trim().is_empty() {
            return;
        }
        self.warnings.push(format!(
            "[Stable Content Degradation] block={} field={} action={} severity=warning",
            block_id, field, action
        ));
        self.degradations.push(StableContentDegradation {
            block_id: block_id.to_string(),
            block_label: block_label.to_string(),
            field,
            action,
            priority: stable_content_priority(field),
            original: original.to_string(),
        });
    }
}

#[derive(Debug)]
struct StableRenderedSlide {
    svg: String,
    layout: String,
    motif: String,
    visual_signature: String,
    structure_fingerprint: String,
    motif_reuse_count: usize,
    duplicate_signature: bool,
    motif_gate_rejections: Vec<String>,
    motif_fallback_reason: Option<String>,
    degradations: Vec<StableContentDegradation>,
    local_repair_logs: Vec<String>,
    reflow_attempts: usize,
    warnings: Vec<String>,
}

fn stable_visual_style_id(style: &str) -> &'static str {
    if style.contains("学术") {
        "editorial"
    } else if style.contains("科技") {
        "blueprint"
    } else if style.contains("路演") {
        "dark-tech"
    } else if style.contains("图文") {
        "photo-editorial"
    } else {
        "swiss-minimal"
    }
}

fn stable_visual_tokens(plan: &SlidePlan, visual_style_id: &str) -> StableVisualTokens {
    let background =
        normalize_hex_color(&plan.theme.background).unwrap_or_else(|| "#F7F9FC".to_string());
    let primary = normalize_hex_color(&plan.theme.primary).unwrap_or_else(|| "#2563EB".to_string());
    let accent = normalize_hex_color(&plan.theme.secondary)
        .or_else(|| normalize_hex_color(&plan.theme.accent))
        .unwrap_or_else(|| "#7C3AED".to_string());
    let dark = color_luminance(&background).unwrap_or(0.95) < 0.42;
    let surface = if dark {
        blend_hex(&background, "#FFFFFF", 0.08)
    } else {
        blend_hex(&background, "#FFFFFF", 0.88)
    };
    let panel = if dark {
        blend_hex(&background, &primary, 0.22)
    } else {
        blend_hex(&background, &primary, 0.07)
    };
    let border = if dark {
        blend_hex(&background, &primary, 0.48)
    } else {
        blend_hex(&background, &primary, 0.26)
    };
    let subtle = if dark {
        blend_hex(&background, "#FFFFFF", 0.14)
    } else {
        blend_hex(&background, "#000000", 0.06)
    };
    let (text, muted) = if dark {
        ("#F8FAFC".to_string(), "#B9C6D8".to_string())
    } else {
        ("#142033".to_string(), "#52657D".to_string())
    };
    let corner_radius = match visual_style_id {
        "swiss-minimal" | "editorial" => 4.0,
        "blueprint" => 6.0,
        "dark-tech" => 8.0,
        _ => 10.0,
    };
    StableVisualTokens {
        background,
        surface,
        panel,
        primary,
        accent,
        text,
        muted,
        border,
        subtle,
        font_family: "Microsoft YaHei, Arial, sans-serif".to_string(),
        corner_radius,
        dark,
    }
}

fn normalize_hex_color(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed.chars().skip(1).all(|ch| ch.is_ascii_hexdigit())
    {
        Some(trimmed.to_ascii_uppercase())
    } else {
        None
    }
}

fn parse_hex_rgb(value: &str) -> Option<(u8, u8, u8)> {
    let normalized = normalize_hex_color(value)?;
    Some((
        u8::from_str_radix(&normalized[1..3], 16).ok()?,
        u8::from_str_radix(&normalized[3..5], 16).ok()?,
        u8::from_str_radix(&normalized[5..7], 16).ok()?,
    ))
}

fn blend_hex(base: &str, overlay: &str, overlay_ratio: f32) -> String {
    let ratio = overlay_ratio.clamp(0.0, 1.0);
    let (br, bg, bb) = parse_hex_rgb(base).unwrap_or((247, 249, 252));
    let (or, og, ob) = parse_hex_rgb(overlay).unwrap_or((255, 255, 255));
    let mix = |a: u8, b: u8| (a as f32 * (1.0 - ratio) + b as f32 * ratio).round() as u8;
    format!("#{:02X}{:02X}{:02X}", mix(br, or), mix(bg, og), mix(bb, ob))
}

fn color_luminance(value: &str) -> Option<f32> {
    let (r, g, b) = parse_hex_rgb(value)?;
    Some((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0)
}

#[derive(Debug, Clone)]
struct StableRenderAttempt {
    layout: StableLayoutKind,
    motif: StableMotif,
    detail_level: StableDetailLevel,
    repair: Option<StableLocalRepairPlan>,
}

#[derive(Debug)]
struct StableRenderAttemptResult {
    draft: StablePageDraft,
    svg: Option<String>,
    failures: Vec<StableLayoutFailure>,
}

fn repair_text_box_locally(failure: &StableLayoutFailure) -> StableLocalRepairPlan {
    StableLocalRepairPlan {
        block_id: failure.block_id.clone(),
        text_role: failure.text_role.clone(),
        failure_type: failure.failure_type,
        level: StableRepairLevel::TextBox,
        strategy: if failure.failure_type == StableFailureType::FooterCollision {
            "shorten_or_omit_footer_annotation"
        } else {
            "reduce_font_and_rewrap"
        },
    }
}

fn repair_content_block_locally(failure: &StableLayoutFailure) -> StableLocalRepairPlan {
    StableLocalRepairPlan {
        block_id: failure.block_id.clone(),
        text_role: failure.text_role.clone(),
        failure_type: failure.failure_type,
        level: StableRepairLevel::ContentBlock,
        strategy: "expand_target_block_and_compact_padding",
    }
}

fn reflow_motif_locally(failure: &StableLayoutFailure) -> StableLocalRepairPlan {
    StableLocalRepairPlan {
        block_id: failure.block_id.clone(),
        text_role: failure.text_role.clone(),
        failure_type: failure.failure_type,
        level: StableRepairLevel::Motif,
        strategy: "reflow_current_motif_internal_layout",
    }
}

fn rerender_page_with_fallback_motif(failure: &StableLayoutFailure) -> StableLocalRepairPlan {
    StableLocalRepairPlan {
        block_id: failure.block_id.clone(),
        text_role: failure.text_role.clone(),
        failure_type: failure.failure_type,
        level: StableRepairLevel::AlternateMotif,
        strategy: "rerender_current_page_with_semantic_backup_motif",
    }
}

fn stable_page_fallback_repair(failure: &StableLayoutFailure) -> StableLocalRepairPlan {
    StableLocalRepairPlan {
        block_id: failure.block_id.clone(),
        text_role: failure.text_role.clone(),
        failure_type: failure.failure_type,
        level: StableRepairLevel::Page,
        strategy: "rerender_current_page_with_safe_layout",
    }
}

fn stable_backup_motif(
    plan: &SlidePlan,
    page_index: usize,
    layout: StableLayoutKind,
    current: StableMotif,
) -> StableMotif {
    let slide = &plan.slides[page_index];
    stable_motif_candidates(slide, layout)
        .into_iter()
        .find(|motif| {
            *motif != current
                && stable_motif_status(*motif) == StableMotifStatus::ProductionReady
                && stable_motif_gate_reason(plan, page_index, layout, *motif).is_none()
        })
        .or_else(|| {
            [
                StableMotif::PlainEditorial,
                StableMotif::EvidenceStrip,
                StableMotif::TopBandCard,
                StableMotif::NumberedBadge,
                StableMotif::SplitPanel,
                StableMotif::TimelineNode,
                StableMotif::StepBlock,
                StableMotif::ComparisonColumn,
                StableMotif::QuoteStatement,
                StableMotif::SectionBanner,
                StableMotif::BigNumber,
            ]
            .into_iter()
            .find(|motif| {
                *motif != current
                    && stable_motif_status(*motif) == StableMotifStatus::ProductionReady
                    && stable_motif_gate_reason(plan, page_index, layout, *motif).is_none()
            })
        })
        .unwrap_or_else(|| stable_safe_fallback_motif(plan, page_index, layout).0)
}

fn repair_failed_page(
    plan: &SlidePlan,
    slide: &Slide,
    page_index: usize,
    primary_layout: StableLayoutKind,
    primary_motif: StableMotif,
    level: StableRepairLevel,
    failure: &StableLayoutFailure,
) -> StableRenderAttempt {
    match level {
        StableRepairLevel::TextBox => StableRenderAttempt {
            layout: primary_layout,
            motif: primary_motif,
            detail_level: StableDetailLevel::Full,
            repair: Some(repair_text_box_locally(failure)),
        },
        StableRepairLevel::ContentBlock => StableRenderAttempt {
            layout: primary_layout,
            motif: primary_motif,
            detail_level: StableDetailLevel::Full,
            repair: Some(repair_content_block_locally(failure)),
        },
        StableRepairLevel::Motif => StableRenderAttempt {
            layout: primary_layout,
            motif: primary_motif,
            detail_level: StableDetailLevel::Reduced,
            repair: Some(reflow_motif_locally(failure)),
        },
        StableRepairLevel::AlternateMotif => StableRenderAttempt {
            layout: primary_layout,
            motif: stable_backup_motif(plan, page_index, primary_layout, primary_motif),
            detail_level: StableDetailLevel::Reduced,
            repair: Some(rerender_page_with_fallback_motif(failure)),
        },
        StableRepairLevel::Page => {
            let layout = stable_fallback_layout(plan, page_index, slide, primary_layout);
            StableRenderAttempt {
                layout,
                motif: stable_safe_fallback_motif(plan, page_index, layout).0,
                detail_level: StableDetailLevel::Essential,
                repair: Some(stable_page_fallback_repair(failure)),
            }
        }
    }
}

fn run_stable_render_attempt(
    plan: &SlidePlan,
    slide: &Slide,
    profile: &StableRenderProfile,
    attempt: &StableRenderAttempt,
) -> Result<StableRenderAttemptResult, AppError> {
    let mut attempt_profile = profile.clone();
    attempt_profile.local_repair = attempt.repair.clone();
    let mut draft = render_stable_layout(
        plan,
        slide,
        &attempt_profile,
        attempt.layout,
        attempt.motif,
        attempt.detail_level,
    )?;
    let mut problems = draft.hard_failures.clone();
    problems.extend(validate_slide_layout(&draft.elements));
    problems.extend(validate_motif_completeness(
        slide,
        attempt.layout,
        attempt.motif,
        attempt.detail_level,
        &draft,
    ));
    draft.warnings.extend(
        validate_visual_fullness(slide, &draft)
            .into_iter()
            .map(|issue| format!("[Stable Visual Fullness] {} severity=warning", issue)),
    );
    problems.extend(validate_semantic_decorations(&draft));

    let mut svg = None;
    if problems.is_empty() {
        let footer = render_stable_footer(
            plan,
            slide,
            &attempt_profile.tokens,
            &mut draft,
            attempt.repair.as_ref(),
        )?;
        let candidate_svg = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720" width="1280" height="720">
<rect x="0" y="0" width="1280" height="720" fill="{background}"/>
{body}
{footer}
</svg>
"#,
            background = attempt_profile.tokens.background,
            body = draft.body,
            footer = footer,
        );
        problems.extend(validate_slide_layout(&draft.elements));
        problems.extend(validate_semantic_decorations(&draft));
        if problems.is_empty() {
            svg = Some(candidate_svg);
        }
    }
    problems.sort();
    problems.dedup();
    let failures = collect_stable_layout_failures(slide.page, &problems, &draft);
    Ok(StableRenderAttemptResult {
        draft,
        svg,
        failures,
    })
}

fn render_slide_svg_with_profile(
    plan: &SlidePlan,
    slide: &Slide,
    profile: &StableRenderProfile,
) -> Result<StableRenderedSlide, AppError> {
    let page_index = slide
        .page
        .saturating_sub(1)
        .min(plan.slides.len().saturating_sub(1));
    let selections = stable_visual_selections(plan, &profile.chart_patterns);
    let primary_selection = selections
        .get(page_index)
        .cloned()
        .ok_or_else(|| AppError::Custom("稳定模式缺少页面视觉选择结果".to_string()))?;
    let primary_layout = primary_selection.signature.layout_family;
    let primary_motif = primary_selection.signature.motif_family;
    let initial_attempt = StableRenderAttempt {
        layout: primary_layout,
        motif: primary_motif,
        detail_level: StableDetailLevel::Full,
        repair: None,
    };
    let mut failure_log = Vec::new();
    let mut local_repair_logs = Vec::new();
    let mut attempted_strategies = Vec::new();
    let mut result = run_stable_render_attempt(plan, slide, profile, &initial_attempt)?;
    let mut current_failure = primary_stable_layout_failure(&result.failures);
    if result.svg.is_none() {
        failure_log.push(format!(
            "initial {}/{}: {}",
            primary_layout.as_str(),
            primary_motif.as_str(),
            result
                .failures
                .iter()
                .map(|failure| failure.message.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let repair_levels = [
        StableRepairLevel::TextBox,
        StableRepairLevel::ContentBlock,
        StableRepairLevel::Motif,
        StableRepairLevel::AlternateMotif,
        StableRepairLevel::Page,
    ];
    let mut used_attempt = initial_attempt.clone();
    let mut repair_count = 0usize;
    for level in repair_levels {
        if result.svg.is_some() {
            break;
        }
        let target_failure = current_failure.clone().unwrap_or(StableLayoutFailure {
            page_index: slide.page,
            block_id: None,
            text_role: None,
            failure_type: StableFailureType::MotifIncomplete,
            bounds: None,
            required_width: None,
            required_height: None,
            attempted_strategy: attempted_strategies.clone(),
            message: "page has no renderable safe layout".to_string(),
        });
        let attempt = repair_failed_page(
            plan,
            slide,
            page_index,
            primary_layout,
            primary_motif,
            level,
            &target_failure,
        );
        let repair = attempt
            .repair
            .as_ref()
            .expect("local repair attempts always carry a repair plan");
        attempted_strategies.push(repair.strategy.to_string());
        result = run_stable_render_attempt(plan, slide, profile, &attempt)?;
        repair_count += 1;
        let passed = result.svg.is_some();
        let repair_log = format!(
            "[Stable Local Repair] page=P{:02} block={} role={} failure={} level={} strategy={} result={}",
            target_failure.page_index,
            target_failure.block_id.as_deref().unwrap_or("page"),
            target_failure.text_role.as_deref().unwrap_or("layout"),
            target_failure.failure_type.as_str(),
            repair.level.as_str(),
            repair.strategy,
            if passed { "passed" } else { "failed" }
        );
        println!("{}", repair_log);
        local_repair_logs.push(repair_log);
        if !passed {
            failure_log.push(format!(
                "level={} {}/{}: {}",
                repair.level.as_str(),
                attempt.layout.as_str(),
                attempt.motif.as_str(),
                result
                    .failures
                    .iter()
                    .map(|failure| failure.message.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            current_failure = primary_stable_layout_failure(&result.failures).map(|mut failure| {
                failure.attempted_strategy = attempted_strategies.clone();
                failure
            });
        }
        used_attempt = attempt;
    }

    if let Some(svg) = result.svg {
        let mut draft = result.draft;
        draft.warnings.extend(failure_log);
        let used_signature = visual_signature(used_attempt.layout, used_attempt.motif);
        let previous_signature = page_index
            .checked_sub(1)
            .and_then(|previous| selections.get(previous))
            .map(|selection| selection.signature);
        let motif_reuse_count = if used_signature == primary_selection.signature {
            primary_selection.motif_reuse_count
        } else {
            selections[..=page_index]
                .iter()
                .filter(|selection| selection.signature.motif_family == used_attempt.motif)
                .count()
                .max(1)
        };
        return Ok(StableRenderedSlide {
            svg,
            layout: used_attempt.layout.as_str().to_string(),
            motif: used_attempt.motif.as_str().to_string(),
            visual_signature: used_signature.describe(),
            structure_fingerprint: stable_structure_fingerprint(
                slide,
                used_attempt.layout,
                used_attempt.motif,
            )
            .describe(),
            motif_reuse_count,
            duplicate_signature: if used_signature == primary_selection.signature {
                primary_selection.duplicate_signature
            } else {
                previous_signature == Some(used_signature)
            },
            motif_gate_rejections: primary_selection.rejected.clone(),
            motif_fallback_reason: if repair_count == 0 {
                primary_selection.fallback_reason.clone()
            } else {
                Some(format!(
                    "local_repair attempts={} final={}/{}",
                    repair_count,
                    used_attempt.layout.as_str(),
                    used_attempt.motif.as_str()
                ))
            },
            reflow_attempts: repair_count,
            degradations: draft.degradations,
            local_repair_logs,
            warnings: draft.warnings,
        });
    }

    let mut final_failure = current_failure.unwrap_or(StableLayoutFailure {
        page_index: slide.page,
        block_id: None,
        text_role: None,
        failure_type: StableFailureType::MotifIncomplete,
        bounds: None,
        required_width: None,
        required_height: None,
        attempted_strategy: attempted_strategies.clone(),
        message: "page has no renderable safe layout".to_string(),
    });
    final_failure.attempted_strategy = attempted_strategies;
    let final_log = format!(
        "[Stable Page Failure] page=P{:02} block={} role={} failure={} reason={} attempts={}",
        final_failure.page_index,
        final_failure.block_id.as_deref().unwrap_or("page"),
        final_failure.text_role.as_deref().unwrap_or("layout"),
        final_failure.failure_type.as_str(),
        final_failure.message,
        final_failure.attempted_strategy.join(" -> ")
    );
    println!("{}", final_log);
    Err(AppError::Custom(format!(
        "稳定模式页面 P{:02} 无法完成必需内容排版：block={} role={} failure={} required={}x{} bounds={}；已完成 5 级局部修复。{}",
        slide.page,
        final_failure.block_id.as_deref().unwrap_or("page"),
        final_failure.text_role.as_deref().unwrap_or("layout"),
        final_failure.failure_type.as_str(),
        final_failure
            .required_width
            .map(|value| format!("{:.1}", value))
            .unwrap_or_else(|| "unknown".to_string()),
        final_failure
            .required_height
            .map(|value| format!("{:.1}", value))
            .unwrap_or_else(|| "unknown".to_string()),
        final_failure
            .bounds
            .map(|rect| format!("{:.1},{:.1},{:.1},{:.1}", rect.x, rect.y, rect.width, rect.height))
            .unwrap_or_else(|| "unknown".to_string()),
        failure_log.join(" | ")
    )))
}

fn stable_visual_selections(
    plan: &SlidePlan,
    chart_patterns: &std::collections::HashSet<String>,
) -> Vec<StableVisualSelection> {
    let mut selections = Vec::with_capacity(plan.slides.len());
    let mut motif_counts = std::collections::HashMap::<StableMotif, usize>::new();
    let mut structure_counts =
        std::collections::HashMap::<StableStructureFingerprint, usize>::new();
    let layouts = stable_layout_sequence(plan, chart_patterns);
    let body_pages = plan.slides.len().saturating_sub(2).max(1);
    let soft_motif_limit = body_pages.div_ceil(2).max(1);

    for (index, slide) in plan.slides.iter().enumerate() {
        let layout = layouts
            .get(index)
            .copied()
            .unwrap_or(StableLayoutKind::EvidenceLed);
        let candidates = stable_motif_candidates(slide, layout);
        let mut rejected = Vec::new();
        let mut eligible = Vec::new();
        for (rank, motif) in candidates.iter().copied().enumerate() {
            if let Some(reason) = stable_motif_gate_reason(plan, index, layout, motif) {
                rejected.push(format!("rejected={} reason={}", motif.as_str(), reason));
            } else {
                eligible.push((
                    motif,
                    rank + stable_motif_content_score(slide, layout, motif),
                ));
            }
        }
        let mut fallback_reason = None;
        if eligible.is_empty() {
            let (fallback, reason) = stable_safe_fallback_motif(plan, index, layout);
            fallback_reason = Some(format!(
                "requested={} fallback={} reason={}",
                candidates
                    .first()
                    .map(|motif| motif.as_str())
                    .unwrap_or("none"),
                fallback.as_str(),
                reason
            ));
            eligible.push((fallback, 0));
        }
        let previous = selections.last().map(|selection: &StableVisualSelection| {
            (selection.signature, selection.structure_fingerprint)
        });
        let mut best = None;
        let mut best_score = usize::MAX;
        for (motif, content_score) in eligible {
            let signature = visual_signature(layout, motif);
            let structure = stable_structure_fingerprint(slide, layout, motif);
            let reuse = *motif_counts.get(&motif).unwrap_or(&0);
            let structure_reuse = *structure_counts.get(&structure).unwrap_or(&0);
            let repeated_motif = previous.is_some_and(|value| value.0.motif_family == motif);
            let repeated_signature = previous.is_some_and(|value| value.0 == signature);
            let repeated_structure = previous.is_some_and(|value| value.1 == structure);
            let over_soft_limit =
                index > 0 && index + 1 < plan.slides.len() && reuse >= soft_motif_limit;
            let score = content_score
                + reuse * 12
                + structure_reuse * 36
                + usize::from(repeated_motif) * 120
                + usize::from(repeated_signature) * 240
                + usize::from(repeated_structure) * 360
                + usize::from(over_soft_limit) * 60;
            if score < best_score {
                best = Some((signature, structure));
                best_score = score;
            }
        }
        let (signature, structure_fingerprint) = best.unwrap_or_else(|| {
            let motif = StableMotif::PlainEditorial;
            (
                visual_signature(layout, motif),
                stable_structure_fingerprint(slide, layout, motif),
            )
        });
        let duplicate_signature =
            previous.is_some_and(|value| value.0 == signature || value.1 == structure_fingerprint);
        let count = motif_counts.entry(signature.motif_family).or_insert(0);
        *count += 1;
        *structure_counts.entry(structure_fingerprint).or_insert(0) += 1;
        selections.push(StableVisualSelection {
            signature,
            structure_fingerprint,
            motif_reuse_count: *count,
            duplicate_signature,
            rejected,
            fallback_reason,
        });
    }
    selections
}

const STABLE_DENSITY_ALL: &[StableDensity] = &[
    StableDensity::Anchor,
    StableDensity::Breathing,
    StableDensity::Balanced,
    StableDensity::Dense,
];
const STABLE_DENSITY_LIGHT: &[StableDensity] = &[
    StableDensity::Anchor,
    StableDensity::Breathing,
    StableDensity::Balanced,
];
const STABLE_DENSITY_STRUCTURED: &[StableDensity] =
    &[StableDensity::Balanced, StableDensity::Dense];

fn stable_motif_status(motif: StableMotif) -> StableMotifStatus {
    match motif {
        StableMotif::ImagePlaceholderEditorial => StableMotifStatus::Disabled,
        _ => StableMotifStatus::ProductionReady,
    }
}

fn stable_motif_requirements(motif: StableMotif) -> StableMotifRequirements {
    match motif {
        StableMotif::PlainEditorial => StableMotifRequirements {
            min_blocks: 1,
            max_blocks: 3,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: true,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_LIGHT,
        },
        StableMotif::TopBandCard => StableMotifRequirements {
            min_blocks: 2,
            max_blocks: 6,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::NumberedBadge => StableMotifRequirements {
            min_blocks: 2,
            max_blocks: 6,
            requires_number: true,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: true,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_STRUCTURED,
        },
        StableMotif::BigNumber => StableMotifRequirements {
            min_blocks: 1,
            max_blocks: 4,
            requires_number: true,
            requires_year_or_metric: true,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::QuoteStatement => StableMotifRequirements {
            min_blocks: 1,
            max_blocks: 3,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: true,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: true,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_LIGHT,
        },
        StableMotif::SplitPanel => StableMotifRequirements {
            min_blocks: 2,
            max_blocks: 6,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: true,
            requires_sequence: false,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::TimelineNode => StableMotifRequirements {
            min_blocks: 3,
            max_blocks: 5,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: true,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_STRUCTURED,
        },
        StableMotif::StepBlock => StableMotifRequirements {
            min_blocks: 2,
            max_blocks: 5,
            requires_number: true,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: true,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_STRUCTURED,
        },
        StableMotif::HubSpoke => StableMotifRequirements {
            min_blocks: 3,
            max_blocks: 6,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: true,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::BracketGroup => StableMotifRequirements {
            min_blocks: 2,
            max_blocks: 6,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::EvidenceStrip => StableMotifRequirements {
            min_blocks: 1,
            max_blocks: 4,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: true,
            requires_image: false,
            min_evidence: 2,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::ImagePlaceholderEditorial => StableMotifRequirements {
            min_blocks: 1,
            max_blocks: 3,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: false,
            requires_image: true,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::MatrixCell => StableMotifRequirements {
            min_blocks: 4,
            max_blocks: 4,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: true,
            requires_sequence: false,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_STRUCTURED,
        },
        StableMotif::ComparisonColumn => StableMotifRequirements {
            min_blocks: 2,
            max_blocks: 6,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: true,
            requires_sequence: false,
            requires_center_concept: false,
            requires_image: false,
            min_evidence: 0,
            allowed_density: STABLE_DENSITY_ALL,
        },
        StableMotif::SectionBanner => StableMotifRequirements {
            min_blocks: 1,
            max_blocks: 6,
            requires_number: false,
            requires_year_or_metric: false,
            requires_quote: false,
            requires_comparison: false,
            requires_sequence: false,
            requires_center_concept: true,
            requires_image: false,
            min_evidence: 0,
            allowed_density: &[StableDensity::Anchor],
        },
    }
}

fn stable_density(slide: &Slide) -> StableDensity {
    for value in [&slide.page_rhythm, &slide.density] {
        match value.trim().to_ascii_lowercase().as_str() {
            "anchor" => return StableDensity::Anchor,
            "breathing" => return StableDensity::Breathing,
            "balanced" => return StableDensity::Balanced,
            "dense" => return StableDensity::Dense,
            _ => {}
        }
    }
    StableDensity::Balanced
}

fn stable_motif_gate_reason(
    plan: &SlidePlan,
    index: usize,
    layout: StableLayoutKind,
    motif: StableMotif,
) -> Option<String> {
    let slide = &plan.slides[index];
    let blocks = slide_blocks(slide);
    let requirements = stable_motif_requirements(motif);
    if blocks.len() < requirements.min_blocks {
        return Some(format!(
            "requires at least {} blocks",
            requirements.min_blocks
        ));
    }
    if blocks.len() > requirements.max_blocks {
        return Some(format!(
            "supports at most {} blocks",
            requirements.max_blocks
        ));
    }
    if !requirements
        .allowed_density
        .contains(&stable_density(slide))
    {
        return Some(format!("density {} is not supported", slide.density));
    }
    if blocks
        .iter()
        .any(|block| block.label.trim().is_empty() || block.text.trim().is_empty())
        && matches!(
            motif,
            StableMotif::TopBandCard
                | StableMotif::NumberedBadge
                | StableMotif::TimelineNode
                | StableMotif::StepBlock
                | StableMotif::MatrixCell
                | StableMotif::ComparisonColumn
        )
    {
        return Some("requires label and text for every block".to_string());
    }
    if requirements.requires_center_concept && stable_core_message(slide).trim().is_empty() {
        return Some("requires a non-empty core message".to_string());
    }
    if requirements.requires_sequence && !stable_has_sequence_semantics(slide) {
        return Some("requires timeline, stage, process, or sequence semantics".to_string());
    }
    if requirements.requires_comparison && !stable_has_dual_relationship(slide) {
        return Some(
            "requires comparison, dual-side, cause-effect, or matrix semantics".to_string(),
        );
    }
    if requirements.requires_quote && !stable_has_quote_semantics(slide) {
        return Some("requires a short quote, definition, or strong statement".to_string());
    }
    if motif == StableMotif::QuoteStatement
        && blocks.iter().any(|block| !block.detail.trim().is_empty())
    {
        return Some("quote layout cannot preserve the available supporting detail".to_string());
    }
    if requirements.requires_year_or_metric
        && !blocks
            .iter()
            .all(|block| stable_numeric_anchor(block).is_some())
    {
        return Some("no meaningful numeric anchor for every displayed block".to_string());
    }
    if requirements.requires_number
        && !requirements.requires_sequence
        && !blocks
            .iter()
            .any(|block| stable_numeric_anchor(block).is_some())
    {
        return Some("requires meaningful numbers".to_string());
    }
    if slide.evidence.len() < requirements.min_evidence {
        return Some(format!(
            "requires at least {} evidence items",
            requirements.min_evidence
        ));
    }
    if requirements.requires_image {
        return Some("stable mode has no resolved image resource".to_string());
    }
    if motif == StableMotif::SectionBanner
        && index != 0
        && index + 1 != plan.slides.len()
        && !matches!(layout, StableLayoutKind::Anchor | StableLayoutKind::Summary)
    {
        return Some(
            "section banner is limited to anchor, transition, or summary pages".to_string(),
        );
    }
    if motif == StableMotif::EvidenceStrip
        && estimated_motif_block_height(layout, blocks.len()) < 166.0
    {
        return Some("available block height cannot carry evidence strip content".to_string());
    }
    match stable_motif_status(motif) {
        StableMotifStatus::ProductionReady => None,
        StableMotifStatus::Disabled => Some("status=disabled".to_string()),
    }
}

fn stable_has_sequence_semantics(slide: &Slide) -> bool {
    let relation = slide.relation.to_ascii_lowercase();
    let chart = slide.chart_type.to_ascii_lowercase();
    let layout = slide.layout.to_ascii_lowercase();
    relation.contains("timeline")
        || relation.contains("process")
        || relation.contains("sequence")
        || relation.contains("stage")
        || chart.contains("timeline")
        || chart.contains("process")
        || chart.contains("step")
        || layout.contains("timeline")
        || layout.contains("process")
}

fn stable_has_dual_relationship(slide: &Slide) -> bool {
    let semantics = format!(
        "{} {} {} {}",
        slide.relation, slide.chart_type, slide.layout, slide.core_message
    )
    .to_ascii_lowercase();
    [
        "compare",
        "comparison",
        "cause",
        "matrix",
        "对比",
        "比较",
        "原因",
        "结果",
        "两类",
        "双方",
    ]
    .iter()
    .any(|token| semantics.contains(token))
}

fn stable_has_quote_semantics(slide: &Slide) -> bool {
    let core = stable_core_message(slide);
    if core.trim().is_empty() || core.chars().count() > 72 {
        return false;
    }
    matches!(
        stable_density(slide),
        StableDensity::Anchor | StableDensity::Breathing
    ) || ["“", "”", "定义", "核心论断", "结论", "意味着"]
        .iter()
        .any(|token| core.contains(token) || slide.title.contains(token))
}

fn stable_numeric_anchor(block: &ContentBlock) -> Option<String> {
    extract_year_token(block).or_else(|| {
        let text = format!("{} {} {}", block.label, block.text, block.detail);
        let chars = text.chars().collect::<Vec<_>>();
        let mut start = 0;
        while start < chars.len() {
            if !chars[start].is_ascii_digit() {
                start += 1;
                continue;
            }
            let mut end = start;
            while end < chars.len() && (chars[end].is_ascii_digit() || ".,%".contains(chars[end])) {
                end += 1;
            }
            let value = chars[start..end].iter().collect::<String>();
            let suffix = chars.get(end).copied().unwrap_or(' ');
            if value.contains('%')
                || value.chars().filter(|ch| ch.is_ascii_digit()).count() >= 2
                || "年项个倍级类阶段".contains(suffix)
            {
                return Some(value);
            }
            start = end;
        }
        None
    })
}

fn estimated_motif_block_height(layout: StableLayoutKind, block_count: usize) -> f32 {
    let count = block_count.max(1);
    match layout {
        StableLayoutKind::EditorialSplit => {
            (430.0 - 14.0 * count.saturating_sub(1) as f32) / count as f32
        }
        StableLayoutKind::Comparison => {
            (344.0 - 14.0 * ((count.div_ceil(2)).saturating_sub(1)) as f32)
                / count.div_ceil(2) as f32
        }
        StableLayoutKind::CategoryGrid | StableLayoutKind::Matrix => {
            let columns = if count <= 2 {
                count
            } else if count == 4 {
                2
            } else {
                3
            };
            let rows = count.div_ceil(columns.max(1));
            (470.0 - 20.0 * rows.saturating_sub(1) as f32) / rows as f32
        }
        _ => 190.0,
    }
}

fn stable_motif_content_score(
    slide: &Slide,
    _layout: StableLayoutKind,
    motif: StableMotif,
) -> usize {
    let blocks = slide_blocks(slide);
    match motif {
        StableMotif::PlainEditorial if blocks.len() <= 2 => 0,
        StableMotif::TopBandCard if blocks.len() >= 3 => 0,
        StableMotif::QuoteStatement if stable_density(slide) == StableDensity::Anchor => 0,
        StableMotif::EvidenceStrip if slide.evidence.len() >= blocks.len().max(2) => 0,
        StableMotif::TimelineNode | StableMotif::StepBlock
            if stable_has_sequence_semantics(slide) =>
        {
            0
        }
        StableMotif::ComparisonColumn | StableMotif::SplitPanel
            if stable_has_dual_relationship(slide) =>
        {
            0
        }
        _ => 4,
    }
}

fn stable_safe_fallback_motif(
    plan: &SlidePlan,
    index: usize,
    layout: StableLayoutKind,
) -> (StableMotif, String) {
    let slide = &plan.slides[index];
    let blocks = slide_blocks(slide);
    let ordered = if stable_has_sequence_semantics(slide) {
        vec![
            StableMotif::NumberedBadge,
            StableMotif::TopBandCard,
            StableMotif::PlainEditorial,
        ]
    } else if blocks.len() <= 3 {
        vec![StableMotif::PlainEditorial, StableMotif::TopBandCard]
    } else {
        vec![StableMotif::TopBandCard, StableMotif::PlainEditorial]
    };
    for motif in ordered {
        if stable_motif_gate_reason(plan, index, layout, motif).is_none() {
            return (
                motif,
                "no requested motif satisfied production requirements".to_string(),
            );
        }
    }
    (
        StableMotif::TopBandCard,
        "using the safest content-bearing renderer after all strict gates failed".to_string(),
    )
}

fn stable_motif_candidates(slide: &Slide, layout: StableLayoutKind) -> Vec<StableMotif> {
    let blocks = slide_blocks(slide);
    let has_year = blocks
        .iter()
        .any(|block| extract_year_token(block).is_some());
    let has_visual_request = slide.visual_intent.contains("图片")
        || slide.visual_intent.to_ascii_lowercase().contains("image")
        || slide.visual_hint.contains("图片");
    match layout {
        StableLayoutKind::Anchor => vec![
            StableMotif::SectionBanner,
            StableMotif::QuoteStatement,
            StableMotif::PlainEditorial,
        ],
        StableLayoutKind::EditorialSplit => {
            let mut candidates = vec![
                StableMotif::PlainEditorial,
                StableMotif::EvidenceStrip,
                StableMotif::SplitPanel,
            ];
            if has_visual_request {
                candidates.insert(0, StableMotif::ImagePlaceholderEditorial);
            }
            candidates
        }
        StableLayoutKind::Timeline => {
            if has_year {
                vec![
                    StableMotif::BigNumber,
                    StableMotif::TimelineNode,
                    StableMotif::StepBlock,
                ]
            } else {
                vec![
                    StableMotif::TimelineNode,
                    StableMotif::StepBlock,
                    StableMotif::BigNumber,
                ]
            }
        }
        StableLayoutKind::CategoryGrid => {
            if blocks.len() >= 4 {
                vec![
                    StableMotif::TopBandCard,
                    StableMotif::MatrixCell,
                    StableMotif::BracketGroup,
                ]
            } else {
                vec![
                    StableMotif::PlainEditorial,
                    StableMotif::TopBandCard,
                    StableMotif::BracketGroup,
                ]
            }
        }
        StableLayoutKind::Comparison => vec![
            StableMotif::ComparisonColumn,
            StableMotif::SplitPanel,
            StableMotif::EvidenceStrip,
        ],
        StableLayoutKind::CauseEffect => vec![
            StableMotif::HubSpoke,
            StableMotif::SplitPanel,
            StableMotif::BracketGroup,
        ],
        StableLayoutKind::Process => vec![
            StableMotif::StepBlock,
            StableMotif::NumberedBadge,
            StableMotif::TimelineNode,
        ],
        StableLayoutKind::Hierarchy => vec![
            StableMotif::BracketGroup,
            StableMotif::StepBlock,
            StableMotif::BigNumber,
        ],
        StableLayoutKind::Matrix => vec![
            StableMotif::MatrixCell,
            StableMotif::BracketGroup,
            StableMotif::TopBandCard,
        ],
        StableLayoutKind::Quote => vec![
            StableMotif::QuoteStatement,
            StableMotif::PlainEditorial,
            StableMotif::EvidenceStrip,
        ],
        StableLayoutKind::EvidenceLed => vec![
            StableMotif::EvidenceStrip,
            StableMotif::BigNumber,
            StableMotif::PlainEditorial,
        ],
        StableLayoutKind::Summary => vec![
            StableMotif::HubSpoke,
            StableMotif::SectionBanner,
            StableMotif::EvidenceStrip,
        ],
    }
}

fn visual_signature(layout: StableLayoutKind, motif: StableMotif) -> StableVisualSignature {
    let (accent_position, container_style, number_style) = match motif {
        StableMotif::PlainEditorial => (
            StableAccentPosition::Top,
            StableContainerStyle::Borderless,
            StableNumberStyle::None,
        ),
        StableMotif::TopBandCard => (
            StableAccentPosition::Top,
            StableContainerStyle::FilledPanel,
            StableNumberStyle::None,
        ),
        StableMotif::NumberedBadge => (
            StableAccentPosition::None,
            StableContainerStyle::PartialRule,
            StableNumberStyle::Circle,
        ),
        StableMotif::BigNumber => (
            StableAccentPosition::Center,
            StableContainerStyle::Borderless,
            StableNumberStyle::Hero,
        ),
        StableMotif::QuoteStatement => (
            StableAccentPosition::Bottom,
            StableContainerStyle::FilledPanel,
            StableNumberStyle::None,
        ),
        StableMotif::SplitPanel => (
            StableAccentPosition::Split,
            StableContainerStyle::SplitField,
            StableNumberStyle::Text,
        ),
        StableMotif::TimelineNode => (
            StableAccentPosition::Center,
            StableContainerStyle::Node,
            StableNumberStyle::Year,
        ),
        StableMotif::StepBlock => (
            StableAccentPosition::Bottom,
            StableContainerStyle::Band,
            StableNumberStyle::Square,
        ),
        StableMotif::HubSpoke => (
            StableAccentPosition::Center,
            StableContainerStyle::Node,
            StableNumberStyle::None,
        ),
        StableMotif::BracketGroup => (
            StableAccentPosition::Bracket,
            StableContainerStyle::Borderless,
            StableNumberStyle::Roman,
        ),
        StableMotif::EvidenceStrip => (
            StableAccentPosition::Bottom,
            StableContainerStyle::PartialRule,
            StableNumberStyle::None,
        ),
        StableMotif::ImagePlaceholderEditorial => (
            StableAccentPosition::Split,
            StableContainerStyle::SplitField,
            StableNumberStyle::None,
        ),
        StableMotif::MatrixCell => (
            StableAccentPosition::Top,
            StableContainerStyle::Matrix,
            StableNumberStyle::Text,
        ),
        StableMotif::ComparisonColumn => (
            StableAccentPosition::Top,
            StableContainerStyle::Band,
            StableNumberStyle::Tab,
        ),
        StableMotif::SectionBanner => (
            StableAccentPosition::Bottom,
            StableContainerStyle::Band,
            StableNumberStyle::Hero,
        ),
    };
    StableVisualSignature {
        layout_family: layout,
        motif_family: motif,
        accent_position,
        container_style,
        number_style,
    }
}

fn stable_structure_fingerprint(
    slide: &Slide,
    layout: StableLayoutKind,
    motif: StableMotif,
) -> StableStructureFingerprint {
    let block_count = slide_blocks(slide).len().clamp(1, 6) as u8;
    let (focal_region, rows, columns, card_count) = match layout {
        StableLayoutKind::Anchor => (StableFocalRegion::Left, 1, 2, 0),
        StableLayoutKind::EditorialSplit => (StableFocalRegion::Left, block_count, 2, 0),
        StableLayoutKind::Timeline => (StableFocalRegion::Axis, 1, block_count, 0),
        StableLayoutKind::CategoryGrid => {
            let columns = if block_count <= 2 {
                2
            } else if block_count == 4 {
                2
            } else {
                3
            };
            let rows = block_count.div_ceil(columns);
            (StableFocalRegion::Grid, rows, columns, block_count)
        }
        StableLayoutKind::Comparison => (StableFocalRegion::Split, 1, 2, 0),
        StableLayoutKind::CauseEffect => (StableFocalRegion::Radial, 1, 3, 0),
        StableLayoutKind::Process => (StableFocalRegion::Axis, 1, block_count, 0),
        StableLayoutKind::Hierarchy => (StableFocalRegion::Vertical, block_count, 1, 0),
        StableLayoutKind::Matrix => (StableFocalRegion::Grid, 2, 2, 0),
        StableLayoutKind::Quote => (StableFocalRegion::Center, 1, 1, 0),
        StableLayoutKind::EvidenceLed => (StableFocalRegion::Left, block_count, 2, 0),
        StableLayoutKind::Summary => (StableFocalRegion::Radial, 2, 3, 0),
    };
    let density = stable_density(slide);
    StableStructureFingerprint {
        layout_family: layout,
        focal_region,
        rows,
        columns,
        card_count,
        has_axis: matches!(
            layout,
            StableLayoutKind::Timeline | StableLayoutKind::Process | StableLayoutKind::Matrix
        ),
        has_connectors: matches!(
            layout,
            StableLayoutKind::Timeline
                | StableLayoutKind::Process
                | StableLayoutKind::CauseEffect
                | StableLayoutKind::Hierarchy
                | StableLayoutKind::Summary
        ),
        has_center_node: matches!(
            layout,
            StableLayoutKind::CauseEffect | StableLayoutKind::Summary
        ),
        has_radial_structure: matches!(
            layout,
            StableLayoutKind::CauseEffect | StableLayoutKind::Summary
        ),
        has_big_number: motif == StableMotif::BigNumber || motif == StableMotif::SectionBanner,
        has_quote: motif == StableMotif::QuoteStatement || layout == StableLayoutKind::Quote,
        has_chart_structure: matches!(
            layout,
            StableLayoutKind::Timeline
                | StableLayoutKind::Comparison
                | StableLayoutKind::CauseEffect
                | StableLayoutKind::Process
                | StableLayoutKind::Hierarchy
                | StableLayoutKind::Matrix
        ),
        asymmetric: matches!(
            layout,
            StableLayoutKind::Anchor
                | StableLayoutKind::EditorialSplit
                | StableLayoutKind::EvidenceLed
                | StableLayoutKind::Hierarchy
        ),
        element_count_band: match block_count {
            0..=2 => 1,
            3..=4 => 2,
            _ => 3,
        },
        occupancy_band: match density {
            StableDensity::Anchor | StableDensity::Breathing => 1,
            StableDensity::Balanced => 2,
            StableDensity::Dense => 3,
        },
    }
}

fn extract_year_token(block: &ContentBlock) -> Option<String> {
    let text = format!("{} {} {}", block.label, block.text, block.detail);
    let mut digits = String::new();
    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            if digits.len() == 4 {
                if let Ok(year) = digits.parse::<u16>() {
                    if (1000..=2999).contains(&year) {
                        return Some(digits);
                    }
                }
            }
            digits.clear();
        }
    }
    None
}

fn roman_numeral(index: usize) -> String {
    match index {
        1 => "I",
        2 => "II",
        3 => "III",
        4 => "IV",
        5 => "V",
        6 => "VI",
        _ => return index.to_string(),
    }
    .to_string()
}

fn stable_compatible_layouts(primary: StableLayoutKind, slide: &Slide) -> Vec<StableLayoutKind> {
    let mut layouts = match primary {
        StableLayoutKind::Anchor => vec![StableLayoutKind::Anchor],
        StableLayoutKind::EditorialSplit => vec![
            StableLayoutKind::EditorialSplit,
            StableLayoutKind::EvidenceLed,
            StableLayoutKind::Quote,
        ],
        StableLayoutKind::Timeline => vec![StableLayoutKind::Timeline, StableLayoutKind::Process],
        StableLayoutKind::CategoryGrid => vec![
            StableLayoutKind::CategoryGrid,
            StableLayoutKind::EvidenceLed,
            StableLayoutKind::EditorialSplit,
            StableLayoutKind::Matrix,
        ],
        StableLayoutKind::Comparison => vec![
            StableLayoutKind::Comparison,
            StableLayoutKind::Matrix,
            StableLayoutKind::EvidenceLed,
        ],
        StableLayoutKind::CauseEffect => vec![
            StableLayoutKind::CauseEffect,
            StableLayoutKind::Process,
            StableLayoutKind::Hierarchy,
        ],
        StableLayoutKind::Process => vec![
            StableLayoutKind::Process,
            StableLayoutKind::Timeline,
            StableLayoutKind::CauseEffect,
        ],
        StableLayoutKind::Hierarchy => vec![
            StableLayoutKind::Hierarchy,
            StableLayoutKind::CauseEffect,
            StableLayoutKind::Process,
        ],
        StableLayoutKind::Matrix => vec![
            StableLayoutKind::Matrix,
            StableLayoutKind::Comparison,
            StableLayoutKind::CategoryGrid,
        ],
        StableLayoutKind::Quote => vec![
            StableLayoutKind::Quote,
            StableLayoutKind::EditorialSplit,
            StableLayoutKind::EvidenceLed,
        ],
        StableLayoutKind::EvidenceLed => vec![
            StableLayoutKind::EvidenceLed,
            StableLayoutKind::EditorialSplit,
            StableLayoutKind::CategoryGrid,
        ],
        StableLayoutKind::Summary => vec![StableLayoutKind::Summary],
    };
    if slide_blocks(slide).len() <= 2 && !layouts.contains(&StableLayoutKind::Quote) {
        layouts.push(StableLayoutKind::Quote);
    }
    layouts
}

fn stable_layout_limit(layout: StableLayoutKind, total: usize) -> usize {
    let short_deck = (6..=8).contains(&total);
    match layout {
        StableLayoutKind::CategoryGrid | StableLayoutKind::EditorialSplit if short_deck => 1,
        StableLayoutKind::CategoryGrid | StableLayoutKind::EditorialSplit => {
            total.saturating_sub(2).div_ceil(4).max(1)
        }
        _ => usize::MAX,
    }
}

fn stable_layout_sequence(
    plan: &SlidePlan,
    chart_patterns: &std::collections::HashSet<String>,
) -> Vec<StableLayoutKind> {
    let mut sequence = Vec::with_capacity(plan.slides.len());
    let mut counts = std::collections::HashMap::<StableLayoutKind, usize>::new();
    let mut used_body = std::collections::HashSet::<StableLayoutKind>::new();

    for (index, slide) in plan.slides.iter().enumerate() {
        let primary =
            stable_semantic_layout(slide, index, plan.slides.len(), &plan.title, chart_patterns);
        let candidates = stable_compatible_layouts(primary, slide);
        let previous = sequence.last().copied();
        let body_page = index > 0 && index + 1 < plan.slides.len();
        let within_limit =
            |layout: StableLayoutKind,
             counts: &std::collections::HashMap<StableLayoutKind, usize>| {
                counts.get(&layout).copied().unwrap_or(0)
                    < stable_layout_limit(layout, plan.slides.len())
            };

        let selected = if !body_page {
            primary
        } else {
            candidates
                .iter()
                .copied()
                .find(|layout| {
                    previous != Some(*layout)
                        && !used_body.contains(layout)
                        && within_limit(*layout, &counts)
                        && stable_layout_is_available(*layout, chart_patterns)
                })
                .or_else(|| {
                    candidates.iter().copied().find(|layout| {
                        previous != Some(*layout)
                            && within_limit(*layout, &counts)
                            && stable_layout_is_available(*layout, chart_patterns)
                    })
                })
                .or_else(|| {
                    candidates.iter().copied().find(|layout| {
                        previous != Some(*layout)
                            && stable_layout_is_available(*layout, chart_patterns)
                    })
                })
                .unwrap_or(primary)
        };

        if body_page {
            *counts.entry(selected).or_insert(0) += 1;
            used_body.insert(selected);
        }
        sequence.push(selected);
    }
    sequence
}

fn stable_semantic_layout(
    slide: &Slide,
    index: usize,
    total: usize,
    deck_title: &str,
    chart_patterns: &std::collections::HashSet<String>,
) -> StableLayoutKind {
    if index == 0 {
        let is_literal_cover = slide.title.trim() == deck_title.trim()
            || (slide.slide_type == "cover" && slide_blocks(slide).len() <= 1);
        return if is_literal_cover {
            StableLayoutKind::Anchor
        } else {
            StableLayoutKind::EditorialSplit
        };
    }
    if index + 1 == total {
        return StableLayoutKind::Summary;
    }
    let relation = slide.relation.trim().to_ascii_lowercase();
    let chart = slide.chart_type.trim().to_ascii_lowercase();
    let layout = slide.layout.trim().to_ascii_lowercase();
    let signals = [relation.as_str(), chart.as_str(), layout.as_str()];
    if stable_layout_matches_signal(StableLayoutKind::Timeline, &signals)
        && stable_layout_is_available(StableLayoutKind::Timeline, chart_patterns)
    {
        StableLayoutKind::Timeline
    } else if stable_layout_matches_signal(StableLayoutKind::Comparison, &signals) {
        StableLayoutKind::Comparison
    } else if stable_layout_matches_signal(StableLayoutKind::CauseEffect, &signals)
        && stable_layout_is_available(StableLayoutKind::CauseEffect, chart_patterns)
    {
        StableLayoutKind::CauseEffect
    } else if stable_layout_matches_signal(StableLayoutKind::Process, &signals)
        && stable_layout_is_available(StableLayoutKind::Process, chart_patterns)
    {
        StableLayoutKind::Process
    } else if stable_layout_matches_signal(StableLayoutKind::Matrix, &signals)
        && stable_layout_is_available(StableLayoutKind::Matrix, chart_patterns)
    {
        StableLayoutKind::Matrix
    } else if stable_layout_matches_signal(StableLayoutKind::Hierarchy, &signals)
        && stable_layout_is_available(StableLayoutKind::Hierarchy, chart_patterns)
    {
        StableLayoutKind::Hierarchy
    } else if stable_layout_matches_signal(StableLayoutKind::EvidenceLed, &signals) {
        StableLayoutKind::EvidenceLed
    } else if stable_layout_matches_signal(StableLayoutKind::CategoryGrid, &signals)
        && stable_layout_is_available(StableLayoutKind::CategoryGrid, chart_patterns)
    {
        StableLayoutKind::CategoryGrid
    } else if slide.density == "breathing" && slide_blocks(slide).len() <= 2 {
        StableLayoutKind::Quote
    } else if slide_blocks(slide).len() <= 3 {
        StableLayoutKind::EditorialSplit
    } else {
        StableLayoutKind::EvidenceLed
    }
}

fn stable_fallback_layout(
    plan: &SlidePlan,
    page_index: usize,
    slide: &Slide,
    current: StableLayoutKind,
) -> StableLayoutKind {
    if matches!(
        current,
        StableLayoutKind::Anchor | StableLayoutKind::Summary
    ) {
        return current;
    }
    let chart_patterns = std::collections::HashSet::new();
    let planned = stable_layout_sequence(plan, &chart_patterns);
    let previous = page_index
        .checked_sub(1)
        .and_then(|index| planned.get(index))
        .copied();
    let next = planned.get(page_index + 1).copied();
    let mut counts = std::collections::HashMap::<StableLayoutKind, usize>::new();
    for (index, layout) in planned.iter().copied().enumerate() {
        if index != page_index && index > 0 && index + 1 < plan.slides.len() {
            *counts.entry(layout).or_insert(0) += 1;
        }
    }
    stable_compatible_layouts(current, slide)
        .into_iter()
        .filter(|layout| *layout != current)
        .find(|layout| {
            previous != Some(*layout)
                && next != Some(*layout)
                && counts.get(layout).copied().unwrap_or(0)
                    < stable_layout_limit(*layout, plan.slides.len())
        })
        .unwrap_or(current)
}

fn render_stable_layout(
    plan: &SlidePlan,
    slide: &Slide,
    profile: &StableRenderProfile,
    layout: StableLayoutKind,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    match layout {
        StableLayoutKind::Anchor => render_stable_anchor(plan, slide, profile, motif, detail_level),
        StableLayoutKind::EditorialSplit => {
            render_stable_editorial_split(slide, profile, motif, detail_level)
        }
        StableLayoutKind::Timeline => render_stable_timeline(slide, profile, motif, detail_level),
        StableLayoutKind::CategoryGrid => {
            render_stable_category_grid(slide, profile, motif, detail_level)
        }
        StableLayoutKind::Comparison => {
            render_stable_comparison(slide, profile, motif, detail_level)
        }
        StableLayoutKind::CauseEffect => {
            render_stable_cause_effect(slide, profile, motif, detail_level)
        }
        StableLayoutKind::Process => render_stable_process(slide, profile, motif, detail_level),
        StableLayoutKind::Hierarchy => render_stable_hierarchy(slide, profile, motif, detail_level),
        StableLayoutKind::Matrix => render_stable_matrix(slide, profile, motif, detail_level),
        StableLayoutKind::Quote => render_stable_quote(slide, profile, motif, detail_level),
        StableLayoutKind::EvidenceLed => {
            render_stable_evidence_led(slide, profile, motif, detail_level)
        }
        StableLayoutKind::Summary => render_stable_summary(slide, profile, motif, detail_level),
    }
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x2E80..=0x2EFF
            | 0x3000..=0x303F
            | 0xFF00..=0xFFEF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
    )
}

fn stable_char_width(ch: char, font_size: f32, weight: &str) -> f32 {
    let mut width = if is_cjk_char(ch) {
        font_size
    } else if ch == ' ' {
        font_size * 0.3
    } else if "mMwWOQ%".contains(ch) {
        font_size * 0.75
    } else if "iIlj!|".contains(ch) {
        font_size * 0.3
    } else {
        font_size * 0.55
    };
    if matches!(weight, "bold" | "600" | "700" | "800" | "900") && !is_cjk_char(ch) {
        width *= 1.05;
    }
    width
}

fn estimate_stable_text_width(text: &str, font_size: f32, weight: &str) -> f32 {
    text.chars()
        .map(|ch| stable_char_width(ch, font_size, weight))
        .sum::<f32>()
        * 1.02
}

fn is_line_start_forbidden(ch: char) -> bool {
    "，。！？；：、）》】」』…,.!?;:)]}".contains(ch)
}

fn is_line_end_forbidden(ch: char) -> bool {
    "《【（「『([{".contains(ch)
}

fn wrap_text_to_width(text: &str, font_size: f32, max_width: f32, weight: &str) -> Vec<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return vec![String::new()];
    }
    let chars: Vec<char> = normalized.chars().collect();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0.0;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        let ch_width = stable_char_width(ch, font_size, weight);
        if current_width + ch_width <= max_width || current.is_empty() {
            current.push(ch);
            current_width += ch_width;
            index += 1;
            continue;
        }

        if is_line_start_forbidden(ch) && current_width + ch_width <= max_width * 1.08 {
            current.push(ch);
            index += 1;
        }

        if current.ends_with(' ') {
            current.pop();
        }
        if let Some(last) = current.chars().last() {
            if is_line_end_forbidden(last) && current.chars().count() > 1 {
                current.pop();
                index = index.saturating_sub(1);
            }
        }
        if !current.trim().is_empty() {
            lines.push(current.trim().to_string());
        }
        current.clear();
        current_width = 0.0;
    }
    if !current.trim().is_empty() {
        lines.push(current.trim().to_string());
    }
    if lines.is_empty() {
        vec![normalized]
    } else {
        lines
    }
}

fn stable_heading_emphasis(level: usize) -> StableTextEmphasis {
    match level {
        1 => StableTextEmphasis::Heading1,
        2 => StableTextEmphasis::Heading2,
        _ => StableTextEmphasis::Heading3,
    }
}

fn stable_heading_font_scale(level: usize) -> f32 {
    match level {
        1 => 1.18,
        2 => 1.14,
        _ => 1.10,
    }
}

fn push_stable_text_run(runs: &mut Vec<StableTextRun>, run: StableTextRun) {
    if run.text.is_empty() {
        return;
    }
    if let Some(previous) = runs.last_mut() {
        if previous.bold == run.bold
            && (previous.font_scale - run.font_scale).abs() < f32::EPSILON
            && previous.emphasis == run.emphasis
        {
            previous.text.push_str(&run.text);
            return;
        }
    }
    runs.push(run);
}

fn parse_stable_text_paragraph(value: &str, policy: StableTextRenderPolicy) -> StableTextParagraph {
    let (heading_level, body) = stable_markdown_heading_prefix(value)
        .map_or((None, value.trim()), |(level, body)| (Some(level), body));
    let heading_style = heading_level
        .filter(|_| policy.allow_heading_scale)
        .map(stable_heading_emphasis)
        .unwrap_or(StableTextEmphasis::Normal);
    let heading_scale = heading_level
        .filter(|_| policy.allow_heading_scale)
        .map(stable_heading_font_scale)
        .unwrap_or(1.0);
    let heading_bold = heading_level.is_some() && policy.allow_heading_scale;
    let marker_count = body.match_indices("**").count();
    if marker_count == 0 || marker_count == 1 || !policy.allow_strong {
        return StableTextParagraph {
            runs: vec![StableTextRun {
                text: body.replace("**", ""),
                bold: heading_bold,
                font_scale: heading_scale,
                emphasis: heading_style,
            }],
        };
    }

    let parsed_body = if marker_count % 2 != 0 {
        let markers = body
            .match_indices("**")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let unmatched = (0..markers.len())
            .min_by_key(|skipped| {
                let retained = markers
                    .iter()
                    .enumerate()
                    .filter_map(|(index, marker)| (index != *skipped).then_some(*marker))
                    .collect::<Vec<_>>();
                retained
                    .chunks_exact(2)
                    .map(|pair| {
                        let length = body[pair[0] + 2..pair[1]].chars().count();
                        if length == 0 {
                            usize::MAX / 8
                        } else {
                            length
                        }
                    })
                    .sum::<usize>()
            })
            .unwrap_or(0);
        let mut repaired = body.to_string();
        repaired.replace_range(markers[unmatched]..markers[unmatched] + 2, "");
        repaired
    } else {
        body.to_string()
    };

    let mut runs = Vec::new();
    let mut remainder = parsed_body.as_str();
    let mut strong = false;
    while let Some(index) = remainder.find("**") {
        let text = &remainder[..index];
        push_stable_text_run(
            &mut runs,
            StableTextRun {
                text: text.to_string(),
                bold: heading_bold || strong,
                font_scale: heading_scale,
                emphasis: if heading_bold {
                    heading_style
                } else if strong {
                    StableTextEmphasis::Strong
                } else {
                    StableTextEmphasis::Normal
                },
            },
        );
        strong = !strong;
        remainder = &remainder[index + 2..];
    }
    push_stable_text_run(
        &mut runs,
        StableTextRun {
            text: remainder.to_string(),
            bold: heading_bold || strong,
            font_scale: heading_scale,
            emphasis: if heading_bold {
                heading_style
            } else if strong {
                StableTextEmphasis::Strong
            } else {
                StableTextEmphasis::Normal
            },
        },
    );
    StableTextParagraph { runs }
}

fn parse_stable_rich_text(value: &str, policy: StableTextRenderPolicy) -> Vec<StableTextParagraph> {
    let mut paragraphs = value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse_stable_text_paragraph(line, policy))
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        paragraphs.push(StableTextParagraph {
            runs: vec![StableTextRun {
                text: String::new(),
                bold: false,
                font_scale: 1.0,
                emphasis: StableTextEmphasis::Normal,
            }],
        });
    }
    paragraphs
}

fn stable_plain_text(value: &str) -> String {
    parse_stable_rich_text(
        value,
        StableTextRenderPolicy {
            allow_strong: false,
            allow_heading_scale: false,
        },
    )
    .iter()
    .map(|paragraph| {
        paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[derive(Debug, Clone, Copy)]
struct StableStyledChar {
    ch: char,
    bold: bool,
    font_scale: f32,
    emphasis: StableTextEmphasis,
}

fn stable_paragraph_chars(paragraph: &StableTextParagraph) -> Vec<StableStyledChar> {
    let mut chars = Vec::new();
    let mut previous_was_space = true;
    for run in &paragraph.runs {
        for ch in run.text.chars() {
            if ch.is_whitespace() {
                if previous_was_space {
                    continue;
                }
                chars.push(StableStyledChar {
                    ch: ' ',
                    bold: run.bold,
                    font_scale: run.font_scale,
                    emphasis: run.emphasis,
                });
                previous_was_space = true;
            } else {
                chars.push(StableStyledChar {
                    ch,
                    bold: run.bold,
                    font_scale: run.font_scale,
                    emphasis: run.emphasis,
                });
                previous_was_space = false;
            }
        }
    }
    while chars.last().is_some_and(|value| value.ch == ' ') {
        chars.pop();
    }
    chars
}

fn stable_styled_char_width(value: StableStyledChar, font_size: f32, weight: &str) -> f32 {
    stable_char_width(
        value.ch,
        font_size * value.font_scale,
        if value.bold { "700" } else { weight },
    )
}

fn stable_chars_to_text_line(chars: &[StableStyledChar]) -> StableTextLine {
    let mut runs = Vec::new();
    for value in chars {
        push_stable_text_run(
            &mut runs,
            StableTextRun {
                text: value.ch.to_string(),
                bold: value.bold,
                font_scale: value.font_scale,
                emphasis: value.emphasis,
            },
        );
    }
    StableTextLine { runs }
}

fn wrap_stable_rich_text(
    paragraphs: &[StableTextParagraph],
    font_size: f32,
    max_width: f32,
    weight: &str,
) -> Vec<StableTextLine> {
    let mut output = Vec::new();
    for paragraph in paragraphs {
        let chars = stable_paragraph_chars(paragraph);
        if chars.is_empty() {
            output.push(StableTextLine { runs: Vec::new() });
            continue;
        }
        let mut current = Vec::new();
        let mut current_width = 0.0;
        let mut index = 0;
        while index < chars.len() {
            let value = chars[index];
            let width = stable_styled_char_width(value, font_size, weight);
            if current_width + width <= max_width || current.is_empty() {
                current.push(value);
                current_width += width;
                index += 1;
                continue;
            }
            if is_line_start_forbidden(value.ch) && current_width + width <= max_width * 1.08 {
                current.push(value);
                index += 1;
            }
            while current.last().is_some_and(|value| value.ch == ' ') {
                current.pop();
            }
            let mut moved_prefix = Vec::new();
            if value.ch.is_ascii_digit()
                && current
                    .last()
                    .is_some_and(|value| value.ch.is_ascii_digit())
            {
                while current
                    .last()
                    .is_some_and(|value| value.ch.is_ascii_digit())
                {
                    if let Some(moved) = current.pop() {
                        moved_prefix.insert(0, moved);
                    }
                }
                if current.is_empty() {
                    current.append(&mut moved_prefix);
                }
            } else if current.len() > 1
                && current
                    .last()
                    .is_some_and(|value| is_line_end_forbidden(value.ch))
            {
                if let Some(moved) = current.pop() {
                    moved_prefix.push(moved);
                }
            }
            if !current.is_empty() {
                output.push(stable_chars_to_text_line(&current));
            }
            current = moved_prefix;
            current_width = current
                .iter()
                .map(|value| stable_styled_char_width(*value, font_size, weight))
                .sum();
        }
        while current.last().is_some_and(|value| value.ch == ' ') {
            current.pop();
        }
        if !current.is_empty() {
            output.push(stable_chars_to_text_line(&current));
        }
    }
    if output.is_empty() {
        vec![StableTextLine { runs: Vec::new() }]
    } else {
        output
    }
}

fn stable_rich_line_width(line: &StableTextLine, font_size: f32, weight: &str) -> f32 {
    line.runs
        .iter()
        .map(|run| {
            estimate_stable_text_width(
                &run.text,
                font_size * run.font_scale,
                if run.bold { "700" } else { weight },
            )
        })
        .sum()
}

fn truncate_stable_rich_line(
    line: &mut StableTextLine,
    font_size: f32,
    box_width: f32,
    weight: &str,
) {
    let fallback = StableTextRun {
        text: String::new(),
        bold: false,
        font_scale: 1.0,
        emphasis: StableTextEmphasis::Normal,
    };
    let ellipsis_style = line.runs.last().cloned().unwrap_or(fallback);
    loop {
        let mut candidate = line.clone();
        push_stable_text_run(
            &mut candidate.runs,
            StableTextRun {
                text: "…".to_string(),
                ..ellipsis_style.clone()
            },
        );
        if stable_rich_line_width(&candidate, font_size, weight) <= box_width
            || line.runs.is_empty()
        {
            *line = candidate;
            break;
        }
        if let Some(last) = line.runs.last_mut() {
            last.text.pop();
            if last.text.is_empty() {
                line.runs.pop();
            }
        }
    }
}

fn fit_stable_rich_text_box(
    text: &str,
    box_width: f32,
    box_height: f32,
    preferred_font_size: f32,
    min_font_size: f32,
    line_height_ratio: f32,
    weight: &str,
    policy: StableTextRenderPolicy,
) -> StableTextFit {
    let paragraphs = parse_stable_rich_text(text, policy);
    let max_font_scale = paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.runs.iter())
        .map(|run| run.font_scale)
        .fold(1.0, f32::max);
    let mut size = preferred_font_size;
    while size >= min_font_size {
        let rich_lines = wrap_stable_rich_text(&paragraphs, size, box_width, weight);
        let line_height = (size * max_font_scale * line_height_ratio).ceil();
        let used_height = rich_lines.len() as f32 * line_height;
        if used_height <= box_height + 0.5 {
            let max_line_width = rich_lines
                .iter()
                .map(|line| stable_rich_line_width(line, size, weight))
                .fold(0.0, f32::max);
            let lines = rich_lines.iter().map(StableTextLine::plain_text).collect();
            return StableTextFit {
                lines,
                rich_lines,
                font_size: size,
                line_height,
                used_height,
                max_line_width,
                required_width: max_line_width,
                required_height: used_height,
                overflowed: false,
            };
        }
        size -= 1.0;
    }

    let size = min_font_size;
    let line_height = (size * max_font_scale * line_height_ratio).ceil();
    let mut rich_lines = wrap_stable_rich_text(&paragraphs, size, box_width, weight);
    let required_height = rich_lines.len() as f32 * line_height;
    let required_width = rich_lines
        .iter()
        .map(|line| stable_rich_line_width(line, size, weight))
        .fold(0.0, f32::max);
    let max_lines = ((box_height / line_height).floor() as usize).max(1);
    let overflowed = rich_lines.len() > max_lines;
    rich_lines.truncate(max_lines);
    if overflowed {
        if let Some(last) = rich_lines.last_mut() {
            truncate_stable_rich_line(last, size, box_width, weight);
        }
    }
    let max_line_width = rich_lines
        .iter()
        .map(|line| stable_rich_line_width(line, size, weight))
        .fold(0.0, f32::max);
    let lines = rich_lines.iter().map(StableTextLine::plain_text).collect();
    StableTextFit {
        used_height: rich_lines.len() as f32 * line_height,
        lines,
        rich_lines,
        font_size: size,
        line_height,
        max_line_width,
        required_width,
        required_height,
        overflowed,
    }
}

#[cfg(test)]
fn fit_text_box(
    text: &str,
    box_width: f32,
    box_height: f32,
    preferred_font_size: f32,
    min_font_size: f32,
    line_height_ratio: f32,
    weight: &str,
) -> StableTextFit {
    fit_stable_rich_text_box(
        text,
        box_width,
        box_height,
        preferred_font_size,
        min_font_size,
        line_height_ratio,
        weight,
        StableTextRenderPolicy {
            allow_strong: true,
            allow_heading_scale: true,
        },
    )
}

fn append_fitted_text(
    draft: &mut StablePageDraft,
    id: &str,
    text: &str,
    rect: StableRect,
    preferred_font_size: f32,
    min_font_size: f32,
    line_height_ratio: f32,
    fill: &str,
    weight: &str,
    align: &str,
    vertical_center: bool,
    kind: StableElementKind,
    container: Option<StableRect>,
) -> StableTextFit {
    let fit = fit_stable_rich_text_box(
        text,
        rect.width,
        rect.height,
        preferred_font_size,
        min_font_size,
        line_height_ratio,
        weight,
        StableTextRenderPolicy::for_element(id, kind),
    );
    debug_assert_eq!(fit.lines.len(), fit.rich_lines.len());
    let top = if vertical_center {
        rect.y + ((rect.height - fit.used_height) / 2.0).max(0.0)
    } else {
        rect.y
    };
    let baseline = top + fit.font_size * 0.84;
    let x = if align == "middle" {
        rect.x + rect.width / 2.0
    } else if align == "end" {
        rect.right()
    } else {
        rect.x
    };
    let text_anchor = if align == "middle" {
        " text-anchor=\"middle\""
    } else if align == "end" {
        " text-anchor=\"end\""
    } else {
        ""
    };
    let mut tspans = String::new();
    for (idx, line) in fit.rich_lines.iter().enumerate() {
        let mut inline_runs = String::new();
        for run in &line.runs {
            let mut attributes = String::new();
            if run.bold {
                attributes.push_str(" font-weight=\"700\"");
            }
            if (run.font_scale - 1.0).abs() > f32::EPSILON {
                attributes.push_str(&format!(
                    " font-size=\"{:.1}\"",
                    fit.font_size * run.font_scale
                ));
            }
            if let Some(level) = run.emphasis.heading_level() {
                attributes.push_str(&format!(" data-stable-heading-level=\"{}\"", level));
            }
            inline_runs.push_str(&format!(
                "<tspan{}>{}</tspan>",
                attributes,
                xml_escape(&run.text)
            ));
        }
        tspans.push_str(&format!(
            "<tspan x=\"{:.1}\" dy=\"{}\">{}</tspan>",
            x,
            if idx == 0 { 0.0 } else { fit.line_height },
            inline_runs
        ));
    }
    draft.body.push_str(&format!(
        "<text id=\"{}\" x=\"{:.1}\" y=\"{:.1}\"{} font-family=\"{}\" font-size=\"{:.1}\" font-weight=\"{}\" fill=\"{}\" data-paragraph-line-height=\"{:.1}\">{}</text>\n",
        id,
        x,
        baseline,
        text_anchor,
        "Microsoft YaHei, Arial, sans-serif",
        fit.font_size,
        weight,
        fill,
        fit.line_height,
        tspans
    ));
    let actual_x = if align == "middle" {
        x - fit.max_line_width / 2.0
    } else if align == "end" {
        x - fit.max_line_width
    } else {
        x
    };
    draft.elements.push(StableLayoutElement {
        id: id.to_string(),
        rect: StableRect {
            x: actual_x,
            y: top,
            width: fit.max_line_width.min(rect.width),
            height: fit.used_height,
        },
        kind,
        container,
    });
    draft.text_boxes.push(StableTextBoxRecord {
        id: id.to_string(),
        requested_rect: rect,
        fit: fit.clone(),
    });
    fit
}

fn append_single_line_centered(
    draft: &mut StablePageDraft,
    id: &str,
    text: &str,
    center_x: f32,
    center_y: f32,
    font_size: f32,
    fill: &str,
    weight: &str,
) {
    let width = estimate_stable_text_width(text, font_size, weight);
    let baseline = center_y + font_size * 0.34;
    draft.body.push_str(&format!(
        "<text id=\"{}\" x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"Microsoft YaHei, Arial, sans-serif\" font-size=\"{:.1}\" font-weight=\"{}\" fill=\"{}\">{}</text>\n",
        id,
        center_x,
        baseline,
        font_size,
        weight,
        fill,
        xml_escape(text)
    ));
    draft.elements.push(StableLayoutElement {
        id: id.to_string(),
        rect: StableRect {
            x: center_x - width / 2.0,
            y: center_y - font_size * 0.5,
            width,
            height: font_size,
        },
        kind: StableElementKind::Text,
        container: None,
    });
}

fn append_standard_header(
    draft: &mut StablePageDraft,
    slide: &Slide,
    tokens: &StableVisualTokens,
    show_core: bool,
    repair: Option<&StableLocalRepairPlan>,
) {
    let repair_title = repair.is_some_and(|plan| {
        plan.level == StableRepairLevel::TextBox && plan.text_role.as_deref() == Some("title")
    });
    let repair_core = repair.is_some_and(|plan| {
        matches!(
            plan.level,
            StableRepairLevel::TextBox | StableRepairLevel::ContentBlock
        ) && plan.text_role.as_deref() == Some("coreMessage")
    });
    let title_fit = append_fitted_text(
        draft,
        "header-title",
        &slide.title,
        StableRect {
            x: 56.0,
            y: 42.0,
            width: 930.0,
            height: 48.0,
        },
        if repair_title { 34.0 } else { 36.0 },
        if repair_title { 28.0 } else { 32.0 },
        if repair_title { 1.08 } else { 1.16 },
        &tokens.text,
        "700",
        "start",
        false,
        StableElementKind::Header,
        None,
    );
    if slide.title.trim().is_empty() || title_fit.overflowed {
        draft
            .hard_failures
            .push("required page title cannot be rendered completely".to_string());
    }
    let supporting = if show_core {
        stable_core_message(slide)
    } else if slide.subtitle.trim().is_empty() {
        slide.page_theme.clone()
    } else {
        slide.subtitle.clone()
    };
    let support_fit = append_fitted_text(
        draft,
        "header-support",
        &supporting,
        StableRect {
            x: 58.0,
            y: 96.0,
            width: 1080.0,
            height: if repair_core { 56.0 } else { 48.0 },
        },
        if repair_core { 17.0 } else { 18.0 },
        if repair_core { 14.0 } else { 16.0 },
        if repair_core { 1.16 } else { 1.35 },
        &tokens.muted,
        "400",
        "start",
        false,
        StableElementKind::Header,
        None,
    );
    if show_core && (supporting.trim().is_empty() || support_fit.overflowed) {
        draft
            .hard_failures
            .push("required coreMessage cannot be rendered completely".to_string());
    } else if !show_core && support_fit.overflowed {
        draft
            .warnings
            .push("header subtitle shortened severity=warning".to_string());
    }
    draft.body.push_str(&format!(
        "<line x1=\"56\" y1=\"158\" x2=\"1224\" y2=\"158\" stroke=\"{}\" stroke-width=\"1.2\"/>\n<line x1=\"56\" y1=\"158\" x2=\"220\" y2=\"158\" stroke=\"{}\" stroke-width=\"4\"/>\n",
        tokens.border, tokens.primary
    ));
    draft.push_decoration(
        "header-divider",
        StableDecorationPurpose::Divider,
        stable_line_rect(56.0, 158.0, 1224.0, 158.0, 1.2),
        &["header-title"],
    );
    draft.push_decoration(
        "header-emphasis",
        StableDecorationPurpose::Emphasis,
        stable_line_rect(56.0, 158.0, 220.0, 158.0, 4.0),
        &["header-title"],
    );
}

fn render_stable_footer(
    plan: &SlidePlan,
    slide: &Slide,
    tokens: &StableVisualTokens,
    draft: &mut StablePageDraft,
    repair: Option<&StableLocalRepairPlan>,
) -> Result<String, AppError> {
    draft.body.push_str(&format!(
        "<g id=\"footer\"><line x1=\"56\" y1=\"665\" x2=\"1224\" y2=\"665\" stroke=\"{}\" stroke-width=\"1\"/>\n",
        tokens.border
    ));
    let page_label = format!("{:02} / {:02}", slide.page, plan.slides.len());
    append_fitted_text(
        draft,
        "footer-page",
        &page_label,
        StableRect {
            x: 56.0,
            y: 678.0,
            width: 100.0,
            height: 20.0,
        },
        12.0,
        11.0,
        1.1,
        &tokens.muted,
        "500",
        "start",
        true,
        StableElementKind::Footer,
        None,
    );
    let title = shorten_to_width(&stable_plain_text(&plan.title), 12.0, 264.0, "400");
    append_fitted_text(
        draft,
        "footer-title",
        &title,
        StableRect {
            x: 960.0,
            y: 678.0,
            width: 264.0,
            height: 20.0,
        },
        12.0,
        11.0,
        1.1,
        &tokens.muted,
        "400",
        "end",
        true,
        StableElementKind::Footer,
        None,
    );
    let mut footer_ids = vec!["footer-page", "footer-title"];
    let suppress_evidence = repair.is_some_and(|plan| {
        plan.failure_type == StableFailureType::FooterCollision
            && matches!(
                plan.level,
                StableRepairLevel::TextBox | StableRepairLevel::ContentBlock
            )
    });
    if suppress_evidence {
        if let Some(evidence) = slide
            .evidence
            .iter()
            .find_map(|value| stable_footer_source_label(value))
        {
            draft.push_degradation(
                "footer",
                "footer annotation",
                "annotation",
                "omitted_to_speaker_notes",
                &evidence,
            );
        }
    } else if let Some(source) = slide
        .evidence
        .iter()
        .find_map(|value| stable_footer_source_label(value))
    {
        let note = shorten_to_width(&source, 11.0, 590.0, "400");
        append_fitted_text(
            draft,
            "footer-evidence",
            &format!("Source · {}", note),
            StableRect {
                x: 210.0,
                y: 679.0,
                width: 620.0,
                height: 18.0,
            },
            11.0,
            10.0,
            1.1,
            &tokens.muted,
            "400",
            "middle",
            true,
            StableElementKind::Footer,
            None,
        );
        footer_ids.push("footer-evidence");
    }
    draft.body.push_str("</g>\n");
    draft.push_decoration(
        "footer-divider",
        StableDecorationPurpose::Divider,
        stable_line_rect(56.0, 665.0, 1224.0, 665.0, 1.0),
        &footer_ids,
    );
    Ok(String::new())
}

fn stable_footer_source_label(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let lower = trimmed.to_ascii_lowercase();
    let source = if lower.starts_with("source:") {
        trimmed.get(7..).unwrap_or_default()
    } else if trimmed.starts_with("来源：") {
        trimmed.get("来源：".len()..).unwrap_or_default()
    } else if trimmed.starts_with("来源:") {
        trimmed.get("来源:".len()..).unwrap_or_default()
    } else if trimmed.starts_with("资料来源：") {
        trimmed.get("资料来源：".len()..).unwrap_or_default()
    } else if trimmed.starts_with("资料来源:") {
        trimmed.get("资料来源:".len()..).unwrap_or_default()
    } else {
        return None;
    };
    let source = stable_plain_text(&sanitize_visible_text(source));
    if source.trim().is_empty() || source.chars().count() > 72 {
        None
    } else {
        Some(source)
    }
}

fn shorten_to_width(text: &str, font_size: f32, max_width: f32, weight: &str) -> String {
    if estimate_stable_text_width(text, font_size, weight) <= max_width {
        return text.trim().to_string();
    }
    let mut out = String::new();
    for ch in text.trim().chars() {
        let candidate = format!("{}{}…", out, ch);
        if estimate_stable_text_width(&candidate, font_size, weight) > max_width {
            break;
        }
        out.push(ch);
    }
    if !out.ends_with('…') {
        out.push('…');
    }
    out
}

fn validate_slide_layout(elements: &[StableLayoutElement]) -> Vec<String> {
    let mut problems = Vec::new();
    let canvas = StableRect {
        x: 0.0,
        y: 0.0,
        width: STABLE_CANVAS_WIDTH,
        height: STABLE_CANVAS_HEIGHT,
    };
    for element in elements {
        if !canvas.contains(element.rect, 0.75) {
            problems.push(format!("{} out of canvas", element.id));
        }
        if let Some(container) = element.container {
            if !container.inset(8.0).contains(element.rect, 1.5) {
                problems.push(format!("{} overflows container", element.id));
            }
        }
        if element.kind == StableElementKind::Card
            && !element.id.starts_with("anchor-")
            && (element.rect.x < STABLE_SAFE_LEFT - 1.0
                || element.rect.right() > STABLE_SAFE_RIGHT + 1.0
                || element.rect.y < STABLE_CONTENT_TOP - 1.0
                || element.rect.bottom() > STABLE_CONTENT_BOTTOM + 1.0)
        {
            problems.push(format!("{} outside safe content area", element.id));
        }
    }
    for left_index in 0..elements.len() {
        for right_index in (left_index + 1)..elements.len() {
            let left = &elements[left_index];
            let right = &elements[right_index];
            if left.kind == StableElementKind::Card
                && right.kind == StableElementKind::Card
                && left.rect.overlaps(right.rect, 1.5)
            {
                problems.push(format!("{} overlaps {}", left.id, right.id));
            }
            if matches!(
                left.kind,
                StableElementKind::Text | StableElementKind::Header
            ) && matches!(
                right.kind,
                StableElementKind::Text | StableElementKind::Header
            ) && left.rect.overlaps(right.rect, 2.0)
            {
                problems.push(format!("text {} overlaps {}", left.id, right.id));
            }
            if left.kind == StableElementKind::Footer
                && matches!(
                    right.kind,
                    StableElementKind::Text | StableElementKind::Card
                )
                && left.rect.overlaps(right.rect, 1.0)
            {
                problems.push(format!("footer {} overlaps {}", left.id, right.id));
            }
            if right.kind == StableElementKind::Footer
                && matches!(left.kind, StableElementKind::Text | StableElementKind::Card)
                && right.rect.overlaps(left.rect, 1.0)
            {
                problems.push(format!("footer {} overlaps {}", right.id, left.id));
            }
        }
    }
    problems.sort();
    problems.dedup();
    problems
}

fn stable_text_role_from_id(id: &str) -> Option<String> {
    if matches!(id, "header-title" | "anchor-title") || id.ends_with("-title") {
        Some("title".to_string())
    } else if id == "header-support" || id.contains("core") || id.contains("message") {
        Some("coreMessage".to_string())
    } else if id.ends_with("-label") || id.contains("kicker") {
        Some("label".to_string())
    } else if id.ends_with("-text") || id.contains("body") {
        Some("text".to_string())
    } else if id.ends_with("-detail") {
        Some("detail".to_string())
    } else if id.contains("evidence") || id.contains("annotation") {
        Some("annotation".to_string())
    } else {
        None
    }
}

fn classify_stable_failure(message: &str) -> StableFailureType {
    let lower = message.to_ascii_lowercase();
    if lower.contains("footer") {
        StableFailureType::FooterCollision
    } else if lower.contains("decoration") {
        StableFailureType::DecorationCollision
    } else if lower.contains("out of canvas") || lower.contains("outside safe") {
        StableFailureType::OutOfSafeBounds
    } else if lower.contains("overlaps") {
        if lower.starts_with("text ") {
            StableFailureType::TextOverlap
        } else {
            StableFailureType::ContainerOverflow
        }
    } else if lower.contains("overflows container") {
        StableFailureType::ContainerOverflow
    } else if lower.contains("overflow") {
        StableFailureType::TextOverflow
    } else if lower.contains("empty")
        || lower.contains("missing")
        || lower.contains("incomplete")
        || lower.contains("not rendered")
    {
        StableFailureType::RequiredContentMissing
    } else {
        StableFailureType::MotifIncomplete
    }
}

fn stable_failure_from_problem(
    page_index: usize,
    message: &str,
    draft: &StablePageDraft,
) -> StableLayoutFailure {
    let direct_text_box = draft
        .text_boxes
        .iter()
        .find(|record| message.contains(&record.id))
        .or_else(|| {
            if message.contains("page title") {
                draft
                    .text_boxes
                    .iter()
                    .find(|record| matches!(record.id.as_str(), "header-title" | "anchor-title"))
            } else if message.contains("coreMessage") || message.contains("core overflow") {
                draft.text_boxes.iter().find(|record| {
                    record.id == "header-support"
                        || record.id.contains("core")
                        || record.id.contains("message")
                })
            } else {
                None
            }
        });
    let block_id = draft
        .rendered_blocks
        .iter()
        .find(|block| {
            message.contains(&block.id)
                || direct_text_box.is_some_and(|record| record.id.starts_with(&block.id))
        })
        .map(|block| block.id.clone());
    let text_role = direct_text_box
        .and_then(|record| stable_text_role_from_id(&record.id))
        .or_else(|| {
            let lower = message.to_ascii_lowercase();
            if lower.contains("title") {
                Some("title".to_string())
            } else if lower.contains("core") || lower.contains("message") {
                Some("coreMessage".to_string())
            } else if lower.contains("label") {
                Some("label".to_string())
            } else if lower.contains("body") || lower.contains("text") {
                Some("text".to_string())
            } else {
                None
            }
        });
    let text_box = direct_text_box.or_else(|| {
        let block_id = block_id.as_deref()?;
        let role = text_role.as_deref()?;
        let suffix = match role {
            "label" => "label",
            "text" => "text",
            "detail" => "detail",
            "annotation" => "evidence",
            _ => return None,
        };
        let expected_id = format!("{}-{}", block_id, suffix);
        draft
            .text_boxes
            .iter()
            .find(|record| record.id == expected_id)
    });
    let failure_type = if text_box.is_some_and(|record| record.fit.overflowed) {
        StableFailureType::TextOverflow
    } else {
        classify_stable_failure(message)
    };
    StableLayoutFailure {
        page_index,
        block_id,
        text_role,
        failure_type,
        bounds: text_box.map(|record| record.requested_rect).or_else(|| {
            draft
                .rendered_blocks
                .iter()
                .find(|block| message.contains(&block.id))
                .map(|block| block.rect)
        }),
        required_width: text_box.map(|record| record.fit.required_width),
        required_height: text_box.map(|record| record.fit.required_height),
        attempted_strategy: Vec::new(),
        message: message.to_string(),
    }
}

fn collect_stable_layout_failures(
    page_index: usize,
    problems: &[String],
    draft: &StablePageDraft,
) -> Vec<StableLayoutFailure> {
    let mut failures = Vec::new();
    for problem in problems {
        let failure = stable_failure_from_problem(page_index, problem, draft);
        if failures.iter().any(|existing: &StableLayoutFailure| {
            existing.failure_type == failure.failure_type
                && existing.block_id == failure.block_id
                && existing.text_role == failure.text_role
                && existing.message == failure.message
        }) {
            continue;
        }
        failures.push(failure);
    }
    failures
}

fn primary_stable_layout_failure(failures: &[StableLayoutFailure]) -> Option<StableLayoutFailure> {
    failures
        .iter()
        .min_by_key(|failure| match failure.failure_type {
            StableFailureType::RequiredContentMissing => 0,
            StableFailureType::TextOverflow => 1,
            StableFailureType::ContainerOverflow => 2,
            StableFailureType::TextOverlap => 3,
            StableFailureType::FooterCollision => 4,
            StableFailureType::OutOfSafeBounds => 5,
            StableFailureType::DecorationCollision => 6,
            StableFailureType::MotifIncomplete => 7,
        })
        .cloned()
}

fn validate_motif_completeness(
    slide: &Slide,
    layout: StableLayoutKind,
    motif: StableMotif,
    _detail_level: StableDetailLevel,
    draft: &StablePageDraft,
) -> Vec<String> {
    let mut problems = Vec::new();
    if slide.title.trim().is_empty() {
        problems.push("required page title is empty".to_string());
    }
    if stable_core_message(slide).trim().is_empty() {
        problems.push("required coreMessage is empty".to_string());
    }
    let title_rendered = draft
        .elements
        .iter()
        .any(|element| matches!(element.id.as_str(), "header-title" | "anchor-title"));
    if !title_rendered {
        problems.push("required page title was not rendered".to_string());
    }
    let core_rendered = draft.elements.iter().any(|element| {
        element.id == "header-support"
            || element.id.contains("core")
            || element.id.contains("message")
    });
    if !core_rendered {
        problems.push("required coreMessage was not rendered".to_string());
    }
    let expected = if layout == StableLayoutKind::Anchor {
        0
    } else {
        slide_blocks(slide)
            .len()
            .min(stable_motif_requirements(motif).max_blocks)
    };
    if draft.rendered_blocks.len() < expected {
        problems.push(format!(
            "motif {} rendered {}/{} content blocks",
            motif.as_str(),
            draft.rendered_blocks.len(),
            expected
        ));
    }
    for block in &draft.rendered_blocks {
        let label_rendered = draft
            .elements
            .iter()
            .any(|element| element.id == format!("{}-label", block.id));
        let text_rendered = draft
            .elements
            .iter()
            .any(|element| element.id == format!("{}-text", block.id));
        if !block.label_complete || !label_rendered {
            problems.push(format!("{} label is incomplete", block.id));
        }
        if !block.text_complete || !text_rendered {
            problems.push(format!("{} body is incomplete", block.id));
        }
    }
    problems
}

fn container_content_ratio(draft: &StablePageDraft, block: &StableRenderedBlock) -> f32 {
    let mut content_bounds: Option<StableRect> = None;
    for element in &draft.elements {
        if element.kind == StableElementKind::Text && element.id.starts_with(&block.id) {
            content_bounds = Some(match content_bounds {
                Some(bounds) => bounds.union(element.rect),
                None => element.rect,
            });
        }
    }
    content_bounds
        .map(|bounds| bounds.area() / block.rect.area().max(1.0))
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn validate_visual_fullness(slide: &Slide, draft: &StablePageDraft) -> Vec<String> {
    let safe_area =
        (STABLE_SAFE_RIGHT - STABLE_SAFE_LEFT) * (STABLE_CONTENT_BOTTOM - STABLE_CONTENT_TOP);
    let mut semantic_area = 0.0;
    for block in &draft.rendered_blocks {
        let utilization = container_content_ratio(draft, block);
        semantic_area += block.rect.area() * (utilization * 2.2).clamp(0.08, 0.82);
    }
    for element in &draft.elements {
        if element.rect.y < STABLE_CONTENT_TOP || element.rect.bottom() > STABLE_CONTENT_BOTTOM {
            continue;
        }
        if element.kind == StableElementKind::Card
            && !draft
                .rendered_blocks
                .iter()
                .any(|block| block.id == element.id)
        {
            semantic_area += element.rect.area() * 0.42;
        } else if element.kind == StableElementKind::Text && element.container.is_none() {
            semantic_area += element.rect.area() * 2.8;
        }
    }
    let occupancy = (semantic_area / safe_area.max(1.0)).clamp(0.0, 1.0);
    let minimum = match stable_density(slide) {
        StableDensity::Anchor => 0.20,
        StableDensity::Breathing => 0.22,
        StableDensity::Balanced => 0.27,
        StableDensity::Dense => 0.34,
    };
    if occupancy + 0.001 < minimum {
        vec![format!(
            "visual occupancy too low ratio={:.2} required={:.2}",
            occupancy, minimum
        )]
    } else {
        Vec::new()
    }
}

fn validate_semantic_decorations(draft: &StablePageDraft) -> Vec<String> {
    let mut problems = Vec::new();
    for decoration in &draft.decorations {
        let minimum_links = match decoration.purpose {
            StableDecorationPurpose::Connector | StableDecorationPurpose::TimelineAxis => 2,
            _ => 1,
        };
        if decoration.associated_ids.len() < minimum_links {
            problems.push(format!(
                "decoration {} has no semantic attachment",
                decoration.id
            ));
        }
        for associated in &decoration.associated_ids {
            let exists = draft.elements.iter().any(|element| {
                element.id == *associated
                    || element.id.starts_with(associated)
                    || associated.starts_with(&element.id)
            });
            if !exists {
                problems.push(format!(
                    "decoration {} references missing object {}",
                    decoration.id, associated
                ));
            }
        }
        for text in draft.elements.iter().filter(|element| {
            matches!(
                element.kind,
                StableElementKind::Text | StableElementKind::Header
            )
        }) {
            if decoration.associated_ids.iter().any(|associated| {
                text.id == *associated
                    || text.id.starts_with(associated)
                    || associated.starts_with(&text.id)
            }) {
                continue;
            }
            if decoration.rect.overlaps(text.rect, 0.5) {
                problems.push(format!(
                    "decoration {} intersects text {}",
                    decoration.id, text.id
                ));
            }
        }
    }
    problems.sort();
    problems.dedup();
    problems
}

fn stable_line_rect(x1: f32, y1: f32, x2: f32, y2: f32, stroke_width: f32) -> StableRect {
    let half = stroke_width.max(1.0) / 2.0;
    StableRect {
        x: x1.min(x2) - half,
        y: y1.min(y2) - half,
        width: (x1 - x2).abs().max(1.0) + stroke_width,
        height: (y1 - y2).abs().max(1.0) + stroke_width,
    }
}

fn preferred_card_height(block: &ContentBlock, evidence: Option<&str>, width: f32) -> f32 {
    let inner_width = (width - 40.0).max(120.0);
    let label_lines = wrap_text_to_width(&block.label, 20.0, inner_width - 48.0, "700").len();
    let text_lines = wrap_text_to_width(&block.text, 17.0, inner_width, "600").len();
    let detail_lines = wrap_text_to_width(&block.detail, 13.0, inner_width, "400")
        .len()
        .min(3);
    36.0 + label_lines as f32 * 25.0
        + text_lines as f32 * 23.0
        + detail_lines as f32 * 18.0
        + if evidence.is_some() { 28.0 } else { 0.0 }
        + 28.0
}

#[derive(Debug, Clone, Copy)]
struct StableBlockTextLayout {
    label: StableRect,
    text: StableRect,
    detail: StableRect,
    evidence: StableRect,
    label_align: &'static str,
}

fn render_stable_motif_block(
    draft: &mut StablePageDraft,
    block: &ContentBlock,
    evidence: Option<&str>,
    rect: StableRect,
    index: usize,
    tokens: &StableVisualTokens,
    motif: StableMotif,
    detail_level: StableDetailLevel,
    repair: Option<&StableLocalRepairPlan>,
    id_prefix: &str,
) {
    let accent = if index % 2 == 0 {
        &tokens.accent
    } else {
        &tokens.primary
    };
    let block_id = format!("{}-{}", id_prefix, index);
    let targets_block = repair
        .and_then(|plan| plan.block_id.as_deref())
        .is_some_and(|target| target == block_id);
    let block_reflow =
        targets_block && repair.is_some_and(|plan| plan.level == StableRepairLevel::ContentBlock);
    let mut rect = rect;
    if block_reflow {
        let expansion = 30.0;
        if rect.bottom() + expansion <= STABLE_CONTENT_BOTTOM {
            rect.height += expansion;
        } else if rect.y - expansion >= STABLE_CONTENT_TOP {
            rect.y -= expansion;
            rect.height += expansion;
        }
    }
    let evidence_visible = !block_reflow
        && (detail_level == StableDetailLevel::Full
            || (detail_level == StableDetailLevel::Reduced && index == 1))
        && rect.height >= 150.0
        && evidence
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
    let detail_visible = !block_reflow
        && detail_level != StableDetailLevel::Essential
        && rect.height >= 168.0
        && !block.detail.trim().is_empty();
    let mut layout = append_motif_chrome(
        draft,
        &block_id,
        block,
        rect,
        index,
        tokens,
        motif,
        detail_visible,
        evidence_visible,
    );
    if block_reflow {
        layout.label.height = layout.label.height.min(34.0);
        layout.text.y = layout.label.bottom() + 4.0;
        layout.text.height = (rect.bottom() - layout.text.y - 14.0).max(28.0);
        layout.detail.height = 0.0;
        layout.evidence.height = 0.0;
    }
    draft.push_rect(&block_id, rect, StableElementKind::Card);

    let repair_label = targets_block
        && repair.is_some_and(|plan| {
            matches!(
                plan.level,
                StableRepairLevel::TextBox | StableRepairLevel::ContentBlock
            ) && (plan.text_role.as_deref() == Some("label")
                || plan.level == StableRepairLevel::ContentBlock)
        });
    let repair_text = targets_block
        && repair.is_some_and(|plan| {
            matches!(
                plan.level,
                StableRepairLevel::TextBox | StableRepairLevel::ContentBlock
            ) && (plan.text_role.as_deref() == Some("text")
                || plan.level == StableRepairLevel::ContentBlock)
        });

    let label_missing = block.label.trim().is_empty();
    if label_missing {
        draft
            .hard_failures
            .push(format!("{} required label is empty", block_id));
    }
    let label_fit = append_fitted_text(
        draft,
        &format!("{}-label", block_id),
        if block.label.trim().is_empty() {
            "重点"
        } else {
            &block.label
        },
        layout.label,
        if repair_label { 18.0 } else { 20.0 },
        if repair_label { 13.0 } else { 15.0 },
        if repair_label { 1.08 } else { 1.2 },
        accent,
        "700",
        layout.label_align,
        true,
        StableElementKind::Text,
        Some(rect),
    );
    let text_missing = block.text.trim().is_empty();
    if text_missing {
        draft
            .hard_failures
            .push(format!("{} required text is empty", block_id));
    }
    let text_value = &block.text;
    let text_fit = append_fitted_text(
        draft,
        &format!("{}-text", block_id),
        text_value,
        layout.text,
        if repair_text { 16.0 } else { 18.0 },
        if repair_text { 12.0 } else { 14.0 },
        if repair_text { 1.12 } else { 1.28 },
        &tokens.text,
        "600",
        "start",
        false,
        StableElementKind::Text,
        Some(rect),
    );
    if text_fit.overflowed {
        draft
            .hard_failures
            .push(format!("{} required text overflow", block_id));
    }
    let detail_expected = !block.detail.trim().is_empty();
    if detail_visible {
        let detail_fit = append_fitted_text(
            draft,
            &format!("{}-detail", block_id),
            &block.detail,
            layout.detail,
            14.0,
            12.0,
            1.28,
            &tokens.muted,
            "400",
            "start",
            false,
            StableElementKind::Text,
            Some(rect),
        );
        if detail_fit.overflowed {
            draft.push_degradation(
                &block_id,
                &block.label,
                "detail",
                "truncated_to_fitted_lines_and_notes",
                &block.detail,
            );
        }
    } else if detail_expected {
        draft.push_degradation(
            &block_id,
            &block.label,
            "detail",
            "omitted_to_speaker_notes",
            &block.detail,
        );
    }
    let evidence_expected = evidence
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if evidence_visible {
        let evidence = evidence.unwrap_or_default();
        let tag_text = shorten_to_width(
            &stable_plain_text(evidence),
            11.0,
            layout.evidence.width,
            "400",
        );
        let evidence_fit = append_fitted_text(
            draft,
            &format!("{}-evidence", block_id),
            &tag_text,
            layout.evidence,
            11.0,
            11.0,
            1.15,
            accent,
            "400",
            "start",
            true,
            StableElementKind::Text,
            Some(rect),
        );
        let evidence_complete = !evidence_fit.overflowed && tag_text == evidence.trim();
        if !evidence_complete {
            draft.push_degradation(
                &block_id,
                &block.label,
                "evidence",
                "shortened_to_single_tag_and_notes",
                evidence,
            );
        }
    } else if evidence_expected {
        draft.push_degradation(
            &block_id,
            &block.label,
            "evidence",
            "omitted_to_speaker_notes",
            evidence.unwrap_or_default(),
        );
    }
    if label_fit.overflowed {
        draft
            .hard_failures
            .push(format!("{} label overflow", block_id));
    }
    draft.rendered_blocks.push(StableRenderedBlock {
        id: block_id,
        rect,
        label_complete: !label_missing && !label_fit.overflowed,
        text_complete: !text_missing && !text_fit.overflowed,
    });
}

#[allow(clippy::too_many_arguments)]
fn append_motif_chrome(
    draft: &mut StablePageDraft,
    id: &str,
    block: &ContentBlock,
    rect: StableRect,
    index: usize,
    tokens: &StableVisualTokens,
    motif: StableMotif,
    detail_visible: bool,
    evidence_visible: bool,
) -> StableBlockTextLayout {
    let accent = if index % 2 == 0 {
        &tokens.accent
    } else {
        &tokens.primary
    };
    let mut content_x = rect.x + 20.0;
    let mut content_width = rect.width - 40.0;
    let mut label_y = rect.y + 16.0;
    let label_height = 42.0;
    let mut label_align = "start";
    let mut body_top = rect.y + 66.0;
    let mut body_bottom = rect.bottom() - 18.0;

    match motif {
        StableMotif::PlainEditorial => {
            draft.body.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"4\"/><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                rect.x, rect.y + 2.0, rect.x + 58.0, rect.y + 2.0, accent,
                rect.x, rect.bottom() - 1.0, rect.right(), rect.bottom() - 1.0, tokens.border
            ));
            draft.push_decoration(
                &format!("{}-top-emphasis", id),
                StableDecorationPurpose::Emphasis,
                stable_line_rect(rect.x, rect.y + 2.0, rect.x + 58.0, rect.y + 2.0, 4.0),
                &[&format!("{}-label", id)],
            );
            draft.push_decoration(
                &format!("{}-bottom-divider", id),
                StableDecorationPurpose::Divider,
                stable_line_rect(
                    rect.x,
                    rect.bottom() - 1.0,
                    rect.right(),
                    rect.bottom() - 1.0,
                    1.0,
                ),
                &[id],
            );
        }
        StableMotif::TopBandCard | StableMotif::ComparisonColumn => {
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"8\" fill=\"{}\"/>\n",
                rect.x, rect.y, rect.width, rect.height, tokens.corner_radius, tokens.surface,
                rect.x, rect.y, rect.width, accent
            ));
            draft.push_decoration(
                &format!("{}-top-band", id),
                StableDecorationPurpose::Emphasis,
                StableRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: 8.0,
                },
                &[&format!("{}-label", id)],
            );
            label_y += 10.0;
            body_top += 8.0;
        }
        StableMotif::NumberedBadge => {
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"18\" fill=\"{}\"/>\n",
                rect.x, rect.y, rect.width, rect.height, tokens.corner_radius, tokens.panel,
                rect.x + 18.0, rect.bottom() - 8.0, rect.right() - 18.0, rect.bottom() - 8.0, accent,
                rect.x + 36.0, rect.y + 34.0, accent
            ));
            append_single_line_centered(
                draft,
                &format!("{}-badge-index", id),
                &index.to_string(),
                rect.x + 36.0,
                rect.y + 34.0,
                14.0,
                if tokens.dark { &tokens.text } else { "#FFFFFF" },
                "700",
            );
            draft.push_decoration(
                &format!("{}-badge", id),
                StableDecorationPurpose::DataMarker,
                StableRect {
                    x: rect.x + 18.0,
                    y: rect.y + 16.0,
                    width: 36.0,
                    height: 36.0,
                },
                &[&format!("{}-badge-index", id), &format!("{}-label", id)],
            );
            draft.push_decoration(
                &format!("{}-bottom-divider", id),
                StableDecorationPurpose::Divider,
                stable_line_rect(
                    rect.x + 18.0,
                    rect.bottom() - 8.0,
                    rect.right() - 18.0,
                    rect.bottom() - 8.0,
                    2.0,
                ),
                &[id],
            );
            content_x += 52.0;
            content_width -= 52.0;
        }
        StableMotif::BigNumber => {
            let number = stable_numeric_anchor(block).unwrap_or_default();
            if number.is_empty() {
                draft
                    .hard_failures
                    .push(format!("{} has no meaningful numeric anchor", id));
            }
            append_fitted_text(
                draft,
                &format!("{}-hero-number", id),
                &number,
                StableRect {
                    x: rect.x + 10.0,
                    y: rect.y + 8.0,
                    width: 106.0,
                    height: rect.height - 16.0,
                },
                if number.len() >= 4 { 40.0 } else { 64.0 },
                28.0,
                1.0,
                &tokens.subtle,
                "800",
                "middle",
                true,
                StableElementKind::Text,
                Some(rect),
            );
            draft.body.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"/>\n",
                rect.x + 124.0, rect.y + 12.0, rect.x + 124.0, rect.bottom() - 12.0, accent
            ));
            draft.push_decoration(
                &format!("{}-number-divider", id),
                StableDecorationPurpose::Divider,
                stable_line_rect(
                    rect.x + 124.0,
                    rect.y + 12.0,
                    rect.x + 124.0,
                    rect.bottom() - 12.0,
                    2.0,
                ),
                &[&format!("{}-hero-number", id), &format!("{}-label", id)],
            );
            content_x += 148.0;
            content_width -= 148.0;
        }
        StableMotif::QuoteStatement => {
            let emphasis_y = rect.bottom() - if evidence_visible { 42.0 } else { 14.0 };
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/><text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"56\" font-weight=\"700\" fill=\"{}\">“</text><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"3\"/>\n",
                rect.x, rect.y, rect.width, rect.height, tokens.corner_radius, tokens.panel,
                rect.x + 18.0, rect.y + 54.0, tokens.font_family, accent,
                rect.x + 22.0, emphasis_y, rect.right() - 22.0, emphasis_y, accent
            ));
            draft.push_decoration(
                &format!("{}-quote-emphasis", id),
                StableDecorationPurpose::Emphasis,
                stable_line_rect(
                    rect.x + 22.0,
                    emphasis_y,
                    rect.right() - 22.0,
                    emphasis_y,
                    3.0,
                ),
                &[&format!("{}-text", id)],
            );
            content_x += 38.0;
            content_width -= 48.0;
        }
        StableMotif::SplitPanel => {
            let split_x = rect.x + rect.width * 0.34;
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"/>\n",
                rect.x, rect.y, rect.width, rect.height, tokens.corner_radius, tokens.surface,
                rect.x, rect.y, rect.width * 0.34, rect.height, tokens.panel,
                split_x, rect.y + 14.0, split_x, rect.bottom() - 14.0, accent
            ));
            draft.push_decoration(
                &format!("{}-split-divider", id),
                StableDecorationPurpose::Grouping,
                stable_line_rect(split_x, rect.y + 14.0, split_x, rect.bottom() - 14.0, 2.0),
                &[&format!("{}-label", id), &format!("{}-text", id)],
            );
            return split_panel_text_layout(rect, evidence_visible, detail_visible);
        }
        StableMotif::TimelineNode => {
            let center_x = rect.x + rect.width / 2.0;
            draft.body.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"11\" fill=\"{}\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"4\" fill=\"{}\"/>\n",
                center_x, rect.y, center_x, rect.y + 42.0, tokens.border,
                center_x, rect.y + 20.0, accent, center_x, rect.y + 20.0, tokens.background
            ));
            draft.push_decoration(
                &format!("{}-timeline-node", id),
                StableDecorationPurpose::DataMarker,
                StableRect {
                    x: center_x - 11.0,
                    y: rect.y,
                    width: 22.0,
                    height: 42.0,
                },
                &[&format!("{}-label", id)],
            );
            label_y = rect.y + 48.0;
            body_top = rect.y + 98.0;
            label_align = "middle";
        }
        StableMotif::StepBlock => {
            let notch = 26.0;
            draft.body.push_str(&format!(
                "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{}\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"46\" height=\"30\" fill=\"{}\"/>\n",
                rect.x, rect.y, rect.right() - notch, rect.y, rect.right(), rect.y + notch,
                rect.right(), rect.bottom(), rect.x, rect.bottom(), tokens.panel,
                rect.x + 18.0, rect.y + 16.0, accent
            ));
            append_single_line_centered(
                draft,
                &format!("{}-step-index", id),
                &format!("{:02}", index),
                rect.x + 41.0,
                rect.y + 31.0,
                12.0,
                if tokens.dark { &tokens.text } else { "#FFFFFF" },
                "700",
            );
            content_x += 56.0;
            content_width -= 70.0;
        }
        StableMotif::HubSpoke => {
            let node_x = rect.x + 28.0;
            let node_y = rect.y + rect.height / 2.0;
            draft.body.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"16\" fill=\"{}\"/>\n",
                node_x, node_y, rect.x + 60.0, node_y, tokens.border,
                node_x, node_y, accent
            ));
            content_x += 72.0;
            content_width -= 72.0;
        }
        StableMotif::BracketGroup => {
            draft.body.push_str(&format!(
                "<path d=\"M {:.1} {:.1} H {:.1} V {:.1} H {:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"3\"/><text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"13\" font-weight=\"700\" fill=\"{}\">{}</text>\n",
                rect.x + 22.0, rect.y + 4.0, rect.x + 8.0, rect.bottom() - 4.0, rect.x + 22.0,
                accent, rect.x + 30.0, rect.y + 20.0, tokens.font_family, accent, roman_numeral(index)
            ));
            content_x += 38.0;
            content_width -= 38.0;
            label_y += 10.0;
        }
        StableMotif::EvidenceStrip => {
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/>\n",
                rect.x, rect.y, rect.width, rect.height, tokens.corner_radius, tokens.surface
            ));
            if evidence_visible {
                draft.body.push_str(&format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"34\" fill=\"{}\"/>\n",
                    rect.x,
                    rect.bottom() - 34.0,
                    rect.width,
                    tokens.panel
                ));
                body_bottom -= 30.0;
            } else {
                draft.body.push_str(&format!(
                    "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                    rect.x + 18.0, rect.bottom() - 10.0, rect.right() - 18.0, rect.bottom() - 10.0, tokens.border
                ));
                draft.push_decoration(
                    &format!("{}-evidence-divider", id),
                    StableDecorationPurpose::Divider,
                    stable_line_rect(
                        rect.x + 18.0,
                        rect.bottom() - 10.0,
                        rect.right() - 18.0,
                        rect.bottom() - 10.0,
                        1.0,
                    ),
                    &[id],
                );
                body_bottom -= 8.0;
            }
        }
        StableMotif::ImagePlaceholderEditorial => {
            let visual_w = (rect.width * 0.30).clamp(86.0, 180.0);
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"4\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"18\" fill=\"{}\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"18\" fill=\"{}\"/>\n",
                rect.x, rect.y, visual_w, rect.height, tokens.panel,
                rect.x + 24.0, rect.y + 34.0, rect.x + visual_w - 24.0, rect.bottom() - 34.0, accent,
                rect.x + 24.0, rect.y + rect.height * 0.34, visual_w - 48.0, tokens.subtle,
                rect.x + 24.0, rect.y + rect.height * 0.58, (visual_w - 48.0) * 0.68, tokens.subtle
            ));
            content_x = rect.x + visual_w + 24.0;
            content_width = rect.width - visual_w - 44.0;
        }
        StableMotif::MatrixCell => {
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"3\"/><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                rect.x, rect.y, rect.width, rect.height, tokens.panel,
                rect.x, rect.y, rect.right(), rect.y, accent,
                rect.x, rect.bottom(), rect.right(), rect.bottom(), tokens.border
            ));
            append_fitted_text(
                draft,
                &format!("{}-matrix-index", id),
                &format!("{:02}", index),
                StableRect {
                    x: rect.right() - 40.0,
                    y: rect.y + 12.0,
                    width: 28.0,
                    height: 24.0,
                },
                11.0,
                10.0,
                1.1,
                &tokens.muted,
                "600",
                "end",
                true,
                StableElementKind::Text,
                Some(rect),
            );
            content_width -= 42.0;
        }
        StableMotif::SectionBanner => {
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"12\" fill=\"{}\"/>\n",
                rect.x, rect.y, rect.width, rect.height, tokens.panel,
                rect.x, rect.bottom() - 12.0, rect.width, accent
            ));
            draft.push_decoration(
                &format!("{}-banner-emphasis", id),
                StableDecorationPurpose::Emphasis,
                StableRect {
                    x: rect.x,
                    y: rect.bottom() - 12.0,
                    width: rect.width,
                    height: 12.0,
                },
                &[&format!("{}-text", id)],
            );
        }
    }

    let evidence_height = if evidence_visible { 22.0 } else { 0.0 };
    if evidence_visible && motif != StableMotif::EvidenceStrip {
        body_bottom -= evidence_height + 6.0;
    }
    let body_height = (body_bottom - body_top).max(36.0);
    let detail_height = if detail_visible {
        (body_height * 0.40)
            .clamp(22.0, 70.0)
            .min((body_height - 31.0).max(0.0))
    } else {
        0.0
    };
    let detail_gap = if detail_visible { 7.0 } else { 0.0 };
    let text_height = (body_height - detail_height - detail_gap).max(24.0);
    StableBlockTextLayout {
        label: StableRect {
            x: content_x,
            y: label_y,
            width: content_width,
            height: label_height,
        },
        text: StableRect {
            x: content_x,
            y: body_top,
            width: content_width,
            height: text_height,
        },
        detail: StableRect {
            x: content_x,
            y: body_top + text_height + detail_gap,
            width: content_width,
            height: detail_height,
        },
        evidence: StableRect {
            x: content_x,
            y: rect.bottom() - evidence_height - 8.0,
            width: content_width,
            height: evidence_height,
        },
        label_align,
    }
}

fn split_panel_text_layout(
    rect: StableRect,
    evidence_visible: bool,
    detail_visible: bool,
) -> StableBlockTextLayout {
    let split_x = rect.x + rect.width * 0.34;
    let right_x = split_x + 22.0;
    let right_width = rect.right() - right_x - 20.0;
    let evidence_height = if evidence_visible { 22.0 } else { 0.0 };
    let body_top = rect.y + 24.0;
    let body_bottom = rect.bottom() - 18.0 - evidence_height;
    let body_height = (body_bottom - body_top).max(40.0);
    let detail_height = if detail_visible {
        (body_height * 0.40)
            .clamp(22.0, 72.0)
            .min((body_height - 32.0).max(0.0))
    } else {
        0.0
    };
    StableBlockTextLayout {
        label: StableRect {
            x: rect.x + 18.0,
            y: rect.y + 24.0,
            width: rect.width * 0.34 - 36.0,
            height: rect.height - 48.0,
        },
        text: StableRect {
            x: right_x,
            y: body_top,
            width: right_width,
            height: body_height - detail_height - if detail_visible { 8.0 } else { 0.0 },
        },
        detail: StableRect {
            x: right_x,
            y: body_bottom - detail_height,
            width: right_width,
            height: detail_height,
        },
        evidence: StableRect {
            x: right_x,
            y: rect.bottom() - evidence_height - 8.0,
            width: right_width,
            height: evidence_height,
        },
        label_align: "start",
    }
}

fn render_stable_anchor(
    plan: &SlidePlan,
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    let repair_title = profile.local_repair.as_ref().is_some_and(|repair| {
        repair.level == StableRepairLevel::TextBox && repair.text_role.as_deref() == Some("title")
    });
    let repair_core = profile.local_repair.as_ref().is_some_and(|repair| {
        matches!(
            repair.level,
            StableRepairLevel::TextBox | StableRepairLevel::ContentBlock
        ) && repair.text_role.as_deref() == Some("coreMessage")
    });
    match motif {
        StableMotif::SectionBanner => draft.body.push_str(&format!(
            "<rect x=\"56\" y=\"54\" width=\"760\" height=\"548\" rx=\"{:.1}\" fill=\"{}\"/><rect x=\"56\" y=\"570\" width=\"760\" height=\"32\" fill=\"{}\"/>\n",
            tokens.corner_radius, tokens.panel, tokens.primary
        )),
        StableMotif::QuoteStatement => draft.body.push_str(&format!(
            "<rect x=\"56\" y=\"54\" width=\"760\" height=\"548\" rx=\"{:.1}\" fill=\"{}\"/><text x=\"84\" y=\"142\" font-family=\"{}\" font-size=\"72\" font-weight=\"700\" fill=\"{}\">“</text><line x1=\"96\" y1=\"570\" x2=\"360\" y2=\"570\" stroke=\"{}\" stroke-width=\"4\"/>\n",
            tokens.corner_radius, tokens.surface, tokens.font_family, tokens.subtle, tokens.accent
        )),
        _ => draft.body.push_str(&format!(
            "<line x1=\"56\" y1=\"54\" x2=\"252\" y2=\"54\" stroke=\"{}\" stroke-width=\"6\"/><line x1=\"56\" y1=\"602\" x2=\"816\" y2=\"602\" stroke=\"{}\" stroke-width=\"1.5\"/>\n",
            tokens.primary, tokens.border
        )),
    }
    draft.push_rect(
        "anchor-main",
        StableRect {
            x: 56.0,
            y: 54.0,
            width: 760.0,
            height: 548.0,
        },
        StableElementKind::Card,
    );
    let title_fit = append_fitted_text(
        &mut draft,
        "anchor-kicker",
        if slide.page_theme.trim().is_empty() {
            &plan.title
        } else {
            &slide.page_theme
        },
        StableRect {
            x: 98.0,
            y: 96.0,
            width: 620.0,
            height: 30.0,
        },
        17.0,
        15.0,
        1.2,
        &tokens.primary,
        "700",
        "start",
        false,
        StableElementKind::Text,
        Some(StableRect {
            x: 56.0,
            y: 54.0,
            width: 760.0,
            height: 548.0,
        }),
    );
    append_fitted_text(
        &mut draft,
        "anchor-title",
        &slide.title,
        StableRect {
            x: 96.0,
            y: 150.0,
            width: 650.0,
            height: if repair_title { 132.0 } else { 120.0 },
        },
        if repair_title { 50.0 } else { 54.0 },
        if repair_title { 36.0 } else { 42.0 },
        if repair_title { 1.05 } else { 1.12 },
        &tokens.text,
        "800",
        "start",
        true,
        StableElementKind::Text,
        Some(StableRect {
            x: 56.0,
            y: 54.0,
            width: 760.0,
            height: 548.0,
        }),
    );
    if slide.title.trim().is_empty() || title_fit.overflowed {
        draft
            .hard_failures
            .push("required page title cannot be rendered completely".to_string());
    }
    let core_fit = append_fitted_text(
        &mut draft,
        "anchor-core",
        &stable_core_message(slide),
        StableRect {
            x: 98.0,
            y: 302.0,
            width: 650.0,
            height: if repair_core { 132.0 } else { 118.0 },
        },
        if repair_core { 23.0 } else { 25.0 },
        if repair_core { 17.0 } else { 20.0 },
        if repair_core { 1.18 } else { 1.42 },
        &tokens.muted,
        "500",
        "start",
        true,
        StableElementKind::Text,
        Some(StableRect {
            x: 56.0,
            y: 54.0,
            width: 760.0,
            height: 548.0,
        }),
    );
    if stable_core_message(slide).trim().is_empty() || core_fit.overflowed {
        draft
            .hard_failures
            .push("anchor core message overflow".to_string());
    }
    let blocks = slide_blocks(slide);
    for (idx, block) in blocks.iter().take(2).enumerate() {
        let y = 464.0 + idx as f32 * 50.0;
        draft.body.push_str(&format!(
            "<circle cx=\"112\" cy=\"{:.1}\" r=\"5\" fill=\"{}\"/>\n",
            y + 11.0,
            if idx == 0 {
                &tokens.primary
            } else {
                &tokens.accent
            }
        ));
        append_fitted_text(
            &mut draft,
            &format!("anchor-support-{}", idx + 1),
            &format!("{} · {}", block.label, block.text),
            StableRect {
                x: 132.0,
                y,
                width: 600.0,
                height: 38.0,
            },
            15.0,
            13.0,
            1.25,
            &tokens.text,
            "500",
            "start",
            true,
            StableElementKind::Text,
            Some(StableRect {
                x: 56.0,
                y: 54.0,
                width: 760.0,
                height: 548.0,
            }),
        );
    }
    let visual_center_x = 1036.0;
    let visual_center_y = 310.0;
    draft.body.push_str(&format!(
        "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"142\" fill=\"{}\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"102\" fill=\"{}\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"52\" fill=\"{}\"/><line x1=\"900\" y1=\"510\" x2=\"1172\" y2=\"510\" stroke=\"{}\" stroke-width=\"5\"/>\n",
        visual_center_x,
        visual_center_y,
        tokens.panel,
        visual_center_x,
        visual_center_y,
        tokens.background,
        visual_center_x,
        visual_center_y,
        tokens.primary,
        tokens.accent
    ));
    append_single_line_centered(
        &mut draft,
        "anchor-page",
        &format!("{:02}", slide.page),
        visual_center_x,
        visual_center_y,
        34.0,
        if tokens.dark { &tokens.text } else { "#FFFFFF" },
        "800",
    );
    if detail_level == StableDetailLevel::Full {
        append_fitted_text(
            &mut draft,
            "anchor-style",
            &plan.style,
            StableRect {
                x: 900.0,
                y: 540.0,
                width: 272.0,
                height: 34.0,
            },
            16.0,
            14.0,
            1.2,
            &tokens.muted,
            "500",
            "middle",
            true,
            StableElementKind::Text,
            None,
        );
    }
    Ok(draft)
}

fn render_stable_timeline(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        true,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let count = blocks.len().clamp(1, 5);
    let gap = if count >= 5 { 14.0 } else { 20.0 };
    let card_width = (1168.0 - gap * (count.saturating_sub(1) as f32)) / count as f32;
    let line_y = 226.0;
    draft.body.push_str(&format!(
        "<line x1=\"82\" y1=\"{:.1}\" x2=\"1198\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"4\"/>\n",
        line_y, line_y, tokens.border
    ));
    draft.push_decoration(
        "timeline-axis",
        StableDecorationPurpose::TimelineAxis,
        stable_line_rect(82.0, line_y, 1198.0, line_y, 4.0),
        &["timeline-card-1", &format!("timeline-card-{}", count)],
    );
    for (idx, block) in blocks.iter().take(count).enumerate() {
        let x = 56.0 + idx as f32 * (card_width + gap);
        let center_x = x + card_width / 2.0;
        let accent = if idx % 2 == 0 {
            &tokens.primary
        } else {
            &tokens.accent
        };
        draft.body.push_str(&format!(
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"266\" stroke=\"{}\" stroke-width=\"2\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"13\" fill=\"{}\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"5\" fill=\"{}\"/>\n",
            center_x, line_y, center_x, tokens.border, center_x, line_y, accent, center_x, line_y, tokens.background
        ));
        draft.push_decoration(
            &format!("timeline-marker-{}", idx + 1),
            StableDecorationPurpose::DataMarker,
            StableRect {
                x: center_x - 13.0,
                y: line_y - 13.0,
                width: 26.0,
                height: 53.0,
            },
            &[&format!("timeline-card-{}", idx + 1)],
        );
        let rect = StableRect {
            x,
            y: 266.0,
            width: card_width,
            height: 360.0,
        };
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            rect,
            idx + 1,
            tokens,
            motif,
            detail_level,
            profile.local_repair.as_ref(),
            "timeline-card",
        );
    }
    Ok(draft)
}

fn render_stable_process(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        true,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let count = blocks.len().clamp(1, 5);
    let gap = 34.0;
    let width = (1168.0 - gap * (count.saturating_sub(1) as f32)) / count as f32;
    for (idx, block) in blocks.iter().take(count).enumerate() {
        let x = 56.0 + idx as f32 * (width + gap);
        let rect = StableRect {
            x,
            y: 222.0,
            width,
            height: 396.0,
        };
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            rect,
            idx + 1,
            tokens,
            motif,
            detail_level,
            profile.local_repair.as_ref(),
            "process-step",
        );
        if idx + 1 < count {
            let arrow_x = rect.right() + 8.0;
            draft.body.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"420\" x2=\"{:.1}\" y2=\"420\" stroke=\"{}\" stroke-width=\"3\"/><polygon points=\"{:.1},412 {:.1},420 {:.1},428\" fill=\"{}\"/>\n",
                arrow_x,
                arrow_x + 18.0,
                tokens.primary,
                arrow_x + 18.0,
                arrow_x + 28.0,
                arrow_x + 18.0,
                tokens.primary
            ));
            draft.push_decoration(
                &format!("process-connector-{}", idx + 1),
                StableDecorationPurpose::Connector,
                stable_line_rect(arrow_x, 420.0, arrow_x + 28.0, 420.0, 3.0),
                &[
                    &format!("process-step-{}", idx + 1),
                    &format!("process-step-{}", idx + 2),
                ],
            );
        }
    }
    Ok(draft)
}

fn grid_rects_for_blocks(slide: &Slide, blocks: &[ContentBlock]) -> Vec<StableRect> {
    let count = blocks.len().clamp(2, 6);
    let columns = match count {
        2 => 2,
        3 => 3,
        4 => 2,
        _ => 3,
    };
    let rows = (count + columns - 1) / columns;
    let gap_x = 24.0;
    let gap_y = 20.0;
    let width = (1168.0 - gap_x * (columns.saturating_sub(1) as f32)) / columns as f32;
    let mut row_heights = vec![0.0_f32; rows];
    for (idx, block) in blocks.iter().take(count).enumerate() {
        let has_detail = !block.detail.trim().is_empty();
        let has_evidence = slide
            .evidence
            .get(idx)
            .is_some_and(|value| !value.trim().is_empty());
        let minimum_height = if has_detail {
            195.0
        } else if has_evidence {
            158.0
        } else {
            132.0
        };
        let preferred =
            preferred_card_height(block, slide.evidence.get(idx).map(String::as_str), width)
                .clamp(minimum_height, 270.0);
        row_heights[idx / columns] = row_heights[idx / columns].max(preferred);
    }
    let available = STABLE_CONTENT_BOTTOM - STABLE_CONTENT_TOP;
    let total_preferred = row_heights.iter().sum::<f32>() + gap_y * (rows.saturating_sub(1) as f32);
    if total_preferred > available {
        let each = (available - gap_y * (rows.saturating_sub(1) as f32)) / rows as f32;
        row_heights.fill(each);
    }
    let total_height = row_heights.iter().sum::<f32>() + gap_y * (rows.saturating_sub(1) as f32);
    let start_y = STABLE_CONTENT_TOP + ((available - total_height) / 2.0).max(0.0);
    let mut rects = Vec::new();
    let mut y = start_y;
    for row in 0..rows {
        let items_in_row = (count - row * columns).min(columns);
        let row_width =
            items_in_row as f32 * width + (items_in_row.saturating_sub(1) as f32) * gap_x;
        let start_x = STABLE_SAFE_LEFT + (1168.0 - row_width) / 2.0;
        for col in 0..items_in_row {
            rects.push(StableRect {
                x: start_x + col as f32 * (width + gap_x),
                y,
                width,
                height: row_heights[row],
            });
        }
        y += row_heights[row] + gap_y;
    }
    rects
}

fn render_stable_category_grid(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        true,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let rects = grid_rects_for_blocks(slide, &blocks);
    for (idx, (block, rect)) in blocks.iter().zip(rects).enumerate() {
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            rect,
            idx + 1,
            tokens,
            motif,
            detail_level,
            profile.local_repair.as_ref(),
            "category-card",
        );
    }
    Ok(draft)
}

fn render_stable_editorial_split(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        false,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let hero_number = blocks
        .iter()
        .find_map(stable_numeric_anchor)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("{:02}", slide.page));
    let left = StableRect {
        x: 56.0,
        y: 194.0,
        width: 438.0,
        height: 430.0,
    };
    draft.body.push_str(&format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"74\" height=\"6\" rx=\"3\" fill=\"{}\"/>\n",
        left.x, left.y, left.width, left.height, tokens.corner_radius, tokens.panel, left.x + 34.0, left.y + 34.0, tokens.primary
    ));
    draft.push_rect("editorial-core", left, StableElementKind::Card);
    draft.body.push_str(&format!(
        "<text x=\"90\" y=\"354\" font-family=\"{}\" font-size=\"88\" font-weight=\"800\" fill=\"{}\">{}</text><circle cx=\"438\" cy=\"564\" r=\"74\" fill=\"{}\"/>\n",
        tokens.font_family,
        tokens.subtle,
        xml_escape(&hero_number),
        tokens.background
    ));
    append_fitted_text(
        &mut draft,
        "editorial-kicker",
        if slide.page_theme.trim().is_empty() {
            "核心判断"
        } else {
            &slide.page_theme
        },
        StableRect {
            x: left.x + 34.0,
            y: left.y + 62.0,
            width: left.width - 68.0,
            height: 30.0,
        },
        16.0,
        14.0,
        1.2,
        &tokens.primary,
        "700",
        "start",
        false,
        StableElementKind::Text,
        Some(left),
    );
    let fit = append_fitted_text(
        &mut draft,
        "editorial-message",
        &stable_core_message(slide),
        StableRect {
            x: left.x + 34.0,
            y: left.y + 190.0,
            width: left.width - 68.0,
            height: 180.0,
        },
        28.0,
        20.0,
        1.35,
        &tokens.text,
        "700",
        "start",
        true,
        StableElementKind::Text,
        Some(left),
    );
    if fit.overflowed {
        draft
            .hard_failures
            .push("editorial core overflow".to_string());
    }
    let right_x = 542.0;
    let gap = 14.0;
    let count = blocks.len().clamp(2, 4);
    let height = (430.0 - gap * (count.saturating_sub(1) as f32)) / count as f32;
    for (idx, block) in blocks.iter().take(count).enumerate() {
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            StableRect {
                x: right_x,
                y: 194.0 + idx as f32 * (height + gap),
                width: 682.0,
                height,
            },
            idx + 1,
            tokens,
            motif,
            if count >= 4 {
                StableDetailLevel::Reduced
            } else {
                detail_level
            },
            profile.local_repair.as_ref(),
            "editorial-support",
        );
    }
    Ok(draft)
}

fn render_stable_comparison(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        true,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let split = (blocks.len() + 1) / 2;
    let groups = [&blocks[..split], &blocks[split..]];
    draft.body.push_str(&format!(
        "<line x1=\"640\" y1=\"202\" x2=\"640\" y2=\"612\" stroke=\"{}\" stroke-width=\"2\"/><circle cx=\"640\" cy=\"407\" r=\"24\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\"/>\n",
        tokens.border, tokens.background, tokens.primary
    ));
    append_single_line_centered(
        &mut draft,
        "compare-axis-label",
        "VS",
        640.0,
        407.0,
        13.0,
        &tokens.primary,
        "800",
    );
    draft.push_decoration(
        "comparison-axis",
        StableDecorationPurpose::Grouping,
        stable_line_rect(640.0, 202.0, 640.0, 612.0, 2.0),
        &[
            "compare-heading-1",
            "compare-heading-2",
            "compare-axis-label",
        ],
    );
    for (side, group) in groups.iter().enumerate() {
        let x = if side == 0 { 56.0 } else { 652.0 };
        let accent = if side == 0 {
            &tokens.primary
        } else {
            &tokens.accent
        };
        if motif == StableMotif::SplitPanel {
            draft.body.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"188\" width=\"572\" height=\"438\" fill=\"{}\"/><line x1=\"{:.1}\" y1=\"188\" x2=\"{:.1}\" y2=\"626\" stroke=\"{}\" stroke-width=\"3\"/>\n",
                x,
                if side == 0 { &tokens.surface } else { &tokens.panel },
                if side == 0 { x + 572.0 } else { x },
                if side == 0 { x + 572.0 } else { x },
                accent
            ));
        } else {
            draft.body.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"188\" x2=\"{:.1}\" y2=\"188\" stroke=\"{}\" stroke-width=\"8\"/><line x1=\"{:.1}\" y1=\"626\" x2=\"{:.1}\" y2=\"626\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                x, x + 572.0, accent, x, x + 572.0, tokens.border
            ));
        }
        append_fitted_text(
            &mut draft,
            &format!("compare-heading-{}", side + 1),
            group
                .first()
                .map(|block| block.label.as_str())
                .unwrap_or(if side == 0 { "视角 A" } else { "视角 B" }),
            StableRect {
                x: x + 28.0,
                y: 214.0,
                width: 500.0,
                height: 38.0,
            },
            23.0,
            19.0,
            1.2,
            accent,
            "700",
            "start",
            true,
            StableElementKind::Text,
            Some(StableRect {
                x,
                y: 188.0,
                width: 572.0,
                height: 438.0,
            }),
        );
        let item_count = group.len().max(1).min(3);
        let item_h = (344.0 - 14.0 * (item_count.saturating_sub(1) as f32)) / item_count as f32;
        let item_motif = if motif == StableMotif::ComparisonColumn {
            StableMotif::PlainEditorial
        } else {
            StableMotif::EvidenceStrip
        };
        for (idx, block) in group.iter().take(item_count).enumerate() {
            render_stable_motif_block(
                &mut draft,
                block,
                slide.evidence.get(side * split + idx).map(String::as_str),
                StableRect {
                    x: x + 20.0,
                    y: 268.0 + idx as f32 * (item_h + 14.0),
                    width: 532.0,
                    height: item_h,
                },
                idx + 1,
                tokens,
                item_motif,
                if item_count > 2 {
                    StableDetailLevel::Reduced
                } else {
                    detail_level
                },
                profile.local_repair.as_ref(),
                &format!("compare-side-{}", side + 1),
            );
        }
    }
    Ok(draft)
}

fn render_stable_cause_effect(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        false,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let split = (blocks.len() + 1) / 2;
    let left_blocks = &blocks[..split];
    let right_blocks = &blocks[split..];
    let center = StableRect {
        x: 462.0,
        y: 264.0,
        width: 356.0,
        height: 264.0,
    };
    if motif == StableMotif::HubSpoke {
        draft.body.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"132\" fill=\"{}\"/><line x1=\"406\" y1=\"396\" x2=\"462\" y2=\"396\" stroke=\"{}\" stroke-width=\"3\"/><line x1=\"818\" y1=\"396\" x2=\"874\" y2=\"396\" stroke=\"{}\" stroke-width=\"3\"/><circle cx=\"462\" cy=\"396\" r=\"6\" fill=\"{}\"/><circle cx=\"818\" cy=\"396\" r=\"6\" fill=\"{}\"/>\n",
            center.x, center.y, center.width, center.height, tokens.panel,
            tokens.primary, tokens.accent, tokens.primary, tokens.accent
        ));
    } else {
        draft.body.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/><line x1=\"420\" y1=\"396\" x2=\"452\" y2=\"396\" stroke=\"{}\" stroke-width=\"4\"/><polygon points=\"452,388 462,396 452,404\" fill=\"{}\"/><line x1=\"828\" y1=\"396\" x2=\"860\" y2=\"396\" stroke=\"{}\" stroke-width=\"4\"/><polygon points=\"860,388 870,396 860,404\" fill=\"{}\"/>\n",
            center.x, center.y, center.width, center.height, tokens.corner_radius, tokens.panel,
            tokens.primary, tokens.primary, tokens.accent, tokens.accent
        ));
    }
    draft.push_rect("cause-core", center, StableElementKind::Card);
    append_fitted_text(
        &mut draft,
        "cause-label",
        if slide.page_theme.trim().is_empty() {
            "逻辑主轴"
        } else {
            &slide.page_theme
        },
        StableRect {
            x: center.x + 28.0,
            y: center.y + 28.0,
            width: center.width - 56.0,
            height: 32.0,
        },
        17.0,
        15.0,
        1.2,
        &tokens.primary,
        "700",
        "middle",
        true,
        StableElementKind::Text,
        Some(center),
    );
    let fit = append_fitted_text(
        &mut draft,
        "cause-message",
        &stable_core_message(slide),
        StableRect {
            x: center.x + 30.0,
            y: center.y + 78.0,
            width: center.width - 60.0,
            height: 150.0,
        },
        25.0,
        19.0,
        1.35,
        &tokens.text,
        "700",
        "middle",
        true,
        StableElementKind::Text,
        Some(center),
    );
    if fit.overflowed {
        draft.hard_failures.push("cause core overflow".to_string());
    }
    for (side, group) in [left_blocks, right_blocks].iter().enumerate() {
        let x = if side == 0 { 56.0 } else { 870.0 };
        let count = group.len().max(1).min(3);
        let gap = 16.0;
        let h = (438.0 - gap * (count.saturating_sub(1) as f32)) / count as f32;
        for (idx, block) in group.iter().take(count).enumerate() {
            render_stable_motif_block(
                &mut draft,
                block,
                slide
                    .evidence
                    .get(if side == 0 { idx } else { split + idx })
                    .map(String::as_str),
                StableRect {
                    x,
                    y: 188.0 + idx as f32 * (h + gap),
                    width: 350.0,
                    height: h,
                },
                idx + 1,
                tokens,
                motif,
                if count >= 3 {
                    StableDetailLevel::Reduced
                } else {
                    detail_level
                },
                profile.local_repair.as_ref(),
                if side == 0 {
                    "cause-input"
                } else {
                    "cause-outcome"
                },
            );
        }
    }
    Ok(draft)
}

fn render_stable_matrix(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        true,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let rects = [
        StableRect {
            x: 56.0,
            y: 188.0,
            width: 548.0,
            height: 202.0,
        },
        StableRect {
            x: 676.0,
            y: 188.0,
            width: 548.0,
            height: 202.0,
        },
        StableRect {
            x: 56.0,
            y: 424.0,
            width: 548.0,
            height: 202.0,
        },
        StableRect {
            x: 676.0,
            y: 424.0,
            width: 548.0,
            height: 202.0,
        },
    ];
    draft.body.push_str(&format!(
        "<line x1=\"640\" y1=\"188\" x2=\"640\" y2=\"626\" stroke=\"{}\" stroke-width=\"3\"/><polygon points=\"633,198 640,188 647,198\" fill=\"{}\"/><line x1=\"56\" y1=\"407\" x2=\"1224\" y2=\"407\" stroke=\"{}\" stroke-width=\"3\"/><polygon points=\"1214,400 1224,407 1214,414\" fill=\"{}\"/><circle cx=\"640\" cy=\"407\" r=\"9\" fill=\"{}\"/>\n",
        tokens.primary,
        tokens.primary,
        tokens.accent,
        tokens.accent,
        tokens.text
    ));
    draft.push_decoration(
        "matrix-y-axis",
        StableDecorationPurpose::Grouping,
        stable_line_rect(640.0, 188.0, 640.0, 626.0, 3.0),
        &["matrix-cell-1", "matrix-cell-3"],
    );
    draft.push_decoration(
        "matrix-x-axis",
        StableDecorationPurpose::Grouping,
        stable_line_rect(56.0, 407.0, 1224.0, 407.0, 3.0),
        &["matrix-cell-1", "matrix-cell-2"],
    );
    for (index, (block, rect)) in blocks.iter().take(4).zip(rects).enumerate() {
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(index).map(String::as_str),
            rect,
            index + 1,
            tokens,
            if motif == StableMotif::TopBandCard {
                StableMotif::MatrixCell
            } else {
                motif
            },
            if detail_level == StableDetailLevel::Full {
                StableDetailLevel::Reduced
            } else {
                detail_level
            },
            profile.local_repair.as_ref(),
            "matrix-cell",
        );
    }
    Ok(draft)
}

fn render_stable_hierarchy(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        true,
        profile.local_repair.as_ref(),
    );
    let blocks = slide_blocks(slide);
    let count = blocks.len().clamp(2, 5);
    let gap = 14.0;
    let height = (438.0 - gap * (count.saturating_sub(1) as f32)) / count as f32;
    draft.body.push_str(&format!(
        "<line x1=\"84\" y1=\"202\" x2=\"84\" y2=\"612\" stroke=\"{}\" stroke-width=\"4\"/>\n",
        tokens.border
    ));
    for (idx, block) in blocks.iter().take(count).enumerate() {
        let inset = idx as f32 * 42.0;
        let rect = StableRect {
            x: 120.0 + inset,
            y: 188.0 + idx as f32 * (height + gap),
            width: 1040.0 - inset * 2.0,
            height,
        };
        let center_y = rect.y + rect.height / 2.0;
        draft.body.push_str(&format!(
            "<line x1=\"84\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"/><circle cx=\"84\" cy=\"{:.1}\" r=\"7\" fill=\"{}\"/>\n",
            center_y,
            rect.x,
            center_y,
            tokens.primary,
            center_y,
            if idx % 2 == 0 { &tokens.primary } else { &tokens.accent }
        ));
        draft.push_decoration(
            &format!("hierarchy-connector-{}", idx + 1),
            StableDecorationPurpose::Grouping,
            stable_line_rect(84.0, center_y, rect.x, center_y, 2.0),
            &[&format!("hierarchy-level-{}", idx + 1)],
        );
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            rect,
            idx + 1,
            tokens,
            motif,
            if detail_level == StableDetailLevel::Essential {
                StableDetailLevel::Essential
            } else {
                StableDetailLevel::Reduced
            },
            profile.local_repair.as_ref(),
            "hierarchy-level",
        );
    }
    Ok(draft)
}

fn render_stable_quote(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    _detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        false,
        profile.local_repair.as_ref(),
    );
    draft.body.push_str(&format!(
        "<text x=\"74\" y=\"310\" font-family=\"{}\" font-size=\"112\" font-weight=\"700\" fill=\"{}\" opacity=\"0.22\">“</text>\n",
        tokens.font_family, tokens.primary
    ));
    let fit = append_fitted_text(
        &mut draft,
        "quote-core",
        &stable_core_message(slide),
        StableRect {
            x: 144.0,
            y: 224.0,
            width: 992.0,
            height: 220.0,
        },
        42.0,
        30.0,
        1.34,
        &tokens.text,
        "700",
        "middle",
        true,
        StableElementKind::Text,
        None,
    );
    if fit.overflowed {
        draft.hard_failures.push("quote core overflow".to_string());
    }
    let blocks = slide_blocks(slide);
    let count = blocks.len().min(3);
    let width = (1040.0 - 18.0 * (count.saturating_sub(1) as f32)) / count.max(1) as f32;
    for (idx, block) in blocks.iter().take(count).enumerate() {
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            StableRect {
                x: 120.0 + idx as f32 * (width + 18.0),
                y: 486.0,
                width,
                height: 140.0,
            },
            idx + 1,
            tokens,
            motif,
            StableDetailLevel::Essential,
            profile.local_repair.as_ref(),
            "quote-support",
        );
    }
    Ok(draft)
}

fn render_stable_evidence_led(
    slide: &Slide,
    profile: &StableRenderProfile,
    motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        false,
        profile.local_repair.as_ref(),
    );
    let core = StableRect {
        x: 56.0,
        y: 198.0,
        width: 472.0,
        height: 420.0,
    };
    let blocks = slide_blocks(slide);
    match motif {
        StableMotif::BigNumber => {
            let number = blocks
                .first()
                .and_then(stable_numeric_anchor)
                .unwrap_or_default();
            if number.is_empty() {
                draft
                    .hard_failures
                    .push("evidence-led page has no meaningful numeric anchor".to_string());
            }
            draft.body.push_str(&format!(
                "<text x=\"72\" y=\"344\" font-family=\"{}\" font-size=\"92\" font-weight=\"800\" fill=\"{}\">{}</text><line x1=\"76\" y1=\"586\" x2=\"488\" y2=\"586\" stroke=\"{}\" stroke-width=\"5\"/>\n",
                tokens.font_family, tokens.subtle, xml_escape(&number), tokens.primary
            ));
            draft.push_decoration(
                "evidence-number-emphasis",
                StableDecorationPurpose::Emphasis,
                stable_line_rect(76.0, 586.0, 488.0, 586.0, 5.0),
                &["evidence-message"],
            );
        }
        StableMotif::PlainEditorial => {
            draft.body.push_str(&format!(
                "<line x1=\"56\" y1=\"198\" x2=\"206\" y2=\"198\" stroke=\"{}\" stroke-width=\"6\"/><line x1=\"56\" y1=\"618\" x2=\"528\" y2=\"618\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                tokens.primary, tokens.border
            ));
            draft.push_decoration(
                "evidence-top-emphasis",
                StableDecorationPurpose::Emphasis,
                stable_line_rect(56.0, 198.0, 206.0, 198.0, 6.0),
                &["evidence-kicker"],
            );
            draft.push_decoration(
                "evidence-bottom-divider",
                StableDecorationPurpose::Divider,
                stable_line_rect(56.0, 618.0, 528.0, 618.0, 1.0),
                &["evidence-message"],
            );
        }
        _ => draft.body.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{:.1}\" fill=\"{}\"/><rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"34\" fill=\"{}\"/>\n",
            core.x, core.y, core.width, core.height, tokens.corner_radius, tokens.surface,
            core.x, core.bottom() - 34.0, core.width, tokens.panel
        )),
    }
    draft.push_rect("evidence-core", core, StableElementKind::Card);
    append_fitted_text(
        &mut draft,
        "evidence-kicker",
        if slide.page_theme.trim().is_empty() {
            "核心结论"
        } else {
            &slide.page_theme
        },
        StableRect {
            x: 90.0,
            y: 236.0,
            width: 300.0,
            height: 34.0,
        },
        17.0,
        15.0,
        1.2,
        &tokens.primary,
        "700",
        "start",
        true,
        StableElementKind::Text,
        Some(core),
    );
    let fit = append_fitted_text(
        &mut draft,
        "evidence-message",
        &stable_core_message(slide),
        StableRect {
            x: 90.0,
            y: 298.0,
            width: 380.0,
            height: 230.0,
        },
        31.0,
        23.0,
        1.36,
        &tokens.text,
        "700",
        "start",
        true,
        StableElementKind::Text,
        Some(core),
    );
    if fit.overflowed {
        draft
            .hard_failures
            .push("evidence core overflow".to_string());
    }
    let count = blocks.len().clamp(2, 4);
    let gap = 14.0;
    let h = (420.0 - gap * (count.saturating_sub(1) as f32)) / count as f32;
    for (idx, block) in blocks.iter().take(count).enumerate() {
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            StableRect {
                x: 558.0,
                y: 198.0 + idx as f32 * (h + gap),
                width: 666.0,
                height: h,
            },
            idx + 1,
            tokens,
            motif,
            if count >= 4 {
                StableDetailLevel::Reduced
            } else {
                detail_level
            },
            profile.local_repair.as_ref(),
            "evidence-support",
        );
    }
    Ok(draft)
}

fn render_stable_summary(
    slide: &Slide,
    profile: &StableRenderProfile,
    _motif: StableMotif,
    detail_level: StableDetailLevel,
) -> Result<StablePageDraft, AppError> {
    let tokens = &profile.tokens;
    let mut draft = StablePageDraft::new();
    append_standard_header(
        &mut draft,
        slide,
        tokens,
        false,
        profile.local_repair.as_ref(),
    );
    let core_rect = StableRect {
        x: 448.0,
        y: 286.0,
        width: 384.0,
        height: 238.0,
    };
    draft.body.push_str(&format!(
        "<circle cx=\"640\" cy=\"405\" r=\"158\" fill=\"{}\"/><circle cx=\"640\" cy=\"405\" r=\"118\" fill=\"{}\"/><text x=\"640\" y=\"374\" text-anchor=\"middle\" font-family=\"{}\" font-size=\"76\" font-weight=\"800\" fill=\"{}\">{:02}</text>\n",
        tokens.panel,
        tokens.background,
        tokens.font_family,
        tokens.subtle,
        slide.page
    ));
    draft.push_rect("summary-core", core_rect, StableElementKind::Card);
    append_fitted_text(
        &mut draft,
        "summary-core-label",
        if slide.page_theme.trim().is_empty() {
            "TAKEAWAY"
        } else {
            &slide.page_theme
        },
        StableRect {
            x: 496.0,
            y: 310.0,
            width: 288.0,
            height: 28.0,
        },
        15.0,
        13.0,
        1.1,
        &tokens.primary,
        "700",
        "middle",
        true,
        StableElementKind::Text,
        Some(core_rect),
    );
    let core_fit = append_fitted_text(
        &mut draft,
        "summary-message",
        &stable_core_message(slide),
        StableRect {
            x: 492.0,
            y: 382.0,
            width: 296.0,
            height: 116.0,
        },
        26.0,
        19.0,
        1.26,
        &tokens.text,
        "700",
        "middle",
        true,
        StableElementKind::Text,
        Some(core_rect),
    );
    if core_fit.overflowed {
        draft
            .hard_failures
            .push("summary core overflow".to_string());
    }
    let blocks = slide_blocks(slide);
    let node_rects = [
        StableRect {
            x: 56.0,
            y: 196.0,
            width: 330.0,
            height: 154.0,
        },
        StableRect {
            x: 894.0,
            y: 196.0,
            width: 330.0,
            height: 154.0,
        },
        StableRect {
            x: 56.0,
            y: 472.0,
            width: 330.0,
            height: 154.0,
        },
        StableRect {
            x: 894.0,
            y: 472.0,
            width: 330.0,
            height: 154.0,
        },
    ];
    let connector_points = [
        (386.0, 273.0, 448.0, 334.0),
        (894.0, 273.0, 832.0, 334.0),
        (386.0, 549.0, 448.0, 476.0),
        (894.0, 549.0, 832.0, 476.0),
    ];
    let count = blocks.len().min(4);
    for (index, block) in blocks.iter().take(count).enumerate() {
        let rect = node_rects[index];
        let (x1, y1, x2, y2) = connector_points[index];
        let accent = if index % 2 == 0 {
            &tokens.primary
        } else {
            &tokens.accent
        };
        draft.body.push_str(&format!(
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"2\"/><circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"7\" fill=\"{}\"/><line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{}\" stroke-width=\"5\"/>\n",
            x1,
            y1,
            x2,
            y2,
            tokens.border,
            x1,
            y1,
            accent,
            rect.x,
            rect.y + 4.0,
            rect.x + 78.0,
            rect.y + 4.0,
            accent
        ));
        let id = format!("summary-node-{}", index + 1);
        draft.push_rect(&id, rect, StableElementKind::Card);
        let label_fit = append_fitted_text(
            &mut draft,
            &format!("{}-label", id),
            &block.label,
            StableRect {
                x: rect.x + 12.0,
                y: rect.y + 18.0,
                width: rect.width - 24.0,
                height: 42.0,
            },
            20.0,
            16.0,
            1.16,
            accent,
            "700",
            "start",
            true,
            StableElementKind::Text,
            Some(rect),
        );
        let text_fit = append_fitted_text(
            &mut draft,
            &format!("{}-text", id),
            &block.text,
            StableRect {
                x: rect.x + 12.0,
                y: rect.y + 70.0,
                width: rect.width - 24.0,
                height: 72.0,
            },
            if detail_level == StableDetailLevel::Essential {
                15.0
            } else {
                17.0
            },
            13.0,
            1.24,
            &tokens.text,
            "500",
            "start",
            false,
            StableElementKind::Text,
            Some(rect),
        );
        if label_fit.overflowed || text_fit.overflowed {
            draft
                .hard_failures
                .push(format!("{} required text overflow", id));
        }
        draft.rendered_blocks.push(StableRenderedBlock {
            id: id.clone(),
            rect,
            label_complete: !block.label.trim().is_empty() && !label_fit.overflowed,
            text_complete: !block.text.trim().is_empty() && !text_fit.overflowed,
        });
        draft.push_decoration(
            &format!("{}-connector", id),
            StableDecorationPurpose::Connector,
            stable_line_rect(x1, y1, x2, y2, 2.0),
            &[&id, "summary-core"],
        );
    }
    Ok(draft)
}

fn render_slide_svg(plan: &SlidePlan, slide: &Slide) -> String {
    let palette = palette_for_style(&plan.style);
    let layout = effective_render_layout(slide, plan.slides.len());
    let body = match layout.as_str() {
        "cover" => render_cover_slide(plan, slide, &palette),
        "section" => render_section_slide(plan, slide, &palette),
        "timeline" => render_timeline_slide(plan, slide, &palette),
        "compare" => render_compare_slide(plan, slide, &palette),
        "process" => render_process_slide(plan, slide, &palette),
        "matrix" => render_matrix_slide(plan, slide, &palette),
        "highlight" => render_highlight_slide(plan, slide, &palette),
        "image_text" => render_image_text_slide(plan, slide, &palette),
        "summary" => render_summary_slide(plan, slide, &palette),
        _ => render_cards_slide(plan, slide, &palette),
    };
    render_svg_shell(plan, slide, &palette, &body)
}

fn render_svg_shell(plan: &SlidePlan, slide: &Slide, palette: &Palette, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 900" width="1600" height="900">
<defs>
  <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0%" stop-color="{bg1}"/>
    <stop offset="100%" stop-color="{bg2}"/>
  </linearGradient>
  <linearGradient id="accentGrad" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0%" stop-color="{accent}"/>
    <stop offset="100%" stop-color="{accent2}"/>
  </linearGradient>
  <pattern id="dotGrid" width="28" height="28" patternUnits="userSpaceOnUse">
    <circle cx="2" cy="2" r="1.4" fill="{line}" opacity="0.55"/>
  </pattern>
</defs>
<rect width="1600" height="900" fill="url(#bg)"/>
<rect x="54" y="48" width="1492" height="780" rx="20" fill="none" stroke="{line}" stroke-width="1.4" opacity="0.75"/>
<circle cx="1415" cy="118" r="132" fill="{accent}" opacity="0.08"/>
<circle cx="1485" cy="184" r="58" fill="{accent2}" opacity="0.12"/>
<rect x="1190" y="570" width="310" height="180" rx="34" fill="url(#dotGrid)" opacity="0.36"/>
{body}
{evidence}
{footer}
</svg>
"#,
        bg1 = palette.bg1,
        bg2 = palette.bg2,
        accent = palette.accent,
        accent2 = palette.accent2,
        line = palette.line,
        body = body,
        evidence = render_evidence_annotation(slide, palette),
        footer = render_footer(plan, slide, palette)
    )
}

fn render_evidence_annotation(slide: &Slide, palette: &Palette) -> String {
    let note = slide_evidence_note(slide);
    if note.trim().is_empty() {
        return String::new();
    }
    format!(
        r#"<g id="evidence-note">
<text x="92" y="812" font-size="14" fill="{muted}">{note}</text>
</g>
"#,
        muted = palette.muted,
        note = xml_escape(&note.chars().take(48).collect::<String>())
    )
}

fn effective_render_layout(slide: &Slide, total: usize) -> String {
    let idx = slide.page.saturating_sub(1);
    if idx == 0 {
        return "cover".to_string();
    }
    if idx + 1 == total {
        return "summary".to_string();
    }
    let relation = slide.relation.trim();
    let chart = slide.chart_type.trim();
    if relation == "timeline" || chart == "timeline" {
        "timeline".to_string()
    } else if relation == "compare" || chart == "compare" {
        "compare".to_string()
    } else if relation == "process" || chart == "process" {
        "process".to_string()
    } else if relation == "cause" || chart == "highlight" {
        "highlight".to_string()
    } else if relation == "category" || chart == "cards" {
        "cards".to_string()
    } else {
        normalize_layout(&slide.layout, idx, total)
    }
}

fn render_header(slide: &Slide, palette: &Palette) -> String {
    format!(
        r#"<g id="header">
<text x="92" y="104" font-size="44" font-weight="700" fill="{title}">{slide_title}</text>
<text x="94" y="148" font-size="22" fill="{muted}">{subtitle}</text>
<line x1="92" y1="180" x2="1508" y2="180" stroke="{line}" stroke-width="2"/>
<rect x="92" y="176" width="210" height="6" rx="3" fill="{accent}"/>
</g>
"#,
        title = palette.title,
        muted = palette.muted,
        line = palette.line,
        accent = palette.accent,
        slide_title = xml_escape(&slide.title),
        subtitle = xml_escape(&stable_core_message(slide))
    )
}

fn render_footer(plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    format!(
        r#"<g id="footer">
<text x="92" y="858" font-size="18" fill="{muted}">{page:02} / {total:02}</text>
<text x="1508" y="858" text-anchor="end" font-size="18" fill="{muted}">{deck_title}</text>
</g>"#,
        muted = palette.muted,
        page = slide.page,
        total = plan.slides.len(),
        deck_title = xml_escape(&plan.title)
    )
}

fn render_cover_slide(plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    format!(
        r#"<g id="cover">
<rect x="96" y="108" width="910" height="600" rx="34" fill="{surface}" opacity="0.96"/>
<rect x="96" y="108" width="10" height="600" rx="5" fill="url(#accentGrad)"/>
<line x1="152" y1="196" x2="360" y2="196" stroke="{accent}" stroke-width="6" stroke-linecap="round"/>
{title}
{subtitle}
<text x="154" y="620" font-size="22" fill="{muted}">{audience}</text>
<text x="154" y="670" font-size="20" fill="{muted}">{deck_title}</text>
<g id="cover-visual">
  <circle cx="1238" cy="342" r="176" fill="url(#accentGrad)" opacity="0.20"/>
  <circle cx="1238" cy="342" r="112" fill="{surface}" opacity="0.88"/>
  <circle cx="1238" cy="342" r="62" fill="{accent}" opacity="0.30"/>
  <path d="M1110 506 C1195 448 1275 596 1410 510" fill="none" stroke="{accent2}" stroke-width="12" stroke-linecap="round" opacity="0.64"/>
  <path d="M1088 248 L1396 176 L1452 450 L1164 544 Z" fill="none" stroke="{line}" stroke-width="3" opacity="0.95"/>
  <rect x="1095" y="612" width="330" height="56" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
  <text x="1260" y="648" text-anchor="middle" font-size="20" fill="{text}">{style}</text>
</g>
</g>
"#,
        surface = palette.surface,
        accent = palette.accent,
        accent2 = palette.accent2,
        line = palette.line,
        muted = palette.muted,
        text = palette.text,
        title = render_wrapped_text(&slide.title, 154, 322, 14, 68, 78, palette.title, "700", 3),
        subtitle = render_wrapped_text(
            &slide.subtitle,
            158,
            442,
            24,
            30,
            42,
            palette.muted,
            "400",
            2
        ),
        audience = xml_escape(&plan.audience),
        style = xml_escape(&plan.style),
        deck_title = xml_escape(&plan.title)
    )
}

fn render_section_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    format!(
        r#"<g id="section">
<rect x="108" y="170" width="1384" height="520" rx="36" fill="{surface}" opacity="0.94"/>
<text x="160" y="294" font-size="112" font-weight="800" fill="{accent}" opacity="0.22">{page:02}</text>
<line x1="160" y1="342" x2="510" y2="342" stroke="{accent}" stroke-width="8" stroke-linecap="round"/>
{title}
{subtitle}
</g>
"#,
        surface = palette.surface,
        accent = palette.accent,
        page = slide.page,
        title = render_wrapped_text(&slide.title, 160, 450, 18, 64, 76, palette.title, "700", 2),
        subtitle = render_wrapped_text(
            &slide.subtitle,
            164,
            562,
            30,
            28,
            40,
            palette.muted,
            "400",
            2
        )
    )
}

fn render_cards_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let mut cards = String::new();
    let count = blocks.len().clamp(2, 6);
    let layout = card_grid_layout(count);
    for (idx, block) in blocks.iter().take(count).enumerate() {
        let (x, y, w, h) = layout[idx];
        cards.push_str(&render_content_card(
            block,
            slide.evidence.get(idx).map(String::as_str),
            x,
            y,
            w,
            h,
            idx + 1,
            palette,
            "card",
        ));
    }
    format!("{}{}", render_header(slide, palette), cards)
}

#[allow(unreachable_code)]
fn render_timeline_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    return render_timeline_slide_rich(slide, palette);
    let bullets = slide_bullets(slide);
    let blocks = slide_blocks(slide);
    let count = blocks.len().clamp(3, 5);
    let start_x = 170;
    let gap = 1260 / (count.saturating_sub(1).max(1) as i32);
    let mut nodes = String::new();
    nodes.push_str(&format!(
        r#"<line x1="{start_x}" y1="420" x2="1430" y2="420" stroke="{line}" stroke-width="8" stroke-linecap="round"/>
<line x1="{start_x}" y1="420" x2="{}" y2="420" stroke="{accent}" stroke-width="8" stroke-linecap="round"/>
"#,
        start_x + gap * ((count - 1) as i32),
        line = palette.line,
        accent = palette.accent
    ));
    for idx in 0..count {
        let x = start_x + gap * idx as i32;
        let y_text = if idx % 2 == 0 { 300 } else { 520 };
        let text_value = bullets
            .get(idx)
            .cloned()
            .unwrap_or_else(|| format!("阶段 {}", idx + 1));
        nodes.push_str(&format!(
            r##"<g id="timeline-{idx}">
<circle cx="{x}" cy="420" r="22" fill="{accent}"/>
<circle cx="{x}" cy="420" r="9" fill="#ffffff"/>
<rect x="{rx}" y="{ry}" width="250" height="118" rx="18" fill="{surface}" stroke="{line}" stroke-width="2"/>
{text}
</g>
"##,
            idx = idx + 1,
            x = x,
            rx = x - 125,
            ry = y_text - 52,
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            surface = palette.surface,
            line = palette.line,
            text = render_wrapped_text(&text_value, x - 102, y_text - 8, 12, 22, 31, palette.text, "600", 3)
        ));
    }
    format!("{}{}", render_header(slide, palette), nodes)
}

#[allow(unreachable_code)]
fn render_compare_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    return render_compare_slide_rich(slide, palette);
    let (left, right) = split_bullets(&slide_bullets(slide));
    format!(
        r#"{header}
<g id="compare">
<rect x="104" y="238" width="645" height="486" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
<rect x="851" y="238" width="645" height="486" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
<line x1="800" y1="278" x2="800" y2="690" stroke="{accent}" stroke-width="4" opacity="0.38"/>
<text x="152" y="304" font-size="32" font-weight="700" fill="{title}">现状 / 问题</text>
<text x="899" y="304" font-size="32" font-weight="700" fill="{title}">方案 / 变化</text>
{left}
{right}
</g>
"#,
        header = render_header(slide, palette),
        surface = palette.surface,
        line = palette.line,
        accent = palette.accent,
        title = palette.title,
        left = render_bullet_list(&left, 152, 370, 510, palette, "left"),
        right = render_bullet_list(&right, 899, 370, 510, palette, "right")
    )
}

#[allow(unreachable_code)]
fn render_process_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    return render_process_slide_rich(slide, palette);
    let bullets = slide_bullets(slide);
    let count = bullets.len().clamp(3, 5);
    let step_w = 240;
    let gap = if count <= 3 { 120 } else { 48 };
    let total_w = count as i32 * step_w + (count as i32 - 1) * gap;
    let start_x = (1600 - total_w) / 2;
    let mut steps = String::new();
    for idx in 0..count {
        let x = start_x + idx as i32 * (step_w + gap);
        let text_value = bullets
            .get(idx)
            .cloned()
            .unwrap_or_else(|| format!("步骤 {}", idx + 1));
        steps.push_str(&format!(
            r##"<g id="process-{idx}">
<rect x="{x}" y="325" width="{step_w}" height="250" rx="26" fill="{surface}" stroke="{line}" stroke-width="2"/>
<circle cx="{cx}" cy="325" r="34" fill="{accent}"/>
<text x="{cx}" y="334" text-anchor="middle" font-size="24" font-weight="700" fill="#ffffff">{num}</text>
{text}
</g>
"##,
            idx = idx + 1,
            x = x,
            step_w = step_w,
            cx = x + step_w / 2,
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            surface = palette.surface,
            line = palette.line,
            num = idx + 1,
            text = render_wrapped_text(&text_value, x + 28, 412, 10, 23, 34, palette.text, "600", 4)
        ));
        if idx + 1 < count {
            steps.push_str(&format!(
                r#"<path d="M{} 450 L{} 450" stroke="{}" stroke-width="4" stroke-linecap="round"/>
<path d="M{} 436 L{} 450 L{} 464" fill="none" stroke="{}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
"#,
                x + step_w + 12,
                x + step_w + gap - 18,
                palette.accent,
                x + step_w + gap - 34,
                x + step_w + gap - 18,
                x + step_w + gap - 34,
                palette.accent
            ));
        }
    }
    format!("{}{}", render_header(slide, palette), steps)
}

#[allow(unreachable_code)]
fn render_matrix_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    return render_matrix_slide_rich(slide, palette);
    let bullets = slide_bullets(slide);
    let positions = [(160, 250), (850, 250), (160, 520), (850, 520)];
    let mut cells = String::new();
    for idx in 0..4 {
        let bullet = bullets
            .get(idx)
            .cloned()
            .unwrap_or_else(|| format!("关键维度 {}", idx + 1));
        let (x, y) = positions[idx];
        cells.push_str(&format!(
            r#"<g id="matrix-{idx}">
<rect x="{x}" y="{y}" width="590" height="210" rx="22" fill="{surface}" stroke="{line}" stroke-width="2"/>
<rect x="{x}" y="{y}" width="12" height="210" rx="6" fill="{accent}"/>
<text x="{tx}" y="{ty}" font-size="28" font-weight="700" fill="{accent}">0{num}</text>
{text}
</g>
"#,
            idx = idx + 1,
            x = x,
            y = y,
            tx = x + 38,
            ty = y + 58,
            num = idx + 1,
            surface = palette.surface,
            line = palette.line,
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            text = render_wrapped_text(&bullet, x + 118, y + 58, 18, 24, 34, palette.text, "600", 3)
        ));
    }
    format!("{}{}", render_header(slide, palette), cells)
}

#[allow(unreachable_code)]
fn render_highlight_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    return render_highlight_slide_rich(slide, palette);
    let bullets = slide_bullets(slide);
    let keyword = bullets.first().map(String::as_str).unwrap_or(&slide.title);
    let rest: Vec<String> = bullets.iter().skip(1).take(3).cloned().collect();
    format!(
        r##"{header}
<g id="highlight">
<rect x="116" y="244" width="620" height="476" rx="34" fill="url(#accentGrad)" opacity="0.92"/>
<text x="166" y="376" font-size="32" font-weight="700" fill="#ffffff">核心强调</text>
{keyword}
<rect x="820" y="278" width="600" height="390" rx="30" fill="{surface}" stroke="{line}" stroke-width="2"/>
{bullets}
</g>
"##,
        header = render_header(slide, palette),
        surface = palette.surface,
        line = palette.line,
        keyword = render_wrapped_text(keyword, 166, 500, 8, 76, 88, "#ffffff", "800", 3),
        bullets = render_bullet_list(&rest, 870, 360, 460, palette, "highlight")
    )
}

#[allow(unreachable_code)]
fn render_image_text_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    return render_mixed_slide_rich(slide, palette);
    let bullets = slide_bullets(slide);
    format!(
        r#"{header}
<g id="image-text">
<rect x="112" y="236" width="570" height="496" rx="32" fill="{surface}" stroke="{line}" stroke-width="2"/>
<rect x="156" y="292" width="482" height="330" rx="28" fill="url(#accentGrad)" opacity="0.16"/>
<path d="M206 570 L315 418 L410 520 L486 390 L610 570 Z" fill="{accent}" opacity="0.28"/>
<circle cx="500" cy="368" r="58" fill="{accent2}" opacity="0.24"/>
<line x1="180" y1="654" x2="614" y2="654" stroke="{line}" stroke-width="3"/>
<text x="185" y="688" font-size="20" fill="{muted}">{hint}</text>
<rect x="774" y="254" width="690" height="448" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
{bullets}
</g>
"#,
        header = render_header(slide, palette),
        surface = palette.surface,
        line = palette.line,
        accent = palette.accent,
        accent2 = palette.accent2,
        muted = palette.muted,
        hint = xml_escape(if slide.visual_hint.trim().is_empty() {
            "抽象视觉占位"
        } else {
            &slide.visual_hint
        }),
        bullets = render_bullet_list(&bullets, 830, 336, 540, palette, "image-text")
    )
}

#[allow(unreachable_code)]
fn render_summary_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    return render_summary_slide_rich(slide, palette);
    let bullets = slide_bullets(slide);
    let mut takes = String::new();
    for (idx, bullet) in bullets.iter().take(3).enumerate() {
        let x = 150 + idx as i32 * 440;
        takes.push_str(&format!(
            r##"<g id="takeaway-{idx}">
<circle cx="{cx}" cy="375" r="84" fill="{accent}" opacity="0.14"/>
<circle cx="{cx}" cy="375" r="48" fill="{accent}"/>
<text x="{cx}" y="388" text-anchor="middle" font-size="30" font-weight="800" fill="#ffffff">{num}</text>
<rect x="{x}" y="482" width="360" height="168" rx="24" fill="{surface}" stroke="{line}" stroke-width="2"/>
{text}
</g>
"##,
            idx = idx + 1,
            x = x,
            cx = x + 180,
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            num = idx + 1,
            surface = palette.surface,
            line = palette.line,
            text = render_wrapped_text(bullet, x + 32, 548, 14, 24, 34, palette.text, "600", 3)
        ));
    }
    format!(
        r#"<g id="summary">
<text x="104" y="138" font-size="52" font-weight="700" fill="{title}">{title_text}</text>
<text x="108" y="184" font-size="24" fill="{muted}">{subtitle}</text>
<line x1="104" y1="220" x2="620" y2="220" stroke="{accent}" stroke-width="7" stroke-linecap="round"/>
{takes}
<text x="800" y="744" text-anchor="middle" font-size="28" font-weight="700" fill="{title}">谢谢观看</text>
</g>
"#,
        title = palette.title,
        title_text = xml_escape(&slide.title),
        muted = palette.muted,
        subtitle = xml_escape(&slide.subtitle),
        accent = palette.accent,
        takes = takes
    )
}

fn render_timeline_slide_rich(slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let count = blocks.len().clamp(3, 5);
    let start_x = 170;
    let gap = 1260 / (count.saturating_sub(1).max(1) as i32);
    let mut nodes = String::new();
    nodes.push_str(&format!(
        r#"<line x1="{start_x}" y1="420" x2="1430" y2="420" stroke="{line}" stroke-width="7" stroke-linecap="round"/>
<line x1="{start_x}" y1="420" x2="{}" y2="420" stroke="{accent}" stroke-width="7" stroke-linecap="round"/>
"#,
        start_x + gap * ((count - 1) as i32),
        line = palette.line,
        accent = palette.accent
    ));
    for idx in 0..count {
        let x = start_x + gap * idx as i32;
        let y_text = if idx % 2 == 0 { 284 } else { 524 };
        let block = blocks.get(idx).cloned().unwrap_or_default();
        nodes.push_str(&format!(
            r##"<g id="timeline-rich-{idx}">
<circle cx="{x}" cy="420" r="24" fill="{accent}"/>
<circle cx="{x}" cy="420" r="9" fill="#ffffff"/>
<rect x="{rx}" y="{ry}" width="278" height="164" rx="18" fill="{surface}" stroke="{line}" stroke-width="2"/>
<text x="{tx}" y="{ly}" font-size="20" font-weight="700" fill="{accent}">{label}</text>
{text}
{detail}
</g>
"##,
            idx = idx + 1,
            x = x,
            rx = x - 139,
            ry = y_text - 72,
            tx = x - 110,
            ly = y_text - 28,
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            surface = palette.surface,
            line = palette.line,
            label = xml_escape(&short_text(&block.label, 13)),
            text = render_wrapped_text(&block.text, x - 110, y_text + 10, 16, 19, 26, palette.text, "600", 2),
            detail = render_wrapped_text(&block.detail, x - 110, y_text + 64, 18, 15, 20, palette.muted, "400", 2)
        ));
    }
    format!("{}{}", render_header(slide, palette), nodes)
}

fn render_compare_slide_rich(slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let (left, right) = split_blocks(&blocks);
    format!(
        r#"{header}
<g id="compare-rich">
<rect x="104" y="238" width="645" height="486" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
<rect x="851" y="238" width="645" height="486" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
<line x1="800" y1="278" x2="800" y2="690" stroke="{accent}" stroke-width="4" opacity="0.38"/>
<text x="152" y="304" font-size="30" font-weight="700" fill="{title}">A</text>
<text x="899" y="304" font-size="30" font-weight="700" fill="{title}">B</text>
{left}
{right}
</g>
"#,
        header = render_header(slide, palette),
        surface = palette.surface,
        line = palette.line,
        accent = palette.accent,
        title = palette.title,
        left = render_block_stack(&left, 152, 350, 540, 3, palette, "compare-left"),
        right = render_block_stack(&right, 899, 350, 540, 3, palette, "compare-right")
    )
}

fn render_process_slide_rich(slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let count = blocks.len().clamp(3, 5);
    let step_w = if count <= 3 { 310 } else { 250 };
    let gap = if count <= 3 { 88 } else { 38 };
    let total_w = count as i32 * step_w + (count as i32 - 1) * gap;
    let start_x = (1600 - total_w) / 2;
    let mut steps = String::new();
    for idx in 0..count {
        let x = start_x + idx as i32 * (step_w + gap);
        let block = blocks.get(idx).cloned().unwrap_or_default();
        steps.push_str(&format!(
            r##"<g id="process-rich-{idx}">
<rect x="{x}" y="288" width="{step_w}" height="338" rx="26" fill="{surface}" stroke="{line}" stroke-width="2"/>
<circle cx="{cx}" cy="288" r="34" fill="{accent}"/>
<text x="{cx}" y="297" text-anchor="middle" font-size="24" font-weight="700" fill="#ffffff">{num}</text>
<text x="{tx}" y="360" font-size="22" font-weight="700" fill="{title}">{label}</text>
{text}
{detail}
</g>
"##,
            idx = idx + 1,
            x = x,
            step_w = step_w,
            cx = x + step_w / 2,
            tx = x + 28,
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            surface = palette.surface,
            line = palette.line,
            title = palette.title,
            num = idx + 1,
            label = xml_escape(&short_text(&block.label, 12)),
            text = render_wrapped_text(&block.text, x + 28, 404, 13, 20, 28, palette.text, "600", 3),
            detail = render_wrapped_text(&block.detail, x + 28, 512, 16, 15, 22, palette.muted, "400", 3)
        ));
        if idx + 1 < count {
            steps.push_str(&format!(
                r#"<path d="M{} 456 L{} 456" stroke="{}" stroke-width="4" stroke-linecap="round"/>
<path d="M{} 442 L{} 456 L{} 470" fill="none" stroke="{}" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
"#,
                x + step_w + 12,
                x + step_w + gap - 18,
                palette.accent,
                x + step_w + gap - 34,
                x + step_w + gap - 18,
                x + step_w + gap - 34,
                palette.accent
            ));
        }
    }
    format!("{}{}", render_header(slide, palette), steps)
}

fn render_matrix_slide_rich(slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let positions = [
        (132, 248, 640, 210),
        (828, 248, 640, 210),
        (132, 526, 640, 210),
        (828, 526, 640, 210),
    ];
    let mut cells = String::new();
    for (idx, (x, y, w, h)) in positions.iter().enumerate() {
        let block = blocks.get(idx).cloned().unwrap_or_default();
        cells.push_str(&render_content_card(
            &block,
            slide.evidence.get(idx).map(String::as_str),
            *x,
            *y,
            *w,
            *h,
            idx + 1,
            palette,
            "matrix",
        ));
    }
    format!("{}{}", render_header(slide, palette), cells)
}

fn render_highlight_slide_rich(slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let lead = stable_core_message(slide);
    let support: Vec<ContentBlock> = blocks.into_iter().take(3).collect();
    format!(
        r##"{header}
<g id="highlight-rich">
<rect x="108" y="236" width="650" height="500" rx="34" fill="url(#accentGrad)" opacity="0.94"/>
<text x="162" y="318" font-size="24" font-weight="700" fill="#ffffff">Core message</text>
{lead}
<rect x="840" y="260" width="580" height="440" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
{support}
</g>
"##,
        header = render_header(slide, palette),
        surface = palette.surface,
        line = palette.line,
        lead = render_wrapped_text(&lead, 162, 430, 11, 54, 66, "#ffffff", "800", 4),
        support = render_block_stack(&support, 890, 330, 450, 3, palette, "highlight-support")
    )
}

fn render_mixed_slide_rich(slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let lead = stable_core_message(slide);
    let right: Vec<ContentBlock> = blocks.into_iter().take(3).collect();
    format!(
        r#"{header}
<g id="mixed-rich">
<rect x="112" y="244" width="560" height="480" rx="32" fill="{surface}" stroke="{line}" stroke-width="2"/>
<circle cx="248" cy="362" r="86" fill="{accent}" opacity="0.14"/>
<path d="M178 558 C300 420 405 665 580 492" fill="none" stroke="{accent}" stroke-width="10" stroke-linecap="round" opacity="0.32"/>
{lead}
<rect x="752" y="244" width="730" height="480" rx="30" fill="{surface}" stroke="{line}" stroke-width="2"/>
{right}
</g>
"#,
        header = render_header(slide, palette),
        surface = palette.surface,
        line = palette.line,
        accent = palette.accent,
        lead = render_wrapped_text(&lead, 162, 430, 13, 38, 52, palette.title, "800", 4),
        right = render_block_stack(&right, 810, 318, 580, 3, palette, "mixed-block")
    )
}

fn render_summary_slide_rich(slide: &Slide, palette: &Palette) -> String {
    let blocks = slide_blocks(slide);
    let mut takes = String::new();
    for (idx, block) in blocks.iter().take(4).enumerate() {
        let x = 122 + idx as i32 * 360;
        takes.push_str(&render_content_card(
            block,
            slide.evidence.get(idx).map(String::as_str),
            x,
            438,
            318,
            230,
            idx + 1,
            palette,
            "summary",
        ));
    }
    format!(
        r#"<g id="summary-rich">
<text x="104" y="138" font-size="52" font-weight="700" fill="{title}">{title_text}</text>
<text x="108" y="184" font-size="24" fill="{muted}">{subtitle}</text>
<line x1="104" y1="220" x2="620" y2="220" stroke="{accent}" stroke-width="7" stroke-linecap="round"/>
<rect x="116" y="260" width="1368" height="112" rx="28" fill="{surface}" stroke="{line}" stroke-width="2"/>
{core}
{takes}
</g>
"#,
        title = palette.title,
        title_text = xml_escape(&slide.title),
        muted = palette.muted,
        subtitle = xml_escape(&slide.subtitle),
        accent = palette.accent,
        surface = palette.surface,
        line = palette.line,
        core = render_wrapped_text(
            &stable_core_message(slide),
            162,
            324,
            42,
            28,
            38,
            palette.title,
            "700",
            2
        ),
        takes = takes
    )
}

fn slide_blocks(slide: &Slide) -> Vec<ContentBlock> {
    let mut blocks: Vec<ContentBlock> = slide
        .content_blocks
        .iter()
        .filter(|block| {
            !block.label.trim().is_empty()
                || !block.text.trim().is_empty()
                || !block.detail.trim().is_empty()
        })
        .cloned()
        .collect();
    if blocks.is_empty() {
        blocks = content_blocks_from_slide(slide);
    }
    if blocks.is_empty() {
        blocks.push(ContentBlock {
            label: slide.title.clone(),
            text: stable_core_message(slide),
            detail: slide.subtitle.clone(),
        });
    }
    blocks.truncate(6);
    blocks
}

fn card_grid_layout(count: usize) -> Vec<(i32, i32, i32, i32)> {
    match count {
        0 | 1 | 2 => vec![(150, 286, 600, 330), (850, 286, 600, 330)],
        3 => vec![
            (112, 284, 440, 330),
            (580, 284, 440, 330),
            (1048, 284, 440, 330),
        ],
        4 => vec![
            (132, 248, 640, 210),
            (828, 248, 640, 210),
            (132, 526, 640, 210),
            (828, 526, 640, 210),
        ],
        _ => vec![
            (104, 238, 430, 226),
            (586, 238, 430, 226),
            (1068, 238, 430, 226),
            (104, 520, 430, 226),
            (586, 520, 430, 226),
            (1068, 520, 430, 226),
        ],
    }
}

fn render_content_card(
    block: &ContentBlock,
    evidence: Option<&str>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    index: usize,
    palette: &Palette,
    id: &str,
) -> String {
    let accent = if index % 2 == 0 {
        palette.accent2
    } else {
        palette.accent
    };
    let label = if block.label.trim().is_empty() {
        format!("Point {}", index)
    } else {
        short_text(&block.label, 16)
    };
    let text = if block.text.trim().is_empty() {
        block.detail.trim()
    } else {
        block.text.trim()
    };
    let detail = if block.detail.trim().is_empty() {
        evidence.unwrap_or("").trim()
    } else {
        block.detail.trim()
    };
    let tag = evidence
        .map(|item| short_text(item, 18))
        .filter(|item| !item.trim().is_empty())
        .unwrap_or_default();
    let tag_svg = if tag.is_empty() {
        String::new()
    } else {
        format!(
            r#"<rect x="{tx}" y="{ty}" width="{tw}" height="24" rx="12" fill="{accent}" opacity="0.10"/>
<text x="{text_x}" y="{text_y}" font-size="13" fill="{accent}">{tag}</text>"#,
            tx = x + 28,
            ty = y + h - 42,
            tw = (tag.chars().count() as i32 * 13 + 28).clamp(86, w - 56),
            text_x = x + 42,
            text_y = y + h - 25,
            accent = accent,
            tag = xml_escape(&tag)
        )
    };
    format!(
        r##"<g id="{id}-{index}">
<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="22" fill="{surface}" stroke="{line}" stroke-width="2"/>
<rect x="{x}" y="{y}" width="8" height="{h}" rx="4" fill="{accent}"/>
<circle cx="{cx}" cy="{cy}" r="24" fill="{accent}"/>
<text x="{cx}" y="{num_y}" text-anchor="middle" font-size="18" font-weight="700" fill="#ffffff">{index}</text>
<text x="{label_x}" y="{label_y}" font-size="22" font-weight="700" fill="{title}">{label}</text>
{text_svg}
{detail_svg}
{tag_svg}
</g>
"##,
        id = id,
        index = index,
        x = x,
        y = y,
        w = w,
        h = h,
        surface = palette.surface,
        line = palette.line,
        accent = accent,
        cx = x + 42,
        cy = y + 42,
        num_y = y + 49,
        label_x = x + 82,
        label_y = y + 50,
        title = palette.title,
        label = xml_escape(&label),
        text_svg = render_wrapped_text(
            text,
            x + 30,
            y + 96,
            ((w - 60) / 19).max(12) as usize,
            20,
            28,
            palette.text,
            "600",
            3
        ),
        detail_svg = render_wrapped_text(
            detail,
            x + 30,
            y + h - 88,
            ((w - 60) / 15).max(14) as usize,
            15,
            21,
            palette.muted,
            "400",
            2
        ),
        tag_svg = tag_svg
    )
}

fn split_blocks(blocks: &[ContentBlock]) -> (Vec<ContentBlock>, Vec<ContentBlock>) {
    let mid = (blocks.len() + 1) / 2;
    let left = blocks[..mid].to_vec();
    let right = blocks[mid..].to_vec();
    let fallback_right = left.clone();
    (
        left,
        if right.is_empty() {
            fallback_right
        } else {
            right
        },
    )
}

fn render_block_stack(
    blocks: &[ContentBlock],
    x: i32,
    y: i32,
    w: i32,
    max_items: usize,
    palette: &Palette,
    id: &str,
) -> String {
    let mut out = String::new();
    for (idx, block) in blocks.iter().take(max_items).enumerate() {
        let item_y = y + idx as i32 * 116;
        let accent = if idx % 2 == 0 {
            palette.accent
        } else {
            palette.accent2
        };
        out.push_str(&format!(
            r#"<g id="{id}-{idx}">
<circle cx="{cx}" cy="{cy}" r="10" fill="{accent}"/>
<text x="{tx}" y="{ly}" font-size="21" font-weight="700" fill="{title}">{label}</text>
{text}
{detail}
</g>
"#,
            id = id,
            idx = idx + 1,
            cx = x,
            cy = item_y - 8,
            accent = accent,
            tx = x + 28,
            ly = item_y,
            title = palette.title,
            label = xml_escape(&short_text(&block.label, 15)),
            text = render_wrapped_text(
                &block.text,
                x + 28,
                item_y + 34,
                ((w - 40) / 18).max(12) as usize,
                18,
                25,
                palette.text,
                "500",
                2
            ),
            detail = render_wrapped_text(
                &block.detail,
                x + 28,
                item_y + 82,
                ((w - 40) / 15).max(14) as usize,
                14,
                19,
                palette.muted,
                "400",
                1
            )
        ));
    }
    out
}

fn short_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}

fn choose_layout(
    idx: usize,
    total: usize,
    title: &str,
    subtitle: &str,
    bullets: &[String],
) -> String {
    if idx == 0 {
        return "cover".to_string();
    }
    if idx + 1 == total {
        return "summary".to_string();
    }
    let text = format!("{} {} {}", title, subtitle, bullets.join(" "));
    if text.contains("年")
        || text.contains("阶段")
        || text.contains("历程")
        || text.contains("路径")
    {
        "timeline".to_string()
    } else if text.contains("对比")
        || text.contains("前后")
        || text.contains("传统")
        || text.contains("问题")
    {
        "compare".to_string()
    } else if text.contains("流程")
        || text.contains("步骤")
        || text.contains("机制")
        || text.contains("闭环")
    {
        "process".to_string()
    } else if text.contains("数据") || text.contains("亮点") || text.contains("核心") {
        "highlight".to_string()
    } else if bullets.len() >= 4 {
        "matrix".to_string()
    } else {
        ["cards", "image_text", "process", "compare", "highlight"][idx % 5].to_string()
    }
}

fn normalize_layout(layout: &str, idx: usize, total: usize) -> String {
    if idx == 0 {
        return "cover".to_string();
    }
    if idx + 1 == total {
        return "summary".to_string();
    }
    match layout.trim() {
        "cover" | "section" | "cards" | "timeline" | "compare" | "process" | "matrix"
        | "highlight" | "image_text" | "summary" => layout.trim().to_string(),
        _ => "cards".to_string(),
    }
}

fn visual_hint_for_layout(layout: &str) -> &'static str {
    match layout {
        "cover" => "大标题 + 抽象科技圆形装饰",
        "section" => "章节过渡页",
        "timeline" => "横向时间线",
        "compare" => "左右对比",
        "process" => "箭头流程",
        "matrix" => "2x2 四象限",
        "highlight" => "大关键词强调",
        "image_text" => "抽象图形 + 文本说明",
        "summary" => "三个核心 takeaway",
        _ => "编号卡片",
    }
}

fn default_theme() -> Theme {
    theme_for_style("科技蓝")
}

fn theme_for_style(style: &str) -> Theme {
    let palette = palette_for_style(style);
    Theme {
        name: palette.name.to_string(),
        primary: palette.accent.to_string(),
        secondary: palette.accent2.to_string(),
        accent: palette.highlight.to_string(),
        background: palette.bg1.to_string(),
    }
}

#[derive(Clone)]
struct Palette {
    name: &'static str,
    bg1: &'static str,
    bg2: &'static str,
    surface: &'static str,
    title: &'static str,
    text: &'static str,
    muted: &'static str,
    line: &'static str,
    accent: &'static str,
    accent2: &'static str,
    highlight: &'static str,
}

fn palette_for_style(style: &str) -> Palette {
    if style.contains("科技") || style.contains("蓝") {
        Palette {
            name: "tech-blue",
            bg1: "#f8fbff",
            bg2: "#eef4ff",
            surface: "#ffffff",
            title: "#102033",
            text: "#1f2937",
            muted: "#52637a",
            line: "#bdd7ff",
            accent: "#2563eb",
            accent2: "#7c3aed",
            highlight: "#38bdf8",
        }
    } else if style.contains("竞赛") || style.contains("路演") {
        Palette {
            name: "pitch",
            bg1: "#fff7ed",
            bg2: "#ffffff",
            surface: "#ffffff",
            title: "#111827",
            text: "#243042",
            muted: "#6b7280",
            line: "#fed7aa",
            accent: "#111827",
            accent2: "#7c3aed",
            highlight: "#f97316",
        }
    } else if style.contains("学术") {
        Palette {
            name: "academic",
            bg1: "#f8fafc",
            bg2: "#eef4fb",
            surface: "#ffffff",
            title: "#1e3a8a",
            text: "#1f2a37",
            muted: "#64748b",
            line: "#c7d7ea",
            accent: "#1e3a8a",
            accent2: "#64748b",
            highlight: "#b91c1c",
        }
    } else if style.contains("图文") {
        Palette {
            name: "visual",
            bg1: "#f9fafb",
            bg2: "#ecfeff",
            surface: "#ffffff",
            title: "#12343b",
            text: "#243042",
            muted: "#5c6670",
            line: "#c7d2fe",
            accent: "#0f766e",
            accent2: "#2563eb",
            highlight: "#e11d48",
        }
    } else {
        Palette {
            name: "business",
            bg1: "#ffffff",
            bg2: "#f3f6fb",
            surface: "#ffffff",
            title: "#1f2937",
            text: "#263244",
            muted: "#667085",
            line: "#d0d7e2",
            accent: "#1f2937",
            accent2: "#2563eb",
            highlight: "#f59e0b",
        }
    }
}

fn slide_bullets(slide: &Slide) -> Vec<String> {
    let mut bullets: Vec<String> = slide
        .content_blocks
        .iter()
        .map(content_block_display)
        .filter(|item| !item.trim().is_empty())
        .take(6)
        .collect();
    if bullets.len() < 2 {
        bullets.extend(
            slide
                .bullets
                .iter()
                .map(|item| item.trim())
                .filter(|item| !item.is_empty())
                .map(ToString::to_string),
        );
    }
    if bullets.len() < 2 {
        bullets.extend(
            slide
                .evidence
                .iter()
                .map(|item| item.trim())
                .filter(|item| !item.is_empty())
                .map(ToString::to_string),
        );
    }
    bullets.truncate(if slide.density == "dense" { 6 } else { 4 });
    if bullets.is_empty() {
        bullets.push(stable_core_message(slide));
    }
    bullets
}

fn slide_evidence_note(slide: &Slide) -> String {
    slide
        .evidence
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .take(2)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn split_bullets(bullets: &[String]) -> (Vec<String>, Vec<String>) {
    let mid = (bullets.len() + 1) / 2;
    let left = bullets[..mid].to_vec();
    let right = bullets[mid..].to_vec();
    let fallback_right = left.clone();
    (
        left,
        if right.is_empty() {
            fallback_right
        } else {
            right
        },
    )
}

fn render_bullet_list(
    items: &[String],
    x: i32,
    y: i32,
    max_chars: usize,
    palette: &Palette,
    id: &str,
) -> String {
    let mut out = String::new();
    for (idx, item) in items.iter().take(5).enumerate() {
        let item_y = y + idx as i32 * 82;
        out.push_str(&format!(
            r#"<g id="{id}-bullet-{idx}">
<circle cx="{cx}" cy="{cy}" r="9" fill="{accent}"/>
{text}
</g>
"#,
            id = id,
            idx = idx + 1,
            cx = x,
            cy = item_y - 8,
            accent = if idx % 2 == 0 {
                palette.accent
            } else {
                palette.accent2
            },
            text = render_wrapped_text(
                item,
                x + 28,
                item_y,
                (max_chars / 24).max(12),
                22,
                31,
                palette.text,
                "500",
                2
            )
        ));
    }
    out
}

fn fill_missing_svgs_with_legacy_fallback(
    plan: &SlidePlan,
    svg_output: &Path,
    log_lines: &mut Vec<String>,
) -> Result<(), AppError> {
    let mut missing = Vec::new();
    for slide in &plan.slides {
        let filename = svg_filename_for_slide(slide);
        if !svg_output.join(&filename).is_file() {
            missing.push((filename, slide));
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    log_lines.push(format!(
        "legacy fallback started for missing SVG pages: {}",
        missing.len()
    ));
    for (filename, slide) in missing {
        write_file(&svg_output.join(filename), &render_slide_svg(plan, slide))?;
    }
    Ok(())
}

fn write_legacy_fallback_svgs(plan: &SlidePlan, svg_output: &Path) -> Result<(), AppError> {
    for slide in &plan.slides {
        let filename = svg_filename_for_slide(slide);
        write_file(&svg_output.join(filename), &render_slide_svg(plan, slide))?;
    }
    Ok(())
}

fn run_python_project_script(
    root: &Path,
    python_path: &str,
    script: &str,
    project: &Path,
    started: Instant,
) -> Result<PptMasterExportResult, AppError> {
    let script_path = root.join(script);
    let python = resolve_python_program(root, python_path);
    let mut cmd = Command::new(&python);
    cmd.current_dir(root).arg(&script_path).arg(project);
    add_no_window(&mut cmd);

    let output = cmd.output().map_err(|e| {
        AppError::Custom(format!("无法启动 {}: {} ({})", script, python.display(), e))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(PptMasterExportResult {
        success: output.status.success(),
        output_path: None,
        exit_code: output.status.code(),
        stdout,
        stderr,
        duration_ms: started.elapsed().as_millis(),
        error: if output.status.success() {
            None
        } else {
            Some(format!(
                "{} 执行失败，退出码: {}",
                script,
                output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "未知".to_string())
            ))
        },
    })
}

fn run_total_md_split(
    root: &Path,
    python_path: &str,
    project: &Path,
    started: Instant,
) -> Result<PptMasterExportResult, AppError> {
    run_python_project_script(root, python_path, TOTAL_MD_SPLIT_SCRIPT, project, started)
}

fn run_finalize_svg(
    root: &Path,
    python_path: &str,
    project: &Path,
    started: Instant,
) -> Result<PptMasterExportResult, AppError> {
    run_python_project_script(root, python_path, FINALIZE_SVG_SCRIPT, project, started)
}

fn render_wrapped_text(
    text: &str,
    x: i32,
    y: i32,
    max_chars: usize,
    font_size: i32,
    line_height: i32,
    fill: &str,
    weight: &str,
    max_lines: usize,
) -> String {
    let lines = wrap_text(text, max_chars, max_lines);
    let mut out = String::new();
    for (idx, line) in lines.iter().enumerate() {
        out.push_str(&format!(
            r#"<text x="{x}" y="{}" font-size="{font_size}" font-weight="{weight}" fill="{fill}">{}</text>"#,
            y + idx as i32 * line_height,
            xml_escape(line)
        ));
    }
    out
}

fn wrap_text(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let limit = max_chars.max(6);
    while start < chars.len() && lines.len() < max_lines {
        let end = (start + limit).min(chars.len());
        let mut line: String = chars[start..end].iter().collect();
        if end < chars.len() && lines.len() + 1 == max_lines {
            line.push('…');
        }
        lines.push(line);
        start = end;
    }
    lines
}

fn run_quality_check(
    root: &Path,
    python_path: &str,
    project: &Path,
    started: Instant,
) -> Result<PptMasterExportResult, AppError> {
    let script_path = root.join(SVG_QUALITY_CHECKER_SCRIPT);
    let python = resolve_python_program(root, python_path);
    let mut cmd = Command::new(&python);
    cmd.current_dir(root).arg(&script_path).arg(project);
    add_no_window(&mut cmd);

    let output = cmd.output().map_err(|e| {
        AppError::Custom(format!(
            "无法启动 SVG 质量检查: {} ({})",
            python.display(),
            e
        ))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(PptMasterExportResult {
        success: output.status.success(),
        output_path: None,
        exit_code: output.status.code(),
        stdout,
        stderr,
        duration_ms: started.elapsed().as_millis(),
        error: if output.status.success() {
            None
        } else {
            Some(format!(
                "svg_quality_checker.py 检查失败，退出码: {}",
                output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "未知".to_string())
            ))
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeQualityFailure {
    page_number: Option<usize>,
    file_name: String,
    violated_rule: String,
    checker_summary: String,
}

fn should_continue_after_quality_failure(
    block_on_quality_failure: bool,
    last_svg_path: &Path,
) -> bool {
    !block_on_quality_failure && last_svg_path.is_file()
}

fn parse_native_quality_failures(stdout: &str, stderr: &str) -> Vec<NativeQualityFailure> {
    let combined = if stderr.trim().is_empty() {
        stdout.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    };
    let mut failures: Vec<NativeQualityFailure> = Vec::new();
    let mut current: Option<usize> = None;
    for line in combined.lines() {
        let trimmed = line.trim();
        if let Some(file_name) = quality_failure_file_name(trimmed) {
            let page_number = file_name
                .split_once('_')
                .and_then(|(prefix, _)| prefix.parse::<usize>().ok());
            failures.push(NativeQualityFailure {
                page_number,
                file_name: file_name.to_string(),
                violated_rule: "SVG Quality Checker hard error".to_string(),
                checker_summary: trimmed.to_string(),
            });
            current = Some(failures.len() - 1);
            continue;
        }
        if trimmed.starts_with("[WARN]") || trimmed.starts_with("[OK]") {
            current = None;
            continue;
        }
        if let Some(index) = current {
            if trimmed.starts_with("[ERROR]") {
                let detail = trimmed.trim_start_matches("[ERROR]").trim().to_string();
                if !detail.is_empty() {
                    failures[index].violated_rule = quality_rule_from_detail(&detail);
                    failures[index].checker_summary.push_str(" | ");
                    failures[index].checker_summary.push_str(&detail);
                }
            } else if trimmed.starts_with("===") || trimmed.starts_with("[SUMMARY]") {
                current = None;
            }
        }
    }
    failures
}

fn quality_failure_file_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("[ERROR] ")?;
    let marker = ".svg - Failed";
    let end = rest.find(marker)? + ".svg".len();
    let file_name = &rest[..end];
    if file_name
        .split_once('_')
        .is_some_and(|(prefix, _)| prefix.chars().all(|character| character.is_ascii_digit()))
    {
        Some(file_name)
    } else {
        None
    }
}

fn quality_rule_from_detail(detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if lower.starts_with("invalid xml") {
        "XML well-formedness".to_string()
    } else if lower.contains("viewbox") || lower.contains("canvas") {
        "native canvas/viewBox 1280x720".to_string()
    } else if lower.contains("clip-path") || lower.contains("clippath") {
        "unsupported clipPath on non-image shape".to_string()
    } else if lower.contains("foreignobject") {
        "unsupported foreignObject".to_string()
    } else if lower.contains("unsupported") {
        "unsupported SVG element or attribute".to_string()
    } else if lower.contains("font") {
        "PowerPoint-safe font".to_string()
    } else if lower.contains("image") || lower.contains("href") {
        "embedded image compatibility".to_string()
    } else {
        detail
            .split([':', '—'])
            .next()
            .unwrap_or("SVG Quality Checker hard error")
            .trim()
            .to_string()
    }
}

fn join_outputs(log_lines: &[String], outputs: &[String]) -> String {
    let mut parts = Vec::new();
    if !log_lines.is_empty() {
        parts.push(log_lines.join("\n"));
    }
    for output in outputs {
        let trimmed = output.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    parts.join("\n\n")
}

fn export_project(
    root: &Path,
    python_path: &str,
    project: &Path,
    started: Instant,
) -> Result<PptMasterExportResult, AppError> {
    let script_path = root.join(SVG_TO_PPTX_SCRIPT);
    let python = resolve_python_program(root, python_path);
    let mut cmd = Command::new(&python);
    cmd.current_dir(root).arg(&script_path).arg(project);
    add_no_window(&mut cmd);

    let output = cmd
        .output()
        .map_err(|e| AppError::Custom(format!("无法启动 Python: {} ({})", python_path, e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();
    let duration_ms = started.elapsed().as_millis();

    if !output.status.success() {
        return Ok(PptMasterExportResult {
            success: false,
            output_path: None,
            exit_code,
            stdout,
            stderr,
            duration_ms,
            error: Some(format!(
                "ppt-master 导出失败，退出码: {}",
                exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "未知".to_string())
            )),
        });
    }

    match latest_exported_pptx(project) {
        Ok(path) => Ok(PptMasterExportResult {
            success: true,
            output_path: Some(path.to_string_lossy().to_string()),
            exit_code,
            stdout,
            stderr,
            duration_ms,
            error: None,
        }),
        Err(e) => Ok(PptMasterExportResult {
            success: false,
            output_path: None,
            exit_code,
            stdout,
            stderr,
            duration_ms,
            error: Some(e.to_string()),
        }),
    }
}

fn validate_native_export_result(result: &PptMasterExportResult) -> Result<PathBuf, AppError> {
    if !result.success {
        return Err(AppError::Custom(
            result
                .error
                .clone()
                .unwrap_or_else(|| "svg_to_pptx.py 原生导出失败".to_string()),
        ));
    }
    let output_path = result.output_path.as_deref().ok_or_else(|| {
        AppError::Custom("svg_to_pptx.py 返回成功但未返回 output_path".to_string())
    })?;
    let path = PathBuf::from(output_path);
    if !path.is_file() {
        return Err(AppError::NotFound(format!(
            "svg_to_pptx.py 返回的 PPTX 不存在: {}",
            path.display()
        )));
    }
    let is_pptx = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pptx"));
    if !is_pptx {
        return Err(AppError::Custom(format!(
            "svg_to_pptx.py 返回的文件不是 PPTX: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn parse_dir(label: &str, value: &str) -> Result<PathBuf, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(format!("{}不能为空", label)));
    }
    let path = PathBuf::from(trimmed);
    if !path.exists() {
        return Err(AppError::NotFound(format!(
            "{}不存在: {}",
            label,
            path.display()
        )));
    }
    if !path.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "{}不是文件夹: {}",
            label,
            path.display()
        )));
    }
    Ok(path)
}

fn create_dir_all(path: &Path) -> Result<(), AppError> {
    fs::create_dir_all(path)
        .map_err(|e| AppError::Custom(format!("创建目录失败: {} ({})", path.display(), e)))
}

fn write_file(path: &Path, content: &str) -> Result<(), AppError> {
    fs::write(path, content)
        .map_err(|e| AppError::Custom(format!("写入文件失败: {} ({})", path.display(), e)))
}

fn safe_filename(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        let ok = ch.is_ascii_alphanumeric()
            || ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || matches!(ch, '-' | '_');
        if ok {
            out.push(ch);
        } else if ch.is_whitespace()
            || matches!(
                ch,
                ':' | '：' | '/' | '\\' | '|' | '?' | '*' | '"' | '<' | '>'
            )
        {
            out.push('_');
        }
        if out.chars().count() >= 18 {
            break;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ensure_python_available(root: &Path, python_path: &str) -> Result<(), AppError> {
    python_version(root, python_path)
        .map(|_| ())
        .map_err(AppError::Custom)
}

fn python_version(root: &Path, python_path: &str) -> Result<String, String> {
    let trimmed = python_path.trim();
    if trimmed.is_empty() {
        return Err("Python 可执行文件路径不能为空".into());
    }

    let python = resolve_python_program(root, trimmed);
    let mut cmd = Command::new(&python);
    cmd.current_dir(root);
    cmd.arg("--version");
    add_no_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("无法启动 Python: {} ({})", python.display(), e))?;

    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    if output.status.success() && !text.is_empty() {
        Ok(text)
    } else {
        Err(format!(
            "Python 检测失败，退出码: {}",
            output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "未知".to_string())
        ))
    }
}

fn resolve_python_program(root: &Path, python_path: &str) -> PathBuf {
    let raw = PathBuf::from(python_path.trim());
    if raw.is_absolute() {
        return raw;
    }
    if python_path.contains('\\') || python_path.contains('/') || python_path.starts_with('.') {
        root.join(raw)
    } else {
        raw
    }
}

fn latest_exported_pptx(project: &Path) -> Result<PathBuf, AppError> {
    let exports = project.join("exports");
    if !exports.is_dir() {
        return Err(AppError::NotFound(format!(
            "导出目录不存在: {}",
            exports.display()
        )));
    }

    let mut latest: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in fs::read_dir(&exports)? {
        let entry = entry?;
        let path = entry.path();
        let is_pptx = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("pptx"))
            .unwrap_or(false);
        if !is_pptx {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        let should_replace = latest
            .as_ref()
            .map(|(_, current)| modified > *current)
            .unwrap_or(true);
        if should_replace {
            latest = Some((path, modified));
        }
    }

    latest.map(|(path, _)| path).ok_or_else(|| {
        AppError::NotFound(format!("导出目录中没有 PPTX 文件: {}", exports.display()))
    })
}

#[cfg(target_os = "windows")]
fn add_no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn add_no_window(_cmd: &mut Command) {}

#[cfg(test)]
mod ppt_generation_failure_tests {
    use super::*;

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("build test runtime")
            .block_on(future)
    }

    #[test]
    fn native_svg_repair_timeout_keeps_engine_and_page_metadata() {
        let result = PptMasterGenerateResult::failure(
            "AI 修复 native 兼容 SVG：01_cover.svg 超时：超过 120 秒，已停止生成。".to_string(),
            "agent".to_string(),
            "ppt_master_native".to_string(),
            120_000,
        );

        assert_eq!(result.generation_mode, "agent");
        assert_eq!(result.generation_engine, "ppt_master_native");
        assert_eq!(
            result.failure_stage.as_deref(),
            Some("native_svg_compat_repair")
        );
        assert_eq!(
            result.failure_type.as_deref(),
            Some("native_svg_repair_timeout")
        );
        assert_eq!(result.failed_page, Some(1));
        assert_eq!(result.timed_out_after_seconds, Some(120));
        assert_eq!(result.failed_svg_file.as_deref(), Some("01_cover.svg"));
    }

    #[test]
    fn quality_repair_timeout_is_classified_as_native_svg_repair() {
        let result = PptMasterGenerateResult::failure(
            "native_svg_repair_timeout: AI 修复 SVG：02_timeline.svg 超时：超过 300 秒，stage=read_response，已停止生成。"
                .to_string(),
            "agent".to_string(),
            "ppt_master_native".to_string(),
            300_000,
        );

        assert_eq!(result.failure_stage.as_deref(), Some("native_svg_repair"));
        assert_eq!(
            result.failure_type.as_deref(),
            Some("native_svg_repair_timeout")
        );
        assert_eq!(result.failed_page, Some(2));
        assert_eq!(result.timed_out_after_seconds, Some(300));
        assert_eq!(result.failed_svg_file.as_deref(), Some("02_timeline.svg"));
    }

    #[test]
    fn path_failure_is_classified_separately_from_native_timeout() {
        let result = PptMasterGenerateResult::failure(
            "ppt-master 根目录不存在".to_string(),
            "template".to_string(),
            "legacy_fallback".to_string(),
            1,
        );

        assert_eq!(result.failure_stage.as_deref(), Some("configuration"));
        assert_eq!(
            result.failure_type.as_deref(),
            Some("ppt_master_root_invalid")
        );
        assert_eq!(result.failed_page, None);
    }

    #[test]
    fn mocked_ppt_ai_request_returns_normal_response() {
        let progress = AiRequestProgress::default();
        let result = block_on(await_ppt_ai_network(
            async { Ok("<svg />".to_string()) },
            1,
            "mock native repair",
            Some("native_svg_repair_timeout"),
            &progress,
        ))
        .expect("mock response should succeed");
        assert_eq!(result, "<svg />");
    }

    #[test]
    fn mocked_ppt_ai_timeout_returns_native_failure_type() {
        let progress = AiRequestProgress::default();
        let error = block_on(await_ppt_ai_network(
            std::future::pending::<Result<String, AppError>>(),
            0,
            "AI 修复 native 兼容 SVG：01_cover.svg",
            Some("native_svg_repair_timeout"),
            &progress,
        ))
        .expect_err("pending mock request should time out")
        .to_string();
        assert!(error.contains("native_svg_repair_timeout"));
        assert!(error.contains("stage=connect_or_wait_response_headers"));
    }

    #[test]
    fn local_svg_parse_is_outside_network_timeout() {
        let progress = AiRequestProgress::default();
        let raw = block_on(await_ppt_ai_network(
            async { Ok("<svg><rect /></svg>".to_string()) },
            1,
            "mock native repair",
            Some("native_svg_repair_timeout"),
            &progress,
        ))
        .expect("network mock should complete");
        std::thread::sleep(std::time::Duration::from_millis(1_050));
        assert_eq!(extract_svg(&raw).as_deref(), Some("<svg><rect /></svg>"));
    }

    #[test]
    fn native_repair_attempts_are_bounded_per_file() {
        let mut attempts = HashMap::new();
        assert!(reserve_native_svg_repair_attempt(
            &mut attempts,
            "01_cover.svg"
        ));
        assert!(reserve_native_svg_repair_attempt(
            &mut attempts,
            "01_cover.svg"
        ));
        assert!(!reserve_native_svg_repair_attempt(
            &mut attempts,
            "01_cover.svg"
        ));
        assert_eq!(attempts.get("01_cover.svg"), Some(&2));
    }

    #[test]
    fn repair_limit_does_not_block_another_page() {
        let mut attempts = HashMap::new();
        for _ in 0..NATIVE_SVG_REPAIR_MAX_ATTEMPTS_PER_PAGE {
            assert!(reserve_native_svg_repair_attempt(
                &mut attempts,
                "01_cover.svg"
            ));
        }
        assert!(reserve_native_svg_repair_attempt(
            &mut attempts,
            "02_agenda.svg"
        ));
    }

    #[test]
    fn native_repair_prompt_only_contains_issue_and_svg() {
        let issue = NativeSvgIssue {
            file_name: "01_cover.svg".to_string(),
            issue_type: "unsupported_pattern".to_string(),
            unsupported_elements: vec!["pattern".to_string()],
            detail: "pattern is not supported".to_string(),
        };
        let svg = "<svg><pattern id=\"p\" /></svg>";
        let prompt = build_native_svg_repair_prompt(&issue, svg);
        assert!(prompt.contains(svg));
        assert!(prompt.contains("unsupported_pattern"));
        assert!(!prompt.contains("design_spec.md"));
        assert!(!prompt.contains("spec_lock.md"));
        assert!(prompt.chars().count() < svg.chars().count() + 1_000);
    }

    #[test]
    fn native_timeout_is_configurable_and_bounded() {
        assert_eq!(
            resolve_native_svg_repair_timeout(None),
            ResolvedNativeSvgRepairTimeout {
                seconds: 300,
                source: "default",
            }
        );
        assert_eq!(resolve_native_svg_repair_timeout(Some("30")).seconds, 60);
        assert_eq!(
            resolve_native_svg_repair_timeout(Some("300")),
            ResolvedNativeSvgRepairTimeout {
                seconds: 300,
                source: "config",
            }
        );
        assert_eq!(resolve_native_svg_repair_timeout(Some("450")).seconds, 450);
        assert_eq!(
            resolve_native_svg_repair_timeout(Some("1500")).seconds,
            1_200
        );
    }

    #[test]
    fn every_native_svg_repair_request_uses_the_unified_timeout_path() {
        for request_id in [
            "ppt_master_agent_svg_repair_02",
            "ppt_master_native_svg_compat_repair_02_timeline.svg",
            "ppt_master_final_text_guard_repair_02_timeline.svg",
        ] {
            assert!(
                is_native_svg_repair_request_id(request_id),
                "request unexpectedly fell back to the legacy 120-second path: {}",
                request_id
            );
        }
        assert!(!is_native_svg_repair_request_id("ppt_master_agent_svg_02"));
    }

    #[test]
    fn native_output_limit_is_proportional_and_capped() {
        assert_eq!(native_svg_repair_output_tokens(2_730), 2_048);
        assert_eq!(native_svg_repair_output_tokens(100_000), 8_192);
    }
}

#[cfg(test)]
mod ppt_understanding_input_tests {
    use super::*;

    #[test]
    fn structured_understanding_precedes_compatibility_inputs() {
        let input: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python",
            "prompt": "legacy compatibility prompt",
            "planningContext": "planning mirror",
            "aiUnderstandingResult": {
                "understandingSummary": "edited summary",
                "keyPriorities": "edited priorities",
                "narrativeMainline": "edited narrative",
                "suggestedPageStructure": "1. opening\n2. evidence",
                "visualExpressionAdvice": "edited visual advice",
                "openQuestions": "none"
            },
            "rawMaterial": "authoritative facts",
            "extraRequirements": "extra requirement",
            "audience": "teachers"
        }))
        .expect("deserialize structured understanding input");
        let context = build_generation_planning_context(&input, input.prompt.trim());
        let structured = context
            .find("[User-Edited Structured AI Understanding]")
            .unwrap();
        let mirror = context.find("[Planning Context Mirror]").unwrap();
        let raw = context.find("[Raw Material").unwrap();
        let extra = context.find("[Extra Requirements]").unwrap();
        let legacy = context.find("[Legacy Prompt - compatibility only").unwrap();
        assert!(structured < mirror && mirror < raw && raw < extra && extra < legacy);
        assert!(context.contains("## AI 理解摘要\nedited summary"));
        assert!(context.contains("## 建议页面结构\n1. opening\n2. evidence"));
    }

    #[test]
    fn legacy_understanding_string_remains_deserializable() {
        let input: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python",
            "prompt": "legacy prompt",
            "aiUnderstandingResult": "legacy AI result",
            "rawMaterial": "facts"
        }))
        .expect("deserialize legacy understanding input");
        assert!(matches!(
            input.ai_understanding_result,
            Some(PptUnderstandingInput::Legacy(ref value)) if value == "legacy AI result"
        ));
        let context = build_generation_planning_context(&input, input.prompt.trim());
        assert!(context.contains("[Legacy AI Understanding Result]\nlegacy AI result"));
    }

    #[test]
    fn custom_style_is_preserved_separately_from_effective_style() {
        let input: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python",
            "prompt": "主题",
            "style": "红色情怀",
            "customStyle": "红色情怀",
            "extraRequirements": "界面要红色情怀拉满"
        }))
        .expect("deserialize custom style");
        assert_eq!(input.style.as_deref(), Some("红色情怀"));
        assert_eq!(input.custom_style.as_deref(), Some("红色情怀"));
        assert_eq!(
            input.extra_requirements.as_deref(),
            Some("界面要红色情怀拉满")
        );
    }
}

#[cfg(test)]
mod native_theme_contract_tests {
    use super::*;

    fn minimal_plan(style: &str) -> SlidePlan {
        SlidePlan {
            title: "主题测试".to_string(),
            subtitle: String::new(),
            audience: "测试受众".to_string(),
            style: style.to_string(),
            theme: default_theme(),
            theme_allocation: Vec::new(),
            slides: Vec::new(),
        }
    }

    #[test]
    fn red_custom_style_does_not_fall_back_to_swiss_blue_orange_lock() {
        let root =
            std::env::temp_dir().join(format!("pome-native-theme-mapping-{}", std::process::id()));
        let input: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": root,
            "pythonPath": "python",
            "prompt": "主题",
            "style": "红色情怀",
            "customStyle": "红色情怀",
            "extraRequirements": "界面要红色情怀拉满"
        }))
        .expect("deserialize custom style input");
        let theme = NativeThemeSpec::from_inputs(
            "红色情怀",
            input.custom_style.as_deref(),
            input.extra_requirements.as_deref(),
            input.visual_expression_advice.as_deref(),
        );
        let mapping = resolve_style_mapping(&root, "红色情怀", &input, &theme);
        assert_eq!(mapping.mode, "narrative");
        assert_eq!(mapping.visual_style, "vintage-poster");

        let plan = minimal_plan("红色情怀");
        let lock = build_ppt_master_spec_lock(&plan, &mapping, &theme);
        assert!(lock.contains("#B91C1C"));
        assert!(lock.contains("#D4A017"));
        assert!(lock.contains("vintage-poster"));
        assert!(!lock.contains("- primary: #1f2937"));
        assert!(!lock.contains("- secondary_accent: #2563eb"));
    }

    #[test]
    fn tech_blue_mapping_and_lock_remain_dark_tech() {
        let root =
            std::env::temp_dir().join(format!("pome-native-theme-tech-{}", std::process::id()));
        let input: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": root,
            "pythonPath": "python",
            "prompt": "主题",
            "style": "科技蓝"
        }))
        .expect("deserialize tech input");
        let theme = NativeThemeSpec::from_inputs("科技蓝", None, None, None);
        let mapping = resolve_style_mapping(&root, "科技蓝", &input, &theme);
        assert_eq!(mapping.mode, "showcase");
        assert_eq!(mapping.visual_style, "dark-tech");
        let lock = build_ppt_master_spec_lock(&minimal_plan("科技蓝"), &mapping, &theme);
        assert!(lock.contains("#081426"));
        assert!(lock.contains("#2563EB"));
        assert!(lock.contains("#38BDF8"));
    }
}

#[cfg(test)]
mod stable_render_tests {
    use super::*;

    fn test_slide(page: usize, relation: &str, chart_type: &str, density: &str) -> Slide {
        Slide {
            page,
            page_index: page,
            page_id: format!("P{:02}", page),
            slide_type: if page == 1 { "cover" } else { "content" }.to_string(),
            layout: String::new(),
            title: format!("Page {}", page),
            subtitle: "A concise subtitle".to_string(),
            bullets: vec!["Evidence one".to_string(), "Evidence two".to_string()],
            visual_hint: String::new(),
            page_theme: format!("Theme {}", page),
            main_claim: format!("Claim {}", page),
            core_message: format!("Core message {}", page),
            content_scope: format!("Scope {}", page),
            content_blocks: (1..=4)
                .map(|index| ContentBlock {
                    label: format!("Point {}", index),
                    text: format!("Specific explanation {}", index),
                    detail: format!("Supporting detail {}", index),
                })
                .collect(),
            evidence: vec![
                "Source detail A".to_string(),
                "Source detail B".to_string(),
                "Source detail C".to_string(),
                "Source detail D".to_string(),
            ],
            relation: relation.to_string(),
            density: density.to_string(),
            visual_intent: String::new(),
            must_include: Vec::new(),
            must_avoid: Vec::new(),
            page_rhythm: density.to_string(),
            chart_ref: String::new(),
            chart_type: chart_type.to_string(),
            file_stem: format!("{:02}_page", page),
            speaker_note: "Speaker note".to_string(),
        }
    }

    fn test_plan(slides: Vec<Slide>) -> SlidePlan {
        SlidePlan {
            title: "Test deck".to_string(),
            subtitle: "Stable renderer".to_string(),
            audience: "General audience".to_string(),
            style: "简约商务".to_string(),
            theme: default_theme(),
            theme_allocation: Vec::new(),
            slides,
        }
    }

    fn realistic_chart_index() -> &'static str {
        r#"{
            "meta": { "total": 6 },
            "charts": {
                "timeline": {},
                "process_flow": {},
                "fishbone_diagram": {},
                "pyramid_chart": {},
                "matrix_2x2": {},
                "labeled_card": {}
            }
        }"#
    }

    fn temporary_ppt_master_root(chart_index: Option<&str>) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pomegranate-stable-chart-index-{}-{}",
            std::process::id(),
            nonce
        ));
        if let Some(contents) = chart_index {
            let charts_dir = root.join(PPT_MASTER_CHARTS_DIR);
            fs::create_dir_all(&charts_dir).expect("create temporary charts directory");
            fs::write(charts_dir.join("charts_index.json"), contents)
                .expect("write temporary charts index");
        }
        root
    }

    #[test]
    fn visible_text_boundary_removes_internal_advice_urls_and_citations() {
        let raw = "[User-Edited Structured AI Understanding]\n毛泽东生于1893年。第3页可用半透明图表和卡片布局。本材料源自维基百科条目。参见[人物条目](https://zh.wikipedia.org/wiki/%E6%AF%9B%E6%B3%BD%E4%B8%9C#cite_note-16)[16]。16\\]](https://zh。封面·总览：标题居中。";
        let cleaned = sanitize_visible_text(raw);

        assert!(cleaned.contains("毛泽东生于1893年"));
        assert!(cleaned.contains("人物条目"));
        for forbidden in [
            "User-Edited",
            "半透明图表",
            "https://",
            "wikipedia",
            "org/wiki",
            "cite_note",
            "%E6",
            "[16]",
            "本材料源自",
            "16\\]](",
            "封面·总览",
        ] {
            assert!(
                !cleaned.contains(forbidden),
                "leaked {forbidden}: {cleaned}"
            );
        }
    }

    #[test]
    fn stable_visible_material_excludes_design_and_open_question_fields() {
        let input: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python",
            "prompt": "legacy",
            "aiUnderstandingResult": {
                "understandingSummary": "人物生平与政治历程",
                "keyPriorities": "1893年至1976年的关键节点",
                "narrativeMainline": "从早年经历到晚年政治活动",
                "suggestedPageStructure": "第3页使用时间轴",
                "visualExpressionAdvice": "第4页可用半透明卡片",
                "openQuestions": "是否在PPT中展示争议数据？"
            }
        }))
        .unwrap();

        let material = build_stable_visible_material(&input, input.prompt.trim());
        assert!(material.contains("人物生平与政治历程"));
        assert!(material.contains("1893年至1976年的关键节点"));
        assert!(!material.contains("时间轴"));
        assert!(!material.contains("半透明卡片"));
        assert!(!material.contains("是否"));
    }

    #[test]
    fn prepare_stable_plan_blocks_metadata_from_rendered_svg() {
        let mut slides = vec![
            test_slide(1, "none", "highlight", "anchor"),
            test_slide(2, "category", "cards", "dense"),
            test_slide(3, "none", "summary", "anchor"),
        ];
        slides[1].title = "User-Edited Struct".to_string();
        slides[1].subtitle = "第2页建议使用半透明卡片".to_string();
        slides[1].core_message = "https://zh.wikipedia.org/wiki/%E6%AF%9B#cite_note-2".to_string();
        slides[1].content_blocks = vec![ContentBlock {
            label: "wikipedia".to_string(),
            text: "org/wiki/%E6%AF%9B".to_string(),
            detail: "[16]".to_string(),
        }];
        slides[1].visual_intent = "INTERNAL_VISUAL_INTENT_DO_NOT_RENDER".to_string();
        let mut plan = test_plan(slides);
        prepare_stable_plan_for_render(
            &mut plan,
            "毛泽东生于1893年；1919年参与五四运动；1949年中华人民共和国成立；1972年尼克松访华",
        );

        let profile = StableRenderProfile::from_plan(&plan);
        let rendered = render_slide_svg_with_profile(&plan, &plan.slides[1], &profile)
            .expect("sanitized stable page should render");
        let visible = format!(
            "{} {} {}",
            plan.slides[1].title,
            stable_core_message(&plan.slides[1]),
            rendered.svg
        );
        for forbidden in [
            "User-Edited",
            "半透明卡片",
            "https://zh",
            "wikipedia",
            "org/wiki",
            "cite_note",
            "%E6",
            "INTERNAL_VISUAL_INTENT_DO_NOT_RENDER",
        ] {
            assert!(!visible.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn semantic_consistency_replaces_unrelated_family_block_on_diplomacy_page() {
        let mut slides = vec![
            test_slide(1, "none", "highlight", "anchor"),
            test_slide(2, "category", "cards", "dense"),
            test_slide(3, "timeline", "timeline", "dense"),
            test_slide(4, "none", "summary", "anchor"),
        ];
        slides[1].title = "尼克松访华与外交破冰".to_string();
        slides[1].page_theme = slides[1].title.clone();
        slides[1].subtitle = "家庭婚姻与子女情况".to_string();
        slides[1].core_message = slides[1].subtitle.clone();
        slides[1].content_blocks = vec![
            ContentBlock {
                label: "家庭".to_string(),
                text: "婚姻与子女构成".to_string(),
                detail: String::new(),
            },
            ContentBlock {
                label: "著作".to_string(),
                text: "诗词与书法作品".to_string(),
                detail: String::new(),
            },
        ];
        slides[2].content_blocks = vec![
            ContentBlock {
                label: "1972".to_string(),
                text: "尼克松访华推动中美关系破冰".to_string(),
                detail: String::new(),
            },
            ContentBlock {
                label: "外交".to_string(),
                text: "中美发布上海公报".to_string(),
                detail: String::new(),
            },
        ];
        let mut plan = test_plan(slides);
        prepare_stable_plan_for_render(&mut plan, "");

        let diplomacy = &plan.slides[1];
        assert!(stable_core_message(diplomacy).contains("尼克松"));
        assert!(diplomacy
            .content_blocks
            .iter()
            .any(|block| content_block_display(block).contains("尼克松")));
        assert!(!diplomacy.subtitle.contains("家庭婚姻"));
    }

    #[test]
    fn semantic_consistency_prefers_complete_visible_evidence_over_truncated_block() {
        let mut slides = vec![
            test_slide(1, "none", "highlight", "anchor"),
            test_slide(2, "process", "process_flow", "dense"),
            test_slide(3, "category", "cards", "balanced"),
            test_slide(4, "none", "summary", "anchor"),
        ];
        slides[1].title = "执政探索与坎坷晚年".to_string();
        slides[1].page_theme = slides[1].title.clone();
        slides[1].content_blocks = vec![ContentBlock {
            label: "执政探索与坎坷晚年（1949—197".to_string(),
            text: "执政探索与坎坷晚年（1949—197".to_string(),
            detail: String::new(),
        }];
        slides[2].bullets = vec![
            "执政探索与坎坷晚年（1949—1976）：从社会主义改造到晚年外交破冰，呈现理想、现实与路线的复杂关系".to_string(),
            "1972年尼克松访华，中美关系开始走向正常化".to_string(),
        ];
        let mut plan = test_plan(slides);
        prepare_stable_plan_for_render(&mut plan, "");

        let complete = plan.slides[1]
            .content_blocks
            .iter()
            .map(stable_block_semantic_text)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            complete.contains("1976"),
            "complete evidence was not restored: {complete}"
        );
        assert!(!plan.slides[1].title.ends_with("（19"));
    }

    #[test]
    fn short_deck_layout_limits_are_hard_and_body_families_are_unique_first() {
        let mut slides = Vec::new();
        slides.push(test_slide(1, "none", "highlight", "anchor"));
        for page in 2..=5 {
            slides.push(test_slide(page, "category", "cards", "balanced"));
        }
        slides.push(test_slide(6, "none", "summary", "anchor"));
        let plan = test_plan(slides);
        let selections = stable_visual_selections(&plan, &std::collections::HashSet::new());
        let body = &selections[1..5];
        let layouts: Vec<_> = body
            .iter()
            .map(|selection| selection.signature.layout_family)
            .collect();
        let unique: std::collections::HashSet<_> = layouts.iter().copied().collect();

        assert_eq!(
            unique.len(),
            body.len(),
            "body layouts should exhaust unique compatible families first: {layouts:?}"
        );
        assert!(layouts.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(
            layouts
                .iter()
                .filter(|layout| **layout == StableLayoutKind::CategoryGrid)
                .count()
                <= 1
        );
        assert!(
            layouts
                .iter()
                .filter(|layout| **layout == StableLayoutKind::EditorialSplit)
                .count()
                <= 1
        );
        let fingerprints: std::collections::HashSet<_> = body
            .iter()
            .map(|selection| selection.structure_fingerprint)
            .collect();
        assert_eq!(fingerprints.len(), body.len());
    }

    fn selected_middle_layout(
        relation: &str,
        chart_type: &str,
        layout: &str,
        chart_patterns: &std::collections::HashSet<String>,
    ) -> StableLayoutKind {
        let mut middle = test_slide(2, relation, chart_type, "dense");
        middle.layout = layout.to_string();
        let plan = test_plan(vec![
            test_slide(1, "none", "highlight", "anchor"),
            middle,
            test_slide(3, "none", "summary", "anchor"),
        ]);
        stable_visual_selections(&plan, chart_patterns)[1]
            .signature
            .layout_family
    }

    #[test]
    fn stable_profile_reads_nested_charts_object() {
        let root = temporary_ppt_master_root(Some(realistic_chart_index()));
        let plan = test_plan(vec![test_slide(1, "none", "highlight", "anchor")]);
        let profile = StableRenderProfile::load(&root, &plan);

        assert!(profile.chart_catalog_loaded);
        for expected in [
            "timeline",
            "process_flow",
            "fishbone_diagram",
            "pyramid_chart",
            "matrix_2x2",
            "labeled_card",
        ] {
            assert!(
                profile.chart_patterns.contains(expected),
                "missing {expected}"
            );
        }
        assert!(!profile.chart_patterns.contains("meta"));
        assert!(!profile.chart_patterns.contains("charts"));
        fs::remove_dir_all(root).expect("remove temporary ppt-master root");
    }

    #[test]
    fn stable_profile_accepts_legacy_root_level_chart_keys() {
        let root = temporary_ppt_master_root(Some(
            r#"{
                "_meta": { "version": 1 },
                "timeline": {},
                "process": {},
                "cause_effect": {},
                "hierarchy": {},
                "matrix": {},
                "category_grid": {}
            }"#,
        ));
        let plan = test_plan(vec![test_slide(1, "none", "highlight", "anchor")]);
        let profile = StableRenderProfile::load(&root, &plan);

        assert!(profile.chart_catalog_loaded);
        for expected in [
            "timeline",
            "process",
            "cause_effect",
            "hierarchy",
            "matrix",
            "category_grid",
        ] {
            assert!(
                profile.chart_patterns.contains(expected),
                "missing {expected}"
            );
        }
        assert!(!profile.chart_patterns.contains("_meta"));
        fs::remove_dir_all(root).expect("remove temporary ppt-master root");
    }

    #[test]
    fn missing_or_invalid_chart_index_degrades_to_empty_catalog() {
        let plan = test_plan(vec![test_slide(1, "none", "highlight", "anchor")]);

        let missing_root = temporary_ppt_master_root(None);
        let missing_profile = StableRenderProfile::load(&missing_root, &plan);
        assert!(!missing_profile.chart_catalog_loaded);
        assert!(missing_profile.chart_patterns.is_empty());

        let invalid_root = temporary_ppt_master_root(Some("{not valid json"));
        let invalid_profile = StableRenderProfile::load(&invalid_root, &plan);
        assert!(!invalid_profile.chart_catalog_loaded);
        assert!(invalid_profile.chart_patterns.is_empty());
        fs::remove_dir_all(invalid_root).expect("remove temporary ppt-master root");
    }

    #[test]
    fn real_catalog_aliases_reach_existing_semantic_layouts() {
        let patterns = parse_stable_chart_patterns(realistic_chart_index());
        let cases = [
            ("timeline", "timeline", "", StableLayoutKind::Timeline),
            ("none", "process_flow", "", StableLayoutKind::Process),
            ("process", "process", "", StableLayoutKind::Process),
            ("cause", "cause_effect", "", StableLayoutKind::CauseEffect),
            ("none", "hierarchy", "", StableLayoutKind::Hierarchy),
            ("none", "matrix_2x2", "", StableLayoutKind::Matrix),
            (
                "category",
                "category_grid",
                "",
                StableLayoutKind::CategoryGrid,
            ),
        ];

        for (relation, chart_type, layout, expected) in cases {
            let selected = selected_middle_layout(relation, chart_type, layout, &patterns);
            assert_eq!(selected, expected, "relation={relation} chart={chart_type}");
            if expected != StableLayoutKind::CategoryGrid {
                assert_ne!(selected, StableLayoutKind::CategoryGrid);
            }
        }
    }

    #[test]
    fn internal_semantic_renderers_do_not_depend_on_external_catalog() {
        let no_external_patterns = std::collections::HashSet::new();
        for (relation, chart_type, expected) in [
            ("timeline", "timeline", StableLayoutKind::Timeline),
            ("process", "process_flow", StableLayoutKind::Process),
            ("cause", "cause_effect", StableLayoutKind::CauseEffect),
            ("none", "hierarchy", StableLayoutKind::Hierarchy),
        ] {
            assert_eq!(
                selected_middle_layout(relation, chart_type, "", &no_external_patterns),
                expected
            );
        }
    }

    #[test]
    fn text_width_distinguishes_cjk_and_latin() {
        let cjk = estimate_stable_text_width("中国历史", 20.0, "400");
        let latin = estimate_stable_text_width("History", 20.0, "400");
        assert!(cjk > latin);
        assert!(cjk >= 78.0);
    }

    #[test]
    fn stable_strong_markup_is_parsed_into_real_runs() {
        let paragraphs = parse_stable_rich_text(
            "毛泽东**（1893年12月26日—1976年9月9日）**，字**润之**",
            StableTextRenderPolicy {
                allow_strong: true,
                allow_heading_scale: true,
            },
        );
        assert_eq!(paragraphs.len(), 1);
        assert_eq!(paragraphs[0].runs.len(), 4);
        assert_eq!(paragraphs[0].runs[0].text, "毛泽东");
        assert!(!paragraphs[0].runs[0].bold);
        assert_eq!(
            paragraphs[0].runs[1].text,
            "（1893年12月26日—1976年9月9日）"
        );
        assert!(paragraphs[0].runs[1].bold);
        assert_eq!(paragraphs[0].runs[2].text, "，字");
        assert!(!paragraphs[0].runs[2].bold);
        assert_eq!(paragraphs[0].runs[3].text, "润之");
        assert!(paragraphs[0].runs[3].bold);
    }

    #[test]
    fn unclosed_strong_markup_safely_falls_back_to_plain_text() {
        let paragraphs = parse_stable_rich_text(
            "毛泽东**（1893年12月26日—1976年9月9日）",
            StableTextRenderPolicy {
                allow_strong: true,
                allow_heading_scale: true,
            },
        );
        assert_eq!(
            paragraphs[0].runs[0].text,
            "毛泽东（1893年12月26日—1976年9月9日）"
        );
        assert!(paragraphs[0].runs.iter().all(|run| !run.bold));
        assert!(paragraphs[0]
            .runs
            .iter()
            .all(|run| !run.text.contains("**")));
    }

    #[test]
    fn odd_strong_markers_preserve_the_shortest_unambiguous_pair() {
        let paragraphs = parse_stable_rich_text(
            "毛泽东**（1893年12月26日—1976年9月9日），字**润之**",
            StableTextRenderPolicy {
                allow_strong: true,
                allow_heading_scale: true,
            },
        );
        assert_eq!(
            paragraphs[0]
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "毛泽东（1893年12月26日—1976年9月9日），字润之"
        );
        assert!(paragraphs[0]
            .runs
            .iter()
            .any(|run| run.text == "润之" && run.bold));
    }

    #[test]
    fn line_start_markdown_headings_map_to_body_hierarchy() {
        let paragraphs = parse_stable_rich_text(
            "# 一级\n## 二级\n### 三级\n#5 型号",
            StableTextRenderPolicy {
                allow_strong: true,
                allow_heading_scale: true,
            },
        );
        assert_eq!(paragraphs.len(), 4);
        for (index, expected) in [
            StableTextEmphasis::Heading1,
            StableTextEmphasis::Heading2,
            StableTextEmphasis::Heading3,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(paragraphs[index].runs[0].emphasis, expected);
            assert!(paragraphs[index].runs[0].bold);
            assert!(paragraphs[index].runs[0].font_scale >= 1.10);
            assert!(!paragraphs[index].runs[0].text.contains('#'));
        }
        assert_eq!(paragraphs[3].runs[0].text, "#5 型号");
        assert_eq!(paragraphs[3].runs[0].emphasis, StableTextEmphasis::Normal);
    }

    #[test]
    fn stable_reference_cleanup_removes_wikipedia_and_escaped_citations() {
        assert_eq!(
            clean_stable_reference_artifacts(r"毛泽东，湖南湘潭人[\[1").trim(),
            "毛泽东，湖南湘潭人"
        );
        assert_eq!(
            clean_stable_reference_artifacts(r"1912年春，毛泽东退出军队，继续求学[\[36").trim(),
            "1912年春，毛泽东退出军队，继续求学"
        );
        assert_eq!(
            clean_stable_reference_artifacts("据多方估计达到4000万至8000万[注2][17]").trim(),
            "据多方估计达到4000万至8000万"
        );
        assert_eq!(
            clean_stable_reference_artifacts(r"35（[\[30（10—11 10月24日，毛泽东投入湖南新军")
                .trim(),
            "10—11 10月24日，毛泽东投入湖南新军"
        );
        assert_eq!(
            clean_stable_reference_artifacts("注1（正文内容").trim(),
            "正文内容"
        );
        assert_eq!(
            clean_stable_reference_artifacts(r"湖南湘潭人[\[12（3813，中国近代史").trim(),
            "湖南湘潭人，中国近代史"
        );
        assert_eq!(
            clean_stable_reference_artifacts("13，中国近代史").trim(),
            "中国近代史"
        );
        assert_eq!(
            clean_stable_reference_artifacts(r"1943年成为最高领导人[\[注1（，是缔造者之一").trim(),
            "1943年成为最高领导人，是缔造者之一"
        );
        assert_eq!(
            clean_stable_reference_artifacts("，中国近代马列主义理论家").trim(),
            "中国近代马列主义理论家"
        );
    }

    #[test]
    fn isolated_reference_numbers_are_removed_without_touching_normal_numbers() {
        assert!(clean_stable_reference_artifacts("2\n13\n17\n31")
            .trim()
            .is_empty());
        let normal = clean_stable_reference_artifacts(
            "数学区间 [0, 1]，型号 #5，第 2 阶段，1912年，数量35，输入/输出",
        );
        for expected in ["[0, 1]", "#5", "第 2 阶段", "1912年", "数量35", "输入/输出"] {
            assert!(normal.contains(expected), "missing {expected}: {normal}");
        }
    }

    #[test]
    fn split_numeric_content_block_boundary_is_repaired_without_inventing_emphasis() {
        let block = sanitize_visible_block(
            &ContentBlock {
                label: "据多方估计".to_string(),
                text: "非正常死亡人数达到4".to_string(),
                detail: "000万至8000万".to_string(),
            },
            0,
        )
        .expect("visible block");
        assert_eq!(block.text, "非正常死亡人数达到4000万至8000万");
        assert!(block.detail.is_empty());
        assert!(!block.text.contains("**"));
        assert!(!block.detail.contains("**"));

        let already_cleaned_boundary = sanitize_visible_block(
            &ContentBlock {
                label: "据多方估计".to_string(),
                text: "非正常死亡人数达到".to_string(),
                detail: "4000万至8000万".to_string(),
            },
            0,
        )
        .expect("visible block after repeated sanitization");
        assert_eq!(
            already_cleaned_boundary.text,
            "非正常死亡人数达到4000万至8000万"
        );
        assert!(already_cleaned_boundary.detail.is_empty());
    }

    #[test]
    fn mixed_cjk_and_english_rich_text_keeps_run_boundaries() {
        let paragraphs = parse_stable_rich_text(
            "中国 **Long March 长征** changed history",
            StableTextRenderPolicy {
                allow_strong: true,
                allow_heading_scale: true,
            },
        );
        let strong = paragraphs[0]
            .runs
            .iter()
            .find(|run| run.bold)
            .expect("strong run");
        assert_eq!(strong.text, "Long March 长征");
        assert_eq!(
            paragraphs[0]
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "中国 Long March 长征 changed history"
        );
    }

    #[test]
    fn stable_svg_uses_nested_tspans_without_raw_markdown_markers() {
        let mut draft = StablePageDraft::new();
        let fit = append_fitted_text(
            &mut draft,
            "rich-body-test",
            "普通**重点内容**\n## 块内小标题",
            StableRect {
                x: 80.0,
                y: 180.0,
                width: 620.0,
                height: 160.0,
            },
            18.0,
            14.0,
            1.25,
            "#111827",
            "400",
            "start",
            false,
            StableElementKind::Text,
            None,
        );
        assert!(!draft.body.contains("**"));
        assert!(!draft.body.contains("## 块内小标题"));
        assert!(draft
            .body
            .contains("<tspan font-weight=\"700\">重点内容</tspan>"));
        assert!(draft.body.contains("data-stable-heading-level=\"2\""));
        assert!(draft.body.contains("font-size=\""));
        assert!(fit
            .rich_lines
            .iter()
            .flat_map(|line| line.runs.iter())
            .any(|run| run.text.contains("重点内容") && run.bold));
    }

    #[test]
    fn wrapping_does_not_leave_closing_punctuation_at_line_start() {
        let lines = wrap_text_to_width(
            "中华文明并非单一起源，而是在多个区域逐步汇合。",
            20.0,
            120.0,
            "400",
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().skip(1).all(|line| line
            .chars()
            .next()
            .is_none_or(|ch| !is_line_start_forbidden(ch))));
    }

    #[test]
    fn rich_wrapping_does_not_split_a_number_between_lines() {
        let paragraphs = parse_stable_rich_text(
            "死亡人数达到4000万至8000万",
            StableTextRenderPolicy {
                allow_strong: true,
                allow_heading_scale: true,
            },
        );
        let lines = wrap_stable_rich_text(&paragraphs, 18.0, 150.0, "400")
            .iter()
            .map(StableTextLine::plain_text)
            .collect::<Vec<_>>();
        for pair in lines.windows(2) {
            assert!(
                !(pair[0].chars().last().is_some_and(|ch| ch.is_ascii_digit())
                    && pair[1].chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            );
        }
        assert_eq!(lines.concat(), "死亡人数达到4000万至8000万");
    }

    #[test]
    fn fit_text_box_never_exceeds_requested_height() {
        let fit = fit_text_box(
            "秦汉制度奠定了长期国家治理结构，并在后续历史中持续演进。",
            220.0,
            70.0,
            18.0,
            14.0,
            1.3,
            "600",
        );
        assert!(fit.used_height <= 70.5);
        assert!(fit.font_size >= 14.0);
    }

    #[test]
    fn semantic_motif_selection_matches_information_relationship() {
        let mut timeline = test_slide(2, "timeline", "timeline", "dense");
        timeline.content_blocks[0].label = "1919".to_string();
        assert_eq!(
            stable_motif_candidates(&timeline, StableLayoutKind::Timeline)[0],
            StableMotif::BigNumber
        );
        assert_eq!(
            stable_motif_candidates(
                &test_slide(2, "compare", "compare", "dense"),
                StableLayoutKind::Comparison,
            )[0],
            StableMotif::ComparisonColumn
        );
        assert_eq!(
            stable_motif_candidates(
                &test_slide(2, "process", "process", "dense"),
                StableLayoutKind::Process,
            )[0],
            StableMotif::StepBlock
        );
        assert_eq!(
            stable_motif_candidates(
                &test_slide(2, "cause", "cause", "dense"),
                StableLayoutKind::CauseEffect,
            )[0],
            StableMotif::HubSpoke
        );
    }

    #[test]
    fn visual_selection_avoids_adjacent_signature_repetition() {
        let slides = (1..=10)
            .map(|page| test_slide(page, "category", "cards", "dense"))
            .collect();
        let plan = test_plan(slides);
        let selections = stable_visual_selections(&plan, &std::collections::HashSet::new());
        assert_eq!(selections.len(), 10);
        assert!(selections
            .windows(2)
            .all(|pair| pair[0].signature != pair[1].signature));
        for index in 1..selections.len() {
            if selections[index - 1].signature.motif_family
                == selections[index].signature.motif_family
            {
                let layout = selections[index].signature.layout_family;
                let alternatives = stable_motif_candidates(&plan.slides[index], layout)
                    .into_iter()
                    .filter(|motif| {
                        stable_motif_gate_reason(&plan, index, layout, *motif).is_none()
                    })
                    .collect::<Vec<_>>();
                assert!(
                    alternatives
                        .iter()
                        .all(|motif| *motif == selections[index].signature.motif_family),
                    "anti-repetition skipped an eligible semantic alternative"
                );
            }
        }
        assert!(selections
            .iter()
            .all(|selection| !selection.duplicate_signature));
    }

    #[test]
    fn motif_catalog_exposes_more_than_eight_distinct_visual_families() {
        let slide = test_slide(2, "category", "cards", "dense");
        let layouts = [
            StableLayoutKind::Anchor,
            StableLayoutKind::EditorialSplit,
            StableLayoutKind::Timeline,
            StableLayoutKind::CategoryGrid,
            StableLayoutKind::Comparison,
            StableLayoutKind::CauseEffect,
            StableLayoutKind::Process,
            StableLayoutKind::Hierarchy,
            StableLayoutKind::Matrix,
            StableLayoutKind::Quote,
            StableLayoutKind::EvidenceLed,
            StableLayoutKind::Summary,
        ];
        let motifs = layouts
            .into_iter()
            .flat_map(|layout| stable_motif_candidates(&slide, layout))
            .collect::<std::collections::HashSet<_>>();
        assert!(motifs.len() >= 12, "motifs={motifs:?}");
    }

    #[test]
    fn motif_components_stay_in_bounds_and_use_native_safe_svg() {
        let plan = test_plan(vec![test_slide(1, "category", "cards", "dense")]);
        let profile = StableRenderProfile::from_plan(&plan);
        let base_block = ContentBlock {
            label: "Concept".to_string(),
            text: "A concrete explanation".to_string(),
            detail: "A supporting detail".to_string(),
        };
        let motifs = [
            StableMotif::PlainEditorial,
            StableMotif::TopBandCard,
            StableMotif::NumberedBadge,
            StableMotif::BigNumber,
            StableMotif::QuoteStatement,
            StableMotif::SplitPanel,
            StableMotif::TimelineNode,
            StableMotif::StepBlock,
            StableMotif::HubSpoke,
            StableMotif::BracketGroup,
            StableMotif::EvidenceStrip,
            StableMotif::ImagePlaceholderEditorial,
            StableMotif::MatrixCell,
            StableMotif::ComparisonColumn,
            StableMotif::SectionBanner,
        ];
        for motif in motifs
            .into_iter()
            .filter(|motif| stable_motif_status(*motif) == StableMotifStatus::ProductionReady)
        {
            let mut draft = StablePageDraft::new();
            let mut block = base_block.clone();
            if motif == StableMotif::BigNumber {
                block.label = "2024".to_string();
            }
            render_stable_motif_block(
                &mut draft,
                &block,
                Some("Material evidence"),
                StableRect {
                    x: 80.0,
                    y: 210.0,
                    width: 420.0,
                    height: 260.0,
                },
                1,
                &profile.tokens,
                motif,
                StableDetailLevel::Full,
                None,
                "motif-test",
            );
            assert!(
                draft.hard_failures.is_empty(),
                "{motif:?}: {:?}",
                draft.hard_failures
            );
            assert!(
                validate_slide_layout(&draft.elements).is_empty(),
                "{motif:?}: {:?}",
                validate_slide_layout(&draft.elements)
            );
            assert!(
                validate_semantic_decorations(&draft).is_empty(),
                "{motif:?}: {:?}",
                validate_semantic_decorations(&draft)
            );
            for banned in [
                "<foreignObject",
                "<use",
                "<symbol",
                "<filter",
                "<mask",
                "<clipPath",
                "rgba(",
                " class=",
            ] {
                assert!(!draft.body.contains(banned), "{motif:?} contains {banned}");
            }
        }
    }

    #[test]
    fn stable_pages_render_with_diverse_motifs_without_model_calls() {
        let slides = vec![
            test_slide(1, "none", "highlight", "anchor"),
            test_slide(2, "timeline", "timeline", "dense"),
            test_slide(3, "category", "cards", "dense"),
            test_slide(4, "compare", "compare", "dense"),
            test_slide(5, "cause", "cause", "breathing"),
            test_slide(6, "none", "summary", "anchor"),
        ];
        let plan = test_plan(slides);
        let profile = StableRenderProfile::from_plan(&plan);
        let mut motifs = std::collections::HashSet::new();
        for slide in &plan.slides {
            let rendered = render_slide_svg_with_profile(&plan, slide, &profile)
                .unwrap_or_else(|error| panic!("P{:02}: {error}", slide.page));
            motifs.insert(rendered.motif);
            assert!(!rendered.svg.contains("<foreignObject"));
            assert!(!rendered.svg.contains("<use"));
        }
        assert!(motifs.len() >= 3, "rendered motifs={motifs:?}");
    }

    #[test]
    fn motif_gate_rejects_missing_semantic_requirements() {
        let mut slide = test_slide(2, "cause", "cause", "dense");
        slide.content_blocks.truncate(2);
        slide.evidence.truncate(2);
        let plan = test_plan(vec![test_slide(1, "none", "highlight", "anchor"), slide]);
        let hub_reason = stable_motif_gate_reason(
            &plan,
            1,
            StableLayoutKind::CauseEffect,
            StableMotif::HubSpoke,
        )
        .unwrap();
        assert!(hub_reason.contains("at least 3 blocks"));

        let number_reason = stable_motif_gate_reason(
            &plan,
            1,
            StableLayoutKind::EvidenceLed,
            StableMotif::BigNumber,
        )
        .unwrap();
        assert!(number_reason.contains("numeric anchor"));

        let image_reason = stable_motif_gate_reason(
            &plan,
            1,
            StableLayoutKind::EditorialSplit,
            StableMotif::ImagePlaceholderEditorial,
        )
        .unwrap();
        assert!(image_reason.contains("no resolved image resource"));

        let banner_reason = stable_motif_gate_reason(
            &plan,
            1,
            StableLayoutKind::CategoryGrid,
            StableMotif::SectionBanner,
        )
        .unwrap();
        assert!(banner_reason.contains("density") || banner_reason.contains("limited"));
    }

    #[test]
    fn anti_repetition_never_promotes_ineligible_motif() {
        let mut slides = vec![test_slide(1, "none", "highlight", "anchor")];
        for page in 2..=5 {
            let mut slide = test_slide(page, "cause", "cause", "dense");
            slide.content_blocks.truncate(2);
            slide.evidence.truncate(2);
            slides.push(slide);
        }
        let plan = test_plan(slides);
        let selections = stable_visual_selections(&plan, &std::collections::HashSet::new());
        assert!(selections
            .iter()
            .all(|selection| selection.signature.motif_family != StableMotif::HubSpoke));
        assert!(selections.iter().all(|selection| {
            stable_motif_status(selection.signature.motif_family)
                == StableMotifStatus::ProductionReady
        }));
    }

    #[test]
    fn empty_container_and_low_occupancy_fail_visual_qa() {
        let mut slide = test_slide(2, "category", "cards", "dense");
        slide.content_blocks.truncate(1);
        slide.evidence.clear();
        let mut draft = StablePageDraft::new();
        draft.rendered_blocks.push(StableRenderedBlock {
            id: "empty-block".to_string(),
            rect: StableRect {
                x: 56.0,
                y: 190.0,
                width: 760.0,
                height: 380.0,
            },
            label_complete: true,
            text_complete: true,
        });
        let completeness = validate_motif_completeness(
            &slide,
            StableLayoutKind::EditorialSplit,
            StableMotif::PlainEditorial,
            StableDetailLevel::Full,
            &draft,
        );
        assert!(completeness.iter().any(|issue| issue
            .contains("required page title was not rendered")
            || issue.contains("label is incomplete")
            || issue.contains("body is incomplete")));
        assert!(validate_visual_fullness(&slide, &draft)
            .iter()
            .any(|issue| issue.contains("occupancy too low")));
    }

    #[test]
    fn decoration_crossing_text_and_orphan_connector_fail_qa() {
        let mut draft = StablePageDraft::new();
        draft.push_rect(
            "left-object",
            StableRect {
                x: 80.0,
                y: 220.0,
                width: 120.0,
                height: 80.0,
            },
            StableElementKind::Card,
        );
        draft.push_rect(
            "right-object",
            StableRect {
                x: 500.0,
                y: 220.0,
                width: 120.0,
                height: 80.0,
            },
            StableElementKind::Card,
        );
        draft.elements.push(StableLayoutElement {
            id: "crossed-text".to_string(),
            rect: StableRect {
                x: 260.0,
                y: 244.0,
                width: 180.0,
                height: 28.0,
            },
            kind: StableElementKind::Text,
            container: None,
        });
        draft.push_decoration(
            "bad-connector",
            StableDecorationPurpose::Connector,
            stable_line_rect(200.0, 258.0, 500.0, 258.0, 3.0),
            &["left-object", "right-object"],
        );
        draft.push_decoration(
            "orphan-connector",
            StableDecorationPurpose::Connector,
            stable_line_rect(80.0, 400.0, 160.0, 400.0, 2.0),
            &[],
        );
        let issues = validate_semantic_decorations(&draft);
        assert!(issues
            .iter()
            .any(|issue| issue.contains("intersects text crossed-text")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("has no semantic attachment")));
    }

    #[test]
    fn footer_text_is_fitted_inside_distinct_safe_columns() {
        let slide = test_slide(1, "category", "cards", "dense");
        let mut plan = test_plan(vec![slide.clone()]);
        plan.title = "A very long presentation title that must remain inside the right footer column without colliding with the source note".repeat(3);
        plan.slides[0].evidence = vec!["Source: National Archives".to_string()];
        let profile = StableRenderProfile::from_plan(&plan);
        let mut draft = StablePageDraft::new();
        render_stable_footer(&plan, &plan.slides[0], &profile.tokens, &mut draft, None).unwrap();
        let layout_issues = validate_slide_layout(&draft.elements);
        let decoration_issues = validate_semantic_decorations(&draft);
        assert!(layout_issues.is_empty(), "{layout_issues:?}");
        assert!(decoration_issues.is_empty(), "{decoration_issues:?}");
        let title = draft
            .elements
            .iter()
            .find(|item| item.id == "footer-title")
            .unwrap();
        let evidence = draft
            .elements
            .iter()
            .find(|item| item.id == "footer-evidence")
            .unwrap();
        assert!(title.rect.right() <= STABLE_SAFE_RIGHT + 0.5);
        assert!(evidence.rect.right() < title.rect.x);
    }

    #[test]
    fn ordinary_evidence_is_not_promoted_to_visible_footer_source() {
        let slide = test_slide(1, "category", "cards", "dense");
        let plan = test_plan(vec![slide]);
        let profile = StableRenderProfile::from_plan(&plan);
        let mut draft = StablePageDraft::new();
        render_stable_footer(&plan, &plan.slides[0], &profile.tokens, &mut draft, None).unwrap();
        assert!(draft
            .elements
            .iter()
            .all(|item| item.id != "footer-evidence"));
    }

    #[test]
    fn optional_detail_can_be_omitted_without_failing_required_content() {
        let mut slide = test_slide(1, "category", "cards", "dense");
        slide.content_blocks.truncate(1);
        slide.evidence.truncate(1);
        let plan = test_plan(vec![slide.clone()]);
        let profile = StableRenderProfile::from_plan(&plan);
        let draft = render_stable_category_grid(
            &slide,
            &profile,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
        )
        .unwrap();
        assert!(draft.hard_failures.is_empty(), "{:?}", draft.hard_failures);
        assert!(validate_slide_layout(&draft.elements).is_empty());
        assert!(validate_motif_completeness(
            &slide,
            StableLayoutKind::CategoryGrid,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
            &draft,
        )
        .is_empty());
        assert!(draft.degradations.iter().any(|item| {
            item.field == "detail"
                && item.action == "omitted_to_speaker_notes"
                && item.priority == StableContentPriority::P1Optional
        }));

        let mut degradations = std::collections::HashMap::new();
        degradations.insert(slide.page, draft.degradations.clone());
        let notes = build_notes_with_degradations(&plan, &degradations);
        assert!(notes.contains("补充说明"));
        assert!(notes.contains(&slide.content_blocks[0].detail));
        assert!(notes.contains(&slide.evidence[0]));
    }

    #[test]
    fn reduced_detail_level_keeps_one_evidence_and_omits_the_rest_safely() {
        let mut slide = test_slide(1, "category", "cards", "dense");
        slide.content_blocks.truncate(3);
        slide.evidence.truncate(3);
        let plan = test_plan(vec![slide.clone()]);
        let profile = StableRenderProfile::from_plan(&plan);
        let draft = render_stable_category_grid(
            &slide,
            &profile,
            StableMotif::TopBandCard,
            StableDetailLevel::Reduced,
        )
        .unwrap();
        let visible_evidence = draft
            .elements
            .iter()
            .filter(|element| element.id.ends_with("-evidence"))
            .count();
        assert_eq!(visible_evidence, 1);
        assert!(draft.hard_failures.is_empty(), "{:?}", draft.hard_failures);
        assert!(validate_slide_layout(&draft.elements).is_empty());
        assert!(
            draft
                .degradations
                .iter()
                .filter(|item| item.field == "evidence")
                .count()
                >= 2
        );
    }

    #[test]
    fn missing_label_or_unreadable_required_text_remains_fatal() {
        let plan = test_plan(vec![test_slide(1, "category", "cards", "dense")]);
        let profile = StableRenderProfile::from_plan(&plan);
        let mut missing_label = ContentBlock {
            label: String::new(),
            text: "Required body".to_string(),
            detail: "Optional detail".to_string(),
        };
        let mut draft = StablePageDraft::new();
        render_stable_motif_block(
            &mut draft,
            &missing_label,
            None,
            StableRect {
                x: 80.0,
                y: 210.0,
                width: 360.0,
                height: 180.0,
            },
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
            None,
            "required-test",
        );
        assert!(draft
            .hard_failures
            .iter()
            .any(|issue| issue.contains("required label is empty")));

        missing_label.label = "Required label".to_string();
        missing_label.text =
            "This required body is intentionally far too long to fit inside the tiny text region. "
                .repeat(20);
        let mut overflow = StablePageDraft::new();
        render_stable_motif_block(
            &mut overflow,
            &missing_label,
            None,
            StableRect {
                x: 80.0,
                y: 210.0,
                width: 180.0,
                height: 110.0,
            },
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
            None,
            "required-overflow",
        );
        assert!(overflow
            .hard_failures
            .iter()
            .any(|issue| issue.contains("required text overflow")));
    }

    #[test]
    fn local_text_repair_targets_only_the_failed_text_box() {
        let plan = test_plan(vec![test_slide(1, "category", "cards", "dense")]);
        let profile = StableRenderProfile::from_plan(&plan);
        let target = ContentBlock {
            label: "Target".to_string(),
            text: "中华文明多元起源制度演进思想文化社会结构历史影响长期发展".repeat(3),
            detail: String::new(),
        };
        let untouched = ContentBlock {
            label: "Untouched".to_string(),
            text: "This block must retain its original geometry and typography.".to_string(),
            detail: String::new(),
        };
        let target_rect = StableRect {
            x: 80.0,
            y: 210.0,
            width: 320.0,
            height: 150.0,
        };
        let untouched_rect = StableRect {
            x: 460.0,
            y: 210.0,
            width: 320.0,
            height: 150.0,
        };
        let mut baseline = StablePageDraft::new();
        render_stable_motif_block(
            &mut baseline,
            &target,
            None,
            target_rect,
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
            None,
            "local",
        );
        render_stable_motif_block(
            &mut baseline,
            &untouched,
            None,
            untouched_rect,
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
            None,
            "untouched",
        );
        let overflow_message = baseline
            .hard_failures
            .iter()
            .find(|message| message.contains("local-1 required text overflow"))
            .expect("target text should overflow before local repair")
            .clone();
        let failure = stable_failure_from_problem(2, &overflow_message, &baseline);
        assert_eq!(failure.block_id.as_deref(), Some("local-1"));
        assert_eq!(failure.text_role.as_deref(), Some("text"));
        assert_eq!(failure.failure_type, StableFailureType::TextOverflow);
        assert!(failure.bounds.is_some());
        assert!(failure.required_height.is_some());

        let repair = repair_text_box_locally(&failure);
        let mut repaired = StablePageDraft::new();
        render_stable_motif_block(
            &mut repaired,
            &target,
            None,
            target_rect,
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
            Some(&repair),
            "local",
        );
        render_stable_motif_block(
            &mut repaired,
            &untouched,
            None,
            untouched_rect,
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Essential,
            Some(&repair),
            "untouched",
        );
        let target_fit = repaired
            .text_boxes
            .iter()
            .find(|record| record.id == "local-1-text")
            .expect("repaired target text box");
        assert!(!target_fit.fit.overflowed, "{:?}", target_fit.fit);
        let baseline_untouched = baseline
            .text_boxes
            .iter()
            .find(|record| record.id == "untouched-1-text")
            .unwrap();
        let repaired_untouched = repaired
            .text_boxes
            .iter()
            .find(|record| record.id == "untouched-1-text")
            .unwrap();
        assert_eq!(
            baseline_untouched.requested_rect,
            repaired_untouched.requested_rect
        );
        assert_eq!(
            baseline_untouched.fit.font_size,
            repaired_untouched.fit.font_size
        );
        assert_eq!(baseline_untouched.fit.lines, repaired_untouched.fit.lines);
    }

    #[test]
    fn content_block_repair_expands_only_the_target_block() {
        let plan = test_plan(vec![test_slide(1, "category", "cards", "dense")]);
        let profile = StableRenderProfile::from_plan(&plan);
        let block = ContentBlock {
            label: "Block".to_string(),
            text: "Concrete required text".to_string(),
            detail: "Optional detail moves to notes during block repair".to_string(),
        };
        let failure = StableLayoutFailure {
            page_index: 2,
            block_id: Some("target-1".to_string()),
            text_role: Some("text".to_string()),
            failure_type: StableFailureType::ContainerOverflow,
            bounds: None,
            required_width: None,
            required_height: None,
            attempted_strategy: Vec::new(),
            message: "target-1-text overflows container".to_string(),
        };
        let repair = repair_content_block_locally(&failure);
        let mut draft = StablePageDraft::new();
        render_stable_motif_block(
            &mut draft,
            &block,
            Some("Target evidence"),
            StableRect {
                x: 80.0,
                y: 210.0,
                width: 360.0,
                height: 180.0,
            },
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Full,
            Some(&repair),
            "target",
        );
        render_stable_motif_block(
            &mut draft,
            &block,
            Some("Other evidence"),
            StableRect {
                x: 80.0,
                y: 450.0,
                width: 360.0,
                height: 180.0,
            },
            1,
            &profile.tokens,
            StableMotif::TopBandCard,
            StableDetailLevel::Full,
            Some(&repair),
            "other",
        );
        let target = draft
            .rendered_blocks
            .iter()
            .find(|block| block.id == "target-1")
            .unwrap();
        let other = draft
            .rendered_blocks
            .iter()
            .find(|block| block.id == "other-1")
            .unwrap();
        assert_eq!(target.rect.height, 210.0);
        assert_eq!(other.rect.height, 180.0);
        assert_eq!(other.rect.y, 450.0);
        assert!(!target.rect.overlaps(other.rect, 1.0));
        assert!(draft
            .degradations
            .iter()
            .any(|item| { item.block_id == "target-1" && item.field == "detail" }));
        assert!(!draft
            .degradations
            .iter()
            .any(|item| { item.block_id == "other-1" && item.field == "detail" }));
    }

    #[test]
    fn local_text_repair_succeeds_before_switching_motif_and_logs_target() {
        let mut slides = vec![
            test_slide(1, "none", "highlight", "anchor"),
            test_slide(2, "category", "cards", "dense"),
            test_slide(3, "none", "summary", "anchor"),
        ];
        slides[1].title =
            "中华文明多元起源制度演进思想文化社会结构历史影响长期发展脉络观察".to_string();
        let plan = test_plan(slides);
        let profile = StableRenderProfile::from_plan(&plan);
        let primary = stable_visual_selections(&plan, &profile.chart_patterns)[1]
            .signature
            .motif_family;
        let rendered = render_slide_svg_with_profile(&plan, &plan.slides[1], &profile)
            .expect("local title repair should make P02 renderable");
        assert_eq!(rendered.motif, primary.as_str());
        assert_eq!(rendered.reflow_attempts, 1);
        assert!(rendered.local_repair_logs.iter().any(|line| {
            line.contains("page=P02")
                && line.contains("role=title")
                && line.contains("level=TextBox")
                && line.contains("strategy=reduce_font_and_rewrap")
                && line.contains("result=passed")
        }));
    }

    #[test]
    fn fallback_motif_is_considered_only_after_local_levels() {
        let slides = vec![
            test_slide(1, "none", "highlight", "anchor"),
            test_slide(2, "category", "cards", "dense"),
            test_slide(3, "none", "summary", "anchor"),
        ];
        let plan = test_plan(slides);
        let profile = StableRenderProfile::from_plan(&plan);
        let selection = stable_visual_selections(&plan, &profile.chart_patterns)[1].clone();
        let failure = StableLayoutFailure {
            page_index: 2,
            block_id: Some("category-card-2".to_string()),
            text_role: Some("text".to_string()),
            failure_type: StableFailureType::TextOverflow,
            bounds: None,
            required_width: None,
            required_height: None,
            attempted_strategy: Vec::new(),
            message: "category-card-2 required text overflow".to_string(),
        };
        let text_box = repair_failed_page(
            &plan,
            &plan.slides[1],
            1,
            selection.signature.layout_family,
            selection.signature.motif_family,
            StableRepairLevel::TextBox,
            &failure,
        );
        let block = repair_failed_page(
            &plan,
            &plan.slides[1],
            1,
            selection.signature.layout_family,
            selection.signature.motif_family,
            StableRepairLevel::ContentBlock,
            &failure,
        );
        let reflow = repair_failed_page(
            &plan,
            &plan.slides[1],
            1,
            selection.signature.layout_family,
            selection.signature.motif_family,
            StableRepairLevel::Motif,
            &failure,
        );
        let alternate = repair_failed_page(
            &plan,
            &plan.slides[1],
            1,
            selection.signature.layout_family,
            selection.signature.motif_family,
            StableRepairLevel::AlternateMotif,
            &failure,
        );
        assert_eq!(text_box.motif, selection.signature.motif_family);
        assert_eq!(block.motif, selection.signature.motif_family);
        assert_eq!(reflow.motif, selection.signature.motif_family);
        assert_ne!(alternate.motif, selection.signature.motif_family);
    }

    #[test]
    fn final_required_content_failure_does_not_change_a_passed_page() {
        let mut slides = vec![
            test_slide(1, "none", "highlight", "anchor"),
            test_slide(2, "category", "cards", "dense"),
            test_slide(3, "none", "summary", "anchor"),
        ];
        slides[1].title = "无法省略的必需页面标题".repeat(80);
        let plan = test_plan(slides);
        let profile = StableRenderProfile::from_plan(&plan);
        let before = render_slide_svg_with_profile(&plan, &plan.slides[0], &profile)
            .expect("P01 should pass before P02 repair")
            .svg;
        let error = render_slide_svg_with_profile(&plan, &plan.slides[1], &profile)
            .expect_err("P02 required title must remain fatal after all local repairs");
        assert!(error.to_string().contains("已完成 5 级局部修复"));
        let after = render_slide_svg_with_profile(&plan, &plan.slides[0], &profile)
            .expect("P01 should remain independently renderable")
            .svg;
        assert_eq!(before, after);
    }

    #[test]
    fn rerender_stable_fixture_when_requested() {
        let Ok(project_value) = std::env::var("POME_STABLE_RENDER_PROJECT") else {
            return;
        };
        let project = PathBuf::from(project_value);
        let plan_text = fs::read_to_string(project.join("slide_plan.json"))
            .expect("read stable slide_plan.json");
        let mut plan: SlidePlan =
            serde_json::from_str(&plan_text).expect("parse stable slide plan");
        let visible_material = std::env::var("POME_STABLE_RENDER_MATERIAL")
            .ok()
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        prepare_stable_plan_for_render(&mut plan, &visible_material);
        let profile = StableRenderProfile::from_plan(&plan);
        let svg_output = project.join("svg_output");
        fs::create_dir_all(&svg_output).expect("create svg_output");
        let mut degradations = std::collections::HashMap::new();
        for slide in &plan.slides {
            let rendered =
                render_slide_svg_with_profile(&plan, slide, &profile).expect("render stable slide");
            println!(
                "P{:02} layout={} motif={} fingerprint={}",
                slide.page, rendered.layout, rendered.motif, rendered.structure_fingerprint
            );
            if !rendered.degradations.is_empty() {
                degradations.insert(slide.page, rendered.degradations.clone());
            }
            fs::write(
                svg_output.join(svg_filename_for_slide(slide)),
                rendered.svg.as_bytes(),
            )
            .expect("write stable SVG");
        }
        fs::write(
            project.join("slide_plan.json"),
            serde_json::to_string_pretty(&plan).expect("serialize sanitized stable slide plan"),
        )
        .expect("write sanitized stable slide plan");
        fs::write(
            project.join("design_spec.md"),
            build_stable_design_spec(&plan),
        )
        .expect("write stable design spec");
        fs::create_dir_all(project.join("notes")).expect("create notes directory");
        fs::write(
            project.join("notes").join("total.md"),
            build_notes_with_degradations(&plan, &degradations),
        )
        .expect("write stable notes");
    }
}

#[cfg(test)]
mod native_strict_pipeline_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pomegranate_native_{label}_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    fn slide(page: usize, file_stem: &str) -> Slide {
        Slide {
            page,
            page_index: page,
            page_id: format!("P{page:02}"),
            slide_type: if page == 1 { "cover" } else { "content" }.to_string(),
            layout: "native".to_string(),
            title: format!("第 {page} 页"),
            subtitle: String::new(),
            bullets: Vec::new(),
            visual_hint: String::new(),
            page_theme: format!("主题 {page}"),
            main_claim: format!("结论 {page}"),
            core_message: String::new(),
            content_scope: String::new(),
            content_blocks: Vec::new(),
            evidence: Vec::new(),
            relation: String::new(),
            density: "breathing".to_string(),
            visual_intent: String::new(),
            must_include: Vec::new(),
            must_avoid: Vec::new(),
            page_rhythm: "breathing".to_string(),
            chart_ref: "none".to_string(),
            chart_type: "none".to_string(),
            file_stem: file_stem.to_string(),
            speaker_note: String::new(),
        }
    }

    fn plan(slides: Vec<Slide>) -> SlidePlan {
        SlidePlan {
            title: "原生严格模式测试".to_string(),
            subtitle: String::new(),
            audience: String::new(),
            style: "technical".to_string(),
            theme: default_theme(),
            theme_allocation: Vec::new(),
            slides,
        }
    }

    fn valid_native_svg(label: &str) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 720" width="1280" height="720"><text x="80" y="100">{label}</text></svg>"#
        )
    }

    #[test]
    fn powerpoint_repair_marker_is_consumed_exactly_once() {
        let marked =
            r#"<svg data-pome-powerpoint-repair-ready="true" viewBox="0 0 1280 720"></svg>"#;
        let cleaned = consume_native_powerpoint_repair_marker(marked).unwrap();
        assert!(!cleaned.contains("data-pome-powerpoint-repair-ready"));
        assert!(consume_native_powerpoint_repair_marker(&cleaned).is_none());
        assert!(consume_native_powerpoint_repair_marker(&valid_native_svg("unchanged")).is_none());
    }

    #[test]
    fn route_resolver_selects_native_and_stable_without_ambiguous_defaults() {
        assert_eq!(
            resolve_generation_route(Some("ppt_master_native"), Some("agent")).unwrap(),
            PptGenerationRoute::PptMasterNative
        );
        assert_eq!(
            resolve_generation_route(Some("legacy_fallback"), Some("template")).unwrap(),
            PptGenerationRoute::LegacyFallback
        );
        assert_eq!(
            resolve_generation_route(None, Some("agent")).unwrap(),
            PptGenerationRoute::PptMasterNative
        );
        assert!(resolve_generation_route(Some("ppt_master_native"), Some("template")).is_err());
    }

    #[test]
    fn missing_block_on_quality_failure_keeps_old_requests_strict() {
        let input: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python",
            "generationMode": "agent",
            "generationEngine": "ppt_master_native"
        }))
        .unwrap();
        assert!(input.block_on_quality_failure());

        let non_blocking: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python",
            "generationMode": "agent",
            "generationEngine": "ppt_master_native",
            "blockOnQualityFailure": false
        }))
        .unwrap();
        assert!(!non_blocking.block_on_quality_failure());
    }

    #[test]
    fn native_plan_parser_normalizes_prose_theme_but_keeps_page_contract() {
        let expected = plan(vec![slide(1, "cover"), slide(2, "architecture")]);
        let mut value = serde_json::to_value(&expected).unwrap();
        value["theme"] = serde_json::Value::String("深色工业科技主题".to_string());
        value["slides"][0]["page"] = serde_json::Value::String("封面".to_string());
        value["slides"][0]["pageIndex"] = serde_json::Value::String("第一页".to_string());
        value["slides"][0]["subtitle"] = serde_json::Value::Null;
        value["slides"][0]["chartRef"] = serde_json::Value::Null;
        value["slides"][0]["contentBlocks"] = serde_json::Value::Null;
        value["slides"][1]["mustAvoid"] =
            serde_json::Value::String("具体技术指标、流程图、长篇文字".to_string());
        let parsed = parse_native_slide_plan_json(&value.to_string()).unwrap();
        assert_eq!(parsed.slides.len(), 2);
        assert_eq!(parsed.slides[0].page, 1);
        assert_eq!(parsed.slides[0].page_index, 1);
        assert_eq!(parsed.slides[1].file_stem, "architecture");
        assert_eq!(parsed.slides[1].must_avoid.len(), 1);
        assert_eq!(parsed.theme.name, default_theme().name);
    }

    #[test]
    fn malformed_native_plan_json_is_rejected_before_bounded_retry() {
        let error =
            parse_native_slide_plan_json(r#"{"title":"Mao Zedong" "slides":[],"theme":{}}"#)
                .unwrap_err();

        assert!(error.to_string().contains("line 1"));
        assert_eq!(NATIVE_PLAN_JSON_MAX_ATTEMPTS, 2);
    }

    #[test]
    fn native_plan_json_retry_reuses_original_request_with_json_only_correction() {
        let original = "ORIGINAL STRICT PLAN REQUEST";
        let retry = build_native_plan_json_retry_prompt(
            original,
            "expected ',' or ']' at line 219 column 68",
        );

        assert!(retry.starts_with(original));
        assert!(retry.contains("one complete JSON object only"));
        assert!(retry.contains("comma, quote, escape sequence, and closing bracket"));
        assert!(retry.contains("line 219 column 68"));
        assert!(!retry.contains("RAW_MODEL_RESPONSE_SENTINEL"));
        assert!(retry.chars().count() < original.chars().count() + 1_000);
    }

    #[test]
    fn native_plan_json_retry_request_ids_keep_json_response_policy() {
        let first = native_plan_json_request_id("ppt_master_agent_design_plan", 1);
        let retry = native_plan_json_request_id("ppt_master_agent_design_plan", 2);
        let dedup_retry = native_plan_json_request_id("ppt_master_agent_design_plan_dedup", 2);

        assert_eq!(first, "ppt_master_agent_design_plan");
        assert_eq!(retry, "ppt_master_agent_design_plan_json_retry_2");
        assert!(retry.starts_with("ppt_master_agent_design_plan"));
        assert!(dedup_retry.starts_with("ppt_master_agent_design_plan"));
    }

    #[test]
    fn structured_planning_retry_contains_only_the_schema_error_summary() {
        let original = "CURRENT PAGE CONTRACT";
        let retry = native_planning_attempt_prompt(
            original,
            Some("SlideSpec: unknown field `coordinates` at line 12 column 4"),
        );

        assert!(retry.starts_with(original));
        assert!(retry.contains("unknown field `coordinates`"));
        assert!(retry.contains("Do not include or reconstruct the previous response"));
        assert!(!retry.contains("RAW_MODEL_RESPONSE_SENTINEL"));
        assert!(retry.chars().count() < original.chars().count() + 700);
    }

    #[test]
    fn strict_native_missing_page_fails_without_creating_fallback_svg() {
        let root = temp_dir("中文 空格 missing");
        let svg_output = root.join("svg_output");
        fs::create_dir_all(&svg_output).unwrap();
        let plan = plan(vec![slide(1, "cover"), slide(2, "architecture")]);
        fs::write(
            svg_output.join("01_cover.svg"),
            valid_native_svg("原生封面"),
        )
        .unwrap();

        let error = validate_native_svg_set(&plan, &svg_output).unwrap_err();
        assert!(error.to_string().contains("02_architecture.svg"));
        assert!(!svg_output.join("02_architecture.svg").exists());
        assert_eq!(
            fs::read_dir(&svg_output)
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_native_accepts_complete_svg_set_in_chinese_space_path() {
        let root = temp_dir("中文 空格 complete");
        let svg_output = root.join("项目 目录").join("svg_output");
        fs::create_dir_all(&svg_output).unwrap();
        let plan = plan(vec![slide(1, "cover"), slide(2, "process")]);
        for slide in &plan.slides {
            fs::write(
                svg_output.join(svg_filename_for_slide(slide)),
                valid_native_svg(&slide.title),
            )
            .unwrap();
        }
        validate_native_svg_set(&plan, &svg_output).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_native_rejects_invalid_or_legacy_sized_svg() {
        let error = validate_native_svg_text(
            "02_process.svg",
            r#"<svg viewBox="0 0 1600 900" width="1600" height="900"></svg>"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("1280 720"));
        assert!(error.to_string().contains("02_process.svg"));

        let error = validate_native_svg_text(
            "03_algorithm.svg",
            "```svg\n<svg viewBox=\"0 0 1280 720\" width=\"1280\" height=\"720\"></svg>\n```",
        )
        .unwrap_err();
        assert!(error.to_string().contains("Markdown"));
    }

    #[test]
    fn semantic_legacy_is_not_misclassified_as_internal_pipeline_leakage() {
        let root = temp_dir("semantic legacy");
        fs::write(
            root.join("06_legacy.svg"),
            valid_native_svg("GLOBAL INFLUENCE · LEGACY · REFLECTION"),
        )
        .unwrap();

        let issues = scan_final_text_leaks(&root).unwrap();
        assert!(issues.is_empty(), "semantic legacy must remain visible");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_legacy_mode_and_fallback_terms_remain_hard_leaks() {
        let root = temp_dir("internal legacy mode");
        fs::write(
            root.join("02_internal.svg"),
            valid_native_svg("legacy mode · fallback"),
        )
        .unwrap();

        let issues = scan_final_text_leaks(&root).unwrap();
        assert_eq!(issues.len(), 1);
        assert!(issues[0]
            .leaked_terms
            .iter()
            .any(|term| term == "legacy mode"));
        assert!(issues[0].leaked_terms.iter().any(|term| term == "fallback"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_compatibility_normalizes_group_opacity_without_fallback() {
        let source = r##"<svg viewBox="0 0 1280 720" width="1280" height="720"><defs><filter id="shadow"><feGaussianBlur stdDeviation="2"/></filter></defs><g opacity="0.12"><rect width="10" height="10" fill="#fff" filter="url(#shadow)"/></g><g stroke="#fff" opacity='0.5'><line x2="5"/></g><text>修复闭合标签</texttext></svg>"##;
        let (normalized, report) = normalize_native_svg_compatibility(source);
        assert_eq!(report.group_opacity_normalized, 2);
        assert_eq!(report.filters_removed, 2);
        assert_eq!(report.malformed_closing_tags_repaired, 1);
        assert!(!native_group_opacity_regex().is_match(&normalized));
        assert!(!native_filter_definition_regex().is_match(&normalized));
        assert!(!native_filter_reference_regex().is_match(&normalized));
        assert!(!normalized.contains("</texttext>"));
        assert!(normalized.contains("</text>"));
        assert!(normalized.contains("fill-opacity=\"0.12\""));
        assert!(normalized.contains("stroke-opacity=\"0.5\""));
        validate_native_svg_text("01_native.svg", &normalized).unwrap();
    }

    #[test]
    fn native_compatibility_normalizes_rgba_colors_and_combines_existing_opacity() {
        let source = r##"<svg viewBox="0 0 1280 720" width="1280" height="720"><defs><linearGradient id="g"><stop offset="0%" stop-color="rgba(37,99,235,0.12)"/></linearGradient></defs><rect width="10" height="10" fill='rgba(10,20,30,0.5)' fill-opacity='0.4' stroke="rgba(1,2,3,1)"/></svg>"##;
        let (normalized, report) = normalize_native_svg_compatibility(source);

        assert_eq!(report.rgba_colors_normalized, 3);
        assert!(!normalized.to_ascii_lowercase().contains("rgba("));
        assert!(normalized.contains("stop-color=\"#2563EB\""));
        assert!(normalized.contains("stop-opacity=\"0.12\""));
        assert!(normalized.contains("fill='#0A141E'"));
        assert!(normalized.contains("fill-opacity='0.2'"));
        assert!(normalized.contains("stroke=\"#010203\""));
        assert!(normalized.contains("stroke-opacity=\"1\""));
        validate_native_svg_text("02_rgba.svg", &normalized).unwrap();
    }

    #[test]
    fn native_compatibility_repairs_unambiguous_duplicate_line_coordinate() {
        let source = r##"<svg viewBox="0 0 1280 720" width="1280" height="720"><line x1="480" y2="564" x2="800" y2="564" stroke="#2563eb"/></svg>"##;
        let (normalized, report) = normalize_native_svg_compatibility(source);

        assert_eq!(report.duplicate_line_coordinates_repaired, 1);
        assert!(normalized.contains("x1=\"480\" y1=\"564\" x2=\"800\" y2=\"564\""));
        assert_eq!(normalized.matches("y1=\"").count(), 1);
        assert_eq!(normalized.matches("y2=\"").count(), 1);
        validate_native_svg_text("01_duplicate_line.svg", &normalized).unwrap();
    }

    #[test]
    fn native_compatibility_keeps_ambiguous_duplicate_line_coordinate_as_hard_error() {
        let source = r##"<svg viewBox="0 0 1280 720" width="1280" height="720"><line x1="480" y2="560" x2="800" y2="564"/></svg>"##;
        let (normalized, report) = normalize_native_svg_compatibility(source);

        assert_eq!(report.duplicate_line_coordinates_repaired, 0);
        assert_eq!(normalized, source);
    }

    #[test]
    fn quality_parser_reports_hard_error_but_not_warnings() {
        let output = r#"
[ERROR] 01_slide01_origin.svg - Failed
   [ERROR] Invalid XML: mismatched tag: line 59, column 133 — SVG must be well-formed XML.

[WARN] 02_slide02_rise.svg - Passed (with warnings)
   [WARN] Top-level visible <g> #3 has no id

[SUMMARY] Check Summary
"#;
        let failures = parse_native_quality_failures(output, "");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].page_number, Some(1));
        assert_eq!(failures[0].file_name, "01_slide01_origin.svg");
        assert_eq!(failures[0].violated_rule, "XML well-formedness");
        assert!(failures[0].checker_summary.contains("line 59"));
    }

    #[test]
    fn clip_path_quality_error_is_classified_before_image_keyword() {
        let output = r#"
[ERROR] 01_mao_01_cover.svg - Failed
   [ERROR] clip-path is only allowed on <image> elements or pptx_to_svg crop wrappers — for shapes, draw the target shape directly instead of clipping
"#;
        let failures = parse_native_quality_failures(output, "");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].page_number, Some(1));
        assert_eq!(
            failures[0].violated_rule,
            "unsupported clipPath on non-image shape"
        );
    }

    #[test]
    fn strict_svg_validation_gets_one_bounded_page_only_retry() {
        assert!(native_page_validation_may_retry("validate_svgs", 1));
        assert!(!native_page_validation_may_retry("validate_svgs", 2));
        assert!(native_page_validation_may_retry(
            "validate_space_utilization",
            1
        ));
        assert!(!native_page_validation_may_retry(
            "validate_space_utilization",
            2
        ));
        assert!(native_page_validation_may_retry(
            "validate_text_geometry",
            1
        ));
        assert!(!native_page_validation_may_retry(
            "validate_text_geometry",
            2
        ));
        assert!(!native_page_validation_may_retry("export_pptx", 1));
    }

    #[test]
    fn density_relayout_must_preserve_all_visible_text_in_order() {
        let before = vec![
            "标题".to_string(),
            "事实一".to_string(),
            "事实二".to_string(),
        ];
        assert!(native_page_relayout_preserved_visible_text(
            &before, &before
        ));
        assert!(!native_page_relayout_preserved_visible_text(
            &before,
            &["标题".to_string(), "事实一".to_string()]
        ));
        assert!(!native_page_relayout_preserved_visible_text(
            &before,
            &[
                "标题".to_string(),
                "事实二".to_string(),
                "事实一".to_string(),
            ]
        ));
    }

    #[test]
    fn executor_clip_path_rule_has_no_ambiguous_exception() {
        let lower = NATIVE_EXECUTOR_CLIP_PATH_RULE.to_ascii_lowercase();
        assert!(lower.contains("never generate <clippath>"));
        assert!(lower.contains("no executor exceptions"));
        assert!(!lower.contains("unless explicitly supported"));
    }

    #[test]
    fn page_validation_does_not_assign_another_pages_hard_error_to_current_page() {
        let output = r#"
[WARN] 01_cover.svg - Passed (with warnings)
   [WARN] Top-level visible <g> #3 has no id

[ERROR] 02_process.svg - Failed
   [ERROR] Invalid XML: mismatched tag: line 12, column 8 — SVG must be well-formed XML.
"#;
        assert!(native_page_quality_failure("01_cover.svg", false, output, "").is_none());
        let failure = native_page_quality_failure("02_process.svg", false, output, "").unwrap();
        assert_eq!(failure.violated_rule, "XML well-formedness");
        assert!(failure.checker_summary.contains("02_process.svg"));
    }

    #[test]
    fn text_geometry_report_preserves_measured_and_allowed_bounds() {
        let report: NativeTextGeometryReport = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "svgPath": "svg_output/02_process.svg",
            "passed": false,
            "hardErrors": [{
                "rule": "text_outside_declared_region",
                "text": "跨越安全边界的流程说明",
                "actualBounds": { "x": 210.0, "y": 340.0, "width": 420.0, "height": 52.0 },
                "allowedBounds": { "x": 220.0, "y": 340.0, "width": 380.0, "height": 64.0 },
                "collisionWith": null
            }],
            "warnings": [],
            "autoFixApplied": []
        }))
        .expect("parse geometry report");

        assert!(!report.passed);
        assert_eq!(report.violated_rule(), "text_outside_declared_region");
        assert_eq!(report.hard_errors[0]["actualBounds"]["width"], 420.0);
        assert_eq!(report.hard_errors[0]["allowedBounds"]["width"], 380.0);
        let state = report.state();
        assert!(!state.passed);
        assert_eq!(state.hard_errors[0]["text"], "跨越安全边界的流程说明");
    }

    #[test]
    fn text_geometry_retry_summary_groups_targets_and_prioritizes_canvas_overflow() {
        let report: NativeTextGeometryReport = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "svgPath": "svg_output/03_page.svg",
            "passed": false,
            "hardErrors": [
                {
                    "rule": "text_outside_declared_region",
                    "domIndex": 3,
                    "regionId": "label",
                    "role": "label",
                    "text": "label text",
                    "overflow": { "top": 2.25 }
                },
                {
                    "rule": "text_outside_canvas",
                    "domIndex": 4,
                    "regionId": "body",
                    "role": "body",
                    "text": "long body outside canvas",
                    "overflow": { "right": 20.0 }
                },
                {
                    "rule": "text_outside_declared_region",
                    "domIndex": 4,
                    "regionId": "body",
                    "role": "body",
                    "text": "long body outside canvas",
                    "overflow": { "right": 96.0 }
                }
            ],
            "warnings": [],
            "autoFixApplied": []
        }))
        .expect("parse geometry report");

        assert_eq!(report.violated_rule(), "text_outside_canvas");
        let summary = report.summary();
        assert!(summary.contains("actionableIssues="));
        assert!(summary.contains("label"));
        assert!(summary.contains("body"));
        assert!(summary.contains("text_outside_canvas"));
        assert!(summary.contains("text_outside_declared_region"));
        assert!(!summary.contains("firstIssue="));
        assert_eq!(
            report.actionable_issues()["targets"]
                .as_array()
                .expect("target array")
                .len(),
            2
        );
    }

    #[test]
    fn powerpoint_geometry_report_identifies_only_the_failed_page() {
        let report: NativePowerPointGeometryReport = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "passed": false,
            "pptxPath": "exports/native.pptx",
            "renderDir": "analysis/powerpoint_text_geometry_render",
            "hardErrors": [
                {
                    "pageNumber": 4,
                    "rule": "powerpoint_text_outside_declared_region",
                    "text": "50 亿册",
                    "actualBounds": { "x": 710.0, "y": 160.0, "width": 180.0, "height": 42.0 },
                    "allowedBounds": { "x": 720.0, "y": 160.0, "width": 160.0, "height": 48.0 }
                },
                {
                    "pageNumber": 4,
                    "rule": "powerpoint_text_collision",
                    "text": "指标说明",
                    "collisionWith": "metric-value"
                }
            ],
            "warnings": [],
            "pages": []
        }))
        .expect("parse PowerPoint geometry report");

        assert_eq!(report.first_page(), Some(4));
        assert_eq!(
            report.first_rule(),
            "powerpoint_text_outside_declared_region"
        );
        assert_eq!(report.issues_for_page(4).len(), 2);
        assert!(report.issues_for_page(3).is_empty());
        assert!(report.summary().contains("hardErrors=2"));
    }

    #[test]
    fn powerpoint_geometry_checker_uses_bounded_relative_font_drift_repair() {
        assert!(NATIVE_POWERPOINT_GEOMETRY_CHECKER_SOURCE
            .contains("function Maximum-SafeRegionOverflow"));
        assert!(
            NATIVE_POWERPOINT_GEOMETRY_CHECKER_SOURCE.contains("[Math]::Min(12.0, $extent * 0.2)")
        );
        assert!(NATIVE_POWERPOINT_GEOMETRY_CHECKER_SOURCE
            .contains("$maxOverflow -gt (Maximum-SafeRegionOverflow $issue)"));
        assert!(!NATIVE_POWERPOINT_GEOMETRY_CHECKER_SOURCE.contains("$maxOverflow -gt 7.5"));
    }

    #[test]
    fn stored_powerpoint_region_drift_can_resume_without_another_ai_page_call() {
        let recoverable = format!(
            "PowerPoint actual text bounds failed: {}",
            serde_json::json!([
                {
                    "rule": "powerpoint_text_outside_declared_region",
                    "allowedBounds": { "x": 450.0, "y": 22.5, "width": 450.0, "height": 42.0 },
                    "overflow": { "left": 0.0, "top": 0.0, "right": 11.0, "bottom": 0.0 }
                },
                {
                    "rule": "powerpoint_text_outside_declared_region",
                    "allowedBounds": { "x": 292.5, "y": 150.0, "width": 375.0, "height": 97.5 },
                    "overflow": { "left": 0.0, "top": 0.0, "right": 0.0, "bottom": 10.0 }
                }
            ])
        );
        assert!(stored_powerpoint_region_drift_is_safely_recheckable(Some(
            &recoverable
        )));

        let too_large = recoverable.replace("\"right\":11.0", "\"right\":13.0");
        assert!(!stored_powerpoint_region_drift_is_safely_recheckable(Some(
            &too_large
        )));

        let collision = recoverable.replace(
            "powerpoint_text_outside_declared_region",
            "powerpoint_text_text_overlap",
        );
        assert!(!stored_powerpoint_region_drift_is_safely_recheckable(Some(
            &collision
        )));
        assert!(!stored_powerpoint_region_drift_is_safely_recheckable(Some(
            "invalid report"
        )));
    }

    #[test]
    fn retry_selection_reuses_validated_pages_and_only_schedules_failed_page() {
        let plan = plan(vec![
            slide(1, "cover"),
            slide(2, "architecture"),
            slide(3, "process"),
            slide(4, "summary"),
        ]);
        let reusable = HashSet::from([1, 2, 4]);
        assert_eq!(native_pages_requiring_generation(&plan, &reusable), vec![3]);
        assert_eq!(sorted_page_list(&reusable), "P01,P02,P04");
    }

    #[test]
    fn resume_preflight_rejects_incomplete_and_legacy_canvas_pages() {
        let incomplete = valid_native_svg("正文").replace("</svg>", "");
        assert!(validate_native_svg_text("04_incomplete.svg", &incomplete).is_err());
        assert!(validate_native_svg_text(
            "04_legacy.svg",
            r#"<svg viewBox="0 0 1600 900" width="1600" height="900"></svg>"#,
        )
        .is_err());
        validate_native_svg_text("04_native.svg", &valid_native_svg("正文")).unwrap();
    }

    #[test]
    fn quality_failure_result_contains_actionable_backend_fields() {
        let project = temp_dir("quality metadata");
        fs::create_dir_all(project.join("svg_output")).unwrap();
        fs::write(
            project.join("svg_output").join("01_origin.svg"),
            valid_native_svg("正文"),
        )
        .unwrap();
        let result = PptMasterGenerateResult::failure(
            "quality failed".to_string(),
            "agent".to_string(),
            "ppt_master_native".to_string(),
            1,
        );
        let result = with_native_quality_failure(
            result,
            "validate_svgs",
            &project,
            Some(1),
            "01_origin.svg",
            "XML well-formedness",
            "Invalid XML: mismatched tag",
        );
        assert_eq!(result.stage.as_deref(), Some("validate_svgs"));
        assert_eq!(result.page_number, Some(1));
        assert_eq!(result.failed_svg_file.as_deref(), Some("01_origin.svg"));
        assert!(result
            .svg_path
            .as_deref()
            .unwrap()
            .ends_with("01_origin.svg"));
        assert_eq!(result.violated_rule.as_deref(), Some("XML well-formedness"));
        assert!(result
            .checker_summary
            .as_deref()
            .unwrap()
            .contains("mismatched tag"));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn native_configuration_errors_cover_missing_root_python_and_resources() {
        let root = temp_dir("configuration");
        let missing_root = root.join("missing ppt-master");
        assert!(parse_dir("ppt-master 根目录", missing_root.to_str().unwrap()).is_err());

        let missing_python = root.join("missing python.exe");
        assert!(python_version(&root, missing_python.to_str().unwrap()).is_err());

        let error = read_ppt_master_resources(&root).unwrap_err();
        assert!(error.to_string().contains("modes/_index.md"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn blocking_quality_failure_stops_after_attempts_are_exhausted() {
        let root = temp_dir("blocking quality failure");
        let svg = root.join("01.svg");
        fs::write(&svg, valid_native_svg("last version")).unwrap();

        assert!(!should_continue_after_quality_failure(true, &svg));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_blocking_quality_failure_keeps_last_svg_and_reaches_exporter() {
        let root = temp_dir("non blocking quality failure");
        let svg = root.join("01.svg");
        let last_svg = valid_native_svg("last version");
        fs::write(&svg, &last_svg).unwrap();
        let pptx = root.join("exported.pptx");
        fs::write(&pptx, b"test pptx fixture").unwrap();

        assert!(should_continue_after_quality_failure(false, &svg));
        assert_eq!(fs::read_to_string(&svg).unwrap(), last_svg);
        let export = PptMasterExportResult {
            success: true,
            output_path: Some(pptx.to_string_lossy().to_string()),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 10,
            error: None,
        };
        assert_eq!(validate_native_export_result(&export).unwrap(), pptx);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_blocking_quality_failure_still_returns_exporter_failure() {
        let root = temp_dir("non blocking export failure");
        let svg = root.join("01.svg");
        fs::write(&svg, valid_native_svg("last version")).unwrap();
        assert!(should_continue_after_quality_failure(false, &svg));

        let export = PptMasterExportResult {
            success: false,
            output_path: None,
            exit_code: Some(1),
            stdout: String::new(),
            stderr: "converter failed".to_string(),
            duration_ms: 10,
            error: Some("ppt-master 导出失败".to_string()),
        };
        let error = validate_native_export_result(&export).unwrap_err();
        assert!(error.to_string().contains("导出失败"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_export_success_returns_existing_editable_pptx_path() {
        let root = temp_dir("export success");
        let pptx = root.join("工业机器人 技术方案.pptx");
        fs::write(&pptx, b"test pptx fixture").unwrap();
        let export = PptMasterExportResult {
            success: true,
            output_path: Some(pptx.to_string_lossy().to_string()),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 10,
            error: None,
        };
        assert_eq!(validate_native_export_result(&export).unwrap(), pptx);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod native_real_entry_tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct NativeDebugInputSnapshot {
        schema_version: u32,
        source_project_path: String,
        source_input_fingerprint: String,
        payload: PptMasterGenerateInput,
    }

    fn required_env(name: &str) -> String {
        std::env::var(name)
            .unwrap_or_else(|_| panic!("missing required environment variable: {name}"))
    }

    /// Replays the exact payload captured from the Pomegranate confirmation page.
    /// The environment switch is deliberately debug-only and forces a fresh project,
    /// while the request still enters through the same public service method as Tauri.
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires a local payload snapshot and configured AI model"]
    async fn native_debug_loop_from_snapshot() {
        let database_path = required_env("POME_NATIVE_REAL_DB");
        let snapshot_path = PathBuf::from(required_env("POME_NATIVE_DEBUG_SNAPSHOT"));
        let result_path = PathBuf::from(required_env("POME_NATIVE_DEBUG_RESULT"));
        let snapshot_raw = fs::read_to_string(&snapshot_path).unwrap_or_else(|error| {
            panic!(
                "read native debug snapshot failed: {} ({error})",
                snapshot_path.display()
            )
        });
        let mut snapshot: NativeDebugInputSnapshot = serde_json::from_str(&snapshot_raw)
            .unwrap_or_else(|error| panic!("parse native debug snapshot failed: {error}"));
        if let Ok(value) = std::env::var("POME_NATIVE_DEBUG_BLOCK_ON_QUALITY_FAILURE") {
            snapshot.payload.block_on_quality_failure = Some(match value.as_str() {
                "true" => true,
                "false" => false,
                other => panic!("unsupported POME_NATIVE_DEBUG_BLOCK_ON_QUALITY_FAILURE: {other}"),
            });
        }
        assert_eq!(snapshot.schema_version, 1);
        assert_eq!(snapshot.payload.generation_mode.as_deref(), Some("agent"));
        assert_eq!(
            snapshot.payload.generation_engine.as_deref(),
            Some("ppt_master_native")
        );
        assert_eq!(snapshot.payload.slide_count, Some(6));

        let db = Database::init(&database_path).expect("open configured application database");
        let prompt = snapshot.payload.prompt.trim();
        let planning_context = build_generation_planning_context(&snapshot.payload, prompt);
        let title = snapshot
            .payload
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("AI PPT");
        let style = snapshot
            .payload
            .style
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("科技蓝");
        let root = PathBuf::from(&snapshot.payload.ppt_master_root);
        let theme_spec = NativeThemeSpec::from_inputs(
            style,
            snapshot.payload.custom_style.as_deref(),
            snapshot.payload.extra_requirements.as_deref(),
            snapshot
                .payload
                .visual_suggestions
                .as_deref()
                .or(snapshot.payload.visual_expression_advice.as_deref()),
        );
        let style_mapping = resolve_style_mapping(&root, style, &snapshot.payload, &theme_spec);
        let (replayed_fingerprint, _) = build_native_input_fingerprint(
            &db,
            &snapshot.payload,
            &planning_context,
            title,
            snapshot.payload.slide_count.unwrap_or(6),
            &style_mapping,
            &theme_spec,
        )
        .expect("compute replayed native fingerprint");
        assert!(!snapshot.source_input_fingerprint.trim().is_empty());
        if replayed_fingerprint != snapshot.source_input_fingerprint {
            println!(
                "[Native Debug] sourceFingerprint={} replayedFingerprint={} reason=generation-spec-version-changed payloadSource=same-snapshot",
                snapshot.source_input_fingerprint, replayed_fingerprint
            );
        }

        let resume_project = std::env::var("POME_NATIVE_DEBUG_RESUME_PROJECT")
            .ok()
            .map(PathBuf::from);
        let result = if let Some(project) = resume_project.as_ref() {
            PptMasterService::generate_from_prompt_ppt_master_native_with_project(
                &db,
                snapshot.payload,
                Some(project.clone()),
            )
            .await
            .expect("native resume service should return a structured result")
        } else {
            std::env::set_var("POME_NATIVE_DEBUG_FORCE_NEW_PROJECT", "1");
            let result = PptMasterService::generate_from_prompt(&db, snapshot.payload)
                .await
                .expect("native service should return a structured result");
            std::env::remove_var("POME_NATIVE_DEBUG_FORCE_NEW_PROJECT");
            result
        };

        if let Some(parent) = result_path.parent() {
            fs::create_dir_all(parent).expect("create native debug result directory");
        }
        let result_json = serde_json::to_string_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "sourceProjectPath": snapshot.source_project_path,
            "sourceInputFingerprint": snapshot.source_input_fingerprint,
            "replayedInputFingerprint": replayed_fingerprint,
            "completedAt": chrono::Utc::now().to_rfc3339(),
            "result": result,
        }))
        .expect("serialize native debug result");
        fs::write(&result_path, result_json).expect("write native debug result");

        let result_raw = fs::read_to_string(&result_path).expect("read written debug result");
        let result_value: serde_json::Value =
            serde_json::from_str(&result_raw).expect("parse written debug result");
        let result = &result_value["result"];
        println!(
            "[Native Debug Result] success={} project={} pptx={} stage={} qualityCheckPassed={} fallbackUsed={} error={} resultFile={}",
            result["success"].as_bool().unwrap_or(false),
            result["projectPath"].as_str().unwrap_or("-"),
            result["pptxPath"].as_str().unwrap_or("-"),
            result["failureStage"].as_str().unwrap_or("-"),
            result["qualityCheckPassed"].as_bool().unwrap_or(false),
            result["stdout"].as_str().is_some_and(|value| value.contains("fallback=true")),
            result["error"].as_str().unwrap_or("-"),
            result_path.display()
        );
        if let Some(project) = resume_project {
            assert_eq!(
                PathBuf::from(
                    result["projectPath"]
                        .as_str()
                        .expect("resumed project path")
                )
                .canonicalize()
                .expect("canonical resumed result project"),
                project
                    .canonicalize()
                    .expect("canonical requested resume project"),
                "debug resume must stay in the requested project"
            );
        } else {
            assert_ne!(
                result["projectPath"].as_str(),
                Some(snapshot.source_project_path.as_str()),
                "debug loop must create a new project"
            );
        }
        assert_eq!(result["generationMode"].as_str(), Some("agent"));
        assert_eq!(
            result["generationEngine"].as_str(),
            Some("ppt_master_native")
        );
        assert_eq!(
            result["success"].as_bool(),
            Some(true),
            "native debug loop failed; inspect {}",
            result_path.display()
        );
    }

    fn industrial_resume_input(
        ppt_master_root: String,
        python_path: String,
        topic: &str,
    ) -> PptMasterGenerateInput {
        let source_material = "面向制造企业质量检测场景，建设工业机器人视觉检测系统。\n\
项目背景：人工检测效率低、稳定性不足，复杂缺陷容易漏检。\n\
背景要点：当前存在漏检、误判、节拍波动和人工追溯困难四类问题，适合用多卡片形式表达。\n\
实施时间轴：第 1 月完成样本采集与标注，第 2 月完成算法验证，第 3 月完成产线联调，第 4 月试运行并验收。\n\
系统架构：工业相机与光源采集图像，边缘计算节点完成预处理与推理；机器人和 PLC 执行分拣，MES 记录结果并形成可追溯闭环。\n\
性能目标：单件检测节拍小于 1 秒，典型缺陷检出率不低于 99%，误判率低于 1%，支持可视化追溯。";
        PptMasterGenerateInput {
            ppt_master_root,
            python_path,
            prompt: topic.to_string(),
            planning_context: Some(format!(
                "主题：{topic}\n受众：制造企业技术负责人和项目决策者\n页数：6\n结构：封面、背景多卡片、实施时间轴、系统架构、多行技术说明、性能指标与总结"
            )),
            ai_understanding_result: Some(PptUnderstandingInput::Structured(
                PptUnderstandingDraftInput {
                    understanding_summary: "以工业机器人视觉检测的建设必要性、四阶段实施时间轴、技术架构和可量化性能为主线。".to_string(),
                    key_priorities: "突出四类背景问题、实施时间轴、系统闭环、多行技术说明和大数字性能目标。".to_string(),
                    narrative_mainline: "痛点与目标 → 四阶段实施 → 系统架构 → 技术说明 → 性能价值。".to_string(),
                    suggested_page_structure: "1 封面；2 背景多卡片；3 四阶段实施时间轴；4 系统架构；5 多行技术说明；6 大数字性能指标与总结。".to_string(),
                    visual_expression_advice: "使用背景多卡片、时间轴、架构图、多行说明和带独立单位的大数字指标。".to_string(),
                    open_questions: String::new(),
                },
            )),
            understanding_summary: Some(topic.to_string()),
            key_priorities: Some("多卡片、时间轴、架构、多行正文、大数字与单位".to_string()),
            suggested_page_structure: Some("封面、背景多卡片、实施时间轴、系统架构、多行技术说明、性能指标与总结".to_string()),
            narrative_mainline: Some("从业务痛点到实施节奏、技术闭环与量化价值".to_string()),
            visual_expression_advice: Some("多卡片、时间轴、架构图、多行说明、重点指标".to_string()),
            visual_suggestions: None,
            open_questions: Some(String::new()),
            raw_material: Some(source_material.to_string()),
            material_sources: Vec::new(),
            extra_requirements: Some("必须生成 6 页；全部页面由 ppt-master 原生链路生成；禁止 legacy fallback。".to_string()),
            model_id: None,
            title: Some(topic.to_string()),
            audience: Some("制造企业技术负责人和项目决策者".to_string()),
            slide_count: Some(6),
            style: Some("科技蓝".to_string()),
            custom_style: None,
            generation_engine: Some("ppt_master_native".to_string()),
            mode: Some("technical".to_string()),
            visual_style: Some("dark-tech".to_string()),
            layout_bias: vec!["architecture".to_string(), "process".to_string()],
            chart_bias: vec!["process_flow".to_string()],
            output_dir: std::env::var("POME_NATIVE_REAL_OUTPUT_DIR").ok(),
            generation_mode: Some("agent".to_string()),
            block_on_quality_failure: Some(true),
        }
    }

    /// 通过与前端相同的 service 入口执行真实原生链路。
    ///
    /// 默认忽略，避免普通单元测试意外调用用户配置的外部模型。仅在人工验收时显式传入
    /// 数据库、ppt-master 和 Python 路径后运行。
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires configured AI model and runs the real ppt-master native pipeline"]
    async fn native_pipeline_from_ui_equivalent_input() {
        let database_path = required_env("POME_NATIVE_REAL_DB");
        let ppt_master_root = required_env("POME_NATIVE_REAL_ROOT");
        let python_path = required_env("POME_NATIVE_REAL_PYTHON");
        let db = Database::init(&database_path).expect("open configured application database");
        let topic = "工业机器人视觉检测系统技术方案";
        let source_material = "面向制造企业质量检测场景，建设工业机器人视觉检测系统。\n\
项目背景：人工检测效率低、稳定性不足，复杂缺陷容易漏检。\n\
系统架构：工业相机与光源采集图像，边缘计算节点完成预处理与推理，机器人和 PLC 执行分拣，MES 记录结果。\n\
检测流程：工件到位、触发采图、图像校正、缺陷检测、结果判定、机器人分拣、数据追溯。\n\
核心算法：定位与配准、表面缺陷检测、尺寸测量、置信度融合和小样本增量优化。\n\
性能目标：单件检测节拍小于 1 秒，典型缺陷检出率不低于 99%，误判率低于 1%，支持可视化追溯。";

        let input = PptMasterGenerateInput {
            ppt_master_root,
            python_path,
            prompt: topic.to_string(),
            planning_context: Some(format!(
                "主题：{topic}\n受众：制造企业技术负责人和项目决策者\n页数：6\n结构：封面、项目背景、系统架构、检测流程、核心算法、性能与总结"
            )),
            ai_understanding_result: Some(PptUnderstandingInput::Structured(
                PptUnderstandingDraftInput {
                    understanding_summary: "以工业机器人视觉检测的建设必要性、技术架构、落地流程和可量化性能为主线。".to_string(),
                    key_priorities: "突出系统闭环、算法能力、工程可实施性和性能目标。".to_string(),
                    narrative_mainline: "痛点与目标 → 系统架构 → 检测闭环 → 核心算法 → 性能价值。".to_string(),
                    suggested_page_structure: "1 封面；2 项目背景；3 系统架构；4 检测流程；5 核心算法；6 性能与总结。".to_string(),
                    visual_expression_advice: "使用架构图、流程图、算法示意和重点指标，不使用重复卡片网格。".to_string(),
                    open_questions: String::new(),
                },
            )),
            understanding_summary: Some("工业机器人视觉检测系统技术方案".to_string()),
            key_priorities: Some("架构、流程、算法、性能".to_string()),
            suggested_page_structure: Some("封面、项目背景、系统架构、检测流程、核心算法、性能与总结".to_string()),
            narrative_mainline: Some("从业务痛点到技术闭环与量化价值".to_string()),
            visual_expression_advice: Some("架构图、流程图、算法示意、重点指标".to_string()),
            visual_suggestions: None,
            open_questions: Some(String::new()),
            raw_material: Some(source_material.to_string()),
            material_sources: Vec::new(),
            extra_requirements: Some("必须生成 6 页；全部页面由 ppt-master 原生链路生成；禁止 legacy fallback。".to_string()),
            model_id: None,
            title: Some(topic.to_string()),
            audience: Some("制造企业技术负责人和项目决策者".to_string()),
            slide_count: Some(6),
            style: Some("科技蓝".to_string()),
            custom_style: None,
            generation_engine: Some("ppt_master_native".to_string()),
            mode: Some("technical".to_string()),
            visual_style: Some("dark-tech".to_string()),
            layout_bias: vec!["architecture".to_string(), "process".to_string()],
            chart_bias: vec!["process_flow".to_string()],
            output_dir: std::env::var("POME_NATIVE_REAL_OUTPUT_DIR").ok(),
            generation_mode: Some("agent".to_string()),
            block_on_quality_failure: Some(true),
        };

        println!(
            "[Native Real Entry] generationMode=agent generationEngine=ppt_master_native slideCount=6 rawMaterialLength={}",
            source_material.chars().count()
        );
        let result = PptMasterService::generate_from_prompt(&db, input)
            .await
            .expect("native service should return a structured result");
        println!(
            "[Native Real Entry Result] success={} project={:?} pptx={:?} final={:?} stage={:?} type={:?}\nstdout:\n{}\nstderr:\n{}\nerror={:?}",
            result.success,
            result.project_path,
            result.pptx_path,
            result.final_pptx_path,
            result.failure_stage,
            result.failure_type,
            result.stdout,
            result.stderr,
            result.error
        );
        assert!(result.success, "native pipeline failed: {:?}", result.error);
        assert_eq!(result.generation_mode, "agent");
        assert_eq!(result.generation_engine, "ppt_master_native");
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "repairs and resumes a real pre-state ppt-master native project"]
    async fn native_resume_existing_failed_project_without_recalling_ai_for_valid_pages() {
        let database_path = required_env("POME_NATIVE_REAL_DB");
        let ppt_master_root = required_env("POME_NATIVE_REAL_ROOT");
        let python_path = required_env("POME_NATIVE_REAL_PYTHON");
        let project = PathBuf::from(required_env("POME_NATIVE_RESUME_PROJECT"));
        let db = Database::init(&database_path).expect("open configured application database");
        let plan_raw =
            fs::read_to_string(project.join("slide_plan.json")).expect("read existing slide plan");
        let plan: SlidePlan = serde_json::from_str(&plan_raw).expect("parse existing slide plan");
        let prompt = fs::read_to_string(project.join("sources").join("confirmed_prompt.md"))
            .expect("read confirmed prompt");
        let planning_context =
            fs::read_to_string(project.join("sources").join("planning_context.md"))
                .expect("read planning context");
        let input = PptMasterGenerateInput {
            ppt_master_root,
            python_path,
            prompt,
            planning_context: Some(planning_context),
            ai_understanding_result: None,
            understanding_summary: None,
            key_priorities: None,
            suggested_page_structure: None,
            narrative_mainline: None,
            visual_expression_advice: None,
            visual_suggestions: None,
            open_questions: None,
            raw_material: None,
            material_sources: Vec::new(),
            extra_requirements: Some(
                "续跑既有 ppt-master 原生严格项目；禁止 legacy/template fallback。".to_string(),
            ),
            model_id: None,
            title: Some(plan.title.clone()),
            audience: Some(plan.audience.clone()),
            slide_count: Some(plan.slides.len()),
            style: Some(plan.style.clone()),
            custom_style: None,
            generation_engine: Some("ppt_master_native".to_string()),
            mode: Some("technical".to_string()),
            visual_style: Some("dark-tech".to_string()),
            layout_bias: Vec::new(),
            chart_bias: Vec::new(),
            output_dir: std::env::var("POME_NATIVE_REAL_OUTPUT_DIR").ok(),
            generation_mode: Some("agent".to_string()),
            block_on_quality_failure: Some(true),
        };
        let result = PptMasterService::generate_from_prompt_ppt_master_native_with_project(
            &db,
            input,
            Some(project.clone()),
        )
        .await
        .expect("resume service result");
        println!(
            "[Native Acceptance A] success={} project={:?} pptx={:?}\nstdout:\n{}\nstderr:\n{}\nerror={:?}",
            result.success,
            result.project_path,
            result.pptx_path,
            result.stdout,
            result.stderr,
            result.error
        );
        assert!(result.success, "resume failed: {:?}", result.error);
        let result_project = PathBuf::from(result.project_path.as_ref().expect("result project"));
        assert_eq!(
            result_project
                .canonicalize()
                .expect("canonical result project"),
            project.canonicalize().expect("canonical expected project")
        );
        assert!(!result.stdout.contains("aiCalled=true"));
        assert_eq!(
            result.stdout.matches("aiCalled=false").count(),
            plan.slides.len()
        );
        let repaired = fs::read_to_string(project.join("svg_output").join("01_slide01_origin.svg"))
            .expect("read repaired SVG");
        assert!(!repaired.contains("</texttext>"));
        let state = read_state(&project).expect("read completed state");
        assert_eq!(state.status, "completed");
        assert!(state.pages.values().all(|page| page.status == "validated"));
        assert!(state.pages.values().all(|page| page.attempts == 0));
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires configured AI model and performs real interrupted native generation"]
    async fn native_resume_after_injected_page_four_failure_only_calls_ai_for_remaining_pages() {
        let database_path = required_env("POME_NATIVE_REAL_DB");
        let ppt_master_root = required_env("POME_NATIVE_REAL_ROOT");
        let python_path = required_env("POME_NATIVE_REAL_PYTHON");
        let db = Database::init(&database_path).expect("open configured application database");
        let topic = std::env::var("POME_NATIVE_TEST_TOPIC").unwrap_or_else(|_| {
            format!(
                "工业机器人视觉检测系统断点续跑验收-{}",
                chrono::Utc::now().format("%Y%m%d%H%M%S")
            )
        });

        std::env::set_var("POME_NATIVE_TEST_FAIL_BEFORE_PAGE", "4");
        let first = PptMasterService::generate_from_prompt(
            &db,
            industrial_resume_input(ppt_master_root.clone(), python_path.clone(), &topic),
        )
        .await
        .expect("first interrupted native result");
        std::env::remove_var("POME_NATIVE_TEST_FAIL_BEFORE_PAGE");
        println!(
            "[Native Acceptance B First Run] success={} project={:?} stage={:?} error={:?}\n{}",
            first.success, first.project_path, first.failure_stage, first.error, first.stdout
        );
        assert!(!first.success, "first run must stop before page 4");
        let project = PathBuf::from(first.project_path.expect("interrupted project path"));
        let first_state = read_state(&project).expect("read interrupted state");
        let first_attempts = (1..=3)
            .map(|page| {
                let page_state = &first_state.pages[&page.to_string()];
                assert_eq!(page_state.status, "validated");
                assert!(page_state.attempts >= 1);
                (page, page_state.attempts)
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(first_state.pages["4"].attempts, 0);
        assert_eq!(
            fs::read_dir(project.join("svg_output"))
                .expect("read partial SVG directory")
                .filter_map(Result::ok)
                .filter(
                    |entry| entry.path().extension().and_then(|value| value.to_str())
                        == Some("svg")
                )
                .count(),
            3
        );

        let second = PptMasterService::generate_from_prompt(
            &db,
            industrial_resume_input(ppt_master_root, python_path, &topic),
        )
        .await
        .expect("resumed native result");
        println!(
            "[Native Acceptance B Resume] success={} project={:?} pptx={:?}\nstdout:\n{}\nstderr:\n{}\nerror={:?}",
            second.success,
            second.project_path,
            second.pptx_path,
            second.stdout,
            second.stderr,
            second.error
        );
        assert!(second.success, "resume failed: {:?}", second.error);
        let second_project = PathBuf::from(second.project_path.as_ref().expect("resume project"));
        assert_eq!(
            second_project
                .canonicalize()
                .expect("canonical resume project"),
            project
                .canonicalize()
                .expect("canonical interrupted project")
        );
        for page in 1..=3 {
            assert!(second.stdout.contains(&format!(
                "page=P{page:02} action=reuse status=validated aiCalled=false"
            )));
            assert!(!second
                .stdout
                .contains(&format!("page=P{page:02} action=generate aiCalled=true")));
        }
        for page in 4..=6 {
            assert!(second
                .stdout
                .contains(&format!("page=P{page:02} action=generate aiCalled=true")));
        }
        let state = read_state(&project).expect("read completed state");
        assert_eq!(state.status, "completed");
        assert!(state.pages.values().all(|page| page.status == "validated"));
        for page in 1..=3 {
            assert_eq!(
                state.pages[&page.to_string()].attempts,
                first_attempts[&page],
                "reused page P{page:02} must not consume another AI attempt"
            );
        }
        for page in 4..=6 {
            assert!(state.pages[&page.to_string()].attempts >= 1);
        }
        let plan = load_native_planning_artifacts(&project, 6)
            .expect("load completed plan")
            .0;
        validate_native_svg_set(&plan, &project.join("svg_output"))
            .expect("all resumed SVG pages are 1280x720 native pages");
        let legacy_count = fs::read_dir(project.join("svg_output"))
            .expect("read completed SVG directory")
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter(|svg| svg.contains("1600 900") || svg.contains("width=\"1600\""))
            .count();
        assert_eq!(legacy_count, 0);
    }

    /// Continues the persisted Acceptance B project after a human-reviewed SVG
    /// repair.  This entry deliberately has no failure injection: preflight must
    /// revalidate pages 1-4, reuse them, and call AI only for pages still absent.
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires configured AI model and continues the persisted native acceptance project"]
    async fn native_complete_existing_interrupted_acceptance_project() {
        let database_path = required_env("POME_NATIVE_REAL_DB");
        let ppt_master_root = required_env("POME_NATIVE_REAL_ROOT");
        let python_path = required_env("POME_NATIVE_REAL_PYTHON");
        let topic = required_env("POME_NATIVE_TEST_TOPIC");
        let db = Database::init(&database_path).expect("open configured application database");

        let result = PptMasterService::generate_from_prompt(
            &db,
            industrial_resume_input(ppt_master_root, python_path, &topic),
        )
        .await
        .expect("continued native result");
        println!(
            "[Native Acceptance B Continue] success={} project={:?} pptx={:?}\nstdout:\n{}\nstderr:\n{}\nerror={:?}",
            result.success,
            result.project_path,
            result.pptx_path,
            result.stdout,
            result.stderr,
            result.error
        );
        assert!(result.success, "continuation failed: {:?}", result.error);
        let project = PathBuf::from(result.project_path.as_ref().expect("continued project"));
        for page in 1..=4 {
            assert!(result.stdout.contains(&format!(
                "page=P{page:02} action=reuse status=validated aiCalled=false"
            )));
            assert!(!result
                .stdout
                .contains(&format!("page=P{page:02} action=generate aiCalled=true")));
        }
        for page in 5..=6 {
            let reused = result.stdout.contains(&format!(
                "page=P{page:02} action=reuse status=validated aiCalled=false"
            ));
            let generated = result
                .stdout
                .contains(&format!("page=P{page:02} action=generate aiCalled=true"));
            assert_ne!(
                reused, generated,
                "page P{page:02} must be either strictly reused or generated, never both/neither"
            );
        }
        let state = read_state(&project).expect("read completed continuation state");
        assert_eq!(state.status, "completed");
        assert!(state.pages.values().all(|page| page.status == "validated"));
        let plan = load_native_planning_artifacts(&project, 6)
            .expect("load continued plan")
            .0;
        validate_native_svg_set(&plan, &project.join("svg_output"))
            .expect("all continued SVG pages are strict 1280x720 native pages");
        let legacy_count = fs::read_dir(project.join("svg_output"))
            .expect("read continued SVG directory")
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter(|svg| svg.contains("1600 900") || svg.contains("width=\"1600\""))
            .count();
        assert_eq!(legacy_count, 0);
        let pptx = result
            .final_pptx_path
            .as_ref()
            .or(result.pptx_path.as_ref())
            .map(PathBuf::from)
            .expect("continued PPTX path");
        assert!(
            pptx.is_file(),
            "continued PPTX must exist: {}",
            pptx.display()
        );
    }
}
