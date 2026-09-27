# DEP-08 · Deployment config baseline

> Snapshot date: 2026-09-27. This slice defines immutable deployment configuration source
> precedence, project trust/environment allowlists, opaque SecretRef usage, revision digests and
> restart/migration impact. Local Cargo test/build/check/clippy/smoke commands are intentionally
> not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-08`](../roadmap.md#step-dep-08) |
| source snapshot | `ab572527` plus this DEP-08 source slice |
| feature_status | `partial` for the immutable deployment config contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | trusted project snapshot + bounded source metadata → immutable deployment snapshot → later ConfigSnapshotStore/startup coordinator |

`DeploymentConfigSourceKind` encodes the fixed precedence compiled default → user file →
`KIANA_HOME` file → project file → explicit environment → command line. Project source entries
must bind the server-owned trust digest; environment names must be in the explicit allowlist;
command-line values are operation-scoped metadata. Effective configuration contains only
non-secret JSON and opaque `SecretRef`s. The snapshot derives a stable `config_revision`, a
redacted digest, snapshot digest and impact flags for restart/migration/lease reacquisition.
`ConfigDiff` binds changed top-level paths and per-change digests to the two immutable revisions.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| precedence/order | unordered input is canonicalized; duplicate source kinds and precedence tamper fail |
| project trust | untrusted project source is rejected before snapshot publication |
| environment | an unallowlisted environment name is rejected |
| secret/redaction | raw password/token values are rejected while opaque SecretRef and `_ref` values remain valid |
| revision | config revision and snapshot digest tampering fail closed |
| impact/diff | immutable restart/migration/lease flags and changed-path digests are bound into the revision pair |
| source boundary | domain/Core contain no filesystem, environment, process, provider, Broker or EventLog effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep08-deployment-config.yml` with domain fixtures, a Core
source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

This slice does not read actual files/environment variables, merge live values, resolve a secret,
publish through `ConfigSnapshotStore`, or reload an active run. Existing provider/daemon config
resolvers remain the later adapter authorities; startup ordering and durable CAS publication are
DEP-10+ work.
