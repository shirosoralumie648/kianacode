# DEP-01 · Deployment profile baseline

> Snapshot date: 2026-09-27. This slice adds pure domain contracts for deployment profiles,
> immutable environment inputs and deployment revisions. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-01`](../roadmap.md#step-dep-01) |
| source snapshot | historical implementation anchor `7068bb73`; focused closure on current dirty worktree (2026-10-05) |
| feature_status | `implemented` for deployment profile/environment/revision value contracts |
| proof_level | `local_behavior` for deny-first focused fixtures; no durable/live/physical promotion |
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

GitHub Actions is wired through `.github/workflows/dep01-deployment-profile.yml` with domain
fixtures, the Core source guard and affected-target compilation. The current local focused receipt
below supersedes the historical no-local-test note; remote workflow results were not awaited.

The path checks are lexical only. Actual ProjectTrust resolution, symlink/hardlink checks,
filesystem capability/space checks, root creation, durable leases, lifecycle facts and deployment
effects remain DEP-06/DEP-07/DEP-09+ work; this source slice cannot prove durable or live deployment.

## Local focused closure receipt (2026-10-05)

The current worktree ran the deny-first domain and Core contract fixtures with serial Cargo/test
threads and a 32 MiB test-thread stack:

```text
source_snapshot: current dirty worktree after DEP-00 inventory closure
worktree_status: DEP-01 row/card promoted to ✅; no automatic commit/push/merge/rebase
command_argv: CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-domain --test dep01_deployment -p kiana-core --test dep01_deployment_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; serial Cargo/test threads; controlled test stack and stripped test profile
fixture·cassette: `kiana-domain/tests/dep01_deployment.rs` 3/3; `kiana-core/tests/dep01_deployment_guard.rs` 1/1; empty root, path escape, cross-project, untrusted project, unknown profile, stable digest/round-trip and pure-domain boundary
exit_code: 0
status_change: DEP-01 promoted from 🔄 to ✅ in `docs/roadmap.md`
proof-level_change: `source` → `local_behavior` for the profile/environment/revision contract
limitations: lexical path validation only; no filesystem trust resolution, symlink/hardlink or capacity preflight, root creation, durable lease, lifecycle effect, cross-process fencing, live deployment or physical evidence; CI workflow was not awaited
reviewer: integration agent; focused local contract reviewer
```
