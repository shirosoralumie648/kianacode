# 集成与连接器专项：实际设计、处理流程与实施步骤

> `INT-00`–`INT-33` 是对 [`module-map.md`](../module-map.md) 第 13 模块和 `P4-K8-01` 的实现路线补全。它描述连接器的业务契约、账号绑定、传输适配、幂等、外部回执、对账和入站事件边界，不创建第二条 Agent 执行循环。所有条目从 `⏳` 开始；完成状态必须由 `CURRENT_STATUS.md` 的证据块提升。
>
> 当前源码只提供 `local_fixture` 连接器适配器；HTTP、支付、出行、IoT、企业租户和远程执行仍未开放。本文中的 live 传输、真实 OAuth 和外部业务系统只是后续设计，不能作为当前能力声明。

## 1. 范围、术语和现状

连接器把一个外部业务系统的“账号、操作、数据范围和结果确认”封装成可审计的 Capability。它不是 Provider 的别名，也不是 MCP server 的简单列表：

```text
ConnectorDefinition + AccountBinding + OperationContract
  → ControlPlane admission
  → one Invocation reservation
  → prepared permit
  → transport adapter / Broker
  → ProviderReceipt or ResultUnknown
  → EventLog / Receipt / Reconciliation
```

当前基线：

- `kiana-domain/src/connectors.rs` 已有 `ConnectorDefinition`、`ConnectorOperation`、`AccountBinding`、`ConnectorBindingSnapshot` 和 `ProviderReceipt`，校验范围固定为 `local_fixture` 与 `local_only`。
- `kiana-core/src/connectors.rs` 已把 `connector.manage`、`connector.invoke` 规范化后重新送入普通授权链；风险和 binding snapshot 由服务端解析。
- `kiana-daemon/src/connectors.rs` 已有基于项目 fixture 的 bind、revoke、invoke、reconcile、速率限制和 EventStore 幂等逻辑；输出显式标记 `external_effect_performed: false`。
- `docs/local-extensions-connectors.md` 是当前本地扩展/fixture 的使用合同；它不证明真实外部连接已经接通。
- INT-07 source slice now supplies narrow `ConnectorAdapter`、`EffectObserver`、`CredentialProbe` and
  `WebhookVerifier` ports with explicit capability registration and fail-closed checked wrappers.
  The ports return only receipt/observation/health/occurrence projections; GitHub Actions remains the
  test authority and no live transport or raw credential material is exposed.
- INT-08 source slice now owns the `kiana.connector-fixture.v1` schema in the domain layer. Fixture
  bytes are bounded and hash-pinned, operation cases match canonical payloads with duplicate and
  unknown-operation rejection, and deterministic `ProviderReceipt` projections always carry
  `source=local_fixture` with no external effect. GitHub Actions remains the test authority.
- 当前工作树有并行 WIP，不能把本文件或新增类型当成已验收能力；状态仍以 `CURRENT_STATUS.md` 为准。

### 1.1 与相邻模块的边界

| 模块 | 负责的问题 | 与 Connector 的关系 | 禁止混淆 |
|---|---|---|---|
| Provider | 哪个模型端点处理推理、模型流和用量 | 连接器可以复用 Provider 的 HTTP/OAuth 基础设施，但不复用模型身份 | API key 可用不等于有项目权限 |
| Capability/Broker | 已授权动作在哪里、以什么沙箱执行 | Connector adapter 只能消费 prepared permit | adapter 不得自行读审批、角色或项目权限 |
| MCP | 工具/资源的协议和传输（当前仅 stdio 已支持） | MCP 可承载 connector operation 的结构化调用 | MCP tool list 不授予账号 scope 或写权限 |
| A2A | 长任务和 Agent-to-Agent 协作 | 未来可把远端 Agent 作为异步 connector，但必须映射为 Task/Artifact | A2A `auth_required` 不是 Kiana approval；不能把消息当事实账本 |
| Browser/Search | 浏览器会话、网页检索和读取 | 可作为只读 connector adapter 的输入来源 | 页面内容、工具描述和搜索结果不能扩大权限 |
| Notification | 把已提交事件投递给人或系统 | Slack/Email 等发送动作仍是 connector effect | 通知投递不是 EventLog 事实，也不能确认业务 outcome |
| Workflow/Trigger | 何时排队、重试和恢复 | 触发器只能创建 Run/Workflow，再由普通 connector invocation 执行 | webhook 不能直达 Broker |

## 2. 参考调研与采用取舍

本专项对仓库内 `reference/` 做了 `connector/integration/oauth/mcp/webhook/idempotency/receipt` 关键词盘点，并精读以下源码和审计材料。参考项目只提供行为启发，不复制源码、许可证、凭据格式或执行循环。

| 来源 | 观察到的可迁移模式 | Kiana 的落点 | 不照搬 |
|---|---|---|---|
| Codex Apps/Connectors | `chatgpt/src/connectors.rs` 将目录元数据、账号范围、缓存 key、可访问工具和 policy 状态分开；批量元数据读取只返回显示投影 | `ConnectorCatalog`、账号/项目绑定、按 account+backend+workspace 分桶的可重建缓存、policy 后过滤 | 后端 account 不作为 Kiana Principal；目录可见不等于可执行 |
| OpenCode Integration/Credential | `packages/schema/src/integration.ts` 将 OAuth/key/env 方法和连接状态建模；`credential.ts` 使用 tagged union；`specs/v2/provider-policy.md` 分离 provider config、credential 和 policy | `CredentialRef/Lease`、ConnectorDefinition、ProviderAccount、policy 独立评估 | 不把 raw key 放进 config/connection；插件不能改 policy；不提供 secret read-back |
| OpenCode MCP 配置 | local/remote、环境、超时、OAuth callback 和 enabled 是配置字段，不代表运行时授权 | `TransportConfig` 只描述候选；ControlPlane 决定是否能 connect/invoke | 当前 HTTP MCP 仍返回 unsupported；不因配置存在而自动连接 |
| Cline | `provider-auth.ts` 统一 provider ID/auth 规范化；MCP OAuth wizard 使用本地 callback、错误保留和可重试状态 | OAuth flow adapter、账号状态投影、显式 reauth | 不让 UI 保存或回传 token；回调不改变 assignment |
| OpenHands | `mcp-credential-validation.ts` 只调用只读 `whoami/list` probe，并区分 invalid credential 与 `missing_scope`；`redact-mcp-secrets.ts` 扫描 header/env/URL/JWT/token 形状 | `CredentialProbe`、健康等级、错误分类和全通道脱敏 | 不执行 destructive probe；未知/非 JSON 错误不删除凭据 |
| MCP Everything server | 初始化握手后按 client capability 注册条件工具；资源订阅按 session 隔离并在断开时清理 | connector/mcp session 生命周期、capability negotiation、disconnect cleanup | 条件注册只影响可见性，不绕过 Kiana policy；不把 session resource 当 durable artifact |
| A2A specification | Task 异步状态、messageId 幂等建议、push webhook 至少一次、事件顺序、`AUTH_REQUIRED` 和 out-of-band credential | 入站 webhook envelope、异步 connector operation、dedupe、事件 cursor 和 human approval | `AUTH_REQUIRED` 本身不授权；webhook 不能直接触发副作用；不把消息当可靠事实 |
| 12-Factor Agents | pause/resume、human input、外部 trigger 和结构化事件形成 outer loop | Connector trigger 进入 Workflow/Run，审批和恢复使用现有 ControlPlane | 不在 webhook handler 中运行模型循环 |
| Goose/Cline/Roo/Crush/OpenCode audits | 工具事务要有 validate→approval→execute→result；取消、重连、tool result 和持久化存在竞态 | Invocation reservation、stop report、result_unknown、receipt 投影 | 不把 UI timeline、内存 promise 或 transcript 当授权事实 |
| Temporal/LangGraph/Restate/DBOS | workflow 必须确定性重放；非确定性网络 I/O 放到独立 activity/effect，结果持久化后再推进 | `ConnectorInvocation`/attempt/effect observation；planner 只产生 intent | 不宣称跨系统 exactly-once；Unknown 不自动 retry |

补充的一手规范（用于实现时逐条核对，而非把外部规范当作 Kiana 现状）：[MCP transports](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports) 与 [MCP authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization) 规定 stdio/HTTP、会话和 OAuth 边界；[A2A specification](https://github.com/a2aproject/A2A/blob/main/docs/specification.md) 规定 Task、messageId 幂等建议、push webhook 和认证错误分类；[RFC 9700 OAuth 2.0 Security BCP](https://www.rfc-editor.org/rfc/rfc9700) 规定 PKCE、重定向和授权码注入防护；[Temporal retry guidance](https://github.com/temporalio/documentation/blob/main/docs/encyclopedia/retry-policies.mdx) 说明 workflow 确定性与 Activity 重试的边界。

## 3. 目标领域合同

### 3.1 核心对象

对象只序列化非秘密元数据。`Debug`、`Display`、错误、Event 和 Receipt 只能出现 opaque ref、digest、状态、时间和限制。

```text
ConnectorId / ConnectorVersion / OperationId / BindingId
ProviderAccountId / SecretRef / CredentialGeneration
InvocationId / AttemptId / IdempotencyKey / RegistryVersion
AuthorityEpoch / PolicyRevision / ConfigRevision / DataEpoch

ConnectorDefinition {
  schema, connector_id, version, provider_id, adapter,
  transport, endpoint_policy, data_processing,
  operations, auth_requirements, rate_limit, concurrency_limit,
  idempotency_required, reconciliation_required, content_hash
}

ConnectorOperation {
  operation_id, effect, risk, input_schema, output_schema,
  required_scopes, data_classes, idempotency_mode,
  timeout, retry_policy, effect_receipt_kind
}

AccountBinding {
  binding_id, connector_id, definition_version, provider_account_id,
  principal_owner, project_scope, secret_ref, scopes,
  endpoint_binding, status, expires_at, generation, revision
}

ConnectorInvocation {
  invocation_id, run_id, session_id, cell_id?, principal_id,
  binding_snapshot, operation_id, payload_digest, idempotency_key,
  authority_epoch, policy_revision, credential_generation,
  attempt, state, source_event_id
}

ProviderReceipt {
  schema, connector_id, binding_id, account_id, operation,
  idempotency_key, final_payload_sha256, provider_receipt_id,
  outcome, source, remote_status?, observed_at?, result, evidence_refs
}

ReconciliationCase {
  recovery_id, invocation_id, original_receipt_ref,
  query_or_evidence_ref, observed_outcome, owner, expires_at,
  safe_actions, forbidden_actions, status, decision_event_id
}
```

### 3.2 风险和操作合同

风险由 `ConnectorDefinition` 和 operation contract 服务端派生；调用方不能用参数把写操作降级成只读。

| 等级 | 示例 | 默认策略 | 当前状态 |
|---|---|---|---|
| R0 | list、inspect、schema、health | 只读，可查询 | `local_fixture` 已有部分实现 |
| R1 | whoami、list issues、read document | 只读外部数据；仍受 scope/data boundary | fake/read-only probe 优先 |
| R2 | 跨边界导出、批量读取、共享 artifact | 需要 data processing grant，通常要人工确认 | 未完成 |
| R3 | create/update/send/comment | 最终 payload 单次确认；一次 invocation 一个 idempotency key | local fixture 可模拟，真实 effect 未开放 |
| R4 | delete、支付、退款、生产发布 | 独立专项和 provider receipt；默认 deny | `not_supported` |

R0/R1 对应现有 `RiskLevel::ReadOnly`；R2 仍需由策略和数据边界单独授予（必要时提升到 `LocalWrite` 以保守拒绝）；R3 对应 `ExternalSideEffect`；R4 对应 `Critical` 且默认拒绝。R0–R4 是连接器专项的策略分层，不新增一套绕过 `kiana-domain` 风险枚举的执行路径。

每个 operation 必须定义输入/输出 schema、最大 payload、必需 scope、数据分类、是否产生 effect、是否支持查询回执、重试上限和取消语义。未声明的 operation、字段、scope 或 transport 均拒绝。

### 3.3 传输适配层

```text
ConnectorAdapter trait
  discover() -> metadata projection
  validate_binding() -> read-only health/evidence
  invoke(PreparedPermit, CredentialLease, CanonicalPayload)
  observe(ProviderReceiptRef) -> EffectObservation
  cancel(AttemptRef) -> StopReport
```

适配器能力必须显式登记：`read_only_probe`、`invoke`、`observe_receipt`、`cancel`、`webhook_verify`、`oauth_pkce`。缺少 `observe_receipt` 或 idempotency 的写操作只能返回 `Unknown`，不能被“网络成功”降级为 `Failed` 或 `Succeeded`。

首发 transport 分层：

1. `local_fixture`：项目内 hash 绑定的确定性 fixture，用于 contract/golden 测试；明确 `external_effect_performed=false`。
2. `stdio_mcp`：复用现有 MCP stdio 生命周期；server/tool schema 是不可信输入，调用仍回到 ControlPlane。
3. `https_oauth` / `https_api_key`：后续 opt-in；必须有 endpoint allowlist、TLS/redirect/代理约束、CredentialLease、provider receipt 和独立 live 证据。
4. `a2a_task`：后续异步任务适配；只映射 Task/Artifact/Status，不把远端 Agent 当作本地 shell 或普通函数。

任意原生插件代码、任意脚本、任意 HTTP MCP、支付/出行/IoT 和远程 worker 不因本专项自动开放。

## 4. 端到端处理流程

### 4.1 注册和账号绑定

```text
operator connector.manage/list
  → DaemonHost → ControlPlane authenticate + ProjectTrust
  → load immutable definition/version and verify hash/signature
  → list metadata or validate fixture/endpoint (read-only)

operator connector.manage/bind
  → validate definition + account + scope + data boundary
  → resolve SecretRef without exposing value
  → create ApprovalRequirement for binding mutation
  → CAS registry version and append connector.binding
  → project active binding and health
```

Definition 版本不可原地覆盖。Binding 的 `connector_id + definition_version + provider_account_id + project_scope + scopes + endpoint_digest` 形成不可变 snapshot；撤销、rotation、scope 变化都递增 revision/generation，使旧 snapshot、permit、approval 和 queued invocation 失效。

### 4.2 只读连接检查

```text
connector.health/check
  → resolve current binding and credential presence
  → authorize a read-only probe from the operation contract
  → issue short CredentialLease at effect boundary
  → call whoami/list/advertised capability only
  → classify verified | connectivity_only | credential_invalid |
    scope_insufficient | endpoint_unreachable | provider_error
  → redact response and persist health projection
```

“已配置”“能连通”“凭据有效”“拥有目标 scope”是四个不同结果。Probe 失败不删除凭据，也不改变 Kiana Principal 或 assignment。

### 4.3 一次出站调用

```text
CLI/Web/Workbench/Workflow/MCP tool request
  → normalized CommandIntent
  → server-derived actor/project/role/definition/binding/operation
  → canonical payload + schema/data-class validation
  → policy + scope + budget + rate/concurrency + approval
  → commit invocation.reserved + idempotency key + action digest
  → issue one-shot CredentialLease
  → recheck authority/config/binding/credential revision and endpoint
  → commit execution.prepared / permit
  → Broker invokes adapter exactly once for this attempt
  → normalize response to ProviderReceipt or ResultUnknown
  → append connector.invoked + usage + evidence before returning result
  → project Receipt and deliver bounded result to Harness/UI
```

`connector.invoke` 的 binding、account、risk、scope、endpoint 和 payload 都由服务端 snapshot 决定。模型、MCP server、网页或客户端不能切换账号、补 scope、指定低风险或直接提交 HTTP 请求。Adapter 不得从 wire payload 自行构造 Permit。

### 4.4 幂等、限流和重试

- Idempotency key 由调用方提供但由服务端绑定到 `project + binding + operation + canonical payload digest`；同 key 同 digest 返回原 receipt，冲突返回稳定 `idempotency_payload_mismatch`。
- Registry、binding、authority、policy、credential 和 data epoch 必须进入 invocation read-set；CAS 冲突要重新读取和重新授权，不能只替换 expected version。
- 限流至少按 connector、binding/account、operation 和 project 计数；计数事实在 EventStore，内存令牌桶只是加速层。
- 已知未 dispatch、纯读取且 adapter 声明幂等的错误可 bounded retry；请求已发出但无 receipt、取消未确认、scope/epoch/approval 变化和非幂等写操作禁止自动 retry。
- 每次 retry 新建 `AttemptId` 和 permit，但保留 Invocation/action digest 关联；旧 attempt 事件不可修改。

### 4.5 ProviderReceipt、Unknown 和对账

```text
adapter response / timeout / process crash
  → classify known success | known failure | result_unknown
  → known result: append connector.invoked with receipt
  → unknown: append connector.invoked(outcome=unknown), open RecoveryCase,
             fence binding/lease and expose Human Inbox
  → explicit reconcile command
  → verify provider receipt/query/manual evidence ownership, audience,
    connector/binding/account/operation/key/payload digest
  → append connector.reconciled; never rewrite original invocation
```

没有 provider query、幂等语义或可信 receipt 的外部写操作只能永久 `Unknown` 或等待人工证据。`RunReceipt.completed` 只证明本机事实链完成，不能证明 GitHub/Jira/Slack/Notion 的业务 outcome。

### 4.6 入站 Webhook 和 A2A 事件

```text
HTTP/stdio/A2A ingress
  → transport authentication + source allowlist + timestamp/signature
  → size/schema/version validation + replay nonce/dedupe
  → map payload to typed InputArtifact with provenance
  → commit connector.event.received / trigger occurrence
  → Workflow/Run admission
  → normal Harness → ControlPlane → Broker path
```

Webhook handler 只落事实和排队命令，不执行模型循环或 capability。签名 key、tenant/account、source connector、event id、received cursor 和 payload digest 必须绑定。事件重复返回原 occurrence；事件 payload 只能填充 workflow input，不能成为未经 schema/policy 检查的 capability 参数。A2A `AUTH_REQUIRED`、push notification 和异步 Task 状态都要映射为可恢复的等待/事件，不自动授予后续操作权限。

### 4.7 撤销、轮换、取消和重启

```text
revoke/rotate binding or credential
  → CAS generation/authority epoch
  → fence queued permits and invalidate leases
  → ask started adapter to stop when supported
  → stop confirmed: cancelled; otherwise Unknown + RecoveryCase
  → preserve original facts and receipt refs

daemon restart
  → rebuild registry/invocation/receipt/recovery projections
  → fence stale workers and leases
  → default unresolved connector attempts to Paused/NeedsRecovery
  → explicit reconcile/resume/retry_without_effect re-authorizes current snapshot
```

## 5. 事件、查询和数据治理

建议的事实事件：

| 事件 | 关键字段 |
|---|---|
| `connector.definition_registered` | definition id/version/hash, adapter, capabilities |
| `connector.binding` | binding snapshot, registry revision, operator, approval id |
| `connector.binding_revoked` | binding id, prior revision, reason, authority epoch |
| `connector.health_checked` | probe kind, status, checked_at, redacted error |
| `connector.event_received` | source, event id, signature key id, payload digest, occurrence id |
| `connector.invocation_reserved` | invocation/attempt, operation, action/payload digest, read-set |
| `connector.invoked` | ProviderReceipt, effect_known, evidence refs, attempt |
| `connector.reconciled` | original invocation, observation/evidence digest, decision |
| `connector.cancel_requested` / `connector.stop_observed` | generation, stop report, effect status |

Receipt/UI 查询必须带 `source_cursor`、`projection_version`、`binding_revision`、`credential_generation`、`data_epoch`、`stale`、`proof_level` 和 `limitations`。Secret、Authorization header、OAuth code/token、完整 payload 和 provider raw error 只能进受控 Artifact/SecretStore，不进入 EventLog、Receipt、Memory、UI、stdout/stderr、argv、env 或 cache。

Connector 输入、输出和缓存都要带 `DataClass`、`Purpose`、owner project、source ref、retention 和 revocation epoch。跨项目共享必须有显式 `SharingGrant`；撤销或删除先写 tombstone、提升 `data_epoch`，再失效 Memory/Index/Cache/Artifact 引用。

## 6. 代码落点和迁移顺序

| 层 | 代码落点 | 责任 |
|---|---|---|
| Domain | `kiana-domain/src/connectors.rs`、`capabilities.rs`、`errors.rs`、`governance.rs` | typed IDs、definition/operation/binding/invocation/receipt/recovery、纯状态转移和 canonical digest |
| Protocol | `kiana-protocol/src/lib.rs`、schema fixtures | `connector.manage/invoke/health/reconcile` DTO、version、错误和 UI event；不接受 caller authority 自声明 |
| Ports | `kiana-ports/src/lib.rs`、`model.rs` | `ConnectorRegistryPort`、`ConnectorAdapter`、`CredentialProbe`、`EffectObserver`、`WebhookVerifier` 窄接口 |
| Core | `kiana-core/src/connectors.rs`、`capabilities.rs`、`approvals.rs`、`recovery.rs` | normalize、policy/gate/approval、reservation、CAS、fence、receipt/reconcile command |
| Broker | `kiana-capability-broker/src/lib.rs` | 只消费 prepared permit；校验 invocation/action/epoch/scope，调用 adapter 并返回结构化 observation |
| Daemon | `kiana-daemon/src/connectors.rs`、`mcp_stdio.rs`、`execution_control.rs` | registry projection、fixture/MCP/native adapters、credential lease 生命周期、bounded worker、shutdown |
| Event/Data | `kiana-eventlog`、`kiana-query` | facts、command dedup、projection、health、receipt、index/cache rebuild；不做第二事实源 |
| Entrypoints/UI | `kiana-entrypoints/src/{cli,web,workbench_chat,product_command}.rs` | 只读 list/health/inbox/reconcile/approval 展示和命令转发，不直接调用 adapter |
| Tests/Scripts | crate integration tests、`scripts/`、fixtures | deny-first、conformance、故障注入、live opt-in 和 evidence block |

迁移分四个门：

1. **Local contract gate**：完成 fixture、schema、idempotency、receipt/reconcile 和所有 deny tests；不改变外部网络状态。
2. **Read-only transport gate**：先支持隔离测试账号的 `whoami/list` 和健康检查；必须有 endpoint/credential/data boundary 证据。
3. **Bounded write gate**：每个 connector 单独实现 final payload approval、provider idempotency、receipt/query、cancel/unknown runbook；默认关闭。
4. **Live/physical gate**：按 connector、account、operation、环境单独 opt-in；真实效果、回滚/补偿和清理证据分别记录，不能用 fixture 或 mock receipt 代替。

## 7. 详细实施步骤（INT-00–INT-33）

每一步都按“先拒绝、再成功、最后回归”执行；表中的测试名是待实现的验收意图，不是现有证据。

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功/回归验收 |
|---|---|---|---|---|
| <a id="step-int-00"></a>`INT-00` | 基线、现状和能力矩阵；`docs/`、`CURRENT_STATUS.md`、现有 connector tests | — | 识别第二执行循环、直达 Broker、raw secret 和外部默认网络；发现即阻断 | 记录 source snapshot、fixture、旧命令兼容边界和 proof 限制 |
| <a id="step-int-01"></a>`INT-01` | 固定 Provider/Connector/MCP/A2A/Notification 术语和边界；`docs`、module map | INT-00 | MCP tool list、A2A message 或 provider account 被当成授权证据 | 每种入口都能映射到唯一 ControlPlane path |
| <a id="step-int-02"></a>`INT-02` | Domain typed IDs、definition/binding/invocation/receipt/recovery 合同 | INT-01 | 空/跨类型 ID、unknown field、非法状态、secret 出现在 Debug/serde | schema round-trip、canonical digest 和合法转移测试 |
| <a id="step-int-03"></a>`INT-03` | Operation input/output schema、风险、scope、data class、retry/timeout 合同 | INT-02 | caller 降级 risk、未声明 field/operation/scope、超限 payload | 同一 canonical payload 产生稳定 action/payload digest |
| <a id="step-int-04"></a>`INT-04` | Connector registry immutable version、hash/signature、CAS、catalog projection | INT-02/03 | 原地覆盖版本、hash/signature 不符、registry race、未知 adapter 自动启用 | list/inspect 可从事实重建；同版本内容唯一 |
| <a id="step-int-05"></a>`INT-05` | AccountBinding、ProviderAccount、project/owner/data boundary、scope intersection | INT-02/04 | 跨项目无 grant、binding owner 伪造、scope 超集、revoked/expired binding | binding snapshot 固化并可审计；撤销递增 revision |
| <a id="step-int-06"></a>`INT-06` | SecretRef/CredentialLease 与 connector invocation 绑定；复用 CI-07 | INT-05 | raw token 到 Core/Runner/Event/UI、调用绑定错配、wrong purpose/audience/endpoint、lease replay/expiry | server-owned invocation envelope 锁定 connector/version/binding revision/account/operation/idempotency/SecretRef generation/effect target；Broker 在 adapter effect boundary 重验并一次消费，结果只投影 digest/ref metadata |
| <a id="step-int-07"></a>`INT-07` | `ConnectorAdapter`、`EffectObserver`、`CredentialProbe`、`WebhookVerifier` ports | INT-02/06 | adapter 直接访问 EventStore/approval、port 返回 raw secret、缺能力却宣称支持 | fake adapter 可注入 known/unknown/stop/health 结果 |
| <a id="step-int-08"></a>`INT-08` | local_fixture schema、hash、payload matching、deterministic receipts | INT-04/07 | fixture 越界/替换/重复 payload、未知 operation、external_effect=true | fixture bind/invoke/replay/reconcile 与现有实现一致 |
| <a id="step-int-09"></a>`INT-09` | read-only health/probe 和状态分类；daemon/query/UI | INT-05/07/08 | destructive probe、scope error 被标为 credential invalid、非 JSON error 删除凭据 | verified/connectivity-only/invalid/scope/unreachable 可区分且脱敏 |
| <a id="step-int-10"></a>`INT-10` | stdio MCP connector adapter 与 capability handshake | INT-07/09 | HTTP/remote MCP 绕过 unsupported、server tool schema 直接授予 scope、session 泄漏 | stdio startup/timeout/disconnect/conditional tool 可回收和审计 |
| <a id="step-int-11"></a>`INT-11` | HTTPS endpoint、TLS、redirect、proxy、DNS/SSRF/egress allowlist | INT-06/07 | URL userinfo、非 HTTPS、跨 origin redirect、内网地址、代理泄漏均 0 dispatch | fake HTTPS transport 只连 pinned origin，错误有结构化分类；[baseline](int11-https-connector-baseline.md) |
| <a id="step-int-12"></a>`INT-12` | OAuth PKCE/state/callback、account store、refresh single-flight | INT-06/11 | state/redirect mismatch、code reuse、scope downgrade、旧 refresh 覆盖新 generation | 提前刷新、CAS rotate、reauth/revoked 和锁内重读测试；[baseline](int12-oauth-account-baseline.md) |
| <a id="step-int-13"></a>`INT-13` | secret redaction/echo sentinel 扫描；domain/daemon/event/UI tests | INT-06/08/12 | token 出现在 prompt/transcript/event/receipt/stdout/stderr/argv/env/cache | provider raw error、URL/header/JWT 形状和 fixture 结果均安全投影 |
| <a id="step-int-14"></a>`INT-14` | `connector.manage/invoke/health/reconcile` protocol DTO 和 normalize | INT-03/05/07 | wire actor/role/risk/binding/endpoint 覆盖服务端值；未 trust/未 auth 0 Broker calls | CLI/Web/Workbench/MCP 得到同一 normalized intent · [baseline](int14-connector-protocol-baseline.md) |
| <a id="step-int-15"></a>`INT-15` | operation risk→policy/gate/approval 映射 | INT-03/14 | R3 无 final payload approval、R4 被默认放行、拒绝后仍 dispatch | R0/R1 只读、R2 data grant、R3 once approval 结果稳定 · [baseline](int15-connector-policy-baseline.md) |
| <a id="step-int-16"></a>`INT-16` | invocation reservation、command digest、idempotency/CAS | INT-04/05/14/15 | 同 key 不同 digest、revision race、未提交 reservation、旧 permit 均 0 effect | replay 返回原 receipt；一个 attempt 只消费一个 permit · [baseline](int16-connector-reservation-baseline.md) |
| <a id="step-int-17"></a>`INT-17` | connector/account/project rate、concurrency、budget reservation | INT-16 | 内存计数重启归零、超额无稳定错误、并发越限仍 dispatch | 多 worker 竞争只有一个 claim；配额结算可重放 · [baseline](int17-connector-quota-baseline.md) |
| <a id="step-int-18"></a>`INT-18` | effect-time permit、authority/config/policy/credential/data epoch fencing | INT-06/15/16 | admission 后撤销/轮换/配置漂移仍调用 adapter | permit 可验证 scope、digest、epoch、expiry，旧 snapshot 被拒 |
| <a id="step-int-19"></a>`INT-19` | Broker dispatch 和 adapter observation 分离 | INT-07/16/18 | connector handler 自己授权/写 EventLog、commit 后重复 effect、late result resurrect | prepared→dispatching→observation→result committed 可重放 |
| <a id="step-int-20"></a>`INT-20` | ProviderReceipt/EffectObservation schema、owner/audience、payload hash | INT-08/19 | receipt 属于其他 binding/account/operation/key、raw response 泄漏 | succeeded/failed/unknown 和 evidence refs 稳定投影 |
| <a id="step-int-21"></a>`INT-21` | retry classifier、attempt、timeout/backoff、idempotency policy | INT-16/20 | Unknown、非幂等写、审批/epoch/scope 错误自动 retry | 仅 known no-effect/declared-idempotent 按 bounded policy 新 attempt |
| <a id="step-int-22"></a>`INT-22` | Unknown quarantine、ReconciliationCase、provider query/manual evidence | INT-20/21 | timeout 映射 failed、没有 receipt 直接 retry、reconcile 覆盖原事件 | explicit reconcile 追加事实；安全/禁止动作进入 Human Inbox |
| <a id="step-int-23"></a>`INT-23` | cancel/stop report/late result fence、lease settlement | INT-18/19/22 | 未 stop 却 cancelled、started effect 被释放锁、late result resurrect | not_executed、stop_confirmed、Unknown 三类结果可重建 |
| <a id="step-int-24"></a>`INT-24` | webhook/A2A ingress auth、signature、timestamp、nonce、dedupe | INT-01/04/07/22 | 未认证、重放、错误 tenant/source、超限 payload、注入字段均 0 Broker calls | event→occurrence→Workflow/Run 可重放，签名证据可查询 |
| <a id="step-int-25"></a>`INT-25` | object mapping、input artifact、schema/provenance、pagination cursor | INT-03/09/24 | 外部字段直接成为 capability 参数、分页 cursor 越权、隐式跨账号 | 映射可版本化、可审计、输入输出顺序稳定 |
| <a id="step-int-26"></a>`INT-26` | DataClass/Purpose/SharingGrant/retention/revocation propagation | INT-05/22/25 | connector 结果进入无 scope Memory/Index、撤销后 cache 继续返回 | tombstone/data epoch 使所有派生视图失效 |
| <a id="step-int-27"></a>`INT-27` | Connector health/invocation/reconcile/approval 的通知投影 | INT-20/22/26 | 通知被当事实、重复投递推进两次、消息带 secret | 至少一次投递可去重，UI 显示 source cursor/evidence/limitation |
| <a id="step-int-28"></a>`INT-28` | CLI/Web/Workbench/MCP 查询与人工 reconcile UI | INT-14/22/27 | UI 自带 actor/approval/context、查询消费 lease、跨项目枚举 binding | 四入口读取同一 projection，错误和状态一致 |
| <a id="step-int-29"></a>`INT-29` | restart/recovery、projection rebuild、stale worker/lease fencing | INT-16/19/22/23/26 | 重启自动 resume Unknown、损坏 journal 当空库、旧 approval 复活 | 默认 Paused/NeedsRecovery；显式新命令重新 admission |
| <a id="step-int-30"></a>`INT-30` | adapter/registry/protocol conformance 和 property tests | INT-02..29 | 只有单元类型测试、没有 deny/TOCTOU/replay/unknown 证据 | fake fixture、MCP fake、HTTP fake 共享同一 conformance |
| <a id="step-int-31"></a>`INT-31` | 一个只读外部 connector pilot（隔离账号，默认关闭） | INT-09/11/12/20/30 | 缺 endpoint/credential/receipt/revocation/cleanup 证据不得启用 | whoami/list live proof 单独绑定环境、scope、版本和限制 |
| <a id="step-int-32"></a>`INT-32` | 一个受控写 connector pilot（每 operation 独立） | INT-15/16/18/20/21/22/23/31 | 无 provider idempotency/receipt/query、无 cancel/compensation、R3/R4 混用即阻断 | 单次 final payload、receipt、对账、失败/Unknown runbook 完整 |
| <a id="step-int-33"></a>`INT-33` | 发布门、live/physical 证据和 `CURRENT_STATUS` 收口 | INT-00..32 | fixture/mock/历史 CI 冒充 live；secret、越权、第二 loop、外部默认网络均阻断 | 每个 connector/operation/account 有 evidence block、feature/proof/limitations 和回滚记录 |

## 8. 依赖波次、验收矩阵和证据

```text
Wave A: INT-00 → INT-01 → INT-02 → INT-03 → INT-04
Wave B: INT-05 → INT-06 → INT-07 → INT-08 → INT-09
Wave C: INT-10 ∥ INT-11 → INT-12 → INT-13
Wave D: INT-14 → INT-15 → INT-16 → INT-17 → INT-18
Wave E: INT-19 → INT-20 → INT-21 ∥ INT-23 → INT-22
Wave F: INT-24 → INT-25 → INT-26 → INT-27 → INT-28
Wave G: INT-29 → INT-30 → INT-31 → INT-32 → INT-33
```

先拒绝矩阵至少覆盖：未认证、未信任项目、伪造 actor/role/binding/risk、definition 未验签、unknown schema、scope 超集、跨项目无 grant、secret 泄漏、URL/redirect/SSRF/代理、错误 OAuth state、MCP HTTP、重复 key 不同 payload、重复 webhook、rate/concurrency 超限、旧 epoch/revision/permit、过期 approval/lease、Unknown 自动 retry、cancel 未 stop、late result、损坏 journal、撤销后 cache 命中和四入口分叉。每个拒绝都要证明 Broker/adapter effect 为零。

成功/恢复矩阵至少覆盖：local fixture、stdio MCP、read-only probe、PKCE refresh、known failure retry、idempotent replay、R3 final payload approval、receipt/query reconciliation、webhook/A2A occurrence、rate limit、cancel/stop、daemon restart、projection rebuild、data revocation、CLI/Web/Workbench/MCP 同一 snapshot。真实外部服务只能按 connector/operation/account 单独 opt-in。

每个 `INT-*` 完成时写入：

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

`feature_status` 与 `proof_level` 分开填写。存在 `ConnectorDefinition`、通过 fixture 测试或生成 `ProviderReceipt` 都不能单独把状态写成 `implemented/live/physical`。外部 effect exactly-once、真实业务 outcome、支付/退款成功和跨系统回滚必须有独立 provider receipt、对账和环境证据。
