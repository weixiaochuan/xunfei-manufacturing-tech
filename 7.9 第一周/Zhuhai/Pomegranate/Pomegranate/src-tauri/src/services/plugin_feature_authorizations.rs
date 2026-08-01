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

#[derive(Debug, Clone)]
pub(crate) struct FeatureAuthorizationView {
    pub(crate) capability_id: String,
    pub(crate) target_kind: &'static str,
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

#[derive(Clone, Copy)]
struct FeatureTarget<'a> {
    capability_id: &'static str,
    target_kind: &'static str,
    contribution_id: &'a str,
    title: &'a str,
    hook: &'static str,
    scenes: &'a [crate::models::PluginScene],
    features: &'a [String],
}

impl FeatureTarget<'_> {
    fn scope(&self, manifest: &PluginManifestV3) -> Result<TrustedResourceScope, AppError> {
        match self.capability_id {
            "ai.context.augment" => {
                TrustedResourceScope::for_declarative_enhancement(manifest, self.contribution_id)
            }
            "ai.invoke" => {
                TrustedResourceScope::for_xingchen_feature(manifest, self.contribution_id)
            }
            _ => Err(AppError::PluginAuthorizationCapabilityInvalid {
                reason: "capability_not_admitted",
            }),
        }
    }
}

fn feature_targets(manifest: &PluginManifestV3) -> Vec<FeatureTarget<'_>> {
    let mut targets = Vec::new();
    if manifest
        .permissions
        .iter()
        .any(|value| value == "ai.context.augment")
    {
        targets.extend(
            manifest
                .contributes
                .enhancements
                .iter()
                .map(|item| FeatureTarget {
                    capability_id: "ai.context.augment",
                    target_kind: "enhancement",
                    contribution_id: &item.id,
                    title: &item.title,
                    hook: "contextEnhancement",
                    scenes: &item.scenes,
                    features: &item.features,
                }),
        );
    }
    if manifest
        .permissions
        .iter()
        .any(|value| value == "ai.invoke")
        && matches!(
            manifest.runtime_kind,
            crate::models::PluginRuntimeKind::XingchenAgent
                | crate::models::PluginRuntimeKind::XingchenWorkflow
        )
    {
        targets.extend(
            manifest
                .contributes
                .features
                .iter()
                .map(|item| FeatureTarget {
                    capability_id: "ai.invoke",
                    target_kind: "xingchenFeature",
                    contribution_id: &item.id,
                    title: &item.title,
                    hook: "featureInvoke",
                    scenes: &item.scenes,
                    features: &[],
                }),
        );
    }
    targets
}

fn feature_target<'a>(
    manifest: &'a PluginManifestV3,
    contribution_id: &str,
) -> Result<FeatureTarget<'a>, AppError> {
    feature_targets(manifest)
        .into_iter()
        .find(|target| target.contribution_id == contribution_id)
        .ok_or_else(|| AppError::InvalidInput("功能不存在或不可访问".into()))
}

pub(crate) async fn list_feature_authorizations(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
) -> Result<Vec<FeatureAuthorizationView>, AppError> {
    let (subject, context) = resolve_actor(db, account).await?;
    let manifest = current_manifest(db, plugin_id)?;
    feature_targets(&manifest)
        .into_iter()
        .map(|target| view_for_target(db, &subject, &context, &manifest, target))
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
    let target = feature_target(&manifest, contribution_id)?;
    view_for_target(db, &subject, &context, &manifest, target)
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
    let target = feature_target(&manifest, contribution_id)?;
    validate_plugin_grant_target(db, plugin_id, target.capability_id)?;
    let scope = target.scope(&manifest)?;
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
        Some(&format!("{}:feature", target.capability_id)),
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
            target.capability_id,
            &scope,
            expires_at,
        )?,
        FeatureAuthorizationAction::Grant => grant_for_actor_and_scope(
            db,
            subject,
            context,
            plugin_id,
            target.capability_id,
            &scope,
            expires_at,
        )?,
        FeatureAuthorizationAction::Deny => deny_for_actor_and_scope(
            db,
            subject,
            context,
            plugin_id,
            target.capability_id,
            &scope,
        )?,
        FeatureAuthorizationAction::Revoke => revoke_for_actor_and_scope(
            db,
            subject,
            context,
            plugin_id,
            target.capability_id,
            &scope,
        )?,
        FeatureAuthorizationAction::Expire => expire_for_actor_and_scope(
            db,
            subject,
            context,
            plugin_id,
            target.capability_id,
            &scope,
        )?,
    };
    // 状态写入已由正式授权表完成；完成事件只作审计，不反向改变授权结果。
    db.write_audit_log(
        plugin_id,
        &format!("{operation}_completed"),
        Some(&format!("{}:feature", target.capability_id)),
    )
    .ok();
    view_for_target(db, subject, context, &manifest, target)
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

fn view_for_target(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    manifest: &PluginManifestV3,
    target: FeatureTarget<'_>,
) -> Result<FeatureAuthorizationView, AppError> {
    let scope = target.scope(manifest)?;
    let authorization = current_exact_plugin_capability_authorization_for_actor_and_scope(
        db,
        subject,
        context,
        &manifest.id,
        target.capability_id,
        &scope,
    )?;
    let scenes = target
        .scenes
        .iter()
        .map(enum_name)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FeatureAuthorizationView {
        capability_id: target.capability_id.to_string(),
        target_kind: target.target_kind,
        contribution_id: target.contribution_id.to_string(),
        title: target.title.to_string(),
        hook: target.hook.to_string(),
        scenes,
        features: target.features.to_vec(),
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

    fn setup_xingchen() -> Fixture {
        let db = Database::init(":memory:").expect("database");
        let directory = std::env::temp_dir().join(format!(
            "pomegranate-xingchen-feature-auth-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("directory");
        fs::write(directory.join("ui.json"), "{}").expect("asset");
        let manifest: PluginManifestV3 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.xingchen-feature-auth-tests",
            "name": "Xingchen Feature Authorization Tests",
            "version": "1.0.0",
            "authorId": "tests",
            "classification": "feature",
            "runtimeKind": "xingchen-workflow",
            "source": "local",
            "supportedScenes": ["global"],
            "permissions": ["ai.invoke"],
            "contributes": { "features": [
                { "id": "feature-a", "title": "Feature A", "scenes": ["global"], "uiSchema": "ui.json" },
                { "id": "feature-b", "title": "Feature B", "scenes": ["global"], "uiSchema": "ui.json" }
            ]},
            "integrity": {"sha256": null},
            "signature": {"status": "unsigned", "signer": null}
        }))
        .expect("manifest");
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
        let other = view_for_target(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            &fixture.manifest,
            feature_target(&fixture.manifest, "enhancement-b").unwrap(),
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
    fn xingchen_feature_authorization_uses_ai_invoke_and_is_feature_isolated() {
        let fixture = setup_xingchen();
        let granted = mutate(&fixture, "feature-a", FeatureAuthorizationAction::Grant)
            .expect("grant xingchen feature");
        assert_eq!(granted.capability_id, "ai.invoke");
        assert_eq!(granted.target_kind, "xingchenFeature");
        assert!(granted.authorization.effective);

        let other = view_for_target(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            &fixture.manifest,
            feature_target(&fixture.manifest, "feature-b").unwrap(),
        )
        .expect("other feature");
        assert!(!other.authorization.effective);

        let feature_scope =
            TrustedResourceScope::for_xingchen_feature(&fixture.manifest, "feature-a")
                .unwrap()
                .canonical_scope()
                .unwrap();
        assert_eq!(feature_scope.kind, "feature");
        assert_ne!(
            feature_scope,
            canonicalize_authorization_scope("global", "v1:*").unwrap()
        );
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
