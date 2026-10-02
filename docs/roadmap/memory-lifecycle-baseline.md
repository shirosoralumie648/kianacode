# CM-02 MemoryRecord lifecycle and legacy import baseline

> 快照日期：2026-09-16。本文记录 CM-02 的 MemoryRecord 生命周期/provenance 字段和 v1 legacy import；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-02`](context-memory.md#step-cm-02) |
| source snapshot | `817277a`（CM-01 来源/scope 提交后的干净基线） |
| feature_status | `implemented`（domain lifecycle + daemon reader/import source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | JSONL row → bounded decode → explicit `MemoryRecord::legacy_import`/lifecycle validation → searchable/review projection |
| this step does | 为 MemoryRecord 增加 purpose/sensitivity/validity/retention/dependencies/import mode，校验 admission/review/state/provenance 组合；v1 行显式导入为 origin Unknown/Candidate/Draft/unverifiable，不默认 approved/verified |
| this step does not | 不建立 mutation/transaction/index generation/processing-grant service，不把 Memory JSONL 文件存在当 durable 事实，不改变 EventLog authority |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain lifecycle contract | `kiana-domain/src/memory.rs` | `42ec42ab1671ce5eec354ef230899867502a92510876c1249a20ee1314f7bcda` |
| Daemon decode/review writer | `kiana-daemon/src/harness_memory.rs` | `d6e22474b4b7b0549cdf90624d9cefa3a721e06efea539c29417f059be02c036` |
| Fixtures/workflow | `kiana-domain/tests/cm02_memory.rs`, `.github/workflows/cm02-memory-lifecycle.yml` | `e2dd6cb59b6a50afb050206194f116655a5295157cfb1d804fcfba9a37286be1`, `b8999f54ec57b1a4e91ee46911f639b0916968af31c703909d81fd7a876b2770` |

hash 只用于 CM-02 源码漂移复核，不构成 Memory durable、审批或业务 Outcome 证明。

## 2. Lifecycle dimensions

`MemoryRecord` 现在区分：

- `kind`、`purpose`、`sensitivity`、`validity`、`retention`、`dependencies`；
- `origin`（Unknown/Model/Hook/Git/User）；
- `admission_state`（Candidate/Qualified/Ephemeral/Rejected）；
- `state`（Draft/Active/Rejected）；
- `import_mode`（Native/LegacyImport）。

合法组合固定为 Candidate→Draft、Qualified→Active、Ephemeral→Active、Rejected→Rejected。Qualified/Active 必须有非 Unknown origin、purpose、sensitivity、reviewer/time 和 evidence；LegacyImport 强制 Unknown origin、Candidate/Draft、无 review，且永不 `searchable` 或 `verified`。Validity/retention/dependency 各自有界校验，坏 revision/collection/layer 不进入投影。

## 3. Legacy import

Daemon JSONL reader 对 `kiana.memory-record.v1` 调用 `MemoryRecord::legacy_import`，不再把缺失 admission/state/classification 的旧行升级为 Qualified/Active。Legacy row 转成 v2-compatible in-memory projection，保留原 text/source 诊断，但 provenance 为 unverifiable、搜索不可见，必须经过新的 server review/approval 才能产生 Native Qualified successor。v2 native candidate/scratch/review writer 显式填充 purpose/sensitivity/import mode；旧文件仍只读兼容。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `legacy_memory_is_unverifiable_until_reviewed` | v1 legacy import 保持 Unknown/Candidate/Draft/unsearchable/unverified |
| `invalid_admission_state_combination_is_denied` | Candidate/Active、错误 validity 等组合 fail-closed |
| `qualified_memory_requires_review_evidence_and_purpose` | Qualified/Active 缺 provenance/review/evidence 不可接受 |
| `memory_record_rejects_unknown_nested_lifecycle_fields` | `Purpose`、`Retention`、`MemoryEvidence` 的嵌套未知字段以及 v1 legacy evidence 未知字段均 fail-closed |
| `qualified_memory_requires_review_evidence_and_purpose` malformed cases | nil event/request/run IDs, blank/oversize quotes, invalid Purpose, blank reviewer and zero review time fail closed |
| `legacy_memory_is_unverifiable_until_reviewed`（daemon） | 真实 JSONL reader 使用 explicit import，不把旧行直接暴露给检索 |
| `schema_v1_native_qualified_record_requires_explicit_legacy_import` | v1 Native Qualified/Active 不能绕过 importer；命名 upcaster 仍生成 v2 LegacyImport |
| `memory_event_projection_rejects_schema_v1_native_qualified_record` | committed memory fact 中嵌入的 v1 Native Qualified/Active 记录不能通过 lifecycle/projection |

以前的独立 `.github/workflows/cm02-memory-lifecycle.yml` 已删除并合并进 `.github/workflows/ci.yml`：`cm02_memory` 由 `kiana-domain-s1/4` shard 执行，daemon legacy reader fixture 随 `kiana-daemon` 包测试执行，fmt 由 Rust gates 执行。本地不运行测试或格式检查。

## 5. 限制与交接

- 当前 lifecycle validation 是值对象/reader gate，不是 Memory mutation 的原子提交点；CM-04/05 负责 CAS、幂等、EventStore/index visibility。
- purpose/retention/sensitivity 尚未由 processing grant/data policy 全量派生，用户私有跨项目隔离与删除传播仍是 CM-03+/PD/SC。
- Legacy import 在内存中转成 v2-compatible record 便于统一 reader，但不改写原 v1 文件或伪造历史 review；没有 successor 之前不可检索。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。

## 6. Qualified evidence and review-value validation (2026-10-02)

Source review found that Qualified/Active previously treated any present `Purpose`, nonempty
evidence vector, and present review fields as complete. Empty Purpose fields, evidence with nil
typed IDs or blank/oversize quotes, blank reviewer strings, and a zero review timestamp could
therefore pass lifecycle admission. The domain validator now validates Purpose values and
requires evidence IDs/quotes and review identity/time to be meaningful; CI-only negative cases
cover these inputs without changing the qualified valid control or the previous fixture-error fix.

```text
source_snapshot: `ae412092` plus fix `5c8a0831`, local merge `eae22402`, and the integrated fixture ownership correction
worktree_status: CM-02 lifecycle correction is merged into local master; only memory lifecycle code, CM-02 fixture, and this baseline changed; Qualified records now validate present Purpose values, evidence IDs/quotes, and nonblank reviewer/positive review time
command_argv: gh run view 36677090825 --job 109764373568 --log-failed; gh run view 36899317942 --json jobs; git diff --check
cwd/environment: /tmp/kiana-cm02-audit; Linux; local test/build/check/clippy/fmt/smoke commands not run
fixture or cassette: kiana-domain/tests/cm02_memory.rs::qualified_memory_requires_review_evidence_and_purpose; common CI maps it into kiana-domain-s1/4 and also runs the daemon package tests
exit_code: 0 for source review and branch/integration `git diff --check`; historical CI run 36677090825 showed the pre-correction broad-error assertion; current master run 36899317942 domain-s1 job was queued when inspected; fixtures added by this correction are not yet observed on GitHub
status_change: none; CM-02 remains 🔄 pending current GitHub CI evidence
proof-level_change: none; source only
limitations: new negative fixtures are unexecuted locally and current remote domain-s1 evidence is pending; exact event-to-quote/source binding remains CM-14/CM-25+; dependency invalidation and data epochs remain CM-06; normalization/sensitive-text handling remains CM-09; no semantic recall or durable-memory claim is made
reviewer: CM-02 implementation agent source review; no runtime test reviewer
```

## 7. Nested lifecycle payload strictness audit (2026-10-02)

Source review found that `MemoryRecord` rejected unknown top-level fields while its nested
`Purpose`, `Retention`, and `MemoryEvidence` values silently ignored unknown members. Those
three value types now reject unknown fields during deserialization. Serialization is unchanged,
and the daemon reader/upcaster continue to route v1 rows through explicit legacy import. The CI
fixture accepts known nested fields in both v2 records and v1 legacy evidence, and rejects unknown
nested keys in both versions.

```text
source_snapshot: `03b6e443` plus this isolated source/test/doc change
worktree_status: CM-02 nested payload decode is strict in `Purpose`, `Retention`, and `MemoryEvidence`; native constructors and serialization shape are unchanged; daemon JSONL reads and named upcasts route v1 through explicit legacy import; new negative fixture awaits GitHub CI after integration
command_argv:
  `rg -n "deny_unknown_fields|memory_record_rejects_unknown_nested_lifecycle_fields" kiana-domain/src/governance.rs kiana-domain/src/memory_proposals.rs kiana-domain/tests/cm02_memory.rs`
  `git diff --check`
  `gh run view 36926015057 --job 110583767786 --log`
cwd/environment: `/tmp/kiana-cm02-audit-20261002`; read-only GitHub CLI for receipts; local Cargo test/build/check/fmt/clippy/smoke commands not run
fixture or cassette: `kiana-domain/tests/cm02_memory.rs::memory_record_rejects_unknown_nested_lifecycle_fields`; historical run `36926015057` logged all three existing CM-02 fixtures passing, while the domain shard failed in unrelated `aut07_workflow_queue_claim::queue_claim_rejects_cycle_duplicate_and_parallel_overflow`; run `36958156294` predates this fixture and its domain shard failed, so it is not evidence for this change
exit_code: historical CM-02 fixture cases 3/3 passed in GitHub CI; `git diff --check` exit 0; new negative fixture has no CI result yet
status_change: none; CM-02 remains 🔄 and this change does not claim the roadmap step complete
proof-level_change: none; source only
limitations: the new negative fixture is unexecuted pending integration; `validate_lifecycle` still permits a fully populated schema-v1 Native object constructed directly in memory, although the daemon JSONL reader and named upcaster explicitly route v1 payloads through legacy import; nested strictness does not bind evidence quotes to immutable EventLog facts or establish durable approval; no local tests or builds were run
reviewer: CM-02 isolated source review; no runtime test reviewer for the new fixture
```

## 8. Schema-v1 lifecycle bypass correction (2026-10-02)

Source review found that a fully populated schema-v1 Native Qualified/Active `MemoryRecord` could
pass lifecycle validation and the EventLog memory projection, bypassing the explicit legacy import
contract. Lifecycle validation now accepts only v2 records; v1 records return
`memory_record_legacy_import_required`, and visibility denies every non-v2 record even when called
without validation. `legacy_import` now accepts only v1 payloads. The named storage upcaster leaves
the source schema intact while validating its field boundary, then delegates the conversion to that
explicit importer.

```text
source_snapshot: source commit `84c1c95ea2e791d927a4244185a88114db537574` based on `f28f1b5b`, integrated locally as `9ac1918f`
worktree_status: schema-v1 Native records fail lifecycle validation and visibility; EventLog projection inherits the same denial; explicit JSONL import and named storage upcast still produce v2 LegacyImport records; roadmap remains partial/source pending GitHub CI
command_argv: source review of MemoryRecord lifecycle/import/visibility, storage upcast, and CM-02/CM-05 fixtures; `git diff` manual review; cherry-pick `84c1c95e`; push pending; no Cargo tests/build/check/fmt/clippy/smoke
cwd/environment: source review in `/tmp/kiana-cm02-v1-lifecycle-20261002`; integration in repository root; GitHub Actions is the only test executor
fixture or cassette: `schema_v1_native_qualified_record_requires_explicit_legacy_import`; `memory_event_projection_rejects_schema_v1_native_qualified_record`; `legacy_memory_is_unverifiable_until_reviewed` includes explicit upcaster control
exit_code: source commit and cherry-pick succeeded; no local tests, build, check, format, clippy, smoke, or diff-check command run; push and GitHub CI pending
status_change: CM-02 remains 🔄; v1 Native Qualified/Active records can no longer become searchable through direct lifecycle or EventLog projection paths, while explicit v1 import/upcast remains available
proof-level_change: `feature_status=partial`; `proof_level=source`; no runtime/durable/live/physical promotion
limitations: fixtures await GitHub CI; event-to-quote/source binding remains CM-14/CM-25+; dependency invalidation remains CM-06; purpose/sensitivity derivation and retention policy remain later scope; no semantic recall or durable-memory claim is made
reviewer: isolated source review of schema gates, import compatibility, lifecycle visibility, and EventLog projection; no runtime test reviewer
```
