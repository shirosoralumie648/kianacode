# CO-30 baseline change impact and publication baseline

## Scope

CO-30 adds a complete baseline change contract. `ChangeImpact` lists affected milestones, packets,
runs, reviews, acceptances and deliveries, plus budget/schedule references and old/new baseline
versions. `BaselinePublication` must carry the complete new charter/plan/packet/criteria/budget/
schedule set and explicit invalidated references.

`ChangePublicationLedger` rejects stale versions, duplicate drift and partial publication while
retaining historical impact decisions. Existing Company `DecideChange` and business baseline
guards remain compatibility command paths; the new contract prevents a caller from treating a
scope string update as a complete graph publication.

## Implemented source slice

- Typed impact and publication digests bind all affected graph/approval/delivery references.
- Old baseline version must advance exactly through the published new version; invalidated refs
  cannot be omitted or duplicated.
- Idempotent ledger preserves previous changes and rejects version conflicts/partial writes.
- GitHub-only fixtures cover full impact set, idempotent publication, stale baseline and partial
  publication rejection.

## CI-only evidence

`.github/workflows/co30-change-contract.yml` runs formatting, the domain change fixtures, the Core
source guard and domain/core test-target compilation on GitHub Actions. Local Cargo tests, builds,
checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The ledger is a typed source contract; durable EventLog CAS publication and full Project/Milestone/
  Packet projector replacement remain open.
- Running attempts, external approvals, semantic change evaluation and live/physical outcomes are
  not inferred from a publication record; CO-31 owns pause/cancel fencing next.
