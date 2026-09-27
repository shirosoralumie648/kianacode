# INT-26 connector data propagation baseline

> Snapshot date: 2026-09-27. This slice adds a source-level propagation fact binding connector
> output to DataClass/Purpose/SharingGrant/retention and revocation epochs. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns the
> fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-26`](../roadmap/integrations-connectors.md#step-int-26) |
| source snapshot | `465657e6` plus this INT-26 source slice |
| feature_status | `partial` for propagation/revocation source contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | connector result → scoped propagation fact → tombstone/data_epoch fence → derived-view invalidation plan |

`ConnectorPropagationFact` carries connector/binding/account identity, source/target project,
object reference, data class, purpose, scope digest, optional cross-project SharingGrant digest,
retention policy digest, source cursor and data epoch. Active facts cannot carry tombstones or
invalidated views. Revoked/expired/quarantined facts require a tombstone epoch and invalidate
Memory, Index and Cache together. Cross-project propagation requires an explicit grant digest.

The Core facade validates facts only. No memory/index/cache write, EventLog append, provider call or
automatic resurrection occurs in this slice.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| active scope | data class/purpose/retention/scope/source cursor and epoch are required |
| sharing fence | cross-project result without SharingGrant digest is rejected |
| revocation | revoked/expired/quarantined result needs tombstone epoch and all derived views invalidated |
| active fence | active result cannot claim tombstone or invalidation |
| shape | digest drift and unknown fields fail closed |
| read-only guard | no derived-view write, Broker, EventLog or external effect path |

## CI and limitations

GitHub Actions runs `.github/workflows/int26-propagation.yml` with domain/Core fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: this is a validation fact, not a propagation worker. It does not persist tombstones,
invalidate Memory/Index/Cache, evaluate live retention/holds, resolve SharingGrant authority or
prove revocation across processes; INT-27+ remains open.
