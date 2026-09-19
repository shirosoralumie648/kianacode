# PD-10 Persistence Read Model Baseline

## Scope

PD-10 adds a read-only `PersistenceReadModel` boundary in `kiana-core`. Given committed facts,
the server derives Run state, Invocation projections and a redacted Receipt from the existing
EventLog reducers, and binds source cursor/event IDs and an optional data epoch. Foreign/empty
sources, duplicate source event IDs, epoch drift and conflicting terminal facts fail closed;
missing terminal facts remain unresolved and can never become `Completed`.

The model owns no EventStore, checkpoint, Broker, Runner or authorization write path. Durable
checkpoint persistence and Cell/Grant/Budget/Lease projection remain subsequent PD-11+ work.

## Evidence and limits

- `kiana-core/tests/pd10_read_model.rs` covers fact-only rebuild, receipt source binding, missing
  terminal, foreign run, old epoch, conflicting terminal and duplicate-event rejection.
- `kiana-core/tests/pd10_read_model_guard.rs` protects the no-write/no-execution boundary.
- `.github/workflows/pd10-read-model.yml` runs the fixtures, source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the core read model and CI
fixtures are present, while durable checkpoint storage, process restart and cross-process recovery
remain open.
