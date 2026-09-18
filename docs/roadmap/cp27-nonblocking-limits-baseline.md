# CP-27 non-blocking storage / clock / capacity baseline

## Scope

CP-27 binds blocking filesystem work to bounded workers, keeps cancellation and deadlines on
async/monotonic paths, rejects clock rollback as untrusted, and makes pending/event/payload/
snapshot/output/log/queue limits explicit structured denials.  Existing reservations and committed
facts remain intact when a limit or worker queue is exhausted.

## Evidence gate

- `cp_slow_storage_does_not_starve_cancellation` checks JSONL/Workspace/Memory/Context/MCP/process
  blocking isolation, bounded worker queues, cancellation/timeout propagation and stream limits.
- `cp_clock_rollback_cannot_extend_authority` checks ClockObservation/ClockPort rollback fencing,
  authority fences, approval/company/cell expiry and monotonic authority versions.
- `cp_pending_and_event_limits_fail_without_resource_leak` checks EventLog/Journal/event payload,
  approval/patch/tool/process/telemetry/model/Runner/health hard caps and structured failure paths.

The GitHub workflow runs existing clock, storage, health, budget, output-limit and resource fixtures,
the eventlog package tests, then the CP-27 source guard and workspace test-target compilation.
Local runtime tests are not run.

## Limits

This is source plus CI-fixture evidence only.  It does not claim capacity numbers are production
benchmarks, physical disk guarantees, cross-host scheduler fairness or live provider latency.
Later PD/ER/DEP gates own representative load, migration and crash matrices.
