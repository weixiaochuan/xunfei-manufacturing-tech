use thiserror::Error;

/// 应用统一错误类型
#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON 解析错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("ZIP 错误: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("未找到: {0}")]
    NotFound(String),

    #[error("参数无效: {0}")]
    InvalidInput(String),

    #[error("插件权限拒绝: plugin={plugin_id:?} required={required_permission:?}")]
    PluginPermissionDenied {
        plugin_id: Option<String>,
        required_permission: Option<String>,
    },

    #[error("插件当前版本未声明 capability: plugin={plugin_id} capability={capability}")]
    PluginCapabilityNotDeclared {
        plugin_id: String,
        capability: String,
    },

    #[error("插件当前版本 Manifest 权限声明无效: plugin={plugin_id} reason={reason}")]
    PluginManifestCapabilityDeclarationInvalid { plugin_id: String, reason: String },

    #[error("插件当前版本权限快照缺失: plugin={plugin_id}")]
    PluginPermissionSnapshotMissing { plugin_id: String },

    #[error("插件当前版本权限快照损坏: plugin={plugin_id} version={version} reason={reason}")]
    PluginPermissionSnapshotInvalid {
        plugin_id: String,
        version: String,
        reason: String,
    },

    #[error("插件当前 Manifest 与版本权限快照语义不一致: plugin={plugin_id} version={version}")]
    PluginPermissionSnapshotMismatch { plugin_id: String, version: String },

    #[error("正式插件授权主体无效: reason={reason}")]
    PluginAuthorizationSubjectInvalid { reason: &'static str },

    #[error("正式插件授权账号不可用: reason={reason}")]
    PluginAuthorizationAccountUnavailable { reason: &'static str },

    #[error("正式插件授权账号验证失败: reason={reason}")]
    PluginAuthorizationAccountVerificationFailed { reason: &'static str },

    #[error("正式插件授权上下文无效: reason={reason}")]
    PluginAuthorizationContextInvalid { reason: &'static str },

    #[error("后端可信资源 owner 上下文无效: reason={reason}")]
    ResourceOwnerContextInvalid { reason: &'static str },

    #[error("正式插件授权 scope 无效: reason={reason}")]
    PluginAuthorizationScopeInvalid { reason: &'static str },

    #[error("capability 不允许写入正式插件授权: reason={reason}")]
    PluginAuthorizationCapabilityInvalid { reason: &'static str },

    #[error("capability 权威语义版本不可用")]
    PluginAuthorizationCapabilitySemanticVersionUnavailable,

    #[error("插件当前 Manifest 未声明正式授权 capability")]
    PluginAuthorizationManifestNotDeclared,

    #[error("插件当前版本权限快照不包含正式授权 capability")]
    PluginAuthorizationSnapshotNotDeclared,

    #[error("正式插件授权 scope 与当前请求不匹配")]
    PluginAuthorizationScopeMismatch,

    #[error("正式插件授权 capability 语义版本不匹配")]
    PluginAuthorizationSemanticVersionMismatch,

    #[error("正式插件授权记录不存在")]
    PluginAuthorizationNotFound,

    #[error("正式插件授权状态转换无效: from={from} to={to}")]
    PluginAuthorizationTransitionInvalid {
        from: &'static str,
        to: &'static str,
    },

    #[error("正式插件授权并发冲突")]
    PluginAuthorizationRevisionConflict,

    #[error("正式插件授权时间无效: field={field}")]
    PluginAuthorizationTimeInvalid { field: &'static str },

    #[error("正式插件授权存储记录损坏: reason={reason}")]
    PluginAuthorizationStoredRecordInvalid { reason: &'static str },

    #[error("{0}")]
    Custom(String),
}

/// 让 Tauri Command 能直接使用 AppError 作为错误类型
impl From<AppError> for String {
    fn from(err: AppError) -> String {
        err.to_string()
    }
}
