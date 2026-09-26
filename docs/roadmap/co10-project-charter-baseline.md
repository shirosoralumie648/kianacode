# CO-10 Charter and project go/no-go baseline

## Scope

CO-10 closes the project Charter admission boundary on the existing Company command and
ControlPlane path. A project cannot be approved until it is chartering, its referenced
objectives are still active in the same organization, a validated project budget is present,
and the Sponsor identity matches the proposal. A registered Charter is checked as an immutable
content and optional typed-version identity; a proposal-time identity digest rejects later
Charter drift. Rejection remains a queryable terminal decision and does not require a budget.

A successful approval records a `ProjectCharterBaseline` containing the Charter, scope,
criteria, non-goals, risk and budget digests. Later project state revisions remain separate from
that frozen baseline snapshot.

## Implemented source slice

- `CompanyState` records proposal-time `charter_digests` and approval-time
  `charter_baselines`; legacy states without those optional maps remain readable.
- `ApproveProject` has deny-first checks for Chartering status, existing budget, Sponsor and
  active objective ancestry, non-empty registered Charter, content/typed-version drift and
  evidence references. `RejectProject` preserves the failure exit after evidence validation.
- `ConfigureBudget` is admitted only before approval and remains single-use. The existing
  `CompanyBusinessAction::ApproveCharter` now enters Chartering when needed, configures budget,
  then uses the same `ApproveProject` handler; no second execution loop or store was added.
- `set_project` continues to advance the ordinary project state revision; the separate
  `ProjectCharterBaseline.version` stays at one for the approved Charter.

## CI-only evidence

`.github/workflows/co10-project-charter.yml` runs the CO-10 domain fixtures, the Core source
boundary guard, formatting and workspace test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The legacy `CompanyArtifact` text snapshot remains the compatibility store; durable blob
  retention, cross-process recovery and typed Charter artifact persistence remain open.
- A legacy project with no proposal digest can only be checked for a present non-empty Charter;
  future migrations must backfill the typed identity before claiming full versioned coverage.
- `ProjectBudget.project_id` is retained as the existing typed budget contract while the Company
  project key and `project_budget_ref` remain legacy string fields; a later migration must make
  that binding explicit across all historical projects.
- No live model, external budget system, source-code write, durable cross-process or physical
  business outcome is claimed.
