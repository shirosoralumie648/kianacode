# NM-15 Web notification snapshot/page and SSE baseline

> Snapshot date: 2026-10-19 (worktree `D:/code/kiana-steps/nm15`, branch `step/nm15-20261018-1`).
> This completion slice ran the focused local targets listed below and did not query remote CI;
> no workspace-wide test, clippy or smoke run and no GitHub Actions receipt is claimed.

## Scope

`/api/notifications` is a loopback Web page adapter over the authenticated daemon principal's
committed `NotificationStore` projection. `/api/notifications/events` is an SSE adapter over the
existing `RunStreamFeedSubscription`; it never creates a second bus and never submits an action.

Implemented in this slice (`kiana-entrypoints/src/web.rs`):

- **Durable-source binding.** Both `last_durable_cursor()` reads go through
  `notification_source_cursor_error`, so a store without a durable cursor (MemoryEventLog health
  `event_store_health_unsupported`, or `cursor_snapshot_unsupported`) fails closed with the stable
  `notification_projection_unavailable` (503) instead of leaking a transport detail or returning an
  empty page.
- **Server-owned page cursor.** `issue_page_cursor` mints a single-use token bound to schema, kind,
  session, tab, instance, authority epoch, durable source cursor and page limit.
  `consume_page_cursor` burns the token **before** the projection read and returns literal
  `web_page_cursor_replayed` / `web_page_cursor_unknown`; scope drift (`web_page_cursor_scope_mismatch`,
  409), limit drift (`web_page_cursor_limit_mismatch`, 400), stale source (`web_page_source_changed`,
  409) and malformed tokens (`web_page_cursor_invalid`) are refused first. A refused follow-up from a
  foreign tab or with a wrong limit does not consume the owner's token.
- **Bounds and redaction.** The page is bounded to 30 items and 16 action labels per item; detail,
  source payload and action arguments are stripped before JSON is returned and before frames are
  written. `notification.approve` leaves the Web surface at HTTP 200 with
  `status: "blocked"`, `error: "command_unregistered"`, and no `run.authorized`, run, capability or
  approval fact is appended.

## Fixtures (deny-first)

`kiana-entrypoints/tests/fixtures/nm15-web-notification.json` (schema
`kiana.web-notification-page.v1`) carries 17 refusal entries **before** the positive contract:
`wrong_host`, `wrong_origin`, `wrong_bearer_or_web_token`, `cross_session_tab`, `missing_tab`,
`unknown_session`, `foreign_or_stale_cursor`, `cursor_conflict`, `malformed_or_forged_cursor`,
`unbounded_page`, `page_cursor_replay_or_scope_shift`, `stale_source_cursor`,
`payload_or_action_argument_leak`, `gap_as_completion`, `sse_reconnect_submits_command`,
`notification_command_execution`, `terminal_then_delta`. Every refusal code in the fixture is
asserted to exist in `WEB_ERROR_MATRIX`, and `kiana-entrypoints/tests/fixtures/ui16-web-routes.json`
was extended (70 codes) with `web_page_cursor_replayed` and `web_page_cursor_unknown` (400, request).

The live loopback fixtures in `kiana-entrypoints/src/web.rs` drive the real Axum router over HTTP
with a real `DaemonHost::local()` (durable `JsonlEventLog`, `write_ledger`-seeded committed
notification events) and cover:

1. `nm15_notification_page_is_authenticated_bounded_and_redacted` — web-token/tab fences, 30-item
   bound, redaction, no action arguments in the body.
2. `nm15_notification_page_token_is_bound_to_the_committed_source_cursor` — a real harness turn
   advances the durable cursor and invalidates the open token with
   `web_page_cursor_scope_mismatch`; wrong tab and wrong limit are refused without burning it.
3. `nm15_notification_page_refuses_a_projection_that_is_not_durable` — MemoryEventLog host yields
   503 `notification_projection_unavailable`, not an empty page.
4. `nm15_notification_sse_reconnects_without_duplicating_and_retains_the_terminal` — boundary-first
   handshake on every reconnect, no duplicate frame after the cursor, terminal replayed once to a
   late subscriber, boundary-only after terminal, `stream_gap` + `snapshot_required` +
   `retry: query_original` for foreign instance/epoch, `ui_feed_cursor_decode_failed` for malformed
   and `ui_feed_cursor_digest_invalid` for forged digests.
5. `nm15_web_notification_surface_cannot_execute_an_action` — the blocked command path with bounded
   event growth.

Static contract tests (`tests/nm15_web_notification.rs`, 5) and the source guard
(`tests/nm15_web_notification_guard.rs`, 2) assert the cursor is consumed before the read, issued
after it, that `next_after_item_id` is only emitted on a full page, that `next_page_token` is cleared
on refusal, that no `notification.approve` / `run_envelope_on_host` call exists on these routes, and
that the browser backoff constants in `src/web_page.html` match the fixture.

## Evidence (this worktree, 2026-10-19)

All commands from `D:/code/kiana-steps/nm15`, `CARGO_TARGET_DIR=D:/code/kiana-steps/target-nm15`,
`CARGO_BUILD_JOBS=4`, `RUST_MIN_STACK=33554432`, `--offline`, `--test-threads=1`:

| Command | Exit | Result |
|---|---|---|
| `cargo test -p kiana-entrypoints --test nm15_web_notification` | 0 | 5 passed |
| `cargo test -p kiana-entrypoints --test nm15_web_notification_guard` | 0 | 2 passed |
| `cargo test -p kiana-entrypoints --lib nm15_notification` | 0 | 4 passed |
| `cargo test -p kiana-entrypoints --lib nm15_web_notification_surface_cannot_execute_an_action` | 0 | 1 passed |
| `cargo test -p kiana-entrypoints --test ui16_web_routes` | 0 | 6 passed |
| `cargo test -p kiana-entrypoints --test ui16_web_routes_guard` | 0 | 3 passed |
| `cargo check -p kiana-entrypoints --tests --offline` | 0 | 0 errors |
| `rustfmt --edition 2021 --check <touched files>` | 0 | clean |

`cargo fmt --all --check` is not used on this Windows host (os error 206); formatting is verified
per file with `rustfmt --edition 2021 --check`. The `ui16_web_routes` `registered_routes` scanner ran
forever on stashed HEAD (never advanced `rest`); this slice fixed it with
`rest = &after[line_end..];` and added `stream_attach_state(` to the sensitive-read marker list.

New manual lane: `.github/workflows/nm15-web-notification.yml` (`workflow_dispatch` only,
`permissions: contents: read`) runs `cargo fmt --all --check` on Linux plus the three test targets and
`--no-run` compile steps.

## Limitations

- `feature_status=implemented`; `proof_level=local_behavior` (was `source`). Proof covers the focused
  `kiana-entrypoints` targets above only — not the workspace, clippy, smoke scripts, CI, CLI/TTY,
  desktop or any live provider.
- Notification read/ACK projection state is rebuildable but not claimed durable in this slice.
- SSE cursor and subscription state are process-local; no cross-process or browser live-delivery
  proof is claimed.
- Runtime deltas are only projected from committed events; no external connector or physical
  delivery receipt evidence.
- The earlier card text "1–30 秒退避" was stale: the browser implements 250 ms → 8 s exponential
  backoff with 6 attempts (`SSE_RECONNECT_BASE_MS`, `SSE_RECONNECT_MAX_MS`, `SSE_MAX_RECONNECTS`
  in `src/web_page.html`), which the fixture and test now assert. The server heartbeat is 15 s.
- The roadmap row stays `🔄`: see `docs/roadmap.md` §3 for the exact remaining gap.
