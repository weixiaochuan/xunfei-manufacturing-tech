use rusqlite::{params, OptionalExtension};
use std::fmt;

use crate::database::Database;
use crate::services::resource_ownership::ResourceOwner;

use super::external_agent::{resolve_external_agent, CallableExternalAgent};
use super::{ResolverError, ResourceKind, TrustedResource, UntrustedResourceRef};

macro_rules! trusted_child {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq)]
        pub(crate) struct $name {
            resource: TrustedResource,
            agent: CallableExternalAgent,
        }

        impl $name {
            pub(crate) fn resource(&self) -> &TrustedResource {
                &self.resource
            }

            pub(crate) fn agent(&self) -> &CallableExternalAgent {
                &self.agent
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("resource", &self.resource)
                    .field("agent", &self.agent)
                    .finish()
            }
        }
    };
}

trusted_child!(TrustedWorkflow);
trusted_child!(TrustedAgentSession);
trusted_child!(TrustedAgentMessage);
trusted_child!(TrustedAgentRequest);

fn resolve_parent_agent(
    db: &Database,
    owner: &ResourceOwner,
    agent_id: &str,
) -> Result<CallableExternalAgent, ResolverError> {
    let reference = UntrustedResourceRef::try_new("external-agent", agent_id.to_string())?;
    resolve_external_agent(db, owner, reference)
}

fn trusted_child<T>(
    reference: UntrustedResourceRef,
    owner: &ResourceOwner,
    agent: CallableExternalAgent,
    constructor: impl FnOnce(TrustedResource, CallableExternalAgent) -> T,
) -> T {
    constructor(
        TrustedResource::from_resolved(reference, owner.clone()),
        agent,
    )
}

/// Workflow 没有独立本地表；其不可信 ID 是当前 API 使用的 Workflow 型 External Agent ID。
pub(crate) fn resolve_workflow(
    db: &Database,
    owner: &ResourceOwner,
    reference: UntrustedResourceRef,
) -> Result<TrustedWorkflow, ResolverError> {
    if reference.kind() != ResourceKind::Workflow {
        return Err(ResolverError::unsupported_kind());
    }
    let agent = resolve_parent_agent(db, owner, reference.raw_id())?;
    let is_workflow = {
        let conn = db
            .conn_lock()
            .map_err(|_| ResolverError::backend_failure("workflow_lookup_failed"))?;
        conn.query_row(
            "SELECT 1
             FROM external_agents ea
             JOIN products p ON p.id = ea.product_id
             WHERE ea.id = ?1 AND p.product_type = 'xingchen-workflow'",
            params![reference.raw_id()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| ResolverError::backend_failure("workflow_lookup_failed"))?
        .is_some()
    };
    if !is_workflow {
        return Err(ResolverError::not_found_or_inaccessible());
    }
    Ok(trusted_child(reference, owner, agent, |resource, agent| {
        TrustedWorkflow { resource, agent }
    }))
}

pub(crate) fn resolve_agent_session(
    db: &Database,
    owner: &ResourceOwner,
    reference: UntrustedResourceRef,
) -> Result<TrustedAgentSession, ResolverError> {
    if reference.kind() != ResourceKind::AgentSession {
        return Err(ResolverError::unsupported_kind());
    }
    let agent_id = lookup_optional_string(
        db,
        "SELECT external_agent_id FROM agent_sessions WHERE id = ?1",
        reference.raw_id(),
        "agent_session_lookup_failed",
    )?
    .ok_or_else(ResolverError::not_found_or_inaccessible)?;
    let agent = resolve_parent_agent(db, owner, &agent_id)?;
    Ok(trusted_child(reference, owner, agent, |resource, agent| {
        TrustedAgentSession { resource, agent }
    }))
}

pub(crate) fn resolve_agent_message(
    db: &Database,
    owner: &ResourceOwner,
    reference: UntrustedResourceRef,
) -> Result<TrustedAgentMessage, ResolverError> {
    if reference.kind() != ResourceKind::AgentMessage {
        return Err(ResolverError::unsupported_kind());
    }
    let agent_id = {
        let conn = db
            .conn_lock()
            .map_err(|_| ResolverError::backend_failure("agent_message_lookup_failed"))?;
        conn.query_row(
            "SELECT s.external_agent_id
             FROM agent_messages m
             JOIN agent_sessions s ON s.id = m.session_id
             WHERE m.id = ?1",
            params![reference.raw_id()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| ResolverError::backend_failure("agent_message_lookup_failed"))?
    }
    .ok_or_else(ResolverError::not_found_or_inaccessible)?;
    let agent = resolve_parent_agent(db, owner, &agent_id)?;
    Ok(trusted_child(reference, owner, agent, |resource, agent| {
        TrustedAgentMessage { resource, agent }
    }))
}

pub(crate) fn resolve_agent_request(
    db: &Database,
    owner: &ResourceOwner,
    reference: UntrustedResourceRef,
) -> Result<TrustedAgentRequest, ResolverError> {
    if reference.kind() != ResourceKind::AgentRequest {
        return Err(ResolverError::unsupported_kind());
    }
    let agent_id = {
        let conn = db
            .conn_lock()
            .map_err(|_| ResolverError::backend_failure("agent_request_lookup_failed"))?;
        let mut stmt = conn
            .prepare(
                "SELECT external_agent_id, session_id
                 FROM usage_events WHERE request_id = ?1",
            )
            .map_err(|_| ResolverError::backend_failure("agent_request_lookup_failed"))?;
        let rows = stmt
            .query_map(params![reference.raw_id()], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .map_err(|_| ResolverError::backend_failure("agent_request_lookup_failed"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ResolverError::backend_failure("agent_request_lookup_failed"))?;
        if rows.len() != 1 {
            return Err(ResolverError::not_found_or_inaccessible());
        }
        let Some((agent_id, session_id)) = rows.into_iter().next() else {
            return Err(ResolverError::not_found_or_inaccessible());
        };
        let Some(agent_id) = agent_id else {
            return Err(ResolverError::not_found_or_inaccessible());
        };

        if let Some(session_id) = &session_id {
            let session_agent = conn
                .query_row(
                    "SELECT external_agent_id FROM agent_sessions WHERE id = ?1",
                    params![session_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|_| ResolverError::backend_failure("agent_request_lookup_failed"))?;
            if session_agent.as_deref() != Some(agent_id.as_str()) {
                return Err(ResolverError::not_found_or_inaccessible());
            }
            let mismatched_messages: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM agent_messages
                     WHERE request_id = ?1 AND session_id != ?2",
                    params![reference.raw_id(), session_id],
                    |row| row.get(0),
                )
                .map_err(|_| ResolverError::backend_failure("agent_request_lookup_failed"))?;
            if mismatched_messages != 0 {
                return Err(ResolverError::not_found_or_inaccessible());
            }
        } else {
            let message_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM agent_messages WHERE request_id = ?1",
                    params![reference.raw_id()],
                    |row| row.get(0),
                )
                .map_err(|_| ResolverError::backend_failure("agent_request_lookup_failed"))?;
            if message_count != 0 {
                return Err(ResolverError::not_found_or_inaccessible());
            }
        }
        agent_id
    };
    let agent = resolve_parent_agent(db, owner, &agent_id)?;
    Ok(trusted_child(reference, owner, agent, |resource, agent| {
        TrustedAgentRequest { resource, agent }
    }))
}

fn lookup_optional_string(
    db: &Database,
    sql: &str,
    id: &str,
    diagnostic: &'static str,
) -> Result<Option<String>, ResolverError> {
    let conn = db
        .conn_lock()
        .map_err(|_| ResolverError::backend_failure(diagnostic))?;
    conn.query_row(sql, params![id], |row| row.get(0))
        .optional()
        .map_err(|_| ResolverError::backend_failure(diagnostic))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT_ID: &str = "agent-child-owner";
    const PRODUCT_ID: &str = "workflow-product";
    const SESSION_ID: &str = "session-owned";
    const MESSAGE_ID: &str = "message-owned";
    const REQUEST_ID: &str = "request-owned";

    #[derive(Clone, Copy, Debug)]
    enum Target {
        Workflow,
        Session,
        Message,
        Request,
    }

    impl Target {
        const ALL: [Self; 4] = [Self::Workflow, Self::Session, Self::Message, Self::Request];

        fn name(self) -> &'static str {
            match self {
                Self::Workflow => "workflow",
                Self::Session => "session",
                Self::Message => "message",
                Self::Request => "request",
            }
        }

        fn resolve(self, db: &Database, owner: &ResourceOwner) -> Result<(), ResolverError> {
            match self {
                Self::Workflow => {
                    resolve_workflow(db, owner, reference("workflow", AGENT_ID)).map(|_| ())
                }
                Self::Session => {
                    resolve_agent_session(db, owner, reference("agent-session", SESSION_ID))
                        .map(|_| ())
                }
                Self::Message => {
                    resolve_agent_message(db, owner, reference("agent-message", MESSAGE_ID))
                        .map(|_| ())
                }
                Self::Request => {
                    resolve_agent_request(db, owner, reference("agent-request", REQUEST_ID))
                        .map(|_| ())
                }
            }
        }
    }

    fn db() -> Database {
        Database::init(":memory:").expect("in-memory database")
    }

    fn owner() -> ResourceOwner {
        ResourceOwner::fixture("subject-a", "installation-a")
    }

    fn reference(kind: &str, id: &str) -> UntrustedResourceRef {
        UntrustedResourceRef::try_new(kind, id).unwrap()
    }

    fn seed_valid_graph(db: &Database, owner: Option<&ResourceOwner>) {
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO plugins (id, name, version, path, main, manifest_json, enabled, status)
             VALUES ('workflow-plugin', 'workflow', '1.0.0', '/tmp/mock', 'main.js', '{}', 1, 'installed')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO products
                (id, developer_id, name, product_type, status, plugin_id,
                 developer_name, runtime_kind, review_status)
             VALUES (?1, 'dev', 'workflow', 'xingchen-workflow', 'published',
                     'workflow-plugin', 'dev', 'xingchen-workflow', 'approved')",
            params![PRODUCT_ID],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO product_versions
                (product_id, version, manifest_json, runtime_kind, source, content_hash,
                 signature_status, status, review_status)
             VALUES (?1, '1.0.0', '{}', 'xingchen-workflow', 'marketplace', 'hash',
                     'unsigned', 'active', 'approved')",
            params![PRODUCT_ID],
        )
        .unwrap();
        let version_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO plugin_installations
                (plugin_id, product_id, product_version_id, installed_version, source,
                 enabled, install_path, content_hash, status)
             VALUES ('workflow-plugin', ?1, ?2, '1.0.0', 'marketplace', 1,
                     '/tmp/mock', 'hash', 'installed')",
            params![PRODUCT_ID, version_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO external_agents
                (id, product_id, provider, name, endpoint, authentication_type,
                 streaming_type, request_mapping_json, response_mapping_json,
                 session_mapping_json, error_mapping_json, mock_mode, enabled)
             VALUES (?1, ?2, 'xingchen', 'workflow', 'mock://workflow', 'none',
                     'none', '{}', '{}', '{}', '{}', 1, 1)",
            params![AGENT_ID, PRODUCT_ID],
        )
        .unwrap();
        if let Some(owner) = owner {
            conn.execute(
                "INSERT INTO external_agent_resource_ownership
                    (external_agent_id, platform_subject_id, host_installation_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    AGENT_ID,
                    owner.platform_subject_id(),
                    owner.host_installation_id()
                ],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO agent_sessions (id, external_agent_id, title)
             VALUES (?1, ?2, 'owned')",
            params![SESSION_ID, AGENT_ID],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_messages (id, session_id, role, content, request_id)
             VALUES (?1, ?2, 'user', 'content', ?3)",
            params![MESSAGE_ID, SESSION_ID, REQUEST_ID],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_events
                (product_id, external_agent_id, session_id, request_id, status)
             VALUES (?1, ?2, ?3, ?4, 'completed')",
            params![PRODUCT_ID, AGENT_ID, SESSION_ID, REQUEST_ID],
        )
        .unwrap();
    }

    #[test]
    fn resolves_each_authoritative_child_relation_to_the_same_callable_agent() {
        let db = db();
        let owner = owner();
        seed_valid_graph(&db, Some(&owner));

        let workflow = resolve_workflow(&db, &owner, reference("workflow", AGENT_ID)).unwrap();
        let session =
            resolve_agent_session(&db, &owner, reference("agent-session", SESSION_ID)).unwrap();
        let message =
            resolve_agent_message(&db, &owner, reference("agent-message", MESSAGE_ID)).unwrap();
        let request =
            resolve_agent_request(&db, &owner, reference("agent-request", REQUEST_ID)).unwrap();

        for (resource, kind, id) in [
            (workflow.resource(), ResourceKind::Workflow, AGENT_ID),
            (session.resource(), ResourceKind::AgentSession, SESSION_ID),
            (message.resource(), ResourceKind::AgentMessage, MESSAGE_ID),
            (request.resource(), ResourceKind::AgentRequest, REQUEST_ID),
        ] {
            assert_eq!(resource.kind(), kind);
            assert_eq!(resource.resource_id(), id);
            assert_eq!(resource.owner(), &owner);
        }
        for agent in [
            workflow.agent(),
            session.agent(),
            message.agent(),
            request.agent(),
        ] {
            assert_eq!(agent.resource().resource_id(), AGENT_ID);
            assert_eq!(agent.resource().owner(), &owner);
        }
    }

    #[test]
    fn rejects_missing_cross_owner_cross_installation_and_legacy_for_every_child() {
        for target in Target::ALL {
            let missing_db = db();
            let error = target.resolve(&missing_db, &owner()).unwrap_err();
            assert_ordinary_rejection(&error, target.name());

            for (case, resolved_owner) in [
                (
                    "cross-subject",
                    ResourceOwner::fixture("subject-b", "installation-a"),
                ),
                (
                    "cross-installation",
                    ResourceOwner::fixture("subject-a", "installation-b"),
                ),
            ] {
                let db = db();
                let stored_owner = owner();
                seed_valid_graph(&db, Some(&stored_owner));
                let error = target.resolve(&db, &resolved_owner).unwrap_err();
                assert_ordinary_rejection(&error, &format!("{}-{case}", target.name()));
            }

            let legacy_db = db();
            seed_valid_graph(&legacy_db, None);
            let error = target.resolve(&legacy_db, &owner()).unwrap_err();
            assert_ordinary_rejection(&error, &format!("{}-legacy", target.name()));
        }
    }

    #[test]
    fn every_child_rechecks_each_callable_agent_state() {
        let mutations = [
            ("agent-disabled", "UPDATE external_agents SET enabled = 0"),
            (
                "agent-unavailable",
                "UPDATE external_agents SET unavailable_reason = 'disabled'",
            ),
            (
                "installation-disabled",
                "UPDATE plugin_installations SET enabled = 0",
            ),
            (
                "installation-uninstalled",
                "UPDATE plugin_installations SET status = 'uninstalled'",
            ),
            ("product-revoked", "UPDATE products SET status = 'revoked'"),
            (
                "product-suspended",
                "UPDATE products SET status = 'suspended'",
            ),
            (
                "product-delisted",
                "UPDATE products SET status = 'delisted'",
            ),
            (
                "version-revoked",
                "UPDATE product_versions SET status = 'revoked'",
            ),
            (
                "installed-version-owned-by-other-product",
                "INSERT INTO products
                    (id, developer_id, name, product_type, status, runtime_kind)
                 VALUES ('other-product', 'dev', 'other', 'xingchen-workflow',
                         'published', 'xingchen-workflow');
                 INSERT INTO product_versions
                    (product_id, version, manifest_json, runtime_kind, source, content_hash,
                     signature_status, status, review_status)
                 VALUES ('other-product', '1.0.0', '{}', 'xingchen-workflow', 'marketplace',
                         'other-hash', 'unsigned', 'active', 'approved');
                 UPDATE plugin_installations
                 SET product_version_id = (
                     SELECT id FROM product_versions WHERE product_id = 'other-product'
                 );",
            ),
            (
                "signature-revoked",
                "UPDATE product_versions SET signature_status = 'revoked'",
            ),
            (
                "runtime-mismatch",
                "UPDATE product_versions SET runtime_kind = 'xingchen-agent'",
            ),
            (
                "non-byok",
                "UPDATE product_versions SET manifest_json = '{\"deliveryMode\":\"hosted-api\"}'",
            ),
        ];

        for target in Target::ALL {
            for (case, mutation) in mutations {
                let db = db();
                let owner = owner();
                seed_valid_graph(&db, Some(&owner));
                db.conn_lock().unwrap().execute_batch(mutation).unwrap();
                let error = target.resolve(&db, &owner).unwrap_err();
                assert_ordinary_rejection(&error, &format!("{}-{case}", target.name()));
            }
        }
    }

    #[test]
    fn rejects_broken_parent_and_cross_parent_relations() {
        let missing_parent_db = db();
        let owner = owner();
        seed_valid_graph(&missing_parent_db, Some(&owner));
        let conn = missing_parent_db.conn_lock().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             DELETE FROM agent_sessions WHERE id = 'session-owned';",
        )
        .unwrap();
        drop(conn);
        assert_ordinary_rejection(
            &resolve_agent_message(
                &missing_parent_db,
                &owner,
                reference("agent-message", MESSAGE_ID),
            )
            .unwrap_err(),
            "message-parent-missing",
        );

        let db = db();
        seed_valid_graph(&db, Some(&owner));
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO external_agents
                (id, product_id, provider, name, endpoint, authentication_type,
                 streaming_type, request_mapping_json, response_mapping_json,
                 session_mapping_json, error_mapping_json, mock_mode, enabled)
             VALUES ('agent-other', ?1, 'xingchen', 'other', 'mock://other', 'none',
                     'none', '{}', '{}', '{}', '{}', 1, 1)",
            params![PRODUCT_ID],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_sessions (id, external_agent_id, title)
             VALUES ('session-other', 'agent-other', 'other')",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE usage_events SET session_id = 'session-other' WHERE request_id = ?1",
            params![REQUEST_ID],
        )
        .unwrap();
        drop(conn);
        assert_ordinary_rejection(
            &resolve_agent_request(&db, &owner, reference("agent-request", REQUEST_ID))
                .unwrap_err(),
            "request-session-agent-mismatch",
        );
    }

    #[test]
    fn rejects_children_whose_authoritative_agent_no_longer_exists() {
        for target in Target::ALL {
            let db = db();
            let owner = owner();
            seed_valid_graph(&db, Some(&owner));
            db.conn_lock()
                .unwrap()
                .execute_batch(
                    "PRAGMA foreign_keys = OFF;
                     DELETE FROM external_agents WHERE id = 'agent-child-owner';",
                )
                .unwrap();

            let error = target.resolve(&db, &owner).unwrap_err();
            assert_ordinary_rejection(&error, &format!("{}-agent-missing", target.name()));
        }
    }

    #[test]
    fn rejects_ambiguous_request_and_message_relationships() {
        let duplicate_db = db();
        let owner = owner();
        seed_valid_graph(&duplicate_db, Some(&owner));
        duplicate_db
            .conn_lock()
            .unwrap()
            .execute(
                "INSERT INTO usage_events
                    (product_id, external_agent_id, session_id, request_id, status)
                 VALUES (?1, ?2, ?3, ?4, 'completed')",
                params![PRODUCT_ID, AGENT_ID, SESSION_ID, REQUEST_ID],
            )
            .unwrap();
        assert_ordinary_rejection(
            &resolve_agent_request(
                &duplicate_db,
                &owner,
                reference("agent-request", REQUEST_ID),
            )
            .unwrap_err(),
            "request-not-unique",
        );

        let db = db();
        seed_valid_graph(&db, Some(&owner));
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO agent_sessions (id, external_agent_id, title)
             VALUES ('session-other', ?1, 'other')",
            params![AGENT_ID],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_messages (id, session_id, role, content, request_id)
             VALUES ('message-other', 'session-other', 'user', 'content', ?1)",
            params![REQUEST_ID],
        )
        .unwrap();
        drop(conn);
        assert_ordinary_rejection(
            &resolve_agent_request(&db, &owner, reference("agent-request", REQUEST_ID))
                .unwrap_err(),
            "request-message-session-mismatch",
        );
    }

    #[test]
    fn wrong_kinds_are_rejected_before_authoritative_lookup() {
        let db = db();
        let owner = owner();
        for error in [
            resolve_workflow(&db, &owner, reference("agent-session", SESSION_ID)).unwrap_err(),
            resolve_agent_session(&db, &owner, reference("agent-message", MESSAGE_ID)).unwrap_err(),
            resolve_agent_message(&db, &owner, reference("agent-request", REQUEST_ID)).unwrap_err(),
            resolve_agent_request(&db, &owner, reference("workflow", AGENT_ID)).unwrap_err(),
        ] {
            assert_eq!(error.diagnostic_code(), "resource_kind_unsupported");
        }
    }

    #[test]
    fn workflow_kind_requires_authoritative_workflow_product_identity() {
        let db = db();
        let owner = owner();
        seed_valid_graph(&db, Some(&owner));
        db.conn_lock()
            .unwrap()
            .execute_batch(
                "UPDATE products
                 SET product_type = 'xingchen-agent', runtime_kind = 'xingchen-agent';
                 UPDATE product_versions SET runtime_kind = 'xingchen-agent';",
            )
            .unwrap();

        let error = resolve_workflow(&db, &owner, reference("workflow", AGENT_ID)).unwrap_err();
        assert_ordinary_rejection(&error, "workflow-product-is-agent");
    }

    #[test]
    fn sqlite_failures_are_distinct_and_fail_closed_for_every_child() {
        for (target, table) in [
            (Target::Workflow, "external_agents"),
            (Target::Session, "agent_sessions"),
            (Target::Message, "agent_messages"),
            (Target::Request, "usage_events"),
        ] {
            let db = db();
            db.conn_lock()
                .unwrap()
                .execute(&format!("DROP TABLE {table}"), [])
                .unwrap();
            let error = target.resolve(&db, &owner()).unwrap_err();
            assert_eq!(
                error.public_message(),
                "资源解析暂不可用",
                "{}",
                target.name()
            );
            assert_ne!(error.public_message(), "资源不存在或不可访问");
        }
    }

    fn assert_ordinary_rejection(error: &ResolverError, case: &str) {
        assert_eq!(error.public_message(), "资源不存在或不可访问", "{case}");
        assert!(!error.to_string().contains(AGENT_ID), "{case}");
        assert!(!error.to_string().contains(SESSION_ID), "{case}");
        assert!(!error.to_string().contains(MESSAGE_ID), "{case}");
        assert!(!error.to_string().contains(REQUEST_ID), "{case}");
    }
}
