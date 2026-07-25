"""
图片支持（Phase 2 补强）——机械制造这门课很多题离开图就没法做（尺寸链、V形块定位、
公差带、装配简图…）。这个脚本把"图片"这件事的地基打好。

—— 存储方案（这是查过业界通行做法后定的，不是拍脑袋）——
业界共识：**图片不要以二进制塞进数据库**，而是"文件放磁盘/对象存储，数据库只存路径"。
理由：图片是大字段，塞进库会把库撑大、拖慢所有查询；而且存路径以后可以直接换成
CDN/对象存储（OSS/S3），迁移零成本。

所以本项目的方案：
    图片文件  ->  assets/images/<章节>/<文件名>.png      （磁盘上，跟着代码走）
    数据库    ->  questions.image_path 存相对路径，如 "assets/images/ch3/v_block_01.png"
                  knowledge_points.figures 已有字段，也用来存该知识点的配图路径
    展示      ->  答题服务器把 /assets/ 目录暴露成静态路径，网页里 <img src="/assets/...">

将来上线、学生量大了，只要把 image_path 换成对象存储的 URL（如 https://oss.../v_block_01.png），
其它代码一行都不用改——这正是"存路径不存二进制"的好处。

—— 图从哪来？三条路，按可靠性排序 ——
1. 教材/真题原图（最准）：老师给的真题库、教材配图，扫描或截图后放进 assets/images/，
   用本脚本登记到题目上。**这是首选**，因为准确性有教材背书。
2. 代码画图（准确、可控）：像尺寸链、公差带、V形块受力这类是**规则图形**，可以用
   matplotlib/SVG 按参数直接画出来，画出来的图和题目数据严格一致，不会错。
   这条路我建议作为主力——因为它天然解决了"准确性怎么保证"的问题：图是由题目的
   数值算出来的，不是模型"想象"出来的。
3. AI 生成图（最不可靠，不推荐用于工程图）：现在的文生图模型画机械工程图会把尺寸、
   投影关系画错，看着像但经不起推敲。**工程课不能用**。真要用也必须人工逐张审。

—— 准确性怎么保证 ——
- 路线1：教材原图，天然准确，只需人工核对图题是否对应。
- 路线2：图由题目参数生成，代码逻辑一次审对，之后所有图都对。
- 任何图入库前，image_reviewed 字段标记是否人工核过；答题端只展示已核过的图。

用法：
    # 1) 建表字段 + 建目录（只需跑一次）
    python3 db/add_image_support.py --init

    # 2) 把一张图登记到某道题上
    python3 db/add_image_support.py --bind --question_id Q_xxx --image assets/images/ch3/v_block_01.png

    # 3) 把一张图登记到某个知识点上（该知识点出的题都可以复用这张图）
    python3 db/add_image_support.py --bind --knowledge_id KN_CH3_005 --image assets/images/ch3/mandrel.png

    # 4) 看看现在有多少题带图
    python3 db/add_image_support.py --stat
"""
import argparse
import os
import sqlite3

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
import sys as _qb_sys
_qb_sys.path.insert(0, BASE_DIR)
from qb_runtime import connect_database, database_path
DB_PATH = database_path()
ASSETS = os.path.join(BASE_DIR, "assets", "images")


def init(conn):
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    added = []
    if "image_path" not in cols:
        conn.execute("ALTER TABLE questions ADD COLUMN image_path TEXT")
        added.append("questions.image_path")
    if "image_reviewed" not in cols:
        conn.execute("ALTER TABLE questions ADD COLUMN image_reviewed INTEGER DEFAULT 0")
        added.append("questions.image_reviewed")
    conn.commit()
    # 目录骨架：按章节分文件夹
    for ch in range(1, 8):
        os.makedirs(os.path.join(ASSETS, f"ch{ch}"), exist_ok=True)
    readme = os.path.join(BASE_DIR, "assets", "README.md")
    if not os.path.exists(readme):
        with open(readme, "w", encoding="utf-8") as f:
            f.write(
                "# 图片资源目录\n\n"
                "- 图片文件放在 `assets/images/ch<章号>/` 下，文件名用英文+数字，不要用中文。\n"
                "- 数据库里只存**相对路径**（如 `assets/images/ch3/v_block_01.png`），不存图片本身。\n"
                "- 答题服务器会把 `/assets/...` 暴露为静态资源，网页直接 `<img src=\"/assets/...\">` 引用。\n"
                "- 将来要上线，把路径换成对象存储 URL 即可，代码不用改。\n\n"
                "## 图从哪来\n"
                "1. 教材/真题原图（最准，首选）\n"
                "2. 用代码按题目参数画图（尺寸链、公差带、V形块受力等规则图形；准确性有保证）\n"
                "3. AI 文生图 —— **工程图不要用**，尺寸和投影关系会错。\n")
    print("已完成：")
    for a in added:
        print(f"  新增字段 {a}")
    if not added:
        print("  字段已存在，无需新增")
    print(f"  图片目录：{ASSETS}/ch1 ... ch7")
    print(f"  说明文件：{readme}")


def bind(conn, question_id, knowledge_id, image, reviewed):
    # 允许传绝对路径或相对路径，统一存相对路径
    rel = image
    if os.path.isabs(image):
        rel = os.path.relpath(image, BASE_DIR)
    full = os.path.join(BASE_DIR, rel)
    if not os.path.exists(full):
        print(f"⚠ 提示：文件不存在 {full}（仍会登记路径，等你把图片放进去即可）")
    if question_id:
        conn.execute("UPDATE questions SET image_path=?, image_reviewed=? WHERE question_id=?",
                     (rel, 1 if reviewed else 0, question_id))
        print(f"已把图片绑定到题目 {question_id}: {rel}")
    if knowledge_id:
        conn.execute("UPDATE knowledge_points SET figures=? WHERE knowledge_id=?",
                     (rel, knowledge_id))
        print(f"已把图片绑定到知识点 {knowledge_id}: {rel}")
    conn.commit()


def stat(conn):
    cols = [r[1] for r in conn.execute("PRAGMA table_info(questions)")]
    if "image_path" not in cols:
        print("还没初始化图片支持，请先跑 --init")
        return
    total = conn.execute("SELECT COUNT(*) FROM questions").fetchone()[0]
    withimg = conn.execute("SELECT COUNT(*) FROM questions WHERE image_path IS NOT NULL AND image_path!=''").fetchone()[0]
    reviewed = conn.execute("SELECT COUNT(*) FROM questions WHERE image_reviewed=1").fetchone()[0]
    kfig = conn.execute("SELECT COUNT(*) FROM knowledge_points WHERE figures IS NOT NULL AND figures!=''").fetchone()[0]
    print(f"题目总数 {total}，带图 {withimg}（其中人工核过 {reviewed}）")
    print(f"知识点带配图 {kfig}")
    if withimg == 0:
        print("\n目前一张图都没有。图片的来源见 assets/README.md；")
        print("推荐先用『代码按题目参数画图』的路线（准确性有保证），或等老师的真题库原图。")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--init", action="store_true")
    ap.add_argument("--bind", action="store_true")
    ap.add_argument("--stat", action="store_true")
    ap.add_argument("--question_id")
    ap.add_argument("--knowledge_id")
    ap.add_argument("--image")
    ap.add_argument("--reviewed", action="store_true", help="标记这张图已人工核对过")
    a = ap.parse_args()

    conn = connect_database()
    if a.init:
        init(conn)
    elif a.bind:
        if not a.image or not (a.question_id or a.knowledge_id):
            ap.error("--bind 需要 --image 且至少一个 --question_id / --knowledge_id")
        bind(conn, a.question_id, a.knowledge_id, a.image, a.reviewed)
    else:
        stat(conn)
    conn.close()


if __name__ == "__main__":
    main()
