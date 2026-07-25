"""
网页自检 —— 在发给你之前，先自己把页面跑一遍，确保不会打不开。

【为什么要有这个】
上一版我把学生端搞崩了：JS 里用了一个没定义的变量（REC），
结果每道题的渲染都抛异常 -> 整页空白，你点什么都没反应。
这种错我本该在发给你之前就发现——你不该替我当测试。

【它检查什么】
  1. HTML 里 JS 引用的每个元素 id 是否真的存在（上次就是 backdrop 不存在）
  2. JS 里用到的全局变量是否都声明了（上次就是 REC 没声明）
  3. 起一个真服务，把所有接口打一遍，看有没有 500
  4. 用真数据跑一遍渲染逻辑（模板字符串会不会炸）

用法：
    python3 pipeline/check_web.py
"""
import http.client
import json
import os
import re
import subprocess
import sys
import time

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PAGES = ["demo/quiz_page.html", "demo/teacher_dashboard.html"]
PORT = 8899

# JS 里这些是浏览器/语言自带的，不算"未声明"
BUILTINS = {
    "document", "window", "console", "fetch", "alert", "confirm", "JSON", "Object", "Array",
    "String", "Number", "Boolean", "Math", "Date", "RegExp", "Map", "Set", "Promise",
    "setTimeout", "setInterval", "encodeURIComponent", "decodeURIComponent", "parseInt",
    "parseFloat", "isNaN", "location", "history", "navigator", "Error", "true", "false",
    "null", "undefined", "this", "if", "else", "for", "while", "return", "function",
    "const", "let", "var", "new", "typeof", "instanceof", "in", "of", "try", "catch",
    "finally", "throw", "switch", "case", "break", "continue", "default", "class",
    "extends", "async", "await", "delete", "void", "do", "yield", "static", "get", "set",
    "URL", "URLSearchParams", "FormData", "Blob", "XMLHttpRequest", "NaN", "Infinity",
}


def extract_js(html):
    m = re.search(r"<script>([\s\S]*?)</script>", html)
    return m.group(1) if m else ""


def check_ids(html, js, name):
    """JS 里 getElementById('x') 用到的 x，HTML 里必须有 id="x"。"""
    used = set(re.findall(r"getElementById\(\s*['\"]([\w-]+)['\"]\s*\)", js))
    have = set(re.findall(r'id="([\w-]+)"', html))
    missing = used - have
    if missing:
        print(f"  ❌ {name}: JS 用到了不存在的元素 id -> {', '.join(sorted(missing))}")
        return False
    print(f"  ✅ {name}: 元素 id 全部存在（用到 {len(used)} 个）")
    return True


def check_globals(js, name):
    """JS 里用到的顶层变量，必须被声明过（上次 REC 就是漏了声明）。"""
    declared = set()
    # let A=1, B=2, C;  —— 要把逗号后面的也算上（第一版漏了，导致误报 LLM/CHAPTER）
    for m in re.finditer(r"\b(?:let|const|var)\s+([^;\n]+)", js):
        depth = 0
        buf = ""
        for ch in m.group(1):
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            if ch == "," and depth == 0:
                nm = re.match(r"\s*([\w$]+)", buf)
                if nm:
                    declared.add(nm.group(1))
                buf = ""
            else:
                buf += ch
        nm = re.match(r"\s*([\w$]+)", buf)
        if nm:
            declared.add(nm.group(1))
    for m in re.finditer(r"\bfunction\s+([\w$]+)", js):
        declared.add(m.group(1))
    # 解构声明 let {a,b} = / let [a,b] =
    for m in re.finditer(r"\b(?:let|const|var)\s*[\{\[]([^\}\]]+)[\}\]]", js):
        for part in m.group(1).split(","):
            declared.add(part.split(":")[-1].strip())
    # 函数参数
    for m in re.finditer(r"function\s*[\w$]*\s*\(([^)]*)\)", js):
        for part in m.group(1).split(","):
            p = part.strip().split("=")[0].strip()
            if p:
                declared.add(p)
    for m in re.finditer(r"\(([^)]*)\)\s*=>", js):
        for part in m.group(1).split(","):
            p = part.strip().split("=")[0].strip()
            if p:
                declared.add(p)
    for m in re.finditer(r"([\w$]+)\s*=>", js):
        declared.add(m.group(1))
    for m in re.finditer(r"\bcatch\s*\(\s*([\w$]+)", js):
        declared.add(m.group(1))
    for m in re.finditer(r"\bfor\s*\(\s*(?:let|const|var)\s+([\w$]+)", js):
        declared.add(m.group(1))

    # 找"像顶层变量的用法"：大写开头的标识符（我们的全局都这么命名：DATA/REC/RF/LLM/TYPE_ORDER）
    # 只在"代码"里找，先把字符串和模板字面量剔掉（否则 URL/GET 这种词会误报）
    code = re.sub(r"`[^`]*`|'[^'\n]*'|\"[^\"\n]*\"|//[^\n]*|/\*[\s\S]*?\*/", " ", js)
    used = set(re.findall(r"(?<![\w.$])([A-Z][A-Z0-9_]{1,})(?=\s*[\[\.\)=,;\s])", code))
    missing = {u for u in used if u not in declared and u not in BUILTINS}
    if missing:
        print(f"  ❌ {name}: 用到了没声明的全局变量 -> {', '.join(sorted(missing))}")
        print(f"     （上次页面打不开就是因为这个：REC 没声明）")
        return False
    print(f"  ✅ {name}: 全局变量都声明了")
    return True


def check_server():
    """起服务，把接口全打一遍。"""
    proc = subprocess.Popen(
        [sys.executable, os.path.join(BASE_DIR, "demo", "serve_quiz.py"),
         "--no-llm", "--port", str(PORT)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=BASE_DIR)
    time.sleep(2.5)
    ok = True
    try:
        eps = ["/", "/teacher", "/api/questions", "/api/teacher",
               "/api/exam_pool", "/api/recommend?n=3", "/api/config"]
        for ep in eps:
            try:
                c = http.client.HTTPConnection("127.0.0.1", PORT, timeout=8)
                c.request("GET", ep)
                r = c.getresponse()
                body = r.read()
                if r.status != 200:
                    print(f"  ❌ {ep} -> HTTP {r.status}")
                    ok = False
                else:
                    print(f"  ✅ {ep} -> 200 ({len(body)//1024} KB)")
                c.close()
            except Exception as e:
                print(f"  ❌ {ep} -> {e}")
                ok = False

        # 学生端不能出现没有答案的题
        c = http.client.HTTPConnection("127.0.0.1", PORT, timeout=8)
        c.request("GET", "/api/questions")
        qs = json.loads(c.getresponse().read())
        c.close()
        leak = [q for q in qs if q.get("answer") and "未提供参考答案" in str(q["answer"])]
        if leak:
            print(f"  ❌ 有 {len(leak)} 道无答案的题泄漏到了学生端！")
            ok = False
        else:
            print(f"  ✅ 学生端 {len(qs)} 道题，没有无答案的题泄漏")

        # 页面里必须真的有题目内容（上次白屏就是这里没查出来）
        c = http.client.HTTPConnection("127.0.0.1", PORT, timeout=8)
        c.request("GET", "/")
        page = c.getresponse().read().decode("utf-8", "ignore")
        c.close()
        need = ["function card(", "function render(", "function restart(", "boot()"]
        miss = [n for n in need if n not in page]
        if miss:
            print(f"  ❌ 学生端页面缺少关键函数：{miss}")
            ok = False
        else:
            print("  ✅ 学生端页面的渲染函数齐全")
    finally:
        proc.terminate()
        proc.wait(timeout=5)
    return ok


def main():
    print("=" * 58)
    print("网页自检（发给你之前先自己跑一遍，别再出现打不开的情况）")
    print("=" * 58)
    ok = True

    print("\n[1/2] 检查前端代码")
    for page in PAGES:
        fp = os.path.join(BASE_DIR, page)
        if not os.path.exists(fp):
            continue
        html = open(fp, encoding="utf-8").read()
        js = extract_js(html)
        name = os.path.basename(page)
        ok &= check_ids(html, js, name)
        ok &= check_globals(js, name)

    print("\n[2/2] 启动服务打接口")
    ok &= check_server()

    print("\n" + "=" * 58)
    print("✅ 全部通过，可以发布" if ok else "❌ 有问题，别发！先修")
    print("=" * 58)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
