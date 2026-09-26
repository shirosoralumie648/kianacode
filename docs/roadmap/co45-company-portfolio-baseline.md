# CO-45 Company portfolio capacity and cost baseline

## Scope

CO-45 adds an organization/project capacity ledger with explicit Reserved, Spent, Released and
Unknown states. Reservations bind organization, project, run, priority and authority epoch; unknown
cost holds capacity until reconciliation. A missing project/organization budget or a new identity
cannot bypass an existing reservation.

## Implemented source slice

- `CompanyPortfolioLedger` enforces organization and project limits, fair shared capacity and
  idempotent reservation facts.
- Unknown reservations remain held; Released capacity becomes available again, while settlement
  state conflicts reject duplicate/changed facts.
- Existing billing/quota contracts remain lower-level cost and provider authorities; this wrapper
  does not turn runtime tokens into business value or financial settlement.

## CI-only evidence

`.github/workflows/co45-company-portfolio.yml` runs formatting, two-project fairness/unknown/budget
fixtures, the Core billing boundary guard and target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- No scheduler or real capacity worker is started; ledger durability and portfolio/read-model
  integration remain open.
- This does not claim financial billing, provider invoices or live cost outcomes.
