# CP-29 ControlPlane authority and crash-matrix baseline

## Scope

CP-29 adds a deterministic evidence contract for the state-machine, concurrency and crash
properties listed in ControlPlane §14.7. `Cp29CommandFact` checks monotonic authority epochs,
permission narrowing, budget reservation/settlement bounds, legal lifecycle transitions, terminal
non-revival and same-command replay without a second effect. `Cp29CrashObservation` keeps an
unconfirmed started effect in `ResultUnknown` with a resource fence. `Cp29AuthorityScenario` checks
continuous sequence, payload-drift rejection, duplicate command idempotency, complete crash-point
coverage and equality of online and replay fold digests.

The domain contract is a validator over committed evidence. The Core adapter does not inject a
crash, start or kill a process, call a Broker/Runner/provider, or create a second authority loop;
the existing TransitionBatch/EventStore/ControlPlane remain the owners of runtime facts.

## Implemented source slice

- `kiana-domain/src/control_plane_authority.rs` defines strict state, command-fact and crash
  observation schemas with canonical digests and fail-closed invariant checks.
- `kiana-core/src/control_plane_authority.rs` exposes a read-only validator facade and source guard
  ties the contract to existing journal/EventStore boundaries.
- CI fixtures cover adversarial permission/budget/terminal/replay/payload cases, seven crash points,
  Unknown fencing, duplicate-effect denial and online/replay digest drift.

## CI-only evidence

`.github/workflows/cp29-control-plane-authority.yml` runs formatting, the domain authority matrix
fixture, the Core boundary guard and affected test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `adversarial_command_sequence_preserves_authority_budget_terminal_and_replay_invariants` | permission widening, budget overrun, terminal revival and payload drift are rejected; replay has zero effect |
| `crash_matrix_keeps_unconfirmed_effect_unknown_and_fenced` | every CP-29 crash point is represented; started/unconfirmed effects remain Unknown and fenced |
| `online_projection_must_match_complete_deterministic_replay_fold` | replay digest drift cannot be reported as equivalent state |

## Limitations and handoff

- The validator does not execute the ControlPlane, provide a real two-host race, kill/restart a
  subprocess, measure storage latency or inspect physical files/processes. Those runtime and durable
  claims remain open for CP-30/ER/PD/DEP evidence.
- The fixture's fold digest is a contract-level comparison, not proof that every production
  projector or adapter has been exercised.
- CI results are intentionally not awaited; proof level remains `source`.
