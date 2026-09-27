# INT-29 connector recovery baseline

> Snapshot date: 2026-09-27. This slice adds a paused-by-default connector recovery fact with
> projection rebuild, Unknown reconciliation, stale worker/lease fencing and fresh admission
> gates. Local Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub
> Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-29`](../roadmap/integrations-connectors.md#step-int-29) |
| source snapshot | `28c9d1fb` plus this INT-29 source slice |
| feature_status | `partial` for connector recovery/fencing source contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | committed connector facts → recovery fact/projection fence → explicit fresh admission |

`ConnectorRecoveryFact` binds connector/binding/account, source and projection cursor/generation,
authority/data/lease/worker epochs, pending Unknown/invocation/reconciliation counts and source /
projection digests. Restart remains `Paused` or `NeedsRecovery`; stale worker and lease epochs must
be fenced. `ReadyForAdmission` requires caught-up projection/generation, no pending work, both
fences and an explicit new admission reference. It never auto-resumes Unknown or reuses an old
approval/lease.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| paused recovery | restart facts default to paused and preserve pending Unknown/reconciliation |
| rebuild gate | projection cursor/generation must equal source before readiness |
| stale fencing | old worker/lease epochs without fence are rejected |
| admission gate | ready state requires no pending invocation/Unknown and fresh admission ref |
| shape | digest drift and unknown fields fail closed |
| effect boundary | source/Core layer contains no resume worker, retry, Broker or EventLog write |

## CI and limitations

GitHub Actions runs `.github/workflows/int29-recovery.yml` with domain/Core fixtures, source guard
and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

Limitations: no durable recovery store/projector worker, process restart, lease CAS, external
reconciliation adapter or new admission command is wired here; INT-30+ remains open.
