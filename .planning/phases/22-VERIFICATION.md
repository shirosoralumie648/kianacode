# v1.0.4 Verification — Workbench conversation surface

Date: 2026-08-24
Proof: `local_behavior`
Verdict: pass
Requirement: Codex/pi-shaped TTY conversation on DaemonHost
Chosen slice: transcript + input + status in `workbench_chat`; TUI stays parked; streaming / v1.x not claimed

## Demo contract

```text
status line names folder / trusted / sandbox / idle|running
/sandbox read-only|workspace-write actually switches
danger-full-access is rejected
Esc cancels only while running
--json cassette still writes GOLDEN_PATH.txt
kiana tui stays parked
```

TTY `kiana` now uses `workbench_chat` (conversation / input / status). `--json`
and non-TTY stay the v1.0.3 one-shot path so cassette tests never enter
alt-screen. `KIANA_WORKBENCH_PLAIN=1` keeps rustyline. Esc/Ctrl-C map to
same-host `cancel_run`; that mapping is unit-tested, not cassette-proven as a
mid-tool abort. Token streaming is not claimed.

## Evidence

| Criterion | Result |
|---|---|
| status line | `workbench_chat::tests::status_line_names_folder_trust_and_sandbox` |
| `/sandbox` switches / shows / rejects danger | `sandbox_slash_actually_switches` + `danger_sandbox_is_rejected` |
| Esc/Ctrl-C keymap | `esc_cancels_only_while_running` |
| receipt-shaped transcript | `apply_response_appends_assistant_and_files` |
| tail of conversation | `visible_messages_keep_the_tail` |
| parse unit tests | `workbench::tests::{path_like_args_are_workdirs, parse_workdir_and_prompt, gui_selects_picker}` |
| `--json` cassette write | `cli_workbench` 6 passed, including `trusted_workbench_cassette_creates_golden_path_file` |
| `kiana run` not broken | `cli_run::trusted_workspace_write_cassette_creates_golden_path_file` |
| smoke gate | `scripts/v10-workbench-smoke.sh` printed `ok` |

## Commands run

```
cargo test -p kiana-entrypoints --lib --offline workbench_chat -- --test-threads=1
cargo test -p kiana-entrypoints --lib --offline workbench -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_workbench --offline -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_run --offline trusted_workspace_write_cassette_creates_golden_path_file -- --test-threads=1
bash -n scripts/v10-workbench-smoke.sh
bash scripts/v10-workbench-smoke.sh
```

All listed commands passed. Did not `cargo fmt --all`, did not format all of
`cli.rs`, did not run `scripts/release-smoke.sh`, did not use a live provider,
did not unpark `kiana tui`, did not open v1.x, did not claim token streaming.

## Not claimed

- live provider / `unsupported_streaming` / token streaming
- interactive alt-screen as a cassette gate (unit tests + `--json` write only)
- mid-tool abort of a gold-path cassette
- permission popups / `@file` / product slash layer
- rewind / cross-process session persist / login / model picker
- `kiana tui` on DaemonHost
- dsh Web UI
- `~/.local/bin` production install / home desktop unless opted in
- packaged tarball / SBOM / signing
- Daily/Research packs (REL-04 stays draft)
- worktrees / SDK / IDE / Desktop app / Web
- JointSymposium / staffing every COMPANY.md role
- TeamCreate / SendMessage
- v1.x enterprise
