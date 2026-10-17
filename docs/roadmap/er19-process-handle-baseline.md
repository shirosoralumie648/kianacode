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

## Behaviour fixtures (2026-10-17, CI-only)

The source guard above is now backed by runnable behaviour fixtures that exercise the real
handle/lease/stop functions instead of only asserting markers exist:

- `kiana-domain/tests/er19_job_handle.rs` — `er19_stale_handle_cannot_stop_new_execution` proves
  the handle digest binds the immutable job/start/process-group/expiry identity (re-pointing
  `job_id` yields `job_handle_digest_mismatch`; a missing process group or zero expiry yields
  `job_handle_header_invalid`), and a `ResourceLease` successor must advance the fence token
  (`resource_lease_fence_token_reused`).  `er19_lease_expiry_does_not_release_live_process` proves
  lease expiry denies (`resource_lease_expired`) rather than reporting release, and a stale fence
  token is rejected (`resource_lease_fence_mismatch`).
- `kiana-daemon/src/execution_control.rs` (`--lib er19`) —
  `er19_stale_handle_cannot_stop_new_execution` calls the real `validate_job_handle` and denies a
  stale handle aimed at a new execution (`job_handle_identity_mismatch`), a foreign owner
  (`job_handle_owner_mismatch`) and a tampered handle (`job_handle_invalid`).
  `er19_lease_expiry_does_not_release_live_process` denies an expired handle
  (`job_handle_expired`) and an expired lease.
- `kiana-daemon/src/process_supervisor.rs` (`--lib er19`) — `er19_stop_confirms_live_group_leader`
  spawns a real child in its own process group and confirms the stop only after leader reap and an
  empty group observation; `er19_stop_without_group_identity_is_unknown` shows that without an
  observable process-group identity a leader exit/reap is **not** reported as confirmed
  (`confirmed=false`, `process_group=Unknown`).

## Limits

Source plus CI-fixture evidence only.  It does not claim container supervisor, Windows job-object,
power-loss, multi-host process fencing, escaped-child detection or daemon-crash restart proof; those
remain platform/DEP/live work.  The OS stop fixture observes the leader and its process group only,
not every descendant.
