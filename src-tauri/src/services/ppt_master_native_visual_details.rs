use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

use super::{add_no_window, materialize_native_text_geometry_checker, single_line_log_value};

const CHECKER_SOURCE: &str = include_str!("../../scripts/ppt_native_visual_details.py");
const CHECKER_FILE: &str = "ppt_native_visual_details_v1.py";

pub(super) const NATIVE_VISUAL_DETAIL_CONTRACT: &str =
    "Native visual-detail contract (theme-independent):\n\
     - Structural elements must have stable ids. Mark cards/panels/nodes/icons/connectors with data-pome-visual-role=\"card|section|node|icon|connector\". Decorative elements use data-pome-visual-role=\"decoration\" and data-pome-decorative=\"true\".\n\
     - A connector <line>, <polyline>, or <path> must declare id, data-pome-visual-role=\"connector\", data-pome-from and data-pome-to. The referenced node/card ids must exist. Endpoints must touch the referenced shape boundary, not stop nearby or cross through it.\n\
     - Text inside a card must declare data-pome-owner with the owning card id. Nested icons/nodes also declare data-pome-owner, or data-pome-allow-overlap=\"true\" only when the overlap is intentional. Preserve at least 8px between the measured glyph bbox and the card border.\n\
     - Repeated cards or nodes that must share a row/column use data-pome-align-group and data-pome-align-axis=\"row|column\" on their wrapping <g>. Keep group members on the same baseline/column and use consistent dimensions and gaps.\n\
     - Connection lines may not pass through visible body/title/label text. Card boundaries, nodes, labels, icons, connectors, page number, and footer must remain inside the 1280x720 canvas.\n\
     - Use data-pome-allow-overlap=\"true\" only for intentional decorative overlap. Never use it to hide a readability or connector error.\n\
     - Before returning SVG, verify text-text, text-shape, card-card, shape-shape, connector endpoint, connector-text, safe-area, and alignment relationships.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeVisualDetailReport {
    pub schema_version: u32,
    pub passed: bool,
    #[serde(default)]
    pub svg_path: Option<String>,
    #[serde(default)]
    pub hard_errors: Vec<serde_json::Value>,
    #[serde(default)]
    pub warnings: Vec<serde_json::Value>,
    #[serde(default)]
    pub auto_fix_applied: Vec<serde_json::Value>,
    #[serde(default)]
    pub visual_elements: Vec<serde_json::Value>,
    #[serde(default)]
    pub visible_texts: Vec<String>,
    #[serde(default)]
    pub measurements: serde_json::Value,
    #[serde(default)]
    pub failure_kind: Option<String>,
    #[serde(default)]
    pub checker_error: Option<String>,
}

impl NativeVisualDetailReport {
    pub(super) fn violated_rule(&self) -> String {
        self.hard_errors
            .first()
            .and_then(|issue| issue.get("rule"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("native_visual_detail_checker_failed")
            .to_string()
    }

    pub(super) fn summary(&self) -> String {
        if let Some(error) = self.checker_error.as_deref() {
            return format!("visual detail checker error: {error}");
        }
        format!(
            "hardErrors={},warnings={},autoFixes={},measurements={},actionableIssues={}",
            self.hard_errors.len(),
            self.warnings.len(),
            self.auto_fix_applied.len(),
            self.measurements,
            serde_json::to_string(&self.hard_errors).unwrap_or_default()
        )
    }
}

fn materialize_checker() -> Result<PathBuf, AppError> {
    let _ = materialize_native_text_geometry_checker()?;
    let directory = std::env::temp_dir().join("pomegranate-native-tools");
    fs::create_dir_all(&directory).map_err(|error| {
        AppError::Custom(format!(
            "创建原生视觉细节检查器目录失败: {} ({error})",
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
                "写入原生视觉细节检查器失败: {} ({error})",
                path.display()
            ))
        })?;
    }
    Ok(path)
}

pub(super) fn run_visual_detail_check(
    python_path: &str,
    svg_path: &Path,
    report_path: &Path,
) -> Result<NativeVisualDetailReport, AppError> {
    let checker = materialize_checker()?;
    let mut command = Command::new(python_path);
    command
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .arg(&checker)
        .arg("--svg")
        .arg(svg_path)
        .arg("--auto-fix")
        .arg("--require-contract")
        .arg("--report")
        .arg(report_path);
    add_no_window(&mut command);
    let output = command.output().map_err(|error| {
        AppError::Custom(format!(
            "启动原生视觉细节检查器失败: python={}, checker={}, svg={} ({error})",
            python_path,
            checker.display(),
            svg_path.display()
        ))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let report: NativeVisualDetailReport = serde_json::from_str(&stdout).map_err(|error| {
        AppError::Custom(format!(
            "解析原生视觉细节检查结果失败: svg={}, exitCode={:?}, stdout={}, stderr={} ({error})",
            svg_path.display(),
            output.status.code(),
            single_line_log_value(&stdout),
            single_line_log_value(&stderr)
        ))
    })?;
    if let Some(checker_error) = report.checker_error.as_deref() {
        return Err(AppError::Custom(format!(
            "原生视觉细节检查器执行失败: svg={}, error={}, stderr={}",
            svg_path.display(),
            checker_error,
            single_line_log_value(&stderr)
        )));
    }
    if !matches!(output.status.code(), Some(0 | 2)) {
        return Err(AppError::Custom(format!(
            "原生视觉细节检查器异常退出: svg={}, exitCode={:?}, stderr={}",
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

    #[test]
    fn visual_detail_report_keeps_actionable_rule_and_visible_text() {
        let report: NativeVisualDetailReport = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "passed": false,
            "hardErrors": [{"rule": "connector_endpoint_not_on_node"}],
            "warnings": [],
            "autoFixApplied": [],
            "visualElements": [],
            "visibleTexts": ["事实一", "事实二"],
            "measurements": {"connectorCount": 1}
        }))
        .unwrap();
        assert_eq!(report.violated_rule(), "connector_endpoint_not_on_node");
        assert_eq!(report.visible_texts, ["事实一", "事实二"]);
        assert!(report.summary().contains("connectorCount"));
    }

    #[test]
    fn contract_is_theme_and_subject_independent() {
        assert!(NATIVE_VISUAL_DETAIL_CONTRACT.contains("data-pome-from"));
        assert!(NATIVE_VISUAL_DETAIL_CONTRACT.contains("card-card"));
        assert!(!NATIVE_VISUAL_DETAIL_CONTRACT.contains("科技蓝"));
        assert!(!NATIVE_VISUAL_DETAIL_CONTRACT.contains("毛泽东"));
        assert!(!NATIVE_VISUAL_DETAIL_CONTRACT.contains('#'));
    }
}
