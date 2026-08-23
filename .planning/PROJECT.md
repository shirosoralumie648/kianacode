# Kiana

## What This Is

本地优先的 Company OS：五个 PMP 过程组是部门；部门内是独立上下文、独立提示词、独立权限/知识面的角色；部门内可开有界 Symposium；跨部门只交 Work Packet；记忆按公司/部门/角色/项目/用户分层。
工人运行时仍是同一 `DaemonHost` / `KianaHarness`。
北星见 `COMPANY.md`。Claude Code 公开行为是后期 Coding pack 审计，不是现在的退出门。

完整阶梯：`DESIGN.md`。git 证据：`PROCESS.md`。公司架构：`COMPANY.md`。逐期剧本：`PHASES.md`。

## Current Milestone

**v0.5 Five departments + six-layer RAG** — Phase 1 已本地绿：五个 `DepartmentSpec` 同时存在；`sponsor` 写 `charter/`；`closer` 写 `lessons/`；默认工人仍是 `role=builder` / `department=executing`。v0.4 Coding pack 仍必须绿（P1-READ skipped）。`kiana tui` 保持 park。下一站：v0.5.2 六层 RAG ACL（MEM）。

后续：v0.4 Coding pack + Reviewer → v0.5 五部门 + 六层 RAG → v0.6 多入口同一核 → v1.0 个人完整产品。

## Core Value

用户把任务交给公司：Builder 在独立上下文里改到文件，失败可见，收据含角色、部门与证据。不是一个万能 chat 包办全过程，也不是全员微信群。

## Constraints

- 双运行时中，只有 owned harness 是产品；legacy `runner.rs` / `kiana-tools` 冻结
- 证明级别本里程碑上限：`local_behavior`
- 专有 reference 只许 clean-room 行为审计
- fail-closed：未信任 deny，默认 sandbox read-only；规划角色不能写 src
- 不把 crate 数、reference 打勾、旧 24-phase、招满角色但不会写盘当完成
