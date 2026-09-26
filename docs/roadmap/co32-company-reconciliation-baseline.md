# CO-32 Company risk, incident and Unknown reconciliation baseline

## Scope

CO-32 separates a risk trigger from a named Company incident and binds an append-only
reconciliation case to the exact project, packet, run or delivery, account, scope, baseline,
authority epoch and original Unknown digest. Independent query, manual evidence and stop receipts
can produce a successor fact; a model/status claim, an Unknown observation, an unconfirmed stop or
evidence for another object is rejected.

## Implemented source slice

- `CompanyRiskTrigger` is a signal only; `CompanyIncident` adds an owner, deadline, recovery plan
  reference and an explicit no-automatic-retry policy.
- `CompanyEffectObservation` supports run and delivery observations and preserves target/account/
  scope identity, baseline and epoch, original Unknown digest, source and evidence references.
- `CompanyReconciliationCase` has Pending → EvidenceAttached → Reconciled/Failed successors;
  successful independent evidence clears dependent blocking, confirmed failure keeps dependents
  blocked, and neither path rewrites the original Run/Delivery result.
- `CompanyReconciliationLedger` keeps trigger, incident and case histories idempotently; existing
  RecoveryPlan, RunCancellationFact and EffectObservation remain the lower-level authorities.

## CI-only evidence

`.github/workflows/co32-company-reconciliation.yml` runs formatting, Company risk/incident and
Unknown fixtures, the Core single-spine guard and target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The Company ledger is a typed append-only source contract; durable EventLog projection and full
  adapter consumption of every run/delivery observation remain open.
- It does not query providers, retry effects, close incidents automatically or change the original
  Unknown Run/Delivery state. CO-33+ own delivery and closeout projection; live/physical outcomes
  are not claimed.
