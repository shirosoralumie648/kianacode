# PD-15 Workspace Patch / Checkpoint / Artifact Baseline

## Scope

PD-15 records the existing workspace patch/checkpoint boundary. `WorkspaceCheckpoint` binds
project/actor/session/role/run/invocation, path allow, transcript offset, data epoch and workspace
revision. Capture is read-only, preview does not write, and restore rechecks exact revision,
company scope, data epoch, path containment and EventStore checkpoint facts before routing a
brokered filesystem restore; the transaction invalidates old approvals/runner context and emits a
restored fact. Artifact references are revalidated for immutable content hash/scope before company
evidence use.

This step does not treat a checkpoint as a backup, does not follow symlinks or current paths, and
does not restore from transcript/UI state or create a direct undo executor.

## Evidence and limits

- `kiana-core/tests/pd15_workspace_artifact_guard.rs` protects checkpoint/revision/data-epoch/
  company/approval fences, artifact ref validation and no-direct-restore boundaries.
- `.github/workflows/pd15-workspace-artifact.yml` runs the source guard and workspace compilation
  in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: source guard and CI compile are
present, while durable ArtifactStore integration, power-loss/cross-process checkpoint recovery and
physical filesystem undo proof remain open.
