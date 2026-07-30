//! 插件系统 DAO
//!
//! 仅负责插件元数据、权限授权与插件设置的 SQLite 持久化。
//! 插件目录扫描、manifest 校验、文件复制/删除都放在 Service 层。

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use super::Database;
use crate::error::AppError;
use crate::models::{
    NormalizedPluginManifest, PluginInfo, PluginInstallationInfo, PluginManifest,
    PluginManifestFormat, PluginRuntimeKind, PluginSource, ProductType, SignatureStatus,
};

const SELECT_COLUMNS: &str = "id, name, version, description, author, path, main, styles, \
     min_app_version, manifest_json, enabled, status, installed_at, updated_at, content_hash, \
     manifest_format, schema_version, product_type, runtime_kind, source, signature_status";

fn list_plugin_permissions(
    conn: &Connection,
    plugin_id: &str,
) -> Result<(Vec<String>, Vec<String>), AppError> {
    let mut stmt = conn.prepare(
        "SELECT permission, granted FROM plugin_permissions
         WHERE plugin_id = ?1
         ORDER BY permission ASC",
    )?;
    let rows = stmt
        .query_map([plugin_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? != 0))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let permissions = rows.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();
    let granted_permissions = rows
        .into_iter()
        .filter_map(|(p, granted)| granted.then_some(p))
        .collect::<Vec<_>>();

    Ok((permissions, granted_permissions))
}

fn parse_or_default<T>(value: String) -> T
where
    T: for<'de> serde::Deserialize<'de> + Default,
{
    serde_json::from_value(serde_json::Value::String(value)).unwrap_or_default()
}

fn serde_enum_string<T: serde::Serialize>(value: &T, fallback: &str) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| fallback.to_string())
}

fn row_to_plugin(conn: &Connection, row: &rusqlite::Row<'_>) -> Result<PluginInfo, AppError> {
    let id: String = row.get(0)?;
    let manifest_json: String = row.get(9)?;
    let manifest: PluginManifest = serde_json::from_str(&manifest_json)?;
    let (permissions, granted_permissions) = list_plugin_permissions(conn, &id)?;
    let manifest_format = parse_or_default::<PluginManifestFormat>(row.get(15)?);
    let product_type = parse_or_default::<ProductType>(row.get(17)?);
    let runtime_kind = parse_or_default::<PluginRuntimeKind>(row.get(18)?);
    let source = parse_or_default::<PluginSource>(row.get(19)?);
    let signature_status = parse_or_default::<SignatureStatus>(row.get(20)?);

    Ok(PluginInfo {
        id,
        name: row.get(1)?,
        version: row.get(2)?,
        description: row.get(3)?,
        author: row.get(4)?,
        path: row.get(5)?,
        main: row.get(6)?,
        styles: row.get(7)?,
        min_app_version: row.get(8)?,
        enabled: row.get::<_, i32>(10)? != 0,
        status: row.get(11)?,
        permissions,
        granted_permissions,
        manifest,
        installed_at: row.get(12)?,

        updated_at: row.get(13)?,
        content_hash: row.get(14).unwrap_or_default(),
        manifest_format,
        schema_version: row.get::<_, i64>(16).unwrap_or(1) as u32,
        product_type,
        runtime_kind,
        source,
        signature_status,
        integrity_status: "not_checked".into(),
        can_execute: false,
        blocked_reason: None,
        raw_invoke_allowed: false,
        installation: None,
    })
}

impl Database {
    /// 列出已安装插件
    pub fn list_plugins(&self) -> Result<Vec<PluginInfo>, AppError> {
        let conn = self.conn_lock()?;
        let sql = format!(
            "SELECT {} FROM plugins ORDER BY name COLLATE NOCASE",
            SELECT_COLUMNS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut plugins = Vec::new();
        while let Some(row) = rows.next()? {
            plugins.push(row_to_plugin(&conn, row)?);
        }
        Ok(plugins)
    }

    /// 获取单个插件
    pub fn get_plugin(&self, plugin_id: &str) -> Result<PluginInfo, AppError> {
        let conn = self.conn_lock()?;
        let sql = format!("SELECT {} FROM plugins WHERE id = ?1", SELECT_COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([plugin_id])?;
        match rows.next()? {
            Some(row) => row_to_plugin(&conn, row),
            None => Err(AppError::NotFound(format!("插件 {} 不存在", plugin_id))),
        }
    }

    /// 插入或更新插件元数据；enabled 状态保留用户原设置
    pub fn upsert_plugin(
        &self,
        manifest: &NormalizedPluginManifest,
        plugin_path: &str,
        content_hash: &str,
    ) -> Result<PluginInfo, AppError> {
        let legacy = &manifest.legacy_manifest;
        let manifest_json = serde_json::to_string(legacy)?;
        let manifest_format = serde_enum_string(&manifest.format, "legacy");
        let product_type = serde_enum_string(&manifest.product_type, "local-plugin");
        let runtime_kind = serde_enum_string(&manifest.runtime_kind, "legacy-js");
        let source = serde_enum_string(&manifest.source, "local");
        let signature_status = serde_enum_string(&manifest.signature.status, "unsigned");
        let main = manifest.main.clone().unwrap_or_default();
        let conn = self.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO plugins
                (id, name, version, description, author, path, main, styles,
                 min_app_version, manifest_json, enabled, status,
                 content_hash, manifest_format, schema_version, product_type, runtime_kind,
                 source, signature_status, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 'installed',
                     ?11, ?12, ?13, ?14, ?15, ?16, ?17, datetime('now','localtime'), datetime('now','localtime'))
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 version = excluded.version,
                 description = excluded.description,
                 author = excluded.author,
                 path = excluded.path,
                 main = excluded.main,
                 styles = excluded.styles,
                 min_app_version = excluded.min_app_version,
                 manifest_json = excluded.manifest_json,
                 content_hash = excluded.content_hash,
                 manifest_format = excluded.manifest_format,
                 schema_version = excluded.schema_version,
                 product_type = excluded.product_type,
                 runtime_kind = excluded.runtime_kind,
                 source = excluded.source,
                 signature_status = excluded.signature_status,
                 status = 'installed',
                 updated_at = excluded.updated_at",
            params![
                manifest.id,
                manifest.name,
                manifest.version,
                manifest.description.as_deref(),
                manifest.author_id.as_deref(),
                plugin_path,
                main,
                manifest.styles.as_deref(),
                manifest.min_app_version.as_deref(),
                manifest_json,
                content_hash,
                manifest_format,
                manifest.schema_version as i64,
                product_type,
                runtime_kind,
                source,
                signature_status,
            ],
        )?;

        let requested = manifest
            .permissions
            .iter()
            .cloned()
            .collect::<HashSet<String>>();
        for permission in &requested {
            tx.execute(
                "INSERT OR IGNORE INTO plugin_permissions
                    (plugin_id, permission, granted, created_at, updated_at)
                 VALUES (?1, ?2, 0, datetime('now','localtime'), datetime('now','localtime'))",
                params![manifest.id, permission],
            )?;
        }

        let existing = tx
            .prepare("SELECT permission FROM plugin_permissions WHERE plugin_id = ?1")?
            .query_map([manifest.id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for permission in existing {
            if !requested.contains(&permission) {
                tx.execute(
                    "DELETE FROM plugin_permissions WHERE plugin_id = ?1 AND permission = ?2",
                    params![manifest.id, permission],
                )?;
            }
        }

        let signature_status = serde_enum_string(&manifest.signature.status, "unsigned");
        let product_type = serde_enum_string(&manifest.product_type, "local-plugin");
        let runtime_kind = serde_enum_string(&manifest.runtime_kind, "legacy-js");
        let source = serde_enum_string(&manifest.source, "local");

        tx.execute(
            "INSERT INTO products
                (id, developer_id, name, description, product_type, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'installed', datetime('now','localtime'), datetime('now','localtime'))
             ON CONFLICT(id) DO UPDATE SET
                developer_id = excluded.developer_id,
                name = excluded.name,
                description = excluded.description,
                product_type = excluded.product_type,
                updated_at = excluded.updated_at",
            params![
                manifest.id,
                manifest.author_id.clone().unwrap_or_else(|| "legacy".into()),
                manifest.name,
                manifest.description.as_deref(),
                product_type,
            ],
        )?;

        tx.execute(
            "INSERT INTO product_versions
                (product_id, version, manifest_json, runtime_kind, source, content_hash,
                 signature_status, min_app_version, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now','localtime'))
             ON CONFLICT(product_id, version) DO UPDATE SET
                manifest_json = excluded.manifest_json,
                runtime_kind = excluded.runtime_kind,
                source = excluded.source,
                content_hash = excluded.content_hash,
                signature_status = excluded.signature_status,
                min_app_version = excluded.min_app_version",
            params![
                manifest.id,
                manifest.version,
                serde_json::to_string(manifest)?,
                runtime_kind,
                source,
                content_hash,
                signature_status,
                manifest.min_app_version.as_deref(),
            ],
        )?;
        let product_version_id: i64 = tx.query_row(
            "SELECT id FROM product_versions WHERE product_id = ?1 AND version = ?2",
            params![manifest.id, manifest.version],
            |row| row.get(0),
        )?;

        tx.execute(
            "DELETE FROM product_permissions WHERE product_version_id = ?1",
            [product_version_id],
        )?;
        for permission in &requested {
            tx.execute(
                "INSERT INTO product_permissions
                    (product_version_id, permission, required, reason)
                 VALUES (?1, ?2, 1, NULL)",
                params![product_version_id, permission],
            )?;
        }

        let source = serde_enum_string(&manifest.source, "local");
        tx.execute(
            "INSERT INTO plugin_installations
                (plugin_id, product_id, product_version_id, installed_version, source, enabled,
                 install_path, content_hash, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, datetime('now','localtime'), datetime('now','localtime'))
             ON CONFLICT(plugin_id) DO UPDATE SET
                product_id = excluded.product_id,
                product_version_id = excluded.product_version_id,
                installed_version = excluded.installed_version,
                source = excluded.source,
                install_path = excluded.install_path,
                content_hash = excluded.content_hash,
                updated_at = excluded.updated_at",
            params![
                manifest.id,
                manifest.id,
                product_version_id,
                manifest.version,
                source,
                plugin_path,
                content_hash,
            ],
        )?;

        tx.execute(
            "INSERT OR IGNORE INTO entitlements
                (product_id, entitlement_type, status, issued_at, expires_at)
             VALUES (?1, 'free', 'active', datetime('now','localtime'), NULL)",
            [manifest.id.as_str()],
        )?;

        tx.commit()?;
        drop(conn);
        self.get_plugin(&manifest.id)
    }

    /// 设置插件启用状态
    pub fn set_plugin_enabled(&self, plugin_id: &str, enabled: bool) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        let affected = conn.execute(
            "UPDATE plugins
             SET enabled = ?1, updated_at = datetime('now','localtime')
             WHERE id = ?2",
            params![i32::from(enabled), plugin_id],
        )?;
        if affected == 0 {
            return Err(AppError::NotFound(format!("插件 {} 不存在", plugin_id)));
        }
        conn.execute(
            "UPDATE plugin_installations
             SET enabled = ?1, updated_at = datetime('now','localtime')
             WHERE plugin_id = ?2",
            params![i32::from(enabled), plugin_id],
        )?;
        Ok(())
    }

    /// 删除插件元数据
    pub fn delete_plugin(&self, plugin_id: &str) -> Result<bool, AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "DELETE FROM plugin_installations WHERE plugin_id = ?1",
            [plugin_id],
        )?;
        let affected = conn.execute("DELETE FROM plugins WHERE id = ?1", [plugin_id])?;
        Ok(affected > 0)
    }

    /// 授权指定权限
    pub fn grant_plugin_permissions(
        &self,
        plugin_id: &str,
        permissions: &[String],
    ) -> Result<usize, AppError> {
        let conn = self.conn_lock()?;
        let mut affected = 0;
        for permission in permissions {
            affected += conn.execute(
                "UPDATE plugin_permissions
                 SET granted = 1, updated_at = datetime('now','localtime')
                 WHERE plugin_id = ?1 AND permission = ?2",
                params![plugin_id, permission],
            )?;
        }
        Ok(affected)
    }

    /// 撤销指定权限
    pub fn revoke_plugin_permissions(
        &self,
        plugin_id: &str,
        permissions: &[String],
    ) -> Result<usize, AppError> {
        let conn = self.conn_lock()?;
        let mut affected = 0;
        for permission in permissions {
            affected += conn.execute(
                "UPDATE plugin_permissions
                 SET granted = 0, updated_at = datetime('now','localtime')
                 WHERE plugin_id = ?1 AND permission = ?2",
                params![plugin_id, permission],
            )?;
        }
        Ok(affected)
    }

    /// 读取插件设置，value 按 JSON 反序列化返回
    pub fn get_plugin_settings(
        &self,
        plugin_id: &str,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        let conn = self.conn_lock()?;
        let exists: Option<i32> = conn
            .query_row("SELECT 1 FROM plugins WHERE id = ?1", [plugin_id], |row| {
                row.get(0)
            })
            .optional()?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("插件 {} 不存在", plugin_id)));
        }

        let mut stmt = conn.prepare(
            "SELECT key, value FROM plugin_settings
             WHERE plugin_id = ?1
             ORDER BY key ASC",
        )?;
        let rows = stmt
            .query_map([plugin_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut map = HashMap::new();
        for (key, value) in rows {
            let parsed =
                serde_json::from_str(&value).unwrap_or_else(|_| serde_json::Value::String(value));
            map.insert(key, parsed);
        }
        Ok(map)
    }

    /// 全量替换插件设置
    pub fn set_plugin_settings(
        &self,
        plugin_id: &str,
        settings: &HashMap<String, serde_json::Value>,
    ) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        let exists: Option<i32> = conn
            .query_row("SELECT 1 FROM plugins WHERE id = ?1", [plugin_id], |row| {
                row.get(0)
            })
            .optional()?;
        if exists.is_none() {
            return Err(AppError::NotFound(format!("插件 {} 不存在", plugin_id)));
        }

        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM plugin_settings WHERE plugin_id = ?1",
            [plugin_id],
        )?;
        for (key, value) in settings {
            tx.execute(
                "INSERT INTO plugin_settings (plugin_id, key, value, updated_at)
                 VALUES (?1, ?2, ?3, datetime('now','localtime'))",
                params![plugin_id, key, serde_json::to_string(value)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 检查插件是否已获得指定权限（T2: plugin_proxy 权限校验用）
    pub fn has_plugin_permission(&self, plugin_id: &str, perm: &str) -> Result<bool, AppError> {
        Ok(self
            .plugin_permission_grant_state(plugin_id, perm)?
            .unwrap_or(false))
    }

    /// 查询单项用户授权记录，区分已授权、已撤权和记录不存在。
    pub fn plugin_permission_grant_state(
        &self,
        plugin_id: &str,
        perm: &str,
    ) -> Result<Option<bool>, AppError> {
        let conn = self.conn_lock()?;
        let granted: Option<i32> = conn
            .query_row(
                "SELECT granted FROM plugin_permissions
                 WHERE plugin_id = ?1 AND permission = ?2",
                params![plugin_id, perm],
                |row| row.get(0),
            )
            .optional()?;
        Ok(granted.map(|value| value != 0))
    }

    /// 查询单个插件设置（T2: plugin_proxy_settings_get 用）
    pub fn get_plugin_setting(
        &self,
        plugin_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let conn = self.conn_lock()?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM plugin_settings
                 WHERE plugin_id = ?1 AND key = ?2",
                params![plugin_id, key],
                |row| row.get(0),
            )
            .optional()?;
        match value {
            Some(v) => Ok(Some(
                serde_json::from_str(&v).unwrap_or(serde_json::Value::String(v)),
            )),
            None => Ok(None),
        }
    }

    /// Upsert 单个插件设置（T2: plugin_proxy_settings_set 用）
    pub fn set_plugin_setting(
        &self,
        plugin_id: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        let value_str = serde_json::to_string(value)?;
        conn.execute(
            "INSERT INTO plugin_settings (plugin_id, key, value, updated_at)
             VALUES (?1, ?2, ?3, datetime('now','localtime'))
             ON CONFLICT(plugin_id, key) DO UPDATE SET
                 value = excluded.value,
                 updated_at = excluded.updated_at",
            params![plugin_id, key, value_str],
        )?;
        Ok(())
    }

    /// T25: 写入插件审计日志
    pub fn write_audit_log(
        &self,
        plugin_id: &str,
        operation: &str,
        target: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO plugin_audit_log (plugin_id, operation, target, timestamp)
             VALUES (?1, ?2, ?3, datetime('now','localtime'))",
            params![plugin_id, operation, target],
        )?;
        Ok(())
    }

    /// T25: 查询某插件的审计日志（最近 N 条）
    pub fn get_plugin_audit_log(
        &self,
        plugin_id: &str,
        limit: u32,
    ) -> Result<Vec<(i64, String, String, Option<String>, String)>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, plugin_id, operation, target, timestamp
             FROM plugin_audit_log
             WHERE plugin_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![plugin_id, limit], |row| {
                Ok((
                    row.get(0)?, // id
                    row.get(1)?, // plugin_id
                    row.get(2)?, // operation
                    row.get(3)?, // target
                    row.get(4)?, // timestamp
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_plugin_installation(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginInstallationInfo>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, plugin_id, product_id, product_version_id, installed_version, source,
                    enabled, install_path, content_hash, installed_at, updated_at
             FROM plugin_installations
             WHERE plugin_id = ?1",
        )?;
        let installation = stmt
            .query_row([plugin_id], |row| {
                let source: String = row.get(5)?;
                Ok(PluginInstallationInfo {
                    id: row.get(0)?,
                    plugin_id: row.get(1)?,
                    product_id: row.get(2)?,
                    product_version_id: row.get(3)?,
                    installed_version: row.get(4)?,
                    source: parse_or_default(source),
                    enabled: row.get::<_, i32>(6)? != 0,
                    install_path: row.get(7)?,
                    content_hash: row.get(8)?,
                    installed_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .optional()?;
        Ok(installation)
    }

    pub fn list_plugin_installations(&self) -> Result<Vec<PluginInstallationInfo>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, plugin_id, product_id, product_version_id, installed_version, source,
                    enabled, install_path, content_hash, installed_at, updated_at
             FROM plugin_installations
             ORDER BY plugin_id ASC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let source: String = row.get(5)?;
                Ok(PluginInstallationInfo {
                    id: row.get(0)?,
                    plugin_id: row.get(1)?,
                    product_id: row.get(2)?,
                    product_version_id: row.get(3)?,
                    installed_version: row.get(4)?,
                    source: parse_or_default(source),
                    enabled: row.get::<_, i32>(6)? != 0,
                    install_path: row.get(7)?,
                    content_hash: row.get(8)?,
                    installed_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// T26: 查询插件的 content_hash
    pub fn get_plugin_content_hash(&self, plugin_id: &str) -> Result<Option<String>, AppError> {
        let conn = self.conn_lock()?;
        Ok(conn
            .query_row(
                "SELECT content_hash FROM plugins WHERE id = ?1",
                [plugin_id],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    /// 创建测试用 Database（:memory: 模式，不走文件系统）
    fn setup_test_db() -> Database {
        Database::init(":memory:").expect("创建内存数据库失败")
    }

    /// 向测试数据库插入一个测试插件
    fn insert_test_plugin(db: &Database, id: &str, enabled: bool, status: &str) {
        let conn = db.conn_lock().unwrap();
        let manifest_json = format!(
            r#"{{"id":"{}","name":"{}","version":"1.0.0","description":"","author":"","main":"main.js","permissions":[],"contributes":{{}}}}"#,
            id, id
        );
        conn.execute(
            "INSERT INTO plugins (id, name, version, description, author, path, main, manifest_json, enabled, status)
             VALUES (?1, ?2, '1.0.0', '', '', '/tmp', 'main.js', ?3, ?4, ?5)",
            params![id, id, manifest_json, enabled as i32, status],
        )
        .unwrap();
    }

    /// 向测试数据库插入一条权限授权记录
    fn grant_permission(db: &Database, plugin_id: &str, permission: &str) {
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO plugin_permissions (plugin_id, permission, granted)
             VALUES (?1, ?2, 1)",
            params![plugin_id, permission],
        )
        .unwrap();
    }

    fn set_permission_state(db: &Database, plugin_id: &str, permission: &str, granted: bool) {
        let conn = db.conn_lock().unwrap();
        conn.execute(
            "INSERT INTO plugin_permissions (plugin_id, permission, granted)
             VALUES (?1, ?2, ?3)",
            params![plugin_id, permission, i32::from(granted)],
        )
        .unwrap();
    }

    #[test]
    fn plugin_permission_grant_state_distinguishes_granted_revoked_and_missing() {
        let db = setup_test_db();
        insert_test_plugin(&db, "permission-state", true, "installed");
        set_permission_state(&db, "permission-state", "ai.invoke", true);
        set_permission_state(&db, "permission-state", "agents.invoke", false);

        assert_eq!(
            db.plugin_permission_grant_state("permission-state", "ai.invoke")
                .unwrap(),
            Some(true)
        );
        assert_eq!(
            db.plugin_permission_grant_state("permission-state", "agents.invoke")
                .unwrap(),
            Some(false)
        );
        assert_eq!(
            db.plugin_permission_grant_state("permission-state", "credentials.use")
                .unwrap(),
            None
        );
    }

    // ─── §10.15 越权测试用例（针对 Database 层的 has_plugin_permission） ───

    /// 用例 5：有效令牌 + 缺 notes:read → has_plugin_permission 返回 false
    #[test]
    fn case_5_missing_notes_read() {
        let db = setup_test_db();
        insert_test_plugin(&db, "cc", true, "installed");
        let has = db.has_plugin_permission("cc", "notes:read").unwrap();
        assert!(!has, "未授权 notes:read 应返回 false");
    }

    /// 用例 6：有效令牌 + 缺 notes:write（仅授 notes:read）→ false
    #[test]
    fn case_6_missing_notes_write() {
        let db = setup_test_db();
        insert_test_plugin(&db, "cc", true, "installed");
        grant_permission(&db, "cc", "notes:read");
        let has = db.has_plugin_permission("cc", "notes:write").unwrap();
        assert!(!has, "缺 notes:write 应返回 false");
    }

    /// 用例 7：有效令牌 + 完整权限 → has_plugin_permission 返回 true
    #[test]
    fn case_7_full_permissions() {
        let db = setup_test_db();
        insert_test_plugin(&db, "cc", true, "installed");
        grant_permission(&db, "cc", "notes:read");
        let has = db.has_plugin_permission("cc", "notes:read").unwrap();
        assert!(has, "已授权 notes:read 应返回 true");
    }

    /// 不存在的插件查询权限 → 返回 false（不 panic）
    #[test]
    fn case_nonexistent_plugin_permission_false() {
        let db = setup_test_db();
        let has = db
            .has_plugin_permission("nonexistent", "notes:read")
            .unwrap();
        assert!(!has, "不存在的插件应返回 false");
    }

    /// 用例 8：跨插件设置隔离
    /// A 插件的 settings_get_all 应只返回 A 的设置，不返回 B 的
    #[test]
    fn case_8_plugin_settings_isolation() {
        let db = setup_test_db();
        insert_test_plugin(&db, "plugin-a", true, "installed");
        insert_test_plugin(&db, "plugin-b", true, "installed");

        db.set_plugin_setting(
            "plugin-a",
            "keyA",
            &serde_json::Value::String("valueA".into()),
        )
        .unwrap();
        db.set_plugin_setting(
            "plugin-b",
            "keyB",
            &serde_json::Value::String("valueB".into()),
        )
        .unwrap();

        let a_settings = db.get_plugin_settings("plugin-a").unwrap();
        assert_eq!(a_settings.get("keyA").unwrap(), "valueA");
        assert!(!a_settings.contains_key("keyB"), "A 不应看到 B 的设置");

        let b_settings = db.get_plugin_settings("plugin-b").unwrap();
        assert_eq!(b_settings.get("keyB").unwrap(), "valueB");
        assert!(!b_settings.contains_key("keyA"), "B 不应看到 A 的设置");
    }

    /// 设置持久化和再次读取的一致性（Upsert 覆盖）
    #[test]
    fn case_settings_upsert_consistency() {
        let db = setup_test_db();
        insert_test_plugin(&db, "cc", true, "installed");

        db.set_plugin_setting("cc", "theme", &serde_json::Value::String("dark".into()))
            .unwrap();
        let v1 = db.get_plugin_setting("cc", "theme").unwrap();
        assert_eq!(v1.unwrap(), "dark");

        db.set_plugin_setting("cc", "theme", &serde_json::Value::String("light".into()))
            .unwrap();
        let v2 = db.get_plugin_setting("cc", "theme").unwrap();
        assert_eq!(v2.unwrap(), "light");
    }
}
