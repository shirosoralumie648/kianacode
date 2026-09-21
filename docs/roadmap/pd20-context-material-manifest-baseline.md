# PD-20 Context material manifest baseline

## Scope

PD-20 binds RepoMap, context artifacts and their dependency graph to one persistent, source-bound
manifest. The manifest records canonical root, source cursor, tool version and content hashes;
readers can reject foreign roots, tampered bytes or stale manifests before rebuilding context.

## Implemented source slice

- `ContextMaterialManifest` composes the existing RepoMap, artifact store and dependency graph
  builders under one root and one digest.
- RepoMap/artifact paths are checked as relative bounded paths, content hashes are validated and
  root mismatches are rejected before publication or read.
- The manifest is written through the same temp-file sync + atomic rename helper used by PD-19.
- The manifest is a query projection only and never grants capabilities or replaces ControlPlane,
  EventLog or ArtifactStore authority.

## Evidence boundary

GitHub Actions is the test authority for the focused query fixtures and source guard. Local tests
are intentionally not run. This step proves source and remote-CI wiring only; it does not claim
durable cross-process manifest leases, external artifact storage, backup/restore or live effects.
