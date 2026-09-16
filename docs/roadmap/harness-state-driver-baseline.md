# H03 Harness state-driver baseline

> 快照日期：2026-09-16。本文记录 H03 的纯状态驱动器与 Harness 门面接线；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`H03`](harness.md#step-h03) |
| source snapshot | `86673f6`（H02 完成后的干净基线） |
| feature_status | `implemented`（pure state-driver source + Harness integration） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | RunnerPort → KianaHarness facade → RunDriver::transition → existing model/Broker I/O → RunnerEvent/ControlPlane |
| this step does | 提取可序列化 RunFrame/TurnFrame、显式 phase/intent 转移、单 driver ownership、bounded mailbox admission；将 Harness 递归 model step 改为显式迭代并在 step/model/tool/cancel 边界更新 driver |
| this step does not | 不新增模型循环、Broker/权限判断、Session durable queue、跨进程 driver lease、真实 Provider 或外部 effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Pure state driver | `kiana-runner/src/state_driver.rs`, `kiana-runner/src/lib.rs` | `4c468299a4ab79ed348eb42b4e1d58304375c76ed751f666eb05c7c8d596d957`, `8a1cd24405b6e9fb7d3956f010a72c47683d509647981726389f973079280a55` |
| Harness facade/integration | `kiana-runner/src/harness.rs`, `kiana-runner/src/inbox.rs` | `77bc0f48a4e886d0d0eb52dd861d3c551e2249555155372f77fe0df6888e1429`, `403f97d22be0254a22c2b1f2dc10066a934858bbb7e64b4345735056eff0b1eb` |
| Remote fixtures/workflow | `kiana-runner/tests/h03_state_driver.rs`, `.github/workflows/h03-state-driver.yml` | `1024c060f867c4848143253ce57ba7eaca3a021694de544c62174e9de860e314`, `de40313f9d0074229f83edf3ddd35e14a970c71e714e03300b519914f8c36df0` |

hash 只用于 H03 源码漂移复核，不构成模型、Broker、持久化或外部效果证明。

## 2. RunFrame/TurnFrame contract

`RunDriver` 是一个无 I/O 的 reducer。`RunFrame` 保存 run/turn identity、phase、当前 StepId、step number、pending tool 数、driver owner、mailbox capacity、accepted input count 和 terminal observation；`TurnFrame` 是同一状态的 turn-scoped projection。`DriverInput` 携带外部生成的 ID 和 effect-known observation，`transition(frame,input)` 只返回新 frame 与 `DriverIntent`，不读取时钟、不分配 ID、不调用模型或 Broker。

允许的关键边界：

- `Idle/Queued → Preparing → ModelPending → ToolPending → ModelPending`；
- model complete → `TurnFinished`；cancel request → `Cancelling → TurnFinished` 或 `RecoveryRequired`；
- 未知工具 effect → `RecoveryRequired`，不可继续当作普通失败；
- 已有 driver owner 时，其他 owner 立即拒绝；mailbox 到达 bounded capacity 后拒绝新输入且计数不回退。

## 3. Harness integration

`KianaHarness::model_step` 现在显式循环调用 `model_step_once`，消除自调用递归。每次 model step 由 driver 接收一个 StepId；model output、tool batch、tool result、cancel 和 checkpoint 都沿同一 `RunDriver` 更新。现有 `RunnerPort`、模型 client、CapabilityRequest 和 EventLog/ControlPlane 交互保持原路径；state driver 只做状态/intent，不成为第二事实源。ActiveRun checkpoint 带可选 RunDriver，旧 checkpoint 缺字段时以受限默认 driver 解码并继续既有校验。

Steer/Inject 先通过 driver 的 mailbox admission，再写现有 Inbox；满载时在写入前返回 `harness_driver_mailbox_full`，已接收消息不会静默丢失。Session 级跨 Run 排队和 durable ACK 仍由 H18/PD 后续步骤负责。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `second_driver_for_same_turn_is_rejected` | 同一 RunFrame 已有 owner 时第二 driver 不能取得执行权 |
| `full_mailbox_does_not_drop_accepted_input` | mailbox 满载拒绝新输入，accepted count 保持不变 |
| `blocked_model_does_not_block_other_run_or_cancel` | 一个 run 的取消/阻塞状态不污染另一个独立 driver |
| `stream_and_buffered_calls_share_transitions` | 纯 reducer 与门面调用得到同一 frame/intent |
| `state_driver_frames_are_versioned_and_validated_without_io` | frame schema、run/turn 关联和边界可序列化校验 |
| `harness_uses_one_explicit_driver_loop` | Harness 没有递归/第二 model loop，所有 step 通过同一 driver |

`.github/workflows/h03-state-driver.yml` 在 GitHub runner 执行 runner 纯状态夹具与 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- RunDriver/ActiveRun 仍位于进程内；checkpoint 字段可序列化不等于已建立跨进程 durable queue/lease 或恢复事实。
- 当前 driver owner 是 Harness 内部 fence，不替代 ControlPlane authorization、permit、EventLog CAS 或 external effect receipt。
- 模型响应结构化无损、stop/retry 分类、完整流 reducer、Session mailbox durability 和 provider adapter 仍是 H04+。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
