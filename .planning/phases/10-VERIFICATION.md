# v0.4 Phase 2 Verification

Date: 2026-08-23
Proof: `local_behavior` (documentation)
Verdict: pass
Requirement: CODE-01

## Demo contract

```bash
test -f docs/coding-pack-matrix.md
```

The artifact *is* the demo. No CLI change. No harness change. No MCP /
skills-on-harness code.

## Evidence

| Criterion | Result |
|---|---|
| File exists | `docs/coding-pack-matrix.md` |
| Columns complete | 公开行为、公开来源、Kiana 现状、owner、测试/证据、许可证、优先级、是否本期 |
| Period flags honest | 已绿 / 文档本期 / 矩阵后开 / 冻结 defined in §0.2 |
| P0 is a core path, not the dump | §1 lists LOOP/WRITE/SHELL/TRUST/SANDBOX/FAIL/CONT/CANCEL/RCPT/ROLE/REV/ORCH as 已绿; MCP/SKILL/HOOK/PROV as 矩阵后开. §7 dump inventory is explicitly not P0 |
| Frozen written | §5 FZ-TEAM, FZ-TOOLS, FZ-DUMP, FZ-CCMAIN, FZ-CCRUST, FZ-CLI, FZ-DEPT, FZ-SWARM, FZ-ENT, FZ-104 |
| License column present | per-row plus §9 source table. Codex Apache-2.0 vs Anthropic ToS vs dump unknown |
| Anti-pattern recorded | §8 `claude-code-rust` day-1 v1.0.0 |
| Next implementation named | §6: v0.4.2 CODE-02 MCP client through daemon. Not five departments |
| No code this phase | CONTEXT locked documents-only. Did not format `cli.rs`, did not `cargo fmt --all`, did not wire MCP/skills |
| Did not restore 104-req corpus | new file under `docs/`, not the deleted schema handbook |

## Commands run

```
test -f docs/coding-pack-matrix.md
test -f .planning/phases/10-CONTEXT.md
rg -n '是否本期|矩阵后开|冻结|CODE-02|TeamCreate|claude-code-rust' docs/coding-pack-matrix.md
```

Did not compile `kiana-entrypoints`. Did not run `scripts/release-smoke.sh`
or live provider. Did not treat `kiana-tools` file count as evidence.

## Not claimed

- MCP client on harness (CODE-02)
- skills/hooks on harness (CODE-03)
- provider explicit degrade as a `kiana run` product proof (CODE-04)
- structured Read/Grep/Glob
- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- migrating TUI
- live provider / physical readiness
- v1.0 REL-03 (P0-MCP/SKILL/HOOK/PROV still open)
