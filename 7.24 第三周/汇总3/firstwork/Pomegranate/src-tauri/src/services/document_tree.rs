use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use zip::ZipArchive;

use crate::database::Database;
use crate::error::AppError;
use crate::services::document_sources::{
    DocumentSource, DocumentSourceListInput, DocumentSourceService, CATEGORY_LEARNING_UPLOAD,
    CATEGORY_LOCAL,
};

const PARSER_VERSION: &str = "unified-document-v1";
const SOURCE_TYPE_LOCAL: &str = "localKnowledgeBase";
const SOURCE_TYPE_LEARNING: &str = "learningUpload";
const SOURCE_TYPE_USER: &str = "userDocument";
const UNCATEGORIZED_NAME: &str = "未分类";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTreeNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub node_type: String,
    pub source_type: String,
    pub system_folder: bool,
    pub folder_id: Option<i64>,
    pub document_source_id: Option<i64>,
    pub file_type: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub parse_status: Option<String>,
    pub parse_message: Option<String>,
    pub can_use_as_learning_source: bool,
    pub child_count: usize,
    pub children: Vec<DocumentTreeNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTreeResult {
    pub roots: Vec<DocumentTreeNode>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedLearningDocument {
    pub document_id: i64,
    pub source_file: String,
    pub source_folder: String,
    pub source_type: String,
    pub file_type: String,
    pub parsed_text: String,
    pub parse_status: String,
    pub parse_message: String,
}

#[derive(Debug, Clone)]
struct ParseCache {
    status: String,
    text: String,
    message: String,
    source_modified_at: i64,
    content_hash: String,
    parser_version: String,
}

#[derive(Debug, Clone)]
struct ResolvedDocument {
    document_id: i64,
    name: String,
    folder_name: String,
    source_type: String,
    file_type: String,
    mime_type: String,
    size: i64,
    content_hash: String,
    path: Option<PathBuf>,
    note_content: Option<String>,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct FlatFolder {
    id: i64,
    name: String,
    parent_id: Option<i64>,
    sort_order: i32,
}

#[derive(Debug, Clone)]
struct FlatNote {
    id: i64,
    title: String,
    content: String,
    folder_id: Option<i64>,
    source_file_path: Option<String>,
    source_file_type: Option<String>,
    content_hash: String,
}

pub struct DocumentTreeService;

impl DocumentTreeService {
    pub fn prepare_learning_uploads(db: &Database, data_dir: &Path) -> Vec<String> {
        let sources = match DocumentSourceService::list(
            db,
            data_dir,
            DocumentSourceListInput {
                category: Some(CATEGORY_LEARNING_UPLOAD.to_string()),
                ..DocumentSourceListInput::default()
            },
        ) {
            Ok(result) => result.sources,
            Err(error) => return vec![error.to_string()],
        };

        let mut warnings = Vec::new();
        for source in sources {
            if let Err(error) = Self::parsed_document(db, data_dir, source.id, false) {
                warnings.push(format!("预解析“{}”失败：{}", source.original_file_name, error));
            }
        }
        warnings
    }

    pub fn list(
        db: &Database,
        data_dir: &Path,
        force_refresh: bool,
    ) -> Result<DocumentTreeResult, AppError> {
        let source_result =
            DocumentSourceService::list(db, data_dir, DocumentSourceListInput::default())?;
        let folders = load_folders(db)?;
        let notes = load_notes(db)?;
        let folder_names = folders
            .iter()
            .map(|folder| (folder.id, folder.name.clone()))
            .collect::<HashMap<_, _>>();

        let linked_source_paths = source_result
            .sources
            .iter()
            .map(|source| source.stored_relative_path.clone())
            .collect::<HashSet<_>>();
        let visible_notes = notes
            .into_iter()
            .filter(|note| {
                note.source_file_path
                    .as_ref()
                    .map(|path| !linked_source_paths.contains(path))
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        let mut warnings = source_result.warnings;
        let mut local_children = Vec::new();
        let mut learning_children = Vec::new();
        for source in &source_result.sources {
            let resolved = resolve_source(source.clone(), data_dir, source.category.clone());
            let node = file_node(db, resolved, None, force_refresh, &mut warnings);
            if source.category == CATEGORY_LOCAL {
                local_children.push(node);
            } else if source.category == CATEGORY_LEARNING_UPLOAD {
                learning_children.push(node);
            }
        }

        local_children.sort_by(|left, right| left.name.cmp(&right.name));
        learning_children.sort_by(|left, right| left.name.cmp(&right.name));

        let local_root = folder_node(
            "system:local".to_string(),
            CATEGORY_LOCAL.to_string(),
            SOURCE_TYPE_LOCAL,
            true,
            None,
            local_children,
        );

        let learning_folder_id = folders
            .iter()
            .find(|folder| folder.parent_id.is_none() && folder.name == CATEGORY_LEARNING_UPLOAD)
            .map(|folder| folder.id);
        let notes_by_folder = group_notes_by_folder(visible_notes);
        let mut roots = vec![local_root];
        let mut root_folders = folders
            .iter()
            .filter(|folder| folder.parent_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        root_folders.sort_by_key(|folder| {
            (
                if Some(folder.id) == learning_folder_id {
                    i32::MIN
                } else {
                    folder.sort_order
                },
                folder.id,
            )
        });

        for folder in root_folders {
            let extra_children = if Some(folder.id) == learning_folder_id {
                learning_children.clone()
            } else {
                Vec::new()
            };
            roots.push(build_folder_tree(
                db,
                data_dir,
                &folder,
                &folders,
                &notes_by_folder,
                &folder_names,
                force_refresh,
                Some(extra_children),
                &mut warnings,
            ));
        }

        if learning_folder_id.is_none() {
            roots.push(folder_node(
                "system:learning-upload".to_string(),
                CATEGORY_LEARNING_UPLOAD.to_string(),
                SOURCE_TYPE_LEARNING,
                true,
                None,
                learning_children,
            ));
        }

        let uncategorized_notes = notes_by_folder.get(&None).cloned().unwrap_or_default();
        let mut uncategorized_children = uncategorized_notes
            .iter()
            .map(|note| {
                let resolved = resolve_note(note, data_dir, UNCATEGORIZED_NAME.to_string());
                file_node(
                    db,
                    resolved,
                    Some("system:uncategorized".to_string()),
                    force_refresh,
                    &mut warnings,
                )
            })
            .collect::<Vec<_>>();
        uncategorized_children.sort_by(|left, right| left.name.cmp(&right.name));
        roots.push(folder_node(
            "system:uncategorized".to_string(),
            UNCATEGORIZED_NAME.to_string(),
            SOURCE_TYPE_USER,
            true,
            None,
            uncategorized_children,
        ));

        Ok(DocumentTreeResult { roots, warnings })
    }

    pub fn parsed_document(
        db: &Database,
        data_dir: &Path,
        document_id: i64,
        force_refresh: bool,
    ) -> Result<ParsedLearningDocument, AppError> {
        let resolved = resolve_document(db, data_dir, document_id)?;
        let cache = parse_and_cache(db, &resolved, force_refresh)?;
        Ok(ParsedLearningDocument {
            document_id,
            source_file: resolved.name,
            source_folder: resolved.folder_name,
            source_type: resolved.source_type,
            file_type: resolved.file_type,
            parsed_text: cache.text,
            parse_status: cache.status,
            parse_message: cache.message,
        })
    }

    pub fn selectable_sources(
        db: &Database,
        data_dir: &Path,
        document_ids: &[i64],
    ) -> Result<Vec<DocumentSource>, AppError> {
        let mut result = Vec::with_capacity(document_ids.len());
        for document_id in document_ids {
            let resolved = resolve_document(db, data_dir, *document_id)?;
            let cache = parse_and_cache(db, &resolved, false)?;
            result.push(DocumentSource {
                id: *document_id,
                display_name: resolved.name.clone(),
                original_file_name: resolved.name.clone(),
                stored_relative_path: resolved
                    .path
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_default(),
                file_extension: resolved.file_type.clone(),
                mime_type: resolved.mime_type.clone(),
                category: resolved.folder_name.clone(),
                source_module: resolved.source_type.clone(),
                is_builtin: resolved.source_type == SOURCE_TYPE_LOCAL,
                is_enabled: resolved.enabled,
                file_size: resolved.size,
                checksum: resolved.content_hash.clone(),
                created_at: String::new(),
                updated_at: String::new(),
                is_available: resolved.enabled && cache.status == "ready",
            });
        }
        Ok(result)
    }

    pub fn invalidate(db: &Database, document_id: i64) -> Result<(), AppError> {
        let conn = db.conn_lock()?;
        conn.execute(
            "DELETE FROM document_parse_cache WHERE document_id = ?1",
            [document_id],
        )?;
        Ok(())
    }
}

fn load_folders(db: &Database) -> Result<Vec<FlatFolder>, AppError> {
    let conn = db.conn_lock()?;
    let mut statement =
        conn.prepare("SELECT id, name, parent_id, sort_order FROM folders ORDER BY sort_order, id")?;
    let rows = statement.query_map([], |row| {
        Ok(FlatFolder {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            sort_order: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_notes(db: &Database) -> Result<Vec<FlatNote>, AppError> {
    let conn = db.conn_lock()?;
    let mut statement = conn.prepare(
        "SELECT id, title, content, folder_id, source_file_path, source_file_type,
                COALESCE(content_hash, '')
         FROM notes
         WHERE is_deleted = 0 AND is_hidden = 0
         ORDER BY sort_order, id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(FlatNote {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            folder_id: row.get(3)?,
            source_file_path: row.get(4)?,
            source_file_type: row.get(5)?,
            content_hash: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn group_notes_by_folder(notes: Vec<FlatNote>) -> HashMap<Option<i64>, Vec<FlatNote>> {
    let mut grouped = HashMap::<Option<i64>, Vec<FlatNote>>::new();
    for note in notes {
        grouped.entry(note.folder_id).or_default().push(note);
    }
    grouped
}

#[allow(clippy::too_many_arguments)]
fn build_folder_tree(
    db: &Database,
    data_dir: &Path,
    folder: &FlatFolder,
    folders: &[FlatFolder],
    notes_by_folder: &HashMap<Option<i64>, Vec<FlatNote>>,
    folder_names: &HashMap<i64, String>,
    force_refresh: bool,
    extra_children: Option<Vec<DocumentTreeNode>>,
    warnings: &mut Vec<String>,
) -> DocumentTreeNode {
    let key = format!("folder:{}", folder.id);
    let mut children = extra_children.unwrap_or_default();
    let mut child_folders = folders
        .iter()
        .filter(|candidate| candidate.parent_id == Some(folder.id))
        .cloned()
        .collect::<Vec<_>>();
    child_folders.sort_by_key(|candidate| (candidate.sort_order, candidate.id));
    for child in child_folders {
        children.push(build_folder_tree(
            db,
            data_dir,
            &child,
            folders,
            notes_by_folder,
            folder_names,
            force_refresh,
            None,
            warnings,
        ));
    }
    for note in notes_by_folder
        .get(&Some(folder.id))
        .cloned()
        .unwrap_or_default()
    {
        let folder_name = folder_names
            .get(&folder.id)
            .cloned()
            .unwrap_or_else(|| folder.name.clone());
        let resolved = resolve_note(&note, data_dir, folder_name);
        children.push(file_node(
            db,
            resolved,
            Some(key.clone()),
            force_refresh,
            warnings,
        ));
    }
    let is_system = folder.parent_id.is_none() && folder.name == CATEGORY_LEARNING_UPLOAD;
    folder_node(
        key,
        folder.name.clone(),
        if is_system {
            SOURCE_TYPE_LEARNING
        } else {
            SOURCE_TYPE_USER
        },
        is_system,
        Some(folder.id),
        children,
    )
}

fn folder_node(
    id: String,
    name: String,
    source_type: &str,
    system_folder: bool,
    folder_id: Option<i64>,
    children: Vec<DocumentTreeNode>,
) -> DocumentTreeNode {
    let child_count = children
        .iter()
        .map(|child| {
            if child.node_type == "file" {
                1
            } else {
                child.child_count
            }
        })
        .sum();
    DocumentTreeNode {
        id,
        parent_id: None,
        name,
        node_type: "folder".to_string(),
        source_type: source_type.to_string(),
        system_folder,
        folder_id,
        document_source_id: None,
        file_type: None,
        mime_type: None,
        size: None,
        parse_status: None,
        parse_message: None,
        can_use_as_learning_source: children
            .iter()
            .any(|child| child.can_use_as_learning_source),
        child_count,
        children,
    }
}

fn file_node(
    db: &Database,
    resolved: ResolvedDocument,
    parent_id: Option<String>,
    force_refresh: bool,
    warnings: &mut Vec<String>,
) -> DocumentTreeNode {
    let cache = match parse_and_cache(db, &resolved, force_refresh) {
        Ok(cache) => cache,
        Err(error) => {
            warnings.push(format!("解析“{}”失败：{}", resolved.name, error));
            ParseCache {
                status: "failed".to_string(),
                text: String::new(),
                message: error.to_string(),
                source_modified_at: 0,
                content_hash: resolved.content_hash.clone(),
                parser_version: PARSER_VERSION.to_string(),
            }
        }
    };
    DocumentTreeNode {
        id: if resolved.document_id > 0 {
            format!("source:{}", resolved.document_id)
        } else {
            format!("note:{}", resolved.document_id.unsigned_abs())
        },
        parent_id,
        name: resolved.name,
        node_type: "file".to_string(),
        source_type: resolved.source_type,
        system_folder: false,
        folder_id: None,
        document_source_id: Some(resolved.document_id),
        file_type: Some(resolved.file_type),
        mime_type: Some(resolved.mime_type),
        size: Some(resolved.size),
        parse_status: Some(cache.status.clone()),
        parse_message: (!cache.message.is_empty()).then_some(cache.message),
        can_use_as_learning_source: resolved.enabled
            && cache.status == "ready"
            && !cache.text.trim().is_empty(),
        child_count: 0,
        children: Vec::new(),
    }
}

fn resolve_document(
    db: &Database,
    data_dir: &Path,
    document_id: i64,
) -> Result<ResolvedDocument, AppError> {
    if document_id > 0 {
        let source = db
            .get_document_source(document_id)?
            .ok_or_else(|| AppError::NotFound(format!("文档 ID {} 不存在。", document_id)))?;
        return Ok(resolve_source(source.clone(), data_dir, source.category));
    }
    if document_id == 0 {
        return Err(AppError::InvalidInput("文档 ID 不能为 0。".to_string()));
    }
    let note_id = document_id
        .checked_abs()
        .ok_or_else(|| AppError::InvalidInput("文档 ID 无效。".to_string()))?;
    let note = {
        let conn = db.conn_lock()?;
        conn.query_row(
            "SELECT n.id, n.title, n.content, n.folder_id, n.source_file_path,
                    n.source_file_type, COALESCE(n.content_hash, ''),
                    COALESCE(f.name, ?2)
             FROM notes n
             LEFT JOIN folders f ON f.id = n.folder_id
             WHERE n.id = ?1 AND n.is_deleted = 0 AND n.is_hidden = 0",
            params![note_id, UNCATEGORIZED_NAME],
            |row| {
                Ok((
                    FlatNote {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        content: row.get(2)?,
                        folder_id: row.get(3)?,
                        source_file_path: row.get(4)?,
                        source_file_type: row.get(5)?,
                        content_hash: row.get(6)?,
                    },
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()?
    }
    .ok_or_else(|| AppError::NotFound(format!("文档 ID {} 不存在。", document_id)))?;
    Ok(resolve_note(&note.0, data_dir, note.1))
}

fn resolve_source(
    source: DocumentSource,
    data_dir: &Path,
    folder_name: String,
) -> ResolvedDocument {
    let path =
        DocumentSourceService::resolve_document_source_path(data_dir, &source.stored_relative_path)
            .ok()
            .filter(|path| path.is_file());
    ResolvedDocument {
        document_id: source.id,
        name: source.original_file_name,
        folder_name,
        source_type: if source.category == CATEGORY_LOCAL {
            SOURCE_TYPE_LOCAL.to_string()
        } else {
            SOURCE_TYPE_LEARNING.to_string()
        },
        file_type: source.file_extension,
        mime_type: source.mime_type,
        size: source.file_size,
        content_hash: source.checksum,
        path,
        note_content: None,
        enabled: source.is_enabled,
    }
}

fn resolve_note(note: &FlatNote, data_dir: &Path, folder_name: String) -> ResolvedDocument {
    let file_type = note
        .source_file_type
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "md".to_string())
        .to_ascii_lowercase();
    let path = note.source_file_path.as_ref().and_then(|raw| {
        let candidate = PathBuf::from(raw);
        let absolute = if candidate.is_absolute() {
            candidate
        } else {
            data_dir.join(candidate)
        };
        absolute.is_file().then_some(absolute)
    });
    let size = path
        .as_ref()
        .and_then(|path| path.metadata().ok())
        .map(|metadata| metadata.len() as i64)
        .unwrap_or_else(|| note.content.len() as i64);
    ResolvedDocument {
        document_id: -note.id,
        name: note.title.clone(),
        folder_name,
        source_type: SOURCE_TYPE_USER.to_string(),
        mime_type: mime_for(&file_type).to_string(),
        file_type,
        size,
        content_hash: if note.content_hash.is_empty() {
            crate::services::hash::sha256_hex(&note.content)
        } else {
            note.content_hash.clone()
        },
        path,
        note_content: Some(note.content.clone()),
        enabled: true,
    }
}

fn parse_and_cache(
    db: &Database,
    document: &ResolvedDocument,
    force_refresh: bool,
) -> Result<ParseCache, AppError> {
    let source_modified_at = document
        .path
        .as_ref()
        .and_then(|path| path.metadata().ok())
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    if !force_refresh {
        if let Some(cache) = load_cache(db, document.document_id)? {
            if cache.content_hash == document.content_hash
                && cache.source_modified_at == source_modified_at
                && cache.parser_version == PARSER_VERSION
            {
                return Ok(cache);
            }
        }
    }

    let parsed = parse_document_content(document);
    let (status, text, message) = match parsed {
        Ok(text) if text.trim().is_empty() && document.file_type == "pdf" => (
            "failed".to_string(),
            String::new(),
            "该 PDF 未检测到可提取文本，可能是扫描版，当前暂不支持 OCR。".to_string(),
        ),
        Ok(text) if text.trim().is_empty() => (
            "failed".to_string(),
            String::new(),
            "文档未检测到可用于学习计划的文本。".to_string(),
        ),
        Ok(text) => ("ready".to_string(), text, String::new()),
        Err(AppError::InvalidInput(message)) if message.starts_with("UNSUPPORTED:") => (
            "unsupported".to_string(),
            String::new(),
            message
                .trim_start_matches("UNSUPPORTED:")
                .trim()
                .to_string(),
        ),
        Err(error) => ("failed".to_string(), String::new(), error.to_string()),
    };
    let cache = ParseCache {
        status,
        text,
        message,
        source_modified_at,
        content_hash: document.content_hash.clone(),
        parser_version: PARSER_VERSION.to_string(),
    };
    save_cache(db, document.document_id, &cache)?;
    Ok(cache)
}

fn load_cache(db: &Database, document_id: i64) -> Result<Option<ParseCache>, AppError> {
    let conn = db.conn_lock()?;
    Ok(conn
        .query_row(
            "SELECT parse_status, parsed_text, parse_message, source_modified_at,
                    content_hash, parser_version
             FROM document_parse_cache WHERE document_id = ?1",
            [document_id],
            |row| {
                Ok(ParseCache {
                    status: row.get(0)?,
                    text: row.get(1)?,
                    message: row.get(2)?,
                    source_modified_at: row.get(3)?,
                    content_hash: row.get(4)?,
                    parser_version: row.get(5)?,
                })
            },
        )
        .optional()?)
}

fn save_cache(db: &Database, document_id: i64, cache: &ParseCache) -> Result<(), AppError> {
    let conn = db.conn_lock()?;
    conn.execute(
        "INSERT INTO document_parse_cache
            (document_id, parse_status, parsed_text, parse_message, parsed_at,
             source_modified_at, content_hash, parser_version)
         VALUES (?1, ?2, ?3, ?4, datetime('now','localtime'), ?5, ?6, ?7)
         ON CONFLICT(document_id) DO UPDATE SET
            parse_status = excluded.parse_status,
            parsed_text = excluded.parsed_text,
            parse_message = excluded.parse_message,
            parsed_at = excluded.parsed_at,
            source_modified_at = excluded.source_modified_at,
            content_hash = excluded.content_hash,
            parser_version = excluded.parser_version",
        params![
            document_id,
            cache.status,
            cache.text,
            cache.message,
            cache.source_modified_at,
            cache.content_hash,
            cache.parser_version
        ],
    )?;
    Ok(())
}

fn parse_document_content(document: &ResolvedDocument) -> Result<String, AppError> {
    let extension = document
        .file_type
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "md" | "markdown" | "txt" | "csv") {
        if let Some(content) = document
            .note_content
            .as_ref()
            .filter(|content| !content.trim().is_empty())
        {
            return Ok(content.clone());
        }
        let path = require_path(document)?;
        return crate::services::import::read_text_auto_encoding(path);
    }
    if extension == "pdf" {
        if let Some(path) = document.path.as_ref() {
            return crate::services::pdf::PdfService::extract_text_only(path);
        }
        return Ok(document.note_content.clone().unwrap_or_default());
    }
    if extension == "docx" {
        return extract_docx_text(require_path(document)?);
    }
    if extension == "pptx" {
        return extract_pptx_text(require_path(document)?);
    }
    if matches!(extension.as_str(), "xlsx" | "xls" | "xlsm" | "xlsb" | "ods") {
        #[cfg(desktop)]
        {
            let path = require_path(document)?;
            return crate::services::excel_parser::read_workbook(&path.to_string_lossy())
                .map(|summary| summary.markdown);
        }
        #[cfg(not(desktop))]
        {
            return Err(AppError::InvalidInput(
                "UNSUPPORTED: 当前移动端构建暂不支持 Excel 解析。".to_string(),
            ));
        }
    }
    Err(AppError::InvalidInput(format!(
        "UNSUPPORTED: 暂不支持解析 .{} 文件。",
        extension
    )))
}

fn require_path(document: &ResolvedDocument) -> Result<&Path, AppError> {
    document
        .path
        .as_deref()
        .ok_or_else(|| AppError::NotFound(format!("文档“{}”的源文件不存在。", document.name)))
}

fn extract_docx_text(path: &Path) -> Result<String, AppError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut output = String::new();
    for name in [
        "word/document.xml",
        "word/footnotes.xml",
        "word/endnotes.xml",
        "word/comments.xml",
    ] {
        if let Ok(mut xml_file) = archive.by_name(name) {
            let mut xml = String::new();
            xml_file.read_to_string(&mut xml)?;
            let text = extract_office_xml_text(&xml);
            if !text.trim().is_empty() {
                if !output.is_empty() {
                    output.push_str("\n\n");
                }
                output.push_str(text.trim());
            }
        }
    }
    Ok(output)
}

pub(crate) fn extract_pptx_text(path: &Path) -> Result<String, AppError> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut slide_names = archive
        .file_names()
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    slide_names.sort_by_key(|name| slide_number(name));
    let mut output = String::new();
    for (index, name) in slide_names.iter().enumerate() {
        let mut slide = archive.by_name(name)?;
        let mut xml = String::new();
        slide.read_to_string(&mut xml)?;
        let text = extract_office_xml_text(&xml);
        if !text.trim().is_empty() {
            output.push_str(&format!("\n## 第{}页\n\n{}\n", index + 1, text.trim()));
        }
    }
    Ok(output.trim().to_string())
}

fn slide_number(name: &str) -> usize {
    name.rsplit("slide")
        .next()
        .and_then(|tail| tail.strip_suffix(".xml"))
        .and_then(|value| value.parse().ok())
        .unwrap_or(usize::MAX)
}

fn extract_office_xml_text(xml: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0usize;
    let mut in_text = false;
    while let Some(relative_start) = xml[cursor..].find('<') {
        let tag_start = cursor + relative_start;
        if in_text && tag_start > cursor {
            output.push_str(&decode_xml_entities(&xml[cursor..tag_start]));
        }
        let Some(relative_end) = xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + relative_end;
        let raw_tag = xml[tag_start + 1..tag_end].trim();
        let closing = raw_tag.starts_with('/');
        let self_closing = raw_tag.ends_with('/');
        let tag_name = raw_tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_ascii_lowercase();
        let local_name = tag_name.rsplit(':').next().unwrap_or(tag_name.as_str());
        match (closing, local_name) {
            (false, "t") => in_text = !self_closing,
            (true, "t") => in_text = false,
            (false, "br") => output.push('\n'),
            (true, "p") => output.push('\n'),
            (true, "tc") => output.push('\t'),
            (true, "tr") => output.push('\n'),
            _ => {}
        }
        cursor = tag_end + 1;
    }
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_xml_entities(raw: &str) -> String {
    raw.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn mime_for(extension: &str) -> &'static str {
    match extension {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "txt" | "md" | "markdown" => "text/plain",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_xml_parser_preserves_text_and_table_cells() {
        let xml = r#"<p:sld><a:p><a:r><a:t>Title</a:t></a:r></a:p>
          <a:tr><a:tc><a:p><a:r><a:t>A</a:t></a:r></a:p></a:tc>
          <a:tc><a:p><a:r><a:t>B</a:t></a:r></a:p></a:tc></a:tr></p:sld>"#;
        let text = extract_office_xml_text(xml);
        assert!(text.contains("Title"));
        assert!(text.contains('A'));
        assert!(text.contains('B'));
    }
}
