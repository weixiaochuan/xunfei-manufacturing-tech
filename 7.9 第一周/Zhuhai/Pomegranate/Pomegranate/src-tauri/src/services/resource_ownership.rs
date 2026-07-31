use std::fmt;

use crate::account::{verified_platform_user_id, AccountState};
use crate::database::Database;
use crate::error::AppError;

/// Rust 后端验证过的本地资源所有者。字段不跨 IPC，也不在 Debug 中暴露身份值。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ResourceOwner {
    platform_subject_id: String,
    host_installation_id: String,
}

impl fmt::Debug for ResourceOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceOwner")
            .finish_non_exhaustive()
    }
}

impl ResourceOwner {
    pub(crate) fn platform_subject_id(&self) -> &str {
        &self.platform_subject_id
    }

    pub(crate) fn host_installation_id(&self) -> &str {
        &self.host_installation_id
    }

    #[cfg(test)]
    pub(crate) fn fixture(platform_subject_id: &str, host_installation_id: &str) -> Self {
        Self {
            platform_subject_id: platform_subject_id.to_string(),
            host_installation_id: host_installation_id.to_string(),
        }
    }
}

/// 同时解析远端验证账号和本机稳定安装身份；任一事实不可用时默认拒绝。
pub(crate) async fn resolve_resource_owner(
    db: &Database,
    account: &AccountState,
) -> Result<ResourceOwner, AppError> {
    let platform_subject_id = verified_platform_user_id(account).await?;
    let host = db.stable_host_installation_context()?;
    Ok(ResourceOwner {
        platform_subject_id,
        host_installation_id: host.id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_exposes_subject_or_installation() {
        let owner = ResourceOwner::fixture("platform-subject-secret", "installation-secret");
        let rendered = format!("{owner:?}");
        assert_eq!(rendered, "ResourceOwner { .. }");
        assert!(!rendered.contains("platform-subject-secret"));
        assert!(!rendered.contains("installation-secret"));
    }
}
