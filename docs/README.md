# Kiana 文档索引

按读者和用途分流。设计文档很多，但**完成判定不看设计篇幅**，只看 capability matrix、测试、运行级 smoke 和发布证据。

## 先看这些

| 文档 | 用途 |
| --- | --- |
| [README.md](../README.md) | 产品定位、架构不变量、workspace 地图 |
| [QUICKSTART.md](../QUICKSTART.md) | 编译、配置、REPL/TUI/print 的最短路径 |
| [USAGE.md](../USAGE.md) | 已接线命令与操作手册 |
| [CONFIG.md](../CONFIG.md) | `~/.kiana/config.toml` 与 provider 环境变量 |
| [INSTALL.md](../INSTALL.md) | 源码安装、Makefile、tarball |
| [docs/tui.md](tui.md) | 产品 TUI 与 `kiana-tui` crate 的边界 |
| [docs/architecture.md](architecture.md) | 控制平面分层与 crate 职责 |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | 开发、验证、提交约定 |

## 运维与发布

| 文档 | 用途 |
| --- | --- |
| [RELEASE.md](../RELEASE.md) | 发布流程 |
| [UPGRADE.md](../UPGRADE.md) | 升级/回滚注意 |
| [SECURITY.md](../SECURITY.md) | 安全策略 |
| [PRIVACY.md](../PRIVACY.md) | 隐私 |
| [TELEMETRY.md](../TELEMETRY.md) | 遥测默认关闭 |
| [CHANGELOG.md](../CHANGELOG.md) | 变更记录 |
| [release-checklist.md](release-checklist.md) | 发布检查单 |
| [commercial-release-readiness.md](commercial-release-readiness.md) | 商业就绪 blocker |
| [distribution-channels.md](distribution-channels.md) | 分发渠道 |
| [toolchain-upgrade-policy.md](toolchain-upgrade-policy.md) | 工具链升级策略 |

## 契约与运行时

| 文档 | 用途 |
| --- | --- |
| [sdk-runtime-events.md](sdk-runtime-events.md) | RuntimeEvent SDK 合同 |
| [workflow-runtime-design.md](workflow-runtime-design.md) | WorkflowRun / EventLog |
| [docs/schemas/](schemas/) | pinned JSON Schema |
| [docs/superpowers/specs/2026-07-17-kiana-control-plane-architecture-design.md](superpowers/specs/2026-07-17-kiana-control-plane-architecture-design.md) | 控制平面权威设计 |

JSON 合同变化后必须跑 `bash scripts/schema-contract-smoke.sh`。

## 规划权威

GSD / 产品规划以这些文件为准，不要用过期的 phase 笔记覆盖它们：

| 文档 | 层级 |
| --- | --- |
| [`.planning/DESIGN-INDEX.md`](../.planning/DESIGN-INDEX.md) | 设计权威索引 |
| [`.planning/PROJECT.md`](../.planning/PROJECT.md) | 产品定义与约束 |
| [`.planning/REQUIREMENTS.md`](../.planning/REQUIREMENTS.md) | 需求账本 |
| [`.planning/ROADMAP.md`](../.planning/ROADMAP.md) | 24 阶段路线图 |
| [`.planning/STATE.md`](../.planning/STATE.md) | 当前 GSD 执行状态 |

`.planning/STATE.md` 的 YAML 头和正文可能短暂不一致；冲突时以 git 最新提交和实际 phase 目录为准，不要用过期百分比当作完成证明。

## 设计档案（不是完成证明）

这些是深度设计或审计输入，供规划引用，不代表对应代码已经交付：

- [kiana-product-functional-and-implementation-design.md](kiana-product-functional-and-implementation-design.md)
- [kiana-personal-project-os-complete-design.md](kiana-personal-project-os-complete-design.md)
- [kiana_project_os/](kiana_project_os/) — Project OS 36 分册
- [reference-feature-matrix.md](reference-feature-matrix.md)
- [reference-migration-roadmap.md](reference-migration-roadmap.md)
- [reference_audit/](reference_audit/)
- [agent-program/](agent-program/)
- [superpowers/plans/](superpowers/plans/) 与 [superpowers/specs/](superpowers/specs/)

`reference/` checkout 默认被 `.gitignore` 排除，审计笔记放在 `docs/reference_audit/`。

## 仓库卫生

- 默认分支是 `master`
- 生成物 `graphify-out/`、本地 `.superpowers/` 会话笔记、VS Code `node_modules` / `*.vsix` 不应进入 git
- 未落地的 ACP / plugins / scripting / VS Code extension 在本地分支 `wip/unlanded-extensions`
- 旧 stash 备份在 `wip/stash-before-ctrl-r-merge`，不要直接应用到当前 master
