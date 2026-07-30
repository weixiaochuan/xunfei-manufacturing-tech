use rusqlite::{params, OptionalExtension, Row};

use crate::error::AppError;
use crate::models::{CourseGraphAiAnalysis, CourseGraphAiRelation};

use super::Database;

impl Database {
    pub fn save_course_graph_ai_analysis(
        &self,
        analysis: &CourseGraphAiAnalysis,
        raw_response: &str,
    ) -> Result<CourseGraphAiAnalysis, AppError> {
        let mut conn = self.conn_lock()?;
        let tx = conn.transaction()?;
        tx.execute(
            r#"
            INSERT INTO course_graph_ai_analyses (
                node_id, node_name, source_kind, source_revision,
                definition, summary, aliases_json, prerequisites_json,
                applications_json, misconceptions_json, model_id, raw_response,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                      datetime('now','localtime'), datetime('now','localtime'))
            ON CONFLICT(node_id, source_kind) DO UPDATE SET
                node_name = excluded.node_name,
                source_revision = excluded.source_revision,
                definition = excluded.definition,
                summary = excluded.summary,
                aliases_json = excluded.aliases_json,
                prerequisites_json = excluded.prerequisites_json,
                applications_json = excluded.applications_json,
                misconceptions_json = excluded.misconceptions_json,
                model_id = excluded.model_id,
                raw_response = excluded.raw_response,
                updated_at = datetime('now','localtime')
            "#,
            params![
                analysis.node_id,
                analysis.node_name,
                analysis.source_kind,
                analysis.source_revision,
                analysis.definition,
                analysis.summary,
                serde_json::to_string(&analysis.aliases)?,
                serde_json::to_string(&analysis.prerequisites)?,
                serde_json::to_string(&analysis.applications)?,
                serde_json::to_string(&analysis.misconceptions)?,
                analysis.model_id,
                raw_response,
            ],
        )?;

        // 重新分析只替换尚未审核的候选；用户已经接受或拒绝的历史判断保持不变。
        tx.execute(
            "DELETE FROM course_graph_ai_relations WHERE source_node_id = ?1 AND source_kind = ?2 AND status = 'pending'",
            params![analysis.node_id, analysis.source_kind],
        )?;
        for relation in &analysis.relations {
            tx.execute(
                r#"
                INSERT INTO course_graph_ai_relations (
                    source_node_id, source_node_name, target_node_id, target_node_name,
                    relation_type, reason, confidence, status, source_kind,
                    source_revision, model_id, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9, ?10,
                          datetime('now','localtime'), datetime('now','localtime'))
                ON CONFLICT(source_node_id, target_node_id, relation_type, source_kind)
                DO UPDATE SET
                    source_node_name = excluded.source_node_name,
                    target_node_name = excluded.target_node_name,
                    reason = excluded.reason,
                    confidence = excluded.confidence,
                    source_revision = excluded.source_revision,
                    model_id = excluded.model_id,
                    updated_at = datetime('now','localtime')
                "#,
                params![
                    relation.source_node_id,
                    relation.source_node_name,
                    relation.target_node_id,
                    relation.target_node_name,
                    relation.relation_type,
                    relation.reason,
                    relation.confidence,
                    relation.source_kind,
                    relation.source_revision,
                    relation.model_id,
                ],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.get_course_graph_ai_analysis(&analysis.node_id, &analysis.source_kind)?
            .ok_or_else(|| AppError::Custom("AI 知识点分析保存后无法读取".into()))
    }

    pub fn get_course_graph_ai_analysis(
        &self,
        node_id: &str,
        source_kind: &str,
    ) -> Result<Option<CourseGraphAiAnalysis>, AppError> {
        let conn = self.conn_lock()?;
        let mut analysis = conn
            .query_row(
                r#"
                SELECT node_id, node_name, source_kind, source_revision,
                       definition, summary, aliases_json, prerequisites_json,
                       applications_json, misconceptions_json, model_id,
                       created_at, updated_at
                FROM course_graph_ai_analyses
                WHERE node_id = ?1 AND source_kind = ?2
                "#,
                params![node_id, source_kind],
                analysis_from_row,
            )
            .optional()?;
        if let Some(value) = analysis.as_mut() {
            value.relations = list_relations_with_conn(&conn, node_id, None)?;
        }
        Ok(analysis)
    }

    pub fn list_course_graph_ai_relations(
        &self,
        node_id: &str,
        status: Option<&str>,
    ) -> Result<Vec<CourseGraphAiRelation>, AppError> {
        let conn = self.conn_lock()?;
        list_relations_with_conn(&conn, node_id, status)
    }

    pub fn review_course_graph_ai_relation(
        &self,
        relation_id: i64,
        status: &str,
    ) -> Result<CourseGraphAiRelation, AppError> {
        let conn = self.conn_lock()?;
        let affected = conn.execute(
            "UPDATE course_graph_ai_relations SET status = ?1, updated_at = datetime('now','localtime') WHERE id = ?2",
            params![status, relation_id],
        )?;
        if affected == 0 {
            return Err(AppError::NotFound("AI 知识关系不存在".into()));
        }
        conn.query_row(
            "SELECT id, source_node_id, source_node_name, target_node_id, target_node_name, relation_type, reason, confidence, status, source_kind, source_revision, model_id, created_at, updated_at FROM course_graph_ai_relations WHERE id = ?1",
            params![relation_id],
            relation_from_row,
        )
        .map_err(AppError::from)
    }
}

fn analysis_from_row(row: &Row<'_>) -> rusqlite::Result<CourseGraphAiAnalysis> {
    Ok(CourseGraphAiAnalysis {
        node_id: row.get(0)?,
        node_name: row.get(1)?,
        source_kind: row.get(2)?,
        source_revision: row.get(3)?,
        definition: row.get(4)?,
        summary: row.get(5)?,
        aliases: parse_string_list(row.get::<_, String>(6)?),
        prerequisites: parse_string_list(row.get::<_, String>(7)?),
        applications: parse_string_list(row.get::<_, String>(8)?),
        misconceptions: parse_string_list(row.get::<_, String>(9)?),
        model_id: row.get(10)?,
        relations: Vec::new(),
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn relation_from_row(row: &Row<'_>) -> rusqlite::Result<CourseGraphAiRelation> {
    Ok(CourseGraphAiRelation {
        id: row.get(0)?,
        source_node_id: row.get(1)?,
        source_node_name: row.get(2)?,
        target_node_id: row.get(3)?,
        target_node_name: row.get(4)?,
        relation_type: row.get(5)?,
        reason: row.get(6)?,
        confidence: row.get(7)?,
        status: row.get(8)?,
        source_kind: row.get(9)?,
        source_revision: row.get(10)?,
        model_id: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn list_relations_with_conn(
    conn: &rusqlite::Connection,
    node_id: &str,
    status: Option<&str>,
) -> Result<Vec<CourseGraphAiRelation>, AppError> {
    let sql = if status.is_some() {
        "SELECT id, source_node_id, source_node_name, target_node_id, target_node_name, relation_type, reason, confidence, status, source_kind, source_revision, model_id, created_at, updated_at FROM course_graph_ai_relations WHERE source_node_id = ?1 AND status = ?2 ORDER BY confidence DESC, id"
    } else {
        "SELECT id, source_node_id, source_node_name, target_node_id, target_node_name, relation_type, reason, confidence, status, source_kind, source_revision, model_id, created_at, updated_at FROM course_graph_ai_relations WHERE source_node_id = ?1 ORDER BY CASE status WHEN 'pending' THEN 0 WHEN 'accepted' THEN 1 ELSE 2 END, confidence DESC, id"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(status) = status {
        stmt.query_map(params![node_id, status], relation_from_row)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![node_id], relation_from_row)?
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(rows)
}

fn parse_string_list(value: String) -> Vec<String> {
    serde_json::from_str(&value).unwrap_or_default()
}
