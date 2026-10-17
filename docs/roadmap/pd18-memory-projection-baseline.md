# PD-18 Memory projection baseline

## Scope

PD-18 makes the projected Memory read surface obey the same server-owned governance epoch and
scope that produced the projection: a row is filtered before ranking or prompt-visible retrieval,
and a committed tombstone cannot be revived by a surviving copy on any derived retrieval surface.
This contract does not mutate EventLog facts, JSONL rows, indexes or caches.

## Implemented source slice

- `MemoryProjectionFence` binds record id/revision, requested project, policy revision and data
  epoch to a digestable allow/deny decision.
- Candidate, rejected/tombstoned, stale-epoch, cross-project, record-retention, revoked-source and
  expired-grant-retention rows are denied before lexical ranking; approved active rows remain
  searchable.
- The daemon applies the fence in `memory.search` and returns the server policy `data_epoch` in the
  result envelope so callers cannot mistake a stale projection for an empty store.
- The core governance projector rechecks the same epoch contract after rebuilding the governance
  snapshot; no second authority or index/cache execution path is added.

## 2026-10-17 completion slice (deletion index linkage, CI-only)

- `kiana-domain/src/memory_deletion_index.rs` adds the read-time deletion index:
  - `MemoryRetrievalSurface` names the four surfaces the card cares about
    (`lexical`, `vector`, `cache`, `receipt`).
  - `MemorySurfaceReceipt` may only claim `Invalidated` together with an `observed_at_ms` and a
    `sha256:` evidence digest; the honest default stays `Unknown` (no receipt, no acknowledgement).
  - `MemoryDeletionEntry` is a per-object tombstone (`record_id`, `collection`, `record_revision`,
    `tombstone_digest`, `deleted_at_ms`) that covers the deleted revision and every later one, and
    reports `pending_surfaces()` until each surface has produced evidence.
  - `MemoryDeletionIndex` is project- and epoch-bound (`bind_policy` → `memory_deletion_index_epoch_stale`
    / `memory_deletion_index_policy_ahead` / `deletion_index_project_mismatch`), digests itself and
    answers `check_read` with `memory_deletion_tombstoned` or
    `memory_deletion_tombstone_revival_denied:<surface>`. `merge_memory_deletion_entries` never drops
    a receipt when an older index is re-read.
- `MemoryProjectionFence::evaluate` now takes the deletion index and denies covered rows, records the
  index digest in the fence, and additionally denies rows past their own record retention
  (`retention_expired`).
- The daemon builds the index in `ensure_memory_projection` from the committed memory stream: rows
  whose newest committed fact is a `delete`, and rows the projection already marks `Denied`, become
  tombstones with all four surfaces pending; a persisted index (written by the governance path) is
  validated, epoch-bound and merged in, and a stale/foreign/tampered index fails the search closed
  (`memory_deletion_index_stale:*`, `memory_deletion_index_project_mismatch`) instead of returning an
  empty green result. `memory.search` reports the `deletion_index` block (schema, epoch, digest,
  tombstones, pending surfaces, receipt `clean`/`partial`/`fenced`/`unjournaled`).
- `data.governance` delete/revoke/expire records per-object tombstones before touching derived data,
  writes memory-deletion-index.json at the new epoch, and only marks lexical/vector/cache/receipt
  `Invalidated` after the purge and derived-store removal actually ran; its result envelope carries
  the index summary.
- The core governance projector rebuilds the same index from the event history
  (`project_memory_deletion_index`), binds it to the current epoch and downgrades
  `bm25-index`/`dense-index`/`cache`/`receipt` to `Unknown` while a tombstone receipt is still
  missing.

## Deny-first fixtures

- `kiana-domain/tests/pd18_memory_projection.rs`: deletion index covers only deleted revisions,
  revival denied while any surface is pending, `memory_deletion_tombstoned` once every surface has
  evidence, stale/ahead/foreign/tampered index denied, record-retention expiry denied, evidence
  digest required for an `Invalidated` claim, merge keeps the strongest receipt.
- `kiana-daemon/src/harness_memory.rs` (in-module, `#[cfg(unix)]`): a committed `delete` fact denies
  a row whose JSONL copy still looks searchable (index reports `partial` with four pending
  surfaces), and a persisted index denies a restored row while a stale-epoch index fails the read
  with `memory_deletion_index_stale:memory_deletion_index_epoch_stale`.
- `kiana-daemon/tests/pd18_memory_projection_guard.rs` and
  `kiana-core/tests/pd18_memory_projection_guard.rs`: source guards bind the fence, the daemon
  wiring, the governance purge/index writers and the domain evidence rules.

## CI wiring

The unified `.github/workflows/ci.yml` lane is the test authority: `kiana-domain-s4/4` already
claims `pd18_memory_projection`, `kiana-core-s5/6` claims `pd18_memory_projection_guard`, and the
`kiana-daemon` shard runs the whole crate (lib unit tests plus integration targets). No new workflow
and no new test target were added, so `scripts/ci/test-shards.json` is unchanged.

## Evidence boundary

GitHub Actions is the test authority for the focused domain fixture and the daemon/core fixtures and
guards. Local tests/build/check/clippy/smoke are intentionally not run in this session (only
`cargo fmt --all`); the CI receipt for this commit is not awaited, so proof level stays `source`.
This step does not claim durable index/vector/cache deletion, cross-process projector recovery, live
policy effects, physical erasure correctness, or that every derived store in the repository has been
migrated to the new deletion index.
