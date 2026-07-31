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
    resolve_host_installation_context, resolve_verified_platform_subject,
};

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
        scope: canonicalize_authorization_scope(GLOBAL_SCOPE_KIND, GLOBAL_SCOPE_KEY)?,
    })
}

fn list_current_formal_plugin_capability_authorizations_for_actor(
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

fn request_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, AppError> {
    let target = current_target(db, plugin_id, capability_id)?;
    if let Some(record) = current_record(db, subject, context, plugin_id, capability_id, &target)? {
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
    let pending = match current_record(db, subject, context, plugin_id, capability_id, &target)? {
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
    let pending = match current_record(db, subject, context, plugin_id, capability_id, &target)? {
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
    let record = current_record(db, subject, context, plugin_id, capability_id, &target)?
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
    let record = current_record(db, subject, context, plugin_id, capability_id, &target)?
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

fn current_record(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    target: &CurrentCapabilityTarget,
) -> Result<Option<PluginCapabilityAuthorization>, AppError> {
    let records = db.list_formal_plugin_capability_authorizations(subject, context, plugin_id)?;
    let mut capability_records = records
        .into_iter()
        .filter(|record| record.capability_id == capability_id);
    let record = capability_records
        .clone()
        .find(|record| record.scope == target.scope);
    if record.is_none() && capability_records.next().is_some() {
        return Err(AppError::PluginAuthorizationScopeMismatch);
    }
    if let Some(record) = record.as_ref() {
        validate_record_semantic_version(record, &target.capability_semantic_version)?;
    }
    Ok(record)
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
}
