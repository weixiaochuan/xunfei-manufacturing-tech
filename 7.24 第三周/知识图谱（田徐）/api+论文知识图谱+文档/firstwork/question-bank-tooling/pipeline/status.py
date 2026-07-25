"""
看一眼题库现在什么样 —— 回答你那个最实在的问题：
"我跑了那些命令，题到底存进去没有？存哪了？"

一句话解释这套东西怎么运作的：
    **所有业务数据都存在 qbctl.py 明确指定的开发数据库中。**
    你跑的每一条命令（出题、导真题、审核、标难度），都是在往这个文件里写东西。
    网页（demo/serve_quiz.py）只是把这个文件里的内容显示出来。
    所以——
      · 交付前先按 README_FIRSTWORK.md 完成审核和发布预检，不要发送包含密钥的本地配置。
      · 想知道跑完命令有没有生效，就跑这个脚本看一眼。

用法：
    python3 pipeline/status.py              # 看题库现状
    python3 pipeline/status.py --diff       # 和上次看的时候比，多了哪些题（跑完命令用这个）
"""
import argparse
import json
import os
import sqlite3
from collections import Counter

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()
SNAP = os.path.join(BASE_DIR, "db", ".last_status.json")


def snapshot(conn):
    rows = conn.execute(
        "SELECT question_type, review_status, COALESCE(source,'AI生成') s, "
        "COALESCE(usage_scope,'学生练习') u, COUNT(*) n "
        "FROM questions GROUP BY question_type, review_status, s, u").fetchall()
    return {f"{a}|{b}|{c}|{d}": n for a, b, c, d, n in rows}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--diff", action="store_true", help="和上次比，多了哪些题")
    a = ap.parse_args()

    if not os.path.exists(DB_PATH):
        print(f"❌ 找不到题库文件：{DB_PATH}")
        return
    conn = connect_database()
    conn.row_factory = sqlite3.Row

    print("=" * 60)
    print(f"题库文件：{DB_PATH}")
    print(f"文件大小：{os.path.getsize(DB_PATH)/1024/1024:.1f} MB")
    print("=" * 60)

    total = conn.execute("SELECT COUNT(*) FROM questions").fetchone()[0]
    print(f"\n【总题数】{total} 道（包含所有审核状态）\n")

    # 1. 学生现在能练到的题
    print("─" * 60)
    print("① 学生现在能练到的题（已通过审核 + 有标准答案）")
    print("─" * 60)
    rows = conn.execute(
        "SELECT COALESCE(source,'AI生成') s, question_type t, COUNT(*) n FROM questions "
        "WHERE review_status='已通过' AND COALESCE(usage_scope,'学生练习')='学生练习' "
        "GROUP BY s, t ORDER BY s DESC, n DESC").fetchall()
    stu = 0
    for r in rows:
        print(f"   {r['s']:<8} {r['t']:<8} {r['n']:>4} 道")
        stu += r["n"]
    print(f"   {'合计':<17} {stu:>4} 道  ← 这就是学生打开网页能看到的题数")

    # 2. 只给老师出题用的
    n_t = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE usage_scope='教师出题'").fetchone()[0]
    if n_t:
        print("\n" + "─" * 60)
        print("② 只给老师出题用的（历年真题里没有标准答案的）")
        print("─" * 60)
        for r in conn.execute(
                "SELECT question_type t, COUNT(*) n FROM questions "
                "WHERE usage_scope='教师出题' GROUP BY t"):
            print(f"   {r['t']:<8} {r['n']:>4} 道")
        print(f"   合计 {n_t} 道　（学生端看不到，只在教师端「出题素材」标签页）")

    # 3. 还没过审的
    print("\n" + "─" * 60)
    print("③ 还没进学生题库的（审核没通过 / 待审）")
    print("─" * 60)
    rows = conn.execute(
        "SELECT review_status r, question_type t, COUNT(*) n FROM questions "
        "WHERE review_status != '已通过' GROUP BY r, t ORDER BY r").fetchall()
    if rows:
        for r in rows:
            print(f"   {r['r']:<8} {r['t']:<8} {r['n']:>4} 道")
        print("\n   说明：'已驳回'=质检没过（有硬伤）；'待复核'/'待审核'=还没审。")
        print("        跑 `python3 review/auto_review.py --provider deepseek` 可以自动审。")
    else:
        print("   （没有）")

    # 4. 配套情况
    print("\n" + "─" * 60)
    print("④ 配套情况")
    print("─" * 60)
    img = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE image_path IS NOT NULL AND image_path!=''").fetchone()[0]
    rub = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE rubric_json IS NOT NULL AND rubric_json NOT IN ('','[]')").fetchone()[0]
    cal = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE irt_difficulty_b IS NOT NULL").fetchone()[0]
    ans = conn.execute("SELECT COUNT(*) FROM student_answers").fetchone()[0]
    print(f"   带配图：      {img:>4} 道")
    print(f"   带采分点：    {rub:>4} 道")
    print(f"   标过难度：    {cal:>4} 道　（自适应推题需要这个）")
    print(f"   学生答题记录：{ans:>4} 条")

    # diff
    cur = snapshot(conn)
    if a.diff and os.path.exists(SNAP):
        old = json.load(open(SNAP, encoding="utf-8"))
        delta = {k: cur.get(k, 0) - old.get(k, 0) for k in set(cur) | set(old)}
        delta = {k: v for k, v in delta.items() if v}
        print("\n" + "=" * 60)
        if delta:
            print("和你上次看的时候比，变化：")
            for k, v in sorted(delta.items(), key=lambda x: -abs(x[1])):
                t, r, s, u = k.split("|")
                sign = "+" if v > 0 else ""
                print(f"   {sign}{v:>4}  {s} 的 {t}（{r}，{u}）")
        else:
            print("和上次比：没有变化（说明刚才那些命令没往库里加东西）")
        print("=" * 60)
    with open(SNAP, "w", encoding="utf-8") as f:
        json.dump(cur, f)

    print("\n💡 提示：")
    print("   · 当前统计只针对上方明确显示的开发数据库。")
    print("   · 正式发布请使用 tools/publish_student_bank.py 先做只读预检。")
    print("   · 每次跑完出题命令，跑 `python3 pipeline/status.py --diff` 就知道加了多少题。")
    conn.close()


if __name__ == "__main__":
    main()
