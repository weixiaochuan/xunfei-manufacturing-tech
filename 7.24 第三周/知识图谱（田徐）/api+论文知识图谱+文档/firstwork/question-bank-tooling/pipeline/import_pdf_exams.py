"""
从你转好的 PDF 里救回计算真题。

【为什么必须用 PDF】
.doc 里公式是 MathType 图片对象，读不到，题干变成 "计算工序尺寸 。"（数值没了）。
PDF 里公式是**渲染好的**，所以：
  · 数字回来了：A₁ = 34.25，上偏差 +0.05，下偏差 -0.09
  · 尺寸链图、零件图也都在

【但有个新问题：PDF 的文字提取会把版面打散】
PDF 里的上标下标（A₁ 的那个 1）、公式里的分子分母，提取出来会跑到**单独的行**上：
    "。其工序图如下图所示。计算工序尺寸"
    "3"
    "A"
    "1 。（20分）"
拼回去是不可能的——那是排版信息，不是文字信息。

【所以这个脚本的做法：把整道题**截图**】
既然文字拼不回来，就**把这道题在卷子上的那一整块（题干+图+公式+答案）裁下来存成图片**，
学生看到的就是**原卷的样子**——公式、尺寸链、零件图，一个不缺。
文字部分（能提取干净的那些）作为题干存着，方便搜索和分类。

这样做的好处：
  · 学生看到的和真实考卷一模一样（这本来就是真题）
  · 不用去猜那些丢掉的符号是什么 —— 不猜，就不会猜错

用法：
    python3 pipeline/import_pdf_exams.py --scan
    python3 pipeline/import_pdf_exams.py --apply
"""
import argparse
import glob
import hashlib
import os
import re
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()
PDF_DIR = os.path.join(BASE_DIR, "data", "real_exams", "pdf_converted")
IMG_DIR = os.path.join(BASE_DIR, "assets", "images", "pdf_exams")

# 题号有两种写法（我第一版只认中文数字，所以漏掉了绝大多数计算题）：
#   · 中文数字："六．在轴上加工键槽…（20分）"
#   · 阿拉伯数字："12．（20分）加工一批齿轮内孔…"
SEC_RE = re.compile(
    r"^\s*(?:([一二三四五六七八九十]{1,2})|(\d{1,2}))\s*[．.、]\s*(.+)$")
SCORE_RE = re.compile(r"[（(]\s*(\d{1,2})\s*分\s*[)）]")
# 计算题的特征：必须**真的要算东西**。
# 第一版写得太松，"在机械加工中自激振动有哪些形式"这种简述题也被算成计算题了
# （因为正文里出现了"计算"两个字）。收紧：题干里必须有明确的计算指令。
CALC_HINT = re.compile(
    r"计算|试计算|求出|求解|尺寸链|定位误差|工序尺寸|加工余量"
    r"|生产纲领|合格率|废品率|正态分布|工艺能力|自由度")
# 明确不是计算题的（简述题的典型问法）
NOT_CALC = re.compile(
    r"有哪些形式|论述.{0,4}机理|因素有哪些|哪一项因素|如何定义|简述|什么是"
    r"|含义是什么|都有哪些方法|各试举一例|举例说明|请说明理由"
    r"|哪些是允许|请给出.{0,6}原则")
ANS_MARK = re.compile(r"^\s*(解|答|参考答案|解[:：])\s*$|^\s*解\s")


def clean_text(s):
    """PDF 提取的文字里，公式碎片会变成孤立的短行（"3" "A" "0.1"）。
    做题干用的话，把这些碎片去掉，留下能读的句子。"""
    keep = []
    for line in (s or "").split("\n"):
        t = line.strip()
        if not t:
            continue
        # 纯数字/单字母/纯符号的碎行 —— 是公式被打散的残片
        if len(t) <= 3 and not re.search(r"[\u4e00-\u9fa5]", t):
            continue
        if re.fullmatch(r"[\d\.\-\+±/×\s A-Za-z]{0,12}", t):
            continue
        keep.append(t)
    return " ".join(keep)


def extract_questions(path):
    """一份 PDF -> 每道大题。

    ⚠️ 一道题（题干 + 图 + 答案）**经常跨页**：题目在这一页底部，"解"和尺寸链在下一页。
    所以题块的范围要能跨页 —— 第一版只在单页里切，把答案切丢了。
    """
    import pdfplumber
    out = []
    with pdfplumber.open(path) as pdf:
        # 先把所有页的题号标记收集起来（记住它在第几页、什么位置）
        all_marks = []
        for pi, page in enumerate(pdf.pages):
            for w in page.extract_words():
                m = SEC_RE.match(w["text"])
                if m and len(w["text"]) > 8:
                    num = m.group(1) or m.group(2)
                    all_marks.append({"num": num, "head": m.group(3),
                                      "page": pi, "top": w["top"]})
        all_marks.sort(key=lambda x: (x["page"], x["top"]))

        for k, mk in enumerate(all_marks):
            nxt = all_marks[k + 1] if k + 1 < len(all_marks) else None
            pages = []      # 这道题占了哪几页的哪些区域
            if nxt is None:
                # 最后一道题：从这里到文档结束
                for pi in range(mk["page"], len(pdf.pages)):
                    y0 = mk["top"] - 6 if pi == mk["page"] else 0
                    pages.append((pi, max(0, y0), pdf.pages[pi].height))
            elif nxt["page"] == mk["page"]:
                pages.append((mk["page"], max(0, mk["top"] - 6), nxt["top"] - 2))
            else:
                # 跨页：本页从题号到页底 + 中间整页 + 末页到下一题
                pages.append((mk["page"], max(0, mk["top"] - 6),
                              pdf.pages[mk["page"]].height))
                for pi in range(mk["page"] + 1, nxt["page"]):
                    pages.append((pi, 0, pdf.pages[pi].height))
                if nxt["top"] > 20:
                    pages.append((nxt["page"], 0, nxt["top"] - 2))
            out.append({"num": mk["num"], "head": mk["head"],
                        "pages": pages, "path": path})
    return out


# 答案在原卷上的起始标记（"答题要点：" / "解" / "参考答案"）
ANS_START = re.compile(r"^\s*(答题要点|参考答案|标准答案|解答|解|答)\s*[：:]?\s*$")


def crop_question(path, q, q_png, a_png):
    """把这道题切成**两张图**：

    ⚠️ 这是用户抓到的一个严重问题：
       以前我把"题干 + 图 + 公式 + 答案"整块截成**一张图**贴在题干下面，
       结果**学生还没提交，答案就已经摆在眼前了**。这道题就废了。

    现在：
       · 【题干图】：从题号 -> "答题要点/解"之前   （做题时看这张）
       · 【答案图】：从"答题要点/解" -> 这道题结束  （提交之后才显示，含尺寸链图和公式）

    切分点：在 PDF 里找"答题要点："或"解"这一行的 y 坐标，从那里切开。
    """
    import pdfplumber
    from PIL import Image

    q_ims, a_ims, q_txt, a_txt = [], [], [], []
    in_answer = False

    with pdfplumber.open(path) as pdf:
        for pi, y0, y1 in q["pages"]:
            if y1 - y0 < 20:
                continue
            page = pdf.pages[pi]
            y0 = max(0, y0)
            y1 = min(page.height, y1)

            # 这一页里，答案是从哪一行开始的？
            split_y = None
            if not in_answer:
                for w in page.extract_words():
                    if w["top"] < y0 or w["bottom"] > y1:
                        continue
                    if ANS_START.match(w["text"].strip()):
                        split_y = w["top"] - 3
                        break

            def grab(a, b, bucket_im, bucket_tx):
                if b - a < 18:
                    return
                box = (0, a, page.width, b)
                try:
                    bucket_im.append(page.crop(box).to_image(resolution=150).original)
                    bucket_tx.append(page.crop(box).extract_text() or "")
                except Exception:
                    pass

            if in_answer:
                grab(y0, y1, a_ims, a_txt)
            elif split_y is not None and split_y > y0:
                grab(y0, split_y, q_ims, q_txt)      # 切分点之前 = 题干
                grab(split_y, y1, a_ims, a_txt)      # 切分点之后 = 答案
                in_answer = True
            else:
                grab(y0, y1, q_ims, q_txt)

    def stack(ims, out):
        if not ims:
            return False
        w = max(i.width for i in ims)
        h = sum(i.height for i in ims)
        if h > 6000 or h < 40:
            return False
        cv = Image.new("RGB", (w, h), "white")
        y = 0
        for i in ims:
            cv.paste(i, (0, y))
            y += i.height
        cv.save(out)
        return True

    ok_q = stack(q_ims, q_png)
    ok_a = stack(a_ims, a_png)
    return ok_q, ok_a, "\n".join(q_txt), "\n".join(a_txt)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    files = sorted(glob.glob(os.path.join(PDF_DIR, "*.pdf")))
    if not files:
        print(f"❌ 没找到 PDF。请把转好的 PDF 放到 {PDF_DIR}/")
        return
    print(f"扫描 {len(files)} 份转好的 PDF\n")
    os.makedirs(IMG_DIR, exist_ok=True)

    found = []
    for f in files:
        base = os.path.basename(f)
        try:
            qs = extract_questions(f)
        except Exception as e:
            print(f"  {base}: 读取失败 {e}")
            continue
        n_calc = 0
        for q in qs:
            key = hashlib.md5((base + str(q["num"]) + q["head"][:30]).encode()).hexdigest()[:10]
            q_png = os.path.join(IMG_DIR, f"q{key}.png")     # 题干图
            a_png = os.path.join(IMG_DIR, f"a{key}.png")     # 答案图（提交后才给看）
            ok_q, ok_a, q_txt, a_txt = crop_question(f, q, q_png, a_png)
            if not ok_q:
                continue
            txt = q_txt + "\n" + a_txt
            # 是不是计算题，要看**整块内容**。
            # 只看题号那一行是不行的——PDF 里题干常常被图片打断，
            # 题号行只有前半句（"六．在轴上加工键槽，设计尺寸如左图所示，相关工序如下"），
            # "计算工序尺寸"这几个字在后面几行，第一版因此漏掉了绝大多数计算题。
            head_line = q["head"][:60]
            if (not CALC_HINT.search(txt)) or NOT_CALC.search(head_line):
                for x in (q_png, a_png):
                    try:
                        os.remove(x)
                    except OSError:
                        pass
                continue
            sm = SCORE_RE.search(txt)
            score = int(sm.group(1)) if sm else 20
            stem = clean_text(SCORE_RE.sub("", q_txt))
            stem = re.sub(r"^[一二三四五六七八九十\d]{1,2}\s*[．.、]\s*", "", stem).strip()
            if len(stem) < 20:
                continue
            if not ok_a:
                # 没切到答案图 -> 这道题没有标准答案，不给学生（进教师素材）
                try:
                    os.remove(a_png)
                except OSError:
                    pass
            found.append({
                "file": base, "num": q["num"], "stem": stem,
                "image": os.path.relpath(q_png, BASE_DIR).replace("\\", "/"),
                "answer_image": os.path.relpath(a_png, BASE_DIR).replace("\\", "/") if ok_a else None,
                "score": score, "has_ans": ok_a,
            })
            n_calc += 1
        print(f"  {base}: 找到 {n_calc} 道计算题")

    print(f"\n合计 {len(found)} 道计算题（都带**原卷截图**：公式、尺寸链、零件图一个不缺）")
    print("\n样例：")
    for x in found[:3]:
        print(f"\n  [{x['score']}分] {x['stem'][:70]}")
        print(f"     截图：{x['image']}　有答案：{'是' if x['has_ans'] else '否'}")

    if not a.apply:
        print("\n（--scan 只看。加 --apply 写库。）")
        return

    conn = connect_database()
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    for c, ddl in [("usage_scope", "TEXT DEFAULT '学生练习'"), ("exam_source", "TEXT"),
                   ("answer_source", "TEXT")]:
        if c not in cols:
            conn.execute(f"ALTER TABLE questions ADD COLUMN {c} {ddl}")

    old = conn.execute("SELECT COUNT(*) FROM questions WHERE question_id LIKE 'P_%'").fetchone()[0]
    conn.execute("DELETE FROM question_knowledge_map WHERE question_id LIKE 'P_%'")
    conn.execute("DELETE FROM questions WHERE question_id LIKE 'P_%'")
    if old:
        print(f"\n清掉上一版的 {old} 道")

    chap_node = {}
    for (ch,) in conn.execute("SELECT DISTINCT chapter FROM knowledge_points"):
        r = conn.execute("SELECT knowledge_id FROM knowledge_points WHERE chapter=? "
                         "ORDER BY learning_order LIMIT 1", (ch,)).fetchone()
        if r:
            chap_node[ch] = r[0]

    def guess_chapter(t):
        if re.search(r"尺寸链|工序尺寸|工艺路线|基准", t):
            return "第二章_机械加工工艺规程设计"
        if re.search(r"定位误差|夹具|V形块|自由度|定位方案", t):
            return "第三章_机床夹具设计"
        if re.search(r"加工精度|正态分布|误差复映|刚度", t):
            return "第四章_机械加工精度及其控制"
        return "第二章_机械加工工艺规程设计"

    ins = 0
    for x in found:
        if not x["has_ans"]:
            continue          # 没答案的不给学生
        ch = guess_chapter(x["stem"])
        node = chap_node.get(ch)
        if not node:
            continue
        qid = "P_" + hashlib.md5(x["stem"][:60].encode()).hexdigest()[:10]
        if conn.execute("SELECT 1 FROM questions WHERE question_id=?", (qid,)).fetchone():
            continue
        conn.execute(
            """INSERT INTO questions
               (question_id, course_chapter, source_node_id, question_type, stem,
                options_json, answer, explanation, bloom_level, generation_model,
                prompt_template_id, review_status, total_score, source, usage_scope,
                exam_source, image_path, answer_image_path, image_reviewed, answer_source)
               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
            (qid, ch, node, "计算", x["stem"], None,
             "【标准答案见下方原卷截图】完整解题过程、尺寸链图、公式和采分点都在图里，"
             "这是老师原卷上的标准答案。",
             None, "应用", "原卷PDF", "pdf_exam", "已通过",
             x["score"], "真题", "学生练习",
             "北京理工大学期末考题|" + x["file"],
             x["image"], x["answer_image"], 1, "原卷截图"))
        conn.execute("INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) "
                     "VALUES (?,?)", (qid, node))
        ins += 1
    conn.commit()
    conn.close()
    print(f"\n已写入 {ins} 道计算真题（带原卷截图）")


if __name__ == "__main__":
    main()
