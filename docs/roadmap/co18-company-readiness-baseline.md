# CO-18 Company readiness and explainable blockers baseline

## Scope

CO-18 adds a read-only Company readiness projection over the existing deterministic DAG/status
predicate. `CompanyReadinessContext` supplies the facts that a legacy WorkPacket cannot infer:
handoff ACK state, target assignment status, declared dependency outcome kind, input versions,
project pause and pending change gates, plus server-derived business blockers.

The projection returns sorted `ready` packet IDs and structured `ReadinessBlocker` entries. A
`RequiresAcceptance` dependency is not satisfied by `Succeeded`; failed/unknown/missing outcomes,
stale inputs, pending ACK, expired/revoked assignments, paused projects and pending changes all
remain blocked. The projection is pure and does not acquire a claim, lease, budget or write scope.

## Implemented source slice

- `company_ready_packets` delegates the graph/cycle/status portion to the canonical domain
  `ready_packets` predicate, then applies typed Company gates in deterministic order.
- `CompanyState::project_company_readiness` builds the snapshot from project status, packet
  approval, legacy and CO-17 handoff projections, and existing business blockers.
- `ControlPlane::company_snapshot` now serializes this one Company readiness projection instead
  of re-filtering ready packets in the view layer. Generic workflow queue and legacy task adapters
  continue to call the same underlying domain predicate.
- GitHub-only fixtures cover required outcome type, pending/expired handoff and assignment,
  input-version mismatch, pause/change blockers, deterministic digest/order and no mutation.

## CI-only evidence

`.github/workflows/co18-company-readiness.yml` runs formatting, the domain readiness fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- Typed Plan edge requirements and external artifact/input version lookup are context inputs; the
  Company event reducer does not yet materialize them as a single durable admission fact.
- The existing claim/start commands still retain their compatibility checks while CO-19 handles
  atomic claims and lease fencing. The readiness query itself never mutates state.
- Cross-process query transport, real budget/assignment authority and live/physical business
  outcomes remain unproven.
