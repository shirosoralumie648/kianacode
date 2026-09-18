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

This is source/static evidence only. It does not promote cross-process supervisor
durability, power-loss recovery, physical process termination or external effect
reconciliation to durable/live/physical proof.
