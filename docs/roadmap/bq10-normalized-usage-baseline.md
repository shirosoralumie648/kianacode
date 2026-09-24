# BQ-10 normalized provider usage baseline

> Snapshot date: 2026-09-25.  This source slice wires protocol-specific provider usage fields to
> one neutral billing vector.  Runtime tests are GitHub-only and are not run locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-10`](../roadmap.md#step-bq-10) |
| source snapshot | `53f32107` plus this BQ-10 source slice |
| feature_status | `implemented` for bounded source contracts and CI wiring |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | admitted `PreparedModelCall` + provider response observation → `normalize_provider_usage` / `normalize_model_reply` → `NormalizedUsage` |

The adapter covers Anthropic Messages, OpenAI Chat, OpenAI Responses, Ollama, Gemini Interactions
and the deterministic legacy/Fake fixture shape.  The existing admitted `ModelReply` adapter
shares the same absent-field handling. Requested model and run/attempt identity remain
server-owned; a provider served-model observation is retained separately.  Core token fields are
never synthesized from a reported total.  Missing fields stay `Partial` or `Unknown`, and a
malformed value or contradictory total is rejected before settlement.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| protocol mapping | all supported protocol shapes map input/output/cache/reasoning fields to the same vector |
| missing usage | absent provider usage stays `Unknown`; a one-sided usage stays `Partial` and is not zero-filled |
| malformed usage | wrong numeric types, negative values and bounded-token overflow fail closed |
| total consistency | reported total is checked against complete core components; contradictions are rejected |
| requested/served model | route requested model is preserved while an observed served model remains a separate field |
| source boundary | adapter emits only `NormalizedUsage`; it has no permit, budget, EventLog, Broker or network authority |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the provider fixture, the
core source guard and `cargo check --workspace --tests --locked`.  Local tests, builds, checks,
clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: this adapter does not prove provider billing truth, durable settlement, invoice
reconciliation, live protocol behavior, external effects or crash recovery.  The response decoder
remains responsible for protocol syntax and stream accumulation; BQ-11+ owns attempt lifecycle,
settlement and projection integration.
