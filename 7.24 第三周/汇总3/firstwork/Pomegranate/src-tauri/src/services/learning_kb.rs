use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::database::Database;
use crate::error::AppError;
use crate::services::document_sources::{
    DocumentSource, DocumentSourceListInput, DocumentSourceService, MODULE_LEARNING_ASSISTANT,
};
use crate::services::local_learning_plan::SelectedLearningSource;
#[cfg(desktop)]
use calamine::{open_workbook_auto, Data, Reader};
use serde::{Deserialize, Serialize};

const EMPTY_MESSAGE: &str = "当前本地知识库暂无与本阶段匹配的内容。";

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningKbSearchInput {
    #[serde(default)]
    pub course: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    #[serde(alias = "stage_name")]
    pub stage_name: String,
    #[serde(default)]
    #[serde(alias = "stage_index")]
    pub stage_index: Option<usize>,
    #[serde(default)]
    #[serde(alias = "stage_goal")]
    pub stage_goal: String,
    #[serde(default)]
    #[serde(alias = "learning_tasks")]
    pub learning_tasks: Vec<String>,
    #[serde(default)]
    #[serde(alias = "resource_tasks")]
    pub resource_tasks: Vec<String>,
    #[serde(default)]
    #[serde(alias = "practice_tasks")]
    pub practice_tasks: Vec<String>,
    #[serde(default)]
    #[serde(alias = "check_tasks")]
    pub check_tasks: Vec<String>,
    #[serde(default)]
    #[serde(alias = "knowledge_points")]
    pub knowledge_points: Vec<String>,
    #[serde(default = "default_top_k")]
    #[serde(alias = "top_k")]
    pub top_k: usize,
    #[serde(default, alias = "document_source_ids")]
    pub document_source_ids: Vec<i64>,
    #[serde(default, alias = "selected_learning_sources")]
    pub selected_learning_sources: Vec<SelectedLearningSource>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningKbResultItem {
    pub document_id: i64,
    pub source_file: String,
    pub source_folder: String,
    pub source_type: String,
    pub file_type: String,
    pub weight: f64,
    pub chunk_index: usize,
    pub sheet_name: String,
    pub section: String,
    pub title: String,
    pub content: String,
    pub matched_keywords: Vec<String>,
    pub score: f64,
    pub reason: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningKbSearchResult {
    pub results: Vec<LearningKbResultItem>,
    pub message: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct KnowledgePointRow {
    document_id: i64,
    source_file: String,
    source_folder: String,
    source_type: String,
    file_type: String,
    source_weight: f64,
    chunk_index: usize,
    sheet_name: String,
    section: String,
    title: String,
    content: String,
    // 为后续人工知识点标注预留；本版不参与文件小时分配。
    importance_label: Option<String>,
    importance_weight: Option<f64>,
}

pub struct LearningKbService;

impl LearningKbService {
    pub fn search(
        db: &Database,
        data_dir: &Path,
        input: LearningKbSearchInput,
    ) -> Result<LearningKbSearchResult, AppError> {
        let input = normalize_input(input);
        let selected_ids = selected_document_ids(&input);
        let sources = if selected_ids.is_empty() {
            DocumentSourceService::list(
                db,
                data_dir,
                DocumentSourceListInput {
                    source_module: Some(MODULE_LEARNING_ASSISTANT.to_string()),
                    file_extension: Some("xlsx".to_string()),
                    ..DocumentSourceListInput::default()
                },
            )?
            .sources
        } else {
            db.get_document_sources_by_ids(&selected_ids)?
        };
        let mut warnings = Vec::new();
        let mut files = Vec::new();
        let mut generic_ids = selected_ids
            .iter()
            .copied()
            .filter(|id| *id < 0)
            .collect::<Vec<_>>();
        for source in sources {
            if !source.is_enabled || !source.file_extension.eq_ignore_ascii_case("xlsx") {
                if source.is_enabled {
                    generic_ids.push(source.id);
                }
                continue;
            }
            match DocumentSourceService::resolve_document_source_path(
                data_dir,
                &source.stored_relative_path,
            ) {
                Ok(path) if path.is_file() => {
                    let weight = source_weight(&input, source.id);
                    files.push((source, path, weight));
                }
                Ok(_) => warnings.push(format!(
                    "文档“{}”的物理文件不存在，已跳过。",
                    source.display_name
                )),
                Err(error) => {
                    warnings.push(format!("文档“{}”路径无效：{error}", source.display_name))
                }
            }
        }
        let mut result = search_knowledge_points(input.clone(), files, warnings)?;
        let mut generic = search_generic_documents(db, data_dir, &input, &generic_ids)?;
        result.warnings.append(&mut generic.warnings);
        result.results.append(&mut generic.results);
        result.results.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.document_id.cmp(&right.document_id))
                .then_with(|| left.chunk_index.cmp(&right.chunk_index))
        });
        result.results.truncate(input.top_k.clamp(1, 50));
        result.message = if result.results.is_empty() {
            EMPTY_MESSAGE.to_string()
        } else {
            format!("已从所选文档中筛选 {} 条相关内容。", result.results.len())
        };
        Ok(result)
    }
}

fn selected_document_ids(input: &LearningKbSearchInput) -> Vec<i64> {
    if input.selected_learning_sources.is_empty() {
        input.document_source_ids.clone()
    } else {
        input
            .selected_learning_sources
            .iter()
            .map(|selection| selection.document_source_id)
            .collect()
    }
}

fn source_weight(input: &LearningKbSearchInput, document_id: i64) -> f64 {
    input
        .selected_learning_sources
        .iter()
        .find(|selection| selection.document_source_id == document_id)
        .map(|selection| selection.importance_level.weight())
        .unwrap_or(1.0)
}

fn search_generic_documents(
    db: &Database,
    data_dir: &Path,
    input: &LearningKbSearchInput,
    document_ids: &[i64],
) -> Result<LearningKbSearchResult, AppError> {
    let keywords = build_keywords(input);
    let mut warnings = Vec::new();
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for document_id in document_ids {
        if !seen.insert(*document_id) {
            continue;
        }
        let weight = source_weight(input, *document_id);
        if weight <= 0.0 {
            continue;
        }
        let parsed = match crate::services::document_tree::DocumentTreeService::parsed_document(
            db,
            data_dir,
            *document_id,
            false,
        ) {
            Ok(parsed) => parsed,
            Err(error) => {
                warnings.push(format!("文档 ID {} 读取失败：{}", document_id, error));
                continue;
            }
        };
        if parsed.parse_status != "ready" || parsed.parsed_text.trim().is_empty() {
            warnings.push(format!(
                "“{}”不可作为学习资料：{}",
                parsed.source_file,
                clean_or(&parsed.parse_message, "未解析到可用文本")
            ));
            continue;
        }

        let mut per_document = split_document_chunks(&parsed.parsed_text);
        let mut scored = per_document
            .drain(..)
            .enumerate()
            .filter_map(|(chunk_index, (section, content))| {
                let (content_score, matched_keywords) =
                    score_generic_content(&parsed.source_file, &section, &content, &keywords);
                let score = if keywords.is_empty() {
                    weight
                } else {
                    content_score * weight
                };
                if score <= 0.0 {
                    return None;
                }
                Some(LearningKbResultItem {
                    document_id: *document_id,
                    source_file: parsed.source_file.clone(),
                    source_folder: parsed.source_folder.clone(),
                    source_type: parsed.source_type.clone(),
                    file_type: parsed.file_type.clone(),
                    weight,
                    chunk_index,
                    sheet_name: section.clone(),
                    section: section.clone(),
                    title: if section.is_empty() {
                        parsed.source_file.clone()
                    } else {
                        section
                    },
                    content: truncate_content(&content, 1_200),
                    matched_keywords: matched_keywords.clone(),
                    score,
                    reason: build_reason(&matched_keywords),
                })
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(8);
        results.extend(scored);
    }
    Ok(LearningKbSearchResult {
        message: String::new(),
        results,
        warnings,
    })
}

fn split_document_chunks(text: &str) -> Vec<(String, String)> {
    const MAX_CHARS: usize = 1_500;
    const MAX_CHUNKS: usize = 80;
    let mut chunks = Vec::new();
    let mut section = String::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if paragraph.starts_with('#') {
            section = paragraph.trim_start_matches('#').trim().to_string();
        }
        if current.chars().count() + paragraph.chars().count() + 2 > MAX_CHARS
            && !current.is_empty()
        {
            chunks.push((section.clone(), current.trim().to_string()));
            current.clear();
            if chunks.len() >= MAX_CHUNKS {
                break;
            }
        }
        if paragraph.chars().count() > MAX_CHARS {
            let chars = paragraph.chars().collect::<Vec<_>>();
            for part in chars.chunks(MAX_CHARS) {
                if !current.is_empty() {
                    chunks.push((section.clone(), current.trim().to_string()));
                    current.clear();
                }
                chunks.push((section.clone(), part.iter().collect()));
                if chunks.len() >= MAX_CHUNKS {
                    return chunks;
                }
            }
        } else {
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(paragraph);
        }
    }
    if !current.trim().is_empty() && chunks.len() < MAX_CHUNKS {
        chunks.push((section, current.trim().to_string()));
    }
    chunks
}

fn score_generic_content(
    source_file: &str,
    section: &str,
    content: &str,
    keywords: &[String],
) -> (f64, Vec<String>) {
    let source_file = source_file.to_lowercase();
    let section = section.to_lowercase();
    let content = content.to_lowercase();
    let mut score = 0.0;
    let mut matched = Vec::new();
    for keyword in keywords {
        let keyword_lower = keyword.to_lowercase();
        let mut keyword_score = 0.0;
        if source_file.contains(&keyword_lower) {
            keyword_score += 6.0;
        }
        if section.contains(&keyword_lower) {
            keyword_score += 8.0;
        }
        if content.contains(&keyword_lower) {
            keyword_score += 3.0;
        }
        if keyword_score > 0.0 {
            score += keyword_score;
            matched.push(keyword.clone());
        }
    }
    (score, matched)
}

#[cfg(desktop)]
fn search_knowledge_points(
    input: LearningKbSearchInput,
    mut files: Vec<(DocumentSource, PathBuf, f64)>,
    mut warnings: Vec<String>,
) -> Result<LearningKbSearchResult, AppError> {
    let top_k = input.top_k.clamp(1, 50);
    let keywords = build_keywords(&input);
    if files.is_empty() {
        return Ok(LearningKbSearchResult {
            results: Vec::new(),
            message: "没有可读取的已登记助学 Excel 文档。".to_string(),
            warnings,
        });
    }
    files.sort_by(|left, right| left.1.cmp(&right.1));
    files.dedup_by(|left, right| left.0.id == right.0.id);

    if files.is_empty() {
        return Ok(LearningKbSearchResult {
            results: Vec::new(),
            message: "本地知识库目录存在，但没有找到 .xlsx 文件。".to_string(),
            warnings,
        });
    }

    let mut rows = Vec::new();
    for (source, file, weight) in files {
        match read_workbook_rows(&file, &source, weight) {
            Ok(mut file_rows) => rows.append(&mut file_rows),
            Err(error) => warnings.push(format!(
                "读取 Excel 失败，已跳过 {}：{}",
                file.display(),
                error
            )),
        }
    }

    if rows.is_empty() {
        return Ok(LearningKbSearchResult {
            results: Vec::new(),
            message: "已找到 Excel 文件，但没有读取到可检索的知识点内容。".to_string(),
            warnings,
        });
    }

    let no_keywords = keywords.is_empty();
    let mut scored = score_rows(rows.clone(), &input, &keywords, no_keywords);

    if scored.is_empty() && !rows.is_empty() {
        warnings.push(
            "当前阶段关键词未精确命中 Excel 知识点，已按阶段/章节返回本地基础资料。".to_string(),
        );
        scored = fallback_stage_rows(rows, &input);
    }

    let results = scored
        .into_iter()
        .take(top_k)
        .map(|(_, _, item)| item)
        .collect::<Vec<_>>();

    let message = if results.is_empty() {
        EMPTY_MESSAGE.to_string()
    } else if no_keywords {
        "未提供明确关键词，返回本地知识库中的基础内容。".to_string()
    } else if warnings
        .iter()
        .any(|warning| warning.contains("按阶段/章节返回本地基础资料"))
    {
        format!(
            "未找到精确关键词命中，已从本地 knowledge_points Excel 知识库返回 {} 条阶段相关基础资料。",
            results.len()
        )
    } else {
        format!(
            "已从本地 knowledge_points Excel 知识库返回 {} 条匹配内容。",
            results.len()
        )
    };

    Ok(LearningKbSearchResult {
        results,
        message,
        warnings,
    })
}

fn score_rows(
    rows: Vec<KnowledgePointRow>,
    _input: &LearningKbSearchInput,
    keywords: &[String],
    no_keywords: bool,
) -> Vec<(f64, usize, LearningKbResultItem)> {
    let mut scored = rows
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let (content_score, matched_keywords) = if no_keywords {
                (1.0, Vec::new())
            } else {
                score_row(&row, &keywords)
            };

            if !no_keywords && content_score <= 0.0 {
                return None;
            }
            let knowledge_weight = row.importance_weight.unwrap_or(1.0);
            let score = crate::services::local_learning_plan::combined_learning_weight(
                row.source_weight,
                content_score * knowledge_weight,
            );
            if score <= 0.0 {
                return None;
            }

            let reason = build_reason(&matched_keywords);
            Some((
                score,
                index,
                LearningKbResultItem {
                    document_id: row.document_id,
                    source_file: row.source_file,
                    source_folder: row.source_folder,
                    source_type: row.source_type,
                    file_type: row.file_type,
                    weight: row.source_weight,
                    chunk_index: row.chunk_index,
                    sheet_name: row.sheet_name,
                    section: row.section,
                    title: row.title,
                    content: truncate_content(&row.content, 900),
                    matched_keywords,
                    score,
                    reason,
                },
            ))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.cmp(&right.1))
    });

    scored
}

#[cfg(not(desktop))]
fn search_knowledge_points(
    input: LearningKbSearchInput,
    files: Vec<(DocumentSource, PathBuf, f64)>,
    warnings: Vec<String>,
) -> Result<LearningKbSearchResult, AppError> {
    let _ = (input, files);
    Ok(LearningKbSearchResult {
        results: Vec::new(),
        message: "当前平台暂不支持读取本地 Excel 知识库。".to_string(),
        warnings,
    })
}

#[cfg(desktop)]
fn read_workbook_rows(
    path: &Path,
    source: &DocumentSource,
    source_weight: f64,
) -> Result<Vec<KnowledgePointRow>, AppError> {
    let mut workbook =
        open_workbook_auto(path).map_err(|e| AppError::Custom(format!("打开 Excel 失败：{e}")))?;
    let source_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown.xlsx")
        .to_string();
    let fallback_section = section_from_file_name(&source_file);
    let mut rows = Vec::new();

    for sheet_name in workbook.sheet_names() {
        let range = match workbook.worksheet_range(&sheet_name) {
            Ok(range) => range,
            Err(error) => {
                log::warn!(
                    "[learning_kb] 读取 Sheet 失败，已跳过 {} / {}: {}",
                    source_file,
                    sheet_name,
                    error
                );
                continue;
            }
        };

        let sheet_rows = range.rows().collect::<Vec<_>>();
        if sheet_rows.is_empty() {
            continue;
        }

        let headers = sheet_rows[0].iter().map(cell_to_string).collect::<Vec<_>>();
        let header_map = build_header_map(&headers);
        let data_rows = if header_map.is_empty() {
            sheet_rows.as_slice()
        } else {
            &sheet_rows[1..]
        };

        for row in data_rows {
            let cells = row.iter().map(cell_to_string).collect::<Vec<_>>();
            if cells.iter().all(|cell| cell.trim().is_empty()) {
                continue;
            }
            rows.push(row_to_knowledge_point(
                source,
                source_weight,
                rows.len(),
                &source_file,
                &sheet_name,
                &fallback_section,
                &header_map,
                &cells,
            ));
        }
    }

    Ok(rows)
}

#[cfg(desktop)]
fn row_to_knowledge_point(
    source: &DocumentSource,
    source_weight: f64,
    chunk_index: usize,
    source_file: &str,
    sheet_name: &str,
    fallback_section: &str,
    header_map: &HashMap<String, usize>,
    cells: &[String],
) -> KnowledgePointRow {
    let section = first_field(
        header_map,
        cells,
        &[
            "section",
            "section_title",
            "sectiontitle",
            "chapter",
            "章节",
            "章",
            "节",
            "所属章节",
        ],
    )
    .unwrap_or_else(|| fallback_section.to_string());
    let title = first_field(
        header_map,
        cells,
        &[
            "title",
            "name",
            "knowledge_title",
            "knowledgetitle",
            "knowledge_point",
            "knowledgepoint",
            "知识点",
            "知识点名称",
            "标题",
            "名称",
        ],
    )
    .unwrap_or_else(|| {
        cells
            .iter()
            .find(|cell| !cell.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| "未命名知识点".to_string())
    });

    let mut content_parts = Vec::new();
    for aliases in [
        &["summary", "摘要", "概述", "简介"][..],
        &["content", "内容", "正文", "说明", "描述"][..],
        &["key_concepts", "keyconcepts", "关键词", "关键概念"][..],
        &["tags", "标签"][..],
        &["formulas", "公式"][..],
        &["figures", "图表"][..],
        &["difficulty", "难度"][..],
        &["knowledge_type", "knowledgetype", "知识类型", "类型"][..],
        &["prerequisites", "前置知识"][..],
        &["dependencies", "关联知识"][..],
        &["stage", "阶段"][..],
    ] {
        if let Some(value) = first_field(header_map, cells, aliases) {
            content_parts.push(value);
        }
    }
    if content_parts.is_empty() {
        content_parts = cells
            .iter()
            .map(|cell| cell.trim().to_string())
            .filter(|cell| !cell.is_empty())
            .collect();
    }
    let importance_label = first_field(
        header_map,
        cells,
        &["重要性", "知识点重要性", "importance", "importance_level"],
    );
    let raw_weight = first_field(header_map, cells, &["importance_weight", "权重"]);
    let numeric_weight = raw_weight
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok());
    let (importance_weight, warning) =
        crate::services::local_learning_plan::resolve_knowledge_point_weight(
            importance_label.as_deref(),
            numeric_weight.or_else(|| raw_weight.as_ref().map(|_| f64::NAN)),
        );
    if let Some(warning) = warning {
        log::warn!("[learning_kb] {source_file}/{sheet_name}: {warning}");
    }

    KnowledgePointRow {
        document_id: source.id,
        source_file: source_file.to_string(),
        source_folder: source.category.clone(),
        source_type: source.source_module.clone(),
        file_type: source.file_extension.clone(),
        source_weight,
        chunk_index,
        sheet_name: sheet_name.to_string(),
        section: clean_or(&section, fallback_section),
        title: clean_or(&title, "未命名知识点"),
        content: content_parts.join("；"),
        importance_label,
        importance_weight: Some(importance_weight),
    }
}

#[cfg(desktop)]
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.trim().to_string(),
        Data::Float(value) => {
            if value.fract() == 0.0 {
                format!("{:.0}", value)
            } else {
                value.to_string()
            }
        }
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::DateTime(value) => value.to_string(),
        Data::DateTimeIso(value) | Data::DurationIso(value) => value.clone(),
        Data::Error(value) => format!("#ERR:{value:?}"),
    }
}

#[cfg(desktop)]
fn build_header_map(headers: &[String]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (index, header) in headers.iter().enumerate() {
        let key = normalize_key(header);
        if !key.is_empty() {
            map.insert(key, index);
        }
    }
    map
}

#[cfg(desktop)]
fn first_field(
    header_map: &HashMap<String, usize>,
    cells: &[String],
    aliases: &[&str],
) -> Option<String> {
    aliases.iter().find_map(|alias| {
        let key = normalize_key(alias);
        header_map
            .get(&key)
            .and_then(|index| cells.get(*index))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn normalize_input(mut input: LearningKbSearchInput) -> LearningKbSearchInput {
    input.course = input.course.trim().to_string();
    input.query = input.query.trim().to_string();
    input.stage_name = input.stage_name.trim().to_string();
    input.stage_goal = input.stage_goal.trim().to_string();
    input.top_k = input.top_k.clamp(1, 50);
    input.learning_tasks = clean_vec(input.learning_tasks);
    input.resource_tasks = clean_vec(input.resource_tasks);
    input.practice_tasks = clean_vec(input.practice_tasks);
    input.check_tasks = clean_vec(input.check_tasks);
    input.knowledge_points = clean_vec(input.knowledge_points);
    input
}

fn clean_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn build_keywords(input: &LearningKbSearchInput) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut keywords = Vec::new();
    let mut sources = vec![
        input.course.clone(),
        input.query.clone(),
        input.stage_name.clone(),
        input.stage_goal.clone(),
    ];
    sources.extend(input.learning_tasks.iter().cloned());
    sources.extend(input.resource_tasks.iter().cloned());
    sources.extend(input.practice_tasks.iter().cloned());
    sources.extend(input.check_tasks.iter().cloned());
    sources.extend(input.knowledge_points.iter().cloned());

    for source in sources {
        push_keyword(&source, &mut seen, &mut keywords);
        for part in source.split([
            ' ', '\n', '\t', '，', ',', '。', '；', ';', '、', '：', ':', '（', '）', '(', ')',
            '[', ']', '【', '】', '/', '\\', '-', '—',
        ]) {
            push_keyword(part, &mut seen, &mut keywords);
        }
    }

    keywords
}

fn push_keyword(value: &str, seen: &mut HashSet<String>, keywords: &mut Vec<String>) {
    let keyword = value
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '「' | '」'))
        .to_string();
    if keyword.chars().count() < 2 || is_generic_keyword(&keyword) {
        return;
    }
    let key = keyword.to_lowercase();
    if seen.insert(key) {
        keywords.push(keyword);
    }
}

fn is_generic_keyword(keyword: &str) -> bool {
    matches!(
        keyword,
        "阶段" | "学习" | "任务" | "资源" | "练习" | "检查" | "检验" | "完成" | "当前" | "目标"
    )
}

fn score_row(row: &KnowledgePointRow, keywords: &[String]) -> (f64, Vec<String>) {
    let file = row.source_file.to_lowercase();
    let section = row.section.to_lowercase();
    let title = row.title.to_lowercase();
    let content = row.content.to_lowercase();
    let mut score = 0.0;
    let mut matched = Vec::new();

    for keyword in keywords {
        let key = keyword.to_lowercase();
        let mut keyword_score = 0.0;
        if file.contains(&key) {
            keyword_score += 8.0;
        }
        if section.contains(&key) {
            keyword_score += 8.0;
        }
        if title.contains(&key) {
            keyword_score += 10.0;
        }
        if content.contains(&key) {
            keyword_score += 3.0;
        }
        if keyword_score > 0.0 {
            score += keyword_score;
            matched.push(keyword.clone());
        }
    }

    ((score * 100.0_f64).round() / 100.0, matched)
}

fn build_reason(matched_keywords: &[String]) -> String {
    if matched_keywords.is_empty() {
        return "未提供明确关键词，返回本地知识库中的基础内容。".to_string();
    }

    let preview = matched_keywords
        .iter()
        .take(6)
        .cloned()
        .collect::<Vec<_>>()
        .join("、");
    format!("该内容命中了当前阶段关键词：{preview}，适合作为本阶段学习资料。")
}

fn fallback_stage_rows(
    rows: Vec<KnowledgePointRow>,
    input: &LearningKbSearchInput,
) -> Vec<(f64, usize, LearningKbResultItem)> {
    let stage_index = input.stage_index.unwrap_or(0);
    let chapter_keywords = fallback_chapter_keywords(stage_index);
    let has_chapter_keywords = !chapter_keywords.is_empty();

    let mut scored = rows
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let row_text = format!(
                "{} {} {} {}",
                row.source_file, row.section, row.title, row.content
            )
            .to_lowercase();
            let mut score = if has_chapter_keywords {
                chapter_keywords
                    .iter()
                    .filter(|keyword| row_text.contains(&keyword.to_lowercase()))
                    .count() as f64
                    * 20.0
            } else {
                1.0
            };

            if has_chapter_keywords && score <= 0.0 {
                return None;
            }

            if row.title.starts_with("KN_") {
                score -= 1.0;
            }
            score *= row.source_weight * row.importance_weight.unwrap_or(1.0);
            if score <= 0.0 {
                return None;
            }

            Some((
                score.max(0.1),
                index,
                LearningKbResultItem {
                    document_id: row.document_id,
                    source_file: row.source_file,
                    source_folder: row.source_folder,
                    source_type: row.source_type,
                    file_type: row.file_type,
                    weight: row.source_weight,
                    chunk_index: row.chunk_index,
                    sheet_name: row.sheet_name,
                    section: row.section,
                    title: row.title,
                    content: truncate_content(&row.content, 900),
                    matched_keywords: chapter_keywords.clone(),
                    score: score.max(0.1),
                    reason: if has_chapter_keywords {
                        format!(
                            "当前阶段关键词未精确命中，系统按阶段序号关联章节：{}，返回该章节基础知识点。",
                            chapter_keywords.join("、")
                        )
                    } else {
                        "当前阶段关键词未精确命中，返回本地知识库中的基础知识点。".to_string()
                    },
                },
            ))
        })
        .collect::<Vec<_>>();

    if scored.is_empty() {
        scored = rows_to_basic_items(Vec::new());
    }

    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.cmp(&right.1))
    });

    scored
}

fn rows_to_basic_items(rows: Vec<KnowledgePointRow>) -> Vec<(f64, usize, LearningKbResultItem)> {
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            (
                0.1,
                index,
                LearningKbResultItem {
                    document_id: row.document_id,
                    source_file: row.source_file,
                    source_folder: row.source_folder,
                    source_type: row.source_type,
                    file_type: row.file_type,
                    weight: row.source_weight,
                    chunk_index: row.chunk_index,
                    sheet_name: row.sheet_name,
                    section: row.section,
                    title: row.title,
                    content: truncate_content(&row.content, 900),
                    matched_keywords: Vec::new(),
                    score: 0.1,
                    reason: "当前阶段关键词未精确命中，返回本地知识库中的基础知识点。".to_string(),
                },
            )
        })
        .collect()
}

fn fallback_chapter_keywords(stage_index: usize) -> Vec<String> {
    let keywords = match stage_index {
        1 => &["第一章", "第二章", "绪论", "工艺规程"][..],
        2 => &["第二章", "第三章", "工艺规程", "夹具"][..],
        3 => &["第四章", "第五章", "加工精度", "表面质量"][..],
        4 => &["第六章", "第七章", "装配", "发展"][..],
        5 => &["第七章", "发展", "综合"][..],
        _ => &[][..],
    };
    keywords.iter().map(|keyword| keyword.to_string()).collect()
}

fn section_from_file_name(source_file: &str) -> String {
    let stem = source_file
        .trim_end_matches(".xlsx")
        .trim_end_matches(".XLSX")
        .trim();
    stem.strip_prefix("knowledge_base_")
        .unwrap_or(stem)
        .to_string()
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_' && *ch != '-' && *ch != '/')
        .collect::<String>()
        .to_lowercase()
}

fn truncate_content(content: &str, max_chars: usize) -> String {
    let content = content.trim();
    if content.chars().count() <= max_chars {
        return content.to_string();
    }

    let mut truncated = content.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn clean_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn default_top_k() -> usize {
    5
}
