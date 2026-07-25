"""
去重 —— 这次做成一个**独立命令**，每次导完题都跑一遍，不再让重复题漏出去。

【你反复反馈这个，我一直没根治，原因说清楚】
1. 我的去重是**写在导入脚本里**的，只在"导入真题"那一步跑。
   但你后来又跑了出题命令、又换了数据库，重复题就又冒出来了。
   **应该做成一个独立的、每次都能跑的命令。** 这次改了。

2. 指纹算法有漏洞。你截的那三道"环形零件铣缺口"：
       "在**题图10**所示的环形零件上铣一缺口…采用定位方案（b）"
       "在**图**所示的环形零件上铣一缺口…采用定位方案（b）"     <- 和上面是同一道题
       "在**图**所示的环形零件上铣一缺口…采用定位方案（d）"     <- 这道**不一样**（方案d≠方案b）
   我的指纹把"题图10"里的"10"也算进去了，所以前两道被当成两道题。

   修法：**先把"题图10""图3-5""如下图"这类图号引用统一抹掉**再算指纹。
   但**（a）（b）（c）（d）这种小问编号必须保留**——它们是真正的区别。

【去重优先级】
   有答案 > 没答案；真题 > 教材习题 > AI生成；有采分点 > 没有；有图 > 没有；答案长 > 答案短

用法：
    python3 pipeline/dedup.py --scan      # 只看会删哪些
    python3 pipeline/dedup.py --apply     # 真的删
"""
import argparse
import os
import re
import sqlite3
from collections import defaultdict

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

SOURCE_PRIORITY = {"真题": 0, "教材习题": 1, "AI生成": 2}


def fingerprint(stem):
    """题干指纹。相同 = 同一道题。

    要能认出这些是**同一道题**：
      · "在题图10所示的环形零件…" == "在图所示的环形零件…"   （图号引用不算区别）
      · "工序、工步、工位" == "工序、工位、工步"              （并列词换序不算区别）
      · "什么是粗基准？简述…" == "什么是粗基准，请说明…"       （问法不算区别）
    但要能区分出这些是**不同的题**：
      · "…采用定位方案（b）" != "…采用定位方案（d）"          （小问编号是真区别！）
    """
    t = stem or ""

    # ① 抹掉图号引用（"题图10""习图2-4-1""图3-5""如下图""如右图"），它们不是题目内容的区别
    t = re.sub(r"(题图|习图|附图|图)\s*[\d\-－—\.]*", "图", t)
    t = re.sub(r"如[左右上下]?图|下图|上图|右图|左图", "图", t)

    # ② 保护小问编号（a)(b)(c)(d) —— 这是真正的区别，不能抹！
    marks = re.findall(r"[（(]\s*([a-dA-D])\s*[)）]", t)
    keep = "".join(sorted(m.lower() for m in marks))

    # ③ 去掉标点、问法词
    t = re.sub(r"[，。、？?：:；;（）()\s\.,\-－—]", "", t)
    t = re.sub(r"(什么是|简述|请说明|试说明|论述|请简要论述|试分析|试计算|计算|求|判断|并)", "", t)

    # ④ 字符排序（吸收并列词换序）
    return keep + "|" + "".join(sorted(t))


def score(r):
    """越小越该保留。"""
    has_ans = 0 if (r["usage_scope"] or "学生练习") == "学生练习" else 1
    src = SOURCE_PRIORITY.get(r["source"] or "AI生成", 9)
    has_rub = 0 if (r["rubric_json"] and r["rubric_json"] not in ("", "[]")) else 1
    has_img = 0 if r["image_path"] else 1
    return (has_ans, src, has_rub, has_img, -len(r["answer"] or ""))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    conn = connect_database()
    conn.row_factory = sqlite3.Row
    rows = [dict(r) for r in conn.execute(
        "SELECT question_id, stem, source, usage_scope, rubric_json, image_path, answer, "
        "question_type FROM questions")]

    groups = defaultdict(list)
    for r in rows:
        groups[fingerprint(r["stem"])].append(r)

    dups = {k: v for k, v in groups.items() if len(v) > 1}
    kill = []
    for k, v in dups.items():
        v.sort(key=score)
        keep = v[0]
        for r in v[1:]:
            kill.append((r, keep))

    print(f"题库共 {len(rows)} 道")
    print(f"重复组 {len(dups)} 组，需要删掉 {len(kill)} 道\n")

    for r, keep in kill[:10]:
        print(f"  删 [{r['source']}·{r['usage_scope']}] {r['stem'][:52]}")
        print(f"  留 [{keep['source']}·{keep['usage_scope']}] {keep['stem'][:52]}")
        print()
    if len(kill) > 10:
        print(f"  …还有 {len(kill)-10} 道\n")

    if not a.apply:
        print("（--scan 只看。加 --apply 真的删。）")
        conn.close()
        return

    for r, _ in kill:
        conn.execute("DELETE FROM question_knowledge_map WHERE question_id=?", (r["question_id"],))
        conn.execute("DELETE FROM student_answers WHERE question_id=?", (r["question_id"],))
        conn.execute("DELETE FROM questions WHERE question_id=?", (r["question_id"],))
    conn.commit()

    # 复查
    rows = [dict(r) for r in conn.execute("SELECT question_id, stem FROM questions")]
    g2 = defaultdict(int)
    for r in rows:
        g2[fingerprint(r["stem"])] += 1
    left = sum(1 for v in g2.values() if v > 1)
    conn.close()
    print(f"已删除 {len(kill)} 道重复题")
    print(f"复查：还剩 {left} 组重复（应为 0）")


if __name__ == "__main__":
    main()
