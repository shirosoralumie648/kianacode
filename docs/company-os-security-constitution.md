# Kiana Company OS 安全宪法与验收策略

> 文档性质：规范与验收合同，不是当前能力声明。
> 总体设计：[`company-os-design.md`](company-os-design.md)
> 实施索引：[`company-os-implementation-outline.md`](company-os-implementation-outline.md)
> 当前最高证明等级：`local_behavior`。

> **本文速览（导读，非规范）**
>
> - **讲什么**：十二条不可违反的安全条款（SEC-01 到 SEC-12），每条都写清设计意图、代码强制点、测试证据要求和当前真实状态；外加 R0–R5 风险能力矩阵、负向验收矩阵、发布门和当前边界声明。
> - **回答的问题**："什么事绝对不能发生，以及怎么证明它确实不会发生。"
> - **地位**：整个仓库优先级最高的文档之一——安全宪法压过任何便利功能；每项能力必须先证明拒绝/失败/取消路径，才允许庆祝成功路径。
> - **什么时候读**：实现任何有副作用的能力之前；判断"能不能宣称某项安全保证"时。
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

## 2. 宪法条款

### SEC-01 真实身份绑定

**Design intent**：当请求缺少 authenticated principal，或请求中的 actor、组织、角色、部门、session、run 与服务端绑定不一致时，ControlPlane 必须拒绝请求，且 Broker 调用数为零。

**Code enforcement**：Daemon 从受保护入口解析 principal；body 中的 `actor_id`、`role_id`、`department_id`、`agent_id` 和 `trust` 只能作为声明，不能授予身份或权限。Approval 绑定 actor、session、精确 payload digest、目标、policy version、nonce 和 expiry。

**Test evidence**：伪造 actor、修改 role/department、复用他人 session/approval；断言稳定错误码、无副作用和 `command.rejected` 事件。

**Status**：DaemonHost 已使用固定本地主体并从 stored ProjectTrust 派生项目可信度，wire 中的 trust/profile 只能作为声明；Web 仍无真实身份认证，role/department assignment 与 durable principal 未完成，状态为 `partial`。

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

**Status**：当前 role/path/policy 有部分实现；完整 Cell Grant 尚未实现，`local_behavior` 不能升级为全面证明。

### SEC-03 细胞资源上限

**Design intent**：Agent 不能创建新 root、无限子节点或无限重试规避配额。

**Code enforcement**：服务端持久化并原子预留 root total cells、parent max children、max depth、全局并发、spawn rate、attempt/retry、TTL、token/tool/effect/storage/CPU 和预算。

**Test evidence**：达到任意上限时原子拒绝且不泄漏预算、锁或 Grant；父级 cancel/fail/retire 级联撤销后代。

**Status**：当前无完整 AgentInstance/SpawnPlan/Budget/Lease 生命周期，`not_supported`。

### SEC-04 外部和物理能力

**Design intent**：支付、预订、发信、下单、解锁、驾驶和设备控制不能由 Agent 自主决定。

**Code enforcement**：所有外部效果只能经过服务端 identity、短期 exact Grant、最终 payload digest、预算、幂等键和有效 Approval 后由 Broker adapter 执行。R3 每次确认，R4 二次确认，R5 默认禁止自治并要求独立 safety controller。

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

**Status**：当前有 loopback allowlist，但无认证、Origin/CSRF 和 ownership，只有 transport evidence。

### SEC-07 路径与 TOCTOU

**Design intent**：权限检查、锁定和实际 effect 必须针对同一 canonical root、文件身份和版本完成。

**Code enforcement**：使用 fd-relative / `openat` / no-follow 或隔离工作树；拒绝 `..`、绝对逃逸、symlink/hardlink/rename/mount swap；锁后执行前重验 file identity/version/cwd/env。

**Test evidence**：授权后执行前替换 symlink、hardlink、目录 rename、文件版本和 cwd；返回 `path_changed` 或 `precondition_failed` 且无越界写。

**Status**：当前有 canonical/path allow 检查，完整 TOCTOU 防护未证明，`intent_only`。

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

**Status**：当前 cancellation watch 不等于进程树终止，`intent_only`。

### SEC-09 Unknown 一等状态

**Design intent**：副作用或证据无法确认时，结果必须为 `result_unknown`，不得自动映射为 failed/success 或盲目重试。

**Code enforcement**：使用 durable intent、idempotent invocation、provider receipt/reconcile；补偿、退款和取消均是新授权副作用。

**Test evidence**：provider timeout、daemon crash、event append 失败、late response、部分 JSONL、cancel 后未确认停止；断言 Unknown、pending reconciliation 和禁止自动重试。

**Status**：当前存在状态名但未形成完整恢复契约，`intent_only`。

### SEC-10 审计与事实源

**Design intent**：Receipt 是事件事实源的投影，不是 Agent 或 UI 自报。

**Code enforcement**：Event Store append-only，支持 aggregate/stream version、expected-version/CAS、幂等和重放。事件包含 actor、组织、部门、role、cell、packet、execution、approval、grant、policy、schema、payload digest、结果和异常。

**Test evidence**：乱序、重复、伪造 Receipt、CAS 冲突、事件回滚、重启重放和 Receipt 与状态不一致均被拒绝。

**Status**：当前 JSONL 有限持久化和按 request 序列，尚非 aggregate/CAS durable authority。

### SEC-11 不可信输入

**Design intent**：模型、网页、provider、MCP、skill、tool description、artifact 和普通消息永不授予权限。

**Code enforcement**：所有输入严格 schema、大小、嵌套和 provenance 校验；tool name、arguments、目标和结果重新授权；skill 记录来源 hash 和版本。

**Test evidence**：提示注入、伪造 tool name、超大 args、未知字段、恶意 provider response 和 skill 内容不得提升权限或扩大数据读取。

**Status**：当前 provider/skill 约束不完整，`intent_only`。

### SEC-12 资源耗尽

**Design intent**：本地 API 和 Agent 不能通过无限输入、输出、并发或日志耗尽资源。

**Code enforcement**：对 body、prompt、turn、event、stdout/stderr、tool args、query result、session、artifact、model steps 和并发设置硬上限、分页、背压和超时。

**Test evidence**：超限返回稳定错误码；磁盘满、输出截断、并发峰值和慢 handler 不造成状态泄漏或锁永久占用。

**Status**：部分现有 limits，全面 quota 未证明，`intent_only`。

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

## 4. 验收矩阵

| ID | 场景 | 必须断言 |
|---|---|---|
| ACT-01 | actor 缺失/伪造/不匹配 | 稳定 deny code；Broker 调用数为零 |
| ACT-02 | approval 改 payload、目标、actor、session 或重放 | deny；single-use；原 digest 不变 |
| ACL-01 | role/department/path/memory 越权 | deny；无文件、网络或数据泄漏 |
| ACL-02 | child grant 超过 parent | 原子拒绝；无权限扩大 |
| CELL-01 | depth、children、root、budget、retry、concurrency 到顶 | 无泄漏预留；级联 revoke |
| EXT-01 | payment/travel/order 未确认、漂移、超时 | 无 effect；Unknown 不盲重试 |
| EXT-02 | lock/gas/camera/vehicle/firmware 自治 | 默认 deny；无 safety controller 不 dispatch |
| SEC-01 | sentinel secret 全链路 | 原值不出 Broker |
| WEB-01 | 非 loopback、无 token、Origin 错误、他人 session | mutation 无状态变化 |
| FS-01 | symlink/hardlink/rename/version race | `path_changed`/`precondition_failed`；无越界写 |
| CAN-01 | cancel 与 approval/dispatch/handler race | 状态和事件可证明；无法确认则 Unknown |
| UNK-01 | provider/daemon/event crash | durable pending reconciliation；禁止假成功 |
| AUD-01 | 乱序、重复、伪造 receipt、CAS 冲突 | reject；重放状态与 live state 一致 |
| FI-01 | 所有故障注入点 | fail-closed、无权限扩大、无 secret 泄漏 |

## 5. 发布门

发布前必须满足：

- 每项能力都有 owner、拒绝路径、故障路径和证据引用；
- 所有 P0 宪法条款通过对应 negative tests；
- 测试绑定 immutable source snapshot；
- 不使用 ignored、flaky、skipped 测试替代证据；
- 未知、审计写失败、身份不确定或 provider 状态不确定都会阻断发布；
- 当前未有 live/physical adapter 的能力必须标记 `not_supported`，不能模拟成功；
- 未签名包和临时安装只能标记为本地开发证据。

## 6. 当前边界声明

当前 Kiana 的安全声明只覆盖受信本地项目、固定本机行为和 `local_behavior` 证据。当前 loopback 限制不等于身份认证，trust 不等于项目安全，Receipt 不等于现实结果验证，Grant/Reviewer/Cell 尚未因“对象存在”而自动成为强制安全边界。

当前实现中的具体限制必须显式保留在证据账本中：

- `DaemonHost` 为 actor 使用固定本地主体，并从经过校验的 stored ProjectTrust 记录派生 project trust；wire 中的 `project_trusted` 和 `permission_profile` 不再授予项目或写权限；role/department 仍是声明，尚无 durable authenticated principal；
- Web 使用进程内 session map 和隐式 active 状态，尚无 token、Host/Origin、CSRF 或完整 session ownership；多标签页隔离尚未证明；
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
- Harness capability 遇到 `AwaitingApproval` 当前可能以 capability failure 返回给 Runner，尚未形成持久 `PendingInvocation` 和同一 Runner 的 approval continuation；
- Broker handler 返回的结果尚未由统一 descriptor、schema、timeout、quota 和 provenance 契约完整约束。

支付、外卖、打车、酒店、机票、火车票和智能家居在未完成 adapter、身份、审批、幂等、故障对账和测试前，统一显示为 `not_supported` 或 `target`，不得伪造成功。
