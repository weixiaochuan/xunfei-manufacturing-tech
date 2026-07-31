//! 正式插件安装平台 DAO。

use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::Database;
use crate::error::AppError;
use crate::models::{
    CurrentManifestCapabilityDeclaration, CurrentPluginPermissionFacts,
    InstalledVersionCapabilitySnapshot, LegacyCapabilityAuthorizationFact,
    LegacyCapabilityGrantState, PluginActivationRule, PluginManifestV3, PluginPermissionFactSource,
    PluginScene, PluginVersionInfo, SignatureStatus,
};
use crate::services::plugin_capabilities::VALID_PERMISSIONS;

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

fn normalize_manifest_capabilities(
    plugin_id: &str,
    capabilities: &[String],
) -> Result<Vec<String>, AppError> {
    let mut normalized = BTreeSet::new();
    for capability in capabilities {
        if !VALID_PERMISSIONS.contains(&capability.as_str()) {
            return Err(AppError::PluginManifestCapabilityDeclarationInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("包含未知 capability：{}", capability),
            });
        }
        if !normalized.insert(capability.clone()) {
            return Err(AppError::PluginManifestCapabilityDeclarationInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("包含重复 capability：{}", capability),
            });
        }
    }
    Ok(normalized.into_iter().collect())
}

fn parse_version_capability_snapshot(
    plugin_id: &str,
    version: &str,
    json: &str,
) -> Result<Vec<String>, AppError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| AppError::PluginPermissionSnapshotInvalid {
            plugin_id: plugin_id.to_string(),
            version: version.to_string(),
            reason: format!("JSON 无法解析：{}", error),
        })?;
    let items = value
        .as_array()
        .ok_or_else(|| AppError::PluginPermissionSnapshotInvalid {
            plugin_id: plugin_id.to_string(),
            version: version.to_string(),
            reason: "顶层必须是 capability 数组".to_string(),
        })?;

    let mut normalized = BTreeSet::new();
    for item in items {
        let capability =
            item.as_str()
                .ok_or_else(|| AppError::PluginPermissionSnapshotInvalid {
                    plugin_id: plugin_id.to_string(),
                    version: version.to_string(),
                    reason: "数组元素必须是 capability 字符串".to_string(),
                })?;
        if !VALID_PERMISSIONS.contains(&capability) {
            return Err(AppError::PluginPermissionSnapshotInvalid {
                plugin_id: plugin_id.to_string(),
                version: version.to_string(),
                reason: format!("包含未知 capability：{}", capability),
            });
        }
        if !normalized.insert(capability.to_string()) {
            return Err(AppError::PluginPermissionSnapshotInvalid {
                plugin_id: plugin_id.to_string(),
                version: version.to_string(),
                reason: format!("包含重复 capability：{}", capability),
            });
        }
    }
    Ok(normalized.into_iter().collect())
}

fn read_current_manifest_capability_declaration(
    conn: &Connection,
    plugin_id: &str,
) -> Result<CurrentManifestCapabilityDeclaration, AppError> {
    let current: Option<(String, String)> = conn
        .query_row(
            "SELECT version, manifest_json
             FROM plugin_versions
             WHERE plugin_id = ?1 AND is_current = 1",
            [plugin_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (version, manifest_json) = current.ok_or_else(|| {
        AppError::NotFound(format!("未找到插件 {} 的当前版本 Manifest", plugin_id))
    })?;
    let manifest: PluginManifestV3 = serde_json::from_str(&manifest_json).map_err(|error| {
        AppError::PluginManifestCapabilityDeclarationInvalid {
            plugin_id: plugin_id.to_string(),
            reason: format!("Manifest 无法解析：{}", error),
        }
    })?;
    if manifest.id != plugin_id || manifest.version != version {
        return Err(AppError::PluginManifestCapabilityDeclarationInvalid {
            plugin_id: plugin_id.to_string(),
            reason: "Manifest 身份或版本与当前版本记录不一致".to_string(),
        });
    }
    Ok(CurrentManifestCapabilityDeclaration {
        plugin_id: plugin_id.to_string(),
        version,
        capabilities: normalize_manifest_capabilities(plugin_id, &manifest.permissions)?,
        source: PluginPermissionFactSource::CurrentManifest,
    })
}

fn read_current_version_capability_snapshot(
    conn: &Connection,
    plugin_id: &str,
) -> Result<InstalledVersionCapabilitySnapshot, AppError> {
    let current: Option<(String, String)> = conn
        .query_row(
            "SELECT version, permissions_json
             FROM plugin_versions
             WHERE plugin_id = ?1 AND is_current = 1",
            [plugin_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (version, permissions_json) =
        current.ok_or_else(|| AppError::PluginPermissionSnapshotMissing {
            plugin_id: plugin_id.to_string(),
        })?;
    Ok(InstalledVersionCapabilitySnapshot {
        plugin_id: plugin_id.to_string(),
        version: version.clone(),
        capabilities: parse_version_capability_snapshot(plugin_id, &version, &permissions_json)?,
        source: PluginPermissionFactSource::InstalledVersionSnapshot,
    })
}

fn read_legacy_capability_authorization(
    conn: &Connection,
    plugin_id: &str,
    capability: &str,
) -> Result<LegacyCapabilityAuthorizationFact, AppError> {
    let granted: Option<i64> = conn
        .query_row(
            "SELECT granted FROM plugin_permissions
             WHERE plugin_id = ?1 AND permission = ?2",
            params![plugin_id, capability],
            |row| row.get(0),
        )
        .optional()?;
    let state = match granted {
        Some(value) if value != 0 => LegacyCapabilityGrantState::Granted,
        Some(_) => LegacyCapabilityGrantState::NotGrantedCompatible,
        None => LegacyCapabilityGrantState::Missing,
    };
    Ok(LegacyCapabilityAuthorizationFact {
        plugin_id: plugin_id.to_string(),
        capability: capability.to_string(),
        state,
        source: PluginPermissionFactSource::LegacyPluginPermissions,
    })
}

impl Database {
    /// 在同一连接锁内读取运行时授权所需的当前状态。
    ///
    /// `manifest_json` 是当前活动版本的权威声明；`plugin_permissions.granted` 仅作为
    /// legacy 布尔授权兼容输入。本查询只读，不会创建、同步或恢复任何授权行。
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

    /// 读取当前版本 Manifest 的 capability 声明，不代表用户授权。
    pub fn current_manifest_capability_declaration(
        &self,
        plugin_id: &str,
    ) -> Result<CurrentManifestCapabilityDeclaration, AppError> {
        let conn = self.conn_lock()?;
        read_current_manifest_capability_declaration(&conn, plugin_id)
    }

    /// 读取当前安装版本的 `permissions_json` capability 快照，不代表用户授权。
    pub fn current_version_capability_snapshot(
        &self,
        plugin_id: &str,
    ) -> Result<InstalledVersionCapabilitySnapshot, AppError> {
        let conn = self.conn_lock()?;
        read_current_version_capability_snapshot(&conn, plugin_id)
    }

    /// 在同一 SQLite 读取快照中取得 Manifest 声明与版本权限快照。
    ///
    /// Service 层需要先区分“Manifest 未声明”和“快照不包含”，再对完整集合执行
    /// fail-closed 一致性检查，因此这里不提前合并两类事实，也不读取 legacy 授权。
    pub(crate) fn current_plugin_capability_contract(
        &self,
        plugin_id: &str,
    ) -> Result<
        (
            CurrentManifestCapabilityDeclaration,
            InstalledVersionCapabilitySnapshot,
        ),
        AppError,
    > {
        let mut conn = self.conn_lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let manifest = read_current_manifest_capability_declaration(&tx, plugin_id)?;
        let snapshot = read_current_version_capability_snapshot(&tx, plugin_id)?;
        tx.commit()?;
        Ok((manifest, snapshot))
    }

    /// 读取单个 capability 的 legacy 布尔授权兼容事实。
    pub fn current_legacy_capability_authorization(
        &self,
        plugin_id: &str,
        capability: &str,
    ) -> Result<LegacyCapabilityAuthorizationFact, AppError> {
        let conn = self.conn_lock()?;
        read_legacy_capability_authorization(&conn, plugin_id, capability)
    }

    /// 返回来源分离且 Manifest/版本快照语义一致的三类只读权限事实。
    pub fn current_plugin_permission_facts(
        &self,
        plugin_id: &str,
        capabilities: &[&str],
    ) -> Result<CurrentPluginPermissionFacts, AppError> {
        let mut conn = self.conn_lock()?;
        // Mutex 串行化同一 Database 实例，deferred 事务再固定 WAL 中跨多次 SELECT 的读取快照，
        // 避免其他 SQLite 连接在 Manifest、版本快照和多项 legacy grant 之间插入写入。
        let tx = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let manifest_declaration = read_current_manifest_capability_declaration(&tx, plugin_id)?;
        let version_snapshot = read_current_version_capability_snapshot(&tx, plugin_id)?;
        if manifest_declaration.version != version_snapshot.version
            || manifest_declaration.capabilities != version_snapshot.capabilities
        {
            return Err(AppError::PluginPermissionSnapshotMismatch {
                plugin_id: plugin_id.to_string(),
                version: version_snapshot.version,
            });
        }
        let legacy_authorizations = capabilities
            .iter()
            .map(|capability| read_legacy_capability_authorization(&tx, plugin_id, capability))
            .collect::<Result<Vec<_>, _>>()?;
        let facts = CurrentPluginPermissionFacts {
            manifest_declaration,
            version_snapshot,
            legacy_authorizations,
        };
        tx.commit()?;
        Ok(facts)
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
        self.plugin_version_manifest_raw(plugin_id, version)?
            .map(|(json, path, hash)| Ok((serde_json::from_str(&json)?, path, hash)))
            .transpose()
    }

    /// 返回版本记录中的原始 Manifest JSON，供宿主在反序列化前执行调用模式相关校验。
    pub fn plugin_version_manifest_raw(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<Option<(String, String, String)>, AppError> {
        let conn = self.conn_lock()?;
        conn.query_row(
            "SELECT manifest_json, install_path, content_hash FROM plugin_versions
                 WHERE plugin_id = ?1 AND version = ?2",
            params![plugin_id, version],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(AppError::from)
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
        let target_exists = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM plugin_versions WHERE plugin_id = ?1 AND version = ?2
             )",
            params![manifest.id, manifest.version],
            |row| row.get::<_, bool>(0),
        )?;
        if !target_exists {
            return Err(AppError::NotFound(format!(
                "未找到插件 {} 的版本 {}",
                manifest.id, manifest.version
            )));
        }

        // SQLite 会逐行维护部分唯一索引；必须先清除旧 current，再精确设置目标版本。
        tx.execute(
            "UPDATE plugin_versions SET is_current = 0
             WHERE plugin_id = ?1 AND is_current = 1",
            [manifest.id.as_str()],
        )?;
        let updated = tx.execute(
            "UPDATE plugin_versions SET is_current = 1
             WHERE plugin_id = ?1 AND version = ?2",
            params![manifest.id, manifest.version],
        )?;
        if updated != 1 {
            return Err(AppError::InvalidInput(format!(
                "插件版本切换目标不唯一：{}@{}",
                manifest.id, manifest.version
            )));
        }
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

    fn temporary_database_path(test_name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pomegranate-{test_name}-{}-{nonce}.db",
            std::process::id()
        ))
    }

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

    #[test]
    fn permission_fact_contract_separates_sources_and_normalizes_semantic_sets() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["agents.invoke", "ai.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/facts",
            "facts-hash",
            &["ai.invoke".to_string()],
        )
        .expect("record current version");
        db.conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions
                 SET permissions_json = '[
                   \"ai.invoke\",
                   \"agents.invoke\"
                 ]'
                 WHERE plugin_id = ?1 AND is_current = 1",
                [&manifest.id],
            )
            .expect("rewrite snapshot formatting");

        let facts = db
            .current_plugin_permission_facts(
                &manifest.id,
                &["ai.invoke", "agents.invoke", "credentials.use"],
            )
            .expect("read permission facts");
        let expected = vec!["agents.invoke".to_string(), "ai.invoke".to_string()];
        assert_eq!(facts.manifest_declaration.capabilities, expected);
        assert_eq!(
            facts.manifest_declaration.source,
            PluginPermissionFactSource::CurrentManifest
        );
        assert_eq!(facts.version_snapshot.capabilities, expected);
        assert_eq!(
            facts.version_snapshot.source,
            PluginPermissionFactSource::InstalledVersionSnapshot
        );
        assert_eq!(
            facts
                .legacy_authorizations
                .iter()
                .map(|fact| {
                    (
                        fact.capability.clone(),
                        fact.state.clone(),
                        fact.source.clone(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (
                    "ai.invoke".to_string(),
                    LegacyCapabilityGrantState::Granted,
                    PluginPermissionFactSource::LegacyPluginPermissions,
                ),
                (
                    "agents.invoke".to_string(),
                    LegacyCapabilityGrantState::NotGrantedCompatible,
                    PluginPermissionFactSource::LegacyPluginPermissions,
                ),
                (
                    "credentials.use".to_string(),
                    LegacyCapabilityGrantState::Missing,
                    PluginPermissionFactSource::LegacyPluginPermissions,
                ),
            ]
        );
    }

    #[test]
    fn permission_fact_read_transaction_keeps_current_version_and_legacy_grants_consistent() {
        let db_path = temporary_database_path("permission-facts-snapshot");
        let db_path_string = db_path.to_string_lossy().into_owned();
        let db = Database::init(&db_path_string).expect("create file database");

        let old = manifest_with_permissions(&["ai.invoke", "agents.invoke"]);
        db.record_plugin_version(
            &old,
            "C:/test/snapshot-v1",
            "snapshot-v1-hash",
            &["ai.invoke".to_string()],
        )
        .expect("record old version");
        let mut new = old.clone();
        new.version = "2.0.0".to_string();
        new.permissions = vec!["credentials.use".to_string()];
        db.record_plugin_version(
            &new,
            "C:/test/snapshot-v2",
            "snapshot-v2-hash",
            &["credentials.use".to_string()],
        )
        .expect("record new version");
        db.switch_plugin_version(&old, "C:/test/snapshot-v1", "snapshot-v1-hash")
            .expect("restore old current version");
        {
            let conn = db.conn_lock().expect("lock database");
            conn.execute(
                "INSERT INTO plugin_permissions
                    (plugin_id, permission, granted, created_at, updated_at)
                 VALUES (?1, 'ai.invoke', 1, datetime('now'), datetime('now'))
                 ON CONFLICT(plugin_id, permission) DO UPDATE SET granted = 1",
                [&old.id],
            )
            .expect("seed granted legacy capability");
            conn.execute(
                "INSERT INTO plugin_permissions
                    (plugin_id, permission, granted, created_at, updated_at)
                 VALUES (?1, 'agents.invoke', 0, datetime('now'), datetime('now'))
                 ON CONFLICT(plugin_id, permission) DO UPDATE SET granted = 0",
                [&old.id],
            )
            .expect("seed not-granted legacy capability");
        }

        let mut conn = db.conn_lock().expect("lock database");
        let read_tx = conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .expect("start read transaction");
        let manifest =
            read_current_manifest_capability_declaration(&read_tx, &old.id).expect("read manifest");
        assert_eq!(manifest.version, "1.0.0");

        // 第二个 WAL 连接在第一次读取后切换 current 并同时改变多项 legacy grant。
        // 后续读取仍必须停留在同一 SQLite 快照，不能拼接新旧状态。
        let mut writer = Connection::open(&db_path).expect("open concurrent writer");
        writer
            .pragma_update(None, "foreign_keys", "ON")
            .expect("enable writer foreign keys");
        let writer_tx = writer.transaction().expect("start writer transaction");
        writer_tx
            .execute(
                "UPDATE plugin_versions SET is_current = CASE version
                    WHEN '1.0.0' THEN 0 WHEN '2.0.0' THEN 1 ELSE is_current END
                 WHERE plugin_id = ?1",
                [&old.id],
            )
            .expect("switch current version concurrently");
        writer_tx
            .execute(
                "UPDATE plugin_permissions SET granted = CASE permission
                    WHEN 'ai.invoke' THEN 0 WHEN 'agents.invoke' THEN 1 ELSE granted END
                 WHERE plugin_id = ?1",
                [&old.id],
            )
            .expect("change multiple legacy grants concurrently");
        writer_tx.commit().expect("commit concurrent write");

        let snapshot =
            read_current_version_capability_snapshot(&read_tx, &old.id).expect("read snapshot");
        let ai = read_legacy_capability_authorization(&read_tx, &old.id, "ai.invoke")
            .expect("read ai grant");
        let agents = read_legacy_capability_authorization(&read_tx, &old.id, "agents.invoke")
            .expect("read agents grant");
        assert_eq!(snapshot.version, manifest.version);
        assert_eq!(snapshot.capabilities, manifest.capabilities);
        assert_eq!(ai.state, LegacyCapabilityGrantState::Granted);
        assert_eq!(
            agents.state,
            LegacyCapabilityGrantState::NotGrantedCompatible
        );
        read_tx.commit().expect("finish read transaction");
        drop(conn);

        let current = db
            .current_plugin_permission_facts(&old.id, &["credentials.use"])
            .expect("read new consistent aggregate");
        assert_eq!(current.manifest_declaration.version, "2.0.0");
        assert_eq!(current.version_snapshot.version, "2.0.0");
        assert_eq!(
            current.legacy_authorizations[0].state,
            LegacyCapabilityGrantState::Granted
        );

        drop(writer);
        drop(db);
        for path in [
            db_path.clone(),
            std::path::PathBuf::from(format!("{}-wal", db_path.display())),
            std::path::PathBuf::from(format!("{}-shm", db_path.display())),
        ] {
            if path.exists() {
                std::fs::remove_file(path).expect("remove temporary database file");
            }
        }
    }

    #[test]
    fn manifest_declaration_rejects_unknown_capability_through_dao() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["ai.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/manifest-unknown",
            "manifest-unknown-hash",
            &manifest.permissions,
        )
        .expect("record current version");
        let mut manifest_value =
            serde_json::to_value(&manifest).expect("serialize current manifest");
        manifest_value["permissions"] = serde_json::json!(["unknown.capability"]);
        let manifest_json =
            serde_json::to_string(&manifest_value).expect("serialize invalid manifest");
        db.conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions SET manifest_json = ?2
                 WHERE plugin_id = ?1 AND is_current = 1",
                params![manifest.id, manifest_json],
            )
            .expect("store unknown capability");

        match db.current_manifest_capability_declaration(&manifest.id) {
            Err(AppError::PluginManifestCapabilityDeclarationInvalid { reason, .. }) => {
                assert!(reason.contains("未知 capability"));
            }
            other => panic!("expected invalid manifest capability error, got {other:?}"),
        }
    }

    #[test]
    fn manifest_declaration_rejects_duplicate_capability_through_dao() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["ai.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/manifest-duplicate",
            "manifest-duplicate-hash",
            &manifest.permissions,
        )
        .expect("record current version");
        let mut manifest_value =
            serde_json::to_value(&manifest).expect("serialize current manifest");
        manifest_value["permissions"] = serde_json::json!(["ai.invoke", "ai.invoke"]);
        db.conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions SET manifest_json = ?2
                 WHERE plugin_id = ?1 AND is_current = 1",
                params![manifest.id, manifest_value.to_string()],
            )
            .expect("store duplicate capability");

        match db.current_manifest_capability_declaration(&manifest.id) {
            Err(AppError::PluginManifestCapabilityDeclarationInvalid { reason, .. }) => {
                assert!(reason.contains("重复 capability"));
            }
            other => panic!("expected invalid manifest capability error, got {other:?}"),
        }
    }

    #[test]
    fn version_snapshot_distinguishes_empty_and_missing() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&[]);
        db.record_plugin_version(&manifest, "C:/test/empty-facts", "empty-facts-hash", &[])
            .expect("record empty current version");
        assert!(db
            .current_version_capability_snapshot(&manifest.id)
            .expect("read empty snapshot")
            .capabilities
            .is_empty());

        assert!(matches!(
            db.current_version_capability_snapshot("com.firstwork.snapshot-missing"),
            Err(AppError::PluginPermissionSnapshotMissing { .. })
        ));
    }

    #[test]
    fn version_snapshot_rejects_invalid_json_shape_elements_unknown_and_duplicates() {
        let cases = [
            ("{", "invalid-json"),
            ("{}", "invalid-top-level"),
            ("[1]", "invalid-element"),
            ("[\"unknown.capability\"]", "unknown-capability"),
            ("[\"ai.invoke\",\"ai.invoke\"]", "duplicate-capability"),
        ];
        for (permissions_json, suffix) in cases {
            let db = Database::init(":memory:").expect("create in-memory database");
            let mut manifest = manifest_with_permissions(&["ai.invoke"]);
            manifest.id = format!("com.firstwork.{}", suffix);
            db.record_plugin_version(
                &manifest,
                "C:/test/invalid-snapshot",
                "invalid-snapshot-hash",
                &manifest.permissions,
            )
            .expect("record current version");
            db.conn_lock()
                .expect("lock database")
                .execute(
                    "UPDATE plugin_versions SET permissions_json = ?2
                     WHERE plugin_id = ?1 AND is_current = 1",
                    params![manifest.id, permissions_json],
                )
                .expect("corrupt snapshot");
            assert!(
                matches!(
                    db.current_version_capability_snapshot(&manifest.id),
                    Err(AppError::PluginPermissionSnapshotInvalid { .. })
                ),
                "case {suffix} must fail closed"
            );
        }
    }

    #[test]
    fn permission_facts_reject_manifest_and_snapshot_semantic_mismatch() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["ai.invoke", "agents.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/mismatch",
            "mismatch-hash",
            &manifest.permissions,
        )
        .expect("record current version");
        db.conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions SET permissions_json = '[\"ai.invoke\"]'
                 WHERE plugin_id = ?1 AND is_current = 1",
                [&manifest.id],
            )
            .expect("remove snapshot capability");
        assert!(matches!(
            db.current_plugin_permission_facts(&manifest.id, &["ai.invoke"]),
            Err(AppError::PluginPermissionSnapshotMismatch { .. })
        ));
    }

    #[test]
    fn manifest_declaration_errors_are_distinct_from_snapshot_and_grant_errors() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["ai.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/manifest-invalid",
            "manifest-invalid-hash",
            &[],
        )
        .expect("record current version");
        db.conn_lock()
            .expect("lock database")
            .execute(
                "UPDATE plugin_versions SET manifest_json = '{'
                 WHERE plugin_id = ?1 AND is_current = 1",
                [&manifest.id],
            )
            .expect("corrupt manifest");
        assert!(matches!(
            db.current_manifest_capability_declaration(&manifest.id),
            Err(AppError::PluginManifestCapabilityDeclarationInvalid { .. })
        ));
        assert!(matches!(
            AppError::PluginCapabilityNotDeclared {
                plugin_id: manifest.id.clone(),
                capability: "agents.invoke".to_string(),
            },
            AppError::PluginCapabilityNotDeclared { .. }
        ));
        assert!(matches!(
            AppError::PluginPermissionDenied {
                plugin_id: Some(manifest.id),
                required_permission: Some("ai.invoke".to_string()),
            },
            AppError::PluginPermissionDenied { .. }
        ));
    }

    #[test]
    fn permission_fact_queries_have_no_permission_write_side_effects() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let manifest = manifest_with_permissions(&["ai.invoke", "agents.invoke"]);
        db.record_plugin_version(
            &manifest,
            "C:/test/read-only-facts",
            "read-only-facts-hash",
            &["ai.invoke".to_string()],
        )
        .expect("record current version");
        let read_rows = || {
            let conn = db.conn_lock().expect("lock database");
            let mut statement = conn
                .prepare(
                    "SELECT permission, granted FROM plugin_permissions
                     WHERE plugin_id = ?1 ORDER BY permission",
                )
                .expect("prepare permission rows");
            statement
                .query_map([&manifest.id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .expect("query permission rows")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect permission rows")
        };
        let before = read_rows();
        db.current_manifest_capability_declaration(&manifest.id)
            .expect("read manifest declaration");
        db.current_version_capability_snapshot(&manifest.id)
            .expect("read version snapshot");
        db.current_legacy_capability_authorization(&manifest.id, "ai.invoke")
            .expect("read legacy grant");
        db.current_plugin_permission_facts(&manifest.id, &["ai.invoke", "agents.invoke"])
            .expect("read all permission facts");
        assert_eq!(before, read_rows());
    }

    #[test]
    fn switch_plugin_version_replaces_existing_current_atomically() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let old = manifest_with_permissions(&[]);
        db.record_plugin_version(&old, "C:/test/v1", "hash-v1", &[])
            .expect("record old version");
        let mut current = old.clone();
        current.version = "2.0.0".into();
        db.record_plugin_version(&current, "C:/test/v2", "hash-v2", &[])
            .expect("record current version");

        let previous = db
            .switch_plugin_version(&old, "C:/test/v1", "hash-v1")
            .expect("switch to historical version");
        assert_eq!(previous.as_deref(), Some("2.0.0"));
        assert_eq!(
            db.current_plugin_version(&old.id)
                .expect("read current version")
                .as_deref(),
            Some("1.0.0")
        );

        let conn = db.conn_lock().expect("lock database");
        let current_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM plugin_versions
                 WHERE plugin_id = ?1 AND is_current = 1",
                [&old.id],
                |row| row.get(0),
            )
            .expect("count current versions");
        assert_eq!(current_count, 1);
        let old_current: bool = conn
            .query_row(
                "SELECT is_current FROM plugin_versions
                 WHERE plugin_id = ?1 AND version = '1.0.0'",
                [&old.id],
                |row| row.get(0),
            )
            .expect("read old version state");
        let new_current: bool = conn
            .query_row(
                "SELECT is_current FROM plugin_versions
                 WHERE plugin_id = ?1 AND version = '2.0.0'",
                [&old.id],
                |row| row.get(0),
            )
            .expect("read new version state");
        assert!(old_current);
        assert!(!new_current);
    }

    #[test]
    fn switch_plugin_version_missing_target_preserves_existing_current() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let current = manifest_with_permissions(&[]);
        db.record_plugin_version(&current, "C:/test/current", "current-hash", &[])
            .expect("record current version");
        let mut missing = current.clone();
        missing.version = "9.9.9".into();

        let error = db
            .switch_plugin_version(&missing, "C:/test/missing", "missing-hash")
            .expect_err("missing target must fail");
        assert!(matches!(error, AppError::NotFound(_)));
        assert_eq!(
            db.current_plugin_version(&current.id)
                .expect("read unchanged current version")
                .as_deref(),
            Some("1.0.0")
        );
        let conn = db.conn_lock().expect("lock database");
        let current_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM plugin_versions
                 WHERE plugin_id = ?1 AND is_current = 1",
                [&current.id],
                |row| row.get(0),
            )
            .expect("count current versions");
        assert_eq!(current_count, 1);
    }
}
