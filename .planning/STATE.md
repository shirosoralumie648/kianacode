---
gsd_state_version: 1.0
milestone: v0.5
milestone_name: Five departments + six-layer RAG
current_phase: 4
current_phase_name: Department symposiums
status: completed
stopped_at: v0.5 later SYMP-04 landed. Each department can convene a bounded symposium. Chair follows can_convene. Planning still emits a Builder packet; other departments write that department's DECISION.json. JointSymposium remains frozen. Next is v0.6 parallel Builder same-core.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.5 later department symposiums. Do not implement JointSymposium, P1-READ, or TUI in the next slice. Next slice is v0.6 parallel Builder same-core.
progress:
  total_phases: 4
  completed_phases: 4
  total_plans: 0
  completed_plans: 0
  percent: 100
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.5 later SYMP-04 complete — next is v0.6 parallel Builder same-core.

## Current Position

Phase: 4 of v0.5
Status: Complete — five department-bounded symposiums on DaemonHost
Last activity: 2026-08-23 — SYMP-04 department symposiums landed; JointSymposium frozen; P1-READ skipped; TUI park kept

## Notes

- v0.2–v0.4 remain regression baselines. Do not regress Builder write, symposium, review, stdio MCP, skills/PreToolUse, fake text-only `unsupported_tools`, or six-layer memory ACL.
- P1-READ skipped: matrix locked Codex-shaped search via `shell`.
- PATH-03 is now `shell` + `apply_patch` + `mcp` + `memory.search` + `memory.write`. Skills remain context, not a model tool.
- Five departments exist as catalog objects. Do not staff every COMPANY.md role.
- Compact is `kiana-runner` history replacement surfaced on the receipt. `kiana-query` is not the engine.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程新 host 仍 `session_not_found`。
- 五部门都能开有界会。决议文件落盘；晋升 RAG 必须显式 `memory.write`。CLI `--symposium` 仍只许 PM。
- 不要扩张 `kiana-tools`，不解冻 TeamCreate/SendMessage。
- 两个 Archon 都不是工人运行时。
- 证明上限仍 `local_behavior`。
- 下一产品站是 v0.6 并行 Builder 同核，不是 JointSymposium，不是 P1-READ，不是向量库。
