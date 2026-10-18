# CO-11 typed criterion coverage baseline

## Scope

CO-11 adds a versioned, typed coverage graph around the existing CO-06 `Criterion` contract.
Project requirements can be explicitly covered/refined by Milestone and Packet criteria, and a
specific typed artifact/event/test reference can verify a criterion. Stable IDs and origin
versions remain distinct even when descriptions are identical. The frozen, Sponsor-approved graph
replaces the legacy text-set inference for every later acceptance snapshot.

## Implemented source slice

- `kiana-domain/src/criteria.rs` defines `CriterionCoverageGraph`, typed relation links
  (`covers`, `refines`, `verifies`), scope/baseline binding, deterministic digest, duplicate/
  missing-parent/cross-level/weak-child/cycle/unknown-evidence rejection, explicit pending
  required criteria, and deterministic trace/gap accessors.
- CO-11 completion: `CriterionCoverageApproval`
  (`kiana.criteria-coverage-approval.v1`) binds the exact graph digest, the approved plan digest
  and the planning/review decision record; only the `sponsor` role can sign, and the decision
  reference has to be a typed `artifact:`/`event:` record. `FrozenCriterionCoverage`
  (`kiana.frozen-criteria-coverage.v1`) freezes graph + approval and rejects graph/approval
  digest drift, project drift and baseline drift.
- `FrozenCriterionCoverage::bind_plan` machine-verifies typed Plan coverage: project, Charter
  baseline, `coverage_digest` and the approved plan digest must match, every `PlanNode`
  `criterion_refs` entry must resolve to a frozen identity, and a required Milestone criterion
  that no plan node is responsible for is rejected (`plan_coverage_required_criterion_unassigned`).
- `resolve_origin` locates a criterion by stable ID plus origin baseline version
  (`criteria_origin_baseline_mismatch`), so a caller that only knows the text cannot silently bind
  a changed criterion. `criteria_ids`/`frozen_criteria` keep same-text/different-origin criteria
  as separate identities.
- A single generic verification statement cannot cover every criterion: one evidence reference may
  verify at most one parent criterion (`criteria_evidence_ref_reused`) and one evidence criterion
  may be the verification target of at most one parent (`criteria_evidence_reused`).
- `CompanyCommand::FreezeCriterionCoverage` records the Sponsor approval on the single Company
  aggregate (`CriterionCoverageFrozen`, `CompanyState::criterion_coverages`). The freeze requires
  the server-side frozen Charter baseline (`criteria_coverage_baseline_missing`/
  `criteria_coverage_baseline_mismatch`), this project's recorded Sponsor
  (`criteria_coverage_sponsor_mismatch`), a verified decision record (`company_evidence_unverified`),
  and is idempotent for the same graph while a different graph is denied
  (`criteria_coverage_already_frozen`). `CompanyState::coverage_freeze_verdict` is the shared
  verdict; the ControlPlane revalidates it before the transition.
- `CompanyState::acceptance_snapshot` (used by `RequestAcceptance`) reads typed criterion
  identities from the frozen coverage and records `coverage_digest`/`coverage_baseline_version`.
  A coverage-bound `CriteriaSnapshot` fails closed
  (`criteria_snapshot_coverage_requires_typed_criteria`) and `criteria()` never falls back to the
  text lists.
- The graph reuses CO-06 `Criterion` and `CriterionId`; it does not create another criterion
  authority, EventStore, planner, evaluator or execution loop.
- `CriteriaSnapshot::criteria()` uses typed `criterion_refs` IDs whenever they are present and
  only falls back to legacy text lists for projects without a frozen coverage. Old replay payloads
  remain readable while text deduplication cannot merge distinct typed criteria.
- The Core source guard and GitHub workflow cover the deny-first fixtures and target compilation.

## Acceptance fixtures

Deny-first fixtures all live in `kiana-domain/tests/co11_criteria_coverage.rs` (success last):

- `criteria_coverage_rejects_missing_parent_duplicate_identity_and_silent_weakening` (card)
- `criteria_coverage_rejects_reused_generic_evidence_and_merged_text_identities`
- `typed_plan_coverage_binds_the_approved_graph_and_rejects_drift`
- `acceptance_snapshot_uses_typed_criteria_and_reports_remaining_gaps`
- `coverage_freeze_command_requires_sponsor_baseline_and_verified_decision`
- `criterion_trace_links_project_requirement_to_specific_test_evidence` (card, success)

## CI-only evidence

`.github/workflows/co11-criteria-coverage.yml` runs locked fetch, formatting, the domain coverage
fixtures, the Core source guard and domain/core test-target compilation on GitHub Actions.
Automatic lanes keep running `kiana-domain-s2/4` (`co11_criteria_coverage`) and
`kiana-core-s2/6` (`co11_criteria_coverage_guard`) from the unified `ci.yml`. Local Cargo tests,
builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: implemented
proof_level: source
```

## Limitations and handoff

- The business (`CompanyBusinessAction::PublishPlan`) adapter keeps its legacy
  `BusinessCriterion`/text contract in this slice; migrating every historical plan to the typed
  graph remains later work. A project without a frozen coverage keeps the readable text-list
  snapshot path for replay compatibility.
- A `verifies` link carries bounded typed reference strings, not a live test runner or artifact
  blob lookup; external evidence truth, durable cross-process recovery and semantic sufficiency
  still require independent planning/review evidence.
- No live model, source-code write, external effect, durable, live or physical proof is claimed.
