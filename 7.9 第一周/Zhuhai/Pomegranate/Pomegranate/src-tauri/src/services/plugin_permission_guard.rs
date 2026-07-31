//! Manifest v3 插件 capability 的唯一 Rust 后端授权求值入口。

use serde::Serialize;

use crate::account::AccountState;
use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    CurrentPluginCapabilityAuthorizationStatus, PluginAuthorizationContext,
    PluginAuthorizationSubject, PluginManifestV3, PluginScene, PluginSource,
};

use super::plugin_authorization_context::{
    resolve_host_installation_context, resolve_verified_platform_subject,
};
use super::plugin_authorizations::list_current_formal_plugin_capability_authorizations_for_actor;
use super::plugin_capabilities::{canonical_capability_policy, is_v3_permission_runtime_allowed};
use super::plugin_rate_limit::PluginRateLimiter;
use super::plugins::PluginService;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PluginGuardDenyCode {
    PluginNotFound,
    InstallStateDenied,
    PluginDisabled,
    PluginBlocked,
    PluginRevoked,
    PluginUninstalled,
    PluginUninstalling,
    PluginDeactivating,
    VersionNotCurrent,
    VersionRevoked,
    TrustStateUnavailable,
    IntegrityUnavailable,
    IntegrityFailed,
    CapabilityUnknown,
    CapabilityBlocked,
    CapabilityReserved,
    CapabilityLegacy,
    CapabilityDeprecated,
    RuntimeCapabilityDenied,
    SourceCapabilityDenied,
    ManifestNotDeclared,
    SnapshotNotDeclared,
    AuthorizationMissing,
    AuthorizationPending,
    AuthorizationDenied,
    AuthorizationRevoked,
    AuthorizationExpired,
    ScopeMismatch,
    SemanticVersionMismatch,
    SceneMismatch,
    TokenContextUnsupported,
    CallContextUntrusted,
    RateLimitExceeded,
    StateReadFailed,
    StoredDataInvalid,
    AuditWriteFailed,
}

impl PluginGuardDenyCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::PluginNotFound => "plugin_not_found",
            Self::InstallStateDenied => "install_state_denied",
            Self::PluginDisabled => "plugin_disabled",
            Self::PluginBlocked => "plugin_blocked",
            Self::PluginRevoked => "plugin_revoked",
            Self::PluginUninstalled => "plugin_uninstalled",
            Self::PluginUninstalling => "plugin_uninstalling",
            Self::PluginDeactivating => "plugin_deactivating",
            Self::VersionNotCurrent => "version_not_current",
            Self::VersionRevoked => "version_revoked",
            Self::TrustStateUnavailable => "trust_state_unavailable",
            Self::IntegrityUnavailable => "integrity_unavailable",
            Self::IntegrityFailed => "integrity_failed",
            Self::CapabilityUnknown => "capability_unknown",
            Self::CapabilityBlocked => "capability_blocked",
            Self::CapabilityReserved => "capability_reserved",
            Self::CapabilityLegacy => "capability_legacy",
            Self::CapabilityDeprecated => "capability_deprecated",
            Self::RuntimeCapabilityDenied => "runtime_capability_denied",
            Self::SourceCapabilityDenied => "source_capability_denied",
            Self::ManifestNotDeclared => "manifest_not_declared",
            Self::SnapshotNotDeclared => "snapshot_not_declared",
            Self::AuthorizationMissing => "authorization_missing",
            Self::AuthorizationPending => "authorization_pending",
            Self::AuthorizationDenied => "authorization_denied",
            Self::AuthorizationRevoked => "authorization_revoked",
            Self::AuthorizationExpired => "authorization_expired",
            Self::ScopeMismatch => "scope_mismatch",
            Self::SemanticVersionMismatch => "semantic_version_mismatch",
            Self::SceneMismatch => "scene_mismatch",
            Self::TokenContextUnsupported => "token_context_unsupported",
            Self::CallContextUntrusted => "call_context_untrusted",
            Self::RateLimitExceeded => "rate_limit_exceeded",
            Self::StateReadFailed => "state_read_failed",
            Self::StoredDataInvalid => "stored_data_invalid",
            Self::AuditWriteFailed => "audit_write_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginGuardDeny {
    pub code: PluginGuardDenyCode,
    pub safe_message: &'static str,
    pub internal_diagnostic: String,
    pub correlation_id: String,
    pub audited: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthorizedPluginContext {
    pub manifest: PluginManifestV3,
    pub version: String,
    pub install_path: String,
    pub correlation_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardRateLimit {
    None,
    Write,
    Ai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardTokenRequirement {
    NotUsed,
    Required,
}

/// 字段私有，IPC 参数不能直接反序列化为可信授权上下文。
pub(crate) struct TrustedPluginCall {
    plugin_id: String,
    capability_id: String,
    expected_version: Option<String>,
    scene: PluginScene,
    correlation_id: String,
    token_requirement: GuardTokenRequirement,
    rate_limit: GuardRateLimit,
}

impl TrustedPluginCall {
    pub(super) fn internal(
        plugin_id: impl Into<String>,
        capability_id: impl Into<String>,
        expected_version: Option<String>,
        scene: PluginScene,
        correlation_id: impl Into<String>,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            capability_id: capability_id.into(),
            expected_version,
            scene,
            correlation_id: correlation_id.into(),
            token_requirement: GuardTokenRequirement::NotUsed,
            rate_limit: GuardRateLimit::None,
        }
    }

    pub(super) fn require_token(mut self) -> Self {
        self.token_requirement = GuardTokenRequirement::Required;
        self
    }

    pub(super) fn with_rate_limit(mut self, limit: GuardRateLimit) -> Self {
        self.rate_limit = limit;
        self
    }
}

/// 生产入口：主体和 host context 均由 Rust 后端解析，不接受前端身份事实。
pub(crate) async fn authorize_plugin_call(
    db: &Database,
    account: &AccountState,
    limiter: &PluginRateLimiter,
    call: TrustedPluginCall,
) -> Result<AuthorizedPluginContext, PluginGuardDeny> {
    let subject = match resolve_verified_platform_subject(account).await {
        Ok(subject) => subject,
        Err(error) => return Err(audit_untrusted_context_failure(db, &call, error)),
    };
    let context = match resolve_host_installation_context(db) {
        Ok(context) => context,
        Err(error) => return Err(audit_untrusted_context_failure(db, &call, error)),
    };
    authorize_resolved_plugin_call(
        db,
        limiter,
        &ResolvedAuthorizationActor { subject, context },
        call,
    )
}

/// actor 字段只在本模块由可信后端解析；Command 无法自行构造该类型。
struct ResolvedAuthorizationActor {
    subject: PluginAuthorizationSubject,
    context: PluginAuthorizationContext,
}

fn authorize_resolved_plugin_call(
    db: &Database,
    limiter: &PluginRateLimiter,
    actor: &ResolvedAuthorizationActor,
    call: TrustedPluginCall,
) -> Result<AuthorizedPluginContext, PluginGuardDeny> {
    let result = evaluate(db, limiter, &actor.subject, &actor.context, &call);
    let (decision, diagnostic) = match &result {
        Ok(_) => ("allow", None),
        Err(deny) => ("deny", Some(deny.code.as_str())),
    };
    let verified_version = result.as_ref().ok().map(|allowed| allowed.version.as_str());
    let audit = write_decision_audit(
        db,
        &actor.subject,
        &actor.context,
        &call,
        verified_version,
        decision,
        diagnostic,
    );
    match (result, audit) {
        (Ok(mut allowed), Ok(())) => {
            allowed.correlation_id = call.correlation_id;
            Ok(allowed)
        }
        (Err(mut denied), Ok(())) => {
            denied.audited = true;
            Err(denied)
        }
        (_, Err(error)) => Err(PluginGuardDeny {
            code: PluginGuardDenyCode::AuditWriteFailed,
            safe_message: "授权审计暂不可用",
            internal_diagnostic: error.to_string(),
            correlation_id: call.correlation_id,
            audited: false,
        }),
    }
}

fn evaluate(
    db: &Database,
    limiter: &PluginRateLimiter,
    subject: &PluginAuthorizationSubject,
    host_context: &PluginAuthorizationContext,
    call: &TrustedPluginCall,
) -> Result<AuthorizedPluginContext, PluginGuardDeny> {
    if call.plugin_id.is_empty() || call.capability_id.is_empty() || call.correlation_id.is_empty()
    {
        return Err(deny(
            call,
            PluginGuardDenyCode::CallContextUntrusted,
            "empty trusted call field",
        ));
    }
    if call.token_requirement == GuardTokenRequirement::Required {
        // 现有 token 仅绑定 plugin_id，不具备 A3 所需的版本/session/expiry 证据。
        return Err(deny(
            call,
            PluginGuardDenyCode::TokenContextUnsupported,
            "legacy token has no version/session/expiry binding",
        ));
    }

    let snapshot = db
        .current_plugin_authorization_snapshot(&call.plugin_id, &[])
        .map_err(|error| mapped_error(call, error))?
        .ok_or_else(|| {
            deny(
                call,
                PluginGuardDenyCode::PluginNotFound,
                "plugin row missing",
            )
        })?;
    match snapshot.status.as_str() {
        "installed" => {}
        "blocked" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::PluginBlocked,
                "status=blocked",
            ))
        }
        "revoked" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::PluginRevoked,
                "status=revoked",
            ))
        }
        "uninstalled" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::PluginUninstalled,
                "status=uninstalled",
            ))
        }
        "uninstalling" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::PluginUninstalling,
                "status=uninstalling",
            ))
        }
        "deactivating" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::PluginDeactivating,
                &format!("status={}", snapshot.status),
            ))
        }
        _ => {
            return Err(deny(
                call,
                PluginGuardDenyCode::InstallStateDenied,
                &format!("status={}", snapshot.status),
            ))
        }
    }
    if !snapshot.enabled {
        return Err(deny(
            call,
            PluginGuardDenyCode::PluginDisabled,
            "enabled=false",
        ));
    }
    let current = snapshot.current_version.ok_or_else(|| {
        deny(
            call,
            PluginGuardDenyCode::VersionNotCurrent,
            "current version missing",
        )
    })?;
    if current.manifest.id != call.plugin_id || current.manifest.version != current.version {
        return Err(deny(
            call,
            PluginGuardDenyCode::StoredDataInvalid,
            "manifest identity/version mismatch",
        ));
    }
    if call
        .expected_version
        .as_deref()
        .is_some_and(|version| version != current.version)
    {
        return Err(deny(
            call,
            PluginGuardDenyCode::VersionNotCurrent,
            "caller version differs from database current",
        ));
    }

    let integrity = PluginService::verify_installation(db, &call.plugin_id).map_err(|error| {
        deny(
            call,
            PluginGuardDenyCode::IntegrityUnavailable,
            &error.to_string(),
        )
    })?;
    if integrity.expected_hash.is_empty() || integrity.actual_hash.is_empty() {
        return Err(deny(
            call,
            PluginGuardDenyCode::IntegrityUnavailable,
            "integrity hash missing",
        ));
    }
    if !integrity.ok {
        return Err(deny(
            call,
            PluginGuardDenyCode::IntegrityFailed,
            "installed content hash mismatch",
        ));
    }

    let policy = canonical_capability_policy(&call.capability_id)
        .map_err(|error| {
            deny(
                call,
                PluginGuardDenyCode::StoredDataInvalid,
                &error.to_string(),
            )
        })?
        .ok_or_else(|| {
            deny(
                call,
                PluginGuardDenyCode::CapabilityUnknown,
                "capability absent from registry",
            )
        })?;
    match policy.status.as_str() {
        "active" | "restricted" => {}
        "blocked" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::CapabilityBlocked,
                "registry status blocked",
            ))
        }
        "reserved" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::CapabilityReserved,
                "registry status reserved",
            ))
        }
        "legacy" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::CapabilityLegacy,
                "registry status legacy",
            ))
        }
        "deprecated" => {
            return Err(deny(
                call,
                PluginGuardDenyCode::CapabilityDeprecated,
                "registry status deprecated",
            ))
        }
        _ => {
            return Err(deny(
                call,
                PluginGuardDenyCode::StoredDataInvalid,
                "unknown registry status",
            ))
        }
    }
    let runtime = enum_string(&current.manifest.runtime_kind);
    if !is_v3_permission_runtime_allowed(&call.capability_id, &runtime)
        || !policy.runtime_kinds.contains(&runtime)
    {
        return Err(deny(
            call,
            PluginGuardDenyCode::RuntimeCapabilityDenied,
            "runtimeKind is not admitted",
        ));
    }
    let source = enum_string(&current.manifest.source);
    if !policy.plugin_sources.contains(&source) {
        return Err(deny(
            call,
            PluginGuardDenyCode::SourceCapabilityDenied,
            "plugin source is not admitted",
        ));
    }
    if current.manifest.source == PluginSource::Marketplace {
        let state = db
            .marketplace_plugin_security_state(&call.plugin_id)
            .map_err(|error| mapped_error(call, error))?
            .ok_or_else(|| {
                deny(
                    call,
                    PluginGuardDenyCode::TrustStateUnavailable,
                    "marketplace installation binding missing",
                )
            })?;
        match state.product_status.as_str() {
            "revoked" | "suspended" | "delisted" => {
                return Err(deny(
                    call,
                    PluginGuardDenyCode::PluginRevoked,
                    "marketplace product is not runnable",
                ))
            }
            "published" | "installed" | "active" => {}
            _ => {
                return Err(deny(
                    call,
                    PluginGuardDenyCode::TrustStateUnavailable,
                    "marketplace product status is not runnable",
                ))
            }
        }
        match state.installation_status.as_str() {
            "installed" => {}
            "uninstalling" => {
                return Err(deny(
                    call,
                    PluginGuardDenyCode::PluginUninstalling,
                    "marketplace installation is uninstalling",
                ))
            }
            "uninstalled" => {
                return Err(deny(
                    call,
                    PluginGuardDenyCode::PluginUninstalled,
                    "marketplace installation is uninstalled",
                ))
            }
            _ => {
                return Err(deny(
                    call,
                    PluginGuardDenyCode::TrustStateUnavailable,
                    "marketplace installation status is not runnable",
                ))
            }
        }
        if state.installed_version != current.version {
            return Err(deny(
                call,
                PluginGuardDenyCode::VersionNotCurrent,
                "marketplace installed version differs from current version",
            ));
        }
        if state.version_status == "revoked" || state.signature_status == "revoked" {
            return Err(deny(
                call,
                PluginGuardDenyCode::VersionRevoked,
                "marketplace version or signature revoked",
            ));
        }
        if state.version_status != "active" || state.signature_status.is_empty() {
            return Err(deny(
                call,
                PluginGuardDenyCode::TrustStateUnavailable,
                "marketplace version trust state unavailable",
            ));
        }
    }
    if !current.manifest.permissions.contains(&call.capability_id) {
        return Err(deny(
            call,
            PluginGuardDenyCode::ManifestNotDeclared,
            "manifest does not declare capability",
        ));
    }
    let (manifest_fact, snapshot_fact) = db
        .current_plugin_capability_contract(&call.plugin_id)
        .map_err(|error| mapped_error(call, error))?;
    if !manifest_fact.capabilities.contains(&call.capability_id) {
        return Err(deny(
            call,
            PluginGuardDenyCode::ManifestNotDeclared,
            "manifest fact does not declare capability",
        ));
    }
    if !snapshot_fact.capabilities.contains(&call.capability_id) {
        return Err(deny(
            call,
            PluginGuardDenyCode::SnapshotNotDeclared,
            "version snapshot does not contain capability",
        ));
    }
    if !current.manifest.supported_scenes.contains(&call.scene)
        && !current
            .manifest
            .supported_scenes
            .contains(&PluginScene::Global)
    {
        return Err(deny(
            call,
            PluginGuardDenyCode::SceneMismatch,
            "scene is not supported",
        ));
    }

    let authorizations = list_current_formal_plugin_capability_authorizations_for_actor(
        db,
        subject,
        host_context,
        &call.plugin_id,
    )
    .map_err(|error| mapped_error(call, error))?;
    let authorization = authorizations
        .iter()
        .find(|item| item.capability_id == call.capability_id)
        .ok_or_else(|| {
            deny(
                call,
                PluginGuardDenyCode::AuthorizationMissing,
                "formal authorization view missing",
            )
        })?;
    let code = match authorization.status {
        CurrentPluginCapabilityAuthorizationStatus::Missing => {
            Some(PluginGuardDenyCode::AuthorizationMissing)
        }
        CurrentPluginCapabilityAuthorizationStatus::Pending => {
            Some(PluginGuardDenyCode::AuthorizationPending)
        }
        CurrentPluginCapabilityAuthorizationStatus::Granted => None,
        CurrentPluginCapabilityAuthorizationStatus::Denied => {
            Some(PluginGuardDenyCode::AuthorizationDenied)
        }
        CurrentPluginCapabilityAuthorizationStatus::Revoked => {
            Some(PluginGuardDenyCode::AuthorizationRevoked)
        }
        CurrentPluginCapabilityAuthorizationStatus::Expired => {
            Some(PluginGuardDenyCode::AuthorizationExpired)
        }
    };
    if let Some(code) = code {
        return Err(deny(call, code, "formal authorization is not effective"));
    }
    if !authorization.effective {
        return Err(deny(
            call,
            PluginGuardDenyCode::StoredDataInvalid,
            "granted view is not effective",
        ));
    }

    let rate_result = match call.rate_limit {
        GuardRateLimit::None => Ok(()),
        GuardRateLimit::Write => limiter.check_write(&call.plugin_id),
        GuardRateLimit::Ai => limiter.check_ai(&call.plugin_id),
    };
    rate_result.map_err(|error| {
        deny(
            call,
            PluginGuardDenyCode::RateLimitExceeded,
            &error.to_string(),
        )
    })?;

    Ok(AuthorizedPluginContext {
        manifest: current.manifest,
        version: current.version,
        install_path: current.install_path,
        correlation_id: String::new(),
    })
}

fn mapped_error(call: &TrustedPluginCall, error: AppError) -> PluginGuardDeny {
    let code = match error {
        AppError::PluginAuthorizationScopeMismatch
        | AppError::PluginAuthorizationScopeInvalid { .. } => PluginGuardDenyCode::ScopeMismatch,
        AppError::PluginAuthorizationSemanticVersionMismatch => {
            PluginGuardDenyCode::SemanticVersionMismatch
        }
        AppError::PluginAuthorizationManifestNotDeclared
        | AppError::PluginCapabilityNotDeclared { .. } => PluginGuardDenyCode::ManifestNotDeclared,
        AppError::PluginAuthorizationSnapshotNotDeclared
        | AppError::PluginPermissionSnapshotMissing { .. } => {
            PluginGuardDenyCode::SnapshotNotDeclared
        }
        AppError::PluginPermissionSnapshotInvalid { .. }
        | AppError::PluginPermissionSnapshotMismatch { .. }
        | AppError::PluginAuthorizationStoredRecordInvalid { .. }
        | AppError::Json(_) => PluginGuardDenyCode::StoredDataInvalid,
        AppError::NotFound(_) => PluginGuardDenyCode::PluginNotFound,
        _ => PluginGuardDenyCode::StateReadFailed,
    };
    deny(call, code, &error.to_string())
}

fn deny(call: &TrustedPluginCall, code: PluginGuardDenyCode, diagnostic: &str) -> PluginGuardDeny {
    PluginGuardDeny {
        code,
        safe_message: "插件调用未获授权",
        internal_diagnostic: diagnostic.chars().take(300).collect(),
        correlation_id: call.correlation_id.clone(),
        audited: false,
    }
}

fn enum_string<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn write_decision_audit(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    call: &TrustedPluginCall,
    verified_version: Option<&str>,
    decision: &str,
    deny_code: Option<&str>,
) -> Result<(), AppError> {
    // 只记录主体/context 类型，不记录账号 ID、token、用户正文或模型输入输出。
    let target = serde_json::json!({
        "capability": call.capability_id,
        "verifiedVersion": verified_version,
        "scene": call.scene.as_str(),
        "subjectKind": enum_string(&subject.kind),
        "hostContextKind": enum_string(&context.kind),
        "scope": "global:v1:*",
        "decision": decision,
        "denyCode": deny_code,
        "correlationId": call.correlation_id,
    });
    db.write_audit_log(
        &call.plugin_id,
        "capability_guard_decision",
        Some(&serde_json::to_string(&target)?),
    )
}

fn audit_untrusted_context_failure(
    db: &Database,
    call: &TrustedPluginCall,
    error: AppError,
) -> PluginGuardDeny {
    let mut denied = deny(
        call,
        PluginGuardDenyCode::CallContextUntrusted,
        &error.to_string(),
    );
    let target = serde_json::json!({
        "capability": call.capability_id,
        "scene": call.scene.as_str(),
        "subjectKind": "unresolved",
        "hostContextKind": "unresolved",
        "decision": "deny",
        "denyCode": denied.code.as_str(),
        "correlationId": call.correlation_id,
    });
    let audit = serde_json::to_string(&target)
        .map_err(AppError::from)
        .and_then(|target| {
            db.write_audit_log(&call.plugin_id, "capability_guard_decision", Some(&target))
        });
    match audit {
        Ok(()) => {
            denied.audited = true;
            denied
        }
        Err(audit_error) => PluginGuardDeny {
            code: PluginGuardDenyCode::AuditWriteFailed,
            safe_message: "授权审计暂不可用",
            internal_diagnostic: audit_error.to_string(),
            correlation_id: call.correlation_id.clone(),
            audited: false,
        },
    }
}

// 旧 helper 仅保留给尚待 A4/A7 迁移的调用点；它不得被视为 A3 正式授权入口。
pub(crate) fn resolve_current_plugin_context(
    db: &Database,
    plugin_id: &str,
) -> Result<AuthorizedPluginContext, AppError> {
    let snapshot = db
        .current_plugin_authorization_snapshot(plugin_id, &[])?
        .ok_or_else(|| AppError::NotFound(format!("未找到插件 {}", plugin_id)))?;
    if snapshot.status != "installed" || !snapshot.enabled {
        return Err(AppError::InvalidInput("插件当前状态不允许调用".into()));
    }
    let current = snapshot
        .current_version
        .ok_or_else(|| AppError::NotFound(format!("未找到插件 {} 的当前版本", plugin_id)))?;
    Ok(AuthorizedPluginContext {
        manifest: current.manifest,
        version: current.version,
        install_path: current.install_path,
        correlation_id: String::new(),
    })
}

/// A4 迁移前兼容签名；保留既有 legacy 语义，正式宿主迁移必须改用
/// `authorize_plugin_call`，不得新增此 helper 的调用点。
pub(crate) fn require_current_plugin_capabilities(
    db: &Database,
    plugin_id: &str,
    capabilities: &[&str],
) -> Result<AuthorizedPluginContext, AppError> {
    let snapshot = db
        .current_plugin_authorization_snapshot(plugin_id, capabilities)?
        .ok_or_else(|| AppError::NotFound(format!("未找到插件 {}", plugin_id)))?;
    if snapshot.status != "installed" || !snapshot.enabled {
        return Err(AppError::InvalidInput("插件当前状态不允许调用".into()));
    }
    let current = snapshot
        .current_version
        .ok_or_else(|| AppError::NotFound(format!("未找到插件 {} 的当前版本", plugin_id)))?;
    for capability in capabilities {
        if !current
            .manifest
            .permissions
            .iter()
            .any(|value| value == capability)
        {
            return Err(AppError::PluginCapabilityNotDeclared {
                plugin_id: plugin_id.to_string(),
                capability: (*capability).to_string(),
            });
        }
        let granted = snapshot
            .grant_states
            .iter()
            .find(|(value, _)| value == capability)
            .and_then(|(_, granted)| *granted)
            .unwrap_or(false);
        if !granted {
            return Err(AppError::PluginPermissionDenied {
                plugin_id: Some(plugin_id.to_string()),
                required_permission: Some((*capability).to_string()),
            });
        }
    }
    Ok(AuthorizedPluginContext {
        manifest: current.manifest,
        version: current.version,
        install_path: current.install_path,
        correlation_id: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::models::{
        PendingPluginCapabilityAuthorization, PluginAuthorizationIdentityBinding,
        PluginAuthorizationIdentityBindingStatus, PluginAuthorizationLifetime,
        PluginAuthorizationSource, PluginAuthorizationSubjectKind, PluginClassification,
        PluginRuntimeKind, PluginSource,
    };
    use crate::services::plugin_authorization_context::{
        canonicalize_authorization_scope, resolve_capability_semantic_version,
    };
    use uuid::Uuid;

    struct Fixture {
        db: Database,
        directory: PathBuf,
        subject: PluginAuthorizationSubject,
        context: PluginAuthorizationContext,
        limiter: PluginRateLimiter,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.directory).ok();
        }
    }

    fn setup(capability: &str, grant: bool) -> Fixture {
        let db = Database::init(":memory:").expect("database");
        let directory = std::env::temp_dir().join(format!("pomegranate-a3-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("plugin directory");
        fs::write(directory.join("ui.json"), "{}").expect("plugin asset");
        let manifest = PluginManifestV3 {
            schema_version: 3,
            id: "com.firstwork.a3-guard".into(),
            name: "A3 Guard".into(),
            version: "1.0.0".into(),
            author_id: "tests".into(),
            description: None,
            min_app_version: None,
            classification: PluginClassification::Feature,
            runtime_kind: PluginRuntimeKind::XingchenWorkflow,
            source: PluginSource::Local,
            activation_events: Vec::new(),
            supported_scenes: vec![PluginScene::Global],
            default_activation: Default::default(),
            permissions: vec![capability.into()],
            configuration_schema: None,
            dependencies: Vec::new(),
            conflicts_with: Vec::new(),
            contributes: Default::default(),
            integrity: Default::default(),
            signature: Default::default(),
        };
        let hash = PluginService::calculate_integrity_for_path(&directory).expect("hash");
        db.record_plugin_version(
            &manifest,
            &directory.to_string_lossy(),
            &hash,
            &[capability.to_string()],
        )
        .expect("record version");
        db.set_plugin_enabled(&manifest.id, true).expect("enable");
        let subject = PluginAuthorizationSubject {
            kind: PluginAuthorizationSubjectKind::PlatformUser,
            id: "test-user".into(),
        };
        let context = resolve_host_installation_context(&db).expect("host context");
        if grant {
            grant_formal(&db, &subject, &context, &manifest.id, capability, None);
        }
        Fixture {
            db,
            directory,
            subject,
            context,
            limiter: PluginRateLimiter::new(),
        }
    }

    fn grant_formal(
        db: &Database,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability: &str,
        expires_at: Option<String>,
    ) {
        let (scope, pending) =
            create_pending(db, subject, context, plugin_id, capability, expires_at);
        db.grant_pending_formal_plugin_capability_authorization(
            subject,
            context,
            plugin_id,
            capability,
            &scope,
            "1.0.0",
            pending.revision,
        )
        .expect("formal grant");
    }

    fn create_pending(
        db: &Database,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability: &str,
        expires_at: Option<String>,
    ) -> (
        crate::models::PluginAuthorizationScope,
        crate::models::PluginCapabilityAuthorization,
    ) {
        let scope = canonicalize_authorization_scope("global", "v1:*").expect("scope");
        let unavailable = PluginAuthorizationIdentityBinding {
            identity: None,
            status: PluginAuthorizationIdentityBindingStatus::Unavailable,
        };
        let pending = db
            .create_or_update_pending_formal_plugin_capability_authorization(
                &PendingPluginCapabilityAuthorization {
                    subject: subject.clone(),
                    context: context.clone(),
                    plugin_id: plugin_id.into(),
                    capability_id: capability.into(),
                    capability_semantic_version: Some(
                        resolve_capability_semantic_version(capability).expect("semantic version"),
                    ),
                    scope: scope.clone(),
                    source: PluginAuthorizationSource::OnDemand,
                    lifetime: PluginAuthorizationLifetime::Persistent,
                    last_confirmed_version: Some("1.0.0".into()),
                    publisher: unavailable.clone(),
                    signature: unavailable,
                    expires_at,
                },
                None,
            )
            .expect("pending authorization");
        (scope, pending)
    }

    fn set_plugin_status(fixture: &Fixture, status: &str) {
        fixture
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugins SET status = ?1 WHERE id = 'com.firstwork.a3-guard'",
                [status],
            )
            .expect("set plugin status");
    }

    fn update_current_manifest(fixture: &Fixture, path: &str, value: &str) {
        fixture
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_versions
                 SET manifest_json = json_set(manifest_json, ?1, json(?2))
                 WHERE plugin_id = 'com.firstwork.a3-guard' AND is_current = 1",
                rusqlite::params![path, value],
            )
            .expect("update current manifest");
    }

    fn bind_marketplace_security_state(fixture: &Fixture) {
        let conn = fixture.db.conn_lock().expect("connection");
        conn.execute(
            "INSERT INTO products (id, developer_id, name, product_type, status)
             VALUES ('a3-product', 'a3-developer', 'A3 Product', 'plugin', 'published')",
            [],
        )
        .expect("insert product");
        conn.execute(
            "INSERT INTO product_versions
                (product_id, version, manifest_json, runtime_kind, source,
                 content_hash, signature_status, status)
             VALUES
                ('a3-product', '1.0.0', '{}', 'xingchen-workflow', 'marketplace',
                 'a3-hash', 'verified', 'active')",
            [],
        )
        .expect("insert product version");
        conn.execute(
            "INSERT INTO plugin_installations
                (plugin_id, product_id, product_version_id, installed_version, source,
                 enabled, install_path, content_hash, status)
             SELECT 'com.firstwork.a3-guard', 'a3-product', id, '1.0.0', 'marketplace',
                    1, '', 'a3-hash', 'installed'
             FROM product_versions
             WHERE product_id = 'a3-product' AND version = '1.0.0'",
            [],
        )
        .expect("insert installation binding");
    }

    fn actor(fixture: &Fixture) -> ResolvedAuthorizationActor {
        ResolvedAuthorizationActor {
            subject: fixture.subject.clone(),
            context: fixture.context.clone(),
        }
    }

    fn authorize(
        fixture: &Fixture,
        call: TrustedPluginCall,
    ) -> Result<AuthorizedPluginContext, PluginGuardDeny> {
        authorize_resolved_plugin_call(&fixture.db, &fixture.limiter, &actor(fixture), call)
    }

    fn call(capability: &str) -> TrustedPluginCall {
        TrustedPluginCall::internal(
            "com.firstwork.a3-guard",
            capability,
            Some("1.0.0".into()),
            PluginScene::Global,
            "a3-test-correlation",
        )
    }

    #[test]
    fn allows_complete_formal_context_and_writes_redacted_audit() {
        let fixture = setup("ai.invoke", true);
        let allowed = authorize(&fixture, call("ai.invoke")).expect("allow");
        assert_eq!(allowed.version, "1.0.0");
        let logs = fixture
            .db
            .get_plugin_audit_log("com.firstwork.a3-guard", 10)
            .expect("audit");
        let target = logs[0].3.as_deref().expect("audit target");
        assert!(target.contains("\"decision\":\"allow\""));
        assert!(target.contains("a3-test-correlation"));
        assert!(!target.contains("test-user"));
        assert!(!target.to_ascii_lowercase().contains("token"));
    }

    #[test]
    fn formal_missing_never_falls_back_to_legacy_and_is_audited() {
        let fixture = setup("ai.invoke", false);
        fixture
            .db
            .grant_plugin_permissions("com.firstwork.a3-guard", &["ai.invoke".into()])
            .expect("legacy grant");
        let denied = authorize(&fixture, call("ai.invoke"))
            .expect_err("formal authorization remains missing");
        assert_eq!(denied.code, PluginGuardDenyCode::AuthorizationMissing);
        assert!(denied.audited);
        let logs = fixture
            .db
            .get_plugin_audit_log("com.firstwork.a3-guard", 10)
            .expect("audit");
        let target = logs[0].3.as_deref().expect("audit target");
        assert!(target.contains("\"denyCode\":\"authorization_missing\""));
        for sensitive in [
            "test-user",
            "credential",
            "user body",
            "model body",
            "token",
        ] {
            assert!(!target.to_ascii_lowercase().contains(sensitive));
        }
    }

    #[test]
    fn rejects_reserved_unknown_and_unverifiable_token_contexts() {
        let reserved = setup("files.readSelected", false);
        let denied = authorize(&reserved, call("files.readSelected")).expect_err("reserved");
        assert_eq!(denied.code, PluginGuardDenyCode::CapabilityReserved);

        let unknown = setup("ai.invoke", false);
        let denied = authorize(&unknown, call("unknown.capability")).expect_err("unknown");
        assert_eq!(denied.code, PluginGuardDenyCode::CapabilityUnknown);

        let token = setup("ai.invoke", true);
        let denied = authorize(&token, call("ai.invoke").require_token())
            .expect_err("legacy token lacks trusted bindings");
        assert_eq!(denied.code, PluginGuardDenyCode::TokenContextUnsupported);
    }

    #[test]
    fn rejects_disabled_version_scene_integrity_and_snapshot_mismatch() {
        let disabled = setup("ai.invoke", true);
        disabled
            .db
            .set_plugin_enabled("com.firstwork.a3-guard", false)
            .expect("disable");
        assert_eq!(
            authorize(&disabled, call("ai.invoke"))
                .expect_err("disabled")
                .code,
            PluginGuardDenyCode::PluginDisabled
        );

        let version = setup("ai.invoke", true);
        let mut wrong_version = call("ai.invoke");
        wrong_version.expected_version = Some("2.0.0".into());
        assert_eq!(
            authorize(&version, wrong_version)
                .expect_err("version")
                .code,
            PluginGuardDenyCode::VersionNotCurrent
        );

        let integrity = setup("ai.invoke", true);
        fs::write(integrity.directory.join("ui.json"), "changed").expect("tamper");
        assert_eq!(
            authorize(&integrity, call("ai.invoke"))
                .expect_err("integrity")
                .code,
            PluginGuardDenyCode::IntegrityFailed
        );
    }

    #[test]
    fn lifecycle_states_have_stable_fail_closed_codes() {
        let missing = setup("ai.invoke", true);
        missing
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "DELETE FROM plugins WHERE id = 'com.firstwork.a3-guard'",
                [],
            )
            .expect("delete plugin");
        assert_eq!(
            authorize(&missing, call("ai.invoke"))
                .expect_err("missing")
                .code,
            PluginGuardDenyCode::PluginNotFound
        );

        for (status, expected) in [
            ("blocked", PluginGuardDenyCode::PluginBlocked),
            ("revoked", PluginGuardDenyCode::PluginRevoked),
            ("uninstalled", PluginGuardDenyCode::PluginUninstalled),
            ("uninstalling", PluginGuardDenyCode::PluginUninstalling),
            ("deactivating", PluginGuardDenyCode::PluginDeactivating),
            ("error", PluginGuardDenyCode::InstallStateDenied),
        ] {
            let fixture = setup("ai.invoke", true);
            set_plugin_status(&fixture, status);
            let denied = authorize(&fixture, call("ai.invoke")).expect_err(status);
            assert_eq!(denied.code, expected, "status={status}");
            assert!(denied.audited);
        }
    }

    #[test]
    fn capability_registry_status_runtime_and_source_policies_are_enforced() {
        let fixture = setup("ai.invoke", true);
        for (capability, expected) in [
            (
                "credentials.configure",
                PluginGuardDenyCode::CapabilityBlocked,
            ),
            (
                "files.readSelected",
                PluginGuardDenyCode::CapabilityReserved,
            ),
            ("notes:read", PluginGuardDenyCode::CapabilityLegacy),
            ("unknown.capability", PluginGuardDenyCode::CapabilityUnknown),
        ] {
            assert_eq!(
                authorize(&fixture, call(capability))
                    .expect_err(capability)
                    .code,
                expected
            );
        }

        let runtime = setup("ai.invoke", true);
        update_current_manifest(&runtime, "$.runtimeKind", r#""prompt-pack""#);
        assert_eq!(
            authorize(&runtime, call("ai.invoke"))
                .expect_err("runtime mismatch")
                .code,
            PluginGuardDenyCode::RuntimeCapabilityDenied
        );

        let source = setup("ai.context.read", false);
        update_current_manifest(&source, "$.runtimeKind", r#""prompt-pack""#);
        assert_eq!(
            authorize(&source, call("ai.context.read"))
                .expect_err("source mismatch")
                .code,
            PluginGuardDenyCode::SourceCapabilityDenied
        );
    }

    #[test]
    fn marketplace_trust_version_and_installation_states_are_authoritative() {
        let missing = setup("ai.invoke", true);
        update_current_manifest(&missing, "$.source", r#""marketplace""#);
        assert_eq!(
            authorize(&missing, call("ai.invoke"))
                .expect_err("marketplace binding is required")
                .code,
            PluginGuardDenyCode::TrustStateUnavailable
        );

        let allowed = setup("ai.invoke", true);
        update_current_manifest(&allowed, "$.source", r#""marketplace""#);
        bind_marketplace_security_state(&allowed);
        assert!(authorize(&allowed, call("ai.invoke")).is_ok());

        for (table, assignment, expected) in [
            (
                "products",
                "status = 'revoked'",
                PluginGuardDenyCode::PluginRevoked,
            ),
            (
                "product_versions",
                "status = 'revoked'",
                PluginGuardDenyCode::VersionRevoked,
            ),
            (
                "product_versions",
                "signature_status = 'revoked'",
                PluginGuardDenyCode::VersionRevoked,
            ),
            (
                "plugin_installations",
                "status = 'uninstalling'",
                PluginGuardDenyCode::PluginUninstalling,
            ),
            (
                "plugin_installations",
                "installed_version = '2.0.0'",
                PluginGuardDenyCode::VersionNotCurrent,
            ),
        ] {
            let fixture = setup("ai.invoke", true);
            update_current_manifest(&fixture, "$.source", r#""marketplace""#);
            bind_marketplace_security_state(&fixture);
            fixture
                .db
                .conn_lock()
                .expect("connection")
                .execute(&format!("UPDATE {table} SET {assignment}"), [])
                .expect("set marketplace security state");
            assert_eq!(
                authorize(&fixture, call("ai.invoke"))
                    .expect_err(assignment)
                    .code,
                expected,
                "assignment={assignment}"
            );
        }
    }

    #[test]
    fn manifest_snapshot_and_formal_authorization_remain_independent_facts() {
        let manifest = setup("ai.invoke", true);
        update_current_manifest(&manifest, "$.permissions", "[]");
        assert_eq!(
            authorize(&manifest, call("ai.invoke"))
                .expect_err("manifest missing")
                .code,
            PluginGuardDenyCode::ManifestNotDeclared
        );

        let snapshot = setup("ai.invoke", true);
        snapshot
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_versions SET permissions_json = '[]'
                 WHERE plugin_id = 'com.firstwork.a3-guard' AND is_current = 1",
                [],
            )
            .expect("clear snapshot");
        assert_eq!(
            authorize(&snapshot, call("ai.invoke"))
                .expect_err("snapshot missing")
                .code,
            PluginGuardDenyCode::SnapshotNotDeclared
        );

        let pending = setup("ai.invoke", false);
        create_pending(
            &pending.db,
            &pending.subject,
            &pending.context,
            "com.firstwork.a3-guard",
            "ai.invoke",
            None,
        );
        assert_eq!(
            authorize(&pending, call("ai.invoke"))
                .expect_err("pending")
                .code,
            PluginGuardDenyCode::AuthorizationPending
        );

        let denied = setup("ai.invoke", false);
        let (scope, record) = create_pending(
            &denied.db,
            &denied.subject,
            &denied.context,
            "com.firstwork.a3-guard",
            "ai.invoke",
            None,
        );
        denied
            .db
            .deny_pending_formal_plugin_capability_authorization(
                &denied.subject,
                &denied.context,
                "com.firstwork.a3-guard",
                "ai.invoke",
                &scope,
                record.revision,
            )
            .expect("deny");
        assert_eq!(
            authorize(&denied, call("ai.invoke"))
                .expect_err("denied")
                .code,
            PluginGuardDenyCode::AuthorizationDenied
        );

        let revoked = setup("ai.invoke", true);
        let scope = canonicalize_authorization_scope("global", "v1:*").expect("scope");
        let record = revoked
            .db
            .get_formal_plugin_capability_authorization(
                &revoked.subject,
                &revoked.context,
                "com.firstwork.a3-guard",
                "ai.invoke",
                &scope,
            )
            .expect("read")
            .expect("record");
        revoked
            .db
            .revoke_granted_formal_plugin_capability_authorization(
                &revoked.subject,
                &revoked.context,
                "com.firstwork.a3-guard",
                "ai.invoke",
                &scope,
                record.revision,
            )
            .expect("revoke");
        assert_eq!(
            authorize(&revoked, call("ai.invoke"))
                .expect_err("revoked")
                .code,
            PluginGuardDenyCode::AuthorizationRevoked
        );

        let expired = setup("ai.invoke", false);
        grant_formal(
            &expired.db,
            &expired.subject,
            &expired.context,
            "com.firstwork.a3-guard",
            "ai.invoke",
            Some("2000-01-01T00:00:00Z".into()),
        );
        assert_eq!(
            authorize(&expired, call("ai.invoke"))
                .expect_err("expired")
                .code,
            PluginGuardDenyCode::AuthorizationExpired
        );
    }

    #[test]
    fn scope_semantic_version_and_scene_fail_closed() {
        let scope = setup("ai.invoke", true);
        scope
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_capability_authorizations SET scope_key = 'v1:other'
                 WHERE plugin_id = 'com.firstwork.a3-guard'",
                [],
            )
            .expect("damage scope");
        assert_eq!(
            authorize(&scope, call("ai.invoke"))
                .expect_err("scope")
                .code,
            PluginGuardDenyCode::ScopeMismatch
        );

        let semantic = setup("ai.invoke", true);
        semantic
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_capability_authorizations
                 SET capability_semantic_version = '9.9.9'
                 WHERE plugin_id = 'com.firstwork.a3-guard'",
                [],
            )
            .expect("damage semantic version");
        assert_eq!(
            authorize(&semantic, call("ai.invoke"))
                .expect_err("semantic version")
                .code,
            PluginGuardDenyCode::SemanticVersionMismatch
        );

        let scene = setup("ai.invoke", true);
        update_current_manifest(&scene, "$.supportedScenes", r#"["learning"]"#);
        let mut wrong_scene = call("ai.invoke");
        wrong_scene.scene = PluginScene::Research;
        assert_eq!(
            authorize(&scene, wrong_scene).expect_err("scene").code,
            PluginGuardDenyCode::SceneMismatch
        );

        let global = setup("ai.invoke", true);
        let mut learning_call = call("ai.invoke");
        learning_call.scene = PluginScene::Learning;
        assert!(authorize(&global, learning_call).is_ok());
    }

    #[test]
    fn integrity_database_rate_limit_and_audit_fail_closed() {
        let missing_integrity = setup("ai.invoke", true);
        missing_integrity
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugins SET content_hash = '' WHERE id = 'com.firstwork.a3-guard'",
                [],
            )
            .expect("remove hash");
        assert_eq!(
            authorize(&missing_integrity, call("ai.invoke"))
                .expect_err("missing integrity")
                .code,
            PluginGuardDenyCode::IntegrityUnavailable
        );

        let corrupt = setup("ai.invoke", true);
        corrupt
            .db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_versions SET manifest_json = '{broken'
                 WHERE plugin_id = 'com.firstwork.a3-guard' AND is_current = 1",
                [],
            )
            .expect("corrupt manifest");
        assert_eq!(
            authorize(&corrupt, call("ai.invoke"))
                .expect_err("corrupt")
                .code,
            PluginGuardDenyCode::StoredDataInvalid
        );

        let database = setup("ai.invoke", true);
        database
            .db
            .conn_lock()
            .expect("connection")
            .execute("DROP TABLE plugins", [])
            .expect("drop source table");
        assert_eq!(
            authorize(&database, call("ai.invoke"))
                .expect_err("database error")
                .code,
            PluginGuardDenyCode::StateReadFailed
        );

        let limited = setup("ai.invoke", true);
        for index in 0..10 {
            let mut request = call("ai.invoke").with_rate_limit(GuardRateLimit::Ai);
            request.correlation_id = format!("rate-{index}");
            authorize(&limited, request).expect("within AI limit");
        }
        assert_eq!(
            authorize(
                &limited,
                call("ai.invoke").with_rate_limit(GuardRateLimit::Ai),
            )
            .expect_err("rate limited")
            .code,
            PluginGuardDenyCode::RateLimitExceeded
        );

        let audit = setup("ai.invoke", true);
        audit
            .db
            .conn_lock()
            .expect("connection")
            .execute("DROP TABLE plugin_audit_log", [])
            .expect("drop audit table");
        let denied = authorize(&audit, call("ai.invoke")).expect_err("audit unavailable");
        assert_eq!(denied.code, PluginGuardDenyCode::AuditWriteFailed);
        assert!(!denied.audited);
    }
}
