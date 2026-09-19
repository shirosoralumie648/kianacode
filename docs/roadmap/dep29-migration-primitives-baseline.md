# DEP-29 bounded migration primitives baseline (partial)

The domain now models the five ordered migration phases expand, backfill, verify, switch and
contract. A MigrationBatchPlan is bound to the registry and step digest, has a bounded batch and
item-digest list, derives its idempotency key, and can only advance a monotonic source cursor.
MigrationCheckpoint::advance is pure and replaying the same batch key is a no-op. Phase skips,
cursor rollback, missing verify before switch/contract, checksum drift, generation overflow and
out-of-bound batches fail closed.

Targets are intentionally limited to store, projection, artifact and config. There is no provider,
shell or MCP target and the primitive module has no runtime, filesystem or broker dependency.

DEP-29 remains partial with source/static evidence only. The primitives do not apply rows,
backfill an index, switch a live reader, acquire a lock, persist a checkpoint or emit a
MigrationRecord. The durable runner, CAS/fence and old-reader compatibility remain DEP-30+.
