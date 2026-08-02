use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::native_theme::NativeThemeSpec;
use super::{Slide, SlidePlan};

pub(super) const NATIVE_DESIGN_SYSTEM_SPEC_FILE: &str = "native_design_system_spec.json";
pub(super) const NATIVE_DESIGN_CONSISTENCY_REPORT_FILE: &str = "native_design_consistency.json";
const NATIVE_DESIGN_SYSTEM_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct IntegerRange {
    pub minimum: u32,
    pub maximum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeColorSystem {
    pub background_primary: String,
    pub background_secondary: String,
    pub surface_color: String,
    pub primary_color: String,
    pub secondary_color: String,
    pub accent_color: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub border_color: String,
    pub muted_color: String,
    pub positive_color: String,
    pub warning_color: String,
    pub negative_color: String,
    pub forbidden_colors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeTypographySystem {
    pub title_font_family: String,
    pub body_font_family: String,
    pub display_font_family: String,
    pub title_size_range: IntegerRange,
    pub subtitle_size_range: IntegerRange,
    pub body_size_range: IntegerRange,
    pub label_size_range: IntegerRange,
    pub number_size_range: IntegerRange,
    pub font_weight_rules: Vec<String>,
    pub line_height_rules: Vec<String>,
    pub maximum_font_families: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeGridSystem {
    pub canvas_safe_margin: u32,
    pub content_grid: String,
    pub column_count: usize,
    pub gutter: u32,
    pub section_spacing: u32,
    pub element_spacing: u32,
    pub card_padding: u32,
    pub alignment_rules: Vec<String>,
    pub page_density_range: IntegerRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeGraphicLanguage {
    pub corner_radius: u32,
    pub border_width: u32,
    pub line_style: String,
    pub card_style: String,
    pub shadow_style: String,
    pub shape_language: String,
    pub icon_style: String,
    pub chart_style: String,
    pub connector_style: String,
    pub decorative_elements: Vec<String>,
    pub image_treatment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeDeckRhythm {
    pub cover_rhythm: String,
    pub body_rhythm: Vec<String>,
    pub breathing_pages: Vec<String>,
    pub dense_pages: Vec<String>,
    pub section_transition_strategy: String,
    pub layout_family_roster: Vec<String>,
    pub maximum_layout_repetition: usize,
    pub adjacent_page_difference: String,
    pub focal_point_strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeContentHierarchy {
    pub page_core_message: String,
    pub primary_visual: String,
    pub supporting_visuals: Vec<String>,
    pub headline_priority: String,
    pub body_priority: String,
    pub evidence_priority: String,
    pub maximum_content_units: usize,
    pub minimum_content_units: usize,
    pub emphasis_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeDesignForbiddens {
    pub forbidden_layout_patterns: Vec<String>,
    pub forbidden_visual_patterns: Vec<String>,
    pub forbidden_font_behavior: Vec<String>,
    pub forbidden_color_behavior: Vec<String>,
    pub forbidden_density_behavior: Vec<String>,
    pub forbidden_repetition: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativePageDesignContract {
    pub page_index: usize,
    pub narrative_role: String,
    pub page_rhythm: String,
    pub layout_family: String,
    pub content_density: String,
    pub focal_point: String,
    pub dominant_visual_type: String,
    pub difference_from_previous: String,
    pub consistency_requirements: Vec<String>,
    pub forbidden_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeDesignSystemSpec {
    pub schema_version: u32,
    pub contract_name: String,
    pub source_style: String,
    pub source_custom_style: String,
    pub source_extra_requirements: String,
    pub source_visual_suggestions: String,
    pub semantic_style_axes: Vec<String>,
    pub color_system: NativeColorSystem,
    pub typography_system: NativeTypographySystem,
    pub grid_system: NativeGridSystem,
    pub graphic_language: NativeGraphicLanguage,
    pub deck_rhythm: NativeDeckRhythm,
    pub content_hierarchy: NativeContentHierarchy,
    pub forbiddens: NativeDesignForbiddens,
    #[serde(default)]
    pub page_contracts: Vec<NativePageDesignContract>,
}

impl NativeDesignSystemSpec {
    pub(super) fn from_inputs(
        theme: &NativeThemeSpec,
        style: &str,
        custom_style: Option<&str>,
        extra_requirements: Option<&str>,
        visual_suggestions: Option<&str>,
    ) -> Self {
        let custom_style = custom_style.unwrap_or("").trim();
        let extra_requirements = extra_requirements.unwrap_or("").trim();
        let visual_suggestions = visual_suggestions.unwrap_or("").trim();
        let corpus = format!(
            "{} {} {} {} {} {} {} {}",
            style,
            custom_style,
            extra_requirements,
            visual_suggestions,
            theme.mood,
            theme.shape_language,
            theme.decoration_language,
            theme.image_treatment
        )
        .to_lowercase();
        let dark = color_luminance(&theme.background_color) < 0.34
            || contains_any(&corpus, &["深色", "暗色", "dark", "夜", "黑"]);
        let editorial = contains_any(
            &corpus,
            &[
                "历史",
                "文化",
                "人文",
                "典雅",
                "档案",
                "编辑",
                "杂志",
                "heritage",
                "editorial",
            ],
        );
        let technical = contains_any(
            &corpus,
            &[
                "科技",
                "技术",
                "工业",
                "未来",
                "精密",
                "数据",
                "digital",
                "tech",
                "industrial",
            ],
        );
        let friendly = contains_any(
            &corpus,
            &[
                "教育",
                "清新",
                "亲和",
                "成长",
                "自然",
                "轻松",
                "fresh",
                "friendly",
                "education",
            ],
        );
        let formal = contains_any(
            &corpus,
            &[
                "商务",
                "正式",
                "专业",
                "金融",
                "庄重",
                "克制",
                "business",
                "formal",
                "professional",
            ],
        );
        let minimal = contains_any(
            &corpus,
            &["简约", "极简", "清爽", "克制", "minimal", "clean"],
        );
        let mut semantic_style_axes = Vec::new();
        semantic_style_axes.push(if dark { "dark" } else { "light" }.to_string());
        if editorial {
            semantic_style_axes.push("editorial".to_string());
        }
        if technical {
            semantic_style_axes.push("technical".to_string());
        }
        if friendly {
            semantic_style_axes.push("friendly".to_string());
        }
        if formal {
            semantic_style_axes.push("formal".to_string());
        }
        if minimal {
            semantic_style_axes.push("minimal".to_string());
        }
        if semantic_style_axes.len() == 1 {
            semantic_style_axes.push("balanced".to_string());
        }

        let (title_font_family, body_font_family, display_font_family) = if editorial {
            (
                "Microsoft YaHei, Arial, sans-serif".to_string(),
                "Microsoft YaHei, Arial, sans-serif".to_string(),
                "SimSun, Georgia, serif".to_string(),
            )
        } else if technical {
            (
                "Microsoft YaHei, Arial, sans-serif".to_string(),
                "Microsoft YaHei, Arial, sans-serif".to_string(),
                "Consolas, Courier New, monospace".to_string(),
            )
        } else {
            let family = "Microsoft YaHei, Arial, sans-serif".to_string();
            (family.clone(), family.clone(), family)
        };
        let corner_radius = if technical {
            10
        } else if friendly {
            18
        } else if formal || editorial {
            6
        } else {
            12
        };
        let card_style = if minimal {
            "flat surface, one border hierarchy, no decorative empty cards"
        } else if dark {
            "layered dark surface with restrained highlight edge"
        } else {
            "solid surface with restrained border and semantic grouping"
        };
        let shadow_style = if minimal || technical {
            "none or one subtle depth level"
        } else {
            "one restrained soft shadow level"
        };
        let mut decorative_elements = split_visual_language(&theme.decoration_language);
        if decorative_elements.is_empty() {
            decorative_elements.push("theme-colored structural divider".to_string());
            decorative_elements.push("one repeated corner or edge motif".to_string());
        }

        Self {
            schema_version: NATIVE_DESIGN_SYSTEM_SCHEMA_VERSION,
            contract_name: format!("{}-cross-slide-design-system", theme.theme_name),
            source_style: style.trim().to_string(),
            source_custom_style: custom_style.to_string(),
            source_extra_requirements: extra_requirements.to_string(),
            source_visual_suggestions: visual_suggestions.to_string(),
            semantic_style_axes,
            color_system: NativeColorSystem {
                background_primary: theme.background_color.clone(),
                background_secondary: theme.secondary_background_color.clone(),
                surface_color: theme.surface_color.clone(),
                primary_color: theme.primary_color.clone(),
                secondary_color: theme.secondary_color.clone(),
                accent_color: theme.accent_color.clone(),
                text_primary: theme.text_primary.clone(),
                text_secondary: theme.text_secondary.clone(),
                border_color: theme.border_color.clone(),
                muted_color: theme.text_secondary.clone(),
                positive_color: "#16835B".to_string(),
                warning_color: "#B7791F".to_string(),
                negative_color: "#B42318".to_string(),
                forbidden_colors: theme.forbidden_colors.clone(),
            },
            typography_system: NativeTypographySystem {
                title_font_family,
                body_font_family,
                display_font_family,
                title_size_range: IntegerRange {
                    minimum: 32,
                    maximum: 76,
                },
                subtitle_size_range: IntegerRange {
                    minimum: 18,
                    maximum: 30,
                },
                body_size_range: IntegerRange {
                    minimum: 14,
                    maximum: 24,
                },
                label_size_range: IntegerRange {
                    minimum: 11,
                    maximum: 18,
                },
                number_size_range: IntegerRange {
                    minimum: 30,
                    maximum: 84,
                },
                font_weight_rules: vec![
                    "titles=700; subtitles=500; body=400; labels=500".to_string(),
                    "bold is reserved for the core message, explicit emphasis, or primary metric"
                        .to_string(),
                ],
                line_height_rules: vec![
                    "titles=1.08-1.18".to_string(),
                    "body=1.35-1.55".to_string(),
                    "labels=1.15-1.30".to_string(),
                ],
                maximum_font_families: 3,
            },
            grid_system: NativeGridSystem {
                canvas_safe_margin: 48,
                content_grid: "12-column 1280x720 canvas with a shared title band and footer-safe zone"
                    .to_string(),
                column_count: 12,
                gutter: 20,
                section_spacing: 28,
                element_spacing: 16,
                card_padding: 22,
                alignment_rules: vec![
                    "align major blocks to shared column lines".to_string(),
                    "keep one dominant alignment axis per page".to_string(),
                    "footer elements do not define the content grid".to_string(),
                ],
                page_density_range: IntegerRange {
                    minimum: 34,
                    maximum: 78,
                },
            },
            graphic_language: NativeGraphicLanguage {
                corner_radius,
                border_width: if technical { 1 } else { 2 },
                line_style: if technical {
                    "precise thin connectors with deliberate nodes"
                } else if editorial {
                    "editorial dividers with one repeated ornamental cadence"
                } else {
                    "clean semantic dividers and connectors"
                }
                .to_string(),
                card_style: card_style.to_string(),
                shadow_style: shadow_style.to_string(),
                shape_language: theme.shape_language.clone(),
                icon_style: "one coherent outline-or-solid family; never mix unrelated icon grammars"
                    .to_string(),
                chart_style: "theme palette, direct labels, shared stroke weight, and no default office-blue palette"
                    .to_string(),
                connector_style: "one arrowhead and node convention across timeline, process, and relationship pages"
                    .to_string(),
                decorative_elements,
                image_treatment: theme.image_treatment.clone(),
            },
            deck_rhythm: NativeDeckRhythm {
                cover_rhythm: "anchor".to_string(),
                body_rhythm: vec!["balanced".to_string(), "dense".to_string()],
                breathing_pages: vec![
                    "cover".to_string(),
                    "section".to_string(),
                    "quote".to_string(),
                ],
                dense_pages: vec![
                    "timeline".to_string(),
                    "process".to_string(),
                    "comparison".to_string(),
                    "data".to_string(),
                ],
                section_transition_strategy:
                    "change background emphasis or focal scale while preserving palette, grid, and graphic grammar"
                        .to_string(),
                layout_family_roster: vec![
                    "hero".to_string(),
                    "editorial_split".to_string(),
                    "timeline".to_string(),
                    "process".to_string(),
                    "comparison".to_string(),
                    "data_focus".to_string(),
                    "relationship".to_string(),
                    "profile".to_string(),
                    "quote_focus".to_string(),
                    "summary".to_string(),
                ],
                maximum_layout_repetition: 2,
                adjacent_page_difference:
                    "adjacent pages must differ in layout family, focal placement, or dominant visual—not only text or card count"
                        .to_string(),
                focal_point_strategy:
                    "one unmistakable primary visual per page, supported by no more than two secondary regions"
                        .to_string(),
            },
            content_hierarchy: NativeContentHierarchy {
                page_core_message: "one sentence that owns the page".to_string(),
                primary_visual: "one semantic visual encoding the core message".to_string(),
                supporting_visuals: vec![
                    "evidence or context region".to_string(),
                    "navigation or relationship structure".to_string(),
                ],
                headline_priority: "title, core claim, then section label".to_string(),
                body_priority: "short evidence blocks ordered by narrative importance".to_string(),
                evidence_priority: "sourced dates, facts, contrasts, and quotations before decoration"
                    .to_string(),
                maximum_content_units: 6,
                minimum_content_units: 2,
                emphasis_rules: vec![
                    "emphasize only the core claim, one metric, date, or explicit source emphasis"
                        .to_string(),
                    "do not enlarge every number or bold every sentence".to_string(),
                ],
            },
            forbiddens: NativeDesignForbiddens {
                forbidden_layout_patterns: vec![
                    "same title-plus-card-grid syntax on adjacent pages".to_string(),
                    "semantic mismatch between content and layout family".to_string(),
                    "small content cluster stranded in one corner".to_string(),
                ],
                forbidden_visual_patterns: theme
                    .forbidden_visual_patterns
                    .iter()
                    .cloned()
                    .chain([
                        "unrelated decorative language on a single page".to_string(),
                        "empty cards or meaningless rectangles used to fill space".to_string(),
                    ])
                    .collect(),
                forbidden_font_behavior: vec![
                    "more than the declared maximum font families".to_string(),
                    "body or label text below the declared minimum size".to_string(),
                    "page-local replacement of the deck typography family".to_string(),
                ],
                forbidden_color_behavior: vec![
                    "page-local reinterpretation of the primary palette".to_string(),
                    "default technology blue when it is not in the contract".to_string(),
                    "theme contract field names rendered as visible text".to_string(),
                ],
                forbidden_density_behavior: vec![
                    "non-functional dead whitespace on balanced or dense pages".to_string(),
                    "all pages forced to the same density".to_string(),
                    "tiny type used to simulate information density".to_string(),
                ],
                forbidden_repetition: vec![
                    "adjacent pages differ only by wording, color, or card count".to_string(),
                    "same layout family exceeds maximumLayoutRepetition".to_string(),
                ],
            },
            page_contracts: Vec::new(),
        }
    }

    pub(super) fn assign_page_contracts(&mut self, plan: &SlidePlan) {
        self.page_contracts.clear();
        let mut previous_family = String::new();
        let mut family_use = BTreeMap::<String, usize>::new();
        for slide in &plan.slides {
            let units = content_units(slide);
            let preferred = semantic_layout_family(slide, units);
            let layout_family = choose_non_repeating_family(
                slide,
                units,
                preferred,
                &previous_family,
                &family_use,
                self.deck_rhythm.maximum_layout_repetition,
            );
            let count = family_use.entry(layout_family.clone()).or_default();
            *count += 1;
            let page_rhythm = semantic_page_rhythm(slide, units);
            let content_density = match units {
                0..=2 => "low",
                3..=4 => "medium",
                _ => "high",
            };
            let (focal_point, dominant_visual_type) = focal_contract(&layout_family);
            let difference_from_previous = if previous_family.is_empty() {
                "establish the deck's visual anchor and recurring design language".to_string()
            } else {
                format!(
                    "change dominant structure from {previous_family} to {layout_family} while preserving palette, typography, grid, spacing, and graphic language"
                )
            };
            let mut forbidden_patterns = vec![
                "do not reinterpret palette, typography, corner radius, line language, or spacing"
                    .to_string(),
                "do not add empty cards, repeated copy, unsupported facts, or template labels"
                    .to_string(),
            ];
            if !previous_family.is_empty() {
                forbidden_patterns.push(format!(
                    "do not repeat the previous page's {previous_family} composition"
                ));
            }
            self.page_contracts.push(NativePageDesignContract {
                page_index: slide.page,
                narrative_role: slide.page_theme.clone(),
                page_rhythm: page_rhythm.to_string(),
                layout_family: layout_family.clone(),
                content_density: content_density.to_string(),
                focal_point: focal_point.to_string(),
                dominant_visual_type: dominant_visual_type.to_string(),
                difference_from_previous,
                consistency_requirements: vec![
                    "use the exact deck color system and typography families".to_string(),
                    "align major regions to the shared 12-column grid and safe margin".to_string(),
                    "reuse the declared line, card, connector, and decoration grammar".to_string(),
                ],
                forbidden_patterns,
            });
            previous_family = layout_family;
        }
    }

    pub(super) fn page_contract(&self, page: usize) -> Option<&NativePageDesignContract> {
        self.page_contracts
            .iter()
            .find(|contract| contract.page_index == page)
    }

    pub(super) fn base_contract_eq(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.page_contracts.clear();
        right.page_contracts.clear();
        left == right
    }

    pub(super) fn prompt_contract(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub(super) fn planning_contract(&self) -> String {
        let mut contract = self.clone();
        contract.page_contracts.clear();
        contract.prompt_contract()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeDesignConsistencyIssue {
    pub page_number: Option<usize>,
    pub rule: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativePageDesignObservation {
    pub page_number: usize,
    pub svg_path: String,
    pub top_colors: Vec<(String, usize)>,
    pub background_color: Option<String>,
    pub font_families: Vec<String>,
    pub minimum_font_size: Option<u32>,
    pub maximum_font_size: Option<u32>,
    pub text_blocks: usize,
    pub substantive_graphics: usize,
    pub rounded_rectangles: usize,
    pub dominant_corner_radius: Option<u32>,
    pub dominant_stroke_width: Option<u32>,
    pub grid_violation_count: usize,
    pub typography_violation_count: usize,
    pub layout_family: String,
    pub page_rhythm: String,
    pub structural_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeDeckDesignConsistencyReport {
    pub schema_version: u32,
    pub passed: bool,
    pub project_path: String,
    pub pages: Vec<NativePageDesignObservation>,
    pub issues: Vec<NativeDesignConsistencyIssue>,
}

impl NativeDeckDesignConsistencyReport {
    pub(super) fn failed_pages(&self) -> Vec<usize> {
        self.issues
            .iter()
            .filter_map(|issue| issue.page_number)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(super) fn page_summary(&self, page: usize) -> String {
        self.issues
            .iter()
            .filter(|issue| issue.page_number == Some(page))
            .map(|issue| format!("{}: {}", issue.rule, issue.summary))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

pub(super) fn persist_design_system_spec(
    project: &Path,
    spec: &NativeDesignSystemSpec,
) -> Result<PathBuf, String> {
    write_json_atomic(project, NATIVE_DESIGN_SYSTEM_SPEC_FILE, spec)
}

pub(super) fn validate_deck_design_consistency(
    project: &Path,
    spec: &NativeDesignSystemSpec,
) -> Result<NativeDeckDesignConsistencyReport, String> {
    let mut pages = Vec::new();
    let mut issues = Vec::new();
    let mut previous_signature = String::new();
    let mut previous_family = String::new();
    let mut dense_pages = 0usize;
    let declared_fonts = declared_font_heads(spec);
    for contract in &spec.page_contracts {
        let svg_path = find_page_svg(project, contract.page_index)?;
        let svg = fs::read_to_string(&svg_path)
            .map_err(|error| format!("read native SVG failed: {} ({error})", svg_path.display()))?;
        let observation = observe_svg(&svg_path, &svg, contract, spec);
        if contract.page_rhythm == "dense" {
            dense_pages += 1;
        }
        let theme_uses = spec_theme_uses(&observation.top_colors, spec);
        if theme_uses == 0 {
            issues.push(issue(
                contract.page_index,
                "design_system_color_drift",
                "page does not use the declared primary, secondary, or accent color",
            ));
        }
        let forbidden_uses = forbidden_color_uses(&observation.top_colors, spec);
        if forbidden_uses > 0 {
            issues.push(issue(
                contract.page_index,
                "design_system_forbidden_color",
                format!("forbidden color uses={forbidden_uses}"),
            ));
        }
        let unknown_fonts = observation
            .font_families
            .iter()
            .filter(|family| !declared_fonts.contains(&font_head(family)))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown_fonts.is_empty()
            || observation.font_families.len() > spec.typography_system.maximum_font_families
        {
            issues.push(issue(
                contract.page_index,
                "design_system_typography_drift",
                format!(
                    "fonts={:?}, unknown={:?}, maximum={}",
                    observation.font_families,
                    unknown_fonts,
                    spec.typography_system.maximum_font_families
                ),
            ));
        }
        if observation.typography_violation_count > 0 {
            issues.push(issue(
                contract.page_index,
                "design_system_type_scale_violation",
                format!(
                    "{} visible text blocks are outside their declared role size range",
                    observation.typography_violation_count
                ),
            ));
        }
        if observation.grid_violation_count > 0 {
            issues.push(issue(
                contract.page_index,
                "design_system_grid_margin_violation",
                format!(
                    "{} declared non-footer text regions are outside the shared safe margin",
                    observation.grid_violation_count
                ),
            ));
        }
        if !background_follows_strategy(svg.as_str(), observation.background_color.as_deref(), spec)
        {
            issues.push(issue(
                contract.page_index,
                "design_system_background_strategy_drift",
                format!(
                    "page background {:?} is outside the declared background strategy",
                    observation.background_color
                ),
            ));
        }
        let needs_support = matches!(contract.page_rhythm.as_str(), "balanced" | "dense");
        if needs_support && observation.text_blocks < 3 {
            issues.push(issue(
                contract.page_index,
                "design_system_missing_focal_or_support",
                format!(
                    "rhythm={}, textBlocks={}, substantiveGraphics={}",
                    contract.page_rhythm, observation.text_blocks, observation.substantive_graphics
                ),
            ));
        }
        if !previous_family.is_empty()
            && previous_family == contract.layout_family
            && previous_signature == observation.structural_signature
        {
            issues.push(issue(
                contract.page_index,
                "design_system_adjacent_layout_repetition",
                format!(
                    "layoutFamily={} and structuralSignature={} repeat the previous page",
                    contract.layout_family, observation.structural_signature
                ),
            ));
        }
        if let Some(space_issue) = density_report_issue(project, contract.page_index, contract) {
            issues.push(space_issue);
        }
        previous_family = contract.layout_family.clone();
        previous_signature = observation.structural_signature.clone();
        pages.push(observation);
    }
    let shared_corner_radius = mode(
        &pages
            .iter()
            .filter_map(|page| page.dominant_corner_radius)
            .collect::<Vec<_>>(),
    );
    let shared_stroke_width = mode(
        &pages
            .iter()
            .filter_map(|page| page.dominant_stroke_width)
            .collect::<Vec<_>>(),
    );
    for page in &pages {
        if let (Some(shared), Some(actual)) = (shared_corner_radius, page.dominant_corner_radius) {
            if shared.abs_diff(actual) > 4 {
                issues.push(issue(
                    page.page_number,
                    "design_system_corner_radius_drift",
                    format!("dominantCornerRadius={actual}, deckDominant={shared}"),
                ));
            }
        }
        if let (Some(shared), Some(actual)) = (shared_stroke_width, page.dominant_stroke_width) {
            if shared.abs_diff(actual) > 1 {
                issues.push(issue(
                    page.page_number,
                    "design_system_line_weight_drift",
                    format!("dominantStrokeWidth={actual}, deckDominant={shared}"),
                ));
            }
        }
    }
    if !pages.is_empty() && dense_pages == pages.len() {
        issues.push(NativeDesignConsistencyIssue {
            page_number: None,
            rule: "design_system_all_pages_dense".to_string(),
            summary: "every page uses dense rhythm; the deck has no breathing or anchor page"
                .to_string(),
        });
    }
    let report = NativeDeckDesignConsistencyReport {
        schema_version: 1,
        passed: issues.is_empty(),
        project_path: project.to_string_lossy().to_string(),
        pages,
        issues,
    };
    let analysis = project.join("analysis");
    fs::create_dir_all(&analysis).map_err(|error| {
        format!(
            "create native design analysis directory failed: {} ({error})",
            analysis.display()
        )
    })?;
    write_json_atomic(&analysis, NATIVE_DESIGN_CONSISTENCY_REPORT_FILE, &report)?;
    Ok(report)
}

fn semantic_layout_family(slide: &Slide, units: usize) -> &'static str {
    match slide.slide_type.as_str() {
        "cover" => "hero",
        "section" => "section",
        "timeline" => "timeline",
        "process" => "process",
        "comparison" => "comparison",
        "data" => "data_focus",
        "quote" => "quote_focus",
        "image" => "image_focus",
        "profile" => "profile",
        "summary" => "summary",
        _ if slide.layout == "timeline" => "timeline",
        _ if slide.layout == "process" => "process",
        _ if matches!(slide.layout.as_str(), "compare" | "comparison") => "comparison",
        _ if units <= 2 => "dominant_statement",
        _ if units == 3 => "editorial_split",
        _ => "relationship",
    }
}

fn choose_non_repeating_family(
    slide: &Slide,
    units: usize,
    preferred: &str,
    previous: &str,
    family_use: &BTreeMap<String, usize>,
    maximum_repetition: usize,
) -> String {
    let used = family_use.get(preferred).copied().unwrap_or_default();
    if preferred != previous && used < maximum_repetition {
        return preferred.to_string();
    }
    let alternatives: &[&str] = match slide.slide_type.as_str() {
        "timeline" => &["timeline", "staged_path"],
        "process" => &["process", "relationship"],
        "comparison" => &["comparison", "editorial_split"],
        "data" => &["data_focus", "editorial_split"],
        "summary" => &["summary", "dominant_statement"],
        "profile" => &["profile", "editorial_split"],
        _ if units <= 2 => &["dominant_statement", "quote_focus", "editorial_split"],
        _ if units == 3 => &["editorial_split", "staged_path", "relationship"],
        _ => &["relationship", "editorial_split", "matrix"],
    };
    alternatives
        .iter()
        .find(|candidate| {
            **candidate != previous
                && family_use.get(**candidate).copied().unwrap_or_default() < maximum_repetition
        })
        .copied()
        .unwrap_or(preferred)
        .to_string()
}

fn semantic_page_rhythm(slide: &Slide, units: usize) -> &'static str {
    if matches!(slide.slide_type.as_str(), "cover" | "quote") {
        "anchor"
    } else if slide.slide_type == "section" {
        "breathing"
    } else if matches!(
        slide.slide_type.as_str(),
        "timeline" | "process" | "comparison" | "data"
    ) && units >= 4
    {
        "dense"
    } else {
        "balanced"
    }
}

fn focal_contract(layout_family: &str) -> (&'static str, &'static str) {
    match layout_family {
        "hero" => (
            "center-left title anchor",
            "hero statement and background structure",
        ),
        "section" => ("single central transition anchor", "section marker"),
        "timeline" | "staged_path" => ("central narrative path", "timeline or staged path"),
        "process" => ("directional center flow", "process flow"),
        "comparison" => ("balanced left-right contrast", "two-sided comparison"),
        "data_focus" => ("dominant metric or chart", "data visualization"),
        "quote_focus" => ("oversized quotation anchor", "quotation"),
        "image_focus" => ("dominant image region", "image with narrative annotation"),
        "profile" => ("portrait-or-name anchor with evidence rail", "profile"),
        "relationship" => (
            "central subject with linked evidence",
            "relationship diagram",
        ),
        "matrix" => ("balanced matrix center", "semantic matrix"),
        "summary" => ("synthesis statement plus evidence arc", "summary synthesis"),
        _ => (
            "dominant claim with one support rail",
            "editorial hierarchy",
        ),
    }
}

fn content_units(slide: &Slide) -> usize {
    slide
        .content_blocks
        .len()
        .max(slide.must_include.len())
        .max(slide.bullets.len())
}

fn split_visual_language(value: &str) -> Vec<String> {
    value
        .split(|character| matches!(character, '、' | '，' | ',' | ';' | '；'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take(5)
        .map(str::to_string)
        .collect()
}

fn color_luminance(color: &str) -> f64 {
    let value = color.trim().trim_start_matches('#');
    if value.len() != 6 {
        return 0.5;
    }
    let component = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&value[range], 16).unwrap_or(128) as f64 / 255.0
    };
    0.2126 * component(0..2) + 0.7152 * component(2..4) + 0.0722 * component(4..6)
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn find_page_svg(project: &Path, page: usize) -> Result<PathBuf, String> {
    let directory = project.join("svg_output");
    let prefix = format!("{page:02}_");
    let mut matches = fs::read_dir(&directory)
        .map_err(|error| format!("read svg_output failed: {} ({error})", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|value| value.to_str()) == Some("svg")
                && path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect::<Vec<_>>();
    matches.sort();
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format!("native SVG for P{page:02} is missing")),
        _ => Err(format!(
            "multiple native SVG files found for P{page:02}: {:?}",
            matches
        )),
    }
}

fn observe_svg(
    svg_path: &Path,
    svg: &str,
    contract: &NativePageDesignContract,
    spec: &NativeDesignSystemSpec,
) -> NativePageDesignObservation {
    let colors = color_attribute_regex()
        .captures_iter(svg)
        .filter_map(|capture| capture.get(1))
        .map(|value| value.as_str().to_ascii_uppercase())
        .fold(BTreeMap::<String, usize>::new(), |mut counts, color| {
            *counts.entry(color).or_default() += 1;
            counts
        });
    let mut top_colors = colors.into_iter().collect::<Vec<_>>();
    top_colors.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    top_colors.truncate(10);
    let font_families = font_family_regex()
        .captures_iter(svg)
        .filter_map(|capture| capture.get(1))
        .map(|value| value.as_str().trim().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let font_sizes = font_size_regex()
        .captures_iter(svg)
        .filter_map(|capture| capture.get(1))
        .filter_map(|value| value.as_str().parse::<f64>().ok())
        .map(|value| value.round().max(0.0) as u32)
        .collect::<Vec<_>>();
    let text_tags = text_tag_regex().captures_iter(svg).collect::<Vec<_>>();
    let text_blocks = text_tags.len();
    let mut grid_violation_count = 0usize;
    let mut typography_violation_count = 0usize;
    let safe = spec.grid_system.canvas_safe_margin.saturating_sub(8) as f64;
    for tag in &text_tags {
        let Some(attributes) = tag.get(1) else {
            continue;
        };
        let attributes = attribute_map(attributes.as_str());
        let role = attributes
            .get("data-pome-role")
            .map(String::as_str)
            .unwrap_or("");
        if role != "footer" {
            let region = [
                attributes.get("data-pome-region-x"),
                attributes.get("data-pome-region-y"),
                attributes.get("data-pome-region-width"),
                attributes.get("data-pome-region-height"),
            ]
            .map(|value| value.and_then(|value| value.parse::<f64>().ok()));
            if let [Some(x), Some(y), Some(width), Some(height)] = region {
                let region_outside = x < safe
                    || y < safe
                    || x + width > 1280.0 - safe
                    || y + height > 720.0 - safe;
                let anchor_outside = attributes
                    .get("x")
                    .and_then(|value| value.parse::<f64>().ok())
                    .zip(
                        attributes
                            .get("y")
                            .and_then(|value| value.parse::<f64>().ok()),
                    )
                    .is_none_or(|(text_x, text_y)| {
                        text_x < safe
                            || text_x > 1280.0 - safe
                            || text_y < safe
                            || text_y > 720.0 - safe
                    });
                if region_outside && anchor_outside {
                    grid_violation_count += 1;
                }
            }
        }
        if let Some(size) = attributes
            .get("font-size")
            .and_then(|value| value.parse::<f64>().ok())
            .map(|value| value.round().max(0.0) as u32)
        {
            let range = match role {
                "title" => Some(&spec.typography_system.title_size_range),
                "subtitle" => Some(&spec.typography_system.subtitle_size_range),
                "body" => Some(&spec.typography_system.body_size_range),
                "caption" | "label" | "footer" | "unit" => {
                    Some(&spec.typography_system.label_size_range)
                }
                "metric" => Some(&spec.typography_system.number_size_range),
                _ => None,
            };
            let local_minimum = attributes
                .get("data-pome-min-font-size")
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| value.round().max(0.0) as u32);
            if range.is_some_and(|range| {
                size < local_minimum.unwrap_or(range.minimum) || size > range.maximum
            }) {
                typography_violation_count += 1;
            }
        }
    }
    let rectangles = rect_tag_regex().captures_iter(svg).collect::<Vec<_>>();
    let mut corner_radii = Vec::new();
    let mut background_color = None;
    for rectangle in &rectangles {
        let Some(attributes) = rectangle.get(1) else {
            continue;
        };
        let attributes = attribute_map(attributes.as_str());
        let width = attributes
            .get("width")
            .and_then(|value| value.parse::<f64>().ok());
        let height = attributes
            .get("height")
            .and_then(|value| value.parse::<f64>().ok());
        let is_background = attributes
            .get("x")
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|value| value.abs() <= 1.0)
            && attributes
                .get("y")
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|value| value.abs() <= 1.0)
            && attributes
                .get("width")
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|value| value >= 1279.0)
            && attributes
                .get("height")
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|value| value >= 719.0);
        if is_background && background_color.is_none() {
            background_color = attributes
                .get("fill")
                .map(|value| value.to_ascii_uppercase());
        }
        if !is_background
            && width.is_some_and(|value| value >= 120.0)
            && height.is_some_and(|value| value >= 40.0)
        {
            if let Some(radius) = attributes
                .get("rx")
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| value.round().max(0.0) as u32)
            {
                corner_radii.push(radius);
            }
        }
    }
    let rounded_rectangles = rectangles
        .iter()
        .filter(|capture| {
            capture
                .get(1)
                .is_some_and(|attrs| rx_regex().is_match(attrs.as_str()))
        })
        .count();
    let shape_count = rectangles.len()
        + circle_regex().find_iter(svg).count()
        + path_regex().find_iter(svg).count()
        + line_regex().find_iter(svg).count();
    let substantive_graphics = shape_count.saturating_sub(1);
    let stroke_widths = stroke_width_regex()
        .captures_iter(svg)
        .filter_map(|capture| capture.get(1))
        .filter_map(|value| value.as_str().parse::<f64>().ok())
        .map(|value| value.round().max(0.0) as u32)
        .collect::<Vec<_>>();
    let dominant_stroke_width = mode(&stroke_widths);
    let structural_signature = format!(
        "t{}-r{}-c{}-p{}-l{}",
        bucket(text_blocks),
        bucket(rectangles.len()),
        bucket(circle_regex().find_iter(svg).count()),
        bucket(path_regex().find_iter(svg).count()),
        bucket(line_regex().find_iter(svg).count())
    );
    NativePageDesignObservation {
        page_number: contract.page_index,
        svg_path: svg_path.to_string_lossy().to_string(),
        top_colors,
        background_color,
        font_families,
        minimum_font_size: font_sizes.iter().copied().min(),
        maximum_font_size: font_sizes.iter().copied().max(),
        text_blocks,
        substantive_graphics,
        rounded_rectangles,
        dominant_corner_radius: mode(&corner_radii),
        dominant_stroke_width,
        grid_violation_count,
        typography_violation_count,
        layout_family: contract.layout_family.clone(),
        page_rhythm: contract.page_rhythm.clone(),
        structural_signature,
    }
}

fn declared_font_heads(spec: &NativeDesignSystemSpec) -> BTreeSet<String> {
    [
        &spec.typography_system.title_font_family,
        &spec.typography_system.body_font_family,
        &spec.typography_system.display_font_family,
    ]
    .into_iter()
    .map(|family| font_head(family))
    .collect()
}

fn font_head(value: &str) -> String {
    value
        .split(',')
        .next()
        .unwrap_or(value)
        .trim()
        .trim_matches(['\'', '"'])
        .to_ascii_lowercase()
}

fn spec_theme_uses(colors: &[(String, usize)], spec: &NativeDesignSystemSpec) -> usize {
    let allowed = [
        &spec.color_system.primary_color,
        &spec.color_system.secondary_color,
        &spec.color_system.accent_color,
    ]
    .into_iter()
    .map(|color| color.to_ascii_uppercase())
    .collect::<BTreeSet<_>>();
    colors
        .iter()
        .filter(|(color, _)| allowed.contains(color))
        .map(|(_, count)| count)
        .sum()
}

fn forbidden_color_uses(colors: &[(String, usize)], spec: &NativeDesignSystemSpec) -> usize {
    let forbidden = spec
        .color_system
        .forbidden_colors
        .iter()
        .map(|color| color.to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    colors
        .iter()
        .filter(|(color, _)| forbidden.contains(color))
        .map(|(_, count)| count)
        .sum()
}

fn background_follows_strategy(
    svg: &str,
    background: Option<&str>,
    spec: &NativeDesignSystemSpec,
) -> bool {
    let Some(background) = background else {
        return true;
    };
    let allowed = [
        &spec.color_system.background_primary,
        &spec.color_system.background_secondary,
        &spec.color_system.surface_color,
        &spec.color_system.primary_color,
        &spec.color_system.secondary_color,
        &spec.color_system.accent_color,
    ]
    .into_iter()
    .map(|color| color.to_ascii_uppercase())
    .collect::<BTreeSet<_>>();
    let normalized = background.trim().to_ascii_uppercase();
    if allowed.contains(&normalized) {
        return true;
    }
    let Some(gradient_id) = normalized
        .strip_prefix("URL(#")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    gradient_tag_regex().captures_iter(svg).any(|capture| {
        let Some(id) = capture.get(1) else {
            return false;
        };
        if !id.as_str().eq_ignore_ascii_case(gradient_id) {
            return false;
        }
        let Some(body) = capture.get(2) else {
            return false;
        };
        let colors = color_attribute_regex()
            .captures_iter(body.as_str())
            .filter_map(|capture| capture.get(1))
            .map(|color| color.as_str().to_ascii_uppercase())
            .collect::<Vec<_>>();
        !colors.is_empty() && colors.iter().all(|color| allowed.contains(color))
    })
}

fn density_report_issue(
    project: &Path,
    page: usize,
    contract: &NativePageDesignContract,
) -> Option<NativeDesignConsistencyIssue> {
    let path = project
        .join("analysis")
        .join("native_space_utilization")
        .join(format!("P{page:02}.json"));
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let passed = value.get("passed").and_then(serde_json::Value::as_bool)?;
    if passed || matches!(contract.page_rhythm.as_str(), "anchor" | "breathing") {
        return None;
    }
    Some(issue(
        page,
        "design_system_dead_whitespace",
        format!(
            "space utilization report failed for pageRhythm={}; largestEmptyInformationRegion={}",
            contract.page_rhythm,
            value
                .get("largestEmptyInformationRegion")
                .cloned()
                .unwrap_or_default()
        ),
    ))
}

fn issue(
    page: usize,
    rule: impl Into<String>,
    summary: impl Into<String>,
) -> NativeDesignConsistencyIssue {
    NativeDesignConsistencyIssue {
        page_number: Some(page),
        rule: rule.into(),
        summary: summary.into(),
    }
}

fn bucket(value: usize) -> usize {
    match value {
        0..=2 => 0,
        3..=7 => 1,
        8..=15 => 2,
        16..=27 => 3,
        _ => 4,
    }
}

fn mode(values: &[u32]) -> Option<u32> {
    values
        .iter()
        .copied()
        .fold(BTreeMap::<u32, usize>::new(), |mut counts, value| {
            *counts.entry(value).or_default() += 1;
            counts
        })
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(value, _)| value)
}

fn write_json_atomic<T: Serialize>(
    directory: &Path,
    file_name: &str,
    value: &T,
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|error| {
        format!(
            "create design system directory failed: {} ({error})",
            directory.display()
        )
    })?;
    let path = directory.join(file_name);
    let temp = directory.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| format!("serialize {file_name} failed: {error}"))?;
    fs::write(&temp, format!("{json}\n"))
        .map_err(|error| format!("write {file_name} failed: {} ({error})", temp.display()))?;
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("replace {file_name} failed: {} ({error})", path.display()))?;
    }
    fs::rename(&temp, &path).map_err(|error| {
        format!(
            "commit {file_name} failed: {} -> {} ({error})",
            temp.display(),
            path.display()
        )
    })?;
    Ok(path)
}

fn color_attribute_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:fill|stroke|stop-color)\s*=\s*["'](#[0-9a-f]{6})["']"#)
            .expect("valid regex")
    })
}

fn font_family_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\bfont-family\s*=\s*["']([^"']+)["']"#).expect("valid regex")
    })
}

fn font_size_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\bfont-size\s*=\s*["']([0-9]+(?:\.[0-9]+)?)["']"#).expect("valid regex")
    })
}

fn stroke_width_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\bstroke-width\s*=\s*["']([0-9]+(?:\.[0-9]+)?)["']"#)
            .expect("valid regex")
    })
}

fn rx_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\brx\s*=\s*["'][0-9.]"#).expect("valid regex"))
}

fn text_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<text\b([^>]*)>").expect("valid regex"))
}

fn attribute_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)([a-z_][a-z0-9_:-]*)\s*=\s*["']([^"']*)["']"#).expect("valid regex")
    })
}

fn attribute_map(attributes: &str) -> BTreeMap<String, String> {
    attribute_regex()
        .captures_iter(attributes)
        .filter_map(|capture| Some((capture.get(1)?.as_str(), capture.get(2)?.as_str())))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_string()))
        .collect()
}

fn rect_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<rect\b([^>]*)>").expect("valid regex"))
}

fn gradient_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)<(?:linearGradient|radialGradient)\b[^>]*\bid\s*=\s*["']([^"']+)["'][^>]*>(.*?)</(?:linearGradient|radialGradient)>"#,
        )
        .expect("valid regex")
    })
}

fn circle_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<circle\b").expect("valid regex"))
}

fn path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<path\b").expect("valid regex"))
}

fn line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<line\b").expect("valid regex"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ContentBlock, Theme, ThemeAllocation};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn theme(style: &str) -> NativeThemeSpec {
        NativeThemeSpec::from_inputs(style, Some(style), None, None)
    }

    fn slide(page: usize, slide_type: &str, layout: &str, units: usize) -> Slide {
        Slide {
            page,
            page_index: page,
            page_id: format!("P{page:02}"),
            slide_type: slide_type.to_string(),
            layout: layout.to_string(),
            title: format!("Page {page}"),
            subtitle: String::new(),
            bullets: (0..units).map(|index| format!("Fact {index}")).collect(),
            visual_hint: String::new(),
            page_theme: format!("Role {page}"),
            main_claim: format!("Claim {page}"),
            core_message: format!("Claim {page}"),
            content_scope: format!("Scope {page}"),
            content_blocks: (0..units)
                .map(|index| ContentBlock {
                    label: format!("L{index}"),
                    text: format!("Fact {index}"),
                    detail: String::new(),
                })
                .collect(),
            evidence: vec!["Evidence".to_string()],
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

    fn plan(slides: Vec<Slide>) -> SlidePlan {
        SlidePlan {
            title: "Deck".to_string(),
            subtitle: String::new(),
            audience: String::new(),
            style: "custom".to_string(),
            theme: Theme {
                name: "test".to_string(),
                primary: "#334155".to_string(),
                secondary: "#64748B".to_string(),
                accent: "#C07A35".to_string(),
                background: "#FAFAF8".to_string(),
            },
            theme_allocation: Vec::<ThemeAllocation>::new(),
            slides,
        }
    }

    fn temp_project(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let project = std::env::temp_dir().join(format!(
            "pomegranate-design-system-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(project.join("svg_output")).expect("create svg_output");
        project
    }

    fn valid_svg(background: &str, primary: &str, extra_shapes: usize) -> String {
        let shapes = (0..extra_shapes)
            .map(|index| {
                format!(
                    r#"<rect x="{}" y="420" width="120" height="80" rx="6" fill="{}" stroke="{}" stroke-width="2"/>"#,
                    80 + index * 150,
                    background,
                    primary
                )
            })
            .collect::<String>();
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="{background}"/>
<line x1="48" y1="120" x2="1232" y2="120" stroke="{primary}" stroke-width="2"/>
<text x="56" y="88" font-family="Microsoft YaHei, Arial, sans-serif" font-size="42" text-anchor="start" data-pome-role="title" data-pome-region-id="title" data-pome-region-x="48" data-pome-region-y="48" data-pome-region-width="760" data-pome-region-height="60">Title</text>
<text x="56" y="200" font-family="Microsoft YaHei, Arial, sans-serif" font-size="16" text-anchor="start" data-pome-role="body" data-pome-region-id="body-1" data-pome-region-x="48" data-pome-region-y="160" data-pome-region-width="520" data-pome-region-height="80">Fact one</text>
<text x="664" y="200" font-family="Microsoft YaHei, Arial, sans-serif" font-size="16" text-anchor="start" data-pome-role="body" data-pome-region-id="body-2" data-pome-region-x="656" data-pome-region-y="160" data-pome-region-width="520" data-pome-region-height="80">Fact two</text>
{shapes}</svg>"##
        )
    }

    #[test]
    fn every_style_uses_the_same_complete_contract_shape() {
        for style in ["科技蓝", "红色情怀", "商务简约", "教育清新", "有文化感"] {
            let spec = NativeDesignSystemSpec::from_inputs(
                &theme(style),
                style,
                Some(style),
                Some("统一而完整"),
                None,
            );
            assert_eq!(spec.schema_version, 1);
            assert_eq!(spec.grid_system.column_count, 12);
            assert!(!spec.color_system.primary_color.is_empty());
            assert!(!spec.typography_system.body_font_family.is_empty());
            assert!(!spec.graphic_language.shape_language.is_empty());
            assert_eq!(spec.typography_system.maximum_font_families, 3);
        }
    }

    #[test]
    fn vague_style_gets_theme_independent_safe_defaults() {
        let spec =
            NativeDesignSystemSpec::from_inputs(&theme("高级一点"), "高级一点", None, None, None);
        assert_eq!(spec.grid_system.canvas_safe_margin, 48);
        assert_eq!(spec.typography_system.body_size_range.minimum, 14);
        assert_eq!(spec.deck_rhythm.maximum_layout_repetition, 2);
        assert!(!spec.contract_name.contains("tech-blue"));
    }

    #[test]
    fn page_contracts_prevent_adjacent_card_syntax_repetition() {
        let mut spec =
            NativeDesignSystemSpec::from_inputs(&theme("商务简约"), "商务简约", None, None, None);
        spec.assign_page_contracts(&plan(vec![
            slide(1, "cover", "cover", 2),
            slide(2, "content", "cards", 4),
            slide(3, "content", "cards", 4),
            slide(4, "content", "cards", 4),
        ]));
        assert_eq!(spec.page_contracts.len(), 4);
        for pair in spec.page_contracts.windows(2) {
            assert_ne!(pair[0].layout_family, pair[1].layout_family);
        }
    }

    #[test]
    fn planning_equality_ignores_deterministic_page_assignment_only() {
        let mut left =
            NativeDesignSystemSpec::from_inputs(&theme("教育清新"), "教育清新", None, None, None);
        let right = left.clone();
        left.assign_page_contracts(&plan(vec![slide(1, "cover", "cover", 2)]));
        assert!(left.base_contract_eq(&right));
    }

    #[test]
    fn prompt_contract_contains_every_deck_system_and_page_assignment() {
        let mut spec = NativeDesignSystemSpec::from_inputs(
            &theme("雅致紫色文化感"),
            "自定义",
            Some("雅致紫色文化感"),
            Some("克制、统一、有文化气质"),
            None,
        );
        spec.assign_page_contracts(&plan(vec![
            slide(1, "cover", "cover", 2),
            slide(2, "timeline", "timeline", 4),
        ]));
        let contract = spec.prompt_contract();
        for required in [
            "colorSystem",
            "typographySystem",
            "gridSystem",
            "graphicLanguage",
            "deckRhythm",
            "contentHierarchy",
            "forbiddens",
            "pageContracts",
            "layoutFamily",
            "differenceFromPrevious",
        ] {
            assert!(
                contract.contains(required),
                "missing contract field: {required}"
            );
        }
    }

    #[test]
    fn multiple_issues_on_one_page_schedule_only_one_page_redo() {
        let report = NativeDeckDesignConsistencyReport {
            schema_version: 1,
            passed: false,
            project_path: "test".to_string(),
            pages: Vec::new(),
            issues: vec![
                issue(2, "design_system_color_drift", "color"),
                issue(2, "design_system_typography_drift", "font"),
                issue(3, "design_system_grid_margin_violation", "grid"),
            ],
        };
        assert_eq!(report.failed_pages(), vec![2, 3]);
        assert!(report.page_summary(2).contains("design_system_color_drift"));
        assert!(report
            .page_summary(2)
            .contains("design_system_typography_drift"));
    }

    #[test]
    fn deck_checker_accepts_shared_palette_fonts_grid_and_graphic_language() {
        let project = temp_project("valid");
        let theme = theme("商务简约");
        let mut spec =
            NativeDesignSystemSpec::from_inputs(&theme, "商务简约", Some("商务简约"), None, None);
        spec.assign_page_contracts(&plan(vec![
            slide(1, "cover", "cover", 2),
            slide(2, "content", "cards", 4),
        ]));
        fs::write(
            project.join("svg_output/01_cover.svg"),
            valid_svg(
                &spec.color_system.background_primary,
                &spec.color_system.primary_color,
                2,
            ),
        )
        .unwrap();
        fs::write(
            project.join("svg_output/02_content.svg"),
            valid_svg(
                &spec.color_system.background_secondary,
                &spec.color_system.primary_color,
                5,
            ),
        )
        .unwrap();
        let report = validate_deck_design_consistency(&project, &spec).unwrap();
        assert!(report.passed, "{:?}", report.issues);
        assert_eq!(report.pages.len(), 2);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn observation_respects_local_text_minimums_theme_gradients_and_card_radius() {
        let theme = theme("科技蓝");
        let mut spec =
            NativeDesignSystemSpec::from_inputs(&theme, "科技蓝", Some("科技蓝"), None, None);
        spec.assign_page_contracts(&plan(vec![slide(1, "cover", "cover", 2)]));
        let contract = &spec.page_contracts[0];
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<defs><linearGradient id="bgGrad"><stop stop-color="{}"/><stop stop-color="{}"/></linearGradient></defs>
<rect x="0" y="0" width="1280" height="720" fill="url(#bgGrad)"/>
<rect x="48" y="180" width="420" height="220" rx="10" fill="{}" stroke="{}" stroke-width="1"/>
<rect x="96" y="240" width="40" height="120" rx="3" fill="{}" data-pome-visual-role="decoration"/>
<text x="56" y="88" font-family="Microsoft YaHei, Arial, sans-serif" font-size="16" data-pome-role="subtitle" data-pome-min-font-size="16" data-pome-region-x="0" data-pome-region-y="48" data-pome-region-width="600" data-pome-region-height="48">Subtitle</text>
<text x="64" y="230" font-family="Microsoft YaHei, Arial, sans-serif" font-size="11" data-pome-role="body" data-pome-min-font-size="11" data-pome-region-x="48" data-pome-region-y="200" data-pome-region-width="360" data-pome-region-height="40">Evidence</text>
</svg>"##,
            spec.color_system.background_primary,
            spec.color_system.background_secondary,
            spec.color_system.surface_color,
            spec.color_system.primary_color,
            spec.color_system.secondary_color,
        );
        let observation = observe_svg(Path::new("01.svg"), &svg, contract, &spec);
        assert_eq!(observation.typography_violation_count, 0);
        assert_eq!(observation.grid_violation_count, 0);
        assert_eq!(observation.dominant_corner_radius, Some(10));
        assert!(background_follows_strategy(
            &svg,
            observation.background_color.as_deref(),
            &spec
        ));
    }

    #[test]
    fn deck_checker_reports_color_font_scale_and_grid_drift_per_page() {
        let project = temp_project("drift");
        let theme = theme("教育清新");
        let mut spec =
            NativeDesignSystemSpec::from_inputs(&theme, "教育清新", Some("教育清新"), None, None);
        spec.assign_page_contracts(&plan(vec![slide(1, "cover", "cover", 2)]));
        fs::write(
            project.join("svg_output/01_cover.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720"><rect x="0" y="0" width="1280" height="720" fill="#081426"/><text x="4" y="20" font-family="Comic Sans MS" font-size="8" text-anchor="start" data-pome-role="title" data-pome-region-id="title" data-pome-region-x="0" data-pome-region-y="0" data-pome-region-width="200" data-pome-region-height="30">Broken</text></svg>"##,
        )
        .unwrap();
        let report = validate_deck_design_consistency(&project, &spec).unwrap();
        assert!(!report.passed);
        let rules = report
            .issues
            .iter()
            .map(|issue| issue.rule.as_str())
            .collect::<BTreeSet<_>>();
        assert!(rules.contains("design_system_color_drift"));
        assert!(rules.contains("design_system_typography_drift"));
        assert!(rules.contains("design_system_type_scale_violation"));
        assert!(rules.contains("design_system_grid_margin_violation"));
        assert!(rules.contains("design_system_background_strategy_drift"));
        fs::remove_dir_all(project).unwrap();
    }
}
