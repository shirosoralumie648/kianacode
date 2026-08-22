# Kiana 文档索引

当前执行是 **v0.2 Runnable Local Agent MVP**。完成判定看黄金路径、测试和运行级 smoke，不看设计篇幅。

## 先看这些

| 文档 | 用途 |
| --- | --- |
| [planning-current.md](planning-current.md) | 当前里程碑（先看这个） |
| [README.md](../README.md) | 产品定位和最短入口 |
| [QUICKSTART.md](../QUICKSTART.md) | 编译、配置、REPL/TUI/print |
| [USAGE.md](../USAGE.md) | 已接线命令 |
| [CONFIG.md](../CONFIG.md) | `~/.kiana/config.toml` 与 provider |
| [docs/architecture.md](architecture.md) | 当前控制平面分层 |
| [docs/tui.md](tui.md) | 产品 TUI 边界 |

## 规划

| 文档 | 用途 |
| --- | --- |
| [`.planning/PROJECT.md`](../.planning/PROJECT.md) | 产品定义与 v0.2 范围 |
| [`.planning/REQUIREMENTS.md`](../.planning/REQUIREMENTS.md) | PATH / SESS / TRUST / EVD |
| [`.planning/ROADMAP.md`](../.planning/ROADMAP.md) | Phase 1–6 |
| [`.planning/MILESTONES.md`](../.planning/MILESTONES.md) | 当前列车 |
| [`.planning/STATE.md`](../.planning/STATE.md) | GSD 状态 |

下一步是 `$gsd-discuss-phase 3`。没有 `03-*-PLAN.md`。

## 运行时契约

| 文档 | 用途 |
| --- | --- |
| [sdk-runtime-events.md](sdk-runtime-events.md) | RuntimeEvent SDK 合同 |
| [docs/schemas/](schemas/) | pinned JSON Schema |
| [docs/eval/fixtures/](eval/fixtures/) | eval 命令 fixture |
| [docs/proof-templates/](proof-templates/) | 发布证明模板 |

JSON 合同变化后跑 `bash scripts/schema-contract-smoke.sh`。

## 其他用户文档

[INSTALL.md](../INSTALL.md) · [CONTRIBUTING.md](../CONTRIBUTING.md) · [RELEASE.md](../RELEASE.md) · [UPGRADE.md](../UPGRADE.md) · [SECURITY.md](../SECURITY.md) · [PRIVACY.md](../PRIVACY.md) · [TELEMETRY.md](../TELEMETRY.md) · [CHANGELOG.md](../CHANGELOG.md)

## 仓库卫生

- 默认分支是 `master`
- 旧 24 阶段 / Project OS / superpowers / Phase 1–2 规划考古 / capability-governance 账本 / reference audit 已删除
- 未落地的 ACP / plugins / scripting / VS Code extension 在 `wip/unlanded-extensions`
- 旧 stash 备份在 `wip/stash-before-ctrl-r-merge`
