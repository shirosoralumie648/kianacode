# ER-14 Effect Receipt 与 provider receipt 基线

> 快照日期：2026-09-17。本页记录外部 effect observation 的 source/CI 边界；本仓库当前
> connector 仍是 local_fixture，不能将本地 receipt 外推为真实 provider 成功或 exactly-once。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-14`](event-receipt-recovery.md#step-er-14) |
| feature_status | `implemented`（strict EffectObservation + connector invoke/reconcile integration） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane committed result + provider receipt/effect observation；observation 不授予 permit |
| this step does | execution/invocation/attempt binding、owner/audience/idempotency digest、provider receipt/query/evidence refs、observed time、ConfirmedSuccess/Failure/NoEffect/Unknown states、connector exact reconcile binding |
| this step does not | 不执行 HTTP/远端 transport，不把 timeout/Unknown 视为 no-effect，不允许 non-idempotent retry，不声称外部/live/physical effect proof |

## 1. Contract

`kiana-domain::EffectObservation` 是 strict digest-only DTO：ConfirmedSuccess/Failure 必须有
provider receipt，NoEffect 必须有 query digest，Unknown 永不自动升级；owner/audience、
idempotency key、execution/invocation/attempt 和 evidence refs 均绑定并校验，unknown fields/
digest/ID/time/receipt identity 失败即拒绝。

`kiana-daemon::ConnectorRegistry` 在 local fixture invoke/reconcile 结果中附带 observation；
reconcile 只接受原 connector/binding/account/operation/idempotency/final-payload digest 的
Unknown receipt，并在同一 connector EventLog 追加 observation。未找到 query/idempotency 的
真实 connector 仍应返回 Unknown，不能通过此 DTO 绕过 ControlPlane。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `effect_observation_binds_provider_receipt_and_scope` | provider receipt 映射 ConfirmedSuccess，owner/audience mismatch 拒绝 |
| `effect_observation_unknown_and_no_effect_never_claim_success` | Unknown 保持 Unknown；NoEffect 需 query；无 receipt 不可声称 success |
| `effect_observation_rejects_unknown_fields_and_bad_provider_receipt` | raw provider/unknown fields、空 idempotency、坏 receipt fail-closed |
| `er14_effect_observation_is_owner_bound_and_unknown_safe` | source guard 固定 connector observation、exact reconcile binding 与 no retry/no unknown-success boundary |

## 3. Proof ceiling and handoff

ER-14 proof ceiling 为 `source`：EffectObservation schema 与 local connector integration 已由
CI-only fixtures 固化；未运行本地测试。真实 provider query/receipt、账号/Secret lease、
external timeout/retry/reconcile、result delivery/handler stop 和 external/live/physical proof
仍留待 INT/BQ/CP-20/ER-15+。
