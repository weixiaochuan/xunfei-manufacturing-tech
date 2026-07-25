from __future__ import annotations

import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FORMAL_ROOT = ROOT.parent / "files.v21_最终" / "question_bank_system"
FORMAL_DB = FORMAL_ROOT / "db" / "question_bank.db"
sys.path.insert(0, str(ROOT))

import qb_runtime


class RuntimeSafetyTests(unittest.TestCase):
    def setUp(self):
        self.previous = os.environ.copy()
        qb_runtime._BACKED_UP.clear()

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self.previous)
        qb_runtime._BACKED_UP.clear()

    def test_database_path_is_mandatory_and_formal_database_is_blocked(self):
        os.environ.pop(qb_runtime.ENV_DB, None)
        with self.assertRaises(qb_runtime.UnsafeDatabasePath):
            qb_runtime.database_path()
        os.environ[qb_runtime.ENV_DB] = str(FORMAL_DB)
        with self.assertRaises(qb_runtime.UnsafeDatabasePath):
            qb_runtime.database_path()

    def test_readonly_connection_rejects_writes(self):
        with tempfile.TemporaryDirectory() as temp:
            db = Path(temp) / "readonly.db"
            setup = sqlite3.connect(db)
            setup.execute("CREATE TABLE sample(value TEXT)")
            setup.commit()
            setup.close()
            os.environ[qb_runtime.ENV_DB] = str(db)
            os.environ.pop(qb_runtime.ENV_WRITE, None)
            conn = qb_runtime.connect_database()
            with self.assertRaises(sqlite3.OperationalError):
                conn.execute("INSERT INTO sample VALUES ('blocked')")
            conn.close()

    def test_first_write_creates_backup(self):
        with tempfile.TemporaryDirectory() as temp:
            db = Path(temp) / "write.db"
            conn = sqlite3.connect(db)
            conn.execute("CREATE TABLE sample(value TEXT)")
            conn.execute("INSERT INTO sample VALUES ('before')")
            conn.commit()
            conn.close()
            os.environ[qb_runtime.ENV_DB] = str(db)
            os.environ[qb_runtime.ENV_WRITE] = "1"
            conn = qb_runtime.connect_database()
            conn.execute("INSERT INTO sample VALUES ('after')")
            conn.commit()
            conn.close()
            backups = list((db.parent / "backups").glob("*.bak"))
            self.assertEqual(len(backups), 1)
            backup = sqlite3.connect(backups[0])
            self.assertEqual(backup.execute("SELECT COUNT(*) FROM sample").fetchone()[0], 1)
            backup.close()

    def test_init_and_mock_generation_only_touch_development_database(self):
        with tempfile.TemporaryDirectory() as temp:
            db = Path(temp) / "development.db"
            env = os.environ.copy()
            env["PYTHONIOENCODING"] = "utf-8"
            init = subprocess.run(
                [sys.executable, str(ROOT / "qbctl.py"), "--db", str(db), "init"],
                cwd=ROOT, env=env, capture_output=True, text=True, encoding="utf-8",
            )
            self.assertEqual(init.returncode, 0, init.stderr)
            generated = subprocess.run(
                [sys.executable, str(ROOT / "qbctl.py"), "--db", str(db), "generate",
                 "--provider", "mock", "--limit", "1", "--no-rag"],
                cwd=ROOT, env=env, capture_output=True, text=True, encoding="utf-8",
            )
            self.assertEqual(generated.returncode, 0, generated.stderr)
            conn = sqlite3.connect(db)
            self.assertEqual(conn.execute("PRAGMA integrity_check").fetchone()[0], "ok")
            self.assertGreater(conn.execute("SELECT COUNT(*) FROM knowledge_points").fetchone()[0], 0)
            self.assertGreater(conn.execute("SELECT COUNT(*) FROM questions").fetchone()[0], 0)
            conn.close()

    def test_all_formal_question_media_references_exist(self):
        conn = sqlite3.connect(f"file:{FORMAL_DB.as_posix()}?mode=ro", uri=True)
        refs = conn.execute("SELECT image_path, answer_image_path FROM questions").fetchall()
        conn.close()
        missing = []
        for row in refs:
            for value in row:
                if value and not (FORMAL_ROOT / value).is_file():
                    missing.append(value)
        self.assertEqual(missing, [])

    def test_student_contract_is_strict_and_never_returns_answers(self):
        with tempfile.TemporaryDirectory() as temp:
            db = Path(temp) / "student-contract.db"
            shutil.copy2(FORMAL_DB, db)
            env = os.environ.copy()
            env[qb_runtime.ENV_DB] = str(db)
            env["PYTHONIOENCODING"] = "utf-8"
            code = (
                "import json; from integration.api import get_questions_for_chapter; "
                "import sqlite3, os; "
                "c=sqlite3.connect(os.environ['QUESTION_BANK_DB']); "
                "ch=c.execute(\"SELECT course_chapter FROM questions WHERE review_status='已通过' "
                "AND COALESCE(usage_scope,'学生练习')='学生练习' LIMIT 1\").fetchone()[0]; "
                "c.close(); print(json.dumps(get_questions_for_chapter(ch, 1000), ensure_ascii=False))"
            )
            result = subprocess.run(
                [sys.executable, "-c", code], cwd=ROOT, env=env,
                capture_output=True, text=True, encoding="utf-8",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            rows = json.loads(result.stdout)
            self.assertTrue(rows)
            self.assertTrue(all("answer" not in row and "explanation" not in row for row in rows))
            conn = sqlite3.connect(db)
            ids = [row["question_id"] for row in rows]
            placeholders = ",".join("?" for _ in ids)
            invalid = conn.execute(
                f"SELECT COUNT(*) FROM questions WHERE question_id IN ({placeholders}) AND ("
                "review_status<>'已通过' OR COALESCE(usage_scope,'学生练习')<>'学生练习' "
                "OR TRIM(COALESCE(answer,''))='' OR TRIM(COALESCE(no_answer_reason,''))<>'')",
                ids,
            ).fetchone()[0]
            conn.close()
            self.assertEqual(invalid, 0)


if __name__ == "__main__":
    unittest.main()
