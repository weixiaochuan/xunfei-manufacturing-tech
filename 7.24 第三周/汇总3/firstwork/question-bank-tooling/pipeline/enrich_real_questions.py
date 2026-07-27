"""
给真题补两样东西：**Bloom 认知层级** 和 **精确的知识点**。

【为什么要做】（都是你抓到的bug）
1. **Bloom 标签没链接到真题** —— 你说得对，323 道 AI 题都有 Bloom，130 道真题一个都没有。
2. **趁热打铁推的永远是同样3道题** —— 根因在这儿：
   导入真题时，我图省事，把**整章的题都挂在了该章的第一个知识点上**
   （54 道第二章的真题，source_node_id 全是 KN_CH2_001）。
   于是"找同知识点的题"就永远返回那前 3 道。**这是糊弄，我认。**

【这个脚本做什么】
1. **按题干内容匹配到真正的知识点**（用知识点的标题和关键词做匹配，而不是一律挂第一个）
2. **按题型 + 题干动词判定 Bloom 层级**：
     · 名词解释 / "什么是" / "简述…的定义"        -> 记忆
     · "简述原则/特点/影响" / "为什么"             -> 理解
     · "试分析" / "计算" / "求" / "安排工艺路线"    -> 应用
     · "比较" / "论述…的机理" / "评价"             -> 分析

用法：
    python3 pipeline/enrich_real_questions.py --scan
    python3 pipeline/enrich_real_questions.py --apply
"""
import argparse
import os
import re
import sqlite3
from collections import Counter

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

# Bloom 判定：从最高层往下匹配（一道题命中多个时取最高的）
BLOOM_RULES = [
    ("分析", [r"比较", r"论述.{0,6}机理", r"评价", r"分析.{0,4}原因", r"为什么.{0,10}会",
              r"有哪些形式.{0,6}机理", r"影响.{0,8}的规律"]),
    ("应用", [r"试分析", r"计算", r"求\s*[^：:]", r"安排.{0,6}工艺路线", r"设计.{0,4}方案",
              r"工序尺寸", r"尺寸链", r"如图", r"下图", r"选择定位", r"限制.{0,4}自由度"]),
    ("理解", [r"简述", r"说明", r"为什么", r"原则", r"特点", r"影响", r"区别", r"作用",
              r"如何", r"怎样", r"措施", r"方法"]),
    ("记忆", [r"什么是", r"定义", r"名词"]),
]


def guess_bloom(qtype, stem):
    if qtype == "名词解释":
        return "记忆"
    for level, pats in BLOOM_RULES:
        for p in pats:
            if re.search(p, stem):
                return level
    return "理解"


def match_node(conn, chapter, stem):
    """把题目挂到**真正相关的知识点**上，而不是一律挂该章第一个。
    做法：拿这一章所有知识点的标题去和题干做关键词匹配，取得分最高的。"""
    rows = conn.execute(
        "SELECT knowledge_id, knowledge_title, COALESCE(key_concepts,'') "
        "FROM knowledge_points WHERE chapter=?",
        (chapter,)).fetchall()
    if not rows:
        return None
    best, best_score = None, 0
    for kid, title, kc in rows:
        # 标题里的关键词（去掉"——"后缀、括号内容）
        core = re.split(r"——|[（(]", title)[0]
        toks = [t for t in re.split(r"[、,，/\s]+", core) if len(t) >= 2]
        # 知识点的"关键概念"字段也拿来匹配，命中更准
        toks += [t for t in re.split(r"[、,，/;；\s]+", kc or "") if len(t) >= 2]
        score = 0
        for t in set(toks):
            if t in stem:
                score += len(t)          # 匹配到的词越长，越可信
        if score > best_score:
            best, best_score = kid, score
    if best_score >= 2:
        return best
    return rows[0][0]        # 实在匹配不上，才退回该章第一个


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    conn = connect_database()
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT question_id, course_chapter, question_type, stem, source_node_id, bloom_level "
        "FROM questions WHERE source='真题'").fetchall()

    plans = []
    for r in rows:
        node = match_node(conn, r["course_chapter"], r["stem"])
        bloom = guess_bloom(r["question_type"], r["stem"])
        if node != r["source_node_id"] or bloom != (r["bloom_level"] or ""):
            plans.append((r["question_id"], node, bloom, r["stem"][:40],
                          r["source_node_id"], r["question_type"]))

    print(f"真题 {len(rows)} 道，需要补/改的 {len(plans)} 道\n")
    print("Bloom 分布（补完之后）:")
    print(" ", dict(Counter(p[2] for p in plans)))
    print("\n知识点分布（补完之后，看还有没有全挤在一个点上）:")
    nd = Counter(p[1] for p in plans)
    for k, v in nd.most_common(6):
        t = conn.execute("SELECT knowledge_title FROM knowledge_points WHERE knowledge_id=?", (k,)).fetchone()
        print(f"   {v:>3} 道  {k}  {t[0][:28] if t else ''}")
    print(f"   （一共分散到 {len(nd)} 个知识点上；之前是全挤在 5 个点上）")

    print("\n样例：")
    for qid, node, bloom, stem, old, qt in plans[:5]:
        t = conn.execute("SELECT knowledge_title FROM knowledge_points WHERE knowledge_id=?", (node,)).fetchone()
        print(f"  [{qt}·{bloom}] {stem}")
        print(f"     {old} -> {node}  ({t[0][:26] if t else ''})")

    if not a.apply:
        print("\n（--scan 只看。加 --apply 写库。）")
        conn.close()
        return

    for qid, node, bloom, _, _, _ in plans:
        conn.execute(
            "UPDATE questions SET source_node_id=?, bloom_level=? WHERE question_id=?",
            (node, bloom, qid))
        conn.execute("DELETE FROM question_knowledge_map WHERE question_id=?", (qid,))
        conn.execute(
            "INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) VALUES (?,?)",
            (qid, node))
    conn.commit()
    conn.close()
    print(f"\n已更新 {len(plans)} 道真题的知识点和 Bloom 标签")


if __name__ == "__main__":
    main()
