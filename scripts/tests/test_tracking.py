"""SQLite writer-coordinated retention safety; every database is disposable."""
from contextlib import closing
import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "kanban_retention.py"
DAY = 86400


class TrackingTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("retention", SCRIPT)
        assert spec and spec.loader
        self.mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.mod)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.db = sqlite3.connect(self.home / "kanban.db")
        self.addCleanup(self.db.close)
        self.db.executescript("""
          CREATE TABLE tasks(id TEXT PRIMARY KEY,status TEXT,completed_at INTEGER);
          CREATE TABLE task_events(task_id TEXT,kind TEXT,created_at INTEGER);
          CREATE TABLE task_comments(task_id TEXT,body TEXT);
          CREATE TABLE task_links(parent_id TEXT,child_id TEXT);
        """)
        self.now = int(time.time())

    def install(self):
        self.assertTrue(hasattr(self.mod, "initialize_tracking"), "durable SQLite tracking required")
        self.mod.initialize_tracking(self.home, "default")

    def task(self, tid="t_archive", status="archived", days=181):
        self.db.execute("INSERT INTO tasks VALUES (?,?,?)", (tid, status, self.now - 31 * DAY))
        self.db.execute("UPDATE pengudeck_retention_clock SET archived_at=?,activity_at=? WHERE task_id=?", (self.now - days * DAY if status == "archived" else None, self.now - days * DAY, tid))
        self.db.commit()

    def test_tracking_preserves_rearchive_and_activity_across_event_gc(self):
        self.install()
        self.task()
        first = self.mod.preview(self.home, self.mod.default_policy(), {}, self.now)
        self.assertEqual(len(first["purge"]), 1)
        self.db.execute("UPDATE tasks SET status='done' WHERE id='t_archive'")
        self.db.execute("UPDATE tasks SET status='archived' WHERE id='t_archive'")
        self.db.execute("DELETE FROM task_events")
        self.db.commit()
        second = self.mod.preview(self.home, self.mod.default_policy(), {"archives":{"default/t_archive":self.now - 400 * DAY}}, self.now)
        self.assertEqual(second["purge"], [])
        self.assertGreaterEqual(second["observed"]["default/t_archive"], self.now)

    def test_untracked_sidecar_age_never_authorizes_deletion(self):
        self.db.execute("INSERT INTO tasks VALUES ('t_archive','archived',?)", (self.now - 400 * DAY,))
        self.db.commit()
        result = self.mod.preview(self.home, self.mod.default_policy(), {"archives":{"default/t_archive":self.now - 400 * DAY}}, self.now)
        self.assertEqual(result["purge"], [])
        with self.assertRaises(ValueError):
            self.mod.handle(self.home, "apply", {"fingerprint":result["fingerprint"], "confirmation":"ARCHIVE AND DELETE"}, self.now)

    def test_concurrent_comment_aborts_delete_atomically(self):
        self.install()
        self.task()
        report = self.mod.preview(self.home, self.mod.default_policy(), {}, self.now)
        item = report["purge"][0]
        with self.mod.mutation_intent(self.home, "default", "purge", item, self.mod.default_policy(), self.now):
            with closing(sqlite3.connect(self.home / "kanban.db")) as writer:
                writer.execute("INSERT INTO task_comments VALUES ('t_archive','new information')")
                writer.commit()
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute("DELETE FROM tasks WHERE id='t_archive'")
            self.db.rollback()
        self.assertEqual(self.db.execute("SELECT status FROM tasks").fetchone()[0], "archived")

    def test_concurrent_reopen_aborts_archive_atomically(self):
        self.install()
        self.task("t_done", "done", 31)
        item = self.mod.preview(self.home, self.mod.default_policy(), {}, self.now)["archive"][0]
        with self.mod.mutation_intent(self.home, "default", "archive", item, self.mod.default_policy(), self.now):
            with closing(sqlite3.connect(self.home / "kanban.db")) as writer:
                writer.execute("UPDATE tasks SET status='review' WHERE id='t_done'")
                writer.commit()
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute("UPDATE tasks SET status='archived' WHERE id='t_done'")
            self.db.rollback()
        self.assertEqual(self.db.execute("SELECT status FROM tasks").fetchone()[0], "review")

    def test_reopened_child_invalidates_intent_even_after_cli_deletes_links(self):
        self.install()
        self.task("t_parent")
        self.task("t_child", "done", 31)
        self.db.execute("INSERT INTO task_links VALUES ('t_parent','t_child')")
        self.db.execute("UPDATE pengudeck_retention_clock SET activity_at=? WHERE task_id='t_parent'", (self.now - 181 * DAY,))
        self.db.commit()
        item = self.mod.preview(self.home, self.mod.default_policy(), {}, self.now)["purge"][0]
        with self.mod.mutation_intent(self.home, "default", "purge", item, self.mod.default_policy(), self.now):
            with closing(sqlite3.connect(self.home / "kanban.db")) as writer:
                writer.execute("UPDATE tasks SET status='review' WHERE id='t_child'")
                writer.commit()
            self.db.execute("BEGIN IMMEDIATE")
            self.db.execute("DELETE FROM task_links WHERE parent_id='t_parent'")
            with self.assertRaises(sqlite3.IntegrityError):
                self.db.execute("DELETE FROM tasks WHERE id='t_parent'")
            self.db.rollback()
        self.assertEqual(self.db.execute("SELECT count(*) FROM task_links").fetchone()[0], 1)


if __name__ == '__main__':
    unittest.main()
