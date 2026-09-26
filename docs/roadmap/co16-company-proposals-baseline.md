# CO-16 Company proposal and Plan approval baseline

## Scope

CO-16 adds structured, inert Company Plan proposal and approval facts. A proposal references a
validated Plan graph, coverage digest, source assignment/session, input artifacts and a bounded
ResultContract. A separate Sponsor approval binds the exact proposal digest, decision reference,
expected Company revision and approver assignment. The ledger rejects stale/forged/duplicate
approvals without mutating the proposal.

## Implemented source slice

- `CompanyPlanProposal` contains no command-text interpreter, Grant or Lease and validates the
  embedded `PlanProposal`/`ResultContract` and project identity.
- `CompanyPlanApproval` binds proposal/project/digest/revision/decision/approver identity and
  requires the Sponsor role. `CompanyProposalLedger` has deny-first submit/approve/duplicate
  behavior and exposes only the exact approved plan graph.
- This is a domain fact contract; it does not add a second EventStore, scheduler, Broker or
  execution loop. Existing Company CAS/receipt paths remain the integration authority.

## CI-only evidence

`.github/workflows/co16-company-proposals.yml` runs formatting, the domain proposal fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- ControlPlane protocol DTOs and CompanyEvent materialization of this new proposal ledger are
  follow-up integration work; current ledger is in-memory domain evidence only.
- Durable CAS/restart recovery, real artifact/coverage reads, role assignment resolution and live
  business effects remain unproven. No source-code write or external outcome is claimed.
