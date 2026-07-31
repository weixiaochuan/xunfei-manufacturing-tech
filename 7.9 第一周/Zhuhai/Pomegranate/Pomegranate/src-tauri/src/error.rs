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

    #[error("{0}")]
    Custom(String),
}

/// 让 Tauri Command 能直接使用 AppError 作为错误类型
impl From<AppError> for String {
    fn from(err: AppError) -> String {
        err.to_string()
    }
}
