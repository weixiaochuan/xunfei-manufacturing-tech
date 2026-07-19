//! Local mock marketplace supply-side workflow.
//!
//! Phase 3 keeps developer/admin/revenue flows fully local. It does not
//! perform real identity, payment, signing, remote download or Xunfei calls.

use rusqlite::{params, params_from_iter, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zip::ZipArchive;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    AdminProductModerationInput, AdminReviewInput, AdminVersionModerationInput,
    AiServiceDeliveryMode, DeveloperDashboard, DeveloperEarning, DeveloperProduct,
    DeveloperProductInput, DeveloperProductVersion, DeveloperSubmitInput,
    DeveloperUploadPackageInput, DeveloperVersionInput, LocalAccountProfile,
    LocalAccountUpdateInput, MarketplaceActionResult, MarketplaceLicenseType, MarketplaceMockRole,
    MarketplaceMockSession, MarketplacePackageReport, MarketplacePrice, MarketplaceReviewStatus,
    MarketplaceRiskFinding, MarketplaceScanStatus, NormalizedPluginManifest, PermissionDiff,
    PluginRuntimeKind, PluginSource, ProductType, SignatureStatus,
};
use crate::services::plugins::PluginService;

const CUSTOMER_ID: &str = "local-demo-buyer";
const DEVELOPER_ID: &str = "local-demo-creator";
const ADMIN_ID: &str = "local-demo-admin";

const MARKETPLACE_DIR: &str = "marketplace";
const UPLOADS_DIR: &str = "developer-uploads";
const REVIEW_PACKAGES_DIR: &str = "review-packages";
const MAX_ZIP_SIZE: u64 = 50 * 1024 * 1024;
const MAX_FILE_COUNT: u64 = 1000;
const MAX_UNPACKED_SIZE: u64 = 200 * 1024 * 1024;
const MAX_SINGLE_FILE_SIZE: u64 = 50 * 1024 * 1024;
const PLATFORM_FEE_BPS: i64 = 2000;

pub struct MarketplaceSupplyService;

impl MarketplaceSupplyService {
    pub fn ensure_local_users(db: &Database) -> Result<(), AppError> {
        let conn = db.conn_lock()?;
        conn.execute_batch(
            r#"
            INSERT OR IGNORE INTO users
                (id, display_name, role, is_mock, nickname, avatar, bio, account_status, developer_status)
            VALUES
                ('local-demo-buyer', '普通买家', 'customer', 1, '普通买家', NULL, '本地演示普通买家账号。', 'active', 'none'),
                ('local-demo-creator', '个人创作者', 'developer', 1, '个人创作者', NULL, '本地演示个人创作者，可购买也可销售。', 'active', 'approved'),
                ('local-demo-admin', '管理员', 'admin', 1, '管理员', NULL, '本地演示管理员账号。', 'active', 'approved'),
                ('official-demo-developer', 'firstwork 官方演示', 'developer', 1, 'firstwork 官方演示', NULL, '内置官方演示商品创作者。', 'active', 'approved');

            INSERT OR IGNORE INTO developer_profiles
                (user_id, developer_name, description, verification_status)
            VALUES
                ('local-demo-creator', '个人创作者', '本地演示个人商店。', 'local_demo'),
                ('official-demo-developer', 'firstwork 官方演示', '内置演示商品开发者。', 'local_demo');
            "#,
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO users
                (id, display_name, role, is_mock, nickname, avatar, bio, account_status, developer_status)
             VALUES
                ('firstwork-official', 'firstwork 官方', 'developer', 1, 'firstwork 官方', NULL,
                 '内置官方演示商品创作者。', 'active', 'approved')",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO developer_profiles
                (user_id, developer_name, description, verification_status)
             VALUES
                ('firstwork-official', 'firstwork 官方', '内置演示商品开发者。', 'local_demo')",
            [],
        )?;
        Ok(())
    }

    pub fn session_for_role(role: MarketplaceMockRole) -> MarketplaceMockSession {
        match role {
            MarketplaceMockRole::Customer => MarketplaceMockSession {
                user_id: CUSTOMER_ID.into(),
                display_name: "普通买家".into(),
                role,
                is_mock: true,
                notice: "本地演示模式：普通买家账号，不代表真实登录或真实支付。".into(),
                nickname: Some("普通买家".into()),
                avatar: None,
                bio: Some("本地演示普通买家账号。".into()),
                account_status: Some("active".into()),
                developer_status: Some("none".into()),
                can_buy: true,
                can_sell: false,
                can_admin: false,
            },
            MarketplaceMockRole::Developer => MarketplaceMockSession {
                user_id: DEVELOPER_ID.into(),
                display_name: "个人创作者".into(),
                role,
                is_mock: true,
                notice: "本地演示模式：个人创作者账号，可购买也可销售，不代表真实认证。".into(),
                nickname: Some("个人创作者".into()),
                avatar: None,
                bio: Some("本地演示个人创作者，可购买也可销售。".into()),
                account_status: Some("active".into()),
                developer_status: Some("approved".into()),
                can_buy: true,
                can_sell: true,
                can_admin: false,
            },
            MarketplaceMockRole::Admin => MarketplaceMockSession {
                user_id: ADMIN_ID.into(),
                display_name: "管理员".into(),
                role,
                is_mock: true,
                notice: "本地演示模式：管理员账号，不代表真实后台登录。".into(),
                nickname: Some("管理员".into()),
                avatar: None,
                bio: Some("本地演示管理员账号。".into()),
                account_status: Some("active".into()),
                developer_status: Some("approved".into()),
                can_buy: true,
                can_sell: true,
                can_admin: true,
            },
        }
    }

    pub fn session_for_user(
        db: &Database,
        user_id: &str,
    ) -> Result<MarketplaceMockSession, AppError> {
        Self::ensure_local_users(db)?;
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, display_name, role, is_mock, COALESCE(nickname, display_name),
                    avatar, bio, account_status, developer_status
             FROM users WHERE id = ?1",
            [user_id],
            |row| {
                let role_str: String = row.get(2)?;
                let developer_status: String = row.get(8)?;
                let role = match role_str.as_str() {
                    "developer" => MarketplaceMockRole::Developer,
                    "admin" => MarketplaceMockRole::Admin,
                    _ => MarketplaceMockRole::Customer,
                };
                Ok(MarketplaceMockSession {
                    user_id: row.get(0)?,
                    display_name: row.get(1)?,
                    role: role.clone(),
                    is_mock: row.get::<_, i32>(3)? != 0,
                    notice: "本地演示模式：账号切换不代表真实登录、实名、支付或提现。".into(),
                    nickname: Some(row.get(4)?),
                    avatar: row.get(5)?,
                    bio: row.get(6)?,
                    account_status: Some(row.get(7)?),
                    can_buy: true,
                    can_sell: role == MarketplaceMockRole::Admin || developer_status == "approved",
                    can_admin: role == MarketplaceMockRole::Admin,
                    developer_status: Some(developer_status),
                })
            },
        )
        .map_err(AppError::from)
    }

    pub fn current_session(db: &Database) -> Result<MarketplaceMockSession, AppError> {
        let user_id = db
            .get_config("marketplace.current_user_id")?
            .unwrap_or_else(|| CUSTOMER_ID.into());
        Self::session_for_user(db, &user_id)
    }

    pub fn set_current_user(
        db: &Database,
        user_id: &str,
    ) -> Result<MarketplaceMockSession, AppError> {
        let session = Self::session_for_user(db, user_id)?;
        db.set_config("marketplace.current_user_id", &session.user_id)?;
        Ok(session)
    }

    pub fn list_accounts(db: &Database) -> Result<Vec<LocalAccountProfile>, AppError> {
        Self::ensure_local_users(db)?;
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(nickname, display_name), avatar, bio, account_status,
                    developer_status, created_at, is_mock, role
             FROM users
             WHERE id IN ('local-demo-buyer','local-demo-creator','local-demo-admin')
             ORDER BY CASE id
                WHEN 'local-demo-buyer' THEN 1
                WHEN 'local-demo-creator' THEN 2
                WHEN 'local-demo-admin' THEN 3
                ELSE 9 END",
        )?;
        let rows = stmt.query_map([], |row| {
            let developer_status: String = row.get(5)?;
            let role: String = row.get(8)?;
            Ok(LocalAccountProfile {
                user_id: row.get(0)?,
                nickname: row.get(1)?,
                avatar: row.get(2)?,
                bio: row.get(3)?,
                account_status: row.get(4)?,
                developer_status: developer_status.clone(),
                created_at: row.get(6)?,
                is_mock: row.get::<_, i32>(7)? != 0,
                can_buy: true,
                can_sell: role == "admin" || developer_status == "approved",
                can_admin: role == "admin",
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn update_account(
        db: &Database,
        session: &MarketplaceMockSession,
        input: LocalAccountUpdateInput,
    ) -> Result<MarketplaceMockSession, AppError> {
        let nickname = input
            .nickname
            .unwrap_or_else(|| session.display_name.clone());
        if nickname.trim().is_empty() {
            return Err(AppError::InvalidInput("昵称不能为空".into()));
        }
        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE users SET display_name = ?2, nickname = ?2, avatar = ?3, bio = ?4
             WHERE id = ?1",
            params![session.user_id, nickname.trim(), input.avatar, input.bio],
        )?;
        conn.execute(
            "UPDATE developer_profiles SET developer_name = ?2, description = ?3, updated_at = datetime('now','localtime')
             WHERE user_id = ?1",
            params![session.user_id, nickname.trim(), input.bio],
        )
        .ok();
        drop(conn);
        Self::session_for_user(db, &session.user_id)
    }

    pub fn apply_developer(
        db: &Database,
        session: &MarketplaceMockSession,
    ) -> Result<MarketplaceMockSession, AppError> {
        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE users SET role = CASE WHEN role = 'admin' THEN role ELSE 'developer' END,
                    developer_status = 'approved'
             WHERE id = ?1",
            [session.user_id.as_str()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO developer_profiles
                (user_id, developer_name, description, verification_status)
             VALUES (?1, ?2, ?3, 'local_demo')",
            params![
                session.user_id,
                session
                    .nickname
                    .clone()
                    .unwrap_or_else(|| session.display_name.clone()),
                session
                    .bio
                    .clone()
                    .unwrap_or_else(|| "本地演示个人商店。".into())
            ],
        )?;
        drop(conn);
        Self::session_for_user(db, &session.user_id)
    }

    pub fn developer_list_products(
        db: &Database,
        session: &MarketplaceMockSession,
    ) -> Result<Vec<DeveloperProduct>, AppError> {
        ensure_developer(session)?;
        Self::ensure_local_users(db)?;
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT p.id, COALESCE(p.plugin_id, p.id), p.name, COALESCE(p.description,''), COALESCE(p.full_description, p.description, ''),
                    COALESCE(p.seller_user_id, p.developer_id), p.developer_name, p.product_type, p.status, COALESCE(p.category,'general'),
                    COALESCE(p.tags_json,'[]'), p.byok_required, p.license_type, p.mock_mode, p.runtime_kind,
                    COALESCE(p.third_party_dependencies,'{}'),
                    COALESCE(pr.currency, 'CNY'), COALESCE(pr.amount, 0), COALESCE(pr.price_type, p.license_type), COALESCE(pr.is_mock, 1)
             FROM products p
             LEFT JOIN prices pr ON pr.id = (
                SELECT id FROM prices WHERE product_id = p.id ORDER BY id DESC LIMIT 1
             )
             WHERE COALESCE(p.seller_user_id, p.developer_id) = ?1
             ORDER BY p.updated_at DESC, p.created_at DESC",
        )?;
        let mut products = stmt
            .query_map([session.user_id.as_str()], |row| {
                let product_type: String = row.get(7)?;
                let status: String = row.get(8)?;
                let tags_json: String = row.get(10)?;
                let license_type: String = row.get(12)?;
                let runtime_kind: String = row.get(14)?;
                let service_metadata: Value =
                    serde_json::from_str(&row.get::<_, String>(15)?).unwrap_or(Value::Null);
                Ok(DeveloperProduct {
                    id: row.get(0)?,
                    plugin_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    full_description: row.get(4)?,
                    developer_id: row.get(5)?,
                    developer_name: row.get(6)?,
                    product_type: parse_product_type(&product_type),
                    runtime_kind: parse_runtime_kind(&runtime_kind),
                    status: parse_review_status(&status),
                    category: row.get(9)?,
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                    byok_required: row.get::<_, i32>(11)? != 0,
                    delivery_mode: service_metadata
                        .get("deliveryMode")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok()),
                    protocol: service_metadata
                        .get("protocol")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    service_configuration: service_metadata.get("configurationSchema").cloned(),
                    license_type: parse_license(&license_type),
                    price: MarketplacePrice {
                        currency: row.get(16)?,
                        amount: row.get(17)?,
                        price_type: parse_license(&row.get::<_, String>(18)?),
                        is_mock: row.get::<_, i32>(19)? != 0,
                    },
                    mock_mode: row.get::<_, i32>(13)? != 0,
                    current_version: None,
                    versions: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let product_ids = products
            .iter()
            .map(|product| product.id.clone())
            .collect::<Vec<_>>();
        if product_ids.is_empty() {
            return Ok(products);
        }

        let placeholders = vec!["?"; product_ids.len()].join(",");
        let version_sql = format!(
            "SELECT product_id, id, version, status, review_status, scan_status, changelog, content_hash,
                    package_path, created_at
             FROM product_versions WHERE product_id IN ({}) ORDER BY product_id ASC, id ASC",
            placeholders,
        );
        let mut version_stmt = conn.prepare(&version_sql)?;
        let version_rows = version_stmt.query_map(
            params_from_iter(product_ids.iter().map(|id| id.as_str())),
            |row| {
                let status: String = row.get(3)?;
                let review_status: String = row.get(4)?;
                let scan_status: String = row.get(5)?;
                Ok((
                    row.get::<_, String>(0)?,
                    DeveloperProductVersion {
                        id: row.get(1)?,
                        version: row.get(2)?,
                        status: parse_review_status(&status),
                        review_status: parse_review_status(&review_status),
                        scan_status: parse_scan_status(&scan_status),
                        changelog: row.get(6)?,
                        content_hash: row.get(7)?,
                        package_path: row.get(8)?,
                        created_at: row.get(9)?,
                    },
                ))
            },
        )?;
        let mut versions_by_product: HashMap<String, Vec<DeveloperProductVersion>> = HashMap::new();
        for row in version_rows {
            let (product_id, version) = row?;
            versions_by_product
                .entry(product_id)
                .or_default()
                .push(version);
        }
        drop(version_stmt);
        for product in &mut products {
            product.versions = versions_by_product.remove(&product.id).unwrap_or_default();
            product.current_version = product
                .versions
                .last()
                .map(|version| version.version.clone());
        }
        Ok(products)
    }

    pub fn developer_get_product(
        db: &Database,
        session: &MarketplaceMockSession,
        product_id: &str,
    ) -> Result<DeveloperProduct, AppError> {
        ensure_developer(session)?;
        let product = Self::read_developer_product(db, product_id)?;
        if product.developer_id != session.user_id {
            return Err(AppError::PluginPermissionDenied {
                plugin_id: Some(product_id.into()),
                required_permission: Some("developer.own_product".into()),
            });
        }
        Ok(product)
    }

    pub fn developer_create_product(
        db: &Database,
        session: &MarketplaceMockSession,
        input: DeveloperProductInput,
    ) -> Result<DeveloperProduct, AppError> {
        ensure_developer(session)?;
        validate_product_input(&input)?;
        let id = unique_product_id(&input.name);
        let tags_json = serde_json::to_string(&input.tags)?;
        let service_metadata = service_metadata_json(&input)?.to_string();
        let conn = db.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO products
             (id, plugin_id, developer_id, seller_user_id, developer_name, name, description, full_description,
              product_type, runtime_kind, status, icon, license_type, byok_required, mock_mode, data_destination,
              file_upload_notice, risk_notes_json, category, tags_json, privacy_notice, usage_guide,
              third_party_dependencies, file_upload_required, support_period, review_status, distribution_channel)
             VALUES
             (?1, ?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'draft', ?9, ?10, ?11, 1, ?12,
              ?13, '[]', ?14, ?15, ?16, ?17, ?18, ?19, ?20, 'draft', 'local-demo')",
            params![
                id,
                session.user_id,
                session.display_name,
                input.name,
                input.description,
                input.full_description.clone().unwrap_or(input.description.clone()),
                product_type_to_str(&input.product_type),
                runtime_kind_to_str(&input.runtime_kind),
                input.icon,
                license_to_str(&input.license_type),
                bool_int(input.byok_required),
                input.data_destination,
                if input.file_upload_required { Some("用户主动选择文件/图片后才会发送到第三方服务。".to_string()) } else { None },
                input.category.unwrap_or_else(|| "general".into()),
                tags_json,
                input.privacy_notice,
                input.usage_guide,
                service_metadata,
                bool_int(input.file_upload_required),
                input.support_period,
            ],
        )?;
        tx.execute(
            "INSERT INTO prices (product_id, currency, amount, price_type, is_mock)
             VALUES (?1, 'CNY', ?2, ?3, 1)",
            params![
                id,
                input.price_amount.max(0),
                license_to_str(&input.license_type)
            ],
        )?;
        tx.commit()?;
        drop(conn);
        write_audit(
            db,
            session,
            "developer_create_product",
            "product",
            &id,
            json!({ "mock": true }),
        )?;
        Self::developer_get_product(db, session, &id)
    }

    pub fn developer_update_product(
        db: &Database,
        session: &MarketplaceMockSession,
        product_id: &str,
        input: DeveloperProductInput,
    ) -> Result<DeveloperProduct, AppError> {
        ensure_developer(session)?;
        validate_product_input(&input)?;
        let current = Self::developer_get_product(db, session, product_id)?;
        if current.status == MarketplaceReviewStatus::Published {
            return Err(AppError::InvalidInput(
                "已发布商品不能直接修改线上资料，请创建新版本或下架后处理".into(),
            ));
        }
        let tags_json = serde_json::to_string(&input.tags)?;
        let service_metadata = service_metadata_json(&input)?.to_string();
        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE products SET
                name = ?2, description = ?3, full_description = ?4, product_type = ?5, runtime_kind = ?6, icon = ?7,
                license_type = ?8, byok_required = ?9, data_destination = ?10,
                file_upload_required = ?11, category = ?12, tags_json = ?13,
                privacy_notice = ?14, usage_guide = ?15, third_party_dependencies = ?16,
                support_period = ?17, updated_at = datetime('now','localtime')
             WHERE id = ?1 AND COALESCE(seller_user_id, developer_id) = ?18",
            params![
                product_id,
                input.name,
                input.description,
                input.full_description.unwrap_or(input.description.clone()),
                product_type_to_str(&input.product_type),
                runtime_kind_to_str(&input.runtime_kind),
                input.icon,
                license_to_str(&input.license_type),
                bool_int(input.byok_required),
                input.data_destination,
                bool_int(input.file_upload_required),
                input.category.unwrap_or_else(|| "general".into()),
                tags_json,
                input.privacy_notice,
                input.usage_guide,
                service_metadata,
                input.support_period,
                session.user_id,
            ],
        )?;
        conn.execute(
            "INSERT INTO prices (product_id, currency, amount, price_type, is_mock)
             VALUES (?1, 'CNY', ?2, ?3, 1)",
            params![
                product_id,
                input.price_amount.max(0),
                license_to_str(&input.license_type)
            ],
        )?;
        drop(conn);
        write_audit(
            db,
            session,
            "developer_update_product",
            "product",
            product_id,
            json!({}),
        )?;
        Self::developer_get_product(db, session, product_id)
    }

    pub fn developer_create_version(
        db: &Database,
        session: &MarketplaceMockSession,
        input: DeveloperVersionInput,
    ) -> Result<DeveloperProductVersion, AppError> {
        let product = Self::developer_get_product(db, session, &input.product_id)?;
        validate_semver(&input.version)?;
        ensure_version_increases(db, &input.product_id, &input.version)?;
        let manifest = draft_manifest(&product, &input.version);
        let manifest_json = serde_json::to_string(&manifest)?;
        let conn = db.conn_lock()?;
        conn.execute(
            "INSERT INTO product_versions
             (product_id, version, manifest_json, runtime_kind, source, content_hash, signature_status,
              min_app_version, status, changelog, review_status, distribution_channel, scan_status)
             VALUES (?1, ?2, ?3, ?4, 'marketplace', '', 'unsigned', '1.8.0', 'draft', ?5, 'draft', 'local-demo', 'not_scanned')",
            params![
                input.product_id,
                input.version,
                manifest_json,
                runtime_kind_to_str(&product.runtime_kind),
                input.changelog.unwrap_or_default(),
            ],
        )?;
        drop(conn);
        write_audit(
            db,
            session,
            "developer_create_version",
            "product",
            &input.product_id,
            json!({ "version": input.version }),
        )?;
        Self::latest_version(db, &input.product_id)
    }

    pub fn developer_upload_package(
        db: &Database,
        data_dir: &Path,
        session: &MarketplaceMockSession,
        input: DeveloperUploadPackageInput,
        app_version: &str,
    ) -> Result<MarketplacePackageReport, AppError> {
        let product = Self::developer_get_product(db, session, &input.product_id)?;
        let zip_path = PathBuf::from(&input.zip_path);
        let (report, extracted_dir) = inspect_zip_package(&zip_path, data_dir, app_version)?;
        if !report.ok {
            write_audit(
                db,
                session,
                "developer_upload_package_failed",
                "product",
                &input.product_id,
                json!({ "errors": report.errors }),
            )?;
            return Ok(report);
        }
        let manifest = PluginService::parse_manifest(&extracted_dir)?;
        let consistency_errors = manifest_consistency_errors(&product, &manifest, &input.version);
        if !consistency_errors.is_empty() {
            let mut failed = report;
            failed.ok = false;
            failed.status = MarketplaceScanStatus::Failed;
            failed.errors.extend(consistency_errors);
            fs::remove_dir_all(&extracted_dir).ok();
            return Ok(failed);
        }
        ensure_version_increases_or_same_draft(db, &input.product_id, &input.version)?;
        let asset_dir = data_dir
            .join(MARKETPLACE_DIR)
            .join(REVIEW_PACKAGES_DIR)
            .join(&input.product_id)
            .join(&input.version);
        if asset_dir.exists() {
            fs::remove_dir_all(&asset_dir)?;
        }
        fs::create_dir_all(asset_dir.parent().unwrap_or_else(|| data_dir))?;
        fs::rename(&extracted_dir, &asset_dir).or_else(|_| {
            copy_dir(&extracted_dir, &asset_dir)?;
            fs::remove_dir_all(&extracted_dir).ok();
            Ok::<(), AppError>(())
        })?;
        let manifest_json = serde_json::to_string(&manifest_to_marketplace_json(&manifest))?;
        let scan_json = serde_json::to_string(&report)?;
        let conn = db.conn_lock()?;
        conn.execute(
            "INSERT INTO product_versions
             (product_id, version, manifest_json, runtime_kind, source, content_hash, signature_status,
              min_app_version, status, changelog, package_path, review_status, distribution_channel, scan_report_json, scan_status)
             VALUES (?1, ?2, ?3, ?4, 'marketplace', ?5, 'unsigned', ?6, 'draft', ?7, ?8, 'draft', 'local-demo', ?9, ?10)
             ON CONFLICT(product_id, version) DO UPDATE SET
                manifest_json = excluded.manifest_json,
                runtime_kind = excluded.runtime_kind,
                content_hash = excluded.content_hash,
                min_app_version = excluded.min_app_version,
                changelog = excluded.changelog,
                package_path = excluded.package_path,
                scan_report_json = excluded.scan_report_json,
                scan_status = excluded.scan_status,
                status = CASE WHEN product_versions.status IN ('published', 'revoked') THEN product_versions.status ELSE 'draft' END,
                review_status = CASE WHEN product_versions.review_status IN ('published', 'revoked') THEN product_versions.review_status ELSE 'draft' END",
            params![
                input.product_id,
                input.version,
                manifest_json,
                runtime_kind_to_str(&manifest.runtime_kind),
                report.sha256,
                manifest.min_app_version,
                input.changelog.unwrap_or_default(),
                asset_dir.to_string_lossy().to_string(),
                scan_json,
                scan_status_to_str(&report.status),
            ],
        )?;
        let version_id: i64 = conn.query_row(
            "SELECT id FROM product_versions WHERE product_id = ?1 AND version = ?2",
            params![input.product_id, input.version],
            |row| row.get(0),
        )?;
        conn.execute(
            "DELETE FROM product_permissions WHERE product_version_id = ?1",
            [version_id],
        )?;
        for permission in &manifest.permissions {
            conn.execute(
                "INSERT INTO product_permissions (product_version_id, permission, required, reason)
                 VALUES (?1, ?2, 1, 'manifest upload')",
                params![version_id, permission],
            )?;
        }
        conn.execute(
            "INSERT INTO product_assets (product_version_id, asset_type, local_path, content_hash, size)
             VALUES (?1, 'review_package_dir', ?2, ?3, ?4)
             ON CONFLICT(product_version_id, asset_type) DO UPDATE SET
                local_path = excluded.local_path,
                content_hash = excluded.content_hash,
                size = excluded.size",
            params![version_id, asset_dir.to_string_lossy().to_string(), report.sha256, report.unpacked_size as i64],
        )?;
        drop(conn);
        write_audit(
            db,
            session,
            "developer_upload_package",
            "product",
            &input.product_id,
            json!({ "version": input.version }),
        )?;
        Ok(report)
    }

    pub fn developer_get_package_report(
        db: &Database,
        session: &MarketplaceMockSession,
        product_id: &str,
        version: &str,
    ) -> Result<MarketplacePackageReport, AppError> {
        Self::developer_get_product(db, session, product_id)?;
        let conn = db.conn_lock()?;
        let report: Option<String> = conn
            .query_row(
                "SELECT scan_report_json FROM product_versions WHERE product_id = ?1 AND version = ?2",
                params![product_id, version],
                |row| row.get(0),
            )
            .optional()?;
        report
            .as_deref()
            .ok_or_else(|| AppError::NotFound("未找到上传检查报告".into()))
            .and_then(|v| serde_json::from_str(v).map_err(AppError::from))
    }

    pub fn developer_submit_product(
        db: &Database,
        session: &MarketplaceMockSession,
        input: DeveloperSubmitInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        let product = Self::developer_get_product(db, session, &input.product_id)?;
        let version = match input.version {
            Some(v) => Self::version_by_product_version(db, &input.product_id, &v)?,
            None => Self::latest_version(db, &input.product_id)?,
        };
        if version.scan_status == MarketplaceScanStatus::Failed
            || version.scan_status == MarketplaceScanStatus::NotScanned
        {
            return Err(AppError::InvalidInput(
                "上传检查未通过，不能提交审核".into(),
            ));
        }
        if product.status == MarketplaceReviewStatus::Published {
            return Err(AppError::InvalidInput(
                "已发布商品更新必须提交新版本，不能覆盖线上版本".into(),
            ));
        }
        let conn = db.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute("UPDATE products SET status = 'submitted', review_status = 'submitted', updated_at = datetime('now','localtime') WHERE id = ?1", [input.product_id.as_str()])?;
        tx.execute("UPDATE product_versions SET status = 'submitted', review_status = 'submitted' WHERE id = ?1", [version.id])?;
        tx.execute(
            "INSERT INTO product_submissions (product_id, product_version_id, submitted_by, status)
             VALUES (?1, ?2, ?3, 'submitted')",
            params![input.product_id, version.id, session.user_id],
        )?;
        let submission_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO product_review_events (submission_id, actor_id, action, from_status, to_status, message)
             VALUES (?1, ?2, 'submit', 'draft', 'submitted', '开发者提交本地模拟审核')",
            params![submission_id, session.user_id],
        )?;
        tx.commit()?;
        drop(conn);
        write_audit(
            db,
            session,
            "developer_submit_product",
            "submission",
            &submission_id.to_string(),
            json!({ "productId": input.product_id }),
        )?;
        Ok(MarketplaceActionResult {
            ok: true,
            product_id: product.id,
            plugin_id: Some(product.plugin_id),
            message: "已提交本地模拟审核".into(),
            requires_permission_confirmation: false,
            permission_diff: None,
            entitlement: None,
            installation: None,
        })
    }

    pub fn developer_dashboard(
        db: &Database,
        session: &MarketplaceMockSession,
    ) -> Result<DeveloperDashboard, AppError> {
        ensure_developer(session)?;
        Self::rebuild_earnings(db)?;
        let conn = db.conn_lock()?;
        let product_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM products WHERE COALESCE(seller_user_id, developer_id) = ?1",
            [session.user_id.as_str()],
            |row| row.get(0),
        )?;
        let external_service_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM external_agents ea
             JOIN products p ON p.id = ea.product_id
             WHERE COALESCE(p.seller_user_id, p.developer_id) = ?1
               AND COALESCE(ea.enabled, 0) = 1",
            [session.user_id.as_str()],
            |row| row.get(0),
        )?;
        let (invocation_count, invocation_success_count, invocation_failed_count): (i64, i64, i64) =
            conn.query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN u.status = 'completed' THEN 1 ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN u.status != 'completed' THEN 1 ELSE 0 END), 0)
                 FROM usage_events u
                 JOIN products p ON p.id = u.product_id
                 WHERE COALESCE(p.seller_user_id, p.developer_id) = ?1",
                [session.user_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
        let mock_order_count: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT oi.order_id)
             FROM order_items oi
             JOIN products p ON p.id = oi.product_id
             JOIN orders o ON o.id = oi.order_id
             WHERE COALESCE(p.seller_user_id, p.developer_id) = ?1 AND o.is_mock = 1",
            [session.user_id.as_str()],
            |row| row.get(0),
        )?;
        let mock_acquire_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM entitlements e JOIN products p ON p.id = e.product_id
             WHERE COALESCE(p.seller_user_id, p.developer_id) = ?1 AND e.status = 'active'",
            [session.user_id.as_str()],
            |row| row.get(0),
        )?;
        let mock_install_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM plugin_installations i JOIN products p ON p.id = i.product_id
             WHERE COALESCE(p.seller_user_id, p.developer_id) = ?1 AND i.status = 'installed'",
            [session.user_id.as_str()],
            |row| row.get(0),
        )?;
        let mock_enabled_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM plugin_installations i JOIN products p ON p.id = i.product_id
             WHERE COALESCE(p.seller_user_id, p.developer_id) = ?1 AND i.enabled = 1",
            [session.user_id.as_str()],
            |row| row.get(0),
        )?;
        let (gross_amount, platform_fee, developer_amount): (i64, i64, i64) = conn.query_row(
            "SELECT COALESCE(SUM(gross_amount),0), COALESCE(SUM(platform_fee),0), COALESCE(SUM(developer_amount),0)
             FROM developer_earnings WHERE developer_id = ?1 AND is_mock = 1",
            [session.user_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(DeveloperDashboard {
            developer_id: session.user_id.clone(),
            product_count,
            external_service_count,
            invocation_count,
            invocation_success_count,
            invocation_failed_count,
            mock_order_count,
            mock_acquire_count,
            mock_install_count,
            mock_enabled_count,
            gross_amount,
            platform_fee,
            developer_amount,
            currency: "CNY".into(),
            is_mock: true,
        })
    }

    pub fn developer_list_earnings(
        db: &Database,
        session: &MarketplaceMockSession,
    ) -> Result<Vec<DeveloperEarning>, AppError> {
        ensure_developer(session)?;
        Self::rebuild_earnings(db)?;
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT e.id, p.id, p.name, e.gross_amount, e.platform_fee, e.developer_amount,
                    e.currency, e.is_mock, e.status, e.created_at
             FROM developer_earnings e
             JOIN order_items oi ON oi.id = e.order_item_id
             JOIN products p ON p.id = oi.product_id
             WHERE e.developer_id = ?1
             ORDER BY e.created_at DESC",
        )?;
        let rows = stmt.query_map([session.user_id.as_str()], |row| {
            Ok(DeveloperEarning {
                id: row.get(0)?,
                product_id: row.get(1)?,
                product_name: row.get(2)?,
                gross_amount: row.get(3)?,
                platform_fee: row.get(4)?,
                developer_amount: row.get(5)?,
                currency: row.get(6)?,
                is_mock: row.get::<_, i32>(7)? != 0,
                status: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn admin_list_submissions(
        db: &Database,
        session: &MarketplaceMockSession,
        status: Option<MarketplaceReviewStatus>,
    ) -> Result<Vec<crate::models::MarketplaceSubmission>, AppError> {
        ensure_admin(session)?;
        Self::ensure_local_users(db)?;
        let conn = db.conn_lock()?;
        let mut sql = String::from(
            "SELECT s.id, s.product_id, s.product_version_id, p.name, pv.version, p.developer_id,
                    p.developer_name, s.status, s.submitted_by, s.submitted_at, s.reviewed_by,
                    s.reviewed_at, s.review_message, pv.scan_report_json
             FROM product_submissions s
             JOIN products p ON p.id = s.product_id
             LEFT JOIN product_versions pv ON pv.id = s.product_version_id",
        );
        let status_str = status.as_ref().map(review_status_to_str);
        if status_str.is_some() {
            sql.push_str(" WHERE s.status = ?1");
        }
        sql.push_str(" ORDER BY s.submitted_at DESC");
        let mut stmt = conn.prepare(&sql)?;
        let mapper = |row: &rusqlite::Row<'_>| {
            let raw_status: String = row.get(7)?;
            let scan_json: Option<String> = row.get(13)?;
            Ok(crate::models::MarketplaceSubmission {
                id: row.get(0)?,
                product_id: row.get(1)?,
                product_version_id: row.get(2)?,
                product_name: row.get(3)?,
                version: row.get(4)?,
                developer_id: row.get(5)?,
                developer_name: row.get(6)?,
                status: parse_review_status(&raw_status),
                submitted_by: row.get(8)?,
                submitted_at: row.get(9)?,
                reviewed_by: row.get(10)?,
                reviewed_at: row.get(11)?,
                review_message: row.get(12)?,
                scan_report: scan_json.and_then(|v| serde_json::from_str(&v).ok()),
            })
        };
        if let Some(s) = status_str {
            stmt.query_map([s], mapper)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from)
        } else {
            stmt.query_map([], mapper)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(AppError::from)
        }
    }

    pub fn admin_get_submission(
        db: &Database,
        session: &MarketplaceMockSession,
        submission_id: i64,
    ) -> Result<crate::models::MarketplaceSubmission, AppError> {
        ensure_admin(session)?;
        Self::admin_list_submissions(db, session, None)?
            .into_iter()
            .find(|s| s.id == submission_id)
            .ok_or_else(|| AppError::NotFound("未找到审核提交".into()))
    }

    pub fn admin_start_review(
        db: &Database,
        session: &MarketplaceMockSession,
        input: AdminReviewInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        ensure_admin(session)?;
        let sub = Self::admin_get_submission(db, session, input.submission_id)?;
        if sub.status != MarketplaceReviewStatus::Submitted {
            return Err(AppError::InvalidInput("只有 submitted 可以开始审核".into()));
        }
        transition_submission(
            db,
            session,
            &sub,
            "start_review",
            "under_review",
            input.message,
        )?;
        Ok(action_for_submission(&sub, "已进入本地模拟审核"))
    }

    pub fn admin_approve_submission(
        db: &Database,
        session: &MarketplaceMockSession,
        input: AdminReviewInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        ensure_admin(session)?;
        let sub = Self::admin_get_submission(db, session, input.submission_id)?;
        if !matches!(
            sub.status,
            MarketplaceReviewStatus::Submitted | MarketplaceReviewStatus::UnderReview
        ) {
            return Err(AppError::InvalidInput(
                "只有 submitted/under_review 可以批准".into(),
            ));
        }
        let conn = db.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE product_submissions SET status = 'approved', reviewed_by = ?2,
                reviewed_at = datetime('now','localtime'), review_message = ?3 WHERE id = ?1",
            params![sub.id, session.user_id, input.message],
        )?;
        tx.execute(
            "UPDATE products SET status = 'published', review_status = 'approved',
                distribution_channel = 'local-demo', updated_at = datetime('now','localtime')
             WHERE id = ?1",
            [sub.product_id.as_str()],
        )?;
        if let Some(version_id) = sub.product_version_id {
            tx.execute(
                "UPDATE product_versions SET status = 'published', review_status = 'approved',
                    distribution_channel = 'local-demo' WHERE id = ?1",
                [version_id],
            )?;
        }
        tx.execute(
            "INSERT INTO product_review_events (submission_id, actor_id, action, from_status, to_status, message)
             VALUES (?1, ?2, 'approve', ?3, 'approved', ?4)",
            params![sub.id, session.user_id, review_status_to_str(&sub.status), input.message],
        )?;
        tx.commit()?;
        drop(conn);
        write_audit(
            db,
            session,
            "admin_approve_submission",
            "submission",
            &sub.id.to_string(),
            json!({ "productId": sub.product_id }),
        )?;
        Ok(action_for_submission(
            &sub,
            "已通过本地模拟审核，未经过远程数字签名",
        ))
    }

    pub fn admin_reject_submission(
        db: &Database,
        session: &MarketplaceMockSession,
        input: AdminReviewInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        ensure_admin(session)?;
        let message = input
            .message
            .clone()
            .filter(|m| !m.trim().is_empty())
            .ok_or_else(|| AppError::InvalidInput("驳回必须填写原因".into()))?;
        let sub = Self::admin_get_submission(db, session, input.submission_id)?;
        transition_submission(db, session, &sub, "reject", "rejected", Some(message))?;
        Ok(action_for_submission(&sub, "已驳回"))
    }

    pub fn admin_suspend_product(
        db: &Database,
        session: &MarketplaceMockSession,
        input: AdminProductModerationInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        ensure_admin(session)?;
        require_reason(&input.reason, "暂停商品")?;
        set_product_status(db, session, &input.product_id, "suspended", &input.reason)?;
        Ok(action_for_product(&input.product_id, "已暂停商品"))
    }

    pub fn admin_restore_product(
        db: &Database,
        session: &MarketplaceMockSession,
        input: AdminProductModerationInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        ensure_admin(session)?;
        require_reason(&input.reason, "恢复商品")?;
        set_product_status(db, session, &input.product_id, "published", &input.reason)?;
        Ok(action_for_product(&input.product_id, "已恢复商品"))
    }

    pub fn admin_delist_product(
        db: &Database,
        session: &MarketplaceMockSession,
        input: AdminProductModerationInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        ensure_admin(session)?;
        require_reason(&input.reason, "下架商品")?;
        set_product_status(db, session, &input.product_id, "delisted", &input.reason)?;
        Ok(action_for_product(&input.product_id, "已下架商品"))
    }

    pub fn admin_revoke_version(
        db: &Database,
        session: &MarketplaceMockSession,
        input: AdminVersionModerationInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        ensure_admin(session)?;
        require_reason(&input.reason, "吊销版本")?;
        let version = match input.version {
            Some(v) => Self::version_by_product_version(db, &input.product_id, &v)?,
            None => Self::latest_version(db, &input.product_id)?,
        };
        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE product_versions SET status = 'revoked', review_status = 'revoked',
                signature_status = 'revoked' WHERE id = ?1",
            [version.id],
        )?;
        drop(conn);
        write_audit(
            db,
            session,
            "admin_revoke_version",
            "version",
            &version.id.to_string(),
            json!({ "reason": input.reason }),
        )?;
        Ok(action_for_product(&input.product_id, "已吊销版本"))
    }

    fn read_developer_product(
        db: &Database,
        product_id: &str,
    ) -> Result<DeveloperProduct, AppError> {
        let conn = db.conn_lock()?;
        let row = conn
            .query_row(
                "SELECT p.id, COALESCE(p.plugin_id, p.id), p.name, COALESCE(p.description,''), COALESCE(p.full_description, p.description, ''),
                        COALESCE(p.seller_user_id, p.developer_id), p.developer_name, p.product_type, p.status, COALESCE(p.category,'general'),
                        COALESCE(p.tags_json,'[]'), p.byok_required, p.license_type, p.mock_mode, p.runtime_kind,
                        COALESCE(p.third_party_dependencies,'{}')
                 FROM products p
                 WHERE p.id = ?1",
                [product_id],
                |row| {
                    let product_type: String = row.get(7)?;
                    let status: String = row.get(8)?;
                    let tags_json: String = row.get(10)?;
                    let license_type: String = row.get(12)?;
                    let runtime_kind: String = row.get(14)?;
                    let service_metadata: Value = serde_json::from_str(&row.get::<_, String>(15)?).unwrap_or(Value::Null);
                    Ok(DeveloperProduct {
                        id: row.get(0)?,
                        plugin_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        full_description: row.get(4)?,
                        developer_id: row.get(5)?,
                        developer_name: row.get(6)?,
                        product_type: parse_product_type(&product_type),
                        runtime_kind: parse_runtime_kind(&runtime_kind),
                        status: parse_review_status(&status),
                        category: row.get(9)?,
                        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                        byok_required: row.get::<_, i32>(11)? != 0,
                        delivery_mode: service_metadata.get("deliveryMode")
                            .cloned().and_then(|v| serde_json::from_value(v).ok()),
                        protocol: service_metadata.get("protocol").and_then(Value::as_str).map(str::to_string),
                        service_configuration: service_metadata.get("configurationSchema").cloned(),
                        license_type: parse_license(&license_type),
                        price: MarketplacePrice {
                            currency: "CNY".into(),
                            amount: 0,
                            price_type: parse_license(&license_type),
                            is_mock: true,
                        },
                        mock_mode: row.get::<_, i32>(13)? != 0,
                        current_version: None,
                        versions: Vec::new(),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("未找到商品".into()))?;
        drop(conn);
        let price = Self::price_for_product(db, &row.id, &row.license_type)?;
        let versions = Self::versions_for_product(db, &row.id)?;
        let current_version = versions.last().map(|v| v.version.clone());
        Ok(DeveloperProduct {
            price,
            versions,
            current_version,
            ..row
        })
    }

    fn price_for_product(
        db: &Database,
        product_id: &str,
        license: &MarketplaceLicenseType,
    ) -> Result<MarketplacePrice, AppError> {
        let conn = db.conn_lock()?;
        let row: Option<(String, i64, String, i32)> = conn
            .query_row(
                "SELECT currency, amount, price_type, is_mock FROM prices WHERE product_id = ?1 ORDER BY id DESC LIMIT 1",
                [product_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        Ok(row
            .map(|(currency, amount, price_type, is_mock)| MarketplacePrice {
                currency,
                amount,
                price_type: parse_license(&price_type),
                is_mock: is_mock != 0,
            })
            .unwrap_or(MarketplacePrice {
                currency: "CNY".into(),
                amount: 0,
                price_type: license.clone(),
                is_mock: true,
            }))
    }

    fn versions_for_product(
        db: &Database,
        product_id: &str,
    ) -> Result<Vec<DeveloperProductVersion>, AppError> {
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, version, status, review_status, scan_status, changelog, content_hash,
                    package_path, created_at
             FROM product_versions WHERE product_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([product_id], |row| {
            let status: String = row.get(2)?;
            let review_status: String = row.get(3)?;
            let scan_status: String = row.get(4)?;
            Ok(DeveloperProductVersion {
                id: row.get(0)?,
                version: row.get(1)?,
                status: parse_review_status(&status),
                review_status: parse_review_status(&review_status),
                scan_status: parse_scan_status(&scan_status),
                changelog: row.get(5)?,
                content_hash: row.get(6)?,
                package_path: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    fn latest_version(
        db: &Database,
        product_id: &str,
    ) -> Result<DeveloperProductVersion, AppError> {
        Self::versions_for_product(db, product_id)?
            .into_iter()
            .last()
            .ok_or_else(|| AppError::NotFound("该商品尚未创建版本或上传包".into()))
    }

    fn version_by_product_version(
        db: &Database,
        product_id: &str,
        version: &str,
    ) -> Result<DeveloperProductVersion, AppError> {
        Self::versions_for_product(db, product_id)?
            .into_iter()
            .find(|v| v.version == version)
            .ok_or_else(|| AppError::NotFound("未找到商品版本".into()))
    }

    fn rebuild_earnings(db: &Database) -> Result<(), AppError> {
        let conn = db.conn_lock()?;
        conn.execute(
            "INSERT OR IGNORE INTO developer_earnings
                (developer_id, order_item_id, gross_amount, platform_fee, developer_amount, currency, is_mock, status)
             SELECT
                COALESCE(oi.seller_user_id, p.seller_user_id, p.developer_id),
                oi.id,
                COALESCE(oi.gross_amount, oi.amount),
                COALESCE(oi.gross_amount, oi.amount) * ?1 / 10000,
                COALESCE(oi.gross_amount, oi.amount) - (COALESCE(oi.gross_amount, oi.amount) * ?1 / 10000),
                o.currency,
                1,
                'pending'
             FROM order_items oi
             JOIN products p ON p.id = oi.product_id
             JOIN orders o ON o.id = oi.order_id
             WHERE o.is_mock = 1 AND COALESCE(o.payment_status, 'paid') = 'paid'",
            params![PLATFORM_FEE_BPS],
        )?;
        Ok(())
    }
}

fn inspect_zip_package(
    zip_path: &Path,
    data_dir: &Path,
    app_version: &str,
) -> Result<(MarketplacePackageReport, PathBuf), AppError> {
    let mut findings = Vec::new();
    let mut errors = Vec::new();
    let metadata = fs::metadata(zip_path)?;
    if metadata.len() > MAX_ZIP_SIZE {
        errors.push("压缩包超过 50MB 限制".into());
    }
    let mut zip_bytes = Vec::new();
    File::open(zip_path)?.read_to_end(&mut zip_bytes)?;
    let sha256 = sha256_bytes(&zip_bytes);
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader)?;
    if archive.len() as u64 > MAX_FILE_COUNT {
        errors.push("压缩包文件数量超过 1000 个".into());
    }
    let temp_dir = data_dir
        .join(MARKETPLACE_DIR)
        .join(UPLOADS_DIR)
        .join(format!("upload-{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_dir)?;
    let mut unpacked_size = 0_u64;
    let mut has_executables = false;
    let mut has_scripts = false;
    let mut has_suspected_secrets = false;
    let mut has_external_urls = false;
    let mut has_absolute_paths = false;
    let mut has_high_risk_permissions = false;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let enclosed = match file.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => {
                has_absolute_paths = true;
                errors.push(format!("发现非法压缩路径: {}", file.name()));
                continue;
            }
        };
        if enclosed.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            has_absolute_paths = true;
            errors.push(format!("发现路径穿越: {}", file.name()));
            continue;
        }
        if is_zip_symlink(file.unix_mode()) {
            errors.push(format!("不允许符号链接: {}", file.name()));
            continue;
        }
        let file_size = file.size();
        if file_size > MAX_SINGLE_FILE_SIZE {
            errors.push(format!("单文件超过 50MB: {}", enclosed.display()));
        }
        unpacked_size = unpacked_size.saturating_add(file_size);
        if unpacked_size > MAX_UNPACKED_SIZE {
            errors.push("解压后总大小超过 200MB 限制".into());
        }
        let ext = enclosed
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "exe" | "dll" | "bat" | "cmd" | "ps1" | "sh") {
            has_executables = true;
            findings.push(finding(
                "high",
                "executable",
                &enclosed,
                "公开市场包默认禁止可执行文件或脚本",
                None,
            ));
        }
        if matches!(
            ext.as_str(),
            "js" | "mjs" | "cjs" | "py" | "ps1" | "sh" | "bat" | "cmd"
        ) {
            has_scripts = true;
            findings.push(finding(
                "high",
                "script",
                &enclosed,
                "公开市场包默认禁止脚本运行时",
                None,
            ));
        }
        if enclosed
            .file_name()
            .and_then(|v| v.to_str())
            .map(|name| {
                let lower = name.to_ascii_lowercase();
                lower == ".env" || lower.contains("credential") || lower.contains("secret")
            })
            .unwrap_or(false)
        {
            has_suspected_secrets = true;
            findings.push(finding(
                "high",
                "secret_file",
                &enclosed,
                "疑似密钥或凭据文件",
                None,
            ));
        }
        let out_path = temp_dir.join(&enclosed);
        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&out_path)?;
            let mut limited = Vec::new();
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            out.write_all(&buffer)?;
            limited.extend(buffer.iter().take(1024 * 1024));
            if let Ok(text) = String::from_utf8(limited) {
                let scan = scan_text(&text, &enclosed);
                if scan.has_secret {
                    has_suspected_secrets = true;
                }
                if scan.has_url {
                    has_external_urls = true;
                }
                if scan.has_abs_path {
                    has_absolute_paths = true;
                }
                findings.extend(scan.findings);
            }
        }
    }

    let manifest_result = PluginService::parse_manifest(&temp_dir);
    let mut manifest_valid = false;
    let mut schema_version = None;
    let mut product_id = None;
    let mut version = None;
    let mut product_type = None;
    let mut runtime_kind = None;
    let mut delivery_mode = None;
    let mut protocol = None;
    let mut source = None;
    let mut permissions = Vec::new();
    let mut credential_requirements = Vec::new();
    let mut signature_status = SignatureStatus::Unsigned;
    let mut compatible = true;
    match manifest_result {
        Ok(manifest) => {
            manifest_valid = true;
            schema_version = Some(manifest.schema_version);
            product_id = Some(manifest.id.clone());
            version = Some(manifest.version.clone());
            product_type = Some(manifest.product_type.clone());
            runtime_kind = Some(manifest.runtime_kind.clone());
            delivery_mode = manifest.delivery_mode.clone();
            protocol = manifest.protocol.clone();
            source = Some(manifest.source.clone());
            permissions = manifest.permissions.clone();
            credential_requirements = manifest.credential_requirements.clone();
            signature_status = manifest.signature.status.clone();
            has_high_risk_permissions = permissions.iter().any(|p| {
                matches!(
                    p.as_str(),
                    "network.request" | "credentials.use" | "files.writeSelected"
                )
            });
            if manifest.runtime_kind == PluginRuntimeKind::LegacyJs {
                errors.push("公开市场新商品禁止 legacy-js 运行时".into());
            }
            if manifest.source != PluginSource::Marketplace {
                errors.push("公开市场包 manifest.source 必须为 marketplace".into());
            }
            if let Some(min) = manifest.min_app_version.as_deref() {
                compatible = semver_cmp(app_version, min)
                    .map(|ord| ord != std::cmp::Ordering::Less)
                    .unwrap_or(false);
                if !compatible {
                    errors.push(format!(
                        "minAppVersion {} 高于当前应用 {}",
                        min, app_version
                    ));
                }
            }
        }
        Err(e) => {
            errors.push(format!("manifest 无效: {}", e));
            if temp_dir.join("plugin.json").exists() {
                errors.push(
                    "公开市场新商品必须使用 manifest.json，plugin.json 仅用于内部兼容".into(),
                );
            }
        }
    }

    if has_executables || has_scripts || has_suspected_secrets {
        errors.push("发现可执行文件、脚本或疑似密钥，已阻止提交".into());
    }
    let status = if !errors.is_empty() {
        MarketplaceScanStatus::Failed
    } else if has_external_urls || has_absolute_paths || has_high_risk_permissions {
        MarketplaceScanStatus::Warning
    } else {
        MarketplaceScanStatus::Passed
    };
    let report = MarketplacePackageReport {
        ok: errors.is_empty(),
        status,
        manifest_valid,
        schema_version,
        product_id,
        version,
        product_type,
        runtime_kind,
        delivery_mode,
        protocol,
        source,
        file_count: archive.len() as u64,
        compressed_size: metadata.len(),
        unpacked_size,
        sha256,
        signature_status,
        permissions,
        credential_requirements,
        has_executables,
        has_scripts,
        has_suspected_secrets,
        has_external_urls,
        has_absolute_paths,
        has_high_risk_permissions,
        compatible,
        findings,
        errors,
    };
    if !report.ok {
        fs::remove_dir_all(&temp_dir).ok();
    }
    Ok((report, temp_dir))
}

struct TextScan {
    has_secret: bool,
    has_url: bool,
    has_abs_path: bool,
    findings: Vec<MarketplaceRiskFinding>,
}

fn scan_text(text: &str, path: &Path) -> TextScan {
    let mut scan = TextScan {
        has_secret: false,
        has_url: false,
        has_abs_path: false,
        findings: Vec::new(),
    };
    let lower = text.to_ascii_lowercase();
    let assignment = regex::Regex::new(
        r#"(?i)(api[_ -]?key|api[_ -]?secret|token|password)\s*[:=]\s*[\"']?([A-Za-z0-9_./+\-]{12,})"#,
    ).expect("secret assignment regex");
    for captures in assignment.captures_iter(text) {
        let marker = captures.get(1).map(|v| v.as_str()).unwrap_or("secret");
        if !captures
            .get(2)
            .map(|v| v.as_str())
            .unwrap_or_default()
            .starts_with("credential-reference")
        {
            scan.has_secret = true;
            scan.findings.push(finding(
                "high",
                "suspected_secret",
                path,
                "疑似密钥或明文凭据",
                Some(redact_marker(marker)),
            ));
        }
    }
    for marker in ["authorization: bearer", "-----begin private key-----"] {
        if lower.contains(marker) {
            scan.has_secret = true;
            scan.findings.push(finding(
                "high",
                "suspected_secret",
                path,
                "发现禁止分发的鉴权头或私钥材料",
                Some(redact_marker(marker)),
            ));
        }
    }
    if lower.contains("http://") || lower.contains("https://") {
        scan.has_url = true;
        scan.findings.push(finding(
            "medium",
            "external_url",
            path,
            "包含外部 URL，请审核数据流向",
            None,
        ));
    }
    if lower.contains("localhost") || lower.contains("127.0.0.1") {
        scan.findings.push(finding(
            "medium",
            "debug_endpoint",
            path,
            "包含 localhost 调试地址",
            None,
        ));
    }
    if text.contains("C:\\")
        || text.contains("D:\\")
        || lower.contains("/users/")
        || lower.contains("c:/users/")
    {
        scan.has_abs_path = true;
        scan.findings.push(finding(
            "medium",
            "absolute_path",
            path,
            "包含本机绝对路径",
            None,
        ));
    }
    scan
}

fn finding(
    severity: &str,
    category: &str,
    path: &Path,
    message: &str,
    redacted_excerpt: Option<String>,
) -> MarketplaceRiskFinding {
    MarketplaceRiskFinding {
        severity: severity.into(),
        category: category.into(),
        file: path.to_string_lossy().replace('\\', "/"),
        message: message.into(),
        redacted_excerpt,
    }
}

fn redact_marker(marker: &str) -> String {
    format!("{}***", marker.chars().take(6).collect::<String>())
}

fn manifest_consistency_errors(
    product: &DeveloperProduct,
    manifest: &NormalizedPluginManifest,
    expected_version: &str,
) -> Vec<String> {
    let mut errors = Vec::new();
    if manifest.id != product.id {
        errors.push(format!("manifest.id 必须等于商品 ID: {}", product.id));
    }
    if manifest.version != expected_version {
        errors.push(format!(
            "manifest.version 必须等于上传版本: {}",
            expected_version
        ));
    }
    if manifest.product_type != product.product_type {
        errors.push("manifest.productType 与商品资料不一致".into());
    }
    if manifest.runtime_kind != product.runtime_kind {
        errors.push("manifest.runtimeKind 与商品资料不一致".into());
    }
    if manifest.delivery_mode != product.delivery_mode {
        errors.push("manifest.deliveryMode 与商品资料不一致".into());
    }
    if manifest.protocol != product.protocol {
        errors.push("manifest.protocol 与商品资料不一致".into());
    }
    if manifest.source != PluginSource::Marketplace {
        errors.push("manifest.source 必须为 marketplace".into());
    }
    errors
}

fn draft_manifest(product: &DeveloperProduct, version: &str) -> Value {
    json!({
        "schemaVersion": 2,
        "id": product.id,
        "name": product.name,
        "version": version,
        "authorId": product.developer_id,
        "description": product.description,
        "minAppVersion": "1.8.0",
        "productType": product_type_to_str(&product.product_type),
        "runtimeKind": runtime_kind_to_str(&product.runtime_kind),
        "source": "marketplace",
        "deliveryMode": product.delivery_mode,
        "protocol": product.protocol,
        "permissions": default_service_permissions(product.delivery_mode.as_ref()),
        "credentialRequirements": default_credential_requirements(product.delivery_mode.as_ref()),
        "configurationSchema": product.service_configuration,
        "contributes": {},
        "integrity": {},
        "signature": { "status": "unsigned" }
    })
}

fn manifest_to_marketplace_json(manifest: &NormalizedPluginManifest) -> Value {
    json!({
        "schemaVersion": manifest.schema_version,
        "id": manifest.id,
        "name": manifest.name,
        "version": manifest.version,
        "authorId": manifest.author_id,
        "description": manifest.description,
        "icon": manifest.icon,
        "minAppVersion": manifest.min_app_version,
        "productType": product_type_to_str(&manifest.product_type),
        "runtimeKind": runtime_kind_to_str(&manifest.runtime_kind),
        "source": "marketplace",
        "deliveryMode": manifest.delivery_mode,
        "protocol": manifest.protocol,
        "main": manifest.main,
        "styles": manifest.styles,
        "permissions": manifest.permissions,
        "credentialRequirements": manifest.credential_requirements,
        "configurationSchema": manifest.configuration_schema,
        "contributes": manifest.contributes,
        "integrity": manifest.integrity,
        "signature": { "status": "unsigned" }
    })
}

fn transition_submission(
    db: &Database,
    session: &MarketplaceMockSession,
    sub: &crate::models::MarketplaceSubmission,
    action: &str,
    to_status: &str,
    message: Option<String>,
) -> Result<(), AppError> {
    let msg = message.clone();
    let conn = db.conn_lock()?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE product_submissions SET status = ?2, reviewed_by = ?3,
            reviewed_at = datetime('now','localtime'), review_message = ?4 WHERE id = ?1",
        params![sub.id, to_status, session.user_id, msg],
    )?;
    tx.execute(
        "UPDATE products SET status = ?2, review_status = ?2, updated_at = datetime('now','localtime') WHERE id = ?1",
        params![sub.product_id, to_status],
    )?;
    if let Some(version_id) = sub.product_version_id {
        tx.execute(
            "UPDATE product_versions SET status = ?2, review_status = ?2 WHERE id = ?1",
            params![version_id, to_status],
        )?;
    }
    tx.execute(
        "INSERT INTO product_review_events (submission_id, actor_id, action, from_status, to_status, message)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![sub.id, session.user_id, action, review_status_to_str(&sub.status), to_status, message],
    )?;
    tx.commit()?;
    drop(conn);
    write_audit(
        db,
        session,
        &format!("admin_{}", action),
        "submission",
        &sub.id.to_string(),
        json!({ "to": to_status }),
    )?;
    Ok(())
}

fn set_product_status(
    db: &Database,
    session: &MarketplaceMockSession,
    product_id: &str,
    status: &str,
    reason: &str,
) -> Result<(), AppError> {
    let conn = db.conn_lock()?;
    let current: String = conn
        .query_row(
            "SELECT status FROM products WHERE id = ?1",
            [product_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("未找到商品".into()))?;
    let legal = matches!(
        (current.as_str(), status),
        ("published", "suspended")
            | ("published", "delisted")
            | ("suspended", "published")
            | ("approved", "published")
            | ("draft", "delisted")
    );
    if !legal && status != "delisted" {
        return Err(AppError::InvalidInput(format!(
            "非法商品状态转换: {} -> {}",
            current, status
        )));
    }
    conn.execute(
        "UPDATE products SET status = ?2, review_status = ?2, updated_at = datetime('now','localtime') WHERE id = ?1",
        params![product_id, status],
    )?;
    drop(conn);
    write_audit(
        db,
        session,
        &format!("admin_product_{}", status),
        "product",
        product_id,
        json!({ "reason": reason }),
    )?;
    Ok(())
}

fn action_for_submission(
    sub: &crate::models::MarketplaceSubmission,
    message: &str,
) -> MarketplaceActionResult {
    MarketplaceActionResult {
        ok: true,
        product_id: sub.product_id.clone(),
        plugin_id: Some(sub.product_id.clone()),
        message: message.into(),
        requires_permission_confirmation: false,
        permission_diff: Some(PermissionDiff::default()),
        entitlement: None,
        installation: None,
    }
}

fn action_for_product(product_id: &str, message: &str) -> MarketplaceActionResult {
    MarketplaceActionResult {
        ok: true,
        product_id: product_id.into(),
        plugin_id: Some(product_id.into()),
        message: message.into(),
        requires_permission_confirmation: false,
        permission_diff: None,
        entitlement: None,
        installation: None,
    }
}

fn write_audit(
    db: &Database,
    session: &MarketplaceMockSession,
    action: &str,
    target_type: &str,
    target_id: &str,
    details: Value,
) -> Result<(), AppError> {
    let conn = db.conn_lock()?;
    conn.execute(
        "INSERT INTO marketplace_audit_logs
         (actor_id, actor_role, action, target_type, target_id, details_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            session.user_id,
            role_to_str(&session.role),
            action,
            target_type,
            target_id,
            details.to_string(),
        ],
    )?;
    Ok(())
}

fn ensure_developer(session: &MarketplaceMockSession) -> Result<(), AppError> {
    if !session.can_sell {
        return Err(AppError::PluginPermissionDenied {
            plugin_id: None,
            required_permission: Some("role.developer".into()),
        });
    }
    Ok(())
}

fn ensure_admin(session: &MarketplaceMockSession) -> Result<(), AppError> {
    if !session.can_admin {
        return Err(AppError::PluginPermissionDenied {
            plugin_id: None,
            required_permission: Some("role.admin".into()),
        });
    }
    Ok(())
}

fn validate_product_input(input: &DeveloperProductInput) -> Result<(), AppError> {
    if input.name.trim().is_empty() {
        return Err(AppError::InvalidInput("商品名称不能为空".into()));
    }
    if input.runtime_kind == PluginRuntimeKind::LegacyJs {
        return Err(AppError::InvalidInput(
            "公开市场禁止通过 UI 创建 legacy-js 商品".into(),
        ));
    }
    if input.license_type == MarketplaceLicenseType::Free && input.price_amount != 0 {
        return Err(AppError::InvalidInput("免费商品价格必须为 0".into()));
    }
    if input.price_amount < 0 {
        return Err(AppError::InvalidInput("价格不能为负数".into()));
    }
    validate_delivery_input(input)?;
    Ok(())
}

fn validate_delivery_input(input: &DeveloperProductInput) -> Result<(), AppError> {
    let Some(mode) = &input.delivery_mode else {
        return Ok(());
    };
    let config = input.service_configuration.as_ref().unwrap_or(&Value::Null);
    if contains_sensitive_configuration(config) {
        return Err(AppError::InvalidInput(
            "商品配置不得包含 API Key、API Secret、Token、Authorization 或私钥明文".into(),
        ));
    }
    match mode {
        AiServiceDeliveryMode::Byok => {
            if !matches!(
                input.product_type,
                ProductType::XingchenAgent | ProductType::XingchenWorkflow
            ) {
                return Err(AppError::InvalidInput(
                    "BYOK 交付必须选择星辰智能体或星辰工作流商品类型".into(),
                ));
            }
            if input.protocol.as_deref() != Some("xingchen-workflow-v1") {
                return Err(AppError::InvalidInput(
                    "BYOK 星辰商品必须使用 xingchen-workflow-v1 协议".into(),
                ));
            }
            if !input.byok_required {
                return Err(AppError::InvalidInput(
                    "BYOK 商品必须声明用户自备凭据".into(),
                ));
            }
        }
        AiServiceDeliveryMode::HostedApi => {
            let endpoint = config
                .pointer("/endpoint/default")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !endpoint.starts_with("https://") && !endpoint.starts_with("mock://") {
                return Err(AppError::InvalidInput(
                    "Hosted API Endpoint 必须使用 HTTPS；本地演示可使用 mock://".into(),
                ));
            }
        }
        AiServiceDeliveryMode::RemoteMcp => {
            if input.product_type != ProductType::McpConnector {
                return Err(AppError::InvalidInput(
                    "Remote MCP 交付必须选择 MCP 连接器商品类型".into(),
                ));
            }
            let endpoint = config
                .pointer("/serverUrl/default")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let local_dev = cfg!(debug_assertions)
                && (endpoint.starts_with("http://localhost")
                    || endpoint.starts_with("http://127.0.0.1"));
            if !endpoint.starts_with("https://") && !endpoint.starts_with("mock://") && !local_dev {
                return Err(AppError::InvalidInput(
                    "Remote MCP URL 必须使用 HTTPS；localhost 仅限开发构建".into(),
                ));
            }
        }
    }
    Ok(())
}

fn service_metadata_json(input: &DeveloperProductInput) -> Result<Value, AppError> {
    validate_delivery_input(input)?;
    Ok(json!({
        "deliveryMode": input.delivery_mode,
        "protocol": input.protocol,
        "configurationSchema": input.service_configuration,
        "thirdPartyDependencies": input.third_party_dependencies,
    }))
}

fn contains_sensitive_configuration(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            let sensitive = key.contains("apikey")
                || key.contains("api_key")
                || key.contains("apisecret")
                || key.contains("api_secret")
                || key == "token"
                || key.contains("authorization")
                || key.contains("privatekey")
                || key.contains("private_key");
            (sensitive
                && value
                    .as_str()
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false))
                || contains_sensitive_configuration(value)
        }),
        Value::Array(items) => items.iter().any(contains_sensitive_configuration),
        _ => false,
    }
}

fn default_service_permissions(mode: Option<&AiServiceDeliveryMode>) -> Vec<&'static str> {
    match mode {
        Some(AiServiceDeliveryMode::Byok) => vec![
            "credentials.use",
            "agents.invoke",
            "network.xingchen",
            "ai.invoke",
        ],
        Some(AiServiceDeliveryMode::HostedApi) => vec![
            "credentials.use",
            "agents.invoke",
            "network.request",
            "ai.invoke",
        ],
        Some(AiServiceDeliveryMode::RemoteMcp) => {
            vec!["credentials.use", "mcp.connect", "network.request"]
        }
        None => Vec::new(),
    }
}

fn default_credential_requirements(mode: Option<&AiServiceDeliveryMode>) -> Value {
    match mode {
        Some(AiServiceDeliveryMode::Byok) => {
            json!([{"id":"xingchen-workflow","label":"讯飞星辰 Workflow 凭据","provider":"xingchen","fields":["appId","apiKey","apiSecret"],"required":true}])
        }
        Some(AiServiceDeliveryMode::HostedApi) => {
            json!([{"id":"hosted-service-token","label":"开发者托管服务 Token","provider":"hosted-api","fields":["bearerToken"],"required":false}])
        }
        Some(AiServiceDeliveryMode::RemoteMcp) => {
            json!([{"id":"remote-mcp-token","label":"远程 MCP 访问 Token","provider":"remote-mcp","fields":["bearerToken"],"required":false}])
        }
        None => json!([]),
    }
}

fn require_reason(reason: &str, action: &str) -> Result<(), AppError> {
    if reason.trim().is_empty() {
        Err(AppError::InvalidInput(format!("{} 必须填写原因", action)))
    } else {
        Ok(())
    }
}

fn validate_semver(version: &str) -> Result<(), AppError> {
    if semver_parse(version).is_some() {
        Ok(())
    } else {
        Err(AppError::InvalidInput(
            "版本号必须为 x.y.z 语义化版本".into(),
        ))
    }
}

fn ensure_version_increases(
    db: &Database,
    product_id: &str,
    version: &str,
) -> Result<(), AppError> {
    validate_semver(version)?;
    for existing in MarketplaceSupplyService::versions_for_product(db, product_id)? {
        if semver_cmp(&existing.version, version) != Some(std::cmp::Ordering::Less) {
            return Err(AppError::InvalidInput("新版本号必须递增".into()));
        }
    }
    Ok(())
}

fn ensure_version_increases_or_same_draft(
    db: &Database,
    product_id: &str,
    version: &str,
) -> Result<(), AppError> {
    validate_semver(version)?;
    let versions = MarketplaceSupplyService::versions_for_product(db, product_id)?;
    for existing in versions {
        if existing.version == version
            && matches!(
                existing.status,
                MarketplaceReviewStatus::Draft | MarketplaceReviewStatus::Rejected
            )
        {
            return Ok(());
        }
        if semver_cmp(&existing.version, version) != Some(std::cmp::Ordering::Less) {
            return Err(AppError::InvalidInput(
                "已发布或已提交版本不可覆盖，新版本号必须递增".into(),
            ));
        }
    }
    Ok(())
}

fn unique_product_id(name: &str) -> String {
    let slug: String = name
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_whitespace() || c == '-' || c == '_' {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(32)
        .collect();
    format!(
        "local-demo-developer-{}-{}",
        if slug.is_empty() { "product" } else { &slug },
        &Uuid::new_v4().to_string()[..8]
    )
}

fn copy_dir(source: &Path, dest: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dest)?;
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|e| AppError::Custom(e.to_string()))?;
        let rel = entry
            .path()
            .strip_prefix(source)
            .map_err(|e| AppError::Custom(e.to_string()))?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), target)?;
        } else {
            return Err(AppError::InvalidInput("不允许符号链接或特殊文件".into()));
        }
    }
    Ok(())
}

fn is_zip_symlink(mode: Option<u32>) -> bool {
    mode.map(|m| (m & 0o170000) == 0o120000).unwrap_or(false)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn semver_parse(v: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}

fn semver_cmp(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    Some(semver_parse(left)?.cmp(&semver_parse(right)?))
}

fn parse_product_type(value: &str) -> ProductType {
    serde_json::from_value(json!(value)).unwrap_or(ProductType::LocalPlugin)
}

fn parse_runtime_kind(value: &str) -> PluginRuntimeKind {
    serde_json::from_value(json!(value)).unwrap_or(PluginRuntimeKind::DeclarativeUi)
}

fn parse_license(value: &str) -> MarketplaceLicenseType {
    serde_json::from_value(json!(value)).unwrap_or(MarketplaceLicenseType::Free)
}

fn parse_review_status(value: &str) -> MarketplaceReviewStatus {
    serde_json::from_value(json!(value)).unwrap_or(MarketplaceReviewStatus::Draft)
}

fn parse_scan_status(value: &str) -> MarketplaceScanStatus {
    serde_json::from_value(json!(value)).unwrap_or(MarketplaceScanStatus::NotScanned)
}

fn role_to_str(value: &MarketplaceMockRole) -> &'static str {
    match value {
        MarketplaceMockRole::Customer => "customer",
        MarketplaceMockRole::Developer => "developer",
        MarketplaceMockRole::Admin => "admin",
    }
}

fn review_status_to_str(value: &MarketplaceReviewStatus) -> &'static str {
    match value {
        MarketplaceReviewStatus::Draft => "draft",
        MarketplaceReviewStatus::Submitted => "submitted",
        MarketplaceReviewStatus::UnderReview => "under_review",
        MarketplaceReviewStatus::Approved => "approved",
        MarketplaceReviewStatus::Published => "published",
        MarketplaceReviewStatus::Rejected => "rejected",
        MarketplaceReviewStatus::Suspended => "suspended",
        MarketplaceReviewStatus::Delisted => "delisted",
        MarketplaceReviewStatus::Revoked => "revoked",
    }
}

fn scan_status_to_str(value: &MarketplaceScanStatus) -> &'static str {
    match value {
        MarketplaceScanStatus::NotScanned => "not_scanned",
        MarketplaceScanStatus::Passed => "passed",
        MarketplaceScanStatus::Warning => "warning",
        MarketplaceScanStatus::Failed => "failed",
    }
}

fn product_type_to_str(value: &ProductType) -> &'static str {
    match value {
        ProductType::LocalPlugin => "local-plugin",
        ProductType::DeclarativeUi => "declarative-ui",
        ProductType::PromptPack => "prompt-pack",
        ProductType::XingchenAgent => "xingchen-agent",
        ProductType::XingchenWorkflow => "xingchen-workflow",
        ProductType::XingchenMcp => "xingchen-mcp",
        ProductType::McpConnector => "mcp-connector",
        ProductType::KnowledgeTemplate => "knowledge-template",
        ProductType::DatabaseTemplate => "database-template",
        ProductType::FileImageAgent => "file-image-agent",
        ProductType::PptMasterExtension => "ppt-master-extension",
        ProductType::LearningAssistantExtension => "learning-assistant-extension",
    }
}

fn runtime_kind_to_str(value: &PluginRuntimeKind) -> &'static str {
    match value {
        PluginRuntimeKind::LegacyJs => "legacy-js",
        PluginRuntimeKind::DeclarativeUi => "declarative-ui",
        PluginRuntimeKind::PromptPack => "prompt-pack",
        PluginRuntimeKind::XingchenAgent => "xingchen-agent",
        PluginRuntimeKind::XingchenWorkflow => "xingchen-workflow",
        PluginRuntimeKind::XingchenMcp => "xingchen-mcp",
        PluginRuntimeKind::McpConnector => "mcp-connector",
        PluginRuntimeKind::PptExtension => "ppt-extension",
        PluginRuntimeKind::LearningExtension => "learning-extension",
    }
}

fn license_to_str(value: &MarketplaceLicenseType) -> &'static str {
    match value {
        MarketplaceLicenseType::Free => "free",
        MarketplaceLicenseType::OneTime => "one_time",
        MarketplaceLicenseType::Subscription => "subscription",
    }
}

fn bool_int(v: bool) -> i32 {
    if v {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "firstwork-supply-test-{}-{}",
            name,
            std::process::id()
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn developer_session() -> MarketplaceMockSession {
        MarketplaceSupplyService::session_for_role(MarketplaceMockRole::Developer)
    }

    fn admin_session() -> MarketplaceMockSession {
        MarketplaceSupplyService::session_for_role(MarketplaceMockRole::Admin)
    }

    fn product_input() -> DeveloperProductInput {
        DeveloperProductInput {
            name: "Demo Prompt Pack".into(),
            description: "Prompt demo".into(),
            full_description: Some("Prompt demo full".into()),
            icon: None,
            product_type: ProductType::PromptPack,
            runtime_kind: PluginRuntimeKind::PromptPack,
            category: Some("prompt".into()),
            tags: vec!["demo".into()],
            byok_required: false,
            delivery_mode: None,
            protocol: None,
            service_configuration: None,
            third_party_dependencies: None,
            file_upload_required: false,
            data_destination: Some("本地".into()),
            privacy_notice: Some("不上传数据".into()),
            usage_guide: Some("安装后使用".into()),
            license_type: MarketplaceLicenseType::Free,
            price_amount: 0,
            support_period: Some("MVP".into()),
        }
    }

    fn write_zip(
        dir: &Path,
        product_id: &str,
        version: &str,
        extra_file: Option<(&str, &str)>,
    ) -> PathBuf {
        let zip_path = dir.join(format!("{}.zip", version));
        let file = File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        let manifest = json!({
            "schemaVersion": 2,
            "id": product_id,
            "name": "Demo Prompt Pack",
            "version": version,
            "authorId": DEVELOPER_ID,
            "description": "Prompt demo",
            "minAppVersion": "1.8.0",
            "productType": "prompt-pack",
            "runtimeKind": "prompt-pack",
            "source": "marketplace",
            "permissions": ["prompts.register"],
            "credentialRequirements": [],
            "contributes": { "prompts": [{ "id": "demo", "title": "Demo" }] },
            "signature": { "status": "unsigned" }
        });
        zip.start_file("manifest.json", opts).unwrap();
        zip.write_all(manifest.to_string().as_bytes()).unwrap();
        zip.start_file("README.md", opts).unwrap();
        zip.write_all(b"demo").unwrap();
        if let Some((name, body)) = extra_file {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        zip_path
    }

    #[test]
    fn customer_cannot_call_developer_flow() {
        let customer = MarketplaceSupplyService::session_for_role(MarketplaceMockRole::Customer);
        assert!(ensure_developer(&customer).is_err());
    }

    #[test]
    fn developer_delivery_metadata_rejects_secrets_and_insecure_endpoints() {
        let mut byok = product_input();
        byok.product_type = ProductType::XingchenWorkflow;
        byok.runtime_kind = PluginRuntimeKind::XingchenWorkflow;
        byok.byok_required = true;
        byok.delivery_mode = Some(AiServiceDeliveryMode::Byok);
        byok.protocol = Some("xingchen-workflow-v1".into());
        byok.service_configuration = Some(json!({"apiSecret":"real-secret-material"}));
        assert!(validate_product_input(&byok).is_err());

        let mut hosted = product_input();
        hosted.product_type = ProductType::XingchenAgent;
        hosted.runtime_kind = PluginRuntimeKind::XingchenAgent;
        hosted.delivery_mode = Some(AiServiceDeliveryMode::HostedApi);
        hosted.protocol = Some("hosted-api".into());
        hosted.service_configuration =
            Some(json!({"endpoint":{"default":"http://example.com/api"}}));
        assert!(validate_product_input(&hosted).is_err());

        let labels_only = scan_text("需要用户填写 API Key 和 API Secret", Path::new("README.md"));
        assert!(!labels_only.has_secret);
        let actual_secret = scan_text("api_key = abcdefghijklmnop", Path::new("config.txt"));
        assert!(actual_secret.has_secret);
    }

    #[test]
    fn developer_cannot_call_admin_flow() {
        let dev = developer_session();
        assert!(ensure_admin(&dev).is_err());
    }

    #[test]
    fn create_upload_submit_and_approve_flow() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("approve");
        let dev = developer_session();
        let admin = admin_session();
        let product =
            MarketplaceSupplyService::developer_create_product(&db, &dev, product_input()).unwrap();
        let zip = write_zip(&dir, &product.id, "1.0.0", None);
        let report = MarketplaceSupplyService::developer_upload_package(
            &db,
            &dir,
            &dev,
            DeveloperUploadPackageInput {
                product_id: product.id.clone(),
                version: "1.0.0".into(),
                zip_path: zip.to_string_lossy().into(),
                changelog: Some("init".into()),
            },
            "1.8.0",
        )
        .unwrap();
        assert!(
            report.ok,
            "report errors: {:?}, findings: {:?}",
            report.errors, report.findings
        );
        MarketplaceSupplyService::developer_submit_product(
            &db,
            &dev,
            DeveloperSubmitInput {
                product_id: product.id.clone(),
                version: Some("1.0.0".into()),
            },
        )
        .unwrap();
        let submissions =
            MarketplaceSupplyService::admin_list_submissions(&db, &admin, None).unwrap();
        assert_eq!(submissions.len(), 1);
        MarketplaceSupplyService::admin_approve_submission(
            &db,
            &admin,
            AdminReviewInput {
                submission_id: submissions[0].id,
                message: Some("ok".into()),
            },
        )
        .unwrap();
        let updated = MarketplaceSupplyService::read_developer_product(&db, &product.id).unwrap();
        assert_eq!(updated.status, MarketplaceReviewStatus::Published);
    }

    #[test]
    fn upload_blocks_suspected_secret_and_script() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("secret");
        let dev = developer_session();
        let product =
            MarketplaceSupplyService::developer_create_product(&db, &dev, product_input()).unwrap();
        let zip = write_zip(
            &dir,
            &product.id,
            "1.0.0",
            Some(("main.js", "const api_key='sk-1234567890abcdef';")),
        );
        let report = MarketplaceSupplyService::developer_upload_package(
            &db,
            &dir,
            &dev,
            DeveloperUploadPackageInput {
                product_id: product.id,
                version: "1.0.0".into(),
                zip_path: zip.to_string_lossy().into(),
                changelog: None,
            },
            "1.8.0",
        )
        .unwrap();
        assert!(!report.ok);
        assert!(report.has_suspected_secrets);
        assert!(report.has_scripts);
    }

    #[test]
    fn reject_requires_reason() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("reject");
        let dev = developer_session();
        let admin = admin_session();
        let product =
            MarketplaceSupplyService::developer_create_product(&db, &dev, product_input()).unwrap();
        let zip = write_zip(&dir, &product.id, "1.0.0", None);
        MarketplaceSupplyService::developer_upload_package(
            &db,
            &dir,
            &dev,
            DeveloperUploadPackageInput {
                product_id: product.id.clone(),
                version: "1.0.0".into(),
                zip_path: zip.to_string_lossy().into(),
                changelog: None,
            },
            "1.8.0",
        )
        .unwrap();
        MarketplaceSupplyService::developer_submit_product(
            &db,
            &dev,
            DeveloperSubmitInput {
                product_id: product.id,
                version: Some("1.0.0".into()),
            },
        )
        .unwrap();
        let sub = MarketplaceSupplyService::admin_list_submissions(&db, &admin, None)
            .unwrap()
            .remove(0);
        assert!(MarketplaceSupplyService::admin_reject_submission(
            &db,
            &admin,
            AdminReviewInput {
                submission_id: sub.id,
                message: None
            }
        )
        .is_err());
    }
}
