# v0.2 Phase 3 Context — Durable receipts

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is: kill the process, start a new CLI, and still list the previous run's tool calls and changed files. A second run must append, not truncate the first.

## Locked discuss decisions

Do not reopen Company OS. Continue/cancel stay in-process. TUI, five departments, Symposium, and RAG stay closed.

1. **Storage engine:** JSONL append-only under `$KIANA_HOME/sessions/events.jsonl`. SQLite waits. Corrupt lines fail closed.
2. **Product path:** `DaemonHost::local()` uses `JsonlEventLog`. In-process tests may keep `MemoryEventLog`.
3. **Receipt CLI:** `kiana run --receipt <id> --json` reads disk facts. Do not wire legacy `kiana --resume` or the workflow `evidence` command to harness receipts.
4. **Live JSON:** `kiana run --json` includes `files_changed` and `capabilities` projected from the run's events. `files_changed` comes from structured `apply_patch` results, not guessed shell side effects.

## Demo

```bash
kiana trust .
kiana run --json --sandbox workspace-write -- "create GOLDEN_PATH.txt"
# capture session_id / run_id
kiana run --receipt "$RUN_ID" --json
```

## Requirements this phase

EVD-01, EVD-02, EVD-03.

## Frozen

- SQLite EventStore
- rewriting `kiana-commands` workflow evidence
- TUI / MCP
- five departments / Symposium / RAG
- expanding `kiana-tools`
