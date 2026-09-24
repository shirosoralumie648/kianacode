# UI-19 Web session ownership and multi-tab baseline

> Snapshot date: 2026-09-25. UI-19 adds a bounded server lease projection and tab-local Web
> draft/submission state on the existing loopback Web → DaemonHost → ControlPlane path. GitHub
> Actions owns runtime verification; local Cargo tests, builds, checks, clippy and smoke commands
> are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-19`](ui-entrypoints.md#step-ui-19) |
| source snapshot | UI-18 HEAD plus this UI-19 source slice |
| feature_status | `implemented` (bounded lease, owner checks, idempotent replay and source fixtures) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Browser tab → loopback Web → authenticated server principal → DaemonHost → ControlPlane → Broker |

## Ownership and tab contract

`kiana.ui-tab-session.v1` binds the server authenticated principal, session, tab, lease epoch and
token generation. The first bootstrap tab receives the mutation lease. Other tabs may attach the
same owner-scoped read-only feed, but `run`, `cancel`, approval, resume and command mutations
require the exact owner tab. A tab ID is a scope label, never a credential or a permission grant.

Draft text and submission identity are kept in per-tab memory. A submission carries a stable
action ID/idempotency key; a repeated click or lost HTTP response reuses it and returns the original
cached command payload. A second payload, tab, principal or target for that key is rejected. The
existing UI cursor CAS remains a server-side stale-card fence before the handler reaches the
ControlPlane.

## Deny-first and recovery matrix

| Boundary | Contract |
|---|---|
| cross-tab mutation | observer tab can consume feed but `session_owner_required` denies action |
| stale card/CAS | server `claim_ui_headers` rejects old epoch/cursor before dispatch |
| duplicate click | bounded submission registry returns original response; no second handler effect |
| close tab | `beforeunload` only closes SSE; it never posts `/api/cancel` or beacon mutation |
| token rotation | server token generation is included in lease metadata; old token is unauthorized |
| owner exit | lease loss does not synthesize cancellation; receipt remains authoritative |
| race | session owner and UI cursor checks are serialized before the existing execution spine |
| feed observation | every tab subscribes through `/api/events` with its own tab ID; no shared mutable draft |

## Fixtures and CI evidence

`ui19-session-tabs.json` covers principal/session/tab/lease/token scope, tab A → tab B denial,
stale card, duplicate click, close-without-cancel, token rotation, owner exit, CAS race, tab-local
drafts, original receipt replay and feed-only observation. `ui19_session_tabs.rs` checks the versioned
protocol/client/Web markers; `ui19_session_tabs_guard.rs` checks owner/CAS fences and no reconnect or
unload side-effect command. GitHub Actions runs both fixtures and workspace test-target compilation;
results are intentionally not awaited.

## Limitations

- Lease/submission maps and token generation are bounded Web process memory; restart, multi-process
  deployment and external browser authentication are not durable/live proof.
- Cached replay is the HTTP response projection; the ControlPlane/EventLog receipt remains the
  authority for actual effects and `result_unknown` reconciliation.
- No browser automation, real sleep/wake timing, external provider, network, OS or physical proof
  is claimed. Closing a tab is deliberately not treated as a cancellation command.

## Evidence block

```text
source_snapshot: UI-18 HEAD plus UI-19 source slice
worktree_status: isolated /tmp/kiana-step-ui19; Web/protocol/client contracts, fixtures, workflow and docs only
command_argv: rustfmt --edition 2021 on touched Rust; git diff --check
cwd·environment: /tmp/kiana-step-ui19; Linux/bash; no Cargo test/build/check/clippy/smoke; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui19-session-tabs.json; deny-first source contracts; no external effect
exit_code: target rustfmt and git diff --check only; GitHub Actions configured and not awaited
status change: UI-19 ⏳ → 🔄; server lease, tab scope, stale/CAS, replay and close-without-cancel guards added
proof-level change: feature_status=implemented; proof_level=source only
limitations: no browser/runtime/durable/live/physical proof; bounded process-local leases and replay cache; CI result not awaited
reviewer: Codex UI-19 source review; independent GitHub CI reviewer pending
```
