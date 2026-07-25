"""
救回"没答案"的名词解释 —— 你说得对，我不该那么死板。

【你的原话】
"我不相信书上没有经济精度、进给速度的名词解释！"

**你是对的。** 我查了：
  · 经济精度 -> 知识库第四章里就有原文定义
  · 进给速度 -> 知识库里有
  · 工序 / 工位 / 欠定位 / 工艺基准 -> 别的卷子里就有标准答案
我却因为"这张卷子上没写答案"就把它们全丢进了"没答案"堆里。**是我不灵活。**

【这个脚本做什么】
对每一道"没答案"的名词解释/简述题，按这个顺序找答案：
  1. **别的真题卷子里有没有同一道题？** 有就用那份（老师的标准答案，最可信）
  2. **知识库（教材）里有没有这个概念的定义？** 有就摘出来
  3. 都找不到 -> 保持"没答案"（绝不编）

找到答案的，标记 **answer_source**（答案是从哪来的），并升级为"学生练习"。
教师端会显示答案来源，老师一眼能看出这是原卷答案还是教材定义。

用法：
    python3 pipeline/rescue_answers.py --scan
    python3 pipeline/rescue_answers.py --apply
"""
import argparse
import os
import re
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()


def norm_term(s):
    return re.sub(r"[\s、,，/。.？?：:]", "", s or "")


def find_in_other_papers(conn, stem):
    """别的卷子里有没有同一道题（有答案的那种）？
    带采分点的排前面——那是老师亲手标的评分标准，最可信。"""
    key = norm_term(stem)
    rows = conn.execute(
        "SELECT stem, answer, rubric_json FROM questions "
        "WHERE source='真题' AND COALESCE(usage_scope,'学生练习')='学生练习' "
        "AND answer IS NOT NULL AND answer != '' "
        "ORDER BY CASE WHEN rubric_json IS NOT NULL AND rubric_json NOT IN ('','[]') "
        "THEN 0 ELSE 1 END").fetchall()
    for r in rows:
        k = norm_term(r[0])
        if not k or not key:
            continue
        # 完全相同，或一个包含另一个（"工序" vs "工序、工步、工位"）
        if k == key:
            return r[1], "其它真题卷"
    # 再试：这个术语是不是某道多定义题的一部分（"工序" 在 "工序、工步、工位" 里）
    for r in rows:
        parts = [norm_term(x) for x in re.split(r"[、,，/]", r[0])]
        if key in parts and len(parts) > 1:
            # 从答案里把这一段摘出来
            m = re.search(rf"[（(]?\d*[)）]?\s*{re.escape(stem.strip())}\s*[：:—–]\s*(.+)", r[1])
            if m:
                return m.group(1).strip(), "其它真题卷"
    return None, None


def find_in_knowledge(conn, stem):
    """教材（知识库）里有没有这个概念的定义？"""
    term = stem.strip()
    if len(term) > 14 or "？" in term:
        return None, None
    rows = conn.execute(
        "SELECT knowledge_title, content FROM knowledge_points "
        "WHERE content LIKE ? OR knowledge_title LIKE ?",
        (f"%{term}%", f"%{term}%")).fetchall()
    for title, content in rows:
        if not content:
            continue
        # 找"XX是指…" / "XX是…" / "称为XX" 这样的定义句
        cands = []
        for pat in [rf"({re.escape(term)}(?:是指|指的?是)[^。]{{10,130}}。)",
                    rf"([^。]{{10,130}}称(?:为|作)\s*{re.escape(term)}[^。]{{0,8}}。)",
                    rf"({re.escape(term)}是[^。]{{10,130}}。)"]:
            for m in re.finditer(pat, content):
                cands.append(m.group(1).strip())
        for c in cands:
            # 排除"结论句"：XX是不允许的 / XX是错误的 / XX会导致… —— 这不是定义
            if re.search(r"是(绝对)?不(允许|可以|能)|是错误的|因此|所以|不能保证", c):
                continue
            return c, f"教材·{title[:18]}"
    return None, None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    conn = connect_database()
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    if "answer_source" not in cols:
        if a.apply:
            conn.execute("ALTER TABLE questions ADD COLUMN answer_source TEXT")

    rows = conn.execute(
        "SELECT question_id, question_type, stem FROM questions "
        "WHERE usage_scope='教师出题' AND question_type IN ('名词解释','简述')").fetchall()

    found, missing = [], []
    for qid, qt, stem in rows:
        ans, src = find_in_other_papers(conn, stem)
        if not ans:
            ans, src = find_in_knowledge(conn, stem)
        if ans and len(ans) >= 12:
            found.append((qid, qt, stem, ans, src))
        else:
            missing.append((qt, stem))

    print(f"没答案的名词解释/简述 {len(rows)} 道\n")
    print(f"✅ 找到答案 {len(found)} 道（可以升级成学生练习题）：")
    for qid, qt, stem, ans, src in found:
        print(f"   [{qt}] {stem[:24]}")
        print(f"       来源：{src}")
        print(f"       答案：{ans[:70]}")
    print(f"\n❌ 确实找不到 {len(missing)} 道（保持'没答案'，绝不编）：")
    for qt, stem in missing:
        print(f"   [{qt}] {stem[:40]}")

    if not a.apply:
        print("\n（--scan 只看。加 --apply 写库。）")
        conn.close()
        return

    for qid, qt, stem, ans, src in found:
        conn.execute(
            "UPDATE questions SET answer=?, usage_scope='学生练习', "
            "no_answer_reason=NULL, answer_source=? WHERE question_id=?",
            (ans, src, qid))
    conn.commit()
    conn.close()
    print(f"\n已救回 {len(found)} 道题（补上答案，升级为学生练习题）")


if __name__ == "__main__":
    main()
