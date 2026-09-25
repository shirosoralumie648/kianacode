# UI-23 Web accessibility, focus and content security baseline

> Snapshot date: 2026-09-25. UI-23 keeps the Web page as a bounded presentation projection and
> adds keyboard/focus, text-only content handling and nonce CSP guards. GitHub Actions owns the
> fixture and compile verification; local Cargo tests, builds, checks, clippy and smoke commands
> are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-23`](ui-entrypoints.md#step-ui-23) |
| source snapshot | `61d390ff` (UI-22 plus INT-15 integration) plus this UI-23 source slice |
| feature_status | `implemented` (typed accessibility scope, text-only sanitizer, focus trap, responsive/ARIA HTML, nonce CSP, deny-first fixture/source guard and CI wiring) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Web page → existing typed hydrate/timeline/inbox/detail/session/SSE projections → loopback Web router → `DaemonHost`; no browser executor or second authority |

## Accessibility and focus contract

The page exposes landmarks, skip navigation, live/status regions, keyboard shortcuts, visible
focus, a reusable focus trap for the onboarding modal and focus restoration to the element that
opened it. Approval/cancel controls remain normal buttons and are disabled when the server card,
owner tab, action decision, session or epoch is not valid. Narrow layouts reflow the sidebar,
conversation and details without hiding safety status; 200% zoom style constraints, high contrast,
forced-colors and reduced-motion preferences are explicit.

`WebFocusScope` binds a return target to the server session, tab and authority epoch. A stale
session/epoch is rejected before the target is focused. Loading, Partial, Unknown and Offline
states remain text-visible and Unknown never enables an automatic retry.

## Text-only and content security contract

Timeline, inbox, artifact, diff, receipt, checkpoint and command JSON values are projected through
bounded `textContent` sinks only. The page has no `innerHTML`, inline event handlers, `eval`, `new
Function`, arbitrary URL fetch or client diff/authorization path. `safeJsonText` routes structured
values through the same bounded sanitizer, while `sanitizeUntrustedContent` rejects NUL/control/
ANSI/OSC and executable URL schemes; literal HTML/Markdown remains inert text. Detail projections
also reject stale tab/instance scope before render. The typed client exposes the same bounded
`validate_text_only`/`sanitize_text_only` contract for non-browser presenters.

The embedded page carries a per-process nonce on its static style/script blocks (the roadmap's
migration exception is recorded here; there is no `unsafe-inline` or inline event handler). Every response receives
the matching `script-src 'nonce-*'` and `style-src 'nonce-*'` CSP, `nosniff`, `no-store`,
`no-referrer`, `DENY` framing, `object-src 'none'`, `base-uri 'none'` and `form-action 'self'`.
`securitypolicyviolation` becomes a visible, redacted status and `/api/csp-report` accepts only a
bounded host-scoped diagnostic body without echoing blocked URLs or source paths.

## Failure-first matrix

| Fixture / guard | Assertion |
|---|---|
| focus scope | focus cannot escape the modal; close returns only to a connected element in the same UI scope |
| action state | hidden/unknown/expired/revoked/non-owner actions remain disabled and Unknown cannot auto-retry |
| session/epoch/tab/instance | stale hydrate/detail/receipt projections are rejected before render/focus |
| content | HTML/SVG/Markdown/script, `javascript:`/`data:` URL, ANSI/OSC, unsafe structured text sinks, secret/raw content and `innerHTML` paths are denied or inert text |
| CSP | no `unsafe-inline`; nonce binds page to response header and a CSP report is visible rather than swallowed |
| responsive | keyboard, ARIA/live text, narrow/200% zoom, high contrast, forced colors and reduced motion remain explicit |

## Evidence block

```text
source_snapshot: 61d390ff (UI-22 plus INT-15 integration) plus UI-23 source slice
worktree_status: isolated /tmp/kiana-step-ui23; typed client focus/text contract, Web nonce CSP/report route, HTML focus/sanitizer/accessibility wiring, fixture/source guards, workflow and baseline
command_argv: target-only rustfmt --edition 2021 on UI-23 Rust files; git diff --check; GitHub Actions will run cargo fetch --locked, cargo fmt --all --check, UI-23 fixture/guard and cargo check --workspace --tests --locked
cwd·environment: /tmp/kiana-step-ui23; Linux/bash; local Cargo test/build/check/clippy/smoke deliberately not run; GitHub Actions is the test authority and is not awaited
fixture·cassette: kiana-entrypoints/tests/fixtures/ui23-web-accessibility.json; focus/session/epoch/action/unknown/XSS/URL/ANSI/secret/CSP/narrow/high-contrast/reduced-motion cases; no provider, filesystem or external effect contacted
exit_code: target-only rustfmt and git diff --check are the only local verification; remote fixtures, workspace compile and CI exit codes are pending/unobserved
status_change: UI-23 source slice is implemented; roadmap row/card advanced from ⏳ to 🔄 pending GitHub evidence
proof-level_change: feature_status=implemented; proof_level=source only
limitations: no browser/axe/screen-reader/keyboard device automation, screenshot/golden, real CSP report delivery, cross-process session lease race, durable projection, provider/Broker effect or live/physical proof; nonce is process-local and page remains a presentation projection
reviewer: Codex UI-23 source review; checked focus trap/restore, stale session/epoch, disabled hidden/Unknown actions, textContent-only DOM, control/URL sanitizer, nonce CSP/security headers and no second execution path; no local runtime test reviewer
```
