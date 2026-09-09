# Kiana CompanyOS 质量、学习与扩展生态

> 文档性质：质量与生态规范目标（Normative Target）。
>
> 本文补齐 CompanyOS 的“系统如何变好、如何扩展、如何保持兼容”部分：Eval、Golden Trace、Human Feedback、模型与 Prompt 版本治理、代码知识能力、Plugin/Skill/Workflow Pack、扩展审核和生态边界。
>
> 当前实现和证据等级以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。本文中的目标对象和流程未被实现或测试证明时，不得写成当前能力。

> **本文速览（导读，非规范）**
>
> - **讲什么**：系统怎么"变好而不悄悄变坏"——可重放的评测（EvalSuite/EvalCase/GoldenTrace）、评分与质量门（`QualityGate` 与 Candidate 状态机、replay divergence 阻断 Promote）、人类反馈与漂移检测、模型/Prompt/路由的版本治理、代码知识索引（repo map/符号索引）、记忆生命周期与质量生态的接口、插件与 Skill 生态（声明≠授权）、Workflow Pack 的版本化 ArtifactGraph 与 `workflow validate` 门禁、供应链安全审查。
> - **回答的问题**："换了模型、改了 Prompt、装了插件之后，怎么知道没有退步、没有引入安全问题。"
> - **核心规则**：学习只能改版本化的配置（Prompt/路由/索引），永远不能自动改权限、安全策略或历史事实；代码知识查询服务于服务端上下文装配，不新增模型可见工具；扩展 / Skill 的声明只是声明，安装成功不等于授权，也不等于安全验证完成。
> - **什么时候读**：做评测、改模型配置、接入插件或 Skill 时。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 目标

一个 Agent 系统不能只记录“运行成功”，还必须能够回答：

- 这次结果是否真的正确；
- 工具是否选对、参数是否安全；
- Context 和 Memory 是否提高了结果质量；
- Workflow 和 Swarm 是否减少了返工；
- 新模型、Prompt、Skill 或 MCP Server 是否引入回归；
- 成本下降是否牺牲了验收质量；
- 新扩展是否改变了权限、数据和恢复语义。

CompanyOS 的质量循环是：

```text
Observe
  → Reproduce
  → Evaluate
  → Diagnose
  → Change one variable
  → Replay
  → Promote / Reject
  → Monitor drift
```

学习只允许改变版本化的 Prompt、模型路由、索引、Skill 或 Workflow 配置，不能让模型自动修改身份、Policy、Grant、预算或事实事件。

## 2. 质量架构

```text
Runtime Event / Receipt / Artifact
              │
              ▼
        Trace Normalizer
              │
      ┌───────┼────────┐
      ▼       ▼        ▼
   EvalCase  Metrics  Feedback
      │       │        │
      └───────┼────────┘
              ▼
     Versioned Candidate
              │
      Shadow / Replay Run
              │
        Quality Gate
              │
      Promote / Rollback
```

质量系统必须与生产执行隔离：

- Eval 可以读取脱敏事件和 Artifact；
- Eval 默认不能产生外部副作用；
- Replay 不得重复真实支付、发送、发布或设备操作；
- Candidate 只有通过 Gate 后才能进入默认路由；
- Human Feedback 需要 provenance、scope 和隐私策略。

## 3. Eval 对象合同

### 3.1 EvalSuite

```text
EvalSuite {
  suite_id
  version
  purpose
  workload_class
  cases[]
  scoring_policy
  safety_policy
  budget_policy
  baseline_ref
  owner
  status
}
```

### 3.2 EvalCase

```text
EvalCase {
  case_id
  suite_id
  input_fixture
  initial_state_fixture
  model_profile
  prompt_version
  tool_catalog_version
  memory_snapshot_ref?
  workflow_definition_ref?
  expected_events[]
  expected_state
  expected_artifacts[]
  expected_receipt_assertions[]
  forbidden_effects[]
  score_policy
}
```

### 3.3 GoldenTrace

```text
GoldenTrace {
  trace_id
  source_run_id
  source_snapshot
  input_hash
  event_cursor_range
  normalized_events[]
  artifact_hashes[]
  receipt_hash
  human_acceptance?
  quality_score
  created_at
  expires_at?
}
```

GoldenTrace 不是把所有历史对话永久复制给模型，而是用于：

- 回归比较；
- Event reducer 测试；
- Provider normalization 测试；
- Tool lifecycle 测试；
- Prompt/Model/Tool catalog 变更比较；
- 业务 Acceptance 和 Outcome 的证据基线。

## 4. 评测维度

### 4.1 Runtime Correctness

```text
model request
→ normalized stream
→ tool call
→ policy verdict
→ approval
→ execution
→ result
→ finalization
```

验证：

- event 顺序；
- call/result correlation；
- retry 和 attempt；
- approval pause/resume；
- cancellation；
- Unknown；
- crash/restart/replay；
- terminal state 合法性。

### 4.2 Tool and Capability Quality

验证：

- Tool Search top-k 是否含正确能力；
- 不可用工具是否被排除；
- 参数 schema 是否正确；
- 模型是否选择了最小权限工具；
- payload digest 是否稳定；
- policy/approval 是否正确阻断；
- tool result 是否被安全地重新注入 Context；
- MCP schema drift 是否被检测。

### 4.3 Context and Memory Quality

验证：

- 召回 precision、recall 和 freshness；
- ACL 过滤是否在排序前执行；
- 是否检索了错误项目或用户记忆；
- compaction 是否保留关键约束；
- context budget 是否超限；
- provenance 是否完整；
- cache prefix 是否稳定；
- token cost 是否下降而质量不下降。

### 4.4 Workflow and Swarm Quality

验证：

- 依赖和状态转移；
- retry、timeout 和 compensation；
- Approval、Signal 和 HumanTask；
- fan-out/fan-in 一致性；
- duplicate WorkFingerprint；
- merge conflict；
- child failure 传播；
- parent/child 权限缩减；
- replay 与 live run 是否分歧。

### 4.5 Business Outcome Quality

验证：

- Objective 是否有可测量的 Outcome；
- Project 是否通过 Acceptance；
- Delivery 是否被确认；
- 返工率是否下降；
- 完成交付成本是否可接受；
- Receipt 是否没有把推测写成事实。

## 5. Scoring 与质量门

### 5.1 评分组成

```text
QualityScore
  = functional_correctness
  + policy_safety
  + evidence_completeness
  + recovery_correctness
  + context_relevance
  + cost_efficiency
  + latency
  + replay_correctness
```

`replay_correctness` 是布尔 gate 维度（同一 EvalCase 重放与 baseline 是否一致，取 0/1），独立于 `recovery_correctness`：后者度量 crash/restart/Unknown 后的恢复能力，前者度量重放确定性。二者不合并，否则数值平均会掩盖 replay divergence。

`replay_correctness` 的判定由确定性重放产生：`kiana replay --run <id>`（仅 CI / 测试入口，不是模型可见工具）只读折叠该 Run 的已记录事件，按 `(invocation_id, attempt, input_digest, 状态, error_code)` 序列逐项比对，**首个不一致即为 divergence point**，该项判 0 并直接阻断 Promote。重放只做只读投影，绝不重跑副作用或调用模型；遇到未知 `logic_version` / 未知 patch marker 一律 fail-closed 拒绝重放，而不是猜测走哪条分支。

安全和事实完整性是阻断项，不得用文本质量或低成本抵消：

```text
if policy_safety < threshold → reject
if evidence_completeness < threshold → reject
if replay_correctness == false → reject   # 含出现 divergence point
if forbidden_effect != empty → reject
```

### 5.2 Candidate 状态与 QualityGate 合同

Candidate 状态机（对象 `VersionedCandidate`，即 §2 图中的 Versioned Candidate；canonical 登记见 [`company-os-implementation-outline.md`](company-os-implementation-outline.md) Slice L2 的 Candidate/Promotion）：

```text
Draft → OfflineEvaluated → Shadowed → Approved
OfflineEvaluated → Rejected
Shadowed → RolledBack / Approved
Approved → Deprecated
```

`VersionedCandidate` 最小字段合同：

```text
VersionedCandidate {
  candidate_id
  baseline_id
  changed_dimension: model | prompt | tool_catalog | memory_index | workflow | route
  changed_version_ref
  suite_version
  status: Draft | OfflineEvaluated | Shadowed | Approved | Rejected | RolledBack | Deprecated
  created_at
  created_by
}
```

Promote 绑定中的其余版本字段由 `changed_version_ref` 指向的版本快照提供，不在 Candidate 上重复定义。

`QualityGate` 是“门槛配置 + 裁决记录”的合同（canonical 登记见 [`company-os-spec-index.md`](company-os-spec-index.md) §4.3 与 [`company-os-implementation-outline.md`](company-os-implementation-outline.md) Slice L1；owner `kiana-runner` / `kiana-eventlog`）：

```text
QualityGate {
  gate_id
  gate_version
  suite_version
  thresholds: { policy_safety, evidence_completeness, recovery_correctness, replay_correctness, ... }
  blocking_rules[]
  verdict: pass | reject | needs_shadow | rollback
  candidate_id
  baseline_id
  score_delta
  known_regressions[]
  approver
  decided_at
  evidence_ref
}
```

门槛配置（`gate_version`、`suite_version`、`thresholds`、`blocking_rules`）与裁决记录（`verdict` 及其后字段）必须分离：配置可以升版本，裁决记录一旦写入不可改写，只能由新的 `gate_version` 追加新裁决。

每次 Promote 都必须绑定：

```text
candidate_id
baseline_id
model/version
prompt/version
tool_catalog/version
memory/index/version
workflow/version
suite/version
score delta
known regressions
approver
```

## 6. Human Feedback 与 Learning Loop

### 6.1 Feedback 对象

```text
Feedback {
  feedback_id
  principal_id
  target_type: turn | tool_call | memory | workflow | artifact | receipt
  target_ref
  label
  comment?
  correction_ref?
  scope
  created_at
  privacy_policy
}
```

`response` 不是 canonical 对象名：对单轮模型回复的评价挂到 `Turn`，对整次执行的评价挂到 `Run`，对事实投影的评价挂到 `Receipt`；`target_ref` 必须指向对应 canonical ID。

Feedback 只能产生候选改进：

```text
Feedback
  → Diagnosed Pattern
  → Candidate Prompt/Tool/Memory/Workflow change
  → EvalSuite
  → Human/Policy Gate
  → Versioned Promotion
```

不能直接：

- 修改安全宪法；
- 扩大 Grant；
- 晋升敏感 Memory；
- 改变 Acceptance criteria；
- 取消 Approval；
- 修改历史 Receipt。

### 6.2 Drift Detection

需要监测：

- 工具选择分布漂移；
- Memory 命中来源漂移；
- Prompt cache reuse 下降；
- Provider refusal、timeout、rate-limit 变化；
- Completion/Acceptance 下降；
- rework、Unknown、cancel 变化；
- token/cost 增长；
- Workflow replay divergence；
- MCP schema 或 Skill version 漂移。

漂移只触发告警、Shadow、降级或回滚，不自动修改安全或授权配置。

## 7. Model、Prompt 与 Route Governance

### 7.1 版本对象

```text
ModelProfile
PromptBundle
ToolCatalogSnapshot
MemoryIndexSnapshot
WorkflowDefinition
PolicySnapshot
```

一次 Run 应记录这些版本：

```text
model_profile_version
prompt_bundle_version
tool_catalog_version
memory_snapshot_version
workflow_version
policy_epoch
```

### 7.2 RouteDecision

```text
RouteDecision {
  route_id
  workload_class
  selected_model
  effort/profile
  provider
  reason_code
  fallback_policy
  budget_impact
  safety_constraints
  created_at
}
```

路由可以根据任务类型、延迟、成本和能力选择模型，但不能降低必要的安全等级：

- Planner、Reviewer 和高风险决策可以使用更强模型；
- 普通检索、格式转换和确定性节点可以使用低成本模型或非模型代码；
- 低成本模型不能自动获得更宽 Grant；
- fallback 不能绕过 Policy/Approval；
- model change 必须重新跑与 workload 匹配的 EvalSuite。

### 7.3 Prompt 版本规则

- 稳定系统、Role、Policy summary 和工具目录必须版本化；
- 动态用户输入、时间、request id 不得污染稳定 cache prefix；
- Prompt 变化必须有 diff、hash、受影响的 EvalSuite 和回滚方式；
- 不用“更详细的 prompt”替代确定性 guard；
- 不把隐藏推理或内部状态写入长期 Memory、Receipt 或日志。

## 8. Code Intelligence 与知识工具

Coding CompanyOS 还需要独立的代码知识能力，而不是只把整个仓库塞进 Context。

§8.2 的查询类型服务于服务端上下文装配，不新增模型可见工具：模型工具面保持冻结，只有已登记且通过冻结流程的固定工具集对模型可见（清单与证据以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 和 [`coding-pack-matrix.md`](coding-pack-matrix.md) 为准）。若未来把某类查询作为工具开放，必须走 `CapabilityDescriptor` 登记与工具面冻结流程，不得直接写进模型工具 schema，也不得把 `kiana-query` 的 repo map/search 当作模型可见工具。

> **开放决策（跨文档）**：是否把结构化查询（Read/Grep/Glob 类）作为模型可见工具开放。本规范不预设打开；打开前必须由 [`coding-pack-matrix.md`](coding-pack-matrix.md) 的 P1-READ 决策、`CapabilityDescriptor` 登记和工具面冻结流程共同确认。

### 8.1 Code Knowledge Index

```text
RepositorySnapshot
  ├── FileIndex
  ├── SymbolIndex
  ├── DependencyGraph
  ├── CallGraph?
  ├── TestIndex
  ├── ConfigIndex
  ├── GitHistoryIndex
  └── RepoMap
```

索引生命周期：

```text
Unindexed → Scanning → Indexed → Stale → Updating → Indexed
Scanning / Updating → Failed → Retry / Quarantined
Quarantined → Rebuild / Purge → Unindexed
```

`Quarantined` 索引不得参与查询，也不得被当作当前仓库状态；退出只有显式 Rebuild 或 Purge 两条路径，必须记录原因和操作者，不能自动重试后直接回到 `Indexed`。

### 8.2 Query 类型

```text
path / glob
symbol
text / regex
syntax / AST
dependency
call relationship
test ownership
git change history
semantic concept
```

搜索选择原则：

- 路径、符号和错误码优先 exact/lexical；
- 结构关系使用 graph；
- 概念问题使用 semantic/vector；
- 结果必须带文件、行号、snapshot 和 freshness；
- 索引结果不能授予写权限；
- stale index 不能伪装成当前仓库状态。

### 8.3 Reference 映射

| 参考项目 | 可吸收设计 | Kiana 约束 |
|---|---|---|
| Aider | repo map、按任务选择代码上下文 | 不把 repo map 当权限 |
| Continue | 多入口共享 Core/ChatHistory/context provider | 必须统一到 DaemonHost/ControlPlane |
| Roo Code | workspace context、history 与 UI timeline 分离 | 不从 UI timeline 推断事实 |
| OpenHands | event bridge、repository/workspace context | 事件和文件快照必须可追溯 |
| Mini-SWE-agent | 小型 parser 接缝和确定性测试 | 不采用不受限 shell 作为默认安全策略 |

## 9. Plugin、Skill、Workflow Pack 与 Extension

### 9.1 ExtensionManifest

```text
ExtensionManifest {
  extension_id
  version
  publisher
  license
  content_hash
  signature?
  extension_type: skill | capability | workflow | memory | provider | ui
  effect: read-only | read-write
  provided_capabilities[]
  required_capabilities[]
  supported_roles[]
  data_classes[]
  network_policy
  secret_refs[]
  configuration_schema
  migration_ref?
  rollback_ref?
  compatibility
  requires {
    kiana_version
    protocol_version
    capability_versions[]
    policy_features[]
  }
}
```

`extension_type` 与 [`company-os-platform-architecture.md`](company-os-platform-architecture.md) §7.1 的 `CapabilityDescriptor.source_type` 属于不同域、不同枚举：`extension_type` 描述扩展包交付的内容形态，`source_type` 描述可被 broker 派发的能力来源；不得把 `extension_type` 直接当 `source_type` 使用。映射关系为：

```text
extension_type skill      → source_type skill
extension_type workflow   → source_type workflow
extension_type capability → 按承载方式记 source_type：内置执行器承载记 built_in，MCP server 提供记 mcp
extension_type memory     → 不新增 source_type；若暴露可调用项，必须归入 skill / workflow / built_in / mcp 之一
extension_type provider   → 不新增 source_type；若暴露可调用项，必须归入 skill / workflow / built_in / mcp 之一
extension_type ui         → 不新增 source_type；若暴露可调用项，必须归入 skill / workflow / built_in / mcp 之一
```

`built_in` 是运行时来源类别（由 Kiana 内置执行器承载），不是 `extension_type` 的取值。

**声明不等于授权。** `effect`、`required_capabilities`、`network_policy` 以及 skill frontmatter 的 `allowed-tools` 等声明字段，只用于预批准提示、展示和安装校验，**不构成执行边界**：

- Skill 的 `allowed-tools` 只影响提示与展示，不进入 policy；技能想使用任何能力，都必须和其它模型工具一样经 broker + policy + approval。补一条 fail-closed 回归：声明 `allowed-tools: [shell]` 的 skill 不得导致任何未经批准的 shell 执行。
- `effect: read-only` 的扩展，其写操作在 broker 层直接拒绝（不依赖扩展自律）；`effect: read-write` 同样不等于获得写授权，实际许可仍由 policy / grant / approval 产生。
- 安装时校验 `content_hash` / `signature` 与 `requires` 兼容性（pack 级细项见 §9.3 的 `requires_*`），校验失败一律拒绝安装，不降级放行；**安装成功不等于安全验证完成**（供应链门见 §10）。

### 9.2 Extension 生命周期

```text
Discovered
  → Inspected
  → TrustPending
  → Approved
  → Installed
  → Registered
  → HealthChecked
  → Enabled
  → Disabled / Upgrading / Revoked
  → Uninstalled
```

扩展不能自行：

- 加宽父级 Grant；
- 增加默认网络或 Secret 权限；
- 修改安全宪法；
- 修改历史 Event/Receipt；
- 把私有 Memory 晋升到 Company 层；
- 覆盖同名 capability；
- 绕过 Workflow 的 Approval。

### 9.3 Pack 兼容性

Workflow Pack、Skill Pack 和 Capability Pack 必须声明：

```text
requires_kiana_version
requires_protocol_version
requires_capability_versions
requires_policy_features
requires_memory_collections
supported_platforms
migration_window
```

升级规则：

```text
Current
  → Snapshot
  → Compatibility Check
  → Install New Version
  → Migration
  → Health/Eval Gate
  → Promote / Rollback
```

### 9.4 Reference 映射

| 参考项目 | 可吸收设计 |
|---|---|
| Cline | extension 与 host/runtime 分离 |
| Goose | extension/tool registration 和 approval boundary |
| DeepSeek Harness | central tool runtime 和 schema contract |
| Archon | versioned workflow nodes、fresh context 和 artifact gate |
| Agency Swarm | role/tool relationship 与定向通信 |
| 本仓 `kiana-skills` / `kiana-tools` | 作为迁移素材，不自动视为新生态完成 |

### 9.5 ArtifactGraph 与 `workflow validate` 门禁

Workflow Pack 与规划产物链用一张**版本化** ArtifactGraph 声明「每个工件由谁生成、依赖什么才能进入实现」：

```text
ArtifactGraph {
  graph_version
  artifacts[] {
    id
    generates            # 该节点产出的工件
    template?            # 生成模板引用
    requires[]           # 进入该节点必须已存在的工件 id（显式字段）
  }
}
```

- `apply` 只认 `requires` 的**传递闭包**：缺依赖即 blocked，不得用 packet 文本或模型声明补齐依赖。
- `kiana workflow validate --json` 是只读门禁：稳定 issue code + 退出码，不写盘、不派发、不新增模型可见工具，只做工件存在性、依赖边与跨产物一致性分析。
- packet 绑定**冻结的 check 文件**（命令 + 期望退出码 / 输出）：Builder 执行后写进 EvidencePacket，Reviewer / Closer 只读结果判 pass / fail；冻结后任何改动即 FAIL。
- 机器可读的 **Constitution Check 是必须通过的 GATE**：硬约束（五工具面、冻结项、单执行路径、fail-closed、审批不绕过）逐条给出 pass / violation，任何 violation 直接阻断（冲突自动 CRITICAL）。条款与校验契约见 [`company-os-security-constitution.md`](company-os-security-constitution.md)。
- 上述命令仍经现有 shell 能力在 policy / sandbox 下执行，不新增第二条执行路径。

### 9.6 Memory 生命周期与质量生态的接口

记忆（含 `extension_type: memory` 与 `MemoryIndexSnapshot`）进入质量生态时必须携带生命周期与来源元数据；质量系统只读消费，不得据此扩大授权：

- **来源**：`provenance` 由捕获通道在服务端派生（model / hook / git / user），不读模型传入的 source 字符串；评测按来源分桶统计命中。
- **可信度**：证据记录（kind / referenceId / relation，含 contradicts）与由 provenance 派生的 confidence 只参与**排序**，不参与授权；晋级到 `qualified` 必须引用至少一条 supports / verifies。
- **准入**：`admission_state`（candidate / qualified / ephemeral）与生命周期 `status` 正交。instance-scratch 层保持默认可见（它是临时草稿），持久层默认 candidate 不可检索；`MemoryIndexSnapshot` 的 EvalCase 必须显式声明它期望的准入集合，避免升级后现有 Builder scratch 写入在测试里突然消失。
- **失效**：事实型记忆带 `valid_from` / `valid_to` / `expired_at`，冲突时旧记录标失效并 append 事件，检索默认过滤已失效；GoldenTrace 比较必须固定快照版本，避免失效时间造成假回归。
- **读 / 管分离**：`can_read` 与 `can_manage` 是两个独立、fail-closed 的谓词（能读 ≠ 能管）；`memory_index` 作为 `changed_dimension` 的 Candidate 只改版本化索引配置，不能借评测结果放宽任一谓词。
- **后续项 / 开放决策**：向量 / 图检索可作为后续项，但检索分数只进排序、不参与授权。

## 10. Security Review 与 Supply Chain

每个模型、Provider、Skill、Plugin、MCP Server 和 Workflow Pack 在启用前必须有：

```text
source/provenance
license
content hash
version
capability diff
network/secret diff
data processing policy
sandbox/profile
known vulnerabilities
rollback path
eval result
owner
```

供应链安全门至少检查：

- 包是否来自允许来源；
- hash/signature 是否匹配；
- transitive dependencies；
- 是否修改启动脚本、安装器或权限配置；
- 是否新增网络或 Secret；
- 是否注册了隐藏能力；
- 是否改变 tool schema 或 workflow state；
- 是否能在卸载后撤销 grant、hook、schedule 和 index。

不得把安装成功或 manifest 存在写成安全验证完成。

## 11. Quality、Ecosystem 与产品边界

> 本节阶段号是**局部命名 Q-1…Q-4**，只表示本文内部的阅读与实施顺序。全局阶段序列以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 为唯一 canonical；本文档的 Skill/Plugin/Workflow Pack 生态在 spec-index §7 属于 P4，不在全局 P2。对应关系：Q-1/Q-2 落在全局 P0–P1，Q-3 对应全局 P4 的 Skill/Plugin ecosystem，Q-4 对应全局 P4–P6。

### Q-1：可靠性和回归

- Runtime normalized event fixture；
- approval/cancel/Unknown/replay 测试；
- GoldenTrace 和 baseline；
- model/prompt/tool catalog versioning；
- provider-independent EvalSuite；
- security/policy regression。

### Q-2：上下文和代码知识

- ContextPlan 和 token budget eval；
- memory retrieval/ACL/provenance eval；
- repo map、symbol/dependency index；
- cache telemetry 和 invalidation diagnosis；
- Tool Search top-k 和 schema validation eval。

### Q-3：扩展和变更治理

- Skill/Plugin/Workflow Pack manifest（含 `effect` / `requires`）；
- trust/install/upgrade/rollback；
- ArtifactGraph 与 `kiana workflow validate --json` 只读门禁、冻结 check 与 Constitution Check GATE；
- MCP schema drift；
- shadow route 和 canary；
- feedback、drift 和 quality promotion。

### Q-4：生态与外部平台

- provider marketplace；
- signed external packs；
- team-shared catalog；
- remote worker；
- tenant-specific extension policy。

## 12. 完成定义

达到 `local_behavior` 前至少需要：

- 同一 EvalCase 可以在 fake model、CLI 和 DaemonHost 主路径中重放；
- Event、Artifact、Receipt 和 Score 可以关联；
- Candidate 的 Model、Prompt、Tool、Memory、Workflow 版本可追溯；
- 安全失败、禁止效果和 replay divergence 都会阻断 Promote；
- Cache、Memory、Tool Search 和 Workflow 指标可按版本分桶；
- Plugin/Skill/MCP schema 变化有 trust、compatibility、migration 和 rollback；
- Extension manifest 的 `effect` / `requires` 在安装时校验，`effect: read-only` 的写操作被 broker 拒绝，声明（含 skill `allowed-tools`）不扩大授权；
- Workflow Pack 的 ArtifactGraph 缺依赖 / 成环、Constitution Check violation、冻结 check 被改动都会阻断 apply 或 promote；
- 反馈不会直接修改权限、事实或安全策略；
- 代码知识结果带 snapshot、来源和 freshness；
- 生态扩展不会产生第二套 Runtime 或绕过 ControlPlane。

## 13. 当前诚实描述

当前状态只能引用 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 的既有证据块，不得从本文目标推断：

- `CURRENT_STATUS.md` §3 证据块 “Current Gate 0 revalidation after MCP, Swarm, and JSONL recovery slices (2026-09-07)” 只把已演练的本地路径证明到 `local_behavior`，不建立 durable/live/physical；
- `CURRENT_STATUS.md` §2 能力表把本地受控 coding 行为、DaemonHost 主路径、WorkPacket/Symposium/Review 标为 `partial`，把 live provider、token streaming、跨进程完整 resume 标为 `not_supported`/`deferred`；
- `CURRENT_STATUS.md` §5 “当前禁止的表述” 继续适用。

> **据此：Kiana 已有部分 query、memory ACL、skills、hooks、capability broker、cassette 和 focused regression 素材；统一 Eval/GoldenTrace、模型与 Prompt 变更治理、代码知识索引、插件生命周期、质量晋级和生态供应链仍处于目标设计阶段。**
