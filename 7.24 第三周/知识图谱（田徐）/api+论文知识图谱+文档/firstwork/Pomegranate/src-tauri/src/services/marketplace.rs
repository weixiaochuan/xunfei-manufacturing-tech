//! Local AI marketplace service.
//!
//! Pomegranate stores listing metadata, local plugin installs and external
//! authorization bindings. It does not implement real payment, settlement or
//! provider credential exchange.

use chrono::{Duration, Local};
use reqwest::Url;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    AiServiceDeliveryMode, MarketplaceAcquireInput, MarketplaceActionResult,
    MarketplaceEntitlement, MarketplaceEntitlementStatus, MarketplaceExternalAuthorizationInput,
    MarketplaceInstallInput, MarketplaceLedgerEntry, MarketplaceLicenseType,
    MarketplaceMockTestResult, MarketplaceOrder, MarketplacePrice, MarketplaceProductDetail,
    MarketplaceProductQuery, MarketplaceProductStatus, MarketplaceProductSummary,
    MarketplaceRefundInput, MarketplaceReviewInfo, MarketplaceReviewInput,
    MarketplaceServiceConfigurationInput, MarketplaceUpdateInfo, MarketplaceUpdateInput,
    McpServerInput, NormalizedPluginManifest, PermissionDiff, PluginCredentialRequirement,
    PluginInfo, PluginInstallationInfo, PluginRuntimeKind, PluginSource, ProductType,
    SignatureStatus,
};
use crate::services::plugins::PluginService;

const LOCAL_USER_ID: &str = "local-demo-buyer";
const PLATFORM_FEE_BPS: i64 = 2000;
const MARKETPLACE_DIR: &str = "marketplace";
const PACKAGES_DIR: &str = "packages";
const BACKUPS_DIR: &str = "plugin-backups";
const PLUGINS_DIR: &str = "plugins";

#[derive(Clone)]
struct SeedVersion {
    version: &'static str,
    status: &'static str,
    changelog: &'static str,
    permissions: Vec<&'static str>,
    configuration_schema: serde_json::Value,
}

#[derive(Clone)]
struct SeedProduct {
    id: &'static str,
    plugin_id: &'static str,
    name: &'static str,
    developer_id: &'static str,
    developer_name: &'static str,
    description: &'static str,
    full_description: &'static str,
    icon: &'static str,
    product_type: ProductType,
    runtime_kind: PluginRuntimeKind,
    license_type: MarketplaceLicenseType,
    amount: i64,
    byok_required: bool,
    data_destination: &'static str,
    file_upload_notice: &'static str,
    risk_notes: Vec<&'static str>,
    credential_requirements: Vec<PluginCredentialRequirement>,
    versions: Vec<SeedVersion>,
}

#[derive(Debug)]
struct ProductRow {
    id: String,
    plugin_id: String,
    developer_id: String,
    developer_name: String,
    seller_user_id: String,
    seller_nickname: Option<String>,
    name: String,
    description: String,
    icon: Option<String>,
    product_type: String,
    status: String,
    license_type: String,
    byok_required: bool,
    mock_mode: bool,
    data_destination: Option<String>,
    file_upload_notice: Option<String>,
    risk_notes_json: String,
}

#[derive(Debug)]
struct VersionRow {
    id: i64,
    product_id: String,
    version: String,
    manifest_json: String,
    runtime_kind: String,
    source: String,
    content_hash: String,
    signature_status: String,
    min_app_version: Option<String>,
    status: String,
    changelog: String,
    package_path: Option<String>,
}

pub struct MarketplaceService;

impl MarketplaceService {
    fn current_user_id(db: &Database) -> Result<String, AppError> {
        Ok(db
            .get_config("marketplace.current_user_id")?
            .unwrap_or_else(|| LOCAL_USER_ID.into()))
    }

    pub fn list_products(
        db: &Database,
        data_dir: &Path,
        query: MarketplaceProductQuery,
    ) -> Result<Vec<MarketplaceProductSummary>, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let rows = Self::product_rows(db)?;
        let mut out = Vec::new();
        for row in rows {
            let summary = Self::summary_for_product(db, &row)?;
            if Self::matches_query(&summary, &query) {
                out.push(summary);
            }
        }
        Ok(out)
    }

    pub fn get_product(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
    ) -> Result<MarketplaceProductDetail, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        let display_version = Self::display_version(db, product_id)?;
        let manifest = manifest_from_row(&display_version)?;
        let summary = Self::summary_for_product(db, &row)?;
        let entitlement = Self::entitlement(db, product_id)?;
        let installation = db.get_plugin_installation(&row.plugin_id)?;
        let (permission_diff, configuration_changed) = if let Some(installed) = &installation {
            let current = Self::version_by_id(db, installed.product_version_id.unwrap_or(-1))
                .ok()
                .and_then(|v| manifest_from_row(&v).ok());
            let current_permissions = current
                .as_ref()
                .map(|m| m.permissions.clone())
                .unwrap_or_default();
            let config_changed = current
                .as_ref()
                .map(|m| {
                    m.configuration_schema != manifest.configuration_schema
                        || m.delivery_mode != manifest.delivery_mode
                        || m.protocol != manifest.protocol
                })
                .unwrap_or(false);
            (
                Some(PluginService::compare_permissions(
                    current_permissions,
                    manifest.permissions.clone(),
                )),
                config_changed,
            )
        } else {
            (None, false)
        };
        Ok(MarketplaceProductDetail {
            full_description: row.description.clone(),
            changelog: display_version.changelog,
            credential_requirements: manifest.credential_requirements.clone(),
            configuration_schema: manifest.configuration_schema.clone(),
            file_upload_notice: row.file_upload_notice,
            data_destination: row.data_destination,
            license_type: parse_license(&row.license_type),
            entitlement,
            installation,
            integrity_status: "managed".into(),
            permission_diff,
            configuration_changed,
            blocked_reason: summary.risk_notes.first().cloned(),
            manifest,
            summary,
        })
    }

    pub fn get_product_version(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
        version: Option<String>,
    ) -> Result<NormalizedPluginManifest, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = match version {
            Some(v) => Self::version_by_product_version(db, product_id, &v)?,
            None => Self::active_version(db, product_id)?,
        };
        manifest_from_row(&row)
    }

    pub fn acquire_product(
        db: &Database,
        data_dir: &Path,
        input: MarketplaceAcquireInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, &input.product_id)?;
        ensure_product_active(&row)?;
        let buyer_user_id = Self::current_user_id(db)?;
        if buyer_user_id == row.seller_user_id {
            return Err(AppError::InvalidInput("不能购买自己发布的商品".into()));
        }
        let version = Self::active_version(db, &row.id)?;
        let manifest = manifest_from_row(&version)?;
        if let Some(existing) = Self::entitlement(db, &row.id)? {
            return Ok(MarketplaceActionResult {
                ok: true,
                product_id: row.id,
                plugin_id: Some(row.plugin_id.clone()),
                message: "该商品已存在本机授权或外部授权绑定".into(),
                requires_permission_confirmation: false,
                permission_diff: None,
                entitlement: Some(existing),
                installation: None,
            });
        }
        if manifest.delivery_mode.is_some() {
            return Ok(MarketplaceActionResult {
                ok: false,
                product_id: row.id,
                plugin_id: Some(row.plugin_id.clone()),
                message: "AI 服务商品不通过 Pomegranate 本地模拟支付获取；请先在讯飞星辰平台或开发者处取得授权，然后回到本机绑定授权。".into(),
                requires_permission_confirmation: false,
                permission_diff: None,
                entitlement: None,
                installation: None,
            });
        }

        let license = input
            .license_type
            .unwrap_or_else(|| parse_license(&row.license_type));
        let price = Self::price_for_product(db, &row.id)?;
        let conn = db.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        let expires_at = if license == MarketplaceLicenseType::Subscription {
            Some(
                (Local::now() + Duration::days(30))
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            )
        } else {
            None
        };
        let platform_fee = price.amount * PLATFORM_FEE_BPS / 10_000;
        let seller_income = price.amount - platform_fee;
        tx.execute(
            "INSERT INTO orders
                (local_user_id, buyer_user_id, seller_user_id, status, payment_status, settlement_status,
                 refund_status, currency, total_amount, gross_amount, platform_fee, seller_income,
                 is_mock, completed_at)
             VALUES
                (?1, ?1, ?2, ?3, ?4, 'settled', 'none', ?5, ?6, ?6, ?7, ?8, 1,
                 CASE WHEN ?4 = 'paid' THEN datetime('now','localtime') ELSE NULL END)",
            params![
                buyer_user_id,
                row.seller_user_id,
                if license == MarketplaceLicenseType::Free { "completed" } else { "completed" },
                if license == MarketplaceLicenseType::Free { "paid" } else { "paid" },
                price.currency,
                price.amount,
                platform_fee,
                seller_income,
            ],
        )?;
        let order_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO order_items
                (order_id, product_id, product_version_id, amount, seller_user_id, currency,
                 gross_amount, platform_fee, seller_income, price_snapshot_json, version_snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?4, ?7, ?8, ?9, ?10)",
            params![
                order_id,
                row.id,
                version.id,
                price.amount,
                row.seller_user_id,
                price.currency,
                platform_fee,
                seller_income,
                json!({
                    "currency": price.currency,
                    "amount": price.amount,
                    "priceType": license_to_str(&license),
                    "isMock": true
                })
                .to_string(),
                version.version,
            ],
        )?;
        let order_item_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO entitlements
                (product_id, entitlement_type, status, issued_at, expires_at, local_user_id,
                 owner_user_id, order_id, order_item_id)
             VALUES (?1, ?2, 'active', datetime('now','localtime'), ?3, ?4, ?4, ?5, ?6)",
            params![
                row.id,
                license_to_str(&license),
                expires_at,
                buyer_user_id,
                order_id,
                order_item_id,
            ],
        )?;
        if price.amount > 0 {
            insert_ledger_tx(
                &tx,
                "buyer_payment",
                Some(order_id),
                Some(order_item_id),
                Some(&buyer_user_id),
                Some(&row.seller_user_id),
                Some(&row.id),
                price.amount,
                &price.currency,
                "模拟买家付款",
            )?;
            insert_ledger_tx(
                &tx,
                "platform_fee",
                Some(order_id),
                Some(order_item_id),
                Some(&buyer_user_id),
                Some(&row.seller_user_id),
                Some(&row.id),
                platform_fee,
                &price.currency,
                "模拟平台服务费 20%",
            )?;
            insert_ledger_tx(
                &tx,
                "seller_income",
                Some(order_id),
                Some(order_item_id),
                Some(&buyer_user_id),
                Some(&row.seller_user_id),
                Some(&row.id),
                seller_income,
                &price.currency,
                "模拟创作者收入",
            )?;
        }
        tx.commit()?;
        drop(conn);
        let entitlement = Self::entitlement(db, &row.id)?;
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_acquire",
            Some(license_to_str(&license)),
        )
        .ok();
        Ok(MarketplaceActionResult {
            ok: true,
            product_id: row.id,
            plugin_id: Some(row.plugin_id.clone()),
            message: if license == MarketplaceLicenseType::Free {
                "免费获取成功，已生成本地演示许可证".into()
            } else {
                "模拟购买成功，不会真实扣款，已生成本地演示许可证".into()
            },
            requires_permission_confirmation: false,
            permission_diff: None,
            entitlement,
            installation: None,
        })
    }

    pub fn bind_external_authorization(
        db: &Database,
        data_dir: &Path,
        input: MarketplaceExternalAuthorizationInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, &input.product_id)?;
        ensure_product_active(&row)?;
        let user_id = Self::current_user_id(db)?;
        if user_id == row.seller_user_id {
            return Err(AppError::InvalidInput(
                "不能给自己发布的商品绑定买家授权".into(),
            ));
        }
        let version = Self::active_version(db, &row.id)?;
        let manifest = manifest_from_row(&version)?;
        if manifest.delivery_mode.is_none() {
            return Err(AppError::InvalidInput(
                "该商品不是外部 AI 服务交付商品，请使用普通获取流程".into(),
            ));
        }
        if let Some(existing) = Self::entitlement(db, &row.id)? {
            if matches!(
                existing.status,
                MarketplaceEntitlementStatus::Active
                    | MarketplaceEntitlementStatus::ExternalAuthorized
            ) {
                return Ok(MarketplaceActionResult {
                    ok: true,
                    product_id: row.id,
                    plugin_id: Some(row.plugin_id.clone()),
                    message: "外部授权已绑定到当前本地账号".into(),
                    requires_permission_confirmation: false,
                    permission_diff: None,
                    entitlement: Some(existing),
                    installation: None,
                });
            }
            let conn = db.conn_lock()?;
            conn.execute(
                "UPDATE entitlements
                 SET status = 'external_authorized',
                     entitlement_type = ?2,
                     expires_at = NULL,
                     order_id = NULL,
                     order_item_id = NULL,
                     revoked_at = NULL,
                     revoked_reason = NULL,
                     issued_at = datetime('now','localtime')
                 WHERE id = ?1",
                params![
                    existing.id,
                    license_to_str(&parse_license(&row.license_type))
                ],
            )?;
        } else {
            let conn = db.conn_lock()?;
            conn.execute(
                "INSERT INTO entitlements
                    (product_id, entitlement_type, status, issued_at, expires_at, local_user_id,
                     owner_user_id, order_id, order_item_id)
                 VALUES (?1, ?2, 'external_authorized', datetime('now','localtime'),
                         NULL, ?3, ?3, NULL, NULL)",
                params![
                    row.id,
                    license_to_str(&parse_license(&row.license_type)),
                    user_id
                ],
            )?;
        }
        let audit_detail = json!({
            "deliveryMode": manifest.delivery_mode,
            "protocol": manifest.protocol,
            "externalReference": input.external_reference.as_deref().map(mask_external_reference),
            "note": input.note.as_deref().map(|value| value.chars().take(120).collect::<String>()),
        });
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_external_authorization_bound",
            Some(&audit_detail.to_string()),
        )
        .ok();
        Ok(MarketplaceActionResult {
            ok: true,
            product_id: row.id.clone(),
            plugin_id: Some(row.plugin_id.clone()),
            message: "已绑定外部授权。本机只保存授权状态和后续 credentialId，不保存开发者密钥，也不代表 Pomegranate 已完成真实支付。".into(),
            requires_permission_confirmation: false,
            permission_diff: None,
            entitlement: Self::entitlement(db, &row.id)?,
            installation: None,
        })
    }

    pub fn list_entitlements(
        db: &Database,
        data_dir: &Path,
    ) -> Result<Vec<MarketplaceEntitlement>, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let user_id = Self::current_user_id(db)?;
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, product_id, entitlement_type, status, issued_at, expires_at, owner_user_id, order_id
             FROM entitlements
             WHERE COALESCE(owner_user_id, local_user_id) = ?1
             ORDER BY issued_at DESC",
        )?;
        let rows = stmt
            .query_map([user_id.as_str()], entitlement_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)?;
        Ok(rows)
    }

    pub fn install_product(
        db: &Database,
        data_dir: &Path,
        input: MarketplaceInstallInput,
        app_version: &str,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, &input.product_id)?;
        ensure_product_active(&row)?;
        Self::ensure_entitled(db, &row.id)?;
        let version = match input.version {
            Some(v) => Self::version_by_product_version(db, &row.id, &v)?,
            None => Self::active_version(db, &row.id)?,
        };
        ensure_version_installable(&version)?;
        let manifest = manifest_from_row(&version)?;
        if !input.confirm_permissions && !manifest.permissions.is_empty() {
            return Ok(permission_confirmation_result(
                &row,
                manifest.permissions.clone(),
                "安装前需要确认权限",
            ));
        }
        let compatibility =
            PluginService::check_compatibility(manifest.min_app_version.clone(), app_version);
        if !compatibility.compatible {
            return Err(AppError::InvalidInput(
                compatibility
                    .reason
                    .unwrap_or_else(|| "应用版本不兼容".into()),
            ));
        }
        if manifest.source != PluginSource::Marketplace {
            return Err(AppError::InvalidInput(
                "市场安装只接受 marketplace 来源包".into(),
            ));
        }
        if manifest.runtime_kind == PluginRuntimeKind::LegacyJs {
            return Err(AppError::InvalidInput(
                "公开市场插件禁止 legacy-js 运行时".into(),
            ));
        }
        if matches!(
            parse_signature(&version.signature_status),
            SignatureStatus::Invalid | SignatureStatus::Revoked
        ) {
            return Err(AppError::InvalidInput("签名无效或已吊销，禁止安装".into()));
        }

        let source = package_path(&version)?;
        let install = Self::copy_and_register(db, data_dir, &row, &version, &source, false)?;
        if manifest.runtime_kind == PluginRuntimeKind::PromptPack {
            Self::install_prompt_pack(db, &row.id, &version.version)?;
        }
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_permissions_granted",
            Some(&manifest.permissions.join(",")),
        )
        .ok();
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_install",
            Some(&version.version),
        )
        .ok();
        Ok(MarketplaceActionResult {
            ok: true,
            product_id: row.id,
            plugin_id: Some(row.plugin_id.clone()),
            message: "安装成功，未签名商品已按本地演示模式标记".into(),
            requires_permission_confirmation: false,
            permission_diff: None,
            entitlement: Self::entitlement(db, &input.product_id)?,
            installation: Some(install),
        })
    }

    pub fn enable_product(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        let installation = db
            .get_plugin_installation(&row.plugin_id)?
            .ok_or_else(|| AppError::NotFound("商品尚未安装".into()))?;
        let version = Self::version_by_id(db, installation.product_version_id.unwrap_or(-1))?;
        if version.status == "revoked" || version.signature_status == "revoked" {
            db.write_audit_log(&row.plugin_id, "marketplace_enable_blocked_revoked", None)
                .ok();
            return Err(AppError::InvalidInput("该版本已吊销，禁止启用".into()));
        }
        Self::ensure_entitled(db, product_id)?;
        PluginService::enable(db, &row.plugin_id)?;
        db.write_audit_log(&row.plugin_id, "marketplace_enable", None)
            .ok();
        Ok(action_ok(&row, "已启用"))
    }

    pub fn disable_product(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        PluginService::disable(db, &row.plugin_id)?;
        db.write_audit_log(&row.plugin_id, "marketplace_disable", None)
            .ok();
        Ok(action_ok(&row, "已禁用"))
    }

    pub fn record_permission_rejection(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
        action: &str,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        let audit_action = match action {
            "install" => "marketplace_install_permission_rejected",
            "update" => "marketplace_update_permission_rejected",
            _ => {
                return Err(AppError::InvalidInput(
                    "unsupported permission rejection action".into(),
                ))
            }
        };
        db.write_audit_log(&row.plugin_id, audit_action, None).ok();
        Ok(action_ok(&row, "permission confirmation cancelled"))
    }

    pub fn uninstall_product(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        if row.product_type == "prompt-pack" {
            Self::uninstall_prompt_pack(db, product_id)?;
        }
        let plugin_dir = plugins_dir(data_dir)?.join(&row.plugin_id);
        if plugin_dir.exists() {
            fs::remove_dir_all(&plugin_dir)?;
        }
        let remote_registry_name = format!("marketplace:{}", row.plugin_id);
        if let Some(server) = db
            .list_mcp_servers()?
            .into_iter()
            .find(|server| server.name == remote_registry_name)
        {
            db.delete_mcp_server(server.id)?;
        }
        db.delete_plugin(&row.plugin_id)?;
        db.write_audit_log(&row.plugin_id, "marketplace_uninstall", None)
            .ok();
        Ok(action_ok(&row, "已卸载，许可证仍保留，可重新安装"))
    }

    pub fn check_updates(
        db: &Database,
        data_dir: &Path,
    ) -> Result<Vec<MarketplaceUpdateInfo>, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let mut out = Vec::new();
        for row in Self::product_rows(db)? {
            if let Some(installed) = db.get_plugin_installation(&row.plugin_id)? {
                if let Some(next) = Self::update_version(db, &row.id)? {
                    let current_permissions =
                        Self::version_by_id(db, installed.product_version_id.unwrap_or(-1))
                            .ok()
                            .and_then(|v| manifest_from_row(&v).ok())
                            .map(|m| m.permissions)
                            .unwrap_or_default();
                    let next_permissions = manifest_from_row(&next)?.permissions;
                    let diff =
                        PluginService::compare_permissions(current_permissions, next_permissions);
                    out.push(MarketplaceUpdateInfo {
                        product_id: row.id,
                        plugin_id: row.plugin_id,
                        installed_version: Some(installed.installed_version),
                        latest_version: next.version,
                        has_update: true,
                        permission_diff: diff,
                        changelog: next.changelog,
                        blocked_reason: if next.status == "revoked" {
                            Some("目标版本已吊销".into())
                        } else {
                            None
                        },
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn update_product(
        db: &Database,
        data_dir: &Path,
        input: MarketplaceUpdateInput,
        app_version: &str,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, &input.product_id)?;
        Self::ensure_entitled(db, &row.id)?;
        let installed = db
            .get_plugin_installation(&row.plugin_id)?
            .ok_or_else(|| AppError::NotFound("商品尚未安装".into()))?;
        let next = Self::update_version(db, &row.id)?
            .ok_or_else(|| AppError::InvalidInput("暂无可用更新".into()))?;
        ensure_version_installable(&next)?;
        let current_manifest = Self::version_by_id(db, installed.product_version_id.unwrap_or(-1))
            .ok()
            .and_then(|v| manifest_from_row(&v).ok());
        let next_manifest = manifest_from_row(&next)?;
        let diff = PluginService::compare_permissions(
            current_manifest.map(|m| m.permissions).unwrap_or_default(),
            next_manifest.permissions.clone(),
        );
        if !diff.added.is_empty() && !input.confirm_added_permissions {
            return Ok(MarketplaceActionResult {
                ok: false,
                product_id: row.id,
                plugin_id: Some(row.plugin_id.clone()),
                message: "更新新增了权限，需要重新确认".into(),
                requires_permission_confirmation: true,
                permission_diff: Some(diff),
                entitlement: Self::entitlement(db, &input.product_id)?,
                installation: Some(installed),
            });
        }
        let compatibility =
            PluginService::check_compatibility(next_manifest.min_app_version.clone(), app_version);
        if !compatibility.compatible {
            return Err(AppError::InvalidInput(
                compatibility
                    .reason
                    .unwrap_or_else(|| "应用版本不兼容".into()),
            ));
        }
        let source = package_path(&next)?;
        let install = Self::copy_and_register(db, data_dir, &row, &next, &source, true)?;
        if next_manifest.runtime_kind == PluginRuntimeKind::PromptPack {
            Self::install_prompt_pack(db, &row.id, &next.version)?;
        }
        db.write_audit_log(&row.plugin_id, "marketplace_update", Some(&next.version))
            .ok();
        Ok(MarketplaceActionResult {
            ok: true,
            product_id: row.id,
            plugin_id: Some(row.plugin_id.clone()),
            message: "更新成功，已保留上一版本备份用于演示回滚".into(),
            requires_permission_confirmation: false,
            permission_diff: Some(diff),
            entitlement: Self::entitlement(db, &input.product_id)?,
            installation: Some(install),
        })
    }

    pub fn verify_installation(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        let check = PluginService::verify_installation(db, &row.plugin_id)?;
        Ok(MarketplaceActionResult {
            ok: check.ok,
            product_id: row.id,
            plugin_id: Some(row.plugin_id.clone()),
            message: check.message.unwrap_or_else(|| "完整性校验通过".into()),
            requires_permission_confirmation: false,
            permission_diff: None,
            entitlement: Self::entitlement(db, product_id)?,
            installation: db.get_plugin_installation(&row.plugin_id)?,
        })
    }

    pub fn list_installed(
        db: &Database,
        data_dir: &Path,
    ) -> Result<Vec<MarketplaceProductSummary>, AppError> {
        let mut query = MarketplaceProductQuery::default();
        query.installed_only = Some(true);
        Self::list_products(db, data_dir, query)
    }

    pub fn dev_revoke_product_version(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
        version: Option<String>,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        let target = match version {
            Some(v) => Self::version_by_product_version(db, product_id, &v)?,
            None => Self::active_version(db, product_id)?,
        };
        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE product_versions SET status = 'revoked', signature_status = 'revoked'
             WHERE id = ?1",
            [target.id],
        )?;
        drop(conn);
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_dev_revoke",
            Some(&target.version),
        )
        .ok();
        Ok(action_ok(&row, "开发模式：已吊销该商品版本"))
    }

    pub fn dev_restore_product_version(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
        version: Option<String>,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        let target = match version {
            Some(v) => Self::version_by_product_version(db, product_id, &v)?,
            None => Self::latest_version_any_status(db, product_id)?,
        };
        let restored_status = if target.status == "update" {
            "update"
        } else {
            "active"
        };
        let conn = db.conn_lock()?;
        conn.execute(
            "UPDATE product_versions
             SET status = ?1, signature_status = CASE WHEN signature_status = 'revoked' THEN 'unsigned' ELSE signature_status END
             WHERE id = ?2",
            params![restored_status, target.id],
        )?;
        drop(conn);
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_dev_restore",
            Some(&target.version),
        )
        .ok();
        Ok(action_ok(&row, "开发模式：已恢复该商品版本"))
    }

    pub fn mock_test_product(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
    ) -> Result<MarketplaceMockTestResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let row = Self::get_product_row(db, product_id)?;
        let product_type = parse_product_type(&row.product_type);
        let message = match product_type {
            ProductType::XingchenAgent => {
                "MockXingchenProvider 已响应：这里不会访问讯飞星辰，也不会读取真实密钥。"
            }
            ProductType::LocalPlugin | ProductType::DeclarativeUi | ProductType::PromptPack => {
                "Prompt 包为本地静态模板，无网络调用。"
            }
            _ => "安全 Mock MCP 已响应：未执行 Shell/Python/PowerShell，也未访问外网。",
        };
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_service_mock_test",
            Some("mock-only"),
        )
        .ok();
        Ok(MarketplaceMockTestResult {
            ok: true,
            product_id: row.id,
            title: row.name,
            message: message.into(),
            mock: true,
        })
    }

    pub fn configure_service(
        db: &Database,
        data_dir: &Path,
        input: MarketplaceServiceConfigurationInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        let detail = Self::get_product(db, data_dir, &input.product_id)?;
        if detail.summary.revoked {
            return Err(AppError::InvalidInput(
                "商品或当前版本已吊销，不能配置服务".into(),
            ));
        }
        let entitlement = detail
            .entitlement
            .as_ref()
            .ok_or_else(|| AppError::InvalidInput("商品尚未获取或授权已失效".into()))?;
        if !matches!(
            entitlement.status,
            MarketplaceEntitlementStatus::Active | MarketplaceEntitlementStatus::ExternalAuthorized
        ) {
            return Err(AppError::InvalidInput("商品授权已失效".into()));
        }
        let installation = detail
            .installation
            .as_ref()
            .filter(|installation| installation.enabled)
            .ok_or_else(|| AppError::InvalidInput("商品必须先安装并启用".into()))?;
        let mode = detail
            .manifest
            .delivery_mode
            .as_ref()
            .ok_or_else(|| AppError::InvalidInput("该商品不是 AI 服务交付商品".into()))?;
        if matches!(mode, AiServiceDeliveryMode::Byok) {
            return Err(AppError::InvalidInput(
                "BYOK 星辰商品应通过智能体配置创建 ExternalAgent".into(),
            ));
        }
        if !input.network_permission_confirmed {
            db.write_audit_log(
                &detail.summary.plugin_id,
                "marketplace_service_permission_rejected",
                Some("network"),
            )
            .ok();
            return Err(AppError::PluginPermissionDenied {
                plugin_id: Some(detail.summary.plugin_id.clone()),
                required_permission: Some("network.request".into()),
            });
        }
        if !detail
            .manifest
            .permissions
            .iter()
            .any(|permission| permission == "network.request")
        {
            return Err(AppError::InvalidInput(
                "商品未声明 network.request 权限".into(),
            ));
        }
        if matches!(mode, AiServiceDeliveryMode::RemoteMcp)
            && !detail
                .manifest
                .permissions
                .iter()
                .any(|permission| permission == "mcp.connect")
        {
            return Err(AppError::InvalidInput(
                "Remote MCP 商品未声明 mcp.connect 权限".into(),
            ));
        }
        if let Some(credential_id) = input.credential_id.as_deref() {
            let conn = db.conn_lock()?;
            let credential: Option<(String, String)> = conn.query_row(
                "SELECT provider, owner_scope FROM credentials WHERE id = ?1 AND configured = 1 LIMIT 1",
                [credential_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?;
            let Some((provider, owner_scope)) = credential else {
                return Err(AppError::InvalidInput(
                    "所选服务凭据不存在或尚未配置".into(),
                ));
            };
            let expected_provider = match mode {
                AiServiceDeliveryMode::HostedApi => "hosted-api",
                AiServiceDeliveryMode::RemoteMcp => "remote-mcp",
                AiServiceDeliveryMode::Byok => "xingchen",
            };
            if provider != expected_provider || owner_scope != "local-user" {
                return Err(AppError::InvalidInput(format!(
                    "所选凭据不属于当前用户或不适用于 {expected_provider} 交付模式"
                )));
            }
        }
        let config = detail.configuration_schema.as_ref().unwrap_or(&Value::Null);
        let endpoint = match mode {
            AiServiceDeliveryMode::HostedApi => config.pointer("/endpoint/default"),
            AiServiceDeliveryMode::RemoteMcp => config.pointer("/serverUrl/default"),
            AiServiceDeliveryMode::Byok => None,
        }
        .and_then(Value::as_str)
        .unwrap_or_default();
        validate_delivery_endpoint(endpoint)?;
        PluginService::set_settings(
            db,
            &detail.summary.plugin_id,
            json!({
                "deliveryMode": mode,
                "credentialId": input.credential_id,
                "endpoint": endpoint,
                "mockOnly": true,
                "installationId": installation.id,
            }),
        )?;
        if matches!(mode, AiServiceDeliveryMode::RemoteMcp) {
            let registry_name = format!("marketplace:{}", detail.summary.plugin_id);
            let mut env = HashMap::new();
            if let Some(credential_id) = input.credential_id.as_deref() {
                env.insert("credentialId".to_string(), credential_id.to_string());
            }
            let registry_input = McpServerInput {
                name: registry_name.clone(),
                transport: "remote-mcp-mock".into(),
                command: endpoint.into(),
                args: vec!["mock-only".into()],
                env,
                enabled: false,
            };
            if let Some(existing) = db
                .list_mcp_servers()?
                .into_iter()
                .find(|server| server.name == registry_name)
            {
                db.update_mcp_server(existing.id, &registry_input)?;
            } else {
                db.create_mcp_server(&registry_input)?;
            }
        }
        let audit_target = match mode {
            AiServiceDeliveryMode::HostedApi => "hosted-api-mock",
            AiServiceDeliveryMode::RemoteMcp => "remote-mcp-mock",
            AiServiceDeliveryMode::Byok => "byok",
        };
        db.write_audit_log(
            &detail.summary.plugin_id,
            "marketplace_service_configured",
            Some(audit_target),
        )
        .ok();
        Ok(action_ok(
            &Self::get_product_row(db, &input.product_id)?,
            "服务配置已保存；本阶段测试结果为 Mock，不代表真实服务已连通",
        ))
    }

    pub fn list_orders(db: &Database, data_dir: &Path) -> Result<Vec<MarketplaceOrder>, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let user_id = Self::current_user_id(db)?;
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT o.id, COALESCE(o.buyer_user_id, o.local_user_id), COALESCE(o.seller_user_id, p.seller_user_id, p.developer_id),
                    oi.product_id, p.name, oi.product_version_id, oi.version_snapshot,
                    o.currency, COALESCE(o.gross_amount, o.total_amount), o.platform_fee, o.seller_income,
                    o.payment_status, o.settlement_status, o.refund_status, o.is_mock,
                    o.created_at, o.completed_at
             FROM orders o
             JOIN order_items oi ON oi.order_id = o.id
             JOIN products p ON p.id = oi.product_id
             WHERE COALESCE(o.buyer_user_id, o.local_user_id) = ?1
             ORDER BY o.created_at DESC, o.id DESC",
        )?;
        let rows = stmt.query_map([user_id.as_str()], order_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn list_ledger(
        db: &Database,
        data_dir: &Path,
    ) -> Result<Vec<MarketplaceLedgerEntry>, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let user_id = Self::current_user_id(db)?;
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, entry_type, order_id, order_item_id, buyer_user_id, seller_user_id,
                    product_id, amount, currency, is_mock, memo, created_at
             FROM commerce_ledger_entries
             WHERE buyer_user_id = ?1 OR seller_user_id = ?1
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([user_id.as_str()], ledger_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn request_refund(
        db: &Database,
        data_dir: &Path,
        input: MarketplaceRefundInput,
    ) -> Result<MarketplaceActionResult, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let user_id = Self::current_user_id(db)?;
        let conn = db.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        let (order_id, product_id, product_name, plugin_id, seller_user_id, order_item_id, gross, seller_income, currency, refund_status): (i64, String, String, String, String, i64, i64, i64, String, String) = tx.query_row(
            "SELECT o.id, oi.product_id, p.name, COALESCE(p.plugin_id, p.id),
                    COALESCE(o.seller_user_id, oi.seller_user_id, p.seller_user_id, p.developer_id),
                    oi.id, COALESCE(o.gross_amount, o.total_amount), o.seller_income, o.currency, o.refund_status
             FROM orders o
             JOIN order_items oi ON oi.order_id = o.id
             JOIN products p ON p.id = oi.product_id
             WHERE o.id = ?1 AND COALESCE(o.buyer_user_id, o.local_user_id) = ?2",
            params![input.order_id, user_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
        ).optional()?.ok_or_else(|| AppError::PluginPermissionDenied {
            plugin_id: None,
            required_permission: Some("order.owner".into()),
        })?;
        if refund_status == "refund_success" {
            return Err(AppError::InvalidInput("该订单已退款".into()));
        }
        tx.execute(
            "UPDATE orders SET refund_status = 'refund_success', settlement_status = 'reversed'
             WHERE id = ?1",
            [order_id],
        )?;
        tx.execute(
            "UPDATE entitlements
             SET status = 'revoked', revoked_at = datetime('now','localtime'), revoked_reason = 'refund_success'
             WHERE order_id = ?1 AND owner_user_id = ?2",
            params![order_id, user_id],
        )?;
        if gross > 0 {
            insert_ledger_tx(
                &tx,
                "refund",
                Some(order_id),
                Some(order_item_id),
                Some(&user_id),
                Some(&seller_user_id),
                Some(&product_id),
                -gross,
                &currency,
                input.reason.as_deref().unwrap_or("模拟退款成功"),
            )?;
            insert_ledger_tx(
                &tx,
                "seller_income_reversal",
                Some(order_id),
                Some(order_item_id),
                Some(&user_id),
                Some(&seller_user_id),
                Some(&product_id),
                -seller_income,
                &currency,
                "模拟创作者收入冲正",
            )?;
        }
        tx.commit()?;
        drop(conn);
        db.write_audit_log(
            &plugin_id,
            "marketplace_refund_success",
            Some(&format!("order:{}", order_id)),
        )
        .ok();
        Ok(MarketplaceActionResult {
            ok: true,
            product_id,
            plugin_id: Some(plugin_id),
            message: format!("{} 已模拟退款成功，授权已撤销", product_name),
            requires_permission_confirmation: false,
            permission_diff: None,
            entitlement: None,
            installation: None,
        })
    }

    pub fn list_reviews(
        db: &Database,
        data_dir: &Path,
        product_id: &str,
    ) -> Result<Vec<MarketplaceReviewInfo>, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT r.id, r.order_id, r.product_id, r.buyer_user_id, COALESCE(u.nickname, u.display_name, r.buyer_user_id),
                    r.seller_user_id, r.rating, r.content, r.status,
                    CASE WHEN e.id IS NOT NULL THEN 1 ELSE 0 END,
                    CASE WHEN o.refund_status = 'refund_success' THEN 1 ELSE 0 END,
                    r.created_at
             FROM product_reviews r
             JOIN orders o ON o.id = r.order_id
             LEFT JOIN entitlements e ON e.order_id = r.order_id AND e.product_id = r.product_id
             LEFT JOIN users u ON u.id = r.buyer_user_id
             WHERE r.product_id = ?1 AND r.status != 'hidden'
             ORDER BY r.created_at DESC",
        )?;
        let rows = stmt.query_map([product_id], review_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn submit_review(
        db: &Database,
        data_dir: &Path,
        input: MarketplaceReviewInput,
    ) -> Result<MarketplaceReviewInfo, AppError> {
        Self::ensure_seed_data(db, data_dir)?;
        let user_id = Self::current_user_id(db)?;
        if !(1..=5).contains(&input.rating) {
            return Err(AppError::InvalidInput("评分必须在 1 到 5 之间".into()));
        }
        if input.content.trim().is_empty() {
            return Err(AppError::InvalidInput("评价内容不能为空".into()));
        }
        let row = Self::get_product_row(db, &input.product_id)?;
        if user_id == row.seller_user_id {
            return Err(AppError::InvalidInput("卖家不能评价自己的商品".into()));
        }
        let conn = db.conn_lock()?;
        let valid_order: Option<i64> = conn
            .query_row(
                "SELECT o.id
             FROM orders o
             JOIN entitlements e ON e.order_id = o.id AND e.product_id = ?2
             WHERE o.id = ?1
               AND COALESCE(o.buyer_user_id, o.local_user_id) = ?3
               AND o.payment_status = 'paid'",
                params![input.order_id, input.product_id, user_id],
                |row| row.get(0),
            )
            .optional()?;
        if valid_order.is_none() {
            return Err(AppError::PluginPermissionDenied {
                plugin_id: Some(input.product_id.clone()),
                required_permission: Some("verified_purchase.review".into()),
            });
        }
        conn.execute(
            "INSERT INTO product_reviews
                (order_id, product_id, buyer_user_id, seller_user_id, rating, content)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                input.order_id,
                input.product_id,
                user_id,
                row.seller_user_id,
                input.rating,
                input.content.trim()
            ],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        db.write_audit_log(
            &row.plugin_id,
            "marketplace_review_submitted",
            Some(&format!("review:{}", id)),
        )
        .ok();
        Self::list_reviews(db, data_dir, &input.product_id)?
            .into_iter()
            .find(|review| review.id == id)
            .ok_or_else(|| AppError::NotFound("评价写入后未找到".into()))
    }

    fn ensure_seed_data(db: &Database, data_dir: &Path) -> Result<(), AppError> {
        fs::create_dir_all(packages_root(data_dir)?)?;
        for seed in seed_products() {
            for version in &seed.versions {
                let package_dir = packages_root(data_dir)?.join(seed.id).join(version.version);
                write_seed_package(&seed, version, &package_dir)?;
                let hash = PluginService::calculate_integrity_for_path(&package_dir)?;
                upsert_seed(db, &seed, version, &package_dir, &hash)?;
            }
        }
        Ok(())
    }

    fn product_rows(db: &Database) -> Result<Vec<ProductRow>, AppError> {
        let conn = db.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT p.id, COALESCE(p.plugin_id, p.id), p.developer_id, p.developer_name,
                    COALESCE(p.seller_user_id, p.developer_id), u.nickname, p.name,
                    COALESCE(description, ''), icon, product_type, status, license_type,
                    byok_required, mock_mode, data_destination, file_upload_notice, risk_notes_json
             FROM products p
             LEFT JOIN users u ON u.id = COALESCE(p.seller_user_id, p.developer_id)
             WHERE p.mock_mode = 1
               AND p.status IN ('active', 'approved', 'published')
             ORDER BY p.name ASC",
        )?;
        let rows = stmt
            .query_map([], product_row_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)?;
        Ok(rows)
    }

    fn get_product_row(db: &Database, product_id: &str) -> Result<ProductRow, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT p.id, COALESCE(p.plugin_id, p.id), p.developer_id, p.developer_name,
                    COALESCE(p.seller_user_id, p.developer_id), u.nickname, p.name,
                    COALESCE(description, ''), icon, product_type, status, license_type,
                    byok_required, mock_mode, data_destination, file_upload_notice, risk_notes_json
             FROM products p
             LEFT JOIN users u ON u.id = COALESCE(p.seller_user_id, p.developer_id)
             WHERE p.id = ?1",
            [product_id],
            product_row_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("商品不存在: {}", product_id)))
    }

    fn active_version(db: &Database, product_id: &str) -> Result<VersionRow, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, product_id, version, manifest_json, runtime_kind, source,
                    content_hash, signature_status, min_app_version, status, changelog, package_path
             FROM product_versions
             WHERE product_id = ?1
               AND status IN ('active', 'approved', 'published')
               AND signature_status != 'revoked'
             ORDER BY id DESC
             LIMIT 1",
            [product_id],
            version_row_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("商品没有可安装版本: {}", product_id)))
    }

    fn display_version(db: &Database, product_id: &str) -> Result<VersionRow, AppError> {
        Self::active_version(db, product_id)
            .or_else(|_| Self::latest_version_any_status(db, product_id))
    }

    fn latest_version_any_status(db: &Database, product_id: &str) -> Result<VersionRow, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, product_id, version, manifest_json, runtime_kind, source,
                    content_hash, signature_status, min_app_version, status, changelog, package_path
             FROM product_versions
             WHERE product_id = ?1
             ORDER BY id DESC
             LIMIT 1",
            [product_id],
            version_row_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("商品版本不存在: {}", product_id)))
    }

    fn update_version(db: &Database, product_id: &str) -> Result<Option<VersionRow>, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, product_id, version, manifest_json, runtime_kind, source,
                    content_hash, signature_status, min_app_version, status, changelog, package_path
             FROM product_versions
             WHERE product_id = ?1
               AND status IN ('update', 'approved', 'published')
               AND signature_status != 'revoked'
             ORDER BY id DESC
             LIMIT 1",
            [product_id],
            version_row_from_row,
        )
        .optional()
        .map_err(AppError::from)
    }

    fn version_by_id(db: &Database, id: i64) -> Result<VersionRow, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, product_id, version, manifest_json, runtime_kind, source,
                    content_hash, signature_status, min_app_version, status, changelog, package_path
             FROM product_versions
             WHERE id = ?1",
            [id],
            version_row_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("商品版本不存在: {}", id)))
    }

    fn version_by_product_version(
        db: &Database,
        product_id: &str,
        version: &str,
    ) -> Result<VersionRow, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, product_id, version, manifest_json, runtime_kind, source,
                    content_hash, signature_status, min_app_version, status, changelog, package_path
             FROM product_versions
             WHERE product_id = ?1 AND version = ?2",
            params![product_id, version],
            version_row_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("商品版本不存在: {}@{}", product_id, version)))
    }

    fn summary_for_product(
        db: &Database,
        row: &ProductRow,
    ) -> Result<MarketplaceProductSummary, AppError> {
        let version = Self::display_version(db, &row.id)?;
        let manifest = manifest_from_row(&version)?;
        let price = Self::price_for_product(db, &row.id)?;
        let entitlement = Self::entitlement(db, &row.id)?;
        let installation = db.get_plugin_installation(&row.plugin_id)?;
        let update = if installation.is_some() {
            Self::update_version(db, &row.id)?
        } else {
            None
        };
        let policy_note = if manifest.runtime_kind == PluginRuntimeKind::LegacyJs
            && manifest.source == PluginSource::Marketplace
        {
            Some("公开市场 legacy-js 已被安全策略阻止".into())
        } else {
            None
        };
        let mut risk_notes = parse_json_vec(&row.risk_notes_json);
        if version.signature_status == "unsigned" {
            risk_notes.push("演示未签名：本地模拟市场允许安装，但真实市场应要求平台签名".into());
        }
        if let Some(note) = policy_note {
            risk_notes.push(note);
        }
        Ok(MarketplaceProductSummary {
            id: row.id.clone(),
            plugin_id: row.plugin_id.clone(),
            name: row.name.clone(),
            developer_id: row.developer_id.clone(),
            developer_name: row.developer_name.clone(),
            seller_user_id: Some(row.seller_user_id.clone()),
            seller_nickname: row.seller_nickname.clone(),
            description: row.description.clone(),
            icon: row.icon.clone(),
            current_version: version.version,
            product_type: parse_product_type(&row.product_type),
            runtime_kind: parse_runtime_kind(&version.runtime_kind),
            status: parse_product_status(&row.status),
            signature_status: parse_signature(&version.signature_status),
            source: parse_source(&version.source),
            min_app_version: version.min_app_version,
            price,
            byok_required: row.byok_required,
            delivery_mode: manifest.delivery_mode.clone(),
            protocol: manifest.protocol.clone(),
            permissions: manifest.permissions.clone(),
            permission_summary: manifest.permissions.iter().take(3).cloned().collect(),
            acquired: entitlement.is_some(),
            installed: installation.is_some(),
            enabled: installation.as_ref().map(|i| i.enabled).unwrap_or(false),
            installed_version: installation.map(|i| i.installed_version),
            has_update: update.is_some(),
            update_version: update.map(|v| v.version),
            revoked: row.status == "revoked" || version.status == "revoked",
            risk_notes,
            mock_mode: row.mock_mode,
            self_owned: Self::current_user_id(db)
                .map(|id| id == row.seller_user_id)
                .unwrap_or(false),
        })
    }

    fn price_for_product(db: &Database, product_id: &str) -> Result<MarketplacePrice, AppError> {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT currency, amount, price_type, is_mock
             FROM prices
             WHERE product_id = ?1
             ORDER BY id DESC
             LIMIT 1",
            [product_id],
            |row| {
                let price_type: String = row.get(2)?;
                Ok(MarketplacePrice {
                    currency: row.get(0)?,
                    amount: row.get(1)?,
                    price_type: parse_license(&price_type),
                    is_mock: row.get::<_, i32>(3)? != 0,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("商品价格不存在: {}", product_id)))
    }

    fn entitlement(
        db: &Database,
        product_id: &str,
    ) -> Result<Option<MarketplaceEntitlement>, AppError> {
        let user_id = Self::current_user_id(db)?;
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT id, product_id, entitlement_type, status, issued_at, expires_at, owner_user_id, order_id
             FROM entitlements
             WHERE COALESCE(owner_user_id, local_user_id) = ?1 AND product_id = ?2
             ORDER BY id DESC
             LIMIT 1",
            params![user_id, product_id],
            entitlement_from_row,
        )
        .optional()
        .map_err(AppError::from)
    }

    fn ensure_entitled(db: &Database, product_id: &str) -> Result<(), AppError> {
        let ent = Self::entitlement(db, product_id)?
            .ok_or_else(|| AppError::InvalidInput("请先获取本地商品或绑定外部授权".into()))?;
        if !matches!(
            ent.status,
            MarketplaceEntitlementStatus::Active | MarketplaceEntitlementStatus::ExternalAuthorized
        ) {
            return Err(AppError::InvalidInput(
                "许可证不可用，禁止安装或更新".into(),
            ));
        }
        if let Some(expires) = &ent.expires_at {
            let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            if expires <= &now {
                return Err(AppError::InvalidInput("订阅许可证已过期".into()));
            }
        }
        Ok(())
    }

    fn copy_and_register(
        db: &Database,
        data_dir: &Path,
        row: &ProductRow,
        version: &VersionRow,
        source: &Path,
        update: bool,
    ) -> Result<PluginInstallationInfo, AppError> {
        let plugins = plugins_dir(data_dir)?;
        let dest = plugins.join(&row.plugin_id);
        let temp = plugins.join(format!(".installing-{}-{}", row.plugin_id, version.version));
        let backup_root = data_dir
            .join(MARKETPLACE_DIR)
            .join(BACKUPS_DIR)
            .join(&row.plugin_id);
        fs::create_dir_all(&backup_root)?;
        if temp.exists() {
            fs::remove_dir_all(&temp)?;
        }
        copy_dir(source, &temp)?;
        let actual = PluginService::calculate_integrity_for_path(&temp)?;
        if actual != version.content_hash {
            fs::remove_dir_all(&temp).ok();
            return Err(AppError::InvalidInput("插件包完整性校验失败".into()));
        }
        let backup = if dest.exists() {
            let backup =
                backup_root.join(format!("{}-{}", Local::now().timestamp(), version.version));
            if backup.exists() {
                fs::remove_dir_all(&backup)?;
            }
            fs::rename(&dest, &backup)?;
            Some(backup)
        } else {
            None
        };
        if let Err(e) = fs::rename(&temp, &dest) {
            if let Some(backup) = &backup {
                fs::rename(backup, &dest).ok();
            }
            fs::remove_dir_all(&temp).ok();
            return Err(AppError::from(e));
        }
        let result: Result<PluginInfo, AppError> = (|| {
            let installed_manifest = PluginService::parse_manifest(&dest)?;
            if installed_manifest.id != row.plugin_id {
                return Err(AppError::InvalidInput(
                    "安装包 manifest id 与商品不一致".into(),
                ));
            }
            db.upsert_plugin(
                &installed_manifest,
                &dest.to_string_lossy(),
                &version.content_hash,
            )?;
            {
                let conn = db.conn_lock()?;
                conn.execute(
                    "UPDATE plugin_installations
                     SET product_id = ?1, product_version_id = ?2, installed_version = ?3,
                         source = 'marketplace', content_hash = ?4, status = 'installed',
                         previous_install_path = ?5, updated_at = datetime('now','localtime')
                     WHERE plugin_id = ?6",
                    params![
                        row.id,
                        version.id,
                        version.version,
                        version.content_hash,
                        backup.as_ref().map(|p| p.to_string_lossy().to_string()),
                        row.plugin_id
                    ],
                )?;
            }
            db.grant_plugin_permissions(&row.plugin_id, &installed_manifest.permissions)?;
            db.get_plugin(&row.plugin_id)
        })();
        match result {
            Ok(_) => db
                .get_plugin_installation(&row.plugin_id)?
                .ok_or_else(|| AppError::Custom("安装记录写入失败".into())),
            Err(e) => {
                if dest.exists() {
                    fs::remove_dir_all(&dest).ok();
                }
                if let Some(backup) = backup {
                    fs::rename(backup, &dest).ok();
                }
                if update {
                    db.write_audit_log(
                        &row.plugin_id,
                        "marketplace_update_rollback",
                        Some(&e.to_string()),
                    )
                    .ok();
                }
                Err(e)
            }
        }
    }

    fn install_prompt_pack(db: &Database, product_id: &str, version: &str) -> Result<(), AppError> {
        let templates = prompt_templates_for_product(product_id, version);
        let conn = db.conn_lock()?;
        let tx = conn.unchecked_transaction()?;
        for tpl in templates {
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT template_id FROM marketplace_prompt_templates
                     WHERE product_id = ?1 AND template_key = ?2",
                    params![product_id, tpl.0],
                    |row| row.get(0),
                )
                .optional()?;
            let hash = sha256_text(tpl.2);
            match existing {
                Some(id) => {
                    tx.execute(
                        "UPDATE prompt_templates
                         SET title = ?1, description = ?2, prompt = ?3, output_mode = 'popup',
                             icon = 'Sparkles', enabled = 1, updated_at = datetime('now','localtime')
                         WHERE id = ?4",
                        params![tpl.1, tpl.3, tpl.2, id],
                    )?;
                    tx.execute(
                        "UPDATE marketplace_prompt_templates SET content_hash = ?1 WHERE product_id = ?2 AND template_id = ?3",
                        params![hash, product_id, id],
                    )?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO prompt_templates
                            (title, description, prompt, output_mode, icon, is_builtin, builtin_code, sort_order, enabled)
                         VALUES (?1, ?2, ?3, 'popup', 'Sparkles', 0, NULL,
                            (SELECT COALESCE(MAX(sort_order), 100) + 10 FROM prompt_templates), 1)",
                        params![tpl.1, tpl.3, tpl.2],
                    )?;
                    let id = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO marketplace_prompt_templates
                            (product_id, template_id, template_key, content_hash)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![product_id, id, tpl.0, hash],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn uninstall_prompt_pack(db: &Database, product_id: &str) -> Result<(), AppError> {
        let conn = db.conn_lock()?;
        let rows = {
            let mut stmt = conn.prepare(
                "SELECT template_id, content_hash
                 FROM marketplace_prompt_templates
                 WHERE product_id = ?1",
            )?;
            let rows = stmt
                .query_map([product_id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (id, original_hash) in rows {
            let current: Option<String> = conn
                .query_row(
                    "SELECT prompt FROM prompt_templates WHERE id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()?;
            if current.as_deref().map(sha256_text) == Some(original_hash) {
                conn.execute("DELETE FROM prompt_templates WHERE id = ?1", [id])?;
            }
        }
        conn.execute(
            "DELETE FROM marketplace_prompt_templates WHERE product_id = ?1",
            [product_id],
        )?;
        Ok(())
    }

    fn matches_query(summary: &MarketplaceProductSummary, query: &MarketplaceProductQuery) -> bool {
        if let Some(keyword) = &query.keyword {
            let keyword = keyword.trim().to_lowercase();
            if !keyword.is_empty()
                && !summary.name.to_lowercase().contains(&keyword)
                && !summary.description.to_lowercase().contains(&keyword)
                && !summary.developer_name.to_lowercase().contains(&keyword)
            {
                return false;
            }
        }
        if let Some(t) = &query.product_type {
            if &summary.product_type != t {
                return false;
            }
        }
        if let Some(r) = &query.runtime_kind {
            if &summary.runtime_kind != r {
                return false;
            }
        }
        if query.free_only == Some(true) && summary.price.amount != 0 {
            return false;
        }
        if query.acquired_only == Some(true) && !summary.acquired {
            return false;
        }
        if query.installed_only == Some(true) && !summary.installed {
            return false;
        }
        if query.byok_only == Some(true) && !summary.byok_required {
            return false;
        }
        if let Some(status) = &query.status {
            if &summary.status != status {
                return false;
            }
        }
        true
    }
}

fn validate_delivery_endpoint(endpoint: &str) -> Result<(), AppError> {
    if endpoint.starts_with("mock://") {
        return Ok(());
    }
    let url = Url::parse(endpoint)
        .map_err(|_| AppError::InvalidInput("服务 Endpoint 不是合法 URL".into()))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let local_dev = cfg!(debug_assertions)
        && url.scheme() == "http"
        && matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1");
    if url.scheme() != "https" && !local_dev {
        return Err(AppError::InvalidInput(
            "服务 Endpoint 必须使用 HTTPS；localhost 仅限开发构建".into(),
        ));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(AppError::InvalidInput(
            "服务 Endpoint 不得包含鉴权信息".into(),
        ));
    }
    Ok(())
}

fn seed_products() -> Vec<SeedProduct> {
    vec![
        SeedProduct {
            id: "official-study-summary-prompts",
            plugin_id: "official-study-summary-prompts",
            name: "学习总结Prompt包",
            developer_id: "firstwork-official",
            developer_name: "firstwork 官方演示",
            description: "本地 Prompt 模板包，安装后导入 3 个学习总结模板。",
            full_description: "学习总结Prompt包会把模板导入现有提示词库，不访问网络，也不创建第二套模板系统。",
            icon: "Sparkles",
            product_type: ProductType::PromptPack,
            runtime_kind: PluginRuntimeKind::PromptPack,
            license_type: MarketplaceLicenseType::Free,
            amount: 0,
            byok_required: false,
            data_destination: "仅保存到本机 prompt_templates 表。",
            file_upload_notice: "不上传文件或图片。",
            risk_notes: vec!["卸载时仅清理由该商品导入且未被用户修改的模板。"],
            credential_requirements: vec![],
            versions: vec![
                SeedVersion {
                    version: "1.0.0",
                    status: "active",
                    changelog: "初始版本：学习总结、错题复盘、周计划三个模板。",
                    permissions: vec!["prompts.register"],
                    configuration_schema: json!({"sections":[{"title":"Prompt模板","description":"安装后自动导入到提示词库。"}]}),
                },
                SeedVersion {
                    version: "1.1.0",
                    status: "update",
                    changelog: "新增声明式模板说明入口，需要 views.register 权限。",
                    permissions: vec!["prompts.register", "views.register"],
                    configuration_schema: json!({"sections":[{"title":"Prompt模板","description":"新增一个只读说明面板，用于演示更新权限差异。"}]}),
                },
            ],
        },
        SeedProduct {
            id: "official-ai-document-summary-plugin",
            plugin_id: "official-ai-document-summary-plugin",
            name: "AI 文档摘要插件",
            developer_id: "firstwork-official",
            developer_name: "firstwork 官方演示",
            description: "声明式文档工具栏插件，启用后在文档编辑器显示 AI 摘要按钮。",
            full_description: "AI 文档摘要插件用于验证普通插件的完整生命周期。它不会执行第三方 JavaScript，不访问真实 AI 服务；点击按钮后读取当前文档标题和正文，由 firstwork 受控后端返回 Mock 摘要预览。",
            icon: "FileText",
            product_type: ProductType::LocalPlugin,
            runtime_kind: PluginRuntimeKind::DeclarativeUi,
            license_type: MarketplaceLicenseType::Free,
            amount: 0,
            byok_required: false,
            data_destination: "仅在本机当前窗口内传递给 firstwork 受控 Mock 摘要命令，不发送到第三方服务。",
            file_upload_notice: "不上传文件或图片；只读取用户当前打开的文档标题和正文。",
            risk_notes: vec![
                "Mock 摘要不会调用真实 AI，不代表真实模型能力。",
                "插件只能通过受控后端命令读取当前文档，不能使用 raw invoke、new Function 或 unsafe-eval。",
            ],
            credential_requirements: vec![],
            versions: vec![
                SeedVersion {
                    version: "1.0.0",
                    status: "active",
                    changelog: "初始版本：注册文档编辑器 AI 摘要按钮，返回 Mock 摘要预览。",
                    permissions: vec!["document.read", "document.write", "ui.editor.toolbar", "ai.invoke"],
                    configuration_schema: json!({
                        "sections": [
                            {
                                "title": "AI 摘要",
                                "description": "启用后在文档编辑器工具栏显示 AI 摘要按钮。当前版本只返回 Mock 摘要。"
                            }
                        ]
                    }),
                },
                SeedVersion {
                    version: "1.1.0",
                    status: "update",
                    changelog: "演示更新版本：优化摘要预览文案和权限差异展示，不新增权限。",
                    permissions: vec!["document.read", "document.write", "ui.editor.toolbar", "ai.invoke"],
                    configuration_schema: json!({
                        "sections": [
                            {
                                "title": "AI 摘要",
                                "description": "更新后继续使用 Mock Provider，并保留相同最小权限。"
                            }
                        ]
                    }),
                },
            ],
        },
        SeedProduct {
            id: "official-planning-with-files",
            plugin_id: "official-planning-with-files",
            name: "Planning with Files",
            developer_id: "firstwork-official",
            developer_name: "firstwork 官方",
            description: "按会话工作的规划文件插件，让 AI 对话携带计划、发现和进度继续协作。",
            full_description: "Planning with Files 是 firstwork 官方声明式插件。启用后可在普通 AI 对话和外部智能体会话中按会话创建 plan.md、findings.md、progress.md，调用前注入精简规划上下文，调用后预览并由用户确认规划更新。插件不执行第三方 JavaScript，不读取凭据，不访问任意文件系统或网络。",
            icon: "ListChecks",
            product_type: ProductType::LocalPlugin,
            runtime_kind: PluginRuntimeKind::DeclarativeUi,
            license_type: MarketplaceLicenseType::Free,
            amount: 0,
            byok_required: false,
            data_destination: "规划文件仅保存到当前 firstwork 数据目录下的 planning-workspaces，不默认写入用户项目或 Git 仓库。",
            file_upload_notice: "不上传文件或图片；导出规划文件必须由用户主动选择目录。",
            risk_notes: vec![
                "默认关闭，按会话启用；关闭后不再注入上下文，也不会删除已有规划文件。",
                "规划文件写入前会进行敏感信息脱敏，不保存 API Key、API Secret、Token 或 Authorization。",
            ],
            credential_requirements: vec![],
            versions: vec![SeedVersion {
                version: "1.0.0",
                status: "active",
                changelog: "初始版本：会话级规划开关、三文件工作区、上下文注入、结构化更新预览和人工确认保存。",
                permissions: vec![
                    "ai.context.read",
                    "ai.context.augment",
                    "ai.session.read",
                    "planning.files.read",
                    "planning.files.write",
                    "ui.chat.toolbar",
                    "ui.chat.panel",
                ],
                configuration_schema: json!({
                    "sections": [
                        {
                            "title": "Planning with Files",
                            "description": "在 AI 对话中按会话维护 plan.md、findings.md 和 progress.md。默认每次确认后应用 AI 提出的规划更新。"
                        }
                    ],
                    "workspace": "planning-workspaces/{sessionId}",
                    "autoApplyDefault": false
                }),
            }],
        },
        SeedProduct {
            id: "official-xingchen-learning-demo",
            plugin_id: "official-xingchen-learning-demo",
            name: "星辰学习助手连接器（演示）",
            developer_id: "firstwork-official",
            developer_name: "firstwork 官方演示",
            description: "BYOK 模式的讯飞星辰智能体连接器示例，当前只返回 Mock 结果。",
            full_description: "该商品演示如何把讯飞星辰 Workflow API 调用项发布到 Pomegranate：工作流本体、调试、发布和授权仍在星辰平台完成；Pomegranate 只保存展示信息、Flow ID 配置结构和 credentialId 引用。",
            icon: "Bot",
            product_type: ProductType::XingchenAgent,
            runtime_kind: PluginRuntimeKind::XingchenAgent,
            license_type: MarketplaceLicenseType::OneTime,
            amount: 990,
            byok_required: true,
            data_destination: "未来会发送到用户配置的第三方讯飞星辰智能体；本轮为 Mock。",
            file_upload_notice: "未来文件/图片需用户主动选择并确认；本轮不上传。",
            risk_notes: vec![
                "外部参考价仅用于展示；Pomegranate 不处理真实支付、余额、提现或分账。",
                "BYOK：插件不得读取凭据明文，只能请求后端使用 credentialId。",
            ],
            credential_requirements: vec![PluginCredentialRequirement {
                id: "xingchen-byok".into(),
                label: Some("讯飞星辰 BYOK 凭据".into()),
                provider: Some("xingchen".into()),
                fields: vec!["appId".into(), "apiKey".into(), "apiSecret".into(), "token".into()],
                required: true,
            }],
            versions: vec![SeedVersion {
                version: "1.0.0",
                status: "active",
                changelog: "初始演示版本：声明式配置页 + Mock 测试按钮。",
                permissions: vec!["credentials.use", "agents.invoke", "network.xingchen", "ai.invoke"],
                configuration_schema: json!({
                    "endpoint": {"type":"string","default":"https://xingchen-api.xf-yun.com/workflow/v1/chat/completions","readOnly":true},
                    "credentialId": {"type":"credential-reference","provider":"xingchen","required":true},
                    "flowId": {"type":"string","required":true,"secret":false},
                    "inputParameter": {"type":"string","default":"AGENT_USER_INPUT"},
                    "responseTextField": {"type":"string","default":"answer"},
                    "externalUrl": {"type":"string","default":"https://xingchen.xfyun.cn/"},
                    "flowIdProvidedByUser": true,
                    "fields": [
                        {"key":"credentialId","label":"讯飞 Workflow 凭据","type":"credential","provider":"xingchen"},
                        {"key":"flowId","label":"Flow ID","type":"text","required":true}
                    ],
                    "mockAction":"xingchen-agent-test"
                }),
            }],
        },
        SeedProduct {
            id: "official-hosted-ai-api-demo",
            plugin_id: "official-hosted-ai-api-demo",
            name: "开发者托管 AI API（演示）",
            developer_id: "firstwork-official",
            developer_name: "firstwork 官方演示",
            description: "Hosted API 交付模式示例，仅验证安全配置与 Mock Provider。",
            full_description: "该商品演示开发者托管 HTTPS API 的交付结构。开发者的上游星辰密钥只应存在于开发者服务端；本地演示不会访问外网，也不代表真实托管服务已经上线。",
            icon: "Cloud",
            product_type: ProductType::XingchenAgent,
            runtime_kind: PluginRuntimeKind::XingchenAgent,
            license_type: MarketplaceLicenseType::Free,
            amount: 0,
            byok_required: false,
            data_destination: "演示数据去向为开发者托管服务；本阶段仅 Mock，不发送数据。",
            file_upload_notice: "本阶段仅支持 Mock 文本，不上传文件或图片。",
            risk_notes: vec!["Hosted API 只允许 HTTPS；开发者的星辰 Key/Secret 不得进入客户端或商品包。"],
            credential_requirements: vec![PluginCredentialRequirement {
                id: "hosted-service-token".into(),
                label: Some("托管服务访问 Token（可选）".into()),
                provider: Some("hosted-api".into()),
                fields: vec!["bearerToken".into()],
                required: false,
            }],
            versions: vec![SeedVersion {
                version: "1.0.0",
                status: "active",
                changelog: "初始演示版本：Hosted API 安全配置 + 明确 Mock 测试。",
                permissions: vec!["credentials.use", "agents.invoke", "network.request", "ai.invoke"],
                configuration_schema: json!({
                    "endpoint":{"type":"string","default":"mock://hosted-api"},
                    "requestMethod":"POST",
                    "requestBodySchema":"{\"input\":\"string\"}",
                    "responseTextField":"data.text",
                    "streaming":true,
                    "authenticationType":"bearer",
                    "externalUrl":{"type":"string","default":""},
                    "credentialId":{"type":"credential-reference","provider":"hosted-api","required":false},
                    "mockOnly":true
                }),
            }],
        },
        SeedProduct {
            id: "official-local-knowledge-mcp-demo",
            plugin_id: "official-local-knowledge-mcp-demo",
            name: "本地知识工具MCP（演示）",
            developer_id: "firstwork-official",
            developer_name: "firstwork 官方演示",
            description: "安全 Mock MCP 连接器，不执行系统命令、不访问外网。",
            full_description: "该示例展示 MCP 商品如何声明工具说明和权限。本轮仅使用 firstwork 内置 Mock，不启动外部进程。",
            icon: "Plug",
            product_type: ProductType::McpConnector,
            runtime_kind: PluginRuntimeKind::McpConnector,
            license_type: MarketplaceLicenseType::Free,
            amount: 0,
            byok_required: false,
            data_destination: "仅本地 Mock，不发送到第三方服务。",
            file_upload_notice: "不读取用户文件，除非未来用户显式选择。",
            risk_notes: vec!["MCP Mock 不执行 PowerShell、Shell、Python 或任意系统命令。"],
            credential_requirements: vec![],
            versions: vec![SeedVersion {
                version: "1.0.0",
                status: "active",
                changelog: "初始演示版本：声明式工具说明 + Mock 调用结果。",
                permissions: vec!["mcp.connect", "network.request"],
                configuration_schema: json!({
                    "serverUrl":{"type":"string","default":"mock://remote-mcp"},
                    "transport":"streamable-http",
                    "authenticationType":"none",
                    "capabilities":["tools"],
                    "timeoutMs":30000,
                    "externalUrl":{"type":"string","default":""},
                    "credentialId":{"type":"credential-reference","provider":"remote-mcp","required":false},
                    "mockOnly":true,
                    "tools":[
                        {"name":"mock_search","description":"返回固定演示搜索结果"},
                        {"name":"mock_summarize","description":"返回固定演示摘要"}
                    ],
                    "mockAction":"mcp-test"
                }),
            }],
        },
    ]
}

fn write_seed_package(
    seed: &SeedProduct,
    version: &SeedVersion,
    dir: &Path,
) -> Result<(), AppError> {
    fs::create_dir_all(dir)?;
    let (delivery_mode, protocol) = match seed.id {
        "official-xingchen-learning-demo" => (Some("byok"), Some("xingchen-workflow-v1")),
        "official-hosted-ai-api-demo" => (Some("hosted-api"), Some("hosted-api")),
        "official-local-knowledge-mcp-demo" => (Some("remote-mcp"), Some("streamable-http")),
        _ => (None, None),
    };
    let manifest = json!({
        "schemaVersion": 2,
        "id": seed.plugin_id,
        "name": seed.name,
        "version": version.version,
        "authorId": seed.developer_id,
        "description": seed.description,
        "icon": seed.icon,
        "minAppVersion": "1.8.0",
        "productType": seed.product_type,
        "runtimeKind": seed.runtime_kind,
        "source": "marketplace",
        "deliveryMode": delivery_mode,
        "protocol": protocol,
        "permissions": version.permissions,
        "credentialRequirements": seed.credential_requirements,
        "configurationSchema": version.configuration_schema,
        "contributes": contributes_for_seed(seed),
        "integrity": {"sha256": null},
        "signature": {"status": "unsigned", "signer": null}
    });
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    fs::write(
        dir.join("README.md"),
        format!(
            "# {}\n\n{}\n\n本包来自 firstwork 本地模拟市场，不包含真实密钥或真实支付信息。\n",
            seed.name, seed.full_description
        ),
    )?;
    fs::write(
        dir.join("CHANGELOG.md"),
        format!("# {}\n\n{}\n", version.version, version.changelog),
    )?;
    Ok(())
}

fn contributes_for_seed(seed: &SeedProduct) -> serde_json::Value {
    if seed.id == "official-ai-document-summary-plugin" {
        return json!({
            "editorToolbar": [
                {
                    "id": "ai-document-summary",
                    "label": "AI 摘要",
                    "tooltip": "生成当前文档的 Mock 摘要预览",
                    "icon": "Sparkles",
                    "action": "mock-document-summary"
                }
            ]
        });
    }
    match seed.runtime_kind {
        PluginRuntimeKind::PromptPack => json!({
            "prompts": [
                {"id":"study-summary","title":"学习总结"},
                {"id":"mistake-review","title":"错题复盘"},
                {"id":"weekly-plan","title":"学习周计划"}
            ]
        }),
        PluginRuntimeKind::XingchenAgent => json!({
            "views": [{"id":"xingchen-demo-config","title":"星辰学习助手配置"}],
            "aiProviders": [{"id":"mock-xingchen","label":"Mock Xingchen Provider","providerType":"xingchen"}]
        }),
        PluginRuntimeKind::McpConnector => json!({
            "views": [{"id":"mock-mcp-tools","title":"本地知识工具说明"}],
            "mcpServers": [{"id":"mock-local-knowledge","label":"Mock Local Knowledge MCP","transport":"mock"}]
        }),
        _ => json!({}),
    }
}

fn upsert_seed(
    db: &Database,
    seed: &SeedProduct,
    version: &SeedVersion,
    package_dir: &Path,
    hash: &str,
) -> Result<(), AppError> {
    let manifest_json = fs::read_to_string(package_dir.join("manifest.json"))?;
    let conn = db.conn_lock()?;
    conn.execute(
        "INSERT INTO products
            (id, plugin_id, developer_id, seller_user_id, developer_name, name, description, icon, product_type,
             status, license_type, byok_required, mock_mode, data_destination, file_upload_notice,
             risk_notes_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?10, 1, ?11, ?12, ?13,
             datetime('now','localtime'), datetime('now','localtime'))
         ON CONFLICT(id) DO UPDATE SET
             plugin_id = excluded.plugin_id,
             developer_id = excluded.developer_id,
             seller_user_id = COALESCE(products.seller_user_id, excluded.seller_user_id),
             developer_name = excluded.developer_name,
             name = excluded.name,
             description = excluded.description,
             icon = excluded.icon,
             product_type = excluded.product_type,
             license_type = excluded.license_type,
             byok_required = excluded.byok_required,
             mock_mode = 1,
             data_destination = excluded.data_destination,
             file_upload_notice = excluded.file_upload_notice,
             risk_notes_json = excluded.risk_notes_json,
             updated_at = datetime('now','localtime')",
        params![
            seed.id,
            seed.plugin_id,
            seed.developer_id,
            seed.developer_name,
            seed.name,
            seed.description,
            seed.icon,
            product_type_to_str(&seed.product_type),
            license_to_str(&seed.license_type),
            i32::from(seed.byok_required),
            seed.data_destination,
            seed.file_upload_notice,
            serde_json::to_string(&seed.risk_notes)?,
        ],
    )?;
    conn.execute(
        "INSERT INTO product_versions
            (product_id, version, manifest_json, runtime_kind, source, content_hash,
             signature_status, min_app_version, status, changelog, package_path, created_at)
         VALUES (?1, ?2, ?3, ?4, 'marketplace', ?5, 'unsigned', '1.8.0', ?6, ?7, ?8,
             datetime('now','localtime'))
         ON CONFLICT(product_id, version) DO UPDATE SET
             manifest_json = excluded.manifest_json,
             runtime_kind = excluded.runtime_kind,
             source = excluded.source,
             content_hash = excluded.content_hash,
             min_app_version = excluded.min_app_version,
             status = CASE
                WHEN product_versions.status = 'revoked' THEN 'revoked'
                ELSE excluded.status
             END,
             changelog = excluded.changelog,
             package_path = excluded.package_path",
        params![
            seed.id,
            version.version,
            manifest_json,
            runtime_kind_to_str(&seed.runtime_kind),
            hash,
            version.status,
            version.changelog,
            package_dir.to_string_lossy().to_string(),
        ],
    )?;
    let version_id: i64 = conn.query_row(
        "SELECT id FROM product_versions WHERE product_id = ?1 AND version = ?2",
        params![seed.id, version.version],
        |row| row.get(0),
    )?;
    conn.execute(
        "DELETE FROM product_permissions WHERE product_version_id = ?1",
        [version_id],
    )?;
    for permission in &version.permissions {
        conn.execute(
            "INSERT INTO product_permissions (product_version_id, permission, required, reason)
             VALUES (?1, ?2, 1, 'manifest')",
            params![version_id, permission],
        )?;
    }
    conn.execute(
        "INSERT INTO prices (product_id, currency, amount, price_type, is_mock)
         VALUES (?1, 'CNY', ?2, ?3, 1)
         ON CONFLICT(product_id, price_type) DO UPDATE SET
            amount = excluded.amount,
            currency = excluded.currency,
            is_mock = 1",
        params![seed.id, seed.amount, license_to_str(&seed.license_type)],
    )?;
    conn.execute(
        "INSERT INTO product_assets (product_version_id, asset_type, local_path, content_hash, size)
         VALUES (?1, 'package_dir', ?2, ?3, 0)
         ON CONFLICT(product_version_id, asset_type) DO UPDATE SET
            local_path = excluded.local_path,
            content_hash = excluded.content_hash",
        params![version_id, package_dir.to_string_lossy().to_string(), hash],
    )?;
    Ok(())
}

fn product_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProductRow> {
    Ok(ProductRow {
        id: row.get(0)?,
        plugin_id: row.get(1)?,
        developer_id: row.get(2)?,
        developer_name: row.get(3)?,
        seller_user_id: row.get(4)?,
        seller_nickname: row.get(5)?,
        name: row.get(6)?,
        description: row.get(7)?,
        icon: row.get(8)?,
        product_type: row.get(9)?,
        status: row.get(10)?,
        license_type: row.get(11)?,
        byok_required: row.get::<_, i32>(12)? != 0,
        mock_mode: row.get::<_, i32>(13)? != 0,
        data_destination: row.get(14)?,
        file_upload_notice: row.get(15)?,
        risk_notes_json: row.get(16)?,
    })
}

fn version_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VersionRow> {
    Ok(VersionRow {
        id: row.get(0)?,
        product_id: row.get(1)?,
        version: row.get(2)?,
        manifest_json: row.get(3)?,
        runtime_kind: row.get(4)?,
        source: row.get(5)?,
        content_hash: row.get(6)?,
        signature_status: row.get(7)?,
        min_app_version: row.get(8)?,
        status: row.get(9)?,
        changelog: row.get(10)?,
        package_path: row.get(11)?,
    })
}

fn entitlement_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MarketplaceEntitlement> {
    let entitlement_type: String = row.get(2)?;
    let status: String = row.get(3)?;
    Ok(MarketplaceEntitlement {
        id: row.get(0)?,
        product_id: row.get(1)?,
        entitlement_type: parse_license(&entitlement_type),
        status: parse_entitlement_status(&status),
        issued_at: row.get(4)?,
        expires_at: row.get(5)?,
        owner_user_id: row.get(6).ok(),
        order_id: row.get(7).ok(),
    })
}

fn order_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MarketplaceOrder> {
    Ok(MarketplaceOrder {
        id: row.get(0)?,
        buyer_user_id: row.get(1)?,
        seller_user_id: row.get(2)?,
        product_id: row.get(3)?,
        product_name: row.get(4)?,
        product_version_id: row.get(5)?,
        version_snapshot: row.get(6)?,
        currency: row.get(7)?,
        gross_amount: row.get(8)?,
        platform_fee: row.get(9)?,
        seller_income: row.get(10)?,
        payment_status: row.get(11)?,
        settlement_status: row.get(12)?,
        refund_status: row.get(13)?,
        is_mock: row.get::<_, i32>(14)? != 0,
        created_at: row.get(15)?,
        completed_at: row.get(16)?,
    })
}

fn ledger_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MarketplaceLedgerEntry> {
    Ok(MarketplaceLedgerEntry {
        id: row.get(0)?,
        entry_type: row.get(1)?,
        order_id: row.get(2)?,
        order_item_id: row.get(3)?,
        buyer_user_id: row.get(4)?,
        seller_user_id: row.get(5)?,
        product_id: row.get(6)?,
        amount: row.get(7)?,
        currency: row.get(8)?,
        is_mock: row.get::<_, i32>(9)? != 0,
        memo: row.get(10)?,
        created_at: row.get(11)?,
    })
}

fn review_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MarketplaceReviewInfo> {
    Ok(MarketplaceReviewInfo {
        id: row.get(0)?,
        order_id: row.get(1)?,
        product_id: row.get(2)?,
        buyer_user_id: row.get(3)?,
        buyer_nickname: row.get(4)?,
        seller_user_id: row.get(5)?,
        rating: row.get(6)?,
        content: row.get(7)?,
        status: row.get(8)?,
        verified_purchase: row.get::<_, i32>(9)? != 0,
        order_refunded: row.get::<_, i32>(10)? != 0,
        created_at: row.get(11)?,
    })
}

fn manifest_from_row(row: &VersionRow) -> Result<NormalizedPluginManifest, AppError> {
    let manifest: crate::models::MarketplaceManifest = serde_json::from_str(&row.manifest_json)?;
    Ok(NormalizedPluginManifest {
        format: crate::models::PluginManifestFormat::V2,
        schema_version: manifest.schema_version,
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        author_id: Some(manifest.author_id.clone()),
        description: manifest.description.clone(),
        icon: manifest.icon.clone(),
        min_app_version: manifest.min_app_version.clone(),
        product_type: manifest.product_type.clone(),
        runtime_kind: manifest.runtime_kind.clone(),
        source: manifest.source.clone(),
        delivery_mode: manifest.delivery_mode.clone(),
        protocol: manifest.protocol.clone(),
        main: manifest.main.clone(),
        styles: manifest.styles.clone(),
        permissions: manifest.permissions.clone(),
        credential_requirements: manifest.credential_requirements.clone(),
        configuration_schema: manifest.configuration_schema.clone(),
        contributes: manifest.contributes.clone(),
        integrity: manifest.integrity.clone(),
        signature: manifest.signature.clone(),
        legacy_manifest: crate::models::PluginManifest {
            id: manifest.id,
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            author: Some(manifest.author_id),
            main: manifest.main.unwrap_or_default(),
            styles: manifest.styles,
            min_app_version: manifest.min_app_version,
            permissions: manifest.permissions,
            contributes: manifest.contributes,
        },
    })
}

fn packages_root(data_dir: &Path) -> Result<PathBuf, AppError> {
    Ok(data_dir.join(MARKETPLACE_DIR).join(PACKAGES_DIR))
}

fn plugins_dir(data_dir: &Path) -> Result<PathBuf, AppError> {
    let dir = data_dir.join(PLUGINS_DIR);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn package_path(row: &VersionRow) -> Result<PathBuf, AppError> {
    let path = row
        .package_path
        .as_ref()
        .ok_or_else(|| AppError::InvalidInput("商品版本缺少本地包路径".into()))?;
    let path = PathBuf::from(path);
    if !path.is_dir() {
        return Err(AppError::NotFound(format!(
            "商品包不存在: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn copy_dir(source: &Path, dest: &Path) -> Result<(), AppError> {
    if !source.is_dir() {
        return Err(AppError::InvalidInput("来源必须是目录".into()));
    }
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
            return Err(AppError::InvalidInput(
                "插件包不允许符号链接或特殊文件".into(),
            ));
        }
    }
    Ok(())
}

fn ensure_product_active(row: &ProductRow) -> Result<(), AppError> {
    if matches!(row.status.as_str(), "revoked" | "delisted" | "suspended") {
        return Err(AppError::InvalidInput(
            "商品已暂停、下架或吊销，禁止获取、安装或更新".into(),
        ));
    }
    if !matches!(row.status.as_str(), "active" | "approved" | "published") {
        return Err(AppError::InvalidInput(format!(
            "商品状态不可用: {}",
            row.status
        )));
    }
    Ok(())
}

fn ensure_version_installable(row: &VersionRow) -> Result<(), AppError> {
    if row.status == "revoked" || row.signature_status == "revoked" {
        return Err(AppError::InvalidInput(
            "该版本已吊销，禁止安装或更新".into(),
        ));
    }
    if row.signature_status == "invalid" {
        return Err(AppError::InvalidInput("签名无效，禁止安装或更新".into()));
    }
    Ok(())
}

fn permission_confirmation_result(
    row: &ProductRow,
    permissions: Vec<String>,
    message: &str,
) -> MarketplaceActionResult {
    MarketplaceActionResult {
        ok: false,
        product_id: row.id.clone(),
        plugin_id: Some(row.plugin_id.clone()),
        message: message.into(),
        requires_permission_confirmation: true,
        permission_diff: Some(PermissionDiff {
            added: permissions,
            removed: Vec::new(),
            unchanged: Vec::new(),
        }),
        entitlement: None,
        installation: None,
    }
}

fn action_ok(row: &ProductRow, message: &str) -> MarketplaceActionResult {
    MarketplaceActionResult {
        ok: true,
        product_id: row.id.clone(),
        plugin_id: Some(row.plugin_id.clone()),
        message: message.into(),
        requires_permission_confirmation: false,
        permission_diff: None,
        entitlement: None,
        installation: None,
    }
}

fn insert_ledger_tx(
    tx: &Transaction<'_>,
    entry_type: &str,
    order_id: Option<i64>,
    order_item_id: Option<i64>,
    buyer_user_id: Option<&str>,
    seller_user_id: Option<&str>,
    product_id: Option<&str>,
    amount: i64,
    currency: &str,
    memo: &str,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO commerce_ledger_entries
            (entry_type, order_id, order_item_id, buyer_user_id, seller_user_id,
             product_id, amount, currency, is_mock, memo)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
        params![
            entry_type,
            order_id,
            order_item_id,
            buyer_user_id,
            seller_user_id,
            product_id,
            amount,
            currency,
            memo,
        ],
    )?;
    Ok(())
}

fn parse_json_vec(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

fn parse_product_type(value: &str) -> ProductType {
    serde_json::from_value(json!(value)).unwrap_or(ProductType::LocalPlugin)
}

fn parse_runtime_kind(value: &str) -> PluginRuntimeKind {
    serde_json::from_value(json!(value)).unwrap_or(PluginRuntimeKind::LegacyJs)
}

fn parse_source(value: &str) -> PluginSource {
    serde_json::from_value(json!(value)).unwrap_or(PluginSource::Local)
}

fn parse_signature(value: &str) -> SignatureStatus {
    serde_json::from_value(json!(value)).unwrap_or(SignatureStatus::Unsigned)
}

fn parse_product_status(value: &str) -> MarketplaceProductStatus {
    serde_json::from_value(json!(value)).unwrap_or(MarketplaceProductStatus::Active)
}

fn parse_license(value: &str) -> MarketplaceLicenseType {
    serde_json::from_value(json!(value)).unwrap_or(MarketplaceLicenseType::Free)
}

fn parse_entitlement_status(value: &str) -> MarketplaceEntitlementStatus {
    serde_json::from_value(json!(value)).unwrap_or(MarketplaceEntitlementStatus::Active)
}

fn mask_external_reference(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        return "****".into();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}****{tail}")
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

fn sha256_text(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn prompt_templates_for_product(
    product_id: &str,
    _version: &str,
) -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    if product_id != "official-study-summary-prompts" {
        return Vec::new();
    }
    vec![
        (
            "study-summary",
            "学习总结：三层复盘",
            "请基于以下学习内容，按“核心概念 / 易错点 / 下一步行动”三层结构总结：\n\n{{selection}}",
            "把一段课堂笔记或资料整理成可复习的学习总结。",
        ),
        (
            "mistake-review",
            "错题复盘：原因定位",
            "请分析这道错题，输出：题目考点、错误原因、正确思路、同类题提醒。\n\n{{selection}}",
            "用于错题本复盘，帮助定位错误原因。",
        ),
        (
            "weekly-plan",
            "学习周计划：目标拆解",
            "请把以下学习目标拆成一周计划，包含每天任务、预计耗时和检查标准：\n\n{{selection}}",
            "将学习目标拆成可执行的一周安排。",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::models::{
        AgentAuthenticationType, AgentProtocolType, AgentStreamingType, ExternalAgentInput,
        MarketplaceEntitlementStatus, MarketplaceExternalAuthorizationInput,
        MarketplaceReviewInput, PluginDocumentSummaryAgentStartInput,
        PluginDocumentSummaryConfigInput, PluginDocumentSummaryInput,
        PluginDocumentSummaryInsertInput,
    };
    use crate::services::plugins::PluginService;
    use crate::services::xingchen_agent::XingchenAgentService;

    fn temp_data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "firstwork-marketplace-test-{}-{}",
            name,
            std::process::id()
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_current_user(db: &Database, user_id: &str) {
        db.conn_lock()
            .unwrap()
            .execute(
                "INSERT INTO app_config (key, value, updated_at)
                 VALUES ('marketplace.current_user_id', ?1, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                [user_id],
            )
            .unwrap();
    }

    fn bind_external(db: &Database, dir: &Path, product_id: &str) -> MarketplaceActionResult {
        MarketplaceService::bind_external_authorization(
            db,
            dir,
            MarketplaceExternalAuthorizationInput {
                product_id: product_id.into(),
                external_reference: Some("test-external-authorization".into()),
                note: Some("test only".into()),
            },
        )
        .unwrap()
    }

    #[test]
    fn marketplace_query_and_free_acquire_work() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("query");
        let products =
            MarketplaceService::list_products(&db, &dir, MarketplaceProductQuery::default())
                .unwrap();
        assert!(products.len() >= 3);
        let input = MarketplaceAcquireInput {
            product_id: "official-study-summary-prompts".into(),
            license_type: None,
        };
        let result = MarketplaceService::acquire_product(&db, &dir, input).unwrap();
        assert!(result.ok);
        assert!(result.entitlement.is_some());
    }

    #[test]
    fn marketplace_exposes_byok_delivery_and_keeps_legacy_products_compatible() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("delivery-mode");
        let xingchen =
            MarketplaceService::get_product(&db, &dir, "official-xingchen-learning-demo").unwrap();
        assert_eq!(
            xingchen.manifest.delivery_mode,
            Some(AiServiceDeliveryMode::Byok)
        );
        assert_eq!(
            xingchen.manifest.protocol.as_deref(),
            Some("xingchen-workflow-v1")
        );
        assert!(xingchen
            .configuration_schema
            .as_ref()
            .unwrap()
            .get("credentialId")
            .is_some());

        let prompt =
            MarketplaceService::get_product(&db, &dir, "official-study-summary-prompts").unwrap();
        assert!(prompt.manifest.delivery_mode.is_none());
        assert!(prompt.manifest.protocol.is_none());
    }

    #[test]
    fn remote_mcp_mock_configuration_registers_safely_without_a_command_runtime() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("remote-mcp-config");
        let product_id = "official-local-knowledge-mcp-demo";
        let authorization = bind_external(&db, &dir, product_id);
        assert!(authorization.ok);
        assert_eq!(
            authorization.entitlement.as_ref().unwrap().status,
            MarketplaceEntitlementStatus::ExternalAuthorized
        );
        MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: product_id.into(),
                version: None,
                confirm_permissions: true,
            },
            "1.8.0",
        )
        .unwrap();
        MarketplaceService::enable_product(&db, &dir, product_id).unwrap();
        let configured = MarketplaceService::configure_service(
            &db,
            &dir,
            MarketplaceServiceConfigurationInput {
                product_id: product_id.into(),
                credential_id: None,
                network_permission_confirmed: true,
            },
        )
        .unwrap();
        assert!(configured.ok);
        let registered = db.list_mcp_servers().unwrap();
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].transport, "remote-mcp-mock");
        assert_eq!(registered[0].command, "mock://remote-mcp");
        assert!(!registered[0].enabled);
        assert!(registered[0].env.get("credentialId").is_none());
    }

    #[test]
    fn hosted_service_rejects_a_credential_from_another_provider() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("hosted-provider-mismatch");
        let product_id = "official-hosted-ai-api-demo";
        let authorization = bind_external(&db, &dir, product_id);
        assert!(authorization.ok);
        MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: product_id.into(),
                version: None,
                confirm_permissions: true,
            },
            "1.8.0",
        )
        .unwrap();
        MarketplaceService::enable_product(&db, &dir, product_id).unwrap();
        db.conn_lock()
            .unwrap()
            .execute(
                "INSERT INTO credentials
                    (id, provider, credential_type, label, owner_scope, secret_reference, configured)
                 VALUES ('wrong-provider', 'xingchen', 'bearer_token', 'wrong', 'local-user',
                         'secure-credentials/wrong-provider.bin', 1)",
                [],
            )
            .unwrap();

        let error = MarketplaceService::configure_service(
            &db,
            &dir,
            MarketplaceServiceConfigurationInput {
                product_id: product_id.into(),
                credential_id: Some("wrong-provider".into()),
                network_permission_confirmed: true,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("不适用于 hosted-api"));
    }

    #[test]
    fn marketplace_ai_service_requires_external_authorization_and_duplicate_bind_is_safe() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("external-authorization");
        let input = MarketplaceAcquireInput {
            product_id: "official-xingchen-learning-demo".into(),
            license_type: None,
        };
        let acquire = MarketplaceService::acquire_product(&db, &dir, input).unwrap();
        assert!(!acquire.ok);
        assert!(acquire.entitlement.is_none());
        assert!(MarketplaceService::list_orders(&db, &dir)
            .unwrap()
            .is_empty());

        let first = bind_external(&db, &dir, "official-xingchen-learning-demo");
        let second = bind_external(&db, &dir, "official-xingchen-learning-demo");
        assert!(first.ok);
        assert!(second.ok);
        let entitlements = MarketplaceService::list_entitlements(&db, &dir).unwrap();
        let xingchen_entitlements = entitlements
            .iter()
            .filter(|e| e.product_id == "official-xingchen-learning-demo")
            .collect::<Vec<_>>();
        assert_eq!(xingchen_entitlements.len(), 1);
        assert_eq!(
            xingchen_entitlements[0].status,
            MarketplaceEntitlementStatus::ExternalAuthorized
        );
        assert!(MarketplaceService::list_orders(&db, &dir)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn marketplace_external_authorization_is_account_scoped_and_orderless() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("personal-external-auth");
        set_current_user(&db, "local-demo-buyer");

        let result = bind_external(&db, &dir, "official-xingchen-learning-demo");
        assert!(result.ok);
        assert!(MarketplaceService::list_orders(&db, &dir)
            .unwrap()
            .is_empty());
        assert!(MarketplaceService::list_ledger(&db, &dir)
            .unwrap()
            .is_empty());

        let entitlements = MarketplaceService::list_entitlements(&db, &dir).unwrap();
        let owned = entitlements
            .iter()
            .find(|item| item.product_id == "official-xingchen-learning-demo")
            .unwrap();
        assert_eq!(owned.owner_user_id.as_deref(), Some("local-demo-buyer"));
        assert_eq!(owned.order_id, None);
        assert_eq!(
            owned.status,
            MarketplaceEntitlementStatus::ExternalAuthorized
        );

        set_current_user(&db, "local-demo-creator");
        let creator_entitlements = MarketplaceService::list_entitlements(&db, &dir).unwrap();
        assert!(creator_entitlements
            .iter()
            .all(|item| item.product_id != "official-xingchen-learning-demo"));
    }

    #[test]
    fn marketplace_reviews_require_verified_purchase_and_one_review_per_order() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("reviews");
        set_current_user(&db, "local-demo-buyer");

        MarketplaceService::acquire_product(
            &db,
            &dir,
            MarketplaceAcquireInput {
                product_id: "official-study-summary-prompts".into(),
                license_type: None,
            },
        )
        .unwrap();
        let order = MarketplaceService::list_orders(&db, &dir)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let review = MarketplaceService::submit_review(
            &db,
            &dir,
            MarketplaceReviewInput {
                order_id: order.id,
                product_id: "official-study-summary-prompts".into(),
                rating: 5,
                content: "useful local demo product".into(),
            },
        )
        .unwrap();
        assert_eq!(review.buyer_user_id, "local-demo-buyer");
        assert!(review.verified_purchase);

        let duplicate = MarketplaceService::submit_review(
            &db,
            &dir,
            MarketplaceReviewInput {
                order_id: order.id,
                product_id: "official-study-summary-prompts".into(),
                rating: 4,
                content: "second review should fail".into(),
            },
        );
        assert!(duplicate.is_err());

        set_current_user(&db, "local-demo-creator");
        let cross_account = MarketplaceService::submit_review(
            &db,
            &dir,
            MarketplaceReviewInput {
                order_id: order.id,
                product_id: "official-study-summary-prompts".into(),
                rating: 5,
                content: "not my order".into(),
            },
        );
        assert!(cross_account.is_err());
    }

    #[test]
    fn marketplace_blocks_install_without_entitlement() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("no-entitlement");
        let result = MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: "official-study-summary-prompts".into(),
                version: None,
                confirm_permissions: true,
            },
            "1.8.0",
        );
        assert!(result.is_err());
    }

    #[test]
    fn marketplace_install_update_and_uninstall_prompt_pack() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("install");
        MarketplaceService::acquire_product(
            &db,
            &dir,
            MarketplaceAcquireInput {
                product_id: "official-study-summary-prompts".into(),
                license_type: None,
            },
        )
        .unwrap();
        let needs_confirm = MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: "official-study-summary-prompts".into(),
                version: None,
                confirm_permissions: false,
            },
            "1.8.0",
        )
        .unwrap();
        assert!(needs_confirm.requires_permission_confirmation);
        let installed = MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: "official-study-summary-prompts".into(),
                version: None,
                confirm_permissions: true,
            },
            "1.8.0",
        )
        .unwrap();
        assert!(installed.ok);
        let updates = MarketplaceService::check_updates(&db, &dir).unwrap();
        assert!(updates.iter().any(|u| !u.permission_diff.added.is_empty()));
        let update_confirm = MarketplaceService::update_product(
            &db,
            &dir,
            MarketplaceUpdateInput {
                product_id: "official-study-summary-prompts".into(),
                confirm_added_permissions: false,
            },
            "1.8.0",
        )
        .unwrap();
        assert!(update_confirm.requires_permission_confirmation);
        let uninstalled =
            MarketplaceService::uninstall_product(&db, &dir, "official-study-summary-prompts")
                .unwrap();
        assert!(uninstalled.ok);
    }

    #[test]
    fn marketplace_document_summary_plugin_lifecycle_is_usable() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("document-summary");
        MarketplaceService::acquire_product(
            &db,
            &dir,
            MarketplaceAcquireInput {
                product_id: "official-ai-document-summary-plugin".into(),
                license_type: None,
            },
        )
        .unwrap();

        let needs_confirm = MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: "official-ai-document-summary-plugin".into(),
                version: None,
                confirm_permissions: false,
            },
            "1.8.0",
        )
        .unwrap();
        assert!(needs_confirm.requires_permission_confirmation);
        assert!(needs_confirm
            .permission_diff
            .as_ref()
            .unwrap()
            .added
            .contains(&"document.read".to_string()));
        assert!(needs_confirm
            .permission_diff
            .as_ref()
            .unwrap()
            .added
            .contains(&"document.write".to_string()));

        let installed = MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: "official-ai-document-summary-plugin".into(),
                version: None,
                confirm_permissions: true,
            },
            "1.8.0",
        )
        .unwrap();
        assert!(installed.ok);
        assert!(PluginService::document_summary_toolbar_buttons(&db)
            .unwrap()
            .is_empty());

        MarketplaceService::enable_product(&db, &dir, "official-ai-document-summary-plugin")
            .unwrap();
        let buttons = PluginService::document_summary_toolbar_buttons(&db).unwrap();
        assert_eq!(buttons.len(), 1);
        assert_eq!(buttons[0].label, "AI 摘要");

        let summary = PluginService::mock_document_summary(
            &db,
            PluginDocumentSummaryInput {
                plugin_id: "official-ai-document-summary-plugin".into(),
                title: "测试文档".into(),
                content: "# 标题\n\n这里是用于摘要的正文。".into(),
            },
        )
        .unwrap();
        assert!(summary.mock);
        assert!(summary.summary.contains("Mock"));

        PluginService::record_document_summary_insert(
            &db,
            PluginDocumentSummaryInsertInput {
                plugin_id: "official-ai-document-summary-plugin".into(),
                title: "测试文档".into(),
            },
        )
        .unwrap();

        PluginService::revoke_permissions(
            &db,
            "official-ai-document-summary-plugin",
            vec!["document.read".into()],
        )
        .unwrap();
        let blocked = PluginService::mock_document_summary(
            &db,
            PluginDocumentSummaryInput {
                plugin_id: "official-ai-document-summary-plugin".into(),
                title: "测试文档".into(),
                content: "正文".into(),
            },
        );
        assert!(blocked.is_err());
        PluginService::grant_permissions(
            &db,
            "official-ai-document-summary-plugin",
            vec!["document.read".into()],
        )
        .unwrap();

        let authorization = bind_external(&db, &dir, "official-xingchen-learning-demo");
        assert!(authorization.ok);
        MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: "official-xingchen-learning-demo".into(),
                version: None,
                confirm_permissions: true,
            },
            "1.8.0",
        )
        .unwrap();
        MarketplaceService::enable_product(&db, &dir, "official-xingchen-learning-demo").unwrap();

        let agent = XingchenAgentService::create_agent(
            &db,
            &dir,
            ExternalAgentInput {
                product_id: "official-xingchen-learning-demo".into(),
                name: "summary-mock-agent".into(),
                endpoint: "mock://xingchen".into(),
                agent_id: None,
                bot_id: None,
                flow_id: Some("mock-flow".into()),
                protocol_type: AgentProtocolType::Configurable,
                local_uid: None,
                authentication_type: AgentAuthenticationType::None,
                credential_id: None,
                streaming_type: AgentStreamingType::Sse,
                request_mapping_json: Some(r#"{"protocolReady":true}"#.into()),
                response_mapping_json: Some("{}".into()),
                session_mapping_json: Some("{}".into()),
                error_mapping_json: Some("{}".into()),
                mock_mode: Some(true),
                enabled: Some(true),
            },
        )
        .unwrap();
        let summary_agents = PluginService::document_summary_agents(
            &db,
            &dir,
            "official-ai-document-summary-plugin",
        )
        .unwrap();
        assert!(summary_agents.iter().any(|row| row.id == agent.id));
        let config = PluginService::set_document_summary_config(
            &db,
            &dir,
            PluginDocumentSummaryConfigInput {
                plugin_id: "official-ai-document-summary-plugin".into(),
                mode: "agent".into(),
                external_agent_id: Some(agent.id.clone()),
            },
        )
        .unwrap();
        assert_eq!(config.external_agent_id.as_deref(), Some(agent.id.as_str()));
        let (_, title, prompt) = PluginService::prepare_document_summary_agent_start(
            &db,
            &dir,
            PluginDocumentSummaryAgentStartInput {
                plugin_id: "official-ai-document-summary-plugin".into(),
                title: "质量记录".into(),
                content: "表面质量用于描述产品表面缺陷、纹理和可接受标准。".into(),
                external_agent_id: Some(agent.id.clone()),
            },
        )
        .unwrap();
        assert_eq!(title, "质量记录");
        assert!(prompt.contains("表面质量"));
        PluginService::revoke_permissions(
            &db,
            "official-ai-document-summary-plugin",
            vec!["ai.invoke".into()],
        )
        .unwrap();
        let blocked_agent_call = PluginService::prepare_document_summary_agent_start(
            &db,
            &dir,
            PluginDocumentSummaryAgentStartInput {
                plugin_id: "official-ai-document-summary-plugin".into(),
                title: "质量记录".into(),
                content: "表面质量".into(),
                external_agent_id: Some(agent.id),
            },
        );
        assert!(blocked_agent_call.is_err());
        PluginService::grant_permissions(
            &db,
            "official-ai-document-summary-plugin",
            vec!["ai.invoke".into()],
        )
        .unwrap();

        MarketplaceService::disable_product(&db, &dir, "official-ai-document-summary-plugin")
            .unwrap();
        assert!(PluginService::document_summary_toolbar_buttons(&db)
            .unwrap()
            .is_empty());

        let uninstalled =
            MarketplaceService::uninstall_product(&db, &dir, "official-ai-document-summary-plugin")
                .unwrap();
        assert!(uninstalled.ok);
    }

    #[test]
    fn marketplace_revoked_version_blocks_enable() {
        let db = Database::init(":memory:").unwrap();
        let dir = temp_data_dir("revoked");
        let authorization = bind_external(&db, &dir, "official-local-knowledge-mcp-demo");
        assert!(authorization.ok);
        MarketplaceService::install_product(
            &db,
            &dir,
            MarketplaceInstallInput {
                product_id: "official-local-knowledge-mcp-demo".into(),
                version: None,
                confirm_permissions: true,
            },
            "1.8.0",
        )
        .unwrap();
        MarketplaceService::dev_revoke_product_version(
            &db,
            &dir,
            "official-local-knowledge-mcp-demo",
            None,
        )
        .unwrap();
        let result =
            MarketplaceService::enable_product(&db, &dir, "official-local-knowledge-mcp-demo");
        assert!(result.is_err());
    }
}
