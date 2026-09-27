# DEP-10 · Startup coordinator baseline

> Snapshot date: 2026-09-27. This slice defines the ordered startup evidence reducer used by
> DaemonHost and Core. Local Cargo test/build/check/clippy/smoke commands are intentionally not
> run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-10`](../roadmap.md#step-dep-10) |
| source snapshot | `67f9e760` plus this DEP-10 source slice |
| feature_status | `partial` for ordered startup evidence and ready gating |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | typed adapter evidence → `DaemonHost::evaluate_startup` → Core startup reducer → startup report |

The reducer enforces the fixed order manifest → root → trust → lease → store → migration →
projector → capacity. Every supplied fact carries source digest, authority/data epochs and
generation. A missing, duplicate, out-of-order, stale or non-ready fact blocks startup; Unknown
remains Unknown. An invalid journal cannot be overwritten, and a pending operation cannot resume
under `Fresh`; `ExplicitRecovery` requires a recovery evidence digest. Ready is emitted only after
all eight facts match the request and project trust is true.

The DaemonHost method is an additive route over already prepared typed evidence. It does not
change existing constructors or create another execution loop.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| order/completeness | constructor canonicalizes adapter fact order; direct wire out-of-order facts, missing stage, duplicate stage, stage digest drift and epoch/generation mismatch block before Ready |
| trust/store | untrusted project and invalid journal block trust/store stages; no overwrite is attempted |
| recovery | pending operation under Fresh is blocked; explicit recovery requires a valid evidence digest |
| unknown | Unknown migration/projector fact remains Unknown and cannot be promoted to Ready |
| success | all eight current, Ready facts yield a stable startup report and startup reason `ok` |
| boundary | domain/Core/daemon coordinator contains no filesystem, EventStore, Broker, runner, migration or process effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep10-startup.yml` with domain fixtures, Core/daemon source
guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

This slice consumes adapter-supplied digests rather than validating a live ReleaseBundle,
StoragePreflightReport, OperationLease CAS, EventStore durability, MigrationPreflightReport,
ProjectionStore checkpoint or capacity probe. It does not acquire a lease, open a store, migrate,
rebuild a projector, probe capacity, persist `startupz` evidence or enforce the gate in command
admission; those remain later durable/health/admission steps.
