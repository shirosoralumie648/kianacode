# EQ-28 runtime correctness evaluator baseline

## Scope

EQ-28 adds `RuntimeCorrectnessEvaluator` to `kiana-quality`. It consumes the canonical,
redacted trace produced by EQ-17/EQ-18 and emits the existing EQ-27 bounded `Finding` shape.
The evaluator checks source-cursor and per-request sequence order, correlation and invocation
identity aliases, one terminal per lifecycle, approval request/decision/consumption order,
retry metadata/order, cancellation fencing and terminal completion, and the rule that an
`Unknown` result must be reconciled before a new attempt.

The input is caller-supplied evidence only. The evaluator does not read EventLog state, start a
runner, contact a provider, dispatch a capability, authorize an approval, cancel a run, or
schedule a retry. Findings are diagnostics for later quality aggregation and are not authority.

## Lifecycle scoping (2026-10-17 completion slice, CI-only)

Terminal uniqueness is scoped by lifecycle identity instead of by aggregate stream alone:

- run lifecycle facts (`run.completed`/`run.failed`/`run.cancelled`/`run.result_unknown`) close
  the run identity, and a later invocation attempt for that run is reported as
  `runtime.event_after_terminal` (the `Unknown` terminal is excluded because its reconciliation
  rule is reported separately);
- capability/invocation/execution attempt facts and model facts are scoped by their attempt
  aliases (`capability_request_id`/`invocation_id`/`execution_id`/`call_id`,
  `model_call_id`/`model_attempt_id`/`attempt_id`/`reservation_id`), so one run may contain
  several attempts without a false duplicate;
- approval facts are scoped per `approval_id` and keep the request → approved/denied →
  consumed ordering plus `runtime.approval_not_granted` binding for invocation facts;
- `run.receipt`, `run.snapshot`, `cell.*` and `receipt.*` are declared post-terminal projection
  and checkpoint facts: the execution spine writes them after a terminal event, so they are not
  lifecycle members and never produce an `event_after_terminal` finding;
- retry metadata rejects non-positive ordinals and a `next_attempt` that does not advance past
  the current `retry_ordinal`, and keeps per-key monotonic ordering;
- `Unknown` binds every alias it carries, so a later attempt is rejected even when the two
  events use different alias subsets.

## Evidence and limits

- `kiana-quality/tests/eq28_runtime.rs` covers a valid approval/invocation/run lifecycle, a
  multi-attempt run with post-terminal receipt/snapshot/cell projections, unmatched and duplicate
  terminals, late lifecycle and invocation events, cursor/sequence/correlation regressions,
  malformed envelopes, invocation alias conflicts, retry metadata/order, several model calls per
  run, approval ordering, cancellation fencing/terminals, Unknown duplicate/retry blocks,
  deterministic evidence-bound output, malformed input rejection and the empty-trace finding.
- `kiana-quality/tests/eq28_runtime_guard.rs` protects the pure bounded boundary, the required
  runtime finding markers and the fixture matrix names.
- `.github/workflows/eq28-runtime-correctness.yml` runs the fixtures, source guard, formatting and
  workspace test-target compilation in GitHub Actions; local tests are intentionally not executed.
  Automatic coverage is the `kiana-quality` shard in `ci.yml` (`targets: null`), which runs every
  test target in the crate.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim durable
EvalStore facts, real target execution, provider/model quality results, statistical aggregation,
promotion authority, cross-process recovery, live or physical evidence. Those remain EQ-29+ and
ER/PD/SC work.

Known upstream limitation: the EQ-17 selection gate treats one aggregate stream as
terminal-once, while this evaluator now accepts several capability attempts per run. A trace
that EQ-17 normalizes can therefore be stricter than the evaluator; reconciling the EQ-17 stream
key contract is out of scope for EQ-28.
