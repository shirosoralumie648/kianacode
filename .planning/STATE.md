---
gsd_state_version: 1.0
milestone: v0.3
milestone_name: Trusted Workbench + company kernel
current_phase: 2
current_phase_name: Independent Builder spawn
status: completed
stopped_at: v0.3 Phase 2 green. Independent Builder spawn from a WorkPacket into a fresh session on the same DaemonHost; packet is the only worker input.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.3 Phase 2 (ORCH-01 / ORCH-03). Next is one bounded symposium. Do not implement five departments or RAG.
progress:
  total_phases: 4
  completed_phases: 2
  total_plans: 0
  completed_plans: 0
  percent: 50
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.3 Phase 2 complete — next is Phase 3 one bounded symposium.

## Current Position

Phase: 2 of 4 in v0.3
Status: Complete — independent Builder spawn from WorkPacket
Last activity: 2026-08-23 — Phase 2 verified (`6-VERIFICATION.md`)

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把 Symposium / 五部门 / RAG 拉进本期已提交范围。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
