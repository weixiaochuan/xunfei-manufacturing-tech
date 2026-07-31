//! Manifest v3 与可安装插件平台的共享模型。
//!
//! 这些类型只描述可信边界两侧交换的数据；文件系统、数据库与运行时逻辑分别留在
//! database/service 层，避免把安全决策泄漏到 WebView。

use std::collections::BTreeMap;

use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginClassification {
    Feature,
    Enhancement,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum PluginCapability {
    Feature,
    Agent,
    View,
    Command,
    ToolProvider,
    Importer,
    Exporter,
    InputProcessor,
    ContextProvider,
    PromptEnhancer,
    OutputProcessor,
    UiContribution,
    #[serde(rename = "xingchen.workflow.invoke")]
    XingchenWorkflowInvoke,
    #[serde(rename = "file.docx.output")]
    FileDocxOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum PluginScene {
    Global,
    Learning,
    Research,
    Teaching,
}

impl PluginScene {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Learning => "learning",
            Self::Research => "research",
            Self::Teaching => "teaching",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginActivationEvent(pub String);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDefaultActivation {
    #[serde(default)]
    pub global: bool,
    #[serde(default)]
    pub scenes: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDeclarativeHandler {
    #[serde(default = "declarative_handler_kind")]
    pub kind: String,
    pub resource: String,
}

fn declarative_handler_kind() -> String {
    "declarative".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PluginExecutionMode {
    Append,
    Replace,
    Exclusive,
}

impl Default for PluginExecutionMode {
    fn default() -> Self {
        Self::Append
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum PluginEnhancementHook {
    InputProcessor,
    ContextProvider,
    PromptEnhancer,
    ToolProvider,
    OutputProcessor,
    UiContribution,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginFeatureContribution {
    /// 仅在解析后的贡献结果中回填；Manifest 中不需要声明。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub scenes: Vec<PluginScene>,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    #[serde(default)]
    pub ui_schema: Option<String>,
    #[serde(default)]
    pub handler: Option<PluginDeclarativeHandler>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAgentContribution {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub scenes: Vec<PluginScene>,
    #[serde(default)]
    pub handler: Option<PluginDeclarativeHandler>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolContribution {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub handler: Option<PluginDeclarativeHandler>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEnhancementContribution {
    pub id: String,
    pub title: String,
    pub hook: PluginEnhancementHook,
    #[serde(default)]
    pub scenes: Vec<PluginScene>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub mode: PluginExecutionMode,
    #[serde(default)]
    pub runs_before: Vec<String>,
    #[serde(default)]
    pub runs_after: Vec<String>,
    pub handler: PluginDeclarativeHandler,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributesV3 {
    #[serde(default)]
    pub features: Vec<PluginFeatureContribution>,
    #[serde(default)]
    pub agents: Vec<PluginAgentContribution>,
    #[serde(default)]
    pub commands: Vec<super::PluginCommandContribution>,
    #[serde(default)]
    pub views: Vec<super::PluginViewContribution>,
    #[serde(default)]
    pub tools: Vec<PluginToolContribution>,
    #[serde(default)]
    pub enhancements: Vec<PluginEnhancementContribution>,
    #[serde(default)]
    pub settings: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDependency {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

fn deserialize_dependencies<'de, D>(deserializer: D) -> Result<Vec<PluginDependency>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(items) => {
            serde_json::from_value(serde_json::Value::Array(items)).map_err(D::Error::custom)
        }
        // 兼容规范示例里的 `"dependencies": {}`，也支持
        // `{"plugin.id":"^1.0.0"}` 与带 required/version 的对象写法。
        serde_json::Value::Object(items) => items
            .into_iter()
            .map(|(id, detail)| match detail {
                serde_json::Value::String(version) => Ok(PluginDependency {
                    id,
                    version: Some(version),
                    required: true,
                }),
                serde_json::Value::Bool(required) => Ok(PluginDependency {
                    id,
                    version: None,
                    required,
                }),
                serde_json::Value::Object(mut fields) => {
                    fields.entry("id").or_insert(serde_json::Value::String(id));
                    serde_json::from_value(serde_json::Value::Object(fields))
                        .map_err(D::Error::custom)
                }
                _ => Err(D::Error::custom("插件依赖必须是版本字符串、布尔值或对象")),
            })
            .collect(),
        _ => Err(D::Error::custom("dependencies 必须是数组或对象")),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConflict {
    pub id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifestV3 {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub author_id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub min_app_version: Option<String>,
    pub classification: PluginClassification,
    pub runtime_kind: super::PluginRuntimeKind,
    #[serde(default)]
    pub source: super::PluginSource,
    #[serde(default)]
    pub activation_events: Vec<PluginActivationEvent>,
    #[serde(default)]
    pub supported_scenes: Vec<PluginScene>,
    #[serde(default)]
    pub default_activation: PluginDefaultActivation,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub configuration_schema: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_dependencies")]
    pub dependencies: Vec<PluginDependency>,
    #[serde(default)]
    pub conflicts_with: Vec<PluginConflict>,
    #[serde(default)]
    pub contributes: PluginContributesV3,
    #[serde(default)]
    pub integrity: super::PluginIntegrity,
    #[serde(default)]
    pub signature: super::PluginSignature,
}

/// 只读权限事实的持久化来源。
///
/// 该来源标记用于防止 Manifest、版本快照和旧布尔授权记录在调用方被误当成同一类事实。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PluginPermissionFactSource {
    CurrentManifest,
    InstalledVersionSnapshot,
    LegacyPluginPermissions,
}

/// 当前安装版本 Manifest 中声明的 capability 集合。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentManifestCapabilityDeclaration {
    pub plugin_id: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub source: PluginPermissionFactSource,
}

/// 当前安装版本保存于 `plugin_versions.permissions_json` 的不可变权限快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledVersionCapabilitySnapshot {
    pub plugin_id: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub source: PluginPermissionFactSource,
}

/// `plugin_permissions.granted` 能表达的旧兼容状态。
///
/// `NotGrantedCompatible` 只表示旧布尔值为 false；它不能被解释为用户明确 denied、
/// revoked 或 expired。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LegacyCapabilityGrantState {
    Granted,
    NotGrantedCompatible,
    Missing,
}

/// 单个 capability 的旧授权兼容事实，不代表新的正式用户授权决策。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyCapabilityAuthorizationFact {
    pub plugin_id: String,
    pub capability: String,
    pub state: LegacyCapabilityGrantState,
    pub source: PluginPermissionFactSource,
}

/// 已完成来源区分并验证 Manifest/版本快照语义一致的只读权限事实。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentPluginPermissionFacts {
    pub manifest_declaration: CurrentManifestCapabilityDeclaration,
    pub version_snapshot: InstalledVersionCapabilitySnapshot,
    pub legacy_authorizations: Vec<LegacyCapabilityAuthorizationFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginExecutionContext {
    pub scene: PluginScene,
    pub feature: String,
    #[serde(default)]
    pub user_role: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub request_id: String,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub selected_resources: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    /// 当前会话/任务覆盖；不写入数据库。
    #[serde(default)]
    pub session_overrides: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginExecutionLogInput {
    pub plugin_id: String,
    pub contribution_id: String,
    pub hook: PluginEnhancementHook,
    pub context: PluginExecutionContext,
    pub status: String,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActivationRule {
    pub plugin_id: String,
    pub scope_type: String,
    pub scope_key: String,
    pub enabled: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginVersionInfo {
    pub plugin_id: String,
    pub version: String,
    pub install_path: String,
    pub content_hash: String,
    pub is_current: bool,
    pub signature_status: super::SignatureStatus,
    pub installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginArchiveLimits {
    pub max_archive_bytes: u64,
    pub max_uncompressed_bytes: u64,
    pub max_file_bytes: u64,
    pub max_files: usize,
    pub max_depth: usize,
}

impl Default for PluginArchiveLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 100 * 1024 * 1024,
            max_uncompressed_bytes: 512 * 1024 * 1024,
            max_file_bytes: 64 * 1024 * 1024,
            max_files: 5_000,
            max_depth: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginArchiveInspection {
    pub archive_path: String,
    pub manifest: PluginManifestV3,
    pub content_hash: String,
    pub root_prefix: Option<String>,
    pub file_count: usize,
    pub uncompressed_bytes: u64,
    pub compatibility: super::PluginCompatibility,
    pub permission_diff: super::PermissionDiff,
    pub added_permissions: Vec<String>,
    pub removed_permissions: Vec<String>,
    pub conflicts: Vec<String>,
    pub missing_dependencies: Vec<String>,
    pub signature_status: super::SignatureStatus,
    pub runtime_policy: super::PluginRuntimePolicy,
    pub warnings: Vec<String>,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallArchiveInput {
    pub path: String,
    pub expected_hash: String,
    #[serde(default)]
    pub approved_permissions: Vec<String>,
    #[serde(default)]
    pub confirm_unsigned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallResult {
    pub plugin_id: String,
    pub version: String,
    pub install_path: String,
    pub previous_version: Option<String>,
    pub content_hash: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPluginContributions {
    pub context: PluginExecutionContext,
    pub active_plugins: Vec<String>,
    pub features: Vec<PluginFeatureContribution>,
    pub agents: Vec<PluginAgentContribution>,
    pub tools: Vec<PluginToolContribution>,
    pub enhancements: Vec<ResolvedEnhancementContribution>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedEnhancementContribution {
    pub plugin_id: String,
    pub contribution: PluginEnhancementContribution,
    pub resource_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginFeatureInvokeInput {
    pub plugin_id: String,
    pub feature_id: String,
    pub external_agent_id: String,
    #[serde(default)]
    pub parameters: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub file_paths: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub plugin_system_context: Option<String>,
    #[serde(default)]
    pub plugin_contribution_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginFeatureInvokeResult {
    pub ok: bool,
    pub request_id: Option<String>,
    pub content: String,
    pub output_kind: String,
    #[serde(default)]
    pub output_files: Vec<super::WorkflowGeneratedFile>,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub usage: Option<serde_json::Value>,
    pub mock: bool,
}

#[derive(Debug, Clone)]
pub struct PluginFeatureInvocationSpec {
    pub output_kind: String,
}
