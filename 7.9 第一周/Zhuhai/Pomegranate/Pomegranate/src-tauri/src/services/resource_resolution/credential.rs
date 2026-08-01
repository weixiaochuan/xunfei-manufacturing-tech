use rusqlite::{params, OptionalExtension};
use std::fmt;

use crate::database::Database;
use crate::services::resource_ownership::ResourceOwner;

use super::{ResolverError, ResourceKind, TrustedResource, UntrustedResourceRef};

/// 已由后端权威数据证明归属且当前可用的 Credential 身份。
///
/// 本类型只携带资源身份，不读取或暴露 secret、secret reference 或 masked hint。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TrustedCredential(TrustedResource);

impl fmt::Debug for TrustedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("TrustedCredential")
            .field(&self.0)
            .finish()
    }
}

impl TrustedCredential {
    pub(crate) fn resource(&self) -> &TrustedResource {
        &self.0
    }
}

/// 解析当前 owner 可使用的 Credential；不检查 capability 或 exact authorization。
pub(crate) fn resolve_credential(
    db: &Database,
    owner: &ResourceOwner,
    reference: UntrustedResourceRef,
) -> Result<TrustedCredential, ResolverError> {
    if reference.kind() != ResourceKind::Credential {
        return Err(ResolverError::unsupported_kind());
    }

    let row = {
        let conn = db
            .conn_lock()
            .map_err(|_| ResolverError::backend_failure("credential_lookup_failed"))?;
        conn.query_row(
            "SELECT c.configured, o.platform_subject_id, o.host_installation_id
             FROM credentials c
             LEFT JOIN credential_resource_ownership o ON o.credential_id = c.id
             WHERE c.id = ?1",
            params![reference.raw_id()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? != 0,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|_| ResolverError::backend_failure("credential_lookup_failed"))?
    };

    let Some((configured, subject, installation)) = row else {
        return Err(ResolverError::not_found_or_inaccessible());
    };
    let (Some(subject), Some(installation)) = (subject, installation) else {
        return Err(ResolverError::ownership_unprovable(
            "credential_owner_missing",
        ));
    };
    if subject != owner.platform_subject_id() {
        return Err(ResolverError::ownership_unprovable(
            "credential_subject_mismatch",
        ));
    }
    if installation != owner.host_installation_id() {
        return Err(ResolverError::ownership_unprovable(
            "credential_installation_mismatch",
        ));
    }
    if !configured {
        return Err(ResolverError::invalid_state("credential_not_configured"));
    }

    Ok(TrustedCredential(TrustedResource::from_resolved(
        reference,
        owner.clone(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::init(":memory:").expect("in-memory database")
    }

    fn seed_credential(db: &Database, id: &str, configured: bool, owner: Option<&ResourceOwner>) {
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO credentials
                (id, provider, credential_type, label, owner_scope, secret_reference,
                 configured, masked_hint)
             VALUES (?1, 'provider', 'api_key', 'label', 'deprecated', ?2, ?3, 'secret-hint')",
            params![
                id,
                format!("secure-credentials/{id}.secret"),
                configured as i64
            ],
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

    fn reference(id: &str) -> UntrustedResourceRef {
        UntrustedResourceRef::try_new("credential", id).unwrap()
    }

    #[test]
    fn resolves_only_configured_credential_for_exact_owner() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_credential(&db, "cred-safe", true, Some(&owner));

        let resolved = resolve_credential(&db, &owner, reference("cred-safe")).unwrap();
        assert_eq!(resolved.resource().kind(), ResourceKind::Credential);
        assert_eq!(resolved.resource().resource_id(), "cred-safe");
        assert_eq!(resolved.resource().owner(), &owner);
        let debug = format!("{:?}", resolved.resource());
        assert!(!debug.contains("cred-safe"));
        assert!(!debug.contains("secret-hint"));
    }

    #[test]
    fn rejects_wrong_kind_and_malformed_id_before_database_lookup() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        let wrong_kind = UntrustedResourceRef::try_new("external-agent", "cred-safe").unwrap();
        assert_eq!(
            resolve_credential(&db, &owner, wrong_kind)
                .unwrap_err()
                .diagnostic_code(),
            "resource_kind_unsupported"
        );
        assert!(UntrustedResourceRef::try_new("credential", " cred-safe").is_err());
    }

    #[test]
    fn missing_cross_owner_and_legacy_unowned_share_safe_external_message() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        let other_subject = ResourceOwner::fixture("subject-b", "installation-a");
        let other_installation = ResourceOwner::fixture("subject-a", "installation-b");
        seed_credential(&db, "cred-owned", true, Some(&owner));
        seed_credential(&db, "cred-legacy", true, None);

        for error in [
            resolve_credential(&db, &owner, reference("cred-missing")).unwrap_err(),
            resolve_credential(&db, &other_subject, reference("cred-owned")).unwrap_err(),
            resolve_credential(&db, &other_installation, reference("cred-owned")).unwrap_err(),
            resolve_credential(&db, &owner, reference("cred-legacy")).unwrap_err(),
        ] {
            assert_eq!(error.public_message(), "资源不存在或不可访问");
            assert!(!error.to_string().contains(error.diagnostic_code()));
        }
    }

    #[test]
    fn unconfigured_credential_is_not_resolved() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_credential(&db, "cred-empty", false, Some(&owner));
        let error = resolve_credential(&db, &owner, reference("cred-empty")).unwrap_err();
        assert_eq!(error.diagnostic_code(), "credential_not_configured");
        assert_eq!(error.public_message(), "资源不存在或不可访问");
    }

    #[test]
    fn database_failure_is_distinct_and_fail_closed() {
        let db = db();
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        db.conn_lock()
            .unwrap()
            .execute("DROP TABLE credentials", [])
            .unwrap();
        let error = resolve_credential(&db, &owner, reference("cred-any")).unwrap_err();
        assert_eq!(error.diagnostic_code(), "credential_lookup_failed");
        assert_eq!(error.public_message(), "资源解析暂不可用");
    }
}
