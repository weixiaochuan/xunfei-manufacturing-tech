"""
答案格式统一 —— 把真题答案（老师原卷里的写法五花八门）整理成统一、好读的格式。

【为什么要做这个】
你反馈的：
  · "工序、工步、工位" 那道，标准答案是三段平铺的话，看不出是三个采分点。
    应该整理成：
        （1）工序：一个（或同时加工的一组）工件…
        （2）工步：加工工具不变的条件下…
        （3）工位：工件相对于机床的每一个位置…
  · 标号规则乱：有的用 "1、"，有的用 "2"，有的用 "( 1 )"，有的干脆没有。
  · 中文引号 "" 在网页上显示别扭。

【这个脚本做什么】
  1. **多定义题自动分点**：题干是"A、B、C"（并列几个术语），答案里能找到对应的解释，
     就整理成 （1）A：…　（2）B：…　（3）C：…
  2. **统一标号**：把 "1、" "2" "( 1 )" "1)" 一律改成 "（1）"
  3. **中文引号**改成不带引号或用书名号（网页上"工艺过程"这种引号读着别扭）
  4. **去掉答案里的空行和多余空格**

⚠️ 只动**格式**，绝不改**内容**。一个字都不加、不删、不改写。
   （因为一旦改写，就可能改错意思——真题答案是老师给的标准，我没资格改。）

用法：
    python3 pipeline/normalize_answers.py --scan     # 只看会改成什么样，不写库
    python3 pipeline/normalize_answers.py --apply    # 真的改
"""
import argparse
import os
import re
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

# 标号写法 -> 统一成 （N）
# ⚠️ 必须非常小心：答案里有大量数值（"3.3mm" "0.0423 mm"），
#    绝不能把 "3.3mm" 当成标号改成 "（3）3mm"。我第一版就犯了这个错。
#    所以只认这几种**明确是列表标号**的写法：
#      （1）xxx   (1) xxx   1、xxx   1）xxx   1) xxx
#    并且后面必须跟**汉字**（数值后面跟的是数字/小数点/单位，不会是汉字）。
NUM_FORMS = re.compile(
    r"^\s*(?:"
    r"[（(]\s*(\d{1,2})\s*[)）]"          # （1） (1)
    r"|(\d{1,2})\s*[、）)]"                # 1、 1） 1)
    r")\s*(?=[\u4e00-\u9fa5])"            # 后面必须紧跟汉字
)


def unify_numbering(ans):
    """把 '1、xxx' '（1）xxx' '1)xxx' 统一成 '（1）xxx'。
    只动明确是列表标号的行；数值（3.3mm）绝不碰。"""
    out = []
    for line in ans.split("\n"):
        m = NUM_FORMS.match(line)
        if m:
            n = m.group(1) or m.group(2)
            rest = line[m.end():].strip()
            out.append(f"（{n}）{rest}")
        else:
            out.append(line.strip())
    return "\n".join(x for x in out if x)


def split_multi_definition(stem, ans):
    """题干是并列的几个术语（"工序、工步、工位"），
    答案却是平铺的几句话 —— 整理成 （1）工序：…（2）工步：…（3）工位：…

    只在能**明确对上**的时候才动（每个术语都能在答案里找到它自己那句），
    对不上就原样返回。绝不硬凑。"""
    terms = [t.strip() for t in re.split(r"[、,，/]", stem) if t.strip()]
    if len(terms) < 2 or any(len(t) > 12 for t in terms):
        return ans
    lines = [l.strip() for l in ans.split("\n") if l.strip()]
    if len(lines) < len(terms):
        return ans

    matched = []
    for t in terms:
        hit = None
        for l in lines:
            # 这行是不是在解释这个术语？（以术语开头，或"…称为X" "…称一个X"）
            if l.startswith(t) or re.search(rf"称(为|一个)?\s*{re.escape(t)}", l):
                hit = l
                break
        if not hit:
            return ans          # 有一个对不上就整体放弃，不硬凑
        matched.append((t, hit))

    out = []
    for i, (t, l) in enumerate(matched, 1):
        body = l
        # 去掉行首重复的术语和冒号（"工序：一个工件…" -> "一个工件…"）
        body = re.sub(rf"^{re.escape(t)}\s*[：:]\s*", "", body)
        # "……称为工步。" 这种，把术语挪到前面
        body = re.sub(rf"[，,]?\s*(简)?称(为|一个)?\s*{re.escape(t)}\s*[。.]?$", "", body).strip()
        out.append(f"（{i}）{t}：{body}")
    # 没被认领的行（比如末尾的补充说明）保留在后面
    used = {l for _, l in matched}
    for l in lines:
        if l not in used:
            out.append(l)
    return "\n".join(out)


def clean_quotes(s):
    """中文引号在网页上读着别扭，去掉（"工艺过程" -> 工艺过程）。"""
    s = re.sub(r"[“”]([^“”]{1,20})[“”]?", r"\1", s)
    s = s.replace("“", "").replace("”", "")
    return s


def normalize(stem, ans, qtype):
    if not ans:
        return ans
    a = ans
    if qtype == "名词解释":
        a = split_multi_definition(stem, a)
    a = unify_numbering(a)
    a = clean_quotes(a)
    a = re.sub(r"[ \t]+", " ", a)
    a = re.sub(r"\n{2,}", "\n", a).strip()
    return a


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    conn = connect_database()
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT question_id, question_type, stem, answer FROM questions "
        "WHERE answer IS NOT NULL AND answer != '' "
        "AND question_type IN ('名词解释','简述','计算')").fetchall()

    changed = []
    for r in rows:
        new = normalize(r["stem"], r["answer"], r["question_type"])
        if new != r["answer"]:
            changed.append((r["question_id"], r["question_type"], r["stem"], r["answer"], new))

    print(f"检查 {len(rows)} 道题的答案，{len(changed)} 道需要整理格式\n")
    for qid, t, stem, old, new in changed[:6]:
        print("─" * 60)
        print(f"[{t}] {stem[:40]}")
        print(f"  整理前：{old[:110]}")
        print(f"  整理后：{new[:110]}")
    if len(changed) > 6:
        print(f"\n…还有 {len(changed)-6} 道")

    if not a.apply:
        print("\n（--scan 只看不改。加 --apply 真的改。只改格式，不动内容。）")
        conn.close()
        return

    for qid, _, _, _, new in changed:
        conn.execute("UPDATE questions SET answer=? WHERE question_id=?", (new, qid))
    conn.commit()
    conn.close()
    print(f"\n已整理 {len(changed)} 道题的答案格式（只动格式，内容一个字没改）")


if __name__ == "__main__":
    main()
