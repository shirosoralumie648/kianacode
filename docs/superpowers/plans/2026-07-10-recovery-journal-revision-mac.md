# Recovery Journal Revision MAC Implementation Plan

> **Execution rule:** This project does not use TDD. The checked steps below are a completed evidence inventory, not a required execution order; future maintenance implements the approved contract first, then runs focused, adversarial, integration, and regression verification.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 为 Bounded Swarm recovery journal 增加外部 HMAC revision chain、Workflow EventLog freshness anchor、immutable archive、strict audit/progress/release proof 门禁。

**Architecture:** 保留现有恢复状态机和 Linux 安全文件操作，把所有 journal 写入收敛到统一 commit helper。每次写入递增 revision、链接上一 signed record、用现有 workflow integrity key 重签，并追加唯一 revision event；archive 通过 immutable artifact commit 进入现有 artifact descriptor trust chain。

**Tech Stack:** Rust、Serde JSON、`ring::hmac`（通过 `kiana-tasks::LocalHmacKey`）、SHA-256、durable atomic write/fsync、WorkflowEvent v2、现有 Evidence Ledger / strict audit / release blocker 脚本。

---

## Task 1：Journal v2 数据契约与验证场景

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/src/tasks.rs`

- [x] **Step 1：新增 schema/revision/HMAC contract verification**

新增测试：

```rust
#[test]
fn recovery_journal_revision_mac_covers_mutable_state() { /* status/index/entry mutation must fail verify */ }

#[test]
fn recovery_journal_revision_chain_links_previous_signed_record() { /* revision N+1 points to hash(N) */ }

#[test]
fn recovery_journal_rejects_missing_key_before_write() { /* no journal and no tree mutation */ }

#[test]
fn recovery_journal_rejects_stripped_integrity_and_legacy_mutation() { /* downgrade */ }
```

- [x] **Step 2：记录实现后 focused verification command**

```bash
cargo test -p kiana-commands recovery_journal_revision_ --lib --locked --offline -- --nocapture
```

预期：缺少 v2 字段和签名 helper，测试失败。

- [x] **Step 3：升级数据结构**

在 `IntegrationRecoveryJournal` 增加：

```rust
revision: u64,
#[serde(default, skip_serializing_if = "Option::is_none")]
integrity: Option<IntegrityEnvelope>,
```

schema 升级为 `kiana.swarm-integration-recovery-journal.v2`，增加 domain/genesis 常量。

- [x] **Step 4：实现 payload 和 record hash helper**

新增：

```rust
fn integration_recovery_journal_payload(journal: &IntegrationRecoveryJournal) -> Result<Vec<u8>>;
fn integration_recovery_journal_record_sha256(journal: &IntegrationRecoveryJournal) -> Result<String>;
fn verify_integration_recovery_journal_mac(journal: &IntegrationRecoveryJournal, key: &LocalHmacKey) -> Result<()>;
```

payload 必须删除 `integrity`；record hash 必须覆盖完整 signed JSON。

- [x] **Step 5：实现统一签名 helper**

```rust
fn sign_integration_recovery_journal(
    journal: &mut IntegrationRecoveryJournal,
    key: &LocalHmacKey,
    revision: u64,
    previous_record_sha256: &str,
) -> Result<()>;
```

清除旧 envelope、设置 revision、签 payload、写回 `integrity`，并立即自验证。

- [x] **Step 6：运行 Task 1 测试**

```bash
cargo test -p kiana-commands recovery_journal_revision_ --lib --locked --offline --no-fail-fast
```

---

## Task 2：Revision Event Anchor 与 Crash Reconcile

**Files:**
- Modify: `kiana-tasks/src/workflow.rs`
- Modify: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/src/tasks.rs`

- [x] **Step 1：增加 replay/gap/unanchored-tail adversarial verification**

覆盖：

- journal revision 比 latest event 旧；
- journal 超前超过 1；
- 同 revision record hash 不同；
- revision 1 + genesis 无 event；
- latest+1 且 previous hash 正确；
- 已存在 event 的重复 reconcile。

- [x] **Step 2：新增 Workflow event kind**

在 `WorkflowEventKind` 增加：

```rust
SwarmIntegrationRecoveryRevisionCommitted,
SwarmIntegrationRecoveryArchived,
```

- [x] **Step 3：实现 revision anchor 读取**

新增：

```rust
struct RecoveryRevisionAnchor { revision: u64, record_sha256: String, previous_record_sha256: String }

fn latest_recovery_revision_anchor(
    artifact_dir: &Path,
    journal: &IntegrationRecoveryJournal,
) -> Result<Option<RecoveryRevisionAnchor>>;
```

必须读取已认证 Workflow EventLog，并按 integration/checkpoint 过滤最高 revision。

- [x] **Step 4：实现 idempotent revision event**

unique ID：

```text
<integration_id>:<checkpoint_sha256>:<revision>
```

event 必须记录 binding、revision、record/payload/previous hash、key_id、status 和 next index。

- [x] **Step 5：实现只允许单步 tail 的 reconcile**

```rust
enum RecoveryRevisionState {
    Anchored,
    RecoverableUnanchoredTail,
}
```

旧 revision、gap、hash mismatch 返回稳定错误；mutation 路径可为 recoverable tail 补写 event，纯 inspector 不写。

- [x] **Step 6：运行 anchor 测试**

```bash
cargo test -p kiana-commands recovery_journal_revision_anchor --lib --locked --offline --no-fail-fast
```

---

## Task 3：统一 Mutation Commit 与恢复状态机接入

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/tests/swarm_command.rs`

- [x] **Step 1：增加 key missing/tamper/replay integration verification**

新增 `swarm_command` 测试：

```rust
swarm_integrate_apply_blocks_recovery_when_integrity_key_is_missing
swarm_integrate_apply_blocks_tampered_recovery_journal_before_tree_mutation
swarm_integrate_apply_blocks_replayed_signed_recovery_revision
swarm_integrate_apply_reconciles_journal_written_before_revision_event
```

每个测试都保存 root 文件 before bytes，并断言阻断时未发生额外 mutation。

- [x] **Step 2：替换直接 write helper**

把：

```rust
write_integration_recovery_journal(path, &journal)
```

替换为：

```rust
commit_integration_recovery_journal_revision(artifact_dir, path, &mut journal)
```

该 helper 必须：加载 key、验证/补 anchor、计算下一 revision、签名、durable write、追加 event。

- [x] **Step 3：保证 mutation 顺序**

保持以下顺序：

1. commit `status=restoring`；
2. 每个 entry commit `entry.status=restoring`；
3. 执行 Linux path operation；
4. commit `entry.status=completed`；
5. 全部完成后 commit `status=completed`。

任何 commit 失败都必须在下一 path operation 前返回。

- [x] **Step 4：修复 error-path best effort 写入**

失败后的 `status=blocked` 也必须通过 signed commit；若 commit 自身失败，返回组合错误但不能覆盖原始恢复错误。

- [x] **Step 5：运行恢复测试**

```bash
cargo test -p kiana-commands recovery_journal_ --lib --locked --offline --no-fail-fast
cargo test -p kiana-commands --test swarm_command swarm_integrate_apply_ --locked --offline --no-fail-fast
```

---

## Task 4：Immutable Archive

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/src/tasks.rs`
- Test: `kiana-commands/tests/swarm_command.rs`

- [x] **Step 1：增加 archive regression verification**

覆盖：

- completed v2 journal 成功归档；
- archive event 和 descriptor 存在；
- active journal 在 commit 后删除；
- archive 已存在时幂等；
- archive mutation/deletion/replacement；
- legacy unsigned journal拒绝归档；
- archive commit 后、active delete 前重试。

- [x] **Step 2：修改 archive 函数签名**

```rust
fn archive_completed_integration_recovery_journal(
    artifact_dir: &Path,
    integration_dir: &Path,
) -> Result<()>;
```

调用点在 checkpoint 创建前传入 workflow artifact dir。

- [x] **Step 3：使用 immutable artifact commit**

构造 `WorkflowArtifactBatch` 写 archive JSON，event kind 为 `SwarmIntegrationRecoveryArchived`，unique field 为 archive ID。成功后删除 active journal并 fsync。

- [x] **Step 4：验证 existing archive**

若 archive/event 已存在，要求 revision、record hash、binding 和 descriptor 全部一致；不覆盖冲突内容。

- [x] **Step 5：运行 archive 测试**

```bash
cargo test -p kiana-commands recovery_journal_archiv --lib --locked --offline --no-fail-fast
cargo test -p kiana-commands --test swarm_command --locked --offline --no-fail-fast
```

---

## Task 5：Inspector、CLI、Strict Audit 与 Progress

**Files:**
- Modify: `kiana-commands/src/tasks.rs`
- Modify: `kiana-commands/src/audit.rs`
- Modify: `kiana-commands/src/report.rs` only if projection needs a typed field
- Test: `kiana-commands/tests/workflow_integrity_command.rs`
- Test: `kiana-commands/tests/workflow_integrity_audit.rs`
- Test: `kiana-commands/tests/report_command.rs`

- [x] **Step 1：增加 inspector/CLI regression verification**

`workflow integrity verify --json` 必须输出 `recovery_integrity`，human output 必须显示 active/archive verified counts；tamper、legacy、replay 和 unanchored tail 不得报告 verified。

- [x] **Step 2：实现只读 inspector**

```rust
pub(crate) fn inspect_swarm_recovery_integrity(
    artifact_dir: &Path,
) -> Result<SwarmRecoveryIntegrityReport>;
```

扫描 active journal 和 history archive，不写事件、不自动 reconcile。

- [x] **Step 3：接入 strict audit**

失败 finding 固定为：

```text
category: workflow_recovery_integrity
severity: block
next_action: resume_or_restore_authenticated_recovery_journal
```

- [x] **Step 4：验证 progress projection**

strict audit 持久化 finding 后运行 `report progress --json`，断言 blocker category/evidence/next_action 被投影且没有重复。

- [x] **Step 5：运行命令回归**

```bash
cargo test -p kiana-commands --test workflow_integrity_command --locked --offline --no-fail-fast
cargo test -p kiana-commands --test workflow_integrity_audit --locked --offline --no-fail-fast
cargo test -p kiana-commands --test report_command --locked --offline --no-fail-fast
```

---

## Task 6：Commercial Release Proof Gate

**Files:**
- Modify: `scripts/commercial-release-blockers-report.sh`
- Modify: `kiana-commands/src/release.rs`
- Modify: `docs/commercial-release-readiness.md`
- Modify: `docs/reference-feature-matrix.md`

- [x] **Step 1：增加 release blocker regression verification**

在 `kiana-commands/src/release.rs` fixture 中覆盖：

- proof missing -> blocking；
- invalid schema -> blocking；
- recovery status mismatch/legacy/unanchored -> blocking；
- verified proof -> satisfied；
- proof 包含 `secret_hex` -> blocking。

- [x] **Step 2：增加 proof contract**

默认路径：

```text
dist/proofs/workflow/recovery-integrity.json
```

检查 `workflow_integrity` 和 `recovery_integrity` 状态、计数及 secret 泄露。

- [x] **Step 3：增加 commercial check**

check ID：

```text
workflow.recovery-integrity
```

resolution scope 为 `final-artifact-derived`；required action 指向生成 workflow integrity proof 和 strict audit。

- [x] **Step 4：更新中文发布文档和 feature matrix**

明确本地 journal/archive trust gap 已关闭；外部签名、硬件密钥和客户验收仍开放。

- [x] **Step 5：运行 release 测试**

```bash
cargo test -p kiana-commands release_blockers --lib --locked --offline --no-fail-fast
bash scripts/commercial-release-blockers-report.sh --json
```

真实仓库输出允许因外部证据继续 `blocked`，但新增 workflow check 必须结构正确且不可伪造为 satisfied。

---

## Task 7：全量验证与范围检查

- [x] **Step 1：运行 focused gates**

```bash
cargo test -p kiana-commands recovery_journal_ --lib --locked --offline --no-fail-fast
cargo test -p kiana-commands --test swarm_command --locked --offline --no-fail-fast
cargo test -p kiana-commands --test workflow_integrity_command --locked --offline --no-fail-fast
cargo test -p kiana-commands --test workflow_integrity_audit --locked --offline --no-fail-fast
cargo test -p kiana-commands --test report_command --locked --offline --no-fail-fast
cargo test -p kiana-commands release_blockers --lib --locked --offline --no-fail-fast
```

- [x] **Step 2：运行 workspace gates**

```bash
cargo test --workspace --locked --offline --no-fail-fast
cargo build --workspace --locked --offline
cargo fmt --all --check
git diff --check
```

- [x] **Step 3：核对安全边界**

确认：

- 没有日志或 CLI 输出 `secret_hex`；
- key missing 在 tree mutation 前失败；
- strict audit 不自动修改 journal/event；
- release proof 不接受 legacy、tail 或 mismatch；
- archive 只通过 immutable commit 写入；
- 未触碰 `reference/**`；
- 未自动 commit/push/merge/clean。

- [x] **Step 4：更新计划实施结果**

记录实际测试数量、命令结果、已关闭和仍开放的商业化边界。

---

## 完成条件

本计划完成后，active recovery journal 和 completed archive 都必须脱离“项目目录内自证”模式：内容真实性由外部 key 验证，最新 revision 由认证 EventLog 锚定，archive 由 immutable artifact descriptor 绑定，strict audit/progress/release gate 都能阻断不可信状态。

## 当前实施结果

- Task 1–7 已完成。Focused gates 通过：14 个 recovery journal 单测、41 个 swarm 集成测试、8 个 integrity CLI 测试、2 个 workflow-integrity audit 测试、8 个 progress report 测试、9 个 artifact-integrity 测试和 2 个 release blocker 测试。
- `cargo test --workspace --locked --offline --no-fail-fast`、`cargo build --workspace --locked --offline`、`cargo fmt --all --check`、`git diff --check` 全部通过；仅保留既有 `kiana-tools/src/agent.rs:5290 unused_mut` 警告。
- `workflow integrity verify`、strict audit、progress report 和 commercial blocker 已共享同一恢复完整性事实源。
- 额外修复了两处测试发现的一致性缺口：归档事件指向的文件被删除时必须阻断；`recovery/history/*.json` 必须使用 immutable artifact descriptor，只有 active `journal.json` 可豁免 orphan 扫描。
- 当前商业报告会因缺少真实 release WorkflowRun 生成的 `dist/proofs/workflow/recovery-integrity.json` 而保持本地 blocker；不得使用 fixture 或伪造 proof 清除该项。
- 仍开放的边界包括 Ed25519、KMS/TPM/PKCS#11、外部签名、真实发布渠道、生产服务凭据和客户验收。
