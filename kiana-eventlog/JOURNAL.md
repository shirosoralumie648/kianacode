# Atomic authority journal

This adapter implements the storage portion of roadmap CP-06/07/27/28. The
ControlPlane must still build an authorized transition and include every authority,
approval, budget, lease and business dependency in its read set. Storage does not
infer missing dependencies or issue execution permits.

## Contract

`EventStorePort` declares `supports_atomic_transitions()` and `capabilities()`.
Default adapters report no support and return explicit unsupported errors from
`commit_transition`, `read_command`, and `read_from`; there is no loop of single
event appends masquerading as a transaction.

`TransitionBatch` contains a stable `RequestId` command ID, its immutable command
intent SHA-256 digest (64 lowercase hexadecimal characters), a unique list of
`AggregateVersion { aggregate_type, aggregate_id, version }`, and complete
`RuntimeEvent` values. Every write stream must be in the read set. New stream
versions must be contiguous, beginning at expected version + 1. Read-only
requirements are checked in the same lock as writes. Aggregate identities and
limits are validated before any effect on the journal.

A successful `CommandReceipt` contains the command ID and digest, commit ID, first
and last logical cursors, ordered event IDs, and final versions of every read
set member. `CommitOutcome` is `Committed { receipt }`, `Replayed { original }`,
`Conflict { changed }`, or `Unknown { command_id, reason }`. Read-set conflicts
report current versions and append nothing. The ControlPlane must reload and
reauthorize; replacing expected versions on an old decision is invalid.

The same command ID and digest returns the original receipt even after streams
advance. The same command ID with a different digest returns
`PortError::Conflict("event_store_command_digest_mismatch")`. Digest ownership
belongs to the immutable command intent; it is not recomputed from a fresh
random event ID or changed authorization result. Retrying a committed command
never appends the newly supplied event list.

`read_command(&RequestId)` confirms the original receipt. An unknown commit does
not permit dispatch. Async task cancellation can lose the response while its
blocking storage operation continues; callers must confirm the same command ID
and must not generate a replacement command to hide uncertainty.

`read_from(cursor, limit)` uses logical event cursors, where zero is the beginning
and every nonzero accepted cursor is a complete commit boundary. Pages never split
a transaction; the first complete batch may exceed the requested count. Stream
and request reads use indexes, and JSONL refresh reads only the appended tail.
`read_all` remains a compatibility/export operation, not the recommended hot path.

## JSONL format, upgrade and recovery

Legacy single-event logs remain readable. Compatibility-only appends preserve
the existing raw event format until the first atomic transition. That transition
first appends and synchronizes a required `kiana.journal-header.v2` writer marker.
After the marker, all records use `kiana.transition-frame.v1` frames, including
single-event compatibility calls. Existing legacy bytes are retained. An old
adapter cannot decode the required marker as a RuntimeEvent and therefore refuses
to append; all cooperating writers use the same process lock file.

Each frame contains the entire event list and receipt, format version, canonical
body byte length and SHA-256 digest. Canonical object keys are sorted recursively.
The reader validates the complete frame, strict shape, IDs, read-set CAS,
contiguous versions, receipt and bounds before publishing any of its events.
Unknown required records, duplicate writer headers, legacy records after the
upgrade marker, complete malformed lines and checksum mismatches fail closed.
A truncated JSON tail is repairable only after a known complete legacy record or
writer header. Complete final JSON without a newline is validated before the
newline is repaired. A malformed first record is rejected, not treated as empty.

The Unix writer holds one `flock` across refreshing disk state, command dedup,
read-set checking, encoding, append and synchronization. It opens the parent and
files with no-follow flags, pins the directory, and verifies file identities.
New parent directories and file creation receive directory synchronization. A
commit is returned only after `write_all`, file `sync_all`, parent `sync_all`, and
identity checks. No fallible operation occurs between publishing the byte cursor
and publishing the corresponding in-memory command receipt. Write, sync, or lost
worker outcomes return Unknown. Recovery synchronizes newly observed complete
frames before returning them; command confirmation also synchronizes the journal.

Before upgrading a production journal, stop legacy writers and retain an export
or copy of the old file. Downgrade cannot write the upgraded file with an old
binary; keep it for read-only access with a capable reader. This adapter does not
migrate separate approval files or grant execution authority to legacy records.
Only a subsequent authorized ControlPlane transition can do that.

## Resource and platform boundaries

- Frame: 4 MiB; event: 1 MiB; transition: 256 events; read set: 1,024 aggregates.
- Cursor page request: 1–4,096 events, with whole-transaction boundaries.
- JSONL file: 512 MiB; logical journal: 1,000,000 events. Capacity failures have
  stable error codes and do not append a partial new transaction.
- Each JsonlEventLog instance admits at most 16 blocking storage operations;
  saturation returns `eventlog_worker_queue_full` instead of an unbounded wait.
- Async trait methods use `spawn_blocking` for file operations, flock and sync.
  Runtime composition should call `open_async` / `open_default_async`; synchronous
  `open` methods remain for startup compatibility.
- MemoryEventLog provides atomicity, command dedup and the same transaction/frame
  validation, but explicitly reports no durable commits. Unix JSONL reports atomic
  durable commit capability. Non-Unix JSONL rejects atomic transitions because it
  does not provide an equivalent cross-process lock and directory-sync guarantee.

A checksum detects corruption, not malicious replacement by a privileged writer.
The incremental cache assumes cooperating append-only writers; reopening validates
all retained records. File replacement/truncation and a same-size mtime change
trigger identity rejection or full prefix revalidation. The adapter does not
promise distributed transactions, network-filesystem lock semantics, or external
effects exactly once. Persisted safe fields and protected payload references are
the responsibility of the ControlPlane; this generic store does not redact or
silently rewrite authorization payloads.

## Evidence

Implementation evidence is source and `cargo check -p kiana-eventlog --lib --locked
--offline` in the isolated checkout. No tests were added or run, as requested.
No fault-injection, process-crash, physical power-loss, latency or cross-platform
behavior has been demonstrated by this slice. Static compilation is not a durable
or live proof level.
