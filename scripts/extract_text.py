from pathlib import Path
import fitz
print("=== 脚本开始运行 ===")

pdf_path = Path("data/raw/制造工艺学.pdf")
print(f"PDF路径: {pdf_path}")
print(f"PDF文件是否存在: {pdf_path.exists()}")

if not pdf_path.exists():
    print("❌ PDF文件没找到！")
else:
    print("✅ PDF文件已找到，开始打开...")
    doc = fitz.open(pdf_path)
    print(f"✅ PDF打开成功，共 {len(doc)} 页")
    
    print("脚本运行结束")