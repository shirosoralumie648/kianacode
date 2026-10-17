# CO-09 Objective and Initiative intake baseline

## Scope

CO-09 strengthens the existing Company command and replay path for Objective and Initiative
intake. Objective activation requires a metric measurement method, a finite improving target, a
forward time window and an owner decision (including organization ownership when the organization
is registered). Initiative submission requires the actor to be its sponsor and all referenced
objectives to belong to the same organization; approval requires active objectives, the declared
information-gap assessment and the recorded sponsor decision; conversion preserves objective
ancestry and is idempotent. A project can also be created directly, but only with an explicit
record of the skipped Initiative lane that keeps the objective ancestry.

## Implemented source slice

- `Objective` carries an optional legacy compatible `measurement_method`; approval requires a
  non-empty method, finite values, a non-reversing target direction and a valid time window. A
  registered `BusinessOrganization` may only be activated by the principal that owns it
  (`objective_organization_owner_mismatch`); unregistered organizations keep the legacy path.
- `Initiative` carries an optional `information_gaps` assessment (`None` = never assessed,
  bounded, duplicate-free). Approval requires an explicit assessment, including an explicit
  "no known gaps" empty list, plus the recorded sponsor decision.
- `SubmitInitiative` rejects sponsor mismatch and cross-organization objective references.
  `AdvanceInitiative` requires active same-organization objectives for approval and conversion,
  refuses conversion without a recorded sponsor decision
  (`initiative_decision_required`), refuses converting a non-approved Initiative
  (`initiative_conversion_requires_approval`), repeats the original conversion result instead of
  minting a second link (`initiative_already_converted`) and keeps one project bound to one
  Initiative (`initiative_project_already_linked`).
- `CompanyCommand::RecordDirectIntake { project_id, reason }` records the explicit skip entry
  (`DirectProjectIntake`) with the organization and objective-ref ancestry snapshot. It refuses a
  blank/oversized reason, a project outside `Proposed`/`Chartering`, a project without objective
  refs, a project already owned by a converted Initiative and a conflicting repeat; an identical
  repeat returns the original entry.
- `CompanyState::project_intake_provenance(project_id)` projects either the converted Initiative or
  the direct-intake record and refuses `project_intake_provenance_required` /
  `project_intake_objective_ancestry_mismatch`. The ControlPlane command boundary consults it for
  `ApproveProject`, so a fresh go/no-go cannot be submitted without intake provenance. Legacy
  events still replay through the domain transition, and the versioned business `ApproveCharter`
  path keeps its own organization/assignment authority and is deliberately not gated here.
- The existing `ControlPlane::handle_company_command` remains the Company entrypoint, with the
  policy and idempotency gates in front of the domain transition.

## 2026-10-17 completion slice (CI-only)

Acceptance fixtures (`kiana-domain/tests/co09_objective_initiative.rs`, deny first):

| Fixture | Proves |
|---|---|
| `objective_approval_rejects_nan_inverted_window_wrong_owner_and_organization` | NaN, flat/reversed targets, inverted/zero windows, missing measurement method, foreign owner and foreign organization owner all refuse activation |
| `initiative_submission_rejects_foreign_sponsor_and_objectives` | A non-sponsor submission and a cross-organization objective reference stay in intake |
| `initiative_cannot_convert_without_approved_objective_and_sponsor_decision` | Conversion refuses inactive objectives, a missing recorded decision, a foreign project ancestry and a non-sponsor actor; rejected/withdrawn Initiatives keep their decision and ancestry and can never convert |
| `initiative_intake_records_hypothesis_value_cost_risk_and_gap_assessment` | Hypothesis/value/cost/risk are required, gaps are bounded and duplicate-free, and approval needs an explicit assessment |
| `approved_initiative_converts_once_and_keeps_objective_ancestry` | The approved Initiative converts once, a repeat returns the original result, a different project is refused, one project keeps one Initiative, and the ancestry survives go/no-go |
| `direct_project_intake_records_the_skip_reason_and_keeps_objective_ancestry` | A direct project needs the explicit skip record with its objective ancestry; repeats are idempotent, conflicts/foreign owners/late records are refused, and the record survives approval |

Guard: `kiana-core/tests/co09_company_intake_guard.rs` keeps the intake on the ControlPlane path
and binds the refusal-code matrix, fixture names/order, the ControlPlane approval gate, the manual
lane and the automatic shard book.

CI wiring: manual `.github/workflows/co09-company-intake.yml` (`workflow_dispatch`-only,
`permissions: contents: read`, `cargo fetch --locked`, format gate, both targets with
`--test-threads=1` and both `--no-run` compiles) plus the existing unified `ci.yml` shards
`kiana-domain-s2/4` (`co09_objective_initiative`) and `kiana-core-s2/6`
(`co09_company_intake_guard`); `scripts/ci/test-shards.json` is unchanged. The evidence chain for
this card is this baseline (`docs/roadmap/co09-company-intake-baseline.md`), the two registered
targets and the manual lane above.

## CI-only evidence

GitHub Actions runs the CO-09 domain fixtures, the Company source guard, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The deny and success paths are covered in source and wired into CI, but no local or CI receipt
was awaited. This slice does not create a new durable Objective/Initiative store or the project
Charter/go/no-go flow (CO-10). Organization identity and sponsor authority for the legacy path are
still supplied by the existing ControlPlane context; the business `ApproveCharter` path keeps its
own organization/assignment authority and is not covered by the intake gate. Live business outcome
measurement, cross-process durability and later Company steps remain open.
