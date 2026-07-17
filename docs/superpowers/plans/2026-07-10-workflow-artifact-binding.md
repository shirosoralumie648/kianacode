# Workflow Artifact Binding Implementation Plan

> **Execution rule:** This project does not use TDD. The checked steps below are a completed evidence inventory, not a required execution order; any maintenance implements the approved contract first, then runs focused, adversarial, integration, and regression verification.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让所有经统一 workflow immutable-artifact commit 写入的事实产物自动进入已认证 EventLog，并在 CLI、strict audit 和 release 证据读取前复算内容，确定性阻断缺失、篡改和未绑定 orphan artifact。

**Architecture:** descriptor 由 `kiana-tasks::workflow` 的统一 commit 层生成，调用方不能传入或覆盖 `integrity_artifacts`。descriptor 作为 event data 的保留字段参与 WorkflowEvent v2 HMAC；读取时先验证 EventLog，再复算 descriptor 指向的文件。CLI 展示计数，strict audit 将 mismatch/orphan 转成 blocking finding，不在命令层重复 hash 或路径规范化逻辑。

**Tech Stack:** Rust 2021、Serde/serde_json、SHA-256、现有 `kiana-tasks` workflow runtime、`kiana-commands` strict audit、Cargo offline tests。

**Working-tree rule:** 当前仓库包含用户和前序未提交修改；本计划不得自动 commit、reset、clean 或覆盖无关改动。

---

## 文件边界

- Modify: `kiana-tasks/src/workflow.rs`
  - descriptor schema/type；
  - commit 时自动注入；
  - verify 时复算；
  - managed artifact orphan 扫描；
  - trust report 计数。
- Modify: `kiana-tasks/src/integrity.rs`
  - 增加稳定 `integrity_artifact_mismatch` 错误。
- Modify: `kiana-tasks/src/lib.rs`
  - 导出 descriptor/report 类型。
- Create: `kiana-tasks/tests/workflow_artifact_integrity.rs`
  - descriptor、reserved field、mutation、missing、orphan、idempotency 测试。
- Modify: `kiana-commands/src/tasks.rs`
  - `workflow integrity verify` human output 增加 artifact 计数。
- Modify: `kiana-commands/tests/workflow_integrity_command.rs`
  - JSON/human artifact 统计和 mismatch 失败。
- Modify: `kiana-commands/src/audit.rs`
  - strict audit 调用 trust verifier 并生成 blocking finding。
- Modify: `kiana-commands/tests/audit_command.rs`
  - artifact mismatch/orphan strict audit 回归。
- Modify: `docs/reference-feature-matrix.md`
  - 记录 Milestone 3 证据和未完成的 journal/release gate 边界。

---

## Task 1：Descriptor 数据契约与验证场景

- [x] **Step 1：创建 artifact integrity 验证文件**

在 `kiana-tasks/tests/workflow_artifact_integrity.rs` 建立独立临时根目录和 `KIANA_HOME` 隔离，避免创建真实用户 key。

测试通过 `initialize_local_hmac_key_at` 和 `initialize_workflow_run` 创建 signed workflow，再调用 `commit_immutable_artifacts_with_unique_event`。

- [x] **Step 2：增加自动 descriptor contract verification**

期望 event data 自动包含按 path 排序的：

```json
{
  "schema": "kiana.workflow-artifact-descriptor.v1",
  "path": "verification/vp-1.json",
  "size": 2,
  "sha256": "sha256:<digest>",
  "media_type": "application/json",
  "role": "verification_packet"
}
```

同一内容重试必须复用同一 event；descriptor 顺序和字段完全一致。

- [x] **Step 3：增加 reserved field adversarial verification**

调用方在 `event_data` 中提供 `integrity_artifacts` 时必须返回：

```text
workflow eventlog is invalid: integrity_artifacts is reserved
```

不得写 artifact，也不得追加 event。

- [x] **Step 4：运行实现后 contract verification**

```bash
cargo test -p kiana-tasks --test workflow_artifact_integrity --locked --offline --no-fail-fast
```

预期：descriptor 类型和自动注入实现后，验证通过；任何失败作为阻塞缺口处理。

---

## Task 2：统一 commit 层自动绑定

- [x] **Step 1：增加 descriptor 类型**

在 `kiana-tasks/src/workflow.rs` 增加：

```rust
pub const WORKFLOW_ARTIFACT_DESCRIPTOR_SCHEMA: &str =
    "kiana.workflow-artifact-descriptor.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowArtifactDescriptor {
    pub schema: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub media_type: String,
    pub role: String,
}
```

- [x] **Step 2：实现确定性 metadata 派生**

`media_type`：

```text
.json            application/json
.md              text/markdown
.yaml/.yml       application/yaml
.toml            application/toml
other            application/octet-stream
```

`role` 优先匹配：

```text
verification/** or **/verification.json     verification_packet
review/**                                   review_packet
**/result-packet.json                       result_packet
**/workpacket.json                          workpacket
**/packet.json                              packet
**/manifest.json                            manifest
fallback                                    workflow_artifact
```

- [x] **Step 3：从 resolved artifact 生成 descriptor**

使用规范化 `relative_path`、原始 bytes 长度和 `sha256_prefixed(contents)`；按 path 升序排序。

- [x] **Step 4：注入保留字段**

在 `commit_immutable_artifacts_with_unique_event` 中：

1. 验证 `event_data` 是 object；
2. 拒绝调用方已有 `integrity_artifacts`；
3. `preflight_artifacts`；
4. 生成 descriptors；
5. 注入 event data；
6. 用注入后的 data 做 existing-event idempotency 比较；
7. 写 artifacts；
8. append 已认证 event。

- [x] **Step 5：运行 Task 1 测试**

```bash
cargo test -p kiana-tasks --test workflow_artifact_integrity --locked --offline --no-fail-fast
```

预期：自动 descriptor、排序、reserved field、idempotency 全部通过。

---

## Task 3：Descriptor 复验与 Orphan 检测

- [x] **Step 1：增加 mutation/missing/orphan adversarial verification**

覆盖：

- descriptor 指向内容被修改；
- descriptor 指向文件被删除；
- signed workflow 的 managed artifact root 出现未被 descriptor 引用的文件；
- descriptor path 非规范化或逃逸；
- unsigned legacy workflow 不被夸大为 artifact verified。

- [x] **Step 2：增加稳定错误**

在 `IntegrityError` 增加：

```rust
#[error("integrity_artifact_mismatch: {0}")]
ArtifactMismatch(String),
```

- [x] **Step 3：实现 descriptor 解析与复算**

只接受 array；每项 schema/path/size/sha256/media_type/role 必须存在。路径使用现有 workflow-relative 规范化函数，拒绝 absolute、`..`、symlink escape 和重复 path。

读取文件后复算 size/SHA-256；任一不一致返回 `ArtifactMismatch(path)`。

- [x] **Step 4：实现 managed orphan 扫描**

扫描以下 workflow-relative roots：

```text
verification/
review/
workers/
swarm/
integrations/
results/
```

只统计 regular file；跳过 `*.tmp`、active lock/lease 和 recovery 辅助名。发现未被任何 authenticated descriptor 引用的文件时返回 `ArtifactMismatch("orphan:<path>")`。

- [x] **Step 5：扩展 trust report**

`WorkflowIntegrityReport` 增加：

```rust
pub artifact_descriptor_count: u64,
pub verified_artifact_count: u64,
pub orphan_artifact_count: u64,
```

`UnsignedLegacy` 和 `UnverifiableKeyMissing` 不声称 artifact verified；已验证链才执行内容复算和 orphan 扫描。

- [x] **Step 6：运行底层测试和 workflow 回归**

```bash
cargo test -p kiana-tasks --test workflow_artifact_integrity --locked --offline --no-fail-fast
cargo test -p kiana-tasks --test workflow_runtime --locked --offline --no-fail-fast
cargo test -p kiana-tasks --test workflow_integrity --locked --offline --no-fail-fast
```

---

## Task 4：CLI 与 Strict Audit 门禁

- [x] **Step 1：增加 CLI regression verification**

`workflow integrity verify --json` 必须输出 descriptor/verified/orphan 计数；修改已绑定文件后命令返回 `integrity_artifact_mismatch`。

human output 增加：

```text
artifact_descriptors: <n>
verified_artifacts: <n>
orphan_artifacts: <n>
```

- [x] **Step 2：增加 strict audit adversarial verification**

对已绑定 artifact 做内容 mutation，执行 strict audit，期望 blocking finding：

```text
category: workflow_integrity_artifact_mismatch
severity: block
next_action: restore_or_regenerate_authenticated_artifact
```

- [x] **Step 3：实现 CLI 输出**

命令层只展示 `WorkflowIntegrityReport` 字段，不读取 artifact、不复算 SHA。

- [x] **Step 4：实现 strict audit finding**

strict audit 在 board/evidence 审计前调用 `inspect_workflow_integrity`：

- artifact mismatch/orphan -> blocking finding；
- EventLog payload/auth/chain/downgrade -> blocking finding；
- key missing/unsigned 状态暂按现有 workflow profile 记录，不在本任务引入 release policy。

- [x] **Step 5：运行命令回归**

```bash
cargo test -p kiana-commands --test workflow_integrity_command --locked --offline --no-fail-fast
cargo test -p kiana-commands --test audit_command --locked --offline --no-fail-fast
cargo test -p kiana-commands workflow_ --lib --locked --offline --no-fail-fast
```

---

## Task 5：全量验证与证据

- [x] **Step 1：运行 crate 全量测试**

```bash
cargo test -p kiana-tasks --locked --offline --no-fail-fast
cargo test -p kiana-commands --locked --offline --no-fail-fast
```

- [x] **Step 2：运行 workspace 门禁**

```bash
cargo test --workspace --locked --offline --no-fail-fast
cargo build --workspace --locked --offline
cargo fmt --all --check
git diff --check
```

- [x] **Step 3：更新 feature matrix**

写明自动 descriptor、内容复验、orphan strict audit、测试命令和计数。继续保留：recovery journal revision MAC、archive verify、release blocker、Ed25519、KMS/TPM/PKCS#11、外部签名与客户验收。

- [x] **Step 4：范围检查**

```bash
git status --short
git diff --stat -- kiana-tasks/src/integrity.rs kiana-tasks/src/workflow.rs kiana-tasks/src/lib.rs kiana-tasks/tests/workflow_artifact_integrity.rs kiana-commands/src/tasks.rs kiana-commands/src/audit.rs kiana-commands/tests/workflow_integrity_command.rs kiana-commands/tests/audit_command.rs docs/reference-feature-matrix.md docs/superpowers/plans/2026-07-10-workflow-artifact-binding.md
```

不得自动提交、推送、合并或清理当前 dirty worktree。

## 完成条件

本计划只完成 trust-root Milestone 3。Recovery journal revision MAC、archive verification 和 commercial release blocker 仍属于 active goal，必须在后续切片继续实现。

## 实施结果（2026-07-10）

- Artifact Binding 的 24 个实施步骤全部完成。
- `workflow_artifact_integrity` 7 个测试通过，覆盖自动 descriptor、排序、保留字段、内容变更、删除、symlink、重复/非规范路径和 orphan。
- `workflow_integrity_command` 8 个测试与 `workflow_integrity_audit` 1 个测试通过。
- `kiana-tasks` 全量 73 个测试通过；swarm 集成套件 41 个测试通过。
- `cargo test --workspace --locked --offline --no-fail-fast`、`cargo build --workspace --locked --offline`、`cargo fmt --all --check` 和 `git diff --check` 全部通过。
- workspace 高负载暴露的 swarm 测试竞态已修复：测试现在轮询 terminal 状态，不再使用固定 300 ms 等待；生产阻断逻辑与严格 `ready` 断言均未放宽。
- 保留既有非阻断警告：`kiana-tools/src/agent.rs:5290` 的 `unused_mut`，本切片不修改该无关文件。
