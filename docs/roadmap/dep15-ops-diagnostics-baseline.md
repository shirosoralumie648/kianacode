# DEP-15 - Ops diagnostics baseline

> Snapshot date: 2026-09-27. This slice defines read-only `ops status`, `ops doctor` and
> `ops preflight` diagnostics. Local Cargo test/build/check/clippy/smoke commands are intentionally
> not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-15`](../roadmap.md#step-dep-15) |
| source snapshot | `1405b222` plus this DEP-15 source slice |
| feature_status | `partial` for redacted diagnostics contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | server-owned config/startup/health/observability facts -> Core reducer -> DaemonHost read-only route |

`OpsDiagnosticsInput` requires one ordered fact for config, startup, health and observability.
`OpsDiagnosticsReport` derives `ready`, `degraded`, `blocked` or `unknown`, carries a stable exit
code, redacted remediation and a bounded `kiana ops <mode> --json` reproduction command. Unknown
or missing evidence can never produce `healthy=true`.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| ready | all four evidence axes produce stable JSON and human output with exit code `0` |
| unknown/missing | unknown remains `unknown`; missing evidence is `blocked`; both are non-healthy |
| redaction | secret markers, absolute paths, traversal and raw file URLs are rejected |
| integrity | unordered/cross-cursor facts, unknown fields and digest tamper fail closed |
| boundary | Core/Daemon paths only reduce or delegate; no repair, provider, broker, EventLog or filesystem effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep15-ops-diagnostics.yml` with domain fixtures, Core and
Daemon source guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and
smoke commands are intentionally not run, and CI results are not awaited.

This is a source-level, adapter-fact contract. It does not read config files or environment values,
probe providers, query EventLog, persist a diagnostic snapshot, execute remediation, wire CLI/Web/
Workbench/Desktop dispatch or change process exit status. The existing legacy `/app/doctor` command
remains a separate settings projection until a later route integration step.
