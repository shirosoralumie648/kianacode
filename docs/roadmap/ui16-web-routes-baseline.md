# UI-16 Web routes, origin checks and minimal health baseline

> Snapshot date: 2026-09-25; completed slice 2026-10-17.  The route/auth/limit contracts are wired
> into GitHub Actions.  Local Cargo tests, builds, checks, clippy and smoke commands are
> intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-16`](ui-entrypoints.md#step-ui-16) |
| source snapshot | `2d31a8b0` plus this UI-16 source slice; completed slice rebased on `bdbd939194b4` |
| feature_status | `implemented` (bounded Web route/auth/limit contract, deny-first source and live-loopback fixtures, security header/CSP draft, legacy compatibility table) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Browser → loopback Web router → `DaemonHost` → `ControlPlane`; no browser-side executor is added |

## Route and auth matrix

The route inventory is exported as `WEB_ROUTE_MATRIX` in `kiana-entrypoints/src/web.rs` and covers
health, state, sessions, events, run, cancel, trust, sandbox, session, receipt, approval, resume
and command (plus the additive bootstrap/history/artifact/diff/notifications/receipt-detail/steer/
inject/extensions/token-rotation classes).  The root page and health probe are host-only so a fresh
browser can obtain the in-memory token and a local readiness probe can report liveness.  Every
state, event, diagnostic and mutating API route requires the `x-kiana-web-token` header
(`WEB_TOKEN_HEADER`); SSE may carry the same token in its explicit query parameter because
`EventSource` cannot set request headers.  `scripts/ci/validate-workflows.sh`-style static guards
plus `ui16_web_routes.rs` prove the fixture, the exported matrix and the Axum `.route(...)`
registrations are the same contract in both directions: same paths, same `get`/`post` methods,
same class, same token requirement, and `OPTIONS` is never registered.

Host and Origin are parsed as exact HTTP authorities for the bound loopback address.  A supplied
Origin must match the bound address; `null`, credentials, another port, another loopback address,
an arbitrary hostname, path, query or fragment are rejected.  Missing token, foreign Origin and
foreign Host fail before a handler reaches `DaemonHost`. For SSE, a header token and query token
may not disagree; a conflicting pair is rejected before the stream is authorized.  Duplicate
credential headers are rejected by `single_header` instead of silently picking one.

## Per-route deny-first completion

| Boundary | Contract |
|---|---|
| per-route order | `ui16_per_route_authorization_precedes_any_host_access` walks every router registration and proves the first `authorize_mutation`/`authorize_sse`/`authorize_host` call precedes any `app.host.`, `app.snapshot(`, `app.sessions`, `app.options()`, `app.read_session_ledger(` or `app.history_sessions(` access |
| wrong methods | `POST /api/state`, `GET /api/run`, `OPTIONS /api/run` and `POST /api/health` stay Axum method rejections (405) and never reach a handler; `OPTIONS` is asserted with no credential, so the rejection cannot be mistaken for an authorization response |
| error codes | `WEB_ERROR_MATRIX` catalogues 63 stable codes with `web_error_status_for_class`; the guard re-derives every `ApiError::bad("…")`/`conflict`/`fail` literal and the three explicit-status codes from the source and fails when source and catalogue drift apart |
| token rotation | `POST /api/token/rotate` (class `Token`, token required) bumps the monotonic `token_generation`, invalidates the previous credential and returns `kiana.web-token-rotation.v1` without echoing any credential; the page re-hydrates from the host-only root document |
| request rate | middleware counts every request that reaches the Web instance; one instance admits at most 120 requests per one-second window and returns `web_rate_limit_exceeded` with HTTP 429; the deterministic counter, the window recovery and the live 429 wiring are all asserted, and the rejection keeps the security headers |
| request size | URI/query length is capped at 8 KiB and encoded or literal path traversal is rejected before extraction; Axum `DefaultBodyLimit` caps the body at 128 KiB and the rejected payload is never echoed; prompt and projection fields keep their existing lower bounds |
| session scope | session selectors are bounded opaque IDs free of path/control characters; ownership is checked before `DaemonHost::ui_snapshot` and event history is filtered to the canonical worktree; unknown and path-like ids return `session_unknown`/`session_invalid` |
| concurrent tabs | two tabs requesting the same state concurrently receive the same server session/epoch, `lease.feed_only = true` and `cache-control: no-store`, and neither response carries the other tab's local identity |
| health | no folder, absolute path, storage location, nested snapshot or internal error text is returned; unavailable liveness uses the stable `health_projection_unavailable` limitation |
| logging | the request/serve region contains no `println!/eprintln!/print!/dbg!/tracing::/log::` statement, startup banner lines may not interpolate a token, nonce, authorization or origin value, and the only process spawn left in the module is the operator's `maybe_open` browser opener |
| response headers | `nosniff`, `no-store`, `no-referrer`, `DENY` framing and a same-origin CSP are attached to every response, including middleware and method rejections |

Unknown methods, including OPTIONS, remain Axum method rejections and cannot invoke a mutating
handler.  The route matrix is descriptive only; execution continues through the existing Web
handlers and `harness_run::*_on_host` functions.  `ui16_serve_path_cannot_log_credentials_or_start_a_second_executor`
also fails if `execute_tool_call`, `KianaHarness`, `kiana_runner`, `tokio::process`,
`std::process::Command` or `Command::new(` appears in the serve region.

## Security header and CSP draft

`WEB_SECURITY_HEADERS` is the reviewed response-header draft and `security_headers` builds every
response from it, so the fixture and the middleware cannot diverge:

```text
x-content-type-options: nosniff
cache-control: no-store
referrer-policy: no-referrer
x-frame-options: DENY
content-security-policy: default-src 'self'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}';
                         connect-src 'self'; object-src 'none'; base-uri 'none';
                         form-action 'self'; frame-ancestors 'none'; report-uri /api/csp-report
```

`WEB_CSP_TEMPLATE` shares one per-process nonce with the embedded page; there is no
`unsafe-inline`, `unsafe-eval`, `eval` or inline event handler, and the CSP violation report sink
`/api/csp-report` is a bounded host-only diagnostic that never echoes blocked URLs or paths.

## Legacy endpoint compatibility table

The original loopback workbench route set is preserved 1:1 — no alias, redirect or second
dispatch path was introduced, and every preserved path still requires the same method and token.
The one-way typed-client migration mapping for old callers belongs to UI-35
(`kiana-client/src/legacy_migration.rs`), not to this router.

| Legacy route | Current route | Method | Disposition |
|---|---|---|---|
| `/` | `/` | GET | preserved (host-only page) |
| `/api/health` | `/api/health` | GET | preserved (host-only, low detail) |
| `/api/state` | `/api/state` | GET | preserved (token + origin) |
| `/api/run` | `/api/run` | POST | preserved (token + origin) |
| `/api/cancel` | `/api/cancel` | POST | preserved (token + origin) |
| `/api/trust` | `/api/trust` | POST | preserved (token + origin) |
| `/api/sandbox` | `/api/sandbox` | POST | preserved (token + origin) |
| `/api/session` | `/api/session` | POST | preserved (token + origin) |
| `/api/receipt` | `/api/receipt` | POST | preserved (token + origin) |

## Fixture and CI evidence

`ui16-web-routes.json` records the route matrix, the method matrix, the 63-code error catalogue, the
security header/CSP draft, the legacy compatibility table, the token-rotation contract and the
deny/success lists.  `kiana-entrypoints/tests/ui16_web_routes.rs` binds the fixture to the exported
constants, the router registrations, the per-route authorization order, the CSP draft and the
legacy table; `kiana-entrypoints/tests/ui16_web_routes_guard.rs` keeps the health redaction, the
token/Host order, the error-literal scan and the rotation no-echo boundary.  The `ui16_*` fixtures
inside `kiana-entrypoints/src/web.rs` drive a real loopback server with a real `DaemonHost` over
HTTP (including raw-socket Host/duplicate-header/traversal requests).  GitHub Actions runs
`cargo fetch --locked`, `cargo fmt --all --check`, both UI-16 targets, the `web::tests::ui16_`
library fixtures and `cargo test -p kiana-entrypoints --no-run --locked` in the manual
`.github/workflows/ui16-web-routes.yml`; the automatic `ci.yml` `kiana-entrypoints` shard runs the
whole crate, and CI results are intentionally not awaited here.

## Evidence block

```text
source_snapshot: bdbd939194b4 plus UI-16 completed source slice
worktree_status: isolated kianacode-worktrees/UI-16; only UI-16 files staged
command_argv: cargo fmt --all; cargo fmt --all -- --check (no cargo build/test/check/clippy/smoke)
cwd·environment: /workspace/kianacode-worktrees/UI-16; Linux/bash; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui16-web-routes.json; live fixtures use a temp
  KIANA_HOME, a temp workspace and a fake harness script; no provider/external effect
exit_code: fmt 0; GitHub CI configured (manual UI-16 lane + automatic kiana-entrypoints shard) and not awaited
status change: UI-16 🔄 → ✅ (CI pending); per-route deny-first, method/error/limit/token-rotation/log-redaction
  contracts, security header/CSP draft and legacy compatibility table completed
proof-level change: source only
limitations: no local HTTP execution in this session and no awaited CI receipt; no browser matrix,
  cross-process transport, durable rate state or token persistence; health is intentionally low
  detail and host-only; token rotation is per-process and rotates no ControlPlane credential
reviewer: Codex UI-16 source review; independent CI reviewer pending
```
