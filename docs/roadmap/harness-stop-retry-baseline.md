# H05 model stop, error and retry baseline

> 快照日期：2026-09-16。本文记录 H05 的模型 StopReason、错误阶段、retry 分类和完整性边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`H05`](harness.md#step-h05) |
| source snapshot | `dec35a1`（H04 模型内容切片后的干净基线） |
| feature_status | `implemented`（typed stop/error/retry source + Harness gate） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | Provider response → ModelFinish/ModelStopReason → ModelReply/ModelError/ModelOutcome → Harness transition → RunnerEvent/ControlPlane |
| this step does | 统一 stop reason vocabulary、ModelOutcome、ModelError phase/retry/side-effect fields；length/refusal/incomplete/unknown stop 在 Harness 完成或工具派发前 fail-closed；保留 provider 原始 detail 供诊断 |
| this step does not | 不改变 H06 流式 accumulator、不引入自动 effect retry、不把 provider 传输成功当业务成功、不宣称 live/physical effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain stop/error contract | `kiana-domain/src/model.rs`, `kiana-domain/src/contracts.rs` | `6243275aa959281071e4ecfe6820d6feeda63170f5e41b0ffdc105519bf16510`, `94f8430b9139fc22003197a757d14b3d2ffb47b6a5cb493145078c6be0d124ff` |
| Harness/provider enforcement | `kiana-runner/src/harness.rs`, `kiana-provider/src/response.rs` | `06075ed0c651ba946dd65c9ae9989e7c1915dfb2f578ea5324d2c3bde4fe2c82`, `06b416a096671fee2637b5c3c782a7e9042f21a23c7f27fd80b85db6a3dbbe66` |
| Fixtures/workflow | `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs`, `.github/workflows/h05-stop-retry.yml` | `147266cd44c4824f4a4d754ad43137daefa3831b2f0ca233acc16219bb40bda9`, `de3696988a730e33e764d839b9c23a66636e5126fde91326394c7dad69afa440`, `e458eee8925eb353b6a288c8e47571b16b0906ff540720c3132c6aaa67bb7bab` |

hash 只用于 H05 源码漂移复核，不构成 provider 网络、账单或外部效果证明。

## 2. Typed stop and outcome

`ModelFinish` 仍是 Provider 完整响应的内部完成类型；`ModelStopReason` 是跨 adapter 的闭合分类，额外含 `Unknown`。`ModelOutput::normalized_stop_reason()` 对缺失/未知 stop 返回 Unknown；`ModelReply::legacy` 只有在 `ModelFinish::require_complete()` 通过后才可交给 Harness，并为旧 cassette 缺失 stop 补上明确的 `end_turn`/`tool_use`。

`ModelError` 统一带 `code / phase / retry_class / request_sent / retry_after_ms / safe_message / side_effect_state`；`ModelOutcome` 只输出受限的 stop、retry、phase、error code 和 side-effect 认知，原始 provider body/headers 不在其中。传输错误在 request 已发送时保守标为 `side_effect_state=unknown`，但不等于 capability effect 已发生。

## 3. Harness and provider gates

- Harness 在处理 `ModelOutput` 后先校验 normalized stop；`length`、`refusal`、`pause`、`incomplete`、Unknown 不会完成 Turn，也不会进入工具队列。
- Provider response parser 对缺 stop、重复 finish、open tool block、截断 SSE、未知 stop 和 refusal 返回稳定 `ModelError`；已有 retry 只允许 `BeforeSend`/`Rejected` 且受剩余 deadline/预算约束。
- `run.model_turn` metadata 同时记录 typed stop reason 和 bounded ModelOutcome；CLI/模型回灌消费的是这一错误分类，诊断 detail 保留但不参与 retry/authority 决策。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `length_stop_never_dispatches_tools_or_completes_turn` | length 响应即使携带工具也不能完成或派发 |
| `refusal_is_not_success` | refusal 归一化为失败，side-effect/retry 语义不伪造成功 |
| `unknown_stop_reason_fails_closed` | 未知 stop 进入稳定 ModelError/Unknown，不猜测 end_turn |
| `normal_text_stop_finishes_and_tool_stop_continues` | end_turn 完成文本，tool_use 继续工具路径 |
| `harness_stop_and_retry_paths_are_typed_and_fail_closed` | Harness/provider 使用 typed stop/retry markers，禁止文本 contains 控制状态 |

`.github/workflows/h05-stop-retry.yml` 在 GitHub runner 执行 domain outcome fixtures、runner source guard 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前错误分类和 stop gate 是本地领域/adapter合同；H06 负责流式分片一致性、H07 预算贯通、H08 静默 I/O 取消。
- `side_effect_state=unknown` 只表示模型请求/传输边界不确定，不替代 capability attempt 的 effect/stop projection。
- Provider-specific stop detail 和 response IDs 仍只作受限诊断，不能跨 route/model 复用；live provider、账单、外部业务 Outcome 和 physical effect 未证明。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
