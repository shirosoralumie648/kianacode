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

## 2026-10-08 Transition event identity denial

`TransitionBatch::validate` now applies the same non-nil `event_id`/`request_id` requirement that
the standalone Event frame already enforced. A checksummed transition frame carrying a nil event
identity is rejected as `journal_event_identity_invalid` before receipt validation or journal
application.

```text
source_snapshot: current dirty worktree; kiana-domain/src/journal.rs; kiana-domain/tests/cp07_journal_frame.rs; kiana-core/tests/cp07_journal_guard.rs
worktree_status: PD-06 roadmap row 200 remains 🔄 / feature_status=implemented / proof_level=source; transition events now reject nil event/request identity; no JSONL writer, fsync, lock, manifest or lockfile change
command_argv: RED `cargo test -p kiana-domain --test cp07_journal_frame transition_frame_rejects_nil_command_and_receipt_commit_ids --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-domain --test cp07_journal_frame --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test cp07_journal_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo job/test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental test profile; RED max RSS 253264 KiB / 38% CPU; domain GREEN max RSS 83536 KiB / 16% CPU; Core guard max RSS 84456 KiB / 29% CPU; all measured runs reported 0 swaps
fixture·cassette: RED valid-checksum transition frames with nil event/request identity and nil command/commit identity were accepted; GREEN domain 5/5 covers complete transition/event recovery, checksum/nil standalone event, both nil transition event identities, nil command_id and nil receipt commit_id; Core guard 1/1 locks `journal_event_identity_invalid`, `journal_command_id_invalid` and `command_receipt_commit_id_invalid`
exit_code: RED 101 expected; domain/Core/fmt/diff 0
status_change: selected transition event identity denial moves source → local_behavior; whole PD-06 remains implemented/source
proof-level_change: local transition-frame validation only; no durable/live/physical promotion
limitations: duplicate-key wire parsing, fsync/power-loss/kill-9, cross-process contention, recovery/indexing and physical durability remain separate or open; this does not claim the JSONL adapter is durable
reviewer: root integration agent with PD-06 read-only journal audit; focused transition-frame identity reviewer
```

## 2026-10-08 Duplicate JSON key denial

The JSONL reader now parses each record with a recursive `StrictValueSeed`/visitor before schema
dispatch. Repeated object keys, including keys nested inside a JournalFrame body, are rejected as
`eventlog_corrupt` instead of being silently collapsed to the last value by `serde_json::Value`.

```text
source_snapshot: current dirty worktree; kiana-eventlog/src/jsonl.rs; kiana-eventlog/tests/pd06_jsonl_recovery.rs; kiana-core/tests/pd06_jsonl_guard.rs
worktree_status: PD-06 row 200 remains 🔄 / feature_status=implemented / proof_level=source; JSONL duplicate-key denial is locally verified for top-level and nested frame objects; no manifest/lockfile, writer/fsync/lock architecture or second fact source changed
command_argv: RED `cargo test -p kiana-eventlog --test pd06_jsonl_recovery jsonl_v2_rejects_duplicate_wire_keys_before_header_or_frame_decode --locked --offline -- --exact --test-threads=1`; focused GREEN same selector; PD-06 `cargo test -p kiana-eventlog --test pd06_jsonl_recovery --locked --offline -- --test-threads=1`; ER-05 `cargo test -p kiana-eventlog --test er05_jsonl_v2 --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test pd06_jsonl_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental test profile; RED max RSS 425424 KiB / 94% CPU; focused GREEN max RSS 281400 KiB / 102% CPU; PD-06 final rerun max RSS 427364 KiB / 86% CPU; ER-05 max RSS 279440 KiB / 105% CPU; Core guard final rerun max RSS 84468 KiB / 30% CPU; every measured command reported 0 swaps
fixture·cassette: RED old `serde_json::from_slice::<Value>` accepted duplicate `schema`; GREEN rejects duplicate top-level header keys and duplicate nested `body.type`; PD-06 3/3 and ER-05 4/4 preserve torn-tail, checksum, lock, sync, transition and reopen behavior; Core guard 1/1 pins `parse_json_value_reject_duplicates` and `StrictValueSeed`
exit_code: RED 101 expected; focused/PD-06/ER-05/Core guard 0
status_change: JSONL duplicate-key denial moves source → local_behavior; whole PD-06 remains implemented/source
proof-level_change: recursive parser refusal is locally verified; no durable/live/physical promotion
limitations: duplicate-key rejection is scoped to this JSONL parser; duplicate handling in other wire boundaries, fsync/power-loss/kill-9, cross-process contention, recovery/indexing and physical durability remain separate or open; this does not claim the JSONL adapter is durable
reviewer: root integration agent with PD-06 read-only storage audit; focused JSONL duplicate-key reviewer
```
