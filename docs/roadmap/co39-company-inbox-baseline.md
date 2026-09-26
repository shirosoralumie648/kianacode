# CO-39 unified Company Human Inbox baseline

## Scope

CO-39 adds one versioned decision-card contract for Charter, Change, RuntimeApproval, Acceptance,
Delivery and Incident follow-up. Each card binds the exact target kind/id/revision/digest, scope,
decider, options, evidence and expiry. The card is a projection and decision-consumption fence; the
referenced command still returns to ControlPlane for fresh authorization.

## Implemented source slice

- `CompanyInboxCard` rejects stale target/scope, wrong decider, invalid option, expired card and
  terminal double consumption; decisions preserve a decision reference and decider identity.
- `CompanyInboxLedger` is idempotent for publication and offers deterministic pending ordering by
  due time/creation/card ID; it never auto-approves, retries or closes an operation.
- `CompanyState` exposes the unified card ledger while existing HumanTask, ApprovalBinding,
  HumanInboxItem and notification projections remain their respective authorities.

## CI-only evidence

`.github/workflows/co39-company-inbox.yml` runs formatting, stale/decider/expiry/double-consumption
fixtures, the Core HumanTask/ControlPlane boundary guard and target compilation on GitHub Actions.
Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This ledger does not persist HumanTask decisions to EventLog or execute command actions; it is a
  typed projection/fence only.
- Cross-process inbox hydration, client parity, notification delivery and durable task recovery
  remain CO-40+ work.
