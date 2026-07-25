use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[cfg(desktop)]
use calamine::{open_workbook_auto, Data, Reader};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::database::Database;
use crate::error::AppError;

const KNOWLEDGE_POINTS_RELATIVE_DIR: &str = "learning-assistant/data/knowledge_points";
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
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningKbResultItem {
    pub source_file: String,
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
    source_file: String,
    sheet_name: String,
    section: String,
    title: String,
    content: String,
}

pub struct LearningKbService;

impl LearningKbService {
    pub fn search(
        _db: &Database,
        input: LearningKbSearchInput,
    ) -> Result<LearningKbSearchResult, AppError> {
        search_knowledge_points(normalize_input(input))
    }
}

#[cfg(desktop)]
fn search_knowledge_points(
    input: LearningKbSearchInput,
) -> Result<LearningKbSearchResult, AppError> {
    let top_k = input.top_k.clamp(1, 50);
    let keywords = build_keywords(&input);
    let (roots, mut warnings) = find_knowledge_points_dirs();
    if roots.is_empty() {
        return Ok(LearningKbSearchResult {
            results: Vec::new(),
            message: format!(
                "未找到本地知识库目录：{}，已返回空结果。",
                KNOWLEDGE_POINTS_RELATIVE_DIR
            ),
            warnings,
        });
    }

    let mut files = Vec::new();
    for root in roots {
        for entry in WalkDir::new(&root).into_iter() {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    let is_xlsx = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx"));
                    let is_temp = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("~$"));
                    if entry.file_type().is_file() && is_xlsx && !is_temp {
                        files.push(path.to_path_buf());
                    }
                }
                Err(error) => warnings.push(format!("遍历知识库目录失败：{error}")),
            }
        }
    }

    files.sort();
    files.dedup();

    if files.is_empty() {
        return Ok(LearningKbSearchResult {
            results: Vec::new(),
            message: "本地知识库目录存在，但没有找到 .xlsx 文件。".to_string(),
            warnings,
        });
    }

    let mut rows = Vec::new();
    for file in files {
        match read_workbook_rows(&file) {
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
            let (score, matched_keywords) = if no_keywords {
                (0.0, Vec::new())
            } else {
                score_row(&row, &keywords)
            };

            if !no_keywords && score <= 0.0 {
                return None;
            }

            let reason = build_reason(&matched_keywords);
            Some((
                score,
                index,
                LearningKbResultItem {
                    source_file: row.source_file,
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
) -> Result<LearningKbSearchResult, AppError> {
    let _ = input;
    Ok(LearningKbSearchResult {
        results: Vec::new(),
        message: "当前平台暂不支持读取本地 Excel 知识库。".to_string(),
        warnings: Vec::new(),
    })
}

#[cfg(desktop)]
fn read_workbook_rows(path: &Path) -> Result<Vec<KnowledgePointRow>, AppError> {
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

    KnowledgePointRow {
        source_file: source_file.to_string(),
        sheet_name: sheet_name.to_string(),
        section: clean_or(&section, fallback_section),
        title: clean_or(&title, "未命名知识点"),
        content: content_parts.join("；"),
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

            Some((
                score.max(0.1),
                index,
                LearningKbResultItem {
                    source_file: row.source_file,
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
                    source_file: row.source_file,
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

#[cfg(desktop)]
fn find_knowledge_points_dirs() -> (Vec<PathBuf>, Vec<String>) {
    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_dir() {
        candidates.push(current.join(KNOWLEDGE_POINTS_RELATIVE_DIR));
        candidates.push(current.join("../learning-assistant/data/knowledge_points"));
        candidates.push(current.join("../../learning-assistant/data/knowledge_points"));
        push_upward_candidates(&current, &mut candidates);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_upward_candidates(parent, &mut candidates);
        }
    }

    let mut seen = HashSet::new();
    let mut dirs = Vec::new();
    let mut warnings = Vec::new();
    for candidate in candidates {
        let normalized = candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate.clone());
        let key = normalized.to_string_lossy().to_string();
        if !seen.insert(key) {
            continue;
        }
        if normalized.is_dir() {
            dirs.push(normalized);
        } else {
            warnings.push(format!("知识库候选目录不存在：{}", candidate.display()));
        }
    }

    (dirs, warnings)
}

#[cfg(desktop)]
fn push_upward_candidates(start: &Path, candidates: &mut Vec<PathBuf>) {
    for ancestor in start.ancestors().take(8) {
        candidates.push(ancestor.join(KNOWLEDGE_POINTS_RELATIVE_DIR));
    }
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

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn bundled_knowledge_points_are_discoverable_and_readable() {
        let (roots, _warnings) = find_knowledge_points_dirs();
        assert!(
            roots.iter().any(|root| root.ends_with("knowledge_points")),
            "firstwork must include learning-assistant/data/knowledge_points"
        );

        let result = search_knowledge_points(LearningKbSearchInput {
            course: "机械制造工艺学".to_string(),
            query: "机械加工精度".to_string(),
            top_k: 3,
            ..LearningKbSearchInput::default()
        })
        .expect("bundled knowledge workbooks should be readable");

        assert!(!result.results.is_empty());
    }
}
