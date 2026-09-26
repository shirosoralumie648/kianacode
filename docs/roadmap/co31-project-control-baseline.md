# CO-31 project pause, resume and cancellation propagation baseline

## Scope

CO-31 adds an append-only project control contract shared by the Company command projection and
the existing ControlPlane cancellation, dispatch intent and cell fencing paths. A plan names the
project, baseline and authority epoch, target status, affected packet/run/department identities and
the observed stop state for each child. Pause blocks new dispatch; a cancellation request is not a
terminal result; cancellation can become `Cancelled` only after every started child is `Stopped` or
`NotStarted`, otherwise it remains `ResultUnknown` and requires reconciliation.

## Implemented source slice

- `ProjectControlPlan` covers Pause, Resume, CancelRequest and CancelConfirm across the project
  lifecycle states used by CompanyOS.
- Child records repeat project and department identity and reject cross-project propagation,
  duplicate packet/run/dispatch identities, stale epochs and malformed stop observations.
- Resume requires an explicit approval reference and rejects Unknown or still stopping children;
  the ledger is idempotent and retains historical plans without executing effects.
- `CompanyState` exposes the typed plan ledger while existing `CompanyCommand` and ControlPlane
  cancellation paths remain the effect authority.

## CI-only evidence

`.github/workflows/co31-project-control.yml` runs formatting, the domain propagation fixtures, the
Core single-spine source guard and target compilation on GitHub Actions. Local Cargo tests, builds,
checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The ledger is a typed source contract; durable EventLog projection and full runtime consumption
  of every child stop observation remain open.
- Actual process stop, late-result fencing, cell retirement and Unknown reconciliation still need
  the existing ControlPlane paths and the CO-32 incident/reconciliation workflow.
- No live, external, or physical cancellation outcome is inferred from this source slice.
