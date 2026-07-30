"""
认知误区库种子数据
对应报告2.4节(1)：出题前先建"典型认知误区库"，出题时从中选干扰项，
而不是让模型随便编3个干扰项。

这里先手工种入报告中提到的两个真实例子作为示范。团队后续应在Phase 1
人工审题、Phase 2 归纳学生答题数据的过程中持续往这张表里补充——
misconceptions 表的 source 字段区分"seed"（人工种子）和"llm_mined"
（未来从答题数据/教师反馈中挖掘），方便追溯质量。

用法：python3 db/seed_misconceptions.py
"""
import os
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

SEED_MISCONCEPTIONS = [
    ("KN_CH1_038", "测量基准必须始终与设计基准重合"),
    ("KN_CH1_037", "定位基准和工序基准是同一个概念，可以混用"),
    ("KN_CH1_032", "过定位在任何情况下都绝对禁止（实际上过定位在特定条件下——如提高刚度且不产生干涉——是允许的，绝对不允许的是欠定位）"),
    ("KN_CH1_031", "完全定位就是限制全部六个自由度，越多越好，不需要考虑加工要求"),
    ("KN_CH2_006", "粗基准和精基准的选择原则可以互相套用，没有本质区别"),
]


def main():
    conn = connect_database()
    cur = conn.cursor()
    inserted = 0
    for knowledge_id, text in SEED_MISCONCEPTIONS:
        # 避免重复跑脚本时重复插入同一条
        exists = cur.execute(
            "SELECT 1 FROM misconceptions WHERE knowledge_id=? AND misconception_text=?",
            (knowledge_id, text),
        ).fetchone()
        if exists:
            continue
        cur.execute(
            "INSERT INTO misconceptions (knowledge_id, misconception_text, source) VALUES (?,?,?)",
            (knowledge_id, text, "seed"),
        )
        inserted += 1
    conn.commit()
    total = cur.execute("SELECT COUNT(*) FROM misconceptions").fetchone()[0]
    print(f"新插入 {inserted} 条，误区库当前共 {total} 条")
    conn.close()


if __name__ == "__main__":
    main()
