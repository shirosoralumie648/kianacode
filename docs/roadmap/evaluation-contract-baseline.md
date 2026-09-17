# EQ-01 legacy evaluation contract baseline

> 快照日期：2026-09-17。本页记录旧 `kiana eval run` 的兼容 contract fence；本地不运行测试，夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-01`](../roadmap.md#step-eq-01) |
| feature_status | `implemented`（extracted legacy schema/limits/field/error/fixture inventory） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | legacy `kiana-commands::EvalCommand` parser → compatibility constants/fixtures; new quality path remains separate and provider-independent |
| this step does | public schema/limit constants, JSON output field inventory, stable finding/error codes, compatibility test-name inventory and source fence |
| this step does not | 不迁移 CLI、实现 Quality DTO/EvalStore/TraceNormalizer/Judge/Promote、改变报告语义、调用模型/provider 或创建第二执行循环；EQ-02+ 负责 |

## 2. Compatibility rules

`kiana-commands/src/eval.rs` 现在公开并实际使用 `EVAL_SUITE_SCHEMA`、`EVAL_REPORT_SCHEMA`、`EVAL_BASELINE_SCHEMA` 以及 case/fixture limits。`LEGACY_EVAL_JSON_FIELDS` 列出 suite/report/baseline/case/metrics/finding 的现有字段，`LEGACY_EVAL_ERROR_CODES` 固定既有 finding/fixture 错误码，`LEGACY_EVAL_JSON_COMPATIBILITY_TESTS` 绑定现有测试名。删除或重命名字段必须先增加明确 upcast/migration 和对应 CI fixture；不允许只改 parser 让旧 JSON 静默丢字段。

这些常量只是 legacy compatibility boundary，不能把 caller-selected fixture path、报告 status 或 baseline finding 当作 ControlPlane authority。`kiana-core::eval` 的 committed-fact evaluator 不被复制或改成第二 evaluator；后续 domain quality contracts 需要通过显式 adapter 保持旧 CLI 输出字段。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `legacy_eval_v1_contract_is_pinned` | schema/limits、报告字段、finding/error code 和兼容 fixture 名单持续存在 |
| `legacy_eval_parser_uses_extracted_contract_constants` | parser 使用公开 constants，不再保留未登记 schema literal |

`.github/workflows/eq01-compatibility.yml` 在 GitHub runner 执行 compatibility fixture、core source guard、fmt 和 command/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 这是 source/compatibility fence，不是旧 eval 全量运行结果；本次不在本地执行现有 `eval_command` 测试或 release smoke。
- 字段 inventory 不证明跨版本 upcaster、durable EvalStore、artifact retention、privacy/scope isolation 或 report redaction；EQ-02–EQ-08+ 继续负责。
- 旧 parser 仍是兼容命令面；后续迁移必须保留 `kiana.eval-suite.v1`/report/baseline fields 或提供可审计 upcast，不能让报告直接修改 policy/Grant/Receipt/Outcome。
