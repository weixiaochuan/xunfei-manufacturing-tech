use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use tauri::{Emitter, Runtime};
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::database::Database;
use crate::error::AppError;
use crate::models::{
    ImportConflictPolicy, ImportProgress, ImportResult, NoteInput, OpenMarkdownResult, ScannedFile,
};
use crate::services::hash::sha256_hex;
use crate::services::quick_capture::{
    normalize_plain_text_for_markdown, preserve_md_indent_outside_fence,
};

/// 接受导入的纯文本扩展名（小写，不含点）。
/// .txt 与 .md 走同一通路：纯文本本身就是合法 markdown，无需独立 Service。
const TEXT_EXTENSIONS: &[&str] = &["md", "markdown", "txt"];
const PDF_EXTENSIONS: &[&str] = &["pdf"];
const WORD_EXTENSIONS: &[&str] = &["doc", "docx"];
const EXCEL_EXTENSIONS: &[&str] = &["xlsx", "xls", "xlsm", "xlsb", "ods"];
const MAX_MIXED_IMPORT_FILES: usize = 5_000;
const MAX_TEXT_FILE_SIZE: u64 = 20 * 1024 * 1024;
const MAX_BINARY_FILE_SIZE: u64 = 100 * 1024 * 1024;

fn is_text_extension(ext: &str) -> bool {
    TEXT_EXTENSIONS.iter().any(|&e| e == ext)
}

fn is_supported_extension(ext: &str) -> bool {
    is_text_extension(ext)
        || PDF_EXTENSIONS.iter().any(|&e| e == ext)
        || WORD_EXTENSIONS.iter().any(|&e| e == ext)
        || EXCEL_EXTENSIONS.iter().any(|&e| e == ext)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportFileKind {
    Text,
    Pdf,
    Word,
    Excel,
}

fn import_file_kind(path: &Path) -> Option<ImportFileKind> {
    match ext_lower(path).as_str() {
        "md" | "markdown" | "txt" => Some(ImportFileKind::Text),
        "pdf" => Some(ImportFileKind::Pdf),
        "doc" | "docx" => Some(ImportFileKind::Word),
        "xlsx" | "xls" | "xlsm" | "xlsb" | "ods" => Some(ImportFileKind::Excel),
        _ => None,
    }
}

/// 读取纯文本文件并自动嗅探编码。
///
/// 优先尝试 UTF-8（最常见、零开销）；失败时用 chardetng 推断（GBK / GB18030 /
/// Big5 / Shift_JIS 等中文 / 日文老 txt 大多落在这里），再用 encoding_rs 解码。
/// 解码用 `decode` 而非 `decode_without_bom_handling` —— 顺手吃掉文件头 BOM。
pub fn read_text_auto_encoding(path: &Path) -> Result<String, AppError> {
    let bytes = std::fs::read(path).map_err(AppError::Io)?;
    if let Ok(s) = std::str::from_utf8(&bytes) {
        // 去掉 UTF-8 BOM（\u{FEFF}），避免显示成行首怪字符
        return Ok(s.trim_start_matches('\u{FEFF}').to_owned());
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(&bytes, true);
    let encoding = detector.guess(None, true);
    let (cow, _, _) = encoding.decode(&bytes);
    Ok(cow.into_owned())
}

/// 取文件扩展名小写形式（无后缀返回空字符串）。
fn ext_lower(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

pub struct ImportService;

impl ImportService {
    /// 扫描文件夹，返回所有 Markdown 文件列表（不导入）
    ///
    /// 每条带 `relative_dir`（相对扫描根的父目录，斜杠统一 '/'，根层为空串），
    /// 以及 `match_kind` + `existing_note_id` —— 扫描阶段就告诉前端哪些文件
    /// 已导入过（path 主判 / title+content_hash 兜底），便于弹窗展示分桶统计。
    pub fn scan_markdown_folder(
        db: &Database,
        folder_path: &str,
    ) -> Result<Vec<ScannedFile>, AppError> {
        Self::scan_folder(db, folder_path, false)
    }

    /// 扫描混合文档文件夹。每个文件按自身扩展名识别，不把整个目录绑定为单一格式。
    pub fn scan_supported_folder(
        db: &Database,
        folder_path: &str,
    ) -> Result<Vec<ScannedFile>, AppError> {
        Self::scan_folder(db, folder_path, true)
    }

    fn scan_folder(
        db: &Database,
        folder_path: &str,
        include_binary_documents: bool,
    ) -> Result<Vec<ScannedFile>, AppError> {
        let root = Path::new(folder_path);
        if !root.is_dir() {
            return Err(AppError::InvalidInput(format!(
                "路径不是文件夹: {}",
                folder_path
            )));
        }

        // 规范化根路径：后续要和每条文件的 parent 做 strip_prefix，统一到一套表示
        let root_canonical: PathBuf =
            std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

        let mut files: Vec<ScannedFile> = WalkDir::new(root)
            .sort_by_file_name() // 同层按字母序稳定排序
            .into_iter()
            // T-009: 跳过 OB 配置目录 / 隐藏目录 / 常见噪音目录，避免把 .obsidian / .trash /
            // .git 这类内部状态当成笔记导入。是否应跳过由 should_skip_dir_entry 判断。
            .filter_entry(|e| !should_skip_dir_entry(e))
            .filter_map(|e| e.ok())
            .filter(|e| {
                if !e.file_type().is_file() {
                    return false;
                }
                let ext = ext_lower(e.path());
                if include_binary_documents {
                    is_supported_extension(&ext)
                } else {
                    is_text_extension(&ext)
                }
            })
            .filter_map(|entry| {
                let path = entry.path();
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("未命名")
                    .to_string();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

                // relative_dir：相对根的父目录，使用正斜杠统一
                let parent = path.parent().unwrap_or(Path::new(""));
                let parent_canonical: PathBuf =
                    std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
                let relative_dir = parent_canonical
                    .strip_prefix(&root_canonical)
                    .ok()
                    .map(|p| {
                        p.components()
                            .filter_map(|c| c.as_os_str().to_str())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_default();

                // 扫描阶段就做去重判定 —— 前端预览需要
                // 文本文件扫描成本低，可立即做内容去重。PDF/Word 解析可能很慢，
                // 扫描阶段只识别类型，真正导入时再做内容级去重。
                let (match_kind, existing_id) = if is_text_extension(&ext_lower(path)) {
                    detect_existing_match(db, path, &name).unwrap_or_else(|e| {
                        log::warn!(
                            "[scan] 检测重复失败（当成 new 处理）: {} -> {}",
                            path.display(),
                            e
                        );
                        ("new".to_string(), None)
                    })
                } else {
                    ("new".to_string(), None)
                };

                Some(ScannedFile {
                    path: path.to_string_lossy().to_string(),
                    relative_dir,
                    name,
                    size,
                    match_kind,
                    existing_note_id: existing_id,
                })
            })
            .collect();

        // 二次排序：先按相对目录，再按文件名，确保前端展示稳定
        files.sort_by(|a, b| {
            a.relative_dir
                .cmp(&b.relative_dir)
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(files)
    }

    /// 导入由文件选择器或混合文件夹扫描得到的文档。
    ///
    /// Markdown/TXT 复用原有导入链（frontmatter、附件与 Obsidian 兼容）；
    /// PDF 复用 PdfService；Word 在 Rust 中解析 DOCX，旧 DOC 先走现有转换器。
    pub async fn import_mixed_files<R: Runtime, E: Emitter<R>>(
        db: &Database,
        file_paths: &[String],
        base_folder_id: Option<i64>,
        root_path: Option<&str>,
        preserve_root: bool,
        policy: ImportConflictPolicy,
        app_data_dir: &Path,
        emitter: &E,
    ) -> Result<ImportResult, AppError> {
        if file_paths.len() > MAX_MIXED_IMPORT_FILES {
            return Err(AppError::InvalidInput(format!(
                "单次最多导入 {} 个文件，当前选择了 {} 个",
                MAX_MIXED_IMPORT_FILES,
                file_paths.len()
            )));
        }

        let mut result = ImportResult::default();
        let mut text_paths = Vec::new();
        let mut binary_paths = Vec::new();

        for file_path in file_paths {
            let path = Path::new(file_path);
            match import_file_kind(path) {
                Some(ImportFileKind::Text) => {
                    if let Err(error) = validate_import_file(path, MAX_TEXT_FILE_SIZE) {
                        result.errors.push(format!(
                            "{}: {}",
                            path.file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or(file_path),
                            error
                        ));
                    } else {
                        text_paths.push(file_path.clone());
                    }
                }
                Some(
                    kind @ (ImportFileKind::Pdf | ImportFileKind::Word | ImportFileKind::Excel),
                ) => {
                    binary_paths.push((file_path.clone(), kind));
                }
                None => {
                    result.skipped += 1;
                    result.errors.push(format!(
                        "{}: 不支持的文件类型（支持 md、markdown、txt、pdf、doc、docx、xlsx、xls、xlsm、xlsb、ods）",
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(file_path)
                    ));
                }
            }
        }

        if !text_paths.is_empty() {
            let text_result = Self::import_selected_files(
                db,
                &text_paths,
                base_folder_id,
                root_path,
                preserve_root,
                policy,
                app_data_dir,
                emitter,
            )
            .await?;
            merge_import_result(&mut result, text_result);
        }

        if binary_paths.is_empty() {
            let _ = emitter.emit("import:done", &result);
            return Ok(result);
        }

        let root_canonical: Option<PathBuf> = root_path
            .map(Path::new)
            .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
        let batch_root_id = if preserve_root {
            if let Some(root) = root_canonical.as_ref() {
                let root_name = root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("导入");
                Some(get_or_create_folder(db, base_folder_id, root_name)?)
            } else {
                base_folder_id
            }
        } else {
            base_folder_id
        };
        let mut folder_cache = HashMap::new();
        folder_cache.insert(String::new(), batch_root_id);
        let total = binary_paths.len();

        for (index, (file_path, kind)) in binary_paths.into_iter().enumerate() {
            let path = Path::new(&file_path);
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("未命名")
                .to_string();
            let _ = emitter.emit(
                "import:progress",
                ImportProgress {
                    current: index + 1,
                    total,
                    file_name: file_name.clone(),
                },
            );

            let max_size = match kind {
                ImportFileKind::Text => MAX_TEXT_FILE_SIZE,
                ImportFileKind::Pdf | ImportFileKind::Word | ImportFileKind::Excel => {
                    MAX_BINARY_FILE_SIZE
                }
            };
            if let Err(error) = validate_import_file(path, max_size) {
                result.errors.push(format!("{}: {}", file_name, error));
                continue;
            }

            let target_folder_id = match root_canonical.as_ref() {
                Some(root) => {
                    let relative_dir = compute_relative_dir(path, root);
                    match ensure_folder_path(db, &relative_dir, batch_root_id, &mut folder_cache) {
                        Ok(id) => id,
                        Err(error) => {
                            result
                                .errors
                                .push(format!("{}: 创建目录失败 - {}", file_name, error));
                            continue;
                        }
                    }
                }
                None => batch_root_id,
            };

            let imported = match kind {
                ImportFileKind::Pdf => {
                    import_pdf_document(db, app_data_dir, path, target_folder_id, policy)
                }
                ImportFileKind::Word => {
                    import_word_document(db, app_data_dir, path, target_folder_id, policy)
                }
                ImportFileKind::Excel => {
                    import_excel_document(db, app_data_dir, path, target_folder_id, policy)
                }
                ImportFileKind::Text => unreachable!("文本文件已在前一批处理"),
            };

            match imported {
                Ok(BinaryImportOutcome::Imported(note_id)) => {
                    result.imported += 1;
                    result.note_ids.push(note_id);
                }
                Ok(BinaryImportOutcome::Duplicated(note_id)) => {
                    result.duplicated += 1;
                    result.note_ids.push(note_id);
                }
                Ok(BinaryImportOutcome::Skipped(existing_id)) => {
                    result.skipped += 1;
                    result.existing_note_ids.push(existing_id);
                }
                Err(error) => result.errors.push(format!("{}: {}", file_name, error)),
            }
        }

        let _ = emitter.emit("import:done", &result);
        Ok(result)
    }

    /// 按指定文件路径列表导入 Markdown 文件
    ///
    /// - `base_folder_id`: 导入到哪个文件夹下。None = 根
    /// - `root_path`: 扫描的根路径。传了才能按相对路径重建目录树；不传则全部平铺到 base
    /// - `preserve_root`: 是否在 base 下多套一层"源文件夹名"。需要 root_path 存在
    /// - `policy`: 已存在的文件怎么办（Skip / Duplicate）
    ///
    /// 同名文件夹按 (parent_id, name) 复用已有记录，避免重复创建。
    /// 每条成功导入的笔记都会写入 canonical `source_file_path`，方便下次导入时去重。
    pub async fn import_selected_files<R: Runtime, E: Emitter<R>>(
        db: &Database,
        file_paths: &[String],
        base_folder_id: Option<i64>,
        root_path: Option<&str>,
        preserve_root: bool,
        policy: ImportConflictPolicy,
        app_data_dir: &Path,
        emitter: &E,
    ) -> Result<ImportResult, AppError> {
        let total = file_paths.len();
        let mut imported = 0usize;
        let mut skipped = 0usize;
        let mut duplicated = 0usize;
        let mut errors = Vec::new();
        // T-009 frontmatter 统计
        let mut tags_attached = 0usize;
        let mut frontmatter_parsed = 0usize;
        // T-009 Commit 2 附件复制统计
        let mut attachments_copied = 0usize;
        let mut attachments_missing: Vec<String> = Vec::new();
        // 跳转所需的 ID 列表
        let mut note_ids: Vec<i64> = Vec::new();
        let mut existing_note_ids: Vec<i64> = Vec::new();

        // 提供了 root_path 时，在那里建附件索引（OB vault 模式）；
        // 没传则不索引（用户只是导入零散 .md 文件，没 vault 上下文）
        let attachment_index = match root_path {
            Some(rp) => crate::services::import_attachments::AttachmentIndex::build(Path::new(rp)),
            None => crate::services::import_attachments::AttachmentIndex::empty(),
        };

        // 预先算好根扫描路径（用于对每个文件算相对目录）+ 预先建"保留根"文件夹
        let root_canonical: Option<PathBuf> = root_path
            .map(Path::new)
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()));

        // 缓存：rel_path ("子A/子B") -> folder_id。空串键对应批次根 folder_id
        let mut folder_cache: HashMap<String, Option<i64>> = HashMap::new();

        // 若 preserve_root，在 base 下先建一个以 root basename 命名的文件夹作为批次根
        let batch_root_id = if preserve_root {
            if let Some(root_c) = root_canonical.as_ref() {
                let root_name = root_c
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("导入");
                match get_or_create_folder(db, base_folder_id, root_name) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        errors.push(format!("创建根文件夹 {} 失败: {}", root_name, e));
                        base_folder_id
                    }
                }
            } else {
                base_folder_id
            }
        } else {
            base_folder_id
        };
        folder_cache.insert(String::new(), batch_root_id);

        for (i, file_path_str) in file_paths.iter().enumerate() {
            let file_path = Path::new(file_path_str);
            let file_name = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("未命名")
                .to_string();

            // 发送进度事件
            let _ = emitter.emit(
                "import:progress",
                ImportProgress {
                    current: i + 1,
                    total,
                    file_name: file_name.clone(),
                },
            );

            // 读取文件内容（自动嗅探 UTF-8 / GBK 等编码，兼容老 .txt）
            let content = match read_text_auto_encoding(file_path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!("{}: 读取失败 - {}", file_name, e));
                    continue;
                }
            };

            // 跳过空文件
            if content.trim().is_empty() {
                skipped += 1;
                continue;
            }

            // ─── T-009: 解析 frontmatter，剥离 yaml block 后才是真正的笔记正文 ───
            let (front_matter, body_content) =
                crate::services::markdown::parse_frontmatter(&content);
            if front_matter.is_some() {
                frontmatter_parsed += 1;
            }

            // ─── 去重判定（与 scan 阶段用同一套逻辑，避免扫描后文件被改动造成不一致）
            // 注意：去重比对用 body_content（剥离 frontmatter 后），与 import 实际写库内容
            // 保持一致，避免"frontmatter 改了但正文没改"被误判为新笔记
            let canonical_path = canonicalize_path(file_path);
            let fm_title = front_matter.as_ref().and_then(|fm| fm.title.clone());
            let title = fm_title
                .or_else(|| extract_title(&body_content))
                .unwrap_or_else(|| file_name.clone());
            let (match_kind, existing_id) = match detect_existing_match_with_content(
                db,
                &canonical_path,
                &title,
                &body_content,
            ) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("{}: 去重检测失败 - {}", file_name, e));
                    continue;
                }
            };

            // 冲突策略分支
            let final_title = match (match_kind.as_str(), policy) {
                ("new", _) => title,
                (_, ImportConflictPolicy::Skip) => {
                    skipped += 1;
                    // 记下命中的已有笔记 ID，供前端"重复命中也跳"用
                    if let Some(id) = existing_id {
                        existing_note_ids.push(id);
                    }
                    continue;
                }
                (_, ImportConflictPolicy::Duplicate) => {
                    // 副本直接加 " (2)" 后缀。重复多次导入会累积为 (2) (2)...
                    // 不查 DB 严格唯一化 —— 用户选副本就是明确要一条独立记录，不追求唯一命名
                    duplicated += 1;
                    format!("{} (2)", title)
                }
            };

            // ─── 定位这条笔记要挂的文件夹 ───
            let target_folder_id = match root_canonical.as_ref() {
                Some(root_c) => {
                    let rel_dir = compute_relative_dir(file_path, root_c);
                    match ensure_folder_path(db, &rel_dir, batch_root_id, &mut folder_cache) {
                        Ok(id) => id,
                        Err(e) => {
                            errors.push(format!("{}: 创建目录失败 - {}", file_name, e));
                            continue;
                        }
                    }
                }
                None => batch_root_id,
            };

            // .txt 来源是 plain text，需要做两道规范化才能正确渲染：
            // 1. 合并孤立列表标记（Notion / 飞书风格的 "1.\n内容" 模式）
            // 2. 行首 ASCII 空格 → NBSP（避免被 markdown 当代码块或 trim 掉）
            // .md / .markdown 走 fence-aware 的轻量处理：fence 外行首空格 → NBSP，
            // 保留 YAML / 树状目录 / 代码片段的视觉缩进；fence 内（``` ... ```）原样
            // 保留不动，避免污染真正的代码块。
            let final_content = match ext_lower(file_path).as_str() {
                "txt" => normalize_plain_text_for_markdown(&body_content),
                _ => preserve_md_indent_outside_fence(&body_content),
            };

            let input = NoteInput {
                title: final_title.clone(),
                content: final_content,
                folder_id: target_folder_id,
            };

            match db.create_note(&input) {
                Ok(note) => {
                    // 记录新建的笔记 ID，给前端"导入后跳转"用
                    note_ids.push(note.id);
                    // 写入 canonical path，下次导入同一文件即可按 path 去重命中
                    // 注意：Duplicate 策略新建的副本也挂 canonical_path —— 这样下次
                    // 再导入同文件仍会命中 path，按用户当时选的策略处理，不会无限新建
                    let src_type = match ext_lower(file_path).as_str() {
                        "txt" => "txt",
                        _ => "md", // markdown / md 统一记 md
                    };
                    let _ = db.set_note_source_file(note.id, Some(&canonical_path), Some(src_type));

                    // ─── T-009: 把 frontmatter 中的标签关联到这条笔记 ───
                    if let Some(fm) = &front_matter {
                        for tag_name in &fm.tags {
                            match db.get_or_create_tag_by_name(tag_name) {
                                Ok(tag_id) => {
                                    if db.add_tag_to_note(note.id, tag_id).is_ok() {
                                        tags_attached += 1;
                                    }
                                }
                                Err(e) => {
                                    log::warn!(
                                        "[import] 处理 frontmatter 标签失败 ({}/{}): {}",
                                        final_title,
                                        tag_name,
                                        e
                                    );
                                }
                            }
                        }
                    }

                    // ─── T-009 Commit 2: 复制图片附件 + body 路径重写 ───
                    // 先跑同步本地路径重写（按当前 .md 目录 / vault 根 / OB 索引）
                    let note_dir_for_local = file_path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| file_path.to_path_buf());
                    let local_root = root_canonical
                        .as_ref()
                        .map(|p| p.as_path())
                        .unwrap_or_else(|| note_dir_for_local.as_path());
                    let mut current_body = input.content.clone();
                    match crate::services::import_attachments::rewrite_image_paths(
                        &current_body,
                        note.id,
                        &note_dir_for_local,
                        local_root,
                        &attachment_index,
                        app_data_dir,
                    ) {
                        Ok(rewrite) => {
                            if rewrite.copied > 0 {
                                attachments_copied += rewrite.copied;
                            }
                            for m in rewrite.missing {
                                attachments_missing.push(format!("{}: {}", final_title, m));
                            }
                            current_body = rewrite.new_body;
                        }
                        Err(e) => {
                            log::warn!("[import] 笔记 {} 本地图片重写失败: {}", note.id, e);
                        }
                    }

                    // 再跑外链下载（微信公众号/知乎等防盗链站点：下载到本地落盘）
                    match crate::services::import_attachments::rewrite_external_images(
                        &current_body,
                        note.id,
                        app_data_dir,
                    )
                    .await
                    {
                        Ok(rewrite) => {
                            if rewrite.copied > 0 {
                                attachments_copied += rewrite.copied;
                            }
                            for m in rewrite.missing {
                                attachments_missing.push(format!("{}: {}", final_title, m));
                            }
                            current_body = rewrite.new_body;
                        }
                        Err(e) => {
                            log::warn!("[import] 笔记 {} 外链图片下载失败: {}", note.id, e);
                        }
                    }

                    // 视频本地引用（与图片同步骤：先本地路径，再外链下载）
                    match crate::services::import_video_attachments::rewrite_video_paths(
                        &current_body,
                        note.id,
                        &note_dir_for_local,
                        local_root,
                        app_data_dir,
                    ) {
                        Ok(rewrite) => {
                            if rewrite.copied > 0 {
                                attachments_copied += rewrite.copied;
                            }
                            for m in rewrite.missing {
                                attachments_missing.push(format!("{}: {}", final_title, m));
                            }
                            current_body = rewrite.new_body;
                        }
                        Err(e) => {
                            log::warn!("[import] 笔记 {} 本地视频重写失败: {}", note.id, e);
                        }
                    }

                    // 视频外链下载
                    match crate::services::import_video_attachments::rewrite_external_videos(
                        &current_body,
                        note.id,
                        app_data_dir,
                    )
                    .await
                    {
                        Ok(rewrite) => {
                            if rewrite.copied > 0 {
                                attachments_copied += rewrite.copied;
                            }
                            for m in rewrite.missing {
                                attachments_missing.push(format!("{}: {}", final_title, m));
                            }
                            current_body = rewrite.new_body;
                        }
                        Err(e) => {
                            log::warn!("[import] 笔记 {} 外链视频下载失败: {}", note.id, e);
                        }
                    }

                    // 内容真的变了才回写，省一次 DB 写
                    if current_body != input.content {
                        if let Err(e) = db.update_note_content(note.id, &current_body) {
                            log::warn!("[import] 笔记 {} 图片重写后回写失败: {}", note.id, e);
                        }
                    }

                    if match_kind == "new" {
                        imported += 1;
                    }
                    // duplicate 计数已在上面分支累计，不重复加
                }
                Err(e) => {
                    errors.push(format!("{}: 导入失败 - {}", final_title, e));
                }
            }
        }

        let result = ImportResult {
            imported,
            skipped,
            duplicated,
            errors,
            tags_attached,
            frontmatter_parsed,
            attachments_copied,
            attachments_missing,
            note_ids,
            existing_note_ids,
        };

        let _ = emitter.emit("import:done", &result);

        Ok(result)
    }

    /// 打开单个 Markdown 文件：
    /// - 首次：创建新笔记并记录 source_file_path
    /// - 重复打开同一文件：复用已有笔记；若文件内容变化则同步回笔记
    ///
    /// 返回 (note_id, was_synced)：was_synced=true 表示发生了内容同步，
    /// 前端可据此显示轻量 toast。
    pub async fn import_single_markdown(
        db: &Database,
        file_path: &str,
        app_data_dir: &Path,
    ) -> Result<OpenMarkdownResult, AppError> {
        let path = Path::new(file_path);

        // 路径规范化（绝对路径 + 大小写/斜杠统一），保证"同文件多种写法"去重
        let canonical: String = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| file_path.to_string());

        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名")
            .to_string();

        let raw_content = read_text_auto_encoding(path)
            .map_err(|e| AppError::Custom(format!("读取文件失败: {} ({})", file_path, e)))?;

        if raw_content.trim().is_empty() {
            return Err(AppError::InvalidInput(format!(
                "文件内容为空: {}",
                file_path
            )));
        }

        // 去重：已有同 source_file_path 的活跃笔记 → 复用
        if let Some((existing_id, existing_content)) =
            db.find_active_note_by_source_path(&canonical)?
        {
            // 外部修改过文件 → 同步最新内容到笔记（含图片处理）
            let was_synced = existing_content != raw_content;
            if was_synced {
                let (processed, mappings) =
                    process_single_md_images(&raw_content, existing_id, path, app_data_dir).await;
                db.update_note_content(existing_id, &processed)?;
                // 重新打开 = 重置 URL 映射（外部可能换了图床/路径）
                let _ = db.clear_url_mappings(existing_id);
                let _ = db.insert_url_mappings(existing_id, &mappings);
                // 同步到笔记之后，把"上次写回时的 mtime"对齐为外部文件当前 mtime，
                // 否则下次保存时 mtime 不一致会误报"外部改过"
                if let Some(mt) = read_file_mtime(path) {
                    let _ = db.set_writeback_mtime(existing_id, mt);
                }
                log::info!(
                    "[open-md] 检测到 {} 内容变化，已同步到笔记 #{}",
                    canonical,
                    existing_id
                );
            }
            return Ok(OpenMarkdownResult {
                note_id: existing_id,
                was_synced,
            });
        }

        // 首次打开：创建笔记并记录来源
        let title = extract_title(&raw_content).unwrap_or(file_name);
        let input = NoteInput {
            title,
            content: raw_content.clone(),
            folder_id: None,
        };
        let note = db.create_note(&input)?;
        let src_type = match ext_lower(path).as_str() {
            "txt" => "txt",
            _ => "md",
        };
        let _ = db.set_note_source_file(note.id, Some(&canonical), Some(src_type));

        // 处理图片：本地相对路径（同级目录） + 外链下载（绕开微信防盗链等）
        let (processed, mappings) =
            process_single_md_images(&raw_content, note.id, path, app_data_dir).await;
        if processed != raw_content {
            if let Err(e) = db.update_note_content(note.id, &processed) {
                log::warn!("[open-md] 笔记 {} 图片重写后回写失败: {}", note.id, e);
            }
        }
        let _ = db.insert_url_mappings(note.id, &mappings);
        // 首次打开就记录 mtime；之后笔记保存时的写回会比对它做冲突检测
        if let Some(mt) = read_file_mtime(path) {
            let _ = db.set_writeback_mtime(note.id, mt);
        }

        Ok(OpenMarkdownResult {
            note_id: note.id,
            was_synced: false,
        })
    }
}

/// 读取文件 mtime（秒级 unix timestamp）；失败返回 None
///
/// 给"打开 .md → 编辑 → 写回"流程做冲突检测：上次写回时记一个 mtime，
/// 下次写回前再读一次，如果不一致说明外部编辑器（VSCode 等）在期间改过文件。
pub fn read_file_mtime(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// 单文件打开场景的图片处理：
///  - 本地相对路径（如 `./images/foo.png`）：以 .md 同级目录为锚点解析并复制到 kb_assets
///  - http(s):// 外链：下载到本地（含微信公众号防盗链处理）
///
/// 处理失败的引用保留原样，不会让笔记打开流程中断。
///
/// 返回 (处理后的 body, 替换映射)：映射 = `Vec<(原始 URL, 内部 asset URL)>`，
/// 给 open_markdown_file 写入 `note_url_mapping`，写回 .md 时按 internal_url 反查恢复原链接。
async fn process_single_md_images(
    body: &str,
    note_id: i64,
    md_path: &Path,
    app_data_dir: &Path,
) -> (String, Vec<(String, String)>) {
    let note_dir = md_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| md_path.to_path_buf());

    // 单文件场景没有 vault 根；用 .md 同级目录兼当 vault 根，
    // OB 附件索引为空（`AttachmentIndex::empty()`），仅靠相对路径解析
    let empty_index = crate::services::import_attachments::AttachmentIndex::empty();
    let mut current = body.to_string();
    let mut all_mappings: Vec<(String, String)> = Vec::new();
    if let Ok(rewrite) = crate::services::import_attachments::rewrite_image_paths(
        &current,
        note_id,
        &note_dir,
        &note_dir,
        &empty_index,
        app_data_dir,
    ) {
        current = rewrite.new_body;
        all_mappings.extend(rewrite.mappings);
    }
    if let Ok(rewrite) = crate::services::import_attachments::rewrite_external_images(
        &current,
        note_id,
        app_data_dir,
    )
    .await
    {
        current = rewrite.new_body;
        all_mappings.extend(rewrite.mappings);
    }
    // 视频本地引用 + 外链下载（与图片同序：先本地，再外链）
    if let Ok(rewrite) = crate::services::import_video_attachments::rewrite_video_paths(
        &current,
        note_id,
        &note_dir,
        &note_dir,
        app_data_dir,
    ) {
        current = rewrite.new_body;
        all_mappings.extend(rewrite.mappings);
    }
    if let Ok(rewrite) = crate::services::import_video_attachments::rewrite_external_videos(
        &current,
        note_id,
        app_data_dir,
    )
    .await
    {
        current = rewrite.new_body;
        all_mappings.extend(rewrite.mappings);
    }
    (current, all_mappings)
}

/// 计算某文件相对扫描根的父目录（斜杠统一为 '/'，根层为空串）
fn compute_relative_dir(file_path: &Path, root_canonical: &Path) -> String {
    let parent = file_path.parent().unwrap_or(Path::new(""));
    let parent_canonical: PathBuf =
        std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    parent_canonical
        .strip_prefix(root_canonical)
        .ok()
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

/// 确保相对路径 "子A/子B" 对应的 folder 链存在；返回最深那层的 folder_id
/// （根层 rel_path="" 直接返回 batch_root_id）。
fn ensure_folder_path(
    db: &Database,
    rel_path: &str,
    batch_root: Option<i64>,
    cache: &mut HashMap<String, Option<i64>>,
) -> Result<Option<i64>, AppError> {
    if let Some(&cached) = cache.get(rel_path) {
        return Ok(cached);
    }

    let parts: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_parent: Option<i64> = batch_root;
    let mut accumulated = String::new();

    for part in parts {
        if !accumulated.is_empty() {
            accumulated.push('/');
        }
        accumulated.push_str(part);

        if let Some(&cached) = cache.get(&accumulated) {
            current_parent = cached;
            continue;
        }

        let folder_id = get_or_create_folder(db, current_parent, part)?;
        cache.insert(accumulated.clone(), Some(folder_id));
        current_parent = Some(folder_id);
    }

    Ok(current_parent)
}

/// 查找同层同名文件夹；存在则复用，否则创建
fn get_or_create_folder(
    db: &Database,
    parent_id: Option<i64>,
    name: &str,
) -> Result<i64, AppError> {
    if let Some(id) = db.find_folder_by_name(parent_id, name)? {
        return Ok(id);
    }
    let folder = db.create_folder(name, parent_id)?;
    Ok(folder.id)
}

/// 从 Markdown 内容提取标题（第一个 # 开头的行）
fn extract_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            let title = trimmed.trim_start_matches('#').trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
        // 跳过空行和 frontmatter
        if trimmed.is_empty() || trimmed == "---" {
            continue;
        }
        // 非标题非空行，停止查找
        if !trimmed.starts_with('#') && !trimmed.starts_with("---") {
            break;
        }
    }
    None
}

/// 文件路径规范化字符串（大小写+斜杠统一），用于与 DB source_file_path 精确比对
fn canonicalize_path(file_path: &Path) -> String {
    std::fs::canonicalize(file_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned())
}

/// 扫描阶段的去重判定：先读文件内容算 hash，再按 (path) / (title, hash) 查 DB
///
/// 扫描大目录时每个文件都要读一遍；几 KB MD 文件 SHA-256 是毫秒级，可接受。
/// 大文件或海量文件场景再考虑跳过 hash 走仅 path 匹配。
fn detect_existing_match(
    db: &Database,
    file_path: &Path,
    file_stem: &str,
) -> Result<(String, Option<i64>), AppError> {
    let canonical = canonicalize_path(file_path);

    // 先按 path 匹配（最精确，不用读文件）
    if let Some((id, _)) = db.find_active_note_by_source_path(&canonical)? {
        return Ok(("path".to_string(), Some(id)));
    }

    // 按 title + content_hash 兜底
    let content = match read_text_auto_encoding(file_path) {
        Ok(c) => c,
        Err(_) => return Ok(("new".to_string(), None)),
    };
    let title = extract_title(&content).unwrap_or_else(|| file_stem.to_string());
    let hash = sha256_hex(&content);
    if let Some(id) = db.find_active_note_by_title_and_hash(&title, &hash)? {
        return Ok(("fuzzy".to_string(), Some(id)));
    }

    Ok(("new".to_string(), None))
}

/// import 阶段的去重判定：content 已读入内存，避免再读文件
fn detect_existing_match_with_content(
    db: &Database,
    canonical_path: &str,
    title: &str,
    content: &str,
) -> Result<(String, Option<i64>), AppError> {
    if let Some((id, _)) = db.find_active_note_by_source_path(canonical_path)? {
        return Ok(("path".to_string(), Some(id)));
    }
    let hash = sha256_hex(content);
    if let Some(id) = db.find_active_note_by_title_and_hash(title, &hash)? {
        return Ok(("fuzzy".to_string(), Some(id)));
    }
    Ok(("new".to_string(), None))
}

/// T-009: 遍历时是否应跳过该目录条目
///
/// 跳过：
/// - 任何点开头的目录（`.obsidian` / `.trash` / `.git` / `.DS_Store` …）— 但**根目录本身**
///   即使是 `.foo` 也不跳，因为用户主动选了它
/// - `node_modules`（OB vault 里很少见，但偶尔有人把代码目录混入）
///
/// 文件不在这里过滤；文件层的 `.md` 后缀过滤交给上游 `.filter` 链。
fn should_skip_dir_entry(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    if !entry.file_type().is_dir() {
        return false;
    }
    let name = match entry.file_name().to_str() {
        Some(n) => n,
        None => return false,
    };
    name.starts_with('.') || name == "node_modules"
}

fn validate_import_file(path: &Path, max_size: u64) -> Result<(), AppError> {
    if !path.is_file() {
        return Err(AppError::InvalidInput(format!(
            "路径不是文件: {}",
            path.display()
        )));
    }
    let size = std::fs::metadata(path)?.len();
    if size > max_size {
        return Err(AppError::InvalidInput(format!(
            "文件过大（{} MB），单文件上限为 {} MB",
            size / 1024 / 1024,
            max_size / 1024 / 1024
        )));
    }
    Ok(())
}

fn merge_import_result(target: &mut ImportResult, mut source: ImportResult) {
    target.imported += source.imported;
    target.skipped += source.skipped;
    target.duplicated += source.duplicated;
    target.tags_attached += source.tags_attached;
    target.frontmatter_parsed += source.frontmatter_parsed;
    target.attachments_copied += source.attachments_copied;
    target.errors.append(&mut source.errors);
    target
        .attachments_missing
        .append(&mut source.attachments_missing);
    target.note_ids.append(&mut source.note_ids);
    target
        .existing_note_ids
        .append(&mut source.existing_note_ids);
}

enum BinaryImportOutcome {
    Imported(i64),
    Duplicated(i64),
    Skipped(i64),
}

fn import_pdf_document(
    db: &Database,
    app_data_dir: &Path,
    source: &Path,
    folder_id: Option<i64>,
    policy: ImportConflictPolicy,
) -> Result<BinaryImportOutcome, AppError> {
    let title = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("未命名 PDF")
        .to_string();

    // 抽取一次结构化正文，同时用于去重和实际落库，避免大 PDF 被重复解析。
    let comparable_content = crate::services::pdf::PdfService::extract_editable_html_only(source)?;
    let canonical = canonicalize_path(source);
    let (match_kind, existing_id) =
        detect_existing_match_with_content(db, &canonical, &title, &comparable_content)?;
    if match_kind != "new" && matches!(policy, ImportConflictPolicy::Skip) {
        return existing_id
            .map(BinaryImportOutcome::Skipped)
            .ok_or_else(|| AppError::Custom("PDF 去重命中但未找到已有笔记".into()));
    }

    let note = crate::services::pdf::PdfService::import_one_with_editable_html(
        app_data_dir,
        db,
        source,
        folder_id,
        comparable_content,
    )?;
    if match_kind == "new" {
        return Ok(BinaryImportOutcome::Imported(note.id));
    }

    let duplicate_title = format!("{} (2)", note.title);
    db.update_note(
        note.id,
        &NoteInput {
            title: duplicate_title,
            content: note.content,
            folder_id,
        },
    )?;
    Ok(BinaryImportOutcome::Duplicated(note.id))
}

fn import_word_document(
    db: &Database,
    app_data_dir: &Path,
    source: &Path,
    folder_id: Option<i64>,
    policy: ImportConflictPolicy,
) -> Result<BinaryImportOutcome, AppError> {
    let raw_text = extract_word_text(source)?;
    let content = normalize_plain_text_for_markdown(&raw_text);
    if content.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Word 文档没有可导入的文本内容".into(),
        ));
    }

    let title = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("导入的 Word 文档")
        .to_string();
    let canonical = canonicalize_path(source);
    let (match_kind, existing_id) =
        detect_existing_match_with_content(db, &canonical, &title, &content)?;
    if match_kind != "new" && matches!(policy, ImportConflictPolicy::Skip) {
        return existing_id
            .map(BinaryImportOutcome::Skipped)
            .ok_or_else(|| AppError::Custom("Word 去重命中但未找到已有笔记".into()));
    }

    let final_title = if match_kind == "new" {
        title
    } else {
        format!("{} (2)", title)
    };
    let note = db.create_note(&NoteInput {
        title: final_title,
        content,
        folder_id,
    })?;
    let source_type = ext_lower(source);
    match crate::services::source_file::SourceFileService::attach(
        app_data_dir,
        note.id,
        source,
        &source_type,
    ) {
        Ok(relative_path) => {
            if let Err(error) =
                db.set_note_source_file(note.id, Some(&relative_path), Some(&source_type))
            {
                log::warn!(
                    "[import] Word 笔记 {} 已创建，但源文件关联失败: {}",
                    note.id,
                    error
                );
            }
        }
        Err(error) => {
            log::warn!(
                "[import] Word 笔记 {} 已创建，但原文件复制失败: {}",
                note.id,
                error
            );
        }
    }

    if match_kind == "new" {
        Ok(BinaryImportOutcome::Imported(note.id))
    } else {
        Ok(BinaryImportOutcome::Duplicated(note.id))
    }
}

#[cfg(desktop)]
fn import_excel_document(
    db: &Database,
    app_data_dir: &Path,
    source: &Path,
    folder_id: Option<i64>,
    policy: ImportConflictPolicy,
) -> Result<BinaryImportOutcome, AppError> {
    let source_path = source.to_string_lossy();
    let summary = crate::services::excel_parser::read_workbook(source_path.as_ref())?;
    let content = render_excel_import_markdown(source, &summary);
    if content.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Excel 文件没有可导入的表格内容".into(),
        ));
    }

    let title = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("导入的 Excel 工作簿")
        .to_string();
    let canonical = canonicalize_path(source);
    let (match_kind, existing_id) =
        detect_existing_match_with_content(db, &canonical, &title, &content)?;
    if match_kind != "new" && matches!(policy, ImportConflictPolicy::Skip) {
        return existing_id
            .map(BinaryImportOutcome::Skipped)
            .ok_or_else(|| AppError::Custom("Excel 去重命中但未找到已有笔记".into()));
    }

    let final_title = if match_kind == "new" {
        title
    } else {
        format!("{} (2)", title)
    };
    let note = db.create_note(&NoteInput {
        title: final_title,
        content,
        folder_id,
    })?;
    let source_type = ext_lower(source);
    match crate::services::source_file::SourceFileService::attach(
        app_data_dir,
        note.id,
        source,
        &source_type,
    ) {
        Ok(relative_path) => {
            if let Err(error) =
                db.set_note_source_file(note.id, Some(&relative_path), Some(&source_type))
            {
                log::warn!(
                    "[import] Excel 笔记 {} 已创建，但源文件关联失败: {}",
                    note.id,
                    error
                );
            }
        }
        Err(error) => {
            log::warn!(
                "[import] Excel 笔记 {} 已创建，但原文件复制失败: {}",
                note.id,
                error
            );
        }
    }

    if match_kind == "new" {
        Ok(BinaryImportOutcome::Imported(note.id))
    } else {
        Ok(BinaryImportOutcome::Duplicated(note.id))
    }
}

#[cfg(not(desktop))]
fn import_excel_document(
    _db: &Database,
    _app_data_dir: &Path,
    _source: &Path,
    _folder_id: Option<i64>,
    _policy: ImportConflictPolicy,
) -> Result<BinaryImportOutcome, AppError> {
    Err(AppError::InvalidInput("当前平台暂不支持 Excel 导入".into()))
}

#[cfg(desktop)]
fn render_excel_import_markdown(
    source: &Path,
    summary: &crate::services::excel_parser::ExcelSummary,
) -> String {
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Excel");
    let mut out = String::new();
    out.push_str(&format!("> 来源工作簿：{}\n", file_name));
    out.push_str(&format!(
        "> Sheet 数：{}，总行数：{}\n",
        summary.sheets.len(),
        summary.total_rows
    ));
    if !summary.truncated_sheet_names.is_empty() {
        out.push_str(&format!(
            "> 因表格较大，以下 Sheet 已保留代表性内容：{}\n",
            summary.truncated_sheet_names.join("、")
        ));
    }
    out.push('\n');
    out.push_str(summary.markdown.trim());
    out.push('\n');
    out
}

fn extract_word_text(source: &Path) -> Result<String, AppError> {
    match ext_lower(source).as_str() {
        "docx" => extract_docx_text(source),
        "doc" => {
            let temp_dir = std::env::temp_dir()
                .join(format!("firstwork-word-import-{}", uuid::Uuid::new_v4()));
            let result = match crate::services::converter::convert_doc_to_docx(source, &temp_dir) {
                Ok(converted) => extract_docx_text(&converted),
                Err(error) => Err(error),
            };
            if let Err(error) = std::fs::remove_dir_all(&temp_dir) {
                log::debug!("清理 Word 转换临时目录失败: {}", error);
            }
            result
        }
        other => Err(AppError::InvalidInput(format!(
            "不支持的 Word 文件类型: {}",
            other
        ))),
    }
}

fn extract_docx_text(source: &Path) -> Result<String, AppError> {
    let file = File::open(source)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::Custom(format!("DOCX 文件结构无效: {}", error)))?;
    let mut document = archive
        .by_name("word/document.xml")
        .map_err(|error| AppError::Custom(format!("DOCX 缺少正文: {}", error)))?;
    let mut xml = String::new();
    document
        .read_to_string(&mut xml)
        .map_err(|error| AppError::Custom(format!("读取 DOCX 正文失败: {}", error)))?;
    Ok(parse_wordprocessingml_text(&xml))
}

/// 提取 WordprocessingML 的可见文本，并把段落、换行和表格单元格转换为纯文本结构。
fn parse_wordprocessingml_text(xml: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0usize;
    let mut in_text_node = false;

    while let Some(relative_start) = xml[cursor..].find('<') {
        let tag_start = cursor + relative_start;
        if in_text_node && tag_start > cursor {
            output.push_str(&decode_xml_entities(&xml[cursor..tag_start]));
        }
        let Some(relative_end) = xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + relative_end;
        let tag = xml[tag_start + 1..tag_end].trim();
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        let tag_name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_ascii_lowercase();

        match (closing, tag_name.as_str()) {
            (false, "w:t" | "w:instrtext") => in_text_node = !self_closing,
            (true, "w:t" | "w:instrtext") => in_text_node = false,
            (false, "w:tab") => output.push('\t'),
            (false, "w:br" | "w:cr") => output.push('\n'),
            (true, "w:tc") => output.push('\t'),
            (true, "w:tr") => output.push('\n'),
            (true, "w:p") => output.push_str("\n\n"),
            _ => {}
        }
        cursor = tag_end + 1;
    }
    if in_text_node && cursor < xml.len() {
        output.push_str(&decode_xml_entities(&xml[cursor..]));
    }

    normalize_plain_text_for_markdown(output.trim())
}

fn decode_xml_entities(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        let entity = &rest[start..];
        let Some(end) = entity.find(';') else {
            output.push_str(entity);
            return output;
        };
        let name = &entity[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16)
                .ok()
                .and_then(char::from_u32),
            _ if name.starts_with('#') => name[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        if let Some(value) = decoded {
            output.push(value);
        } else {
            output.push_str(&entity[..=end]);
        }
        rest = &entity[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_import_recognizes_each_supported_file_type() {
        assert_eq!(
            import_file_kind(Path::new("note.MD")),
            Some(ImportFileKind::Text)
        );
        assert_eq!(
            import_file_kind(Path::new("plain.txt")),
            Some(ImportFileKind::Text)
        );
        assert_eq!(
            import_file_kind(Path::new("paper.PDF")),
            Some(ImportFileKind::Pdf)
        );
        assert_eq!(
            import_file_kind(Path::new("report.docx")),
            Some(ImportFileKind::Word)
        );
        assert_eq!(
            import_file_kind(Path::new("legacy.DOC")),
            Some(ImportFileKind::Word)
        );
        assert_eq!(
            import_file_kind(Path::new("table.XLSX")),
            Some(ImportFileKind::Excel)
        );
        assert_eq!(
            import_file_kind(Path::new("macro.xlsm")),
            Some(ImportFileKind::Excel)
        );
        assert_eq!(
            import_file_kind(Path::new("sheet.ods")),
            Some(ImportFileKind::Excel)
        );
        assert_eq!(import_file_kind(Path::new("image.png")), None);
    }

    #[test]
    fn wordprocessingml_parser_preserves_paragraphs_tables_and_entities() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <w:document><w:body>
              <w:p><w:r><w:t>标题 &amp; 摘要</w:t></w:r></w:p>
              <w:p><w:r><w:t>第一行</w:t><w:br/><w:t>第二行</w:t></w:r></w:p>
              <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
              <w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            </w:body></w:document>"#;
        let text = parse_wordprocessingml_text(xml);
        assert!(text.contains("标题 & 摘要"));
        assert!(text.contains("第一行\n第二行"));
        assert!(text.contains('A'));
        assert!(text.contains('B'));
    }

    #[test]
    fn docx_archive_extracts_document_xml() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let path = std::env::temp_dir().join(format!(
            "firstwork-import-test-{}.docx",
            uuid::Uuid::new_v4()
        ));
        let file = File::create(&path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"<w:document><w:body><w:p><w:r><w:t>Hello Word</w:t></w:r></w:p></w:body></w:document>")
            .unwrap();
        writer.finish().unwrap();

        let extracted = extract_docx_text(&path).unwrap();
        assert_eq!(extracted, "Hello Word");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn word_document_import_creates_note_and_skips_duplicate() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let root = std::env::temp_dir().join(format!(
            "firstwork-word-import-integration-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("项目说明.docx");
        let file = File::create(&source).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(
                b"<w:document><w:body><w:p><w:r><w:t>Imported content</w:t></w:r></w:p></w:body></w:document>",
            )
            .unwrap();
        writer.finish().unwrap();

        let db = Database::init(":memory:").unwrap();
        let first =
            import_word_document(&db, &root, &source, None, ImportConflictPolicy::Skip).unwrap();
        let note_id = match first {
            BinaryImportOutcome::Imported(id) => id,
            _ => panic!("首次 Word 导入应创建笔记"),
        };
        let note = db.get_note(note_id).unwrap().unwrap();
        assert!(note.content.contains("Imported content"));
        assert_eq!(note.source_file_type.as_deref(), Some("docx"));

        let second =
            import_word_document(&db, &root, &source, None, ImportConflictPolicy::Skip).unwrap();
        assert!(matches!(
            second,
            BinaryImportOutcome::Skipped(id) if id == note_id
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(desktop)]
    #[test]
    fn excel_document_import_creates_markdown_note_and_skips_duplicate() {
        let root = std::env::temp_dir().join(format!(
            "firstwork-excel-import-integration-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("process-plan.xlsx");
        write_minimal_xlsx(&source);

        let db = Database::init(":memory:").unwrap();
        let first =
            import_excel_document(&db, &root, &source, None, ImportConflictPolicy::Skip).unwrap();
        let note_id = match first {
            BinaryImportOutcome::Imported(id) => id,
            _ => panic!("首次 Excel 导入应创建笔记"),
        };
        let note = db.get_note(note_id).unwrap().unwrap();
        assert!(note.content.contains("## Sheet: Sheet1"));
        assert!(note.content.contains("| Topic | Value |"));
        assert!(note.content.contains("| Process | 42 |"));
        assert_eq!(note.source_file_type.as_deref(), Some("xlsx"));

        let second =
            import_excel_document(&db, &root, &source, None, ImportConflictPolicy::Skip).unwrap();
        assert!(matches!(
            second,
            BinaryImportOutcome::Skipped(id) if id == note_id
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(desktop)]
    fn write_minimal_xlsx(path: &Path) {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        writer.start_file("[Content_Types].xml", options).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
            )
            .unwrap();

        writer.start_file("_rels/.rels", options).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
            )
            .unwrap();

        writer.start_file("xl/workbook.xml", options).unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#,
            )
            .unwrap();

        writer
            .start_file("xl/_rels/workbook.xml.rels", options)
            .unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#,
            )
            .unwrap();

        writer
            .start_file("xl/worksheets/sheet1.xml", options)
            .unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="inlineStr"><is><t>Topic</t></is></c>
      <c r="B1" t="inlineStr"><is><t>Value</t></is></c>
    </row>
    <row r="2">
      <c r="A2" t="inlineStr"><is><t>Process</t></is></c>
      <c r="B2"><v>42</v></c>
    </row>
  </sheetData>
</worksheet>"#,
            )
            .unwrap();

        writer.finish().unwrap();
    }

    #[test]
    fn skip_dot_dirs() {
        // 用临时目录构造，避免依赖项目内文件
        let tmp = std::env::temp_dir().join(format!("kb-import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join(".obsidian")).unwrap();
        std::fs::create_dir_all(tmp.join(".trash")).unwrap();
        std::fs::create_dir_all(tmp.join("regular")).unwrap();
        std::fs::create_dir_all(tmp.join("node_modules")).unwrap();
        std::fs::write(tmp.join(".obsidian/workspace.md"), "x").unwrap();
        std::fs::write(tmp.join(".trash/old.md"), "x").unwrap();
        std::fs::write(tmp.join("regular/keep.md"), "x").unwrap();
        std::fs::write(tmp.join("node_modules/lib.md"), "x").unwrap();
        std::fs::write(tmp.join("root.md"), "x").unwrap();

        let mds: Vec<_> = WalkDir::new(&tmp)
            .into_iter()
            .filter_entry(|e| !should_skip_dir_entry(e))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(mds.contains(&"root.md".to_string()));
        assert!(mds.contains(&"keep.md".to_string()));
        assert!(!mds.iter().any(|n| n == "workspace.md"));
        assert!(!mds.iter().any(|n| n == "old.md"));
        assert!(!mds.iter().any(|n| n == "lib.md"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
