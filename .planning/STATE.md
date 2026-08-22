---
gsd_state_version: 1.0
milestone: v0.4
milestone_name: Coding pack baseline
current_phase: 5
current_phase_name: Provider degrade on harness
status: completed
stopped_at: v0.4.4 CODE-04 landed. Fake text-only provider fails closed with unsupported_tools; GOLDEN_PATH.txt is not written. Next is v0.4.5 P1-READ only if the matrix still needs it; otherwise open v0.5.
last_updated: "2026-08-23"
last_activity: 2026-08-23
last_activity_desc: Land v0.4.4 provider degrade on harness. Do not implement five departments, RAG, live provider matrix, or unsupported_streaming in the next slice.
progress:
  total_phases: 5
  completed_phases: 5
  total_plans: 0
  completed_plans: 0
  percent: 100
---

# Project State

See: `COMPANY.md`, `DESIGN.md`, `PROCESS.md`, `PHASES.md`, `.planning/PROJECT.md`

**Core value:** A company of small agents finishes real work and proves it.
**Current focus:** v0.4.4 complete — next is optional P1-READ, else v0.5 five departments.

## Current Position

Phase: 5 of later v0.4 slices (readonly tools remain only if the matrix still needs P1-READ)
Status: Complete — fake text-only provider fails with `unsupported_tools`
Last activity: 2026-08-23 — CODE-04 landed: `with_env_harness`; capability code preserved; TUI park kept

## Notes

- v0.2 Phase 1–4 remain the write-path baseline. Do not regress `kiana run` Builder.
- v0.3 Phase 1–4 remain the planning/executing/symposium baseline. Do not regress `--symposium` / `--packet`.
- v0.4 Phase 1 remains Reviewer ≠ author. Do not regress `--review`.
- v0.4 Phase 2 remains the public-behavior matrix. Do not restore the 104-req corpus.
- v0.4 Phase 3 remains stdio MCP. HTTP still `mcp_transport_unsupported`.
- PATH-03 is still `shell` + `apply_patch` + `mcp`. Skills are context, not a fourth model tool.
- Continue 完成定义是同一 `DaemonHost` 生命周期。跨进程 CLI 新开 host 必须 fail-closed, except durable receipts / review lookup by persisted `session_id`.
- `kiana --resume` / `cli_resume.rs` 仍是 legacy SDK，不是 harness session.
- 不要扩张 `kiana-tools`，不要把五部门 / RAG 拉进下一期 unless the matrix §6 says v0.5 is next.
- 两个 Archon：`reference/Archon` = 冲击半径工具；`reference/Archon-Knowledge` = 过程引擎教材。都不是工人运行时。
- 证明上限仍 `local_behavior`。live provider / `~/.local/bin` 生产安装不是完成。
- 不要把 `unsupported_streaming` 或 live Anthropic/OpenAI/Ollama 标成完成。
- 下一产品站按矩阵 §6：仅当需要才做 v0.4.5 P1-READ；否则打开 v0.5。不要跳站。
