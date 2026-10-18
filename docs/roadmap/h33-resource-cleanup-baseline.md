# H33 运行资源、关闭和异常退出清理基线

> 快照日期：2026-10-19。本页记录 H33 完成切片：运行时清理生产者、panic 证据、daemon shutdown
> 集成、retention 上限接线，以及本地实际运行的拒绝优先夹具。CI lane 是手动诊断通道，回落执
> 行结果不作为本地证据。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H33`](harness.md#step-h33) |
| source snapshot | `28dc3cf6` + 本 H33 完成切片（分支 `step/h33-20261018-1`） |
| feature_status | `partial`：owner 绑定的清理计划/报告、运行时生产者、panic 证据、daemon shutdown 取消与落账、retention 上限已接入组合根；MCP 连接/临时文件/JobHandle 的 owner 采样仍缺 |
| proof_level | `local_behavior`：domain / runner / core / daemon 的聚焦 target 在本地实跑通过；跨进程、外部停止与三类未采样 owner 无证据 |
| authority | Cleanup reducer 只记录证据；adapter/supervisor 拥有真实停止与释放；EventLog/ControlPlane 拥有恢复与所有权事实 |
| this step does | 从 run 结束时真实存在的 owner 证据构造 `ResourceCleanupReport` 并写 `resource_cleanup` 聚合；panic 中止把资源标为 `IsolatedUnknown`；shutdown 先取消在飞 run 并写 `Shutdown` 报告、观测失败再写独立 `LogFailure` 报告，全部先于 `close_event_store`；默认 `ResourceRetention` 在组合根翻译给 runner，约束已终态 run/frame/history 字节 |
| this step does not | reducer 不杀进程、不释放 OS 锁、不删临时文件；Host drop 不是停止确认；不提供跨进程资源 projector、真实外部停止收据或 live provider 证据；MCP 连接、临时文件与 JobHandle 的 daemon owner 表尚未进入 run 清理报告（见 §5） |

## 1. Contract

`ResourceCleanupPlan::observe_all` 只接受 adapter 的 stop/effect 证据。需要 stop confirmation
而没有确认，或 effect 未知，资源只能进入 `IsolatedUnknown`；只有 `Released` 资源允许上层
进一步释放 ownership。未收到观察的资源保持 Pending。`ResourceRetention` 为 retained runs、
frames 和 history bytes 提供明确上限，任一维度为零是 `resource_retention_invalid`，超出是
`resource_retention_exceeded`。

`kiana-core/src/resource_cleanup_producer.rs` 是运行时**生产者**：它读取本进程的 owner 状态
（in-flight 登记、执行停止集合、Builder 路径锁、panic 证据、观测失败原因码）构造报告，并且
只在报告 `safe_to_release_all()` 时调用受守卫的路径锁释放。报告写 `resource_cleanup` 聚合
（按 run 分线，事件 kind `resource.cleanup_report`），绝不写 run 聚合或 `resource_quarantine`
聚合，因此清理证据不能改写 run 终态，也不能伪装成释放事实。

现有 ProcessSupervisor 的 bounded TERM/KILL/reap、`ProcessGuard` kill-on-drop、Runner
RunCancellation/in-flight unregister、Core TerminalScope/path-lock release 和 Daemon graceful
shutdown 仍是实际边界；本步只把它们的证据语义统一写成可验证 contract 并接到运行路径上。

## 2. Failure-first fixture matrix

先拒绝、后成功；每个拒绝夹具都断言稳定拒绝码。

| Target | Fixture / guard | 断言 |
|---|---|---|
| `kiana-domain/tests/h33_resource_cleanup.rs` (6) | `unconfirmed_process_or_lock_never_becomes_released` | 缺 stop/effect 证据隔离为 Unknown，不释放 ownership |
| | `confirmed_normal_cleanup_releases_everything_and_retention_is_bounded` | 成功：确认清理全部 Released，retention 超限 `resource_retention_exceeded` |
| | `panic_and_log_failure_keep_unobserved_resources_pending` | 无观察保持 Pending，不静默变干净 |
| | `unconfirmed_process_cannot_release_write_ownership_to_next_run` | `path_lock_release_requires_confirmed_stop`；下一个 run 的同一锁仍只能 Unknown |
| | `panic_does_not_leave_untracked_background_work` | panic 资源全部 Pending（无观察），不宣称已释放 |
| | `many_short_runs_return_handles_locks_and_memory_to_baseline` | 成功：8 个短 run 全部 Released，retention 基线/超限稳定 |
| `kiana-runner/tests/h33_resource_cleanup_behavior.rs` (6) | `panic_does_not_leave_untracked_background_work` | `result_unknown:harness_panic`；in-flight 清除、panic 证据一次认领、全部 `stop_confirmed=false` |
| | `unconfirmed_process_cannot_release_write_ownership_to_next_run` | 未确认停止不得登记为可继续/可释放 |
| | `retention_budget_rejects_zero_dimensions` | `resource_retention_invalid` |
| | `retention_does_not_silently_trim_history_bytes` | `resource_retention_exceeded`，不截断历史 |
| | `retained_runs_and_history_stay_within_budget_without_evicting_active_runs` | 终态 run 有界裁剪，挂起 run 不被裁剪 |
| | `many_short_runs_return_handles_locks_and_memory_to_baseline` | 成功（放最后）：短 run 基线回收、释放幂等 |
| `kiana-core/tests/h33_resource_cleanup_behavior.rs` (3) | `unconfirmed_process_cannot_release_write_ownership_to_next_run` | `writable_scope_release_requires_confirmed_stop` + `path_lock_release_requires_confirmed_stop`；报告 `released_count == 0` |
| | `panic_does_not_leave_untracked_background_work` | panic 证据 → `CleanupCause::Panic`，全部 IsolatedUnknown，证据只认领一次 |
| | `many_short_runs_return_handles_locks_and_memory_to_baseline` | 成功：8 个 run 都写 Normal 报告、全 Released；retention 超限稳定 |
| `kiana-daemon/tests/h33_resource_cleanup_shutdown.rs` (2) | `shutdown_cannot_claim_in_flight_run_stopped` | 关闭报告 `safe_to_release_all == false`、model_task `IsolatedUnknown`（`model_task_shutdown_unconfirmed`）；LogFailure 报告含 `observability_subscription`（`observability_flush_cancelled`） |
| | `shutdown_persists_cleanup_reports_before_event_store_close` | 成功：`close()` 内部快照同时含 Shutdown 与 LogFailure 报告且顺序正确；run 聚合无清理事件 |
| `kiana-core/tests/h33_resource_cleanup_guard.rs` (3) | `cleanup_contract_and_existing_supervisors_keep_unknown_isolated` | domain/runner/supervisor/daemon/core marker 仍连到清理证据 |
| | `cleanup_does_not_equate_host_drop_with_confirmed_stop` | observe/validate 源码：只有 confirmed+effect known 才 Released |
| | `cleanup_producer_reads_owner_evidence_and_is_wired_to_run_end_and_shutdown` | 生产者只读 owner 证据、不执行 OS 级清理、报告写 `resource_cleanup`；run 结束与 shutdown 均接线，证据先于 `close_event_store` |
| `kiana-core/tests/er16_terminal_uniqueness.rs` (3) | 既有回归 | 终态唯一性：清理报告不进 run 聚合，run 流仍只有一条终态 |
| `kiana-daemon` lib target (`cargo test -p kiana-daemon --lib`) | `default_resource_retention_is_accepted_by_the_runner` | `DEFAULT_RESOURCE_RETENTION` 被 runner 接受，基线快照为空 |

## 3. Local evidence

```text
cargo test -p kiana-domain --test h33_resource_cleanup                          -> 6 passed, exit 0
cargo test -p kiana-runner --test h33_resource_cleanup_behavior                 -> 6 passed, exit 0
cargo test -p kiana-core --test h33_resource_cleanup_behavior                   -> 3 passed, exit 0
cargo test -p kiana-core --test h33_resource_cleanup_guard                      -> 6 passed, exit 0
cargo test -p kiana-core --test er16_terminal_uniqueness                        -> 3 passed, exit 0
cargo test -p kiana-daemon --test h33_resource_cleanup_shutdown                 -> 2 passed, exit 0
cargo test -p kiana-daemon --lib default_resource_retention                     -> 1 passed, exit 0
```

全部使用 `--locked --offline --test-threads=1`（`--locked` 用于 CI lane 命令；本地为
`--offline`）。`cargo check -p kiana-daemon --tests --offline` 为 exit 0（既有 warning）。

## 4. CI lane

`.github/workflows/h33-resource-cleanup.yml` 是 `workflow_dispatch` only，`permissions: contents: read`；
步骤覆盖 `cargo fetch --locked`、fmt 检查、domain/runner/core/daemon 四个 target、三组
先拒绝后成功的具名夹具循环、core source guard、er16 回归，以及各 target 的 `--no-run` 编译。
Lane 不等待回执，也不把 CI 结果当作本地证据。

`scripts/ci/test-shards.json` 的自动分片登记了 `h33_resource_cleanup`（domain）、
`h33_resource_cleanup_behavior` 和 `h33_resource_cleanup_guard`（core）；daemon/runner 由整 crate
分片覆盖。

## 5. Remaining exit condition (row stays 🔄)

卡片第 1 条要求「模型任务、工具 future、MCP 连接、临时文件、JobHandle、path lock、budget lease
和订阅」每份资源都有 owner 与释放条件。运行时生产者目前采样的是 Core 拥有的 run 级 owner：
模型任务、执行停止集合（工具 future）、Builder 路径锁、预算租约与订阅。三类资源的 owner 表
在 daemon adapter 里，尚未在 run 结束/取消时汇总：

- `McpConnection`：`kiana-daemon/src/harness_mcp.rs` 的 `McpRegistry`；
- `TemporaryArtifact`：workspace/execution 临时目录（`kiana-daemon/src/execution_workspace.rs`）；
- `JobHandle`：`kiana-daemon/src/execution_control.rs` 的 `ExecutionControl::processes`
  （`identity.run_id` 已携带，但没有按 run 汇总的读取入口）。

这些资源当前只会在 panic 证据里按 kind 出现（`HarnessPanicResource.kind` 已映射），正常结束/
取消/关闭路径不会为它们写 `Pending`/`IsolatedUnknown` 条目，也没有对账入口。因此行 588 与专项卡
保持 🔄；接通需要新增 daemon→Core 的按 run owner 采样（不新增执行循环、不直接杀进程），并补
「未确认停止的 MCP 连接/临时文件/JobHandle 不得报告为 Released」的拒绝夹具。

## 6. Proof ceiling and limitations

上限是 `local_behavior`，并且是进程内证据：

- EventStore 在 daemon 夹具中是 `MemoryEventLog` 的包装（补 flush/close，仅采集 close 时刻快照），
  不是 durable 日志；关闭顺序的证明依赖该包装而不是磁盘布局。
- 夹具证明「取消消息已送达 runner、报告保持 Unknown」，不证明外部进程/工具 future 已经停止；
  runner 夹具的模型任务是进程内 future，不是 OS 进程。
- 不释放 OS 文件锁；路径锁释放是进程内 fencing，`IsolatedUnknown` 时锁保留。
- 生产者只采样 Core 拥有的 run 级 owner（模型任务、执行停止集合、Builder 路径锁、预算租约、
  订阅）。`JobHandle` / `McpConnection` / `TemporaryArtifact` 的 owner 表位于 daemon adapter
  （`ExecutionControl`、MCP registry、临时目录），run 结束时尚未汇总进报告；这些资源当前只有
  panic 证据能按 kind 命名，真实收割仍由各自 adapter/supervisor 负责。这是本步明确的缺口，
  不宣称「全部外部资源已清理」。
- retention 只约束进程内 retained run/frame/history bytes；不移除 EventLog 历史，不做跨进程
  projector。
- shutdown 观测 drain 上限 250ms 是本地时序参数；慢消费者/真实网络投递未覆盖。
- 未运行 `cargo test --workspace`、clippy、smoke 或 release gate；GitHub Actions 回执未等待。
- 跨进程资源 projector、durable 清理事实、外部停止收据和 live/physical 证据仍留给 H36/PD/ER。
