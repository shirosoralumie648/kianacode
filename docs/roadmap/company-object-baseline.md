# P3-I-01 Company object contract baseline

> 快照日期：2026-09-16。本页记录 Objective → Incident 的十类 Company 业务对象、状态机和领域不变量；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P3-I-01`](../roadmap.md#step-p3-i-01) |
| feature_status | `implemented`（domain objects/state contracts + core boundary source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | versioned domain object → CompanyCommand/CompanyState transition → ControlPlane CAS/EventLog → projections/receipts |
| this step does | 十类对象结构、deny-unknown-field DTO、状态 enum/合法转移、版本/identity/measurement/evidence invariants 和 core domain boundary |
| this step does not | 不冻结九个 command/event wire pairs，不声称完整 CompanyOS replay/durable projector、业务结果或外部交付已完成；P3-I-02+、CO/ER/PD 负责 |

## 2. Object contract

十类对象为 Objective、Initiative、Project、Milestone、Acceptance、Delivery、Outcome、ChangeRequest、Risk、Incident。每类对象都有稳定 identity、版本或时间/证据边界；状态由 domain `states!` 宏生成的 transition contract 管理，非法边和 terminal 复活由 `DomainError` 拒绝。Acceptance/Review/MetricObservation 现在也拒绝 unknown fields，并分别校验 criteria snapshot、review key set、measurement finite/time/evidence。

`CompanyState::transition` 是唯一业务状态推进入口，先验证 authority/allowed role，再复制状态并调用 `apply`；ControlPlane 负责 command policy、read-set/CAS、EventLog 和 receipt，UI/transcript/prompt 不得直接修改对象。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `company_objects_expose_invariants` | 十类对象与 acceptance/review/measurement invariants 通过，Acceptance unknown field fail-closed |
| `company_objects_are_domain_contracts_with_explicit_invariants` | 所有对象/状态/validate 方法位于 domain，core 只通过 CompanyState/ControlPlane boundary 消费 |

`.github/workflows/p3-i01-company-objects.yml` 在 GitHub runner 执行 domain object fixtures、core source guard、fmt 和 domain/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 这十类对象仍由现有 CompanyState/EventStore adapter 承载，跨进程 durable projector、版本 upcast、command/event freeze 和全链恢复属于 P3-I-02/03、CO/ER/PD。
- 领域 `validate` 证明结构不变量，不证明现实业务 outcome、外部系统 receipt 或交付成功；ResultUnknown 继续要求对账。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
