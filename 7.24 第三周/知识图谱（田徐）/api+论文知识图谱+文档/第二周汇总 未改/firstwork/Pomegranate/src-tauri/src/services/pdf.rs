use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(desktop)]
use std::sync::{Mutex, OnceLock};

#[cfg(desktop)]
use pdfium_render::prelude::*;
use serde::Serialize;
#[cfg(desktop)]
use sha2::{Digest, Sha256};

use crate::database::Database;
use crate::error::AppError;
use crate::models::{Note, NoteInput};
#[cfg(desktop)]
use crate::services::{asset_path, image::ImageService};

/// PDFium 全局实例（嵌入 pdfium.dll），用作 pdf-extract 的 fallback。
///
/// pdfium-render 的 `Pdfium` 不实现 Send（内部 `Box<dyn PdfiumLibraryBindings>`），
/// 但 Tauri Command 在 worker 线程执行，需要跨线程共享。因此用 newtype 包裹并手动声明
/// Send/Sync —— 安全性由外层 `Mutex` 保证（同一时刻只有一个线程持有 PDFium 引用）。
///
/// 仅桌面端：移动端 NDK 加载动态库受沙盒限制，不引入 PDFium。
#[cfg(desktop)]
struct PdfiumGuard(Pdfium);
// SAFETY: PDFium 底层 C API 非线程安全，但我们通过 Mutex 串行化所有访问
#[cfg(desktop)]
unsafe impl Send for PdfiumGuard {}
#[cfg(desktop)]
unsafe impl Sync for PdfiumGuard {}

#[cfg(desktop)]
static PDFIUM: OnceLock<Mutex<PdfiumGuard>> = OnceLock::new();

/// 应用启动时调用：初始化 PDFium 全局实例（仅桌面端）。
///
/// Windows: 编译时嵌入 pdfium.dll 到 EXE，运行时提取到 %TEMP% 加载。
/// macOS/Linux: 暂不支持嵌入，需外部提供 pdfium 动态库。
#[cfg(desktop)]
pub fn init_pdfium() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        const EMBEDDED_DLL: &[u8] = include_bytes!("../../pdfium/embedded/pdfium.dll");
        let dll_path = std::env::temp_dir().join("inb_pdfium_embedded.dll");
        // 每次启动覆盖写入（临时目录在重启后由 OS 清理）
        std::fs::write(&dll_path, EMBEDDED_DLL)
            .map_err(|e| format!("PDFium 写入临时目录失败: {}", e))?;

        let bindings = Pdfium::bind_to_library(&dll_path).map_err(|e| e.to_string())?;
        let pdfium = Pdfium::new(bindings);
        PDFIUM
            .set(Mutex::new(PdfiumGuard(pdfium)))
            .map_err(|_| "PDFium 已被初始化过".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        // 非 Windows：尝试从当前目录加载系统库
        let bindings = Pdfium::bind_to_system_library().map_err(|e| e.to_string())?;
        let pdfium = Pdfium::new(bindings);
        PDFIUM
            .set(Mutex::new(PdfiumGuard(pdfium)))
            .map_err(|_| "PDFium 已被初始化过".to_string())
    }
}

/// PDF 资产目录名（dev 模式加 dev- 前缀实现数据隔离）
const PDFS_DIR_PROD: &str = "pdfs";
const PDFS_DIR_DEV: &str = "dev-pdfs";
#[cfg(desktop)]
const MAX_EXTRACTED_PDF_IMAGES: usize = 100;
#[cfg(desktop)]
const MAX_EXTRACTED_IMAGE_BYTES: usize = 100 * 1024 * 1024;
#[cfg(desktop)]
const MAX_EXTRACTED_IMAGE_WIDTH: i32 = 2_000;
#[cfg(desktop)]
const MIN_EXTRACTED_IMAGE_SIDE: i32 = 80;

#[inline]
fn pdfs_dir_name() -> &'static str {
    if cfg!(debug_assertions) {
        PDFS_DIR_DEV
    } else {
        PDFS_DIR_PROD
    }
}

/// 单个 PDF 导入结果，供前端展示进度/错误清单
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfImportResult {
    pub source_path: String,
    /// 成功：对应的笔记 id；失败：None
    pub note_id: Option<i64>,
    /// 成功：笔记标题；失败：None
    pub title: Option<String>,
    /// 失败时的错误消息
    pub error: Option<String>,
}

pub struct PdfService;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtractedPdfImage {
    page_number: usize,
    image_number: usize,
    asset_url: String,
    width: i32,
    height: i32,
}

impl PdfService {
    /// 获取 PDF 根目录: {app_data_dir}/{prefix}pdfs/
    /// 仅抽取 PDF 纯文本（不落盘、不创建笔记）。供 AI 会话附件等场景复用，
    /// 内部走与 `import_one` 相同的「pdf-extract → pdfium 修复重试」通路。
    pub fn extract_text_only(source: &Path) -> Result<String, AppError> {
        if !source.exists() {
            return Err(AppError::NotFound(format!(
                "PDF 文件不存在: {}",
                source.display()
            )));
        }
        let raw = extract_text_with_repair(source)?;
        Ok(normalize_text(&raw))
    }

    pub fn pdfs_dir(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join(pdfs_dir_name())
    }

    /// 确保 PDF 目录存在
    pub fn ensure_dir(app_data_dir: &Path) -> Result<PathBuf, AppError> {
        let dir = Self::pdfs_dir(app_data_dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// 把一个 PDF 文件导入为笔记：抽取文本 → 创建笔记 → 拷贝原文件 → 更新 pdf_path
    pub fn import_one(
        app_data_dir: &Path,
        db: &Database,
        source_path: &str,
        folder_id: Option<i64>,
    ) -> Result<Note, AppError> {
        let source = Path::new(source_path);
        if !source.exists() {
            return Err(AppError::NotFound(format!(
                "PDF 文件不存在: {}",
                source_path
            )));
        }

        // 抽取并重建为 Tiptap 可继续编辑的结构化 HTML。
        let content = Self::extract_editable_html_only(source)?;

        Self::persist_editable_note(app_data_dir, db, source, folder_id, content)
    }

    /// 只抽取 PDF 并生成可编辑 HTML，不写数据库，供混合导入去重复用。
    pub(crate) fn extract_editable_html_only(source: &Path) -> Result<String, AppError> {
        let raw_text = extract_text_with_repair(source)?;
        let normalized = normalize_text(&raw_text);
        if is_likely_scanned_pdf(&normalized) {
            return Err(AppError::Custom(format!(
                "PDF 抽出文字过少（仅 {} 字），多半是扫描件 / 图片型 PDF（无文字层）。当前版本不内置 OCR;建议先用 Adobe Acrobat、ABBYY、mineru 等工具把 PDF 转成可搜索文本后再导入。",
                normalized.chars().count()
            )));
        }
        Ok(text_to_editable_html(&raw_text))
    }

    /// 使用已经抽取好的结构化正文创建笔记，避免混合导入重复解析 PDF。
    pub(crate) fn import_one_with_editable_html(
        app_data_dir: &Path,
        db: &Database,
        source: &Path,
        folder_id: Option<i64>,
        content: String,
    ) -> Result<Note, AppError> {
        Self::persist_editable_note(app_data_dir, db, source, folder_id, content)
    }

    fn persist_editable_note(
        app_data_dir: &Path,
        db: &Database,
        source: &Path,
        folder_id: Option<i64>,
        content: String,
    ) -> Result<Note, AppError> {
        // 2. 标题取源文件名（去后缀）
        let title = source
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "未命名 PDF".to_string());

        // 3. 先创建笔记，图片资产需要使用 note.id 隔离目录。
        let note = db.create_note(&NoteInput {
            title: title.clone(),
            content: content.clone(),
            folder_id,
        })?;

        // 图片提取失败不应阻止文字导入；原始 PDF 仍可通过原文预览查看全部内容。
        let content = append_pdf_images_fail_soft(source, app_data_dir, note.id, content);
        db.update_note_content(note.id, &content)?;

        // 4. 拷贝原 PDF 到 pdfs/<id>/<原文件名>.pdf
        //    用 note.id 作为子目录隔离避免重名；保留原文件名让用户在文件系统里也能识别
        let safe_name = sanitize_pdf_filename(source);
        let rel_path = format!("{}/{}/{}", pdfs_dir_name(), note.id, safe_name);
        let dst = app_data_dir.join(&rel_path);
        if let Some(parent) = dst.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("PDF 子目录创建失败（笔记已建）: {}", e);
                return db
                    .get_note(note.id)?
                    .ok_or_else(|| AppError::NotFound("刚创建的笔记查询失败".into()));
            }
        }
        if let Err(e) = std::fs::copy(source, &dst) {
            // 拷贝失败：笔记已经建好了也算导入成功，只是不关联 PDF
            log::warn!("PDF 原文件拷贝失败（笔记已建）: {}", e);
            return db
                .get_note(note.id)?
                .ok_or_else(|| AppError::NotFound("刚创建的笔记查询失败".into()));
        }

        // 5. 更新 source_file_path 和 source_file_type
        db.set_note_source_file(note.id, Some(&rel_path), Some("pdf"))?;

        // 6. 重新取完整 note 带 source_file_path 返回
        let note = db
            .get_note(note.id)?
            .ok_or_else(|| AppError::NotFound("刚创建的笔记查询失败".into()))?;
        Ok(note)
    }

    /// 用原始 PDF 重新生成结构化可编辑正文。标题、文件夹和来源附件保持不变。
    pub fn rebuild_editable_note(
        app_data_dir: &Path,
        db: &Database,
        note_id: i64,
    ) -> Result<Note, AppError> {
        let note = db
            .get_note(note_id)?
            .ok_or_else(|| AppError::NotFound(format!("笔记 {} 不存在", note_id)))?;
        if note.source_file_type.as_deref() != Some("pdf") {
            return Err(AppError::InvalidInput("当前笔记不是 PDF 导入文档".into()));
        }
        let relative_path = note
            .source_file_path
            .as_deref()
            .ok_or_else(|| AppError::NotFound("当前笔记未关联原始 PDF".into()))?;
        let source = Self::resolve_pdf_absolute_path(app_data_dir, relative_path)
            .ok_or_else(|| AppError::NotFound("原始 PDF 文件不存在".into()))?;
        let content = Self::extract_editable_html_only(&source)?;
        let content = append_pdf_images_fail_soft(&source, app_data_dir, note_id, content);
        db.update_note(
            note_id,
            &NoteInput {
                title: note.title,
                content,
                folder_id: note.folder_id,
            },
        )
    }

    /// 批量导入，收集每条结果（不中断整体流程）
    pub fn import_many(
        app_data_dir: &Path,
        db: &Database,
        source_paths: &[String],
        folder_id: Option<i64>,
    ) -> Vec<PdfImportResult> {
        source_paths
            .iter()
            .map(|p| match Self::import_one(app_data_dir, db, p, folder_id) {
                Ok(note) => PdfImportResult {
                    source_path: p.clone(),
                    note_id: Some(note.id),
                    title: Some(note.title),
                    error: None,
                },
                Err(e) => PdfImportResult {
                    source_path: p.clone(),
                    note_id: None,
                    title: None,
                    error: Some(e.to_string()),
                },
            })
            .collect()
    }

    /// 根据 note_id 解析出 PDF 绝对路径（不存在则返回 None）
    pub fn resolve_pdf_absolute_path(app_data_dir: &Path, pdf_path: &str) -> Option<PathBuf> {
        let abs = app_data_dir.join(pdf_path);
        if abs.exists() {
            Some(abs)
        } else {
            None
        }
    }

    /// 删除笔记关联的所有 PDF 文件（永久删除笔记时调用）。
    ///
    /// 新格式（方案 C）：删整个 `pdfs/<note_id>/` 子目录；
    /// 旧格式（`pdfs/<note_id>.pdf`）由 trash 服务的 source_file_path 单文件删除负责，
    /// 这里只关注新格式目录，互不冲突。
    pub fn delete_note_pdfs(app_data_dir: &Path, note_id: i64) -> Result<(), AppError> {
        let dir = Self::pdfs_dir(app_data_dir).join(note_id.to_string());
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)?;
            log::info!("已删除笔记 {} 的 PDF 子目录: {:?}", note_id, dir);
        }
        Ok(())
    }
}

fn append_pdf_images_fail_soft(
    source: &Path,
    app_data_dir: &Path,
    note_id: i64,
    content: String,
) -> String {
    match extract_pdf_images_to_assets(source, app_data_dir, note_id) {
        Ok(images) => append_pdf_image_section(content, &images),
        Err(error) => {
            log::warn!("PDF 图片提取失败，保留可编辑文字和原文附件: {}", error);
            content
        }
    }
}

fn append_pdf_image_section(mut content: String, images: &[ExtractedPdfImage]) -> String {
    if images.is_empty() {
        return content;
    }

    content.push_str("<hr><h2>文档图片</h2>");
    let mut current_page = None;
    for image in images {
        if current_page != Some(image.page_number) {
            current_page = Some(image.page_number);
            content.push_str(&format!("<h3>第 {} 页</h3>", image.page_number));
        }
        content.push_str(&format!(
            "<figure><img src=\"{}\" alt=\"第 {} 页图片 {}\"><figcaption>第 {} 页图片 {}（{} × {}）</figcaption></figure>",
            image.asset_url,
            image.page_number,
            image.image_number,
            image.page_number,
            image.image_number,
            image.width,
            image.height
        ));
    }
    content
}

#[cfg(desktop)]
fn extract_pdf_images_to_assets(
    source: &Path,
    app_data_dir: &Path,
    note_id: i64,
) -> Result<Vec<ExtractedPdfImage>, AppError> {
    let pdfium = PDFIUM
        .get()
        .ok_or_else(|| AppError::Custom("PDFium 尚未初始化，无法提取 PDF 图片".into()))?;
    let guard = pdfium
        .lock()
        .map_err(|_| AppError::Custom("PDFium 图片提取锁异常".into()))?;
    let document = guard
        .0
        .load_pdf_from_file(source, None)
        .map_err(|error| AppError::Custom(format!("PDF 图片解析失败: {}", error)))?;

    let mut extracted = Vec::new();
    let mut seen_hashes = HashSet::new();
    let mut total_bytes = 0usize;

    'pages: for (page_index, page) in document.pages().iter().enumerate() {
        let mut page_image_number = 0usize;
        for object in page.objects().iter() {
            let Some(image_object) = object.as_image_object() else {
                continue;
            };
            let (Ok(width), Ok(height)) = (image_object.width(), image_object.height()) else {
                continue;
            };
            if width < MIN_EXTRACTED_IMAGE_SIDE || height < MIN_EXTRACTED_IMAGE_SIDE {
                continue;
            }
            page_image_number += 1;

            let image_result = if width > MAX_EXTRACTED_IMAGE_WIDTH {
                image_object.get_processed_image_with_width(&document, MAX_EXTRACTED_IMAGE_WIDTH)
            } else {
                image_object.get_processed_image(&document)
            };
            let image = match image_result {
                Ok(image) => image,
                Err(error) => {
                    log::warn!(
                        "跳过 PDF 第 {} 页无法解码的图片 {}: {}",
                        page_index + 1,
                        page_image_number,
                        error
                    );
                    continue;
                }
            };

            let temp_path = std::env::temp_dir()
                .join(format!("firstwork-pdf-image-{}.png", uuid::Uuid::new_v4()));
            if let Err(error) = image.save(&temp_path) {
                let _ = std::fs::remove_file(&temp_path);
                log::warn!(
                    "跳过 PDF 第 {} 页无法编码的图片 {}: {}",
                    page_index + 1,
                    page_image_number,
                    error
                );
                continue;
            }
            let bytes = match std::fs::read(&temp_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    let _ = std::fs::remove_file(&temp_path);
                    log::warn!(
                        "跳过 PDF 第 {} 页临时文件读取失败的图片 {}: {}",
                        page_index + 1,
                        page_image_number,
                        error
                    );
                    continue;
                }
            };
            let _ = std::fs::remove_file(&temp_path);

            let hash = format!("{:x}", Sha256::digest(&bytes));
            if !seen_hashes.insert(hash) {
                continue;
            }
            if total_bytes.saturating_add(bytes.len()) > MAX_EXTRACTED_IMAGE_BYTES {
                log::warn!("PDF 图片累计超过 100 MiB，已停止继续提取");
                break 'pages;
            }

            let page_number = page_index + 1;
            let absolute = match ImageService::save_bytes(
                app_data_dir,
                note_id,
                &format!("pdf-page-{}-image-{}.png", page_number, page_image_number),
                &bytes,
            ) {
                Ok(absolute) => absolute,
                Err(error) => {
                    log::warn!(
                        "跳过 PDF 第 {} 页无法保存的图片 {}: {}",
                        page_number,
                        page_image_number,
                        error
                    );
                    continue;
                }
            };
            let Some(relative) = asset_path::abs_to_rel(Path::new(&absolute), app_data_dir) else {
                log::warn!(
                    "跳过 PDF 第 {} 页未落入受控目录的图片 {}",
                    page_number,
                    page_image_number
                );
                continue;
            };
            total_bytes += bytes.len();
            extracted.push(ExtractedPdfImage {
                page_number,
                image_number: page_image_number,
                asset_url: format!("kb-asset://{}", relative),
                width,
                height,
            });

            if extracted.len() >= MAX_EXTRACTED_PDF_IMAGES {
                log::warn!(
                    "PDF 图片数量达到上限 {}，已停止继续提取",
                    MAX_EXTRACTED_PDF_IMAGES
                );
                break 'pages;
            }
        }
    }

    Ok(extracted)
}

#[cfg(mobile)]
fn extract_pdf_images_to_assets(
    _source: &Path,
    _app_data_dir: &Path,
    _note_id: i64,
) -> Result<Vec<ExtractedPdfImage>, AppError> {
    Ok(Vec::new())
}

/// PDF 原文件名清洗：保留中文 / 字母 / 数字 / 常见标点，过滤跨平台不安全字符。
///
/// 处理：
///  - 跨平台文件系统不允许的字符 `/ \ : * ? " < > |` 替换为 `_`
///  - 控制字符（0x00-0x1F、0x7F）一律删除
///  - 前后空白 / 点号 trim（Windows 不允许文件名以点结尾）
///  - 去后缀后限长 200 字符（仍预留给重名后缀），保留 `.pdf` 扩展
///  - 兜底：清洗后为空时返回 `untitled.pdf`
fn sanitize_pdf_filename(source: &Path) -> String {
    let raw_stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let cleaned: String = raw_stem
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            // 控制字符直接删
            c if (c as u32) < 0x20 || c == '\u{007F}' => '\0',
            other => other,
        })
        .filter(|c| *c != '\0')
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    let limited: String = trimmed.chars().take(200).collect();
    // 全部都是下划线（说明原名几乎全是非法字符）也视作无效，避免出现 "____.pdf"
    let all_underscore = !limited.is_empty() && limited.chars().all(|c| c == '_');
    if limited.is_empty() || all_underscore {
        "untitled.pdf".to_string()
    } else {
        format!("{}.pdf", limited)
    }
}

/// 抽取 PDF 文本；若首轮失败且错误属于 xref 类损坏，尝试字节级修复后重跑一次。
/// pdf_extract 0.9 对不认识的字体编码（如 CNKI 常见的 `GBK-EUC-H`）会 `panic!`，
/// 所以这里用 `catch_unwind` 全程兜底，避免整个 Tauri 后端进程被一份坏 PDF 击穿。
///
/// 完整失败链：
/// 1. `pdf-extract` 直抽
/// 2. xref 错误 → 字节级修复后重抽
/// 3. 上面两步都失败 → PDFium fallback（唯一能解中文 CMap 的路径）
/// 4. PDFium 也抽不出 → 返回友好错误
fn extract_text_with_repair(source: &Path) -> Result<String, AppError> {
    let first_err = match safe_extract_text(source) {
        Ok(t) if !t.trim().is_empty() => return Ok(t),
        Ok(_) => "pdf-extract 返回空文本".to_string(),
        Err(e) => e,
    };

    // 若是 xref 类错误，先试字节级修复
    let second_err = if is_xref_error(&first_err) {
        log::warn!("PDF 首轮抽取失败（xref 错误），尝试修复重试: {}", first_err);
        match try_extract_after_repair(source) {
            Ok(t) if !t.trim().is_empty() => {
                log::info!("PDF xref 修复成功，已抽取文本");
                return Ok(t);
            }
            Ok(_) => "修复后 pdf-extract 返回空文本".to_string(),
            Err(e) => e,
        }
    } else {
        first_err.clone()
    };

    // 最后的手段：PDFium fallback（仅桌面端）
    #[cfg(desktop)]
    {
        match extract_with_pdfium(source) {
            Ok(t) if !t.trim().is_empty() => {
                log::info!(
                    "PDF 通过 PDFium fallback 抽取成功（pdf-extract 路径报错: {}）",
                    first_err
                );
                Ok(t)
            }
            Ok(_) => {
                log::warn!("PDFium 抽取返回空文本（可能是扫描件 / 无文本层）");
                Err(AppError::Custom(
                    "PDF 无文本层（可能是纯图片扫描件），请先 OCR 后再导入".into(),
                ))
            }
            Err(pdfium_err) => {
                log::warn!(
                    "PDF 全部路径失败: pdf-extract={}, repair={}, pdfium={}",
                    first_err,
                    second_err,
                    pdfium_err
                );
                // 友好提示基于 pdf-extract 的错误文本（用户通常装的是 pdf-extract 路径）
                Err(AppError::Custom(friendly_extract_error(&second_err)))
            }
        }
    }

    // 移动端无 PDFium fallback，pdf-extract 路径都失败时直接返回友好错误
    #[cfg(mobile)]
    {
        log::warn!(
            "PDF 抽取失败（移动端无 PDFium fallback）: pdf-extract={}, repair={}",
            first_err,
            second_err
        );
        Err(AppError::Custom(friendly_extract_error(&second_err)))
    }
}

/// 用 PDFium 抽取 PDF 文本（逐页拼接）。PDFium 未初始化时返回 Err。
/// 仅桌面端：移动端无 PDFium 绑定。
#[cfg(desktop)]
fn extract_with_pdfium(source: &Path) -> Result<String, String> {
    let mutex = PDFIUM
        .get()
        .ok_or_else(|| "PDFium 未初始化（dll 加载失败）".to_string())?;
    let guard = mutex
        .lock()
        .map_err(|e| format!("PDFium 锁被毒化: {}", e))?;
    let pdfium = &guard.0;

    let doc = pdfium
        .load_pdf_from_file(source, None)
        .map_err(|e| format!("PDFium 打开 PDF 失败: {}", e))?;

    let mut pages_text = Vec::new();
    for page in doc.pages().iter() {
        let page_text = page
            .text()
            .map_err(|e| format!("PDFium 读取页面文本失败: {}", e))?;
        pages_text.push(page_text.all());
    }
    Ok(pages_text.join("\n\n"))
}

/// 用 `catch_unwind` 包裹 pdf_extract::extract_text，把 panic 也转成普通错误返回
fn safe_extract_text(path: &Path) -> Result<String, String> {
    let path = path.to_path_buf();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text(&path)
    }));
    match result {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(e.to_string()),
        Err(panic_payload) => {
            let msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "PDF 解析发生未知 panic".to_string()
            };
            Err(format!("pdf-extract panic: {}", msg))
        }
    }
}

/// 读入整份 PDF → 做字节级修复 → 写到临时文件 → 重新调用 pdf_extract
fn try_extract_after_repair(source: &Path) -> Result<String, String> {
    let raw = std::fs::read(source).map_err(|e| e.to_string())?;
    let repaired = repair_pdf_bytes(&raw);

    // 临时文件放在系统临时目录，名字加 PID + 源文件 stem 防冲突
    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("src");
    // 只保留 ASCII 字母数字，避免临时目录路径里混中文被某些环境拒绝
    let safe_stem: String = stem
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(32)
        .collect();
    let tmp_name = format!("kb_pdf_repair_{}_{}.pdf", std::process::id(), safe_stem);
    let tmp_path = std::env::temp_dir().join(tmp_name);

    std::fs::write(&tmp_path, &repaired).map_err(|e| e.to_string())?;
    let result = safe_extract_text(&tmp_path);
    // 清理临时文件（失败不阻断）
    let _ = std::fs::remove_file(&tmp_path);
    result
}

/// 判定 pdf_extract 的错误信息是否属于"可修复"的 xref/trailer 类
fn is_xref_error(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.contains("cross reference")
        || lower.contains("xref")
        || lower.contains("invalid start value")
        || lower.contains("trailer")
}

/// 字节级修复 PDF 中常见的 CNKI（知网）/非标准工具写出的格式问题
///
/// 处理两类违规：
/// 1. xref 头同行：`xref N M\n` → `xref\nN M\n`（PDF 1.7 §7.5.4 要求 xref 单独一行）
/// 2. `%%EOF` 之后有额外字节（CNKI 的 `WebFastLoad<FileProperty>...`）→ 截断
pub(crate) fn repair_pdf_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    fix_xref_header_inline(&mut out);
    truncate_after_last_eof(&mut out);
    out
}

/// 把 `xref ` 后紧跟数字的位置的空格替换为换行符
/// 只处理 `xref` 关键字且前后无字母数字粘连（避免误伤出现在流数据里的字节串）
fn fix_xref_header_inline(data: &mut [u8]) {
    let pat = b"xref ";
    let mut i = 0;
    while i + pat.len() < data.len() {
        if &data[i..i + pat.len()] == pat {
            // 前一个字节必须是换行/回车/空白，才是真正的 xref 关键字
            let prev_ok = i == 0 || matches!(data[i - 1], b'\n' | b'\r' | b' ' | b'\t');
            let next_byte = data[i + pat.len()];
            if prev_ok && next_byte.is_ascii_digit() {
                // i+4 是 "xref " 里的空格，替换为 \n
                data[i + 4] = b'\n';
            }
            i += pat.len();
        } else {
            i += 1;
        }
    }
}

/// 保留最后一个 `%%EOF` 及其后的一个换行符，截掉后续所有字节
fn truncate_after_last_eof(data: &mut Vec<u8>) {
    let eof = b"%%EOF";
    let pos = match data.windows(eof.len()).rposition(|w| w == eof) {
        Some(p) => p,
        None => return,
    };
    let after = pos + eof.len();
    let mut keep = after;
    if keep < data.len() && data[keep] == b'\r' {
        keep += 1;
        if keep < data.len() && data[keep] == b'\n' {
            keep += 1;
        }
    } else if keep < data.len() && data[keep] == b'\n' {
        keep += 1;
    }
    if keep < data.len() {
        data.truncate(keep);
    }
}

/// 把 pdf_extract 原始错误文本转成面向用户的友好提示
///
/// 常见失败类型：
/// - 字体编码 panic（如 `unsupported encoding GBK-EUC-H`）：CNKI 知网/方正等用了非标准中文 CMap
/// - xref / trailer 相关：PDF 交叉引用表损坏
/// - Encrypt / encryption：加密或带权限限制
/// - 其他：走通用提示，保留原文便于排查
fn friendly_extract_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    // 字体编码优先判断（CNKI PDF 修完 xref 后最常撞到的就是这个）
    if lower.contains("unsupported encoding")
        || lower.contains("cmap")
        || lower.contains("gbk-euc")
        || lower.contains("gb-euc")
        || (lower.contains("panic") && lower.contains("encoding"))
    {
        format!(
            "PDF 使用了当前版本不支持的中文字体编码（常见于中国知网下载件）。解决方案：用 Chrome/Edge 打开该 PDF，按 Ctrl+P → 目标选「另存为 PDF」→ 保存新文件后再导入即可。原始错误: {}",
            raw
        )
    } else if lower.contains("cross reference")
        || lower.contains("xref")
        || lower.contains("invalid start value")
        || lower.contains("trailer")
    {
        format!(
            "PDF 交叉引用表损坏，无法解析。请用 Chrome/Edge 打开该 PDF，然后「打印 → 另存为 PDF」生成新文件后再导入。原始错误: {}",
            raw
        )
    } else if lower.contains("encrypt") {
        format!(
            "PDF 已加密或有权限限制，暂不支持导入。请先解除加密后再试。原始错误: {}",
            raw
        )
    } else {
        format!("PDF 文本抽取失败: {}", raw)
    }
}

/// T-B06: 启发式判定一份 PDF 是否多半是扫描件（无文字层）
///
/// 阈值 50 char 是经验值：
/// - 普通 PDF 即便只有 1 页，正文也常 100+ 字符
/// - 仅页码 / 页眉的"几乎空"PDF 一般不会被用户用来"导入笔记"
/// - 扫描件 pdf-extract 几乎必然返回空字符串或纯空白
///
/// 仅看 trim 后的 char 数；不分中英文。
fn is_likely_scanned_pdf(text: &str) -> bool {
    text.trim().chars().count() < 50
}

const PAGE_BREAK_MARKER: &str = "[[FIRSTWORK_PDF_PAGE_BREAK]]";

#[derive(Debug, PartialEq, Eq)]
enum EditablePdfBlock {
    Heading(u8, String),
    Paragraph(String),
    UnorderedList(Vec<String>),
    OrderedList(Vec<String>),
    PageBreak,
}

/// 把 PDF 的视觉行重建为可编辑 HTML，而不是把每个换行机械转换成 `<br/>`。
///
/// 这是保守的本地规则：恢复标题、段落、列表和分页，清理页码与跨页重复页眉页脚；
/// 不调用 AI，不会补写原文不存在的内容。
pub(crate) fn text_to_editable_html(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let lines = prepare_pdf_lines(raw);
    let blocks = build_editable_blocks(&lines);
    blocks
        .into_iter()
        .map(|block| match block {
            EditablePdfBlock::Heading(level, text) => {
                format!("<h{level}>{}</h{level}>", html_escape(&text))
            }
            EditablePdfBlock::Paragraph(text) => format!("<p>{}</p>", html_escape(&text)),
            EditablePdfBlock::UnorderedList(items) => format!(
                "<ul>{}</ul>",
                items
                    .into_iter()
                    .map(|item| format!("<li><p>{}</p></li>", html_escape(&item)))
                    .collect::<Vec<_>>()
                    .join("")
            ),
            EditablePdfBlock::OrderedList(items) => format!(
                "<ol>{}</ol>",
                items
                    .into_iter()
                    .map(|item| format!("<li><p>{}</p></li>", html_escape(&item)))
                    .collect::<Vec<_>>()
                    .join("")
            ),
            EditablePdfBlock::PageBreak => "<hr>".to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prepare_pdf_lines(raw: &str) -> Vec<String> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let pages = normalized.split('\u{000C}').collect::<Vec<_>>();
    let cleaned_pages = pages
        .iter()
        .map(|page| {
            page.lines()
                .map(clean_line)
                .map(|line| line.trim().to_string())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let repeated_edges = repeated_page_edges(&cleaned_pages);
    let mut output = Vec::new();

    for (page_index, lines) in cleaned_pages.iter().enumerate() {
        let non_empty = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !line.is_empty())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let edge_indices = non_empty
            .iter()
            .take(3)
            .chain(non_empty.iter().rev().take(3))
            .copied()
            .collect::<HashSet<_>>();

        for (line_index, line) in lines.iter().enumerate() {
            if line.is_empty() {
                if output.last().is_some_and(|last: &String| !last.is_empty()) {
                    output.push(String::new());
                }
                continue;
            }
            if is_page_number(line)
                || (edge_indices.contains(&line_index) && repeated_edges.contains(line))
            {
                continue;
            }
            output.push(line.clone());
        }
        while output.last().is_some_and(String::is_empty) {
            output.pop();
        }
        if page_index + 1 < cleaned_pages.len() && !output.is_empty() {
            output.push(PAGE_BREAK_MARKER.to_string());
        }
    }
    output
}

fn repeated_page_edges(pages: &[Vec<String>]) -> HashSet<String> {
    if pages.len() < 2 {
        return HashSet::new();
    }
    let mut counts = HashMap::<String, usize>::new();
    for page in pages {
        let non_empty = page
            .iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let candidates = non_empty
            .iter()
            .take(2)
            .chain(non_empty.iter().rev().take(2));
        let mut seen_on_page = HashSet::new();
        for candidate in candidates {
            if candidate.chars().count() <= 100 && seen_on_page.insert((*candidate).clone()) {
                *counts.entry((*candidate).clone()).or_default() += 1;
            }
        }
    }
    let threshold = 2usize.max(pages.len().div_ceil(2));
    counts
        .into_iter()
        .filter_map(|(line, count)| (count >= threshold).then_some(line))
        .collect()
}

fn build_editable_blocks(lines: &[String]) -> Vec<EditablePdfBlock> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut unordered = Vec::new();
    let mut ordered = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let previous_blank =
            index == 0 || lines[index - 1].is_empty() || lines[index - 1] == PAGE_BREAK_MARKER;
        let next_blank = index + 1 == lines.len()
            || lines[index + 1].is_empty()
            || lines[index + 1] == PAGE_BREAK_MARKER;

        if line == PAGE_BREAK_MARKER {
            flush_pdf_buffers(&mut blocks, &mut paragraph, &mut unordered, &mut ordered);
            if !matches!(blocks.last(), Some(EditablePdfBlock::PageBreak)) {
                blocks.push(EditablePdfBlock::PageBreak);
            }
            continue;
        }
        if line.is_empty() {
            flush_pdf_buffers(&mut blocks, &mut paragraph, &mut unordered, &mut ordered);
            continue;
        }
        if let Some(level) = heading_level(line, previous_blank && next_blank) {
            flush_pdf_buffers(&mut blocks, &mut paragraph, &mut unordered, &mut ordered);
            blocks.push(EditablePdfBlock::Heading(level, line.clone()));
            continue;
        }
        if let Some(item) = unordered_item(line) {
            flush_paragraph_and_ordered(&mut blocks, &mut paragraph, &mut ordered);
            unordered.push(item.to_string());
            continue;
        }
        if let Some(item) = ordered_item(line) {
            flush_paragraph_and_unordered(&mut blocks, &mut paragraph, &mut unordered);
            ordered.push(item.to_string());
            continue;
        }

        flush_lists(&mut blocks, &mut unordered, &mut ordered);
        append_wrapped_line(&mut paragraph, line);
        if paragraph.chars().count() >= 800 {
            flush_paragraph(&mut blocks, &mut paragraph);
        }
    }
    flush_pdf_buffers(&mut blocks, &mut paragraph, &mut unordered, &mut ordered);
    while matches!(blocks.last(), Some(EditablePdfBlock::PageBreak)) {
        blocks.pop();
    }
    blocks
}

fn flush_pdf_buffers(
    blocks: &mut Vec<EditablePdfBlock>,
    paragraph: &mut String,
    unordered: &mut Vec<String>,
    ordered: &mut Vec<String>,
) {
    flush_paragraph(blocks, paragraph);
    flush_lists(blocks, unordered, ordered);
}

fn flush_paragraph_and_ordered(
    blocks: &mut Vec<EditablePdfBlock>,
    paragraph: &mut String,
    ordered: &mut Vec<String>,
) {
    flush_paragraph(blocks, paragraph);
    if !ordered.is_empty() {
        blocks.push(EditablePdfBlock::OrderedList(std::mem::take(ordered)));
    }
}

fn flush_paragraph_and_unordered(
    blocks: &mut Vec<EditablePdfBlock>,
    paragraph: &mut String,
    unordered: &mut Vec<String>,
) {
    flush_paragraph(blocks, paragraph);
    if !unordered.is_empty() {
        blocks.push(EditablePdfBlock::UnorderedList(std::mem::take(unordered)));
    }
}

fn flush_paragraph(blocks: &mut Vec<EditablePdfBlock>, paragraph: &mut String) {
    if !paragraph.trim().is_empty() {
        blocks.push(EditablePdfBlock::Paragraph(
            std::mem::take(paragraph).trim().to_string(),
        ));
    }
}

fn flush_lists(
    blocks: &mut Vec<EditablePdfBlock>,
    unordered: &mut Vec<String>,
    ordered: &mut Vec<String>,
) {
    if !unordered.is_empty() {
        blocks.push(EditablePdfBlock::UnorderedList(std::mem::take(unordered)));
    }
    if !ordered.is_empty() {
        blocks.push(EditablePdfBlock::OrderedList(std::mem::take(ordered)));
    }
}

fn append_wrapped_line(paragraph: &mut String, next: &str) {
    if paragraph.is_empty() {
        paragraph.push_str(next.trim());
        return;
    }
    let previous = paragraph.chars().last();
    let following = next.trim().chars().next();
    if paragraph.ends_with('-')
        && following.is_some_and(|ch| ch.is_ascii_lowercase())
        && previous == Some('-')
    {
        paragraph.pop();
    } else if !previous.zip(following).is_some_and(|(left, right)| {
        is_cjk(left) || is_cjk(right) || left.is_whitespace() || right.is_whitespace()
    }) {
        paragraph.push(' ');
    }
    paragraph.push_str(next.trim());
}

fn heading_level(line: &str, standalone: bool) -> Option<u8> {
    let trimmed = line.trim();
    let char_count = trimmed.chars().count();
    if char_count == 0 || char_count > 80 || ends_sentence(trimmed) {
        return None;
    }
    if trimmed.starts_with('第')
        && (trimmed.contains('章') || trimmed.contains('节') || trimmed.contains('篇'))
    {
        return Some(2);
    }
    if matches!(
        trimmed,
        "摘要" | "关键词" | "目录" | "引言" | "前言" | "结论" | "参考文献" | "附录"
    ) {
        return Some(2);
    }
    if starts_numbered_heading(trimmed)
        && (standalone || trimmed.matches('.').count() + trimmed.matches('．').count() >= 2)
    {
        return Some(if trimmed.matches('.').count() >= 2 {
            3
        } else {
            2
        });
    }
    if starts_chinese_heading(trimmed) {
        return Some(3);
    }
    if standalone && char_count <= 35 {
        return Some(2);
    }
    None
}

fn starts_numbered_heading(line: &str) -> bool {
    let mut saw_digit = false;
    let mut saw_separator = false;
    for ch in line.chars().take(12) {
        if ch.is_ascii_digit() {
            saw_digit = true;
            continue;
        }
        if saw_digit && matches!(ch, '.' | '．') {
            saw_separator = true;
            continue;
        }
        if saw_digit && matches!(ch, '、' | ' ') {
            return saw_separator || ch == '、' || ch == ' ';
        }
        if saw_separator && !ch.is_ascii_digit() {
            return true;
        }
        return false;
    }
    false
}

fn starts_chinese_heading(line: &str) -> bool {
    const NUMERALS: &str = "一二三四五六七八九十";
    let chars = line.chars().collect::<Vec<_>>();
    (chars.len() >= 2 && NUMERALS.contains(chars[0]) && chars[1] == '、')
        || (chars.len() >= 3
            && matches!(chars[0], '(' | '（')
            && NUMERALS.contains(chars[1])
            && matches!(chars[2], ')' | '）'))
}

fn unordered_item(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    for prefix in ["• ", "● ", "○ ", "▪ ", "· ", "- ", "* "] {
        if let Some(value) = trimmed.strip_prefix(prefix) {
            return Some(value.trim());
        }
    }
    None
}

fn ordered_item(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let mut chars = trimmed.char_indices();
    let (_, first) = chars.next()?;
    let end = if matches!(first, '(' | '（') {
        let (_, digit) = chars.next()?;
        if !digit.is_ascii_digit() {
            return None;
        }
        let (close_index, close) = chars.next()?;
        if !matches!(close, ')' | '）') {
            return None;
        }
        close_index + close.len_utf8()
    } else if first.is_ascii_digit() {
        let mut number_end = first.len_utf8();
        for (index, ch) in chars {
            if ch.is_ascii_digit() {
                number_end = index + ch.len_utf8();
                continue;
            }
            if matches!(ch, '.' | '．' | '、') {
                number_end = index + ch.len_utf8();
                break;
            }
            return None;
        }
        number_end
    } else {
        return None;
    };
    let value = trimmed.get(end..)?.trim_start();
    (!value.is_empty()).then_some(value)
}

fn is_page_number(line: &str) -> bool {
    let compact = line.trim().replace(' ', "");
    if compact.chars().all(|ch| ch.is_ascii_digit()) && compact.len() <= 5 {
        return true;
    }
    let lower = compact.to_ascii_lowercase();
    if (lower.starts_with("page") && lower.contains("of"))
        || (compact.starts_with('第') && compact.ends_with('页'))
        || compact.split_once('/').is_some_and(|(left, right)| {
            left.chars().all(|ch| ch.is_ascii_digit())
                && right.chars().all(|ch| ch.is_ascii_digit())
        })
    {
        return true;
    }
    compact
        .strip_prefix('-')
        .and_then(|value| value.strip_suffix('-'))
        .is_some_and(|value| value.chars().all(|ch| ch.is_ascii_digit()))
}

fn ends_sentence(line: &str) -> bool {
    line.trim_end()
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '。' | '！' | '？' | '.' | '!' | '?' | ';' | '；'))
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// 规范化文本：清洗 pdf-extract 抽出的常见垃圾字符并修整结构
///
/// 处理顺序：
/// 1. 换行规范化（CRLF → LF）
/// 2. 逐行清洗：去零宽字符、行首 PUA/豆腐字符还原为 "• "、行内 PUA/替换字符删除
/// 3. 多余空行压成最多 2 个
fn normalize_text(raw: &str) -> String {
    let lf = raw.replace("\r\n", "\n").replace('\r', "\n");
    let cleaned: String = lf
        .split('\n')
        .map(clean_line)
        .collect::<Vec<_>>()
        .join("\n");
    collapse_blank_lines(&cleaned)
}

/// 单行清洗：处理零宽字符、行首项目符号字形、行内不可打印字符
fn clean_line(line: &str) -> String {
    // 1. 去零宽字符
    let no_zw: String = line.chars().filter(|c| !is_zero_width(*c)).collect();

    // 2. 行首处理：跳过前导空白，若开头是疑似项目符号字形（PUA / FFFD 等），还原成 "•"
    let leading_ws: String = no_zw.chars().take_while(|c| c.is_whitespace()).collect();
    let body = &no_zw[leading_ws.len()..];

    if let Some(first) = body.chars().next() {
        if is_likely_bullet_glyph(first) {
            // 吃掉连续多个 bullet 字形（PDF 有时一个 bullet 占多个字符）
            let bullet_end = body
                .char_indices()
                .find(|(_, c)| !is_likely_bullet_glyph(*c))
                .map(|(i, _)| i)
                .unwrap_or(body.len());
            let rest = &body[bullet_end..];
            return format!("{}• {}", leading_ws, strip_unprintable(rest).trim_start());
        }
    }

    // 3. 非项目符号行：仅做行内不可打印清洗
    format!("{}{}", leading_ws, strip_unprintable(body))
}

/// 删除行内的 PUA 区段字符与替换字符（这些是 pdf-extract 没解出的字形残留）
fn strip_unprintable(s: &str) -> String {
    s.chars()
        .filter(|&c| !is_pua(c) && c != '\u{FFFD}')
        .collect()
}

/// 0-宽字符（不可见但污染搜索/光标）
fn is_zero_width(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{2060}'
    )
}

/// Unicode Private Use Area（PDF 嵌入子集字体常用区段，无字形定义）
fn is_pua(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF)
}

/// 判断是否疑似"被错抽的项目符号字形"
///
/// PDF 里项目符号 `•` 在很多字体（如 Wingdings、Symbol、自制嵌入字体）
/// 走的是 PUA 字形，pdf-extract 输出 \uF0B7 / \uFFFD / 各种 PUA 码点。
fn is_likely_bullet_glyph(c: char) -> bool {
    is_pua(c) || c == '\u{FFFD}'
}

/// 把连续 3+ 个换行压成 2 个，整体 trim
fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pua_at_line_start_becomes_bullet() {
        let raw = "\u{E020} 将本软件作为独立产品销售\n普通段落";
        let out = normalize_text(raw);
        assert!(out.starts_with("• 将本软件作为独立产品销售"));
        assert!(out.contains("普通段落"));
    }

    #[test]
    fn fffd_at_line_start_becomes_bullet() {
        let raw = "\u{FFFD} 第一项\n\u{FFFD} 第二项";
        let out = normalize_text(raw);
        assert_eq!(out, "• 第一项\n• 第二项");
    }

    #[test]
    fn zero_width_chars_removed() {
        let raw = "正\u{200B}文\u{FEFF}内\u{200C}容";
        assert_eq!(normalize_text(raw), "正文内容");
    }

    #[test]
    fn inline_pua_stripped_normal_line_kept() {
        let raw = "正文里夹\u{E100}个 PUA";
        assert_eq!(normalize_text(raw), "正文里夹个 PUA");
    }

    #[test]
    fn excessive_blank_lines_collapsed() {
        let raw = "A\n\n\n\nB\n\n\n\n\nC";
        assert_eq!(normalize_text(raw), "A\n\nB\n\nC");
    }

    #[test]
    fn editable_html_recovers_headings_paragraphs_and_lists() {
        let raw = "第一章 项目概述\n\n这是第一行没有结束\n并且应当与下一行合并。\n\n• 目标一\n• 目标二\n\n1. 步骤一\n2. 步骤二";
        let html = text_to_editable_html(raw);
        assert!(html.contains("<h2>第一章 项目概述</h2>"));
        assert!(html.contains("<p>这是第一行没有结束并且应当与下一行合并。</p>"));
        assert!(html.contains("<ul><li><p>目标一</p></li><li><p>目标二</p></li></ul>"));
        assert!(html.contains("<ol><li><p>步骤一</p></li><li><p>步骤二</p></li></ol>"));
    }

    #[test]
    fn editable_html_removes_repeated_headers_footers_and_page_numbers() {
        let raw = "内部资料\n\n第一页正文内容足够长，用于验证页面结构整理。\n\n第 1 页\u{000C}内部资料\n\n第二页正文内容足够长，用于验证页面结构整理。\n\n第 2 页";
        let html = text_to_editable_html(raw);
        assert!(!html.contains("内部资料"));
        assert!(!html.contains("第 1 页"));
        assert!(!html.contains("第 2 页"));
        assert_eq!(html.matches("<hr>").count(), 1);
        assert!(html.contains("第一页正文内容"));
        assert!(html.contains("第二页正文内容"));
    }

    #[test]
    fn editable_html_escapes_pdf_text() {
        let html = text_to_editable_html("结论\n\nA < B & B > C。");
        assert!(html.contains("A &lt; B &amp; B &gt; C。"));
    }

    #[test]
    fn image_section_groups_images_by_page_with_portable_asset_urls() {
        let images = vec![
            ExtractedPdfImage {
                page_number: 1,
                image_number: 1,
                asset_url: "kb-asset://dev-kb_assets/images/42/page-1.png".into(),
                width: 640,
                height: 480,
            },
            ExtractedPdfImage {
                page_number: 2,
                image_number: 1,
                asset_url: "kb-asset://dev-kb_assets/images/42/page-2.png".into(),
                width: 800,
                height: 600,
            },
        ];

        let html = append_pdf_image_section("<p>正文</p>".into(), &images);

        assert!(html.starts_with("<p>正文</p><hr><h2>文档图片</h2>"));
        assert_eq!(html.matches("<h3>第 ").count(), 2);
        assert_eq!(html.matches("<figure>").count(), 2);
        assert!(html.contains("kb-asset://dev-kb_assets/images/42/page-1.png"));
        assert!(!html.contains("D:\\"));
    }

    #[test]
    fn empty_image_list_leaves_editable_content_unchanged() {
        let original = "<h2>标题</h2><p>正文</p>".to_string();
        assert_eq!(append_pdf_image_section(original.clone(), &[]), original);
    }

    // ─── PDF 字节级修复测试 ──────────────────────────────

    #[test]
    fn is_xref_error_matches_real_lopdf_message() {
        let msg = "PDF error: failed parsing cross reference table: invalid start value";
        assert!(is_xref_error(msg));
    }

    #[test]
    fn is_xref_error_ignores_unrelated_errors() {
        assert!(!is_xref_error("Encrypted PDF is not supported"));
        assert!(!is_xref_error("Unknown font encoding"));
    }

    #[test]
    fn friendly_error_recognizes_gbk_encoding_panic() {
        let msg = "pdf-extract panic: unsupported encoding GBK-EUC-H";
        let out = friendly_extract_error(msg);
        assert!(out.contains("中文字体编码"));
        assert!(out.contains("Chrome") || out.contains("Edge"));
    }

    #[test]
    fn friendly_error_recognizes_xref_error() {
        let msg = "PDF error: failed parsing cross reference table: invalid start value";
        let out = friendly_extract_error(msg);
        assert!(out.contains("交叉引用表损坏"));
    }

    #[test]
    fn friendly_error_recognizes_encrypted_pdf() {
        let msg = "PDF error: document is encrypted";
        let out = friendly_extract_error(msg);
        assert!(out.contains("加密"));
    }

    #[test]
    fn fix_xref_header_converts_inline_to_canonical() {
        // CNKI 常见：xref 0 3\n...
        let mut data = b"header\nxref 0 3\n0000000000 65535 f\ntrailer".to_vec();
        fix_xref_header_inline(&mut data);
        assert_eq!(
            data,
            b"header\nxref\n0 3\n0000000000 65535 f\ntrailer".to_vec()
        );
    }

    #[test]
    fn fix_xref_header_leaves_canonical_form_untouched() {
        let original = b"header\nxref\n0 3\n0000000000 65535 f\ntrailer".to_vec();
        let mut data = original.clone();
        fix_xref_header_inline(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn fix_xref_header_does_not_touch_xref_keyword_inside_word() {
        // "xxref 0 5" 不是关键字（前字节是 'x'），不应该被改
        let original = b"xxref 0 5\n".to_vec();
        let mut data = original.clone();
        fix_xref_header_inline(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn truncate_after_last_eof_strips_trailing_garbage() {
        let mut data =
            b"%PDF-1.6\nsome content\n%%EOF\nWebFastLoad<FileProperty>trash</FileProperty>"
                .to_vec();
        truncate_after_last_eof(&mut data);
        assert_eq!(data, b"%PDF-1.6\nsome content\n%%EOF\n".to_vec());
    }

    #[test]
    fn truncate_after_last_eof_keeps_crlf() {
        let mut data = b"%PDF-1.6\n%%EOF\r\nGARBAGE".to_vec();
        truncate_after_last_eof(&mut data);
        assert_eq!(data, b"%PDF-1.6\n%%EOF\r\n".to_vec());
    }

    #[test]
    fn truncate_after_last_eof_noop_when_clean() {
        let original = b"%PDF-1.6\n%%EOF\n".to_vec();
        let mut data = original.clone();
        truncate_after_last_eof(&mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn repair_pdf_bytes_fixes_cnki_style_document() {
        // 模拟 CNKI 输出：xref 和头同行 + %%EOF 后附加元数据
        let input = b"%PDF-1.6\n\
                      3 0 obj\nendobj\n\
                      xref 0 2\n0000000000 65535 f\n0000000015 00000 n\n\
                      trailer\n<<>>\nstartxref\n20\n%%EOF\n\
                      WebFastLoad<FileProperty>junk</FileProperty>"
            .to_vec();
        let out = repair_pdf_bytes(&input);
        // xref 必须被换行
        assert!(out.windows(5).any(|w| w == b"xref\n"));
        assert!(!out.windows(5).any(|w| w == b"xref "));
        // %%EOF 后不允许再出现 WebFastLoad
        let tail = String::from_utf8_lossy(&out);
        assert!(!tail.contains("WebFastLoad"));
        assert!(tail.ends_with("%%EOF\n"));
    }

    // ─── T-B06 扫描件检测 ─────────────────────────────

    #[test]
    fn scanned_pdf_empty_text_detected() {
        assert!(is_likely_scanned_pdf(""));
        assert!(is_likely_scanned_pdf("   \n  \t  "));
    }

    #[test]
    fn scanned_pdf_only_page_number_detected() {
        // 只有页码 / 页眉的极简 PDF 也算扫描件
        assert!(is_likely_scanned_pdf("1\n\n2\n\n3"));
        assert!(is_likely_scanned_pdf("Page 1 of 10"));
    }

    #[test]
    fn normal_pdf_not_detected_as_scanned() {
        // 一段正常正文（>= 50 字符）不应被误判为扫描件
        let normal_text = "这是一份正常的 PDF 文档，包含了足够多的中英文混合文字内容，
应该能够顺利被识别为有完整文字层的可导入 PDF 文件，不会被错误判定为扫描件。";
        assert!(!is_likely_scanned_pdf(normal_text));

        let english =
            "This is a normal PDF document with enough text content to pass the scanned-PDF detection threshold.";
        assert!(!is_likely_scanned_pdf(english));
    }

    // ─── sanitize_pdf_filename 测试（方案 C 路径生成依赖） ─────────────

    #[test]
    fn sanitize_keeps_chinese_and_normal_chars() {
        let p = Path::new("D:/dl/管理视角读故事-绩效考核.pdf");
        assert_eq!(sanitize_pdf_filename(p), "管理视角读故事-绩效考核.pdf");
    }

    #[test]
    fn sanitize_replaces_unsafe_chars() {
        // 不含路径分隔符的单段名，Windows 不允许的字符全部转 _
        // (`/` `\` 是 Path 分隔符不能放在文件名里测，已被 file_stem 切掉)
        let p = Path::new(r#"D:/dl/a:b*c?d"e<f>g|h.pdf"#);
        assert_eq!(sanitize_pdf_filename(p), "a_b_c_d_e_f_g_h.pdf");
    }

    #[test]
    fn sanitize_strips_control_chars() {
        // 模拟带换行 / 制表符的文件名（极罕见但理论可能）
        let p = Path::new("D:/dl/abc\u{0007}\tdef.pdf");
        assert_eq!(sanitize_pdf_filename(p), "abcdef.pdf");
    }

    #[test]
    fn sanitize_trims_trailing_dot_and_space() {
        let p = Path::new("D:/dl/  hello..  .pdf");
        // file_stem 切掉 .pdf 后是 "  hello..  "，trim 空白 + 去末尾点 → "hello"
        assert_eq!(sanitize_pdf_filename(p), "hello.pdf");
    }

    #[test]
    fn sanitize_falls_back_to_untitled_when_empty() {
        // 全是非法字符 → 清洗后空串 → 兜底
        let p = Path::new(r#"D:/dl/?/<>|".pdf"#);
        assert_eq!(sanitize_pdf_filename(p), "untitled.pdf");
    }

    #[test]
    fn sanitize_truncates_excessively_long_names() {
        // 200 个汉字 + .pdf 是合理上限
        let stem: String = "字".repeat(300);
        let raw = format!("D:/dl/{}.pdf", stem);
        let p = Path::new(&raw);
        let out = sanitize_pdf_filename(p);
        // 200 个汉字 + ".pdf"
        assert_eq!(out.chars().count(), 204);
        assert!(out.ends_with(".pdf"));
    }

    #[test]
    fn boundary_exactly_50_chars_not_scanned() {
        // 50 字符正好应当通过（阈值是 < 50）
        let text: String = "a".repeat(50);
        assert!(!is_likely_scanned_pdf(&text));
        let text: String = "a".repeat(49);
        assert!(is_likely_scanned_pdf(&text));
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
