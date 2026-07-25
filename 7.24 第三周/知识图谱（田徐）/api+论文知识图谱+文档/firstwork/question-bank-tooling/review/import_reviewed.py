"""
Import graded review results back into the DB (v2).

Handles the Excel produced when Claude Sonnet reviews a batch. Compared with the
first version, this one also:
  1) INSERTS questions that are not yet in the DB (questions generated on your own
     machine only live in the exported Excel until this step).
  2) Understands the graded "review_status" column
     (✅通过 / 🟢轻微 / 🟡中度 / 🟠大改 / ❌重写) OR a blank one that falls back to
     the plain 已通过/已驳回 values.
  3) APPLIES the fixes Sonnet wrote: if 修改后题目 / 修改后答案 are filled, the stem /
     answer / steps are updated to the corrected version, so "modify-then-pass"
     questions actually enter the bank in their fixed form.
  4) Stores 问题类型 + 修改建议 into review_comment for traceability.

Grade -> review_status mapping (tweak here if you want a different policy):
    ✅ 通过             -> 已通过
    🟢 修改后通过（轻微） -> 已通过   (minor issues; enters bank, note kept)
    🟡 修改后通过（中度） -> 已通过   (fix applied from 修改后* columns, then pass)
    🟠 修改后通过（大改） -> 已通过   (fix applied from 修改后* columns, then pass)
    ❌ 不合格（建议重写）  -> 已驳回

Usage:
    python3 review/import_reviewed.py --file "review/xxx_已审核.xlsx"
    python3 review/import_reviewed.py --dir  "review/round2/"   # a whole folder
"""
import argparse
import glob
import json
import os
import sqlite3

import openpyxl

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

GRADE_MAP = {
    "✅ 通过": "已通过",
    "🟢 修改后通过（轻微）": "已通过",
    "🟡 修改后通过（中度）": "已通过",
    "🟠 修改后通过（大改）": "已通过",
    "❌ 不合格（建议重写）": "已驳回",
    # plain values (older exports)
    "已通过": "已通过", "已驳回": "已驳回", "待审核": "待审核", "待复核": "待复核",
}


def _cell(row, idx, name):
    return row[idx[name]] if name in idx and idx[name] < len(row) else None


def process_file(conn, path):
    ws = openpyxl.load_workbook(path).active
    rows = list(ws.iter_rows(values_only=True))
    if not rows:
        return 0, 0, {}
    hdr = list(rows[0])
    idx = {h: i for i, h in enumerate(hdr)}
    existing = {r[0] for r in conn.execute("SELECT question_id FROM questions")}

    inserted, updated, by_status = 0, 0, {}
    for row in rows[1:]:
        qid = _cell(row, idx, "question_id")
        if not qid:
            continue

        grade = _cell(row, idx, "review_status")
        status = GRADE_MAP.get(str(grade).strip()) if grade is not None else None
        if status is None:
            # maybe grade sits in a separate 审核结果 column
            g2 = _cell(row, idx, "审核结果")
            status = GRADE_MAP.get(str(g2).strip()) if g2 is not None else "待审核"

        # Apply Sonnet's fixes when provided
        stem = _cell(row, idx, "修改后题目") or _cell(row, idx, "stem")
        fixed_ans = _cell(row, idx, "修改后答案")
        answer = _cell(row, idx, "answer")
        options_json = _cell(row, idx, "options_json")
        qtype = _cell(row, idx, "question_type")
        # For computation questions, 修改后答案 usually holds corrected steps+final answer.
        # We keep the human-readable answer in `answer`; store corrected steps into options_json
        # only if it looks like step text (starts with 第 or 步).
        if fixed_ans:
            fa = str(fixed_ans)
            if qtype == "计算":
                options_json = json.dumps([s for s in fa.split("\n") if s.strip()],
                                          ensure_ascii=False)
                # last line often is "最终答案：..."; keep whole as answer fallback
                answer = fa.strip().split("\n")[-1][:200] if answer is None else answer
            else:
                answer = fa.strip()[:200]

        # traceability note
        notes = []
        for c in ("问题类型", "修改建议"):
            v = _cell(row, idx, c)
            if v and str(v).strip() not in ("", "无", "None"):
                notes.append(f"{c}:{v}")
        comment = " || ".join(notes)[:800] if notes else _cell(row, idx, "review_comment")

        if qid in existing:
            conn.execute(
                "UPDATE questions SET stem=?, answer=?, options_json=COALESCE(?,options_json), "
                "review_status=?, common_error_tags=? WHERE question_id=?",
                (stem, answer, options_json, status, comment, qid),
            )
            updated += 1
        else:
            conn.execute(
                """INSERT INTO questions
                   (question_id, course_chapter, source_node_id, question_type, stem,
                    options_json, answer, explanation, bloom_level, generation_model,
                    prompt_template_id, review_status, calc_verify_status,
                    calc_verify_detail, common_error_tags, subjective_difficulty)
                   VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
                (qid, _cell(row, idx, "course_chapter"), _cell(row, idx, "source_node_id"),
                 qtype, stem, options_json, answer, _cell(row, idx, "explanation"),
                 _cell(row, idx, "bloom_level"), _cell(row, idx, "generation_model"),
                 "v2", status, _cell(row, idx, "calc_verify_status"),
                 _cell(row, idx, "calc_verify_detail"), comment, None),
            )
            # keep the many-to-many map consistent
            sn = _cell(row, idx, "source_node_id")
            if sn:
                conn.execute(
                    "INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) VALUES (?,?)",
                    (qid, sn),
                )
            inserted += 1
        by_status[status] = by_status.get(status, 0) + 1
    conn.commit()
    return inserted, updated, by_status


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--file", help="single reviewed xlsx")
    ap.add_argument("--dir", help="a folder of reviewed xlsx")
    args = ap.parse_args()

    paths = []
    if args.file:
        paths = [args.file]
    elif args.dir:
        paths = sorted(glob.glob(os.path.join(args.dir, "*.xlsx")))
    else:
        ap.error("give --file or --dir")

    conn = connect_database()
    tot_ins, tot_upd, agg = 0, 0, {}
    for p in paths:
        if os.path.basename(p).startswith("~$"):
            continue
        ins, upd, bs = process_file(conn, p)
        tot_ins += ins; tot_upd += upd
        for k, v in bs.items():
            agg[k] = agg.get(k, 0) + v
        print(f"  {os.path.basename(p)}: 新增{ins} 更新{upd}")
    conn.close()
    print(f"\n合计：新增 {tot_ins} 道，更新 {tot_upd} 道")
    for k, v in sorted(agg.items()):
        print(f"   {k}: {v}")


if __name__ == "__main__":
    main()
