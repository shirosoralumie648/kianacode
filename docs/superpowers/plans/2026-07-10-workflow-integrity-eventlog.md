# Workflow Integrity EventLog Implementation Plan

> **Execution rule:** This project does not use TDD. The checked steps below are a completed evidence inventory, not a required execution order; future maintenance implements the approved contract first, then runs focused, adversarial, integration, and regression verification.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Kiana WorkflowRun 增加项目目录外的 HMAC trust root、可认证 EventLog、legacy genesis seal 和稳定 CLI 验证接口。

**Architecture:** 新建 `kiana-tasks::integrity` 负责密钥存储、HMAC、canonical payload 和 trust report；`workflow.rs` 只负责在 writer lease 内构造、签名和验证事件。旧事件保持可读，但一旦进入认证链，后续 unsigned event 被视为 downgrade。CLI 仅暴露 key metadata 与 trust status，绝不输出 secret。

**Tech Stack:** Rust 2021、`ring::hmac`、`rand::rngs::OsRng`、`sha2`、`serde_json`、现有 workflow writer lease 与原子文件写模式。

**安全边界：** 当前工作树包含大量未提交修改。本计划不得执行 `git reset`、`git clean`、revert、自动 commit、push 或 merge；只修改列出的文件。

---

## 文件结构

- Create: `kiana-tasks/src/integrity.rs`：外部 key path、key schema、权限检查、HMAC signer/verifier、generic envelope。
- Modify: `kiana-tasks/src/lib.rs`：导出 integrity API。
- Modify: `kiana-tasks/Cargo.toml`：增加直接 `ring = "0.17"` 依赖。
- Modify: `kiana-tasks/src/workflow.rs`：WorkflowEvent v2、认证 append/read、legacy 状态、genesis seal。
- Modify: `kiana-tasks/tests/workflow_runtime.rs`：使用隔离 `KIANA_HOME` 的 workflow integrity 集成测试。
- Modify: `kiana-commands/src/tasks.rs`：`workflow integrity init|status|verify|seal`。
- Create: `kiana-commands/tests/workflow_integrity_command.rs`：CLI 合同和 redaction 测试。
- Modify: `docs/reference-feature-matrix.md`：记录真实实现状态和剩余缺口。

## Task 1：密钥存储与 HMAC 原语

- [x] **Step 1：记录 key/HMAC verification cases**

在 `kiana-tasks/src/integrity.rs` 的测试模块覆盖以下行为：

```rust
#[test]
fn integrity_key_init_is_idempotent_and_owner_only() {
    let root = temp_root();
    let path = root.join("trust/workflow-integrity-key.json");
    let first = initialize_local_hmac_key_at(&path).unwrap();
    let second = initialize_local_hmac_key_at(&path).unwrap();
    assert_eq!(first.key_id, second.key_id);
    assert_eq!(first.algorithm, LOCAL_HMAC_ALGORITHM);
    #[cfg(unix)]
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn integrity_key_loader_rejects_key_id_mismatch() {
    let root = temp_root();
    let path = root.join("trust/workflow-integrity-key.json");
    initialize_local_hmac_key_at(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["key_id"] = serde_json::json!("sha256:forged");
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(matches!(load_local_hmac_key_at(&path), Err(IntegrityError::KeyIdMismatch)));
}

#[test]
fn hmac_envelope_detects_payload_mutation() {
    let key = LocalHmacKey::from_secret_for_test([7u8; 32]);
    let envelope = key.sign_payload("workflow-event", b"payload", "genesis").unwrap();
    assert!(key.verify_payload("workflow-event", b"payload", &envelope).is_ok());
    assert!(matches!(
        key.verify_payload("workflow-event", b"changed", &envelope),
        Err(IntegrityError::PayloadMismatch)
    ));
}
```

- [x] **Step 2：记录实现后 focused verification command**

Run:

```bash
cargo test -p kiana-tasks integrity::tests --lib --locked --offline --no-fail-fast
```

Expected: integrity 模块和 API 实现后 PASS；任何失败作为阻塞验证缺口处理。

- [x] **Step 3：增加依赖和公共类型**

`kiana-tasks/Cargo.toml` 增加：

```toml
ring = "0.17"
```

`integrity.rs` 定义：

```rust
pub const INTEGRITY_KEY_SCHEMA: &str = "kiana.workflow-integrity-key.v1";
pub const INTEGRITY_ENVELOPE_SCHEMA: &str = "kiana.integrity-envelope.v1";
pub const LOCAL_HMAC_ALGORITHM: &str = "local_hmac_sha256_v1";

#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("integrity_key_missing")]
    KeyMissing,
    #[error("integrity_key_invalid: {0}")]
    KeyInvalid(String),
    #[error("integrity_key_permissions_insecure")]
    InsecurePermissions,
    #[error("integrity_key_id_mismatch")]
    KeyIdMismatch,
    #[error("integrity_payload_mismatch")]
    PayloadMismatch,
    #[error("integrity_auth_mismatch")]
    AuthMismatch,
    #[error("integrity_chain_mismatch")]
    ChainMismatch,
    #[error("integrity_downgrade_detected")]
    DowngradeDetected,
    #[error("integrity filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("integrity json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntegrityEnvelope {
    pub schema: String,
    pub algorithm: String,
    pub key_id: String,
    pub payload_sha256: String,
    pub previous_record_sha256: String,
    pub auth: String,
}
```

- [x] **Step 4：实现外部 key path 和原子写**

必须实现：

```rust
pub fn workflow_integrity_key_path() -> Result<PathBuf, IntegrityError>;
pub fn initialize_local_hmac_key() -> Result<IntegrityKeyStatus, IntegrityError>;
pub fn initialize_local_hmac_key_at(path: &Path) -> Result<IntegrityKeyStatus, IntegrityError>;
pub fn inspect_local_hmac_key() -> Result<IntegrityKeyStatus, IntegrityError>;
pub fn load_local_hmac_key() -> Result<Option<LocalHmacKey>, IntegrityError>;
pub fn load_local_hmac_key_at(path: &Path) -> Result<LocalHmacKey, IntegrityError>;
```

路径优先级：

```text
KIANA_WORKFLOW_INTEGRITY_KEY_FILE
KIANA_HOME/trust/workflow-integrity-key.json
HOME/.kiana/trust/workflow-integrity-key.json
```

没有外部 home 时返回 `KeyMissing`，禁止项目目录 fallback。

Unix 写入顺序：创建 trust dir、chmod 0700、create_new 0600 temp、write、sync_all、rename、chmod 0600、sync parent。

- [x] **Step 5：实现 HMAC 输入和验证**

使用长度前缀编码：

```rust
fn push_len_prefixed(buffer: &mut Vec<u8>, value: &[u8]) {
    buffer.extend_from_slice(&(value.len() as u64).to_be_bytes());
    buffer.extend_from_slice(value);
}
```

公共方法：

```rust
pub fn sign_payload(&self, domain: &str, payload: &[u8], previous_record_sha256: &str)
    -> Result<IntegrityEnvelope, IntegrityError>;
pub fn verify_payload(&self, domain: &str, payload: &[u8], envelope: &IntegrityEnvelope)
    -> Result<(), IntegrityError>;
```

- [x] **Step 6：运行 focused verification**

```bash
cargo test -p kiana-tasks integrity::tests --lib --locked --offline --no-fail-fast
```

Expected: PASS，stdout 不包含 secret。

## Task 2：WorkflowEvent v2 认证链

- [x] **Step 1：增加签名链 adversarial verification**

在 `kiana-tasks/tests/workflow_runtime.rs` 使用静态环境锁和隔离 `KIANA_HOME`，增加：

```rust
#[test]
fn signed_workflow_eventlog_detects_middle_event_mutation() {
    let _guard = INTEGRITY_ENV_LOCK.lock().unwrap();
    let root = temp_root();
    std::env::set_var("KIANA_HOME", root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(&root, gated_init("signed workflow")).unwrap();
    append_workflow_event(&run.artifact_dir, WorkflowEventKind::Learned, "learn", json!({"value": "original"})).unwrap();
    let path = run.artifact_dir.join("eventlog.jsonl");
    let contents = std::fs::read_to_string(&path).unwrap().replace("capture", "tampered");
    std::fs::write(&path, contents).unwrap();
    let error = read_workflow_events(&run.artifact_dir).unwrap_err().to_string();
    assert!(error.contains("integrity_payload_mismatch"), "{error}");
}
```

同时增加 wrong key、删除中间事件、重排事件、signed 后 unsigned downgrade、key missing 和全 legacy 可读测试。

- [x] **Step 2：记录实现后 regression command**

```bash
cargo test -p kiana-tasks --test workflow_runtime signed_workflow --locked --offline --no-fail-fast
```

- [x] **Step 3：扩展 WorkflowEvent**

```rust
pub struct WorkflowEvent {
    pub schema: String,
    pub seq: u64,
    pub at_ms: u64,
    pub kind: WorkflowEventKind,
    pub node_id: String,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<IntegrityEnvelope>,
}
```

- [x] **Step 4：统一事件构造函数**

新增：

```rust
fn build_workflow_event_under_lease(
    artifact_dir: &Path,
    kind: WorkflowEventKind,
    node_id: String,
    data: Value,
) -> WorkflowResult<WorkflowEvent>;
```

规则：无 key 写 v1；空 EventLog + key 写 v2 genesis；已签名链继续签名；legacy + key 且无 seal 返回 `integrity_seal_required`；所有 append 和 artifact commit 共用该函数。

- [x] **Step 5：实现 read 状态机**

```text
unsigned* -> EOF          unsigned_legacy
signed* -> EOF            verified
unsigned* -> signed*      only with valid genesis seal
signed -> unsigned        downgrade error
signed + missing key      key missing error
```

- [x] **Step 6：运行 workflow 回归**

```bash
cargo test -p kiana-tasks --test workflow_runtime --locked --offline --no-fail-fast
cargo test -p kiana-tasks --lib --locked --offline --no-fail-fast
```

## Task 3：Legacy Genesis Seal 与 Trust Report

- [x] **Step 1：增加 seal adversarial verification**

测试 legacy workflow 在 key 创建后直接 append 返回 `integrity_seal_required`，并覆盖 seal 幂等、history 变化冲突、seal 后签名 append 和 trust report 计数。

- [x] **Step 2：实现类型与 API**

```rust
pub struct WorkflowIntegrityGenesisSeal {
    pub schema: String,
    pub workflow_id: String,
    pub legacy_event_count: u64,
    pub legacy_eventlog_sha256: String,
    pub sealed_at_ms: u64,
    pub key_id: String,
    pub integrity: IntegrityEnvelope,
}

pub enum WorkflowTrustStatus {
    UnsignedLegacy,
    SealedLegacyPrefix,
    Verified,
    UnverifiableKeyMissing,
    Invalid,
}

pub fn seal_legacy_workflow(artifact_dir: &Path) -> WorkflowResult<WorkflowIntegrityGenesisSeal>;
pub fn inspect_workflow_integrity(artifact_dir: &Path) -> WorkflowResult<WorkflowIntegrityReport>;
```

- [x] **Step 3：写 immutable seal**

seal 路径固定为 `integrity/genesis-seal.json`。同内容重复调用幂等；不同内容返回 `integrity_genesis_conflict`。

- [x] **Step 4：运行 seal tests**

```bash
cargo test -p kiana-tasks --test workflow_runtime integrity_seal --locked --offline --no-fail-fast
```

## Task 4：Workflow Integrity CLI

- [x] **Step 1：创建 CLI regression verification**

Create `kiana-commands/tests/workflow_integrity_command.rs`，覆盖：init 幂等和 redaction、status missing key、verify legacy、seal、tampered event、unknown option、human output 不泄露 secret。

- [x] **Step 2：记录实现后 focused verification command**

```bash
cargo test -p kiana-commands --test workflow_integrity_command --locked --offline --no-fail-fast
```

- [x] **Step 3：扩展 parser 与 usage**

```text
workflow integrity init [--json]
workflow integrity status [--json]
workflow integrity verify [--json] [run_id]
workflow integrity seal [--json] <run_id>
```

JSON schema：

```text
kiana.workflow-integrity-key-status.v1
kiana.workflow-integrity-report.v1
kiana.workflow-integrity-seal-result.v1
```

command layer 只调用 `kiana-tasks` API，不重复密码学实现。

- [x] **Step 4：运行 CLI tests**

```bash
cargo test -p kiana-commands --test workflow_integrity_command --locked --offline --no-fail-fast
cargo test -p kiana-commands workflow_ --lib --locked --offline --no-fail-fast
```

## Task 5：回归、文档与证据

- [x] **Step 1：局部完整验证**

```bash
cargo test -p kiana-tasks --locked --offline --no-fail-fast
cargo test -p kiana-commands --test workflow_integrity_command --locked --offline --no-fail-fast
cargo test -p kiana-commands workflow_ --lib --locked --offline --no-fail-fast
```

- [x] **Step 2：workspace 验证**

```bash
cargo test --workspace --locked --offline --no-fail-fast
cargo build --workspace --locked --offline
cargo fmt --all --check
git diff --check
```

- [x] **Step 3：secret 扫描**

```bash
rg -n "secret_hex|workflow-integrity-key" .kiana docs kiana-* -g '!target/**' -g '!reference/**'
```

字段名可以存在于源码和 schema，但 report、EventLog、fixture 和命令输出中不得出现真实 secret。

- [x] **Step 4：更新 feature matrix**

记录 external HMAC key store、authenticated EventLog、legacy seal、integrity CLI 和实际测试命令。继续保留 artifact descriptor、journal MAC、Ed25519、KMS/TPM、外部签名和客户验收缺口。

- [x] **Step 5：范围检查**

```bash
git status --short
git diff --stat -- kiana-tasks/Cargo.toml kiana-tasks/src/integrity.rs kiana-tasks/src/lib.rs kiana-tasks/src/workflow.rs kiana-tasks/tests/workflow_runtime.rs kiana-commands/src/tasks.rs kiana-commands/tests/workflow_integrity_command.rs docs/reference-feature-matrix.md docs/superpowers/specs/2026-07-10-workflow-integrity-trust-root-design.md docs/superpowers/plans/2026-07-10-workflow-integrity-eventlog.md
```

不得自动提交当前 dirty worktree。

## 完成条件

本计划完成只代表总规格 Milestone 1 和 Milestone 2 完成。Artifact descriptor、Recovery Journal MAC、strict audit/report/release blocker、Ed25519 和企业 signer adapter 仍必须保留在 active goal 中继续实现。
