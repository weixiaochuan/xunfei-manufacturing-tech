"""
RAG 检索引擎 —— 出题时"先查资料，再出题"。

═══════════════════════════════════════════════════════════════
【为什么要做这个】
═══════════════════════════════════════════════════════════════
以前出题：只把**这一个知识点**的原文喂给模型。
    "第二章-尺寸链" → 出一道尺寸链的题

问题：**真实的题常常跨知识点**。
    比如一道尺寸链计算题，同时用到：
      · 工艺基准（第二章）
      · 尺寸链的封闭环/组成环（第二章）
      · 公差与配合（第四章）
    模型只看到"尺寸链"这一个知识点，**出不来这种有深度的题**。

现在（RAG）：出题前先**检索出相关的多个知识点**，一起喂给模型。
    "尺寸链" → 检索 → [尺寸链, 工艺基准, 公差带, 工序尺寸...] → 一起塞进 Prompt

═══════════════════════════════════════════════════════════════
【技术选型：为什么用 TF-IDF 而不是向量模型？】
═══════════════════════════════════════════════════════════════
标准做法是"向量检索"（把文本转成向量，按语义相似度召回）。但：

1. **DeepSeek 没有 embedding 接口**（官方 GitHub 明确说了 "No Embedding Support"）。
   要用向量，得再接一个模型（OpenAI/通义/本地 BGE），多一个依赖、多一份钱。

2. **我们的场景，TF-IDF 反而更合适**：
   · 语料很专业（机械工艺术语："封闭环""增环""基准重合"）
   · **专业术语的字面匹配 = 语义匹配**（"尺寸链"这三个字出现，就是在讲尺寸链）
   · 语料量小（283 个知识点），不需要复杂的近似检索
   · 通用向量模型反而可能把"基准"理解成"benchmark"这种日常语义，**不如字面匹配准**

3. **中文分词的坑**：中文没有空格，标准 TF-IDF 要先分词（jieba）。
   我们用**字符 n-gram**（2~3 个字一组）绕过分词：
     "尺寸链" → ["尺寸", "寸链", "尺寸链"]
   这样"工序尺寸链"和"尺寸链"能匹配上，不依赖分词器。

**换成真向量很容易**：只要实现 `embed(texts) -> vectors`，替换 `_build_index` 即可。
接口已经留好了（见 `EMBEDDING_ADAPTER` 注释）。

═══════════════════════════════════════════════════════════════
【检索什么？三个库】
═══════════════════════════════════════════════════════════════
1. **知识点库**（283条）→ 出题时提供教材原文
2. **真题库**（137条）  → 出题时给模型"参考真题的出题风格"⭐
3. **误区库**            → 出题时提供干扰项素材

第 2 条是关键：以前 AI 出的题"像教材复述"，因为它没见过真题长什么样。
现在把**同知识点的真题**检索出来当范例，模型能学到真题的**出题手法**
（怎么给条件、怎么设计干扰项、怎么埋坑）。

用法：
    python3 pipeline/rag.py --build              # 建索引（几秒钟）
    python3 pipeline/rag.py --query "尺寸链计算"   # 试检索
"""
import argparse
import json
import os
import pickle
import re
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path, rag_index_path
DB_PATH = database_path()
INDEX_PATH = str(rag_index_path())

# ══════════════════════════════════════════════════════════════
# EMBEDDING_ADAPTER
# 以后要换成真向量模型（BGE / OpenAI / 通义），只需要：
#   1. 实现 embed(list[str]) -> np.ndarray  (n × dim)
#   2. 把 _build_index 里的 TfidfVectorizer 换成它
#   3. _search 里的余弦相似度计算不用动（一样的）
# 其它代码（出题、Prompt 拼装）**完全不用改**。
# ══════════════════════════════════════════════════════════════


def _clean(t):
    """去掉标点和空白，只留下有信息量的字。"""
    return re.sub(r"[\s，。、；：？！（）()\[\]「」【】,.;:?!/\\|\-—－_=+*#]", "", t or "")


def _load_corpus(conn):
    """把三个库读出来，做成统一的"文档"格式。"""
    conn.row_factory = sqlite3.Row
    docs = []

    # ① 知识点（283条）—— 出题的原料
    for r in conn.execute(
            "SELECT knowledge_id, chapter, section_title, knowledge_title, content, "
            "key_concepts, formulas FROM knowledge_points"):
        d = dict(r)
        # 检索用的文本：标题权重高，所以重复几次（简易加权）
        text = ((d["knowledge_title"] or "") + "。") * 3 + \
               (d["key_concepts"] or "") + "。" + \
               (d["section_title"] or "") + "。" + \
               (d["content"] or "")[:600] + "。" + \
               (d["formulas"] or "")
        docs.append({
            "kind": "knowledge",
            "id": d["knowledge_id"],
            "title": d["knowledge_title"],
            "chapter": d["chapter"],
            "text": text,
            "payload": d,
        })

    # ② 范例题 —— 出题时给模型看"好题长什么样"⭐ 这是提升 AI 题质量的关键
    #    真题优先（最权威），教材原题次之。
    #    ⚠️ 只收**有标准答案、能给学生做**的题：
    #       没答案的题（教师素材库）不能当范例——模型学不到"答案该怎么写"。
    for r in conn.execute(
            "SELECT question_id, course_chapter, question_type, stem, answer, "
            "rubric_json, total_score, source FROM questions "
            "WHERE source IN ('真题','教材习题') "
            "AND COALESCE(usage_scope,'学生练习')='学生练习' "
            "AND review_status='已通过' "
            "AND answer IS NOT NULL AND answer != ''"):
        d = dict(r)
        docs.append({
            "kind": "real_exam",
            "id": d["question_id"],
            "title": (d["stem"] or "")[:40],
            "chapter": d["course_chapter"],
            "text": (d["stem"] or "") + "。" + (d["answer"] or "")[:300],
            "payload": d,
        })

    # ③ 误区（干扰项素材）
    try:
        for r in conn.execute(
                "SELECT knowledge_id, misconception_text FROM misconceptions"):
            d = dict(r)
            docs.append({
                "kind": "misconception",
                "id": d["knowledge_id"],
                "title": (d["misconception_text"] or "")[:30],
                "chapter": None,
                "text": d["misconception_text"] or "",
                "payload": d,
            })
    except sqlite3.OperationalError:
        pass

    return docs


def build_index(verbose=True):
    """建索引。几秒钟，不联网、不花钱。"""
    from sklearn.feature_extraction.text import TfidfVectorizer

    conn = connect_database()
    docs = _load_corpus(conn)
    conn.close()
    if not docs:
        raise RuntimeError("语料是空的，检查数据库")

    corpus = [_clean(d["text"]) for d in docs]

    # 字符 n-gram：不依赖中文分词器。
    # ngram_range=(2,3)：2~3个字一组。"尺寸链" → 尺寸/寸链/尺寸链
    vec = TfidfVectorizer(
        analyzer="char",
        ngram_range=(2, 3),
        min_df=1,
        max_features=60000,
        sublinear_tf=True,      # 长文档不会因为词多就占便宜
    )
    matrix = vec.fit_transform(corpus)

    with open(INDEX_PATH, "wb") as f:
        pickle.dump({"vec": vec, "matrix": matrix, "docs": docs}, f)

    if verbose:
        n_k = sum(1 for d in docs if d["kind"] == "knowledge")
        n_e = sum(1 for d in docs if d["kind"] == "real_exam")
        n_m = sum(1 for d in docs if d["kind"] == "misconception")
        print(f"✅ 索引建好了：{len(docs)} 篇文档")
        print(f"   知识点 {n_k} · 真题 {n_e} · 误区 {n_m}")
        print(f"   特征维度 {matrix.shape[1]}")
        print(f"   存到 {INDEX_PATH}")
    return len(docs)


_CACHE = None


def _load_index():
    global _CACHE
    if _CACHE is None:
        if not os.path.exists(INDEX_PATH):
            raise RuntimeError(
                "索引还没建。先跑：python3 pipeline/rag.py --build")
        with open(INDEX_PATH, "rb") as f:
            _CACHE = pickle.load(f)
    return _CACHE


def search(query, kind=None, top_k=5, exclude_ids=None, chapter=None):
    """检索。

    query      : 查什么（一段文字）
    kind       : 只查某一类（knowledge / real_exam / misconception）
    top_k      : 返回几条
    exclude_ids: 排除掉哪些 id（比如出题时排除掉自己）
    chapter    : 限定章节
    """
    from sklearn.metrics.pairwise import cosine_similarity
    import numpy as np

    idx = _load_index()
    vec, matrix, docs = idx["vec"], idx["matrix"], idx["docs"]

    qv = vec.transform([_clean(query)])
    sims = cosine_similarity(qv, matrix)[0]

    exclude_ids = set(exclude_ids or [])
    scored = []
    for i, s in enumerate(sims):
        d = docs[i]
        if kind and d["kind"] != kind:
            continue
        if d["id"] in exclude_ids:
            continue
        if chapter and d["chapter"] and d["chapter"] != chapter:
            continue
        if s <= 0.01:
            continue
        scored.append((float(s), d))

    scored.sort(key=lambda x: -x[0])
    return [{"score": round(s, 4), **d} for s, d in scored[:top_k]]


def retrieve_for_generation(knowledge_point, n_related=3, n_examples=2):
    """⭐ 出题专用检索。

    给定一个知识点，返回出题需要的全部素材：
      · related  : 相关的其它知识点（让模型能出跨知识点的题）
      · examples : 同主题的真题（让模型学真题的出题手法）⭐
      · misconceptions : 学生的常见误区（做干扰项）
    """
    kid = knowledge_point.get("knowledge_id")
    query = ((knowledge_point.get("knowledge_title") or "") + "。" +
             (knowledge_point.get("key_concepts") or "") + "。" +
             (knowledge_point.get("content") or "")[:300])

    related = search(query, kind="knowledge", top_k=n_related,
                     exclude_ids=[kid] if kid else None)

    # 范例题：多召回一些，然后**真题优先**排序（真题比教材题更权威）
    cand = search(query, kind="real_exam", top_k=n_examples * 4)
    cand.sort(key=lambda x: (0 if x["payload"].get("source") == "真题" else 1,
                             -x["score"]))
    examples = cand[:n_examples]

    mis = search(query, kind="misconception", top_k=3)

    return {
        "related": related,
        "examples": examples,
        "misconceptions": [m["text"] for m in mis],
    }


def format_context(retrieved, max_chars=2400):
    """把检索到的东西拼成一段文字，塞进 Prompt。"""
    parts = []

    if retrieved.get("related"):
        parts.append("【相关知识点】（出题时可以综合运用，出跨知识点的题）")
        for r in retrieved["related"]:
            p = r["payload"]
            body = (p.get("content") or "")[:260]
            parts.append(f"· {r['title']}（{r['chapter']}）：{body}")
            if p.get("formulas"):
                parts.append(f"  公式：{p['formulas'][:120]}")
        parts.append("")

    if retrieved.get("examples"):
        parts.append("【范例题】⭐ 这是老师真实出过的题 / 教材原题，请学习它的**出题手法**")
        parts.append("（怎么给条件、怎么设计问法、答案怎么分采分点。**不要抄题，要学风格**）")
        for r in retrieved["examples"]:
            p = r["payload"]
            src = p.get("source") or "真题"
            parts.append(f"· [{src}·{p.get('question_type')}·{p.get('total_score') or ''}分] "
                         f"{(p.get('stem') or '')[:170]}")
            ans = (p.get("answer") or "")[:200]
            if ans:
                parts.append(f"  参考答案：{ans}")
            rub = p.get("rubric_json")
            if rub and rub not in ("", "[]"):
                try:
                    pts = json.loads(rub)
                    tags = "；".join(f"{x.get('point')}({x.get('score')}分)"
                                     for x in pts[:4] if isinstance(x, dict))
                    if tags:
                        parts.append(f"  采分点：{tags}")
                except Exception:
                    pass
        parts.append("")

    if retrieved.get("misconceptions"):
        parts.append("【学生的常见误区】（做干扰项用）")
        for m in retrieved["misconceptions"]:
            parts.append(f"· {m}")

    out = "\n".join(parts)
    return out[:max_chars]


def verify_answer(stem, answer, top_k=3):
    """⭐ 防幻觉：出完题之后，反查知识库，看这道题的答案在教材里有没有依据。

    比赛大纲第5页明确写着：「通过 RAG 技术**解决大模型幻觉问题**」。
    这就是那一步。

    做法：把「题干 + 答案」当查询词，去知识库里搜。
      · 搜得到高度相关的知识点 → 这道题有教材依据 ✅
      · 什么都搜不到          → 模型可能在编 ⚠️ 需要人工看一眼

    返回 (是否可信, 依据的知识点, 相似度)
    """
    q = (stem or "") + "。" + (answer or "")[:400]
    hits = search(q, kind="knowledge", top_k=top_k)
    if not hits:
        return False, [], 0.0
    best = hits[0]["score"]
    # 0.08 是经验阈值：低于这个基本就是"教材里找不到对应内容"
    return best >= 0.08, hits, best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--build", action="store_true", help="建索引")
    ap.add_argument("--query", help="试检索")
    ap.add_argument("--kind", choices=["knowledge", "real_exam", "misconception"])
    ap.add_argument("--top", type=int, default=5)
    a = ap.parse_args()

    if a.build:
        build_index()
        return

    if a.query:
        hits = search(a.query, kind=a.kind, top_k=a.top)
        if not hits:
            print("没检索到")
            return
        print(f"查询：{a.query}\n")
        for h in hits:
            kind_cn = {"knowledge": "知识点", "real_exam": "真题",
                       "misconception": "误区"}[h["kind"]]
            print(f"[{h['score']:.3f}] ({kind_cn}) {h['title']}")
            if h["chapter"]:
                print(f"         {h['chapter']}")
        return

    ap.print_help()


if __name__ == "__main__":
    main()
