# EQ-33 workflow and swarm evaluator baseline

## Scope

EQ-33 adds `WorkflowSwarmEvaluator` to `kiana-quality`. It evaluates a bounded, caller-supplied
workflow/swarm evidence graph: DAG structure (parent existence, self edges, cycles and depth
order), positive and strictly increasing attempts, fan-out and fan-in bounds and counts, child
scope containment inherited from every parent, digest-verified fan-in merge evidence and explicit
compensation facts with receipt references.

The evaluator is diagnostic only. It does not schedule or dispatch nodes, create child Cells,
merge workspaces, run a swarm, send messages between agents, or authorize/reuse compensation
approvals.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first workflow/swarm contract:

- Envelope: structural drift fails closed in `WorkflowSwarmInput::validate`
  (`workflow_swarm_input_schema_invalid`, `workflow_swarm_node_limit_exceeded`); zero declared
  fan-out/depth bounds are explicit `workflow.bounds_invalid` findings bound to `workflow:input`.
- DAG: nodes need a unique bounded identity (`workflow.node_identity_invalid`), parent references
  are bounded and unique (`workflow.parent_identity_invalid`), a node cannot parent itself
  (`workflow.parent_self`), every declared parent must exist (`workflow.parent_missing`), depth
  must stay within the declared bound (`workflow.depth_exceeded`) and above every parent's depth
  (`workflow.depth_inconsistent`), and cycles are rejected (`workflow.dag_cycle` bound to
  `workflow:dag`).
- Attempts: the attempt history must be non-empty, positive and strictly increasing
  (`workflow.attempt_invalid`) and within the per-node limit (`workflow.attempt_unbounded`).
- Fan-out/fan-in: observed and expected children stay within `max_fan_out`
  (`workflow.fanout_exceeded`) and agree with each other (`workflow.fanout_mismatch`); the declared
  parent count stays within the hard fan-in limit (`workflow.fanin_unbounded`) and matches the
  declared and observed parent counts (`workflow.fanin_mismatch`).
- Child scope: scope entries are bounded within a per-set limit (`workflow.scope_invalid`), the
  child scope is a subset of the node's parent scope (`workflow.child_scope_expanded`, the card's
  acceptance scenario) and the parent scope never exceeds what each parent granted
  (`workflow.parent_scope_expanded`, reported per offending parent edge).
- Merge: a true fan-in requires a verified merge (`workflow.merge_missing`), verified merges need
  a strict `sha256:` expected/observed digest pair (`workflow.merge_invalid`) that must be equal
  (`workflow.merge_mismatch`), and merge evidence below fan-in is unexpected
  (`workflow.merge_unexpected`).
- Compensation: a required compensation must be recorded outside the in-flight `compensating`
  status (`workflow.compensation_missing`), an unrequested record is unexpected
  (`workflow.compensation_unexpected`), recorded compensation needs a bounded receipt reference
  (`workflow.compensation_receipt_missing`) which is not accepted without a record
  (`workflow.compensation_receipt_unexpected`), and `compensated`/`compensating` statuses must be
  coherent with the recorded facts (`workflow.compensation_invalid`).
- Nodes are ordered by a stable total key before evaluation, so the finding set does not depend on
  the caller's array order while duplicate evidence stays visible. Findings carry per-section
  evidence references (`workflow:input`, `workflow:dag`, `workflow:node:<index>`) and every
  caller-supplied value goes through `Finding::redacted`.

## Evidence and limits

- `kiana-quality/tests/eq33_workflow.rs` (23 rejection-first fixtures) covers the card's acceptance
  scenario `child_scope_expansion_fails_quality_gate`, bounded DAG/attempt/scope success paths
  including a digest-verified fan-in, cycle/missing/self parents, identity and parent bounds,
  depth order, attempts, fan-out/fan-in bounds and counts, merge requirement/validity/equality and
  unexpected evidence, parent scope containment, scope entry bounds and set deduplication,
  compensation facts and statuses, envelope/node schema drift, oversized input, envelope bound
  declarations, node order independence with duplicate evidence, the redaction boundary over
  caller values, typed round-trip, and deterministic evidence-bound registry output.
- `kiana-quality/tests/eq33_workflow_guard.rs` protects the pure bounded boundary (no
  filesystem/network/process/provider/store/broker/messaging dependency), the required markers and
  stable codes, the fixture matrix, the manual lane's exact steps and the automatic
  `kiana-quality` shard (`targets: null`).
- `.github/workflows/eq33-workflow-swarm.yml` runs `cargo fetch --locked`, the format gate, both
  fixture targets and two `--no-run` compile steps in GitHub Actions; local tests are intentionally
  not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim durable queue
or child lifecycle facts, real concurrent workers, independent review/merge receipts, recovery,
compensation success, promotion authority, live or physical evidence.

## Known limits

- Findings are diagnostics over caller-supplied values only; they never schedule or dispatch a
  node, create a child Cell, merge a workspace, run a swarm, send a message between agents, or
  authorize compensation. Mapping them into blocking rules belongs to EQ-42.
- Node statuses, attempt histories, child counts, parent counts, scopes, merge digests and
  compensation receipts are caller-asserted evidence. The evaluator can refuse an incoherent or
  unauthorized shape, but it cannot prove that a real workflow ran, that the recorded fan-in
  counts are the counts the product observed, or that a merge actually happened.
- `parent_scope`/`child_scope` are caller-supplied sets; containment is checked as set inclusion
  over a bounded ASCII identity alphabet and unknown shapes are rejected as invalid rather than
  normalized.
- The merge digests are bound and bounded but never recomputed against a workspace, artifact or
  receipt store.
- References are restricted to a bounded ASCII identity alphabet; unknown shapes are rejected as
  invalid rather than normalized.
- `Finding::redacted` covers known secret shapes; unmarked high-entropy values may still pass.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
