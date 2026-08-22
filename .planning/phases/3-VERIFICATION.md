# v0.2 Phase 3 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

```bash
kiana trust .
kiana run --json --sandbox workspace-write -- "create GOLDEN_PATH.txt"
# capture run_id
kiana run --receipt "$RUN_ID" --json
```

A new CLI process reads the same disk facts. A second run appends; it does not truncate the first receipt.

Storage is append-only JSONL at `$KIANA_HOME/sessions/events.jsonl`. `DaemonHost::local()` uses `JsonlEventLog`. In-process tests may keep `MemoryEventLog`.

## Evidence

| Criterion | Result |
|---|---|
| Tool calls and file changes land on disk | `JsonlEventLog` appends; CLI golden path asserts `$KIANA_HOME/sessions/events.jsonl` exists |
| User can list files changed this run | live `--json` and `--receipt` project `files_changed` from structured `apply_patch` `changed[].path` |
| Restart keeps the first receipt; second run does not overwrite | `disk_receipts_survive_restart_and_do_not_overwrite_the_first_run` |
| Product path is disk, not memory | `DaemonHost::local()` opens `JsonlEventLog::open_default()` |
| Corrupt lines fail closed | `jsonl_corrupt_line_fails_closed` |
| Unknown receipt is visible | CLI `receipt_unknown_session_fails_closed` → `receipt_not_found` |
| Legacy `--resume` stays legacy | `cli_resume.rs` still green; not wired to harness receipts |

## Commands run

```
cargo test -p kiana-eventlog --locked
cargo test -p kiana-protocol --locked
cargo test -p kiana-client --locked
cargo test -p kiana-core --test control_plane --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_run --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_resume --locked -- --test-threads=1
```

All listed targets passed (4 / 4 / 2 / 14 / 12 / 9 / 3).

## Not claimed

- SQLite EventStore
- TUI cancel / TUI receipts (Phase 4)
- live provider
- physical/target readiness
- five departments / Symposium / RAG
- rewriting `kiana-commands` workflow evidence
