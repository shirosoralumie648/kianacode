# EQ-02 quality identity and lifecycle contract baseline

> 快照日期：2026-09-17。本页记录质量对象的 domain 基础合同；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-02`](../roadmap.md#step-eq-02) |
| feature_status | `implemented`（stable quality IDs, strict artifact/transition DTOs and status matrix） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | quality domain value objects → future quality ports/core; no evaluator/provider/Broker path added |
| this step does | domain-owned IDs for dataset/suite/case/golden/experiment/result/candidate/gate/feedback/drift objects, strict `QualityArtifact`/`QualityStateTransition`, digest/canonical bytes and deny-first status transitions |
| this step does not | 不定义完整 EvalDataset/Suite/Case/GoldenTrace 字段、fixture store、normalizer、judge、quality gate decision、promote/rollback 或第二执行循环；EQ-03+ 负责 |

## 2. Contract rules

新增 `QualityArtifactId`/`QualityTransitionId` 以及后续质量对象的稳定 UUID IDs，并登记到 domain ID contract round-trip matrix。`QualityArtifact` 绑定 object type、owner、source digest、revision/time 和 `QualityArtifactStatus`；`QualityStateTransition` 绑定 from/to、revision、reason 和 digest。两个 DTO 都 `deny_unknown_fields`，nil ID、未知 object、bad digest/owner/time、secret marker 和 digest drift fail-closed。

状态矩阵只允许 Draft→Admitted→Running→Completed→Passed/Failed/Blocked 等有界边；terminal 状态不能重开，重复/非法转移和时间回退拒绝。`canonical_quality_bytes` 复用 domain canonical journal bytes。对象存在不代表质量结论或 promotion authority；后续 gate 必须由 ControlPlane 重新授权。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `quality_ids_and_state_transitions_are_validated` | stable ID、合法状态链、非法 terminal/direct transition 和 digest validation |
| `quality_contract_rejects_unknown_fields_bad_digest_owner_and_revision_time` | unknown field、bad digest/owner、时间回退与 canonical round-trip 拒绝 |
| `quality_foundation_is_domain_owned_and_has_no_promotion_side_effect` | ID/schema/source ownership guard，quality domain 不依赖 Broker/Promote |

`.github/workflows/eq02-quality-contract.yml` 在 GitHub runner 执行 domain quality fixture、core source guard、fmt 和 domain/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 `QualityArtifact` 是通用生命周期基础，不等同于完整 EvalDataset/Suite/Case/GoldenTrace/EvalResult 等对象；EQ-03+ 会在此基础上添加字段和 upcast。
- digest 是完整性指纹而非签名，owner 仍是文本 contract；真实 principal/project privacy、durable store/CAS/replay、isolation、quality scoring 和 promotion approval 尚未证明。
- 状态 helper 不写 EventLog、不执行模型/provider/Broker；质量失败不能修改 Policy/Grant/Receipt/Acceptance/Outcome。
