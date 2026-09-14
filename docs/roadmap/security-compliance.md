# 安全与合规专项：实际代码设计、处理流程与实施步骤

> 更新时间：2026-09-14
> 文档性质：实施设计与执行卡，不是合规认证声明。
> 总入口：[docs/roadmap.md 的安全与合规专项](../roadmap.md#security-compliance-plan)
> 模块入口：[module-map.md 第 20 模块](../module-map.md)
> 现状权威：[CURRENT_STATUS.md](../../CURRENT_STATUS.md)

这份专项把 module-map.md 的“安全与合规”从横向主题拆成可落到 Rust crate、端口、测试和证据账本的工程切片。安全控制仍然嵌在唯一执行脊柱中：

~~~text
kiana-entrypoints
  -> kiana-client / kiana-protocol
  -> kiana-daemon::DaemonHost
  -> kiana-core::ControlPlane
  -> kiana-policy + kiana-gates + approval
  -> kiana-capability-broker / kiana-runner::KianaHarness
  -> handlers
  -> EventStore
  -> Receipt / projections
~~~

本文件的 SC-00 至 SC-43 是安全专项的局部执行索引，不替代 P0-P6、CP、CAP、ER、CM、EXT、PD、INT、OA、DEP、BQ 或 EQ 的 canonical step。若旧版追加区的限制与本专项或最新状态账本冲突，按本次安全设计、company-os-security-constitution.md 和 CURRENT_STATUS.md 的较新证据解释；本文件不会把“有类型”或“有单测”写成已经完成。

## 1. 目标、范围与设计结论

### 1.1 目标

安全与合规要解决的是一条可证明的控制链，而不是增加一个独立的安全服务：

1. 服务端从可验证来源解析主体、项目可信度、角色、部门和会话；调用者自报字段只能作为输入，不能成为授权事实。
2. 每次有副作用的命令都由 ControlPlane 生成不可转移的、短期的、精确绑定的授权租约；Broker 在实际 effect 前再次检查租约、版本、路径、预算和数据边界。
3. 模型输出、项目文件、skill/plugin/hook、MCP 描述、Webhook 和 provider 响应全部按不可信输入处理；它们不能直接扩大能力或改变身份。
4. Secret 只以 opaque SecretRef 穿过协议，解析和注入发生在 Broker 受控内存中；日志、事件、Receipt、指标、备份和错误消息只保留引用或摘要。
5. EventLog 是产品事实源，Audit 是受控的安全证据投影，Trace/Metric/Transcript 是可删除的观察视图；未知结果、取消、重放和对账都保留原事实。
6. 数据分类、目的、跨边界共享、保留、删除、法务保留和导出都有显式策略，并能从 EventLog 和 manifest 重建证明。
7. 扩展和发布物要有来源、摘要、版本、许可证、漏洞和签名/证明记录；缺失或漂移时 fail closed。

### 1.2 不在本专项中伪造的结论

- 当前源码的实际完成度仍以 CURRENT_STATUS.md 为准；本设计不把 SEC-01 至 SEC-12 直接改成 code_enforced。
- “合规”在这里表示可审计的控制、证据和流程设计，不表示已经取得 GDPR、HIPAA、SOC 2、ISO 27001 或其他外部认证。
- 本地 fixture、静态审计和离线测试不能证明真实 provider、真实外部系统、生产密钥、云租户或物理 effect 的安全结果。
- result_unknown 不会被转换成成功，也不会在没有新的授权和幂等证据时自动重试。
- 不增加第二条模型循环、自由消息总线、绕过 ControlPlane 的连接器或权限并集。

### 1.3 不变量

| 编号 | 不变量 | 违反时的处理 |
|---|---|---|
| INV-S01 | 主体、项目、Cell、Grant、Approval、Budget、PathLock、DataBoundary 和 extension digest 必须属于同一个授权快照 | 拒绝 admission，写稳定 reason code |
| INV-S02 | 子 Cell 能力是父级、模板、部门、项目、WorkPacket 和 approval 的交集 | 交集为空或版本漂移时拒绝 |
| INV-S03 | canonical commit 先于 effect；effect 只能使用该 commit 产生的 permit | 无 commit、permit 过期或 fence 不匹配时不得 dispatch |
| INV-S04 | permit 的 payload digest、目标 endpoint、路径、secret/account、环境和 scope 不可在 effect 前改变 | 重新 admission；不能原地修改 permit |
| INV-S05 | secret value、原始 token、完整 prompt/response 和受限个人数据不能出现在不适合的 Event、Log、Metric、Trace、Receipt 或 manifest | 递归脱敏；无法安全脱敏时拒绝写出 |
| INV-S06 | unknown 是独立状态，事实可重放，外部结果要通过 idempotency/ref 重新确认 | 进入 reconcile inbox，冻结相关预算和租约 |
| INV-S07 | EventLog 只追加；projection、cache、transcript、metric 和 UI 不得反写事实 | 检测到 cursor/hash 漂移时隔离 projection |
| INV-S08 | policy、config、authority、data epoch 单调增加；旧进程、旧 root、旧 approval 不能复活 | fencing 或 quarantine |
| INV-S09 | 所有限制都有有界上限：输入、输出、队列、并发、重试、wall time、bytes、索引和保留空间 | backpressure 或拒绝，不允许无界降级 |
| INV-S10 | 每个安全决定能关联 operation、run、attempt、revision、source cursor 和 evidence ref | 缺少关联字段时不得宣称可审计 |

## 2. 调研结论

### 2.1 本仓库 reference 与审计材料

以下材料在 2026-09-14 复核。reference 目录是历史快照；目录存在不代表实现已经被 Kiana 采用。

| 来源 | 看到的机制 | 对 Kiana 的落点 | 边界 |
|---|---|---|---|
| company-os-security-constitution.md | SEC-01 至 SEC-12、R0-R5、proof level、拒绝矩阵 | 作为本专项的条款和发布门；每条 SEC 映射到 SC 卡片 | 宪法是目标与验收合同，现状看 CURRENT_STATUS.md |
| reference-agent-audit/00-unified-agent-flow.md、01-codex.md | 事件优先、控制面授权、取消和 workload/identity 边界 | admission、effect、observation 三段式事实链 | 不把 transcript 或 provider SDK 当权威 |
| 02-deepseek-harness.md、07-goose.md、08-opencode.md | durable ledger、checkpoint、projector/hydration、permission manager、失败保留 | PendingInvocation、replay、CAS、redacted projection | 进程内回调不能替代 durable 事件 |
| 06-openhands.md、15-agent-framework.md、16-adk-python.md | runtime host、credential probe、事件共识、function-call approval 绑定 | connector/secret probe 走 Broker，approval 绑定 name+args digest | 探测不能自动授予权限；数据责任仍在宿主 |
| 17-openai-agents-python.md、18-pydantic-ai.md | usage、limit、retry、trace privacy、run state | typed usage、attempt 级限制、trace 与 EventLog 分离 | provider 自报用量不能直接授权 |
| 19-autogen.md、20-agency-swarm.md、21-agno.md、22-crewai.md、23-chatdev.md、24-metagpt.md | 角色/团队/轮次/预算和 handoff | 只采用 bounded delegation、Cell 预算和角色视图 | 不采用自由消息总线或全局进程计数器 |
| 25-letta-code.md、26-12-factor-agents.md、beads、a2a | task state、claim/lease、确定性控制流、暂停/恢复 | Task/WorkPacket 状态、lease/fence 和显式继续 | 外部 TaskState 不能成为 Kiana 权限源 |
| grok-build 审计与实现快照 | req hash、dense sequence、divergence fail closed、hunk attribution、CoW 和 OS sandbox | request/sequence/digest、patch lock、sandbox profile、证据 manifest | 不把参考项目的 OS 覆盖范围当作本机证明 |
| temporal-sdk-python、container-use、gastown | replay/determinism、environment checkpoint、estop/恢复 | 版本 pin、replay、quarantine root、stop/unknown | 不引入外部 exactly-once 或把容器当安全边界 |
| graphiti、mem0、memorix、claude-mem-candidate | temporal memory、content hash、显式 qualify、task-lensed context | candidate/draft、origin、purpose、retention、memory boundary | memory 不是授权或事实账本 |
| pm-skills、GitNexus、gsd-core | exact commit pin、release digest/manifest、fork 不可信、负向安全测试 | extension trust roots、SBOM/provenance、negative proof | 不默认信任项目 fork、市场条目或安装脚本 |
| gpt-pilot、crush、opencode、autogen 的安全说明 | 凭据泄露、配置/插件的全权限风险、只信任明确 MCP | secret probe、ProjectTrust、MCP server allowlist、默认最小能力 | OAuth 或 keychain 的存在不等于 Kiana 已完成认证 |

本次 reference 复核按四组覆盖：执行与入口（codex、deepseek-harness、goose、opencode、crush、openhands、cline、roo-code、continue、aider、pi、mini-swe-agent、gpt-pilot、agent-framework、adk、openai-agents、pydantic-ai、autogen、agency-swarm、agno、crewai、chatdev、metagpt、letta-code、12-factor-agents）；状态与记忆（beads、graphiti、mem0、memorix、claude-mem-candidate）；隔离、恢复与运维（grok-build、temporal、container-use、gastown）；规范、供应链与工作流（spec-kit、OpenSpec、pm-skills、GitNexus、gsd-core 及 reference-agent-audit 的 00–26、99 映射）。其余 reference 目录只作为候选行为对照，不把目录存在或 README 描述当作安全证据；采用/排除理由均回收到本节的控制边界和 SC 卡片。

### 2.2 外部一手标准

这些链接只作为设计输入，不改变本仓库的事实权威。

| 标准/项目 | 对本专项的具体影响 |
|---|---|
| [MCP Authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization) 与 [MCP Specification](https://modelcontextprotocol.io/specification/2025-06-18) | MCP 资源要验证 token audience；禁止把收到的 token passthrough 给下游；使用短期 token、PKCE、精确 redirect/resource，工具描述和数据共享需要用户同意 |
| [NIST AI RMF](https://www.nist.gov/itl/ai-risk-management-framework) 与 [GenAI Profile](https://www.nist.gov/publications/artificial-intelligence-risk-management-framework-generative-artificial-intelligence-profile) | 把风险识别、测量、治理和全生命周期证据接到设计、执行、事故和退役，而不是只做 prompt 过滤 |
| [OWASP GenAI / LLM Top 10](https://genai.owasp.org/llm-top-10/) | 将 prompt injection、insecure output、supply chain、sensitive disclosure、insecure plugin、excessive agency、unbounded consumption 转成拒绝测试和资源闸门 |
| [W3C Trace Context](https://www.w3.org/TR/trace-context/) | trace id 只做关联，不携带 PII/secret；跨进程传递要验证格式、边界和采样隐私 |
| [OpenTelemetry handling sensitive data](https://opentelemetry.io/docs/security/handling-sensitive-data/) | telemetry 采用最小收集、允许列表、删除/哈希/截断和独立保留；哈希小空间 ID 不能直接当匿名化 |
| [SLSA specification](https://slsa.dev/spec/v1.2/) | release/extension 记录 builder、external parameters、resolved dependencies、subject digest 和验证根，消费方验证而不是只看文件名 |
| [Sigstore security model](https://docs.sigstore.dev/about/security/) | 可用短期签名、证书透明日志和 append-only transparency log 记录 artifact 来源；私有部署仍需自己的信任根和监控 |

### 2.3 归纳出的工程原则

1. “授权”是对主体、意图、数据、资源和 effect 的联合判断，不是检查一个 API key。
2. “审计”必须能从不可变事实重放决定；日志里的解释文字不能替代决定输入。
3. “隐私”要约束采集、处理、投影、查询、导出、保留和删除整个生命周期。
4. “供应链”既包括 Cargo/npm/OS 依赖，也包括项目 skill、plugin、hook、MCP manifest、模型路由和 prompt pack。
5. “未知”是安全信号和预算占用，不是错误码的别名；所有恢复动作都需要新的证据。

## 3. 威胁模型和信任边界

### 3.1 资产

| 资产 | 机密性 | 完整性 | 可用性/可追责性 |
|---|---|---|---|
| Principal、session、role、Grant、Approval、policy revision | 高 | 高 | 高 |
| prompt、tool input/output、仓库和项目文件 | 依数据分类 | 高 | 中 |
| SecretRef、provider credential、OAuth refresh token | 极高 | 高 | 中 |
| EventLog、Receipt、Audit、backup manifest | 高 | 极高 | 极高 |
| Memory、index、artifact、评测数据 | 依 DataClass | 高 | 中 |
| 外部 API、Webhook、发布、支付或物理 effect | 高 | 极高 | 极高 |
| extension、依赖、binary、container、SBOM/provenance | 高 | 极高 | 高 |
| logs、metrics、traces、health、incident evidence | 中到高 | 高 | 高 |

### 3.2 不可信输入

模型文本和 tool call、用户提交的 actor/project/role 字段、项目内 skill/plugin/hook、仓库内容、网页/MCP 返回、provider response、Webhook payload、环境变量、配置文件、fork/marketplace metadata、旧版本事件和恢复请求都必须先解析、分类、验证，再进入策略计算。它们可以触发 needs_approval，但不能单独产生 Grant。

### 3.3 信任边界图

~~~mermaid
flowchart LR
  U[用户/入口/外部系统] --> I[协议解析与身份解析]
  P[模型/项目文件/插件/MCP/网页] --> N[不可信输入归一化]
  I --> A[ControlPlane Admission]
  N --> A
  A -->|deny / approval / permit| B[Capability Broker]
  B --> S[OS sandbox + SecretStore + network policy]
  S --> E[本地或外部 effect]
  A --> F[EventLog: decision/commit]
  E --> O[observation/result/unknown]
  O --> F
  F --> Q[Receipt / Audit / Query projections]
  Q -.只读.-> UI[CLI/Web/Workbench/Desktop]
  X[CI: SBOM/provenance/signature] -.信任根.-> A
  T[Telemetry] -.redacted view.-> Q
~~~

关键边界：

- DaemonHost 是组合根，ControlPlane 是授权和生命周期权威，Broker 是唯一 effect 适配边界。
- SecretStore、OS sandbox、网络策略和外部系统都是独立边界；任何一个边界不可用都不能被“本地成功”替代。
- UI、Transcript、Metric、Trace、Memory、Index 和通知只能读 projection 或提交 versioned command。
- Provider 可以返回结果和 usage，但不拥有 Principal、ProjectTrust、Approval 或 Budget 的权威。

### 3.4 威胁到控制的映射

| 威胁 | 典型触发 | 必须先拒绝/隔离的条件 | 主要控制 |
|---|---|---|---|
| T01 提示注入/间接注入 | 仓库或网页要求模型泄露 secret、改权限、调用隐藏工具 | 不可信文本试图改变 system/developer policy 或 capability | 输入分层、tool schema、SC-24、SC-39 |
| T02 confused deputy | MCP proxy、Webhook 或模型借用高权限账户 | audience/主体/目的/下游 token 不匹配 | SC-06、SC-10、SC-17、SC-26 |
| T03 越权/权限升级 | wire 自报 role/project、子 Cell 请求并集权限 | server snapshot 与请求或父 Grant 不一致 | SC-04、SC-07、SC-09、SC-10 |
| T04 Secret exfiltration | prompt、stdout、argv、trace、backup 或错误包含 token | 未分类、无目的、无法递归脱敏或超出 Broker | SC-18 至 SC-21、SC-39 |
| T05 TOCTOU/路径逃逸 | symlink、hardlink、rename、外部 root 或 stale permit | canonical path、inode/generation、fence 不一致 | SC-12、SC-13 |
| T06 重放/重复 effect | retry、恢复、重复 command、旧 webhook | idempotency/digest/sequence/epoch 不匹配 | SC-12、SC-15、SC-17 |
| T07 SSRF/网络越界 | 模型控制 URL、DNS rebinding、metadata endpoint | endpoint 不在 allowlist、解析后地址改变或跨 DataBoundary | SC-14、SC-17 |
| T08 资源耗尽 | 无限输出、队列、retry、索引、memory 或 provider 调用 | 预算/配额/并发/bytes/wall time 无剩余 | SC-16、SC-40 |
| T09 事实篡改/隐瞒 | projection 覆盖 EventLog、删除失败 attempt、修改 audit | cursor/hash/sequence 或来源不一致 | SC-02、SC-31、SC-32 |
| T10 供应链投毒 | 未锁定依赖、恶意插件、换包、未签 binary | digest、license、SBOM、provenance、trust root 缺失 | SC-25 至 SC-30 |
| T11 隐私/保留违规 | 默认收集 prompt、跨项目检索、删除只删 UI | purpose、scope、retention、tombstone 未传播 | SC-21 至 SC-24 |
| T12 事故误恢复 | crash 后自动重发、旧 revision 继续写、unknown 被归零 | 无 reconcile、fence、backup 或恢复证据 | SC-15、SC-33、SC-37 |

## 4. 目标代码设计

### 4.1 领域对象

对象放在 kiana-domain，只承载值、状态转移、不变量和序列化合同，不读文件、网络、进程或 secret：

~~~rust
struct Principal {
    principal_id: PrincipalId,
    issuer: IssuerId,
    authn_method: AuthnMethod,
    assurance: AssuranceLevel,
    session_id: SessionId,
}

struct SecurityContext {
    principal: Principal,
    project: ProjectId,
    trust: ProjectTrustSnapshot,
    role_assignment: RoleAssignmentSnapshot,
    policy_revision: PolicyRevision,
    authority_epoch: AuthorityEpoch,
    data_epoch: DataEpoch,
}

struct Grant {
    grant_id: GrantId,
    parent: Option<GrantId>,
    scope: CapabilityScope,
    data_boundary: DataBoundary,
    expires_at: Timestamp,
    payload_digest: Digest,
    fence: FenceToken,
}

struct Approval {
    approval_id: ApprovalId,
    approver: PrincipalId,
    command_kind: CommandKind,
    payload_digest: Digest,
    target_digest: Digest,
    scope: CapabilityScope,
    expires_at: Timestamp,
    decision: ApprovalDecision,
}

struct SecretRef {
    ref_id: SecretRefId,
    provider: ProviderId,
    account: AccountId,
    purpose: Purpose,
    expires_at: Timestamp,
    one_shot: bool,
}

struct AuditRecord {
    audit_id: AuditId,
    operation_id: OperationId,
    decision: SecurityDecision,
    reason: ReasonCode,
    actor_ref: PrincipalId,
    source_cursor: SourceCursor,
    evidence_refs: Vec<EvidenceRef>,
}
~~~

还需要稳定的 DataClass、Purpose、RiskLevel、ResourceBudget、IdempotencyKey、ExtensionAttestation、RetentionPolicy、DeletionTombstone、Incident、EvidenceManifest 和 UnknownReason。这些对象都禁止携带原始 secret；对外 DTO 使用版本化 envelope，未知 major 版本拒绝。

### 4.2 端口与职责

| 端口/组件 | 责任 | 明确禁止 |
|---|---|---|
| IdentityResolver | 把入口会话、OS/local identity 或 OAuth assertion 解析成 server-owned Principal | 不接受 caller 自报 role/project 作为事实 |
| PolicyEvaluator | 以 immutable SecurityContext、CommandIntent、Grant、Approval、Budget、DataBoundary 和 extension attestation 计算决定 | 不执行 shell、HTTP、MCP 或模型调用 |
| SecretStore | 解析 SecretRef、短期 lease、轮换、吊销和审计 | 不向模型、EventLog、Metric 或 UI 返回原值 |
| AuditSink | 追加 decision/observation/incident evidence，做 schema 校验和 redaction | 不把普通 log 变成事实源 |
| DataGovernancePort | 分类、purpose 检查、retention、legal hold、删除传播、导出 manifest | 不直接绕过 ControlPlane 删除事实 |
| SupplyChainVerifier | 检查 digest、签名、SBOM、provenance、license 和 advisory | 不因“本地路径”自动信任 |
| SandboxPort | 将 permit 映射到文件、进程、网络、环境和资源 profile | 不接受模型/插件自行扩大 profile |
| ClockPort / RandomPort | 提供可测试时间、nonce、TTL 和 replay 边界 | 不用 wall clock 作为唯一顺序 |
| EventStore / LeaseStore | CAS、append、flush、cursor、fence、recovery | 不允许 projection/cache 原地改事实 |

### 4.3 安全决定算法

所有 entrypoint、scheduler、workflow、connector、swarm 和 harness capability 请求都调用同一个算法：

~~~text
1. parse versioned envelope; reject unknown major, malformed IDs and oversized payload
2. resolve server-owned Principal, session, ProjectTrust and RoleAssignment
3. classify input/output and determine requested Purpose/DataBoundary/RiskLevel
4. load policy/config/authority/data epoch and extension/provider attestations
5. evaluate deny rules first: trust, identity, scope, approval, budget, path, endpoint, retention
6. intersect parent/template/department/project/packet/approval grants; never union them
7. bind exact command kind, target, payload digest, config revision, workflow digest and idempotency key
8. append AdmissionCommitted with CAS; reserve budget and issue short permit/lease
9. Broker rechecks permit, fence, path/endpoint and secret lease immediately before effect
10. append Started/Observed/Succeeded/Failed/Unknown; flush before acknowledging durable completion
11. project redacted views; update quota, audit, incident and retention indexes
12. on crash, timeout, cancellation or missing observation, keep Unknown and require reconcile
~~~

决定类型只有 allow、deny、needs_approval 和 unknown。unknown 只用于事实/外部观察不完整的情况，不能作为模型请求的“软允许”。

### 4.4 错误和证据

稳定错误至少分为：

~~~text
AUTH_*       identity/session/role/project trust
POLICY_*     scope/approval/policy revision/epoch
DATA_*       classification/purpose/boundary/retention/deletion
SECRET_*     ref/lease/audience/rotation/revocation/redaction
EXT_*        manifest/digest/signature/license/provenance
FS_*         root/path/symlink/generation/TOCTOU
NET_*        endpoint/DNS/SSRF/audience/replay
RESOURCE_*   quota/queue/concurrency/bytes/wall time
FACT_*       CAS/cursor/sequence/redaction/projection
UNKNOWN_*    timeout/EOF/cancel/receipt/reconcile
~~~

错误响应只返回稳定 code、reason class、operation id、retryability、remediation 和 redacted evidence refs；不回显令牌、完整 prompt、环境、命令行或任意 provider 错误原文。

## 5. 数据分类、目的和生命周期

### 5.1 分类

| DataClass | 例子 | 默认存储 | 默认出边界 |
|---|---|---|---|
| Public | 公开文档、公开依赖元数据 | 普通 artifact/index | 可按 purpose 共享 |
| Internal | 运行状态、非敏感指标、组织目录 | EventLog/projection | 仅同一组织/项目授权范围 |
| Confidential | 私有代码、工单、评测样本、模型配置摘要 | 加密存储，最小索引 | 禁止默认 provider/telemetry 外发 |
| Restricted | prompt、tool payload、个人数据、审计详情 | 加密、字段级访问、短保留 | 必须有 purpose、scope、审批或脱敏 |
| Secret | API key、OAuth refresh token、cookie、私钥 | SecretStore/HSM/keychain | 只给 Broker 单次 lease，禁止持久事件 |
| Regulated | 受法律/合同约束的数据或删除请求 | 专用策略、legal hold、受控导出 | 需要数据主体、地域和保留规则 |

分类不能只看字段名：来源、目的、项目、租户/组织、地域、处理方式、下游接收方和组合后的再识别风险都进入 DataLabel。未知分类按更严格等级处理。

### 5.2 Purpose 与最小共享

可用目的包括 execution、audit、debug、evaluation、support、export、recovery 和 retention。每次读取、复制、provider 请求、telemetry 发送、索引和导出都带 purpose；目的改变要重新授权。默认策略：

- execution 只拿当前 command 所需字段；
- audit 保留决定输入、摘要、引用和时间，不保留 secret/raw payload；
- debug 默认关闭 Restricted/Regulated 内容，启用必须有短期 approval；
- evaluation 使用脱敏或合成 fixture，结果绑定数据版本和 purpose；
- support/export 生成最小、带过期时间的 manifest，不能从 UI 直接复制原始库；
- recovery 只能在 quarantine root 中读取，激活前仍受 DataBoundary 和 secret rebind 检查。

### 5.3 保留与删除

写入路径执行以下顺序：

~~~text
ingress classification
  -> purpose and data-boundary check
  -> redaction/tokenization
  -> append fact + retention label
  -> projection/index/cache/export policy
  -> scheduled retention evaluation
~~~

删除请求不是“删一条 UI 记录”：

1. 认证主体提交带 scope、purpose、legal basis 或 legal-hold 状态的 DeleteRequest。
2. ControlPlane 对 Event、Artifact、Memory、Index、Cache、Telemetry、Export 和 backup dependency 计算影响面。
3. 追加 DeletionRequested 和 DeletionTombstone，提高 data epoch；旧 projection 不能继续向外提供对象。
4. 对可擦除 payload 执行加密擦除或物理删除；不可改写的 EventLog 只保留最小不可识别事实和 tombstone，禁止把原文重新投影。
5. 传播到搜索索引、memory、缓存、通知、导出包和外部副本；每个 adapter 返回 receipt 或 unknown。
6. 生成删除 manifest，列出 source cursor、对象摘要、完成/unknown 项和 limitations；未确认的外部副本进入 incident/reconcile。
7. legal hold、security incident hold 和备份依赖在单独策略中声明；不能悄悄延长保留或绕过 hold。

Secret 不进入 EventLog，因此吊销、轮换和 provider-side deletion 以 SecretStore receipt 为准。删除证明最多说明 Kiana 管辖的数据根已按 manifest 处理，不声称外部系统已经物理删除。

## 6. 端到端处理流程

### 6.1 启动、配置和扩展信任

~~~text
resolve binary/config/project roots
  -> verify ReleaseManifest, lockfile digest, SBOM, signature/provenance
  -> ProjectTrust check before loading project skill/plugin/hook/MCP
  -> parse immutable ConfigSnapshot and SecretRef handles
  -> acquire data/authority lease and fence stale instance
  -> publish redacted startup decision and HealthSnapshot
~~~

任一 digest、签名、许可证、信任根、项目所有权、配置 schema、SecretRef 或 epoch 不可验证时，进入 deny 或 quarantine；不能因为是本地目录或 loopback 就跳过。

### 6.2 命令和 effect

~~~text
entrypoint envelope
  -> authenticated principal + server-owned project/trust
  -> input classification + purpose
  -> policy/gate/approval/budget/path/endpoint evaluation
  -> canonical admission commit (CAS)
  -> short permit + Broker dispatch
  -> effect observation or result_unknown
  -> append receipt and audit evidence
  -> redacted projections to all four entrypoints
~~~

批准绑定 command_kind + target_digest + payload_digest + scope + expiry + authority_epoch；改一个字段就必须重新批准。Broker 只接受 ControlPlane 发的 permit，handler 不直接读取用户的权限字段。

### 6.3 Secret

~~~text
SecretRef in command
  -> ControlPlane checks provider/account/endpoint/purpose
  -> SecretStore issues one-shot short lease
  -> Broker injects into controlled memory/transport
  -> scrub buffers, argv/env/logs/stdout/stderr
  -> revoke/expire lease and append redacted observation
~~~

provider response、错误、trace、crash dump、backup 和 receipt 都要经过同一 redaction boundary。无法证明 scrub 或下游不记录时，拒绝该路径或降级为无 secret 的能力。

### 6.4 不可信内容和模型输出

输入携带来源层级：system/developer/user/tool/external/project/memory/provider。低信任层可以作为资料，但不能覆盖高信任层。模型生成的 tool call 先通过 schema、DataBoundary、digest、预算和 approval，输出再按目标上下文编码；不会把“模型说已完成”写成业务完成。

### 6.5 MCP、连接器和 Webhook

- stdio MCP 记录 server binary/path、manifest digest、工作目录、环境允许列表和 capability catalog。
- HTTP MCP 记录 canonical resource URI、audience、scope、PKCE/state/redirect 和 provider/downstream token 分离；token 不 passthrough。
- Webhook 校验签名、时间窗口、nonce、source account、event schema、payload digest 和 idempotency；入站事件先变成 typed intent，再回 ControlPlane。
- Connector 只提交 account-bound command 或读取 projection；第三方返回的对象、链接、富文本和错误都按不可信数据分类。

### 6.6 取消、崩溃和事故

~~~text
cancel/timeout/crash
  -> stop new intake and fence permit/lease
  -> observe started process/network/provider attempt
  -> append CancelRequested + Stopped or Unknown
  -> freeze budget/data/export and open Incident when needed
  -> reconcile external idempotency/receipt
  -> explicit close/retry_without_effect/compensate/abandon
~~~

事故关闭必须同时有根因、影响范围、处置动作、证据引用、剩余风险和 reviewer。关闭 incident 不会删除原始未知事实。

<a id="security-compliance-steps"></a>

## 7. 详细实施步骤

每张卡都要求：先实现拒绝路径，再实现成功路径；先有 fixture/故障注入，再接入口；完成时写入 CURRENT_STATUS.md 证据块。依赖是硬前置，∥ 表示可并行但不能绕过共同的合同卡。

### Wave A：合同、基线与决定模型

| ID | 代码落点 | 依赖 | 先证明的拒绝路径 | 交付与成功证据 |
|---|---|---|---|---|
| SC-00 | CURRENT_STATUS.md、docs/module-map.md、专项 manifest | 无 | 把规范、类型、静态审计写成 implemented；遗漏现有 partial/unknown | 资产/入口/控制/证据清单，标注 feature_status 与 proof_level |
| SC-01 | docs threat register、security fixture catalog | SC-00 | 未登记 prompt injection、secret、TOCTOU、replay、retention 或供应链威胁 | T01-T12 abuse case、影响、控制、owner 和回归 fixture |
| SC-02 | kiana-domain security IDs/schema registry | SC-00 | unknown major、重复 ID、digest/epoch/sequence 回退、secret 字段 serde | 版本化对象、生成/解析规则、upcaster 拒绝测试 |
| SC-03 | kiana-domain/kiana-protocol reason codes | SC-02 | 所有拒绝塌缩成 I/O/unauthorized；错误回显原文 | 稳定 AUTH_* 至 UNKNOWN_* code、retryability/remediation 合同 |
| SC-04 | kiana-core SecurityContext、入口身份边界 | SC-02,SC-03 | caller 自报 actor/project/role/trust 被采信；匿名 loopback 直接执行 | server-owned context fixture，四入口同一解析结果 |
| SC-05 | kiana-policy PolicyBundle/DecisionTrace/PolicyRevision | SC-03,SC-04 | policy 版本漂移、默认 allow、无法解释 deny、旧 revision 继续授权 | deny-first evaluator、可重放输入摘要、revision/epoch 绑定 |

### Wave B：身份、权限和审批

| ID | 代码落点 | 依赖 | 先证明的拒绝路径 | 交付与成功证据 |
|---|---|---|---|---|
| SC-06 | kiana-domain/kiana-daemon Principal、session、authn adapter | SC-04,SC-05 | 伪造 session、过期 assertion、主体切换、低 assurance 执行高风险 effect | Principal snapshot、过期/吊销/重启测试；未完成的外部认证保持 partial |
| SC-07 | kiana-core ProjectTrust、RoleAssignment、DepartmentSnapshot | SC-06 | 未信任项目加载资源；wire role 覆盖 server assignment；项目边界混用 | trust/role 来源和 revision 可查询，项目隔离拒绝矩阵 |
| SC-08 | kiana-core authority epoch、session fence、policy refresh | SC-06,SC-07 | 旧进程、旧 token、旧 approval 在撤销/配置变更后继续运行 | monotonic epoch、CAS/fence、late observation 不 resurrect |
| SC-09 | kiana-policy GrantScope intersection、Cell inheritance | SC-07,SC-08 | 子 Cell 权限并集、Grant 转移、读写/网络/secret/外部账户混成一种权限 | typed scope intersection、父子/模板/packet 交集 property tests |
| SC-10 | kiana-core Approval binding、Human Inbox | SC-05,SC-08,SC-09 | 只批准 tool name 不批准 args；过期/重复/跨项目 approval；自批 | exact command/target/payload digest、approver scope、一次性消费和回执 |
| SC-11 | kiana-entrypoints、scheduler/workflow/swarm/connector parity | SC-04,SC-09,SC-10 | 任一入口绕过 DaemonHost/ControlPlane；UI 本地决定 allow | CLI/TTY/Web/Desktop/scheduler/swarm 同一 command fixture 和 zero-handler-on-deny |

### Wave C：effect、隔离、网络和资源

| ID | 代码落点 | 依赖 | 先证明的拒绝路径 | 交付与成功证据 |
|---|---|---|---|---|
| SC-12 | kiana-core PendingInvocation/Permit、CAS、idempotency | SC-08,SC-09,SC-10 | 重复 command、digest/sequence/fence 不匹配、无 admission 直接 dispatch | committed permit、dedup 原 receipt、并发只有一个 effect |
| SC-13 | kiana-capability-broker、kiana-daemon path/TOCTOU | SC-12 | symlink/hardlink、rename、root 漂移、patch lock 过期、检查后文件替换 | root-relative open/update、generation/inode/fence、拒绝零 effect |
| SC-14 | Broker sandbox/network profile、endpoint resolver | SC-12,SC-13 | 任意 URL、DNS rebinding、metadata/loopback escape、跨 DataBoundary | endpoint allowlist、解析后再校验、egress audit、sandbox fixture |
| SC-15 | kiana-runner/kiana-core cancel fencing、Unknown | SC-12,SC-14 | cancel 后仍发新请求；timeout/EOF 自动 retry；unknown 归零或成功 | Started/Stopped/Unknown 生命周期、reconcile inbox、无双终态 |
| SC-16 | kiana-core/Broker quotas、bounded channels、backpressure | SC-09,SC-12 | 无限输入/输出/queue/retry/process/bytes；退避占槽；超额仍 dispatch | 每 Cell/run/project/provider 维度的 reservation、释放和容量指标 |
| SC-17 | kiana-daemon MCP/connector/webhook ingress | SC-06,SC-10,SC-14,SC-15 | token audience 不符、passthrough、坏签名、重放、未绑定 account/目的 | typed connector intent、signature/nonce/receipt、下游 token 分离 |

### Wave D：秘密、隐私与数据治理

| ID | 代码落点 | 依赖 | 先证明的拒绝路径 | 交付与成功证据 |
|---|---|---|---|---|
| SC-18 | kiana-domain SecretRef、kiana-ports SecretStore | SC-04,SC-09,SC-17 | wire/log/event 带 secret value；未知 provider/account/目的；SecretRef 可转移 | opaque ref schema、store adapter、sentinel secret negative fixture |
| SC-19 | kiana-daemon secret lease、rotation/revocation | SC-18,SC-15 | lease 过期仍注入、重复使用、吊销后继续请求、恢复复制明文 | one-shot TTL、revoke/rotate receipt、crash/restore 无 secret 泄露 |
| SC-20 | kiana-domain redaction、Broker/Provider/Runner/Event boundaries | SC-03,SC-18 | key 外 secret、Bearer/API key、provider echo、argv/env/stdout/stderr 未脱敏 | schema + recursive text redaction、失败时 fail closed、fixture 覆盖所有出口 |
| SC-21 | kiana-domain DataClass/Purpose/DataBoundary | SC-02,SC-05,SC-20 | 未分类内容进入 provider/trace/index/export；purpose 变更不重审 | ingress label、目的/范围 evaluator、unknown 按高敏等级处理 |
| SC-22 | kiana-core/kiana-eventlog RetentionPolicy、legal hold | SC-21,PD-05,OA-08 | 默认无限保留、删除 audit 事实、hold 被静默覆盖、保留跨项目 | policy revision、retention scan、hold receipt、source/projection 边界 |
| SC-23 | kiana-core/kiana-eventlog DeleteRequest/Tombstone/data epoch | SC-22,SC-12 | 只删 UI/缓存；旧 projection 恢复原文；未知外部删除标成功 | append tombstone、crypto erase/delete adapter、传播 manifest 和 unknown |
| SC-24 | kiana-query memory/index/cache/export boundary | SC-21,SC-22,SC-23,CM | memory 自批、自写跨 project、索引残留、导出越 scope | candidate/draft/origin/purpose、rebuild-after-delete、最小导出 receipt |

### Wave E：扩展、依赖和发布供应链

| ID | 代码落点 | 依赖 | 先证明的拒绝路径 | 交付与成功证据 |
|---|---|---|---|---|
| SC-25 | kiana-policy ProjectTrust、user/KIANA_HOME/project trust roots | SC-04,SC-07 | 未信任目录的 skill/plugin/hook/MCP 被加载；路径伪造 trust | trust scope、来源优先级、deny reason 和加载前审计 |
| SC-26 | kiana-domain ExtensionManifest/CapabilityCatalog | SC-02,SC-09,SC-25 | manifest 的 allowed-tools 产生授权；摘要/能力/版本漂移仍运行 | manifest digest、capability intersection、read-only/write negative tests |
| SC-27 | kiana-daemon hook/skill/plugin lifecycle、sandbox | SC-14,SC-16,SC-26 | 安装脚本任意 shell/网络/secret；hook 直接改事实或开第二循环 | lifecycle phase、sandbox profile、permit-only callback、卸载/撤销 |
| SC-28 | scripts、CI SBOM、dependency/license/advisory scanner | SC-02,SC-25 | 未锁定依赖、许可证未知、已知高危漏洞仍发布、lockfile drift | lockfile digest、SPDX/CycloneDX manifest、阈值和 quarantine |
| SC-29 | release artifact signing/provenance verifier | SC-28 | 文件名/tag 伪造、builder/source/toolchain 不匹配、subject digest 不符 | ReleaseManifest、SLSA-style provenance、签名/透明日志验证 |
| SC-30 | provider/model/prompt-pack/MCP route attestation | SC-17,SC-26,SC-29 | provider 自报 model/route、未审 prompt pack、下游 token 混用 | route digest、data/use policy、credential/account/audience 绑定 |

### Wave F：审计、事故和跨入口保证

| ID | 代码落点 | 依赖 | 先证明的拒绝路径 | 交付与成功证据 |
|---|---|---|---|---|
| SC-31 | kiana-protocol/kiana-eventlog AuditRecord schema | SC-02,SC-03,SC-05,SC-20 | 审计缺 actor/decision/reason/source cursor；原始 secret/prompt 进入记录 | append-only schema、redacted field contract、versioned event tests |
| SC-32 | kiana-query audit projector、CAS/cursor/rebuild | SC-12,SC-23,SC-31,PD | projection 覆盖事实、cursor 跳过、旧视图标 fresh、查询越 scope | 可删除可重建 projection、hash/cursor/freshness、审计查询授权 |
| SC-33 | kiana-core Incident、Vulnerability、Reconcile workflow | SC-15,SC-22,SC-31,SC-32 | unknown/secret leak/供应链漂移无 incident；关闭后无法追溯 | severity/owner/deadline/evidence、contain/fence/reconcile/close |
| SC-34 | docs control crosswalk、policy registry | SC-01,SC-05,SC-31,SC-33 | 把单测或类型宣称为法规认证；控制与证据没有 owner | SEC/NIST/OWASP/内部控制映射，scope、假设、proof ceiling |
| SC-35 | scripts EvidenceManifest、fixture/cassette registry | SC-29,SC-31,SC-34 | 命令、环境、源码、fixture、限制缺失；证据被手工改写 | digest/source snapshot、命令 argv、退出码、reviewer、签名/链路 |
| SC-36 | CLI/Workbench/Web/Desktop/daemon protocol parity | SC-11,SC-32,SC-35 | 入口显示不同状态、局部 approve、不同 redaction 或本地计算权限 | 同一 receipt/golden trace、deny/unknown/approval 视觉和协议一致 |

### Wave G：验证、发布门和状态回填

| ID | 代码落点 | 依赖 | 先证明的拒绝路径 | 交付与成功证据 |
|---|---|---|---|---|
| SC-37 | crate negative tests、integration fixtures | SC-12,SC-13,SC-15,SC-18,SC-21,SC-31 | 每类越权、重放、泄露、TOCTOU、删除和入口绕过没有 zero-effect 证明 | deny-first matrix、稳定 reason、handler/provider dispatch count 为零 |
| SC-38 | property/fuzz/serialization/replay tests | SC-02,SC-05,SC-09,SC-12,SC-31 | 随机 payload、截断事件、重复 frame、乱序 cursor、unknown major 导致 allow | invariant/property、fuzz corpus、upcaster 和 replay determinism |
| SC-39 | security red-team/eval fixtures | SC-01,SC-20,SC-24,SC-26,SC-30 | prompt injection、间接注入、secret exfil、恶意插件/MCP 描述能触发 effect | attack corpus、预期 deny/approval/unknown、无真实 secret/外部 effect |
| SC-40 | capacity/resource fault injection | SC-16,SC-22,SC-32 | 无界 queue、输出洪泛、磁盘满、clock rollback、provider 429/5xx 后越额 | bounded latency/queue/bytes、backpressure、恢复后 reservation 无泄漏 |
| SC-41 | .github/workflows、release scripts、security gate | SC-28,SC-29,SC-34,SC-37,SC-40 | fmt/test/scan 失败仍发布；只跑 happy path；artifact 无签名/manifest | CI 阻断规则、offline focused commands、签名/审计/证据检查 |
| SC-42 | scripts smoke、recovery/retention rehearsal | SC-23,SC-32,SC-33,SC-41 | 重启/恢复/删除/重放后丢事实、旧租约继续 effect、retention 破坏 evidence | local durable rehearsal、quarantine/restore/reconcile、限制清单 |
| SC-43 | CURRENT_STATUS.md、module map、review record | SC-34,SC-35,SC-36,SC-41,SC-42 | 只因卡片完成或单测通过就 Promote；limitations 未记录 | 每卡 evidence block、reviewer 签字、feature/proof 分离、下一步明确 |

## 8. 执行波次、依赖和现有专项接点

~~~text
Wave A  SC-00 -> SC-01 -> SC-02 -> SC-03 -> SC-04 -> SC-05
Wave B  SC-06 -> SC-07 -> SC-08 -> SC-09 -> SC-10 -> SC-11
Wave C  SC-12 -> SC-13 ∥ SC-14 -> SC-15 -> SC-16 -> SC-17
Wave D  SC-18 -> SC-19 -> SC-20 -> SC-21 -> SC-22 -> SC-23 -> SC-24
Wave E  SC-25 -> SC-26 -> SC-27 ∥ SC-28 -> SC-29 -> SC-30
Wave F  SC-31 -> SC-32 -> SC-33 -> SC-34 -> SC-35 -> SC-36
Wave G  SC-37 ∥ SC-38 ∥ SC-39 -> SC-40 -> SC-41 -> SC-42 -> SC-43
~~~

与已有专项的边界：

| 既有专项 | 安全专项承接 | 不重复建设 |
|---|---|---|
| CP-*、P0-A/B/F/G/J1 | identity、admission、approval、budget、cancel、fence、Unknown 接点 | 不在 provider、UI、workflow 或 connector 再造授权 |
| CAP-*、P4-J7-* | permit、sandbox、MCP/provider endpoint、usage 和 effect 证明 | 不把 capability catalog、provider response 或 token 当安全事实 |
| ER-*、PD-* | append/CAS、Receipt、replay、backup、migration、retention、delete projection | 不建立第二 EventLog 或用 projection 覆盖事实 |
| CM-*、EXT-* | memory purpose、candidate、ProjectTrust、skill/plugin/hook lifecycle | allowed-tools、memory origin、manifest 不能扩大 Grant |
| INT-*、NM-*、UI-* | connector/webhook ingress、通知、四入口 parity 和 redacted projections | 实时流、消息和 UI 不构成送达、授权或审计事实 |
| OA-*、EQ-*、DEP-*、BQ-* | telemetry privacy、security eval、release/restore、quota/resource gate | metrics、评测、部署脚本和成本账本不能替代 EventLog/ControlPlane |

## 9. SEC 条款映射与验收门

| 宪法条款 | 主要 SC 卡 | 最低必须可证明的结果 |
|---|---|---|
| SEC-01 真实身份绑定 | SC-04、SC-06 至 SC-11 | server-owned principal、项目/角色绑定、匿名/伪造字段拒绝；外部认证缺失保留 partial |
| SEC-02 权限单调缩减 | SC-07 至 SC-10、SC-26 | grant 只交集、不能转移/并集，approval 精确绑定 |
| SEC-03 Cell 资源上限 | SC-09、SC-16、SC-40 | 子 Cell 不超过父级预算、并发、输出、wall time、bytes |
| SEC-04 外部/物理能力 | SC-12 至 SC-17、SC-30 | exact permit、二次 effect 检查、审批、idempotency；无 adapter 的能力保持 not_supported |
| SEC-05 Secret 不出 Broker | SC-18 至 SC-20、SC-39 | sentinel 在 event/log/argv/env/stdout/stderr/trace/backup 全链路拒绝泄露 |
| SEC-06 Loopback 不是认证 | SC-04、SC-06、SC-11、SC-36 | loopback、Host/Origin、页面 token 不能冒充 durable principal |
| SEC-07 路径与 TOCTOU | SC-12 至 SC-14、SC-37 | root-relative、generation/fence、symlink/rename/替换均 zero effect |
| SEC-08 Cancel fencing | SC-08、SC-12、SC-15、SC-42 | cancel 后不新增 effect；已启动工作有 observation 或 Unknown |
| SEC-09 Unknown 一等状态 | SC-12、SC-15、SC-33、SC-42 | unknown 不成功、不归零、不盲重试；reconcile 可追踪 |
| SEC-10 审计与事实源 | SC-02、SC-31 至 SC-36 | EventLog append-only，审计/Receipt 可关联、可重放、可重建 |
| SEC-11 不可信输入 | SC-17、SC-21、SC-24 至 SC-30、SC-39 | 输入分层、ProjectTrust、extension digest、memory candidate 和 MCP trust |
| SEC-12 资源耗尽 | SC-16、SC-22、SC-40、SC-41 | 所有输入、执行、队列、索引、保留和重试都有上限与背压 |

### 9.1 发布门

| Gate | 条件 | 允许的证明等级 |
|---|---|---|
| S0 contract | SC-00 至 SC-05、reason/schema/fixture registry | source，不能宣称 enforcement |
| S1 deny path | SC-06 至 SC-24、SC-37 至 SC-39 的拒绝矩阵通过 | 相关局部可到 local_behavior；仍需逐条记录限制 |
| S2 durable evidence | EventLog/CAS/replay/recovery/delete/incident manifest 可跨进程重建 | 覆盖对象可到 durable；外部 effect 仍可能 unknown |
| S3 opt-in live | 明确 provider/connector、真实身份、密钥、网络、回执和人工批准，且有逐连接证据 | 只可把对应连接提升到 live；不传播到其他连接 |
| S4 physical | 独立 safety controller、物理确认、现场回执和事故演练 | 当前保持 not_supported，除非另有书面决策和证据 |

任何 Gate 失败都阻断 Promote；不能靠删断言、#[ignore]、改期望值或把 unknown 改成 failed 来变绿。

## 10. 验证命令和证据格式

实现每个 SC 卡时，按风险选择最小聚焦命令；daemon/control-plane 测试串行：

~~~text
cargo fmt --all --check
cargo check --workspace --locked --offline
cargo test -p kiana-domain --locked --offline -- --test-threads=1
cargo test -p kiana-core --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --locked --offline -- --test-threads=1
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/release-smoke.sh
bash scripts/harness-golden-smoke.sh
~~~

命令需要根据受影响 crate 缩小；以上只是验证菜单，不是本次已经执行的回执。每张卡在 CURRENT_STATUS.md 写：

~~~text
source_snapshot:
worktree_status:
command_argv:
cwd·environment:
fixture·cassette:
exit_code:
status change:
proof-level change:
limitations:
reviewer:
~~~

证据还要包含 decision digest、operation/run/attempt、authority/data epoch、source/projection cursor、redacted artifact refs 和 zero-effect 计数。若 fixture 是 fake provider、local connector、临时 key、离线 store 或未签名 extension，必须在 limitations 明写，不能写成 live、physical 或外部业务 outcome。

## 11. 当前状态和交接规则

本文件完成的是设计和可执行拆分，当前 feature_status 仍是 target/partial/deferred 的混合状态。已有证据显示本地 ProjectTrust、部分事件 redaction、部分 cancel/Unknown 和 shell/Broker sentinel 已有局部 local_behavior；authenticated principal、完整 SecretRef 链路、全路径 TOCTOU、durable audit projector、跨进程恢复、供应链验证、删除传播和真实连接器回执仍需逐卡验收。准确状态必须回写 CURRENT_STATUS.md，不要从本专项表格反推。

专项完成后，module-map.md 第 20 模块只需要链接到本文件和总路线图；实现仍必须遵守 DaemonHost -> ControlPlane -> Broker -> EventLog -> Receipt。任何新增代码若会产生第二执行循环、权限并集、未过 ProjectTrust 的资源加载、秘密出 Broker、或把 transcript/metric/UI 设为事实源，应在 code review 阶段直接拒绝。
