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

## 2026-10-17 Denial and recovery fixtures close out the card (CI-only)

The card's deny column (torn middle, checksum, directory replacement, disk full, lock invalidation,
writer worker crash) and success column (kill-9/reopen keeps the complete prefix, observable
Flush/Shutdown ack, bounded throughput without silently dropping data) now each have a tree-resident
fixture. This slice adds only fixtures and guards; `kiana-eventlog/src/jsonl.rs` is unchanged.

- `jsonl_v2_rejects_a_corrupt_middle_record_without_truncating_or_exposing`: a corrupt record between
  two valid v2 records fails as `eventlog_corrupt` and the file is not truncated.
- `jsonl_v2_refuses_directory_replacement_after_open`: after the pinned parent is renamed and a
  same-named directory is recreated, `append` fails as `eventlog_parent_changed`; the moved parent
  keeps the committed prefix and the replacement directory receives nothing.
- `jsonl_v2_refuses_a_replaced_or_hardlinked_writer_lock`: replacing `.jsonl.lock` with a symlink or
  hardlink fails the next operation as `eventlog_symlink_rejected` / `eventlog_hardlink_rejected`.
- `jsonl_v2_refuses_an_over_capacity_log_before_exposing_any_fact`: a log longer than
  `MAX_JOURNAL_LOG_BYTES` (a bounded stand-in for disk full) is refused with `eventlog_disk_limit`
  before any fact is exposed.
- `jsonl_v2_flush_and_shutdown_acks_are_observable_and_close_rejects_writes`: `flush` returns a
  durable cursor, `close` returns `closed`, and post-close writes fail as `eventlog_closed`.
- `jsonl_v2_reopen_keeps_the_complete_prefix_and_repairs_only_the_torn_tail`: an incomplete v2 tail
  after a committed frame is truncated on reopen while the complete prefix is preserved byte for
  byte; a second reopen is stable.

`kiana-core/tests/pd06_jsonl_guard.rs` adds the matching fail-closed markers (parent / symlink /
hardlink / disk-limit / worker / closed / file-removed) and still rejects a widened fact source.

```text
source_snapshot: docs/roadmap.md §1.1 row 200 (🔄 → ✅, CI pending) and §3 changelog; kiana-eventlog/tests/pd06_jsonl_recovery.rs; kiana-core/tests/pd06_jsonl_guard.rs
worktree_status: row 200 🔄 → ✅ (CI pending); fixtures/guards and shared docs only; kiana-eventlog/src/jsonl.rs unchanged; no Cargo.toml/Cargo.lock/package.json change
command_argv: CI-only; no local cargo/make/npm/smoke run; .github/workflows/pd06-jsonl-recovery.yml runs pd06_jsonl_recovery and pd06_jsonl_guard with format and target-scoped --no-run
cwd·environment: worktree /workspace/kianacode-worktrees/PD-06, branch step/pd-06-20261017-1 (base master ce32b8741ae9); GitHub Actions ubuntu-latest is the runner (result not awaited)
fixture·cassette: pd06_jsonl_recovery +5 fixtures (corrupt middle, directory replacement, lock replacement, over-capacity, flush/close ack, v2 reopen prefix) and extended source markers; pd06_jsonl_guard +9 fail-closed markers
exit_code: not run locally; CI result not awaited
status_change: PD-06 deny and success columns are covered at the source/fixture level; row 200 ✅ (CI pending) / feature_status=implemented
proof-level_change: source only; no local_behavior/durable/live/physical promotion
limitations: fixtures run on ubuntu-latest/temp (not production roots); a sparse file stands in for a real ENOSPC; no physical power-loss, real kill -9, or cross-process flock contention is demonstrated; multi-process backpressure is PD-27, the fault matrix is PD-30, and the platform/filesystem matrix is PD-32; a checksum detects corruption, not privileged replacement
reviewer: root integration agent; PD-06 JSONL v2 frame/checksum/fsync/lock/tail-recovery static review
```
