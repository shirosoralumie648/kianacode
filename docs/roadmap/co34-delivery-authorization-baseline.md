# CO-34 delivery authorization, effect and recipient confirmation baseline

## Scope

CO-34 binds delivery authorization, one-shot dispatch, Broker effect observation and recipient
confirmation to the exact CO-33 manifest and local package. Approval has an expiry; dispatch is a
single intent; a Broker receipt is required before `Delivered` or `Failed`; an `Unknown` receipt
puts the intent into reconciliation and prevents a repeat dispatch. Only the named recipient can
confirm the exact package with a human confirmation.

## Implemented source slice

- `DeliveryAuthorization` freezes manifest/destination/recipient/project/baseline and an expiry;
  stale or changed authorization is rejected.
- `DeliveryDispatchIntent` is one-shot and idempotent. `DeliveryEffectReceipt` accepts only the
  Broker source and exact package/manifest binding; sender claims cannot become effect evidence.
- Unknown dispatches are fenced. `DeliveryRecipientConfirmation` requires the Broker Delivered
  receipt, exact recipient/package/manifest, and human confirmation; duplicate confirmation is
  idempotent.
- `DeliveryAuthorizationLedger` preserves authorization, dispatch, receipt and confirmation
  facts in the CompanyState projection; no external sender or provider is called here.

## CI-only evidence

`.github/workflows/co34-delivery-authorization.yml` runs formatting, delivery authorization and
confirmation fixtures, the Core Broker/recipient guard and target compilation on GitHub Actions.
Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The ledger does not dispatch a package, contact an external channel, or prove a recipient saw the
  package; it only records validated source facts.
- Unknown effects remain fenced for CO-32 reconciliation. Durable EventLog projection and complete
  HumanTask/notification wiring remain open; no live or physical delivery outcome is claimed.
