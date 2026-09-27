# DEP-01 · Deployment profile baseline

> Snapshot date: 2026-09-27. This slice adds pure domain contracts for deployment profiles,
> immutable environment inputs and deployment revisions. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-01`](../roadmap.md#step-dep-01) |
| source snapshot | `7068bb73` plus this DEP-01 source slice |
| feature_status | `partial` for deployment profile/value contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | resolved trust/root evidence → domain profile/revision values → later ControlPlane admission |

`StorageRootId` is reused from the existing domain storage contract. `InstanceId` is a new typed
server-owned UUID. `DeploymentProfile` accepts `embedded-local`, `managed-local`, `container` and
`orchestrated`; the latter remains a target/deferred adapter shape. `EnvironmentProfile` binds only
opaque `SecretRefId` values and a resolved ProjectTrust summary, never secret values or filesystem
reads. `DeploymentRevision` records lifecycle metadata and epochs without starting or stopping a
process.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| empty root | empty storage root is rejected before any adapter effect |
| path escape | lexical `..`/relative roots fail closed |
| cross project | storage/project identity mismatch is rejected |
| untrusted project | a project-bound profile requires a trusted resolved summary |
| unknown profile | serde rejects profile values outside the four known shapes |
| stable round-trip | equal inputs produce the same profile digest and JSON round-trip |
| source boundary | domain contains no filesystem, process, Tokio, Broker or EventLog write path |

## CI and limitations

GitHub Actions runs `.github/workflows/dep01-deployment-profile.yml` with domain fixtures, the
Core source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and
smoke commands are intentionally not run, and CI results are not awaited.

The path checks are lexical only. Actual ProjectTrust resolution, symlink/hardlink checks,
filesystem capability/space checks, root creation, durable leases, lifecycle facts and deployment
effects remain DEP-06/DEP-07/DEP-09+ work; this source slice cannot prove durable or live deployment.
