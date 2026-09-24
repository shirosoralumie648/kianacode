# CP-23 Company transactions baseline

## Scope

Company mutations use the existing `ControlPlane` transaction boundary.  The
server derives `CompanyProof` and any `HumanDecision` from the authenticated
`RequestContext` and current Company revision, validates the decision binding,
transitions `CompanyState`, and commits the Company event through the protected
EventStore path.  A `CompanyCommandReceipt` is created only after that commit.

The decision binding covers actor, actor kind, role, session, command event
name and digest, expected revision, authority scope digest, and expiry.  A
caller cannot supply a proof, receipt, or model conclusion in the strict
`CompanyCommandRequest` payload.  `TransitionBatch` includes both the Company
aggregate and the authority read-set, so stale authority or Company revisions
cannot silently authorize a mutation.

## GitHub evidence

`cp23_company_policy.rs` exercises the accepted server decision and rejects
actor, role, session, command digest, revision, scope, expiry, missing-decision,
and unexpected-decision cases.  `cp23_company_transaction_guard.rs` checks the
source ordering and protected commit path, the request DTO boundary, and the
absence of a Company-specific Broker/model execution loop.

`.github/workflows/cp23-company-transactions.yml` runs these fixtures and
workspace test-target compilation on GitHub Actions.  Local runtime tests and
build commands are intentionally not part of this step.

## Limits

This is source plus remote-fixture wiring evidence.  It does not claim a
cross-process durable Company transaction, physical power-loss recovery, real
external business effect, live provider effect, or physical proof.  GitHub CI
results are not observed by the implementation branch; later closeout work must
promote proof only from recorded CI evidence.

