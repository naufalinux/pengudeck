# Retention implementation verification

## Delivery

- Branch: `feature/kanban-retention-settings`.
- Isolated worktree: `/home/gnu/work/git/naufalinux/pengudeck-retention`.
- Original checkout: `/home/gnu/work/git/naufalinux/pengudeck`, still on `agent/archi/t_70a0f24f-web`.
- Fifty copied baseline application files were hash-compared with the original checkout; no original content changes were found.
- At implementation verification time the feature and inherited baseline snapshot were uncommitted; no unrelated baseline commit was made. The user subsequently authorized publishing and merging the reviewed retention feature. Publication is based on GitHub `main`, preserving its existing content rather than publishing duplicate local baseline history.

## Implemented

- Standalone stdlib Python script: `scripts/kanban_retention.py`.
- Settings menu with persisted scope/day values, tracking consent, enable/disable, saved-policy preview, fingerprint-bound manual apply, typed confirmations, last attempt and last completed run.
- Default policy: disabled; Done archival after 30 days; archived-card purge after 180 days; explicit board scope.
- API-lifecycle scheduler, maximum one automatic attempt per 24 hours and 50 candidates per batch.
- Backups before schema initialization/card mutation; durable per-task tracking independent of event GC; database-level revision/age guards in the actual Hermes mutation transaction.
- Host/Origin/custom-header protection, output limits, bounded commands and sanitized errors.
- Explicit tracking initialization starts a new conservative observation window for pre-existing cards. No unsafe sidecar-based age inference.

## Executed checks

- `cargo test --workspace --offline`: **65 passed**, exit 0.
- `cargo clippy --workspace --offline -- -D warnings`: exit 0.
- `python3 -m unittest discover -s scripts/tests -v`: **19 passed**, exit 0, including the final expanded real-CLI purge race regression.
- `node --test crates/web/tests/*.test.mjs`: **27 passed**, exit 0.
- `cargo build -p hermes-dash-api --offline`: exit 0; actual API binary produced.
- `cargo build --offline -p hermes-dash-api --example retention_fixture`: exit 0.
- `dx build --platform web --offline --locked` from `crates/web`: exit 0; client build completed and six local font/licence assets were copied. Confirmed the wasm contains retention confirmation UI.
- `git diff --check`: exit 0.
- Feature-only added-line security scan: no `shell=True`, `os.system`, `pickle.loads`, or hardcoded secret assignments detected. SQL values use placeholders; interpolated tracking SQL identifiers are fixed program constants.

## Destructive-operation regression evidence

All mutations were confined to disposable fixture databases.

- Successful real Hermes CLI archive and purge, then independent CLI read-back.
- Real Hermes comment inserted after retention intent creation but before archive: archive rejected, card remained Done.
- Real Hermes comment inserted after intent creation but before purge: purge rejected; card, comments, events and dependency links all survived byte-for-byte row comparison. Prepared intent removed afterwards.
- Separate-connection SQLite checks for concurrent reopen and linked-child reopen; even relation cleanup before the guarded DELETE rolled back.
- Rearchive followed by event GC retains a recent durable clock; old sidecar timestamps cannot authorize deletion.
- Missing tracking fails closed; dry-run does not write retention state.
- Disabling remains possible after the scoped database disappears.
- Backup failure blocks mutations; failed attempts are throttled; locks prevent overlapping runs.

## Browser verification

**9 successful runs:** Chromium 3, Firefox 3, WebKit 3. Each run used an isolated fixture API on an ephemeral loopback port, with browser requests intercepted so the existing app on 8081/8788 was not used.

Each exercised tracking initialization, invalid input, policy persistence/reload, typed enable confirmation, disabling, preview, typed apply confirmation, actual CLI archive/purge and read-back.

- Desktop/light: 1440 × 1000 viewport.
- Mobile/dark: 390 × 844 viewport.
- Contrast audit: 30 rendered text elements in light mode and 24 in dark mode; no threshold failures per run.
- No unexpected external requests, recorded page/console errors, or horizontal Settings overflow.
- Final mobile screenshot inspected for field/button clipping and overlap; none identified.
- No pixel-diff assertion against the prior application was made: this is a new Settings panel, not an existing visual baseline.

WebKit host libraries were unavailable. The existing matching Playwright container image was used instead, without pulling/installing packages. The test container and disposable servers were stopped afterwards.

Evidence directories on this host:

- `/home/gnu/.hermes/cache/scratch/pengudeck-retention-checks/`
- `/home/gnu/.hermes/cache/scratch/pengudeck-retention-final-browser/`
- `/home/gnu/.hermes/cache/scratch/pengudeck-retention-browser-webkit/`

## Independent review

Initial review identified blockers in snapshot-to-CLI races, rearchive/GC age inference, and disabling with a missing board. They were addressed with writer-maintained SQLite clocks/guards and a board-independent disable path.

Final independent review verdict: **passed**, no remaining security concerns or logic errors. Its optional suggestion to extend real-CLI race coverage to purge was implemented and the full Python suite rerun successfully afterwards. The final review was read-only, not a live-board activation test.

## Production boundaries and limitations

- No live task deletions, live tracking initialization, Hermes configuration changes, service restarts, or deployment occurred.
- Read-only checks of default/reporty/zaha databases found no `pengudeck_retention_*` or `pgret_*` schema objects. The live retention state directory does not exist.
- Existing app listeners remain loopback-only on 8081/8788; only the pre-existing Caddy and Matrix containers remain running.
- Automatic execution stays disabled until explicitly configured and enabled. Tracking needs a separate typed initialization action in Settings and changes the selected databases' schema.
- Tracking remains active inside initialized databases while automation is disabled, to preserve activity/rearchive history.
- Database backups retain deleted content indefinitely; this is not secure erasure. Workspace/attachment files are not backed up by the database backup.
- Loopback/Origin checks are not user authentication. Do not publish the write API without an authentication design.
- Compatibility was tested with the installed Hermes version/schema. Future schema migrations require revalidation; missing required tracking fails closed.

Operator instructions and standalone examples: `docs/RETENTION.md`.
