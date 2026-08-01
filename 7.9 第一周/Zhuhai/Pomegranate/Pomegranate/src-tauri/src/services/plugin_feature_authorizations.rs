use crate::account::AccountState;
use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    CurrentPluginCapabilityAuthorization, PluginAuthorizationContext, PluginAuthorizationSubject,
    PluginManifestV3,
};

use super::plugin_authorization_context::{
    resolve_host_installation_context, resolve_verified_platform_subject, TrustedResourceScope,
};
use super::plugin_authorizations::{
    current_exact_plugin_capability_authorization_for_actor_and_scope, deny_for_actor_and_scope,
    expire_for_actor_and_scope, grant_for_actor_and_scope, request_for_actor_and_scope,
    revoke_for_actor_and_scope,
};
use super::plugin_exact_authorizations::{
    validate_exact_authorization_expiration, validate_plugin_grant_target,
};

const CAPABILITY_ID: &str = "ai.context.augment";

#[derive(Debug, Clone)]
pub(crate) struct FeatureAuthorizationView {
    pub(crate) contribution_id: String,
    pub(crate) title: String,
    pub(crate) hook: String,
    pub(crate) scenes: Vec<String>,
    pub(crate) features: Vec<String>,
    pub(crate) authorization: CurrentPluginCapabilityAuthorization,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FeatureAuthorizationAction {
    Request,
    Grant,
    Deny,
    Revoke,
    Expire,
}

pub(crate) async fn list_feature_authorizations(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
) -> Result<Vec<FeatureAuthorizationView>, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    let manifest = current_manifest(db, plugin_id)?;
    manifest
        .contributes
        .enhancements
        .iter()
        .map(|contribution| view_for_actor(db, &subject, &context, &manifest, &contribution.id))
        .collect()
}

pub(crate) async fn query_feature_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    contribution_id: &str,
) -> Result<FeatureAuthorizationView, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    let manifest = current_manifest(db, plugin_id)?;
    view_for_actor(db, &subject, &context, &manifest, contribution_id)
}

pub(crate) async fn mutate_feature_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    contribution_id: &str,
    action: FeatureAuthorizationAction,
    expires_at: Option<String>,
) -> Result<FeatureAuthorizationView, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    mutate_for_actor(
        db,
        &subject,
        &context,
        plugin_id,
        contribution_id,
        action,
        expires_at,
    )
}

fn mutate_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    contribution_id: &str,
    action: FeatureAuthorizationAction,
    expires_at: Option<String>,
) -> Result<FeatureAuthorizationView, AppError> {
    let manifest = current_manifest(db, plugin_id)?;
    let scope = TrustedResourceScope::for_declarative_enhancement(&manifest, contribution_id)?;
    let operation = match action {
        FeatureAuthorizationAction::Request => "feature_authorization_request",
        FeatureAuthorizationAction::Grant => "feature_authorization_grant",
        FeatureAuthorizationAction::Deny => "feature_authorization_deny",
        FeatureAuthorizationAction::Revoke => "feature_authorization_revoke",
        FeatureAuthorizationAction::Expire => "feature_authorization_expire",
    };
    // 先记录不含 contribution ID/scope key 的固定审计事件，确保后续拒绝也有正式证据。
    db.write_audit_log(
        plugin_id,
        &format!("{operation}_attempt"),
        Some("ai.context.augment:feature"),
    )?;
    let expires_at = match action {
        FeatureAuthorizationAction::Request | FeatureAuthorizationAction::Grant => {
            validate_exact_authorization_expiration(expires_at, chrono::Utc::now())?
        }
        _ => None,
    };
    match action {
        FeatureAuthorizationAction::Request => request_for_actor_and_scope(
            db,
            subject,
            context,
            plugin_id,
            CAPABILITY_ID,
            &scope,
            expires_at,
        )?,
        FeatureAuthorizationAction::Grant => grant_for_actor_and_scope(
            db,
            subject,
            context,
            plugin_id,
            CAPABILITY_ID,
            &scope,
            expires_at,
        )?,
        FeatureAuthorizationAction::Deny => {
            deny_for_actor_and_scope(db, subject, context, plugin_id, CAPABILITY_ID, &scope)?
        }
        FeatureAuthorizationAction::Revoke => {
            revoke_for_actor_and_scope(db, subject, context, plugin_id, CAPABILITY_ID, &scope)?
        }
        FeatureAuthorizationAction::Expire => {
            expire_for_actor_and_scope(db, subject, context, plugin_id, CAPABILITY_ID, &scope)?
        }
    };
    // 状态写入已由正式授权表完成；完成事件只作审计，不反向改变授权结果。
    db.write_audit_log(
        plugin_id,
        &format!("{operation}_completed"),
        Some("ai.context.augment:feature"),
    )
    .ok();
    view_for_actor(db, subject, context, &manifest, contribution_id)
}

async fn resolve_actor(
    db: &Database,
    account: &AccountState,
) -> Result<(PluginAuthorizationSubject, PluginAuthorizationContext), AppError> {
    Ok((
        resolve_verified_platform_subject(account).await?,
        resolve_host_installation_context(db)?,
    ))
}

fn current_manifest(db: &Database, plugin_id: &str) -> Result<PluginManifestV3, AppError> {
    validate_plugin_grant_target(db, plugin_id, CAPABILITY_ID)?;
    let snapshot = db
        .current_plugin_authorization_snapshot(plugin_id, &[])?
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_not_installed",
        })?;
    Ok(snapshot
        .current_version
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_current_version_missing",
        })?
        .manifest)
}

fn view_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    manifest: &PluginManifestV3,
    contribution_id: &str,
) -> Result<FeatureAuthorizationView, AppError> {
    let contribution = manifest
        .contributes
        .enhancements
        .iter()
        .find(|item| item.id == contribution_id)
        .ok_or_else(|| AppError::InvalidInput("声明式贡献不存在或不可访问".to_string()))?;
    let scope = TrustedResourceScope::for_declarative_enhancement(manifest, contribution_id)?;
    let authorization = current_exact_plugin_capability_authorization_for_actor_and_scope(
        db,
        subject,
        context,
        &manifest.id,
        CAPABILITY_ID,
        &scope,
    )?;
    let hook = enum_name(&contribution.hook)?;
    let scenes = contribution
        .scenes
        .iter()
        .map(enum_name)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FeatureAuthorizationView {
        contribution_id: contribution.id.clone(),
        title: contribution.title.clone(),
        hook,
        scenes,
        features: contribution.features.clone(),
        authorization,
    })
}

fn enum_name<T: serde::Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .ok_or_else(|| AppError::InvalidInput("声明式贡献不存在或不可访问".to_string()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::models::{PluginAuthorizationSubjectKind, PluginCapabilityAuthorization};
    use crate::services::plugin_authorization_context::canonicalize_authorization_scope;
    use crate::services::plugins::PluginService;

    struct Fixture {
        db: Database,
        directory: std::path::PathBuf,
        subject: PluginAuthorizationSubject,
        context: PluginAuthorizationContext,
        manifest: PluginManifestV3,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.directory).ok();
        }
    }

    fn setup() -> Fixture {
        let db = Database::init(":memory:").expect("database");
        let directory =
            std::env::temp_dir().join(format!("pomegranate-feature-auth-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("directory");
        fs::write(directory.join("prompt.md"), "safe prompt").expect("asset");
        let manifest: PluginManifestV3 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.feature-auth-tests",
            "name": "Feature Authorization Tests",
            "version": "1.0.0",
            "authorId": "tests",
            "classification": "enhancement",
            "runtimeKind": "prompt-pack",
            "source": "local",
            "supportedScenes": ["global"],
            "permissions": ["ai.context.augment"],
            "contributes": { "enhancements": [
                { "id": "enhancement-a", "title": "A", "hook": "promptEnhancer", "scenes": ["global"], "features": ["chat"], "handler": { "kind": "declarative", "resource": "prompt.md" } },
                { "id": "enhancement-b", "title": "B", "hook": "promptEnhancer", "scenes": ["global"], "features": ["summary"], "handler": { "kind": "declarative", "resource": "prompt.md" } }
            ]},
            "integrity": {"sha256": null},
            "signature": {"status": "unsigned", "signer": null}
        })).expect("manifest");
        let hash = PluginService::calculate_integrity_for_path(&directory).expect("hash");
        db.record_plugin_version(
            &manifest,
            &directory.to_string_lossy(),
            &hash,
            &manifest.permissions,
        )
        .expect("record");
        db.set_plugin_enabled(&manifest.id, true).expect("enable");
        let context = resolve_host_installation_context(&db).expect("context");
        Fixture {
            db,
            directory,
            subject: PluginAuthorizationSubject {
                kind: PluginAuthorizationSubjectKind::PlatformUser,
                id: "subject-a".into(),
            },
            context,
            manifest,
        }
    }

    fn mutate(
        fixture: &Fixture,
        id: &str,
        action: FeatureAuthorizationAction,
    ) -> Result<FeatureAuthorizationView, AppError> {
        mutate_for_actor(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            &fixture.manifest.id,
            id,
            action,
            matches!(
                action,
                FeatureAuthorizationAction::Request | FeatureAuthorizationAction::Grant
            )
            .then(|| (Utc::now() + Duration::hours(1)).to_rfc3339()),
        )
    }

    #[test]
    fn feature_authorization_lifecycle_is_canonical_and_contribution_isolated() {
        let fixture = setup();
        let pending = mutate(
            &fixture,
            "enhancement-a",
            FeatureAuthorizationAction::Request,
        )
        .expect("request");
        assert_eq!(format!("{:?}", pending.authorization.status), "Pending");
        let granted =
            mutate(&fixture, "enhancement-a", FeatureAuthorizationAction::Grant).expect("grant");
        assert!(granted.authorization.effective);
        let other = view_for_actor(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            &fixture.manifest,
            "enhancement-b",
        )
        .expect("other");
        assert!(!other.authorization.effective);

        let scope_a =
            TrustedResourceScope::for_declarative_enhancement(&fixture.manifest, "enhancement-a")
                .unwrap();
        let scope_b =
            TrustedResourceScope::for_declarative_enhancement(&fixture.manifest, "enhancement-b")
                .unwrap();
        assert_ne!(
            scope_a.canonical_scope().unwrap(),
            scope_b.canonical_scope().unwrap()
        );
        let global = canonicalize_authorization_scope("global", "v1:*").unwrap();
        let records: Vec<PluginCapabilityAuthorization> = fixture
            .db
            .list_formal_plugin_capability_authorizations(
                &fixture.subject,
                &fixture.context,
                &fixture.manifest.id,
            )
            .unwrap();
        assert!(records
            .iter()
            .any(|record| record.scope == scope_a.canonical_scope().unwrap()));
        assert!(!records.iter().any(|record| record.scope == global));

        let revoked = mutate(
            &fixture,
            "enhancement-a",
            FeatureAuthorizationAction::Revoke,
        )
        .expect("revoke");
        assert!(!revoked.authorization.effective);

        let denied =
            mutate(&fixture, "enhancement-b", FeatureAuthorizationAction::Deny).expect("deny");
        assert_eq!(format!("{:?}", denied.authorization.status), "Denied");
        let logs = fixture
            .db
            .get_plugin_audit_log(&fixture.manifest.id, 20)
            .unwrap();
        assert!(logs
            .iter()
            .any(|item| item.2 == "feature_authorization_grant_completed"));
        assert!(logs
            .iter()
            .all(|item| item.3.as_deref() == Some("ai.context.augment:feature")));
    }

    #[test]
    fn rejects_forged_contribution_and_disabled_plugin() {
        let fixture = setup();
        assert!(mutate(&fixture, "missing", FeatureAuthorizationAction::Grant).is_err());
        fixture
            .db
            .set_plugin_enabled(&fixture.manifest.id, false)
            .expect("disable");
        assert!(mutate(&fixture, "enhancement-a", FeatureAuthorizationAction::Grant).is_err());
    }
}
