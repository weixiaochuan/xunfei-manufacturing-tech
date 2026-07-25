"""
修复计算题的答案 —— 这是你反馈了四五次的问题，我这次从根上改。

【为什么一直没改好，我说实话】
根因在**我写的 Prompt 里**：
    "answer": "最终数值答案（含单位）"
我**明确要求模型只给一个数字**。所以模型给的答案就是 "Cp≈0.83，不合格品率≈1.24%"——
完整的解题过程被塞进了 explanation，而界面上"标准答案"那一栏显示的是 answer。
**是我的 Prompt 写错了，不是模型的问题。** 我之前几次只在界面和清洗上打补丁，没去动 Prompt，所以你怎么反馈都没用。

【已经改的】
1. Prompt 改了：answer 必须是**考生要在卷子上写的全部内容**（依据+公式+代入算式+中间结果+结论），
   并且必须给出采分点；explanation 只讲思路（为什么用这个公式），不重复算式。
2. 入库时会存采分点和满分（以前计算题根本没存）。

【这个脚本做什么】
把**已经在库里的**那些"只有一个数字"的计算题答案修好。
好消息：出题时模型给的 calculation_steps（分步算式）**一直都存着**（在 options_json 里），
所以不用重新调模型，直接把它们拼成完整的标准答案就行——**不花钱，不联网**。

拼出来的答案长这样：
    （1）依据：工艺能力指数 Cp = T/(6σ)。
    （2）已知公差带 T = 2×0.05 = 0.10 mm，σ = 0.02 mm。
    （3）代入：Cp = 0.10/(6×0.02) = 0.833。
    （4）结论：Cp = 0.83 < 1，工艺能力不足。

用法：
    python3 pipeline/fix_calc_answers.py --scan
    python3 pipeline/fix_calc_answers.py --apply
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


def looks_like_bare_result(ans):
    """这个答案是不是"只有一个结果"（没有过程）？"""
    if not ans:
        return True
    a = ans.strip()
    if len(a) < 90 and "\n" not in a:
        return True
    # 没有任何等号算式 -> 没有过程
    if "=" not in a and "＝" not in a:
        return True
    return False


def build_answer(steps, old_answer):
    """把分步算式拼成标准答案。"""
    if not steps:
        return None
    lines = []
    n = 1
    for s in steps:
        t = re.sub(r"^第?\s*\d+\s*步\s*[：:、.]\s*", "", str(s)).strip()
        if not t:
            continue
        lines.append(f"（{n}）{t}")
        n += 1
    if not lines:
        return None
    # 最后补一句结论（用原来的那个数字答案）
    if old_answer and old_answer.strip():
        concl = old_answer.strip()
        if not any(concl[:12] in l for l in lines):
            lines.append(f"（{n}）结论：{concl}")
    return "\n".join(lines)


def build_rubric(steps, total=10):
    """按步骤分采分点。"""
    if not steps:
        return []
    n = len(steps)
    base = total // n
    rem = total - base * n
    out = []
    for i, s in enumerate(steps):
        t = re.sub(r"^第?\s*\d+\s*步\s*[：:、.]\s*", "", str(s)).strip()
        sc = base + (1 if i < rem else 0)
        if sc <= 0:
            continue
        out.append({"point": t[:60], "score": sc})
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    conn = connect_database()
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT question_id, stem, answer, explanation, options_json, rubric_json "
        "FROM questions WHERE question_type='计算' AND source='AI生成'").fetchall()

    plans = []
    for r in rows:
        if not looks_like_bare_result(r["answer"]):
            continue
        try:
            steps = json.loads(r["options_json"] or "[]")
        except Exception:
            steps = []
        if not isinstance(steps, list) or not steps:
            continue
        new_ans = build_answer(steps, r["answer"])
        if not new_ans or len(new_ans) < 40:
            continue
        rub = build_rubric(steps)
        plans.append((r["question_id"], r["stem"], r["answer"], new_ans, rub))

    print(f"AI生成的计算题 {len(rows)} 道")
    print(f"其中【答案只有一个结果、没有过程】的 {len(plans)} 道 —— 可以用已存的分步算式补全\n")

    for qid, stem, old, new, rub in plans[:3]:
        print("─" * 62)
        print(f"题：{stem[:56]}")
        print(f"\n修之前的'标准答案'（就是你说的只有一个数字）：\n  {old[:80]}")
        print(f"\n修之后：\n" + "\n".join("  " + l for l in new.split("\n")[:6]))
        pts = " | ".join("{}({}分)".format(x["point"][:22], x["score"]) for x in rub[:3])
        print("\n采分点：" + pts)
    if len(plans) > 3:
        print(f"\n…还有 {len(plans)-3} 道")

    if not a.apply:
        print("\n（--scan 只看。加 --apply 写库。）")
        conn.close()
        return

    for qid, _, _, new_ans, rub in plans:
        total = sum(p["score"] for p in rub) or 10
        conn.execute(
            "UPDATE questions SET answer=?, rubric_json=?, total_score=? WHERE question_id=?",
            (new_ans, json.dumps(rub, ensure_ascii=False), total, qid))
    conn.commit()
    conn.close()
    print(f"\n已修复 {len(plans)} 道计算题的标准答案（补上完整解题过程 + 采分点）")


if __name__ == "__main__":
    main()
