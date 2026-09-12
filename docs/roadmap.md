# Kiana 执行路线图（P0–P6 执行骨架 + 进度）

> **一屏看进度** → §1 总图。**查全部 Step** → §1.1 全量索引。**看基础卡** → §4–§8。**看专项卡** → 专项拆分文档。**你想加东西** → §11 追加区。
> 当前事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文只排顺序、记进度、写验收口径，**不定义新规范**。
> 阶段编号以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 的 P0–P6 为唯一 canonical。
> 单元清单来源：`company-os-implementation-outline.md` §3 的切片 A–M 与子切片 J1–M7（共 38 个）；产品特有单元（角色目录、会议、六层记忆、提示词来源）另见 `COMPANY.md` §3/§4/§5/§7。
> 本次核对：2026-09-12，源码快照 `db77c24`；共享工作树另有持续变化的 WIP，不能套用历史 CI。当前窗口见 §2，核对证据见 `CURRENT_STATUS.md`「Roadmap source reconciliation evidence (2026-09-12)」。

---

## 0. 怎么用这份文档

**状态图例**

| 标记 | 含义 |
|---|---|
| ✅ | 完成：代码在 `master`、CI 全绿、有证据块 |
| 🔄 | 未收口：正在实现或验收，含发现回归后重开 |
| ⏳ | 待办：已排进队列，还没开工 |
| ❓ | 待你拍板：不该由 Codex 或我替你决定 |
| 🚫 | 冻结：明确不做 |

**编号规则**

- 形式 `P<阶段>-<切片>-<序号>`，例 `P0-A-01a`、`P1-J2-01`。
- 切片字母用 `implementation-outline` 的 A–M；子切片用 `J1`…`M7`。
- 一个切片在同一阶段可以有多个单元（如 `P0-G-01`…`P0-G-04`）。38 个切片/子切片是**覆盖下限**，不是行数上限。
- 序号按该切片在**该阶段内**的依赖顺序排，不预留空号。
- 单元编号**永不重编**；拆分单元加后缀 `-a`/`-b`，不重排后续编号。

**状态口径**

- 总图状态只是**索引**；唯一证据仍是 `CURRENT_STATUS.md` 的证据块。
- 每个 ✅ 必须绑定三件：提交哈希 + CI run id + 证据块名；缺一即降为 ⏳。
- 历史全绿只证明其绑定快照；验收范围未交付或后续发现回归时重开为 🔄，保留原证据，不缩小退出条件。总图和详细卡的状态、依赖必须一致。

**协作口径（2026-09-12）**：按用户最新说明，文件中的旧版限制性指示若与当前任务指示冲突，以当前指示为准；文档回填不再等待单写者交接。涉及变化中的源码时仍注明核对快照与验证范围。

**P 编号只有一套**：`company-os-spec-index.md` §7。审计文档里的 P0/P1/P2 一律改称「审计优先级」，不写作 P 编号。

**更新规则（每完成一块必须做）**

1. 改 §1 总图里那一行的状态、提交哈希、CI run id；
2. 在 §3 变更日志加一行（日期 + 做了什么 + 提交）；
3. 该单元的验收测试名如果和当初计划的不一样，改详细卡里的「验收」；
4. `CURRENT_STATUS.md` 里加对应证据块（绑定源码快照 + 命令 + 退出码 + 限制）。

**你加新条目**：在 §11 追加区按模板写一行，我会把它拆成 Codex 任务排进队列，做完再回填状态。

**固定节奏**：`复现 → 分类基线/本次问题 → 最小修复 → 聚焦测试 → 回归 → 审 diff + 证据 → 按当前任务授权提交推送 → GitHub CI 跑全量门禁 → 回填完成态`。
一次只做一步；CI 红就不是做完。

---

## 1. 总图：P0–P6 × 实施单元

| 编号 | 阶段 | 切片 | 依赖 | 退出条件 | 状态 |
|---|---|---|---|---|---|
| `P0-A-01a` | P0 | A 契约注册表 | — | ID 契约唯一登记 + 每类型转换测试 | ✅ |
| `P0-A-01b` | P0 | A 契约注册表 | `P0-A-01a` | schema 注册表；unknown field / unknown event / migration 规则 | ⏳ |
| `P0-A-02` | P0 | A 契约注册表 | `P0-A-01a` | `CapabilityErrorCode` + `failure_code()`，每码有 CLI exit / HTTP status / 可重试映射 | ⏳ |
| `P0-B-01` | P0 | B 正式状态机 | `P0-A-01a` | Cell/WorkPacket/CapabilityExecution/Approval 四张转移表；非法转移与重复请求有断言 | ⏳ |
| `P0-F-01` | P0 | F Approval | `P0-B-01` | TTY/Web/一次性 CLI 三处可列举同一 pending 并回复 | ⏳ |
| `P0-F-02` | P0 | F Approval | `P0-F-01` | 每次批/拒都有 durable 记录；重复消费与过期被拒 | ⏳ |
| `P0-F-03` | P0 | F Approval | `P0-G-02b`、`P0-G-03`、`P0-F-02` | 重启默认暂停；显式恢复重新过授权，续跑同一 Runner；缺材料 fail-closed | ⏳ |
| `P0-G-01` | P0 | G 事实源与恢复 | — | 内存未命中时只读回读重建；账本无记录仍 fail-closed | ✅ |
| `P0-G-02a` | P0 | G 事实源与恢复 | `P0-G-01` | `run.prompt`/`run.tool_call` 落账并过 `redact_event_value` | ✅ |
| `P0-G-02b` | P0 | G 事实源与恢复 | `P0-G-02a` | 只读折叠函数可从 `run.*`/`capability.*` 重建 model-visible history | ✅ |
| `P0-G-03` | P0 | G 事实源与恢复 | `P0-G-02b` | additive `ResumeRequest`，`PROTOCOL_SCHEMA` 不动，复用同一 `drive_run` | ⏳ |
| `P0-G-04` | P0 | G 事实源与恢复 | `P0-G-01` | 新进程仅凭事件重建 Run/Invocation；矛盾终态 fail-closed | 🔄 |
| `P0-J1-01` | P0 | J1 Runtime | `P0-B-01` | `RunCancellationState` + 转移表；`ExecutionStatus` 补 `Queued`/`Cancelling`；每 run 恰好一条终态 | ⏳ |
| `P0-J1-02` | P0 | J1 Runtime | `P0-J1-01` | queued tool calls 排空并合成 replay-safe 结果 | ⏳ |
| `P0-J1-03` | P0 | J1 Runtime | `P0-J1-01` | 取消路径确认进程组停止；无法确认进 `result_unknown` | ⏳ |
| `P0-J1-04` | P0 | J1 Runtime | `P0-J1-01`–`03` | 保留 `cancelling_mid_stream_never_completes_or_emits_a_late_delta` 语义 | ⏳ |
| `P0-J1-05a` | P0 | J1 Runtime | — | 重复工具调用与 run 级 wall-time 预算 fail-closed，阈值进 `RuntimeConfig` 且在产品路径生效 | 🔄 |
| `P0-J1-05b` | P0 | J1 Runtime | `P0-J1-05a` | 角色步数经 ControlPlane 命令在 harness 生效；环境覆盖、run 间隔离与原有 wall-time 均有行为断言 | 🔄 |
| `P0-J7-01` | P0 | J7 Provider/Output | — | 账本粒度、不完整流 fail-closed、默认开启均已落地并有证据块 | ✅ |
| `P0-K1-01` | P0 | K1 Identity | `P0-A-01a` | 由受保护入口解析身份；服务端从不可变 assignment 派生 role/department | ⏳ |
| `P0-M1-01` | P0 | M1 Workbench | — | CLI/TTY/Web/Desktop 对同一 run 的 terminal state 一致 | ⏳ |
| `P1-C-01` | P1 | C 组织与 Cell | `P0-A-01a` | 六类组织契约定义齐备；子权限只减不增 | ⏳ |
| `P1-C-02` | P1 | C 组织与 Cell | `P1-C-01` | reserve→commit→terminal→retire 全链；retire 撤销 grant、释放锁与预算 | ⏳ |
| `P1-C-03` | P1 | C 组织与 Cell | `P1-C-01` | 五部门 × 角色 RoleSpec 数据集；`model_profile` 到达 provider 路由 | ⏳ |
| `P1-D-01` | P1 | D WorkPacket | `P0-A-01a` | `ready_packets(graph, now)` 单实现；三处调用结果一致 | ⏳ |
| `P1-D-02` | P1 | D WorkPacket | `P1-D-01` | `validate_dependency_dag` 输出确定性规范化环；缺依赖不推进状态 | ⏳ |
| `P1-D-03` | P1 | D WorkPacket | `P1-D-01` | 过期 lease 退回 ready 并记事件；worker 死亡后可回收且不重复派发 | ⏳ |
| `P1-E-01` | P1 | E 通信与问责 | `P0-B-01` | 七类消息分离；Handoff 必须定向并 ACK | ⏳ |
| `P1-E-02` | P1 | E 通信与问责 | `P1-E-01` | 现有 symposium 会议路径有验收测试；决定事件 durable 可重放 | ⏳ |
| `P1-H-01` | P1 | H Capability/Broker | `P0-A-01a` | 工具权威单一真源；不新增模型可见工具，保持 5 个 | ⏳ |
| `P1-H-02` | P1 | H Capability/Broker | — | 映射期拒绝非法参数；`additionalProperties` 不默认禁止 | ✅ |
| `P1-H-03` | P1 | H Capability/Broker | `P1-H-01` | 所有副作用工具共用同一 containment | ⏳ |
| `P1-J2-01` | P1 | J2 Context/Cache | `P0-G-04` | `PromptSection{name, order, text}` + `render_prompt()` + provenance | ⏳ |
| `P1-J2-02` | P1 | J2 Context/Cache | `P1-J2-01` | `TokenBudget` 计入 tool schemas 与 system prompt；越界 fail-closed | ⏳ |
| `P1-J2-03` | P1 | J2 Context/Cache | `P1-J2-01` | `RoleSpec.prompt` 进入 provider 的 system message | ⏳ |
| `P1-J2-04` | P1 | J2 Context/Cache | `P1-J2-03` | 角色 prompt 从角色包加载；`prompt_hash` 进收据可复现 | ⏳ |
| `P1-J3-01` | P1 | J3 Memory | `P0-A-01a` | 模型写入一律 candidate+draft；`origin` 服务端派生；默认检索排除 | ⏳ |
| `P1-J3-02` | P1 | J3 Memory | `P1-J3-01` | 检索带相关性打分且命中进收据可追溯；grants ACL 两端一致 | ⏳ |
| `P1-J3-03` | P1 | J3 Memory | `P1-J3-01`、`P0-F-01` | 抽取建议包带 evidence 与相似旧记录；三档准入落地 | ⏳ |
| `P1-J3-04` | P1 | J3 Memory | `P1-J3-02` | hybrid 检索（BM25+本地向量+RRF+MMR）确定性可复现；模型 hash 校验 fail-closed | ⏳ |
| `P1-J4-01` | P1 | J4 Capability/MCP | `P0-A-01a` | MCP server/tool schema、health、trust、version、result validation 可追踪 | ⏳ |
| `P1-J8-01` | P1 | J8 Observability | `P0-G-04` | provider/model、policy verdict、tool args hash、usage、retry/cancel reason 可追溯且不泄密 | ⏳ |
| `P1-K5-01` | P1 | K5 Cost/capacity | `P0-G-04` | `UsageRecord`/`CostLedger`/`Quota`；`RuntimeBudget` 与 `ProjectBudget` 不混用 | ⏳ |
| `P1-L1-01` | P1 | L1 Eval | `P0-G-04` | GoldenTrace 绑定源码快照/输入 hash/版本/Receipt；replay 无真实副作用 | ⏳ |
| `P1-L4-01` | P1 | L4 Code intelligence | `P0-A-01a` | 结果带 snapshot、来源与 freshness | ⏳ |
| `P2-J5-01` | P2 | J5 Workflow | `P0-G-04` | 版本固定；重试/取消/审批/补偿可重放 | ⏳ |
| `P2-K3-01` | P2 | K3 Human control | `P0-F-02` | Approval/Review/Acceptance/Incident 进入同一 Inbox | ⏳ |
| `P2-K4-01` | P2 | K4 Artifact | `P0-G-04` | CheckpointService 绑定 transcript offset + workspace revision + invocation；恢复后旧 approval 作废 | ⏳ |
| `P2-K6-01` | P2 | K6 Reliability | `P2-K4-01` | 六类失败各有 Incident/Recovery | ⏳ |
| `P2-K7-01` | P2 | K7 Data governance | `P0-A-01a`、`P1-J3-04` | 删除/过期/撤销传播到 Memory、Artifact、Index、Compaction、cache policy | ⏳ |
| `P2-L2-01` | P2 | L2 Feedback | `P1-L1-01` | Feedback 只产生候选，不能直接改 Role/Grant/Policy/历史事实 | ⏳ |
| `P2-M2-01` | P2 | M2 UI projection | `P0-M1-01` | `UiSnapshot`/`UiAction`/cursor/epoch；乐观更新不覆盖更新事件 | ⏳ |
| `P2-M3-01` | P2 | M3 Human actions | `P2-M2-01` | Approval/Review/Acceptance/Incident 动作卡三处复用 | ⏳ |
| `P2-M4-01` | P2 | M4 Run/Artifact detail | `P2-M2-01` | Run timeline/Invocation/Diff/Evidence/Receipt 可相互定位 | ⏳ |
| `P2-M5-01` | P2 | M5 Web sync | `P2-M2-01` | snapshot hydration + 事件订阅 + 重连不重放 delta | ⏳ |
| `P2-M5-02` | P2 | M5 Web sync | `P0-G-01` | 只读列出持久会话 | ✅ |
| `P2-M7-01` | P2 | M7 accessible fallback | `P2-M2-01` | 键盘、窄屏、文本状态、aria/高对比 | ⏳ |
| `P3-I-01` | P3 | I Company 生命周期 | `P0-A-01a` | 十类业务对象定义与不变量 | ⏳ |
| `P3-I-02` | P3 | I Company 生命周期 | `P3-I-01` | 九个命令/事件冻结 | ⏳ |
| `P3-I-03` | P3 | I Company 生命周期 | `P3-I-02`、`P0-G-04` | 新进程可从事件与 Artifact 引用重建全链 | ⏳ |
| `P3-I-04` | P3 | I Company 生命周期 | `P3-I-02` | criteria snapshot 冻结；Reviewer 不改写 Builder 原始事实 | ⏳ |
| `P3-I-05` | P3 | I Company 生命周期 | `P3-I-03` | Project 关闭需 Acceptance+Delivery+ClosingReceipt 或显式豁免；Outcome 不自动夸大 | ⏳ |
| `P3-I-06` | P3 | I Company 生命周期 | `P3-I-05` | 端到端产出完整 ClosingReceipt | ⏳ |
| `P4-E-03` | P4 | E 通信与问责 | `P1-E-02`、`P1-J3-02`、`P1-J3-03` | 五部门可各自开会；决议写入部门记忆层 | ⏳ |
| `P4-J3-05` | P4 | J3 Memory | `P1-J3-03` | run 蒸馏产出 lesson candidate 入部门层 | ⏳ |
| `P4-J6-01` | P4 | J6 Swarm | `P1-C-02` | fan-out 有 parent/partition/预算/并发/TTL/WorkFingerprint/MergeDecision | ⏳ |
| `P4-J7-02` | P4 | J7 Provider/Output | `P0-J7-01` | additive `sequence`/`epoch`；`PROTOCOL_SCHEMA` 不动 | ⏳ |
| `P4-J7-03` | P4 | J7 Provider/Output | `P4-J7-02` | Usage/ToolCall/ApprovalRequested/Error 投影；terminal 重放给迟到订阅者 | ⏳ |
| `P4-K2-01` | P4 | K2 Trigger | `P0-B-01` | Trigger 只能创建 Workflow/Run，不能直接执行 Capability | ⏳ |
| `P4-K8-01` | P4 | K8 Connector | `P0-A-01a` | 不绕过 ControlPlane/Approval/Idempotency/Receipt/reconciliation | ⏳ |
| `P4-L3-01` | P4 | L3 Version governance | `P1-L1-01` | ModelProfile/PromptBundle/RouteDecision/DriftReport 按版本分桶 | ⏳ |
| `P4-L5-01` | P4 | L5 Extension | `P1-H-01` | skill `allowed-tools` 不进 policy；read-only 扩展写操作在 broker 拒绝 | ⏳ |
| `P4-L6-01` | P4 | L6 Supply chain | `P4-L5-01` | content hash/license/signature/capability diff/rollback 可审计 | ⏳ |
| `P4-M6-01` | P4 | M6 Desktop shell | `P2-M2-01` | workspace onboarding/health/tray/background/safe close | ⏳ |

---

<a id="all-step-index"></a>

### 1.1 全量 Step 总图：依赖波次（403 张）

> 这里列出基础路线图 74 张验收卡与 9 个专项 329 张实施卡，共 403 张登记项。基础卡是阶段验收口径，专项卡是实现拆分；两者有意重叠，不能相加当作 403 份独立交付。

**排序原则**：先固定事实基线和已验证基础，再收口当前 wall-time/role-limit 与共享契约；随后建立权威账本、运行时、执行和恢复；再推进跨模块编排与 CompanyOS；最后做入口一致性、后置平台扩展和发布门。`W0–W11` 只是建议执行波次，不是新的 P 阶段或完成状态。

| 波次 | 名称 | 登记项 | 说明 |
|---|---|---:|---|
| W0 | 已有证据与当前收口 | 20 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W1 | 共享契约与身份 | 55 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W2 | 事实账本、授权与资源 | 43 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W3 | 实际执行、取消与恢复 | 54 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W4 | 上下文、记忆与扩展 | 68 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W5 | Provider 协议与调用链 | 17 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W6 | 投影与可用入口 | 39 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W7 | CompanyOS 业务闭环 | 42 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W8 | 并行、治理与复用 | 13 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W9 | 离线联合验收 | 35 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W10 | 后置平台与能力扩展 | 7 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W11 | 真实环境与发布证据 | 10 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |

**依赖字段说明**：表中的“前置”只收录卡片明确写出的硬依赖；“关联原单元”是接口对齐关系，不会被误当作整项完成门。Event/Receipt、Context/Memory 的线性项遵循各专项已经发布的保守顺序。

| 顺序 | 波次 | 类型 | Step | 工作项 | 前置 | 状态 | 详细卡 |
|---:|---|---|---|---|---|---|---|
| **W0** | **已有证据与当前收口** |  |  |  |  |  |  |
| 001 | W0 | 基础 | [`P0-A-01a`](#step-p0-a-01a) | P0 基础 · ID 契约注册表 | — | ✅ | [基础卡](#step-p0-a-01a) |
| 002 | W0 | 基础 | [`P0-G-01`](#step-p0-g-01) | P0 基础 · session→run 绑定从账本重建 | — | ✅ | [基础卡](#step-p0-g-01) |
| 003 | W0 | 基础 | [`P0-G-02a`](#step-p0-g-02a) | P0 基础 · 账本记录 prompt 与 tool_call 身份 | `P0-G-01` | ✅ | [基础卡](#step-p0-g-02a) |
| 004 | W0 | 基础 | [`P0-G-02b`](#step-p0-g-02b) | P0 基础 · 折叠账本重建 history | `P0-G-02a` | ✅ | [基础卡](#step-p0-g-02b) |
| 005 | W0 | 基础 | [`P0-J7-01`](#step-p0-j7-01) | P0 基础 · 流式基线收尾 | — | ✅ | [基础卡](#step-p0-j7-01) |
| 006 | W0 | 基础 | [`P1-H-02`](#step-p1-h-02) | P1 基础 · 参数 schema 校验 | — | ✅ | [基础卡](#step-p1-h-02) |
| 007 | W0 | 基础 | [`P2-M5-02`](#step-p2-m5-02) | P2 基础 · 重启后列出历史会话 | `P0-G-01` | ✅ | [基础卡](#step-p2-m5-02) |
| 008 | W0 | 基础 | [`P0-J1-05a`](#step-p0-j1-05a) | P0 基础 · 重复调用检测与 wall-time 预算接线 | — | 🔄 | [基础卡](#step-p0-j1-05a) |
| 009 | W0 | 基础 | [`P0-J1-05b`](#step-p0-j1-05b) | P0 基础 · 按角色的 max_steps | `P0-J1-05a` | 🔄 | [基础卡](#step-p0-j1-05b) |
| 010 | W0 | 基础 | [`P1-J3-01`](#step-p1-j3-01) | P1 基础 · Memory 写入候选制 | `P0-A-01a` | ⏳ | [基础卡](#step-p1-j3-01) |
| 011 | W0 | 专项 | [`CP-00`](roadmap/control-plane.md#step-cp-00) | ControlPlane · 固定基线，列出所有有后果的入口 | — | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-00) |
| 012 | W0 | 专项 | [`ER-00`](roadmap/event-receipt-recovery.md#step-er-00) | Event / Receipt / Recovery · 固定基线与事实边界 | — | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-00) |
| 013 | W0 | 专项 | [`CAP-00`](roadmap/capability.md#step-cap-00) | Capability · 固定可复核基线，消除计划与 WIP 重叠 | — | ⏳ | [专项卡](roadmap/capability.md#step-cap-00) |
| 014 | W0 | 专项 | [`H01`](roadmap/harness.md#step-h01) | Harness · 固定接线基线与可执行验收骨架 | — | ⏳ | [专项卡](roadmap/harness.md#step-h01) |
| 015 | W0 | 专项 | [`P4-J7-04`](roadmap/provider.md#step-p4-j7-04) | Provider · Provider 基线、快照和现有测试 | `P0-J7-01` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-04) |
| 016 | W0 | 专项 | [`CM-00`](roadmap/context-memory.md#step-cm-00) | Context / Memory · 固定源码快照、差异与证据边界 | — | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-00) |
| 017 | W0 | 专项 | [`EXT-00`](roadmap/skills-plugins-hooks.md#step-ext-00) | Skills / Plugins / Hooks · 基线与决策回执 | — | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-00) |
| 018 | W0 | 专项 | [`UI-00`](roadmap/ui-entrypoints.md#step-ui-00) | UI / Entrypoints · 建立入口基线与验收矩阵 | — | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-00) |
| 019 | W0 | 专项 | [`CO-01`](roadmap/companyos.md#step-co-01) | CompanyOS · 锁定当前实现与计划的交接基线 | — | ⏳ | [专项卡](roadmap/companyos.md#step-co-01) |
| 020 | W0 | 基础 | [`P0-G-04`](#step-p0-g-04) | P0 基础 · 事件重建投影 | `P0-G-01` | 🔄 | [基础卡](#step-p0-g-04) |
| **W1** | **共享契约与身份** |  |  |  |  |  |  |
| 021 | W1 | 专项 | [`CP-01`](roadmap/control-plane.md#step-cp-01) | ControlPlane · 服务端主体与项目身份 | `CP-00` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-01) |
| 022 | W1 | 专项 | [`CP-02`](roadmap/control-plane.md#step-cp-02) | ControlPlane · 使用既有 ID，明确状态机和 Continue/Resume | `CP-00` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-02) |
| 023 | W1 | 专项 | [`CP-03`](roadmap/control-plane.md#step-cp-03) | ControlPlane · 规范化 action，风险与执行元数据服务端所有 | `CP-01`、`CP-02` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-03) |
| 024 | W1 | 专项 | [`CP-04`](roadmap/control-plane.md#step-cp-04) | ControlPlane · 权限交集与单调决策在 core 强制 | `CP-03` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-04) |
| 025 | W1 | 专项 | [`CP-05`](roadmap/control-plane.md#step-cp-05) | ControlPlane · 合并三条授权执行路径 | `CP-04` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-05) |
| 026 | W1 | 专项 | [`CP-06`](roadmap/control-plane.md#step-cp-06) | ControlPlane · 定义原子状态转移端口 | `CP-02`、`CP-04` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-06) |
| 027 | W1 | 专项 | [`ER-01`](roadmap/event-receipt-recovery.md#step-er-01) | Event / Receipt / Recovery · 事件 schema、kind registry 与迁移规则 | `ER-00` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-01) |
| 028 | W1 | 专项 | [`ER-02`](roadmap/event-receipt-recovery.md#step-er-02) | Event / Receipt / Recovery · 统一身份、关联和顺序语义 | `ER-01` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-02) |
| 029 | W1 | 专项 | [`ER-03`](roadmap/event-receipt-recovery.md#step-er-03) | Event / Receipt / Recovery · 事件边界脱敏和 Artifact 引用 | `ER-02` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-03) |
| 030 | W1 | 专项 | [`ER-04`](roadmap/event-receipt-recovery.md#step-er-04) | Event / Receipt / Recovery · CommandReceipt 与 transition read-set | `ER-03` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-04) |
| 031 | W1 | 专项 | [`CAP-01`](roadmap/capability.md#step-cap-01) | Capability · descriptor、schema、policy metadata 与 handler binding 单一来源 | `CAP-00` | ⏳ | [专项卡](roadmap/capability.md#step-cap-01) |
| 032 | W1 | 专项 | [`CAP-02`](roadmap/capability.md#step-cap-02) | Capability · 统一参数边界与输入摘要 | `CAP-01` | ⏳ | [专项卡](roadmap/capability.md#step-cap-02) |
| 033 | W1 | 专项 | [`CAP-03`](roadmap/capability.md#step-cap-03) | Capability · 从 authority chain 派生不可变 ExecutionScope | `CAP-02` | ⏳ | [专项卡](roadmap/capability.md#step-cap-03) |
| 034 | W1 | 专项 | [`CAP-04`](roadmap/capability.md#step-cap-04) | Capability · 状态与 outcome 不再依赖字符串猜测 | `CAP-02` | ⏳ | [专项卡](roadmap/capability.md#step-cap-04) |
| 035 | W1 | 专项 | [`H02`](roadmap/harness.md#step-h02) | Harness · Session / Run / Turn / Step 的身份与生命周期 | `H01` | ⏳ | [专项卡](roadmap/harness.md#step-h02) |
| 036 | W1 | 专项 | [`H03`](roadmap/harness.md#step-h03) | Harness · 将 KianaHarness 收敛为单一状态驱动器 | `H02` | ⏳ | [专项卡](roadmap/harness.md#step-h03) |
| 037 | W1 | 专项 | [`H04`](roadmap/harness.md#step-h04) | Harness · 结构化模型消息与无损 Provider 转换 | `H03` | ⏳ | [专项卡](roadmap/harness.md#step-h04) |
| 038 | W1 | 专项 | [`H05`](roadmap/harness.md#step-h05) | Harness · 统一停止原因、错误与重试分类 | `H04` | ⏳ | [专项卡](roadmap/harness.md#step-h05) |
| 039 | W1 | 专项 | [`P4-J7-05`](roadmap/provider.md#step-p4-j7-05) | Provider · 非流式工具响应必须严格解析 | `P4-J7-04` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-05) |
| 040 | W1 | 专项 | [`CM-01`](roadmap/context-memory.md#step-cm-01) | Context / Memory · 建立共享来源与 scope 值对象 | `CM-00` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-01) |
| 041 | W1 | 专项 | [`CM-02`](roadmap/context-memory.md#step-cm-02) | Context / Memory · 统一 MemoryRecord 生命周期与兼容导入 | `CM-01` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-02) |
| 042 | W1 | 专项 | [`CM-03`](roadmap/context-memory.md#step-cm-03) | Context / Memory · 服务端派生 read/write scope 与 purpose | `CM-02` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-03) |
| 043 | W1 | 专项 | [`CM-04`](roadmap/context-memory.md#step-cm-04) | Context / Memory · 统一 Memory mutation 与幂等键 | `CM-03` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-04) |
| 044 | W1 | 专项 | [`EXT-01`](roadmap/skills-plugins-hooks.md#step-ext-01) | Skills / Plugins / Hooks · 稳定扩展领域合同 | `EXT-00` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-01) |
| 045 | W1 | 专项 | [`EXT-02`](roadmap/skills-plugins-hooks.md#step-ext-02) | Skills / Plugins / Hooks · SourceResolver、ProjectTrust 与路径根 | `EXT-01` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-02) |
| 046 | W1 | 专项 | [`EXT-03`](roadmap/skills-plugins-hooks.md#step-ext-03) | Skills / Plugins / Hooks · 严格解析器与兼容层 | `EXT-02` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-03) |
| 047 | W1 | 专项 | [`EXT-04`](roadmap/skills-plugins-hooks.md#step-ext-04) | Skills / Plugins / Hooks · Catalog、优先级、重复和可解释性 | `EXT-03` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-04) |
| 048 | W1 | 专项 | [`EXT-05`](roadmap/skills-plugins-hooks.md#step-ext-05) | Skills / Plugins / Hooks · 快照与失效 | `EXT-04` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-05) |
| 049 | W1 | 专项 | [`UI-01`](roadmap/ui-entrypoints.md#step-ui-01) | UI / Entrypoints · 定义 versioned UI protocol DTO | `UI-00` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-01) |
| 050 | W1 | 专项 | [`UI-02`](roadmap/ui-entrypoints.md#step-ui-02) | UI / Entrypoints · 统一错误、能力和 surface handshake | `UI-01` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-02) |
| 051 | W1 | 专项 | [`UI-03`](roadmap/ui-entrypoints.md#step-ui-03) | UI / Entrypoints · 本地实例身份、发现和 transport | `UI-01`、`UI-02` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-03) |
| 052 | W1 | 专项 | [`CO-02`](roadmap/companyos.md#step-co-02) | CompanyOS · 稳定组织、业务项目与工作区绑定 | `CO-01` | ⏳ | [专项卡](roadmap/companyos.md#step-co-02) |
| 053 | W1 | 专项 | [`CO-03`](roadmap/companyos.md#step-co-03) | CompanyOS · 角色任命、有效期与撤销接入服务端身份 | `CO-02` | ⏳ | [专项卡](roadmap/companyos.md#step-co-03) |
| 054 | W1 | 专项 | [`CO-04`](roadmap/companyos.md#step-co-04) | CompanyOS · 五部门与专业岗位成为版本化目录 | `CO-03` | ⏳ | [专项卡](roadmap/companyos.md#step-co-04) |
| 055 | W1 | 专项 | [`CO-05`](roadmap/companyos.md#step-co-05) | CompanyOS · 业务命令权限、责任和人工决定合同 | `CO-03`、`CO-04` | ⏳ | [专项卡](roadmap/companyos.md#step-co-05) |
| 056 | W1 | 专项 | [`CO-06`](roadmap/companyos.md#step-co-06) | CompanyOS · 不可变工件、Evidence 与 Criterion 引用合同 | `CO-02`、`CO-05` | ⏳ | [专项卡](roadmap/companyos.md#step-co-06) |
| 057 | W1 | 专项 | [`CO-07`](roadmap/companyos.md#step-co-07) | CompanyOS · 版本化业务事实与稳定命令回执 | `CO-05`、`CO-06` | ⏳ | [专项卡](roadmap/companyos.md#step-co-07) |
| 058 | W1 | 专项 | [`CO-08`](roadmap/companyos.md#step-co-08) | CompanyOS · 业务状态机、历史重放与兼容迁移 | `CO-07` | ⏳ | [专项卡](roadmap/companyos.md#step-co-08) |
| 059 | W1 | 基础 | [`P0-A-01b`](#step-p0-a-01b) | P0 基础 · schema 注册表与 unknown field/migration 规则 | `P0-A-01a` | ⏳ | [基础卡](#step-p0-a-01b) |
| 060 | W1 | 基础 | [`P0-A-02`](#step-p0-a-02) | P0 基础 · 稳定错误码枚举 | `P0-A-01a` | ⏳ | [基础卡](#step-p0-a-02) |
| 061 | W1 | 专项 | [`P4-J7-06`](roadmap/provider.md#step-p4-j7-06) | Provider · 中立内容、调用身份、错误与模型端口 | `P4-J7-05`、`P0-A-01b`、`P0-A-02`、`CP-02` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-06) |
| 062 | W1 | 专项 | [`P4-J7-07`](roadmap/provider.md#step-p4-j7-07) | Provider · 提取 kiana-provider 并迁移装配 | `P4-J7-06` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-07) |
| 063 | W1 | 专项 | [`P4-J7-08`](roadmap/provider.md#step-p4-j7-08) | Provider · 连接、profile 和配置快照 | `P4-J7-07` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-08) |
| 064 | W1 | 专项 | [`P4-J7-09`](roadmap/provider.md#step-p4-j7-09) | Provider · 凭据管理与 HTTP 目标校验 | `P4-J7-08` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-09) |
| 065 | W1 | 专项 | [`P4-J7-10`](roadmap/provider.md#step-p4-j7-10) | Provider · 能力目录、未知能力和显式 discovery | `P4-J7-08` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-10) |
| 066 | W1 | 基础 | [`P0-B-01`](#step-p0-b-01) | P0 基础 · 正式状态机转移表 | `P0-A-01a` | ⏳ | [基础卡](#step-p0-b-01) |
| 067 | W1 | 基础 | [`P0-K1-01`](#step-p0-k1-01) | P0 基础 · 服务端身份与 authority epoch | `P0-A-01a` | ⏳ | [基础卡](#step-p0-k1-01) |
| 068 | W1 | 基础 | [`P1-C-01`](#step-p1-c-01) | P1 基础 · 组织与 Cell 契约 | `P0-A-01a` | ⏳ | [基础卡](#step-p1-c-01) |
| 069 | W1 | 基础 | [`P1-C-03`](#step-p1-c-03) | P1 基础 · 五部门角色目录与 model_profile 接线 | `P1-C-01` | ⏳ | [基础卡](#step-p1-c-03) |
| 070 | W1 | 基础 | [`P1-D-01`](#step-p1-d-01) | P1 基础 · WorkPacket 单一 ready 谓词 | `P0-A-01a` | ⏳ | [基础卡](#step-p1-d-01) |
| 071 | W1 | 基础 | [`P1-D-02`](#step-p1-d-02) | P1 基础 · 依赖缺失 / 成环 fail-closed | `P1-D-01` | ⏳ | [基础卡](#step-p1-d-02) |
| 072 | W1 | 基础 | [`P1-E-01`](#step-p1-e-01) | P1 基础 · 通信与问责分层 | `P0-B-01` | ⏳ | [基础卡](#step-p1-e-01) |
| 073 | W1 | 基础 | [`P1-H-01`](#step-p1-h-01) | P1 基础 · `ToolSpec` registry | `P0-A-01a` | ⏳ | [基础卡](#step-p1-h-01) |
| 074 | W1 | 基础 | [`P1-H-03`](#step-p1-h-03) | P1 基础 · 路径 containment 共享实现 | `P1-H-01` | ⏳ | [基础卡](#step-p1-h-03) |
| 075 | W1 | 基础 | [`P3-I-01`](#step-p3-i-01) | P3 基础 · Company 业务对象契约 | `P0-A-01a` | ⏳ | [基础卡](#step-p3-i-01) |
| **W2** | **事实账本、授权与资源** |  |  |  |  |  |  |
| 076 | W2 | 专项 | [`CP-07`](roadmap/control-plane.md#step-cp-07) | ControlPlane · 实现 JSONL 事务帧及失败恢复 | `CP-06` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-07) |
| 077 | W2 | 专项 | [`CP-08`](roadmap/control-plane.md#step-cp-08) | ControlPlane · Grant 账本与 authority epoch | `CP-01`、`CP-04`、`CP-07` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-08) |
| 078 | W2 | 专项 | [`CP-09`](roadmap/control-plane.md#step-cp-09) | ControlPlane · 精确审批 subject 与可恢复材料 | `CP-03`、`CP-07`、`CP-08` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-09) |
| 079 | W2 | 专项 | [`CP-10`](roadmap/control-plane.md#step-cp-10) | ControlPlane · 审批决定、单次消费和 pending 持久化 | `CP-05`、`CP-07`、`CP-09` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-10) |
| 080 | W2 | 专项 | [`CP-11`](roadmap/control-plane.md#step-cp-11) | ControlPlane · 模型与工具统一消耗预算 | `CP-02`、`CP-07`、`CP-08` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-11) |
| 081 | W2 | 专项 | [`CP-12`](roadmap/control-plane.md#step-cp-12) | ControlPlane · 路径锁、资源租约和 fencing token | `CP-07`、`CP-08` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-12) |
| 082 | W2 | 专项 | [`CP-13`](roadmap/control-plane.md#step-cp-13) | ControlPlane · 执行许可与派发线性化点 | `CP-05`、`CP-10`、`CP-11`、`CP-12` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-13) |
| 083 | W2 | 专项 | [`CP-14`](roadmap/control-plane.md#step-cp-14) | ControlPlane · 统一执行结果、核销和结果回灌 | `CP-13` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-14) |
| 084 | W2 | 专项 | [`ER-05`](roadmap/event-receipt-recovery.md#step-er-05) | Event / Receipt / Recovery · JSONL v2 原子 frame、锁与损坏策略 | `ER-04` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-05) |
| 085 | W2 | 专项 | [`ER-06`](roadmap/event-receipt-recovery.md#step-er-06) | Event / Receipt / Recovery · 异步写入、背压与 shutdown ack | `ER-05` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-06) |
| 086 | W2 | 专项 | [`ER-07`](roadmap/event-receipt-recovery.md#step-er-07) | Event / Receipt / Recovery · 通用 replay reader 和 projector checkpoint | `ER-06` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-07) |
| 087 | W2 | 专项 | [`ER-08`](roadmap/event-receipt-recovery.md#step-er-08) | Event / Receipt / Recovery · Run/Turn 状态投影和 terminal 约束 | `ER-07` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-08) |
| 088 | W2 | 专项 | [`ER-09`](roadmap/event-receipt-recovery.md#step-er-09) | Event / Receipt / Recovery · Invocation/Execution/Attempt 投影 | `ER-08` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-09) |
| 089 | W2 | 专项 | [`ER-10`](roadmap/event-receipt-recovery.md#step-er-10) | Event / Receipt / Recovery · Approval、Budget、Lease 和 pending projection | `ER-09` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-10) |
| 090 | W2 | 专项 | [`ER-11`](roadmap/event-receipt-recovery.md#step-er-11) | Event / Receipt / Recovery · Receipt DTO、redacted view 与 source cursor | `ER-10` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-11) |
| 091 | W2 | 专项 | [`ER-12`](roadmap/event-receipt-recovery.md#step-er-12) | Event / Receipt / Recovery · Cost、files、model turns 与 evidence aggregation | `ER-11` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-12) |
| 092 | W2 | 专项 | [`ER-13`](roadmap/event-receipt-recovery.md#step-er-13) | Event / Receipt / Recovery · 统一 result commit 和 result delivery | `ER-12` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-13) |
| 093 | W2 | 专项 | [`ER-14`](roadmap/event-receipt-recovery.md#step-er-14) | Event / Receipt / Recovery · Effect Receipt 与外部 provider receipt | `ER-13` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-14) |
| 094 | W2 | 专项 | [`ER-15`](roadmap/event-receipt-recovery.md#step-er-15) | Event / Receipt / Recovery · Hook、MCP、Memory、Patch 结果统一边界 | `ER-14` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-15) |
| 095 | W2 | 专项 | [`ER-16`](roadmap/event-receipt-recovery.md#step-er-16) | Event / Receipt / Recovery · 终态事件唯一性与 terminal 必达 | `ER-15` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-16) |
| 096 | W2 | 专项 | [`CAP-05`](roadmap/capability.md#step-cap-05) | Capability · 核验授权事实，原子领取一次执行 | `CAP-03`、`CAP-04` | ⏳ | [专项卡](roadmap/capability.md#step-cap-05) |
| 097 | W2 | 专项 | [`CAP-06`](roadmap/capability.md#step-cap-06) | Capability · 审批看见并绑定将被执行的最终计划 | `CAP-05` | ⏳ | [专项卡](roadmap/capability.md#step-cap-06) |
| 098 | W2 | 专项 | [`H06`](roadmap/harness.md#step-h06) | Harness · 一个流归一化器产生增量与完整响应 | `H05` | ⏳ | [专项卡](roadmap/harness.md#step-h06) |
| 099 | W2 | 专项 | [`H07`](roadmap/harness.md#step-h07) | Harness · 贯通预算配置、预留与累计结算 | `H05` | ⏳ | [专项卡](roadmap/harness.md#step-h07) |
| 100 | W2 | 专项 | [`H08`](roadmap/harness.md#step-h08) | Harness · 将 deadline 和取消贯穿静默 I/O | `H03`、`H06`、`H07` | ⏳ | [专项卡](roadmap/harness.md#step-h08) |
| 101 | W2 | 专项 | [`H09`](roadmap/harness.md#step-h09) | Harness · 工具目录成为单一、可版本化的数据源 | `H04`、`H05` | ⏳ | [专项卡](roadmap/harness.md#step-h09) |
| 102 | W2 | 专项 | [`H10`](roadmap/harness.md#step-h10) | Harness · 一次生成、全程稳定的调用身份 | `H02`、`H09` | ⏳ | [专项卡](roadmap/harness.md#step-h10) |
| 103 | W2 | 专项 | [`H11`](roadmap/harness.md#step-h11) | Harness · 工具结果分类与给模型的可修复反馈 | `H05`、`H10` | ⏳ | [专项卡](roadmap/harness.md#step-h11) |
| 104 | W2 | 专项 | [`H12`](roadmap/harness.md#step-h12) | Harness · 串行批次先完整闭环，再考虑并行 | `H08`、`H10`、`H11` | ⏳ | [专项卡](roadmap/harness.md#step-h12) |
| 105 | W2 | 专项 | [`H13`](roadmap/harness.md#step-h13) | Harness · Invocation 账本与结果立即持久化 | `H10`、`H11`、`H12` | ⏳ | [专项卡](roadmap/harness.md#step-h13) |
| 106 | W2 | 专项 | [`H14`](roadmap/harness.md#step-h14) | Harness · 审批暂停与原调用恢复 | `H12`、`H13` | ⏳ | [专项卡](roadmap/harness.md#step-h14) |
| 107 | W2 | 专项 | [`CM-05`](roadmap/context-memory.md#step-cm-05) | Context / Memory · EventStore 唯一提交点与 JSONL/index 投影 | `CM-04` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-05) |
| 108 | W2 | 专项 | [`CM-06`](roadmap/context-memory.md#step-cm-06) | Context / Memory · Source dependency graph 与治理 epoch | `CM-05` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-06) |
| 109 | W2 | 基础 | [`P0-F-01`](#step-p0-f-01) | P0 基础 · 审批一等请求/应答 | `P0-B-01` | ⏳ | [基础卡](#step-p0-f-01) |
| 110 | W2 | 基础 | [`P0-F-02`](#step-p0-f-02) | P0 基础 · 审批决定事件与单次消费 | `P0-F-01` | ⏳ | [基础卡](#step-p0-f-02) |
| 111 | W2 | 基础 | [`P0-J1-01`](#step-p0-j1-01) | P0 基础 · 统一 cancellation token 与状态词表 | `P0-B-01` | ⏳ | [基础卡](#step-p0-j1-01) |
| 112 | W2 | 基础 | [`P1-C-02`](#step-p1-c-02) | P1 基础 · Cell 生命周期与 retire | `P1-C-01` | ⏳ | [基础卡](#step-p1-c-02) |
| 113 | W2 | 基础 | [`P1-D-03`](#step-p1-d-03) | P1 基础 · claim / lease 心跳回收 | `P1-D-01` | ⏳ | [基础卡](#step-p1-d-03) |
| 114 | W2 | 基础 | [`P1-J8-01`](#step-p1-j8-01) | P1 基础 · Observability 与 trace/receipt | `P0-G-04` | ⏳ | [基础卡](#step-p1-j8-01) |
| 115 | W2 | 基础 | [`P1-K5-01`](#step-p1-k5-01) | P1 基础 · 成本与容量账本 | `P0-G-04` | ⏳ | [基础卡](#step-p1-k5-01) |
| 116 | W2 | 基础 | [`P3-I-02`](#step-p3-i-02) | P3 基础 · 命令与事件冻结 | `P3-I-01` | ⏳ | [基础卡](#step-p3-i-02) |
| 117 | W2 | 基础 | [`P4-J7-02`](#step-p4-j7-02) | P4 基础 · wire 加 `sequence`/`epoch` | `P0-J7-01` | ⏳ | [基础卡](#step-p4-j7-02) |
| 118 | W2 | 基础 | [`P4-J7-03`](#step-p4-j7-03) | P4 基础 · 事件种类补齐与 terminal 必达 | `P4-J7-02` | ⏳ | [基础卡](#step-p4-j7-03) |
| **W3** | **实际执行、取消与恢复** |  |  |  |  |  |  |
| 119 | W3 | 专项 | [`CP-15`](roadmap/control-plane.md#step-cp-15) | ControlPlane · 统一取消状态，覆盖审批与排队竞态 | `CP-02`、`CP-07`、`CP-13`、`CP-14` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-15) |
| 120 | W3 | 专项 | [`CP-16`](roadmap/control-plane.md#step-cp-16) | ControlPlane · Handler 真正停止与文件提交证据 | `CP-12`、`CP-13`、`CP-15` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-16) |
| 121 | W3 | 专项 | [`CP-17`](roadmap/control-plane.md#step-cp-17) | ControlPlane · 撤销、失败清理与 Cell 退休 | `CP-08`、`CP-11`、`CP-12`、`CP-15`、`CP-16` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-17) |
| 122 | W3 | 专项 | [`CP-18`](roadmap/control-plane.md#step-cp-18) | ControlPlane · RunSnapshot 与安全 checkpoint | `CP-07`、`CP-09`、`CP-14`、`CP-17` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-18) |
| 123 | W3 | 专项 | [`CP-19`](roadmap/control-plane.md#step-cp-19) | ControlPlane · 显式 Resume 与新进程重建 | `CP-10`、`CP-14`、`CP-17`、`CP-18` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-19) |
| 124 | W3 | 专项 | [`CP-20`](roadmap/control-plane.md#step-cp-20) | ControlPlane · Unknown 对账、重试与补偿 | `CP-14`、`CP-17`、`CP-19` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-20) |
| 125 | W3 | 专项 | [`CP-21`](roadmap/control-plane.md#step-cp-21) | ControlPlane · 投影、Receipt 和只读查询 | `CP-07`、`CP-10`、`CP-14`、`CP-19` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-21) |
| 126 | W3 | 专项 | [`CP-22`](roadmap/control-plane.md#step-cp-22) | ControlPlane · Protocol、动作卡与各入口同一事实 | `CP-10`、`CP-15`、`CP-19`、`CP-21` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-22) |
| 127 | W3 | 专项 | [`CP-26`](roadmap/control-plane.md#step-cp-26) | ControlPlane · 决策解释、审计关联与证据 | `CP-14`、`CP-20`、`CP-21` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-26) |
| 128 | W3 | 专项 | [`CP-27`](roadmap/control-plane.md#step-cp-27) | ControlPlane · 非阻塞存储、时钟和资源限额 | `CP-07`、`CP-11`、`CP-15`、`CP-21` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-27) |
| 129 | W3 | 专项 | [`CP-28`](roadmap/control-plane.md#step-cp-28) | ControlPlane · 迁移、兼容 adapter 与旁路收口 | `CP-02`、`CP-07`、`CP-19`、`CP-22` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-28) |
| 130 | W3 | 专项 | [`ER-17`](roadmap/event-receipt-recovery.md#step-er-17) | Event / Receipt / Recovery · Serializable RunSnapshot 与 pending writes | `ER-16` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-17) |
| 131 | W3 | 专项 | [`ER-18`](roadmap/event-receipt-recovery.md#step-er-18) | Event / Receipt / Recovery · Workspace checkpoint transaction 与 restore evidence | `ER-17` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-18) |
| 132 | W3 | 专项 | [`ER-19`](roadmap/event-receipt-recovery.md#step-er-19) | Event / Receipt / Recovery · Worker/process handle 与 fencing token | `ER-18` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-19) |
| 133 | W3 | 专项 | [`ER-20`](roadmap/event-receipt-recovery.md#step-er-20) | Event / Receipt / Recovery · Restart projector 与默认暂停 | `ER-19` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-20) |
| 134 | W3 | 专项 | [`ER-21`](roadmap/event-receipt-recovery.md#step-er-21) | Event / Receipt / Recovery · Explicit resume preflight and claim | `ER-20` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-21) |
| 135 | W3 | 专项 | [`ER-22`](roadmap/event-receipt-recovery.md#step-er-22) | Event / Receipt / Recovery · Cancel recovery and stop confirmation | `ER-21` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-22) |
| 136 | W3 | 专项 | [`ER-23`](roadmap/event-receipt-recovery.md#step-er-23) | Event / Receipt / Recovery · Unknown incident 与 RecoveryPlan | `ER-22` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-23) |
| 137 | W3 | 专项 | [`ER-24`](roadmap/event-receipt-recovery.md#step-er-24) | Event / Receipt / Recovery · Reconciliation commands and evidence | `ER-23` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-24) |
| 138 | W3 | 专项 | [`ER-25`](roadmap/event-receipt-recovery.md#step-er-25) | Event / Receipt / Recovery · Retry policy and new attempt | `ER-24` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-25) |
| 139 | W3 | 专项 | [`ER-26`](roadmap/event-receipt-recovery.md#step-er-26) | Event / Receipt / Recovery · Cursor query、snapshot 和慢消费者 | `ER-25` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-26) |
| 140 | W3 | 专项 | [`ER-27`](roadmap/event-receipt-recovery.md#step-er-27) | Event / Receipt / Recovery · 四入口统一只读 receipt/recovery commands | `ER-26` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-27) |
| 141 | W3 | 专项 | [`CAP-07`](roadmap/capability.md#step-cap-07) | Capability · EnvironmentPort 与可验证 backend 选择 | `CAP-03`、`CAP-04` | ⏳ | [专项卡](roadmap/capability.md#step-cap-07) |
| 142 | W3 | 专项 | [`CAP-08`](roadmap/capability.md#step-cap-08) | Capability · 共享 PathResolver 与文件身份前置条件 | `CAP-07` | ⏳ | [专项卡](roadmap/capability.md#step-cap-08) |
| 143 | W3 | 专项 | [`CAP-09`](roadmap/capability.md#step-cap-09) | Capability · Linux 最小文件视图与不可扩大的挂载集 | `CAP-08` | ⏳ | [专项卡](roadmap/capability.md#step-cap-09) |
| 144 | W3 | 专项 | [`CAP-10`](roadmap/capability.md#step-cap-10) | Capability · 为严格 packet 写集提供隔离写层 | `CAP-09` | ⏳ | [专项卡](roadmap/capability.md#step-cap-10) |
| 145 | W3 | 专项 | [`CAP-11`](roadmap/capability.md#step-cap-11) | Capability · 清理 ambient authority，落实默认断网 | `CAP-09` | ⏳ | [专项卡](roadmap/capability.md#step-cap-11) |
| 146 | W3 | 专项 | [`CAP-12`](roadmap/capability.md#step-cap-12) | Capability · ProcessSupervisor 持有进程树与资源预算 | `CAP-07`、`CAP-11` | ⏳ | [专项卡](roadmap/capability.md#step-cap-12) |
| 147 | W3 | 专项 | [`CAP-13`](roadmap/capability.md#step-cap-13) | Capability · 统一有界输出与脱敏工件 | `CAP-12` | ⏳ | [专项卡](roadmap/capability.md#step-cap-13) |
| 148 | W3 | 专项 | [`CAP-14`](roadmap/capability.md#step-cap-14) | Capability · shell 适配迁入统一执行器 | `CAP-05`、`CAP-12`、`CAP-13` | ⏳ | [专项卡](roadmap/capability.md#step-cap-14) |
| 149 | W3 | 专项 | [`CAP-15`](roadmap/capability.md#step-cap-15) | Capability · patch 一次解析，所有受影响路径可预览 | `CAP-02`、`CAP-08` | ⏳ | [专项卡](roadmap/capability.md#step-cap-15) |
| 150 | W3 | 专项 | [`CAP-16`](roadmap/capability.md#step-cap-16) | Capability · patch 提交点、有限回滚和崩溃恢复 | `CAP-10`、`CAP-15` | ⏳ | [专项卡](roadmap/capability.md#step-cap-16) |
| 151 | W3 | 专项 | [`CAP-17`](roadmap/capability.md#step-cap-17) | Capability · 取消与撤销作用于整个 execution 集合 | `CAP-04`、`CAP-12`、`CAP-14`、`CAP-16` | ⏳ | [专项卡](roadmap/capability.md#step-cap-17) |
| 152 | W3 | 专项 | [`CAP-18`](roadmap/capability.md#step-cap-18) | Capability · Hook 本身受控，改写输入重新授权 | `CAP-06`、`CAP-12`、`CAP-17` | ⏳ | [专项卡](roadmap/capability.md#step-cap-18) |
| 153 | W3 | 专项 | [`CAP-19`](roadmap/capability.md#step-cap-19) | Capability · Memory 使用逻辑资源 scope 与可靠提交监督 | `CAP-03`、`CAP-08`、`CAP-17` | ⏳ | [专项卡](roadmap/capability.md#step-cap-19) |
| 154 | W3 | 专项 | [`CAP-20`](roadmap/capability.md#step-cap-20) | Capability · MCP 先建立可信配置与 discovery snapshot | `CAP-01`、`CAP-03`、`CAP-05`、`CAP-12` | ⏳ | [专项卡](roadmap/capability.md#step-cap-20) |
| 155 | W3 | 专项 | [`CAP-21`](roadmap/capability.md#step-cap-21) | Capability · stdio MCP 协议和停止全过程有界 | `CAP-13`、`CAP-17`、`CAP-20` | ⏳ | [专项卡](roadmap/capability.md#step-cap-21) |
| 156 | W3 | 专项 | [`CAP-22`](roadmap/capability.md#step-cap-22) | Capability · MCP result/schema drift 和连接复用隔离 | `CAP-06`、`CAP-21` | ⏳ | [专项卡](roadmap/capability.md#step-cap-22) |
| 157 | W3 | 专项 | [`CAP-23`](roadmap/capability.md#step-cap-23) | Capability · 并发调度与资源冲突一致 | `CAP-10`、`CAP-17`、`CAP-19`、`CAP-22` | ⏳ | [专项卡](roadmap/capability.md#step-cap-23) |
| 158 | W3 | 专项 | [`CAP-24`](roadmap/capability.md#step-cap-24) | Capability · 以事实重建 Invocation，限制重试并支持对账 | `CAP-05`、`CAP-06`、`CAP-16`、`CAP-17`、`CAP-22`、`CAP-23` | ⏳ | [专项卡](roadmap/capability.md#step-cap-24) |
| 159 | W3 | 专项 | [`CAP-25`](roadmap/capability.md#step-cap-25) | Capability · Receipt、模型与四入口看到一致事实 | `CAP-04`、`CAP-13`、`CAP-24` | ⏳ | [专项卡](roadmap/capability.md#step-cap-25) |
| 160 | W3 | 专项 | [`H15`](roadmap/harness.md#step-h15) | Harness · 工具输出有界、完整结果可按需读取 | `H09`、`H11`、`H13` | ⏳ | [专项卡](roadmap/harness.md#step-h15) |
| 161 | W3 | 专项 | [`H16`](roadmap/harness.md#step-h16) | Harness · 有界并行工具组与独占屏障 | `H08`、`H09`、`H12`、`H13`、`H15` | ⏳ | [专项卡](roadmap/harness.md#step-h16) |
| 162 | W3 | 专项 | [`H17`](roadmap/harness.md#step-h17) | Harness · 后台进程和长工具的可恢复句柄 | `H08`、`H13`、`H15` | ⏳ | [专项卡](roadmap/harness.md#step-h17) |
| 163 | W3 | 专项 | [`H18`](roadmap/harness.md#step-h18) | Harness · 持久 Inbox、ACK 与原子消费 | `H02`、`H03`、`H13` | ⏳ | [专项卡](roadmap/harness.md#step-h18) |
| 164 | W3 | 专项 | [`H19`](roadmap/harness.md#step-h19) | Harness · Continue / Steer / Inject 的产品接线 | `H08`、`H18` | ⏳ | [专项卡](roadmap/harness.md#step-h19) |
| 165 | W3 | 基础 | [`P0-G-03`](#step-p0-g-03) | P0 基础 · `resume_run` 与协议入口 | `P0-G-02b` | ⏳ | [基础卡](#step-p0-g-03) |
| 166 | W3 | 基础 | [`P0-F-03`](#step-p0-f-03) | P0 基础 · 续跑材料落盘与 RunSnapshot | `P0-G-02b`、`P0-G-03`、`P0-F-02` | ⏳ | [基础卡](#step-p0-f-03) |
| 167 | W3 | 基础 | [`P0-J1-02`](#step-p0-j1-02) | P0 基础 · 排空已启动工作 + 合成未启动结果 | `P0-J1-01` | ⏳ | [基础卡](#step-p0-j1-02) |
| 168 | W3 | 基础 | [`P0-J1-03`](#step-p0-j1-03) | P0 基础 · 进程组确认与 `stop_confirmed` | `P0-J1-01` | ⏳ | [基础卡](#step-p0-j1-03) |
| 169 | W3 | 基础 | [`P0-J1-04`](#step-p0-j1-04) | P0 基础 · 取消竞态负向证据 | `P0-J1-01`、`P0-J1-02`、`P0-J1-03` | ⏳ | [基础卡](#step-p0-j1-04) |
| 170 | W3 | 基础 | [`P1-J4-01`](#step-p1-j4-01) | P1 基础 · Capability Descriptor 与 MCP 生命周期 | `P0-A-01a` | ⏳ | [基础卡](#step-p1-j4-01) |
| 171 | W3 | 基础 | [`P2-K4-01`](#step-p2-k4-01) | P2 基础 · Artifact 版本与编辑级 undo | `P0-G-04` | ⏳ | [基础卡](#step-p2-k4-01) |
| 172 | W3 | 基础 | [`P2-K6-01`](#step-p2-k6-01) | P2 基础 · 可靠性与对账 | `P2-K4-01` | ⏳ | [基础卡](#step-p2-k6-01) |
| **W4** | **上下文、记忆与扩展** |  |  |  |  |  |  |
| 173 | W4 | 专项 | [`CP-25`](roadmap/control-plane.md#step-cp-25) | ControlPlane · Skills、Hooks、Memory、MCP 与 Secret 的统一边界 | `CP-03`、`CP-04`、`CP-08`、`CP-13`、`CP-18` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-25) |
| 174 | W4 | 专项 | [`H20`](roadmap/harness.md#step-h20) | Harness · 不可变 StepContext 与可解释的上下文编译 | `H04`、`H09`、`H18` | ⏳ | [专项卡](roadmap/harness.md#step-h20) |
| 175 | W4 | 专项 | [`H21`](roadmap/harness.md#step-h21) | Harness · 真实请求预算与稳定缓存前缀 | `H07`、`H20` | ⏳ | [专项卡](roadmap/harness.md#step-h21) |
| 176 | W4 | 专项 | [`H22`](roadmap/harness.md#step-h22) | Harness · 真正保留工作状态的 Compaction | `H05`、`H11`、`H15`、`H20`、`H21` | ⏳ | [专项卡](roadmap/harness.md#step-h22) |
| 177 | W4 | 专项 | [`H23`](roadmap/harness.md#step-h23) | Harness · 压缩结果提交、来源和失效传播 | `H13`、`H18`、`H22` | ⏳ | [专项卡](roadmap/harness.md#step-h23) |
| 178 | W4 | 专项 | [`H24`](roadmap/harness.md#step-h24) | Harness · 完整检查点与显式 Resume | `H13`、`H14`、`H17`、`H18`、`H23` | ⏳ | [专项卡](roadmap/harness.md#step-h24) |
| 179 | W4 | 专项 | [`H25`](roadmap/harness.md#step-h25) | Harness · 重放、故障注入与 Unknown 对账 | `H13`、`H16`、`H23`、`H24` | ⏳ | [专项卡](roadmap/harness.md#step-h25) |
| 180 | W4 | 专项 | [`H26`](roadmap/harness.md#step-h26) | Harness · 澄清请求与权限审批分离 | `H09`、`H14`、`H18`、`H19`、`H24` | ⏳ | [专项卡](roadmap/harness.md#step-h26) |
| 181 | W4 | 专项 | [`H27`](roadmap/harness.md#step-h27) | Harness · 结构化输出与准确的 TurnOutcome | `H05`、`H11`、`H14`、`H26` | ⏳ | [专项卡](roadmap/harness.md#step-h27) |
| 182 | W4 | 专项 | [`H28`](roadmap/harness.md#step-h28) | Harness · 进度、停滞检测与有界修复策略 | `H07`、`H11`、`H17`、`H27` | ⏳ | [专项卡](roadmap/harness.md#step-h28) |
| 183 | W4 | 专项 | [`H29`](roadmap/harness.md#step-h29) | Harness · 有序、受约束的 Hooks / Skills 扩展点 | `H09`、`H20`、`H27`、`H28` | ⏳ | [专项卡](roadmap/harness.md#step-h29) |
| 184 | W4 | 专项 | [`H30`](roadmap/harness.md#step-h30) | Harness · 检索、Memory 与代码索引进入同一 ContextPlan | `H15`、`H20`、`H21`、`H23`、`H29` | ⏳ | [专项卡](roadmap/harness.md#step-h30) |
| 185 | W4 | 专项 | [`CM-07`](roadmap/context-memory.md#step-cm-07) | Context / Memory · Workspace/artifact snapshot 与安全读取 | `CM-06` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-07) |
| 186 | W4 | 专项 | [`CM-08`](roadmap/context-memory.md#step-cm-08) | Context / Memory · 稳定 chunker 与 offset provenance | `CM-07` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-08) |
| 187 | W4 | 专项 | [`CM-09`](roadmap/context-memory.md#step-cm-09) | Context / Memory · 文本规范化、语言和敏感数据边界 | `CM-08` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-09) |
| 188 | W4 | 专项 | [`CM-10`](roadmap/context-memory.md#step-cm-10) | Context / Memory · ContextIndex generation 与原子切换 | `CM-09` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-10) |
| 189 | W4 | 专项 | [`CM-11`](roadmap/context-memory.md#step-cm-11) | Context / Memory · 增量更新、rename/delete 与缓存失效 | `CM-10` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-11) |
| 190 | W4 | 专项 | [`CM-12`](roadmap/context-memory.md#step-cm-12) | Context / Memory · 统一 sparse/dense/RRF/MMR 检索器 | `CM-11` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-12) |
| 191 | W4 | 专项 | [`CM-13`](roadmap/context-memory.md#step-cm-13) | Context / Memory · Repo map 任务相关排序和依赖证据 | `CM-12` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-13) |
| 192 | W4 | 专项 | [`CM-14`](roadmap/context-memory.md#step-cm-14) | Context / Memory · 检索结果 provenance、freshness 与 health | `CM-13` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-14) |
| 193 | W4 | 专项 | [`CM-15`](roadmap/context-memory.md#step-cm-15) | Context / Memory · ContextPlan 选材与 omission 解释 | `CM-14` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-15) |
| 194 | W4 | 专项 | [`CM-16`](roadmap/context-memory.md#step-cm-16) | Context / Memory · 真实 wire budget 与稳定前缀 | `CM-15` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-16) |
| 195 | W4 | 专项 | [`CM-17`](roadmap/context-memory.md#step-cm-17) | Context / Memory · ResolvedStepContext 单一快照 | `CM-16` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-17) |
| 196 | W4 | 专项 | [`CM-18`](roadmap/context-memory.md#step-cm-18) | Context / Memory · 工具结果有界预览与受控 spill | `CM-17` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-18) |
| 197 | W4 | 专项 | [`CM-19`](roadmap/context-memory.md#step-cm-19) | Context / Memory · CompactSummary 结构化生成与校验 | `CM-18` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-19) |
| 198 | W4 | 专项 | [`CM-20`](roadmap/context-memory.md#step-cm-20) | Context / Memory · ContextCheckpoint CAS 提交和新输入并发 | `CM-19` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-20) |
| 199 | W4 | 专项 | [`CM-21`](roadmap/context-memory.md#step-cm-21) | Context / Memory · Resume、cache 和删除失效 | `CM-20` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-21) |
| 200 | W4 | 专项 | [`CM-22`](roadmap/context-memory.md#step-cm-22) | Context / Memory · 六层 ACL 与逐记录过滤统一化 | `CM-21` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-22) |
| 201 | W4 | 专项 | [`CM-23`](roadmap/context-memory.md#step-cm-23) | Context / Memory · candidate / scratch / user-private 负向门 | `CM-22` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-23) |
| 202 | W4 | 专项 | [`CM-24`](roadmap/context-memory.md#step-cm-24) | Context / Memory · 相关性、时效、冲突与历史查询 | `CM-23` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-24) |
| 203 | W4 | 专项 | [`CM-25`](roadmap/context-memory.md#step-cm-25) | Context / Memory · Turn extraction proposal 管线 | `CM-24` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-25) |
| 204 | W4 | 专项 | [`CM-26`](roadmap/context-memory.md#step-cm-26) | Context / Memory · Distillation、decision 与 lesson 统一入库 | `CM-25` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-26) |
| 205 | W4 | 专项 | [`CM-27`](roadmap/context-memory.md#step-cm-27) | Context / Memory · Retrieval/selection/citation receipt | `CM-26` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-27) |
| 206 | W4 | 专项 | [`CM-28`](roadmap/context-memory.md#step-cm-28) | Context / Memory · 删除、过期、撤销传播 | `CM-27` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-28) |
| 207 | W4 | 专项 | [`CM-29`](roadmap/context-memory.md#step-cm-29) | Context / Memory · Projection lag、recovery 与 result_unknown | `CM-28` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-29) |
| 208 | W4 | 专项 | [`EXT-06`](roadmap/skills-plugins-hooks.md#step-ext-06) | Skills / Plugins / Hooks · 三层渐进披露 | `EXT-05` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-06) |
| 209 | W4 | 专项 | [`EXT-07`](roadmap/skills-plugins-hooks.md#step-ext-07) | Skills / Plugins / Hooks · 显式激活与包内资源 | `EXT-06` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-07) |
| 210 | W4 | 专项 | [`EXT-08`](roadmap/skills-plugins-hooks.md#step-ext-08) | Skills / Plugins / Hooks · 条件 Skill、路径和参数 | `EXT-07` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-08) |
| 211 | W4 | 专项 | [`EXT-09`](roadmap/skills-plugins-hooks.md#step-ext-09) | Skills / Plugins / Hooks · Prompt provenance 与预算 | `EXT-05`、`EXT-06` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-09) |
| 212 | W4 | 专项 | [`EXT-10`](roadmap/skills-plugins-hooks.md#step-ext-10) | Skills / Plugins / Hooks · Skill invocation 兼容 | `EXT-08`、`EXT-09` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-10) |
| 213 | W4 | 专项 | [`EXT-11`](roadmap/skills-plugins-hooks.md#step-ext-11) | Skills / Plugins / Hooks · Hook schema 与事件 | `EXT-02` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-11) |
| 214 | W4 | 专项 | [`EXT-12`](roadmap/skills-plugins-hooks.md#step-ext-12) | Skills / Plugins / Hooks · Discovery、匹配和聚合输入 | `EXT-11`、`EXT-04` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-12) |
| 215 | W4 | 专项 | [`EXT-13`](roadmap/skills-plugins-hooks.md#step-ext-13) | Skills / Plugins / Hooks · ProcessSupervisor | `EXT-12` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-13) |
| 216 | W4 | 专项 | [`EXT-14`](roadmap/skills-plugins-hooks.md#step-ext-14) | Skills / Plugins / Hooks · Outcome 与失败策略 | `EXT-13` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-14) |
| 217 | W4 | 专项 | [`EXT-15`](roadmap/skills-plugins-hooks.md#step-ext-15) | Skills / Plugins / Hooks · PreTool 最终输入重新授权 | `EXT-14` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-15) |
| 218 | W4 | 专项 | [`EXT-16`](roadmap/skills-plugins-hooks.md#step-ext-16) | Skills / Plugins / Hooks · 全生命周期事件接线 | `EXT-14` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-16) |
| 219 | W4 | 专项 | [`EXT-17`](roadmap/skills-plugins-hooks.md#step-ext-17) | Skills / Plugins / Hooks · 取消、递归、异步 observer | `EXT-13`、`EXT-16` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-17) |
| 220 | W4 | 专项 | [`EXT-18`](roadmap/skills-plugins-hooks.md#step-ext-18) | Skills / Plugins / Hooks · Receipt、重放和恢复 | `EXT-15`、`EXT-16`、`EXT-17` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-18) |
| 221 | W4 | 专项 | [`EXT-19`](roadmap/skills-plugins-hooks.md#step-ext-19) | Skills / Plugins / Hooks · Plugin manifest v2 | `EXT-03`、`EXT-04` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-19) |
| 222 | W4 | 专项 | [`EXT-20`](roadmap/skills-plugins-hooks.md#step-ext-20) | Skills / Plugins / Hooks · 供应链和不可变包 | `EXT-19` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-20) |
| 223 | W4 | 专项 | [`EXT-21`](roadmap/skills-plugins-hooks.md#step-ext-21) | Skills / Plugins / Hooks · 依赖图与 binding | `EXT-19`、`EXT-20` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-21) |
| 224 | W4 | 专项 | [`EXT-22`](roadmap/skills-plugins-hooks.md#step-ext-22) | Skills / Plugins / Hooks · inspect → stage → install → enable | `EXT-20`、`EXT-21` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-22) |
| 225 | W4 | 专项 | [`EXT-23`](roadmap/skills-plugins-hooks.md#step-ext-23) | Skills / Plugins / Hooks · upgrade、disable、revoke、rollback、uninstall | `EXT-22` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-23) |
| 226 | W4 | 专项 | [`EXT-24`](roadmap/skills-plugins-hooks.md#step-ext-24) | Skills / Plugins / Hooks · secret、state 和 migration | `EXT-22`、`EXT-23` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-24) |
| 227 | W4 | 专项 | [`EXT-25`](roadmap/skills-plugins-hooks.md#step-ext-25) | Skills / Plugins / Hooks · 签名 Skill 服务端绑定 | `EXT-07`、`EXT-09`、`EXT-21` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-25) |
| 228 | W4 | 专项 | [`EXT-26`](roadmap/skills-plugins-hooks.md#step-ext-26) | Skills / Plugins / Hooks · Plugin component adapter | `EXT-15`、`EXT-21`、`EXT-22` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-26) |
| 229 | W4 | 基础 | [`P1-J2-01`](#step-p1-j2-01) | P1 基础 · 类型化区段 + provenance | `P0-G-04` | ⏳ | [基础卡](#step-p1-j2-01) |
| 230 | W4 | 基础 | [`P1-J2-02`](#step-p1-j2-02) | P1 基础 · 预算覆盖 tool schemas 与 system prompt | `P1-J2-01` | ⏳ | [基础卡](#step-p1-j2-02) |
| 231 | W4 | 基础 | [`P1-J2-03`](#step-p1-j2-03) | P1 基础 · 角色 prompt 接线 | `P1-J2-01` | ⏳ | [基础卡](#step-p1-j2-03) |
| 232 | W4 | 基础 | [`P1-J2-04`](#step-p1-j2-04) | P1 基础 · 提示词来源与角色包加载 | `P1-J2-03` | ⏳ | [基础卡](#step-p1-j2-04) |
| 233 | W4 | 基础 | [`P1-J3-02`](#step-p1-j3-02) | P1 基础 · 分层检索与密级 | `P1-J3-01` | ⏳ | [基础卡](#step-p1-j3-02) |
| 234 | W4 | 基础 | [`P1-J3-03`](#step-p1-j3-03) | P1 基础 · 抽取建议包与三档准入 | `P1-J3-01`、`P0-F-01` | ⏳ | [基础卡](#step-p1-j3-03) |
| 235 | W4 | 基础 | [`P1-J3-04`](#step-p1-j3-04) | P1 基础 · hybrid 检索基建 | `P1-J3-02` | ⏳ | [基础卡](#step-p1-j3-04) |
| 236 | W4 | 基础 | [`P1-L4-01`](#step-p1-l4-01) | P1 基础 · Code intelligence 快照 | `P0-A-01a` | ⏳ | [基础卡](#step-p1-l4-01) |
| 237 | W4 | 基础 | [`P2-K7-01`](#step-p2-k7-01) | P2 基础 · 数据治理与删除传播 | `P0-A-01a`、`P1-J3-04` | ⏳ | [基础卡](#step-p2-k7-01) |
| 238 | W4 | 基础 | [`P4-J3-05`](#step-p4-j3-05) | P4 基础 · run 蒸馏与 lesson 入库 | `P1-J3-03` | ⏳ | [基础卡](#step-p4-j3-05) |
| 239 | W4 | 基础 | [`P4-L5-01`](#step-p4-l5-01) | P4 基础 · 扩展与技能包 | `P1-H-01` | ⏳ | [基础卡](#step-p4-l5-01) |
| 240 | W4 | 基础 | [`P4-L6-01`](#step-p4-l6-01) | P4 基础 · 供应链 | `P4-L5-01` | ⏳ | [基础卡](#step-p4-l6-01) |
| **W5** | **Provider 协议与调用链** |  |  |  |  |  |  |
| 241 | W5 | 专项 | [`P4-J7-11`](roadmap/provider.md#step-p4-j7-11) | Provider · 类型化角色路由与每 attempt 准入 | `P4-J7-09`、`P4-J7-10`、`P1-C-03`、`P0-K1-01`、`P1-K5-01`、`CP-11`、`CP-13` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-11) |
| 242 | W5 | 专项 | [`P4-J7-12`](roadmap/provider.md#step-p4-j7-12) | Provider · 请求编译、工具映射与上下文完整性 | `P4-J7-06`、`P4-J7-11`、`P1-H-01`、`P1-J2-02`、`P1-J2-04` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-12) |
| 243 | W5 | 专项 | [`P4-J7-13`](roadmap/provider.md#step-p4-j7-13) | Provider · HTTP、SSE、NDJSON 的有界传输 | `P4-J7-07`、`P4-J7-09` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-13) |
| 244 | W5 | 专项 | [`P4-J7-14`](roadmap/provider.md#step-p4-j7-14) | Provider · 唯一 accumulator 与协议终态 | `P4-J7-06`、`P4-J7-13` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-14) |
| 245 | W5 | 专项 | [`P4-J7-15`](roadmap/provider.md#step-p4-j7-15) | Provider · Anthropic Messages 完整收口 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-15) |
| 246 | W5 | 专项 | [`P4-J7-16`](roadmap/provider.md#step-p4-j7-16) | Provider · OpenAI Chat Completions 原生流式 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-16) |
| 247 | W5 | 专项 | [`P4-J7-17`](roadmap/provider.md#step-p4-j7-17) | Provider · OpenAI Responses 原生适配 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-17) |
| 248 | W5 | 专项 | [`P4-J7-18`](roadmap/provider.md#step-p4-j7-18) | Provider · Ollama 原生 NDJSON 与本地模型体验 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-18) |
| 249 | W5 | 专项 | [`P4-J7-19`](roadmap/provider.md#step-p4-j7-19) | Provider · Gemini Interactions 原生协议 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-19) |
| 250 | W5 | 专项 | [`P4-J7-20`](roadmap/provider.md#step-p4-j7-20) | Provider · 推理签名、续接资料与短期保护存储 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-19`、`P2-K7-01`、`CP-18`、`CP-25` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-20) |
| 251 | W5 | 专项 | [`P4-J7-21`](roadmap/provider.md#step-p4-j7-21) | Provider · 结构化输出的请求与验收 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-21) |
| 252 | W5 | 专项 | [`P4-J7-22`](roadmap/provider.md#step-p4-j7-22) | Provider · 图片输入与敏感数据出站准入 | `P4-J7-15`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P2-K7-01`、`P4-J7-16`、`CP-25` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-22) |
| 253 | W5 | 专项 | [`P4-J7-23`](roadmap/provider.md#step-p4-j7-23) | Provider · 单层重试、绝对时限和取消传递 | `P4-J7-11`、`P4-J7-13`、`P4-J7-14`、`P0-J1-04`、`P0-J1-05a`、`P0-J1-05b`、`CP-15` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-23) |
| 254 | W5 | 专项 | [`P4-J7-24`](roadmap/provider.md#step-p4-j7-24) | Provider · Provider 用量、价格快照与预算结算 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P4-J7-23`、`P1-K5-01`、`CP-11`、`CP-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-24) |
| 255 | W5 | 专项 | [`P4-J7-25`](roadmap/provider.md#step-p4-j7-25) | Provider · 容量、熔断与白名单 fallback | `P4-J7-23`、`P4-J7-24` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-25) |
| 256 | W5 | 专项 | [`P4-J7-26`](roadmap/provider.md#step-p4-j7-26) | Provider · 模型事件、脱敏和完整关联链 | `P4-J7-20`、`P4-J7-23`、`P4-J7-24`、`P1-J8-01`、`P0-G-04`、`CP-26` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-26) |
| 257 | W5 | 专项 | [`P4-J7-27`](roadmap/provider.md#step-p4-j7-27) | Provider · 完整轮次恢复与 in-flight 对账 | `P4-J7-20`、`P4-J7-26`、`P0-G-03`、`P0-F-03`、`P2-K6-01`、`CP-18`、`CP-19`、`CP-20` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-27) |
| **W6** | **投影与可用入口** |  |  |  |  |  |  |
| 258 | W6 | 专项 | [`H32`](roadmap/harness.md#step-h32) | Harness · 事实流、展示流与三入口状态一致性 | `H06`、`H13`、`H18`、`H24`、`H26`、`H27` | ⏳ | [专项卡](roadmap/harness.md#step-h32) |
| 259 | W6 | 专项 | [`EXT-27`](roadmap/skills-plugins-hooks.md#step-ext-27) | Skills / Plugins / Hooks · 动态可见性投影 | `EXT-06`、`EXT-25`、`EXT-26` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-27) |
| 260 | W6 | 专项 | [`EXT-28`](roadmap/skills-plugins-hooks.md#step-ext-28) | Skills / Plugins / Hooks · 命令与 UI 合同 | `EXT-22`、`EXT-23`、`EXT-27` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-28) |
| 261 | W6 | 专项 | [`UI-04`](roadmap/ui-entrypoints.md#step-ui-04) | UI / Entrypoints · 动作 CAS、idempotency 与响应丢失 | `UI-01`、`UI-02`、`UI-03` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-04) |
| 262 | W6 | 专项 | [`UI-05`](roadmap/ui-entrypoints.md#step-ui-05) | UI / Entrypoints · 原子 snapshot projector 与分页 | `UI-01`、`UI-02`、`UI-03`、`UI-04` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-05) |
| 263 | W6 | 专项 | [`UI-06`](roadmap/ui-entrypoints.md#step-ui-06) | UI / Entrypoints · feed cursor、gap、replay 与背压 | `UI-05` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-06) |
| 264 | W6 | 专项 | [`UI-07`](roadmap/ui-entrypoints.md#step-ui-07) | UI / Entrypoints · typed client query/feed/action API | `UI-02`、`UI-03`、`UI-04`、`UI-05`、`UI-06` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-07) |
| 265 | W6 | 专项 | [`UI-08`](roadmap/ui-entrypoints.md#step-ui-08) | UI / Entrypoints · 共享 reducer/entity store | `UI-05`、`UI-06`、`UI-07` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-08) |
| 266 | W6 | 专项 | [`UI-09`](roadmap/ui-entrypoints.md#step-ui-09) | UI / Entrypoints · schema 资产、生成和兼容门 | `UI-01`、`UI-07`、`UI-08` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-09) |
| 267 | W6 | 专项 | [`UI-10`](roadmap/ui-entrypoints.md#step-ui-10) | UI / Entrypoints · CLI 命令和输出归一化 | `UI-07`、`UI-08`、`UI-09` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-10) |
| 268 | W6 | 专项 | [`UI-11`](roadmap/ui-entrypoints.md#step-ui-11) | UI / Entrypoints · CLI JSON/TTY/exit code presenter | `UI-02`、`UI-10` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-11) |
| 269 | W6 | 专项 | [`UI-12`](roadmap/ui-entrypoints.md#step-ui-12) | UI / Entrypoints · TTY 输入状态机 | `UI-08`、`UI-10` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-12) |
| 270 | W6 | 专项 | [`UI-13`](roadmap/ui-entrypoints.md#step-ui-13) | UI / Entrypoints · Workbench 时间线与结果渲染 | `UI-06`、`UI-08`、`UI-12` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-13) |
| 271 | W6 | 专项 | [`UI-14`](roadmap/ui-entrypoints.md#step-ui-14) | UI / Entrypoints · Workbench controller 与命令面板 | `UI-07`、`UI-08`、`UI-10`、`UI-11`、`UI-12`、`UI-13` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-14) |
| 272 | W6 | 专项 | [`UI-15`](roadmap/ui-entrypoints.md#step-ui-15) | UI / Entrypoints · Workbench inbox、Diff 和 Receipt | `UI-05`、`UI-08`、`UI-13`、`UI-14` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-15) |
| 273 | W6 | 专项 | [`UI-16`](roadmap/ui-entrypoints.md#step-ui-16) | UI / Entrypoints · Web 路由、来源校验和最小健康信息 | `UI-03`、`UI-07`、`UI-11` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-16) |
| 274 | W6 | 专项 | [`UI-17`](roadmap/ui-entrypoints.md#step-ui-17) | UI / Entrypoints · Web snapshot hydrate、历史和分页 | `UI-05`、`UI-08`、`UI-16` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-17) |
| 275 | W6 | 专项 | [`UI-18`](roadmap/ui-entrypoints.md#step-ui-18) | UI / Entrypoints · Web SSE reconnect、Last-Event-ID 和 gap | `UI-06`、`UI-16`、`UI-17` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-18) |
| 276 | W6 | 专项 | [`UI-19`](roadmap/ui-entrypoints.md#step-ui-19) | UI / Entrypoints · Web session ownership 与多 tab 并发 | `UI-04`、`UI-08`、`UI-16`、`UI-17`、`UI-18` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-19) |
| 277 | W6 | 专项 | [`UI-20`](roadmap/ui-entrypoints.md#step-ui-20) | UI / Entrypoints · Web 时间线组件迁移 | `UI-08`、`UI-13`、`UI-17`、`UI-18` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-20) |
| 278 | W6 | 专项 | [`UI-21`](roadmap/ui-entrypoints.md#step-ui-21) | UI / Entrypoints · Web Human Inbox 与审批动作卡 | `UI-02`、`UI-04`、`UI-05`、`UI-15`、`UI-20` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-21) |
| 279 | W6 | 专项 | [`UI-22`](roadmap/ui-entrypoints.md#step-ui-22) | UI / Entrypoints · Web artifact/diff/receipt detail | `UI-05`、`UI-15`、`UI-20`、`UI-21` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-22) |
| 280 | W6 | 专项 | [`UI-23`](roadmap/ui-entrypoints.md#step-ui-23) | UI / Entrypoints · Web 可访问性、焦点和内容安全 | `UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-23) |
| 281 | W6 | 专项 | [`UI-24`](roadmap/ui-entrypoints.md#step-ui-24) | UI / Entrypoints · Electron IPC sender、导航和新窗口 allowlist | `UI-02`、`UI-03`、`UI-16`、`UI-23` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-24) |
| 282 | W6 | 专项 | [`UI-25`](roadmap/ui-entrypoints.md#step-ui-25) | UI / Entrypoints · Desktop readiness、attach 和 worker 生命周期 | `UI-03`、`UI-24` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-25) |
| 283 | W6 | 专项 | [`UI-26`](roadmap/ui-entrypoints.md#step-ui-26) | UI / Entrypoints · Desktop workspace、托盘、通知与关闭策略 | `UI-08`、`UI-19`、`UI-24`、`UI-25` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-26) |
| 284 | W6 | 专项 | [`UI-27`](roadmap/ui-entrypoints.md#step-ui-27) | UI / Entrypoints · Desktop 安全持久化和 detach | `UI-04`、`UI-25`、`UI-26` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-27) |
| 285 | W6 | 专项 | [`UI-28`](roadmap/ui-entrypoints.md#step-ui-28) | UI / Entrypoints · 共享静态资产、版本和生产打包 | `UI-20`、`UI-23`、`UI-24`、`UI-25`、`UI-26`、`UI-27` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-28) |
| 286 | W6 | 专项 | [`UI-29`](roadmap/ui-entrypoints.md#step-ui-29) | UI / Entrypoints · ACP/IDE session adapter | `UI-01`、`UI-02`、`UI-07`、`UI-18`、`UI-21`、`UI-25` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-29) |
| 287 | W6 | 专项 | [`UI-30`](roadmap/ui-entrypoints.md#step-ui-30) | UI / Entrypoints · IDE editor/terminal capability boundary | `UI-04`、`UI-07`、`UI-29` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-30) |
| 288 | W6 | 基础 | [`P0-M1-01`](#step-p0-m1-01) | P0 基础 · Workbench 基线 | — | ⏳ | [基础卡](#step-p0-m1-01) |
| 289 | W6 | 基础 | [`P2-K3-01`](#step-p2-k3-01) | P2 基础 · Human Inbox | `P0-F-02` | ⏳ | [基础卡](#step-p2-k3-01) |
| 290 | W6 | 基础 | [`P2-M2-01`](#step-p2-m2-01) | P2 基础 · UI 投影合同 | `P0-M1-01` | ⏳ | [基础卡](#step-p2-m2-01) |
| 291 | W6 | 基础 | [`P2-M3-01`](#step-p2-m3-01) | P2 基础 · 人工动作卡 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m3-01) |
| 292 | W6 | 基础 | [`P2-M4-01`](#step-p2-m4-01) | P2 基础 · Run/Artifact 详情 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m4-01) |
| 293 | W6 | 基础 | [`P2-M5-01`](#step-p2-m5-01) | P2 基础 · Web 快照水合与重连 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m5-01) |
| 294 | W6 | 专项 | [`P4-J7-28`](roadmap/provider.md#step-p4-j7-28) | Provider · 模型选择、诊断与事件投影 | `P4-J7-10`、`P4-J7-11`、`P4-J7-25`、`P4-J7-26`、`P4-J7-02`、`P4-J7-03`、`P2-M2-01`、`P2-M5-01`、`CP-22` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-28) |
| 295 | W6 | 基础 | [`P2-M7-01`](#step-p2-m7-01) | P2 基础 · 无障碍回退 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m7-01) |
| 296 | W6 | 基础 | [`P4-M6-01`](#step-p4-m6-01) | P4 基础 · Desktop 壳 | `P2-M2-01` | ⏳ | [基础卡](#step-p4-m6-01) |
| **W7** | **CompanyOS 业务闭环** |  |  |  |  |  |  |
| 297 | W7 | 专项 | [`CP-23`](roadmap/control-plane.md#step-cp-23) | ControlPlane · Company 命令也使用控制面事务 | `CP-04`、`CP-07`、`CP-08`、`CP-13` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-23) |
| 298 | W7 | 专项 | [`CP-24`](roadmap/control-plane.md#step-cp-24) | ControlPlane · 调度、WorkPacket、委派与 Workflow | `CP-11`、`CP-12`、`CP-17`、`CP-23` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-24) |
| 299 | W7 | 专项 | [`ER-28`](roadmap/event-receipt-recovery.md#step-er-28) | Event / Receipt / Recovery · CompanyOS / Workflow / Artifact 业务引用 | `ER-27` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-28) |
| 300 | W7 | 专项 | [`CO-09`](roadmap/companyos.md#step-co-09) | CompanyOS · Objective 与 Initiative 受理和取舍 | `CO-08` | ⏳ | [专项卡](roadmap/companyos.md#step-co-09) |
| 301 | W7 | 专项 | [`CO-10`](roadmap/companyos.md#step-co-10) | CompanyOS · Charter 与项目 go/no-go | `CO-09`、`CO-06` | ⏳ | [专项卡](roadmap/companyos.md#step-co-10) |
| 302 | W7 | 专项 | [`CO-11`](roadmap/companyos.md#step-co-11) | CompanyOS · 建立标准覆盖图，替换文本集合推断 | `CO-06`、`CO-10` | ⏳ | [专项卡](roadmap/companyos.md#step-co-11) |
| 303 | W7 | 专项 | [`CO-12`](roadmap/companyos.md#step-co-12) | CompanyOS · Milestone、Plan 与依赖图提案 | `CO-10`、`CO-11` | ⏳ | [专项卡](roadmap/companyos.md#step-co-12) |
| 304 | W7 | 专项 | [`CO-13`](roadmap/companyos.md#step-co-13) | CompanyOS · WorkPacket 扩为通用部门工作合同 | `CO-04`、`CO-06`、`CO-12` | ⏳ | [专项卡](roadmap/companyos.md#step-co-13) |
| 305 | W7 | 专项 | [`CO-14`](roadmap/companyos.md#step-co-14) | CompanyOS · 会议、黑板和决议对象补齐 | `CO-04`、`CO-06`、`CO-13` | ⏳ | [专项卡](roadmap/companyos.md#step-co-14) |
| 306 | W7 | 专项 | [`CO-15`](roadmap/companyos.md#step-co-15) | CompanyOS · 用现有 Harness 驱动有界角色会议 | `CO-14` | ⏳ | [专项卡](roadmap/companyos.md#step-co-15) |
| 307 | W7 | 专项 | [`CO-16`](roadmap/companyos.md#step-co-16) | CompanyOS · 角色提案进入业务命令，原子批准计划 | `CO-07`、`CO-12`、`CO-13`、`CO-15` | ⏳ | [专项卡](roadmap/companyos.md#step-co-16) |
| 308 | W7 | 专项 | [`CO-17`](roadmap/companyos.md#step-co-17) | CompanyOS · 跨部门交接与 ACK 责任转移 | `CO-13`、`CO-16` | ⏳ | [专项卡](roadmap/companyos.md#step-co-17) |
| 309 | W7 | 专项 | [`CO-18`](roadmap/companyos.md#step-co-18) | CompanyOS · 唯一 ready 谓词与可解释阻塞原因 | `CO-08`、`CO-12`、`CO-17` | ⏳ | [专项卡](roadmap/companyos.md#step-co-18) |
| 310 | W7 | 专项 | [`CO-19`](roadmap/companyos.md#step-co-19) | CompanyOS · 原子 claim、租约 fencing 与执行尝试 | `CO-07`、`CO-18` | ⏳ | [专项卡](roadmap/companyos.md#step-co-19) |
| 311 | W7 | 专项 | [`CO-20`](roadmap/companyos.md#step-co-20) | CompanyOS · Cell 资源预留、提交与回收闭环 | `CO-03`、`CO-19` | ⏳ | [专项卡](roadmap/companyos.md#step-co-20) |
| 312 | W7 | 专项 | [`CO-21`](roadmap/companyos.md#step-co-21) | CompanyOS · 角色任务接入 fresh Run 与明确 Company scope | `CO-04`、`CO-13`、`CO-20` | ⏳ | [专项卡](roadmap/companyos.md#step-co-21) |
| 313 | W7 | 专项 | [`CO-22`](roadmap/companyos.md#step-co-22) | CompanyOS · 确定性 Company ProcessManager | `CO-08`、`CO-16`、`CO-17`、`CO-21` | ⏳ | [专项卡](roadmap/companyos.md#step-co-22) |
| 314 | W7 | 专项 | [`CO-23`](roadmap/companyos.md#step-co-23) | CompanyOS · 持久唤醒队列与意图消费 | `CO-07`、`CO-19`、`CO-22` | ⏳ | [专项卡](roadmap/companyos.md#step-co-23) |
| 315 | W7 | 专项 | [`CO-24`](roadmap/companyos.md#step-co-24) | CompanyOS · 执行结果归集为不可变 EvidenceBundle | `CO-06`、`CO-21`、`CO-23` | ⏳ | [专项卡](roadmap/companyos.md#step-co-24) |
| 316 | W7 | 专项 | [`CO-25`](roadmap/companyos.md#step-co-25) | CompanyOS · 独立 Reviewer 与逐条证据结论 | `CO-03`、`CO-11`、`CO-24` | ⏳ | [专项卡](roadmap/companyos.md#step-co-25) |
| 317 | W7 | 专项 | [`CO-26`](roadmap/companyos.md#step-co-26) | CompanyOS · Packet 级验收与输出接收 | `CO-24`、`CO-25` | ⏳ | [专项卡](roadmap/companyos.md#step-co-26) |
| 318 | W7 | 专项 | [`CO-27`](roadmap/companyos.md#step-co-27) | CompanyOS · Milestone 独立验收，消除阶段依赖等待环 | `CO-12`、`CO-18`、`CO-26` | ⏳ | [专项卡](roadmap/companyos.md#step-co-27) |
| 319 | W7 | 专项 | [`CO-28`](roadmap/companyos.md#step-co-28) | CompanyOS · Project 验收、拒绝与显式豁免 | `CO-05`、`CO-25`、`CO-27` | ⏳ | [专项卡](roadmap/companyos.md#step-co-28) |
| 320 | W7 | 专项 | [`CO-29`](roadmap/companyos.md#step-co-29) | CompanyOS · 有界返工与 successor/attempt 历史 | `CO-19`、`CO-26`、`CO-27`、`CO-28` | ⏳ | [专项卡](roadmap/companyos.md#step-co-29) |
| 321 | W7 | 专项 | [`CO-30`](roadmap/companyos.md#step-co-30) | CompanyOS · ChangeRequest 实际应用到整组基线 | `CO-07`、`CO-11`、`CO-16`、`CO-28`、`CO-29` | ⏳ | [专项卡](roadmap/companyos.md#step-co-30) |
| 322 | W7 | 专项 | [`CO-31`](roadmap/companyos.md#step-co-31) | CompanyOS · 项目暂停、恢复、取消与部门传播 | `CO-03`、`CO-19`、`CO-23`、`CO-30` | ⏳ | [专项卡](roadmap/companyos.md#step-co-31) |
| 323 | W7 | 专项 | [`CO-32`](roadmap/companyos.md#step-co-32) | CompanyOS · 风险、事故、Unknown 与对账工作流 | `CO-24`、`CO-30`、`CO-31` | ⏳ | [专项卡](roadmap/companyos.md#step-co-32) |
| 324 | W7 | 专项 | [`CO-33`](roadmap/companyos.md#step-co-33) | CompanyOS · 版本化 DeliveryManifest 与本地交付包 | `CO-06`、`CO-28`、`CO-32` | ⏳ | [专项卡](roadmap/companyos.md#step-co-33) |
| 325 | W7 | 专项 | [`CO-34`](roadmap/companyos.md#step-co-34) | CompanyOS · 交付授权、效果记录与接收确认 | `CO-05`、`CO-07`、`CO-32`、`CO-33` | ⏳ | [专项卡](roadmap/companyos.md#step-co-34) |
| 326 | W7 | 专项 | [`CO-35`](roadmap/companyos.md#step-co-35) | CompanyOS · 成功、失败、取消和豁免的 ClosingReceipt | `CO-28`、`CO-31`、`CO-32`、`CO-34` | ⏳ | [专项卡](roadmap/companyos.md#step-co-35) |
| 327 | W7 | 专项 | [`CO-36`](roadmap/companyos.md#step-co-36) | CompanyOS · Outcome 测量与目标实现判定 | `CO-09`、`CO-10`、`CO-35` | ⏳ | [专项卡](roadmap/companyos.md#step-co-36) |
| 328 | W7 | 专项 | [`CO-37`](roadmap/companyos.md#step-co-37) | CompanyOS · 部门决议、收尾经验与 Memory 候选晋升 | `CO-15`、`CO-35` | ⏳ | [专项卡](roadmap/companyos.md#step-co-37) |
| 329 | W7 | 专项 | [`CO-38`](roadmap/companyos.md#step-co-38) | CompanyOS · 组织与项目的可重建读模型 | `CO-18`、`CO-22`、`CO-28`、`CO-35`、`CO-36` | ⏳ | [专项卡](roadmap/companyos.md#step-co-38) |
| 330 | W7 | 专项 | [`CO-39`](roadmap/companyos.md#step-co-39) | CompanyOS · 统一 Human Inbox 与有后续动作的决定卡 | `CO-05`、`CO-23`、`CO-28`、`CO-34`、`CO-38` | ⏳ | [专项卡](roadmap/companyos.md#step-co-39) |
| 331 | W7 | 专项 | [`CO-40`](roadmap/companyos.md#step-co-40) | CompanyOS · CLI 与 Workbench 的 Company 用户流程 | `CO-38`、`CO-39` | ⏳ | [专项卡](roadmap/companyos.md#step-co-40) |
| 332 | W7 | 专项 | [`CO-41`](roadmap/companyos.md#step-co-41) | CompanyOS · Web 与 Desktop 复用同一 Company 状态 | `CO-38`、`CO-39`、`CO-40` | ⏳ | [专项卡](roadmap/companyos.md#step-co-41) |
| 333 | W7 | 基础 | [`P1-E-02`](#step-p1-e-02) | P1 基础 · Symposium 会议对象契约化 | `P1-E-01` | ⏳ | [基础卡](#step-p1-e-02) |
| 334 | W7 | 基础 | [`P2-J5-01`](#step-p2-j5-01) | P2 基础 · Workflow definition 与重放 | `P0-G-04` | ⏳ | [基础卡](#step-p2-j5-01) |
| 335 | W7 | 基础 | [`P3-I-03`](#step-p3-i-03) | P3 基础 · 全链重建 | `P3-I-02`、`P0-G-04` | ⏳ | [基础卡](#step-p3-i-03) |
| 336 | W7 | 基础 | [`P3-I-04`](#step-p3-i-04) | P3 基础 · Acceptance 快照与独立 Review | `P3-I-02` | ⏳ | [基础卡](#step-p3-i-04) |
| 337 | W7 | 基础 | [`P3-I-05`](#step-p3-i-05) | P3 基础 · Delivery / ClosingReceipt / Outcome | `P3-I-03` | ⏳ | [基础卡](#step-p3-i-05) |
| 338 | W7 | 基础 | [`P4-E-03`](#step-p4-e-03) | P4 基础 · 五部门开会与决议入部门 RAG | `P1-E-02`、`P1-J3-02`、`P1-J3-03` | ⏳ | [基础卡](#step-p4-e-03) |
| **W8** | **并行、治理与复用** |  |  |  |  |  |  |
| 339 | W8 | 专项 | [`ER-29`](roadmap/event-receipt-recovery.md#step-er-29) | Event / Receipt / Recovery · Data governance、retention 和 deletion propagation | `ER-28` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-29) |
| 340 | W8 | 专项 | [`ER-30`](roadmap/event-receipt-recovery.md#step-er-30) | Event / Receipt / Recovery · Health、metrics、trace correlation and operator evidence | `ER-29` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-30) |
| 341 | W8 | 专项 | [`H31`](roadmap/harness.md#step-h31) | Harness · 受控子 Agent 的 Harness 接缝 | `H07`、`H09`、`H13`、`H24`、`H27`、`H30` | ⏳ | [专项卡](roadmap/harness.md#step-h31) |
| 342 | W8 | 专项 | [`H33`](roadmap/harness.md#step-h33) | Harness · 运行资源、关闭和异常退出的完整清理 | `H08`、`H17`、`H24`、`H31`、`H32` | ⏳ | [专项卡](roadmap/harness.md#step-h33) |
| 343 | W8 | 专项 | [`H34`](roadmap/harness.md#step-h34) | Harness · 旧协议、cassette 与入口迁移 | `H02`、`H04`、`H19`、`H24`、`H27`、`H32`、`H33` | ⏳ | [专项卡](roadmap/harness.md#step-h34) |
| 344 | W8 | 专项 | [`CO-42`](roadmap/companyos.md#step-co-42) | CompanyOS · 全业务链跨进程恢复与 schema 升级演练 | `CO-08`、`CO-23`、`CO-29`、`CO-30`、`CO-32`、`CO-35`、`CO-39` | ⏳ | [专项卡](roadmap/companyos.md#step-co-42) |
| 345 | W8 | 专项 | [`CO-43`](roadmap/companyos.md#step-co-43) | CompanyOS · 有界多角色/多 Builder 并行 | `CO-18`、`CO-19`、`CO-20`、`CO-21`、`CO-29`、`CO-42` | ⏳ | [专项卡](roadmap/companyos.md#step-co-43) |
| 346 | W8 | 专项 | [`CO-44`](roadmap/companyos.md#step-co-44) | CompanyOS · Integrator、冲突处理与 MergeReceipt | `CO-25`、`CO-26`、`CO-43` | ⏳ | [专项卡](roadmap/companyos.md#step-co-44) |
| 347 | W8 | 专项 | [`CO-45`](roadmap/companyos.md#step-co-45) | CompanyOS · 多项目优先级、容量与组织成本账 | `CO-02`、`CO-20`、`CO-23`、`CO-36`、`CO-38`、`CO-43` | ⏳ | [专项卡](roadmap/companyos.md#step-co-45) |
| 348 | W8 | 专项 | [`CO-46`](roadmap/companyos.md#step-co-46) | CompanyOS · 版本化流程模板、组织配置升级与第二种业务样例 | `CO-04`、`CO-13`、`CO-22`、`CO-37`、`CO-42`、`CO-45` | ⏳ | [专项卡](roadmap/companyos.md#step-co-46) |
| 349 | W8 | 基础 | [`P4-J6-01`](#step-p4-j6-01) | P4 基础 · 有界 Swarm | `P1-C-02` | ⏳ | [基础卡](#step-p4-j6-01) |
| 350 | W8 | 基础 | [`P4-K2-01`](#step-p4-k2-01) | P4 基础 · 触发器与调度 | `P0-B-01` | ⏳ | [基础卡](#step-p4-k2-01) |
| 351 | W8 | 基础 | [`P4-K8-01`](#step-p4-k8-01) | P4 基础 · Connector | `P0-A-01a` | ⏳ | [基础卡](#step-p4-k8-01) |
| **W9** | **离线联合验收** |  |  |  |  |  |  |
| 352 | W9 | 专项 | [`CP-29`](roadmap/control-plane.md#step-cp-29) | ControlPlane · 状态机性质、并发与崩溃验收 | `CP-05`、`CP-07`、`CP-13`、`CP-14`、`CP-15`、`CP-16`、`CP-17`、`CP-18`、`CP-19`、`CP-20`、`CP-23`、`CP-24`、`CP-25`、`CP-26`、`CP-27`、`CP-28` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-29) |
| 353 | W9 | 专项 | [`CP-30`](roadmap/control-plane.md#step-cp-30) | ControlPlane · 产品流程和证据收口 | `CP-21`、`CP-22`、`CP-23`、`CP-24`、`CP-25`、`CP-26`、`CP-27`、`CP-28`、`CP-29` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-30) |
| 354 | W9 | 专项 | [`ER-31`](roadmap/event-receipt-recovery.md#step-er-31) | Event / Receipt / Recovery · Crash-point and fault-injection matrix | `ER-30` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-31) |
| 355 | W9 | 专项 | [`ER-32`](roadmap/event-receipt-recovery.md#step-er-32) | Event / Receipt / Recovery · Property/conformance tests for adapters | `ER-31` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-32) |
| 356 | W9 | 专项 | [`ER-33`](roadmap/event-receipt-recovery.md#step-er-33) | Event / Receipt / Recovery · Performance、容量和迁移演练 | `ER-32` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-33) |
| 357 | W9 | 专项 | [`ER-34`](roadmap/event-receipt-recovery.md#step-er-34) | Event / Receipt / Recovery · Local durable gate | `ER-33` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-34) |
| 358 | W9 | 专项 | [`ER-35`](roadmap/event-receipt-recovery.md#step-er-35) | Event / Receipt / Recovery · Cross-entry and CompanyOS gate | `ER-34` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-35) |
| 359 | W9 | 专项 | [`CAP-26`](roadmap/capability.md#step-cap-26) | Capability · 本地五工具 + Hook 的完整执行闭环验收 | `CAP-14`、`CAP-15`、`CAP-16`、`CAP-17`、`CAP-18`、`CAP-19`、`CAP-20`、`CAP-21`、`CAP-22`、`CAP-23`、`CAP-24`、`CAP-25` | ⏳ | [专项卡](roadmap/capability.md#step-cap-26) |
| 360 | W9 | 专项 | [`H35`](roadmap/harness.md#step-h35) | Harness · Harness 轨迹评测与性能验证 | `H25`、`H27`、`H28`、`H30`、`H34` | ⏳ | [专项卡](roadmap/harness.md#step-h35) |
| 361 | W9 | 专项 | [`P4-J7-29`](roadmap/provider.md#step-p4-j7-29) | Provider · 离线合同矩阵、属性测试与故障语料 | `P4-J7-20`、`P4-J7-21`、`P4-J7-22`、`P4-J7-25`、`P4-J7-27` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-29) |
| 362 | W9 | 专项 | [`P4-J7-30`](roadmap/provider.md#step-p4-j7-30) | Provider · 产品链和四表面回归 | `P4-J7-28`、`P4-J7-29`、`P0-M1-01` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-30) |
| 363 | W9 | 专项 | [`CM-30`](roadmap/context-memory.md#step-cm-30) | Context / Memory · Golden ContextPlan / retrieval fixture | `CM-29` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-30) |
| 364 | W9 | 专项 | [`CM-31`](roadmap/context-memory.md#step-cm-31) | Context / Memory · Retrieval quality and safety evaluation | `CM-30` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-31) |
| 365 | W9 | 专项 | [`CM-32`](roadmap/context-memory.md#step-cm-32) | Context / Memory · Context/Memory Inspector 与用户纠正 | `CM-31` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-32) |
| 366 | W9 | 专项 | [`CM-33`](roadmap/context-memory.md#step-cm-33) | Context / Memory · Code graph / temporal fact 后置扩展 | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-33) |
| 367 | W9 | 专项 | [`CM-34`](roadmap/context-memory.md#step-cm-34) | Context / Memory · Local embedding package and model rotation | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-34) |
| 368 | W9 | 专项 | [`CM-35`](roadmap/context-memory.md#step-cm-35) | Context / Memory · External context/resource adapter | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-35) |
| 369 | W9 | 专项 | [`CM-36`](roadmap/context-memory.md#step-cm-36) | Context / Memory · User memory workbench and bulk operations | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-36) |
| 370 | W9 | 专项 | [`CM-37`](roadmap/context-memory.md#step-cm-37) | Context / Memory · Cache, index and retention maintenance | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-37) |
| 371 | W9 | 专项 | [`EXT-29`](roadmap/skills-plugins-hooks.md#step-ext-29) | Skills / Plugins / Hooks · 本地 fake golden | `EXT-10`、`EXT-18`、`EXT-25`、`EXT-28` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-29) |
| 372 | W9 | 专项 | [`EXT-30`](roadmap/skills-plugins-hooks.md#step-ext-30) | Skills / Plugins / Hooks · Durable、故障注入和恢复 | `EXT-18`、`EXT-23`、`EXT-24`、`EXT-29` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-30) |
| 373 | W9 | 专项 | [`EXT-31`](roadmap/skills-plugins-hooks.md#step-ext-31) | Skills / Plugins / Hooks · 性能、供应链回归和文档收口 | `EXT-29`、`EXT-30` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-31) |
| 374 | W9 | 专项 | [`UI-31`](roadmap/ui-entrypoints.md#step-ui-31) | UI / Entrypoints · CLI/Workbench/Web/Desktop 行为 parity | `UI-10`、`UI-11`、`UI-12`、`UI-13`、`UI-14`、`UI-15`、`UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22`、`UI-23`、`UI-24`、`UI-25`、`UI-26`、`UI-27`、`UI-28`、`UI-29`、`UI-30` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-31) |
| 375 | W9 | 专项 | [`UI-32`](roadmap/ui-entrypoints.md#step-ui-32) | UI / Entrypoints · deny-first 安全路径集成测试 | `UI-04`、`UI-16`、`UI-19`、`UI-21`、`UI-23`、`UI-24`、`UI-30`、`UI-31` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-32) |
| 376 | W9 | 专项 | [`UI-33`](roadmap/ui-entrypoints.md#step-ui-33) | UI / Entrypoints · reconnect/replay/gap/crash recovery e2e | `UI-05`、`UI-06`、`UI-07`、`UI-08`、`UI-18`、`UI-25` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-33) |
| 377 | W9 | 专项 | [`UI-34`](roadmap/ui-entrypoints.md#step-ui-34) | UI / Entrypoints · 性能、资源上限和可访问性验收 | `UI-13`、`UI-20`、`UI-23`、`UI-28`、`UI-33` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-34) |
| 378 | W9 | 专项 | [`UI-35`](roadmap/ui-entrypoints.md#step-ui-35) | UI / Entrypoints · 旧 Web/CLI 迁移与兼容收口 | `UI-10`、`UI-11`、`UI-12`、`UI-13`、`UI-14`、`UI-15`、`UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22`、`UI-23`、`UI-31` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-35) |
| 379 | W9 | 专项 | [`UI-36`](roadmap/ui-entrypoints.md#step-ui-36) | UI / Entrypoints · 生产构建、安装和发布前 smoke | `UI-28`、`UI-34`、`UI-35` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-36) |
| 380 | W9 | 专项 | [`UI-37`](roadmap/ui-entrypoints.md#step-ui-37) | UI / Entrypoints · 用户文档、模块图和操作 runbook | `UI-31`、`UI-32`、`UI-33`、`UI-34`、`UI-35`、`UI-36` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-37) |
| 381 | W9 | 专项 | [`UI-38`](roadmap/ui-entrypoints.md#step-ui-38) | UI / Entrypoints · 协议/入口 conformance 集成门 | `UI-01`、`UI-02`、`UI-03`、`UI-04`、`UI-05`、`UI-06`、`UI-07`、`UI-08`、`UI-09`、`UI-10`、`UI-11`、`UI-12`、`UI-13`、`UI-14`、`UI-15`、`UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22`、`UI-23`、`UI-24`、`UI-25`、`UI-26`、`UI-27`、`UI-28`、`UI-29`、`UI-30`、`UI-31`、`UI-32`、`UI-33`、`UI-34`、`UI-35`、`UI-36`、`UI-37` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-38) |
| 382 | W9 | 专项 | [`CO-47`](roadmap/companyos.md#step-co-47) | CompanyOS · fake-model Company 黄金闭环与故障矩阵 | `CO-35`、`CO-36`、`CO-37`、`CO-40`、`CO-41`、`CO-42`、`CO-44`、`CO-46` | ⏳ | [专项卡](roadmap/companyos.md#step-co-47) |
| 383 | W9 | 基础 | [`P1-L1-01`](#step-p1-l1-01) | P1 基础 · EvalSuite 与 GoldenTrace | `P0-G-04` | ⏳ | [基础卡](#step-p1-l1-01) |
| 384 | W9 | 基础 | [`P2-L2-01`](#step-p2-l2-01) | P2 基础 · 反馈与候选改进 | `P1-L1-01` | ⏳ | [基础卡](#step-p2-l2-01) |
| 385 | W9 | 基础 | [`P3-I-06`](#step-p3-i-06) | P3 基础 · fake-model coding 黄金闭环 | `P3-I-05` | ⏳ | [基础卡](#step-p3-i-06) |
| 386 | W9 | 基础 | [`P4-L3-01`](#step-p4-l3-01) | P4 基础 · 版本治理与 drift | `P1-L1-01` | ⏳ | [基础卡](#step-p4-l3-01) |
| **W10** | **后置平台与能力扩展** |  |  |  |  |  |  |
| 387 | W10 | 专项 | [`CAP-27`](roadmap/capability.md#step-cap-27) | Capability · 长任务、process handle 与 PTY | `CAP-12`、`CAP-13`、`CAP-17`、`CAP-23`、`CAP-25`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-27) |
| 388 | W10 | 专项 | [`CAP-28`](roadmap/capability.md#step-cap-28) | Capability · 受控 egress 与最小凭据注入 | `CAP-03`、`CAP-06`、`CAP-11`、`CAP-17`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-28) |
| 389 | W10 | 专项 | [`CAP-29`](roadmap/capability.md#step-cap-29) | Capability · Streamable HTTP MCP | `CAP-22`、`CAP-24`、`CAP-28` | ⏳ | [专项卡](roadmap/capability.md#step-cap-29) |
| 390 | W10 | 专项 | [`CAP-30`](roadmap/capability.md#step-cap-30) | Capability · 动态工具搜索与受控扩展准入 | `CAP-01`、`CAP-02`、`CAP-05`、`CAP-22`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-30) |
| 391 | W10 | 专项 | [`CAP-31`](roadmap/capability.md#step-cap-31) | Capability · macOS 原生后端 | `CAP-07`、`CAP-08`、`CAP-10`、`CAP-12`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-31) |
| 392 | W10 | 专项 | [`CAP-32`](roadmap/capability.md#step-cap-32) | Capability · Windows 原生后端 | `CAP-07`、`CAP-08`、`CAP-10`、`CAP-12`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-32) |
| 393 | W10 | 专项 | [`CAP-33`](roadmap/capability.md#step-cap-33) | Capability · Container / gVisor 可选执行环境 | `CAP-07`、`CAP-10`、`CAP-12`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-33) |
| **W11** | **真实环境与发布证据** |  |  |  |  |  |  |
| 394 | W11 | 专项 | [`ER-36`](roadmap/event-receipt-recovery.md#step-er-36) | Event / Receipt / Recovery · Physical/live boundary and handoff | `ER-35` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-36) |
| 395 | W11 | 专项 | [`CAP-34`](roadmap/capability.md#step-cap-34) | Capability · 扩展组合验收与证据收口 | `CAP-27`、`CAP-28`、`CAP-29`、`CAP-30`、`CAP-31`、`CAP-32`、`CAP-33` | ⏳ | [专项卡](roadmap/capability.md#step-cap-34) |
| 396 | W11 | 专项 | [`H36`](roadmap/harness.md#step-h36) | Harness · 三入口集成、真实 Provider 和证据收口 | `H35` | ⏳ | [专项卡](roadmap/harness.md#step-h36) |
| 397 | W11 | 专项 | [`P4-J7-31`](roadmap/provider.md#step-p4-j7-31) | Provider · 逐协议、逐连接 live 验收与迁移收口 | `P4-J7-30` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-31) |
| 398 | W11 | 专项 | [`CM-38`](roadmap/context-memory.md#step-cm-38) | Context / Memory · End-to-end fake Provider / live opt-in evidence | `CM-33`、`CM-34`、`CM-35`、`CM-36`、`CM-37` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-38) |
| 399 | W11 | 专项 | [`CM-39`](roadmap/context-memory.md#step-cm-39) | Context / Memory · 文档、状态和交接收口 | `CM-38` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-39) |
| 400 | W11 | 专项 | [`UI-39`](roadmap/ui-entrypoints.md#step-ui-39) | UI / Entrypoints · live ACP/IDE opt-in 验证 | `UI-29`、`UI-30`、`UI-33`、`UI-38` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-39) |
| 401 | W11 | 专项 | [`UI-40`](roadmap/ui-entrypoints.md#step-ui-40) | UI / Entrypoints · 发布门与证据收口 | `UI-32`、`UI-33`、`UI-34`、`UI-35`、`UI-36`、`UI-37`、`UI-38`、`UI-39` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-40) |
| 402 | W11 | 专项 | [`UI-41`](roadmap/ui-entrypoints.md#step-ui-41) | UI / Entrypoints · 交接、审查和后续缺口 | `UI-40` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-41) |
| 403 | W11 | 专项 | [`CO-48`](roadmap/companyos.md#step-co-48) | CompanyOS · 真实模型闭环验证、文档回填与交接 | `CO-47` | ⏳ | [专项卡](roadmap/companyos.md#step-co-48) |

---

## 2. 当前窗口

| 顺序 | 单元 | 已有证据 / 缺口 | 开始或退出条件 |
|---|---|---|---|
| 核对基线 | 源码快照与 WIP | `db77c24` 是本次源码审查基线；工作树另有恢复、取消、记忆、提示词等 WIP，文档已按用户指示直接回填 | 测试前记录相关文件快照与 WIP 清单；逐项核验新增实现，不批量套用历史完成态 |
| 当前 1 | `P0-J1-05a` 回归 | 历史 wall-time 证据保留；`db77c24` 的 `local_with_model_config` 丢失 wall-time 配置与非法值拒绝，本次观察 WIP 仍使用该 helper | 先证明该构造路径的有效预算/非法值负向断言，再最小修复、回归 |
| 当前 2 | `P0-J1-05b` 接线 | `db77c24` 的 harness 丢弃命令限额；未提交 WIP 已补 core 角色选择与 per-run 接线，真实产品链尚待验收 | 先做下表中的真实链路负向验收；核验角色/环境/构造上限的组合，保住 `05a` |
| 当前之后 | `P1-J3-01` | 已有实施计划 `superpowers/plans/2026-09-10-memory-j3-01-j3-02.md`；共享 WIP 不能视为已验收 | 保留记忆线原顺序；`05b` 收口后单独推进候选写入，先覆盖伪造来源、自批、ACL 与旧记录降级 |
| 恢复线重开 | `P0-G-04` | 历史证据只覆盖 Run 只读投影；WIP 已新增 Invocation 折叠与恢复代码，产品消费、未决集合及缓存替换尚待证明 | 保留完整退出条件；依赖此单元的条目不得因历史 Run 测试通过而视为已满足依赖 |

**当前切片的验收缺口（以下名称为待补/待加强，不代表已有测试通过）**

| 顺序 | 测试名 | 必须观察到的断言 |
|---|---|---|
| 先拒绝 | `model_config_rejects_invalid_wall_time_budget` | 经实际 model-config 构造路径输入零值、非数字配置，创建失败；不请求模型 |
| 先拒绝 | `model_config_wall_time_budget_fails_closed` | 有效 wall-time 预算耗尽后返回 `run_budget_exceeded:wall_time`，无后续模型调用或完成态 |
| 先拒绝 | `role_max_steps_reaches_the_harness` | 从同一 DaemonHost 发起两种不同角色的 run，超各自上限均失败；断言模型实际调用次数与 Receipt，不能仅调用配置 helper |
| 先拒绝 | `start_command_max_steps_limits_that_run`（加强现有） | harness 构造限额与 Start 命令限额刻意不同；命令限额真正生效，不靠 `.with_max_steps(3)` 代替接线证明 |
| 再成功 / 回归 | `environment_max_steps_overrides_role_in_product_run` | 用 fake model 跑完整产品链，在覆盖限额内完成、越界失败；配置非法时 fail-closed |
| 再成功 / 回归 | `role_step_limits_are_isolated_across_runs_and_continue` | 同 host 不同 run 的预算互不串扰；Continue 沿已有每 turn 重置语义且保留该 run 的角色限额 |

验证顺序：聚焦失败复现 → runner/core/daemon 回归（daemon/control-plane 串行）→ workspace check/fmt/clippy → 全量 release gate。每轮只修当前切片；记录精确命令、命中测试数、退出码、源码快照与限制。

> 本文已登记的历史全绿：CI `34500579350`（源码 `3a319be`），只覆盖当时快照；本次没有查询远端 CI，不声称它是当前最新或覆盖 `db77c24`。
> 本轮共享 WIP 上的 `cargo check --workspace --locked --offline` 曾返回 101；检查期间文件继续变化，该结果仅作阻塞观察，不能归因于稳定 HEAD，也不能作为其他写者后续修复的验证。

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
| 2026-09-10 | 从账本折叠 model-visible history（不新增事件 kind）+ 预派发失败补 `capability_request_id` + ID 契约注册表；CI 修复 chrome apt 源抖动；证据块「Model-visible history rebuild and pre-dispatch pairing evidence (2026-09-10)」 | `41bb971` + `9d255d1` + `d3f7601` + `5f9ce29` → CI `34386563396` ✅ |
| 2026-09-10 | 补入产品特有单元 5 个（`P1-C-03`/`P1-E-02`/`P1-J2-04`/`P1-J3-02`/`P4-E-03`），来源 `COMPANY.md` §3/§4/§5/§7；同时修正 `P1-J2-03` 现状表述（builder prompt 已部分接线） | `7aa619f` |
| 2026-09-10 | 新进程凭事件重建 Run 状态（projection，矛盾终态 fail-closed）；证据块「Run state event projection evidence (2026-09-10)」 | `3a319be` + `c5de094` → CI `34500579350` ✅ |
| 2026-09-10 | `P0-J1-05a` 收口：wall-time 预算接线进产品路径（`KIANA_HARNESS_WALL_TIME_MS` / `KIANA_HARNESS_MAX_STEPS`，默认不变）；直跑 `34381844056` 红于 CI 环境 apt 问题（`c83a357`/`5f9ce29` 修复后覆盖跑绿）；证据块「Run-level wall-time budget evidence (2026-09-10)」 | `409cfc7` + `747ff8b` → 覆盖 CI `34389804309` ✅ |
| 2026-09-10 | 记忆架构设计 spec + J3-01/J3-02 实施计划入库；roadmap 新增 `P1-J3-03`/`P1-J3-04`/`P4-J3-05` | `0bb624e` + `28fe392` + `a1fb227` |
| 2026-09-12 | 按 `db77c24` 核对当前窗口：`05b` 已有提交但真实链路未证明；重开 `05a` 的 wall-time 回归和 `G-04` 未交付范围，补齐审批/记忆依赖；历史证据不删除 | 文档修订未提交；证据块「Roadmap source reconciliation evidence (2026-09-12)」；无新增 CI |

---

## 4. P0 — 事实基线、Identity、Runtime ledger、Event/Receipt







<a id="step-p0-a-01a"></a>

### P0-A-01a ID 契约注册表　✅

- **现状**：`kiana-domain/src/contracts.rs` 已登记全部 20 个 ID 类型（18 个 UUID 型 + `SessionId` + `WorkFingerprint`）；CI `34386563396` ✅，证据块「Model-visible history rebuild and pre-dispatch pairing evidence (2026-09-10)」已落。
- **做什么**：保持现状；`ID_CONTRACTS` 表（type_name / owner_crate / wire_name / wire_shape）+ 唯一性断言 + 每类型 serde 往返测试，对照真实 workspace manifest。
- **风险（已核对关闭）**：未改任何 ID 的 serde 表示，线协议兼容保持。
- **验收**：`every_public_type_has_one_owner_and_a_conversion_test`
- **依赖 / 边界**：无依赖；只登记不断言 schema / unknown field / migration 规则（留给 `-01b`）。
- **依据**：`company-os-implementation-outline.md` §Slice A｜`d3f7601` + CI `34386563396` ✅







<a id="step-p0-a-01b"></a>

### P0-A-01b schema 注册表与 unknown field/migration 规则　⏳

- **现状**：ID 契约已登记（`P0-A-01a`），但 canonical domain schema 与 wire protocol schema 未区分注册，unknown field / unknown event / migration 规则缺失。
- **做什么**：建 schema 注册表并区分 domain schema 与 wire schema；定 unknown field / unknown event 处理规则与 migration 规则。
- **风险**：兼容字段增加可升 minor，破坏性变化必须升 major 并提供 upcaster/迁移；未知 major 必须 fail-closed。
- **验收**：`unknown_major_version_fails_closed`
- **依赖 / 边界**：依赖 `P0-A-01a`；兼容边界以 `company-os-spec-index.md` §6.2 为准。
- **依据**：`company-os-implementation-outline.md` §Slice A、`company-os-spec-index.md` §6.2







<a id="step-p0-a-02"></a>

### P0-A-02 稳定错误码枚举　⏳

- **现状**：错误以字符串理由跨层传递，没有 enum 与统一映射（`company-os-implementation-outline.md` §Slice B）。
- **做什么**：`CapabilityErrorCode` enum + `CapabilityResult::failure_code()`；每个码定义 CLI exit、HTTP status、是否可重试、是否需新授权或补偿。
- **风险**：错误码一旦进入 wire 就只能加新码，不能改语义。
- **验收**：`path_escape_uses_stable_error_code`
- **依赖 / 边界**：依赖 `P0-A-01a`；`result_unknown` 必须进对账队列，不能自动 retry。
- **依据**：`company-os-implementation-outline.md` §Slice B







<a id="step-p0-b-01"></a>

### P0-B-01 正式状态机转移表　⏳

- **现状**：状态散落在 core/runner/daemon，取消与 dispatch 的线性化点没有冻结。
- **做什么**：为 Cell、WorkPacket、CapabilityExecution、Approval 各出一张状态转移表，冻结终态不可回、expired approval 不得执行、`result_unknown` 不得自动变成功。
- **风险**：新增中间态会改变 `run.cancelled` 时序，必须保住「每 run 恰好一条终态」。
- **验收**：`illegal_state_transition_is_rejected`
- **依赖 / 边界**：依赖 `P0-A-01a`；Review 不得覆盖 Builder 原始事实。
- **依据**：`company-os-implementation-outline.md` §Slice B







<a id="step-p0-f-01"></a>

### P0-F-01 审批一等请求/应答　⏳

- **现状**：审批以请求内联形式出现，三处前端各自渲染，没有统一的 pending 列表。
- **做什么**：`kiana-protocol` 定义审批请求/应答（单号、对象、`available_decisions`、有效期），由 DaemonHost 发出，TTY / Web / 一次性 CLI 三处渲染并回复。
- **风险**：非交互场景若默认自动批准就破坏了暂停语义，必须显式恢复。
- **验收**：`three_surfaces_list_and_answer_the_same_pending_approval`
- **依赖 / 边界**：依赖 `P0-B-01`；传输保持进程内。
- **依据**：`company-os-implementation-outline.md` §Slice F







<a id="step-p0-f-02"></a>

### P0-F-02 审批决定事件与单次消费　⏳

- **现状**：审批决定不总是产生 durable 记录，重复消费与过期没有统一拒绝。
- **做什么**：新增审批决定事件（actor、scope、subject、expiry、结果），由 ControlPlane 追加进 EventLog 并投影到 transcript 与 Receipt；只能消费一次。
- **风险**：把 decision 的 `request_id` 当作执行关联会破坏审计链，原始执行 `request_id` 必须保留。
- **验收**：`approval_decision_is_durable_and_single_use`
- **依赖 / 边界**：依赖 `P0-F-01`；拒绝与中止是不同终态。
- **依据**：`company-os-implementation-outline.md` §Slice F







<a id="step-p0-f-03"></a>

### P0-F-03 续跑材料落盘与 RunSnapshot　⏳

- **现状**：PendingInvocation 依赖内存，进程重启后无法续跑。
- **做什么**：审批暂存前写 invocation 请求事件（含经 `redact_event_value` 处理的 `CapabilityRequest`、invocation_id、attempt、policy_snapshot、sandbox、游标）及 domain RunSnapshot；重启重建 pending，显式恢复才由 ControlPlane 调 `resume_run`。
- **风险**：runner 若自己读盘恢复，就绕过了 ControlPlane 独占调用账本的约束。
- **验收**：`fresh_process_resume_reconstructs_pending_approval`、`restart_pending_approval_waits_for_explicit_resume`、`restored_pending_approval_rechecks_policy_gate_and_approval`（待补）
- **依赖 / 边界**：依赖 `P0-G-02b`、`P0-G-03`、`P0-F-02`；默认暂停、重新过 policy/gate/approval；缺材料返回 `approval_continuation_unavailable`。
- **依据**：`company-os-implementation-outline.md` §Slice F（A-1）







<a id="step-p0-g-01"></a>

### P0-G-01 session→run 绑定从账本重建　✅

- **现状**：绑定原先只在 `kiana-core/src/sessions.rs` 的内存 map；重启后 `continue_run`/`cancel_run`/`read_receipt` 一律 `session_not_found`。
- **做什么**：内存未命中时从账本只读回读并重建；账本里确实没有时仍 fail-closed。
- **风险**：不得因「从磁盘读」放宽 owner/role/path/approval 授权。
- **验收**：`fresh_control_plane_rebuilds_session_binding_from_ledger`
- **依赖 / 边界**：无依赖；`PROTOCOL_SCHEMA` 未动。
- **依据**：`company-os-implementation-outline.md` §Slice G｜证据：`e8d9346` + CI `34376675138`







<a id="step-p0-g-02a"></a>

### P0-G-02a 账本记录 prompt 与 tool_call 身份　✅

- **现状**：已落地——`run.prompt` 在 start/continue 两处记录并过 redaction，`run.tool_call` 记录 `call_id` / capability / operation。
- **做什么**：保持现状；这是账本可重建 history 的前半。
- **风险**：每轮新增 append，必须沿用轮次聚合的合并语义（粒度测试仍须通过）。
- **验收**：`start_and_continue_prompts_are_recorded_and_redacted_in_the_ledger`
- **依赖 / 边界**：依赖 `P0-G-01`；`call_id` 在 runner 未提供时为 `null`。
- **依据**：`company-os-implementation-outline.md` §Slice G｜`40420bd` + CI `34378214555` + 证据块「Ledger prompt and tool-call identity evidence (2026-09-10)」（`f08a1cf`）







<a id="step-p0-g-02b"></a>

### P0-G-02b 折叠账本重建 history　✅

- **现状**：折叠函数已落地（`41bb971`），三条预派发失败载荷的 `capability_request_id` 已补（`32692da`/`9d255d1`）；CI `34386563396` ✅，证据块「Model-visible history rebuild and pre-dispatch pairing evidence (2026-09-10)」已落。
- **做什么**：保持现状；只读折叠 `run.prompt`/`run.delta`/`capability.*` 为 `Vec<ConversationMessage>`，**不新增事件 kind**。
- **风险（已核对关闭）**：`capability.result_unknown` 会被折叠跳过、且其载荷本就带 `capability_request_id`，不会产出半配对的 history。
- **验收**：`resume_rebuilds_model_visible_history_from_ledger`
- **依赖 / 边界**：依赖 `P0-G-02a`；不新增模型可见工具，不动 `PROTOCOL_SCHEMA`。
- **依据**：`company-os-implementation-outline.md` §Slice G｜`41bb971` + `9d255d1` + CI `34386563396` ✅







<a id="step-p0-g-03"></a>

### P0-G-03 `resume_run` 与协议入口　⏳

- **现状**：没有跨进程 resume 入口；`CURRENT_STATUS.md` 把「跨进程完整 resume」记为 `deferred`。
- **做什么**：`kiana-core` 新增 `resume_run`，复用同一条 `drive_run`；`kiana-protocol` 加 additive 的 `ResumeRequest`。
- **风险**：新增入口若另起一条驱动路径，就形成了第二条执行循环。
- **验收**：`resume_run_reuses_the_same_drive_run_path`
- **依赖 / 边界**：依赖 `P0-G-02b`；`PROTOCOL_SCHEMA` 不动。
- **依据**：`company-os-implementation-outline.md` §Slice G







<a id="step-p0-g-04"></a>

### P0-G-04 事件重建投影　🔄

- **现状**：历史账本只证明 Run 只读投影，原 ✅ 超出完整验收范围；本次观察 WIP 已有 `kiana-core/src/invocation_projection.rs` 与 `recovery.rs`，Invocation 折叠的产品消费及缓存权威替换未获验收证据。
- **做什么**：新增 RunProjection / InvocationProjection，用 `read_stream("run", run_id)` 与 `read_all` 折叠 `run.*`/`capability.*`/`approval.*`；首次按 run_id/session 访问时惰性重建。
- **风险**：折叠遇矛盾终态必须保持 `run_terminal_conflict`/`result_unknown` fail-closed，不能猜。
- **验收**：已有 `new_process_rebuilds_run_state_from_events_alone`；待补 `new_process_rebuilds_invocation_state_from_events_alone`、`projection_cache_miss_rebuilds_pending_invocations_with_authorization_recheck`。
- **依赖 / 边界**：依赖 `P0-G-01`；内存 map 降级为写穿缓存。
- **依据**：`company-os-implementation-outline.md` §Slice G（A-2）｜`3a319be` + CI `34500579350` 是 Run 子集的历史证据；重开见账本「Roadmap source reconciliation evidence (2026-09-12)」。







<a id="step-p0-j1-01"></a>

### P0-J1-01 统一 cancellation token 与状态词表　⏳

- **现状**：三套取消机制并存——core 用 `watch<bool>`、runner 用 `Mutex<Option<String>>`、legacy 入口第三套。
- **做什么**：`RunCancellationState { Accepted, Active, Queued, Cancelling, Cancelled, Terminal }` + 转移表；`ExecutionStatus` 补 `Queued` / `Cancelling`。
- **风险**：新增中间态会改变 `run.cancelled` 时序，必须保住「每 run 恰好一条终态」。
- **验收**：`cancel_transitions_are_total_and_irreversible`
- **依赖 / 边界**：依赖 `P0-B-01`；不新增模型可见工具。
- **依据**：`company-os-implementation-outline.md` §Slice J1







<a id="step-p0-j1-02"></a>

### P0-J1-02 排空已启动工作 + 合成未启动结果　⏳

- **现状**：`KianaHarness::cancel` 在 in-flight 时只发 `Failed{cancelled:}` 就返回，`pending_tools` 与 inbox 直接丢弃。
- **做什么**：排空已启动工作，为未启动的工具调用合成 replay-safe 结果，保证重放不产生副作用。
- **风险**：合成结果若被当成真实执行结果，会污染后续模型可见 history。
- **验收**：`cancel_drains_queued_tool_calls_with_replay_safe_results`
- **依赖 / 边界**：依赖 `P0-J1-01`；参考形状见 `reference/crush/internal/agent/agent.go:432-465`。
- **依据**：`company-os-implementation-outline.md` §Slice J1







<a id="step-p0-j1-03"></a>

### P0-J1-03 进程组确认与 `stop_confirmed`　⏳

- **现状**：取消路径不校验进程是否真的停了；MCP stdio 只在 Drop 里 `start_kill`。
- **做什么**：取消路径确认 shell 进程组已停止并写 `stop_confirmed`；无法确认时进 `result_unknown`。
- **风险**：误报「已停止」会让用户以为副作用已回滚。
- **验收**：`cancel_confirms_shell_process_group_stopped`
- **依赖 / 边界**：依赖 `P0-J1-01`；不改 runner 之外的执行路径。
- **依据**：`company-os-implementation-outline.md` §Slice J1







<a id="step-p0-j1-04"></a>

### P0-J1-04 取消竞态负向证据　⏳

- **现状**：取消与 dispatch 的竞态没有负向测试覆盖。
- **做什么**：补竞态用例，证明取消竞争不产生错误完成、不留孤立 tool calls。
- **风险**：新用例若放宽既有断言，就等于用测试掩盖回归。
- **验收**：`cancel_race_never_produces_wrong_completion`
- **依赖 / 边界**：依赖 `P0-J1-01`–`03`；必须保留 `daemon_host::cancelling_mid_stream_never_completes_or_emits_a_late_delta` 的语义。
- **依据**：`company-os-implementation-outline.md` §Slice J1







<a id="step-p0-j1-05a"></a>

### P0-J1-05a 重复调用检测与 wall-time 预算接线　🔄

- **现状**：重复调用与 wall-time 有 `d973ff6` / `409cfc7` / `747ff8b` 历史证据；`db77c24:kiana-daemon/src/lib.rs:138` 改用仅读 max_steps 的配置 helper，model-config 构造路径丢失 wall-time，按源码回归重开。
- **做什么**：复现并修复 model-config 路径的 wall-time 接线，保留其他构造路径行为；角色步数由 `05b` 单独验收。
- **风险**：非法 wall-time 值也会被该路径忽略；必须在请求模型前拒绝，不能只验证配置 helper。
- **验收**：保留 `run_wall_time_budget_fails_closed`；待补 `model_config_rejects_invalid_wall_time_budget`、`model_config_wall_time_budget_fails_closed`（见 §2）。
- **依赖 / 边界**：无依赖；不同参数不算重复调用，不得误伤。
- **依据**：`company-os-implementation-outline.md` §Slice J1｜`409cfc7` + `747ff8b` + 历史 CI `34389804309`；账本「Run-level wall-time budget evidence (2026-09-10)」及「Roadmap source reconciliation evidence (2026-09-12)」。







<a id="step-p0-j1-05b"></a>

### P0-J1-05b 按角色的 max_steps　🔄

- **现状**：`db77c24` 丢弃 Start 限额且未传角色快照；本次观察 WIP 已在 core 选择角色限额、harness 按 run 保存和消费，并与构造上限取 `min`；原 helper / 构造参数测试尚不足以证明完整产品链。
- **做什么**：沿既有 Start 字段将服务端角色限额传入每个 run；显式环境覆盖优先，未覆盖时使用角色快照，无角色的底层默认保持 32。
- **风险**：必须断言模型调用次数和拒绝结果，避免 helper 通过但执行仍无约束；不以改变共享 harness 全局配置实现不同角色限额。
- **验收**：`role_max_steps_reaches_the_harness`（待补）、`start_command_max_steps_limits_that_run`（加强），及 §2 的环境覆盖、run/Continue 隔离行为断言。
- **依赖 / 边界**：依赖 `P0-J1-05a`；不改 `RoleSpec` 现有字段语义。
- **依据**：`company-os-implementation-outline.md` §Slice J1；现有提交 `db77c24`；账本「Roadmap source reconciliation evidence (2026-09-12)」，尚无本单元完成证据。







<a id="step-p0-j7-01"></a>

### P0-J7-01 流式基线收尾　✅

- **现状**：已落地——增量按轮次聚合落账（`e9df8b4`）、不完整流 fail-closed（`3b65af2`）、命令行默认流式 + `--no-stream`（`e2b15c1`/`d704add`）、断线发 `stream_gap`（`8b2aecb`）。
- **做什么**：保持现状；后续协议扩展见 `P4-J7-02`/`-03`。
- **风险**：无已知风险；回归由 `run_streams_each_delta_by_default_before_terminal_receipt` 覆盖。
- **验收**：`run_streams_each_delta_by_default_before_terminal_receipt`
- **依赖 / 边界**：无依赖；Web 仍不声称 token streaming 之外的能力。
- **依据**：`company-os-implementation-outline.md` §Slice J7｜证据：CI `34376675138`







<a id="step-p0-k1-01"></a>

### P0-K1-01 服务端身份与 authority epoch　⏳

- **现状**：`DaemonHost` 使用固定本地主体并从 stored ProjectTrust 派生 project trust；role/department 仍由请求选择，无 durable authenticated principal。
- **做什么**：由受保护入口解析身份，服务端从不可变 assignment 派生 role/department/authority epoch。
- **风险**：wire 上的 actor/trust/profile 若被当作授权来源，就是越权入口。
- **验收**：`wire_actor_cannot_grant_role_or_department`
- **依赖 / 边界**：依赖 `P0-A-01a`；不改现有 ProjectTrust 派生逻辑的语义。
- **依据**：`company-os-implementation-outline.md` §Slice K1







<a id="step-p0-m1-01"></a>

### P0-M1-01 Workbench 基线　⏳

- **现状**：Workbench 已有 conversation/input/status、trust、sandbox、cancel 和 receipt 入口。
- **做什么**：补齐四表面一致的 terminal state 呈现与 status line/transcript/input 基线。
- **风险**：各表面各自维护状态会产生第二真相。
- **验收**：`workbench_surfaces_agree_on_terminal_state`
- **依赖 / 边界**：无依赖；颜色与布局变化不得隐藏安全状态。
- **依据**：`company-os-implementation-outline.md` §Slice M1

---

## 5. P1 — ContextPlan、Memory、Cache、Capability Catalog、Cost、Eval







<a id="step-p1-c-01"></a>

### P1-C-01 组织与 Cell 契约　⏳

- **现状**：Cell 相关类型零散，`kiana-domain` 没有 `AgentTemplate`/`CellSpec`/`SpawnPlan` 的统一契约。
- **做什么**：定义 `AgentTemplate`、`CellSpec`、`SpawnPlan`、`BudgetLease`、`CapabilityGrant`、`SupervisionLease`；模板版本固定，子权限只减不增，默认不可再委派。
- **风险**：模板版本若不固定，历史 Cell 无法复现。
- **验收**：`child_grant_cannot_exceed_parent_grant`
- **依赖 / 边界**：依赖 `P0-A-01a`；优先在 `kiana-domain` 定义契约。
- **依据**：`company-os-implementation-outline.md` §Slice C







<a id="step-p1-c-02"></a>

### P1-C-02 Cell 生命周期与 retire　⏳

- **现状**：registry/budget 仍是进程内状态，尚无 durable Cell projector 和跨进程恢复。
- **做什么**：打通 reserve→commit→terminal→retire 全链；retire 撤销 grant、释放锁和预算。
- **风险**：spawn 预留预算、锁和 grant 必须具有原子语义，否则会出现预算泄漏。
- **验收**：`retire_revokes_grants_and_releases_budget`
- **依赖 / 边界**：依赖 `P1-C-01`；`kiana-core` 执行授权和生命周期，`kiana-daemon` 提供目录与调度。
- **依据**：`company-os-implementation-outline.md` §Slice C







<a id="step-p1-c-03"></a>

### P1-C-03 五部门角色目录与 model_profile 接线　⏳

- **现状**：`RoleSpec` 十二字段齐备（`kiana-domain/src/roles.rs:116-133`），但角色实例只有写死的构造函数（如 `RoleSpec::builder()`）；`model_profile` 在 `roles.rs` 之外零消费，规划/执行无法异模型。
- **做什么**：按 `COMPANY.md` §3/§4 把五部门 × 角色落成数据集（角色目录），并把 `RoleSpec.model_profile` 接到 provider 路由。
- **风险**：角色目录若散落各 crate 会形成第二真相；「规划用强模型、执行用便宜模型」必须可在收据里复现。
- **验收**：`planning_and_execution_roles_can_use_different_models`
- **依赖 / 边界**：依赖 `P1-C-01`；不新增模型可见工具。
- **依据**：`COMPANY.md` §3、§4







<a id="step-p1-d-01"></a>

### P1-D-01 WorkPacket 单一 ready 谓词　⏳

- **现状**：spawn 校验、`kiana project next`、Web/Desktop 看板各自判断就绪。
- **做什么**：`kiana-domain`/`kiana-tasks` 只暴露一个 `ready_packets(graph, now)`，三处必须调用同一实现。
- **风险**：三处各写一份会让「可派发」的定义漂移。
- **验收**：`single_ready_predicate_agrees_across_three_callers`
- **依赖 / 边界**：依赖 `P0-A-01a`；就绪 = 状态可派发 + 依赖全成功 + 无未过期 lease 冲突。
- **依据**：`company-os-implementation-outline.md` §Slice D







<a id="step-p1-d-02"></a>

### P1-D-02 依赖缺失 / 成环 fail-closed　⏳

- **现状**：依赖边未强制为显式字段，成环检测未在 approve 与模板注册时调用。
- **做什么**：`WorkPacket.dependencies` 显式字段 + `validate_dependency_dag`，输出确定性规范化环（两次运行字节一致），失败拒绝落盘。
- **风险**：从 packet 文本解析依赖会引入不确定性和注入面。
- **验收**：`dependency_cycle_is_rejected_deterministically`
- **依赖 / 边界**：依赖 `P1-D-01`；父 packet blocked 时子 packet 派生 blocked。
- **依据**：`company-os-implementation-outline.md` §Slice D







<a id="step-p1-d-03"></a>

### P1-D-03 claim / lease 心跳回收　⏳

- **现状**：没有 claim(owner, lease_expires_at, heartbeat_at) 与过期扫描。
- **做什么**：spawn / continue / 每个 turn 续租；后台确定性扫描过期 lease，把 packet 退回 ready 并记事件。
- **风险**：ready 只是查询，执行许可仍必须由 policy/gates/approval 产生。
- **验收**：`expired_lease_is_reclaimed_without_double_dispatch`
- **依赖 / 边界**：依赖 `P1-D-01`；worker 死亡后可回收且不重复派发。
- **依据**：`company-os-implementation-outline.md` §Slice D







<a id="step-p1-e-01"></a>

### P1-E-01 通信与问责分层　⏳

- **现状**：Chat、Command、Handoff 等消息没有类型区分，自由聊天可能被当成授权。
- **做什么**：区分 Chat、Command、Handoff、Decision、StatusReport、Evidence、Incident；Handoff 必须定向并 ACK。
- **风险**：自由聊天一旦产生授权，问责链就断了。
- **验收**：`free_chat_never_grants_authority`
- **依赖 / 边界**：依赖 `P0-B-01`；`kiana-ports` 定义接口，`kiana-core` 产生正式事件。
- **依据**：`company-os-implementation-outline.md` §Slice E







<a id="step-p1-e-02"></a>

### P1-E-02 Symposium 会议对象契约化　⏳

- **现状**：`Symposium`/投票/黑板/`DecisionRecord` 已实现（`kiana-domain/src/symposiums.rs`），`convene_symposium` 有 chair 必须为 PM、`can_convene` 与 workspace-write 校验（`kiana-core/src/collaboration.rs:955-1044`）——对应 `COMPANY.md` §5.4 的 v0.3/v0.4，但从未进验收追踪。
- **做什么**：给现有会议路径补验收测试（chair 校验、投票、决议产出）；会议决定事件 durable 可重放。
- **风险**：会议代码已存在却不在 §1 表里，回归不可见；决定事件若不 durable，重启后决议丢失。
- **验收**：`symposium_decision_is_durable_and_replayable`
- **依赖 / 边界**：依赖 `P1-E-01`；不改会议参会边界（Builder 不进规划/监控会，冻结项）。
- **依据**：`COMPANY.md` §5.1–5.4







<a id="step-p1-h-01"></a>

### P1-H-01 `ToolSpec` registry　⏳

- **现状**：工具权威分散在 5 处，新增工具要改多处。
- **做什么**：`kiana-domain` 新增 `tool_authority` 模块（`ToolSpec{name, aliases, capability, operation, risk_policy, side_effecting, schema}` + `TOOL_SPECS`）。
- **风险**：registry 若成为第二套 authority 而不被 policy 消费，就是摆设。
- **验收**：`tool_authority_covers_every_model_visible_tool`
- **依赖 / 边界**：依赖 `P0-A-01a`；**不新增模型可见工具**，保持 5 个。
- **依据**：`company-os-implementation-outline.md` §Slice H







<a id="step-p1-h-02"></a>

### P1-H-02 参数 schema 校验　✅

- **现状**：`9095ea7` 已加 `validate_tool_arguments`，在映射到 capability 前按现有 schema 表校验。
- **做什么**：保持现状；校验覆盖 `type` / `required` / `minimum` / `enum` 子集。
- **风险**：`additionalProperties` 不拒绝（老 cassette 不被误拒）；`schema_name_for_tool` 与 `tools.rs:234` 的别名表重复，`P1-H-01` 必须收敛。
- **验收**：`malformed_arguments_are_rejected_before_capability_mapping`
- **依赖 / 边界**：无硬依赖（`P1-H-01` 落地后只需替换数据源）；不改变已有工具的接受集。
- **依据**：`company-os-implementation-outline.md` §Slice H｜`9095ea7` + CI `34380323510` + 证据块「Tool argument validation at capability mapping evidence (2026-09-10)」（`e144d30`）







<a id="step-p1-h-03"></a>

### P1-H-03 路径 containment 共享实现　⏳

- **现状**：策略层只对 `apply_patch` 提取路径，shell 全靠 bwrap/workdir。
- **做什么**：抽出一份共享的 path containment，所有副作用工具共用。
- **风险**：只对部分工具生效会留下绕过面（symlink / hardlink / rename）。
- **验收**：`path_containment_is_shared_by_every_side_effecting_tool`
- **依赖 / 边界**：依赖 `P1-H-01`；不得放宽现有 fail-closed 行为。
- **依据**：`company-os-implementation-outline.md` §Slice H







<a id="step-p1-j2-01"></a>

### P1-J2-01 类型化区段 + provenance　⏳

- **现状**：context 按字符串拼接，无区段类型、无 provenance。
- **做什么**：`PromptSection{name, order, text}` + `render_prompt()`，每个区段带来源。
- **风险**：改了拼接顺序会影响 cassette 命中，必须固定 `order`。
- **验收**：`context_sections_render_with_provenance`
- **依赖 / 边界**：依赖 `P0-G-04`；不新增模型可见工具。
- **依据**：`company-os-implementation-outline.md` §Slice J2







<a id="step-p1-j2-02"></a>

### P1-J2-02 预算覆盖 tool schemas 与 system prompt　⏳

- **现状**：token 预算只算消息，不算 tool schemas 与 system prompt。
- **做什么**：`TokenBudget` 计入 tool schemas 与 system prompt；越界 fail-closed。
- **风险**：漏算会让实际请求超出 provider 上限并报错在错误的层。
- **验收**：`tool_schemas_count_toward_the_budget`
- **依赖 / 边界**：依赖 `P1-J2-01`；压缩策略不在本单元。
- **依据**：`company-os-implementation-outline.md` §Slice J2







<a id="step-p1-j2-03"></a>

### P1-J2-03 角色 prompt 接线　⏳

- **现状**：部分接线——写死的 `RoleSpec::builder().prompt` 已是 system message 来源（`kiana-daemon/src/model_client.rs:166`，环境变量可覆盖），按角色注入仍缺。
- **做什么**：把 `RoleSpec.prompt` 接到 provider 的 system message。
- **风险**：角色 prompt 若覆盖系统安全指令，会削弱边界。
- **验收**：`role_prompt_reaches_the_provider_system_message`
- **依赖 / 边界**：依赖 `P1-J2-01`；不改变现有安全指令的优先级。
- **依据**：`company-os-implementation-outline.md` §Slice J2







<a id="step-p1-j2-04"></a>

### P1-J2-04 提示词来源与角色包加载　⏳

- **现状**：`provider_system_prompt()`（`kiana-daemon/src/model_client.rs:166`）先读环境变量，否则回落 `RoleSpec::builder().prompt`——角色提示词写死在 `roles.rs` 构造函数，不从 kiana-skills 角色包加载。
- **做什么**：角色提示词改从角色包 / `kiana-skills` 加载；`prompt_hash` 进收据可复现。
- **风险**：提示词换源后 hash 必须按换源后的内容计算，否则收据复现失效；环境变量逃生口保留但不得成为产品路径。
- **验收**：`role_prompt_loads_from_the_role_pack`
- **依赖 / 边界**：依赖 `P1-J2-03`（先有管道再换源）；提示词产品拥有（12-factor #2）。
- **依据**：`COMPANY.md` §4 原则 2







<a id="step-p1-j3-01"></a>

### P1-J3-01 Memory 写入候选制　⏳

- **现状**：模型写入的 memory 可能直接进入可检索集合。
- **做什么**：`memory.write` 由模型写入一律落 candidate + draft；`origin` 由服务端派生（model / hook / git / user）；默认检索排除 candidate，只有操作者或目标层 owner 显式批准才转 active。
- **风险**：模型不能自批；instance-scratch 层保持默认可见。
- **验收**：`model_written_memory_stays_unsearchable_until_approved`
- **依赖 / 边界**：依赖 `P0-A-01a`；持久层 candidate 默认不可检索。
- **依据**：`company-os-implementation-outline.md` §Slice J3







<a id="step-p1-j3-02"></a>

### P1-J3-02 分层检索与密级　⏳

- **现状**：分层存储已存在——home 侧 `company/user/user-prefs/user-private.jsonl`，项目侧 `department/role/project/instance/*.jsonl`（`kiana-daemon/src/harness_memory.rs:220-293`）；读写两端都有 RoleSpec grants ACL（`allows_knowledge` / `allows_memory_write`）。缺：相关性打分（现为纯 AND 词项包含 `text_matches`）、命中与收据的显式关联（COMPANY.md 的 `AgentInstance.retrieved`）、「检索结果不得静默拼进系统提示」的强制。
- **做什么**：检索升级为多词项 OR + 计数打分（CLI 侧 `search_memory_records` 已有此实现，搬到 harness 工具面）；命中显式写进收据（谁查了什么、用了哪几条、来自哪层）。
- **风险**：无法指认来源的内容不得进入 Reviewer 的「已验证」结论。
- **验收**：`memory_hits_respect_knowledge_grants_and_reach_the_receipt`
- **依赖 / 边界**：依赖 `P1-J3-01`；检索结果本回合注入、下回合重查，不得静默拼进系统提示；不引入网络 embedding 服务。
- **依据**：`COMPANY.md` §7｜参考：mem0 extract→consolidate 管线（`reference/mem0`）、ruflo hybrid 检索与 ReasoningBank 蒸馏（机制层面）







<a id="step-p1-j3-03"></a>

### P1-J3-03 抽取建议包与三档准入　⏳

- **现状**：`memory.write` 在 policy ACL 内直写目标层即生效（`harness_memory.rs:157`），无建议包、无 turn 结束抽取。
- **做什么**：turn 结束抽取器产出 `kiana.memory-proposal.v1`（facts + ADD/UPDATE/DELETE 建议 + evidence 引文 + 相似旧记录 top-3）；持久层写入落 candidate 不可检索、批准转 qualified；scratch 即时生效；user-private 仅人工。
- **风险**：抽取失败不阻塞 run（incident 事件）；建议缺 evidence 视为 bug。
- **验收**：`extraction_proposals_carry_evidence_and_similar_records`
- **依赖 / 边界**：依赖 `P1-J3-01`、`P0-F-01`；审批入口复用后一单元，不另起。
- **依据**：设计 `docs/superpowers/specs/2026-09-10-memory-architecture-design.md` §7｜`COMPANY.md` §7







<a id="step-p1-j3-04"></a>

### P1-J3-04 hybrid 检索基建　⏳

- **现状**：J3-02 归一后为词项 OR + 计分；无向量通道、无 BM25。
- **做什么**：BM25（CJK 双字组）+ 本地 ONNX embedding（模型注册表 hash 钉版）→ RRF → MMR；ort feature flag 默认关，CI 用 fixture embedder；模型缺失降级纯词项且命中带 `degraded` 标记。
- **风险**：索引是派生物可重建；确定性断言（钉住模型 → 同输入同命中）。
- **验收**：`hybrid_retrieval_is_deterministic_for_a_pinned_model`
- **依赖 / 边界**：依赖 `P1-J3-02`；不引入网络 embedding 服务；新增构建依赖须 feature 门控。
- **依据**：设计 `docs/superpowers/specs/2026-09-10-memory-architecture-design.md` §5–§6、§11







<a id="step-p1-j4-01"></a>

### P1-J4-01 Capability Descriptor 与 MCP 生命周期　⏳

- **现状**：capability 合同较窄，MCP server/tool schema、health、trust、version 不可追踪。
- **做什么**：把 `(CapabilityKind, operation)` 扩展为带域、版本、风险、scope、approval、幂等和补偿描述的 descriptor；MCP 走 stdio 生命周期管理。
- **风险**：未知 operation、参数越界、资源越界、过期 grant 必须 fail-closed。
- **验收**：`mcp_tool_schema_and_health_are_traceable`
- **依赖 / 边界**：依赖 `P0-A-01a`；**HTTP MCP 冻结**，只支持 stdio。
- **依据**：`company-os-implementation-outline.md` §Slice H、§Slice J4







<a id="step-p1-j8-01"></a>

### P1-J8-01 Observability 与 trace/receipt　⏳

- **现状**：provider/model、policy verdict、usage、retry/cancel reason 没有统一记录。
- **做什么**：记录 provider/model、policy verdict、tool arguments hash、workspace、timing、usage/cost、retry/cancellation reason 和 persistence revision，同时不泄露 secrets。
- **风险**：记录 tool arguments 若不做 redaction 就是泄密面。
- **验收**：`observability_record_is_redacted_and_traceable`
- **依赖 / 边界**：依赖 `P0-G-04`；复用现有 `redact_event_value` 边界。
- **依据**：`company-os-implementation-outline.md` §Slice J8







<a id="step-p1-k5-01"></a>

### P1-K5-01 成本与容量账本　⏳

- **现状**：usage 未形成账本，预算类型混用。
- **做什么**：`UsageRecord`/`CostLedger`/`Quota`；`RuntimeBudget`、`ProjectBudget` 和未来的 `FinancialBudget` 不得混用。
- **风险**：混用预算会让成本归属失真。
- **验收**：`runtime_and_project_budgets_are_not_interchangeable`
- **依赖 / 边界**：依赖 `P0-G-04`；不引入真实计费。
- **依据**：`company-os-implementation-outline.md` §Slice K5







<a id="step-p1-l1-01"></a>

### P1-L1-01 EvalSuite 与 GoldenTrace　⏳

- **现状**：已有 cassette、focused regression，但没有统一 EvalSuite 与 GoldenTrace。
- **做什么**：GoldenTrace 绑定源码快照、输入 hash、版本和 Receipt；eval replay 不产生真实外部副作用。
- **风险**：GoldenTrace 若不绑定快照，回归会被静默吞掉。
- **验收**：`golden_trace_replay_has_no_external_side_effects`
- **依赖 / 边界**：依赖 `P0-G-04`；安全失败、禁止效果、replay divergence 和 evidence 缺失会阻断 Promote。
- **依据**：`company-os-implementation-outline.md` §Slice L1







<a id="step-p1-l4-01"></a>

### P1-L4-01 Code intelligence 快照　⏳

- **现状**：query/repo-map 已有素材，但结果不带 snapshot 与 freshness。
- **做什么**：`RepositorySnapshot`/`SymbolIndex`/`DependencyGraph`/`RepoMap` 的结果带 snapshot、来源和 freshness。
- **风险**：无 freshness 的索引结果会被当成当前事实。
- **验收**：`code_intelligence_results_carry_freshness`
- **依赖 / 边界**：依赖 `P0-A-01a`；不改变现有索引的可见范围。
- **依据**：`company-os-implementation-outline.md` §Slice L4

---

## 6. P2 — Durable Workflow、Human Inbox、Recovery、Artifact、Client cursor







<a id="step-p2-j5-01"></a>

### P2-J5-01 Workflow definition 与重放　⏳

- **现状**：`kiana-workflow` 的 WorkflowDefinition/NodeExecution/Signal/Compensation 为 `target`。
- **做什么**：Workflow definition 版本固定，重试、取消、审批、补偿和恢复均可重放。
- **风险**：版本不固定时，重放会执行到与当初不同的节点。
- **验收**：`workflow_definition_replays_after_restart`
- **依赖 / 边界**：依赖 `P0-G-04`；本切片不新增第二套 ControlPlane 或 Agent loop。
- **依据**：`company-os-implementation-outline.md` §Slice J5







<a id="step-p2-k3-01"></a>

### P2-K3-01 Human Inbox　⏳

- **现状**：审批、复核、验收、异常分散在不同界面，没有统一待办入口。
- **做什么**：Approval、Review、Acceptance、Incident 和 Reconciliation 进入同一 Human Inbox。
- **风险**：Inbox 若只做展示不做 resolve，会形成第二套决策入口。
- **验收**：`human_inbox_collects_all_decision_types`
- **依赖 / 边界**：依赖 `P0-F-02`；决策仍由 ControlPlane resolve。
- **依据**：`company-os-implementation-outline.md` §Slice K3







<a id="step-p2-k4-01"></a>

### P2-K4-01 Artifact 版本与编辑级 undo　⏳

- **现状**：已有 `apply_patch` 前置快照（capture_preconditions / restore_snapshot），但没有绑定 transcript 的 CheckpointService。
- **做什么**：首发只做状态层 + 编辑级 undo——CheckpointService 绑定 transcript offset + workspace revision + invocation，写工具前与用户输入前快照。
- **风险**：preview 绝不能写盘；undo 是受控操作，不作为模型可见工具。
- **验收**：`restore_invalidates_stale_approval`
- **依赖 / 边界**：依赖 `P0-G-04`；文件层 shadow git 列为第二阶段。
- **依据**：`company-os-implementation-outline.md` §Slice K4（A-10）







<a id="step-p2-k6-01"></a>

### P2-K6-01 可靠性与对账　⏳

- **现状**：crash/timeout/cancel/disk full/MCP failure/Provider Unknown 没有统一 Incident/Recovery 路径。
- **做什么**：六类失败各有 Incident 与 RecoveryPlan，`result_unknown` 进 reconciliation queue。
- **风险**：`result_unknown` 若被自动 retry，可能产生重复副作用。
- **验收**：`every_failure_class_has_an_incident_and_recovery`
- **依赖 / 边界**：依赖 `P2-K4-01`；不自动重试 `result_unknown`。
- **依据**：`company-os-implementation-outline.md` §Slice K6







<a id="step-p2-k7-01"></a>

### P2-K7-01 数据治理与删除传播　⏳

- **现状**：`DataClass`/`Purpose`/`ProcessingGrant`/`Retention` 为 `target`，删除不会传播。
- **做什么**：删除、过期和撤销能传播到 Memory、Artifact、Index、Compaction 和 cache policy。
- **风险**：传播不全等于数据没删干净，却对外声称已删。
- **验收**：`deletion_propagates_to_memory_and_index`
- **依赖 / 边界**：依赖 `P0-A-01a`、`P1-J3-04`；不改变现有 ACL 语义。
- **依据**：`company-os-implementation-outline.md` §Slice K7







<a id="step-p2-l2-01"></a>

### P2-L2-01 反馈与候选改进　⏳

- **现状**：`Feedback`/`Pattern`/`Candidate`/`Promotion` 为 `target`。
- **做什么**：Feedback 只能产生候选改进，不能直接修改 Role、Grant、Policy 或历史事实。
- **风险**：反馈若直接改 policy，就绕过了审批与审计。
- **验收**：`feedback_cannot_mutate_policy_or_history`
- **依赖 / 边界**：依赖 `P1-L1-01`；晋级必须经质量门禁。
- **依据**：`company-os-implementation-outline.md` §Slice L2







<a id="step-p2-m2-01"></a>

### P2-M2-01 UI 投影合同　⏳

- **现状**：各前端各自维护会话状态，没有统一的 `UiSnapshot`/`UiAction`。
- **做什么**：`kiana-protocol` 定义 `UiSnapshot`/`UiAction`（带 cursor、epoch、pending action），`kiana-daemon` 生成投影。
- **风险**：乐观更新若覆盖更新的服务端事件，界面会显示错误状态。
- **验收**：`stale_ui_action_is_rejected_by_epoch`
- **依赖 / 边界**：依赖 `P0-M1-01`；UI 不维护第二套运行循环。
- **依据**：`company-os-implementation-outline.md` §Slice M2







<a id="step-p2-m3-01"></a>

### P2-M3-01 人工动作卡　⏳

- **现状**：审批、复核、验收、异常的动作界面各写一份。
- **做什么**：Approval、Review、Acceptance、Incident 动作卡三处（CLI/TTY/Web）复用同一实现。
- **风险**：动作卡若各自决定 `available_decisions`，会出现越权选项。
- **验收**：`action_card_is_shared_by_all_surfaces`
- **依赖 / 边界**：依赖 `P2-M2-01`；动作必须带 target ID、owner、expected epoch 和 idempotency key。
- **依据**：`company-os-implementation-outline.md` §Slice M3







<a id="step-p2-m4-01"></a>

### P2-M4-01 Run/Artifact 详情　⏳

- **现状**：Run timeline、Invocation、Diff、Evidence、Receipt 没有统一详情视图。
- **做什么**：四类详情可相互定位（Receipt ↔ Artifact ↔ Evidence ↔ Review）。
- **风险**：详情视图若自行拼装数据，会与事件事实不一致。
- **验收**：`receipt_artifact_and_evidence_cross_locate`
- **依赖 / 边界**：依赖 `P2-M2-01`；不引入新的存储。
- **依据**：`company-os-implementation-outline.md` §Slice M4







<a id="step-p2-m5-01"></a>

### P2-M5-01 Web 快照水合与重连　⏳

- **现状**：Web 订阅事件但没有合并 snapshot load 期间观察到的事件，重连语义未冻结。
- **做什么**：snapshot hydration + 事件订阅 + 重连不重放已送 delta。
- **风险**：重放已送 delta 会让前端重复渲染。
- **验收**：`web_sse_reconnect_emits_stream_gap_without_replaying_delta_items`
- **依赖 / 边界**：依赖 `P2-M2-01`；Web 保持 loopback-only。
- **依据**：`company-os-implementation-outline.md` §Slice M5







<a id="step-p2-m5-02"></a>

### P2-M5-02 重启后列出历史会话　✅

- **现状**：`1c504a0` 已实现只读列出；数据源是 `DaemonHost::persisted_events()` 端口，web 层不再自己解析 JSONL。
- **做什么**：保持现状；只读列出持久会话（不含写入与续跑）。
- **风险**：列表若暴露其他主体的会话就是越权；存储不支持全量读取时返回 `web_session_history_unsupported`。
- **验收**：`web_lists_persisted_sessions_after_restart`
- **依赖 / 边界**：依赖 `P0-G-01`；前置 enabler 是 `0a9a56b`（只读端口）；只读，不提供续跑入口。
- **依据**：`company-os-implementation-outline.md` §Slice M5｜`1c504a0` + CI `34380076942` + 证据块「Read-only persisted web session history evidence (2026-09-10)」（`e144d30`）







<a id="step-p2-m7-01"></a>

### P2-M7-01 无障碍回退　⏳

- **现状**：状态主要靠颜色和布局表达。
- **做什么**：键盘、窄屏、文本状态、aria/高对比全部可用。
- **风险**：颜色与动画变化不得隐藏安全状态。
- **验收**：`status_is_reachable_without_color`
- **依赖 / 边界**：依赖 `P2-M2-01`；不改动信息层级。
- **依据**：`company-os-implementation-outline.md` §Slice M7

---

## 7. P3 — Objective → Project → Packet → Builder → Review → Acceptance → Close







<a id="step-p3-i-01"></a>

### P3-I-01 Company 业务对象契约　⏳

- **现状**：Company 业务域对象基本为 `target`，现有 WorkPacket/Review 不能替代完整聚合。
- **做什么**：定义 `Objective`、`Initiative`、`Project`、`Milestone`、`Acceptance`、`Delivery`、`Outcome`、`ChangeRequest`、`Risk`、`Incident` 及不变量。
- **风险**：把业务对象实现成 prompt 里的名词，而不是 domain 类型。
- **验收**：`company_objects_expose_invariants`
- **依赖 / 边界**：依赖 `P0-A-01a`；字段与状态机以 `company-os-domain-contracts.md` 为准。
- **依据**：`company-os-implementation-outline.md` §Slice I







<a id="step-p3-i-02"></a>

### P3-I-02 命令与事件冻结　⏳

- **现状**：九个业务命令/事件尚未冻结。
- **做什么**：冻结 `propose_objective` → `ObjectiveProposed` 等九个命令/事件对，并版本化。
- **风险**：命令一旦发布只能加版本，不能改语义。
- **验收**：`company_commands_are_frozen_and_versioned`
- **依赖 / 边界**：依赖 `P3-I-01`；`kiana-protocol` 承载 versioned DTO。
- **依据**：`company-os-implementation-outline.md` §Slice I







<a id="step-p3-i-03"></a>

### P3-I-03 全链重建　⏳

- **现状**：Objective → Project → Packet → Run → Acceptance → Receipt 无法从事实重建。
- **做什么**：新进程可以从事件和 Artifact 引用重建全链。
- **风险**：未批准、过期审批、越权路径、重复命令和重复交付必须 fail-closed。
- **验收**：`new_process_rebuilds_the_company_chain`
- **依赖 / 边界**：依赖 `P3-I-02`、`P0-G-04`；不引入第二事实源。
- **依据**：`company-os-implementation-outline.md` §Slice I







<a id="step-p3-i-04"></a>

### P3-I-04 Acceptance 快照与独立 Review　⏳

- **现状**：验收标准未冻结为 snapshot，Reviewer 与 Builder 可能共享 author session。
- **做什么**：`Acceptance` 进入 `Requested` 时从已冻结的上层标准派生并冻结为 `criteria_snapshot`；决策阶段只读校验。
- **风险**：Reviewer 改写 Builder 原始事实会破坏问责链。
- **验收**：`reviewer_cannot_rewrite_builder_facts`
- **依赖 / 边界**：依赖 `P3-I-02`；Reviewer 与 Builder 不共享 author session 或可修改的验收基线。
- **依据**：`company-os-implementation-outline.md` §Slice I







<a id="step-p3-i-05"></a>

### P3-I-05 Delivery / ClosingReceipt / Outcome　⏳

- **现状**：没有 Delivery、ClosingReceipt 与 Outcome 测量。
- **做什么**：Project 关闭必须有 Acceptance、Delivery、ClosingReceipt，或有明确的失败关闭/人工豁免；Outcome 需要测量窗口与观测证据。
- **风险**：Objective `Achieved` 不能由 Receipt 或模型文本直接宣称。
- **验收**：`outcome_cannot_be_claimed_without_measurement`
- **依赖 / 边界**：依赖 `P3-I-03`；`result_unknown` 必须关联 Incident/Reconciliation。
- **依据**：`company-os-implementation-outline.md` §Slice I







<a id="step-p3-i-06"></a>

### P3-I-06 fake-model coding 黄金闭环　⏳

- **现状**：没有端到端的 Company 闭环验收。
- **做什么**：用 fake model 跑通 Objective → Project → Planner → Builder → Reviewer → Acceptance → Closer，产出完整 ClosingReceipt。
- **风险**：把「Agent 运行成功」当成「业务结果成功」。
- **验收**：`fake_model_coding_project_produces_closing_receipt`
- **依赖 / 边界**：依赖 `P3-I-05`；拒绝、返工、暂停、取消、失败关闭和 Unknown 各有可重放状态。
- **依据**：`company-os-implementation-outline.md` §Slice I

---

## 8. P4 — 有界 Swarm、Scheduler、Provider streaming、Skill/Plugin 生态







<a id="step-p4-e-03"></a>

### P4-E-03 五部门开会与决议入部门 RAG　⏳

- **现状**：只有规划部（v0.3）与监控部（v0.4）会议路径；五部门开会与「决议进部门 RAG」（`COMPANY.md` §5.4 v0.5）未实现。
- **做什么**：五部门可各自开会；决议写入部门记忆层，受 `P1-J3-02` 的密级约束。
- **风险**：跨部门联席必须仍是显式对象；决议入库走 `memory.write` 晋升规则，不得自动写入。
- **验收**：`department_resolutions_enter_the_department_memory_layer`
- **依赖 / 边界**：依赖 `P1-E-02`、`P1-J3-02`、`P1-J3-03`（决议走建议包通道，`kind=decision`）；多租户部门会议 ACL 仍属 P6 边界。
- **依据**：`COMPANY.md` §5.4







<a id="step-p4-j3-05"></a>

### P4-J3-05 run 蒸馏与 lesson 入库　⏳

- **现状**：run 到终态只留账本事件，无蒸馏。
- **做什么**：run 终态触发蒸馏器产 `kiana.memory-distillation.v1` lesson 候选（带 verdict）→ 部门层 candidate → 审批入库（`kind=lesson`）；symposium 决议走同一通道（`kind=decision`）。
- **风险**：蒸馏是 LLM 输出，必须过 T1 准入，噪音止步于审批卡；triple 只收集不检索。
- **验收**：`run_distillation_lands_as_lesson_candidate`
- **依赖 / 边界**：依赖 `P1-J3-03`；不新增模型可见工具。
- **依据**：设计 `docs/superpowers/specs/2026-09-10-memory-architecture-design.md` §9







<a id="step-p4-j6-01"></a>

### P4-J6-01 有界 Swarm　⏳

- **现状**：`SwarmPlan`/`Partition`/`Child Cell`/`MergeDecision` 为 `target`。
- **做什么**：fan-out 必须有 parent、partition、预算、并发、TTL、WorkFingerprint 和 MergeDecision。
- **风险**：无限 fan-out 会让预算与授权失效。
- **验收**：`swarm_fanout_is_bounded_and_merges_deterministically`
- **依赖 / 边界**：依赖 `P1-C-02`；相同 WorkFingerprint 不重复创建。
- **依据**：`company-os-implementation-outline.md` §Slice J6







<a id="step-p4-j7-02"></a>

### P4-J7-02 wire 加 `sequence`/`epoch`　⏳

- **现状**：事件 wire 没有 `sequence`/`epoch`，客户端无法判断乱序与陈旧。
- **做什么**：additive 地加 `sequence`/`epoch`；序号单调。
- **风险**：破坏性改动会打翻现有客户端，必须 additive。
- **验收**：`run_stream_sequence_is_monotonic`
- **依赖 / 边界**：依赖 `P0-J7-01`；`PROTOCOL_SCHEMA` 不动。
- **依据**：`company-os-implementation-outline.md` §Slice J7







<a id="step-p4-j7-03"></a>

### P4-J7-03 事件种类补齐与 terminal 必达　⏳

- **现状**：`Usage`/`ToolCall`/`ApprovalRequested`/`Error` 未投影；terminal 对迟到订阅者不保证必达。
- **做什么**：补齐四类事件投影；terminal 重放给迟到订阅者（`Last-Event-ID`）。
- **风险**：terminal 丢失会让客户端永远等不到结束。
- **验收**：`terminal_is_replayed_to_late_subscriber`
- **依赖 / 边界**：依赖 `P4-J7-02`；不新增运行循环。
- **依据**：`company-os-implementation-outline.md` §Slice J7







<a id="step-p4-k2-01"></a>

### P4-K2-01 触发器与调度　⏳

- **现状**：`TriggerDefinition`/`TriggerFiring`/`Schedule`/`Signal` 为 `target`。
- **做什么**：Trigger 只能创建 Workflow/Run，不能直接执行 Capability。
- **风险**：Trigger 直连 capability 就绕过了审批。
- **验收**：`trigger_cannot_execute_a_capability_directly`
- **依赖 / 边界**：依赖 `P0-B-01`；调度不持有执行许可。
- **依据**：`company-os-implementation-outline.md` §Slice K2







<a id="step-p4-k8-01"></a>

### P4-K8-01 Connector　⏳

- **现状**：`ConnectorDefinition`/`AccountBinding`/`ProviderReceipt` 为 `not_supported`/`target`。
- **做什么**：Connector 不绕过 ControlPlane、Approval、Idempotency、Receipt 和 reconciliation。
- **风险**：Connector 是外部副作用入口，绕过控制面即等于无审计。
- **验收**：`connector_cannot_bypass_the_control_plane`
- **依赖 / 边界**：依赖 `P0-A-01a`；R3+ 需最终 payload 单次确认。
- **依据**：`company-os-implementation-outline.md` §Slice K8







<a id="step-p4-l3-01"></a>

### P4-L3-01 版本治理与 drift　⏳

- **现状**：`ModelProfile`/`PromptBundle`/`RouteDecision`/`DriftReport` 为 `target`。
- **做什么**：模型、Prompt、Route 与 drift 报告按版本分桶。
- **风险**：不分桶就无法判断指标变化来自哪次变更。
- **验收**：`drift_report_is_bucketed_by_version`
- **依赖 / 边界**：依赖 `P1-L1-01`；不自动切换模型版本。
- **依据**：`company-os-implementation-outline.md` §Slice L3







<a id="step-p4-l5-01"></a>

### P4-L5-01 扩展与技能包　⏳

- **现状**：`ExtensionManifest`/`SkillPack` 为 `partial`/`target`；skill 的 `allowed-tools` 语义未冻结。
- **做什么**：skill 的 `allowed-tools` 只影响提示/展示，不进入 policy；扩展清单增加 effect、required_capabilities、network_policy、content_hash/signature、requires。
- **风险**：声明不等于授权；read-only 扩展的写操作必须在 broker 层直接拒绝。
- **验收**：`skill_allowed_tools_cannot_grant_shell`
- **依赖 / 边界**：依赖 `P1-H-01`；扩展不能创建第二套 Runtime。
- **依据**：`company-os-implementation-outline.md` §Slice L5（F-2）







<a id="step-p4-l6-01"></a>

### P4-L6-01 供应链　⏳

- **现状**：content hash、license、signature、capability diff、rollback 为 `target`。
- **做什么**：安装、升级、迁移、撤销和回滚可审计，安装时校验摘要与兼容性。
- **风险**：安装成功不等于安全验证完成。
- **验收**：`extension_signature_is_verified_before_install`
- **依赖 / 边界**：依赖 `P4-L5-01`；不做签名分发的生产化。
- **依据**：`company-os-implementation-outline.md` §Slice L6







<a id="step-p4-m6-01"></a>

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
| P6 | Team / Remote / Enterprise | 需要 durable principal、跨机身份与供应链证据；本范围仍 deferred/source，provider/streaming 的有限 live 证据不证明企业或远程能力 | `company-os-spec-index.md` §7；`CURRENT_STATUS.md` §2 |

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

---

<a id="appendix-navigation"></a>


## 专项设计导航（拆分文档）

> 为了让总图可快速扫描，以下专项保留在独立文档中。主文档承载 P0–P4 基础执行单元、当前窗口、全量 Step 索引、冻结项与完成定义；专项文档承载设计说明和细化步骤。两层编号互不替代，状态仍以本页总图与 `CURRENT_STATUS.md` 的证据块为准。

| 专项 | 细化卡 | 内容 |
|---|---:|---|
| ControlPlane | CP-00–CP-30（31） | [授权、状态转移与恢复设计](roadmap/control-plane.md) |
| Harness | H01–H36（36） | [Agent 运行时设计与实施步骤](roadmap/harness.md) |
| Provider | P4-J7-04–P4-J7-31（28） | [Provider 协议与 streaming 设计](roadmap/provider.md) |
| CompanyOS | CO-01–CO-48（48） | [组织、业务流程与交付闭环](roadmap/companyos.md) |
| Capability | CAP-00–CAP-34（35） | [能力目录、Broker 与执行边界](roadmap/capability.md) |
| Event / Receipt / Recovery | ER-00–ER-36（37） | [事实账本、收据与恢复](roadmap/event-receipt-recovery.md) |
| Context / Memory | CM-00–CM-39（40） | [上下文、检索、记忆与治理](roadmap/context-memory.md) |
| Skills / Plugins / Hooks | EXT-00–EXT-31（32） | [扩展来源、信任与生命周期](roadmap/skills-plugins-hooks.md) |
| UI / Entrypoints | UI-00–UI-41（42） | [CLI、Workbench、Web、Desktop 与入口一致性](roadmap/ui-entrypoints.md) |

---

## 14. ControlPlane 专项追加：设计、处理流程与实施步骤（2026-09-12）

> 本专项已拆到 [ControlPlane 专项](roadmap/control-plane.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

<a id="harness-runtime-plan"></a>

## 15. Harness：Agent 运行时设计与实施步骤（2026-09-12 追加）

> 本专项已拆到 [Harness 专项](roadmap/harness.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

<a id="provider-plan"></a>

## 16. Provider 专项：设计与详细执行步骤（2026-09-12 追加）

> 本专项已拆到 [Provider 专项](roadmap/provider.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

<a id="companyos-business-design"></a>

## 17. CompanyOS 组织与业务补全（2026-09-12 追加设计）

> 本专项已拆到 [CompanyOS 组织与业务专项](roadmap/companyos.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

<a id="companyos-business-steps"></a>

## 18. CompanyOS 详细步骤（CO-01–CO-48）

> 本专项已拆到 [CompanyOS 组织与业务专项](roadmap/companyos.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

<!-- 保留原 CO-01–CO-48 入站锚点。 -->
<a id="co-01"></a> [CO-01 详细卡](roadmap/companyos.md#step-co-01)
<a id="co-02"></a> [CO-02 详细卡](roadmap/companyos.md#step-co-02)
<a id="co-03"></a> [CO-03 详细卡](roadmap/companyos.md#step-co-03)
<a id="co-04"></a> [CO-04 详细卡](roadmap/companyos.md#step-co-04)
<a id="co-05"></a> [CO-05 详细卡](roadmap/companyos.md#step-co-05)
<a id="co-06"></a> [CO-06 详细卡](roadmap/companyos.md#step-co-06)
<a id="co-07"></a> [CO-07 详细卡](roadmap/companyos.md#step-co-07)
<a id="co-08"></a> [CO-08 详细卡](roadmap/companyos.md#step-co-08)
<a id="co-09"></a> [CO-09 详细卡](roadmap/companyos.md#step-co-09)
<a id="co-10"></a> [CO-10 详细卡](roadmap/companyos.md#step-co-10)
<a id="co-11"></a> [CO-11 详细卡](roadmap/companyos.md#step-co-11)
<a id="co-12"></a> [CO-12 详细卡](roadmap/companyos.md#step-co-12)
<a id="co-13"></a> [CO-13 详细卡](roadmap/companyos.md#step-co-13)
<a id="co-14"></a> [CO-14 详细卡](roadmap/companyos.md#step-co-14)
<a id="co-15"></a> [CO-15 详细卡](roadmap/companyos.md#step-co-15)
<a id="co-16"></a> [CO-16 详细卡](roadmap/companyos.md#step-co-16)
<a id="co-17"></a> [CO-17 详细卡](roadmap/companyos.md#step-co-17)
<a id="co-18"></a> [CO-18 详细卡](roadmap/companyos.md#step-co-18)
<a id="co-19"></a> [CO-19 详细卡](roadmap/companyos.md#step-co-19)
<a id="co-20"></a> [CO-20 详细卡](roadmap/companyos.md#step-co-20)
<a id="co-21"></a> [CO-21 详细卡](roadmap/companyos.md#step-co-21)
<a id="co-22"></a> [CO-22 详细卡](roadmap/companyos.md#step-co-22)
<a id="co-23"></a> [CO-23 详细卡](roadmap/companyos.md#step-co-23)
<a id="co-24"></a> [CO-24 详细卡](roadmap/companyos.md#step-co-24)
<a id="co-25"></a> [CO-25 详细卡](roadmap/companyos.md#step-co-25)
<a id="co-26"></a> [CO-26 详细卡](roadmap/companyos.md#step-co-26)
<a id="co-27"></a> [CO-27 详细卡](roadmap/companyos.md#step-co-27)
<a id="co-28"></a> [CO-28 详细卡](roadmap/companyos.md#step-co-28)
<a id="co-29"></a> [CO-29 详细卡](roadmap/companyos.md#step-co-29)
<a id="co-30"></a> [CO-30 详细卡](roadmap/companyos.md#step-co-30)
<a id="co-31"></a> [CO-31 详细卡](roadmap/companyos.md#step-co-31)
<a id="co-32"></a> [CO-32 详细卡](roadmap/companyos.md#step-co-32)
<a id="co-33"></a> [CO-33 详细卡](roadmap/companyos.md#step-co-33)
<a id="co-34"></a> [CO-34 详细卡](roadmap/companyos.md#step-co-34)
<a id="co-35"></a> [CO-35 详细卡](roadmap/companyos.md#step-co-35)
<a id="co-36"></a> [CO-36 详细卡](roadmap/companyos.md#step-co-36)
<a id="co-37"></a> [CO-37 详细卡](roadmap/companyos.md#step-co-37)
<a id="co-38"></a> [CO-38 详细卡](roadmap/companyos.md#step-co-38)
<a id="co-39"></a> [CO-39 详细卡](roadmap/companyos.md#step-co-39)
<a id="co-40"></a> [CO-40 详细卡](roadmap/companyos.md#step-co-40)
<a id="co-41"></a> [CO-41 详细卡](roadmap/companyos.md#step-co-41)
<a id="co-42"></a> [CO-42 详细卡](roadmap/companyos.md#step-co-42)
<a id="co-43"></a> [CO-43 详细卡](roadmap/companyos.md#step-co-43)
<a id="co-44"></a> [CO-44 详细卡](roadmap/companyos.md#step-co-44)
<a id="co-45"></a> [CO-45 详细卡](roadmap/companyos.md#step-co-45)
<a id="co-46"></a> [CO-46 详细卡](roadmap/companyos.md#step-co-46)
<a id="co-47"></a> [CO-47 详细卡](roadmap/companyos.md#step-co-47)
<a id="co-48"></a> [CO-48 详细卡](roadmap/companyos.md#step-co-48)

---

## 19. 本追加范围的验证、证据与执行交接

> 本专项已拆到 [CompanyOS 组织与业务专项](roadmap/companyos.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 20. Capability 专项：调研结论与实现设计（2026-09-12 追加）

> 本专项已拆到 [Capability 专项](roadmap/capability.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 21. Capability 详细实施 step

> 本专项已拆到 [Capability 专项](roadmap/capability.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 22. Capability 验收矩阵、执行方式与证据

> 本专项已拆到 [Capability 专项](roadmap/capability.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 23. Event / Receipt / Recovery 专项：事实、收据与恢复的实际设计（2026-09-12 追加）

> 本专项已拆到 [Event / Receipt / Recovery 专项](roadmap/event-receipt-recovery.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

<a id="context-memory-design"></a>

## 24. Context / Memory 专项：代码设计与完整处理流程（2026-09-12 追加）

> 本专项已拆到 [Context / Memory 专项](roadmap/context-memory.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 25. Skills / Plugins / Hooks 专项：调研结论与实现路线（2026-09-12 追加）

> 本专项已拆到 [Skills / Plugins / Hooks 专项](roadmap/skills-plugins-hooks.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 26. UI / Entrypoints 专项：用户入口实际设计与处理流程（2026-09-12 追加）

> 本专项已拆到 [UI / Entrypoints 专项](roadmap/ui-entrypoints.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 27. UI / Entrypoints 详细实施 steps（UI-00–UI-41）

> 本专项已拆到 [UI / Entrypoints 专项](roadmap/ui-entrypoints.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---

## 28. UI / Entrypoints 执行批次、验证命令与证据规则

> 本专项已拆到 [UI / Entrypoints 专项](roadmap/ui-entrypoints.md)。本页保留入口和原编号，详细设计、处理流程、实施卡与验证规则请打开独立文档。

---
