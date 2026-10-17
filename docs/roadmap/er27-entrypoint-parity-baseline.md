# ER-27 four-entry receipt and recovery command baseline

ER-27 keeps CLI, Workbench, Web and Desktop as views/adapters over one
`DaemonHost → ControlPlane → EventLog` path. Shared protocol/client DTOs carry receipt,
pending approval, resume, cancel, reconciliation and restore commands; entrypoints
construct envelopes and render safe projections only. DaemonHost authenticates the
principal, routes the envelope and returns the same status/error/receipt semantics.

Cursor gaps, reconnects and UI action cards remain read-only until the explicit command
is sent. No entrypoint parses or mutates EventLog, supplies an authority owner/scope,
starts execution from a receipt query, or auto-approves an action. GitHub Actions runs
CP-22 protocol-surface, P2-M3 action-card, P2-K3 Human Inbox, P2-M7 accessibility and
ER-27 source guards plus workspace target compilation. Local runtime tests and smoke
commands are intentionally not run.

## Behavioural fixtures (2026-10-17, CI-only)

The card names are now backed by runnable behaviour instead of source markers alone.
`kiana-daemon/tests/er27_entrypoint_receipt_recovery.rs` drives the real
`DaemonHost → ControlPlane → EventLog` path with a zero-call Runner and a
`MemoryEventLog`; CLI, Workbench, Web and Desktop differ only by the
`EntryPointKind` correlation label:

- `er27_ui_supplied_owner_or_scope_is_denied` — a forged actor is refused with
  `AUTH_CALLER_UNTRUSTED`, a forged department with `role_department_mismatch` and a
  forged role with `role_unknown`, all with zero runner send/restore and no new fact.
- `er27_receipt_query_never_starts_execution_for_unknown_run` — an unknown run returns
  `receipt_not_found` instead of a synthesized empty receipt.
- `er27_same_denial_error_code_across_four_entrypoints` — a malformed schema returns the
  same `protocol_schema_unsupported` on every surface.
- `er27_receipt_query_is_read_only_and_agrees_across_four_entrypoints` — identical
  `run_receipt.receipt_digest`, zero runner calls and an unchanged fact list on all four
  surfaces.
- `er27_recovery_commands_agree_and_stay_read_only` — terminal `resume` returns
  `ResultUnknown` with `run_resume_terminal_completed`, `cancel` returns `Completed` with
  `already_terminal: true`, `pending` returns `Completed` with empty approvals, and none of
  `execution.prepared`, `invocation.dispatching`, `run.resume_prepared` or `run.cancelled`
  is appended.

`er27_entrypoint_parity_guard` keeps its source guards and adds
`behavioural_fixtures_are_wired_into_the_er27_lane` to pin the fixture names and the
`.github/workflows/er27-entrypoint-parity.yml` wiring (run + `--no-run`). `kiana-daemon` is
a whole-crate shard, so the new `tests/` target needs no shard change.

This is source/static evidence only; desktop process supervision, external transport
and live/physical delivery remain outside this slice. Local runtime tests were not run in
this CI-only session.
