use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::time::{timeout, Duration};

use crate::database::Database;
use crate::error::AppError;
use crate::models::{PluginAiChatInput, PluginAiMessage};
use crate::services::ai::AiService;

#[path = "ppt_master_native_quality.rs"]
mod native_quality;
#[path = "ppt_master_strict.rs"]
mod strict_engine;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PptUnderstandingInput {
    Structured(PptUnderstandingDraftInput),
    Legacy(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PptMaterialSourceInput {
    pub id: i64,
    pub source_type: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub block_on_quality_failure: Option<bool>,
    #[serde(default)]
    pub native_quality_enabled: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
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
        let engine = input
            .generation_engine
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("legacy_fallback");
        println!("[Engine] generation_engine={}", engine);
        if engine == "legacy_fallback" {
            return Self::generate_from_prompt_template(db, input).await;
        }

        let mode = input
            .generation_mode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("template");
        if mode == "template" {
            return Self::generate_from_prompt_template(db, input).await;
        }
        Self::generate_from_prompt_ppt_master_native(db, input).await
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
                if extract_material_units(&planning_context).is_empty() {
                    return Err(AppError::Custom(
                        "AI slide_plan 生成失败，且未能从用户语料中提取可用内容，已停止生成，避免输出占位 PPT。".into(),
                    ));
                }
                default_slide_plan(&title, requested_count, &style, &planning_context)
            }
        };
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
                    let repaired = normalize_slide_plan(repaired, &title, requested_count, &style);
                    if validate_stable_content_plan(&repaired).is_none() {
                        plan = repaired;
                        log_lines.push("[Stable Content Repair] done".to_string());
                    } else {
                        enrich_plan_from_material(&mut plan, &planning_context);
                        log_lines.push("[Stable Content Repair] ai output still thin; fallback enrichment applied".to_string());
                    }
                }
                Err(e) => {
                    enrich_plan_from_material(&mut plan, &planning_context);
                    log_lines.push(format!(
                        "[Stable Content Repair] ai failed; fallback enrichment applied: {}",
                        e
                    ));
                }
            }
        }
        if let Some(report) = validate_stable_content_plan(&plan) {
            return Err(AppError::Custom(format!(
                "Stable slide_plan content is still too thin after repair: {}",
                report
            )));
        }
        log_stable_content_check(&plan, "[Stable Content Check Final]", &mut log_lines);
        let native_quality_report = if native_quality::is_enabled(&input) {
            let outcome =
                native_quality::apply_native_quality_chain(plan, &input, &planning_context);
            plan = outcome.plan;
            log_lines.extend(outcome.log_lines);
            Some(outcome.report)
        } else {
            None
        };
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
        if let Some(report) = &native_quality_report {
            let path = project.join(native_quality::NATIVE_QUALITY_REPORT_FILE);
            let json = serde_json::to_string_pretty(report).map_err(|e| {
                AppError::Custom(format!("serialize native quality report failed: {}", e))
            })?;
            write_file(&path, &format!("{json}\n"))?;
            log_lines.push(format!(
                "[Native Quality] report={}",
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(native_quality::NATIVE_QUALITY_REPORT_FILE)
            ));
        }
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
                "[Stable Visual Diversity] duplicate_signature={} motif_reuse_count={} signature={}",
                rendered.duplicate_signature,
                rendered.motif_reuse_count,
                rendered.visual_signature
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
        })
    }

    async fn generate_from_prompt_ppt_master_native(
        db: &Database,
        input: PptMasterGenerateInput,
    ) -> Result<PptMasterGenerateResult, AppError> {
        if std::env::var("POME_PPT_NATIVE_ENGINE")
            .ok()
            .as_deref()
            != Some("baseline")
        {
            let request = serde_json::to_value(&input).map_err(|error| {
                AppError::Custom(format!("序列化严格 PPT 生成请求失败: {error}"))
            })?;
            let response = strict_engine::generate_native_from_value(db, request).await?;
            return serde_json::from_value(response).map_err(|error| {
                AppError::Custom(format!("解析严格 PPT 生成结果失败: {error}"))
            });
        }

        let started = Instant::now();
        println!("[PPT Pipeline] service entered");
        let root = parse_dir("ppt-master 根目录", &input.ppt_master_root)?;
        println!("[Config] pptMasterRoot={}", root.display());
        println!("[Config] pythonPath={}", input.python_path);
        println!("[Engine] generation_engine=ppt_master_native");
        ensure_python_available(&root, &input.python_path)?;

        let export_script = root.join(SVG_TO_PPTX_SCRIPT);
        if !export_script.is_file() {
            return Err(AppError::NotFound(format!(
                "找不到 svg_to_pptx.py 脚本: {}",
                export_script.display()
            )));
        }
        for script in [
            PROJECT_MANAGER_SCRIPT,
            TOTAL_MD_SPLIT_SCRIPT,
            FINALIZE_SVG_SCRIPT,
        ] {
            let script_path = root.join(script);
            if !script_path.is_file() {
                return Err(AppError::NotFound(format!(
                    "找不到 ppt-master 脚本: {}",
                    script_path.display()
                )));
            }
        }
        let checker_script = root.join(SVG_QUALITY_CHECKER_SCRIPT);
        if !checker_script.is_file() {
            return Err(AppError::NotFound(format!(
                "找不到 svg_quality_checker.py 脚本: {}",
                checker_script.display()
            )));
        }

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
        let style_mapping = resolve_style_mapping(&root, &style, &input);

        let mut log_lines = Vec::new();
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

        println!("[Project] init start");
        log_lines.push("[Project] init start".to_string());
        let project =
            init_project_with_project_manager(&root, &input.python_path, &title, &mut log_lines)?;
        println!("[Project] init done: {}", project.display());
        log_lines.push(format!("[Project] init done: {}", project.display()));
        let sources = project.join("sources");
        let notes = project.join("notes");
        let svg_output = project.join("svg_output");
        write_file(&sources.join("confirmed_prompt.md"), prompt)?;
        write_file(&sources.join("planning_context.md"), &planning_context)?;

        let skill_text = read_ppt_master_skill(&root)?;
        let resources = read_ppt_master_resources(&root)?;
        let chart_catalog = load_chart_catalog(&root);

        println!("[AI] slide_plan start");
        log_lines.push("[AI] slide_plan start".to_string());
        let mut slide_plan_source = "ai_understanding/planning_context".to_string();
        let mut plan = match generate_agent_design_plan(
            db,
            &skill_text,
            &planning_context,
            input.model_id,
            &title,
            requested_count,
            &style,
        )
        .await
        {
            Ok(plan) => normalize_slide_plan(plan, &title, requested_count, &style),
            Err(e) => {
                log_lines.push(format!("Agent 规划失败，使用 fallback slide_plan: {}", e));
                slide_plan_source = "fallback".to_string();
                log_lines.push("[Fallback Plan] used=true".to_string());
                log_lines.push(format!("reason={}", e));
                log_lines.push("fallback_is_domain_neutral=true".to_string());
                default_slide_plan(&title, requested_count, &style, &planning_context)
            }
        };
        if slide_plan_source != "fallback" {
            log_lines.push("[Fallback Plan] used=false".to_string());
        }
        log_lines.push("[Slide Plan Source]".to_string());
        log_lines.push(format!("source={}", slide_plan_source));
        println!("[AI] slide_plan done");
        log_lines.push("[AI] slide_plan done".to_string());
        ensure_layout_variety(&mut plan);
        enrich_slide_execution_plan(&mut plan, &style_mapping, &chart_catalog);
        if let Some(duplicate_report) = detect_slide_plan_duplicates(&plan) {
            println!("[Slide Plan] duplicate detected: {}", duplicate_report);
            log_lines.push(format!(
                "[Slide Plan] duplicate detected: {}",
                duplicate_report
            ));
            log_lines.push("[Slide Plan] regenerate with de-duplication".to_string());
            match regenerate_agent_design_plan_with_dedup(
                db,
                &skill_text,
                &planning_context,
                &plan,
                &duplicate_report,
                input.model_id,
                &title,
                requested_count,
                &style,
            )
            .await
            {
                Ok(replanned) => {
                    plan = normalize_slide_plan(replanned, &title, requested_count, &style);
                    ensure_layout_variety(&mut plan);
                    enrich_slide_execution_plan(&mut plan, &style_mapping, &chart_catalog);
                    log_lines.push("[Slide Plan] de-duplication applied".to_string());
                }
                Err(e) => {
                    log_lines.push(format!(
                        "[Slide Plan] de-duplication failed, keep first plan: {}",
                        e
                    ));
                }
            }
        }
        plan.style = style_mapping.user_style.clone();
        plan.theme = theme_for_style(&plan.style);
        log_slide_plan_summary(&plan, &mut log_lines);

        copy_layout_templates(&root, &project, &style_mapping, &mut log_lines)?;

        println!("[Spec] write design_spec/spec_lock");
        log_lines.push("[Spec] write design_spec/spec_lock".to_string());
        log_design_spec_pages(&plan, &style_mapping, &mut log_lines);
        let design_spec = build_ppt_master_design_spec(&plan, &planning_context, &style_mapping);
        let spec_lock = build_ppt_master_spec_lock(&plan, &style_mapping);
        let design_spec_path = project.join("design_spec.md");
        let spec_lock_path = project.join("spec_lock.md");
        let slide_plan_path = project.join("slide_plan.json");
        write_file(&design_spec_path, &design_spec)?;
        write_file(&spec_lock_path, &spec_lock)?;
        let plan_json = serde_json::to_string_pretty(&plan)
            .map_err(|e| AppError::Custom(format!("序列化 slide_plan 失败: {}", e)))?;
        write_file(&slide_plan_path, &plan_json)?;
        write_file(&notes.join("total.md"), &build_notes(&plan))?;
        log_lines.push("[Generation]".to_string());
        log_lines.push(format!(
            "design_spec generated: {}",
            design_spec_path.display()
        ));
        log_lines.push(format!("spec_lock generated: {}", spec_lock_path.display()));
        log_lines.push(format!(
            "slide_plan generated: {}",
            slide_plan_path.display()
        ));

        println!("[SVG] generate start");
        log_lines.push("[SVG] generate start".to_string());
        for idx in 0..plan.slides.len() {
            let slide = &plan.slides[idx];
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
            let svg = match generate_ppt_master_driven_slide_svg(
                db,
                &skill_text,
                &resources,
                &design_spec,
                &spec_lock_path,
                &style_mapping,
                &plan,
                slide,
                prev_title,
                next_title,
                input.model_id,
            )
            .await
            {
                Ok(svg) => svg,
                Err(e) => {
                    log_lines.push(format!(
                        "第 {} 页 ppt-master 驱动 SVG 生成失败，先记录，稍后进入 fallback: {}",
                        slide.page, e
                    ));
                    String::new()
                }
            };
            if svg.trim().is_empty() {
                continue;
            }
            let filename = svg_filename_for_slide(slide);
            write_file(&svg_output.join(&filename), &svg)?;
            log_lines.push(format!("写入 SVG: svg_output/{}", filename));
        }

        fill_missing_svgs_with_legacy_fallback(&plan, &svg_output, &mut log_lines)?;
        log_lines.push("[SVG] all pages generated".to_string());
        enforce_final_text_guard(
            db,
            &design_spec,
            &spec_lock,
            &plan,
            &svg_output,
            input.model_id,
            &mut log_lines,
        )
        .await?;
        log_lines.push("[Check] svg_quality_checker start".to_string());
        let mut quality = run_quality_check(&root, &input.python_path, &project, started)?;
        log_lines.push(format!(
            "[Check] svg_quality_checker done: success={}",
            quality.success
        ));
        if !quality.success {
            log_lines.push(
                "[Repair] start: svg_quality_checker failed, AI repair max 1 pass".to_string(),
            );
            log_lines.push("质量检查未通过，尝试让 AI 修复 SVG（最多 1 次）".to_string());
            repair_agent_svgs_once(
                db,
                &skill_text,
                &design_spec,
                &spec_lock,
                &plan,
                &svg_output,
                &join_outputs(&[], &[quality.stdout.clone(), quality.stderr.clone()]),
                input.model_id,
            )
            .await?;
            log_lines.push("[Repair] done: svg_quality_checker AI repair".to_string());
            log_lines.push("[Check] svg_quality_checker start".to_string());
            quality = run_quality_check(&root, &input.python_path, &project, started)?;
            log_lines.push(format!(
                "[Check] svg_quality_checker done: success={}",
                quality.success
            ));
            if !quality.success {
                log_lines.push(
                    "AI 修复后仍未通过，使用 render_slide_svg 作为最终 legacy fallback".to_string(),
                );
                write_legacy_fallback_svgs(&plan, &svg_output)?;
                log_lines.push("[Check] svg_quality_checker start".to_string());
                quality = run_quality_check(&root, &input.python_path, &project, started)?;
                log_lines.push(format!(
                    "[Check] svg_quality_checker done: success={}",
                    quality.success
                ));
            }
        } else {
            log_lines.push("[Repair] skipped: svg_quality_checker passed".to_string());
        }
        log_lines.push(format!("SVG 质量检查通过: {}", quality.success));

        let split = run_total_md_split(&root, &input.python_path, &project, started)?;
        log_lines.push("total_md_split started".to_string());
        log_lines.push("[Finalize] start".to_string());
        let mut finalize = run_finalize_svg(&root, &input.python_path, &project, started)?;
        log_lines.push(format!("[Finalize] done: success={}", finalize.success));

        log_lines.push("[Native Compat] scan start: source=svg_output".to_string());
        let mut native_issues = scan_native_incompatible_svgs(&project.join("svg_output"))?;
        log_lines.push(format!(
            "[Native Compat] scan done: issue_count={}",
            native_issues.len()
        ));
        if !native_issues.is_empty() {
            log_lines.push("[Repair] start: native compatibility repair max 1 pass".to_string());
            log_lines.push(format!(
                "[Native Compat] pre-export check found unsupported SVG elements: {}",
                summarize_native_issues(&native_issues)
            ));
            repair_native_svg_issues_once(
                db,
                &design_spec,
                &spec_lock,
                &plan,
                &svg_output,
                &native_issues,
                input.model_id,
                &mut log_lines,
            )
            .await?;
            log_lines.push("[Repair] done: native compatibility repair".to_string());
            log_lines.push("[Check] svg_quality_checker start".to_string());
            quality = run_quality_check(&root, &input.python_path, &project, started)?;
            log_lines.push(format!(
                "[Check] svg_quality_checker done: success={}",
                quality.success
            ));
            log_lines.push("[Finalize] start".to_string());
            finalize = run_finalize_svg(&root, &input.python_path, &project, started)?;
            log_lines.push(format!("[Finalize] done: success={}", finalize.success));
            log_lines.push("[Native Compat] scan start: source=svg_output".to_string());
            native_issues = scan_native_incompatible_svgs(&project.join("svg_output"))?;
            log_lines.push(format!(
                "[Native Compat] scan done: issue_count={}",
                native_issues.len()
            ));
            if !native_issues.is_empty() {
                log_lines.push(format!(
                    "[Native Compat] unsupported elements remain after repair: {}",
                    summarize_native_issues(&native_issues)
                ));
            }
        } else {
            log_lines.push("[Repair] skipped: native compatibility passed".to_string());
        }

        enforce_final_text_guard(
            db,
            &design_spec,
            &spec_lock,
            &plan,
            &svg_output,
            input.model_id,
            &mut log_lines,
        )
        .await?;

        println!("[Export] start");
        log_lines.push("[Export] svg source: svg_output".to_string());
        log_lines.push(
            "[Export] command: svg_to_pptx.py <project> (native default source=svg_output)"
                .to_string(),
        );
        let mut export = export_project(&root, &input.python_path, &project, started)?;
        log_lines.push(format!(
            "[Export] svg_to_pptx default done: success={}",
            export.success
        ));

        if !export.success {
            let export_text = join_outputs(&[], &[export.stdout.clone(), export.stderr.clone()]);
            let export_issues = parse_native_export_issues(&export_text);
            if !export_issues.is_empty() {
                log_lines.push(
                    "[Repair] start: export failure targeted native repair max 1 pass".to_string(),
                );
                log_lines.push(format!(
                    "[Native Compat] svg_to_pptx failed with native issue: {}",
                    summarize_native_issues(&export_issues)
                ));
                repair_native_svg_issues_once(
                    db,
                    &design_spec,
                    &spec_lock,
                    &plan,
                    &svg_output,
                    &export_issues,
                    input.model_id,
                    &mut log_lines,
                )
                .await?;
                log_lines.push("[Repair] done: export failure targeted native repair".to_string());
                log_lines.push("[Check] svg_quality_checker start".to_string());
                quality = run_quality_check(&root, &input.python_path, &project, started)?;
                log_lines.push(format!(
                    "[Check] svg_quality_checker done: success={}",
                    quality.success
                ));
                log_lines.push("[Finalize] start".to_string());
                finalize = run_finalize_svg(&root, &input.python_path, &project, started)?;
                log_lines.push(format!("[Finalize] done: success={}", finalize.success));
                log_lines.push("[Export] svg source: svg_output".to_string());
                log_lines.push(
                    "[Export] command: svg_to_pptx.py <project> (native default source=svg_output)"
                        .to_string(),
                );
                export = export_project(&root, &input.python_path, &project, started)?;
                log_lines.push(format!(
                    "[Export] svg_to_pptx default done: success={}",
                    export.success
                ));
            }
        }
        let mut success = export.success;
        let quality_passed = quality.success;
        let export_native_issues = parse_native_export_issues(&join_outputs(
            &[],
            &[export.stdout.clone(), export.stderr.clone()],
        ));
        let detailed_export_error = if export.success || export_native_issues.is_empty() {
            export.error
        } else {
            Some(summarize_native_issues(&export_native_issues))
        };
        let mut error = detailed_export_error.or(split.error).or(finalize.error);
        let mut final_pptx_path = None;

        if export.success && split.success && finalize.success {
            if let (Some(dir), Some(pptx)) =
                (input.output_dir.as_deref(), export.output_path.as_deref())
            {
                match copy_final_pptx(Path::new(pptx), dir, &plan.title) {
                    Ok(path) => {
                        log_lines.push(format!("复制到 outputDir: {}", path.display()));
                        final_pptx_path = Some(path.to_string_lossy().to_string());
                    }
                    Err(e) => {
                        success = false;
                        error = Some(format!("PPTX 已生成，但复制到导出文件夹失败: {}", e));
                    }
                }
            }
        } else {
            success = false;
        }

        if let Some(path) = export.output_path.as_deref() {
            println!("[Done] pptx={}", path);
            log_lines.push(format!("[Done] pptx={}", path));
        }
        let stdout = join_outputs(
            &log_lines,
            &[quality.stdout, split.stdout, finalize.stdout, export.stdout],
        );
        let stderr = join_outputs(
            &[],
            &[quality.stderr, split.stderr, finalize.stderr, export.stderr],
        );
        Ok(PptMasterGenerateResult {
            success,
            project_path: Some(project.to_string_lossy().to_string()),
            pptx_path: export.output_path,
            final_pptx_path,
            slide_plan_path: Some(slide_plan_path.to_string_lossy().to_string()),
            design_spec_path: Some(design_spec_path.to_string_lossy().to_string()),
            quality_check_passed: Some(quality_passed),
            generation_mode: "agent".to_string(),
            exit_code: export.exit_code,
            stdout,
            stderr,
            duration_ms: export.duration_ms,
            error,
            generation_engine: "ppt_master_native".to_string(),
        })
    }
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
    } else {
        (
            "pyramid",
            "swiss-minimal",
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
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    match timeout(
        Duration::from_secs(AI_PPT_TIMEOUT_SECS),
        AiService::plugin_chat_sync(db, input, cancel_rx),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(AppError::Custom(format!(
            "{} 超时：超过 {} 秒，已停止生成，请检查模型/API 或降低页数。",
            context, AI_PPT_TIMEOUT_SECS
        ))),
    }
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
         themeAllocation must be an array of {{pageId, assignedTheme, exclusiveScope}}. Each assignedTheme must be unique; exclusiveScope must say what this page owns and what it does not cover.\n\
         Required slide JSON fields per page: page, pageIndex, pageId, type, layout, title, subtitle, pageTheme, mainClaim, contentScope, mustInclude, mustAvoid, bullets, visualHint, pageRhythm, chartRef, chartType, fileStem, speakerNote.\n\n\
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
    let input = PluginAiChatInput {
        request_id: "ppt_master_agent_design_plan".to_string(),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let raw = ppt_ai_chat_with_timeout(db, input, "AI 生成 slide_plan").await?;
    parse_slide_plan_json(&raw)
}

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
    let input = PluginAiChatInput {
        request_id: "ppt_master_agent_design_plan_dedup".to_string(),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let raw = ppt_ai_chat_with_timeout(db, input, "AI 修正 slide_plan 去重").await?;
    parse_slide_plan_json(&raw)
}

async fn generate_ppt_master_driven_slide_svg(
    db: &Database,
    skill_text: &str,
    resources: &PptMasterResources,
    design_spec: &str,
    spec_lock_path: &Path,
    mapping: &PptMasterStyleMapping,
    plan: &SlidePlan,
    slide: &Slide,
    prev_title: &str,
    next_title: &str,
    model_id: Option<i64>,
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
         - Apply page_rhythm strictly: anchor = large visual center and few words; dense = structured proof/chart/matrix; breathing = whitespace plus one strong claim and minimal support.\n\
         - If page_chart is not none, borrow that ppt-master chart type's information structure.\n\
         - Forbidden SVG: <use>, <symbol>, visual defs + use references, <foreignObject>, <style>, class, filter, mask, clipPath unless explicitly supported, textPath, animation, script, iframe, external href image, rgba(), group opacity, HTML named entities such as &nbsp; &mdash; &copy;.\n\
         - Repeated graphics must be expanded as real rect/path/text/circle/line/polyline/polygon elements. Never use <use href=\"#...\">.\n",
        page = slide.page,
        file_name = svg_filename_for_slide(slide),
        page_rhythm = page_rhythm_for_slide(slide),
        page_chart = chart_reference_for_slide(slide, mapping).unwrap_or_else(|| "none".to_string()),
        mode = mapping.mode,
        visual_style = mapping.visual_style,
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
         Execution rule: render ONLY this page contract. Do not re-plan the deck. Do not pull another page's theme into this page. Use the global storyline only as background.\n\
         Final visible text boundary: you are generating the final user-visible PPT page. Do not render internal field names, prompt words, template labels, developer terminology, agent workflow terminology, or product names.\n\
         Never render these visible terms: Prompt, confirmedPrompt, MVP, Demo, Pomegranate, PPT Master, Executor, Agent, Workflow, fallback, legacy, native, spec_lock, design_spec, slide_plan, pageTheme, contentScope, chartType, chartRef, background pain point, core solution, technical flow, closed-loop validation.\n\
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
    );
    let ai_prompt = format!(
        "你是 ppt-master Executor。请按照 ppt-master 的设计体系逐页手写 SVG，而不是生成简单占位框。\n\
         只输出完整 SVG，不要 markdown，不要解释。\n\n\
         硬性要求：\n\
         1. SVG viewBox 必须是 \"0 0 1280 720\"，width=\"1280\" height=\"720\"。\n\
         2. 必须遵守 spec_lock.md 中的 mode、visual_style、colors、typography、page_rhythm、page_layouts、page_charts、forbidden。\n\
         3. 视觉设计由 ppt-master reference 驱动：读取 locked mode 与 locked visual_style 的语义，不要退化成普通卡片模板。\n\
         4. 如本页有 page_charts，应借鉴对应 charts 模板的信息结构；如本页有 page_layouts，应继承对应 layout 的结构精神。\n\
         5. 不要使用外部网络图片；不要使用 forbidden 中禁止的 SVG 元素。\n\
         6. 每页要有明确视觉层级、留白节奏和页面角色差异。\n\n\
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
        skill_excerpt = skill_excerpt(skill_text),
        executor_excerpt = truncate_for_prompt(&resources.executor_base, 6500),
        standards_excerpt = truncate_for_prompt(&resources.shared_standards, 5000),
        modes_index = truncate_for_prompt(&resources.modes_index, 2600),
        visual_styles_index = truncate_for_prompt(&resources.visual_styles_index, 4200),
        mode = mapping.mode,
        mode_reference = truncate_for_prompt(&mapping.mode_reference, 3600),
        visual_style = mapping.visual_style,
        visual_reference = truncate_for_prompt(&mapping.visual_style_reference, 4200),
        layouts_index = truncate_for_prompt(&resources.layouts_index, 2200),
        charts_index = truncate_for_prompt(&resources.charts_index, 4200),
        locked_page_context = locked_page_context,
        design_spec = format!("{}\n\n{}", current_page_task, design_spec),
        spec_lock = spec_lock,
        deck_title = plan.title,
        total = plan.slides.len(),
        prev_title = prev_title,
        next_title = next_title,
        slide_json = serde_json::to_string_pretty(slide).unwrap_or_default()
    );
    let input = PluginAiChatInput {
        request_id: format!("ppt_master_native_svg_{:02}", slide.page),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let raw =
        ppt_ai_chat_with_timeout(db, input, &format!("AI 生成第 {} 页 SVG", slide.page)).await?;
    extract_svg(&raw)
        .ok_or_else(|| AppError::Custom(format!("第 {} 页 AI 未返回完整 SVG", slide.page)))
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
) -> Result<(), AppError> {
    for slide in &plan.slides {
        let filename = svg_filename_for_slide(slide);
        let path = svg_output.join(&filename);
        if !path.is_file() {
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
        let raw =
            ppt_ai_chat_with_timeout(db, input, &format!("AI 修复 SVG：{}", filename)).await?;
        if let Some(svg) = extract_svg(&raw) {
            write_file(&path, &svg)?;
        }
    }
    Ok(())
}

async fn repair_native_svg_issues_once(
    db: &Database,
    design_spec: &str,
    spec_lock: &str,
    plan: &SlidePlan,
    svg_output: &Path,
    issues: &[NativeSvgIssue],
    model_id: Option<i64>,
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
            log_lines.push(format!(
                "[Native Compat] repair skipped, source SVG not found: svg_output/{}",
                issue.file_name
            ));
            continue;
        }
        let old_svg = fs::read_to_string(&path).unwrap_or_default();
        let slide_json = slide_json_for_svg_file(plan, &issue.file_name);
        let prompt = format!(
            "You are repairing a ppt-master SVG for native DrawingML PPTX export. Output only the complete fixed SVG, no markdown.\n\n\
             Failed file: {file_name}\n\
             Error type: {issue_type}\n\
             Unsupported elements: {unsupported}\n\
             Converter detail: {detail}\n\n\
             Hard requirements:\n\
             - Preserve the visual intent and page content.\n\
             - Keep <svg width=\"1280\" height=\"720\" viewBox=\"0 0 1280 720\">.\n\
             - Do not use <use>, <symbol>, visual defs + use references, <foreignObject>, HTML inside SVG, external href images, <filter>, <mask>, <clipPath>, or unsupported <pattern>.\n\
             - Expand any <use href=\"#...\"> reference into actual rect/path/text/circle/line elements.\n\
             - Do not rely on external assets or browser-only SVG features.\n\
             - Use native-safe elements: svg, g, rect, circle, ellipse, line, polyline, polygon, path, text, tspan only when simple, linearGradient/radialGradient only for paint server definitions.\n\n\
             design_spec.md:\n{design_spec}\n\n\
             spec_lock.md:\n{spec_lock}\n\n\
             Slide JSON:\n{slide_json}\n\n\
             Original SVG:\n{old_svg}",
            file_name = issue.file_name,
            issue_type = issue.issue_type,
            unsupported = issue.unsupported_elements.join(", "),
            detail = issue.detail,
            design_spec = design_spec,
            spec_lock = spec_lock,
            slide_json = slide_json,
            old_svg = old_svg,
        );
        let input = PluginAiChatInput {
            request_id: format!("ppt_master_native_svg_compat_repair_{}", issue.file_name),
            model_id,
            messages: vec![PluginAiMessage {
                role: "user".to_string(),
                content: prompt,
            }],
        };
        let raw = ppt_ai_chat_with_timeout(
            db,
            input,
            &format!("AI 修复 native 兼容 SVG：{}", issue.file_name),
        )
        .await?;
        if let Some(svg) = extract_svg(&raw) {
            write_file(&path, &svg)?;
            log_lines.push(format!(
                "[Native Compat] repaired unsupported SVG elements in svg_output/{}",
                issue.file_name
            ));
            repaired.push(issue.file_name.clone());
        } else {
            log_lines.push(format!(
                "[Native Compat] repair failed, AI did not return SVG for {}",
                issue.file_name
            ));
        }
    }
    Ok(())
}

async fn repair_final_text_leaks_once(
    db: &Database,
    design_spec: &str,
    spec_lock: &str,
    plan: &SlidePlan,
    svg_output: &Path,
    issues: &[FinalTextIssue],
    model_id: Option<i64>,
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
        let raw = ppt_ai_chat_with_timeout(
            db,
            input,
            &format!("AI 修复 final text leakage: {}", issue.file_name),
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
        "legacy",
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

fn extract_material_units(text: &str) -> Vec<String> {
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
        .map(|(idx, unit)| {
            let label = title_from_material_unit(unit, idx);
            ContentBlock {
                label: if label.trim().is_empty() {
                    fallback_label.chars().take(12).collect()
                } else {
                    label
                },
                text: unit.chars().take(52).collect(),
                detail: unit.chars().skip(52).take(90).collect(),
            }
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
) -> String {
    let mut out = String::new();
    let palette = palette_for_style(&mapping.user_style);
    out.push_str(&format!("# {} - Design Spec\n\n", plan.title));
    out.push_str("> Generated by Pomegranate as a ppt-master-compatible planning artifact. Pomegranate owns user interaction and slide_plan; ppt-master owns design resources, SVG constraints, and PPTX export.\n\n");
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
        palette.bg1
    ));
    out.push_str(&format!(
        "| **Secondary bg** | `{}` | Secondary page background |\n",
        palette.bg2
    ));
    out.push_str(&format!(
        "| **Primary** | `{}` | Primary emphasis |\n",
        palette.accent
    ));
    out.push_str(&format!(
        "| **Accent** | `{}` | Data highlights and key information |\n",
        palette.highlight
    ));
    out.push_str(&format!(
        "| **Secondary accent** | `{}` | Secondary emphasis |\n",
        palette.accent2
    ));
    out.push_str(&format!(
        "| **Body text** | `{}` | Main text |\n",
        palette.text
    ));
    out.push_str(&format!(
        "| **Secondary text** | `{}` | Captions and notes |\n",
        palette.muted
    ));
    out.push_str(&format!(
        "| **Border/divider** | `{}` | Lines and separators |\n\n",
        palette.line
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

fn build_ppt_master_spec_lock(plan: &SlidePlan, mapping: &PptMasterStyleMapping) -> String {
    let palette = palette_for_style(&mapping.user_style);
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
    out.push_str("## colors\n");
    out.push_str(&format!("- bg: {}\n", palette.bg1));
    out.push_str(&format!("- secondary_bg: {}\n", palette.bg2));
    out.push_str(&format!("- surface: {}\n", palette.surface));
    out.push_str(&format!("- panel: {}\n", palette.bg2));
    out.push_str(&format!("- primary: {}\n", palette.accent));
    out.push_str(&format!("- accent: {}\n", palette.highlight));
    out.push_str(&format!("- secondary_accent: {}\n", palette.accent2));
    out.push_str(&format!("- text: {}\n", palette.text));
    out.push_str(&format!("- text_secondary: {}\n", palette.muted));
    out.push_str(&format!("- muted: {}\n", palette.muted));
    out.push_str(&format!("- border: {}\n", palette.line));
    out.push_str(&format!("- grid: {}\n\n", palette.line));
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
    if matches!(explicit, "anchor" | "dense" | "breathing") {
        return explicit.to_string();
    }
    match slide.layout.as_str() {
        "cover" | "section" | "summary" => "anchor",
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
        let chart_patterns = fs::read_to_string(&chart_index_path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.as_object().cloned())
            .map(|object| {
                object
                    .keys()
                    .filter(|key| key.as_str() != "_meta")
                    .cloned()
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
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
    Experimental,
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

fn stable_local_reflow_layout(slide: &Slide, layout: StableLayoutKind) -> StableLayoutKind {
    match layout {
        StableLayoutKind::EditorialSplit if slide_blocks(slide).len() >= 3 => {
            StableLayoutKind::CategoryGrid
        }
        StableLayoutKind::EditorialSplit => StableLayoutKind::EvidenceLed,
        StableLayoutKind::CategoryGrid if slide_blocks(slide).len() <= 3 => {
            StableLayoutKind::EditorialSplit
        }
        StableLayoutKind::Timeline => StableLayoutKind::Process,
        StableLayoutKind::Process => StableLayoutKind::Timeline,
        StableLayoutKind::Comparison => StableLayoutKind::EvidenceLed,
        StableLayoutKind::CauseEffect => StableLayoutKind::EvidenceLed,
        StableLayoutKind::Hierarchy => StableLayoutKind::CategoryGrid,
        StableLayoutKind::Matrix => StableLayoutKind::CategoryGrid,
        StableLayoutKind::EvidenceLed => StableLayoutKind::EditorialSplit,
        StableLayoutKind::Quote => StableLayoutKind::EditorialSplit,
        StableLayoutKind::Summary => StableLayoutKind::EvidenceLed,
        StableLayoutKind::Anchor => StableLayoutKind::Anchor,
        _ => stable_alternate_layout(slide, layout),
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
            layout: stable_local_reflow_layout(slide, primary_layout),
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
            let layout = stable_fallback_layout(slide, primary_layout);
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
    let body_pages = plan.slides.len().saturating_sub(2).max(1);
    let soft_motif_limit = body_pages.div_ceil(2).max(1);

    for (index, slide) in plan.slides.iter().enumerate() {
        let layout = stable_layout_for_index(plan, index, chart_patterns);
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
        let previous = selections
            .last()
            .map(|selection: &StableVisualSelection| selection.signature);
        let mut best = None;
        let mut best_score = usize::MAX;
        for (motif, content_score) in eligible {
            let signature = visual_signature(layout, motif);
            let reuse = *motif_counts.get(&motif).unwrap_or(&0);
            let repeated_motif = previous.is_some_and(|value| value.motif_family == motif);
            let repeated_signature = previous == Some(signature);
            let over_soft_limit =
                index > 0 && index + 1 < plan.slides.len() && reuse >= soft_motif_limit;
            let score = content_score
                + reuse * 12
                + usize::from(repeated_motif) * 120
                + usize::from(repeated_signature) * 240
                + usize::from(over_soft_limit) * 60;
            if score < best_score {
                best = Some(signature);
                best_score = score;
            }
        }
        let signature =
            best.unwrap_or_else(|| visual_signature(layout, StableMotif::PlainEditorial));
        let duplicate_signature = previous == Some(signature);
        let count = motif_counts.entry(signature.motif_family).or_insert(0);
        *count += 1;
        selections.push(StableVisualSelection {
            signature,
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
        StableMotif::HubSpoke | StableMotif::BracketGroup | StableMotif::MatrixCell => {
            StableMotifStatus::Experimental
        }
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
            max_blocks: 6,
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
    match slide.density.trim().to_ascii_lowercase().as_str() {
        "anchor" => StableDensity::Anchor,
        "breathing" => StableDensity::Breathing,
        "dense" => StableDensity::Dense,
        _ => StableDensity::Balanced,
    }
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
        StableMotifStatus::Experimental => Some("status=experimental".to_string()),
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
            StableMotif::EvidenceStrip,
            StableMotif::HubSpoke,
            StableMotif::SectionBanner,
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

fn stable_layout_for_index(
    plan: &SlidePlan,
    index: usize,
    chart_patterns: &std::collections::HashSet<String>,
) -> StableLayoutKind {
    let mut previous = None;
    let mut selected = StableLayoutKind::CategoryGrid;
    for idx in 0..=index {
        let slide = &plan.slides[idx];
        let mut current =
            stable_semantic_layout(slide, idx, plan.slides.len(), &plan.title, chart_patterns);
        if previous == Some(current)
            && !matches!(
                current,
                StableLayoutKind::Anchor | StableLayoutKind::Summary
            )
        {
            current = stable_alternate_layout(slide, current);
        }
        selected = current;
        previous = Some(current);
    }
    selected
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
    let available = |pattern: &str| chart_patterns.is_empty() || chart_patterns.contains(pattern);
    if (relation == "timeline" || chart == "timeline") && available("timeline") {
        StableLayoutKind::Timeline
    } else if relation == "compare" || chart == "compare" || layout == "compare" {
        StableLayoutKind::Comparison
    } else if (relation == "cause" || chart.contains("cause")) && available("fishbone_diagram") {
        StableLayoutKind::CauseEffect
    } else if (relation == "process" || chart.contains("process") || layout == "process")
        && available("process_flow")
    {
        StableLayoutKind::Process
    } else if (chart.contains("matrix") || layout == "matrix") && available("matrix_2x2") {
        StableLayoutKind::Matrix
    } else if (chart.contains("hierarchy") || chart.contains("pyramid"))
        && available("pyramid_chart")
    {
        StableLayoutKind::Hierarchy
    } else if chart == "highlight" || layout == "highlight" {
        StableLayoutKind::EvidenceLed
    } else if (relation == "category" || chart == "cards" || layout == "cards")
        && available("labeled_card")
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

fn stable_alternate_layout(slide: &Slide, current: StableLayoutKind) -> StableLayoutKind {
    match current {
        StableLayoutKind::CategoryGrid => StableLayoutKind::EditorialSplit,
        StableLayoutKind::Timeline => StableLayoutKind::Process,
        StableLayoutKind::Process => StableLayoutKind::CauseEffect,
        StableLayoutKind::Comparison => StableLayoutKind::EvidenceLed,
        StableLayoutKind::EvidenceLed => StableLayoutKind::EditorialSplit,
        _ if slide_blocks(slide).len() <= 3 => StableLayoutKind::EditorialSplit,
        _ => StableLayoutKind::EvidenceLed,
    }
}

fn stable_fallback_layout(slide: &Slide, current: StableLayoutKind) -> StableLayoutKind {
    if slide_blocks(slide).len() >= 4 && current != StableLayoutKind::CategoryGrid {
        StableLayoutKind::CategoryGrid
    } else if slide_blocks(slide).len() <= 3 && current != StableLayoutKind::EditorialSplit {
        StableLayoutKind::EditorialSplit
    } else if current != StableLayoutKind::EvidenceLed {
        StableLayoutKind::EvidenceLed
    } else {
        StableLayoutKind::CategoryGrid
    }
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

fn fit_text_box(
    text: &str,
    box_width: f32,
    box_height: f32,
    preferred_font_size: f32,
    min_font_size: f32,
    line_height_ratio: f32,
    weight: &str,
) -> StableTextFit {
    let mut size = preferred_font_size;
    while size >= min_font_size {
        let lines = wrap_text_to_width(text, size, box_width, weight);
        let line_height = (size * line_height_ratio).ceil();
        let used_height = lines.len() as f32 * line_height;
        if used_height <= box_height + 0.5 {
            let max_line_width = lines
                .iter()
                .map(|line| estimate_stable_text_width(line, size, weight))
                .fold(0.0, f32::max);
            return StableTextFit {
                lines,
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
    let line_height = (size * line_height_ratio).ceil();
    let mut lines = wrap_text_to_width(text, size, box_width, weight);
    let required_height = lines.len() as f32 * line_height;
    let required_width = lines
        .iter()
        .map(|line| estimate_stable_text_width(line, size, weight))
        .fold(0.0, f32::max);
    let max_lines = ((box_height / line_height).floor() as usize).max(1);
    let overflowed = lines.len() > max_lines;
    lines.truncate(max_lines);
    if overflowed {
        if let Some(last) = lines.last_mut() {
            while !last.is_empty()
                && estimate_stable_text_width(&format!("{}…", last), size, weight) > box_width
            {
                last.pop();
            }
            if !last.ends_with('…') {
                last.push('…');
            }
        }
    }
    let max_line_width = lines
        .iter()
        .map(|line| estimate_stable_text_width(line, size, weight))
        .fold(0.0, f32::max);
    StableTextFit {
        used_height: lines.len() as f32 * line_height,
        lines,
        font_size: size,
        line_height,
        max_line_width,
        required_width,
        required_height,
        overflowed,
    }
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
    let fit = fit_text_box(
        text,
        rect.width,
        rect.height,
        preferred_font_size,
        min_font_size,
        line_height_ratio,
        weight,
    );
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
    for (idx, line) in fit.lines.iter().enumerate() {
        tspans.push_str(&format!(
            "<tspan x=\"{:.1}\" dy=\"{}\">{}</tspan>",
            x,
            if idx == 0 { 0.0 } else { fit.line_height },
            xml_escape(line)
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
    let title = shorten_to_width(&plan.title, 12.0, 264.0, "400");
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
        if let Some(evidence) = slide.evidence.first() {
            draft.push_degradation(
                "footer",
                "footer annotation",
                "annotation",
                "omitted_to_speaker_notes",
                evidence,
            );
        }
    } else if let Some(evidence) = slide.evidence.first() {
        let cleaned = evidence
            .trim()
            .trim_start_matches("材料依据")
            .trim_start_matches(['：', ':', '·', ' ']);
        let note = shorten_to_width(cleaned, 11.0, 590.0, "400");
        append_fitted_text(
            draft,
            "footer-evidence",
            &format!("依据 · {}", note),
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
        let tag_text = shorten_to_width(evidence, 11.0, layout.evidence.width, "400");
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
    let count = blocks.len().clamp(2, 5);
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
    let count = blocks.len().clamp(2, 5);
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
            y: left.y + 112.0,
            width: left.width - 68.0,
            height: 250.0,
        },
        30.0,
        22.0,
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
    let blocks = slide_blocks(slide);
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
    render_stable_category_grid(slide, profile, motif, detail_level)
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
    for (idx, block) in blocks.iter().take(count).enumerate() {
        let inset = idx as f32 * 42.0;
        let rect = StableRect {
            x: 120.0 + inset,
            y: 188.0 + idx as f32 * (height + gap),
            width: 1040.0 - inset * 2.0,
            height,
        };
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
    let core_rect = StableRect {
        x: 56.0,
        y: 188.0,
        width: 1168.0,
        height: 122.0,
    };
    match motif {
        StableMotif::SectionBanner => draft.body.push_str(&format!(
            "<rect x=\"56\" y=\"188\" width=\"1168\" height=\"122\" fill=\"{}\"/><rect x=\"56\" y=\"298\" width=\"1168\" height=\"12\" fill=\"{}\"/>\n",
            tokens.panel, tokens.primary
        )),
        StableMotif::HubSpoke => draft.body.push_str(&format!(
            "<rect x=\"292\" y=\"188\" width=\"696\" height=\"122\" rx=\"61\" fill=\"{}\"/>\n",
            tokens.panel
        )),
        _ => draft.body.push_str(&format!(
            "<rect x=\"56\" y=\"188\" width=\"1168\" height=\"122\" rx=\"{:.1}\" fill=\"{}\"/><rect x=\"56\" y=\"290\" width=\"1168\" height=\"20\" fill=\"{}\"/>\n",
            tokens.corner_radius, tokens.surface, tokens.panel
        )),
    }
    draft.push_rect("summary-core", core_rect, StableElementKind::Card);
    let core_fit = append_fitted_text(
        &mut draft,
        "summary-message",
        &stable_core_message(slide),
        StableRect {
            x: 90.0,
            y: 204.0,
            width: 1090.0,
            height: 76.0,
        },
        27.0,
        21.0,
        1.34,
        &tokens.text,
        "700",
        "start",
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
    let count = blocks.len().clamp(2, 4);
    let columns = if count <= 2 { count } else { 2 };
    let rows = (count + columns - 1) / columns;
    let gap_x = 22.0;
    let gap_y = 18.0;
    let width = (1168.0 - gap_x * (columns.saturating_sub(1) as f32)) / columns as f32;
    let height = (318.0 - gap_y * (rows.saturating_sub(1) as f32)) / rows as f32;
    for (idx, block) in blocks.iter().take(count).enumerate() {
        let row = idx / columns;
        let col = idx % columns;
        render_stable_motif_block(
            &mut draft,
            block,
            slide.evidence.get(idx).map(String::as_str),
            StableRect {
                x: 56.0 + col as f32 * (width + gap_x),
                y: 328.0 + row as f32 * (height + gap_y),
                width,
                height,
            },
            idx + 1,
            tokens,
            motif,
            if rows > 1 {
                StableDetailLevel::Reduced
            } else {
                detail_level
            },
            profile.local_repair.as_ref(),
            "summary-point",
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

    #[test]
    fn text_width_distinguishes_cjk_and_latin() {
        let cjk = estimate_stable_text_width("中国历史", 20.0, "400");
        let latin = estimate_stable_text_width("History", 20.0, "400");
        assert!(cjk > latin);
        assert!(cjk >= 78.0);
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
        plan.slides[0].evidence = vec!["A very long source explanation that must be shortened independently and remain centered inside its own footer column".repeat(3)];
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
        let plan: SlidePlan = serde_json::from_str(&plan_text).expect("parse stable slide plan");
        let profile = StableRenderProfile::from_plan(&plan);
        let svg_output = project.join("svg_output");
        fs::create_dir_all(&svg_output).expect("create svg_output");
        for slide in &plan.slides {
            let rendered =
                render_slide_svg_with_profile(&plan, slide, &profile).expect("render stable slide");
            fs::write(
                svg_output.join(svg_filename_for_slide(slide)),
                rendered.svg.as_bytes(),
            )
            .expect("write stable SVG");
        }
        fs::write(
            project.join("design_spec.md"),
            build_stable_design_spec(&plan),
        )
        .expect("write stable design spec");
    }

    #[test]
    #[ignore = "authorized real-AI strict native quality A/B harness"]
    fn strict_native_quality_ab_when_authorized() {
        if std::env::var("POME_STRICT_NATIVE_AB").ok().as_deref() != Some("1") {
            return;
        }

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("resolve firstwork root")
            .to_path_buf();
        let root = std::env::var("POME_STRICT_AB_PPT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("ppt-master"));
        let python = std::env::var("POME_STRICT_AB_PYTHON").unwrap_or_else(|_| {
            root.join(".venv/Scripts/python.exe")
                .to_string_lossy()
                .to_string()
        });
        let db_path = std::env::var("POME_STRICT_AB_DB").unwrap_or_else(|_| {
            repo_root
                .join(".runtime-data-final/dev-app.db")
                .to_string_lossy()
                .to_string()
        });
        let artifact_root = std::env::var("POME_STRICT_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join(".runtime-data-final/ppt-native-quality-strict-ab"));
        let reuse_base = std::env::var("POME_STRICT_AB_REUSE_BASE").ok().as_deref() == Some("1");
        if !reuse_base {
            let _ = fs::remove_dir_all(&artifact_root);
        }
        fs::create_dir_all(&artifact_root).expect("create strict A/B artifact directory");

        let material = strict_ab_material();
        let char_count = material.chars().count();
        assert!(
            (1500..=2500).contains(&char_count),
            "material must be 1500-2500 chars, got {char_count}"
        );
        let title = "增材制造质量控制与工艺优化";
        let style = "科技蓝";
        let audience = "机械制造课程学习者";
        let slide_count = 5usize;
        let input_hash = sha256_hex(material.as_bytes());
        let input_snapshot = serde_json::json!({
            "title": title,
            "style": style,
            "audience": audience,
            "slideCount": slide_count,
            "materialChars": char_count,
            "inputHash": input_hash,
            "material": material,
            "note": "One real AI request is authorized only for base_slide_plan generation. A/B rendering is offline."
        });
        write_json(&artifact_root.join("input_snapshot.json"), &input_snapshot)
            .expect("write input snapshot");

        let planning_context = format!(
            "[Raw Material]\n{}\n\n[Audience]\n{}\n\n[Requirement]\n生成 5 页教学型 PPT，强调事实覆盖、流程关系、质量控制和工程应用，语言简洁。",
            material, audience
        );
        let started = Instant::now();
        let base_plan = if reuse_base {
            let raw = fs::read_to_string(artifact_root.join("base_slide_plan.json"))
                .expect("read reusable base_slide_plan.json");
            serde_json::from_str(&raw).expect("parse reusable base_slide_plan.json")
        } else {
            let db = Database::init(&db_path).expect("open configured runtime db");
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("create tokio runtime");
            runtime
                .block_on(generate_slide_plan_with_ai(
                    &db,
                    &planning_context,
                    None,
                    title,
                    slide_count,
                    style,
                ))
                .expect("single authorized AI slide_plan request must succeed")
        };
        let mut base_plan = normalize_slide_plan(base_plan, title, slide_count, style);
        if let Some(report) = validate_stable_content_plan(&base_plan) {
            enrich_plan_from_material(&mut base_plan, &planning_context);
            if let Some(after) = validate_stable_content_plan(&base_plan) {
                panic!(
                    "base slide_plan failed content validation without using repair AI: before={report}; after={after}"
                );
            }
        }
        let base_json = serde_json::to_value(&base_plan).expect("serialize base plan");
        let base_plan_hash = sha256_hex(
            serde_json::to_string(&base_json)
                .expect("stringify base plan")
                .as_bytes(),
        );
        write_json(&artifact_root.join("base_slide_plan.json"), &base_json)
            .expect("write base plan");

        let a_input = strict_ab_input(&root, &python, title, audience, style, slide_count, false);
        let b_input = strict_ab_input(&root, &python, title, audience, style, slide_count, true);
        let a = render_strict_ab_group(
            &root,
            &python,
            &artifact_root,
            "A",
            base_plan.clone(),
            &a_input,
            "",
        )
        .expect("render A offline");
        let b = render_strict_ab_group(
            &root,
            &python,
            &artifact_root,
            "B",
            base_plan.clone(),
            &b_input,
            &planning_context,
        )
        .expect("render B offline");
        assert!(
            !a.project
                .join(native_quality::NATIVE_QUALITY_REPORT_FILE)
                .exists(),
            "A must not generate native_quality_plan.json"
        );
        assert!(
            b.project
                .join(native_quality::NATIVE_QUALITY_REPORT_FILE)
                .exists(),
            "B must generate native_quality_plan.json"
        );

        let diff = strict_ab_plan_diff(&a.plan, &b.plan);
        let summary = serde_json::json!({
            "materialChars": char_count,
            "inputHash": input_hash,
            "baseSlidePlanHash": base_plan_hash,
            "aiRequests": {
                "thisHarnessRunBaseSlidePlan": if reuse_base { 0 } else { 1 },
                "aRender": 0,
                "bRender": 0,
                "thisHarnessRunTotal": if reuse_base { 0 } else { 1 },
                "authorizedExperimentTotal": 1,
                "basePlanReused": reuse_base
            },
            "elapsedMs": started.elapsed().as_millis(),
            "fixedInputs": {
                "title": title,
                "style": style,
                "audience": audience,
                "slideCount": slide_count,
                "pptMasterMode": "template/stable-offline",
                "model": "current default provider from runtime DB"
            },
            "a": a.summary(),
            "b": b.summary(),
            "fieldDiff": diff,
            "leakScan": {
                "a": scan_plan_for_internal_fields(&a.plan),
                "b": scan_plan_for_internal_fields(&b.plan)
            }
        });
        write_json(&artifact_root.join("ab_summary.json"), &summary).expect("write summary");
    }

    struct StrictAbRenderResult {
        label: String,
        project: PathBuf,
        pptx_path: Option<String>,
        plan: SlidePlan,
        quality_success: bool,
        quality_stdout: String,
        export_success: bool,
        export_stdout: String,
        render_warnings: Vec<String>,
    }

    impl StrictAbRenderResult {
        fn summary(&self) -> serde_json::Value {
            serde_json::json!({
                "label": self.label,
                "project": self.project.to_string_lossy(),
                "pptxPath": self.pptx_path,
                "slidePlanPath": self.project.join("slide_plan.json").to_string_lossy(),
                "nativeQualityPlanPath": self.project.join(native_quality::NATIVE_QUALITY_REPORT_FILE).to_string_lossy(),
                "nativeQualityPlanExists": self.project.join(native_quality::NATIVE_QUALITY_REPORT_FILE).exists(),
                "svgOutput": self.project.join("svg_output").to_string_lossy(),
                "previewDir": self.project.join("render_preview").to_string_lossy(),
                "qualitySuccess": self.quality_success,
                "qualityStdout": self.quality_stdout,
                "exportSuccess": self.export_success,
                "exportStdout": self.export_stdout,
                "renderWarnings": self.render_warnings,
                "slideCount": self.plan.slides.len(),
                "densities": self.plan.slides.iter().map(|slide| slide.density.clone()).collect::<Vec<_>>(),
                "theme": self.plan.theme,
                "slideResponsibilities": self.plan.slides.iter().map(|slide| serde_json::json!({
                    "page": slide.page,
                    "title": slide.title,
                    "pageTheme": slide.page_theme,
                    "coreMessage": stable_core_message(slide),
                    "density": slide.density,
                    "chartType": slide.chart_type,
                    "relation": slide.relation,
                    "contentBlockCount": slide.content_blocks.len(),
                    "evidenceCount": slide.evidence.len()
                })).collect::<Vec<_>>()
            })
        }
    }

    fn render_strict_ab_group(
        root: &Path,
        python: &str,
        artifact_root: &Path,
        label: &str,
        mut plan: SlidePlan,
        input: &PptMasterGenerateInput,
        planning_context: &str,
    ) -> Result<StrictAbRenderResult, AppError> {
        let started = Instant::now();
        let mut render_warnings = Vec::new();
        let native_report = if native_quality::is_enabled(input) {
            let outcome = native_quality::apply_native_quality_chain(plan, input, planning_context);
            plan = outcome.plan;
            render_warnings.extend(outcome.log_lines);
            Some(outcome.report)
        } else {
            None
        };

        let project = create_project_dir(root)?;
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
            "render_preview",
        ] {
            create_dir_all(&project.join(dir_name))?;
        }
        write_file(
            &project.join("sources/confirmed_prompt.md"),
            "strict native quality A/B offline render",
        )?;
        write_file(
            &project.join("design_spec.md"),
            &build_stable_design_spec(&plan),
        )?;
        write_file(&project.join("notes/total.md"), &build_notes(&plan))?;
        if let Some(report) = native_report {
            let report_json = serde_json::to_string_pretty(&report)
                .map_err(|e| AppError::Custom(format!("serialize native report failed: {e}")))?;
            write_file(
                &project.join(native_quality::NATIVE_QUALITY_REPORT_FILE),
                &format!("{report_json}\n"),
            )?;
            write_file(
                &artifact_root.join("B_native_quality_plan.json"),
                &format!("{report_json}\n"),
            )?;
        }

        let plan_json = serde_json::to_string_pretty(&plan)
            .map_err(|e| AppError::Custom(format!("serialize {label} plan failed: {e}")))?;
        write_file(&project.join("slide_plan.json"), &format!("{plan_json}\n"))?;
        write_file(
            &artifact_root.join(format!("{label}_final_slide_plan.json")),
            &format!("{plan_json}\n"),
        )?;

        let profile = StableRenderProfile::load(root, &plan);
        for slide in &plan.slides {
            let rendered = render_slide_svg_with_profile(&plan, slide, &profile)?;
            render_warnings.extend(rendered.motif_gate_rejections);
            render_warnings.extend(rendered.local_repair_logs);
            render_warnings.extend(rendered.warnings);
            write_file(
                &project
                    .join("svg_output")
                    .join(svg_filename_for_slide(slide)),
                &rendered.svg,
            )?;
        }

        let quality = run_quality_check(root, python, &project, started)?;
        let export = export_project(root, python, &project, started)?;
        if let Some(pptx) = export.output_path.as_deref() {
            let target = artifact_root.join(format!("{label}_output.pptx"));
            fs::copy(pptx, &target).map_err(|e| {
                AppError::Custom(format!(
                    "copy {label} PPTX failed: {pptx} -> {} ({e})",
                    target.display()
                ))
            })?;
        }
        if std::env::var("POME_STRICT_AB_SKIP_CAIRO_PREVIEW")
            .ok()
            .as_deref()
            != Some("1")
        {
            let _ = render_svg_previews_with_cairosvg(root, python, &project, label);
            copy_svg_previews(&project, artifact_root, label)?;
        }

        Ok(StrictAbRenderResult {
            label: label.to_string(),
            project,
            pptx_path: export.output_path,
            plan,
            quality_success: quality.success,
            quality_stdout: quality.stdout,
            export_success: export.success,
            export_stdout: export.stdout,
            render_warnings,
        })
    }

    fn strict_ab_input(
        root: &Path,
        python: &str,
        title: &str,
        audience: &str,
        style: &str,
        slide_count: usize,
        native_quality_enabled: bool,
    ) -> PptMasterGenerateInput {
        serde_json::from_value(serde_json::json!({
            "pptMasterRoot": root.to_string_lossy(),
            "pythonPath": python,
            "prompt": "strict native quality A/B offline render",
            "title": title,
            "audience": audience,
            "style": style,
            "slideCount": slide_count,
            "nativeQualityEnabled": native_quality_enabled
        }))
        .expect("deserialize strict A/B input")
    }

    fn strict_ab_material() -> &'static str {
        "增材制造是一类以数字模型为基础、通过材料逐层堆积形成零件的制造方法，常见工艺包括选择性激光熔化、激光定向能量沉积、熔融沉积成形和光固化成形。它的优势不是简单替代切削加工，而是在复杂内腔、轻量化结构、小批量定制和快速迭代场景中提供新的工艺路线。以金属粉末床熔化为例，工艺链通常从三维建模开始，经过拓扑优化、支撑设计、切片、铺粉、激光扫描、冷却、去支撑、热处理和表面精整，最终还要进行尺寸检测和性能验证。每个环节都会影响零件质量，因此质量控制必须贯穿设计、制造和后处理全过程。\n\n在设计阶段，工程师需要判断零件是否适合增材制造。壁厚过薄会导致成形不稳定，悬垂角过大会增加支撑和后处理成本，封闭空腔可能造成未熔粉末难以清理。为了提高成功率，设计时会采用圆角过渡、减小孤立尖角、设置排粉孔，并让载荷路径与材料沉积方向相匹配。对于承力构件，还要避免把关键受力面直接放在粗糙支撑接触区。设计规则并不是限制创新，而是把工艺约束提前纳入结构方案，减少后期试错。\n\n在成形阶段，激光功率、扫描速度、层厚、道间距和预热温度共同决定能量密度。能量不足容易形成未熔合孔隙，能量过高可能产生匙孔孔隙、飞溅和组织粗化。扫描策略也很重要，连续长扫描会积累热量并引起翘曲，分区扫描和层间旋转可以降低残余应力。粉末状态同样不可忽视，粒度分布、含氧量、流动性和循环使用次数会改变铺粉均匀性。实际生产中通常要把参数窗口、粉末批次和设备状态一起记录，形成可追溯的过程数据。\n\n过程稳定性不仅取决于设备参数，还取决于生产组织。设备需要定期标定光斑直径、铺粉机构、保护气流和平台平面度；粉末需要建立入库、干燥、筛分、混粉和回收制度；操作人员需要按照同一作业指导书记录基板预处理、仓内氧含量、扫描程序版本和异常停机情况。对于批量生产，单件合格并不代表工艺稳定，企业更关注同批次、跨批次和跨设备的一致性。统计过程控制可以把尺寸偏差、孔隙率、表面粗糙度和力学性能转化为趋势图，帮助工程师提前发现漂移。\n\n缺陷检测是质量闭环的关键。制造前可用仿真预测热变形和支撑风险，制造中可通过熔池监测、铺粉图像、温度场记录和声音信号发现异常，制造后则使用三坐标测量、工业 CT、金相分析、硬度测试和拉伸试验验证结果。不同检测方法关注的问题不同：CT 适合识别内部孔隙和未熔合缺陷，金相适合观察组织和熔池边界，三坐标更适合尺寸与形位误差。只有把过程监测与最终检测关联起来，才能判断缺陷来自设计、粉末、参数还是后处理。\n\n后处理会显著改变最终性能。去支撑和喷砂可以改善表面状态，热处理能够释放残余应力并调整组织，热等静压可降低内部孔隙，机械加工则用于保证装配面和密封面的精度。后处理不是附加步骤，而是增材制造工艺链的一部分。对于航空、医疗和模具等高可靠场景，企业往往会建立从原材料入厂、设备校准、参数锁定、过程记录、检测报告到批次放行的质量体系。\n\n工程案例中，某些复杂冷却通道模具可以通过增材制造实现更均匀的温度控制，但如果通道内表面粗糙、残粉清理不足或热处理制度不稳定，模具寿命仍会受到影响。又如轻量化支架可以通过晶格结构减重，但晶格杆径、节点过渡和构建方向必须与载荷路径匹配，否则局部应力集中会抵消轻量化收益。这说明质量控制不是单个检测动作，而是从方案选择到服役评价的系统工程。\n\n从课程教学角度看，增材制造质量控制适合用“设计约束、工艺参数、过程监测、后处理、验证放行”五个层次来组织。学生不应只记住某个设备名称，而要理解设计选择如何影响支撑和残余应力，参数窗口如何影响孔隙和组织，检测方法如何服务于缺陷定位，质量体系如何把一次成功转化为可重复制造。教学中理解这条链路，有助于把增材制造从“能打印复杂形状”提升到“能稳定制造合格零件”的工程认识。"
    }

    fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), AppError> {
        let json = serde_json::to_string_pretty(value)
            .map_err(|e| AppError::Custom(format!("serialize json failed: {e}")))?;
        write_file(path, &format!("{json}\n"))
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    fn strict_ab_plan_diff(a: &SlidePlan, b: &SlidePlan) -> serde_json::Value {
        let slides = a
            .slides
            .iter()
            .zip(b.slides.iter())
            .map(|(left, right)| {
                serde_json::json!({
                    "page": left.page,
                    "titleChanged": left.title != right.title,
                    "pageTheme": {"a": left.page_theme, "b": right.page_theme, "changed": left.page_theme != right.page_theme},
                    "coreMessage": {"a": stable_core_message(left), "b": stable_core_message(right), "changed": stable_core_message(left) != stable_core_message(right)},
                    "density": {"a": left.density, "b": right.density, "changed": left.density != right.density},
                    "chartType": {"a": left.chart_type, "b": right.chart_type, "changed": left.chart_type != right.chart_type},
                    "relation": {"a": left.relation, "b": right.relation, "changed": left.relation != right.relation},
                    "contentChanged": serde_json::to_value(&left.content_blocks).ok() != serde_json::to_value(&right.content_blocks).ok()
                        || left.bullets != right.bullets
                        || left.evidence != right.evidence,
                    "contentBlockCount": {"a": left.content_blocks.len(), "b": right.content_blocks.len()},
                    "evidenceCount": {"a": left.evidence.len(), "b": right.evidence.len()}
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "pageCount": {"a": a.slides.len(), "b": b.slides.len(), "changed": a.slides.len() != b.slides.len()},
            "theme": {"a": a.theme, "b": b.theme, "changed": serde_json::to_value(&a.theme).ok() != serde_json::to_value(&b.theme).ok()},
            "slides": slides
        })
    }

    fn scan_plan_for_internal_fields(plan: &SlidePlan) -> Vec<String> {
        let text = serde_json::to_string(plan)
            .unwrap_or_default()
            .to_ascii_lowercase();
        [
            "requestkind",
            "request_kind",
            "runid",
            "run_id",
            "cachekey",
            "cache_key",
            "system_prompt",
            "internalprompt",
            "internal_prompt",
            "planning_context",
            "native_quality",
            "role\":\"system",
        ]
        .iter()
        .filter(|needle| text.contains(**needle))
        .map(|needle| (*needle).to_string())
        .collect()
    }

    fn render_svg_previews_with_cairosvg(
        root: &Path,
        python: &str,
        project: &Path,
        label: &str,
    ) -> Result<(), AppError> {
        let script = r#"
import sys
from pathlib import Path
project = Path(sys.argv[1])
out = project / 'render_preview'
out.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, str(Path(sys.argv[2]) / 'skills' / 'ppt-master' / 'scripts'))
from svg_to_pptx.pptx_media import convert_svg_to_png
ok = True
for svg in sorted((project / 'svg_output').glob('*.svg')):
    png = out / (svg.stem + '.png')
    if not convert_svg_to_png(svg, png, 1600, 900):
        print(f'preview failed: {svg.name}', file=sys.stderr)
        ok = False
if not ok:
    sys.exit(3)
"#;
        let mut cmd = Command::new(resolve_python_program(root, python));
        cmd.current_dir(root)
            .arg("-c")
            .arg(script)
            .arg(project)
            .arg(root);
        add_no_window(&mut cmd);
        let output = cmd.output().map_err(|e| {
            AppError::Custom(format!("launch preview renderer failed for {label}: {e}"))
        })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(AppError::Custom(format!(
                "preview renderer failed for {label}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    fn copy_svg_previews(
        project: &Path,
        artifact_root: &Path,
        label: &str,
    ) -> Result<(), AppError> {
        let source = project.join("render_preview");
        let target = artifact_root.join(format!("{label}_render_preview"));
        create_dir_all(&target)?;
        if !source.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(source).map_err(|e| AppError::Custom(e.to_string()))? {
            let entry = entry.map_err(|e| AppError::Custom(e.to_string()))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("png") {
                fs::copy(&path, target.join(path.file_name().unwrap())).map_err(|e| {
                    AppError::Custom(format!("copy preview failed: {} ({e})", path.display()))
                })?;
            }
        }
        Ok(())
    }
}
