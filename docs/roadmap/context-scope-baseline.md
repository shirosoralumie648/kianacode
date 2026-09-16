# CM-01 shared source and scope baseline

> 快照日期：2026-09-16。本文记录 CM-01 的 Context/Memory 来源、快照、用途和读取范围值对象；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-01`](context-memory.md#step-cm-01) |
| source snapshot | `d4df97d`（P4-J7-05 文档修订后的干净基线） |
| feature_status | `implemented`（domain SourceRef/SourceSnapshot/MemoryScope contracts） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | server identity/project → SourceRef/Snapshot + MemoryScope → Context/Memory adapter → bounded model content |
| this step does | 定义 versioned `SourceRef`/`SourceSnapshot`、`SourceKind`、`Freshness`、`EvidenceStatus` 和 server-owned `MemoryScope`，绑定 principal/project/session/collection/purpose，校验 digest/cursor/unknown fields |
| this step does not | 不扫描文件、不写索引/Memory、不推断用户身份、不把 collection/path/model arguments 当 principal、不宣称 durable source generation 或 semantic recall |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain shared source/scope | `kiana-domain/src/context_scope.rs`, `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs` | `8ac4dadc375ba536030e6df4a0458f0f2e125defe2c965ccd101f46f7e6d6f1d`, `802d68a61e0fd25193307d36a4380c18241ef1135f05484b8184c2c34479f05f`, `0415da636ec5683d78fb6083cff74884ccab3563b06ab97b586ddba5ed2fb65e` |
| Existing source consumers | `kiana-domain/src/memory.rs`, `kiana-domain/src/governance.rs`, `kiana-domain/src/prompts.rs` | `7e7330b09e582af7871b0bf6cdfb11a9c62001326bb2f21fb829cff05dadde2f`, `8d64488c287fe7fe8e2a67f9db2cafa685c50d0012600a6ac23fe4d372e1a777`, `ddcf19d5f449d11fac615a0e1ebff4c25900da183e359e9285a38e4f48bb69bd` |
| Fixtures/workflow | `kiana-domain/tests/cm01_sources.rs`, `.github/workflows/cm01-sources.yml` | `54ad1415b00601862d38c9fe09b99736199bbd7971d43e7fe26d8822987df811`, `000091a3df4bb9f41fc898380b6d5f97b66e2a0eb7b03cc00251386f95681698` |

hash 只用于 CM-01 源码漂移复核，不构成文件身份、索引 generation、Memory durable 或模型效果证明。

## 2. Shared source contract

`SourceRef` 记录稳定 `source_id`、`kind`、bounded locator、source revision、content digest、可选 EventLog cursor 与 `EvidenceStatus`。`SourceSnapshot` 在此基础上记录 freshness、observed time 和 snapshot digest；Verified snapshot 必须有 Verified source，缺失/篡改/非法 cursor 或 unknown field fail-closed。SourceKind 明确区分 workspace file、artifact、event、memory、prompt、user import、connector，避免只凭路径/字符串猜用途。

## 3. Server-owned MemoryScope

`MemoryScope` 绑定 `AuthenticatedPrincipalRef`、`ProjectIdentity`、`SessionId`、规范化 `MemoryCollection` 集合和已有 `Purpose`；集合必须非空、去重且可解析，scope digest 覆盖所有字段。`allows_collection` 只判断 server-provided collection 是否被 grant collection 覆盖；模型 arguments、collection 名称和 prompt 文本不会生成 principal、project 或写权限。`allow_write` 是范围请求的显式字段，不是自动授权。

SourceRef/Scope 是值对象，不执行 I/O。Context index、Memory JSONL、PromptBundle、Artifact adapter 后续通过转换函数消费这些值，并继续回到 ControlPlane/Capability Broker 做实时授权与 data epoch 检查。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `source_refs_roundtrip_and_reject_missing_identity` | SourceRef/Snapshot serde round-trip，缺 source identity、坏 digest/cursor、unknown field 均拒绝 |
| `scope_resolution_never_uses_model_principal` | MemoryScope 只接受 server principal/project，伪造模型主体破坏 digest 后 fail-closed |
| `prompt_and_memory_consumers_produce_source_refs_without_authority` | PromptSection/MemoryRecord 只转换为带 digest/provenance 的 SourceRef，不授予权限 |

`.github/workflows/cm01-sources.yml` 在 GitHub runner 执行 domain source fixtures 与 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- SourceRef locator/digest 是 bounded identity evidence，不等于已打开文件句柄、inode/dirfd containment、fresh read 或完整 offset map；P1-H-03/CAP-08+ 继续补。
- MemoryScope 尚未接入 core 的完整 processing grant/purpose/retention/revocation ledger；CM-02/03 负责 MemoryRecord 生命周期和 server-derived read/write scope。
- `Purpose` 当前复用 data-governance 值对象，跨用途/数据分类/外发审计仍需后续 DataPolicy/PD/SC 接线；semantic embedding/recall 未证明。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
