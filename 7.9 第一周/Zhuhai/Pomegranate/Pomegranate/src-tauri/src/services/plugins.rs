//! Plugin service layer.
//!
//! Phase 1 keeps the existing plugin tables and runtime, then adds a unified
//! marketplace manifest, package integrity, permission diffing and runtime
//! policy checks around them.

use chrono::Local;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    AgentProtocolType, AiServiceDeliveryMode, ExternalAgentConfig, MarketplaceManifest,
    NormalizedPluginManifest, PermissionDiff, PluginCompatibility, PluginContributes,
    PluginDocumentSummaryAgentFinalizeInput, PluginDocumentSummaryAgentStartInput,
    PluginDocumentSummaryConfig, PluginDocumentSummaryConfigInput, PluginDocumentSummaryInput,
    PluginDocumentSummaryInsertInput, PluginDocumentSummaryResult, PluginDocumentToolbarButton,
    PluginInfo, PluginInstallationInfo, PluginIntegrity, PluginIntegrityCheck, PluginManifest,
    PluginManifestFormat, PluginPackageInspection, PluginRuntimeKind, PluginRuntimePolicy,
    PluginSignature, PluginSource, PluginSummaryAgentOption, ProductType,
};
use crate::services::credentials::CredentialService;
pub(crate) use crate::services::plugin_capabilities::VALID_PERMISSIONS;
use crate::services::resource_ownership::ResourceOwner;
use crate::services::xingchen_agent::XingchenAgentService;

const MANIFEST_FILE_V2: &str = "manifest.json";
const LEGACY_MANIFEST_FILE: &str = "plugin.json";
const SUPPORTED_SCHEMA_VERSION: u32 = 2;
const SUMMARY_MODE_KEY: &str = "summaryMode";
const SUMMARY_EXTERNAL_AGENT_KEY: &str = "summaryExternalAgentId";

pub struct PluginService;

impl PluginService {
    pub fn list(db: &Database) -> Result<Vec<PluginInfo>, AppError> {
        db.list_plugins().map(|plugins| {
            plugins
                .into_iter()
                .map(|plugin| enrich_plugin_info(plugin, None))
                .collect()
        })
    }

    pub fn scan(db: &Database, data_dir: &Path) -> Result<Vec<PluginInfo>, AppError> {
        let plugins_dir = ensure_plugins_dir(data_dir)?;
        for entry in fs::read_dir(&plugins_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let dir = entry.path();
            match Self::read_valid_manifest(&dir).and_then(|manifest| {
                ensure_declared_entry_exists(&dir, &manifest)?;
                let path = dir.to_string_lossy().to_string();
                let hash = Self::calculate_integrity_for_path(&dir)?;
                db.upsert_plugin(&manifest, &path, &hash)
            }) {
                Ok(_) => {}
                Err(e) => log::warn!("[plugins] skipped invalid plugin {}: {}", dir.display(), e),
            }
        }
        Self::list(db)
    }

    pub fn install_from_dir(
        db: &Database,
        data_dir: &Path,
        source_path: &str,
    ) -> Result<PluginInfo, AppError> {
        let source = PathBuf::from(source_path);
        if !source.is_dir() {
            return Err(AppError::InvalidInput("插件来源必须是目录".into()));
        }
        let manifest = Self::read_valid_manifest(&source)?;
        ensure_declared_entry_exists(&source, &manifest)?;

        let plugins_dir = ensure_plugins_dir(data_dir)?;
        let dest = plugins_dir.join(&manifest.id);
        if dest.exists() {
            return Err(AppError::InvalidInput(format!(
                "插件 {} 已存在，请先卸载后再安装",
                manifest.id
            )));
        }

        copy_plugin_dir(&source, &dest)?;
        let installed_manifest = Self::read_valid_manifest(&dest)?;
        ensure_declared_entry_exists(&dest, &installed_manifest)?;
        let installed_path = dest.to_string_lossy().to_string();
        let hash = Self::calculate_integrity_for_path(&dest)?;
        db.upsert_plugin(&installed_manifest, &installed_path, &hash)
            .map(|plugin| enrich_plugin_info(plugin, Some("installed".into())))
    }

    pub fn enable(db: &Database, plugin_id: &str) -> Result<(), AppError> {
        validate_plugin_id(plugin_id)?;
        let policy = Self::can_execute_runtime(db, plugin_id)?;
        if !policy.can_execute {
            db.write_audit_log(
                plugin_id,
                "runtime_blocked",
                policy.blocked_reason.as_deref(),
            )
            .ok();
            return Err(AppError::InvalidInput(
                policy
                    .blocked_reason
                    .unwrap_or_else(|| "插件运行时被安全策略阻止".into()),
            ));
        }
        let integrity = Self::verify_installation(db, plugin_id)?;
        if !integrity.ok {
            db.write_audit_log(plugin_id, "integrity_failed", integrity.message.as_deref())
                .ok();
            return Err(AppError::InvalidInput(
                integrity
                    .message
                    .unwrap_or_else(|| "插件内容已改变，无法启用".into()),
            ));
        }
        db.set_plugin_enabled(plugin_id, true)
    }

    pub fn disable(db: &Database, plugin_id: &str) -> Result<(), AppError> {
        validate_plugin_id(plugin_id)?;
        db.set_plugin_enabled(plugin_id, false)
    }

    pub fn uninstall(db: &Database, data_dir: &Path, plugin_id: &str) -> Result<bool, AppError> {
        validate_plugin_id(plugin_id)?;
        let plugin_dir = ensure_plugins_dir(data_dir)?.join(plugin_id);
        if plugin_dir.exists() {
            fs::remove_dir_all(&plugin_dir)?;
        }
        db.delete_plugin(plugin_id)
    }

    pub fn get_manifest(db: &Database, plugin_id: &str) -> Result<PluginManifest, AppError> {
        validate_plugin_id(plugin_id)?;
        Ok(db.get_plugin(plugin_id)?.manifest)
    }

    pub fn grant_permissions(
        db: &Database,
        plugin_id: &str,
        permissions: Vec<String>,
    ) -> Result<usize, AppError> {
        validate_plugin_id(plugin_id)?;
        validate_permissions(&permissions)?;
        let plugin = db.get_plugin(plugin_id)?;
        ensure_permissions_declared(&plugin, &permissions)?;
        db.grant_plugin_permissions(plugin_id, &permissions)
    }

    pub fn revoke_permissions(
        db: &Database,
        plugin_id: &str,
        permissions: Vec<String>,
    ) -> Result<usize, AppError> {
        validate_plugin_id(plugin_id)?;
        validate_permissions(&permissions)?;
        let plugin = db.get_plugin(plugin_id)?;
        ensure_permissions_declared(&plugin, &permissions)?;
        db.revoke_plugin_permissions(plugin_id, &permissions)
    }

    pub fn get_settings(
        db: &Database,
        plugin_id: &str,
    ) -> Result<HashMap<String, serde_json::Value>, AppError> {
        validate_plugin_id(plugin_id)?;
        db.get_plugin_settings(plugin_id)
    }

    pub fn set_settings(
        db: &Database,
        plugin_id: &str,
        settings: serde_json::Value,
    ) -> Result<(), AppError> {
        validate_plugin_id(plugin_id)?;
        let obj = settings
            .as_object()
            .ok_or_else(|| AppError::InvalidInput("插件设置必须是 JSON 对象".into()))?;
        let map = obj
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<HashMap<_, _>>();
        db.set_plugin_settings(plugin_id, &map)
    }

    pub fn read_asset(
        db: &Database,
        data_dir: &Path,
        plugin_id: &str,
        relative_path: &str,
    ) -> Result<String, AppError> {
        validate_plugin_id(plugin_id)?;
        if !is_safe_relative_path(relative_path, false) {
            return Err(AppError::InvalidInput("资源路径非法".into()));
        }
        let plugin = db.get_plugin(plugin_id)?;
        if plugin.schema_version >= 3 {
            return Err(AppError::InvalidInput(
                "Manifest v3 插件必须通过声明资源专用入口读取资源".into(),
            ));
        }
        Self::read_asset_from_install_path(data_dir, &plugin.path, relative_path)
    }

    pub(crate) fn read_asset_from_install_path(
        data_dir: &Path,
        install_path: &str,
        relative_path: &str,
    ) -> Result<String, AppError> {
        if !is_safe_relative_path(relative_path, false) {
            return Err(AppError::InvalidInput("资源路径非法".into()));
        }
        let plugins_root = fs::canonicalize(ensure_plugins_dir(data_dir)?)?;
        let plugin_dir = fs::canonicalize(PathBuf::from(install_path))?;
        if !plugin_dir.starts_with(&plugins_root) {
            return Err(AppError::InvalidInput("插件安装路径不在受控目录内".into()));
        }
        let asset_path = plugin_dir.join(relative_path);
        if !asset_path.is_file() {
            return Err(AppError::NotFound(format!(
                "插件资源不存在: {}",
                relative_path
            )));
        }
        let canonical_asset = fs::canonicalize(&asset_path)?;
        if !canonical_asset.starts_with(&plugin_dir) {
            return Err(AppError::InvalidInput("资源路径越过插件目录".into()));
        }
        Ok(fs::read_to_string(canonical_asset)?)
    }

    pub fn parse_manifest(plugin_dir: &Path) -> Result<NormalizedPluginManifest, AppError> {
        let manifest = Self::read_manifest(plugin_dir)?;
        validate_normalized_manifest(&manifest)?;
        Ok(manifest)
    }

    pub fn validate_manifest_path(plugin_dir: &Path) -> Result<NormalizedPluginManifest, AppError> {
        Self::parse_manifest(plugin_dir)
    }

    pub fn inspect_package(
        db: &Database,
        plugin_dir: &Path,
        app_version: &str,
    ) -> Result<PluginPackageInspection, AppError> {
        let manifest = Self::read_valid_manifest(plugin_dir)?;
        ensure_declared_entry_exists(plugin_dir, &manifest)?;
        let content_hash = Self::calculate_integrity_for_path(plugin_dir)?;
        let compatibility =
            check_app_compatibility(manifest.min_app_version.as_deref(), app_version);
        let runtime_policy = runtime_policy_for_manifest(&manifest);
        let current = db
            .get_plugin(&manifest.id)
            .map(|p| p.permissions)
            .unwrap_or_default();
        let permission_diff = Self::compare_permissions(current, manifest.permissions.clone());
        Ok(PluginPackageInspection {
            signature_status: manifest.signature.status.clone(),
            manifest,
            content_hash,
            compatibility,
            runtime_policy,
            permission_diff,
        })
    }

    pub fn calculate_integrity_for_path(plugin_dir: &Path) -> Result<String, AppError> {
        if !plugin_dir.is_dir() {
            return Err(AppError::InvalidInput("插件路径必须是目录".into()));
        }
        let mut files = Vec::new();
        for entry in WalkDir::new(plugin_dir).follow_links(false) {
            let entry = entry.map_err(|e| AppError::Custom(e.to_string()))?;
            if entry.file_type().is_symlink() {
                return Err(AppError::InvalidInput(format!(
                    "插件目录不允许包含符号链接: {}",
                    entry.path().display()
                )));
            }
            if entry.file_type().is_file() {
                files.push(entry.path().to_path_buf());
            }
        }
        files.sort();
        let mut hasher = Sha256::new();
        for file in files {
            let rel = file
                .strip_prefix(plugin_dir)
                .map_err(|e| AppError::Custom(e.to_string()))?;
            let rel_string = rel.to_string_lossy().replace('\\', "/");
            hasher.update(rel_string.as_bytes());
            hasher.update([0]);
            hasher.update(fs::read(file)?);
            hasher.update([0]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    pub fn compare_permissions(current: Vec<String>, next: Vec<String>) -> PermissionDiff {
        let current = current.into_iter().collect::<BTreeSet<_>>();
        let next = next.into_iter().collect::<BTreeSet<_>>();
        PermissionDiff {
            added: next.difference(&current).cloned().collect(),
            removed: current.difference(&next).cloned().collect(),
            unchanged: current.intersection(&next).cloned().collect(),
        }
    }

    pub fn check_compatibility(
        min_app_version: Option<String>,
        app_version: &str,
    ) -> PluginCompatibility {
        check_app_compatibility(min_app_version.as_deref(), app_version)
    }

    pub fn get_installation(
        db: &Database,
        plugin_id: &str,
    ) -> Result<Option<PluginInstallationInfo>, AppError> {
        validate_plugin_id(plugin_id)?;
        db.get_plugin_installation(plugin_id)
    }

    pub fn list_installations(db: &Database) -> Result<Vec<PluginInstallationInfo>, AppError> {
        db.list_plugin_installations()
    }

    pub fn verify_installation(
        db: &Database,
        plugin_id: &str,
    ) -> Result<PluginIntegrityCheck, AppError> {
        validate_plugin_id(plugin_id)?;
        let plugin = db.get_plugin(plugin_id)?;
        let expected = plugin.content_hash.clone();
        let actual = Self::calculate_integrity_for_path(Path::new(&plugin.path))?;
        let ok = expected == actual;
        Ok(PluginIntegrityCheck {
            plugin_id: plugin_id.to_string(),
            expected_hash: expected,
            actual_hash: actual,
            ok,
            message: (!ok).then(|| "插件内容已改变，请重新安装或确认更新".into()),
        })
    }

    pub fn can_execute_runtime(
        db: &Database,
        plugin_id: &str,
    ) -> Result<PluginRuntimePolicy, AppError> {
        validate_plugin_id(plugin_id)?;
        let plugin = db.get_plugin(plugin_id)?;
        Ok(runtime_policy_for_plugin(&plugin))
    }

    pub fn document_summary_toolbar_buttons(
        db: &Database,
    ) -> Result<Vec<PluginDocumentToolbarButton>, AppError> {
        let plugins = db.list_plugins()?;
        let mut out = Vec::new();
        for plugin in plugins {
            if !plugin.enabled || plugin.status != "installed" {
                continue;
            }
            if plugin.manifest.contributes.editor_toolbar.is_empty() {
                continue;
            }
            if !document_summary_permissions_granted(db, &plugin.id)? {
                db.write_audit_log(
                    &plugin.id,
                    "permission_denied",
                    Some("document-summary-toolbar"),
                )
                .ok();
                continue;
            }
            for item in &plugin.manifest.contributes.editor_toolbar {
                if item.action.as_deref() != Some("mock-document-summary") {
                    continue;
                }
                out.push(PluginDocumentToolbarButton {
                    plugin_id: plugin.id.clone(),
                    plugin_name: plugin.name.clone(),
                    id: item.id.clone(),
                    label: item.label.clone(),
                    tooltip: item
                        .tooltip
                        .clone()
                        .unwrap_or_else(|| "使用 Mock Provider 生成当前文档摘要".into()),
                    icon: item.icon.clone().unwrap_or_else(|| "Sparkles".into()),
                    action: "mock-document-summary".into(),
                });
            }
        }
        Ok(out)
    }

    pub fn mock_document_summary(
        db: &Database,
        input: PluginDocumentSummaryInput,
    ) -> Result<PluginDocumentSummaryResult, AppError> {
        validate_plugin_id(&input.plugin_id)?;
        let plugin = db.get_plugin(&input.plugin_id)?;
        if !plugin.enabled || plugin.status != "installed" {
            db.write_audit_log(
                &input.plugin_id,
                "document_summary_blocked",
                Some("plugin_disabled"),
            )
            .ok();
            return Err(AppError::InvalidInput(
                "插件未启用，无法读取文档或调用 Mock AI".into(),
            ));
        }
        if !document_summary_permissions_granted(db, &input.plugin_id)? {
            db.write_audit_log(
                &input.plugin_id,
                "permission_denied",
                Some("document-summary"),
            )
            .ok();
            return Err(AppError::InvalidInput(
                "插件权限不足：需要 document.read、document.write、ui.editor.toolbar、ai.invoke"
                    .into(),
            ));
        }

        if input.content.trim().is_empty() {
            db.write_audit_log(
                &input.plugin_id,
                "document_summary_blocked",
                Some("empty_document"),
            )
            .ok();
            return Err(AppError::InvalidInput(
                "当前文档正文为空，无法生成摘要".into(),
            ));
        }

        let title = if input.title.trim().is_empty() {
            "未命名文档".to_string()
        } else {
            input.title.trim().chars().take(120).collect()
        };
        let plain = markdown_to_plain_text(&input.content);
        let word_count = plain.chars().filter(|c| !c.is_whitespace()).count();
        let preview: String = plain.chars().take(260).collect();
        let summary = if preview.trim().is_empty() {
            "Mock 摘要：当前文档暂时没有可摘要的正文内容。".to_string()
        } else {
            format!(
                "Mock 摘要（未调用真实 AI）：《{}》主要内容可概括为：{}{}",
                title,
                preview.trim(),
                if plain.chars().count() > 260 {
                    "…"
                } else {
                    ""
                }
            )
        };

        db.write_audit_log(&input.plugin_id, "document_summary_invoked", Some(&title))
            .ok();
        Ok(PluginDocumentSummaryResult {
            plugin_id: input.plugin_id,
            title,
            summary,
            mock: true,
            provider_label: "Mock Provider（未调用真实 AI）".into(),
            word_count,
            generated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        })
    }

    pub fn record_document_summary_insert(
        db: &Database,
        input: PluginDocumentSummaryInsertInput,
    ) -> Result<(), AppError> {
        validate_plugin_id(&input.plugin_id)?;
        let plugin = db.get_plugin(&input.plugin_id)?;
        if !plugin.enabled || plugin.status != "installed" {
            db.write_audit_log(
                &input.plugin_id,
                "document_summary_insert_blocked",
                Some("plugin_disabled"),
            )
            .ok();
            return Err(AppError::InvalidInput("插件未启用，无法写入文档".into()));
        }
        if !document_summary_permissions_granted(db, &input.plugin_id)? {
            db.write_audit_log(
                &input.plugin_id,
                "permission_denied",
                Some("document-summary-insert"),
            )
            .ok();
            return Err(AppError::InvalidInput(
                "插件权限不足：需要 document.read、document.write、ui.editor.toolbar、ai.invoke"
                    .into(),
            ));
        }

        let title = if input.title.trim().is_empty() {
            "未命名文档".to_string()
        } else {
            input.title.trim().chars().take(120).collect()
        };
        db.write_audit_log(&input.plugin_id, "document_summary_inserted", Some(&title))
            .ok();
        Ok(())
    }

    pub fn document_summary_agents(
        db: &Database,
        data_dir: &Path,
        owner: &ResourceOwner,
        plugin_id: &str,
    ) -> Result<Vec<PluginSummaryAgentOption>, AppError> {
        validate_plugin_id(plugin_id)?;
        let _ = db.get_plugin(plugin_id)?;
        let agents = XingchenAgentService::list_agents(db, owner)?;
        let mut out = Vec::new();
        for agent in agents {
            if !summary_agent_is_available(db, data_dir, owner, &agent) {
                continue;
            }
            out.push(PluginSummaryAgentOption {
                id: agent.id,
                name: agent.name,
                product_id: agent.product_id,
                product_name: agent.product_name,
                provider: agent.provider,
                protocol_type: agent.protocol_type,
                mock_mode: agent.mock_mode,
                enabled: agent.enabled,
            });
        }
        Ok(out)
    }

    pub fn get_document_summary_config(
        db: &Database,
        data_dir: &Path,
        owner: &ResourceOwner,
        plugin_id: &str,
    ) -> Result<PluginDocumentSummaryConfig, AppError> {
        validate_plugin_id(plugin_id)?;
        let _ = db.get_plugin(plugin_id)?;
        let settings = db.get_plugin_settings(plugin_id)?;
        let mode = settings
            .get(SUMMARY_MODE_KEY)
            .and_then(|v| v.as_str())
            .filter(|v| *v == "agent" || *v == "mock")
            .unwrap_or("mock")
            .to_string();
        let configured_external_agent_id = settings
            .get(SUMMARY_EXTERNAL_AGENT_KEY)
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string);
        // plugin settings 是全局存储；账户切换后必须用当前后端可信 owner 重新证明资源归属。
        // 无法证明时只对外隐藏 ID，不改写存量配置，也不自动认领 legacy 资源。
        let external_agent_id = match configured_external_agent_id {
            Some(agent_id) if XingchenAgentService::get_agent(db, owner, &agent_id)?.is_some() => {
                Some(agent_id)
            }
            _ => None,
        };
        Ok(PluginDocumentSummaryConfig {
            plugin_id: plugin_id.to_string(),
            mode,
            external_agent_id,
            available_agents: Self::document_summary_agents(db, data_dir, owner, plugin_id)?,
        })
    }

    pub fn set_document_summary_config(
        db: &Database,
        data_dir: &Path,
        owner: &ResourceOwner,
        input: PluginDocumentSummaryConfigInput,
    ) -> Result<PluginDocumentSummaryConfig, AppError> {
        validate_plugin_id(&input.plugin_id)?;
        let _ = db.get_plugin(&input.plugin_id)?;
        if input.mode != "mock" && input.mode != "agent" {
            return Err(AppError::InvalidInput(
                "摘要模式只能是 mock 或 agent".into(),
            ));
        }
        if input.mode == "agent" {
            let agent_id = input
                .external_agent_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| AppError::InvalidInput("请选择摘要智能体".into()))?;
            let available = Self::document_summary_agents(db, data_dir, owner, &input.plugin_id)?;
            if !available.iter().any(|agent| agent.id == agent_id) {
                db.write_audit_log(&input.plugin_id, "agent_select_failed", Some(agent_id))
                    .ok();
                return Err(AppError::InvalidInput(
                    "所选智能体不可用，请前往 AI 资源中心检查配置".into(),
                ));
            }
            db.set_plugin_setting(
                &input.plugin_id,
                SUMMARY_EXTERNAL_AGENT_KEY,
                &serde_json::Value::String(agent_id.to_string()),
            )?;
            db.write_audit_log(&input.plugin_id, "agent_selected", Some(agent_id))
                .ok();
        } else {
            db.set_plugin_setting(
                &input.plugin_id,
                SUMMARY_EXTERNAL_AGENT_KEY,
                &serde_json::Value::Null,
            )?;
            db.write_audit_log(&input.plugin_id, "agent_selected", Some("mock"))
                .ok();
        }
        db.set_plugin_setting(
            &input.plugin_id,
            SUMMARY_MODE_KEY,
            &serde_json::Value::String(input.mode),
        )?;
        Self::get_document_summary_config(db, data_dir, owner, &input.plugin_id)
    }

    pub fn prepare_document_summary_agent_start(
        db: &Database,
        data_dir: &Path,
        owner: &ResourceOwner,
        input: PluginDocumentSummaryAgentStartInput,
    ) -> Result<(ExternalAgentConfig, String, String), AppError> {
        ensure_document_summary_plugin_ready(db, &input.plugin_id)?;
        if input.content.trim().is_empty() {
            db.write_audit_log(
                &input.plugin_id,
                "document_summary_failed",
                Some("empty_document"),
            )
            .ok();
            return Err(AppError::InvalidInput(
                "当前文档正文为空，无法生成摘要".into(),
            ));
        }
        let config = Self::get_document_summary_config(db, data_dir, owner, &input.plugin_id)?;
        if config.mode != "agent" {
            return Err(AppError::InvalidInput(
                "未配置真实摘要智能体，当前仍为 Mock 演示模式".into(),
            ));
        }
        let external_agent_id = input
            .external_agent_id
            .or(config.external_agent_id)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                AppError::InvalidInput("未配置摘要智能体，请先前往 AI 资源中心配置智能体".into())
            })?;
        let available = Self::document_summary_agents(db, data_dir, owner, &input.plugin_id)?;
        if !available.iter().any(|agent| agent.id == external_agent_id) {
            db.write_audit_log(
                &input.plugin_id,
                "document_summary_failed",
                Some("agent_unavailable"),
            )
            .ok();
            return Err(AppError::InvalidInput(
                "摘要智能体不可用或授权已失效".into(),
            ));
        }
        let agent = XingchenAgentService::get_agent(db, owner, &external_agent_id)?
            .ok_or_else(|| AppError::InvalidInput("摘要智能体不存在".into()))?;
        let title = if input.title.trim().is_empty() {
            "未命名文档".to_string()
        } else {
            input.title.trim().chars().take(120).collect()
        };
        let effective_content = input
            .effective_content
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&input.content);
        let prompt = format!(
            "系统任务：\n请对下面的文档生成结构清晰、忠于原文的中文摘要，不要补充原文不存在的事实。\n\n文档标题：\n{}\n\n文档正文：\n{}",
            title,
            effective_content
        );
        Ok((agent, title, prompt))
    }

    pub fn record_document_summary_agent_started(
        db: &Database,
        plugin_id: &str,
        external_agent_id: &str,
        session_id: &str,
        request_id: &str,
    ) {
        let target = format!(
            "agent={};session={};request={}",
            external_agent_id, session_id, request_id
        );
        db.write_audit_log(plugin_id, "document_summary_started", Some(&target))
            .ok();
    }

    pub fn finalize_document_summary_agent(
        db: &Database,
        input: PluginDocumentSummaryAgentFinalizeInput,
    ) -> Result<(), AppError> {
        validate_plugin_id(&input.plugin_id)?;
        let _ = db.get_plugin(&input.plugin_id)?;
        let action = match input.status.as_str() {
            "completed" => "document_summary_completed",
            "cancelled" => "document_summary_cancelled",
            _ => "document_summary_failed",
        };
        let target = format!(
            "agent={};session={};request={};error={}",
            input.external_agent_id,
            input.session_id,
            input.request_id,
            input.error_code.unwrap_or_default()
        );
        db.write_audit_log(&input.plugin_id, action, Some(&target))
            .ok();
        Ok(())
    }

    fn read_valid_manifest(plugin_dir: &Path) -> Result<NormalizedPluginManifest, AppError> {
        let manifest = Self::read_manifest(plugin_dir)?;
        validate_normalized_manifest(&manifest)?;
        Ok(manifest)
    }

    fn read_manifest(plugin_dir: &Path) -> Result<NormalizedPluginManifest, AppError> {
        let v2_path = plugin_dir.join(MANIFEST_FILE_V2);
        if v2_path.is_file() {
            let content = fs::read_to_string(&v2_path)
                .map_err(|e| AppError::Custom(format!("读取 {} 失败: {}", v2_path.display(), e)))?;
            let manifest: MarketplaceManifest = serde_json::from_str(&content)?;
            if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
                return Err(AppError::InvalidInput(format!(
                    "不支持的 manifest schemaVersion: {}，当前支持 {}",
                    manifest.schema_version, SUPPORTED_SCHEMA_VERSION
                )));
            }
            return Ok(normalize_v2_manifest(manifest));
        }

        let legacy_path = plugin_dir.join(LEGACY_MANIFEST_FILE);
        if legacy_path.is_file() {
            let content = fs::read_to_string(&legacy_path).map_err(|e| {
                AppError::Custom(format!("读取 {} 失败: {}", legacy_path.display(), e))
            })?;
            let manifest: PluginManifest = serde_json::from_str(&content)?;
            return Ok(normalize_legacy_manifest(manifest));
        }

        Err(AppError::NotFound(format!(
            "未找到 {} 或 {}",
            MANIFEST_FILE_V2, LEGACY_MANIFEST_FILE
        )))
    }
}

fn normalize_legacy_manifest(manifest: PluginManifest) -> NormalizedPluginManifest {
    NormalizedPluginManifest {
        format: PluginManifestFormat::Legacy,
        schema_version: 1,
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        author_id: manifest.author.clone(),
        description: manifest.description.clone(),
        icon: None,
        min_app_version: manifest.min_app_version.clone(),
        product_type: ProductType::LocalPlugin,
        runtime_kind: PluginRuntimeKind::LegacyJs,
        source: PluginSource::Development,
        delivery_mode: None,
        protocol: None,
        main: Some(manifest.main.clone()),
        styles: manifest.styles.clone(),
        permissions: manifest.permissions.clone(),
        credential_requirements: Vec::new(),
        configuration_schema: None,
        contributes: manifest.contributes.clone(),
        integrity: PluginIntegrity::default(),
        signature: PluginSignature::default(),
        legacy_manifest: manifest,
    }
}

fn normalize_v2_manifest(manifest: MarketplaceManifest) -> NormalizedPluginManifest {
    let legacy_manifest = PluginManifest {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        author: Some(manifest.author_id.clone()),
        main: manifest.main.clone().unwrap_or_default(),
        styles: manifest.styles.clone(),
        min_app_version: manifest.min_app_version.clone(),
        permissions: manifest.permissions.clone(),
        contributes: manifest.contributes.clone(),
    };
    NormalizedPluginManifest {
        format: PluginManifestFormat::V2,
        schema_version: manifest.schema_version,
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        author_id: Some(manifest.author_id),
        description: manifest.description,
        icon: manifest.icon,
        min_app_version: manifest.min_app_version,
        product_type: manifest.product_type,
        runtime_kind: manifest.runtime_kind,
        source: manifest.source,
        delivery_mode: manifest.delivery_mode,
        protocol: manifest.protocol,
        main: manifest.main,
        styles: manifest.styles,
        permissions: manifest.permissions,
        credential_requirements: manifest.credential_requirements,
        configuration_schema: manifest.configuration_schema,
        contributes: manifest.contributes,
        integrity: manifest.integrity,
        signature: manifest.signature,
        legacy_manifest,
    }
}

fn ensure_plugins_dir(data_dir: &Path) -> Result<PathBuf, AppError> {
    let dir = data_dir.join("plugins");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn validate_normalized_manifest(manifest: &NormalizedPluginManifest) -> Result<(), AppError> {
    validate_plugin_id(&manifest.id)?;
    if manifest.name.trim().is_empty() {
        return Err(AppError::InvalidInput("插件名称不能为空".into()));
    }
    validate_semver(&manifest.version)?;
    if let Some(min_app_version) = &manifest.min_app_version {
        validate_semver(min_app_version)?;
    }
    if manifest.runtime_kind == PluginRuntimeKind::LegacyJs {
        let main = manifest
            .main
            .as_deref()
            .ok_or_else(|| AppError::InvalidInput("legacy-js 插件必须声明 main".into()))?;
        if !is_safe_relative_path(main, false) {
            return Err(AppError::InvalidInput("插件 main 路径非法".into()));
        }
    }
    if let Some(styles) = &manifest.styles {
        if !is_safe_relative_path(styles, false) {
            return Err(AppError::InvalidInput("插件 styles 路径非法".into()));
        }
    }
    if let Some(icon) = &manifest.icon {
        if !is_safe_relative_path(icon, false) {
            return Err(AppError::InvalidInput("插件 icon 路径非法".into()));
        }
    }
    validate_permissions(&manifest.permissions)?;
    validate_service_delivery(manifest)?;
    validate_contributes(&manifest.contributes)?;
    Ok(())
}

fn validate_service_delivery(manifest: &NormalizedPluginManifest) -> Result<(), AppError> {
    let Some(mode) = &manifest.delivery_mode else {
        return Ok(());
    };
    let config = manifest
        .configuration_schema
        .as_ref()
        .unwrap_or(&serde_json::Value::Null);
    match mode {
        AiServiceDeliveryMode::Byok => {
            if !matches!(
                manifest.product_type,
                ProductType::XingchenAgent | ProductType::XingchenWorkflow
            ) {
                return Err(AppError::InvalidInput(
                    "BYOK 交付仅适用于星辰智能体或工作流商品".into(),
                ));
            }
            if manifest.protocol.as_deref() != Some("xingchen-workflow-v1") {
                return Err(AppError::InvalidInput(
                    "BYOK 星辰商品 protocol 必须为 xingchen-workflow-v1".into(),
                ));
            }
            ensure_configuration_has_no_secret_values(config)?;
        }
        AiServiceDeliveryMode::HostedApi => {
            let endpoint = config
                .pointer("/endpoint/default")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if !endpoint.starts_with("https://") && !endpoint.starts_with("mock://") {
                return Err(AppError::InvalidInput(
                    "Hosted API Endpoint 必须使用 HTTPS；Mock 仅允许 mock://".into(),
                ));
            }
            ensure_configuration_has_no_secret_values(config)?;
        }
        AiServiceDeliveryMode::RemoteMcp => {
            if !manifest.permissions.iter().any(|p| p == "mcp.connect")
                || !manifest.permissions.iter().any(|p| p == "network.request")
            {
                return Err(AppError::InvalidInput(
                    "Remote MCP 必须声明 mcp.connect 和 network.request 权限".into(),
                ));
            }
            let endpoint = config
                .pointer("/serverUrl/default")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let local_dev = cfg!(debug_assertions)
                && (endpoint.starts_with("http://localhost")
                    || endpoint.starts_with("http://127.0.0.1"));
            if !endpoint.starts_with("https://") && !local_dev && !endpoint.starts_with("mock://") {
                return Err(AppError::InvalidInput(
                    "Remote MCP URL 必须使用 HTTPS；localhost 仅限开发构建".into(),
                ));
            }
            ensure_configuration_has_no_secret_values(config)?;
        }
    }
    Ok(())
}

fn ensure_configuration_has_no_secret_values(value: &serde_json::Value) -> Result<(), AppError> {
    fn walk(value: &serde_json::Value, key: Option<&str>) -> bool {
        match value {
            serde_json::Value::Object(map) => map.iter().any(|(k, v)| walk(v, Some(k))),
            serde_json::Value::Array(items) => items.iter().any(|v| walk(v, key)),
            serde_json::Value::String(text) => {
                let sensitive_key = key
                    .map(|k| {
                        let k = k.to_ascii_lowercase();
                        k.contains("apikey")
                            || k.contains("api_key")
                            || k.contains("apisecret")
                            || k.contains("api_secret")
                            || k == "token"
                            || k.contains("authorization")
                            || k.contains("privatekey")
                            || k.contains("private_key")
                    })
                    .unwrap_or(false);
                sensitive_key
                    && !text.trim().is_empty()
                    && !matches!(
                        text.as_str(),
                        "credential-reference" | "bearer" | "optional" | "required"
                    )
            }
            _ => false,
        }
    }
    if walk(value, None) {
        Err(AppError::InvalidInput(
            "Manifest 配置中不得包含 API Key、API Secret、Token、Authorization 或私钥明文".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_contributes(contributes: &PluginContributes) -> Result<(), AppError> {
    for command in &contributes.commands {
        if command.id.trim().is_empty() || command.title.trim().is_empty() {
            return Err(AppError::InvalidInput("插件命令 id/title 不能为空".into()));
        }
    }
    for view in contributes
        .views
        .iter()
        .chain(contributes.sidebar_views.iter())
    {
        if view.id.trim().is_empty() || view.title.trim().is_empty() {
            return Err(AppError::InvalidInput("插件视图 id/title 不能为空".into()));
        }
    }
    for prompt in &contributes.prompts {
        if prompt.id.trim().is_empty() || prompt.title.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "Prompt 贡献 id/title 不能为空".into(),
            ));
        }
    }
    for item in &contributes.editor_toolbar {
        if item.id.trim().is_empty() || item.label.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "编辑器工具栏贡献 id/label 不能为空".into(),
            ));
        }
        if item.action.as_deref() != Some("mock-document-summary") {
            return Err(AppError::InvalidInput(
                "编辑器工具栏贡献 action 当前只允许 mock-document-summary".into(),
            ));
        }
    }
    Ok(())
}

fn document_summary_permissions_granted(db: &Database, plugin_id: &str) -> Result<bool, AppError> {
    for permission in [
        "document.read",
        "document.write",
        "ui.editor.toolbar",
        "ai.invoke",
    ] {
        if !db.has_plugin_permission(plugin_id, permission)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn ensure_document_summary_plugin_ready(db: &Database, plugin_id: &str) -> Result<(), AppError> {
    validate_plugin_id(plugin_id)?;
    let plugin = db.get_plugin(plugin_id)?;
    if !plugin.enabled || plugin.status != "installed" {
        db.write_audit_log(
            plugin_id,
            "document_summary_failed",
            Some("plugin_disabled"),
        )
        .ok();
        return Err(AppError::InvalidInput(
            "插件未启用，无法调用摘要智能体".into(),
        ));
    }
    if !document_summary_permissions_granted(db, plugin_id)? {
        db.write_audit_log(
            plugin_id,
            "permission_denied",
            Some("document-summary-agent"),
        )
        .ok();
        return Err(AppError::InvalidInput(
            "插件权限不足：需要 document.read、document.write、ui.editor.toolbar、ai.invoke".into(),
        ));
    }
    Ok(())
}

fn summary_agent_is_available(
    db: &Database,
    data_dir: &Path,
    owner: &ResourceOwner,
    agent: &ExternalAgentConfig,
) -> bool {
    if !agent.enabled || agent.unavailable_reason.is_some() {
        return false;
    }
    let binding_ok = (|| -> Result<bool, AppError> {
        let user_id = db
            .get_config("marketplace.current_user_id")?
            .unwrap_or_else(|| "local-demo-buyer".into());
        let conn = db.conn_lock()?;
        let ok: Option<i64> = conn
            .query_row(
                "SELECT 1
                 FROM products p
                 JOIN plugin_installations pi ON pi.product_id = p.id
                 LEFT JOIN product_versions pv ON pv.id = pi.product_version_id
                 WHERE p.id = ?1
                   AND p.status NOT IN ('revoked', 'suspended', 'delisted')
                   AND pi.enabled = 1
                   AND COALESCE(pi.status, 'installed') = 'installed'
                   AND COALESCE(pv.status, 'active') != 'revoked'
                   AND COALESCE(pv.signature_status, 'unsigned') != 'revoked'
                   AND EXISTS (
                        SELECT 1 FROM entitlements e
                        WHERE e.product_id = p.id
                          AND COALESCE(e.owner_user_id, e.local_user_id) = ?2
                          AND e.status IN ('active', 'external_authorized')
                          AND (e.expires_at IS NULL OR e.expires_at > datetime('now','localtime'))
                   )
                 LIMIT 1",
                rusqlite::params![agent.product_id, user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(ok.is_some())
    })()
    .unwrap_or(false);
    if !binding_ok {
        return false;
    }

    if agent.mock_mode {
        return true;
    }
    let credential_id = match agent.credential_id.as_deref() {
        Some(id) if !id.trim().is_empty() => id,
        _ => return false,
    };
    if CredentialService::load_secret(db, data_dir, owner, credential_id).is_err() {
        return false;
    }
    if agent.protocol_type == AgentProtocolType::XingchenWorkflowV1 {
        return agent
            .flow_id
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .len()
            > 0
            && agent.endpoint == "https://xingchen-api.xf-yun.com/workflow/v1/chat/completions";
    }
    serde_json::from_str::<serde_json::Value>(&agent.request_mapping_json)
        .ok()
        .and_then(|mapping| mapping.get("protocolReady").and_then(|v| v.as_bool()))
        == Some(true)
}

fn markdown_to_plain_text(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches('#')
                .trim_start_matches(['-', '*', '>', ' '])
                .replace(['`', '*', '_', '[', ']', '(', ')'], "")
        })
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), AppError> {
    if plugin_id.is_empty() || plugin_id.len() > 128 {
        return Err(AppError::InvalidInput("插件 ID 长度非法".into()));
    }
    let bytes = plugin_id.as_bytes();
    let valid_first = bytes
        .first()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    let valid_rest = bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    });
    if !valid_first || !valid_rest {
        return Err(AppError::InvalidInput(
            "插件 ID 必须以小写字母或数字开头，且只能包含小写字母、数字、点、连字符和下划线".into(),
        ));
    }
    Ok(())
}

fn validate_semver(version: &str) -> Result<(), AppError> {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts = core.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || p.parse::<u64>().is_err())
    {
        return Err(AppError::InvalidInput(format!(
            "非法语义化版本: {}，应使用 major.minor.patch",
            version
        )));
    }
    Ok(())
}

fn semver_cmp(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    fn parse(v: &str) -> Option<[u64; 3]> {
        let core = v.split(['-', '+']).next().unwrap_or(v);
        let parts = core
            .split('.')
            .map(|p| p.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]])
    }
    Some(parse(left)?.cmp(&parse(right)?))
}

fn validate_permissions(permissions: &[String]) -> Result<(), AppError> {
    for permission in permissions {
        if !VALID_PERMISSIONS.contains(&permission.as_str()) {
            return Err(AppError::InvalidInput(format!(
                "未知插件权限: {}",
                permission
            )));
        }
    }
    Ok(())
}

fn ensure_permissions_declared(
    plugin: &PluginInfo,
    permissions: &[String],
) -> Result<(), AppError> {
    for permission in permissions {
        if !plugin.permissions.contains(permission) {
            return Err(AppError::InvalidInput(format!(
                "插件 {} 未声明权限 {}",
                plugin.id, permission
            )));
        }
    }
    Ok(())
}

fn is_safe_relative_path(value: &str, allow_empty: bool) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return allow_empty;
    }
    let path = Path::new(trimmed);
    !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn ensure_declared_entry_exists(
    plugin_dir: &Path,
    manifest: &NormalizedPluginManifest,
) -> Result<(), AppError> {
    if manifest.runtime_kind == PluginRuntimeKind::LegacyJs {
        let main = manifest.main.as_deref().unwrap_or_default();
        let main_path = plugin_dir.join(main);
        if !main_path.is_file() {
            return Err(AppError::InvalidInput(format!(
                "插件入口文件不存在: {}",
                main
            )));
        }
    }
    if let Some(styles) = &manifest.styles {
        let styles_path = plugin_dir.join(styles);
        if !styles_path.is_file() {
            return Err(AppError::InvalidInput(format!(
                "插件样式文件不存在: {}",
                styles
            )));
        }
    }
    Ok(())
}

fn copy_plugin_dir(source: &Path, dest: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            return Err(AppError::InvalidInput(format!(
                "插件目录不允许包含符号链接: {}",
                entry.path().display()
            )));
        }
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_plugin_dir(&entry.path(), &target)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn check_app_compatibility(
    min_app_version: Option<&str>,
    app_version: &str,
) -> PluginCompatibility {
    match min_app_version {
        None => PluginCompatibility {
            compatible: true,
            app_version: app_version.into(),
            min_app_version: None,
            reason: None,
        },
        Some(min_version) => {
            let compatible = matches!(
                semver_cmp(app_version, min_version),
                Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
            );
            PluginCompatibility {
                compatible,
                app_version: app_version.into(),
                min_app_version: Some(min_version.into()),
                reason: (!compatible)
                    .then(|| format!("需要应用版本 >= {}，当前 {}", min_version, app_version)),
            }
        }
    }
}

fn runtime_policy_for_manifest(manifest: &NormalizedPluginManifest) -> PluginRuntimePolicy {
    runtime_policy(
        manifest.id.clone(),
        manifest.runtime_kind.clone(),
        manifest.source.clone(),
    )
}

fn runtime_policy_for_plugin(plugin: &PluginInfo) -> PluginRuntimePolicy {
    runtime_policy(
        plugin.id.clone(),
        plugin.runtime_kind.clone(),
        plugin.source.clone(),
    )
}

fn runtime_policy(
    plugin_id: String,
    runtime_kind: PluginRuntimeKind,
    source: PluginSource,
) -> PluginRuntimePolicy {
    let allow_local_legacy = cfg!(debug_assertions)
        && std::env::var("FIRSTWORK_ALLOW_LOCAL_LEGACY_JS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

    let (can_execute, raw_invoke_allowed, blocked_reason) = match (&runtime_kind, &source) {
        (PluginRuntimeKind::LegacyJs, PluginSource::Marketplace) => (
            false,
            false,
            Some("公开市场来源插件禁止使用 legacy-js 运行时".into()),
        ),
        (PluginRuntimeKind::LegacyJs, _) if !allow_local_legacy => (
            false,
            false,
            Some(
                "legacy-js 插件默认禁用；仅开发构建显式设置 FIRSTWORK_ALLOW_LOCAL_LEGACY_JS=1 后允许"
                    .into(),
            ),
        ),
        (PluginRuntimeKind::LegacyJs, PluginSource::Local) if !allow_local_legacy => (
            false,
            false,
            Some(
                "本地 legacy-js 插件默认禁用；开发模式需显式设置 FIRSTWORK_ALLOW_LOCAL_LEGACY_JS=1"
                    .into(),
            ),
        ),
        (
            PluginRuntimeKind::LegacyJs,
            PluginSource::Bundled | PluginSource::Internal | PluginSource::Development,
        ) => (true, false, None),
        (PluginRuntimeKind::LegacyJs, PluginSource::Local) => (true, false, None),
        _ => (true, false, None),
    };

    PluginRuntimePolicy {
        plugin_id,
        runtime_kind,
        source,
        can_execute,
        raw_invoke_allowed,
        blocked_reason,
    }
}

fn enrich_plugin_info(mut plugin: PluginInfo, integrity_status: Option<String>) -> PluginInfo {
    let policy = runtime_policy_for_plugin(&plugin);
    plugin.can_execute = policy.can_execute;
    plugin.raw_invoke_allowed = policy.raw_invoke_allowed;
    plugin.blocked_reason = policy.blocked_reason;
    plugin.integrity_status = integrity_status.unwrap_or_else(|| "not_checked".into());
    plugin
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PluginManifestV3;
    use rusqlite::params;

    fn fixture_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "firstwork-plugin-test-{}-{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seed_document_summary_fixture(db: &Database) {
        let manifest = normalize_legacy_manifest(PluginManifest {
            id: "summary-plugin".into(),
            name: "Summary Plugin".into(),
            version: "1.0.0".into(),
            description: None,
            author: Some("tests".into()),
            main: "main.js".into(),
            styles: None,
            min_app_version: None,
            permissions: Vec::new(),
            contributes: PluginContributes::default(),
        });
        db.upsert_plugin(&manifest, "/tmp/summary-plugin", "test-hash")
            .expect("insert summary plugin");
        let conn = db.conn_lock().expect("lock database");
        conn.execute(
            "INSERT INTO products (id, developer_id, name, product_type, status)
             VALUES ('summary-product', 'developer', 'Summary Product', 'xingchen-agent', 'published')",
            [],
        )
        .expect("insert summary product");
    }

    fn seed_summary_agent(db: &Database, id: &str, owner: Option<&ResourceOwner>) {
        let conn = db.conn_lock().expect("lock database");
        conn.execute(
            "INSERT INTO external_agents
                (id, product_id, provider, name, endpoint, protocol_type, mock_mode, enabled)
             VALUES (?1, 'summary-product', 'xingchen', ?1, 'mock://summary',
                     'configurable', 1, 1)",
            params![id],
        )
        .expect("insert summary agent");
        if let Some(owner) = owner {
            conn.execute(
                "INSERT INTO external_agent_resource_ownership
                    (external_agent_id, platform_subject_id, host_installation_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    id,
                    owner.platform_subject_id(),
                    owner.host_installation_id()
                ],
            )
            .expect("insert summary agent ownership");
        }
    }

    fn set_summary_agent_config(db: &Database, agent_id: &str) {
        db.set_plugin_setting(
            "summary-plugin",
            SUMMARY_MODE_KEY,
            &serde_json::Value::String("agent".into()),
        )
        .expect("set summary mode");
        db.set_plugin_setting(
            "summary-plugin",
            SUMMARY_EXTERNAL_AGENT_KEY,
            &serde_json::Value::String(agent_id.into()),
        )
        .expect("set summary agent");
    }

    #[test]
    fn document_summary_config_only_returns_agent_id_to_its_owner() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = fixture_dir("summary-config-owner");
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_document_summary_fixture(&db);
        seed_summary_agent(&db, "owned-agent", Some(&owner));
        set_summary_agent_config(&db, "owned-agent");

        let config =
            PluginService::get_document_summary_config(&db, &data_dir, &owner, "summary-plugin")
                .expect("load owned summary config");
        assert_eq!(config.mode, "agent");
        assert_eq!(config.external_agent_id.as_deref(), Some("owned-agent"));

        fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn document_summary_config_hides_agent_id_from_other_owners() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = fixture_dir("summary-config-cross-owner");
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_document_summary_fixture(&db);
        seed_summary_agent(&db, "private-agent", Some(&owner));
        set_summary_agent_config(&db, "private-agent");

        for denied_owner in [
            ResourceOwner::fixture("subject-b", "installation-a"),
            ResourceOwner::fixture("subject-a", "installation-b"),
        ] {
            let config = PluginService::get_document_summary_config(
                &db,
                &data_dir,
                &denied_owner,
                "summary-plugin",
            )
            .expect("load inaccessible summary config without leaking the agent");
            assert_eq!(config.mode, "agent");
            assert!(config.external_agent_id.is_none());
            assert!(config.available_agents.is_empty());
        }

        fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn document_summary_config_hides_unowned_and_missing_agents_without_claiming_them() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = fixture_dir("summary-config-unowned");
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        seed_document_summary_fixture(&db);
        seed_summary_agent(&db, "legacy-agent", None);

        for inaccessible_id in ["legacy-agent", "missing-agent"] {
            set_summary_agent_config(&db, inaccessible_id);
            let config = PluginService::get_document_summary_config(
                &db,
                &data_dir,
                &owner,
                "summary-plugin",
            )
            .expect("hide inaccessible summary agent");
            assert_eq!(config.mode, "agent");
            assert!(config.external_agent_id.is_none());
        }
        let ownership_count: i64 = db
            .conn_lock()
            .expect("lock database")
            .query_row(
                "SELECT COUNT(*) FROM external_agent_resource_ownership
                 WHERE external_agent_id = 'legacy-agent'",
                [],
                |row| row.get(0),
            )
            .expect("count legacy ownership rows");
        assert_eq!(ownership_count, 0);

        fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn document_summary_config_save_rejects_cross_owner_agent() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = fixture_dir("summary-config-save-owner");
        let owner = ResourceOwner::fixture("subject-a", "installation-a");
        let other_owner = ResourceOwner::fixture("subject-b", "installation-a");
        seed_document_summary_fixture(&db);
        seed_summary_agent(&db, "private-agent", Some(&owner));

        let error = PluginService::set_document_summary_config(
            &db,
            &data_dir,
            &other_owner,
            PluginDocumentSummaryConfigInput {
                plugin_id: "summary-plugin".into(),
                mode: "agent".into(),
                external_agent_id: Some("private-agent".into()),
            },
        )
        .expect_err("cross-owner summary config must be rejected");
        assert!(!error.to_string().contains("private-agent"));
        assert!(db
            .get_plugin_settings("summary-plugin")
            .expect("load settings")
            .get(SUMMARY_EXTERNAL_AGENT_KEY)
            .is_none());

        fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn plugin_id_validation_matches_manifest_v3_rules() {
        for valid in [
            "legacy-demo",
            "com.pomegranate.demo.document-summary",
            "plugin_2",
        ] {
            assert!(validate_plugin_id(valid).is_ok(), "应接受插件 ID: {valid}");
        }

        for invalid in [
            "",
            ".hidden",
            "-leading",
            "_leading",
            "Uppercase",
            "plugin/path",
            "plugin\\path",
            "插件",
        ] {
            assert!(
                validate_plugin_id(invalid).is_err(),
                "应拒绝插件 ID: {invalid}"
            );
        }
    }

    #[test]
    fn generic_asset_read_preserves_legacy_and_rejects_manifest_v3() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = fixture_dir("asset-boundary");
        let plugins_dir = data_dir.join("plugins");

        let legacy_dir = plugins_dir.join("legacy-asset");
        fs::create_dir_all(&legacy_dir).expect("create legacy plugin directory");
        fs::write(legacy_dir.join("main.js"), "legacy content").expect("write legacy asset");
        let legacy = normalize_legacy_manifest(PluginManifest {
            id: "legacy-asset".into(),
            name: "Legacy Asset".into(),
            version: "1.0.0".into(),
            description: None,
            author: Some("tests".into()),
            main: "main.js".into(),
            styles: None,
            min_app_version: None,
            permissions: Vec::new(),
            contributes: PluginContributes::default(),
        });
        let legacy_hash =
            PluginService::calculate_integrity_for_path(&legacy_dir).expect("hash legacy plugin");
        db.upsert_plugin(&legacy, &legacy_dir.to_string_lossy(), &legacy_hash)
            .expect("install legacy plugin");
        assert_eq!(
            PluginService::read_asset(&db, &data_dir, "legacy-asset", "main.js",)
                .expect("read legacy asset"),
            "legacy content"
        );

        let v3_dir = plugins_dir.join("com.firstwork.v3-asset").join("1.0.0");
        fs::create_dir_all(&v3_dir).expect("create v3 plugin directory");
        fs::write(v3_dir.join("ui.json"), "{}").expect("write v3 asset");
        let manifest: PluginManifestV3 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.v3-asset",
            "name": "V3 Asset",
            "version": "1.0.0",
            "authorId": "tests",
            "classification": "feature",
            "runtimeKind": "declarative-ui",
            "permissions": [],
            "contributes": {
                "features": [{
                    "id": "feature",
                    "title": "Feature",
                    "uiSchema": "ui.json"
                }]
            }
        }))
        .expect("parse v3 manifest");
        let v3_hash = PluginService::calculate_integrity_for_path(&v3_dir).expect("hash v3 plugin");
        db.record_plugin_version(&manifest, &v3_dir.to_string_lossy(), &v3_hash, &[])
            .expect("install v3 plugin");
        assert!(PluginService::read_asset(&db, &data_dir, &manifest.id, "ui.json",).is_err());

        fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn legacy_plugin_json_is_normalized() {
        let dir = fixture_dir("legacy");
        fs::write(
            dir.join(LEGACY_MANIFEST_FILE),
            r#"{
                "id":"legacy-demo",
                "name":"Legacy Demo",
                "version":"1.0.0",
                "author":"dev",
                "main":"main.js",
                "permissions":["notes:read"],
                "contributes":{"commands":[],"sidebarViews":[],"settings":false}
            }"#,
        )
        .unwrap();
        fs::write(dir.join("main.js"), "module.exports = {};").unwrap();

        let manifest = PluginService::parse_manifest(&dir).unwrap();
        assert_eq!(manifest.format, PluginManifestFormat::Legacy);
        assert_eq!(manifest.runtime_kind, PluginRuntimeKind::LegacyJs);
        assert_eq!(manifest.source, PluginSource::Development);
        assert_eq!(manifest.main.as_deref(), Some("main.js"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn declarative_manifest_v2_is_accepted_without_main_js() {
        let dir = fixture_dir("declarative");
        fs::write(
            dir.join(MANIFEST_FILE_V2),
            r#"{
                "schemaVersion":2,
                "id":"declarative-demo",
                "name":"Declarative Demo",
                "version":"1.2.3",
                "authorId":"seller-1",
                "productType":"local-plugin",
                "runtimeKind":"declarative-ui",
                "source":"marketplace",
                "permissions":["views.register"],
                "contributes":{"commands":[],"views":[],"sidebarViews":[],"settings":false}
            }"#,
        )
        .unwrap();

        let manifest = PluginService::parse_manifest(&dir).unwrap();
        assert_eq!(manifest.format, PluginManifestFormat::V2);
        assert_eq!(manifest.runtime_kind, PluginRuntimeKind::DeclarativeUi);
        assert_eq!(manifest.source, PluginSource::Marketplace);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn byok_manifest_accepts_credential_reference_but_rejects_embedded_secret() {
        let safe = fixture_dir("byok-safe");
        fs::write(
            safe.join(MANIFEST_FILE_V2),
            r#"{
                "schemaVersion":2,
                "id":"safe-byok",
                "name":"Safe BYOK",
                "version":"1.0.0",
                "authorId":"seller-1",
                "productType":"xingchen-workflow",
                "runtimeKind":"xingchen-workflow",
                "source":"marketplace",
                "deliveryMode":"byok",
                "protocol":"xingchen-workflow-v1",
                "permissions":["credentials.use","agents.invoke","network.xingchen","ai.invoke"],
                "configurationSchema":{
                    "endpoint":{"type":"string","default":"https://xingchen-api.xf-yun.com/workflow/v1/chat/completions"},
                    "credentialId":{"type":"credential-reference","required":true},
                    "flowId":{"type":"string","required":true,"secret":false},
                    "inputParameter":{"type":"string","default":"AGENT_USER_INPUT"}
                }
            }"#,
        ).unwrap();
        assert!(PluginService::parse_manifest(&safe).is_ok());
        fs::remove_dir_all(&safe).ok();

        let unsafe_dir = fixture_dir("byok-secret");
        fs::write(
            unsafe_dir.join(MANIFEST_FILE_V2),
            r#"{
                "schemaVersion":2,
                "id":"unsafe-byok",
                "name":"Unsafe BYOK",
                "version":"1.0.0",
                "authorId":"seller-1",
                "productType":"xingchen-workflow",
                "runtimeKind":"xingchen-workflow",
                "source":"marketplace",
                "deliveryMode":"byok",
                "protocol":"xingchen-workflow-v1",
                "permissions":["credentials.use","agents.invoke","network.xingchen","ai.invoke"],
                "configurationSchema":{"apiKey":"embedded-secret-value"}
            }"#,
        )
        .unwrap();
        assert!(PluginService::parse_manifest(&unsafe_dir).is_err());
        fs::remove_dir_all(&unsafe_dir).ok();
    }

    #[test]
    fn hosted_and_remote_delivery_enforce_endpoint_and_permissions() {
        let hosted = fixture_dir("hosted-http");
        fs::write(
            hosted.join(MANIFEST_FILE_V2),
            r#"{
                "schemaVersion":2,"id":"hosted-http","name":"Hosted","version":"1.0.0","authorId":"seller-1",
                "productType":"xingchen-agent","runtimeKind":"xingchen-agent","source":"marketplace",
                "deliveryMode":"hosted-api","protocol":"hosted-api",
                "permissions":["credentials.use","agents.invoke","network.request","ai.invoke"],
                "configurationSchema":{"endpoint":{"type":"string","default":"http://example.com/api"}}
            }"#,
        ).unwrap();
        assert!(PluginService::parse_manifest(&hosted).is_err());
        fs::remove_dir_all(&hosted).ok();

        let remote = fixture_dir("remote-mcp-permission");
        fs::write(
            remote.join(MANIFEST_FILE_V2),
            r#"{
                "schemaVersion":2,"id":"remote-mcp","name":"Remote MCP","version":"1.0.0","authorId":"seller-1",
                "productType":"mcp-connector","runtimeKind":"mcp-connector","source":"marketplace",
                "deliveryMode":"remote-mcp","protocol":"streamable-http",
                "permissions":["mcp.connect"],
                "configurationSchema":{"serverUrl":{"type":"string","default":"https://mcp.example.com"}}
            }"#,
        ).unwrap();
        assert!(PluginService::parse_manifest(&remote).is_err());
        fs::remove_dir_all(&remote).ok();
    }

    #[test]
    fn unknown_schema_and_invalid_semver_are_rejected() {
        let dir = fixture_dir("bad-schema");
        fs::write(
            dir.join(MANIFEST_FILE_V2),
            r#"{
                "schemaVersion":99,
                "id":"bad-schema",
                "name":"Bad Schema",
                "version":"1.0.0",
                "authorId":"seller-1",
                "productType":"local-plugin",
                "runtimeKind":"declarative-ui",
                "source":"local"
            }"#,
        )
        .unwrap();
        assert!(PluginService::parse_manifest(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);

        let manifest = normalize_legacy_manifest(PluginManifest {
            id: "bad-version".into(),
            name: "Bad Version".into(),
            version: "1.0".into(),
            description: None,
            author: None,
            main: "main.js".into(),
            styles: None,
            min_app_version: None,
            permissions: Vec::new(),
            contributes: PluginContributes::default(),
        });
        assert!(validate_normalized_manifest(&manifest).is_err());
    }

    #[test]
    fn marketplace_legacy_js_is_blocked() {
        let manifest = NormalizedPluginManifest {
            format: PluginManifestFormat::V2,
            schema_version: 2,
            id: "market-js".into(),
            name: "Market JS".into(),
            version: "1.0.0".into(),
            author_id: Some("seller".into()),
            description: None,
            icon: None,
            min_app_version: None,
            product_type: ProductType::LocalPlugin,
            runtime_kind: PluginRuntimeKind::LegacyJs,
            source: PluginSource::Marketplace,
            delivery_mode: None,
            protocol: None,
            main: Some("main.js".into()),
            styles: None,
            permissions: Vec::new(),
            credential_requirements: Vec::new(),
            configuration_schema: None,
            contributes: PluginContributes::default(),
            integrity: PluginIntegrity::default(),
            signature: PluginSignature::default(),
            legacy_manifest: PluginManifest {
                id: "market-js".into(),
                name: "Market JS".into(),
                version: "1.0.0".into(),
                description: None,
                author: Some("seller".into()),
                main: "main.js".into(),
                styles: None,
                min_app_version: None,
                permissions: Vec::new(),
                contributes: PluginContributes::default(),
            },
        };
        let policy = runtime_policy_for_manifest(&manifest);
        assert!(!policy.can_execute);
        assert!(!policy.raw_invoke_allowed);
    }

    #[test]
    fn local_legacy_js_is_blocked_by_default() {
        let manifest = NormalizedPluginManifest {
            format: PluginManifestFormat::V2,
            schema_version: 2,
            id: "local-js".into(),
            name: "Local JS".into(),
            version: "1.0.0".into(),
            author_id: Some("dev".into()),
            description: None,
            icon: None,
            min_app_version: None,
            product_type: ProductType::LocalPlugin,
            runtime_kind: PluginRuntimeKind::LegacyJs,
            source: PluginSource::Local,
            delivery_mode: None,
            protocol: None,
            main: Some("main.js".into()),
            styles: None,
            permissions: Vec::new(),
            credential_requirements: Vec::new(),
            configuration_schema: None,
            contributes: PluginContributes::default(),
            integrity: PluginIntegrity::default(),
            signature: PluginSignature::default(),
            legacy_manifest: PluginManifest {
                id: "local-js".into(),
                name: "Local JS".into(),
                version: "1.0.0".into(),
                description: None,
                author: Some("dev".into()),
                main: "main.js".into(),
                styles: None,
                min_app_version: None,
                permissions: Vec::new(),
                contributes: PluginContributes::default(),
            },
        };
        let policy = runtime_policy_for_manifest(&manifest);
        assert!(!policy.can_execute);
        assert!(!policy.raw_invoke_allowed);
        assert!(policy
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("legacy-js"));
    }

    #[test]
    fn permission_diff_detects_added_removed_unchanged() {
        let diff = PluginService::compare_permissions(
            vec!["notes.read".into(), "tasks.read".into()],
            vec!["tasks.read".into(), "ai.invoke".into()],
        );
        assert_eq!(diff.added, vec!["ai.invoke"]);
        assert_eq!(diff.removed, vec!["notes.read"]);
        assert_eq!(diff.unchanged, vec!["tasks.read"]);
    }
}
