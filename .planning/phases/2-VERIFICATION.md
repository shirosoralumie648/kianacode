# v0.2 Phase 2 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

Same `DaemonHost` lifetime:

```bash
# start returns session_id + run_id
kiana run --json --sandbox workspace-write -- "create GOLDEN_PATH.txt"
# continue appends onto the in-process ActiveRun
kiana run --continue "$RUN_ID" --json -- "append a second line"
# cancel aborts in-flight shell.exec (kill_on_drop)
kiana run --cancel "$RUN_ID" --json
```

Cross-process continue/cancel against a new CLI `DaemonHost` is fail-closed `session_not_found` / `run_not_found`. That is the completion definition, not a bug.

## Evidence

| Criterion | Result |
|---|---|
| Continue reuses the same harness run on one host | `continue_on_the_same_host_reuses_the_run` (daemon), `continue_run_reuses_the_same_run_id` (core) |
| Missing session does not silently start a new run | `continue_unknown_session_does_not_start_a_new_run`, CLI `continue_unknown_session_fails_closed` |
| Cancel stops in-flight `shell.exec` before it writes | `cancel_stops_in_flight_shell_before_it_writes` — `CANCELLED.txt` absent |
| Cancel unknown id is visible | `cancel_unknown_run_fails_closed`, CLI `cancel_unknown_session_fails_closed` |
| Empty prompt / no model / untrusted write stay fail-closed | existing daemon + CLI tests; JSON `status != completed` |
| Legacy `--resume` stays legacy | `cli_resume.rs` still green; does not claim `harness: kiana-harness` |

## Commands run

```
cargo test -p kiana-core --test control_plane --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_run --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_resume --locked -- --test-threads=1
```

All listed targets passed (14 / 11 / 8 / 3).

## Not claimed

- disk EventStore / JSONL receipts (Phase 3)
- TUI cancel UX (Phase 4)
- live provider
- physical/target readiness
- five departments / Symposium / RAG
