# v0.2 Phase 4 Context — TUI parked

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is a written, test-locked statement that `kiana tui` is not the v0.2 product path.

## Locked discuss decisions

Do not reopen Company OS. Receipts, continue/cancel, and the Builder write path stay as Phase 1–3 left them.

1. **Park, do not migrate.** `kiana tui` remains on the legacy SDK/stream spine (`tui.rs` → `unstable_v2_prompt_streaming_*`). v0.2 product path stays `kiana run` / print → `DaemonHost` → `KianaHarness`.
2. **Why park:** wiring TUI to `KianaClient` now is a second runtime migration, not a golden-path fix. PROCESS.md and DESIGN.md already allow park. Parking unblocks v0.3 (planning + executing + one symposium).
3. **Park is not deletion.** The TUI binary still starts in a real terminal. It must not be used as PATH/SESS/EVD evidence. Do not add features in `kiana-tui/` to fake progress.
4. **Reopen later:** v0.3 `WB-03` may keep the park; v0.6 is the extra-surfaces station if TUI should share the harness.

## Demo

```bash
kiana --help | grep tui
# must say parked / not DaemonHost
kiana tui </dev/null
# fail closed without a TTY; must not print harness: kiana-harness
```

## Requirements this phase

SURF-01.

## Frozen

- migrating TUI onto DaemonHost
- expanding `kiana-tui/**` or `tui.rs` features
- MCP / chrome / computer-use
- five departments / Symposium / RAG
- expanding `kiana-tools`
