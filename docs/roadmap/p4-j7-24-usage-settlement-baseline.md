# P4-J7-24 · Provider usage, price snapshots and budget settlement baseline

This slice adds the source boundary for provider usage and cost settlement. Provider reported
usage remains an observation; the domain contract pins the requested/served model, route, attempt
and rate-card revision while keeping estimated, measured and unknown cost separate.

## Source contract

- `kiana-domain::ProviderUsageSettlement` requires server-owned usage/run/attempt identity, a
  provider and route, bounded idempotency, a final `NormalizedUsage` observation and a digest.
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

`.github/workflows/p4-j7-24-usage-settlement.yml` runs the domain settlement fixture, provider
source guard, Core boundary guard, formatting and workspace test-target compilation on GitHub
Actions. Local tests, builds, checks, clippy and smoke commands are intentionally not run.

The fixtures cover missing provider/rate-card usage, measured receipt attachment, exact replay,
conflicting attempt settlement, run identity drift and the absence of Broker/EventLog/network
authority in the value adapters.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus CI wiring. It does not
claim durable reservation/CAS, provider invoice reconciliation, project/org financial allocation,
capacity queues, live provider billing, cross-process replay or physical external effects. BQ-10+
and later Provider/Receipt/EventLog steps must consume these contracts before any budget or cost
statement can be promoted.
