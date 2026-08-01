use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::account::AccountState;
use crate::database::Database;
use crate::error::AppError;
use crate::models::{PluginAuthorizationContext, PluginAuthorizationSubject, PluginScene};
use crate::services::hash::sha256_hex;
use crate::services::plugin_authorization_context::{
    resolve_host_installation_context, resolve_verified_platform_subject, TrustedResourceScope,
};
use crate::services::plugin_authorizations::{
    grant_for_actor_and_scope, revoke_for_actor_and_scope,
};
use crate::services::plugin_permission_guard::{
    authorize_plugin_call, resolve_current_plugin_context, GuardRateLimit, TrustedPluginCall,
};
use crate::services::plugin_rate_limit::PluginRateLimiter;

const SELECTION_LIFETIME_MINUTES: i64 = 10;
const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct SelectedFileExportRegistry {
    entries: Mutex<HashMap<String, SelectedFileExport>>,
}

#[derive(Clone)]
struct SelectedFileExport {
    subject: PluginAuthorizationSubject,
    context: PluginAuthorizationContext,
    plugin_id: String,
    plugin_version: String,
    feature_id: String,
    feature_fingerprint: String,
    target: PathBuf,
    target_fingerprint: String,
    allowed_extension: String,
    allow_overwrite: bool,
    expires_at: DateTime<Utc>,
    consumed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectedFileExportView {
    pub selection_handle: String,
    pub target_name: String,
    pub allowed_extension: String,
    pub allow_overwrite: bool,
    pub expires_at: String,
}

pub(crate) struct PluginFileExportService;

impl PluginFileExportService {
    pub(crate) fn suggested_file_name(
        db: &Database,
        plugin_id: &str,
        feature_id: &str,
    ) -> Result<String, AppError> {
        let current = resolve_current_plugin_context(db, plugin_id)?;
        let feature = current
            .manifest
            .contributes
            .features
            .iter()
            .find(|feature| feature.id == feature_id)
            .ok_or_else(|| AppError::InvalidInput("插件功能不存在或不可访问".into()))?;
        let ui_schema = feature
            .ui_schema
            .as_deref()
            .ok_or_else(|| AppError::InvalidInput("feature 缺少 uiSchema".into()))?;
        let schema: serde_json::Value = serde_json::from_str(&fs::read_to_string(
            Path::new(&current.install_path).join(ui_schema),
        )?)?;
        let output_kind = schema
            .pointer("/output/kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("text");
        match output_kind {
            "docx-base64" => Ok("workflow-result.docx".into()),
            "file-base64" => Ok("workflow-result.bin".into()),
            _ => Err(AppError::InvalidInput("当前功能不产生可导出的文件".into())),
        }
    }

    pub(crate) async fn issue_selection(
        registry: &SelectedFileExportRegistry,
        db: &Database,
        account: &AccountState,
        plugin_id: &str,
        feature_id: &str,
        selected_target: &Path,
    ) -> Result<SelectedFileExportView, AppError> {
        let subject = resolve_verified_platform_subject(account).await?;
        let context = resolve_host_installation_context(db)?;
        let current = resolve_current_plugin_context(db, plugin_id)?;
        Self::suggested_file_name(db, plugin_id, feature_id)?;
        let feature = current
            .manifest
            .contributes
            .features
            .iter()
            .find(|feature| feature.id == feature_id)
            .ok_or_else(|| AppError::InvalidInput("插件功能不存在或不可访问".into()))?;
        let feature_fingerprint = sha256_hex(&serde_json::to_string(&(
            "selected-file-export-v1",
            &current.manifest.version,
            feature,
        ))?);
        let (target, extension) = validate_new_file_target(selected_target)?;
        let target_fingerprint = sha256_hex(target.to_string_lossy().as_ref());
        let expires_at = Utc::now() + Duration::minutes(SELECTION_LIFETIME_MINUTES);
        // Two independent UUIDv4 values keep the opaque handle unpredictable while revealing no path.
        let handle = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let entry = SelectedFileExport {
            subject,
            context,
            plugin_id: plugin_id.to_string(),
            plugin_version: current.version,
            feature_id: feature_id.to_string(),
            feature_fingerprint,
            target: target.clone(),
            target_fingerprint,
            allowed_extension: extension.clone(),
            allow_overwrite: false,
            expires_at,
            consumed: false,
        };
        registry
            .entries
            .lock()
            .map_err(|_| AppError::Custom("selected_file_registry_unavailable".into()))?
            .insert(handle.clone(), entry);
        Ok(SelectedFileExportView {
            selection_handle: handle,
            target_name: target
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("selected-file")
                .to_string(),
            allowed_extension: extension,
            allow_overwrite: false,
            expires_at: expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        })
    }

    pub(crate) async fn grant(
        registry: &SelectedFileExportRegistry,
        db: &Database,
        account: &AccountState,
        plugin_id: &str,
        feature_id: &str,
        handle: &str,
    ) -> Result<(), AppError> {
        let (selection, scope) =
            resolve_active(registry, db, account, plugin_id, feature_id, handle).await?;
        db.write_audit_log(
            plugin_id,
            "selected_file_authorization_grant_attempt",
            Some("files.writeSelected:user-selected-resource"),
        )?;
        grant_for_actor_and_scope(
            db,
            &selection.subject,
            &selection.context,
            plugin_id,
            "files.writeSelected",
            &scope,
            Some(
                selection
                    .expires_at
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
            ),
        )?;
        db.write_audit_log(
            plugin_id,
            "selected_file_authorization_grant_completed",
            Some("files.writeSelected:user-selected-resource"),
        )
        .ok();
        Ok(())
    }

    pub(crate) async fn revoke(
        registry: &SelectedFileExportRegistry,
        db: &Database,
        account: &AccountState,
        plugin_id: &str,
        feature_id: &str,
        handle: &str,
    ) -> Result<(), AppError> {
        let (selection, scope) =
            resolve_active(registry, db, account, plugin_id, feature_id, handle).await?;
        db.write_audit_log(
            plugin_id,
            "selected_file_authorization_revoke_attempt",
            Some("files.writeSelected:user-selected-resource"),
        )?;
        revoke_for_actor_and_scope(
            db,
            &selection.subject,
            &selection.context,
            plugin_id,
            "files.writeSelected",
            &scope,
        )?;
        db.write_audit_log(
            plugin_id,
            "selected_file_authorization_revoke_completed",
            Some("files.writeSelected:user-selected-resource"),
        )
        .ok();
        Ok(())
    }

    pub(crate) async fn preflight_authorized(
        registry: &SelectedFileExportRegistry,
        db: &Database,
        account: &AccountState,
        limiter: &PluginRateLimiter,
        plugin_id: &str,
        feature_id: &str,
        handle: &str,
    ) -> Result<(), AppError> {
        let (_, scope) =
            resolve_active(registry, db, account, plugin_id, feature_id, handle).await?;
        authorize_selected_file(
            db,
            account,
            limiter,
            plugin_id,
            feature_id,
            scope,
            "selected-file-preflight",
        )
        .await
    }

    pub(crate) async fn write_authorized(
        registry: &SelectedFileExportRegistry,
        db: &Database,
        account: &AccountState,
        limiter: &PluginRateLimiter,
        plugin_id: &str,
        feature_id: &str,
        handle: &str,
        file_name: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<crate::models::WorkflowGeneratedFile, AppError> {
        if bytes.len() > MAX_EXPORT_BYTES {
            return Err(AppError::InvalidInput("文件输出超过大小限制".into()));
        }
        let (selection, scope) =
            resolve_active(registry, db, account, plugin_id, feature_id, handle).await?;
        validate_payload_binding(&selection, file_name, bytes)?;
        authorize_selected_file(
            db,
            account,
            limiter,
            plugin_id,
            feature_id,
            scope,
            "selected-file-write",
        )
        .await?;

        // The target comes only from the native selection registry. Guard and validation complete
        // before any create, truncate, replace, or write operation is attempted.
        atomic_write_new(&selection.target, bytes)?;
        consume(registry, handle)?;
        Ok(crate::models::WorkflowGeneratedFile {
            file_name: selection
                .target
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(file_name)
                .to_string(),
            path: selection.target.to_string_lossy().to_string(),
            size: bytes.len() as u64,
            content_type: content_type.to_string(),
        })
    }
}

async fn authorize_selected_file(
    db: &Database,
    account: &AccountState,
    limiter: &PluginRateLimiter,
    plugin_id: &str,
    feature_id: &str,
    scope: TrustedResourceScope,
    request_prefix: &str,
) -> Result<(), AppError> {
    let current = resolve_current_plugin_context(db, plugin_id)?;
    let feature = current
        .manifest
        .contributes
        .features
        .iter()
        .find(|feature| feature.id == feature_id)
        .ok_or_else(|| AppError::InvalidInput("插件功能不存在或不可访问".into()))?;
    let scene = feature
        .scenes
        .first()
        .cloned()
        .unwrap_or(PluginScene::Global);
    let call = TrustedPluginCall::internal(
        plugin_id,
        "files.writeSelected",
        Some(current.version),
        scene,
        format!("{request_prefix}-{}", Uuid::new_v4()),
        scope,
    );
    let call = if request_prefix == "selected-file-write" {
        call.with_rate_limit(GuardRateLimit::Write)
    } else {
        call
    };
    authorize_plugin_call(db, account, limiter, call)
        .await
        .map(|_| ())
        .map_err(|_| AppError::PluginPermissionDenied {
            plugin_id: Some(plugin_id.to_string()),
            required_permission: Some("files.writeSelected".into()),
        })
}

async fn resolve_active(
    registry: &SelectedFileExportRegistry,
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    feature_id: &str,
    handle: &str,
) -> Result<(SelectedFileExport, TrustedResourceScope), AppError> {
    if handle.len() != 64 || !handle.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(inaccessible());
    }
    let selection = registry
        .entries
        .lock()
        .map_err(|_| AppError::Custom("selected_file_registry_unavailable".into()))?
        .get(handle)
        .cloned()
        .ok_or_else(inaccessible)?;
    let subject = resolve_verified_platform_subject(account).await?;
    let context = resolve_host_installation_context(db)?;
    let current = resolve_current_plugin_context(db, plugin_id)?;
    let feature = current
        .manifest
        .contributes
        .features
        .iter()
        .find(|feature| feature.id == feature_id)
        .ok_or_else(inaccessible)?;
    let current_fingerprint = sha256_hex(&serde_json::to_string(&(
        "selected-file-export-v1",
        &current.manifest.version,
        feature,
    ))?);
    if selection.consumed
        || selection.expires_at <= Utc::now()
        || selection.subject != subject
        || selection.context != context
        || selection.plugin_id != plugin_id
        || selection.plugin_version != current.version
        || selection.feature_id != feature_id
        || selection.feature_fingerprint != current_fingerprint
    {
        return Err(inaccessible());
    }
    let scope = TrustedResourceScope::for_selected_file_export(
        &current.manifest,
        feature_id,
        &selection.target_fingerprint,
        &selection.allowed_extension,
        selection.allow_overwrite,
    )?;
    Ok((selection, scope))
}

fn validate_new_file_target(target: &Path) -> Result<(PathBuf, String), AppError> {
    if !target.is_absolute()
        || target
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(inaccessible());
    }
    if target.exists() {
        return Err(AppError::InvalidInput(
            "当前导出仅支持新建文件，不能覆盖现有文件".into(),
        ));
    }
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(inaccessible)?;
    validate_windows_file_name(name)?;
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::InvalidInput("请选择带有效扩展名的目标文件".into()))?;
    if !extension.bytes().all(|byte| byte.is_ascii_alphanumeric()) || extension.len() > 12 {
        return Err(AppError::InvalidInput("目标文件扩展名无效".into()));
    }
    let parent = target.parent().ok_or_else(inaccessible)?;
    let canonical_parent = fs::canonicalize(parent)?;
    if !canonical_parent.is_dir()
        || fs::symlink_metadata(&canonical_parent)?
            .file_type()
            .is_symlink()
    {
        return Err(inaccessible());
    }
    Ok((canonical_parent.join(name), extension))
}

fn validate_windows_file_name(name: &str) -> Result<(), AppError> {
    if name.is_empty()
        || name.ends_with('.')
        || name.ends_with(' ')
        || name
            .chars()
            .any(|ch| ch.is_control() || "<>:\"/\\|?*".contains(ch))
    {
        return Err(AppError::InvalidInput("目标文件名无效".into()));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        return Err(AppError::InvalidInput("目标文件名为系统保留名称".into()));
    }
    Ok(())
}

fn validate_payload_binding(
    selection: &SelectedFileExport,
    file_name: &str,
    bytes: &[u8],
) -> Result<(), AppError> {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(inaccessible)?;
    if extension != selection.allowed_extension {
        return Err(AppError::InvalidInput(
            "Workflow 输出扩展名与用户选择不一致".into(),
        ));
    }
    if extension == "docx" && !bytes.starts_with(b"PK") {
        return Err(AppError::InvalidInput(
            "Workflow 输出不是有效的 DOCX 数据".into(),
        ));
    }
    Ok(())
}

fn atomic_write_new(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if target.exists() {
        return Err(AppError::InvalidInput(
            "目标文件已存在，未获覆盖确认".into(),
        ));
    }
    let parent = target.parent().ok_or_else(inaccessible)?;
    let current_parent = fs::canonicalize(parent)?;
    if current_parent != parent || fs::symlink_metadata(parent)?.file_type().is_symlink() {
        return Err(inaccessible());
    }
    let temp = parent.join(format!(".pomegranate-export-{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<(), AppError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::hard_link(&temp, target)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temp);
    result
}

fn consume(registry: &SelectedFileExportRegistry, handle: &str) -> Result<(), AppError> {
    let mut entries = registry
        .entries
        .lock()
        .map_err(|_| AppError::Custom("selected_file_registry_unavailable".into()))?;
    let selection = entries.get_mut(handle).ok_or_else(inaccessible)?;
    selection.consumed = true;
    Ok(())
}

fn inaccessible() -> AppError {
    AppError::InvalidInput("文件选择不存在或不可访问".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PluginManifestV3;
    use crate::services::plugins::PluginService;

    fn installed_file_plugin() -> (Database, PathBuf, PluginManifestV3, AccountState) {
        let db = Database::init(":memory:").expect("database");
        let directory = std::env::temp_dir().join(format!("selected-plugin-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("plugin dir");
        fs::write(
            directory.join("ui.json"),
            r#"{"output":{"kind":"docx-base64"}}"#,
        )
        .expect("schema");
        let manifest: PluginManifestV3 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.selected-export-test",
            "name": "Selected Export Test",
            "version": "1.0.0",
            "authorId": "firstwork-tests",
            "classification": "feature",
            "runtimeKind": "xingchen-workflow",
            "source": "local",
            "supportedScenes": ["global"],
            "permissions": [
                "credentials.use", "agents.invoke", "network.xingchen", "ai.invoke",
                "files.writeSelected"
            ],
            "contributes": {"features": [{
                "id": "export-feature",
                "title": "Export Feature",
                "scenes": ["global"],
                "uiSchema": "ui.json"
            }]}
        }))
        .expect("manifest");
        let hash = PluginService::calculate_integrity_for_path(&directory).expect("integrity");
        db.record_plugin_version(
            &manifest,
            &directory.to_string_lossy(),
            &hash,
            &manifest.permissions,
        )
        .expect("install");
        db.set_plugin_enabled(&manifest.id, true).expect("enable");
        (
            db,
            directory,
            manifest,
            AccountState::verified_test_session("selected-export-user"),
        )
    }

    #[test]
    fn target_validation_rejects_existing_parent_escape_and_reserved_names() {
        let dir = std::env::temp_dir().join(format!("selected-export-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("tempdir");
        let valid = dir.join("result.docx");
        let (target, extension) = validate_new_file_target(&valid).expect("valid target");
        assert_eq!(target, fs::canonicalize(&dir).unwrap().join("result.docx"));
        assert_eq!(extension, "docx");

        fs::write(dir.join("existing.docx"), b"old").expect("fixture");
        assert!(validate_new_file_target(&dir.join("existing.docx")).is_err());
        assert!(validate_new_file_target(&dir.join("..\\escape.docx")).is_err());
        assert!(validate_new_file_target(&dir.join("CON.docx")).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn payload_binding_rejects_extension_and_double_extension_mismatch() {
        let selection = SelectedFileExport {
            subject: PluginAuthorizationSubject {
                kind: crate::models::PluginAuthorizationSubjectKind::PlatformUser,
                id: "user".into(),
            },
            context: PluginAuthorizationContext {
                kind: crate::models::PluginAuthorizationContextKind::HostInstallation,
                id: "installation".into(),
            },
            plugin_id: "plugin".into(),
            plugin_version: "1.0.0".into(),
            feature_id: "feature".into(),
            feature_fingerprint: "fingerprint".into(),
            target: PathBuf::from("result.docx"),
            target_fingerprint: "target".into(),
            allowed_extension: "docx".into(),
            allow_overwrite: false,
            expires_at: Utc::now() + Duration::minutes(1),
            consumed: false,
        };
        assert!(validate_payload_binding(&selection, "result.docx", b"PKdata").is_ok());
        assert!(validate_payload_binding(&selection, "result.pdf", b"data").is_err());
        assert!(validate_payload_binding(&selection, "result.docx.exe", b"data").is_err());
    }

    #[test]
    fn atomic_new_write_never_truncates_an_existing_target() {
        let dir = std::env::temp_dir().join(format!("selected-export-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("tempdir");
        let target = dir.join("result.bin");
        fs::write(&target, b"original").expect("fixture");
        assert!(atomic_write_new(&target, b"replacement").is_err());
        assert_eq!(fs::read(&target).expect("read"), b"original");
        assert_eq!(fs::read_dir(&dir).expect("entries").count(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn formal_selected_scope_guards_writes_revocation_and_handle_bindings() {
        let (db, plugin_dir, manifest, account) = installed_file_plugin();
        let export_dir = std::env::temp_dir().join(format!("selected-target-{}", Uuid::new_v4()));
        fs::create_dir_all(&export_dir).expect("export dir");
        let registry = SelectedFileExportRegistry::default();
        let limiter = PluginRateLimiter::new();
        let target = export_dir.join("result.docx");
        let selected = PluginFileExportService::issue_selection(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            &target,
        )
        .await
        .expect("native selection boundary");

        assert!(PluginFileExportService::grant(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            "not-a-real-handle",
        )
        .await
        .is_err());
        assert!(PluginFileExportService::grant(
            &registry,
            &db,
            &account,
            &manifest.id,
            "other-feature",
            &selected.selection_handle,
        )
        .await
        .is_err());
        assert!(PluginFileExportService::grant(
            &registry,
            &db,
            &account,
            "com.firstwork.other-plugin",
            "export-feature",
            &selected.selection_handle,
        )
        .await
        .is_err());

        assert!(PluginFileExportService::preflight_authorized(
            &registry,
            &db,
            &account,
            &limiter,
            &manifest.id,
            "export-feature",
            &selected.selection_handle,
        )
        .await
        .is_err());
        assert!(PluginFileExportService::grant(
            &registry,
            &db,
            &AccountState::verified_test_session("other-user"),
            &manifest.id,
            "export-feature",
            &selected.selection_handle,
        )
        .await
        .is_err());

        PluginFileExportService::grant(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            &selected.selection_handle,
        )
        .await
        .expect("formal exact grant");
        PluginFileExportService::write_authorized(
            &registry,
            &db,
            &account,
            &limiter,
            &manifest.id,
            "export-feature",
            &selected.selection_handle,
            "result.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            b"PK\x03\x04document",
        )
        .await
        .expect("guarded write");
        assert_eq!(
            fs::read(&target).expect("written target"),
            b"PK\x03\x04document"
        );
        assert!(PluginFileExportService::preflight_authorized(
            &registry,
            &db,
            &account,
            &limiter,
            &manifest.id,
            "export-feature",
            &selected.selection_handle,
        )
        .await
        .is_err());

        let revoke_target = export_dir.join("revoked.docx");
        let revoked = PluginFileExportService::issue_selection(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            &revoke_target,
        )
        .await
        .expect("second selection");
        PluginFileExportService::grant(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            &revoked.selection_handle,
        )
        .await
        .expect("grant before revoke");
        PluginFileExportService::revoke(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            &revoked.selection_handle,
        )
        .await
        .expect("revoke");
        assert!(PluginFileExportService::preflight_authorized(
            &registry,
            &db,
            &account,
            &limiter,
            &manifest.id,
            "export-feature",
            &revoked.selection_handle,
        )
        .await
        .is_err());
        assert!(!revoke_target.exists());

        let expired_target = export_dir.join("expired.docx");
        let expired = PluginFileExportService::issue_selection(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            &expired_target,
        )
        .await
        .expect("expiring selection");
        registry
            .entries
            .lock()
            .unwrap()
            .get_mut(&expired.selection_handle)
            .unwrap()
            .expires_at = Utc::now() - Duration::seconds(1);
        assert!(PluginFileExportService::grant(
            &registry,
            &db,
            &account,
            &manifest.id,
            "export-feature",
            &expired.selection_handle,
        )
        .await
        .is_err());

        let _ = fs::remove_dir_all(plugin_dir);
        let _ = fs::remove_dir_all(export_dir);
    }
}
