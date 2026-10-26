# t_fb574c29 — implementation handoff

## Scope / checkout

- Base: `7fc7e211571629c8bae423e87f47ce39969397cc`.
- Isolated worktree: `~/work/git/hermes-dash/t_fb574c29`.
- Branch: `agent/archi/t_dad2d4e5-cors`; no push.
- Implementation commits: `4837dab`, `9c80bd8`.
- Only `crates/api` changed; Cargo manifests/lockfile, core and web unchanged.
- Original T6 checkout remains clean on `agent/archi/t_70a0f24f-web` at base SHA.

## Behavior

Router-wide middleware covers GET `/api` and `/api/` subtree, including fallback/errors. Exactly one Origin header equal byte-for-byte to `http://127.0.0.1:8080` receives ACAO. Missing, malformed, duplicate or other origins do not; responses retain existing status/body. Vary merges using an additional header field, preserving existing fields and recognizing case-insensitive Origin tokens and wildcard Vary. No wildcard ACAO, credentials, expose headers, private-network or preflight support. Non-GET behavior is unchanged, including implicit axum HEAD handling.

## Verification (deterministic, not live acceptance)

| Command | Exit | Result |
| --- | ---: | --- |
| `CARGO_NET_OFFLINE=true cargo test --offline -p hermes-dash-api --test cors` (RED) | 101 | 1 expected failure: missing ACAO |
| Same command after first implementation | 0 | 1 passed |
| `CARGO_NET_OFFLINE=true cargo test --offline -p hermes-dash-api --lib` (Vary RED) | 101 | 1 expected failure: duplicate Origin Vary token |
| `CARGO_NET_OFFLINE=true cargo test --offline --workspace` | 0 | 48 passed, 0 failed, 0 ignored; 6 new CORS tests included |
| `CARGO_NET_OFFLINE=true cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | clean; initial combined command timed out (exit 124), standalone retry passed |
| `cargo fmt --all --check` | 0 | clean |
| `node --test crates/web/tests/fetch.test.mjs` | 127 | node absent from worker PATH |
| `~/.nvm/versions/node/v24.22.0/bin/node --test crates/web/tests/fetch.test.mjs` | 0 | 10 passed, 0 failed; existing Node v24.22.0, no install |
| `git diff --check 7fc7e21` | 0 | clean |

Coverage: exact origin, missing/null/malformed/external origins, localhost, wrong port/scheme, deceptive prefix/suffix, comma/space lists, duplicate headers in either order; statuses 200/400/404/502, unknown API fallback, `/api` root, non-API boundary, HEAD/POST/OPTIONS/PUT/DELETE/PATCH. 502 test asserts exact sanitized body. Vary tests cover multiple fields, existing mixed-case Origin token, wildcard Vary. Fake sources are test-only.

Raw execution logs are in worktree `evidence/` and the attached evidence archive. Test total calculated by `evidence/count_tests.py`.

## Operation / rollback / remaining QA

`ss -ltnp '( sport = :8080 or sport = :8787 )'` showed neither listener before or after implementation. No server process was started or killed. API listener remains fixed at 127.0.0.1:8787. UI must use numeric `http://127.0.0.1:8080`, not localhost. CORS is not authentication and does not deny non-browser requests.

Strict no-install/no-download honored: Cargo ran offline; no dx, cargo fetch, package-manager or helper download was run. WASM rebuild and helper-cache compatibility were not exercised; this task changes API only. Do not infer a fresh strict web build from Rust host tests. For any future dx build, verify helper cache and actual network isolation first per the plan; otherwise mark that rebuild BLOCKED.

No database/config migration. Roll back the two implementation commits in reverse order on a review branch, or use the base SHA; no data rollback needed.

Independent QA child `t_d9775048` owns remaining acceptance: real browser/WASM at origin 8080, live API/CLI snapshots for default and zaha, screenshots/network/console evidence, negative-origin browser enforcement. None of those are claimed PASS here. Implementation completion only releases Kevin's QA; product remains unaccepted pending his verdict.
