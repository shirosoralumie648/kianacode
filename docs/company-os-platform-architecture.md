# Kiana CompanyOS 平台能力架构

> 文档性质：平台能力总规范（Normative Target）。
>
> 本文补齐 CompanyOS 中除业务领域和控制面之外的平台能力：Agent Runtime、Context/Memory、Capability Discovery/MCP、Workflow、Swarm、Provider、Cache 和 Observability。
>
> 运营与长期运行保障见 [`company-os-operations-governance.md`](company-os-operations-governance.md)；质量、评测和扩展生态见 [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md)。
>
> 当前能力与证据等级以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。本文中的目标架构、参考项目和接口形状，不代表当前 checkout 已经实现。

> **本文速览（导读，非规范）**
>
> - **讲什么**：Agent 平台的九个"平面"——运行时（Session/Turn/Run/Invocation，含 pre-tool hook 位置与合法转移）、上下文与记忆（ContextPlan/Memory，含 promotion/expiry/collection）、缓存与压缩（Prompt Cache/Compaction）、工具发现（CapabilityDescriptor/Tool Search/MCP，含失败恢复与 trust decision）、工作流（Workflow 确定性合同与 replay divergence）、有界多 Agent（Swarm，含 Partition/MergeDecision）、Provider 网关（NormalizedEvent 与客户端 epoch）、可观测与评测——每个平面的对象合同、硬性不变量和参考项目取舍。
> - **回答的问题**："模型每次应该看到什么、工具怎么被发现和授权、并行怎么不失控、不同模型服务商的输出怎么统一、失败与重放凭什么可复现。"
> - **什么时候读**：实现运行时、记忆、MCP、工作流、Swarm 相关能力时按章节查阅；§3 的十条硬不变量值得所有人先通读一遍；规范与代码的已知冲突集中在 §16 开放决策。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 为什么需要平台能力层

CompanyOS 不能只有：

```text
ControlPlane → Policy → Broker → Handler
```

控制面只能回答：

> 这个请求是否有资格执行？

完整平台还必须回答：

- 模型当前应该看到哪些上下文；
- 哪些历史应该成为长期记忆；
- 如何在大量工具中发现最小候选集；
- 一个任务应该按什么确定性流程推进；
- 什么时候可以分裂成多个执行 Cell；
- Provider 的不同流格式如何归一化；
- 长对话如何压缩、恢复和复用缓存；
- 运行失败、取消、恢复和成本如何被观测。

因此 CompanyOS 采用以下平台分层：

```text
Company Domain
  Objective / Project / Acceptance / Outcome
                 │
Governance / Control Plane
  identity / policy / gate / approval / budget / ownership
                 │
Platform Planes
  ┌──────────────┬───────────────┬────────────────┐
  │ Runtime      │ Context/Memory│ Capability     │
  │ Session/Run  │ RAG/Compaction│ Catalog/MCP    │
  └──────┬───────┴───────┬───────┴──────┬─────────┘
         │               │              │
  Workflow Engine  Swarm Runtime  Provider Gateway
         │               │              │
         └───────────────┴──────────────┘
                         │
              Event Store / Receipt / Eval
                         │
                 CLI / Web / Desktop / SDK
```

## 2. 九个层面及其职责

| 平面 | 核心问题 | canonical 合同 | 推荐实现位置 | 当前判断 |
|---|---|---|---|---|
| Company Domain | 为什么做、交付什么、结果是否实现 | Objective、Project、Acceptance、Outcome | `kiana-domain` | 目标合同已补，运行时未完整 |
| Control Plane | 谁能做什么 | identity、policy、gate、approval、grant | `kiana-core` | 主路径已有局部实现 |
| Agent Runtime | 一次执行如何运行、暂停和恢复 | Session、Turn、Run、Invocation | `kiana-runner` + `kiana-eventlog` | 单机局部实现，耐久恢复未完成 |
| Context / Memory | 模型应该知道什么 | ContextPlan、ContextCheckpoint、MemoryRecord、MemoryQuery | `kiana-query` + ports | 有索引、ACL、budget 片段，生命周期未闭合 |
| Capability Discovery | 如何发现可用工具 | CapabilityDescriptor、ToolSnapshot | broker + discovery service | 工具存在，统一 catalog 仍需建立 |
| Workflow | 按什么顺序完成工作 | WorkflowDefinition、NodeExecution、Signal | `kiana-workflow` + `kiana-core` | 需要收敛为同核确定性引擎 |
| Swarm | 何时有界并行、如何合并 | SpawnPlan、Cell、Partition、MergeDecision | `kiana-core` + `kiana-daemon` | Cell/Packet 有合同，完整 scheduler 未完成 |
| Provider / Output | 如何统一模型和客户端事件 | NormalizedEvent、Cursor、Usage | `kiana-daemon` / protocol | 当前不宣称 live/token streaming |
| Evidence / Observability | 如何证明、计量和复盘 | RuntimeEvent、Receipt、Trace、Eval | `kiana-eventlog` + query | Receipt/EventLog 有局部实现 |

这些平面共享 `organization_id`、`project_id`、`session_id`、`run_id`、`turn_id`、`invocation_id` 和 `correlation_id`，但每个平面必须有自己的 owner。不能因为它们都围绕 Agent 就都实现成 `kiana-core` 中的可变全局状态。

## 3. 平面之间的硬不变量

1. **单一执行脊柱**：所有入口都经过 `DaemonHost → ControlPlane`；不能在 Workflow、MCP、Workbench 或 Swarm 中另起一个绕过控制面的模型循环。
2. **发现不等于授权**：工具搜索、语义检索、Skill、MCP server 描述和模型文本都不能授予 Grant。
3. **记忆不等于事实**：检索结果必须带 provenance、scope、authority 和 freshness；低置信度记忆不能覆盖事件事实。
4. **缓存不等于持久化**：Prompt Cache 命中不能证明 Session、Approval、Invocation 或 Receipt 已保存。
5. **Workflow 是软件**：模型可以提出动作，但合法状态转移、重试、补偿和终止由确定性引擎执行。
6. **Swarm 是有界执行**：所有 fan-out 都必须有 parent、partition、预算、并发、深度、TTL、合并规则和 retire 规则。
7. **Provider 事件先归一化**：Provider 特有的 delta、tool call 和 stop reason 不能直接驱动 CompanyOS 状态机。
8. **事实先于展示**：先 append/commit canonical event，再发布 UI 或 SSE；有损传输必须有 cursor 和 terminal event。
9. **所有动态输入不可信**：网页、MCP 返回值、工具描述、Artifact 和 Memory 都不能改变 role、grant、budget 或 policy。
10. **证据上限诚实**：没有 deterministic fixture、源码快照和真实 adapter 证据时，只能宣称 `local_behavior`。

## 4. 统一运行时合同

### 4.1 Session、Turn、Run、Invocation

```text
Session  长期会话容器，绑定 owner、workspace、project 和 lineage
Turn     一次用户输入到终止/暂停的交互边界
Run      Turn 的一次执行生命周期，可以排队、运行、暂停、取消或恢复
Invocation 一次可恢复的工具/能力请求，拥有稳定 call identity
```

目标关系：

```text
Session 1 ── * Turn 1 ── * Run 1 ── * Invocation
                         └── * ContextCheckpoint
```

每个 Invocation 必须拥有：

```text
invocation_id
run_id / turn_id / session_id
capability_id + capability_version
canonical_input_digest
policy_snapshot
approval_ref?
attempt
status
result_ref?
causation_id / correlation_id
```

禁止使用“当前活动 session”或共享全局 run 作为请求目标。每个入口必须显式携带并校验 session/run ownership。

### 4.2 规范运行循环

```text
Ingress
  → Load Session/Run state
  → Assemble ContextPlan
  → Provider request
  → Normalize provider events
  → Persist model output / tool calls
  → Validate tool arguments
  → Policy / Gate verdict
  → Pre-tool hook（仅前序 Allowed 后执行）
  → Approval（策略或 hook Ask 要求时）
  → Broker dispatch
  → Persist tool result and evidence
  → Re-assemble next context
  → Continue / Compact / Retry / Pause / Finalize
  → Publish receipt and client events
```

终态至少区分：

```text
completed       有最终回答且所有必要事实已保存
paused          等待用户、Approval 或外部 Signal
failed          明确失败且无未确认副作用
cancelled       取消已安全收敛
result_unknown  可能已有副作用，但事实尚未确认
```

Pre-tool hook 合同（新增规范）：

- **位置**：hook 只在 Policy/Gate 判定为 `Allowed` 之后、Broker dispatch 之前执行；Policy/Gate 已拒绝的请求不进入 hook。
- **与 Policy verdict 的关系**：hook 只能收紧，不能放宽——`Allow` 不是授权，hook 不得把 Deny 改写为 Allow，也不得改写请求以扩大 capability、path、budget 或 approval scope；需要改变输入时按新请求重新走完整 verdict。
- **Block 的终态**：hook 返回 `Block`，或 hook 执行失败（端口错误、配置损坏）时，本次 Invocation 以 `failed` 收敛，稳定错误码 `hook_blocked:<reason>`；因为尚未 dispatch、无副作用，不得记为 `result_unknown`，Run 也不得 `completed`。
- **Ask 的归宿**：hook 返回 `Ask` 时必须进入与 [`company-os-design.md`](company-os-design.md) §10.1 相同的 ApprovalChallenge/PendingInvocation 通路；审批通路不可用或无人值守时 fail-closed 为 `failed`（`hook_ask_unattended:<reason>`），不得静默放行（当前实现差距见 §16）。
- **可观测**：hook 决策（decision、reason、hook/policy 版本）必须写入事件，Receipt 必须能回答“哪个 hook 因什么阻断了本次调用”。
- **局部证据**：`CURRENT_STATUS.md` 的 G0-02 slice evidence（`PreToolHookPort` 注入 daemon）与 P1-02 证据块；`docs/coding-pack-matrix.md` P0-HOOK 行（`hook_blocked` 可在 broker 执行前阻断）。这些证据只覆盖已实现部分，不覆盖 Ask→Approval 通路。

### 4.3 参考项目与吸收决策

| 参考项目 | 值得吸收的设计 | 不直接复制的部分 |
|---|---|---|
| DeepSeek Harness | step boundary、事件追加、工具结果按顺序回灌、恢复接口 | 不把 TypeScript runtime 直接搬入 Rust |
| OpenCode | event projector、按 session 串行化、hydration 合并 | 不把 UI event 当唯一事实源 |
| Cline | Local Runtime Host、SDK/IDE/CLI 共用 host、abort 队列 | 不复制多层兼容 runtime |
| Goose | effect-before-event、状态机 checkpoint、按步骤恢复 | 不保留长期并行 legacy loop |
| Crush | RunID 关联、accepted/active/queued cancel、terminal event 必达 | 不把协作式取消误称为副作用已停止 |
| Agno / Agent Framework | 可序列化 requirements、HITL resume、checkpoint/fork | 不依赖不透明 callback 保存状态 |
| 12-factor-agents | 小型 reducer/event loop、限制 Agent 步数 | 不复制无限 `while(true)` |

这些参考的统一结论和证据索引见 [`reference-agent-audit/00-unified-agent-flow.md`](reference-agent-audit/00-unified-agent-flow.md) 与 [`reference-agent-audit/99-kiana-mapping.md`](reference-agent-audit/99-kiana-mapping.md)。

### 4.4 Run 与 Invocation 的合法转移（新增）

**基数**：`Session 1 ── * Turn 1 ── * Run 1 ── * Invocation`（§4.1）；`Invocation 1 ── * CapabilityExecution`——Invocation 是可恢复的逻辑调用（稳定 `invocation_id`），每次重试/重派产生一个新的 `CapabilityExecution` attempt（`execution_id` + 递增 `attempt`）。`invocation_id` 与 `execution_id` 不得混用（[`company-os-design.md`](company-os-design.md) §5.2）。

**Run 合法转移**：

```text
queued           → running | cancel_requested
running          → paused | cancel_requested | completed | failed | result_unknown
paused           → running | cancel_requested
cancel_requested → cancelled | result_unknown
```

- 只有 `running` 可以进入 `completed` / `failed` / `result_unknown`；终态无隐式出边。
- `cancel_requested` 是唯一的取消中间态 wire 名；`cancelling` 只是 UI 投影名，不是终态。取消的终态是 `cancelled`，若副作用无法确认则是 `result_unknown`（design §10.2）。
- `paused` 的恢复是同一 Run 的 continuation，不是新 Run；retry 产生新 attempt 或新 Run，不复活终态 Run。
- `result_unknown` 语义：可能已有副作用、事实未确认；禁止自动重试，也禁止隐式转 `completed`（design §9.4）。它没有自动出边；只能由显式 reconciliation 依据新证据产生 correction event，或由新的授权发起补偿/重试（那是新的 Run/Invocation，不改变原 Run 的终态）。

**Invocation.status 与 `company-os-design.md` §9.3 CapabilityExecution 的映射**：两者是同一状态机的逻辑调用/具体尝试两层，不是两套枚举；状态名与转移 canonical owner 是 design §9.3。

```text
Invocation.status（投影）   CapabilityExecution（attempt）
Requested                   Requested
PolicyChecked               PolicyChecked
AwaitingApproval            AwaitingApproval
Authorized                  Authorized
Dispatching                 Dispatching
Executing                   Executing
Succeeded                   Succeeded
Failed                      Failed
Cancelled                   Cancelled
result_unknown              design §9.3 的 Unknown（wire/终态名以 result_unknown 为准）
```

- Invocation 的终态取最近一次 attempt 的终态；只要存在一个副作用未确认的 attempt，Invocation 必须为 `result_unknown`，不得报 `Succeeded` / `Failed` / `Cancelled`。
- `attempt` 单调递增，旧 attempt 的事件不可改写；重试必须携带新的 `idempotency_key` 或复用同一幂等键，按 capability 的 `idempotency_policy` 决定。

## 5. Context / Memory 平面

### 5.1 ContextPlan

ContextPlan 是一次 Provider request 的可审计上下文计划，不是一个随意拼接的字符串。

```text
ContextPlan {
  session_id
  run_id
  turn_id
  system_baseline_ref
  role_prompt_ref
  policy_summary_ref
  capability_catalog_ref
  project_snapshot_ref?
  work_packet_ref?
  memory_query_plan
  history_cursor
  checkpoint_ref?
  token_budget
  sections[]
  provenance[]
  serialization_version
}
```

Section 清单（本节只登记“必须有哪些 section”，不定义渲染顺序）：

- system / security constitution；
- role / AgentTemplate prompt；
- capability catalog 摘要；
- project / workspace / repo map 快照；
- 历史压缩摘要与 checkpoint；
- 当前 WorkPacket、acceptance 和约束；
- 按 ACL 检索的 Memory 结果；
- 当前工具结果、时间敏感状态和用户输入。

**渲染顺序以 §6.2 的 cache 友好序为准**（tools / capability catalog → stable system / security constitution → role prompt → …）；本节不再维护第二份顺序。

身份、Policy、Grant 和 Approval 由服务端强制；ContextPlan 中的文本只是模型输入，不能作为安全边界。

### 5.2 Token Budget

每次 request 应划分预算：

```text
context_budget
  = stable_system_budget
  + role_budget
  + tool_catalog_budget
  + project_budget
  + history_budget
  + memory_budget
  + live_result_budget
  + user_input_budget
  + output_reserve
```

预算不足时按顺序处理：

1. 删除重复和低 authority 的检索片段；
2. 清除旧的低价值 tool result；
3. 使用带 cursor 和 provenance 的 compaction；
4. 降低工具候选 schema 数量；
5. 明确返回 context budget exceeded，而不是静默截断关键约束。

### 5.3 Memory 类型与六层 ACL

内容类型和访问层是两个维度：

```text
类型：Working / Episodic / Semantic / Procedural / Artifact / Preference
层级：Company / Department / Role / Project / User / InstanceScratch
```

MemoryRecord 至少需要：

```text
memory_id
kind
layer / collection
subject
content_or_artifact_ref
source_ref / source_event_cursor
writer_principal
authority
confidence
sensitivity
purpose
created_at / last_verified_at
expires_at?
supersedes?
status: candidate | active | stale | revoked | deleted
```

写入流程：

```text
Candidate
  → classify
  → redact secrets/PII
  → validate source and scope
  → deduplicate / supersede
  → policy decision
  → append MemoryWritten
  → optional promote
  → verify / expire / revoke / delete
```

检索流程：

```text
Intent parse
  → purpose + ACL filter
  → exact metadata/identifier lookup
  → lexical/BM25 search
  → vector/semantic search
  → optional graph expansion
  → authority + freshness + recency ranking
  → deduplicate / MMR
  → token pack
  → provenance citation
```

向量相似度只能决定候选排序，不能决定授权、事实优先级或 Memory promotion。

**Promotion（新增规范）**：

```text
promotion 是跨层/跨 collection 的显式、可审计跃迁，不是检索排序的结果。
```

- **授权主体**：由目标层的 owner（HumanPrincipal）或该层 policy 明确授权的 approver 批准；写入者不得自批。`Project/User/Role → Department/Company` 的晋升必须由目标层授权主体批准，跨组织晋升默认禁止。
- **门槛（全部满足才可晋升）**：`source_ref` 可验证且未被 revoke；`confidence` ≥ 目标层阈值；`sensitivity` 允许进入目标层；`purpose` 与目标层用途匹配；未过期；去重/冲突检查通过。敏感数据与 PII 默认不晋升，除非存在显式 `ProcessingGrant`（operations §9.2）。
- **记录**：append `PromotionDecision { memory_id, from_layer, to_layer, target_collection, approver, policy_snapshot, evidence_refs }`，并产生新的 `MemoryRecord` 版本（`supersedes` 原记录），不原地改写；原记录保留 provenance。
- **禁止**：promotion 扩大 ACL；低 `authority` 覆盖高 `authority`；扩展把私有 Memory 晋升到 Company 层（quality §9.2）；把 promotion 当作授权或事实优先级来源。

**Expiry 与清理责任（新增规范）**：

- `expires_at` 到达：`status: active → stale`；检索默认排除 `stale`，除非请求显式要求历史证据并带 provenance。
- `revoked`：立即从检索与索引排除；`deleted`：写 tombstone，不复活，不参与去重之外的任何用途。
- 清理责任：daemon / `kiana-query` 的 memory service 执行确定性 sweep，并 append `MemoryExpired` / `MemoryStale` 事件；删除传播范围以 [`company-os-operations-governance.md`](company-os-operations-governance.md) §9.3 为准（primary record → event projection → artifact index → vector index → graph index → compaction summary → provider-bound cache policy → search result cache）。
- Memory 的过期与删除不得改写历史 Event/Receipt；只改变可检索性与派生索引。

**Collection 分类学与六层 ACL（新增规范，最小第一版枚举）**：

- `collection` 是存储与检索分区，恰好属于一个 ACL 层，必须声明 `layer`、`subject`、`purpose`、`retention`；一个 collection 不得跨两个 ACL 层。
- 最小第一版枚举与六层一一对应：

```text
company/<organization_id>/
department/<department_id>/
role/<role_id>/
project/<project_id>/
user/<principal_id>/
instance/<session_id>/scratch
```

- ACL 判定以 collection 声明的 `layer + subject` 为准；跨 collection 检索必须逐层授权，不能因为同一用户而自动 union 全部 Memory（operations §3.3）。
- promotion 是显式的跨 collection 迁移；仅修改 collection 字段而不走 promotion 属于违规。

### 5.4 Memory 参考设计

| 参考项目/材料 | 可吸收能力 | Kiana 约束 |
|---|---|---|
| Continue、Letta Code | session history、resume/fork、服务端/持久化记忆边界 | 必须接入 Event/ACL，不把 thread id 当权限 |
| Aider | repo map、按任务选择上下文、context budget | repo map 只提供检索上下文，不授予写权限 |
| MemPalace / memorix Git Memory | diary、lessons、按项目沉淀经验 | 写入必须有 collection、source 和 retention |
| Agent Framework / Agno | 可序列化 run state、requirements/checkpoint | 不以 callback 或内存 Map 代替 durable memory |
| 本仓 `kiana-query` | repo map、token budget、停止 hook、索引 | promotion / expiry 的规范规则见 §5.3；provenance 与统一 catalog 见 §5.3、§7 |

## 6. Prompt Cache、Compaction 与 Context Editing

### 6.1 三种不同机制

```text
Prompt Cache       Provider 复用相同输入前缀，降低重复输入成本
Compaction         把旧历史压缩成带 cursor 的摘要
Context Editing    清除旧 tool result/thinking 等内容
Memory             把跨任务知识按 ACL 检索出来
```

一个机制不能假装成另一个机制：缓存命中不能代替恢复，压缩摘要不能覆盖原始事件，Memory 命中不能直接成为权限。

### 6.2 稳定前缀策略

对于支持 prefix cache 的 Provider，建议按以下顺序渲染：

```text
tools / capability catalog
→ stable system / security constitution
→ stable role prompt
→ stable project instructions / repo map version
→ compacted history checkpoint
→ current work packet
→ memory hits
→ volatile tool results / timestamps / request id / user input
```

必须避免：

- 在 system prompt 中放当前时间、随机 UUID 或 request id；
- 工具 schema 无序、每轮改变或混入动态描述；
- 每轮改写整个 system prompt；
- Provider、model、effort、tool set 在没有显式原因时漂移；
- 把全部历史和全部 MCP schema 放入每次请求。

### 6.3 Cache telemetry

每个 Provider response 应保留不含秘密的 usage：

```text
input_tokens
cache_write_tokens
cache_read_tokens
output_tokens
context_tokens_before / after
memory_tokens
tool_catalog_tokens
compacted_tokens
model / provider
prompt_version
catalog_version
invalidation_reason?
```

基础指标：

```text
prefix_reuse_ratio
  = cache_read_tokens
    / (input_tokens + cache_write_tokens + cache_read_tokens)

uncached_input_ratio
  = input_tokens
    / (input_tokens + cache_write_tokens + cache_read_tokens)

cost_per_accepted_delivery
  = total_provider_cost / accepted_delivery_count
```

字段命名以 [`company-os-operations-governance.md`](company-os-operations-governance.md) §7.2 的 `UsageRecord` 为准（`cache_read_tokens` / `cache_write_tokens`）；成本指标统一为 `cost_per_accepted_delivery`。

缓存命中率必须按 `model + provider + prompt_version + catalog_version + workload` 分桶，不能只报全局平均值。

### 6.4 Compaction 合同

```text
ContextCheckpoint {
  checkpoint_id
  session_id / run_id / turn_id
  source_event_cursor
  model_visible_history_hash
  retained_refs[]
  dropped_refs[]
  summary_artifact_ref
  summary_prompt_version
  token_counts
  created_at
}
```

规则：

- 原始 Event 永远保留或按明确 retention 归档；
- compaction 只产生新 Artifact/Projection，不修改历史事实；
- summary 必须记录覆盖的 event cursor 和 hash；
- tool result 插入后、下一次 Provider request 前可以建立 checkpoint；
- fork 产生新 lineage，不覆盖父 Session；
- resume 由 Event + checkpoint 重建，而不是依赖 Provider cache。

### 6.5 参考设计

| 参考项目 | 设计 | Kiana 吸收方式 |
|---|---|---|
| OpenCode | processor、重试、压缩、中止和事件 projector | 统一进 `Run` reducer 和 Event Store |
| Continue | history、resume/fork、compaction、permission manager | 将 branch lineage 与权限 ownership 分开 |
| Roo Code | UI timeline 与 model history 分离，恢复补齐悬空 tool result，写前 checkpoint | 保留 canonical invocation/result，不从 UI 推断状态 |
| Pi | append-only JSONL tree、fork/resume/compaction | 借鉴树形 lineage，补充 CAS、ACL 和 receipt |
| Goose | session state machine 和 checkpoint | 借鉴 effect-before-event 和 quiescent recovery |
| Aider | repo map 和上下文预算 | 只吸收 repo-context 选择，不吸收日志作为事实源 |

当前 Kiana 不应宣称真实 Prompt Cache 命中率或 token streaming，除非有 provider 级 usage、固定输入 fixture 和对应证据。

## 7. Capability Discovery / Tool Search / MCP

### 7.1 CapabilityDescriptor

```text
CapabilityDescriptor {
  capability_id
  namespace
  operation
  version
  source_type: built_in | mcp | skill | workflow
  provider_ref?
  input_schema_ref
  output_schema_ref
  schema_hash
  domains[]
  tags[]
  side_effect_class
  risk_level
  data_classes[]
  required_scopes[]
  supported_roles[]
  approval_policy
  idempotency_policy
  timeout / retry_policy
  compensation_ref?
  reconciliation_ref?
  health
  trust_state
  license_ref?
  deprecation?
}
```

一个 Descriptor 必须同时服务于：

```text
模型工具 schema
工具搜索索引
Policy metadata
Broker binding
Approval preview
Receipt / provenance
```

`source_type` 与 [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md) §9.1 的 `extension_type` 不是同一枚举，映射关系为：`built_in` 对应 Kiana 内置、不来自 ExtensionManifest；`skill` ↔ `skill`；`workflow` ↔ `workflow`；`mcp` 与 `capability` 都来自 `extension_type: capability`；`extension_type` 的 `memory` / `provider` / `ui` 不新增 `source_type`，它们暴露的可调用项仍须归入上述四种之一。

### 7.2 Tool Search pipeline

```text
User/Agent intent
  → exact alias/name lookup
  → role/department/project visibility filter
  → trust/health/version filter
  → risk and data-scope pre-filter
  → lexical/BM25/tag retrieval
  → optional semantic rerank
  → top-k compact descriptors
  → selected schema expansion
  → model tool call
  → ControlPlane final authorization
  → Broker dispatch
```

必须遵守：

- 搜索只返回候选，不返回 Grant；
- semantic rank 不得绕过 ACL、Policy 或 Approval；
- 默认只加载 top-k 工具的完整 schema，防止工具目录污染上下文；
- descriptor、schema 和 executor 必须版本绑定；
- catalog 变化、MCP reconnect、Skill upgrade、Policy epoch 变化都要使相应缓存失效；
- 未知工具、未知版本、malformed arguments 和 schema hash mismatch 必须 fail-closed。

### 7.3 MCP 生命周期

当前产品只把 stdio MCP 作为支持路径；HTTP MCP 保持 unsupported，不能因为增加一个 URL transport 就宣称安全远程能力。

```text
Configured
  → Spawned
  → Handshake
  → SchemaSnapshot
  → HealthChecked
  → Discoverable
  → InvocationScoped
  → ResultValidated
  → Quiescing
  → Stopped / Revoked
```

`McpServerDescriptor` 至少记录：

```text
server_id
binary/path or package identity
version/hash
transport
namespace
schema_snapshot_hash
trust decision
allowed environment refs
health / last_seen
spawn limits
```

每次 MCP tool call 还必须记录：

```text
server_id + schema_snapshot_hash
capability_id + operation + version
input_digest
policy/approval decision
execution start/end
result schema validation
redacted error/provenance
```

MCP 返回值、tool description 和 server log 都是不可信输入，不能修改 Role、Grant、Budget 或 Project 状态。

失败与恢复转移（新增规范）：

```text
Handshake 失败   → HandshakeFailed（不得进入 Discoverable）；按 retry_policy 退避，
                   超过上限 → Quarantined / Revoked
crash（进程退出） → Crashed → Reconnecting → 重新 Handshake + SchemaSnapshot
schema drift     → SchemaDrifted；旧 trust decision 与 catalog 缓存失效，
                   必须重新 trust decision 后才能回到 Discoverable
Revoked          → 停止接受新 invocation；in-flight 进入 Quiescing
```

- **in-flight 收敛**：crash、`Revoked` 或 `Quiescing` 时不得假定调用成功或失败；结果未确认一律 `result_unknown`，禁止自动重放非幂等调用（[`company-os-operations-governance.md`](company-os-operations-governance.md) §8.4）；只有幂等且 `idempotency_key` 一致的调用才可重试。
- **schema drift 的连带失效**：descriptor/schema 缓存、进行中的 approval 和未 dispatch 的授权必须失效并重新走 verdict；已 dispatch 的调用按上一条收敛。
- **Revoked 的收尾**：释放子进程与 spawn 配额、撤销 Grant、从 catalog 移除、使缓存失效，并保留 redacted 证据。

**Trust decision 来源（新增规范）**：

- **谁批准**：HumanPrincipal，或 Company policy 明确授权的 approver；模型输出、server 自述和安装脚本不能自行批准。
- **存哪**：记录在 `McpServerDescriptor.trust_decision`，至少包含 `approver`、`policy_snapshot`、`schema_snapshot_hash`、`scope`、`expiry`、`decided_at`；持久化在 daemon 的信任账本中，不进入 prompt、transcript 或模型上下文。
- **何时失效**：schema drift、version/hash 变化、scope 扩大、expiry 到达或 revoke；失效后必须重新批准才能回到 `Discoverable`。

### 7.4 参考项目

| 参考项目 | 设计 | Kiana 吸收方式 |
|---|---|---|
| DeepSeek Harness | Central ToolRuntime、schema/approval/执行集中 | 由 Broker + Catalog 统一承载 |
| OpenCode | permission allow/deny/ask、tool part 生命周期 | 将 permission verdict 与 Invocation event 绑定 |
| Goose | extension/tool approval、effect persistence | MCP effect 先受控、再记录结果 |
| Cline | host/runtime 分离、扩展和 hooks | server lifecycle 放到 DaemonHost，不进 UI |
| Agent Framework | typed tool schema、approval 绑定原始 model call | approval 绑定 invocation digest 和 call identity |
| 12-factor-agents | tool/action 接缝小而清晰 | 先做狭窄 catalog，不先暴露百级工具 |

## 8. Workflow 平面

### 8.1 Workflow 与 Swarm 的关系

```text
Workflow = 确定性流程图和状态机
Swarm    = Workflow 中一种有界 fan-out/fan-in 执行策略
```

Workflow Engine 负责顺序、依赖、信号、重试、超时、审批和补偿；它不直接拥有 shell 或 Provider 权限。

### 8.2 WorkflowDefinition

```text
WorkflowDefinition {
  workflow_id
  version
  input_schema
  output_schema
  nodes[]
  edges[]
  compensation_policy
  retry_policy
  timeout_policy
  context_policy
  required_roles
  capability_refs
  budget_policy
}
```

节点类型：

```text
Deterministic     代码判断、转换和校验
AgentTask         启动受限 Agent Run
Capability        通过 Broker 调用能力
Approval          等待可序列化的人类决定
Wait/Signal       等待时间或外部事件
FanOut/FanIn      有界并行和聚合
Gate              确定性验收
SubWorkflow       调用版本固定的子流程
```

### 8.3 WorkflowInstance 状态

```text
Created → Validating → Ready → Running
Running → WaitingApproval / WaitingSignal / Paused
Running → Retrying / Compensating / cancel_requested
Running → Succeeded / Failed / Cancelled / result_unknown
```

终态名与 §4.2 Run 终态统一：`cancel_requested` 是取消中间态的 wire 名，`cancelling` 只是 UI 投影名，不是终态；design §9.3 的 `Unknown` 在本规范中写作 `result_unknown`。

每个 NodeExecution 必须有：

```text
node_execution_id
workflow_instance_id
definition_version
input_digest
attempt
lease/owner
started_at / ended_at
output_ref / error_code
child_run_refs[]
```

### 8.4 Workflow 规则

- definition version 固定到一次 Instance；
- 重试必须有 attempt，不复活旧 NodeExecution；
- 每个 side-effecting node 必须有 idempotency 和 compensation/reconciliation 说明；
- Approval pause 必须可恢复同一个 Run 或明确创建新 Turn；
- workflow state 从 Event/State Store 重建；
- LLM 只提出候选节点或参数，不能直接改变 Instance status；
- 运行中改变验收或 scope 必须产生 ChangeRequest；
- workflow completion 不自动代表 Project 或 Objective success。

### 8.5 确定性合同（新增）

- **replay 合同**：相同 `WorkflowDefinition` version、相同 initial state fixture 和相同输入，必须重产出相同的 `NodeExecution` 序列（`node_id`、`attempt` 顺序一致）以及相同的 `input_digest` / `output_ref`。
- **模型输出的入口**：模型输出只能作为候选参数，经 ApprovalGate（Approval 节点或等价 policy gate）后才写入 `NodeExecution` 参数；模型不能直接改变 `WorkflowInstance` 或 `NodeExecution` 状态（§8.4）。
- **replay divergence 的计算**：逐项比对基准与重放的 `(node_id, attempt, input_digest, output_ref, error_code)` 序列，首个不一致项即 divergence point；结果必须写入 Receipt/Eval，并按 [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md) §5.1 阻断 Promote。
- **非确定输入**：时间、随机数、request id、进程 id 等必须来自可注入的 deterministic source，并纳入 `input_digest`；否则 replay 结果不成立。

### 8.6 参考项目

| 参考项目 | 可吸收设计 | 不直接复制 |
|---|---|---|
| Pydantic AI | 显式 Graph、typed state、validate-before-defer、retry 分离 | 不把 Python Graph API 作为 Kiana wire contract |
| CrewAI | Flow 与 Crew 分离、事件驱动 checkpoint | 不吞掉 checkpoint/handler failure |
| ChatDev 2 / MacNet | 图节点可含 memory、human、tool | 不使用共享 ChatChain 作为事实源 |
| Archon | YAML/DAG、fresh context、确定性 bash 节点、人审批门、worktree 隔离 | 不把 workflow 当部门编制，不依赖外部 SDK 工人 |
| gpt-pilot | 状态驱动 task/command step | 文件副作用和 DB commit 必须有明确事务边界 |
| AutoGen | runtime 与 team state 分离、RequestToSpeak、有界 max turns | 不采用无限 group chat |

## 9. Swarm / Multi-Agent 平面

### 9.1 允许的拓扑

第一版只建议：

```text
Supervisor → Workers
Planner → Builders → Reviewer
Parallel Map → Deterministic Reduce
Specialists → Synthesizer
```

不把自由广播、任意私聊、无限递归或 Queen/Hive 作为默认产品协议。

### 9.2 SwarmPlan 与 Cell

```text
SwarmPlan {
  plan_id
  workflow_instance_id
  parent_cell_id
  reason_code
  partition_strategy
  candidate_template_refs[]
  child_count_limit
  max_depth
  max_concurrency
  budget_reservation
  ttl
  merge_strategy
  idempotency_key
}
```

所有 child Cell 必须满足：

```text
child_grant
⊆ parent_grant
∩ template_grant
∩ department_policy
∩ project_policy
∩ packet_scope
```

`WorkFingerprint` 用于防重复劳动；相同 fingerprint 的活跃任务应复用、订阅结果或拒绝重复创建。

### 9.3 Fan-out / Fan-in 生命周期

```text
validate plan
  → reserve budget + locks + grants
  → create child cells
  → assign typed WorkPackets
  → run independently
  → heartbeat/checkpoint
  → collect typed results
  → validate outputs
  → Review / MergeDecision
  → release grants/locks/unused budget
  → retire children
```

Child failure、timeout、cancel 或 `result_unknown` 必须向 parent 和 Workflow 汇报；不能只在一个共享 transcript 中留下文字。

### 9.4 Partition 与 MergeDecision（新增）

```text
Partition {
  partition_id
  swarm_plan_id
  partition_key
  input_refs[]
  owned_paths[] / data_scope[]
  output_contract
  child_cell_id?
  status
}

MergeDecision {
  merge_decision_id
  workflow_instance_id
  parent_cell_id
  merge_strategy
  accepted_child_outputs[]
  rejected_child_outputs[]   # 附 reason_code
  conflict_refs[]
  reviewer / acceptor
  policy_snapshot
  evidence_refs[]
  created_at
}
```

- `SwarmPlan.partition_strategy` 必须物化为互斥、可验证的 `Partition`；每个 Partition 只归属一个 child Cell，不得重叠写集。
- **MergeDecision 与 design §5.6 MergeReceipt 的关系**：MergeDecision 是“是否合并”的授权决定；MergeReceipt 是该决定的回执投影——一个决定至多产生一个回执，回执必须引用 `merge_decision_id` 并携带 reviewer/acceptor 与 provenance（[`company-os-design.md`](company-os-design.md) §5.6、§6.5）。
- **SwarmPlan 生命周期与 SupervisionLease**：validate/authorize → 原子预留 `BudgetLease` / Grant / 锁 → 创建 child Cell → 每个 child Cell 绑定 design §5.6 的 `SupervisionLease`（heartbeat、checkpoint、stall threshold、retry limit）→ 收集 typed result → MergeDecision → 释放 lease/grant/未用预算 → retire。
- `result_unknown` 的 child 不得被合并进父输出；必须先 reconciliation 或按补偿规则收敛。

### 9.5 Swarm 参考项目

| 参考项目 | 设计 | Kiana 吸收方式 |
|---|---|---|
| Agency Swarm | 定向 communication flows、handoff 与 orchestrator-worker 区分 | 使用定向 WorkPacket，不使用自由 SendMessage |
| AutoGen Magentic-One | RequestToSpeak、progress ledger、max turns | 使用有界发言/步数和 progress state |
| MetaGPT | PM/Architect/Engineer/QA 角色分工和工件协议 | 只吸收角色与工件，不吸收全员广播环境 |
| ChatDev | 阶段性角色会议、图编排 | 只允许有议程、有轮次、有产物的 Symposium |
| CrewAI | Crew 与 Flow 分层 | Swarm 作为 Workflow 执行策略，而非独立 runtime |
| DeepSeek subagent | child context 独立、不是 fork 父窗口 | child 使用 fresh Session/Run 和显式 lineage |
| 本仓 `kiana-tasks` | WorkPacket、path isolation 形状 | 继续迁移到 canonical domain WorkPacket |

## 10. Provider Gateway 与事件输出

### 10.1 NormalizedEvent

Provider adapter 必须把不同供应商转换为内部事件代数：

```text
NormalizedEvent {
  event_id
  provider_request_id
  run_id / turn_id
  sequence
  kind: text_delta | reasoning_delta | tool_call_delta
        | usage | finish | error | pause
  payload_ref
  schema_version
}
```

内部循环只依赖：

```text
text
reasoning summary (if allowed)
tool call fragments
usage
finish reason
retry classification
pause/approval signal
error
```

必须覆盖：

- non-stream fallback；
- malformed tool arguments；
- truncated stream；
- duplicate/out-of-order delta；
- provider timeout；
- rate limit 和 retry-after；
- provider request 与 Invocation correlation；
- `refusal`、`pause`、`tool_use` 和 terminal reason 的版本化映射。

本节只定义 Provider 归一化事件；`RuntimeEvent` 的最低字段以 [`company-os-design.md`](company-os-design.md) §11 为 canonical，本文不重复定义。

### 10.2 客户端事件桥

CLI、Web、Desktop 和未来 SDK 使用：

```text
snapshot + event_cursor + run_id + epoch
```

Hydration 流程：

```text
subscribe before load
  → load durable snapshot
  → merge events after snapshot cursor
  → apply live events
  → require terminal event / receipt
```

断线重连不能靠“重新打印当前 transcript”修复；必须使用 cursor、sequence 和 terminal receipt。

**epoch（新增规范）**：客户端事件桥的 fence 版本，用于拒绝跨代重放、乱序事件和过期快照。

```text
递增条件（任一满足即递增）：
  - authority_epoch 变化（operations §3.3：Membership / RoleAssignment 撤销或降权）；
  - daemon 重启或 Run ownership 转移（进程内 cursor 与 in-flight 状态失效）；
  - Capability catalog / ToolSnapshot 的 schema_hash 集合变化；
  - policy/gate snapshot 或 WorkflowDefinition version 发生影响本次 Run 的变化。
```

- **作用域**：epoch 绑定 `(daemon instance, run_id)`；同一 Run 内 `sequence` 单调递增。
- **失效行为**：epoch 变化后旧 cursor 失效，客户端必须重新拉取 snapshot，再合并新 epoch 的事件；不得把旧 epoch 的事件拼进新 snapshot。
- **与 operations §3.3 `authority_epoch` 的关系**：桥 epoch 必须携带 `authority_epoch`；`authority_epoch` 变化必然使桥 epoch 递增，反之不成立（重启、catalog 变化也可独立递增）。

### 10.3 参考项目

| 参考项目 | 可吸收设计 |
|---|---|
| DeepSeek Harness | provider translate、step boundary、tool block 归一化 |
| Cline | 丰富的流规范化、Local Runtime Host、abort 队列 |
| Letta Code | OTID/run/seq、cursor 重放、流重连和审批恢复 |
| Crush | RunID 关联、terminal RunComplete 必达 |
| OpenCode | event projector 同时驱动持久化和 UI hydration |
| Aider | 本地流式 UX、Ctrl-C 和 diff context，但需加强进程取消 |

当前 Kiana Web 不宣称 token streaming；上述设计是未来 protocol version 的目标，不是现状说明。

## 11. Observability、Receipt 与 Eval

### 11.1 Trace 结构

```text
Objective
  → Project
  → WorkflowInstance
  → WorkPacket
  → Cell
  → Run
  → Turn
  → Invocation
  → RuntimeEvent / Artifact / Evidence
  → Acceptance / Receipt / Outcome
```

每一层都应能通过 `correlation_id`、`causation_id` 和 parent reference 回溯。

**Trace 对象定义（新增）**：Trace 是从 `RuntimeEvent` / `Artifact` / `Receipt` 派生出的只读关联视图，不是新的事实源；它可以被重放重建，但不能反向修改事件，也不能替代 Receipt 的证据断言。

### 11.2 必须观测的指标

| 维度 | 指标 |
|---|---|
| 交付 | accepted delivery rate、rework rate、time to acceptance |
| Runtime | run duration、step count、retry、pause、cancel、result_unknown |
| Context | context utilization、compaction frequency、dropped result count |
| Memory | retrieval hit、authority mix、stale-hit、promotion、deletion |
| Tool Search | candidate top-k、selected rate、schema load、unknown tool rate |
| MCP | server health、handshake failure、schema drift、timeout |
| Workflow | node success、blocked time、compensation、replay divergence |
| Swarm | fan-out count、duplicate fingerprint、queue wait、merge conflict |
| Provider | latency、rate limit、malformed stream、usage、cost |
| Cache | cache_read_tokens / cache_write_tokens、prefix reuse、invalidation reason |
| Security | deny、approval、expired、scope violation、secret redaction |

### 11.3 Eval 结构

每个能力必须有 provider-independent fixture：

```text
fake model stream
→ expected normalized events
→ expected policy verdict
→ expected tool lifecycle
→ expected event sequence
→ expected final state
→ expected Receipt assertions
```

最低测试集：

- tool call → result → final answer；
- malformed arguments、unknown tool、schema drift；
- approval deny / approve / expire / replay；
- pre-tool hook deny：hook `Block` 后 Broker 不得执行、无副作用，事件与 Receipt 出现稳定 `hook_blocked:<reason>`；
- hook 失败可见：hook 端口错误或配置损坏必须 fail-closed 为 `failed` 且错误可观测，不得静默放行；hook `Ask` 的审批归宿按 §4.2（实现差距见 §16）；
- compaction、checkpoint、fresh-process resume；
- cancellation race、orphan process、partial effect；
- concurrent turn、stale response、duplicate request；
- memory ACL、redaction、promotion、expiry；
- tool search visibility、risk filter、MCP reconnect；
- Workflow retry、compensation、signal、replay；
- Swarm partition、budget exhaustion、merge conflict、child failure。

已登记的局部证据：`CURRENT_STATUS.md` 的 G0-02 slice evidence（`PreToolHookPort` 注入）与 P1-02 证据块；`docs/coding-pack-matrix.md` P0-HOOK 行（`hook_blocked` 在 broker 执行前阻断）。这些只覆盖已实现部分；hook `Ask` 到 daemon 产品路径的审批回归仍缺。

## 12. 参考能力总表

| 能力 | 首选参考 | 备选参考 | Kiana 目标阶段 |
|---|---|---|---|
| durable Session/Run ledger | DeepSeek Harness | OpenCode、Cline、Goose | P0 |
| event projector/hydration | OpenCode | Crush、Cline | P0 |
| approval continuation | Agno | Agent Framework、Letta、Cline | P0 |
| cancellation fencing | Crush | DeepSeek Harness、Cline | P0 |
| context assembly | Aider | DeepSeek Harness、gpt-pilot | P1 |
| compaction/checkpoint/fork | Pi | Continue、Roo、Goose | P1 |
| prompt cache telemetry | Provider adapter contract | Claude API cache usage pattern | P1 |
| memory lifecycle | Continue / Letta | MemPalace、memorix | P1 |
| tool catalog/search | DeepSeek ToolRuntime | OpenCode permission、Goose extensions | P1 |
| MCP lifecycle | Cline host | Goose extensions | P1 |
| deterministic workflow | Pydantic AI Graph | CrewAI Flow、Archon YAML/DAG | P2 |
| controlled multi-agent | AutoGen / Agency Swarm | MetaGPT、ChatDev、CrewAI | P4 |
| normalized provider stream | Cline / DeepSeek | Letta、Crush | P1 |
| client replay/cursor | OpenCode / Crush | Letta、Cline | P1 |
| observability/eval | Agno / DeepSeek fixtures | OpenCode、CrewAI checkpoints | P1 |

## 13. 取舍清单

### 13.1 应该吸收

- 一个规范 Agent loop，而不是多个并行 loop；
- durable Session/Run/Invocation ledger；
- normalized provider events；
- serializable Approval/PendingInvocation；
- ContextPlan、token budget、compaction 和 checkpoint；
- 分层 Memory、ACL、provenance、promotion 和 expiry；
- CapabilityDescriptor 和两阶段 Tool Search；
- stdio MCP server lifecycle；
- version-pinned deterministic Workflow；
- 有界 Swarm fan-out/fan-in；
- cursor、hydration、terminal receipt；
- provider-independent fake-model eval。

### 13.2 暂时不要吸收

- 自由 `TeamCreate` / `SendMessage` 总线；
- 无限 swarm、Queen/Hive/BFT 作为默认协议；
- 把全部工具 schema 常驻 prompt；
- 不受限 shell 或自动安装依赖；
- 把 Markdown transcript 作为状态库；
- 先做 HTTP MCP、远程执行、computer-use、真实支付和 IoT；
- 先做多租户、跨机器同步和企业 RBAC；
- 为同步、异步、streaming 分别维护三套生命周期；
- 用缓存命中或模型文字证明现实世界结果。

## 14. 实施依赖和发布门

全局阶段序列以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 为唯一 canonical；本节只登记与本平台平面相关的阶段门。

```text
P0 事实基线、Identity、Runtime ledger、Event/Receipt 和负向测试
  ↓
P1 ContextPlan、Memory、Cache telemetry、Capability Catalog、Cost、Eval
  ↓
P2 Durable Workflow、Human Inbox、Recovery、Artifact、Client cursor
  ↓
P3 Objective → Project → Packet → Builder → Review → Acceptance → Close
  ↓
P4 有界 Swarm、Scheduler、Provider streaming、Skill/Plugin ecosystem
  ↓
P5 Office / Commerce / Travel / IoT bounded contexts
  ↓
P6 Team / Remote / Enterprise
```

阶段门：

- P0 未通过前，不得以“有 Tool/Memory 类型”宣称平台完成；
- P1 未通过前，不得以“有向量索引”宣称可用 Memory；
- P2 未通过前，不得以“有 YAML/DAG”宣称 durable Workflow；
- P3 未通过前，不得以“跑通一次 Builder 闭环”宣称 CompanyOS 交付生命周期完成；
- P4 未通过前，不得以“能创建多个 Agent”宣称 Swarm，也不得宣称 token streaming、Scheduler 或 Provider parity；
- P5 未完成身份、审批、幂等、对账和真实 adapter 证据前，不得打开外部副作用；
- P6 未重新设计 authenticated identity、租户、网络和运营安全前，不得宣称企业能力。

## 15. 当前诚实产品描述

> **Kiana 已形成以 `DaemonHost → ControlPlane → Broker/KianaHarness` 为核心的本地 Agent 治理骨架，并有局部的查询、记忆 ACL、WorkPacket、EventLog、Approval、Review 和 Receipt 实现；Context/Cache、统一 Capability Discovery/MCP、durable Workflow、受控 Swarm、Provider streaming 和跨进程恢复仍是分阶段建设目标。**

## 16. 开放决策（规范 vs 代码冲突，待 ADR）

以下冲突只登记，不在本文放宽规范迁就代码；解决前相关路径保持 fail-closed。

1. **hook `Ask` 与 Approval 通路未打通**：§4.2 要求 `Ask` 进入 PendingInvocation/ApprovalChallenge（design §10.1）；当前 harness 产品路径在无人值守时把 `Ask` fail-closed 为 `hook_ask_unattended`，两条“要人确认”的通路尚未接通（`docs/features/05-approvals.md` 记载；`CURRENT_STATUS.md` 的 P1-02 证据块显示 continuation 仍为 process-local partial）。需要 ADR 决定 `Ask` 的规范归宿与过渡语义。
2. **epoch / authority_epoch 的持久化载体未定**：§10.2 要求 epoch 在 daemon 重启后仍能 fence 旧 cursor；当前 session/run/lock/approval 仍有进程内状态，`CURRENT_STATUS.md` 记“跨进程完整 resume = deferred”。需要 ADR 决定两者的持久化位置与迁移方式。
3. **MCP trust decision 的持久化位置未定**：§7.3 要求存放在 daemon 持久信任账本；是扩展 `ProjectTrust` 还是新增独立账本，需要 ADR。
4. **本地单用户版的 Memory promotion 审批主体未定**：§5.3 要求由目标层 owner 批准；本地版是否允许 HumanPrincipal 自批（以及如何与 Company policy 区分）需要 ADR。
5. **Invocation 与 CapabilityExecution 的最终形态**：本文采用 `1 Invocation : N CapabilityExecution attempt` 的映射；若未来选择合一，必须先修改 [`company-os-design.md`](company-os-design.md) §5.2 的“`execution_id` 与 `invocation_id` 不得混用”约束。
