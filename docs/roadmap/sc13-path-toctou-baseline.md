# SC-13 Path / TOCTOU Baseline

## Scope

SC-13 closes a source/CI evidence slice around the existing path mutation boundary. Patch and
workspace publication canonicalize the project root, reject symlink components and hardlinks,
capture metadata/content preconditions, and commit through descriptor-relative `openat`/`renameat`
operations with `O_NOFOLLOW`. Execution workspaces re-check opened file identity, the sandbox
rejects external symlink traversal, durable path locks serialize overlapping mutations, and the
authority fence/broker rechecks keep stale scope or permit state from reaching an effect handler.

The source guard does not create a new filesystem executor or authorization path. Existing daemon
fixtures remain the runtime owners for rename, replacement, rollback, symlink and hardlink cases.

## Evidence and limits

- `kiana-core/tests/sc13_path_toctou_guard.rs` pins root-relative, inode/generation, lock, fence,
  broker-recheck and zero-free-message markers in CI.
- GitHub Actions runs the source guard and workspace compile; local tests are intentionally not
  executed.

This slice is `feature_status=implemented`, `proof_level=source`: cross-process crash recovery,
physical filesystem durability, endpoint/secret fences and external/live effect proof remain
SC-14+ / PD / ER work.
