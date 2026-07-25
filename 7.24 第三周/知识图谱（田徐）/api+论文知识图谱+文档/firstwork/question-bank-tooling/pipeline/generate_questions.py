"""
端到端出题流水线（对应报告6.3.1节步骤1-6）
knowledge_points -> Prompt生成(RAG锚定原文) -> LLM生成 -> 质量校验 -> 入库

用法示例：
    # 默认模型(config.roles)，给第一章前5个知识点各出1道概念题
    python3 pipeline/generate_questions.py --chapter 第一章_绪论 --limit 5 --type concept

    # 临时指定用 deepseek 跑（覆盖默认），方便和其它模型对比
    python3 pipeline/generate_questions.py --chapter 第二章_机械加工工艺规程设计 --limit 10 --type auto --provider deepseek

    # 给单个知识点出计算题
    python3 pipeline/generate_questions.py --knowledge_id KN_CH2_015 --type computation --provider deepseek
"""
import argparse
import json
import os
import sqlite3
import sys
import uuid

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from llm.client import get_client
from prompts import (concept_question_prompt, computation_question_prompt,
                     subjective_question_prompt, multichoice_question_prompt)
from pipeline.validators import validate_concept_question, validate_computation_question

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()


def fetch_knowledge_points(conn, chapter=None, knowledge_id=None, limit=None):
    cols = [d[0] for d in conn.execute("SELECT * FROM knowledge_points LIMIT 0").description]
    sql = "SELECT * FROM knowledge_points WHERE 1=1"
    params = []
    if chapter:
        sql += " AND chapter=?"; params.append(chapter)
    if knowledge_id:
        sql += " AND knowledge_id=?"; params.append(knowledge_id)
    sql += " ORDER BY learning_order"
    if limit:
        sql += " LIMIT ?"; params.append(limit)
    rows = conn.execute(sql, params).fetchall()
    return [dict(zip(cols, row)) for row in rows]


def fetch_misconceptions(conn, knowledge_id):
    rows = conn.execute(
        "SELECT misconception_text FROM misconceptions WHERE knowledge_id=?", (knowledge_id,)
    ).fetchall()
    return [r[0] for r in rows]


def fetch_existing_stems(conn, knowledge_id):
    """取该知识点已经出过的题干，喂给Prompt让模型换角度、避免出雷同题。
    对应首轮审核发现的'同一知识点反复出雷同题、甚至答案打架'问题。"""
    rows = conn.execute(
        "SELECT stem FROM questions WHERE source_node_id=?", (knowledge_id,)
    ).fetchall()
    return [r[0] for r in rows if r[0]]


def mine_misconceptions(conn, knowledge_id, options):
    """把这道概念题里每个干扰项标注的认知误区，自动补进误区库(source=llm_mined)。
    这样误区库会随着出题自动变厚，而不是只靠人工种的那几条。已存在的不重复插。"""
    added = 0
    for o in options or []:
        if o.get("is_correct"):
            continue
        text = (o.get("misconception") or "").strip()
        if not text:
            continue
        exists = conn.execute(
            "SELECT 1 FROM misconceptions WHERE knowledge_id=? AND misconception_text=?",
            (knowledge_id, text),
        ).fetchone()
        if not exists:
            conn.execute(
                "INSERT INTO misconceptions (knowledge_id, misconception_text, source) VALUES (?,?,?)",
                (knowledge_id, text, "llm_mined"),
            )
            added += 1
    return added


def parse_llm_json(raw_text: str) -> dict:
    """容错解析：有的模型会用 ```json 代码块包裹，剥掉围栏再解析。"""
    try:
        return json.loads(raw_text)
    except json.JSONDecodeError:
        cleaned = raw_text.strip()
        if cleaned.startswith("```"):
            cleaned = cleaned.strip("`")
            cleaned = cleaned.split("\n", 1)[1] if "\n" in cleaned else cleaned
        # 再兜底：截取第一个 { 到最后一个 }
        if "{" in cleaned and "}" in cleaned:
            cleaned = cleaned[cleaned.index("{"): cleaned.rindex("}") + 1]
        return json.loads(cleaned)


def build_rag_block(kp, use_rag=True):
    """⭐ RAG：出题前先检索资料。

    检索三样东西塞进 Prompt：
      · 相关知识点 → 让模型能出**跨知识点**的题（以前只看得到一个知识点）
      · 范例题（真题优先）→ 让模型学**真题的出题手法**，而不是复述教材
      · 常见误区 → 做干扰项

    索引没建 / 出错时**返回空字符串**，出题照常进行（只是退回到老的单知识点模式）。
    也就是说：RAG 是**增强**，不是**依赖**。这样即使 RAG 挂了，系统也不会崩。
    """
    if not use_rag:
        return ""
    try:
        from pipeline.rag import retrieve_for_generation, format_context
        ctx = format_context(retrieve_for_generation(kp))
        if not ctx.strip():
            return ""
        return ("\n【检索到的参考资料】\n"
                "（以下是从知识库里检索出来的相关材料，请结合它们出题）\n"
                + ctx + "\n\n")
    except Exception as e:  # noqa: BLE001
        print(f"   [RAG] 检索失败，退回单知识点模式：{e}")
        return ""


def rag_ground_check(stem, answer, use_rag=True):
    """⭐ 防幻觉：出完题反查知识库，看答案在教材里有没有依据。

    比赛大纲第5页：「通过 RAG 技术解决大模型幻觉问题」——这就是那一步。
    返回一句提示（写进 calc_verify_detail 里），出题不因此驳回，只做标记。
    """
    if not use_rag:
        return None
    try:
        from pipeline.rag import verify_answer
        ok, hits, score = verify_answer(stem, answer)
        if ok:
            return None
        return (f"[RAG防幻觉] 这道题在教材里找不到对应依据（相似度仅{score:.2f}），"
                f"可能是模型编的，建议人工看一眼")
    except Exception:
        return None


def suggest_bloom(kp):
    """根据知识点难度/类型给一个 Bloom 建议值，模型可在此基础上自行调整。"""
    kt = kp.get("knowledge_type") or ""
    diff = kp.get("difficulty") or ""
    if any(x in kt for x in ["公式", "计算", "方法", "步骤"]):
        return "应用"
    if diff in ("困难", "进阶"):
        return "分析"
    if diff in ("基础",):
        return "记忆"
    return "理解"


def generate_concept_question(conn, kp, provider_name, use_rag=True):
    misconceptions = fetch_misconceptions(conn, kp["knowledge_id"])
    bloom = suggest_bloom(kp)
    avoid_stems = fetch_existing_stems(conn, kp["knowledge_id"])
    system_prompt, user_prompt = concept_question_prompt.build_prompt(
        kp, misconceptions, bloom_level=bloom, avoid_stems=avoid_stems,
        rag_block=build_rag_block(kp, use_rag))
    client = get_client("concept", provider_name=provider_name)
    raw = client.chat(system_prompt, user_prompt)
    q = parse_llm_json(raw)
    check = validate_concept_question(q)

    # RAG 防幻觉：反查知识库，看这道题在教材里有没有依据
    warn = rag_ground_check(q.get("stem"), q.get("answer"), use_rag)
    if warn:
        check.setdefault("errors", []).append(warn)

    question_id = f"Q_{uuid.uuid4().hex[:10]}"
    conn.execute(
        """INSERT INTO questions (
            question_id, course_chapter, source_node_id, question_type, stem,
            options_json, answer, explanation, bloom_level, generation_model,
            prompt_template_id, review_status, calc_verify_status, calc_verify_detail,
            subjective_difficulty
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (question_id, kp["chapter"], kp["knowledge_id"], "单选",
         q.get("stem"), json.dumps(q.get("options"), ensure_ascii=False),
         q.get("answer"), q.get("explanation"), q.get("bloom_level") or bloom,
         client.label, "concept_v1",
         "待审核" if check["passed"] else "已驳回",
         "无需验算", "; ".join(check["errors"]) if check["errors"] else None,
         kp["difficulty"]),
    )
    conn.execute(
        "INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) VALUES (?,?)",
        (question_id, kp["knowledge_id"]),
    )
    mined = mine_misconceptions(conn, kp["knowledge_id"], q.get("options"))
    check["mined"] = mined
    return question_id, check


def generate_multichoice_question(conn, kp, provider_name, use_rag=True):
    """出多选题。

    【为什么补这个】用户发现 AI 出的题里**一道多选都没有** —— 因为我压根没写多选的 Prompt。
    教材里那 91 道多选是导进来的，不是 AI 出的。是我漏了。

    多选题的价值全在**干扰项**：必须"似是而非"（学生真会犯的错），
    而不是一眼假的废话。判分规则很严（错一个就 0 分），所以干扰项质量决定这题的区分度。
    """
    misconceptions = fetch_misconceptions(conn, kp["knowledge_id"])
    bloom = suggest_bloom(kp)
    system_prompt, user_prompt = multichoice_question_prompt.build_prompt(
        kp, misconceptions, bloom_level=bloom,
        rag_block=build_rag_block(kp, use_rag))
    client = get_client("concept", provider_name=provider_name)
    raw = client.chat(system_prompt, user_prompt)
    q = parse_llm_json(raw)

    opts = q.get("options") or []
    n_correct = sum(1 for o in opts if o.get("is_correct"))
    errors = []
    if len(opts) < 4:
        errors.append("选项少于4个")
    if n_correct < 2:
        errors.append(f"正确选项只有 {n_correct} 个（多选题至少要2个）")
    if n_correct == len(opts):
        errors.append("所有选项都是对的（这不是多选题）")
    passed = not errors

    correct_texts = [o.get("text") for o in opts if o.get("is_correct")]
    question_id = f"Q_{uuid.uuid4().hex[:10]}"
    conn.execute(
        """INSERT INTO questions (
            question_id, course_chapter, source_node_id, question_type, stem,
            options_json, answer, explanation, bloom_level, generation_model,
            prompt_template_id, review_status, calc_verify_status, calc_verify_detail,
            subjective_difficulty, total_score
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (question_id, kp["chapter"], kp["knowledge_id"], "多选",
         q.get("stem"), json.dumps(opts, ensure_ascii=False),
         "；".join(correct_texts), q.get("explanation"), q.get("bloom_level") or bloom,
         client.label, "multichoice_v1",
         "待审核" if passed else "已驳回",
         "无需验算", "; ".join(errors) if errors else None,
         kp["difficulty"], 4),
    )
    conn.execute(
        "INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) VALUES (?,?)",
        (question_id, kp["knowledge_id"]),
    )
    mined = mine_misconceptions(conn, kp["knowledge_id"], opts)
    return question_id, {"passed": passed, "errors": errors, "mined": mined,
                         "calc_status": "无需验算"}


def generate_computation_question(conn, kp, provider_name, use_rag=True):
    system_prompt, user_prompt = computation_question_prompt.build_prompt(
        kp, rag_block=build_rag_block(kp, use_rag))
    client = get_client("computation", provider_name=provider_name)
    raw = client.chat(system_prompt, user_prompt)
    q = parse_llm_json(raw)
    check = validate_computation_question(q)

    question_id = f"Q_{uuid.uuid4().hex[:10]}"
    # 计算题也要存采分点和满分 —— 学生反馈"答案只有一个数字"就是因为以前没存这些
    calc_rubric = q.get("rubric") or []
    calc_total = sum(int(p.get("score", 0)) for p in calc_rubric) or 10
    conn.execute(
        """INSERT INTO questions (
            question_id, course_chapter, source_node_id, question_type, stem,
            options_json, answer, explanation, bloom_level, generation_model,
            prompt_template_id, review_status, calc_verify_status, calc_verify_detail,
            subjective_difficulty, rubric_json, total_score
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (question_id, kp["chapter"], kp["knowledge_id"], "计算",
         q.get("stem"), json.dumps(q.get("calculation_steps"), ensure_ascii=False),
         q.get("answer"), q.get("explanation"), q.get("bloom_level") or "应用",
         client.label, "computation_v2",
         "待审核" if check["passed"] else "已驳回",
         check["calc_status"], check["detail"],
         kp["difficulty"],
         json.dumps(calc_rubric, ensure_ascii=False) if calc_rubric else None,
         calc_total),
    )
    conn.execute(
        "INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) VALUES (?,?)",
        (question_id, kp["knowledge_id"]),
    )
    return question_id, check


def generate_subjective_question(conn, kp, provider_name, kind, use_rag=True):
    """出【名词解释】或【简述题】——完全按老师给的历年真题的样式（自带采分点）。
    kind: 'noun'(名词解释,4分) 或 'brief'(简述题,10分)"""
    # 已出过的，避免重复
    qtype = "名词解释" if kind == "noun" else "简述"
    existing = [r[0] for r in conn.execute(
        "SELECT stem FROM questions WHERE question_type=? AND course_chapter=?",
        (qtype, kp["chapter"]))]

    content = kp.get("content") or ""
    if kp.get("formulas"):
        content += f"\n【相关公式】{kp['formulas']}"

    rag = build_rag_block(kp, use_rag)
    if kind == "noun":
        sysp, userp = subjective_question_prompt.build_noun_prompt(
            kp["knowledge_title"], kp["knowledge_id"], content, existing,
            rag_block=rag)
        total = 4
    else:
        sysp, userp = subjective_question_prompt.build_brief_prompt(
            kp["knowledge_title"], kp["knowledge_id"], content, existing,
            rag_block=rag)
        total = 10

    client = get_client("concept", provider_name=provider_name)
    q = parse_llm_json(client.chat(sysp, userp))

    rubric = q.get("rubric") or []
    rub_sum = sum(int(p.get("score", 0)) for p in rubric)
    errors = []
    if not q.get("stem"):
        errors.append("缺题干")
    if not q.get("answer"):
        errors.append("缺标准答案")
    if not rubric:
        errors.append("缺采分点")
    elif rub_sum != total:
        errors.append(f"采分点总分{rub_sum}≠满分{total}")
    passed = not errors

    # 确保表有这几列
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    for c, ddl in [("rubric_json", "TEXT"), ("total_score", "INTEGER"),
                   ("source", "TEXT DEFAULT 'AI生成'")]:
        if c not in cols:
            conn.execute(f"ALTER TABLE questions ADD COLUMN {c} {ddl}")

    question_id = f"S_{uuid.uuid4().hex[:10]}"
    conn.execute(
        """INSERT INTO questions (
            question_id, course_chapter, source_node_id, question_type, stem,
            options_json, answer, explanation, bloom_level, generation_model,
            prompt_template_id, review_status, subjective_difficulty,
            rubric_json, total_score, source
        ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (question_id, kp["chapter"], kp["knowledge_id"], qtype,
         q.get("stem"), None, q.get("answer"), q.get("explanation"),
         q.get("bloom_level") or ("记忆" if kind == "noun" else "理解"),
         client.label, f"subjective_{kind}_v1",
         "待审核" if passed else "已驳回", kp.get("difficulty"),
         json.dumps(rubric, ensure_ascii=False), total, "AI生成"))
    conn.execute(
        "INSERT OR IGNORE INTO question_knowledge_map (question_id, knowledge_id) VALUES (?,?)",
        (question_id, kp["knowledge_id"]))
    return question_id, {"passed": passed, "errors": errors}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--chapter", help="按章节筛选，如 第一章_绪论")
    parser.add_argument("--knowledge_id", help="只针对单个知识点出题")
    parser.add_argument("--limit", type=int, default=5, help="最多处理多少个知识点")
    parser.add_argument("--type",
                        choices=["concept", "multichoice", "computation",
                                 "noun", "brief", "auto"],
                        default="auto",
                        help="concept=单选; multichoice=多选; computation=计算; "
                             "noun=名词解释(4分); brief=简述题(10分); "
                             "auto=有公式出计算题否则出单选。")
    parser.add_argument("--no-rag", action="store_true",
                        help="不用RAG检索（退回老的单知识点模式）。默认是开着RAG的。")
    parser.add_argument("--provider", default=None,
                        help="临时指定用哪个已登记模型（如 deepseek/xinghuo/kimi），不填则用config.roles默认值")
    args = parser.parse_args()

    # RAG 索引没建的话，自动建一个（几秒钟，不联网不花钱）
    if not args.no_rag:
        import os as _os
        from pipeline.rag import INDEX_PATH, build_index
        if not _os.path.exists(INDEX_PATH):
            print("RAG 索引还没建，正在建（几秒钟）...")
            build_index(verbose=False)
        print("🔍 RAG 已开启：出题前会先检索相关知识点 + 真题范例")
    else:
        print("⚠️  RAG 已关闭（--no-rag），退回单知识点模式")

    conn = connect_database()
    kps = fetch_knowledge_points(conn, chapter=args.chapter, knowledge_id=args.knowledge_id, limit=args.limit)
    if not kps:
        print("没有匹配到知识点，检查 --chapter / --knowledge_id 参数是否正确")
        return

    print(f"本次使用模型：{args.provider or '(config.roles 默认)'}，共处理 {len(kps)} 个知识点\n")
    passed_count, failed_count, mined_total = 0, 0, 0
    for kp in kps:
        q_type = args.type
        if q_type == "auto":
            q_type = "computation" if kp.get("formulas") else "concept"
        try:
            use_rag = not args.no_rag
            if q_type == "computation":
                qid, check = generate_computation_question(conn, kp, args.provider, use_rag)
            elif q_type == "multichoice":
                qid, check = generate_multichoice_question(conn, kp, args.provider, use_rag)
                mined_total += check.get("mined", 0)
            elif q_type in ("noun", "brief"):
                qid, check = generate_subjective_question(conn, kp, args.provider,
                                                          q_type, use_rag)
            else:
                qid, check = generate_concept_question(conn, kp, args.provider, use_rag)
                mined_total += check.get("mined", 0)
            status = "通过" if check["passed"] else f"未通过({check['errors']})"
            print(f"[{q_type}] {kp['knowledge_id']} {kp['knowledge_title']} -> {qid} 质检{status}")
            passed_count += check["passed"]; failed_count += not check["passed"]
        except Exception as e:  # noqa: BLE001
            print(f"[错误] {kp['knowledge_id']} 生成失败: {e}")
            failed_count += 1

    conn.commit(); conn.close()
    print(f"\n本次：质检通过 {passed_count} 道，未通过/需人工核查 {failed_count} 道，"
          f"自动补入误区库 {mined_total} 条")


if __name__ == "__main__":
    main()
