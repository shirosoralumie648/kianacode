# P4-J7-24 · Provider usage, price snapshots and budget settlement baseline

This slice adds the source boundary for provider usage and cost settlement. Provider reported
usage remains an observation; the domain contract pins the requested/served model, route, attempt
and rate-card revision while keeping estimated, measured and unknown cost separate.

## Source contract

- `kiana-domain::ProviderUsageSettlement` requires server-owned usage/run/attempt identity, a
  provider and route, bounded idempotency, a final `NormalizedUsage` observation and a digest.
  `from_usage_with_execution_id` additionally binds a non-nil `ExecutionId` supplied by the
  authoritative caller; the provider adapter has no input for choosing that identity.
- `SettlementCost::Unknown` preserves missing usage, missing rate cards and incomplete provider
  fields. It is never represented as zero cost. `Estimated` carries the `RateCard` ID/version;
  `Measured` requires an opaque `ProviderReceiptRef` and `Money` amount.
- `ProviderUsageLedger` is an attempt-local deterministic deduplication boundary. It rejects a
  conflicting second settlement and treats the exact digest as a replay; it is not a durable
  EventLog projector or a financial ledger.
- `kiana-provider::normalize_model_reply` copies only server-owned route/model identity and turns
  absent provider usage into `UsageConfidence::Unknown`. The caller supplies attempt/run IDs;
  provider wire data cannot authorize budget consumption.

## CI-only fixtures

`.github/workflows/p4-j7-24-usage-settlement.yml` is a `workflow_dispatch`-only lane with
`permissions: contents: read`; it runs `cargo fetch --locked`, `cargo fmt --all --check` and the
three focused fixtures (`-p kiana-domain --test p4_j7_24_usage_settlement`,
`-p kiana-provider --test p4_j7_24_usage_containment`,
`-p kiana-core --test p4_j7_24_usage_settlement_guard`, each `--locked -- --test-threads=1`). The
automatic lane is the unified `ci.yml`: `kiana-domain-s3/4` lists `p4_j7_24_usage_settlement`,
`kiana-provider` is a `targets=null` whole-crate shard (so the new target is discovered
automatically) and `kiana-core-s5/6` lists `p4_j7_24_usage_settlement_guard`. Results are pending
and unobserved. Local tests, builds, checks, clippy and smoke commands are intentionally not run.

The fixtures cover missing provider/rate-card usage, explicit non-nil execution identity binding,
measured receipt attachment, exact replay, conflicting attempt settlement, run identity drift and
the absence of Broker/EventLog/network authority in the value adapters.

## 2026-10-17 completion slice (CI-only)

The 2026-10-17 slice (per-protocol containment, the attempt reserve/dispatch/settle/release ledger
and the late-usage correction) was written in a CI-only session: only `cargo fmt --all` was run
locally, and no build, check, clippy, smoke or test command was executed. Its results are source
plus CI wiring only and stay unobserved until the manual lane or the unified shards report.

- `kiana-domain/src/provider_usage_settlement.rs`: `ProviderUsageContainment` declares the cache
  read/write and reasoning relationships per wire protocol as three-way (`Some(true)` contained,
  `Some(false)` disjoint, `None` undeclared); `PricedUnits`/`ChargeableUsage` count every reported
  token once with checked arithmetic and refuse a declaration the numbers contradict
  (`provider_usage_containment_violated:<field>`) or cannot prove
  (`provider_usage_containment_unprovable:<field>`). `RateCardSnapshot` pins
  source/currency/validity/version/digest, `ProviderUsageCorrection` appends late usage carrying
  `replaces_settlement_digest`, and `ProviderUsageAttemptLedger` records
  reserve/dispatch/settle/release/correct. Unknown consumption keeps the reserved upper bound
  charged, a charge above the hold is committed before `provider_usage_charge_exceeds_reservation`
  (mirroring the CP-11 rule that an overspend fact is never dropped), only a never-dispatched hold
  can be released, a replay is idempotent by digest, and a delta observation is refused at
  settlement (`provider_usage_delta_observation_not_settleable`). `from_usage` keeps its previous
  conservative rule; only `from_usage_with_protocol` consumes the table. A self-hosted route
  (`OllamaChat`) can never present a zero-priced card as a priced call.
- `kiana-domain/src/billing_pricing.rs`: `estimate` keeps its behaviour through `estimate_inner`,
  and `estimate_with_containment` charges a contained dimension once while an undeclared overlap
  still yields `Unsupported` instead of a priced estimate.
- Fixtures: `kiana-domain/tests/p4_j7_24_usage_settlement.rs` covers the card's acceptance list
  (`missing_usage_and_price_remain_unknown`, `usage_overflow_is_rejected`,
  `concurrent_model_calls_cannot_overspend_reserved_budget`,
  `cumulative_cache_and_reasoning_usage_is_not_double_counted`,
  `failed_attempt_usage_is_retained`, `replayed_usage_is_settled_once`) plus
  `delta_observation_is_not_settled_as_cumulative`, the identity/execution/run/release/rate-card
  scope fail-closed cases and the over-reservation commit;
  `kiana-provider/tests/p4_j7_24_usage_containment.rs` normalizes one raw response per protocol and
  proves the table matches what the adapter extracts (OpenAI chat/responses contained, Gemini cache
  contained but thinking tokens additive, Anthropic disjoint, Legacy/Ollama undeclared) and that
  known tokens without a rate card never become a billed amount;
  `kiana-core/tests/p4_j7_24_usage_settlement_guard.rs` binds the new markers, adds
  `billing_pricing.rs` to the checked sources and keeps the value-layer boundary (no authority, no
  event write, no wrapping/saturating arithmetic).

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus CI wiring. The new
execution binding is a value-level seam only; no production ControlPlane/EventStore caller is
claimed until that integration is wired and verified. It does not
claim durable reservation/CAS, provider invoice reconciliation, project/org financial allocation,
capacity queues, live provider billing, cross-process replay or physical external effects. BQ-10+
and later Provider/Receipt/EventLog steps must consume these contracts before any budget or cost
statement can be promoted.

The 2026-10-17 completion slice stays under the same ceiling: `ProviderUsageAttemptLedger` is a
deterministic in-memory value boundary, so durable reservation/CAS, the production ControlPlane
caller, Receipt projection of the new cost/unknown states and cross-process replay are still open.
Token counting is explicitly not a bill — an unpriced dimension keeps the estimate unknown and a
self-hosted route keeps `Unknown{Unsupported}` even with a zero-priced card; the new fixtures are
source plus CI wiring whose receipts were not awaited.

## 2026-10-06 current-head settlement invariant receipt

The constructor now derives `BillingState::Unknown` from an unknown `SettlementCost`, including
known provider usage without a rate card or a priced dimension. State transitions validate a
candidate before assignment, so rejected unknown-cost settlement/correction attempts leave state
and digest unchanged. Measured receipt validation reuses `ProviderReceiptRef` constraints after
serde as well as on attachment.

```text
source_snapshot: current dirty worktree; kiana-domain/src/provider_usage_settlement.rs and kiana-domain/tests/p4_j7_24_usage_settlement.rs
worktree_status: P4-J7-24 remains roadmap row 401 🔄 / feature_status=implemented / proof_level=source; no manifest/lockfile or automatic commit/push/merge/rebase
command_argv: bounded serial `cargo test -p kiana-domain --test p4_j7_24_usage_settlement --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test p4_j7_24_usage_settlement_guard --locked --offline -- --test-threads=1`; `cargo test -p kiana-domain --test bq01_billing_contracts --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB test stack, stripped/non-incremental profile; final domain 7/7 max RSS 258440 KiB (~252.4 MiB), Core 1/1 max RSS 4098280 KiB (~3.91 GiB), BQ-01 3/3 max RSS 238908 KiB (~233.3 MiB), swaps 0; `/tmp/kiana-roadmap-20261006-p4j7-24/{green-final,core-final,bq01}.{log,resources}`
fixture·cassette: missing rate card/known usage, known usage with an unpriced nonzero tool dimension, explicit execution ID, measured receipt replay, unknown transition no-mutation, and post-serde secret receipt rejection; no provider request or EventLog write
exit_code: all three targets 0
status_change: selected settlement state/cost and receipt validation behavior moved to local_behavior; whole P4-J7-24 remains 🔄
proof-level_change: selected source → local_behavior; no durable/live/physical promotion
limitations: ControlPlane/EventStore production caller, durable CAS, reservation/consume/release integration, late correction producer, provider invoice reconciliation, capacity/fallback and live billing remain open; full P4-J7-24 acceptance is not claimed
reviewer: root integration agent; P4-J7-24 read-only settlement audit; focused Rust test reviewer
```
