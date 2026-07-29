use rusqlite::Connection;

use crate::error::AppError;

/// 当前 Schema 版本
pub const SCHEMA_VERSION: i32 = 55;

/// 获取数据库版本
pub fn get_version(conn: &Connection) -> Result<i32, AppError> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    Ok(version)
}

/// 设置数据库版本
pub fn set_version(conn: &Connection, version: i32) -> Result<(), AppError> {
    conn.pragma_update(None, "user_version", version)?;
    Ok(())
}

/// 执行数据库迁移
pub fn migrate(conn: &Connection) -> Result<(), AppError> {
    let mut version = get_version(conn)?;

    if version > SCHEMA_VERSION {
        return Err(AppError::Custom(format!(
            "数据库版本({})高于应用支持的版本({}), 请升级应用",
            version, SCHEMA_VERSION
        )));
    }

    while version < SCHEMA_VERSION {
        match version {
            0 => migrate_v0_to_v1(conn)?,
            1 => migrate_v1_to_v2(conn)?,
            2 => migrate_v2_to_v3(conn)?,
            3 => migrate_v3_to_v4(conn)?,
            4 => migrate_v4_to_v5(conn)?,
            5 => migrate_v5_to_v6(conn)?,
            6 => migrate_v6_to_v7(conn)?,
            7 => migrate_v7_to_v8(conn)?,
            8 => migrate_v8_to_v9(conn)?,
            9 => migrate_v9_to_v10(conn)?,
            10 => migrate_v10_to_v11(conn)?,
            11 => migrate_v11_to_v12(conn)?,
            12 => migrate_v12_to_v13(conn)?,
            13 => migrate_v13_to_v14(conn)?,
            14 => migrate_v14_to_v15(conn)?,
            15 => migrate_v15_to_v16(conn)?,
            16 => migrate_v16_to_v17(conn)?,
            17 => migrate_v17_to_v18(conn)?,
            18 => migrate_v18_to_v19(conn)?,
            19 => migrate_v19_to_v20(conn)?,
            20 => migrate_v20_to_v21(conn)?,
            21 => migrate_v21_to_v22(conn)?,
            22 => migrate_v22_to_v23(conn)?,
            23 => migrate_v23_to_v24(conn)?,
            24 => migrate_v24_to_v25(conn)?,
            25 => migrate_v25_to_v26(conn)?,
            26 => migrate_v26_to_v27(conn)?,
            27 => migrate_v27_to_v28(conn)?,
            28 => migrate_v28_to_v29(conn)?,
            29 => migrate_v29_to_v30(conn)?,
            30 => migrate_v30_to_v31(conn)?,
            31 => migrate_v31_to_v32(conn)?,
            32 => migrate_v32_to_v33(conn)?,
            33 => migrate_v33_to_v34(conn)?,
            34 => migrate_v34_to_v35(conn)?,
            35 => migrate_v35_to_v36(conn)?,
            36 => migrate_v36_to_v37(conn)?,
            37 => migrate_v37_to_v38(conn)?,
            38 => migrate_v38_to_v39(conn)?,
            39 => migrate_v39_to_v40(conn)?,
            40 => migrate_v40_to_v41(conn)?,
            41 => migrate_v41_to_v42(conn)?,
            42 => migrate_v42_to_v43(conn)?,
            43 => migrate_v43_to_v44(conn)?,
            44 => migrate_v44_to_v45(conn)?,
            45 => migrate_v45_to_v46(conn)?,
            46 => migrate_v46_to_v47(conn)?,
            47 => migrate_v47_to_v48(conn)?,
            48 => migrate_v48_to_v49(conn)?,
            49 => migrate_v49_to_v50(conn)?,
            50 => migrate_v50_to_v51(conn)?,
            51 => migrate_v51_to_v52(conn)?,
            52 => migrate_v52_to_v53(conn)?,
            53 => migrate_v53_to_v54(conn)?,
            54 => migrate_v54_to_v55(conn)?,
            _ => {
                return Err(AppError::Custom(format!("未知的数据库版本: {}", version)));
            }
        }
        version = get_version(conn)?;
    }

    log::info!("数据库迁移完成, 当前版本: {}", version);
    Ok(())
}

/// v0 -> v1: 初始化表结构
fn migrate_v0_to_v1(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v0 -> v1");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS app_config (
            key         TEXT PRIMARY KEY,
            value       TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- 插入默认配置
        INSERT OR IGNORE INTO app_config (key, value) VALUES ('theme', 'light');
        INSERT OR IGNORE INTO app_config (key, value) VALUES ('language', 'zh-CN');
        INSERT OR IGNORE INTO app_config (key, value) VALUES ('sidebar_collapsed', 'false');
        ",
    )?;

    set_version(conn, 1)?;
    Ok(())
}

/// v1 -> v2: 创建 folders 表和 notes 表
fn migrate_v1_to_v2(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v1 -> v2");

    conn.execute_batch(
        "
        -- 文件夹表（树形结构）
        CREATE TABLE IF NOT EXISTS folders (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            parent_id   INTEGER REFERENCES folders(id) ON DELETE SET NULL,
            sort_order  INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- 笔记表
        CREATE TABLE IF NOT EXISTS notes (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL,
            content     TEXT NOT NULL DEFAULT '',
            folder_id   INTEGER REFERENCES folders(id) ON DELETE SET NULL,
            is_daily    INTEGER NOT NULL DEFAULT 0,
            daily_date  TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- 索引
        CREATE INDEX IF NOT EXISTS idx_notes_folder  ON notes(folder_id);
        CREATE INDEX IF NOT EXISTS idx_notes_updated ON notes(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_notes_daily   ON notes(is_daily, daily_date);
        ",
    )?;

    set_version(conn, 2)?;
    Ok(())
}

/// v2 -> v3: 添加标签、双向链接、FTS5 全文搜索、回收站等功能
fn migrate_v2_to_v3(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v2 -> v3");

    conn.execute_batch(
        "
        -- 给 notes 表添加新字段
        ALTER TABLE notes ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE notes ADD COLUMN is_deleted INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE notes ADD COLUMN deleted_at TEXT;
        ALTER TABLE notes ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0;

        -- 标签表
        CREATE TABLE IF NOT EXISTS tags (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL UNIQUE,
            color       TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- 笔记-标签关联表
        CREATE TABLE IF NOT EXISTS note_tags (
            note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (note_id, tag_id)
        );

        -- 双向链接表
        CREATE TABLE IF NOT EXISTS note_links (
            source_id   INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            target_id   INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            context     TEXT,
            PRIMARY KEY (source_id, target_id)
        );

        -- FTS5 全文搜索虚拟表
        CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
            title, content, content=notes, content_rowid=id,
            tokenize='unicode61'
        );

        -- FTS5 同步触发器
        CREATE TRIGGER IF NOT EXISTS notes_fts_insert AFTER INSERT ON notes BEGIN
            INSERT INTO notes_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS notes_fts_update AFTER UPDATE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
            INSERT INTO notes_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
        END;

        CREATE TRIGGER IF NOT EXISTS notes_fts_delete AFTER DELETE ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
        END;

        -- 索引
        CREATE INDEX IF NOT EXISTS idx_notes_deleted ON notes(is_deleted, deleted_at);
        CREATE INDEX IF NOT EXISTS idx_notes_pinned ON notes(is_pinned, updated_at DESC) WHERE is_deleted = 0;
        CREATE INDEX IF NOT EXISTS idx_note_tags_tag ON note_tags(tag_id);
        CREATE INDEX IF NOT EXISTS idx_note_links_target ON note_links(target_id);

        -- 将已有笔记数据同步到 FTS5
        INSERT INTO notes_fts(rowid, title, content) SELECT id, title, content FROM notes;
        ",
    )?;

    set_version(conn, 3)?;
    Ok(())
}

/// v3 -> v4: AI 知识问答（模型配置、对话、消息）
fn migrate_v3_to_v4(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v3 -> v4");

    conn.execute_batch(
        "
        -- AI 模型配置表
        CREATE TABLE IF NOT EXISTS ai_models (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            provider    TEXT NOT NULL,
            api_url     TEXT NOT NULL,
            api_key     TEXT,
            model_id    TEXT NOT NULL,
            is_default  INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- AI 对话表
        CREATE TABLE IF NOT EXISTS ai_conversations (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            title       TEXT NOT NULL DEFAULT '新对话',
            model_id    INTEGER NOT NULL REFERENCES ai_models(id) ON DELETE CASCADE,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- AI 消息表
        CREATE TABLE IF NOT EXISTS ai_messages (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL REFERENCES ai_conversations(id) ON DELETE CASCADE,
            role            TEXT NOT NULL,
            content         TEXT NOT NULL,
            references_json TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- 索引
        CREATE INDEX IF NOT EXISTS idx_ai_conv_updated ON ai_conversations(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ai_msg_conv ON ai_messages(conversation_id, created_at);

        -- 默认 Ollama 本地模型
        INSERT INTO ai_models (name, provider, api_url, api_key, model_id, is_default)
        VALUES ('Ollama Llama3', 'ollama', 'http://localhost:11434', NULL, 'llama3', 1);
        ",
    )?;

    set_version(conn, 4)?;
    Ok(())
}

/// v4 -> v5: 性能优化索引 + 字数统计触发器
fn migrate_v4_to_v5(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v4 -> v5");

    conn.execute_batch(
        "
        -- 笔记标题索引（加速搜索）
        CREATE INDEX IF NOT EXISTS idx_notes_title ON notes(title) WHERE is_deleted = 0;

        -- 笔记创建时间索引
        CREATE INDEX IF NOT EXISTS idx_notes_created ON notes(created_at DESC) WHERE is_deleted = 0;

        -- 字数统计触发器：插入时自动计算
        CREATE TRIGGER IF NOT EXISTS notes_word_count_insert AFTER INSERT ON notes BEGIN
            UPDATE notes SET word_count = LENGTH(REPLACE(new.content, ' ', ''))
            WHERE id = new.id;
        END;

        -- 字数统计触发器：更新时自动计算
        CREATE TRIGGER IF NOT EXISTS notes_word_count_update AFTER UPDATE OF content ON notes BEGIN
            UPDATE notes SET word_count = LENGTH(REPLACE(new.content, ' ', ''))
            WHERE id = new.id;
        END;

        -- 优化现有数据字数
        UPDATE notes SET word_count = LENGTH(REPLACE(content, ' ', ''))
        WHERE word_count = 0 AND LENGTH(content) > 0;

        -- ANALYZE 更新统计信息
        ANALYZE;
        ",
    )?;

    set_version(conn, 5)?;
    Ok(())
}

/// v5 -> v6: 修复 FTS5 触发器级联导致的索引损坏
///
/// 问题根因：notes_fts_update 监听 AFTER UPDATE ON notes（全列），
/// 当 word_count 触发器更新 word_count 列时，也会触发 FTS 更新，
/// 导致 FTS 索引被反复 DELETE+INSERT，最终损坏 → "database disk image is malformed"
///
/// 修复：将 FTS 更新触发器限定为 AFTER UPDATE OF title, content
fn migrate_v5_to_v6(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v5 -> v6 (修复 FTS 触发器级联)");

    conn.execute_batch(
        "
        -- 1. 删除有问题的 FTS 更新触发器（监听全列）
        DROP TRIGGER IF EXISTS notes_fts_update;

        -- 2. 重建：只在 title 或 content 变更时触发
        CREATE TRIGGER IF NOT EXISTS notes_fts_update AFTER UPDATE OF title, content ON notes BEGIN
            INSERT INTO notes_fts(notes_fts, rowid, title, content) VALUES('delete', old.id, old.title, old.content);
            INSERT INTO notes_fts(rowid, title, content) VALUES (new.id, new.title, new.content);
        END;

        -- 3. 重建 FTS 索引，清除可能已损坏的数据
        INSERT INTO notes_fts(notes_fts) VALUES('rebuild');
        ",
    )?;

    set_version(conn, 6)?;
    Ok(())
}

/// v6 -> v7: 笔记模板表
fn migrate_v6_to_v7(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v6 -> v7 (笔记模板)");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS note_templates (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            content     TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        -- 预置常用模板
        INSERT INTO note_templates (name, description, content) VALUES
        ('会议记录', '记录会议要点、决策和待办事项', '<h2>会议信息</h2><p><strong>日期：</strong></p><p><strong>参与人：</strong></p><p><strong>主题：</strong></p><h2>议题与讨论</h2><ol><li><p></p></li></ol><h2>决策事项</h2><ul><li><p></p></li></ul><h2>待办事项</h2><ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p></p></div></li></ul>'),
        ('读书笔记', '记录书籍要点、摘抄和感想', '<h2>书籍信息</h2><p><strong>书名：</strong></p><p><strong>作者：</strong></p><p><strong>阅读日期：</strong></p><h2>核心观点</h2><ol><li><p></p></li></ol><h2>精彩摘录</h2><blockquote><p></p></blockquote><h2>我的思考</h2><p></p>'),
        ('周报', '总结本周工作和下周计划', '<h2>本周完成</h2><ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"true\"><label><input type=\"checkbox\"><span></span></label><div><p></p></div></li></ul><h2>进行中</h2><ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p></p></div></li></ul><h2>下周计划</h2><ol><li><p></p></li></ol><h2>问题与风险</h2><p></p>'),
        ('项目文档', '记录项目背景、方案和进展', '<h2>项目概述</h2><p></p><h2>背景与目标</h2><p></p><h2>技术方案</h2><p></p><h2>里程碑</h2><ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p></p></div></li></ul><h2>参考资料</h2><ul><li><p></p></li></ul>');
        ",
    )?;

    set_version(conn, 7)?;
    Ok(())
}

/// v7 -> v8: notes 表加 pdf_path 字段，用于关联导入的 PDF 原文件
fn migrate_v7_to_v8(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v7 -> v8 (notes.pdf_path)");

    conn.execute_batch(
        "
        -- 存相对路径 pdfs/<note_id>.pdf，拼 app_data_dir 得到绝对路径
        ALTER TABLE notes ADD COLUMN pdf_path TEXT;
        ",
    )?;

    set_version(conn, 8)?;
    Ok(())
}

/// 列出表的所有列名（用 PRAGMA table_info）
fn list_columns(conn: &Connection, table: &str) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

/// v8 -> v9: 把 pdf_path 升级为通用源文件路径
///
/// - 新增 source_file_type 列，区分 pdf/docx/doc 等
/// - pdf_path 列重命名为 source_file_path（SQLite 3.25+ 支持 RENAME COLUMN）
/// - 旧 pdf_path 不为空的行回填 source_file_type='pdf'
fn migrate_v8_to_v9(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v8 -> v9 (pdf_path → source_file_path + source_file_type)");

    conn.execute_batch(
        "
        ALTER TABLE notes ADD COLUMN source_file_type TEXT;
        ALTER TABLE notes RENAME COLUMN pdf_path TO source_file_path;
        UPDATE notes SET source_file_type = 'pdf' WHERE source_file_path IS NOT NULL;
        ",
    )?;

    set_version(conn, 9)?;
    Ok(())
}

/// v9 -> v10: 自愈迁移
///
/// 修复 v9 在某些环境下未完整执行的问题（user_version 已推到 9 但列没补齐）。
/// 通过 PRAGMA table_info 探测当前列状态，缺啥补啥，幂等可重跑。
///
/// 目标终态：notes 表必有 source_file_path 与 source_file_type 两列。
fn migrate_v9_to_v10(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v9 -> v10 (自愈 source_file_path / source_file_type)");

    let cols = list_columns(conn, "notes")?;
    let has_path = cols.iter().any(|c| c == "source_file_path");
    let has_type = cols.iter().any(|c| c == "source_file_type");
    let has_pdf = cols.iter().any(|c| c == "pdf_path");

    // 处理 source_file_path
    if !has_path {
        if has_pdf {
            log::info!("[v10 自愈] RENAME COLUMN pdf_path -> source_file_path");
            conn.execute_batch("ALTER TABLE notes RENAME COLUMN pdf_path TO source_file_path;")?;
        } else {
            log::info!("[v10 自愈] ADD COLUMN source_file_path");
            conn.execute_batch("ALTER TABLE notes ADD COLUMN source_file_path TEXT;")?;
        }
    } else if has_pdf {
        // 极端情况：两列都存在，把 pdf_path 残留数据合并过去
        log::info!("[v10 自愈] 合并残留 pdf_path 数据到 source_file_path");
        conn.execute_batch(
            "UPDATE notes SET source_file_path = pdf_path
             WHERE source_file_path IS NULL AND pdf_path IS NOT NULL;",
        )?;
        // 不 DROP COLUMN pdf_path，避免触发 FTS 触发器引用问题；不影响功能
    }

    // 处理 source_file_type
    if !has_type {
        log::info!("[v10 自愈] ADD COLUMN source_file_type");
        conn.execute_batch("ALTER TABLE notes ADD COLUMN source_file_type TEXT;")?;
    }

    // 回填类型（只填还没值的行）
    conn.execute_batch(
        "UPDATE notes SET source_file_type = 'pdf'
         WHERE source_file_path IS NOT NULL AND source_file_type IS NULL;",
    )?;

    set_version(conn, 10)?;
    Ok(())
}

/// v10 -> v11: 新增同步历史表（sync_history）
fn migrate_v10_to_v11(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v10 -> v11（同步历史表）");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sync_history (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            direction    TEXT NOT NULL,      -- 'export' / 'import' / 'push' / 'pull'
            started_at   TEXT NOT NULL,
            finished_at  TEXT,
            success      INTEGER NOT NULL DEFAULT 0,
            error        TEXT,
            stats_json   TEXT NOT NULL DEFAULT '{}'
        );

        CREATE INDEX IF NOT EXISTS idx_sync_history_started ON sync_history(started_at DESC);
        ",
    )?;

    set_version(conn, 11)?;
    Ok(())
}

/// v11 -> v12: 新增待办任务表 + 任务关联表
fn migrate_v11_to_v12(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v11 -> v12（待办任务）");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS tasks (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            title        TEXT NOT NULL,
            description  TEXT,
            priority     INTEGER NOT NULL DEFAULT 1,  -- 0=urgent / 1=normal / 2=low
            important    INTEGER NOT NULL DEFAULT 0,  -- 0/1 艾森豪威尔重要性维度
            status       INTEGER NOT NULL DEFAULT 0,  -- 0=todo / 1=done
            due_date     TEXT,                        -- 'YYYY-MM-DD'，NULL 表示无截止
            completed_at TEXT,                        -- 完成时间（ISO）
            created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_status    ON tasks(status);
        CREATE INDEX IF NOT EXISTS idx_tasks_due_date  ON tasks(due_date);
        CREATE INDEX IF NOT EXISTS idx_tasks_priority  ON tasks(priority);

        -- 任务关联（多态）：一个任务可以挂多个笔记 / 路径 / URL
        CREATE TABLE IF NOT EXISTS task_links (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id    INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            kind       TEXT NOT NULL,          -- 'note' / 'path' / 'url'
            target     TEXT NOT NULL,          -- note_id 字符串 / 绝对路径 / URL
            label      TEXT,                   -- 展示文案（如笔记标题）
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_task_links_task ON task_links(task_id);
        ",
    )?;

    set_version(conn, 12)?;
    Ok(())
}

/// v12 -> v13: 为 HTML → Markdown 迁移做准备
///
/// 思路：笔记存储最终要切到 Markdown，但现存 content 全是 HTML。
/// 本次迁移只做**一次性备份**，不动任何代码逻辑：
///   1. notes 表新增 content_html 字段（幂等）
///   2. 把现有 content（HTML）整段拷贝到 content_html 做兜底
///
/// 后续阶段：
///   · 阶段 2：接入 tiptap-markdown，编辑器切 MD I/O
///   · 阶段 3：批量把 content_html → Markdown 写回 content
///   · 阶段 4：清理 strip_html 等遗留逻辑
///
/// 即便后续翻车，content_html 始终保留原始 HTML，可以随时回滚。
fn migrate_v12_to_v13(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v12 -> v13 (notes 新增 content_html 备份字段)");

    let cols = list_columns(conn, "notes")?;
    if !cols.iter().any(|c| c == "content_html") {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN content_html TEXT;")?;
    }

    // 幂等回填：仅对尚未备份的行执行
    conn.execute_batch(
        "UPDATE notes
            SET content_html = content
          WHERE content_html IS NULL AND content IS NOT NULL;",
    )?;

    set_version(conn, 13)?;
    Ok(())
}

/// v13 -> v14: 批量把 notes.content 从 HTML 转成 Markdown
///
/// 配合前端 Tiptap 切换到 Markdown I/O 模式，数据库内容格式也从 HTML 切到 MD。
/// 依赖 v13 已经把原 HTML 备份到 content_html，本步骤可随时回滚。
///
/// 回滚 SQL（仅开发者手动执行）：
///   UPDATE notes SET content = content_html WHERE content_html IS NOT NULL;
fn migrate_v13_to_v14(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v13 -> v14 (notes.content HTML → Markdown)");

    // 1) 取出所有待转换的笔记（content 非空且未被清空的）
    let mut stmt =
        conn.prepare("SELECT id, content FROM notes WHERE content IS NOT NULL AND content != ''")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    log::info!("[v14] 准备转换 {} 条笔记", rows.len());

    // 2) 一次事务内批量更新
    let tx = conn.unchecked_transaction()?;
    for (id, html) in &rows {
        let md = crate::services::markdown::html_to_markdown(html);
        tx.execute(
            "UPDATE notes SET content = ?1 WHERE id = ?2",
            rusqlite::params![md, id],
        )?;
    }
    tx.commit()?;

    log::info!("[v14] 转换完成");
    set_version(conn, 14)?;
    Ok(())
}

/// v14 -> v15: 待办任务增加定时提醒字段
///
/// due_date 字段保留原名，但字符串格式从仅 'YYYY-MM-DD' 扩展为可选带时分
/// ('YYYY-MM-DD HH:MM:SS')。旧数据不迁移，继续视作全天截止。
///
/// 新增两列：
///   · remind_before_minutes：提前 N 分钟提醒，NULL = 不提醒
///   · reminded_at：上次触发提醒的时刻（ISO），用于去重
fn migrate_v14_to_v15(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v14 -> v15 (tasks 定时提醒字段)");

    let cols = list_columns(conn, "tasks")?;
    if !cols.iter().any(|c| c == "remind_before_minutes") {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN remind_before_minutes INTEGER;")?;
    }
    if !cols.iter().any(|c| c == "reminded_at") {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN reminded_at TEXT;")?;
    }

    set_version(conn, 15)?;
    Ok(())
}

/// v15 -> v16: 补 note_links.source_id 索引
///
/// 原先只建了 idx_note_links_target（反向链接查询走这条），
/// 但保存笔记时 `DELETE FROM note_links WHERE source_id = ?1` 没有 source_id 单列索引可用。
/// 笔记数量大时该 DELETE 会退化为全表扫描，导致保存明显变慢。
fn migrate_v15_to_v16(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v15 -> v16 (补 note_links.source_id 索引)");

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_note_links_source ON note_links(source_id);",
    )?;

    set_version(conn, 16)?;
    Ok(())
}

/// v16 -> v17: notes 新增 title_normalized 列 + 索引，解除 wiki 链接匹配的全表扫
///
/// 背景：`find_note_id_by_title_loose` 是 [[wiki-link]] 编辑器自动补全、保存时链接同步
/// 的热路径。老实现 `SELECT id, title FROM notes WHERE is_deleted = 0` 全表拉回来，
/// 再在 Rust 侧对每行 title 做 `normalize_title`（去转义 + 空白折叠 + lowercase）再比较。
/// 10k 笔记时每次调用要几十毫秒，打字时卡顿肉眼可见。
///
/// 本迁移：
/// 1) ALTER TABLE 新增 title_normalized 列（幂等）
/// 2) 用 Rust 侧 `normalize_title` 批量回填（保证和运行时比较使用同一套规则）
/// 3) 建部分索引 `idx_notes_title_normalized WHERE is_deleted = 0`
///
/// 之后 `find_note_id_by_title_loose` 直接 `WHERE title_normalized = ?`，走 O(log n) 索引。
///
/// **DAO 协议**：`create_note` / `update_note` / `get_or_create_daily` 写入时必须同步
/// 维护 `title_normalized`。老数据一次性回填后不再需要运行时 fallback。
fn migrate_v16_to_v17(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v16 -> v17 (notes.title_normalized + 索引)");

    let cols = list_columns(conn, "notes")?;
    if !cols.iter().any(|c| c == "title_normalized") {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN title_normalized TEXT;")?;
    }

    // 回填：仅对 title_normalized IS NULL 的行（幂等可重跑）
    let mut stmt = conn.prepare("SELECT id, title FROM notes WHERE title_normalized IS NULL")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    log::info!("[v17] 准备回填 {} 条笔记的 title_normalized", rows.len());

    let tx = conn.unchecked_transaction()?;
    for (id, title) in &rows {
        let norm = crate::database::links::normalize_title(title);
        tx.execute(
            "UPDATE notes SET title_normalized = ?1 WHERE id = ?2",
            rusqlite::params![norm, id],
        )?;
    }
    tx.commit()?;

    // 部分索引：只对活跃笔记建索引，更紧凑
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_notes_title_normalized
         ON notes(title_normalized) WHERE is_deleted = 0;",
    )?;

    set_version(conn, 17)?;
    Ok(())
}

/// v17 -> v18: tasks 新增循环提醒字段
///
/// 原 v15 给任务加了"提前 N 分钟提醒 + reminded_at 去重"，只能提醒一次。
/// 本迁移补上循环规则，让待办可按"每天/每周某几天/每月/每 N 天"反复提醒。
///
/// 新增列：
///   · repeat_kind        'none'/'daily'/'weekly'/'monthly'，默认 'none'
///   · repeat_interval    每 N 个单位（默认 1）
///   · repeat_weekdays    '1,2,3,4,5'（1=Mon..7=Sun），仅 weekly 有效；NULL 表示按 interval 周
///   · repeat_until       'YYYY-MM-DD'，循环终止日期；NULL 表示无上限
///   · repeat_count       总触发次数上限（含首次）；NULL 表示无上限
///   · repeat_done_count  已触发次数，默认 0
fn migrate_v17_to_v18(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v17 -> v18 (tasks 循环提醒字段)");

    let cols = list_columns(conn, "tasks")?;
    if !cols.iter().any(|c| c == "repeat_kind") {
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN repeat_kind TEXT NOT NULL DEFAULT 'none';",
        )?;
    }
    if !cols.iter().any(|c| c == "repeat_interval") {
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN repeat_interval INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    if !cols.iter().any(|c| c == "repeat_weekdays") {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN repeat_weekdays TEXT;")?;
    }
    if !cols.iter().any(|c| c == "repeat_until") {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN repeat_until TEXT;")?;
    }
    if !cols.iter().any(|c| c == "repeat_count") {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN repeat_count INTEGER;")?;
    }
    if !cols.iter().any(|c| c == "repeat_done_count") {
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN repeat_done_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    set_version(conn, 18)?;
    Ok(())
}

/// v18 -> v19: AI 提示词库（prompt_templates）+ 7 条内置模板
///
/// 背景：编辑器 AI 菜单原本硬编码了 7 个 action（续写/总结/改写/扩展/精简/译英/译中），
/// 用户没法加自己的 Prompt，也没法改内置文案。本迁移把模板迁移到 DB：
///   · is_builtin=1 + builtin_code=xxx 的行是内置，首次安装写入；
///   · 用户自定义模板 is_builtin=0；
///   · 菜单改为读 DB 列表，点击时走 `ai_write_assist` 的 `prompt:{id}` 分支。
///
/// 字段说明：
///   · output_mode: 'replace'（替换选区，默认） / 'append'（追加到选区末尾，续写场景） / 'popup'（仅展示，如总结）
///   · builtin_code: 和旧硬编码 action 保持一致，万一前端旧版本传入也能映射到 DB
///   · sort_order: 越小越靠前，内置占 10/20/30… 让用户插队有空间
fn migrate_v18_to_v19(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v18 -> v19 (prompt_templates + 内置模板)");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS prompt_templates (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            title         TEXT NOT NULL,
            description   TEXT NOT NULL DEFAULT '',
            prompt        TEXT NOT NULL,
            output_mode   TEXT NOT NULL DEFAULT 'replace',
            icon          TEXT,
            is_builtin    INTEGER NOT NULL DEFAULT 0,
            builtin_code  TEXT UNIQUE,
            sort_order    INTEGER NOT NULL DEFAULT 0,
            enabled       INTEGER NOT NULL DEFAULT 1,
            created_at    TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at    TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_prompt_templates_sort
            ON prompt_templates(sort_order, id);
        ",
    )?;

    // 内置模板（首次插入，INSERT OR IGNORE 保证再跑不覆盖用户修改）
    //
    // 所有 prompt 用 {{selection}} / {{context}} / {{title}} 三个占位符，
    // services/prompt.rs 的 render 函数会在调用 AI 前做字符串替换。
    //
    // 短模板为主，长指令保留给用户自行 fork，避免内置"太啰嗦"。
    let builtins: &[(&str, &str, &str, &str, &str, i32)] = &[
        ("续写", "根据上下文自然地续写", "你是一个写作助手。请根据下面的上下文和已有内容，自然地续写下去。只输出续写的新内容，不要重复已有内容。使用中文。\n\n【上下文】\n{{context}}\n\n【已有内容】\n{{selection}}",
         "append", "ArrowRight", 10),
        ("总结", "提炼关键信息", "你是一个写作助手。请对以下文本进行简洁的总结概括，突出关键信息和核心观点。使用中文。\n\n【原文】\n{{selection}}",
         "popup", "FileText", 20),
        ("改写", "优化表达让文本更流畅", "你是一个写作助手。请改写以下文本，使其表达更加流畅、专业。保持原意不变。只输出改写后的内容，不要解释。使用中文。\n\n【原文】\n{{selection}}",
         "replace", "RefreshCw", 30),
        ("扩展", "补充细节和论述", "你是一个写作助手。请对以下文本进行扩展，补充更多细节、论据或例子。保持原有观点不变。使用中文。\n\n【原文】\n{{selection}}",
         "replace", "Expand", 40),
        ("精简", "去掉冗余保留核心", "你是一个写作助手。请精简以下文本，保留核心信息，去除冗余表达。只输出精简后的内容。使用中文。\n\n【原文】\n{{selection}}",
         "replace", "Shrink", 50),
        ("译英", "翻译成地道英文", "你是一个翻译助手。请将以下文本翻译成地道的英文。只输出翻译结果，不要解释。\n\n【原文】\n{{selection}}",
         "replace", "Languages", 60),
        ("译中", "翻译成准确中文", "你是一个翻译助手。请将以下文本翻译成准确、通顺的中文。只输出翻译结果，不要解释。\n\n【原文】\n{{selection}}",
         "replace", "Languages", 70),
    ];

    // builtin_code 对应旧硬编码 action
    let codes = [
        "continue",
        "summarize",
        "rewrite",
        "expand",
        "shorten",
        "translate_en",
        "translate_zh",
    ];

    for (i, (title, desc, prompt, mode, icon, sort)) in builtins.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO prompt_templates
                (title, description, prompt, output_mode, icon, is_builtin, builtin_code, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7)",
            rusqlite::params![title, desc, prompt, mode, icon, codes[i], sort],
        )?;
    }

    set_version(conn, 19)?;
    Ok(())
}

/// v19 -> v20: ai_messages 加 skill_calls_json 字段
///
/// 用途：T-004 Skills 框架下，assistant 消息里可能发生一次或多次 tool_call
/// （`search_notes` / `get_note` 等）。把每次调用（name + args + result + status）
/// 序列化成 JSON 数组存到这一列，便于：
///   1. 重开对话时重绘 SkillCall 折叠卡片
///   2. 诊断问题（AI 为啥调了这个工具、返回啥）
///
/// 为什么新增独立列而不是塞进 references_json：
///   - references_json 是纯 note id 数组（给 UI 标"引用的笔记"用）
///   - skill_calls_json 结构复杂（包含 args/result/status），语义完全不同
fn migrate_v19_to_v20(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v19 -> v20 (ai_messages.skill_calls_json)");

    let cols = list_columns(conn, "ai_messages")?;
    if !cols.iter().any(|c| c == "skill_calls_json") {
        conn.execute_batch("ALTER TABLE ai_messages ADD COLUMN skill_calls_json TEXT;")?;
    }

    set_version(conn, 20)?;
    Ok(())
}

/// v20 -> v21: notes 新增 is_hidden 字段（T-003 笔记"隐藏"标记）
///
/// 语义是"弱隐藏"：默认视图（笔记列表 / 搜索 / 反链 / 图谱 / RAG）完全看不见；
/// 但 wiki link [[...]] 点击跳转仍允许打开，保证链接不失效。
///
/// 这不是加密——数据库文件打开还是能看到内容。加密放 T-007。
///
/// 部分索引只建在"活跃笔记"上（is_deleted=0），避免回收站的 hidden 条目
/// 干扰热路径查询。
fn migrate_v20_to_v21(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v20 -> v21 (notes.is_hidden)");

    let cols = list_columns(conn, "notes")?;
    if !cols.iter().any(|c| c == "is_hidden") {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN is_hidden INTEGER NOT NULL DEFAULT 0;")?;
    }

    // is_hidden 出现在 WHERE 条件里很频繁（所有主路径查询都加 is_hidden=0），
    // 建部分索引帮助过滤；索引只覆盖"活跃"笔记，和现有 idx_notes_pinned 的思路一致
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_notes_hidden
         ON notes(is_hidden, updated_at DESC) WHERE is_deleted = 0;",
    )?;

    set_version(conn, 21)?;
    Ok(())
}

/// v21 -> v22: notes 新增 content_hash 字段 + 索引 + 存量回填
///
/// 背景：导入 Markdown 文件夹时，原实现不做去重，同一批文件反复导入会产生重复笔记。
/// 本迁移把"内容指纹"持久化到 notes.content_hash（SHA-256 十六进制串），
/// 后续扫描外部 md 时按 (title, content_hash) 做兜底匹配（source_file_path 为主判）。
///
/// DAO 协议：`create_note` / `update_note` / `update_note_content` / `get_or_create_daily`
/// 写入正文时必须同步维护 content_hash。存量笔记由本迁移一次性回填。
///
/// 部分索引只覆盖活跃笔记（is_deleted=0），和 idx_notes_pinned / idx_notes_hidden 的思路一致。
fn migrate_v21_to_v22(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v21 -> v22 (notes.content_hash)");

    let cols = list_columns(conn, "notes")?;
    if !cols.iter().any(|c| c == "content_hash") {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN content_hash TEXT;")?;
    }

    // 回填：仅对 content_hash IS NULL 的行（幂等可重跑）
    let mut stmt = conn.prepare("SELECT id, content FROM notes WHERE content_hash IS NULL")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    log::info!("[v22] 准备回填 {} 条笔记的 content_hash", rows.len());

    let tx = conn.unchecked_transaction()?;
    for (id, content) in &rows {
        let hash = crate::services::hash::sha256_hex(content);
        tx.execute(
            "UPDATE notes SET content_hash = ?1 WHERE id = ?2",
            rusqlite::params![hash, id],
        )?;
    }
    tx.commit()?;

    // 部分索引：只对活跃笔记建索引
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_notes_content_hash
         ON notes(content_hash) WHERE is_deleted = 0;",
    )?;

    set_version(conn, 22)?;
    Ok(())
}

/// v22 -> v23: 笔记加密基础字段（T-007 笔记加密保险库）
///
/// - `notes.is_encrypted` 0/1：是否处于加密态
/// - `notes.encrypted_blob` BLOB：密文全量包（nonce ‖ ciphertext ‖ tag）
/// - vault 主密码相关写 app_config：
///   - `vault.salt`       16 字节 base64（派生 key 用的盐）
///   - `vault.verifier`   加密后的常量字符串（用于解锁时校验密码对不对，不泄露 key）
///
/// 设计取舍：
/// 1. **App 层加密**（B1 方案）：密文存在现有 notes 表的新 BLOB 列里，不换 SQLCipher
/// 2. **加密笔记的 content 列保留"🔒 已加密"占位**：这样老代码读取 content 时不会看到乱码；
///    FTS5 索引到的也是这个占位，自然过滤掉加密笔记的搜索命中
/// 3. 忘记主密码 = 数据丢失（T-007 决策 ④）：verifier 不是 key，靠解密校验密码
fn migrate_v22_to_v23(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v22 -> v23 (notes 加密字段 + vault 基础)");

    let cols = list_columns(conn, "notes")?;
    if !cols.iter().any(|c| c == "is_encrypted") {
        conn.execute_batch(
            "ALTER TABLE notes ADD COLUMN is_encrypted INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !cols.iter().any(|c| c == "encrypted_blob") {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN encrypted_blob BLOB;")?;
    }

    // 部分索引，过滤/定位加密笔记的常用热路径
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_notes_encrypted
         ON notes(is_encrypted) WHERE is_deleted = 0;",
    )?;

    // vault 相关配置是可选的——不预置，在首次 setup 时由代码写入

    set_version(conn, 23)?;
    Ok(())
}

/// v23 → v24: T-024 同步架构 V1
///
/// 新表 `sync_backends`：用户配置的同步后端列表（可同时挂多个）
///   - `id` 自增主键
///   - `kind`：local / webdav / s3 / git
///   - `name`：用户起的名字（如"我的坚果云"）
///   - `config_json`：backend 专属配置（路径 / endpoint / bucket / 凭据等，凭据已用 vault 加密）
///   - `enabled` / `auto_sync` / `created_at`
///
/// 新表 `sync_remote_state`：每条笔记 × 每个 backend 的同步状态
///   - 唯一键 (backend_id, note_id)
///   - `last_synced_hash`：上次同步时的笔记内容 SHA-256（供 diff 用）
///   - `last_synced_ts`：上次同步成功时间（last-write-wins 的依据之一）
///   - `remote_path`：在 backend 上的相对路径（如 "notes/<uuid>.md"）
///   - `tombstone`：本地已删除标记（同步后告诉远端也删，远端确认后才能从表里移除）
///
/// 设计要点：
/// 1. **不动 notes 表本身**：所有同步元数据放独立表，未启用同步的用户零成本
/// 2. **per-backend 状态独立**：用户可以同时配 LocalPath + WebDAV + S3，互不干扰
/// 3. **soft delete 走 tombstone**：硬删笔记时同步表保留 tombstone 行，下次 sync 推出删除
fn migrate_v23_to_v24(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v23 -> v24 (T-024 同步 V1: sync_backends + sync_remote_state)");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sync_backends (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            kind         TEXT    NOT NULL,             -- 'local' | 'webdav' | 's3' | 'git'
            name         TEXT    NOT NULL,
            config_json  TEXT    NOT NULL DEFAULT '{}',
            enabled      INTEGER NOT NULL DEFAULT 1,
            auto_sync    INTEGER NOT NULL DEFAULT 0,
            sync_interval_min INTEGER NOT NULL DEFAULT 30,
            last_push_ts TEXT,
            last_pull_ts TEXT,
            created_at   DATETIME NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at   DATETIME NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        CREATE TABLE IF NOT EXISTS sync_remote_state (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            backend_id         INTEGER NOT NULL,
            note_id            INTEGER NOT NULL,
            remote_path        TEXT    NOT NULL,
            last_synced_hash   TEXT    NOT NULL,
            last_synced_ts     TEXT    NOT NULL,
            tombstone          INTEGER NOT NULL DEFAULT 0,
            UNIQUE (backend_id, note_id),
            FOREIGN KEY (backend_id) REFERENCES sync_backends(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_sync_remote_state_backend
            ON sync_remote_state(backend_id);
        CREATE INDEX IF NOT EXISTS idx_sync_remote_state_note
            ON sync_remote_state(note_id);
        "#,
    )?;

    set_version(conn, 24)?;
    Ok(())
}

/// v24 -> v25: AI 双向打通笔记
///   1. ai_models 加 max_context（用户可在设置页填模型上下文窗口大小，
///      默认 32000，给前端动态计算附加笔记的截断阈值用）
///   2. ai_conversations 加 attached_note_ids（JSON 数组字符串，挂在对话级，
///      整个对话共享一组附加笔记，类比 ChatGPT 项目）
///   3. notes 加 from_ai_conversation_id（归档来源追溯，给 B 方向"AI → 笔记"用）
fn migrate_v24_to_v25(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v24 -> v25");

    conn.execute_batch(
        r#"
        ALTER TABLE ai_models
            ADD COLUMN max_context INTEGER NOT NULL DEFAULT 32000;

        ALTER TABLE ai_conversations
            ADD COLUMN attached_note_ids TEXT NOT NULL DEFAULT '[]';

        ALTER TABLE notes
            ADD COLUMN from_ai_conversation_id INTEGER REFERENCES ai_conversations(id) ON DELETE SET NULL;

        CREATE INDEX IF NOT EXISTS idx_notes_from_ai_conv
            ON notes(from_ai_conversation_id)
            WHERE from_ai_conversation_id IS NOT NULL;
        "#,
    )?;

    set_version(conn, 25)?;
    Ok(())
}

/// v25 -> v26: 笔记伴生 AI 对话
///
/// `companion_conversation_id` 给"在编辑器右侧抽屉里问 AI"功能用：
/// 每篇笔记懒创建一个独立 AI 对话，下次打开同笔记自动复用对话历史。
/// 删除笔记时如果对话还在，对话不会被强制删（用户可能想保留聊天记录），
/// 这里 ON DELETE SET NULL 让对话自由存在。
fn migrate_v25_to_v26(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v25 -> v26");
    conn.execute_batch(
        r#"
        ALTER TABLE notes
            ADD COLUMN companion_conversation_id INTEGER
            REFERENCES ai_conversations(id) ON DELETE SET NULL;

        CREATE INDEX IF NOT EXISTS idx_notes_companion_conv
            ON notes(companion_conversation_id)
            WHERE companion_conversation_id IS NOT NULL;
        "#,
    )?;
    set_version(conn, 26)?;
    Ok(())
}

/// v26 -> v27: tasks 表加 source_batch_id（AI 批量导入用，支持一键撤销整批）
///
/// 当用户用「AI 智能规划」一次生成 N 条待办时，所有同批次任务共享一个 batch_id，
/// 后续可以按 batch_id 批量删除/撤销，避免用户手动一条条清理。
/// 老数据 source_batch_id 为 NULL，自然不参与批次操作。
fn migrate_v26_to_v27(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v26 -> v27 (tasks 新增 source_batch_id)");

    let cols = list_columns(conn, "tasks")?;
    if !cols.iter().any(|c| c == "source_batch_id") {
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN source_batch_id TEXT;")?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tasks_source_batch
            ON tasks(source_batch_id)
            WHERE source_batch_id IS NOT NULL;",
    )?;

    set_version(conn, 27)?;
    Ok(())
}

/// v27 -> v28: 外部 .md 双向同步基础设施
///
/// 1. 新表 `note_url_mapping`：记录笔记里每张图的"内部 URL ↔ 原始 URL"映射
///    - 打开 .md 时：原始链接（./images/foo.png 或 https://...）→ 内部 asset.localhost URL
///      在收集替换的同时把这一对落库；
///    - 写回 .md 时：扫笔记 content 的所有 URL，命中映射就反查替换回原始 URL
///      → 原文件链接保持原样，不污染用户的图床/相对路径写法。
///    - 用户在编辑器里新插的图（不在映射表）按"复制到 <basename>.assets/"策略处理。
///    UNIQUE (note_id, internal_url) 保证同一张图不会被反复写多条。
///
/// 2. notes 表新增 `last_writeback_mtime`：上次成功写回原 .md 时该文件的 mtime（秒级时间戳）。
///    每次写回前比对：若磁盘当前 mtime ≠ 此值，说明外部编辑器（VSCode 等）改过文件，
///    弹冲突 Modal 让用户选「覆盖外部 / 保留外部 / 取消」。
fn migrate_v27_to_v28(conn: &Connection) -> Result<(), AppError> {
    log::info!(
        "数据库迁移: v27 -> v28 (外部 .md 双向同步: note_url_mapping + last_writeback_mtime)"
    );

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS note_url_mapping (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id       INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            internal_url  TEXT    NOT NULL,
            original_url  TEXT    NOT NULL,
            created_at    TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
            UNIQUE (note_id, internal_url)
        );

        CREATE INDEX IF NOT EXISTS idx_url_mapping_note ON note_url_mapping(note_id);
        ",
    )?;

    let cols = list_columns(conn, "notes")?;
    if !cols.iter().any(|c| c == "last_writeback_mtime") {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN last_writeback_mtime INTEGER;")?;
    }

    set_version(conn, 28)?;
    Ok(())
}

/// v28 -> v29: 把笔记 content 里的素材绝对路径替换成相对协议 `kb-asset://`
///
/// 历史背景：旧版前端用 `convertFileSrc(absolute)` 直接把 `http://asset.localhost/<URL编码的绝对路径>`
/// 写进笔记 content。这导致一旦用户改变数据目录（指针文件 / KB_DATA_DIR），
/// 笔记里 src 仍指向旧位置，文件读不到 → 全裂图。
///
/// 治本方案：content 里只存 `kb-asset://<相对 data_dir 的 POSIX 路径>`，
/// 渲染时由前端 MutationObserver 实时拼当前 data_dir 解析。
///
/// 本迁移 = 一次性数据清洗：
/// 1. 正则扫每条笔记 content，匹配 `http://asset.localhost/<encoded>` 与 `asset://localhost/<encoded>`
/// 2. URL-decode 拿到原始绝对路径
/// 3. 用 `services::asset_path::abs_to_rel` 把绝对路径转相对（支持 fallback 找已知子目录段，
///    解决"绝对路径来自旧机器/旧 data_dir、当前 data_dir 不是其前缀"的场景）
/// 4. 替换为 `kb-asset://<rel>` 写回
///
/// 失败兜底：路径既不在 data_dir 下也找不到已知段名时保留原样（极少见，迁移日志会告警）。
/// 跑完后 content 里出现的 `http://asset.localhost/...` 全部应为 0；遗留的视为外链。
fn migrate_v28_to_v29(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v28 -> v29 (笔记 content 绝对资产路径 → kb-asset://)");

    use regex::Regex;

    // 匹配两种 asset 协议前缀，捕获后面的 URL 编码部分（直到引号/空格/<>/换行/反引号）
    let re = Regex::new(
        r#"(?P<scheme>(?:http://asset\.localhost/|asset://localhost/))(?P<path>[^"'\s<>`]+)"#,
    )
    .expect("正则字面量恒定，编译失败属于代码 BUG");

    let mut stmt =
        conn.prepare("SELECT id, content FROM notes WHERE content IS NOT NULL AND content != ''")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    log::info!("[v29] 扫描 {} 条笔记里的 asset URL", rows.len());

    // 临时拿一个 dummy data_dir：abs_to_rel 在 fallback 路径里只用 known segments 切，
    // 不实际依赖 data_dir。准确 strip_prefix 走不通时 fallback 即可。
    let dummy_data_dir = std::path::Path::new("");

    let mut replaced_notes = 0usize;
    let mut replaced_urls = 0usize;
    let mut unresolved = 0usize;

    let tx = conn.unchecked_transaction()?;
    for (id, content) in &rows {
        let mut changed_in_this_note = false;
        let new_content = re.replace_all(content, |caps: &regex::Captures<'_>| -> String {
            let encoded = caps.name("path").map(|m| m.as_str()).unwrap_or("");
            let decoded = match urlencoding::decode(encoded) {
                Ok(s) => s.into_owned(),
                Err(_) => return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string(),
            };
            let abs = std::path::Path::new(&decoded);
            match crate::services::asset_path::abs_to_rel(abs, dummy_data_dir) {
                Some(rel) => {
                    replaced_urls += 1;
                    changed_in_this_note = true;
                    format!("kb-asset://{}", rel)
                }
                None => {
                    unresolved += 1;
                    log::warn!(
                        "[v29] 笔记 {} 中 asset 路径无法解析为相对路径，保留原样: {}",
                        id,
                        decoded
                    );
                    caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
                }
            }
        });

        if changed_in_this_note {
            tx.execute(
                "UPDATE notes SET content = ?1 WHERE id = ?2",
                rusqlite::params![new_content.as_ref(), id],
            )?;
            replaced_notes += 1;
        }
    }
    tx.commit()?;

    log::info!(
        "[v29] 迁移完成：触达 {} 条笔记，替换 {} 个 asset URL，{} 个无法解析（已保留）",
        replaced_notes,
        replaced_urls,
        unresolved
    );

    set_version(conn, 29)?;
    Ok(())
}

/// v29 → v30: 待办任务一级分类
///
/// 新表 `task_categories`：用户自定义分类（彩色圆点 + 名称 + 排序）
/// `tasks.category_id`：外键，NULL = 未分类（虚拟分类）
///
/// 设计：
/// - `ON DELETE SET NULL`：删分类时任务回落到未分类，不级联删任务
/// - 不预置种子数据，让用户首次进设置页自己建（避免清理负担）
fn migrate_v29_to_v30(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v29 -> v30 (待办分类: task_categories + tasks.category_id)");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS task_categories (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL UNIQUE,
            color      TEXT NOT NULL DEFAULT '#1677ff',
            icon       TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        ",
    )?;

    let cols = list_columns(conn, "tasks")?;
    if !cols.iter().any(|c| c == "category_id") {
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN category_id INTEGER
                REFERENCES task_categories(id) ON DELETE SET NULL;",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tasks_category
            ON tasks(category_id) WHERE category_id IS NOT NULL;",
    )?;

    set_version(conn, 30)?;
    Ok(())
}

/// v30 -> v31: notes 加 sort_order 字段（笔记自定义排序）
///
/// 设计：
/// - INTEGER NOT NULL DEFAULT 0；越小越靠前；同 folder 内按 1000 间隔留空隙
///   留给未来插队（同 folders.sort_order 一致的模式）
/// - 初始化：每个 folder_id 分组内按 (updated_at DESC, id ASC) 给序号 *1000
///   未分类（folder_id IS NULL）的笔记按 -1 单独分组
/// - 索引 idx_notes_folder_sort 覆盖 (folder_id, sort_order)，is_deleted=0
///   的部分索引，与 idx_notes_folder / idx_notes_pinned 思路一致
fn migrate_v30_to_v31(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v30 -> v31 (notes.sort_order 自定义排序)");

    let cols = list_columns(conn, "notes")?;
    if !cols.iter().any(|c| c == "sort_order") {
        conn.execute_batch("ALTER TABLE notes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;")?;
    }

    // 初始化已有数据：同一 folder 内按修改时间倒序分配 0/1000/2000...
    // ROW_NUMBER 在 SQLite 3.25+ 提供，rusqlite bundled 自带的版本远高于此
    conn.execute_batch(
        "
        WITH ranked AS (
            SELECT id,
                   (ROW_NUMBER() OVER (
                        PARTITION BY COALESCE(folder_id, -1)
                        ORDER BY updated_at DESC, id ASC
                   ) - 1) * 1000 AS new_order
            FROM notes
            WHERE is_deleted = 0
        )
        UPDATE notes
        SET sort_order = (SELECT new_order FROM ranked WHERE ranked.id = notes.id)
        WHERE id IN (SELECT id FROM ranked);
        ",
    )?;

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_notes_folder_sort
            ON notes(folder_id, sort_order) WHERE is_deleted = 0;",
    )?;

    set_version(conn, 31)?;
    Ok(())
}

/// v31 -> v32: tasks 加 parent_task_id（子任务支持）
///
/// 设计：
/// - parent_task_id NULL → 主任务（出现在主列表）
/// - parent_task_id 非 NULL → 子任务（在主任务详情下显示）
/// - ON DELETE CASCADE：删主任务自动删所有子任务，避免孤儿
/// - 部分索引只覆盖子任务（非 NULL），节省空间
///
/// 不限制嵌套层级（DB 层允许多层），但前端 UI 默认只展示 1 层 —— 与
/// Microsoft To Do / Things 一致的"步骤"模型，足够个人使用。
fn migrate_v31_to_v32(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v31 -> v32 (tasks.parent_task_id 子任务)");

    let cols = list_columns(conn, "tasks")?;
    if !cols.iter().any(|c| c == "parent_task_id") {
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN parent_task_id INTEGER
                REFERENCES tasks(id) ON DELETE CASCADE;",
        )?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_tasks_parent
            ON tasks(parent_task_id) WHERE parent_task_id IS NOT NULL;",
    )?;

    set_version(conn, 32)?;
    Ok(())
}

/// v32 -> v33: 外部 MCP server 注册表（M5-2）
///
/// 让用户可以在主应用里加任意 MCP server（GitHub / Filesystem / 高德地图…），
/// 自家 AI 对话页通过 services::mcp_client::McpClientManager 统一调用。
///
/// 字段说明：
/// - transport: 目前只支持 "stdio"（streamable-http 留给后续）
/// - command: 可执行文件路径或命令名（如 "npx" / 绝对路径）
/// - args: JSON array of strings，命令行参数
/// - env: JSON object，环境变量（OAuth token 等敏感配置走这里）
/// - enabled: 0/1，禁用时不会被 spawn
fn migrate_v32_to_v33(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v32 -> v33 (mcp_servers 外部 MCP 注册表)");

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mcp_servers (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL UNIQUE,
            transport   TEXT NOT NULL DEFAULT 'stdio',
            command     TEXT NOT NULL,
            args        TEXT NOT NULL DEFAULT '[]',
            env         TEXT NOT NULL DEFAULT '{}',
            enabled     INTEGER NOT NULL DEFAULT 1,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled);",
    )?;

    set_version(conn, 33)?;
    Ok(())
}

/// v33 -> v34: 闪卡 + FSRS 复习
///
/// `cards` 存卡片正反面 + FSRS 调度状态（下次到期/稳定度/难度等）。
/// `card_review_logs` 每次复习写一条历史，可用于参数优化（FSRS optimizer）和统计图表。
///
/// SRS 算法（FSRS）跑在前端 ts-fsrs，后端只负责持久化。前端复习时算出
/// 新的 (due/stability/difficulty/...) 一起传回 review_card 命令更新。
fn migrate_v33_to_v34(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v33 -> v34 (闪卡 + FSRS 复习)");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cards (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id         INTEGER REFERENCES notes(id) ON DELETE SET NULL,
            front           TEXT NOT NULL,
            back            TEXT NOT NULL,
            deck            TEXT NOT NULL DEFAULT 'default',

            -- FSRS 调度状态（默认值对应『新卡』）
            due             TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            stability       REAL NOT NULL DEFAULT 0,
            difficulty      REAL NOT NULL DEFAULT 0,
            elapsed_days    INTEGER NOT NULL DEFAULT 0,
            scheduled_days  INTEGER NOT NULL DEFAULT 0,
            reps            INTEGER NOT NULL DEFAULT 0,
            lapses          INTEGER NOT NULL DEFAULT 0,
            -- FSRS state: 0=New, 1=Learning, 2=Review, 3=Relearning
            state           INTEGER NOT NULL DEFAULT 0,
            last_review     TEXT,

            -- 软删除（与 notes 一致的回收站语义）
            is_deleted      INTEGER NOT NULL DEFAULT 0,

            created_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        CREATE INDEX IF NOT EXISTS idx_cards_due ON cards(due) WHERE is_deleted = 0;
        CREATE INDEX IF NOT EXISTS idx_cards_deck ON cards(deck) WHERE is_deleted = 0;
        CREATE INDEX IF NOT EXISTS idx_cards_note ON cards(note_id) WHERE is_deleted = 0;

        CREATE TABLE IF NOT EXISTS card_review_logs (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            card_id             INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
            -- 用户评分: 1=Again, 2=Hard, 3=Good, 4=Easy（与 ts-fsrs Rating 枚举一致）
            rating              INTEGER NOT NULL,
            state               INTEGER NOT NULL,
            due                 TEXT NOT NULL,
            stability           REAL NOT NULL,
            difficulty          REAL NOT NULL,
            elapsed_days        INTEGER NOT NULL,
            last_elapsed_days   INTEGER NOT NULL DEFAULT 0,
            scheduled_days      INTEGER NOT NULL,
            review              TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        CREATE INDEX IF NOT EXISTS idx_card_logs_card ON card_review_logs(card_id);
        CREATE INDEX IF NOT EXISTS idx_card_logs_review ON card_review_logs(review);
        ",
    )?;

    set_version(conn, 34)?;
    Ok(())
}

/// v34 -> v35: Obsidian 风格插件管理 MVP
///
/// 只落地插件元数据、启用状态、权限授权与插件设置持久化。
/// 第三方 JS 插件执行留给后续沙箱阶段，本迁移不引入代码执行能力。
fn migrate_v34_to_v35(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v34 -> v35 (插件系统 MVP)");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS plugins (
            id              TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            version         TEXT NOT NULL,
            description     TEXT,
            author          TEXT,
            path            TEXT NOT NULL,
            main            TEXT NOT NULL,
            styles          TEXT,
            min_app_version TEXT,
            manifest_json   TEXT NOT NULL,
            enabled         INTEGER NOT NULL DEFAULT 0,
            status          TEXT NOT NULL DEFAULT 'installed',
            installed_at    TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        CREATE INDEX IF NOT EXISTS idx_plugins_enabled ON plugins(enabled);

        CREATE TABLE IF NOT EXISTS plugin_permissions (
            plugin_id   TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
            permission  TEXT NOT NULL,
            granted     INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            PRIMARY KEY (plugin_id, permission)
        );

        CREATE TABLE IF NOT EXISTS plugin_settings (
            plugin_id   TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
            key         TEXT NOT NULL,
            value       TEXT NOT NULL,
            updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            PRIMARY KEY (plugin_id, key)
        );
        ",
    )?;

    set_version(conn, 35)?;
    Ok(())
}

/// v35 → v36: 插件设置键名前缀隔离 — 给已有 key 加 plugin:<id>: 前缀
fn migrate_v35_to_v36(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "UPDATE plugin_settings
           SET key = 'plugin:' || plugin_id || ':' || key
         WHERE key NOT LIKE 'plugin:%';",
    )?;
    set_version(conn, 36)?;
    Ok(())
}

/// v36 → v37: 插件审计日志表
fn migrate_v36_to_v37(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plugin_audit_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin_id   TEXT NOT NULL,
            operation   TEXT NOT NULL,
            target      TEXT,
            timestamp   TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_plugin_audit_plugin_id ON plugin_audit_log(plugin_id);
        CREATE INDEX IF NOT EXISTS idx_plugin_audit_ts ON plugin_audit_log(timestamp);",
    )?;
    set_version(conn, 37)?;
    Ok(())
}

/// v37 → v38: 插件完整性校验 — plugins 表加 content_hash 列
fn migrate_v37_to_v38(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch("ALTER TABLE plugins ADD COLUMN content_hash TEXT NOT NULL DEFAULT '';")?;
    set_version(conn, 38)?;
    Ok(())
}

/// v38 → v39: 任务执行会话 — 会话表 + 阶段表 + 执行日志表
fn migrate_v38_to_v39(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_sessions (
            id              TEXT PRIMARY KEY NOT NULL,
            plan_path       TEXT NOT NULL,
            plan_name       TEXT NOT NULL DEFAULT '',
            status          TEXT NOT NULL DEFAULT 'idle',
            current_phase_index INTEGER NOT NULL DEFAULT 0,
            total_phases    INTEGER NOT NULL DEFAULT 0,
            created_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        CREATE TABLE IF NOT EXISTS execution_phases (
            id              TEXT PRIMARY KEY NOT NULL,
            session_id      TEXT NOT NULL,
            index_num       INTEGER NOT NULL DEFAULT 0,
            name            TEXT NOT NULL DEFAULT '',
            description     TEXT NOT NULL DEFAULT '',
            status          TEXT NOT NULL DEFAULT 'pending',
            files_modified  TEXT,
            result_summary  TEXT,
            started_at      TEXT,
            finished_at     TEXT,
            FOREIGN KEY (session_id) REFERENCES task_sessions(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_phases_session ON execution_phases(session_id);

        CREATE TABLE IF NOT EXISTS execution_logs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  TEXT NOT NULL,
            phase_id    TEXT,
            level       TEXT NOT NULL DEFAULT 'info',
            message     TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            FOREIGN KEY (session_id) REFERENCES task_sessions(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_logs_session ON execution_logs(session_id);
        CREATE INDEX IF NOT EXISTS idx_logs_phase ON execution_logs(phase_id);",
    )?;
    set_version(conn, 39)?;
    Ok(())
}

/// v39 → v40: 项目文件夹会话（Tab 化管理）
fn migrate_v39_to_v40(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_sessions (
            id              TEXT PRIMARY KEY NOT NULL,
            project_name    TEXT NOT NULL,
            project_path    TEXT NOT NULL UNIQUE,
            status          TEXT NOT NULL DEFAULT 'idle',
            git_branch      TEXT,
            is_open         INTEGER NOT NULL DEFAULT 0,
            last_active_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            created_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        );

        CREATE INDEX IF NOT EXISTS idx_project_sessions_open
            ON project_sessions(is_open);

        CREATE TABLE IF NOT EXISTS project_session_contexts (
            session_id          TEXT PRIMARY KEY NOT NULL,
            project_path        TEXT NOT NULL,
            git_branch          TEXT,
            changed_files_json  TEXT NOT NULL DEFAULT '[]',
            pinned_files_json   TEXT NOT NULL DEFAULT '[]',
            recent_files_json   TEXT NOT NULL DEFAULT '[]',
            current_task        TEXT,
            updated_at          TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            FOREIGN KEY (session_id) REFERENCES project_sessions(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS project_session_messages (
            id          TEXT PRIMARY KEY NOT NULL,
            session_id  TEXT NOT NULL,
            role        TEXT NOT NULL,
            content     TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            FOREIGN KEY (session_id) REFERENCES project_sessions(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_project_session_msgs
            ON project_session_messages(session_id, created_at);",
    )?;
    set_version(conn, 40)?;
    Ok(())
}

/// v40 -> v41: AI 模型能力字段 — 协议类型 + 工具调用/视觉/最大输出 token
fn migrate_v40_to_v41(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v40 -> v41");

    conn.execute_batch(
        r#"
        ALTER TABLE ai_models ADD COLUMN protocol TEXT NOT NULL DEFAULT 'openai_compatible';
        ALTER TABLE ai_models ADD COLUMN supports_tools INTEGER NOT NULL DEFAULT 1;
        ALTER TABLE ai_models ADD COLUMN supports_vision INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE ai_models ADD COLUMN max_output_tokens INTEGER;

        -- 存量 deepseek 模型自动标协议（deepseek 走 OpenAI 兼容，无需改）
        "#,
    )?;

    set_version(conn, 41)?;
    Ok(())
}

/// v41 -> v42: Claude Code Agent Runner — agent 会话 + 事件日志表
fn migrate_v41_to_v42(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v41 -> v42");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS claude_agent_sessions (
            id          TEXT PRIMARY KEY NOT NULL,
            project_path TEXT NOT NULL,
            prompt      TEXT NOT NULL,
            session_name TEXT,
            permission_mode TEXT NOT NULL DEFAULT 'readonly',
            status      TEXT NOT NULL DEFAULT 'pending',
            pid         INTEGER,
            exit_code   INTEGER,
            error_message TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            started_at  TEXT,
            finished_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_cas_project ON claude_agent_sessions(project_path);

        CREATE TABLE IF NOT EXISTS claude_agent_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id  TEXT NOT NULL,
            kind        TEXT NOT NULL,
            content     TEXT NOT NULL,
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            FOREIGN KEY (session_id) REFERENCES claude_agent_sessions(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_cae_session ON claude_agent_events(session_id);
        ",
    )?;

    set_version(conn, 42)?;
    Ok(())
}

/// v42 -> v43: unified plugin product, installation and entitlement foundation.
fn migrate_v42_to_v43(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v42 -> v43 (插件商品、安装和授权基础模型)");

    let plugin_cols = list_columns(conn, "plugins")?;
    if !plugin_cols.iter().any(|c| c == "manifest_format") {
        conn.execute_batch(
            "ALTER TABLE plugins ADD COLUMN manifest_format TEXT NOT NULL DEFAULT 'legacy';",
        )?;
    }
    if !plugin_cols.iter().any(|c| c == "schema_version") {
        conn.execute_batch(
            "ALTER TABLE plugins ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    if !plugin_cols.iter().any(|c| c == "product_type") {
        conn.execute_batch(
            "ALTER TABLE plugins ADD COLUMN product_type TEXT NOT NULL DEFAULT 'local-plugin';",
        )?;
    }
    if !plugin_cols.iter().any(|c| c == "runtime_kind") {
        conn.execute_batch(
            "ALTER TABLE plugins ADD COLUMN runtime_kind TEXT NOT NULL DEFAULT 'legacy-js';",
        )?;
    }
    if !plugin_cols.iter().any(|c| c == "source") {
        conn.execute_batch(
            "ALTER TABLE plugins ADD COLUMN source TEXT NOT NULL DEFAULT 'development';",
        )?;
    }
    if !plugin_cols.iter().any(|c| c == "signature_status") {
        conn.execute_batch(
            "ALTER TABLE plugins ADD COLUMN signature_status TEXT NOT NULL DEFAULT 'unsigned';",
        )?;
    }

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS products (
            id              TEXT PRIMARY KEY,
            developer_id    TEXT NOT NULL,
            name            TEXT NOT NULL,
            description     TEXT,
            product_type    TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'draft',
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );

        CREATE TABLE IF NOT EXISTS product_versions (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id          TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            version             TEXT NOT NULL,
            manifest_json       TEXT NOT NULL,
            runtime_kind        TEXT NOT NULL,
            source              TEXT NOT NULL,
            content_hash        TEXT NOT NULL DEFAULT '',
            signature_status    TEXT NOT NULL DEFAULT 'unsigned',
            min_app_version     TEXT,
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            UNIQUE(product_id, version)
        );
        CREATE INDEX IF NOT EXISTS idx_product_versions_product ON product_versions(product_id);

        CREATE TABLE IF NOT EXISTS product_permissions (
            product_version_id   INTEGER NOT NULL REFERENCES product_versions(id) ON DELETE CASCADE,
            permission           TEXT NOT NULL,
            required             INTEGER NOT NULL DEFAULT 1,
            reason               TEXT,
            PRIMARY KEY(product_version_id, permission)
        );

        CREATE TABLE IF NOT EXISTS plugin_installations (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin_id           TEXT NOT NULL UNIQUE REFERENCES plugins(id) ON DELETE CASCADE,
            product_id          TEXT REFERENCES products(id) ON DELETE SET NULL,
            product_version_id  INTEGER REFERENCES product_versions(id) ON DELETE SET NULL,
            installed_version   TEXT NOT NULL,
            source              TEXT NOT NULL,
            enabled             INTEGER NOT NULL DEFAULT 0,
            install_path        TEXT NOT NULL,
            content_hash        TEXT NOT NULL DEFAULT '',
            installed_at        TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at          TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_plugin_installations_product ON plugin_installations(product_id);
        CREATE INDEX IF NOT EXISTS idx_plugin_installations_enabled ON plugin_installations(enabled);

        CREATE TABLE IF NOT EXISTS entitlements (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id          TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            entitlement_type    TEXT NOT NULL,
            status              TEXT NOT NULL,
            issued_at           TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            expires_at          TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_entitlements_product_status ON entitlements(product_id, status);

        INSERT OR IGNORE INTO products
            (id, developer_id, name, description, product_type, status, created_at, updated_at)
        SELECT
            id,
            COALESCE(NULLIF(author, ''), 'legacy'),
            name,
            description,
            product_type,
            status,
            installed_at,
            updated_at
        FROM plugins;

        INSERT OR IGNORE INTO product_versions
            (product_id, version, manifest_json, runtime_kind, source, content_hash,
             signature_status, min_app_version, created_at)
        SELECT
            id,
            version,
            manifest_json,
            runtime_kind,
            source,
            content_hash,
            signature_status,
            min_app_version,
            installed_at
        FROM plugins;

        INSERT OR IGNORE INTO product_permissions
            (product_version_id, permission, required, reason)
        SELECT
            pv.id,
            pp.permission,
            1,
            NULL
        FROM plugin_permissions pp
        JOIN plugins p ON p.id = pp.plugin_id
        JOIN product_versions pv ON pv.product_id = p.id AND pv.version = p.version;

        INSERT OR IGNORE INTO plugin_installations
            (plugin_id, product_id, product_version_id, installed_version, source, enabled,
             install_path, content_hash, installed_at, updated_at)
        SELECT
            p.id,
            p.id,
            pv.id,
            p.version,
            p.source,
            p.enabled,
            p.path,
            p.content_hash,
            p.installed_at,
            p.updated_at
        FROM plugins p
        LEFT JOIN product_versions pv ON pv.product_id = p.id AND pv.version = p.version;

        INSERT OR IGNORE INTO entitlements
            (product_id, entitlement_type, status, issued_at, expires_at)
        SELECT id, 'free', 'active', installed_at, NULL FROM plugins;
        "#,
    )?;

    set_version(conn, 43)?;
    Ok(())
}

/// v43 -> v44: local mock AI marketplace commerce and managed package assets.
fn migrate_v43_to_v44(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v43 -> v44 (local mock AI marketplace)");

    let product_cols = list_columns(conn, "products")?;
    if !product_cols.iter().any(|c| c == "plugin_id") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN plugin_id TEXT;")?;
        conn.execute(
            "UPDATE products SET plugin_id = id WHERE plugin_id IS NULL",
            [],
        )?;
    }
    if !product_cols.iter().any(|c| c == "developer_name") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN developer_name TEXT NOT NULL DEFAULT 'Unknown Developer';")?;
    }
    if !product_cols.iter().any(|c| c == "icon") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN icon TEXT;")?;
    }
    if !product_cols.iter().any(|c| c == "license_type") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN license_type TEXT NOT NULL DEFAULT 'free';",
        )?;
    }
    if !product_cols.iter().any(|c| c == "byok_required") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN byok_required INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !product_cols.iter().any(|c| c == "mock_mode") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN mock_mode INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !product_cols.iter().any(|c| c == "data_destination") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN data_destination TEXT;")?;
    }
    if !product_cols.iter().any(|c| c == "file_upload_notice") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN file_upload_notice TEXT;")?;
    }
    if !product_cols.iter().any(|c| c == "risk_notes_json") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN risk_notes_json TEXT NOT NULL DEFAULT '[]';",
        )?;
    }

    let version_cols = list_columns(conn, "product_versions")?;
    if !version_cols.iter().any(|c| c == "status") {
        conn.execute_batch(
            "ALTER TABLE product_versions ADD COLUMN status TEXT NOT NULL DEFAULT 'active';",
        )?;
    }
    if !version_cols.iter().any(|c| c == "changelog") {
        conn.execute_batch(
            "ALTER TABLE product_versions ADD COLUMN changelog TEXT NOT NULL DEFAULT '';",
        )?;
    }
    if !version_cols.iter().any(|c| c == "package_path") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN package_path TEXT;")?;
    }
    if !version_cols.iter().any(|c| c == "previous_version") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN previous_version TEXT;")?;
    }

    let installation_cols = list_columns(conn, "plugin_installations")?;
    if !installation_cols.iter().any(|c| c == "status") {
        conn.execute_batch(
            "ALTER TABLE plugin_installations ADD COLUMN status TEXT NOT NULL DEFAULT 'installed';",
        )?;
    }
    if !installation_cols
        .iter()
        .any(|c| c == "previous_install_path")
    {
        conn.execute_batch(
            "ALTER TABLE plugin_installations ADD COLUMN previous_install_path TEXT;",
        )?;
    }

    let entitlement_cols = list_columns(conn, "entitlements")?;
    if !entitlement_cols.iter().any(|c| c == "local_user_id") {
        conn.execute_batch("ALTER TABLE entitlements ADD COLUMN local_user_id TEXT NOT NULL DEFAULT 'local-demo-user';")?;
    }
    conn.execute(
        "UPDATE entitlements SET local_user_id = 'local-demo-user' WHERE local_user_id IS NULL OR local_user_id = ''",
        [],
    )?;
    conn.execute_batch(
        r#"
        DELETE FROM entitlements
        WHERE id NOT IN (
            SELECT MAX(id)
            FROM entitlements
            WHERE status IN ('active', 'expired')
            GROUP BY local_user_id, product_id
            UNION
            SELECT id
            FROM entitlements
            WHERE status NOT IN ('active', 'expired')
        );
        "#,
    )?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS prices (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id      TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            currency        TEXT NOT NULL DEFAULT 'CNY',
            amount          INTEGER NOT NULL DEFAULT 0,
            price_type      TEXT NOT NULL DEFAULT 'free',
            is_mock         INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            UNIQUE(product_id, price_type)
        );
        CREATE INDEX IF NOT EXISTS idx_prices_product ON prices(product_id);

        CREATE TABLE IF NOT EXISTS orders (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            local_user_id   TEXT NOT NULL,
            status          TEXT NOT NULL,
            currency        TEXT NOT NULL DEFAULT 'CNY',
            total_amount    INTEGER NOT NULL DEFAULT 0,
            is_mock         INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            completed_at    TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_orders_local_user ON orders(local_user_id, status);

        CREATE TABLE IF NOT EXISTS order_items (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            order_id            INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
            product_id          TEXT NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
            product_version_id  INTEGER REFERENCES product_versions(id) ON DELETE SET NULL,
            amount              INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_order_items_product ON order_items(product_id);

        CREATE TABLE IF NOT EXISTS product_assets (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            product_version_id  INTEGER NOT NULL REFERENCES product_versions(id) ON DELETE CASCADE,
            asset_type          TEXT NOT NULL,
            local_path          TEXT NOT NULL,
            content_hash        TEXT NOT NULL DEFAULT '',
            size                INTEGER NOT NULL DEFAULT 0,
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            UNIQUE(product_version_id, asset_type)
        );

        CREATE TABLE IF NOT EXISTS marketplace_prompt_templates (
            product_id          TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            template_id         INTEGER NOT NULL REFERENCES prompt_templates(id) ON DELETE CASCADE,
            template_key        TEXT NOT NULL,
            content_hash        TEXT NOT NULL,
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            PRIMARY KEY(product_id, template_id)
        );
        CREATE INDEX IF NOT EXISTS idx_marketplace_prompt_product ON marketplace_prompt_templates(product_id);

        CREATE UNIQUE INDEX IF NOT EXISTS idx_entitlements_user_product
            ON entitlements(local_user_id, product_id)
            WHERE status IN ('active', 'expired');
        CREATE INDEX IF NOT EXISTS idx_product_versions_status
            ON product_versions(product_id, status);
        CREATE INDEX IF NOT EXISTS idx_products_status_type
            ON products(status, product_type);
        "#,
    )?;

    set_version(conn, 44)?;
    Ok(())
}

/// v44 -> v45: developer center, review workflow and mock revenue ledger.
fn migrate_v44_to_v45(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v44 -> v45 (developer review marketplace)");

    let product_cols = list_columns(conn, "products")?;
    if !product_cols.iter().any(|c| c == "category") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN category TEXT NOT NULL DEFAULT 'general';",
        )?;
    }
    if !product_cols.iter().any(|c| c == "tags_json") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';",
        )?;
    }
    if !product_cols.iter().any(|c| c == "privacy_notice") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN privacy_notice TEXT;")?;
    }
    if !product_cols.iter().any(|c| c == "usage_guide") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN usage_guide TEXT;")?;
    }
    if !product_cols.iter().any(|c| c == "third_party_dependencies") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN third_party_dependencies TEXT;")?;
    }
    if !product_cols.iter().any(|c| c == "file_upload_required") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN file_upload_required INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !product_cols.iter().any(|c| c == "support_period") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN support_period TEXT;")?;
    }
    if !product_cols.iter().any(|c| c == "review_status") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN review_status TEXT NOT NULL DEFAULT 'approved';",
        )?;
    }
    if !product_cols.iter().any(|c| c == "runtime_kind") {
        conn.execute_batch(
            "ALTER TABLE products ADD COLUMN runtime_kind TEXT NOT NULL DEFAULT 'declarative-ui';",
        )?;
    }
    if !product_cols.iter().any(|c| c == "distribution_channel") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN distribution_channel TEXT NOT NULL DEFAULT 'local-demo';")?;
    }
    if !product_cols.iter().any(|c| c == "full_description") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN full_description TEXT;")?;
        conn.execute(
            "UPDATE products SET full_description = description WHERE full_description IS NULL",
            [],
        )?;
    }

    let version_cols = list_columns(conn, "product_versions")?;
    if !version_cols.iter().any(|c| c == "review_status") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN review_status TEXT NOT NULL DEFAULT 'approved';")?;
    }
    if !version_cols.iter().any(|c| c == "distribution_channel") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN distribution_channel TEXT NOT NULL DEFAULT 'local-demo';")?;
    }
    if !version_cols.iter().any(|c| c == "scan_report_json") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN scan_report_json TEXT;")?;
    }
    if !version_cols.iter().any(|c| c == "scan_status") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN scan_status TEXT NOT NULL DEFAULT 'not_scanned';")?;
    }

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id              TEXT PRIMARY KEY,
            display_name    TEXT NOT NULL,
            role            TEXT NOT NULL,
            is_mock         INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );

        CREATE TABLE IF NOT EXISTS developer_profiles (
            user_id             TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
            developer_name      TEXT NOT NULL,
            description         TEXT,
            verification_status TEXT NOT NULL DEFAULT 'local_demo',
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at          TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );

        CREATE TABLE IF NOT EXISTS product_submissions (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id          TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            product_version_id  INTEGER REFERENCES product_versions(id) ON DELETE CASCADE,
            submitted_by        TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
            status              TEXT NOT NULL,
            submitted_at        TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            reviewed_by         TEXT REFERENCES users(id) ON DELETE SET NULL,
            reviewed_at         TEXT,
            review_message      TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_product_submissions_status
            ON product_submissions(status, submitted_at DESC);
        CREATE INDEX IF NOT EXISTS idx_product_submissions_product
            ON product_submissions(product_id, product_version_id);

        CREATE TABLE IF NOT EXISTS product_review_events (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            submission_id   INTEGER REFERENCES product_submissions(id) ON DELETE CASCADE,
            actor_id        TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
            action          TEXT NOT NULL,
            from_status     TEXT,
            to_status       TEXT,
            message         TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_product_review_events_submission
            ON product_review_events(submission_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS developer_earnings (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            developer_id        TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
            order_item_id       INTEGER NOT NULL UNIQUE REFERENCES order_items(id) ON DELETE CASCADE,
            gross_amount        INTEGER NOT NULL DEFAULT 0,
            platform_fee        INTEGER NOT NULL DEFAULT 0,
            developer_amount    INTEGER NOT NULL DEFAULT 0,
            currency            TEXT NOT NULL DEFAULT 'CNY',
            is_mock             INTEGER NOT NULL DEFAULT 1,
            status              TEXT NOT NULL DEFAULT 'pending',
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_developer_earnings_developer
            ON developer_earnings(developer_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS marketplace_audit_logs (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            actor_id        TEXT NOT NULL,
            actor_role      TEXT NOT NULL,
            action          TEXT NOT NULL,
            target_type     TEXT NOT NULL,
            target_id       TEXT NOT NULL,
            details_json    TEXT NOT NULL DEFAULT '{}',
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_marketplace_audit_target
            ON marketplace_audit_logs(target_type, target_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_marketplace_audit_actor
            ON marketplace_audit_logs(actor_id, created_at DESC);

        INSERT OR IGNORE INTO users (id, display_name, role, is_mock)
        VALUES
            ('local-demo-user', '本地演示用户', 'customer', 1),
            ('local-demo-developer', '本地演示开发者', 'developer', 1),
            ('local-demo-admin', '本地演示管理员', 'admin', 1),
            ('official-demo-developer', 'firstwork 官方演示', 'developer', 1);

        INSERT OR IGNORE INTO developer_profiles
            (user_id, developer_name, description, verification_status)
        VALUES
            ('local-demo-developer', '本地演示开发者', '仅用于 firstwork 本地市场 MVP，不代表真实认证。', 'local_demo'),
            ('official-demo-developer', 'firstwork 官方演示', '内置演示商品开发者。', 'local_demo');
        "#,
    )?;

    set_version(conn, 45)?;
    Ok(())
}

fn migrate_v45_to_v46(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v45 -> v46");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS credentials (
            id                  TEXT PRIMARY KEY,
            provider            TEXT NOT NULL,
            credential_type     TEXT NOT NULL,
            label               TEXT NOT NULL,
            owner_scope         TEXT NOT NULL DEFAULT 'local-user',
            secret_reference    TEXT NOT NULL,
            configured          INTEGER NOT NULL DEFAULT 0,
            masked_hint         TEXT,
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at          TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            last_used_at        TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_credentials_provider
            ON credentials(provider, credential_type);

        CREATE TABLE IF NOT EXISTS external_agents (
            id                      TEXT PRIMARY KEY,
            installation_id          INTEGER REFERENCES plugin_installations(id) ON DELETE SET NULL,
            product_id              TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            product_version_id       INTEGER REFERENCES product_versions(id) ON DELETE SET NULL,
            provider                TEXT NOT NULL,
            name                    TEXT NOT NULL,
            endpoint                TEXT NOT NULL,
            agent_id                TEXT,
            bot_id                  TEXT,
            flow_id                 TEXT,
            authentication_type     TEXT NOT NULL DEFAULT 'none',
            credential_id           TEXT REFERENCES credentials(id) ON DELETE RESTRICT,
            streaming_type          TEXT NOT NULL DEFAULT 'none',
            request_mapping_json    TEXT NOT NULL DEFAULT '{}',
            response_mapping_json   TEXT NOT NULL DEFAULT '{}',
            session_mapping_json    TEXT NOT NULL DEFAULT '{}',
            error_mapping_json      TEXT NOT NULL DEFAULT '{}',
            mock_mode               INTEGER NOT NULL DEFAULT 1,
            enabled                 INTEGER NOT NULL DEFAULT 1,
            unavailable_reason      TEXT,
            last_tested_at          TEXT,
            last_test_status        TEXT,
            created_at              TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at              TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_external_agents_product
            ON external_agents(product_id, enabled);
        CREATE INDEX IF NOT EXISTS idx_external_agents_credential
            ON external_agents(credential_id);

        CREATE TABLE IF NOT EXISTS agent_sessions (
            id                  TEXT PRIMARY KEY,
            external_agent_id   TEXT NOT NULL REFERENCES external_agents(id) ON DELETE CASCADE,
            remote_session_id   TEXT,
            title               TEXT NOT NULL,
            status              TEXT NOT NULL DEFAULT 'active',
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at          TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_agent
            ON agent_sessions(external_agent_id, updated_at DESC);

        CREATE TABLE IF NOT EXISTS agent_messages (
            id              TEXT PRIMARY KEY,
            session_id      TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
            role            TEXT NOT NULL,
            content         TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'completed',
            request_id      TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_agent_messages_session
            ON agent_messages(session_id, created_at);

        CREATE TABLE IF NOT EXISTS usage_events (
            id                      INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id              TEXT,
            external_agent_id       TEXT,
            session_id              TEXT,
            request_id              TEXT NOT NULL,
            started_at              TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            completed_at            TEXT,
            duration_ms             INTEGER,
            status                  TEXT NOT NULL,
            provider_error_code     TEXT,
            estimated_input_usage   INTEGER,
            estimated_output_usage  INTEGER,
            metadata_json           TEXT NOT NULL DEFAULT '{}',
            FOREIGN KEY(product_id) REFERENCES products(id) ON DELETE SET NULL,
            FOREIGN KEY(external_agent_id) REFERENCES external_agents(id) ON DELETE SET NULL,
            FOREIGN KEY(session_id) REFERENCES agent_sessions(id) ON DELETE SET NULL
        );
        CREATE INDEX IF NOT EXISTS idx_usage_events_agent
            ON usage_events(external_agent_id, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_usage_events_session
            ON usage_events(session_id, started_at DESC);
        ",
    )?;

    set_version(conn, 46)?;
    Ok(())
}

fn migrate_v46_to_v47(conn: &Connection) -> Result<(), AppError> {
    log::info!("数据库迁移: v46 -> v47 (xingchen workflow protocol)");

    let external_agent_cols = list_columns(conn, "external_agents")?;
    if !external_agent_cols.iter().any(|c| c == "protocol_type") {
        conn.execute_batch(
            "ALTER TABLE external_agents ADD COLUMN protocol_type TEXT NOT NULL DEFAULT 'configurable';",
        )?;
    }
    if !external_agent_cols.iter().any(|c| c == "local_uid") {
        conn.execute_batch("ALTER TABLE external_agents ADD COLUMN local_uid TEXT;")?;
    }

    conn.execute_batch(
        r#"
        INSERT OR IGNORE INTO product_permissions
            (product_version_id, permission, required, reason)
        SELECT pv.id, 'agents.invoke', 1, 'Allow installed Xingchen products to invoke configured agents'
        FROM product_versions pv
        JOIN products p ON p.id = pv.product_id
        WHERE p.product_type IN ('xingchen-agent', 'xingchen-workflow');

        INSERT OR IGNORE INTO product_permissions
            (product_version_id, permission, required, reason)
        SELECT pv.id, 'network.xingchen', 1, 'Allow outbound calls to approved Xingchen endpoints'
        FROM product_versions pv
        JOIN products p ON p.id = pv.product_id
        WHERE p.product_type IN ('xingchen-agent', 'xingchen-workflow');

        INSERT OR IGNORE INTO product_permissions
            (product_version_id, permission, required, reason)
        SELECT pv.id, 'credentials.use', 1, 'Allow backend-only use of selected BYOK credential'
        FROM product_versions pv
        JOIN products p ON p.id = pv.product_id
        WHERE p.product_type IN ('xingchen-agent', 'xingchen-workflow');
        "#,
    )?;

    set_version(conn, 47)?;
    Ok(())
}

fn migrate_v47_to_v48(conn: &Connection) -> Result<(), AppError> {
    log::info!("database migration: v47 -> v48 (secure legacy AI/ASR credentials)");

    let ai_model_cols = list_columns(conn, "ai_models")?;
    if !ai_model_cols.iter().any(|c| c == "credential_id") {
        conn.execute_batch(
            "ALTER TABLE ai_models ADD COLUMN credential_id TEXT REFERENCES credentials(id) ON DELETE SET NULL;",
        )?;
    }
    if !ai_model_cols
        .iter()
        .any(|c| c == "credential_migration_status")
    {
        conn.execute_batch(
            "ALTER TABLE ai_models ADD COLUMN credential_migration_status TEXT NOT NULL DEFAULT 'pending';",
        )?;
    }

    conn.execute_batch(
        "
        CREATE INDEX IF NOT EXISTS idx_ai_models_credential
            ON ai_models(credential_id);
        INSERT OR IGNORE INTO app_config (key, value)
            VALUES ('asr.credential_migration_status', 'pending');
        ",
    )?;

    set_version(conn, 48)?;
    Ok(())
}

/// v48 -> v49: Planning with Files 会话级状态。
fn migrate_v48_to_v49(conn: &Connection) -> Result<(), AppError> {
    log::info!("database migration: v48 -> v49 (planning with files)");
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS planning_sessions (
            session_key         TEXT PRIMARY KEY,
            session_kind        TEXT NOT NULL,
            session_id          TEXT NOT NULL,
            enabled             INTEGER NOT NULL DEFAULT 0,
            auto_apply          INTEGER NOT NULL DEFAULT 0,
            pending_update_json TEXT,
            last_updated_at     TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            created_at          TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_planning_sessions_kind_id
            ON planning_sessions(session_kind, session_id);
        ",
    )?;
    set_version(conn, 49)?;
    Ok(())
}

/// v49 -> v50: personal account commerce ownership, ledgers and reviews.
fn migrate_v49_to_v50(conn: &Connection) -> Result<(), AppError> {
    log::info!("database migration: v49 -> v50 (personal account commerce)");

    let user_cols = list_columns(conn, "users")?;
    if !user_cols.iter().any(|c| c == "nickname") {
        conn.execute_batch("ALTER TABLE users ADD COLUMN nickname TEXT;")?;
        conn.execute(
            "UPDATE users SET nickname = display_name WHERE nickname IS NULL",
            [],
        )?;
    }
    if !user_cols.iter().any(|c| c == "avatar") {
        conn.execute_batch("ALTER TABLE users ADD COLUMN avatar TEXT;")?;
    }
    if !user_cols.iter().any(|c| c == "bio") {
        conn.execute_batch("ALTER TABLE users ADD COLUMN bio TEXT;")?;
    }
    if !user_cols.iter().any(|c| c == "account_status") {
        conn.execute_batch(
            "ALTER TABLE users ADD COLUMN account_status TEXT NOT NULL DEFAULT 'active';",
        )?;
    }
    if !user_cols.iter().any(|c| c == "developer_status") {
        conn.execute_batch(
            "ALTER TABLE users ADD COLUMN developer_status TEXT NOT NULL DEFAULT 'none';",
        )?;
        conn.execute(
            "UPDATE users SET developer_status = CASE WHEN role IN ('developer','admin') THEN 'approved' ELSE 'none' END",
            [],
        )?;
    }

    let product_cols = list_columns(conn, "products")?;
    if !product_cols.iter().any(|c| c == "seller_user_id") {
        conn.execute_batch("ALTER TABLE products ADD COLUMN seller_user_id TEXT;")?;
        conn.execute(
            "UPDATE products SET seller_user_id = COALESCE(NULLIF(developer_id, ''), 'official-demo-developer') WHERE seller_user_id IS NULL",
            [],
        )?;
    }

    let order_cols = list_columns(conn, "orders")?;
    if !order_cols.iter().any(|c| c == "buyer_user_id") {
        conn.execute_batch("ALTER TABLE orders ADD COLUMN buyer_user_id TEXT;")?;
        conn.execute(
            "UPDATE orders SET buyer_user_id = COALESCE(NULLIF(local_user_id, ''), 'local-demo-buyer') WHERE buyer_user_id IS NULL",
            [],
        )?;
    }
    if !order_cols.iter().any(|c| c == "seller_user_id") {
        conn.execute_batch("ALTER TABLE orders ADD COLUMN seller_user_id TEXT;")?;
    }
    if !order_cols.iter().any(|c| c == "payment_status") {
        conn.execute_batch(
            "ALTER TABLE orders ADD COLUMN payment_status TEXT NOT NULL DEFAULT 'paid';",
        )?;
    }
    if !order_cols.iter().any(|c| c == "settlement_status") {
        conn.execute_batch(
            "ALTER TABLE orders ADD COLUMN settlement_status TEXT NOT NULL DEFAULT 'settled';",
        )?;
    }
    if !order_cols.iter().any(|c| c == "refund_status") {
        conn.execute_batch(
            "ALTER TABLE orders ADD COLUMN refund_status TEXT NOT NULL DEFAULT 'none';",
        )?;
    }
    if !order_cols.iter().any(|c| c == "gross_amount") {
        conn.execute_batch(
            "ALTER TABLE orders ADD COLUMN gross_amount INTEGER NOT NULL DEFAULT 0;",
        )?;
        conn.execute(
            "UPDATE orders SET gross_amount = total_amount WHERE gross_amount = 0",
            [],
        )?;
    }
    if !order_cols.iter().any(|c| c == "platform_fee") {
        conn.execute_batch(
            "ALTER TABLE orders ADD COLUMN platform_fee INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !order_cols.iter().any(|c| c == "seller_income") {
        conn.execute_batch(
            "ALTER TABLE orders ADD COLUMN seller_income INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    let item_cols = list_columns(conn, "order_items")?;
    if !item_cols.iter().any(|c| c == "seller_user_id") {
        conn.execute_batch("ALTER TABLE order_items ADD COLUMN seller_user_id TEXT;")?;
    }
    if !item_cols.iter().any(|c| c == "currency") {
        conn.execute_batch(
            "ALTER TABLE order_items ADD COLUMN currency TEXT NOT NULL DEFAULT 'CNY';",
        )?;
    }
    if !item_cols.iter().any(|c| c == "gross_amount") {
        conn.execute_batch(
            "ALTER TABLE order_items ADD COLUMN gross_amount INTEGER NOT NULL DEFAULT 0;",
        )?;
        conn.execute(
            "UPDATE order_items SET gross_amount = amount WHERE gross_amount = 0",
            [],
        )?;
    }
    if !item_cols.iter().any(|c| c == "platform_fee") {
        conn.execute_batch(
            "ALTER TABLE order_items ADD COLUMN platform_fee INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !item_cols.iter().any(|c| c == "seller_income") {
        conn.execute_batch(
            "ALTER TABLE order_items ADD COLUMN seller_income INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !item_cols.iter().any(|c| c == "price_snapshot_json") {
        conn.execute_batch(
            "ALTER TABLE order_items ADD COLUMN price_snapshot_json TEXT NOT NULL DEFAULT '{}';",
        )?;
    }
    if !item_cols.iter().any(|c| c == "version_snapshot") {
        conn.execute_batch("ALTER TABLE order_items ADD COLUMN version_snapshot TEXT;")?;
    }

    let entitlement_cols = list_columns(conn, "entitlements")?;
    if !entitlement_cols.iter().any(|c| c == "owner_user_id") {
        conn.execute_batch("ALTER TABLE entitlements ADD COLUMN owner_user_id TEXT;")?;
        conn.execute(
            "UPDATE entitlements SET owner_user_id = COALESCE(NULLIF(local_user_id, ''), 'local-demo-buyer') WHERE owner_user_id IS NULL",
            [],
        )?;
    }
    if !entitlement_cols.iter().any(|c| c == "order_id") {
        conn.execute_batch("ALTER TABLE entitlements ADD COLUMN order_id INTEGER REFERENCES orders(id) ON DELETE SET NULL;")?;
    }
    if !entitlement_cols.iter().any(|c| c == "order_item_id") {
        conn.execute_batch("ALTER TABLE entitlements ADD COLUMN order_item_id INTEGER REFERENCES order_items(id) ON DELETE SET NULL;")?;
    }
    if !entitlement_cols.iter().any(|c| c == "revoked_at") {
        conn.execute_batch("ALTER TABLE entitlements ADD COLUMN revoked_at TEXT;")?;
    }
    if !entitlement_cols.iter().any(|c| c == "revoked_reason") {
        conn.execute_batch("ALTER TABLE entitlements ADD COLUMN revoked_reason TEXT;")?;
    }

    conn.execute_batch(
        r#"
        UPDATE products SET seller_user_id = 'official-demo-developer'
        WHERE seller_user_id IS NULL OR seller_user_id = '' OR seller_user_id = 'legacy';

        UPDATE order_items
        SET seller_user_id = (
            SELECT COALESCE(p.seller_user_id, p.developer_id, 'official-demo-developer')
            FROM products p WHERE p.id = order_items.product_id
        )
        WHERE seller_user_id IS NULL OR seller_user_id = '';

        UPDATE order_items
        SET version_snapshot = (
            SELECT version FROM product_versions pv WHERE pv.id = order_items.product_version_id
        )
        WHERE version_snapshot IS NULL;

        UPDATE order_items
        SET price_snapshot_json = json_object(
            'currency', currency,
            'amount', gross_amount,
            'isMock', 1
        )
        WHERE price_snapshot_json IS NULL OR price_snapshot_json = '{}';

        UPDATE orders
        SET seller_user_id = (
            SELECT seller_user_id FROM order_items oi WHERE oi.order_id = orders.id LIMIT 1
        )
        WHERE seller_user_id IS NULL OR seller_user_id = '';
        "#,
    )?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS commerce_ledger_entries (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            entry_type      TEXT NOT NULL,
            order_id        INTEGER REFERENCES orders(id) ON DELETE SET NULL,
            order_item_id   INTEGER REFERENCES order_items(id) ON DELETE SET NULL,
            buyer_user_id   TEXT,
            seller_user_id  TEXT,
            product_id      TEXT,
            amount          INTEGER NOT NULL,
            currency        TEXT NOT NULL DEFAULT 'CNY',
            is_mock         INTEGER NOT NULL DEFAULT 1,
            memo            TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_commerce_ledger_order
            ON commerce_ledger_entries(order_id, created_at);
        CREATE INDEX IF NOT EXISTS idx_commerce_ledger_seller
            ON commerce_ledger_entries(seller_user_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_commerce_ledger_buyer
            ON commerce_ledger_entries(buyer_user_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS product_reviews (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            order_id        INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
            product_id      TEXT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
            buyer_user_id   TEXT NOT NULL,
            seller_user_id  TEXT NOT NULL,
            rating          INTEGER NOT NULL CHECK(rating BETWEEN 1 AND 5),
            content         TEXT NOT NULL,
            status          TEXT NOT NULL DEFAULT 'visible',
            hidden_by       TEXT,
            hidden_reason   TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            UNIQUE(order_id, product_id)
        );
        CREATE INDEX IF NOT EXISTS idx_product_reviews_product
            ON product_reviews(product_id, status, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_product_reviews_buyer
            ON product_reviews(buyer_user_id, created_at DESC);

        CREATE INDEX IF NOT EXISTS idx_products_seller
            ON products(seller_user_id, status);
        CREATE INDEX IF NOT EXISTS idx_orders_buyer_status
            ON orders(buyer_user_id, payment_status, refund_status);
        CREATE INDEX IF NOT EXISTS idx_orders_seller_status
            ON orders(seller_user_id, payment_status, refund_status);
        CREATE INDEX IF NOT EXISTS idx_entitlements_owner_product
            ON entitlements(owner_user_id, product_id, status);
        "#,
    )?;

    conn.execute_batch(
        r#"
        INSERT OR IGNORE INTO users
            (id, display_name, role, is_mock, nickname, avatar, bio, account_status, developer_status)
        VALUES
            ('local-demo-buyer', '普通买家', 'customer', 1, '普通买家', NULL, '本地演示普通买家账号。', 'active', 'none'),
            ('local-demo-creator', '个人创作者', 'developer', 1, '个人创作者', NULL, '本地演示个人创作者，可购买也可销售。', 'active', 'approved'),
            ('local-demo-admin', '管理员', 'admin', 1, '管理员', NULL, '本地演示管理员账号。', 'active', 'approved'),
            ('official-demo-developer', 'firstwork 官方演示', 'developer', 1, 'firstwork 官方演示', NULL, '内置官方演示商品创作者。', 'active', 'approved');

        INSERT OR IGNORE INTO developer_profiles
            (user_id, developer_name, description, verification_status)
        VALUES
            ('local-demo-creator', '个人创作者', '本地演示个人商店。', 'local_demo'),
            ('official-demo-developer', 'firstwork 官方演示', '内置演示商品开发者。', 'local_demo');
        "#,
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO users
            (id, display_name, role, is_mock, nickname, avatar, bio, account_status, developer_status)
         VALUES
            ('firstwork-official', 'firstwork 官方', 'developer', 1, 'firstwork 官方', NULL,
             '内置官方演示商品创作者。', 'active', 'approved')",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO developer_profiles
            (user_id, developer_name, description, verification_status)
         VALUES
            ('firstwork-official', 'firstwork 官方', '内置演示商品开发者。', 'local_demo')",
        [],
    )?;

    set_version(conn, 50)?;
    Ok(())
}

/// v50 -> v51: Manifest v3、版本化安装、分层启用和受控执行日志。
fn migrate_v50_to_v51(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS plugin_versions (
            plugin_id        TEXT NOT NULL,
            version          TEXT NOT NULL,
            install_path     TEXT NOT NULL,
            manifest_json    TEXT NOT NULL,
            content_hash     TEXT NOT NULL,
            permissions_json TEXT NOT NULL DEFAULT '[]',
            signature_status TEXT NOT NULL DEFAULT 'unsigned',
            is_current       INTEGER NOT NULL DEFAULT 0 CHECK(is_current IN (0, 1)),
            installed_at     TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            PRIMARY KEY(plugin_id, version)
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_plugin_versions_one_current
            ON plugin_versions(plugin_id) WHERE is_current = 1;
        CREATE INDEX IF NOT EXISTS idx_plugin_versions_installed
            ON plugin_versions(plugin_id, installed_at DESC);

        CREATE TABLE IF NOT EXISTS plugin_activation_settings (
            plugin_id    TEXT NOT NULL,
            scope_type   TEXT NOT NULL CHECK(scope_type IN ('global','scene','feature')),
            scope_key    TEXT NOT NULL DEFAULT '',
            enabled      INTEGER NOT NULL CHECK(enabled IN (0, 1)),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            PRIMARY KEY(plugin_id, scope_type, scope_key)
        );
        CREATE INDEX IF NOT EXISTS idx_plugin_activation_scope
            ON plugin_activation_settings(scope_type, scope_key, enabled);

        CREATE TABLE IF NOT EXISTS plugin_execution_logs (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin_id        TEXT NOT NULL,
            contribution_id  TEXT,
            hook             TEXT,
            scene            TEXT NOT NULL,
            feature          TEXT NOT NULL,
            request_id       TEXT NOT NULL,
            status           TEXT NOT NULL,
            duration_ms      INTEGER,
            error_message    TEXT,
            created_at       TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_plugin_execution_request
            ON plugin_execution_logs(request_id, id);
        CREATE INDEX IF NOT EXISTS idx_plugin_execution_plugin
            ON plugin_execution_logs(plugin_id, created_at DESC);

        CREATE TABLE IF NOT EXISTS plugin_install_history (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin_id        TEXT NOT NULL,
            operation        TEXT NOT NULL,
            from_version     TEXT,
            to_version       TEXT,
            content_hash     TEXT,
            status           TEXT NOT NULL,
            error_message    TEXT,
            created_at       TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE INDEX IF NOT EXISTS idx_plugin_install_history_plugin
            ON plugin_install_history(plugin_id, created_at DESC);
        "#,
    )?;
    set_version(conn, 51)?;
    Ok(())
}

/// v51 -> v52: lock reviewed marketplace packages to a concrete format, manifest and hash.
fn migrate_v51_to_v52(conn: &Connection) -> Result<(), AppError> {
    log::info!("database migration: v51 -> v52 (manifest v3 marketplace bridge)");
    let columns = list_columns(conn, "product_versions")?;
    if !columns.iter().any(|column| column == "package_format") {
        conn.execute_batch(
            "ALTER TABLE product_versions ADD COLUMN package_format TEXT NOT NULL DEFAULT 'v2-zip';",
        )?;
    }
    if !columns
        .iter()
        .any(|column| column == "manifest_schema_version")
    {
        conn.execute_batch(
            "ALTER TABLE product_versions ADD COLUMN manifest_schema_version INTEGER NOT NULL DEFAULT 2;",
        )?;
    }
    if !columns.iter().any(|column| column == "plugin_id") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN plugin_id TEXT;")?;
        conn.execute(
            "UPDATE product_versions
             SET plugin_id = (SELECT COALESCE(products.plugin_id, products.id)
                              FROM products WHERE products.id = product_versions.product_id)
             WHERE plugin_id IS NULL",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "classification") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN classification TEXT;")?;
    }
    if !columns
        .iter()
        .any(|column| column == "approved_content_hash")
    {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN approved_content_hash TEXT;")?;
    }
    if !columns.iter().any(|column| column == "package_locked") {
        conn.execute_batch(
            "ALTER TABLE product_versions ADD COLUMN package_locked INTEGER NOT NULL DEFAULT 0 CHECK(package_locked IN (0, 1));",
        )?;
    }
    if !columns.iter().any(|column| column == "approved_at") {
        conn.execute_batch("ALTER TABLE product_versions ADD COLUMN approved_at TEXT;")?;
    }
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_product_versions_package_identity
            ON product_versions(plugin_id, version, package_format);
        CREATE INDEX IF NOT EXISTS idx_product_versions_review_installable
            ON product_versions(product_id, review_status, package_locked);
        "#,
    )?;
    set_version(conn, 52)?;
    Ok(())
}

/// v52 -> v53: register learning assistant document sources without changing user content.
fn migrate_v52_to_v53(conn: &Connection) -> Result<(), AppError> {
    log::info!("database migration: v52 -> v53 (learning assistant document sources)");
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS document_sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            display_name TEXT NOT NULL,
            original_file_name TEXT NOT NULL,
            stored_relative_path TEXT NOT NULL UNIQUE,
            file_extension TEXT NOT NULL,
            mime_type TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL,
            source_module TEXT NOT NULL,
            is_builtin INTEGER NOT NULL DEFAULT 0 CHECK(is_builtin IN (0, 1)),
            is_enabled INTEGER NOT NULL DEFAULT 1 CHECK(is_enabled IN (0, 1)),
            file_size INTEGER NOT NULL DEFAULT 0,
            checksum TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_document_sources_identity
            ON document_sources(source_module, category, original_file_name, checksum);
        CREATE INDEX IF NOT EXISTS idx_document_sources_category
            ON document_sources(category, source_module, is_enabled);
        "#,
    )?;
    set_version(conn, 53)?;
    Ok(())
}

/// v53 -> v54: cache parsed document text for the unified learning document tree.
fn migrate_v53_to_v54(conn: &Connection) -> Result<(), AppError> {
    log::info!("database migration: v53 -> v54 (learning document parse cache)");
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS document_parse_cache (
            document_id        INTEGER PRIMARY KEY,
            parse_status       TEXT NOT NULL DEFAULT 'parsing',
            parsed_text        TEXT NOT NULL DEFAULT '',
            parse_message      TEXT NOT NULL DEFAULT '',
            parsed_at          TEXT,
            source_modified_at INTEGER NOT NULL DEFAULT 0,
            content_hash       TEXT NOT NULL DEFAULT '',
            parser_version     TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_document_parse_cache_status
            ON document_parse_cache(parse_status);
        "#,
    )?;
    set_version(conn, 54)?;
    Ok(())
}

/// v54 -> v55: store AI course graph analyses and reviewed relation suggestions.
fn migrate_v54_to_v55(conn: &Connection) -> Result<(), AppError> {
    log::info!("database migration: v54 -> v55 (course graph AI suggestions)");
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS course_graph_ai_analyses (
            id                   INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id              TEXT NOT NULL,
            node_name            TEXT NOT NULL,
            source_kind          TEXT NOT NULL,
            source_revision      TEXT NOT NULL,
            definition           TEXT NOT NULL,
            summary              TEXT NOT NULL,
            aliases_json         TEXT NOT NULL DEFAULT '[]',
            prerequisites_json   TEXT NOT NULL DEFAULT '[]',
            applications_json    TEXT NOT NULL DEFAULT '[]',
            misconceptions_json  TEXT NOT NULL DEFAULT '[]',
            model_id             INTEGER NOT NULL,
            raw_response         TEXT NOT NULL,
            created_at           TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at           TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            UNIQUE(node_id, source_kind)
        );

        CREATE TABLE IF NOT EXISTS course_graph_ai_relations (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            source_node_id    TEXT NOT NULL,
            source_node_name  TEXT NOT NULL,
            target_node_id    TEXT NOT NULL,
            target_node_name  TEXT NOT NULL,
            relation_type     TEXT NOT NULL,
            reason            TEXT NOT NULL,
            confidence        REAL NOT NULL CHECK(confidence >= 0 AND confidence <= 1),
            status            TEXT NOT NULL DEFAULT 'pending'
                              CHECK(status IN ('pending', 'accepted', 'rejected')),
            source_kind       TEXT NOT NULL,
            source_revision   TEXT NOT NULL,
            model_id          INTEGER NOT NULL,
            created_at        TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at        TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            UNIQUE(source_node_id, target_node_id, relation_type, source_kind)
        );
        CREATE INDEX IF NOT EXISTS idx_course_graph_ai_relations_source_status
            ON course_graph_ai_relations(source_node_id, status, confidence DESC);
        CREATE INDEX IF NOT EXISTS idx_course_graph_ai_relations_target_status
            ON course_graph_ai_relations(target_node_id, status);
        "#,
    )?;
    set_version(conn, 55)?;
    Ok(())
}

#[cfg(test)]
mod week4_plugin_integration_tests {
    use super::*;
    use crate::database::Database;
    use crate::models::MarketplaceProductQuery;
    use crate::services::marketplace::MarketplaceService;
    use crate::services::plugins::PluginService;
    use rusqlite::params;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use uuid::Uuid;

    type Migration = fn(&Connection) -> Result<(), AppError>;

    fn migrate_fixture_to_v42(conn: &Connection) -> Result<(), AppError> {
        let migrations: [Migration; 42] = [
            migrate_v0_to_v1,
            migrate_v1_to_v2,
            migrate_v2_to_v3,
            migrate_v3_to_v4,
            migrate_v4_to_v5,
            migrate_v5_to_v6,
            migrate_v6_to_v7,
            migrate_v7_to_v8,
            migrate_v8_to_v9,
            migrate_v9_to_v10,
            migrate_v10_to_v11,
            migrate_v11_to_v12,
            migrate_v12_to_v13,
            migrate_v13_to_v14,
            migrate_v14_to_v15,
            migrate_v15_to_v16,
            migrate_v16_to_v17,
            migrate_v17_to_v18,
            migrate_v18_to_v19,
            migrate_v19_to_v20,
            migrate_v20_to_v21,
            migrate_v21_to_v22,
            migrate_v22_to_v23,
            migrate_v23_to_v24,
            migrate_v24_to_v25,
            migrate_v25_to_v26,
            migrate_v26_to_v27,
            migrate_v27_to_v28,
            migrate_v28_to_v29,
            migrate_v29_to_v30,
            migrate_v30_to_v31,
            migrate_v31_to_v32,
            migrate_v32_to_v33,
            migrate_v33_to_v34,
            migrate_v34_to_v35,
            migrate_v35_to_v36,
            migrate_v36_to_v37,
            migrate_v37_to_v38,
            migrate_v38_to_v39,
            migrate_v39_to_v40,
            migrate_v40_to_v41,
            migrate_v41_to_v42,
        ];
        for migration in migrations {
            migration(conn)?;
        }
        assert_eq!(get_version(conn)?, 42);
        Ok(())
    }

    fn seed_v42_representative_data(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, ?2)",
            params!["account.last_platform_user_id", "platform-user-v42"],
        )?;
        conn.execute(
            "INSERT INTO folders (id, name, parent_id, sort_order) VALUES (?1, ?2, NULL, ?3)",
            params![4201_i64, "账号业务资料", 7_i64],
        )?;
        conn.execute(
            "INSERT INTO notes (id, title, content, folder_id) VALUES (?1, ?2, ?3, ?4)",
            params![
                4202_i64,
                "v42 保留笔记",
                "不可被市场迁移覆盖的正文",
                4201_i64
            ],
        )?;
        conn.execute(
            "INSERT INTO tasks (id, title, description, priority, important, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                4203_i64,
                "v42 账号业务待办",
                "用于验证本地业务数据保留",
                2_i64,
                1_i64,
                0_i64
            ],
        )?;
        conn.execute(
            "INSERT INTO plugins
                (id, name, version, description, author, path, main, manifest_json,
                 enabled, status, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                "com.pomegranate.test.v42",
                "v42 旧插件",
                "0.9.0",
                "升级数据保留夹具",
                "fixture-author",
                "C:/isolated-test/plugins/com.pomegranate.test.v42",
                "main.js",
                r#"{"id":"com.pomegranate.test.v42","name":"v42 旧插件","version":"0.9.0","main":"main.js","permissions":["notes.read"]}"#,
                1_i64,
                "installed",
                "fixture-content-hash"
            ],
        )?;
        conn.execute(
            "INSERT INTO plugin_permissions (plugin_id, permission, granted)
             VALUES (?1, ?2, ?3)",
            params!["com.pomegranate.test.v42", "notes.read", 1_i64],
        )?;
        conn.execute(
            "INSERT INTO plugin_settings (plugin_id, key, value)
             VALUES (?1, ?2, ?3)",
            params![
                "com.pomegranate.test.v42",
                "plugin:com.pomegranate.test.v42:mode",
                r#""preserve-me""#
            ],
        )?;
        Ok(())
    }

    fn assert_v42_data_preserved(conn: &Connection) -> Result<(), AppError> {
        let account_marker: String = conn.query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            ["account.last_platform_user_id"],
            |row| row.get(0),
        )?;
        assert_eq!(account_marker, "platform-user-v42");

        let note: (String, String, i64) = conn.query_row(
            "SELECT title, content, folder_id FROM notes WHERE id = ?1",
            [4202_i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(
            note,
            (
                "v42 保留笔记".into(),
                "不可被市场迁移覆盖的正文".into(),
                4201_i64
            )
        );
        let folder_name: String =
            conn.query_row("SELECT name FROM folders WHERE id = ?1", [note.2], |row| {
                row.get(0)
            })?;
        assert_eq!(folder_name, "账号业务资料");

        let task: (String, i64, i64) = conn.query_row(
            "SELECT title, important, status FROM tasks WHERE id = ?1",
            [4203_i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(task, ("v42 账号业务待办".into(), 1_i64, 0_i64));

        let plugin: (String, String, i64, String, String) = conn.query_row(
            "SELECT name, version, enabled, path, content_hash FROM plugins WHERE id = ?1",
            ["com.pomegranate.test.v42"],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        assert_eq!(
            plugin,
            (
                "v42 旧插件".into(),
                "0.9.0".into(),
                1_i64,
                "C:/isolated-test/plugins/com.pomegranate.test.v42".into(),
                "fixture-content-hash".into()
            )
        );
        let setting: String = conn.query_row(
            "SELECT value FROM plugin_settings WHERE plugin_id = ?1 AND key = ?2",
            params![
                "com.pomegranate.test.v42",
                "plugin:com.pomegranate.test.v42:mode"
            ],
            |row| row.get(0),
        )?;
        assert_eq!(setting, r#""preserve-me""#);
        let granted: i64 = conn.query_row(
            "SELECT granted FROM plugin_permissions WHERE plugin_id = ?1 AND permission = ?2",
            params!["com.pomegranate.test.v42", "notes.read"],
            |row| row.get(0),
        )?;
        assert_eq!(granted, 1);
        Ok(())
    }

    fn assert_week4_schema_objects(conn: &Connection) -> Result<(), AppError> {
        for table in [
            "products",
            "product_versions",
            "plugin_installations",
            "entitlements",
            "orders",
            "credentials",
            "external_agents",
            "plugin_versions",
            "plugin_activation_settings",
            "plugin_execution_logs",
        ] {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "缺少或重复数据库表：{table}");
        }
        for index in [
            "idx_product_versions_product",
            "idx_credentials_provider",
            "idx_external_agents_product",
            "idx_plugin_versions_one_current",
            "idx_product_versions_review_installable",
        ] {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "缺少或重复数据库索引：{index}");
        }

        for table in ["plugins", "products", "product_versions", "external_agents"] {
            let columns = list_columns(conn, table)?;
            let unique: HashSet<_> = columns.iter().collect();
            assert_eq!(columns.len(), unique.len(), "存在重复列：{table}");
        }
        Ok(())
    }

    fn assert_migrated_plugin_relations(conn: &Connection) -> Result<(), AppError> {
        let product: (String, String) = conn.query_row(
            "SELECT id, plugin_id FROM products WHERE id = ?1",
            ["com.pomegranate.test.v42"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(
            product,
            (
                "com.pomegranate.test.v42".into(),
                "com.pomegranate.test.v42".into()
            )
        );

        let version_id: i64 = conn.query_row(
            "SELECT id FROM product_versions WHERE product_id = ?1 AND version = ?2",
            params!["com.pomegranate.test.v42", "0.9.0"],
            |row| row.get(0),
        )?;
        let installation: (String, i64, String, i64) = conn.query_row(
            "SELECT product_id, product_version_id, installed_version, enabled
             FROM plugin_installations WHERE plugin_id = ?1",
            ["com.pomegranate.test.v42"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        assert_eq!(
            installation,
            (
                "com.pomegranate.test.v42".into(),
                version_id,
                "0.9.0".into(),
                1_i64
            )
        );
        Ok(())
    }

    fn assert_services_read_migrated_data(conn: Connection) -> Result<(), AppError> {
        let db = Database {
            conn: Mutex::new(conn),
        };
        let plugins = PluginService::list(&db)?;
        let legacy = plugins
            .iter()
            .find(|plugin| plugin.id == "com.pomegranate.test.v42")
            .ok_or_else(|| AppError::NotFound("迁移后的旧插件无法由 PluginService 读取".into()))?;
        assert!(legacy.enabled);
        assert_eq!(legacy.version, "0.9.0");

        let settings = PluginService::get_settings(&db, "com.pomegranate.test.v42")?;
        assert_eq!(
            settings.get("plugin:com.pomegranate.test.v42:mode"),
            Some(&serde_json::Value::String("preserve-me".into()))
        );

        let temp_root =
            std::env::temp_dir().join(format!("pomegranate-week4-migration-{}", Uuid::new_v4()));
        let query = MarketplaceProductQuery {
            keyword: None,
            product_type: None,
            runtime_kind: None,
            free_only: None,
            acquired_only: None,
            installed_only: None,
            byok_only: None,
            status: None,
        };
        let products = MarketplaceService::list_products(&db, &temp_root, query);
        let cleanup = if temp_root.exists() {
            std::fs::remove_dir_all(&temp_root)
        } else {
            Ok(())
        };
        let products = products?;
        cleanup?;
        assert!(
            !products.is_empty(),
            "MarketplaceService 应能读取 Week4 初始化写入的市场商品"
        );
        assert!(
            !products
                .iter()
                .any(|product| product.id == "com.pomegranate.test.v42"),
            "旧本地插件不应以公开市场商品身份暴露"
        );
        Ok(())
    }

    fn create_partial_v43_products_table(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(
            r#"
            CREATE TABLE products (
                id              TEXT PRIMARY KEY,
                developer_id    TEXT NOT NULL,
                name            TEXT NOT NULL,
                description     TEXT,
                product_type    TEXT NOT NULL,
                status          TEXT NOT NULL DEFAULT 'draft',
                created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                updated_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
            );
            CREATE TRIGGER week4_abort_product_seed
            BEFORE INSERT ON products
            BEGIN
                SELECT RAISE(ABORT, 'week4 injected migration failure');
            END;
            "#,
        )?;
        Ok(())
    }

    #[test]
    fn initializes_plugin_marketplace_schema_without_account_regression() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;

        let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        assert_eq!(version, SCHEMA_VERSION);

        // 同时检查插件市场新增表与账号核心表，防止定向合并遗漏任一侧迁移。
        for table in [
            "plugins",
            "plugin_versions",
            "products",
            "orders",
            "credentials",
            "external_agents",
            "users",
        ] {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "缺少数据库表：{table}");
        }

        Ok(())
    }

    #[test]
    fn upgrades_real_v42_fixture_to_v55_without_losing_data() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate_fixture_to_v42(&conn)?;
        seed_v42_representative_data(&conn)?;

        migrate(&conn)?;

        assert_eq!(get_version(&conn)?, 55);
        assert_v42_data_preserved(&conn)?;
        assert_week4_schema_objects(&conn)?;
        assert_migrated_plugin_relations(&conn)?;
        assert_services_read_migrated_data(conn)?;
        Ok(())
    }

    #[test]
    fn retries_v42_after_partial_v43_ddl_without_data_loss() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate_fixture_to_v42(&conn)?;
        seed_v42_representative_data(&conn)?;
        create_partial_v43_products_table(&conn)?;

        let first_failure = match migrate(&conn) {
            Ok(()) => panic!("故障注入应使首次迁移失败"),
            Err(error) => error.to_string(),
        };
        assert!(
            first_failure.contains("week4 injected migration failure"),
            "首次失败并非来自预期注入点: {first_failure}"
        );
        assert_eq!(get_version(&conn)?, 42, "迁移失败后不得推进 user_version");
        assert_v42_data_preserved(&conn)?;
        assert!(
            list_columns(&conn, "plugins")?
                .iter()
                .any(|column| column == "manifest_format"),
            "首次失败前应已形成可重复的部分 v43 DDL 状态"
        );
        for table in [
            "products",
            "product_versions",
            "product_permissions",
            "plugin_installations",
            "entitlements",
        ] {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "失败后应保留已创建的部分 v43 DDL: {table}");
        }
        let partial_product_rows: i64 =
            conn.query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))?;
        assert_eq!(
            partial_product_rows, 0,
            "注入失败发生在首条商品写入前，不应留下半条商品数据"
        );

        conn.execute_batch("DROP TRIGGER week4_abort_product_seed;")?;
        migrate(&conn)?;

        assert_eq!(get_version(&conn)?, 55);
        assert_v42_data_preserved(&conn)?;
        assert_week4_schema_objects(&conn)?;
        assert_migrated_plugin_relations(&conn)?;
        assert_services_read_migrated_data(conn)?;
        Ok(())
    }
}
