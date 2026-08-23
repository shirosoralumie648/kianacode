# v1.0.3 Context — Personal folder workbench

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirements: personal launch UX (Codex / pi / dsh folder entry)
Chosen slice: cwd / `--workdir` / GUI folder picker on the same `DaemonHost` as `kiana run`. Not `kiana tui`, not dsh Web UI, not v1.x, not live provider.

## Classify

Phase, not spike. User-visible completion is: open a project folder (terminal cwd, file-manager `.desktop`, or GUI picker), type a request, and the owned harness works. Proof is cassette / fake-script write of `GOLDEN_PATH.txt` plus fail-closed untrusted writes.

This is not migrating `kiana tui`. This is not cloning dsh `web`. This is not opening v1.x / worktrees / SDK / IDE.

## Locked discuss decisions

Do not reopen v0.2–v1.0.2, TUI park, or TeamCreate/SendMessage.

1. **Product spine stays `DaemonHost`.** Empty `kiana` on a TTY is the folder workbench, not the legacy REPL. `kiana run` remains the one-shot script path and keeps default read-only sandbox.
2. **Workbench default sandbox is `workspace-write`.** Untrusted one-shot uses the same fail-closed code as `kiana run --sandbox workspace-write`: `workspace_write_requires_trusted_non_safe_profile`.
3. **GUI is a picker, not a product UI.** `kiana --pick-folder` / `kiana gui` use `KIANA_FOLDER_PICKER_CMD`, else zenity/kdialog/tkinter when DISPLAY/WAYLAND exist. No display → `pick_folder_unavailable`.
4. **File manager entry is `--workdir`.** `contrib/kiana.desktop` `Exec=kiana --workdir %f`. `install.sh` may copy it; `KIANA_SKIP_PATH_SETUP=1` skips unless `KIANA_DESKTOP_DIR` is set.
5. **Do not unpark TUI. Do not open v1.x.**

Demo:

```text
kiana --help lists --workdir and --pick-folder
untrusted workbench workspace-write does not create GOLDEN_PATH.txt
trusted --workdir cassette creates GOLDEN_PATH.txt containing hello
--pick-folder without display is pick_folder_unavailable
contrib/kiana.desktop targets --workdir %f
kiana tui stays parked
```

## Requirements this phase

Personal folder launch UX. REL-01/02/03 still true. REL-04 stays draft. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV remain regression.

## Frozen

- unparking / migrating `kiana tui` onto DaemonHost
- dsh Web UI clone
- expanding `kiana-tools` / SkillTool / P1-READ
- live provider / unsupported_streaming
- HTTP/SSE MCP as complete
- `scripts/release-smoke.sh` / SBOM / signing
- `~/.local/bin` production install
- Daily/Research as complete
- JointSymposium / staffing every COMPANY.md role
- TeamCreate / SendMessage
- formatting all of `cli.rs`
- worktrees / SDK / IDE
- opening v1.x
