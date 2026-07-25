"""Safe launcher for the isolated question-bank production system."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from pathlib import Path

from qb_runtime import ENV_DB, ENV_WRITE, database_path


ROOT = Path(__file__).resolve().parent
COMMANDS = {
    "init": "db/init_db.py",
    "seed": "db/seed_misconceptions.py",
    "generate": "pipeline/generate_questions.py",
    "import-exercise": "pipeline/import_exercise_pdf.py",
    "import-pdf": "pipeline/import_pdf_exams.py",
    "import-word": "pipeline/import_real_exams.py",
    "import-word-llm": "pipeline/import_real_exams_llm.py",
    "auto-review": "review/auto_review.py",
    "review-export": "review/export_for_review.py",
    "review-import": "review/import_reviewed.py",
    "dedup": "pipeline/dedup.py",
    "normalize": "pipeline/normalize_answers.py",
    "calibrate": "pipeline/calibrate_difficulty.py",
    "serve": "demo/serve_quiz.py",
    "check-web": "pipeline/check_web.py",
    "status": "pipeline/status.py",
    "coverage": "pipeline/syllabus_coverage.py",
    "rag": "pipeline/rag.py",
}
WRITE_COMMANDS = {
    "init", "seed", "generate", "import-exercise", "import-pdf", "import-word",
    "import-word-llm", "auto-review", "review-import", "dedup", "normalize",
    "calibrate", "serve",
}


def main() -> int:
    parser = argparse.ArgumentParser(description="题库生产工具安全启动器")
    parser.add_argument("--db", required=True, help="开发数据库绝对路径（禁止正式学生库）")
    parser.add_argument("command", choices=[*COMMANDS, "test"])
    parser.add_argument("args", nargs=argparse.REMAINDER)
    ns = parser.parse_args()

    os.environ[ENV_DB] = ns.db
    os.environ["PYTHONIOENCODING"] = "utf-8"
    database_path()  # Fail before launching any child process.

    env = os.environ.copy()
    needs_write = ns.command in WRITE_COMMANDS or (
        ns.command == "review-export" and bool(ns.args) and ns.args[0] == "import_back"
    )
    if needs_write:
        env[ENV_WRITE] = "1"
    else:
        env.pop(ENV_WRITE, None)

    if ns.command == "test":
        cmd = [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-v"]
    else:
        cmd = [sys.executable, str(ROOT / COMMANDS[ns.command]), *ns.args]
    return subprocess.call(cmd, cwd=ROOT, env=env)


if __name__ == "__main__":
    raise SystemExit(main())
