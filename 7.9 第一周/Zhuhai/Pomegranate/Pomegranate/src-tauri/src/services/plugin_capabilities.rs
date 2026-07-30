//! Canonical plugin capability registry bridge.

pub(crate) use super::plugin_capabilities_generated::{
    V3_MANIFEST_PERMISSIONS, V3_PERMISSION_RUNTIME_KINDS, VALID_PERMISSIONS,
};

/// 这三个权限在 registry 清理前维持既有 v3 可申请行为，不得扩展此列表。
pub(crate) const V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS: &[&str] =
    &["tasks.read", "tasks.write", "mcp.connect"];

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
mod tests {
    use super::{
        is_v3_permission_runtime_allowed, V3_MANIFEST_PERMISSIONS,
        V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS, V3_PERMISSION_RUNTIME_KINDS,
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
}
