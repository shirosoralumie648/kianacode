# EQ-04 evaluation admission metadata baseline

> 快照日期：2026-09-17。本页记录 dataset/case split/privacy/owner/expiry/sample/tag admission 约束；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-04`](../roadmap.md#step-eq-04) |
| feature_status | `implemented`（admission metadata and expiry/sample/tag validation） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | EvalDataset/EvalCase domain DTO → future controlled admission; no store/runner/provider path added |
| this step does | privacy class allow-list, owner, expiry, minimum sample, canonical workload tags and explicit `validate_for_admission(now)` |
| this step does not | 不实现 fixture loader/store/path containment、isolated workspace/clock、normalizer、experiment runner、judge、quality gate 或真实质量结论；EQ-08+ 负责 |

## 2. Admission rules

`EvalDataset` 现在绑定 `EvalSplit`（train/validation/regression/red_team/performance）、privacy class（public/internal/confidential/restricted/red_team）、non-empty owner/provenance、case refs、`minimum_sample` 和 canonical `workload_tags`；`EvalCase` 同样绑定 owner、privacy、expiry、minimum sample 和 tags。新字段带 serde defaults 以读取旧对象，但 admission 仍要求有效 owner/sample/privacy。

`validate_for_admission(now)` 先执行完整 digest/schema/strict validation，再拒绝 `now >= expires_at`；owner 缺失、未知 privacy、sample 超界、tag duplicate/order drift、secret marker 和 expiry 回退都 fail-closed。标签/引用在构造时排序，校验时要求相同 canonical 顺序；这些字段只描述评测范围，不授予文件/项目/模型权限。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `expired_or_unowned_dataset_is_not_admitted` | dataset 过期、owner 缺失、sample/tag policy 拒绝 |
| `case_admission_binds_owner_expiry_sample_and_workload_tags` | case admission 元数据 canonical 化及过期拒绝 |
| `privacy_and_tag_contracts_reject_unknown_or_noncanonical_values` | 未知 privacy、非 canonical tags 拒绝 |
| `quality_admission_metadata_is_explicit_and_denies_expiry_or_unowned_inputs` | domain source guard 无 owner bypass/Broker |

`.github/workflows/eq04-admission.yml` 在 GitHub runner 执行 domain admission fixtures、core source guard、fmt 和 domain/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- owner 是文本字段，尚未解析为 authenticated Principal/ProjectTrust；expiry 是对象校验，不是 durable lease/clock authority。
- workload tags/sample 不代表统计代表性或真实质量；dataset/case refs 尚未由 FixtureStore 做路径、size、privacy isolation，EQ-08/09/13+ 负责。
- 旧 core/command eval surfaces 仍可产生兼容报告；本步骤不改变 parser、EvalStore、normalization、Judge 或 Promote/Rollback。
