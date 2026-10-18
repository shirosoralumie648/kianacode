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
| page cursor fixture | wrong kind, old source cursor, foreign tab, wrong limit, unknown cursor and single-use replay are rejected; a scope mismatch does not consume the owner's valid cursor |
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

## 2026-10-17 completion slice (CI-only)

The completion slice turns the source-only contracts above into a deny-first behaviour boundary. It
keeps the original guarantee — snapshot first, feed after hydrate — and closes the remaining card
exits: multi-session/refresh hydration, bounded pagination, cache eviction and visible states.

Server (`kiana-entrypoints/src/web.rs`):

- The hydrate envelope declares a bounded `kiana.web-hydrate-trace.v1` trace whose steps are
  `snapshot` → `history` → `feed`; a trace step carries schema/requirement names only, so the
  hydrate trace is bounded and credential-free.
- `bootstrap` hydrates a session that exists only as committed history without claiming a lease:
  the response stays `read_only`, `lease.disposition=closed`, `feed_only=true` and no action is
  allowed. An unknown session id still fails closed with `session_unknown`, so a known empty
  session is not a missing session.
- `consume_page_cursor` validates kind/session/tab/instance/epoch/source scope **and the issued
  limit** before removing the token: a foreign tab, a stale source cursor or a malformed follow-up
  cannot burn the owner's cursor, and server cursor pages are single-use (a replay reports
  `web_page_cursor_replayed`). The post-read source check is one shared
  `ensure_page_source_unchanged` fence (`web_page_source_changed`).

Browser (`kiana-entrypoints/src/web_page.html`):

- `recordHydrateTrace` keeps a bounded (`MAX_HYDRATE_TRACE=16`) trace of the canonical order and
  exposes it as `window.__KIANA_HYDRATE_TRACE__`; it records scope ids and cursors only, never the
  Web token, a prompt/draft, an approval payload or artifact bytes.
- A snapshot whose hydrate envelope is not `memory_only` is refused
  (`hydrate_cache_scope_invalid`); an instance/epoch change resets the scope cache
  (`hydrate_scope_reset`); history pages are deduplicated and bounded per session
  (`MAX_HISTORY_CACHE_ENTRIES=256`) and across sessions (`MAX_HISTORY_SESSIONS=8`,
  `evictHistorySessions`).
- loading/empty/partial/limited/ready/offline stay explicit: the hydrate and history status lines
  report the server status, `limited`/`offline` are visible, and an empty reply is never rendered as
  a missing session.

Acceptance fixtures (deny-first first):

| Fixture | Assertion |
|---|---|
| `ui17_bootstrap_hydrates_before_feed_and_matches_the_cli_parity_snapshot` | real loopback router over a seeded ledger: hydrate schema/tab/cache/instance/epoch/cursor, the 3-step trace, a closed historical lease, an empty live session that stays `empty`, `session_unknown` for an unknown id, and re-hydrated status matching the CLI parity projection (`EntryPointKind::Cli`) field by field |
| `ui17_history_pages_follow_one_server_cursor_and_refuse_replay_and_foreign_tabs` | `partial` → `ready` pagination by one server cursor; foreign tab → `web_page_cursor_scope_mismatch`; wrong limit → `web_page_cursor_limit_mismatch` without burning the cursor; replay → `web_page_cursor_replayed`; a cursor issued before a new committed action is refused |
| `ui17_artifact_page_is_run_scoped_and_never_returns_bytes` | server refs only (`content: null`), run/session-scoped enumeration, explicit `empty` for a non-matching ref, multi-session hydrate and per-session pages |
| `ui17_source_change_fence_rejects_a_cursor_that_moved_mid_read` | `ensure_page_source_unchanged` accepts the current identity and refuses a stale epoch/cursor, including one that a real committed turn moved |
| `ui17_fixture_binds_hydrate_cache_and_page_cursor_contract` | declarative fixture ↔ server/browser constants, routes, deny codes, browser markers and forbidden persisted state |
| `ui17_page_hydrates_before_feed_and_keeps_cache_tab_scoped` | page order (snapshot → feed → render), trace order, credential-free trace body, tab-scoped/bounded cache |
| `ui17_runtime_fixtures_are_wired_to_the_web_module` / `ui17_completion_slice_stays_wired_to_the_fixture_lane_and_shard` | runtime fixture names ↔ `web.rs`, required markers ↔ this baseline, lane commands and the whole-crate `kiana-entrypoints` shard |

Required markers (bound by the declarative fixture and `ui17_web_hydrate_guard.rs`):

- snapshot first, feed after hydrate
- server cursor pages are single-use
- a known empty session is not a missing session
- the hydrate trace is bounded and credential-free
- tab-local caches are bounded and memory-only
- visible loading/empty/partial/limited/offline states
- re-hydrated status matches the CLI parity projection
- no second execution path

CI wiring: manual `.github/workflows/ui17-web-hydrate.yml` (`workflow_dispatch`-only,
`permissions: contents: read`) runs `cargo fetch --locked`, the `cargo fmt --all --check` gate, the
`ui17_web_hydrate` / `ui17_web_hydrate_guard` targets, the `web::tests::ui17_` loopback fixtures and
`cargo test -p kiana-entrypoints --no-run --locked`. The automatic gate remains the unified
`ci.yml` `kiana-entrypoints` whole-crate shard (`scripts/ci/test-shards.json` unchanged), which
discovers the new targets without a second dispatch. There is no second execution path: the
hydrate/history/artifact handlers stay read-only projections over the existing
`DaemonHost → ControlPlane → Broker → EventStore` spine.

Status: the UI-17 card exits are covered at the source level and the fixtures are wired to CI, so
the roadmap row moves 🔄 → ✅ (CI pending, receipt not awaited). `proof_level` stays `source`.

Limitations: the runtime fixtures run only on the CI Linux loopback process — no real browser
matrix, no durable cross-process cursor/cache, no artifact bytes or filesystem freshness proof, no
live provider/transport/physical evidence. The browser trace/order guarantees are source-level
bindings, not a browser execution receipt. CI results are not awaited.
