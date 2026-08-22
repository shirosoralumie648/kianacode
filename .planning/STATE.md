---
gsd_state_version: 1.0
milestone: v0.3
milestone_name: Trusted Workbench + company kernel
current_phase: 4
current_phase_name: Eval + install
status: completed
stopped_at: v0.3 Phase 4 green. Cassette golden path + temp install demo; TUI stays parked.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.3 Phase 4 (WB-01..03). Next is v0.4 Reviewer≠author. Do not implement five departments or RAG.
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
**Current focus:** v0.3 complete — next is v0.4 Reviewer≠author (`REV-01`).

## Current Position

Phase: 4 of 4 in v0.3
Status: Complete — eval + install demo
Last activity: 2026-08-23 — Phase 4 landed: `scripts/harness-golden-smoke.sh` + `scripts/v03-workbench-smoke.sh`; TUI park kept

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把五部门 / RAG 拉进下一期除非版本打开。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
- 证明上限仍 `local_behavior`。live provider / `~/.local/bin` 生产安装不是完成。
- 下一产品站是 v0.4 `REV-01`（Reviewer 与 Builder 不得同一 session_id），不是五部门。
