# H33 运行资源、关闭和异常退出清理基线

> 快照日期：2026-09-19。本页记录 H33 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、协议和 Core/daemon source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H33`](harness.md#step-h33) |
| feature_status | `implemented`（domain cleanup/retention contract + existing runner/daemon/core boundary guard；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | Cleanup reducer records evidence; adapters/supervisor own actual stop/release, EventLog/ControlPlane own recovery and ownership facts |
| this step does | 明确 ModelTask/ToolFuture/MCP/TemporaryArtifact/JobHandle/PathLock/BudgetLease/Subscription resource owner/status；stop confirmation 或 effect evidence 缺失时隔离为 IsolatedUnknown；normal/cancel/panic/log-failure/shutdown cause、retention limits 和 bounded report 已定义；已有 Runner cancellation/checkpoint、daemon ProcessSupervisor/shutdown、Core path-lock/terminal scope 保持 source-guarded |
| this step does not | 不从 reducer 直接杀进程/释放 OS lock，不把 Host drop 当作 confirmed stop，不宣称跨进程资源 projector、真实 panic rehearsal 或全部外部清理已完成 |

## 1. Contract

`ResourceCleanupPlan::observe_all` 只接受 adapter 的 stop/effect 证据。需要 stop confirmation
而没有确认，或 effect 未知，资源只能进入 `IsolatedUnknown`；只有 `Released` 资源允许上层
进一步释放 ownership。未收到观察的资源保持 Pending。`ResourceRetention` 为 retained runs、
frames 和 history bytes 提供明确上限，超过即拒绝。

现有 ProcessSupervisor 的 bounded TERM/KILL/reap、`ProcessGuard` kill-on-drop、Runner
RunCancellation/in-flight unregister、Core TerminalScope/path-lock release 和 Daemon graceful
shutdown 仍是实际边界；本步只把它们的证据语义统一写成可验证 contract。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `unconfirmed_process_or_lock_never_becomes_released` | missing stop/effect evidence isolates Unknown and does not release ownership |
| `confirmed_normal_cleanup_releases_everything_and_retention_is_bounded` | confirmed normal cleanup releases resources and retention limits reject overflow |
| `panic_and_log_failure_keep_unobserved_resources_pending` | missing observation remains Pending rather than silently clean |
| `cleanup_contract_and_existing_supervisors_keep_unknown_isolated` | Runner/ProcessSupervisor/Daemon/Core existing boundaries remain connected to cleanup evidence |

## 3. Proof ceiling and handoff

H33 proof ceiling is `source`: cleanup status, Unknown isolation and retention contract are defined;
existing supervisor/shutdown/release source is guarded. Real process/panic/shutdown stress, durable
cross-process resource projector, external stop receipts and live/physical proof remain H36/PD/ER.
