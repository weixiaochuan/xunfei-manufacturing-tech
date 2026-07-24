use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, Row};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const RESOURCE_DB_RELATIVE: &str = "resources/course-graph/process_graph.db";
const RESOURCE_DB_BUNDLED_RELATIVE: &str = "course-graph/process_graph.db";
const COURSE_GRAPH_DATABASE: &str = "process_graph.db";
const COURSE_GRAPH_MODE: &str = "bundled-sqlite";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseGraphConfig {
    pub mode: String,
    pub database_name: String,
    pub database_path: Option<String>,
    pub resource_ready: bool,
    pub readonly: bool,
    pub requires_external_service: bool,
    pub service_dir_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseGraphStats {
    pub version: String,
    pub source_zip: String,
    pub generated_at: String,
    pub chapters: i64,
    pub sections: i64,
    pub knowledges: i64,
    pub concepts: i64,
    pub nodes: i64,
    pub edges: i64,
    pub source_relationships: i64,
    pub skipped_invalid_relationships: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourseGraphHealth {
    pub reachable: bool,
    pub status: String,
    pub mode: String,
    pub database_name: String,
    pub database_path: Option<String>,
    pub version: Option<String>,
    pub stats: Option<CourseGraphStats>,
    pub error: Option<String>,
}

pub struct CourseGraphService;

impl CourseGraphService {
    pub fn get_config(app: &AppHandle) -> Result<CourseGraphConfig, String> {
        let path = resolve_resource_db_path(app).ok();
        Ok(CourseGraphConfig {
            mode: COURSE_GRAPH_MODE.to_string(),
            database_name: COURSE_GRAPH_DATABASE.to_string(),
            database_path: path.as_ref().map(path_to_string),
            resource_ready: path.is_some(),
            readonly: true,
            requires_external_service: false,
            service_dir_hint: find_service_dir_hint(),
        })
    }

    pub fn health(app: &AppHandle) -> Result<CourseGraphHealth, String> {
        match Self::stats(app) {
            Ok(stats) => Ok(CourseGraphHealth {
                reachable: true,
                status: "ok".to_string(),
                mode: COURSE_GRAPH_MODE.to_string(),
                database_name: COURSE_GRAPH_DATABASE.to_string(),
                database_path: resolve_resource_db_path(app)
                    .ok()
                    .as_ref()
                    .map(path_to_string),
                version: Some(stats.version.clone()),
                stats: Some(stats),
                error: None,
            }),
            Err(error) => Ok(CourseGraphHealth {
                reachable: false,
                status: "missing_or_invalid".to_string(),
                mode: COURSE_GRAPH_MODE.to_string(),
                database_name: COURSE_GRAPH_DATABASE.to_string(),
                database_path: resolve_resource_db_path(app)
                    .ok()
                    .as_ref()
                    .map(path_to_string),
                version: None,
                stats: None,
                error: Some(error),
            }),
        }
    }

    pub fn stats(app: &AppHandle) -> Result<CourseGraphStats, String> {
        let path = resolve_resource_db_path(app)?;
        stats_from_path(&path)
    }

    pub fn chapters(app: &AppHandle) -> Result<Value, String> {
        let path = resolve_resource_db_path(app)?;
        chapters_from_path(&path)
    }

    pub fn expand(app: &AppHandle, element_id: String) -> Result<Value, String> {
        let node_id = checked_node_id(&element_id, "elementId")?;
        let path = resolve_resource_db_path(app)?;
        expand_from_path(&path, &node_id)
    }

    pub fn search(app: &AppHandle, query: String, limit: Option<u32>) -> Result<Value, String> {
        let query = query.trim().to_string();
        if query.is_empty() {
            return Err("搜索关键词不能为空".to_string());
        }
        let limit = limit.unwrap_or(20).clamp(1, 20);
        let path = resolve_resource_db_path(app)?;
        search_from_path(&path, &query, limit)
    }

    pub fn node_detail(app: &AppHandle, node_id: String) -> Result<Value, String> {
        let node_id = checked_node_id(&node_id, "nodeId")?;
        let path = resolve_resource_db_path(app)?;
        node_detail_from_path(&path, &node_id)
    }

    pub fn knowledge(app: &AppHandle, knowledge_id: String) -> Result<Value, String> {
        let knowledge_id = checked_node_id(&knowledge_id, "knowledgeId")?;
        let path = resolve_resource_db_path(app)?;
        node_detail_from_path(&path, &knowledge_id)
    }

    pub fn related(app: &AppHandle, node_id: String) -> Result<Value, String> {
        let node_id = checked_node_id(&node_id, "nodeId")?;
        let path = resolve_resource_db_path(app)?;
        related_from_path(&path, &node_id)
    }
}

fn checked_node_id(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} 不能为空"));
    }
    if value.len() > 128 || value.contains(['/', '\\', '\0']) {
        return Err(format!("{label} 不合法"));
    }
    Ok(value.to_string())
}

fn resolve_resource_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = resource_db_candidates();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(RESOURCE_DB_RELATIVE));
        candidates.push(resource_dir.join(RESOURCE_DB_BUNDLED_RELATIVE));
    }
    first_existing_db(candidates)
}

fn resource_db_candidates() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    vec![
        manifest_dir.join(RESOURCE_DB_RELATIVE),
        manifest_dir.join(RESOURCE_DB_BUNDLED_RELATIVE),
    ]
}

fn first_existing_db(candidates: Vec<PathBuf>) -> Result<PathBuf, String> {
    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }
    Err(format!(
        "课程图谱 SQLite 资源缺失，请重新生成或重新打包：{}",
        candidates
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    ))
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    if !path.exists() {
        return Err(format!("课程图谱 SQLite 数据库不存在：{}", path.display()));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("课程图谱 SQLite 数据库无法打开或已损坏：{e}"))?;
    conn.pragma_update(None, "query_only", "ON")
        .map_err(|e| format!("无法启用课程图谱只读模式：{e}"))?;
    Ok(conn)
}

fn stats_from_path(path: &Path) -> Result<CourseGraphStats, String> {
    let conn = open_readonly(path)?;
    Ok(CourseGraphStats {
        version: metadata_string(&conn, "version")?,
        source_zip: metadata_string(&conn, "sourceZip")?,
        generated_at: metadata_string(&conn, "generatedAt")?,
        chapters: metadata_i64(&conn, "chapters")?,
        sections: metadata_i64(&conn, "sections")?,
        knowledges: metadata_i64(&conn, "knowledges")?,
        concepts: metadata_i64(&conn, "concepts")?,
        nodes: metadata_i64(&conn, "nodes")?,
        edges: metadata_i64(&conn, "edges")?,
        source_relationships: metadata_i64(&conn, "sourceRelationships")?,
        skipped_invalid_relationships: metadata_i64(&conn, "skippedInvalidRelationships")?,
    })
}

fn chapters_from_path(path: &Path) -> Result<Value, String> {
    let conn = open_readonly(path)?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT n.id, n.node_type, n.name, n.content, n.chapter_id, n.section_id, n.metadata
            FROM chapters c
            JOIN nodes n ON n.id = c.id
            ORDER BY c.chapter_order, n.name
            "#,
        )
        .map_err(query_error)?;
    let nodes = stmt
        .query_map([], node_from_row)
        .map_err(query_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_error)?;
    Ok(json!({ "nodes": nodes, "relationships": [] }))
}

fn expand_from_path(path: &Path, node_id: &str) -> Result<Value, String> {
    let conn = open_readonly(path)?;
    let parent = get_node(&conn, node_id)?.ok_or_else(|| "课程图谱节点不存在".to_string())?;
    let node_type = parent
        .get("labels")
        .and_then(Value::as_array)
        .and_then(|labels| {
            labels
                .iter()
                .filter_map(Value::as_str)
                .find(|label| *label != "Entity")
        })
        .unwrap_or("Entity");
    let relations: Vec<&str> = match node_type {
        "Chapter" => vec!["HAS_SECTION"],
        "Section" => vec!["CONTAINS"],
        "Knowledge" => vec!["HAS_CONCEPT", "RELATED_TO"],
        _ => vec![],
    };
    if relations.is_empty() {
        return Ok(json!({ "results": [] }));
    }

    let mut results = Vec::new();
    for relation in relations {
        let direction = if relation == "RELATED_TO" {
            "both"
        } else {
            "outgoing"
        };
        let rows = relation_rows(&conn, node_id, relation, direction)?;
        for (edge, other) in rows {
            results.push(json!({ "n": parent, "r": edge, "m": other }));
        }
    }
    Ok(json!({ "results": results }))
}

fn search_from_path(path: &Path, query: &str, limit: u32) -> Result<Value, String> {
    let conn = open_readonly(path)?;
    let pattern = format!("%{}%", escape_like(query));
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, node_type, name, content, chapter_id, section_id, metadata
            FROM nodes
            WHERE name LIKE ?1 ESCAPE '\'
               OR content LIKE ?1 ESCAPE '\'
            ORDER BY CASE WHEN name = ?2 THEN 0 WHEN name LIKE ?3 ESCAPE '\' THEN 1 ELSE 2 END,
                     CASE node_type WHEN 'Chapter' THEN 0 WHEN 'Section' THEN 1 WHEN 'Knowledge' THEN 2 ELSE 3 END,
                     name
            LIMIT ?4
            "#,
        )
        .map_err(query_error)?;
    let prefix = format!("{}%", escape_like(query));
    let nodes = stmt
        .query_map(params![pattern, query, prefix, limit as i64], node_from_row)
        .map_err(query_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_error)?;
    Ok(json!({ "nodes": nodes, "relationships": [] }))
}

fn node_detail_from_path(path: &Path, node_id: &str) -> Result<Value, String> {
    let conn = open_readonly(path)?;
    let node = get_node(&conn, node_id)?.ok_or_else(|| "课程图谱节点不存在".to_string())?;
    let metadata = node.get("metadata").cloned().unwrap_or_else(|| json!({}));
    let chapter_id = node.get("chapterId").and_then(Value::as_str);
    let section_id = node.get("sectionId").and_then(Value::as_str);
    let chapter = chapter_id
        .and_then(|id| get_node_name(&conn, id).ok().flatten())
        .unwrap_or_default();
    let section = section_id
        .and_then(|id| get_node_name(&conn, id).ok().flatten())
        .unwrap_or_default();
    Ok(json!({
        "id": node_id,
        "name": node.get("name").and_then(Value::as_str).unwrap_or_default(),
        "nodeType": node.get("nodeType").and_then(Value::as_str).unwrap_or_default(),
        "content": node.get("content").and_then(Value::as_str).unwrap_or_default(),
        "knowledgeType": metadata.get("knowledgeType").and_then(Value::as_str).unwrap_or_default(),
        "chapter": chapter,
        "section": section,
        "metadata": metadata,
    }))
}

fn related_from_path(path: &Path, node_id: &str) -> Result<Value, String> {
    let conn = open_readonly(path)?;
    let parent = get_node(&conn, node_id)?.ok_or_else(|| "课程图谱节点不存在".to_string())?;
    let rows = relation_rows(&conn, node_id, "RELATED_TO", "both")?;
    let results: Vec<Value> = rows
        .into_iter()
        .map(|(edge, other)| json!({ "n": parent, "r": edge, "m": other }))
        .collect();
    Ok(json!({ "results": results }))
}

fn relation_rows(
    conn: &Connection,
    node_id: &str,
    relation: &str,
    direction: &str,
) -> Result<Vec<(Value, Value)>, String> {
    let sql = if direction == "both" {
        r#"
        SELECT e.id, e.source_id, e.target_id, e.relation_type, e.metadata,
               n.id, n.node_type, n.name, n.content, n.chapter_id, n.section_id, n.metadata
        FROM edges e
        JOIN nodes n ON n.id = CASE WHEN e.source_id = ?1 THEN e.target_id ELSE e.source_id END
        WHERE e.relation_type = ?2 AND (e.source_id = ?1 OR e.target_id = ?1)
        ORDER BY n.node_type, n.name
        "#
    } else {
        r#"
        SELECT e.id, e.source_id, e.target_id, e.relation_type, e.metadata,
               n.id, n.node_type, n.name, n.content, n.chapter_id, n.section_id, n.metadata
        FROM edges e
        JOIN nodes n ON n.id = e.target_id
        WHERE e.relation_type = ?2 AND e.source_id = ?1
        ORDER BY n.node_type, n.name
        "#
    };
    let mut stmt = conn.prepare(sql).map_err(query_error)?;
    let rows = stmt
        .query_map(params![node_id, relation], |row| {
            Ok((edge_from_joined_row(row)?, node_from_joined_row(row)?))
        })
        .map_err(query_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_error)?;
    Ok(rows)
}

fn get_node(conn: &Connection, node_id: &str) -> Result<Option<Value>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, node_type, name, content, chapter_id, section_id, metadata FROM nodes WHERE id = ?1",
        )
        .map_err(query_error)?;
    let mut rows = stmt.query(params![node_id]).map_err(query_error)?;
    if let Some(row) = rows.next().map_err(query_error)? {
        Ok(Some(node_from_row(row).map_err(query_error)?))
    } else {
        Ok(None)
    }
}

fn get_node_name(conn: &Connection, node_id: &str) -> Result<Option<String>, String> {
    let mut stmt = conn
        .prepare("SELECT name FROM nodes WHERE id = ?1")
        .map_err(query_error)?;
    let mut rows = stmt.query(params![node_id]).map_err(query_error)?;
    if let Some(row) = rows.next().map_err(query_error)? {
        Ok(Some(row.get::<_, String>(0).map_err(query_error)?))
    } else {
        Ok(None)
    }
}

fn node_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let id: String = row.get(0)?;
    let node_type: String = row.get(1)?;
    let name: String = row.get(2)?;
    let content: String = row.get(3)?;
    let chapter_id: Option<String> = row.get(4)?;
    let section_id: Option<String> = row.get(5)?;
    let metadata_text: String = row.get(6)?;
    Ok(node_json(
        id,
        node_type,
        name,
        content,
        chapter_id,
        section_id,
        metadata_text,
    ))
}

fn node_from_joined_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let id: String = row.get(5)?;
    let node_type: String = row.get(6)?;
    let name: String = row.get(7)?;
    let content: String = row.get(8)?;
    let chapter_id: Option<String> = row.get(9)?;
    let section_id: Option<String> = row.get(10)?;
    let metadata_text: String = row.get(11)?;
    Ok(node_json(
        id,
        node_type,
        name,
        content,
        chapter_id,
        section_id,
        metadata_text,
    ))
}

fn edge_from_joined_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let id: String = row.get(0)?;
    let source_id: String = row.get(1)?;
    let target_id: String = row.get(2)?;
    let relation_type: String = row.get(3)?;
    let metadata_text: String = row.get(4)?;
    let metadata = parse_metadata(&metadata_text);
    Ok(json!({
        "elementId": id,
        "id": id,
        "source": source_id,
        "target": target_id,
        "startNodeElementId": source_id,
        "endNodeElementId": target_id,
        "type": relation_type,
        "metadata": metadata,
    }))
}

fn node_json(
    id: String,
    node_type: String,
    name: String,
    content: String,
    chapter_id: Option<String>,
    section_id: Option<String>,
    metadata_text: String,
) -> Value {
    let metadata = parse_metadata(&metadata_text);
    json!({
        "elementId": id,
        "id": id,
        "name": name,
        "content": content,
        "labels": ["Entity", node_type],
        "nodeType": node_type,
        "chapterId": chapter_id,
        "sectionId": section_id,
        "metadata": metadata,
    })
}

fn parse_metadata(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| json!({}))
}

fn metadata_string(conn: &Connection, key: &str) -> Result<String, String> {
    conn.query_row(
        "SELECT value FROM metadata WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .map_err(query_error)
}

fn metadata_i64(conn: &Connection, key: &str) -> Result<i64, String> {
    let value = metadata_string(conn, key)?;
    value
        .parse::<i64>()
        .map_err(|e| format!("课程图谱统计字段 {key} 无法解析：{e}"))
}

fn escape_like(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn path_to_string(path: &PathBuf) -> String {
    path.to_string_lossy().to_string()
}

fn query_error(error: rusqlite::Error) -> String {
    format!("课程图谱 SQLite 查询失败：{error}")
}

fn find_service_dir_hint() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let candidates = [
        cwd.join("../mechanical-knowledge-graph-service"),
        cwd.join("../../mechanical-knowledge-graph-service"),
        cwd.join("mechanical-knowledge-graph-service"),
    ];
    candidates
        .into_iter()
        .find(|path| path.join("backend/app/main.py").exists())
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db_path() -> PathBuf {
        first_existing_db(resource_db_candidates()).expect("process_graph.db should exist")
    }

    #[test]
    fn stats_match_source_zip() {
        let stats = stats_from_path(&test_db_path()).unwrap();
        assert_eq!(stats.chapters, 7);
        assert_eq!(stats.sections, 47);
        assert_eq!(stats.knowledges, 283);
        assert_eq!(stats.concepts, 1449);
        assert_eq!(stats.nodes, 1786);
        assert_eq!(stats.edges, 2389);
        assert_eq!(stats.skipped_invalid_relationships, 1);
    }

    #[test]
    fn expands_chapter_section_and_knowledge() {
        let path = test_db_path();
        let chapters = chapters_from_path(&path).unwrap();
        assert_eq!(chapters["nodes"].as_array().unwrap().len(), 7);

        let chapter_id = chapters["nodes"][0]["id"].as_str().unwrap();
        let sections = expand_from_path(&path, chapter_id).unwrap();
        assert!(!sections["results"].as_array().unwrap().is_empty());

        let section_id = sections["results"][0]["m"]["id"].as_str().unwrap();
        let knowledges = expand_from_path(&path, section_id).unwrap();
        assert!(!knowledges["results"].as_array().unwrap().is_empty());

        let knowledge_id = knowledges["results"][0]["m"]["id"].as_str().unwrap();
        let concepts = expand_from_path(&path, knowledge_id).unwrap();
        assert!(!concepts["results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn search_detail_and_related_work() {
        let path = test_db_path();
        let search = search_from_path(&path, "定位", 20).unwrap();
        assert!(!search["nodes"].as_array().unwrap().is_empty());

        let knowledge = search["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["nodeType"] == "Knowledge")
            .expect("search should include a knowledge node");
        let id = knowledge["id"].as_str().unwrap();
        let detail = node_detail_from_path(&path, id).unwrap();
        assert!(
            detail["name"].as_str().unwrap().contains("定位")
                || detail["content"].as_str().unwrap().contains("定位")
        );

        let related = related_from_path(&path, id).unwrap();
        assert!(related["results"].is_array());
    }

    #[test]
    fn database_is_opened_readonly() {
        let conn = open_readonly(&test_db_path()).unwrap();
        let result = conn.execute("CREATE TABLE should_fail (id INTEGER)", []);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_node_ids() {
        assert!(checked_node_id("", "nodeId").is_err());
        assert!(checked_node_id("../x", "nodeId").is_err());
        assert!(checked_node_id("KN_CH7_001", "nodeId").is_ok());
    }
}
