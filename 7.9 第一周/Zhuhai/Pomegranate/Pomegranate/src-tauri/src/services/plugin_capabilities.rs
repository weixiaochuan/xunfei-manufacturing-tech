//! Canonical plugin capability registry bridge.

pub(crate) use super::plugin_capabilities_generated::{
    V3_MANIFEST_PERMISSIONS, VALID_PERMISSIONS,
};

#[cfg(test)]
mod tests {
    use super::{V3_MANIFEST_PERMISSIONS, VALID_PERMISSIONS};
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[test]
    fn generated_permissions_match_canonical_registry() {
        let registry: Value = serde_json::from_str(include_str!(
            "../../../config/plugin-capabilities.v1.json"
        ))
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
    }
}
