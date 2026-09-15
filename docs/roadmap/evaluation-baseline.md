# EQ-00 Evaluation / Quality source baseline

> 快照日期：2026-09-16。本文是 `EQ-00` 的 source-only inventory，不是质量平台、真实模型
> 质量、在线 drift、Promote、rollback、durable EvalStore 或 live provider 的完成声明。本轮不在
> 本地运行测试；`eval_baseline` 只由 GitHub Actions 执行。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-00`](../roadmap.md#step-eq-00) |
| source snapshot | `6d0b084`（NM-00 已推送的干净基线） |
| feature status | `partial`；存在两套局部 offline eval 代码，但没有统一 quality authority |
| proof ceiling | `source`；source guard/test-target 编译不提升 `local_behavior`；现有旧命令测试结果只可按其自身证据描述 |
| canonical fact source | Runtime/Company/EventLog/Receipt/Artifact 等事实及其受控 projections；eval report、GoldenTrace、baseline、score 和 UI 都是派生质量材料 |
| this step does | 固定旧/new eval 面、输入/输出/事实边界、缺口、迁移规则、fixture 命名和 EQ-01..51 handoff |
| this step does not | 不新增 Quality DTO、EvalStore、TraceNormalizer、Judge、Promote/Rollback、脚本 catalog、隔离 runner、第二模型循环或真实 provider 调用 |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Legacy offline eval command | `kiana-commands/src/eval.rs` | `3522b35762a64c45b525f79678061f57d1fb34150528121c4fff1aaf34edfc0d` |
| Legacy eval tests | `kiana-commands/tests/eval_command.rs`, `kiana-entrypoints/tests/cli_eval.rs` | `dffff99aa7d2830ec62a207967c59c190489b3d464374bbd250160041bbb133c`, `6f3dd40f6aab8f23b52e2914b6f82e225e22e9fb5c25df245add4b1828ca73b5` |
| Legacy CLI command dispatch | `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/command_dispatch.rs` | `6baaebf2ba9b9ca9aadebf6fe5fdd7923c6779ac8434ce4deacdc0d2663671d9`, `c78480c008785cfa509aabca24c640be1ee77468d333cf055e837ebb4e620dc5` |
| Provider-independent core eval | `kiana-core/src/eval.rs` | `dc2d6e06758e04343a9ee3a33865e0fdd988a7e86e97949ad536eaebc3d1df45` |
| Existing release smoke eval wiring | `scripts/release-smoke.sh` | `9ef54ad4863a64c41618c645110594d12a933976fa67c384f43b12eb41beb5de` |
| EQ-00 source guard/workflow | `kiana-core/tests/eval_baseline.rs`, `.github/workflows/eq00-baseline.yml` | `6c404ef205b74cd7d3e95e2636734187d52a9a708a483c22dd2cb7fa499e0edb`, `21cecc4e3313acb2569517075cd4a02032dabdb7574540d564e1916135f0dccc` |

Later EQ steps touching these files must refresh the corresponding row in the same commit. Hashes are
source anchors, not quality results or model/provider evidence.

## 3. Two current evaluation surfaces

### 3.1 Legacy `kiana-commands::EvalCommand`

`kiana-commands/src/eval.rs` implements a local `eval run --suite <path> [--baseline <path>]`
command. It parses `kiana.eval-suite.v1` JSON and bounded JSONL fixtures, computes event/tool/usage/
text metrics, checks optional `kiana.eval-baseline.v1` thresholds and emits `kiana.eval-report.v1`
with suite/fixture SHA-256 and findings. The CLI reaches it through the historical command registry
and `command_dispatch::execute_command`, not through a `DaemonHost` request or the CompanyOS/EventLog
control path. It is read-only fixture analysis and does not call a Model/Provider/Broker, but it still
reads caller-selected paths directly and has no server-derived identity, data scope, retention or
durable EvalStore.

Existing `kiana-commands/tests/eval_command.rs`, `kiana-entrypoints/tests/cli_eval.rs` and release
smoke checks provide useful local-behavior-style compatibility fixtures (schema, thresholds, bad
paths, symlink escape, malformed/duplicate cases), but this baseline does not rerun them. A passing
legacy report does not prove RuntimeEvent order, Capability safety, Receipt/Audit/Health consistency,
business Outcome, real provider quality, or promotion authority.

### 3.2 Provider-independent `kiana-core` eval (OA-23)

`kiana-core/src/eval.rs::evaluate_provider_independent` is a separate read-only reducer over committed
facts. It reuses normalized event kinds, OA-15 Audit, OA-10 Metric, OA-07 Span, Run/Receipt and OA-21
replay diagnostics; missing evidence, secret/forbidden effect, status/replay divergence, measured cost
or latency mismatch yields Fail/Blocked and `EvalSuiteReport.promote` is recomputed only when every case
passes. `ControlPlane::evaluate_provider_independent` reads EventLog and never invokes Model/Provider/
Broker or writes a promotion fact.

OA-23 therefore gives a stronger source-bound quality gate than the legacy command, but it is still an
in-process projection: no fixture store, isolated target runner, trace normalizer/diff artifact, judge,
quality finding registry, baseline/candidate ownership, feedback/drift, Promote/Rollback command,
durable EvalStore or real model quality evidence exists.

## 4. Input → evaluator → output matrix

| Surface | Input owner | Evaluator | Output | Current limitation |
|---|---|---|---|---|
| Legacy command | caller path to suite/baseline/JSONL fixture | `EvalCommand::run_suite` | `kiana.eval-report.v1` + optional findings | direct filesystem path, no DaemonHost identity/scope, no EventLog/Receipt authority |
| OA-23 core | committed RuntimeEvent slice | `evaluate_provider_independent` | `EvalSuiteReport`/case results with projection digests | no durable EvalStore, artifact archive, target isolation, semantic judge or external receipt |
| Release smoke | packaged fixture and release binary | shell/Python assertions around legacy command | smoke exit code/log | release smoke is a gate script, not a quality fact source or Promote authority |
| UI/entrypoint | CLI/Web/Workbench display state | no unified quality adapter | text/JSON/display card | transcript/UI/GoldenTrace cannot grant, close, deploy or change policy |

## 5. Missing platform and migration guard

The quality platform is currently missing:

- `EvalDataset`/`EvalSuite`/`EvalCase`/`GoldenTrace` versioned domain DTOs with owner, privacy class,
  expiry, workload and provenance;
- explicit FixtureStore/TraceSource/ArtifactReader/EvalStore/Judge/MetricsSink ports and a scrubbed,
  temporary `KIANA_HOME`/workspace target;
- one TraceNormalizer that validates cursor/sequence/terminal/correlation, canonicalizes declared
  volatile fields and redacts secrets without silently dropping evidence;
- deterministic runtime/safety/evidence/recovery/context/workflow/performance evaluators and stable
  finding codes, followed by baseline/candidate/QualityGate state and second ControlPlane authorization
  for Promote/Rollback;
- curated/deep/release CI lanes that archive report, JUnit, trace diff, reproduction command and
  evidence manifest with source/fixture/environment hashes.

Migration rule: the legacy command may remain compatibility-only. New quality work must either consume
the OA-23 provider-independent reducer through an explicit adapter or create a new typed quality
projection; it must not copy the legacy parser into a second evaluator or let a report/score alter
policy, Grant, Receipt, Acceptance, Delivery, Outcome or provider route. Any target that can touch
network, secret, MCP, payment, publish, desktop or workspace write must use a deny-by-default Broker
and the existing `DaemonHost → ControlPlane` spine. A missing/unknown/infra result is visible and
blocking, never treated as pass.

## 6. Fixture catalog and handoff

| Fixture | Purpose | Owner step |
|---|---|---|
| `eval_baseline` | source-only legacy/core split and missing platform guard | EQ-00 |
| `legacy_eval_v1_contract_is_pinned` | preserve old suite/report/baseline schema and fields | EQ-01/05 |
| `quality_ids_and_state_transitions_are_validated` | typed IDs/status/digest/unknown-field contract | EQ-02 |
| `fixture_path_escape_and_unknown_fields_fail_closed` | deterministic loader scope and size limits | EQ-08 |
| `eval_target_isolated_from_operator_home` | temporary workspace/home and cleanup | EQ-09 |
| `forbidden_capability_never_reaches_real_executor` | deny network/secret/MCP/payment/publish/write | EQ-11 |
| `eval_uses_the_same_daemonhost_spine` | no second runner loop or authority | EQ-12 |
| `event_flush_failure_never_returns_eval_pass` | committed source/flush/Unknown evidence | EQ-14 |
| `invalid_event_cursor_or_multiple_terminal_is_rejected` | normalizer and first divergence | EQ-17/21 |
| `safety_failure_blocks_even_with_good_final_text` | evaluator blocking precedence | EQ-27..42 |
| `passing_eval_cannot_promote_without_authority` | Promote/Rollback second authorization | EQ-43/44 |
| `cli_eval_commands_route_through_control_plane` | migrate CLI without legacy bypass | EQ-47 |
| `report_is_machine_readable_and_redacted` | report/JUnit/evidence/reproduction artifacts | EQ-48..51 |

All behavior fixtures run in GitHub Actions only. EQ-00 closes with an explicit partial/local_behavior
baseline and migration guard; it does not claim a quality platform, real model evaluation, durable
evidence, online drift or promotion capability.
