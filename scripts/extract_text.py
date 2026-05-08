print("=== PDF文本提取脚本开始运行 ===")

import os
from pathlib import Path
import fitz

print("当前工作目录:", os.getcwd())

pdf_path = Path("data/raw/制造工艺学.pdf")
print("PDF完整路径:", pdf_path.absolute())
print("PDF文件是否存在:", pdf_path.exists())

if pdf_path.exists():
    print("PDF文件已找到，正在打开...")
    doc = fitz.open(pdf_path)
    print(f"PDF打开成功！共 {len(doc)} 页")
    
    # 提取前100字符预览
    if len(doc) > 0:
        preview = doc[0].get_text("text")[:200]
        print("第一页文本预览（前200字符）:\n", preview)
else:
    print("❌ 未找到PDF文件！请检查路径")

print("=== 脚本运行结束 ===")