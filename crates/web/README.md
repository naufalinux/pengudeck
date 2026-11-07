# Hermes dashboard web — t_70a0f24f

Dioxus web `=0.7.10`; the only direct dependencies are Dioxus and
`hermes-dash-core`. Browser HTTP/JSON uses `document::eval` and native `fetch`;
no HTTP client, JS framework, or additional Rust dependency is needed.

## Build and verify

```sh
cargo test --workspace
cd crates/web
dx build --platform web
```

Output: `target/dx/hermes-dash-web/debug/web/public/` from the workspace root.
Do not run a persistent dev server as part of verification.

**Build-helper caveat:** dx can automatically download wasm-bindgen and esbuild
when its helper cache is empty. The first T6 build did so (wasm-bindgen-cli
0.2.129 and esbuild 0.27.3). No package manager or `cargo install` was invoked.
For an installation-free/offline environment, have the owner pre-provision the
helper cache; do not assume that an installed dx alone prevents downloads.

Optional fetch-program regression tests use an existing Node.js runtime and
only Node built-ins (no npm install):

```sh
node --test crates/web/tests/fetch.test.mjs # from workspace root
```

## Runtime behavior

The API base is the Rust constant `http://127.0.0.1:8788`. Board options come
from `GET /api/boards`; choose one to fetch `GET /api/boards/{slug}/tasks`.
There is no hardcoded default board. All six columns remain visible, including
empty columns. Core owns grouping and sorts cards within each column by
`last_updated` descending (newest first). Cards without a timestamp come last;
equal or missing timestamps retain their original relative order. Sorting does
not modify the source tasks. Card content is rendered as text, never HTML.
A changed selection cancels the prior resource and excludes stale-board results.
The reload button retries requests. Requests time out after ten seconds and
omit credentials. Errors are short static messages; response error bodies and
exception details are neither rendered nor logged by this code.

## Integration limitation (not a browser-live acceptance claim)

The existing API does not set CORS response headers or serve these static files.
A UI hosted on another port/origin will therefore show a network error for
cross-origin API reads. Do not disable browser security, use `no-cors`, or add
wildcard CORS as a workaround. Zaha must separately scope either same-origin
static hosting at the fixed API origin or narrowly allowed local UI origins.
T6 deliberately changes neither `crates/api` nor `crates/core`.

T6's browser smoke used intercepted **synthetic fixtures** with CORS allowed to
verify actual built WASM, board switching, escaped card values, and HTTP/network
error UI. This is not evidence of a working live cross-origin API integration.
Independent QA remains on task `t_4026f226`.

No database changes or migrations. Rollback: revert T6 commits in reverse order
(including workspace membership and lockfile changes). No production deployment.
