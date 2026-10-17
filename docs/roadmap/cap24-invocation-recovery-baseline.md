# CAP-24 Invocation recovery, bounded retry and reconciliation baseline

CAP-24 uses the existing EventLog as the authority for Invocation state. `project_invocations`
rebuilds request, approval, dispatch, attempt and terminal facts by stable request/call identity;
duplicate or conflicting terminal evidence fails closed, and a dispatch without a terminal fact
projects as `Unknown` rather than success.

Resume is an explicit claim over a committed run snapshot. The binding rechecks owner, session,
project, role prompt, parameter/action/catalog digests, sandbox, pending batch and authority/data
epochs. Redacted snapshot material is display-only and makes the snapshot non-resumable; an
unavailable continuation does not mint executable input. A successful resume restores the existing
Runner/Cell state through ControlPlane and never directly invokes a handler.

Model retries are new bounded attempts with their own identity, deadline, retry class, budget
reservation and settlement. Unknown, cancelled, denied, or effectful/non-idempotent outcomes are
not automatically retried. Reconciliation accepts only explicit scoped evidence, appends a
correction/evidence fact, keeps the original runtime outcome unchanged, and never acts as a retry;
compensation or a new request must obtain its own authorization.

## Behavioural fixtures

The invocation recovery fixtures live in the in-module `#[cfg(test)] mod invocation_tests`
(`kiana-core/src/recovery_invocation_tests.rs`, wired from `kiana-core/src/recovery.rs`). They drive
the production `ControlPlane::resume_run`, `checkpoint_run` and `rebuild_pending_invocation` paths
against a durable `JsonlEventLog` (closed and reopened to model a process restart) or a
`MemoryEventLog`; only the Runner and ApprovalStore are test doubles, and both record their call
counts so "no execution" and "no material read" are asserted facts, not implied ones. The Broker is
the real `kiana_capability_broker::CapabilityBroker`; the fixtures never reach it because no scenario
dispatches a handler. Hand-built facts carry no stream metadata, so storage keeps append order (the
same convention as `dispatch_delivery_tests` / `er20_restart_projector`), while `run.snapshot`,
`run.resume_prepared` and terminal facts are still written by the production CAS paths.

Deny-first fixtures, in the order the focused lane runs them:

| fixture | what it proves |
|---|---|
| `restart_never_reexecutes_unknown_non_idempotent_call` | a `mcp.call` `ExternalSideEffect` paused mid-dispatch is checkpointed; after reopening the durable journal `resume_run` restores the Runner once and records exactly one `run.resume_prepared` claim plus one `run.result_unknown` (`result_unknown:resume_pending_tool`), the projection stays `Unknown` with no result, and a second resume on the recorded unknown outcome is denied with `result_unknown:run_resume_result_unknown` with zero extra writes — never a second `execution.prepared` or `run.completed` |
| `changed_input_or_descriptor_causes_replay_divergence` | for a pending `shell.exec` approval, a rewritten protected command (with its scope/action digest recomputed) rebuilds to `approval_action_changed` after one decision read and two material reads; the unchanged command recovered under a different sandbox descriptor fails the binding with `invocation_resume_binding_changed`; both variants leave zero Runner calls and no new facts |
| `redacted_snapshot_cannot_be_used_as_executable_payload` | a Runner state containing top-level and nested JSON-string `api_key` secrets checkpoints to a redacted snapshot with `resumable=false` and the secret bytes never reach the journal; resume is blocked (`run_snapshot_invalid`) before decoding or any approval read, and a forged digest-consistent `resumable=true` snapshot carrying `[REDACTED]` placeholder material is still refused with `run_snapshot_pending_invocation_missing` — zero Runner calls, zero approval reads, zero new facts |
| `foreign_owner_cannot_read_or_resume_another_runs_invocation` | another actor on the same session gets `session_owner_mismatch` and an unknown session gets `session_not_found` before snapshot decode, approval material read or Runner restore; the durable facts are unchanged |
| `completed_result_is_reused_from_facts_and_terminal_replay_is_denied` | a committed `execution.result_committed` + `run.completed` projects `Succeeded`, the committed output is reused from the event facts without executing anything, and resume on the terminal run is denied with `result_unknown:run_resume_terminal_completed`; exactly one prepared/committed/completed fact set remains |
| `pending_approval_rebuild_is_fact_bound_and_ambiguity_fails_closed` | the single pending approval rebuilds the continuation from the approval-event binding plus protected store material (one decision read, two material reads, `event_sequence` refreshed to the approval event stream) with request/binding identity equality and zero execution or writes; with two pending candidates the rebuild fails closed with `approval_continuation_ambiguous` before reading any decision or material |
| `terminal_failed_or_cancelled_run_refuses_resume_without_execution` | a `run.failed` and a `run.cancelled` run are both read back as terminal facts after reopening the journal and resume is denied with `result_unknown:run_resume_terminal_failed` / `result_unknown:run_resume_terminal_cancelled` before any Runner call or approval read, with no new facts — the "known failed" and cancelled recovery categories behave like the completed one |

Focused `--exact` filters `recovery::invocation_tests::<name>` are the executable receipt for each
row. These fixtures do not replace a process-level crash matrix: the "restart" is an in-process
close/reopen of the journal, the Runner is a double, and event-persistence/spawn/result-window fault
injection belongs to the capability crash-window matrix (CAP-26). Local CAS de-duplication is never
presented as cross-external-service exactly-once.

Two existing behavioural targets carry the card's remaining regression bullets and are wired into
the same lane: `kiana-core/tests/dep25_effect_reconcile_route.rs` (a reconciliation is refused for
anonymous or cell-scoped callers, `Unknown` is never retried through a command, an admissible
resolution is recorded while the route stays read-only) and `kiana-runner/tests/p4_j7_23_retry.rs`
(a post-send unknown is not retried automatically, a retryable pre-send rejection retries within the
bounded attempt/deadline budget with a fresh permit per attempt, and attempt reservations settle or
stay unknown without dangling). No new test target, manifest or shard entry is added by this step.

## CI wiring

`.github/workflows/cap24-invocation-recovery.yml` keeps the existing replay, resume, invocation,
reliability, retry and reconciliation fixtures plus the CAP-24 source guard, and adds the in-module
run (`cargo test -p kiana-core --lib recovery::invocation_tests::`), a per-fixture `--exact` loop in
deny-first order, the two behavioural retry/reconciliation targets and a `--no-run` compilation for
each. `kiana-core`'s unified `ci.yml` shards run
explicit integration-test targets only, so they cannot execute `--lib` fixtures; the same `--lib`
command therefore also runs as the automatic `cap24-invocation-fixtures` job in
`.github/workflows/ci.yml`. `kiana-core/tests/cap24_invocation_recovery_guard.rs` adds
`cap24_behavioural_fixtures_are_wired_and_deny_first`: every fixture name must exist in the in-module
fixture source, in the focused lane and in this baseline; no fixture may be `#[ignore]`d; the module
wiring must stay in place; both lanes must run the module; the deny-first fixtures must precede
the completed-result reuse and pending-rebuild regressions in the focused lane; and the two
behavioural retry/reconciliation targets must stay wired.

GitHub Actions runs replay, resume, invocation, retry, reliability and reconciliation fixtures,
the CAP-24 recovery fixtures, source guards and workspace compilation. No local runtime tests or
smoke commands were run.
