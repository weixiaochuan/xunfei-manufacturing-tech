//! 正式插件安装平台 DAO。

use rusqlite::{params, OptionalExtension};

use super::Database;
use crate::error::AppError;
use crate::models::{
    PluginActivationRule, PluginManifestV3, PluginScene, PluginVersionInfo, SignatureStatus,
};

#[derive(Debug, Clone)]
pub(crate) struct CurrentPluginVersionAuthorization {
    pub version: String,
    pub manifest: PluginManifestV3,
    pub install_path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CurrentPluginAuthorizationSnapshot {
    pub enabled: bool,
    pub status: String,
    pub current_version: Option<CurrentPluginVersionAuthorization>,
    pub grant_states: Vec<(String, Option<bool>)>,
}

fn enum_value<T: serde::Serialize>(value: &T, fallback: &str) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| fallback.to_string())
}

fn signature_from_db(value: String) -> SignatureStatus {
    serde_json::from_value(serde_json::Value::String(value)).unwrap_or_default()
}

impl Database {
    /// 在同一连接锁内读取运行时授权所需的当前状态。
    ///
    /// `manifest_json` 是当前活动版本的权威声明，`plugin_permissions.granted`
    /// 是用户真实授权；本查询只读，不会创建、同步或恢复任何授权行。
    pub(crate) fn current_plugin_authorization_snapshot(
        &self,
        plugin_id: &str,
        capabilities: &[&str],
    ) -> Result<Option<CurrentPluginAuthorizationSnapshot>, AppError> {
        let conn = self.conn_lock()?;
        let plugin: Option<(i64, String)> = conn
            .query_row(
                "SELECT enabled, status FROM plugins WHERE id = ?1",
                [plugin_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((enabled, status)) = plugin else {
            return Ok(None);
        };

        let current: Option<(String, String, String)> = conn
            .query_row(
                "SELECT version, manifest_json, install_path
                 FROM plugin_versions
                 WHERE plugin_id = ?1 AND is_current = 1",
                [plugin_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let current_version = current
            .map(
                |(
                    version,
                    manifest_json,
                    install_path,
                )|
                 -> Result<CurrentPluginVersionAuthorization, AppError> {
                Ok(CurrentPluginVersionAuthorization {
                    version,
                    manifest: serde_json::from_str(&manifest_json)?,
                    install_path,
                })
            },
            )
            .transpose()?;

        let mut grant_states = Vec::with_capacity(capabilities.len());
        let mut statement = conn.prepare(
            "SELECT granted FROM plugin_permissions
             WHERE plugin_id = ?1 AND permission = ?2",
        )?;
        for capability in capabilities {
            let granted: Option<i64> = statement
                .query_row(params![plugin_id, capability], |row| row.get(0))
                .optional()?;
            grant_states.push(((*capability).to_string(), granted.map(|value| value != 0)));
        }

        Ok(Some(CurrentPluginAuthorizationSnapshot {
            enabled: enabled != 0,
            status,
            current_version,
            grant_states,
        }))
    }

    pub fn current_plugin_version(&self, plugin_id: &str) -> Result<Option<String>, AppError> {
        let conn = self.conn_lock()?;
        Ok(conn
            .query_row(
                "SELECT version FROM plugin_versions WHERE plugin_id = ?1 AND is_current = 1",
                [plugin_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// 返回当前活动版本保存的 Manifest 权限声明。
    ///
    /// 该字段是版本声明快照，不代表用户当前仍然授权这些权限。
    pub fn current_version_declared_permissions(
        &self,
        plugin_id: &str,
    ) -> Result<Option<Vec<String>>, AppError> {
        let conn = self.conn_lock()?;
        let json: Option<String> = conn
            .query_row(
                "SELECT permissions_json FROM plugin_versions
                 WHERE plugin_id = ?1 AND is_current = 1",
                [plugin_id],
                |row| row.get(0),
            )
            .optional()?;
        match json {
            Some(value) => Ok(Some(serde_json::from_str(&value)?)),
            None => Ok(None),
        }
    }

    /// 在同一事务内记录版本、切换 current，并同步旧插件表供现有 UI 使用。
    pub fn record_plugin_version(
        &self,
        manifest: &PluginManifestV3,
        install_path: &str,
        content_hash: &str,
        approved_permissions: &[String],
    ) -> Result<Option<String>, AppError> {
        let conn = self.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        let previous_version: Option<String> = tx
            .query_row(
                "SELECT version FROM plugin_versions WHERE plugin_id = ?1 AND is_current = 1",
                [manifest.id.as_str()],
                |row| row.get(0),
            )
            .optional()?;

        tx.execute(
            "UPDATE plugin_versions SET is_current = 0 WHERE plugin_id = ?1",
            [manifest.id.as_str()],
        )?;
        let signature = enum_value(&manifest.signature.status, "unsigned");
        tx.execute(
            "INSERT INTO plugin_versions
                (plugin_id, version, install_path, manifest_json, content_hash,
                 permissions_json, signature_status, is_current, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, datetime('now','localtime'))
             ON CONFLICT(plugin_id, version) DO UPDATE SET
                install_path = excluded.install_path,
                manifest_json = excluded.manifest_json,
                content_hash = excluded.content_hash,
                permissions_json = excluded.permissions_json,
                signature_status = excluded.signature_status,
                is_current = 1",
            params![
                manifest.id,
                manifest.version,
                install_path,
                serde_json::to_string(manifest)?,
                content_hash,
                serde_json::to_string(&manifest.permissions)?,
                signature,
            ],
        )?;

        // 旧表存储一个安全的声明式适配 manifest，避免已有列表/设置页失效。
        let main = manifest
            .contributes
            .features
            .iter()
            .find_map(|item| item.ui_schema.clone())
            .unwrap_or_else(|| "manifest.json".to_string());
        let legacy_manifest = crate::models::PluginManifest {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            author: Some(manifest.author_id.clone()),
            main: main.clone(),
            styles: None,
            min_app_version: manifest.min_app_version.clone(),
            permissions: manifest.permissions.clone(),
            contributes: crate::models::PluginContributes::default(),
        };
        let runtime_kind = enum_value(&manifest.runtime_kind, "declarative-ui");
        let source = enum_value(&manifest.source, "local");
        let product_type = match manifest.classification {
            crate::models::PluginClassification::Feature => "declarative-ui",
            crate::models::PluginClassification::Enhancement => "prompt-pack",
            crate::models::PluginClassification::Hybrid => "local-plugin",
        };
        tx.execute(
            "INSERT INTO plugins
                (id, name, version, description, author, path, main, styles,
                 min_app_version, manifest_json, enabled, status, content_hash,
                 manifest_format, schema_version, product_type, runtime_kind, source,
                 signature_status, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, 0, 'installed',
                     ?10, 'v3', 3, ?11, ?12, ?13, ?14,
                     datetime('now','localtime'), datetime('now','localtime'))
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                version = excluded.version,
                description = excluded.description,
                author = excluded.author,
                path = excluded.path,
                main = excluded.main,
                min_app_version = excluded.min_app_version,
                manifest_json = excluded.manifest_json,
                status = 'installed',
                content_hash = excluded.content_hash,
                manifest_format = 'v3',
                schema_version = 3,
                product_type = excluded.product_type,
                runtime_kind = excluded.runtime_kind,
                source = excluded.source,
                signature_status = excluded.signature_status,
                updated_at = datetime('now','localtime')",
            params![
                manifest.id,
                manifest.name,
                manifest.version,
                manifest.description,
                manifest.author_id,
                install_path,
                main,
                manifest.min_app_version,
                serde_json::to_string(&legacy_manifest)?,
                content_hash,
                product_type,
                runtime_kind,
                source,
                signature,
            ],
        )?;

        tx.execute(
            "DELETE FROM plugin_permissions WHERE plugin_id = ?1",
            [manifest.id.as_str()],
        )?;
        for permission in &manifest.permissions {
            tx.execute(
                "INSERT INTO plugin_permissions
                    (plugin_id, permission, granted, created_at, updated_at)
                 VALUES (?1, ?2, ?3, datetime('now','localtime'), datetime('now','localtime'))",
                params![
                    manifest.id,
                    permission,
                    i32::from(approved_permissions.contains(permission)),
                ],
            )?;
        }

        tx.execute(
            "INSERT INTO plugin_install_history
                (plugin_id, operation, from_version, to_version, content_hash, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'success')",
            params![
                manifest.id,
                if previous_version.is_some() {
                    "update"
                } else {
                    "install"
                },
                previous_version,
                manifest.version,
                content_hash,
            ],
        )?;
        tx.commit()?;
        Ok(previous_version)
    }

    pub fn list_plugin_versions_v3(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginVersionInfo>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT plugin_id, version, install_path, content_hash, is_current,
                    signature_status, installed_at
             FROM plugin_versions WHERE plugin_id = ?1
             ORDER BY installed_at DESC, version DESC",
        )?;
        let rows = stmt
            .query_map([plugin_id], |row| {
                Ok(PluginVersionInfo {
                    plugin_id: row.get(0)?,
                    version: row.get(1)?,
                    install_path: row.get(2)?,
                    content_hash: row.get(3)?,
                    is_current: row.get::<_, i32>(4)? != 0,
                    signature_status: signature_from_db(row.get(5)?),
                    installed_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn plugin_version_manifest(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<Option<(PluginManifestV3, String, String)>, AppError> {
        let conn = self.conn_lock()?;
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT manifest_json, install_path, content_hash FROM plugin_versions
                 WHERE plugin_id = ?1 AND version = ?2",
                params![plugin_id, version],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        row.map(|(json, path, hash)| Ok((serde_json::from_str(&json)?, path, hash)))
            .transpose()
    }

    pub fn current_v3_plugins(&self) -> Result<Vec<(PluginManifestV3, String, bool)>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT pv.manifest_json, pv.install_path, COALESCE(p.enabled, 0)
             FROM plugin_versions pv
             LEFT JOIN plugins p ON p.id = pv.plugin_id
             WHERE pv.is_current = 1",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i32>(2)? != 0,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(json, path, enabled)| Ok((serde_json::from_str(&json)?, path, enabled)))
            .collect()
    }

    pub fn switch_plugin_version(
        &self,
        manifest: &PluginManifestV3,
        install_path: &str,
        content_hash: &str,
    ) -> Result<Option<String>, AppError> {
        let conn = self.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT version FROM plugin_versions WHERE plugin_id = ?1 AND is_current = 1",
                [manifest.id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        tx.execute(
            "UPDATE plugin_versions SET is_current = CASE WHEN version = ?2 THEN 1 ELSE 0 END
             WHERE plugin_id = ?1",
            params![manifest.id, manifest.version],
        )?;
        tx.execute(
            "UPDATE plugins SET version = ?2, path = ?3, content_hash = ?4,
                    updated_at = datetime('now','localtime') WHERE id = ?1",
            params![manifest.id, manifest.version, install_path, content_hash],
        )?;
        tx.execute(
            "INSERT INTO plugin_install_history
                (plugin_id, operation, from_version, to_version, content_hash, status)
             VALUES (?1, 'rollback', ?2, ?3, ?4, 'success')",
            params![manifest.id, previous, manifest.version, content_hash],
        )?;
        tx.commit()?;
        Ok(previous)
    }

    pub fn get_plugin_activation_settings(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginActivationRule>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT plugin_id, scope_type, scope_key, enabled
             FROM plugin_activation_settings WHERE plugin_id = ?1
             ORDER BY CASE scope_type WHEN 'global' THEN 0 WHEN 'scene' THEN 1 ELSE 2 END,
                      scope_key",
        )?;
        let rows = stmt
            .query_map([plugin_id], |row| {
                Ok(PluginActivationRule {
                    plugin_id: row.get(0)?,
                    scope_type: row.get(1)?,
                    scope_key: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    source: "persisted".to_string(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_plugin_activation_setting(
        &self,
        plugin_id: &str,
        scope_type: &str,
        scope_key: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        if !matches!(scope_type, "global" | "scene" | "feature") {
            return Err(AppError::InvalidInput(
                "激活范围必须是 global/scene/feature".into(),
            ));
        }
        if scope_type == "global" && !scope_key.is_empty() {
            return Err(AppError::InvalidInput(
                "global 范围的 scopeKey 必须为空".into(),
            ));
        }
        if scope_type != "global" && scope_key.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "scene/feature 范围必须提供 scopeKey".into(),
            ));
        }
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO plugin_activation_settings
                (plugin_id, scope_type, scope_key, enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now','localtime'))
             ON CONFLICT(plugin_id, scope_type, scope_key) DO UPDATE SET
                enabled = excluded.enabled,
                updated_at = excluded.updated_at",
            params![plugin_id, scope_type, scope_key, i32::from(enabled)],
        )?;
        Ok(())
    }

    pub fn resolve_plugin_enabled(
        &self,
        manifest: &PluginManifestV3,
        scene: &PluginScene,
        feature: &str,
        session_override: Option<bool>,
    ) -> Result<(bool, String), AppError> {
        if let Some(enabled) = session_override {
            return Ok((enabled, "session".into()));
        }
        let rules = self.get_plugin_activation_settings(&manifest.id)?;
        if let Some(rule) = rules
            .iter()
            .find(|rule| rule.scope_type == "feature" && rule.scope_key == feature)
        {
            return Ok((rule.enabled, "feature".into()));
        }
        if let Some(rule) = rules
            .iter()
            .find(|rule| rule.scope_type == "scene" && rule.scope_key == scene.as_str())
        {
            return Ok((rule.enabled, "scene".into()));
        }
        if let Some(rule) = rules.iter().find(|rule| rule.scope_type == "global") {
            return Ok((rule.enabled, "global".into()));
        }
        if let Some(value) = manifest.default_activation.scenes.get(scene.as_str()) {
            return Ok((*value, "manifest-scene".into()));
        }
        Ok((manifest.default_activation.global, "manifest-global".into()))
    }

    pub fn record_plugin_execution(
        &self,
        plugin_id: &str,
        contribution_id: Option<&str>,
        hook: Option<&str>,
        scene: &str,
        feature: &str,
        request_id: &str,
        status: &str,
        duration_ms: Option<i64>,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO plugin_execution_logs
                (plugin_id, contribution_id, hook, scene, feature, request_id,
                 status, duration_ms, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                plugin_id,
                contribution_id,
                hook,
                scene,
                feature,
                request_id,
                status,
                duration_ms,
                error_message,
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with_permissions(permissions: &[&str]) -> PluginManifestV3 {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.permission-snapshot",
            "name": "Permission Snapshot",
            "version": "1.0.0",
            "authorId": "tests",
            "classification": "feature",
            "runtimeKind": "declarative-ui",
            "source": "local",
            "supportedScenes": ["global"],
            "permissions": permissions,
            "contributes": {
                "features": [{
                    "id": "snapshot-feature",
                    "title": "Snapshot Feature",
                    "scenes": ["global"],
                    "uiSchema": "ui.json"
                }]
            }
        }))
        .expect("parse test manifest")
    }

    #[test]
    fn authorization_snapshot_reads_current_manifest_and_exact_grant_states() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["ai.invoke", "agents.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/current-authorized",
            "authorized-hash",
            &["ai.invoke".to_string()],
        )
        .expect("record current version");
        db.set_plugin_enabled(&manifest.id, true)
            .expect("enable plugin");

        let snapshot = db
            .current_plugin_authorization_snapshot(
                &manifest.id,
                &["ai.invoke", "agents.invoke", "ai"],
            )
            .expect("query authorization snapshot")
            .expect("plugin exists");
        assert!(snapshot.enabled);
        assert_eq!(snapshot.status, "installed");
        let current = snapshot.current_version.expect("current version exists");
        assert_eq!(current.version, "1.0.0");
        assert_eq!(current.install_path, "C:/test/current-authorized");
        assert_eq!(current.manifest.permissions, manifest.permissions);
        assert_eq!(
            snapshot.grant_states,
            vec![
                ("ai.invoke".to_string(), Some(true)),
                ("agents.invoke".to_string(), Some(false)),
                ("ai".to_string(), None),
            ]
        );
    }

    #[test]
    fn authorization_snapshot_distinguishes_missing_plugin_and_current_version() {
        let db = Database::init(":memory:").expect("create in-memory database");
        assert!(db
            .current_plugin_authorization_snapshot("com.firstwork.missing", &["ai.invoke"])
            .expect("query missing plugin")
            .is_none());

        let manifest = manifest_with_permissions(&["ai.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/no-current",
            "no-current-hash",
            &manifest.permissions,
        )
        .expect("record plugin");
        db.conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions SET is_current = 0 WHERE plugin_id = ?1",
                [&manifest.id],
            )
            .expect("clear current version");
        let snapshot = db
            .current_plugin_authorization_snapshot(&manifest.id, &["ai.invoke"])
            .expect("query plugin without current version")
            .expect("plugin exists");
        assert!(snapshot.current_version.is_none());
        assert_eq!(
            snapshot.grant_states,
            vec![("ai.invoke".to_string(), Some(true))]
        );
    }

    #[test]
    fn current_version_declared_permissions_distinguishes_present_empty_and_missing() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let declared = manifest_with_permissions(&["ai.invoke"]);
        db.record_plugin_version(
            &declared,
            "C:/test/declared",
            "declared-hash",
            &declared.permissions,
        )
        .expect("record declared version");
        assert_eq!(
            db.current_version_declared_permissions(&declared.id)
                .expect("query declared permissions"),
            Some(vec!["ai.invoke".to_string()])
        );

        let mut empty = manifest_with_permissions(&[]);
        empty.id = "com.firstwork.permission-empty".into();
        db.record_plugin_version(&empty, "C:/test/empty", "empty-hash", &[])
            .expect("record empty version");
        assert_eq!(
            db.current_version_declared_permissions(&empty.id)
                .expect("query empty permissions"),
            Some(Vec::new())
        );
        assert_eq!(
            db.current_version_declared_permissions("com.firstwork.permission-missing")
                .expect("query missing version"),
            None
        );
    }

    #[test]
    fn permission_queries_do_not_create_or_regrant_records() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["ai.invoke"]);
        db.record_plugin_version(&manifest, "C:/test/query", "query-hash", &[])
            .expect("record denied permission");

        assert_eq!(
            db.plugin_permission_grant_state(&manifest.id, "ai.invoke")
                .expect("query denied permission"),
            Some(false)
        );
        assert_eq!(
            db.plugin_permission_grant_state(&manifest.id, "agents.invoke")
                .expect("query missing permission"),
            None
        );
        db.current_version_declared_permissions(&manifest.id)
            .expect("query version declaration");
        assert_eq!(
            db.plugin_permission_grant_state(&manifest.id, "ai.invoke")
                .expect("re-query denied permission"),
            Some(false)
        );
        assert_eq!(
            db.plugin_permission_grant_state(&manifest.id, "agents.invoke")
                .expect("re-query missing permission"),
            None
        );
    }
}
