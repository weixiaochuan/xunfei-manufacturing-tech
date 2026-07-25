"""
真题导入器 v3 —— 彻底重写。前两版答案质量很差，根因是我一直没搞清文档的真实结构。

【前两版错在哪（三个坑，全踩了）】
1. **空白试卷和参考答案卷混在一起，我没区分。**
   从空白试卷里"抽答案"，抽到的其实是下一道题的题干。
   这就是你看到的：标准答案 = "2．假设该零件的加工直径符合正态分布…"（那是第2小问，不是答案！）
2. **题干经常跨多行**（"工序5 车外形…" 和 "工序10 平端面…" 是同一道题的两行），
   我却在第一行就切断，把剩下的题干当成了答案。
   这就是：标准答案 = "工序10 平端面，保证工序尺寸C" 的由来。
3. **名词解释的术语行没有题号**（就是光秃秃一个词），我按题号找，自然错位。

【这一版基于逐份核对后确认的真实结构】
参考答案卷长这样（规整得很）：

    解释下列工艺学名词：（每题4分，共20分）
    机械加工工艺过程                     <- 术语（没有题号！）
    用切削加工的方法，直接改变…           <- 紧跟的行就是定义
    工序、工步、工位                      <- 下一个术语
    工序：一个（或同时加工的一组）…        <- 它的定义（可多行）
    二．简述题（40分）
    6．（10分）什么是精基准？简述精基准的选择原则。   <- 有题号+分值
    用已加工过的表面所作的定位基准…                  <- 答案，直到下一个题号

所以：
  1. **先判断这份文件是不是答案卷**——看名词解释段里术语行后面有没有定义行。

     ⭐ 【两条路，都不浪费】
     · **有答案的题** -> 清洗后入库，`usage_scope='学生练习'`，学生可以做、能得到反馈。
     · **没答案的题**（空白试卷里的题、答案残缺的题）-> 也入库，但标记
       `usage_scope='教师出题'`：**不给学生做**（没有标准答案，反馈会是错的），
       但**保留给老师做出题素材**——这些都是真实考过的题，对老师很有价值。
     这样既不给学生错答案，也不浪费老师给的真题。
  2. 名词解释段：短行=术语，长行=定义。
  3. 简述/计算段：题号行开题，**题干允许跨行**（靠"还在提问/还在给条件"判断）。
  4. 图片按段落位置归属到它所在的那道题。
  5. 质量闸门：不合格直接丢弃，并打印原因。

用法：
    python3 pipeline/import_real_exams.py --scan     # 逐题打印，方便你抽查
    python3 pipeline/import_real_exams.py --apply    # 写库
"""
import argparse
import glob as _glob
import hashlib
import json
import os
import re
import sqlite3
import zipfile
from collections import Counter

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()
DOCX_DIR = os.path.join(BASE_DIR, "data", "real_exams", "docx")
DOCX_DIR2 = os.path.join(BASE_DIR, "data", "real_exams", "docx2")   # 另一批老卷子（格式不同）
DOCX_DIR3 = os.path.join(BASE_DIR, "data", "real_exams", "docx3")   # 老师后来给的《真题集》
IMG_DIR = os.path.join(BASE_DIR, "assets", "images", "real_exams")

SCORE_RE = re.compile(r"[（(]\s*(\d+)\s*分\s*[)）]")
NUM_RE = re.compile(r"^\s*(\d{1,2})\s*[．\.、]\s*(.*)$")
FIG_CAP = re.compile(r"^\s*(题?\s*\d+\s*图|图\s*[\d\-–\.]+|答\s*\d+\s*图|[（(][a-d][)）])\s*\S{0,25}$")
TABLE_HEAD = re.compile(r"^正态分布.{0,15}表\s*$")

# 有些卷子用中文数字给题编号（"六．在机械加工中自激振动…"），它也是一道新题的开始。
# 上一版没认这个，导致第6题的题干把第7题整道题吞进来了。
CN_NUM_RE = re.compile(r"^\s*[一二三四五六七八九十]{1,2}\s*[．\.、]\s*(.+)$")

SEC_NOUN = re.compile(r"解释下列.{0,10}名词|名词解释")
SEC_BRIEF = re.compile(r"简述题|简答题")
SEC_CALC = re.compile(r"分析计算题|计算题|分析题")

# 【关键】答案的结束标志：出现"看起来像一道新题"的行。
# 不能只靠题号！转换后题号经常丢失，于是出现 "（10分）什么是…" 这种没号的新题，
# 上一版就是因此把后面整张卷子的题目全吞进了答案里。
# 所以改成：**只要这行长得像一道题，答案就到此为止。**
NEW_Q_LOOKS_LIKE = re.compile(
    r"^\s*[（(]\s*\d+\s*分\s*[)）]"                    # 以（10分）开头
    r"|^\s*\d{1,2}\s*[．\.、]\s*[（(]?\s*\d*\s*分?"   # 1．（8分）
    r"|^\s*\d{1,2}\s*[．\.、]\s*(什么是|简述|试|计算|求|分析|论述|说明|如|下图|在|加工)"
    r"|^\s*[一二三四五六七八九十]{1,2}\s*[．\.、]"          # 六．
)
# 题干续行的特征：还在提问、还在给条件
STEM_CONT = re.compile(
    r"[？?：:；;]\s*$"          # 以问号/冒号/分号结尾 -> 还在问
    r"|^[（(]\d+[)）]"          # (1)(2)(3) 小问
    r"|^(试|请|计算|求|分析|已知|要求|设|其|工序\s*\d)"   # 以这些词开头 -> 还在给条件/提要求
    r"|(请|试)\s*(安排|计算|分析|确定|说明|指出|给出|求)"   # 句中含"请安排/试计算" -> 还是题干
)

CHAPTER_KEYWORDS = [
    ("第二章_机械加工工艺规程设计",
     ["工序", "工步", "工位", "生产纲领", "工艺规程", "基准", "加工余量", "尺寸链",
      "工序分散", "工序集中", "加工阶段", "毛坯", "工艺路线", "工艺过程"]),
    ("第三章_机床夹具设计",
     ["夹具", "定位", "自由度", "V形块", "V型块", "过定位", "欠定位", "夹紧", "支承", "菱形销"]),
    ("第四章_机械加工精度及其控制",
     ["加工精度", "加工误差", "误差复映", "刚度", "回转误差", "主轴", "热变形",
      "系统误差", "随机误差", "正态分布", "合格品率", "工艺能力", "自激振动", "颤振", "振动"]),
    ("第五章_机械加工表面质量及其控制",
     ["表面质量", "表面粗糙度", "冷作硬化", "残余应力", "表面层", "耐磨性", "疲劳", "磨削烧伤"]),
    ("第六章_机器装配工艺过程设计",
     ["装配", "总装", "选择装配", "分组选配", "修配", "互换", "装配精度"]),
    ("第七章_机械制造工艺理论和技术的发展",
     ["先进制造", "成组技术", "并行工程", "智能制造", "特种加工"]),
    ("第一章_绪论",
     ["制造技术", "制造系统", "切削", "刀具", "前角", "后角", "机床", "成型方法", "进给速度"]),
]


def dedup_key(stem):
    """去重用的指纹。要能认出这两种情况是同一道题：
       · "工序、工步、工位"  vs  "工序、工位、工步"   （并列词换了顺序）
       · "什么是粗基准？简述粗基准的选择原则。" vs "什么是粗基准，请说明粗基准的选择原则"
    做法：去掉所有标点，把并列项排序，只留汉字。
    （上一版只按原文哈希，所以这两道被当成了两道题，你就看到重复了。）
    """
    t = re.sub(r"[，。、？?：:；;（）()\s\.]", "", stem or "")
    t = re.sub(r"(什么是|简述|请说明|试说明|论述|请简要论述|的选择原则|选择原则)", "", t)
    return "".join(sorted(t))


def guess_chapter(text):
    best, sc = "第一章_绪论", 0
    for ch, kws in CHAPTER_KEYWORDS:
        s = sum(1 for k in kws if k in text)
        if s > sc:
            best, sc = ch, s
    return best


def _img_names(par, doc_part):
    """这一段引用了哪些媒体文件。
    注意：公式/零件图是 OLE 对象，它同时引用 oleObject.bin（二进制）和 image.wmf（预览图），
    我们要的是后者。用 target_ref 取，比 target_part 稳（.bin 取 partname 会抛异常，
    之前就是因此把整段的图都丢了）。"""
    out = []
    for rid in re.findall(r'r:(?:embed|id)="(rId\d+)"', par._p.xml):
        try:
            ref = doc_part.rels[rid].target_ref      # 如 "media/image2.wmf"
        except Exception:
            continue
        name = os.path.basename(ref)
        if name.lower().endswith((".wmf", ".emf", ".png", ".jpg", ".jpeg", ".gif")):
            out.append(name)                         # oleObject*.bin 自动被过滤掉
    return out


def read_doc(path):
    from docx import Document
    d = Document(path)
    return [(p.text.strip(), _img_names(p, d.part)) for p in d.paragraphs]


def _is_usable_image(fp):
    """图里到底有没有东西？
    这批 .doc 里的零件图大多是 WMF 矢量对象，在 Word 之外根本渲染不出来；
    转出来的 png/jpeg 常常是**一张白板**。
    上一版把这些白板当成"题目配图"塞给了学生 —— 就是你说的"图有误"。
    所以这里做个体检：非白像素太少（<1.5%）的，判定为废图，不要。
    """
    try:
        from PIL import Image
        im = Image.open(fp).convert("L")
        if im.width < 60 or im.height < 40:
            return False
        px = im.resize((min(im.width, 200), min(im.height, 200))).tobytes()
        dark = sum(1 for b in px if b < 205)
        return (dark / max(len(px), 1)) >= 0.015
    except Exception:
        return False        # 读不出来的也不要


_MEDIA_MAP = None


def load_media_map():
    """读 pipeline/extract_formulas.py 转好的图（含 WMF 公式和零件图）。

    以前我跳过 WMF，以为那只是没用的公式碎片——**大错**。
    那 609 个 WMF 里装的正是**零件图和尺寸公式**。缺了它们，计算题的题干就是残的
    （"计算工序尺寸和。"——尺寸符号没了），学生根本没法做，
    这就是计算题一直质量差、数量少的根因。
    """
    global _MEDIA_MAP
    if _MEDIA_MAP is None:
        mp = os.path.join(BASE_DIR, "data", "real_exams", "media_map.json")
        try:
            with open(mp, encoding="utf-8") as f:
                _MEDIA_MAP = json.load(f)
        except Exception:
            _MEDIA_MAP = {}
    return _MEDIA_MAP


def extract_media(path):
    """这份文档里有哪些图（媒体名 -> 图片路径）。
    优先用 media_map（WMF 已经被 extract_formulas.py 转成 PNG 了）。"""
    mm = load_media_map().get(os.path.basename(path))
    if mm:
        return {k: v["path"] for k, v in mm.items()}
    # 兜底：没跑过 extract_formulas.py 时，只能抽位图（WMF 抽不了）
    out = {}
    try:
        z = zipfile.ZipFile(path)
    except Exception:
        return out
    os.makedirs(IMG_DIR, exist_ok=True)
    for n in z.namelist():
        if not n.startswith("word/media/"):
            continue
        ext = os.path.splitext(n)[1].lower()
        if ext not in (".png", ".jpg", ".jpeg", ".gif"):
            continue
        data = z.read(n)
        if len(data) < 5000:
            continue
        fn = hashlib.md5(data).hexdigest()[:10] + ext
        fp = os.path.join(IMG_DIR, fn)
        if not os.path.exists(fp):
            with open(fp, "wb") as f:
                f.write(data)
        if not _is_usable_image(fp):
            try:
                os.remove(fp)
            except OSError:
                pass
            continue
        out[os.path.basename(n)] = os.path.relpath(fp, BASE_DIR).replace("\\", "/")
    return out
def exam_source(paras, fname):
    """从卷子抬头抽出处，例如：
       "北京理工大学 2022-2023学年第二学期 2020级 设计与制造基础Ⅲ 终考试题A"
    老师说得对：给他一道没有答案的题，如果连出处都没有，他不敢用。
    有了出处，他能自己去翻原卷核对。"""
    head = " ".join(t for t, _ in paras[:6] if t)
    uni = "北京理工大学" if "北京理工大学" in head else ""
    ym = re.search(r"(20\d\d)\s*[-–—]\s*(20\d\d)\s*学年(第.学期)?", head)
    grade = re.search(r"(20\d\d)级", head)
    course = re.search(r"(设计与制造基础[ⅠⅡⅢIV\d]*|机械制造工程学[A-B]?|机械制造工艺学)", head)
    paper = re.search(r"((?:终考|补考|期末|期中)?试题\s*[AB])", head)
    code = re.search(r"课程编号[：:]\s*(\w+)", head)
    parts = [x for x in [
        uni,
        (f"{ym.group(1)}-{ym.group(2)}学年" + (ym.group(3) or "")) if ym else "",
        (grade.group(1) + "级") if grade else "",
        course.group(1) if course else "",
        paper.group(1) if paper else "",
    ] if x]
    full = " ".join(parts) if parts else f"历年真题({fname})"
    if code:
        full += f"（课程编号 {code.group(1)}）"
    # 短标签：一眼看清是哪儿的（长的那串点开再看）
    short = (uni or "历年真题")
    if "补考" in head:
        short += "补考题"
    elif "期中" in head:
        short += "期中考题"
    else:
        short += "期末考题"
    return short + "|" + full        # 用 | 分隔：短|全


def is_answer_key(paras):
    """【最关键的一步】这份是【参考答案卷】还是【空白试卷】？
    空白试卷里根本没有答案，从里面"抽答案"就是上一版全部错误的根源。

    判据：名词解释段里，术语行后面有没有跟着"定义行"（长句）。
      空白试卷：术语一行接一行（机械加工工艺过程 / 工序 / 欠定位…），中间没有定义。
      答案卷：  术语后面紧跟一段定义文字。
    """
    texts = [t for t, _ in paras]
    start = None
    for i, t in enumerate(texts):
        if t and SEC_NOUN.search(t) and len(t) < 40:
            start = i
            break
    if start is None:
        return False
    seg = [t for t in texts[start + 1:start + 16] if t]
    if len(seg) < 3:
        return False
    long_lines = sum(1 for t in seg[:12] if len(t) > 25)
    return long_lines >= 2


def parse_noun_section(lines):
    """名词解释：短行=术语（无题号），长行=定义。"""
    out, cur = [], None
    for t, media in lines:
        if not t:
            continue
        t2 = NUM_RE.sub(r"\2", t).strip()
        is_term = (2 <= len(t2) <= 18 and not re.search(r"[。，；]", t2)
                   and not t2.startswith(("答", "即", "指", "是")))
        if is_term:
            if cur:
                out.append(cur)
            cur = {"stem": t2, "ans": [], "media": list(media)}
        elif cur is not None:
            cur["ans"].append(t)
            cur["media"].extend(media)
    if cur:
        out.append(cur)
    res = []
    for q in out:
        ans = "\n".join(q["ans"]).strip()
        if ans:
            res.append({"type": "名词解释", "stem": q["stem"], "answer_raw": ans,
                        "media": q["media"], "total_score": 4})
    return res


def parse_qa_section(lines, qtype):
    """简述/计算：题号行开题；题干可跨行；其余是答案。"""
    out, cur = [], None
    for t, media in lines:
        if not t and not media:
            continue
        # 中文数字题号（"六．…"）= 新题开始，把当前题收尾
        if t and CN_NUM_RE.match(t) and cur is not None:
            out.append(cur)
            cur = {"num": cur["num"] + 1,
                   "stem_lines": [CN_NUM_RE.match(t).group(1).strip()],
                   "ans": [], "media": list(media), "in_ans": False}
            continue

        m = NUM_RE.match(t) if t else None
        if m and cur is not None and int(m.group(1)) <= cur["num"]:
            m = None      # 答案里的 1)2)3) 小标号，不是新题
        if m:
            if cur:
                out.append(cur)
            cur = {"num": int(m.group(1)), "stem_lines": [m.group(2).strip()],
                   "ans": [], "media": list(media), "in_ans": False}
        elif cur is not None:
            if t:
                # 【答案截断】已经在写答案了，又冒出一行"长得像新题"的 -> 答案到此为止。
                # 这行以及后面的内容都不属于当前题（它们是下一道题/别的卷子的题）。
                if cur["in_ans"] and NEW_Q_LOOKS_LIKE.match(t):
                    out.append(cur)
                    cur = None
                    continue
                # 题干还没说完？（答案还没开始 + 这行还在提问/给条件）
                cont = (not cur["in_ans"]) and (not cur["ans"]) and STEM_CONT.search(t)
                if cont:
                    cur["stem_lines"].append(t)
                    if re.search(r"(请|试)\s*(安排|计算|分析|确定|说明|指出|给出|求)", t):
                        cur["in_ans"] = True
                else:
                    cur["in_ans"] = True
                    cur["ans"].append(t)
            if cur is not None:
                cur["media"].extend(media)
    if cur:
        out.append(cur)

    res = []
    for q in out:
        stem = " ".join(q["stem_lines"]).strip()
        ans = "\n".join(q["ans"]).strip()
        if not stem or not ans:
            continue
        sm = SCORE_RE.search(stem)
        total = int(sm.group(1)) if sm else 10
        res.append({"type": qtype, "stem": SCORE_RE.sub("", stem).strip(),
                    "answer_raw": ans, "media": q["media"], "total_score": total})
    return res


def clean_answer(ans):
    """清洗给学生看的答案：
       · 去掉图注行、孤立表头
       · **去掉句子中间的采分点标注**（"利用较多（2分）的简单工序（1分）"读着很别扭）
         —— 采分点已经单独存在 rubric 里了，正文不用再重复。"""
    out = []
    for l in [x.strip() for x in (ans or "").split("\n")]:
        if not l or FIG_CAP.match(l) or TABLE_HEAD.match(l):
            continue
        # 【截断】"10分）什么是工序？…" —— 这是下一道题（左括号在转换时被吞了）。
        # 我原来的规则要求以"（"开头才认，所以漏掉了这种，答案就拖了个尾巴。
        if re.match(r"^\s*\d{1,2}\s*分\s*[)）]", l):
            break
        if NEW_Q_LOOKS_LIKE.match(l) and out:
            break
        l = SCORE_RE.sub("", l)
        l = re.sub(r"[（(]\s*每项\d+分[^)）]*[)）]", "", l)
        l = re.sub(r"^(参考答案|答题要点|答案|答)\s*[：:]\s*", "", l)   # 剥掉"答："这种前缀
        l = re.sub(r"\s{2,}", " ", l).strip(" ，,")
        if l:
            out.append(l)
    return "\n".join(out).strip()


def parse_rubric(ans, total):
    pts, last = [], 0
    for m in SCORE_RE.finditer(ans):
        seg = re.sub(r"\s+", " ", ans[last:m.start()]).strip()
        if seg:
            pts.append({"point": seg[-80:].strip(" 。；;，,、"), "score": int(m.group(1))})
        last = m.end()
    if pts and total:
        s = sum(p["score"] for p in pts)
        if s > total * 1.4 or s < total * 0.5:
            return []       # 抽错了，宁可不要（错采分点比没有更糟）
    return pts


TRUNC = [re.compile(r"(尺寸|直径|深度|余量|偏差|角度|长度|键槽)\s*和?\s*[。.]$"),
         re.compile(r"为\s*mm\s*$"), re.compile(r"要求为\s*[。.]"), re.compile(r"深为\s*mm"),
         # 公式符号被吞后留下的空洞："必须限制、、、4个自由度"、"求得、、"
         re.compile(r"[、，,]\s*[、，,]\s*[、，,]"),
         re.compile(r"限制\s*[、，,]{2,}"),
         re.compile(r"[（(]\s*[)）]"),          # 空括号
         ]


# 常见的正经术语用字（术语总由这些字组成）。全是数字/怪字的（"三一一连续"）挡掉。
TERM_OK = re.compile(r"[\u4e00-\u9fa5]{2,}")
TERM_BAD = re.compile(r"^[一二三四五六七八九十百零〇\d\s、．.]+")


def quality_check_teacher(q):
    """给【老师出题素材】的标准（宽松）：只要题干是完整的一道题就行，不要求有答案。"""
    stem = q["stem"]
    if q["type"] == "名词解释":
        if not (2 <= len(stem) <= 20) or "？" in stem:
            return False, "不像术语"
        if TERM_BAD.match(stem) or not TERM_OK.search(stem):
            return False, "题干是乱码/数字"
    else:
        if len(stem) < 12:
            return False, "题干过短（被截断了）"
        for p in TRUNC:
            if p.search(stem):
                return False, "题干有公式被吞掉的断头句"
    return True, ""


def quality_check(q):
    """给【学生练习】的标准（严）：必须有正确、完整的答案，需要图就得真有图。"""
    stem, ans = q["stem"], q["answer"]
    if q["type"] == "名词解释":
        if not (2 <= len(stem) <= 20) or "？" in stem:
            return False, "名词解释题干不像术语"
        if TERM_BAD.match(stem) or not TERM_OK.search(stem):
            return False, "名词解释题干是乱码/数字（不是术语）"
        # 答案开头如果是"1）2）"这种罗列，说明抽到的是别的题的答案
        if re.match(r"^\s*[1１]\s*[）)]", ans):
            return False, "名词解释的答案像是罗列条款（抽错了）"
    elif len(stem) < 10:
        return False, "题干过短"
    # 答案长度门槛要按题型分开定 —— 一刀切是不对的。
    # "进给速度——刀具在进给方向上的运动速度。" 这是个**完全正确的名词解释**，
    # 只有 16 个字，却被我按"少于15字就算没抽到"的规则误杀了。你说我"不灵活"，说得对。
    min_len = 8 if q["type"] == "名词解释" else 15
    if len(ans) < min_len:
        return False, "答案过短/没抽到"
    first = ans.split("\n")[0].strip()
    if NUM_RE.match(first):
        return False, "答案开头是另一道题的题号（切错了）"

    # 【新增】答案里的公式被吞光了 —— 这种答案给学生等于害他
    #   "可列出尺寸链如图 / 图中，(A₀) 为封闭环 / 上偏差：(ES=...)"
    #   括号里的符号和公式在原Word里都是**图片对象**，读不出来。
    #   剩下的这句话是**没法用的**（学生看不到尺寸链、看不到封闭环是哪个、看不到公式）。
    #   这类题必须转 PDF 才能救，现在只能进教师素材库。
    formula_holes = 0
    if re.search(r"(可列出|列)尺寸链如图|尺寸链(图)?如图", ans):
        formula_holes += 1
    if re.search(r"图中\s*[，,]\s*为封闭环|[，,]\s*为封闭环", ans):
        formula_holes += 1     # "图中，A₀ 为封闭环" —— A₀ 没了
    if re.search(r"(上|下)偏差\s*[：:]\s*$", ans, re.M):
        formula_holes += 1     # "上偏差：" 后面的公式没了
    if formula_holes >= 2:
        return False, "答案里的公式/尺寸链符号被吞掉了（需要原卷PDF才能救）"

    # 计算题的答案必须**真的有计算过程**。
    # "列尺寸链如图，封闭环为设计孔的位置尺寸" —— 这一句话不是答案，是废话：
    # 学生看不到尺寸链、看不到封闭环是哪个、没有任何算式。给了等于害他。
    if q["type"] == "计算":
        has_math = bool(re.search(r"[=＝]", ans))          # 有算式
        if not has_math or len(ans) < 55:
            return False, "计算题答案没有计算过程（公式被吞掉了，需要原卷PDF）"

    # 答案开头是题干残片（"试安排…" "Φ50外圆：表面淬火。"）
    if re.match(r"^(试(安排|计算|分析|求)|Φ?\d*外圆\s*[：:]\s*表面淬火)", first):
        return False, "答案开头是题干残片（切错了）"
    # 答案开头是题干的残片（"零件图"、"工序10 车小头外圆"、"试安排…"）
    if re.match(r"^(零件图|工序\s*\d+|毛坯图|试(安排|计算|分析|求)|如[左右下上]?图)", first):
        return False, "答案开头是题干残片（切错了）"
    for p in TRUNC:
        if p.search(stem) or p.search(ans):
            return False, "有公式被吞掉的断头句"
    if re.search(r"下图|右图|左图|如图|图\s*\d|所示图|题\s*\d+\s*图", stem) and not q["images"]:
        return False, "题干要看图但没抽到图"
    return True, ""


REF_ANS_RE = re.compile(r"^\s*(参考答案|答案|评分标准)\s*[：:）)]?\s*$")


def parse_老卷(paras, imgs, src):
    """另一种卷型（2005-2015 的老卷子），格式是这样的：

        一．解释下列工艺学名词：（每题3分，共15分）
        1．生产纲领——包括备品率和废品率在内的计划年产量。      <- 术语——定义 写在一行
        2．动刚度——在振动中，振动幅度与激振力的比值。
        二．什么是工序时间？…可以用哪些技术手段来减少工序时间？（15分）   <- 大题直接就是题干
        参考答案：                                              <- 用这行分隔
        一个零件完成一道工序所需要的时间称工序时间。（5分）        <- 答案，带采分点
        ...
        三．机械加工的表面成型方法有哪几种形式？…（10分）

    和新卷子的区别：名词解释用"——"把术语和定义写在同一行；
    大题不按 1.2.3. 编号，而是用中文数字（一二三四），且用"参考答案："分隔题干和答案。
    我之前只认新卷子的格式，所以这 40 多份卷子一道题都没收——白白浪费了。
    """
    res = []
    # 找"名词解释"段
    noun_start = None
    for i, (t, _) in enumerate(paras):
        if t and SEC_NOUN.search(t):
            noun_start = i
            break
    # 名词解释：形如 "1．生产纲领——包括…"
    if noun_start is not None:
        for t, media in paras[noun_start + 1:noun_start + 20]:
            if not t:
                continue
            if re.match(r"^\s*[一二三四五六七八九十]\s*[．\.、]", t):
                break                       # 到下一个大题了
            m = re.match(r"^\s*\d{1,2}\s*[．\.、]\s*(.+)$", t)
            if not m:
                continue
            body = m.group(1)
            parts = re.split(r"[—–]{1,2}|[:：]", body, maxsplit=1)
            if len(parts) < 2:
                continue
            term, defi = parts[0].strip(), parts[1].strip()
            if not term or len(term) > 20 or len(defi) < 8:
                continue
            res.append({"type": "名词解释", "stem": term, "answer_raw": defi,
                        "media": list(media), "total_score": 4, "exam_source": src})

    # 大题：中文数字开头 + （N分），下面跟"参考答案："
    marks = []
    for i, (t, _) in enumerate(paras):
        if t and re.match(r"^\s*[二三四五六七八九十]\s*[．\.、]", t) and SCORE_RE.search(t):
            marks.append(i)
    for k, start in enumerate(marks):
        end = marks[k + 1] if k + 1 < len(marks) else len(paras)
        head = paras[start][0]
        stem = re.sub(r"^\s*[二三四五六七八九十]\s*[．\.、]\s*", "", head).strip()
        sm = SCORE_RE.search(stem)
        total = int(sm.group(1)) if sm else 10
        stem = SCORE_RE.sub("", stem).strip()

        ans_lines, media_all, started = [], list(paras[start][1]), False
        for t, media in paras[start + 1:end]:
            if t and REF_ANS_RE.match(t):
                started = True
                continue
            if started and t:
                ans_lines.append(t)
            media_all.extend(media)
        if not started:          # 没有"参考答案："分隔 -> 题干后面的都算答案
            ans_lines = [t for t, _ in paras[start + 1:end] if t]
        answer = "\n".join(ans_lines).strip()
        if not stem or len(answer) < 15:
            continue
        qtype = "计算" if re.search(r"计算|求|尺寸链|公差|工序尺寸", stem) else "简述"
        res.append({"type": qtype, "stem": stem, "answer_raw": answer,
                    "media": media_all, "total_score": total, "exam_source": src})
    return res


def parse_file(path, old_format=False):
    """返回 (题目列表, 是否答案卷)。
    空白试卷也解析——它的题目照样有价值，只是没答案，只能给老师当出题素材。"""
    paras = read_doc(path)
    imgs = extract_media(path)
    src = exam_source(paras, os.path.basename(path))

    if old_format:
        # 老卷子（2005-2015）：格式和新卷不同，走专门的解析
        qs = parse_老卷(paras, imgs, src)
        for q in qs:
            q["answer"] = clean_answer(q.get("answer_raw", ""))
            q["rubric"] = parse_rubric(q.get("answer_raw", ""), q["total_score"])
            q["images"] = [imgs[m] for m in q["media"] if m in imgs]
            q["chapter"] = guess_chapter(q["stem"] + q["answer"][:200])
            q["has_answer"] = bool(q["answer"])
        return qs, True

    has_ans = is_answer_key(paras)

    marks = []
    for i, (t, _) in enumerate(paras):
        if not t or len(t) > 40:
            continue
        if SEC_NOUN.search(t):
            marks.append((i, "名词解释"))
        elif SEC_BRIEF.search(t):
            marks.append((i, "简述"))
        elif SEC_CALC.search(t):
            marks.append((i, "计算"))
    if not marks:
        return [], has_ans

    res = []
    for k, (start, sec) in enumerate(marks):
        end = marks[k + 1][0] if k + 1 < len(marks) else len(paras)
        seg = paras[start + 1:end]
        if has_ans:
            qs = parse_noun_section(seg) if sec == "名词解释" else parse_qa_section(seg, sec)
        else:
            qs = parse_stems_only(seg, sec)      # 空白试卷：只抽题干
        for q in qs:
            q["answer"] = clean_answer(q.get("answer_raw", ""))
            q["rubric"] = parse_rubric(q.get("answer_raw", ""), q["total_score"])
            q["images"] = [imgs[m] for m in q["media"] if m in imgs]
            q["chapter"] = guess_chapter(q["stem"] + q["answer"][:200])
            q["has_answer"] = bool(q["answer"])
            q["exam_source"] = src
            res.append(q)
    return res, has_ans


def parse_stems_only(lines, qtype):
    """空白试卷：只抽题干（没有答案）。这些题给老师当出题素材，不给学生做。"""
    out, cur = [], None
    for t, media in lines:
        if not t and not media:
            continue
        m = NUM_RE.match(t) if t else None
        cn = CN_NUM_RE.match(t) if t else None
        if m or cn:
            if cur:
                out.append(cur)
            head = (m.group(2) if m else cn.group(1)).strip()
            cur = {"stem_lines": [head], "media": list(media)}
        elif cur is not None:
            if t and not FIG_CAP.match(t):
                cur["stem_lines"].append(t)      # 题干续行
            cur["media"].extend(media)
        elif qtype == "名词解释" and t:
            # 名词解释在空白卷里就是一行一个术语
            out.append({"stem_lines": [t], "media": list(media)})
    if cur:
        out.append(cur)

    res = []
    for q in out:
        stem = " ".join(q["stem_lines"]).strip()
        if not stem:
            continue
        sm = SCORE_RE.search(stem)
        total = int(sm.group(1)) if sm else (4 if qtype == "名词解释" else 10)
        res.append({"type": qtype, "stem": SCORE_RE.sub("", stem).strip(),
                    "answer_raw": "", "media": q["media"], "total_score": total})
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    a = ap.parse_args()

    files = [(f, False) for f in sorted(_glob.glob(os.path.join(DOCX_DIR, "*.docx")))]
    files += [(f, True) for f in sorted(_glob.glob(os.path.join(DOCX_DIR2, "*.docx")))]
    files += [(f, True) for f in sorted(_glob.glob(os.path.join(DOCX_DIR3, "*.docx")))]
    print(f"扫描 {len(files)} 份真题"
          f"（{sum(1 for _, o in files if not o)} 份新卷 + {sum(1 for _, o in files if o)} 份老卷）\n")

    # 两条路：
    #   student = 有正确答案 -> 学生可以练，能拿到反馈
    #   teacher = 没答案/答案残缺 -> 只给老师当出题素材，绝不给学生做
    student, teacher, dropped, seen = [], [], [], set()
    best = {}          # dedup_key -> 目前最可信的那份（带采分点的优先）
    n_ans_files = 0
    for f, old_fmt in files:
        try:
            qs, has_ans = parse_file(f, old_format=old_fmt)
        except Exception as e:
            dropped.append({"stem": os.path.basename(f), "reason": f"读取失败:{e}"})
            continue
        if has_ans:
            n_ans_files += 1
        for q in qs:
            key = dedup_key(q["stem"])
            if key in seen:
                # 这道题之前已经收过了。但**不同卷子给的答案可能不一样**，
                # 而且有的卷子（比如被人填过的电子稿）答案是错的。
                # 判据：**带采分点的答案最可信**（那是老师亲手标的评分标准）。
                prev = best.get(key)
                if prev is not None:
                    now_score = (2 if q.get("rubric") else 0) + (1 if len(q["answer"]) > len(prev["answer"]) else 0)
                    prev_score = (2 if prev.get("rubric") else 0)
                    if now_score > prev_score:
                        ok2, _ = quality_check(q)
                        if ok2:
                            best[key] = q          # 换成更可信的这一份
                continue
            seen.add(key)
            ok, why = quality_check(q)
            if ok:
                q["usage"] = "学生练习"
                best[key] = q
                student.append(q)
                continue
            # 学生练习不合格 -> 看看能不能当老师的出题素材
            ok2, why2 = quality_check_teacher(q)
            if ok2:
                q["usage"] = "教师出题"
                q["teacher_reason"] = why      # 为什么不能给学生做
                teacher.append(q)
            else:
                dropped.append({"stem": q["stem"][:45], "reason": why2})

    # 用"最可信版本"替换掉先收进来的那份
    student = [best.get(dedup_key(q["stem"]), q) for q in student]

    print(f"其中 {n_ans_files} 份是【参考答案卷】，{len(files)-n_ans_files} 份是【空白试卷】\n")
    print("=" * 66)
    print(f"✅ 【学生练习题】{len(student)} 道 —— 有正确答案，学生做完能拿到反馈")
    print(f"📘 【教师出题素材】{len(teacher)} 道 —— 没有答案/答案残缺，")
    print( "     **不给学生做**（没标准答案，反馈会是错的），但保留给老师出题用")
    print(f"❌ 丢弃 {len(dropped)} 道 —— 题干本身就残缺，留着也没用")
    print("=" * 66)
    print()
    print("学生练习题：", dict(Counter(q["type"] for q in student)),
          "| 带图", sum(1 for q in student if q["images"]),
          "| 带采分点", sum(1 for q in student if q["rubric"]))
    print("教师出题素材：", dict(Counter(q["type"] for q in teacher)))
    print("\n不能给学生做的原因：")
    for r, n in Counter(q.get("teacher_reason", "?") for q in teacher).most_common(6):
        print(f"  {n:>3}  {r}")
    print("\n彻底丢弃的原因：")
    for r, n in Counter(d["reason"] for d in dropped).most_common(5):
        print(f"  {n:>3}  {r}")

    print("\n" + "=" * 70)
    print("【学生练习题】逐题清单（请抽查答案对不对）")
    print("=" * 70)
    for i, q in enumerate(student, 1):
        print(f"\n{i}. [{q['type']}·{q['total_score']}分]{' 📷' if q['images'] else ''} {q['stem'][:70]}")
        print(f"   答案：{q['answer'][:95]}")

    kept = student + teacher

    if not a.apply:
        print("\n（扫描结果。加 --apply 写库。）")
        return

    conn = connect_database()
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    for c, ddl in [("rubric_json", "TEXT"), ("total_score", "INTEGER"),
                   ("source", "TEXT DEFAULT 'AI生成'"), ("image_path", "TEXT"),
                   ("image_reviewed", "INTEGER DEFAULT 0"),
                   # 用途：'学生练习'（有答案，能给反馈）/ '教师出题'（没答案，只给老师看）
                   ("usage_scope", "TEXT DEFAULT '学生练习'"),
                   ("no_answer_reason", "TEXT"),
                   ("exam_source", "TEXT")]:      # 出处：哪一年哪张卷子
        if c not in cols:
            conn.execute(f"ALTER TABLE questions ADD COLUMN {c} {ddl}")
    old = conn.execute("SELECT COUNT(*) FROM questions WHERE question_id LIKE 'R_%'").fetchone()[0]
    conn.execute("DELETE FROM question_knowledge_map WHERE question_id LIKE 'R_%'")
    conn.execute("DELETE FROM questions WHERE question_id LIKE 'R_%'")
    print(f"\n清掉上一版的 {old} 道真题")

    chap_node = {}
    for (ch,) in conn.execute("SELECT DISTINCT chapter FROM knowledge_points"):
        r = conn.execute("SELECT knowledge_id FROM knowledge_points WHERE chapter=? "
                         "ORDER BY learning_order LIMIT 1", (ch,)).fetchone()
        if r:
            chap_node[ch] = r[0]

    ins = 0
    for q in kept:
        node = chap_node.get(q["chapter"])
        if not node:
            continue
        qid = "R_" + hashlib.md5(q["stem"][:60].encode()).hexdigest()[:10]
        if conn.execute("SELECT 1 FROM questions WHERE question_id=?", (qid,)).fetchone():
            continue
        conn.execute(
            """INSERT INTO questions
               (question_id, course_chapter, source_node_id, question_type, stem,
                options_json, answer, explanation, bloom_level, generation_model,
                prompt_template_id, review_status, rubric_json, total_score, source,
                image_path, image_reviewed, usage_scope, no_answer_reason, exam_source)
               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
            (qid, q["chapter"], node, q["type"], q["stem"], None,
             # 教师出题素材没有答案。answer 字段非空，所以写一句明确的说明，
             # 而不是塞一个假答案进去——绝不能让任何地方误以为这是标准答案。
             (q["answer"] or "（本题来自历年真题，但原卷未提供参考答案；仅作教师出题素材，不用于学生练习）"),
             None,
             None, "真题(教师提供)", "real_exam_v3", "已通过",
             json.dumps(q["rubric"], ensure_ascii=False), q["total_score"], "真题",
             q["images"][0] if q["images"] else None, 1 if q["images"] else 0,
             q.get("usage", "学生练习"), q.get("teacher_reason"), q.get("exam_source")))
        conn.execute("INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) "
                     "VALUES (?,?)", (qid, node))
        ins += 1
    conn.commit()
    n_stu = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE source='真题' AND usage_scope='学生练习'").fetchone()[0]
    n_tea = conn.execute(
        "SELECT COUNT(*) FROM questions WHERE source='真题' AND usage_scope='教师出题'").fetchone()[0]
    conn.close()
    print(f"已写入 {ins} 道真题：")
    print(f"   学生练习 {n_stu} 道（有答案，学生能做、能拿反馈）")
    print(f"   教师出题 {n_tea} 道（没答案，只在教师端出题素材里显示，学生看不到）")


if __name__ == "__main__":
    main()
