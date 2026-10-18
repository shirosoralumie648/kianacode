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

Windowing keeps all retained `pending` and `unknown` items in the visible projection even when
older history is paged away. If the projection is full of protected items, a new item is not added
past the hard 512-item bound; the renderer exposes `partial`/limit state and waits for hydrate or
history pagination rather than silently evicting a protected item. Loading, partial/offline and
replay states are explicit status nodes. A replay projection is marked as history and cannot be
promoted to a live run by the renderer. The existing UI-17 hydrate cache, UI-18 SSE gap/reconnect
state and UI-19 tab/session scope remain the source of those boundaries; this slice only renders
their state.

## Failure-first and parity matrix

| Fixture / guard | Assertion |
|---|---|
| typed kind fixture | delta/tool/approval/error/unknown/terminal remain distinct |
| content safety guard | XSS-shaped body is a text node; no `innerHTML` timeline insertion |
| identity guard | server item ID is the render key; array index is not used |
| bounded window guard | pending and unknown survive the recent window; old ordinary items are bounded; an all-protected overflow remains capped and visible as partial |
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

## Completion slice: component directory, migration switch, DOM golden and bundle baseline (2026-10-17)

The card's remaining products are now in the source and wired to CI.  `UI-20` keeps the canonical
path `server ItemView/hydrate/SSE projection → shared Web client contract/store state → DOM-only
timeline renderer`; nothing here authorizes an action, executes a command or owns a second
projection.

| Product | Where | Bound by |
|---|---|---|
| component directory | `kiana-client/src/web_timeline.rs` `WEB_TIMELINE_COMPONENTS` + `TIMELINE_COMPONENT_DIRECTORY` in `web_page.html` (thread `#chat`, turn `section.timeline-turn`, item `article.timeline-item`, status bar `#timelineState`, feed indicator `#streamBanner`) | `ui20_fixture_binds_the_typed_timeline_and_migration_contract`, `ui20_browser_component_directory_and_switch_stay_text_only` |
| migration switch | `kiana.web-timeline-migration.v1` inside every snapshot (`web_timeline_contract`), requested value validated by `validate_timeline_renderer` | `ui20_timeline_renderer_requests_denied_before_lease_or_projection`, `ui20_switch_and_window_bounds_stay_server_owned` |
| DOM golden | `kiana-entrypoints/tests/fixtures/ui20-web-timeline-golden.json` | `ui20_browser_component_directory_and_switch_stay_text_only`, `ui20_completion_slice_stays_wired_to_the_fixture_lane_and_shard` |
| bundle baseline | `kiana.web-timeline-bundle.v1` measured from the served artifact, `WEB_TIMELINE_PAGE_MAX_BYTES`, `WEB_TIMELINE_COMPONENT_MAX_BYTES`, marked region `// >>> UI-20 timeline components` … `// <<< UI-20 timeline components` | `ui20_bundle_and_component_region_fail_closed`, `ui20_bootstrap_publishes_the_server_owned_timeline_migration_contract` |

Deny-first behaviour of the switch: only `typed_inline` is in the reviewed allowlist; a
`legacy_text` request (text-based kind guessing) is refused with
`web_timeline_renderer_text_guess_forbidden` and any other value with
`web_timeline_renderer_unsupported`, both before a session lease is claimed or a projection is read.
A page whose component region is missing or over budget is published as `unavailable` instead of
being painted, and a browser that receives an unreviewed renderer, a diverging window bound, an
incomplete component directory or an over-budget bundle fails closed: the projection stays
`partial`, keeps pending/unknown items visible and refuses ordinary new items until a hydrate
carries a reviewed contract again.  The switch is never read from a URL, hash, local storage or tab
state.

Verification wiring: `kiana-entrypoints/tests/fixtures/ui20-web-timeline.json` is a declarative
`kiana.ui20-web-timeline.v1` matrix (schemas, reviewed renderers, refused requests, components,
window, budgets, deny-first, server/browser required markers, routes, fixtures, required markers,
limitations); `ui20_web_timeline.rs` binds it plus the typed contract to the real source constants
and the golden; `ui20_web_timeline_guard.rs` binds it to the deny paths, the manual lane
`ui20-web-timeline.yml`, the baseline and the `kiana-entrypoints` whole-crate shard.  The runtime
fixtures `web::tests::ui20_bootstrap_publishes_the_server_owned_timeline_migration_contract` and
`web::tests::ui20_timeline_renderer_requests_denied_before_lease_or_projection` drive a real
loopback `DaemonHost` router over the embedded artifact.

```text
driver: cargo test -p kiana-entrypoints --test ui20_web_timeline --locked -- --test-threads=1
driver: cargo test -p kiana-entrypoints --test ui20_web_timeline_guard --locked -- --test-threads=1
driver: cargo test -p kiana-entrypoints --lib web::tests::ui20_ --locked -- --test-threads=1
lane: .github/workflows/ui20-web-timeline.yml (workflow_dispatch only, permissions: contents: read)
automatic shard: ci.yml kiana-entrypoints whole-crate shard (scripts/ci/test-shards.json unchanged)
```

```text
source_snapshot: master 63ab82657671 plus this UI-20 completion slice (branch step/ui-20-20261017-1)
worktree_status: isolated worktree /workspace/kianacode-worktrees/UI-20; typed client contract, page component directory/migration switch, page HTML markers, server contract, fixtures, golden, guard, lane and docs
command_argv: cargo fmt --all; cargo fmt --all -- --check; static rg/cat/sed/git inspection only
cwd·environment: /workspace/kianacode-worktrees/UI-20; Linux/bash; no Cargo build/test/check/clippy/run, no smoke or acceptance script; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui20-web-timeline.json, ui20-web-timeline-golden.json and web::tests::ui20_* over a real loopback DaemonHost router; no external effect
exit_code: local rustfmt only; GitHub Actions owns compile and behaviour verification and its result is not awaited
status_change: UI-20 🔄 → ✅ (CI pending); component directory, server-owned migration switch with deny-first renderer requests, DOM golden and measured bundle baseline added on top of the typed item/window contract
proof_level: source; the lane and the automatic crate shard are wired but no local behaviour, durable, live or physical evidence exists
limitations: no browser screenshot or visual-diff lane, no real long-stream timing, narrow-screen or deep-link browser measurement, no cross-process/durable cache or replay proof, no provider/live or physical evidence; CI receipt not awaited
reviewer: this session's source self-review against the card's deny-first/parity list; no local runtime test reviewer
```

