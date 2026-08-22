# 当前规划：先跑通本地 Agent MVP

**Date:** 2026-08-22  
**Milestone:** v0.2 Runnable Local Agent MVP  
**Authority:** `.planning/PROJECT.md`, `.planning/REQUIREMENTS.md`, `.planning/ROADMAP.md`, `.planning/STATE.md`

## 决策

原 24 阶段 1.0 路线是横切地基：契约 / 状态 / 策略 / RuntimeHost 先做完，再做能力包和入口。那会把第一条用户能用的黄金路径推到 Phase 10 附近。

2026-08-22 起，**剩余工作改为纵向切片**：先做一个能跑通的本地 coding agent，再逐步扩展成更通用的助手。这不是把 Phase 2 标成完成，也不是执行旧的 `$gsd-discuss-phase 3`（契约与 Schema 基线）。

## 当前可声明的事实

- 仓库能编译、有测试、有 `kiana` 二进制，不等于 Claude Code / Desktop / `reference/` 已完成。
- 产品入口已经接线：`kiana` REPL、`kiana -p`、`kiana run [--json]`、`kiana tui`。
- 已有 Anthropic / OpenAI-compatible / Ollama、工具循环、ProjectTrust fail-closed、`kiana-daemon` / `kiana-core`。
- `kiana architecture status --json` 仍报告 `legacy_edges_remaining: 9`。
- Phase 1 证据治理已完成。Phase 2 实现已落地，但 `human_verify_mode: end-of-phase` 仍开着；签名 / 真实 `dist/` 不能从 local_behavior 升级成 target_environment。

## 本里程碑要证明什么

用户在一个 **已信任的本地仓库** 里给出任务，Kiana 用 **一个真实 provider** 和 **read / edit / shell** 干活，并在 CLI / TUI 流出结果。信任和权限 fail-closed。失败必须可见。跑完后能指出哪些工具真正执行过、哪些文件被改过。

本里程碑 **不** 证明：

- 全入口（IDE / Desktop / Web / Cloud / Enterprise）
- Research / Daily 能力包
- 38 个 `reference/` 的产品完成
- 多 provider 能力协商超集
- 已签名发布物或 1.0 措辞

## 阶段切分

| Phase | 焦点 | 状态 |
|-------|------|------|
| 1 | 现状基线与证据治理 | Complete（2026-07-26） |
| 2 | 可复现工具链与依赖收敛 | 实现已落地；人审门禁仍开，不阻塞 MVP |
| 3 | 黄金路径能跑通 | 下一步：`$gsd-discuss-phase 3` |
| 4 | 会话可恢复、可取消、失败可见 | Pending |
| 5 | 信任与权限让 MVP 能用且 fail-closed | Pending |
| 6 | 任务确实执行过的证据 | Pending |

旧 Phase 3–24 设计文档、特性账本、journeys 和 1.0 需求附录已删除。后续里程碑需要时再写，不再从旧档案恢复。

## 证据边界

| 说法 | 本里程碑是否允许 |
|------|------------------|
| 已实现 / 代码存在 | 可以，但不是完成 |
| 本地可跑通黄金路径 | Phase 3–6 的目标 |
| 已验证（local_behavior） | 需要对应 VERIFICATION |
| 目标环境 / 物理就绪 / 1.0 | 不允许用本里程碑声称 |

`skip_discuss: false`。本文件只重切路线，不代替 Phase 3 的 discuss/plan，也不创建 `03-*-PLAN.md`。
