# CAP-15 typed patch plan and preview baseline

CAP-15 keeps one parser/overlay/precondition `PlannedPatch` for both a read-only
`apply_patch.preview` capability and the real commit path. The preview reports a digest-bound
operation list, both sides of moves, affected paths, before content hashes, patch byte/hunk counts
and `read_only=true`; it acquires no patch lock and performs no write. The commit returns the same
plan digest/path/before-hash projection after its existing journaled transaction.

Patch bytes, NULs and hunk count are bounded before planning. The existing in-memory overlay means
late malformed/context-mismatch hunks fail before any write; move source and destination enter the
same precondition/scope set. Handler path/data-policy checks are shared by preview and commit.

GitHub Actions runs the daemon preview/commit fixture, CAP-15 source guards and workspace
compilation. No local runtime tests or smoke commands were run.

## 2026-10-13 guard-scope regression

The CAP-15 no-write guard previously inspected only `preview_codex_patch` while requiring the
`read_only` projection marker emitted by its `patch_preview` helper. The guard now checks the
preview function for lock/commit absence and checks `patch_preview` separately for `read_only=true`.

```text
source_snapshot: current dirty worktree; kiana-core/tests/cap15_patch_plan_guard.rs; kiana-daemon/src/apply_patch.rs
worktree_status: CAP-15 remains 🔄 / feature_status=implemented / proof_level=source; only guard scope changed
command_argv: RED exact `patch_plan_never_turns_preview_into_a_write_path`; GREEN full `cap15_patch_plan_guard`; both locked/offline through the serial launcher
cwd·environment: repository root; one Cargo job/test thread, 32 MiB stack, stripped/non-incremental profile
fixture·cassette: RED failed because helper-owned `read_only` was searched in the preview body; GREEN 4/4
exit_code: RED 101 / max RSS 4055916 KiB / CPU 89% / swap 0; GREEN 0 / max RSS 141552 KiB / CPU 44% / swap 0
status_change: corrected source-guard scope; no production patch-plan semantics changed
proof-level_change: none; guard behavior evidence only
limitations: daemon runtime fixture, cross-file atomicity, crash recovery, physical filesystem guarantees and full workspace regression remain open
reviewer: root integration agent; assertions align with the production function that owns each marker
```
