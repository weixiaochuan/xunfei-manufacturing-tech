//! 语音识别（ASR）服务层。
//!
//! 抽象出统一的入口 `AsrService`，按 `AsrProviderKind` 分发到具体实现。
//! 当前仅一家：阿里云百炼 DashScope（[`dashscope`]）。
//!
//! 配置元数据走 `app_config` 表 KV，前缀 `asr.*`；API Key 本体写入
//! secure-credentials，旧明文 `asr.api_key` 仅作为一次性迁移来源。

pub mod dashscope;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{AsrConfig, AsrProviderKind, AsrTestResult, TranscribeResult};
use crate::services::credentials::CredentialService;
use std::path::Path;

pub struct AsrService;

impl AsrService {
    const KEY_PROVIDER: &'static str = "asr.provider";
    const KEY_API_KEY: &'static str = "asr.api_key";
    const KEY_CREDENTIAL_ID: &'static str = "asr.credential_id";
    const KEY_MIGRATION_STATUS: &'static str = "asr.credential_migration_status";
    const KEY_MODEL: &'static str = "asr.model";
    const KEY_REGION: &'static str = "asr.region";
    const KEY_ENABLED: &'static str = "asr.enabled";
    const FIXED_CREDENTIAL_ID: &'static str = "asr-dashscope-api-key";

    /// 读取当前 ASR 配置。任意字段缺失走 `AsrConfig::default()`。
    pub fn get_config(db: &Database, data_dir: &Path) -> Result<AsrConfig, AppError> {
        let mut cfg = AsrConfig::default();
        if let Some(v) = db.get_config(Self::KEY_PROVIDER)? {
            if let Some(k) = AsrProviderKind::parse(&v) {
                cfg.provider = k;
            }
        }
        cfg.credential_id = db.get_config(Self::KEY_CREDENTIAL_ID)?;
        cfg.credential_configured = cfg.credential_id.is_some();
        cfg.credential_migration_status = db.get_config(Self::KEY_MIGRATION_STATUS)?;
        if let Some(v) = db.get_config(Self::KEY_MODEL)? {
            if !v.is_empty() {
                cfg.model = v;
            }
        }
        if let Some(v) = db.get_config(Self::KEY_REGION)? {
            if !v.is_empty() {
                cfg.region = v;
            }
        }
        if let Some(v) = db.get_config(Self::KEY_ENABLED)? {
            cfg.enabled = matches!(v.as_str(), "1" | "true");
        }
        if let Some(v) = db.get_config(Self::KEY_API_KEY)? {
            if !v.trim().is_empty() {
                match Self::store_api_key(db, data_dir, &v) {
                    Ok(id) => {
                        db.set_config(Self::KEY_CREDENTIAL_ID, &id)?;
                        db.set_config(Self::KEY_API_KEY, "")?;
                        db.set_config(Self::KEY_MIGRATION_STATUS, "migrated")?;
                        cfg.credential_id = Some(id);
                        cfg.credential_configured = true;
                        cfg.credential_migration_status = Some("migrated".into());
                    }
                    Err(_) => {
                        db.set_config(Self::KEY_MIGRATION_STATUS, "failed_retry_pending")?;
                        cfg.credential_migration_status = Some("failed_retry_pending".into());
                    }
                }
            }
        }
        Ok(cfg)
    }

    /// 保存配置；启用 = true 时强校验 api_key 非空，避免静默失败。
    pub fn save_config(db: &Database, data_dir: &Path, cfg: &AsrConfig) -> Result<(), AppError> {
        let mut credential_id = cfg.credential_id.clone();
        if !cfg.api_key.trim().is_empty() {
            credential_id = Some(Self::store_api_key(db, data_dir, &cfg.api_key)?);
            db.set_config(Self::KEY_MIGRATION_STATUS, "migrated")?;
        }
        if cfg.enabled && credential_id.is_none() {
            return Err(AppError::InvalidInput(
                "启用语音识别前必须填写 API Key".into(),
            ));
        }
        db.set_config(Self::KEY_PROVIDER, cfg.provider.as_str())?;
        db.set_config(Self::KEY_API_KEY, "")?;
        if let Some(id) = credential_id {
            db.set_config(Self::KEY_CREDENTIAL_ID, &id)?;
        }
        db.set_config(Self::KEY_MODEL, &cfg.model)?;
        db.set_config(Self::KEY_REGION, &cfg.region)?;
        db.set_config(Self::KEY_ENABLED, if cfg.enabled { "1" } else { "0" })?;
        Ok(())
    }

    /// 转录入口：从 DB 读配置 → 校验 → 派发到具体实现。
    pub async fn transcribe(
        db: &Database,
        data_dir: &Path,
        audio_b64: &str,
        mime: &str,
        language: Option<&str>,
    ) -> Result<TranscribeResult, AppError> {
        let mut cfg = Self::get_config(db, data_dir)?;
        if !cfg.enabled {
            return Err(AppError::InvalidInput("语音识别未启用".into()));
        }
        cfg.api_key = Self::load_configured_api_key(db, data_dir, &cfg)?;
        if cfg.api_key.trim().is_empty() {
            return Err(AppError::InvalidInput("尚未配置 API Key".into()));
        }
        match cfg.provider {
            AsrProviderKind::Dashscope => {
                dashscope::transcribe(&cfg, audio_b64, mime, language).await
            }
        }
    }

    /// 「测试连接」按钮专用：仅校验鉴权 / 端点可达，不真正消耗识别用量。
    pub async fn test_connection(cfg: &AsrConfig) -> AsrTestResult {
        let start = std::time::Instant::now();
        if cfg.api_key.trim().is_empty() {
            return AsrTestResult {
                ok: false,
                latency_ms: 0,
                message: Some("API Key 不能为空".into()),
            };
        }
        let result = match cfg.provider {
            AsrProviderKind::Dashscope => dashscope::probe(cfg).await,
        };
        match result {
            Ok(_) => AsrTestResult {
                ok: true,
                latency_ms: start.elapsed().as_millis() as u64,
                message: None,
            },
            Err(e) => AsrTestResult {
                ok: false,
                latency_ms: start.elapsed().as_millis() as u64,
                message: Some(e.to_string()),
            },
        }
    }

    fn store_api_key(db: &Database, data_dir: &Path, api_key: &str) -> Result<String, AppError> {
        CredentialService::upsert_api_key(
            db,
            data_dir,
            Self::FIXED_CREDENTIAL_ID,
            "dashscope",
            "ASR DashScope API Key",
            api_key,
        )?;
        Ok(Self::FIXED_CREDENTIAL_ID.to_string())
    }

    fn load_configured_api_key(
        db: &Database,
        data_dir: &Path,
        cfg: &AsrConfig,
    ) -> Result<String, AppError> {
        let id = cfg
            .credential_id
            .as_deref()
            .ok_or_else(|| AppError::InvalidInput("å°šæœªé…ç½® API Key".into()))?;
        Ok(CredentialService::load_api_key(db, data_dir, id)?.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db() -> (PathBuf, Database) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("firstwork-asr-test-{}", unique));
        fs::create_dir_all(&dir).unwrap();
        let db = Database::init(dir.join("app.db").to_str().unwrap()).unwrap();
        (dir, db)
    }

    #[test]
    fn asr_save_does_not_store_api_key_plaintext() {
        let (dir, db) = temp_db();
        let mut cfg = AsrConfig::default();
        cfg.api_key = "test-asr-secret-key".into();
        cfg.enabled = true;
        AsrService::save_config(&db, &dir, &cfg).unwrap();

        let stored = db.get_config(AsrService::KEY_API_KEY).unwrap();
        assert_eq!(stored.as_deref(), Some(""));
        let credential_id = db.get_config(AsrService::KEY_CREDENTIAL_ID).unwrap();
        assert_eq!(
            credential_id.as_deref(),
            Some(AsrService::FIXED_CREDENTIAL_ID)
        );
        let public_cfg = AsrService::get_config(&db, &dir).unwrap();
        assert!(public_cfg.api_key.is_empty());
        assert!(public_cfg.credential_configured);
    }

    #[test]
    fn asr_legacy_plaintext_key_migrates_without_duplication() {
        let (dir, db) = temp_db();
        db.set_config(AsrService::KEY_API_KEY, "legacy-asr-secret-key")
            .unwrap();
        let cfg = AsrService::get_config(&db, &dir).unwrap();
        assert!(cfg.api_key.is_empty());
        assert!(cfg.credential_configured);
        assert_eq!(cfg.credential_migration_status.as_deref(), Some("migrated"));
        assert_eq!(
            db.get_config(AsrService::KEY_API_KEY).unwrap().as_deref(),
            Some("")
        );

        let _ = AsrService::get_config(&db, &dir).unwrap();
        let conn = db.conn_lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE id = ?1",
                [AsrService::FIXED_CREDENTIAL_ID],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
