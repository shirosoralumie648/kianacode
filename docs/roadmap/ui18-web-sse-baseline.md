# UI-18 Web SSE reconnect, Last-Event-ID and gap baseline

> Snapshot date: 2026-09-25; completion slice 2026-10-17. UI-18 adds a bounded SSE reconnect
> contract to the existing loopback Web projection. GitHub Actions owns runtime verification; local
> Cargo tests, builds, checks, clippy and smoke commands are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-18`](ui-entrypoints.md#step-ui-18) |
| source snapshot | `5abce647` SSE slice + the 2026-10-17 completion slice on top of UI-17 |
| feature_status | `implemented` (bounded SSE frames, reconnect state machine, observable reconnect metrics, deny-first runtime fixtures) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Browser EventSource → loopback `/api/events` → `DaemonHost` run-stream display projection; actions remain HTTP/ControlPlane |

The Web stream remains a read-only display projection. It emits the server-owned `id`, event name,
schema, epoch and sequence. Run deltas retain `kiana.protocol.v1`; transport control frames use
`kiana.web-sse.v1`. `Last-Event-ID` is read from the HTTP header, with a query fallback for the
native browser wrapper; conflicting values fail closed. A bounded payload fallback prevents a
large provider delta from turning the browser buffer into an unbounded stream.

## Frame and cursor contract

| Boundary | Contract |
|---|---|
| frame fields | `id` = `epoch:sequence`, event name from the server mapping, `kiana.protocol.v1` envelope for run frames and `kiana.web-sse.v1` for heartbeat/gap/error |
| cursor source | `epoch:sequence` is server-owned; the browser never mints one |
| duplicates | an already-seen cursor is dropped, never re-rendered (`duplicate_frame`) |
| skipped frames | a non-dense sequence is refused and hydrates a fresh snapshot (`stream_sequence_gap`) |
| old epoch | a previous daemon incarnation is refused and hydrates a fresh snapshot (`stream_epoch_changed`) |
| oversized frames | `MAX_WEB_SSE_EVENT_BYTES` (256 KiB) degrades to a bounded `stream_payload_too_large` frame carrying the cursor it belongs to |

## Reconnect and failure-first contract

| Boundary | Contract |
|---|---|
| listener ordering | Event listeners are installed before `onopen` can release the snapshot-first caller; the `/api/run` command is sent only by the explicit turn submission path |
| frame pump | `next_run_stream_frame` is the single implementation of the heartbeat/gap/error/terminal mapping used by `/api/events` and by the boundary fixtures |
| control frames | explicit bounded heartbeat; gap/error and oversized-frame fallbacks carry schema, epoch, sequence and `hydrate=true`; terminal closes the display stream and remains receipt-authoritative |
| browser reconnect | six attempts maximum with 250 ms exponential backoff capped at 8 s; reconnect only recreates EventSource and carries the last cursor, never `/api/run`, `/api/cancel` or another side-effect command |
| gap recovery | the browser closes the stale listener, requests a fresh server snapshot and resumes only after hydration; incomplete display never becomes completed execution |
| slow tab | a consumer that falls behind the bounded queue (256 frames) receives one explicit `stream_error` (`stream_subscription_lagged:<n>`) with the cursor it attached at; facts keep committing and a fresh subscriber still resumes from the retained terminal |
| terminal retention | a reconnecting tab is served the retained terminal once and never the deltas it already rendered; `begin_turn` replaces the retained terminal so a stale one cannot be replayed |
| metrics | `kiana.web-sse-metrics.v1` is exposed as `window.__KIANA_SSE_METRICS__` and rendered in `#sseMetrics`: bounded counters (installs/connects/errors/reconnects/exhausted/hydrate/duplicate/sequence gaps/stale epochs/heartbeats/incomplete), the last server cursor, and at most 8 reasons; no token, prompt, approval payload or artifact bytes |
| secret boundary | the Web token is accepted only by the existing loopback auth path; SSE data, event IDs, browser history and diagnostics never contain the token |

The daemon subscription and broadcast remain bounded. Slow consumers receive an explicit stream
error/gap and must hydrate; they cannot block EventLog/ControlPlane facts. Existing event names and
payload envelope shape stay compatible with the previous `/api/events` fixture while control metadata
is additive.

## Fixture and CI evidence

`kiana-entrypoints/tests/fixtures/ui18-web-sse.json` (`kiana.ui18-web-sse.v1`) declares the frame
fields, the `Last-Event-ID` header/query contract with its denial codes, the server/browser/queue
budgets (256 KiB frame, 256-byte cursor, 512-byte reason, 15 s heartbeat, 256-frame queue, 6
reconnects 250 ms→8 s, 8 metric reasons), the event and lifecycle sets, `deny_first`,
`server_required`, `browser_required`, `browser_forbidden`, the `/api/events` route, the named
fixtures, `required` markers and `limitations`. `ui18_web_sse.rs` binds the fixture to the real
source constants, the route matrix and the browser contract; `ui18_web_sse_guard.rs` keeps the
display route read-only, the reconnect path command-free, the metrics credential-free, and binds the
fixture, the manual lane, the automatic shard and this baseline together.

Runtime fixtures (`kiana-entrypoints/src/web.rs`, `web::tests::ui18_*`) drive a real loopback
`DaemonHost` run over the wire:

| Fixture | What it proves |
|---|---|
| `ui18_events_frames_carry_server_owned_cursors_and_fence_last_event_id_conflicts` | `id`/event/schema/epoch/sequence on the wire, dense server cursor, token-free frames, conflicting/malformed/oversized cursor denials before any subscription, `POST /api/events` is a method rejection, duplicate header and query cursors replay nothing, an old epoch starts with a snapshot-required gap on the current epoch |
| `ui18_reconnect_and_gap_paths_never_resubmit_effectful_commands` | listener-before-command order, a reconnect from an older cursor gets `snapshot_required_after_stream_gap` and never re-renders a consumed delta, exactly one `run.authorized` and one `run.prompt` fact exist after the reconnect |
| `ui18_terminal_retention_replays_the_latest_terminal_without_deltas` | the retained terminal is replayed once after a completed turn, an already-consumed cursor replays nothing, resuming from the retained cursor observes the next turn live, and a stale cursor only ever replays the newest terminal |
| `ui18_slow_tab_becomes_an_explicit_lagged_error_without_blocking_facts` | a subscription that never drains past the 256-frame queue receives one explicit `stream_subscription_lagged` error with its attach cursor, `state.done` closes the stream, the run keeps completing turns and a fresh subscriber resumes from the retained terminal |
| `ui18_heartbeat_and_error_frames_re_state_the_cursor_without_payloads` | heartbeat/error frames carry the server cursor as `id`, declare `kiana.web-sse.v1` and `hydrate=true`, and contain no credential |

GitHub Actions runs the two fixtures, the named `web::tests::ui18_` runtime fixtures and workspace
test-target compilation (`.github/workflows/ui18-web-sse.yml`, `workflow_dispatch`-only,
`permissions: contents: read`); the automatic gate remains the whole-crate `kiana-entrypoints` shard
in `scripts/ci/test-shards.json` (`targets: null`). Results are intentionally not awaited.

## Limitations

- SSE cursors, browser reconnect state, reconnect metrics and terminal retention are process-local
  bounded projections; restart or epoch change requires snapshot hydration.
- The browser reconnect timers, metrics rendering and EventSource behaviour are exercised as source
  fixtures and by reading real HTTP frames, not by a real browser or a proxy.
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

### Completion slice (2026-10-17, CI-only)

```text
source_snapshot: branch step/ui-18-20261017-1 (master cbcf1a3a0156, UI-17 merge point) plus this UI-18 completion slice
worktree_status: only UI-18 files changed; the single frame pump, runtime loopback fixtures, browser metrics, fixture, guard, lane and baseline stay inside the Web display projection
command_argv: CI-only; no local cargo build/test/check/clippy/run; `cargo fmt --all` is the only local cargo command; static checks with rg/sed/git and one-off python3 fixture/source comparisons
cwd·environment: /workspace/kianacode-worktrees/UI-18 (Linux); GitHub Actions ubuntu-latest is the test authority (receipt not awaited)
fixture·cassette: ui18-web-sse.json (kiana.ui18-web-sse.v1) + web::tests::ui18_* real DaemonHost/loopback HTTP fixtures; no provider, network or external effect
exit_code: not run locally; CI receipt not awaited
status_change: UI-18 🔄 → ✅ (CI pending): single next_run_stream_frame pump, real reconnect/cursor/terminal-retention/slow-tab fixtures, observable reconnect metrics, lane and guard wiring
proof-level change: feature_status=implemented; proof_level=source only (no local execution, no CI receipt)
limitations: durable cross-process cursors/subscriptions, real browser/proxy timing, provider/live streams, receipt correctness and physical proof remain open
reviewer: session source self-review; independent GitHub CI reviewer pending
```
