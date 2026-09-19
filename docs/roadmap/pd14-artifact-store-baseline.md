# PD-14 Immutable ArtifactStore Baseline

## Scope

PD-14 adds a non-durable `MemoryArtifactStore` implementation of `ArtifactStorePort` for CI
semantics. It enforces `ArtifactVersion` content hash/size/schema/provenance validation, separates
stage from commit, refuses duplicate version overwrite, and only reads/verifies committed bytes
against the exact immutable manifest and scope digest. No workspace path or symlink is followed.

This adapter is explicitly process-local and does not claim durable files, atomic rename/fsync,
cross-process locking, retention, backup, or physical TOCTOU evidence.

## Evidence and limits

- `kiana-eventlog/tests/pd14_artifact_store.rs` covers uncommitted read rejection, stage/commit/
  verify/read, duplicate version refusal, scope/hash/manifest drift and revision conflict.
- `kiana-eventlog/tests/pd14_artifact_guard.rs` protects the no-path/no-effect boundary.
- `.github/workflows/pd14-artifact-store.yml` runs the fixtures, source guard and workspace
  compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: the adapter and CI fixtures are
present, while durable ArtifactStore, manifest persistence, restart/cross-process recovery and
retention/backup remain open.
