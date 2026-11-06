#!/usr/bin/env python3
"""Pengudeck simple cleanup facade. GPL-3.0-only. Stdlib only.

The original kanban_retention.py engine stays untouched and is the only code
that inspects or mutates boards. This facade only orchestrates its documented
operations so a UI can offer one confirmed Save instead of the script workflow
(track -> save -> preview -> typed confirmations -> apply).
"""
import argparse
import json
import os
from pathlib import Path
import re
import sqlite3
import sys
import time

# Fixed, non-sensitive categories only; anything else is reported generically.
SAFE_ERRORS = {
    "confirmation_required", "stale_preview", "retention_busy", "initialize_tracking_first",
    "invalid_policy", "invalid_enabled", "invalid_days", "invalid_boards", "invalid_board",
    "invalid_payload", "backup_timeout", "backup_failed", "incomplete_tracking_schema",
    "unsupported_tracking_version",
}
SOURCE_ENV = "PENGUDECK_RETENTION_SCRIPT"


def load_engine():
    source = os.environ.get(SOURCE_ENV)
    if source is None:
        source = (Path(__file__).resolve().parent / "kanban_retention.py").read_text(encoding="utf-8")
    namespace = {"__name__": "kanban_retention_engine"}
    exec(compile(source, "kanban_retention.py", "exec"), namespace)  # noqa: S102 - trusted bundled engine
    return namespace


def summarize(report):
    return {"archive": len(report["archive"]), "purge": len(report["purge"]), "untracked": report["tracking_required"]}


def state_for(engine, home):
    return engine["validate_state"](engine["read_json"](home / "kanban/retention/state.json", {}))


def operate(engine, home, operation, payload, now):
    directory = home / "kanban/retention"
    handle = engine["handle"]
    if operation == "status":
        current = handle(home, "get", {})
        try:
            report = handle(home, "preview", {})
            summary, fingerprint = summarize(report), report["fingerprint"]
        except (ValueError, OSError, sqlite3.Error):
            summary, fingerprint = None, None
        return {**current, "summary": summary, "fingerprint": fingerprint}
    if operation == "review":
        policy = engine["validate_policy"](payload.get("policy"))
        report = engine["preview"](home, policy, state_for(engine, home), now)
        return {"summary": summarize(report)}
    if operation == "save":
        policy = engine["validate_policy"](payload.get("policy"))
        if not policy["enabled"]:
            # Disabling never touches board databases, so it works even if a board vanished.
            return handle(home, "save", {"policy": policy})
        if payload.get("confirmed") is not True:
            raise ValueError("confirmation_required")
        untracked = engine["preview"](home, policy, state_for(engine, home), now)["tracking_required"]
        if untracked:
            # Fail closed: automation stays off unless every step below succeeds.
            safe = {**policy, "enabled": False}
            handle(home, "save", {"policy": safe})
            with engine["locked"](directory):
                engine["backup_boards"](home, directory, {"archive": [{"board": b} for b in untracked], "purge": [], "policy": safe, "observed": {}}, now)
                for board in untracked:
                    engine["initialize_tracking"](home, board)
        return handle(home, "save", {"policy": policy, "confirmation": "ENABLE RETENTION"})
    if operation == "run":
        if payload.get("confirmed") is not True:
            raise ValueError("confirmation_required")
        fingerprint = payload.get("fingerprint")
        if not isinstance(fingerprint, str) or not re.fullmatch(r"[0-9a-f]{64}", fingerprint):
            raise ValueError("stale_preview")
        return handle(home, "apply", {"fingerprint": fingerprint, "confirmation": "ARCHIVE AND DELETE"})
    raise ValueError("invalid_payload")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["status", "review", "save", "run"])
    parser.add_argument("--home", type=Path, default=Path(os.environ.get("HERMES_KANBAN_HOME", str(Path.home() / ".hermes"))))
    parser.add_argument("--input-json", action="store_true")
    args = parser.parse_args()
    try:
        payload = json.loads(sys.stdin.read(16385)) if args.input_json else {}
        if not isinstance(payload, dict):
            raise ValueError("invalid_payload")
        home = args.home.expanduser().resolve()
        print(json.dumps(operate(load_engine(), home, args.operation, payload, int(time.time()))))
    except ValueError as error:
        code = str(error)
        print(json.dumps({"error": code if code in SAFE_ERRORS else "cleanup_failed"}))
    except (OSError, sqlite3.Error):
        print(json.dumps({"error": "cleanup_failed"}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
