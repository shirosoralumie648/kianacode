# v0.2 Phase 1 Context — CLI golden path

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is a trusted local repo writing a file through `DaemonHost` → `KianaHarness`.

## Locked discuss decisions

Do not reopen Company OS. Five departments, Symposium, RAG, MCP, and TUI stay closed.

1. **Non-interactive LocalWrite:** A. Trusted project + `--sandbox workspace-write` auto-Allows in-workspace `LocalWrite`. Safe profile still Asks. Untrusted still Denies `project_untrusted`. Secret / sensitive / External / Critical still Ask. No `--yolo`.
2. **Provider proof:** CI uses cassette playback. `KIANA_HARNESS_SCRIPT` is routing regression plus recorded tool-call playback; live provider is optional and does not raise the proof level.
3. **Success shape:** File created by `apply_patch` or `shell` inside project root. JSON must report `harness: kiana-harness`.
4. **Role entry:** Keep `kiana run`. Receipts force `role_id=builder` and `department_id=executing`. `RoleSpec` / `DepartmentSpec` types land now; they do not drive five-department behavior.

## Demo

```bash
kiana trust .
kiana run --sandbox workspace-write -- "create a file named GOLDEN_PATH.txt containing hello"
test -f GOLDEN_PATH.txt
```

`kiana trust .` is an alias for `kiana trust trust`.

## Requirements this phase

PATH-01..04, TRUST-01..03, ROLE-01..03.

## Frozen

- `kiana-tools/**` new tools
- expanding `kiana-entrypoints/src/runner.rs`
- TUI / MCP / chrome / computer-use
- TeamCreate / SendMessage
- restoring the 24-phase corpus
