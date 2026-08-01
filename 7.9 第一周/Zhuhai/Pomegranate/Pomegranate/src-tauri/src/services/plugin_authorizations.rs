use chrono::{DateTime, Utc};

use crate::account::AccountState;
use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    CurrentPluginCapabilityAuthorization, CurrentPluginCapabilityAuthorizationStatus,
    PendingPluginCapabilityAuthorization, PluginAuthorizationContext,
    PluginAuthorizationIdentityBinding, PluginAuthorizationIdentityBindingStatus,
    PluginAuthorizationLifetime, PluginAuthorizationScope, PluginAuthorizationSource,
    PluginAuthorizationState, PluginAuthorizationSubject, PluginCapabilityAuthorization,
};

use super::plugin_authorization_context::{
    canonicalize_authorization_scope, resolve_capability_semantic_version,
    resolve_host_installation_context, resolve_verified_platform_subject, TrustedResourceScope,
};
use super::plugin_capabilities::canonical_capability_policy;

const GLOBAL_SCOPE_KIND: &str = "global";
const GLOBAL_SCOPE_KEY: &str = "v1:*";

#[derive(Debug)]
struct CurrentCapabilityTarget {
    plugin_version: String,
    capability_semantic_version: String,
    scope: PluginAuthorizationScope,
}

/// 列出当前可信主体/context 对全部当前 Manifest capability 的正式授权事实。
pub(crate) async fn list_current_formal_plugin_capability_authorizations(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
) -> Result<Vec<CurrentPluginCapabilityAuthorization>, AppError> {
    let subject = resolve_verified_platform_subject(account).await?;
    let context = resolve_host_installation_context(db)?;
    list_current_formal_plugin_capability_authorizations_for_actor(
        db, &subject, &context, plugin_id,
    )
}

/// 创建或取得当前 capability 的 pending 请求；不会覆盖既有决定。
pub(crate) async fn request_current_formal_plugin_capability_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    capability_id: &str,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    request_for_actor(db, &subject, &context, plugin_id, capability_id, expires_at)
}

/// 用户明确同意：missing 会先创建 pending，再按 revision 转为 granted。
pub(crate) async fn grant_current_formal_plugin_capability_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    capability_id: &str,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    grant_for_actor(db, &subject, &context, plugin_id, capability_id, expires_at)
}

/// 用户明确拒绝：missing 会先创建 pending，再按 revision 转为 denied。
pub(crate) async fn deny_current_formal_plugin_capability_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    capability_id: &str,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    deny_for_actor(db, &subject, &context, plugin_id, capability_id)
}

/// 用户明确撤权；只允许 granted -> revoked，重复撤权幂等返回原记录。
pub(crate) async fn revoke_current_formal_plugin_capability_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    capability_id: &str,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    revoke_for_actor(db, &subject, &context, plugin_id, capability_id)
}

/// 将确已到期的 pending/granted 明确落为 expired；读取本身不依赖该写回。
pub(crate) async fn expire_current_formal_plugin_capability_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    capability_id: &str,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    expire_for_actor(db, &subject, &context, plugin_id, capability_id)
}

async fn resolve_actor(
    db: &Database,
    account: &AccountState,
) -> Result<(PluginAuthorizationSubject, PluginAuthorizationContext), AppError> {
    let subject = resolve_verified_platform_subject(account).await?;
    let context = resolve_host_installation_context(db)?;
    Ok((subject, context))
}

fn current_target(
    db: &Database,
    plugin_id: &str,
    capability_id: &str,
) -> Result<CurrentCapabilityTarget, AppError> {
    current_target_with_scope(
        db,
        plugin_id,
        capability_id,
        canonicalize_authorization_scope(GLOBAL_SCOPE_KIND, GLOBAL_SCOPE_KEY)?,
    )
}

fn current_target_with_scope(
    db: &Database,
    plugin_id: &str,
    capability_id: &str,
    scope: PluginAuthorizationScope,
) -> Result<CurrentCapabilityTarget, AppError> {
    let capability_semantic_version = resolve_capability_semantic_version(capability_id)?;
    let (manifest, snapshot) = db.current_plugin_capability_contract(plugin_id)?;
    if !manifest
        .capabilities
        .iter()
        .any(|item| item == capability_id)
    {
        return Err(AppError::PluginAuthorizationManifestNotDeclared);
    }
    if !snapshot
        .capabilities
        .iter()
        .any(|item| item == capability_id)
    {
        return Err(AppError::PluginAuthorizationSnapshotNotDeclared);
    }
    if manifest.version != snapshot.version || manifest.capabilities != snapshot.capabilities {
        return Err(AppError::PluginPermissionSnapshotMismatch {
            plugin_id: plugin_id.to_string(),
            version: snapshot.version,
        });
    }
    Ok(CurrentCapabilityTarget {
        plugin_version: manifest.version,
        capability_semantic_version,
        scope,
    })
}

fn trusted_target(
    db: &Database,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
) -> Result<CurrentCapabilityTarget, AppError> {
    let policy = canonical_capability_policy(capability_id)
        .map_err(|_| AppError::PluginAuthorizationScopeInvalid {
            reason: "registry_scope_policy_invalid",
        })?
        .ok_or(AppError::PluginAuthorizationCapabilityInvalid {
            reason: "capability_not_admitted",
        })?;
    if policy.scope_type.as_str() != trusted_scope.scope_type() {
        return Err(AppError::PluginAuthorizationScopeMismatch);
    }
    current_target_with_scope(
        db,
        plugin_id,
        capability_id,
        trusted_scope.canonical_scope()?,
    )
}

pub(super) fn current_formal_plugin_capability_authorization_for_actor_and_scope(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
) -> Result<CurrentPluginCapabilityAuthorization, AppError> {
    let target = trusted_target(db, plugin_id, capability_id, trusted_scope)?;
    current_view_for_target(db, subject, context, plugin_id, capability_id, &target)
}

/// Read one exact scope without treating unrelated scopes for the same capability as aliases.
/// The Guard keeps its stricter mismatch diagnostic; the authorization-management API needs a
/// stable `Missing` view so several exact resources can coexist independently.
pub(super) fn current_exact_plugin_capability_authorization_for_actor_and_scope(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
) -> Result<CurrentPluginCapabilityAuthorization, AppError> {
    let target = trusted_target(db, plugin_id, capability_id, trusted_scope)?;
    current_view_for_exact_target(db, subject, context, plugin_id, capability_id, &target)
}

pub(super) fn list_formal_plugin_capability_authorizations_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
) -> Result<Vec<PluginCapabilityAuthorization>, AppError> {
    let records = db.list_formal_plugin_capability_authorizations(subject, context, plugin_id)?;
    for record in &records {
        validate_stored_scope(&record.scope)?;
    }
    Ok(records)
}

pub(super) fn request_for_actor_and_scope(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = trusted_target(db, plugin_id, capability_id, trusted_scope)?;
    request_for_target(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        target,
        expires_at,
    )
}

pub(super) fn grant_for_actor_and_scope(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = trusted_target(db, plugin_id, capability_id, trusted_scope)?;
    grant_for_target(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        target,
        expires_at,
    )
}

pub(super) fn deny_for_actor_and_scope(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = trusted_target(db, plugin_id, capability_id, trusted_scope)?;
    deny_for_target(db, subject, context, plugin_id, capability_id, target)
}

pub(super) fn revoke_for_actor_and_scope(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = trusted_target(db, plugin_id, capability_id, trusted_scope)?;
    revoke_for_target(db, subject, context, plugin_id, capability_id, target)
}

pub(super) fn expire_for_actor_and_scope(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    trusted_scope: &TrustedResourceScope,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = trusted_target(db, plugin_id, capability_id, trusted_scope)?;
    expire_for_target(db, subject, context, plugin_id, capability_id, target)
}

pub(super) fn list_current_formal_plugin_capability_authorizations_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
) -> Result<Vec<CurrentPluginCapabilityAuthorization>, AppError> {
    let (manifest, snapshot) = db.current_plugin_capability_contract(plugin_id)?;
    if manifest.version != snapshot.version || manifest.capabilities != snapshot.capabilities {
        return Err(AppError::PluginPermissionSnapshotMismatch {
            plugin_id: plugin_id.to_string(),
            version: snapshot.version,
        });
    }
    let scope = canonicalize_authorization_scope(GLOBAL_SCOPE_KIND, GLOBAL_SCOPE_KEY)?;
    let records = db.list_formal_plugin_capability_authorizations(subject, context, plugin_id)?;
    manifest
        .capabilities
        .iter()
        .map(|capability_id| {
            let semantic_version = resolve_capability_semantic_version(capability_id)?;
            view_for_capability(
                plugin_id,
                &manifest.version,
                capability_id,
                &semantic_version,
                &scope,
                &records,
            )
        })
        .collect()
}

fn view_for_capability(
    plugin_id: &str,
    plugin_version: &str,
    capability_id: &str,
    semantic_version: &str,
    scope: &PluginAuthorizationScope,
    records: &[PluginCapabilityAuthorization],
) -> Result<CurrentPluginCapabilityAuthorization, AppError> {
    let matching_capability = records
        .iter()
        .filter(|record| record.capability_id == capability_id)
        .collect::<Vec<_>>();
    let record = matching_capability
        .iter()
        .find(|record| record.scope == *scope)
        .copied();
    if record.is_none() && !matching_capability.is_empty() {
        return Err(AppError::PluginAuthorizationScopeMismatch);
    }
    let Some(record) = record else {
        return Ok(CurrentPluginCapabilityAuthorization {
            plugin_id: plugin_id.to_string(),
            plugin_version: plugin_version.to_string(),
            capability_id: capability_id.to_string(),
            capability_semantic_version: semantic_version.to_string(),
            scope: scope.clone(),
            status: CurrentPluginCapabilityAuthorizationStatus::Missing,
            effective: false,
            revision: None,
            expires_at: None,
        });
    };
    validate_record_semantic_version(record, semantic_version)?;
    let expired_now = record.state == PluginAuthorizationState::Granted
        && record
            .expires_at
            .as_deref()
            .map(parse_utc)
            .transpose()?
            .is_some_and(|expires_at| expires_at <= Utc::now());
    let status = if expired_now {
        CurrentPluginCapabilityAuthorizationStatus::Expired
    } else {
        status_from_state(record.state)
    };
    Ok(CurrentPluginCapabilityAuthorization {
        plugin_id: plugin_id.to_string(),
        plugin_version: plugin_version.to_string(),
        capability_id: capability_id.to_string(),
        capability_semantic_version: semantic_version.to_string(),
        scope: scope.clone(),
        status,
        effective: status == CurrentPluginCapabilityAuthorizationStatus::Granted,
        revision: Some(record.revision),
        expires_at: record.expires_at.clone(),
    })
}

fn current_view_for_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: &CurrentCapabilityTarget,
) -> Result<CurrentPluginCapabilityAuthorization, AppError> {
    let record = exact_record(db, subject, context, plugin_id, capability_id, target, true)?;
    let Some(record) = record else {
        return Ok(CurrentPluginCapabilityAuthorization {
            plugin_id: plugin_id.to_string(),
            plugin_version: target.plugin_version.clone(),
            capability_id: capability_id.to_string(),
            capability_semantic_version: target.capability_semantic_version.clone(),
            scope: target.scope.clone(),
            status: CurrentPluginCapabilityAuthorizationStatus::Missing,
            effective: false,
            revision: None,
            expires_at: None,
        });
    };
    let expired_now = record.state == PluginAuthorizationState::Granted
        && record
            .expires_at
            .as_deref()
            .map(parse_utc)
            .transpose()?
            .is_some_and(|expires_at| expires_at <= Utc::now());
    let status = if expired_now {
        CurrentPluginCapabilityAuthorizationStatus::Expired
    } else {
        status_from_state(record.state)
    };
    Ok(CurrentPluginCapabilityAuthorization {
        plugin_id: plugin_id.to_string(),
        plugin_version: target.plugin_version.clone(),
        capability_id: capability_id.to_string(),
        capability_semantic_version: target.capability_semantic_version.clone(),
        scope: target.scope.clone(),
        status,
        effective: status == CurrentPluginCapabilityAuthorizationStatus::Granted,
        revision: Some(record.revision),
        expires_at: record.expires_at,
    })
}

fn current_view_for_exact_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: &CurrentCapabilityTarget,
) -> Result<CurrentPluginCapabilityAuthorization, AppError> {
    let record = exact_record(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        target,
        false,
    )?;
    let Some(record) = record else {
        return Ok(CurrentPluginCapabilityAuthorization {
            plugin_id: plugin_id.to_string(),
            plugin_version: target.plugin_version.clone(),
            capability_id: capability_id.to_string(),
            capability_semantic_version: target.capability_semantic_version.clone(),
            scope: target.scope.clone(),
            status: CurrentPluginCapabilityAuthorizationStatus::Missing,
            effective: false,
            revision: None,
            expires_at: None,
        });
    };
    let expired_now = record.state == PluginAuthorizationState::Granted
        && record
            .expires_at
            .as_deref()
            .map(parse_utc)
            .transpose()?
            .is_some_and(|expires_at| expires_at <= Utc::now());
    let status = if expired_now {
        CurrentPluginCapabilityAuthorizationStatus::Expired
    } else {
        status_from_state(record.state)
    };
    Ok(CurrentPluginCapabilityAuthorization {
        plugin_id: plugin_id.to_string(),
        plugin_version: target.plugin_version.clone(),
        capability_id: capability_id.to_string(),
        capability_semantic_version: target.capability_semantic_version.clone(),
        scope: target.scope.clone(),
        status,
        effective: status == CurrentPluginCapabilityAuthorizationStatus::Granted,
        revision: Some(record.revision),
        expires_at: record.expires_at,
    })
}

fn request_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = current_target(db, plugin_id, capability_id)?;
    request_for_target(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        target,
        expires_at,
    )
}

fn request_for_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: CurrentCapabilityTarget,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    if let Some(record) = exact_record(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        &target,
        false,
    )? {
        if record.state == PluginAuthorizationState::Pending {
            return Ok(record);
        }
        return Err(transition_error(
            record.state,
            PluginAuthorizationState::Pending,
        ));
    }
    let input = pending_input(
        subject,
        context,
        plugin_id,
        capability_id,
        &target,
        expires_at,
    );
    db.create_or_update_pending_formal_plugin_capability_authorization(&input, None)
}

fn grant_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = current_target(db, plugin_id, capability_id)?;
    grant_for_target(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        target,
        expires_at,
    )
}

fn grant_for_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: CurrentCapabilityTarget,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let pending = match exact_record(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        &target,
        false,
    )? {
        Some(record) if record.state == PluginAuthorizationState::Granted => return Ok(record),
        Some(record) if record.state == PluginAuthorizationState::Pending => record,
        Some(record) => {
            return Err(transition_error(
                record.state,
                PluginAuthorizationState::Granted,
            ))
        }
        None => {
            let input = pending_input(
                subject,
                context,
                plugin_id,
                capability_id,
                &target,
                expires_at,
            );
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None)?
        }
    };
    db.grant_pending_formal_plugin_capability_authorization(
        subject,
        context,
        plugin_id,
        capability_id,
        &target.scope,
        &target.plugin_version,
        pending.revision,
    )
}

fn deny_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = current_target(db, plugin_id, capability_id)?;
    deny_for_target(db, subject, context, plugin_id, capability_id, target)
}

fn deny_for_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: CurrentCapabilityTarget,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let pending = match exact_record(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        &target,
        false,
    )? {
        Some(record) if record.state == PluginAuthorizationState::Denied => return Ok(record),
        Some(record) if record.state == PluginAuthorizationState::Pending => record,
        Some(record) => {
            return Err(transition_error(
                record.state,
                PluginAuthorizationState::Denied,
            ))
        }
        None => {
            let input = pending_input(subject, context, plugin_id, capability_id, &target, None);
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None)?
        }
    };
    db.deny_pending_formal_plugin_capability_authorization(
        subject,
        context,
        plugin_id,
        capability_id,
        &target.scope,
        pending.revision,
    )
}

fn revoke_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = current_target(db, plugin_id, capability_id)?;
    revoke_for_target(db, subject, context, plugin_id, capability_id, target)
}

fn revoke_for_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: CurrentCapabilityTarget,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let record = exact_record(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        &target,
        true,
    )?
    .ok_or(AppError::PluginAuthorizationNotFound)?;
    if record.state == PluginAuthorizationState::Revoked {
        return Ok(record);
    }
    db.revoke_granted_formal_plugin_capability_authorization(
        subject,
        context,
        plugin_id,
        capability_id,
        &target.scope,
        record.revision,
    )
}

fn expire_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = current_target(db, plugin_id, capability_id)?;
    expire_for_target(db, subject, context, plugin_id, capability_id, target)
}

fn expire_for_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: CurrentCapabilityTarget,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let record = exact_record(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        &target,
        true,
    )?
    .ok_or(AppError::PluginAuthorizationNotFound)?;
    if record.state == PluginAuthorizationState::Expired {
        return Ok(record);
    }
    db.expire_due_formal_plugin_capability_authorization(
        subject,
        context,
        plugin_id,
        capability_id,
        &target.scope,
        record.revision,
    )
}

fn exact_record(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: &CurrentCapabilityTarget,
    mismatch_if_other_scope: bool,
) -> Result<Option<PluginCapabilityAuthorization>, AppError> {
    let record = db.get_formal_plugin_capability_authorization(
        subject,
        context,
        plugin_id,
        capability_id,
        &target.scope,
    )?;
    if let Some(record) = record.as_ref() {
        validate_stored_scope(&record.scope)?;
        validate_record_semantic_version(record, &target.capability_semantic_version)?;
        return Ok(Some(record.clone()));
    }
    if mismatch_if_other_scope {
        let records =
            db.list_formal_plugin_capability_authorizations(subject, context, plugin_id)?;
        let mut found_other = false;
        for candidate in records
            .iter()
            .filter(|candidate| candidate.capability_id == capability_id)
        {
            validate_stored_scope(&candidate.scope)?;
            found_other = true;
        }
        if found_other {
            return Err(AppError::PluginAuthorizationScopeMismatch);
        }
    }
    Ok(None)
}

fn validate_stored_scope(scope: &PluginAuthorizationScope) -> Result<(), AppError> {
    canonicalize_authorization_scope(&scope.kind, &scope.key)
        .map(|_| ())
        .map_err(|_| AppError::PluginAuthorizationStoredRecordInvalid { reason: "scope" })
}

fn pending_input(
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: &CurrentCapabilityTarget,
    expires_at: Option<String>,
) -> PendingPluginCapabilityAuthorization {
    let unavailable = PluginAuthorizationIdentityBinding {
        identity: None,
        status: PluginAuthorizationIdentityBindingStatus::Unavailable,
    };
    PendingPluginCapabilityAuthorization {
        subject: subject.clone(),
        context: context.clone(),
        plugin_id: plugin_id.to_string(),
        capability_id: capability_id.to_string(),
        capability_semantic_version: Some(target.capability_semantic_version.clone()),
        scope: target.scope.clone(),
        source: PluginAuthorizationSource::OnDemand,
        lifetime: PluginAuthorizationLifetime::Persistent,
        last_confirmed_version: Some(target.plugin_version.clone()),
        publisher: unavailable.clone(),
        signature: unavailable,
        expires_at,
    }
}

fn validate_record_semantic_version(
    record: &PluginCapabilityAuthorization,
    expected: &str,
) -> Result<(), AppError> {
    if record.capability_semantic_version.as_deref() != Some(expected) {
        return Err(AppError::PluginAuthorizationSemanticVersionMismatch);
    }
    Ok(())
}

fn status_from_state(
    state: PluginAuthorizationState,
) -> CurrentPluginCapabilityAuthorizationStatus {
    match state {
        PluginAuthorizationState::Pending => CurrentPluginCapabilityAuthorizationStatus::Pending,
        PluginAuthorizationState::Granted => CurrentPluginCapabilityAuthorizationStatus::Granted,
        PluginAuthorizationState::Denied => CurrentPluginCapabilityAuthorizationStatus::Denied,
        PluginAuthorizationState::Revoked => CurrentPluginCapabilityAuthorizationStatus::Revoked,
        PluginAuthorizationState::Expired => CurrentPluginCapabilityAuthorizationStatus::Expired,
    }
}

fn transition_error(from: PluginAuthorizationState, to: PluginAuthorizationState) -> AppError {
    AppError::PluginAuthorizationTransitionInvalid {
        from: state_name(from),
        to: state_name(to),
    }
}

fn state_name(state: PluginAuthorizationState) -> &'static str {
    match state {
        PluginAuthorizationState::Pending => "pending",
        PluginAuthorizationState::Granted => "granted",
        PluginAuthorizationState::Denied => "denied",
        PluginAuthorizationState::Revoked => "revoked",
        PluginAuthorizationState::Expired => "expired",
    }
}

fn parse_utc(value: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| AppError::PluginAuthorizationStoredRecordInvalid {
            reason: "expires_at_not_utc_rfc3339",
        })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, SecondsFormat};

    use super::*;
    use crate::models::{
        PluginAuthorizationContextKind, PluginAuthorizationSubjectKind, PluginManifestV3,
    };

    fn actor() -> (PluginAuthorizationSubject, PluginAuthorizationContext) {
        (
            PluginAuthorizationSubject {
                kind: PluginAuthorizationSubjectKind::PlatformUser,
                id: "platform-user-tests".into(),
            },
            PluginAuthorizationContext {
                kind: PluginAuthorizationContextKind::HostInstallation,
                id: "550e8400-e29b-41d4-a716-446655440000".into(),
            },
        )
    }

    fn manifest(permissions: &[&str]) -> PluginManifestV3 {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.formal-auth-service",
            "name": "Formal Authorization Service",
            "version": "1.0.0",
            "authorId": "tests",
            "classification": "feature",
            "runtimeKind": "declarative-ui",
            "source": "local",
            "supportedScenes": ["global"],
            "permissions": permissions,
            "contributes": {}
        }))
        .expect("manifest")
    }

    fn setup(
        permissions: &[&str],
    ) -> (
        Database,
        PluginAuthorizationSubject,
        PluginAuthorizationContext,
    ) {
        let db = Database::init(":memory:").expect("database");
        let manifest = manifest(permissions);
        db.record_plugin_version(
            &manifest,
            "C:/test/formal-auth-service",
            "formal-auth-service-hash",
            &[],
        )
        .expect("current version");
        let (subject, context) = actor();
        (db, subject, context)
    }

    #[test]
    fn grant_read_revoke_and_missing_are_distinct_and_persistent() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        let missing = list_current_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect("missing view");
        assert_eq!(
            missing[0].status,
            CurrentPluginCapabilityAuthorizationStatus::Missing
        );
        assert!(!missing[0].effective);

        let granted = grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            None,
        )
        .expect("grant");
        assert_eq!(granted.state, PluginAuthorizationState::Granted);
        assert_eq!(
            grant_for_actor(
                &db,
                &subject,
                &context,
                "com.firstwork.formal-auth-service",
                "ai.invoke",
                None,
            )
            .expect("idempotent grant")
            .id,
            granted.id
        );
        let view = list_current_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect("granted view");
        assert!(view[0].effective);

        let revoked = revoke_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
        )
        .expect("revoke");
        assert_eq!(revoked.state, PluginAuthorizationState::Revoked);
        let view = list_current_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect("revoked view");
        assert_eq!(
            view[0].status,
            CurrentPluginCapabilityAuthorizationStatus::Revoked
        );
        assert!(!view[0].effective);
        assert!(grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            None,
        )
        .is_err());
    }

    #[test]
    fn deny_and_expiry_have_stable_fail_closed_statuses() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        let denied = deny_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
        )
        .expect("deny");
        assert_eq!(denied.state, PluginAuthorizationState::Denied);
        let view = list_current_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect("denied view");
        assert_eq!(
            view[0].status,
            CurrentPluginCapabilityAuthorizationStatus::Denied
        );
        assert!(!view[0].effective);

        let (db, subject, context) = setup(&["ai.invoke"]);
        let expires_at =
            (Utc::now() - Duration::minutes(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
        grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            Some(expires_at),
        )
        .expect("grant expired record");
        let view = list_current_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect("expired read");
        assert_eq!(
            view[0].status,
            CurrentPluginCapabilityAuthorizationStatus::Expired
        );
        assert!(!view[0].effective);
        assert_eq!(
            expire_for_actor(
                &db,
                &subject,
                &context,
                "com.firstwork.formal-auth-service",
                "ai.invoke",
            )
            .expect("persist expiry")
            .state,
            PluginAuthorizationState::Expired
        );
    }

    #[test]
    fn manifest_snapshot_scope_and_semantic_version_all_fail_closed() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        assert!(matches!(
            grant_for_actor(
                &db,
                &subject,
                &context,
                "com.firstwork.formal-auth-service",
                "agents.invoke",
                None,
            ),
            Err(AppError::PluginAuthorizationManifestNotDeclared)
        ));

        db.conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_versions SET permissions_json = '[]'
                 WHERE plugin_id = 'com.firstwork.formal-auth-service' AND is_current = 1",
                [],
            )
            .expect("damage snapshot");
        assert!(matches!(
            grant_for_actor(
                &db,
                &subject,
                &context,
                "com.firstwork.formal-auth-service",
                "ai.invoke",
                None,
            ),
            Err(AppError::PluginAuthorizationSnapshotNotDeclared)
        ));

        let (db, subject, context) = setup(&["ai.invoke"]);
        let target =
            current_target(&db, "com.firstwork.formal-auth-service", "ai.invoke").expect("target");
        let mut input = pending_input(
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &target,
            None,
        );
        input.scope = PluginAuthorizationScope {
            kind: "future-resource".into(),
            key: "v1:secret".into(),
        };
        db.create_or_update_pending_formal_plugin_capability_authorization(&input, None)
            .expect("other scope");
        assert!(matches!(
            list_current_formal_plugin_capability_authorizations_for_actor(
                &db,
                &subject,
                &context,
                "com.firstwork.formal-auth-service"
            ),
            Err(AppError::PluginAuthorizationScopeMismatch)
        ));

        let (db, subject, context) = setup(&["ai.invoke"]);
        grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            None,
        )
        .expect("grant");
        db.conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_capability_authorizations
                 SET capability_semantic_version = 'sensitive-stale-version'",
                [],
            )
            .expect("stale semantic version");
        let error = list_current_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect_err("semantic mismatch");
        assert!(matches!(
            error,
            AppError::PluginAuthorizationSemanticVersionMismatch
        ));
        assert!(!error.to_string().contains("sensitive-stale-version"));
    }

    #[test]
    fn formal_service_never_reads_or_writes_legacy_permission_rows() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        let before = db
            .current_legacy_capability_authorization(
                "com.firstwork.formal-auth-service",
                "ai.invoke",
            )
            .expect("legacy before");
        grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            None,
        )
        .expect("formal grant");
        let after = db
            .current_legacy_capability_authorization(
                "com.firstwork.formal-auth-service",
                "ai.invoke",
            )
            .expect("legacy after");
        assert_eq!(before, after);
    }

    #[test]
    fn existing_grant_cannot_bypass_changed_manifest_or_snapshot() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            None,
        )
        .expect("grant");

        db.conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_versions
                 SET manifest_json = json_set(manifest_json, '$.permissions', json('[]'))
                 WHERE plugin_id = 'com.firstwork.formal-auth-service' AND is_current = 1",
                [],
            )
            .expect("remove manifest declaration");
        assert!(matches!(
            current_target(&db, "com.firstwork.formal-auth-service", "ai.invoke"),
            Err(AppError::PluginAuthorizationManifestNotDeclared)
        ));

        let manifest = manifest(&["ai.invoke"]);
        db.conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_versions
                 SET manifest_json = ?1, permissions_json = '[]'
                 WHERE plugin_id = 'com.firstwork.formal-auth-service' AND is_current = 1",
                [serde_json::to_string(&manifest).expect("manifest json")],
            )
            .expect("remove snapshot declaration");
        assert!(matches!(
            current_target(&db, "com.firstwork.formal-auth-service", "ai.invoke"),
            Err(AppError::PluginAuthorizationSnapshotNotDeclared)
        ));
    }

    #[test]
    fn authorization_isolated_by_actor_and_revocation_survives_version_switch() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            None,
        )
        .expect("grant");

        let other_subject = PluginAuthorizationSubject {
            kind: PluginAuthorizationSubjectKind::PlatformUser,
            id: "other-platform-user".into(),
        };
        let other_context = PluginAuthorizationContext {
            kind: PluginAuthorizationContextKind::HostInstallation,
            id: "other-host-installation".into(),
        };
        for (candidate_subject, candidate_context) in
            [(&other_subject, &context), (&subject, &other_context)]
        {
            let view = list_current_formal_plugin_capability_authorizations_for_actor(
                &db,
                candidate_subject,
                candidate_context,
                "com.firstwork.formal-auth-service",
            )
            .expect("isolated view");
            assert_eq!(
                view[0].status,
                CurrentPluginCapabilityAuthorizationStatus::Missing
            );
            assert!(!view[0].effective);
        }

        revoke_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
        )
        .expect("revoke");
        let mut next = manifest(&["ai.invoke"]);
        next.version = "2.0.0".into();
        db.record_plugin_version(
            &next,
            "C:/test/formal-auth-service/2.0.0",
            "formal-auth-service-v2-hash",
            &[],
        )
        .expect("switch current version");

        let view = list_current_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect("revocation after version switch");
        assert_eq!(
            view[0].status,
            CurrentPluginCapabilityAuthorizationStatus::Revoked
        );
        assert!(!view[0].effective);
    }

    fn feature_scope(feature_id: &str, fingerprint: &str) -> TrustedResourceScope {
        TrustedResourceScope::feature("com.firstwork.formal-auth-service", feature_id, fingerprint)
    }

    #[test]
    fn exact_resource_scopes_coexist_and_mutations_only_touch_the_target() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        let scope_a = feature_scope("feature-a", "fingerprint-a");
        let scope_b = feature_scope("feature-b", "fingerprint-b");

        let granted_a = grant_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope_a,
            None,
        )
        .expect("grant resource A");
        let pending_b = request_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope_b,
            None,
        )
        .expect("request resource B");
        assert_ne!(granted_a.scope, pending_b.scope);

        let view_a = current_formal_plugin_capability_authorization_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope_a,
        )
        .expect("read A");
        let view_b = current_formal_plugin_capability_authorization_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope_b,
        )
        .expect("read B");
        assert_eq!(
            view_a.status,
            CurrentPluginCapabilityAuthorizationStatus::Granted
        );
        assert_eq!(
            view_b.status,
            CurrentPluginCapabilityAuthorizationStatus::Pending
        );

        deny_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope_b,
        )
        .expect("deny B");
        revoke_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope_a,
        )
        .expect("revoke A");
        let records = list_formal_plugin_capability_authorizations_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
        )
        .expect("list exact records");
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .any(|record| record.state == PluginAuthorizationState::Denied));
        assert!(records
            .iter()
            .any(|record| record.state == PluginAuthorizationState::Revoked));
    }

    #[test]
    fn global_grant_never_authorizes_a_restricted_resource_scope() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        grant_for_actor(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            None,
        )
        .expect("legacy-compatible global grant");
        let error = current_formal_plugin_capability_authorization_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &feature_scope("feature-a", "fingerprint-a"),
        )
        .expect_err("global cannot cover feature resource");
        assert!(matches!(error, AppError::PluginAuthorizationScopeMismatch));
    }

    #[test]
    fn exact_resource_expiry_and_damaged_stored_scope_fail_closed() {
        let (db, subject, context) = setup(&["ai.invoke"]);
        let scope = feature_scope("feature-a", "fingerprint-a");
        let expired_at =
            (Utc::now() - Duration::minutes(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
        grant_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope,
            Some(expired_at),
        )
        .expect("grant expired resource");
        let expired = current_formal_plugin_capability_authorization_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope,
        )
        .expect("read expired resource");
        assert_eq!(
            expired.status,
            CurrentPluginCapabilityAuthorizationStatus::Expired
        );
        expire_for_actor_and_scope(
            &db,
            &subject,
            &context,
            "com.firstwork.formal-auth-service",
            "ai.invoke",
            &scope,
        )
        .expect("persist exact expiry");

        db.conn_lock()
            .expect("connection")
            .execute(
                "UPDATE plugin_capability_authorizations SET scope_key = 'v2:corrupt'",
                [],
            )
            .expect("damage stored scope");
        assert!(matches!(
            list_formal_plugin_capability_authorizations_for_actor(
                &db,
                &subject,
                &context,
                "com.firstwork.formal-auth-service"
            ),
            Err(AppError::PluginAuthorizationStoredRecordInvalid { reason: "scope" })
        ));
    }
}
