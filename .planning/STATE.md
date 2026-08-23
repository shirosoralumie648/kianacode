---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: Personal complete product
current_phase: 3
current_phase_name: Personal folder workbench
status: completed
stopped_at: v1.0.3 folder workbench is green at local_behavior (cwd / --workdir / GUI picker on DaemonHost). TUI stays parked. v1.x is not opened.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v1.0.3 folder workbench. Do not open v1.x, worktrees, SDK/IDE, live provider, or unpark kiana tui.
progress:
  total_phases: 3
  completed_phases: 3
  total_plans: 0
  completed_plans: 0
  percent: 100
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v1.0.3 personal folder workbench landed. v1.x is not opened.

## Current Position

Phase: 3 of v1.0
Status: Complete — folder workbench (Codex/pi/dsh launch UX) on DaemonHost
Last activity: 2026-08-23 — kiana/--workdir/--pick-folder cassette green; TUI park kept; v1.x not opened

## Notes

- v0.2–v0.6 remain regression baselines. Do not regress Builder write, symposium, review, stdio MCP, skills/PreToolUse, fake text-only `unsupported_tools`, six-layer memory ACL, department symposiums, parallel path locks, or temp install lifecycle.
- P1-READ skipped: matrix locked Codex-shaped search via `shell`.
- PATH-03 is now `shell` + `apply_patch` + `mcp` + `memory.search` + `memory.write`. Skills remain context, not a model tool.
- Five departments exist as catalog objects. Do not staff every COMPANY.md role.
- Compact is `kiana-runner` history replacement surfaced on the receipt. `kiana-query` is not the engine.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程新 host 仍 `session_not_found`。
- Spawn 仍是原语。路径锁在 ControlPlane。空 `path_allow` 锁 `*`。packet 越权 `packet_path_denied`。
- REL-01 证明是临时 `INSTALL_DIR` + `install.sh --uninstall`，不是 `~/.local/bin`，不是 dist tarball。
- REL-02 是 `USER.md` 真命令，不是 104 req。
- REL-03 是 P0 审计收口 + `NOTICE`，不是再做工具，不是 SBOM。
- REL-04 Daily/Research 仍草案。
- 文件夹工作台是 `kiana` / `--workdir` / `--pick-folder`。未信任写盘失败码与 `kiana run` 相同：`workspace_write_requires_trusted_non_safe_profile`。
- 不要扩张 `kiana-tools`，不解冻 TeamCreate/SendMessage。
- 两个 Archon 都不是工人运行时。
- 证明上限仍 `local_behavior`。v1.0 不是 live provider、不是签名包、不是企业。
- 不要打开 v1.x，除非用户显式要求。文件夹工作台不是 v1.x，也不是把 `kiana tui` 当产品。
