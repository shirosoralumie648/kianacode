# INT-31 connector pilot baseline

> Snapshot date: 2026-09-27. This slice adds a default-off gate for an isolated read-only
> external connector pilot. Local Cargo test/build/check/clippy/smoke commands are intentionally
> not run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-31`](../roadmap/integrations-connectors.md#step-int-31) |
| source snapshot | `6db4e950` plus this INT-31 source slice |
| feature_status | `partial` for default-off pilot admission/evidence contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | isolated account/scope → explicit opt-in/approval → provider receipt + cleanup/revocation evidence gate |

`ConnectorPilotGate` allows only read-only health/list/whoami operations. DefaultOff/Fake remains
blocked. ReadyForOperator requires `OptedIn`, LiveNetwork evidence, isolated account identity,
endpoint/credential/scope digests, approval ref, provider receipt digest, cleanup plan digest,
revocation/data epochs and limitations. The gate does not open a network connection or invoke an
adapter.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| default off | Fake/DefaultOff gate remains blocked |
| opt-in | LiveNetwork + `approval:` ref is required for ready gate |
| evidence | provider receipt, cleanup plan, endpoint/credential/scope and epoch digests are mandatory |
| isolation | account identity must differ from connector identity; operation is read-only allowlist |
| safety | missing/forged evidence, unknown fields and live gate without approval fail closed |
| effect boundary | no network/process/Broker/EventLog path in domain/Core gate |

## CI and limitations

GitHub Actions runs `.github/workflows/int31-pilot.yml` with domain/Core fixtures, source guard and
affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

Limitations: no account provisioning, network listener, provider request, receipt retrieval,
revocation worker or cleanup execution is wired; this is not live/physical evidence; INT-32+ remains
open.
