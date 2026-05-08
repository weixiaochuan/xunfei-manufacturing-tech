from pathlib import Path
import re

input_file = Path("data/processed/全书_raw_text.txt")
output_dir = Path("data/processed/chapters")
output_dir.mkdir(parents=True, exist_ok=True)

print("正在按章节拆分教材...")

with open(input_file, "r", encoding="utf-8") as f:
    text = f.read()

# 简单按常见章节标题拆分（可根据实际目录调整）
chapters = re.split(r'(第\s*\d+\s*章.*?)\n', text)

current_chapter = ""
chapter_num = 0

for part in chapters:
    if re.match(r'第\s*\d+\s*章', part.strip()):
        if current_chapter:
            chapter_num += 1
            filename = output_dir / f"第{chapter_num:02d}章.md"
            with open(filename, "w", encoding="utf-8") as f:
                f.write(current_chapter.strip())
            print(f"✅ 已保存: {filename.name}")
        current_chapter = part
    else:
        current_chapter += part

# 保存最后一章
if current_chapter:
    chapter_num += 1
    filename = output_dir / f"第{chapter_num:02d}章.md"
    with open(filename, "w", encoding="utf-8") as f:
        f.write(current_chapter.strip())
    print(f"✅ 已保存: {filename.name}")

print(f"\n🎉 拆分完成！共拆出 {chapter_num} 章，保存在 data/processed/chapters/")