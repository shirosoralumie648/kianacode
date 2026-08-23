---
gsd_state_version: 1.0
milestone: v0.6
milestone_name: Parallel builders, same core
current_phase: 1
current_phase_name: Parallel builders + path locks
status: completed
stopped_at: v0.6.1 ORCH-04 landed. Same DaemonHost can run two packet Builders. Path locks fail closed on live overlap. Packet path_allow intersects apply_patch. Worktrees / SDK / IDE remain later. Next is v1.0 install/docs/P0 core paths.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.6.1 parallel builders on the same core. Do not implement worktrees, SDK/IDE, JointSymposium, or TUI in the next slice. Next slice is v1.0 personal complete product.
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
**Current focus:** v0.6.1 ORCH-04 complete — next is v1.0 install/docs/P0 core paths.

## Current Position

Phase: 1 of v0.6
Status: Complete — two packet Builders on one DaemonHost with path locks
Last activity: 2026-08-23 — ORCH-04 parallel builders landed; worktrees/SDK/IDE later; JointSymposium frozen; P1-READ skipped; TUI park kept

## Notes

- v0.2–v0.5 remain regression baselines. Do not regress Builder write, symposium, review, stdio MCP, skills/PreToolUse, fake text-only `unsupported_tools`, six-layer memory ACL, or department symposiums.
- P1-READ skipped: matrix locked Codex-shaped search via `shell`.
- PATH-03 is now `shell` + `apply_patch` + `mcp` + `memory.search` + `memory.write`. Skills remain context, not a model tool.
- Five departments exist as catalog objects. Do not staff every COMPANY.md role.
- Compact is `kiana-runner` history replacement surfaced on the receipt. `kiana-query` is not the engine.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程新 host 仍 `session_not_found`。
- Spawn 仍是原语。路径锁在 ControlPlane。空 `path_allow` 锁 `*`。packet 越权 `packet_path_denied`。
- 不要扩张 `kiana-tools`，不解冻 TeamCreate/SendMessage。
- 两个 Archon 都不是工人运行时。
- 证明上限仍 `local_behavior`。
- 下一产品站是 v1.0 安装/文档/P0 核心路径，不是 worktree，不是 SDK/IDE，不是 JointSymposium。
