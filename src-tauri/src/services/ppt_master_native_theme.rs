use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

pub(super) const NATIVE_THEME_SPEC_FILE: &str = "native_theme_spec.json";
const NATIVE_THEME_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeThemeSpec {
    pub schema_version: u32,
    pub theme_name: String,
    pub mood: String,
    pub background_strategy: String,
    pub background_color: String,
    pub secondary_background_color: String,
    pub surface_color: String,
    pub panel_color: String,
    pub primary_color: String,
    pub secondary_color: String,
    pub accent_color: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub border_color: String,
    pub highlight_style: String,
    pub shape_language: String,
    pub decoration_language: String,
    pub image_treatment: String,
    pub forbidden_colors: Vec<String>,
    pub forbidden_visual_patterns: Vec<String>,
    pub source_style: String,
    pub source_custom_style: String,
    pub source_extra_requirements: String,
    pub source_visual_suggestions: String,
}

impl NativeThemeSpec {
    pub(super) fn from_inputs(
        style: &str,
        custom_style: Option<&str>,
        extra_requirements: Option<&str>,
        visual_suggestions: Option<&str>,
    ) -> Self {
        let style = style.trim();
        let custom_style = custom_style.unwrap_or("").trim();
        let extra_requirements = extra_requirements.unwrap_or("").trim();
        let visual_suggestions = visual_suggestions.unwrap_or("").trim();
        let explicit = format!("{style}\n{custom_style}\n{extra_requirements}");

        let mut spec = if contains_any(&explicit, &["红色情怀", "红色", "赤色", "党史", "革命风"])
        {
            Self::red_heritage()
        } else if contains_any(&explicit, &["科技", "科技蓝", "深蓝", "赛博", "未来感"])
        {
            Self::tech_blue()
        } else if contains_any(&explicit, &["黑金", "奢华", "高端"]) {
            Self::black_gold()
        } else if contains_any(&explicit, &["学术", "论文", "答辩"]) {
            Self::academic()
        } else if contains_any(&explicit, &["图文", "杂志", "摄影", "编辑"]) {
            Self::photo_editorial()
        } else {
            Self::neutral_custom(style)
        };

        spec.source_style = style.to_string();
        spec.source_custom_style = custom_style.to_string();
        spec.source_extra_requirements = extra_requirements.to_string();
        spec.source_visual_suggestions = visual_suggestions.to_string();
        spec
    }

    fn red_heritage() -> Self {
        Self {
            schema_version: NATIVE_THEME_SCHEMA_VERSION,
            theme_name: "red-heritage".to_string(),
            mood: "庄重、热烈、历史叙事感，红色情怀明确但避免六页纯红铺满".to_string(),
            background_strategy:
                "米白/暖灰为正文底，深红用于封面、章节或重点区，整套共享红金视觉锚点".to_string(),
            background_color: "#F7F0E6".to_string(),
            secondary_background_color: "#8B1E1E".to_string(),
            surface_color: "#FFF9F0".to_string(),
            panel_color: "#F3E3D3".to_string(),
            primary_color: "#B91C1C".to_string(),
            secondary_color: "#7F1D1D".to_string(),
            accent_color: "#D4A017".to_string(),
            text_primary: "#2A1713".to_string(),
            text_secondary: "#6B4B43".to_string(),
            border_color: "#C88A58".to_string(),
            highlight_style: "红金分隔线、金色重点、印章感方形强调".to_string(),
            shape_language: "旗帜感斜线、硬朗矩形、克制圆角、历史档案构图".to_string(),
            decoration_language:
                "放射线、印章方形、红金线条、档案纸纹理感（仅用基础 SVG 图形表达）".to_string(),
            image_treatment: "如有图片，使用暖色/低饱和历史档案处理，并以红金边框统一".to_string(),
            forbidden_colors: vec![
                "#2563EB".to_string(),
                "#38BDF8".to_string(),
                "#7C3AED".to_string(),
            ],
            forbidden_visual_patterns: vec![
                "科技蓝成为主要视觉色".to_string(),
                "六页退化成普通白底蓝线".to_string(),
                "各页自行选择互不相关的主题色".to_string(),
            ],
            source_style: String::new(),
            source_custom_style: String::new(),
            source_extra_requirements: String::new(),
            source_visual_suggestions: String::new(),
        }
    }

    fn tech_blue() -> Self {
        Self {
            schema_version: NATIVE_THEME_SCHEMA_VERSION,
            theme_name: "tech-blue".to_string(),
            mood: "深色、精密、未来感、信息可视化".to_string(),
            background_strategy:
                "深海军蓝为主背景，深蓝面板分层，亮蓝和紫色作为线条、节点与数据强调".to_string(),
            background_color: "#081426".to_string(),
            secondary_background_color: "#0D2340".to_string(),
            surface_color: "#102B4E".to_string(),
            panel_color: "#0B1F38".to_string(),
            primary_color: "#2563EB".to_string(),
            secondary_color: "#38BDF8".to_string(),
            accent_color: "#7C3AED".to_string(),
            text_primary: "#F8FBFF".to_string(),
            text_secondary: "#CBD5E1".to_string(),
            border_color: "#2563EB".to_string(),
            highlight_style: "蓝紫发光节点、亮蓝数据与细线连接".to_string(),
            shape_language: "精密网格、细线框、圆形节点、适度圆角卡片".to_string(),
            decoration_language: "电路线、坐标网格、光点、蓝紫渐变层次".to_string(),
            image_treatment: "冷色调、深色蒙版、蓝色描边".to_string(),
            forbidden_colors: vec![],
            forbidden_visual_patterns: vec!["大面积暖橙或正红成为主色".to_string()],
            source_style: String::new(),
            source_custom_style: String::new(),
            source_extra_requirements: String::new(),
            source_visual_suggestions: String::new(),
        }
    }

    fn academic() -> Self {
        Self::preset(
            "academic",
            "理性、克制、可信",
            "#F8FAFC",
            "#EEF4FB",
            "#FFFFFF",
            "#EEF4FB",
            "#1E3A8A",
            "#64748B",
            "#B91C1C",
            "#1F2A37",
            "#64748B",
            "#C7D7EA",
        )
    }

    fn photo_editorial() -> Self {
        Self::preset(
            "photo-editorial",
            "编辑感、图文叙事、留白",
            "#F9FAFB",
            "#ECFEFF",
            "#FFFFFF",
            "#F1F5F9",
            "#0F766E",
            "#2563EB",
            "#E11D48",
            "#243042",
            "#5C6670",
            "#C7D2FE",
        )
    }

    fn black_gold() -> Self {
        Self::preset(
            "black-gold",
            "高端、沉稳、戏剧性",
            "#111111",
            "#211A12",
            "#1A1A1A",
            "#241F18",
            "#D4A017",
            "#A77B24",
            "#F4D06F",
            "#FFF8E7",
            "#D6C7A1",
            "#8C6A25",
        )
    }

    fn neutral_custom(style: &str) -> Self {
        let mut spec = Self::preset(
            if style.trim().is_empty() {
                "custom"
            } else {
                style.trim()
            },
            "统一、克制、清晰",
            "#FAFAF8",
            "#F1F0EA",
            "#FFFFFF",
            "#F5F4EF",
            "#334155",
            "#64748B",
            "#C07A35",
            "#1F2937",
            "#64748B",
            "#CBD5E1",
        );
        spec.forbidden_visual_patterns = vec!["各页自行选择互不相关的主题色".to_string()];
        spec
    }

    #[allow(clippy::too_many_arguments)]
    fn preset(
        name: &str,
        mood: &str,
        background: &str,
        secondary_background: &str,
        surface: &str,
        panel: &str,
        primary: &str,
        secondary: &str,
        accent: &str,
        text_primary: &str,
        text_secondary: &str,
        border: &str,
    ) -> Self {
        Self {
            schema_version: NATIVE_THEME_SCHEMA_VERSION,
            theme_name: name.to_string(),
            mood: mood.to_string(),
            background_strategy:
                "按封面、正文与总结的节奏交替使用主背景和次背景，但保持同一配色合同".to_string(),
            background_color: background.to_string(),
            secondary_background_color: secondary_background.to_string(),
            surface_color: surface.to_string(),
            panel_color: panel.to_string(),
            primary_color: primary.to_string(),
            secondary_color: secondary.to_string(),
            accent_color: accent.to_string(),
            text_primary: text_primary.to_string(),
            text_secondary: text_secondary.to_string(),
            border_color: border.to_string(),
            highlight_style: "使用主色、辅色和点缀色建立一致强调层级".to_string(),
            shape_language: "整套页面共享一致的线条粗细、圆角尺度和图形气质".to_string(),
            decoration_language: "装饰元素随布局变化，但颜色与线条语言保持统一".to_string(),
            image_treatment: "图片色调与主题色协调，并使用统一遮罩或边框".to_string(),
            forbidden_colors: vec![],
            forbidden_visual_patterns: vec![],
            source_style: String::new(),
            source_custom_style: String::new(),
            source_extra_requirements: String::new(),
            source_visual_suggestions: String::new(),
        }
    }

    pub(super) fn preferred_mode(&self) -> &'static str {
        match self.theme_name.as_str() {
            "tech-blue" => "showcase",
            "red-heritage" => "narrative",
            "academic" => "instructional",
            "photo-editorial" => "showcase",
            _ => "pyramid",
        }
    }

    pub(super) fn preferred_visual_style(&self) -> &'static str {
        match self.theme_name.as_str() {
            "tech-blue" => "dark-tech",
            "red-heritage" => "vintage-poster",
            "academic" => "data-journalism",
            "photo-editorial" => "photo-editorial",
            _ => "swiss-minimal",
        }
    }

    pub(super) fn prompt_contract(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub(super) fn theme_colors(&self) -> [&str; 3] {
        [
            self.primary_color.as_str(),
            self.secondary_color.as_str(),
            self.accent_color.as_str(),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NativeThemeValidation {
    pub passed: bool,
    pub top_colors: Vec<(String, usize)>,
    pub theme_color_uses: usize,
    pub forbidden_color_uses: usize,
    pub summary: String,
}

pub(super) fn validate_svg_theme(svg: &str, theme: &NativeThemeSpec) -> NativeThemeValidation {
    let counts = svg_color_counts(svg);
    let theme_color_uses = theme
        .theme_colors()
        .iter()
        .map(|color| {
            counts
                .get(&color.to_ascii_uppercase())
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let forbidden_color_uses = theme
        .forbidden_colors
        .iter()
        .map(|color| {
            counts
                .get(&color.to_ascii_uppercase())
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let mut top_colors: Vec<(String, usize)> = counts.into_iter().collect();
    top_colors.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    top_colors.truncate(8);
    let passed = theme_color_uses > 0 && forbidden_color_uses == 0;
    let summary = format!(
        "theme={},themeColorUses={},forbiddenColorUses={},topColors={}",
        theme.theme_name,
        theme_color_uses,
        forbidden_color_uses,
        top_colors
            .iter()
            .map(|(color, count)| format!("{color}:{count}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    NativeThemeValidation {
        passed,
        top_colors,
        theme_color_uses,
        forbidden_color_uses,
        summary,
    }
}

pub(super) fn validate_visible_text_integrity(svg: &str) -> Result<(), String> {
    if svg.contains('\u{FFFD}') {
        return Err("visible_text_contains_unicode_replacement_character".to_string());
    }
    for visible in visible_svg_text(svg) {
        validate_visible_text_fragment(&visible)?;
    }
    Ok(())
}

pub(super) fn validate_visible_text_fragment(value: &str) -> Result<(), String> {
    if value.contains('\u{FFFD}') {
        return Err("text_contains_unicode_replacement_character".to_string());
    }
    if cjk_hash_cjk_regex().is_match(value) {
        return Err("text_contains_abnormal_markdown_hash_fragment".to_string());
    }
    if value
        .lines()
        .any(|line| markdown_heading_regex().is_match(line))
    {
        return Err("text_contains_visible_markdown_heading_marker".to_string());
    }
    Ok(())
}

pub(super) fn persist_theme_spec(
    project: &Path,
    theme: &NativeThemeSpec,
) -> Result<PathBuf, String> {
    let path = project.join(NATIVE_THEME_SPEC_FILE);
    let temp = project.join(format!(".{NATIVE_THEME_SPEC_FILE}.tmp"));
    let json = serde_json::to_string_pretty(theme)
        .map_err(|error| format!("serialize native theme spec failed: {error}"))?;
    fs::write(&temp, format!("{json}\n")).map_err(|error| {
        format!(
            "write native theme spec failed: {} ({error})",
            temp.display()
        )
    })?;
    if path.is_file() {
        fs::remove_file(&path).map_err(|error| {
            format!(
                "replace native theme spec failed: {} ({error})",
                path.display()
            )
        })?;
    }
    fs::rename(&temp, &path).map_err(|error| {
        format!(
            "commit native theme spec failed: {} -> {} ({error})",
            temp.display(),
            path.display()
        )
    })?;
    Ok(path)
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn svg_color_counts(svg: &str) -> BTreeMap<String, usize> {
    let without_metadata = metadata_regex().replace_all(svg, "");
    let mut counts = BTreeMap::new();
    for captures in color_attribute_regex().captures_iter(&without_metadata) {
        let Some(color) = captures.get(1) else {
            continue;
        };
        *counts
            .entry(color.as_str().to_ascii_uppercase())
            .or_default() += 1;
    }
    counts
}

fn visible_svg_text(svg: &str) -> Vec<String> {
    text_element_regex()
        .captures_iter(svg)
        .filter_map(|captures| captures.get(1))
        .map(|content| nested_tag_regex().replace_all(content.as_str(), ""))
        .map(|content| {
            content
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'")
        })
        .collect()
}

fn metadata_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<metadata\b[^>]*>.*?</metadata\s*>").expect("valid regex"))
}

fn color_attribute_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:fill|stroke|stop-color)\s*=\s*["'](#[0-9a-f]{6})["']"#)
            .expect("valid regex")
    })
}

fn text_element_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<text\b[^>]*>(.*?)</text\s*>").expect("valid regex"))
}

fn nested_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<[^>]+>").expect("valid regex"))
}

fn cjk_hash_cjk_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[\p{Han}]#{1,3}[\p{Han}]").expect("valid regex"))
}

fn markdown_heading_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*#{1,3}\s+").expect("valid regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_custom_style_overrides_stale_blue_visual_suggestion() {
        let theme = NativeThemeSpec::from_inputs(
            "红色情怀",
            Some("红色情怀"),
            Some("界面要红色情怀拉满"),
            Some("可使用科技蓝线条"),
        );
        assert_eq!(theme.theme_name, "red-heritage");
        assert_eq!(theme.primary_color, "#B91C1C");
        assert_eq!(theme.preferred_visual_style(), "vintage-poster");
    }

    #[test]
    fn tech_blue_preserves_dark_tech_contract() {
        let theme = NativeThemeSpec::from_inputs("科技蓝", None, None, None);
        assert_eq!(theme.theme_name, "tech-blue");
        assert_eq!(theme.background_color, "#081426");
        assert_eq!(theme.preferred_visual_style(), "dark-tech");
    }

    #[test]
    fn theme_validation_requires_theme_color_and_rejects_forbidden_blue() {
        let theme = NativeThemeSpec::from_inputs("红色情怀", Some("红色情怀"), None, None);
        let valid = r##"<svg><rect fill="#F7F0E6"/><path stroke="#B91C1C"/><line stroke="#D4A017"/></svg>"##;
        assert!(validate_svg_theme(valid, &theme).passed);

        let invalid = r##"<svg><rect fill="#FFFFFF"/><path stroke="#2563EB"/></svg>"##;
        let result = validate_svg_theme(invalid, &theme);
        assert!(!result.passed);
        assert_eq!(result.theme_color_uses, 0);
        assert_eq!(result.forbidden_color_uses, 1);
    }

    #[test]
    fn visible_text_integrity_rejects_corruption_and_preserves_legal_hashes() {
        assert!(validate_visible_text_fragment("大���进").is_err());
        assert!(validate_visible_text_fragment("三个#界").is_err());
        assert!(validate_visible_text_fragment("## 小标题").is_err());
        assert!(validate_visible_text_fragment("采用 #5 型号与 C# 语言").is_ok());
        assert!(validate_visible_text_fragment("中文 UTF-8 与 English 混排").is_ok());
    }

    #[test]
    fn svg_integrity_checks_only_visible_hash_fragments() {
        let valid = r##"<svg><path fill="#B91C1C"/><text>第 2 阶段使用 #5 型号</text></svg>"##;
        assert!(validate_visible_text_integrity(valid).is_ok());
        let invalid = "<svg><text>三个#界</text></svg>";
        assert!(validate_visible_text_integrity(invalid).is_err());
    }

    #[test]
    fn theme_spec_is_persisted_as_project_artifact() {
        let project = std::env::temp_dir().join(format!(
            "pome-native-theme-spec-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&project).expect("create theme test project");
        let theme = NativeThemeSpec::from_inputs(
            "红色情怀",
            Some("红色情怀"),
            Some("界面要红色情怀拉满"),
            None,
        );
        let path = persist_theme_spec(&project, &theme).expect("persist theme spec");
        let loaded: NativeThemeSpec =
            serde_json::from_str(&fs::read_to_string(&path).expect("read persisted theme spec"))
                .expect("parse persisted theme spec");
        assert_eq!(loaded, theme);
        fs::remove_dir_all(project).expect("remove theme test project");
    }
}
