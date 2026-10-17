# BQ-13 estimated/measured cost and Receipt breakdown baseline

> Snapshot date: 2026-09-25. This slice adds a bounded source contract and read-only receipt/query
> projections. Runtime fixtures run in GitHub Actions; no local tests are executed.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-13`](../roadmap.md#step-bq-13) |
| source snapshot | `2d31a8b0` plus this BQ-13 source slice |
| feature_status | `implemented` for cost breakdown contracts, Receipt aggregation wiring and query projector |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | normalized usage → pinned estimate or provider measured receipt → Receipt/query breakdown |

`CostBreakdown::from_usage` keeps absent/partial usage and missing price dimensions explicit. A
known estimate carries the exact `RateCardId` and `card_version` and exposes checked per-dimension
lines. `from_provider_receipt` rejects incomplete usage, missing RateCard scope, currency drift and
invalid opaque receipts; the serialized measured state contains only the provider receipt and
amount, never a rate-card estimate. Unknown retains a `BillingUnknownReason` and never writes a
numeric zero.

`ReceiptCostBreakdown` folds per-attempt facts into separate estimated/measured totals and a stable
unknown-reason list. Estimate totals are projection data only and cannot authorize execution or a
financial budget. Replayed estimate→measured facts replace the estimate for one attempt so a
provider receipt is not double-counted. Cross-run entries, duplicate source events, stream gaps,
kind drift and invalid transitions fail closed.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| pinned estimate | amount uses checked integer micros, has rate-card id/version and dimension lines |
| missing card / partial usage | state is Unknown with a reason and no amount; no cost=0 fallback |
| measured admission | incomplete usage or absent RateCard is rejected; measured carries opaque provider receipt |
| receipt fold | estimated and measured totals remain separate; unknown reasons stay visible |
| replay transition | one attempt's estimate→measured transition counts only the measured amount |
| identity/source fence | cross-run entries, duplicate events, stream gaps and malformed cost payloads fail closed |
| core Receipt | existing aggregation keeps legacy fields and adds optional typed `cost_breakdown` |
| query projector | source cursor/event IDs and unknown limitations are rebuilt read-only from committed facts |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain/core/query BQ-13
fixtures, the source guard and `cargo check --workspace --tests --locked`. Local tests, builds,
checks, clippy and smoke commands are intentionally not run; CI results are not awaited.

Limitations: the contracts and projectors are read-only source projections. They do not append or
flush EventLog facts, consume permits, release quota reservations, import invoices, apply
corrections, reconcile external provider accounts or provide cross-process checkpoints. Financial
allocation, append-only ledger corrections and durable daily rollups remain BQ-14/BQ-19/BQ-20;
there is no provider billing, live or physical effect proof here.

## 2026-10-17 completion slice (CI-only)

The estimate/measured/unknown contract above is unchanged in shape.  This slice closes the card's
remaining refusal matrix, makes the receipt fold reject replayed or attempt-less entries, keeps the
legacy flat cost fields derived from the typed breakdown, restores the manual lane deleted by the
workflow consolidation (`08552ada`) and keeps the automatic shards.  Nothing here promotes the
proof level beyond `source`; local tests/builds/checks/clippy/smoke are not run and the CI receipt
is not awaited.

Behavior fixtures, denials first (`kiana-domain/tests/bq13_cost_receipt.rs`, 7 → 15):

| Fixture | Assertion |
|---|---|
| `estimate_scope_drift_and_invalid_observation_are_refused` | foreign provider, foreign model selector, a not-yet-effective card, a zero request count and a zero observation instant are refused with stable codes |
| `measured_rejects_negative_amount_and_scope_or_currency_drift` | negative measured micros, currency drift, an expired card and a missing card are refused before a measured fact exists |
| `estimate_lines_use_checked_integer_micros` | amount and per-dimension lines use checked integer micros (10×2 + 4×3 + 1×5 = 37) |
| `measured_zero_is_an_explicit_observation_and_unknown_never_carries_an_amount` | a zero amount is only representable as an explicit measured receipt; unknown serializes a reason with no amount and a smuggled amount fails to decode |
| `receipt_breakdown_requires_unique_attempts_and_rejects_replayed_entries` | attempt-less entries (`receipt_cost_attempt_missing`), two facts for one attempt (`receipt_cost_attempt_duplicate`) and an exact replay (`receipt_cost_entry_duplicate`) are refused |
| `receipt_breakdown_recomputes_totals_and_rejects_tampered_totals` | the estimated/measured totals are re-derived; a recomputed self-digest cannot bless a drifted total |
| `cost_reducer_refuses_duplicate_stream_gaps_kind_drift_and_usage_digest_drift` | duplicate runtime event id, revision gap, envelope kind drift, usage-digest drift and foreign billing-attempt stream metadata are refused |
| `estimate_and_measured_facts_are_registered_billing_attempt_events` | all three cost kinds pass `validate_runtime_event`/`validate_event_payload` as `billing_attempt` facts; a missing `attempt_id` or an extra field is refused |
| `estimate_is_card_version_bound_and_has_dimension_lines`, `overlapping_usage_is_unknown_before_cost_lines_are_emitted`, `missing_card_or_partial_usage_is_unknown_and_never_zero`, `measured_requires_card_and_complete_usage_but_serializes_only_receipt`, `receipt_breakdown_keeps_estimate_measured_and_unknown_separate`, `receipt_breakdown_rejects_cross_run_entries`, `cost_events_replay_latest_measured_without_double_counting_estimate` | the original pinned-estimate, overlap-unknown, no-zero, receipt-only measured, run-mismatch and no-double-count fixtures are preserved |

Core fixtures (`kiana-core/tests/bq13_cost_receipt.rs`, 3 → 6):

| Fixture | Assertion |
|---|---|
| `receipt_aggregation_keeps_legacy_cost_fields_in_sync_with_the_breakdown` | estimate-only, measured-only, unknown-only and estimate+measured attempts keep `cost_micros`/`cost_estimated` exactly derivable from the breakdown |
| `receipt_aggregation_rejects_tampered_cost_consistency` | even with recomputed self-digests, a legacy field that disagrees with the breakdown, a drifted total and a numeric zero next to unknown reasons are refused |
| `aggregate_receipt_facts_rejects_duplicate_and_malformed_cost_facts` | duplicated cost events and undecodable but run-bound cost payloads fail the aggregation with `receipt_cost_projection:<reason>` |

Core guard (`kiana-core/tests/bq13_cost_receipt_guard.rs`): expanded markers plus negative
assertions (no `EventStorePort`/`CapabilityBrokerPort`/`commit_transition`/`append_event(`/`std::fs`/
`std::net`/`TcpStream`/`unsafe`/`FinancialBudget`/`#[ignore`/`Uuid::new_v4` in the cost contract or
query projector), the refusal-code matrix bound to fixtures, and
`bq13_fixture_names_are_wired_to_the_manual_lane_and_the_auto_shards` which pins every fixture name,
the manual-lane commands and the `scripts/ci/test-shards.json` registrations.

Query fixtures (`kiana-query/tests/bq13_cost_projector.rs`, 3 → 6):
`query_projector_rejects_envelope_cursor_drift` (envelope cursor must equal the breakdown cursor),
`query_projector_keeps_unknown_reasons_and_measured_total_separate` (unknown shows its reason with
no amount; measured carries only the receipt amount) and
`query_projector_rejects_duplicate_and_malformed_cost_facts` (duplicate event ids and undecodable
payloads fail closed instead of becoming a zero-cost projection).

CI wiring: manual `.github/workflows/bq13-cost-receipt.yml` (`workflow_dispatch` only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, the three fixture
targets and the guard, plus four `--no-run` compile steps).  Automatic coverage stays with the
unified `ci.yml` shards `kiana-domain-s1/4` (`bq13_cost_receipt`), `kiana-core-s1/6`
(`bq13_cost_receipt`, `bq13_cost_receipt_guard`) and the whole-crate `kiana-query` shard;
`scripts/ci/test-shards.json` was already complete for these targets.

Limitations of this slice: local tests/builds/checks/clippy are not run (CI-only session) and the
CI receipt is not awaited.  Cost facts and projections remain read-only: they do not append/flush
EventLog facts, consume or release reservations, import provider invoices, apply corrections,
allocate FinancialBudget or provide cross-process checkpoints; there is no provider billing, live
or physical effect proof here (BQ-14/BQ-19/BQ-20/BQ-22 and later steps own those).
