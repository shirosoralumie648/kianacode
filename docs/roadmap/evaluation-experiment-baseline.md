# EQ-38 EvalExperiment replay baseline

## Scope

EQ-38 adds a pure replayable `EvalExperiment` contract to `kiana-quality`. Admission, start,
case-completed and terminal events reduce deterministically into an experiment status and a
case-result index. Sequence gaps, missing admission/terminal, illegal transitions, duplicate case
result digest drift and events after terminal fail closed; the resulting experiment carries a
replay digest.

This is a value/reducer contract only. It does not persist to EvalStore, run cases, invoke a
provider/judge, allocate budget, or submit a quality-gate decision.

## Completion slice (2026-10-17, CI-only)

The 2026-09-26 slice was a narrow reducer with three fixtures. This slice closes the card exit
conditions in source and CI wiring:

- `kiana-quality/src/experiment.rs` validates the envelope itself (schema drift
  `experiment_replay_schema_invalid`, empty/whitespace/unbounded identity
  `experiment_replay_identity_invalid`, more than `MAX_EXPERIMENT_EVENTS`
  `experiment_event_limit_exceeded`) before any event is reduced, so `replay_experiment` fails
  closed for direct callers exactly like the evaluator.
- The log shape is fully fenced: the first event must be the single admission
  (`experiment_admission_missing`), sequences must be contiguous from 1
  (`experiment_event_sequence_invalid`, catching gaps, repeats and reordering), transitions must
  follow admission/start (`experiment_transition_invalid`), every case result needs a bounded
  identity and a strict `sha256:` digest (`experiment_case_result_invalid`), one case may not be
  reported twice (`experiment_case_result_repeated` for an identical repeat,
  `experiment_case_result_conflict` for a drifted digest) and any event after a terminal state is
  refused (`experiment_event_after_terminal`); a log that never terminates is refused
  (`experiment_terminal_missing`).
- `EvalResultIndex` keeps the `source_sequence` of the single result event, the index is ordered by
  stable case identity and the replay digest binds identity, status, index, terminal sequence and
  the complete event log (canonical `json_digest`).
- Typed constructors (`ExperimentEvent::new`/`with_case_result`, `ExperimentReplayInput::new`)
  and schema constants (`EXPERIMENT_EVENT_SCHEMA`, `EXPERIMENT_SCHEMA`, `MAX_EXPERIMENT_EVENTS`,
  `MAX_EXPERIMENT_TEXT_BYTES`) are exported so fixtures and future steps share one contract.

## Evidence and limits

- `kiana-quality/tests/eq38_experiment.rs` covers the card acceptance fixture
  (`experiment_state_is_replayable_from_events`) plus sequence/gap, admission, transition,
  case-result, duplicate/drift, post-terminal, missing-terminal, terminal-matrix, digest-binding,
  envelope-bound, unknown-field, determinism/registry and typed round-trip refusals.
- `kiana-quality/tests/eq38_experiment_guard.rs` protects the pure replay/index boundary, binds
  the fixture names and pins the manual lane plus the automatic `kiana-quality` shard
  (`targets: null`).
- `.github/workflows/eq38-experiment.yml` runs locked fetch, formatting and both test targets
  with `--test-threads=1`, then compiles both targets in GitHub Actions; local tests are
  intentionally not executed. The automatic lane remains the unified `ci.yml`
  `kiana-quality` shard.

This slice is `feature_status=implemented`, `proof_level=source`: no durable experiment/result
store, real target execution, provider/judge evidence, gate decision, promotion, live or physical
proof is claimed. The reducer validates caller-supplied event history only; it does not prove that
a real experiment ever emitted these events.
