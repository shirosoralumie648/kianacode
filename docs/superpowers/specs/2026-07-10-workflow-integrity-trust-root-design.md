# Kiana Workflow Integrity Trust Root 设计规格

## 1. 文档状态

- 状态：已选定默认路线，进入实施。
- 范围：Workflow EventLog、Workflow Artifact、Swarm Recovery Journal 的可认证完整性。
- 当前阶段：P0 本机外部 HMAC trust root。
- 后续阶段：P1 Ed25519 可公开验证签名，P2 企业外部签名器。
- 不包含：伪造生产签名、云端 KMS 凭据、客户验收或发布渠道证明。

## 2. 问题定义

当前 WorkflowRun 已具备：

- append-only `eventlog.jsonl`；
- 递增 `seq`；
- state 与 eventlog 序号一致性校验；
- artifact SHA-256；
- integration plan、checkpoint、verification、packet 之间的摘要绑定；
- recovery journal immutable binding hash；
- 原子写、writer lease、`fsync` 与崩溃恢复。

但这些数据全部位于项目 `.kiana/` 写域。拥有项目目录写权限的进程可以同时修改：

1. EventLog；
2. state；
3. artifact；
4. artifact SHA-256；
5. recovery journal；
6. journal binding hash；
7. 验证事件和结果 packet。

因此当前机制可以发现意外损坏、局部篡改和不完整恢复，但无法抵抗“整体重写并重新计算全部 SHA-256”的攻击。

## 3. 设计目标

### 3.1 核心目标

1. 信任根必须位于项目目录之外。
2. 项目写权限不能等价于完整性签名权限。
3. EventLog 必须形成可认证链，而不是普通 hash chain。
4. Artifact 必须通过认证事件绑定 path、size 和 content digest。
5. Recovery Journal 每次持久化修订都必须认证。
6. 读取、恢复、继续、审计和交付必须报告明确 trust status。
7. 缺失密钥、错误密钥、未知 key id、MAC 不匹配和降级攻击必须 fail closed。
8. 旧 workflow 不得被追溯声称为“从创建起可信”。

### 3.2 商业化目标

1. `/audit strict` 能识别未配置 trust root、未认证 workflow 和完整性失败。
2. release readiness 将本地完整性配置纳入 local blocker。
3. export/report 输出 trust summary，不泄露 secret。
4. 企业离线环境不依赖云服务即可验证本机生成证据。
5. 后续可以无损替换为 Ed25519、TPM、PKCS#11 或 KMS signer。

## 4. 非目标

P0 不解决：

- root 用户或同一用户账户已完全失陷；
- 内核、文件系统或运行时已被控制；
- 公共第三方在没有共享 secret 时验证 HMAC；
- 组织级 key rotation、revocation 和证书透明日志；
- 自动上传签名证据到外部服务；
- 生产制品代码签名或 notarization。

## 5. 威胁模型

### 5.1 要抵抗

- worker、plugin、hook 或 shell 只能写项目目录；
- 非法进程重写 `.kiana/workflows/<id>/`；
- 删除或替换 EventLog 中间事件；
- 修改 event data 后重算普通 SHA-256；
- 替换 artifact 并同步修改 packet hash；
- 修改 recovery journal 状态以跳过恢复步骤；
- 将已认证 workflow 降级成未认证格式；
- 使用另一台机器或另一用户的密钥伪造当前 key id。

### 5.2 不保证

- 攻击者可以读取 `$KIANA_HOME/trust/`；
- 攻击者可以调用合法 Kiana signer API；
- 攻击者控制当前用户进程空间；
- 攻击者拥有 root、内核或硬件级权限。

## 6. 方案比较

### 6.1 普通 hash chain

优点：

- 无密钥；
- 易迁移；
- 易公开验证。

缺点：

- 项目写者可重算整条链；
- 不构成独立 trust root。

结论：只作为内部链结构，不作为认证机制。

### 6.2 本机 HMAC-SHA256

优点：

- 高性能；
- 实现和验证简单；
- 适合每个 event 和 journal revision；
- 可完全离线；
- 与现有 SHA-256 artifact 模型兼容。

缺点：

- 验证方必须拥有同一 secret；
- 无法让客户只拿公钥验证；
- secret 泄露后可伪造历史。

结论：P0 默认实现。

### 6.3 Ed25519

优点：

- 私钥签名、公钥验证；
- 导出包可离线公开验证；
- 更适合客户、审计和跨机器 handoff。

缺点：

- key rotation、revocation 和身份绑定复杂；
- 每事件签名成本和 artifact 体积高于 HMAC；
- 私钥存储需要更强平台集成。

结论：P1 在统一 signer 接口上实现。

### 6.4 企业外部 signer

可选 backend：

- TPM；
- OS keystore；
- PKCS#11；
- Vault Transit；
- 云 KMS。

结论：P2 adapter，不作为本地 P0 强依赖。

## 7. 总体架构

```text
Workflow Writer
    |
    v
Canonical Payload ----> SHA-256
    |                       |
    |                       v
    +--------------> IntegritySigner
                            |
                            v
                   Integrity Envelope
                            |
                            v
                  Event / Journal / Receipt
```

核心模块：

```text
kiana-tasks::integrity
  - IntegritySigner
  - IntegrityVerifier
  - LocalHmacKeyStore
  - IntegrityEnvelope
  - IntegrityStatus
  - canonical payload helpers
```

调用方：

- `kiana-tasks::workflow`：EventLog 与 artifact commit；
- `kiana-commands::tasks`：swarm recovery journal；
- `kiana-commands::audit`：strict finding；
- `kiana-commands::report`：trust summary；
- release readiness：local blocker 与 proof summary。

## 8. Trust Root 存储

### 8.1 默认路径

优先级：

1. `KIANA_WORKFLOW_INTEGRITY_KEY_FILE`；
2. `$KIANA_HOME/trust/workflow-integrity-key.json`；
3. `$HOME/.kiana/trust/workflow-integrity-key.json`。

禁止回退到项目 `.kiana/`。

### 8.2 权限

Unix：

- trust directory：`0700`；
- key file：`0600`；
- 创建必须使用 `create_new`；
- 临时文件同样使用 `0600`；
- 写入后 `sync_all`；
- rename 后同步父目录；
- 已存在文件权限过宽时，status 为 `insecure_permissions`。

Windows：

- P0 使用用户目录文件；
- status 明确报告 `file_acl_unverified`；
- P1/P2 接入 DPAPI、Credential Manager 或 TPM 后再提升等级。

### 8.3 Key schema

```json
{
  "schema": "kiana.workflow-integrity-key.v1",
  "algorithm": "hmac-sha256",
  "key_id": "sha256:<digest>",
  "created_at_ms": 0,
  "secret_hex": "<64 lowercase hex chars>"
}
```

约束：

- secret 为 32 字节 CSPRNG 输出；
- `key_id = SHA256(domain || secret)`；
- key id 可公开；
- secret 不进入 event、report、artifact 或日志；
- status 输出只允许 path、algorithm、key_id、permission state。

## 9. EventLog 数据契约

### 9.1 WorkflowEvent v2

```json
{
  "schema": "kiana.workflow-event.v2",
  "seq": 3,
  "at_ms": 0,
  "kind": "artifact_written",
  "node_id": "execute",
  "data": {},
  "integrity": {
    "schema": "kiana.integrity-envelope.v1",
    "algorithm": "hmac-sha256",
    "key_id": "sha256:<digest>",
    "payload_sha256": "sha256:<digest>",
    "previous_record_sha256": "sha256:<digest>|genesis",
    "auth": "hmac-sha256:<digest>"
  }
}
```

### 9.2 Canonical payload

参与 `payload_sha256` 的字段：

- schema；
- seq；
- at_ms；
- kind；
- node_id；
- data。

不包含 `integrity`。

Canonical JSON 规则：

- UTF-8；
- object key 稳定排序；
- 无额外空白；
- integer 保持十进制；
- 禁止 NaN/Infinity；
- 不接受重复 JSON key。

### 9.3 Auth input

```text
domain = "kiana.workflow-event.v2\0"
workflow_id
seq
key_id
payload_sha256
previous_record_sha256
```

所有字段使用长度前缀编码，不使用模糊字符串拼接。

### 9.4 Chain

- 第一条已认证事件使用 `genesis`；
- 后续事件的 `previous_record_sha256` 是上一条完整 canonical event record 的 SHA-256；
- 删除、中插、重排、替换或降级任一事件都会导致验证失败；
- EventLog 尾部截断会通过 state、expected tail 和 seal receipt 检出。

## 10. Legacy 与迁移

### 10.1 状态

- `unsigned_legacy`：全部事件未认证；
- `sealed_legacy_prefix`：前缀未认证，之后进入认证链；
- `verified`：从第一条事件开始认证；
- `unverifiable_key_missing`：有认证事件但本机无对应 key；
- `invalid`：摘要、链或 auth 不匹配；
- `downgrade_detected`：认证事件之后出现未认证事件。

### 10.2 Genesis seal

对旧 workflow 执行 seal 时生成：

```json
{
  "schema": "kiana.workflow-integrity-genesis.v1",
  "workflow_id": "...",
  "legacy_event_count": 12,
  "legacy_eventlog_sha256": "sha256:<digest>",
  "sealed_at_ms": 0,
  "key_id": "sha256:<digest>",
  "auth": "hmac-sha256:<digest>"
}
```

它只证明“seal 时看见的历史”，不得描述成从创建起可信。

### 10.3 禁止静默迁移

- key 创建后不自动把旧历史标成 verified；
- 第一次向旧 workflow 写认证事件前必须存在 genesis seal；
- 缺少 seal 时返回 `integrity_seal_required`；
- seal 不能覆盖已有不同内容；
- seal 必须有独立 artifact path 和 event reference。

## 11. Artifact 认证

### 11.1 Artifact descriptor

```json
{
  "path": "verification/vp_001.json",
  "size": 1024,
  "sha256": "sha256:<digest>",
  "media_type": "application/json",
  "role": "verification_packet"
}
```

### 11.2 绑定方式

`write_workflow_artifacts_with_event` 自动将 descriptors 写入 event data 的保留字段：

```json
{
  "integrity_artifacts": []
}
```

规则：

- 调用方不能覆盖该字段；
- descriptor 按 path 排序；
- path 必须是规范化 workflow-relative path；
- artifact 写入成功后才签 event；
- event 写入失败时 artifact 保留为 orphan，并由 audit 明确报告；
- 已存在同内容 artifact 可复用，但 descriptor 必须一致。

## 12. Recovery Journal 认证

### 12.1 Journal envelope

Recovery journal 增加：

```json
{
  "integrity": {
    "schema": "kiana.integrity-envelope.v1",
    "algorithm": "hmac-sha256",
    "key_id": "sha256:<digest>",
    "payload_sha256": "sha256:<digest>",
    "previous_record_sha256": "revision:<n-1 digest>|genesis",
    "auth": "hmac-sha256:<digest>"
  }
}
```

### 12.2 Revision 规则

- journal 增加递增 `revision`；
- 每次状态改变后对完整 payload 重新认证；
- `previous_record_sha256` 绑定上一 revision；
- 原子写保留现有临时文件、rename 和目录同步；
- active journal 与 history archive 都必须验证；
- key 存在时，删除 integrity 字段视为 downgrade；
- key 缺失时，不允许继续 mutation，只能返回 block report。

## 13. 命令设计

### 13.1 Key 管理

```text
kiana tasks workflow integrity init [--json]
kiana tasks workflow integrity status [--json]
kiana tasks workflow integrity verify [run_id] [--json]
kiana tasks workflow integrity seal <run_id> [--json]
```

### 13.2 init

行为：

- 创建外部 key；
- 已存在同 schema key 时幂等返回；
- 已存在非法文件时 fail closed；
- 不输出 secret；
- 返回 key id、algorithm、path、permission state。

### 13.3 status

返回：

- key configured；
- key id；
- storage path；
- permission state；
- backend；
- commercial readiness status。

### 13.4 verify

返回：

- workflow id；
- trust status；
- verified event count；
- legacy prefix count；
- first authenticated seq；
- last authenticated seq；
- artifact descriptor count；
- journal status；
- failure reason 与 failure seq。

### 13.5 seal

行为：

- 读取并校验 legacy event seq；
- 计算完整 EventLog SHA-256；
- 写 genesis seal artifact；
- 后续 append 使用认证 Event v2；
- 重复 seal 同内容幂等；
- 历史变化后 seal 冲突。

## 14. Policy 与 Gate

### 14.1 Quick

- key 缺失：允许未认证 workflow，但显示 warning；
- 已认证 workflow 验证失败：阻断 mutation。

### 14.2 Standard

- key 缺失：允许创建，但 report 标记 `unsigned_legacy`；
- ship、export proof 和高风险 mutation 前必须 verified。

### 14.3 Gated

- key 未配置：`integrity_key_required`；
- 旧 workflow 未 seal：`integrity_seal_required`；
- verify 失败：block；
- 不允许 silent downgrade。

### 14.4 Ship / Release

必须满足：

- trust status 为 `verified` 或被显式允许的 `sealed_legacy_prefix`；
- key permissions secure；
- artifact descriptors verified；
- active/history recovery journals verified；
- strict audit 无 integrity critical/high finding。

## 15. 错误码

- `integrity_key_missing`
- `integrity_key_invalid`
- `integrity_key_permissions_insecure`
- `integrity_key_id_mismatch`
- `integrity_seal_required`
- `integrity_genesis_conflict`
- `integrity_payload_mismatch`
- `integrity_auth_mismatch`
- `integrity_chain_mismatch`
- `integrity_downgrade_detected`
- `integrity_artifact_mismatch`
- `integrity_journal_mismatch`
- `integrity_backend_unsupported`

## 16. 恢复与一致性

### 16.1 Crash before event append

- artifact 可能已存在；
- EventLog 不增加；
- audit 报告 orphan artifact；
- 重试同内容可复用 artifact 并写认证事件。

### 16.2 Crash after event append before state update

- EventLog 是 source of truth；
- replay 验证 MAC 后重建 state；
- 未验证前不得修复 state。

### 16.3 Crash during journal update

- 依靠原子 rename 保留上一完整 revision；
- temp file 不作为 authoritative journal；
- 下一次恢复验证 current revision 后继续。

### 16.4 Key rotation

P0 不自动 rotate。

未来 rotation event：

- 旧 key 对 rotation payload 认证；
- 新 key 对相同 payload 认证；
- 后续 event 使用新 key id；
- 两个 auth 缺一不可完成 rotation。

## 17. 审计与报告

### 17.1 Strict audit finding

Critical：

- 已认证链 MAC 不匹配；
- downgrade；
- journal MAC 不匹配；
- artifact descriptor 与内容不匹配。

High：

- gated/ship workflow 无 key；
- key 权限不安全；
- legacy workflow 未 seal 却用于 release proof。

Medium：

- standard workflow 未认证；
- Windows file ACL 尚未验证。

### 17.2 Report trust summary

Report 只输出：

- backend；
- key id；
- trust status；
- verified counts；
- blockers；
- verification timestamp。

不得输出 secret、raw key、MAC input 或可恢复 secret 的材料。

## 18. P0 实施切片

### Milestone 1：Key Store 与 EventLog

- `IntegritySigner` 抽象；
- HMAC key init/status；
- WorkflowEvent v2；
- append 时认证；
- read/replay 时验证；
- legacy、mixed 和 downgrade 状态。

### Milestone 2：Seal 与 CLI

- init/status/verify/seal 命令；
- genesis seal；
- stable JSON schema；
- human-readable output。

### Milestone 3：Artifact Binding

- 自动 descriptors；
- artifact mismatch；
- orphan artifact audit；
- verification/result/review packet 覆盖。

### Milestone 4：Recovery Journal

- journal revision；
- envelope；
- archive verify；
- crash/retry/downgrade tests。

### Milestone 5：Commercial Gate

- strict audit；
- report trust summary；
- release blocker；
- schema smoke；
- package lifecycle proof。

## 19. 测试矩阵

### 19.1 Key Store

- 创建 32-byte key；
- 幂等 init；
- 拒绝非法 schema；
- 拒绝 key id mismatch；
- Unix 目录 0700、文件 0600；
- 不输出 secret；
- 禁止项目内 fallback。

### 19.2 EventLog

- signed append/read；
- event data 修改；
- seq 修改；
- 删除中间事件；
- 重排事件；
- wrong key；
- key missing；
- signed 后 unsigned downgrade；
- legacy prefix + seal；
- concurrent writer；
- crash after append before state update。

### 19.3 Artifact

- content mutation；
- path mutation；
- descriptor order；
- existing same content reuse；
- conflicting artifact；
- orphan detection。

### 19.4 Journal

- every revision auth；
- progress mutation；
- strip integrity；
- wrong key；
- archive tamper；
- process crash resume；
- multi-file partial restore。

### 19.5 Regression

- existing unsigned workflow tests continue passing；
- signed mode has independent tests with isolated `KIANA_HOME`；
- workspace offline tests；
- workspace offline build；
- fmt；
- `git diff --check`。

## 20. 验收标准

P0 完成必须同时满足：

1. 项目目录写者无法在不知道外部 HMAC key 的情况下伪造有效 EventLog。
2. EventLog 任意中间 mutation 被确定性检测。
3. signed 后移除 integrity 字段被识别为 downgrade。
4. artifact content 与认证 descriptor 不一致时 gate 阻断。
5. recovery journal mutation 或 revision rollback 被阻断。
6. 旧 workflow trust status 不被夸大。
7. strict audit 和 release blocker 能报告本地缺口。
8. secret 不出现在 stdout、JSON report、EventLog、artifact 或测试快照中。
9. 所有本地可执行测试与 build 通过。
10. 外部签名、客户验收和线上服务仍作为明确外部 blocker，不伪造完成。

## 21. P1/P2 兼容性要求

`IntegritySigner` 不得硬编码 HMAC 字段语义。统一 envelope 必须允许：

- `local_hmac_sha256_v1`；
- `local_ed25519_v1`；
- `pkcs11_ed25519_v1`；
- `kms_asymmetric_v1`。

Verifier 根据 `algorithm` 与 `key_id` 路由，未知 algorithm 返回稳定错误，不允许降级成 unsigned。

## 22. Definition of Done

本设计不是以“新增一个 hash 字段”为完成，而是以以下闭环为完成：

```text
external key
  -> authenticated EventLog
  -> authenticated artifact descriptors
  -> authenticated recovery journal
  -> verified resume/audit/report
  -> commercial release blocker/proof
```

任何一段缺失，都只能标记为 partial，不得声称 trust root 闭环完成。
