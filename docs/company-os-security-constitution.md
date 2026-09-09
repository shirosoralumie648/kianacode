# Kiana Company OS 安全宪法与验收策略

> 文档性质：规范与验收合同，不是当前能力声明。
> 总体设计：[`company-os-design.md`](company-os-design.md)
> 实施索引：[`company-os-implementation-outline.md`](company-os-implementation-outline.md)
> 当前最高证明等级：`local_behavior`。

> **本文速览（导读，非规范）**
>
> - **讲什么**：十二条不可违反的安全条款（SEC-01 到 SEC-12，全部 `SEC-P0`），每条都写清设计意图、代码强制点、测试证据要求和当前真实状态（状态标记定义见 §1.1）；外加 plan / analyze 可读取并逐条校验的机器可读宪法契约、R0–R5 风险能力矩阵、带证据引用的负向验收矩阵、发布门和当前边界声明。
> - **回答的问题**："什么事绝对不能发生，以及怎么证明它确实不会发生。"
> - **地位**：整个仓库优先级最高的文档之一——安全宪法压过任何便利功能；每项能力必须先证明拒绝/失败/取消路径，才允许庆祝成功路径。
> - **什么时候读**：实现任何有副作用的能力之前；判断"能不能宣称某项安全保证"时。
> - **已定案**：entrypoint auto-approve 保留但严格收窄——只覆盖 `LocalWrite`，开关默认关闭，每次放行必须由 ControlPlane 追加带「自动批准」标记的事件并在 Receipt 可见（SEC-01 末段）。
> - **开放决策**：代码 `RiskLevel` 四档与 R0–R5 的映射（§3 末段）——本文只标注现状和待决问题，不自行拍板。
>
> 术语看不懂先查 [`company-os-overview.md`](company-os-overview.md) 的白话词典。

## 1. 证明等级

| 等级 | 含义 |
|---|---|
| `source` | 源码或文档中存在定义 |
| `local_behavior` | 固定本机环境、受信项目和 cassette/fake-script 下可观察 |
| `durable` | 重启、崩溃和重放后仍可证明 |
| `live` | 真实第三方 provider 或外部服务 |
| `physical` | 真实设备或物理世界效果 |

只有同时具备设计意图、代码强制、拒绝路径、故障路径和测试引用，规则才能标记为 `code_enforced`。只有有绑定源码快照的测试结果，才能标记为 `proven_*`。

### 1.1 状态标记

条款 Status 行只使用下列四个标记；它们不是新的 `feature_status` 枚举，必须映射到 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 的既有状态：

| 标记 | 含义 | 与 CURRENT_STATUS `feature_status` 的映射 |
|---|---|---|
| `code_enforced` | 同时具备设计意图、代码强制点、拒绝路径、故障路径和测试引用，规则本身已被强制 | 不是 `feature_status` 取值，是条款成熟度标记；当前无一条达到 |
| `partial` | 代码路径部分存在且已有负向证据，但存在已声明缺口 | `partial` |
| `intent_only` | 只有设计约束，代码强制点或负向证据不足，不得据此宣称能力 | 账本最低档：`target`，或带已知绕过口子的 `partial` |
| `not_supported` | 当前必须拒绝或显示不可用，不得伪造成功 | `not_supported` |

当前 SEC-01–SEC-12 没有任何一条标记为 `code_enforced`：全部条款都至少缺一项（完整代码强制点、拒绝路径、故障路径或绑定源码快照的测试引用）。Status 行只是当前事实的快照摘要，权威状态和证据以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。

## 2. 宪法条款

**条款优先级**：SEC-01–SEC-12 全部为 `SEC-P0`。安全宪法不设低优先级条款——任何一条不满足对应负向测试即阻断发布（见 §5）。这里的 `SEC-P0` 是安全条款优先级，与 [`company-os-ui-ux.md`](company-os-ui-ux.md) §13 的 UI P0–P4（界面实施顺序）和 [`company-os-spec-index.md`](company-os-spec-index.md) §7 的实施阶段 P0–P6（工程推进顺序）不是同一个维度，不得互相换算。

**已定案（2026-09-08，产品主人授权按推荐方案执行）**：

- entrypoint auto-approve 保留但严格收窄：只覆盖 `LocalWrite`，开关默认关闭，每次放行必须追加带「自动批准」标记的事件并在 Receipt 可见，更高风险一律弹审批（规范见 SEC-01 末段）。

**开放决策（规范空白，本宪法不自行拍板）**：

- 代码 `RiskLevel` 四档与 R0–R5 的版本化映射（现状与待决问题见 §3 末段）。

### SEC-01 真实身份绑定

**Design intent**：当请求缺少 authenticated principal，或请求中的 actor、组织、角色、部门、session、run 与服务端绑定不一致时，ControlPlane 必须拒绝请求，且 Broker 调用数为零。

**Code enforcement**：Daemon 从受保护入口解析 principal；body 中的 `actor_id`、`role_id`、`department_id`、`agent_id` 和 `trust` 只能作为声明，不能授予身份或权限。Approval 绑定 actor、session、精确 payload digest、目标、policy version、nonce 和 expiry。

**Test evidence**：伪造 actor、修改 role/department、复用他人 session/approval；断言稳定错误码、无副作用和 `command.rejected` 事件。

**Status**：`partial`——DaemonHost 已使用固定本地主体并从 stored ProjectTrust 派生项目可信度，wire 中的 trust/profile 只能作为声明（CURRENT_STATUS P1-01，2026-08-31 host-derived ProjectTrust authority）；Web 已有进程内页面 token，但不构成 durable principal（P1-01，2026-09-07）；role/department assignment 与 durable authenticated principal 未完成（P1-01，2026-09-02 已证明进程内 session role/department 绑定，不等于 durable principal）。

**Auto-approve 边界（已定案，2026-09-08）**：entrypoint auto-approve 保留但严格收窄，只允许 `RiskLevel::LocalWrite` 一档，且必须同时满足：用户已显式开启开关、开关默认关闭、project 已受信。`ExternalSideEffect`、`Critical` 以及 R3 及以上一律弹审批，绝不自动放行；R4 二次确认，R5 默认禁止自治并要求独立 safety controller。每次自动批准必须由 ControlPlane 追加一条带「自动批准」标记的事件（含 actor、开关来源、payload digest、目标、policy version、expiry），并投影到 Receipt，用户可见；没有该事件不得宣称发生过自动批准。自动批准不构成 durable 授权证据，不得用于重启恢复、重放或跨进程续跑——重启后自动批准一律失效，按默认暂停处理，用户显式恢复才继续。

**Test evidence（auto-approve）**：开关关闭时 `LocalWrite` 必须弹审批；开关开启时每次放行都产生带标记事件且 Receipt 可见；`ExternalSideEffect`/`Critical`/R3+ 在开关开启时仍必须弹审批；重启后未显式恢复不得自动续跑；缺少「自动批准」事件的放行按 fail-closed 拒绝。

### SEC-02 权限单调缩减

**Design intent**：子 Cell 的有效权限必须是所有上级边界的交集：

```text
child_grant ⊆ parent_grant
  ∩ template_grant
  ∩ department_policy
  ∩ project_policy
  ∩ packet_scope
  ∩ approval_scope
```

**Code enforcement**：只有 ControlPlane 能签发或撤销 Grant；Grant 不可通过 Artifact、消息、SecretRef 或 URL 转移。读、写、删除、网络、进程、secret、外部账户和物理动作必须是独立权限。

**Test evidence**：随机生成 parent/child grants，任何 child 超集都拒绝；child 默认 `delegation_allowed=false`。

**Status**：`partial`——role/department/path/policy 有部分实现（CURRENT_STATUS P1-01，2026-09-02 immutable session role/department；P3-01，2026-08-31 进程内 Cell/Grant registry）；完整 Cell Grant 与子权限单调缩减的全面证明尚未实现，`local_behavior` 不能升级为全面证明。

### SEC-03 细胞资源上限

**Design intent**：Agent 不能创建新 root、无限子节点或无限重试规避配额。

**Code enforcement**：服务端持久化并原子预留 root total cells、parent max children、max depth、全局并发、spawn rate、attempt/retry、TTL、token/tool/effect/storage/CPU 和预算。

**Test evidence**：达到任意上限时原子拒绝且不泄漏预算、锁或 Grant；父级 cancel/fail/retire 级联撤销后代。

**Status**：`partial`——进程内 CellRegistry reserve/commit、Cell/Grant/Budget 绑定与 lease accounting 已有证据（CURRENT_STATUS P3-01，2026-08-31：Cell lifecycle admission、capability scope/budget fence、malformed Cell scope fail-closed）；durable projector、AgentInstance/SpawnPlan 完整生命周期、跨重启级联撤销未证明。

### SEC-04 外部和物理能力

**Design intent**：支付、预订、发信、下单、解锁、驾驶和设备控制不能由 Agent 自主决定。

**Code enforcement**：所有外部效果只能经过服务端 identity、短期 exact Grant、最终 payload digest、预算、幂等键和有效 Approval 后由 Broker adapter 执行。R3 每次确认，R4 二次确认，R5 默认禁止自治并要求独立 safety controller。SEC-01 的 entrypoint auto-approve 绝不覆盖本条：任何外部或物理能力一律弹审批，开关开启也不例外。

**Test evidence**：未确认、payload 漂移、过期 approval、重复请求、provider 注入和超预算均无 effect；确认后只能执行一次并可核验。

**Status**：当前无真实支付、旅行、外卖或 IoT adapter，`not_supported`。

### SEC-05 Secret 不出 Broker

**Design intent**：secret 原值不得进入 prompt、transcript、event、Receipt、stdout、stderr、argv、环境、缓存、子 Cell 输入或 provider 回显。

**Code enforcement**：只传 opaque SecretRef；Broker 在单次 invocation 的受控内存中解析，Grant 绑定 provider/account/endpoint/目的，默认不可委托、短 TTL、单次使用。

**Test evidence**：向 fake broker 注入 sentinel secret，扫描所有输出、日志、事件、Receipt、错误、子进程参数和缓存，原值出现即失败。

**Status**：当前主要是设计约束，`intent_only`。

### SEC-06 Loopback 不是认证

**Design intent**：只允许 `127.0.0.1` / `::1` 监听；监听范围不能替代身份认证。

**Code enforcement**：目标实现使用 per-instance bearer 或受保护 Unix socket，校验 Host/Origin、CSRF 和 session ownership；health 可按策略例外，mutation 必须认证。

**Test evidence**：绑定 `0.0.0.0`、LAN、公网或解析后的非 loopback 时拒绝；无 token、错误 token、错误 Origin、他人 session 对 mutation 无状态和文件变化。

**Status**：`partial`——已有进程内随机 token、exact Host 与绑定地址 Origin 拒绝（CURRENT_STATUS P1-01，2026-09-07：`web_rejects_wrong_origin_and_host_without_mutating_trust`、`web_rejects_foreign_bearers_and_sessions_without_mutation`），属 transport + 进程内证据；durable principal、跨进程 session recovery、CSRF 与 OS 级边界未证明。

### SEC-07 路径与 TOCTOU

**Design intent**：权限检查、锁定和实际 effect 必须针对同一 canonical root、文件身份和版本完成。

**Code enforcement**：使用 fd-relative / `openat` / no-follow 或隔离工作树；拒绝 `..`、绝对逃逸、symlink/hardlink/rename/mount swap；锁后执行前重验 file identity/version/cwd/env。

**Test evidence**：授权后执行前替换 symlink、hardlink、目录 rename、文件版本和 cwd；返回 `path_changed` 或 `precondition_failed` 且无越界写。

**Status**：`partial`——已有 canonical/path allow 检查与 symlink/hardlink/file-identity、descriptor-anchored commit 证据（CURRENT_STATUS P1-04，2026-09-07 descriptor-anchored update commit；2026-08-31 process/filesystem fencing）；完整 effect-time TOCTOU 防护（fd-relative/no-follow 全链路、mount swap、锁后执行前全量重验）未证明。

### SEC-08 Cancel fencing

**Design intent**：取消只能阻止未来 effect；无法确认停止时不得返回 completed。

**Code enforcement**：

```text
CancelRequested
→ authorization fence
→ dispatch fence
→ handler fence
→ runner stop
→ process-group/cgroup termination
→ durable result confirmation
```

Shell、孙进程和 MCP 子进程必须可停止或进入 Unknown。

**Test evidence**：授权前、审批中、dispatching、handler started、effect done/result lost、重复 cancel 和 parent cancel 均有时序测试。

**Status**：`partial`——packet cancellation 已显式经过 `CancelRequested` → `Cancelled` → `Retired`（CURRENT_STATUS P1-03，2026-08-31 packet Cell cancellation lifecycle）；shell timeout 要求 process-group stop confirmation，无法确认时进入 `shell_result_unknown`（P1-03，2026-08-31）；direct cancel 只接受无歧义的 `cancelled:` 响应，其余响应记录 `run.result_unknown`（P1-03，2026-09-02）。durable cancel generation、cgroup/进程树完整停止证明与跨重启恢复未证明，cancellation watch 仍不等于进程树终止。

### SEC-09 Unknown 一等状态

**Design intent**：副作用或证据无法确认时，结果必须为 `result_unknown`，不得自动映射为 failed/success 或盲目重试。委派（delegation）是 ControlPlane 的 assign / handoff 操作，不是模型可见工具；模型只能在 WorkPacket 内容里提出建议，实际目标由 ControlPlane 按 role / grant 白名单解析。

**Code enforcement**：使用 durable intent、idempotent invocation、provider receipt/reconcile；补偿、退款和取消均是新授权副作用。`DelegationPacket` 的 max_turns / max_messages / termination_predicate / handoff_allowlist 来自 grant / template 而非模型文本，超预算自动终止并写事件；子 Cell 失败写 typed `ChildFailureReport{child_cell_id, reason_code, error_class, partial_output_refs, result_unknown, retryable, policy_snapshot}` 并随 EventLog 持久化，delegation_started / completed / failed / reconciled 事件带 parent / child 关联。合并裁决（MergeDecision，当前域类型 `MergeReceipt`）必须拒绝任何 `result_unknown=true` 的 child 输出：含 unknown 的合并结果是 Unknown，而不是部分成功。

**Test evidence**：provider timeout、daemon crash、event append 失败、late response、部分 JSONL、cancel 后未确认停止；断言 Unknown、pending reconciliation 和禁止自动重试。另断言：模型试图把 delegation 当第六个工具或自选 handoff 目标时 deny；未知 handoff 目标被白名单拒绝；超过 max_turns / max_messages 自动终止并写事件；子 Cell 返回 `result_unknown` 时合并裁决拒绝合并并保持 Unknown。

**Status**：`partial`——receipt replay 已优先识别持久化 `run.result_unknown`，无 `run.completed` 时返回 Unknown 而不是 Completed，且不完整/冲突/无终止事件的历史投影为 Unknown/Failed/Cancelled（CURRENT_STATUS P1/P2，2026-09-02 receipt-read and terminal replay）；未确认 cancel 与停止不确定的 shell 已进入 Unknown（P1-03，2026-09-02）。reconciliation queue、provider verification 与完整恢复契约未证明。委派即 ControlPlane 操作、`ChildFailureReport` typed 失败与「`result_unknown` 不得进合并裁决」目前尚无代码强制点，属 `intent_only`。

### SEC-10 审计与事实源

**Design intent**：Receipt 是事件事实源的投影，不是 Agent 或 UI 自报。

**Code enforcement**：Event Store append-only，支持 aggregate/stream version、expected-version/CAS、幂等和重放。事件包含 actor、组织、部门、role、cell、packet、execution、approval、grant、policy、schema、payload digest、结果和异常。

**Test evidence**：乱序、重复、伪造 Receipt、CAS 冲突、事件回滚、重启重放和 Receipt 与状态不一致均被拒绝。

**Status**：`partial`——JSONL EventLog 有限持久化、按 request 序列，已有 stream-read CAS fail-closed 与合法未终止尾记录恢复证据（CURRENT_STATUS P2-01，2026-09-02 / 2026-09-06）；尚非 aggregate/CAS durable authority，durable Session/Run/Invocation projector 与跨进程恢复未证明。

### SEC-11 不可信输入

**Design intent**：模型、网页、provider、MCP、skill、tool description、artifact 和普通消息永不授予权限。模型写入的记忆一律是 candidate，来源由服务端派生，模型不得自批；skill 的 `allowed-tools` 与扩展清单只是声明，不是授权。

**Code enforcement**：所有输入严格 schema、大小、嵌套和 provenance 校验；tool name、arguments、目标和结果重新授权；skill 记录来源 hash 和版本。

- **记忆（F-1）**：`memory.write` 一律落 candidate + draft，默认检索排除；origin 由服务端从捕获通道派生（model / hook / git / user），不读模型传入的 `source` 或 `promote_to`；只有目标层 owner 或操作者显式批准才转 active。instance-scratch 层保持默认可见（临时草稿），持久层默认 candidate 不可检索；模型不得自批。
- **技能与扩展（F-2）**：skill 的 `allowed-tools` 只影响提示与展示，不进入 policy、不构成授权；技能想用任何能力都必须与其它模型工具一样经 broker + policy + approval。扩展 / 插件清单必须声明 `effect`（read-only / read-write）、`required_capabilities`、`network_policy`、`content_hash` / `signature` 与 `requires`（版本 / 能力），安装时校验摘要与兼容性；`read-only` 扩展的任何写操作在 broker 层直接拒绝。声明不等于授权，安装成功也不等于安全验证完成。

**Test evidence**：提示注入、伪造 tool name、超大 args、未知字段、恶意 provider response 和 skill 内容不得提升权限或扩大数据读取。另断言：声明 `allowed-tools: [shell]` 的 skill 不能导致任何未经批准的 shell 执行（fail-closed 回归）；模型经 `memory.write` 写入的记录在显式批准前不可检索、不可自批，伪造 `source` 不改变服务端派生的 origin；缺 `content_hash` 或摘要不符的扩展安装被拒绝，`read-only` 扩展的写调用在 broker 层拒绝且无 effect。

**Status**：`partial`——MCP advertised-schema argument fence（CURRENT_STATUS P1-06，2026-09-06：`unknown_stdio_mcp_tool_is_rejected_before_tools_call`）与 server-owned MCP risk boundary（P1-06，2026-09-02）已有证据；任意 JSON-Schema combinator、完整 provenance/attestation、恶意 skill 与 live provider 输入仍未证明。本条新增的记忆 candidate / 来源服务端派生、skill `allowed-tools` 非授权边界、扩展清单校验与 `read-only` 写拒绝均尚无代码强制点，属 `intent_only`。

### SEC-12 资源耗尽

**Design intent**：本地 API 和 Agent 不能通过无限输入、输出、并发或日志耗尽资源。

**Code enforcement**：对 body、prompt、turn、event、stdout/stderr、tool args、query result、session、artifact、model steps 和并发设置硬上限、分页、背压和超时。

**Test evidence**：超限返回稳定错误码；磁盘满、输出截断、并发峰值和慢 handler 不造成状态泄漏或锁永久占用。

**Status**：`partial`——已有 Web body 128 KiB、prompt 64 KiB、session count、transcript turn 与 projection output 配额及超限 fail-closed 证据（CURRENT_STATUS P1-12，2026-08-31）；完整 event/tool-args/并发/artifact 配额、背压、磁盘满与慢 handler 状态泄漏未证明。

### 宪法校验契约（机器可读，plan / analyze 门禁）

**Design intent**：本宪法的硬约束必须可被 plan / analyze 节点机械读取并逐条校验，而不是只作为人读文档。硬约束集合至少包含：五工具面锁死（永远只有 shell / apply_patch / mcp / memory.search / memory.write 五个模型可见工具）、冻结项（完整清单以规范索引登记的不可启用项为准）、单一执行路径（一切副作用经 ControlPlane）、fail-closed、审批不可绕过。

**Code enforcement**：把上述硬约束抽成版本化的机器可读宪法清单（稳定 clause id + 描述 + 检查器），plan / analyze 节点对每条 clause 输出 `pass` 或 `violation`（稳定 issue code）；任何 `violation` 直接阻断（fail-closed），不得进入 apply / 实现。校验是只读分析，经既有 shell 能力在 policy / sandbox 下执行，不新增模型可见工具、不引入第二条执行路径、不产生副作用。

**Test evidence**：plan / analyze 提议新增第六个模型可见工具、引入第二条执行路径、绕过审批、或启用冻结项时，必须给出 `violation` 与稳定 issue code 并阻断；无 `violation` 的方案正常放行；校验命令只读、幂等、可重跑，且不写任何事件或文件。

**Status**：`intent_only`——当前安全宪法只是文档，尚无 plan / analyze 可读取的机器可读清单与校验器；`kiana workflow validate` 尚未实现。

## 3. 风险等级与能力矩阵

| 风险 | Coding | Office / Work | Search | Commerce / Food | Mobility / Travel | Home / IoT |
|---|---|---|---|---|---|---|
| R0 | read/search/test | read/summarize | query/compare | menu/price query | route/quote query | state read |
| R1 | local patch/draft | notes/report draft | saved shortlist | cart draft | itinerary draft | scene draft |
| R2 | PR draft | form/email draft | recommendation | prefilled order | prefilled booking | schedule draft |
| R3 | push/publish | send/update ticket | external submit | non-payment submission | non-payment reservation | short reversible device action |
| R4 | release/production change | legal/financial submit | n/a | order/payment/refund | ticket/hotel/taxi/change | firmware/pairing/delete |
| R5 | safety-critical production | signing/regulated commitment | n/a | regulated purchase | vehicle control | lock/gas/heater/camera/mic/security |

同一动作按副作用、敏感性、资金、法律、物理危险和可逆性取最高风险，不能用“只是工具调用”降级。

**开放决策（规范空白）——RiskLevel 四档与 R0–R5 的映射**：本矩阵的 R0–R5 是目标风险分级；当前代码只有四档 `RiskLevel`（`ReadOnly` / `LocalWrite` / `ExternalSideEffect` / `Critical`，见 `kiana-domain/src/lib.rs`），两者尚未建立版本化映射（[`company-os-spec-index.md`](company-os-spec-index.md) §6.3 已登记该兼容边界）。待决问题：四档到 R0–R5 的映射表、由谁在何处标注、无法映射时的 fail-closed 规则、以及映射表自身的版本与迁移策略。在映射落地前，不得用代码中的四档反推 R 级，也不得把 `Critical` 直接等同于 R5。

## 4. 验收矩阵

| ID | 场景 | 必须断言 | 证据引用 |
|---|---|---|---|
| ACT-01 | actor 缺失/伪造/不匹配 | 稳定 deny code；Broker 调用数为零 | CURRENT_STATUS P1-01（2026-09-02 direct role/department boundary；2026-08-29 local principal and session ownership） |
| ACT-02 | approval 改 payload、目标、actor、session 或重放 | deny；single-use；原 digest 不变 | CURRENT_STATUS P1-02（2026-08-29 structured approval digest / authorization-context binding；2026-09-07 restart approval proof preflight） |
| ACL-01 | role/department/path/memory 越权 | deny；无文件、网络或数据泄漏 | CURRENT_STATUS P1-01（2026-09-02 immutable session role/department）；P1-03（2026-08-31 Builder path lock） |
| ACL-02 | child grant 超过 parent | 原子拒绝；无权限扩大 | 尚无 child-grant 单调性专项证据；CURRENT_STATUS P3-01（2026-08-31）只覆盖 Cell scope/budget fence，本条待建 |
| CELL-01 | depth、children、root、budget、retry、concurrency 到顶 | 无泄漏预留；级联 revoke | CURRENT_STATUS P3-01（2026-08-31 Cell lifecycle admission；capability scope/budget fence；lease accounting） |
| EXT-01 | payment/travel/order 未确认、漂移、超时 | 无 effect；Unknown 不盲重试 | 无 adapter；CURRENT_STATUS 标记 `not_supported`（SEC-04） |
| EXT-02 | lock/gas/camera/vehicle/firmware 自治 | 默认 deny；无 safety controller 不 dispatch | 无 adapter；CURRENT_STATUS 标记 `not_supported`（SEC-04） |
| SEC-05 | sentinel secret 全链路 | 原值不出 Broker | CURRENT_STATUS P1-05（2026-09-07 shell/Broker sentinel regression）；完整 stdout/stderr/argv/env 链路仍开放 |
| WEB-01 | 非 loopback、无 token、Origin 错误、他人 session | mutation 无状态变化 | CURRENT_STATUS P1-01（2026-09-07 `web_rejects_wrong_origin_and_host_without_mutating_trust`、`web_rejects_foreign_bearers_and_sessions_without_mutation`） |
| FS-01 | symlink/hardlink/rename/version race | `path_changed`/`precondition_failed`；无越界写 | CURRENT_STATUS P1-04（2026-09-07 descriptor-anchored update commit；2026-08-31 process/filesystem fencing） |
| CAN-01 | cancel 与 approval/dispatch/handler race | 状态和事件可证明；无法确认则 Unknown | CURRENT_STATUS P1-03（2026-09-02 cancellation confirmation and pre-signalled fence；2026-08-31 packet Cell cancellation lifecycle） |
| UNK-01 | provider/daemon/event crash | durable pending reconciliation；禁止假成功 | CURRENT_STATUS P1/P2（2026-09-02 receipt-read and terminal replay）；durable reconciliation 与 provider verification 仍未完成 |
| AUD-01 | 乱序、重复、伪造 receipt、CAS 冲突 | reject；重放状态与 live state 一致 | CURRENT_STATUS P2-01（2026-09-02 EventStore stream-read CAS fail-closed；2026-09-06 JSONL valid-unterminated-tail recovery） |
| INP-01 | 伪造 tool name、超大 args、未知字段、恶意 skill | 不提权；不扩大数据读取 | CURRENT_STATUS P1-06（2026-09-06 MCP advertised-schema argument fence；2026-09-02 advertised tool/result boundary） |
| QUO-01 | body/prompt/event/并发超限 | 稳定错误码；无状态泄漏 | CURRENT_STATUS P1-12（2026-08-31 body/prompt/session/turn/projection quotas）；完整 event/并发/backpressure quota 仍未完成 |
| MEM-01 | 模型 `memory.write` 写入与自批 | 落 candidate/draft；服务端派生 origin；自批被拒；默认检索不返回 | 尚无专项证据；本条待建（SEC-11） |
| INP-02 | skill `allowed-tools`、扩展清单与 `read-only` 写 | `allowed-tools` 不产生授权；缺摘要/不符安装拒绝；`read-only` 写调用 broker 拒绝 | 尚无专项证据；本条待建（SEC-11） |
| DEL-01 | 委派与 child failure 合并 | 委派只经 ControlPlane；`result_unknown` 子输出不得进合并裁决 | 尚无专项证据；本条待建（SEC-09） |
| CONST-01 | plan / analyze 违反宪法硬约束 | `violation` 阻断；稳定 issue code；无副作用 | 尚无专项证据；本条待建（机器可读宪法契约） |
| FI-01 | 所有故障注入点 | fail-closed、无权限扩大、无 secret 泄漏 | 分散在 P1-02/P1-03/P1-04/P1-05 各负向回归；尚无统一 fault-injection 矩阵 |

矩阵 ID 与条款对应：ACT-01/ACT-02 → SEC-01，ACL-01/ACL-02 → SEC-02，CELL-01 → SEC-03，EXT-01/EXT-02 → SEC-04，SEC-05 → SEC-05（sentinel secret，不再与 SEC-01 条款撞号），WEB-01 → SEC-06，FS-01 → SEC-07，CAN-01 → SEC-08，UNK-01/DEL-01 → SEC-09，AUD-01 → SEC-10，INP-01/INP-02/MEM-01 → SEC-11，QUO-01 → SEC-12，CONST-01 → 宪法校验契约（机器可读），FI-01 为跨条款故障注入。

## 5. 发布门

发布前必须满足：

- 每项能力都有 owner、拒绝路径、故障路径和证据引用；
- 所有 `SEC-P0` 宪法条款（SEC-01–SEC-12 全部，见 §2）通过对应 negative tests；
- plan / analyze 的机器可读宪法校验存在任何 `violation` 时阻断发布（见 §2 宪法校验契约）；
- 测试绑定 immutable source snapshot；
- 不使用 ignored、flaky、skipped 测试替代证据；
- 未知、审计写失败、身份不确定或 provider 状态不确定都会阻断发布；
- 当前未有 live/physical adapter 的能力必须标记 `not_supported`，不能模拟成功；
- 未签名包和临时安装只能标记为本地开发证据。

## 6. 当前边界声明

> **本节为快照摘要，权威以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本节与账本冲突时以账本为准，并必须立即修正本节。**

当前 Kiana 的安全声明只覆盖受信本地项目、固定本机行为和 `local_behavior` 证据。当前 loopback 限制不等于身份认证，trust 不等于项目安全，Receipt 不等于现实结果验证，Grant/Reviewer/Cell 尚未因“对象存在”而自动成为强制安全边界。

当前实现中的具体限制必须显式保留在证据账本中：

- `DaemonHost` 为 actor 使用固定本地主体，并从经过校验的 stored ProjectTrust 记录派生 project trust；wire 中的 `project_trusted` 和 `permission_profile` 不再授予项目或写权限；role/department 仍是声明，尚无 durable authenticated principal；
- Web 已有进程内随机 token 和 exact Host/Origin 拒绝（`partial`，transport + 进程内证据；CURRENT_STATUS P1-01，2026-09-07），但 session map 和隐式 active 状态仍只属于当前进程；durable principal、跨进程 session recovery 和 OS 级边界未证明，多标签页隔离尚未证明；
- `JsonlEventLog` 目前是进程内锁、按 request 的 sequence 和有限重启读取，不是跨进程 aggregate/CAS durable authority；显式 run receipt 已在投影前校验持久化 `run.authorized` owner，但完整 state/session recovery 仍未完成；
- Builder packet admission 现在额外使用基于项目和写集哈希的 kernel-backed lock file；这只证明跨 ControlPlane 的 admission fencing，不等于 durable Cell/Grant/Budget projector 或 effect-time TOCTOU 防护；
- packet spawn 现在在执行前通过 CellRegistry reserve/commit template、budget、grant 和 supervision，并在终态 retire；这些资源事实仍是进程内 registry，不能宣称 durable Cell/Grant/Budget projector。
- Cell-bound capability request 现在在 dispatch 前由 ControlPlane 绑定 server-owned Cell/Grant/Budget identity，并在 success/failure/cancel/unknown 结果上完成 lease accounting；这仍不等于 durable state projector。
- packet cancellation 现在显式经过 `CancelRequested` → `Cancelled` → `Retired` 并产生 `packet.cancelled`；只有收到 effect stop confirmation 才能把该状态提升为 durable cancelled，当前仍需 process-group/handler fence。
- receipt replay 现在优先识别持久化 `run.result_unknown`，没有 `run.completed` 时返回 `result_unknown` 而不是 Completed；reconciliation queue 和 provider verification 仍未完成。
- 持久 approval 若关联 Run 但当前没有可恢复的 PendingInvocation，现在返回 `approval_continuation_unavailable` 且不 direct-dispatch；独立 direct capability approval 保持兼容，完整 durable Runner recovery 仍未完成。
- Builder path lock 现在由 reservation-scope RAII guard 持有，Cell admission 或 pre-run event failure 会释放锁；这只覆盖本地资源泄漏，不替代 durable registry reconciliation。
- shell timeout 现在要求 process-group stop confirmation；无法确认时返回 `shell_result_unknown` 并进入 `result_unknown`，不能把 exit 124 当作停止成功。
- apply_patch commit 阶段现在使用 per-project kernel-backed lock 并在 lock 后重新验证 snapshots；这只串行化本地 commit，不等于 fd-relative/no-follow effect-time fencing。
- patch lock contention 会等待当前短 commit 窗口而不是伪造成功或盲目失败；等待后仍必须重新验证 snapshots，目标漂移继续 fail-closed。
- 缺少 `cell_id` 但携带 Grant/Budget identity 的 malformed request 现在在 broker 前 fail-closed，返回稳定的 `cell_capability_scope_incomplete`，且不产生 broker effect。
- RuntimeEvent 的嵌套 `Value::String` 现在也经过统一文本 redaction，可遮蔽常见 Bearer/token/API-key 形式；`secret_ref` 仍只保留 opaque reference，完整 stdout/stderr/argv/env 链路扫描仍未完成。
- Web router 现在限制 body 为 128 KiB，prompt 为 64 KiB，超限请求在 DaemonHost/broker 前拒绝；完整 session/artifact quota 和 backpressure 仍未完成。
- capability result event 现在保留经过 redaction 的 Cell/Grant/Budget/request correlation 字段，便于审计关联；这不等于结果 provenance 或 durable audit authority。
- `cancel_run` 现在返回独立的 `cancelled` 状态并写入 `run.cancelled`，但这仍不能等同于已确认所有 effect 停止；Runner/provider/handler 的停止保证和 `result_unknown` 对账仍需实现；
- Harness capability 遇到 `AwaitingApproval` 已有同一 host 的 `PendingInvocation` continuation（CURRENT_STATUS P1-02，2026-08-29：PendingInvocation same-host continuation implemented）；若持久 approval 关联 Run 但当前没有可恢复的 PendingInvocation，则返回 `approval_continuation_unavailable` 且不 direct-dispatch；跨进程/durable Runner recovery 仍未完成；
- Broker handler 返回的结果尚未由统一 descriptor、schema、timeout、quota 和 provenance 契约完整约束。

支付、外卖、打车、酒店、机票、火车票和智能家居在未完成 adapter、身份、审批、幂等、故障对账和测试前，统一显示为 `not_supported` 或 `target`，不得伪造成功。
