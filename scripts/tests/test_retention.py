"""Isolated retention contract tests; never access the host Kanban."""
import importlib.util
from pathlib import Path
import sqlite3
from contextlib import closing
import tempfile
import unittest
import time

SCRIPT = Path(__file__).resolve().parents[1] / "kanban_retention.py"
DAY = 86400
NOW = int(time.time())


class SelectionTests(unittest.TestCase):
    def test_preview_selects_only_done_at_thirty_days_without_writes(self):
        self.assertTrue(SCRIPT.exists(), "retention script must exist")
        spec = importlib.util.spec_from_file_location("retention", SCRIPT)
        assert spec is not None and spec.loader is not None
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            db = sqlite3.connect(root / "kanban.db")
            db.executescript("""
                CREATE TABLE tasks(id TEXT PRIMARY KEY,status TEXT,completed_at INTEGER);
                CREATE TABLE task_events(task_id TEXT,kind TEXT,created_at INTEGER);
                CREATE TABLE task_links(parent_id TEXT,child_id TEXT);
                CREATE TABLE task_comments(task_id TEXT,body TEXT);
            """)
            mod.initialize_tracking(root, "default")
            db.executemany("INSERT INTO tasks VALUES (?,?,?)", [
                ("t_old", "done", NOW - 30 * DAY),
                ("t_fresh", "done", NOW - 30 * DAY + 1),
                ("t_blocked", "blocked", NOW - 90 * DAY),
                ("t_unknown", "done", None),
            ])
            db.commit()
            db.execute("UPDATE pengudeck_retention_clock SET activity_at=(SELECT completed_at FROM tasks WHERE id=task_id) WHERE task_id IN ('t_old','t_fresh','t_blocked')")
            db.commit()
            db.close()
            before = (root / "kanban.db").read_bytes()
            report = mod.preview(root, mod.default_policy(), {}, NOW)
            self.assertEqual([x["id"] for x in report["archive"]], ["t_old"])
            self.assertEqual(report["purge"], [])
            self.assertEqual(before, (root / "kanban.db").read_bytes())
            self.assertFalse((root / "kanban/retention").exists())


class RetentionTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location("retention", SCRIPT)
        assert spec is not None and spec.loader is not None
        self.mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.mod)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.db = sqlite3.connect(self.root / "kanban.db")
        self.addCleanup(self.db.close)
        self.db.executescript("""
            CREATE TABLE tasks(id TEXT PRIMARY KEY,status TEXT,completed_at INTEGER);
            CREATE TABLE task_events(task_id TEXT,kind TEXT,created_at INTEGER);
            CREATE TABLE task_links(parent_id TEXT,child_id TEXT);
            CREATE TABLE task_comments(task_id TEXT,body TEXT);
        """)
        self.mod.initialize_tracking(self.root, "default")

    def task(self, tid, status, age=None):
        self.db.execute("INSERT INTO tasks VALUES (?,?,?)", (tid, status, NOW - age * DAY if age is not None else None))
        self.db.execute("UPDATE pengudeck_retention_clock SET activity_at=?,archived_at=? WHERE task_id=?", (NOW - (age or 0) * DAY, NOW - (age or 0) * DAY if status == 'archived' else None, tid))
        self.db.commit()

    def event(self, tid, kind, age):
        self.db.execute("INSERT INTO task_events VALUES (?,?,?)", (tid, kind, NOW - age * DAY))
        self.db.execute("UPDATE pengudeck_retention_clock SET activity_at=? WHERE task_id=?", (NOW - age * DAY, tid))
        if kind == "archived":
            self.db.execute("UPDATE pengudeck_retention_clock SET archived_at=? WHERE task_id=?", (NOW - age * DAY, tid))
        self.db.commit()

    def test_archive_age_survives_event_gc_and_new_activity_postpones_purge(self):
        self.task("t_archive", "archived", 180)
        state = {"archives": {"default/t_archive": NOW - 180 * DAY}}
        first = self.mod.preview(self.root, self.mod.default_policy(), state, NOW)
        self.assertEqual([x["id"] for x in first["purge"]], ["t_archive"])
        self.event("t_archive", "comment", 1)
        second = self.mod.preview(self.root, self.mod.default_policy(), state, NOW)
        self.assertEqual(second["purge"], [])

    def test_unknown_archive_starts_observation_not_creation_age(self):
        self.task("t_unknown", "archived")
        result = self.mod.preview(self.root, self.mod.default_policy(), {}, NOW)
        self.assertEqual(result["purge"], [])
        self.assertEqual(result["observed"]["default/t_unknown"], NOW)

    def test_unfinished_dependents_protect_done_and_archived_parents(self):
        self.task("t_done", "done", 90)
        self.task("t_archived", "archived")
        self.event("t_archived", "archived", 300)
        self.task("t_child", "blocked")
        self.db.executemany("INSERT INTO task_links VALUES (?,?)", [("t_done", "t_child"), ("t_archived", "t_child")])
        self.db.commit()
        result = self.mod.preview(self.root, self.mod.default_policy(), {}, NOW)
        self.assertEqual(result["archive"], [])
        self.assertEqual(result["purge"], [])

    def test_apply_requires_preview_and_verifies_archive_purge_with_backup(self):
        self.assertTrue(hasattr(self.mod, "apply_retention"), "apply implementation is required")
        self.task("t_done", "done", 31)
        self.task("t_purge", "archived")
        self.event("t_purge", "archived", 180)
        result = self.mod.handle(self.root, "preview", {}, NOW)
        with self.assertRaises(ValueError):
            self.mod.handle(self.root, "apply", {"fingerprint": result["fingerprint"]}, NOW)
        calls = []

        def fixture_cli(home, board, args, deadline):
            calls.append(args)
            if args[:2] == ["archive", "--rm"]:
                self.db.execute("DELETE FROM tasks WHERE id=?", (args[2],))
            else:
                self.db.execute("UPDATE tasks SET status='archived' WHERE id=?", (args[1],))
                self.db.execute("INSERT INTO task_events VALUES (?,'archived',?)", (args[1], NOW))
            self.db.commit()

        self.mod.run_cli = fixture_cli
        applied = self.mod.handle(self.root, "apply", {
            "fingerprint": result["fingerprint"], "confirmation": "ARCHIVE AND DELETE"
        }, NOW)
        self.assertFalse(applied["dry_run"])
        self.assertEqual(len(applied["archived"]), 1)
        self.assertEqual(len(applied["purged"]), 1)
        self.assertEqual(len(calls), 2)
        backup = next((self.root / "kanban/retention/backups").glob("*/default.db"))
        with closing(sqlite3.connect(backup)) as db:
            self.assertEqual(db.execute("SELECT count(*) FROM tasks").fetchone()[0], 2)
        state = self.mod.read_json(self.root / "kanban/retention/state.json", {})
        self.assertEqual(state["archives"]["default/t_done"], NOW)
        self.assertEqual(state["last_run"]["at"], NOW)

    def test_stale_preview_and_disabled_tick_do_not_mutate_tasks(self):
        self.assertTrue(hasattr(self.mod, "apply_retention"))
        self.task("t_done", "done", 40)
        result = self.mod.handle(self.root, "preview", {}, NOW)
        self.db.execute("UPDATE tasks SET status='review'")
        self.db.commit()
        with self.assertRaises(ValueError):
            self.mod.handle(self.root, "apply", {"fingerprint": result["fingerprint"], "confirmation": "ARCHIVE AND DELETE"}, NOW)
        result = self.mod.handle(self.root, "tick", {}, NOW)
        self.assertEqual(result["skipped"], "disabled")

    def test_failed_cli_stops_batch_and_tick_waits_a_day(self):
        self.task("t_done", "done", 40)
        policy = {**self.mod.default_policy(), "enabled": True}
        self.mod.handle(self.root, "save", {"policy": policy, "confirmation": "ENABLE RETENTION"}, NOW)
        calls = []
        def fail(*args):
            calls.append(args)
            raise ValueError("cli_failed")
        setattr(self.mod, "run_cli", fail)
        first = self.mod.handle(self.root, "tick", {}, NOW)
        self.assertEqual(first["errors"][0]["error"], "cli_failed")
        self.assertEqual(len(calls), 1)
        self.assertEqual(first["remaining"], 1)
        self.assertEqual(self.mod.handle(self.root, "tick", {}, NOW + 1)["skipped"], "not_due")
        self.assertEqual(self.db.execute("SELECT status FROM tasks").fetchone()[0], "done")

    def test_backups_fail_closed_and_are_rate_limited(self):
        self.task("t_done", "done", 40)
        self.mod.handle(self.root, "save", {"policy": {**self.mod.default_policy(), "enabled": True}, "confirmation": "ENABLE RETENTION"}, NOW)
        def fail(*args):
            raise ValueError("backup_failed")
        setattr(self.mod, "backup_boards", fail)
        with self.assertRaises(ValueError):
            self.mod.handle(self.root, "tick", {}, NOW)
        self.assertEqual(self.mod.handle(self.root, "tick", {}, NOW + 1)["skipped"], "not_due")
        self.assertEqual(self.db.execute("SELECT status FROM tasks").fetchone()[0], "done")

    def test_state_rejects_corrupt_timestamps_and_preserves_excluded_board_anchors(self):
        self.assertTrue(hasattr(self.mod, "validate_state"))
        for state in ([], {"archives": []}, {"archives": {"default/t_bad": "yesterday"}}, {"last_attempt": []}):
            with self.assertRaises(ValueError):
                self.mod.validate_state(state)
        directory = self.root / "kanban/retention"
        self.mod.private_dir(directory)
        self.mod.atomic_json(directory / "state.json", {"archives": {"excluded/t_old": NOW - 300 * DAY}})
        result = self.mod.handle(self.root, "preview", {}, NOW)
        self.mod.handle(self.root, "apply", {"fingerprint": result["fingerprint"], "confirmation": "ARCHIVE AND DELETE"}, NOW)
        self.assertEqual(self.mod.read_json(directory / "state.json", {})["archives"]["excluded/t_old"], NOW - 300 * DAY)

    def test_disable_remains_available_when_board_database_disappears(self):
        self.mod.handle(self.root, "save", {"policy": {**self.mod.default_policy(), "enabled": True}, "confirmation": "ENABLE RETENTION"}, NOW)
        self.db.close()
        (self.root / "kanban.db").unlink()
        result = self.mod.handle(self.root, "save", {"policy": self.mod.default_policy()}, NOW)
        self.assertFalse(result["policy"]["enabled"])

    def test_lock_prevents_overlapping_apply(self):
        directory = self.root / "kanban/retention"
        with self.mod.locked(directory):
            with self.assertRaisesRegex(ValueError, "retention_busy"):
                self.mod.handle(self.root, "apply", {"confirmation": "ARCHIVE AND DELETE"}, NOW)

    def test_rearchive_event_postpones_purge_after_old_anchor(self):
        self.task("t_archive", "archived")
        self.event("t_archive", "archived", 1)
        result = self.mod.preview(self.root, self.mod.default_policy(), {"archives": {"default/t_archive": NOW - 400 * DAY}}, NOW)
        self.assertEqual(result["purge"], [])
        self.assertEqual(result["observed"]["default/t_archive"], NOW - DAY)

    def test_invalid_policy_rejected_and_enabling_requires_confirmation(self):
        self.assertTrue(hasattr(self.mod, "validate_policy"), "policy validation is required")
        for update in ({"archive_days": 0}, {"purge_days": -1}, {"archive_days": True}, {"boards": ["../oops"]}, {"boards": []}, {"enabled": "false"}):
            with self.assertRaises(ValueError):
                self.mod.validate_policy({**self.mod.default_policy(), **update})
        with self.assertRaises(ValueError):
            self.mod.handle(self.root, "save", {"policy": {**self.mod.default_policy(), "enabled": True}}, NOW)
        self.assertFalse((self.root / "kanban/retention/policy.json").exists())


if __name__ == "__main__":
    unittest.main()
