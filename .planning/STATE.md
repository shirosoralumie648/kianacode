---
gsd_state_version: 1.0
milestone: v0.2
milestone_name: Runnable Local Agent MVP
current_phase: 4
current_phase_name: TUI parked
status: completed
stopped_at: v0.2 four phases locally verified (CLI write path, continue/cancel, JSONL receipts, TUI park). Next is v0.3 (planning + executing + one bounded symposium). Do not implement five departments/RAG yet.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: SURF-01 park — kiana tui stays on legacy SDK/stream; help + non-TTY tests lock it off the v0.2 product path. Proof ceiling remains local_behavior.
progress:
  total_phases: 4
  completed_phases: 4
  total_plans: 0
  completed_plans: 0
  percent: 100
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it. v0.2 still starts with one Builder in the executing department.
**Current focus:** v0.2 complete locally. Open v0.3 next — planning + executing + one bounded symposium.

## Current Position

Phase: 4 of 4 in v0.2 (v0.3–v1.x exist as a frozen ladder, not live counters)
Status: Completed — TUI parked; product path is still `kiana run` / print
Last activity: 2026-08-23 — Phase 4 park green (help + non-TTY lock tests)

## Notes

- 旧 Phase 1（governance）/ Phase 2（toolchain）是历史，不是本计数。
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed。
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session。
- 不要扩张 `kiana-tools`，不要把 v0.4–v1.x 需求拉进本期。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
