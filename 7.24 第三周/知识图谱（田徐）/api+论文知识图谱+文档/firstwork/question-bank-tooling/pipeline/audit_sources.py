"""
把 75 份真题**逐份**过一遍，明确告诉用户：**到底哪几个文件需要转成 PDF**。

【为什么要这个】
用户说得对：我一直说"先别转"，但又没读完，等于在拖进度。
这个脚本给出一个**确定的清单**：
  · A 类：解析得好好的，**不用转**
  · B 类：题干里有"公式空洞"（公式被吞掉，只剩顿号/空括号）-> **必须转 PDF**
  · C 类：本来就是空白卷（没答案），转了也没答案 -> **不用转**

判断"公式空洞"的依据（这些都是公式变成图之后留下的痕迹）：
  · "计算工序尺寸和。"        —— 尺寸符号没了
  · "必须限制、、、4个自由度"   —— 顿号串
  · "要求为mm"                —— 数值没了
  · "（）"                    —— 空括号

用法：
    python3 pipeline/audit_sources.py
"""
import glob
import os
import re
import sys

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, BASE_DIR)

# 原始文件名映射（docx/docx2/docx3 是我改过名的，要告诉用户原文件叫什么）
ORIG_MAP = os.path.join(BASE_DIR, "data", "real_exams", "source_map.json")

# 公式被吞掉之后留下的痕迹
HOLE_PATTERNS = [
    (re.compile(r"[、，,]\s*[、，,]\s*[、，,]"), "顿号串（符号被吞）"),
    (re.compile(r"[（(]\s*[)）]"), "空括号（公式被吞）"),
    (re.compile(r"(尺寸|直径|深度|余量|偏差|角度|长度|键槽|公差)\s*和?\s*[。.]"), "句子缺数值就结束了"),
    (re.compile(r"为\s*mm|要求为\s*[。.]|深为\s*mm|至\s*[。.]"), "数值缺失"),
    (re.compile(r"限制\s*[、，,]{2,}"), "自由度符号被吞"),
]


def find_holes(text):
    hits = []
    for pat, name in HOLE_PATTERNS:
        if pat.search(text or ""):
            hits.append(name)
    return hits


def main():
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "imp", os.path.join(BASE_DIR, "pipeline", "import_real_exams.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)

    files = []
    for d, old in [("docx", False), ("docx2", True), ("docx3", True)]:
        for f in sorted(glob.glob(os.path.join(BASE_DIR, "data", "real_exams", d, "*.docx"))):
            files.append((f, old, d))

    need_pdf, clean, blank, broken = [], [], [], []

    for f, old, d in files:
        base = os.path.basename(f)
        try:
            qs, has_ans = m.parse_file(f, old_format=old)
        except Exception as e:
            broken.append((d, base, str(e)[:40]))
            continue

        if not qs:
            blank.append((d, base, "解析不出题目"))
            continue

        # 有答案的题里，有多少道题干有"公式空洞"？
        holed, total_calc = [], 0
        for q in qs:
            if q["type"] == "计算":
                total_calc += 1
            hits = find_holes(q["stem"])
            if hits and q["type"] == "计算":
                holed.append((q["stem"][:42], hits))

        if not has_ans:
            blank.append((d, base, f"空白卷（没有参考答案），{len(qs)}道题"))
        elif holed:
            need_pdf.append((d, base, len(qs), total_calc, holed))
        else:
            clean.append((d, base, len(qs), total_calc))

    print("=" * 72)
    print(f"真题源文件盘点：共 {len(files)} 份")
    print("=" * 72)

    print(f"\n【A类】解析正常，不用转 PDF —— {len(clean)} 份")
    print(f"   （共 {sum(x[2] for x in clean)} 道题，其中计算题 {sum(x[3] for x in clean)} 道）")

    print(f"\n【B类】⚠️ 计算题的题干有【公式空洞】，需要转成 PDF —— {len(need_pdf)} 份")
    if need_pdf:
        print("   转了之后，这些计算题就能救回来：\n")
        for d, base, n, nc, holed in need_pdf:
            print(f"   📄 data/real_exams/{d}/{base}   （{len(holed)} 道计算题受影响）")
            for stem, hits in holed[:2]:
                print(f"        · {stem}…")
                print(f"          问题：{ '、'.join(hits) }")
        print()

    print(f"\n【C类】空白卷 / 没有参考答案，转了也没用 —— {len(blank)} 份")
    if broken:
        print(f"\n【D类】读不出来 —— {len(broken)} 份")
        for d, base, e in broken:
            print(f"   {d}/{base}: {e}")

    # 写一个清单文件，用户可以照着转
    out = os.path.join(BASE_DIR, "需要转PDF的文件清单.txt")
    with open(out, "w", encoding="utf-8") as fh:
        fh.write("需要转成 PDF 的真题文件清单\n")
        fh.write("=" * 50 + "\n\n")
        fh.write("说明：这些文件里的计算题，题干中的公式/尺寸符号在 .doc 里是图片对象，\n")
        fh.write("      程序读不到，导致题干残缺（例如\"计算工序尺寸和。\"）。\n")
        fh.write("      转成 PDF 后公式是渲染好的，文字能完整提取。\n\n")
        fh.write(f"共 {len(need_pdf)} 个文件需要转：\n\n")
        for i, (d, base, n, nc, holed) in enumerate(need_pdf, 1):
            fh.write(f"{i}. data/real_exams/{d}/{base}\n")
            fh.write(f"   受影响的计算题：{len(holed)} 道\n")
            for stem, hits in holed[:3]:
                fh.write(f"     · {stem}…\n")
            fh.write("\n")
        fh.write("\n转好之后，把 PDF 放到 data/real_exams/pdf_new/ 目录里发我就行。\n")
    print(f"\n清单已写到：{out}")


if __name__ == "__main__":
    main()
