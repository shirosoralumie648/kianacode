# CM-37 · Cache, index and retention maintenance baseline

This source slice defines a server-bound maintenance plan without adding a cleanup executor. It
keeps Event/Receipt references, retention policy and legal holds ahead of any future physical
operation.

## Source contract

- `MaintenanceCandidate` classifies index generations, summary/result artifacts, cache entries and
  tombstones. A delete-eligible candidate must be older than the current generation, expired, or
  below the safe tombstone cursor as appropriate; it must be `RetentionDisposition::Eligible`, not
  held, not orphaned, and have neither Event nor Receipt references.
- `MaintenanceDecision::ReportOrphan` requires explicit no-reference and recoverable evidence.
  Orphans are reported for recovery and can never be silently converted into deletion.
- `MemoryMaintenancePlan` binds retention-scan digest, source/policy/data epochs, observation
  time, quota, candidates and checked reclaim bytes. It reports whether the planned reclaim would
  satisfy quota and exposes read-only delete/orphan lists; it does not delete, rename, compact or
  advance a watermark.
- Existing `RetentionScan`/`LegalHoldReceipt`, `RetentionWatermark`, `IndexGenerationState` and
  `ContextCacheDecision` remain their respective policy, cursor, generation and cache authorities.

## CI-only fixtures

`.github/workflows/cm37-memory-maintenance.yml` runs formatting, the domain plan/fence fixture,
the Core source guard and workspace test-target compilation. The path filter includes current
CM-36 `kiana-domain/src/memory_workbench.rs` so the repository-wide formatting dependency is
covered by a fresh remote run. Local tests, builds, checks, clippy and smoke commands are
deliberately not run, and GitHub CI is not awaited.

## Evidence ceiling and limitations

The slice is `feature_status=partial` with `proof_level=source` plus CI wiring. No EventStore
retention scan adapter, index writer, ArtifactStore, disk quota monitor, cross-process orphan
recovery, physical deletion, durable compaction or live/physical proof is claimed. A planned
reclaim is not proof that an external file was removed or that a business result remains correct.
