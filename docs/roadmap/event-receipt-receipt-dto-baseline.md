# ER-11 Receipt DTO、redacted view 与 source cursor 基线

> 快照日期：2026-09-17。本页记录 Run/Execution receipt DTO 的 source/CI 边界；Receipt
> 是从 EventLog 与受控投影生成的只读视图，不是 handler/provider 或业务结果的现实证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-11`](event-receipt-recovery.md#step-er-11) |
| feature_status | `implemented`（strict RunReceipt/ExecutionReceipt + compatibility projection） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog facts + ControlPlane receipt projection；typed DTO 不可反向授权 |
| this step does | versioned Run/Execution receipt schemas、owner/project/scope digest、status/unknown、source cursor/event IDs、redaction profile、feature/proof level、result/receipt digest、legacy JSON compatibility |
| this step does not | 不保存 raw prompt/arguments/secret/output，不从 cache/模型自述创造成功，不证明 provider/external effect 或 exactly-once；cost/files/evidence aggregation 留待 ER-12 |

## 1. Contract

`kiana-domain::RunReceipt` 绑定 run/session/owner、project digest、ExecutionStatus、terminal
reason、bounded source cursor/event IDs、redaction profile、feature/proof level、result digest
和子 Execution receipt digests。`ExecutionReceipt` 绑定 request/execution/invocation/attempt、
action digest、typed execution state、effect_known、stop_confirmed、fenced、可选 result digest、
source IDs 和同一 proof metadata。unknown 未 fencing、success/effect_known 矛盾、owner/schema/
digest/unknown fields 均 fail-closed。

`receipt_from_events` 仍输出历史 `kiana.run-result.v1` 字段，另外附带 strict `run_receipt` 与
`execution_receipts`；它们从 persisted events、Invocation/Attempt reducer 和统一 redaction
profile 构造。typed projection 失败只产生明确的 result_unknown/error marker，不能把 compatibility
输出升级为成功事实。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `run_receipt_binds_owner_source_and_redaction_metadata` | RunReceipt strict fields、owner/project/source/redaction/proof/result digests 往返校验 |
| `execution_receipt_keeps_unknown_effect_fenced` | Unknown effect 必须 effect_known=false 且 fenced；success contradiction 拒绝 |
| `receipt_contracts_reject_unknown_fields_and_tampered_digest` | raw/unknown fields、digest/version tamper fail-closed |
| `er11_receipts_are_typed_redacted_and_source_bound` | source guard 固定 typed receipt integration、redaction、projection-error→unknown 与 no raw/authority boundary |

## 3. Proof ceiling and handoff

ER-11 proof ceiling 为 `source`：严格 receipt DTO 与兼容输出边界已由 CI-only fixtures 固化；
未运行本地测试。Cost/files/model/evidence aggregation、provider receipt、artifact failure、
multi-entrypoint parity、durable projection/restart 和 external/live/physical proof 留待 ER-12+。
