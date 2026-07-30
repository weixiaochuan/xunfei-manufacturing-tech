use std::path::{Component, Path, PathBuf};
use std::{fs, io::Read};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::database::Database;
use crate::error::AppError;
use crate::services::safe_filename;

pub const CATEGORY_LOCAL: &str = "本地数据";
pub const CATEGORY_LEARNING_UPLOAD: &str = "助学模块上传";
pub const MODULE_LEARNING_ASSISTANT: &str = "learning-assistant";
const DOCUMENT_DATA_DIR_PROD: &str = "document-data";
const DOCUMENT_DATA_DIR_DEV: &str = "dev-document-data";
const LEARNING_UPLOAD_EXTENSIONS: &[&str] =
    &["pdf", "docx", "pptx", "txt", "md", "xlsx", "xls", "csv"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSource {
    pub id: i64,
    pub display_name: String,
    pub original_file_name: String,
    #[serde(skip_serializing)]
    pub stored_relative_path: String,
    pub file_extension: String,
    pub mime_type: String,
    pub category: String,
    pub source_module: String,
    pub is_builtin: bool,
    pub is_enabled: bool,
    pub file_size: i64,
    #[serde(skip_serializing)]
    pub checksum: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_available: bool,
}

#[derive(Debug, Clone)]
pub struct NewDocumentSource {
    pub display_name: String,
    pub original_file_name: String,
    pub stored_relative_path: String,
    pub file_extension: String,
    pub mime_type: String,
    pub category: String,
    pub source_module: String,
    pub is_builtin: bool,
    pub file_size: i64,
    pub checksum: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourceListInput {
    pub category: Option<String>,
    pub source_module: Option<String>,
    pub file_extension: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSourceListResult {
    pub sources: Vec<DocumentSource>,
    pub warnings: Vec<String>,
}

pub struct DocumentSourceService;

impl DocumentSourceService {
    pub fn document_data_root(data_dir: &Path) -> PathBuf {
        data_dir.join(if cfg!(debug_assertions) {
            DOCUMENT_DATA_DIR_DEV
        } else {
            DOCUMENT_DATA_DIR_PROD
        })
    }

    pub fn resolve_document_source_path(
        data_dir: &Path,
        relative: &str,
    ) -> Result<PathBuf, AppError> {
        let relative = Path::new(relative);
        if relative.is_absolute()
            || relative.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(AppError::InvalidInput("文档数据源路径非法。".to_string()));
        }
        let root = Self::document_data_root(data_dir);
        let expected_prefix = root
            .file_name()
            .ok_or_else(|| AppError::Custom("文档数据目录无效。".to_string()))?;
        if relative.components().next().map(|part| part.as_os_str()) != Some(expected_prefix) {
            return Err(AppError::InvalidInput(
                "文档数据源不属于统一文档目录。".to_string(),
            ));
        }
        Ok(data_dir.join(relative))
    }

    pub fn list(
        db: &Database,
        data_dir: &Path,
        input: DocumentSourceListInput,
    ) -> Result<DocumentSourceListResult, AppError> {
        let mut sources = db.list_document_sources(&input)?;
        let mut warnings = Vec::new();
        for source in &mut sources {
            match Self::resolve_document_source_path(data_dir, &source.stored_relative_path) {
                Ok(path) => {
                    source.is_available = source.is_enabled && path.is_file();
                    if source.is_enabled && !path.is_file() {
                        warnings.push(format!("文档“{}”的物理文件已丢失。", source.display_name));
                    }
                }
                Err(error) => {
                    source.is_available = false;
                    warnings.push(format!("文档“{}”路径无效：{error}", source.display_name));
                }
            }
        }
        Ok(DocumentSourceListResult { sources, warnings })
    }

    pub fn import_learning_file(
        db: &Database,
        data_dir: &Path,
        source_path: &Path,
    ) -> Result<DocumentSource, AppError> {
        if !source_path.is_file() {
            return Err(AppError::NotFound("选择的文件不存在。".to_string()));
        }
        let original = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| AppError::InvalidInput("文件名无效。".to_string()))?
            .to_string();
        let extension = source_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !LEARNING_UPLOAD_EXTENSIONS.contains(&extension.as_str()) {
            return Err(AppError::InvalidInput(
                "暂不支持登记该文件类型。".to_string(),
            ));
        }
        let checksum = checksum_file(source_path)?;
        let target_dir = Self::document_data_root(data_dir).join("uploads/learning-assistant");
        fs::create_dir_all(&target_dir)?;
        let safe = safe_file_name(&original);
        let target = unique_target(&target_dir, &safe, &checksum)?;
        let copied = !target.exists();
        if copied {
            fs::copy(source_path, &target)?;
        }
        let relative = target
            .strip_prefix(data_dir)
            .map_err(|_| AppError::Custom("无法生成文档相对路径。".to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let display = Path::new(&original)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&original)
            .to_string();
        let input = NewDocumentSource {
            display_name: display,
            original_file_name: original,
            stored_relative_path: relative,
            file_extension: extension.clone(),
            mime_type: mime_for(&extension).to_string(),
            category: CATEGORY_LEARNING_UPLOAD.to_string(),
            source_module: MODULE_LEARNING_ASSISTANT.to_string(),
            is_builtin: false,
            file_size: target.metadata()?.len() as i64,
            checksum,
        };
        let mut result = match db.upsert_learning_upload_document_source(&input) {
            Ok(source) => source,
            Err(error) => {
                if copied {
                    let _ = fs::remove_file(&target);
                }
                return Err(error);
            }
        };
        result.is_available = true;
        if let Err(error) = crate::services::document_tree::DocumentTreeService::parsed_document(
            db, data_dir, result.id, false,
        ) {
            log::warn!(
                "[document_sources] initial parse failed for {}: {}",
                result.original_file_name,
                error
            );
        }
        Ok(result)
    }

    pub fn repair_learning_upload_folder(db: &Database) -> Result<usize, AppError> {
        db.repair_learning_upload_document_notes()
    }

    pub fn initialize_builtin(db: &Database, data_dir: &Path, resource_dir: &Path) -> Vec<String> {
        let mut warnings = Vec::new();
        let candidates = [
            resource_dir.join("learning-assistant/knowledge_points"),
            resource_dir.join("resources/learning-assistant/knowledge_points"),
        ];
        let Some(source_dir) = candidates.into_iter().find(|path| path.is_dir()) else {
            return vec![format!(
                "未找到内置助学 Excel 资源目录：{}",
                resource_dir.display()
            )];
        };
        let target_dir = Self::document_data_root(data_dir).join("builtin/learning-assistant");
        if let Err(error) = fs::create_dir_all(&target_dir) {
            return vec![format!("创建内置文档目录失败：{error}")];
        }
        let entries = match fs::read_dir(&source_dir) {
            Ok(entries) => entries,
            Err(error) => return vec![format!("读取内置助学资源失败：{error}")],
        };
        for entry in entries.filter_map(Result::ok) {
            let source = entry.path();
            if source
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("xlsx"))
                != Some(true)
            {
                continue;
            }
            if let Err(error) = initialize_one_builtin(db, data_dir, &target_dir, &source) {
                warnings.push(format!("初始化内置文档“{}”失败：{error}", source.display()));
            }
        }
        warnings
    }

    pub fn delete(db: &Database, data_dir: &Path, id: i64) -> Result<(), AppError> {
        let source = db
            .get_document_source(id)?
            .ok_or_else(|| AppError::NotFound("文档数据源不存在。".to_string()))?;
        if source.is_builtin {
            return Err(AppError::InvalidInput("内置本地数据不能删除。".to_string()));
        }
        let path = Self::resolve_document_source_path(data_dir, &source.stored_relative_path)?;
        if path.exists() {
            fs::remove_file(&path)?;
        }
        db.delete_document_source_record(id)?;
        crate::services::document_tree::DocumentTreeService::invalidate(db, id)
    }
}

fn initialize_one_builtin(
    db: &Database,
    data_dir: &Path,
    target_dir: &Path,
    source: &Path,
) -> Result<(), AppError> {
    let original = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::InvalidInput("内置文件名无效。".to_string()))?
        .to_string();
    let checksum = checksum_file(source)?;
    let target = unique_target(target_dir, &safe_file_name(&original), &checksum)?;
    if !target.exists() {
        fs::copy(source, &target)?;
    }
    let relative = target
        .strip_prefix(data_dir)
        .map_err(|_| AppError::Custom("无法生成内置文档相对路径。".to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    let display = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&original)
        .to_string();
    db.upsert_document_source(&NewDocumentSource {
        display_name: display,
        original_file_name: original,
        stored_relative_path: relative,
        file_extension: "xlsx".to_string(),
        mime_type: mime_for("xlsx").to_string(),
        category: CATEGORY_LOCAL.to_string(),
        source_module: MODULE_LEARNING_ASSISTANT.to_string(),
        is_builtin: true,
        file_size: target.metadata()?.len() as i64,
        checksum,
    })?;
    Ok(())
}

fn checksum_file(path: &Path) -> Result<String, AppError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn unique_target(directory: &Path, file_name: &str, checksum: &str) -> Result<PathBuf, AppError> {
    let direct = directory.join(file_name);
    if !direct.exists() || checksum_file(&direct).ok().as_deref() == Some(checksum) {
        return Ok(direct);
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    Ok(directory.join(if extension.is_empty() {
        format!("{stem}-{}", &checksum[..8])
    } else {
        format!("{stem}-{}.{}", &checksum[..8], extension)
    }))
}

fn mime_for(extension: &str) -> &'static str {
    match extension {
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

fn safe_file_name(original: &str) -> String {
    let path = Path::new(original);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let stem = safe_filename::sanitize_stem(stem);
    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("pomegranate-{label}-{nonce}"));
        fs::create_dir_all(&path).expect("test root");
        path
    }

    fn learning_upload_folder_id(db: &Database) -> i64 {
        db.find_folder_by_name(None, CATEGORY_LEARNING_UPLOAD)
            .expect("find folder")
            .expect("learning upload folder")
    }

    #[test]
    fn rejects_absolute_and_parent_paths() {
        let root = Path::new("C:/app-data");
        assert!(
            DocumentSourceService::resolve_document_source_path(root, "../secret.xlsx").is_err()
        );
        assert!(
            DocumentSourceService::resolve_document_source_path(root, "C:/secret.xlsx").is_err()
        );
    }

    #[test]
    fn builtin_initialization_is_idempotent() {
        let temp = test_root("builtin");
        let db_path = temp.join("app.db");
        let db = Database::init(db_path.to_str().expect("db path")).expect("database");
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        assert!(DocumentSourceService::initialize_builtin(&db, &temp, &resources).is_empty());
        assert!(DocumentSourceService::initialize_builtin(&db, &temp, &resources).is_empty());
        let listed = DocumentSourceService::list(&db, &temp, DocumentSourceListInput::default())
            .expect("list");
        assert_eq!(listed.sources.len(), 7);
        assert!(listed
            .sources
            .iter()
            .all(|source| source.is_available && source.is_builtin));
    }

    #[test]
    fn upload_is_registered_and_missing_file_is_reported() {
        let temp = test_root("upload");
        let db_path = temp.join("app.db");
        let db = Database::init(db_path.to_str().expect("db path")).expect("database");
        let source = temp.join("上传.xlsx");
        fs::write(&source, b"test workbook placeholder").expect("write source");
        let imported =
            DocumentSourceService::import_learning_file(&db, &temp, &source).expect("import");
        assert_eq!(imported.category, CATEGORY_LEARNING_UPLOAD);
        assert_eq!(imported.source_module, MODULE_LEARNING_ASSISTANT);
        let folder_id = learning_upload_folder_id(&db);
        let notes = db
            .list_notes(Some(folder_id), None, 1, 100, false, true, None)
            .expect("list notes")
            .0;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].folder_id, Some(folder_id));
        assert_eq!(
            notes[0].source_file_path.as_deref(),
            Some(imported.stored_relative_path.as_str())
        );
        assert_eq!(notes[0].source_file_type.as_deref(), Some("xlsx"));
        let stored = DocumentSourceService::resolve_document_source_path(
            &temp,
            &imported.stored_relative_path,
        )
        .expect("stored path");
        fs::remove_file(stored).expect("remove stored");
        let listed = DocumentSourceService::list(&db, &temp, DocumentSourceListInput::default())
            .expect("list");
        assert!(!listed.sources[0].is_available);
        assert!(!listed.warnings.is_empty());
    }
}
