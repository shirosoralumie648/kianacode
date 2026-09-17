# EQ-05 legacy evaluation adapter baseline

> 快照日期：2026-09-17。本页记录 legacy `kiana.eval-suite.v1` 到 typed quality DTO 的显式 adapter；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-05`](../roadmap.md#step-eq-05) |
| feature_status | `implemented`（read-only legacy→domain adapter with report compatibility） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | legacy EvalCommand parse → `kiana_domain::adapt_legacy_eval_suite` → existing read-only evaluator/report |
| this step does | deterministic typed ID mapping for legacy suite/case strings, strict legacy field/schema check, typed Dataset/Suite/Case validation and unchanged report schema/fields |
| this step does not | 不实现 EvalStore/FixtureStore、path isolation、TraceNormalizer/Judge/Experiment、Promote/Rollback、provider/model calls 或第二 evaluator/runner |

## 2. Adapter rules

`adapt_legacy_eval_suite` 只接受旧 `kiana.eval-suite.v1` object 的 `schema/id/description/cases` 字段，case 只接受 `id/kind/fixture/expect`，expect 字段使用明确 allow-list；unknown field/schema/empty IDs/fixture/kind fail-closed。旧字符串 suite/case IDs 通过 namespaced SHA-256 派生 stable UUID typed IDs，不打开 fixture path、不改原 JSON。

Adapter 生成 domain `EvalDataset`（legacy regression/internal provenance）、`EvalSuite`（legacy replay policies）和 `EvalCase`（fixture ref/legacy kind/expected final status），重新计算各自 digest；bundle 只作为只读 contract。`EvalCommand::run_suite` 在原有 parser validation 后调用 adapter，再继续旧 metrics/baseline evaluator，仍输出 `kiana.eval-report.v1` 原字段，因此没有第二质量执行循环或 promotion authority。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `legacy_eval_cli_round_trips_through_quality_dto` | 同一 legacy suite 得到稳定 typed suite/dataset/case IDs，旧 report schema/fields 保留 |
| `legacy_adapter_rejects_unknown_fields_without_changing_report_contract` | 未登记 legacy 字段拒绝，兼容 report contract 不变 |
| `legacy_eval_adapter_is_explicit_and_report_shape_remains_compatible` | commands→domain adapter/source guard，无 QualityGate/authority execution |

`.github/workflows/eq05-legacy-adapter.yml` 在 GitHub runner 执行 adapter fixture、core source guard、fmt 和 domain/commands/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Adapter 是 compatibility validation，不证明 legacy CLI 已经通过 DaemonHost/ControlPlane，也不把 caller path 变成 trusted FixtureStore。
- legacy `description` 只保留兼容解析，不写入 typed object；旧 report metrics/findings 仍由旧 evaluator 产生，EQ-06+ 负责 wire commands and richer quality facts。
- stable UUID 是 deterministic identity/fingerprint，不是 cryptographic signature；owner/privacy/dataset expiry 和 fixture access 仍需 authenticated admission/isolation/PD/SC 证据。
