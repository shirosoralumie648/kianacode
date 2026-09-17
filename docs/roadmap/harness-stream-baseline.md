# H06 Stream Normalizer 基线

> 快照日期：2026-09-17。本页记录模型 attempt 的 stream accumulator 边界；增量展示是易失
> 视图，只有完整、校验过的 `ModelOutput` 才能继续到工具/turn 状态机。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H06`](harness.md#step-h06) |
| feature_status | `implemented`（per-attempt ModelStreamAccumulator） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | Runner accumulator validates shape; ControlPlane/EventLog still own authorization and facts |
| this step does | Text/ToolArguments/Usage/Stop fragments、attempt identity、delta/text/tool limits、tool JSON/object/identity validation、stop/eof/usage conflict、cancel late-delta fence、single complete output path |
| this step does not | 不从 partial JSON 派发工具、不把 EOF/Unknown/length 当正常完成、不以增量/UI transcript 取代 EventLog、不新增模型循环 |

## 1. Contract

`ModelStreamAccumulator` 是每个 model attempt 的唯一拼接器。Text 增量仅在配额内追加；tool
argument 按 index/id/name 缓存，完整 stop 后才 parse bounded JSON 并与 final `ModelOutput`
逐项对账；Usage 不能回退，Stop 不能重复/冲突，EOF 缺 stop 返回
`eof_without_stop_never_completes`。取消后任何晚到 delta 返回
`late_delta_after_cancel_is_discarded`。

`KianaHarness::invoke_model` 在每次 attempt 建立 accumulator，所有 callback delta 先进入
accumulator，再经过 StreamingRedactor 进入临时 `RunnerEvent::Delta`；provider 返回的完整
output 还必须通过同一 accumulator.finish。工具请求只由 finish 后的完整 output 生成，失败时
保留 model attempt error，不进入工具派发。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `interleaved_tool_deltas_equal_nonstream_response` | 交错文本/工具分片与非流式完整 output 等价 |
| `split_invalid_tool_json_has_zero_dispatches` | 分片 JSON 不完整时 fail-closed、零工具派发 |
| `eof_and_late_cancel_deltas_never_complete` | EOF 无 stop 与取消后 late delta 均拒绝 |
| `h06_harness_uses_one_attempt_stream_accumulator` | Harness 单 accumulator、limits、validation 和 no-second-stream source guard |

## 3. Proof ceiling and handoff

H06 proof ceiling 为 `source`：runner/domain accumulator 与 Harness 接线已由 CI-only fixtures/source
guard 固化，未运行本地测试。真实 provider 多块/重连/网络截断、stream usage billing、跨进程
attempt persistence、terminal/recovery 和 live/physical proof 仍留待 H07+ / P4 / ER/PD/INT。
