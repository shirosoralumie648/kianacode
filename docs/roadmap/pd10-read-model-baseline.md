# PD-10 Persistence Read Model Baseline

## Scope

PD-10 adds a read-only `PersistenceReadModel` boundary in `kiana-core`. Given committed facts,
the server derives Run state, Invocation projections and a redacted Receipt from the existing
EventLog reducers, and binds source cursor/event IDs and an optional data epoch. Foreign/empty
sources, duplicate source event IDs, epoch drift and conflicting terminal facts fail closed;
missing terminal facts remain unresolved and can never become `Completed`.

The model owns no EventStore, checkpoint, Broker, Runner or authorization write path. Durable
checkpoint persistence and Cell/Grant/Budget/Lease projection remain subsequent PD-11+ work.

## 2026-10-04 stable terminal-conflict error repair

GitHub run `37188557818` on `38cf305c839ece27595a2f28bacdd1fbdab36559` failed the existing
`missing_terminal_foreign_run_old_epoch_and_conflict_fail_closed` assertion. The reducer correctly
returned a typed terminal conflict, but the read-model adapter formatted its human-readable Display
(`run terminal conflict`) instead of the stable `run_terminal_conflict` code.

The adapter now matches `RunProjectionError::TerminalConflict`, preserves the
`persistence_read_model_run:` context, and appends `run_terminal_conflict:<kinds>`. The original
rejection assertion remains unchanged. CI regression fixtures cover all ordered pairs of the four
terminal kinds and started/queued/awaiting-approval facts with no terminal; unresolved state must
never produce a completed model or receipt. The reducer stops at the first conflicting pair, so
only that pair is asserted as evidence.

New-source validation is exclusively `.github/workflows/pd10-read-model.yml` and the unified CI.
This repair does not complete PD-10's fresh-process persistence acceptance or upgrade its proof.

## 2026-10-17 card closeout (CI-only)

The card names a success condition (`新进程只靠 facts 重建同一状态；Receipt 带 evidence/source
cursor`) and an Attempt in its title. Both were still open at source level, so this slice adds:

- `PersistenceReadModel.attempts`: the read model now folds the same facts through
  `project_capability_attempts` (per `(request_id, attempt)`) instead of leaving attempts inside
  the receipt only. Every record is validated to belong to the projected run
  (`persistence_read_model_attempt_scope_invalid`); attempt-fold errors keep their own stable
  prefix (`persistence_read_model_attempt:capability_attempt_*`).
- `project_capability_attempts` returns records sorted by `(request_id, attempt)`. The previous
  `HashMap::into_values()` order could not be reproduced by a fresh process, which is what the
  cross-process read-model comparison below needs.
- `kiana-core/tests/pd10_read_model.rs` adds `read_model_projects_capability_attempts_and_receipt_evidence`
  (full lifecycle: attempt identity/effect/stop/fence/source cursor plus run-receipt status,
  execution-receipt and observability-attempt evidence round-tripped through the typed DTOs) and
  `foreign_or_conflicting_attempt_facts_fail_closed` (a terminal result whose attempt has no
  request/dispatch facts, a second terminal for the same attempt with different evidence, and a
  caller-rebound attempt record all fail closed).
- `read_model_rebuilds_the_same_state_in_a_fresh_process`: the lifecycle facts are appended to a
  real `JsonlEventLog` (no stream metadata, so only append order is preserved) and a child process
  (`std::env::current_exe()` + `--exact`) reopens that journal and rebuilds the model from facts
  only. The parent re-reads the same journal, projects online, and asserts the two serialized
  models are identical, that the child appended/rewrote nothing (journal bytes unchanged), and
  that the rebuilt model carries attempts plus a `completed` run receipt. The existing
  missing-terminal assertions were strengthened to read the typed `run_receipt.status` instead of
  the absent top-level `status` key.
- `pd10_read_model_guard.rs` gained `pd10_read_model_projects_attempts_without_adding_authority`
  and `pd10_behavioural_fixtures_are_wired` (fixture names, child-process env contract, stable
  attempt codes and lane/shard wiring).
- `read_model_rebuilds_run_invocations_and_receipt_from_facts` also asserts `attempts.is_empty()`
  for a run without capability facts, so "no attempt is invented" is pinned on the success path.

No new test target, dependency, manifest or execution path: both fixtures live in the existing
`pd10_read_model` target, which `scripts/ci/test-shards.json` (`kiana-core-s5/6`) and the manual
lane already claim.

## Evidence and limits

- `kiana-core/tests/pd10_read_model.rs` covers fact-only rebuild, receipt source binding and typed
  run-receipt status, missing terminal, foreign run, old epoch, conflicting terminal, duplicate
  event, capability attempts with receipt evidence, foreign/conflicting attempt rejection, and the
  fresh-process rebuild from a durable journal.
- `kiana-core/tests/pd10_read_model_guard.rs` protects the no-write/no-execution boundary, the
  attempt projection/ordering markers and the fixture/lane wiring.
- `.github/workflows/pd10-read-model.yml` runs the fixtures, source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the read model, its attempt
projection and both CI acceptance paths are present, while durable checkpoint storage, daemon
restart/cross-host recovery and Cell/Grant/Budget/Lease projection remain PD-11+ work.

Known limits:

- Retry attempts are not folded: `project_invocations` is request-scoped, so a second attempt after
  a terminal fact for the same request fails the whole read model closed
  (`persistence_read_model_invocation:invocation_state_transition_invalid`) instead of exposing two
  attempt records. Widening the invocation fold is ER-09 territory and was deliberately not
  touched here.
- The fresh-process fixture starts a second test process over the same journal file; it does not
  restart a daemon/BackendHost, rebuild a durable checkpoint, or prove cross-host durability.
- Local `cargo fmt --all` (rustfmt 1.10.0-stable, 2026-10-13) reports pre-existing formatting drift
  in `kiana-daemon/src/execution_control.rs`, `kiana-daemon/src/harness_memory.rs` and
  `kiana-daemon/tests/h17_job_handle.rs`, which belong to H17/CAP-19 (the latter file digest is
  pinned by that step's snapshot guard). The drift was reverted and left untouched; the lane's
  `cargo fmt --all --check` step may fail on this inherited condition until the owning slices or the
  integration owner re-run rustfmt on those files.

