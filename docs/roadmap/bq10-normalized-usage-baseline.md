# BQ-10 normalized provider usage baseline

> Snapshot date: 2026-09-25, extended 2026-10-17 with the CI-only completion slice.
> This source slice wires protocol-specific provider usage fields to one neutral billing vector.
> Runtime tests are GitHub-only and are not run locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-10`](../roadmap.md#step-bq-10) |
| source snapshot | `53f32107` plus the BQ-10 source slice; 2026-10-17 slice adds `provider_usage_containment` / `check_containment` on top of `97d840d4` |
| feature_status | `implemented` for bounded source contracts and CI wiring |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | admitted `PreparedModelCall` + provider response observation → `normalize_provider_usage` / `normalize_model_reply` → `NormalizedUsage` |
| manual lane | `.github/workflows/bq10-normalized-usage.yml` (`workflow_dispatch`-only) |
| automatic lane | unified `ci.yml`: `kiana-provider` whole-crate shard covers `bq10_normalized_usage`; `kiana-core-s1/6` lists `bq10_normalized_usage_guard` |

The adapter covers Anthropic Messages, OpenAI Chat, OpenAI Responses, Ollama, Gemini Interactions
and the deterministic legacy/Fake fixture shape.  The existing admitted `ModelReply` adapter
shares the same absent-field handling. Requested model and run/attempt identity remain
server-owned; a provider served-model observation is retained separately.  Core token fields are
never synthesized from a reported total.  Missing fields stay `Partial` or `Unknown`, and a
malformed value or contradictory total is rejected before settlement.

## Per-protocol field containment

`provider_usage_containment(protocol)` exposes the domain-owned three-way declaration to adapter
consumers without copying the table, and `normalize_provider_usage` enforces the provable half of
it before an observation can be recorded:

| Protocol | `cache_read_contained_in_input` | `cache_write_contained_in_input` | `reasoning_contained_in_output` |
|---|---|---|---|
| OpenAI Chat / Responses | `Some(true)` — `cached_tokens` sits inside the prompt count | `None` | `Some(true)` — `reasoning_tokens` sits inside the completion count |
| Gemini Interactions | `Some(true)` — `cached_content_token_count` sits inside the prompt count | `None` | `Some(false)` — thinking tokens are reported on top of the candidate count |
| Anthropic Messages | `Some(false)` — cache read is reported outside `input_tokens` | `Some(false)` | `None` |
| Ollama / Fake (Legacy) | `None` | `None` | `None` |

A dimension the protocol declares contained that exceeds its reported container is refused with
`provider_usage_containment_violated:<container field>`.  An undeclared (`None`) relationship is
never treated as additive or as contained: the observation is kept conservative and the
containment arithmetic reports `provider_usage_containment_unprovable:<container field>` when a
dimension is present without its container.

## Failure-first fixture matrix

`kiana-provider/tests/bq10_normalized_usage.rs`:

| Fixture / guard | Assertion |
|---|---|
| `all_supported_provider_shapes_map_to_one_neutral_vector` | all six protocol shapes map input/output/cache/reasoning fields to the same vector and record `kiana.provider-usage-adapter.v1:<dialect>` (Legacy → `fake`) |
| `missing_usage_stays_unknown_and_partial_fields_never_become_zero` | absent provider usage stays `Unknown`; a one-sided usage stays `Partial` and is not zero-filled |
| `malformed_or_inconsistent_provider_usage_is_rejected_before_settlement` | non-object response/usage/details, wrong numeric types, negative, float and bounded-token overflow values, and unbounded/control-character served-model strings each return their stable rejection code; the bound value itself is accepted, not truncated |
| `reported_total_contradictions_are_rejected` | reported total is checked against complete core components and one-sided components; contradictions are rejected while an equal total stays `Known` and a one-sided total stays `Partial` |
| `requested_and_served_model_are_separate_observations_and_the_response_cannot_rewrite_identity` | route requested model, provider, connection, usage/attempt/run identity survive a response that tries to spoof every identity field; the served model stays a separate observation and no spoofed text reaches the encoded record |
| `server_run_identity_binding_is_enforced_before_usage_is_recorded` | a server-installed `ModelAssignment` run id must match the caller's run (`provider_usage_run_mismatch`); an implausible retry ordinal is refused |
| `provider_self_reported_fields_cannot_extend_or_authorize_the_observation` | authorization/permit/grant sentinels are recorded only as the raw digest, never surface in the encoded observation, an honest round-trip is stable, and an injected `permit` field is unrepresentable (`deny_unknown_fields`) |
| `declared_containment_matches_every_wire_dialect` | the declaration table matches all six dialects and `chargeable` counts every reported token exactly once (OpenAI net = input+output; Anthropic additive; Gemini cache contained/thinking additive; Ollama declared-none; Fake undeclared-overlap) |
| `containment_contradictions_are_rejected_before_settlement` | contained dimension > container is refused for OpenAI Chat/Responses and Gemini; the same number is legal for Anthropic (independent); undeclared Fake stays conservative; a dimension without its container stays `Partial` and its arithmetic is `provider_usage_containment_unprovable:<field>` |
| `normalized_digest_is_over_safe_shape_and_does_not_authorize_effects` | the digest covers the safe shape only and provider authorization material never enters the record |
| `legacy_reply_without_served_model_keeps_model_observation_unknown` | the admitted `ModelReply` adapter keeps a missing served model as `None` |

`kiana-core/tests/bq10_normalized_usage_guard.rs`:

| Guard | Assertion |
|---|---|
| `normalized_usage_adapter_is_a_bounded_observation_boundary` | containment markers and rejection vocabulary are present, the legacy adapter keeps the absent-usage branch, and neither adapter file contains EventStore/ControlPlane/Broker/permit/approval/network/secret authority |
| `bq10_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards` | every fixture name is pinned, the manual lane stays `workflow_dispatch`-only with fetch/fmt gates and both test/compile steps, and the automatic shards still cover both targets |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

The manual lane runs `cargo fetch --locked`, `cargo fmt --all --check`, the provider fixture, the
core source guard and both `--no-run` compiles.  The automatic lane remains the unified `ci.yml`
shards.  Local tests, builds, checks, clippy and smoke commands are intentionally not run; CI
results are not awaited.

Limitations: this adapter does not prove provider billing truth, durable settlement, invoice
reconciliation, live protocol behavior, external effects or crash recovery.  The response decoder
remains responsible for protocol syntax and stream accumulation; BQ-11+ owns attempt lifecycle,
settlement and projection integration.
