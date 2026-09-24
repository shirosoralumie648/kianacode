# ER-30 Observability / operator evidence baseline

> This baseline records the ER-30 source slice. GitHub Actions is the runtime test authority;
> local tests and smoke commands are intentionally not run for this step.

## Scope

`kiana-core::project_operator_evidence` folds committed EventLog facts into a bounded,
digest-bound `kiana.operator-evidence.v1` projection. It binds the existing metric and health
snapshots at one source cursor and carries only low-cardinality correlation/causation references.
`DaemonHost::operator_evidence` adds the in-process observability queue depth as an observation.
Run Receipt observability metadata carries a non-authorizing operator-evidence pointer at the same
source cursor; the full health/metric projection is only available through the read-only query. No
method writes EventLog, dispatches a capability, or changes authorization.

The snapshot records append/flush/projector/recovery latency samples, queue depth, last durable
cursor, unknown and orphan counts, stop confirmation evidence, and artifact bytes. Missing samples
are explicit limitations. `effect_success_claim` is fixed to `false`, so metrics cannot claim an
external effect succeeded. An Unknown health status cannot be emitted as healthy.

## Refusal boundaries

- Empty EventLog input, invalid metric/health projection, and an invalid cursor fail closed.
- `validate_secret_free` scans committed telemetry before publication. Secret-bearing event data
  returns `telemetry_secret_scan_blocks_publish`; it is not silently redacted into evidence.
- Unknown effects, orphan dispatch, and unconfirmed stops force a degraded status and retain a
  bounded limitation. Operator evidence does not close or reconcile an invocation.
- Correlation and causation values are metadata only. Trace correlation is represented by bounded
  digests; raw prompts, tool arguments, provider headers and credentials are not exported.
- Queue depth is process-local observation. It is not a durable exporter acknowledgement or a
  provider health result.

## Proof ceiling

`feature_status=implemented`, `proof_level=source`. The ER-30 workflow runs the focused fixture and
workspace test-target compilation in GitHub Actions. This source slice does not prove durable
checkpoint recovery, an external telemetry backend, provider/business health, or physical/live
effects. CI results are intentionally not awaited as part of the implementation step.
