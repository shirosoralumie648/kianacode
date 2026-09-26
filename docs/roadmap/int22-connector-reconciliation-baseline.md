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
delivery, external business outcome, compensation or cancellation settlement.
