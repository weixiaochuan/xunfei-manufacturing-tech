use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{PluginAiChatInput, PluginAiMessage};
use crate::services::ai::AiService;

const SVG_TO_PPTX_SCRIPT: &str = "skills/ppt-master/scripts/svg_to_pptx.py";
const SVG_QUALITY_CHECKER_SCRIPT: &str = "skills/ppt-master/scripts/svg_quality_checker.py";
const PPT_MASTER_SKILL_MD: &str = "skills/ppt-master/SKILL.md";

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PptMasterGenerateInput {
    pub ppt_master_root: String,
    pub python_path: String,
    pub prompt: String,
    pub model_id: Option<i64>,
    pub title: Option<String>,
    pub slide_count: Option<usize>,
    pub style: Option<String>,
    pub output_dir: Option<String>,
    pub generation_mode: Option<String>,
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
    slides: Vec<Slide>,
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
    #[serde(rename = "type")]
    slide_type: String,
    #[serde(default)]
    layout: String,
    title: String,
    subtitle: String,
    bullets: Vec<String>,
    #[serde(default)]
    visual_hint: String,
    speaker_note: String,
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
        let mode = input
            .generation_mode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("agent");
        if mode == "template" {
            return Self::generate_from_prompt_template(db, input).await;
        }
        Self::generate_from_prompt_agent_mode(db, input).await
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
        if prompt.is_empty() {
            return Err(AppError::InvalidInput("确认 Prompt 不能为空".into()));
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

        let plan = match generate_slide_plan_with_ai(db, prompt, input.model_id, &title, requested_count, &style).await {
            Ok(plan) => normalize_slide_plan(plan, &title, requested_count, &style),
            Err(e) => {
                log::warn!("AI slide_plan 生成失败，使用默认方案: {}", e);
                default_slide_plan(&title, requested_count, &style, prompt)
            }
        };

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
        write_file(&project.join("design_spec.md"), &build_design_spec(&plan))?;
        let plan_json = serde_json::to_string_pretty(&plan)
            .map_err(|e| AppError::Custom(format!("序列化 slide_plan 失败: {}", e)))?;
        let slide_plan_path = project.join("slide_plan.json");
        write_file(&slide_plan_path, &plan_json)?;
        write_file(&notes.join("total.md"), &build_notes(&plan))?;

        for slide in &plan.slides {
            let filename = format!("{:02}_{}.svg", slide.page, safe_filename(&slide.title, "slide"));
            write_file(&svg_output.join(filename), &render_slide_svg(&plan, slide))?;
        }

        let output_dir = input.output_dir.clone();
        let export = export_project(&root, &input.python_path, &project, started)?;
        let mut success = export.success;
        let mut error = export.error;
        let mut final_pptx_path = None;

        if export.success {
            if let (Some(dir), Some(pptx)) = (output_dir.as_deref(), export.output_path.as_deref()) {
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
            stdout: export.stdout,
            stderr: export.stderr,
            duration_ms: export.duration_ms,
            error,
        })
    }

    async fn generate_from_prompt_agent_mode(
        db: &Database,
        input: PptMasterGenerateInput,
    ) -> Result<PptMasterGenerateResult, AppError> {
        let started = Instant::now();
        let root = parse_dir("ppt-master 根目录", &input.ppt_master_root)?;
        ensure_python_available(&root, &input.python_path)?;

        let export_script = root.join(SVG_TO_PPTX_SCRIPT);
        if !export_script.is_file() {
            return Err(AppError::NotFound(format!(
                "找不到 svg_to_pptx.py 脚本: {}",
                export_script.display()
            )));
        }
        let checker_script = root.join(SVG_QUALITY_CHECKER_SCRIPT);
        if !checker_script.is_file() {
            return Err(AppError::NotFound(format!(
                "找不到 svg_quality_checker.py 脚本: {}",
                checker_script.display()
            )));
        }

        let skill_text = read_ppt_master_skill(&root)?;
        let prompt = input.prompt.trim();
        if prompt.is_empty() {
            return Err(AppError::InvalidInput("确认 Prompt 不能为空".into()));
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

        let project = create_project_dir(&root)?;
        let sources = project.join("sources");
        let notes = project.join("notes");
        let svg_output = project.join("svg_output");
        create_dir_all(&sources)?;
        create_dir_all(&notes)?;
        create_dir_all(&svg_output)?;

        let mut log_lines = Vec::new();
        log_lines.push(format!("创建项目目录: {}", project.display()));
        write_file(&sources.join("confirmed_prompt.md"), prompt)?;

        let mut plan = match generate_agent_design_plan(db, &skill_text, prompt, input.model_id, &title, requested_count, &style).await {
            Ok(plan) => normalize_slide_plan(plan, &title, requested_count, &style),
            Err(e) => {
                log_lines.push(format!("Agent 规划失败，使用 fallback slide_plan: {}", e));
                default_slide_plan(&title, requested_count, &style, prompt)
            }
        };
        ensure_layout_variety(&mut plan);

        let design_spec = build_agent_design_spec(&plan, prompt, &skill_text);
        let spec_lock = build_agent_spec_lock(&plan);
        let design_spec_path = project.join("design_spec.md");
        let spec_lock_path = project.join("spec_lock.md");
        let slide_plan_path = project.join("slide_plan.json");
        write_file(&design_spec_path, &design_spec)?;
        write_file(&spec_lock_path, &spec_lock)?;
        let plan_json = serde_json::to_string_pretty(&plan)
            .map_err(|e| AppError::Custom(format!("序列化 slide_plan 失败: {}", e)))?;
        write_file(&slide_plan_path, &plan_json)?;
        write_file(&notes.join("total.md"), &build_notes(&plan))?;
        log_lines.push(format!("写入 design_spec.md: {}", design_spec_path.display()));
        log_lines.push(format!("写入 spec_lock.md: {}", spec_lock_path.display()));
        log_lines.push(format!("写入 slide_plan.json: {}", slide_plan_path.display()));

        for idx in 0..plan.slides.len() {
            let slide = &plan.slides[idx];
            let prev_title = idx.checked_sub(1).and_then(|i| plan.slides.get(i)).map(|s| s.title.as_str()).unwrap_or("");
            let next_title = plan.slides.get(idx + 1).map(|s| s.title.as_str()).unwrap_or("");
            let svg = match generate_agent_slide_svg(
                db,
                &skill_text,
                &design_spec,
                &spec_lock,
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
                    log_lines.push(format!("第 {} 页 Agent SVG 生成失败，使用 template fallback: {}", slide.page, e));
                    render_slide_svg(&plan, slide)
                }
            };
            let filename = format!("{:02}_{}.svg", slide.page, safe_filename(&slide.title, "slide"));
            write_file(&svg_output.join(&filename), &svg)?;
            log_lines.push(format!("写入 SVG: svg_output/{}", filename));
        }

        let mut quality = run_quality_check(&root, &input.python_path, &project, started)?;
        log_lines.push("运行 svg_quality_checker.py".to_string());
        if !quality.success {
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
            quality = run_quality_check(&root, &input.python_path, &project, started)?;
        }
        let quality_passed = quality.success;
        log_lines.push(format!("SVG 质量检查通过: {}", quality_passed));

        let export = export_project(&root, &input.python_path, &project, started)?;
        let mut success = export.success;
        let mut error = export.error;
        let mut final_pptx_path = None;

        if export.success {
            if let (Some(dir), Some(pptx)) = (input.output_dir.as_deref(), export.output_path.as_deref()) {
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
        }

        let stdout = join_outputs(&log_lines, &[quality.stdout, export.stdout]);
        let stderr = join_outputs(&[], &[quality.stderr, export.stderr]);
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
    let ai_prompt = format!(
        "你是 PPT 策划专家和信息架构师。请基于用户确认后的 PPT 需求，输出严格 JSON，不要 markdown，不要代码块，不要解释。\n\
         JSON schema:\n\
         {{\"title\":\"PPT标题\",\"subtitle\":\"一句话副标题\",\"audience\":\"汇报对象\",\"style\":\"风格说明\",\"theme\":{{\"name\":\"tech-blue\",\"primary\":\"#2563eb\",\"secondary\":\"#7c3aed\",\"accent\":\"#38bdf8\",\"background\":\"#f8fbff\"}},\"slides\":[{{\"page\":1,\"type\":\"cover\",\"layout\":\"cover\",\"title\":\"封面标题\",\"subtitle\":\"副标题\",\"bullets\":[],\"visualHint\":\"大标题 + 抽象科技圆形装饰\",\"speakerNote\":\"演讲备注\"}},{{\"page\":2,\"type\":\"content\",\"layout\":\"timeline\",\"title\":\"发展脉络\",\"subtitle\":\"用时间线说明变化过程\",\"bullets\":[\"阶段1\",\"阶段2\",\"阶段3\"],\"visualHint\":\"横向时间线\",\"speakerNote\":\"演讲备注\"}}]}}\n\n\
         layout 只能从这些值中选择：cover, section, cards, timeline, compare, process, matrix, highlight, image_text, summary。\n\
         规则：封面必须 cover；最后一页优先 summary；不要每页都选 cards；涉及年份、历程、阶段优先 timeline；涉及两类对象、优劣、前后变化优先 compare；涉及流程、步骤、机制优先 process；涉及多个并列能力优先 cards 或 matrix；涉及核心数据/关键词优先 highlight。\n\
         要求：slides 数量尽量等于 {slide_count}；每页只表达一个核心观点；每页 3-5 个 bullet，避免大段文字；中文输出。\n\n\
         【建议标题】\n{title}\n\n【建议风格】\n{style}\n\n【确认 Prompt】\n{prompt}"
    );
    let input = PluginAiChatInput {
        request_id: "ppt_master_generate_slide_plan".to_string(),
        model_id,
        messages: vec![PluginAiMessage {
            role: "user".to_string(),
            content: ai_prompt,
        }],
    };
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let raw = AiService::plugin_chat_sync(db, input, cancel_rx).await?;
    parse_slide_plan_json(&raw)
}

fn read_ppt_master_skill(root: &Path) -> Result<String, AppError> {
    let path = root.join(PPT_MASTER_SKILL_MD);
    if !path.is_file() {
        return Err(AppError::NotFound(format!("找不到 ppt-master SKILL.md: {}", path.display())));
    }
    fs::read_to_string(&path)
        .map_err(|e| AppError::Custom(format!("读取 SKILL.md 失败: {} ({})", path.display(), e)))
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
        "你是 ppt-master Strategist。请基于用户确认 Prompt 和 ppt-master 规则，输出严格 JSON，不要 markdown，不要代码块。\n\
         目标：生成高质量 PPT 的 design plan，而不是普通 bullet PPT。\n\
         必须为每页指定 layout、视觉重心、排版说明 visualHint。每页应有明显不同的设计意图。\n\
         layout 只能取：cover, section, cards, timeline, compare, process, matrix, highlight, image_text, summary。\n\
         封面必须 cover，最后一页优先 summary，不要每页都 cards。\n\n\
         JSON schema:\n\
         {{\"title\":\"PPT标题\",\"subtitle\":\"一句话副标题\",\"audience\":\"汇报对象\",\"style\":\"风格说明\",\"theme\":{{\"name\":\"tech-blue\",\"primary\":\"#2563eb\",\"secondary\":\"#7c3aed\",\"accent\":\"#38bdf8\",\"background\":\"#f8fbff\"}},\"slides\":[{{\"page\":1,\"type\":\"cover\",\"layout\":\"cover\",\"title\":\"封面标题\",\"subtitle\":\"副标题\",\"bullets\":[],\"visualHint\":\"大标题 + 抽象科技圆形装饰\",\"speakerNote\":\"演讲备注\"}}]}}\n\n\
         【页数】{slide_count}\n【标题】{title}\n【风格】{style}\n\n\
         【ppt-master 核心规则摘要】\n{skill_excerpt}\n\n\
         【用户确认 Prompt】\n{prompt}",
        skill_excerpt = skill_excerpt(skill_text)
    );
    let input = PluginAiChatInput {
        request_id: "ppt_master_agent_design_plan".to_string(),
        model_id,
        messages: vec![PluginAiMessage { role: "user".to_string(), content: ai_prompt }],
    };
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let raw = AiService::plugin_chat_sync(db, input, cancel_rx).await?;
    parse_slide_plan_json(&raw)
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
        messages: vec![PluginAiMessage { role: "user".to_string(), content: ai_prompt }],
    };
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let raw = AiService::plugin_chat_sync(db, input, cancel_rx).await?;
    extract_svg(&raw).ok_or_else(|| AppError::Custom(format!("第 {} 页 AI 输出中找不到完整 SVG", slide.page)))
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
        let filename = format!("{:02}_{}.svg", slide.page, safe_filename(&slide.title, "slide"));
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
                PluginAiMessage { role: "system".to_string(), content: skill_excerpt(skill_text).to_string() },
                PluginAiMessage { role: "user".to_string(), content: prompt },
            ],
        };
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let raw = AiService::plugin_chat_sync(db, input, cancel_rx).await?;
        if let Some(svg) = extract_svg(&raw) {
            write_file(&path, &svg)?;
        }
    }
    Ok(())
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
    let start = raw.find('{').ok_or_else(|| AppError::Custom("AI 返回中找不到 JSON 起点".into()))?;
    let end = raw.rfind('}').ok_or_else(|| AppError::Custom("AI 返回中找不到 JSON 终点".into()))?;
    if end <= start {
        return Err(AppError::Custom("AI 返回 JSON 范围无效".into()));
    }
    serde_json::from_str::<SlidePlan>(&raw[start..=end])
        .map_err(|e| AppError::Custom(format!("AI 返回的 slide_plan JSON 无法解析: {}", e)))
}

fn normalize_slide_plan(mut plan: SlidePlan, title: &str, slide_count: usize, style: &str) -> SlidePlan {
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
        return default_slide_plan(title, slide_count, style, "");
    }
    if plan.slides.len() > slide_count {
        plan.slides.truncate(slide_count);
    }
    let total_slides = plan.slides.len();
    for (idx, slide) in plan.slides.iter_mut().enumerate() {
        slide.page = idx + 1;
        if slide.slide_type.trim().is_empty() {
            slide.slide_type = if idx == 0 { "cover" } else { "content" }.to_string();
        }
        if slide.layout.trim().is_empty() {
            slide.layout = choose_layout(idx, total_slides, &slide.title, &slide.subtitle, &slide.bullets);
        }
        slide.layout = normalize_layout(&slide.layout, idx, total_slides);
        if slide.title.trim().is_empty() {
            slide.title = if idx == 0 { plan.title.clone() } else { format!("第 {} 页", idx + 1) };
        }
        if slide.subtitle.trim().is_empty() {
            slide.subtitle = "本页核心观点".to_string();
        }
        if idx > 0 && slide.bullets.is_empty() {
            slide.bullets = vec!["关键信息".into(), "核心依据".into(), "建议行动".into()];
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
    plan
}

fn default_slide_plan(title: &str, slide_count: usize, style: &str, prompt: &str) -> SlidePlan {
    let count = slide_count.clamp(1, 30);
    let templates = [
        ("cover", "cover", "封面", "主题与一句话价值主张"),
        ("content", "cards", "背景痛点", "说明为什么需要这个方案"),
        ("content", "compare", "核心方案", "说明方案如何解决问题"),
        ("content", "process", "技术流程", "用流程化结构表达输入到输出"),
        ("content", "highlight", "Demo 展示", "说明最小闭环如何演示"),
        ("content", "summary", "总结展望", "归纳价值、亮点与后续拓展"),
        ("content", "timeline", "推进节奏", "用阶段结构说明实施路径"),
        ("content", "matrix", "能力矩阵", "从多个维度展示支撑能力"),
    ];
    let mut slides = Vec::with_capacity(count);
    for idx in 0..count {
        let (slide_type, layout, fallback_title, subtitle) = templates
            .get(idx)
            .copied()
            .unwrap_or(("content", "cards", "补充页面", "围绕主题补充关键说明"));
        let layout = normalize_layout(layout, idx, count);
        let visual_hint = visual_hint_for_layout(&layout).to_string();
        slides.push(Slide {
            page: idx + 1,
            slide_type: slide_type.to_string(),
            layout,
            title: if idx == 0 { title.to_string() } else { fallback_title.to_string() },
            subtitle: subtitle.to_string(),
            bullets: if idx == 0 {
                Vec::new()
            } else {
                vec![
                    "提炼确认 Prompt 中的关键事实".into(),
                    "突出听众最关心的价值点".into(),
                    "使用短句和结构化表达".into(),
                ]
            },
            visual_hint,
            speaker_note: if prompt.trim().is_empty() {
                format!("讲解 {}：{}", fallback_title, subtitle)
            } else {
                format!("围绕确认 Prompt 讲解本页重点：{}", subtitle)
            },
        });
    }
    SlidePlan {
        title: title.to_string(),
        subtitle: "基于确认需求自动生成的演示文稿".to_string(),
        audience: "目标听众".to_string(),
        style: style.to_string(),
        theme: theme_for_style(style),
        slides,
    }
}

fn create_project_dir(root: &Path) -> Result<PathBuf, AppError> {
    let projects = root.join("projects");
    create_dir_all(&projects)?;
    let base = format!("pome_ppt_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    for index in 0..100 {
        let name = if index == 0 { base.clone() } else { format!("{}_{}", base, index) };
        let path = projects.join(name);
        if !path.exists() {
            create_dir_all(&path)?;
            return Ok(path);
        }
    }
    Err(AppError::Custom("无法创建唯一的 ppt-master 项目目录".into()))
}

fn copy_final_pptx(source: &Path, output_dir: &str, title: &str) -> Result<PathBuf, AppError> {
    let trimmed = output_dir.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("导出文件夹不能为空".into()));
    }
    let dir = PathBuf::from(trimmed);
    if !dir.exists() {
        return Err(AppError::NotFound(format!("导出文件夹不存在: {}", dir.display())));
    }
    if !dir.is_dir() {
        return Err(AppError::InvalidInput(format!("导出路径不是文件夹: {}", dir.display())));
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
    out.push_str("- Goal: Generate visually varied, editable PPTX pages through ppt-master SVG export.\n\n");
    out.push_str("## Confirmed Prompt\n\n");
    out.push_str(prompt);
    out.push_str("\n\n## Color Scheme\n");
    out.push_str(&format!(
        "- Primary: {}\n- Secondary: {}\n- Accent: {}\n- Background: {}\n\n",
        plan.theme.primary, plan.theme.secondary, plan.theme.accent, plan.theme.background
    ));
    out.push_str("## Typography\n");
    out.push_str("- Font family: Microsoft YaHei, PingFang SC, SimSun, Arial, sans-serif\n");
    out.push_str("- Hierarchy: cover title 64-84, page title 40-52, subtitle 22-28, body 18-26.\n\n");
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
    let layouts = ["cover", "cards", "timeline", "compare", "process", "matrix", "highlight", "image_text", "summary"];
    for (idx, slide) in plan.slides.iter_mut().enumerate() {
        slide.layout = normalize_layout(layouts[idx % layouts.len()], idx, total);
        slide.visual_hint = visual_hint_for_layout(&slide.layout).to_string();
    }
}

fn build_notes(plan: &SlidePlan) -> String {
    let mut out = String::new();
    for slide in &plan.slides {
        out.push_str(&format!("# 第 {} 页：{}\n\n{}\n\n", slide.page, slide.title, slide.speaker_note));
    }
    out
}

fn render_slide_svg(plan: &SlidePlan, slide: &Slide) -> String {
    let palette = palette_for_style(&plan.style);
    let layout = normalize_layout(&slide.layout, slide.page.saturating_sub(1), plan.slides.len());
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
{footer}
</svg>
"#,
        bg1 = palette.bg1,
        bg2 = palette.bg2,
        accent = palette.accent,
        accent2 = palette.accent2,
        line = palette.line,
        body = body,
        footer = render_footer(plan, slide, palette)
    )
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
        subtitle = xml_escape(&slide.subtitle)
    )
}

fn render_footer(plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    format!(
        r#"<g id="footer">
<text x="92" y="858" font-size="18" fill="{muted}">{page:02} / {total:02}</text>
<text x="1508" y="858" text-anchor="end" font-size="18" fill="{muted}">Pomegranate · PPT Master</text>
</g>"#,
        muted = palette.muted,
        page = slide.page,
        total = plan.slides.len()
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
<text x="154" y="670" font-size="20" fill="{muted}">Pomegranate · PPT Master</text>
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
        subtitle = render_wrapped_text(&slide.subtitle, 158, 442, 24, 30, 42, palette.muted, "400", 2),
        audience = xml_escape(&plan.audience),
        style = xml_escape(&plan.style)
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
        subtitle = render_wrapped_text(&slide.subtitle, 164, 562, 30, 28, 40, palette.muted, "400", 2)
    )
}

fn render_cards_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    let bullets = slide_bullets(slide);
    let mut cards = String::new();
    let positions = [(104, 250), (556, 250), (1008, 250), (104, 540), (556, 540), (1008, 540)];
    for (idx, bullet) in bullets.iter().take(6).enumerate() {
        let (x, y) = positions[idx];
        cards.push_str(&format!(
            r##"<g id="card-{idx}">
<rect x="{x}" y="{y}" width="388" height="218" rx="24" fill="{surface}" stroke="{line}" stroke-width="2"/>
<circle cx="{cx}" cy="{cy}" r="28" fill="{accent}"/>
<text x="{cx}" y="{num_y}" text-anchor="middle" font-size="22" font-weight="700" fill="#ffffff">{num}</text>
{text}
</g>
"##,
            idx = idx + 1,
            x = x,
            y = y,
            cx = x + 48,
            cy = y + 48,
            num_y = y + 56,
            num = idx + 1,
            surface = palette.surface,
            line = palette.line,
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            text = render_wrapped_text(bullet, x + 92, y + 54, 15, 24, 34, palette.text, "600", 4)
        ));
    }
    format!("{}{}", render_header(slide, palette), cards)
}

fn render_timeline_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    let bullets = slide_bullets(slide);
    let count = bullets.len().clamp(3, 5);
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

fn render_compare_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
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

fn render_process_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
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

fn render_matrix_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
    let bullets = slide_bullets(slide);
    let positions = [(160, 250), (850, 250), (160, 520), (850, 520)];
    let mut cells = String::new();
    for idx in 0..4 {
        let bullet = bullets.get(idx).cloned().unwrap_or_else(|| format!("关键维度 {}", idx + 1));
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

fn render_highlight_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
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

fn render_image_text_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
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
        hint = xml_escape(if slide.visual_hint.trim().is_empty() { "抽象视觉占位" } else { &slide.visual_hint }),
        bullets = render_bullet_list(&bullets, 830, 336, 540, palette, "image-text")
    )
}

fn render_summary_slide(_plan: &SlidePlan, slide: &Slide, palette: &Palette) -> String {
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

fn choose_layout(idx: usize, total: usize, title: &str, subtitle: &str, bullets: &[String]) -> String {
    if idx == 0 {
        return "cover".to_string();
    }
    if idx + 1 == total {
        return "summary".to_string();
    }
    let text = format!("{} {} {}", title, subtitle, bullets.join(" "));
    if text.contains("年") || text.contains("阶段") || text.contains("历程") || text.contains("路径") {
        "timeline".to_string()
    } else if text.contains("对比") || text.contains("前后") || text.contains("传统") || text.contains("问题") {
        "compare".to_string()
    } else if text.contains("流程") || text.contains("步骤") || text.contains("机制") || text.contains("闭环") {
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
        "cover" | "section" | "cards" | "timeline" | "compare" | "process" | "matrix" | "highlight" | "image_text" | "summary" => layout.trim().to_string(),
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
        Palette { name: "tech-blue", bg1: "#f8fbff", bg2: "#eef4ff", surface: "#ffffff", title: "#102033", text: "#1f2937", muted: "#52637a", line: "#bdd7ff", accent: "#2563eb", accent2: "#7c3aed", highlight: "#38bdf8" }
    } else if style.contains("竞赛") || style.contains("路演") {
        Palette { name: "pitch", bg1: "#fff7ed", bg2: "#ffffff", surface: "#ffffff", title: "#111827", text: "#243042", muted: "#6b7280", line: "#fed7aa", accent: "#111827", accent2: "#7c3aed", highlight: "#f97316" }
    } else if style.contains("学术") {
        Palette { name: "academic", bg1: "#f8fafc", bg2: "#eef4fb", surface: "#ffffff", title: "#1e3a8a", text: "#1f2a37", muted: "#64748b", line: "#c7d7ea", accent: "#1e3a8a", accent2: "#64748b", highlight: "#b91c1c" }
    } else if style.contains("图文") {
        Palette { name: "visual", bg1: "#f9fafb", bg2: "#ecfeff", surface: "#ffffff", title: "#12343b", text: "#243042", muted: "#5c6670", line: "#c7d2fe", accent: "#0f766e", accent2: "#2563eb", highlight: "#e11d48" }
    } else {
        Palette { name: "business", bg1: "#ffffff", bg2: "#f3f6fb", surface: "#ffffff", title: "#1f2937", text: "#263244", muted: "#667085", line: "#d0d7e2", accent: "#1f2937", accent2: "#2563eb", highlight: "#f59e0b" }
    }
}

fn slide_bullets(slide: &Slide) -> Vec<String> {
    let mut bullets: Vec<String> = slide
        .bullets
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .take(5)
        .map(ToString::to_string)
        .collect();
    if bullets.is_empty() {
        bullets.push(slide.subtitle.clone());
    }
    bullets
}

fn split_bullets(bullets: &[String]) -> (Vec<String>, Vec<String>) {
    let mid = (bullets.len() + 1) / 2;
    let left = bullets[..mid].to_vec();
    let right = bullets[mid..].to_vec();
    let fallback_right = left.clone();
    (left, if right.is_empty() { fallback_right } else { right })
}

fn render_bullet_list(items: &[String], x: i32, y: i32, max_chars: usize, palette: &Palette, id: &str) -> String {
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
            accent = if idx % 2 == 0 { palette.accent } else { palette.accent2 },
            text = render_wrapped_text(item, x + 28, item_y, (max_chars / 24).max(12), 22, 31, palette.text, "500", 2)
        ));
    }
    out
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

    let output = cmd
        .output()
        .map_err(|e| AppError::Custom(format!("无法启动 SVG 质量检查: {} ({})", python.display(), e)))?;
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
                output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "未知".to_string())
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
                exit_code.map(|c| c.to_string()).unwrap_or_else(|| "未知".to_string())
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
        } else if ch.is_whitespace() || matches!(ch, ':' | '：' | '/' | '\\' | '|' | '?' | '*' | '"' | '<' | '>') {
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
