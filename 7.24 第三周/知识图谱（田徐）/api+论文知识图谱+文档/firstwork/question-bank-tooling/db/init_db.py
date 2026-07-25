"""
初始化SQLite数据库：建表 + 导入知识库Excel数据
用法：python3 db/init_db.py [--reset]
"""
import argparse
import os
import sqlite3
import sys

import openpyxl

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path, reset_database
DB_PATH = database_path()
SCHEMA_PATH = os.path.join(BASE_DIR, "db", "schema.sql")
DATA_DIR = os.path.join(BASE_DIR, "data")

# 知识库Excel文件名 -> chapter标签。
# 【注意】文件名改成了纯英文 ch1~ch7。
# 原因：原来用中文文件名，在 Windows 上解压时中文被转成 "#U7ed7#Ue0ff..." 这种转义串，
# 文件名长度暴涨到 180 多个字符，触发 Windows 的"路径太长"错误(0x80010135)，压缩包解不开。
# 章节的中文名保留在下面的值里，不影响任何功能。
KNOWLEDGE_FILES = {
    "knowledge_base_ch1.xlsx": "第一章_绪论",
    "knowledge_base_ch2.xlsx": "第二章_机械加工工艺规程设计",
    "knowledge_base_ch3.xlsx": "第三章_机床夹具设计",
    "knowledge_base_ch4.xlsx": "第四章_机械加工精度及其控制",
    "knowledge_base_ch5.xlsx": "第五章_机械加工表面质量及其控制",
    "knowledge_base_ch6.xlsx": "第六章_机器装配工艺过程设计",
    "knowledge_base_ch7.xlsx": "第七章_机械制造工艺理论和技术的发展",
}


def build_schema(conn):
    with open(SCHEMA_PATH, "r", encoding="utf-8") as f:
        conn.executescript(f.read())
    conn.commit()


QUESTION_EXTENSION_COLUMNS = {
    "explanation_old": "TEXT",
    "image_path": "TEXT",
    "image_reviewed": "INTEGER DEFAULT 0",
    "rubric_json": "TEXT",
    "total_score": "REAL",
    "source": "TEXT DEFAULT 'AI生成'",
    "usage_scope": "TEXT DEFAULT '学生练习'",
    "no_answer_reason": "TEXT",
    "exam_source": "TEXT",
    "answer_source": "TEXT",
    "answer_image_path": "TEXT",
}


def ensure_question_columns(conn):
    """Bring early development databases up to the production-tooling shape."""
    existing = {row[1] for row in conn.execute("PRAGMA table_info(questions)")}
    for name, sql_type in QUESTION_EXTENSION_COLUMNS.items():
        if name not in existing:
            conn.execute(f"ALTER TABLE questions ADD COLUMN {name} {sql_type}")
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_questions_student_scope "
        "ON questions(review_status, usage_scope, course_chapter)"
    )
    conn.commit()


def import_knowledge_file(conn, filepath, chapter_label):
    wb = openpyxl.load_workbook(filepath, data_only=True)
    ws = wb.active
    rows = list(ws.iter_rows(values_only=True))
    headers = rows[0]
    expected = [
        "knowledge_id", "section_title", "knowledge_title", "content",
        "key_concepts", "formulas", "figures", "difficulty", "knowledge_type",
        "prerequisites", "dependencies", "tags", "page", "learning_order", "metadata",
    ]
    # 兼容第三章多出来的 chapter/chapter_num 两列：只要必需列都在即可，
    # 后面按列名取值，多余的列自动忽略，不依赖列的先后顺序。
    missing = [c for c in expected if c not in headers]
    if missing:
        print(f"  [警告] {filepath} 缺少必需列 {missing}，实际表头: {headers}")

    cur = conn.cursor()
    count = 0
    for row in rows[1:]:
        if row[0] is None:  # 跳过空行
            continue
        rec = dict(zip(headers, row))
        cur.execute(
            """
            INSERT INTO knowledge_points
                (knowledge_id, chapter, section_title, knowledge_title, content,
                 key_concepts, formulas, figures, difficulty, knowledge_type,
                 prerequisites, dependencies, tags, page, learning_order, metadata)
            VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(knowledge_id) DO UPDATE SET
                chapter=excluded.chapter, section_title=excluded.section_title,
                knowledge_title=excluded.knowledge_title, content=excluded.content,
                key_concepts=excluded.key_concepts, formulas=excluded.formulas,
                figures=excluded.figures, difficulty=excluded.difficulty,
                knowledge_type=excluded.knowledge_type, prerequisites=excluded.prerequisites,
                dependencies=excluded.dependencies, tags=excluded.tags, page=excluded.page,
                learning_order=excluded.learning_order, metadata=excluded.metadata
            """,
            (
                rec["knowledge_id"], chapter_label, rec["section_title"], rec["knowledge_title"],
                rec["content"], rec["key_concepts"], rec["formulas"], rec["figures"],
                rec["difficulty"], rec["knowledge_type"], rec["prerequisites"],
                rec["dependencies"], rec["tags"], rec["page"], rec["learning_order"],
                rec["metadata"],
            ),
        )
        count += 1
    conn.commit()
    return count


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reset", action="store_true", help="删除已有数据库重新建库")
    args = parser.parse_args()

    if args.reset and os.path.exists(DB_PATH):
        reset_database()
        print(f"已删除旧数据库 {DB_PATH}")

    conn = connect_database()
    build_schema(conn)
    ensure_question_columns(conn)
    print("建表完成。")

    total = 0
    for fname, chapter_label in KNOWLEDGE_FILES.items():
        fpath = os.path.join(DATA_DIR, fname)
        if not os.path.exists(fpath):
            print(f"  [跳过] 未找到 {fpath}")
            continue
        n = import_knowledge_file(conn, fpath, chapter_label)
        print(f"  导入 {fname} -> {n} 条知识点")
        total += n

    print(f"知识点导入总计：{total} 条")

    cur = conn.execute("SELECT COUNT(*) FROM knowledge_points")
    print(f"数据库中 knowledge_points 表当前共 {cur.fetchone()[0]} 条记录")
    conn.close()


if __name__ == "__main__":
    sys.exit(main())
