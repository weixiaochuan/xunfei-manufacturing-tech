"""
把题库里"第三人称"的误区标注改写成"第二人称"（离线、规则化，不调模型、不花钱）。

为什么需要：
    出题时模型把误区写成了"学生可能认为…"。但这段文本会直接显示在学生面前的
    "你可能踩的误区"标题下面，于是出现了"标题说你、内容说学生"的割裂感。
    424 条误区里有 102 条是这种第三人称写法。

做法：
    纯字符串替换，把开头的"学生可能/学生误以为/学生容易…"等换成"你可能/你误以为…"，
    并去掉句中残留的"学生"主语。不改变语义，不需要模型。

用法：
    python3 db/fix_misconception_person.py            # 预览会改哪些（不写库）
    python3 db/fix_misconception_person.py --apply    # 真正写库
"""
import argparse
import json
import os
import re
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

# 顺序有意义：先处理长的、更具体的搭配
RULES = [
    (r"^学生可能会", "你可能会"),
    (r"^学生可能误以为", "你可能误以为"),
    (r"^学生可能认为", "你可能认为"),
    (r"^学生可能混淆", "你可能混淆了"),
    (r"^学生可能", "你可能"),
    (r"^学生误以为", "你误以为"),
    (r"^学生误认为", "你误认为"),
    (r"^学生容易", "你容易"),
    (r"^学生常", "你常"),
    (r"^学生往往", "你往往"),
    (r"^学生", "你"),
    (r"学生可能会", "你可能会"),
    (r"学生可能认为", "你可能认为"),
    (r"学生可能", "你可能"),
    (r"学生误以为", "你误以为"),
    (r"学生容易", "你容易"),
    (r"部分学生", "你"),
    (r"一些学生", "你"),
    (r"学生", "你"),
]


def to_second_person(text):
    if not text or "学生" not in text:
        return text
    out = text
    for pat, rep in RULES:
        out = re.sub(pat, rep, out)
    # 清理可能产生的重复"你你"
    out = out.replace("你你", "你")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="真正写入数据库；不加则只预览")
    a = ap.parse_args()

    conn = connect_database()
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT question_id, options_json FROM questions "
        "WHERE question_type='单选' AND options_json LIKE '%学生%'").fetchall()

    changed_q, changed_m = 0, 0
    samples = []
    for r in rows:
        try:
            opts = json.loads(r["options_json"])
        except Exception:
            continue
        touched = False
        for o in opts:
            m = o.get("misconception")
            new = to_second_person(m)
            if new != m:
                if len(samples) < 6:
                    samples.append((m, new))
                o["misconception"] = new
                changed_m += 1
                touched = True
        if touched:
            changed_q += 1
            if a.apply:
                conn.execute("UPDATE questions SET options_json=? WHERE question_id=?",
                             (json.dumps(opts, ensure_ascii=False), r["question_id"]))

    # 误区库(misconceptions表)里同样处理
    mrows = conn.execute(
        "SELECT rowid AS rid, misconception_text FROM misconceptions WHERE misconception_text LIKE '%学生%'").fetchall()
    changed_lib = 0
    for r in mrows:
        new = to_second_person(r["misconception_text"])
        if new != r["misconception_text"]:
            changed_lib += 1
            if a.apply:
                conn.execute("UPDATE misconceptions SET misconception_text=? WHERE rowid=?",
                             (new, r["rid"]))

    if a.apply:
        conn.commit()
    conn.close()

    print(f"{'已修改' if a.apply else '将修改(预览)'}：")
    print(f"  题目 {changed_q} 道，误区标注 {changed_m} 条")
    print(f"  误区库 {changed_lib} 条")
    print("\n样例（改前 -> 改后）：")
    for old, new in samples:
        print(f"  - {old[:48]}")
        print(f"    -> {new[:48]}")
    if not a.apply:
        print("\n以上只是预览。确认没问题后加 --apply 真正写库。")


if __name__ == "__main__":
    main()
