---
gsd_state_version: 1.0
milestone: v0.4
milestone_name: Coding pack baseline
current_phase: 3
current_phase_name: MCP client through daemon
status: completed
stopped_at: v0.4 Phase 3 / v0.4.2 CODE-02 landed. Trusted Builder can call one stdio MCP server through DaemonHost. Next is v0.4.3 skills/hooks on harness (CODE-03).
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.4.2 MCP client through daemon. Do not implement five departments, RAG, or provider live matrix in the next slice. Next slice is skills/hooks via harness.
progress:
  total_phases: 3
  completed_phases: 3
  total_plans: 0
  completed_plans: 0
  percent: 100
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.4 Phase 3 complete — next implementation is v0.4.3 skills/hooks on harness (`CODE-03`).

## Current Position

Phase: 3 of 3 in the v0.4 *opened* plan (skills/provider remain later slices until this matrix says so — it now says CODE-03 next)
Status: Complete — MCP client through daemon (stdio)
Last activity: 2026-08-23 — Phase 3 landed: model tool `mcp` → broker `mcp.call`; TUI park kept

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- v0.3 Phase 1–4 remain the planning/executing/symposium baseline. Do not regress `--symposium` / `--packet`.
- v0.4 Phase 1 remains Reviewer ≠ author. Do not regress `--review`.
- v0.4 Phase 2 remains the public-behavior matrix. Do not restore the 104-req corpus.
- PATH-03 is now `shell` + `apply_patch` + `mcp`. Still not the `kiana-tools` registry.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed, except durable receipts / review lookup by persisted `session_id`.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把五部门 / RAG 拉进下一期。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
- 证明上限仍 `local_behavior`。live provider / `~/.local/bin` 生产安装不是完成。
- HTTP MCP 仍是 `mcp_transport_unsupported`。不要把 `kiana-tools/mcp_tool.rs` 或 `kiana-entrypoints/src/mcp.rs` 标成完成。
- 下一产品站是 v0.4.3 skills/hooks 接到 harness（CODE-03），按矩阵 §6，不是五部门、不是 provider-first。
