# v0.2 Phase 2 Context — Session continue / cancel / visible failure

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is the same harness session continuing, a cancel that stops in-flight shell/patch, and provider/policy misses that never report `completed`.

## Locked discuss decisions

Do not reopen Company OS. Five departments, Symposium, RAG, MCP, TUI, and disk EventStore stay closed.

1. **Session storage:** In-process `ActiveRun` keyed by `run_id`, plus a daemon/control-plane session index. Disk receipts / JSONL EventStore wait for Phase 3. Cross-process continue without a live `DaemonHost` is fail-closed `session_not_found` / `run_not_found`, not a silent new run.
2. **Legacy resume stays legacy:** `kiana --resume` / `cli_resume.rs` keep reading SDK `events.jsonl`. Do not wire them to `runner.rs` or claim they are harness sessions. New commands are `kiana run --continue <id>` and `kiana run --cancel <id>`.
3. **Continue identity:** Same `DaemonHost` lifetime. First `start_run` returns `run_id` + `session_id`. Continue appends a user message onto that `ActiveRun.messages`. Missing id never starts a new run.
4. **Cancel:** Must abort in-flight brokered `shell.exec` (child `kill_on_drop`) and drop the stored run so no later write lands. Completed runs can be cancelled to forget them.

## Demo

```bash
kiana run --json --sandbox workspace-write -- "create GOLDEN_PATH.txt"
# capture run_id from JSON
kiana run --continue "$RUN_ID" --json -- "append a second line"
kiana run --cancel "$RUN_ID" --json
```

Same-process proof is the completion definition. A second CLI process against a dead host must fail closed.

## Requirements this phase

SESS-01, SESS-02, SESS-03, TRUST-03.

## Frozen

- `kiana-tools/**` new tools
- expanding `kiana-entrypoints/src/runner.rs`
- TUI / MCP / chrome / computer-use
- disk EventStore / JSONL receipts
- TeamCreate / SendMessage
- five departments / Symposium / RAG
