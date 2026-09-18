# CP-20 Unknown Reconciliation / Retry Baseline

## Scope

CP-20 keeps uncertain effects fenced and turns reconciliation into an evidence-gated, read-only
workflow. Unknown failures project a bounded `FailureIncident`/`RecoveryPlan` with owner, source
event, evidence and quarantine actions; platform reconciliation requires independent event evidence,
appends `failure.reconciled` without rewriting the original outcome, and explicitly keeps automatic
retry disabled. Connector Unknown receipts are listed and reconciled against the original binding,
account, operation, idempotency key and payload digest. Effect observations preserve Unknown until
verification; a future attempt must be a new authorized execution with its own budget/permit.

The generic error policy marks ResultUnknown/CompensationRequired as non-retryable and requiring
reconciliation; ModelPort/Cell/adapter contracts retain unknown usage and attempt identity rather
than treating an uncertain result as free or successful.

## Evidence and limits

- `kiana-core/tests/p2_k6_01_reliability.rs` covers incident/recovery/reconciliation/no-auto-retry
  source boundaries.
- `kiana-domain/tests/p4_k8_01_connector.rs` covers typed connector unknown/reconciliation contract;
  `kiana-core/tests/cp20_unknown_reconcile_guard.rs` adds the cross-layer guard.
- GitHub Actions runs fixtures, guard and workspace compile; local tests are intentionally not
  executed.

This slice is `feature_status=implemented`, `proof_level=source`: handler-specific external
verification, compensation adapters, durable multi-host reconcile claims and live/physical proof
remain ER/connector/provider work.
