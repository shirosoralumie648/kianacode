# CO-27 milestone-local acceptance baseline

## Scope

CO-27 adds a milestone-local acceptance contract to stop project-wide acceptance from waiting on
future milestone runs. `MilestoneAcceptanceRequest` fixes the milestone/project version, required
packet IDs, accepted packet acceptance digests, milestone criteria and evidence refs. An Accept
requires every packet declared for this milestone; foreign packet/evidence refs fail closed.

The ledger records the current milestone decision without traversing or mutating dependent
milestones. A first milestone can be accepted while a dependent milestone has no Run; later ready
projection can use the accepted milestone fact to unlock only its declared dependents.

## Implemented source slice

- Typed Accept/Reject request and idempotent `MilestoneAcceptanceLedger`.
- Required packet completeness, packet acceptance digest, criteria/evidence, foreign evidence and
  duplicate-digest fences.
- Core adapter and source guard reference existing Company Milestone lifecycle/AcceptanceTarget
  and readiness predicate without adding a second scheduler or rewriting all milestones.
- GitHub-only fixtures cover missing packet, foreign evidence, first-stage acceptance before M2,
  rejection and serialization-compatible ledger state.

## CI-only evidence

`.github/workflows/co27-milestone-acceptance.yml` runs formatting, the domain milestone fixtures,
the Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The ledger is a source contract and is not yet the durable Company acceptance projector or ready
  query input. Existing project-wide RequestAcceptance remains compatibility behavior.
- Cross-process acceptance persistence, semantic criteria evaluation, delivery and live/physical
  outcomes remain open; dependent milestone execution is not started by this contract.
