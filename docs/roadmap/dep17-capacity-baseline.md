# DEP-17 - Capacity and shutdown limits baseline

> Snapshot date: 2026-09-27. This slice defines bounded append, artifact, operation, log,
> diagnostic, migration and shutdown capacity facts. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-17`](../roadmap.md#step-dep-17) |
| source snapshot | `903cae08` plus this DEP-17 capacity source slice |
| feature_status | `partial` for source-level capacity/backpressure contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | server-owned capacity facts -> bounded Core reducer -> DaemonHost read-only route |

`CapacityFact` covers seven deployment resources with hard limits, requested/used bytes or
entries, queue depth, boundedness, committed-fact preservation, source/evidence cursor binding and
shutdown deadline observations. The report maps available, backpressure, rejected and unknown
facts to stable denial codes. `capacity_exceeded` blocks new work without deleting committed facts;
Unknown capacity or missing shutdown evidence remains non-admitting.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| ready | all seven resources within limits produce stable JSON/human output and admit new work |
| backpressure | a non-empty bounded queue denies new work while existing work remains observable |
| hard limit | over-limit and checked-add overflow produce `capacity_exceeded`; no fact deletion |
| safety | unbounded channel, facts-not-preserved and shutdown deadline at/over expiry reject |
| unknown | result-unknown or missing shutdown deadline denies both new and existing admission |
| binding | cursor/epoch/generation/source/evidence/resource-order/digest tamper fails closed |
| boundary | Core/Daemon only reduce/delegate; no filesystem, EventLog, queue allocation, Broker or provider effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep17-capacity.yml` with domain fixtures, Core and Daemon
source guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

This is a source-only adapter-fact contract. It does not probe physical disk bytes/inodes,
allocate channels, reserve artifacts, enforce EventLog append/fsync limits, persist capacity
snapshots or run shutdown timers. Existing journal/artifact/observability/migration constants and
ports remain the later enforcement authorities. Backup bytes and restore/backup capacity remain
with DEP-19/20; this slice does not claim backup admission. `allow_existing_work` is an admission
projection, not permission to interrupt a committed append or flush; uncertain writes remain
Unknown for later reconciliation.
