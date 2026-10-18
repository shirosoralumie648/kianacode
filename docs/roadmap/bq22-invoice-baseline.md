# BQ-22 provider invoice and reconciliation baseline

> Snapshot date: 2026-10-18. Completion slice on branch `step/bq22-20261018-1`. This step adds the
> local-only document import adapter, binds invoice expectations to the authoritative cost ledger,
> and closes the discrepancy→correction loop through the existing BQ-14 approval path.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-22`](../roadmap.md#step-bq-22) |
| source snapshot | `92975663` plus this BQ-22 completion slice |
| feature_status | `implemented` for local invoice import, ledger-bound comparison, and approved correction admission |
| proof_level | `local_behavior` (two focused domain/core targets run locally); no durable/live/physical promotion |
| canonical path | local invoice file → strict parser → authenticated import fact → ledger-bound expectation → server-derived comparison → observation → BQ-14 approved correction |

Binding is the whole card: `InvoiceLedgerExpectation::from_ledger_entry` /
`TryFrom<&CostLedgerEntry>` copies the digest, period, model, usage digest (`source_digest` for
measured facts), measured amount and `provider_receipt_ref` from the committed entry, so an adapter
cannot hand-pass any of them. A fact without a period or model fails closed
(`provider_invoice_ledger_entry_period_missing` / `provider_invoice_ledger_entry_model_missing`),
and estimated facts keep usage/amount unknown so the comparison can only be reviewed.

`ProviderReceiptRef` is compared field by field: a differing receipt refuses before any comparison
fact exists (`provider_invoice_receipt_mismatch`), a missing side yields `receipt_unknown`, and a
`Matched` state now requires both receipt references. The comparison digest covers both fields.

The correction loop now has its missing caller: `ProviderInvoiceAdmission::admit_invoice_correction`
(and `ControlPlane::admit_provider_invoice_correction`) validates the handoff, requires the
committed target digest to equal the handoff digest, refuses an unapproved draft with
`cost_correction_approval_required`, re-checks the command's delta/evidence/reason binding, and then
delegates the approved command to `CostCorrectionAdmission::authorize`. It never appends a fact,
consumes an approval, or creates a second correction ledger.

## Local-only document parser

`parse_provider_invoice_document` in `kiana-domain/src/billing_invoice.rs` is a pure function over
bytes. It:

- enforces `MAX_PROVIDER_INVOICE_DOCUMENT_BYTES` (256 KiB) and UTF-8 before JSON;
- decodes `ProviderInvoiceDocument` with `deny_unknown_fields` (vendor exports with extra columns,
  including self-reported digests, are rejected);
- requires `kiana.provider-invoice-document.v1`, recomputes `import_digest` and the evidence digest
  locally, and refuses negative money, bad receipt references, bad digests, zero verification
  time/epoch/cursor, and blank verifier/import keys;
- performs no I/O, no sockets, no `std::net`, no signature verification.

`kiana-daemon::LocalProviderInvoiceImporter` is the only filesystem adapter. It requires an
absolute root, rejects absolute/`..`/prefix paths, canonicalizes and confines the file under the
root, requires a regular file, and re-checks the size bound while reading. It is not registered in
the capability broker or the runner's tool schemas, so the model-visible tool surface stays exactly
`shell`, `apply_patch`, `mcp`, `memory.search`, `memory.write`.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| `invoice_document_parser_refuses_unknown_fields_bad_shapes_and_bad_money` | empty/oversized/non-UTF-8, unknown/self-reported digest fields, wrong schema, negative/bad money, bad receipt/digest/usage/period, zero verified/epoch/cursor, blank key/verifier |
| `forged_ledger_digest_receipts_and_missing_comparison_fields_are_refused` | forged entry digest, receipt mismatch, missing receipt (review only), missing period/model, half/zero-width period |
| `ledger_expectation_is_derived_from_the_authoritative_entry_not_from_wire_text` | success: expectation equals the committed fact; estimated facts stay review |
| `local_invoice_document_parses_into_a_bound_import_and_evidence` | success: deterministic import + evidence bound to each other, append idempotent |
| `unapproved_invoice_draft_is_refused_and_approved_correction_is_admitted` | `cost_correction_approval_required` first, then an approved command yields a bound `CostCorrection` through the static and facade entry points |
| `forged_ledger_digest_and_drifted_approvals_are_refused_before_admission` | forged-ledger handoff target digest, drifted correction reference, missing reason |
| `correction_loop_owns_no_ledger_dispatch_or_network_authority` | core source guard: no append/consume/update/delete, no broker/model/process/net/fs, no tool schema; pins the domain refusal codes |
| `bq22_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards` | manual lane commands, dispatch-only, shard registration (`kiana-domain-s1/4`, `kiana-core-s1/6`) |

## CI and limitations

`.github/workflows/bq22-invoice.yml` is `workflow_dispatch`-only with `permissions: contents: read`;
it runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain fixtures, both core targets,
and `--no-run` compilation. The stale exemption line for that workflow is removed from
`scripts/ci/doc-reference-exemptions.txt`; the automatic lane is the existing sharded `ci.yml`.

Known limitations, stated rather than implied:

- **No durable import idempotency.** The import adapter is stateless and `ProviderInvoiceLedger` is
  in-memory. `cost.invoice_imported` / `cost.invoice_observed` are not yet folded into an
  EventLog-backed store, so replaying the same file in a new process is not prevented here.
- No real provider signature verifier; `authenticated`/`verifier_ref` are evidence fields, not
  cryptographic proof.
- No network/provider adapter by design; the parser and importer never open a socket.
- `admit_invoice_correction` returns a fact the caller must still append through the existing
  ControlPlane/EventStore transaction; no durable correction store is added here.
- Local evidence is two focused targets (`--locked --offline --test-threads=1`), not workspace
  build/clippy/smoke; CI results are not awaited.
