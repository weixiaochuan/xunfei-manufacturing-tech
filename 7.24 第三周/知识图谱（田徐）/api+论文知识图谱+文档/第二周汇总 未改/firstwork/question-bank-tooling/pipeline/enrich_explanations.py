"""
把题库里"只会拿原文当挡箭牌"的解析，升级成"讲清楚为什么"的解析（跑一次即可）。

为什么要单独做这件事：
    用户反馈的头号问题——很多解析写成 "原文明确指出'X是Y'，因此选X"。
    这等于告诉学生"书上就这么写的，别问为什么"，学习变成了背书。
    出题 Prompt 已经改好了（以后出的新题不会再这样），但**题库里已经通过审核的
    178 道老题，解析还是旧的**。离线模式直接展示这个解析，所以必须回头把它们补好。

它做什么：
    对每道已通过的题，把 题干+选项+原答案+旧解析+知识点原文 交给模型，
    要求重写解析：讲清因果/机理/区别，而不是复述原文；干扰项要说清错在哪一步推理。
    只改 explanation 字段，不动题干、选项、答案（避免引入新错误）。
    改写前会把旧解析备份进 explanation_old 列，随时可回滚。

用法：
    # 先小批量试 5 道，看看质量
    python3 pipeline/enrich_explanations.py --provider deepseek --limit 5

    # 满意后跑全部已通过的题（178道，大约几毛钱）
    python3 pipeline/enrich_explanations.py --provider deepseek

    # 只跑某一章
    python3 pipeline/enrich_explanations.py --provider deepseek --chapter 第一章_绪论

    # 回滚（把备份的旧解析恢复回去）
    python3 pipeline/enrich_explanations.py --rollback
"""
import argparse
import json
import os
import sqlite3
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

SYSTEM = """你是一位机械制造课的资深教师，正在重写一道题的【解析】，让学生**理解**而不是背书。

【不合格的解析长这样（禁止）】
✗ "原文明确指出'X是Y'，因此正确答案是X。其余选项与原文不符。"
   —— 这只是把原文抄一遍，等于告诉学生"书上就这么写，别问为什么"，毫无价值。

【合格的解析要做到】
1. 讲清**因果/机理/区别**：正确答案为什么成立？它凭什么区别于另外三个选项？
   背后的原理、条件、时间顺序、从属关系是什么？
2. 每个干扰项要点出它**错在哪一步推理**（不是简单一句"与原文不符"）。
3. 用"你"和学生说话，简洁、有信息量，不说废话、不重复同一句话。
4. 忠于事实：可以基于本学科通识把"为什么"补全，但**不得编造原文没有的事实性结论
   （数据、公式、定义）**。补的是逻辑，不是事实。
5. 长度控制在 3-6 句，别写小作文。

【只输出一个JSON对象，不要用Markdown代码块包裹】
{"explanation": "重写后的解析"}"""

USER = """【题目】{stem}

【选项】
{options}

【正确答案】{answer}

【原来的解析（质量不好，需要你重写）】
{old}

【知识点原文】（可据此把"为什么"讲清楚，但不得编造原文没有的事实）
{content}

请重写这道题的解析。"""


def _ensure_backup_column(conn):
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    if "explanation_old" not in cols:
        conn.execute("ALTER TABLE questions ADD COLUMN explanation_old TEXT")
        conn.commit()


def _content(conn, node):
    r = conn.execute("SELECT content, formulas FROM knowledge_points WHERE knowledge_id=?", (node,)).fetchone()
    if not r:
        return ""
    c = r[0] or ""
    if r[1]:
        c += f"\n【相关公式】{r[1]}"
    return c


def _options_text(options_json, qtype):
    if not options_json:
        return "（无）"
    try:
        opts = json.loads(options_json)
    except Exception:
        return str(options_json)[:800]
    if qtype == "单选" and opts and isinstance(opts[0], dict):
        letters = "ABCDEF"
        return "\n".join(f"{letters[i]}. {o.get('text')}" for i, o in enumerate(opts))
    return "\n".join(str(s) for s in opts)[:1200]


def rollback(conn):
    n = conn.execute(
        "UPDATE questions SET explanation=explanation_old "
        "WHERE explanation_old IS NOT NULL AND explanation_old != ''").rowcount
    conn.commit()
    print(f"已回滚 {n} 道题的解析（恢复为旧版）。")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--provider", help="用哪个模型重写解析，如 deepseek")
    ap.add_argument("--chapter", default=None)
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--rollback", action="store_true", help="恢复旧解析")
    a = ap.parse_args()

    conn = connect_database()
    conn.row_factory = sqlite3.Row
    _ensure_backup_column(conn)

    if a.rollback:
        rollback(conn)
        conn.close()
        return
    if not a.provider:
        ap.error("需要 --provider（如 deepseek），或用 --rollback")

    from llm.client import get_client
    from pipeline.generate_questions import parse_llm_json
    client = get_client("concept", provider_name=a.provider)

    sql = "SELECT * FROM questions WHERE review_status='已通过'"
    params = []
    if a.chapter:
        sql += " AND course_chapter=?"; params.append(a.chapter)
    sql += " ORDER BY course_chapter, created_at"
    if a.limit:
        sql += " LIMIT ?"; params.append(a.limit)
    rows = [dict(r) for r in conn.execute(sql, params).fetchall()]
    print(f"准备重写 {len(rows)} 道题的解析，模型：{a.provider}\n")

    ok, fail = 0, 0
    for i, q in enumerate(rows, 1):
        try:
            user = USER.format(
                stem=q["stem"],
                options=_options_text(q["options_json"], q["question_type"]),
                answer=q["answer"], old=q["explanation"] or "（无）",
                content=_content(conn, q["source_node_id"])[:2500])
            data = parse_llm_json(client.chat(SYSTEM, user, temperature=0.4))
            new = (data.get("explanation") or "").strip()
            if not new:
                raise ValueError("模型没给出解析")
            if not q.get("explanation_old"):
                conn.execute("UPDATE questions SET explanation_old=? WHERE question_id=?",
                             (q["explanation"], q["question_id"]))
            conn.execute("UPDATE questions SET explanation=? WHERE question_id=?",
                         (new, q["question_id"]))
            conn.commit()
            ok += 1
            print(f"[{i}/{len(rows)}] {q['question_id']} ✓")
            if i <= 2:
                print(f"    旧：{(q['explanation'] or '')[:60]}")
                print(f"    新：{new[:60]}")
        except Exception as e:
            fail += 1
            print(f"[{i}/{len(rows)}] {q['question_id']} ✗ {e}")

    conn.close()
    print(f"\n完成：成功 {ok} 道，失败 {fail} 道。")
    print("旧解析已备份在 explanation_old 列；不满意可用 --rollback 一键恢复。")


if __name__ == "__main__":
    main()
