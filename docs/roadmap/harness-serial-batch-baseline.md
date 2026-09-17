# H12 Serial Tool Batch 基线

> 快照日期：2026-09-18。本页记录工具批次的 phase、顺序屏障和取消排空边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H12`](harness.md#step-h12) |
| feature_status | `implemented`（queued/dispatched/settled phase + serial barrier） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane/Broker still authorize and execute each request; runner owns only ordered pending state |
| this step does | complete batch queued before first dispatch、wrong result identity/phase rejection、serial next-call barrier、cancel drain of not-started siblings、replay-safe synthetic results |
| this step does not | 不并行派发、不把 queued synthetic result 当真实 effect、不替代 H13 durable Invocation ledger、不扩大 cancel authority |

## 1. Contract

`PendingTool` 绑定稳定 request ID、provider call identity、arguments 和 `Queued/Dispatched/Settled`
phase。完整 assistant batch 先全部映射成功，才进入 queue；只有 front entry 处于 Dispatched 且
request ID 匹配时才弹出并结算。错误 ID 或尚未 dispatch 的回包不会消费 pending，并将 run 保留
待正确结果。

每次只向 ControlPlane 发出一个 `CapabilityRequested`。前项 observation 到达后，才允许下一个
Queued entry 转为 Dispatched；队列清空后才启动下一 model step。取消时已 dispatch 的 front 视为
可能在途，剩余 Queued entries 生成带 `not_executed=true`、`replay_safe=true` 的 `ToolCancelled`
事件，绝不继续派发。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `wrong_result_does_not_consume_pending_call` | 错 request ID 不弹出 pending，随后正确回包仍能完成 |
| `cancelled_batch_never_dispatches_remaining_calls` | 取消三调用批次只留下首项在途，剩余两项合成未执行结果 |
| `three_serial_calls_produce_three_ordered_results_before_model` | 三个调用按声明顺序逐个派发，全部结果后才请求模型 |
| `h12_batches_are_serial_and_wrong_results_do_not_advance` | source guard 固定 phase/屏障/取消排空和 no-bypass |

## 3. Proof ceiling and handoff

H12 proof ceiling 为 `source`：runner 串行批次和错误回包/取消边界已接入，CI-only fixtures/source
guard 固化。批次 declaration、dispatch permit、每个 outcome 的 durable 立即持久化、跨进程 CAS、
审批恢复和 external/live/physical effect proof 仍留待 H13+ / CP/PD/INT。
