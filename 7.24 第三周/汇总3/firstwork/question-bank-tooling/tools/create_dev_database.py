"""Create a new isolated development database; never overwrites a database."""

from __future__ import annotations

import argparse
import os
import sqlite3
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", required=True)
    args = parser.parse_args()
    target = Path(args.db).expanduser()
    if not target.is_absolute():
        raise SystemExit("--db 必须是绝对路径")
    target = target.resolve()
    if target.exists():
        raise SystemExit(f"目标已存在，拒绝覆盖：{target}")
    target.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        [sys.executable, str(ROOT / "qbctl.py"), "--db", str(target), "init"],
        cwd=ROOT,
        check=False,
    )
    if result.returncode:
        return result.returncode
    conn = sqlite3.connect(f"file:{target.as_posix()}?mode=ro", uri=True)
    try:
        integrity = conn.execute("PRAGMA integrity_check").fetchone()[0]
        knowledge = conn.execute("SELECT COUNT(*) FROM knowledge_points").fetchone()[0]
    finally:
        conn.close()
    if integrity != "ok":
        raise SystemExit(f"开发库完整性检查失败：{integrity}")
    print(f"开发库已创建：{target}")
    print(f"knowledge_points={knowledge}, integrity={integrity}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
