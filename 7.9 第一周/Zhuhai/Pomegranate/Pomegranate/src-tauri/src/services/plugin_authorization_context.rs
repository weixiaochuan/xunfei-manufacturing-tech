use std::fmt;

use crate::account::{verified_platform_user_id, AccountState};
use crate::database::Database;
use crate::error::AppError;
use crate::models::plugin_platform::{
    PluginAuthorizationContext, PluginAuthorizationScope, PluginAuthorizationSubject,
    PluginAuthorizationSubjectKind,
};
use crate::services::hash::sha256_hex;
use crate::services::plugin_capabilities::{
    canonical_capability_semantic_version, VALID_PERMISSIONS,
};

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
    fn semantic_version_comes_from_canonical_registry() {
        assert_eq!(
            resolve_capability_semantic_version("ai.invoke").unwrap(),
            "1.0.0"
        );
        assert!(resolve_capability_semantic_version("unknown.capability").is_err());
    }
}
