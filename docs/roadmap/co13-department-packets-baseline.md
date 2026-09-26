# CO-13 versioned department packet baseline

## Scope

CO-13 adds a separate versioned contract for analysis, assessment, Charter/Plan proposal,
implementation, verification, delivery preparation and knowledge capture work. The existing v1
`WorkPacket` remains the Builder execution adapter and keeps its `packet_role_must_be_builder`
semantics.

## Implemented source slice

- `DepartmentPacket` and `ResultContract` carry a versioned kind, responsible department/role,
  assignment reference, typed Intake/Initiative/Charter/Plan basis, bounded input refs, output
  schema and controlled write scope.
- Analysis/planning/verification packets can be proposed without an approved Plan where their
  input basis permits it; implementation packets require a Plan reference and a non-empty write
  set. Paths are normalized and restricted to the kind's controlled prefixes.
- Explicit `runtime_grant` and `budget_lease` compatibility fields are rejected, so a business
  packet cannot smuggle runtime authority. Actual Grant/Lease derivation remains in ControlPlane.
- The old Builder packet is unchanged and is covered alongside the new contract by CI fixtures and
  a Core source guard.

## CI-only evidence

`.github/workflows/co13-department-packets.yml` runs formatting, the domain packet fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The new contract is a domain boundary; it is not yet the sole Company/ControlPlane admission
  DTO and does not dispatch a role session or write a proposal artifact.
- Assignment validity, ProjectTrust, Plan approval, broker capability intersection and durable
  packet persistence remain server-side follow-up work. No Grant/Lease or live effect is claimed.
