---
gsd_state_version: 1.0
milestone: v0.4
milestone_name: Coding pack baseline
current_phase: 1
current_phase_name: Reviewer ≠ author
status: completed
stopped_at: v0.4 Phase 1 green. monitoring Reviewer reviews a Builder run in a new session and writes gate/REVIEW.json. Next is the coding pack matrix draft.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.4 Phase 1 (REV-01). Next is docs/coding-pack-matrix.md. Do not implement five departments, RAG, MCP, or skills-on-harness.
progress:
  total_phases: 2
  completed_phases: 1
  total_plans: 0
  completed_plans: 0
  percent: 50
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.4 Phase 1 complete — next is v0.4 Phase 2 coding pack public-behavior matrix (`CODE-01`).

## Current Position

Phase: 1 of 2 in v0.4 (matrix is the next opened phase; MCP/skills stay frozen until the matrix says so)
Status: Complete — Reviewer ≠ author
Last activity: 2026-08-23 — Phase 1 landed: `kiana run --review <author_session_id>` writes `gate/REVIEW.json`; TUI park kept

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- v0.3 Phase 1–4 remain the planning/executing/symposium baseline. Do not regress `--symposium` / `--packet`.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed, except durable receipts / review lookup by persisted `session_id`.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把五部门 / RAG / MCP / skills-on-harness 拉进下一期除非矩阵打开。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
- 证明上限仍 `local_behavior`。live provider / `~/.local/bin` 生产安装不是完成。
- 下一产品站是 v0.4 Phase 2 `docs/coding-pack-matrix.md`（CODE-01），不是五部门。
