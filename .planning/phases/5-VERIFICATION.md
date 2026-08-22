# v0.3 Phase 1 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

```bash
kiana trust .
kiana run --sandbox workspace-write -- "create GOLDEN_PATH.txt containing hello"
# still builder; file appears; receipt role_id=builder department_id=executing

kiana run --role pm --sandbox workspace-write -- "create GOLDEN_PATH.txt"
# fail closed; GOLDEN_PATH.txt absent; error role_path_denied

kiana run --role pm --sandbox workspace-write -- "write plan/WORK.md"
# apply_patch under plan/ is allowed
```

CI equivalent: cassette + DaemonHost/CLI tests with isolated `$KIANA_HOME`.
Unknown `--role ceo` fails closed with `role_unknown` (CLI rejects before JSON envelope; in-process host returns `Blocked`).

## Evidence

| Criterion | Result |
|---|---|
| Catalog has planning/pm, planning/architect, executing/builder | `v0_3_catalog_has_planning_and_executing_roles` (domain) |
| Default `kiana run` is still Builder and still writes | daemon `trusted_workspace_write_apply_patch_creates_file`; CLI `trusted_workspace_write_cassette_creates_golden_path_file`; receipt `role_id=builder` `department_id=executing` |
| PM cannot `apply_patch` src | policy/daemon/CLI `planning_pm_cannot_apply_patch_source` → `role_path_denied`; `GOLDEN_PATH.txt` absent |
| PM can `apply_patch` `plan/` | policy/daemon/CLI `planning_pm_can_apply_patch_plan_artifact`; file is `hello\n` |
| Architect cannot take workspace-write | daemon `architect_workspace_write_is_role_sandbox_read_only` → `role_sandbox_read_only` |
| Architect tools empty | policy `architect_cannot_apply_patch` → `role_tool_denied` |
| Unknown role fail-closed | daemon `unknown_role_is_rejected` (`Blocked`/`role_unknown`); CLI `unknown_role_fails_closed`; policy `unknown_role_is_denied` |
| Missing protocol role fields default to builder | protocol `missing_role_fields_default_to_executing_builder` |
| `role_*` deny fails the run, file does not appear | core capability path returns `Err(reason)` for `role_*`; daemon PM src test |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-policy -p kiana-protocol -p kiana-core -p kiana-daemon
cargo test -p kiana-domain --locked --lib -- --test-threads=1
cargo test -p kiana-policy --locked -- --test-threads=1
cargo test -p kiana-protocol --locked --lib -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_run --locked -- --test-threads=1
```

All listed targets passed (6 / 8 / 5 / 14 / 16 / 14). Did not `cargo fmt --all` or format `kiana-entrypoints`.

## Not claimed

- independent Builder spawn from a WorkPacket (Phase 2)
- Symposium / DecisionRecord / blackboard (Phase 3)
- five departments / six-layer RAG
- TeamCreate / SendMessage
- eval harness / install.sh as completion (Phase 4)
- live provider / physical readiness
- migrating TUI
- changing the runner tool advertisement (policy is the enforcement)
