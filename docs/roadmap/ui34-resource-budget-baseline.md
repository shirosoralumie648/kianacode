# UI-34 性能、资源上限和可访问性验收基线

> 快照日期：2026-09-25。Rust 验收由 GitHub Actions 执行；本步骤不在本地运行测试、build、
> check、clippy 或 smoke，且不等待 CI 结果。

## Shared resource budget

`UiResourceBudget`/`UiResourceUsage` 统一描述 Feed、Snapshot、Artifact、DOM、TTY buffer 和 IPC
的 bytes/items/sessions/queue depth。`evaluate_budget` 返回 Accept、显式 Degraded 或 Reject：
hard bytes/queue 超限不得继续无界渲染；item/session 超限必须回到 hydrate/pagination/limited
presenter；pending/unknown items 先做保护项检查，保护项超过上限直接返回稳定错误，不能静默淘汰。

该 contract 是 presentation/resource decision，不拥有 EventLog、ControlPlane、cancel、retry、
approval 或 effect 权限；Degraded/Reject 只改变 UI 可见状态和下一步 query，不改写业务终态。

## CI-only fixture 与限制

`kiana-client/tests/fixtures/ui34-resource-budget.json` 与 `ui34_resource_budget.rs` 覆盖 within/
Degraded/Reject、bytes/queue hard bound、protected pending/unknown、非法 schema/limit/usage。
`.github/workflows/ui34-resource-budget.yml` 运行 Rust format、聚焦 budget fixture 与 workspace
test-target compile。
其 push/pull_request path filter 现在同时包含当前 CM-36 `kiana-domain/src/memory_workbench.rs`，
fresh remote run 会覆盖 repository-wide fmt dependency；该远程结果 pending/unobserved。

`feature_status=implemented`; `proof_level=source`。未证明真实长流/100+ sessions/慢磁盘/低带宽、
RSS/p50/p95/queue metrics、浏览器/PTY/Electron accessibility runner、bundle performance、跨进程
backpressure、provider/Broker effect 或 live/physical proof；CI 结果保持 pending/unobserved。
