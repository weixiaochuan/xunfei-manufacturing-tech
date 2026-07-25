"""
从《习题与答案》PDF 里导题 —— 这是目前最干净的一个题源。

【为什么它好】
· **PDF 里的文字是完整的**（φ120H7、公差符号都在），不像 .doc 那样公式变成图丢掉
· 题目和答案**分两个文件**，靠题号（1-1、2-3、4-2）一一对应，对得上
· 7 章 × (习题 + 答案)，正好和我们的 7 章知识库对得上
· 一共 405 处答案标记

【我之前搞错了，得说清楚】
我第一次看的时候只翻了第5章，看到"答案"文件里也是一堆题目，就下结论说"答案提取不出来"。
**是我看得太草率。** 实际上答案在文件后半部分，格式很规整：
    1-1答案：③ 要素
    3-2 答案： ×
    4-1答案：
    要点：批量法则仍适用
    1） 多品种、中小批量生产是指产品而言……

【题型】
· 单项选择 / 多项选择 -> 选择题（有选项、有正确答案）
· 判断题 -> 判断题（∨ / ×）
· 填空题 -> 跳过（我们的界面不支持填空，而且填空题考试也不考）
· 分析题 / 分析计算题 -> 简述题 / 计算题（有"要点：…"的标准答案）

用法：
    python3 pipeline/import_exercise_pdf.py --scan     # 看能收多少
    python3 pipeline/import_exercise_pdf.py --apply    # 写库
"""
import argparse
import glob
import hashlib
import json
import os
import re
import sqlite3
from collections import Counter

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()
PDF_DIR = os.path.join(BASE_DIR, "data", "real_exams", "exercise_pdf")

CHAPTER_MAP = {
    1: "第一章_绪论",
    2: "第三章_机床夹具设计",              # "定位原理" 对应我们的夹具/定位章
    3: "第一章_绪论",                     # "切削原理" 归到绪论（我们的知识库里切削在第一章）
    4: "第四章_机械加工精度及其控制",
    5: "第二章_机械加工工艺规程设计",
    6: "第六章_机器装配工艺过程设计",
    7: "第七章_机械制造工艺理论和技术的发展",
}

# 分节标题
SEC_RE = re.compile(r"^\s*\d\.\s*(单项选择|多项选择|判断题|填空题|分析题|分析计算题|计算题|简答题)")
# 题号：1-1 / 2-10
QNO_RE = re.compile(r"^\s*(\d+)-(\d+)\s*(.*)$")
# 答案行：1-1答案：③ 要素     /    3-2 答案： ×
ANS_RE = re.compile(r"^\s*(\d+)-(\d+)\s*答案\s*[：:]\s*(.*)$")
# 选项行：① 铸件 ② 锻件 ③ 棒料 ④ 管材
OPT_RE = re.compile(r"[①②③④⑤]")
OPT_NUM = {"①": 0, "②": 1, "③": 2, "④": 3, "⑤": 4}


def read_pdf(path):
    import pdfplumber
    with pdfplumber.open(path) as pdf:
        return "\n".join(p.extract_text() or "" for p in pdf.pages)


IMG_DIR = os.path.join(BASE_DIR, "assets", "images", "exercise")
# 注意：题注后面常紧跟正文（"习图2-4-1所示为…"），
# 如果写成 (\d+) 会把后面的数字也吞进来，变成 2-4-14。所以第三段限定 1~2 位且后面不能再是数字。
FIGCAP_RE = re.compile(r"习图\s*(\d+)\s*-\s*(\d+)\s*-\s*(\d{1,2})(?!\d)")


def extract_figures(path, chapter_no):
    """把习题 PDF 里的插图切出来。

    这些图是**矢量图**（用线条画的），不是嵌入的位图，所以不能直接"抽图片"，
    只能**按位置把那块页面区域裁下来渲染成 PNG**。
    定位方法：找"习图 2-4-1"这样的题注，题注**上方**那一片有大量线条的区域就是图。

    返回 {"2-4-1": "assets/images/exercise/xxx.png", ...}
    """
    import pdfplumber
    os.makedirs(IMG_DIR, exist_ok=True)
    out = {}
    with pdfplumber.open(path) as pdf:
        for page in pdf.pages:
            words = page.extract_words()
            # 找题注。注意：pdfplumber 会把"习图2-4-1"和后面的正文拆成好几个 word，
            # 直接把相邻 word 拼起来会把正文里的数字也粘进来（"2-4-1" 变成 "2-4-14"）。
            # 所以只拼**紧挨着的、同一行的**几个 word，并且遇到非编号字符就停。
            caps = []
            for i, w in enumerate(words):
                if "习图" not in w["text"]:
                    continue
                buf = w["text"]
                for x in words[i + 1:i + 4]:
                    if abs(x["top"] - w["top"]) > 3:      # 不同行，不拼
                        break
                    buf += x["text"]
                    if len(buf) > 24:
                        break
                m = FIGCAP_RE.search(buf)
                if m:
                    caps.append((f"{m.group(1)}-{m.group(2)}-{m.group(3)}",
                                 w["top"], w["bottom"]))
            if not caps:
                continue
            shapes = page.curves + page.lines + page.rects
            for key, top, bottom in caps:
                # 题注正上方那一片图形（往上最多 330pt）
                band = [s_ for s_ in shapes
                        if s_["bottom"] <= top + 2 and s_["top"] >= top - 330]
                if len(band) < 6:            # 线条太少，不像是图
                    continue
                y0 = max(0, min(s_["top"] for s_ in band) - 6)
                y1 = min(page.height, bottom + 4)
                # 左右也按图形的实际范围裁（别把整页宽度和正文框进来）
                x0 = max(0, min(s_["x0"] for s_ in band) - 10)
                x1 = min(page.width, max(s_["x1"] for s_ in band) + 10)
                if y1 - y0 < 40 or x1 - x0 < 60:
                    continue
                try:
                    im = page.crop((x0, y0, x1, y1)).to_image(resolution=170)
                    fn = f"ch{chapter_no}_{key.replace('-', '_')}.png"
                    fp = os.path.join(IMG_DIR, fn)
                    im.save(fp)
                    out[key] = os.path.relpath(fp, BASE_DIR).replace("\\", "/")
                except Exception:
                    continue
    return out


def parse_questions(text):
    """习题文件 -> {题号: {sec, stem, options}}"""
    out, sec, cur = {}, None, None
    for line in text.split("\n"):
        line = line.rstrip()
        if not line:
            continue
        m = SEC_RE.match(line)
        if m:
            sec = m.group(1)
            cur = None
            continue
        m = QNO_RE.match(line)
        if m and sec:
            key = f"{m.group(1)}-{m.group(2)}"
            cur = {"sec": sec, "stem": m.group(3).strip(), "options": []}
            out[key] = cur
            continue
        if cur is None:
            continue
        # 选项行
        if OPT_RE.search(line):
            parts = re.split(r"[①②③④⑤]", line)
            marks = OPT_RE.findall(line)
            for mk, txt in zip(marks, parts[1:]):
                t = txt.strip()
                if t:
                    cur["options"].append((mk, t))
        else:
            cur["stem"] += " " + line.strip()
    return out


def parse_answers(text):
    """答案文件 -> {题号: 答案文本}"""
    out, key, buf = {}, None, []
    for line in text.split("\n"):
        line = line.rstrip()
        if not line:
            continue
        m = ANS_RE.match(line)
        if m:
            if key:
                out[key] = "\n".join(buf).strip()
            key = f"{m.group(1)}-{m.group(2)}"
            buf = [m.group(3).strip()]
            continue
        if SEC_RE.match(line):
            if key:
                out[key] = "\n".join(buf).strip()
            key, buf = None, []
            continue
        if key:
            buf.append(line.strip())
    if key:
        out[key] = "\n".join(buf).strip()
    return {k: v for k, v in out.items() if v}


def build(chapter_no, qs, ans, figs=None):
    """把题目和答案配对，产出可入库的题。figs: {"2-4-1": 图片路径}"""
    figs = figs or {}
    res = []
    course_chapter = CHAPTER_MAP[chapter_no]
    for key, q in qs.items():
        a = ans.get(key)
        if not a:
            continue
        sec = q["sec"]
        stem = re.sub(r"\s{2,}", " ", q["stem"]).strip()
        if len(stem) < 8:
            continue

        if sec == "填空题":
            continue        # 界面不支持，考试也不考，跳过

        if sec in ("单项选择", "多项选择"):
            if len(q["options"]) < 2:
                continue
            # 答案形如 "③ 要素" 或 "① 生产对象 ② 生产资料"
            picks = OPT_RE.findall(a)
            if not picks:
                continue
            if sec == "单项选择" and len(picks) != 1:
                continue
            if sec == "多项选择" and len(picks) < 2:
                continue
            correct = []
            for p in picks:
                i = OPT_NUM.get(p)
                if i is not None and i < len(q["options"]):
                    correct.append(q["options"][i][1])
            if not correct:
                continue
            opts = [{"key": chr(65 + i), "text": t,
                     "is_correct": t in correct} for i, (_, t) in enumerate(q["options"])]
            res.append({"type": "单选" if sec == "单项选择" else "多选",
                        "stem": stem, "options": opts,
                        "answer": "；".join(correct),
                        "chapter": course_chapter, "total_score": 2, "key": key})

        elif sec == "判断题":
            # 判断题转成"两个选项的单选"——这样现有界面直接就能做，不用新写一套。
            v = "正确" if ("∨" in a or "√" in a or "对" in a) else "错误"
            opts = [{"key": "A", "text": "正确", "is_correct": v == "正确"},
                    {"key": "B", "text": "错误", "is_correct": v == "错误"}]
            res.append({"type": "单选", "stem": "（判断）" + stem, "options": opts,
                        "answer": v, "chapter": course_chapter,
                        "total_score": 2, "key": key})

        else:   # 分析题 / 计算题 / 简答题
            body = re.sub(r"^要点\s*[：:]\s*", "", a).strip()
            if len(body) < 15:
                continue
            qtype = "计算" if re.search(r"计算|求|尺寸链|工序尺寸|公差", stem) else "简述"
            # 题干里提到"习图 X-X-X" -> 把那张图挂上
            img = None
            mm = FIGCAP_RE.search(stem)
            if mm:
                img = figs.get(f"{mm.group(1)}-{mm.group(2)}-{mm.group(3)}")
            # 题干要看图但图没切到 -> 不要（残题不给学生）
            if re.search(r"习图|如图|下图|所示", stem) and not img:
                continue
            res.append({"type": qtype, "stem": stem, "options": None,
                        "answer": body, "chapter": course_chapter,
                        "total_score": 10, "key": key, "image": img})
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    if not os.path.isdir(PDF_DIR):
        print(f"❌ 没找到 {PDF_DIR}")
        return

    all_q = []
    for ch in range(1, 8):
        qf = os.path.join(PDF_DIR, f"ch{ch}.pdf")
        af = os.path.join(PDF_DIR, f"ch{ch}_ans.pdf")
        if not (os.path.exists(qf) and os.path.exists(af)):
            print(f"第{ch}章：缺文件，跳过")
            continue
        qs = parse_questions(read_pdf(qf))
        ans = parse_answers(read_pdf(af))
        figs = extract_figures(qf, ch)
        built = build(ch, qs, ans, figs)
        print(f"第{ch}章：习题 {len(qs)} 道，答案 {len(ans)} 条 -> 配上对的 {len(built)} 道 "
              f"({dict(Counter(x['type'] for x in built))})")
        all_q.extend(built)

    print(f"\n合计可用 {len(all_q)} 道")
    print("题型：", dict(Counter(x["type"] for x in all_q)))
    print("章节：", dict(Counter(x["chapter"] for x in all_q)))

    print("\n样例：")
    for x in all_q[:4]:
        print(f"\n  [{x['type']}] {x['stem'][:60]}")
        if x["options"]:
            for o in x["options"]:
                print(f"      {'✔' if o['is_correct'] else ' '} {o['key']}. {o['text'][:30]}")
        print(f"      答案：{x['answer'][:60]}")

    if not a.apply:
        print("\n（--scan 只看。加 --apply 写库。）")
        return

    conn = connect_database()
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    for c, ddl in [("usage_scope", "TEXT DEFAULT '学生练习'"), ("exam_source", "TEXT"),
                   ("total_score", "INTEGER"), ("source", "TEXT DEFAULT 'AI生成'")]:
        if c not in cols:
            conn.execute(f"ALTER TABLE questions ADD COLUMN {c} {ddl}")

    old = conn.execute("SELECT COUNT(*) FROM questions WHERE question_id LIKE 'X_%'").fetchone()[0]
    conn.execute("DELETE FROM question_knowledge_map WHERE question_id LIKE 'X_%'")
    conn.execute("DELETE FROM questions WHERE question_id LIKE 'X_%'")
    if old:
        print(f"\n清掉上一版的 {old} 道")

    chap_node = {}
    for (ch,) in conn.execute("SELECT DISTINCT chapter FROM knowledge_points"):
        r = conn.execute("SELECT knowledge_id FROM knowledge_points WHERE chapter=? "
                         "ORDER BY learning_order LIMIT 1", (ch,)).fetchone()
        if r:
            chap_node[ch] = r[0]

    ins = 0
    for q in all_q:
        node = chap_node.get(q["chapter"])
        if not node:
            continue
        qid = "X_" + hashlib.md5(q["stem"][:60].encode()).hexdigest()[:10]
        if conn.execute("SELECT 1 FROM questions WHERE question_id=?", (qid,)).fetchone():
            continue
        conn.execute(
            """INSERT INTO questions
               (question_id, course_chapter, source_node_id, question_type, stem,
                options_json, answer, explanation, bloom_level, generation_model,
                prompt_template_id, review_status, total_score, source, usage_scope,
                exam_source, image_path, image_reviewed)
               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
            (qid, q["chapter"], node, q["type"], q["stem"],
             json.dumps(q["options"], ensure_ascii=False) if q["options"] else None,
             q["answer"], None, None, "教材习题集", "exercise_pdf", "已通过",
             q["total_score"], "教材习题", "学生练习",
             "《机械制造技术基础》习题集|《机械制造技术基础》配套习题与答案",
             q.get("image"), 1 if q.get("image") else 0))
        conn.execute("INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) "
                     "VALUES (?,?)", (qid, node))
        ins += 1
    conn.commit()
    conn.close()
    print(f"\n已写入 {ins} 道教材习题")


if __name__ == "__main__":
    main()
