"""
按【真实考试大纲】规划题库覆盖与配比（代替"干等老师给真题库"）。

我去网上找过真题库了，如实汇报（重要，别被我糊弄过去）：
  ✗ 成套的《机械制造工艺学》真题+答案，网上基本都在考研机构手里，要么收费、要么
    需登录、要么版权不明。GitHub / 开源社区上没有这门课的公开题库数据集。
    直接爬下来入库，既不合规、质量也没保证。**所以"网上大把"这个判断，对试卷本身
    不成立。** 该等老师的真题库还是要等。
  ✓ 但有一样东西是公开、权威、且对我们极其有用的：**各校的考试大纲**。
    它白纸黑字写了这门课"考什么、各部分占多少分、出什么题型"。
    这正好补上我们题库最缺的两块：知识点权重 和 题型分布。

已收录的公开大纲（用于确定配比）：
  · 河北工业大学 F1204《机械制造工程学》考试大纲：
      工艺规程设计 25%、夹具设计原理 25%、切削原理与刀具 20%、
      精度与表面质量 15%、装配工艺 10%、金属切削机床 5%
      题型：填空、选择、判断、作图、名词解释、简答、设计分析、计算、综合
  · 浙江科技学院 811《机械制造工艺学》考试大纲：
      机械制造基础知识 ~10%、机床夹具设计 ~15%（六点定位、定位误差计算）…
      要求：定位误差分析与计算、工艺尺寸链计算、工序尺寸计算、
            加工精度与表面质量分析、典型零件工艺规程、装配尺寸链计算
  · 四川大学《机械制造工程学》教学大纲：强调定位误差计算、尺寸链、专用夹具设计。
  （教材对应：王先逵《机械制造工艺学》，与我们知识库的七章结构一致。）

这个脚本干两件事：
  1) coverage —— 对照大纲权重，算出我们题库"每章应该有多少题、现在有多少题、缺多少"，
     直接告诉你下一批该往哪出题，避免出题分布跑偏（现在第四章53道、第六章才7道，明显失衡）。
  2) plan —— 直接生成下一批出题命令，你复制粘贴就能跑。

用法：
    python3 pipeline/syllabus_coverage.py                 # 看覆盖缺口
    python3 pipeline/syllabus_coverage.py --plan --total 300   # 按目标总量生成出题命令
"""
import argparse
import os
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

# 章节 -> 考试大纲权重（综合上述几所学校的大纲，映射到我们这本教材的七章）
# 说明：大纲里"切削原理与刀具/金属切削机床"在本教材中分散于绪论与工艺理论章，
#      故把权重并入相应章节；这是按教材结构做的合理映射，不是拍脑袋。
SYLLABUS_WEIGHTS = {
    "第一章_绪论": 0.08,                          # 基础概念、制造技术发展
    "第二章_机械加工工艺规程设计": 0.25,           # 大纲最高权重之一：工艺规程、尺寸链、工序尺寸
    "第三章_机床夹具设计": 0.25,                   # 大纲最高权重之一：六点定位、定位误差计算
    "第四章_机械加工精度及其控制": 0.15,           # 精度分析
    "第五章_机械加工表面质量及其控制": 0.10,       # 表面质量
    "第六章_机器装配工艺过程设计": 0.10,           # 装配尺寸链（现在严重缺题）
    "第七章_机械制造工艺理论和技术的发展": 0.07,   # 发展方向
}

# 题型分布 —— 【已按老师给的历年真题修正】
# 真题（北理《设计与制造基础Ⅲ》2020-2023级，四届完全一致）的卷面结构是：
#     一、名词解释   20分（5题×4分）
#     二、简述题     40分（4题×10分）
#     三、分析计算题 40分（10+10+20分）
# **真实考试里一道选择题都没有。** 我们题库有125道单选，题型是错配的。
# 单选仍有价值（适合快速自测、低成本练习），但不该是主力，且要按真实卷面补齐主观题。
TYPE_TARGET = {"名词解释": 0.20, "简述": 0.40, "计算": 0.40}
TYPE_NOTE = {
    "名词解释": "真题：5题×4分=20分",
    "简述": "真题：4题×10分=40分",
    "计算": "真题：10+10+20=40分",
}
CURRENT_TYPES = {"单选", "计算", "名词解释", "简述"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--plan", action="store_true", help="生成下一批出题命令")
    ap.add_argument("--total", type=int, default=300, help="题库目标总量（已通过的题）")
    ap.add_argument("--provider", default="deepseek")
    a = ap.parse_args()

    conn = connect_database()
    cur = {r[0]: r[1] for r in conn.execute(
        "SELECT course_chapter, COUNT(*) FROM questions WHERE review_status='已通过' "
        "GROUP BY course_chapter")}
    types = {r[0]: r[1] for r in conn.execute(
        "SELECT question_type, COUNT(*) FROM questions WHERE review_status='已通过' "
        "GROUP BY question_type")}
    conn.close()
    have_total = sum(cur.values())

    print("=" * 66)
    print(f"题库覆盖对照（依据公开考试大纲权重）　目标总量：{a.total} 道　当前：{have_total} 道")
    print("=" * 66)
    print(f"{'章节':<32}{'大纲权重':>8}{'应有':>6}{'现有':>6}{'缺口':>6}")
    plans = []
    for ch, w in SYLLABUS_WEIGHTS.items():
        should = round(a.total * w)
        have = cur.get(ch, 0)
        gap = max(0, should - have)
        flag = "  ← 缺得多" if gap >= 20 else ("  ← 缺" if gap > 0 else "  ✓")
        print(f"{ch:<32}{w*100:>7.0f}%{should:>6}{have:>6}{gap:>6}{flag}")
        if gap > 0:
            plans.append((ch, gap))

    print()
    print("题型分布（按老师给的历年真题卷面 vs 我们现有）")
    subj_total = sum(types.get(t, 0) for t in TYPE_TARGET)
    for t, w in TYPE_TARGET.items():
        have = types.get(t, 0)
        should = round(a.total * w)
        gap = max(0, should - have)
        note = f"   ← 缺 {gap} 道" if gap > 0 else "   ✓"
        print(f"  {t:<6} 真题占分{w*100:>3.0f}%（{TYPE_NOTE[t]}）  应有{should:>4}  现有{have:>4}{note}")
    print(f"  {'单选':<6} 真题里没有这个题型；现有 {types.get('单选',0)} 道"
          f"（可保留做快速自测，但不该当主力）")

    if a.plan and plans:
        print()
        print("=" * 66)
        print("下一批出题命令（按缺口从大到小，复制到命令行依次执行）")
        print("=" * 66)
        for ch, gap in sorted(plans, key=lambda x: -x[1]):
            # 概念题:计算题 按 55:45 拆
            n_con = max(1, round(gap * 0.55))
            n_cal = gap - n_con
            if n_con:
                print(f"python pipeline/generate_questions.py --chapter {ch} "
                      f"--limit {n_con} --type concept --provider {a.provider}")
            if n_cal:
                print(f"python pipeline/generate_questions.py --chapter {ch} "
                      f"--limit {n_cal} --type computation --provider {a.provider}")
        print()
        print("出完后跑自动审核：")
        print(f"python review/auto_review.py --provider {a.provider}")


if __name__ == "__main__":
    main()
