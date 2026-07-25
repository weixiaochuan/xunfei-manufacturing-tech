"""
Local quiz + feedback server (Phase 2 interactive demo).

Why a tiny server instead of just a static HTML?
    The static HTML can only show the *stored* explanation. To get the real
    Phase-2 value — LLM process-level feedback grounded in the knowledge base —
    the page has to call the feedback module, which needs Python + your model key.
    This server does exactly that, using only Python's built-in http.server
    (no Flask, nothing extra to install).

Run:
    python3 demo/serve_quiz.py
    # then open the printed URL in your browser (usually http://127.0.0.1:8000)

    # to use LLM feedback (needs a working key in config), add a provider:
    python3 demo/serve_quiz.py --provider deepseek
    # to stay offline (no model calls), it just falls back to stored explanations.

Endpoints:
    GET  /                      -> the quiz page
    GET  /api/questions         -> approved questions (answers stripped for 单选)
    POST /api/answer            -> {question_id, answer, mode} -> feedback JSON
    GET  /api/progress          -> demo_student progress
    GET  /api/class             -> class error hotspots (teacher view)
"""
import argparse
import json
import os
import sqlite3
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, BASE_DIR)
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()

from feedback.feedback import (generate_feedback, student_progress,  # noqa
                               class_error_hotspots, answer_followup,
                               teacher_overview, teaching_export)

PROVIDER = None      # set from CLI
USE_LLM = True
STUDENT_ID = "demo_student"   # 助学组接入后，由他们把真实学生ID传进来

PAGE_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "quiz_page.html")
TEACHER_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "teacher_dashboard.html")


_CH_NUM = {"一": 1, "二": 2, "三": 3, "四": 4, "五": 5, "六": 6, "七": 7,
           "八": 8, "九": 9, "十": 10}


def _chapter_no(chapter):
    """把'第三章_机床夹具设计'解析成 3，供前端按一~七正确排序（用户反馈：章节顺序乱）。"""
    if not chapter:
        return 99
    for cn, num in _CH_NUM.items():
        if chapter.startswith(f"第{cn}章"):
            return num
    return 99


def approved_questions():
    conn = connect_database()
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        "SELECT question_id, course_chapter, source_node_id, question_type, stem, "
        "options_json, answer, explanation, bloom_level, generation_model, image_path, "
        "total_score, source FROM questions "
        "WHERE review_status='已通过' "
        # 【重要】只给学生"有标准答案"的题。没答案的真题(usage_scope='教师出题')
        # 只在教师端的出题素材里出现——给学生做的话，反馈必然是错的。
        "AND COALESCE(usage_scope,'学生练习')='学生练习' "
        "ORDER BY course_chapter"
    ).fetchall()
    conn.close()
    out = []
    for r in rows:
        d = dict(r)
        opts = json.loads(d["options_json"]) if d["options_json"] else None
        # 单选和多选都要把选项发给前端（之前只发单选的，
        # 所以多选题在界面上没有选项、退化成了填空框——用户反馈的bug）
        if d["question_type"] in ("单选", "多选") and opts and isinstance(opts[0], dict):
            options = [o.get("text") for o in opts]      # 不发 is_correct，防止学生看答案
        else:
            options = None
        out.append({
            "id": d["question_id"], "chapter": d["course_chapter"], "node": d["source_node_id"],
            "type": d["question_type"], "stem": d["stem"], "options": options,
            "bloom": d["bloom_level"] or "", "model": d["generation_model"] or "",
            "image": ("/" + d["image_path"]) if d.get("image_path") else None,
            "chapter_no": _chapter_no(d["course_chapter"]),
            "total_score": d.get("total_score"),
            "is_real": (d.get("source") == "真题"),
            "src": d.get("source") or "AI生成",     # 真题 / 教材习题 / AI生成
            "scan": bool(d.get("question_id","").startswith("P_")),   # 原卷截图题
            # ⚠️ 注意：answer_image 绝对不能放进题目列表！
            # 学生还没提交就看到答案，这道题就废了（用户抓到的bug）。
            # 答案图只在 /api/answer 的批改结果里返回。
            # 【重要】题目列表里**不发任何答案**。
            # 以前这里把 answer / steps / explanation 都发给了前端（除了单选题），
            # 也就是说学生按 F12 就能直接看到标准答案 —— 这是严重泄题。
            # 前端本来也不用这几个字段（答案是提交后从 /api/answer 拿的），
            # 所以直接删掉，没有副作用。
        })
    return out


class Handler(BaseHTTPRequestHandler):
    def _send(self, code, body, ctype="application/json"):
        self.send_response(code)
        self.send_header("Content-Type", f"{ctype}; charset=utf-8")
        # CORS：别的组的前端（助学端/助教端）要能跨域调我们的接口。
        # 没有这几个头，浏览器会直接拦掉请求，对面根本连不上。
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, X-Student-Id")
        self.end_headers()
        if isinstance(body, (dict, list)):
            body = json.dumps(body, ensure_ascii=False)
        self.wfile.write(body.encode("utf-8"))

    def _send_file(self, rel):
        """静态资源（题目配图）。图片存磁盘、库里只存路径——见 db/add_image_support.py。"""
        full = os.path.normpath(os.path.join(BASE_DIR, rel))
        if not full.startswith(BASE_DIR) or not os.path.exists(full):
            self._send(404, {"error": "not found"})
            return
        ext = os.path.splitext(full)[1].lower()
        ctype = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg",
                 ".svg": "image/svg+xml", ".gif": "image/gif"}.get(ext, "application/octet-stream")
        with open(full, "rb") as f:
            data = f.read()
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *a):
        pass  # quiet

    def _student(self, payload=None):
        """取学生 ID。

        【为什么重要】以前 /api/answer 里写死了 "demo_student"，
        也就是**所有学生的作答记录会混在一起**。接入助学端之后，
        张三做的题会算到李四头上，掌握度、推题全乱。

        取值顺序：请求体 student_id -> URL 参数 ?student= -> 请求头 X-Student-Id -> demo_student
        助学端只要在调用时带上其中任一即可。
        """
        if payload and payload.get("student_id"):
            return str(payload["student_id"])
        qs = parse_qs(urlparse(self.path).query)
        if qs.get("student_id"):
            return qs["student_id"][0]
        if qs.get("student"):
            return qs["student"][0]
        hdr = self.headers.get("X-Student-Id")
        if hdr:
            return hdr
        return "demo_student"

    def do_OPTIONS(self):
        """浏览器跨域 POST 之前会先发 OPTIONS 预检请求。不回它，POST 就发不出来。"""
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, X-Student-Id")
        self.end_headers()

    def do_GET(self):
        path = urlparse(self.path).path
        if path == "/" or path == "/index.html":
            with open(PAGE_PATH, encoding="utf-8") as f:
                self._send(200, f.read(), "text/html")
        elif path == "/teacher":
            with open(TEACHER_PATH, encoding="utf-8") as f:
                self._send(200, f.read(), "text/html")
        elif path == "/api/questions":
            self._send(200, approved_questions())
        elif path == "/api/progress":
            self._send(200, student_progress(self._student()))
        elif path == "/api/class":
            self._send(200, class_error_hotspots())
        elif path == "/api/teacher":
            qs = parse_qs(urlparse(self.path).query)
            self._send(200, teacher_overview(
                chapter=(qs.get("chapter") or [None])[0],
                student_id=(qs.get("student") or [None])[0]))
        elif path == "/api/teaching_export":
            qs = parse_qs(urlparse(self.path).query)
            self._send(200, teaching_export(chapter=(qs.get("chapter") or [None])[0]))
        elif path == "/api/exam_pool":
            # 教师出题素材库：历年真题里没有标准答案的那些题。
            # 学生端看不到（没答案没法给反馈），但对老师出题很有价值。
            qs = parse_qs(urlparse(self.path).query)
            conn = connect_database()
            conn.row_factory = sqlite3.Row
            sql = ("SELECT question_id, course_chapter, question_type, stem, total_score, "
                   "image_path, no_answer_reason, answer, exam_source, "
                   "COALESCE(usage_scope,'学生练习') AS usage_scope FROM questions "
                   "WHERE source='真题' ORDER BY usage_scope DESC, course_chapter")
            rows = [dict(r) for r in conn.execute(sql)]
            conn.close()
            out = []
            for d in rows:
                out.append({
                    "id": d["question_id"], "chapter": d["course_chapter"],
                    "type": d["question_type"], "stem": d["stem"],
                    "total_score": d["total_score"],
                    "image": ("/" + d["image_path"]) if d.get("image_path") else None,
                    "usage": d["usage_scope"],
                    # 出处分两段：短的直接显示，全的点开看（用户："有不有点长？"）
                    "exam_source": (d.get("exam_source") or "").split("|")[0] or "历年真题",
                    "exam_source_full": (d.get("exam_source") or "").split("|")[-1],
                    "has_answer": d["usage_scope"] == "学生练习",
                    "answer": d.get("answer") if d["usage_scope"] == "学生练习" else None,
                    "why_no_answer": d.get("no_answer_reason"),
                })
            self._send(200, {"items": out})
        elif path == "/api/recommend":
            qs = parse_qs(urlparse(self.path).query)
            from pipeline.recommend import recommend, GOALS, DEFAULT_GOAL
            n = int((qs.get("n") or ["5"])[0])
            goal = (qs.get("goal") or [DEFAULT_GOAL])[0]
            qtype = (qs.get("qtype") or [None])[0]
            # 用请求里带的 student_id（不能用全局的，否则所有学生推的题都一样）
            items = recommend(self._student(), n=n, explain=True, goal=goal, qtype=qtype)
            self._send(200, {"items": items, "goal": goal,
                             "goal_desc": GOALS.get(goal, {}).get("desc", "")})
        elif path == "/api/wrong_book":
            # ⭐ 错题本：学生做错的题，按知识点归堆
            from feedback.feedback import wrong_book
            qs = parse_qs(urlparse(self.path).query)
            allq = (qs.get("all") or ["0"])[0] == "1"
            self._send(200, wrong_book(self._student(), only_unmastered=not allq))
        elif path == "/api/config":
            self._send(200, {"use_llm": USE_LLM, "provider": PROVIDER})
        elif path.startswith("/assets/"):
            self._send_file(path.lstrip("/"))
        else:
            self._send(404, {"error": "not found"})

    def do_POST(self):
        path = urlparse(self.path).path
        length = int(self.headers.get("Content-Length", 0))
        payload = json.loads(self.rfile.read(length) or "{}")
        try:
            if path == "/api/answer":
                fb = generate_feedback(
                    payload["question_id"], payload.get("answer", ""),
                    student_id=self._student(payload),
                    mode=payload.get("mode", "练习"),
                    provider_name=PROVIDER, use_llm=USE_LLM)
                self._send(200, fb)
            elif path == "/api/reinforce_summary":
                from feedback.feedback import reinforce_summary
                out = reinforce_summary(
                    payload["knowledge_id"], payload.get("results", []),
                    student_id=self._student(payload),
                    provider_name=PROVIDER, use_llm=USE_LLM)
                self._send(200, out)
            elif path == "/api/ask":
                out = answer_followup(
                    payload["question_id"], payload.get("message", ""),
                    was_correct=payload.get("was_correct"), provider_name=PROVIDER)
                self._send(200, out)
            else:
                self._send(404, {"error": "not found"})
        except Exception as e:
            self._send(500, {"error": str(e)})


def main():
    global PROVIDER, USE_LLM
    ap = argparse.ArgumentParser()
    ap.add_argument("--provider", default=None, help="用哪个模型生成反馈(如 deepseek)；不填则离线")
    ap.add_argument("--no-llm", action="store_true", help="强制离线，不调模型")
    ap.add_argument("--port", type=int, default=8000)
    a = ap.parse_args()
    PROVIDER = a.provider
    USE_LLM = (not a.no_llm) and (a.provider is not None)

    n = len(approved_questions())
    mode = f"LLM反馈(模型={a.provider})" if USE_LLM else "离线反馈(不调模型)"
    print("=" * 58)
    print(f"  学-练-考 · 答题+反馈 本地服务已启动")
    print(f"  题库：{n} 道已通过审核的题")
    print(f"  反馈模式：{mode}")
    print(f"  学生端(答题)： http://127.0.0.1:{a.port}")
    print(f"  教师端(看板)： http://127.0.0.1:{a.port}/teacher")
    print(f"  停止：在这个命令行窗口按 Ctrl + C")
    print("=" * 58)
    ThreadingHTTPServer(("127.0.0.1", a.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
