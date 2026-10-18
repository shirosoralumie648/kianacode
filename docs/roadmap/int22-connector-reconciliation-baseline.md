# INT-22 connector Unknown reconciliation baseline

## Scope

INT-22 quarantines an Unknown connector result and requires an explicit reconciliation case. A
provider query or manually supplied receipt can close the case only when it matches the original
invocation, attempt, connector binding, account, operation, idempotency key and payload hash.
Reconciliation is a successor fact and never rewrites the original Unknown receipt.

## Implemented source slice

- `ConnectorReconciliationCase::from_unknown` creates a digest-bound Pending case with
  `automatic_retry_allowed=false`, safe actions for query/manual evidence/human review, and
  forbidden actions for automatic retry, idempotency-key reuse and original receipt replacement.
- `ConnectorReconciliationEvidence` validates the resolved receipt and effect observation through
  the INT-20 contract. `attach_evidence` rejects cross-binding, cross-account, cross-operation,
  payload, key, owner/audience and attempt drift; `commit_reconciled` is a separate terminal step.
- `ConnectorHumanInboxItem` exposes the safe/forbidden action projection. The existing daemon
  `connector.reconcile` path now carries the case through manual evidence and appends it alongside
  the reconciled successor fact.

## 2026-10-17 completion slice (CI-only)

The card's refusal faces and success columns are now executable, deny-first fixtures:

- **timeout must not be recorded as failed** — `from_unknown` requires an Unknown receipt
  (`connector_reconciliation_unknown_receipt_required`) *and* an Unknown-state observation
  (`connector_reconciliation_unknown_observation_required`), so a timeout cannot be re-labelled as
  a plain failure and cannot later be "retried" on that basis.
- **no receipt, no retry** — `ConnectorReconciliationCase::authorize_automatic_retry` denies with
  `connector_reconciliation_automatic_retry_forbidden` in Pending, EvidenceAttached and Reconciled;
  closing a case requires a hash-pinned artifact (`connector_reconciliation_manual_evidence_required`)
  or a recorded query digest (`connector_reconciliation_query_evidence_required`); committing
  without evidence is refused (`connector_reconciliation_evidence_required`), re-committing is
  refused (`connector_reconciliation_case_already_reconciled`) and re-attaching to a closed case is
  refused (`connector_reconciliation_case_not_pending`).
- **reconcile never covers/rewrites the original event** — the case keeps
  `original_receipt_digest` across every transition, and the new
  `ConnectorReconciliationSuccessor` (`kiana.connector-reconciliation-successor.v1`) pins
  invocation/attempt/connector/binding/account/operation + original/reconciled receipt and
  observation digests with `appends_new_fact=true` and `replaces_original_fact=false`. A successor
  that claims to replace the original, or that belongs to another case/outcome, is refused
  (`connector_reconciliation_successor_invalid` / `connector_reconciliation_successor_mismatch`),
  and `kiana-domain/src/connectors.rs` re-validates any `reconciliation_successor` in a committed
  `connector.reconciled` fact against that fact's case.
- **explicit reconcile appends a fact; safe/forbidden actions reach the Human Inbox** — the daemon
  invoke output for an Unknown outcome carries `reconciliation_required`,
  `automatic_retry_allowed=false`, the case state and the `human_inbox_item`; the `list` projection
  surfaces the same pending case + inbox item for every unresolved invocation; the reconcile output
  and the appended `connector.reconciled` fact carry the reconciled case and its successor, and the
  original `connector.invoked` event stays byte-identical. `ConnectorHumanInboxItem` now carries the
  bounded case identity (invocation/attempt/connector/binding/account/operation/payload hash +
  observed outcome) and is checked against its case
  (`connector_human_inbox_item_mismatch`).

Fixtures (`kiana-domain/tests/int22_connector_reconciliation.rs`, deny-first, success last):
`a_timeout_cannot_be_recorded_as_a_failure_or_open_a_quarantine`,
`a_quarantine_never_authorizes_a_retry_without_evidence`,
`evidence_from_another_binding_account_operation_key_or_attempt_is_refused`,
`reconciliation_appends_a_successor_and_never_rewrites_the_original_receipt`,
`explicit_reconcile_projects_the_human_inbox_and_binds_pending_and_query_evidence`.

Daemon real-spine fixtures (`kiana-daemon/tests/int22_connector_reconciliation.rs`,
`DaemonHost → ControlPlane → Broker → EventStore` + hash-pinned `local_fixture`):
`an_unknown_outcome_is_quarantined_with_a_human_inbox_item_and_no_automatic_retry`,
`wire_supplied_reconciliation_material_is_refused_before_any_fact`,
`explicit_reconcile_appends_a_successor_fact_and_keeps_the_original_event`.

CI wiring: the manual `workflow_dispatch`-only `.github/workflows/int22-connector-reconciliation.yml`
lane runs `cargo fetch --locked`, the fmt gate, the three targets with `--test-threads=1` and three
compile-only steps; the automatic authority stays the unified `ci.yml` shards
(`kiana-domain-s3/4` → `int22_connector_reconciliation`, `kiana-core-s4/6` →
`int22_connector_reconciliation_guard`, whole-crate `kiana-daemon` for the daemon target).
`scripts/ci/test-shards.json` is unchanged.

## CI-only evidence

GitHub Actions runs the reconciliation fixtures, source guard, formatting and workspace test-target
compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not run and CI is
not awaited.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

The provider query remains a contract and no live query transport is opened. Manual receipt files
are still supplied evidence; the slice does not prove provider truth, durable cross-process inbox
delivery, external business outcome, compensation or cancellation settlement. Unknown quarantine is
in-process state derived from committed connector facts: restart recovery, projection rebuild and
lease settlement belong to INT-23/INT-29. The daemon still accepts only manual evidence files; the
`provider_query` source exists in the domain contract and is exercised by the domain fixtures only.
