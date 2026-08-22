# v0.2 Phase 1 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

```bash
kiana trust .
kiana run --sandbox workspace-write -- "create a file named GOLDEN_PATH.txt containing hello"
test -f GOLDEN_PATH.txt
```

CI equivalent is cassette playback of a real-provider-shaped `apply_patch` tool call through `DaemonHost`, not a fake-script routing stub.

## Evidence

| Criterion | Result |
|---|---|
| Trusted fixture + workspace-write writes the file | `trusted_workspace_write_apply_patch_creates_file` (daemon) and `trusted_workspace_write_cassette_creates_golden_path_file` (CLI) |
| Content matches prompt | `GOLDEN_PATH.txt` == `hello\n` |
| Untrusted workspace-write fail-closed | `workspace_write_requires_trusted_non_safe_profile`, file absent |
| Empty prompt fail-closed | `prompt_required` |
| No model fail-closed | existing `run_without_model_fails_closed` |
| JSON receipt | `harness: kiana-harness`, `role_id: builder`, `department_id: executing` |
| No kiana-tools Read/Edit/Grep | CLI dumped JSON assertion |

## Commands run

```
cargo test -p kiana-domain --locked --lib -- v0_2_worker
cargo test -p kiana-policy --locked
cargo test -p kiana-core --test control_plane --locked
cargo test -p kiana-commands --locked --lib -- trust
cargo test -p kiana-daemon --test daemon_host --locked
cargo test -p kiana-entrypoints --test cli_run --locked
```

All listed targets passed.

## Not claimed

- live provider
- physical/target readiness
- session continue/cancel (Phase 2)
- durable disk receipts (Phase 3)
- TUI (Phase 4)
