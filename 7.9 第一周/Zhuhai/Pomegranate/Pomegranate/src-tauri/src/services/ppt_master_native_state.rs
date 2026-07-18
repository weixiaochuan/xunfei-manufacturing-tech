use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const NATIVE_STATE_FILE: &str = "native_generation_state.json";
pub(super) const NATIVE_STATE_SCHEMA_VERSION: u32 = 1;
pub(super) const NATIVE_GENERATION_SPEC_VERSION: &str =
    "pomegranate-ppt-master-native-v4.2-page-density-contract";
pub(super) const NATIVE_CANVAS: &str = "1280x720";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeFingerprintInput {
    pub topic: String,
    pub prompt: String,
    pub planning_context: String,
    pub raw_material: String,
    pub understanding: String,
    pub extra_requirements: String,
    pub audience: String,
    pub slide_count: usize,
    pub style: String,
    pub custom_style: String,
    pub visual_suggestions: String,
    pub theme_spec: String,
    pub mode: String,
    pub visual_style: String,
    pub layout_bias: Vec<String>,
    pub chart_bias: Vec<String>,
    pub model_database_id: i64,
    pub model_provider: String,
    pub model_id: String,
    pub generation_mode: String,
    pub generation_engine: String,
    pub generation_spec_version: String,
    pub canvas: String,
    pub max_output_tokens: i64,
    pub timeout_seconds: u64,
}

impl NativeFingerprintInput {
    pub(super) fn fingerprint(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("序列化原生生成输入指纹失败: {error}"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeStateModel {
    pub database_id: i64,
    pub provider: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeTextGeometryState {
    pub passed: bool,
    #[serde(default)]
    pub hard_errors: Vec<serde_json::Value>,
    #[serde(default)]
    pub warnings: Vec<serde_json::Value>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativePageState {
    pub status: String,
    pub attempts: usize,
    pub svg_path: String,
    pub last_error: Option<String>,
    #[serde(default)]
    pub violated_rule: Option<String>,
    #[serde(default)]
    pub checker_summary: Option<String>,
    #[serde(default)]
    pub text_geometry: Option<NativeTextGeometryState>,
    #[serde(default)]
    pub reused: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeArtifactState {
    pub design_spec_path: String,
    pub spec_lock_path: String,
    pub slide_plan_path: String,
    pub notes_path: String,
    pub final_pptx_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NativeGenerationState {
    pub schema_version: u32,
    pub input_fingerprint: String,
    pub generation_mode: String,
    pub generation_engine: String,
    pub generation_spec_version: String,
    pub canvas: String,
    pub topic: String,
    pub slide_count: usize,
    pub model: NativeStateModel,
    pub current_stage: String,
    pub status: String,
    pub pages: BTreeMap<String, NativePageState>,
    pub artifacts: NativeArtifactState,
    pub started_at: String,
    pub updated_at: String,
}

impl NativeGenerationState {
    pub(super) fn new(
        input_fingerprint: String,
        topic: String,
        slide_count: usize,
        model: NativeStateModel,
        project: &Path,
    ) -> Self {
        let now = now();
        let pages = (1..=slide_count)
            .map(|page| {
                (
                    page.to_string(),
                    NativePageState {
                        status: "pending".to_string(),
                        attempts: 0,
                        svg_path: project
                            .join("svg_output")
                            .join(format!("{page:02}_pending.svg"))
                            .to_string_lossy()
                            .to_string(),
                        last_error: None,
                        violated_rule: None,
                        checker_summary: None,
                        text_geometry: None,
                        reused: false,
                        updated_at: now.clone(),
                    },
                )
            })
            .collect();
        Self {
            schema_version: NATIVE_STATE_SCHEMA_VERSION,
            input_fingerprint,
            generation_mode: "agent".to_string(),
            generation_engine: "ppt_master_native".to_string(),
            generation_spec_version: NATIVE_GENERATION_SPEC_VERSION.to_string(),
            canvas: NATIVE_CANVAS.to_string(),
            topic,
            slide_count,
            model,
            current_stage: "prepare_project".to_string(),
            status: "running".to_string(),
            pages,
            artifacts: NativeArtifactState {
                design_spec_path: project.join("design_spec.md").to_string_lossy().to_string(),
                spec_lock_path: project.join("spec_lock.md").to_string_lossy().to_string(),
                slide_plan_path: project
                    .join("slide_plan.json")
                    .to_string_lossy()
                    .to_string(),
                notes_path: project.join("notes").to_string_lossy().to_string(),
                final_pptx_path: None,
            },
            started_at: now.clone(),
            updated_at: now,
        }
    }

    pub(super) fn set_stage(&mut self, stage: &str) {
        self.current_stage = stage.to_string();
        self.status = "running".to_string();
        self.updated_at = now();
    }

    pub(super) fn set_status(&mut self, status: &str) {
        self.status = status.to_string();
        self.updated_at = now();
    }

    pub(super) fn page_mut(&mut self, page: usize) -> &mut NativePageState {
        self.pages.entry(page.to_string()).or_insert_with(|| {
            let updated_at = now();
            NativePageState {
                status: "pending".to_string(),
                attempts: 0,
                svg_path: String::new(),
                last_error: None,
                violated_rule: None,
                checker_summary: None,
                text_geometry: None,
                reused: false,
                updated_at,
            }
        })
    }
}

pub(super) fn now() -> String {
    Utc::now().to_rfc3339()
}

pub(super) fn read_state(project: &Path) -> Result<NativeGenerationState, String> {
    let path = project.join(NATIVE_STATE_FILE);
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("读取原生断点状态失败: {} ({error})", path.display()))?;
    let state: NativeGenerationState = serde_json::from_str(&raw)
        .map_err(|error| format!("原生断点状态 JSON 损坏: {} ({error})", path.display()))?;
    if state.schema_version != NATIVE_STATE_SCHEMA_VERSION {
        return Err(format!(
            "原生断点状态版本不支持: expected={}, actual={}, path={}",
            NATIVE_STATE_SCHEMA_VERSION,
            state.schema_version,
            path.display()
        ));
    }
    Ok(state)
}

pub(super) fn write_state_atomic(
    project: &Path,
    state: &NativeGenerationState,
) -> Result<PathBuf, String> {
    let target = project.join(NATIVE_STATE_FILE);
    let temp = project.join(format!(".{NATIVE_STATE_FILE}.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("序列化原生断点状态失败: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)
        .map_err(|error| format!("创建原生断点临时文件失败: {} ({error})", temp.display()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("写入原生断点临时文件失败: {} ({error})", temp.display()))?;
    drop(file);

    match fs::rename(&temp, &target) {
        Ok(()) => Ok(target),
        Err(first_error) if target.exists() => {
            fs::remove_file(&target).map_err(|error| {
                format!(
                    "替换原生断点状态失败: {} ({first_error}); 删除旧状态失败: {error}",
                    target.display()
                )
            })?;
            fs::rename(&temp, &target)
                .map_err(|error| format!("替换原生断点状态失败: {} ({error})", target.display()))?;
            Ok(target)
        }
        Err(error) => Err(format!(
            "提交原生断点状态失败: {} ({error})",
            target.display()
        )),
    }
}

pub(super) fn find_matching_resume_project(
    root: &Path,
    input_fingerprint: &str,
) -> Result<(Option<(PathBuf, NativeGenerationState)>, Vec<String>), String> {
    let projects = root.join("projects");
    if !projects.is_dir() {
        return Ok((None, Vec::new()));
    }
    let mut warnings = Vec::new();
    let mut matches = Vec::new();
    let entries = fs::read_dir(&projects)
        .map_err(|error| format!("扫描原生断点项目失败: {} ({error})", projects.display()))?;
    for entry in entries.filter_map(Result::ok) {
        let project = entry.path();
        if !project.is_dir() || !project.join(NATIVE_STATE_FILE).is_file() {
            continue;
        }
        match read_state(&project) {
            Ok(state)
                if state.input_fingerprint == input_fingerprint
                    && state.generation_mode == "agent"
                    && state.generation_engine == "ppt_master_native"
                    && state.status != "completed" =>
            {
                matches.push((project, state));
            }
            Ok(_) => {}
            Err(error) => warnings.push(format!(
                "[Native Resume] ignoredCorruptState=true project={} error={}",
                project.display(),
                error
            )),
        }
    }
    matches.sort_by(|left, right| left.1.updated_at.cmp(&right.1.updated_at));
    Ok((matches.pop(), warnings))
}

pub(super) fn invalidate_downstream(project: &Path) -> Result<(), String> {
    for name in ["notes", "svg_final", "exports"] {
        let path = project.join(name);
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|error| {
                format!("清理失效的原生下游产物失败: {} ({error})", path.display())
            })?;
        }
        fs::create_dir_all(&path)
            .map_err(|error| format!("重建原生下游目录失败: {} ({error})", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_project(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let project = std::env::temp_dir().join(format!(
            "pomegranate_native_state_{label}_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&project).expect("create test project");
        project
    }

    fn model() -> NativeStateModel {
        NativeStateModel {
            database_id: 7,
            provider: "openai".to_string(),
            model_id: "test-model".to_string(),
        }
    }

    #[test]
    fn state_round_trip_is_complete_and_atomic_temp_is_removed() {
        let project = temp_project("round_trip");
        let mut state = NativeGenerationState::new(
            "fingerprint".to_string(),
            "测试主题".to_string(),
            6,
            model(),
            &project,
        );
        state.page_mut(1).text_geometry = Some(NativeTextGeometryState {
            passed: false,
            hard_errors: vec![serde_json::json!({
                "rule": "text_outside_declared_region",
                "text": "长标题",
                "actualBounds": { "x": 100, "y": 100, "width": 320, "height": 24 },
                "allowedBounds": { "x": 100, "y": 100, "width": 280, "height": 48 }
            })],
            warnings: vec![],
            checked_at: "2026-07-17T00:00:00Z".to_string(),
        });
        let path = write_state_atomic(&project, &state).expect("write state");
        assert_eq!(path, project.join(NATIVE_STATE_FILE));
        let loaded = read_state(&project).expect("read state");
        assert_eq!(loaded.slide_count, 6);
        assert_eq!(loaded.pages.len(), 6);
        assert_eq!(loaded.model, model());
        let geometry = loaded.pages["1"]
            .text_geometry
            .as_ref()
            .expect("text geometry state");
        assert!(!geometry.passed);
        assert_eq!(geometry.hard_errors[0]["text"], "长标题");
        assert!(!project
            .join(format!(".{NATIVE_STATE_FILE}.{}.tmp", std::process::id()))
            .exists());
        fs::remove_dir_all(project).expect("remove test project");
    }

    #[test]
    fn fingerprint_changes_when_content_or_generation_contract_changes() {
        let mut input = NativeFingerprintInput {
            topic: "主题 A".to_string(),
            prompt: "正文".to_string(),
            planning_context: "规划".to_string(),
            raw_material: "材料".to_string(),
            understanding: "理解".to_string(),
            extra_requirements: String::new(),
            audience: "评委".to_string(),
            slide_count: 6,
            style: "科技蓝".to_string(),
            custom_style: String::new(),
            visual_suggestions: "深色科技线条".to_string(),
            theme_spec: "{\"themeName\":\"tech-blue\"}".to_string(),
            mode: "technical".to_string(),
            visual_style: "dark-tech".to_string(),
            layout_bias: vec!["process".to_string()],
            chart_bias: vec!["process_flow".to_string()],
            model_database_id: 7,
            model_provider: "openai".to_string(),
            model_id: "test-model".to_string(),
            generation_mode: "agent".to_string(),
            generation_engine: "ppt_master_native".to_string(),
            generation_spec_version: NATIVE_GENERATION_SPEC_VERSION.to_string(),
            canvas: NATIVE_CANVAS.to_string(),
            max_output_tokens: 16_384,
            timeout_seconds: 300,
        };
        let first = input.fingerprint().expect("fingerprint");
        input.slide_count = 8;
        let second = input.fingerprint().expect("fingerprint");
        assert_ne!(first, second);
        input.slide_count = 6;
        input.prompt = "不同正文".to_string();
        assert_ne!(first, input.fingerprint().expect("fingerprint"));
        input.prompt = "正文".to_string();
        input.custom_style = "红色情怀".to_string();
        input.theme_spec = "{\"themeName\":\"red-heritage\"}".to_string();
        assert_ne!(first, input.fingerprint().expect("fingerprint"));
    }

    #[test]
    fn corrupted_state_returns_explicit_error() {
        let project = temp_project("corrupt");
        fs::write(project.join(NATIVE_STATE_FILE), "{broken").expect("write corrupt state fixture");
        let error = read_state(&project).expect_err("corrupt state must fail");
        assert!(error.contains("JSON 损坏"));
        fs::remove_dir_all(project).expect("remove test project");
    }

    #[test]
    fn downstream_artifacts_are_invalidated_after_upstream_change() {
        let project = temp_project("invalidate");
        for name in ["notes", "svg_final", "exports"] {
            let dir = project.join(name);
            fs::create_dir_all(&dir).expect("create downstream directory");
            fs::write(dir.join("stale.txt"), "stale").expect("write stale artifact");
        }
        invalidate_downstream(&project).expect("invalidate downstream");
        for name in ["notes", "svg_final", "exports"] {
            let dir = project.join(name);
            assert!(dir.is_dir());
            assert_eq!(fs::read_dir(dir).expect("read directory").count(), 0);
        }
        fs::remove_dir_all(project).expect("remove test project");
    }

    #[test]
    fn resume_discovery_only_reuses_exact_input_fingerprint() {
        let root = temp_project("discovery_root");
        let project = root.join("projects").join("native_failed_project");
        fs::create_dir_all(&project).expect("create failed project");
        let mut state = NativeGenerationState::new(
            "fingerprint-a".to_string(),
            "主题 A".to_string(),
            6,
            model(),
            &project,
        );
        state.set_status("failed");
        write_state_atomic(&project, &state).expect("write state");

        let (matching, warnings) =
            find_matching_resume_project(&root, "fingerprint-a").expect("find matching state");
        assert!(warnings.is_empty());
        assert_eq!(matching.expect("matching project").0, project);

        let (mismatched, warnings) =
            find_matching_resume_project(&root, "fingerprint-b").expect("scan mismatch");
        assert!(warnings.is_empty());
        assert!(mismatched.is_none());
        fs::remove_dir_all(root).expect("remove test root");
    }
}
