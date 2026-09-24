# UI-17 Web snapshot hydrate, history and pagination baseline

> Snapshot date: 2026-09-25. The bounded Web bootstrap, history cursor and artifact-reference
> queries are wired into GitHub Actions. Local Cargo tests, builds, checks, clippy and smoke
> commands are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-17`](ui-entrypoints.md#step-ui-17) |
| source snapshot | `e9e2d4ad` plus this UI-17 source slice |
| feature_status | `implemented` (bounded bootstrap/hydrate, server cursor pages and deny-first source fixtures) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Browser → Web bootstrap/history router → `DaemonHost` snapshot/EventLog projection → existing feed/ControlPlane paths |

UI-17 makes the first Web read a server-owned hydrate boundary. The browser installs or resumes
the display feed only after accepting the hydrate envelope. The envelope carries schema,
instance, epoch, snapshot cursor, owner/session and tab scope; browser cache is bounded in
memory and is never reconstructed from localStorage.

## Query contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| bootstrap/state | `kiana.web-hydrate.v1` with `snapshot_first` and `feed_after_hydrate` | missing schema, instance, epoch, session or foreign tab scope is rejected; an empty session is still a known session |
| history | `kiana.web-history-page.v1` with bounded `entries`, server-issued `next_page`, source cursor and explicit status | cursor is single-use and bound to instance/epoch/session/tab/limit/source cursor; replay, unknown, stale and cross-tab cursors fail closed |
| artifact | `kiana.web-artifact-page.v1` with server-owned references and metadata only | path/client fetch and artifact bytes are absent; empty refs are `empty`, not a missing session |
| browser cache | tab/session/instance/epoch/schema key, max eight entries, memory only | old epoch/instance evicts the scope and requires hydrate; offline state remains a limitation, not completion |

History status is explicit: `loading`, `empty`, `partial`, `limited`, `ready` and `offline`.
`limited` records the existing EventLog/display bounds. Artifact pages intentionally return only
server references; content loading remains a later artifact viewer step.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| hydrate fixture | schema, instance/epoch/session/tab cache scope, bounded eviction and snapshot-before-feed order |
| page cursor fixture | wrong kind, old source cursor, foreign tab, wrong limit, unknown cursor and single-use replay are rejected |
| session/history fixture | empty known session is not treated as absent; history is owner-filtered and read-only |
| browser source guard | no persisted Web state, cross-tab cache reuse, client artifact fetch or browser execution authority |

The query handlers only call `resolve_human_session`, EventLog-derived projections and `DaemonHost`;
they do not claim UI actions, start a harness, execute a capability or write files. Cursor state is
process-local and disposable. A restart or instance/epoch change requires a new hydrate.

## Evidence block

```text
source_snapshot: e9e2d4ad plus UI-17 source slice
worktree_status: isolated /tmp/kiana-step-ui17; only UI-17 files staged; unrelated worktree changes preserved
command_argv: rustfmt --edition 2021 kiana-entrypoints/src/web.rs; git diff --check
cwd·environment: /tmp/kiana-step-ui17; Linux/bash; no Cargo test/build/check/clippy/smoke; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui17-web-hydrate.json; source-only browser/query fixture; no runtime/provider/external effect
exit_code: target rustfmt and git diff --check are the only local checks; GitHub Actions configured and not awaited
status change: UI-17 ⏳ → 🔄; bootstrap hydrate, bounded history/artifact pages, cursor fencing and browser status/cache source contracts added
proof-level change: feature_status=implemented; proof_level=source only
limitations: no local HTTP/browser behavior, no durable cross-process cursor/cache, no artifact bytes or filesystem freshness proof, no live provider/transport/physical proof; CI result not awaited
reviewer: Codex UI-17 source review; independent GitHub CI reviewer pending
```
