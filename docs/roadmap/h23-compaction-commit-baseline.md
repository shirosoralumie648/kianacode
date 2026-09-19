# H23 compaction artifact and commit baseline

## Delivered source slice

- `CompactionArtifact` validates summary/artifact digests, source cursor range and content size
  before a view commit can reference it.
- `CompactionCommit` binds the artifact and summary to source event IDs/cursor, old/new context
  revisions, prompt/profile/budget digests, workspace revision and data epoch.
- `validate_before_commit` rejects concurrent steering/source changes, workspace changes and data
  revocation before the compacted view advances; the old context remains the safe fallback.
- CI source guard connects the contract to the existing runner checkpoint/prompt provenance and
  Compacted event path without adding a second event loop.

## Boundary and proof ceiling

This is a typed artifact-first/CAS contract. It does not claim a durable artifact adapter,
cross-store two-phase commit, model-generated summary retry, physical GC or restart recovery; those
remain later H/PD/SC work.
