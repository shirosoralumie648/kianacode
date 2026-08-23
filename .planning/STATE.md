---
gsd_state_version: 1.0
milestone: v0.5
milestone_name: Five departments + six-layer RAG
current_phase: 1
current_phase_name: Five department objects
status: completed
stopped_at: v0.5.1 DEPT-02 landed. Catalog has initiating/planning/executing/monitoring/closing. Sponsor writes charter/; closer writes lessons/; default Builder still writes GOLDEN_PATH.txt. Next is six-layer RAG ACL (MEM).
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.5.1 five department objects. Do not implement memory.search, JointSymposium, or P1-READ in the next slice. Next slice is MEM.
progress:
  total_phases: 1
  completed_phases: 1
  total_plans: 0
  completed_plans: 0
  percent: 100
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.5.1 complete — next is six-layer RAG ACL (`MEM-01`..`MEM-04`).

## Current Position

Phase: 1 of v0.5
Status: Complete — five DepartmentSpec objects; sponsor/closer path ACL
Last activity: 2026-08-23 — DEPT-02 landed; P1-READ skipped; TUI park kept

## Notes

- v0.2–v0.4 remain regression baselines. Do not regress Builder write, symposium, review, stdio MCP, skills/PreToolUse, or fake text-only `unsupported_tools`.
- P1-READ skipped: matrix locked Codex-shaped search via `shell`.
- PATH-03 is still `shell` + `apply_patch` + `mcp`. Skills are context, not a fourth model tool.
- Five departments exist as catalog objects. Do not staff every COMPANY.md role this milestone's next slice.
- `rag_collection` is a name on DepartmentSpec, not a working memory tool.
- Continue 完成定义是同一 `DaemonHost` 生命周期。
- 不要扩张 `kiana-tools`，不解冻 TeamCreate/SendMessage。
- 两个 Archon 都不是工人运行时。
- 证明上限仍 `local_behavior`。
- 下一产品站是 v0.5.2 六层 RAG ACL（MEM），不是 JointSymposium，不是 P1-READ。
