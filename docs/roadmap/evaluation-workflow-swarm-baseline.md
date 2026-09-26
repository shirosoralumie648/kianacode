# EQ-33 workflow and swarm evaluator baseline

## Scope

EQ-33 adds `WorkflowSwarmEvaluator` to `kiana-quality`. It evaluates a bounded workflow/swarm
evidence graph for DAG cycles and missing parents, monotonic attempts, depth/fan-out/fan-in
limits, child scope containment, deterministic merge evidence and explicit compensation facts.

The evaluator is diagnostic only. It does not schedule or dispatch nodes, create child Cells,
merge workspaces, run a swarm, send messages, or authorize/reuse compensation approvals.

## Evidence and limits

- `kiana-quality/tests/eq33_workflow.rs` covers a valid bounded DAG, child scope expansion,
  cycle/attempt/fan-out/fan-in/merge/compensation failures and strict unknown-field rejection.
- `kiana-quality/tests/eq33_workflow_guard.rs` protects the pure bounded evaluator and no-free-
  message-bus/no-execution boundary.
- `.github/workflows/eq33-workflow-swarm.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim durable queue
or child lifecycle facts, real concurrent workers, independent review/merge receipts, recovery,
compensation success, promotion authority, live or physical evidence.
