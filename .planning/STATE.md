---
gsd_state_version: 1.0
milestone: v0.3
milestone_name: Trusted Workbench + company kernel
current_phase: 2
current_phase_name: Independent Builder spawn
status: planning
stopped_at: v0.3 Phase 1 role catalog + policy verified locally; next is independent Builder spawn from a WorkPacket. Do not implement Symposium, five departments, or RAG yet.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Landed RoleSpec catalog (pm/architect/builder) and policy-by-role. Default kiana run remains executing/builder; PM cannot patch src; unknown role fails closed.
progress:
  total_phases: 4
  completed_phases: 1
  total_plans: 0
  completed_plans: 0
  percent: 25
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.3 Phase 2 — dispatch an independent Builder from a WorkPacket (new session, no orchestrator transcript copy).

## Current Position

Phase: 2 of 4 in v0.3
Status: Planning — Phase 1 locally verified; discuss/plan independent spawn next
Last activity: 2026-08-23 — Phase 1 green (role catalog + policy by role)

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把 Symposium / 五部门 / RAG 拉进本期。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
