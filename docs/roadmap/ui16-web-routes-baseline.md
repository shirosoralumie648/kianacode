# UI-16 Web routes, origin checks and minimal health baseline

> Snapshot date: 2026-09-25.  The route/auth/limit contracts are wired into GitHub Actions.
> Local Cargo tests, builds, checks, clippy and smoke commands are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-16`](ui-entrypoints.md#step-ui-16) |
| source snapshot | `2d31a8b0` plus this UI-16 source slice |
| feature_status | `implemented` (bounded Web route/auth contract and deny-first source fixtures) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Browser → loopback Web router → `DaemonHost` → `ControlPlane`; no browser-side executor is added |

## Route and auth matrix

The route inventory is exported as `WEB_ROUTE_MATRIX` in `kiana-entrypoints/src/web.rs` and covers
health, state, sessions, events, run, cancel, trust, sandbox, session, receipt, approval, resume
and command.  The root page and health probe are host-only so a fresh browser can obtain the
in-memory token and a local readiness probe can report liveness.  Every state, event, diagnostic
and mutating API route requires `x-kiana-web-token`; SSE may carry the same token in its explicit
query parameter because `EventSource` cannot set request headers.

Host and Origin are parsed as exact HTTP authorities for the bound loopback address.  A supplied
Origin must match the bound address; `null`, credentials, another port, another loopback address,
an arbitrary hostname, path, query or fragment are rejected.  Missing token, foreign Origin and
foreign Host fail before a handler reaches `DaemonHost`.

## Deny-first boundaries

| Boundary | Contract |
|---|---|
| request target | URI/query length is capped at 8 KiB; encoded or literal path traversal is rejected before extraction |
| request body | Axum `DefaultBodyLimit` caps the body at 128 KiB; prompt and projection fields keep their existing lower bounds |
| request rate | request middleware counts every request that reaches the Web instance; one instance admits at most 120 requests per one-second window and returns `web_rate_limit_exceeded` with HTTP 429; this bounds unauthorized floods as well as legitimate tabs |
| session scope | session selectors are bounded opaque IDs free of path/control characters; ownership is checked before `DaemonHost::ui_snapshot` and event history is filtered to the canonical worktree |
| health | no folder, absolute path, storage location, nested snapshot or internal error text is returned; unavailable liveness uses the stable `health_projection_unavailable` limitation |
| response headers | `nosniff`, `no-store`, `no-referrer`, `DENY` framing and a same-origin CSP are attached to every response |

Unknown methods, including OPTIONS, remain Axum method rejections and cannot invoke a mutating
handler.  The route matrix is descriptive only; execution continues through the existing Web
handlers and `harness_run::*_on_host` functions.

## Fixture and CI evidence

`ui16-web-routes.json` records the route matrix, limits and deny cases.  The two UI-16 tests are
source contracts: they verify route/auth markers, loopback/origin/token checks, body/URI/rate
bounds, path/session fences, security headers and health redaction.  GitHub Actions runs
`cargo fetch --locked`, `cargo fmt --all --check`, both UI-16 fixtures and
`cargo check --workspace --tests --locked`; CI results are intentionally not awaited here.

## Evidence block

```text
source_snapshot: 2d31a8b0 plus UI-16 source slice
worktree_status: isolated /tmp/kiana-step-ui16; only UI-16 files staged; unrelated WIP preserved
command_argv: rustfmt --edition 2021 kiana-entrypoints/src/web.rs; git diff --check
cwd·environment: /tmp/kiana-step-ui16; Linux/bash; no Cargo test/build/check/clippy/smoke; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui16-web-routes.json; no runtime/provider/external effect
exit_code: rustfmt 0; git diff --check 0; GitHub CI configured and not awaited
status change: UI-16 ⏳ → 🔄; route/auth/limit and health-redaction source contracts added
proof-level change: source only
limitations: no local HTTP behavior, browser matrix, cross-process transport, token rotation over restart,
  durable rate state or live/physical proof; health is intentionally low detail and host-only
reviewer: Codex UI-16 source review; independent CI reviewer pending
```
