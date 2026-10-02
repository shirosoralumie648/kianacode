# H05 model stop, error and retry baseline

> 首次快照：2026-09-16；typed recovery 分类切片：2026-10-02（基线 `e17143f7`）。本文记录 H05 的模型 StopReason、错误阶段、retry/recovery 分类和完整性边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`H05`](harness.md#step-h05) |
| source snapshot | `e17143f7`（H05 typed recovery classification slice base） |
| feature_status | `partial`（typed stop/error/recovery contract + transport retry gate; repair execution remains unwired） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | Provider response → ModelFinish/ModelStopReason → ModelReply/ModelError/ModelOutcome → Harness transition → RunnerEvent/ControlPlane |
| this step does | 统一 stop reason vocabulary、ModelOutcome、ModelError phase/retry/recovery/side-effect fields；length/refusal/incomplete/unknown stop 在 Harness 完成或工具派发前 fail-closed；Domain retry policy 仅接受 `TransportRetry`，Runner 对未接线 repair disposition 明确拒绝；保留 provider 原始 detail 供诊断 |
| this step does not | 不改变 H06 流式 accumulator、不引入自动 effect retry、不把 provider 传输成功当业务成功、不宣称 live/physical effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Original H05 source snapshot | `kiana-domain/src/model.rs`, `kiana-domain/src/contracts.rs`, `kiana-runner/src/harness.rs`, `kiana-provider/src/response.rs` | `6243275aa959281071e4ecfe6820d6feeda63170f5e41b0ffdc105519bf16510`, `94f8430b9139fc22003197a757d14b3d2ffb47b6a5cb493145078c6be0d124ff`, `06075ed0c651ba946dd65c9ae9989e7c1915dfb2f578ea5324d2c3bde4fe2c82`, `06b416a096671fee2637b5c3c782a7e9042f21a23c7f27fd80b85db6a3dbbe66` |
| Explicit-incomplete source at integrated commit `958ece21` | `kiana-domain/src/model.rs`, `kiana-provider/src/response.rs`, `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `30724bcc1dc42b2129712f92c67cd709597b39c4e17e09c36a00ad37d8a4b702`, `95590f54331f8b86f5ab7651134a932caf5606fddc1eaaaba3daead26d12b301`, `91264fcc6c82a91587db80d4afde52c12611c615abe1def1059824eceaa1cf6f`, `5eb206a2e2241db713fe2c86452e428efb103364548c60d7f480573a50dc2051` |
| Current stop and typed-recovery source/fixtures | `kiana-domain/src/model.rs`, `kiana-provider/src/response.rs`, `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `050ca8aae19f62ef4f6adf18583e7d0bccae3304c8c69800fc58b1c0bbea416e`, `51b4ec0c38382c0d7695ad1070c4f6dcd8dc4ac498bbbd1bb1366abb7060679a`, `7d6ef0b5ad7acfffb4b69856daeb78c23f1ba86c27840e4d8fe70ee4c2a241b4`, `5faf66f8339a2d889550eca995467d203e05f89bd334c9cb090b12be20c385ef` |
| Typed recovery classification slice | `kiana-domain/src/model.rs`, `kiana-domain/src/retry_policy.rs`, `kiana-runner/src/retry.rs`, `kiana-runner/src/harness.rs`, `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `050ca8aae19f62ef4f6adf18583e7d0bccae3304c8c69800fc58b1c0bbea416e`, `c8a123777f93443d30a6838bdf256dffbd8d4c70ca86767f955e7bad5c9093be`, `6d864b9101eb28d544d4af2871e924cb40f93ae6849d8926e1d9042b01c203fe`, `24cf1673137fc7e5be3f136750a1b6e3b8107e58a7449f511e2fe18208465ef2`, `7d6ef0b5ad7acfffb4b69856daeb78c23f1ba86c27840e4d8fe70ee4c2a241b4`, `5faf66f8339a2d889550eca995467d203e05f89bd334c9cb090b12be20c385ef` |
| Original H05 fixtures | `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `c570a3cc4fe1f9c7c9ca2a6cd6c50ac1f67d441d0dd74fd02b42718f214e5b34`, `402ed08a46d6222da228635e51cd60d8a507a614fd74caf1c36b7279fc8a7331` |
| Current CI wiring | `.github/workflows/ci.yml`, `scripts/ci/test-shards.json` | `038675197ece4752f335d4265f229aaa422a46df287c9fb94dd6387b3a9d0ef2`, `73fe32c73194833a25a120ea459f57c7c72f283fc77cac1e5a28d91cc4f323b6` |
| Historical standalone workflow | `.github/workflows/h05-stop-retry.yml` | `e458eee8925eb353b6a288c8e47571b16b0906ff540720c3132c6aaa67bb7bab` |

hash 只用于 H05 源码漂移复核，不构成 provider 网络、账单或外部效果证明。

## 2. Typed stop and outcome

`ModelFinish` 仍是 Provider 完整响应的内部完成类型；`ModelStopReason` 是跨 adapter 的闭合分类，额外含 `Unknown`。`ModelOutput::normalized_stop_reason()` 对缺失/未知 stop 返回 Unknown；`ModelReply::legacy` 只有在 `ModelFinish::require_complete()` 通过后才可交给 Harness，并为旧 cassette 缺失 stop 补上明确的 `end_turn`/`tool_use`。

显式 `incomplete` 统一归一化为 `ModelFinish::Incomplete` / `ModelStopReason::Incomplete`，并以 `model_transport_incomplete` 结束 Provider 与 Harness 调用；OpenAI Responses 和 Gemini Interactions 的完整响应状态及流事件均映射到同一错误。包含 tool call 的 incomplete 响应不会形成 `ModelReply`，Runner 也不会发出 `CapabilityRequested` 或 `Completed`。

`ModelError` 统一带 `code / phase / retry_class / request_sent / retry_after_ms / safe_message / side_effect_state`；`ModelOutcome` 只输出受限的 stop、retry、phase、error code 和 side-effect 认知，原始 provider body/headers 不在其中。传输错误在 request 已发送时保守标为 `side_effect_state=unknown`，但不等于 capability effect 已发生。

## 3. Harness and provider gates

- Harness 在处理 `ModelOutput` 后先校验 normalized stop；`length`、`refusal`、`pause`、`incomplete`、Unknown 不会完成 Turn，也不会进入工具队列。
- Provider response parser 对缺 stop、重复 finish、open tool block、截断 SSE、未知 stop 和 refusal 返回稳定 `ModelError`；已有 retry 只允许 `BeforeSend`/`Rejected` 且受剩余 deadline/预算约束。
- `run.model_turn` metadata 同时记录 typed stop reason 和 bounded ModelOutcome；CLI/模型回灌消费的是这一错误分类，诊断 detail 保留但不参与 retry/authority 决策。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `length_stop_never_dispatches_tools_or_completes_turn` | Runner 收到带 shell tool call 的 length 响应后发出 `model_output_truncated`，且没有发出 `CapabilityRequested` 移交给 ControlPlane/Broker 或 `Completed` |
| `incomplete_stop_never_dispatches_tools_or_completes_turn` | Runner 收到带 shell tool call 的 incomplete 响应后发出 `model_transport_incomplete`，且没有发出 capability handoff 或 `Completed` |
| `length_stop_is_rejected_by_legacy_reply_conversion` | Domain legacy reply conversion 将 length 响应拒绝为 `model_output_truncated`；不单独证明 Runner 派发边界 |
| `explicit_incomplete_stop_is_not_success_even_with_tool_calls` | Domain 将 incomplete 分类为 `ModelStopReason::Incomplete` 并拒绝带工具调用的响应 |
| `explicit_incomplete_statuses_share_one_fail_closed_outcome` | Anthropic、OpenAI Chat/Responses、Ollama 与 Gemini 的显式 incomplete 完整响应均映射为 `model_transport_incomplete` |
| `explicit_incomplete_stream_events_share_one_fail_closed_outcome` | OpenAI Responses 与 Gemini 的显式 incomplete 流事件映射为相同的 typed outcome |
| `refusal_is_not_success` | refusal 归一化为失败，side-effect/retry 语义不伪造成功 |
| `unknown_stop_reason_fails_closed` | 未知 stop 进入稳定 ModelError/Unknown，不猜测 end_turn |
| `normal_text_stop_finishes_and_tool_stop_continues` | end_turn 完成文本，tool_use 继续工具路径 |
| `harness_stop_and_retry_paths_are_typed_and_fail_closed` | Harness/provider 使用 typed stop/retry markers，禁止文本 contains 控制状态 |
| `recovery_dispositions_are_typed_and_survive_error_round_trips` | 五类 recovery disposition 在 ModelError serde round trip 后保持 typed 值 |
| `legacy_model_error_and_retry_observation_fail_closed_without_disposition` | 旧错误缺 recovery 字段时采用 Terminal，旧 RetryObservation 缺 phase/disposition 时策略校验失败 |
| `unknown_recovery_disposition_is_rejected` | 未知未来 disposition serde 解码失败 |
| `retry_policy_denies_non_transport_recovery_dispositions` | 只有 TransportRetry 能进入现有有界 retry policy |
| `typed_recovery_disposition_bounds_runner_routing` | 两种安全 transport 分类按既有策略 retry；format/tool/context repair 明确失败且不重试；Terminal 保持原错误 |

`.github/workflows/ci.yml` 在 GitHub runner 的 `kiana-domain-s3/4`、`kiana-runner` 和 `kiana-provider` shards 执行这些 domain outcome、Runner 行为和 Provider parser fixtures；本切片复用现有 `h05_model_outcome` / `h05_stop_guard` targets，不改 shard map。本地不运行测试。

Exact prior H05 receipts: run `37008943358` / head `cc303315`, Runner job `110844252604` passed all 3 `h05_stop_guard` tests, Domain job `110844252435` passed all 5 `h05_model_outcome` tests, and Provider job `110844252490` passed both `explicit_incomplete_*` fixtures. The overall run was later cancelled; each relevant shard was red on sibling targets. H05 production/source/test/shard files were byte-identical from `cc303315` through base `e17143f7`. The typed-recovery fixtures in this slice have no CI receipt.

## 5. 限制与交接

- 当前错误分类和 stop gate 是本地领域/adapter合同；H06 负责流式分片一致性、H07 预算贯通、H08 静默 I/O 取消。
- `ModelRecoveryDisposition` is typed, but no current production provider error maps to FormatRepair/ToolRepair/ContextRepair. Runner reports the corresponding `*_repair_unavailable` failure; the existing dead-code OutputRepair helper remains inactive and no second repair loop is introduced.
- TransportRetry still requires the existing `RetryPolicy` class/status/effect checks, per-attempt budget reservation, deadline, observed-delta fence and cancellation-aware delay. Legacy ModelError without a disposition becomes Terminal; unknown values fail deserialization.
- `side_effect_state=unknown` 只表示模型请求/传输边界不确定，不替代 capability attempt 的 effect/stop projection。
- Provider-specific stop detail 和 response IDs 仍只作受限诊断，不能跨 route/model 复用；live provider、账单、外部业务 Outcome 和 physical effect 未证明。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
