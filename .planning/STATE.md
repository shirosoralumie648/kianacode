---
gsd_state_version: 1.0
milestone: v0.5
milestone_name: Five departments + six-layer RAG
current_phase: 2
current_phase_name: Six-layer RAG ACL
status: completed
stopped_at: v0.5.2 MEM landed. Six JSONL layers exist. memory.search/write go through DaemonHost. Hits land on receipts. Scratch does not promote. Builder cannot read user-private or unpublished planning debate. Next is compact/resume or department symposiums, not JointSymposium staffing.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.5.2 six-layer RAG ACL. Do not implement JointSymposium, P1-READ, or TUI in the next slice. Next slice is v0.5 later compact/resume or department symposiums.
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
**Current focus:** v0.5.2 complete — next is compact/resume or department symposiums.

## Current Position

Phase: 2 of v0.5
Status: Complete — six-layer memory ACL on DaemonHost
Last activity: 2026-08-23 — MEM-01..04 landed; P1-READ skipped; TUI park kept

## Notes

- v0.2–v0.4 remain regression baselines. Do not regress Builder write, symposium, review, stdio MCP, skills/PreToolUse, or fake text-only `unsupported_tools`.
- P1-READ skipped: matrix locked Codex-shaped search via `shell`.
- PATH-03 is now `shell` + `apply_patch` + `mcp` + `memory.search` + `memory.write`. Skills remain context, not a model tool.
- Five departments exist as catalog objects. Do not staff every COMPANY.md role.
- `rag_collection` is still only a name on DepartmentSpec. The working memory engine is partitioned JSONL + brokered tools.
- Continue 完成定义是同一 `DaemonHost` 生命周期。
- 不要扩张 `kiana-tools`，不解冻 TeamCreate/SendMessage。
- 两个 Archon 都不是工人运行时。
- 证明上限仍 `local_behavior`。
- 下一产品站是 v0.5 later（compact/resume 或部门会），不是 JointSymposium，不是 P1-READ，不是向量库。
