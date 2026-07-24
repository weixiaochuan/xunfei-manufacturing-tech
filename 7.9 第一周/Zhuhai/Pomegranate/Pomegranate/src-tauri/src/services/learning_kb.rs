use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[cfg(desktop)]
use calamine::{open_workbook_auto, Data, Reader};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::services::local_learning_plan::resolve_knowledge_point_weight;

const RESOURCE_KNOWLEDGE_POINTS_RELATIVE: &str = "resources/learning-assistant/knowledge-points";
const RESOURCE_KNOWLEDGE_POINTS_BUNDLED_RELATIVE: &str = "learning-assistant/knowledge-points";
const EMPTY_MESSAGE: &str = "当前本地知识库暂无与本阶段匹配的内容。";
const EXPECTED_WORKBOOK_COUNT: usize = 7;
const EXPECTED_KNOWLEDGE_POINT_COUNT: usize = 283;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningKbSearchInput {
    #[serde(default)]
    pub course: String,
    #[serde(default)]
    pub query: String,
    #[serde(default, alias = "stage_name")]
    pub stage_name: String,
    #[serde(default, alias = "stage_index")]
    pub stage_index: Option<usize>,
    #[serde(default, alias = "stage_goal")]
    pub stage_goal: String,
    #[serde(default, alias = "learning_tasks")]
    pub learning_tasks: Vec<String>,
    #[serde(default, alias = "resource_tasks")]
    pub resource_tasks: Vec<String>,
    #[serde(default, alias = "practice_tasks")]
    pub practice_tasks: Vec<String>,
    #[serde(default, alias = "check_tasks")]
    pub check_tasks: Vec<String>,
    #[serde(default, alias = "knowledge_points")]
    pub knowledge_points: Vec<String>,
    #[serde(default = "default_top_k", alias = "top_k")]
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
    pub importance: Option<String>,
    pub importance_weight: Option<f64>,
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

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningKbWorkbookStats {
    pub file_name: String,
    pub row_count: usize,
    pub has_importance: bool,
    pub importance_non_empty_count: usize,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LearningKbInventory {
    pub ok: bool,
    pub directory: String,
    pub workbook_count: usize,
    pub knowledge_point_count: usize,
    pub workbooks: Vec<LearningKbWorkbookStats>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct KnowledgePointRow {
    source_file: String,
    sheet_name: String,
    section: String,
    title: String,
    content: String,
    importance_label: Option<String>,
    importance_weight: Option<f64>,
}

pub struct LearningKbService;

impl LearningKbService {
    pub fn inventory(app: &AppHandle) -> Result<LearningKbInventory, String> {
        let dir = resolve_knowledge_points_dir(app)?;
        inventory_from_dir(&dir)
    }

    pub fn search(
        app: &AppHandle,
        input: LearningKbSearchInput,
    ) -> Result<LearningKbSearchResult, String> {
        let dir = resolve_knowledge_points_dir(app)?;
        search_in_dir(&dir, input)
    }
}

pub fn manifest_knowledge_points_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(RESOURCE_KNOWLEDGE_POINTS_RELATIVE)
}

fn resolve_knowledge_points_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = vec![manifest_knowledge_points_dir()];
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(RESOURCE_KNOWLEDGE_POINTS_RELATIVE));
        candidates.push(resource_dir.join(RESOURCE_KNOWLEDGE_POINTS_BUNDLED_RELATIVE));
    }
    for candidate in &candidates {
        if candidate.is_dir() {
            return Ok(candidate.clone());
        }
    }
    Err(format!(
        "AI 助学知识点 Excel 资源缺失，请确认打包资源包含 learning-assistant/knowledge-points：{}",
        candidates
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    ))
}

pub fn inventory_from_dir(dir: &Path) -> Result<LearningKbInventory, String> {
    let files = list_xlsx_files(dir)?;
    let mut workbooks = Vec::new();
    let mut warnings = Vec::new();
    let mut total = 0usize;
    for file in &files {
        match workbook_stats(file) {
            Ok(stats) => {
                if !stats.has_importance {
                    warnings.push(format!("{} 缺少 importance 列。", stats.file_name));
                }
                if stats.has_importance && stats.importance_non_empty_count != stats.row_count {
                    warnings.push(format!(
                        "{} 的 importance 非空数量不等于数据行数。",
                        stats.file_name
                    ));
                }
                total += stats.row_count;
                workbooks.push(stats);
            }
            Err(error) => warnings.push(format!("读取 {} 失败：{error}", file.display())),
        }
    }
    if files.len() != EXPECTED_WORKBOOK_COUNT {
        warnings.push(format!(
            "知识点 Excel 数量为 {}，预期为 {}。",
            files.len(),
            EXPECTED_WORKBOOK_COUNT
        ));
    }
    if total != EXPECTED_KNOWLEDGE_POINT_COUNT {
        warnings.push(format!(
            "知识点总数为 {}，预期为 {}。",
            total, EXPECTED_KNOWLEDGE_POINT_COUNT
        ));
    }
    Ok(LearningKbInventory {
        ok: warnings.is_empty(),
        directory: dir.to_string_lossy().to_string(),
        workbook_count: files.len(),
        knowledge_point_count: total,
        workbooks,
        warnings,
    })
}

pub fn search_in_dir(
    dir: &Path,
    input: LearningKbSearchInput,
) -> Result<LearningKbSearchResult, String> {
    let input = normalize_input(input);
    let top_k = input.top_k.clamp(1, 50);
    let files = list_xlsx_files(dir)?;
    let mut warnings = Vec::new();
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
                "读取 Excel 失败，已跳过 {}：{error}",
                file.display()
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

    let keywords = build_keywords(&input);
    let no_keywords = keywords.is_empty();
    let mut scored = score_rows(rows.clone(), &keywords, no_keywords);
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

fn list_xlsx_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Err(format!("AI 助学知识点目录不存在：{}", dir.display()));
    }
    let mut files = std::fs::read_dir(dir)
        .map_err(|error| format!("读取 AI 助学知识点目录失败：{error}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xlsx"))
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

#[cfg(desktop)]
fn workbook_stats(path: &Path) -> Result<LearningKbWorkbookStats, String> {
    let mut workbook =
        open_workbook_auto(path).map_err(|error| format!("打开 Excel 失败：{error}"))?;
    let file_name = file_name(path);
    let mut total = 0usize;
    let mut has_importance = false;
    let mut importance_non_empty_count = 0usize;
    for sheet_name in workbook.sheet_names() {
        let range = workbook
            .worksheet_range(&sheet_name)
            .map_err(|error| format!("读取 Sheet 失败：{sheet_name}: {error}"))?;
        let rows = range.rows().collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }
        let headers = rows[0].iter().map(cell_to_string).collect::<Vec<_>>();
        let header_map = build_header_map(&headers);
        let importance_index = find_header_index(
            &header_map,
            &["importance", "importancelevel", "重要性", "知识点重要性"],
        );
        if importance_index.is_some() {
            has_importance = true;
        }
        let data_rows = if header_map.is_empty() {
            rows.as_slice()
        } else {
            &rows[1..]
        };
        for row in data_rows {
            let cells = row.iter().map(cell_to_string).collect::<Vec<_>>();
            if cells.iter().all(|cell| cell.trim().is_empty()) {
                continue;
            }
            total += 1;
            if let Some(index) = importance_index {
                if cells
                    .get(index)
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
                {
                    importance_non_empty_count += 1;
                }
            }
        }
    }
    Ok(LearningKbWorkbookStats {
        file_name,
        row_count: total,
        has_importance,
        importance_non_empty_count,
    })
}

#[cfg(not(desktop))]
fn workbook_stats(path: &Path) -> Result<LearningKbWorkbookStats, String> {
    let _ = path;
    Err("当前平台暂不支持读取本地 Excel 知识库。".to_string())
}

#[cfg(desktop)]
fn read_workbook_rows(path: &Path) -> Result<Vec<KnowledgePointRow>, String> {
    let mut workbook =
        open_workbook_auto(path).map_err(|error| format!("打开 Excel 失败：{error}"))?;
    let source_file = file_name(path);
    let fallback_section = section_from_file_name(&source_file);
    let mut rows = Vec::new();

    for sheet_name in workbook.sheet_names() {
        let range = workbook
            .worksheet_range(&sheet_name)
            .map_err(|error| format!("读取 Sheet 失败：{sheet_name}: {error}"))?;
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

#[cfg(not(desktop))]
fn read_workbook_rows(path: &Path) -> Result<Vec<KnowledgePointRow>, String> {
    let _ = path;
    Err("当前平台暂不支持读取本地 Excel 知识库。".to_string())
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

    let importance_label = first_field(
        header_map,
        cells,
        &["重要性", "知识点重要性", "importance", "importance_level"],
    );
    let raw_weight = first_field(header_map, cells, &["importance_weight", "权重"]);
    let numeric_weight = raw_weight
        .as_deref()
        .and_then(|value| value.parse::<f64>().ok());
    let (importance_weight, _warning) =
        resolve_knowledge_point_weight(importance_label.as_deref(), numeric_weight);

    KnowledgePointRow {
        source_file: source_file.to_string(),
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
                format!("{value:.0}")
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

fn score_rows(
    rows: Vec<KnowledgePointRow>,
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
                score_row(&row, keywords)
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
                    importance: row.importance_label,
                    importance_weight: row.importance_weight,
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
                    importance: row.importance_label,
                    importance_weight: row.importance_weight,
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

    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.cmp(&right.1))
    });

    scored
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

fn find_header_index(header_map: &HashMap<String, usize>, aliases: &[&str]) -> Option<usize> {
    aliases
        .iter()
        .find_map(|alias| header_map.get(&normalize_key(alias)).copied())
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

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown.xlsx")
        .to_string()
}

fn default_top_k() -> usize {
    5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_workbooks_are_complete() {
        let inventory = inventory_from_dir(&manifest_knowledge_points_dir()).unwrap();
        assert_eq!(inventory.workbook_count, EXPECTED_WORKBOOK_COUNT);
        assert_eq!(
            inventory.knowledge_point_count,
            EXPECTED_KNOWLEDGE_POINT_COUNT
        );
        assert!(inventory
            .workbooks
            .iter()
            .all(|item| item.has_importance && item.importance_non_empty_count == item.row_count));
        assert!(inventory.warnings.is_empty(), "{:?}", inventory.warnings);
    }

    #[test]
    fn search_reads_bundled_excel() {
        let result = search_in_dir(
            &manifest_knowledge_points_dir(),
            LearningKbSearchInput {
                query: "定位".to_string(),
                top_k: 10,
                ..LearningKbSearchInput::default()
            },
        )
        .unwrap();
        assert!(!result.results.is_empty());
        assert!(result
            .results
            .iter()
            .any(|item| item.title.contains("定位") || item.content.contains("定位")));
    }

    #[test]
    fn stage_fallback_returns_chapter_related_rows() {
        let result = search_in_dir(
            &manifest_knowledge_points_dir(),
            LearningKbSearchInput {
                query: "不存在的专用关键词".to_string(),
                stage_index: Some(2),
                top_k: 5,
                ..LearningKbSearchInput::default()
            },
        )
        .unwrap();
        assert!(!result.results.is_empty());
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("阶段")));
    }
}
