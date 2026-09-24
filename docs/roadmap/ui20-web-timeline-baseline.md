# UI-20 Web timeline component migration baseline

> Snapshot date: 2026-09-25. UI-20 migrates the inline Web thread projection to a bounded typed
> renderer. GitHub Actions owns contract and compile verification; local Cargo tests, builds,
> checks, clippy and smoke commands are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-20`](ui-entrypoints.md#step-ui-20) |
| source snapshot | `c162ecaa` (UI-19) plus this UI-20 source slice |
| feature_status | `implemented` (typed item contract, Web renderer, bounded window and source fixtures) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | server `ItemView`/hydrate/SSE projection → shared Web client contract/store state → DOM-only timeline renderer |

The renderer consumes server-owned item kinds and stable item IDs. It does not infer a kind from
the title or body, execute a command, or treat a DOM state as a receipt. Legacy `userMessage`,
`agentMessage`, `commandExecution` and `fileChange` names are explicit compatibility aliases only;
unknown values remain visible as `unknown` and require a refresh/hydrate path.

## Typed item and state contract

| Server kind | DOM role | Contract |
|---|---|---|
| `delta` | assistant/output text | plain `textContent`, bounded body |
| `tool` | tool projection | display-only, no executable payload |
| `approval` | pending action marker | protected from recent-window eviction; decision remains a server action |
| `error` | structured failure | remains visible and is not replaced by a later delta |
| `unknown` | schema/stream uncertainty | explicit refresh-required marker; never guessed from text |
| `terminal` | terminal status | visible terminal fence; receipt remains authoritative |

The server now supplies stable `ItemView.id` values. Browser render keys use that ID (with a
deterministic compatibility fallback) and never use the array position. Untrusted content is
inserted with `textContent`; the timeline does not use `innerHTML` or markdown/HTML execution.

## Bounds, windowing and recovery visibility

```text
TIMELINE_WINDOW_SIZE = 64 recent items
TIMELINE_MAX_ITEMS   = 512 retained projection items
TIMELINE_MAX_BODY_BYTES = 16 KiB per item
```

Windowing keeps all `pending` and `unknown` items in the visible projection even when older
history is paged away. Loading, partial/offline and replay states are explicit status nodes. A
replay projection is marked as history and cannot be promoted to a live run by the renderer. The
existing UI-17 hydrate cache, UI-18 SSE gap/reconnect state and UI-19 tab/session scope remain the
source of those boundaries; this slice only renders their state.

## Failure-first and parity matrix

| Fixture / guard | Assertion |
|---|---|
| typed kind fixture | delta/tool/approval/error/unknown/terminal remain distinct |
| content safety guard | XSS-shaped body is a text node; no `innerHTML` timeline insertion |
| identity guard | server item ID is the render key; array index is not used |
| bounded window guard | pending and unknown survive the recent window; old ordinary items are bounded |
| state/replay guard | loading, partial, replay and offline remain visible; replay is not live authority |
| execution boundary guard | renderer only consumes shared state and does not create an execution loop or resubmit reconnect commands |

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the UI-20 fixture and
deny-first guard, and `cargo check --workspace --tests --locked`. Local tests, builds, checks,
clippy and smoke commands are not run; CI results are intentionally not awaited.

## Evidence block

```text
source_snapshot: c162ecaa (UI-19) plus UI-20 source slice
worktree_status: isolated /tmp/kiana-step-ui20; typed client, Web timeline renderer, stable server item IDs, fixtures, guard, workflow and docs
command_argv: rustfmt --edition 2021 kiana-client/src/web_timeline.rs kiana-entrypoints/src/web.rs kiana-entrypoints/src/web_thread.rs kiana-entrypoints/tests/ui20_web_timeline.rs kiana-entrypoints/tests/ui20_web_timeline_guard.rs; git diff --check
cwd·environment: /tmp/kiana-step-ui20; Linux/bash; no Cargo test/build/check/clippy/smoke; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui20-web-timeline.json; typed server kinds, XSS text boundary, stable IDs, protected pending/unknown window, loading/partial/replay/offline; no external effect
exit_code: local target rustfmt and git diff --check only; GitHub Actions configured and not awaited
status_change: UI-20 ⏳ → 🔄; typed Web timeline item contract, DOM-safe rendering, bounded protected window and deny-first source guards added
proof-level change: feature_status=implemented for bounded Web source contracts; proof_level=source only
limitations: no browser screenshot/golden, real HTTP/SSE timing, visual parity, cross-process/durable cache, provider/live timing, receipt correctness, external effect or physical proof; CI result not awaited
reviewer: Codex UI-20 source review; checks explicit server kind mapping, text-only rendering, stable IDs, protected pending/unknown window, visible loading/partial/replay/offline state, shared hydrate/SSE/session boundaries and no second execution loop; no local runtime test reviewer
```
