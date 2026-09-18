# PD-06 JSONL v2 Durability / Recovery Baseline

## Scope

PD-06 records CI evidence for the existing JSONL v2 EventStore boundary: checksummed JournalFrame
and JournalHeader, append-only/no-follow symlink open, process lock, file/directory sync, bounded
blocking workers, cached file identity, and torn-tail-only repair. A checksum/malformed middle
frame remains corruption; only an incomplete final line is truncated after validation and sync.

The fixture does not claim a new implementation or physical disk guarantee beyond the adapter's
capability declaration. Durable cross-process and kill-9 evidence remains bounded by the host/CI
environment and the later PD-07+ recovery matrix.

## Evidence and limits

- `kiana-eventlog/tests/pd06_jsonl_recovery.rs` covers torn-tail repair, checksum corruption
  rejection and source markers for lock/sync/bounded recovery.
- `kiana-core/tests/pd06_jsonl_guard.rs` protects the EventStore-only/fail-closed boundary.
  GitHub Actions runs fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no claim of physical durability,
kill-9 power-loss recovery, multi-host filesystem semantics or complete PD-07 indexing is made.
