# BQ-01 Billing Contract Baseline

## Scope

BQ-01 adds stable domain identities (`UsageId`, `ReservationId`, `LedgerEntryId`, `RateCardId`,
`CostCorrectionId`, `QuotaReservationId`), a major-versioned billing contract header, explicit
unknown/reason values, state-transition rules and stable billing error codes. `ProviderReceiptRef`
is an opaque, bounded, secret-free reference only.

`BillingContractHeader` rejects unknown major versions, digest drift and inconsistent unknown
reasons. `BillingState` keeps reserved/observed/settled/released/unknown/expired/corrected
transitions explicit; unknown is never silently treated as zero or success. These are inert domain
contracts and do not authorize execution, reserve provider capacity or establish a financial bill.

## Evidence and limits

- `kiana-domain/tests/bq01_billing_contracts.rs` covers stable IDs, schema-major rejection,
  unknown/reason consistency, state transitions, error parsing and opaque receipt references.
- `kiana-core/tests/bq01_billing_contract_guard.rs` protects the domain-only/no-network/no-I/O
  boundary. GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: UsageVector/NormalizedUsage,
snapshot/delta accumulation, Money/RateCard arithmetic, reservations, provider billing and
durable quota remain BQ-02+.
