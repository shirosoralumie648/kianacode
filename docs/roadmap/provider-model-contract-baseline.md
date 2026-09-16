# P4-J7-06 provider-neutral model contract baseline

> 快照日期：2026-09-16。本文记录 ModelContent/ModelCall/Attempt/Finish/Error/Usage、PreparedModelCall 和单一 ModelClient 端口；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P4-J7-06`](provider.md#step-p4-j7-06) |
| source snapshot | `b58dee2`（P0-A-02 parent；H04/H05 已完成的中立模型源码）加本步 CI evidence/fixtures；最终 commit 记录在 git history |
| feature_status | `implemented`（domain/ports neutral contract + provider/runner adapters source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | server `ModelAssignment` → ports `ModelClient::prepare_call/complete_admitted` → frozen PreparedModelCall/route/budget → ProviderGateway; Runner only consumes final ModelReply |
| this step does | 复核并 CI-wiring 现有 typed ModelContent/ProviderContinuation、ModelRequest/Output/Reply、ModelFinish/Outcome/Error/Usage、PreparedModelCall 和 ports ModelClient；legacy text/tool_calls 兼容桥与 conflicting-field fail-closed；domain/ports 不依赖 provider/runner |
| this step does not | 不新增 provider 直接执行工具或 Runner loop；不把 legacy cassette/文本 fallback 当权限；不把 ModelClient contract 直接暴露成公网 wire，具体 provider crate migration 和 protocol accumulator 留给 P4-J7-07+ |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain model contract | `kiana-domain/src/model.rs`, `kiana-domain/src/contracts.rs` | `bf19567fbc27e24b2d68b051fc91a0efd348d7e8b08179e0eeb62881822059b6`, `aa796d275675e113d0a5fa338664fae0d4234357ee833de19acb384e72370403` |
| Model port/re-export | `kiana-ports/src/model.rs`, `kiana-runner/src/model.rs` | `3677190f103b43962273061a0dc8f5a2099c9538baa6d14b4e8111f30d1582f6`, `458f46de8d8a320e1987ea7b5a47bd87faa053143898b843c5d1493c9275b7ff` |
| Provider codec | `kiana-provider/src/{lib,request,response}.rs` | `f8b04b4b2749dd872a88aaf2025ea2ca3b69894e861fb5e5c4c2d79424908ed`, `ec5a05bf3fd136bdbfbc4bab8351dc3a807c2f67ecacf072f89e7a01cab75471`, `06b416a096671fee2637b5c3c782a7e9042f21a23c7f27fd80b85db6a3dbbe66` |
| Fixtures and CI | `kiana-domain/tests/p4_j7_06_model_contract.rs`, `kiana-core/tests/p4_j7_06_model_contract_guard.rs`, `.github/workflows/p4-j7-06-model-contract.yml` | `fcad693fde3ed4971e6f9dec39f7f74996a48900023ddf7b3dd3c88aae10ff51`, `82b2338ab258cfe262c48834f0e14b7b3eee0c7493cea9234227f95036d99f01`, `43e68a87370a4a600fd5306f65e79c3d4d7393baaac7b9628e41f049f77150d2` |

## 2. Neutral contract

`ModelContent` 用有序 Text/ToolCall/ToolResult/AttachmentRef/ProviderOpaque block 表达 provider 中立内容；legacy `ModelMessage.text/tool_calls` 只在 content 为空时转换。非空 content 与 legacy text/tool_calls/tool_call_id 冲突会 `model_content_legacy_conflict`，避免两套事实源。Attachment/opaque/continuation 只保存 reference/digest，ProviderGateway 在最终 route 编译前验证 provider/protocol/route，不携带 raw bytes/header/body。

`ModelCallSpec` 绑定 call/attempt/step/turn/run、purpose、assignment、response format、deadline；`PreparedModelCall` 冻结 route/request/budget/tool catalog/request hash，只有 `complete_admitted` 能在 ModelBudgetPort permit 后进入 provider。`ModelFinish`/`ModelStopReason`/`ModelOutcome`/`ModelError` 保留终止、retry、phase、request_sent、side-effect state 和安全错误分类；未知/截断/refusal/不完整不假装完成。

`ModelClient` 在 `kiana-ports` 是唯一异步模型端口，`kiana-runner` 仅 re-export 并驱动状态机；ProviderGateway/codec 实现位于 `kiana-provider`，不能绕过 ControlPlane/permit 或直接执行工具。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `ordered_text_tool_and_result_blocks_round_trip` | 有序 content blocks 保持 Text/ToolCall identity 与顺序 |
| `conflicting_legacy_and_block_content_is_rejected` | legacy text 与 typed block 同时出现 fail-closed，不择一静默覆盖 |
| `model_contract_major_and_error_classification_fail_closed` | unknown major、structured ModelError/Outcome retry/side-effect classification 明确 |
| `provider_neutral_model_contract_stays_below_provider_and_runner_implementations` | domain/ports 不依赖 provider，runner re-export，provider codec 只消费 prepared contract |

`.github/workflows/p4-j7-06-model-contract.yml` 在 GitHub runner 执行 domain model fixtures、core source guard、fmt 和 domain/ports/runner/provider/core test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 provider-neutral contracts 与 ProviderGateway 已在源码接线，但全量协议编译、唯一 streaming accumulator、provider facade 迁移和实际 HTTP/live cross-provider 仍留给 P4-J7-07/12/14+。
- `ModelClient` legacy default 仍为兼容 adapter；只有 prepared/admitted path 才具备 provider request boundary，未配置 provider 的脚本/fake 不证明 live model。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
