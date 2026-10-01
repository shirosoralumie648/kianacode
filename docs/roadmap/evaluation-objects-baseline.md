# EQ-03 evaluation object and GoldenTrace baseline

> 快照日期：2026-09-17。本页记录 EvalDataset/EvalSuite/EvalCase/GoldenTrace domain DTO；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-03`](../roadmap.md#step-eq-03) |
| feature_status | `implemented`（strict schema/version/provenance evaluation objects） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | quality domain DTOs → future EvalStore/quality core; no evaluator/provider/Broker path added |
| this step does | EvalDataset split/privacy/owner/provenance, EvalSuite target/policy/refs, EvalCase fixture/target/oracle fields, GoldenTrace source/cursor/normalized events/version/digests |
| this step does not | 不实现 case loader、fixture path isolation、TraceNormalizer/diff、Judge、EvalStore、experiment runner、quality scoring 或 Promote/Rollback；EQ-04+ / EQ-17+ 负责 |

## 2. Contract rules

`EvalDataset` 使用 `EvalDatasetId`、split、purpose、privacy/owner、canonical case refs、provenance 和 optional expiry；`EvalSuite` 绑定 dataset/cases、workload/target kind、evaluator/scoring/safety/budget refs、fixture schema、owner/status；`EvalCase` 绑定 suite、input/initial fixture refs、safe target config、expected events/state/artifacts/receipt assertions、forbidden effects、assertions/privacy；四类都严格 schema/version、nil ID、bounded text/list、digest 和 unknown-field 拒绝。

`GoldenTrace` 绑定 suite/case/source run/snapshot/input hash、target version map、event cursor range、normalized event values、artifact/receipt hashes、normalization version、optional human acceptance/score、expiry/provenance 和 digest。cursor 回退、source ID/target/version/hash 错误、未标记 secret、非有限 score、expiry 错误或 digest drift fail-closed。现有 registry 中旧 core `golden-trace.v1` owner/宽松定义已收敛为 domain strict contract；core capture payload 仍是后续 adapter 的兼容输入。

所有列表在构造时排序、校验时要求 canonical；`canonical_quality_bytes` 复用 domain canonical journal bytes。对象只描述质量材料，不授予 policy/Grant/approval/route 权限。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `eval_dataset_suite_case_and_golden_trace_bind_schema_and_provenance` | 四类对象 schema/version/owner/provenance/refs/cursor/digest/canonical round-trip |
| `quality_objects_reject_unknown_schema_fields_missing_provenance_and_secret_config` | unknown schema/field、缺 provenance、secret target config fail-closed |
| `golden_trace_rejects_cursor_and_expiry_regressions` | cursor/expiry 回退拒绝 |
| `quality_eval_objects_are_strict_and_only_value_contracts` | domain/protocol/source ownership guard，无 Tokio/Broker/second runner |

`.github/workflows/eq03-eval-objects.yml` 在 GitHub runner 执行 domain object fixtures、core source guard、fmt 和 domain/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Dataset/Suite/Case/GoldenTrace 尚未被 EvalStore 或 deterministic fixture loader 持久化/读取；fixture refs 是声明，不是路径访问授权。
- `GoldenTrace` normalized_events 只做 bounded safe-value contract，不执行 normalization/diff；volatile allow-list、cursor/checkpoint、artifact reader、source run capture 和 replay proof 留待 EQ-08/17–25、ER/PD。
- owner/privacy/provenance 是 domain fields，不等于 authenticated Principal/ProjectTrust 或 retention/deletion enforcement；Judge/QualityGate/Promote 仍不存在。

## 5. EQ-03 fixture identity correction (2026-10-02)

GitHub CI [run 36677090825, domain shard 2/4](https://github.com/shirosoralumie648/kianacode/actions/runs/36677090825/job/109764373648)
在 `c221c211c1c08b6796c4324bcf82d827dbbb0b5a` 上执行 `eq03_eval_objects`，结果为
2 passed / 1 failed。失败位于成功夹具的 `assert_eq!(case.case_id, case_id)`：dataset/suite
提前引用一个 `EvalCaseId::new()`，而 `EvalCase::new` 又分配了另一个 ID。夹具现在显式绑定
已声明的 case ID，再重新计算 `case_digest` 并执行既有 `validate`；原等值断言和其余断言全部保留。
这与 legacy adapter 预置稳定 ID 后重算摘要的既有做法一致，不改变生产对象的 ID 分配或验证规则。

```text
source_snapshot: 64ae786072a1aa6f491d1dea0ef5e863646c80bf + EQ-03 fixture correction
worktree_status: isolated feat/eq03-evaluation-guards worktree; only eq03_eval_objects fixture and this baseline changed
command_argv: gh run view 36677090825 --job 109764373648 --log-failed; git diff --check
cwd/environment: /tmp/kiana-eq03-step; Linux; local test/build/check/clippy/fmt/smoke commands not run
fixture or cassette: kiana-domain/tests/eq03_eval_objects.rs; already included in scripts/ci/test-shards.json kiana-domain-s2/4
exit_code: 0 for CI log retrieval and git diff --check; corrected fixture has not been executed
status_change: baseline fixture correction only; existing EQ-03 contract status unchanged
proof-level_change: source review only; no proof-level promotion
limitations: historical CI failure is evidence for the pre-fix fixture; the corrected fixture and regressions require GitHub CI after integration, whose result is not awaited; runtime, durable, live and physical behavior remain unproven by this slice
reviewer: EQ-03 implementation agent source review; no runtime test reviewer
```
