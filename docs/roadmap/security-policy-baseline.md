# SC-05 PolicyBundle、DecisionTrace 与 revision 基线

> 快照日期：2026-09-17。本页记录 `kiana-policy` 的纯 source contract 与 GitHub CI-only fixtures；
> 不把策略 DTO 或 CI 运行结果写成整体授权 enforcement。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-05`](security-compliance.md#step-sc-05) |
| feature_status | `implemented`（PolicyBundle/Revision/DecisionTrace、deny-first evaluator、compatibility adapter） |
| proof_level | `source`；本地不运行测试，CI 负责运行 fixtures |
| authority | ControlPlane 仍是唯一授权/生命周期事实源；Bundle 只提供纯策略输入和解释 |
| this step does | strict versioned rule bundle、default deny、revision/authority snapshot check、input/context digest、deterministic matched rule trace、stable reason code |
| this step does not | 不执行 Broker/handler、不消费 approval、不读取网络/文件/secret、不替换现有 ControlPlane loop 或宣称 durable/live |

## 1. Contract

`kiana-policy/src/security.rs` 新增：

- `PolicyRule`：精确 operation/capability/risk selector、priority、Deny/Ask/Allow effect 和
  必要 `SecurityReasonCode`；重复 selector/id、Allow 携带 reason、Deny/Ask 缺 reason 失败。
- `PolicyBundle`：schema/version、`SecurityPolicyId`、revision、authority epoch、规则摘要、
  `default_effect`。默认只能 Deny，unknown operation 不走隐式 allow；规则先执行现有
  `hard_policy_denial`，再按 Deny→Ask→Allow 的确定性顺序选择。
- `PolicyRevision`：独立的 revision/authority/policy digest 记录，可 strict serde round-trip。
- `DecisionTrace`：`SecurityDecisionId`、policy revision/epoch、request/input/context digest、
  matched rule IDs、outcome/reason 和 trace digest；不保存原始参数/提示/provider 错误。
- `BundlePolicyEngine`：实现已有 `PolicyEngine` trait 的兼容适配器，异常只返回稳定
  `POLICY_BUNDLE_INVALID` Deny，不创建第二执行循环。

`kiana-domain/src/contracts.rs` 登记 `kiana.policy-bundle.v1`、`kiana.policy-revision.v1` 和
`kiana.policy-decision-trace.v1`，owner 明确为 `kiana-policy`；SC-03 reason code 继续提供
稳定分类。`evaluate_with_snapshot` 对 authority epoch/policy digest mismatch 只生成 Deny
trace，禁止回退旧/默认策略。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `policy_bundle_is_versioned_digest_bound_and_default_deny` | strict schema/digest/revision、round-trip 和 default-Allow 禁止 |
| `policy_evaluator_is_deny_first_and_trace_is_replayable` | 显式 Deny 胜过 Allow，Ask/unknown outcome 与 trace serde 稳定 |
| `stale_snapshot_and_invalid_policy_never_fall_back_to_allow` | epoch/revision drift 与 unknown major 均 Deny，不回退放行 |
| `policy_rules_reject_ambiguity_and_unknown_fields` | duplicate selector、effect/reason 组合和未知字段 fail-closed |
| `policy_bundle_contract_stays_pure_and_consumes_existing_policy_trait` | source guard 固定 pure policy、既有 trait adapter、无 Broker/IO/执行依赖 |

## 3. Proof ceiling and handoff

SC-05 的证明上限是 `source`：Bundle/Trace 可解析且拒绝优先的源码合同已固定，但尚未把每个
Daemon/CLI/Web/Workbench/Runner 请求统一携带 context digest，也未实现持久 PolicyStore、
revision CAS、完整 Grant/Approval/DataBoundary 交集或跨进程恢复。SC-06+ 负责 authenticated
Principal/session 与 policy refresh/fence；后续 core 接线必须继续复用唯一 `PolicyEngine`→Gate→
Approval→Broker 脊柱，不能由 UI、模型或 Bundle 自己授予能力。
