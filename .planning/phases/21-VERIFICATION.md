# v1.0.3 Verification — Personal folder workbench

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: personal Codex/pi/dsh folder launch on DaemonHost
Chosen slice: cwd / `--workdir` / `--pick-folder` workbench; TUI stays parked; v1.x not opened

## Demo contract

```text
kiana --help lists --workdir and --pick-folder
untrusted workbench workspace-write does not create GOLDEN_PATH.txt
trusted --workdir cassette creates GOLDEN_PATH.txt containing hello
--pick-folder without display is pick_folder_unavailable
contrib/kiana.desktop targets --workdir %f
kiana tui stays parked
```

Workbench default sandbox is `workspace-write`. Untrusted writes fail closed
with the same code as `kiana run --sandbox workspace-write`:
`workspace_write_requires_trusted_non_safe_profile`. GUI is a folder picker,
not a dsh Web UI. Interactive TTY banner is not cassette-proven; one-shot
`--json` is the gate.

## Evidence

| Criterion | Result |
|---|---|
| help lists workdir/picker | `cli_workbench::top_level_help_lists_workdir_and_picker` + `workbench --help` |
| untrusted write fail-closed | `untrusted_workbench_write_fails_closed`; no `GOLDEN_PATH.txt` |
| trusted `--workdir` cassette write | `trusted_workbench_cassette_creates_golden_path_file`; `files_changed` |
| picker without display | `pick_folder_without_display_fails_closed` → `pick_folder_unavailable` |
| picker command write | `pick_folder_command_then_one_shot_write` via `KIANA_FOLDER_PICKER_CMD` |
| parse unit tests | `workbench::tests::{path_like_args_are_workdirs, parse_workdir_and_prompt, gui_selects_picker}` |
| desktop file | `contrib/kiana.desktop` `Exec=kiana --workdir %f` |
| `kiana run` not broken | `cli_run::trusted_workspace_write_cassette_creates_golden_path_file` |
| smoke gate | `scripts/v10-workbench-smoke.sh` printed `ok` |

## Commands run

```
cargo test -p kiana-entrypoints --test cli_workbench --locked --offline -- --test-threads=1
cargo test -p kiana-entrypoints --lib --locked --offline workbench
bash -n scripts/v10-workbench-smoke.sh
bash scripts/v10-workbench-smoke.sh
cargo test -p kiana-entrypoints --test cli_run --locked --offline trusted_workspace_write_cassette_creates_golden_path_file -- --test-threads=1
```

All listed commands passed. Did not `cargo fmt --all`, did not format all of
`cli.rs`, did not run `scripts/release-smoke.sh`, did not use a live provider,
did not unpark `kiana tui`, did not open v1.x.

## Not claimed

- live provider / `unsupported_streaming`
- interactive TTY rustyline session (one-shot cassette only)
- zenity/kdialog/tkinter on a real desktop (picker command cassette only)
- `kiana tui` on DaemonHost
- dsh Web UI
- `~/.local/bin` production install / home desktop unless opted in
- packaged tarball / SBOM / signing
- Daily/Research packs (REL-04 stays draft)
- worktrees / SDK / IDE / Desktop app / Web
- JointSymposium / staffing every COMPANY.md role
- TeamCreate / SendMessage
- v1.x enterprise
