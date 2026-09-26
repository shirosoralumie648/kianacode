# CO-38 Company rebuildable read model baseline

## Scope

CO-38 adds a scope-bound, deterministic project read model derived from one `CompanyState`
snapshot. The DTO carries source/projection cursors, revision, authority epoch, project/packet
views, blockers with owner and allowed read-side actions, and evidence links. It never advances a
project from chat or client progress and never becomes a second fact source.

## Implemented source slice

- `CompanyReadModelSnapshot` rejects foreign project scope, invalid cursor/revision/epoch and
  digest/freshness drift; cursor lag is visible as Pending/Unknown rather than hidden.
- `CompanyProjectView` and `CompanyPacketView` expose runtime/packet status, acceptance/delivery/
  close references, result-unknown or missing-run blockers, responsible role/sponsor and explicit
  inspect/reconcile/start actions, plus evidence links.
- Core delegates to the pure domain projector; existing CompanyGovernanceSnapshot remains the
  compatibility cross-entry projection and CompanyState/EventLog remain authoritative.

## CI-only evidence

`.github/workflows/co38-company-read-model.yml` runs formatting, scope/cursor/rebuild fixtures, the
Core read-only source guard and target compilation on GitHub Actions. Local Cargo tests, builds,
checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This is a deterministic in-memory DTO projection; durable cross-process snapshots, pagination,
  daemon query wiring and four-entrypoint rendering remain CO-39+ work.
- Evidence links are references only; the projection does not claim business outcome or live/physical
  effect success.
