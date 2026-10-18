# NM-14 CLI/TTY inbox and run-status baseline

> 快照日期：2026-09-25；完成切片 2026-10-17。测试只由 GitHub Actions 执行；本步骤不在本地运行
> 测试、build、check、clippy 或 smoke，且不等待 CI（提交前只运行 `cargo fmt --all`）。

`kiana-entrypoints::notification_cli` is a display-only adapter for the shared NotificationStore
page and existing `UiFeedFrameV1` run feed. It renders bounded rows, urgency/due/read/ACK/snooze
metadata and action id/command labels while dropping payload/detail authority. Run status keeps
Running/Heartbeat/Terminal/Unknown distinct and maps Unknown/gap to `query_original` plus snapshot
reconciliation.

The adapter does not alter frozen `cli.rs`, call Broker/Runner/processes, submit action commands,
approve HumanTask, retry Unknown or treat presenter output as a receipt. `feature_status=implemented`;
`proof_level=source`.

## Completion slice (2026-10-17, CI-only)

The 2026-09-25 slice was a presenter only: nothing parsed an inbox line, nothing bound a page or an
approval challenge to a session, and a dropped TTY connection simply ended the loop. This slice adds
the closed CLI/TTY routing core in the same module and wires it into `workbench_chat.rs`; frozen
`cli.rs` is still not split.

* `parse_cli_inbox_line` is the single parser for `/inbox`, `/read`, `/ack`, `/approve`, `/deny`,
  `/review`, `/reconcile`, `/snooze`, `/escalate`, `/delegate`, `/withdraw` and detach lines. Extra
  tokens, `key=value` pairs, `--flags` and `session:`/`scope:`/`project:`/`role:`/`sandbox:`
  prefixes are refused as forged scope (`cli_inbox_scope_forbidden`) before any binding exists.
* `CliInboxRoute` binds one server page (cursor, page digest, optional server clock) and the
  server-issued approval challenges to the CLI session that fetched them; `CliApprovalBinding`
  takes id/hash/nonce/offered decisions straight from the listing. `plan_cli_inbox_intent` is the
  only planner: a foreign session (`cli_inbox_session_mismatch`), a target outside the projection
  (`cli_inbox_action_not_in_projection`), a decision the server did not offer
  (`cli_inbox_decision_not_offered`) and a read/ACK state borrowed as a decision
  (`cli_inbox_read_is_not_consume`) are all refused before anything reaches the daemon.
* A read is presentation state (`submission_route=notification_read_state`, `decides=false`) and
  never consumes the action; a planned action stays a candidate the ControlPlane must re-admit. A
  disconnect is a detach (`cli_inbox_disconnect_is_not_cancel`, `requery_original`), never an
  implicit cancel or resume.
* `present_session_badges` projects `unread`/`read`/`acknowledged`/`unknown`/`critical`/pending
  approvals from the bound server pages and requires a server clock reading; a route without a page
  fails `cli_inbox_page_digest_required`, and client counters have no input path at all.
* `workbench_chat.rs` routes `/inbox`, `/read`, the ACK-family verbs and approval decisions through
  the same planner; `/approvals` stores the listed bindings; the approval handler re-lists, parses
  `CliApprovalBinding`, plans the decision and cross-checks the challenge `request_hash` against the
  plan before spawning `decide_approval_envelope_on_host`; EOF plans a detach instead of silently
  breaking. The workbench holds no second parse surface and no local decision state.

Fixtures: `kiana-entrypoints/tests/nm14_notification_cli.rs` grows from 3 to 11 tests with the
refusals first — `notification_cli_refuses_forged_scope_before_binding_any_action`,
`notification_cli_refuses_client_hints_and_pages_without_server_binding`,
`notification_cli_refuses_targets_and_decisions_outside_the_server_projection`,
`notification_cli_refuses_another_sessions_line_and_never_crosses_routes`,
`notification_cli_read_stays_presentation_and_never_consumes_the_action`,
`notification_cli_disconnect_is_a_detach_and_never_cancels_or_resumes`,
`notification_cli_badges_refuse_client_counters_and_unbound_pages`,
`malformed_and_oversized_pages_fail_closed` — then the combined success fixture last:
`notification_cli_inbox_approvals_and_badges_share_one_server_bound_route`. The original
`presenter_redacts_payload_and_keeps_actions_display_only` and
`run_status_preserves_unknown_and_terminal_retry_semantics` assertions are unchanged.
`nm14_notification_cli_guard.rs` grows from 1 to 2 guards (boundary markers plus a refusal-code
matrix, fixture names and order, declared fixture, whole-crate shard binding and lane markers).
`.github/workflows/nm14-notification-cli.yml` is `workflow_dispatch`-only with `contents: read`,
`cargo fetch --locked`, the fmt gate, both targets with `--test-threads=1` and two `--no-run`
compile steps. Automatic coverage stays the unified `ci.yml` whole-crate `kiana-entrypoints` shard
(`scripts/ci/test-shards.json` unchanged).

Known limits: no real interactive TTY/CLI E2E; no durable inbox/read state (this entrypoint has no
daemon read-state write route, so a read stays a stated display state); no ControlPlane notification
action submission wire route (the NM-10 `admit_notification_action` facade exists, but this
entrypoint prints planned actions as "candidate only; not submitted"); no SSE/Web/Desktop parity,
PTY/accessibility or live/physical delivery evidence is claimed.
