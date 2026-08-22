---
gsd_state_version: 1.0
milestone: v0.2
milestone_name: Runnable Local Agent MVP
current_phase: 2
current_phase_name: Session continue / cancel / visible failure
status: planning
stopped_at: Phase 1 CLI golden path verified locally; next is v0.2 Phase 2 (continue/cancel/visible failure). Do not implement 5 depts/symposium/RAG in v0.2.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Trusted workspace-write Builder cassette writes GOLDEN_PATH.txt through DaemonHost/KianaHarness. Proof ceiling remains local_behavior.
progress:
  total_phases: 4
  completed_phases: 1
  total_plans: 0
  completed_plans: 0
  percent: 25
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it. v0.2 still starts with one Builder in the executing department.
**Current focus:** v0.2 Phase 2 — session continue / cancel / visible failure on the same harness spine

## Current Position

Phase: 2 of 4 in v0.2 (v0.3–v1.x exist as a frozen ladder, not live counters)
Status: Planning — Phase 1 golden path is locally verified; discuss/plan Phase 2 next
Last activity: 2026-08-23 — Phase 1 write path green (`kiana trust .` + cassette `apply_patch` creates `GOLDEN_PATH.txt`)

## Notes

- 旧 Phase 1（governance）/ Phase 2（toolchain）是历史，不是本计数。
- `cli_run.rs` 路由测试仍在，但 PATH 完成看的是写盘 cassette，不是 fake-script ls。
- 不要扩张 `kiana-tools`，不要把 v0.4–v1.x 需求拉进本期。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
