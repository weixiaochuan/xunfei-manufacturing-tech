"""Shared, fail-closed database runtime for the question-bank tooling."""

from __future__ import annotations

import os
import shutil
import sqlite3
from datetime import datetime
from pathlib import Path


ROOT = Path(__file__).resolve().parent
ENV_DB = "QUESTION_BANK_DB"
ENV_WRITE = "QUESTION_BANK_ALLOW_WRITE"

_BACKED_UP: set[Path] = set()


class UnsafeDatabasePath(RuntimeError):
    pass


def _blocked_paths() -> set[Path]:
    return {
        (ROOT / "db" / "question_bank.db").resolve(),
        (
            ROOT.parent
            / "files.v21_最终"
            / "question_bank_system"
            / "db"
            / "question_bank.db"
        ).resolve(),
    }


def database_path() -> Path:
    raw = os.environ.get(ENV_DB, "").strip()
    if not raw:
        raise UnsafeDatabasePath(
            f"未指定开发数据库。请通过 qbctl.py --db <绝对路径> 运行，或设置 {ENV_DB}。"
        )
    path = Path(raw).expanduser()
    if not path.is_absolute():
        raise UnsafeDatabasePath(f"{ENV_DB} 必须是绝对路径，当前值已被拒绝。")
    path = path.resolve()
    if path in _blocked_paths():
        raise UnsafeDatabasePath("拒绝操作学生端正式库或工具源码目录中的默认库。")
    return path


def writes_enabled() -> bool:
    return os.environ.get(ENV_WRITE, "").strip() == "1"


def _backup_before_write(path: Path) -> Path | None:
    if not path.exists() or path in _BACKED_UP:
        return None
    backup_dir = path.parent / "backups"
    backup_dir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    backup = backup_dir / f"{path.stem}-{stamp}{path.suffix}.bak"
    source = sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True)
    target = sqlite3.connect(backup)
    try:
        source.backup(target)
    finally:
        target.close()
        source.close()
    _BACKED_UP.add(path)
    return backup


def connect_database(*, row_factory: bool = False) -> sqlite3.Connection:
    path = database_path()
    if writes_enabled():
        path.parent.mkdir(parents=True, exist_ok=True)
        _backup_before_write(path)
        conn = sqlite3.connect(path)
    else:
        if not path.is_file():
            raise FileNotFoundError(f"只读数据库不存在：{path}")
        conn = sqlite3.connect(f"file:{path.as_posix()}?mode=ro", uri=True)
    if row_factory:
        conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys=ON")
    return conn


def reset_database() -> None:
    if not writes_enabled():
        raise PermissionError(f"重建数据库需要显式设置 {ENV_WRITE}=1。")
    path = database_path()
    _backup_before_write(path)
    for candidate in (path, Path(f"{path}-wal"), Path(f"{path}-shm")):
        if candidate.exists():
            candidate.unlink()


def rag_index_path() -> Path:
    path = database_path()
    return path.with_name(f"{path.stem}.rag_index.pkl")


def copy_database(source: Path, target: Path) -> None:
    """Copy a database only after both endpoints passed the runtime path policy."""
    source = source.resolve()
    target = target.resolve()
    if source in _blocked_paths() or target in _blocked_paths():
        raise UnsafeDatabasePath("复制目标不能是学生端正式库；发布请使用专用发布工具。")
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
