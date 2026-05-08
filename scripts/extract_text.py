from pathlib import Path
import fitz
from tqdm import tqdm

pdf_path = Path("data/raw/制造工艺学.pdf")
output_dir = Path("data/processed")
output_dir.mkdir(parents=True, exist_ok=True)

print(f"正在处理: {pdf_path.name}")
doc = fitz.open(pdf_path)

full_text = []
print(f"PDF 共 {len(doc)} 页，开始提取...")

for page_num in tqdm(range(len(doc))):
    text = doc[page_num].get_text("text")
    if text.strip():
        full_text.append(f"--- 第 {page_num+1} 页 ---\n{text}\n")

# 保存全文
with open(output_dir / "全书_raw_text.txt", "w", encoding="utf-8") as f:
    f.write("\n".join(full_text))

print(f"\n✅ 全文提取完成！共 {len(full_text)} 页")
print(f"保存位置: {output_dir / '全书_raw_text.txt'}")