# ER-19 process/job handle and fencing baseline

## Scope

ER-19 binds long-running process continuation and stop to a server-owned JobHandle, owner/session/
run/turn/project/authority identity, process-group evidence and expiry.  Resource leases carry
fence tokens and successors; supervisors record stop/capture/leader-reap evidence, and an unknown
or escaped child remains fenced instead of being reported successful.

## Evidence gate

- `er_stale_handle_cannot_stop_new_execution` checks immutable JobHandle identity, owner/scope/
  authority/expiry, path/resource fencing and cancellation port boundaries.
- `er_child_escape_or_leader_exit_is_unknown` checks process-group/leader-reap/stop-confirmed
  evidence, output drain limits and Unknown/fenced Receipt dimensions.
- `er_lease_expiry_does_not_release_live_process` checks expiry/authority revocation, query lease
  semantics, durable path ownership and lifecycle stop/Unknown handling.

The GitHub workflow runs existing H15 output/process and CP-12 resource fixtures, then this guard
and workspace test-target compilation.  Local runtime tests are not run.

## Limits

Source plus CI-fixture evidence only.  It does not claim container supervisor, Windows job-object,
power-loss or multi-host process fencing proof; those remain platform/DEP/live work.
