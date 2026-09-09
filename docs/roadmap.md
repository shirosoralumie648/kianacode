# Kiana 执行路线图（P0–P6 执行骨架 + 进度）

> **一屏看进度** → §1 总图。**看某步具体做什么** → §4 起的详细卡。**你想加东西** → §11 追加区。
> 当前事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文只排顺序、记进度、写验收口径，**不定义新规范**。
> 阶段编号以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 的 P0–P6 为唯一 canonical。
> 单元清单来源：`company-os-implementation-outline.md` §3 的切片 A–M 与子切片 J1–M7（共 38 个）。

---

## 0. 怎么用这份文档

**状态图例**

| 标记 | 含义 |
|---|---|
| ✅ | 完成：代码在 `master`、CI 全绿、有证据块 |
| 🔄 | 进行中：Codex 在写，或已推送等 CI |
| ⏳ | 待办：已排进队列，还没开工 |
| ❓ | 待你拍板：不该由 Codex 或我替你决定 |
| 🚫 | 冻结：明确不做 |

**编号规则**

- 形式 `P<阶段>-<切片>-<序号>`，例 `P0-A-01`、`P1-J2-01`。
- 切片字母用 `implementation-outline` 的 A–M；子切片用 `J1`…`M7`。
- 一个切片在同一阶段可以有多个单元（如 `P0-G-01`…`P0-G-04`）。38 个切片/子切片是**覆盖下限**，不是行数上限。
- 序号按该切片在**该阶段内**的依赖顺序排，不预留空号。
- 单元编号**永不重编**；拆分单元加后缀 `-a`/`-b`，不重排后续编号。

**状态口径**

- 总图状态只是**索引**；唯一证据仍是 `CURRENT_STATUS.md` 的证据块。
- 每个 ✅ 必须绑定三件：提交哈希 + CI run id + 证据块名；缺一即降为 ⏳。

**P 编号只有一套**：`company-os-spec-index.md` §7。审计文档里的 P0/P1/P2 一律改称「审计优先级」，不写作 P 编号。

**更新规则（每完成一块必须做）**

1. 改 §1 总图里那一行的状态、提交哈希、CI run id；
2. 在 §3 变更日志加一行（日期 + 做了什么 + 提交）；
3. 该单元的验收测试名如果和当初计划的不一样，改详细卡里的「验收」；
4. `CURRENT_STATUS.md` 里加对应证据块（绑定源码快照 + 命令 + 退出码 + 限制）。

**你加新条目**：在 §11 追加区按模板写一行，我会把它拆成 Codex 任务排进队列，做完再回填状态。

**固定节奏**：`Codex 写实现 → 我审 diff + 编译检查 + 提交推送 → GitHub CI 跑全量门禁 → 回填本文件 + 证据块`。
一次只做一步；CI 红就不是做完。

---

## 1. 总图：P0–P6 × 实施单元

| 编号 | 阶段 | 切片 | 依赖 | 退出条件 | 状态 |
|---|---|---|---|---|---|
| `P0-A-01a` | P0 | A 契约注册表 | — | ID 契约唯一登记 + 每类型转换测试 | ✅ |
| `P0-A-01b` | P0 | A 契约注册表 | `P0-A-01a` | schema 注册表；unknown field / unknown event / migration 规则 | ⏳ |
| `P0-A-02` | P0 | A 契约注册表 | `P0-A-01` | `CapabilityErrorCode` + `failure_code()`，每码有 CLI exit / HTTP status / 可重试映射 | ⏳ |
| `P0-B-01` | P0 | B 正式状态机 | `P0-A-01` | Cell/WorkPacket/CapabilityExecution/Approval 四张转移表；非法转移与重复请求有断言 | ⏳ |
| `P0-F-01` | P0 | F Approval | `P0-B-01` | TTY/Web/一次性 CLI 三处可列举同一 pending 并回复 | ⏳ |
| `P0-F-02` | P0 | F Approval | `P0-F-01` | 每次批/拒都有 durable 记录；重复消费与过期被拒 | ⏳ |
| `P0-F-03` | P0 | F Approval | `P0-G-02b` | 重启后可续跑同一 Runner；缺材料返回 `approval_continuation_unavailable` | ⏳ |
| `P0-G-01` | P0 | G 事实源与恢复 | — | 内存未命中时只读回读重建；账本无记录仍 fail-closed | ✅ |
| `P0-G-02a` | P0 | G 事实源与恢复 | `P0-G-01` | `run.prompt`/`run.tool_call` 落账并过 `redact_event_value` | ✅ |
| `P0-G-02b` | P0 | G 事实源与恢复 | `P0-G-02a` | 只读折叠函数可从 `run.*`/`capability.*` 重建 model-visible history | ✅ |
| `P0-G-03` | P0 | G 事实源与恢复 | `P0-G-02b` | additive `ResumeRequest`，`PROTOCOL_SCHEMA` 不动，复用同一 `drive_run` | ⏳ |
| `P0-G-04` | P0 | G 事实源与恢复 | `P0-G-01` | 新进程仅凭事件重建 Run/Invocation；矛盾终态 fail-closed | ⏳ |
| `P0-J1-01` | P0 | J1 Runtime | `P0-B-01` | `RunCancellationState` + 转移表；`ExecutionStatus` 补 `Queued`/`Cancelling`；每 run 恰好一条终态 | ⏳ |
| `P0-J1-02` | P0 | J1 Runtime | `P0-J1-01` | queued tool calls 排空并合成 replay-safe 结果 | ⏳ |
| `P0-J1-03` | P0 | J1 Runtime | `P0-J1-01` | 取消路径确认进程组停止；无法确认进 `result_unknown` | ⏳ |
| `P0-J1-04` | P0 | J1 Runtime | `P0-J1-01`–`03` | 保留 `cancelling_mid_stream_never_completes_or_emits_a_late_delta` 语义 | ⏳ |
| `P0-J1-05a` | P0 | J1 Runtime | — | 重复工具调用与 run 级 wall-time 预算 fail-closed，阈值进 `RuntimeConfig` 且在产品路径生效 | 🔄 |
| `P0-J1-05b` | P0 | J1 Runtime | `P0-J1-05a` | `RoleSpec.max_steps` 经 `RuntimeConfig` 到达 harness | ⏳ |
| `P0-J7-01` | P0 | J7 Provider/Output | — | 账本粒度、不完整流 fail-closed、默认开启均已落地并有证据块 | ✅ |
| `P0-K1-01` | P0 | K1 Identity | `P0-A-01` | 由受保护入口解析身份；服务端从不可变 assignment 派生 role/department | ⏳ |
| `P0-M1-01` | P0 | M1 Workbench | — | CLI/TTY/Web/Desktop 对同一 run 的 terminal state 一致 | ⏳ |
| `P1-C-01` | P1 | C 组织与 Cell | `P0-A-01` | 六类组织契约定义齐备；子权限只减不增 | ⏳ |
| `P1-C-02` | P1 | C 组织与 Cell | `P1-C-01` | reserve→commit→terminal→retire 全链；retire 撤销 grant、释放锁与预算 | ⏳ |
| `P1-D-01` | P1 | D WorkPacket | `P0-A-01` | `ready_packets(graph, now)` 单实现；三处调用结果一致 | ⏳ |
| `P1-D-02` | P1 | D WorkPacket | `P1-D-01` | `validate_dependency_dag` 输出确定性规范化环；缺依赖不推进状态 | ⏳ |
| `P1-D-03` | P1 | D WorkPacket | `P1-D-01` | 过期 lease 退回 ready 并记事件；worker 死亡后可回收且不重复派发 | ⏳ |
| `P1-E-01` | P1 | E 通信与问责 | `P0-B-01` | 七类消息分离；Handoff 必须定向并 ACK | ⏳ |
| `P1-H-01` | P1 | H Capability/Broker | `P0-A-01` | 工具权威单一真源；不新增模型可见工具，保持 5 个 | ⏳ |
| `P1-H-02` | P1 | H Capability/Broker | — | 映射期拒绝非法参数；`additionalProperties` 不默认禁止 | ✅ |
| `P1-H-03` | P1 | H Capability/Broker | `P1-H-01` | 所有副作用工具共用同一 containment | ⏳ |
| `P1-J2-01` | P1 | J2 Context/Cache | `P0-G-04` | `PromptSection{name, order, text}` + `render_prompt()` + provenance | ⏳ |
| `P1-J2-02` | P1 | J2 Context/Cache | `P1-J2-01` | `TokenBudget` 计入 tool schemas 与 system prompt；越界 fail-closed | ⏳ |
| `P1-J2-03` | P1 | J2 Context/Cache | `P1-J2-01` | `RoleSpec.prompt` 进入 provider 的 system message | ⏳ |
| `P1-J3-01` | P1 | J3 Memory | `P0-A-01` | 模型写入一律 candidate+draft；`origin` 服务端派生；默认检索排除 | ⏳ |
| `P1-J4-01` | P1 | J4 Capability/MCP | `P0-A-01` | MCP server/tool schema、health、trust、version、result validation 可追踪 | ⏳ |
| `P1-J8-01` | P1 | J8 Observability | `P0-G-04` | provider/model、policy verdict、tool args hash、usage、retry/cancel reason 可追溯且不泄密 | ⏳ |
| `P1-K5-01` | P1 | K5 Cost/capacity | `P0-G-04` | `UsageRecord`/`CostLedger`/`Quota`；`RuntimeBudget` 与 `ProjectBudget` 不混用 | ⏳ |
| `P1-L1-01` | P1 | L1 Eval | `P0-G-04` | GoldenTrace 绑定源码快照/输入 hash/版本/Receipt；replay 无真实副作用 | ⏳ |
| `P1-L4-01` | P1 | L4 Code intelligence | `P0-A-01` | 结果带 snapshot、来源与 freshness | ⏳ |
| `P2-J5-01` | P2 | J5 Workflow | `P0-G-04` | 版本固定；重试/取消/审批/补偿可重放 | ⏳ |
| `P2-K3-01` | P2 | K3 Human control | `P0-F-02` | Approval/Review/Acceptance/Incident 进入同一 Inbox | ⏳ |
| `P2-K4-01` | P2 | K4 Artifact | `P0-G-04` | CheckpointService 绑定 transcript offset + workspace revision + invocation；恢复后旧 approval 作废 | ⏳ |
| `P2-K6-01` | P2 | K6 Reliability | `P2-K4-01` | 六类失败各有 Incident/Recovery | ⏳ |
| `P2-K7-01` | P2 | K7 Data governance | `P0-A-01` | 删除/过期/撤销传播到 Memory、Artifact、Index、Compaction、cache policy | ⏳ |
| `P2-L2-01` | P2 | L2 Feedback | `P1-L1-01` | Feedback 只产生候选，不能直接改 Role/Grant/Policy/历史事实 | ⏳ |
| `P2-M2-01` | P2 | M2 UI projection | `P0-M1-01` | `UiSnapshot`/`UiAction`/cursor/epoch；乐观更新不覆盖更新事件 | ⏳ |
| `P2-M3-01` | P2 | M3 Human actions | `P2-M2-01` | Approval/Review/Acceptance/Incident 动作卡三处复用 | ⏳ |
| `P2-M4-01` | P2 | M4 Run/Artifact detail | `P2-M2-01` | Run timeline/Invocation/Diff/Evidence/Receipt 可相互定位 | ⏳ |
| `P2-M5-01` | P2 | M5 Web sync | `P2-M2-01` | snapshot hydration + 事件订阅 + 重连不重放 delta | ⏳ |
| `P2-M5-02` | P2 | M5 Web sync | `P0-G-01` | 只读列出持久会话 | ✅ |
| `P2-M7-01` | P2 | M7 accessible fallback | `P2-M2-01` | 键盘、窄屏、文本状态、aria/高对比 | ⏳ |
| `P3-I-01` | P3 | I Company 生命周期 | `P0-A-01` | 十类业务对象定义与不变量 | ⏳ |
| `P3-I-02` | P3 | I Company 生命周期 | `P3-I-01` | 九个命令/事件冻结 | ⏳ |
| `P3-I-03` | P3 | I Company 生命周期 | `P3-I-02`、`P0-G-04` | 新进程可从事件与 Artifact 引用重建全链 | ⏳ |
| `P3-I-04` | P3 | I Company 生命周期 | `P3-I-02` | criteria snapshot 冻结；Reviewer 不改写 Builder 原始事实 | ⏳ |
| `P3-I-05` | P3 | I Company 生命周期 | `P3-I-03` | Project 关闭需 Acceptance+Delivery+ClosingReceipt 或显式豁免；Outcome 不自动夸大 | ⏳ |
| `P3-I-06` | P3 | I Company 生命周期 | `P3-I-05` | 端到端产出完整 ClosingReceipt | ⏳ |
| `P4-J6-01` | P4 | J6 Swarm | `P1-C-02` | fan-out 有 parent/partition/预算/并发/TTL/WorkFingerprint/MergeDecision | ⏳ |
| `P4-J7-02` | P4 | J7 Provider/Output | `P0-J7-01` | additive `sequence`/`epoch`；`PROTOCOL_SCHEMA` 不动 | ⏳ |
| `P4-J7-03` | P4 | J7 Provider/Output | `P4-J7-02` | Usage/ToolCall/ApprovalRequested/Error 投影；terminal 重放给迟到订阅者 | ⏳ |
| `P4-K2-01` | P4 | K2 Trigger | `P0-B-01` | Trigger 只能创建 Workflow/Run，不能直接执行 Capability | ⏳ |
| `P4-K8-01` | P4 | K8 Connector | `P0-A-01` | 不绕过 ControlPlane/Approval/Idempotency/Receipt/reconciliation | ⏳ |
| `P4-L3-01` | P4 | L3 Version governance | `P1-L1-01` | ModelProfile/PromptBundle/RouteDecision/DriftReport 按版本分桶 | ⏳ |
| `P4-L5-01` | P4 | L5 Extension | `P1-H-01` | skill `allowed-tools` 不进 policy；read-only 扩展写操作在 broker 拒绝 | ⏳ |
| `P4-L6-01` | P4 | L6 Supply chain | `P4-L5-01` | content hash/license/signature/capability diff/rollback 可审计 | ⏳ |
| `P4-M6-01` | P4 | M6 Desktop shell | `P2-M2-01` | workspace onboarding/health/tray/background/safe close | ⏳ |

---

## 2. 当前窗口

| 事项 | 单元 | 位置 | 卡在哪 |
|---|---|---|---|
| 折叠账本重建 history | `P0-G-02b` | `kiana-core/src/history.rs` | `41bb971` + `32692da` 已提交，等 CI + 证据块 |
| `RuntimeConfig` 接线到产品路径 | `P0-J1-05a` | `kiana-daemon/src/lib.rs` | `747ff8b` 已提交，等 CI + 证据块 |

> 最近一次全绿：CI `34380542728`（tip `409cfc7`）—— 覆盖 `P0-J1-05a` 的重复调用与 wall-time 部分。`P1-H-02`（`34380323510`）与 `P2-M5-02`（`34380076942`）各自 CI 亦绿且证据块已落。

---

## 3. 变更日志

| 日期 | 做了什么 | 提交 |
|---|---|---|
| 2026-09-09 | 流式增量按轮次聚合落账，修掉 129 条 `run.delta` 的写放大 | `e9df8b4` |
| 2026-09-09 | 不完整流 fail-closed（`provider_stream_incomplete`）+ 4 个 mock 改回 SSE，修掉 CI 挂 9 小时 | `3b65af2` |
| 2026-09-09 | README/USER/CURRENT_STATUS 与已落地证据对齐 | `99237ad` |
| 2026-09-09 | 命令行默认开启流式 + `--no-stream` 关闭开关 | `e2b15c1` |
| 2026-09-10 | 本文件建立 | `26c69ef` |
| 2026-09-10 | SSE 断线发 `stream_gap`、不完整的轮次不标记完成 | `8b2aecb` |
| 2026-09-10 | 连续重复工具调用 fail-closed（`repeated_tool_call:<name>`） | `d973ff6` |
| 2026-09-10 | 修正被默认翻转影响的 `cli_run` 测试（显式 `--no-stream` + 默认路径覆盖） | `d704add` |
| 2026-09-10 | session 绑定可从事件账本重建（内存未命中时回读 `run.authorized`，仍走 owner 校验） | `e8d9346` |
| 2026-09-10 | 0.4 / 0.5 / 1.1 / 4.1 经 `release-smoke` 全绿，状态置 ✅ | CI `34376675138` |
| 2026-09-10 | 本文件改为 P0–P6 执行骨架：编号重编为 `P<阶段>-<切片>-<序号>`、总图扩到 63 个单元、补 P0 详细卡 | `e5fc47f` |
| 2026-09-10 | 账本记录 user prompt 与 tool_call 身份（`run.prompt` / `run.tool_call`），证据块「Ledger prompt and tool-call identity evidence (2026-09-10)」；`run.tool_result` 仍未做 | `40420bd` + `f08a1cf` |
| 2026-09-10 | 参数在映射到 capability 前校验（`invalid_arguments:<tool>:<field>`），证据块「Tool argument validation at capability mapping evidence (2026-09-10)」 | `9095ea7` + `e144d30` |
| 2026-09-10 | 网页重启后只读列出历史会话，改走 `DaemonHost::persisted_events()` 端口；证据块「Read-only persisted web session history evidence (2026-09-10)」 | `1c504a0` + `e144d30` |
| 2026-09-10 | run 级 wall-time 预算 + `RuntimeConfig` 接进 daemon harness（含 `KIANA_HARNESS_MAX_STEPS` / `KIANA_HARNESS_WALL_TIME_MS`）；证据块「Run-level wall-time budget evidence (2026-09-10)」 | `409cfc7` + `747ff8b` |

---

## 4. P0 — 事实基线、Identity、Runtime ledger、Event/Receipt

### P0-A-01a ID 契约注册表　✅

- **现状**：`kiana-domain/src/contracts.rs` 已登记全部 20 个 ID 类型（18 个 UUID 型 + `SessionId` + `WorkFingerprint`）；CI `34386563396` ✅，证据块「Model-visible history rebuild and pre-dispatch pairing evidence (2026-09-10)」已落。
- **做什么**：保持现状；`ID_CONTRACTS` 表（type_name / owner_crate / wire_name / wire_shape）+ 唯一性断言 + 每类型 serde 往返测试，对照真实 workspace manifest。
- **风险（已核对关闭）**：未改任何 ID 的 serde 表示，线协议兼容保持。
- **验收**：`every_public_type_has_one_owner_and_a_conversion_test`
- **依赖 / 边界**：无依赖；只登记不断言 schema / unknown field / migration 规则（留给 `-01b`）。
- **依据**：`company-os-implementation-outline.md` §Slice A｜`d3f7601` + CI `34386563396` ✅

### P0-A-01b schema 注册表与 unknown field/migration 规则　⏳

### P0-A-02 稳定错误码枚举　⏳

- **现状**：错误以字符串理由跨层传递，没有 enum 与统一映射（`company-os-implementation-outline.md` §Slice B）。
- **做什么**：`CapabilityErrorCode` enum + `CapabilityResult::failure_code()`；每个码定义 CLI exit、HTTP status、是否可重试、是否需新授权或补偿。
- **风险**：错误码一旦进入 wire 就只能加新码，不能改语义。
- **验收**：`path_escape_uses_stable_error_code`
- **依赖 / 边界**：依赖 `P0-A-01`；`result_unknown` 必须进对账队列，不能自动 retry。
- **依据**：`company-os-implementation-outline.md` §Slice B

### P0-B-01 正式状态机转移表　⏳

- **现状**：状态散落在 core/runner/daemon，取消与 dispatch 的线性化点没有冻结。
- **做什么**：为 Cell、WorkPacket、CapabilityExecution、Approval 各出一张状态转移表，冻结终态不可回、expired approval 不得执行、`result_unknown` 不得自动变成功。
- **风险**：新增中间态会改变 `run.cancelled` 时序，必须保住「每 run 恰好一条终态」。
- **验收**：`illegal_state_transition_is_rejected`
- **依赖 / 边界**：依赖 `P0-A-01`；Review 不得覆盖 Builder 原始事实。
- **依据**：`company-os-implementation-outline.md` §Slice B

### P0-F-01 审批一等请求/应答　⏳

- **现状**：审批以请求内联形式出现，三处前端各自渲染，没有统一的 pending 列表。
- **做什么**：`kiana-protocol` 定义审批请求/应答（单号、对象、`available_decisions`、有效期），由 DaemonHost 发出，TTY / Web / 一次性 CLI 三处渲染并回复。
- **风险**：非交互场景若默认自动批准就破坏了暂停语义，必须显式恢复。
- **验收**：`three_surfaces_list_and_answer_the_same_pending_approval`
- **依赖 / 边界**：依赖 `P0-B-01`；传输保持进程内。
- **依据**：`company-os-implementation-outline.md` §Slice F

### P0-F-02 审批决定事件与单次消费　⏳

- **现状**：审批决定不总是产生 durable 记录，重复消费与过期没有统一拒绝。
- **做什么**：新增审批决定事件（actor、scope、subject、expiry、结果），由 ControlPlane 追加进 EventLog 并投影到 transcript 与 Receipt；只能消费一次。
- **风险**：把 decision 的 `request_id` 当作执行关联会破坏审计链，原始执行 `request_id` 必须保留。
- **验收**：`approval_decision_is_durable_and_single_use`
- **依赖 / 边界**：依赖 `P0-F-01`；拒绝与中止是不同终态。
- **依据**：`company-os-implementation-outline.md` §Slice F

### P0-F-03 续跑材料落盘与 RunSnapshot　⏳

- **现状**：PendingInvocation 依赖内存，进程重启后无法续跑。
- **做什么**：审批暂存前先写 invocation 请求事件（含经 `redact_event_value` 的 CapabilityRequest、invocation_id、attempt、policy_snapshot、sandbox、事件游标）；domain 层定义 RunSnapshot，由 ControlPlane 写入、启动时 `resume_run` 重建。
- **风险**：runner 若自己读盘恢复，就绕过了 ControlPlane 独占调用账本的约束。
- **验收**：`fresh_process_resume_reconstructs_pending_approval`
- **依赖 / 边界**：依赖 `P0-G-02b`；缺材料返回 `approval_continuation_unavailable`，不假装成功。
- **依据**：`company-os-implementation-outline.md` §Slice F（A-1）

### P0-G-01 session→run 绑定从账本重建　✅

- **现状**：绑定原先只在 `kiana-core/src/sessions.rs` 的内存 map；重启后 `continue_run`/`cancel_run`/`read_receipt` 一律 `session_not_found`。
- **做什么**：内存未命中时从账本只读回读并重建；账本里确实没有时仍 fail-closed。
- **风险**：不得因「从磁盘读」放宽 owner/role/path/approval 授权。
- **验收**：`fresh_control_plane_rebuilds_session_binding_from_ledger`
- **依赖 / 边界**：无依赖；`PROTOCOL_SCHEMA` 未动。
- **依据**：`company-os-implementation-outline.md` §Slice G｜证据：`e8d9346` + CI `34376675138`

### P0-G-02a 账本记录 prompt 与 tool_call 身份　✅

- **现状**：已落地——`run.prompt` 在 start/continue 两处记录并过 redaction，`run.tool_call` 记录 `call_id` / capability / operation。
- **做什么**：保持现状；这是账本可重建 history 的前半。
- **风险**：每轮新增 append，必须沿用轮次聚合的合并语义（粒度测试仍须通过）。
- **验收**：`start_and_continue_prompts_are_recorded_and_redacted_in_the_ledger`
- **依赖 / 边界**：依赖 `P0-G-01`；`call_id` 在 runner 未提供时为 `null`。
- **依据**：`company-os-implementation-outline.md` §Slice G｜`40420bd` + CI `34378214555` + 证据块「Ledger prompt and tool-call identity evidence (2026-09-10)」（`f08a1cf`）

### P0-G-02b 折叠账本重建 history　✅

- **现状**：折叠函数已落地（`41bb971`），三条预派发失败载荷的 `capability_request_id` 已补（`32692da`/`9d255d1`）；CI `34386563396` ✅，证据块「Model-visible history rebuild and pre-dispatch pairing evidence (2026-09-10)」已落。
- **做什么**：保持现状；只读折叠 `run.prompt`/`run.delta`/`capability.*` 为 `Vec<ConversationMessage>`，**不新增事件 kind**。
- **风险（已核对关闭）**：`capability.result_unknown` 会被折叠跳过、且其载荷本就带 `capability_request_id`，不会产出半配对的 history。
- **验收**：`resume_rebuilds_model_visible_history_from_ledger`
- **依赖 / 边界**：依赖 `P0-G-02a`；不新增模型可见工具，不动 `PROTOCOL_SCHEMA`。
- **依据**：`company-os-implementation-outline.md` §Slice G｜`41bb971` + `9d255d1` + CI `34386563396` ✅

### P0-G-03 `resume_run` 与协议入口　⏳

- **现状**：没有跨进程 resume 入口；`CURRENT_STATUS.md` 把「跨进程完整 resume」记为 `deferred`。
- **做什么**：`kiana-core` 新增 `resume_run`，复用同一条 `drive_run`；`kiana-protocol` 加 additive 的 `ResumeRequest`。
- **风险**：新增入口若另起一条驱动路径，就形成了第二条执行循环。
- **验收**：`resume_run_reuses_the_same_drive_run_path`
- **依赖 / 边界**：依赖 `P0-G-02b`；`PROTOCOL_SCHEMA` 不动。
- **依据**：`company-os-implementation-outline.md` §Slice G

### P0-G-04 事件重建投影　⏳

- **现状**：run/invocation 状态主要存在内存 `Mutex<HashMap>`，不是事件的投影。
- **做什么**：新增 RunProjection / InvocationProjection，用 `read_stream("run", run_id)` 与 `read_all` 折叠 `run.*`/`capability.*`/`approval.*`；首次按 run_id/session 访问时惰性重建。
- **风险**：折叠遇矛盾终态必须保持 `run_terminal_conflict`/`result_unknown` fail-closed，不能猜。
- **验收**：`new_process_rebuilds_run_state_from_events_alone`
- **依赖 / 边界**：依赖 `P0-G-01`；内存 map 降级为写穿缓存。
- **依据**：`company-os-implementation-outline.md` §Slice G（A-2）

### P0-J1-01 统一 cancellation token 与状态词表　⏳

- **现状**：三套取消机制并存——core 用 `watch<bool>`、runner 用 `Mutex<Option<String>>`、legacy 入口第三套。
- **做什么**：`RunCancellationState { Accepted, Active, Queued, Cancelling, Cancelled, Terminal }` + 转移表；`ExecutionStatus` 补 `Queued` / `Cancelling`。
- **风险**：新增中间态会改变 `run.cancelled` 时序，必须保住「每 run 恰好一条终态」。
- **验收**：`cancel_transitions_are_total_and_irreversible`
- **依赖 / 边界**：依赖 `P0-B-01`；不新增模型可见工具。
- **依据**：`company-os-implementation-outline.md` §Slice J1

### P0-J1-02 排空已启动工作 + 合成未启动结果　⏳

- **现状**：`KianaHarness::cancel` 在 in-flight 时只发 `Failed{cancelled:}` 就返回，`pending_tools` 与 inbox 直接丢弃。
- **做什么**：排空已启动工作，为未启动的工具调用合成 replay-safe 结果，保证重放不产生副作用。
- **风险**：合成结果若被当成真实执行结果，会污染后续模型可见 history。
- **验收**：`cancel_drains_queued_tool_calls_with_replay_safe_results`
- **依赖 / 边界**：依赖 `P0-J1-01`；参考形状见 `reference/crush/internal/agent/agent.go:432-465`。
- **依据**：`company-os-implementation-outline.md` §Slice J1

### P0-J1-03 进程组确认与 `stop_confirmed`　⏳

- **现状**：取消路径不校验进程是否真的停了；MCP stdio 只在 Drop 里 `start_kill`。
- **做什么**：取消路径确认 shell 进程组已停止并写 `stop_confirmed`；无法确认时进 `result_unknown`。
- **风险**：误报「已停止」会让用户以为副作用已回滚。
- **验收**：`cancel_confirms_shell_process_group_stopped`
- **依赖 / 边界**：依赖 `P0-J1-01`；不改 runner 之外的执行路径。
- **依据**：`company-os-implementation-outline.md` §Slice J1

### P0-J1-04 取消竞态负向证据　⏳

- **现状**：取消与 dispatch 的竞态没有负向测试覆盖。
- **做什么**：补竞态用例，证明取消竞争不产生错误完成、不留孤立 tool calls。
- **风险**：新用例若放宽既有断言，就等于用测试掩盖回归。
- **验收**：`cancel_race_never_produces_wrong_completion`
- **依赖 / 边界**：依赖 `P0-J1-01`–`03`；必须保留 `daemon_host::cancelling_mid_stream_never_completes_or_emits_a_late_delta` 的语义。
- **依据**：`company-os-implementation-outline.md` §Slice J1

### P0-J1-05a 重复调用检测与 wall-time 预算接线　🔄

- **现状**：重复调用检测已落地（`d973ff6`）；`wall_time_budget` 与 `with_wall_time_budget` 已加（`409cfc7`）；`RuntimeConfig` 已接进 daemon 的 5 处 harness 构造，支持 `KIANA_HARNESS_MAX_STEPS` / `KIANA_HARNESS_WALL_TIME_MS`（`747ff8b`）。
- **做什么**：保持现状；确认无环境变量时默认行为不变（32 步、无 wall-time）。
- **风险**：环境变量非法值必须 fail-closed；只加字段不接线等于配置存在但不生效。
- **验收**：`run_wall_time_budget_fails_closed`
- **依赖 / 边界**：无依赖；不同参数不算重复调用，不得误伤。
- **依据**：`company-os-implementation-outline.md` §Slice J1｜`409cfc7` + `747ff8b`，等 CI + 证据块

### P0-J1-05b 按角色的 max_steps　⏳

- **现状**：`RoleSpec.max_steps`（`kiana-domain/src/roles.rs:140`）仍未被消费；`with_max_steps` 零调用点，产品路径恒 32 步。
- **做什么**：把角色 `max_steps` 经 `RuntimeConfig` 传到 harness，并加断言。
- **风险**：阈值写死在 harness 会让角色配置形同虚设。
- **验收**：`role_max_steps_reaches_the_harness`
- **依赖 / 边界**：依赖 `P0-J1-05a`；不改 `RoleSpec` 现有字段语义。
- **依据**：`company-os-implementation-outline.md` §Slice J1

### P0-J7-01 流式基线收尾　✅

- **现状**：已落地——增量按轮次聚合落账（`e9df8b4`）、不完整流 fail-closed（`3b65af2`）、命令行默认流式 + `--no-stream`（`e2b15c1`/`d704add`）、断线发 `stream_gap`（`8b2aecb`）。
- **做什么**：保持现状；后续协议扩展见 `P4-J7-02`/`-03`。
- **风险**：无已知风险；回归由 `run_streams_each_delta_by_default_before_terminal_receipt` 覆盖。
- **验收**：`run_streams_each_delta_by_default_before_terminal_receipt`
- **依赖 / 边界**：无依赖；Web 仍不声称 token streaming 之外的能力。
- **依据**：`company-os-implementation-outline.md` §Slice J7｜证据：CI `34376675138`

### P0-K1-01 服务端身份与 authority epoch　⏳

- **现状**：`DaemonHost` 使用固定本地主体并从 stored ProjectTrust 派生 project trust；role/department 仍由请求选择，无 durable authenticated principal。
- **做什么**：由受保护入口解析身份，服务端从不可变 assignment 派生 role/department/authority epoch。
- **风险**：wire 上的 actor/trust/profile 若被当作授权来源，就是越权入口。
- **验收**：`wire_actor_cannot_grant_role_or_department`
- **依赖 / 边界**：依赖 `P0-A-01`；不改现有 ProjectTrust 派生逻辑的语义。
- **依据**：`company-os-implementation-outline.md` §Slice K1

### P0-M1-01 Workbench 基线　⏳

- **现状**：Workbench 已有 conversation/input/status、trust、sandbox、cancel 和 receipt 入口。
- **做什么**：补齐四表面一致的 terminal state 呈现与 status line/transcript/input 基线。
- **风险**：各表面各自维护状态会产生第二真相。
- **验收**：`workbench_surfaces_agree_on_terminal_state`
- **依赖 / 边界**：无依赖；颜色与布局变化不得隐藏安全状态。
- **依据**：`company-os-implementation-outline.md` §Slice M1

---

## 5. P1 — ContextPlan、Memory、Cache、Capability Catalog、Cost、Eval

### P1-C-01 组织与 Cell 契约　⏳

- **现状**：Cell 相关类型零散，`kiana-domain` 没有 `AgentTemplate`/`CellSpec`/`SpawnPlan` 的统一契约。
- **做什么**：定义 `AgentTemplate`、`CellSpec`、`SpawnPlan`、`BudgetLease`、`CapabilityGrant`、`SupervisionLease`；模板版本固定，子权限只减不增，默认不可再委派。
- **风险**：模板版本若不固定，历史 Cell 无法复现。
- **验收**：`child_grant_cannot_exceed_parent_grant`
- **依赖 / 边界**：依赖 `P0-A-01`；优先在 `kiana-domain` 定义契约。
- **依据**：`company-os-implementation-outline.md` §Slice C

### P1-C-02 Cell 生命周期与 retire　⏳

- **现状**：registry/budget 仍是进程内状态，尚无 durable Cell projector 和跨进程恢复。
- **做什么**：打通 reserve→commit→terminal→retire 全链；retire 撤销 grant、释放锁和预算。
- **风险**：spawn 预留预算、锁和 grant 必须具有原子语义，否则会出现预算泄漏。
- **验收**：`retire_revokes_grants_and_releases_budget`
- **依赖 / 边界**：依赖 `P1-C-01`；`kiana-core` 执行授权和生命周期，`kiana-daemon` 提供目录与调度。
- **依据**：`company-os-implementation-outline.md` §Slice C

### P1-D-01 WorkPacket 单一 ready 谓词　⏳

- **现状**：spawn 校验、`kiana project next`、Web/Desktop 看板各自判断就绪。
- **做什么**：`kiana-domain`/`kiana-tasks` 只暴露一个 `ready_packets(graph, now)`，三处必须调用同一实现。
- **风险**：三处各写一份会让「可派发」的定义漂移。
- **验收**：`single_ready_predicate_agrees_across_three_callers`
- **依赖 / 边界**：依赖 `P0-A-01`；就绪 = 状态可派发 + 依赖全成功 + 无未过期 lease 冲突。
- **依据**：`company-os-implementation-outline.md` §Slice D

### P1-D-02 依赖缺失 / 成环 fail-closed　⏳

- **现状**：依赖边未强制为显式字段，成环检测未在 approve 与模板注册时调用。
- **做什么**：`WorkPacket.dependencies` 显式字段 + `validate_dependency_dag`，输出确定性规范化环（两次运行字节一致），失败拒绝落盘。
- **风险**：从 packet 文本解析依赖会引入不确定性和注入面。
- **验收**：`dependency_cycle_is_rejected_deterministically`
- **依赖 / 边界**：依赖 `P1-D-01`；父 packet blocked 时子 packet 派生 blocked。
- **依据**：`company-os-implementation-outline.md` §Slice D

### P1-D-03 claim / lease 心跳回收　⏳

- **现状**：没有 claim(owner, lease_expires_at, heartbeat_at) 与过期扫描。
- **做什么**：spawn / continue / 每个 turn 续租；后台确定性扫描过期 lease，把 packet 退回 ready 并记事件。
- **风险**：ready 只是查询，执行许可仍必须由 policy/gates/approval 产生。
- **验收**：`expired_lease_is_reclaimed_without_double_dispatch`
- **依赖 / 边界**：依赖 `P1-D-01`；worker 死亡后可回收且不重复派发。
- **依据**：`company-os-implementation-outline.md` §Slice D

### P1-E-01 通信与问责分层　⏳

- **现状**：Chat、Command、Handoff 等消息没有类型区分，自由聊天可能被当成授权。
- **做什么**：区分 Chat、Command、Handoff、Decision、StatusReport、Evidence、Incident；Handoff 必须定向并 ACK。
- **风险**：自由聊天一旦产生授权，问责链就断了。
- **验收**：`free_chat_never_grants_authority`
- **依赖 / 边界**：依赖 `P0-B-01`；`kiana-ports` 定义接口，`kiana-core` 产生正式事件。
- **依据**：`company-os-implementation-outline.md` §Slice E

### P1-H-01 `ToolSpec` registry　⏳

- **现状**：工具权威分散在 5 处，新增工具要改多处。
- **做什么**：`kiana-domain` 新增 `tool_authority` 模块（`ToolSpec{name, aliases, capability, operation, risk_policy, side_effecting, schema}` + `TOOL_SPECS`）。
- **风险**：registry 若成为第二套 authority 而不被 policy 消费，就是摆设。
- **验收**：`tool_authority_covers_every_model_visible_tool`
- **依赖 / 边界**：依赖 `P0-A-01`；**不新增模型可见工具**，保持 5 个。
- **依据**：`company-os-implementation-outline.md` §Slice H

### P1-H-02 参数 schema 校验　✅

- **现状**：`9095ea7` 已加 `validate_tool_arguments`，在映射到 capability 前按现有 schema 表校验。
- **做什么**：保持现状；校验覆盖 `type` / `required` / `minimum` / `enum` 子集。
- **风险**：`additionalProperties` 不拒绝（老 cassette 不被误拒）；`schema_name_for_tool` 与 `tools.rs:234` 的别名表重复，`P1-H-01` 必须收敛。
- **验收**：`malformed_arguments_are_rejected_before_capability_mapping`
- **依赖 / 边界**：无硬依赖（`P1-H-01` 落地后只需替换数据源）；不改变已有工具的接受集。
- **依据**：`company-os-implementation-outline.md` §Slice H｜`9095ea7` + CI `34380323510` + 证据块「Tool argument validation at capability mapping evidence (2026-09-10)」（`e144d30`）

### P1-H-03 路径 containment 共享实现　⏳

- **现状**：策略层只对 `apply_patch` 提取路径，shell 全靠 bwrap/workdir。
- **做什么**：抽出一份共享的 path containment，所有副作用工具共用。
- **风险**：只对部分工具生效会留下绕过面（symlink / hardlink / rename）。
- **验收**：`path_containment_is_shared_by_every_side_effecting_tool`
- **依赖 / 边界**：依赖 `P1-H-01`；不得放宽现有 fail-closed 行为。
- **依据**：`company-os-implementation-outline.md` §Slice H

### P1-J2-01 类型化区段 + provenance　⏳

- **现状**：context 按字符串拼接，无区段类型、无 provenance。
- **做什么**：`PromptSection{name, order, text}` + `render_prompt()`，每个区段带来源。
- **风险**：改了拼接顺序会影响 cassette 命中，必须固定 `order`。
- **验收**：`context_sections_render_with_provenance`
- **依赖 / 边界**：依赖 `P0-G-04`；不新增模型可见工具。
- **依据**：`company-os-implementation-outline.md` §Slice J2

### P1-J2-02 预算覆盖 tool schemas 与 system prompt　⏳

- **现状**：token 预算只算消息，不算 tool schemas 与 system prompt。
- **做什么**：`TokenBudget` 计入 tool schemas 与 system prompt；越界 fail-closed。
- **风险**：漏算会让实际请求超出 provider 上限并报错在错误的层。
- **验收**：`tool_schemas_count_toward_the_budget`
- **依赖 / 边界**：依赖 `P1-J2-01`；压缩策略不在本单元。
- **依据**：`company-os-implementation-outline.md` §Slice J2

### P1-J2-03 角色 prompt 接线　⏳

- **现状**：`RoleSpec.prompt` 已定义但没有进入 provider 的 system message。
- **做什么**：把 `RoleSpec.prompt` 接到 provider 的 system message。
- **风险**：角色 prompt 若覆盖系统安全指令，会削弱边界。
- **验收**：`role_prompt_reaches_the_provider_system_message`
- **依赖 / 边界**：依赖 `P1-J2-01`；不改变现有安全指令的优先级。
- **依据**：`company-os-implementation-outline.md` §Slice J2

### P1-J3-01 Memory 写入候选制　⏳

- **现状**：模型写入的 memory 可能直接进入可检索集合。
- **做什么**：`memory.write` 由模型写入一律落 candidate + draft；`origin` 由服务端派生（model / hook / git / user）；默认检索排除 candidate，只有操作者或目标层 owner 显式批准才转 active。
- **风险**：模型不能自批；instance-scratch 层保持默认可见。
- **验收**：`model_written_memory_stays_unsearchable_until_approved`
- **依赖 / 边界**：依赖 `P0-A-01`；持久层 candidate 默认不可检索。
- **依据**：`company-os-implementation-outline.md` §Slice J3

### P1-J4-01 Capability Descriptor 与 MCP 生命周期　⏳

- **现状**：capability 合同较窄，MCP server/tool schema、health、trust、version 不可追踪。
- **做什么**：把 `(CapabilityKind, operation)` 扩展为带域、版本、风险、scope、approval、幂等和补偿描述的 descriptor；MCP 走 stdio 生命周期管理。
- **风险**：未知 operation、参数越界、资源越界、过期 grant 必须 fail-closed。
- **验收**：`mcp_tool_schema_and_health_are_traceable`
- **依赖 / 边界**：依赖 `P0-A-01`；**HTTP MCP 冻结**，只支持 stdio。
- **依据**：`company-os-implementation-outline.md` §Slice H、§Slice J4

### P1-J8-01 Observability 与 trace/receipt　⏳

- **现状**：provider/model、policy verdict、usage、retry/cancel reason 没有统一记录。
- **做什么**：记录 provider/model、policy verdict、tool arguments hash、workspace、timing、usage/cost、retry/cancellation reason 和 persistence revision，同时不泄露 secrets。
- **风险**：记录 tool arguments 若不做 redaction 就是泄密面。
- **验收**：`observability_record_is_redacted_and_traceable`
- **依赖 / 边界**：依赖 `P0-G-04`；复用现有 `redact_event_value` 边界。
- **依据**：`company-os-implementation-outline.md` §Slice J8

### P1-K5-01 成本与容量账本　⏳

- **现状**：usage 未形成账本，预算类型混用。
- **做什么**：`UsageRecord`/`CostLedger`/`Quota`；`RuntimeBudget`、`ProjectBudget` 和未来的 `FinancialBudget` 不得混用。
- **风险**：混用预算会让成本归属失真。
- **验收**：`runtime_and_project_budgets_are_not_interchangeable`
- **依赖 / 边界**：依赖 `P0-G-04`；不引入真实计费。
- **依据**：`company-os-implementation-outline.md` §Slice K5

### P1-L1-01 EvalSuite 与 GoldenTrace　⏳

- **现状**：已有 cassette、focused regression，但没有统一 EvalSuite 与 GoldenTrace。
- **做什么**：GoldenTrace 绑定源码快照、输入 hash、版本和 Receipt；eval replay 不产生真实外部副作用。
- **风险**：GoldenTrace 若不绑定快照，回归会被静默吞掉。
- **验收**：`golden_trace_replay_has_no_external_side_effects`
- **依赖 / 边界**：依赖 `P0-G-04`；安全失败、禁止效果、replay divergence 和 evidence 缺失会阻断 Promote。
- **依据**：`company-os-implementation-outline.md` §Slice L1

### P1-L4-01 Code intelligence 快照　⏳

- **现状**：query/repo-map 已有素材，但结果不带 snapshot 与 freshness。
- **做什么**：`RepositorySnapshot`/`SymbolIndex`/`DependencyGraph`/`RepoMap` 的结果带 snapshot、来源和 freshness。
- **风险**：无 freshness 的索引结果会被当成当前事实。
- **验收**：`code_intelligence_results_carry_freshness`
- **依赖 / 边界**：依赖 `P0-A-01`；不改变现有索引的可见范围。
- **依据**：`company-os-implementation-outline.md` §Slice L4

---

## 6. P2 — Durable Workflow、Human Inbox、Recovery、Artifact、Client cursor

### P2-J5-01 Workflow definition 与重放　⏳

- **现状**：`kiana-workflow` 的 WorkflowDefinition/NodeExecution/Signal/Compensation 为 `target`。
- **做什么**：Workflow definition 版本固定，重试、取消、审批、补偿和恢复均可重放。
- **风险**：版本不固定时，重放会执行到与当初不同的节点。
- **验收**：`workflow_definition_replays_after_restart`
- **依赖 / 边界**：依赖 `P0-G-04`；本切片不新增第二套 ControlPlane 或 Agent loop。
- **依据**：`company-os-implementation-outline.md` §Slice J5

### P2-K3-01 Human Inbox　⏳

- **现状**：审批、复核、验收、异常分散在不同界面，没有统一待办入口。
- **做什么**：Approval、Review、Acceptance、Incident 和 Reconciliation 进入同一 Human Inbox。
- **风险**：Inbox 若只做展示不做 resolve，会形成第二套决策入口。
- **验收**：`human_inbox_collects_all_decision_types`
- **依赖 / 边界**：依赖 `P0-F-02`；决策仍由 ControlPlane resolve。
- **依据**：`company-os-implementation-outline.md` §Slice K3

### P2-K4-01 Artifact 版本与编辑级 undo　⏳

- **现状**：已有 `apply_patch` 前置快照（capture_preconditions / restore_snapshot），但没有绑定 transcript 的 CheckpointService。
- **做什么**：首发只做状态层 + 编辑级 undo——CheckpointService 绑定 transcript offset + workspace revision + invocation，写工具前与用户输入前快照。
- **风险**：preview 绝不能写盘；undo 是受控操作，不作为模型可见工具。
- **验收**：`restore_invalidates_stale_approval`
- **依赖 / 边界**：依赖 `P0-G-04`；文件层 shadow git 列为第二阶段。
- **依据**：`company-os-implementation-outline.md` §Slice K4（A-10）

### P2-K6-01 可靠性与对账　⏳

- **现状**：crash/timeout/cancel/disk full/MCP failure/Provider Unknown 没有统一 Incident/Recovery 路径。
- **做什么**：六类失败各有 Incident 与 RecoveryPlan，`result_unknown` 进 reconciliation queue。
- **风险**：`result_unknown` 若被自动 retry，可能产生重复副作用。
- **验收**：`every_failure_class_has_an_incident_and_recovery`
- **依赖 / 边界**：依赖 `P2-K4-01`；不自动重试 `result_unknown`。
- **依据**：`company-os-implementation-outline.md` §Slice K6

### P2-K7-01 数据治理与删除传播　⏳

- **现状**：`DataClass`/`Purpose`/`ProcessingGrant`/`Retention` 为 `target`，删除不会传播。
- **做什么**：删除、过期和撤销能传播到 Memory、Artifact、Index、Compaction 和 cache policy。
- **风险**：传播不全等于数据没删干净，却对外声称已删。
- **验收**：`deletion_propagates_to_memory_and_index`
- **依赖 / 边界**：依赖 `P0-A-01`；不改变现有 ACL 语义。
- **依据**：`company-os-implementation-outline.md` §Slice K7

### P2-L2-01 反馈与候选改进　⏳

- **现状**：`Feedback`/`Pattern`/`Candidate`/`Promotion` 为 `target`。
- **做什么**：Feedback 只能产生候选改进，不能直接修改 Role、Grant、Policy 或历史事实。
- **风险**：反馈若直接改 policy，就绕过了审批与审计。
- **验收**：`feedback_cannot_mutate_policy_or_history`
- **依赖 / 边界**：依赖 `P1-L1-01`；晋级必须经质量门禁。
- **依据**：`company-os-implementation-outline.md` §Slice L2

### P2-M2-01 UI 投影合同　⏳

- **现状**：各前端各自维护会话状态，没有统一的 `UiSnapshot`/`UiAction`。
- **做什么**：`kiana-protocol` 定义 `UiSnapshot`/`UiAction`（带 cursor、epoch、pending action），`kiana-daemon` 生成投影。
- **风险**：乐观更新若覆盖更新的服务端事件，界面会显示错误状态。
- **验收**：`stale_ui_action_is_rejected_by_epoch`
- **依赖 / 边界**：依赖 `P0-M1-01`；UI 不维护第二套运行循环。
- **依据**：`company-os-implementation-outline.md` §Slice M2

### P2-M3-01 人工动作卡　⏳

- **现状**：审批、复核、验收、异常的动作界面各写一份。
- **做什么**：Approval、Review、Acceptance、Incident 动作卡三处（CLI/TTY/Web）复用同一实现。
- **风险**：动作卡若各自决定 `available_decisions`，会出现越权选项。
- **验收**：`action_card_is_shared_by_all_surfaces`
- **依赖 / 边界**：依赖 `P2-M2-01`；动作必须带 target ID、owner、expected epoch 和 idempotency key。
- **依据**：`company-os-implementation-outline.md` §Slice M3

### P2-M4-01 Run/Artifact 详情　⏳

- **现状**：Run timeline、Invocation、Diff、Evidence、Receipt 没有统一详情视图。
- **做什么**：四类详情可相互定位（Receipt ↔ Artifact ↔ Evidence ↔ Review）。
- **风险**：详情视图若自行拼装数据，会与事件事实不一致。
- **验收**：`receipt_artifact_and_evidence_cross_locate`
- **依赖 / 边界**：依赖 `P2-M2-01`；不引入新的存储。
- **依据**：`company-os-implementation-outline.md` §Slice M4

### P2-M5-01 Web 快照水合与重连　⏳

- **现状**：Web 订阅事件但没有合并 snapshot load 期间观察到的事件，重连语义未冻结。
- **做什么**：snapshot hydration + 事件订阅 + 重连不重放已送 delta。
- **风险**：重放已送 delta 会让前端重复渲染。
- **验收**：`web_sse_reconnect_emits_stream_gap_without_replaying_delta_items`
- **依赖 / 边界**：依赖 `P2-M2-01`；Web 保持 loopback-only。
- **依据**：`company-os-implementation-outline.md` §Slice M5

### P2-M5-02 重启后列出历史会话　✅

- **现状**：`1c504a0` 已实现只读列出；数据源是 `DaemonHost::persisted_events()` 端口，web 层不再自己解析 JSONL。
- **做什么**：保持现状；只读列出持久会话（不含写入与续跑）。
- **风险**：列表若暴露其他主体的会话就是越权；存储不支持全量读取时返回 `web_session_history_unsupported`。
- **验收**：`web_lists_persisted_sessions_after_restart`
- **依赖 / 边界**：依赖 `P0-G-01`；前置 enabler 是 `0a9a56b`（只读端口）；只读，不提供续跑入口。
- **依据**：`company-os-implementation-outline.md` §Slice M5｜`1c504a0` + CI `34380076942` + 证据块「Read-only persisted web session history evidence (2026-09-10)」（`e144d30`）

### P2-M7-01 无障碍回退　⏳

- **现状**：状态主要靠颜色和布局表达。
- **做什么**：键盘、窄屏、文本状态、aria/高对比全部可用。
- **风险**：颜色与动画变化不得隐藏安全状态。
- **验收**：`status_is_reachable_without_color`
- **依赖 / 边界**：依赖 `P2-M2-01`；不改动信息层级。
- **依据**：`company-os-implementation-outline.md` §Slice M7

---

## 7. P3 — Objective → Project → Packet → Builder → Review → Acceptance → Close

### P3-I-01 Company 业务对象契约　⏳

- **现状**：Company 业务域对象基本为 `target`，现有 WorkPacket/Review 不能替代完整聚合。
- **做什么**：定义 `Objective`、`Initiative`、`Project`、`Milestone`、`Acceptance`、`Delivery`、`Outcome`、`ChangeRequest`、`Risk`、`Incident` 及不变量。
- **风险**：把业务对象实现成 prompt 里的名词，而不是 domain 类型。
- **验收**：`company_objects_expose_invariants`
- **依赖 / 边界**：依赖 `P0-A-01`；字段与状态机以 `company-os-domain-contracts.md` 为准。
- **依据**：`company-os-implementation-outline.md` §Slice I

### P3-I-02 命令与事件冻结　⏳

- **现状**：九个业务命令/事件尚未冻结。
- **做什么**：冻结 `propose_objective` → `ObjectiveProposed` 等九个命令/事件对，并版本化。
- **风险**：命令一旦发布只能加版本，不能改语义。
- **验收**：`company_commands_are_frozen_and_versioned`
- **依赖 / 边界**：依赖 `P3-I-01`；`kiana-protocol` 承载 versioned DTO。
- **依据**：`company-os-implementation-outline.md` §Slice I

### P3-I-03 全链重建　⏳

- **现状**：Objective → Project → Packet → Run → Acceptance → Receipt 无法从事实重建。
- **做什么**：新进程可以从事件和 Artifact 引用重建全链。
- **风险**：未批准、过期审批、越权路径、重复命令和重复交付必须 fail-closed。
- **验收**：`new_process_rebuilds_the_company_chain`
- **依赖 / 边界**：依赖 `P3-I-02`、`P0-G-04`；不引入第二事实源。
- **依据**：`company-os-implementation-outline.md` §Slice I

### P3-I-04 Acceptance 快照与独立 Review　⏳

- **现状**：验收标准未冻结为 snapshot，Reviewer 与 Builder 可能共享 author session。
- **做什么**：`Acceptance` 进入 `Requested` 时从已冻结的上层标准派生并冻结为 `criteria_snapshot`；决策阶段只读校验。
- **风险**：Reviewer 改写 Builder 原始事实会破坏问责链。
- **验收**：`reviewer_cannot_rewrite_builder_facts`
- **依赖 / 边界**：依赖 `P3-I-02`；Reviewer 与 Builder 不共享 author session 或可修改的验收基线。
- **依据**：`company-os-implementation-outline.md` §Slice I

### P3-I-05 Delivery / ClosingReceipt / Outcome　⏳

- **现状**：没有 Delivery、ClosingReceipt 与 Outcome 测量。
- **做什么**：Project 关闭必须有 Acceptance、Delivery、ClosingReceipt，或有明确的失败关闭/人工豁免；Outcome 需要测量窗口与观测证据。
- **风险**：Objective `Achieved` 不能由 Receipt 或模型文本直接宣称。
- **验收**：`outcome_cannot_be_claimed_without_measurement`
- **依赖 / 边界**：依赖 `P3-I-03`；`result_unknown` 必须关联 Incident/Reconciliation。
- **依据**：`company-os-implementation-outline.md` §Slice I

### P3-I-06 fake-model coding 黄金闭环　⏳

- **现状**：没有端到端的 Company 闭环验收。
- **做什么**：用 fake model 跑通 Objective → Project → Planner → Builder → Reviewer → Acceptance → Closer，产出完整 ClosingReceipt。
- **风险**：把「Agent 运行成功」当成「业务结果成功」。
- **验收**：`fake_model_coding_project_produces_closing_receipt`
- **依赖 / 边界**：依赖 `P3-I-05`；拒绝、返工、暂停、取消、失败关闭和 Unknown 各有可重放状态。
- **依据**：`company-os-implementation-outline.md` §Slice I

---

## 8. P4 — 有界 Swarm、Scheduler、Provider streaming、Skill/Plugin 生态

### P4-J6-01 有界 Swarm　⏳

- **现状**：`SwarmPlan`/`Partition`/`Child Cell`/`MergeDecision` 为 `target`。
- **做什么**：fan-out 必须有 parent、partition、预算、并发、TTL、WorkFingerprint 和 MergeDecision。
- **风险**：无限 fan-out 会让预算与授权失效。
- **验收**：`swarm_fanout_is_bounded_and_merges_deterministically`
- **依赖 / 边界**：依赖 `P1-C-02`；相同 WorkFingerprint 不重复创建。
- **依据**：`company-os-implementation-outline.md` §Slice J6

### P4-J7-02 wire 加 `sequence`/`epoch`　⏳

- **现状**：事件 wire 没有 `sequence`/`epoch`，客户端无法判断乱序与陈旧。
- **做什么**：additive 地加 `sequence`/`epoch`；序号单调。
- **风险**：破坏性改动会打翻现有客户端，必须 additive。
- **验收**：`run_stream_sequence_is_monotonic`
- **依赖 / 边界**：依赖 `P0-J7-01`；`PROTOCOL_SCHEMA` 不动。
- **依据**：`company-os-implementation-outline.md` §Slice J7

### P4-J7-03 事件种类补齐与 terminal 必达　⏳

- **现状**：`Usage`/`ToolCall`/`ApprovalRequested`/`Error` 未投影；terminal 对迟到订阅者不保证必达。
- **做什么**：补齐四类事件投影；terminal 重放给迟到订阅者（`Last-Event-ID`）。
- **风险**：terminal 丢失会让客户端永远等不到结束。
- **验收**：`terminal_is_replayed_to_late_subscriber`
- **依赖 / 边界**：依赖 `P4-J7-02`；不新增运行循环。
- **依据**：`company-os-implementation-outline.md` §Slice J7

### P4-K2-01 触发器与调度　⏳

- **现状**：`TriggerDefinition`/`TriggerFiring`/`Schedule`/`Signal` 为 `target`。
- **做什么**：Trigger 只能创建 Workflow/Run，不能直接执行 Capability。
- **风险**：Trigger 直连 capability 就绕过了审批。
- **验收**：`trigger_cannot_execute_a_capability_directly`
- **依赖 / 边界**：依赖 `P0-B-01`；调度不持有执行许可。
- **依据**：`company-os-implementation-outline.md` §Slice K2

### P4-K8-01 Connector　⏳

- **现状**：`ConnectorDefinition`/`AccountBinding`/`ProviderReceipt` 为 `not_supported`/`target`。
- **做什么**：Connector 不绕过 ControlPlane、Approval、Idempotency、Receipt 和 reconciliation。
- **风险**：Connector 是外部副作用入口，绕过控制面即等于无审计。
- **验收**：`connector_cannot_bypass_the_control_plane`
- **依赖 / 边界**：依赖 `P0-A-01`；R3+ 需最终 payload 单次确认。
- **依据**：`company-os-implementation-outline.md` §Slice K8

### P4-L3-01 版本治理与 drift　⏳

- **现状**：`ModelProfile`/`PromptBundle`/`RouteDecision`/`DriftReport` 为 `target`。
- **做什么**：模型、Prompt、Route 与 drift 报告按版本分桶。
- **风险**：不分桶就无法判断指标变化来自哪次变更。
- **验收**：`drift_report_is_bucketed_by_version`
- **依赖 / 边界**：依赖 `P1-L1-01`；不自动切换模型版本。
- **依据**：`company-os-implementation-outline.md` §Slice L3

### P4-L5-01 扩展与技能包　⏳

- **现状**：`ExtensionManifest`/`SkillPack` 为 `partial`/`target`；skill 的 `allowed-tools` 语义未冻结。
- **做什么**：skill 的 `allowed-tools` 只影响提示/展示，不进入 policy；扩展清单增加 effect、required_capabilities、network_policy、content_hash/signature、requires。
- **风险**：声明不等于授权；read-only 扩展的写操作必须在 broker 层直接拒绝。
- **验收**：`skill_allowed_tools_cannot_grant_shell`
- **依赖 / 边界**：依赖 `P1-H-01`；扩展不能创建第二套 Runtime。
- **依据**：`company-os-implementation-outline.md` §Slice L5（F-2）

### P4-L6-01 供应链　⏳

- **现状**：content hash、license、signature、capability diff、rollback 为 `target`。
- **做什么**：安装、升级、迁移、撤销和回滚可审计，安装时校验摘要与兼容性。
- **风险**：安装成功不等于安全验证完成。
- **验收**：`extension_signature_is_verified_before_install`
- **依赖 / 边界**：依赖 `P4-L5-01`；不做签名分发的生产化。
- **依据**：`company-os-implementation-outline.md` §Slice L6

### P4-M6-01 Desktop 壳　⏳

- **现状**：`contrib/desktop` 为 `partial`，安装、升级、worker 崩溃恢复和供应链证据未达生产级。
- **做什么**：workspace onboarding、health、tray、background、safe close。
- **风险**：safe close 若留下孤儿进程，会与下一次启动争抢锁。
- **验收**：`desktop_safe_close_leaves_no_orphan_process`
- **依赖 / 边界**：依赖 `P2-M2-01`；不宣称生产级安装与升级。
- **依据**：`company-os-implementation-outline.md` §Slice M6

---

## 9. P5 / P6 边界登记（不写卡）

本阶段**只登记边界与「为什么不现在写」**，不写六行卡。

| 阶段 | 范围 | 为什么现在不写 | 依据 |
|---|---|---|---|
| P5 | Office / Work | 发送、提交和外部资料修改属 R3，需最终 payload 单次确认；先做本地文档整理、报告草稿、只读检索 | `company-os-implementation-outline.md` §4 |
| P5 | Search / Recommendation | 先做只读搜索、来源、时间和新鲜度；搜索结果永不产生购买或预订授权 | 同上 |
| P5 | Commerce / Food | 购物车/下单/支付/退款属 R4，必须绑定商户、商品、数量、总成本、地址、时间、条款、账户、digest 和 idempotency key | 同上 |
| P5 | Mobility / Travel | 出票、打车、订房、改签、取消需单次确认、供应商状态核验和 Unknown 对账 | 同上 |
| P5 | Home / IoT | 门锁、摄像、麦克风、燃气、高功率、固件、车辆和安防属 R5，默认禁止自治，需独立安全控制器、watchdog、急停和人工接管 | 同上 |
| P6 | Team / Remote / Enterprise | 需要 durable principal、跨机身份与供应链证据；仓库当前证明上限为 `local_behavior` | `company-os-spec-index.md` §7 |

**进入条件**：先落地 `P0-K1-01`（身份）、`P1-J4-01`（MCP 生命周期）、`P4-K8-01`（Connector）与 `P4-L6-01`（供应链）。

---

## 10. 冻结与不做项

| 项 | 状态 | 说明 | 依据 |
|---|---|---|---|
| `kiana-entrypoints/src/runner.rs:633/1095` 的第二条执行循环 | ❓ 待拍板 | 直连 `execute_tool_calls_with_permission_handler`，绕过 broker 与 ControlPlane；`kiana-tools/src/agent.rs:2937` 生成的团队 agent 脚本会带 `--resident-teammate`，因此这条路径**可达**。两条路：(a) 迁到 `DaemonHost` 脊柱；(b) 标记 legacy 并冻结且不进产品帮助文案 | `CLAUDE.md`「Keep model/tool execution brokered」 |
| 新增模型可见工具 | 🚫 冻结 | 保持 5 个 | `AGENTS.md` §7 |
| Builder 参与规划/监控 symposium | 🚫 冻结 | 参会边界冻结，不得改成联合 symposium | `CLAUDE.md` |
| HTTP MCP | 🚫 冻结 | 只支持 stdio | `CLAUDE.md` |
| 第二套执行循环或控制面 | 🚫 冻结 | 产品脊柱只有 `DaemonHost` | `AGENTS.md` §8 |

**决定前不动代码**：❓ 项需要先产出书面决定 + 文档更新。

---

## 11. 追加区（你写这里，我接着派给 Codex）

在下面按模板加行即可；我会把它拆成任务、排进队列，做完回填 §1 与 §3。

**模板**

```text
- [ ] 标题：一句话说清要什么
      验收：怎么算做完（能写成测试名最好）
      边界：不许碰什么
```

**待办**

```text
（在这里追加）
```

---

## 12. 每步的完成定义（不许放宽）

一步算完成，必须同时满足：

1. 代码在 `master` 上，CI 的 `release-smoke` **全绿**；
2. 验收测试名出现在 §1 总图里，且**先红后绿**（有观察记录）；
3. `CURRENT_STATUS.md` 有绑定源码快照的证据块；
4. 没有删除、放宽或跳过任何既有断言；
5. 冻结项没有被打开。

**不算完成**：代码存在但没进产品路径、单测绿但 CI 红、一次本机成功就写成 `durable`/`live`、把 `target` 改写成 `implemented`。

---

## 13. 文档维护项（不属于产品单元，不进总图）

- [ ] 重审 `codex` / `deepseek-harness` / `goose` 三份审计（旧 `8.1`）
- [ ] 新增 `grok-build` 审计 + 补审计 9 项（旧 `8.2`）
