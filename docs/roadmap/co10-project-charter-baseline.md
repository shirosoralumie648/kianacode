# CO-10 Charter and project go/no-go baseline

## Scope

CO-10 closes the project Charter admission boundary on the existing Company command and
ControlPlane path. A project cannot be approved until it is chartering, its referenced
objectives are still active in the same organization, a validated project budget is present,
and the Sponsor identity matches the proposal. A registered Charter is checked as an immutable
content and optional typed-version identity; a proposal-time identity digest rejects later
Charter drift. Rejection remains a queryable terminal decision and does not require a budget.

A successful approval records a `ProjectCharterBaseline` containing the Charter, scope,
criteria, non-goals, risk and budget digests. Later project state revisions remain separate from
that frozen baseline snapshot.

## Implemented source slice

- `CompanyState` records proposal-time `charter_digests` and approval-time
  `charter_baselines`; legacy states without those optional maps remain readable.
- `ApproveProject` has deny-first checks for Chartering status, existing budget, Sponsor and
  active objective ancestry, non-empty registered Charter, content/typed-version drift and
  evidence references. `RejectProject` preserves the failure exit after evidence validation.
- `ConfigureBudget` is admitted only before approval and remains single-use. The existing
  `CompanyBusinessAction::ApproveCharter` now enters Chartering when needed, configures budget,
  then uses the same `ApproveProject` handler; no second execution loop or store was added.
- `set_project` continues to advance the ordinary project state revision; the separate
  `ProjectCharterBaseline.version` stays at one for the approved Charter.

## 2026-10-17 completion slice (CI-only)

The card's remaining ordering items and its two named acceptance fixtures are now covered at the
source level and wired into CI. Refusals stay in front of the success fixture.

- **Versioned Charter draft (card item ①/②).** `kiana-domain/src/company.rs` adds
  `ProjectCharterDraft` and `CharterStakeholder` under the new
  `kiana.project-charter-draft.v1` schema, with bounded free-text/list/stakeholder budgets
  (`MAX_CHARTER_DRAFT_TEXT_BYTES`/`MAX_CHARTER_DRAFT_LIST_ITEMS`/`MAX_CHARTER_DRAFT_STAKEHOLDERS`).
  The draft carries scope, non-goals, stakeholders (at least one delivery recipient), success
  criteria, risk summary, the project's budget reference, the authenticated drafter, the
  drafting time and version 1. `CompanyCommand::DraftCharter` (event `ProjectCharterDrafted`,
  Project mutation class, `sponsor`/`pm` roles) records it once per project: an identical
  re-submission is idempotent, different content is refused (`project_charter_draft_exists`),
  and a draft cannot widen the proposed project
  (`project_charter_draft_project_mismatch`), borrow another budget reference
  (`project_charter_draft_budget_ref_mismatch`), forge the drafter or be future-dated
  (`project_charter_draft_author_mismatch`), or be recorded by a Sponsor-role actor that is not
  this project's sponsor (`project_charter_draft_role_denied`).
- **Sponsor approves the exact draft version.** `StartChartering` freezes the draft identity in
  `CompanyState.charter_draft_digests`; `ApproveProject` re-reads the recorded draft, validates
  it and requires the frozen digest (`project_charter_draft_changed`,
  `project_charter_draft_missing`). The frozen `ProjectCharterBaseline` now records
  `charter_draft_digest`, `charter_draft_version` and `stakeholder_principals` with
  `#[serde(default)]`, so legacy approvals without a draft stay readable.
- **Typed business path.** `CompanyBusinessAction::ApproveCharter` records the draft it approves
  (built from its typed criteria/stakeholders and the project fields) through the same
  `CompanyCommand::DraftCharter` apply path when the project has no draft yet, so the versioned
  path always binds an explicit draft identity instead of only the compatibility `Project`
  fields.
- **Frozen baseline stays frozen.** The card-named success fixture proves that a later
  `CreateMilestone` advances `Project.version` (3 → 4) while
  `ProjectCharterBaseline.version` stays 1 with an unchanged draft digest, and that a rejected
  project remains queryable (`ProjectStatus::Rejected` + `decision_ref`) with every write path
  closed (`company_project_not_approved_or_active`). The runtime side stays closed too:
  `company_approved_packet_required`, `company_run_not_authorized` and
  `company_capability_scope_inactive` still require an approved, active project bound to a
  reserved packet before any capability reaches a Broker.
- **Fixtures.** `kiana-domain/tests/co10_project_charter.rs` 3 → 6 fixtures, refusals first:
  the retained three, the draft refusal fixture
  `project_charter_draft_rejects_role_drift_and_stakeholder_gaps`, the card-named refusal
  fixture `project_approval_rejects_missing_budget_changed_charter_and_inactive_objective`
  (non-empty `project_budget_ref` without a configured budget, Charter content drift, typed
  version drift, draft drift, inactive/foreign objective, Sponsor mismatch, non-chartering
  project, missing/empty Charter artifact, unverified decision evidence) and, last, the
  card-named success fixture `project_charter_approval_freezes_scope_criteria_and_budget`.
- **Guard and lane.** `kiana-core/tests/co10_project_charter_guard.rs` 1 → 2 tests: the marker
  guard plus `co10_charter_completion_slice_stays_wired_to_fixtures_lane_and_shards`, which
  binds the refusal-code matrix to source and fixtures, the deny-first order, the lane steps and
  the `kiana-domain-s2/4`/`kiana-core-s2/6` shards, and fails closed on a second execution loop.
  `.github/workflows/co10-project-charter.yml` adds `cargo fetch --locked` and the two
  `--no-run` compile steps (still `workflow_dispatch`-only, `permissions: contents: read`).
  `scripts/ci/test-shards.json` and the unified `ci.yml` were not changed.

## CI-only evidence

Baseline: `docs/roadmap/co10-project-charter-baseline.md`.
`.github/workflows/co10-project-charter.yml` runs the CO-10 domain fixtures, the Core source
boundary guard, formatting and workspace test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: implemented
proof_level: source
```

## Limitations and handoff

- The draft is an additive, typed identity on top of the compatibility `Project` fields. A
  legacy/replayed project without a recorded draft keeps the artifact-identity path only;
  a migration must backfill drafts before the typed identity can be claimed for all historical
  projects. `StartChartering` therefore freezes a draft only when one exists, so legacy event
  sequences still replay.
- The draft is recorded once per project; a revised Charter is a ChangeRequest (CO-30) that
  publishes a new baseline, not a silent re-draft. `DraftCharter` is registered for the
  `sponsor`/`pm` roles only; opening it to further chartering roles (for example `analyst`)
  would widen the CP-23 Project mutation-class subject set and is left to the role/assignment
  work.
- `ProjectBudget.project_id` is retained as the existing typed budget contract while the Company
  project key and `project_budget_ref` remain legacy string fields; the draft binds the same
  reference and the budget digest is frozen, but a later migration must still make the typed
  binding explicit across all historical projects.
- The legacy `CompanyArtifact` text snapshot remains the compatibility store; durable blob
  retention, cross-process recovery and typed Charter artifact persistence remain open.
- No live model, external budget system, source-code write, durable cross-process or physical
  business outcome is claimed.
