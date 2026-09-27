# INT-25 connector mapping baseline

> Snapshot date: 2026-09-27. This slice adds versioned object mapping, input ArtifactRef,
> provenance digest and account-scoped pagination cursor contracts. Local Cargo test/build/check/
> clippy/smoke commands are intentionally not run; GitHub Actions owns the fixtures and target
> compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-25`](../roadmap/integrations-connectors.md#step-int-25) |
| source snapshot | `b262c50c` plus this INT-25 source slice |
| feature_status | `partial` for data-only mapping, artifact and cursor contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | connector operation schemas → versioned object mapping → scoped input artifact/output page → bound page cursor |

`ConnectorObjectMapping` is server-owned metadata with input/output schema digests and unique
external→data-field targets. Capability/actor/role/approval/scope/path/command/endpoint targets are
rejected so external fields cannot become authority arguments. `ConnectorInputArtifact` reuses the
immutable ArtifactRef and binds its mapping digest/source cursor. `ConnectorMappedObject` and page
output retain value/provenance digests instead of granting execution authority.

`ConnectorPageCursor` contains connector/binding/account/operation, scope/mapping digest, source
cursor, opaque page-token digest and authority epoch. It has no raw token and must match the
mapping identity. Page ordinals are strictly increasing, keeping output order deterministic.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| mapping authority | duplicate/forbidden target fields and schema/digest drift fail closed |
| input artifact | missing/foreign ArtifactRef scope or mapping/source digest is rejected |
| cursor scope | foreign account/binding/operation/mapping/epoch cursor cannot be reused |
| output order | object value/provenance digest drift and non-monotonic ordinals fail |
| shape | unknown fields and oversized mapped values are denied |
| read-only guard | Core/domain mapping layer contains no Broker, provider, EventLog or approval effect |

## CI and limitations

GitHub Actions runs `.github/workflows/int25-mapping.yml` with domain/Core fixtures, source guard
and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

Limitations: no connector adapter invocation, durable ArtifactStore write, schema registry lookup,
provider pagination network call, ControlPlane command mapping or live/physical evidence is added;
INT-26+ remains open.
