use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    CredentialCreateInput, CredentialInfo, CredentialSecretInput, CredentialUpdateInput,
    CredentialUsage,
};
use crate::services::crypto;

const SECRET_DIR: &str = "secure-credentials";
const DPAPI_PREFIX: &str = "dpapi:v1:";

pub struct CredentialService;

impl CredentialService {
    pub fn list(db: &Database) -> Result<Vec<CredentialInfo>, AppError> {
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, provider, credential_type, label, owner_scope, secret_reference,
                    configured, masked_hint, created_at, updated_at, last_used_at
             FROM credentials
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], credential_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create(
        db: &Database,
        data_dir: &Path,
        input: CredentialCreateInput,
    ) -> Result<CredentialInfo, AppError> {
        validate_provider(&input.provider)?;
        validate_label(&input.label)?;
        validate_secret_input(&input.secrets)?;

        let id = format!("cred-{}", Uuid::new_v4());
        let secret_reference = secret_reference(&id);
        write_secret(data_dir, &secret_reference, &input.secrets)?;
        let masked_hint = masked_hint(&input.secrets);
        let credential_type = serde_json::to_value(&input.credential_type)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "app_key_secret".to_string());

        {
            let conn = db.conn_lock()?;
            conn.execute(
                "INSERT INTO credentials
                    (id, provider, credential_type, label, owner_scope, secret_reference,
                     configured, masked_hint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
                params![
                    id,
                    input.provider,
                    credential_type,
                    input.label,
                    input.owner_scope,
                    secret_reference,
                    masked_hint
                ],
            )?;
        }
        Self::get(db, &id)?.ok_or_else(|| AppError::Custom("凭据创建后读取失败".to_string()))
    }

    pub fn update(
        db: &Database,
        data_dir: &Path,
        id: &str,
        input: CredentialUpdateInput,
    ) -> Result<CredentialInfo, AppError> {
        let existing = Self::get_internal(db, id)?
            .ok_or_else(|| AppError::Custom("凭据不存在".to_string()))?;
        if let Some(label) = &input.label {
            validate_label(label)?;
        }

        let mut configured = existing.configured;
        let mut masked = existing.masked_hint.clone();
        if input.clear_secret {
            delete_secret(data_dir, &existing.secret_reference)?;
            configured = false;
            masked = None;
        }
        if let Some(secrets) = &input.secrets {
            if has_any_secret(secrets) {
                validate_secret_input(secrets)?;
                write_secret(data_dir, &existing.secret_reference, secrets)?;
                configured = true;
                masked = masked_hint(secrets);
            }
        }

        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE credentials
             SET label = COALESCE(?2, label),
                 configured = ?3,
                 masked_hint = ?4,
                 updated_at = datetime('now','localtime')
             WHERE id = ?1",
            params![id, input.label, configured as i64, masked],
        )?;
        drop(conn);
        Self::get(db, id)?.ok_or_else(|| AppError::Custom("凭据更新后读取失败".to_string()))
    }

    pub fn delete(db: &Database, data_dir: &Path, id: &str, force: bool) -> Result<(), AppError> {
        let usage = Self::usage(db, id)?;
        if !usage.is_empty() && !force {
            return Err(AppError::Custom(
                "该凭据仍被智能体引用，请先解绑或确认级联失效".to_string(),
            ));
        }
        let existing = Self::get_internal(db, id)?
            .ok_or_else(|| AppError::Custom("凭据不存在".to_string()))?;
        let conn = db.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE external_agents
             SET credential_id = NULL, updated_at = datetime('now','localtime')
             WHERE credential_id = ?1
               AND COALESCE(unavailable_reason, '') = 'deleted'",
            params![id],
        )?;
        if force {
            tx.execute(
                "UPDATE external_agents
                 SET credential_id = NULL, enabled = 0, unavailable_reason = 'credential_deleted',
                     updated_at = datetime('now','localtime')
                 WHERE credential_id = ?1",
                params![id],
            )?;
        }
        tx.execute("DELETE FROM credentials WHERE id = ?1", params![id])?;
        tx.commit()?;
        drop(conn);
        delete_secret(data_dir, &existing.secret_reference)?;
        Ok(())
    }

    pub fn usage(db: &Database, id: &str) -> Result<Vec<CredentialUsage>, AppError> {
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT ea.id, ea.name, ea.product_id, p.name, ea.enabled
             FROM external_agents ea
             JOIN products p ON p.id = ea.product_id
             WHERE ea.credential_id = ?1
               AND COALESCE(ea.unavailable_reason, '') != 'deleted'
             ORDER BY ea.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(CredentialUsage {
                credential_id: id.to_string(),
                external_agent_id: row.get(0)?,
                agent_name: row.get(1)?,
                product_id: row.get(2)?,
                product_name: row.get(3)?,
                enabled: row.get::<_, i64>(4)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn load_secret(
        db: &Database,
        data_dir: &Path,
        id: &str,
    ) -> Result<CredentialSecretInput, AppError> {
        let existing = Self::get_internal(db, id)?
            .ok_or_else(|| AppError::Custom("credential_missing".to_string()))?;
        if !existing.configured {
            return Err(AppError::Custom("credential_missing".to_string()));
        }
        let path = secret_path(data_dir, &existing.secret_reference)?;
        let encrypted = fs::read_to_string(path)
            .map_err(|_| AppError::Custom("credential_missing".to_string()))?;
        let plain = decrypt_secret_payload(&encrypted)?;
        serde_json::from_str(&plain)
            .map_err(|_| AppError::Custom("credential_unavailable".to_string()))
    }

    pub fn touch_last_used(db: &Database, id: &str) -> Result<(), AppError> {
        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE credentials
             SET last_used_at = datetime('now','localtime'), updated_at = updated_at
             WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn upsert_api_key(
        db: &Database,
        data_dir: &Path,
        id: &str,
        provider: &str,
        label: &str,
        api_key: &str,
    ) -> Result<CredentialInfo, AppError> {
        validate_provider(provider)?;
        validate_label(label)?;
        let trimmed = api_key.trim();
        if trimmed.is_empty() {
            return Err(AppError::InvalidInput("API Key must not be empty".into()));
        }

        let secret_reference = secret_reference(id);
        let secrets = CredentialSecretInput {
            app_id: None,
            api_key: Some(trimmed.to_string()),
            api_secret: None,
            bearer_token: None,
        };
        write_secret(data_dir, &secret_reference, &secrets)?;
        let masked = masked_hint(&secrets);
        let conn = db.conn_lock()?;
        conn.execute(
            "INSERT INTO credentials
                (id, provider, credential_type, label, owner_scope, secret_reference,
                 configured, masked_hint)
             VALUES (?1, ?2, 'api_key', ?3, 'local-user', ?4, 1, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 provider = excluded.provider,
                 credential_type = excluded.credential_type,
                 label = excluded.label,
                 secret_reference = excluded.secret_reference,
                 configured = 1,
                 masked_hint = excluded.masked_hint,
                 updated_at = datetime('now','localtime')",
            params![id, provider, label, secret_reference, masked],
        )?;
        drop(conn);
        Self::get(db, id)?.ok_or_else(|| AppError::Custom("credential upsert failed".into()))
    }

    pub fn load_api_key(
        db: &Database,
        data_dir: &Path,
        id: &str,
    ) -> Result<Option<String>, AppError> {
        let secret = Self::load_secret(db, data_dir, id)?;
        Ok(secret
            .api_key
            .or(secret.bearer_token)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()))
    }

    fn get(db: &Database, id: &str) -> Result<Option<CredentialInfo>, AppError> {
        Ok(Self::get_internal(db, id)?.map(|c| c.into_public()))
    }

    fn get_internal(db: &Database, id: &str) -> Result<Option<CredentialInternal>, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, provider, credential_type, label, owner_scope, secret_reference,
                    configured, masked_hint, created_at, updated_at, last_used_at
             FROM credentials WHERE id = ?1",
            params![id],
            credential_internal_from_row,
        )
        .optional()
        .map_err(AppError::from)
    }
}

#[derive(Debug, Clone)]
struct CredentialInternal {
    id: String,
    provider: String,
    credential_type: String,
    label: String,
    owner_scope: String,
    secret_reference: String,
    configured: bool,
    masked_hint: Option<String>,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
}

impl CredentialInternal {
    fn into_public(self) -> CredentialInfo {
        let credential_type = serde_json::from_value(json!(self.credential_type))
            .unwrap_or(crate::models::CredentialType::AppKeySecret);
        CredentialInfo {
            id: self.id,
            provider: self.provider,
            credential_type,
            label: self.label,
            owner_scope: self.owner_scope,
            configured: self.configured,
            masked_hint: self.masked_hint,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_used_at: self.last_used_at,
        }
    }
}

fn credential_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialInfo> {
    Ok(credential_internal_from_row(row)?.into_public())
}

fn credential_internal_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialInternal> {
    Ok(CredentialInternal {
        id: row.get(0)?,
        provider: row.get(1)?,
        credential_type: row.get(2)?,
        label: row.get(3)?,
        owner_scope: row.get(4)?,
        secret_reference: row.get(5)?,
        configured: row.get::<_, i64>(6)? != 0,
        masked_hint: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_used_at: row.get(10)?,
    })
}

fn validate_provider(provider: &str) -> Result<(), AppError> {
    let ok = !provider.trim().is_empty()
        && provider.len() <= 80
        && provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(AppError::Custom("invalid credential provider".to_string()))
    }
}

fn validate_label(label: &str) -> Result<(), AppError> {
    if label.trim().is_empty() || label.len() > 80 {
        return Err(AppError::Custom(
            "凭据名称不能为空且不能超过80字符".to_string(),
        ));
    }
    Ok(())
}

fn validate_secret_input(input: &CredentialSecretInput) -> Result<(), AppError> {
    if !has_any_secret(input) {
        return Err(AppError::Custom("至少需要填写一个凭据字段".to_string()));
    }
    Ok(())
}

fn has_any_secret(input: &CredentialSecretInput) -> bool {
    [
        &input.app_id,
        &input.api_key,
        &input.api_secret,
        &input.bearer_token,
    ]
    .iter()
    .any(|v| v.as_ref().map(|s| !s.trim().is_empty()).unwrap_or(false))
}

fn masked_hint(input: &CredentialSecretInput) -> Option<String> {
    let candidate = input
        .bearer_token
        .as_ref()
        .or(input.api_key.as_ref())
        .or(input.api_secret.as_ref())
        .or(input.app_id.as_ref())?;
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        None
    } else {
        let suffix: String = trimmed
            .chars()
            .rev()
            .take(4)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        Some(format!("****{}", suffix))
    }
}

fn secret_reference(id: &str) -> String {
    format!("{}/{}.secret", SECRET_DIR, id)
}

fn secret_path(data_dir: &Path, reference: &str) -> Result<PathBuf, AppError> {
    if reference.contains("..") || reference.starts_with('/') || reference.contains(':') {
        return Err(AppError::Custom("非法凭据引用".to_string()));
    }
    Ok(data_dir.join(reference))
}

fn write_secret(
    data_dir: &Path,
    reference: &str,
    input: &CredentialSecretInput,
) -> Result<(), AppError> {
    let path = secret_path(data_dir, reference)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let plain = serde_json::to_string(input)
        .map_err(|e| AppError::Custom(format!("凭据序列化失败: {}", e)))?;
    let encrypted = encrypt_secret_payload(&plain)?;
    fs::write(path, encrypted)?;
    Ok(())
}

fn delete_secret(data_dir: &Path, reference: &str) -> Result<(), AppError> {
    let path = secret_path(data_dir, reference)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn encrypt_secret_payload(plain: &str) -> Result<String, AppError> {
    #[cfg(windows)]
    {
        let protected = dpapi_protect(plain.as_bytes())?;
        return Ok(format!(
            "{}{}",
            DPAPI_PREFIX,
            base64::engine::general_purpose::STANDARD.encode(protected)
        ));
    }

    #[cfg(not(windows))]
    {
        crypto::encrypt(plain)
    }
}

fn decrypt_secret_payload(encrypted: &str) -> Result<String, AppError> {
    if let Some(payload) = encrypted.trim().strip_prefix(DPAPI_PREFIX) {
        #[cfg(windows)]
        {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(payload)
                .map_err(|_| AppError::Custom("credential_unavailable".to_string()))?;
            let plain = dpapi_unprotect(&bytes)?;
            return String::from_utf8(plain)
                .map_err(|_| AppError::Custom("credential_unavailable".to_string()));
        }

        #[cfg(not(windows))]
        {
            let _ = payload;
            return Err(AppError::Custom(
                "credential_unavailable: DPAPI protected credential can only be read on Windows"
                    .to_string(),
            ));
        }
    }

    // Backward compatibility for credentials saved before the DPAPI upgrade.
    crypto::decrypt(encrypted).map_err(|_| AppError::Custom("credential_unavailable".to_string()))
}

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, AppError> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            null(),
            null_mut(),
            null_mut(),
            null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(AppError::Custom("credential_store_unavailable".to_string()));
    }
    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(protected)
}

#[cfg(windows)]
fn dpapi_unprotect(protected: &[u8]) -> Result<Vec<u8>, AppError> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: protected.len() as u32,
        pbData: protected.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(AppError::Custom("credential_unavailable".to_string()));
    }
    let plain =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db() -> (PathBuf, Database) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("firstwork-cred-test-{}", unique));
        fs::create_dir_all(&dir).unwrap();
        let db = Database::init(dir.join("app.db").to_str().unwrap()).unwrap();
        (dir, db)
    }

    #[test]
    fn credential_secret_not_stored_in_sqlite() {
        let (dir, db) = temp_db();
        let created = CredentialService::create(
            &db,
            &dir,
            CredentialCreateInput {
                provider: "xingchen".into(),
                credential_type: crate::models::CredentialType::BearerToken,
                label: "test".into(),
                owner_scope: "local-user".into(),
                secrets: CredentialSecretInput {
                    app_id: None,
                    api_key: None,
                    api_secret: None,
                    bearer_token: Some("super-secret-token".into()),
                },
            },
        )
        .unwrap();
        assert_eq!(created.masked_hint.as_deref(), Some("****oken"));
        let listed = CredentialService::list(&db).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
        assert!(listed[0].configured);
        assert_eq!(listed[0].masked_hint.as_deref(), Some("****oken"));
        let conn = db.conn_lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE secret_reference LIKE '%super-secret-token%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_credential_clears_soft_deleted_agent_reference() {
        let (dir, db) = temp_db();
        let created = CredentialService::create(
            &db,
            &dir,
            CredentialCreateInput {
                provider: "xingchen".into(),
                credential_type: crate::models::CredentialType::AppKeySecret,
                label: "workflow".into(),
                owner_scope: "local-user".into(),
                secrets: CredentialSecretInput {
                    app_id: Some("app".into()),
                    api_key: Some("key".into()),
                    api_secret: Some("secret".into()),
                    bearer_token: None,
                },
            },
        )
        .unwrap();
        {
            let conn = db.conn_lock().unwrap();
            conn.execute(
                "INSERT INTO products
                    (id, developer_id, name, description, product_type, runtime_kind, status)
                 VALUES ('product-xingchen', 'dev', 'workflow', '', 'xingchen-agent', 'xingchen-agent', 'published')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO external_agents
                    (id, product_id, provider, name, endpoint, authentication_type,
                     credential_id, streaming_type, request_mapping_json, response_mapping_json,
                     session_mapping_json, error_mapping_json, mock_mode, enabled, unavailable_reason)
                 VALUES
                    ('agent-deleted', 'product-xingchen', 'xingchen', 'deleted agent',
                     'https://xingchen-api.xf-yun.com/workflow/v1/chat/completions',
                     'bearer', ?1, 'sse', '{}', '{}', '{}', '{}', 0, 0, 'deleted')",
                params![created.id],
            )
            .unwrap();
        }

        assert!(CredentialService::usage(&db, &created.id)
            .unwrap()
            .is_empty());
        CredentialService::delete(&db, &dir, &created.id, false).unwrap();
        let conn = db.conn_lock().unwrap();
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE id = ?1",
                params![created.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
        let agent_credential: Option<String> = conn
            .query_row(
                "SELECT credential_id FROM external_agents WHERE id = 'agent-deleted'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(agent_credential.is_none());
    }
}
