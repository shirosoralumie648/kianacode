# SC-23 deletion request, tombstone and data-epoch baseline

## Delivered source slice

- `kiana-domain` defines typed `DeleteRequest`, immutable `DeletionTombstone`,
  `DeletionManifest`, `DeletionPropagation`, and `DeletionPlan` contracts.
- `kiana-core::plan_deletion` consumes only an exact SC-22 retention scan. Held, unknown,
  retained, cross-project, stale-revision, stale-epoch and stale-cursor targets are denied.
- A valid plan advances `data_epoch` exactly once, creates digest-only tombstones, and starts
  every receipt-less propagation target as `Unknown`.
- `RetentionStorePort` now has typed tombstone/manifest methods. `MemoryRetentionStore` records
  tombstones before manifests, enforces idempotent digest/CAS and epoch monotonicity, and never
  reports an unconfirmed external target as completed.

## Boundary and proof ceiling

The source EventLog remains append-only; tombstones do not rewrite historical event payloads.
The current adapter is in-memory and records deletion facts/unknown propagation only. Physical
crypto erase, filesystem/index/cache/memory/backup/external deletion, restart recovery and live
business/compliance receipts are not claimed by SC-23.
