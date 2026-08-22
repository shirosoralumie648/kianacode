---
gsd_state_version: 1.0
milestone: v0.4
milestone_name: Coding pack baseline
current_phase: 4
current_phase_name: Skills / hooks on harness
status: completed
stopped_at: v0.4 Phase 4 / v0.4.3 CODE-03 landed. Trusted Builder sees a project Skill in the harness System message; untrusted withholds it; PreToolUse can block apply_patch. Next is v0.4.4 provider degrade (CODE-04).
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.4.3 skills and PreToolUse hooks on harness. Do not implement five departments, RAG, or provider live matrix in the next slice. Next slice is CODE-04.
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
**Current focus:** v0.4 Phase 4 complete — next implementation is v0.4.4 provider degrade (`CODE-04`).

## Current Position

Phase: 4 of later v0.4 slices (provider / readonly tools remain until the matrix says so — it now says CODE-04 next)
Status: Complete — skills as harness context + one blocking PreToolUse hook
Last activity: 2026-08-23 — Phase 4 landed: `SkillAwareRunner` system pack; PreToolUse `hook_blocked`; TUI park kept

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- v0.3 Phase 1–4 remain the planning/executing/symposium baseline. Do not regress `--symposium` / `--packet`.
- v0.4 Phase 1 remains Reviewer ≠ author. Do not regress `--review`.
- v0.4 Phase 2 remains the public-behavior matrix. Do not restore the 104-req corpus.
- v0.4 Phase 3 remains stdio MCP. HTTP still `mcp_transport_unsupported`.
- PATH-03 is still `shell` + `apply_patch` + `mcp`. Skills are context, not a fourth model tool.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed, except durable receipts / review lookup by persisted `session_id`.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把五部门 / RAG 拉进下一期。
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
- 证明上限仍 `local_behavior`。live provider / `~/.local/bin` 生产安装不是完成。
- 不要把 `kiana-tools` SkillTool 或全套 post/stop/session hook 标成完成。角色 Skill ACL 等到 v0.5 MEM。
- 下一产品站是 v0.4.4 provider 显式降级（CODE-04），按矩阵 §6，不是五部门。
