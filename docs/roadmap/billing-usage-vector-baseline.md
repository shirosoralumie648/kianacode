# BQ-02 Usage Vector Baseline

## Scope

BQ-02 adds provider-neutral `UsageVector` and `NormalizedUsage` contracts. Optional token fields
preserve absent versus explicit zero (`None` versus `Some(0)`), while `UsageConfidence` distinguishes
known, partial and unknown observations with an explicit `BillingUnknownReason`. Each normalized
observation binds stable usage/attempt/run identity, provider/model/route provenance, source,
basis, snapshot/delta/final observation kind, optional sequence and a raw digest.

This slice validates one observation only. Sequence monotonicity, duplicate handling and
snapshot/delta/final accumulation remain BQ-03; no provider adapter, pricing, reservation or
financial cost is introduced.

## Evidence and limits

- `kiana-domain/tests/bq02_usage_vector.rs` covers absent/zero/present, source/basis/sequence,
  partial/unknown reason requirements and digest/tamper rejection.
- `kiana-core/tests/bq02_usage_vector_guard.rs` protects the domain-only/no-provider/no-I/O
  boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no rate-card arithmetic,
stream accumulator, quota reservation, provider invoice or durable usage projector is claimed.
