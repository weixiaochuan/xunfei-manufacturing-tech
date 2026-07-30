"""
Phase 3 第一步：题目难度标定（Difficulty calibration）

自适应推题（IRT/CAT）的前提是"每道题有一个难度值 b"。理想情况下 b 由大量真实作答
数据拟合出来（每题 30~50 条作答）。我们现在没有那么多作答数据，所以先做**冷启动标定**：
用三个互相独立的证据源估一个初始 b，等真实作答数据攒够了再用数据校准（refresh 命令）。

三个证据源（都不需要真人刷题）：

  1. 【真题证据 · 最硬】老师的历年真题告诉了我们真实的难度分层：
     - 名词解释 4 分  —— 只考记忆，最简单
     - 简述题  10 分  —— 考理解和归纳，中等
     - 分析计算题 10~20 分 —— 考应用和推导，最难（20分那道通常是尺寸链综合题）
     分值本身就是老师对"这题多难/多重要"的判断，这是最可信的先验。
     另外，**在历年卷子里反复出现的知识点 = 重点**，我们统计了复现次数。

  2. 【考试大纲权重】各章在考试中的分值占比（见 syllabus_coverage.py），
     权重高的章节 = 更核心 = 学生更该练。（这影响"该不该推"，不直接影响难度。）

  3. 【LLM 估计】让模型读题目内容，估一个 1~5 的难度。
     ⚠️ 老实说：**LLM 估的难度不可靠**，它没见过学生，只能凭"看起来难不难"猜。
     所以它只占权重的一小部分，且只在没有更硬证据时起作用。

b 的取值范围沿用 IRT 惯例：-3（最易）到 +3（最难），0 是中等。

用法：
    # 冷启动：给所有题标一个初始难度（不需要模型也能跑，只是少一个证据源）
    python3 pipeline/calibrate_difficulty.py --cold-start
    python3 pipeline/calibrate_difficulty.py --cold-start --provider deepseek   # 带LLM估计

    # 有真实作答数据后：用数据校准（每题≥20条作答才会被校准）
    python3 pipeline/calibrate_difficulty.py --refresh

    # 看现在的难度分布
    python3 pipeline/calibrate_difficulty.py --stat
"""
import argparse
import json
import math
import os
import sqlite3
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

# 证据1：真题题型 -> 难度先验（分值越高、越靠推导，越难）
TYPE_PRIOR = {
    "名词解释": -1.2,   # 记忆，最易
    "单选": -0.5,       # 我们AI出的选择题，普遍偏易（真实考试里根本没有选择题）
    "简述": 0.3,        # 理解+归纳
    "计算": 1.0,        # 应用+推导，最难
    "作图": 0.8,
}
# 证据1b：分值越高越难（20分的综合题明显比10分的难）
SCORE_ADJ = {4: -0.2, 10: 0.0, 15: 0.3, 20: 0.6, 40: 0.8}

# Bloom 认知层级 -> 难度微调
BLOOM_ADJ = {"记忆": -0.5, "理解": -0.2, "应用": 0.3, "分析": 0.6, "评价": 0.8, "创造": 1.0}

LLM_SYSTEM = """你是一位机械制造课的资深教师，正在估计一道题对**大二/大三本科生**的难度。

【难度分5档，只输出数字】
1 = 很简单（背过定义就能答）
2 = 简单（理解概念即可）
3 = 中等（需要理解并会套用）
4 = 较难（要多步推导，或综合几个概念）
5 = 很难（综合题，要建模、多步计算，容易出错）

【重要】你没见过真实学生的作答，所以你的估计只是个参考。请保守、诚实地估，
不要因为题目文字长就判难，也不要因为看起来眼熟就判易。

只输出一个JSON：{"difficulty": 1-5的整数, "reason": "一句话理由"}"""


def _clamp(x, lo=-3.0, hi=3.0):
    return max(lo, min(hi, x))


def real_exam_hot_nodes(conn):
    """统计：哪些知识点在历年真题里反复出现 = 老师眼里的重点。"""
    rows = conn.execute(
        "SELECT course_chapter, stem FROM questions WHERE source='真题'").fetchall()
    c = Counter(r[0] for r in rows)
    return c


def cold_start(conn, provider=None):
    from_llm = 0
    client = None
    if provider:
        try:
            from llm.client import get_client
            client = get_client("concept", provider_name=provider)
        except Exception as e:
            print(f"（模型不可用，跳过LLM估计：{e}）")
            client = None

    hot = real_exam_hot_nodes(conn)
    rows = conn.execute(
        "SELECT question_id, question_type, bloom_level, total_score, course_chapter, "
        "stem, source FROM questions WHERE review_status='已通过'").fetchall()

    print(f"冷启动标定 {len(rows)} 道题…\n")
    updated = 0
    for r in rows:
        qid, qtype, bloom, total, chapter, stem, source = r

        # 证据1：题型先验（真题告诉我们的难度分层）
        b = TYPE_PRIOR.get(qtype, 0.0)

        # 证据1b：分值
        if total:
            # 找最接近的档
            key = min(SCORE_ADJ, key=lambda k: abs(k - total))
            b += SCORE_ADJ[key]

        # 证据1c：真题本身通常比AI出的题难（真题是考场题，AI题偏基础）
        if source == "真题":
            b += 0.3

        # 证据2：Bloom
        if bloom:
            for k, v in BLOOM_ADJ.items():
                if k in str(bloom):
                    b += v
                    break

        # 证据3：LLM 估计（权重小，只做微调；它最不可靠）
        if client:
            try:
                from pipeline.generate_questions import parse_llm_json
                d = parse_llm_json(client.chat(
                    LLM_SYSTEM, f"【题型】{qtype}\n【题目】{stem[:400]}", temperature=0.2))
                lv = int(d.get("difficulty", 3))
                # 1~5 映射到 -1..+1，再乘 0.4 的权重（不让它主导）
                b += 0.4 * ((lv - 3) / 2.0)
                from_llm += 1
            except Exception:
                pass

        b = _clamp(round(b, 2))
        conn.execute(
            "UPDATE questions SET irt_difficulty_b=?, irt_discrimination_a=? WHERE question_id=?",
            (b, 1.0, qid))
        updated += 1
    conn.commit()
    print(f"已标定 {updated} 道题" + (f"（其中 {from_llm} 道用了LLM估计）" if from_llm else "（未用LLM）"))
    print("\n⚠️ 重要提醒：这是**冷启动估计值，不是真实难度**。")
    print("   它的作用是让自适应推题现在就能跑起来，而不是精确的难度。")
    print("   等真实作答数据攒够（每题≥20条），跑 --refresh 用数据校准。")


def refresh(conn, min_answers=20):
    """有真实作答数据后，用数据校准难度。
    用最朴素但稳健的做法：b = -logit(正确率)（正确率越低，b 越大=越难）。
    等数据更多了可以换成完整的 IRT 2PL 拟合。"""
    rows = conn.execute(
        """SELECT q.question_id, COUNT(*) n, SUM(CASE WHEN a.is_correct=1 THEN 1 ELSE 0 END) c
           FROM student_answers a JOIN questions q ON a.question_id=q.question_id
           WHERE a.is_correct IS NOT NULL
           GROUP BY q.question_id HAVING n >= ?""", (min_answers,)).fetchall()
    if not rows:
        n_ans = conn.execute("SELECT COUNT(*) FROM student_answers").fetchone()[0]
        print(f"还没有任何题达到 {min_answers} 条作答（当前总作答 {n_ans} 条），无法用数据校准。")
        print("继续收集学生作答数据即可。现在用的是冷启动估计值。")
        return
    print(f"用真实作答数据校准 {len(rows)} 道题（每题≥{min_answers}条作答）…\n")
    for qid, n, c in rows:
        p = c / n
        p = min(max(p, 0.02), 0.98)          # 防止 log(0)
        b = _clamp(-math.log(p / (1 - p)))   # 正确率50% -> b=0
        conn.execute(
            "UPDATE questions SET irt_difficulty_b=?, correct_rate=?, answer_count=? "
            "WHERE question_id=?", (round(b, 2), round(p, 3), n, qid))
        print(f"  {qid}  正确率{p*100:.0f}%  ->  b={b:+.2f}")
    conn.commit()
    print(f"\n已用真实数据校准 {len(rows)} 道题。这些题的难度现在是**可信的**。")


def stat(conn):
    rows = conn.execute(
        "SELECT question_type, source, COUNT(*), AVG(irt_difficulty_b), "
        "MIN(irt_difficulty_b), MAX(irt_difficulty_b) "
        "FROM questions WHERE review_status='已通过' AND irt_difficulty_b IS NOT NULL "
        "GROUP BY question_type, source").fetchall()
    if not rows:
        print("还没有标定过难度。先跑 --cold-start。")
        return
    print(f"{'题型':<10}{'来源':<8}{'题数':>5}{'平均难度b':>10}{'范围':>16}")
    for t, src, n, avg, mn, mx in rows:
        print(f"{t:<10}{src or 'AI生成':<8}{n:>5}{avg:>10.2f}   [{mn:+.1f}, {mx:+.1f}]")
    calibrated = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE answer_count >= 20").fetchone()[0]
    total = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE review_status='已通过'").fetchone()[0]
    print(f"\n其中用真实作答数据校准过的：{calibrated} / {total} 道")
    if calibrated == 0:
        print("（其余都是冷启动估计值，仅供推题时排序用，不要当成真实难度）")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cold-start", action="store_true")
    ap.add_argument("--refresh", action="store_true")
    ap.add_argument("--stat", action="store_true")
    ap.add_argument("--provider", default=None)
    ap.add_argument("--min-answers", type=int, default=20)
    a = ap.parse_args()

    conn = connect_database()
    if a.cold_start:
        cold_start(conn, a.provider)
    elif a.refresh:
        refresh(conn, a.min_answers)
    else:
        stat(conn)
    conn.close()


if __name__ == "__main__":
    main()
