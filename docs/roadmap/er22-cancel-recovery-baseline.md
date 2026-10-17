# ER-22 cancel recovery and stop confirmation baseline

ER-22 binds cancellation to a durable `RunCancellationFact` and a run-stream
compare-and-swap version. The fact records the cancellation command, expected
generation/version, target invocations, stop request and stop confirmation. Queued
tool calls and pending approvals are closed as `not_executed`/`replay_safe`; started
capabilities must publish stop/effect observations before a terminal `run.cancelled`
fact is accepted.

Runner, model, shell process-group and MCP boundaries all consume the same cancellation
signal. Dispatch and result delivery fence a cancelling/cancelled run, and unconfirmed
stop or mismatched/late results remain `result_unknown` instead of success. Existing
P0-J1, CP-15, SC-15, H08 and H02 fixtures plus the ER-22 source guard run in GitHub
Actions; local runtime tests and smoke commands are intentionally not run.

## Behavioural fixtures (2026-10-17)

The three card exit-condition fixtures live in the shared `kiana-core/tests/control_plane.rs`
target and are deny-first: `cancel_before_dispatch_has_zero_effect` (a cancel that lands while
an invocation is pending approval never reaches the broker, records no dispatch/effect fact and
closes the unexecuted tool as `not_executed` before a single confirmed `run.cancelled`),
`cancel_after_dispatch_without_stop_is_unknown` (a dispatched in-flight effect whose stop can
never be confirmed closes as `result_unknown:cancel_stop_unconfirmed`, not `run.cancelled`) and
`late_result_cannot_resurrect_run` (a completion that arrives after a confirmed cancel is
rejected with `port_conflict:run_terminal_conflict` and the run stays `Cancelled`). Success and
regression stay covered by the existing `control_plane` cancel fixtures and the daemon
`daemon_host` mid-stream / stop-confirmation / approval-cancel fixtures. The
`er22_cancel_recovery_guard` target keeps its source guards and adds
`cancel_behavioural_fixtures_are_wired` to pin the fixture names and the workflow wiring.

This is source/static evidence only. It does not promote cross-process supervisor
durability, power-loss recovery, physical process termination or external effect
reconciliation to durable/live/physical proof.
