use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

use super::{
    add_no_window, materialize_native_text_geometry_checker, single_line_log_value, Slide,
};

const CHECKER_SOURCE: &str = include_str!("../../scripts/ppt_native_space_utilization.py");
const CHECKER_FILE: &str = "ppt_native_space_utilization_v1.py";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativePageDensityContract {
    pub schema_version: u32,
    pub page_rhythm: String,
    pub content_density: String,
    pub visual_weight: String,
    pub focal_region: String,
    pub supporting_regions: Vec<String>,
    pub expected_content_units: usize,
    pub minimum_supporting_regions: usize,
    pub allow_large_whitespace: bool,
    pub layout_capacity_strategy: String,
}

impl NativePageDensityContract {
    pub(super) fn for_slide(slide: &Slide) -> Self {
        let expected_content_units = slide
            .content_blocks
            .len()
            .max(slide.must_include.len())
            .max(slide.bullets.len());
        let semantic_anchor =
            matches!(
                slide.slide_type.as_str(),
                "hero" | "section" | "quote_focus"
            ) || matches!(slide.layout.as_str(), "cover" | "section" | "highlight");
        let page_rhythm = if semantic_anchor {
            if slide.slide_type == "section" || slide.layout == "section" {
                "breathing"
            } else {
                "anchor"
            }
        } else if matches!(slide.layout.as_str(), "timeline" | "process" | "compare")
            && expected_content_units >= 4
        {
            "dense"
        } else {
            "balanced"
        };
        let content_density = match expected_content_units {
            0..=2 => "low",
            3..=4 => "medium",
            _ => "high",
        };
        let layout_capacity_strategy = if semantic_anchor {
            "single-focal-anchor"
        } else if expected_content_units <= 2 {
            "dominant-claim-with-semantic-support"
        } else if slide.layout == "timeline" && expected_content_units <= 3 {
            "compact-staged-path"
        } else if matches!(slide.layout.as_str(), "cards" | "matrix") && expected_content_units < 4
        {
            "semantic-split-not-empty-grid"
        } else {
            "full-main-region-with-support"
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
        Self {
            schema_version: 1,
            page_rhythm: page_rhythm.to_string(),
            content_density: content_density.to_string(),
            visual_weight: visual_weight.to_string(),
            focal_region: focal_region.to_string(),
            supporting_regions,
            expected_content_units,
            minimum_supporting_regions,
            allow_large_whitespace: semantic_anchor,
            layout_capacity_strategy: layout_capacity_strategy.to_string(),
        }
    }

    pub(super) fn prompt_contract(&self) -> String {
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        format!(
            "{json}\n\
             Universal density rules (independent of theme and color):\n\
             - anchor/breathing pages may keep purposeful whitespace only when one unmistakable visual focal point and background structure explain it.\n\
             - balanced/dense content pages must use the main effective canvas plus at least the declared supporting regions; do not leave an unused horizontal or vertical third.\n\
             - Match layout capacity to meaning: 1-2 ideas use a dominant claim or quote structure; 3 stages use a compact staged path; 4-6 parallel items may use a matrix/cards; comparisons use two-sided structure; timelines require actual temporal stages.\n\
             - Use every mustInclude fact. Supporting visuals must encode meaning (relations, stage labels, data bars, quote emphasis, section bands, or semantic decoration), not empty rectangles.\n\
             - Preserve every fact semantically, but express it as concise labels plus short supporting text instead of copying mainClaim or long source sentences verbatim; allocate wrapped text regions before drawing.\n\
             - Density must come from the declared focal and supporting regions. Do not add footer slogans, duplicate page-number fragments, or edge annotations to fake occupancy; the footer strip does not count as page content.\n\
             - Do not invent facts, repeat text, add placeholder cards, shrink fonts below the geometry contract, or turn every page into a card grid."
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeSpaceUtilizationReport {
    pub schema_version: u32,
    pub passed: bool,
    #[serde(default)]
    pub svg_path: Option<String>,
    #[serde(default)]
    pub page_rhythm: String,
    #[serde(default)]
    pub expected_content_units: usize,
    #[serde(default)]
    pub information_occupancy_ratio: f64,
    #[serde(default)]
    pub visual_structure_ratio: f64,
    #[serde(default)]
    pub combined_occupancy_ratio: f64,
    #[serde(default)]
    pub largest_empty_information_region: serde_json::Value,
    #[serde(default)]
    pub occupied_zone_count: usize,
    #[serde(default)]
    pub dominant_zone_share: f64,
    #[serde(default)]
    pub band_occupancy: serde_json::Value,
    #[serde(default)]
    pub text_block_count: usize,
    #[serde(default)]
    pub body_text_block_count: usize,
    #[serde(default)]
    pub card_count: usize,
    #[serde(default)]
    pub substantive_graphic_count: usize,
    #[serde(default)]
    pub rendered_content_units: usize,
    #[serde(default)]
    pub issues: Vec<serde_json::Value>,
    #[serde(default)]
    pub checker_error: Option<String>,
}

impl NativeSpaceUtilizationReport {
    pub(super) fn violated_rule(&self) -> String {
        self.issues
            .first()
            .and_then(|issue| issue.get("rule"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("native_space_utilization_checker_failed")
            .to_string()
    }

    pub(super) fn summary(&self) -> String {
        if let Some(error) = self.checker_error.as_deref() {
            return format!("space utilization checker error: {error}");
        }
        format!(
            "pageRhythm={},informationOccupancy={:.4},combinedOccupancy={:.4},largestEmptyRegion={},occupiedZones={},textBlocks={},cards={},substantiveGraphics={},issues={}",
            self.page_rhythm,
            self.information_occupancy_ratio,
            self.combined_occupancy_ratio,
            self.largest_empty_information_region,
            self.occupied_zone_count,
            self.text_block_count,
            self.card_count,
            self.substantive_graphic_count,
            serde_json::to_string(&self.issues).unwrap_or_default()
        )
    }
}

fn materialize_checker() -> Result<PathBuf, AppError> {
    let _ = materialize_native_text_geometry_checker()?;
    let directory = std::env::temp_dir().join("pomegranate-native-tools");
    fs::create_dir_all(&directory).map_err(|error| {
        AppError::Custom(format!(
            "创建原生空间占用检查器目录失败: {} ({error})",
            directory.display()
        ))
    })?;
    let path = directory.join(CHECKER_FILE);
    let needs_write = fs::read_to_string(&path)
        .map(|current| current != CHECKER_SOURCE)
        .unwrap_or(true);
    if needs_write {
        fs::write(&path, CHECKER_SOURCE).map_err(|error| {
            AppError::Custom(format!(
                "写入原生空间占用检查器失败: {} ({error})",
                path.display()
            ))
        })?;
    }
    Ok(path)
}

pub(super) fn run_space_utilization_check(
    python_path: &str,
    svg_path: &Path,
    contract: &NativePageDensityContract,
    report_path: &Path,
) -> Result<NativeSpaceUtilizationReport, AppError> {
    let checker = materialize_checker()?;
    let mut command = Command::new(python_path);
    command
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .arg(&checker)
        .arg("--svg")
        .arg(svg_path)
        .arg("--page-rhythm")
        .arg(&contract.page_rhythm)
        .arg("--expected-content-units")
        .arg(contract.expected_content_units.to_string())
        .arg("--report")
        .arg(report_path);
    if contract.allow_large_whitespace {
        command.arg("--allow-large-whitespace");
    }
    add_no_window(&mut command);
    let output = command.output().map_err(|error| {
        AppError::Custom(format!(
            "启动原生空间占用检查器失败: python={}, checker={}, svg={} ({error})",
            python_path,
            checker.display(),
            svg_path.display()
        ))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let report: NativeSpaceUtilizationReport = serde_json::from_str(&stdout).map_err(|error| {
        AppError::Custom(format!(
            "解析原生空间占用检查结果失败: svg={}, exitCode={:?}, stdout={}, stderr={} ({error})",
            svg_path.display(),
            output.status.code(),
            single_line_log_value(&stdout),
            single_line_log_value(&stderr)
        ))
    })?;
    if let Some(checker_error) = report.checker_error.as_deref() {
        return Err(AppError::Custom(format!(
            "原生空间占用检查器执行失败: svg={}, error={}, stderr={}",
            svg_path.display(),
            checker_error,
            single_line_log_value(&stderr)
        )));
    }
    if !matches!(output.status.code(), Some(0 | 2)) {
        return Err(AppError::Custom(format!(
            "原生空间占用检查器异常退出: svg={}, exitCode={:?}, stderr={}",
            svg_path.display(),
            output.status.code(),
            single_line_log_value(&stderr)
        )));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ContentBlock, Slide};

    fn slide(layout: &str, slide_type: &str, units: usize) -> Slide {
        Slide {
            page: 2,
            page_index: 2,
            page_id: "P02".to_string(),
            slide_type: slide_type.to_string(),
            layout: layout.to_string(),
            title: "标题".to_string(),
            subtitle: String::new(),
            bullets: (0..units).map(|index| format!("事实 {index}")).collect(),
            visual_hint: String::new(),
            page_theme: String::new(),
            main_claim: "核心结论".to_string(),
            core_message: "核心结论".to_string(),
            content_scope: String::new(),
            content_blocks: (0..units)
                .map(|index| ContentBlock {
                    label: format!("要点 {index}"),
                    text: format!("事实 {index}"),
                    detail: String::new(),
                })
                .collect(),
            evidence: Vec::new(),
            relation: String::new(),
            density: String::new(),
            visual_intent: String::new(),
            must_include: (0..units).map(|index| format!("事实 {index}")).collect(),
            must_avoid: Vec::new(),
            page_rhythm: String::new(),
            chart_ref: String::new(),
            chart_type: String::new(),
            file_stem: "slide_02".to_string(),
            speaker_note: String::new(),
        }
    }

    #[test]
    fn normal_content_pages_are_balanced_or_dense() {
        assert_eq!(
            NativePageDensityContract::for_slide(&slide("cards", "overview", 5)).page_rhythm,
            "balanced"
        );
        assert_eq!(
            NativePageDensityContract::for_slide(&slide("timeline", "timeline", 5)).page_rhythm,
            "dense"
        );
    }

    #[test]
    fn low_content_selects_dominant_claim_instead_of_empty_grid() {
        let contract = NativePageDensityContract::for_slide(&slide("cards", "summary", 2));
        assert_eq!(
            contract.layout_capacity_strategy,
            "dominant-claim-with-semantic-support"
        );
        assert_eq!(contract.expected_content_units, 2);
    }

    #[test]
    fn density_contract_has_no_theme_input_or_color_branch() {
        let base = slide("cards", "overview", 5);
        let first = NativePageDensityContract::for_slide(&base);
        let second = NativePageDensityContract::for_slide(&base);
        assert_eq!(first, second);
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains('#'));
        assert!(!serialized.contains('红'));
        assert!(!serialized.contains('蓝'));
        assert!(!serialized.contains("毛泽东"));
    }
}
