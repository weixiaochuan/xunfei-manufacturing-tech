use crate::account::AccountState;
use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    CurrentPluginCapabilityAuthorizationStatus, PluginAuthorizationContext,
    PluginAuthorizationState, PluginAuthorizationSubject,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};

use super::credentials::CredentialService;
use super::plugin_authorization_context::{
    resolve_host_installation_context, resolve_verified_platform_subject, TrustedResourceScope,
};
use super::plugin_authorizations::{
    current_exact_plugin_capability_authorization_for_actor_and_scope, grant_for_actor_and_scope,
    list_formal_plugin_capability_authorizations_for_actor,
};
use super::plugin_capabilities::{canonical_capability_policy, is_v3_permission_runtime_allowed};
use super::plugins::PluginService;
use super::resource_ownership::{resolve_resource_owner, ResourceOwner};
use super::resource_resolution::agent_children::resolve_workflow;
use super::resource_resolution::credential::resolve_credential;
use super::resource_resolution::external_agent::{
    resolve_external_agent, ExternalAgentRuntimeKind,
};
use super::resource_resolution::{ResolverError, UntrustedResourceRef};
use super::xingchen_agent::XingchenAgentService;

const AUTHORIZATION_HANDLE_PREFIX: &str = "exact-auth-v1:";
const MAX_EXACT_AUTHORIZATION_HOURS: i64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactAuthorizableResourceKind {
    Credential,
    ExternalAgent,
    Workflow,
}

impl ExactAuthorizableResourceKind {
    pub(crate) fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "credential" => Ok(Self::Credential),
            "external-agent" => Ok(Self::ExternalAgent),
            "workflow" => Ok(Self::Workflow),
            _ => Err(AppError::InvalidInput("不支持的具体资源类型".into())),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::ExternalAgent => "external-agent",
            Self::Workflow => "workflow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactAuthorizationView {
    pub(crate) authorization_id: Option<String>,
    pub(crate) plugin_id: String,
    pub(crate) capability_id: String,
    pub(crate) resource_kind: String,
    pub(crate) status: CurrentPluginCapabilityAuthorizationStatus,
    pub(crate) effective: bool,
    pub(crate) available: Option<bool>,
    pub(crate) expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactAuthorizationResourceOption {
    pub(crate) resource_kind: String,
    pub(crate) resource_id: String,
    pub(crate) display_name: String,
    pub(crate) compatible_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactAuthorizationCatalog {
    pub(crate) capability_ids: Vec<String>,
    pub(crate) resources: Vec<ExactAuthorizationResourceOption>,
    pub(crate) max_duration_hours: i64,
}

pub(crate) async fn list_exact_authorization_catalog(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
) -> Result<ExactAuthorizationCatalog, AppError> {
    let (owner, _, _) = resolve_actor(db, account).await?;
    catalog_for_owner(db, &owner, plugin_id)
}

pub(crate) async fn grant_exact_resource_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    capability_id: &str,
    resource_kind: &str,
    resource_id: &str,
    expires_at: Option<String>,
) -> Result<ExactAuthorizationView, AppError> {
    let (owner, subject, context) = resolve_actor(db, account).await?;
    grant_for_owner(
        db,
        &owner,
        &subject,
        &context,
        plugin_id,
        capability_id,
        ExactAuthorizableResourceKind::parse(resource_kind)?,
        resource_id,
        expires_at,
    )
}

pub(crate) async fn query_exact_resource_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    capability_id: &str,
    resource_kind: &str,
    resource_id: &str,
) -> Result<ExactAuthorizationView, AppError> {
    let (owner, subject, context) = resolve_actor(db, account).await?;
    query_for_owner(
        db,
        &owner,
        &subject,
        &context,
        plugin_id,
        capability_id,
        ExactAuthorizableResourceKind::parse(resource_kind)?,
        resource_id,
    )
}

pub(crate) async fn list_exact_resource_authorizations(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
) -> Result<Vec<ExactAuthorizationView>, AppError> {
    let subject = resolve_verified_platform_subject(account).await?;
    let context = resolve_host_installation_context(db)?;
    list_for_actor(db, &subject, &context, plugin_id)
}

pub(crate) async fn revoke_exact_resource_authorization(
    db: &Database,
    account: &AccountState,
    plugin_id: &str,
    authorization_id: &str,
) -> Result<ExactAuthorizationView, AppError> {
    let subject = resolve_verified_platform_subject(account).await?;
    let context = resolve_host_installation_context(db)?;
    revoke_for_actor(db, &subject, &context, plugin_id, authorization_id)
}

async fn resolve_actor(
    db: &Database,
    account: &AccountState,
) -> Result<
    (
        ResourceOwner,
        PluginAuthorizationSubject,
        PluginAuthorizationContext,
    ),
    AppError,
> {
    let owner = resolve_resource_owner(db, account).await?;
    let subject = resolve_verified_platform_subject(account).await?;
    let context = resolve_host_installation_context(db)?;
    if owner.platform_subject_id() != subject.id || owner.host_installation_id() != context.id {
        return Err(AppError::ResourceOwnerContextInvalid {
            reason: "authorization_actor_mismatch",
        });
    }
    Ok((owner, subject, context))
}

fn grant_for_owner(
    db: &Database,
    owner: &ResourceOwner,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    resource_kind: ExactAuthorizableResourceKind,
    resource_id: &str,
    expires_at: Option<String>,
) -> Result<ExactAuthorizationView, AppError> {
    validate_plugin_grant_target(db, plugin_id, capability_id)?;
    let scope = resolve_scope(db, owner, capability_id, resource_kind, resource_id)?;
    let expires_at = validate_exact_authorization_expiration(expires_at, Utc::now())?;
    let record = grant_for_actor_and_scope(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        &scope,
        expires_at,
    )?;
    Ok(view_from_record(&record, resource_kind.as_str(), true))
}

fn catalog_for_owner(
    db: &Database,
    owner: &ResourceOwner,
    plugin_id: &str,
) -> Result<ExactAuthorizationCatalog, AppError> {
    let snapshot = db
        .current_plugin_authorization_snapshot(plugin_id, &[])?
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_not_installed",
        })?;
    let current = snapshot
        .current_version
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_current_version_missing",
        })?;
    let mut capability_ids = Vec::new();
    for capability_id in ["credentials.use", "agents.invoke", "network.xingchen"] {
        if current
            .manifest
            .permissions
            .iter()
            .any(|permission| permission == capability_id)
        {
            validate_plugin_grant_target(db, plugin_id, capability_id)?;
            capability_ids.push(capability_id.to_string());
        }
    }

    let mut resources = Vec::new();
    if capability_ids.iter().any(|id| id == "credentials.use") {
        for credential in CredentialService::list(db, owner)? {
            if catalog_scope_available(resolve_scope(
                db,
                owner,
                "credentials.use",
                ExactAuthorizableResourceKind::Credential,
                &credential.id,
            ))? {
                resources.push(ExactAuthorizationResourceOption {
                    resource_kind: "credential".to_string(),
                    resource_id: credential.id,
                    display_name: credential.label,
                    compatible_capabilities: vec!["credentials.use".to_string()],
                });
            }
        }
    }

    let agent_capabilities: Vec<String> = capability_ids
        .iter()
        .filter(|id| matches!(id.as_str(), "agents.invoke" | "network.xingchen"))
        .cloned()
        .collect();
    if !agent_capabilities.is_empty() {
        for agent in XingchenAgentService::list_agents(db, owner)? {
            let mut resolved_kind = None;
            for kind in [
                ExactAuthorizableResourceKind::ExternalAgent,
                ExactAuthorizableResourceKind::Workflow,
            ] {
                for capability_id in &agent_capabilities {
                    if catalog_scope_available(resolve_scope(
                        db,
                        owner,
                        capability_id,
                        kind,
                        &agent.id,
                    ))? {
                        resolved_kind = Some(kind);
                        break;
                    }
                }
                if resolved_kind.is_some() {
                    break;
                }
            }
            if let Some(kind) = resolved_kind {
                let mut compatible_capabilities = Vec::new();
                for capability_id in &agent_capabilities {
                    if catalog_scope_available(resolve_scope(
                        db,
                        owner,
                        capability_id,
                        kind,
                        &agent.id,
                    ))? {
                        compatible_capabilities.push(capability_id.clone());
                    }
                }
                resources.push(ExactAuthorizationResourceOption {
                    resource_kind: kind.as_str().to_string(),
                    resource_id: agent.id,
                    display_name: agent.name,
                    compatible_capabilities,
                });
            }
        }
    }

    Ok(ExactAuthorizationCatalog {
        capability_ids,
        resources,
        max_duration_hours: MAX_EXACT_AUTHORIZATION_HOURS,
    })
}

pub(super) fn validate_exact_authorization_expiration(
    expires_at: Option<String>,
    now: DateTime<Utc>,
) -> Result<Option<String>, AppError> {
    let raw = expires_at.ok_or_else(|| {
        AppError::InvalidInput("具体资源授权必须选择不超过 24 小时的有效期".to_string())
    })?;
    let expires = DateTime::parse_from_rfc3339(&raw)
        .map_err(|_| AppError::InvalidInput("授权到期时间格式无效".to_string()))?
        .with_timezone(&Utc);
    if expires <= now {
        return Err(AppError::InvalidInput(
            "授权到期时间必须晚于当前时间".to_string(),
        ));
    }
    if expires > now + Duration::hours(MAX_EXACT_AUTHORIZATION_HOURS) {
        return Err(AppError::InvalidInput(
            "具体资源授权最长为 24 小时".to_string(),
        ));
    }
    Ok(Some(expires.to_rfc3339_opts(SecondsFormat::Secs, true)))
}

fn query_for_owner(
    db: &Database,
    owner: &ResourceOwner,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    resource_kind: ExactAuthorizableResourceKind,
    resource_id: &str,
) -> Result<ExactAuthorizationView, AppError> {
    let scope = resolve_scope(db, owner, capability_id, resource_kind, resource_id)?;
    let current = current_exact_plugin_capability_authorization_for_actor_and_scope(
        db,
        subject,
        context,
        plugin_id,
        capability_id,
        &scope,
    )?;
    let authorization_id =
        matching_record_id(db, subject, context, plugin_id, capability_id, &scope)?;
    Ok(ExactAuthorizationView {
        authorization_id: authorization_id.map(encode_authorization_handle),
        plugin_id: current.plugin_id,
        capability_id: current.capability_id,
        resource_kind: resource_kind.as_str().to_string(),
        status: current.status,
        effective: current.effective,
        available: Some(true),
        expires_at: current.expires_at,
    })
}

fn list_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
) -> Result<Vec<ExactAuthorizationView>, AppError> {
    list_formal_plugin_capability_authorizations_for_actor(db, subject, context, plugin_id)?
        .into_iter()
        .filter(|record| is_exact_scope_kind(&record.scope.kind))
        .map(|record| {
            let kind = safe_kind_from_scope(&record.scope.kind);
            Ok(view_from_record(&record, kind, false))
        })
        .collect()
}

fn revoke_for_actor(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    authorization_id: &str,
) -> Result<ExactAuthorizationView, AppError> {
    let id = decode_authorization_handle(authorization_id)?;
    let record =
        list_formal_plugin_capability_authorizations_for_actor(db, subject, context, plugin_id)?
            .into_iter()
            .find(|record| record.id == id && is_exact_scope_kind(&record.scope.kind))
            .ok_or(AppError::PluginAuthorizationNotFound)?;
    if record.state == PluginAuthorizationState::Revoked {
        return Ok(view_from_record(
            &record,
            safe_kind_from_scope(&record.scope.kind),
            false,
        ));
    }
    let revoked = db.revoke_granted_formal_plugin_capability_authorization(
        subject,
        context,
        plugin_id,
        &record.capability_id,
        &record.scope,
        record.revision,
    )?;
    Ok(view_from_record(
        &revoked,
        safe_kind_from_scope(&revoked.scope.kind),
        false,
    ))
}

fn resolve_scope(
    db: &Database,
    owner: &ResourceOwner,
    capability_id: &str,
    resource_kind: ExactAuthorizableResourceKind,
    resource_id: &str,
) -> Result<TrustedResourceScope, AppError> {
    let reference = UntrustedResourceRef::try_new(resource_kind.as_str(), resource_id.to_string())
        .map_err(resolver_error)?;
    match resource_kind {
        ExactAuthorizableResourceKind::Credential => {
            let credential = resolve_credential(db, owner, reference).map_err(resolver_error)?;
            TrustedResourceScope::for_credential(capability_id, &credential)
        }
        ExactAuthorizableResourceKind::ExternalAgent => {
            let agent = resolve_external_agent(db, owner, reference).map_err(resolver_error)?;
            if agent.runtime_kind() != ExternalAgentRuntimeKind::Agent {
                return Err(AppError::InvalidInput("资源不存在或不可访问".to_string()));
            }
            TrustedResourceScope::for_external_agent(capability_id, &agent)
        }
        ExactAuthorizableResourceKind::Workflow => {
            let workflow = resolve_workflow(db, owner, reference).map_err(resolver_error)?;
            TrustedResourceScope::for_workflow(capability_id, &workflow)
        }
    }
}

fn resolver_error(error: ResolverError) -> AppError {
    AppError::InvalidInput(error.public_message().to_string())
}

fn catalog_scope_available(
    result: Result<TrustedResourceScope, AppError>,
) -> Result<bool, AppError> {
    match result {
        Ok(_) => Ok(true),
        Err(AppError::InvalidInput(message)) if message == "资源不存在或不可访问" => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn validate_plugin_grant_target(
    db: &Database,
    plugin_id: &str,
    capability_id: &str,
) -> Result<(), AppError> {
    let snapshot = db
        .current_plugin_authorization_snapshot(plugin_id, &[])?
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_not_installed",
        })?;
    if snapshot.status != "installed" || !snapshot.enabled {
        return Err(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_not_enabled",
        });
    }
    let current = snapshot
        .current_version
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_current_version_missing",
        })?;
    if current.manifest.id != plugin_id || current.manifest.version != current.version {
        return Err(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_current_version_invalid",
        });
    }
    let integrity = PluginService::verify_installation(db, plugin_id)?;
    if integrity.expected_hash.is_empty() || integrity.actual_hash.is_empty() || !integrity.ok {
        return Err(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_integrity_invalid",
        });
    }
    let policy = canonical_capability_policy(capability_id)?.ok_or(
        AppError::PluginAuthorizationCapabilityInvalid {
            reason: "capability_not_admitted",
        },
    )?;
    if !matches!(policy.status.as_str(), "active" | "restricted") {
        return Err(AppError::PluginAuthorizationCapabilityInvalid {
            reason: "capability_not_active",
        });
    }
    let runtime = serde_json::to_value(&current.manifest.runtime_kind)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_runtime_invalid",
        })?;
    if !is_v3_permission_runtime_allowed(capability_id, &runtime)
        || !policy.runtime_kinds.contains(&runtime)
    {
        return Err(AppError::PluginAuthorizationCapabilityInvalid {
            reason: "capability_runtime_mismatch",
        });
    }
    let source = serde_json::to_value(&current.manifest.source)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .ok_or(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_source_invalid",
        })?;
    if !policy.plugin_sources.contains(&source) {
        return Err(AppError::PluginAuthorizationCapabilityInvalid {
            reason: "capability_source_mismatch",
        });
    }
    if source == "marketplace" {
        let state = db.marketplace_plugin_security_state(plugin_id)?.ok_or(
            AppError::PluginAuthorizationContextInvalid {
                reason: "marketplace_binding_missing",
            },
        )?;
        if !matches!(
            state.product_status.as_str(),
            "published" | "installed" | "active"
        ) || state.installation_status != "installed"
            || state.installed_version != current.version
            || state.version_status != "active"
            || state.signature_status.is_empty()
            || state.signature_status == "revoked"
        {
            return Err(AppError::PluginAuthorizationContextInvalid {
                reason: "marketplace_trust_invalid",
            });
        }
    }
    Ok(())
}

fn matching_record_id(
    db: &Database,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    scope: &TrustedResourceScope,
) -> Result<Option<i64>, AppError> {
    let canonical = scope.canonical_scope()?;
    Ok(db
        .get_formal_plugin_capability_authorization(
            subject,
            context,
            plugin_id,
            capability_id,
            &canonical,
        )?
        .map(|record| record.id))
}

fn view_from_record(
    record: &crate::models::PluginCapabilityAuthorization,
    resource_kind: &str,
    available: bool,
) -> ExactAuthorizationView {
    let expired = record.expires_at.as_ref().is_some_and(|value| {
        DateTime::parse_from_rfc3339(value)
            .map(|expires| expires.with_timezone(&Utc) <= Utc::now())
            .unwrap_or(true)
    });
    let status = match (record.state, expired) {
        (PluginAuthorizationState::Granted, true) => {
            CurrentPluginCapabilityAuthorizationStatus::Expired
        }
        (PluginAuthorizationState::Pending, _) => {
            CurrentPluginCapabilityAuthorizationStatus::Pending
        }
        (PluginAuthorizationState::Granted, false) => {
            CurrentPluginCapabilityAuthorizationStatus::Granted
        }
        (PluginAuthorizationState::Denied, _) => CurrentPluginCapabilityAuthorizationStatus::Denied,
        (PluginAuthorizationState::Revoked, _) => {
            CurrentPluginCapabilityAuthorizationStatus::Revoked
        }
        (PluginAuthorizationState::Expired, _) => {
            CurrentPluginCapabilityAuthorizationStatus::Expired
        }
    };
    ExactAuthorizationView {
        authorization_id: Some(encode_authorization_handle(record.id)),
        plugin_id: record.plugin_id.clone(),
        capability_id: record.capability_id.clone(),
        resource_kind: resource_kind.to_string(),
        status,
        effective: status == CurrentPluginCapabilityAuthorizationStatus::Granted,
        available: available.then_some(true),
        expires_at: record.expires_at.clone(),
    }
}

fn is_exact_scope_kind(kind: &str) -> bool {
    matches!(
        kind,
        "bound-credential" | "bound-agent" | "xingchen-service"
    )
}

fn safe_kind_from_scope(kind: &str) -> &'static str {
    match kind {
        "bound-credential" => "credential",
        "bound-agent" | "xingchen-service" => "agent-or-workflow",
        _ => "unavailable",
    }
}

fn encode_authorization_handle(id: i64) -> String {
    format!("{AUTHORIZATION_HANDLE_PREFIX}{id:x}")
}

fn decode_authorization_handle(value: &str) -> Result<i64, AppError> {
    let raw = value
        .strip_prefix(AUTHORIZATION_HANDLE_PREFIX)
        .filter(|raw| {
            !raw.is_empty()
                && raw
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or(AppError::PluginAuthorizationNotFound)?;
    i64::from_str_radix(raw, 16).map_err(|_| AppError::PluginAuthorizationNotFound)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use rusqlite::params;
    use uuid::Uuid;

    use super::*;
    use crate::models::{
        PluginAuthorizationSubjectKind, PluginClassification, PluginManifestV3, PluginRuntimeKind,
        PluginScene, PluginSource,
    };

    const PLUGIN_ID: &str = "com.firstwork.exact-authorization-tests";

    struct Fixture {
        db: Database,
        directory: PathBuf,
        owner: ResourceOwner,
        subject: PluginAuthorizationSubject,
        context: PluginAuthorizationContext,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.directory).ok();
        }
    }

    fn setup_fixture() -> Fixture {
        let db = Database::init(":memory:").expect("database");
        let directory = std::env::temp_dir().join(format!("pomegranate-r5-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("plugin directory");
        fs::write(directory.join("ui.json"), "{}").expect("plugin asset");
        let permissions = vec![
            "credentials.use".to_string(),
            "agents.invoke".to_string(),
            "network.xingchen".to_string(),
        ];
        let manifest = PluginManifestV3 {
            schema_version: 3,
            id: PLUGIN_ID.into(),
            name: "Exact Authorization Tests".into(),
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
            permissions: permissions.clone(),
            configuration_schema: None,
            dependencies: Vec::new(),
            conflicts_with: Vec::new(),
            contributes: Default::default(),
            integrity: Default::default(),
            signature: Default::default(),
        };
        let hash = PluginService::calculate_integrity_for_path(&directory).expect("hash");
        db.record_plugin_version(&manifest, &directory.to_string_lossy(), &hash, &permissions)
            .expect("record plugin");
        db.set_plugin_enabled(PLUGIN_ID, true)
            .expect("enable plugin");
        let context = resolve_host_installation_context(&db).expect("host context");
        let subject = PluginAuthorizationSubject {
            kind: PluginAuthorizationSubjectKind::PlatformUser,
            id: "subject-a".into(),
        };
        let owner = ResourceOwner::fixture(&subject.id, &context.id);
        Fixture {
            db,
            directory,
            owner,
            subject,
            context,
        }
    }

    fn seed_credential(fixture: &Fixture, id: &str, owner: Option<&ResourceOwner>) {
        let conn = fixture.db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO credentials
                (id, provider, credential_type, label, owner_scope, secret_reference,
                 configured, masked_hint)
             VALUES (?1, 'xingchen', 'api_key', 'safe label', 'deprecated',
                     'secret-must-never-leak', 1, '***')",
            [id],
        )
        .unwrap();
        if let Some(owner) = owner {
            conn.execute(
                "INSERT INTO credential_resource_ownership
                    (credential_id, platform_subject_id, host_installation_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    id,
                    owner.platform_subject_id(),
                    owner.host_installation_id()
                ],
            )
            .unwrap();
        }
    }

    fn seed_agent(fixture: &Fixture, id: &str, workflow: bool, owner: Option<&ResourceOwner>) {
        let runtime = if workflow {
            "xingchen-workflow"
        } else {
            "xingchen-agent"
        };
        let product_id = format!("product-{id}");
        let runtime_plugin_id = format!("runtime-{id}");
        let conn = fixture.db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO plugins (id, name, version, path, main, manifest_json, enabled, status)
             VALUES (?1, ?1, '1.0.0', '/tmp/mock', 'main.js', '{}', 1, 'installed')",
            [&runtime_plugin_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO products
                (id, developer_id, name, product_type, status, plugin_id,
                 developer_name, runtime_kind, review_status)
             VALUES (?1, 'dev', ?1, ?2, 'published', ?3, 'dev', ?2, 'approved')",
            params![product_id, runtime, runtime_plugin_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO product_versions
                (product_id, version, manifest_json, runtime_kind, source, content_hash,
                 signature_status, status, review_status)
             VALUES (?1, '1.0.0', '{\"deliveryMode\":\"byok\"}', ?2,
                     'marketplace', 'hash', 'verified', 'active', 'approved')",
            params![product_id, runtime],
        )
        .unwrap();
        let version_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO plugin_installations
                (plugin_id, product_id, product_version_id, installed_version, source,
                 enabled, install_path, content_hash, status)
             VALUES (?1, ?2, ?3, '1.0.0', 'marketplace', 1, '/tmp/mock', 'hash', 'installed')",
            params![runtime_plugin_id, product_id, version_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO external_agents
                (id, product_id, provider, name, endpoint, authentication_type,
                 streaming_type, request_mapping_json, response_mapping_json,
                 session_mapping_json, error_mapping_json, mock_mode, enabled)
             VALUES (?1, ?2, 'xingchen', 'agent', 'mock://xingchen', 'none',
                     'none', '{}', '{}', '{}', '{}', 1, 1)",
            params![id, product_id],
        )
        .unwrap();
        if let Some(owner) = owner {
            conn.execute(
                "INSERT INTO external_agent_resource_ownership
                    (external_agent_id, platform_subject_id, host_installation_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    id,
                    owner.platform_subject_id(),
                    owner.host_installation_id()
                ],
            )
            .unwrap();
        }
    }

    fn grant(
        fixture: &Fixture,
        capability: &str,
        kind: ExactAuthorizableResourceKind,
        id: &str,
    ) -> Result<ExactAuthorizationView, AppError> {
        grant_for_owner(
            &fixture.db,
            &fixture.owner,
            &fixture.subject,
            &fixture.context,
            PLUGIN_ID,
            capability,
            kind,
            id,
            Some((Utc::now() + Duration::hours(1)).to_rfc3339()),
        )
    }

    #[test]
    fn grants_owned_credential_agent_and_workflow_with_distinct_exact_scopes() {
        let fixture = setup_fixture();
        seed_credential(&fixture, "shared-id", Some(&fixture.owner));
        seed_agent(&fixture, "agent-a", false, Some(&fixture.owner));
        seed_agent(&fixture, "workflow-a", true, Some(&fixture.owner));

        let credential = grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "shared-id",
        )
        .expect("credential grant");
        let agent = grant(
            &fixture,
            "agents.invoke",
            ExactAuthorizableResourceKind::ExternalAgent,
            "agent-a",
        )
        .expect("agent grant");
        let workflow = grant(
            &fixture,
            "agents.invoke",
            ExactAuthorizableResourceKind::Workflow,
            "workflow-a",
        )
        .expect("workflow grant");
        let network = grant(
            &fixture,
            "network.xingchen",
            ExactAuthorizableResourceKind::Workflow,
            "workflow-a",
        )
        .expect("network grant");

        for view in [&credential, &agent, &workflow, &network] {
            assert_eq!(
                view.status,
                CurrentPluginCapabilityAuthorizationStatus::Granted
            );
            assert!(view.effective);
            assert!(view.authorization_id.is_some());
        }
        let records = list_formal_plugin_capability_authorizations_for_actor(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            PLUGIN_ID,
        )
        .unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(
            records
                .iter()
                .map(|record| (&record.capability_id, &record.scope.kind, &record.scope.key))
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
        assert!(records
            .iter()
            .all(|record| { !format!("{record:?}").contains("secret-must-never-leak") }));
    }

    #[test]
    fn grant_is_idempotent_and_query_uses_the_same_guard_visible_record() {
        let fixture = setup_fixture();
        seed_credential(&fixture, "credential-a", Some(&fixture.owner));
        let first = grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a",
        )
        .unwrap();
        let second = grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a",
        )
        .unwrap();
        assert_eq!(first.authorization_id, second.authorization_id);

        let queried = query_for_owner(
            &fixture.db,
            &fixture.owner,
            &fixture.subject,
            &fixture.context,
            PLUGIN_ID,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a",
        )
        .unwrap();
        assert_eq!(queried.authorization_id, first.authorization_id);
        assert!(queried.effective);

        seed_credential(&fixture, "credential-b", Some(&fixture.owner));
        let missing = query_for_owner(
            &fixture.db,
            &fixture.owner,
            &fixture.subject,
            &fixture.context,
            PLUGIN_ID,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-b",
        )
        .expect("an unrelated exact grant must not alias this resource");
        assert_eq!(
            missing.status,
            CurrentPluginCapabilityAuthorizationStatus::Missing
        );
        assert!(missing.authorization_id.is_none());
    }

    #[test]
    fn resource_matrix_and_owner_boundaries_fail_closed() {
        let fixture = setup_fixture();
        let other_subject =
            ResourceOwner::fixture("subject-b", fixture.owner.host_installation_id());
        let other_installation =
            ResourceOwner::fixture(fixture.owner.platform_subject_id(), "other-installation");
        seed_credential(&fixture, "other-subject", Some(&other_subject));
        seed_credential(&fixture, "other-installation", Some(&other_installation));
        seed_credential(&fixture, "legacy", None);
        seed_agent(&fixture, "workflow-a", true, Some(&fixture.owner));

        for result in [
            grant(
                &fixture,
                "credentials.use",
                ExactAuthorizableResourceKind::Credential,
                "other-subject",
            ),
            grant(
                &fixture,
                "credentials.use",
                ExactAuthorizableResourceKind::Credential,
                "other-installation",
            ),
            grant(
                &fixture,
                "credentials.use",
                ExactAuthorizableResourceKind::Credential,
                "legacy",
            ),
            grant(
                &fixture,
                "agents.invoke",
                ExactAuthorizableResourceKind::Credential,
                "other-subject",
            ),
            grant(
                &fixture,
                "agents.invoke",
                ExactAuthorizableResourceKind::ExternalAgent,
                "workflow-a",
            ),
        ] {
            assert!(result.is_err());
        }
        assert!(ExactAuthorizableResourceKind::parse("agent-session").is_err());
        assert!(ExactAuthorizableResourceKind::parse("agent-message").is_err());
        assert!(ExactAuthorizableResourceKind::parse("agent-request").is_err());
        assert!(ExactAuthorizableResourceKind::parse("Credential").is_err());
    }

    #[test]
    fn revoke_by_scoped_handle_survives_resource_deletion_and_is_precise() {
        let fixture = setup_fixture();
        seed_credential(&fixture, "credential-a", Some(&fixture.owner));
        seed_credential(&fixture, "credential-b", Some(&fixture.owner));
        let first = grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a",
        )
        .unwrap();
        let second = grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-b",
        )
        .unwrap();
        fixture
            .db
            .conn_lock()
            .unwrap()
            .execute("DELETE FROM credentials WHERE id = 'credential-a'", [])
            .unwrap();

        let revoked = revoke_for_actor(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            PLUGIN_ID,
            first.authorization_id.as_deref().unwrap(),
        )
        .expect("revoke deleted resource authorization");
        assert_eq!(
            revoked.status,
            CurrentPluginCapabilityAuthorizationStatus::Revoked
        );
        let repeated = revoke_for_actor(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            PLUGIN_ID,
            first.authorization_id.as_deref().unwrap(),
        )
        .expect("idempotent revoke");
        assert_eq!(
            repeated.status,
            CurrentPluginCapabilityAuthorizationStatus::Revoked
        );

        let listed = list_for_actor(&fixture.db, &fixture.subject, &fixture.context, PLUGIN_ID)
            .expect("safe list");
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|item| item.available.is_none()));
        assert!(listed.iter().any(|item| {
            item.authorization_id == second.authorization_id
                && item.status == CurrentPluginCapabilityAuthorizationStatus::Granted
        }));

        let other_subject = PluginAuthorizationSubject {
            kind: PluginAuthorizationSubjectKind::PlatformUser,
            id: "subject-b".into(),
        };
        assert!(revoke_for_actor(
            &fixture.db,
            &other_subject,
            &fixture.context,
            PLUGIN_ID,
            second.authorization_id.as_deref().unwrap(),
        )
        .is_err());
        assert!(revoke_for_actor(
            &fixture.db,
            &fixture.subject,
            &fixture.context,
            "other-plugin",
            second.authorization_id.as_deref().unwrap(),
        )
        .is_err());
    }

    #[test]
    fn plugin_state_manifest_snapshot_integrity_and_backend_failures_reject_grant() {
        let fixture = setup_fixture();
        seed_credential(&fixture, "credential-a", Some(&fixture.owner));
        fixture.db.set_plugin_enabled(PLUGIN_ID, false).unwrap();
        assert!(grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a"
        )
        .is_err());
        fixture.db.set_plugin_enabled(PLUGIN_ID, true).unwrap();
        fs::write(fixture.directory.join("tampered.txt"), "tampered").unwrap();
        assert!(grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a"
        )
        .is_err());

        let fixture = setup_fixture();
        seed_credential(&fixture, "credential-a", Some(&fixture.owner));
        fixture
            .db
            .conn_lock()
            .unwrap()
            .execute(
                "UPDATE plugin_versions SET permissions_json = '[]'
                 WHERE plugin_id = ?1 AND is_current = 1",
                [PLUGIN_ID],
            )
            .unwrap();
        assert!(grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a"
        )
        .is_err());

        let fixture = setup_fixture();
        fixture
            .db
            .conn_lock()
            .unwrap()
            .execute("DROP TABLE credentials", [])
            .unwrap();
        let error = grant(
            &fixture,
            "credentials.use",
            ExactAuthorizableResourceKind::Credential,
            "credential-a",
        )
        .expect_err("backend failure must reject");
        assert_eq!(error.to_string(), "参数无效: 资源解析暂不可用");
        assert!(!error.to_string().contains("sqlite"));
    }

    #[test]
    fn handles_are_strict_and_do_not_expose_scope_or_resource_identity() {
        for value in [
            "1",
            "exact-auth-v1:",
            "exact-auth-v1:01z",
            "exact-auth-v1:2A",
            "EXACT-AUTH-V1:1",
            "exact-auth-v2:1",
        ] {
            assert!(decode_authorization_handle(value).is_err(), "{value}");
        }
        assert_eq!(decode_authorization_handle("exact-auth-v1:2a").unwrap(), 42);
    }

    #[test]
    fn exact_expiration_is_backend_bounded_and_canonical() {
        let now = Utc::now();
        assert!(validate_exact_authorization_expiration(None, now).is_err());
        assert!(validate_exact_authorization_expiration(Some("invalid".into()), now).is_err());
        assert!(validate_exact_authorization_expiration(
            Some((now - Duration::seconds(1)).to_rfc3339()),
            now,
        )
        .is_err());
        assert!(validate_exact_authorization_expiration(
            Some((now + Duration::hours(25)).to_rfc3339()),
            now,
        )
        .is_err());
        assert_eq!(
            validate_exact_authorization_expiration(
                Some((now + Duration::hours(1)).to_rfc3339()),
                now,
            )
            .unwrap(),
            Some((now + Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Secs, true)),
        );
    }

    #[test]
    fn catalog_only_returns_resolver_verified_safe_resource_summaries() {
        let fixture = setup_fixture();
        seed_credential(&fixture, "credential-owned", Some(&fixture.owner));
        seed_credential(&fixture, "credential-legacy", None);
        seed_agent(&fixture, "agent-owned", false, Some(&fixture.owner));
        seed_agent(&fixture, "workflow-owned", true, Some(&fixture.owner));

        let catalog = catalog_for_owner(&fixture.db, &fixture.owner, PLUGIN_ID).unwrap();
        assert_eq!(catalog.max_duration_hours, 24);
        assert_eq!(catalog.capability_ids.len(), 3);
        assert!(catalog.resources.iter().any(|resource| {
            resource.resource_kind == "credential"
                && resource.resource_id == "credential-owned"
                && resource.display_name == "safe label"
                && resource.compatible_capabilities == ["credentials.use"]
        }));
        assert!(catalog.resources.iter().any(|resource| {
            resource.resource_kind == "external-agent"
                && resource.resource_id == "agent-owned"
                && resource
                    .compatible_capabilities
                    .contains(&"agents.invoke".to_string())
                && resource
                    .compatible_capabilities
                    .contains(&"network.xingchen".to_string())
        }));
        assert!(catalog.resources.iter().any(|resource| {
            resource.resource_kind == "workflow" && resource.resource_id == "workflow-owned"
        }));
        assert!(!catalog
            .resources
            .iter()
            .any(|resource| resource.resource_id == "credential-legacy"));
        let debug = format!("{catalog:?}");
        assert!(!debug.contains("secret-must-never-leak"));
        assert!(!debug.contains(fixture.owner.platform_subject_id()));
        assert!(!debug.contains(fixture.owner.host_installation_id()));
    }

    #[test]
    fn catalog_propagates_backend_failure_instead_of_returning_an_empty_catalog() {
        let fixture = setup_fixture();
        fixture
            .db
            .conn_lock()
            .unwrap()
            .execute("DROP TABLE credentials", [])
            .unwrap();

        let error = catalog_for_owner(&fixture.db, &fixture.owner, PLUGIN_ID).unwrap_err();
        assert!(!error.to_string().contains("secret"));
        assert_ne!(error.to_string(), "参数无效: 资源不存在或不可访问");
    }
}
