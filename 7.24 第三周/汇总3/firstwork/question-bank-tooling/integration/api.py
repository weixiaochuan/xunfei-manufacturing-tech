"""
题库对外接口层 —— 这是留给「AI 助学」调用的唯一入口（合同边界）。

为什么单独放一层？
    助学页面上的「开始测试」按钮，将来不需要知道我们内部有几张表、
    用了哪个模型。它只要按下面的合同调用，就能拿到题目。内部怎么改，
    只要这层函数的输入输出不变，助学那边就不用动。这就是「留接口」。

两种调用方式（和石榴调 ppt-master 的思路一致）：
  1) Python 直接 import：from integration.api import get_questions_for_knowledge
  2) 命令行子进程（推荐给 Tauri/Rust 用）：
       python3 integration/api.py get_questions --knowledge_id KN_CH2_006 --n 5
     标准输出是一段 JSON，Rust 端拿到 stdout 解析即可。

合同（输出 JSON 结构）见 integration/接口说明.md。
"""
import argparse
import json
import os
import sqlite3
import sys

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()


def _conn():
    conn = connect_database()
    conn.row_factory = sqlite3.Row
    return conn


def _row_to_question(row):
    """把数据库一行整理成给助学用的干净结构。选项里的 misconception 不外发，
    避免把答案、解析和误区线索提前透给学生端。"""
    d = dict(row)
    options = json.loads(d["options_json"]) if d.get("options_json") else None
    clean_options = None
    if options and isinstance(options, list) and options and isinstance(options[0], dict) and "text" in options[0]:
        clean_options = [{"text": o.get("text")} for o in options]  # 只给选项文字
    return {
        "question_id": d["question_id"],
        "chapter": d["course_chapter"],
        "source_node_id": d["source_node_id"],   # 答错可跳回原文
        "question_type": d["question_type"],
        "stem": d["stem"],
        "options": clean_options,                 # 计算题为 None
        "bloom_level": d["bloom_level"],
        "difficulty": d["subjective_difficulty"],
        "image_path": d.get("image_path"),
        "total_score": d.get("total_score"),
    }


def list_chapters():
    """返回每章知识点数 + 已通过审核的题目数，供助学做章节/阶段选择。"""
    conn = _conn()
    out = []
    for r in conn.execute("SELECT chapter, COUNT(*) c FROM knowledge_points GROUP BY chapter ORDER BY chapter"):
        q = conn.execute(
            "SELECT COUNT(*) FROM questions WHERE course_chapter=? "
            "AND review_status='已通过' "
            "AND COALESCE(usage_scope,'学生练习')='学生练习' "
            "AND TRIM(COALESCE(answer,''))<>'' "
            "AND TRIM(COALESCE(no_answer_reason,''))=''",
            (r["chapter"],),
        ).fetchone()[0]
        out.append({"chapter": r["chapter"], "knowledge_point_count": r["c"], "approved_question_count": q})
    conn.close()
    return out


def get_questions_for_knowledge(knowledge_id, n=5, only_approved=True):
    if not only_approved:
        raise ValueError("学生端接口禁止读取待审核或教师出题内容")
    conn = _conn()
    sql = ("SELECT * FROM questions WHERE source_node_id=? "
           "AND review_status='已通过' "
           "AND COALESCE(usage_scope,'学生练习')='学生练习' "
           "AND TRIM(COALESCE(answer,''))<>'' "
           "AND TRIM(COALESCE(no_answer_reason,''))='' ")
    params = [knowledge_id]
    sql += " ORDER BY RANDOM() LIMIT ?"
    params.append(n)
    rows = conn.execute(sql, params).fetchall()
    conn.close()
    return [_row_to_question(r) for r in rows]


def get_questions_for_chapter(chapter, n=10, only_approved=True):
    if not only_approved:
        raise ValueError("学生端接口禁止读取待审核或教师出题内容")
    conn = _conn()
    sql = ("SELECT * FROM questions WHERE course_chapter=? "
           "AND review_status='已通过' "
           "AND COALESCE(usage_scope,'学生练习')='学生练习' "
           "AND TRIM(COALESCE(answer,''))<>'' "
           "AND TRIM(COALESCE(no_answer_reason,''))='' ")
    params = [chapter]
    sql += " ORDER BY RANDOM() LIMIT ?"
    params.append(n)
    rows = conn.execute(sql, params).fetchall()
    conn.close()
    return [_row_to_question(r) for r in rows]


def submit_answer(question_id, student_answer, student_id,
                  mode="练习", provider=None, use_llm=False):
    """Phase 2 入口：助学的"提交答案"按钮调这个，拿回判分+反馈。
    默认 use_llm=False（离线、即时、免费）；助学想要 LLM 过程层反馈时传 use_llm=True + provider。"""
    from feedback.feedback import generate_feedback
    return generate_feedback(question_id, student_answer, student_id=student_id,
                             mode=mode, provider_name=provider, use_llm=use_llm)


def get_student_progress(student_id="demo_student"):
    """助学/教师端：学生学情画像。"""
    from feedback.feedback import student_progress
    return student_progress(student_id)


def _print_json(obj):
    print(json.dumps(obj, ensure_ascii=False, indent=2))


def main():
    p = argparse.ArgumentParser(description="题库对外接口（供 AI 助学调用）")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list_chapters")
    g = sub.add_parser("get_questions")
    g.add_argument("--knowledge_id")
    g.add_argument("--chapter")
    g.add_argument("--n", type=int, default=5)
    s = sub.add_parser("submit_answer")
    s.add_argument("--question_id", required=True)
    s.add_argument("--answer", required=True)
    s.add_argument("--student_id", required=True)
    s.add_argument("--mode", default="练习")
    s.add_argument("--provider", default=None)
    s.add_argument("--use_llm", action="store_true")
    pr = sub.add_parser("progress")
    pr.add_argument("--student_id", default="demo_student")
    args = p.parse_args()

    if args.cmd == "list_chapters":
        _print_json(list_chapters())
    elif args.cmd == "get_questions":
        if args.knowledge_id:
            _print_json(get_questions_for_knowledge(args.knowledge_id, args.n))
        elif args.chapter:
            _print_json(get_questions_for_chapter(args.chapter, args.n))
        else:
            print("请提供 --knowledge_id 或 --chapter", file=sys.stderr); sys.exit(1)
    elif args.cmd == "submit_answer":
        _print_json(submit_answer(args.question_id, args.answer, args.student_id, mode=args.mode,
                                  provider=args.provider, use_llm=args.use_llm))
    elif args.cmd == "progress":
        _print_json(get_student_progress(args.student_id))


if __name__ == "__main__":
    main()
