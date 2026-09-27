# DEP-03 · Deployment compatibility baseline

> Snapshot date: 2026-09-27. This slice defines the version axes and a deterministic compatibility
> matrix. Local Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub
> Actions owns fixtures and affected-target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-03`](../roadmap.md#step-dep-03) |
| source snapshot | `b3129bd9` plus this DEP-03 source slice |
| feature_status | `partial` for source-level compatibility evaluation |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | current/candidate server snapshots → version axes → compatibility matrix → later admission/migration gate |

`DeploymentVersionAxes` carries app, protocol, domain, store, projection, workflow, provider,
extension, config, authority epoch, data epoch and generation fields. `DeploymentCompatibilityMatrix`
recomputes stable reasons and status. Major mismatches, store/epoch/generation downgrades and
workflow/provider/extension drift block the candidate; app build/config text remains inspectable
inputs rather than caller authority.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| unknown major | protocol/domain/projection major mismatch is blocked |
| downgrade | store format and authority/data/generation rollback are blocked |
| workflow/provider/extension drift | route or definition drift is explicit and blocked |
| forged status/reasons | recomputation and digest binding reject tampering |
| stable matrix | equal axes produce Compatible and strict JSON round-trip |
| source boundary | no migration, process, Broker or EventLog effect is added |

## CI and limitations

GitHub Actions runs `.github/workflows/dep03-compatibility.yml` with domain fixtures, the Core
source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

This matrix does not inspect a store, migrate data, verify a release artifact, acquire a lease or
decide a provider outcome. Compatibility windows, schema adapters, startup admission and durable
operation facts remain later DEP steps.
