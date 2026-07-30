//! Canonical plugin capability registry bridge.

pub(crate) use super::plugin_capabilities_generated::{
    V3_CLASSIFICATION_CONTRIBUTION_RULES, V3_CONTRIBUTION_REQUIRED_PERMISSIONS,
    V3_FEATURE_CAPABILITY_REQUIRED_PERMISSIONS, V3_MANIFEST_PERMISSIONS,
    V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS, V3_PERMISSION_RUNTIME_KINDS,
    V3_RUNTIME_CLASSIFICATION_RULES, V3_RUNTIME_CONTRIBUTION_REQUIRED_PERMISSIONS,
    VALID_PERMISSIONS,
};

pub(crate) fn is_v3_permission_runtime_allowed(permission: &str, runtime_kind: &str) -> bool {
    if V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS.contains(&permission) {
        return true;
    }
    V3_PERMISSION_RUNTIME_KINDS
        .iter()
        .find(|(id, _)| *id == permission)
        .is_some_and(|(_, runtime_kinds)| runtime_kinds.contains(&runtime_kind))
}

#[cfg(test)]
pub(crate) fn is_v3_classification_contribution_allowed(
    classification: &str,
    contributions: &[&str],
) -> bool {
    evaluate_v3_classification_contributions(classification, contributions).is_ok()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum V3ClassificationContributionViolation<'a> {
    UnknownClassification {
        classification: &'a str,
    },
    MissingRequiredContribution {
        classification: &'a str,
        contribution: &'static str,
    },
    ForbiddenContribution {
        classification: &'a str,
        contribution: &'static str,
    },
}

pub(crate) fn evaluate_v3_classification_contributions<'a>(
    classification: &'a str,
    contributions: &[&str],
) -> Result<(), V3ClassificationContributionViolation<'a>> {
    let Some((_, required, forbidden)) = V3_CLASSIFICATION_CONTRIBUTION_RULES
        .iter()
        .find(|(value, _, _)| *value == classification)
    else {
        return Err(
            V3ClassificationContributionViolation::UnknownClassification { classification },
        );
    };
    if let Some(contribution) = required.iter().find(|item| !contributions.contains(item)) {
        return Err(
            V3ClassificationContributionViolation::MissingRequiredContribution {
                classification,
                contribution,
            },
        );
    }
    if let Some(contribution) = forbidden.iter().find(|item| contributions.contains(item)) {
        return Err(
            V3ClassificationContributionViolation::ForbiddenContribution {
                classification,
                contribution,
            },
        );
    }
    Ok(())
}

pub(crate) fn is_v3_runtime_classification_allowed(
    runtime_kind: &str,
    classification: &str,
) -> bool {
    V3_RUNTIME_CLASSIFICATION_RULES
        .iter()
        .find(|(value, _)| *value == runtime_kind)
        .is_some_and(|(_, classifications)| classifications.contains(&classification))
}

#[cfg(test)]
pub(crate) fn required_v3_policy_permissions(
    runtime_kind: &str,
    contributions: &[&str],
    feature_capabilities: &[&str],
) -> Vec<&'static str> {
    let mut required = Vec::new();
    for (contribution, permissions) in V3_CONTRIBUTION_REQUIRED_PERMISSIONS {
        if contributions.contains(contribution) {
            for permission in *permissions {
                if !required.contains(permission) {
                    required.push(*permission);
                }
            }
        }
    }
    for (runtime_kinds, contribution, permissions) in V3_RUNTIME_CONTRIBUTION_REQUIRED_PERMISSIONS {
        if runtime_kinds.contains(&runtime_kind) && contributions.contains(contribution) {
            for permission in *permissions {
                if !required.contains(permission) {
                    required.push(*permission);
                }
            }
        }
    }
    for (feature_capability, permissions) in V3_FEATURE_CAPABILITY_REQUIRED_PERMISSIONS {
        if feature_capabilities.contains(feature_capability) {
            for permission in *permissions {
                if !required.contains(permission) {
                    required.push(*permission);
                }
            }
        }
    }
    required
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum V3RequiredPermissionViolation<'a> {
    Contribution {
        contribution: &'static str,
        permission: &'static str,
    },
    RuntimeContribution {
        runtime_kind: &'a str,
        contribution: &'static str,
        permission: &'static str,
    },
    FeatureCapability {
        feature_capability: &'static str,
        permission: &'static str,
    },
}

pub(crate) fn evaluate_v3_required_permissions<'a>(
    runtime_kind: &'a str,
    contributions: &[&str],
    feature_capabilities: &[&str],
    permissions: &[&str],
) -> Result<(), V3RequiredPermissionViolation<'a>> {
    for (contribution, required) in V3_CONTRIBUTION_REQUIRED_PERMISSIONS {
        if !contributions.contains(contribution) {
            continue;
        }
        if let Some(permission) = required.iter().find(|item| !permissions.contains(item)) {
            return Err(V3RequiredPermissionViolation::Contribution {
                contribution,
                permission,
            });
        }
    }
    for (runtime_kinds, contribution, required) in V3_RUNTIME_CONTRIBUTION_REQUIRED_PERMISSIONS {
        if !runtime_kinds.contains(&runtime_kind) || !contributions.contains(contribution) {
            continue;
        }
        if let Some(permission) = required.iter().find(|item| !permissions.contains(item)) {
            return Err(V3RequiredPermissionViolation::RuntimeContribution {
                runtime_kind,
                contribution,
                permission,
            });
        }
    }
    for (feature_capability, required) in V3_FEATURE_CAPABILITY_REQUIRED_PERMISSIONS {
        if !feature_capabilities.contains(feature_capability) {
            continue;
        }
        if let Some(permission) = required.iter().find(|item| !permissions.contains(item)) {
            return Err(V3RequiredPermissionViolation::FeatureCapability {
                feature_capability,
                permission,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        is_v3_classification_contribution_allowed, is_v3_permission_runtime_allowed,
        is_v3_runtime_classification_allowed, required_v3_policy_permissions,
        V3_CLASSIFICATION_CONTRIBUTION_RULES, V3_CONTRIBUTION_REQUIRED_PERMISSIONS,
        V3_FEATURE_CAPABILITY_REQUIRED_PERMISSIONS, V3_MANIFEST_PERMISSIONS,
        V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS, V3_PERMISSION_RUNTIME_KINDS,
        V3_RUNTIME_CLASSIFICATION_RULES, V3_RUNTIME_CONTRIBUTION_REQUIRED_PERMISSIONS,
        VALID_PERMISSIONS,
    };
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[test]
    fn generated_permissions_match_canonical_registry() {
        let registry: Value =
            serde_json::from_str(include_str!("../../../config/plugin-capabilities.v1.json"))
                .expect("capability registry must be valid JSON");
        let capabilities = registry["capabilities"]
            .as_array()
            .expect("capabilities must be an array");
        let all = capabilities
            .iter()
            .map(|item| item["id"].as_str().expect("id must be a string"))
            .collect::<BTreeSet<_>>();
        let v3 = capabilities
            .iter()
            .filter(|item| matches!(item["status"].as_str(), Some("active" | "restricted")))
            .map(|item| item["id"].as_str().expect("id must be a string"))
            .collect::<BTreeSet<_>>();
        assert_eq!(all, VALID_PERMISSIONS.iter().copied().collect());
        assert_eq!(v3, V3_MANIFEST_PERMISSIONS.iter().copied().collect());
        assert_eq!(all.len(), 42);
        assert_eq!(v3.len(), 20);
        let runtime_map = capabilities
            .iter()
            .filter(|item| matches!(item["status"].as_str(), Some("active" | "restricted")))
            .map(|item| {
                (
                    item["id"].as_str().expect("id must be a string"),
                    item["runtimeKinds"]
                        .as_array()
                        .expect("runtimeKinds must be an array")
                        .iter()
                        .map(|kind| kind.as_str().expect("runtime kind must be a string"))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            runtime_map,
            V3_PERMISSION_RUNTIME_KINDS
                .iter()
                .map(|(id, kinds)| (*id, kinds.to_vec()))
                .collect::<Vec<_>>()
        );
        let policy = &registry["v3Policy"];
        assert_eq!(
            policy["runtimePermissionCompatibilityExceptions"],
            serde_json::json!(V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS)
        );
        assert_eq!(
            policy["classificationContributionRules"],
            serde_json::json!(V3_CLASSIFICATION_CONTRIBUTION_RULES
                .iter()
                .map(|(classification, required, forbidden)| serde_json::json!({
                    "classification": classification,
                    "requiredContributions": required,
                    "forbiddenContributions": forbidden,
                }))
                .collect::<Vec<_>>())
        );
        assert_eq!(
            policy["runtimeClassificationRules"],
            serde_json::json!(V3_RUNTIME_CLASSIFICATION_RULES
                .iter()
                .map(|(runtime_kind, classifications)| serde_json::json!({
                    "runtimeKind": runtime_kind,
                    "classifications": classifications,
                }))
                .collect::<Vec<_>>())
        );
        assert_eq!(
            policy["contributionRequiredPermissions"],
            serde_json::json!(V3_CONTRIBUTION_REQUIRED_PERMISSIONS
                .iter()
                .map(|(contribution, permissions)| serde_json::json!({
                    "contribution": contribution,
                    "permissions": permissions,
                }))
                .collect::<Vec<_>>())
        );
        assert_eq!(
            policy["runtimeContributionRequiredPermissions"],
            serde_json::json!(V3_RUNTIME_CONTRIBUTION_REQUIRED_PERMISSIONS
                .iter()
                .map(
                    |(runtime_kinds, contribution, permissions)| serde_json::json!({
                        "runtimeKinds": runtime_kinds,
                        "contribution": contribution,
                        "permissions": permissions,
                    })
                )
                .collect::<Vec<_>>())
        );
        assert_eq!(
            policy["featureCapabilityRequiredPermissions"],
            serde_json::json!(V3_FEATURE_CAPABILITY_REQUIRED_PERMISSIONS
                .iter()
                .map(|(feature_capability, permissions)| serde_json::json!({
                    "featureCapability": feature_capability,
                    "permissions": permissions,
                }))
                .collect::<Vec<_>>())
        );
    }

    #[test]
    fn runtime_mapping_and_compatibility_exceptions_are_exact() {
        for runtime in [
            "prompt-pack",
            "declarative-ui",
            "xingchen-agent",
            "xingchen-workflow",
        ] {
            assert!(is_v3_permission_runtime_allowed(
                "ai.context.augment",
                runtime
            ));
        }
        for runtime in ["legacy-js", "mcp-connector", "unknown-runtime"] {
            assert!(!is_v3_permission_runtime_allowed(
                "ai.context.augment",
                runtime
            ));
        }
        assert_eq!(
            V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS,
            ["tasks.read", "tasks.write", "mcp.connect"]
        );
        for permission in V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS {
            assert!(is_v3_permission_runtime_allowed(
                permission,
                "declarative-ui"
            ));
        }
        assert!(!is_v3_permission_runtime_allowed(
            "planning.files.write",
            "declarative-ui"
        ));
    }

    #[test]
    fn generated_v3_policy_preserves_frozen_admission_matrix() {
        for (classification, contributions) in [
            ("feature", &["feature"][..]),
            ("enhancement", &["enhancement"][..]),
            ("hybrid", &["feature", "enhancement"][..]),
        ] {
            assert!(is_v3_classification_contribution_allowed(
                classification,
                contributions
            ));
        }
        for (classification, contributions) in [
            ("feature", &["feature", "enhancement"][..]),
            ("enhancement", &["feature", "enhancement"][..]),
            ("hybrid", &["feature"][..]),
            ("hybrid", &["enhancement"][..]),
        ] {
            assert!(!is_v3_classification_contribution_allowed(
                classification,
                contributions
            ));
        }
        for (runtime_kind, classification) in [
            ("declarative-ui", "feature"),
            ("prompt-pack", "enhancement"),
            ("xingchen-agent", "feature"),
            ("xingchen-agent", "hybrid"),
            ("xingchen-workflow", "feature"),
            ("xingchen-workflow", "hybrid"),
        ] {
            assert!(is_v3_runtime_classification_allowed(
                runtime_kind,
                classification
            ));
        }
        for (runtime_kind, classification) in [
            ("declarative-ui", "enhancement"),
            ("declarative-ui", "hybrid"),
            ("prompt-pack", "feature"),
            ("prompt-pack", "hybrid"),
            ("xingchen-agent", "enhancement"),
            ("xingchen-workflow", "enhancement"),
        ] {
            assert!(!is_v3_runtime_classification_allowed(
                runtime_kind,
                classification
            ));
        }
        assert_eq!(
            required_v3_policy_permissions(
                "xingchen-workflow",
                &["feature", "enhancement"],
                &["file.docx.output"]
            ),
            [
                "ai.context.augment",
                "credentials.use",
                "agents.invoke",
                "network.xingchen",
                "ai.invoke",
                "files.writeSelected",
            ]
        );
    }
}
