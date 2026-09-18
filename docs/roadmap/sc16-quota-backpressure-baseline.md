# SC-16 Quota / Backpressure Baseline

## Scope

SC-16 closes the bounded-resource boundary across the existing product spine. Runtime, lease,
project and provider limits are intersected into an `EffectiveBudget`; UTC `QuotaWindowBudget`
checks request/token/concurrency headroom; digest-bound quota reservations use idempotency,
revision, authority and config fences; CellRegistry reserves and releases active capability slots;
Runner reserves model attempts/tokens and retains unknown usage; and the observability queue is
bounded, evicts only best-effort signals and synchronously rejects a full critical queue.

All these values remain admission/accounting facts, not a second authorization path. EventLog and
receipts remain the durable truth; queue flush or local budget state does not claim external effect
success.

## Evidence and limits

- `kiana-core/tests/sc16_quota_backpressure.rs` covers quota window overage rejection and critical
  fact preservation under best-effort backpressure.
- `kiana-core/tests/sc16_quota_backpressure_guard.rs` pins budget intersection, reservation/CAS,
  CellRegistry, Runner, model accounting, bounded queue and capacity metric markers.
- GitHub Actions runs the fixtures, source guard and workspace compile; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: durable cross-process quota
settlement, provider-specific live capacity, physical queue durability and external/live effect
proof remain SC-17+ / BQ / PD / ER work.
