# P4-J7-05 strict non-streaming tool response baseline

> 快照日期：2026-09-16。本文记录 Provider 非流式工具响应的严格解析与身份保留；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P4-J7-05`](provider.md#step-p4-j7-05) |
| source snapshot | `6287b00`（H05 完成后的干净基线） |
| feature_status | `implemented`（legacy provider parser + daemon fixture parser source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | provider response JSON → strict non-stream parser → ModelToolCall/MessageResponse → Harness/ControlPlane capability request |
| this step does | OpenAI-compatible/Ollama non-stream tool ID/name/arguments/finish/terminal checks；duplicate IDs、坏 JSON、缺身份、非 object 参数和 unknown content fail-closed；保留 Ollama 无原生 ID 的唯一 ordinal correlation 规则 |
| this step does not | 不接通新的 provider、隐藏 SDK tool loop、自动修复坏 JSON、跨 provider identity、外部 effect 或账单/live 证明 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Legacy provider response/request parser | `kiana-services/src/api/provider.rs` | `bb4fc17a316fc5dd2bd910eb10151d948fe66f7fc4ad475bd573e254bb165fbb` |
| Owned provider/daemon parser | `kiana-provider/src/response.rs`, `kiana-daemon/src/model_client.rs` | `06b416a096671fee2637b5c3c782a7e9042f21a23c7f27fd80b85db6a3dbbe66`, `32aec09c6a47cb9f090fd3749164c693b9687c24d7efb0b9d0c9db709cbaec53` |
| Fixtures/workflow | `kiana-services/src/api/provider.rs` tests, `.github/workflows/provider-strict-response.yml` | `bb4fc17a316fc5dd2bd910eb10151d948fe66f7fc4ad475bd573e254bb165fbb`, `342d4af17b535080d42b6be3f32c73d69e2cabb25bd7de24d1d99a57be5e2b86` |

hash 只用于 P4-J7-05 源码漂移复核，不构成真实 provider/live 请求或外部效果证明。

## 2. Strict non-stream contract

- OpenAI-compatible response 必须有 exactly-one choice、object message、response id、finish_reason；tool call 必须有非空 native id/name，arguments 必须是 JSON object，重复 ID 一律拒绝。
- Ollama response 必须 `done=true`、object message、created_at、done_reason；原生缺失 tool call ID 时只使用 `toolu_ollama_<ordinal>` 的协议特例，name/arguments/function 仍严格验证，重复 supplied/generated ID 拒绝。
- 请求侧 tool definitions、tool result call ID/name 和 message content 不再以 `tool`/`shell`/`{}`/`toolu_unknown` 等默认值静默修复；坏输入在请求/dispatch 前返回结构化 ProviderError/ModelError。
- `valid_empty_object_arguments` 保留 `{}`，不把合法空对象误判为缺参；解析失败不会生成 capability request。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `malformed_provider_tool_arguments_never_dispatch` | 非法工具 arguments 不产生 ModelToolCall/dispatch |
| `missing_native_tool_identity_is_rejected` | OpenAI 缺原生 call ID fail-closed |
| `duplicate_tool_id_with_different_payload_is_rejected` | 相同 ID 不论 payload 是否不同都拒绝 |
| `valid_empty_object_arguments_are_preserved` | 合法 `{}` 原样保留 |
| `provider_tool_result_round_trip_keeps_identity` | tool result 的 call ID 不被默认值替换 |

`.github/workflows/provider-strict-response.yml` 在 GitHub runner 执行 kiana-services lib fixtures、workspace static compile gate 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 4. 限制与交接

- `kiana-services` 是兼容 fixture/provider surface，生产主路径仍是 `DaemonHost → ProviderGateway`；本步不把 legacy parser 变成第二执行 spine。
- Parser 严格性不等于 HTTP/TLS/auth/credential/route/streaming/usage/durability 或 ProviderReceipt 正确；P4-J7-06+ 和 H06/H07/H08 继续处理。
- Ollama 无原生 ID 的 ordinal 仅是本次 response 的 correlation，不是跨 retry/run 的 stable InvocationId；attempt identity 由 H02/CP-02/后续 P4 卡绑定。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
