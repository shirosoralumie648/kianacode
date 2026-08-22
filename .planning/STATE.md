---
gsd_state_version: 1.0
milestone: v0.4
milestone_name: Coding pack baseline
current_phase: 2
current_phase_name: Coding pack matrix draft
status: completed
stopped_at: v0.4 Phase 2 signed. docs/coding-pack-matrix.md exists with owner/test/license/period columns. Next is v0.4.2 MCP client through daemon (CODE-02).
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.4 Phase 2 (CODE-01) public-behavior matrix. Do not implement five departments, RAG, or skills-on-harness in the next slice. Next slice is MCP via daemon.
progress:
  total_phases: 2
  completed_phases: 2
  total_plans: 0
  completed_plans: 0
  percent: 100
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.4 Phase 2 complete — next implementation is v0.4.2 MCP client through daemon (`CODE-02`).

## Current Position

Phase: 2 of 2 in the v0.4 *opened* plan (MCP/skills/provider remain later slices until this matrix says so — it now says CODE-02 next)
Status: Complete — coding pack public-behavior matrix draft
Last activity: 2026-08-23 — Phase 2 landed: `docs/coding-pack-matrix.md`; TUI park kept

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- v0.3 Phase 1–4 remain the planning/executing/symposium baseline. Do not regress `--symposium` / `--packet`.
- v0.4 Phase 1 remains Reviewer ≠ author. Do not regress `--review`.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed, except durable receipts / review lookup by persisted `session_id`.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把五部门 / RAG 拉进下一期。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
- 证明上限仍 `local_behavior`。live provider / `~/.local/bin` 生产安装不是完成。
- 下一产品站是 v0.4.2 MCP client 经 daemon（CODE-02），按矩阵 §6，不是五部门、不是 skills-first。
