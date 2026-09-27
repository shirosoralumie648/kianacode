# INT-30 connector conformance baseline

> Snapshot date: 2026-09-27. This slice adds a digest-bound conformance report for local fixture,
> stdio MCP fake and HTTP fake adapter scenarios. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-30`](../roadmap/integrations-connectors.md#step-int-30) |
| source snapshot | `be64bcc1` plus this INT-30 source slice |
| feature_status | `partial` for shared conformance case/report contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | fake adapter fixture → deny/replay/Unknown/TOCTOU/schema/success case → conformance report |

`ConnectorConformanceCase` records adapter kind, scenario, status, effect count, Unknown
preservation, replay dedupe and scope/TOCTOU fence evidence. Verified deny/schema cases require
zero effects; replay/Unknown cases require dedupe or uncertainty preservation. Non-verified cases
must carry a reason. `ConnectorConformanceReport` rejects duplicate adapter/scenario cases and
recomputes status/counts from cases, preventing a forged Complete/Verified summary.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| shared adapters | local fixture, stdio MCP fake and HTTP fake use the same case schema |
| deny/schema | verified deny path has effect_count=0 |
| replay | dedupe is verified and effects are bounded to one |
| Unknown | uncertainty remains visible and is not auto-success/retry |
| TOCTOU | scope fence evidence is required |
| report integrity | duplicate cases, forged counts and missing nonverified reason fail closed |

## CI and limitations

GitHub Actions runs `.github/workflows/int30-conformance.yml` with domain/Core fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: this is evidence schema/fixture coverage, not an adapter runner or property-test
engine; no MCP/HTTP network, connector effect, durable registry or live/physical proof is claimed;
INT-31+ remains open.
