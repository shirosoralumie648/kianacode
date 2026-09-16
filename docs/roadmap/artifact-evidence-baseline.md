# CO-06 immutable Artifact, Evidence and Criterion baseline

> 快照日期：2026-09-16。本文记录不可变工件版本、证据引用、标准引用和受限内容读取边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CO-06`](companyos.md#step-co-06) |
| source snapshot | `9946296`（CO-05 parent）加本步源码；最终 commit 记录在 git history |
| feature_status | `implemented`（typed artifact/evidence/criterion contracts + confined content boundary source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | confined workspace read / persisted blob → ArtifactVersion(content hash) → ArtifactRef/EvidenceRef/Criterion → CompanyProof typed metadata → existing EventLog CAS |
| this step does | 新增 ArtifactProvenance、ArtifactVersion/ArtifactRef、EvidenceRef、Criterion 和 stable EvidenceId/CriterionId；ArtifactContentPort 只按 `(artifact_id, version)` 读取并核对 hash；CompanyArtifact/CompanyProof/CriteriaSnapshot 增加 optional typed references，保留旧 text/event 字段以便迁移 |
| this step does not | 不把当前可变 workspace 文件当历史原件；不把路径、命令退出码或模型自述当 Evidence；不新增第二 EventStore/blob database；当前内存 blob adapter 仅为 fixture，尚无跨进程 durable store |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain contracts | `kiana-domain/src/artifact_contracts.rs`, `kiana-domain/src/company.rs`, `kiana-domain/src/ids.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `817ff058471e439153e4fa2c394d0870a12c0521764274f6cd5b481ddc2ac981`, `11354c01c309e29ae15dc39c0080b9fe68722c1b046fa9d67c7418ab89bf5d48`, `4bd57b83341681175d80f73b9717472bea1ab1746dbe0c0118387fced079689c`, `84137b792326d192c25c4b29625dbd4ee5d17b5714e9d4abae520d19c813327f`, `805a189446a020fad0e8feadee659442caa74b24518fd7b7331dd552dd82b42f` |
| Confined core path | `kiana-core/src/artifacts.rs`, `kiana-core/src/company.rs` | `6e098ed80e8cad1c5a8acf903a2a4bef622743346f1aa1852070f94def047138`, `6d484d1aff86aed98756b96862aa6c6541059865cdbba28fc51271e367852b34` |
| Read port | `kiana-ports/src/lib.rs` | `650dd3b5639f9173148875a0b0a6a41438c6f7d10658f9f3efeebaba444855f0` |
| Protocol/export | `kiana-protocol/src/lib.rs` | `855f675794c19869a6b28a6f8041a0f9d2e8b80bae9e27f96d56b10692960251` |
| Fixtures and CI | `kiana-domain/tests/co06_artifact.rs`, `kiana-ports/tests/co06_artifact_port.rs`, `kiana-core/tests/co06_artifact_guard.rs`, `kiana-core/tests/oa27_company_governance.rs`, `.github/workflows/co06-artifact-evidence.yml` | `b9879bdceffdc0fd77cb48cae572b1198905338f6d3e99fb048af23c842a03d2`, `1aa3739bf48ac1093ae9a218200e63e613d0bc55e3ac4fe3a91b911a40ca6c7e`, `4a74e3b86ceca05a6c42b117db6bfaefe25b6267c1384937c260609be1a8e9b7`, `8fbb40b17db65af178f08161606faace380a6b3412192921586eb65dd99ebc04`, `48e2059198f0360d90ceabca68dfb03bc03942634a38131333606a071b4ded6a` |

## 2. Immutable reference contract

`ArtifactVersion` 以 typed ArtifactId、单调 version、artifact schema、内容 sha256、大小、scope digest、producer/source provenance 和创建时间描述已保存的历史 blob，并强制 `immutable=true`。`ArtifactRef` 是不携带内容的可复核引用；两者都拒绝 unknown fields、越界大小、错误 digest、空 provenance 和 version=0。

`EvidenceRef` 绑定稳定 EvidenceId、run/invocation、evidence kind、ArtifactRef、scope digest 和 provenance，要求 evidence scope 与 artifact scope 相同。`ArtifactContentPort` 不接收 workspace path；实现必须在读取 `(artifact_id,version)` 后再次核对 content hash，缺 blob、scope mismatch 和 hash drift 均 fail-closed。`kiana-core::artifacts` 的文件读取继续使用既有 canonical/confined/O_NOFOLLOW/atomic helpers。

`Criterion` 使用独立 CriterionId、source ref/version、description、verification method、evidence kinds、required、scope 和 digest。digest 包含 CriterionId，因此相同文字的两个标准不会被文本去重吞掉；legacy `CriteriaSnapshot` 的字符串列表仍可 replay，新的 typed `criterion_refs` 为迁移入口。

## 3. Company integration

当 `RegisterArtifact` 或 Company business evidence 从受限文件读取成功时，core 保留旧 `CompanyArtifact.text` 快照，并在 artifact id 是 UUID 时生成/校验 `ArtifactVersion` metadata 写入 CompanyProof；历史 state 可继续读取旧字段。未来写入 durable blob 后，应通过 `ArtifactContentPort` 用 typed reference 重建原件，再独立报告当前 workspace 变化，不覆盖历史版本。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `artifact_version_reference_and_evidence_bind_immutable_content` | ArtifactVersion/Ref/EvidenceRef 保留 hash/provenance/scope，篡改引用不会伪造内容 |
| `criterion_ids_keep_same_text_distinct_and_scope_mismatch_fails` | 相同 criterion 文本因独立 ID 保持不同，Evidence scope mismatch/unknown fields 被拒 |
| `artifact_read_port_requires_persisted_hash_matching_blob` | 缺 blob、正确 blob、hash drift 分别返回 missing/success/conflict |
| `company_evidence_uses_immutable_artifact_and_read_port_boundaries` | CompanyProof typed fields、core confined read/atomic path 和只读 port 边界存在 |

`.github/workflows/co06-artifact-evidence.yml` 在 GitHub runner 执行 domain/ports fixtures、core source guard、fmt 和 domain/ports/core/daemon test-target compile；本地只做格式、静态编译和 diff 检查。

## 5. 限制与交接

- `InMemoryArtifactContentStore` 不是 durable blob store；尚无跨进程 cursor/recovery、retention/deletion、CAS 与 orphan-blob garbage collection。
- 现有 CompanyState/BusinessState 仍以 String artifact/event refs 和 text criteria 为兼容事实；typed refs 尚未覆盖全部业务命令、Review/Delivery/Acceptance projector 或历史 upcast。
- 文件当前内容与历史 artifact bytes 的双向 freshness/availability 查询要在 CO-07+ receipt/evidence 链继续接入；伪造退出码和外部结果仍需独立 adapter 证据。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
