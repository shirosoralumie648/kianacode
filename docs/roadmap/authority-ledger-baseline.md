# CI-05 durable authority ledger baseline

> 快照日期：2026-09-16。本页记录 Membership/RoleAssignment/ProjectAssignment、PolicyProfile、DataBoundary、SharingGrant 与 EventStore 可重建 authority epoch；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CI-05`](../roadmap.md#step-ci-05) |
| feature_status | `implemented`（typed authority facts + pure EventStore reducer source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | committed authority stream → `AuthorityLedger::rebuild` → epoch/assignment/policy/data/share snapshot → ControlPlane revalidation |
| this step does | strict PolicyProfile/DataBoundary/SharingGrant contracts、epoch/revision/digest、event replay/version gap guard、跨项目 sharing operations intersection |
| this step does not | 不声称 JSONL/SQLite 已完成全量 durable identity projector、跨进程 revoke propagation、Grant/Approval/Cell 全维度消费或真实 tenant/RBAC |

## 2. Authority rules

- Membership、RoleAssignment、ProjectAssignment 都绑定主体/组织/项目窗口与 authority_epoch；Role/Project assignment 复用 CO-03 的 principal/role/window/digest 校验。
- PolicyProfile 只声明允许 operation 与 DataBoundary 引用；DataBoundary 固定项目集合/数据分类/外部分享开关；它们都不是 Broker permit。
- SharingGrant 只能在不同 source/target project 间授予有限 scope/operations、expiry 和同一 epoch；ledger 的 `sharing_operations` 只返回当前 epoch、未撤销且未过期 grants 的操作交集，反向/无 grant 为空。
- `AuthorityLedger::rebuild` 要求 authority stream version 连续，未知 kind、缺版本、duplicate、epoch rollback/stale fail-closed；纯 reducer 不执行副作用。
- Core authority epoch 读取通过 ledger reducer；每次 dispatch/continue/approval/effect 仍需独立 revalidation，snapshot/digest 不可自发授权。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `durable_authority_ledger_rebuilds_and_intersects_sharing_scope` | authority facts 从事件重建，跨项目 sharing operations 只返回有限交集，revoke 后立即为空 |
| `authority_ledger_rejects_unknown_or_noncontiguous_facts` | 未知 event kind、版本 gap/regression fail-closed |
| `authority_ledger_is_event_replayable_and_epoch_fenced` | domain reducer、core epoch/ledger helper 与 CO-03 assignment fence source guard 存在 |

`.github/workflows/ci05-authority-ledger.yml` 在 GitHub runner 执行 authority fixtures、core source guard、fmt 和 domain/core/protocol test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- `AuthorityLedger` 是 EventStore 事件的纯重建视图，当前没有把 assignment/policy/share 写入生产 authority stream；现有 local adapter 与 future durable projector 仍需 CI-06+、CP/PD/SC。
- SharingGrant scope/operation intersection 不替代完整 CapabilityGrant/ScopeSet/Policy/Gate/Approval 交集；跨项目无 grant、旧 epoch、过期 assignment 的 effect-time denial 由后续 CI-08/CP/CAP/SC 继续覆盖。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
