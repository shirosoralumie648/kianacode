# CO-14 symposium, blackboard and decision governance baseline

## Scope

CO-14 adds a governance-bound domain ledger around the existing `Symposium` and
`DecisionRecord` contracts. It binds a meeting to Project/baseline/agenda, named assignments,
chair, round limits and output schema; contributions are typed Claim/Vote/Draft records with
visibility and evidence; decisions retain alternatives, dissent, unresolved items and explicit
Sponsor approval identity.

## Implemented source slice

- `SymposiumGovernance` binds Project, baseline version, agenda reference, chair assignment,
  attendee assignment map, max rounds, output schema and canonical digest.
- `SymposiumBoard` rejects uninvited, stale, duplicate and unauthorized private contributions;
  contribution visibility is explicit and the board never treats vote count as approval.
- `SymposiumDecision` requires chair authorship, evidence and preserves alternatives/dissent/
  unresolved context. Sponsor approval is a separate explicit attachment requiring a named sponsor
  assignment, so a meeting decision cannot impersonate the human gate.
- Existing `Symposium`/`DecisionRecord` and the current ControlPlane/Harness meeting path remain
  available; no second meeting runner or effect loop was added.

## CI-only evidence

`.github/workflows/co14-symposium-governance.yml` runs formatting, the domain governance fixtures,
the Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local
Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The governance board is a pure contract and is not yet the sole persistence/projector for the
  existing live symposium path; durable cross-process replay and UI/private-brief projection remain
  later work.
- Assignment authority, ProjectTrust, approval consumption and model session scheduling remain
  ControlPlane/Harness responsibilities. No live model, source-code write, external effect,
durable, live or physical outcome is claimed.
