# v0.4 Phase 4 Context — Skills / hooks on harness

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirement: CODE-03

## Classify

Phase, not spike. User-visible completion is: a trusted Builder run sees one
project Skill in the harness **context** (system message). The same Skill is
withheld when the project is untrusted. A PreToolUse hook can block a brokered
tool so the side effect does not happen.

This is not a new model-visible tool. PATH-03 stays `shell` + `apply_patch` +
`mcp`. This is not `kiana-tools` SkillTool. This is not six-layer RAG.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, v0.4 review, the matrix,
MCP stdio, TUI park, five departments, RAG, or TeamCreate/SendMessage.

1. **Skills are context, not a fourth model tool.** Daemon loads
   `kiana-skills::load_all_skills_with_trust` and injects a system message
   through `RunnerCommand::Start.instructions`. No `skill` tool this slice.
2. **Trust.** User / `KIANA_HOME/skills` / bundled remain when untrusted.
   Project `.claude/skills` and `.kiana/skills` require Trusted. Role ACL for
   skills waits for v0.5 MEM.
3. **One blocking hook: PreToolUse.** After policy Allow, before broker
   execute. `ToolHookDecision::Block` → capability result
   `hook_blocked:...` and the handler does not run. Ask is fail-closed
   (`hook_ask_unattended`). Config is existing `KIANA_PRE_TOOL_USE_HOOKS` /
   `KIANA_HOOKS` / `KIANA_HOOKS_FILE`. No new CLI flag. Do not format `cli.rs`.
4. **PATH-03 unchanged.** Still not the `kiana-tools` registry.

Demo (same-host):

```text
trusted project with .kiana/skills/demo/SKILL.md
cassette text-only
→ first model request has a System message containing the skill name
untrusted → that project skill is absent
KIANA_PRE_TOOL_USE_HOOKS blocks apply_patch
→ GOLDEN_PATH.txt does not appear; tool result contains hook_blocked
```

## Requirements this phase

CODE-03. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01/CODE-02 still true.

## Frozen

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- provider live matrix (CODE-04)
- structured Read/Grep/Glob
- HTTP/SSE MCP
- exploding skills into model tools
- `kiana-tools` wiring
- migrating TUI
- formatting `kiana-entrypoints/src/cli.rs`
