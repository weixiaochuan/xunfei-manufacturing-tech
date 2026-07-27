"""
Answer judging + layered feedback (Phase 2 core, v2). See prompts/feedback_prompt.py.

Improvements in v2 (from user feedback):
  * Offline fallback is now HONEST: shows the stored explanation + misconception and
    clearly labels itself "离线简版", instead of dressing up a template as real
    process-level feedback.
  * Subjective calculation questions are GRADED like a teacher (step + result points)
    when a model is available; offline they still show reference answer + steps.
  * New: interactive follow-up (你的疑问/你的反思) -> answer_followup().
  * New: related questions for "趁热打铁" -> related_questions() returns 2-3 approved
    questions on the same knowledge point (or same chapter) for immediate practice.
  * Person/voice fixed in the prompt (talks to "你").

Two modes (report 5.4): 练习 gives full feedback; 测试 records right/wrong only.
LLM feedback is optional (needs a provider); everything degrades gracefully offline.
"""
import re
import json
import os
import sqlite3
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()


SUBJECTIVE_TYPES = {"计算", "简述", "名词解释", "作图"}   # 都按采分点制批改


def _norm(s):
    return (s or "").strip().replace(" ", "")


def _conn():
    conn = connect_database()
    conn.row_factory = sqlite3.Row
    return conn


def _resolve_pick(options, student_answer):
    if not (options and isinstance(options, list) and options and isinstance(options[0], dict)):
        return None, student_answer
    letters = ["A", "B", "C", "D", "E", "F"]
    sa = str(student_answer).strip()
    picked = None
    if sa.upper() in letters and letters.index(sa.upper()) < len(options):
        picked = options[letters.index(sa.upper())]
    elif sa.isdigit() and int(sa) < len(options):
        picked = options[int(sa)]
    else:
        for o in options:
            if _norm(o.get("text")) == _norm(sa):
                picked = o
                break
    return picked, (picked.get("text") if picked else student_answer)


def _knowledge_content(conn, knowledge_id):
    row = conn.execute(
        "SELECT knowledge_title, content, formulas FROM knowledge_points WHERE knowledge_id=?",
        (knowledge_id,)).fetchone()
    if not row:
        return None, None
    content = row["content"] or ""
    if row["formulas"]:
        content += f"\n【相关公式】{row['formulas']}"
    return row["knowledge_title"], content


def _options_display(options):
    if not (options and isinstance(options, list) and options and isinstance(options[0], dict)):
        return "（计算题，无选项）"
    letters = ["A", "B", "C", "D", "E", "F"]
    return "\n".join(f"{letters[i]}. {o.get('text')}" for i, o in enumerate(options))


def _get_client(provider_name):
    from llm.client import get_client
    return get_client("concept", provider_name=provider_name)


def _parse_json(raw):
    from pipeline.generate_questions import parse_llm_json
    return parse_llm_json(raw)


def _llm_choice_feedback(question, student_text, misconception, knowledge_id, content, provider):
    try:
        from prompts import feedback_prompt
        client = _get_client(provider)
        sysp, userp = feedback_prompt.build_prompt(question, student_text, misconception,
                                                   knowledge_id, content)
        data = _parse_json(client.chat(sysp, userp, temperature=0.3))
        return {"error_cause": data.get("error_cause"),
                "explanation": data.get("explanation"),   # LLM 重写的解析（讲为什么，不复述原文）
                "action_suggestion": data.get("action_suggestion"),
                "review_node": data.get("review_node") or knowledge_id,
                "source": "llm", "model": getattr(client, "label", "llm")}
    except Exception as e:
        return {"_error": str(e)}


def _llm_grade_calc(question, student_answer, reference, explanation, content, provider,
                    rubric=None, total=None, qtype="计算"):
    """按【采分点制】批改主观题（评分方式学自本课程历年真题的参考答案）。"""
    try:
        from prompts import feedback_prompt
        client = _get_client(provider)
        sysp, userp = feedback_prompt.build_grade_prompt(
            question, student_answer, reference, explanation, content,
            rubric=rubric, total=total, qtype=qtype)
        data = _parse_json(client.chat(sysp, userp, temperature=0.2))
        return {"score": data.get("score"), "total_score": data.get("total_score") or total,
                "rubric_check": data.get("rubric_check"),
                "correct_points": data.get("correct_points"),
                "lost_points": data.get("lost_points"), "suggestion": data.get("suggestion"),
                "source": "llm", "model": getattr(client, "label", "llm")}
    except Exception as e:
        return {"_error": str(e)}


def generate_feedback(question_id, student_answer, student_id="demo_student",
                      mode="练习", time_seconds=None, provider_name=None,
                      use_llm=False, conn=None):
    """Judge + feedback for one attempt. Records to student_answers and updates mastery."""
    own = conn is None
    if own:
        conn = _conn()
    row = conn.execute("SELECT * FROM questions WHERE question_id=?", (question_id,)).fetchone()
    if not row:
        if own:
            conn.close()
        raise ValueError(f"题目不存在: {question_id}")
    q = dict(row)
    options = json.loads(q["options_json"]) if q.get("options_json") else None
    knowledge_id = q["source_node_id"]
    is_calc = q["question_type"] in SUBJECTIVE_TYPES

    # =============== 计算题：评分（步骤分+结果分）===============
    if is_calc:
        title, content = _knowledge_content(conn, knowledge_id)
        reference = q.get("answer")
        steps = None
        if q.get("options_json"):
            try:
                steps = json.loads(q["options_json"])
            except Exception:
                steps = None
        reference_full = (("参考答案：" + str(reference) + "\n") if reference else "") + \
                         ("\n".join(steps) if isinstance(steps, list) else "")
        rubric = None
        if q.get("rubric_json"):
            try:
                rubric = json.loads(q["rubric_json"])
            except Exception:
                rubric = None
        total_score = q.get("total_score") or (4 if q["question_type"] == "名词解释" else 10)
        fb = {"question_id": question_id, "question_type": q["question_type"],
              "reference_answer": reference, "reference_steps": steps,
              "rubric": rubric, "total_score": total_score,
              "explanation": q.get("explanation"), "review_knowledge_id": knowledge_id,
              "knowledge_title": title, "mode": mode,
              "is_real_exam": (q.get("source") == "真题"),
              # 原卷截图题：标准答案是一张图（含尺寸链、公式、采分点）。
              # **只在这里返回**——学生提交之后才看得到。
              "answer_image": ("/" + q["answer_image_path"]) if q.get("answer_image_path") else None}
        graded = None
        if use_llm and mode != "测试":
            graded = _llm_grade_calc({"stem": q["stem"]}, student_answer, reference_full,
                                     q.get("explanation"), content, provider_name,
                                     rubric=rubric, total=total_score,
                                     qtype=q["question_type"])
        if graded and "_error" not in graded:
            fb.update({"graded": True, "score": graded["score"],
                       "total_score": graded.get("total_score") or total_score,
                       "rubric_check": graded.get("rubric_check"),
                       "correct_points": graded["correct_points"],
                       "lost_points": graded["lost_points"],
                       "suggestion": graded["suggestion"], "feedback_source": "llm",
                       "feedback_model": graded.get("model")})
            # 及格线按满分的 60% 算（满分不再固定是100）
            _t = graded.get("total_score") or total_score or 10
            got = (graded.get("score") or 0) >= 0.6 * _t
        else:
            fb.update({"graded": False, "feedback_source": "offline",
                       "note": "离线模式，不自动批改。加粗处即采分点，对照自评。"})
            if graded and "_error" in graded:
                fb["_llm_error"] = graded["_error"]
            got = None
        fb["reinforce"] = related_questions(knowledge_id, q["course_chapter"],
                                            exclude=question_id, n=3, conn=conn,
                                            qtype=q["question_type"],
                                            bloom=q["bloom_level"],
                                            student_id=student_id)
        _record(conn, student_id, question_id, mode, student_answer,
                (1 if got else 0) if got is not None else None, time_seconds, None, fb)
        if got is not None:
            _update_mastery(conn, student_id, knowledge_id, got)
        conn.commit()
        if own:
            conn.close()
        return fb

    # =============== 多选题：判分 ===============
    if q["question_type"] == "多选":
        correct = {_norm(o["text"]) for o in (options or []) if o.get("is_correct")}
        picked = {_norm(x) for x in re.split(r"[;；,，|]", student_answer or "") if x.strip()}
        total = q["total_score"] or 4
        # 判分规则（老师定的）：全答对=满分；答对一部分（且没答错的）=一半分；但凡错一个=0分
        if picked and picked <= correct:
            score = total if picked == correct else total / 2
        else:
            score = 0
        fb = {"question_id": question_id, "question_type": "多选",
              "is_correct": score == total,
              "score": score, "total_score": total,
              "your_answer": "；".join(sorted(picked)),
              "correct_answer": q["answer"],
              "review_knowledge_id": knowledge_id, "mode": mode,
              "grade_note": ("全部答对" if score == total else
                             ("答对一部分（没有答错的），得一半分" if score else
                              "有答错的选项，本题不得分")),
              "reinforce": related_questions(knowledge_id, q["course_chapter"],
                                             exclude=question_id, n=3, conn=conn,
                                             qtype=q["question_type"],
                                             bloom=q["bloom_level"],
                                             student_id=student_id)}
        _record(conn, student_id, question_id, mode, fb["your_answer"],
                1 if score == total else 0, time_seconds, None, fb)
        _update_mastery(conn, student_id, knowledge_id, score == total)
        conn.commit()
        if own:
            conn.close()
        return fb

    # =============== 单选题：判分 + 反馈 ===============
    picked_option, picked_text = _resolve_pick(options, student_answer)
    is_correct = _norm(picked_text) == _norm(q["answer"])
    misconception = (picked_option or {}).get("misconception") if picked_option else None

    fb = {"question_id": question_id, "question_type": "单选", "is_correct": is_correct,
          "your_answer": picked_text, "correct_answer": q["answer"],
          "review_knowledge_id": knowledge_id, "mode": mode}

    if mode == "测试":
        _record(conn, student_id, question_id, mode, picked_text, is_correct,
                time_seconds, misconception, {"is_correct": is_correct})
        _update_mastery(conn, student_id, knowledge_id, is_correct)
        conn.commit()
        if own:
            conn.close()
        return {"question_id": question_id, "is_correct": is_correct, "mode": "测试",
                "note": "测试模式：解析在交卷后统一展示"}

    fb["explanation"] = q["explanation"]
    if not is_correct:
        title, content = _knowledge_content(conn, knowledge_id)
        fb["knowledge_title"] = title
        if misconception:
            fb["your_misconception"] = misconception
        llm = None
        if use_llm:
            qd = {"stem": q["stem"], "answer": q["answer"], "options_display": _options_display(options)}
            llm = _llm_choice_feedback(qd, picked_text, misconception, knowledge_id, content, provider_name)
        if llm and "_error" not in llm:
            # LLM 重写的解析质量更高（讲清为什么），优先用它替换库里存的旧解析
            if llm.get("explanation"):
                fb["explanation"] = llm["explanation"]
            fb["process_feedback"] = llm.get("error_cause")
            fb["action_suggestion"] = llm.get("action_suggestion")
            fb["review_node"] = llm.get("review_node")
            fb["feedback_source"] = "llm"
            fb["feedback_model"] = llm.get("model")
        else:
            fb["offline_misconception"] = misconception
            fb["feedback_source"] = "offline"
            fb["note"] = "离线简版反馈：以上为出题时预存的解析与误区标注。开启LLM(--provider)后可获得针对你这次作答的错因分析与学习建议。"
            fb["review_node"] = knowledge_id
            if llm and "_error" in llm:
                fb["_llm_error"] = llm["_error"]
        fb["reinforce"] = related_questions(knowledge_id, q["course_chapter"],
                                            exclude=question_id, n=3, conn=conn,
                                            qtype=q["question_type"],
                                            bloom=q["bloom_level"],
                                            student_id=student_id)

    _record(conn, student_id, question_id, mode, picked_text, is_correct,
            time_seconds, misconception, fb)
    _update_mastery(conn, student_id, knowledge_id, is_correct)
    conn.commit()
    if own:
        conn.close()
    return fb


def answer_followup(question_id, student_message, was_correct=None, provider_name=None, conn=None):
    """The '你的疑问/你的反思' box: student says what they don't get, LLM replies (RAG-anchored)."""
    own = conn is None
    if own:
        conn = _conn()
    row = conn.execute("SELECT * FROM questions WHERE question_id=?", (question_id,)).fetchone()
    if not row:
        if own:
            conn.close()
        raise ValueError(f"题目不存在: {question_id}")
    q = dict(row)
    title, content = _knowledge_content(conn, q["source_node_id"])
    if own:
        conn.close()
    correctness = "我答对了" if was_correct else ("我答错了" if was_correct is False else "（未知）")
    try:
        from prompts import feedback_prompt
        client = _get_client(provider_name)
        sysp, userp = feedback_prompt.build_ask_prompt(
            {"stem": q["stem"], "answer": q["answer"], "explanation": q.get("explanation")},
            student_message, correctness, content)
        reply = client.chat(sysp, userp, temperature=0.4, response_json=False)
        return {"reply": reply.strip(), "source": "llm", "model": getattr(client, "label", "llm")}
    except Exception as e:
        return {"reply": "（需要连上模型才能回答你的追问。启动服务器时用 --provider 指定一个模型，例如 deepseek。）",
                "source": "offline", "_error": str(e)}


def reinforce_summary(knowledge_id, results, student_id="demo_student",
                     provider_name=None, use_llm=False, conn=None):
    """趁热打铁做完后的小结（用户要求）：
    results = [{"question_id":..., "is_correct": True/False}, ...]
    给出：这轮巩固的掌握情况 + 错因总结 + 下一步建议。
    有模型时由 LLM 基于该知识点原文生成；没有模型时给出基于数据的诚实小结。"""
    own = conn is None
    if own:
        conn = _conn()
    title, content = _knowledge_content(conn, knowledge_id)
    total = len(results)
    correct = sum(1 for r in results if r.get("is_correct"))
    # 收集这轮答错的题干与命中的误区，作为错因素材
    wrong_items = []
    for r in results:
        if r.get("is_correct"):
            continue
        row = conn.execute("SELECT stem, answer, options_json FROM questions WHERE question_id=?",
                           (r.get("question_id"),)).fetchone()
        if row:
            wrong_items.append({"stem": row["stem"], "answer": row["answer"],
                                "your_answer": r.get("your_answer")})
    mastery = conn.execute(
        "SELECT mastery_prob FROM student_knowledge_mastery WHERE student_id=? AND knowledge_id=?",
        (student_id, knowledge_id)).fetchone()
    mastery_prob = mastery["mastery_prob"] if mastery else None
    if own:
        conn.close()

    base = {"knowledge_id": knowledge_id, "knowledge_title": title,
            "total": total, "correct": correct,
            "mastery_prob": mastery_prob}

    if not use_llm or not wrong_items:
        # 诚实的离线小结：不硬凑"深度分析"
        if correct == total:
            base["summary"] = f"这轮 {total} 道全对，「{title}」这个知识点你已经掌握得不错了。"
            base["advice"] = "可以往下一个知识点走了。"
        else:
            base["summary"] = f"这轮 {total} 道对了 {correct} 道。「{title}」还需要再巩固。"
            base["advice"] = f"建议回看知识点 {knowledge_id} 的原文，重点看你答错的那几道对应的概念。"
        base["source"] = "offline"
        return base

    # LLM 版：基于原文+这轮错题，做错因总结与建议
    try:
        from prompts import feedback_prompt
        client = _get_client(provider_name)
        sysp, userp = feedback_prompt.build_summary_prompt(
            title, knowledge_id, content, total, correct, wrong_items)
        data = _parse_json(client.chat(sysp, userp, temperature=0.4))
        base.update({"summary": data.get("summary"), "error_pattern": data.get("error_pattern"),
                     "advice": data.get("advice"), "source": "llm",
                     "model": getattr(client, "label", "llm")})
    except Exception as e:
        base.update({"summary": f"这轮 {total} 道对了 {correct} 道。",
                     "advice": f"建议回看知识点 {knowledge_id} 的原文。",
                     "source": "offline", "_error": str(e)})
    return base


def _weakness_type(conn, student_id, knowledge_id, qtype):
    """这个学生这道题做错了，到底是【概念没懂】还是【这个题型不行】？

    用户提的问题很准："生产系统"名词解释错了，要区分：
      ① 他是名词解释这个题型不行  -> 应该推**别的知识点的名词解释**（练题型）
      ② 还是生产系统这个概念没理解 -> 应该推**同一个知识点的其它题型**（换个角度考同一个概念）
    这两种情况该推的题完全不一样。

    判断依据（用学生自己的历史数据）：
      · 他在**这个知识点**上的正确率（低 -> 概念没懂）
      · 他在**这个题型**上的整体正确率（低 -> 题型不行）
    两个都低就都练；数据不够就默认按"概念没懂"处理（更常见）。
    """
    row = conn.execute(
        "SELECT AVG(CASE WHEN is_correct=1 THEN 1.0 ELSE 0 END) r, COUNT(*) n "
        "FROM student_answers sa JOIN questions q ON q.question_id=sa.question_id "
        "WHERE sa.student_id=? AND q.source_node_id=? AND sa.is_correct IS NOT NULL",
        (student_id, knowledge_id)).fetchone()
    kp_rate, kp_n = (row[0], row[1]) if row and row[1] else (None, 0)

    row = conn.execute(
        "SELECT AVG(CASE WHEN is_correct=1 THEN 1.0 ELSE 0 END) r, COUNT(*) n "
        "FROM student_answers sa JOIN questions q ON q.question_id=sa.question_id "
        "WHERE sa.student_id=? AND q.question_type=? AND sa.is_correct IS NOT NULL",
        (student_id, qtype)).fetchone()
    qt_rate, qt_n = (row[0], row[1]) if row and row[1] else (None, 0)

    concept_weak = (kp_n < 2) or (kp_rate is not None and kp_rate < 0.6)
    type_weak = (qt_n >= 3 and qt_rate is not None and qt_rate < 0.5)
    return concept_weak, type_weak


def related_questions(knowledge_id, chapter=None, exclude=None, n=3, conn=None,
                      qtype=None, bloom=None, student_id="demo_student"):
    """趁热打铁：做错之后，推几道真正的同类题。

    【之前是假的，用户抓到了】
    老代码是 `WHERE source_node_id=? LIMIT 3`，没排序没过滤没随机；
    而导入真题时又把整章的题全挂在该章第一个知识点上 ->
    每道题推出来永远是同样那 3 道。**是糊弄。**

    【现在的规则】
    1. **先判断学生是"概念没懂"还是"题型不行"**（见 _weakness_type）：
       · 概念没懂 -> 推**同知识点、换题型**的题（换个角度考同一个概念，巩固理解）
       · 题型不行 -> 推**同题型、别的知识点**的题（专练这个题型）
       · 两个都弱 -> 各推一半
    2. **计算题只推计算题**（计算能力和概念记忆是两码事，跨题型没意义）
    3. 同 Bloom 认知层级的优先
    4. **找不到就不推**（宁可没有，不拿不相干的题糊弄）
    """
    own = conn is None
    if own:
        conn = _conn()
    conn.row_factory = sqlite3.Row

    concept_weak, type_weak = _weakness_type(conn, student_id, knowledge_id, qtype)

    base = ("SELECT question_id, question_type, stem, course_chapter, bloom_level, "
            "source_node_id FROM questions WHERE review_status='已通过' "
            "AND COALESCE(usage_scope,'学生练习')='学生练习' AND question_id!=?")

    def fetch(where, params, limit=12):
        return list(conn.execute(base + where + " ORDER BY RANDOM() LIMIT ?",
                                 [exclude or ""] + params + [limit]))

    picked, seen = [], set()

    def add(rows, want_bloom=False, cap=None):
        for r in rows:
            if len(picked) >= (cap or n) or r["question_id"] in seen:
                continue
            if want_bloom and bloom and r["bloom_level"] != bloom:
                continue
            seen.add(r["question_id"])
            picked.append(r)

    if qtype == "计算":
        # 计算题：只推计算题，同知识点优先，然后同章节
        add(fetch(" AND question_type='计算' AND source_node_id=?", [knowledge_id]))
        if len(picked) < n and chapter:
            add(fetch(" AND question_type='计算' AND course_chapter=?", [chapter]))
    else:
        # A. 概念没懂 -> 同知识点、**换个题型**（同一个概念换个角度考）
        if concept_weak:
            cap = n if not type_weak else max(1, n // 2)
            add(fetch(" AND source_node_id=? AND question_type!=? AND question_type!='计算'",
                      [knowledge_id, qtype]), cap=cap)
            # 换题型的题不够，就用同知识点同题型的补
            if len(picked) < cap:
                add(fetch(" AND source_node_id=? AND question_type!='计算'",
                          [knowledge_id]), cap=cap)
        # B. 题型不行 -> **同题型**、别的知识点（专练这个题型）
        if type_weak and len(picked) < n:
            add(fetch(" AND question_type=? AND source_node_id!=? AND course_chapter=?",
                      [qtype, knowledge_id, chapter or ""]))
        # C. 兜底：同章节、同 Bloom
        if len(picked) < n and chapter:
            add(fetch(" AND course_chapter=? AND question_type!='计算'", [chapter]), True)
        if len(picked) < n and chapter:
            add(fetch(" AND course_chapter=? AND question_type!='计算'", [chapter]))

    if own:
        conn.close()
    if not picked:
        return []      # 没有同类题 -> 一道都不推

    why = ("这个概念再从别的角度练两道" if concept_weak and not type_weak else
           "这个题型多练两道" if type_weak and not concept_weak else
           "概念和题型都再巩固一下")
    return [{"question_id": r["question_id"], "type": r["question_type"],
             "stem": (r["stem"][:60] + ("…" if len(r["stem"]) > 60 else "")),
             "why": why}
            for r in picked[:n]]


def _record(conn, student_id, question_id, mode, picked_text, is_correct,
            time_seconds, misconception, fb):
    conn.execute(
        """INSERT INTO student_answers
           (student_id, question_id, mode, student_answer, is_correct,
            time_seconds, error_type_tag, feedback_json)
           VALUES (?,?,?,?,?,?,?,?)""",
        (student_id, question_id, mode, picked_text,
         (None if is_correct is None else (1 if is_correct else 0)),
         time_seconds, misconception, json.dumps(fb, ensure_ascii=False)))


def _update_mastery(conn, student_id, knowledge_id, is_correct):
    if not knowledge_id:
        return
    row = conn.execute(
        "SELECT mastery_prob FROM student_knowledge_mastery WHERE student_id=? AND knowledge_id=?",
        (student_id, knowledge_id)).fetchone()
    target = 1.0 if is_correct else 0.0
    alpha = 0.4
    if row is None:
        new = 0.5 + (target - 0.5) * alpha
        conn.execute("INSERT INTO student_knowledge_mastery (student_id, knowledge_id, mastery_prob, updated_at) "
                     "VALUES (?,?,?,datetime('now'))", (student_id, knowledge_id, round(new, 4)))
    else:
        new = row["mastery_prob"] + (target - row["mastery_prob"]) * alpha
        conn.execute("UPDATE student_knowledge_mastery SET mastery_prob=?, updated_at=datetime('now') "
                     "WHERE student_id=? AND knowledge_id=?", (round(new, 4), student_id, knowledge_id))


def student_progress(student_id, conn=None):
    own = conn is None
    if own:
        conn = _conn()
    total = conn.execute("SELECT COUNT(*) FROM student_answers WHERE student_id=?", (student_id,)).fetchone()[0]
    correct = conn.execute("SELECT COUNT(*) FROM student_answers WHERE student_id=? AND is_correct=1", (student_id,)).fetchone()[0]
    weak = conn.execute(
        """SELECT q.source_node_id, kp.knowledge_title, COUNT(*) wrong
           FROM student_answers a JOIN questions q ON a.question_id=q.question_id
           LEFT JOIN knowledge_points kp ON q.source_node_id=kp.knowledge_id
           WHERE a.student_id=? AND a.is_correct=0
           GROUP BY q.source_node_id ORDER BY wrong DESC LIMIT 5""", (student_id,)).fetchall()
    if own:
        conn.close()
    return {"student_id": student_id, "answered": total, "correct": correct,
            "accuracy": round(correct / total, 3) if total else None,
            "weak_knowledge_points": [{"knowledge_id": w[0], "title": w[1], "wrong_count": w[2]} for w in weak]}


def class_error_hotspots(conn=None, limit=10):
    own = conn is None
    if own:
        conn = _conn()
    kp = conn.execute(
        """SELECT q.source_node_id, kp.knowledge_title,
                  SUM(CASE WHEN a.is_correct=0 THEN 1 ELSE 0 END) wrong, COUNT(*) total
           FROM student_answers a JOIN questions q ON a.question_id=q.question_id
           LEFT JOIN knowledge_points kp ON q.source_node_id=kp.knowledge_id
           GROUP BY q.source_node_id HAVING total>0
           ORDER BY (1.0*wrong/total) DESC, wrong DESC LIMIT ?""", (limit,)).fetchall()
    mis = conn.execute(
        """SELECT error_type_tag, COUNT(*) c FROM student_answers
           WHERE is_correct=0 AND error_type_tag IS NOT NULL
           GROUP BY error_type_tag ORDER BY c DESC LIMIT ?""", (limit,)).fetchall()
    if own:
        conn.close()
    return {"hardest_knowledge_points": [
                {"knowledge_id": r[0], "title": r[1], "wrong": r[2], "total": r[3],
                 "error_rate": round(r[2] / r[3], 3)} for r in kp],
            "top_misconceptions": [{"misconception": r[0], "count": r[1]} for r in mis]}


if __name__ == "__main__":
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("--question_id", required=True)
    ap.add_argument("--answer", required=True)
    ap.add_argument("--mode", default="练习", choices=["练习", "测试"])
    ap.add_argument("--provider", default=None)
    ap.add_argument("--no-llm", action="store_true")
    a = ap.parse_args()
    out = generate_feedback(a.question_id, a.answer, mode=a.mode, provider_name=a.provider,
                            use_llm=not a.no_llm)
    print(json.dumps(out, ensure_ascii=False, indent=2))

# ---------------------------------------------------------------- 教师端聚合
MISCONCEPTION_CATEGORIES = [
    ("概念理解不到位", ["混淆", "误以为", "误认为", "不理解", "概念", "定义", "区分", "分不清"]),
    ("公式/原理记忆不准", ["公式", "原理", "定理", "记错", "记忆"]),
    ("计算或单位失误", ["计算", "单位", "换算", "舍入", "精度", "数值"]),
    ("适用条件判断错误", ["条件", "适用", "场合", "前提", "假设"]),
    ("知识范围/层级混淆", ["范畴", "层级", "属于", "分类", "包含"]),
]


def _categorize(text):
    """把一条误区文本归到一个大类里（供教师端按'原因'分类，用户要求）。"""
    t = text or ""
    for name, kws in MISCONCEPTION_CATEGORIES:
        if any(k in t for k in kws):
            return name
    return "其他"


def teacher_overview(conn=None, chapter=None, student_id=None, limit=10):
    """教师端主数据：支持按章节 / 按学生下钻（用户要求的分类）。
    返回 KPI、最难知识点、误区(按大类归并)、章节错误率、每日趋势。"""
    own = conn is None
    if own:
        conn = _conn()
    where, params = ["1=1"], []
    if chapter:
        where.append("q.course_chapter=?"); params.append(chapter)
    if student_id:
        where.append("a.student_id=?"); params.append(student_id)
    W = " AND ".join(where)

    kpi = conn.execute(
        f"""SELECT COUNT(*) total,
                   SUM(CASE WHEN a.is_correct=0 THEN 1 ELSE 0 END) wrong,
                   COUNT(DISTINCT a.student_id) students,
                   COUNT(DISTINCT q.source_node_id) nodes
            FROM student_answers a JOIN questions q ON a.question_id=q.question_id
            WHERE {W}""", params).fetchone()

    hardest = conn.execute(
        f"""SELECT q.source_node_id, kp.knowledge_title, q.course_chapter,
                   SUM(CASE WHEN a.is_correct=0 THEN 1 ELSE 0 END) wrong, COUNT(*) total
            FROM student_answers a JOIN questions q ON a.question_id=q.question_id
            LEFT JOIN knowledge_points kp ON q.source_node_id=kp.knowledge_id
            WHERE {W} GROUP BY q.source_node_id HAVING total>0
            ORDER BY (1.0*wrong/total) DESC, wrong DESC LIMIT ?""", params + [limit]).fetchall()

    by_chapter = conn.execute(
        f"""SELECT q.course_chapter,
                   SUM(CASE WHEN a.is_correct=0 THEN 1 ELSE 0 END) wrong, COUNT(*) total
            FROM student_answers a JOIN questions q ON a.question_id=q.question_id
            WHERE {W} GROUP BY q.course_chapter ORDER BY q.course_chapter""", params).fetchall()

    trend = conn.execute(
        f"""SELECT substr(a.answered_at,1,10) d,
                   SUM(CASE WHEN a.is_correct=0 THEN 1 ELSE 0 END) wrong, COUNT(*) total
            FROM student_answers a JOIN questions q ON a.question_id=q.question_id
            WHERE {W} GROUP BY d ORDER BY d""", params).fetchall()

    mis_rows = conn.execute(
        f"""SELECT a.error_type_tag, q.course_chapter, kp.knowledge_title, COUNT(*) c
            FROM student_answers a JOIN questions q ON a.question_id=q.question_id
            LEFT JOIN knowledge_points kp ON q.source_node_id=kp.knowledge_id
            WHERE {W} AND a.is_correct=0 AND a.error_type_tag IS NOT NULL
            GROUP BY a.error_type_tag ORDER BY c DESC""", params).fetchall()
    # 按"原因大类"归并，再挂具体知识点（用户要求：先章节、再原因、再细化到知识点）
    cats = {}
    for r in mis_rows:
        cat = _categorize(r["error_type_tag"])
        d = cats.setdefault(cat, {"category": cat, "count": 0, "items": []})
        d["count"] += r["c"]
        d["items"].append({"misconception": r["error_type_tag"], "count": r["c"],
                           "chapter": r["course_chapter"], "knowledge_title": r["knowledge_title"]})
    categories = sorted(cats.values(), key=lambda x: -x["count"])

    students = conn.execute(
        f"""SELECT a.student_id,
                   COUNT(*) total, SUM(CASE WHEN a.is_correct=1 THEN 1 ELSE 0 END) correct
            FROM student_answers a JOIN questions q ON a.question_id=q.question_id
            WHERE {W} GROUP BY a.student_id ORDER BY total DESC""", params).fetchall()

    bank = conn.execute(
        "SELECT course_chapter, COUNT(*) n FROM questions WHERE review_status='已通过' "
        "GROUP BY course_chapter").fetchall()
    all_chapters = [r[0] for r in conn.execute(
        "SELECT DISTINCT course_chapter FROM questions ORDER BY course_chapter")]
    if own:
        conn.close()

    total = kpi["total"] or 0
    return {
        "kpi": {"answers": total, "wrong": kpi["wrong"] or 0, "students": kpi["students"] or 0,
                "nodes": kpi["nodes"] or 0,
                "error_rate": round((kpi["wrong"] or 0) / total, 3) if total else 0},
        "hardest": [{"knowledge_id": r[0], "title": r[1], "chapter": r[2],
                     "wrong": r[3], "total": r[4], "error_rate": round(r[3]/r[4], 3)}
                    for r in hardest],
        "by_chapter": [{"chapter": r[0], "wrong": r[1], "total": r[2],
                        "error_rate": round(r[1]/r[2], 3) if r[2] else 0} for r in by_chapter],
        "trend": [{"date": r[0], "wrong": r[1], "total": r[2],
                   "error_rate": round(r[1]/r[2], 3) if r[2] else 0} for r in trend],
        "misconception_categories": categories,
        "students": [{"student_id": r[0], "total": r[1], "correct": r[2],
                      "accuracy": round(r[2]/r[1], 3) if r[1] else 0} for r in students],
        "bank": [{"chapter": r[0], "n": r[1]} for r in bank],
        "all_chapters": all_chapters,
    }


def teaching_export(conn=None, chapter=None, top=5):
    """输出给【助教组】做PPT/教案的结构化数据（用户要求：反作用于助教端）。
    给出：本章最该讲的薄弱知识点 + 学生高发误区 + 建议讲解重点。助教直接拿这个生成课件。"""
    ov = teacher_overview(conn=conn, chapter=chapter)
    focus = []
    for h in ov["hardest"][:top]:
        mis = []
        for cat in ov["misconception_categories"]:
            for it in cat["items"]:
                if it.get("knowledge_title") == h["title"]:
                    mis.append({"category": cat["category"], "misconception": it["misconception"],
                                "count": it["count"]})
        focus.append({
            "knowledge_id": h["knowledge_id"], "knowledge_title": h["title"],
            "chapter": h["chapter"], "error_rate": h["error_rate"],
            "wrong": h["wrong"], "total": h["total"],
            "student_misconceptions": mis,
            "teaching_hint": f"该知识点错误率 {h['error_rate']*100:.0f}%，建议课上重点讲解，"
                             f"并针对上述误区做对比澄清。",
        })
    return {"scope": chapter or "全部章节",
            "class_error_rate": ov["kpi"]["error_rate"],
            "answers": ov["kpi"]["answers"],
            "focus_points": focus,
            "misconception_categories": [
                {"category": c["category"], "count": c["count"]} for c in ov["misconception_categories"]],
            "note": "此结构可直接用于生成课件/教案：focus_points 即建议的讲解重点。"}



def wrong_book(student_id, conn=None, only_unmastered=True):
    """⭐ 错题本。

    【为什么要有】学生刷完题，最有价值的复习材料就是**他自己做错的题**。
    以前没有这个功能——做错了，反馈看一眼就过去了，再也找不回来。

    【"掌握了"的判断】
    一道错题什么时候可以从错题本里划掉？
      · 重做对了 → 划掉（但保留记录，标成"已攻克"）
      · 还没重做 / 重做还是错 → 留在错题本里

    所以我们看这道题**最近一次**的作答：对了就是攻克了，错了就还欠着。

    only_unmastered=True  只看还没攻克的（默认，学生要复习的就是这些）
    only_unmastered=False 全部（包括已攻克的，可以看成长记录）
    """
    close = conn is None
    if conn is None:
        conn = connect_database()
    conn.row_factory = sqlite3.Row

    # 每道题只看最近一次作答（用 answer_id 最大的那条）
    rows = [dict(r) for r in conn.execute(
        """
        SELECT q.question_id, q.course_chapter, q.question_type, q.stem,
               q.total_score, q.image_path, q.source, q.bloom_level,
               q.source_node_id,
               kp.knowledge_title,
               sa.student_answer, sa.is_correct, sa.answered_at,
               sa.error_type_tag,
               (SELECT COUNT(*) FROM student_answers x
                 WHERE x.student_id = sa.student_id
                   AND x.question_id = sa.question_id) AS attempts
          FROM student_answers sa
          JOIN questions q ON q.question_id = sa.question_id
     LEFT JOIN knowledge_points kp ON kp.knowledge_id = q.source_node_id
         WHERE sa.student_id = ?
           AND sa.answer_id = (
                 SELECT MAX(y.answer_id) FROM student_answers y
                  WHERE y.student_id = sa.student_id
                    AND y.question_id = sa.question_id)
           AND sa.is_correct IS NOT NULL
      ORDER BY sa.answered_at DESC
        """, (student_id,))]

    items, mastered = [], []
    for r in rows:
        rec = {
            "question_id": r["question_id"],
            "chapter": r["course_chapter"],
            "type": r["question_type"],
            "stem": r["stem"],
            "total_score": r["total_score"],
            "image": ("/" + r["image_path"]) if r["image_path"] else None,
            "src": r["source"] or "AI生成",
            "node": r["source_node_id"],
            "knowledge_title": r["knowledge_title"],
            "your_answer": r["student_answer"],
            "attempts": r["attempts"],
            "answered_at": r["answered_at"],
            "error_tag": r["error_type_tag"],
        }
        if r["is_correct"]:
            rec["status"] = "已攻克"      # 最近一次做对了
            mastered.append(rec)
        else:
            rec["status"] = "待攻克"
            items.append(rec)

    # 按知识点归堆：让学生看清"我到底哪个概念反复错"
    by_node = {}
    for it in items:
        k = it["knowledge_title"] or it["chapter"]
        by_node.setdefault(k, []).append(it)
    weak = sorted(by_node.items(), key=lambda x: -len(x[1]))

    out = {
        "student_id": student_id,
        "todo": items,                    # 还没攻克的错题（要复习的）
        "mastered": mastered if not only_unmastered else [],
        "todo_count": len(items),
        "mastered_count": len(mastered),
        "weak_points": [                  # 最该补的知识点
            {"knowledge": k, "wrong_count": len(v),
             "question_ids": [x["question_id"] for x in v]}
            for k, v in weak[:5]
        ],
    }
    if close:
        conn.close()
    return out
