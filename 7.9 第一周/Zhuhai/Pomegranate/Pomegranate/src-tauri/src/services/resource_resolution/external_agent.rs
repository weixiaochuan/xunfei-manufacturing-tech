use rusqlite::{params, OptionalExtension};
use std::fmt;

use crate::database::Database;
use crate::services::resource_ownership::ResourceOwner;

use super::{ResolverError, ResourceKind, TrustedResource, UntrustedResourceRef};

/// 已由权威关系和当前本地状态证明可进入 Agent 调用链的资源身份。
///
/// capability、entitlement、exact authorization 和远端健康状态仍由后续层独立判断。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CallableExternalAgent {
    resource: TrustedResource,
    runtime_kind: ExternalAgentRuntimeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExternalAgentRuntimeKind {
    Agent,
    Workflow,
}

impl fmt::Debug for CallableExternalAgent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("CallableExternalAgent")
            .field(&self.resource)
            .finish()
    }
}

impl CallableExternalAgent {
    pub(crate) fn resource(&self) -> &TrustedResource {
        &self.resource
    }

    pub(crate) fn runtime_kind(&self) -> ExternalAgentRuntimeKind {
        self.runtime_kind
    }
}

#[derive(Debug)]
struct AgentResolutionRow {
    enabled: bool,
    unavailable_reason: Option<String>,
    owner_subject: Option<String>,
    owner_installation: Option<String>,
    callable_runtime_kind: Option<String>,
}

/// 解析当前 owner 可调用的 External Agent；不授予任何插件 capability。
pub(crate) fn resolve_external_agent(
    db: &Database,
    owner: &ResourceOwner,
    reference: UntrustedResourceRef,
) -> Result<CallableExternalAgent, ResolverError> {
    if reference.kind() != ResourceKind::ExternalAgent {
        return Err(ResolverError::unsupported_kind());
    }

    let row = {
        let conn = db
            .conn_lock()
            .map_err(|_| ResolverError::backend_failure("external_agent_lookup_failed"))?;
        conn.query_row(
            "SELECT ea.enabled, ea.unavailable_reason,
                    o.platform_subject_id, o.host_installation_id,
                    (
                        SELECT p.product_type
                        FROM products p
                        JOIN plugin_installations pi ON pi.product_id = p.id
                        LEFT JOIN product_versions pv ON pv.id = pi.product_version_id
                        WHERE p.id = ea.product_id
                          AND (
                            (
                              p.product_type = 'xingchen-agent'
                              AND p.runtime_kind = 'xingchen-agent'
                              AND pv.runtime_kind = 'xingchen-agent'
                            )
                            OR (
                              p.product_type = 'xingchen-workflow'
                              AND p.runtime_kind = 'xingchen-workflow'
                              AND pv.runtime_kind = 'xingchen-workflow'
                            )
                          )
                          AND p.status NOT IN ('revoked', 'suspended', 'delisted')
                          AND pi.enabled = 1
                          AND COALESCE(pi.status, 'installed') != 'uninstalled'
                          AND pv.product_id = p.id
                          AND COALESCE(pv.status, 'active') != 'revoked'
                          AND COALESCE(pv.signature_status, 'unsigned') != 'revoked'
                          AND COALESCE(json_extract(pv.manifest_json, '$.deliveryMode'), 'byok') = 'byok'
                        LIMIT 1
                    )
             FROM external_agents ea
             LEFT JOIN external_agent_resource_ownership o ON o.external_agent_id = ea.id
             WHERE ea.id = ?1",
            params![reference.raw_id()],
            |row| {
                Ok(AgentResolutionRow {
                    enabled: row.get::<_, i64>(0)? != 0,
                    unavailable_reason: row.get(1)?,
                    owner_subject: row.get(2)?,
                    owner_installation: row.get(3)?,
                    callable_runtime_kind: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|_| ResolverError::backend_failure("external_agent_lookup_failed"))?
    };

    let Some(row) = row else {
        return Err(ResolverError::not_found_or_inaccessible());
    };
    let (Some(subject), Some(installation)) = (row.owner_subject, row.owner_installation) else {
        return Err(ResolverError::ownership_unprovable(
            "external_agent_owner_missing",
        ));
    };
    if subject != owner.platform_subject_id() {
        return Err(ResolverError::ownership_unprovable(
            "external_agent_subject_mismatch",
        ));
    }
    if installation != owner.host_installation_id() {
        return Err(ResolverError::ownership_unprovable(
            "external_agent_installation_mismatch",
        ));
    }
    if !row.enabled {
        return Err(ResolverError::invalid_state("external_agent_disabled"));
    }
    if row.unavailable_reason.is_some() {
        return Err(ResolverError::invalid_state("external_agent_unavailable"));
    }
    let runtime_kind = match row.callable_runtime_kind.as_deref() {
        Some("xingchen-agent") => ExternalAgentRuntimeKind::Agent,
        Some("xingchen-workflow") => ExternalAgentRuntimeKind::Workflow,
        _ => {
            return Err(ResolverError::invalid_state(
                "external_agent_product_unavailable",
            ))
        }
    };
    Ok(CallableExternalAgent {
        resource: TrustedResource::from_resolved(reference, owner.clone()),
        runtime_kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::init(":memory:").expect("in-memory database")
    }

    fn seed_product(db: &Database, product_id: &str, enabled: bool, status: &str) {
        seed_product_with_type(db, product_id, enabled, status, "xingchen-agent");
    }

    fn seed_product_with_type(
        db: &Database,
        product_id: &str,
        enabled: bool,
        status: &str,
        runtime_kind: &str,
    ) {
        let conn = db.conn_lock().unwrap();
        let plugin_id = format!("{product_id}-plugin");
        conn.execute(
            "INSERT INTO plugins (id, name, version, path, main, manifest_json, enabled, status)
             VALUES (?1, ?2, '1.0.0', '/tmp/mock', 'main.js', '{}', ?3, 'installed')",
            params![plugin_id, product_id, enabled as i64],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO products
                (id, developer_id, name, product_type, status, plugin_id,
                 developer_name, runtime_kind, review_status)
             VALUES (?1, 'dev', ?1, ?4, ?2, ?3,
                     'dev', ?4, 'approved')",
            params![product_id, status, plugin_id, runtime_kind],
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
             VALUES (?1, ?2, ?3, '1.0.0', 'marketplace', ?4, '/tmp/mock', 'hash', 'installed')",
            params![plugin_id, product_id, version_id, enabled as i64],
        )
        .unwrap();
    }

    fn seed_agent(
        db: &Database,
        id: &str,
        product_id: &str,
        enabled: bool,
        unavailable_reason: Option<&str>,
        owner: Option<&ResourceOwner>,
    ) {
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO external_agents
                (id, product_id, provider, name, endpoint, authentication_type,
                 streaming_type, request_mapping_json, response_mapping_json,
                 session_mapping_json, error_mapping_json, mock_mode, enabled,
                 unavailable_reason)
             VALUES (?1, ?2, 'xingchen', 'agent', 'mock://xingchen', 'none',
                     'none', '{}', '{}', '{}', '{}', 1, ?3, ?4)",
            params![id, product_id, enabled as i64, unavailable_reason],
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

    fn reference(id: &str) -> UntrustedResourceRef {
        UntrustedResourceRef::try_new("external-agent", id).unwrap()
    }

    #[test]
    fn resolves_only_owned_enabled_available_agent_with_active_product() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_product(&db, "product-a", true, "published");
        seed_agent(&db, "agent-safe", "product-a", true, None, Some(&owner));

        let resolved = resolve_external_agent(&db, &owner, reference("agent-safe")).unwrap();
        assert_eq!(resolved.resource().kind(), ResourceKind::ExternalAgent);
        assert_eq!(resolved.resource().resource_id(), "agent-safe");
        assert_eq!(resolved.resource().owner(), &owner);
        assert!(!format!("{:?}", resolved.resource()).contains("agent-safe"));
    }

    #[test]
    fn rejects_wrong_kind_and_malformed_id_before_database_lookup() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        let wrong_kind = UntrustedResourceRef::try_new("credential", "agent-safe").unwrap();
        assert_eq!(
            resolve_external_agent(&db, &owner, wrong_kind)
                .unwrap_err()
                .diagnostic_code(),
            "resource_kind_unsupported"
        );
        assert!(UntrustedResourceRef::try_new("external-agent", "agent-safe\n").is_err());
    }

    #[test]
    fn missing_cross_owner_and_legacy_unowned_share_safe_external_message() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        let other_subject = ResourceOwner::fixture("subject-b", "installation-a");
        let other_installation = ResourceOwner::fixture("subject-a", "installation-b");
        seed_product(&db, "product-a", true, "published");
        seed_agent(&db, "agent-owned", "product-a", true, None, Some(&owner));
        seed_agent(&db, "agent-legacy", "product-a", true, None, None);

        for error in [
            resolve_external_agent(&db, &owner, reference("agent-missing")).unwrap_err(),
            resolve_external_agent(&db, &other_subject, reference("agent-owned")).unwrap_err(),
            resolve_external_agent(&db, &other_installation, reference("agent-owned")).unwrap_err(),
            resolve_external_agent(&db, &owner, reference("agent-legacy")).unwrap_err(),
        ] {
            assert_eq!(error.public_message(), "资源不存在或不可访问");
            assert!(!error.to_string().contains(error.diagnostic_code()));
        }
    }

    #[test]
    fn rejects_every_persisted_non_callable_state_with_one_external_message() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_product(&db, "product-active", true, "published");
        seed_product(&db, "product-disabled", false, "published");
        seed_product(&db, "product-revoked", true, "revoked");
        seed_product_with_type(
            &db,
            "product-wrong-runtime",
            true,
            "published",
            "declarative-ui",
        );
        seed_agent(
            &db,
            "agent-disabled",
            "product-active",
            false,
            None,
            Some(&owner),
        );
        seed_agent(
            &db,
            "agent-deleted",
            "product-active",
            false,
            Some("deleted"),
            Some(&owner),
        );
        seed_agent(
            &db,
            "agent-invalid",
            "product-active",
            true,
            Some("credential_deleted"),
            Some(&owner),
        );
        seed_agent(
            &db,
            "agent-product-disabled",
            "product-disabled",
            true,
            None,
            Some(&owner),
        );
        seed_agent(
            &db,
            "agent-product-revoked",
            "product-revoked",
            true,
            None,
            Some(&owner),
        );
        seed_agent(
            &db,
            "agent-wrong-runtime",
            "product-wrong-runtime",
            true,
            None,
            Some(&owner),
        );

        for id in [
            "agent-disabled",
            "agent-deleted",
            "agent-invalid",
            "agent-product-disabled",
            "agent-product-revoked",
            "agent-wrong-runtime",
        ] {
            let error = resolve_external_agent(&db, &owner, reference(id)).unwrap_err();
            assert_eq!(error.public_message(), "资源不存在或不可访问");
            assert!(!error.to_string().contains(id));
        }
    }

    #[test]
    fn resolves_workflow_only_when_all_runtime_fields_match() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_product_with_type(
            &db,
            "workflow-product",
            true,
            "published",
            "xingchen-workflow",
        );
        seed_agent(
            &db,
            "workflow-agent",
            "workflow-product",
            true,
            None,
            Some(&owner),
        );

        assert!(resolve_external_agent(&db, &owner, reference("workflow-agent")).is_ok());
    }

    #[test]
    fn rejects_each_independent_product_and_installation_state() {
        for (case, mutation) in [
            (
                "product-runtime-non-xingchen",
                "UPDATE products SET runtime_kind = 'declarative-ui' WHERE id = 'product-a';",
            ),
            (
                "product-type-non-xingchen",
                "UPDATE products SET product_type = 'local-plugin' WHERE id = 'product-a';",
            ),
            (
                "agent-product-workflow-runtime",
                "UPDATE products SET runtime_kind = 'xingchen-workflow' WHERE id = 'product-a';",
            ),
            (
                "workflow-product-agent-runtime",
                "UPDATE products SET product_type = 'xingchen-workflow' WHERE id = 'product-a';",
            ),
            (
                "version-runtime-mismatch",
                "UPDATE product_versions SET runtime_kind = 'xingchen-workflow' WHERE product_id = 'product-a';",
            ),
            (
                "all-runtime-fields-non-xingchen",
                "UPDATE products SET product_type = 'local-plugin', runtime_kind = 'declarative-ui' WHERE id = 'product-a';
                 UPDATE product_versions SET runtime_kind = 'declarative-ui' WHERE product_id = 'product-a';",
            ),
            (
                "product-suspended",
                "UPDATE products SET status = 'suspended' WHERE id = 'product-a';",
            ),
            (
                "product-delisted",
                "UPDATE products SET status = 'delisted' WHERE id = 'product-a';",
            ),
            (
                "installation-disabled",
                "UPDATE plugin_installations SET enabled = 0 WHERE product_id = 'product-a';",
            ),
            (
                "installation-uninstalled",
                "UPDATE plugin_installations SET status = 'uninstalled' WHERE product_id = 'product-a';",
            ),
            (
                "version-revoked",
                "UPDATE product_versions SET status = 'revoked' WHERE product_id = 'product-a';",
            ),
            (
                "signature-revoked",
                "UPDATE product_versions SET signature_status = 'revoked' WHERE product_id = 'product-a';",
            ),
            (
                "delivery-mode-hosted",
                "UPDATE product_versions SET manifest_json = '{\"deliveryMode\":\"hosted-api\"}' WHERE product_id = 'product-a';",
            ),
        ] {
            let db = db();
            let owner = ResourceOwner::fixture("subject-a", "installation-a");
            seed_product(&db, "product-a", true, "published");
            seed_agent(&db, "agent-a", "product-a", true, None, Some(&owner));
            db.conn_lock().unwrap().execute_batch(mutation).unwrap();

            let error = resolve_external_agent(&db, &owner, reference("agent-a"))
                .unwrap_err();
            assert_eq!(
                error.diagnostic_code(),
                "external_agent_product_unavailable",
                "{case}"
            );
            assert_eq!(error.public_message(), "资源不存在或不可访问", "{case}");
        }
    }

    #[test]
    fn rejects_product_type_and_version_runtime_mismatch() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_product_with_type(
            &db,
            "workflow-product",
            true,
            "published",
            "xingchen-workflow",
        );
        seed_agent(
            &db,
            "workflow-agent",
            "workflow-product",
            true,
            None,
            Some(&owner),
        );
        db.conn_lock()
            .unwrap()
            .execute(
                "UPDATE product_versions SET runtime_kind = 'xingchen-agent'
                 WHERE product_id = 'workflow-product'",
                [],
            )
            .unwrap();

        assert!(resolve_external_agent(&db, &owner, reference("workflow-agent")).is_err());
    }

    #[test]
    fn rejects_installed_version_owned_by_another_product() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_product(&db, "product-a", true, "published");
        seed_agent(&db, "agent-a", "product-a", true, None, Some(&owner));
        let conn = db.conn_lock().unwrap();
        conn.execute_batch(
            "INSERT INTO products
                (id, developer_id, name, product_type, status, runtime_kind)
             VALUES ('product-b', 'dev', 'other', 'xingchen-agent', 'published', 'xingchen-agent');
             INSERT INTO product_versions
                (product_id, version, manifest_json, runtime_kind, source, content_hash,
                 signature_status, status, review_status)
             VALUES ('product-b', '1.0.0', '{}', 'xingchen-agent', 'marketplace', 'hash-b',
                     'unsigned', 'active', 'approved');
             UPDATE plugin_installations
             SET product_version_id = (SELECT id FROM product_versions WHERE product_id = 'product-b')
             WHERE product_id = 'product-a';",
        )
        .unwrap();
        drop(conn);

        assert!(resolve_external_agent(&db, &owner, reference("agent-a")).is_err());
    }

    #[test]
    fn independent_rejections_share_one_external_message() {
        for (case, mutation) in [
            (
                "runtime-mismatch",
                "UPDATE products SET runtime_kind = 'xingchen-workflow' WHERE id = 'product-a';",
            ),
            (
                "installation-disabled",
                "UPDATE plugin_installations SET enabled = 0 WHERE product_id = 'product-a';",
            ),
            (
                "version-revoked",
                "UPDATE product_versions SET status = 'revoked' WHERE product_id = 'product-a';",
            ),
        ] {
            let db = db();
            let owner = ResourceOwner::fixture("subject-a", "installation-a");
            seed_product(&db, "product-a", true, "published");
            seed_agent(&db, "agent-a", "product-a", true, None, Some(&owner));
            db.conn_lock().unwrap().execute_batch(mutation).unwrap();

            let error = resolve_external_agent(&db, &owner, reference("agent-a")).unwrap_err();
            assert_eq!(error.public_message(), "资源不存在或不可访问", "{case}");
            assert!(!error.to_string().contains(case));
        }
    }

    #[test]
    fn database_failure_is_distinct_and_fail_closed() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        db.conn_lock()
            .unwrap()
            .execute("DROP TABLE external_agents", [])
            .unwrap();
        let error = resolve_external_agent(&db, &owner, reference("agent-any")).unwrap_err();
        assert_eq!(error.diagnostic_code(), "external_agent_lookup_failed");
        assert_eq!(error.public_message(), "资源解析暂不可用");
    }
}
