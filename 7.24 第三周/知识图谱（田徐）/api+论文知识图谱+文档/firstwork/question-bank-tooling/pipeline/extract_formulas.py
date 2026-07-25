"""
把真题里的【公式和零件图】救出来 —— 这是计算题一直做不好的根因。

【问题是什么】
你反馈计算题"缺图、公式提取错误、答案不对"，根因就一个：
    这批 .doc 里的**公式和零件图，都是 OLE 对象**（老版 MathType / 公式编辑器画的），
    在文档里以 **WMF 矢量图**的形式存着。
    python-docx 读文字时**完全看不到它们**，于是：
       "计算工序尺寸和。"   <- "A和B"这两个尺寸符号是公式对象，没了
       "要求为mm。"        <- 尺寸公差是公式对象，没了
    题干缺了关键信息，学生根本没法做，我却还把它收进了题库。

【怎么解决的】（这次去查了资料）
    Word 的公式有两种存法：
      · 新版：OMML（XML，能直接转 LaTeX）—— 但我们这批**一个都没有**
      · 老版：OLE 对象（MathType），存的是二进制 + 一张 **WMF 预览图** —— 我们这批 778 个全是这种
    所以 LaTeX 那条路走不通。**但 WMF 预览图是可以渲染出来的**：
        WMF --(LibreOffice)--> PNG --(裁掉大片白边)--> 可用的图
    我之前试过一次，看到一片白就以为失败了，其实是**内容很小、白边很大**，裁一下就有了。
    这次裁完确认：真的是零件图、真的是公式。

【这个脚本做什么】
    1. 遍历所有真题 docx，把里面每一个 WMF/EMF 抽出来
    2. 用 LibreOffice 转成 PNG
    3. 自动裁掉白边（这一步是关键，不裁就是一片白）
    4. 判断这张图是"公式"还是"插图"：
         · 又扁又小（高<80px、宽高比>2） -> 多半是行内公式
         · 其它 -> 零件图/工序图
    5. 存到 assets/images/real_exams/，并记下"它在文档里的第几个位置"，
       这样导入题目时就能把图挂到**它所属的那道题**上。

用法：
    python3 pipeline/extract_formulas.py --scan     # 看能救出多少
    python3 pipeline/extract_formulas.py --apply    # 真的转（慢，几分钟）
"""
import argparse
import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import zipfile

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCX_DIRS = [os.path.join(BASE_DIR, "data", "real_exams", "docx"),
             os.path.join(BASE_DIR, "data", "real_exams", "docx2"),
             os.path.join(BASE_DIR, "data", "real_exams", "docx3")]
IMG_DIR = os.path.join(BASE_DIR, "assets", "images", "real_exams")
MAP_PATH = os.path.join(BASE_DIR, "data", "real_exams", "media_map.json")


def convert_wmf(data, out_png):
    """WMF/EMF -> PNG，并裁掉白边。裁边这步很关键：不裁就是一张几乎全白的A4。"""
    with tempfile.TemporaryDirectory() as td:
        src = os.path.join(td, "x.wmf")
        with open(src, "wb") as f:
            f.write(data)
        try:
            subprocess.run(
                ["libreoffice", "--headless", "--convert-to", "png", "--outdir", td, src],
                capture_output=True, timeout=90)
        except Exception:
            return False
        png = os.path.join(td, "x.png")
        if not os.path.exists(png):
            return False
        try:
            from PIL import Image
            import numpy as np
            im = Image.open(png).convert("RGB")
            a = np.asarray(im.convert("L"))
            ys, xs = (a < 200).nonzero()
            if len(xs) < 30:              # 几乎没内容 -> 废图
                return False
            pad = 6
            box = (max(0, xs.min() - pad), max(0, ys.min() - pad),
                   min(im.width, xs.max() + pad), min(im.height, ys.max() + pad))
            crop = im.crop(box)
            if crop.width < 20 or crop.height < 10:
                return False
            # 太小的放大一点，网页上看得清
            if crop.width < 300:
                k = min(3, 300 // max(crop.width, 1) + 1)
                crop = crop.resize((crop.width * k, crop.height * k), Image.LANCZOS)
            crop.save(out_png)
            return True
        except Exception:
            return False


def classify(png):
    """这张图是公式还是插图？"""
    try:
        from PIL import Image
        im = Image.open(png)
        w, h = im.size
        if h < 120 and w / max(h, 1) > 2.2:
            return "公式"
        return "插图"
    except Exception:
        return "插图"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scan", action="store_true")
    ap.add_argument("--apply", action="store_true")
    ap.add_argument("--limit", type=int, default=None)
    a = ap.parse_args()

    files = []
    for d in DOCX_DIRS:
        files += sorted(glob.glob(os.path.join(d, "*.docx")))
    if a.limit:
        files = files[:a.limit]

    total_wmf = 0
    for f in files:
        try:
            z = zipfile.ZipFile(f)
            total_wmf += sum(1 for n in z.namelist()
                             if n.startswith("word/media/") and n.lower().endswith((".wmf", ".emf")))
        except Exception:
            pass
    print(f"{len(files)} 份真题里，共有 {total_wmf} 个 WMF/EMF 对象（公式 + 零件图都在这里面）\n")

    if not a.apply:
        print("（--scan 只统计。加 --apply 真的转换，会比较慢，几分钟。）")
        return

    os.makedirs(IMG_DIR, exist_ok=True)
    mapping = {}           # docx文件名 -> {媒体名: {path, kind}}
    ok = fail = 0
    for i, f in enumerate(files, 1):
        base = os.path.basename(f)
        try:
            z = zipfile.ZipFile(f)
        except Exception:
            continue
        got = {}
        for n in z.namelist():
            if not n.startswith("word/media/"):
                continue
            ext = os.path.splitext(n)[1].lower()
            data = z.read(n)
            name = os.path.basename(n)

            if ext in (".png", ".jpg", ".jpeg", ".gif"):
                if len(data) < 4000:
                    continue
                fn = hashlib.md5(data).hexdigest()[:10] + ext
                fp = os.path.join(IMG_DIR, fn)
                if not os.path.exists(fp):
                    with open(fp, "wb") as fh:
                        fh.write(data)
                # 位图也要体检：全白的不要
                try:
                    from PIL import Image
                    import numpy as np
                    arr = np.asarray(Image.open(fp).convert("L"))
                    if (arr < 200).mean() < 0.005:
                        os.remove(fp)
                        continue
                except Exception:
                    pass
                got[name] = {"path": os.path.relpath(fp, BASE_DIR).replace("\\", "/"),
                             "kind": classify(fp)}
                ok += 1

            elif ext in (".wmf", ".emf"):
                if len(data) < 400:            # 太小的是符号碎片
                    continue
                fn = "f" + hashlib.md5(data).hexdigest()[:10] + ".png"
                fp = os.path.join(IMG_DIR, fn)
                if os.path.exists(fp) or convert_wmf(data, fp):
                    got[name] = {"path": os.path.relpath(fp, BASE_DIR).replace("\\", "/"),
                                 "kind": classify(fp)}
                    ok += 1
                else:
                    fail += 1
        if got:
            mapping[base] = got
        print(f"[{i}/{len(files)}] {base}: 救出 {len(got)} 张")

    with open(MAP_PATH, "w", encoding="utf-8") as fh:
        json.dump(mapping, fh, ensure_ascii=False, indent=1)

    kinds = {}
    for d in mapping.values():
        for v in d.values():
            kinds[v["kind"]] = kinds.get(v["kind"], 0) + 1
    print(f"\n成功 {ok} 张，失败 {fail} 张")
    print("类型分布：", kinds)
    print(f"图片存到：{IMG_DIR}")
    print(f"位置映射：{MAP_PATH}（导入题目时用它把图挂到对应的题上）")


if __name__ == "__main__":
    main()
