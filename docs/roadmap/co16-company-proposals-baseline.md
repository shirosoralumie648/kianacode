# CO-16 Company proposal and Plan approval baseline

## Scope

CO-16 turns a role task's Plan output into business state without giving text any power. A
planning role (PM or Architect) submits a structured role-task result; the intake refuses every
prose surface (README, stdout, assistant message, transcript) before it looks at the body. The
validated Plan preview is what the Sponsor reviews, and one Company CAS freezes the exact
Plan/Milestone/Packet reference set plus inert follow-on Handoff intents.

## Completion slice (2026-10-19, local_behavior)

### Intake contract

- `CompanyRoleTaskResult` (`kiana.company-role-task-result.v1`) binds task, assignment, role, the
  session that produced it and the proposal digest. Only `pm` and `architect` are accepted
  (`company_proposal_source_role_denied`); a Builder task result is refused.
- `CompanyPlanProposalIntake` (`kiana.company-plan-proposal-intake.v1`) has two constructors:
  `role_task(...)` for the structured result and `text_surface(kind, observed_text)` for the
  audit-only prose surfaces `Readme`, `Stdout`, `AssistantMessage`, `Transcript`. Any non
  `RoleTaskResult` kind is refused with `company_proposal_source_not_role_task_result`, and a
  `RoleTaskResult` claim without a typed result is refused with the same code. `observed_text` is
  stored for audit and is never parsed. The ledger has no `serde_json` command decode and no
  `CompanyCommand::from` path; the source guard pins both absences.

### Preview and all-or-nothing

- `CompanyPlanPreview::build` validates project/baseline/coverage equality, the required-criteria
  set, node/packet budgets, the exact node-version set, one parent Milestone per packet
  (`company_plan_preview_packet_parent_invalid`), blocking dependency order and duplicate
  packets, then derives milestones, packets and handoff intents and the whole-plan digest. Every
  check runs before any ledger field is touched.
- `CompanyProposalLedger::submit` builds the complete preview before inserting the proposal or
  preview fact. A failure on the Nth packet leaves `proposals`, `previews`, `approvals`,
  `materializations` empty and `revision` at 0.

### One CAS, digest identity, idempotent replay

- `CompanyPlanApproval` binds the proposal digest, the preview digest, the expected Company
  revision, the decision reference and the Sponsor approver. `approve` checks proposal/preview
  digests (`company_plan_approval_preview_mismatch`), the revision
  (`company_plan_approval_revision_conflict`) and duplicates
  (`company_plan_approval_duplicate`), then calls `preview.materialize(...)` **before**
  `self.revision += 1` and before either insert.
- `CompanyPlanMaterialization` carries `approved_plan_digest` and the frozen plan graph.
  `approved_plan_digest == preview.plan_digest == FrozenPlanGraph.plan_digest` is asserted in
  both the ledger fixture and the aggregate fixture. An exact replay of the same approval returns
  the same materialization and does not duplicate packets or handoffs.
- The materialization grants no authority: handoff intents carry `runtime_authority = false`, and
  the approval creates no milestone, packet, claim, lease, budget or run.

### Aggregate command

- `CompanyCommand::ApproveCompanyPlan { intake, basis, approval }` (event `CompanyPlanApproved`,
  Sponsor-only, Project mutation class, `CompanyState::company_proposals`) is the business
  command. `kiana-core` runs `company_plan_approval_verdict` before `state.transition`, so a
  refused plan produces no event, no receipt transition and no field change.
- The aggregate verdict re-reads server-side facts: writable project, frozen Charter baseline
  (`company_plan_baseline_missing`/`company_plan_baseline_mismatch`), CO-11 coverage binding when
  frozen, a configured budget (`company_plan_budget_missing`), the frozen Plan graph
  (`company_plan_preview_plan_not_frozen`) and every node version
  (`frozen_plan_graph_node_version_mismatch`). Basis and approval revisions must equal the
  aggregate revision.
- `approve_company_plan` clones the ledger, sets `next.revision = self.revision`, submits and
  approves on the clone, and assigns it back only on success; the exact-approval replay branch
  returns `Ok(())` before the revision CAS because the digest binds the consumed revision.

## Acceptance fixtures

`kiana-domain/tests/co16_company_proposals.rs` (3, deny first):

1. `proposal_text_cannot_execute_commands_and_invalid_plan_commits_nothing` - four prose surfaces
   embedding a complete command JSON are refused with
   `company_proposal_source_not_role_task_result`; an untyped structured intake is refused; a
   Builder role task result is refused; the invalid third packet is refused with
   `company_plan_preview_packet_parent_invalid` and leaves zero facts.
2. `preview_refuses_version_budget_and_criteria_drift_before_any_preview_exists` - node-version
   drift/missing, packet budget, coverage digest, forged preview digest, stale revision and a
   non-Sponsor approver are all refused with stable codes and no preview/approval exists.
3. `approved_plan_materializes_the_exact_reviewed_packet_graph_once` - success last; digest
   equality, 2 packets/2 handoffs with no runtime authority, revision 1, exact replay idempotent,
   different approval refused as duplicate.

`kiana-domain/tests/co16_company_proposals_command.rs` (2, deny first):

1. `approve_company_plan_refuses_prose_and_invalid_later_packets_without_mutation` - the same
   prose surfaces on the real `CompanyState::transition` path, a PM role denial
   (`company_role_denied`), the invalid later packet, a stale CAS revision, a missing budget and a
   drifted node version; every refusal leaves the proposal ledger untouched.
2. `approved_plan_is_one_cas_and_replay_does_not_duplicate_packets` - success last; digest
   equality with the reviewed preview, two packets and two inert handoffs, no milestone/packet/run
   created, exact replay keeps one approval and two packets.

`kiana-core/tests/co16_company_proposals_guard.rs` (2 guards):

1. `company_proposals_accept_only_structured_role_task_results_and_freeze_the_reference_set` -
   schema/type markers, the closed prose source enum, the intake refusal, and the absence of
   command decode, process spawn, Broker, EventStore, Harness or runtime-grant identifiers.
2. `plan_approval_is_one_atomic_cas_and_the_ci_lane_keeps_the_deny_first_order` - `materialize`
   precedes the revision bump and both inserts in `approve`; `build` precedes the proposal insert
   in `submit`; deny fixtures precede success in both domain files; the manual lane keeps
   `workflow_dispatch:` only, `permissions: contents: read`, locked fetch, format gate, the three
   focused targets serially and three `--no-run` steps; the automatic shard ledger still binds
   `kiana-domain-s2/4` and `kiana-core-s2/6`; no second execution surface.

## Evidence

Local focused runs, `--offline --test-threads=1`:

```text
cargo test -p kiana-domain --test co16_company_proposals          -> 3 passed, exit 0
cargo test -p kiana-domain --test co16_company_proposals_command  -> 2 passed, exit 0
cargo test -p kiana-core --test co16_company_proposals_guard      -> 5 passed, exit 0
cargo check -p kiana-domain --tests --offline                     -> exit 0
cargo check -p kiana-core --tests --offline                       -> exit 0
rustfmt --edition 2021 --check <touched Rust files>               -> exit 0
```

`cargo fmt --all --check` is unavailable on this host (os error 206, path too long), so formatting
was verified per file. `cargo test --workspace`, clippy and the smoke scripts were not run. The
GitHub Actions lane was not triggered and its receipt was not awaited.

```text
feature_status: implemented
proof_level: local_behavior
```

## Limitations and handoff

- The contract is in-process domain state plus the `CompanyState` aggregate. Protocol DTOs,
  durable cross-process proposal storage and restart recovery are not delivered.
- Handoff intents are inert references; CO-17 still owns the ACK and accountability transfer, and
  CO-18/CO-19 own ready predicates, claims and leases. No scheduler or second execution loop was
  added.
- Real artifact/coverage reads, role assignment resolution, live provider output and any external
  business effect remain unproven; the fixtures use typed values only.
- The aggregate proof runs through `CompanyState::transition` with `CompanyProof::default()`; it
  does not exercise a live DaemonHost session or the web/CLI surfaces.
- `scripts/ci/test-shards.json` registers `co16_company_proposals_command`. The shard ledger still
  has one pre-existing gap unrelated to this card: `kiana-core/tests/ui22_receipt_detail.rs`
  (from commit 67fe1032) is not claimed by any shard. This slice did not touch that file.
