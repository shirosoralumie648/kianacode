# UI-18 Web SSE reconnect, Last-Event-ID and gap baseline

> Snapshot date: 2026-09-25. UI-18 adds a bounded SSE reconnect contract to the existing
> loopback Web projection. GitHub Actions owns runtime verification; local Cargo tests, builds,
> checks, clippy and smoke commands are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-18`](ui-entrypoints.md#step-ui-18) |
| source snapshot | `5abce647` plus this UI-18 source slice |
| feature_status | `implemented` (bounded SSE metadata, reconnect state machine and deny-first source fixtures) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Browser EventSource → loopback `/api/events` → `DaemonHost` run-stream display projection; actions remain HTTP/ControlPlane |

The Web stream remains a read-only display projection. It emits the server-owned `id`, event name,
schema, epoch and sequence. Run deltas retain `kiana.protocol.v1`; transport control frames use
`kiana.web-sse.v1`. `Last-Event-ID` is read from the HTTP header, with a query fallback for the
native browser wrapper; conflicting values fail closed. A bounded payload fallback prevents a
large provider delta from turning the browser buffer into an unbounded stream.

## Reconnect and failure-first contract

| Boundary | Contract |
|---|---|
| listener ordering | Event listeners are installed before `onopen` can release the snapshot-first caller; the `/api/run` command is sent only by the explicit turn submission path |
| cursor | `epoch:sequence` is server-owned; duplicate IDs are ignored, sequence gaps and old epochs trigger hydration, and invalid/conflicting cursors are rejected |
| control frames | explicit bounded heartbeat; gap/error carry schema, epoch, sequence and `hydrate=true`; terminal closes the display stream and remains receipt-authoritative |
| browser reconnect | six attempts maximum with 250 ms exponential backoff capped at 8 s; reconnect only recreates EventSource and carries the last cursor, never `/api/run`, `/api/cancel` or another side-effect command |
| gap recovery | the browser closes the stale listener, requests a fresh server snapshot and resumes only after hydration; incomplete display never becomes completed execution |
| secret boundary | the Web token is accepted only by the existing loopback auth path; SSE data, event IDs, browser history and diagnostics never contain the token |

The daemon subscription and broadcast remain bounded. Slow consumers receive an explicit stream
error/gap and must hydrate; they cannot block EventLog/ControlPlane facts. Existing event names and
payload envelope shape stay compatible with the previous `/api/events` fixture while control metadata
is additive.

## Fixture and CI evidence

`ui18-web-sse.json` covers event metadata, header/query cursor precedence, bounded frame and
reconnect limits, duplicate/gap/old-epoch/oversized/token-leak/re-submit/terminal-after-delta
denials and snapshot hydration. `ui18_web_sse.rs` checks the source contract; the guard checks that
the SSE route is read-only and reconnect code contains no command submission. GitHub Actions runs
the two fixtures and workspace test-target compilation; results are intentionally not awaited.

## Limitations

- SSE cursors, browser reconnect state and terminal retention are process-local bounded projections;
  restart or epoch change requires snapshot hydration.
- The source slice does not claim durable cross-process browser subscriptions, external network
  delivery, provider/live timing, exactly-once effects, receipt correctness or physical proof.
- A gap/incomplete display is not an execution outcome; the ControlPlane/EventLog Receipt remains the
  authority for terminal status and side effects.

## Evidence block

```text
source_snapshot: 5abce647 plus UI-18 source slice
worktree_status: isolated /tmp/kiana-step-ui18; only UI-18 files staged; unrelated worktrees preserved
command_argv: rustfmt --edition 2021 kiana-entrypoints/src/web.rs; git diff --check
cwd·environment: /tmp/kiana-step-ui18; Linux/bash; no Cargo test/build/check/clippy/smoke; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui18-web-sse.json; source-only browser/SSE contract; no external effect
exit_code: target rustfmt and git diff --check are the only local checks; GitHub Actions configured and not awaited
status_change: UI-18 ⏳ → 🔄; bounded Last-Event-ID, heartbeat, backoff, gap→hydrate and deny-first guards added
proof-level change: feature_status=implemented; proof_level=source only
limitations: no live HTTP/browser timing, durable cross-process cursor, provider/live stream, external effect or physical proof; CI result not awaited
reviewer: Codex UI-18 source review; independent GitHub CI reviewer pending
```
