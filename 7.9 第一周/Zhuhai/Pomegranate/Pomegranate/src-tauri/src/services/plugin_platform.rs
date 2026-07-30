//! `.firstwork-plugin` 正式安装、版本管理与贡献解析。

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    AgentWorkflowInvokeResult, PluginArchiveInspection, PluginArchiveLimits,
    PluginExecutionContext, PluginExecutionLogInput, PluginExecutionMode,
    PluginFeatureInvocationSpec, PluginFeatureInvokeResult, PluginInstallArchiveInput,
    PluginInstallResult, PluginManifestV3, PluginRuntimeKind, PluginRuntimePolicy, PluginScene,
    PluginVersionInfo, ResolvedEnhancementContribution, ResolvedPluginContributions,
    SignatureStatus,
};
use crate::services::plugin_permission_guard::{
    require_current_plugin_capabilities, resolve_current_plugin_context, AuthorizedPluginContext,
};
use crate::services::plugins::PluginService;

const ARCHIVE_EXTENSION: &str = "firstwork-plugin";

pub struct PluginPlatformService;

struct ExtractedArchive {
    staging_dir: PathBuf,
    plugin_root: PathBuf,
    root_prefix: Option<String>,
    file_count: usize,
    uncompressed_bytes: u64,
}

impl PluginPlatformService {
    pub fn inspect_archive(
        db: &Database,
        data_dir: &Path,
        archive_path: &Path,
        app_version: &str,
    ) -> Result<PluginArchiveInspection, AppError> {
        validate_archive_extension(archive_path)?;
        let content_hash = hash_file(archive_path)?;
        let extracted =
            extract_archive_safely(data_dir, archive_path, &PluginArchiveLimits::default())?;
        let result = inspect_extracted(db, archive_path, &content_hash, &extracted, app_version);
        let cleanup = fs::remove_dir_all(&extracted.staging_dir);
        if let Err(error) = cleanup {
            log::warn!(
                "[plugin-platform] 清理预检目录失败 {}: {}",
                extracted.staging_dir.display(),
                error
            );
        }
        result
    }

    pub fn install_archive(
        db: &Database,
        data_dir: &Path,
        input: PluginInstallArchiveInput,
        app_version: &str,
    ) -> Result<PluginInstallResult, AppError> {
        let archive_path = PathBuf::from(&input.path);
        let inspection = Self::inspect_archive(db, data_dir, &archive_path, app_version)?;
        if !inspection.compatibility.compatible {
            return Err(AppError::InvalidInput(
                inspection
                    .compatibility
                    .reason
                    .unwrap_or_else(|| "插件与当前应用版本不兼容".into()),
            ));
        }
        if !inspection.runtime_policy.can_execute {
            return Err(AppError::InvalidInput(
                inspection
                    .runtime_policy
                    .blocked_reason
                    .unwrap_or_else(|| "该插件运行时不允许通过正式插件包安装".into()),
            ));
        }
        if inspection.content_hash != input.expected_hash {
            return Err(AppError::InvalidInput(
                "插件包在预检后发生变化，请重新预检".into(),
            ));
        }
        if !inspection.conflicts.is_empty() || !inspection.missing_dependencies.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "依赖或冲突检查未通过：{}{}",
                inspection.missing_dependencies.join("、"),
                inspection.conflicts.join("、")
            )));
        }
        if inspection.signature_status == SignatureStatus::Unsigned && !input.confirm_unsigned {
            return Err(AppError::InvalidInput(
                "未签名插件必须在确认窗口中明确同意后才能安装".into(),
            ));
        }
        let approved: BTreeSet<_> = input.approved_permissions.iter().cloned().collect();
        let requested: BTreeSet<_> = inspection.manifest.permissions.iter().cloned().collect();
        if approved != requested {
            return Err(AppError::InvalidInput(
                "必须明确同意 Manifest 声明的全部权限后才能安装".into(),
            ));
        }

        let extracted =
            extract_archive_safely(data_dir, &archive_path, &PluginArchiveLimits::default())?;
        let manifest = read_manifest_v3(&extracted.plugin_root)?;
        if manifest.id != inspection.manifest.id || manifest.version != inspection.manifest.version
        {
            fs::remove_dir_all(&extracted.staging_dir).ok();
            return Err(AppError::InvalidInput(
                "安装阶段 Manifest 与预检结果不一致".into(),
            ));
        }

        let plugins_dir = ensure_plugins_dir(data_dir)?;
        let plugin_dir = plugins_dir.join(&manifest.id);
        let versions_dir = plugin_dir.join("versions");
        fs::create_dir_all(&versions_dir)?;
        let final_dir = versions_dir.join(&manifest.version);
        if final_dir.exists() {
            fs::remove_dir_all(&extracted.staging_dir).ok();
            return Err(AppError::InvalidInput(format!(
                "插件 {} 的版本 {} 已安装",
                manifest.id, manifest.version
            )));
        }

        // staging 与 versions 位于同一文件系统，rename 是原子的。
        let installing_dir = versions_dir.join(format!(".installing-{}", Uuid::new_v4()));
        let install_result = (|| -> Result<PluginInstallResult, AppError> {
            fs::rename(&extracted.plugin_root, &installing_dir)?;
            fs::rename(&installing_dir, &final_dir)?;
            let install_path = final_dir.to_string_lossy().to_string();
            // 预检哈希用于确认用户批准的压缩包未被替换；落盘后改用目录哈希，
            // 这样后续启用/运行前的完整性校验能准确发现已安装文件被篡改。
            let installed_content_hash = PluginService::calculate_integrity_for_path(&final_dir)?;
            let previous = db.record_plugin_version(
                &manifest,
                &install_path,
                &installed_content_hash,
                &input.approved_permissions,
            )?;
            write_current_pointer(&plugin_dir, &manifest.version, &installed_content_hash)?;
            Ok(PluginInstallResult {
                plugin_id: manifest.id.clone(),
                version: manifest.version.clone(),
                install_path,
                previous_version: previous,
                content_hash: installed_content_hash,
                enabled: false,
            })
        })();

        if let Err(error) = &install_result {
            // 数据库失败时不留下半安装版本；旧 current 指针尚未切换。
            fs::remove_dir_all(&installing_dir).ok();
            fs::remove_dir_all(&final_dir).ok();
            log::error!("[plugin-platform] 原子安装失败: {}", error);
        }
        fs::remove_dir_all(&extracted.staging_dir).ok();
        install_result
    }

    pub fn list_versions(
        db: &Database,
        plugin_id: &str,
    ) -> Result<Vec<PluginVersionInfo>, AppError> {
        validate_plugin_id(plugin_id)?;
        db.list_plugin_versions_v3(plugin_id)
    }

    pub fn rollback(
        db: &Database,
        data_dir: &Path,
        plugin_id: &str,
        version: &str,
    ) -> Result<PluginInstallResult, AppError> {
        validate_plugin_id(plugin_id)?;
        validate_version(version)?;
        let (manifest, install_path, content_hash) = db
            .plugin_version_manifest(plugin_id, version)?
            .ok_or_else(|| AppError::NotFound(format!("未找到版本 {}", version)))?;
        if !Path::new(&install_path).is_dir() {
            return Err(AppError::NotFound(format!(
                "版本目录不存在：{}",
                install_path
            )));
        }
        validate_manifest_resources(Path::new(&install_path), &manifest)?;
        let actual_content_hash =
            PluginService::calculate_integrity_for_path(Path::new(&install_path))?;
        if actual_content_hash != content_hash {
            return Err(AppError::InvalidInput(
                "插件内容已改变，无法回滚到该版本".into(),
            ));
        }
        let previous = db.switch_plugin_version(&manifest, &install_path, &content_hash)?;
        let plugin_dir = ensure_plugins_dir(data_dir)?.join(plugin_id);
        write_current_pointer(&plugin_dir, version, &content_hash)?;
        Ok(PluginInstallResult {
            plugin_id: plugin_id.to_string(),
            version: version.to_string(),
            install_path,
            previous_version: previous,
            content_hash,
            enabled: false,
        })
    }

    pub fn resolve_enabled_contributions(
        db: &Database,
        context: PluginExecutionContext,
    ) -> Result<ResolvedPluginContributions, AppError> {
        let mut active_plugins = Vec::new();
        let mut features = Vec::new();
        let mut agents = Vec::new();
        let mut tools = Vec::new();
        let mut enhancements = Vec::new();
        let mut warnings = Vec::new();
        let installed = db.current_v3_plugins()?;
        let installed_ids: HashSet<String> = installed
            .iter()
            .map(|(manifest, _, _)| manifest.id.clone())
            .collect();

        for (manifest, _install_path, legacy_enabled) in installed {
            let matching_enhancement_ids = manifest
                .contributes
                .enhancements
                .iter()
                .filter(|item| {
                    scene_matches(&item.scenes, &context.scene)
                        && (item.features.is_empty() || item.features.contains(&context.feature))
                })
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            if !legacy_enabled {
                if !matching_enhancement_ids.is_empty() {
                    if let Err(error) = require_current_plugin_capabilities(
                        db,
                        &manifest.id,
                        &["ai.context.augment"],
                    ) {
                        append_enhancement_guard_warnings(
                            &mut warnings,
                            &manifest.id,
                            &matching_enhancement_ids,
                            &error,
                        );
                    }
                }
                continue;
            }
            let override_value = context.session_overrides.get(&manifest.id).copied();
            let (enabled, _) = db.resolve_plugin_enabled(
                &manifest,
                &context.scene,
                &context.feature,
                override_value,
            )?;
            if !enabled {
                continue;
            }
            let conflicts = manifest
                .conflicts_with
                .iter()
                .filter(|item| installed_ids.contains(&item.id))
                .map(|item| item.id.clone())
                .collect::<Vec<_>>();
            if !conflicts.is_empty() {
                warnings.push(format!(
                    "插件 {} 因冲突未激活：{}",
                    manifest.id,
                    conflicts.join("、")
                ));
                continue;
            }
            active_plugins.push(manifest.id.clone());
            features.extend(manifest.contributes.features.iter().filter_map(|item| {
                if !scene_matches(&item.scenes, &context.scene) {
                    return None;
                }
                let mut resolved = item.clone();
                resolved.plugin_id = Some(manifest.id.clone());
                Some(resolved)
            }));
            agents.extend(
                manifest
                    .contributes
                    .agents
                    .iter()
                    .filter(|item| scene_matches(&item.scenes, &context.scene))
                    .cloned(),
            );
            tools.extend(manifest.contributes.tools.iter().cloned());
            if matching_enhancement_ids.is_empty() {
                continue;
            }
            let authorized = match require_current_plugin_capabilities(
                db,
                &manifest.id,
                &["ai.context.augment"],
            ) {
                Ok(authorized) => authorized,
                Err(error) => {
                    append_enhancement_guard_warnings(
                        &mut warnings,
                        &manifest.id,
                        &matching_enhancement_ids,
                        &error,
                    );
                    continue;
                }
            };
            let authorized_override = context
                .session_overrides
                .get(&authorized.manifest.id)
                .copied();
            let (authorized_enabled, _) = db.resolve_plugin_enabled(
                &authorized.manifest,
                &context.scene,
                &context.feature,
                authorized_override,
            )?;
            if !authorized_enabled {
                continue;
            }
            for item in authorized
                .manifest
                .contributes
                .enhancements
                .iter()
                .filter(|item| {
                    scene_matches(&item.scenes, &context.scene)
                        && (item.features.is_empty() || item.features.contains(&context.feature))
                })
            {
                enhancements.push(ResolvedEnhancementContribution {
                    plugin_id: authorized.manifest.id.clone(),
                    contribution: item.clone(),
                    resource_path: Path::new(&authorized.install_path)
                        .join(&item.handler.resource)
                        .to_string_lossy()
                        .to_string(),
                });
            }
        }
        let enhancements = order_enhancements(enhancements, &mut warnings);
        Ok(ResolvedPluginContributions {
            context,
            active_plugins,
            features,
            agents,
            tools,
            enhancements,
            warnings,
        })
    }

    pub fn read_enhancement_resource(
        db: &Database,
        data_dir: &Path,
        plugin_id: &str,
        contribution_id: &str,
    ) -> Result<String, AppError> {
        validate_plugin_id(plugin_id)?;
        validate_contribution_id(contribution_id)?;
        let authorized =
            require_current_plugin_capabilities(db, plugin_id, &["ai.context.augment"])?;
        let contribution = authorized
            .manifest
            .contributes
            .enhancements
            .iter()
            .find(|item| item.id == contribution_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "当前 Manifest 未声明 enhancement：{}",
                    contribution_id
                ))
            })?;
        if contribution.handler.kind != "declarative" {
            return Err(AppError::InvalidInput(
                "enhancement 资源读取只支持 declarative handler".into(),
            ));
        }
        PluginService::read_asset_from_install_path(
            data_dir,
            &authorized.install_path,
            &contribution.handler.resource,
        )
    }

    pub fn read_feature_ui_schema(
        db: &Database,
        data_dir: &Path,
        plugin_id: &str,
        feature_id: &str,
    ) -> Result<String, AppError> {
        validate_plugin_id(plugin_id)?;
        validate_contribution_id(feature_id)?;
        let current = resolve_current_plugin_context(db, plugin_id)?;
        let feature = current
            .manifest
            .contributes
            .features
            .iter()
            .find(|item| item.id == feature_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("当前 Manifest 未声明 feature：{}", feature_id))
            })?;
        let ui_schema = feature
            .ui_schema
            .as_deref()
            .ok_or_else(|| AppError::InvalidInput("feature 缺少 uiSchema".into()))?;
        PluginService::read_asset_from_install_path(data_dir, &current.install_path, ui_schema)
    }

    pub fn record_execution(db: &Database, input: PluginExecutionLogInput) -> Result<(), AppError> {
        validate_plugin_id(&input.plugin_id)?;
        validate_contribution_id(&input.contribution_id)?;
        if !matches!(input.status.as_str(), "success" | "failed" | "skipped") {
            return Err(AppError::InvalidInput("非法插件执行状态".into()));
        }
        db.get_plugin(&input.plugin_id)?;
        let hook = serde_json::to_value(&input.hook)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "unknown".into());
        db.record_plugin_execution(
            &input.plugin_id,
            Some(&input.contribution_id),
            Some(&hook),
            input.context.scene.as_str(),
            &input.context.feature,
            &input.context.request_id,
            &input.status,
            input.duration_ms,
            input.error_message.as_deref(),
        )
    }

    pub fn prepare_feature_invocation(
        db: &Database,
        plugin_id: &str,
        feature_id: &str,
    ) -> Result<PluginFeatureInvocationSpec, AppError> {
        validate_plugin_id(plugin_id)?;
        validate_contribution_id(feature_id)?;
        let plugin = db.get_plugin(plugin_id)?;
        if !plugin.enabled || plugin.status != "installed" {
            return Err(AppError::InvalidInput("插件尚未安装并启用".into()));
        }
        let integrity = PluginService::verify_installation(db, plugin_id)?;
        if !integrity.ok {
            return Err(AppError::InvalidInput(
                integrity
                    .message
                    .unwrap_or_else(|| "插件内容完整性校验失败".into()),
            ));
        }
        let fixed_capabilities = [
            "credentials.use",
            "agents.invoke",
            "network.xingchen",
            "ai.invoke",
        ];
        let authorized = require_current_plugin_capabilities(db, plugin_id, &fixed_capabilities)?;
        let output_kind = Self::validate_feature_context(db, &authorized, feature_id)?;
        let authorized = if matches!(output_kind.as_str(), "docx-base64" | "file-base64") {
            let all_capabilities = [
                "credentials.use",
                "agents.invoke",
                "network.xingchen",
                "ai.invoke",
                "files.writeSelected",
            ];
            require_current_plugin_capabilities(db, plugin_id, &all_capabilities)?
        } else {
            authorized
        };
        let output_kind = Self::validate_feature_context(db, &authorized, feature_id)?;
        Ok(PluginFeatureInvocationSpec { output_kind })
    }

    fn validate_feature_context(
        db: &Database,
        authorized: &AuthorizedPluginContext,
        feature_id: &str,
    ) -> Result<String, AppError> {
        let manifest = &authorized.manifest;
        if manifest.version != authorized.version {
            return Err(AppError::InvalidInput(
                "授权上下文中的插件版本不一致".into(),
            ));
        }
        if !matches!(
            manifest.classification,
            crate::models::PluginClassification::Feature
                | crate::models::PluginClassification::Hybrid
        ) {
            return Err(AppError::InvalidInput(
                "该插件没有独立 feature 调用入口".into(),
            ));
        }
        if !matches!(
            manifest.runtime_kind,
            PluginRuntimeKind::XingchenAgent | PluginRuntimeKind::XingchenWorkflow
        ) {
            return Err(AppError::InvalidInput(
                "该 feature 不是受控星辰 Workflow 运行时".into(),
            ));
        }
        let feature = manifest
            .contributes
            .features
            .iter()
            .find(|feature| feature.id == feature_id)
            .ok_or_else(|| AppError::NotFound(format!("未找到插件功能 {}", feature_id)))?;
        let scene = feature
            .scenes
            .first()
            .cloned()
            .unwrap_or(PluginScene::Global);
        let (enabled, _) = db.resolve_plugin_enabled(&manifest, &scene, feature_id, None)?;
        if !enabled {
            return Err(AppError::InvalidInput("当前插件功能已被禁用".into()));
        }
        let ui_schema = feature
            .ui_schema
            .as_deref()
            .ok_or_else(|| AppError::InvalidInput("feature 缺少 uiSchema".into()))?;
        let schema_path = Path::new(&authorized.install_path).join(ui_schema);
        let schema_text = fs::read_to_string(&schema_path)?;
        let schema: serde_json::Value = serde_json::from_str(&schema_text)
            .map_err(|error| AppError::InvalidInput(format!("uiSchema JSON 无效：{}", error)))?;
        let output_kind = schema
            .pointer("/output/kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("text")
            .to_ascii_lowercase();
        if !matches!(
            output_kind.as_str(),
            "text" | "markdown" | "json" | "docx-base64" | "file-base64"
        ) {
            return Err(AppError::InvalidInput(format!(
                "不支持的 feature 输出类型：{}",
                output_kind
            )));
        }
        Ok(output_kind)
    }

    pub fn finish_feature_invocation(
        spec: PluginFeatureInvocationSpec,
        result: AgentWorkflowInvokeResult,
    ) -> Result<PluginFeatureInvokeResult, AppError> {
        let content = if !result.ok {
            result.message.clone()
        } else if spec.output_kind == "json" {
            normalize_json_output(&result.content)?
        } else {
            result.content.clone()
        };
        if result.ok
            && matches!(spec.output_kind.as_str(), "docx-base64" | "file-base64")
            && result.output_files.is_empty()
        {
            return Err(AppError::InvalidInput(
                "Workflow 未返回可验证的文件输出".into(),
            ));
        }
        Ok(PluginFeatureInvokeResult {
            ok: result.ok,
            request_id: Some(result.request_id),
            content,
            output_kind: spec.output_kind,
            output_files: result.output_files,
            progress: result.progress,
            usage: result.usage,
            mock: result.mock,
        })
    }

    pub fn record_feature_invocation(
        db: &Database,
        plugin_id: &str,
        feature_id: &str,
        request_id: &str,
        status: &str,
        duration_ms: i64,
        error: Option<&str>,
    ) {
        let safe_error = error.map(|value| {
            value
                .chars()
                .take(500)
                .collect::<String>()
                .replace('\n', " ")
        });
        db.record_plugin_execution(
            plugin_id,
            Some(feature_id),
            Some("featureInvoke"),
            "global",
            feature_id,
            request_id,
            status,
            Some(duration_ms),
            safe_error.as_deref(),
        )
        .ok();
    }
}

fn normalize_json_output(content: &str) -> Result<String, AppError> {
    let mut candidate = content.trim();
    if candidate.starts_with("```") {
        candidate = candidate
            .strip_prefix("```json")
            .or_else(|| candidate.strip_prefix("```JSON"))
            .or_else(|| candidate.strip_prefix("```"))
            .unwrap_or(candidate)
            .trim();
        candidate = candidate.strip_suffix("```").unwrap_or(candidate).trim();
    }
    let mut value: serde_json::Value = serde_json::from_str(candidate)
        .map_err(|error| AppError::InvalidInput(format!("Workflow JSON 输出无效：{}", error)))?;
    for _ in 0..2 {
        let serde_json::Value::String(inner) = &value else {
            break;
        };
        value = serde_json::from_str(inner).map_err(|error| {
            AppError::InvalidInput(format!("Workflow 二次 JSON 输出无效：{}", error))
        })?;
    }
    serde_json::to_string_pretty(&value).map_err(AppError::from)
}

fn inspect_extracted(
    db: &Database,
    archive_path: &Path,
    content_hash: &str,
    extracted: &ExtractedArchive,
    app_version: &str,
) -> Result<PluginArchiveInspection, AppError> {
    let manifest = read_manifest_v3(&extracted.plugin_root)?;
    validate_manifest(&manifest)?;
    validate_manifest_resources(&extracted.plugin_root, &manifest)?;
    scan_package_security(&extracted.plugin_root)?;
    let compatibility =
        PluginService::check_compatibility(manifest.min_app_version.clone(), app_version);
    let current_permissions = db
        .current_version_declared_permissions(&manifest.id)?
        .unwrap_or_default();
    let permission_diff =
        PluginService::compare_permissions(current_permissions, manifest.permissions.clone());
    let installed = db.current_v3_plugins()?;
    let installed_ids: HashSet<_> = installed
        .iter()
        .map(|(item, _, _)| item.id.as_str())
        .collect();
    let missing_dependencies = manifest
        .dependencies
        .iter()
        .filter(|item| item.required && !installed_ids.contains(item.id.as_str()))
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let conflicts = manifest
        .conflicts_with
        .iter()
        .filter(|item| installed_ids.contains(item.id.as_str()))
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    let signature_status = match manifest.signature.status {
        SignatureStatus::Invalid | SignatureStatus::Revoked => {
            return Err(AppError::InvalidInput("插件签名无效或已撤销".into()));
        }
        SignatureStatus::Valid => {
            warnings.push("当前版本只记录签名状态，尚未配置可信公钥验证器".into());
            SignatureStatus::Valid
        }
        SignatureStatus::Unsigned => {
            warnings.push("这是未签名插件，仅应安装来自可信来源的包".into());
            SignatureStatus::Unsigned
        }
    };
    if !compatibility.compatible {
        warnings.push(
            compatibility
                .reason
                .clone()
                .unwrap_or_else(|| "应用版本不兼容".into()),
        );
    }
    if !permission_diff.added.is_empty() {
        warnings.push(format!(
            "插件请求新增权限：{}",
            permission_diff.added.join("、")
        ));
    }
    let runtime_policy = runtime_policy_for_manifest(&manifest);
    if !runtime_policy.can_execute {
        warnings.push(
            runtime_policy
                .blocked_reason
                .clone()
                .unwrap_or_else(|| "运行时被策略阻止".into()),
        );
    }
    let requires_confirmation = signature_status == SignatureStatus::Unsigned
        || !permission_diff.added.is_empty()
        || db.current_plugin_version(&manifest.id)?.is_some();
    Ok(PluginArchiveInspection {
        archive_path: archive_path.to_string_lossy().to_string(),
        manifest,
        content_hash: content_hash.to_string(),
        root_prefix: extracted.root_prefix.clone(),
        file_count: extracted.file_count,
        uncompressed_bytes: extracted.uncompressed_bytes,
        compatibility,
        added_permissions: permission_diff.added.clone(),
        removed_permissions: permission_diff.removed.clone(),
        permission_diff,
        conflicts,
        missing_dependencies,
        signature_status,
        runtime_policy,
        warnings,
        requires_confirmation,
    })
}

fn extract_archive_safely(
    data_dir: &Path,
    archive_path: &Path,
    limits: &PluginArchiveLimits,
) -> Result<ExtractedArchive, AppError> {
    let metadata = fs::metadata(archive_path)?;
    if metadata.len() == 0 || metadata.len() > limits.max_archive_bytes {
        return Err(AppError::InvalidInput(format!(
            "插件包大小必须在 1 字节到 {} MiB 之间",
            limits.max_archive_bytes / 1024 / 1024
        )));
    }
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::InvalidInput(format!("无法读取插件压缩包：{}", error)))?;
    if archive.len() == 0 || archive.len() > limits.max_files {
        return Err(AppError::InvalidInput(format!(
            "插件包文件数量超过限制 {}",
            limits.max_files
        )));
    }
    let staging_parent = ensure_plugins_dir(data_dir)?.join(".staging");
    fs::create_dir_all(&staging_parent)?;
    let staging_dir = staging_parent.join(Uuid::new_v4().to_string());
    fs::create_dir(&staging_dir)?;
    let mut seen = HashSet::new();
    let mut manifest_paths = Vec::new();
    let mut total = 0u64;
    let mut file_count = 0usize;

    let extraction = (|| -> Result<(), AppError> {
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                AppError::InvalidInput(format!("读取压缩包条目失败：{}", error))
            })?;
            let raw_name = entry.name().replace('\\', "/");
            if raw_name.contains('\0') || raw_name.starts_with('/') || has_windows_prefix(&raw_name)
            {
                return Err(AppError::InvalidInput(format!(
                    "插件包包含非法绝对路径：{}",
                    raw_name
                )));
            }
            let enclosed = entry.enclosed_name().ok_or_else(|| {
                AppError::InvalidInput(format!("插件包包含目录穿越路径：{}", raw_name))
            })?;
            validate_relative_path(&enclosed, limits.max_depth)?;
            let normalized = normalize_relative_path(&enclosed)?;
            let duplicate_key = normalized.to_lowercase();
            if !seen.insert(duplicate_key) {
                return Err(AppError::InvalidInput(format!(
                    "插件包包含重复文件名：{}",
                    normalized
                )));
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err(AppError::InvalidInput(format!(
                    "插件包不允许符号链接：{}",
                    normalized
                )));
            }
            if entry.size() > limits.max_file_bytes {
                return Err(AppError::InvalidInput(format!(
                    "单个文件超过 {} MiB：{}",
                    limits.max_file_bytes / 1024 / 1024,
                    normalized
                )));
            }
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| AppError::InvalidInput("插件包解压大小溢出".into()))?;
            if total > limits.max_uncompressed_bytes {
                return Err(AppError::InvalidInput(format!(
                    "插件包解压后超过 {} MiB",
                    limits.max_uncompressed_bytes / 1024 / 1024
                )));
            }
            let output = staging_dir.join(enclosed);
            if entry.is_dir() {
                fs::create_dir_all(&output)?;
                continue;
            }
            file_count += 1;
            if normalized == "manifest.json" || normalized.ends_with("/manifest.json") {
                manifest_paths.push(normalized.clone());
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output_file = File::create(&output)?;
            std::io::copy(&mut entry, &mut output_file)?;
            output_file.flush()?;
        }
        Ok(())
    })();
    if let Err(error) = extraction {
        fs::remove_dir_all(&staging_dir).ok();
        return Err(error);
    }

    let (root_prefix, plugin_root) = locate_plugin_root(&staging_dir, &manifest_paths)?;
    Ok(ExtractedArchive {
        staging_dir,
        plugin_root,
        root_prefix,
        file_count,
        uncompressed_bytes: total,
    })
}

fn locate_plugin_root(
    staging_dir: &Path,
    manifest_paths: &[String],
) -> Result<(Option<String>, PathBuf), AppError> {
    let root_manifests = manifest_paths
        .iter()
        .filter(|path| path.as_str() == "manifest.json")
        .collect::<Vec<_>>();
    if root_manifests.len() == 1 {
        if manifest_paths.len() != 1 {
            return Err(AppError::InvalidInput(
                "插件包中只能有一个 manifest.json".into(),
            ));
        }
        return Ok((None, staging_dir.to_path_buf()));
    }
    let nested = manifest_paths
        .iter()
        .filter_map(|path| {
            let parts = path.split('/').collect::<Vec<_>>();
            (parts.len() == 2 && parts[1] == "manifest.json").then(|| parts[0].to_string())
        })
        .collect::<Vec<_>>();
    if nested.len() != 1 || manifest_paths.len() != 1 {
        return Err(AppError::InvalidInput(
            "manifest.json 必须位于压缩包根目录或唯一的第一层目录中".into(),
        ));
    }
    let prefix = nested[0].clone();
    for entry in fs::read_dir(staging_dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy() != prefix {
            return Err(AppError::InvalidInput(
                "使用第一层插件目录时，压缩包根目录不能包含其他文件".into(),
            ));
        }
    }
    Ok((Some(prefix.clone()), staging_dir.join(prefix)))
}

fn read_manifest_v3(plugin_root: &Path) -> Result<PluginManifestV3, AppError> {
    let path = plugin_root.join("manifest.json");
    let bytes = fs::read(&path)?;
    if bytes.len() > 1024 * 1024 {
        return Err(AppError::InvalidInput("manifest.json 超过 1 MiB".into()));
    }
    let raw_manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::InvalidInput(format!("Manifest v3 解析失败：{}", error)))?;
    if let Some(path) = find_forbidden_secret_field(&raw_manifest, "manifest") {
        return Err(AppError::InvalidInput(format!(
            "Manifest 不得声明或收集密钥明文字段：{}；请改用 credentialId",
            path
        )));
    }
    let manifest: PluginManifestV3 = serde_json::from_value(raw_manifest)
        .map_err(|error| AppError::InvalidInput(format!("Manifest v3 字段无效：{}", error)))?;
    if manifest.schema_version != 3 {
        return Err(AppError::InvalidInput(format!(
            "正式插件包仅接受 Manifest v3，收到 v{}",
            manifest.schema_version
        )));
    }
    Ok(manifest)
}

fn find_forbidden_secret_field(value: &serde_json::Value, path: &str) -> Option<String> {
    match value {
        serde_json::Value::Object(fields) => fields.iter().find_map(|(key, child)| {
            if is_forbidden_secret_field_name(key) {
                return Some(format!("{}.{}", path, key));
            }
            find_forbidden_secret_field(child, &format!("{}.{}", path, key))
        }),
        serde_json::Value::Array(items) => items.iter().enumerate().find_map(|(index, child)| {
            find_forbidden_secret_field(child, &format!("{}[{}]", path, index))
        }),
        _ => None,
    }
}

fn is_forbidden_secret_field_name(value: &str) -> bool {
    let normalized = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "apikey"
            | "apisecret"
            | "accesstoken"
            | "refreshtoken"
            | "bearertoken"
            | "authorization"
            | "clientsecret"
            | "password"
    )
}

fn validate_manifest(manifest: &PluginManifestV3) -> Result<(), AppError> {
    validate_plugin_id(&manifest.id)?;
    validate_version(&manifest.version)?;
    if manifest.name.trim().is_empty() || manifest.author_id.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Manifest 的 name 与 authorId 不能为空".into(),
        ));
    }
    if manifest.supported_scenes.is_empty() {
        return Err(AppError::InvalidInput(
            "Manifest v3 必须声明 supportedScenes".into(),
        ));
    }
    let permissions = manifest.permissions.iter().collect::<BTreeSet<_>>();
    if permissions.len() != manifest.permissions.len() {
        return Err(AppError::InvalidInput("Manifest 权限列表包含重复项".into()));
    }
    for permission in &manifest.permissions {
        if permission.is_empty()
            || permission.len() > 128
            || !permission.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-')
            })
        {
            return Err(AppError::InvalidInput(format!(
                "非法权限名称：{}",
                permission
            )));
        }
        if !crate::services::plugin_capabilities::V3_MANIFEST_PERMISSIONS
            .contains(&permission.as_str())
        {
            return Err(AppError::InvalidInput(format!(
                "Manifest 申请了不存在的权限：{}",
                permission
            )));
        }
        if !crate::services::plugin_capabilities::is_v3_permission_runtime_allowed(
            permission,
            runtime_kind_name(&manifest.runtime_kind),
        ) {
            return Err(AppError::InvalidInput(format!(
                "Manifest 权限 {} 不允许用于 runtimeKind {}",
                permission,
                runtime_kind_name(&manifest.runtime_kind)
            )));
        }
    }
    let mut ids = HashSet::new();
    for id in manifest
        .contributes
        .features
        .iter()
        .map(|item| item.id.as_str())
        .chain(
            manifest
                .contributes
                .agents
                .iter()
                .map(|item| item.id.as_str()),
        )
        .chain(
            manifest
                .contributes
                .commands
                .iter()
                .map(|item| item.id.as_str()),
        )
        .chain(
            manifest
                .contributes
                .views
                .iter()
                .map(|item| item.id.as_str()),
        )
        .chain(
            manifest
                .contributes
                .tools
                .iter()
                .map(|item| item.id.as_str()),
        )
        .chain(
            manifest
                .contributes
                .enhancements
                .iter()
                .map(|item| item.id.as_str()),
        )
    {
        validate_contribution_id(id)?;
        if !ids.insert(id.to_string()) {
            return Err(AppError::InvalidInput(format!("贡献点 ID 重复：{}", id)));
        }
    }
    let has_features = !manifest.contributes.features.is_empty();
    let has_enhancements = !manifest.contributes.enhancements.is_empty();
    match manifest.classification {
        crate::models::PluginClassification::Feature if !has_features => {
            return Err(AppError::InvalidInput(
                "feature 插件必须至少声明一个 features 贡献点".into(),
            ));
        }
        crate::models::PluginClassification::Enhancement if !has_enhancements => {
            return Err(AppError::InvalidInput(
                "enhancement 插件必须至少声明一个 enhancements 贡献点".into(),
            ));
        }
        crate::models::PluginClassification::Hybrid if !has_features || !has_enhancements => {
            return Err(AppError::InvalidInput(
                "hybrid 插件必须同时声明 features 和 enhancements 贡献点".into(),
            ));
        }
        _ => {}
    }
    validate_v3_capability_combination(manifest, has_features, has_enhancements)?;

    for feature in &manifest.contributes.features {
        if feature.ui_schema.is_none() {
            return Err(AppError::InvalidInput(format!(
                "feature 贡献点 {} 必须声明 uiSchema",
                feature.id
            )));
        }
        if let Some(handler) = &feature.handler {
            validate_declarative_handler(&feature.id, handler)?;
        }
    }
    for agent in &manifest.contributes.agents {
        if let Some(handler) = &agent.handler {
            validate_declarative_handler(&agent.id, handler)?;
        }
    }
    for tool in &manifest.contributes.tools {
        if let Some(handler) = &tool.handler {
            validate_declarative_handler(&tool.id, handler)?;
        }
    }
    for enhancement in &manifest.contributes.enhancements {
        validate_declarative_handler(&enhancement.id, &enhancement.handler)?;
    }
    Ok(())
}

fn validate_v3_capability_combination(
    manifest: &PluginManifestV3,
    has_features: bool,
    has_enhancements: bool,
) -> Result<(), AppError> {
    use crate::models::PluginClassification;

    match manifest.classification {
        PluginClassification::Feature if has_enhancements => {
            return Err(AppError::InvalidInput(
                "classification=feature 不得声明 enhancement contribution".into(),
            ));
        }
        PluginClassification::Enhancement if has_features => {
            return Err(AppError::InvalidInput(
                "classification=enhancement 不得声明 feature contribution".into(),
            ));
        }
        _ => {}
    }
    let runtime = runtime_kind_name(&manifest.runtime_kind);
    if !matches!(
        (runtime, &manifest.classification),
        ("declarative-ui", PluginClassification::Feature)
            | ("prompt-pack", PluginClassification::Enhancement)
            | (
                "xingchen-agent" | "xingchen-workflow",
                PluginClassification::Feature | PluginClassification::Hybrid
            )
    ) {
        return Err(AppError::InvalidInput(format!(
            "runtimeKind {} 与 classification/contribution 组合不兼容",
            runtime
        )));
    }
    let permissions = manifest
        .permissions
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if has_enhancements && !permissions.contains("ai.context.augment") {
        return Err(AppError::InvalidInput(
            "包含 enhancement contribution 的 Manifest 必须声明 ai.context.augment".into(),
        ));
    }
    if has_features && matches!(runtime, "xingchen-agent" | "xingchen-workflow") {
        for required in [
            "credentials.use",
            "agents.invoke",
            "network.xingchen",
            "ai.invoke",
        ] {
            if !permissions.contains(required) {
                return Err(AppError::InvalidInput(format!(
                    "Xingchen feature 缺少必需权限 {}",
                    required
                )));
            }
        }
    }
    if manifest.contributes.features.iter().any(|feature| {
        feature
            .capabilities
            .contains(&crate::models::PluginCapability::FileDocxOutput)
    }) && !permissions.contains("files.writeSelected")
    {
        return Err(AppError::InvalidInput(
            "feature capability file.docx.output 必须声明 files.writeSelected".into(),
        ));
    }
    Ok(())
}

fn runtime_kind_name(runtime_kind: &PluginRuntimeKind) -> &'static str {
    match runtime_kind {
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

fn validate_declarative_handler(
    contribution_id: &str,
    handler: &crate::models::PluginDeclarativeHandler,
) -> Result<(), AppError> {
    if handler.kind != "declarative" {
        return Err(AppError::InvalidInput(format!(
            "贡献点 {} 只允许 declarative handler，不得执行第三方代码",
            contribution_id
        )));
    }
    Ok(())
}

fn validate_manifest_resources(root: &Path, manifest: &PluginManifestV3) -> Result<(), AppError> {
    if !root.join("README.md").is_file() {
        return Err(AppError::InvalidInput(
            "正式插件包必须包含面向用户的 README.md".into(),
        ));
    }
    let resources = manifest
        .contributes
        .features
        .iter()
        .filter_map(|item| item.ui_schema.as_deref())
        .chain(manifest.contributes.features.iter().filter_map(|item| {
            item.handler
                .as_ref()
                .map(|handler| handler.resource.as_str())
        }))
        .chain(manifest.contributes.agents.iter().filter_map(|item| {
            item.handler
                .as_ref()
                .map(|handler| handler.resource.as_str())
        }))
        .chain(manifest.contributes.tools.iter().filter_map(|item| {
            item.handler
                .as_ref()
                .map(|handler| handler.resource.as_str())
        }))
        .chain(
            manifest
                .contributes
                .enhancements
                .iter()
                .map(|item| item.handler.resource.as_str()),
        );
    for resource in resources {
        let relative = Path::new(resource);
        validate_relative_path(relative, 32)?;
        let path = root.join(relative);
        if !path.is_file() {
            return Err(AppError::InvalidInput(format!(
                "Manifest 声明的资源不存在：{}",
                resource
            )));
        }
    }
    for feature in &manifest.contributes.features {
        let Some(resource) = feature.ui_schema.as_deref() else {
            continue;
        };
        let path = root.join(resource);
        let metadata = fs::metadata(&path)?;
        if metadata.len() > 1024 * 1024 {
            return Err(AppError::InvalidInput(format!(
                "uiSchema 超过 1 MiB：{}",
                resource
            )));
        }
        let schema: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)
            .map_err(|error| AppError::InvalidInput(format!("uiSchema JSON 无效：{}", error)))?;
        if let Some(secret_path) = find_forbidden_secret_field(&schema, "uiSchema") {
            return Err(AppError::InvalidInput(format!(
                "uiSchema 不得收集密钥明文字段：{}；请在 AI 资源中心绑定凭据",
                secret_path
            )));
        }
        let fields = schema
            .get("fields")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| AppError::InvalidInput("uiSchema.fields 必须是数组".into()))?;
        if fields.len() > 100 {
            return Err(AppError::InvalidInput(
                "uiSchema 字段数量不能超过 100".into(),
            ));
        }
        for field in fields {
            let key = field
                .get("key")
                .or_else(|| field.get("id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if is_forbidden_secret_field_name(key)
                || field
                    .get("sensitive")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            {
                return Err(AppError::InvalidInput(format!(
                    "uiSchema 字段 {} 不得收集凭据明文，请改为 credentialId 引用",
                    key
                )));
            }
        }
    }
    Ok(())
}

fn runtime_policy_for_manifest(manifest: &PluginManifestV3) -> PluginRuntimePolicy {
    let (can_execute, blocked_reason) = match manifest.runtime_kind {
        PluginRuntimeKind::LegacyJs => (
            false,
            Some("正式安装的插件禁止在 WebView 中执行 legacy JS".into()),
        ),
        PluginRuntimeKind::DeclarativeUi
        | PluginRuntimeKind::PromptPack
        | PluginRuntimeKind::XingchenAgent
        | PluginRuntimeKind::XingchenWorkflow => (true, None),
        _ => (false, Some("该运行时尚未接入正式插件 Runtime Host".into())),
    };
    PluginRuntimePolicy {
        plugin_id: manifest.id.clone(),
        runtime_kind: manifest.runtime_kind.clone(),
        source: manifest.source.clone(),
        can_execute,
        raw_invoke_allowed: false,
        blocked_reason,
    }
}

fn scan_package_security(root: &Path) -> Result<(), AppError> {
    const CODE_EXTENSIONS: &[&str] = &[
        "js", "mjs", "cjs", "py", "ps1", "bat", "cmd", "exe", "dll", "so", "dylib",
    ];
    const PRIVATE_EXTENSIONS: &[&str] = &["pem", "p12", "pfx", "key"];
    const TEXT_EXTENSIONS: &[&str] = &[
        "json", "md", "markdown", "txt", "yaml", "yml", "toml", "xml", "csv",
    ];

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| AppError::InvalidInput(error.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name == ".env"
            || name.starts_with(".env.")
            || name == "id_rsa"
            || name == "id_ed25519"
            || PRIVATE_EXTENSIONS.contains(&extension.as_str())
        {
            return Err(AppError::InvalidInput(format!(
                "插件包包含禁止分发的凭据文件：{}",
                path.strip_prefix(root).unwrap_or(path).display()
            )));
        }
        if CODE_EXTENSIONS.contains(&extension.as_str()) {
            return Err(AppError::InvalidInput(format!(
                "正式插件包不允许携带可执行脚本或二进制：{}",
                path.strip_prefix(root).unwrap_or(path).display()
            )));
        }
        if !TEXT_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| AppError::InvalidInput(error.to_string()))?;
        if metadata.len() > 1024 * 1024 {
            continue;
        }
        let text = fs::read_to_string(path)?;
        if contains_likely_secret(&text) {
            return Err(AppError::InvalidInput(format!(
                "插件包资源疑似包含 API Key、Secret、Token 或 Authorization 明文：{}",
                path.strip_prefix(root).unwrap_or(path).display()
            )));
        }
    }
    Ok(())
}

fn contains_likely_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("authorization: bearer ")
    {
        return true;
    }
    for marker in [
        "api_key",
        "apikey",
        "api-secret",
        "api_secret",
        "apisecret",
        "access_token",
        "refresh_token",
        "bearer_token",
        "client_secret",
    ] {
        let mut rest = lower.as_str();
        while let Some(index) = rest.find(marker) {
            let tail = &rest[index + marker.len()..];
            let Some(separator) = tail.find(|character: char| character == ':' || character == '=')
            else {
                break;
            };
            if separator > 32 {
                rest = tail.get(1..).unwrap_or_default();
                continue;
            }
            let after_separator = &tail[separator + 1..];
            let value = after_separator
                .trim_start_matches(|character: char| {
                    character.is_whitespace() || matches!(character, '\"' | '\'')
                })
                .split(|character: char| {
                    character.is_whitespace() || matches!(character, '\"' | '\'' | ',' | '}')
                })
                .next()
                .unwrap_or_default();
            if !value.is_empty()
                && value.len() >= 8
                && !value.contains("${")
                && !value.starts_with("your_")
                && !value.starts_with("replace_")
                && !value.starts_with("example")
                && !value.starts_with("placeholder")
                && !value.starts_with("***")
                && !value.starts_with('<')
            {
                return true;
            }
            rest = after_separator;
        }
    }
    false
}

fn order_enhancements(
    mut items: Vec<ResolvedEnhancementContribution>,
    warnings: &mut Vec<String>,
) -> Vec<ResolvedEnhancementContribution> {
    items.sort_by(|left, right| {
        right
            .contribution
            .priority
            .cmp(&left.contribution.priority)
            .then_with(|| left.plugin_id.cmp(&right.plugin_id))
            .then_with(|| left.contribution.id.cmp(&right.contribution.id))
    });
    let exclusive_winners = items
        .iter()
        .filter(|item| item.contribution.mode == PluginExecutionMode::Exclusive)
        .fold(HashMap::<String, String>::new(), |mut winners, item| {
            winners
                .entry(format!("{:?}", item.contribution.hook))
                .or_insert_with(|| format!("{}:{}", item.plugin_id, item.contribution.id));
            winners
        });
    items.retain(|item| {
        let hook = format!("{:?}", item.contribution.hook);
        if let Some(winner) = exclusive_winners.get(&hook) {
            let key = format!("{}:{}", item.plugin_id, item.contribution.id);
            if &key != winner {
                warnings.push(format!("独占增强 {} 生效，已跳过 {}", winner, key));
                return false;
            }
        }
        true
    });

    let keys = items
        .iter()
        .map(|item| format!("{}:{}", item.plugin_id, item.contribution.id))
        .collect::<Vec<_>>();
    let mut bare = HashMap::<String, Vec<usize>>::new();
    for (index, item) in items.iter().enumerate() {
        bare.entry(item.contribution.id.clone())
            .or_default()
            .push(index);
    }
    let mut edges = vec![BTreeSet::<usize>::new(); items.len()];
    let mut indegree = vec![0usize; items.len()];
    let resolve = |value: &str| -> Option<usize> {
        keys.iter().position(|key| key == value).or_else(|| {
            bare.get(value)
                .filter(|values| values.len() == 1)
                .map(|values| values[0])
        })
    };
    for (index, item) in items.iter().enumerate() {
        for target in &item.contribution.runs_before {
            if let Some(target_index) = resolve(target) {
                if edges[index].insert(target_index) {
                    indegree[target_index] += 1;
                }
            }
        }
        for dependency in &item.contribution.runs_after {
            if let Some(dependency_index) = resolve(dependency) {
                if edges[dependency_index].insert(index) {
                    indegree[index] += 1;
                }
            }
        }
    }
    let mut queue = VecDeque::new();
    for (index, value) in indegree.iter().enumerate() {
        if *value == 0 {
            queue.push_back(index);
        }
    }
    let mut ordered = Vec::new();
    while let Some(index) = queue.pop_front() {
        ordered.push(index);
        for target in edges[index].iter().copied() {
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    if ordered.len() != items.len() {
        let cyclic = indegree
            .iter()
            .enumerate()
            .filter_map(|(index, value)| (*value > 0).then(|| keys[index].clone()))
            .collect::<Vec<_>>();
        warnings.push(format!(
            "增强贡献存在循环依赖，已跳过：{}",
            cyclic.join("、")
        ));
    }
    ordered
        .into_iter()
        .map(|index| items[index].clone())
        .collect()
}

fn scene_matches(scenes: &[PluginScene], scene: &PluginScene) -> bool {
    scenes.is_empty() || scenes.contains(scene) || scenes.contains(&PluginScene::Global)
}

fn append_enhancement_guard_warnings(
    warnings: &mut Vec<String>,
    plugin_id: &str,
    contribution_ids: &[String],
    error: &AppError,
) {
    for contribution_id in contribution_ids {
        let warning = match error {
            AppError::PluginCapabilityNotDeclared { .. } => format!(
                "插件 {} 的增强贡献 {} 未在 Manifest 声明 ai.context.augment，已跳过",
                plugin_id, contribution_id
            ),
            AppError::PluginPermissionDenied { .. } => format!(
                "插件 {} 的增强贡献 {} 未获 ai.context.augment 授权或授权已撤销，已跳过",
                plugin_id, contribution_id
            ),
            _ => format!(
                "插件 {} 的增强贡献 {} 因插件生命周期状态不允许调用而跳过：{}",
                plugin_id, contribution_id, error
            ),
        };
        warnings.push(warning);
    }
}

fn ensure_plugins_dir(data_dir: &Path) -> Result<PathBuf, AppError> {
    let path = data_dir.join("plugins");
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn write_current_pointer(plugin_dir: &Path, version: &str, hash: &str) -> Result<(), AppError> {
    fs::create_dir_all(plugin_dir)?;
    let target = plugin_dir.join("current.json");
    let temporary = plugin_dir.join(format!(".current-{}.json", Uuid::new_v4()));
    let value = serde_json::json!({ "version": version, "contentHash": hash });
    fs::write(&temporary, serde_json::to_vec_pretty(&value)?)?;
    if target.exists() {
        fs::remove_file(&target)?;
    }
    fs::rename(&temporary, &target)?;
    Ok(())
}

fn validate_archive_extension(path: &Path) -> Result<(), AppError> {
    if !path.is_file() {
        return Err(AppError::NotFound(format!(
            "插件包不存在：{}",
            path.display()
        )));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case(ARCHIVE_EXTENSION) {
        return Err(AppError::InvalidInput(
            "正式安装只接受 .firstwork-plugin 文件".into(),
        ));
    }
    Ok(())
}

fn validate_relative_path(path: &Path, max_depth: usize) -> Result<(), AppError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(AppError::InvalidInput("插件资源路径必须是相对路径".into()));
    }
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => depth += 1,
            _ => {
                return Err(AppError::InvalidInput(format!(
                    "插件资源路径包含非法组件：{}",
                    path.display()
                )));
            }
        }
    }
    if depth == 0 || depth > max_depth {
        return Err(AppError::InvalidInput(format!(
            "插件资源路径深度超过限制 {}",
            max_depth
        )));
    }
    Ok(())
}

fn normalize_relative_path(path: &Path) -> Result<String, AppError> {
    validate_relative_path(path, usize::MAX)?;
    Ok(path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/"))
}

fn has_windows_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || value.starts_with("//")
}

fn hash_file(path: &Path) -> Result<String, AppError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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

fn validate_contribution_id(value: &str) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(AppError::InvalidInput(format!("非法贡献点 ID：{}", value)));
    }
    Ok(())
}

fn validate_version(value: &str) -> Result<(), AppError> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || part.parse::<u64>().is_err())
    {
        return Err(AppError::InvalidInput(format!(
            "插件版本必须使用 x.y.z：{}",
            value
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        PluginDeclarativeHandler, PluginEnhancementContribution, PluginEnhancementHook,
        PluginExecutionMode,
    };

    #[test]
    fn plugin_id_validation_matches_packaging_and_runtime_rules() {
        assert!(validate_plugin_id("com.pomegranate.demo.document-summary").is_ok());
        assert!(validate_plugin_id("plugin_2").is_ok());
        assert!(validate_plugin_id("Uppercase").is_err());
        assert!(validate_plugin_id(".hidden").is_err());
        assert!(validate_plugin_id("plugin/path").is_err());
    }
    use zip::write::SimpleFileOptions;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("firstwork-v3-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create temp directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn test_archive(entries: &[(&str, &[u8], Option<u32>)]) -> (TestDir, PathBuf) {
        let directory = TestDir::new();
        let archive_path = directory.path().join("test.firstwork-plugin");
        let file = File::create(&archive_path).expect("create archive");
        let mut archive = zip::ZipWriter::new(file);
        for (name, content, mode) in entries {
            let options = mode
                .map(|value| SimpleFileOptions::default().unix_permissions(value))
                .unwrap_or_default();
            archive.start_file(*name, options).expect("start entry");
            archive.write_all(content).expect("write entry");
        }
        archive.finish().expect("finish archive");
        // zip writer 会把 unix_permissions 限制为权限位；测试需要显式把中央目录
        // 的“创建平台”和文件类型位改成 Unix 符号链接。
        if let Some(mode) = entries.iter().find_map(|(_, _, mode)| *mode) {
            let mut bytes = fs::read(&archive_path).expect("read archive");
            let central = bytes
                .windows(4)
                .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
                .expect("find central directory");
            bytes[central + 5] = 3;
            bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
            fs::write(&archive_path, bytes).expect("patch unix mode");
        }
        (directory, archive_path)
    }

    #[test]
    fn rejects_parent_and_absolute_paths() {
        assert!(validate_relative_path(Path::new("../manifest.json"), 8).is_err());
        assert!(validate_relative_path(Path::new("C:/manifest.json"), 8).is_err());
        assert!(has_windows_prefix("C:/manifest.json"));
    }

    #[test]
    fn detects_ordering_cycle_and_drops_cyclic_items() {
        let item = |id: &str, after: &str| ResolvedEnhancementContribution {
            plugin_id: "demo.plugin".into(),
            contribution: PluginEnhancementContribution {
                id: id.into(),
                title: id.into(),
                hook: PluginEnhancementHook::PromptEnhancer,
                scenes: vec![PluginScene::Learning],
                features: vec![],
                priority: 0,
                mode: PluginExecutionMode::Append,
                runs_before: vec![],
                runs_after: vec![after.into()],
                handler: PluginDeclarativeHandler {
                    kind: "declarative".into(),
                    resource: format!("prompts/{}.md", id),
                },
            },
            resource_path: String::new(),
        };
        let mut warnings = Vec::new();
        let result = order_enhancements(vec![item("a", "b"), item("b", "a")], &mut warnings);
        assert!(result.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn rejects_zip_slip_during_extraction() {
        let (directory, archive) = test_archive(&[("../manifest.json", b"{}", None)]);
        let result =
            extract_archive_safely(directory.path(), &archive, &PluginArchiveLimits::default());
        assert!(result.is_err());
        assert!(!directory.path().join("manifest.json").exists());
    }

    #[test]
    fn rejects_symlink_and_size_limits() {
        let (directory, symlink_archive) =
            test_archive(&[("manifest.json", b"target", Some(0o120777))]);
        assert!(extract_archive_safely(
            directory.path(),
            &symlink_archive,
            &PluginArchiveLimits::default(),
        )
        .is_err());

        let (directory, large_archive) = test_archive(&[("manifest.json", b"12345", None)]);
        let limits = PluginArchiveLimits {
            max_file_bytes: 4,
            ..PluginArchiveLimits::default()
        };
        assert!(extract_archive_safely(directory.path(), &large_archive, &limits).is_err());
    }

    #[test]
    fn parses_manifest_v3_with_object_dependencies() {
        let directory = TestDir::new();
        fs::write(
            directory.path().join("manifest.json"),
            r#"{
                "schemaVersion": 3,
                "id": "com.firstwork.test",
                "name": "Test",
                "version": "1.0.0",
                "authorId": "tester",
                "classification": "feature",
                "runtimeKind": "declarative-ui",
                "supportedScenes": ["global"],
                "dependencies": {"com.firstwork.base": "^1.0.0"},
                "contributes": {}
            }"#,
        )
        .expect("write manifest");
        let manifest = read_manifest_v3(directory.path()).expect("parse v3 manifest");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].id, "com.firstwork.base");
        assert_eq!(manifest.dependencies[0].version.as_deref(), Some("^1.0.0"));
    }

    #[test]
    fn exclusive_contribution_is_the_only_hook_winner() {
        let item =
            |id: &str, mode: PluginExecutionMode, priority: i32| ResolvedEnhancementContribution {
                plugin_id: "demo.plugin".into(),
                contribution: PluginEnhancementContribution {
                    id: id.into(),
                    title: id.into(),
                    hook: PluginEnhancementHook::PromptEnhancer,
                    scenes: vec![PluginScene::Learning],
                    features: vec![],
                    priority,
                    mode,
                    runs_before: vec![],
                    runs_after: vec![],
                    handler: PluginDeclarativeHandler {
                        kind: "declarative".into(),
                        resource: format!("prompts/{}.md", id),
                    },
                },
                resource_path: String::new(),
            };
        let mut warnings = Vec::new();
        let ordered = order_enhancements(
            vec![
                item("append", PluginExecutionMode::Append, 200),
                item("exclusive", PluginExecutionMode::Exclusive, 100),
            ],
            &mut warnings,
        );
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].contribution.id, "exclusive");
        assert_eq!(warnings.len(), 1);
    }

    fn manifest_for(classification: &str, runtime_kind: &str) -> PluginManifestV3 {
        let mut manifest: PluginManifestV3 = serde_json::from_value(serde_json::json!({
            "schemaVersion": 3,
            "id": "com.firstwork.platform-test",
            "name": "Platform Test",
            "version": "1.0.0",
            "authorId": "firstwork-tests",
            "classification": classification,
            "runtimeKind": runtime_kind,
            "source": "local",
            "supportedScenes": ["global"],
            "permissions": [],
            "contributes": {
                "features": [{
                    "id": "test-feature",
                    "title": "Test Feature",
                    "scenes": ["global"],
                    "uiSchema": "ui.json"
                }],
                "enhancements": [{
                    "id": "test-enhancement",
                    "title": "Test Enhancement",
                    "hook": "promptEnhancer",
                    "scenes": ["global"],
                    "handler": {"kind": "declarative", "resource": "prompt.md"}
                }]
            }
        }))
        .expect("parse test manifest");
        if matches!(classification, "enhancement" | "hybrid") {
            manifest.permissions.push("ai.context.augment".into());
        }
        if matches!(runtime_kind, "xingchen-agent" | "xingchen-workflow")
            && matches!(classification, "feature" | "hybrid")
        {
            manifest.permissions.extend(
                [
                    "credentials.use",
                    "agents.invoke",
                    "network.xingchen",
                    "ai.invoke",
                ]
                .into_iter()
                .map(str::to_string),
            );
        }
        manifest
    }

    fn execution_context() -> PluginExecutionContext {
        serde_json::from_value(serde_json::json!({
            "scene": "global",
            "feature": "test-feature",
            "requestId": "a2-permission-test"
        }))
        .expect("parse execution context")
    }

    fn install_test_manifest(
        db: &Database,
        directory: &TestDir,
        manifest: &PluginManifestV3,
        approved_permissions: &[String],
        output_kind: &str,
    ) {
        fs::write(directory.path().join("prompt.md"), "safe enhancement")
            .expect("write enhancement resource");
        fs::write(
            directory.path().join("ui.json"),
            serde_json::json!({"output": {"kind": output_kind}}).to_string(),
        )
        .expect("write feature schema");
        let hash = PluginService::calculate_integrity_for_path(directory.path())
            .expect("calculate test integrity");
        db.record_plugin_version(
            manifest,
            &directory.path().to_string_lossy(),
            &hash,
            approved_permissions,
        )
        .expect("record test plugin version");
        db.set_plugin_enabled(&manifest.id, true)
            .expect("enable test plugin");
    }

    fn install_resource_manifest(
        db: &Database,
        data_dir: &TestDir,
        manifest: &PluginManifestV3,
        approved_permissions: &[String],
        prompt: &str,
        schema: &str,
    ) -> PathBuf {
        let install_path = data_dir
            .path()
            .join("plugins")
            .join(&manifest.id)
            .join(&manifest.version);
        fs::create_dir_all(&install_path).expect("create resource plugin directory");
        fs::write(install_path.join("prompt.md"), prompt).expect("write enhancement resource");
        fs::write(install_path.join("ui.json"), schema).expect("write feature schema");
        let hash = PluginService::calculate_integrity_for_path(&install_path)
            .expect("calculate resource plugin integrity");
        db.record_plugin_version(
            manifest,
            &install_path.to_string_lossy(),
            &hash,
            approved_permissions,
        )
        .expect("record resource plugin version");
        db.set_plugin_enabled(&manifest.id, true)
            .expect("enable resource plugin");
        install_path
    }

    fn enhancement_manifest() -> PluginManifestV3 {
        let mut manifest = manifest_for("enhancement", "prompt-pack");
        manifest.contributes.features.clear();
        manifest.permissions = vec!["ai.context.augment".into()];
        manifest.default_activation.global = true;
        manifest
    }

    fn feature_manifest(output_kind: &str) -> PluginManifestV3 {
        let mut manifest = manifest_for("feature", "xingchen-workflow");
        manifest.contributes.enhancements.clear();
        manifest.default_activation.global = true;
        manifest.permissions = vec![
            "credentials.use".into(),
            "agents.invoke".into(),
            "network.xingchen".into(),
            "ai.invoke".into(),
        ];
        if matches!(output_kind, "docx-base64" | "file-base64") {
            manifest.permissions.push("files.writeSelected".into());
        }
        manifest
    }

    #[test]
    fn enhancement_resource_read_rechecks_declaration_and_current_grant() {
        let cases = [
            (true, Some(true), true),
            (true, Some(false), false),
            (true, None, false),
            (false, Some(true), false),
        ];
        for (declared, grant_state, expected_success) in cases {
            let db = Database::init(":memory:").expect("create in-memory database");
            let data_dir = TestDir::new();
            let mut manifest = enhancement_manifest();
            if !declared {
                manifest.permissions.clear();
            }
            let approved = if declared && grant_state == Some(true) {
                manifest.permissions.clone()
            } else {
                Vec::new()
            };
            install_resource_manifest(
                &db,
                &data_dir,
                &manifest,
                &approved,
                "current enhancement",
                "{}",
            );
            let conn = db.conn_lock().expect("lock test database");
            if grant_state.is_none() {
                conn.execute(
                    "DELETE FROM plugin_permissions
                     WHERE plugin_id = ?1 AND permission = 'ai.context.augment'",
                    [&manifest.id],
                )
                .expect("remove grant row");
            } else if !declared && grant_state == Some(true) {
                conn.execute(
                    "INSERT INTO plugin_permissions (plugin_id, permission, granted)
                     VALUES (?1, 'ai.context.augment', 1)",
                    [&manifest.id],
                )
                .expect("insert undeclared grant");
            }
            drop(conn);

            let result = PluginPlatformService::read_enhancement_resource(
                &db,
                data_dir.path(),
                &manifest.id,
                "test-enhancement",
            );
            assert_eq!(result.is_ok(), expected_success);
        }
    }

    #[test]
    fn enhancement_resource_read_uses_only_current_manifest_and_install_path() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = TestDir::new();
        let old = enhancement_manifest();
        install_resource_manifest(
            &db,
            &data_dir,
            &old,
            &old.permissions,
            "old enhancement",
            "{}",
        );

        let mut current = old.clone();
        current.version = "2.0.0".into();
        current.contributes.enhancements[0].id = "current-enhancement".into();
        install_resource_manifest(
            &db,
            &data_dir,
            &current,
            &current.permissions,
            "current enhancement",
            "{}",
        );

        let content = PluginPlatformService::read_enhancement_resource(
            &db,
            data_dir.path(),
            &current.id,
            "current-enhancement",
        )
        .expect("read current enhancement");
        assert_eq!(content, "current enhancement");
        assert!(PluginPlatformService::read_enhancement_resource(
            &db,
            data_dir.path(),
            &current.id,
            "test-enhancement",
        )
        .is_err());
    }

    #[test]
    fn declared_resource_reads_reject_path_escape() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = TestDir::new();
        let mut enhancement = enhancement_manifest();
        enhancement.contributes.enhancements[0].handler.resource = "../outside.txt".into();
        install_resource_manifest(
            &db,
            &data_dir,
            &enhancement,
            &enhancement.permissions,
            "unused",
            "{}",
        );
        fs::write(
            data_dir.path().join("plugins").join("outside.txt"),
            "outside",
        )
        .expect("write outside resource");
        assert!(PluginPlatformService::read_enhancement_resource(
            &db,
            data_dir.path(),
            &enhancement.id,
            "test-enhancement",
        )
        .is_err());

        let mut feature = feature_manifest("text");
        feature.id = "com.firstwork.feature-path-test".into();
        feature.contributes.features[0].ui_schema = Some("../outside.json".into());
        install_resource_manifest(&db, &data_dir, &feature, &[], "unused", "{}");
        assert!(PluginPlatformService::read_feature_ui_schema(
            &db,
            data_dir.path(),
            &feature.id,
            "test-feature",
        )
        .is_err());
    }

    #[test]
    fn feature_ui_schema_uses_current_context_without_execution_grants() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let data_dir = TestDir::new();
        let old = feature_manifest("text");
        install_resource_manifest(&db, &data_dir, &old, &[], "unused", r#"{"version":"old"}"#);
        let mut current = old.clone();
        current.version = "2.0.0".into();
        current.contributes.features[0].id = "current-feature".into();
        install_resource_manifest(
            &db,
            &data_dir,
            &current,
            &[],
            "unused",
            r#"{"version":"current"}"#,
        );

        let content = PluginPlatformService::read_feature_ui_schema(
            &db,
            data_dir.path(),
            &current.id,
            "current-feature",
        )
        .expect("read current feature schema without execution grants");
        assert_eq!(content, r#"{"version":"current"}"#);
        assert!(PluginPlatformService::read_feature_ui_schema(
            &db,
            data_dir.path(),
            &current.id,
            "test-feature",
        )
        .is_err());
    }

    #[test]
    fn enhancement_requires_manifest_declaration_and_current_user_grant() {
        let cases = [
            (true, Some(true), true, None),
            (true, Some(false), false, Some("未获")),
            (true, None, false, Some("未获")),
            (false, Some(true), false, Some("未在 Manifest 声明")),
        ];
        for (declared, grant_state, expected_resolved, warning_fragment) in cases {
            let db = Database::init(":memory:").expect("create in-memory database");
            let directory = TestDir::new();
            let mut manifest = enhancement_manifest();
            if !declared {
                manifest.permissions.clear();
            }
            let approved = if grant_state == Some(true) && declared {
                manifest.permissions.clone()
            } else {
                Vec::new()
            };
            install_test_manifest(&db, &directory, &manifest, &approved, "text");
            if !declared && grant_state == Some(true) {
                let conn = db.conn_lock().expect("lock test database");
                conn.execute(
                    "INSERT INTO plugin_permissions (plugin_id, permission, granted)
                     VALUES (?1, 'ai.context.augment', 1)",
                    [&manifest.id],
                )
                .expect("insert undeclared grant");
            } else if grant_state.is_none() {
                let conn = db.conn_lock().expect("lock test database");
                conn.execute(
                    "DELETE FROM plugin_permissions
                     WHERE plugin_id = ?1 AND permission = 'ai.context.augment'",
                    [&manifest.id],
                )
                .expect("remove grant row");
            }

            let resolved =
                PluginPlatformService::resolve_enabled_contributions(&db, execution_context())
                    .expect("resolve contributions");
            assert_eq!(resolved.enhancements.len() == 1, expected_resolved);
            if let Some(fragment) = warning_fragment {
                assert!(
                    resolved
                        .warnings
                        .iter()
                        .any(|warning| warning.contains(fragment)),
                    "expected warning containing {fragment}: {:?}",
                    resolved.warnings
                );
            }
        }
    }

    #[test]
    fn enhancement_lifecycle_rejection_warns_without_blocking_other_plugins() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let rejected_directory = TestDir::new();
        let allowed_directory = TestDir::new();
        let mut rejected = enhancement_manifest();
        rejected.id = "com.firstwork.rejected-enhancement".into();
        let mut allowed = enhancement_manifest();
        allowed.id = "com.firstwork.allowed-enhancement".into();
        install_test_manifest(
            &db,
            &rejected_directory,
            &rejected,
            &rejected.permissions,
            "text",
        );
        install_test_manifest(
            &db,
            &allowed_directory,
            &allowed,
            &allowed.permissions,
            "text",
        );
        db.set_plugin_enabled(&rejected.id, false)
            .expect("disable rejected plugin");

        let resolved =
            PluginPlatformService::resolve_enabled_contributions(&db, execution_context())
                .expect("resolve contributions");
        assert_eq!(resolved.enhancements.len(), 1);
        assert_eq!(resolved.enhancements[0].plugin_id, allowed.id);
        assert!(resolved.warnings.iter().any(|warning| {
            warning.contains(&rejected.id) && warning.contains("生命周期状态")
        }));
    }

    #[test]
    fn enhancement_uses_current_guard_manifest_and_install_path() {
        let db = Database::init(":memory:").expect("create in-memory database");
        let old_directory = TestDir::new();
        let current_directory = TestDir::new();
        let old = enhancement_manifest();
        install_test_manifest(&db, &old_directory, &old, &old.permissions, "text");

        let mut current = old.clone();
        current.version = "2.0.0".into();
        current.contributes.enhancements[0].id = "current-enhancement".into();
        current.contributes.enhancements[0].handler.resource = "current-prompt.md".into();
        fs::write(
            current_directory.path().join("current-prompt.md"),
            "current enhancement",
        )
        .expect("write current resource");
        let hash = PluginService::calculate_integrity_for_path(current_directory.path())
            .expect("calculate current integrity");
        db.record_plugin_version(
            &current,
            &current_directory.path().to_string_lossy(),
            &hash,
            &current.permissions,
        )
        .expect("record current version");

        let resolved =
            PluginPlatformService::resolve_enabled_contributions(&db, execution_context())
                .expect("resolve contributions");
        assert_eq!(resolved.enhancements.len(), 1);
        let enhancement = &resolved.enhancements[0];
        assert_eq!(enhancement.contribution.id, "current-enhancement");
        assert_eq!(
            enhancement.resource_path,
            current_directory
                .path()
                .join("current-prompt.md")
                .to_string_lossy()
        );
    }

    #[test]
    fn feature_capabilities_distinguish_revoked_missing_and_undeclared() {
        for capability in [
            "credentials.use",
            "agents.invoke",
            "network.xingchen",
            "ai.invoke",
            "files.writeSelected",
        ] {
            let output_kind = if capability == "files.writeSelected" {
                "file-base64"
            } else {
                "text"
            };
            for state in ["granted", "revoked", "missing", "undeclared"] {
                let db = Database::init(":memory:").expect("create in-memory database");
                let directory = TestDir::new();
                let mut manifest = feature_manifest(output_kind);
                if state == "undeclared" {
                    manifest
                        .permissions
                        .retain(|permission| permission != capability);
                }
                let approved = manifest
                    .permissions
                    .iter()
                    .filter(|permission| state == "granted" || permission.as_str() != capability)
                    .cloned()
                    .collect::<Vec<_>>();
                install_test_manifest(&db, &directory, &manifest, &approved, output_kind);
                if state == "missing" {
                    let conn = db.conn_lock().expect("lock test database");
                    conn.execute(
                        "DELETE FROM plugin_permissions
                         WHERE plugin_id = ?1 AND permission = ?2",
                        rusqlite::params![manifest.id, capability],
                    )
                    .expect("remove grant row");
                } else if state == "undeclared" {
                    let conn = db.conn_lock().expect("lock test database");
                    conn.execute(
                        "INSERT INTO plugin_permissions (plugin_id, permission, granted)
                         VALUES (?1, ?2, 1)",
                        rusqlite::params![manifest.id, capability],
                    )
                    .expect("insert undeclared grant");
                }

                let result = PluginPlatformService::prepare_feature_invocation(
                    &db,
                    &manifest.id,
                    "test-feature",
                );
                match state {
                    "granted" => assert!(result.is_ok(), "{capability} should be allowed"),
                    "revoked" | "missing" => assert!(
                        matches!(result, Err(AppError::PluginPermissionDenied { .. })),
                        "{capability} {state} should be permission denied: {result:?}"
                    ),
                    "undeclared" => assert!(
                        matches!(result, Err(AppError::PluginCapabilityNotDeclared { .. })),
                        "{capability} should be reported as undeclared: {result:?}"
                    ),
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn feature_invocation_reports_missing_plugin_and_current_version() {
        let db = Database::init(":memory:").expect("create in-memory database");
        assert!(matches!(
            PluginPlatformService::prepare_feature_invocation(
                &db,
                "com.firstwork.missing-plugin",
                "test-feature"
            ),
            Err(AppError::NotFound(_))
        ));

        let directory = TestDir::new();
        let manifest = feature_manifest("text");
        install_test_manifest(&db, &directory, &manifest, &manifest.permissions, "text");
        {
            let conn = db.conn_lock().expect("lock test database");
            conn.execute(
                "UPDATE plugin_versions SET is_current = 0 WHERE plugin_id = ?1",
                [&manifest.id],
            )
            .expect("clear current version");
        }
        assert!(matches!(
            PluginPlatformService::prepare_feature_invocation(&db, &manifest.id, "test-feature"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn validates_feature_enhancement_and_hybrid_contracts() {
        let mut feature = manifest_for("feature", "declarative-ui");
        feature.contributes.enhancements.clear();
        assert!(validate_manifest(&feature).is_ok());
        feature.contributes.features.clear();
        assert!(validate_manifest(&feature).is_err());

        let mut enhancement = manifest_for("enhancement", "prompt-pack");
        enhancement.contributes.features.clear();
        assert!(validate_manifest(&enhancement).is_ok());
        enhancement.contributes.enhancements.clear();
        assert!(validate_manifest(&enhancement).is_err());

        let hybrid = manifest_for("hybrid", "xingchen-workflow");
        assert!(validate_manifest(&hybrid).is_ok());
        let mut incomplete_hybrid = hybrid.clone();
        incomplete_hybrid.contributes.features.clear();
        assert!(validate_manifest(&incomplete_hybrid).is_err());
    }

    #[test]
    fn validates_frozen_runtime_classification_matrix() {
        for runtime in ["xingchen-agent", "xingchen-workflow"] {
            let feature = manifest_for("feature", runtime);
            let mut feature_only = feature.clone();
            feature_only.contributes.enhancements.clear();
            assert!(
                validate_manifest(&feature_only).is_ok(),
                "{runtime} feature"
            );

            let hybrid = manifest_for("hybrid", runtime);
            assert!(validate_manifest(&hybrid).is_ok(), "{runtime} hybrid");

            let mut pure_enhancement = manifest_for("enhancement", runtime);
            pure_enhancement.contributes.features.clear();
            assert!(
                validate_manifest(&pure_enhancement).is_err(),
                "{runtime} pure enhancement"
            );
        }
        for (classification, runtime) in [
            ("enhancement", "declarative-ui"),
            ("hybrid", "declarative-ui"),
            ("feature", "prompt-pack"),
            ("hybrid", "prompt-pack"),
        ] {
            assert!(
                validate_manifest(&manifest_for(classification, runtime)).is_err(),
                "{runtime} {classification}"
            );
        }
        let feature_with_enhancement = manifest_for("feature", "xingchen-workflow");
        assert!(validate_manifest(&feature_with_enhancement).is_err());
        let mut enhancement_with_feature = manifest_for("enhancement", "prompt-pack");
        assert!(validate_manifest(&enhancement_with_feature).is_err());
        enhancement_with_feature.contributes.features.clear();
        assert!(validate_manifest(&enhancement_with_feature).is_ok());
    }

    #[test]
    fn requires_enhancement_xingchen_and_file_output_permissions() {
        let mut hybrid = manifest_for("hybrid", "xingchen-workflow");
        hybrid
            .permissions
            .retain(|permission| permission != "ai.context.augment");
        assert!(validate_manifest(&hybrid).is_err());

        for runtime in ["xingchen-agent", "xingchen-workflow"] {
            for missing in [
                "credentials.use",
                "agents.invoke",
                "network.xingchen",
                "ai.invoke",
            ] {
                let mut manifest = manifest_for("feature", runtime);
                manifest.contributes.enhancements.clear();
                manifest
                    .permissions
                    .retain(|permission| permission != missing);
                assert!(validate_manifest(&manifest).is_err(), "{runtime} {missing}");
            }
        }

        let mut output = manifest_for("feature", "xingchen-workflow");
        output.contributes.enhancements.clear();
        output.contributes.features[0]
            .capabilities
            .push(crate::models::PluginCapability::FileDocxOutput);
        assert!(validate_manifest(&output).is_err());
        output.permissions.push("files.writeSelected".into());
        assert!(validate_manifest(&output).is_ok());
    }

    #[test]
    fn rejects_permission_runtime_mismatch_but_keeps_three_compatibility_exceptions() {
        let mut manifest = manifest_for("feature", "declarative-ui");
        manifest.contributes.enhancements.clear();
        manifest.permissions = vec!["planning.files.write".into()];
        assert!(validate_manifest(&manifest).is_err());

        for permission in ["tasks.read", "tasks.write", "mcp.connect"] {
            manifest.permissions = vec![permission.into()];
            assert!(validate_manifest(&manifest).is_ok(), "{permission}");
        }
        manifest.permissions = vec!["tasks.read".into(), "tasks.read".into()];
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn v3_permissions_reject_legacy_reserved_blocked_and_unknown() {
        let mut manifest = manifest_for("feature", "declarative-ui");
        manifest.contributes.enhancements.clear();
        manifest.permissions = vec!["ai.invoke".into()];
        assert!(validate_manifest(&manifest).is_ok());

        for permission in [
            "ai:chat",
            "notes.read",
            "credentials.configure",
            "unknown.permission",
        ] {
            manifest.permissions = vec![permission.into()];
            assert!(
                validate_manifest(&manifest).is_err(),
                "{permission} must not be accepted by v3"
            );
        }
    }

    #[test]
    fn formal_contributions_require_declarative_ui_and_handlers() {
        let mut missing_ui = manifest_for("feature", "declarative-ui");
        missing_ui.contributes.enhancements.clear();
        missing_ui.contributes.features[0].ui_schema = None;
        assert!(validate_manifest(&missing_ui).is_err());

        let mut executable_feature = manifest_for("feature", "declarative-ui");
        executable_feature.contributes.enhancements.clear();
        executable_feature.contributes.features[0].handler = Some(PluginDeclarativeHandler {
            kind: "javascript".into(),
            resource: "main.js".into(),
        });
        assert!(validate_manifest(&executable_feature).is_err());

        let mut executable_enhancement = manifest_for("enhancement", "prompt-pack");
        executable_enhancement.contributes.features.clear();
        executable_enhancement.contributes.enhancements[0]
            .handler
            .kind = "python".into();
        assert!(validate_manifest(&executable_enhancement).is_err());
    }

    #[test]
    fn formal_runtime_allows_controlled_xingchen_but_blocks_legacy_js() {
        let xingchen = manifest_for("hybrid", "xingchen-workflow");
        assert!(runtime_policy_for_manifest(&xingchen).can_execute);

        let legacy = manifest_for("hybrid", "legacy-js");
        let policy = runtime_policy_for_manifest(&legacy);
        assert!(!policy.can_execute);
        assert!(!policy.raw_invoke_allowed);
    }

    #[test]
    fn package_security_rejects_scripts_and_likely_secrets() {
        let script_dir = TestDir::new();
        fs::write(script_dir.path().join("main.js"), "console.log('unsafe')")
            .expect("write script");
        assert!(scan_package_security(script_dir.path()).is_err());

        let secret_dir = TestDir::new();
        fs::write(
            secret_dir.path().join("config.json"),
            r#"{"apiSecret":"real-looking-secret-value"}"#,
        )
        .expect("write secret fixture");
        assert!(scan_package_security(secret_dir.path()).is_err());

        let placeholder_dir = TestDir::new();
        fs::write(
            placeholder_dir.path().join("README.md"),
            "api_key=${YOUR_API_KEY}\nToken 由用户在安全凭据中心配置。",
        )
        .expect("write placeholder fixture");
        assert!(scan_package_security(placeholder_dir.path()).is_ok());
    }

    #[test]
    fn manifest_and_ui_schema_cannot_collect_plaintext_credentials() {
        let manifest_dir = TestDir::new();
        fs::write(
            manifest_dir.path().join("manifest.json"),
            r#"{
                "schemaVersion":3,
                "id":"com.firstwork.secret-form",
                "name":"Secret Form",
                "version":"1.0.0",
                "authorId":"tests",
                "classification":"feature",
                "runtimeKind":"xingchen-workflow",
                "supportedScenes":["global"],
                "configurationSchema":{"apiKey":{"type":"string"}},
                "contributes":{}
            }"#,
        )
        .expect("write manifest");
        assert!(read_manifest_v3(manifest_dir.path()).is_err());

        let schema_dir = TestDir::new();
        fs::write(schema_dir.path().join("README.md"), "safe example").unwrap();
        fs::write(
            schema_dir.path().join("ui.json"),
            r#"{"fields":[{"key":"api_secret","label":"Secret","type":"text"}]}"#,
        )
        .unwrap();
        let mut manifest = manifest_for("feature", "xingchen-workflow");
        manifest.contributes.enhancements.clear();
        assert!(validate_manifest_resources(schema_dir.path(), &manifest).is_err());
    }

    #[test]
    fn normalizes_json_code_fence_and_double_serialization() {
        assert_eq!(
            normalize_json_output("```json\n{\"answer\":\"ok\"}\n```").unwrap(),
            "{\n  \"answer\": \"ok\"\n}"
        );
        assert_eq!(
            normalize_json_output(
                &serde_json::to_string(r#"{"answer":"ok"}"#).expect("serialize nested JSON"),
            )
            .unwrap(),
            "{\n  \"answer\": \"ok\"\n}"
        );
        assert!(normalize_json_output("not json").is_err());
    }
}
