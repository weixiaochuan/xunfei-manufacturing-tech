use rusqlite::{params, OptionalExtension, TransactionBehavior};
use uuid::{Uuid, Version};

use crate::error::AppError;
use crate::models::plugin_platform::{PluginAuthorizationContext, PluginAuthorizationContextKind};

use super::Database;

fn parse_host_installation_id(value: &str) -> Result<Uuid, AppError> {
    let id = Uuid::parse_str(value).map_err(|_| AppError::PluginAuthorizationContextInvalid {
        reason: "host_installation_id_corrupt",
    })?;
    if id.get_version() != Some(Version::Random) || id.to_string() != value {
        return Err(AppError::PluginAuthorizationContextInvalid {
            reason: "host_installation_id_corrupt",
        });
    }
    Ok(id)
}

impl Database {
    /// 原子取得宿主安装身份；已有值损坏时拒绝，不会静默轮换身份。
    pub(crate) fn stable_host_installation_context(
        &self,
    ) -> Result<PluginAuthorizationContext, AppError> {
        let mut conn = self.conn_lock()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                "SELECT installation_id FROM host_installation_identity WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        let id = match existing {
            Some(value) => parse_host_installation_id(&value)?,
            None => {
                let generated = Uuid::new_v4();
                tx.execute(
                    "INSERT INTO host_installation_identity (singleton, installation_id)
                     VALUES (1, ?1)",
                    params![generated.to_string()],
                )?;
                generated
            }
        };
        tx.commit()?;
        Ok(PluginAuthorizationContext {
            kind: PluginAuthorizationContextKind::HostInstallation,
            id: id.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};

    use super::*;

    #[test]
    fn host_installation_context_is_stable_and_not_derived_from_plugin_identity() {
        let db = Database::init(":memory:").expect("database");
        let first = db.stable_host_installation_context().expect("first");
        let second = db.stable_host_installation_context().expect("second");
        assert_eq!(first, second);
        assert_eq!(first.kind, PluginAuthorizationContextKind::HostInstallation);
        assert_ne!(first.id, "local-demo-buyer");
        assert_ne!(first.id, "plugin-installation-id");
    }

    #[test]
    fn concurrent_initialization_persists_one_host_identity() {
        let db = Arc::new(Database::init(":memory:").expect("database"));
        let barrier = Arc::new(Barrier::new(8));
        let handles = (0..8)
            .map(|_| {
                let db = Arc::clone(&db);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    db.stable_host_installation_context().expect("context").id
                })
            })
            .collect::<Vec<_>>();
        let ids = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect::<Vec<_>>();
        assert!(ids.iter().all(|id| id == &ids[0]));
    }

    #[test]
    fn corrupt_host_identity_variants_fail_closed_without_rotation() {
        for corrupt in [
            "not-a-uuid",
            "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "01890f5e-7b5c-7cc2-98c4-dc0c0c07398f",
            "550E8400-E29B-41D4-A716-446655440000",
            "550e8400e29b41d4a716446655440000",
            " 550e8400-e29b-41d4-a716-446655440000 ",
        ] {
            let db = Database::init(":memory:").expect("database");
            db.conn_lock()
                .expect("connection")
                .execute(
                    "INSERT INTO host_installation_identity(singleton, installation_id)
                     VALUES (1, ?1)",
                    [corrupt],
                )
                .expect("inject corrupt value");
            assert!(matches!(
                db.stable_host_installation_context(),
                Err(AppError::PluginAuthorizationContextInvalid {
                    reason: "host_installation_id_corrupt"
                })
            ));
            let stored: String = db
                .conn_lock()
                .expect("connection")
                .query_row(
                    "SELECT installation_id FROM host_installation_identity WHERE singleton = 1",
                    [],
                    |row| row.get(0),
                )
                .expect("stored identity");
            assert_eq!(stored, corrupt);
        }
    }

    #[test]
    fn blank_host_identity_is_rejected_by_sql_check() {
        for blank in ["", "   "] {
            let db = Database::init(":memory:").expect("database");
            assert!(db
                .conn_lock()
                .expect("connection")
                .execute(
                    "INSERT INTO host_installation_identity(singleton, installation_id)
                     VALUES (1, ?1)",
                    [blank],
                )
                .is_err());
            let count: i64 = db
                .conn_lock()
                .expect("connection")
                .query_row(
                    "SELECT COUNT(*) FROM host_installation_identity",
                    [],
                    |row| row.get(0),
                )
                .expect("identity count");
            assert_eq!(count, 0);
        }

        let db = Database::init(":memory:").expect("database");
        let original = Uuid::new_v4().to_string();
        db.conn_lock()
            .expect("connection")
            .execute(
                "INSERT INTO host_installation_identity(singleton, installation_id)
                 VALUES (1, ?1)",
                [&original],
            )
            .expect("valid identity");
        assert!(db
            .conn_lock()
            .expect("connection")
            .execute(
                "UPDATE host_installation_identity SET installation_id = '   '
                 WHERE singleton = 1",
                [],
            )
            .is_err());
        let stored: String = db
            .conn_lock()
            .expect("connection")
            .query_row(
                "SELECT installation_id FROM host_installation_identity WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .expect("stored identity");
        assert_eq!(stored, original);
    }

    #[test]
    fn canonical_v4_host_identity_is_read_without_rotation() {
        let db = Database::init(":memory:").expect("database");
        let original = Uuid::new_v4().to_string();
        db.conn_lock()
            .expect("connection")
            .execute(
                "INSERT INTO host_installation_identity(singleton, installation_id)
                 VALUES (1, ?1)",
                [&original],
            )
            .expect("valid identity");
        let context = db.stable_host_installation_context().expect("context");
        assert_eq!(context.id, original);
    }

    #[test]
    fn host_identity_survives_database_reconstruction() {
        let path = std::env::temp_dir().join(format!(
            "pomegranate-host-context-{}.sqlite",
            Uuid::new_v4()
        ));
        let path_text = path.to_string_lossy().into_owned();
        let first = {
            let db = Database::init(&path_text).expect("first database");
            db.stable_host_installation_context().expect("first").id
        };
        let second = {
            let db = Database::init(&path_text).expect("second database");
            db.stable_host_installation_context().expect("second").id
        };
        assert_eq!(first, second);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn context_initialization_does_not_write_authorization_tables() {
        let db = Database::init(":memory:").expect("database");
        db.stable_host_installation_context().expect("context");
        let conn = db.conn_lock().expect("connection");
        let formal: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM plugin_capability_authorizations",
                [],
                |row| row.get(0),
            )
            .expect("formal count");
        let legacy: i64 = conn
            .query_row("SELECT COUNT(*) FROM plugin_permissions", [], |row| {
                row.get(0)
            })
            .expect("legacy count");
        assert_eq!((formal, legacy), (0, 0));
    }
}
