//! 后端可信资源解析的类型边界。
//!
//! 本模块只定义不可信引用、可信解析结果和统一失败语义。具体资源 resolver
//! 必须作为本模块的子模块实现，才能使用私有构造能力。

use std::fmt;

use super::resource_ownership::ResourceOwner;

pub(crate) mod agent_children;
pub(crate) mod credential;
pub(crate) mod external_agent;

const MAX_RESOURCE_ID_BYTES: usize = 512;
const NOT_FOUND_OR_INACCESSIBLE_MESSAGE: &str = "资源不存在或不可访问";

/// 后端识别的资源种类；种类本身不表示资源已被解析或授权。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ResourceKind {
    Credential,
    ExternalAgent,
    Workflow,
    AgentSession,
    AgentMessage,
    AgentRequest,
}

impl ResourceKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Credential => "credential",
            Self::ExternalAgent => "external-agent",
            Self::Workflow => "workflow",
            Self::AgentSession => "agent-session",
            Self::AgentMessage => "agent-message",
            Self::AgentRequest => "agent-request",
        }
    }

    fn parse(raw: &str) -> Result<Self, ResolverError> {
        match raw {
            "credential" => Ok(Self::Credential),
            "external-agent" => Ok(Self::ExternalAgent),
            "workflow" => Ok(Self::Workflow),
            "agent-session" => Ok(Self::AgentSession),
            "agent-message" => Ok(Self::AgentMessage),
            "agent-request" => Ok(Self::AgentRequest),
            _ => Err(ResolverError::unsupported_kind()),
        }
    }
}

/// 来自 IPC 或普通业务输入的资源引用；它不携带 owner、capability 或授权含义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UntrustedResourceRef {
    kind: ResourceKind,
    raw_id: String,
}

impl UntrustedResourceRef {
    pub(crate) fn try_new(kind: &str, raw_id: impl Into<String>) -> Result<Self, ResolverError> {
        let kind = ResourceKind::parse(kind)?;
        let raw_id = raw_id.into();
        validate_raw_resource_id(&raw_id)?;
        Ok(Self { kind, raw_id })
    }

    pub(crate) fn kind(&self) -> ResourceKind {
        self.kind
    }

    pub(crate) fn raw_id(&self) -> &str {
        &self.raw_id
    }
}

fn validate_raw_resource_id(raw_id: &str) -> Result<(), ResolverError> {
    if raw_id.is_empty() || raw_id.trim().is_empty() {
        return Err(ResolverError::malformed("resource_id_empty"));
    }
    if raw_id.trim() != raw_id {
        return Err(ResolverError::malformed("resource_id_not_canonical"));
    }
    if raw_id.len() > MAX_RESOURCE_ID_BYTES {
        return Err(ResolverError::malformed("resource_id_too_long"));
    }
    if raw_id.chars().any(char::is_control) {
        return Err(ResolverError::malformed("resource_id_contains_control"));
    }
    Ok(())
}

/// 只能由本模块内的具体 resolver 在完成权威查询后构造的资源身份。
///
/// 该类型不表示 capability 或 exact resource authorization 已通过。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TrustedResource {
    kind: ResourceKind,
    resource_id: String,
    owner: ResourceOwner,
}

impl TrustedResource {
    fn from_resolved(reference: UntrustedResourceRef, owner: ResourceOwner) -> Self {
        Self {
            kind: reference.kind,
            resource_id: reference.raw_id,
            owner,
        }
    }

    pub(crate) fn kind(&self) -> ResourceKind {
        self.kind
    }

    pub(crate) fn resource_id(&self) -> &str {
        &self.resource_id
    }

    pub(crate) fn owner(&self) -> &ResourceOwner {
        &self.owner
    }
}

impl fmt::Debug for TrustedResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustedResource")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolverFailure {
    MalformedReference,
    UnsupportedResourceKind,
    NotFoundOrInaccessible,
    OwnershipUnprovable,
    BackendFailure,
    InvalidResourceState,
}

/// Resolver 的内部失败分类。诊断码只能是静态字符串，不得携带资源 ID 或敏感内容。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ResolverError {
    failure: ResolverFailure,
    diagnostic: &'static str,
}

impl ResolverError {
    fn malformed(diagnostic: &'static str) -> Self {
        Self {
            failure: ResolverFailure::MalformedReference,
            diagnostic,
        }
    }

    fn unsupported_kind() -> Self {
        Self {
            failure: ResolverFailure::UnsupportedResourceKind,
            diagnostic: "resource_kind_unsupported",
        }
    }

    fn not_found_or_inaccessible() -> Self {
        Self {
            failure: ResolverFailure::NotFoundOrInaccessible,
            diagnostic: "resource_not_found_or_inaccessible",
        }
    }

    fn ownership_unprovable(diagnostic: &'static str) -> Self {
        Self {
            failure: ResolverFailure::OwnershipUnprovable,
            diagnostic,
        }
    }

    fn backend_failure(diagnostic: &'static str) -> Self {
        Self {
            failure: ResolverFailure::BackendFailure,
            diagnostic,
        }
    }

    fn invalid_state(diagnostic: &'static str) -> Self {
        Self {
            failure: ResolverFailure::InvalidResourceState,
            diagnostic,
        }
    }

    pub(crate) fn public_message(&self) -> &'static str {
        match self.failure {
            ResolverFailure::MalformedReference => "资源引用无效",
            ResolverFailure::UnsupportedResourceKind => "不支持的资源类型",
            ResolverFailure::NotFoundOrInaccessible
            | ResolverFailure::OwnershipUnprovable
            | ResolverFailure::InvalidResourceState => NOT_FOUND_OR_INACCESSIBLE_MESSAGE,
            ResolverFailure::BackendFailure => "资源解析暂不可用",
        }
    }

    pub(crate) fn diagnostic_code(&self) -> &'static str {
        self.diagnostic
    }
}

impl fmt::Debug for ResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolverError")
            .field("failure", &self.failure)
            .field("diagnostic", &self.diagnostic)
            .finish()
    }
}

impl fmt::Display for ResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.public_message())
    }
}

impl std::error::Error for ResolverError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_reference_is_strict_and_has_no_security_context() {
        let reference = UntrustedResourceRef::try_new("credential", "cred-123").unwrap();
        assert_eq!(reference.kind(), ResourceKind::Credential);
        assert_eq!(reference.raw_id(), "cred-123");

        for (kind, id) in [
            ("unknown", "resource-1"),
            ("Credential", "resource-1"),
            ("credential", ""),
            ("credential", "  "),
            ("credential", " resource-1"),
            ("credential", "resource-1 "),
            ("credential", "resource\n1"),
        ] {
            assert!(UntrustedResourceRef::try_new(kind, id).is_err());
        }
    }

    #[test]
    fn resource_kinds_are_stable_and_never_silently_interchanged() {
        let cases = [
            ("credential", ResourceKind::Credential),
            ("external-agent", ResourceKind::ExternalAgent),
            ("workflow", ResourceKind::Workflow),
            ("agent-session", ResourceKind::AgentSession),
            ("agent-message", ResourceKind::AgentMessage),
            ("agent-request", ResourceKind::AgentRequest),
        ];
        for (raw, expected) in cases {
            let reference = UntrustedResourceRef::try_new(raw, "same-id").unwrap();
            assert_eq!(reference.kind(), expected);
            assert_eq!(reference.kind().as_str(), raw);
        }
    }

    #[test]
    fn trusted_resource_retains_kind_id_and_verified_owner_without_debug_leak() {
        let owner = ResourceOwner::fixture("subject-secret", "installation-secret");
        let reference = UntrustedResourceRef::try_new("external-agent", "agent-secret").unwrap();
        let trusted = TrustedResource::from_resolved(reference, owner);

        assert_eq!(trusted.kind(), ResourceKind::ExternalAgent);
        assert_eq!(trusted.resource_id(), "agent-secret");
        assert_eq!(trusted.owner().platform_subject_id(), "subject-secret");
        assert_eq!(
            trusted.owner().host_installation_id(),
            "installation-secret"
        );
        let debug = format!("{trusted:?}");
        for sensitive in ["agent-secret", "subject-secret", "installation-secret"] {
            assert!(!debug.contains(sensitive));
        }
    }

    #[test]
    fn inaccessible_ownership_and_state_share_one_external_semantics() {
        let cases = [
            ResolverError::not_found_or_inaccessible(),
            ResolverError::ownership_unprovable("owner_missing"),
            ResolverError::ownership_unprovable("subject_mismatch"),
            ResolverError::ownership_unprovable("installation_mismatch"),
            ResolverError::ownership_unprovable("legacy_unowned"),
            ResolverError::invalid_state("resource_disabled"),
        ];
        for error in cases {
            assert_eq!(error.public_message(), NOT_FOUND_OR_INACCESSIBLE_MESSAGE);
            assert!(!error.to_string().contains(error.diagnostic_code()));
        }
    }

    #[test]
    fn backend_failure_is_fail_closed_and_diagnostics_cannot_carry_runtime_data() {
        let error = ResolverError::backend_failure("storage_read_failed");
        assert_eq!(error.public_message(), "资源解析暂不可用");
        assert_eq!(error.diagnostic_code(), "storage_read_failed");
        assert!(!error.to_string().contains("storage_read_failed"));
    }

    #[test]
    fn trusted_types_expose_no_generic_construction_traits() {
        fn assert_clone<T: Clone>() {}
        fn assert_error<T: std::error::Error>() {}

        assert_clone::<TrustedResource>();
        assert_error::<ResolverError>();
        // 编译边界由类型定义证明：TrustedResource 无公开字段、无 pub(crate) 构造器，
        // 且未实现 Default、From<String>、Serialize 或 Deserialize。
    }
}
