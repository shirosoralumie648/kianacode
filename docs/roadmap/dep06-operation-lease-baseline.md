# DEP-06 · Operation lease baseline

> Snapshot date: 2026-09-27. This slice defines a deterministic operation lease, heartbeat,
> fence token and authority/data epoch CAS contract. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-06`](../roadmap.md#step-dep-06) |
| source snapshot | `1ddfe530` plus this DEP-06 source slice |
| feature_status | `partial` for the pure lease/CAS contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | OperationJournal → OperationLeaseCas → later durable single-writer adapter → effect-time fence recheck |

`OperationLeaseCas` models a server-owned compare-and-swap projection. It binds an operation to
an instance, storage root, fence token, authority epoch and data epoch; every acquire, heartbeat,
expire and release advances the CAS revision. An active lease cannot be superseded by a second
owner, stale revisions fail closed, expired leases require explicit reclaim, and a fence token
cannot be reused.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| active competition | a second owner is rejected while an active lease exists |
| stale CAS | an old expected revision cannot mutate the lease |
| stale owner/fence | heartbeat and release require the original owner and fence token |
| epoch rollback | authority or data epoch rollback is rejected before mutation |
| expiry | expired lease cannot be silently replaced; explicit expire/reclaim is required |
| release | released lease clears active writer and its fence cannot be reused |
| source boundary | domain/Core contain no filesystem lock, process, Tokio, Broker, EventStore append or dispatch effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep06-operation-lease.yml` with domain fixtures, a Core
source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

This source slice does not hold a kernel lock, persist CAS revisions, coordinate multiple
processes, issue a supervisor command, append EventLog facts or prove restart/durable lease
recovery. Those effects and cross-process guarantees remain later deployment steps; callers must
revalidate the lease and fence at the effect boundary.
