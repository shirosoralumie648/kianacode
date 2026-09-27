# BQ-22 provider invoice and reconciliation baseline

> Snapshot date: 2026-09-27. This slice adds immutable provider invoice import and comparison
> facts. Local Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub
> Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-22`](../roadmap.md#step-bq-22) |
| source snapshot | `29546366` plus this BQ-22 source slice |
| feature_status | `partial` for authenticated import, duplicate identity and discrepancy contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | authenticated provider receipt fact → duplicate fence → ledger comparison → existing BQ-14 correction route |

`ProviderInvoiceImport` requires a bounded provider/account/invoice identity, period, source
cursor, opaque receipt reference and explicit authentication evidence. Unauthenticated receipts
are rejected. The duplicate check is keyed by provider account, invoice identity and period, so a
replayed invoice cannot silently add another fact. Missing usage remains reviewable instead of
becoming zero.

`InvoiceComparison` keeps period, model, usage digest and amount differences as ordered reason
codes. A matched comparison requires complete equal fields; a mismatch needs a correction reference
and a review/unknown case cannot claim a correction. The reference is only a handoff to the
existing approval-bound BQ-14 command; this slice does not append or apply a correction.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| receipt authentication | unauthenticated or malformed period/shape is rejected |
| duplicate invoice | repeated provider/account/invoice/period identity is rejected |
| matched import | complete equal period/model/usage/amount produces `Matched` |
| correction path | period/amount mismatch carries explicit discrepancy codes and correction ref |
| unknown path | missing usage is `ReviewRequired`, never a measured success |
| shape/source guard | unknown fields/digest drift and effect execution paths fail closed |

## CI and limitations

GitHub Actions runs `.github/workflows/bq22-invoice.yml` with formatting, domain fixtures, the Core
source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

Limitations: authentication and source cursor are evidence fields; a provider-specific signature
verifier, durable import idempotency store, EventLog append, BQ-14 approval transaction, invoice
network adapter and live provider truth remain outside this source-only slice.
