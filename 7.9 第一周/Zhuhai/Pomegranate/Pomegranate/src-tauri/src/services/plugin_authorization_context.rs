use std::fmt;

use crate::account::{verified_platform_user_id, AccountState};
use crate::database::Database;
use crate::error::AppError;
use crate::models::plugin_platform::{
    PluginAuthorizationContext, PluginAuthorizationScope, PluginAuthorizationSubject,
    PluginAuthorizationSubjectKind, PluginManifestV3,
};
use crate::services::hash::sha256_hex;
use crate::services::plugin_capabilities::{
    canonical_capability_policy, canonical_capability_semantic_version, CanonicalScopeType,
    VALID_PERMISSIONS,
};
use crate::services::resource_resolution::agent_children::{
    TrustedAgentMessage, TrustedAgentRequest, TrustedAgentSession, TrustedWorkflow,
};
use crate::services::resource_resolution::credential::TrustedCredential;
use crate::services::resource_resolution::external_agent::{
    CallableExternalAgent, ExternalAgentRuntimeKind,
};
use crate::services::resource_resolution::TrustedResource;

const GLOBAL_SCOPE_KIND: &str = "global";
const GLOBAL_SCOPE_KEY: &str = "v1:*";
const SCOPE_KEY_VERSION: &str = "v1";
const SCOPE_HASH_LENGTH: usize = 64;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum TrustedResourceScope {
    Global,
    Feature(FeatureScope),
    BoundAgent(BoundAgentScope),
    BoundCredential(BoundCredentialScope),
    XingchenService(XingchenServiceScope),
    ExactResource(ExactResourceScope),
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct FeatureScope {
    plugin_id: String,
    feature_id: String,
    contribution_fingerprint: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct BoundAgentScope {
    external_agent_id: String,
    configuration_fingerprint: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct BoundCredentialScope {
    credential_id: String,
    provider: String,
    target_service: String,
    binding_fingerprint: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct XingchenServiceScope {
    external_agent_id: String,
    workflow_id: String,
    configuration_fingerprint: String,
}

#[derive(Clone, PartialEq, Eq)]
struct ExactResourceScope {
    scope_type: CanonicalScopeType,
    capability_id: String,
    resource_kind: &'static str,
    resource_id: String,
    platform_subject_id: String,
    host_installation_id: String,
}

impl fmt::Debug for TrustedResourceScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedResourceScope")
            .field("scope_type", &self.scope_type())
            .finish_non_exhaustive()
    }
}

impl TrustedResourceScope {
    pub(super) fn global() -> Self {
        Self::Global
    }

    /// 仅从当前可信 Manifest 中重新定位声明式 enhancement，并据此构造 feature scope。
    ///
    /// 调用方只能提交 contribution ID；scene、feature、hook、handler 与插件版本均进入
    /// 后端计算的指纹，避免 WebView 通过拼接 scope 扩大授权范围。
    pub(super) fn for_declarative_enhancement(
        manifest: &PluginManifestV3,
        contribution_id: &str,
    ) -> Result<Self, AppError> {
        let policy = canonical_capability_policy("ai.context.augment")
            .map_err(|_| AppError::PluginAuthorizationScopeInvalid {
                reason: "registry_scope_policy_invalid",
            })?
            .ok_or(AppError::PluginAuthorizationCapabilityInvalid {
                reason: "capability_not_admitted",
            })?;
        if !matches!(policy.status.as_str(), "active" | "restricted")
            || policy.scope_type != CanonicalScopeType::Feature
            || policy.scope_schema_version != 1
        {
            return Err(AppError::PluginAuthorizationScopeMismatch);
        }
        if !manifest
            .permissions
            .iter()
            .any(|permission| permission == "ai.context.augment")
        {
            return Err(AppError::PluginAuthorizationManifestNotDeclared);
        }
        let contribution = manifest
            .contributes
            .enhancements
            .iter()
            .find(|contribution| contribution.id == contribution_id)
            .ok_or_else(|| AppError::InvalidInput("声明式贡献不存在或不可访问".into()))?;
        let canonical_contribution = serde_json::to_string(&(
            "declarative-context-augment-v1",
            &manifest.version,
            contribution,
        ))?;
        Ok(Self::Feature(FeatureScope {
            plugin_id: manifest.id.clone(),
            feature_id: contribution.id.clone(),
            contribution_fingerprint: sha256_hex(&canonical_contribution),
        }))
    }

    pub(super) fn for_credential(
        capability_id: &str,
        credential: &TrustedCredential,
    ) -> Result<Self, AppError> {
        Self::exact_resource_scope(
            capability_id,
            CanonicalScopeType::BoundCredential,
            credential.resource(),
            "credential",
        )
    }

    pub(super) fn for_external_agent(
        capability_id: &str,
        agent: &CallableExternalAgent,
    ) -> Result<Self, AppError> {
        Self::for_callable_agent(capability_id, agent)
    }

    pub(super) fn for_workflow(
        capability_id: &str,
        workflow: &TrustedWorkflow,
    ) -> Result<Self, AppError> {
        if workflow.agent().runtime_kind() != ExternalAgentRuntimeKind::Workflow {
            return Err(AppError::PluginAuthorizationScopeInvalid {
                reason: "trusted_workflow_runtime_mismatch",
            });
        }
        Self::for_callable_agent(capability_id, workflow.agent())
    }

    pub(super) fn for_agent_session(
        capability_id: &str,
        session: &TrustedAgentSession,
    ) -> Result<Self, AppError> {
        Self::for_callable_agent(capability_id, session.agent())
    }

    pub(super) fn for_agent_message(
        capability_id: &str,
        message: &TrustedAgentMessage,
    ) -> Result<Self, AppError> {
        Self::for_callable_agent(capability_id, message.agent())
    }

    pub(super) fn for_agent_request(
        capability_id: &str,
        request: &TrustedAgentRequest,
    ) -> Result<Self, AppError> {
        Self::for_callable_agent(capability_id, request.agent())
    }

    fn for_callable_agent(
        capability_id: &str,
        agent: &CallableExternalAgent,
    ) -> Result<Self, AppError> {
        let expected_scope_type = match capability_id {
            "agents.invoke" => CanonicalScopeType::BoundAgent,
            "network.xingchen" => CanonicalScopeType::XingchenService,
            _ => {
                return Err(AppError::PluginAuthorizationScopeMismatch);
            }
        };
        let resource_kind = match agent.runtime_kind() {
            ExternalAgentRuntimeKind::Agent => "external-agent",
            ExternalAgentRuntimeKind::Workflow => "workflow",
        };
        Self::exact_resource_scope(
            capability_id,
            expected_scope_type,
            agent.resource(),
            resource_kind,
        )
    }

    fn exact_resource_scope(
        capability_id: &str,
        expected_scope_type: CanonicalScopeType,
        resource: &TrustedResource,
        resource_kind: &'static str,
    ) -> Result<Self, AppError> {
        let policy = canonical_capability_policy(capability_id)
            .map_err(|_| AppError::PluginAuthorizationScopeInvalid {
                reason: "registry_scope_policy_invalid",
            })?
            .ok_or(AppError::PluginAuthorizationCapabilityInvalid {
                reason: "capability_not_admitted",
            })?;
        if !matches!(policy.status.as_str(), "active" | "restricted")
            || policy.scope_type != expected_scope_type
            || policy.scope_schema_version != 1
        {
            return Err(AppError::PluginAuthorizationScopeMismatch);
        }
        Ok(Self::ExactResource(ExactResourceScope {
            scope_type: expected_scope_type,
            capability_id: capability_id.to_string(),
            resource_kind,
            resource_id: resource.resource_id().to_string(),
            platform_subject_id: resource.owner().platform_subject_id().to_string(),
            host_installation_id: resource.owner().host_installation_id().to_string(),
        }))
    }

    #[cfg(test)]
    pub(super) fn feature(
        plugin_id: impl Into<String>,
        feature_id: impl Into<String>,
        contribution_fingerprint: impl Into<String>,
    ) -> Self {
        Self::Feature(FeatureScope {
            plugin_id: plugin_id.into(),
            feature_id: feature_id.into(),
            contribution_fingerprint: contribution_fingerprint.into(),
        })
    }

    #[cfg(test)]
    pub(super) fn bound_agent(
        external_agent_id: impl Into<String>,
        configuration_fingerprint: impl Into<String>,
    ) -> Self {
        Self::BoundAgent(BoundAgentScope {
            external_agent_id: external_agent_id.into(),
            configuration_fingerprint: configuration_fingerprint.into(),
        })
    }

    #[cfg(test)]
    pub(super) fn bound_credential(
        credential_id: impl Into<String>,
        provider: impl Into<String>,
        target_service: impl Into<String>,
        binding_fingerprint: impl Into<String>,
    ) -> Self {
        Self::BoundCredential(BoundCredentialScope {
            credential_id: credential_id.into(),
            provider: provider.into(),
            target_service: target_service.into(),
            binding_fingerprint: binding_fingerprint.into(),
        })
    }

    #[cfg(test)]
    pub(super) fn xingchen_service(
        external_agent_id: impl Into<String>,
        workflow_id: impl Into<String>,
        configuration_fingerprint: impl Into<String>,
    ) -> Self {
        Self::XingchenService(XingchenServiceScope {
            external_agent_id: external_agent_id.into(),
            workflow_id: workflow_id.into(),
            configuration_fingerprint: configuration_fingerprint.into(),
        })
    }

    pub(super) fn scope_type(&self) -> &'static str {
        match self {
            Self::Global => GLOBAL_SCOPE_KIND,
            Self::Feature(_) => "feature",
            Self::BoundAgent(_) => "bound-agent",
            Self::BoundCredential(_) => "bound-credential",
            Self::XingchenService(_) => "xingchen-service",
            Self::ExactResource(scope) => scope.scope_type.as_str(),
        }
    }

    pub(super) fn canonical_scope(&self) -> Result<PluginAuthorizationScope, AppError> {
        if matches!(self, Self::Global) {
            return canonicalize_authorization_scope(GLOBAL_SCOPE_KIND, GLOBAL_SCOPE_KEY);
        }
        let fields: Vec<&str> = match self {
            Self::Global => unreachable!("global scope handled above"),
            Self::Feature(scope) => vec![
                "feature-invoke",
                &scope.plugin_id,
                &scope.feature_id,
                &scope.contribution_fingerprint,
            ],
            Self::BoundAgent(scope) => vec![
                "agent-invoke",
                &scope.external_agent_id,
                &scope.configuration_fingerprint,
            ],
            Self::BoundCredential(scope) => vec![
                "credential-use",
                &scope.credential_id,
                &scope.provider,
                &scope.target_service,
                &scope.binding_fingerprint,
            ],
            Self::XingchenService(scope) => vec![
                "xingchen-request",
                "xingchen-workflow-v1",
                &scope.external_agent_id,
                &scope.workflow_id,
                &scope.configuration_fingerprint,
            ],
            Self::ExactResource(scope) => vec![
                "exact-resource-v1",
                &scope.capability_id,
                scope.resource_kind,
                &scope.resource_id,
                &scope.platform_subject_id,
                &scope.host_installation_id,
            ],
        };
        if fields.iter().any(|field| field.trim().is_empty()) {
            return Err(AppError::PluginAuthorizationScopeInvalid {
                reason: "trusted_scope_field_empty",
            });
        }
        let material = canonical_fingerprint_material(self.scope_type(), &fields);
        canonicalize_authorization_scope(
            self.scope_type(),
            &format!("{SCOPE_KEY_VERSION}:{}", sha256_hex(&material)),
        )
    }
}

fn canonical_fingerprint_material(scope_type: &str, fields: &[&str]) -> String {
    let mut material = format!("scope-v1|{}:{}", scope_type.len(), scope_type);
    for field in fields {
        material.push('|');
        material.push_str(&field.len().to_string());
        material.push(':');
        material.push_str(field);
    }
    material
}

pub(super) fn scope_audit_summary(scope: &PluginAuthorizationScope) -> String {
    let digest = sha256_hex(&format!("{}\0{}", scope.kind, scope.key));
    digest[..16].to_string()
}

/// Produce stable audit evidence without serializing raw resource identities.
///
/// Invalid trusted scopes are already denied by the Guard. Their audit evidence is derived only
/// from the public scope type and a stable failure class so malformed raw fields cannot leak.
pub(super) fn trusted_scope_audit_summary(scope: &TrustedResourceScope) -> String {
    match scope.canonical_scope() {
        Ok(canonical) => scope_audit_summary(&canonical),
        Err(_) => {
            let digest = sha256_hex(&format!(
                "invalid-scope-v1\0{}\0scope_canonicalization_failed",
                scope.scope_type()
            ));
            digest[..16].to_string()
        }
    }
}

/// Resolve an account identity that has already been verified by the Rust backend.
pub(crate) async fn resolve_verified_platform_subject(
    account: &AccountState,
) -> Result<PluginAuthorizationSubject, AppError> {
    let id = verified_platform_user_id(account).await?;
    Ok(PluginAuthorizationSubject {
        kind: PluginAuthorizationSubjectKind::PlatformUser,
        id,
    })
}

/// Resolve the stable host installation identity persisted by the Rust backend.
pub(crate) fn resolve_host_installation_context(
    db: &Database,
) -> Result<PluginAuthorizationContext, AppError> {
    db.stable_host_installation_context()
}

/// Validate the versioned database representation of a trusted resource scope.
pub(crate) fn canonicalize_authorization_scope(
    kind: &str,
    key: &str,
) -> Result<PluginAuthorizationScope, AppError> {
    if kind == GLOBAL_SCOPE_KIND {
        if key != GLOBAL_SCOPE_KEY {
            return Err(AppError::PluginAuthorizationScopeInvalid {
                reason: "scope_key_not_canonical",
            });
        }
    } else {
        if !matches!(
            kind,
            "feature" | "bound-agent" | "bound-credential" | "xingchen-service"
        ) {
            return Err(AppError::PluginAuthorizationScopeInvalid {
                reason: "scope_kind_not_supported",
            });
        }
        let Some(digest) = key.strip_prefix("v1:") else {
            return Err(AppError::PluginAuthorizationScopeInvalid {
                reason: "scope_key_version_unsupported",
            });
        };
        if digest.len() != SCOPE_HASH_LENGTH
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(AppError::PluginAuthorizationScopeInvalid {
                reason: "scope_key_not_canonical",
            });
        }
    }
    Ok(PluginAuthorizationScope {
        kind: kind.to_string(),
        key: key.to_string(),
    })
}

/// Resolve semantic version exclusively from the A1 canonical registry.
pub(crate) fn resolve_capability_semantic_version(capability_id: &str) -> Result<String, AppError> {
    if !VALID_PERMISSIONS.contains(&capability_id) {
        return Err(AppError::PluginAuthorizationCapabilityInvalid {
            reason: "capability_not_admitted",
        });
    }
    canonical_capability_semantic_version(capability_id)
        .map_err(|_| AppError::PluginAuthorizationCapabilitySemanticVersionUnavailable)?
        .filter(|version| !version.trim().is_empty())
        .ok_or(AppError::PluginAuthorizationCapabilitySemanticVersionUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    use crate::services::resource_ownership::ResourceOwner;
    use crate::services::resource_resolution::agent_children::{
        resolve_agent_message, resolve_agent_request, resolve_agent_session, resolve_workflow,
    };
    use crate::services::resource_resolution::credential::resolve_credential;
    use crate::services::resource_resolution::external_agent::resolve_external_agent;
    use crate::services::resource_resolution::UntrustedResourceRef;

    fn db() -> Database {
        Database::init(":memory:").expect("in-memory database")
    }

    fn owner(subject: &str, installation: &str) -> ResourceOwner {
        ResourceOwner::fixture(subject, installation)
    }

    fn reference(kind: &str, id: &str) -> UntrustedResourceRef {
        UntrustedResourceRef::try_new(kind, id).unwrap()
    }

    fn seed_credential(db: &Database, id: &str, owner: Option<&ResourceOwner>) {
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO credentials
                (id, provider, credential_type, label, owner_scope, secret_reference, configured)
             VALUES (?1, 'provider', 'api_key', 'label', 'deprecated', 'secret-ref', 1)",
            params![id],
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

    fn seed_agent_graph(
        db: &Database,
        id: &str,
        runtime_kind: &str,
        owner: Option<&ResourceOwner>,
    ) {
        let conn = db.conn_lock().unwrap();
        let product_id = format!("product-{runtime_kind}");
        let plugin_id = format!("plugin-{runtime_kind}");
        conn.execute(
            "INSERT INTO plugins (id, name, version, path, main, manifest_json, enabled, status)
             VALUES (?1, ?2, '1.0.0', '/tmp/mock', 'main.js', '{}', 1, 'installed')",
            params![plugin_id, runtime_kind],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO products
                (id, developer_id, name, product_type, status, plugin_id,
                 developer_name, runtime_kind, review_status)
             VALUES (?1, 'dev', ?1, ?2, 'published', ?3, 'dev', ?2, 'approved')",
            params![product_id, runtime_kind, plugin_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO product_versions
                (product_id, version, manifest_json, runtime_kind, source, content_hash,
                 signature_status, status, review_status)
             VALUES (?1, '1.0.0', '{}', ?2, 'marketplace', 'hash',
                     'unsigned', 'active', 'approved')",
            params![product_id, runtime_kind],
        )
        .unwrap();
        let version_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO plugin_installations
                (plugin_id, product_id, product_version_id, installed_version, source,
                 enabled, install_path, content_hash, status)
             VALUES (?1, ?2, ?3, '1.0.0', 'marketplace', 1,
                     '/tmp/mock', 'hash', 'installed')",
            params![plugin_id, product_id, version_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO external_agents
                (id, product_id, provider, name, endpoint, authentication_type,
                 streaming_type, request_mapping_json, response_mapping_json,
                 session_mapping_json, error_mapping_json, mock_mode, enabled)
             VALUES (?1, ?2, 'xingchen', 'agent', 'mock://agent', 'none',
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

    fn seed_children(db: &Database, agent_id: &str, resource_id: &str) {
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO agent_sessions (id, external_agent_id, title)
             VALUES (?1, ?2, 'session')",
            params![resource_id, agent_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_messages (id, session_id, role, content, request_id)
             VALUES (?1, ?1, 'user', 'content', ?1)",
            params![resource_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_events (external_agent_id, session_id, request_id, status)
             VALUES (?1, ?2, ?2, 'completed')",
            params![agent_id, resource_id],
        )
        .unwrap();
    }

    #[test]
    fn trusted_scopes_are_stable_versioned_and_isolated() {
        let feature = TrustedResourceScope::feature("plugin-a", "feature-a", "manifest-hash");
        let feature_repeat =
            TrustedResourceScope::feature("plugin-a", "feature-a", "manifest-hash");
        let agent = TrustedResourceScope::bound_agent("agent-a", "agent-config-a");
        let credential = TrustedResourceScope::bound_credential(
            "credential-a",
            "xingchen",
            "xingchen-workflow-v1",
            "binding-a",
        );
        let service =
            TrustedResourceScope::xingchen_service("agent-a", "workflow-a", "service-config-a");
        assert_eq!(
            feature.canonical_scope().unwrap(),
            feature_repeat.canonical_scope().unwrap()
        );
        let scopes =
            [feature, agent, credential, service].map(|scope| scope.canonical_scope().unwrap());
        assert_eq!(
            scopes.clone().map(|scope| scope.kind),
            [
                "feature",
                "bound-agent",
                "bound-credential",
                "xingchen-service",
            ]
        );
        assert_eq!(
            scopes
                .map(|scope| scope.key)
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            4
        );
    }

    #[test]
    fn binding_changes_change_canonical_scope() {
        let cases = [
            (
                TrustedResourceScope::bound_agent("agent", "config-a"),
                TrustedResourceScope::bound_agent("agent", "config-b"),
            ),
            (
                TrustedResourceScope::bound_credential("credential", "provider", "target", "a"),
                TrustedResourceScope::bound_credential("credential", "provider", "target", "b"),
            ),
            (
                TrustedResourceScope::xingchen_service("agent", "workflow-a", "config"),
                TrustedResourceScope::xingchen_service("agent", "workflow-b", "config"),
            ),
        ];
        for (left, right) in cases {
            assert_ne!(
                left.canonical_scope().unwrap(),
                right.canonical_scope().unwrap()
            );
        }
    }

    #[test]
    fn canonical_scope_rejects_unknown_malformed_and_unsupported_versions() {
        for (kind, key) in [
            (
                "unknown",
                "v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            (
                "feature",
                "v2:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            ("feature", "v1:short"),
            (
                "feature",
                "v1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            ),
            ("global", "v1:anything"),
        ] {
            assert!(canonicalize_authorization_scope(kind, key).is_err());
        }
    }

    #[test]
    fn credential_identity_is_not_exposed_by_key_debug_or_audit() {
        let secret_marker = "credential-sensitive-marker";
        let scope = TrustedResourceScope::bound_credential(
            secret_marker,
            "xingchen",
            "xingchen-workflow-v1",
            "binding-sensitive-marker",
        );
        let canonical = scope.canonical_scope().unwrap();
        let rendered = format!("{scope:?}");
        let audit = scope_audit_summary(&canonical);
        for output in [canonical.key.as_str(), rendered.as_str(), audit.as_str()] {
            assert!(!output.contains(secret_marker));
            assert!(!output.contains("binding-sensitive-marker"));
        }
    }

    #[test]
    fn invalid_scope_audit_summary_is_stable_non_empty_and_redacted() {
        let raw_marker = "credential-sensitive-marker";
        let invalid = TrustedResourceScope::bound_credential(
            raw_marker,
            "",
            "target-sensitive-marker",
            "binding-sensitive-marker",
        );
        assert!(invalid.canonical_scope().is_err());

        let first = trusted_scope_audit_summary(&invalid);
        let second = trusted_scope_audit_summary(&invalid);
        assert_eq!(first, second);
        assert_eq!(first.len(), 16);
        for sensitive in [
            raw_marker,
            "target-sensitive-marker",
            "binding-sensitive-marker",
            "scope_field_empty",
        ] {
            assert!(!first.contains(sensitive));
        }
    }

    #[test]
    fn trusted_resolvers_produce_deterministic_exact_scopes_for_all_resource_classes() {
        let db = db();
        let owner = owner("subject-a", "installation-a");
        seed_credential(&db, "credential-a", Some(&owner));
        seed_agent_graph(&db, "workflow-a", "xingchen-workflow", Some(&owner));
        seed_children(&db, "workflow-a", "child-a");

        let credential =
            resolve_credential(&db, &owner, reference("credential", "credential-a")).unwrap();
        let agent =
            resolve_external_agent(&db, &owner, reference("external-agent", "workflow-a")).unwrap();
        let workflow = resolve_workflow(&db, &owner, reference("workflow", "workflow-a")).unwrap();
        let session =
            resolve_agent_session(&db, &owner, reference("agent-session", "child-a")).unwrap();
        let message =
            resolve_agent_message(&db, &owner, reference("agent-message", "child-a")).unwrap();
        let request =
            resolve_agent_request(&db, &owner, reference("agent-request", "child-a")).unwrap();

        let credential_scope =
            TrustedResourceScope::for_credential("credentials.use", &credential).unwrap();
        let agent_scope =
            TrustedResourceScope::for_external_agent("agents.invoke", &agent).unwrap();
        let workflow_scope =
            TrustedResourceScope::for_workflow("agents.invoke", &workflow).unwrap();
        let session_scope =
            TrustedResourceScope::for_agent_session("agents.invoke", &session).unwrap();
        let message_scope =
            TrustedResourceScope::for_agent_message("agents.invoke", &message).unwrap();
        let request_scope =
            TrustedResourceScope::for_agent_request("agents.invoke", &request).unwrap();

        assert_eq!(credential_scope.scope_type(), "bound-credential");
        for scope in [
            &agent_scope,
            &workflow_scope,
            &session_scope,
            &message_scope,
            &request_scope,
        ] {
            assert_eq!(scope.scope_type(), "bound-agent");
        }
        assert_eq!(
            agent_scope.canonical_scope().unwrap(),
            workflow_scope.canonical_scope().unwrap()
        );
        assert_eq!(
            agent_scope.canonical_scope().unwrap(),
            session_scope.canonical_scope().unwrap()
        );
        assert_eq!(
            agent_scope.canonical_scope().unwrap(),
            message_scope.canonical_scope().unwrap()
        );
        assert_eq!(
            agent_scope.canonical_scope().unwrap(),
            request_scope.canonical_scope().unwrap()
        );
        assert_eq!(
            session_scope.canonical_scope().unwrap(),
            TrustedResourceScope::for_agent_session("agents.invoke", &session)
                .unwrap()
                .canonical_scope()
                .unwrap()
        );

        for scope in [credential_scope, agent_scope, workflow_scope] {
            let encoded = scope.canonical_scope().unwrap();
            let parsed = canonicalize_authorization_scope(&encoded.kind, &encoded.key).unwrap();
            assert_eq!(parsed, encoded);
            assert_eq!(
                canonicalize_authorization_scope(&parsed.kind, &parsed.key).unwrap(),
                encoded
            );
        }
    }

    #[test]
    fn exact_scopes_isolate_runtime_type_actor_installation_capability_and_special_ids() {
        fn resolved_agent_scope(
            runtime_kind: &str,
            subject: &str,
            installation: &str,
            id: &str,
            capability: &str,
        ) -> PluginAuthorizationScope {
            let db = db();
            let owner = owner(subject, installation);
            seed_agent_graph(&db, id, runtime_kind, Some(&owner));
            let agent =
                resolve_external_agent(&db, &owner, reference("external-agent", id)).unwrap();
            TrustedResourceScope::for_external_agent(capability, &agent)
                .unwrap()
                .canonical_scope()
                .unwrap()
        }

        let id = "same:/% id|字段";
        let agent = resolved_agent_scope(
            "xingchen-agent",
            "subject-a",
            "installation-a",
            id,
            "agents.invoke",
        );
        let workflow = resolved_agent_scope(
            "xingchen-workflow",
            "subject-a",
            "installation-a",
            id,
            "agents.invoke",
        );
        let other_subject = resolved_agent_scope(
            "xingchen-agent",
            "subject-b",
            "installation-a",
            id,
            "agents.invoke",
        );
        let other_installation = resolved_agent_scope(
            "xingchen-agent",
            "subject-a",
            "installation-b",
            id,
            "agents.invoke",
        );
        let service = resolved_agent_scope(
            "xingchen-agent",
            "subject-a",
            "installation-a",
            id,
            "network.xingchen",
        );

        for other in [workflow, other_subject, other_installation, service] {
            assert_ne!(agent, other);
        }
        assert_eq!(agent.key.len(), 67);
        assert!(!agent.key.contains(id));
        assert!(!agent.key.contains("subject-a"));
        assert!(!agent.key.contains("installation-a"));
    }

    #[test]
    fn child_scope_uses_authoritative_parent_not_the_child_id_or_caller_input() {
        let db = db();
        let owner = owner("subject-a", "installation-a");
        seed_agent_graph(&db, "parent-agent", "xingchen-agent", Some(&owner));
        seed_children(&db, "parent-agent", "same-id");

        let parent =
            resolve_external_agent(&db, &owner, reference("external-agent", "parent-agent"))
                .unwrap();
        let session =
            resolve_agent_session(&db, &owner, reference("agent-session", "same-id")).unwrap();
        let message =
            resolve_agent_message(&db, &owner, reference("agent-message", "same-id")).unwrap();
        let request =
            resolve_agent_request(&db, &owner, reference("agent-request", "same-id")).unwrap();

        let expected = TrustedResourceScope::for_external_agent("agents.invoke", &parent)
            .unwrap()
            .canonical_scope()
            .unwrap();
        for actual in [
            TrustedResourceScope::for_agent_session("agents.invoke", &session),
            TrustedResourceScope::for_agent_message("agents.invoke", &message),
            TrustedResourceScope::for_agent_request("agents.invoke", &request),
        ] {
            assert_eq!(actual.unwrap().canonical_scope().unwrap(), expected);
        }
    }

    #[test]
    fn capability_mismatch_unresolved_ownership_and_backend_failure_never_create_scope() {
        let owned_db = db();
        let owner = owner("subject-a", "installation-a");
        seed_credential(&owned_db, "credential-a", Some(&owner));
        let credential =
            resolve_credential(&owned_db, &owner, reference("credential", "credential-a")).unwrap();
        assert!(TrustedResourceScope::for_credential("agents.invoke", &credential).is_err());

        let legacy_db = db();
        seed_credential(&legacy_db, "legacy", None);
        assert!(resolve_credential(&legacy_db, &owner, reference("credential", "legacy")).is_err());

        let failed_db = db();
        failed_db
            .conn_lock()
            .unwrap()
            .execute("DROP TABLE credentials", [])
            .unwrap();
        assert!(
            resolve_credential(&failed_db, &owner, reference("credential", "credential-a"))
                .is_err()
        );
    }

    #[test]
    fn exact_comparison_rejects_prefix_substring_truncation_and_noncanonical_forms() {
        let canonical = canonicalize_authorization_scope(
            "bound-agent",
            "v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        for rejected in [
            "v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-extra",
            "V1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "v1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(canonicalize_authorization_scope("bound-agent", rejected).is_err());
            assert_ne!(canonical.key, rejected);
        }
        assert!(canonicalize_authorization_scope("bound-agent-extra", &canonical.key).is_err());
    }

    #[test]
    fn semantic_version_comes_from_canonical_registry() {
        assert_eq!(
            resolve_capability_semantic_version("ai.invoke").unwrap(),
            "1.0.0"
        );
        assert!(resolve_capability_semantic_version("unknown.capability").is_err());
    }
}
