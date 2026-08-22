---
gsd_state_version: 1.0
milestone: v0.2
milestone_name: Runnable Local Agent MVP
current_phase: 4
current_phase_name: TUI on harness, or park
status: planning
stopped_at: Phase 3 JSONL receipts verified locally; next is v0.2 Phase 4 (TUI on DaemonHost or written park). Do not implement 5 depts/symposium/RAG in v0.2.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: DaemonHost::local() appends JSONL receipts; kiana run --receipt rereads files_changed after restart; second run appends. Proof ceiling remains local_behavior.
progress:
  total_phases: 4
  completed_phases: 3
  total_plans: 0
  completed_plans: 0
  percent: 75
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it. v0.2 still starts with one Builder in the executing department.
**Current focus:** v0.2 Phase 4 — TUI on the same DaemonHost spine, or written park

## Current Position

Phase: 4 of 4 in v0.2 (v0.3–v1.x exist as a frozen ladder, not live counters)
Status: Planning — Phase 3 disk receipts are locally verified; discuss/plan Phase 4 next
Last activity: 2026-08-23 — Phase 3 green (JSONL receipts, `--receipt`, restart-safe append)

## Notes

- 旧 Phase 1（governance）/ Phase 2（toolchain）是历史，不是本计数。
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed。
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session。
- 不要扩张 `kiana-tools`，不要把 v0.4–v1.x 需求拉进本期。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
