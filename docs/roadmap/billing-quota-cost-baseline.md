# BQ-00 Billing / Quota / Cost Baseline

> Snapshot: `6904f0c54c9ff1b03fc01b7cc1cec100a43881c6` (2026-09-18). This is a source-only
> inventory. Runtime fixtures run in GitHub Actions; no local tests are executed.

## Current source boundaries

| Area | Current source and behavior | Proof/status ceiling |
|---|---|---|
| Runtime usage | `kiana-domain/src/usage.rs::UsageRecord` records request/run/step/provider/model, optional input/output tokens and elapsed time. `CostLedger::from_records` only sums a dimension when every record is known; `cost_micros` remains `None`. | `feature_status=partial`, `proof_level=source`; no rate card, currency, provider invoice or measured bill. |
| Model admission | `kiana-core/src/model_budget.rs::JournalModelBudget` reserves/settles bounded model budget facts through EventStore, validates prepared permits and authority/lease revisions, and preserves unknown settlement. | `feature_status=implemented` for existing model-budget slice; provider pricing, quota windows/capacity and cross-process reconciliation remain open. |
| Receipts | `kiana-core/src/receipts.rs::cost_ledger_from_events` projects `run.model_turn` usage into the legacy CostLedger; receipt aggregation separately carries usage/effect/provider references and unknown states. | Projection only; it is not a financial ledger or provider invoice. |
| Provider response | Domain `ModelUsage` carries optional input/output token counts; provider/stream adapters can report incomplete usage and error/unknown outcomes. | No neutral `UsageVector`/snapshot-delta-final accumulator or rate-card binding yet. |
| Cell / task capacity | `kiana-core/src/cell_registry.rs` keeps bounded in-process cell reservations and capability leases; Company policy separates project/runtime/quota budgets. | No durable quota reservation window, provider capacity queue or cross-process CAS projector. |
| Event facts | Existing model reservation/dispatch/settlement/unknown and capability facts are EventLog-bound; receipt and metrics are projections. | Existing facts remain readable; BQ steps must add explicit upcasters and never reinterpret missing usage as zero. |

## Conflict and migration inventory

| Existing field/path | Target contract | Migration owner | Constraint |
|---|---|---|---|
| `UsageRecord.input_tokens/output_tokens: Option<u64>` | `UsageVector` + `NormalizedUsage` with confidence/source/basis/sequence | BQ-01..03 | `None` is unknown, not zero; preserve legacy JSON and source digest. |
| `CostLedger.cost_micros: Option<u64>` | `Money`/`RateCard` estimated vs measured amounts | BQ-04..05, BQ-13 | Never calculate a bill from current price; no provider receipt means not measured. |
| `RuntimeBudget` | Run-scoped hard limits and `BudgetLease` intersection | BQ-06..09 | Do not merge with `ProjectBudget`, `Quota` or FinancialBudget. |
| `ProjectBudget` / `Quota` in Company policy | Project-period allocation vs capacity window | BQ-06..08, BQ-19 | Wire-supplied project/role/budget never becomes authority. |
| `JournalModelBudget` facts | Attempt reservation/settlement/release/unknown events | BQ-08, BQ-11..12 | Reuse EventLog/CAS and permit fence; no second budget loop. |
| `cost_ledger_from_events` | Source-cursor rollup and reconciliation projection | BQ-13..14, BQ-20 | Receipt remains a projection; corrections append facts. |
| CellRegistry in-memory reservations | Durable quota reservation + lease/fence/epoch | BQ-08, BQ-16, BQ-21 | Unknown or stale lease is conservatively retained and reconciled. |

## Current RED / explicit limits

- No `UsageVector`, `NormalizedUsage`, `RateCard`, `Money`, `QuotaReservation`, `CostLedgerEntry`
  or `CostCorrection` target contract is wired into the production path yet.
- Token usage is optional and provider-reported; a missing field is not a zero-cost assertion.
- `cost_micros` has no currency/rate-card/provider-receipt provenance and must stay unknown.
- Model reservation is bounded and EventLog-backed, but provider RPM/TPM, fair capacity queues,
  quota windows, invoice import, correction approval and durable rollups are not implemented.
- `UsageRecord`, Receipt and metrics are observations/projections; none authorizes capabilities,
  proves payment, or proves business outcome. FinancialBudget remains a non-authorizing boundary.

## CI fixture names and handoff

The BQ sequence keeps these names stable for later steps:

`runtime_and_project_budgets_are_not_interchangeable`, `usage_unknown_is_not_zero`,
`usage_snapshot_delta_final_is_monotonic`, `rate_card_version_is_pinned`,
`quota_reservation_never_overbooks_parent`, `unknown_settlement_is_not_released`,
`provider_invoice_requires_receipt`, `cost_correction_is_append_only`.

BQ-00 only records the source snapshot and migration boundary. BQ-01 owns stable IDs/schema and
unknown/reason enums; BQ-02/03 own normalized usage; BQ-04/05 own price arithmetic/rate cards;
later BQ steps own reservation, capacity, settlement, rollups, corrections, migration and release
evidence. No source implementation is claimed by this baseline.
