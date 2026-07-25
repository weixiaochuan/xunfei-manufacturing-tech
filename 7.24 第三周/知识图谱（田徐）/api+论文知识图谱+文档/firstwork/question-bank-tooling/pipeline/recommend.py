"""
Phase 3 第二步：自适应推题（Adaptive recommendation）

思路（这是 CAT/自适应练习的最小可用版本，不玩花的）：

  1. 学生对每个知识点有一个掌握度 θ（0~1，答题时自动更新，见 feedback/feedback.py）。
  2. 每道题有一个难度 b（-3~+3，见 pipeline/calibrate_difficulty.py）。
  3. 推题原则 —— **推"刚好比你现在水平高一点点"的题**：
     · 太简单 → 学不到东西，浪费时间
     · 太难   → 打击信心，且做不出来学不到东西
     · 最优区间：学生答对概率大约 50%~70% 的那些题（教育测量学里的"最大信息量"点，
       也是"最近发展区"的工程化表达）。
  4. 优先推**薄弱知识点**上的题，且薄弱得越厉害越优先。
  5. 同时按**考试大纲权重**加权（第二、三章占25%，比第七章的7%更该练）。

用 2PL 模型算答对概率：
     P(答对) = 1 / (1 + exp(-a * (θ_scaled - b)))
其中 θ_scaled 把 0~1 的掌握度映射到 -3~+3 的能力标尺。

⚠️ 诚实说明：现在难度 b 是冷启动估计值（不是真实数据拟合的），所以推题的**排序方向是对的，
   但精度有限**。等真实作答数据攒够、跑过 calibrate_difficulty.py --refresh 之后，
   推题才会真正准。这套管线现在搭好，数据一到就能立刻用上。

用法：
    # 给某个学生推 5 道题
    python3 pipeline/recommend.py --student demo_student --n 5

    # 只在某一章里推
    python3 pipeline/recommend.py --student demo_student --n 5 --chapter 第三章_机床夹具设计

    # 解释推题理由（调试/答辩用）
    python3 pipeline/recommend.py --student demo_student --n 5 --explain
"""
import argparse
import math
import os
import sqlite3
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

# 考试大纲权重（来自公开考试大纲，见 syllabus_coverage.py）
CHAPTER_WEIGHT = {
    "第一章_绪论": 0.08,
    "第二章_机械加工工艺规程设计": 0.25,
    "第三章_机床夹具设计": 0.25,
    "第四章_机械加工精度及其控制": 0.15,
    "第五章_机械加工表面质量及其控制": 0.10,
    "第六章_机器装配工艺过程设计": 0.10,
    "第七章_机械制造工艺理论和技术的发展": 0.07,
}

DEFAULT_MASTERY = 0.5                       # 没做过的知识点，假设中等

# 【学习目标】—— 用户说得对：推难题还是简单题，应该让学生自己选。
# 不同目标对应不同的"最优答对率区间"和不同的侧重：
#   · 巩固基础：推有把握的题（答对率70-85%），先把会的做熟，建立信心
#   · 攻破薄弱：推薄弱知识点上"跳一跳够得着"的题（答对率50-70%）—— 学习效率最高
#   · 提升拔高：推有挑战的难题（答对率30-50%），冲高分
GOALS = {
    "巩固基础": {"p_low": 0.70, "p_high": 0.85, "weak_w": 0.10, "desc": "推你有把握的题，先把会的做熟"},
    "攻破薄弱": {"p_low": 0.50, "p_high": 0.70, "weak_w": 0.35, "desc": "推你薄弱知识点上跳一跳够得着的题"},
    "提升拔高": {"p_low": 0.30, "p_high": 0.50, "weak_w": 0.10, "desc": "推有挑战的难题，冲高分"},
}
DEFAULT_GOAL = "攻破薄弱"


def theta_from_mastery(m):
    """把掌握度 0~1 映射到能力标尺 -3~+3。0.5 -> 0。"""
    m = min(max(m, 0.02), 0.98)
    return math.log(m / (1 - m))


def p_correct(theta, b, a=1.0):
    return 1.0 / (1.0 + math.exp(-a * (theta - b)))


def recommend(student_id, n=5, chapter=None, conn=None, explain=False,
              goal=DEFAULT_GOAL, qtype=None):
    """goal: 巩固基础 / 攻破薄弱 / 提升拔高   qtype: 只推某个题型"""
    g = GOALS.get(goal, GOALS[DEFAULT_GOAL])
    TARGET_P_LOW, TARGET_P_HIGH = g["p_low"], g["p_high"]
    own = conn is None
    if own:
        conn = connect_database()
    conn.row_factory = sqlite3.Row

    # 学生的掌握度
    mastery = {r["knowledge_id"]: r["mastery_prob"] for r in conn.execute(
        "SELECT knowledge_id, mastery_prob FROM student_knowledge_mastery WHERE student_id=?",
        (student_id,))}
    # 已做过的题（不重复推）
    done = {r[0] for r in conn.execute(
        "SELECT DISTINCT question_id FROM student_answers WHERE student_id=?", (student_id,))}

    sql = ("SELECT question_id, course_chapter, source_node_id, question_type, stem, "
           "irt_difficulty_b, irt_discrimination_a, total_score, source "
           "FROM questions WHERE review_status='已通过' AND irt_difficulty_b IS NOT NULL "
           "AND COALESCE(usage_scope,'学生练习')='学生练习'")   # 无答案的题绝不推给学生
    params = []
    if chapter:
        sql += " AND course_chapter=?"
        params.append(chapter)
    if qtype:
        sql += " AND question_type=?"
        params.append(qtype)
    rows = [dict(r) for r in conn.execute(sql, params)]

    scored = []
    for q in rows:
        if q["question_id"] in done:
            continue
        node = q["source_node_id"]
        m = mastery.get(node, DEFAULT_MASTERY)
        theta = theta_from_mastery(m)
        b = q["irt_difficulty_b"] or 0.0
        a = q["irt_discrimination_a"] or 1.0
        p = p_correct(theta, b, a)

        # 1) 挑战度：离最优区间(50%~70%)越近越好
        if TARGET_P_LOW <= p <= TARGET_P_HIGH:
            fit = 1.0
        else:
            target = TARGET_P_LOW if p < TARGET_P_LOW else TARGET_P_HIGH
            fit = max(0.0, 1.0 - abs(p - target) * 2.5)

        # 2) 薄弱优先：掌握度越低越该练
        weak = 1.0 - m

        # 3) 大纲权重：考试更看重的章节更该练
        w = CHAPTER_WEIGHT.get(q["course_chapter"], 0.1)
        w_norm = w / 0.25   # 归一到 0~1（0.25是最高权重）

        # 权重随学习目标变化：攻破薄弱时更看重"薄弱"，其它目标更看重"难度匹配"
        wk = g["weak_w"]
        score = (1.0 - wk - 0.15) * fit + wk * weak + 0.15 * w_norm
        scored.append((score, p, m, b, q))

    scored.sort(key=lambda x: -x[0])
    out = []
    for score, p, m, b, q in scored[:n]:
        item = {
            "question_id": q["question_id"], "type": q["question_type"],
            "chapter": q["course_chapter"], "stem": q["stem"][:70],
            "difficulty_b": b, "predicted_correct_prob": round(p, 2),
            "your_mastery": round(m, 2), "source": q["source"] or "AI生成",
        }
        if explain:
            # 只给一个极短标签。学生不需要一段解释——信噪比要高。
            item["tag"] = ("巩固" if p >= 0.7 else "适中" if p >= 0.5 else "挑战")
            if m < 0.5:
                item["tag"] += "·薄弱点"
            item["goal"] = goal
        out.append(item)
    if own:
        conn.close()
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--student", default="demo_student")
    ap.add_argument("--n", type=int, default=5)
    ap.add_argument("--chapter", default=None)
    ap.add_argument("--explain", action="store_true")
    a = ap.parse_args()

    recs = recommend(a.student, a.n, a.chapter, explain=a.explain)
    if not recs:
        print("推不出题：可能是题库还没标定难度（先跑 pipeline/calibrate_difficulty.py --cold-start），"
              "或该筛选条件下的题都做过了。")
        return
    print(f"给 {a.student} 推荐 {len(recs)} 道题：\n")
    for i, r in enumerate(recs, 1):
        tag = "【真题】" if r["source"] == "真题" else ""
        print(f"{i}. {tag}[{r['type']}] {r['stem']}")
        print(f"   难度 b={r['difficulty_b']:+.1f}　你的掌握度 {r['your_mastery']*100:.0f}%　"
              f"预计答对率 {r['predicted_correct_prob']*100:.0f}%")
        if a.explain:
            print(f"   理由：{r['why']}")
        print()
    print("说明：优先推'刚好比你现在水平高一点点'的题（预计答对率50%~70%），")
    print("      并向薄弱知识点和考试大纲权重高的章节倾斜。")


if __name__ == "__main__":
    main()
