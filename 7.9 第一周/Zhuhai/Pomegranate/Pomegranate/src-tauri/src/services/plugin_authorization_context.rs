use crate::account::{verified_platform_user_id, AccountState};
use crate::database::Database;
use crate::error::AppError;
use crate::models::plugin_platform::{
    PluginAuthorizationContext, PluginAuthorizationScope, PluginAuthorizationSubject,
    PluginAuthorizationSubjectKind,
};
use crate::services::plugin_capabilities::{
    canonical_capability_semantic_version, VALID_PERMISSIONS,
};

const GLOBAL_SCOPE_KIND: &str = "global";
const GLOBAL_SCOPE_KEY: &str = "v1:*";

/// 解析后端验证成功的账号主体；该接口不接收前端 user_id，也不返回账号 token。
pub(crate) async fn resolve_verified_platform_subject(
    account: &AccountState,
) -> Result<PluginAuthorizationSubject, AppError> {
    let id = verified_platform_user_id(account).await?;
    Ok(PluginAuthorizationSubject {
        kind: PluginAuthorizationSubjectKind::PlatformUser,
        id,
    })
}

/// 取得由 Rust 后端持久化的宿主安装上下文。
pub(crate) fn resolve_host_installation_context(
    db: &Database,
) -> Result<PluginAuthorizationContext, AppError> {
    db.stable_host_installation_context()
}

/// 首版只接受精确 canonical global scope，不接受自由文本或资源级 scope。
pub(crate) fn canonicalize_authorization_scope(
    kind: &str,
    key: &str,
) -> Result<PluginAuthorizationScope, AppError> {
    if kind != GLOBAL_SCOPE_KIND {
        return Err(AppError::PluginAuthorizationScopeInvalid {
            reason: "scope_kind_not_supported",
        });
    }
    if key != GLOBAL_SCOPE_KEY {
        return Err(AppError::PluginAuthorizationScopeInvalid {
            reason: "scope_key_not_canonical",
        });
    }
    Ok(PluginAuthorizationScope {
        kind: GLOBAL_SCOPE_KIND.to_string(),
        key: GLOBAL_SCOPE_KEY.to_string(),
    })
}

/// 仅从 A1 canonical registry 读取 capability 语义版本。
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
    fn global_scope_has_one_canonical_identity() {
        assert_eq!(
            canonicalize_authorization_scope("global", "v1:*").expect("scope"),
            PluginAuthorizationScope {
                kind: "global".to_string(),
                key: "v1:*".to_string(),
            }
        );
    }

    #[test]
    fn scope_rejects_unknown_empty_and_free_text_values() {
        for (kind, key) in [
            ("", "v1:*"),
            ("GLOBAL", "v1:*"),
            ("file", "C:\\secret"),
            ("global", ""),
            ("global", "anything"),
        ] {
            assert!(matches!(
                canonicalize_authorization_scope(kind, key),
                Err(AppError::PluginAuthorizationScopeInvalid { .. })
            ));
        }
    }

    #[test]
    fn semantic_version_comes_from_canonical_registry() {
        assert_eq!(
            resolve_capability_semantic_version("ai.invoke").expect("version"),
            "1.0.0"
        );
        assert!(matches!(
            resolve_capability_semantic_version("unknown.capability"),
            Err(AppError::PluginAuthorizationCapabilityInvalid {
                reason: "capability_not_admitted"
            })
        ));
    }
}
