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
| `legacy_memory_is_unverifiable_until_reviewed`（daemon） | 真实 JSONL reader 使用 explicit import，不把旧行直接暴露给检索 |

`.github/workflows/cm02-memory-lifecycle.yml` 在 GitHub runner 执行 domain lifecycle fixtures、daemon legacy reader fixture 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 lifecycle validation 是值对象/reader gate，不是 Memory mutation 的原子提交点；CM-04/05 负责 CAS、幂等、EventStore/index visibility。
- purpose/retention/sensitivity 尚未由 processing grant/data policy 全量派生，用户私有跨项目隔离与删除传播仍是 CM-03+/PD/SC。
- Legacy import 在内存中转成 v2-compatible record 便于统一 reader，但不改写原 v1 文件或伪造历史 review；没有 successor 之前不可检索。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
