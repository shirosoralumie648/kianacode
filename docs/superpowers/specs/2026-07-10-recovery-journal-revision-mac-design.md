# Recovery Journal Revision MAC 设计

**状态：** 已批准进入实施（承接 active commercial goal 与 Artifact Binding 后续路线）

**目标：** 让 `kiana.swarm-integration-recovery-journal` 从项目目录内可任意改写的恢复状态，升级为由项目外 HMAC key 认证、可检测篡改/降级/回放、可在 crash window 中确定性续跑、可形成 strict audit 与商业发布证据的恢复事实链。

**范围：** 本设计只覆盖 Bounded Swarm 串行集成的 active recovery journal、journal revision 锚点、completed archive、strict audit/progress/release proof。不会在本切片实现 Ed25519、KMS/TPM/PKCS#11、远端透明日志、团队多签或自动 push/merge/deploy。

---

## 1. 问题定义

当前 `recovery/journal.json` 已经记录：

- integration/dispatch/plan/checkpoint 绑定；
- 恢复前后 working-tree fingerprint；
- source manifest、Git HEAD、index diff；
- 每个路径的 source/restored descriptor；
- `pending -> restoring -> completed` 状态；
- crash/retry 进度和 `last_error`。

它已经能阻断 unrelated drift、checkpoint 篡改和非幂等恢复，但仍存在四个信任缺口：

1. 项目目录写权限持有者可以直接修改 `status`、`next_entry_index`、entry status 或 descriptor。
2. 给当前 JSON 单独加 HMAC 仍不能阻止旧的合法 journal revision 被回放。
3. completed journal archive 目前只是 `rename`，archive 内容和归档动作没有进入 immutable artifact/event 事实链。
4. strict audit、progress report 和 release blockers 没有统一暴露 journal integrity 状态。

商业化边界要求：恢复机制不能只“看起来有日志”，而必须能够证明“当前执行依据的是最新、完整、由外部 key 认证的恢复状态”。

---

## 2. 设计原则

1. **密钥在项目外。** 复用 `kiana.workflow-integrity-key.v1`，不创建第二套隐式 secret。
2. **每次 mutation 都是新 revision。** 任何状态或 entry 变化都必须递增 revision 并重签。
3. **认证不等于新鲜度。** journal HMAC 证明内容未伪造；Workflow EventLog revision anchor 证明它是当前最新 revision。
4. **先记录意图，再执行文件系统操作。** `restoring` revision 必须在路径 mutation 之前 durable。
5. **crash window 可判定。** journal 已写、event 未写是唯一允许自动 reconcile 的 unanchored tail。
6. **archive 是 immutable fact。** completed journal 通过统一 immutable artifact commit 写入 history 并由 event descriptor 认证。
7. **缺密钥 fail closed。** 没有 key 时不得开始或继续任何 recovery mutation。
8. **不自动信任 legacy。** v1/无 integrity journal 可被审计读取，但不能 mutation、archive 或作为发布证据。
9. **一个事实，多种投影。** CLI、strict audit、progress 和 release proof 都消费同一 inspector 结果。

---

## 3. 方案比较

### 3.1 方案 A：只给当前 journal JSON 加 HMAC

优点：改动最小，验证简单。

缺点：攻击者可以保存 revision 2，等系统运行到 revision 8 后再放回 revision 2；旧记录的 HMAC 仍然合法，系统无法判断回放。

**结论：拒绝。** 它解决篡改，不解决 freshness。

### 3.2 方案 B：journal HMAC revision chain + Workflow EventLog 最新锚点

每个 journal revision：

- 包含单调递增 `revision`；
- integrity envelope 中包含上一份完整签名记录的 SHA-256；
- 每次 durable write 后追加一个认证 Workflow EventLog revision anchor；
- inspector 比较 journal revision/hash 与最新 anchor。

优点：复用现有外部 HMAC trust root 和 EventLog；能检测篡改、删除、降级、旧合法 revision 回放；与现有 crash/retry 状态机兼容。

缺点：每个状态 mutation 会增加一个 EventLog 事件；需要明确 write-event crash reconciliation。

**结论：采用。** 这是当前架构下最小且完整的商业级方案。

### 3.3 方案 C：把 recovery journal 改成独立 append-only log

优点：天然保留全部 revision，可做完整 forensic replay。

缺点：需要重新设计恢复读取、压缩、归档、并发 lease、迁移和 CLI；与当前单文件状态机差异过大。

**结论：暂不采用。** 可作为 P2 远端 witness/透明日志方向。

---

## 4. 数据契约

### 4.1 Journal schema

schema 升级为：

```text
kiana.swarm-integration-recovery-journal.v2
```

新增字段：

```json
{
  "schema": "kiana.swarm-integration-recovery-journal.v2",
  "revision": 7,
  "integration_id": "int_...",
  "dispatch_id": "dispatch_...",
  "plan_sha256": "sha256:...",
  "checkpoint_path": "checkpoint/manifest.json",
  "checkpoint_sha256": "sha256:...",
  "status": "restoring",
  "next_entry_index": 1,
  "entries": [],
  "created_at_ms": 0,
  "updated_at_ms": 0,
  "integrity": {
    "schema": "kiana.integrity-envelope.v1",
    "algorithm": "local_hmac_sha256_v1",
    "key_id": "sha256:...",
    "payload_sha256": "sha256:...",
    "previous_record_sha256": "sha256:...",
    "auth": "hmac-sha256:..."
  }
}
```

`integrity` 在 Rust 类型中允许 `Option` 只为反序列化和 downgrade 诊断；任何生产 write 都要求 `Some`。

### 4.2 Payload canonicalization

签名 payload 为 journal 的 JSON object 删除 `integrity` 后的紧凑 UTF-8 JSON：

```text
serde_json::to_value(journal)
remove("integrity")
serde_json::to_vec(value)
```

字段由 Rust struct 稳定序列化；不接受任意调用方 JSON merge。

签名 domain：

```text
swarm-integration-recovery-journal
```

### 4.3 Record hash

`record_sha256` 是完整、已签名 journal JSON 的 SHA-256：

```text
sha256_prefixed(serde_json::to_vec(journal))
```

下一 revision 的 `integrity.previous_record_sha256` 必须等于当前 `record_sha256`。

第一条 revision 使用固定 genesis：

```text
sha256:0000000000000000000000000000000000000000000000000000000000000000
```

### 4.4 Revision event

新增 `WorkflowEventKind::SwarmIntegrationRecoveryRevisionCommitted`。

事件数据最小结构：

```json
{
  "journal_revision_id": "<integration>:<checkpoint>:<revision>",
  "integration_id": "int_...",
  "dispatch_id": "dispatch_...",
  "checkpoint_sha256": "sha256:...",
  "journal_path": "swarm/integrations/<id>/recovery/journal.json",
  "journal_binding_sha256": "sha256:...",
  "revision": 7,
  "record_sha256": "sha256:...",
  "payload_sha256": "sha256:...",
  "previous_record_sha256": "sha256:...",
  "key_id": "sha256:...",
  "status": "restoring",
  "next_entry_index": 1
}
```

unique field 为 `journal_revision_id`，重复 reconcile 必须 idempotent。

### 4.5 Archive event

新增 `WorkflowEventKind::SwarmIntegrationRecoveryArchived`。

archive 通过 `commit_immutable_artifacts_with_unique_event` 写入：

```text
swarm/integrations/<integration>/recovery/history/journal-<created>-<checkpoint>-r<revision>.json
```

事件和自动 `integrity_artifacts` descriptor 同时绑定 archive path、size、SHA-256、media type 和 role。

---

## 5. 统一写入协议

所有 journal mutation 必须调用一个统一 commit helper，禁止直接 `write_json_durable_atomic`。

### 5.1 创建 revision 1

1. 在任何项目文件 mutation 前加载外部 HMAC key。
2. 构造 unsigned journal draft，`revision = 1`。
3. 设置 `previous_record_sha256 = genesis`。
4. 生成 payload 和 integrity envelope。
5. durable atomic write `journal.json`，包含 file fsync 和 parent dir fsync。
6. 计算完整 signed record hash。
7. 追加 idempotent revision event。
8. 返回已认证 journal。

### 5.2 更新 revision N+1

1. 读取当前 journal 并验证 schema、HMAC、binding、tree state。
2. 与最新 revision event reconcile。
3. 在 clone 上应用一个状态 mutation。
4. `revision = current.revision + 1`。
5. `previous_record_sha256 = current.record_sha256`。
6. 清除旧 integrity，签新 payload。
7. durable atomic write。
8. 追加 revision event。

调用方只有在步骤 8 成功或被判定为可 reconcile tail 后，才能执行下一次 mutation。

---

## 6. Crash / Retry 一致性模型

### 6.1 Journal write 前 crash

磁盘仍是上一 revision；重试从上一 revision 继续，无特殊处理。

### 6.2 Journal write 成功、revision event 未写

这是唯一自动修复的 unanchored tail：

- journal HMAC 必须有效；
- journal revision 必须等于 latest anchor revision + 1；
- journal previous hash 必须等于 latest anchor record hash；
- 如果不存在 anchor，只允许 revision 1 + genesis previous hash；
- reconcile 只追加缺失 event，不重写 journal。

### 6.3 Revision event 成功后 crash

journal 与 event 已一致；重试直接继续。

### 6.4 文件系统操作后、Completed revision 前 crash

journal 保持 `restoring`。现有 descriptor 状态机允许 source/restored 两种受控状态；重试重复安全操作并提交 Completed revision。

### 6.5 Archive commit 后、active journal 删除前 crash

immutable archive event/artifact 已存在；重试验证 archive 与 active journal record hash 一致，再删除 active journal。

### 6.6 Active journal 删除后 crash

archive event/artifact 是完成证据；下一 checkpoint 可以继续，不需要恢复 active journal。

---

## 7. 回放、篡改和降级规则

以下情况全部 fail closed：

- journal schema 为 v2 但缺少 integrity；
- HMAC、payload hash、key ID 或 algorithm 不匹配；
- revision 为 0、倒退或与 latest anchor 不一致；
- 同 revision 的 record hash 与 event 不一致；
- journal 比 latest anchor 旧；
- journal 比 latest anchor 超前超过 1；
- previous record hash 不链接 latest anchor；
- v1/无签名 journal尝试 mutation 或 archive；
- key missing/insecure/invalid；
- archive 内容与 immutable descriptor/event 不一致。

稳定错误前缀：

```text
integration_recovery_integrity_key_missing
integration_recovery_integrity_downgrade
integration_recovery_integrity_mismatch
integration_recovery_revision_replay
integration_recovery_revision_gap
integration_recovery_archive_mismatch
```

错误不得被转换为普通 `blocked` 状态后继续文件 mutation。

---

## 8. 读取与检查模型

新增 command-layer inspector：

```text
inspect_swarm_recovery_integrity(artifact_dir)
```

报告字段：

```json
{
  "schema": "kiana.swarm-recovery-integrity-report.v1",
  "status": "verified",
  "active_journal_count": 1,
  "verified_active_journal_count": 1,
  "archived_journal_count": 2,
  "verified_archived_journal_count": 2,
  "recoverable_unanchored_tail_count": 0,
  "legacy_unsigned_count": 0,
  "mismatch_count": 0,
  "key_id": "sha256:..."
}
```

纯 inspector 不写文件、不自动 reconcile。只有 recovery mutation 路径可以补写可证明的缺失 revision event。

---

## 9. 产品门禁投影

### 9.1 Strict audit

`audit strict` 在 board/evidence 审计前调用 recovery inspector。

blocking finding：

```text
category: workflow_recovery_integrity
severity: block
owner: workflow-owner
next_action: resume_or_restore_authenticated_recovery_journal
```

`recoverable_unanchored_tail` 仍是 blocker，因为 audit 是只读操作，不能代替 resume 自动修复。

### 9.2 Progress report

strict audit 持久化 finding 后，现有 Evidence Ledger / unresolved blocker 投影将其显示在 `report progress`。新增回归测试证明 category、evidence 和 next_action 不丢失，不另造第二套 report 状态。

### 9.3 Workflow CLI proof

`tasks workflow integrity verify --json` 增加嵌套 `recovery_integrity` 报告，human output 增加 active/archive verified 计数。

### 9.4 Commercial release blocker

发布证据路径：

```text
dist/proofs/workflow/recovery-integrity.json
```

内容使用 workflow integrity verify 的 JSON 输出。`commercial-release-blockers-report.sh` 增加 local/final-artifact-derived gate：

- schema 正确；
- EventLog status verified；
- recovery integrity status verified；
- mismatch、legacy unsigned、unanchored tail 均为 0；
- key material 不得出现在 proof 中。

缺失或不合格时保留 blocking check，不伪造 release readiness。

---

## 10. Archive 协议

1. active journal 必须 `status=completed`。
2. HMAC、revision anchor、binding 和 tree state必须验证。
3. archive path 必须是 workflow-relative safe path。
4. immutable artifact commit 写 archive bytes 和 `SwarmIntegrationRecoveryArchived` event。
5. commit 成功后删除 active journal并 fsync recovery dir。
6. 若 archive/event 已存在，要求 descriptor、record hash、revision 和 binding 全部一致后幂等删除 active journal。
7. 不接受无 event 的预置 archive，也不覆盖不同内容。

---

## 11. 测试矩阵

### 11.1 Unit / command internal

- revision 1 使用 genesis previous hash；
- 每次 mutation revision +1；
- HMAC 覆盖 status、entry status、next index、error、timestamps；
- wrong key、payload mutation、strip integrity；
- current journal replay到旧 revision；
- revision gap；
- key missing 时零项目 mutation；
- write-before-event crash reconcile；
- event-present retry idempotency；
- completed archive immutable commit；
- archive mutation、deletion、replacement；
- partial restore 与 crash retry。

### 11.2 Integration

- `swarm integrate apply` 正常 rollback；
- crash 后 retry；
- unrelated user edit；
- checkpoint backup tamper；
- active journal tamper；
- old signed journal replay；
- missing key；
- archive tamper；
- strict audit blocker；
- report progress blocker projection；
- workflow integrity CLI counts；
- release blockers proof accept/reject。

### 11.3 Full gates

```bash
cargo test -p kiana-commands recovery_journal_ --lib --locked --offline --no-fail-fast
cargo test -p kiana-commands --test swarm_command --locked --offline --no-fail-fast
cargo test -p kiana-commands --test workflow_integrity_command --locked --offline --no-fail-fast
cargo test -p kiana-commands --test workflow_integrity_audit --locked --offline --no-fail-fast
cargo test -p kiana-commands --test report_command --locked --offline --no-fail-fast
cargo test --workspace --locked --offline --no-fail-fast
cargo build --workspace --locked --offline
cargo fmt --all --check
git diff --check
```

---

## 12. 兼容与迁移

- 新创建 journal 一律 v2。
- v1 active journal返回 `legacy_unsigned`，允许只读诊断，不允许 mutation/archive。
- 不自动把 v1 内容升级并签名，因为无法证明此前内容未被修改。
- 操作员必须恢复到可信 checkpoint/source 后重新开始 recovery，或保留 v1 作为非发布诊断证据。
- 已有 v1 archive 不作为 authenticated archive 计数，strict audit 和 release proof 均保持 blocker。

---

## 13. NOT_BUILDING

本切片不实现：

- 非对称签名与跨机器信任；
- HSM/KMS/TPM/PKCS#11；
- 远端 transparency log / witness；
- journal 内容加密；
- 自动修复任意缺失 EventLog；
- 自动接受或签名 legacy journal；
- 自动 commit/push/merge/deploy；
- 非 Linux secure restore 语义扩展。

---

## 14. 完成标准

只有同时满足以下条件，本切片才能标记完成：

1. 所有 journal mutation 经过统一 revision MAC commit。
2. key missing、strip integrity、tamper、wrong key、replay、gap 全部在文件 mutation 前阻断。
3. write-before-event crash 可确定性 reconcile。
4. completed archive 由 immutable artifact/event 认证并可验证。
5. strict audit、progress、workflow integrity CLI 和 release blockers 都能消费同一事实。
6. focused、workspace、build、fmt、diff gates 全通过。
7. feature matrix 只关闭本地 journal/archive trust gap；Ed25519、硬件密钥、外部签名和客户验收继续保持开放。
