"""Validate and atomically publish a reviewed development bank to the student runtime."""

from __future__ import annotations

import argparse
import os
import shutil
import sqlite3
import tempfile
from datetime import datetime
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FORMAL_DB = (
    ROOT.parent / "files.v21_最终" / "question_bank_system" / "db" / "question_bank.db"
).resolve()
CONFIRMATION = "PUBLISH_REVIEWED_STUDENT_BANK"


def _student_filter() -> str:
    return (
        "review_status='已通过' "
        "AND COALESCE(usage_scope,'学生练习')='学生练习' "
        "AND TRIM(COALESCE(answer,''))<>'' "
        "AND TRIM(COALESCE(no_answer_reason,''))=''"
    )


def _validate(source: Path) -> tuple[int, list[Path]]:
    if not source.is_file():
        raise ValueError(f"开发数据库不存在：{source}")
    conn = sqlite3.connect(f"file:{source.as_posix()}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    try:
        integrity = conn.execute("PRAGMA integrity_check").fetchone()[0]
        if integrity != "ok":
            raise ValueError(f"数据库完整性检查失败：{integrity}")
        columns = {row[1] for row in conn.execute("PRAGMA table_info(questions)")}
        required = {"review_status", "usage_scope", "no_answer_reason", "image_path", "answer_image_path"}
        if not required.issubset(columns):
            raise ValueError(f"questions 缺少字段：{sorted(required - columns)}")
        count = conn.execute(f"SELECT COUNT(*) FROM questions WHERE {_student_filter()}").fetchone()[0]
        rows = conn.execute(
            f"SELECT image_path, answer_image_path FROM questions WHERE {_student_filter()}"
        ).fetchall()
    finally:
        conn.close()
    media: set[Path] = set()
    for row in rows:
        for value in row:
            if not value:
                continue
            relative = Path(value)
            if relative.is_absolute() or ".." in relative.parts:
                raise ValueError(f"非法媒体路径：{value}")
            source_file = (ROOT / relative).resolve()
            if ROOT not in source_file.parents or not source_file.is_file():
                raise ValueError(f"媒体文件缺失：{value}")
            media.add(relative)
    return count, sorted(media)


def _atomic_copy_database(source: Path, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    backup_dir = target.parent / "backups"
    backup_dir.mkdir(exist_ok=True)
    if target.exists():
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        backup = backup_dir / f"question_bank-{stamp}.db.bak"
        src_conn = sqlite3.connect(f"file:{target.as_posix()}?mode=ro", uri=True)
        dst_conn = sqlite3.connect(backup)
        try:
            src_conn.backup(dst_conn)
        finally:
            dst_conn.close()
            src_conn.close()
    fd, temp_name = tempfile.mkstemp(prefix="question-bank-publish-", suffix=".db", dir=target.parent)
    os.close(fd)
    temp = Path(temp_name)
    try:
        src_conn = sqlite3.connect(f"file:{source.as_posix()}?mode=ro", uri=True)
        dst_conn = sqlite3.connect(temp)
        try:
            src_conn.backup(dst_conn)
        finally:
            dst_conn.close()
            src_conn.close()
        os.replace(temp, target)
    finally:
        if temp.exists():
            temp.unlink()


def main() -> int:
    parser = argparse.ArgumentParser(description="审核后题库发布工具（默认仅预检）")
    parser.add_argument("--source", required=True, help="已审核开发数据库绝对路径")
    parser.add_argument("--target", default=str(FORMAL_DB), help="只允许 firstwork 学生正式库")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--confirm", default="")
    args = parser.parse_args()
    source = Path(args.source).expanduser().resolve()
    target = Path(args.target).expanduser().resolve()
    if target != FORMAL_DB:
        raise SystemExit(f"发布目标被拒绝，只允许：{FORMAL_DB}")
    if source == target:
        raise SystemExit("开发库与正式库不能是同一文件")
    try:
        count, media = _validate(source)
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
    print(f"Validation passed: {count} reviewed student questions, {len(media)} media files")
    if not args.apply:
        print("Dry run only. 未修改正式库；发布需追加 --apply --confirm PUBLISH_REVIEWED_STUDENT_BANK")
        return 0
    if args.confirm != CONFIRMATION:
        raise SystemExit("缺少发布确认口令，未修改正式库")
    _atomic_copy_database(source, target)
    formal_root = FORMAL_DB.parents[1]
    for relative in media:
        destination = formal_root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / relative, destination)
    print(f"发布完成：{target}（已备份旧库并同步 {len(media)} 个媒体文件）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
