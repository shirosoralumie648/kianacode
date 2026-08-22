# v0.4 Phase 2 Context — Coding pack public-behavior matrix

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior` (documentation)
Requirement: CODE-01

## Classify

Phase, not spike. User-visible completion is a signed draft at
`docs/coding-pack-matrix.md`. Columns: public behavior, public source,
Kiana status, owner crate, test/evidence, license, period flag.

This phase writes documents. It does not change CLI, harness, MCP, skills,
or `kiana-tools`.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, v0.4 Phase 1 review,
TUI park, five departments, RAG, or TeamCreate/SendMessage.

1. **Documents only.** No `kiana run` flags, no harness tool schemas, no
   MCP/skills wiring, no `kiana-tools` expansion. Do not format
   `kiana-entrypoints/src/cli.rs`. Do not `cargo fmt --all`.
2. **P0 is a core path, not a dump inventory.** P0 covers: CLI write,
   trust/sandbox, session continue/cancel/receipt, visible failure,
   Reviewer ≠ author, plus *documented* gaps MCP / skills / hooks /
   explicit provider degrade. The 50+ restored tool directories in
   `reference/claude-code-rev-main/src/tools/` are not P0.
3. **Owned harness stays Codex-shaped.** Model-visible tools remain
   `shell` + `apply_patch`. Search today is shell. Structured
   Read/Grep/Glob is v0.4.5 only if MCP/skills still leave a gap; it is
   not this phase and not P0.
4. **License boundary is a column, not a footnote.** Codex Apache-2.0
   shapes may be reused. Claude Code dumps and
   `reference/claude-code-main (2)` are public-behavior clean-room only.
   `reference/claude-code-rust` is an anti-pattern (day-1 v1.0.0).
5. **After this draft, the next *implementation* station is CODE-02
   (MCP client through daemon), not five departments.** Skills/hooks
   (CODE-03) follow MCP. Provider degrade (CODE-04) may parallel after
   MCP is a real harness tool, not before. Do not implement them in the
   Phase 2 commit.

Demo (the artifact *is* the demo):

```bash
test -f docs/coding-pack-matrix.md
```

## Requirements this phase

CODE-01. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV still true as
regression and must not be reopened here.

## Frozen

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage as product bus
- MCP client / skills-on-harness / provider live matrix **code**
- structured Read/Grep/Glob on harness
- `kiana-tools` new family or wiring the 50-tool registry onto harness
- migrating TUI
- Desktop / Web / chrome / computer-use
- restoring the deleted 104-req corpus
- using `kiana-tasks` ReviewPacket as the product path
