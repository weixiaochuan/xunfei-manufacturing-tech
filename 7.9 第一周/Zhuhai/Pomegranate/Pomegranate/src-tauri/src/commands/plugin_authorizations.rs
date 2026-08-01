use serde::{Deserialize, Serialize};
use tauri::State;

use crate::account::AccountState;
use crate::models::{CurrentPluginCapabilityAuthorization, PluginCapabilityAuthorization};
use crate::services::plugin_authorizations;
use crate::services::plugin_exact_authorizations;
use crate::services::plugin_feature_authorizations::{self, FeatureAuthorizationAction};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeatureAuthorizationRequest {
    pub plugin_id: String,
    pub contribution_id: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeatureAuthorizationTargetRequest {
    pub plugin_id: String,
    pub contribution_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeatureAuthorizationListRequest {
    pub plugin_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureAuthorizationResponse {
    pub contribution_id: String,
    pub title: String,
    pub hook: String,
    pub scenes: Vec<String>,
    pub features: Vec<String>,
    pub status: crate::models::CurrentPluginCapabilityAuthorizationStatus,
    pub effective: bool,
    pub expires_at: Option<String>,
}

impl From<plugin_feature_authorizations::FeatureAuthorizationView>
    for FeatureAuthorizationResponse
{
    fn from(value: plugin_feature_authorizations::FeatureAuthorizationView) -> Self {
        Self {
            contribution_id: value.contribution_id,
            title: value.title,
            hook: value.hook,
            scenes: value.scenes,
            features: value.features,
            status: value.authorization.status,
            effective: value.authorization.effective,
            expires_at: value.authorization.expires_at,
        }
    }
}

macro_rules! feature_mutation_command {
    ($name:ident, $action:expr) => {
        #[tauri::command]
        pub async fn $name(
            state: State<'_, AppState>,
            account: State<'_, AccountState>,
            request: FeatureAuthorizationRequest,
        ) -> Result<FeatureAuthorizationResponse, String> {
            plugin_feature_authorizations::mutate_feature_authorization(
                &state.db,
                &account,
                &request.plugin_id,
                &request.contribution_id,
                $action,
                request.expires_at,
            )
            .await
            .map(Into::into)
            .map_err(Into::into)
        }
    };
}

feature_mutation_command!(
    request_declarative_feature_authorization,
    FeatureAuthorizationAction::Request
);
feature_mutation_command!(
    grant_declarative_feature_authorization,
    FeatureAuthorizationAction::Grant
);
feature_mutation_command!(
    deny_declarative_feature_authorization,
    FeatureAuthorizationAction::Deny
);
feature_mutation_command!(
    revoke_declarative_feature_authorization,
    FeatureAuthorizationAction::Revoke
);
feature_mutation_command!(
    expire_declarative_feature_authorization,
    FeatureAuthorizationAction::Expire
);

#[tauri::command]
pub async fn query_declarative_feature_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request: FeatureAuthorizationTargetRequest,
) -> Result<FeatureAuthorizationResponse, String> {
    plugin_feature_authorizations::query_feature_authorization(
        &state.db,
        &account,
        &request.plugin_id,
        &request.contribution_id,
    )
    .await
    .map(Into::into)
    .map_err(Into::into)
}

#[tauri::command]
pub async fn list_declarative_feature_authorizations(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request: FeatureAuthorizationListRequest,
) -> Result<Vec<FeatureAuthorizationResponse>, String> {
    plugin_feature_authorizations::list_feature_authorizations(
        &state.db,
        &account,
        &request.plugin_id,
    )
    .await
    .map(|items| items.into_iter().map(Into::into).collect())
    .map_err(Into::into)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactResourceAuthorizationRequest {
    pub plugin_id: String,
    pub capability_id: String,
    pub resource_kind: String,
    pub resource_id: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactResourceAuthorizationQuery {
    pub plugin_id: String,
    pub capability_id: String,
    pub resource_kind: String,
    pub resource_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactResourceAuthorizationListRequest {
    pub plugin_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExactResourceAuthorizationRevokeRequest {
    pub plugin_id: String,
    pub authorization_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactResourceAuthorizationResponse {
    pub authorization_id: Option<String>,
    pub plugin_id: String,
    pub capability_id: String,
    pub resource_kind: String,
    pub status: crate::models::CurrentPluginCapabilityAuthorizationStatus,
    pub effective: bool,
    pub available: Option<bool>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactAuthorizationResourceOptionResponse {
    pub resource_kind: String,
    pub resource_id: String,
    pub display_name: String,
    pub compatible_capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactAuthorizationCatalogResponse {
    pub capability_ids: Vec<String>,
    pub resources: Vec<ExactAuthorizationResourceOptionResponse>,
    pub max_duration_hours: i64,
}

impl From<plugin_exact_authorizations::ExactAuthorizationView>
    for ExactResourceAuthorizationResponse
{
    fn from(value: plugin_exact_authorizations::ExactAuthorizationView) -> Self {
        Self {
            authorization_id: value.authorization_id,
            plugin_id: value.plugin_id,
            capability_id: value.capability_id,
            resource_kind: value.resource_kind,
            status: value.status,
            effective: value.effective,
            available: value.available,
            expires_at: value.expires_at,
        }
    }
}

impl From<plugin_exact_authorizations::ExactAuthorizationCatalog>
    for ExactAuthorizationCatalogResponse
{
    fn from(value: plugin_exact_authorizations::ExactAuthorizationCatalog) -> Self {
        Self {
            capability_ids: value.capability_ids,
            resources: value
                .resources
                .into_iter()
                .map(|resource| ExactAuthorizationResourceOptionResponse {
                    resource_kind: resource.resource_kind,
                    resource_id: resource.resource_id,
                    display_name: resource.display_name,
                    compatible_capabilities: resource.compatible_capabilities,
                })
                .collect(),
            max_duration_hours: value.max_duration_hours,
        }
    }
}

#[tauri::command]
pub async fn list_exact_authorization_catalog(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request: ExactResourceAuthorizationListRequest,
) -> Result<ExactAuthorizationCatalogResponse, String> {
    plugin_exact_authorizations::list_exact_authorization_catalog(
        &state.db,
        &account,
        &request.plugin_id,
    )
    .await
    .map(Into::into)
    .map_err(Into::into)
}

#[tauri::command]
pub async fn grant_exact_resource_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request: ExactResourceAuthorizationRequest,
) -> Result<ExactResourceAuthorizationResponse, String> {
    plugin_exact_authorizations::grant_exact_resource_authorization(
        &state.db,
        &account,
        &request.plugin_id,
        &request.capability_id,
        &request.resource_kind,
        &request.resource_id,
        request.expires_at,
    )
    .await
    .map(Into::into)
    .map_err(Into::into)
}

#[tauri::command]
pub async fn query_exact_resource_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request: ExactResourceAuthorizationQuery,
) -> Result<ExactResourceAuthorizationResponse, String> {
    plugin_exact_authorizations::query_exact_resource_authorization(
        &state.db,
        &account,
        &request.plugin_id,
        &request.capability_id,
        &request.resource_kind,
        &request.resource_id,
    )
    .await
    .map(Into::into)
    .map_err(Into::into)
}

#[tauri::command]
pub async fn list_exact_resource_authorizations(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request: ExactResourceAuthorizationListRequest,
) -> Result<Vec<ExactResourceAuthorizationResponse>, String> {
    plugin_exact_authorizations::list_exact_resource_authorizations(
        &state.db,
        &account,
        &request.plugin_id,
    )
    .await
    .map(|items| items.into_iter().map(Into::into).collect())
    .map_err(Into::into)
}

#[tauri::command]
pub async fn revoke_exact_resource_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    request: ExactResourceAuthorizationRevokeRequest,
) -> Result<ExactResourceAuthorizationResponse, String> {
    plugin_exact_authorizations::revoke_exact_resource_authorization(
        &state.db,
        &account,
        &request.plugin_id,
        &request.authorization_id,
    )
    .await
    .map(Into::into)
    .map_err(Into::into)
}

/// 查询当前可信账号与宿主安装上下文下的正式 capability 授权事实。
#[tauri::command]
pub async fn list_current_formal_plugin_capability_authorizations(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
) -> Result<Vec<CurrentPluginCapabilityAuthorization>, String> {
    plugin_authorizations::list_current_formal_plugin_capability_authorizations(
        &state.db, &account, &plugin_id,
    )
    .await
    .map_err(Into::into)
}

/// 创建 pending 授权请求；主体、context、scope 和 semanticVersion 均由后端确定。
#[tauri::command]
pub async fn request_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::request_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
        expires_at,
    )
    .await
    .map_err(Into::into)
}

/// 明确同意当前 capability。
#[tauri::command]
pub async fn grant_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
    expires_at: Option<String>,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::grant_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
        expires_at,
    )
    .await
    .map_err(Into::into)
}

/// 明确拒绝当前 capability。
#[tauri::command]
pub async fn deny_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::deny_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
    )
    .await
    .map_err(Into::into)
}

/// 撤销当前 capability 的 granted 记录，不删除历史事实。
#[tauri::command]
pub async fn revoke_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::revoke_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
    )
    .await
    .map_err(Into::into)
}

/// 显式写回已经到期的 pending/granted 记录。
#[tauri::command]
pub async fn expire_current_formal_plugin_capability_authorization(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    plugin_id: String,
    capability_id: String,
) -> Result<PluginCapabilityAuthorization, String> {
    plugin_authorizations::expire_current_formal_plugin_capability_authorization(
        &state.db,
        &account,
        &plugin_id,
        &capability_id,
    )
    .await
    .map_err(Into::into)
}

#[cfg(test)]
mod exact_resource_authorization_dto_tests {
    use super::*;

    #[test]
    fn grant_dto_accepts_only_minimal_untrusted_intent() {
        let valid = serde_json::json!({
            "pluginId": "plugin-a",
            "capabilityId": "credentials.use",
            "resourceKind": "credential",
            "resourceId": "credential-a"
        });
        assert!(serde_json::from_value::<ExactResourceAuthorizationRequest>(valid).is_ok());
        for forbidden in [
            "subject",
            "installation",
            "owner",
            "scopeKind",
            "scopeKey",
            "canonicalHash",
            "pluginInstallationId",
            "pluginVersion",
            "parentAgentId",
            "granted",
            "createdAt",
        ] {
            let mut value = serde_json::json!({
                "pluginId": "plugin-a",
                "capabilityId": "credentials.use",
                "resourceKind": "credential",
                "resourceId": "credential-a"
            });
            value[forbidden] = serde_json::json!("forged");
            assert!(
                serde_json::from_value::<ExactResourceAuthorizationRequest>(value).is_err(),
                "forbidden field {forbidden} must be rejected"
            );
        }
    }

    #[test]
    fn query_list_and_revoke_dtos_reject_unknown_fields() {
        assert!(
            serde_json::from_value::<ExactResourceAuthorizationQuery>(serde_json::json!({
                "pluginId": "plugin-a",
                "capabilityId": "agents.invoke",
                "resourceKind": "external-agent",
                "resourceId": "agent-a",
                "scopeKey": "forged"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ExactResourceAuthorizationListRequest>(
                serde_json::json!({"pluginId": "plugin-a", "subject": "forged"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<ExactResourceAuthorizationRevokeRequest>(serde_json::json!({
                "pluginId": "plugin-a",
                "authorizationId": "exact-auth-v1:1",
                "scopeKey": "forged"
            }))
            .is_err()
        );
    }

    #[test]
    fn feature_dtos_reject_frontend_scope_and_selector_expansion() {
        let valid = serde_json::json!({
            "pluginId": "plugin-a",
            "contributionId": "enhancement-a",
            "expiresAt": "2099-01-01T00:00:00Z"
        });
        assert!(serde_json::from_value::<FeatureAuthorizationRequest>(valid).is_ok());
        for forbidden in [
            "capabilityId",
            "scene",
            "feature",
            "hook",
            "scope",
            "scopeKind",
            "scopeKey",
            "subject",
            "installation",
            "owner",
        ] {
            let mut value = serde_json::json!({
                "pluginId": "plugin-a",
                "contributionId": "enhancement-a"
            });
            value[forbidden] = serde_json::json!("forged");
            assert!(
                serde_json::from_value::<FeatureAuthorizationRequest>(value).is_err(),
                "forbidden field {forbidden} must be rejected"
            );
        }
    }
}
