# CO-12 Plan and WorkGraph proposal baseline

## Scope

CO-12 adds a versioned, data-only Plan proposal contract on top of the existing Company
Milestone/WorkPacket and packet DAG contracts. A plan carries a frozen Charter baseline version,
coverage digest, typed Milestone/Packet nodes and explicit parent/child, artifact, run-success,
acceptance and related edges.

## Implemented source slice

- `kiana-domain/src/plan.rs` defines `PlanProposal`, `PlanNode`, `PlanEdge` and deterministic
  `topological_order()` with bounded sizes, stable digest, project/version binding and strict
  unknown-field rejection.
- Validation rejects missing endpoints, cross-project nodes, duplicate edges/nodes, malformed
  references, invalid parent/child kinds, dependency cycles and isolated required nodes. Related
  edges remain informational and do not affect the blocking DAG.
- The contract reuses `CriterionId` for typed coverage references and records the approved Charter
  baseline and coverage digest. It does not start runs, claim packets, consume budgets or create a
  second scheduler.
- A required Milestone has to own at least one required Packet output (`plan_required_milestone_empty`
  otherwise), a declared required criterion has to be owned by a required node
  (`plan_required_criterion_isolated`), and the reading range is closed: every node input outside
  the declared `read_scope` is refused (`plan_read_scope_insufficient`), so a proposal cannot widen
  what an admitted plan may read.
- `PlanGraphReport` exposes the batch facts for a candidate graph that is not (yet) a valid
  proposal: first rejection reason, stable topological order over the resolvable blocking edges,
  the cycle path (`a -> b -> a`, stable across edge permutations) and unresolved edge endpoints
  with their edge index.
- CO-12 completion: `PlanProposalDraft` (`kiana.company-plan-draft.v1`) is an open batch
  accumulator — nodes, blocking edges, required criteria and read scope may be pushed in any order
  (edges before nodes, children before parents) and duplicate/malformed additions fail at push time.
  The complete graph is validated exactly once at `freeze`/`freeze_versioned`, which is also where
  the exact plan revision is bound.
- CO-12 completion: `FrozenPlanGraph` (`kiana.frozen-plan-graph.v1`) freezes the admitted plan
  digest, plan/Charter versions, coverage digest and the per-node dependency versions.
  `FrozenPlanGraph::verify` refuses project drift, a removed/added node, a bumped dependency
  version, plan-version/baseline/coverage drift and finally a whole-plan digest drift, each with
  its own stable reason.
- CO-12 completion: `CompanyCommand::FreezePlanGraph { plan }` (event `PlanGraphFrozen`,
  `CompanyState::plan_graphs`) is the approval transaction. `CompanyState::plan_graph_freeze_verdict`
  is the shared verdict: it revalidates the complete candidate graph, requires the server-side
  frozen Charter baseline (`plan_graph_baseline_missing`/`plan_graph_baseline_mismatch`), binds the
  plan to a frozen CO-11 coverage when one exists (`plan_coverage_digest_mismatch` and the other
  coverage reasons) and is idempotent for the exact admitted revision while a different revision of
  the same project is refused (`plan_graph_already_frozen`). The ControlPlane revalidates the
  verdict before the transition, so an invalid batch draft changes no related object.
- The freeze grants nothing: no Milestone, Packet, PacketClaim, Run, budget or lease is created,
  and the project revision does not move.

## Acceptance fixtures

Deny-first fixtures all live in `kiana-domain/tests/co12_plan_graph.rs` (card success fixture
last):

- `plan_rejects_missing_dependencies_cycles_and_cross_project_edges` (card)
- `plan_rejects_empty_required_milestones_isolated_criteria_and_narrow_read_scope`
- `plan_draft_accepts_any_insert_order_and_validates_once_at_freeze`
- `frozen_plan_graph_pins_dependency_versions_and_rejects_every_drift`
- `plan_graph_freeze_command_admits_the_exact_revision_once`
- `two_milestone_plan_preserves_dependency_and_acceptance_boundaries` (card, success)

Real-spine fixtures in `kiana-daemon/tests/co12_plan_graph_transactions.rs` drive
`DaemonHost -> ControlPlane -> EventStore`:

- `co12_invalid_plan_graph_commits_no_related_object` (card, deny-first)
- `co12_two_milestone_plan_commits_once_through_the_approval_transaction` (card, success)

The Core source guard `kiana-core/tests/co12_plan_graph_guard.rs` (2 tests) pins the markers,
the 46-code rejection matrix, the fixture names and deny-first order, the ControlPlane gate
running before the transition, the manual lane commands and the automatic shard bindings.

## CI-only evidence

`.github/workflows/co12-plan-graph.yml` runs locked fetch, formatting, the domain Plan fixtures,
the daemon approval-transaction fixtures, the Core source guard and the compilation of all three
targets on GitHub Actions. Automatic lanes keep running `kiana-domain-s2/4` (`co12_plan_graph`),
`kiana-core-s2/6` (`co12_plan_graph_guard`) and the whole-crate `kiana-daemon` shard (which
discovers `co12_plan_graph_transactions`) from the unified `ci.yml`. Local Cargo tests, builds,
checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: implemented
proof_level: source
```

## Limitations and handoff

- This slice is the typed proposal/validation/admission contract. The Business `PublishPlan`
  adapter keeps its legacy text contract, and no historical plan is migrated; that later
  integration belongs to CO-16/CO-18.
- Dependency references are typed and bounded strings; artifact/acceptance truth, durable plan
  storage, cross-process recovery and live scheduling remain unproven. The admitted graph lives in
  the Company aggregate, so its durability is exactly the EventLog's.
- `FrozenPlanGraph` records dependency versions; it does not yet drive a scheduler, ready
  predicate or packet claim (CO-18/CO-19).
- No model, broker, source-code write, external effect, durable, live or physical outcome is
  claimed.
