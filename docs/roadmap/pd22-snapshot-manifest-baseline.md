# PD-22 Snapshot manifest baseline

## Scope

PD-22 defines the sealed metadata boundary for full and incremental backups. A snapshot carries
owner/store/instance identity, active and backup roots, file hashes, source cursor, data epoch,
quiesce/WAL seals and an immutable manifest digest. It does not copy files or activate a restore.

## Implemented source slice

- `SnapshotManifest` supports full and incremental modes with deterministic file ordering and
  content-hash seals.
- Full snapshots reject a previous snapshot reference; incremental snapshots require one.
- Unquiesced or unsealed WAL state, missing cursor/epoch, unsafe backup roots and invalid hashes
  fail closed before any adapter effect.
- Daemon/EventLog guards keep the manifest as metadata over the existing EventStore authority; no
  second backup or execution loop is added.

## Evidence boundary

GitHub Actions is the test authority for the focused domain fixture and daemon/eventlog source
guards. Local tests are intentionally not run. This step does not claim physical file copying,
fsync durability across crashes, restore activation, or external effect reconciliation.
