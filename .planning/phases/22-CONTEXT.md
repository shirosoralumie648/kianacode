# v1.0.4 Context — Workbench conversation surface

Date: 2026-08-24
Status: locked
Proof ceiling: `local_behavior`
Requirements: Codex/pi-shaped TTY conversation (transcript + input + status)
Chosen slice: `workbench_chat` on the same `DaemonHost` as `kiana run`. Not `kiana tui`, not token streaming, not v1.x, not live provider.

## Classify

Phase, not spike. User-visible completion is: on a TTY, `kiana` shows a conversation pane, an input box, and a status line. A request still goes through `DaemonHost`. Proof is view/slash/key unit tests plus the existing one-shot `--json` cassette write of `GOLDEN_PATH.txt`.

This is not migrating `kiana tui`. This is not claiming `unsupported_streaming`. This is not opening v1.x / worktrees / SDK / IDE.

## Locked discuss decisions

Do not reopen v0.2–v1.0.3, TUI park, or TeamCreate/SendMessage.

1. **Product spine stays `DaemonHost`.** Conversation lives in `kiana-entrypoints/src/workbench_chat.rs`, wired from `workbench.rs`. `kiana tui` stays parked on the legacy SDK stream.
2. **`--json` and non-TTY stay one-shot.** Cassette tests must not enter alt-screen. `KIANA_WORKBENCH_PLAIN=1` keeps rustyline.
3. **`/sandbox` actually switches.** `read-only` / `workspace-write` update the next turn. `danger-full-access` is `danger_full_access_rejected`. Empty `/sandbox` shows the current value.
4. **Esc / Ctrl-C cancel the running turn** via same-host `cancel_envelope_on_host`. Do not use a gold-path cassette to prove mid-tool abort; cassette turns finish instantly.
5. **Do not claim token streaming.** Status line is idle/running. `model_client` remains `stream: Some(false)`.
6. **Do not unpark TUI. Do not open v1.x.**

Demo:

```text
status line names folder / trusted / sandbox / idle|running
/sandbox read-only|workspace-write actually switches
danger-full-access is rejected
Esc cancels only while running
--json cassette still writes GOLDEN_PATH.txt
kiana tui stays parked
```

## Requirements this phase

Personal conversation surface. REL-01/02/03 still true. REL-04 stays draft. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV remain regression.

## Frozen

- unparking / migrating `kiana tui` onto DaemonHost
- token streaming / `unsupported_streaming` as complete
- permission popups / `@file` / product slash layer / rewind / cross-process resume
- login / model picker / IDE / Desktop / cloud
- dsh Web UI clone
- expanding `kiana-tools` / SkillTool / P1-READ
- live provider
- HTTP/SSE MCP as complete
- `scripts/release-smoke.sh` / SBOM / signing
- `~/.local/bin` production install
- Daily/Research as complete
- JointSymposium / staffing every COMPANY.md role
- TeamCreate / SendMessage
- formatting all of `cli.rs`
- worktrees / SDK / IDE
- opening v1.x
