# SW-13 swarm merge reducer baseline

> Snapshot date: 2026-09-27. This slice adds a versioned deterministic merge reducer over SW-12
> typed child results. Local Cargo test/build/check/clippy/smoke commands are intentionally not
> run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SW-13`](../roadmap.md#step-sw-13) |
| source snapshot | `17f6cf71` plus this SW-13 source slice |
| feature_status | `partial` for deterministic partition coverage/strategy contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | typed child results → canonical partition order/strategy reducer → merge decision evidence |

`SwarmMergeDecision` rejects unordered or duplicate partition rows, missing coverage,
`ResultUnknown`, implicit first-success semantics and success outputs without evidence. `AllSuccess`
requires every partition success; `AllSettled` remains review-only; `ExplicitPolicy` requires a
policy and acceptance evidence digest for partial/failed outcomes. The reducer is pure and does not
merge artifacts, release budgets, append EventLog or mark Company acceptance.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| canonical order | unordered/duplicate/missing partition coverage is rejected |
| unknown | ResultUnknown cannot be treated as success or settled merge |
| strategy | AllSuccess/AllSettled/ExplicitPolicy semantics are distinct |
| partial policy | explicit policy and acceptance evidence are mandatory |
| integrity | partition/report digest and unknown fields fail closed |
| boundary | reducer has no Broker/EventLog/effect or first-success path |

## CI and limitations

GitHub Actions runs `.github/workflows/sw13-merge-reducer.yml` with domain/Core fixtures, source
guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands
are intentionally not run, and CI results are not awaited.

Limitations: this is a source reducer contract only; independent review/merge receipt, artifact
merge, budget release, Company acceptance and durable reducer replay remain SW-14+ work.
