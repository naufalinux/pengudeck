"""Integration with installed Hermes CLI on a private disposable Kanban home."""
from contextlib import closing
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import time
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "kanban_retention.py"
DAY = 86400


@unittest.skipUnless(shutil.which("hermes"), "installed Hermes CLI required")
class HermesIntegration(unittest.TestCase):
    def test_real_cli_archive_and_purge_only_private_fixture(self):
        with tempfile.TemporaryDirectory(prefix="pengudeck-hermes-") as directory:
            home = Path(directory)
            env = dict(os.environ, HERMES_KANBAN_HOME=str(home), HERMES_KANBAN_DB=str(home / "kanban.db"))

            def cli(*args):
                return subprocess.run(["hermes", "kanban", "--board", "default", *args], env=env,
                                      text=True, capture_output=True, timeout=15, check=True).stdout

            cli("init")
            first = json.loads(cli("create", "retention integration done", "--initial-status", "blocked", "--json"))["id"]
            second = json.loads(cli("create", "retention integration archive", "--initial-status", "blocked", "--json"))["id"]
            cli("complete", first, "--result", "isolated fixture only")
            cli("archive", second)
            subprocess.run(["python3", str(SCRIPT), "track", "--home", str(home), "--input-json"], input=json.dumps({"confirmation": "INITIALIZE TRACKING"}), text=True, capture_output=True, check=True, timeout=15)
            now = int(time.time())
            with closing(sqlite3.connect(home / "kanban.db")) as db:
                db.execute("UPDATE tasks SET completed_at=? WHERE id=?", (now - 31 * DAY, first))
                db.execute("UPDATE task_events SET created_at=? WHERE task_id=?", (now - 31 * DAY, first))
                db.execute("UPDATE task_events SET created_at=? WHERE task_id=?", (now - 181 * DAY, second))
                db.execute("UPDATE pengudeck_retention_clock SET activity_at=? WHERE task_id=?", (now - 31 * DAY, first))
                db.execute("UPDATE pengudeck_retention_clock SET activity_at=?,archived_at=? WHERE task_id=?", (now - 181 * DAY, now - 181 * DAY, second))
                db.commit()
            preview = subprocess.run(["python3", str(SCRIPT), "--home", str(home)], text=True, capture_output=True, check=True, timeout=15)
            report = json.loads(preview.stdout)
            self.assertEqual([x["id"] for x in report["archive"]], [first])
            self.assertEqual([x["id"] for x in report["purge"]], [second])
            self.assertFalse((home / "kanban/retention/state.json").exists())
            applied = subprocess.run(["python3", str(SCRIPT), "apply", "--home", str(home), "--input-json"],
                                     input=json.dumps({"fingerprint": report["fingerprint"], "confirmation": "ARCHIVE AND DELETE"}),
                                     text=True, capture_output=True, check=True, timeout=45)
            result = json.loads(applied.stdout)
            self.assertEqual(result["errors"], [])
            self.assertEqual([x["id"] for x in result["archived"]], [first])
            self.assertEqual([x["id"] for x in result["purged"]], [second])
            rows = json.loads(cli("list", "--archived", "--json"))
            self.assertEqual({x["id"]: x["status"] for x in rows}, {first: "archived"})
            self.assertTrue(list((home / "kanban/retention/backups").glob("*/default.db")))
            # Force a real Hermes writer into the exact intent-to-CLI interval.
            raced = json.loads(cli("create", "concurrent comment fixture", "--initial-status", "blocked", "--json"))["id"]
            cli("complete", raced, "--result", "fixture only")
            with closing(sqlite3.connect(home / "kanban.db")) as db:
                db.execute("UPDATE tasks SET completed_at=? WHERE id=?", (now - 31 * DAY, raced))
                db.execute("UPDATE pengudeck_retention_clock SET activity_at=? WHERE task_id=?", (now - 31 * DAY, raced))
                db.commit()
            spec = importlib.util.spec_from_file_location("retention_race", SCRIPT)
            assert spec and spec.loader
            mod = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(mod)
            original = mod.run_cli
            def with_concurrent_comment(*args):
                cli("comment", raced, "new evidence while retention is pending")
                original(*args)
            setattr(mod, "run_cli", with_concurrent_comment)
            report = mod.handle(home, "preview", {}, now)
            result = mod.handle(home, "apply", {"fingerprint": report["fingerprint"], "confirmation": "ARCHIVE AND DELETE"}, now)
            self.assertEqual(result["errors"][0]["error"], "cli_failed")
            rows = json.loads(cli("list", "--archived", "--json"))
            self.assertEqual(next(row["status"] for row in rows if row["id"] == raced), "done")
            purged_race = json.loads(cli("create", "concurrent purge fixture", "--initial-status", "blocked", "--json"))["id"]
            cli("complete", purged_race, "--result", "fixture only")
            cli("archive", purged_race)
            cli("link", purged_race, raced)
            cli("comment", purged_race, "original retained evidence")
            with closing(sqlite3.connect(home / "kanban.db")) as db:
                db.execute("UPDATE pengudeck_retention_clock SET activity_at=?,archived_at=? WHERE task_id=?", (now - 181 * DAY, now - 181 * DAY, purged_race))
                db.commit()
            expected_rows = {}
            def with_concurrent_purge_comment(*args):
                cli("comment", purged_race, "new evidence during pending purge")
                with closing(sqlite3.connect(home / "kanban.db")) as db:
                    for table in ("task_comments", "task_events"):
                        expected_rows[table] = db.execute("SELECT * FROM " + table + " WHERE task_id=? ORDER BY rowid", (purged_race,)).fetchall()
                    expected_rows["task_links"] = db.execute("SELECT * FROM task_links WHERE parent_id=? ORDER BY rowid", (purged_race,)).fetchall()
                original(*args)
            setattr(mod, "run_cli", with_concurrent_purge_comment)
            report = mod.handle(home, "preview", {}, now)
            self.assertEqual([item["id"] for item in report["purge"]], [purged_race])
            result = mod.handle(home, "apply", {"fingerprint":report["fingerprint"], "confirmation":"ARCHIVE AND DELETE"}, now)
            self.assertEqual(result["errors"][0]["error"], "cli_failed")
            with closing(sqlite3.connect(home / "kanban.db")) as db:
                self.assertEqual(db.execute("SELECT status FROM tasks WHERE id=?", (purged_race,)).fetchone()[0], "archived")
                for table in ("task_comments", "task_events"):
                    self.assertEqual(db.execute("SELECT * FROM " + table + " WHERE task_id=? ORDER BY rowid", (purged_race,)).fetchall(), expected_rows[table])
                self.assertEqual(db.execute("SELECT * FROM task_links WHERE parent_id=? ORDER BY rowid", (purged_race,)).fetchall(), expected_rows["task_links"])
                self.assertEqual(db.execute("SELECT count(*) FROM pengudeck_retention_intent").fetchone()[0], 0)


if __name__ == "__main__":
    unittest.main()
