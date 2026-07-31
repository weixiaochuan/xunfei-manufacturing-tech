use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::AppError;
use crate::models::{
    PendingPluginCapabilityAuthorization, PluginAuthorizationContext,
    PluginAuthorizationContextKind, PluginAuthorizationIdentityBinding,
    PluginAuthorizationIdentityBindingStatus, PluginAuthorizationLifetime,
    PluginAuthorizationScope, PluginAuthorizationSource, PluginAuthorizationState,
    PluginAuthorizationSubject, PluginAuthorizationSubjectKind, PluginCapabilityAuthorization,
};
use crate::services::plugin_capabilities::V3_MANIFEST_PERMISSIONS;

use super::Database;

const AUTHORIZATION_COLUMNS: &str = "
    id, subject_kind, subject_id, context_kind, context_id, plugin_id, capability_id,
    capability_semantic_version, scope_kind, scope_key, state, source, lifetime,
    first_authorized_version, last_confirmed_version,
    publisher_identity, publisher_binding_status,
    signature_identity, signature_binding_status,
    created_at, updated_at, revoked_at, expires_at, revision";

#[derive(Debug)]
struct RawAuthorization {
    id: i64,
    subject_kind: String,
    subject_id: String,
    context_kind: String,
    context_id: String,
    plugin_id: String,
    capability_id: String,
    capability_semantic_version: Option<String>,
    scope_kind: String,
    scope_key: String,
    state: String,
    source: String,
    lifetime: String,
    first_authorized_version: Option<String>,
    last_confirmed_version: Option<String>,
    publisher_identity: Option<String>,
    publisher_binding_status: String,
    signature_identity: Option<String>,
    signature_binding_status: String,
    created_at: String,
    updated_at: String,
    revoked_at: Option<String>,
    expires_at: Option<String>,
    revision: i64,
}

impl PluginAuthorizationSubjectKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::PlatformUser => "platform_user",
            Self::LocalProfile => "local_profile",
            Self::Administrator => "administrator",
        }
    }

    fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "platform_user" => Ok(Self::PlatformUser),
            "local_profile" => Ok(Self::LocalProfile),
            "administrator" => Ok(Self::Administrator),
            _ => Err(stored_invalid("subject_kind")),
        }
    }
}

impl PluginAuthorizationContextKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Installation => "installation",
        }
    }

    fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "device" => Ok(Self::Device),
            "installation" => Ok(Self::Installation),
            _ => Err(stored_invalid("context_kind")),
        }
    }
}

impl PluginAuthorizationState {
    fn as_db(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Granted => "granted",
            Self::Denied => "denied",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        }
    }

    fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "pending" => Ok(Self::Pending),
            "granted" => Ok(Self::Granted),
            "denied" => Ok(Self::Denied),
            "revoked" => Ok(Self::Revoked),
            "expired" => Ok(Self::Expired),
            _ => Err(stored_invalid("state")),
        }
    }
}

impl PluginAuthorizationSource {
    fn as_db(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::OnDemand => "on_demand",
            Self::AdminPolicy => "admin_policy",
            Self::Migration => "migration",
        }
    }

    fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "install" => Ok(Self::Install),
            "on_demand" => Ok(Self::OnDemand),
            "admin_policy" => Ok(Self::AdminPolicy),
            "migration" => Ok(Self::Migration),
            _ => Err(stored_invalid("source")),
        }
    }
}

impl PluginAuthorizationLifetime {
    fn as_db(self) -> &'static str {
        match self {
            Self::Persistent => "persistent",
            Self::Session => "session",
            Self::OneShot => "one_shot",
            Self::Policy => "policy",
        }
    }

    fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "persistent" => Ok(Self::Persistent),
            "session" => Ok(Self::Session),
            "one_shot" => Ok(Self::OneShot),
            "policy" => Ok(Self::Policy),
            _ => Err(stored_invalid("lifetime")),
        }
    }
}

impl PluginAuthorizationIdentityBindingStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Unverified => "unverified",
            Self::Verified => "verified",
        }
    }

    fn from_db(value: &str) -> Result<Self, AppError> {
        match value {
            "unavailable" => Ok(Self::Unavailable),
            "unverified" => Ok(Self::Unverified),
            "verified" => Ok(Self::Verified),
            _ => Err(stored_invalid("identity_binding_status")),
        }
    }
}

impl Database {
    /// 读取一条正式授权记录。记录缺失保持为 `None`，不会回退到 legacy 布尔授权。
    pub fn get_formal_plugin_capability_authorization(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability_id: &str,
        scope: &PluginAuthorizationScope,
    ) -> Result<Option<PluginCapabilityAuthorization>, AppError> {
        validate_identity(subject, context, plugin_id, capability_id, scope)?;
        let conn = self.conn_lock()?;
        read_authorization(&conn, subject, context, plugin_id, capability_id, scope)
    }

    /// 列出同一主体、上下文和插件下的全部正式授权记录。
    pub fn list_formal_plugin_capability_authorizations(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
    ) -> Result<Vec<PluginCapabilityAuthorization>, AppError> {
        validate_subject(subject)?;
        validate_context(context)?;
        validate_non_empty_plugin_id(plugin_id)?;
        let conn = self.conn_lock()?;
        let sql = format!(
            "SELECT {AUTHORIZATION_COLUMNS}
             FROM plugin_capability_authorizations
             WHERE subject_kind = ?1 AND subject_id = ?2
               AND context_kind = ?3 AND context_id = ?4 AND plugin_id = ?5
             ORDER BY capability_id, scope_kind, scope_key"
        );
        let mut statement = conn.prepare(&sql)?;
        let rows = statement
            .query_map(
                params![
                    subject.kind.as_db(),
                    subject.id,
                    context.kind.as_db(),
                    context.id,
                    plugin_id
                ],
                raw_from_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(parse_raw).collect()
    }

    /// 创建 pending 请求，或在 revision 匹配时刷新已有 pending 请求。
    ///
    /// denied/revoked/expired/granted 均不会被该普通入口覆盖。
    pub fn create_or_update_pending_formal_plugin_capability_authorization(
        &self,
        input: &PendingPluginCapabilityAuthorization,
        expected_revision: Option<i64>,
    ) -> Result<PluginCapabilityAuthorization, AppError> {
        validate_pending_input(input)?;
        let now = utc_now();
        let conn = self.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        let existing = read_authorization(
            &tx,
            &input.subject,
            &input.context,
            &input.plugin_id,
            &input.capability_id,
            &input.scope,
        )?;
        match existing {
            None => {
                if expected_revision.is_some() {
                    return Err(AppError::PluginAuthorizationRevisionConflict);
                }
                tx.execute(
                    "INSERT INTO plugin_capability_authorizations (
                        subject_kind, subject_id, context_kind, context_id,
                        plugin_id, capability_id, capability_semantic_version,
                        scope_kind, scope_key, state, source, lifetime,
                        first_authorized_version, last_confirmed_version,
                        publisher_identity, publisher_binding_status,
                        signature_identity, signature_binding_status,
                        created_at, updated_at, revoked_at, expires_at, revision
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                        'pending', ?10, ?11, NULL, ?12, ?13, ?14, ?15, ?16,
                        ?17, ?17, NULL, ?18, 0
                     )",
                    params![
                        input.subject.kind.as_db(),
                        input.subject.id,
                        input.context.kind.as_db(),
                        input.context.id,
                        input.plugin_id,
                        input.capability_id,
                        input.capability_semantic_version,
                        input.scope.kind,
                        input.scope.key,
                        input.source.as_db(),
                        input.lifetime.as_db(),
                        input.last_confirmed_version,
                        input.publisher.identity,
                        input.publisher.status.as_db(),
                        input.signature.identity,
                        input.signature.status.as_db(),
                        now,
                        input.expires_at,
                    ],
                )?;
            }
            Some(record) => {
                if record.state != PluginAuthorizationState::Pending {
                    return Err(transition_invalid(
                        record.state,
                        PluginAuthorizationState::Pending,
                    ));
                }
                require_expected_revision(&record, expected_revision)?;
                let affected = tx.execute(
                    "UPDATE plugin_capability_authorizations
                     SET capability_semantic_version = ?1, source = ?2, lifetime = ?3,
                         last_confirmed_version = ?4,
                         publisher_identity = ?5, publisher_binding_status = ?6,
                         signature_identity = ?7, signature_binding_status = ?8,
                         expires_at = ?9, updated_at = ?10, revision = revision + 1
                     WHERE id = ?11 AND revision = ?12 AND state = 'pending'",
                    params![
                        input.capability_semantic_version,
                        input.source.as_db(),
                        input.lifetime.as_db(),
                        input.last_confirmed_version,
                        input.publisher.identity,
                        input.publisher.status.as_db(),
                        input.signature.identity,
                        input.signature.status.as_db(),
                        input.expires_at,
                        now,
                        record.id,
                        record.revision,
                    ],
                )?;
                require_single_revision_update(affected)?;
            }
        }
        let updated = read_authorization(
            &tx,
            &input.subject,
            &input.context,
            &input.plugin_id,
            &input.capability_id,
            &input.scope,
        )?
        .ok_or(AppError::PluginAuthorizationNotFound)?;
        tx.commit()?;
        Ok(updated)
    }

    /// 将 pending 正式授权明确批准为 granted。
    pub fn grant_pending_formal_plugin_capability_authorization(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability_id: &str,
        scope: &PluginAuthorizationScope,
        confirmed_version: &str,
        expected_revision: i64,
    ) -> Result<PluginCapabilityAuthorization, AppError> {
        validate_non_empty_version(confirmed_version)?;
        self.transition_formal_plugin_capability_authorization(
            subject,
            context,
            plugin_id,
            capability_id,
            scope,
            expected_revision,
            PluginAuthorizationState::Pending,
            PluginAuthorizationState::Granted,
            Some(confirmed_version),
        )
    }

    /// 将 pending 正式授权明确记录为 denied。
    pub fn deny_pending_formal_plugin_capability_authorization(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability_id: &str,
        scope: &PluginAuthorizationScope,
        expected_revision: i64,
    ) -> Result<PluginCapabilityAuthorization, AppError> {
        self.transition_formal_plugin_capability_authorization(
            subject,
            context,
            plugin_id,
            capability_id,
            scope,
            expected_revision,
            PluginAuthorizationState::Pending,
            PluginAuthorizationState::Denied,
            None,
        )
    }

    /// 将 granted 正式授权明确撤销；普通 grant 入口不能恢复该状态。
    pub fn revoke_granted_formal_plugin_capability_authorization(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability_id: &str,
        scope: &PluginAuthorizationScope,
        expected_revision: i64,
    ) -> Result<PluginCapabilityAuthorization, AppError> {
        self.transition_formal_plugin_capability_authorization(
            subject,
            context,
            plugin_id,
            capability_id,
            scope,
            expected_revision,
            PluginAuthorizationState::Granted,
            PluginAuthorizationState::Revoked,
            None,
        )
    }

    /// 将已到期的 pending 或 granted 正式授权显式落为 expired。
    pub fn expire_due_formal_plugin_capability_authorization(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability_id: &str,
        scope: &PluginAuthorizationScope,
        expected_revision: i64,
    ) -> Result<PluginCapabilityAuthorization, AppError> {
        validate_identity(subject, context, plugin_id, capability_id, scope)?;
        let now = Utc::now();
        let now_text = format_utc(now);
        let conn = self.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        let record = read_authorization(&tx, subject, context, plugin_id, capability_id, scope)?
            .ok_or(AppError::PluginAuthorizationNotFound)?;
        require_expected_revision(&record, Some(expected_revision))?;
        if !matches!(
            record.state,
            PluginAuthorizationState::Pending | PluginAuthorizationState::Granted
        ) {
            return Err(transition_invalid(
                record.state,
                PluginAuthorizationState::Expired,
            ));
        }
        let expires_at =
            record
                .expires_at
                .as_deref()
                .ok_or(AppError::PluginAuthorizationTimeInvalid {
                    field: "expires_at",
                })?;
        if parse_utc(expires_at, "expires_at")? > now {
            return Err(AppError::PluginAuthorizationTimeInvalid {
                field: "expires_at_not_due",
            });
        }
        let affected = tx.execute(
            "UPDATE plugin_capability_authorizations
             SET state = 'expired', revoked_at = NULL, updated_at = ?1,
                 revision = revision + 1
             WHERE id = ?2 AND revision = ?3 AND state IN ('pending','granted')",
            params![now_text, record.id, record.revision],
        )?;
        require_single_revision_update(affected)?;
        let updated = read_authorization(&tx, subject, context, plugin_id, capability_id, scope)?
            .ok_or(AppError::PluginAuthorizationNotFound)?;
        tx.commit()?;
        Ok(updated)
    }

    /// 按调用方提供的 UTC 时点判断正式授权是否有效；missing 明确返回 false。
    pub fn is_formal_plugin_capability_authorization_effective_at(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability_id: &str,
        scope: &PluginAuthorizationScope,
        at: &str,
    ) -> Result<bool, AppError> {
        let at = parse_utc(at, "at")?;
        let Some(record) = self.get_formal_plugin_capability_authorization(
            subject,
            context,
            plugin_id,
            capability_id,
            scope,
        )?
        else {
            return Ok(false);
        };
        if record.state != PluginAuthorizationState::Granted {
            return Ok(false);
        }
        match record.expires_at {
            Some(value) => Ok(parse_stored_utc(&value, "expires_at")? > at),
            None => Ok(true),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_formal_plugin_capability_authorization(
        &self,
        subject: &PluginAuthorizationSubject,
        context: &PluginAuthorizationContext,
        plugin_id: &str,
        capability_id: &str,
        scope: &PluginAuthorizationScope,
        expected_revision: i64,
        from: PluginAuthorizationState,
        to: PluginAuthorizationState,
        confirmed_version: Option<&str>,
    ) -> Result<PluginCapabilityAuthorization, AppError> {
        validate_identity(subject, context, plugin_id, capability_id, scope)?;
        let now = utc_now();
        let conn = self.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        let record = read_authorization(&tx, subject, context, plugin_id, capability_id, scope)?
            .ok_or(AppError::PluginAuthorizationNotFound)?;
        require_expected_revision(&record, Some(expected_revision))?;
        if record.state != from {
            return Err(transition_invalid(record.state, to));
        }
        let revoked_at = (to == PluginAuthorizationState::Revoked).then_some(now.as_str());
        let affected = tx.execute(
            "UPDATE plugin_capability_authorizations
             SET state = ?1,
                 first_authorized_version = CASE
                     WHEN ?1 = 'granted' AND first_authorized_version IS NULL THEN ?2
                     ELSE first_authorized_version
                 END,
                 last_confirmed_version = CASE
                     WHEN ?1 = 'granted' THEN ?2
                     ELSE last_confirmed_version
                 END,
                 revoked_at = ?3,
                 updated_at = ?4,
                 revision = revision + 1
             WHERE id = ?5 AND revision = ?6 AND state = ?7",
            params![
                to.as_db(),
                confirmed_version,
                revoked_at,
                now,
                record.id,
                record.revision,
                from.as_db(),
            ],
        )?;
        require_single_revision_update(affected)?;
        let updated = read_authorization(&tx, subject, context, plugin_id, capability_id, scope)?
            .ok_or(AppError::PluginAuthorizationNotFound)?;
        tx.commit()?;
        Ok(updated)
    }
}

fn validate_pending_input(input: &PendingPluginCapabilityAuthorization) -> Result<(), AppError> {
    validate_identity(
        &input.subject,
        &input.context,
        &input.plugin_id,
        &input.capability_id,
        &input.scope,
    )?;
    validate_optional_non_empty(
        input.capability_semantic_version.as_deref(),
        AppError::PluginAuthorizationCapabilityInvalid {
            reason: "semantic_version_empty",
        },
    )?;
    validate_optional_non_empty(
        input.last_confirmed_version.as_deref(),
        AppError::PluginAuthorizationTimeInvalid {
            field: "last_confirmed_version",
        },
    )?;
    validate_identity_binding(&input.publisher)?;
    validate_identity_binding(&input.signature)?;
    if let Some(expires_at) = input.expires_at.as_deref() {
        parse_utc(expires_at, "expires_at")?;
    }
    Ok(())
}

fn validate_identity(
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    scope: &PluginAuthorizationScope,
) -> Result<(), AppError> {
    validate_subject(subject)?;
    validate_context(context)?;
    validate_non_empty_plugin_id(plugin_id)?;
    if !V3_MANIFEST_PERMISSIONS.contains(&capability_id) {
        return Err(AppError::PluginAuthorizationCapabilityInvalid {
            reason: "not_admitted_for_v3",
        });
    }
    validate_scope(scope)
}

fn validate_subject(subject: &PluginAuthorizationSubject) -> Result<(), AppError> {
    if subject.id.trim().is_empty() {
        return Err(AppError::PluginAuthorizationSubjectInvalid {
            reason: "subject_id_empty",
        });
    }
    Ok(())
}

fn validate_context(context: &PluginAuthorizationContext) -> Result<(), AppError> {
    if context.id.trim().is_empty() {
        return Err(AppError::PluginAuthorizationContextInvalid {
            reason: "context_id_empty",
        });
    }
    Ok(())
}

fn validate_non_empty_plugin_id(plugin_id: &str) -> Result<(), AppError> {
    if plugin_id.trim().is_empty() {
        return Err(AppError::PluginAuthorizationContextInvalid {
            reason: "plugin_id_empty",
        });
    }
    Ok(())
}

fn validate_non_empty_version(version: &str) -> Result<(), AppError> {
    if version.trim().is_empty() {
        return Err(AppError::PluginAuthorizationTimeInvalid {
            field: "confirmed_version",
        });
    }
    Ok(())
}

fn validate_scope(scope: &PluginAuthorizationScope) -> Result<(), AppError> {
    if scope.kind.trim().is_empty() {
        return Err(AppError::PluginAuthorizationScopeInvalid {
            reason: "scope_kind_empty",
        });
    }
    if scope.key.trim().is_empty() {
        return Err(AppError::PluginAuthorizationScopeInvalid {
            reason: "scope_key_empty",
        });
    }
    Ok(())
}

fn validate_identity_binding(binding: &PluginAuthorizationIdentityBinding) -> Result<(), AppError> {
    match binding.status {
        PluginAuthorizationIdentityBindingStatus::Unavailable => {
            if binding.identity.is_some() {
                return Err(AppError::PluginAuthorizationContextInvalid {
                    reason: "unavailable_identity_present",
                });
            }
        }
        PluginAuthorizationIdentityBindingStatus::Unverified
        | PluginAuthorizationIdentityBindingStatus::Verified => {
            if binding
                .identity
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                return Err(AppError::PluginAuthorizationContextInvalid {
                    reason: "bound_identity_missing",
                });
            }
        }
    }
    Ok(())
}

fn validate_optional_non_empty(value: Option<&str>, error: AppError) -> Result<(), AppError> {
    if value.is_some_and(|item| item.trim().is_empty()) {
        return Err(error);
    }
    Ok(())
}

fn read_authorization(
    conn: &Connection,
    subject: &PluginAuthorizationSubject,
    context: &PluginAuthorizationContext,
    plugin_id: &str,
    capability_id: &str,
    scope: &PluginAuthorizationScope,
) -> Result<Option<PluginCapabilityAuthorization>, AppError> {
    let sql = format!(
        "SELECT {AUTHORIZATION_COLUMNS}
         FROM plugin_capability_authorizations
         WHERE subject_kind = ?1 AND subject_id = ?2
           AND context_kind = ?3 AND context_id = ?4
           AND plugin_id = ?5 AND capability_id = ?6
           AND scope_kind = ?7 AND scope_key = ?8"
    );
    let raw = conn
        .query_row(
            &sql,
            params![
                subject.kind.as_db(),
                subject.id,
                context.kind.as_db(),
                context.id,
                plugin_id,
                capability_id,
                scope.kind,
                scope.key,
            ],
            raw_from_row,
        )
        .optional()?;
    raw.map(parse_raw).transpose()
}

fn raw_from_row(row: &Row<'_>) -> rusqlite::Result<RawAuthorization> {
    Ok(RawAuthorization {
        id: row.get(0)?,
        subject_kind: row.get(1)?,
        subject_id: row.get(2)?,
        context_kind: row.get(3)?,
        context_id: row.get(4)?,
        plugin_id: row.get(5)?,
        capability_id: row.get(6)?,
        capability_semantic_version: row.get(7)?,
        scope_kind: row.get(8)?,
        scope_key: row.get(9)?,
        state: row.get(10)?,
        source: row.get(11)?,
        lifetime: row.get(12)?,
        first_authorized_version: row.get(13)?,
        last_confirmed_version: row.get(14)?,
        publisher_identity: row.get(15)?,
        publisher_binding_status: row.get(16)?,
        signature_identity: row.get(17)?,
        signature_binding_status: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        revoked_at: row.get(21)?,
        expires_at: row.get(22)?,
        revision: row.get(23)?,
    })
}

fn parse_raw(raw: RawAuthorization) -> Result<PluginCapabilityAuthorization, AppError> {
    if raw.subject_id.trim().is_empty()
        || raw.context_id.trim().is_empty()
        || raw.plugin_id.trim().is_empty()
        || raw.capability_id.trim().is_empty()
        || raw.scope_kind.trim().is_empty()
        || raw.scope_key.trim().is_empty()
    {
        return Err(stored_invalid("required_identity"));
    }
    if !V3_MANIFEST_PERMISSIONS.contains(&raw.capability_id.as_str()) {
        return Err(stored_invalid("capability_not_admitted"));
    }
    let state = PluginAuthorizationState::from_db(&raw.state)?;
    validate_stored_authorization_provenance(
        state,
        raw.first_authorized_version.as_deref(),
        raw.last_confirmed_version.as_deref(),
    )?;
    validate_optional_stored(&raw.capability_semantic_version, "semantic_version")?;
    validate_optional_stored(&raw.first_authorized_version, "first_authorized_version")?;
    validate_optional_stored(&raw.last_confirmed_version, "last_confirmed_version")?;
    parse_stored_utc(&raw.created_at, "created_at")?;
    parse_stored_utc(&raw.updated_at, "updated_at")?;
    if let Some(value) = raw.revoked_at.as_deref() {
        parse_stored_utc(value, "revoked_at")?;
    }
    if let Some(value) = raw.expires_at.as_deref() {
        parse_stored_utc(value, "expires_at")?;
    }
    if raw.revision < 0 {
        return Err(stored_invalid("revision"));
    }
    if (state == PluginAuthorizationState::Revoked) != raw.revoked_at.is_some() {
        return Err(stored_invalid("revoked_at"));
    }
    let publisher = parse_stored_binding(
        raw.publisher_identity,
        &raw.publisher_binding_status,
        "publisher_identity",
    )?;
    let signature = parse_stored_binding(
        raw.signature_identity,
        &raw.signature_binding_status,
        "signature_identity",
    )?;
    Ok(PluginCapabilityAuthorization {
        id: raw.id,
        subject: PluginAuthorizationSubject {
            kind: PluginAuthorizationSubjectKind::from_db(&raw.subject_kind)?,
            id: raw.subject_id,
        },
        context: PluginAuthorizationContext {
            kind: PluginAuthorizationContextKind::from_db(&raw.context_kind)?,
            id: raw.context_id,
        },
        plugin_id: raw.plugin_id,
        capability_id: raw.capability_id,
        capability_semantic_version: raw.capability_semantic_version,
        scope: PluginAuthorizationScope {
            kind: raw.scope_kind,
            key: raw.scope_key,
        },
        state,
        source: PluginAuthorizationSource::from_db(&raw.source)?,
        lifetime: PluginAuthorizationLifetime::from_db(&raw.lifetime)?,
        first_authorized_version: raw.first_authorized_version,
        last_confirmed_version: raw.last_confirmed_version,
        publisher,
        signature,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        revoked_at: raw.revoked_at,
        expires_at: raw.expires_at,
        revision: raw.revision,
    })
}

fn parse_stored_binding(
    identity: Option<String>,
    status: &str,
    reason: &'static str,
) -> Result<PluginAuthorizationIdentityBinding, AppError> {
    let status = PluginAuthorizationIdentityBindingStatus::from_db(status)?;
    let valid = match status {
        PluginAuthorizationIdentityBindingStatus::Unavailable => identity.is_none(),
        PluginAuthorizationIdentityBindingStatus::Unverified
        | PluginAuthorizationIdentityBindingStatus::Verified => identity
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
    };
    if !valid {
        return Err(stored_invalid(reason));
    }
    Ok(PluginAuthorizationIdentityBinding { identity, status })
}

fn validate_stored_authorization_provenance(
    state: PluginAuthorizationState,
    first_authorized_version: Option<&str>,
    last_confirmed_version: Option<&str>,
) -> Result<(), AppError> {
    if !matches!(
        state,
        PluginAuthorizationState::Granted | PluginAuthorizationState::Revoked
    ) {
        return Ok(());
    }
    if first_authorized_version.is_none_or(|value| value.trim().is_empty()) {
        return Err(stored_invalid("first_authorized_version_required"));
    }
    if last_confirmed_version.is_none_or(|value| value.trim().is_empty()) {
        return Err(stored_invalid("last_confirmed_version_required"));
    }
    Ok(())
}

fn validate_optional_stored(value: &Option<String>, reason: &'static str) -> Result<(), AppError> {
    if value.as_deref().is_some_and(|item| item.trim().is_empty()) {
        return Err(stored_invalid(reason));
    }
    Ok(())
}

fn require_expected_revision(
    record: &PluginCapabilityAuthorization,
    expected_revision: Option<i64>,
) -> Result<(), AppError> {
    if expected_revision != Some(record.revision) {
        return Err(AppError::PluginAuthorizationRevisionConflict);
    }
    Ok(())
}

fn require_single_revision_update(affected: usize) -> Result<(), AppError> {
    if affected != 1 {
        return Err(AppError::PluginAuthorizationRevisionConflict);
    }
    Ok(())
}

fn transition_invalid(from: PluginAuthorizationState, to: PluginAuthorizationState) -> AppError {
    AppError::PluginAuthorizationTransitionInvalid {
        from: from.as_db(),
        to: to.as_db(),
    }
}

fn stored_invalid(reason: &'static str) -> AppError {
    AppError::PluginAuthorizationStoredRecordInvalid { reason }
}

fn utc_now() -> String {
    format_utc(Utc::now())
}

fn format_utc(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_utc(value: &str, field: &'static str) -> Result<DateTime<Utc>, AppError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| AppError::PluginAuthorizationTimeInvalid { field })?;
    if parsed.offset().local_minus_utc() != 0 {
        return Err(AppError::PluginAuthorizationTimeInvalid { field });
    }
    Ok(parsed.with_timezone(&Utc))
}

fn parse_stored_utc(value: &str, reason: &'static str) -> Result<DateTime<Utc>, AppError> {
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| stored_invalid(reason))?;
    if parsed.offset().local_minus_utc() != 0 {
        return Err(stored_invalid(reason));
    }
    Ok(parsed.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::ErrorCode;

    fn database() -> Database {
        Database::init(":memory:").expect("create migrated in-memory database")
    }

    fn subject(id: &str) -> PluginAuthorizationSubject {
        PluginAuthorizationSubject {
            kind: PluginAuthorizationSubjectKind::PlatformUser,
            id: id.to_string(),
        }
    }

    fn context(id: &str) -> PluginAuthorizationContext {
        PluginAuthorizationContext {
            kind: PluginAuthorizationContextKind::Installation,
            id: id.to_string(),
        }
    }

    fn scope(key: &str) -> PluginAuthorizationScope {
        PluginAuthorizationScope {
            kind: "resource_set".to_string(),
            key: key.to_string(),
        }
    }

    fn unavailable_binding() -> PluginAuthorizationIdentityBinding {
        PluginAuthorizationIdentityBinding {
            identity: None,
            status: PluginAuthorizationIdentityBindingStatus::Unavailable,
        }
    }

    fn pending(
        subject_id: &str,
        context_id: &str,
        plugin_id: &str,
        capability_id: &str,
        scope_key: &str,
    ) -> PendingPluginCapabilityAuthorization {
        PendingPluginCapabilityAuthorization {
            subject: subject(subject_id),
            context: context(context_id),
            plugin_id: plugin_id.to_string(),
            capability_id: capability_id.to_string(),
            capability_semantic_version: None,
            scope: scope(scope_key),
            source: PluginAuthorizationSource::Install,
            lifetime: PluginAuthorizationLifetime::Persistent,
            last_confirmed_version: Some("1.0.0".to_string()),
            publisher: unavailable_binding(),
            signature: unavailable_binding(),
            expires_at: None,
        }
    }

    fn insert_pending(
        db: &Database,
        input: &PendingPluginCapabilityAuthorization,
    ) -> PluginCapabilityAuthorization {
        db.create_or_update_pending_formal_plugin_capability_authorization(input, None)
            .expect("insert pending authorization")
    }

    fn grant_pending(
        db: &Database,
        input: &PendingPluginCapabilityAuthorization,
        revision: i64,
    ) -> PluginCapabilityAuthorization {
        db.grant_pending_formal_plugin_capability_authorization(
            &input.subject,
            &input.context,
            &input.plugin_id,
            &input.capability_id,
            &input.scope,
            "1.0.0",
            revision,
        )
        .expect("grant pending authorization")
    }

    fn read_record(
        db: &Database,
        input: &PendingPluginCapabilityAuthorization,
    ) -> PluginCapabilityAuthorization {
        db.get_formal_plugin_capability_authorization(
            &input.subject,
            &input.context,
            &input.plugin_id,
            &input.capability_id,
            &input.scope,
        )
        .expect("read formal authorization")
        .expect("formal authorization exists")
    }

    fn inject_corrupted_update(db: &Database, id: i64, sql: &str) {
        let conn = db.conn_lock().expect("lock in-memory database");
        conn.pragma_update(None, "ignore_check_constraints", true)
            .expect("temporarily disable CHECK constraints");
        let update_result = conn.execute(sql, [id]);
        let restore_result = conn.pragma_update(None, "ignore_check_constraints", false);
        restore_result.expect("restore CHECK constraints");
        update_result.expect("inject controlled corrupted row");
        let ignored: i64 = conn
            .pragma_query_value(None, "ignore_check_constraints", |row| row.get(0))
            .expect("read CHECK constraint setting");
        assert_eq!(ignored, 0, "CHECK constraints must be restored");
    }

    fn assert_stored_invalid_without_secrets(
        error: AppError,
        expected_reason: &'static str,
        secrets: &[&str],
    ) {
        assert!(matches!(
            error,
            AppError::PluginAuthorizationStoredRecordInvalid { reason }
                if reason == expected_reason
        ));
        let rendered = error.to_string();
        for secret in secrets {
            assert!(
                !rendered.contains(secret),
                "stored-record error leaked sensitive test input"
            );
        }
    }

    fn assert_terminal_record_unchanged(
        db: &Database,
        input: &PendingPluginCapabilityAuthorization,
        before: &PluginCapabilityAuthorization,
    ) {
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(
                input,
                Some(before.revision)
            ),
            Err(AppError::PluginAuthorizationTransitionInvalid { to: "pending", .. })
        ));
        assert_eq!(&read_record(db, input), before);
        assert!(matches!(
            db.grant_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "2.0.0",
                before.revision,
            ),
            Err(AppError::PluginAuthorizationTransitionInvalid { to: "granted", .. })
        ));
        assert_eq!(&read_record(db, input), before);
    }

    #[test]
    fn formal_authorizations_are_isolated_by_subject_context_plugin_capability_and_scope() {
        let db = database();
        let base = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        insert_pending(&db, &base);

        let variants = [
            pending("user-b", "install-a", "plugin-a", "ai.invoke", "scope-a"),
            pending("user-a", "install-b", "plugin-a", "ai.invoke", "scope-a"),
            pending("user-a", "install-a", "plugin-b", "ai.invoke", "scope-a"),
            pending(
                "user-a",
                "install-a",
                "plugin-a",
                "agents.invoke",
                "scope-a",
            ),
            pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-b"),
        ];
        for variant in &variants {
            insert_pending(&db, variant);
        }

        let rows = db
            .list_formal_plugin_capability_authorizations(
                &base.subject,
                &base.context,
                &base.plugin_id,
            )
            .expect("list isolated authorizations");
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|row| row.capability_id == "agents.invoke"));
        assert_eq!(
            rows.iter()
                .filter(|row| row.capability_id == "ai.invoke")
                .count(),
            2
        );
    }

    #[test]
    fn pending_grant_revoke_and_revision_conflict_are_explicit() {
        let db = database();
        let input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        let pending = insert_pending(&db, &input);
        assert_eq!(pending.state, PluginAuthorizationState::Pending);
        assert_eq!(pending.revision, 0);

        let granted = db
            .grant_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "1.0.0",
                pending.revision,
            )
            .expect("grant pending authorization");
        assert_eq!(granted.state, PluginAuthorizationState::Granted);
        assert_eq!(granted.first_authorized_version.as_deref(), Some("1.0.0"));
        assert_eq!(granted.revision, 1);

        assert!(matches!(
            db.revoke_granted_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                0,
            ),
            Err(AppError::PluginAuthorizationRevisionConflict)
        ));

        let revoked = db
            .revoke_granted_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                granted.revision,
            )
            .expect("revoke granted authorization");
        assert_eq!(revoked.state, PluginAuthorizationState::Revoked);
        assert!(revoked.revoked_at.is_some());
        assert_eq!(revoked.revision, 2);

        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(
                &input,
                Some(revoked.revision)
            ),
            Err(AppError::PluginAuthorizationTransitionInvalid {
                from: "revoked",
                to: "pending"
            })
        ));
        assert!(matches!(
            db.grant_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "1.1.0",
                revoked.revision,
            ),
            Err(AppError::PluginAuthorizationTransitionInvalid {
                from: "revoked",
                to: "granted"
            })
        ));
    }

    #[test]
    fn pending_can_be_denied_and_denied_is_not_overwritten() {
        let db = database();
        let input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        let pending = insert_pending(&db, &input);
        let denied = db
            .deny_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                pending.revision,
            )
            .expect("deny pending authorization");
        assert_eq!(denied.state, PluginAuthorizationState::Denied);
        assert!(denied.revoked_at.is_none());
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(
                &input,
                Some(denied.revision)
            ),
            Err(AppError::PluginAuthorizationTransitionInvalid {
                from: "denied",
                to: "pending"
            })
        ));
    }

    #[test]
    fn expiry_is_fail_closed_without_background_state_update() {
        let db = database();
        let mut input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        input.expires_at = Some("2020-01-01T00:00:00Z".to_string());
        let pending = insert_pending(&db, &input);
        let granted = db
            .grant_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "1.0.0",
                pending.revision,
            )
            .expect("grant already time-bounded authorization");
        assert!(!db
            .is_formal_plugin_capability_authorization_effective_at(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "2021-01-01T00:00:00Z",
            )
            .expect("evaluate expiry"));

        let expired = db
            .expire_due_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                granted.revision,
            )
            .expect("persist expired state");
        assert_eq!(expired.state, PluginAuthorizationState::Expired);
        assert!(expired.revoked_at.is_none());
        assert!(!db
            .is_formal_plugin_capability_authorization_effective_at(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "2019-01-01T00:00:00Z",
            )
            .expect("expired state remains ineffective"));
    }

    #[test]
    fn unexpired_grant_is_effective_and_missing_is_not_a_state() {
        let db = database();
        let mut input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        input.expires_at = Some("2099-01-01T00:00:00Z".to_string());
        assert!(db
            .get_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
            )
            .expect("read missing authorization")
            .is_none());
        assert!(!db
            .is_formal_plugin_capability_authorization_effective_at(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "2030-01-01T00:00:00Z",
            )
            .expect("missing must be ineffective"));

        let pending = insert_pending(&db, &input);
        let granted = db
            .grant_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "1.0.0",
                pending.revision,
            )
            .expect("grant authorization");
        assert_eq!(granted.state, PluginAuthorizationState::Granted);
        assert!(db
            .is_formal_plugin_capability_authorization_effective_at(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "2030-01-01T00:00:00Z",
            )
            .expect("unexpired grant must be effective"));
    }

    #[test]
    fn invalid_identity_scope_capability_and_time_are_rejected() {
        let db = database();
        let mut input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");

        input.subject.id.clear();
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationSubjectInvalid { .. })
        ));
        input.subject.id = "user-a".to_string();

        input.context.id.clear();
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationContextInvalid { .. })
        ));
        input.context.id = "install-a".to_string();

        input.scope.kind.clear();
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationScopeInvalid { .. })
        ));
        input.scope.kind = "resource_set".to_string();
        input.scope.key.clear();
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationScopeInvalid { .. })
        ));
        input.scope.key = "scope-a".to_string();

        for capability in ["unknown.capability", "notes:read", "notes.read"] {
            input.capability_id = capability.to_string();
            assert!(matches!(
                db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
                Err(AppError::PluginAuthorizationCapabilityInvalid {
                    reason: "not_admitted_for_v3"
                })
            ));
        }
        input.capability_id = "ai.invoke".to_string();

        input.expires_at = Some("2026-01-01 00:00:00".to_string());
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationTimeInvalid {
                field: "expires_at"
            })
        ));
        input.expires_at = Some("2026-01-01T08:00:00+08:00".to_string());
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationTimeInvalid {
                field: "expires_at"
            })
        ));
    }

    #[test]
    fn identity_binding_status_requires_consistent_snapshot() {
        let db = database();
        let mut input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        input.publisher = PluginAuthorizationIdentityBinding {
            identity: Some("manifest-self-report".to_string()),
            status: PluginAuthorizationIdentityBindingStatus::Unavailable,
        };
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationContextInvalid { .. })
        ));

        input.publisher = PluginAuthorizationIdentityBinding {
            identity: None,
            status: PluginAuthorizationIdentityBindingStatus::Verified,
        };
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(&input, None),
            Err(AppError::PluginAuthorizationContextInvalid { .. })
        ));
    }

    #[test]
    fn database_checks_reject_invalid_enums_and_duplicate_identity() {
        let db = database();
        let input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        let record = insert_pending(&db, &input);
        let conn = db.conn_lock().expect("lock database");
        let enum_error = conn
            .execute(
                "UPDATE plugin_capability_authorizations SET state = 'invalid' WHERE id = ?1",
                [record.id],
            )
            .expect_err("state CHECK must reject invalid value");
        assert_eq!(
            enum_error.sqlite_error_code(),
            Some(ErrorCode::ConstraintViolation)
        );
        let duplicate = conn
            .execute(
                "INSERT INTO plugin_capability_authorizations (
                    subject_kind, subject_id, context_kind, context_id,
                    plugin_id, capability_id, scope_kind, scope_key,
                    state, source, lifetime,
                    publisher_binding_status, signature_binding_status,
                    created_at, updated_at
                 ) SELECT
                    subject_kind, subject_id, context_kind, context_id,
                    plugin_id, capability_id, scope_kind, scope_key,
                    state, source, lifetime,
                    publisher_binding_status, signature_binding_status,
                    created_at, updated_at
                 FROM plugin_capability_authorizations WHERE id = ?1",
                [record.id],
            )
            .expect_err("unique identity must reject duplicate");
        assert_eq!(
            duplicate.sqlite_error_code(),
            Some(ErrorCode::ConstraintViolation)
        );
    }

    #[test]
    fn stored_invalid_time_fails_closed() {
        let db = database();
        let input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        let record = insert_pending(&db, &input);
        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute(
                "UPDATE plugin_capability_authorizations
                 SET created_at = 'not-rfc3339' WHERE id = ?1",
                [record.id],
            )
            .expect("inject corrupted time");
        }
        assert!(matches!(
            db.get_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
            ),
            Err(AppError::PluginAuthorizationStoredRecordInvalid {
                reason: "created_at"
            })
        ));
    }

    #[test]
    fn failed_state_write_rolls_back_without_partial_update() {
        let db = database();
        let input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        let record = insert_pending(&db, &input);
        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute_batch(
                "CREATE TRIGGER reject_formal_authorization_update
                 BEFORE UPDATE ON plugin_capability_authorizations
                 BEGIN
                     SELECT RAISE(ABORT, 'injected formal authorization failure');
                 END;",
            )
            .expect("create failure trigger");
        }
        assert!(db
            .grant_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "1.0.0",
                record.revision,
            )
            .is_err());
        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute_batch("DROP TRIGGER reject_formal_authorization_update;")
                .expect("drop failure trigger");
        }
        let unchanged = db
            .get_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
            )
            .expect("read unchanged record")
            .expect("record remains");
        assert_eq!(unchanged.state, PluginAuthorizationState::Pending);
        assert_eq!(unchanged.revision, 0);
    }

    #[test]
    fn formal_and_legacy_storage_are_strictly_isolated_and_not_cascaded() {
        let db = database();
        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute(
                "INSERT INTO plugins (
                    id, name, version, path, main, manifest_json, enabled, status,
                    content_hash, manifest_format, schema_version, product_type,
                    runtime_kind, source, signature_status
                 ) VALUES (
                    'plugin-a', 'Plugin A', '1.0.0', 'test', 'manifest.json',
                    '{\"id\":\"plugin-a\",\"name\":\"Plugin A\",\"version\":\"1.0.0\",\"main\":\"manifest.json\",\"permissions\":[\"ai.invoke\"]}',
                    0, 'installed', '', 'legacy', 1, 'local-plugin',
                    'legacy-js', 'development', 'unsigned'
                 )",
                [],
            )
            .expect("insert legacy plugin");
            conn.execute(
                "INSERT INTO plugin_permissions(plugin_id, permission, granted)
                 VALUES ('plugin-a', 'ai.invoke', 1)",
                [],
            )
            .expect("insert legacy true");
            conn.execute(
                "INSERT INTO plugin_installations(
                    plugin_id, installed_version, source, enabled, install_path, content_hash
                 ) VALUES ('plugin-a', '1.0.0', 'development', 0, 'test', '')",
                [],
            )
            .expect("insert installation");
        }

        let input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        assert!(db
            .get_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
            )
            .expect("legacy true must not become formal")
            .is_none());
        assert!(!db
            .is_formal_plugin_capability_authorization_effective_at(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "2026-01-01T00:00:00Z",
            )
            .expect("legacy true must not be effective formally"));

        let pending = insert_pending(&db, &input);
        let granted = db
            .grant_pending_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "1.0.0",
                pending.revision,
            )
            .expect("grant formal record");
        assert_eq!(granted.state, PluginAuthorizationState::Granted);
        db.revoke_plugin_permissions("plugin-a", &["ai.invoke".to_string()])
            .expect("legacy revoke");
        assert_eq!(
            db.get_formal_plugin_capability_authorization(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
            )
            .expect("read formal after legacy write")
            .expect("formal remains")
            .state,
            PluginAuthorizationState::Granted
        );

        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute(
                "DELETE FROM plugin_permissions WHERE plugin_id = 'plugin-a'",
                [],
            )
            .expect("delete legacy permissions");
            conn.execute(
                "DELETE FROM plugin_installations WHERE plugin_id = 'plugin-a'",
                [],
            )
            .expect("delete installation");
            conn.execute("DELETE FROM plugins WHERE id = 'plugin-a'", [])
                .expect("delete plugin");
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM plugin_capability_authorizations",
                    [],
                    |row| row.get(0),
                )
                .expect("count formal records");
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn pending_refresh_requires_current_revision_and_is_atomic() {
        let db = database();
        let mut input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "scope-a");
        let inserted = insert_pending(&db, &input);
        assert_eq!(inserted.state, PluginAuthorizationState::Pending);
        assert_eq!(inserted.revision, 0);

        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute(
                "UPDATE plugin_capability_authorizations
                 SET updated_at = '2000-01-01T00:00:00.000Z'
                 WHERE id = ?1",
                [inserted.id],
            )
            .expect("make timestamp update deterministic");
        }
        let before_refresh = read_record(&db, &input);
        input.last_confirmed_version = Some("1.1.0".to_string());
        input.expires_at = Some("2099-01-01T00:00:00Z".to_string());
        let refreshed = db
            .create_or_update_pending_formal_plugin_capability_authorization(
                &input,
                Some(before_refresh.revision),
            )
            .expect("refresh pending authorization");
        assert_eq!(refreshed.state, PluginAuthorizationState::Pending);
        assert_eq!(refreshed.revision, before_refresh.revision + 1);
        assert_ne!(refreshed.updated_at, before_refresh.updated_at);
        assert_eq!(refreshed.last_confirmed_version.as_deref(), Some("1.1.0"));
        assert_eq!(
            refreshed.expires_at.as_deref(),
            Some("2099-01-01T00:00:00Z")
        );

        let before_conflict = refreshed.clone();
        input.last_confirmed_version = Some("sensitive-stale-version".to_string());
        input.expires_at = Some("2088-01-01T00:00:00Z".to_string());
        assert!(matches!(
            db.create_or_update_pending_formal_plugin_capability_authorization(
                &input,
                Some(before_refresh.revision),
            ),
            Err(AppError::PluginAuthorizationRevisionConflict)
        ));
        assert_eq!(read_record(&db, &input), before_conflict);
    }

    #[test]
    fn pending_expiry_requires_due_expiration_and_updates_atomically() {
        let db = database();

        let mut due = pending("user-a", "install-a", "plugin-a", "ai.invoke", "due");
        due.expires_at = Some("2020-01-01T00:00:00Z".to_string());
        let due_record = insert_pending(&db, &due);
        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute(
                "UPDATE plugin_capability_authorizations
                 SET updated_at = '2000-01-01T00:00:00.000Z'
                 WHERE id = ?1",
                [due_record.id],
            )
            .expect("make timestamp update deterministic");
        }
        let due_before = read_record(&db, &due);
        let expired = db
            .expire_due_formal_plugin_capability_authorization(
                &due.subject,
                &due.context,
                &due.plugin_id,
                &due.capability_id,
                &due.scope,
                due_before.revision,
            )
            .expect("expire due pending authorization");
        assert_eq!(expired.state, PluginAuthorizationState::Expired);
        assert_eq!(expired.revision, due_before.revision + 1);
        assert_ne!(expired.updated_at, due_before.updated_at);
        assert!(!db
            .is_formal_plugin_capability_authorization_effective_at(
                &due.subject,
                &due.context,
                &due.plugin_id,
                &due.capability_id,
                &due.scope,
                "2019-01-01T00:00:00Z",
            )
            .expect("expired record is ineffective"));

        let no_expiry = pending("user-a", "install-a", "plugin-a", "ai.invoke", "no-expiry");
        let no_expiry_record = insert_pending(&db, &no_expiry);
        assert!(matches!(
            db.expire_due_formal_plugin_capability_authorization(
                &no_expiry.subject,
                &no_expiry.context,
                &no_expiry.plugin_id,
                &no_expiry.capability_id,
                &no_expiry.scope,
                no_expiry_record.revision,
            ),
            Err(AppError::PluginAuthorizationTimeInvalid {
                field: "expires_at"
            })
        ));
        assert_eq!(read_record(&db, &no_expiry), no_expiry_record);

        let mut future = pending("user-a", "install-a", "plugin-a", "ai.invoke", "future");
        future.expires_at = Some("2099-01-01T00:00:00Z".to_string());
        let future_record = insert_pending(&db, &future);
        assert!(matches!(
            db.expire_due_formal_plugin_capability_authorization(
                &future.subject,
                &future.context,
                &future.plugin_id,
                &future.capability_id,
                &future.scope,
                future_record.revision,
            ),
            Err(AppError::PluginAuthorizationTimeInvalid {
                field: "expires_at_not_due"
            })
        ));
        assert_eq!(read_record(&db, &future), future_record);
    }

    #[test]
    fn terminal_states_are_not_restored_by_ordinary_pending_or_grant_apis() {
        let db = database();

        let denied_input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "denied");
        let denied_pending = insert_pending(&db, &denied_input);
        let denied = db
            .deny_pending_formal_plugin_capability_authorization(
                &denied_input.subject,
                &denied_input.context,
                &denied_input.plugin_id,
                &denied_input.capability_id,
                &denied_input.scope,
                denied_pending.revision,
            )
            .expect("deny pending authorization");
        assert_terminal_record_unchanged(&db, &denied_input, &denied);

        let revoked_input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "revoked");
        let revoked_pending = insert_pending(&db, &revoked_input);
        let granted = grant_pending(&db, &revoked_input, revoked_pending.revision);
        let revoked = db
            .revoke_granted_formal_plugin_capability_authorization(
                &revoked_input.subject,
                &revoked_input.context,
                &revoked_input.plugin_id,
                &revoked_input.capability_id,
                &revoked_input.scope,
                granted.revision,
            )
            .expect("revoke granted authorization");
        assert_terminal_record_unchanged(&db, &revoked_input, &revoked);

        let mut expired_input = pending("user-a", "install-a", "plugin-a", "ai.invoke", "expired");
        expired_input.expires_at = Some("2020-01-01T00:00:00Z".to_string());
        let expired_pending = insert_pending(&db, &expired_input);
        let expired = db
            .expire_due_formal_plugin_capability_authorization(
                &expired_input.subject,
                &expired_input.context,
                &expired_input.plugin_id,
                &expired_input.capability_id,
                &expired_input.scope,
                expired_pending.revision,
            )
            .expect("expire pending authorization");
        assert_terminal_record_unchanged(&db, &expired_input, &expired);
    }

    #[test]
    fn sqlite_checks_reject_invalid_source_lifetime_and_identity_bindings() {
        const INSERT_SQL: &str = "
            INSERT INTO plugin_capability_authorizations (
                subject_kind, subject_id, context_kind, context_id,
                plugin_id, capability_id, scope_kind, scope_key,
                state, source, lifetime,
                publisher_identity, publisher_binding_status,
                signature_identity, signature_binding_status,
                created_at, updated_at
            ) VALUES (
                'platform_user', 'user-a', 'installation', 'install-a',
                'plugin-a', 'ai.invoke', 'resource_set', ?1,
                'pending', ?2, ?3, ?4, ?5, ?6, ?7,
                '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'
            )";
        let db = database();
        let cases = [
            (
                "invalid-source",
                "invalid",
                "persistent",
                None,
                "unavailable",
                None,
                "unavailable",
            ),
            (
                "invalid-lifetime",
                "install",
                "invalid",
                None,
                "unavailable",
                None,
                "unavailable",
            ),
            (
                "invalid-publisher-status",
                "install",
                "persistent",
                None,
                "invalid",
                None,
                "unavailable",
            ),
            (
                "invalid-signature-status",
                "install",
                "persistent",
                None,
                "unavailable",
                None,
                "invalid",
            ),
            (
                "unavailable-publisher-with-identity",
                "install",
                "persistent",
                Some("publisher-secret"),
                "unavailable",
                None,
                "unavailable",
            ),
            (
                "verified-publisher-without-identity",
                "install",
                "persistent",
                None,
                "verified",
                None,
                "unavailable",
            ),
            (
                "unverified-publisher-with-blank-identity",
                "install",
                "persistent",
                Some("   "),
                "unverified",
                None,
                "unavailable",
            ),
            (
                "unavailable-signature-with-identity",
                "install",
                "persistent",
                None,
                "unavailable",
                Some("signature-secret"),
                "unavailable",
            ),
            (
                "verified-signature-without-identity",
                "install",
                "persistent",
                None,
                "unavailable",
                None,
                "verified",
            ),
            (
                "unverified-signature-with-blank-identity",
                "install",
                "persistent",
                None,
                "unavailable",
                Some("   "),
                "unverified",
            ),
        ];
        let conn = db.conn_lock().expect("lock database");
        for (
            scope_key,
            source,
            lifetime,
            publisher_identity,
            publisher_status,
            signature_identity,
            signature_status,
        ) in cases
        {
            let error = conn
                .execute(
                    INSERT_SQL,
                    params![
                        scope_key,
                        source,
                        lifetime,
                        publisher_identity,
                        publisher_status,
                        signature_identity,
                        signature_status,
                    ],
                )
                .expect_err("SQLite CHECK must reject invalid authorization");
            assert_eq!(
                error.sqlite_error_code(),
                Some(ErrorCode::ConstraintViolation)
            );
        }
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM plugin_capability_authorizations",
                [],
                |row| row.get(0),
            )
            .expect("count records after rejected inserts");
        assert_eq!(count, 0, "failed inserts must not leave records");
    }

    #[test]
    fn granted_and_revoked_records_require_complete_provenance() {
        let secrets = [
            "sensitive-user",
            "sensitive-install",
            "sensitive-plugin",
            "sensitive-scope",
        ];
        for (state, sql, reason) in [
            (
                PluginAuthorizationState::Granted,
                "UPDATE plugin_capability_authorizations
                 SET first_authorized_version = NULL WHERE id = ?1",
                "first_authorized_version_required",
            ),
            (
                PluginAuthorizationState::Granted,
                "UPDATE plugin_capability_authorizations
                 SET last_confirmed_version = NULL WHERE id = ?1",
                "last_confirmed_version_required",
            ),
            (
                PluginAuthorizationState::Revoked,
                "UPDATE plugin_capability_authorizations
                 SET first_authorized_version = NULL WHERE id = ?1",
                "first_authorized_version_required",
            ),
            (
                PluginAuthorizationState::Revoked,
                "UPDATE plugin_capability_authorizations
                 SET last_confirmed_version = NULL WHERE id = ?1",
                "last_confirmed_version_required",
            ),
            (
                PluginAuthorizationState::Granted,
                "UPDATE plugin_capability_authorizations
                 SET first_authorized_version = '   ' WHERE id = ?1",
                "first_authorized_version_required",
            ),
            (
                PluginAuthorizationState::Revoked,
                "UPDATE plugin_capability_authorizations
                 SET last_confirmed_version = '   ' WHERE id = ?1",
                "last_confirmed_version_required",
            ),
        ] {
            let db = database();
            let input = pending(secrets[0], secrets[1], secrets[2], "ai.invoke", secrets[3]);
            let pending = insert_pending(&db, &input);
            let granted = grant_pending(&db, &input, pending.revision);
            let record = if state == PluginAuthorizationState::Revoked {
                db.revoke_granted_formal_plugin_capability_authorization(
                    &input.subject,
                    &input.context,
                    &input.plugin_id,
                    &input.capability_id,
                    &input.scope,
                    granted.revision,
                )
                .expect("revoke authorization before corruption")
            } else {
                granted
            };
            inject_corrupted_update(&db, record.id, sql);
            let error = db
                .get_formal_plugin_capability_authorization(
                    &input.subject,
                    &input.context,
                    &input.plugin_id,
                    &input.capability_id,
                    &input.scope,
                )
                .expect_err("missing provenance must fail closed");
            assert_stored_invalid_without_secrets(error, reason, &secrets);
            let effective_error = db
                .is_formal_plugin_capability_authorization_effective_at(
                    &input.subject,
                    &input.context,
                    &input.plugin_id,
                    &input.capability_id,
                    &input.scope,
                    "2030-01-01T00:00:00Z",
                )
                .expect_err("effective read must propagate stored provenance damage");
            assert_stored_invalid_without_secrets(effective_error, reason, &secrets);
        }

        let db = database();
        let input = pending(
            "normal-user",
            "normal-install",
            "normal-plugin",
            "ai.invoke",
            "normal-scope",
        );
        let pending = insert_pending(&db, &input);
        let granted = grant_pending(&db, &input, pending.revision);
        assert_eq!(read_record(&db, &input), granted);
        assert!(db
            .is_formal_plugin_capability_authorization_effective_at(
                &input.subject,
                &input.context,
                &input.plugin_id,
                &input.capability_id,
                &input.scope,
                "2030-01-01T00:00:00Z",
            )
            .expect("normal granted authorization remains effective"));
    }

    #[test]
    fn corrupted_stored_enums_bindings_and_scope_fail_closed() {
        let cases = [
            (
                "UPDATE plugin_capability_authorizations SET state = 'sensitive-invalid-state'
                 WHERE id = ?1",
                "state",
            ),
            (
                "UPDATE plugin_capability_authorizations SET source = 'sensitive-invalid-source'
                 WHERE id = ?1",
                "source",
            ),
            (
                "UPDATE plugin_capability_authorizations SET lifetime = 'sensitive-invalid-lifetime'
                 WHERE id = ?1",
                "lifetime",
            ),
            (
                "UPDATE plugin_capability_authorizations
                 SET publisher_binding_status = 'sensitive-invalid-binding' WHERE id = ?1",
                "identity_binding_status",
            ),
            (
                "UPDATE plugin_capability_authorizations
                 SET signature_binding_status = 'sensitive-invalid-binding' WHERE id = ?1",
                "identity_binding_status",
            ),
            (
                "UPDATE plugin_capability_authorizations
                 SET publisher_binding_status = 'verified', publisher_identity = NULL
                 WHERE id = ?1",
                "publisher_identity",
            ),
            (
                "UPDATE plugin_capability_authorizations
                 SET signature_binding_status = 'verified', signature_identity = NULL
                 WHERE id = ?1",
                "signature_identity",
            ),
            (
                "UPDATE plugin_capability_authorizations SET scope_kind = '' WHERE id = ?1",
                "required_identity",
            ),
            (
                "UPDATE plugin_capability_authorizations SET scope_key = '' WHERE id = ?1",
                "required_identity",
            ),
        ];
        for (sql, reason) in cases {
            let db = database();
            let input = pending(
                "sensitive-user",
                "sensitive-install",
                "sensitive-plugin",
                "ai.invoke",
                "sensitive-scope",
            );
            let pending = insert_pending(&db, &input);
            let granted = grant_pending(&db, &input, pending.revision);
            inject_corrupted_update(&db, granted.id, sql);
            let error = db
                .list_formal_plugin_capability_authorizations(
                    &input.subject,
                    &input.context,
                    &input.plugin_id,
                )
                .expect_err("corrupted stored record must fail closed");
            assert_stored_invalid_without_secrets(
                error,
                reason,
                &[
                    "sensitive-user",
                    "sensitive-install",
                    "sensitive-plugin",
                    "sensitive-scope",
                    "sensitive-invalid",
                ],
            );
        }
    }
}
