# Kanban retention

Pengudeck provides **Settings → Pembersihan card otomatis** and two stdlib-only Python programs:

- `scripts/kanban_retention.py` is the original engine and standalone command-line tool. It is kept unchanged (its workflow, flags and confirmations are documented below) and remains the only code that inspects or mutates boards.
- `scripts/kanban_cleanup.py` is a thin facade used by the Settings page. It calls the engine's own functions so the browser needs one confirmed **Simpan** instead of the script's track → save → preview → typed confirmation → apply sequence.

Deploying the code does not enable deletion or initialize tracking.

## Policy and activation

- Defaults: disabled, board `default`, archive after 30 days, purge after 180 days.
- Only `done` cards with a known completion time are archived. Age is measured from the later of completion or durable last activity.
- Only `archived` cards with a durable archive-generation timestamp are purged. Age is measured from the later of archival or durable last activity.
- Unfinished linked work is protected. Revisions are checked inside the destructive SQLite statement, not just before calling the CLI.
- No tracking means **no eligible cards**, regardless of old sidecar observations or creation timestamps.
- Initialization gives **all existing cards a new observation window**. This deliberately delays cleanup rather than assuming no unobserved rearchive/activity occurred before installation.
- Days are elapsed Unix seconds, independent of server timezone. Configuration accepts integer days 1–3650 and explicit board slugs, not a wildcard covering future boards.

Settings workflow (one card, no typed phrases):

1. Choose board checkboxes (loaded from the real board list), the archive age and the purge age. Automation is off by default.
2. Press **Simpan**. Turning automation off saves immediately and never touches board databases, so it works even if a board disappeared.
3. Turning it on opens one confirmation dialog stating, in plain language, the ages, the exact number of cards that would be archived or permanently deleted right now (computed for the unsaved values, read-only), and, when a board has no tracking yet, that Pengudeck will back it up and install its tracking triggers (existing cards start a new observation window today). **Aktifkan** performs everything in order: save the policy disabled, back up only boards lacking tracking, install tracking, then enable. If any step fails the policy stays disabled (fail closed) and the page reports a plain reason.
4. **Lanjutan** (collapsed) shows the last run and **Jalankan sekarang**. It uses the fingerprint of the summary you were shown and a dialog; if the board changed in between, the request is rejected as stale and nothing is deleted. The button is enabled only for a saved policy whose boards are tracked and which has candidates.
5. A board removed from the checkbox list keeps its tracking triggers so rearchives and activity keep being observed.

The server converts the dialog's `confirmed: true` into the engine's confirmations; anything other than the boolean `true` is rejected. The engine still takes a backup before every card mutation, validates revision/status/age inside the destructive SQLite statement, holds its lock, and caps a run at 50 cards. Replacing typed phrases with a dialog lowers the friction for the destructive action; the 180-day archive age, automatic backup, default-off setting and exact counts in the dialog are the mitigation.

Settings shows the last completed run. A run with errors says so; inspect the engine's `runs.jsonl` for detail. An old successful run is not proof that a later attempt succeeded.

Facade API (same Host/Origin/`X-Pengudeck-Request` rules as below; error bodies contain only a fixed lowercase category):

- `GET /api/settings/cleanup`: policy, last run, read-only summary `{archive, purge, untracked}` and fingerprint for the saved policy.
- `POST /api/settings/cleanup/review` `{policy}`: read-only summary for unsaved values.
- `PUT /api/settings/cleanup` `{policy, confirmed}`: save; `confirmed: true` is required only when enabling.
- `POST /api/settings/cleanup/run` `{fingerprint, confirmed}`: apply a previously shown summary.

The older `/api/settings/retention*` routes still expose the engine's original workflow operations for compatibility; the Settings page no longer uses them.

The scheduler belongs to the API lifecycle. It checks once a minute, while the script permits at most one automatic attempt per 24 hours, including preview/backup failures. There is no new host cron job or service. No running API means no cleanup. Each batch handles at most 50 candidates with bounded command time; `remaining` includes failed/unattempted candidates.

## Standalone script

Use `--home`, otherwise `HERMES_KANBAN_HOME`, otherwise `~/.hermes`. This is the **shared Kanban root**, not a profile directory. Custom installations should set `HERMES_KANBAN_HOME` for both API and script.

```sh
# Read-only: preview is the default operation.
python3 scripts/kanban_retention.py --home /path/to/shared-root
python3 scripts/kanban_retention.py get --home /path/to/shared-root

# Store a disabled policy; disabling works if a board disappeared.
printf '%s' '{"policy":{"enabled":false,"archive_days":30,"purge_days":180,"boards":["default"]}}' \
  | python3 scripts/kanban_retention.py save --home /path/to/shared-root --input-json

# Explicit schema/tracking initialization after reading the warnings.
printf '%s' '{"confirmation":"INITIALIZE TRACKING"}' \
  | python3 scripts/kanban_retention.py track --home /path/to/shared-root --input-json

# Apply only a reviewed, current fingerprint.
printf '%s' '{"fingerprint":"<preview fingerprint>","confirmation":"ARCHIVE AND DELETE"}' \
  | python3 scripts/kanban_retention.py apply --home /path/to/shared-root --input-json

# No action unless the saved policy is enabled and due.
python3 scripts/kanban_retention.py tick --home /path/to/shared-root
```

Output is JSON. Rejected requests exit nonzero. A completed batch may contain partial failures in `errors`; callers must inspect that field.

## Writer-coordinated safety

Initialization installs `pengudeck_retention_meta`, `pengudeck_retention_clock`, `pengudeck_retention_intent` and eleven `pgret_*` triggers in each explicitly selected database, without changing existing task columns. Installation is transactional; incomplete/incompatible tracking fails closed.

Triggers record task changes, event/comment insertions and updates, dependency additions/updates, and archive-generation changes. Event GC does not erase these clocks. Rearchival while the API is off resets the clock even if events are subsequently GC'd. Related-card status changes invalidate linked-card eligibility too.

Before calling supported Hermes `archive`/`archive --rm`, the script validates status, age, revision and dependencies under `BEGIN IMMEDIATE` and writes an intent for that exact candidate. SQLite triggers guard the actual task UPDATE/DELETE inside the CLI transaction. A concurrent comment, reopen, new link or linked-card status change invalidates the intent and aborts the transaction; preceding relation cleanup rolls back too. Each target is read back afterwards. A separate file lock serializes Pengudeck runs.

Ordinary Hermes transitions without a retention intent retain normal lifecycle behavior. A crash may leave a fail-closed candidate intent; a later validated attempt can replace it. Do not drop the metadata/triggers while retention is enabled. Restoring a pre-tracking database requires reinitialization and a new observation window. Future Hermes schema upgrades require compatibility testing; missing required triggers never fall back to unsafe age inference.

API binding remains `127.0.0.1:8788`. Writes require exact Host `127.0.0.1:8788`, exact Origin `http://127.0.0.1:8081` and `X-Pengudeck-Request: retention-v1`. Duplicate/missing/foreign headers are rejected. Only retention routes allow write preflight; board routes stay GET-only. These prevent browser CSRF/rebinding but **are not user authentication**. Same-OS-user processes remain trusted. Do not publish this write surface without a separate authentication design.

Commands use argv, fixed operations/executables, explicit database pins, output limits and timeouts. Underlying CLI/SQLite diagnostics are not returned to the browser. Hermes worker guards are not disabled or bypassed.

## State, backup and deletion semantics

Under `<shared-home>/kanban/retention/`:

- `policy.json`: saved policy.
- `state.json`: reporting observations, last attempt and last run; **not authority for purge age**.
- `lock`: cross-process lock.
- `runs.jsonl`: execution audit.
- `backups/<timestamp>-<random>/`: online SQLite backups plus a manifest before initialization or card mutation.

State/backups are private (`0600`, directories `0700`). Backup integrity is checked before changes; failure prevents card mutation. An interrupted backup may leave a partial directory: require a successful manifest before treating it as a recovery copy.

**Permanent deletion removes data from the board, not from backups.** Backups deliberately have no automatic expiry. Their storage/expiry policy is a separate operator decision; this feature never silently deletes recovery copies.

Archival may clean Hermes-managed scratch/worktree directories. Database backups do **not** cover workspace or attachment files. Restore only offline after a fresh backup and explicit operator approval; never overwrite a live database as an ad-hoc rollback.

## Verification

```sh
python3 -m unittest discover -s scripts/tests -v
cargo test --workspace --offline
cargo clippy --workspace --offline -- -D warnings
node --test crates/web/tests/*.test.mjs
(cd crates/web && dx build --platform web --offline --locked)
```

Python integration uses the installed Hermes CLI on a disposable private Kanban home, including a concurrent real Hermes comment between intent creation and CLI mutation. SQLite tests cover concurrent reopen/linked-card changes and rearchive followed by event GC.

For browser tests, build the fixture and web app, then use an **already installed** matching Playwright package:

```sh
cargo build --offline -p hermes-dash-api --example retention_fixture
PLAYWRIGHT_CORE=/absolute/path/to/playwright-core QA_OUT=/path/to/qa-output \
  node crates/web/tests/retention-browser.cjs chromium firefox
```

The fixture API uses an ephemeral loopback port. All browser HTTP requests are intercepted and fulfilled from build files or the private API, never the existing services on 8081/8788. The harness exercises the simple Settings page: no typed fields, input validation, cancel by button and Escape leaving the database untouched, one-dialog backup+tracking+enable, persistence, dialog-free disable, manual run with real Hermes CLI read-back, desktop/light and mobile/dark layouts with the dialog open, contrast, and console/network checks. Fixture processes stop afterwards. `PW_WEBKIT_WS` supports a matching cached Playwright container when host WebKit libraries are absent.
