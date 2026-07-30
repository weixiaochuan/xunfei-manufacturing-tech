//! Manifest v3 插件运行时 capability 的统一只读门禁。

use crate::database::Database;
use crate::error::AppError;
use crate::models::PluginManifestV3;

/// 已通过最小生命周期和 capability 授权检查的当前版本上下文。
///
/// 该上下文不代表完整性、scene、feature、runtime、classification、输入输出或
/// Marketplace entitlement 已通过；调用方仍须执行对应的业务校验。
#[derive(Debug, Clone)]
pub(crate) struct AuthorizedPluginContext {
    pub manifest: PluginManifestV3,
    pub version: String,
    pub install_path: String,
}

/// 只读解析已安装并启用的当前 Manifest 上下文，不授予或检查任何 capability。
///
/// 该入口供只需要读取当前声明元数据的宿主路径使用；需要执行 capability 的调用方
/// 仍必须使用 `require_current_plugin_capabilities`。
pub(crate) fn resolve_current_plugin_context(
    db: &Database,
    plugin_id: &str,
) -> Result<AuthorizedPluginContext, AppError> {
    validate_plugin_id(plugin_id)?;
    let (context, _) = resolve_current_plugin_context_and_grants(db, plugin_id, &[])?;
    Ok(context)
}

/// 要求当前 Manifest 声明且用户真实授予全部 capability。
///
/// 此函数只读、全有或全无，并且不记录日志或缓存授权结果。
pub(crate) fn require_current_plugin_capabilities(
    db: &Database,
    plugin_id: &str,
    capabilities: &[&str],
) -> Result<AuthorizedPluginContext, AppError> {
    validate_plugin_id(plugin_id)?;
    validate_capabilities(capabilities)?;

    let (context, grant_states) =
        resolve_current_plugin_context_and_grants(db, plugin_id, capabilities)?;
    for capability in capabilities {
        if !context
            .manifest
            .permissions
            .iter()
            .any(|permission| permission == capability)
        {
            return Err(AppError::PluginCapabilityNotDeclared {
                plugin_id: plugin_id.to_string(),
                capability: (*capability).to_string(),
            });
        }
    }
    for capability in capabilities {
        let granted = grant_states
            .iter()
            .find(|(permission, _)| permission == capability)
            .and_then(|(_, state)| *state)
            .unwrap_or(false);
        if !granted {
            return Err(AppError::PluginPermissionDenied {
                plugin_id: Some(plugin_id.to_string()),
                required_permission: Some((*capability).to_string()),
            });
        }
    }

    Ok(context)
}

fn resolve_current_plugin_context_and_grants(
    db: &Database,
    plugin_id: &str,
    capabilities: &[&str],
) -> Result<(AuthorizedPluginContext, Vec<(String, Option<bool>)>), AppError> {
    let snapshot = db
        .current_plugin_authorization_snapshot(plugin_id, capabilities)?
        .ok_or_else(|| AppError::NotFound(format!("未找到插件 {}", plugin_id)))?;
    if snapshot.status != "installed" {
        return Err(AppError::InvalidInput(format!(
            "插件 {} 当前状态不允许调用",
            plugin_id
        )));
    }
    if !snapshot.enabled {
        return Err(AppError::InvalidInput(format!(
            "插件 {} 已禁用，不能调用",
            plugin_id
        )));
    }
    let current = snapshot
        .current_version
        .ok_or_else(|| AppError::NotFound(format!("未找到插件 {} 的当前版本", plugin_id)))?;
    if current.manifest.id != plugin_id || current.manifest.version != current.version {
        return Err(AppError::InvalidInput(format!(
            "插件 {} 的当前版本 Manifest 身份不一致",
            plugin_id
        )));
    }

    Ok((
        AuthorizedPluginContext {
            manifest: current.manifest,
            version: current.version,
            install_path: current.install_path,
        },
        snapshot.grant_states,
    ))
}

fn validate_plugin_id(value: &str) -> Result<(), AppError> {
    let bytes = value.as_bytes();
    let valid_first = bytes
        .first()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    let valid_rest = bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    });
    if value.len() > 128 || !valid_first || !valid_rest {
        return Err(AppError::InvalidInput(format!("非法插件 ID：{}", value)));
    }
    Ok(())
}

fn validate_capabilities(capabilities: &[&str]) -> Result<(), AppError> {
    if capabilities.is_empty() {
        return Err(AppError::InvalidInput(
            "capability 列表不能为空".to_string(),
        ));
    }
    for capability in capabilities {
        if capability.is_empty()
            || capability.len() > 128
            || !capability
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(AppError::InvalidInput(format!(
                "非法 capability：{}",
                capability
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn manifest(version: &str, permissions: &[&str]) -> PluginManifestV3 {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.guard-test",
            "name": "Guard Test",
            "version": version,
            "authorId": "firstwork-tests",
            "classification": "feature",
            "runtimeKind": "declarative-ui",
            "source": "local",
            "supportedScenes": ["global"],
            "permissions": permissions,
            "contributes": {}
        }))
        .expect("parse guard manifest")
    }

    fn setup(permissions: &[&str], approved: &[&str]) -> Database {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest("1.0.0", permissions);
        db.record_plugin_version(
            &manifest,
            "C:/plugins/guard-test/1.0.0",
            "guard-test-hash",
            &approved
                .iter()
                .map(|item| (*item).to_string())
                .collect::<Vec<_>>(),
        )
        .expect("record guard plugin");
        db.set_plugin_enabled(&manifest.id, true)
            .expect("enable guard plugin");
        db
    }

    fn set_grant(db: &Database, capability: &str, granted: bool) {
        let conn = db.conn_lock().expect("lock database");
        conn.execute(
            "INSERT INTO plugin_permissions (plugin_id, permission, granted)
             VALUES ('com.firstwork.guard-test', ?1, ?2)
             ON CONFLICT(plugin_id, permission) DO UPDATE SET granted = excluded.granted",
            params![capability, i64::from(granted)],
        )
        .expect("set grant state");
    }

    #[test]
    fn accepts_declared_and_granted_capabilities_and_returns_current_context() {
        let db = setup(
            &["ai.invoke", "agents.invoke"],
            &["ai.invoke", "agents.invoke"],
        );
        let context = require_current_plugin_capabilities(
            &db,
            "com.firstwork.guard-test",
            &["ai.invoke", "agents.invoke"],
        )
        .expect("authorize capabilities");

        assert_eq!(context.version, "1.0.0");
        assert_eq!(context.install_path, "C:/plugins/guard-test/1.0.0");
        assert_eq!(context.manifest.version, "1.0.0");
    }

    #[test]
    fn current_context_resolver_keeps_lifecycle_checks_without_capability_grants() {
        let enabled = setup(&["ai.invoke"], &[]);
        let context = resolve_current_plugin_context(&enabled, "com.firstwork.guard-test")
            .expect("resolve current context without grants");
        assert_eq!(context.version, "1.0.0");
        assert_eq!(context.manifest.id, "com.firstwork.guard-test");

        let disabled = setup(&["ai.invoke"], &[]);
        disabled
            .set_plugin_enabled("com.firstwork.guard-test", false)
            .expect("disable plugin");
        assert!(resolve_current_plugin_context(&disabled, "com.firstwork.guard-test").is_err());

        let no_current = setup(&["ai.invoke"], &[]);
        no_current
            .conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions SET is_current = 0
                 WHERE plugin_id = 'com.firstwork.guard-test'",
                [],
            )
            .expect("clear current version");
        assert!(resolve_current_plugin_context(&no_current, "com.firstwork.guard-test").is_err());
    }

    #[test]
    fn rejects_revoked_missing_and_undeclared_capabilities() {
        let revoked = setup(&["ai.invoke"], &[]);
        set_grant(&revoked, "ai.invoke", false);
        assert!(matches!(
            require_current_plugin_capabilities(
                &revoked,
                "com.firstwork.guard-test",
                &["ai.invoke"]
            ),
            Err(AppError::PluginPermissionDenied { .. })
        ));

        let missing = setup(&["ai.invoke"], &[]);
        assert!(matches!(
            require_current_plugin_capabilities(
                &missing,
                "com.firstwork.guard-test",
                &["ai.invoke"]
            ),
            Err(AppError::PluginPermissionDenied { .. })
        ));

        let undeclared = setup(&[], &[]);
        set_grant(&undeclared, "ai.invoke", true);
        assert!(matches!(
            require_current_plugin_capabilities(
                &undeclared,
                "com.firstwork.guard-test",
                &["ai.invoke"]
            ),
            Err(AppError::PluginCapabilityNotDeclared { .. })
        ));
    }

    #[test]
    fn rejects_invalid_lifecycle_and_missing_current_version() {
        let missing = Database::init(":memory:").expect("create in-memory database");
        assert!(matches!(
            require_current_plugin_capabilities(
                &missing,
                "com.firstwork.guard-test",
                &["ai.invoke"]
            ),
            Err(AppError::NotFound(_))
        ));

        let disabled = setup(&["ai.invoke"], &["ai.invoke"]);
        disabled
            .set_plugin_enabled("com.firstwork.guard-test", false)
            .expect("disable plugin");
        assert!(matches!(
            require_current_plugin_capabilities(
                &disabled,
                "com.firstwork.guard-test",
                &["ai.invoke"]
            ),
            Err(AppError::InvalidInput(_))
        ));

        let invalid_status = setup(&["ai.invoke"], &["ai.invoke"]);
        invalid_status
            .conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugins SET status = 'error' WHERE id = 'com.firstwork.guard-test'",
                [],
            )
            .expect("set invalid status");
        assert!(matches!(
            require_current_plugin_capabilities(
                &invalid_status,
                "com.firstwork.guard-test",
                &["ai.invoke"]
            ),
            Err(AppError::InvalidInput(_))
        ));

        let no_current = setup(&["ai.invoke"], &["ai.invoke"]);
        no_current
            .conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions SET is_current = 0
                 WHERE plugin_id = 'com.firstwork.guard-test'",
                [],
            )
            .expect("clear current version");
        assert!(matches!(
            require_current_plugin_capabilities(
                &no_current,
                "com.firstwork.guard-test",
                &["ai.invoke"]
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn rejects_empty_and_partially_authorized_capability_sets() {
        let db = setup(&["ai.invoke", "agents.invoke"], &["ai.invoke"]);
        assert!(matches!(
            require_current_plugin_capabilities(&db, "com.firstwork.guard-test", &[]),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            require_current_plugin_capabilities(
                &db,
                "com.firstwork.guard-test",
                &["ai.invoke", "agents.invoke"]
            ),
            Err(AppError::PluginPermissionDenied { .. })
        ));

        set_grant(&db, "agents.invoke", true);
        let conn = db.conn_lock().expect("lock database");
        conn.execute(
            "UPDATE plugin_versions
             SET manifest_json = json_set(manifest_json, '$.permissions', json('[\"ai.invoke\"]'))
             WHERE plugin_id = 'com.firstwork.guard-test' AND is_current = 1",
            [],
        )
        .expect("remove current declaration");
        drop(conn);
        assert!(matches!(
            require_current_plugin_capabilities(
                &db,
                "com.firstwork.guard-test",
                &["ai.invoke", "agents.invoke"]
            ),
            Err(AppError::PluginCapabilityNotDeclared { .. })
        ));
    }

    #[test]
    fn authorization_query_has_no_permission_write_side_effects() {
        let db = setup(&["ai.invoke", "agents.invoke"], &["ai.invoke"]);
        let before = {
            let conn = db.conn_lock().expect("lock database");
            let mut statement = conn
                .prepare(
                    "SELECT permission, granted FROM plugin_permissions
                     WHERE plugin_id = 'com.firstwork.guard-test' ORDER BY permission",
                )
                .expect("prepare before query");
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .expect("query before rows")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect before rows")
        };

        assert!(require_current_plugin_capabilities(
            &db,
            "com.firstwork.guard-test",
            &["ai.invoke", "agents.invoke"]
        )
        .is_err());

        let after = {
            let conn = db.conn_lock().expect("lock database");
            let mut statement = conn
                .prepare(
                    "SELECT permission, granted FROM plugin_permissions
                     WHERE plugin_id = 'com.firstwork.guard-test' ORDER BY permission",
                )
                .expect("prepare after query");
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .expect("query after rows")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect after rows")
        };
        assert_eq!(before, after);
    }
}
