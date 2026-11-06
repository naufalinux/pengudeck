"""Simple cleanup facade tests; every database is disposable."""
from contextlib import closing
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
CLEANUP = SCRIPTS / "kanban_cleanup.py"
ORIGINAL = SCRIPTS / "kanban_retention.py"
ORIGINAL_SHA256 = "db0e41c3f8416f4f693a30e413a1b4f2daf621bc3d7f118dcdab5f31350cff06"
DAY = 86400
POLICY = {"enabled": True, "archive_days": 30, "purge_days": 180, "boards": ["default"]}


def run(home, operation, payload=None):
    result = subprocess.run(
        [sys.executable, str(CLEANUP), operation, "--home", str(home), "--input-json"],
        input=json.dumps(payload or {}), text=True, capture_output=True, timeout=30,
    )
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.now = int(time.time())
        self.make_board("default")

    def make_board(self, board):
        path = self.home / "kanban.db" if board == "default" else self.home / "kanban/boards" / board / "kanban.db"
        path.parent.mkdir(parents=True, exist_ok=True)
        with closing(sqlite3.connect(path)) as db:
            db.executescript("""
              CREATE TABLE tasks(id TEXT PRIMARY KEY,status TEXT,completed_at INTEGER);
              CREATE TABLE task_events(task_id TEXT,kind TEXT,created_at INTEGER);
              CREATE TABLE task_comments(task_id TEXT,body TEXT);
              CREATE TABLE task_links(parent_id TEXT,child_id TEXT);
            """)
            db.execute("INSERT INTO tasks VALUES ('t_old','done',?)", (self.now - 40 * DAY,))
            db.commit()
        return path

    def tracked_names(self, board="default"):
        path = self.home / "kanban.db" if board == "default" else self.home / "kanban/boards" / board / "kanban.db"
        with closing(sqlite3.connect(path)) as db:
            return {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE name LIKE 'pgret_%'")}

    def test_original_script_is_preserved_byte_for_byte(self):
        self.assertEqual(hashlib.sha256(ORIGINAL.read_bytes()).hexdigest(), ORIGINAL_SHA256)

    def test_status_is_read_only_and_reports_untracked_boards(self):
        before = sorted(p.name for p in self.home.iterdir())
        status = run(self.home, "status")
        self.assertEqual(status["policy"], {**POLICY, "enabled": False})
        self.assertEqual(status["summary"], {"archive": 0, "purge": 0, "untracked": ["default"]})
        self.assertEqual(sorted(p.name for p in self.home.iterdir()), before)
        self.assertEqual(self.tracked_names(), set())

    def test_enabling_requires_confirmation_and_changes_nothing_without_it(self):
        result = run(self.home, "save", {"policy": POLICY})
        self.assertEqual(result, {"error": "confirmation_required"})
        self.assertFalse((self.home / "kanban/retention").exists())
        self.assertEqual(self.tracked_names(), set())
        result = run(self.home, "save", {"policy": POLICY, "confirmed": "yes"})
        self.assertEqual(result, {"error": "confirmation_required"})

    def test_one_confirmed_save_tracks_backs_up_and_enables(self):
        review = run(self.home, "review", {"policy": POLICY})
        self.assertEqual(review["summary"], {"archive": 0, "purge": 0, "untracked": ["default"]})
        self.assertEqual(self.tracked_names(), set())  # review never mutates
        saved = run(self.home, "save", {"policy": POLICY, "confirmed": True})
        self.assertEqual(saved["policy"], POLICY)
        self.assertTrue(self.tracked_names())
        backups = list((self.home / "kanban/retention/backups").glob("*/default.db"))
        self.assertEqual(len(backups), 1)
        status = run(self.home, "status")
        self.assertEqual(status["policy"], POLICY)
        self.assertEqual(status["summary"]["untracked"], [])
        # New observation window: an old Done card is not immediately eligible.
        self.assertEqual(status["summary"]["archive"], 0)

    def test_second_save_does_not_reinitialize_or_create_another_backup(self):
        run(self.home, "save", {"policy": POLICY, "confirmed": True})
        run(self.home, "save", {"policy": {**POLICY, "archive_days": 45}, "confirmed": True})
        self.assertEqual(len(list((self.home / "kanban/retention/backups").glob("*/default.db"))), 1)
        self.assertEqual(run(self.home, "status")["policy"]["archive_days"], 45)

    def test_disabling_needs_no_confirmation_and_works_when_board_vanished(self):
        run(self.home, "save", {"policy": POLICY, "confirmed": True})
        gone = {**POLICY, "enabled": False, "boards": ["default", "missing"]}
        saved = run(self.home, "save", {"policy": gone})
        self.assertEqual(saved["policy"], gone)

    def test_failed_tracking_leaves_automation_disabled(self):
        policy = {**POLICY, "boards": ["default", "missing"]}
        result = run(self.home, "save", {"policy": policy, "confirmed": True})
        self.assertIn("error", result)
        self.assertFalse(run(self.home, "status").get("policy", {"enabled": False})["enabled"])

    def test_run_applies_only_the_previewed_fingerprint_and_needs_confirmation(self):
        run(self.home, "save", {"policy": POLICY, "confirmed": True})
        with closing(sqlite3.connect(self.home / "kanban.db")) as db:
            db.execute("UPDATE pengudeck_retention_clock SET activity_at=? WHERE task_id='t_old'", (self.now - 40 * DAY,))
            db.commit()
        status = run(self.home, "status")
        self.assertEqual(status["summary"]["archive"], 1)
        self.assertEqual(run(self.home, "run", {"fingerprint": status["fingerprint"]}), {"error": "confirmation_required"})
        self.assertEqual(run(self.home, "run", {"fingerprint": "0" * 64, "confirmed": True}), {"error": "stale_preview"})
        with closing(sqlite3.connect(self.home / "kanban.db")) as db:
            self.assertEqual(db.execute("SELECT status FROM tasks WHERE id='t_old'").fetchone()[0], "done")

    def test_invalid_input_returns_only_a_fixed_error_code(self):
        for payload in ({"policy": {"secret": "SENTINEL"}}, {"policy": {**POLICY, "boards": ["../x"]}}, {}):
            result = run(self.home, "save", payload)
            self.assertEqual(set(result), {"error"})
            self.assertRegex(result["error"], r"^[a-z_]{1,48}$")
            self.assertNotIn("SENTINEL", json.dumps(result))


if __name__ == "__main__":
    unittest.main()
