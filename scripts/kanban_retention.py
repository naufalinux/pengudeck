#!/usr/bin/env python3
"""Pengudeck retention. GPL-3.0-only. Stdlib only; dry-run by default."""
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import argparse
from contextlib import closing, contextmanager
import fcntl
import os
import subprocess
import sys
import time
import uuid

DAY = 86400
SLUG = re.compile(r"[A-Za-z0-9_-]{1,64}\Z")


def default_policy():
    return {"enabled": False, "archive_days": 30, "purge_days": 180, "boards": ["default"]}


def db_path(home, board):
    if not isinstance(board, str) or not SLUG.fullmatch(board):
        raise ValueError("invalid_board")
    path = home / "kanban.db" if board == "default" else home / "kanban/boards" / board / "kanban.db"
    if not path.resolve().is_relative_to(home.resolve()):
        raise ValueError("invalid_board_path")
    return path


def snapshot(home, board):
    path = db_path(home, board)
    with closing(sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True, timeout=3)) as db:
        db.row_factory = sqlite3.Row
        db.execute("BEGIN")
        tasks = {r["id"]: dict(r) for r in db.execute("SELECT id,status,completed_at FROM tasks")}
        events = {}
        archives = {}
        for row in db.execute("SELECT task_id,kind,MAX(created_at) AS ts FROM task_events GROUP BY task_id,kind"):
            events[row["task_id"]] = max(events.get(row["task_id"], 0), row["ts"])
            if row["kind"] == "archived":
                archives[row["task_id"]] = row["ts"]
        links = list(db.execute("SELECT parent_id,child_id FROM task_links"))
        clocks = tracking_clocks(db)
    protected = set()
    for parent, child in links:
        if tasks.get(child, {}).get("status") not in ("done", "archived"):
            protected.add(parent)
            protected.add(child)
    return tasks, events, archives, protected, clocks


def preview(home, policy, state, now):
    archive, purge, observed, untracked = [], [], {}, []
    for board in policy["boards"]:
        tasks, events, archives, protected, clocks = snapshot(home, board)
        if clocks is None:
            untracked.append(board)
            continue
        for tid, task in sorted(tasks.items()):
            key = board + "/" + tid
            clock = clocks.get(tid)
            if clock is None:
                continue
            if task["status"] == "archived":
                # Missing history starts a conservative observation window. Later
                # activity and re-archive events can only postpone deletion.
                if clock["archived_at"] is None:
                    continue
                anchor = max(clock["archived_at"], clock["activity_at"])
                observed[key] = anchor
                if tid not in protected and 0 < anchor <= now - policy["purge_days"] * DAY:
                    purge.append({"board": board, "id": tid, "since": anchor, "revision": clock["revision"]})
            elif task["status"] == "done" and task["completed_at"] is not None:
                anchor = max(task["completed_at"], clock["activity_at"])
                if tid not in protected and 0 < anchor <= now - policy["archive_days"] * DAY:
                    archive.append({"board": board, "id": tid, "since": anchor, "revision": clock["revision"]})
    body = {"policy": policy, "archive": archive, "purge": purge, "tracking_required": untracked}
    token = hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()
    return {**body, "fingerprint": token, "observed": observed, "dry_run": True}


TRACKING_TRIGGERS = {
    "pgret_task_insert", "pgret_task_update", "pgret_task_delete",
    "pgret_event_insert", "pgret_event_update", "pgret_comment_insert",
    "pgret_comment_update", "pgret_link_insert", "pgret_link_update",
    "pgret_guard_archive", "pgret_guard_delete",
}


def tracking_clocks(db):
    names = {row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='trigger'")}
    if not TRACKING_TRIGGERS.issubset(names):
        return None
    if [r[0] for r in db.execute("SELECT version FROM pengudeck_retention_meta")] != [1]:
        raise ValueError("unsupported_tracking_version")
    return {row["task_id"]: dict(row) for row in db.execute("SELECT task_id,revision,activity_at,archived_at FROM pengudeck_retention_clock")}


def initialize_tracking(home, board):
    """Explicit opt-in schema extension; no Hermes source/config changes.

    All ordinary SQLite writers maintain the clocks and honor pending guards.
    Existing tasks start a NEW conservative observation window.
    """
    path = db_path(home, board)
    with closing(sqlite3.connect(path.resolve().as_uri() + "?mode=rw", uri=True, timeout=3)) as db:
        existing = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='trigger'")}
        if TRACKING_TRIGGERS.issubset(existing):
            return
        if existing.intersection(TRACKING_TRIGGERS):
            raise ValueError("incomplete_tracking_schema")
        clock = "pengudeck_retention_clock"
        intent = "pengudeck_retention_intent"
        now = "CAST(strftime('%s','now') AS INTEGER)"
        script = f"""
        BEGIN IMMEDIATE;
        CREATE TABLE pengudeck_retention_meta(version INTEGER NOT NULL);
        INSERT INTO pengudeck_retention_meta VALUES (1);
        CREATE TABLE {clock}(task_id TEXT PRIMARY KEY,revision INTEGER NOT NULL,activity_at INTEGER NOT NULL,archived_at INTEGER);
        CREATE TABLE {intent}(task_id TEXT PRIMARY KEY,action TEXT NOT NULL,expected_revision INTEGER NOT NULL,cutoff INTEGER NOT NULL,nonce TEXT NOT NULL);
        INSERT INTO {clock} SELECT id,0,{now},CASE WHEN status='archived' THEN {now} END FROM tasks;
        CREATE TRIGGER pgret_task_insert AFTER INSERT ON tasks BEGIN
          INSERT INTO {clock} VALUES(NEW.id,0,{now},CASE WHEN NEW.status='archived' THEN {now} END);
        END;
        CREATE TRIGGER pgret_task_update AFTER UPDATE ON tasks BEGIN
          UPDATE {clock} SET revision=revision+1,activity_at=MAX(activity_at,{now}),
            archived_at=CASE WHEN task_id=NEW.id THEN
              CASE WHEN NEW.status!='archived' THEN NULL WHEN OLD.status!='archived' THEN {now} ELSE archived_at END
              ELSE archived_at END
          WHERE task_id=NEW.id OR task_id IN (SELECT parent_id FROM task_links WHERE child_id=NEW.id)
            OR task_id IN (SELECT child_id FROM task_links WHERE parent_id=NEW.id);
        END;
        CREATE TRIGGER pgret_task_delete AFTER DELETE ON tasks BEGIN
          DELETE FROM {clock} WHERE task_id=OLD.id;
          DELETE FROM {intent} WHERE task_id=OLD.id;
        END;
        """
        for label, table in (("event", "task_events"), ("comment", "task_comments")):
            for operation in ("INSERT", "UPDATE"):
                script += f"""
                CREATE TRIGGER pgret_{label}_{operation.lower()} AFTER {operation} ON {table} BEGIN
                  UPDATE {clock} SET revision=revision+1,activity_at=MAX(activity_at,{now}) WHERE task_id=NEW.task_id;
                END;
                """
        for operation in ("INSERT", "UPDATE"):
            script += f"""
            CREATE TRIGGER pgret_link_{operation.lower()} AFTER {operation} ON task_links BEGIN
              UPDATE {clock} SET revision=revision+1,activity_at=MAX(activity_at,{now}) WHERE task_id IN (NEW.parent_id,NEW.child_id);
            END;
            """
        for action, name, event, status in (("archive", "pgret_guard_archive", "UPDATE OF status", "done"), ("purge", "pgret_guard_delete", "DELETE", "archived")):
            when = " AND NEW.status='archived'" if action == "archive" else ""
            archive_check = " AND c.archived_at IS NOT NULL AND c.archived_at<=i.cutoff" if action == "purge" else " AND OLD.completed_at IS NOT NULL AND OLD.completed_at<=i.cutoff"
            script += f"""
            CREATE TRIGGER {name} BEFORE {event} ON tasks
            WHEN EXISTS(SELECT 1 FROM {intent} WHERE task_id=OLD.id AND action='{action}'){when}
            BEGIN
              SELECT CASE WHEN OLD.status!='{status}' OR NOT EXISTS(
                SELECT 1 FROM {clock} c JOIN {intent} i ON i.task_id=c.task_id
                WHERE c.task_id=OLD.id AND c.revision=i.expected_revision AND c.activity_at<=i.cutoff{archive_check}
              ) THEN RAISE(ABORT,'retention eligibility changed') END;
            END;
            """
        script += "COMMIT;"
        try:
            db.executescript(script)
        except BaseException:
            db.rollback()
            raise


@contextmanager
def mutation_intent(home, board, action, item, policy, now):
    path = db_path(home, board)
    nonce = uuid.uuid4().hex
    with closing(sqlite3.connect(path.resolve().as_uri() + "?mode=rw", uri=True, timeout=3)) as db:
        db.row_factory = sqlite3.Row
        db.execute("BEGIN IMMEDIATE")
        clocks = tracking_clocks(db)
        task = db.execute("SELECT status,completed_at FROM tasks WHERE id=?", (item["id"],)).fetchone()
        clock = clocks.get(item["id"]) if clocks is not None else None
        cutoff = now - policy["purge_days" if action == "purge" else "archive_days"] * DAY
        expected_status = "archived" if action == "purge" else "done"
        anchor = clock["archived_at"] if clock and action == "purge" else (task["completed_at"] if task else None)
        if not task or not clock or task["status"] != expected_status or anchor is None or max(anchor, clock["activity_at"]) > cutoff or clock["revision"] != item.get("revision"):
            raise ValueError("eligibility_changed_or_tracking_required")
        linked = db.execute("""SELECT 1 FROM task_links l JOIN tasks t
            ON (l.parent_id=? AND t.id=l.child_id) OR (l.child_id=? AND t.id=l.parent_id)
            WHERE t.status NOT IN ('done','archived') LIMIT 1""", (item["id"], item["id"])).fetchone()
        if linked:
            raise ValueError("unfinished_dependency")
        db.execute("INSERT OR REPLACE INTO pengudeck_retention_intent VALUES (?,?,?,?,?)", (item["id"], action, clock["revision"], cutoff, nonce))
        db.commit()
        try:
            yield
        finally:
            db.execute("DELETE FROM pengudeck_retention_intent WHERE task_id=? AND nonce=?", (item["id"], nonce))
            db.commit()


def validate_policy(policy):
    if not isinstance(policy, dict) or set(policy) != set(default_policy()):
        raise ValueError("invalid_policy")
    if type(policy["enabled"]) is not bool:
        raise ValueError("invalid_enabled")
    for key in ("archive_days", "purge_days"):
        if type(policy[key]) is not int or not 1 <= policy[key] <= 3650:
            raise ValueError("invalid_days")
    boards = policy["boards"]
    if not isinstance(boards, list) or not 1 <= len(boards) <= 32:
        raise ValueError("invalid_boards")
    if any(not isinstance(b, str) or not SLUG.fullmatch(b) for b in boards) or len(set(boards)) != len(boards):
        raise ValueError("invalid_boards")
    return policy


def read_json(path, default):
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return default


def validate_state(state):
    if not isinstance(state, dict) or not isinstance(state.get("archives", {}), dict):
        raise ValueError("invalid_state")
    if any(not isinstance(key, str) or type(value) is not int or value <= 0
           for key, value in state.get("archives", {}).items()):
        raise ValueError("invalid_state")
    attempt = state.get("last_attempt", {})
    if not isinstance(attempt, dict) or type(attempt.get("at", 0)) is not int:
        raise ValueError("invalid_state")
    return state


def private_dir(path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.is_symlink() or path.stat().st_mode & 0o077:
        raise ValueError("state_directory_must_be_private")


def atomic_json(path, value):
    temp = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    try:
        with open(temp, "x", opener=lambda p, flags: os.open(p, flags, 0o600)) as file:
            json.dump(value, file, sort_keys=True)
            file.flush()
            os.fsync(file.fileno())
        os.replace(temp, path)
        fd = os.open(path.parent, os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        temp.unlink(missing_ok=True)


@contextmanager
def locked(directory):
    private_dir(directory)
    with open(directory / "lock", "a", opener=lambda p, flags: os.open(p, flags | os.O_NOFOLLOW, 0o600)) as file:
        try:
            fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError("retention_busy") from None
        yield


def handle(home, operation, payload, now=None):
    now = int(time.time()) if now is None else now
    home = Path(home).expanduser().resolve()
    directory = home / "kanban/retention"
    if not directory.resolve().is_relative_to(home):
        raise ValueError("invalid_state_path")
    policy = validate_policy(read_json(directory / "policy.json", default_policy()))
    state = validate_state(read_json(directory / "state.json", {}))
    if operation == "get":
        return {"policy": policy, "last_run": state.get("last_run"), "last_attempt": state.get("last_attempt")}
    if operation == "preview":
        result = preview(home, policy, state, now)
        result.pop("observed")
        return result
    if operation == "track":
        if payload.get("confirmation") != "INITIALIZE TRACKING":
            raise ValueError("tracking_confirmation_required")
        with locked(directory):
            policy = validate_policy(read_json(directory / "policy.json", default_policy()))
            # Preserve the original schema/data before installing namespaced metadata.
            backup = backup_boards(home, directory, {"archive": [{"board": b} for b in policy["boards"]], "purge": [], "policy": policy, "observed": {}}, now)
            for board in policy["boards"]:
                initialize_tracking(home, board)
        return {"tracking_initialized": policy["boards"], "backup": backup}
    if operation == "save":
        new = validate_policy(payload.get("policy"))
        if new["enabled"] and payload.get("confirmation") != "ENABLE RETENTION":
            raise ValueError("enable_confirmation_required")
        # Emergency disable must work even if a scoped board vanished.
        if new["enabled"]:
            for board in new["boards"]:
                if snapshot(home, board)[4] is None:
                    raise ValueError("initialize_tracking_first")
        with locked(directory):
            atomic_json(directory / "policy.json", new)
        return {"policy": new, "last_run": state.get("last_run"), "last_attempt": state.get("last_attempt")}
    if operation == "tick" and not policy["enabled"]:
        return {"skipped": "disabled"}
    if operation in ("apply", "tick"):
        with locked(directory):
            policy = validate_policy(read_json(directory / "policy.json", default_policy()))
            state = validate_state(read_json(directory / "state.json", {}))
            if operation == "tick":
                if not policy["enabled"]:
                    return {"skipped": "disabled"}
                if now - state.get("last_attempt", {}).get("at", 0) < DAY:
                    return {"skipped": "not_due"}
            elif payload.get("confirmation") != "ARCHIVE AND DELETE":
                raise ValueError("apply_confirmation_required")
            if operation == "tick":
                state["last_attempt"] = {"at": now, "status": "started"}
                atomic_json(directory / "state.json", state)
            report = preview(home, policy, state, now)
            if report["tracking_required"]:
                raise ValueError("initialize_tracking_first")
            if operation == "apply" and payload.get("fingerprint") != report["fingerprint"]:
                raise ValueError("stale_preview")
            return apply_retention(home, directory, report, state, now)
    raise ValueError("unsupported_operation")


def backup_boards(home, directory, report, now):
    deadline = time.monotonic() + 20
    def progress(status, remaining, total):
        if time.monotonic() >= deadline:
            raise ValueError("backup_timeout")
    boards = sorted({item["board"] for key in ("archive", "purge") for item in report[key][:50]})
    if not boards:
        return None
    target = directory / "backups" / (str(now) + "-" + uuid.uuid4().hex)
    private_dir(target)
    for board in boards:
        dest = target / (board + ".db")
        with open(dest, "x", opener=lambda p, flags: os.open(p, flags, 0o600)):
            pass
        with closing(sqlite3.connect(db_path(home, board).resolve().as_uri() + "?mode=ro", uri=True, timeout=3)) as source:
            with closing(sqlite3.connect(dest)) as copy:
                source.backup(copy, pages=128, sleep=0.05, progress=progress)
                if copy.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                    raise ValueError("backup_failed")
        with dest.open("rb") as file:
            os.fsync(file.fileno())
    atomic_json(target / "manifest.json", {"at": now, "policy": report["policy"], "archives": report["observed"], "scope": boards})
    return str(target)


def run_cli(home, board, args, deadline):
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise ValueError("run_budget_exceeded")
    env = dict(os.environ, HERMES_KANBAN_HOME=str(home), HERMES_KANBAN_DB=str(db_path(home, board)))
    try:
        result = subprocess.run(["hermes", "kanban", "--board", board, *args],
                                env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                timeout=min(8, remaining), check=False)
    except (OSError, subprocess.TimeoutExpired):
        raise ValueError("cli_unavailable_or_timeout") from None
    if result.returncode != 0:
        raise ValueError("cli_failed")


def apply_retention(home, directory, report, state, now):
    if os.environ.get("HERMES_KANBAN_TASK"):
        raise ValueError("worker_context_not_allowed")
    deadline = time.monotonic() + 40
    state["last_attempt"] = {"at": now, "status": "backing_up"}
    atomic_json(directory / "state.json", state)
    try:
        backup = backup_boards(home, directory, report, now)
    except (OSError, sqlite3.Error, ValueError):
        state["last_attempt"] = {"at": now, "status": "backup_failed"}
        atomic_json(directory / "state.json", state)
        raise
    scoped = tuple(board + "/" for board in report["policy"]["boards"])
    state["archives"] = {key: value for key, value in state.get("archives", {}).items() if not key.startswith(scoped)} | report["observed"]
    result = {"at": now, "dry_run": False, "archived": [], "purged": [], "errors": [], "backup": backup}
    state["last_attempt"] = {"at": now, "status": "running"}
    atomic_json(directory / "state.json", state)
    # Purge first, so cards archived in this run can never be deleted in it.
    actions = [("purge", x) for x in report["purge"]] + [("archive", x) for x in report["archive"]]
    result["remaining"] = max(0, len(actions) - 50)
    for action, item in actions[:50]:
        board, tid = item["board"], item["id"]
        try:
            if time.monotonic() >= deadline:
                raise ValueError("run_budget_exceeded")
            current = preview(home, {**report["policy"], "boards": [board]}, state, now)
            if item not in current[action]:
                raise ValueError("eligibility_changed")
            if not re.fullmatch(r"t_[A-Za-z0-9_-]{1,100}", tid):
                raise ValueError("invalid_task_id")
            args = ["archive", "--rm", tid] if action == "purge" else ["archive", tid]
            with mutation_intent(home, board, action, item, report["policy"], now):
                run_cli(home, board, args, deadline)
            tasks = snapshot(home, board)[0]
            if action == "purge":
                if tid in tasks:
                    raise ValueError("verification_failed")
                state["archives"].pop(board + "/" + tid, None)
                result["purged"].append(item)
            else:
                if tasks.get(tid, {}).get("status") != "archived":
                    raise ValueError("verification_failed")
                state["archives"][board + "/" + tid] = now
                result["archived"].append(item)
            atomic_json(directory / "state.json", state)
        except (ValueError, sqlite3.Error, OSError) as error:
            # No provider output, paths, SQL errors or credentials enter the UI.
            code = str(error) if type(error) is ValueError else "storage_failure"
            result["errors"].append({"board": board, "id": tid, "error": code})
            break
    result["remaining"] = len(actions) - len(result["archived"]) - len(result["purged"])
    state["last_run"] = result
    state["last_attempt"] = {"at": now, "status": "failed" if result["errors"] else "finished"}
    atomic_json(directory / "state.json", state)
    with open(directory / "runs.jsonl", "a", opener=lambda p, flags: os.open(p, flags | os.O_NOFOLLOW, 0o600)) as log:
        log.write(json.dumps(result) + "\n")
        log.flush()
        os.fsync(log.fileno())
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", nargs="?", default="preview", choices=["get", "save", "track", "preview", "apply", "tick"])
    parser.add_argument("--home", type=Path, default=Path(os.environ.get("HERMES_KANBAN_HOME", str(Path.home() / ".hermes"))))
    parser.add_argument("--input-json", action="store_true", help="Read request JSON from stdin (otherwise {}).")
    args = parser.parse_args()
    try:
        payload = json.loads(sys.stdin.read(16385)) if args.input_json else {}
        if not isinstance(payload, dict):
            raise ValueError("invalid_payload")
        print(json.dumps(handle(args.home, args.operation, payload)))
        return 0
    except (ValueError, OSError, sqlite3.Error):
        # Diagnostic categories only: underlying CLI/SQLite output may contain secrets.
        print(json.dumps({"error": "retention_request_failed"}))
        return 1


if __name__ == "__main__":
    sys.exit(main())
