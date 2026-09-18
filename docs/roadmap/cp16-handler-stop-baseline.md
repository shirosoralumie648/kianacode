# CP-16 Handler Stop / File Commit Baseline

## Scope

CP-16 closes the actual handler boundary after CP-15 cancellation intent. Shell handlers create a
process group, terminate descendants with bounded escalation, wait/reap and drain pipes; failure to
confirm stop is `Unknown`, never a successful cancel. Patch handlers validate path/file preconditions
before commit, use descriptor-relative no-follow writes/rename and rollback/result-unknown evidence;
multi-file output is a bounded effect list, not a claim of global filesystem atomicity. MCP stdio
handlers own the child process, classify disconnect/stop uncertainty, retain reconciliation
workspace state and expose stderr only as bounded metadata/digest. Broker/ports keep cancellation
and permit checks outside individual adapters.

## Evidence and limits

- `kiana-daemon/tests/p0_j1_03_process_group.rs` and `p0_j1_04_cancel_race.rs` pin process-group
  stop, descendant handling, no late delta/completion and unknown stop outcomes.
- `kiana-core/tests/cp16_handler_stop_guard.rs` pins shell/patch/MCP/Broker/port/sandbox stop and
  file-commit markers and rejects known false-success bypass strings.
- GitHub Actions runs the fixtures, source guard and workspace compile; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: OS-specific kill guarantees,
physical filesystem durability, remote effect revocation and external/live/physical proof remain
CP-17+ / ER / CAP work.
