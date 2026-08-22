# v0.4 Phase 4 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: CODE-03
PATH-03: unchanged (`shell` + `apply_patch` + `mcp`)

## Demo contract

```text
trusted project with .kiana/skills/demo/SKILL.md
cassette text-only
→ first model request has a System message containing the skill name
untrusted → that project skill is absent
KIANA_PRE_TOOL_USE_HOOKS blocks apply_patch
→ GOLDEN_PATH.txt does not appear; tool result contains hook_blocked
```

Same-host proof is in-process DaemonHost. Skills are **context**, not a
fourth model tool. The blocking hook is **PreToolUse** after policy Allow
and before broker execute. No new `kiana run` flag. Did not format `cli.rs`.

## Evidence

| Criterion | Result |
|---|---|
| Skills are context, not a model tool | `SkillAwareRunner` fills empty `Start.instructions`; PATH-03 still `shell` + `apply_patch` + `mcp` |
| Trusted project `.kiana/skills` visible | `trusted_project_skill_appears_in_harness_system_message`: first System text contains `code03-harness-demo` |
| Untrusted project skill withheld | `untrusted_project_skill_is_withheld_from_harness_system_message` |
| User / `KIANA_HOME/skills` / bundled stay when untrusted | existing `kiana-skills` trust tests; `.kiana/skills` now walks with `.claude/skills` |
| PreToolUse can block a brokered tool | `pre_tool_use_hook_blocks_apply_patch_before_broker_execute`: file absent; tool result `hook_blocked`; run still Completed |
| Ask is fail-closed | `hook_ask_unattended:...` in `pre_tool_hook_block` |
| Config is existing env | `KIANA_PRE_TOOL_USE_HOOKS` / `KIANA_HOOKS` / `KIANA_HOOKS_FILE`; no new CLI flag |
| Not SkillTool / full hook suite | Did not add model tool `skill`; did not mark post/stop/session hooks as product-complete |

## Commands run

```
cargo fmt -p kiana-runner-protocol -p kiana-runner -p kiana-core -p kiana-skills -p kiana-daemon
cargo test -p kiana-runner-protocol --locked --lib -- --test-threads=1
cargo test -p kiana-runner --locked --lib -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked -- --test-threads=1
cargo test -p kiana-skills --locked --lib -- --test-threads=1
cargo test -p kiana-daemon --locked --lib -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
```

All listed targets passed (4 / 23 / 25 / 10 / 24 / 29). Did not `cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not compile the CLI crate, did not run `scripts/release-smoke.sh` or live provider.

## Not claimed

- model-visible `skill` tool / `kiana-tools` SkillTool
- post / stop / session hooks on the product path (only PreToolUse blocks)
- role ACL for skills (waits for v0.5 MEM)
- HTTP / SSE / WS MCP
- provider explicit degrade as a `kiana run` product proof (CODE-04)
- structured Read/Grep/Glob
- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- migrating TUI
- live provider / physical readiness
- v1.0 REL-03 (P0-PROV still open)
