# CP-26 decision trace / audit / evidence baseline

## Scope

CP-26 closes the diagnostic evidence boundary around the ControlPlane.  Decisions, authorization
snapshots, approvals, dispatch attempts, results, verification and reconciliation retain stable
request/command/correlation/causation links, authority/data epochs, action/input digests and
source-event references.  Audit and explanation paths are projections of committed facts, not
new authority paths.

## Evidence gate

- `cp_decision_trace_links_every_effect_to_its_authority` checks the server-owned audit reducer,
  CorrelationContext, SecurityContext/AuthorityFence, ApprovalBinding, EventLog identity links,
  dispatch commit confirmation, typed Receipt and daemon effect telemetry.
- `cp_secret_never_appears_in_error_event_receipt_or_explain` checks SecurityReason digest-only
  explanations, EventLog/Receipt/export redaction and provider diagnostic presence-only fields.
- `cp_explain_does_not_consume_grant_or_approval` checks authenticated read-only audit query/export
  routing and forbids permit, approval, broker or handler calls in the explain path.

The GitHub workflow runs existing OA/ER/SC audit, receipt, redaction and security fixtures, then
the CP-26 source guard and workspace test-target compilation.  Local runtime tests are not run.

## Limits

This is source plus CI-fixture evidence only.  It does not claim a remote SIEM, live provider
effect correctness, durable cross-host explain latency or physical secret-store proof.  Later
steps own non-blocking telemetry, migration and final crash/product gates.
