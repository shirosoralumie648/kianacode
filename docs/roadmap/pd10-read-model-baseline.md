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

## Evidence and limits

- `kiana-core/tests/pd10_read_model.rs` covers fact-only rebuild, receipt source binding, missing
  terminal, foreign run, old epoch, conflicting terminal and duplicate-event rejection.
- `kiana-core/tests/pd10_read_model_guard.rs` protects the no-write/no-execution boundary.
- `.github/workflows/pd10-read-model.yml` runs the fixtures, source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the core read model and CI
fixtures are present, while durable checkpoint storage, process restart and cross-process recovery
remain open.
