use serde::{Deserialize, Serialize};

use super::{ContentBlock, PptMasterGenerateInput, Slide, SlidePlan, Theme, ThemeAllocation};

pub(super) const NATIVE_QUALITY_REPORT_FILE: &str = "native_quality_plan.json";
const NATIVE_QUALITY_SCHEMA_VERSION: u32 = 1;
const NATIVE_QUALITY_SPEC_VERSION: &str = "pomegranate-native-quality-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeQualityReport {
    pub schema_version: u32,
    pub spec_version: String,
    pub enabled: bool,
    pub theme: NativeThemeContract,
    pub slides: Vec<NativeSlideQuality>,
    pub density_summary: NativeDensitySummary,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeThemeContract {
    pub theme_name: String,
    pub mood: String,
    pub background_strategy: String,
    pub background_color: String,
    pub secondary_background_color: String,
    pub surface_color: String,
    pub primary_color: String,
    pub secondary_color: String,
    pub accent_color: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub source_style: String,
    pub user_theme_priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeSlideQuality {
    pub page: usize,
    pub page_id: String,
    pub narrative_role: String,
    pub layout_intent: String,
    pub page_theme: String,
    pub density: NativeDensityContract,
    pub required_facts: Vec<String>,
    pub avoid: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeDensityContract {
    pub page_rhythm: String,
    pub content_density: String,
    pub visual_weight: String,
    pub focal_region: String,
    pub supporting_regions: Vec<String>,
    pub expected_content_units: usize,
    pub minimum_supporting_regions: usize,
    pub allow_large_whitespace: bool,
    pub layout_capacity_strategy: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeDensitySummary {
    pub sparse_pages: usize,
    pub normal_pages: usize,
    pub slightly_dense_pages: usize,
    pub hard_dense_pages: usize,
}

#[derive(Debug, Clone)]
pub(super) struct NativeQualityOutcome {
    pub plan: SlidePlan,
    pub report: NativeQualityReport,
    pub log_lines: Vec<String>,
}

pub(super) fn is_enabled(input: &PptMasterGenerateInput) -> bool {
    input.native_quality_enabled.unwrap_or(false)
}

pub(super) fn apply_native_quality_chain(
    mut plan: SlidePlan,
    input: &PptMasterGenerateInput,
    planning_context: &str,
) -> NativeQualityOutcome {
    let theme = NativeThemeContract::from_input(input);
    let mut warnings = Vec::new();
    let mut log_lines = vec![
        "[Native Quality] enabled=true".to_string(),
        "[Native Quality] phase=planning-theme-density".to_string(),
    ];

    plan.theme = theme.as_slide_theme();
    plan.theme_allocation.clear();

    let total = plan.slides.len().max(1);
    let context_keywords = planning_keywords(planning_context);
    let mut report_slides = Vec::new();
    let mut summary = NativeDensitySummary::default();

    for index in 0..plan.slides.len() {
        let slide = &mut plan.slides[index];
        let quality = enhance_slide(slide, index, total, &context_keywords);
        match quality.density.severity.as_str() {
            "sparse" => summary.sparse_pages += 1,
            "slightly_dense" => summary.slightly_dense_pages += 1,
            "hard_dense" => summary.hard_dense_pages += 1,
            _ => summary.normal_pages += 1,
        }
        for warning in &quality.warnings {
            warnings.push(format!("P{:02}: {}", slide.page, warning));
        }
        plan.theme_allocation.push(ThemeAllocation {
            page_id: slide.page_id.clone(),
            assigned_theme: slide.page_theme.clone(),
            exclusive_scope: slide.content_scope.clone(),
        });
        log_lines.push(format!(
            "[Native Quality] P{:02} role={} density={} severity={} units={}",
            quality.page,
            quality.narrative_role,
            quality.density.page_rhythm,
            quality.density.severity,
            quality.density.expected_content_units
        ));
        report_slides.push(quality);
    }

    log_lines.push(format!(
        "[Native Quality] theme={} userThemePriority={}",
        theme.theme_name, theme.user_theme_priority
    ));
    log_lines.push(format!(
        "[Native Quality] density normal={} sparse={} slightlyDense={} hardDense={}",
        summary.normal_pages,
        summary.sparse_pages,
        summary.slightly_dense_pages,
        summary.hard_dense_pages
    ));

    let report = NativeQualityReport {
        schema_version: NATIVE_QUALITY_SCHEMA_VERSION,
        spec_version: NATIVE_QUALITY_SPEC_VERSION.to_string(),
        enabled: true,
        theme,
        slides: report_slides,
        density_summary: summary,
        warnings,
    };

    NativeQualityOutcome {
        plan,
        report,
        log_lines,
    }
}

impl NativeThemeContract {
    fn from_input(input: &PptMasterGenerateInput) -> Self {
        let style = input.style.as_deref().unwrap_or("").trim();
        let custom_style = input.custom_style.as_deref().unwrap_or("").trim();
        let extra = input.extra_requirements.as_deref().unwrap_or("").trim();
        let visual = input
            .visual_expression_advice
            .as_deref()
            .unwrap_or("")
            .trim();
        let explicit = format!("{style}\n{custom_style}\n{extra}\n{visual}").to_lowercase();
        let user_theme_priority = if !custom_style.is_empty() {
            "customStyle"
        } else if !style.is_empty() {
            "style"
        } else {
            "default"
        };

        if contains_any(
            &explicit,
            &[
                "red heritage",
                "red",
                "heritage",
                "party history",
                "revolution",
                "hongse",
                "红色",
                "党史",
                "革命",
            ],
        ) {
            Self::preset(
                "red-heritage",
                "solemn, warm, historical",
                "warm neutral pages with restrained red and gold anchors",
                "#F7F0E6",
                "#8B1E1E",
                "#FFF9F0",
                "#B91C1C",
                "#7F1D1D",
                "#D4A017",
                "#2A1713",
                "#6B4B43",
                style,
                user_theme_priority,
            )
        } else if contains_any(&explicit, &["tech", "blue", "future", "科技", "蓝", "未来"]) {
            Self::preset(
                "tech-blue",
                "precise, modern, data oriented",
                "deep or cool blue palette with bright technical accents",
                "#081426",
                "#0D2340",
                "#102B4E",
                "#2563EB",
                "#38BDF8",
                "#7C3AED",
                "#F8FBFF",
                "#CBD5E1",
                style,
                user_theme_priority,
            )
        } else if contains_any(&explicit, &["academic", "paper", "defense", "学术", "论文"]) {
            Self::preset(
                "academic",
                "calm, rigorous, restrained",
                "light academic background with reserved emphasis colors",
                "#F8FAFC",
                "#EEF4FB",
                "#FFFFFF",
                "#1E3A8A",
                "#64748B",
                "#B91C1C",
                "#1F2A37",
                "#64748B",
                style,
                user_theme_priority,
            )
        } else {
            Self::preset(
                if style.is_empty() { "business" } else { style },
                "clear, balanced, consistent",
                "neutral deck-wide palette with a single accent color",
                "#FAFAF8",
                "#F1F0EA",
                "#FFFFFF",
                "#334155",
                "#64748B",
                "#C07A35",
                "#1F2937",
                "#64748B",
                style,
                user_theme_priority,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn preset(
        theme_name: &str,
        mood: &str,
        background_strategy: &str,
        background_color: &str,
        secondary_background_color: &str,
        surface_color: &str,
        primary_color: &str,
        secondary_color: &str,
        accent_color: &str,
        text_primary: &str,
        text_secondary: &str,
        source_style: &str,
        user_theme_priority: &str,
    ) -> Self {
        Self {
            theme_name: theme_name.to_string(),
            mood: mood.to_string(),
            background_strategy: background_strategy.to_string(),
            background_color: background_color.to_string(),
            secondary_background_color: secondary_background_color.to_string(),
            surface_color: surface_color.to_string(),
            primary_color: primary_color.to_string(),
            secondary_color: secondary_color.to_string(),
            accent_color: accent_color.to_string(),
            text_primary: text_primary.to_string(),
            text_secondary: text_secondary.to_string(),
            source_style: source_style.to_string(),
            user_theme_priority: user_theme_priority.to_string(),
        }
    }

    fn as_slide_theme(&self) -> Theme {
        Theme {
            name: self.theme_name.clone(),
            primary: self.primary_color.clone(),
            secondary: self.secondary_color.clone(),
            accent: self.accent_color.clone(),
            background: self.background_color.clone(),
        }
    }
}

fn enhance_slide(
    slide: &mut Slide,
    index: usize,
    total: usize,
    context_keywords: &[String],
) -> NativeSlideQuality {
    let narrative_role = narrative_role(slide, index, total);
    let layout_intent = layout_intent(slide);
    if slide.page_theme.trim().is_empty()
        || generic_page_theme(&slide.page_theme, slide.page)
        || (index > 0 && slide.page_theme == slide.title)
    {
        slide.page_theme = narrative_role.clone();
    }
    if slide.content_scope.trim().is_empty() {
        slide.content_scope = slide
            .content_blocks
            .iter()
            .map(block_text)
            .filter(|item| !item.is_empty())
            .take(2)
            .collect::<Vec<_>>()
            .join(" | ");
    }
    if slide.content_scope.trim().is_empty() {
        slide.content_scope = slide.core_message.clone();
    }

    let density = NativeDensityContract::for_slide(slide, index, total);
    slide.density = density.page_rhythm.clone();
    slide.page_rhythm = density.page_rhythm.clone();
    if slide.visual_intent.trim().is_empty() {
        slide.visual_intent = format!(
            "{}; density {}; focal {}",
            layout_intent, density.page_rhythm, density.focal_region
        );
    }

    let required_facts = required_facts(slide, context_keywords);
    if slide.must_include.is_empty() {
        slide.must_include = required_facts.clone();
    }
    let mut avoid = slide.must_avoid.clone();
    for value in [
        "Do not show non-content process notes",
        "Do not show implementation labels",
        "Do not duplicate adjacent page themes",
    ] {
        if !avoid.iter().any(|item| item == value) {
            avoid.push(value.to_string());
        }
    }
    slide.must_avoid = avoid.clone();

    let warnings = density_warnings(&density);
    NativeSlideQuality {
        page: slide.page,
        page_id: slide.page_id.clone(),
        narrative_role,
        layout_intent,
        page_theme: slide.page_theme.clone(),
        density,
        required_facts,
        avoid,
        warnings,
    }
}

impl NativeDensityContract {
    fn for_slide(slide: &Slide, index: usize, total: usize) -> Self {
        let units = content_units(slide);
        let semantic_anchor = index == 0
            || index + 1 == total
            || matches!(slide.layout.as_str(), "cover" | "section" | "highlight")
            || matches!(slide.slide_type.as_str(), "cover" | "section" | "quote");
        let page_rhythm = if semantic_anchor {
            if units <= 2 {
                "anchor"
            } else {
                "breathing"
            }
        } else if matches!(
            slide.layout.as_str(),
            "timeline" | "process" | "compare" | "matrix"
        ) || units >= 5
        {
            "dense"
        } else {
            "balanced"
        };
        let severity = match (semantic_anchor, units) {
            (_, 0..=1) if index > 0 && index + 1 < total => "sparse",
            (_, 0..=2) => "normal",
            (_, 3..=6) => "normal",
            (false, 7..=8) => "slightly_dense",
            (true, 7..=9) => "slightly_dense",
            _ => "hard_dense",
        };
        let content_density = match units {
            0..=2 => "low",
            3..=4 => "medium",
            5..=8 => "high",
            _ => "too_high",
        };
        let (visual_weight, focal_region, supporting_regions, minimum_supporting_regions) =
            match page_rhythm {
                "anchor" => (
                    "strong",
                    "dominant-center-or-asymmetric-hero",
                    vec!["context-band".to_string()],
                    0,
                ),
                "breathing" => (
                    "strong",
                    "single-dominant-region",
                    vec!["supporting-context".to_string()],
                    1,
                ),
                "dense" => (
                    "balanced",
                    "main-evidence-region",
                    vec![
                        "secondary-evidence".to_string(),
                        "navigation-structure".to_string(),
                    ],
                    2,
                ),
                _ => (
                    "balanced",
                    "main-content-region",
                    vec!["secondary-information".to_string()],
                    1,
                ),
            };
        let layout_capacity_strategy = if semantic_anchor {
            "single-focal-anchor"
        } else if units <= 2 {
            "dominant-claim-with-semantic-support"
        } else if slide.layout == "timeline" && units <= 3 {
            "compact-staged-path"
        } else if matches!(slide.layout.as_str(), "cards" | "matrix") && units < 4 {
            "semantic-split-not-empty-grid"
        } else if severity == "hard_dense" {
            "must-reduce-or-move-detail-to-speaker-notes"
        } else {
            "full-main-region-with-support"
        };

        Self {
            page_rhythm: page_rhythm.to_string(),
            content_density: content_density.to_string(),
            visual_weight: visual_weight.to_string(),
            focal_region: focal_region.to_string(),
            supporting_regions,
            expected_content_units: units,
            minimum_supporting_regions,
            allow_large_whitespace: semantic_anchor,
            layout_capacity_strategy: layout_capacity_strategy.to_string(),
            severity: severity.to_string(),
        }
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn content_units(slide: &Slide) -> usize {
    slide
        .content_blocks
        .len()
        .max(slide.bullets.len())
        .max(slide.evidence.len())
        .max(slide.must_include.len())
}

fn density_warnings(density: &NativeDensityContract) -> Vec<String> {
    match density.severity.as_str() {
        "sparse" => vec!["content is too sparse for a normal content page".to_string()],
        "slightly_dense" => vec!["content is slightly dense; prefer concise labels".to_string()],
        "hard_dense" => {
            vec!["content is hard dense; move detail to speaker notes before rendering".to_string()]
        }
        _ => Vec::new(),
    }
}

fn narrative_role(slide: &Slide, index: usize, total: usize) -> String {
    if index == 0 {
        return "opening and audience orientation".to_string();
    }
    if index + 1 == total {
        return "summary and closing action".to_string();
    }
    let title = slide.title.trim();
    let claim = slide.core_message.trim();
    if !claim.is_empty() && claim != title {
        format!("evidence for {}", clamp_words(claim, 10))
    } else if !title.is_empty() {
        format!("develop {}", clamp_words(title, 10))
    } else {
        format!("page {} narrative step", index + 1)
    }
}

fn layout_intent(slide: &Slide) -> String {
    match slide.layout.as_str() {
        "timeline" => "timeline",
        "process" => "process",
        "compare" => "comparison",
        "matrix" => "matrix",
        "highlight" => "hero",
        "summary" => "summary",
        "cover" => "hero",
        _ => match slide.relation.as_str() {
            "timeline" => "timeline",
            "process" => "process",
            "compare" => "comparison",
            "cause" => "cause-effect",
            _ => "editorial-split",
        },
    }
    .to_string()
}

fn generic_page_theme(value: &str, page: usize) -> bool {
    let trimmed = value.trim();
    trimmed.eq_ignore_ascii_case(&format!("page {page}"))
        || trimmed.eq_ignore_ascii_case(&format!("P{page:02}"))
        || trimmed == format!("第 {page} 页")
}

fn required_facts(slide: &Slide, context_keywords: &[String]) -> Vec<String> {
    let mut facts = Vec::new();
    for item in slide
        .content_blocks
        .iter()
        .map(block_text)
        .chain(slide.evidence.iter().map(|item| item.trim().to_string()))
        .chain(slide.bullets.iter().map(|item| item.trim().to_string()))
    {
        push_unique(&mut facts, item);
        if facts.len() >= 5 {
            break;
        }
    }
    for keyword in context_keywords {
        if facts.len() >= 5 {
            break;
        }
        push_unique(&mut facts, keyword.clone());
    }
    if facts.is_empty() {
        push_unique(&mut facts, slide.core_message.trim().to_string());
    }
    facts
}

fn planning_keywords(planning_context: &str) -> Vec<String> {
    let mut keywords = Vec::new();
    for line in planning_context.lines() {
        let cleaned = line
            .trim()
            .trim_start_matches(['-', '*', '#', ' ', '\t'])
            .trim()
            .to_string();
        let len = cleaned.chars().count();
        if (4..=80).contains(&len) && !looks_like_internal_diagnostic(&cleaned) {
            push_unique(&mut keywords, cleaned);
        }
        if keywords.len() >= 8 {
            break;
        }
    }
    keywords
}

fn looks_like_internal_diagnostic(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let compact: String = lower.chars().filter(|ch| !ch.is_whitespace()).collect();
    let key_like = compact.contains("requestkind=")
        || compact.contains("request_kind=")
        || compact.contains("runid=")
        || compact.contains("run_id=")
        || compact.contains("cachekey=")
        || compact.contains("cache_key=")
        || compact.contains("system_prompt")
        || compact.contains("internalprompt")
        || compact.contains("internal_prompt")
        || compact.contains("planning_context")
        || compact.contains("native_quality")
        || compact.contains("slide_plan")
        || compact.contains("role=system")
        || compact.contains("\"role\":\"system\"")
        || compact.contains("'role':'system'");
    let credential_like = compact.contains("authorization:")
        || compact.contains("authorization=")
        || compact.contains("apikey=")
        || compact.contains("api_key=")
        || compact.contains("apisecret=")
        || compact.contains("api_secret=")
        || compact.contains("bearertoken=")
        || compact.contains("access_token=");
    key_like || credential_like
}

fn block_text(block: &ContentBlock) -> String {
    let label = block.label.trim();
    let text = block.text.trim();
    let detail = block.detail.trim();
    if !label.is_empty() && !text.is_empty() {
        format!("{label}: {text}")
    } else if !text.is_empty() {
        text.to_string()
    } else if !label.is_empty() {
        label.to_string()
    } else {
        detail.to_string()
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if !values.iter().any(|item| item == trimmed) {
        values.push(trimmed.to_string());
    }
}

fn clamp_words(value: &str, max_words: usize) -> String {
    let mut out = value
        .split_whitespace()
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ");
    if out.is_empty() {
        out = value.chars().take(40).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(label: &str, text: &str) -> ContentBlock {
        ContentBlock {
            label: label.to_string(),
            text: text.to_string(),
            detail: String::new(),
        }
    }

    fn slide(page: usize, layout: &str, units: usize) -> Slide {
        Slide {
            page,
            page_index: page,
            page_id: format!("P{page:02}"),
            slide_type: if page == 1 { "cover" } else { "content" }.to_string(),
            layout: layout.to_string(),
            title: format!("Slide {page}"),
            subtitle: "subtitle".to_string(),
            bullets: (0..units).map(|idx| format!("bullet {idx}")).collect(),
            visual_hint: String::new(),
            page_theme: String::new(),
            main_claim: format!("claim {page}"),
            core_message: format!("core {page}"),
            content_scope: String::new(),
            content_blocks: (0..units)
                .map(|idx| block(&format!("point {idx}"), &format!("fact {idx}")))
                .collect(),
            evidence: Vec::new(),
            relation: String::new(),
            density: String::new(),
            visual_intent: String::new(),
            must_include: Vec::new(),
            must_avoid: Vec::new(),
            page_rhythm: String::new(),
            chart_ref: String::new(),
            chart_type: String::new(),
            file_stem: format!("slide_{page:02}"),
            speaker_note: String::new(),
        }
    }

    fn plan() -> SlidePlan {
        SlidePlan {
            title: "Deck".to_string(),
            subtitle: "Sub".to_string(),
            audience: "Audience".to_string(),
            style: "business".to_string(),
            theme: Theme {
                name: "old".to_string(),
                primary: "#111111".to_string(),
                secondary: "#222222".to_string(),
                accent: "#333333".to_string(),
                background: "#FFFFFF".to_string(),
            },
            theme_allocation: Vec::new(),
            slides: vec![
                slide(1, "cover", 1),
                slide(2, "cards", 1),
                slide(3, "timeline", 5),
                slide(4, "matrix", 8),
                slide(5, "summary", 2),
            ],
        }
    }

    fn input(enabled: bool) -> PptMasterGenerateInput {
        serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python",
            "prompt": "topic",
            "style": "tech blue",
            "customStyle": null,
            "nativeQualityEnabled": enabled
        }))
        .expect("deserialize input")
    }

    #[test]
    fn native_quality_defaults_to_disabled() {
        let parsed: PptMasterGenerateInput = serde_json::from_value(serde_json::json!({
            "pptMasterRoot": "D:/ppt-master",
            "pythonPath": "python"
        }))
        .expect("deserialize input");
        assert!(!is_enabled(&parsed));
    }

    #[test]
    fn native_quality_applies_planning_theme_and_density() {
        let outcome = apply_native_quality_chain(plan(), &input(true), "process quality timeline");
        assert_eq!(outcome.report.theme.theme_name, "tech-blue");
        assert_eq!(outcome.plan.theme.name, "tech-blue");
        assert_eq!(outcome.report.slides.len(), 5);
        assert_eq!(outcome.plan.theme_allocation.len(), 5);
        assert!(outcome
            .report
            .slides
            .iter()
            .any(|slide| slide.density.page_rhythm == "dense"));
        assert!(outcome
            .log_lines
            .iter()
            .any(|line| line.contains("phase=planning-theme-density")));
    }

    #[test]
    fn custom_theme_has_priority_over_generic_style() {
        let mut input = input(true);
        input.style = Some("tech blue".to_string());
        input.custom_style = Some("red heritage".to_string());
        let outcome = apply_native_quality_chain(plan(), &input, "");
        assert_eq!(outcome.report.theme.theme_name, "red-heritage");
        assert_eq!(outcome.report.theme.user_theme_priority, "customStyle");
        assert_eq!(outcome.plan.theme.primary, "#B91C1C");
    }

    #[test]
    fn density_classifies_sparse_normal_slight_and_hard_dense() {
        let outcome = apply_native_quality_chain(plan(), &input(true), "");
        assert!(outcome.report.density_summary.sparse_pages >= 1);
        assert!(outcome.report.density_summary.normal_pages >= 1);
        assert!(outcome.report.density_summary.slightly_dense_pages >= 1);
        assert_eq!(outcome.report.density_summary.hard_dense_pages, 0);

        let mut very_dense = plan();
        very_dense.slides[3].content_blocks = (0..12)
            .map(|idx| block(&format!("p{idx}"), &format!("fact {idx}")))
            .collect();
        let dense = apply_native_quality_chain(very_dense, &input(true), "");
        assert!(dense.report.density_summary.hard_dense_pages >= 1);
        assert!(dense
            .report
            .warnings
            .iter()
            .any(|warning| warning.contains("hard dense")));
    }

    #[test]
    fn native_quality_report_does_not_expose_internal_runtime_fields() {
        let outcome = apply_native_quality_chain(
            plan(),
            &input(true),
            "requestKind=chunk\nrunId=secret\ncacheKey=should-not-leak\nVisible finding",
        );
        let json = serde_json::to_string(&outcome.report).expect("serialize report");
        assert!(!json.contains("requestKind"));
        assert!(!json.contains("runId"));
        assert!(!json.contains("cacheKey"));
        assert!(!json.to_ascii_lowercase().contains("system"));
        assert!(!json.to_ascii_lowercase().contains("prompt"));
        assert!(json.contains("Visible finding"));
    }

    #[test]
    fn native_quality_filter_does_not_reject_normal_domain_terms() {
        let context = "manufacturing system integration\ncontrol system feedback\nPrompt工程的教学案例\ncache原理与局部性\nrequestKind=chunk\nrole=system";
        let keywords = planning_keywords(context);
        assert!(keywords
            .iter()
            .any(|item| item == "manufacturing system integration"));
        assert!(keywords
            .iter()
            .any(|item| item == "control system feedback"));
        assert!(keywords.iter().any(|item| item == "Prompt工程的教学案例"));
        assert!(keywords.iter().any(|item| item == "cache原理与局部性"));
        assert!(!keywords.iter().any(|item| item.contains("requestKind")));
        assert!(!keywords.iter().any(|item| item.contains("role=system")));
    }
}
