# BQ-23 billing query and protocol baseline

> Snapshot date: 2026-10-19 completion slice. The 2026-09-27 source slice added the versioned
> read-only billing query envelope and the query adapter over the BQ-20 projection. This pass
> attaches that query to the existing `DaemonHost` command/query path and closes the card's deny
> column and its four-surface success column with deny-first fixtures. Local Cargo tests are run
> here (`--offline`, `--test-threads=1`); GitHub Actions receipts are not awaited.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-23`](../roadmap.md#step-bq-23) |
| source snapshot | `e3ad600e` plus this BQ-23 completion slice |
| feature_status | `implemented` for bounded read-only budget summary, usage breakdown, cost export and reconciliation inbox queries over the process EventLog |
| proof_level | `local_behavior` for the seven focused targets below; no durable/live/physical promotion |
| canonical path | human command `budget.query.v1` (CLI/Workbench/Web) or protocol `RequestBody::BillingQuery` -> `DaemonHost` -> `billing_query_service` -> boundary/cursor validation -> BQ-20 projection rebuild -> `query_billing_projection` -> one surface JSON |

`BillingQueryRequest` binds a project identity, server scope digest, source cursor, page limit and
read-only marker. A `BillingQueryCursor` binds the same boundary and projection digest, so a page
from another scope or projection cannot be consumed. `BillingQueryResponse` carries source cursor,
projection digest, freshness, Unknown/quarantine counts and an optional export digest.

`kiana-query::query_billing_projection` is the common adapter for budget summary, usage breakdown,
cost export and reconciliation inbox views. It reads `BillingProjectionSnapshot` only. Export needs
an explicitly allowed query data boundary; revoked/index-denied scopes fail before a response is
materialized. The protocol envelope has no reservation or approval mutation branch.

### Four-surface wiring

`kiana-daemon/src/billing_query_service.rs` is the only place that answers a query. It re-derives
the project identity, data epoch and scope digest from the authenticated context; the wire body and
the command arguments can only select kind, limit and paging. `DaemonHost::handle` intercepts
`budget.query.v1` before `ControlPlane::handle_command` and serves `RequestBody::BillingQuery`
directly, so the terminal-run publish and memory-distillation tails never run for a query.
`request_may_execute` lists `budget.query.v1` as read-only, skipping assignment enforcement, trust
gate, authority sync and session binding exactly like the other read-only commands. No
model-visible tool schema changes: the five tools stay `shell`, `apply_patch`, `mcp`,
`memory.search`, `memory.write`.

| Surface | Route | Shared shape |
|---|---|---|
| CLI | `kiana_commands::budget::BudgetCommand::route` -> `ControlPlane { name: "budget.query.v1", arguments: {"schema": "kiana.budget-card-query.v1", ...} }` | `command_result` text |
| Workbench | `harness_run::command_envelope_on_host` | `command_result` text |
| Web | `harness_run::command_envelope_on_host` (same helper) | `command_result` text |
| Protocol | `RequestEnvelope::billing_query` -> `RequestBody::BillingQuery` | `billing_query` object |

All four carry `kiana.billing-query-surface.v1` = `{schema, billing_query, receipt_view,
cursor_snapshot_digest}`. The entrypoint fixture decodes the command text and compares
`projection_digest`, `response_digest`, `source_cursor`, `freshness`, `unknown_count`,
`quarantine_count`, `project_id`, `data_boundary_digest` and `cursor_snapshot_digest` field by field
with the protocol answer from the same host, so no surface recomputes or reshapes the summary.

### Receipt view: runtime is not business outcome

`kiana-core::BillingQueryReceiptView` separates the runtime status from the business result. The
daemon serves `runtime_only(Completed)`, so `runtime_completed_is_not_business_outcome = true`,
`business_outcome_confirmed = false` and `business_outcome` is null. A view that claims an outcome
without runtime status, evidence or confirmation is refused.

## Failure-first fixture matrix

Deny cases come before the success case in every target.

| Fixture / guard | Assertion | Stable code |
|---|---|---|
| query `projection_without_source_cursor_is_refused_not_rendered_empty` | a projection with no source cursor is refused, never rendered as a zero-row success | `billing_query_projection_source_cursor_missing` |
| query `export_requires_explicit_boundary_and_stale_cursor_is_rejected` | CostExport without an export-allowed boundary; cursor bound to another projection | `billing_query_boundary_denied`, `billing_query_cursor_projection_mismatch` |
| query `pagination_cannot_skip_the_cursor_and_estimate_stays_estimated` | a page cursor past the snapshot progress is refused; estimate 700 / measured 900 stay in separate slots | `billing_query_cursor_out_of_range` |
| query success `query_returns_same_projection_fence_and_freshness_for_read_views` | same source fence, `Freshness::Current`, zero quarantine | - |
| query guard `billing_query_is_read_only_and_does_not_widen_authority` | four kinds, cursor errors, missing-source refusal and no append/reserve/approval path | source markers |
| daemon `empty_log_is_not_an_empty_bill_success` | an empty EventLog is refused, not answered with zero rows | `billing_query_projection_source_cursor_missing` |
| daemon `caller_chosen_boundary_digest_is_refused` | a wire scope digest other than the server-derived one | `billing_query_boundary_mismatch` |
| daemon `wire_actor_other_than_the_authenticated_principal_is_refused` | a project id other than the authenticated one | `billing_query_unauthenticated`, `billing_query_project_mismatch` |
| entrypoints `cli_budget_route_refuses_identity_override_flags` | `--actor` / `--project` never reach the daemon | `budget_identity_override_rejected` |
| entrypoints `daemon_refuses_a_command_identity_override` | identity keys in command arguments are refused by name, not dropped | `billing_query_identity_override_rejected` |
| core `admission_refuses_claim_approval_and_reservation_effects` | claim consumption, approval consumption/staging, reservation/storage and fact writes | `billing_query_consumed_claim`, `_consumed_approval`, `_staged_approval`, `_reserved_budget`, `_released_reservation`, `_wrote_facts` |
| core `receipt_view_refuses_runtime_business_conflation` | business outcome without runtime status, evidence or confirmation | `billing_receipt_view_invalid:billing_receipt_runtime_business_outcome_conflated` |
| core `query_sources_keep_deny_codes_and_read_only_shape` | query sources keep the deny codes and no effect/tool path | source markers |
| daemon guard `billing_query_ingress_is_identity_checked_and_not_a_second_execution_path` | the query arm is a direct await: no model client, subprocess, spawn or append | source markers |
| daemon guard `billing_query_service_is_a_projection_reader` | the service reads the projection and never writes facts | source markers |
| entrypoints guard `surfaces_share_the_command_helper` | Web/Workbench/CLI keep the one command helper; no surface claims the projection is unwired | source markers |
| core guard `fixtures_are_deny_first_and_wired_to_the_manual_lane` | deny fixtures precede the success fixture; lane commands and shard registration are pinned | source markers |
| daemon success `four_kinds_share_one_surface_and_recompute_stably` | four kinds over one seeded history: same projection digest, sha256 digest, non-zero source cursor, unconfirmed receipt, identical `response_digest` on repeat, command text equals protocol body | - |
| entrypoints success `three_entrypoints_and_the_protocol_body_return_one_projection` | CLI route + Workbench + Web + protocol return one projection, freshness, unknown and provenance | - |

## Local evidence (2026-10-19)

Commands: `cargo test -p <crate> --test <target> --offline -- --test-threads=1` with
`CARGO_TARGET_DIR=D:/code/kiana-steps/target-bq23`, `CARGO_BUILD_JOBS=4`,
`RUST_MIN_STACK=33554432`.

| Target | Result |
|---|---|
| `kiana-core --test bq23_billing_query_guard` | 4 passed, exit 0 |
| `kiana-daemon --test bq23_billing_query_guard` | 2 passed, exit 0 |
| `kiana-daemon --test bq23_billing_query_surfaces` | 4 passed, exit 0 |
| `kiana-entrypoints --test bq23_budget_query_surfaces` | 4 passed, exit 0 |
| `kiana-protocol --test bq23_billing_query` | 2 passed, exit 0 |
| `kiana-query --test bq23_billing_api` | 4 passed, exit 0 |
| `kiana-query --test bq23_billing_api_guard` | 1 passed, exit 0 |

`cargo fmt --all --check` cannot run on this Windows host (os error 206, path too long), so every
changed Rust file was checked with `rustfmt --edition 2021 --check`. `cargo test --workspace` was
not run.

## CI and limitations

`.github/workflows/bq23-query.yml` is `workflow_dispatch`-only with `permissions: contents: read`;
it runs the format gate and then the targets above with `--locked -- --test-threads=1`.
`scripts/ci/test-shards.json` registers `bq23_billing_query_guard` in `kiana-core-s1/6`; the
daemon, entrypoints, protocol and query targets are discovered by their whole-crate shards. CI
receipts are not awaited.

Limitations: the projection is rebuilt from the in-process memory EventLog for each query, so it is
not a durable or cross-process read model, and an empty or truncated log is refused rather than
answered. The adapter returns bounded projection summaries; it does not slice detailed rows or
materialize an export file. `request.project_id` is echoed in the response but is not compared with
a snapshot-internal project id, because the snapshot DTO carries none; the daemon compares it with
the authenticated project identity instead. Server-side DataClass/SharingGrant authorization beyond
the derived index/export disposition, persisted query cursors, approval-inbox records, provider
invoice truth and live/physical billing remain outside this slice.
