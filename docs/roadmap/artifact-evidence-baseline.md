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

历史 CO-06 专属 workflow 曾在 GitHub runner 执行 domain/ports fixtures、core source guard、fmt 和 domain/ports/core/daemon test-target compile；现由统一 CI shards 覆盖，本地不运行测试。

## 5. 限制与交接

- `InMemoryArtifactContentStore` 不是 durable blob store；尚无跨进程 cursor/recovery、retention/deletion、CAS 与 orphan-blob garbage collection。
- 现有 CompanyState/BusinessState 仍以 String artifact/event refs 和 text criteria 为兼容事实；typed refs 尚未覆盖全部业务命令、Review/Delivery/Acceptance projector 或历史 upcast。
- 文件当前内容与历史 artifact bytes 的双向 freshness/availability 查询要在 CO-07+ receipt/evidence 链继续接入；伪造退出码和外部结果仍需独立 adapter 证据。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。

## 6. 2026-10-02 focused CI receipts and remaining exit conditions

GitHub [run 36677090825](https://github.com/shirosoralumie648/kianacode/actions/runs/36677090825)
在 `c221c211` 上通过 `co06_artifact` 两项、`co06_artifact_port` 一项和
`co06_artifact_guard` 一项。三组 fixture 到本轮 `0121cb5b` 未改；生产 artifact
合同未被本轮不相关的 credential/secret-schema/metric 修复改写。这四项只证明各自合同，
总 run 仍有其它失败。现在由统一 `ci.yml` 及 `scripts/ci/test-shards.json` 覆盖，
旧 CO-06 专属 workflow 仅供历史快照追溯，不是当前 CI lane。

完整 CO-06 仍为 🔄：在上述 `0121cb5b` 快照中，`ControlPlane::company_proof` 把工件文本快照和可选 typed metadata
写入 Company 事实；当时 `ArtifactStorePort` 只有内存 adapter，组合根尚未接入
独立持久 blob 的 stage/commit/read。§7 记录新增的本地 adapter；它仍未接入 Company 事实路径。
`artifact_version_remains_reviewable_after_workspace_file_changes`
和 `company_evidence_rejects_foreign_run_changed_content_and_missing_blob` 仍需完整产品链
fixture；原件/当前文件差异展示和 blob/事件写入间故障也必须保留为退出条件。
通过四个合同用例不会删除这些要求，也不会提升 durable/live 证明。

## 7. 2026-10-02 descriptor-pinned local store source slice

`kiana-daemon::LocalArtifactStore` now implements `ArtifactStorePort` and
`ArtifactContentPort` behind an explicitly supplied absolute root. It persists immutable blob,
manifest, stage marker and commit marker files under scope-digest/artifact-ID directories using
the descriptor-relative `LocalDir` helpers. Reads and commits validate the complete typed
reference, manifest digest, schema, provenance, content hash and size; missing commit/stage data,
drift, malformed files and non-regular files fail closed. The adapter does not infer project
paths, perform authorization, or claim physical erasure. Stage/commit join failures report
`result_unknown` because the worker may have completed a filesystem effect before its join result
was observed.

The GitHub-only fixture `kiana-daemon/tests/co06_local_artifact_store.rs` covers uncommitted
reads, malformed or drifted metadata, duplicate-version conflicts, missing/corrupt files,
symlink/directory/FIFO/hardlink denial, concurrent conflicts, reopen reads and isolation from
later workspace edits. These fixtures have not been observed on GitHub CI for this commit.

CO-06 remains 🔄 with `feature_status=partial` and `proof_level=source`. The adapter is now
available through the explicit `ControlPlane::with_artifact_store` composition hook. Company
artifact commands publish stage→commit only after policy and state-transition validation and before
the canonical EventLog fact append; immutable reads use `ArtifactContentPort` when a typed
historical reference is present. The default constructors retain the legacy text-only path until
the composition root supplies a store. There is still no cross-store transaction: an EventLog
failure can leave an unreferenced committed blob, so recovery/reconciliation remains open, as do
original-versus-current presentation, cross-process recovery, retention/deletion and power-loss
guarantees. The local fsync/link sequence is a source-level protocol only and does not establish
`durable`, `live` or `physical` proof.

## 8. 2026-10-02 Company ControlPlane wiring source slice

`ControlPlane::with_artifact_store` is an explicit optional dependency; no constructor silently
derives a project-local or hard-coded blob path. After the existing policy decision and
`CompanyState::transition` succeed, `persist_company_artifact` calls the port's retry-aware
`stage_artifact_version`, commits the returned persisted manifest, and only then serializes the
Company event. A denied transition therefore cannot publish through this path. Retries with the
same artifact reference, provenance and bytes return the first manifest (including its original
creation timestamp); changed hash or provenance is rejected. When a typed historical reference is
used by an immutable Company business command, the proof path re-reads bytes from the injected
port and rechecks the reference hash before comparing the recorded snapshot.

The CI-only guards cover deny ordering, explicit injection, historical port reads, first-timestamp
retry, and hash/provenance drift. They are source/contract evidence only; they do not prove a
cross-store atomic commit, crash recovery, orphan cleanup, or a live/durable deployment.

## 9. 2026-10-02 Company historical-byte integration fixture

`kiana-daemon/tests/co06_company_artifact_history.rs::artifact_version_remains_reviewable_after_workspace_file_changes`
constructs a `ControlPlane` with `with_artifact_store`, `LocalArtifactStore`, and the canonical
JSONL `EventStorePort`. It first sends a registration command for a missing source and asserts the
request is blocked without publishing a blob. It then registers a real file, reads the committed
Company fact from EventLog, changes the workspace file, closes both adapters, and reopens them. The
fixture takes the typed version from the reopened Company fact, reads that exact version from the
reopened artifact adapter, and checks that its bytes remain the original while the workspace file
contains the newer bytes.

The unified CI mapping keeps `kiana-daemon` as a whole-crate shard (`targets: null`), so the new
integration target is included without changing `scripts/ci/test-shards.json`. The isolated branch
has no CI receipt yet. CO-06 remains 🔄 with `feature_status=partial` and `proof_level=source`;
this fixture reads the blob port directly from the typed version recovered from EventLog and does
not execute the later Company business-command read-back branch. It does not establish cross-store
atomicity, product UI difference presentation, crash recovery, retention/deletion, or power-loss
durability.

The first CI compile of the Company integration fixture, run `37016768593` daemon job
`110870088862`, rejected its helper's named private `kiana_core::CoreResponse` return type. The
helper now returns the public `kiana_domain::CoreResponse` so failure diagnostics retain both
status and reason. A later CI run, `37019474036`, compiled the target but showed the successful
registration was still Blocked; source review found the fixture used Sponsor, while the current
RegisterArtifact policy allows Builder, Reviewer, Architect, PM and Closer. The fixture now uses
Builder and explicitly checks the missing-source failure reason before its success path. Neither
follow-up has a post-fix CI receipt yet.

Run `37025517103` daemon job `110899633464` then reached both requested CO-06 targets. The
duplicate-version store target passed. The Company history target failed before its deny/success
assertions with `Port(Failed("authority_snapshot_missing"))` at
`co06_company_artifact_history.rs:105`: it invoked ControlPlane directly without first creating
the authority stream required by `commit_protected_event`. The fixture now calls the public
`ControlPlane::synchronize_authority` with its trusted test context and a stable hashed fixture
configuration revision before the Company command; it does not synthesize an authority event.
It retains the exact missing-source denial and store/EventLog no-publication assertions. This
precondition correction has no post-fix CI receipt yet.

Run `37010476076` daemon job `110849027662` exposed two stale expectations in
`local_artifact_duplicate_version_cannot_replace_content_or_manifest`. The fixture now aligns
with the adapter contract: the legacy `stage_artifact` entry returns
`artifact_version_already_staged` whenever that version's manifest already exists, while
`stage_artifact_version` is the retry-aware API. The fixture now asserts that changed bytes and
manifest metadata are denied without changing the committed original; legacy exact duplicates also
remain rejected. Separately, the retry-aware API returns the first manifest including its original
timestamp. No production conflict classification changed and the corrected fixture has not yet
run on GitHub CI.
