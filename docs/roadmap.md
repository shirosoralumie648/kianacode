# Kiana 执行路线图（P0–P6 执行骨架 + 进度）

> **一屏看进度** → §1 总图。**按顺序逐条执行** → §1.1 的 749 张 Step 总队列；每行都有明确前置、状态和详细卡链接。**看设计说明** → 专项设计导航。**看基础卡** → §4–§8。**你想加东西** → §11 追加区。
> 当前事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文只排顺序、记进度、写验收口径，**不定义新规范**。
> 阶段编号以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 的 P0–P6 为唯一 canonical。
> 单元清单来源：`company-os-implementation-outline.md` §3 的切片 A–M 与子切片 J1–M7（共 38 个）；产品特有单元（角色目录、会议、六层记忆、提示词来源）另见 `COMPANY.md` §3/§4/§5/§7。
> 本次核对：2026-09-14；源码基线仍以 `CURRENT_STATUS.md` 中绑定的快照为准，文档变更不提升任何源码能力的证明等级。共享工作树另有持续变化的 WIP，不能套用历史 CI。当前窗口见 §2，核对证据见 `CURRENT_STATUS.md`「Roadmap source reconciliation evidence (2026-09-12)」。

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
| `P0-A-01b` | P0 | A 契约注册表 | `P0-A-01a` | schema 注册表；unknown field / unknown event / migration 规则 | ✅ |
| `P0-A-02` | P0 | A 契约注册表 | `P0-A-01a` | `CapabilityErrorCode` + `failure_code()`，每码有 CLI exit / HTTP status / 可重试映射 | ✅ |
| `P0-B-01` | P0 | B 正式状态机 | `P0-A-01a` | Cell/WorkPacket/CapabilityExecution/Approval 四张转移表；非法转移与重复请求有断言 | ✅ |
| `P0-F-01` | P0 | F Approval | `P0-B-01` | TTY/Web/一次性 CLI 三处可列举同一 pending 并回复 | ⏳ |
| `P0-F-02` | P0 | F Approval | `P0-F-01` | 每次批/拒都有 durable 记录；重复消费与过期被拒 | ⏳ |
| `P0-F-03` | P0 | F Approval | `P0-G-02b`、`P0-G-03`、`P0-F-02` | 重启默认暂停；显式恢复重新过授权，续跑同一 Runner；缺材料 fail-closed | ⏳ |
| `P0-G-01` | P0 | G 事实源与恢复 | — | 内存未命中时只读回读重建；账本无记录仍 fail-closed | ✅ |
| `P0-G-02a` | P0 | G 事实源与恢复 | `P0-G-01` | `run.prompt`/`run.tool_call` 落账并过 `redact_event_value` | ✅ |
| `P0-G-02b` | P0 | G 事实源与恢复 | `P0-G-02a` | 只读折叠函数可从 `run.*`/`capability.*` 重建 model-visible history | ✅ |
| `P0-G-03` | P0 | G 事实源与恢复 | `P0-G-02b` | additive `ResumeRequest`，`PROTOCOL_SCHEMA` 不动，复用同一 `drive_run` | ⏳ |
| `P0-G-04` | P0 | G 事实源与恢复 | `P0-G-01` | 新进程仅凭事件重建 Run/Invocation；矛盾终态 fail-closed | ✅ |
| `P0-J1-01` | P0 | J1 Runtime | `P0-B-01` | `RunCancellationState` + 转移表；`ExecutionStatus` 补 `Queued`/`Cancelling`；每 run 恰好一条终态 | ⏳ |
| `P0-J1-02` | P0 | J1 Runtime | `P0-J1-01` | queued tool calls 排空并合成 replay-safe 结果 | ⏳ |
| `P0-J1-03` | P0 | J1 Runtime | `P0-J1-01` | 取消路径确认进程组停止；无法确认进 `result_unknown` | ⏳ |
| `P0-J1-04` | P0 | J1 Runtime | `P0-J1-01`–`03` | 保留 `cancelling_mid_stream_never_completes_or_emits_a_late_delta` 语义 | ⏳ |
| `P0-J1-05a` | P0 | J1 Runtime | — | 重复工具调用与 run 级 wall-time 预算 fail-closed，阈值进 `RuntimeConfig` 且在产品路径生效 | ✅ |
| `P0-J1-05b` | P0 | J1 Runtime | `P0-J1-05a` | 角色步数经 ControlPlane 命令在 harness 生效；环境覆盖、run 间隔离与原有 wall-time 均有行为断言 | ✅ |
| `P0-J7-01` | P0 | J7 Provider/Output | — | 账本粒度、不完整流 fail-closed、默认开启均已落地并有证据块 | ✅ |
| `P0-K1-01` | P0 | K1 Identity | `P0-A-01a` | 由受保护入口解析身份；服务端从不可变 assignment 派生 role/department | ✅ |
| `P0-M1-01` | P0 | M1 Workbench | — | CLI/TTY/Web/Desktop 对同一 run 的 terminal state 一致 | ⏳ |
| `P1-C-01` | P1 | C 组织与 Cell | `P0-A-01a` | 六类组织契约定义齐备；子权限只减不增 | ✅ |
| `P1-C-02` | P1 | C 组织与 Cell | `P1-C-01` | reserve→commit→terminal→retire 全链；retire 撤销 grant、释放锁与预算 | ⏳ |
| `P1-C-03` | P1 | C 组织与 Cell | `P1-C-01` | 五部门 × 角色 RoleSpec 数据集；`model_profile` 到达 provider 路由 | ✅ |
| `P1-D-01` | P1 | D WorkPacket | `P0-A-01a` | `ready_packets(graph, now)` 单实现；三处调用结果一致 | ✅ |
| `P1-D-02` | P1 | D WorkPacket | `P1-D-01` | `validate_dependency_dag` 输出确定性规范化环；缺依赖不推进状态 | ✅ |
| `P1-D-03` | P1 | D WorkPacket | `P1-D-01` | 过期 lease 退回 ready 并记事件；worker 死亡后可回收且不重复派发 | ✅ |
| `P1-E-01` | P1 | E 通信与问责 | `P0-B-01` | 七类消息分离；Handoff 必须定向并 ACK | ✅ |
| `P1-E-02` | P1 | E 通信与问责 | `P1-E-01` | 现有 symposium 会议路径有验收测试；决定事件 durable 可重放 | ⏳ |
| `P1-H-01` | P1 | H Capability/Broker | `P0-A-01a` | 工具权威单一真源；不新增模型可见工具，保持 5 个 | ✅ |
| `P1-H-02` | P1 | H Capability/Broker | — | 映射期拒绝非法参数；`additionalProperties` 不默认禁止 | ✅ |
| `P1-H-03` | P1 | H Capability/Broker | `P1-H-01` | 所有副作用工具共用同一 containment | ✅ |
| `P1-J2-01` | P1 | J2 Context/Cache | `P0-G-04` | `PromptSection{name, order, text}` + `render_prompt()` + provenance | ⏳ |
| `P1-J2-02` | P1 | J2 Context/Cache | `P1-J2-01` | `TokenBudget` 计入 tool schemas 与 system prompt；越界 fail-closed | ⏳ |
| `P1-J2-03` | P1 | J2 Context/Cache | `P1-J2-01` | `RoleSpec.prompt` 进入 provider 的 system message | ⏳ |
| `P1-J2-04` | P1 | J2 Context/Cache | `P1-J2-03` | 角色 prompt 从角色包加载；`prompt_hash` 进收据可复现 | ⏳ |
| `P1-J3-01` | P1 | J3 Memory | `P0-A-01a` | 模型写入一律 candidate+draft；`origin` 服务端派生；默认检索排除 | ✅ |
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
| `P3-I-01` | P3 | I Company 生命周期 | `P0-A-01a` | 十类业务对象定义与不变量 | ✅ |
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

### 1.1 全量 Step 总图：依赖波次（749 张）

> 这里把基础路线图 74 张验收卡、既有专项 329 张实施卡，以及本轮新增的 `CI-*`、`SW-*`、`OA-*`、`AUT-*`、`NM-*`、`PD-*`、`INT-*`、`EQ-*`、`BQ-*`、`DEP-*`、`SC-*` 346 张实施卡统一登记，共 749 张可逐条领取的 Step。基础卡是阶段验收口径，专项卡是实现拆分；两者有意重叠，749 不是 749 份独立功能。
>
> 本表是唯一的逐步执行队列：所有 Step 都在这里出现，按依赖拓扑和 W0–W11 波次排序；同一波次中原本可并行的卡也按表内顺序串行化，方便 agent 一个接一个领取。专项设计导航和独立文件只承载设计说明与详细验收，不再拥有另一套执行顺序。

**排序原则**：先固定事实基线和已验证基础，再收口当前 wall-time/role-limit 与共享契约；随后建立权威账本、运行时、执行和恢复；再推进跨模块编排与 CompanyOS；最后做入口一致性、后置平台扩展和发布门。`W0–W11` 只是建议执行波次，不是新的 P 阶段或完成状态。

| 波次 | 名称 | 登记项 | 说明 |
|---|---|---:|---|
| W0 | 已有证据与当前收口 | 29 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W1 | 共享契约与身份 | 93 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W2 | 事实账本、授权与资源 | 92 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W3 | 实际执行、取消与恢复 | 91 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W4 | 上下文、记忆与扩展 | 82 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W5 | Provider 协议与调用链 | 29 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W6 | 投影与可用入口 | 88 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W7 | CompanyOS 业务闭环 | 80 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W8 | 并行、治理与复用 | 39 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W9 | 离线联合验收 | 87 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W10 | 后置平台与能力扩展 | 16 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |
| W11 | 真实环境与发布证据 | 23 | 只有明确前置完成后才进入下一波；同波按表内顺序执行 |

**依赖字段说明**：表中的“前置”是把各专项卡明确写出的硬依赖展开成已登记的 Step ID；`CP-04/08/13`、`PD-01..03` 这类缩写已展开，`P0-J1`、`CM` 等模块级引用只映射到对应的合同/门，不把整模块隐式设为前置。Event/Receipt、Context/Memory 的线性项遵循各专项已经发布的保守顺序；评测中原可并行的分组按本表顺序串行化，便于 agent 逐条领取。

**Agent 领取规则**：从上到下找到第一条“所有前置均为 ✅”且自身不是 ✅ 的行，只领取这一条；完成后按 `CURRENT_STATUS.md` 的证据块更新状态，再领取下一条。`⏳`、`🔄` 和 `⚠️` 都不能被当作已满足前置；没有前置的基线卡也必须先完成盘点和证据记录。

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
| 008 | W0 | 基础 | [`P0-J1-05a`](#step-p0-j1-05a) | P0 基础 · 重复调用检测与 wall-time 预算接线 | — | ✅ | [基础卡](#step-p0-j1-05a) |
| 009 | W0 | 基础 | [`P0-J1-05b`](#step-p0-j1-05b) | P0 基础 · 按角色的 max_steps | `P0-J1-05a` | ✅ | [基础卡](#step-p0-j1-05b) |
| 010 | W0 | 基础 | [`P1-J3-01`](#step-p1-j3-01) | P1 基础 · Memory 写入候选制 | `P0-A-01a` | ✅ | [基础卡](#step-p1-j3-01) |
| 011 | W0 | 专项 | [`CP-00`](roadmap/control-plane.md#step-cp-00) | ControlPlane · 固定基线，列出所有有后果的入口 | — | ✅ | [专项卡](roadmap/control-plane.md#step-cp-00) |
| 012 | W0 | 专项 | [`ER-00`](roadmap/event-receipt-recovery.md#step-er-00) | Event / Receipt / Recovery · 固定基线与事实边界 | — | ✅ | [专项卡](roadmap/event-receipt-recovery.md#step-er-00) |
| 013 | W0 | 专项 | [`CAP-00`](roadmap/capability.md#step-cap-00) | Capability · 固定可复核基线，消除计划与 WIP 重叠 | — | ✅ | [专项卡](roadmap/capability.md#step-cap-00) |
| 014 | W0 | 专项 | [`H01`](roadmap/harness.md#step-h01) | Harness · 固定接线基线与可执行验收骨架 | — | ✅ | [专项卡](roadmap/harness.md#step-h01) |
| 015 | W0 | 专项 | [`P4-J7-04`](roadmap/provider.md#step-p4-j7-04) | Provider · Provider 基线、快照和现有测试 | `P0-J7-01` | ✅ | [专项卡](roadmap/provider.md#step-p4-j7-04) |
| 016 | W0 | 专项 | [`CM-00`](roadmap/context-memory.md#step-cm-00) | Context / Memory · 固定源码快照、差异与证据边界 | — | ✅ | [专项卡](roadmap/context-memory.md#step-cm-00) |
| 017 | W0 | 专项 | [`EXT-00`](roadmap/skills-plugins-hooks.md#step-ext-00) | Skills / Plugins / Hooks · 基线与决策回执 | — | ✅ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-00) |
| 018 | W0 | 专项 | [`UI-00`](roadmap/ui-entrypoints.md#step-ui-00) | UI / Entrypoints · 建立入口基线与验收矩阵 | — | ✅ | [专项卡](roadmap/ui-entrypoints.md#step-ui-00) |
| 019 | W0 | 专项 | [`CO-01`](roadmap/companyos.md#step-co-01) | CompanyOS · 锁定当前实现与计划的交接基线 | — | ✅ | [专项卡](roadmap/companyos.md#step-co-01) |
| 020 | W0 | 基础 | [`P0-G-04`](#step-p0-g-04) | P0 基础 · 事件重建投影 | `P0-G-01` | ✅ | [基础卡](#step-p0-g-04) |
| 021 | W0 | 专项 | [`CI-01`](#step-ci-01) | 基线盘点与迁移护栏；`docs/schemas`、`kiana-daemon/model_client.rs`、`kiana-provider/config.rs` | — | ✅ | [专项卡](#step-ci-01) |
| 022 | W0 | 专项 | [`SW-00`](#step-sw-00) | 现状 reconciliation；`CURRENT_STATUS.md`、`kiana-domain/{swarm,packet_graph,work_packets}.rs`、`kiana-core/{swarm,cell_registry,collaboration}.rs`、`kiana-ports`、daemon tests | — | ✅ | [专项卡](#step-sw-00) |
| 023 | W0 | 专项 | [`OA-00`](#step-oa-00) | 基线与信号 inventory；`module-map.md`、`CURRENT_STATUS.md`、`kiana-core/events.rs`、`kiana-eventlog/*`、现有 `ER-30`/`P1-J8-01` | — | ✅ | [专项卡](#step-oa-00) |
| 024 | W0 | 专项 | [`AUT-01`](#step-aut-01) | 基线与迁移护栏；盘点 `module-map`、现有 automation tests、`CURRENT_STATUS`，记录旧 `watch_scheduled_tasks` 兼容边界 | — | ✅ | [专项卡](#step-aut-01) |
| 025 | W0 | 专项 | [`NM-00`](#step-nm-00) | 现状/事件种类/入口 inventory；`run_stream.rs`、`platform.rs`、`web.rs`、`workbench_chat.rs`、`CURRENT_STATUS.md` | — | ✅ | [专项卡](#step-nm-00) |
| 026 | W0 | 专项 | [`EQ-00`](#step-eq-00) | 固定当前源码快照、工作树状态和现有 `eval` 行为；在 `docs/roadmap.md`/`CURRENT_STATUS.md` 建立本专项证据模板 | — | ✅ | [专项卡](#step-eq-00) |
| 027 | W0 | 专项 | [`PD-00`](roadmap/persistence-data-layer.md#step-pd-00) | 固定数据层基线、事实/投影/Artifact/Memory/Index/Approval 现状；`docs/roadmap*`、`CURRENT_STATUS.md` | — | ✅ | [专项卡](roadmap/persistence-data-layer.md#step-pd-00) |
| 028 | W0 | 专项 | [`INT-00`](roadmap/integrations-connectors.md#step-int-00) | 基线、现状和能力矩阵；`docs/`、`CURRENT_STATUS.md`、现有 connector tests | — | ✅ | [专项卡](roadmap/integrations-connectors.md#step-int-00) |
| 029 | W0 | 专项 | [`SC-00`](roadmap/security-compliance.md#step-sc-00) | CURRENT_STATUS.md、docs/module-map.md、专项 manifest | — | ✅ | [专项卡](roadmap/security-compliance.md#step-sc-00) |
| **W1** | **共享契约与身份** |  |  |  |  |  |  |
| 030 | W1 | 专项 | [`CP-01`](roadmap/control-plane.md#step-cp-01) | ControlPlane · 服务端主体与项目身份 | `CP-00` | ✅ | [专项卡](roadmap/control-plane.md#step-cp-01) |
| 031 | W1 | 专项 | [`CP-02`](roadmap/control-plane.md#step-cp-02) | ControlPlane · 使用既有 ID，明确状态机和 Continue/Resume | `CP-00` | ✅ | [专项卡](roadmap/control-plane.md#step-cp-02) |
| 032 | W1 | 专项 | [`CP-03`](roadmap/control-plane.md#step-cp-03) | ControlPlane · 规范化 action，风险与执行元数据服务端所有 | `CP-01`、`CP-02` | ✅ | [专项卡](roadmap/control-plane.md#step-cp-03) |
| 033 | W1 | 专项 | [`CP-04`](roadmap/control-plane.md#step-cp-04) | ControlPlane · 权限交集与单调决策在 core 强制 | `CP-03` | ✅ | [专项卡](roadmap/control-plane.md#step-cp-04) |
| 034 | W1 | 专项 | [`CP-05`](roadmap/control-plane.md#step-cp-05) | ControlPlane · 合并三条授权执行路径 | `CP-04` | ✅ | [专项卡](roadmap/control-plane.md#step-cp-05) |
| 035 | W1 | 专项 | [`CP-06`](roadmap/control-plane.md#step-cp-06) | ControlPlane · 定义原子状态转移端口 | `CP-02`、`CP-04` | ✅ | [专项卡](roadmap/control-plane.md#step-cp-06) |
| 036 | W1 | 专项 | [`ER-01`](roadmap/event-receipt-recovery.md#step-er-01) | Event / Receipt / Recovery · 事件 schema、kind registry 与迁移规则 | `ER-00` | ✅ | [专项卡](roadmap/event-receipt-recovery.md#step-er-01) |
| 037 | W1 | 专项 | [`ER-02`](roadmap/event-receipt-recovery.md#step-er-02) | Event / Receipt / Recovery · 统一身份、关联和顺序语义 | `ER-01` | ✅ | [专项卡](roadmap/event-receipt-recovery.md#step-er-02) |
| 038 | W1 | 专项 | [`ER-03`](roadmap/event-receipt-recovery.md#step-er-03) | Event / Receipt / Recovery · 事件边界脱敏和 Artifact 引用 | `ER-02` | ✅ | [专项卡](roadmap/event-receipt-recovery.md#step-er-03) |
| 039 | W1 | 专项 | [`ER-04`](roadmap/event-receipt-recovery.md#step-er-04) | Event / Receipt / Recovery · CommandReceipt 与 transition read-set | `ER-03` | ✅ | [专项卡](roadmap/event-receipt-recovery.md#step-er-04) |
| 040 | W1 | 专项 | [`CAP-01`](roadmap/capability.md#step-cap-01) | Capability · descriptor、schema、policy metadata 与 handler binding 单一来源 | `CAP-00` | ✅ | [专项卡](roadmap/capability.md#step-cap-01) |
| 041 | W1 | 专项 | [`CAP-02`](roadmap/capability.md#step-cap-02) | Capability · 统一参数边界与输入摘要 | `CAP-01` | ✅ | [专项卡](roadmap/capability.md#step-cap-02) |
| 042 | W1 | 专项 | [`CAP-03`](roadmap/capability.md#step-cap-03) | Capability · 从 authority chain 派生不可变 ExecutionScope | `CAP-02` | ✅ | [专项卡](roadmap/capability.md#step-cap-03) |
| 043 | W1 | 专项 | [`CAP-04`](roadmap/capability.md#step-cap-04) | Capability · 状态与 outcome 不再依赖字符串猜测 | `CAP-02` | ✅ | [专项卡](roadmap/capability.md#step-cap-04) |
| 044 | W1 | 专项 | [`H02`](roadmap/harness.md#step-h02) | Harness · Session / Run / Turn / Step 的身份与生命周期 | `H01` | ✅ | [专项卡](roadmap/harness.md#step-h02) |
| 045 | W1 | 专项 | [`H03`](roadmap/harness.md#step-h03) | Harness · 将 KianaHarness 收敛为单一状态驱动器 | `H02` | ✅ | [专项卡](roadmap/harness.md#step-h03) |
| 046 | W1 | 专项 | [`H04`](roadmap/harness.md#step-h04) | Harness · 结构化模型消息与无损 Provider 转换 | `H03` | ✅ | [专项卡](roadmap/harness.md#step-h04) |
| 047 | W1 | 专项 | [`H05`](roadmap/harness.md#step-h05) | Harness · 统一停止原因、错误与重试分类 | `H04` | ✅ | [专项卡](roadmap/harness.md#step-h05) |
| 048 | W1 | 专项 | [`P4-J7-05`](roadmap/provider.md#step-p4-j7-05) | Provider · 非流式工具响应必须严格解析 | `P4-J7-04` | ✅ | [专项卡](roadmap/provider.md#step-p4-j7-05) |
| 049 | W1 | 专项 | [`CM-01`](roadmap/context-memory.md#step-cm-01) | Context / Memory · 建立共享来源与 scope 值对象 | `CM-00` | ✅ | [专项卡](roadmap/context-memory.md#step-cm-01) |
| 050 | W1 | 专项 | [`CM-02`](roadmap/context-memory.md#step-cm-02) | Context / Memory · 统一 MemoryRecord 生命周期与兼容导入 | `CM-01` | ✅ | [专项卡](roadmap/context-memory.md#step-cm-02) |
| 051 | W1 | 专项 | [`CM-03`](roadmap/context-memory.md#step-cm-03) | Context / Memory · 服务端派生 read/write scope 与 purpose | `CM-02` | ✅ | [专项卡](roadmap/context-memory.md#step-cm-03) |
| 052 | W1 | 专项 | [`CM-04`](roadmap/context-memory.md#step-cm-04) | Context / Memory · 统一 Memory mutation 与幂等键 | `CM-03` | ✅ | [专项卡](roadmap/context-memory.md#step-cm-04) |
| 053 | W1 | 专项 | [`EXT-01`](roadmap/skills-plugins-hooks.md#step-ext-01) | Skills / Plugins / Hooks · 稳定扩展领域合同 | `EXT-00` | ✅ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-01) |
| 054 | W1 | 专项 | [`EXT-02`](roadmap/skills-plugins-hooks.md#step-ext-02) | Skills / Plugins / Hooks · SourceResolver、ProjectTrust 与路径根 | `EXT-01` | ✅ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-02) |
| 055 | W1 | 专项 | [`EXT-03`](roadmap/skills-plugins-hooks.md#step-ext-03) | Skills / Plugins / Hooks · 严格解析器与兼容层 | `EXT-02` | ✅ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-03) |
| 056 | W1 | 专项 | [`EXT-04`](roadmap/skills-plugins-hooks.md#step-ext-04) | Skills / Plugins / Hooks · Catalog、优先级、重复和可解释性 | `EXT-03` | ✅ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-04) |
| 057 | W1 | 专项 | [`EXT-05`](roadmap/skills-plugins-hooks.md#step-ext-05) | Skills / Plugins / Hooks · 快照与失效 | `EXT-04` | ✅ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-05) |
| 058 | W1 | 专项 | [`UI-01`](roadmap/ui-entrypoints.md#step-ui-01) | UI / Entrypoints · 定义 versioned UI protocol DTO | `UI-00` | ✅ | [专项卡](roadmap/ui-entrypoints.md#step-ui-01) |
| 059 | W1 | 专项 | [`UI-02`](roadmap/ui-entrypoints.md#step-ui-02) | UI / Entrypoints · 统一错误、能力和 surface handshake | `UI-01` | ✅ | [专项卡](roadmap/ui-entrypoints.md#step-ui-02) |
| 060 | W1 | 专项 | [`UI-03`](roadmap/ui-entrypoints.md#step-ui-03) | UI / Entrypoints · 本地实例身份、发现和 transport | `UI-01`、`UI-02` | ✅ | [专项卡](roadmap/ui-entrypoints.md#step-ui-03) |
| 061 | W1 | 专项 | [`CO-02`](roadmap/companyos.md#step-co-02) | CompanyOS · 稳定组织、业务项目与工作区绑定 | `CO-01` | ✅ | [专项卡](roadmap/companyos.md#step-co-02) |
| 062 | W1 | 专项 | [`CO-03`](roadmap/companyos.md#step-co-03) | CompanyOS · 角色任命、有效期与撤销接入服务端身份 | `CO-02` | ✅ | [专项卡](roadmap/companyos.md#step-co-03) |
| 063 | W1 | 专项 | [`CO-04`](roadmap/companyos.md#step-co-04) | CompanyOS · 五部门与专业岗位成为版本化目录 | `CO-03` | ✅ | [专项卡](roadmap/companyos.md#step-co-04) |
| 064 | W1 | 专项 | [`CO-05`](roadmap/companyos.md#step-co-05) | CompanyOS · 业务命令权限、责任和人工决定合同 | `CO-03`、`CO-04` | ✅ | [专项卡](roadmap/companyos.md#step-co-05) |
| 065 | W1 | 专项 | [`CO-06`](roadmap/companyos.md#step-co-06) | CompanyOS · 不可变工件、Evidence 与 Criterion 引用合同 | `CO-02`、`CO-05` | ✅ | [专项卡](roadmap/companyos.md#step-co-06) |
| 066 | W1 | 专项 | [`CO-07`](roadmap/companyos.md#step-co-07) | CompanyOS · 版本化业务事实与稳定命令回执 | `CO-05`、`CO-06` | ✅ | [专项卡](roadmap/companyos.md#step-co-07) |
| 067 | W1 | 专项 | [`CO-08`](roadmap/companyos.md#step-co-08) | CompanyOS · 业务状态机、历史重放与兼容迁移 | `CO-07` | ✅ | [专项卡](roadmap/companyos.md#step-co-08) |
| 068 | W1 | 基础 | [`P0-A-01b`](#step-p0-a-01b) | P0 基础 · schema 注册表与 unknown field/migration 规则 | `P0-A-01a` | ✅ | [基础卡](#step-p0-a-01b) |
| 069 | W1 | 基础 | [`P0-A-02`](#step-p0-a-02) | P0 基础 · 稳定错误码枚举 | `P0-A-01a` | ✅ | [基础卡](#step-p0-a-02) |
| 070 | W1 | 专项 | [`P4-J7-06`](roadmap/provider.md#step-p4-j7-06) | Provider · 中立内容、调用身份、错误与模型端口 | `P4-J7-05`、`P0-A-01b`、`P0-A-02`、`CP-02` | ✅ | [专项卡](roadmap/provider.md#step-p4-j7-06) |
| 071 | W1 | 专项 | [`P4-J7-07`](roadmap/provider.md#step-p4-j7-07) | Provider · 提取 kiana-provider 并迁移装配 | `P4-J7-06` | ✅ | [专项卡](roadmap/provider.md#step-p4-j7-07) |
| 072 | W1 | 专项 | [`P4-J7-08`](roadmap/provider.md#step-p4-j7-08) | Provider · 连接、profile 和配置快照 | `P4-J7-07` | ✅ | [专项卡](roadmap/provider.md#step-p4-j7-08) |
| 073 | W1 | 专项 | [`P4-J7-09`](roadmap/provider.md#step-p4-j7-09) | Provider · 凭据管理与 HTTP 目标校验 | `P4-J7-08` | ✅ | [专项卡](roadmap/provider.md#step-p4-j7-09) |
| 074 | W1 | 专项 | [`P4-J7-10`](roadmap/provider.md#step-p4-j7-10) | Provider · 能力目录、未知能力和显式 discovery | `P4-J7-08` | ✅ | [专项卡](roadmap/provider.md#step-p4-j7-10) |
| 075 | W1 | 基础 | [`P0-B-01`](#step-p0-b-01) | P0 基础 · 正式状态机转移表 | `P0-A-01a` | ✅ | [基础卡](#step-p0-b-01) |
| 076 | W1 | 基础 | [`P0-K1-01`](#step-p0-k1-01) | P0 基础 · 服务端身份与 authority epoch | `P0-A-01a` | ✅ | [基础卡](#step-p0-k1-01) |
| 077 | W1 | 基础 | [`P1-C-01`](#step-p1-c-01) | P1 基础 · 组织与 Cell 契约 | `P0-A-01a` | ✅ | [基础卡](#step-p1-c-01) |
| 078 | W1 | 基础 | [`P1-C-03`](#step-p1-c-03) | P1 基础 · 五部门角色目录与 model_profile 接线 | `P1-C-01` | ✅ | [基础卡](#step-p1-c-03) |
| 079 | W1 | 基础 | [`P1-D-01`](#step-p1-d-01) | P1 基础 · WorkPacket 单一 ready 谓词 | `P0-A-01a` | ✅ | [基础卡](#step-p1-d-01) |
| 080 | W1 | 基础 | [`P1-D-02`](#step-p1-d-02) | P1 基础 · 依赖缺失 / 成环 fail-closed | `P1-D-01` | ✅ | [基础卡](#step-p1-d-02) |
| 081 | W1 | 基础 | [`P1-E-01`](#step-p1-e-01) | P1 基础 · 通信与问责分层 | `P0-B-01` | ✅ | [基础卡](#step-p1-e-01) |
| 082 | W1 | 基础 | [`P1-H-01`](#step-p1-h-01) | P1 基础 · `ToolSpec` registry | `P0-A-01a` | ✅ | [基础卡](#step-p1-h-01) |
| 083 | W1 | 基础 | [`P1-H-03`](#step-p1-h-03) | P1 基础 · 路径 containment 共享实现 | `P1-H-01` | ✅ | [基础卡](#step-p1-h-03) |
| 084 | W1 | 基础 | [`P3-I-01`](#step-p3-i-01) | P3 基础 · Company 业务对象契约 | `P0-A-01a` | ✅ | [基础卡](#step-p3-i-01) |
| 085 | W1 | 专项 | [`CI-02`](#step-ci-02) | Domain 稳定 ID、Principal/Assignment/ProviderAccount/SecretRef/ConfigSnapshot/AuthoritySnapshot 合同；`kiana-domain` | `CI-01` | ✅ | [专项卡](#step-ci-02) |
| 086 | W1 | 专项 | [`CI-03`](#step-ci-03) | Ports 分层；`IdentityResolver`、`CredentialResolver`、`ConfigSnapshotStore`、`Rotation/Revoke`；`kiana-ports` | `CI-02` | ✅ | [专项卡](#step-ci-03) |
| 087 | W1 | 专项 | [`CI-04`](#step-ci-04) | 受保护 Daemon ingress 与本地主体迁移；`kiana-daemon`、`kiana-client`、`kiana-protocol` | `CI-02`、`CI-03` | ✅ | [专项卡](#step-ci-04) |
| 088 | W1 | 专项 | [`CI-05`](#step-ci-05) | Durable Membership/RoleAssignment/ProjectAssignment/PolicyProfile/DataBoundary/SharingGrant 与 authority epoch；`kiana-core`、`kiana-domain` | `CI-02`、`CI-04` | ✅ | [专项卡](#step-ci-05) |
| 089 | W1 | 专项 | [`SW-01`](#step-sw-01) | 稳定 ID、schema 和 lineage；domain + protocol + ports | `SW-00` | ✅ | [专项卡](#step-sw-01) |
| 090 | W1 | 专项 | [`SW-02`](#step-sw-02) | 显式 Partition/WorkGraph validator；复用 `packet_graph` | `SW-01` | ✅ | [专项卡](#step-sw-02) |
| 091 | W1 | 专项 | [`SW-03`](#step-sw-03) | Swarm/Partition/Attempt 状态 reducer 与 typed transition events；core/domain events | `SW-02` | ✅ | [专项卡](#step-sw-03) |
| 092 | W1 | 专项 | [`OA-01`](#step-oa-01) | Domain schema 注册；新增 `observability.v1`、`audit-record.v1`、`metric-catalog.v1`、`trace-summary.v1` 合同 | `OA-00` | ✅ | [专项卡](#step-oa-01) |
| 093 | W1 | 专项 | [`OA-02`](#step-oa-02) | `CorrelationContext`、TraceRef、SpanRef、causation/parent link；`kiana-domain`/`kiana-ports` | `OA-01` | ✅ | [专项卡](#step-oa-02) |
| 094 | W1 | 专项 | [`OA-03`](#step-oa-03) | 统一 `RedactionProfile`、classification、bounded value encoder；复用 `redact_event_value` 并补 span/log/metric/audit/export 边界 | `OA-01` | ✅ | [专项卡](#step-oa-03) |
| 095 | W1 | 专项 | [`OA-04`](#step-oa-04) | Audit taxonomy 与 `AuditRecord` reducer；`kiana-domain`/`kiana-core` | `OA-01`、`OA-03` | ✅ | [专项卡](#step-oa-04) |
| 096 | W1 | 专项 | [`OA-05`](#step-oa-05) | `ObservabilityPort`/`TraceSink`/`MetricSink`/`AuditQueryPort`/`HealthProbePort`；Memory/JSONL fake adapters | `OA-01`、`OA-04`、`OA-02`、`OA-03` | ✅ | [专项卡](#step-oa-05) |
| 097 | W1 | 专项 | [`NM-01`](#step-nm-01) | Domain contracts 与 schema registry；Message/Notification/Subscription/Attempt/ActionRef/DeliveryReceipt | `NM-00` | ⏳ | [专项卡](#step-nm-01) |
| 098 | W1 | 专项 | [`NM-02`](#step-nm-02) | 七类 `CommunicationMessage` 命令与生命周期；`kiana-domain`/`kiana-core` | `P1-E-01`、`NM-01` | ⏳ | [专项卡](#step-nm-02) |
| 099 | W1 | 专项 | [`NM-03`](#step-nm-03) | Event kind registry 与分类规则；`kiana-domain/contracts.rs`、`kiana-core/events.rs` | `ER-01`、`NM-01` | ⏳ | [专项卡](#step-nm-03) |
| 100 | W1 | 专项 | [`EQ-01`](#step-eq-01) | 从 `kiana-commands/src/eval.rs` 提取 schema 常量、错误码和 JSON 兼容测试清单，禁止无记录的字段删除 | `EQ-00` | ⏳ | [专项卡](#step-eq-01) |
| 101 | W1 | 专项 | [`EQ-02`](#step-eq-02) | 在 `kiana-domain/src/quality.rs` 加稳定 ID、digest、状态枚举和 `deny_unknown_fields` DTO | `EQ-01` | ⏳ | [专项卡](#step-eq-02) |
| 102 | W1 | 专项 | [`EQ-03`](#step-eq-03) | 定义 `EvalDataset`/`EvalSuite`/`EvalCase`/`GoldenTrace` schema、版本和 provenance | `EQ-02` | ⏳ | [专项卡](#step-eq-03) |
| 103 | W1 | 专项 | [`EQ-04`](#step-eq-04) | 定义 case split、privacy class、owner、expires_at、minimum sample 和 workload tags | `EQ-03` | ⏳ | [专项卡](#step-eq-04) |
| 104 | W1 | 专项 | [`EQ-05`](#step-eq-05) | 把 `kiana.eval-suite.v1`/baseline/report 与新 domain DTO 做显式 adapter，保留旧 CLI 输出字段 | `EQ-04` | ⏳ | [专项卡](#step-eq-05) |
| 105 | W1 | 专项 | [`EQ-06`](#step-eq-06) | 在 `kiana-protocol` 登记 `eval.run/capture/compare` 和 `quality.feedback/promote/rollback` 命令/事件 | `EQ-05` | ⏳ | [专项卡](#step-eq-06) |
| 106 | W1 | 专项 | [`EQ-07`](#step-eq-07) | 在 `kiana-ports` 增加 `EvalStore`、`FixtureStore`、`TraceSource`、`ArtifactReader`、`Judge`、`MetricsSink` | `EQ-06` | ⏳ | [专项卡](#step-eq-07) |
| 107 | W1 | 专项 | [`EQ-08`](#step-eq-08) | 建立 `tests/eval/` 目录、case manifest、fixture size/path/schema 限制和 deterministic loader | `EQ-07` | ⏳ | [专项卡](#step-eq-08) |
| 108 | W1 | 专项 | [`PD-01`](roadmap/persistence-data-layer.md#step-pd-01) | 定义 `StorageRoot`、`StoreIdentity`、owner scope、逻辑 namespace 和锁；`kiana-domain`、`kiana-daemon` | `PD-00` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-01) |
| 109 | W1 | 专项 | [`PD-02`](roadmap/persistence-data-layer.md#step-pd-02) | 建立 schema registry、canonical JSON/bytes、upcaster 和 unknown-field/major 规则；`kiana-domain`、`kiana-protocol` | `PD-00` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-02) |
| 110 | W1 | 专项 | [`PD-03`](roadmap/persistence-data-layer.md#step-pd-03) | 统一 `StorageError`、`StoreHealth`、`IntegrityIncident`、能力限制和错误码；`kiana-domain`、`kiana-ports` | `PD-01`、`PD-02` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-03) |
| 111 | W1 | 专项 | [`PD-04`](roadmap/persistence-data-layer.md#step-pd-04) | 下沉 `ProjectionStorePort`、`ArtifactStorePort`、`BackupStorePort`、`MigrationRunnerPort`、`RetentionStorePort`；`kiana-ports` | `PD-01`、`PD-02`、`PD-03` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-04) |
| 112 | W1 | 专项 | [`SC-01`](roadmap/security-compliance.md#step-sc-01) | docs threat register、security fixture catalog | `SC-00` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-01) |
| 113 | W1 | 专项 | [`SC-02`](roadmap/security-compliance.md#step-sc-02) | kiana-domain security IDs/schema registry | `SC-00` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-02) |
| 114 | W1 | 专项 | [`SC-03`](roadmap/security-compliance.md#step-sc-03) | kiana-domain/kiana-protocol reason codes | `SC-02` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-03) |
| 115 | W1 | 专项 | [`SC-04`](roadmap/security-compliance.md#step-sc-04) | kiana-core SecurityContext、入口身份边界 | `SC-02`、`SC-03` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-04) |
| 116 | W1 | 专项 | [`SC-05`](roadmap/security-compliance.md#step-sc-05) | kiana-policy PolicyBundle/DecisionTrace/PolicyRevision | `SC-03`、`SC-04` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-05) |
| 117 | W1 | 专项 | [`SC-06`](roadmap/security-compliance.md#step-sc-06) | kiana-domain/kiana-daemon Principal、session、authn adapter | `SC-04`、`SC-05` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-06) |
| 118 | W1 | 专项 | [`SC-07`](roadmap/security-compliance.md#step-sc-07) | kiana-core ProjectTrust、RoleAssignment、DepartmentSnapshot | `SC-06` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-07) |
| 119 | W1 | 专项 | [`SC-08`](roadmap/security-compliance.md#step-sc-08) | kiana-core authority epoch、session fence、policy refresh | `SC-06`、`SC-07` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-08) |
| 120 | W1 | 专项 | [`SC-09`](roadmap/security-compliance.md#step-sc-09) | kiana-policy GrantScope intersection、Cell inheritance | `SC-07`、`SC-08` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-09) |
| 121 | W1 | 专项 | [`SC-10`](roadmap/security-compliance.md#step-sc-10) | kiana-core Approval binding、Human Inbox | `SC-05`、`SC-08`、`SC-09` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-10) |
| 122 | W1 | 专项 | [`SC-11`](roadmap/security-compliance.md#step-sc-11) | kiana-entrypoints、scheduler/workflow/swarm/connector parity | `SC-04`、`SC-09`、`SC-10` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-11) |
| **W2** | **事实账本、授权与资源** |  |  |  |  |  |  |
| 123 | W2 | 专项 | [`CP-07`](roadmap/control-plane.md#step-cp-07) | ControlPlane · 实现 JSONL 事务帧及失败恢复 | `CP-06` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-07) |
| 124 | W2 | 专项 | [`CP-08`](roadmap/control-plane.md#step-cp-08) | ControlPlane · Grant 账本与 authority epoch | `CP-01`、`CP-04`、`CP-07` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-08) |
| 125 | W2 | 专项 | [`CP-09`](roadmap/control-plane.md#step-cp-09) | ControlPlane · 精确审批 subject 与可恢复材料 | `CP-03`、`CP-07`、`CP-08` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-09) |
| 126 | W2 | 专项 | [`CP-10`](roadmap/control-plane.md#step-cp-10) | ControlPlane · 审批决定、单次消费和 pending 持久化 | `CP-05`、`CP-07`、`CP-09` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-10) |
| 127 | W2 | 专项 | [`CP-11`](roadmap/control-plane.md#step-cp-11) | ControlPlane · 模型与工具统一消耗预算 | `CP-02`、`CP-07`、`CP-08` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-11) |
| 128 | W2 | 专项 | [`CP-12`](roadmap/control-plane.md#step-cp-12) | ControlPlane · 路径锁、资源租约和 fencing token | `CP-07`、`CP-08` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-12) |
| 129 | W2 | 专项 | [`CP-13`](roadmap/control-plane.md#step-cp-13) | ControlPlane · 执行许可与派发线性化点 | `CP-05`、`CP-10`、`CP-11`、`CP-12` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-13) |
| 130 | W2 | 专项 | [`CP-14`](roadmap/control-plane.md#step-cp-14) | ControlPlane · 统一执行结果、核销和结果回灌 | `CP-13` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-14) |
| 131 | W2 | 专项 | [`ER-05`](roadmap/event-receipt-recovery.md#step-er-05) | Event / Receipt / Recovery · JSONL v2 原子 frame、锁与损坏策略 | `ER-04` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-05) |
| 132 | W2 | 专项 | [`ER-06`](roadmap/event-receipt-recovery.md#step-er-06) | Event / Receipt / Recovery · 异步写入、背压与 shutdown ack | `ER-05` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-06) |
| 133 | W2 | 专项 | [`ER-07`](roadmap/event-receipt-recovery.md#step-er-07) | Event / Receipt / Recovery · 通用 replay reader 和 projector checkpoint | `ER-06` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-07) |
| 134 | W2 | 专项 | [`ER-08`](roadmap/event-receipt-recovery.md#step-er-08) | Event / Receipt / Recovery · Run/Turn 状态投影和 terminal 约束 | `ER-07` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-08) |
| 135 | W2 | 专项 | [`ER-09`](roadmap/event-receipt-recovery.md#step-er-09) | Event / Receipt / Recovery · Invocation/Execution/Attempt 投影 | `ER-08` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-09) |
| 136 | W2 | 专项 | [`ER-10`](roadmap/event-receipt-recovery.md#step-er-10) | Event / Receipt / Recovery · Approval、Budget、Lease 和 pending projection | `ER-09` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-10) |
| 137 | W2 | 专项 | [`ER-11`](roadmap/event-receipt-recovery.md#step-er-11) | Event / Receipt / Recovery · Receipt DTO、redacted view 与 source cursor | `ER-10` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-11) |
| 138 | W2 | 专项 | [`ER-12`](roadmap/event-receipt-recovery.md#step-er-12) | Event / Receipt / Recovery · Cost、files、model turns 与 evidence aggregation | `ER-11` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-12) |
| 139 | W2 | 专项 | [`ER-13`](roadmap/event-receipt-recovery.md#step-er-13) | Event / Receipt / Recovery · 统一 result commit 和 result delivery | `ER-12` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-13) |
| 140 | W2 | 专项 | [`ER-14`](roadmap/event-receipt-recovery.md#step-er-14) | Event / Receipt / Recovery · Effect Receipt 与外部 provider receipt | `ER-13` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-14) |
| 141 | W2 | 专项 | [`ER-15`](roadmap/event-receipt-recovery.md#step-er-15) | Event / Receipt / Recovery · Hook、MCP、Memory、Patch 结果统一边界 | `ER-14` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-15) |
| 142 | W2 | 专项 | [`ER-16`](roadmap/event-receipt-recovery.md#step-er-16) | Event / Receipt / Recovery · 终态事件唯一性与 terminal 必达 | `ER-15` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-16) |
| 143 | W2 | 专项 | [`CAP-05`](roadmap/capability.md#step-cap-05) | Capability · 核验授权事实，原子领取一次执行 | `CAP-03`、`CAP-04` | ⏳ | [专项卡](roadmap/capability.md#step-cap-05) |
| 144 | W2 | 专项 | [`CAP-06`](roadmap/capability.md#step-cap-06) | Capability · 审批看见并绑定将被执行的最终计划 | `CAP-05` | ⏳ | [专项卡](roadmap/capability.md#step-cap-06) |
| 145 | W2 | 专项 | [`H06`](roadmap/harness.md#step-h06) | Harness · 一个流归一化器产生增量与完整响应 | `H05` | ⏳ | [专项卡](roadmap/harness.md#step-h06) |
| 146 | W2 | 专项 | [`H07`](roadmap/harness.md#step-h07) | Harness · 贯通预算配置、预留与累计结算 | `H05` | ⏳ | [专项卡](roadmap/harness.md#step-h07) |
| 147 | W2 | 专项 | [`H08`](roadmap/harness.md#step-h08) | Harness · 将 deadline 和取消贯穿静默 I/O | `H03`、`H06`、`H07` | ⏳ | [专项卡](roadmap/harness.md#step-h08) |
| 148 | W2 | 专项 | [`H09`](roadmap/harness.md#step-h09) | Harness · 工具目录成为单一、可版本化的数据源 | `H04`、`H05` | ⏳ | [专项卡](roadmap/harness.md#step-h09) |
| 149 | W2 | 专项 | [`H10`](roadmap/harness.md#step-h10) | Harness · 一次生成、全程稳定的调用身份 | `H02`、`H09` | ⏳ | [专项卡](roadmap/harness.md#step-h10) |
| 150 | W2 | 专项 | [`H11`](roadmap/harness.md#step-h11) | Harness · 工具结果分类与给模型的可修复反馈 | `H05`、`H10` | ⏳ | [专项卡](roadmap/harness.md#step-h11) |
| 151 | W2 | 专项 | [`H12`](roadmap/harness.md#step-h12) | Harness · 串行批次先完整闭环，再考虑并行 | `H08`、`H10`、`H11` | ⏳ | [专项卡](roadmap/harness.md#step-h12) |
| 152 | W2 | 专项 | [`H13`](roadmap/harness.md#step-h13) | Harness · Invocation 账本与结果立即持久化 | `H10`、`H11`、`H12` | ⏳ | [专项卡](roadmap/harness.md#step-h13) |
| 153 | W2 | 专项 | [`H14`](roadmap/harness.md#step-h14) | Harness · 审批暂停与原调用恢复 | `H12`、`H13` | ⏳ | [专项卡](roadmap/harness.md#step-h14) |
| 154 | W2 | 专项 | [`CM-05`](roadmap/context-memory.md#step-cm-05) | Context / Memory · EventStore 唯一提交点与 JSONL/index 投影 | `CM-04` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-05) |
| 155 | W2 | 专项 | [`CM-06`](roadmap/context-memory.md#step-cm-06) | Context / Memory · Source dependency graph 与治理 epoch | `CM-05` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-06) |
| 156 | W2 | 基础 | [`P0-F-01`](#step-p0-f-01) | P0 基础 · 审批一等请求/应答 | `P0-B-01` | ⏳ | [基础卡](#step-p0-f-01) |
| 157 | W2 | 基础 | [`P0-F-02`](#step-p0-f-02) | P0 基础 · 审批决定事件与单次消费 | `P0-F-01` | ⏳ | [基础卡](#step-p0-f-02) |
| 158 | W2 | 基础 | [`P0-J1-01`](#step-p0-j1-01) | P0 基础 · 统一 cancellation token 与状态词表 | `P0-B-01` | ⏳ | [基础卡](#step-p0-j1-01) |
| 159 | W2 | 基础 | [`P1-C-02`](#step-p1-c-02) | P1 基础 · Cell 生命周期与 retire | `P1-C-01` | ⏳ | [基础卡](#step-p1-c-02) |
| 160 | W2 | 基础 | [`P1-D-03`](#step-p1-d-03) | P1 基础 · claim / lease 心跳回收 | `P1-D-01` | ✅ | [基础卡](#step-p1-d-03) |
| 161 | W2 | 基础 | [`P1-J8-01`](#step-p1-j8-01) | P1 基础 · Observability 与 trace/receipt | `P0-G-04` | ⏳ | [基础卡](#step-p1-j8-01) |
| 162 | W2 | 基础 | [`P1-K5-01`](#step-p1-k5-01) | P1 基础 · 成本与容量账本 | `P0-G-04` | ⏳ | [基础卡](#step-p1-k5-01) |
| 163 | W2 | 基础 | [`P3-I-02`](#step-p3-i-02) | P3 基础 · 命令与事件冻结 | `P3-I-01` | ⏳ | [基础卡](#step-p3-i-02) |
| 164 | W2 | 基础 | [`P4-J7-02`](#step-p4-j7-02) | P4 基础 · wire 加 `sequence`/`epoch` | `P0-J7-01` | ⏳ | [基础卡](#step-p4-j7-02) |
| 165 | W2 | 基础 | [`P4-J7-03`](#step-p4-j7-03) | P4 基础 · 事件种类补齐与 terminal 必达 | `P4-J7-02` | ⏳ | [基础卡](#step-p4-j7-03) |
| 166 | W2 | 专项 | [`CI-06`](#step-ci-06) | 单一配置解析器与 schema/migration；新增 `ConfigResolver`，删除 daemon legacy parser；`kiana-provider`/`kiana-daemon` | `CI-01`、`CI-03`、`CI-04` | ⏳ | [专项卡](#step-ci-06) |
| 167 | W2 | 专项 | [`CI-07`](#step-ci-07) | SecretStore 与 CredentialLease；env/keyring/file/OS backend 的窄适配器；`kiana-provider`/`kiana-capability-broker` | `CI-02`、`CI-03`、`CI-06` | ⏳ | [专项卡](#step-ci-07) |
| 168 | W2 | 专项 | [`CI-08`](#step-ci-08) | ProviderGateway 接线与 route admission；`kiana-provider`、`kiana-daemon`、`kiana-core` | `CI-05`、`CI-06`、`CI-07` | ⏳ | [专项卡](#step-ci-08) |
| 169 | W2 | 专项 | [`CI-09`](#step-ci-09) | OAuth/工作负载身份生命周期；PKCE、state、callback、refresh single-flight、generation CAS、0600 atomic file | `CI-07`、`CI-08` | ⏳ | [专项卡](#step-ci-09) |
| 170 | W2 | 专项 | [`CI-10`](#step-ci-10) | Provider policy、只读 credential probe 与 UI/诊断边界；`kiana-policy`、`kiana-entrypoints`、protocol DTO | `CI-06`、`CI-08`、`CI-09` | ⏳ | [专项卡](#step-ci-10) |
| 171 | W2 | 专项 | [`SW-04`](#step-sw-04) | 从 parent/template/department/project/packet/approval 派生 child grant；`kiana-core` authority/capabilities + `cell_registry` | `SW-03`、`CP-04`、`P1-C-02`、`CP-08`、`CP-13` | ⏳ | [专项卡](#step-sw-04) |
| 172 | W2 | 专项 | [`OA-06`](#step-oa-06) | EventStore commit observer；`kiana-eventlog`、`StreamEventStore`、`TransitionBatch` | `OA-05` | ✅ | [专项卡](#step-oa-06) |
| 173 | W2 | 专项 | [`OA-07`](#step-oa-07) | Run/Turn/Invocation span 生命周期；`kiana-core` projection/runner bridge | `OA-02`、`OA-06` | ✅ | [专项卡](#step-oa-07) |
| 174 | W2 | 专项 | [`OA-08`](#step-oa-08) | Provider/model/stream/usage instrumentation；`kiana-provider`、`kiana-daemon/model_client.rs` | `OA-03`、`OA-07` | ✅ | [专项卡](#step-oa-08) |
| 175 | W2 | 专项 | [`OA-09`](#step-oa-09) | Broker/approval/effect/stop instrumentation；`kiana-core/capabilities.rs`、`kiana-daemon/harness_capabilities.rs` | `OA-04`、`OA-07` | ✅ | [专项卡](#step-oa-09) |
| 176 | W2 | 专项 | [`OA-10`](#step-oa-10) | EventLog/projector/Receipt/Artifact/Recovery metrics；`kiana-eventlog`、`projection.rs`、`receipts.rs`、`recovery.rs` | `OA-06`、`OA-09`、`OA-07`、`OA-08` | ✅ | [专项卡](#step-oa-10) |
| 177 | W2 | 专项 | [`AUT-02`](#step-aut-02) | `ClockPort`、wall/monotonic、clock trust/rollback；`kiana-ports`、`kiana-domain` | `AUT-01` | ⏳ | [专项卡](#step-aut-02) |
| 178 | W2 | 专项 | [`AUT-03`](#step-aut-03) | definition/version/digest、DAG/schema/role/project validation；`kiana-domain`、`kiana-workflow` | `P0-J1-01`、`P0-B-01`、`AUT-01` | ⏳ | [专项卡](#step-aut-03) |
| 179 | W2 | 专项 | [`AUT-04`](#step-aut-04) | TriggerDefinition、event envelope、occurrence key、approval/authority/policy 绑定；`kiana-domain`、`kiana-protocol` | `AUT-02`、`AUT-03` | ⏳ | [专项卡](#step-aut-04) |
| 180 | W2 | 专项 | [`AUT-05`](#step-aut-05) | automation event envelope、aggregate stream、command dedup、CAS/cursor query；`kiana-eventlog`、`kiana-ports` | `AUT-03`、`AUT-04` | ⏳ | [专项卡](#step-aut-05) |
| 181 | W2 | 专项 | [`EQ-09`](#step-eq-09) | 在 `kiana-daemon/src/eval_runtime.rs` 实现临时 workspace、临时 `KIANA_HOME`、固定 clock/random seed | `EQ-08` | ⏳ | [专项卡](#step-eq-09) |
| 182 | W2 | 专项 | [`EQ-10`](#step-eq-10) | 实现 fake provider adapter，支持完整 reply、分块 stream、tool call、malformed stream、provider error | `EQ-09` | ⏳ | [专项卡](#step-eq-10) |
| 183 | W2 | 专项 | [`EQ-11`](#step-eq-11) | 实现 deny-by-default broker；将真实 network/secret/MCP/payment/publish/desktop effect 映射为稳定拒绝 | `EQ-10` | ⏳ | [专项卡](#step-eq-11) |
| 184 | W2 | 专项 | [`EQ-12`](#step-eq-12) | 通过 `DaemonHost`/`ControlPlane` 启动 target，禁止 quality crate 自行创建 runner loop | `EQ-11` | ⏳ | [专项卡](#step-eq-12) |
| 185 | W2 | 专项 | [`EQ-13`](#step-eq-13) | 将 initial state、policy snapshot、role assignment、memory/workflow/artifact fixture 装入受控 store | `EQ-12` | ⏳ | [专项卡](#step-eq-13) |
| 186 | W2 | 专项 | [`EQ-14`](#step-eq-14) | 采集 RuntimeEvent、Invocation、Artifact、Receipt 引用和 command receipt；flush 失败产生 infra/Unknown | `EQ-13` | ⏳ | [专项卡](#step-eq-14) |
| 187 | W2 | 专项 | [`EQ-15`](#step-eq-15) | 增加 fault plan：approval deny/expire、cancel race、crash after effect、restart、stale lease、result unknown | `EQ-14` | ⏳ | [专项卡](#step-eq-15) |
| 188 | W2 | 专项 | [`EQ-16`](#step-eq-16) | 对进程树、文件 diff、网络 syscall、secret pattern 做 eval-only evidence capture | `EQ-15` | ⏳ | [专项卡](#step-eq-16) |
| 189 | W2 | 专项 | [`BQ-00`](#step-bq-00) | 基线、快照和冲突清单；盘点 `usage.rs`、`model_budget.rs`、`receipts.rs`、Provider response、CellRegistry、现有事件和测试 | — | ⏳ | [专项卡](#step-bq-00) |
| 190 | W2 | 专项 | [`BQ-01`](#step-bq-01) | 稳定 ID、schema major、unknown/reason、状态枚举与错误码；`kiana-domain`/contracts | `BQ-00` | ⏳ | [专项卡](#step-bq-01) |
| 191 | W2 | 专项 | [`BQ-02`](#step-bq-02) | `UsageVector` 和 `NormalizedUsage`；区分 absent/zero/partial、source/basis/sequence | `BQ-01` | ⏳ | [专项卡](#step-bq-02) |
| 192 | W2 | 专项 | [`BQ-03`](#step-bq-03) | snapshot/delta/final stream 累计器；按 sequence 去重、单调性和包含关系校验 | `BQ-02` | ⏳ | [专项卡](#step-bq-03) |
| 193 | W2 | 专项 | [`BQ-04`](#step-bq-04) | `Money`、整数 micros、checked pricing arithmetic；`RateCard` 版本和有效时间 | `BQ-01`、`BQ-02` | ⏳ | [专项卡](#step-bq-04) |
| 194 | W2 | 专项 | [`BQ-05`](#step-bq-05) | `RateCardStore` 与模型/provider/缓存/音频/工具单价映射 | `BQ-04` | ⏳ | [专项卡](#step-bq-05) |
| 195 | W2 | 专项 | [`BQ-06`](#step-bq-06) | 五类预算合同和交集算法；`RuntimeBudget`、`BudgetLease`、`ProjectBudget`、`ProviderBudget` | `BQ-01`、`BQ-04` | ⏳ | [专项卡](#step-bq-06) |
| 196 | W2 | 专项 | [`BQ-07`](#step-bq-07) | Quota dimension/window、UTC/clock、quota group（alias/credential/model） | `BQ-06` | ⏳ | [专项卡](#step-bq-07) |
| 197 | W2 | 专项 | [`BQ-08`](#step-bq-08) | durable `QuotaReservation`、lease/fence/authority/config revision、CAS/dedup | `BQ-06`、`BQ-07` | ⏳ | [专项卡](#step-bq-08) |
| 198 | W2 | 专项 | [`BQ-09`](#step-bq-09) | admission estimator：最终 wire 请求、输出上限、retry allowance、tool/effect/storage 预算 | `BQ-02`、`BQ-05`、`BQ-08` | ⏳ | [专项卡](#step-bq-09) |
| 199 | W2 | 专项 | [`PD-05`](roadmap/persistence-data-layer.md#step-pd-05) | 为 EventStore 建立 adapter conformance；`kiana-eventlog/tests` | `ER-04`、`PD-04` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-05) |
| 200 | W2 | 专项 | [`PD-06`](roadmap/persistence-data-layer.md#step-pd-06) | 收口 JSONL v2 frame、checksum、fsync、writer lock、尾部恢复；`kiana-eventlog` | `PD-05` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-06) |
| 201 | W2 | 专项 | [`PD-07`](roadmap/persistence-data-layer.md#step-pd-07) | 完成 command/event/aggregate/cursor 索引和 page boundary；`kiana-eventlog` | `PD-05`、`PD-06` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-07) |
| 202 | W2 | 专项 | [`PD-08`](roadmap/persistence-data-layer.md#step-pd-08) | 启动 integrity scan、quarantine、recovery report 和 health gate；`kiana-eventlog`、`kiana-daemon` | `PD-06`、`PD-07` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-08) |
| 203 | W2 | 专项 | [`PD-09`](roadmap/persistence-data-layer.md#step-pd-09) | 建立 projector runner、checkpoint、重试/暂停/重建协议；`kiana-core`、`kiana-daemon` | `PD-07`、`PD-08` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-09) |
| 204 | W2 | 专项 | [`NM-04`](#step-nm-04) | `NotificationProjector` + source cursor/checkpoint；`kiana-eventlog`/`kiana-core` | `PD-05`、`PD-06`、`PD-07`、`PD-08`、`PD-09`、`NM-03` | ⏳ | [专项卡](#step-nm-04) |
| 205 | W2 | 专项 | [`NM-05`](#step-nm-05) | recipient/scope/subscription resolver；Principal/Assignment/ProjectTrust/authority epoch | `CI-05`、`NM-01`、`NM-04` | ⏳ | [专项卡](#step-nm-05) |
| 206 | W2 | 专项 | [`SC-12`](roadmap/security-compliance.md#step-sc-12) | kiana-core PendingInvocation/Permit、CAS、idempotency | `SC-08`、`SC-09`、`SC-10` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-12) |
| 207 | W2 | 专项 | [`SC-13`](roadmap/security-compliance.md#step-sc-13) | kiana-capability-broker、kiana-daemon path/TOCTOU | `SC-12` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-13) |
| 208 | W2 | 专项 | [`SC-14`](roadmap/security-compliance.md#step-sc-14) | Broker sandbox/network profile、endpoint resolver | `SC-12`、`SC-13` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-14) |
| 209 | W2 | 专项 | [`SC-15`](roadmap/security-compliance.md#step-sc-15) | kiana-runner/kiana-core cancel fencing、Unknown | `SC-12`、`SC-14` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-15) |
| 210 | W2 | 专项 | [`SC-16`](roadmap/security-compliance.md#step-sc-16) | kiana-core/Broker quotas、bounded channels、backpressure | `SC-09`、`SC-12` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-16) |
| 211 | W2 | 专项 | [`SC-17`](roadmap/security-compliance.md#step-sc-17) | kiana-daemon MCP/connector/webhook ingress | `SC-06`、`SC-10`、`SC-14`、`SC-15` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-17) |
| 212 | W2 | 专项 | [`SC-18`](roadmap/security-compliance.md#step-sc-18) | kiana-domain SecretRef、kiana-ports SecretStore | `SC-04`、`SC-09`、`SC-17` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-18) |
| 213 | W2 | 专项 | [`SC-19`](roadmap/security-compliance.md#step-sc-19) | kiana-daemon secret lease、rotation/revocation | `SC-18`、`SC-15` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-19) |
| 214 | W2 | 专项 | [`SC-20`](roadmap/security-compliance.md#step-sc-20) | kiana-domain redaction、Broker/Provider/Runner/Event boundaries | `SC-03`、`SC-18` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-20) |
| **W3** | **实际执行、取消与恢复** |  |  |  |  |  |  |
| 215 | W3 | 专项 | [`CP-15`](roadmap/control-plane.md#step-cp-15) | ControlPlane · 统一取消状态，覆盖审批与排队竞态 | `CP-02`、`CP-07`、`CP-13`、`CP-14` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-15) |
| 216 | W3 | 专项 | [`CP-16`](roadmap/control-plane.md#step-cp-16) | ControlPlane · Handler 真正停止与文件提交证据 | `CP-12`、`CP-13`、`CP-15` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-16) |
| 217 | W3 | 专项 | [`CP-17`](roadmap/control-plane.md#step-cp-17) | ControlPlane · 撤销、失败清理与 Cell 退休 | `CP-08`、`CP-11`、`CP-12`、`CP-15`、`CP-16` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-17) |
| 218 | W3 | 专项 | [`CP-18`](roadmap/control-plane.md#step-cp-18) | ControlPlane · RunSnapshot 与安全 checkpoint | `CP-07`、`CP-09`、`CP-14`、`CP-17` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-18) |
| 219 | W3 | 专项 | [`CP-19`](roadmap/control-plane.md#step-cp-19) | ControlPlane · 显式 Resume 与新进程重建 | `CP-10`、`CP-14`、`CP-17`、`CP-18` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-19) |
| 220 | W3 | 专项 | [`CP-20`](roadmap/control-plane.md#step-cp-20) | ControlPlane · Unknown 对账、重试与补偿 | `CP-14`、`CP-17`、`CP-19` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-20) |
| 221 | W3 | 专项 | [`CP-21`](roadmap/control-plane.md#step-cp-21) | ControlPlane · 投影、Receipt 和只读查询 | `CP-07`、`CP-10`、`CP-14`、`CP-19` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-21) |
| 222 | W3 | 专项 | [`CP-22`](roadmap/control-plane.md#step-cp-22) | ControlPlane · Protocol、动作卡与各入口同一事实 | `CP-10`、`CP-15`、`CP-19`、`CP-21` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-22) |
| 223 | W3 | 专项 | [`CP-26`](roadmap/control-plane.md#step-cp-26) | ControlPlane · 决策解释、审计关联与证据 | `CP-14`、`CP-20`、`CP-21` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-26) |
| 224 | W3 | 专项 | [`CP-27`](roadmap/control-plane.md#step-cp-27) | ControlPlane · 非阻塞存储、时钟和资源限额 | `CP-07`、`CP-11`、`CP-15`、`CP-21` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-27) |
| 225 | W3 | 专项 | [`CP-28`](roadmap/control-plane.md#step-cp-28) | ControlPlane · 迁移、兼容 adapter 与旁路收口 | `CP-02`、`CP-07`、`CP-19`、`CP-22` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-28) |
| 226 | W3 | 专项 | [`ER-17`](roadmap/event-receipt-recovery.md#step-er-17) | Event / Receipt / Recovery · Serializable RunSnapshot 与 pending writes | `ER-16` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-17) |
| 227 | W3 | 专项 | [`ER-18`](roadmap/event-receipt-recovery.md#step-er-18) | Event / Receipt / Recovery · Workspace checkpoint transaction 与 restore evidence | `ER-17` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-18) |
| 228 | W3 | 专项 | [`ER-19`](roadmap/event-receipt-recovery.md#step-er-19) | Event / Receipt / Recovery · Worker/process handle 与 fencing token | `ER-18` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-19) |
| 229 | W3 | 专项 | [`ER-20`](roadmap/event-receipt-recovery.md#step-er-20) | Event / Receipt / Recovery · Restart projector 与默认暂停 | `ER-19` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-20) |
| 230 | W3 | 专项 | [`ER-21`](roadmap/event-receipt-recovery.md#step-er-21) | Event / Receipt / Recovery · Explicit resume preflight and claim | `ER-20` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-21) |
| 231 | W3 | 专项 | [`ER-22`](roadmap/event-receipt-recovery.md#step-er-22) | Event / Receipt / Recovery · Cancel recovery and stop confirmation | `ER-21` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-22) |
| 232 | W3 | 专项 | [`ER-23`](roadmap/event-receipt-recovery.md#step-er-23) | Event / Receipt / Recovery · Unknown incident 与 RecoveryPlan | `ER-22` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-23) |
| 233 | W3 | 专项 | [`ER-24`](roadmap/event-receipt-recovery.md#step-er-24) | Event / Receipt / Recovery · Reconciliation commands and evidence | `ER-23` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-24) |
| 234 | W3 | 专项 | [`ER-25`](roadmap/event-receipt-recovery.md#step-er-25) | Event / Receipt / Recovery · Retry policy and new attempt | `ER-24` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-25) |
| 235 | W3 | 专项 | [`ER-26`](roadmap/event-receipt-recovery.md#step-er-26) | Event / Receipt / Recovery · Cursor query、snapshot 和慢消费者 | `ER-25` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-26) |
| 236 | W3 | 专项 | [`ER-27`](roadmap/event-receipt-recovery.md#step-er-27) | Event / Receipt / Recovery · 四入口统一只读 receipt/recovery commands | `ER-26` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-27) |
| 237 | W3 | 专项 | [`CAP-07`](roadmap/capability.md#step-cap-07) | Capability · EnvironmentPort 与可验证 backend 选择 | `CAP-03`、`CAP-04` | ⏳ | [专项卡](roadmap/capability.md#step-cap-07) |
| 238 | W3 | 专项 | [`CAP-08`](roadmap/capability.md#step-cap-08) | Capability · 共享 PathResolver 与文件身份前置条件 | `CAP-07` | ⏳ | [专项卡](roadmap/capability.md#step-cap-08) |
| 239 | W3 | 专项 | [`CAP-09`](roadmap/capability.md#step-cap-09) | Capability · Linux 最小文件视图与不可扩大的挂载集 | `CAP-08` | ⏳ | [专项卡](roadmap/capability.md#step-cap-09) |
| 240 | W3 | 专项 | [`CAP-10`](roadmap/capability.md#step-cap-10) | Capability · 为严格 packet 写集提供隔离写层 | `CAP-09` | ⏳ | [专项卡](roadmap/capability.md#step-cap-10) |
| 241 | W3 | 专项 | [`CAP-11`](roadmap/capability.md#step-cap-11) | Capability · 清理 ambient authority，落实默认断网 | `CAP-09` | ⏳ | [专项卡](roadmap/capability.md#step-cap-11) |
| 242 | W3 | 专项 | [`CAP-12`](roadmap/capability.md#step-cap-12) | Capability · ProcessSupervisor 持有进程树与资源预算 | `CAP-07`、`CAP-11` | ⏳ | [专项卡](roadmap/capability.md#step-cap-12) |
| 243 | W3 | 专项 | [`CAP-13`](roadmap/capability.md#step-cap-13) | Capability · 统一有界输出与脱敏工件 | `CAP-12` | ⏳ | [专项卡](roadmap/capability.md#step-cap-13) |
| 244 | W3 | 专项 | [`CAP-14`](roadmap/capability.md#step-cap-14) | Capability · shell 适配迁入统一执行器 | `CAP-05`、`CAP-12`、`CAP-13` | ⏳ | [专项卡](roadmap/capability.md#step-cap-14) |
| 245 | W3 | 专项 | [`CAP-15`](roadmap/capability.md#step-cap-15) | Capability · patch 一次解析，所有受影响路径可预览 | `CAP-02`、`CAP-08` | ⏳ | [专项卡](roadmap/capability.md#step-cap-15) |
| 246 | W3 | 专项 | [`CAP-16`](roadmap/capability.md#step-cap-16) | Capability · patch 提交点、有限回滚和崩溃恢复 | `CAP-10`、`CAP-15` | ⏳ | [专项卡](roadmap/capability.md#step-cap-16) |
| 247 | W3 | 专项 | [`CAP-17`](roadmap/capability.md#step-cap-17) | Capability · 取消与撤销作用于整个 execution 集合 | `CAP-04`、`CAP-12`、`CAP-14`、`CAP-16` | ⏳ | [专项卡](roadmap/capability.md#step-cap-17) |
| 248 | W3 | 专项 | [`CAP-18`](roadmap/capability.md#step-cap-18) | Capability · Hook 本身受控，改写输入重新授权 | `CAP-06`、`CAP-12`、`CAP-17` | ⏳ | [专项卡](roadmap/capability.md#step-cap-18) |
| 249 | W3 | 专项 | [`CAP-19`](roadmap/capability.md#step-cap-19) | Capability · Memory 使用逻辑资源 scope 与可靠提交监督 | `CAP-03`、`CAP-08`、`CAP-17` | ⏳ | [专项卡](roadmap/capability.md#step-cap-19) |
| 250 | W3 | 专项 | [`CAP-20`](roadmap/capability.md#step-cap-20) | Capability · MCP 先建立可信配置与 discovery snapshot | `CAP-01`、`CAP-03`、`CAP-05`、`CAP-12` | ⏳ | [专项卡](roadmap/capability.md#step-cap-20) |
| 251 | W3 | 专项 | [`CAP-21`](roadmap/capability.md#step-cap-21) | Capability · stdio MCP 协议和停止全过程有界 | `CAP-13`、`CAP-17`、`CAP-20` | ⏳ | [专项卡](roadmap/capability.md#step-cap-21) |
| 252 | W3 | 专项 | [`CAP-22`](roadmap/capability.md#step-cap-22) | Capability · MCP result/schema drift 和连接复用隔离 | `CAP-06`、`CAP-21` | ⏳ | [专项卡](roadmap/capability.md#step-cap-22) |
| 253 | W3 | 专项 | [`CAP-23`](roadmap/capability.md#step-cap-23) | Capability · 并发调度与资源冲突一致 | `CAP-10`、`CAP-17`、`CAP-19`、`CAP-22` | ⏳ | [专项卡](roadmap/capability.md#step-cap-23) |
| 254 | W3 | 专项 | [`CAP-24`](roadmap/capability.md#step-cap-24) | Capability · 以事实重建 Invocation，限制重试并支持对账 | `CAP-05`、`CAP-06`、`CAP-16`、`CAP-17`、`CAP-22`、`CAP-23` | ⏳ | [专项卡](roadmap/capability.md#step-cap-24) |
| 255 | W3 | 专项 | [`CAP-25`](roadmap/capability.md#step-cap-25) | Capability · Receipt、模型与四入口看到一致事实 | `CAP-04`、`CAP-13`、`CAP-24` | ⏳ | [专项卡](roadmap/capability.md#step-cap-25) |
| 256 | W3 | 专项 | [`H15`](roadmap/harness.md#step-h15) | Harness · 工具输出有界、完整结果可按需读取 | `H09`、`H11`、`H13` | ⏳ | [专项卡](roadmap/harness.md#step-h15) |
| 257 | W3 | 专项 | [`H16`](roadmap/harness.md#step-h16) | Harness · 有界并行工具组与独占屏障 | `H08`、`H09`、`H12`、`H13`、`H15` | ⏳ | [专项卡](roadmap/harness.md#step-h16) |
| 258 | W3 | 专项 | [`H17`](roadmap/harness.md#step-h17) | Harness · 后台进程和长工具的可恢复句柄 | `H08`、`H13`、`H15` | ⏳ | [专项卡](roadmap/harness.md#step-h17) |
| 259 | W3 | 专项 | [`H18`](roadmap/harness.md#step-h18) | Harness · 持久 Inbox、ACK 与原子消费 | `H02`、`H03`、`H13` | ⏳ | [专项卡](roadmap/harness.md#step-h18) |
| 260 | W3 | 专项 | [`H19`](roadmap/harness.md#step-h19) | Harness · Continue / Steer / Inject 的产品接线 | `H08`、`H18` | ⏳ | [专项卡](roadmap/harness.md#step-h19) |
| 261 | W3 | 基础 | [`P0-G-03`](#step-p0-g-03) | P0 基础 · `resume_run` 与协议入口 | `P0-G-02b` | ⏳ | [基础卡](#step-p0-g-03) |
| 262 | W3 | 基础 | [`P0-F-03`](#step-p0-f-03) | P0 基础 · 续跑材料落盘与 RunSnapshot | `P0-G-02b`、`P0-G-03`、`P0-F-02` | ⏳ | [基础卡](#step-p0-f-03) |
| 263 | W3 | 基础 | [`P0-J1-02`](#step-p0-j1-02) | P0 基础 · 排空已启动工作 + 合成未启动结果 | `P0-J1-01` | ⏳ | [基础卡](#step-p0-j1-02) |
| 264 | W3 | 基础 | [`P0-J1-03`](#step-p0-j1-03) | P0 基础 · 进程组确认与 `stop_confirmed` | `P0-J1-01` | ⏳ | [基础卡](#step-p0-j1-03) |
| 265 | W3 | 基础 | [`P0-J1-04`](#step-p0-j1-04) | P0 基础 · 取消竞态负向证据 | `P0-J1-01`、`P0-J1-02`、`P0-J1-03` | ⏳ | [基础卡](#step-p0-j1-04) |
| 266 | W3 | 基础 | [`P1-J4-01`](#step-p1-j4-01) | P1 基础 · Capability Descriptor 与 MCP 生命周期 | `P0-A-01a` | ⏳ | [基础卡](#step-p1-j4-01) |
| 267 | W3 | 基础 | [`P2-K4-01`](#step-p2-k4-01) | P2 基础 · Artifact 版本与编辑级 undo | `P0-G-04` | ⏳ | [基础卡](#step-p2-k4-01) |
| 268 | W3 | 基础 | [`P2-K6-01`](#step-p2-k6-01) | P2 基础 · 可靠性与对账 | `P2-K4-01` | ⏳ | [基础卡](#step-p2-k6-01) |
| 269 | W3 | 专项 | [`CI-11`](#step-ci-11) | 审计、redaction、rotation/revoke、recovery projection；`kiana-core`、`kiana-eventlog`、`kiana-daemon` | `CI-04`、`CI-10`、`CI-05`、`CI-06`、`CI-07`、`CI-08`、`CI-09` | ⏳ | [专项卡](#step-ci-11) |
| 270 | W3 | 专项 | [`OA-11`](#step-oa-11) | Health snapshot、readiness/liveness、component capability；`kiana-daemon`/`kiana-core` | `OA-10` | ✅ | [专项卡](#step-oa-11) |
| 271 | W3 | 专项 | [`OA-12`](#step-oa-12) | Metric catalog/reducer/cardinality guard；`kiana-core`/`kiana-eventlog` | `OA-10` | ✅ | [专项卡](#step-oa-12) |
| 272 | W3 | 专项 | [`OA-13`](#step-oa-13) | 异步队列、背压和丢弃策略；`kiana-daemon`/`kiana-eventlog` | `OA-05`、`OA-10` | ✅ | [专项卡](#step-oa-13) |
| 273 | W3 | 专项 | [`OA-14`](#step-oa-14) | Trace exporter 与 W3C context adapter；可选 `kiana-observability` crate 或 daemon module | `OA-02`、`OA-07`、`OA-13` | ✅ | [专项卡](#step-oa-14) |
| 274 | W3 | 专项 | [`OA-15`](#step-oa-15) | AuditProjection checkpoint/rebuild；`kiana-eventlog`/`kiana-core` | `OA-04`、`OA-06`、`OA-10` | ✅ | [专项卡](#step-oa-15) |
| 275 | W3 | 专项 | [`AUT-06`](#step-aut-06) | 纯 planner intent：ready nodes、wait、terminal、reservation、next queue item；`kiana-workflow` | `AUT-03`、`AUT-05` | ⏳ | [专项卡](#step-aut-06) |
| 276 | W3 | 专项 | [`AUT-07`](#step-aut-07) | WorkPacket 与 workflow queue 共用 claim/scope/budget/path-lock contract；`kiana-domain`、`kiana-core` | `AUT-05`、`AUT-06` | ⏳ | [专项卡](#step-aut-07) |
| 277 | W3 | 专项 | [`AUT-08`](#step-aut-08) | `WorkflowQueueStore`、lease/heartbeat/fence/reclaim；`kiana-eventlog`、`kiana-ports`、`kiana-core` | `AUT-05`、`AUT-07` | ⏳ | [专项卡](#step-aut-08) |
| 278 | W3 | 专项 | [`AUT-09`](#step-aut-09) | `DaemonHost` 内 Tokio scheduler/worker service、bounded channel、shutdown；`kiana-daemon` | `AUT-02`、`AUT-08` | ⏳ | [专项卡](#step-aut-09) |
| 279 | W3 | 专项 | [`AUT-10`](#step-aut-10) | Interval due/cursor/missed policy；`kiana-workflow`、`kiana-core` | `AUT-02`、`AUT-04`、`AUT-09` | ⏳ | [专项卡](#step-aut-10) |
| 280 | W3 | 专项 | [`AUT-11`](#step-aut-11) | Event/Webhook ingress、签名/source allowlist、dedupe、filter；`kiana-daemon`、`kiana-protocol` | `AUT-04`、`AUT-05`、`AUT-09` | ⏳ | [专项卡](#step-aut-11) |
| 281 | W3 | 专项 | [`AUT-12`](#step-aut-12) | Reject/Queue/Replace/Coalesce 精确定义与有界 pending index；`kiana-workflow` | `AUT-07`、`AUT-10`、`AUT-11` | ⏳ | [专项卡](#step-aut-12) |
| 282 | W3 | 专项 | [`NM-07`](#step-nm-07) | dedup/idempotency/OCC；dedup key、content hash、subscription revision | `NM-04`、`NM-05` | ⏳ | [专项卡](#step-nm-07) |
| 283 | W3 | 专项 | [`EQ-17`](#step-eq-17) | 在 `kiana-quality/src/normalize.rs` 实现 durable event 选择和 sequence/terminal/correlation 校验 | `P0-G-02a`、`P0-G-02b`、`P0-G-04`、`EQ-16` | ⏳ | [专项卡](#step-eq-17) |
| 284 | W3 | 专项 | [`EQ-18`](#step-eq-18) | 实现 canonical JSON、稳定数组策略、字段白名单和 redaction 复用 | `EQ-17` | ⏳ | [专项卡](#step-eq-18) |
| 285 | W3 | 专项 | [`EQ-19`](#step-eq-19) | 实现受控 volatile normalization（timestamp/UUID/temp path/actor）并记录替换计数 | `EQ-18` | ⏳ | [专项卡](#step-eq-19) |
| 286 | W3 | 专项 | [`EQ-20`](#step-eq-20) | 计算 event/trace/artifact/receipt digest，绑定 `normalization_version` | `EQ-19` | ⏳ | [专项卡](#step-eq-20) |
| 287 | W3 | 专项 | [`EQ-21`](#step-eq-21) | 实现 `TraceDiff`：首个 divergence、字段路径、cursor、expected/actual 摘要和分类 | `EQ-20` | ⏳ | [专项卡](#step-eq-21) |
| 288 | W3 | 专项 | [`EQ-22`](#step-eq-22) | 支持 exact、ordered、multiset、numeric tolerance、regex/contains 等声明式 assertion | `EQ-21` | ⏳ | [专项卡](#step-eq-22) |
| 289 | W3 | 专项 | [`EQ-23`](#step-eq-23) | 添加 `eval capture`：只从明确 source run/fixture 生成新 GoldenTrace，原文件不可覆盖 | `EQ-22` | ⏳ | [专项卡](#step-eq-23) |
| 290 | W3 | 专项 | [`EQ-24`](#step-eq-24) | 添加 Beads 风格 reference/candidate scenario runner、环境清理、in-scope/out-of-scope predicate | `EQ-23` | ⏳ | [专项卡](#step-eq-24) |
| 291 | W3 | 专项 | [`EQ-25`](#step-eq-25) | 支持 curated/deep catalog、no-golden、skip reason、scenario dedupe 和 stable ordering | `EQ-24` | ⏳ | [专项卡](#step-eq-25) |
| 292 | W3 | 专项 | [`EQ-26`](#step-eq-26) | 为 Runtime、Approval、Hook、Memory、Workflow、Swarm 各补 provider-independent trace fixture | `EQ-25` | ⏳ | [专项卡](#step-eq-26) |
| 293 | W3 | 专项 | [`PD-10`](roadmap/persistence-data-layer.md#step-pd-10) | Run/Invocation/Attempt/Receipt 读模型；`kiana-core` | `ER-08`、`ER-09`、`ER-10`、`ER-11`、`ER-12`、`ER-13`、`ER-14`、`ER-15`、`ER-16`、`PD-09` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-10) |
| 294 | W3 | 专项 | [`PD-11`](roadmap/persistence-data-layer.md#step-pd-11) | Cell/Grant/Budget/Lease/Authority 状态投影；`kiana-core`、`kiana-domain` | `CP-10`、`CAP-17`、`PD-09` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-11) |
| 295 | W3 | 专项 | [`PD-12`](roadmap/persistence-data-layer.md#step-pd-12) | 把 ApprovalStore 从独立 JSONL 权威迁为 EventStore 事实 + 查询适配器；`kiana-daemon` | `ER-10`、`PD-10`、`PD-11` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-12) |
| 296 | W3 | 专项 | [`PD-13`](roadmap/persistence-data-layer.md#step-pd-13) | PendingInvocation、Runner continuation、workspace checkpoint 的持久化和恢复材料；`kiana-core`、`kiana-daemon` | `ER-17`、`ER-18`、`ER-19`、`ER-20`、`ER-21`、`ER-22`、`PD-10`、`PD-11`、`PD-12` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-13) |
| 297 | W3 | 专项 | [`PD-14`](roadmap/persistence-data-layer.md#step-pd-14) | 统一 ArtifactStore、content hash、manifest、ref、原子读写；`kiana-core`/新 port | `ER-03`、`PD-04` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-14) |
| 298 | W3 | 专项 | [`PD-15`](roadmap/persistence-data-layer.md#step-pd-15) | Workspace patch/checkpoint、diff、undo 与 Artifact refs 绑定；`kiana-core` | `P2-K4-01`、`PD-14` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-15) |
| 299 | W3 | 专项 | [`PD-16`](roadmap/persistence-data-layer.md#step-pd-16) | Receipt 重算器、evidence graph 和 delivery/closing 引用；`kiana-core`、`kiana-query` | `PD-10`、`PD-14`、`PD-15` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-16) |
| 300 | W3 | 专项 | [`SC-21`](roadmap/security-compliance.md#step-sc-21) | kiana-domain DataClass/Purpose/DataBoundary | `SC-02`、`SC-05`、`SC-20` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-21) |
| 301 | W3 | 专项 | [`SC-22`](roadmap/security-compliance.md#step-sc-22) | kiana-core/kiana-eventlog RetentionPolicy、legal hold | `PD-05`、`OA-08`、`SC-21` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-22) |
| 302 | W3 | 专项 | [`SC-23`](roadmap/security-compliance.md#step-sc-23) | kiana-core/kiana-eventlog DeleteRequest/Tombstone/data epoch | `SC-22`、`SC-12` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-23) |
| 303 | W3 | 专项 | [`SC-25`](roadmap/security-compliance.md#step-sc-25) | kiana-policy ProjectTrust、user/KIANA_HOME/project trust roots | `SC-04`、`SC-07` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-25) |
| 304 | W3 | 专项 | [`SC-26`](roadmap/security-compliance.md#step-sc-26) | kiana-domain ExtensionManifest/CapabilityCatalog | `SC-02`、`SC-09`、`SC-25` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-26) |
| 305 | W3 | 专项 | [`SC-27`](roadmap/security-compliance.md#step-sc-27) | kiana-daemon hook/skill/plugin lifecycle、sandbox | `SC-14`、`SC-16`、`SC-26` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-27) |
| **W4** | **上下文、记忆与扩展** |  |  |  |  |  |  |
| 306 | W4 | 专项 | [`CP-25`](roadmap/control-plane.md#step-cp-25) | ControlPlane · Skills、Hooks、Memory、MCP 与 Secret 的统一边界 | `CP-03`、`CP-04`、`CP-08`、`CP-13`、`CP-18` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-25) |
| 307 | W4 | 专项 | [`H20`](roadmap/harness.md#step-h20) | Harness · 不可变 StepContext 与可解释的上下文编译 | `H04`、`H09`、`H18` | ⏳ | [专项卡](roadmap/harness.md#step-h20) |
| 308 | W4 | 专项 | [`H21`](roadmap/harness.md#step-h21) | Harness · 真实请求预算与稳定缓存前缀 | `H07`、`H20` | ⏳ | [专项卡](roadmap/harness.md#step-h21) |
| 309 | W4 | 专项 | [`H22`](roadmap/harness.md#step-h22) | Harness · 真正保留工作状态的 Compaction | `H05`、`H11`、`H15`、`H20`、`H21` | ⏳ | [专项卡](roadmap/harness.md#step-h22) |
| 310 | W4 | 专项 | [`H23`](roadmap/harness.md#step-h23) | Harness · 压缩结果提交、来源和失效传播 | `H13`、`H18`、`H22` | ⏳ | [专项卡](roadmap/harness.md#step-h23) |
| 311 | W4 | 专项 | [`H24`](roadmap/harness.md#step-h24) | Harness · 完整检查点与显式 Resume | `H13`、`H14`、`H17`、`H18`、`H23` | ⏳ | [专项卡](roadmap/harness.md#step-h24) |
| 312 | W4 | 专项 | [`H25`](roadmap/harness.md#step-h25) | Harness · 重放、故障注入与 Unknown 对账 | `H13`、`H16`、`H23`、`H24` | ⏳ | [专项卡](roadmap/harness.md#step-h25) |
| 313 | W4 | 专项 | [`H26`](roadmap/harness.md#step-h26) | Harness · 澄清请求与权限审批分离 | `H09`、`H14`、`H18`、`H19`、`H24` | ⏳ | [专项卡](roadmap/harness.md#step-h26) |
| 314 | W4 | 专项 | [`H27`](roadmap/harness.md#step-h27) | Harness · 结构化输出与准确的 TurnOutcome | `H05`、`H11`、`H14`、`H26` | ⏳ | [专项卡](roadmap/harness.md#step-h27) |
| 315 | W4 | 专项 | [`H28`](roadmap/harness.md#step-h28) | Harness · 进度、停滞检测与有界修复策略 | `H07`、`H11`、`H17`、`H27` | ⏳ | [专项卡](roadmap/harness.md#step-h28) |
| 316 | W4 | 专项 | [`H29`](roadmap/harness.md#step-h29) | Harness · 有序、受约束的 Hooks / Skills 扩展点 | `H09`、`H20`、`H27`、`H28` | ⏳ | [专项卡](roadmap/harness.md#step-h29) |
| 317 | W4 | 专项 | [`H30`](roadmap/harness.md#step-h30) | Harness · 检索、Memory 与代码索引进入同一 ContextPlan | `H15`、`H20`、`H21`、`H23`、`H29` | ⏳ | [专项卡](roadmap/harness.md#step-h30) |
| 318 | W4 | 专项 | [`CM-07`](roadmap/context-memory.md#step-cm-07) | Context / Memory · Workspace/artifact snapshot 与安全读取 | `CM-06` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-07) |
| 319 | W4 | 专项 | [`CM-08`](roadmap/context-memory.md#step-cm-08) | Context / Memory · 稳定 chunker 与 offset provenance | `CM-07` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-08) |
| 320 | W4 | 专项 | [`CM-09`](roadmap/context-memory.md#step-cm-09) | Context / Memory · 文本规范化、语言和敏感数据边界 | `CM-08` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-09) |
| 321 | W4 | 专项 | [`CM-10`](roadmap/context-memory.md#step-cm-10) | Context / Memory · ContextIndex generation 与原子切换 | `CM-09` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-10) |
| 322 | W4 | 专项 | [`CM-11`](roadmap/context-memory.md#step-cm-11) | Context / Memory · 增量更新、rename/delete 与缓存失效 | `CM-10` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-11) |
| 323 | W4 | 专项 | [`CM-12`](roadmap/context-memory.md#step-cm-12) | Context / Memory · 统一 sparse/dense/RRF/MMR 检索器 | `CM-11` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-12) |
| 324 | W4 | 专项 | [`CM-13`](roadmap/context-memory.md#step-cm-13) | Context / Memory · Repo map 任务相关排序和依赖证据 | `CM-12` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-13) |
| 325 | W4 | 专项 | [`CM-14`](roadmap/context-memory.md#step-cm-14) | Context / Memory · 检索结果 provenance、freshness 与 health | `CM-13` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-14) |
| 326 | W4 | 专项 | [`CM-15`](roadmap/context-memory.md#step-cm-15) | Context / Memory · ContextPlan 选材与 omission 解释 | `CM-14` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-15) |
| 327 | W4 | 专项 | [`CM-16`](roadmap/context-memory.md#step-cm-16) | Context / Memory · 真实 wire budget 与稳定前缀 | `CM-15` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-16) |
| 328 | W4 | 专项 | [`CM-17`](roadmap/context-memory.md#step-cm-17) | Context / Memory · ResolvedStepContext 单一快照 | `CM-16` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-17) |
| 329 | W4 | 专项 | [`CM-18`](roadmap/context-memory.md#step-cm-18) | Context / Memory · 工具结果有界预览与受控 spill | `CM-17` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-18) |
| 330 | W4 | 专项 | [`CM-19`](roadmap/context-memory.md#step-cm-19) | Context / Memory · CompactSummary 结构化生成与校验 | `CM-18` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-19) |
| 331 | W4 | 专项 | [`CM-20`](roadmap/context-memory.md#step-cm-20) | Context / Memory · ContextCheckpoint CAS 提交和新输入并发 | `CM-19` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-20) |
| 332 | W4 | 专项 | [`CM-21`](roadmap/context-memory.md#step-cm-21) | Context / Memory · Resume、cache 和删除失效 | `CM-20` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-21) |
| 333 | W4 | 专项 | [`CM-22`](roadmap/context-memory.md#step-cm-22) | Context / Memory · 六层 ACL 与逐记录过滤统一化 | `CM-21` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-22) |
| 334 | W4 | 专项 | [`CM-23`](roadmap/context-memory.md#step-cm-23) | Context / Memory · candidate / scratch / user-private 负向门 | `CM-22` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-23) |
| 335 | W4 | 专项 | [`CM-24`](roadmap/context-memory.md#step-cm-24) | Context / Memory · 相关性、时效、冲突与历史查询 | `CM-23` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-24) |
| 336 | W4 | 专项 | [`CM-25`](roadmap/context-memory.md#step-cm-25) | Context / Memory · Turn extraction proposal 管线 | `CM-24` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-25) |
| 337 | W4 | 专项 | [`CM-26`](roadmap/context-memory.md#step-cm-26) | Context / Memory · Distillation、decision 与 lesson 统一入库 | `CM-25` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-26) |
| 338 | W4 | 专项 | [`CM-27`](roadmap/context-memory.md#step-cm-27) | Context / Memory · Retrieval/selection/citation receipt | `CM-26` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-27) |
| 339 | W4 | 专项 | [`CM-28`](roadmap/context-memory.md#step-cm-28) | Context / Memory · 删除、过期、撤销传播 | `CM-27` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-28) |
| 340 | W4 | 专项 | [`CM-29`](roadmap/context-memory.md#step-cm-29) | Context / Memory · Projection lag、recovery 与 result_unknown | `CM-28` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-29) |
| 341 | W4 | 专项 | [`EXT-06`](roadmap/skills-plugins-hooks.md#step-ext-06) | Skills / Plugins / Hooks · 三层渐进披露 | `EXT-05` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-06) |
| 342 | W4 | 专项 | [`EXT-07`](roadmap/skills-plugins-hooks.md#step-ext-07) | Skills / Plugins / Hooks · 显式激活与包内资源 | `EXT-06` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-07) |
| 343 | W4 | 专项 | [`EXT-08`](roadmap/skills-plugins-hooks.md#step-ext-08) | Skills / Plugins / Hooks · 条件 Skill、路径和参数 | `EXT-07` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-08) |
| 344 | W4 | 专项 | [`EXT-09`](roadmap/skills-plugins-hooks.md#step-ext-09) | Skills / Plugins / Hooks · Prompt provenance 与预算 | `EXT-05`、`EXT-06` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-09) |
| 345 | W4 | 专项 | [`EXT-10`](roadmap/skills-plugins-hooks.md#step-ext-10) | Skills / Plugins / Hooks · Skill invocation 兼容 | `EXT-08`、`EXT-09` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-10) |
| 346 | W4 | 专项 | [`EXT-11`](roadmap/skills-plugins-hooks.md#step-ext-11) | Skills / Plugins / Hooks · Hook schema 与事件 | `EXT-02` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-11) |
| 347 | W4 | 专项 | [`EXT-12`](roadmap/skills-plugins-hooks.md#step-ext-12) | Skills / Plugins / Hooks · Discovery、匹配和聚合输入 | `EXT-11`、`EXT-04` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-12) |
| 348 | W4 | 专项 | [`EXT-13`](roadmap/skills-plugins-hooks.md#step-ext-13) | Skills / Plugins / Hooks · ProcessSupervisor | `EXT-12` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-13) |
| 349 | W4 | 专项 | [`EXT-14`](roadmap/skills-plugins-hooks.md#step-ext-14) | Skills / Plugins / Hooks · Outcome 与失败策略 | `EXT-13` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-14) |
| 350 | W4 | 专项 | [`EXT-15`](roadmap/skills-plugins-hooks.md#step-ext-15) | Skills / Plugins / Hooks · PreTool 最终输入重新授权 | `EXT-14` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-15) |
| 351 | W4 | 专项 | [`EXT-16`](roadmap/skills-plugins-hooks.md#step-ext-16) | Skills / Plugins / Hooks · 全生命周期事件接线 | `EXT-14` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-16) |
| 352 | W4 | 专项 | [`EXT-17`](roadmap/skills-plugins-hooks.md#step-ext-17) | Skills / Plugins / Hooks · 取消、递归、异步 observer | `EXT-13`、`EXT-16` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-17) |
| 353 | W4 | 专项 | [`EXT-18`](roadmap/skills-plugins-hooks.md#step-ext-18) | Skills / Plugins / Hooks · Receipt、重放和恢复 | `EXT-15`、`EXT-16`、`EXT-17` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-18) |
| 354 | W4 | 专项 | [`EXT-19`](roadmap/skills-plugins-hooks.md#step-ext-19) | Skills / Plugins / Hooks · Plugin manifest v2 | `EXT-03`、`EXT-04` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-19) |
| 355 | W4 | 专项 | [`EXT-20`](roadmap/skills-plugins-hooks.md#step-ext-20) | Skills / Plugins / Hooks · 供应链和不可变包 | `EXT-19` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-20) |
| 356 | W4 | 专项 | [`EXT-21`](roadmap/skills-plugins-hooks.md#step-ext-21) | Skills / Plugins / Hooks · 依赖图与 binding | `EXT-19`、`EXT-20` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-21) |
| 357 | W4 | 专项 | [`EXT-22`](roadmap/skills-plugins-hooks.md#step-ext-22) | Skills / Plugins / Hooks · inspect → stage → install → enable | `EXT-20`、`EXT-21` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-22) |
| 358 | W4 | 专项 | [`EXT-23`](roadmap/skills-plugins-hooks.md#step-ext-23) | Skills / Plugins / Hooks · upgrade、disable、revoke、rollback、uninstall | `EXT-22` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-23) |
| 359 | W4 | 专项 | [`EXT-24`](roadmap/skills-plugins-hooks.md#step-ext-24) | Skills / Plugins / Hooks · secret、state 和 migration | `EXT-22`、`EXT-23` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-24) |
| 360 | W4 | 专项 | [`EXT-25`](roadmap/skills-plugins-hooks.md#step-ext-25) | Skills / Plugins / Hooks · 签名 Skill 服务端绑定 | `EXT-07`、`EXT-09`、`EXT-21` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-25) |
| 361 | W4 | 专项 | [`EXT-26`](roadmap/skills-plugins-hooks.md#step-ext-26) | Skills / Plugins / Hooks · Plugin component adapter | `EXT-15`、`EXT-21`、`EXT-22` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-26) |
| 362 | W4 | 基础 | [`P1-J2-01`](#step-p1-j2-01) | P1 基础 · 类型化区段 + provenance | `P0-G-04` | ⏳ | [基础卡](#step-p1-j2-01) |
| 363 | W4 | 基础 | [`P1-J2-02`](#step-p1-j2-02) | P1 基础 · 预算覆盖 tool schemas 与 system prompt | `P1-J2-01` | ⏳ | [基础卡](#step-p1-j2-02) |
| 364 | W4 | 基础 | [`P1-J2-03`](#step-p1-j2-03) | P1 基础 · 角色 prompt 接线 | `P1-J2-01` | ⏳ | [基础卡](#step-p1-j2-03) |
| 365 | W4 | 基础 | [`P1-J2-04`](#step-p1-j2-04) | P1 基础 · 提示词来源与角色包加载 | `P1-J2-03` | ⏳ | [基础卡](#step-p1-j2-04) |
| 366 | W4 | 基础 | [`P1-J3-02`](#step-p1-j3-02) | P1 基础 · 分层检索与密级 | `P1-J3-01` | ⏳ | [基础卡](#step-p1-j3-02) |
| 367 | W4 | 基础 | [`P1-J3-03`](#step-p1-j3-03) | P1 基础 · 抽取建议包与三档准入 | `P1-J3-01`、`P0-F-01` | ⏳ | [基础卡](#step-p1-j3-03) |
| 368 | W4 | 基础 | [`P1-J3-04`](#step-p1-j3-04) | P1 基础 · hybrid 检索基建 | `P1-J3-02` | ⏳ | [基础卡](#step-p1-j3-04) |
| 369 | W4 | 基础 | [`P1-L4-01`](#step-p1-l4-01) | P1 基础 · Code intelligence 快照 | `P0-A-01a` | ⏳ | [基础卡](#step-p1-l4-01) |
| 370 | W4 | 基础 | [`P2-K7-01`](#step-p2-k7-01) | P2 基础 · 数据治理与删除传播 | `P0-A-01a`、`P1-J3-04` | ⏳ | [基础卡](#step-p2-k7-01) |
| 371 | W4 | 基础 | [`P4-J3-05`](#step-p4-j3-05) | P4 基础 · run 蒸馏与 lesson 入库 | `P1-J3-03` | ⏳ | [基础卡](#step-p4-j3-05) |
| 372 | W4 | 基础 | [`P4-L5-01`](#step-p4-l5-01) | P4 基础 · 扩展与技能包 | `P1-H-01` | ⏳ | [基础卡](#step-p4-l5-01) |
| 373 | W4 | 基础 | [`P4-L6-01`](#step-p4-l6-01) | P4 基础 · 供应链 | `P4-L5-01` | ⏳ | [基础卡](#step-p4-l6-01) |
| 374 | W4 | 专项 | [`PD-17`](roadmap/persistence-data-layer.md#step-pd-17) | Memory mutation journal、candidate/draft/qualify/approve/supersede/tombstone；`kiana-daemon`、`kiana-eventlog` | `CM-04`、`CM-05`、`PD-07`、`PD-09` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-17) |
| 375 | W4 | 专项 | [`PD-18`](roadmap/persistence-data-layer.md#step-pd-18) | Memory projection、ACL/治理 epoch、retention 和删除索引联动；`kiana-daemon`、`kiana-core` | `CM-20`、`CM-21`、`CM-22`、`CM-23`、`CM-24`、`CM-25`、`CM-26`、`CM-27`、`CM-28`、`CM-29`、`PD-17` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-18) |
| 376 | W4 | 专项 | [`PD-19`](roadmap/persistence-data-layer.md#step-pd-19) | ContextIndex generation、source fingerprint、freshness、原子切换；`kiana-query` | `CM-10`、`CM-11`、`CM-12`、`CM-13`、`CM-14`、`PD-09` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-19) |
| 377 | W4 | 专项 | [`PD-20`](roadmap/persistence-data-layer.md#step-pd-20) | RepoMap、context artifact ingest 和依赖图的持久 manifest；`kiana-query` | `PD-14`、`PD-19` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-20) |
| 378 | W4 | 专项 | [`PD-21`](roadmap/persistence-data-layer.md#step-pd-21) | Cache 与事实/投影分离，淘汰、大小/时间上限和禁用开关；`kiana-query`、`kiana-daemon` | `PD-09`、`PD-18`、`PD-19`、`PD-20` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-21) |
| 379 | W4 | 专项 | [`PD-22`](roadmap/persistence-data-layer.md#step-pd-22) | SnapshotManifest、增量/全量 backup、文件 hash、cursor/epoch seal；`kiana-daemon`、`kiana-eventlog` | `PD-08`、`PD-09`、`PD-10`、`PD-11`、`PD-12`、`PD-13`、`PD-14`、`PD-15`、`PD-16`、`PD-17`、`PD-18`、`PD-19`、`PD-20`、`PD-21` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-22) |
| 380 | W4 | 专项 | [`PD-24`](roadmap/persistence-data-layer.md#step-pd-24) | 有序 migration runner、preflight、lock、checksum、MigrationRecord；`kiana-domain`、`kiana-daemon` | `PD-02`、`PD-22`、`PD-03` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-24) |
| 381 | W4 | 专项 | [`PD-25`](roadmap/persistence-data-layer.md#step-pd-25) | Retention policy、legal/audit hold、archive、bounded prune/watermark；`kiana-core`、`kiana-eventlog` | `PD-18`、`PD-22`、`PD-24` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-25) |
| 382 | W4 | 专项 | [`INT-01`](roadmap/integrations-connectors.md#step-int-01) | 固定 Provider/Connector/MCP/A2A/Notification 术语和边界；`docs`、module map | `INT-00` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-01) |
| 383 | W4 | 专项 | [`INT-02`](roadmap/integrations-connectors.md#step-int-02) | Domain typed IDs、definition/binding/invocation/receipt/recovery 合同 | `INT-01` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-02) |
| 384 | W4 | 专项 | [`INT-03`](roadmap/integrations-connectors.md#step-int-03) | Operation input/output schema、风险、scope、data class、retry/timeout 合同 | `INT-02` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-03) |
| 385 | W4 | 专项 | [`INT-04`](roadmap/integrations-connectors.md#step-int-04) | Connector registry immutable version、hash/signature、CAS、catalog projection | `INT-02`、`INT-03` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-04) |
| 386 | W4 | 专项 | [`INT-05`](roadmap/integrations-connectors.md#step-int-05) | AccountBinding、ProviderAccount、project/owner/data boundary、scope intersection | `INT-02`、`INT-04` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-05) |
| 387 | W4 | 专项 | [`SC-24`](roadmap/security-compliance.md#step-sc-24) | kiana-query memory/index/cache/export boundary | `CM-20`、`CM-21`、`CM-22`、`CM-23`、`CM-24`、`CM-25`、`CM-26`、`CM-27`、`CM-28`、`CM-29`、`SC-21`、`SC-22`、`SC-23` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-24) |
| **W5** | **Provider 协议与调用链** |  |  |  |  |  |  |
| 388 | W5 | 专项 | [`P4-J7-11`](roadmap/provider.md#step-p4-j7-11) | Provider · 类型化角色路由与每 attempt 准入 | `P4-J7-09`、`P4-J7-10`、`P1-C-03`、`P0-K1-01`、`P1-K5-01`、`CP-11`、`CP-13` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-11) |
| 389 | W5 | 专项 | [`P4-J7-12`](roadmap/provider.md#step-p4-j7-12) | Provider · 请求编译、工具映射与上下文完整性 | `P4-J7-06`、`P4-J7-11`、`P1-H-01`、`P1-J2-02`、`P1-J2-04` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-12) |
| 390 | W5 | 专项 | [`P4-J7-13`](roadmap/provider.md#step-p4-j7-13) | Provider · HTTP、SSE、NDJSON 的有界传输 | `P4-J7-07`、`P4-J7-09` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-13) |
| 391 | W5 | 专项 | [`P4-J7-14`](roadmap/provider.md#step-p4-j7-14) | Provider · 唯一 accumulator 与协议终态 | `P4-J7-06`、`P4-J7-13` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-14) |
| 392 | W5 | 专项 | [`P4-J7-15`](roadmap/provider.md#step-p4-j7-15) | Provider · Anthropic Messages 完整收口 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-15) |
| 393 | W5 | 专项 | [`P4-J7-16`](roadmap/provider.md#step-p4-j7-16) | Provider · OpenAI Chat Completions 原生流式 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-16) |
| 394 | W5 | 专项 | [`P4-J7-17`](roadmap/provider.md#step-p4-j7-17) | Provider · OpenAI Responses 原生适配 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-17) |
| 395 | W5 | 专项 | [`P4-J7-18`](roadmap/provider.md#step-p4-j7-18) | Provider · Ollama 原生 NDJSON 与本地模型体验 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-18) |
| 396 | W5 | 专项 | [`P4-J7-19`](roadmap/provider.md#step-p4-j7-19) | Provider · Gemini Interactions 原生协议 | `P4-J7-12`、`P4-J7-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-19) |
| 397 | W5 | 专项 | [`P4-J7-20`](roadmap/provider.md#step-p4-j7-20) | Provider · 推理签名、续接资料与短期保护存储 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-19`、`P2-K7-01`、`CP-18`、`CP-25` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-20) |
| 398 | W5 | 专项 | [`P4-J7-21`](roadmap/provider.md#step-p4-j7-21) | Provider · 结构化输出的请求与验收 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-21) |
| 399 | W5 | 专项 | [`P4-J7-22`](roadmap/provider.md#step-p4-j7-22) | Provider · 图片输入与敏感数据出站准入 | `P4-J7-15`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P2-K7-01`、`P4-J7-16`、`CP-25` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-22) |
| 400 | W5 | 专项 | [`P4-J7-23`](roadmap/provider.md#step-p4-j7-23) | Provider · 单层重试、绝对时限和取消传递 | `P4-J7-11`、`P4-J7-13`、`P4-J7-14`、`P0-J1-04`、`P0-J1-05a`、`P0-J1-05b`、`CP-15` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-23) |
| 401 | W5 | 专项 | [`P4-J7-24`](roadmap/provider.md#step-p4-j7-24) | Provider · Provider 用量、价格快照与预算结算 | `P4-J7-15`、`P4-J7-16`、`P4-J7-17`、`P4-J7-18`、`P4-J7-19`、`P4-J7-23`、`P1-K5-01`、`CP-11`、`CP-14` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-24) |
| 402 | W5 | 专项 | [`P4-J7-25`](roadmap/provider.md#step-p4-j7-25) | Provider · 容量、熔断与白名单 fallback | `P4-J7-23`、`P4-J7-24` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-25) |
| 403 | W5 | 专项 | [`P4-J7-26`](roadmap/provider.md#step-p4-j7-26) | Provider · 模型事件、脱敏和完整关联链 | `P4-J7-20`、`P4-J7-23`、`P4-J7-24`、`P1-J8-01`、`P0-G-04`、`CP-26` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-26) |
| 404 | W5 | 专项 | [`P4-J7-27`](roadmap/provider.md#step-p4-j7-27) | Provider · 完整轮次恢复与 in-flight 对账 | `P4-J7-20`、`P4-J7-26`、`P0-G-03`、`P0-F-03`、`P2-K6-01`、`CP-18`、`CP-19`、`CP-20` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-27) |
| 405 | W5 | 专项 | [`BQ-10`](#step-bq-10) | Provider normalized usage adapters；Anthropic/OpenAI/Ollama/Gemini/Fake 字段包含关系 | `P4-J7-24`、`BQ-02`、`BQ-03`、`BQ-05`、`BQ-09` | ⏳ | [专项卡](#step-bq-10) |
| 406 | W5 | 专项 | [`BQ-11`](#step-bq-11) | Model attempt 生命周期和事件：prepared/dispatching/observed/settled/unknown | `BQ-08`、`BQ-10` | ⏳ | [专项卡](#step-bq-11) |
| 407 | W5 | 专项 | [`BQ-12`](#step-bq-12) | settlement/release/unknown fold；已知消费、未用预留和 result_unknown 分离 | `BQ-11` | ⏳ | [专项卡](#step-bq-12) |
| 408 | W5 | 专项 | [`BQ-13`](#step-bq-13) | estimated/measured cost 计算和 Receipt breakdown；`receipts.rs`、query projector | `BQ-05`、`BQ-12` | ⏳ | [专项卡](#step-bq-13) |
| 409 | W5 | 专项 | [`INT-06`](roadmap/integrations-connectors.md#step-int-06) | SecretRef/CredentialLease 与 connector invocation 绑定；复用 CI-07 | `CI-07`、`INT-05` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-06) |
| 410 | W5 | 专项 | [`INT-07`](roadmap/integrations-connectors.md#step-int-07) | `ConnectorAdapter`、`EffectObserver`、`CredentialProbe`、`WebhookVerifier` ports | `INT-02`、`INT-06` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-07) |
| 411 | W5 | 专项 | [`INT-08`](roadmap/integrations-connectors.md#step-int-08) | local_fixture schema、hash、payload matching、deterministic receipts | `INT-04`、`INT-07` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-08) |
| 412 | W5 | 专项 | [`INT-09`](roadmap/integrations-connectors.md#step-int-09) | read-only health/probe 和状态分类；daemon/query/UI | `INT-05`、`INT-07`、`INT-08` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-09) |
| 413 | W5 | 专项 | [`INT-10`](roadmap/integrations-connectors.md#step-int-10) | stdio MCP connector adapter 与 capability handshake | `INT-07`、`INT-09` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-10) |
| 414 | W5 | 专项 | [`INT-11`](roadmap/integrations-connectors.md#step-int-11) | HTTPS endpoint、TLS、redirect、proxy、DNS/SSRF/egress allowlist | `INT-06`、`INT-07` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-11) |
| 415 | W5 | 专项 | [`INT-12`](roadmap/integrations-connectors.md#step-int-12) | OAuth PKCE/state/callback、account store、refresh single-flight | `CI-09`、`INT-06`、`INT-11` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-12) |
| 416 | W5 | 专项 | [`INT-13`](roadmap/integrations-connectors.md#step-int-13) | secret redaction/echo sentinel 扫描；domain/daemon/event/UI tests | `INT-06`、`INT-08`、`INT-12` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-13) |
| **W6** | **投影与可用入口** |  |  |  |  |  |  |
| 417 | W6 | 专项 | [`H32`](roadmap/harness.md#step-h32) | Harness · 事实流、展示流与三入口状态一致性 | `H06`、`H13`、`H18`、`H24`、`H26`、`H27` | ⏳ | [专项卡](roadmap/harness.md#step-h32) |
| 418 | W6 | 专项 | [`EXT-27`](roadmap/skills-plugins-hooks.md#step-ext-27) | Skills / Plugins / Hooks · 动态可见性投影 | `EXT-06`、`EXT-25`、`EXT-26` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-27) |
| 419 | W6 | 专项 | [`EXT-28`](roadmap/skills-plugins-hooks.md#step-ext-28) | Skills / Plugins / Hooks · 命令与 UI 合同 | `EXT-22`、`EXT-23`、`EXT-27` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-28) |
| 420 | W6 | 专项 | [`UI-04`](roadmap/ui-entrypoints.md#step-ui-04) | UI / Entrypoints · 动作 CAS、idempotency 与响应丢失 | `UI-01`、`UI-02`、`UI-03` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-04) |
| 421 | W6 | 专项 | [`UI-05`](roadmap/ui-entrypoints.md#step-ui-05) | UI / Entrypoints · 原子 snapshot projector 与分页 | `UI-01`、`UI-02`、`UI-03`、`UI-04` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-05) |
| 422 | W6 | 专项 | [`UI-06`](roadmap/ui-entrypoints.md#step-ui-06) | UI / Entrypoints · feed cursor、gap、replay 与背压 | `UI-05` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-06) |
| 423 | W6 | 专项 | [`UI-07`](roadmap/ui-entrypoints.md#step-ui-07) | UI / Entrypoints · typed client query/feed/action API | `UI-02`、`UI-03`、`UI-04`、`UI-05`、`UI-06` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-07) |
| 424 | W6 | 专项 | [`UI-08`](roadmap/ui-entrypoints.md#step-ui-08) | UI / Entrypoints · 共享 reducer/entity store | `UI-05`、`UI-06`、`UI-07` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-08) |
| 425 | W6 | 专项 | [`UI-09`](roadmap/ui-entrypoints.md#step-ui-09) | UI / Entrypoints · schema 资产、生成和兼容门 | `UI-01`、`UI-07`、`UI-08` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-09) |
| 426 | W6 | 专项 | [`UI-10`](roadmap/ui-entrypoints.md#step-ui-10) | UI / Entrypoints · CLI 命令和输出归一化 | `UI-07`、`UI-08`、`UI-09` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-10) |
| 427 | W6 | 专项 | [`UI-11`](roadmap/ui-entrypoints.md#step-ui-11) | UI / Entrypoints · CLI JSON/TTY/exit code presenter | `UI-02`、`UI-10` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-11) |
| 428 | W6 | 专项 | [`UI-12`](roadmap/ui-entrypoints.md#step-ui-12) | UI / Entrypoints · TTY 输入状态机 | `UI-08`、`UI-10` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-12) |
| 429 | W6 | 专项 | [`UI-13`](roadmap/ui-entrypoints.md#step-ui-13) | UI / Entrypoints · Workbench 时间线与结果渲染 | `UI-06`、`UI-08`、`UI-12` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-13) |
| 430 | W6 | 专项 | [`UI-14`](roadmap/ui-entrypoints.md#step-ui-14) | UI / Entrypoints · Workbench controller 与命令面板 | `UI-07`、`UI-08`、`UI-10`、`UI-11`、`UI-12`、`UI-13` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-14) |
| 431 | W6 | 专项 | [`UI-15`](roadmap/ui-entrypoints.md#step-ui-15) | UI / Entrypoints · Workbench inbox、Diff 和 Receipt | `UI-05`、`UI-08`、`UI-13`、`UI-14` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-15) |
| 432 | W6 | 专项 | [`UI-16`](roadmap/ui-entrypoints.md#step-ui-16) | UI / Entrypoints · Web 路由、来源校验和最小健康信息 | `UI-03`、`UI-07`、`UI-11` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-16) |
| 433 | W6 | 专项 | [`UI-17`](roadmap/ui-entrypoints.md#step-ui-17) | UI / Entrypoints · Web snapshot hydrate、历史和分页 | `UI-05`、`UI-08`、`UI-16` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-17) |
| 434 | W6 | 专项 | [`UI-18`](roadmap/ui-entrypoints.md#step-ui-18) | UI / Entrypoints · Web SSE reconnect、Last-Event-ID 和 gap | `UI-06`、`UI-16`、`UI-17` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-18) |
| 435 | W6 | 专项 | [`UI-19`](roadmap/ui-entrypoints.md#step-ui-19) | UI / Entrypoints · Web session ownership 与多 tab 并发 | `UI-04`、`UI-08`、`UI-16`、`UI-17`、`UI-18` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-19) |
| 436 | W6 | 专项 | [`UI-20`](roadmap/ui-entrypoints.md#step-ui-20) | UI / Entrypoints · Web 时间线组件迁移 | `UI-08`、`UI-13`、`UI-17`、`UI-18` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-20) |
| 437 | W6 | 专项 | [`UI-21`](roadmap/ui-entrypoints.md#step-ui-21) | UI / Entrypoints · Web Human Inbox 与审批动作卡 | `UI-02`、`UI-04`、`UI-05`、`UI-15`、`UI-20` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-21) |
| 438 | W6 | 专项 | [`UI-22`](roadmap/ui-entrypoints.md#step-ui-22) | UI / Entrypoints · Web artifact/diff/receipt detail | `UI-05`、`UI-15`、`UI-20`、`UI-21` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-22) |
| 439 | W6 | 专项 | [`UI-23`](roadmap/ui-entrypoints.md#step-ui-23) | UI / Entrypoints · Web 可访问性、焦点和内容安全 | `UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-23) |
| 440 | W6 | 专项 | [`UI-24`](roadmap/ui-entrypoints.md#step-ui-24) | UI / Entrypoints · Electron IPC sender、导航和新窗口 allowlist | `UI-02`、`UI-03`、`UI-16`、`UI-23` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-24) |
| 441 | W6 | 专项 | [`UI-25`](roadmap/ui-entrypoints.md#step-ui-25) | UI / Entrypoints · Desktop readiness、attach 和 worker 生命周期 | `UI-03`、`UI-24` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-25) |
| 442 | W6 | 专项 | [`UI-26`](roadmap/ui-entrypoints.md#step-ui-26) | UI / Entrypoints · Desktop workspace、托盘、通知与关闭策略 | `UI-08`、`UI-19`、`UI-24`、`UI-25` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-26) |
| 443 | W6 | 专项 | [`UI-27`](roadmap/ui-entrypoints.md#step-ui-27) | UI / Entrypoints · Desktop 安全持久化和 detach | `UI-04`、`UI-25`、`UI-26` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-27) |
| 444 | W6 | 专项 | [`UI-28`](roadmap/ui-entrypoints.md#step-ui-28) | UI / Entrypoints · 共享静态资产、版本和生产打包 | `UI-20`、`UI-23`、`UI-24`、`UI-25`、`UI-26`、`UI-27` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-28) |
| 445 | W6 | 专项 | [`UI-29`](roadmap/ui-entrypoints.md#step-ui-29) | UI / Entrypoints · ACP/IDE session adapter | `UI-01`、`UI-02`、`UI-07`、`UI-18`、`UI-21`、`UI-25` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-29) |
| 446 | W6 | 专项 | [`UI-30`](roadmap/ui-entrypoints.md#step-ui-30) | UI / Entrypoints · IDE editor/terminal capability boundary | `UI-04`、`UI-07`、`UI-29` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-30) |
| 447 | W6 | 基础 | [`P0-M1-01`](#step-p0-m1-01) | P0 基础 · Workbench 基线 | — | ⏳ | [基础卡](#step-p0-m1-01) |
| 448 | W6 | 基础 | [`P2-K3-01`](#step-p2-k3-01) | P2 基础 · Human Inbox | `P0-F-02` | ⏳ | [基础卡](#step-p2-k3-01) |
| 449 | W6 | 基础 | [`P2-M2-01`](#step-p2-m2-01) | P2 基础 · UI 投影合同 | `P0-M1-01` | ⏳ | [基础卡](#step-p2-m2-01) |
| 450 | W6 | 基础 | [`P2-M3-01`](#step-p2-m3-01) | P2 基础 · 人工动作卡 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m3-01) |
| 451 | W6 | 基础 | [`P2-M4-01`](#step-p2-m4-01) | P2 基础 · Run/Artifact 详情 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m4-01) |
| 452 | W6 | 基础 | [`P2-M5-01`](#step-p2-m5-01) | P2 基础 · Web 快照水合与重连 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m5-01) |
| 453 | W6 | 专项 | [`P4-J7-28`](roadmap/provider.md#step-p4-j7-28) | Provider · 模型选择、诊断与事件投影 | `P4-J7-10`、`P4-J7-11`、`P4-J7-25`、`P4-J7-26`、`P4-J7-02`、`P4-J7-03`、`P2-M2-01`、`P2-M5-01`、`CP-22` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-28) |
| 454 | W6 | 基础 | [`P2-M7-01`](#step-p2-m7-01) | P2 基础 · 无障碍回退 | `P2-M2-01` | ⏳ | [基础卡](#step-p2-m7-01) |
| 455 | W6 | 基础 | [`P4-M6-01`](#step-p4-m6-01) | P4 基础 · Desktop 壳 | `P2-M2-01` | ⏳ | [基础卡](#step-p4-m6-01) |
| 456 | W6 | 专项 | [`OA-16`](#step-oa-16) | Audit query command/wire DTO；`kiana-protocol`、`kiana-client`、`DaemonHost` | `CP-21`、`CP-22`、`OA-15` | ✅ | [专项卡](#step-oa-16) |
| 457 | W6 | 专项 | [`OA-17`](#step-oa-17) | Query cursor、snapshot、分页和慢查询；`kiana-eventlog` cursor API、protocol | `OA-15`、`OA-16` | ✅ | [专项卡](#step-oa-17) |
| 458 | W6 | 专项 | [`OA-18`](#step-oa-18) | 审计导出、manifest、delivery evidence；`kiana-entrypoints`/ArtifactStore/ControlPlane | `OA-03`、`OA-16`、`OA-17` | ✅ | [专项卡](#step-oa-18) |
| 459 | W6 | 专项 | [`OA-19`](#step-oa-19) | Alert/Incident 规则、去重和 Recovery 关联；`kiana-core/recovery.rs`、daemon health | `OA-10`、`OA-11`、`OA-15` | ✅ | [专项卡](#step-oa-19) |
| 460 | W6 | 专项 | [`OA-20`](#step-oa-20) | DataClass/Purpose/Retention/Deletion propagation；`data_governance.rs`、Memory/Artifact/Query/Telemetry stores | `OA-03`、`OA-15`、`OA-19`、`OA-16`、`OA-17`、`OA-18` | ✅ | [专项卡](#step-oa-20) |
| 461 | W6 | 专项 | [`OA-21`](#step-oa-21) | Replay/reconciliation diagnostics；新增只读 `replay`/audit consistency fixture | `OA-15`、`OA-19`、`OA-20` | ✅ | [专项卡](#step-oa-21) |
| 462 | W6 | 专项 | [`OA-22`](#step-oa-22) | Crash/fault injection；EventStore、Broker、Provider、projector、export、shutdown | `OA-06`、`OA-21`、`OA-07`、`OA-08`、`OA-09`、`OA-10`、`OA-11`、`OA-12`、`OA-13`、`OA-14`、`OA-15`、`OA-16`、`OA-17`、`OA-18`、`OA-19`、`OA-20` | ✅ | [专项卡](#step-oa-22) |
| 463 | W6 | 专项 | [`NM-06`](#step-nm-06) | Notification materializer 与 HumanTask bridge；`kiana-core/platform.rs`、`kiana-domain/platform.rs` | `P2-K3-01`、`NM-04`、`NM-05` | ⏳ | [专项卡](#step-nm-06) |
| 464 | W6 | 专项 | [`NM-08`](#step-nm-08) | durable outbox + DeliveryWorker；attempt lease/fence、shutdown drain | `PD-10`、`PD-11`、`PD-12`、`PD-13`、`NM-06`、`NM-07` | ⏳ | [专项卡](#step-nm-08) |
| 465 | W6 | 专项 | [`NM-09`](#step-nm-09) | in-process/in-app channel；NotificationStore query/page | `NM-06`、`NM-08` | ⏳ | [专项卡](#step-nm-09) |
| 466 | W6 | 专项 | [`NM-10`](#step-nm-10) | action refs 与 HumanTask action command；审批、ACK、review、reconcile、snooze/escalate/delegate/withdraw | `CP-18`、`CP-19`、`NM-06`、`NM-09` | ⏳ | [专项卡](#step-nm-10) |
| 467 | W6 | 专项 | [`NM-11`](#step-nm-11) | unread/read/ack/snooze/digest 投影和排序；server time、due/urgency | `NM-09`、`NM-10` | ⏳ | [专项卡](#step-nm-11) |
| 468 | W6 | 专项 | [`NM-12`](#step-nm-12) | durable inbox rebuild、retention/withdraw/supersede；`NotificationProjector`/PD-24..PD-26 | `P2-K7-01`、`NM-04`、`NM-09` | ⏳ | [专项卡](#step-nm-12) |
| 469 | W6 | 专项 | [`NM-13`](#step-nm-13) | run stream/notification bridge；snapshot-first、after cursor、epoch、gap、heartbeat/disposed | `UI-17`、`UI-18`、`NM-04`、`NM-08` | ⏳ | [专项卡](#step-nm-13) |
| 470 | W6 | 专项 | [`NM-14`](#step-nm-14) | CLI/TTY inbox 与运行状态；`cli.rs`、`workbench_chat.rs` | `UI-12`、`UI-15`、`NM-09`、`NM-10`、`NM-13` | ⏳ | [专项卡](#step-nm-14) |
| 471 | W6 | 专项 | [`NM-15`](#step-nm-15) | Web REST snapshot/page + SSE；`web.rs`/`kiana-client` | `UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-21`、`NM-09`、`NM-10`、`NM-13` | ⏳ | [专项卡](#step-nm-15) |
| 472 | W6 | 专项 | [`NM-16`](#step-nm-16) | Desktop local notification adapter；OS permission、tray、close/detach | `UI-24`、`UI-25`、`UI-26`、`UI-27`、`NM-15` | ⏳ | [专项卡](#step-nm-16) |
| 473 | W6 | 专项 | [`EQ-27`](#step-eq-27) | 实现 deterministic evaluator trait 和 finding schema（code/expected/actual/message/evidence_ref） | `EQ-26` | ⏳ | [专项卡](#step-eq-27) |
| 474 | W6 | 专项 | [`EQ-28`](#step-eq-28) | 实现 runtime correctness evaluator：事件顺序、调用关联、terminal、retry、approval、cancel、Unknown | `EQ-27` | ⏳ | [专项卡](#step-eq-28) |
| 475 | W6 | 专项 | [`EQ-29`](#step-eq-29) | 实现 capability/safety evaluator：schema、grant scope、policy verdict、hook、network/process/file effect | `EQ-28` | ⏳ | [专项卡](#step-eq-29) |
| 476 | W6 | 专项 | [`EQ-30`](#step-eq-30) | 实现 evidence/receipt evaluator：artifact hash、receipt assertions、redaction、provenance、source cursor | `EQ-29` | ⏳ | [专项卡](#step-eq-30) |
| 477 | W6 | 专项 | [`EQ-31`](#step-eq-31) | 实现 recovery/replay evaluator：crash/restart、fence、result_unknown、logic version、divergence | `EQ-30` | ⏳ | [专项卡](#step-eq-31) |
| 478 | W6 | 专项 | [`EQ-32`](#step-eq-32) | 实现 context/memory evaluator：ACL-before-ranking、provenance、freshness、compaction、budget | `EQ-31` | ⏳ | [专项卡](#step-eq-32) |
| 479 | W6 | 专项 | [`EQ-33`](#step-eq-33) | 实现 workflow/swarm evaluator：DAG、attempt、fan-in/out、child scope、merge、compensation | `EQ-32` | ⏳ | [专项卡](#step-eq-33) |
| 480 | W6 | 专项 | [`EQ-34`](#step-eq-34) | 实现 performance/cost metrics evaluator：duration、tokens、tool calls、cache、cost buckets | `P1-J8-01`、`P1-K5-01`、`EQ-33` | ⏳ | [专项卡](#step-eq-34) |
| 481 | W6 | 专项 | [`EQ-35`](#step-eq-35) | 实现可选 semantic judge port；固定 judge prompt/model/version，judge 不可用不降级为 pass | `EQ-34` | ⏳ | [专项卡](#step-eq-35) |
| 482 | W6 | 专项 | [`EQ-36`](#step-eq-36) | 实现维度聚合、absolute/relative threshold、minimum sample 和置信区间策略 | `EQ-35` | ⏳ | [专项卡](#step-eq-36) |
| 483 | W6 | 专项 | [`EQ-37`](#step-eq-37) | 实现 retry-once flake classifier、quarantine 记录和 infra failure 分类 | `EQ-36` | ⏳ | [专项卡](#step-eq-37) |
| 484 | W6 | 专项 | [`BQ-14`](#step-bq-14) | append-only `CostLedgerEntry` 和 `CostCorrection` command/approval | `CP-11`、`ER-12`、`BQ-13` | ⏳ | [专项卡](#step-bq-14) |
| 485 | W6 | 专项 | [`BQ-15`](#step-bq-15) | Tool/effect/resource usage；Broker invocation、shell/MCP、Artifact/log/storage 计量 | `CAP-17`、`BQ-06`、`BQ-08`、`BQ-11` | ⏳ | [专项卡](#step-bq-15) |
| 486 | W6 | 专项 | [`BQ-16`](#step-bq-16) | Provider capacity、RPM/TPM、semaphore、bounded fair queue、backpressure | `BQ-07`、`BQ-08`、`BQ-15` | ⏳ | [专项卡](#step-bq-16) |
| 487 | W6 | 专项 | [`BQ-17`](#step-bq-17) | Retry classifier、attempt reservation、Retry-After 和 cancellation | `P4-J7-23`、`BQ-11`、`BQ-12`、`BQ-16` | ⏳ | [专项卡](#step-bq-17) |
| 488 | W6 | 专项 | [`BQ-18`](#step-bq-18) | 白名单 fallback 与 route/authority/data/price 重新准入 | `BQ-05`、`BQ-08`、`BQ-10`、`BQ-17` | ⏳ | [专项卡](#step-bq-18) |
| 489 | W6 | 专项 | [`BQ-19`](#step-bq-19) | project/org/workflow/cell/run allocation；避免父子/多维重复相加 | `BQ-13`、`BQ-14`、`BQ-15` | ⏳ | [专项卡](#step-bq-19) |
| 490 | W6 | 专项 | [`BQ-20`](#step-bq-20) | EventLog ledger projector、source cursor、projection version、daily/window rollups | `PD-05`、`BQ-11`、`BQ-14`、`BQ-19` | ⏳ | [专项卡](#step-bq-20) |
| 491 | W6 | 专项 | [`INT-14`](roadmap/integrations-connectors.md#step-int-14) | `connector.manage/invoke/health/reconcile` protocol DTO 和 normalize | `INT-03`、`INT-05`、`INT-07` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-14) |
| 492 | W6 | 专项 | [`INT-15`](roadmap/integrations-connectors.md#step-int-15) | operation risk→policy/gate/approval 映射 | `INT-03`、`INT-14` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-15) |
| 493 | W6 | 专项 | [`INT-16`](roadmap/integrations-connectors.md#step-int-16) | invocation reservation、command digest、idempotency/CAS | `CP-13`、`ER-07`、`INT-04`、`INT-05`、`INT-14`、`INT-15` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-16) |
| 494 | W6 | 专项 | [`INT-17`](roadmap/integrations-connectors.md#step-int-17) | connector/account/project rate、concurrency、budget reservation | `BQ-08`、`INT-16` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-17) |
| 495 | W6 | 专项 | [`INT-18`](roadmap/integrations-connectors.md#step-int-18) | effect-time permit、authority/config/policy/credential/data epoch fencing | `INT-06`、`INT-15`、`INT-16` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-18) |
| 496 | W6 | 专项 | [`INT-19`](roadmap/integrations-connectors.md#step-int-19) | Broker dispatch 和 adapter observation 分离 | `INT-07`、`INT-16`、`INT-18` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-19) |
| 497 | W6 | 专项 | [`INT-20`](roadmap/integrations-connectors.md#step-int-20) | ProviderReceipt/EffectObservation schema、owner/audience、payload hash | `INT-08`、`INT-19` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-20) |
| 498 | W6 | 专项 | [`INT-21`](roadmap/integrations-connectors.md#step-int-21) | retry classifier、attempt、timeout/backoff、idempotency policy | `INT-16`、`INT-20` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-21) |
| 499 | W6 | 专项 | [`INT-22`](roadmap/integrations-connectors.md#step-int-22) | Unknown quarantine、ReconciliationCase、provider query/manual evidence | `INT-20`、`INT-21` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-22) |
| 500 | W6 | 专项 | [`INT-23`](roadmap/integrations-connectors.md#step-int-23) | cancel/stop report/late result fence、lease settlement | `INT-18`、`INT-19`、`INT-22` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-23) |
| 501 | W6 | 专项 | [`SC-28`](roadmap/security-compliance.md#step-sc-28) | scripts、CI SBOM、dependency/license/advisory scanner | `SC-02`、`SC-25` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-28) |
| 502 | W6 | 专项 | [`SC-29`](roadmap/security-compliance.md#step-sc-29) | release artifact signing/provenance verifier | `SC-28` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-29) |
| 503 | W6 | 专项 | [`SC-30`](roadmap/security-compliance.md#step-sc-30) | provider/model/prompt-pack/MCP route attestation | `SC-17`、`SC-26`、`SC-29` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-30) |
| 504 | W6 | 专项 | [`SC-31`](roadmap/security-compliance.md#step-sc-31) | kiana-protocol/kiana-eventlog AuditRecord schema | `SC-02`、`SC-03`、`SC-05`、`SC-20` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-31) |
| **W7** | **CompanyOS 业务闭环** |  |  |  |  |  |  |
| 505 | W7 | 专项 | [`CP-23`](roadmap/control-plane.md#step-cp-23) | ControlPlane · Company 命令也使用控制面事务 | `CP-04`、`CP-07`、`CP-08`、`CP-13` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-23) |
| 506 | W7 | 专项 | [`CP-24`](roadmap/control-plane.md#step-cp-24) | ControlPlane · 调度、WorkPacket、委派与 Workflow | `CP-11`、`CP-12`、`CP-17`、`CP-23` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-24) |
| 507 | W7 | 专项 | [`ER-28`](roadmap/event-receipt-recovery.md#step-er-28) | Event / Receipt / Recovery · CompanyOS / Workflow / Artifact 业务引用 | `ER-27` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-28) |
| 508 | W7 | 专项 | [`CO-09`](roadmap/companyos.md#step-co-09) | CompanyOS · Objective 与 Initiative 受理和取舍 | `CO-08` | ⏳ | [专项卡](roadmap/companyos.md#step-co-09) |
| 509 | W7 | 专项 | [`CO-10`](roadmap/companyos.md#step-co-10) | CompanyOS · Charter 与项目 go/no-go | `CO-09`、`CO-06` | ⏳ | [专项卡](roadmap/companyos.md#step-co-10) |
| 510 | W7 | 专项 | [`CO-11`](roadmap/companyos.md#step-co-11) | CompanyOS · 建立标准覆盖图，替换文本集合推断 | `CO-06`、`CO-10` | ⏳ | [专项卡](roadmap/companyos.md#step-co-11) |
| 511 | W7 | 专项 | [`CO-12`](roadmap/companyos.md#step-co-12) | CompanyOS · Milestone、Plan 与依赖图提案 | `CO-10`、`CO-11` | ⏳ | [专项卡](roadmap/companyos.md#step-co-12) |
| 512 | W7 | 专项 | [`CO-13`](roadmap/companyos.md#step-co-13) | CompanyOS · WorkPacket 扩为通用部门工作合同 | `CO-04`、`CO-06`、`CO-12` | ⏳ | [专项卡](roadmap/companyos.md#step-co-13) |
| 513 | W7 | 专项 | [`CO-14`](roadmap/companyos.md#step-co-14) | CompanyOS · 会议、黑板和决议对象补齐 | `CO-04`、`CO-06`、`CO-13` | ⏳ | [专项卡](roadmap/companyos.md#step-co-14) |
| 514 | W7 | 专项 | [`CO-15`](roadmap/companyos.md#step-co-15) | CompanyOS · 用现有 Harness 驱动有界角色会议 | `CO-14` | ⏳ | [专项卡](roadmap/companyos.md#step-co-15) |
| 515 | W7 | 专项 | [`CO-16`](roadmap/companyos.md#step-co-16) | CompanyOS · 角色提案进入业务命令，原子批准计划 | `CO-07`、`CO-12`、`CO-13`、`CO-15` | ⏳ | [专项卡](roadmap/companyos.md#step-co-16) |
| 516 | W7 | 专项 | [`CO-17`](roadmap/companyos.md#step-co-17) | CompanyOS · 跨部门交接与 ACK 责任转移 | `CO-13`、`CO-16` | ⏳ | [专项卡](roadmap/companyos.md#step-co-17) |
| 517 | W7 | 专项 | [`CO-18`](roadmap/companyos.md#step-co-18) | CompanyOS · 唯一 ready 谓词与可解释阻塞原因 | `CO-08`、`CO-12`、`CO-17` | ⏳ | [专项卡](roadmap/companyos.md#step-co-18) |
| 518 | W7 | 专项 | [`CO-19`](roadmap/companyos.md#step-co-19) | CompanyOS · 原子 claim、租约 fencing 与执行尝试 | `CO-07`、`CO-18` | ⏳ | [专项卡](roadmap/companyos.md#step-co-19) |
| 519 | W7 | 专项 | [`CO-20`](roadmap/companyos.md#step-co-20) | CompanyOS · Cell 资源预留、提交与回收闭环 | `CO-03`、`CO-19` | ⏳ | [专项卡](roadmap/companyos.md#step-co-20) |
| 520 | W7 | 专项 | [`CO-21`](roadmap/companyos.md#step-co-21) | CompanyOS · 角色任务接入 fresh Run 与明确 Company scope | `CO-04`、`CO-13`、`CO-20` | ⏳ | [专项卡](roadmap/companyos.md#step-co-21) |
| 521 | W7 | 专项 | [`CO-22`](roadmap/companyos.md#step-co-22) | CompanyOS · 确定性 Company ProcessManager | `CO-08`、`CO-16`、`CO-17`、`CO-21` | ⏳ | [专项卡](roadmap/companyos.md#step-co-22) |
| 522 | W7 | 专项 | [`CO-23`](roadmap/companyos.md#step-co-23) | CompanyOS · 持久唤醒队列与意图消费 | `CO-07`、`CO-19`、`CO-22` | ⏳ | [专项卡](roadmap/companyos.md#step-co-23) |
| 523 | W7 | 专项 | [`CO-24`](roadmap/companyos.md#step-co-24) | CompanyOS · 执行结果归集为不可变 EvidenceBundle | `CO-06`、`CO-21`、`CO-23` | ⏳ | [专项卡](roadmap/companyos.md#step-co-24) |
| 524 | W7 | 专项 | [`CO-25`](roadmap/companyos.md#step-co-25) | CompanyOS · 独立 Reviewer 与逐条证据结论 | `CO-03`、`CO-11`、`CO-24` | ⏳ | [专项卡](roadmap/companyos.md#step-co-25) |
| 525 | W7 | 专项 | [`CO-26`](roadmap/companyos.md#step-co-26) | CompanyOS · Packet 级验收与输出接收 | `CO-24`、`CO-25` | ⏳ | [专项卡](roadmap/companyos.md#step-co-26) |
| 526 | W7 | 专项 | [`CO-27`](roadmap/companyos.md#step-co-27) | CompanyOS · Milestone 独立验收，消除阶段依赖等待环 | `CO-12`、`CO-18`、`CO-26` | ⏳ | [专项卡](roadmap/companyos.md#step-co-27) |
| 527 | W7 | 专项 | [`CO-28`](roadmap/companyos.md#step-co-28) | CompanyOS · Project 验收、拒绝与显式豁免 | `CO-05`、`CO-25`、`CO-27` | ⏳ | [专项卡](roadmap/companyos.md#step-co-28) |
| 528 | W7 | 专项 | [`CO-29`](roadmap/companyos.md#step-co-29) | CompanyOS · 有界返工与 successor/attempt 历史 | `CO-19`、`CO-26`、`CO-27`、`CO-28` | ⏳ | [专项卡](roadmap/companyos.md#step-co-29) |
| 529 | W7 | 专项 | [`CO-30`](roadmap/companyos.md#step-co-30) | CompanyOS · ChangeRequest 实际应用到整组基线 | `CO-07`、`CO-11`、`CO-16`、`CO-28`、`CO-29` | ⏳ | [专项卡](roadmap/companyos.md#step-co-30) |
| 530 | W7 | 专项 | [`CO-31`](roadmap/companyos.md#step-co-31) | CompanyOS · 项目暂停、恢复、取消与部门传播 | `CO-03`、`CO-19`、`CO-23`、`CO-30` | ⏳ | [专项卡](roadmap/companyos.md#step-co-31) |
| 531 | W7 | 专项 | [`CO-32`](roadmap/companyos.md#step-co-32) | CompanyOS · 风险、事故、Unknown 与对账工作流 | `CO-24`、`CO-30`、`CO-31` | ⏳ | [专项卡](roadmap/companyos.md#step-co-32) |
| 532 | W7 | 专项 | [`CO-33`](roadmap/companyos.md#step-co-33) | CompanyOS · 版本化 DeliveryManifest 与本地交付包 | `CO-06`、`CO-28`、`CO-32` | ⏳ | [专项卡](roadmap/companyos.md#step-co-33) |
| 533 | W7 | 专项 | [`CO-34`](roadmap/companyos.md#step-co-34) | CompanyOS · 交付授权、效果记录与接收确认 | `CO-05`、`CO-07`、`CO-32`、`CO-33` | ⏳ | [专项卡](roadmap/companyos.md#step-co-34) |
| 534 | W7 | 专项 | [`CO-35`](roadmap/companyos.md#step-co-35) | CompanyOS · 成功、失败、取消和豁免的 ClosingReceipt | `CO-28`、`CO-31`、`CO-32`、`CO-34` | ⏳ | [专项卡](roadmap/companyos.md#step-co-35) |
| 535 | W7 | 专项 | [`CO-36`](roadmap/companyos.md#step-co-36) | CompanyOS · Outcome 测量与目标实现判定 | `CO-09`、`CO-10`、`CO-35` | ⏳ | [专项卡](roadmap/companyos.md#step-co-36) |
| 536 | W7 | 专项 | [`CO-37`](roadmap/companyos.md#step-co-37) | CompanyOS · 部门决议、收尾经验与 Memory 候选晋升 | `CO-15`、`CO-35` | ⏳ | [专项卡](roadmap/companyos.md#step-co-37) |
| 537 | W7 | 专项 | [`CO-38`](roadmap/companyos.md#step-co-38) | CompanyOS · 组织与项目的可重建读模型 | `CO-18`、`CO-22`、`CO-28`、`CO-35`、`CO-36` | ⏳ | [专项卡](roadmap/companyos.md#step-co-38) |
| 538 | W7 | 专项 | [`CO-39`](roadmap/companyos.md#step-co-39) | CompanyOS · 统一 Human Inbox 与有后续动作的决定卡 | `CO-05`、`CO-23`、`CO-28`、`CO-34`、`CO-38` | ⏳ | [专项卡](roadmap/companyos.md#step-co-39) |
| 539 | W7 | 专项 | [`CO-40`](roadmap/companyos.md#step-co-40) | CompanyOS · CLI 与 Workbench 的 Company 用户流程 | `CO-38`、`CO-39` | ⏳ | [专项卡](roadmap/companyos.md#step-co-40) |
| 540 | W7 | 专项 | [`CO-41`](roadmap/companyos.md#step-co-41) | CompanyOS · Web 与 Desktop 复用同一 Company 状态 | `CO-38`、`CO-39`、`CO-40` | ⏳ | [专项卡](roadmap/companyos.md#step-co-41) |
| 541 | W7 | 基础 | [`P1-E-02`](#step-p1-e-02) | P1 基础 · Symposium 会议对象契约化 | `P1-E-01` | ⏳ | [基础卡](#step-p1-e-02) |
| 542 | W7 | 基础 | [`P2-J5-01`](#step-p2-j5-01) | P2 基础 · Workflow definition 与重放 | `P0-G-04` | ⏳ | [基础卡](#step-p2-j5-01) |
| 543 | W7 | 基础 | [`P3-I-03`](#step-p3-i-03) | P3 基础 · 全链重建 | `P3-I-02`、`P0-G-04` | ⏳ | [基础卡](#step-p3-i-03) |
| 544 | W7 | 基础 | [`P3-I-04`](#step-p3-i-04) | P3 基础 · Acceptance 快照与独立 Review | `P3-I-02` | ⏳ | [基础卡](#step-p3-i-04) |
| 545 | W7 | 基础 | [`P3-I-05`](#step-p3-i-05) | P3 基础 · Delivery / ClosingReceipt / Outcome | `P3-I-03` | ⏳ | [基础卡](#step-p3-i-05) |
| 546 | W7 | 基础 | [`P4-E-03`](#step-p4-e-03) | P4 基础 · 五部门开会与决议入部门 RAG | `P1-E-02`、`P1-J3-02`、`P1-J3-03` | ⏳ | [基础卡](#step-p4-e-03) |
| 547 | W7 | 专项 | [`SW-05`](#step-sw-05) | 统一原子 admission；budget/path/data lock/claim/grant/supervision/intent 同一 CAS 边界 | `SW-04`、`CO-19`、`CO-20` | ⏳ | [专项卡](#step-sw-05) |
| 548 | W7 | 专项 | [`SW-06`](#step-sw-06) | durable `DispatchIntent`/`QueueEntry`、容量、公平顺序、claim lease/fence/backoff；`kiana-core`/`kiana-eventlog`/`kiana-ports` | `SW-05`、`P1-D-01`、`P1-D-02`、`P1-D-03` | ⏳ | [专项卡](#step-sw-06) |
| 549 | W7 | 专项 | [`SW-07`](#step-sw-07) | fresh child Cell/Session/Run/Attempt 和 DelegationPacket；daemon/core/runner | `SW-06`、`CO-21`、`CO-23` | ⏳ | [专项卡](#step-sw-07) |
| 550 | W7 | 专项 | [`SW-08`](#step-sw-08) | 统一执行路由和 child correlation；复用 `DaemonHost→ControlPlane→Broker→KianaHarness` | `SW-07`、`H01`、`CP-23`、`CP-24` | ⏳ | [专项卡](#step-sw-08) |
| 551 | W7 | 专项 | [`SW-09`](#step-sw-09) | heartbeat、checkpoint、ProgressLedger、stall/escalation 与分层预算 | `P0-J1-01`、`P0-J1-02`、`P0-J1-03`、`P0-J1-04`、`P2-K4-01`、`P4-J7-02`、`P4-J7-03`、`SW-08` | ⏳ | [专项卡](#step-sw-09) |
| 552 | W7 | 专项 | [`SW-10`](#step-sw-10) | parent→child cancel、drain、fence、Incident/Unknown；core/daemon/process supervisor | `P0-J1-01`、`P0-J1-02`、`P0-J1-03`、`P0-J1-04`、`CP-15`、`CP-16`、`CP-20`、`SW-09` | ⏳ | [专项卡](#step-sw-10) |
| 553 | W7 | 专项 | [`SW-11`](#step-sw-11) | EventLog replay、pending writes、崩溃恢复与显式 re-admission | `P0-G-04`、`CP-18`、`CP-19`、`SW-10` | ⏳ | [专项卡](#step-sw-11) |
| 554 | W7 | 专项 | [`SW-12`](#step-sw-12) | TypedChildResult/ChildFailureReport 与 delegation facts；domain/core/event projection | `SW-11` | ⏳ | [专项卡](#step-sw-12) |
| 555 | W7 | 专项 | [`AUT-13`](#step-aut-13) | `Advance` ready-node、dependency、FanOut/FanIn/SubWorkflow planner；`kiana-workflow` | `AUT-06`、`AUT-07` | ⏳ | [专项卡](#step-aut-13) |
| 556 | W7 | 专项 | [`AUT-14`](#step-aut-14) | effect reservation、permit、action digest、authority/config/policy revision；`kiana-core`、`kiana-eventlog` | `AUT-05`、`AUT-08`、`AUT-13` | ⏳ | [专项卡](#step-aut-14) |
| 557 | W7 | 专项 | [`AUT-15`](#step-aut-15) | worker dispatch/observation 分离；把当前 inline dispatch 改成可重取 command；`kiana-core`、`kiana-daemon` | `AUT-09`、`AUT-14` | ⏳ | [专项卡](#step-aut-15) |
| 558 | W7 | 专项 | [`AUT-16`](#step-aut-16) | retry classifier、attempt/lease/deadline、idempotent descriptor；`kiana-workflow`、`kiana-core` | `AUT-14`、`AUT-15` | ⏳ | [专项卡](#step-aut-16) |
| 559 | W7 | 专项 | [`AUT-17`](#step-aut-17) | cancel generation、stop report、late result fence、result_unknown；`kiana-core`、`kiana-daemon` | `AUT-08`、`AUT-15` | ⏳ | [专项卡](#step-aut-17) |
| 560 | W7 | 专项 | [`AUT-18`](#step-aut-18) | approval/signal pause-resume、single consume、checkpoint metadata；`kiana-core`、`kiana-runner` | `AUT-14`、`AUT-17` | ⏳ | [专项卡](#step-aut-18) |
| 561 | W7 | 专项 | [`AUT-19`](#step-aut-19) | parent-child workflow/WorkPacket fan-out、depth/concurrency/TTL、fail-fast/fan-in；`kiana-workflow`、`kiana-core` | `AUT-07`、`AUT-13`、`AUT-17` | ⏳ | [专项卡](#step-aut-19) |
| 562 | W7 | 专项 | [`AUT-20`](#step-aut-20) | compensation workflow、原实例引用和新授权；`kiana-workflow`、`kiana-core` | `AUT-16`、`AUT-19` | ⏳ | [专项卡](#step-aut-20) |
| 563 | W7 | 专项 | [`AUT-21`](#step-aut-21) | boot recovery、projection/index rebuild、reconcile case；`kiana-daemon`、`kiana-eventlog`、`kiana-core` | `AUT-15`、`AUT-17`、`AUT-18` | ⏳ | [专项卡](#step-aut-21) |
| 564 | W7 | 专项 | [`NM-17`](#step-nm-17) | severity/digest/reminder/escalation；primary/fallback、quiet hours、deadline | `P2-K3-01`、`CO-39`、`NM-06`、`NM-11` | ⏳ | [专项卡](#step-nm-17) |
| 565 | W7 | 专项 | [`NM-18`](#step-nm-18) | cancellation/revocation/expiry/reconciliation；`recovery.rs`、`connectors.rs` | `P2-K6-01`、`NM-08`、`NM-10` | ⏳ | [专项卡](#step-nm-18) |
| 566 | W7 | 专项 | [`NM-20`](#step-nm-20) | fault injection 与容量/安全测试；EventLog/Projector/Worker/Channel/UI 全链 | `NM-04`、`NM-08`、`NM-13`、`NM-18` | ⏳ | [专项卡](#step-nm-20) |
| 567 | W7 | 专项 | [`NM-21`](#step-nm-21) | 跨入口 E2E 与消息/通知/动作对账；CLI/TTY/Web/Desktop、fake model/provider | `CO-39`、`CO-40`、`CO-41`、`NM-14`、`NM-15`、`NM-16`、`NM-20` | ⏳ | [专项卡](#step-nm-21) |
| 568 | W7 | 专项 | [`EQ-38`](#step-eq-38) | 定义 `EvalExperiment` admission/terminal 状态和 case result 索引 | `EQ-37` | ⏳ | [专项卡](#step-eq-38) |
| 569 | W7 | 专项 | [`EQ-39`](#step-eq-39) | 实现 baseline registry：suite/case/target/evaluator digest、owner、expiry、refresh provenance | `EQ-38` | ⏳ | [专项卡](#step-eq-39) |
| 570 | W7 | 专项 | [`EQ-40`](#step-eq-40) | 实现 `QualityCandidate` 单主变更维度和版本快照绑定 | `EQ-39` | ⏳ | [专项卡](#step-eq-40) |
| 571 | W7 | 专项 | [`EQ-41`](#step-eq-41) | 实现 `QualityGate` 配置与 `QualityGateDecision` 裁决分离、不可改写 | `EQ-40` | ⏳ | [专项卡](#step-eq-41) |
| 572 | W7 | 专项 | [`EQ-42`](#step-eq-42) | 实现 blocking rules：safety/evidence/replay/forbidden effect/fixture integrity/infra | `EQ-41` | ⏳ | [专项卡](#step-eq-42) |
| 573 | W7 | 专项 | [`EQ-43`](#step-eq-43) | 实现 `quality.promote`/`quality.rollback` 的 ControlPlane 二次授权、审批、scope 和 epoch 重查 | `EQ-42` | ⏳ | [专项卡](#step-eq-43) |
| 574 | W7 | 专项 | [`EQ-44`](#step-eq-44) | 实现 shadow admission、sample/TTL/rollback route 和自动回滚证据 | `EQ-43` | ⏳ | [专项卡](#step-eq-44) |
| 575 | W7 | 专项 | [`BQ-21`](#step-bq-21) | Restart/continue/replay/recovery；in-flight reservation、unknown attempt 和 fencing | `ER-21`、`BQ-08`、`BQ-12`、`BQ-20` | ⏳ | [专项卡](#step-bq-21) |
| 576 | W7 | 专项 | [`BQ-22`](#step-bq-22) | Provider invoice/receipt import、差异检测、correction workflow | `BQ-05`、`BQ-14`、`BQ-20` | ⏳ | [专项卡](#step-bq-22) |
| 577 | W7 | 专项 | [`BQ-23`](#step-bq-23) | query/protocol API：预算摘要、usage breakdown、cost export、reconciliation inbox | `BQ-20`、`BQ-22` | ⏳ | [专项卡](#step-bq-23) |
| 578 | W7 | 专项 | [`PD-23`](roadmap/persistence-data-layer.md#step-pd-23) | Restore verifier、替换 fencing、外部 effect reconciliation；`kiana-daemon`、`kiana-core` | `ER-23`、`ER-24`、`ER-25`、`ER-26`、`ER-27`、`ER-28`、`PD-22` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-23) |
| 579 | W7 | 专项 | [`INT-24`](roadmap/integrations-connectors.md#step-int-24) | webhook/A2A ingress auth、signature、timestamp、nonce、dedupe | `AUT-11`、`INT-01`、`INT-04`、`INT-07`、`INT-22` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-24) |
| 580 | W7 | 专项 | [`INT-25`](roadmap/integrations-connectors.md#step-int-25) | object mapping、input artifact、schema/provenance、pagination cursor | `INT-03`、`INT-09`、`INT-24` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-25) |
| 581 | W7 | 专项 | [`INT-26`](roadmap/integrations-connectors.md#step-int-26) | DataClass/Purpose/SharingGrant/retention/revocation propagation | `INT-05`、`INT-22`、`INT-25` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-26) |
| 582 | W7 | 专项 | [`INT-27`](roadmap/integrations-connectors.md#step-int-27) | Connector health/invocation/reconcile/approval 的通知投影 | `INT-20`、`INT-22`、`INT-26` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-27) |
| 583 | W7 | 专项 | [`INT-28`](roadmap/integrations-connectors.md#step-int-28) | CLI/Web/Workbench/MCP 查询与人工 reconcile UI | `INT-14`、`INT-22`、`INT-27` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-28) |
| 584 | W7 | 专项 | [`INT-29`](roadmap/integrations-connectors.md#step-int-29) | restart/recovery、projection rebuild、stale worker/lease fencing | `INT-16`、`INT-19`、`INT-22`、`INT-23`、`INT-26` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-29) |
| **W8** | **并行、治理与复用** |  |  |  |  |  |  |
| 585 | W8 | 专项 | [`ER-29`](roadmap/event-receipt-recovery.md#step-er-29) | Event / Receipt / Recovery · Data governance、retention 和 deletion propagation | `ER-28` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-29) |
| 586 | W8 | 专项 | [`ER-30`](roadmap/event-receipt-recovery.md#step-er-30) | Event / Receipt / Recovery · Health、metrics、trace correlation and operator evidence | `ER-29` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-30) |
| 587 | W8 | 专项 | [`H31`](roadmap/harness.md#step-h31) | Harness · 受控子 Agent 的 Harness 接缝 | `H07`、`H09`、`H13`、`H24`、`H27`、`H30` | ⏳ | [专项卡](roadmap/harness.md#step-h31) |
| 588 | W8 | 专项 | [`H33`](roadmap/harness.md#step-h33) | Harness · 运行资源、关闭和异常退出的完整清理 | `H08`、`H17`、`H24`、`H31`、`H32` | ⏳ | [专项卡](roadmap/harness.md#step-h33) |
| 589 | W8 | 专项 | [`H34`](roadmap/harness.md#step-h34) | Harness · 旧协议、cassette 与入口迁移 | `H02`、`H04`、`H19`、`H24`、`H27`、`H32`、`H33` | ⏳ | [专项卡](roadmap/harness.md#step-h34) |
| 590 | W8 | 专项 | [`CO-42`](roadmap/companyos.md#step-co-42) | CompanyOS · 全业务链跨进程恢复与 schema 升级演练 | `CO-08`、`CO-23`、`CO-29`、`CO-30`、`CO-32`、`CO-35`、`CO-39` | ⏳ | [专项卡](roadmap/companyos.md#step-co-42) |
| 591 | W8 | 专项 | [`CO-43`](roadmap/companyos.md#step-co-43) | CompanyOS · 有界多角色/多 Builder 并行 | `CO-18`、`CO-19`、`CO-20`、`CO-21`、`CO-29`、`CO-42` | ⏳ | [专项卡](roadmap/companyos.md#step-co-43) |
| 592 | W8 | 专项 | [`CO-44`](roadmap/companyos.md#step-co-44) | CompanyOS · Integrator、冲突处理与 MergeReceipt | `CO-25`、`CO-26`、`CO-43` | ⏳ | [专项卡](roadmap/companyos.md#step-co-44) |
| 593 | W8 | 专项 | [`CO-45`](roadmap/companyos.md#step-co-45) | CompanyOS · 多项目优先级、容量与组织成本账 | `CO-02`、`CO-20`、`CO-23`、`CO-36`、`CO-38`、`CO-43` | ⏳ | [专项卡](roadmap/companyos.md#step-co-45) |
| 594 | W8 | 专项 | [`CO-46`](roadmap/companyos.md#step-co-46) | CompanyOS · 版本化流程模板、组织配置升级与第二种业务样例 | `CO-04`、`CO-13`、`CO-22`、`CO-37`、`CO-42`、`CO-45` | ⏳ | [专项卡](roadmap/companyos.md#step-co-46) |
| 595 | W8 | 基础 | [`P4-J6-01`](#step-p4-j6-01) | P4 基础 · 有界 Swarm | `P1-C-02` | ⏳ | [基础卡](#step-p4-j6-01) |
| 596 | W8 | 基础 | [`P4-K2-01`](#step-p4-k2-01) | P4 基础 · 触发器与调度 | `P0-B-01` | ⏳ | [基础卡](#step-p4-k2-01) |
| 597 | W8 | 基础 | [`P4-K8-01`](#step-p4-k8-01) | P4 基础 · Connector | `P0-A-01a` | ⏳ | [基础卡](#step-p4-k8-01) |
| 598 | W8 | 专项 | [`SW-13`](#step-sw-13) | versioned deterministic reducer、冲突和完整覆盖检查 | `CO-43`、`CO-44`、`SW-12` | ⏳ | [专项卡](#step-sw-13) |
| 599 | W8 | 专项 | [`SW-14`](#step-sw-14) | Independent Review、MergeDecision、MergeReceipt；`company` review + core swarm | `P3-I-04`、`CO-44`、`SW-13` | ⏳ | [专项卡](#step-sw-14) |
| 600 | W8 | 专项 | [`SW-15`](#step-sw-15) | exactly-once release/retire、残余预算与事实保留 | `SW-14` | ⏳ | [专项卡](#step-sw-15) |
| 601 | W8 | 专项 | [`SW-16`](#step-sw-16) | 定向 WorkPacket/Handoff ACK/StatusReport/Evidence/Incident；可选 bounded Symposium | `P1-E-01`、`P1-E-02`、`P4-E-03`、`SW-15` | ⏳ | [专项卡](#step-sw-16) |
| 602 | W8 | 专项 | [`SW-17`](#step-sw-17) | parent-child UI/event projection、sequence/epoch/cursor、terminal replay/hydration | `P4-J7-02`、`P4-J7-03`、`P2-M5-01`、`P2-M5-02`、`SW-16` | ⏳ | [专项卡](#step-sw-17) |
| 603 | W8 | 专项 | [`SW-18`](#step-sw-18) | deny-first 全矩阵、fake golden、崩溃/竞态/replay 和发布证据 | `P4-J6-01`、`CO-42`、`CO-43`、`CO-44`、`SW-17` | ⏳ | [专项卡](#step-sw-18) |
| 604 | W8 | 专项 | [`NM-19`](#step-nm-19) | 外部 Connector/webhook contract（默认关闭）；签名、allowlist、nonce、provider receipt | `P4-K8-01`、`NM-08`、`NM-18` | ⏳ | [专项卡](#step-nm-19) |
| 605 | W8 | 专项 | [`DEP-00`](#step-dep-00) | 盘点 `module-map`、`CURRENT_STATUS`、release scripts、DaemonHost、EventLog、现有 schema/migration/WIP；建立 source snapshot 与缺口分类 | — | ⏳ | [专项卡](#step-dep-00) |
| 606 | W8 | 专项 | [`DEP-01`](#step-dep-01) | 在 `kiana-domain` 定义 `DeploymentProfile`、`EnvironmentProfile`、`StorageRootId`、`InstanceId`、`DeploymentRevision` | `DEP-00` | ⏳ | [专项卡](#step-dep-01) |
| 607 | W8 | 专项 | [`DEP-02`](#step-dep-02) | 定义 `ReleaseManifest`、artifact digest/signature、build/toolchain/Cargo.lock/source provenance | `DEP-00` | ⏳ | [专项卡](#step-dep-02) |
| 608 | W8 | 专项 | [`DEP-03`](#step-dep-03) | 定义 app/protocol/domain/store/projection/workflow/provider/extension/config/authority/data 兼容矩阵 | `DEP-01`、`DEP-02` | ⏳ | [专项卡](#step-dep-03) |
| 609 | W8 | 专项 | [`DEP-04`](#step-dep-04) | 在 `kiana-protocol` 注册 `ops.*` commands、query、events、error/unknown envelope、idempotency key | `DEP-01` | ⏳ | [专项卡](#step-dep-04) |
| 610 | W8 | 专项 | [`DEP-05`](#step-dep-05) | 定义 lifecycle/operation 状态机、OperationJournal、phase deadlines、terminal/unknown 语义 | `DEP-04` | ⏳ | [专项卡](#step-dep-05) |
| 611 | W8 | 专项 | [`DEP-06`](#step-dep-06) | 实现 `OperationLease`、heartbeat、fence token、authority/data epoch 与单 writer CAS | `DEP-01`、`DEP-05` | ⏳ | [专项卡](#step-dep-06) |
| 612 | W8 | 专项 | [`DEP-07`](#step-dep-07) | 实现 StorageRoot resolver、ProjectTrust、symlink/hardlink/path/capability/filesystem preflight | `DEP-01`、`DEP-06` | ⏳ | [专项卡](#step-dep-07) |
| 613 | W8 | 专项 | [`DEP-08`](#step-dep-08) | 实现 ConfigSource 优先级、immutable `ConfigSnapshot`、config revision、SecretRef/redaction | `DEP-01`、`DEP-02`、`DEP-07` | ⏳ | [专项卡](#step-dep-08) |
| 614 | W8 | 专项 | [`DEP-09`](#step-dep-09) | 定义 `SupervisorPort`，接入 systemd/launchd/Windows/container stop/start/restart 的窄 adapter | `DEP-04`、`DEP-05`、`DEP-06` | ⏳ | [专项卡](#step-dep-09) |
| 615 | W8 | 专项 | [`DEP-10`](#step-dep-10) | 在 `DaemonHost` 内实现 startup coordinator：manifest/root/trust/lease/store/migration/projector/capacity 顺序 | `DEP-03`、`DEP-06`、`DEP-07`、`DEP-08` | ⏳ | [专项卡](#step-dep-10) |
| 616 | W8 | 专项 | [`DEP-11`](#step-dep-11) | 实现 `HealthSnapshot` aggregator 与 startup/live/ready/drain/maintenance probe DTO | `DEP-05`、`DEP-10` | ⏳ | [专项卡](#step-dep-11) |
| 617 | W8 | 专项 | [`DEP-12`](#step-dep-12) | 实现 ready admission、maintenance window、pause intake、drain deadline 与摘流语义 | `DEP-05`、`DEP-06`、`DEP-11` | ⏳ | [专项卡](#step-dep-12) |
| 618 | W8 | 专项 | [`PD-26`](roadmap/persistence-data-layer.md#step-pd-26) | data revocation/delete/expire 到 facts、Artifact、Memory、Index、cache、checkpoint 的传播；`kiana-core` | `ER-29`、`PD-18`、`PD-21`、`PD-25` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-26) |
| 619 | W8 | 专项 | [`PD-27`](roadmap/persistence-data-layer.md#step-pd-27) | 多进程 writer、backpressure、flush/shutdown、取消和资源上限；`kiana-eventlog`、`kiana-daemon` | `ER-30`、`PD-06`、`PD-09` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-27) |
| 620 | W8 | 专项 | [`SC-32`](roadmap/security-compliance.md#step-sc-32) | kiana-query audit projector、CAS/cursor/rebuild | `PD-09`、`PD-26`、`SC-12`、`SC-23`、`SC-31` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-32) |
| 621 | W8 | 专项 | [`SC-33`](roadmap/security-compliance.md#step-sc-33) | kiana-core Incident、Vulnerability、Reconcile workflow | `SC-15`、`SC-22`、`SC-31`、`SC-32` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-33) |
| 622 | W8 | 专项 | [`SC-34`](roadmap/security-compliance.md#step-sc-34) | docs control crosswalk、policy registry | `SC-01`、`SC-05`、`SC-31`、`SC-33` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-34) |
| 623 | W8 | 专项 | [`SC-35`](roadmap/security-compliance.md#step-sc-35) | scripts EvidenceManifest、fixture/cassette registry | `SC-29`、`SC-31`、`SC-34` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-35) |
| **W9** | **离线联合验收** |  |  |  |  |  |  |
| 624 | W9 | 专项 | [`CP-29`](roadmap/control-plane.md#step-cp-29) | ControlPlane · 状态机性质、并发与崩溃验收 | `CP-05`、`CP-07`、`CP-13`、`CP-14`、`CP-15`、`CP-16`、`CP-17`、`CP-18`、`CP-19`、`CP-20`、`CP-23`、`CP-24`、`CP-25`、`CP-26`、`CP-27`、`CP-28` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-29) |
| 625 | W9 | 专项 | [`CP-30`](roadmap/control-plane.md#step-cp-30) | ControlPlane · 产品流程和证据收口 | `CP-21`、`CP-22`、`CP-23`、`CP-24`、`CP-25`、`CP-26`、`CP-27`、`CP-28`、`CP-29` | ⏳ | [专项卡](roadmap/control-plane.md#step-cp-30) |
| 626 | W9 | 专项 | [`ER-31`](roadmap/event-receipt-recovery.md#step-er-31) | Event / Receipt / Recovery · Crash-point and fault-injection matrix | `ER-30` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-31) |
| 627 | W9 | 专项 | [`ER-32`](roadmap/event-receipt-recovery.md#step-er-32) | Event / Receipt / Recovery · Property/conformance tests for adapters | `ER-31` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-32) |
| 628 | W9 | 专项 | [`ER-33`](roadmap/event-receipt-recovery.md#step-er-33) | Event / Receipt / Recovery · Performance、容量和迁移演练 | `ER-32` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-33) |
| 629 | W9 | 专项 | [`ER-34`](roadmap/event-receipt-recovery.md#step-er-34) | Event / Receipt / Recovery · Local durable gate | `ER-33` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-34) |
| 630 | W9 | 专项 | [`ER-35`](roadmap/event-receipt-recovery.md#step-er-35) | Event / Receipt / Recovery · Cross-entry and CompanyOS gate | `ER-34` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-35) |
| 631 | W9 | 专项 | [`CAP-26`](roadmap/capability.md#step-cap-26) | Capability · 本地五工具 + Hook 的完整执行闭环验收 | `CAP-14`、`CAP-15`、`CAP-16`、`CAP-17`、`CAP-18`、`CAP-19`、`CAP-20`、`CAP-21`、`CAP-22`、`CAP-23`、`CAP-24`、`CAP-25` | ⏳ | [专项卡](roadmap/capability.md#step-cap-26) |
| 632 | W9 | 专项 | [`H35`](roadmap/harness.md#step-h35) | Harness · Harness 轨迹评测与性能验证 | `H25`、`H27`、`H28`、`H30`、`H34` | ⏳ | [专项卡](roadmap/harness.md#step-h35) |
| 633 | W9 | 专项 | [`P4-J7-29`](roadmap/provider.md#step-p4-j7-29) | Provider · 离线合同矩阵、属性测试与故障语料 | `P4-J7-20`、`P4-J7-21`、`P4-J7-22`、`P4-J7-25`、`P4-J7-27` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-29) |
| 634 | W9 | 专项 | [`P4-J7-30`](roadmap/provider.md#step-p4-j7-30) | Provider · 产品链和四表面回归 | `P4-J7-28`、`P4-J7-29`、`P0-M1-01` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-30) |
| 635 | W9 | 专项 | [`CM-30`](roadmap/context-memory.md#step-cm-30) | Context / Memory · Golden ContextPlan / retrieval fixture | `CM-29` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-30) |
| 636 | W9 | 专项 | [`CM-31`](roadmap/context-memory.md#step-cm-31) | Context / Memory · Retrieval quality and safety evaluation | `CM-30` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-31) |
| 637 | W9 | 专项 | [`CM-32`](roadmap/context-memory.md#step-cm-32) | Context / Memory · Context/Memory Inspector 与用户纠正 | `CM-31` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-32) |
| 638 | W9 | 专项 | [`CM-33`](roadmap/context-memory.md#step-cm-33) | Context / Memory · Code graph / temporal fact 后置扩展 | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-33) |
| 639 | W9 | 专项 | [`CM-34`](roadmap/context-memory.md#step-cm-34) | Context / Memory · Local embedding package and model rotation | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-34) |
| 640 | W9 | 专项 | [`CM-35`](roadmap/context-memory.md#step-cm-35) | Context / Memory · External context/resource adapter | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-35) |
| 641 | W9 | 专项 | [`CM-36`](roadmap/context-memory.md#step-cm-36) | Context / Memory · User memory workbench and bulk operations | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-36) |
| 642 | W9 | 专项 | [`CM-37`](roadmap/context-memory.md#step-cm-37) | Context / Memory · Cache, index and retention maintenance | `CM-06`、`CM-14`、`CM-28`、`CM-32` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-37) |
| 643 | W9 | 专项 | [`EXT-29`](roadmap/skills-plugins-hooks.md#step-ext-29) | Skills / Plugins / Hooks · 本地 fake golden | `EXT-10`、`EXT-18`、`EXT-25`、`EXT-28` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-29) |
| 644 | W9 | 专项 | [`EXT-30`](roadmap/skills-plugins-hooks.md#step-ext-30) | Skills / Plugins / Hooks · Durable、故障注入和恢复 | `EXT-18`、`EXT-23`、`EXT-24`、`EXT-29` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-30) |
| 645 | W9 | 专项 | [`EXT-31`](roadmap/skills-plugins-hooks.md#step-ext-31) | Skills / Plugins / Hooks · 性能、供应链回归和文档收口 | `EXT-29`、`EXT-30` | ⏳ | [专项卡](roadmap/skills-plugins-hooks.md#step-ext-31) |
| 646 | W9 | 专项 | [`UI-31`](roadmap/ui-entrypoints.md#step-ui-31) | UI / Entrypoints · CLI/Workbench/Web/Desktop 行为 parity | `UI-10`、`UI-11`、`UI-12`、`UI-13`、`UI-14`、`UI-15`、`UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22`、`UI-23`、`UI-24`、`UI-25`、`UI-26`、`UI-27`、`UI-28`、`UI-29`、`UI-30` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-31) |
| 647 | W9 | 专项 | [`UI-32`](roadmap/ui-entrypoints.md#step-ui-32) | UI / Entrypoints · deny-first 安全路径集成测试 | `UI-04`、`UI-16`、`UI-19`、`UI-21`、`UI-23`、`UI-24`、`UI-30`、`UI-31` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-32) |
| 648 | W9 | 专项 | [`UI-33`](roadmap/ui-entrypoints.md#step-ui-33) | UI / Entrypoints · reconnect/replay/gap/crash recovery e2e | `UI-05`、`UI-06`、`UI-07`、`UI-08`、`UI-18`、`UI-25` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-33) |
| 649 | W9 | 专项 | [`UI-34`](roadmap/ui-entrypoints.md#step-ui-34) | UI / Entrypoints · 性能、资源上限和可访问性验收 | `UI-13`、`UI-20`、`UI-23`、`UI-28`、`UI-33` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-34) |
| 650 | W9 | 专项 | [`UI-35`](roadmap/ui-entrypoints.md#step-ui-35) | UI / Entrypoints · 旧 Web/CLI 迁移与兼容收口 | `UI-10`、`UI-11`、`UI-12`、`UI-13`、`UI-14`、`UI-15`、`UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22`、`UI-23`、`UI-31` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-35) |
| 651 | W9 | 专项 | [`UI-36`](roadmap/ui-entrypoints.md#step-ui-36) | UI / Entrypoints · 生产构建、安装和发布前 smoke | `UI-28`、`UI-34`、`UI-35` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-36) |
| 652 | W9 | 专项 | [`UI-37`](roadmap/ui-entrypoints.md#step-ui-37) | UI / Entrypoints · 用户文档、模块图和操作 runbook | `UI-31`、`UI-32`、`UI-33`、`UI-34`、`UI-35`、`UI-36` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-37) |
| 653 | W9 | 专项 | [`UI-38`](roadmap/ui-entrypoints.md#step-ui-38) | UI / Entrypoints · 协议/入口 conformance 集成门 | `UI-01`、`UI-02`、`UI-03`、`UI-04`、`UI-05`、`UI-06`、`UI-07`、`UI-08`、`UI-09`、`UI-10`、`UI-11`、`UI-12`、`UI-13`、`UI-14`、`UI-15`、`UI-16`、`UI-17`、`UI-18`、`UI-19`、`UI-20`、`UI-21`、`UI-22`、`UI-23`、`UI-24`、`UI-25`、`UI-26`、`UI-27`、`UI-28`、`UI-29`、`UI-30`、`UI-31`、`UI-32`、`UI-33`、`UI-34`、`UI-35`、`UI-36`、`UI-37` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-38) |
| 654 | W9 | 专项 | [`CO-47`](roadmap/companyos.md#step-co-47) | CompanyOS · fake-model Company 黄金闭环与故障矩阵 | `CO-35`、`CO-36`、`CO-37`、`CO-40`、`CO-41`、`CO-42`、`CO-44`、`CO-46` | ⏳ | [专项卡](roadmap/companyos.md#step-co-47) |
| 655 | W9 | 基础 | [`P1-L1-01`](#step-p1-l1-01) | P1 基础 · EvalSuite 与 GoldenTrace | `P0-G-04` | ⏳ | [基础卡](#step-p1-l1-01) |
| 656 | W9 | 基础 | [`P2-L2-01`](#step-p2-l2-01) | P2 基础 · 反馈与候选改进 | `P1-L1-01` | ⏳ | [基础卡](#step-p2-l2-01) |
| 657 | W9 | 基础 | [`P3-I-06`](#step-p3-i-06) | P3 基础 · fake-model coding 黄金闭环 | `P3-I-05` | ⏳ | [基础卡](#step-p3-i-06) |
| 658 | W9 | 基础 | [`P4-L3-01`](#step-p4-l3-01) | P4 基础 · 版本治理与 drift | `P1-L1-01` | ⏳ | [基础卡](#step-p4-l3-01) |
| 659 | W9 | 专项 | [`CI-12`](#step-ci-12) | 产品链 deny-first/UAT 与发布证据收口；CLI/Web/Workbench/Desktop、fake provider、live opt-in | `CI-01`、`CI-11`、`CI-02`、`CI-03`、`CI-04`、`CI-05`、`CI-06`、`CI-07`、`CI-08`、`CI-09`、`CI-10` | ⏳ | [专项卡](#step-ci-12) |
| 660 | W9 | 专项 | [`OA-23`](#step-oa-23) | Provider-independent eval suite；fake model/provider/broker、GoldenTrace、Promptfoo 风格断言 | `P1-L1-01`、`OA-08`、`OA-09`、`OA-21` | ✅ | [专项卡](#step-oa-23) |
| 661 | W9 | 专项 | [`OA-24`](#step-oa-24) | 四入口审计/健康/Receipt parity；CLI/Web/Workbench/Desktop | `P2-M2-01`、`P2-M3-01`、`P2-M4-01`、`P2-M5-01`、`P2-M5-02`、`OA-16`、`OA-23`、`OA-17`、`OA-18`、`OA-19`、`OA-20`、`OA-21`、`OA-22` | ✅ | [专项卡](#step-oa-24) |
| 662 | W9 | 专项 | [`OA-25`](#step-oa-25) | 容量、性能和迁移演练；journal/projector/query/export benchmark | `OA-12`、`OA-13`、`OA-17`、`OA-20` | ✅ | [专项卡](#step-oa-25) |
| 663 | W9 | 专项 | [`OA-26`](#step-oa-26) | Local durable observability gate；release/smoke/CURRENT_STATUS | `ER-34`、`OA-22`、`OA-25`、`OA-23`、`OA-24` | ✅ | [专项卡](#step-oa-26) |
| 664 | W9 | 专项 | [`OA-27`](#step-oa-27) | Cross-entry/company governance gate；`ER-35`、CompanyOS Review/Delivery/Close、Cost/Memory/Data governance | `OA-24`、`OA-26`、`OA-25` | ✅ | [专项卡](#step-oa-27) |
| 665 | W9 | 专项 | [`AUT-22`](#step-aut-22) | scheduler/workflow/trigger snapshot、Receipt、incident query/UI adapter；`kiana-query`、`kiana-protocol` | `AUT-05`、`AUT-21` | ⏳ | [专项卡](#step-aut-22) |
| 666 | W9 | 专项 | [`AUT-23`](#step-aut-23) | 跨 CLI/Web/Workbench/Desktop/MCP 的同一 DaemonHost 端到端 UAT；entrypoints、daemon、core | `AUT-09`、`AUT-11`、`AUT-18`、`AUT-22` | ⏳ | [专项卡](#step-aut-23) |
| 667 | W9 | 专项 | [`AUT-24`](#step-aut-24) | durable/live 证据与发布门；scripts、fixtures、`CURRENT_STATUS.md` | `AUT-01`、`AUT-23`、`AUT-02`、`AUT-03`、`AUT-04`、`AUT-05`、`AUT-06`、`AUT-07`、`AUT-08`、`AUT-09`、`AUT-10`、`AUT-11`、`AUT-12`、`AUT-13`、`AUT-14`、`AUT-15`、`AUT-16`、`AUT-17`、`AUT-18`、`AUT-19`、`AUT-20`、`AUT-21`、`AUT-22` | ⏳ | [专项卡](#step-aut-24) |
| 668 | W9 | 专项 | [`EQ-45`](#step-eq-45) | 实现 `quality.feedback`，只引用 canonical target，服务端派生 provenance/privacy scope | `EQ-44` | ⏳ | [专项卡](#step-eq-45) |
| 669 | W9 | 专项 | [`EQ-46`](#step-eq-46) | 实现版本分桶 drift metrics、告警和 `drift.alerted` 事件 | `EQ-45` | ⏳ | [专项卡](#step-eq-46) |
| 670 | W9 | 专项 | [`EQ-47`](#step-eq-47) | 扩展 `kiana eval`：`run/capture/compare/explain/list`，旧 `run --suite` 参数保持兼容 | `EQ-46` | ⏳ | [专项卡](#step-eq-47) |
| 671 | W9 | 专项 | [`EQ-48`](#step-eq-48) | 输出 JSON report、JUnit、human summary、evidence manifest、reproduction command；路径和 secret 脱敏 | `EQ-47` | ⏳ | [专项卡](#step-eq-48) |
| 672 | W9 | 专项 | [`EQ-49`](#step-eq-49) | 新增 `scripts/eval-curated.sh`、`eval-deep.sh`、`eval-capture-golden.sh`、`eval-compare.sh` | `EQ-48` | ⏳ | [专项卡](#step-eq-49) |
| 673 | W9 | 专项 | [`EQ-50`](#step-eq-50) | 接入 PR curated/package、nightly deep、release candidate workflow，daemon/core 测试串行 | `EQ-49` | ⏳ | [专项卡](#step-eq-50) |
| 674 | W9 | 专项 | [`EQ-51`](#step-eq-51) | 归档 report/trace-diff/evidence/reproduction，生成 `CURRENT_STATUS.md` 证据块 | `EQ-50` | ⏳ | [专项卡](#step-eq-51) |
| 675 | W9 | 专项 | [`BQ-24`](#step-bq-24) | UI/入口展示与命令；只读预算卡、队列、超额原因、correction approval | `UI-00`、`BQ-23` | ⏳ | [专项卡](#step-bq-24) |
| 676 | W9 | 专项 | [`BQ-25`](#step-bq-25) | Redaction、DataClass、telemetry separation；Event/Log/Metric/Trace/Receipt 安全 | `OA-08`、`BQ-11`、`BQ-13`、`BQ-23` | ⏳ | [专项卡](#step-bq-25) |
| 677 | W9 | 专项 | [`BQ-26`](#step-bq-26) | 并发、崩溃、磁盘满、网络 EOF、provider 429/5xx、clock fault 注入 | `BQ-08`、`BQ-12`、`BQ-16`、`BQ-20` | ⏳ | [专项卡](#step-bq-26) |
| 678 | W9 | 专项 | [`BQ-27`](#step-bq-27) | Legacy usage/cassette/config upcaster 和迁移；旧 `CostLedger`/`Quota` 字段 | `BQ-01`、`BQ-02`、`BQ-04`、`BQ-20` | ⏳ | [专项卡](#step-bq-27) |
| 679 | W9 | 专项 | [`BQ-28`](#step-bq-28) | 性能、容量、retention 和归档；usage/index/receipt 保留边界 | `BQ-20`、`BQ-25`、`BQ-26` | ⏳ | [专项卡](#step-bq-28) |
| 680 | W9 | 专项 | [`BQ-29`](#step-bq-29) | GoldenTrace 与跨入口端到端：model→tool→event→receipt→invoice correction | `BQ-21`、`BQ-22`、`BQ-23`、`BQ-24`、`BQ-26`、`BQ-27` | ⏳ | [专项卡](#step-bq-29) |
| 681 | W9 | 专项 | [`BQ-30`](#step-bq-30) | 发布门、状态账本和逐连接 live 证据；更新 `CURRENT_STATUS.md`/`module-map.md` | `BQ-00`、`BQ-29`、`BQ-01`、`BQ-02`、`BQ-03`、`BQ-04`、`BQ-05`、`BQ-06`、`BQ-07`、`BQ-08`、`BQ-09`、`BQ-10`、`BQ-11`、`BQ-12`、`BQ-13`、`BQ-14`、`BQ-15`、`BQ-16`、`BQ-17`、`BQ-18`、`BQ-19`、`BQ-20`、`BQ-21`、`BQ-22`、`BQ-23`、`BQ-24`、`BQ-25`、`BQ-26`、`BQ-27`、`BQ-28` | ⏳ | [专项卡](#step-bq-30) |
| 682 | W9 | 专项 | [`DEP-13`](#step-dep-13) | 把 cancellation、scheduler stop、runner/tool drain、EventStore/artifact flush ack 接入统一 shutdown | `DEP-09`、`DEP-12` | ⏳ | [专项卡](#step-dep-13) |
| 683 | W9 | 专项 | [`DEP-14`](#step-dep-14) | 接入 lifecycle/operation metrics、structured logs、trace/evidence refs 和 audit event schema | `DEP-04`、`DEP-05`、`DEP-11` | ⏳ | [专项卡](#step-dep-14) |
| 684 | W9 | 专项 | [`DEP-15`](#step-dep-15) | 实现 `ops status/doctor/preflight`，输出 redacted diagnostics、remediation 和 reproduction command | `DEP-08`、`DEP-11`、`DEP-14` | ⏳ | [专项卡](#step-dep-15) |
| 685 | W9 | 专项 | [`DEP-16`](#step-dep-16) | 实现 projector/index/queue/lease repair 与 `ops reconcile` 只读检查/显式提交 | `DEP-05`、`DEP-06`、`DEP-10`、`DEP-14` | ⏳ | [专项卡](#step-dep-16) |
| 686 | W9 | 专项 | [`DEP-17`](#step-dep-17) | 实现 append/artifact/operation/log/diagnostic/migration capacity、backpressure 和 shutdown limits | `DEP-10`、`DEP-13`、`DEP-14` | ⏳ | [专项卡](#step-dep-17) |
| 687 | W9 | 专项 | [`DEP-18`](#step-dep-18) | 建立 incident schema、runbook refs、phase deadline/alert routing 和 `RunbookEvidence` | `DEP-14`、`DEP-15`、`DEP-16` | ⏳ | [专项卡](#step-dep-18) |
| 688 | W9 | 专项 | [`DEP-19`](#step-dep-19) | 在 `kiana-eventlog`/`kiana-ports` 定义 `BackupManifest`、hash/chunk、cursor/generation/epoch、artifact/config refs | `DEP-02`、`DEP-07`、`DEP-14` | ⏳ | [专项卡](#step-dep-19) |
| 689 | W9 | 专项 | [`DEP-20`](#step-dep-20) | 实现 quiesce snapshot：EventLog JSONL/WAL、ArtifactStore、projection checkpoint、migration registry 的一致快照 | `DEP-12`、`DEP-13`、`DEP-19` | ⏳ | [专项卡](#step-dep-20) |
| 690 | W9 | 专项 | [`DEP-21`](#step-dep-21) | 实现 incremental backup、retention、legal hold、archive、encryption/key ref 和删除依赖图 | `DEP-19`、`DEP-20` | ⏳ | [专项卡](#step-dep-21) |
| 691 | W9 | 专项 | [`DEP-22`](#step-dep-22) | 实现 restore quarantine root、manifest/signature/hash/schema/cursor/epoch 校验和 projector/index rebuild | `DEP-19`、`DEP-20`、`DEP-21` | ⏳ | [专项卡](#step-dep-22) |
| 692 | W9 | 专项 | [`DEP-23`](#step-dep-23) | 实现 restore activation、new lease/fence、old root read-only、activation readiness gate | `DEP-06`、`DEP-11`、`DEP-22` | ⏳ | [专项卡](#step-dep-23) |
| 693 | W9 | 专项 | [`DEP-24`](#step-dep-24) | 建立 crash/restore/backup fault fixtures，测量 RPO/RTO 并生成演练证据 | `DEP-20`、`DEP-22`、`DEP-23` | ⏳ | [专项卡](#step-dep-24) |
| 694 | W9 | 专项 | [`DEP-25`](#step-dep-25) | 实现 external effect receipt lookup、idempotency key、Unknown reconciliation 与 compensation gate | `DEP-13`、`DEP-16`、`DEP-23` | ⏳ | [专项卡](#step-dep-25) |
| 695 | W9 | 专项 | [`DEP-26`](#step-dep-26) | 把 retention/deletion/revocation 与 PD-25/PD-26、audit/backup/artifact/memory 生命周期接通 | `DEP-18`、`DEP-21`、`DEP-25` | ⏳ | [专项卡](#step-dep-26) |
| 696 | W9 | 专项 | [`PD-28`](roadmap/persistence-data-layer.md#step-pd-28) | 路径/权限/secret redaction/可选加密/文件身份安全边界；`kiana-core`、`kiana-eventlog` | `CI-07`、`CI-11`、`PD-14` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-28) |
| 697 | W9 | 专项 | [`PD-29`](roadmap/persistence-data-layer.md#step-pd-29) | storage health、projection lag、backup/migration/retention metrics 和诊断 DTO；`kiana-core`、`kiana-daemon` | `PD-03`、`PD-09`、`PD-22`、`PD-25` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-29) |
| 698 | W9 | 专项 | [`PD-30`](roadmap/persistence-data-layer.md#step-pd-30) | 故障注入矩阵：kill-9、磁盘满、权限、锁争用、坏帧、崩溃时机、网络 Unknown；各 adapter tests | `PD-05`、`PD-06`、`PD-07`、`PD-08`、`PD-09`、`PD-10`、`PD-11`、`PD-12`、`PD-13`、`PD-14`、`PD-15`、`PD-16`、`PD-17`、`PD-18`、`PD-19`、`PD-20`、`PD-21`、`PD-22`、`PD-23`、`PD-24`、`PD-25`、`PD-26`、`PD-27`、`PD-28`、`PD-29` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-30) |
| 699 | W9 | 专项 | [`PD-31`](roadmap/persistence-data-layer.md#step-pd-31) | Memory/JSONL/durable/未来 SQLite adapter conformance；`kiana-eventlog/tests`、`kiana-ports` | `PD-05`、`PD-09`、`PD-22` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-31) |
| 700 | W9 | 专项 | [`NM-22`](#step-nm-22) | 发布门、证据和 `CURRENT_STATUS` 回填；feature/proof 分离 | `ER-34`、`PD-30`、`PD-31`、`UI-31`、`UI-32`、`UI-33`、`NM-01`、`NM-21`、`NM-02`、`NM-03`、`NM-04`、`NM-05`、`NM-06`、`NM-07`、`NM-08`、`NM-09`、`NM-10`、`NM-11`、`NM-12`、`NM-13`、`NM-14`、`NM-15`、`NM-16`、`NM-17`、`NM-18`、`NM-19`、`NM-20` | ⏳ | [专项卡](#step-nm-22) |
| 701 | W9 | 专项 | [`PD-32`](roadmap/persistence-data-layer.md#step-pd-32) | 平台和文件系统矩阵：Linux ext4/tmpfs、Windows/macOS fallback、网络 FS、时钟/编码 | `PD-06`、`PD-14`、`PD-27`、`PD-28` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-32) |
| 702 | W9 | 专项 | [`INT-30`](roadmap/integrations-connectors.md#step-int-30) | adapter/registry/protocol conformance 和 property tests | `INT-02`、`INT-03`、`INT-04`、`INT-05`、`INT-06`、`INT-07`、`INT-08`、`INT-09`、`INT-10`、`INT-11`、`INT-12`、`INT-13`、`INT-14`、`INT-15`、`INT-16`、`INT-17`、`INT-18`、`INT-19`、`INT-20`、`INT-21`、`INT-22`、`INT-23`、`INT-24`、`INT-25`、`INT-26`、`INT-27`、`INT-28`、`INT-29` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-30) |
| 703 | W9 | 专项 | [`INT-31`](roadmap/integrations-connectors.md#step-int-31) | 一个只读外部 connector pilot（隔离账号，默认关闭） | `INT-09`、`INT-11`、`INT-12`、`INT-20`、`INT-30` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-31) |
| 704 | W9 | 专项 | [`INT-32`](roadmap/integrations-connectors.md#step-int-32) | 一个受控写 connector pilot（每 operation 独立） | `INT-15`、`INT-16`、`INT-18`、`INT-20`、`INT-21`、`INT-22`、`INT-23`、`INT-31` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-32) |
| 705 | W9 | 专项 | [`INT-33`](roadmap/integrations-connectors.md#step-int-33) | 发布门、live/physical 证据和 `CURRENT_STATUS` 收口 | `INT-00`、`INT-01`、`INT-02`、`INT-03`、`INT-04`、`INT-05`、`INT-06`、`INT-07`、`INT-08`、`INT-09`、`INT-10`、`INT-11`、`INT-12`、`INT-13`、`INT-14`、`INT-15`、`INT-16`、`INT-17`、`INT-18`、`INT-19`、`INT-20`、`INT-21`、`INT-22`、`INT-23`、`INT-24`、`INT-25`、`INT-26`、`INT-27`、`INT-28`、`INT-29`、`INT-30`、`INT-31`、`INT-32` | ⏳ | [专项卡](roadmap/integrations-connectors.md#step-int-33) |
| 706 | W9 | 专项 | [`SC-36`](roadmap/security-compliance.md#step-sc-36) | CLI/Workbench/Web/Desktop/daemon protocol parity | `SC-11`、`SC-32`、`SC-35` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-36) |
| 707 | W9 | 专项 | [`SC-37`](roadmap/security-compliance.md#step-sc-37) | crate negative tests、integration fixtures | `SC-12`、`SC-13`、`SC-15`、`SC-18`、`SC-21`、`SC-31` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-37) |
| 708 | W9 | 专项 | [`SC-38`](roadmap/security-compliance.md#step-sc-38) | property/fuzz/serialization/replay tests | `SC-02`、`SC-05`、`SC-09`、`SC-12`、`SC-31` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-38) |
| 709 | W9 | 专项 | [`SC-39`](roadmap/security-compliance.md#step-sc-39) | security red-team/eval fixtures | `SC-01`、`SC-20`、`SC-24`、`SC-26`、`SC-30` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-39) |
| 710 | W9 | 专项 | [`SC-40`](roadmap/security-compliance.md#step-sc-40) | capacity/resource fault injection | `SC-16`、`SC-22`、`SC-32` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-40) |
| **W10** | **后置平台与能力扩展** |  |  |  |  |  |  |
| 711 | W10 | 专项 | [`CAP-27`](roadmap/capability.md#step-cap-27) | Capability · 长任务、process handle 与 PTY | `CAP-12`、`CAP-13`、`CAP-17`、`CAP-23`、`CAP-25`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-27) |
| 712 | W10 | 专项 | [`CAP-28`](roadmap/capability.md#step-cap-28) | Capability · 受控 egress 与最小凭据注入 | `CAP-03`、`CAP-06`、`CAP-11`、`CAP-17`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-28) |
| 713 | W10 | 专项 | [`CAP-29`](roadmap/capability.md#step-cap-29) | Capability · Streamable HTTP MCP | `CAP-22`、`CAP-24`、`CAP-28` | ⏳ | [专项卡](roadmap/capability.md#step-cap-29) |
| 714 | W10 | 专项 | [`CAP-30`](roadmap/capability.md#step-cap-30) | Capability · 动态工具搜索与受控扩展准入 | `CAP-01`、`CAP-02`、`CAP-05`、`CAP-22`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-30) |
| 715 | W10 | 专项 | [`CAP-31`](roadmap/capability.md#step-cap-31) | Capability · macOS 原生后端 | `CAP-07`、`CAP-08`、`CAP-10`、`CAP-12`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-31) |
| 716 | W10 | 专项 | [`CAP-32`](roadmap/capability.md#step-cap-32) | Capability · Windows 原生后端 | `CAP-07`、`CAP-08`、`CAP-10`、`CAP-12`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-32) |
| 717 | W10 | 专项 | [`CAP-33`](roadmap/capability.md#step-cap-33) | Capability · Container / gVisor 可选执行环境 | `CAP-07`、`CAP-10`、`CAP-12`、`CAP-26` | ⏳ | [专项卡](roadmap/capability.md#step-cap-33) |
| 718 | W10 | 专项 | [`DEP-27`](#step-dep-27) | 定义 MigrationRegistry、checksum、ordered steps、precondition、owner、backup requirement、compatibility window | `DEP-03`、`DEP-19` | ⏳ | [专项卡](#step-dep-27) |
| 719 | W10 | 专项 | [`DEP-28`](#step-dep-28) | 实现 read-only migration preflight：store/schema/projection/workflow/provider/config/space/clock/lease 矩阵 | `DEP-07`、`DEP-08`、`DEP-27` | ⏳ | [专项卡](#step-dep-28) |
| 720 | W10 | 专项 | [`DEP-29`](#step-dep-29) | 实现 expand/backfill/verify/switch/contract 的 idempotent bounded migration primitives | `DEP-27`、`DEP-28` | ⏳ | [专项卡](#step-dep-29) |
| 721 | W10 | 专项 | [`DEP-30`](#step-dep-30) | 实现 migration runner lock/fence、`MigrationStarted/Step/Blocked/Completed`、resume token 和 failure quarantine | `DEP-06`、`DEP-27`、`DEP-28`、`DEP-29` | ⏳ | [专项卡](#step-dep-30) |
| 722 | W10 | 专项 | [`DEP-31`](#step-dep-31) | 实现 post-migration projector/index rebuild、source/projection cursor/generation/receipt invariant 验证 | `DEP-16`、`DEP-22`、`DEP-30` | ⏳ | [专项卡](#step-dep-31) |
| 723 | W10 | 专项 | [`DEP-32`](#step-dep-32) | 实现 binary/data rollback decision gate、restore fallback、old root retention 和 rollback receipt | `DEP-23`、`DEP-28`、`DEP-30`、`DEP-31` | ⏳ | [专项卡](#step-dep-32) |
| 724 | W10 | 专项 | [`DEP-33`](#step-dep-33) | 在 workflow/runner/provider/skills/plugins 中实现 build/digest pin、replay compatibility 和 old revision drain | `DEP-03`、`DEP-28`、`DEP-31` | ⏳ | [专项卡](#step-dep-33) |
| 725 | W10 | 专项 | [`DEP-34`](#step-dep-34) | 扩展 release preflight：reproducible build、Cargo.lock、target matrix、SBOM/checksum/signature、migration/backup gate | `DEP-02`、`DEP-03`、`DEP-18`、`DEP-27`、`DEP-28` | ⏳ | [专项卡](#step-dep-34) |
| 726 | W10 | 专项 | [`DEP-35`](#step-dep-35) | 实现 managed-local/embedded-local 单机 rollout：plan→preflight→backup→drain→replace→ready→promote | `DEP-10`、`DEP-13`、`DEP-20`、`DEP-28`、`DEP-34` | ⏳ | [专项卡](#step-dep-35) |
| **W11** | **真实环境与发布证据** |  |  |  |  |  |  |
| 727 | W11 | 专项 | [`ER-36`](roadmap/event-receipt-recovery.md#step-er-36) | Event / Receipt / Recovery · Physical/live boundary and handoff | `ER-35` | ⏳ | [专项卡](roadmap/event-receipt-recovery.md#step-er-36) |
| 728 | W11 | 专项 | [`CAP-34`](roadmap/capability.md#step-cap-34) | Capability · 扩展组合验收与证据收口 | `CAP-27`、`CAP-28`、`CAP-29`、`CAP-30`、`CAP-31`、`CAP-32`、`CAP-33` | ⏳ | [专项卡](roadmap/capability.md#step-cap-34) |
| 729 | W11 | 专项 | [`H36`](roadmap/harness.md#step-h36) | Harness · 三入口集成、真实 Provider 和证据收口 | `H35` | ⏳ | [专项卡](roadmap/harness.md#step-h36) |
| 730 | W11 | 专项 | [`P4-J7-31`](roadmap/provider.md#step-p4-j7-31) | Provider · 逐协议、逐连接 live 验收与迁移收口 | `P4-J7-30` | ⏳ | [专项卡](roadmap/provider.md#step-p4-j7-31) |
| 731 | W11 | 专项 | [`CM-38`](roadmap/context-memory.md#step-cm-38) | Context / Memory · End-to-end fake Provider / live opt-in evidence | `CM-33`、`CM-34`、`CM-35`、`CM-36`、`CM-37` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-38) |
| 732 | W11 | 专项 | [`CM-39`](roadmap/context-memory.md#step-cm-39) | Context / Memory · 文档、状态和交接收口 | `CM-38` | ⏳ | [专项卡](roadmap/context-memory.md#step-cm-39) |
| 733 | W11 | 专项 | [`UI-39`](roadmap/ui-entrypoints.md#step-ui-39) | UI / Entrypoints · live ACP/IDE opt-in 验证 | `UI-29`、`UI-30`、`UI-33`、`UI-38` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-39) |
| 734 | W11 | 专项 | [`UI-40`](roadmap/ui-entrypoints.md#step-ui-40) | UI / Entrypoints · 发布门与证据收口 | `UI-32`、`UI-33`、`UI-34`、`UI-35`、`UI-36`、`UI-37`、`UI-38`、`UI-39` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-40) |
| 735 | W11 | 专项 | [`UI-41`](roadmap/ui-entrypoints.md#step-ui-41) | UI / Entrypoints · 交接、审查和后续缺口 | `UI-40` | ⏳ | [专项卡](roadmap/ui-entrypoints.md#step-ui-41) |
| 736 | W11 | 专项 | [`CO-48`](roadmap/companyos.md#step-co-48) | CompanyOS · 真实模型闭环验证、文档回填与交接 | `CO-47` | ⏳ | [专项卡](roadmap/companyos.md#step-co-48) |
| 737 | W11 | 专项 | [`OA-28`](#step-oa-28) | Physical/live handoff；目标 OS、OTLP backend、隔离 provider/connector 和 operator runbook | `ER-36`、`OA-26`、`OA-27` | ✅ | [专项卡](#step-oa-28) |
| 738 | W11 | 专项 | [`DEP-36`](#step-dep-36) | 实现 container adapter：immutable image、volume/root identity、env allowlist、SIGTERM、startup/readiness/liveness probe | `DEP-09`、`DEP-11`、`DEP-17`、`DEP-35` | ⏳ | [专项卡](#step-dep-36) |
| 739 | W11 | 专项 | [`DEP-37`](#step-dep-37) | 设计 orchestrated canary/blue-green/rainbow rollout 与 worker build routing；标记真实编排器为 target | `DEP-33`、`DEP-35`、`DEP-36` | ⏳ | [专项卡](#step-dep-37) |
| 740 | W11 | 专项 | [`DEP-38`](#step-dep-38) | 实现 rollout pause/resume/promote/rollback、old revision retirement、retention 和 post-deploy verification | `DEP-32`、`DEP-35`、`DEP-37` | ⏳ | [专项卡](#step-dep-38) |
| 741 | W11 | 专项 | [`DEP-39`](#step-dep-39) | 接入供应链、签名、SBOM、依赖许可、desktop package/checksum、secret scanning/compliance gate | `DEP-02`、`DEP-34`、`DEP-38` | ⏳ | [专项卡](#step-dep-39) |
| 742 | W11 | 专项 | [`DEP-40`](#step-dep-40) | 编写 release/upgrade/rollback/backup/restore/migration/health 的跨 CLI/Web/Workbench/Desktop E2E/UAT | `DEP-15`、`DEP-24`、`DEP-31`、`DEP-36`、`DEP-38`、`DEP-39` | ⏳ | [专项卡](#step-dep-40) |
| 743 | W11 | 专项 | [`DEP-41`](#step-dep-41) | 维护 runbook、operator reference、CURRENT_STATUS 证据块、release gate 和 capability/proof matrix | `DEP-00`、`DEP-40`、`DEP-01`、`DEP-02`、`DEP-03`、`DEP-04`、`DEP-05`、`DEP-06`、`DEP-07`、`DEP-08`、`DEP-09`、`DEP-10`、`DEP-11`、`DEP-12`、`DEP-13`、`DEP-14`、`DEP-15`、`DEP-16`、`DEP-17`、`DEP-18`、`DEP-19`、`DEP-20`、`DEP-21`、`DEP-22`、`DEP-23`、`DEP-24`、`DEP-25`、`DEP-26`、`DEP-27`、`DEP-28`、`DEP-29`、`DEP-30`、`DEP-31`、`DEP-32`、`DEP-33`、`DEP-34`、`DEP-35`、`DEP-36`、`DEP-37`、`DEP-38`、`DEP-39` | ⏳ | [专项卡](#step-dep-41) |
| 744 | W11 | 专项 | [`PD-33`](roadmap/persistence-data-layer.md#step-pd-33) | 备份→恢复→升级→重启→治理删除的端到端 UAT；CLI/Web/Workbench 共用 DaemonHost | `ER-33`、`ER-34`、`ER-35`、`ER-36`、`PD-22`、`PD-23`、`PD-24`、`PD-25`、`PD-26`、`PD-27`、`PD-28`、`PD-29`、`PD-30`、`PD-31`、`PD-32` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-33) |
| 745 | W11 | 专项 | [`PD-34`](roadmap/persistence-data-layer.md#step-pd-34) | 容量、吞吐、延迟和退化预算；事件帧、Artifact、索引、backup/prune 的压力测试 | `PD-27`、`PD-29`、`PD-33` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-34) |
| 746 | W11 | 专项 | [`PD-35`](roadmap/persistence-data-layer.md#step-pd-35) | 收口文档、CURRENT_STATUS 证据、发布门与迁移 runbook；`docs/`、scripts、CI | `PD-00`、`PD-01`、`PD-02`、`PD-03`、`PD-04`、`PD-05`、`PD-06`、`PD-07`、`PD-08`、`PD-09`、`PD-10`、`PD-11`、`PD-12`、`PD-13`、`PD-14`、`PD-15`、`PD-16`、`PD-17`、`PD-18`、`PD-19`、`PD-20`、`PD-21`、`PD-22`、`PD-23`、`PD-24`、`PD-25`、`PD-26`、`PD-27`、`PD-28`、`PD-29`、`PD-30`、`PD-31`、`PD-32`、`PD-33`、`PD-34` | ⏳ | [专项卡](roadmap/persistence-data-layer.md#step-pd-35) |
| 747 | W11 | 专项 | [`SC-41`](roadmap/security-compliance.md#step-sc-41) | .github/workflows、release scripts、security gate | `SC-28`、`SC-29`、`SC-34`、`SC-37`、`SC-40` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-41) |
| 748 | W11 | 专项 | [`SC-42`](roadmap/security-compliance.md#step-sc-42) | scripts smoke、recovery/retention rehearsal | `SC-23`、`SC-32`、`SC-33`、`SC-41` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-42) |
| 749 | W11 | 专项 | [`SC-43`](roadmap/security-compliance.md#step-sc-43) | CURRENT_STATUS.md、module map、review record | `SC-34`、`SC-35`、`SC-36`、`SC-41`、`SC-42` | ⏳ | [专项卡](roadmap/security-compliance.md#step-sc-43) |
---

## 2. 当前窗口

| 顺序 | 单元 | 已有证据 / 缺口 | 开始或退出条件 |
|---|---|---|---|
| 核对基线 | 源码快照与 WIP | `db77c24` 是本次源码审查基线；工作树另有恢复、取消、记忆、提示词等 WIP，文档已按用户指示直接回填 | 测试前记录相关文件快照与 WIP 清单；逐项核验新增实现，不批量套用历史完成态 |
| 当前 1 | `P0-J1-05a` 回归 | 已修复 `local_with_model_config` 丢失 wall-time 配置的问题，并补上非法配置与预算耗尽的 daemon 级验收；源码提交 `dd6a8d5` 已推送，CI 尚未等待 | 保留历史 wall-time 证据；远端 CI 负责行为测试回执 |
| 当前 2 | `P0-J1-05b` 接线 | `bd9dea0` 已补真实 DaemonHost 角色/环境/Continue 产品链断言，并加强 Start 命令限额；已推送，CI 尚未等待 | 保留每 run 的角色快照、构造上限取 `min`、wall-time Continue 语义；远端 CI 负责行为回执 |
| 当前 3 | `P1-J3-01` 候选写入 | `758ffbe` 已将模型持久写入固定为 `origin=model`、`candidate/draft`，默认检索排除；补齐审批前后产品链和 v1 兼容验收，已推送，CI 尚未等待 | 保留模型不能自批/伪造来源，v1 存量可检索但 provenance 不可验证；远端 CI 负责行为回执 |
| 当前 4 | `CP-00` 固定基线 | `f366436` 源码盘点后新增入口矩阵，明确 DaemonHost/ControlPlane/Runner/Broker/Approval/Hook 边界和 CP-04/05 handoff；已推送，CI 尚未等待 | 保持差异显式，不把三入口共用部分 helper 写成已完成的原子统一处理器 |
| 当前 5 | `ER-00` 固定事实边界 | `0a29de5` 源码快照上新增 Event/Receipt/Recovery 基线矩阵，登记 EventStore capabilities、事实所有权、ID 链、缓存边界与失败分类；已推送，CI 尚未等待 | 不提升 ER-01+ 或 durable/live 证明；运行时 fixture 由 GitHub CI 负责 |
| 当前 6 | `CAP-00` 固定可复核基线 | 本次文档提交新增 [Capability 基线](roadmap/capability-baseline.md)，绑定 `b49cd62` 源码快照、相关 hash、registry/scope/cancel/patch/MCP/memory 六条链、五工具边界、timeout 分层和 CP/H handoff；已推送，CI 尚未等待 | 仅 baseline artifact 为 `implemented/source`；产品 capability 保持 `partial/target/deferred`，运行时 fixture 由 GitHub CI 负责 |
| 当前 | `P4-J7-04` Provider 基线 | 已收口：`roadmap/provider-baseline.md` 固定产品路径与 legacy_fixtures 分界、调研缺口复核（10 项中 7 已修）和测试索引（kiana-provider 零测试为 RED） | 运行时回执由 GitHub CI 负责 |
| 当前 7 | `P0-G-04` 事件重建投影（恢复线重开） | `38f23bc` 已把 Invocation 折叠、惰性缓存、终态冲突拒绝和审批恢复接入 `kiana-core`；新增重启/冲突验收骨架，远程 CI 尚未回执 | P0-G-04 保持 🔄：历史 Run 证据之外，Invocation 运行时验收和集成回归由 GitHub CI 负责 |
| 当前 8 | `CI-01` 配置/凭据/身份基线 | 已加入 `config-credentials-identity-baseline.md`、五个 deterministic fixture、provider precedence/redaction 集成护栏和 GitHub Actions 专用 job；不提前实现 CI-06/07/08 | `feature_status=partial`、`proof_level=source`；本地不运行测试，远端 CI 已触发且不等待；raw-secret sentinel、parser 差异和 local-user 迁移边界保持显式 |
| 当前 9 | `SW-00` Swarm 现状 reconciliation | 已新增 [`swarm-baseline.md`](roadmap/swarm-baseline.md)、10 个源码 hash、已接线/仅类型/缺测试/未实现矩阵，以及 `swarm_reconciliation_does_not_claim_durable_from_in_memory_cas` 的 GitHub Actions 专用护栏；不把 MemoryCellRegistry 或 event replay 写成 durable | `feature_status=partial`、`proof_level=source`；本地不运行测试，远端 CI 已触发且不等待；下一步按队列进入 `OA-00` |
| 当前 10 | `OA-00` Observability / Audit 现状 inventory | 已新增 [`observability-audit-baseline.md`](roadmap/observability-audit-baseline.md)、19 个源码 hash、signal matrix、owner/proof ceiling/迁移清单，以及 `observability_inventory_preserves_fact_projection_and_open_contracts` 的 GitHub Actions 专用护栏；明确 EventLog/Receipt/RunStream/GoldenTrace/usage/Incident 的事实与派生边界 | `feature_status=partial`、`proof_level=source`；本地不运行测试，远端 CI 已触发且不等待；不把 golden trace、receipt、UI cursor、metric 或 health 设为事实/Outcome；下一步进入 `OA-01` |
| 当前 11 | `OA-01` Domain schema 注册 | 已在 `kiana-domain` 注册四类 versioned signal contract，加入 owner/compatibility registry、`deny_unknown_fields`、minor-compatible headers、source cursor/attribute bounds、canonical digest 和未注册 metric 拒绝；新增 `oa01_contracts` GitHub Actions 专用测试 | `feature_status=implemented`（domain/schema source）、`proof_level=source`；没有新增 sink、projector、授权判断或外部 exporter；下一步进入 `OA-02` correlation/trace references |
| 当前 12 | `OA-02` CorrelationContext / TraceRef / SpanRef | 已在 domain 注册 `kiana.correlation-context.v1`；严格解析 W3C traceparent，服务端生成 fresh root，绑定 authenticated request/scope/authority/data epoch，提供 causation/attempt links、run→turn→invocation 顺序、local child 与 async `FollowsFrom` child；加入 `CorrelationContextPort` 和远端专用测试 | `feature_status=implemented`（domain/port source）、`proof_level=source`；trace 仍只做关联，不作为 actor、authority、policy、approval 或 effect 证据；下一步进入 `OA-03` redaction/classification |
| 当前 13 | `OA-03` RedactionProfile / bounded signal encoder | 已在 domain 注册 `kiana.redaction-profile.v1`；按 signal/data class 绑定 profile digest、字节/深度上限，复用递归 redactor 后统一检查 NUL、编码、残余 secret marker 和大小，错误绝不回退原文；新增五类信号的远端 sentinel/边界护栏 | `feature_status=implemented`（profile/encoder source）、`proof_level=source`；runtime sink、EventStore observer 与四入口接线仍未完成；下一步进入 `OA-04` Audit taxonomy/reducer |
| 当前 14 | `OA-04` Audit taxonomy / reducer | `kiana-domain/src/audit.rs` 固定八类 action taxonomy，从带 source binding/epoch 的 committed `RuntimeEvent` 派生服务端 actor、cursor、source IDs、redacted action/reason digest；`kiana-core` 提供同一只读 facade；未知 `audit.*`、伪造 actor/record、缺 source/epoch、重复/矛盾 decision fail-closed；新增 domain/core 远端 reducer 夹具 | `feature_status=implemented`（domain/core reducer source）、`proof_level=source`；尚无 EventLog commit observer、AuditProjection checkpoint、query/export sink；下一步进入 `OA-05` ports/fake adapters |
| 当前 15 | `OA-05` Observability/Audit ports and fake adapters | `kiana-ports` 新增封闭 signal union、`ObservabilityPort`/`TraceSink`/`MetricSink`/`AuditQueryPort`/`HealthProbePort`，能力协商、flush/cancel/capacity fail-closed；Memory/JSONL fake 支持验证后记录、注入失败、query page/probe，新增 versioned `HealthSnapshot` | `feature_status=implemented`（domain/ports/fakes source）、`proof_level=source`；fake 不等于 durable sink，尚无 EventLog commit observer/backpressure/入口接线；下一步进入 `OA-06` commit observer |
| 当前 16 | `OA-06` EventStore commit observer | `kiana-ports` 新增严格 `CommittedTransition`/observer contract；`kiana-eventlog::StreamEventStore` 仅在 fresh `Committed` 后顺序通知，replay/conflict/unknown 不通知；observer failure 有界记录且不改写已提交 outcome，`read_from` 保留重启补偿 | `feature_status=implemented`（observer source）、`proof_level=source`；callback 是可丢 wake hint，尚无 durable projection checkpoint/backpressure/入口接线；下一步进入 `OA-07` run/turn/invocation span 生命周期 |
| 当前 17 | `OA-07` Run/Turn/Invocation span 生命周期 | `kiana-core::span_projection` 从已关联的 committed `RuntimeEvent` 派生稳定 trace/span ID、start/pause/resume/checkpoint/end 生命周期，绑定 source cursor/event；终态冲突、迟到事件、重复 terminal 和旧 attempt 不覆盖事实；ControlPlane 提供只读重建入口，远端专用夹具已加入 | `feature_status=implemented`（projection source）、`proof_level=source`；span 是可重建派生记录，不是 EventLog 事实或 exporter durable/live 证明；下一步进入 `OA-08` provider/model/usage instrumentation |
| 当前 18 | `OA-08` Provider/model/stream/usage instrumentation | `kiana.model-attempt.v1` 只从 committed `run.model_turn` 派生 provider/model/route/prompt hash、stream/latency/stop/usage/retry/cache 记录；provider 与 daemon 共享 allow-listed prepared summary，流截断、超时、重试、缺 usage 和 malformed metadata fail-closed；远端 provider/core 专用夹具已加入 | `feature_status=implemented`（projection/provider boundary source）、`proof_level=source`；model attempt 是可重建 projection，不是 provider receipt、账单证明或 live telemetry sink；prompt/header/raw response 不可表示；下一步进入 `OA-09` broker/approval/effect/stop instrumentation |
| 当前 19 | `OA-09` Broker/approval/effect/stop instrumentation | 新增 `kiana.capability-attempt.v1` 与 committed capability-attempt reducer，固定 admission/approval/permit/dispatch/execution/result/stop/fence/zero-effect 字段；ControlPlane 在 handler 边界前提交 `invocation.executing`，拒绝、过期审批、TOCTOU/lease mismatch 和取消未确认均保持 zero-effect 或 `unknown`；shell/patch 输出补充有界 effect/stop 元数据，新增远端拒绝/成功/取消夹具 | `feature_status=implemented`（projection/core/daemon source）、`proof_level=source`；attempt 记录是 EventLog 派生证据，不是外部效果 receipt、durable sink 或 live stop 证明；unknown 仍需后续 reconcile/recovery；下一步进入 `OA-10` metrics |
| 当前 20 | `OA-10` EventLog/projector/Receipt/Artifact/Recovery metrics | 新增 `kiana.metric-snapshot.v1` 与 EventLog 派生 operational metrics reducer，固定 durable/projector cursor、lag、commit/append/flush/rebuild/query latency、orphan/unknown、artifact bytes 与 last-error presence；空源、cursor gap、孤儿 dispatch、未知效果和 artifact 失败保留 degraded/limitation，不生成健康零工作；ControlPlane 提供只读 metrics bridge，新增远端 core 夹具 | `feature_status=implemented`（domain/core source）、`proof_level=source`；快照是 committed EventLog 的可重放投影，不是 durable projector checkpoint、runtime gauge、receipt 事实、external effect 或 live exporter；下一步进入 `OA-11` health/readiness |
| 当前 21 | `OA-11` Health snapshot/readiness/liveness/component capability | 扩展 `HealthSnapshot` 为有界 probe kind、component health/version/last-success/limitation；core 聚合 EventLog metrics、EventStore capability、cursor/lag/unknown/orphan，并提供 startup/readiness/liveness 只读入口；DaemonHost 复用同一投影补 daemon component；远端空源、ready 拒绝、liveness、capability/serde 夹具 | `feature_status=implemented`（domain/core/daemon source）、`proof_level=source`；health 仅是 committed source projection，不是 durable heartbeat、provider/Broker/exporter live probe 或 admission gate；readiness 对 inferred projector cursor/Unknown/不完整 EventStore 保持 degraded；下一步进入 `OA-12` metric catalog/reducer/cardinality guard |
| 当前 22 | `OA-12` Metric catalog/reducer/cardinality guard | 扩展 MetricCatalog/MetricPoint 的类型、单位、quality（measured/estimated）、cursor 与 digest 边界；新增 `MetricCardinalityGuard`（allowlist、敏感 label/value 拒绝、有界 series/value、overflow）、`MetricReducer`（增量/replay 共用校验、counter reset/cursor regression 拒绝、catalog digest 绑定）；新增远端目录、基数、quality、counter/replay 夹具 | `feature_status=implemented`（domain/core source）、`proof_level=source`；未接入 durable metric sink/queue、runtime gauge、跨进程 checkpoint 或 live exporter；估算值仍不能进入 measured/cost 账本；下一步进入 `OA-13` queue/backpressure |
| 当前 23 | `OA-13` bounded observability queue/backpressure | `kiana-ports` 新增 non-blocking bounded queue 与 Event/Audit/Approval/Recovery/terminal criticality；满载只淘汰 best-effort log/trace/metric，critical 满载立即结构化拒绝；提供 depth/drop reason/counter、flush/shutdown/reopen/cancellable ack，DaemonHost 持有并复用该队列，drop/reject 使 telemetry/health 降级；新增远端队列夹具 | `feature_status=implemented`（ports/daemon source）、`proof_level=source`；队列不是 EventLog/durable spool，critical reject 需由事实扫描补偿，尚无 async consumer/exporter、持久 spool、跨进程 shutdown 或 live telemetry；下一步进入 `OA-14` trace exporter/context adapter |
| 当前 24 | `OA-14` Trace exporter/W3C context adapter | 新增 `kiana.trace-export-span.v1` 有界导出合同与 core `LocalTraceExporter`/`NoopTraceExporter`；foreign `traceparent` 只解析为 `ForeignParent` link，服务端 trace/span/actor/scope 不被外部上下文覆盖；采样关闭返回 SampledOut，capacity/invalid parent/summary/exporter closed fail-closed，JSONL/flush/shutdown/reopen 仅处理已验证投影；新增远端 trace 夹具 | `feature_status=implemented`（domain/core source）、`proof_level=source`；无 OTLP/外部 backend、durable exporter、异步 consumer 或 sampling policy persistence；exporter 失败不改 Receipt/授权；下一步进入 `OA-15` AuditProjection checkpoint/rebuild |
| 当前 25 | `OA-15` AuditProjection checkpoint/rebuild | 新增 `kiana.audit-projection.v1`/`kiana.audit-projection-checkpoint.v1`、`AuditProjectionSnapshot`/`Checkpoint`；core `rebuild_audit_projection` 与 `AuditProjection` 支持有界 rebuild/append/restore，严格校验 source cursor/event、schema/decision、record IDs/digest/checkpoint binding，gap/duplicate/conflict fail-closed 且不覆盖原事实；新增远端审计投影夹具 | `feature_status=implemented`（domain/core source）、`proof_level=source`；checkpoint 尚无 durable store/跨进程自动加载、Artifact ref/query/export 接线或 audit correction/incident workflow；下一步进入 `OA-16` audit query wire DTO |
| 当前 26 | `OA-16` Audit query command/wire DTO | protocol 新增 bounded `AuditQueryRequest`/`AuditQueryResponse` 与 `RequestBody::AuditQuery`，client 提供只读 query facade；DaemonHost 先拒绝缺失/伪造 actor、invalid limit/cursor，再把请求交给 ControlPlane；core 从 OA-15 全量 projection 按服务端 actor/session/project 与 approval/request lineage 过滤，输出 redacted AuditRecord page，不暴露 raw EventLog/owner scope 覆盖 | `feature_status=implemented`（core/protocol/client/daemon source）、`proof_level=source`；远端仅 wire/static fixture，尚无 durable AuditProjection query index、snapshot/filter cursor、跨入口实际调用、export/delivery 或 live auth provider；下一步进入 `OA-17` query cursor/snapshot/paging |
| 当前 27 | `OA-17` query cursor/snapshot/paging | 新增 `kiana.audit-query-cursor.v1` 与 `AuditQueryCursor`，绑定 epoch/projection version/source+after cursor/filter digest/cursor digest；AuditQueryRequest/Response 支持 cursor，core 校验 cursor 与当前 OA-15 projection/过滤、拒绝 stale/ahead，next page 返回新绑定 cursor；新增远端 cursor serde/boundary 夹具 | `feature_status=implemented`（domain/core/protocol/daemon source）、`proof_level=source`；cursor/epoch/filter snapshot 尚无 durable query index、retention revoke、多入口 reconnect 或慢查询 instrumentation；下一步进入 `OA-18` audit export/manifest/delivery |
| 当前 28 | `OA-18` audit export/manifest/delivery | 新增 `kiana.audit-export.v1`/`kiana.audit-delivery-receipt.v1`、`AuditExportManifest`/`AuditDeliveryReceipt` 与 protocol `AuditExportRequest/Response`；core 复用 server-scoped query，输出 JSONL/JSON/CSV redacted content + query/source/projection/artifact hashes，Safe/invalid scope/secret/oversize fail-closed；delivery 请求无独立 confirmation 只返回 Unknown，不声称 delivered；新增远端 domain/wire 夹具 | `feature_status=implemented`（domain/core/protocol/client/daemon source）、`proof_level=source`；没有 durable ArtifactStore/export file、external delivery connector/confirmation、query index/retention/export audit fact；下一步进入 `OA-19` incident/recovery association |
| 当前 29 | `OA-19` observability alert/incident/recovery association | 新增 `kiana.observability-alert.v1`/`incident.v1`/`incident-snapshot.v1` 与有界 Alert/Incident 合同；core 仅由 OA-10 metrics/committed failure facts 生成规则 fingerprint 去重的 projector lag、audit/artifact loss、redaction、queue overflow、journal corruption、effect unknown、orphan dispatch incidents，所有 recovery plan 禁止自动 retry/approve/close，unknown 保持 open/reconciliation required；新增远端去重/自报拒绝夹具 | `feature_status=implemented`（domain/core source）、`proof_level=source`；没有 durable incident checkpoint、EventLog incident fact、operator workflow、queue/exporter live state 或 cross-process recovery；Company Incident/FailureIncident 仍是不同事实域；下一步进入 `OA-20` data governance/retention propagation |
| 当前 30 | `OA-20` data governance/retention/deletion propagation | 增强 `DataPolicy` 为 schema/version/revision/data_epoch/digest，校验 Purpose/Retention/ProcessingGrant；新增 `DataGovernanceSnapshot`/`DataRetentionObservation` 明确 payload 与 audit metadata，core 从 committed revocation/governance facts 派生 Receipt/Audit/Artifact/Memory/Index/Cache/Export 状态，pending revocation 为 Unknown、revoke/expire 传播为 Revoked/Expired；daemon policy 读取 integrity 校验并输出 data_epoch/propagation map | `feature_status=implemented`（domain/core/daemon source）、`proof_level=source`；policy/snapshot 与 derived stores 尚无跨进程 durable checkpoint，EventLog/Artifact/Telemetry 实际 purge/export wiring、retention scheduler、external legal hold 和 live deletion evidence 仍开放；下一步进入 `OA-21` replay/reconciliation diagnostics |
| 当前 31 | `OA-21` replay/reconciliation diagnostics | 新增 `kiana.replay-diagnostic.v1`/snapshot 与只读 `diagnose_replay`/`ControlPlane::replay_diagnostics`，复用 Invocation/Run/Metric/Audit/Health/Span deterministic projections，输出 bounded divergence（invocation/attempt/input digest/expected-observed status/error/source refs）和 projection digests；source duplicate、gap、schema/terminal/projection mismatch、unknown effect 保留 Unknown，不调用模型/Provider/Broker/Recovery action；新增远端 replay/expectation/self-report 拒绝夹具 | `feature_status=implemented`（domain/core source）、`proof_level=source`；无自动 reconcile/compensate/retry、durable diagnostic checkpoint、provider receipt 或 cross-process replay; Receipt/Health/Incident 全量 consistency 仍需后续故障/容量门；下一步进入 `OA-22` crash/fault injection |
| 当前 32 | `OA-22` crash/fault injection matrix | 新增 `kiana.fault-case.v1`/`fault-matrix.v1` 与 core deterministic fault simulator，覆盖 prepare/commit/dispatch/result/flush/projector/export/shutdown 八个边界；每个 case 绑定 seed/source cursor/events、rejected/unknown/observed、effect known/started、resource fenced、duplicate/false-success guards；unknown 强制 fenced、rejected 强制无 effect、矩阵可重放，source duplicate/invalid seed fail-closed；新增远端矩阵夹具 | `feature_status=implemented`（domain/core source）、`proof_level=source`；仅 replay-only safety model，无真实 crash/process kill、EventStore/Broker/Provider/projector/export/shutdown fault hook、durable incident/receipt/reconcile 或 cross-process resource evidence；下一步进入 `OA-23` provider-independent eval |
| 当前 33 | `OA-23` provider-independent eval suite | 新增 `kiana.eval-case.v1`/`eval-result.v1`/`eval-suite.v1` 与 core read-only eval runner，复用 committed normalized events、Audit/Metric/Span/Run receipt/Replay projections；要求的证据缺失、secret、forbidden effect、status/replay divergence、measured cost/latency mismatch 生成 Fail/Blocked，`promote` 仅全 Pass；cost kind 与低基数 latency bucket 可重现，不调用真实 Model/Provider/Broker | `feature_status=implemented`（domain/core source）、`proof_level=source`；fake model/provider/broker 仅由远端夹具/事实构造，无真实 provider eval、Promptfoo 外部 runner、durable eval artifact、自动 Promote/rollback 或 external cost receipt；下一步进入 `OA-24` 四入口 parity |
| 当前 34 | `OA-24` 四入口审计/健康/Receipt parity | 新增 `kiana.entrypoint-parity.v1` 与统一 ControlPlane owner-scoped parity projection；CLI `kiana parity`、Workbench `/parity`、Web `/api/parity`（Desktop 复用 Web）只通过协议/DaemonHost 读取同一 source cursor、status、Receipt/Audit/Health digests 与 limitations；健康端点不再硬编码成功，owner mismatch、source gap、terminal conflict、retention revoke 和未知字段 fail-closed；新增远端四入口 parity/wire fixtures | `feature_status=implemented`（domain/core/protocol/client/daemon/entrypoints source）、`proof_level=source`；本地不运行测试，静态编译通过，GitHub Actions 已触发且未等待；parity/health 仍是 EventLog 的进程内只读投影，不是 durable query index、外部认证、真实健康探针、Receipt/business Outcome 或跨进程 retention/reconcile 证明；下一步进入 `OA-25` 容量/性能/迁移 |
| 当前 35 | `OA-25` 容量、性能和迁移演练 | 新增 bounded `PerformanceBaseline`/`BenchmarkSummary`/`CapacityEnvelope`/`MigrationObservation` 合同与 percentile reducer；远端夹具覆盖 append/flush/project/rebuild/query/export 六类摘要、journal hard limits、high-cardinality/oversize/backpressure guard、rotation/archive/upgrade/downgrade 与未知 writer version；不在本地运行测试 | `feature_status=implemented`（domain/core source）、`proof_level=source`；本地只做格式与 test-target 静态编译，GitHub Actions 已触发且未等待；时间样本仍是远端 fixture 观测，不是生产 p50/p95/p99 承诺，未完成 durable benchmark artifact、真实大 artifact/慢 exporter、跨平台 rotation/archive 或 live capacity proof；下一步进入 `OA-26` local durable gate |
| 当前 36 | `OA-26` Local durable observability gate | 新增 CI-only `scripts/oa26-durable-observability-gate.sh` 与 workflow；GitHub runner 串行验证 JSONL append→close→reopen→read_all、OA-24 parity/Unknown、critical queue preservation、source/release/artifact SHA-256 与 secret sentinel scan，非 CI 调用主动返回 `remote_ci_required`；不把 memory sink、历史 CI 或 mock 视为 durable | `feature_status=implemented`（CI gate/source）、`proof_level=source`（待远端回执，单机 reopen 上限为 `local_behavior`）；本地只做 shell syntax/格式与 test-target 静态编译，GitHub Actions 已触发且未等待；无 physical power-loss/kill-9、跨进程/网络文件系统、真实 provider/Broker/telemetry backend、durable benchmark artifact、retention/reconcile 或 live/physical 证明；下一步进入 `OA-27` cross-entry/company governance gate |
| 当前 37 | `OA-27` Cross-entry/company governance gate | 新增 `kiana.company-governance.v1` 与只读 CompanyOS runtime→review→acceptance→delivery→close projection；Runtime Completed 不自动成为 Outcome，Closed 必须有独立 Review、Accepted/waived Acceptance、Confirmed Delivery、独立 Closer、全部 runtime Completed 与 ClosingReceipt；CLI `company-governance`、Workbench `/governance`、Web `/api/company-governance`（Desktop 复用 Web）经过同一 protocol/DaemonHost；跨 actor/project/source、链缺失、review/closer 重叠和未知状态 fail-closed；不运行本地测试 | `feature_status=implemented`（domain/core/protocol/client/daemon/entrypoints source）、`proof_level=source`；本地只做格式与 test-target 静态编译，GitHub Actions 已触发且未等待；CompanyState/Delivery/ClosingReceipt 仍是本地 EventLog 投影，尚无 durable business query index、跨组织 auth、外部 delivery confirmation、Outcome measurement 或 live/physical 证明；下一步进入 `OA-28` physical/live handoff |
| 当前 38 | `OA-28` Physical/live handoff | 新增 `kiana.live-handoff.v1`/`LiveHandoffManifest`，按 provider/connector/OTLP backend/operating system 固定目标、环境、非秘密 credential ref、config/source digest、operator approval、provider receipt、retention、incident 与 cleanup；`scripts/oa28-live-handoff-preflight.sh` 未设置 opt-in 时 fail-closed，远端 workflow 只验证 manifest/secret/unknown 安全合同，不连接外部服务、不把 mock/CI 写成 live/physical | `feature_status=implemented`（domain/runbook/preflight source）、`proof_level=source`；本地只做 shell syntax、格式与 test-target 静态编译，GitHub Actions 已触发且未等待；真实 live/physical 仍需逐目标独立环境、凭据、人工批准、receipt/reconcile、retention/cleanup/incident 证据，当前 provider/connector/OTLP/OS 保持 not_supported 或 source；下一步进入后续总路线的 durable/live 交付门 |
| 当前 39 | `AUT-01` automation baseline and migration guard | 新增 [automation-baseline.md](roadmap/automation-baseline.md)、`automation_baseline` GitHub Actions source-only guard 与 fixture catalog；确认 `kiana-workflow::plan_command` 纯 planner、`kiana-core::automation` 先提交 workflow facts 再路由 Company/Broker、DaemonHost 无第二 scheduler，旧 `sdk::watch_scheduled_tasks` 仅兼容目录/DTO API；本地不运行测试 | `feature_status=implemented`（baseline/source）、`proof_level=source`；本地只做格式、shell syntax 与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；ClockPort、durable trigger/queue/claim/lease/fence、scheduler worker、restart/retry/cancel/compensation 和 automation UAT 仍是 AUT-02..AUT-24；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 40 | `NM-00` notifications/messaging baseline | 新增 [notifications-baseline.md](roadmap/notifications-baseline.md) 与 GitHub Actions source-only guard；盘点 EventLog→HumanInbox/RunStream/SSE/transcript 的 event→recipient→channel 矩阵，确认 `HumanInboxItem`/RunStream 均为派生展示，未实现 durable NotificationStore/read state/subscription/outbox/DeliveryWorker/外部通道；旧 watcher 仍兼容隔离；本地不运行测试 | `feature_status=implemented`（baseline/source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；Notification/Message/Subscription/Delivery/recipient resolver、cursor checkpoint、ACK/reconcile 与四入口通知 UAT 仍是 NM-01..NM-22；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 41 | `EQ-00` evaluation/quality baseline | 新增 [evaluation-baseline.md](roadmap/evaluation-baseline.md) 与 GitHub Actions source-only guard；盘点旧 `kiana-commands::EvalCommand`（caller fixture→`kiana.eval-report.v1`）与 OA-23 core committed-fact reducer 的边界，确认两者都不是 QualityGate/EvalStore/Promote authority，冻结隔离/normalizer/finding/gate/CI fixture catalog 与迁移规则；本地不运行测试 | `feature_status=implemented`（baseline/source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；Quality DTO、EvalStore、TraceNormalizer、Judge、isolated runner、promote/rollback、feedback/drift 与真实 model-quality 仍是 EQ-01..EQ-51；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 42 | `PD-00` persistence/data-layer baseline | 新增 [persistence-data-layer-baseline.md](roadmap/persistence-data-layer-baseline.md) 与 GitHub Actions source-only guard；盘点 EventStore/JSONL/Memory/Approval/Artifact/Memory/Query index/Receipt/Projection 的 owner、capabilities、limits、cursor、scope 与失败边界，冻结 StorageRoot/StoreIdentity/Projection/Backup/Migration/Retention 目标和 fixture catalog；本地不运行测试 | `feature_status=implemented`（baseline/source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；统一 StorageRoot、Projection/Artifact/Backup/Migration/Retention ports、跨进程 durable/conformance/recovery 仍是 PD-01..PD-35；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 43 | `INT-00` integrations/connectors baseline | 新增 [integrations-baseline.md](roadmap/integrations-baseline.md) 与 GitHub Actions source-only guard；固定 Provider/Connector/MCP/A2A/Notification 术语和边界，盘点 local_fixture/stdio server-owned binding/scope/idempotency/ProviderReceipt/reconcile，确认 HTTP/remote MCP、OAuth/A2A/webhook、真实外部账户、通知通道和 physical effect 未开放；冻结 INT-01..33 fixture catalog 与迁移护栏；本地不运行测试 | `feature_status=implemented`（baseline/source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；真实 connector/provider/credential/receipt/reconcile/retention、Webhook/A2A ingress、四入口 UAT 和 live/physical 仍是 INT-01..INT-33；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 44 | `CP-01` 服务端主体与项目身份 | 新增 `AuthenticatedPrincipalRef`、`ProjectIdentity`、`SessionAssignment` typed contracts/schema registry；DaemonHost 由服务端固定 local principal、ProjectTrustAuthority 与 canonical root/device/inode/trust digest 派生 project identity，effectful request 先解析 identity，再同步 authority；session assignment CAS 写入 typed identity/role/department，重建时校验 digest，wire actor/role/trust 不可扩权；远端 domain/core fixtures 覆盖 deterministic identity、assignment tamper/unknown fields 与 server metadata guard；不运行本地测试 | `feature_status=implemented`（domain/core/daemon source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；local-user/role allowlist 仍是本地兼容身份，OS credential/OAuth/enterprise tenant、durable principal provider、Grant epoch 与完整 cross-process auth 尚未实现；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 45 | `CP-02` Run/Turn/Invocation/Execution 合同 | 新增 `TurnIdentity`/`InvocationIdentity` typed contracts 与 schema registry；显式 `run.turn.v2` 创建新 Turn/Run 并记录 predecessor，终态旧 Run 不复活，旧 `Continue` 保留 `LegacyContinue` 兼容语义，`Resume` 仍是同一 Run 的显式恢复边界；请求事实记录 server-derived invocation/turn/attempt，Broker 在 prepared/dispatching/executing/result committed 阶段绑定真实 ExecutionId；重复 call_id 不能合并账本，direct command 不虚构 Harness Run；远端 domain/core fixtures 与 CI source guard；不运行本地测试 | `feature_status=implemented`（domain/core/dispatch/protocol/client source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；历史无 TurnId 的 upcast、完整 durable InvocationLedger/RunSnapshot、跨进程恢复、自动重试/对账和真实外部效果仍需 CP-03+、ER/PD/INT/DEP 步骤；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 46 | `SC-00` 安全与合规现状基线 | 新增 [security-compliance-baseline.md](roadmap/security-compliance-baseline.md) 与 `security_baseline` GitHub Actions source guard；固定 Principal/Session/Role/ProjectTrust、Run/Turn/Invocation/Execution、Policy/Gate/Approval/Budget/Lease、Capability/Sandbox/PathLock、Secret/Provider、EventLog/Receipt/Audit/Projection、Memory/Index/Artifact/Notification/UI、外部/物理 effect 与供应链资产边界；逐入口列出 server-owned 控制和缺口，SEC-01..12 保持 partial/target/not_supported 双维度，SC-01..43 交接与证据等级/限制护栏已登记；不运行本地测试 | `feature_status=implemented`（baseline/inventory source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；安全能力本身仍是 partial/target/not_supported，认证、SecretStore、完整 TOCTOU/egress、durable recovery/retention/delete、SBOM/signing、外部/physical effect 和安全 UAT 仍需 SC-01..43 及 CP/CAP/ER/PD/DEP/INT；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 47 | `P0-G-04` 事件重建投影与重启恢复收口 | 现有 `project_run_state`/`project_invocations`/lazy cache/recovery 已覆盖新进程 Run/Invocation 重建、pending approval 授权重检、result_unknown 和矛盾终态拒绝；新增独立 `p0_g04_projection_guard` 与 GitHub Actions 聚焦 workflow，显式绑定 EventLog 事实源和 Receipt/projection 只读边界；不运行本地测试 | `feature_status=implemented`（core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；跨进程真实持久/断电恢复、完整 snapshot/ledger、外部 receipt/reconcile、终态/取消全量故障注入仍需 ER/CP/PD/SC 后续步骤；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 48 | `CP-03` 规范化 action、风险与执行元数据 | 完成闭合 `ACTION_OPERATIONS` descriptor catalog/schema/resource/effect/cancel/reconcile/idempotency 元数据校验；`prepare_capability_action` 复用 bounded JSON/schema、server-stamped identity、canonical operation/path/sandbox/defaults、hook 后重归一化，生成 immutable `PreparedAction` 的 catalog/action digest；ReadOnly 伪造写/外部风险、未知 operation、duplicate JSON key、非法 numeric/path/alias、catalog drift 在 Broker 前拒绝；Runner 五工具、direct Company/context、policy/approval/handler 共享同一 action digest；新增 domain fixtures、core guard 与 CI workflow；不运行本地测试 | `feature_status=implemented`（domain/core/runner/daemon source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；参数 schema 仍是 bounded subset，PreparedAction 不是 permit，完整 ExecutionContext/Grant 交集、durable ToolSnapshot、所有 handler 的 TOCTOU/egress、外部效果与 live/physical 仍由 CP/CAP/ER/PD/SC 后续步骤负责；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 49 | `CP-04` 权限交集与单调决策 | 新增带 digest/version 的 `ScopeSet`、`ScopeDimension::{NotApplicable,Restricted}`、budget/depth limits、path-aware intersection 与 subset 校验；ControlPlane prepare 阶段对 canonical operation/path/MCP server/Memory collection 与 server path scope 求交，空交集 fail-closed；policy hard deny、Gate/Hooks Deny/Ask 单调合并，Allow 不覆盖拒绝或其他审批要求，Cell parent grant 继续做 subset 检查；新增 domain/core 远端 fixtures、CI source guard 与基线文档；不运行本地测试 | `feature_status=implemented`（domain/core/policy/gates source）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；ScopeSet 尚未成为完整持久 ExecutionContext，authority epoch/Grant/Approval/Cell 全维度交集、跨进程 revoke、真实外部/physical effects 和全量 property/UAT 仍由 CP-08+、CAP/ER/PD/SC 负责；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 50 | `CP-05` 三条能力路径共享处理器 | `authorize_and_execute`（direct）、`broker_harness_capability`（Harness）和 `resume_approved_invocation`（approval continuation）统一复用 prepare/authorize/stage/dispatch/finalize；审批 continuation 使用当前 decision context 重做 action/policy/gate/approval 校验，所有结果/取消/Unknown 通过同一 finalize/permit/Broker/EventLog 边界，direct command 保持显式 scope；新增远端跨路径 fixtures、source guard 和 CI workflow；不运行本地测试 | `feature_status=implemented`（core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；三条路径的 adapter/事件序列仍有兼容差异，持久 atomic dispatch、完整 lease/epoch、跨进程 approval recovery、真实 provider/connector/physical effect 与全量 parity/UAT 仍需 CP-06+、CAP/ER/PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 51 | `CP-06` 原子状态转移与 command 幂等合同 | `TransitionBatch` 校验 command digest、唯一 aggregate read-set、bounded frame/events 与 contiguous stream versions；EventStorePort/Memory/JSONL 统一 CAS、Committed/Replayed/Conflict/Unknown，same command 不同 payload 冲突、stale read-set 不写部分 aggregate，observer 仅收到 fresh commit；新增 eventlog 事务 fixtures、core source guard 与 CI workflow；不运行本地测试 | `feature_status=implemented`（domain/ports/eventlog/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；掉电/损坏/跨主机 durable exactly-once、外部 effect receipt/reconcile、CP-07 磁盘事务帧强化、审批/预算/lease 同批原子转移仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 52 | `ER-01` 事件 schema、kind registry 与迁移规则 | 新增 machine-readable `EventKindSpec`/`EVENT_KIND_SPECS`、统一 `kiana.runtime-event.v1` registry、required IDs/allowed fields/terminal/secret policy/aggregate owner/migration metadata；unknown opaque event 保留查询但 request/run/capability/approval/invocation/execution/action/session family 的未知 kind fail-closed，schema major downgrade、缺 required ID、payload unknown field 拒绝；RuntimeEvent/旧 JSONL serde 形状保持兼容，新增 domain fixtures/core source guard/CI workflow；不运行本地测试 | `feature_status=implemented`（domain/protocol registry source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；registry 尚未强制嵌入所有 legacy event、177+ kind 全量 schema/upcaster、durable projector/retention/redaction/receipt migration 仍需 ER-02+、PD/CP/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 53 | `ER-02` 统一身份、关联和顺序语义 | `RuntimeEvent` 新增可选 command/correlation/causation/parent links，新事件默认 correlation=request_id；现有 `CorrelationContext`/AttemptRef/CausationRef 继续校验 scope/epoch/attempt，EventStore 全局拒绝 event_id 重用和 command digest 漂移，Run/Invocation/Span projection 按稳定 run/invocation/execution IDs 与 aggregate stream 而非 request sequence 配对；legacy 缺 links/stream metadata 只读兼容且不授予 authority；新增 domain/eventlog/core remote fixtures、CI workflow 与 identity baseline；不运行本地测试 | `feature_status=implemented`（domain/core/eventlog source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；全量 legacy upcast、持久 InvocationLedger/attempt retry/reconcile、durable cross-process ordering、外部 effect receipt 和 retention/delete 仍是 ER-03+、CP/PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 54 | `ER-03` 事件边界脱敏与 Artifact 引用 | EventStore core append/terminal 入口统一递归 redaction、二次稳定性检查、NUL/深度/JSON 字节上限和非法 data epoch/artifact refs 拒绝；新事件记录 redaction profile digest、payload_recoverable=false、data_epoch、protected artifact_refs，legacy envelope 继续可读；domain bounded RedactionProfile/StreamingRedactor 及 Receipt/Artifact redaction 边界保持一致；新增 secret/oversize/deep/non-resumable/artifact fixture、core source guard、CI workflow 与 baseline；不运行本地测试 | `feature_status=implemented`（domain/core/event/receipt/artifact source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；key/marker redaction 非任意编码 secret 检测，直接 RuntimeEvent/legacy writer 尚未全量强制 profile，SecretStore/进程内存/OS argv-env、durable retention/delete、外部/live/physical effect 仍需 ER-04+、CAP/PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 55 | `ER-04` CommandReceipt 与 transition read-set | `CommandReceipt::validate_against` 绑定 batch command/digest、连续 cursor、event IDs 与 read-set versions；EventStore/ControlPlane 继续以 `TransitionBatch` CAS 和 `read_command` 作为原子提交/Unknown 唯一确认，缺 dependency、stale CAS、same command payload drift 不产生部分事实，Unknown 不发布 dispatch/observer 权限；新增 eventlog fixtures、core source guard、CI workflow 与 receipt baseline；不运行本地测试 | `feature_status=implemented`（domain/ports/eventlog/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；JSONL 掉电/跨主机 durable、legacy append 收口、外部 effect receipt/reconcile、CP-07 磁盘事务强化和 approval/budget/lease 全组合事务仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 56 | `CAP-01` descriptor/schema/policy metadata 与 handler binding | `ACTION_OPERATIONS`/`CapabilityActionDescriptor`/`PreparedAction` 与 bounded schema、risk/effect/resource/cancel/reconcile/idempotency 元数据由 domain 单一目录提供；DaemonHost 注册全部 exact bindings，Broker seal 前校验 catalog、kind、binding version、handler 数量，seal 后拒绝注册；Runner 五工具 schema/mapping 与 operator-only direct 能力分离，unknown/duplicate/readonly downgrade 不 fallback shell；新增 domain/Broker/core remote fixtures、source guard、CI workflow 与 authority baseline；不运行本地测试 | `feature_status=implemented`（domain/broker/daemon/runner/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；静态 catalog/handler seal 不等于 Grant/Approval/Permit/ExecutionScope、动态扩展 provenance、durable ToolSnapshot、handler TOCTOU/egress、外部/live/physical effect 或全量 adapter/UAT，后续 CAP-02+、CP/ER/PD/SC 继续；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 57 | `CAP-02` 统一参数边界与输入摘要 | bounded JSON parser/schema subset、duplicate key/depth/bytes/items/invalid numeric、shell string/argv/patch/MCP/Memory normalization、reserved authority field cleanup、MCP alias conflict、canonical operation/path/sandbox/defaults 与 `canonical_action_input_digest` 已统一；PreparedAction 记录 catalog/action/input digest，Broker 只接 normalized action；新增 domain/core remote fixtures/source guard/CI workflow；不运行本地测试 | `feature_status=implemented`（domain/core/runner/daemon/broker source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；JSON Schema/ref/secret/TOCTOU/egress/ExecutionScope 仍是 bounded/后续 CAP-03+，digest 不证明 handler/外部 effect，完整 adapter/UAT 和 durable snapshot 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 58 | `CAP-03` 从 authority chain 派生不可变 ExecutionScope | 新增 `kiana.execution-scope.v1` typed contract 并绑定 principal/project/session/run/turn/cell、Grant/Budget refs、roots/denies、Memory/server/network、environment/workspace、authority/trust/data/cancel epochs、deadline/fencing、catalog/action/scope digests；ControlPlane 清除 caller scope、合并 ScopeSet/action context 并写入 CapabilityRequest，Broker 必须校验 scope/digest/resource/run/turn 后才调用 handler，direct 无 Run/Turn 仍保留显式 scope；新增 domain/core remote fixtures/source guard/CI workflow 与 baseline；不运行本地测试 | `feature_status=implemented`（domain/core/broker/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；当前 epoch/fence/Environment/ExecutionContext 仍是本地受控 snapshot，完整 Grant/Approval/lease intersection/revocation、durable ToolSnapshot、TOCTOU/egress/OS containment、跨进程恢复和外部/live/physical effect 仍需 CAP-04+、CP/ER/PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 59 | `CAP-04` Capability 状态与 outcome 类型化 | `CapabilityExecutionState` 新增 queued/阶段取消、dispatch 启动失败和恢复 Unknown 的合法转移，并集中到 `can_transition_via`；新增 `CapabilityResultDimensions`（process/stop/effect/exit/failure code），归一化器拒绝 success=true 的非零 exit/未知 effect，CLI/HTTP/retry/reconcile 从同一 `CapabilityErrorCode` 派生；Invocation/attempt projection 拒绝终态复活、Unknown→Cancelled 和未声明 foreign attempt，事件 payload 不再用 result_unknown 文本猜测；新增 domain/core fixtures、source guard、CI workflow 与 baseline；不运行本地测试 | `feature_status=implemented`（domain/core/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；typed dimensions 仍是当前 handler/事件证据，permit 单次消费、跨进程 attempt ledger、外部 effect receipt/reconcile、真实 provider/connector/live/physical 仍需 CAP-05+、ER/PD/SC/INT；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 60 | `H02` Session / Run / Turn / Step 身份与生命周期 | 新增 `StepId`/`ModelAttemptId` 及 `StepIdentity`/`ModelAttemptIdentity` typed contracts，注册 ID/schema 并接入 ModelAttemptRecord；Start wire 携带可选 server `TurnId`，ControlPlane native Start/Continue、Daemon skill wrapper、Harness ActiveRun/checkpoint 全链路传播 turn，模型 step/attempt facts 记录 typed identity；旧 Start payload/legacy Continue 保持显式兼容，Run projection/ControlPlane closed-run gate 与 cross-run late-result guard 加入 H02 夹具；新增 domain/core/runner-protocol fixtures、CI workflow 与 identity baseline；不运行本地测试 | `feature_status=implemented`（domain/runner/core/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；Session 级 durable queue/driver lease、完整 RunSnapshot/InvocationLedger、跨进程恢复、H03 单一状态驱动器、H10 全程稳定 Invocation 与真实 Provider/live/physical effect 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 61 | `H03` Harness 单一状态驱动器 | 新增无 I/O `RunDriver`/`RunFrame`/`TurnFrame` reducer 与 `DriverInput`/`DriverIntent`，统一 phase、driver ownership、bounded mailbox、step/tool/result/cancel/recovery transitions；KianaHarness `model_step` 改为显式 loop，ActiveRun/checkpoint/Inbox/模型结果沿 driver 更新，未知 effect 进入 RecoveryRequired，第二 driver 和满 mailbox fail-closed；新增 runner 纯 reducer fixtures、CI workflow 与 state-driver baseline；不运行本地测试 | `feature_status=implemented`（runner source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；RunDriver 仍是进程内状态，Session durable queue/ACK、跨进程 driver lease/恢复、结构化 Provider/stream/stop/retry 仍需 H04+、H18/PD；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 62 | `H04` 结构化模型消息与无损 Provider 转换 | `ModelMessage`/`ModelOutput` 保留旧 text/tool_calls cassette 并增加 typed `ModelContent`（Text/ToolCall/ToolResult/AttachmentRef/ProviderOpaque）与 `ProviderContinuation`；history 校验统一做 orphan/role/call 绑定，Provider compiler 在请求前按最终 route 校验 provider/protocol/route digest，unsupported attachment/opaque/continuation fail-closed，不携带 raw bytes/header/response；新增 domain/provider fixtures、source guard、CI workflow 与 model-content baseline；不运行本地测试 | `feature_status=implemented`（domain/provider/daemon/runner source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；当前 attachment/opaque 仍是引用且 provider wire mapping 默认拒绝，完整 multimodal/continuation adapters、provider receipt、跨连接 live/physical effect 和 UI UAT 仍需 P4/INT/H05+；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 63 | `H05` 模型 StopReason、错误与重试类型化 | 新增闭合 `ModelStopReason`（含 Unknown）、`ModelOutcome` 与 `ModelError.side_effect_state`，legacy 缺 stop 的响应补显式 end_turn/tool_use；Harness 在完成/派发工具前拒绝 length/refusal/pause/incomplete/unknown，Provider parser 保留原始 detail 但统一 code/phase/retry/request_sent；`run.model_turn` 记录 typed stop/outcome；新增 domain/runner fixtures、CI workflow 与 stop/retry baseline；不运行本地测试 | `feature_status=implemented`（domain/runner/provider source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；重试仍受现有剩余 deadline/预算但跨 provider 完整分类、流式 accumulator、账单/外部效果与 live/physical 仍需 H06+、P4/INT；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 64 | `P4-J7-05` Provider 非流式工具响应严格解析 | `kiana-services` OpenAI-compatible/Ollama response/request 转换拒绝 exactly-one/terminal/identity/name/JSON-object/duplicate-ID 缺陷，保留 Ollama 无原生 ID 的 ordinal 特例；daemon legacy adapter 移除 tool/shell/{} 默认修复并在缺身份/坏参数前拒绝；新增 malformed/missing/duplicate/empty-object/tool-result fixtures、CI workflow 与 strict-response baseline；不运行本地测试 | `feature_status=implemented`（services/provider/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；kiana-services 仍是兼容 surface，生产 ProviderGateway 的 streaming/HTTP/auth/usage/receipt/外部 effect 与跨 provider live 仍需 P4-J7-06+、H06/INT；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 65 | `CM-01` Context/Memory 共享来源与 scope 值对象 | 新增 domain `SourceRef`/`SourceSnapshot`/`SourceKind`/`Freshness`/`EvidenceStatus`/server-owned `MemoryScope`，绑定 principal/project/session/collections/purpose，校验 digest/cursor/unknown fields 和 collection 覆盖；现有 Memory/Prompt/Artifact 消费边界保持只读，模型 arguments/path/collection 不生成 principal；新增 domain source/scope fixtures、CI workflow 与 context-scope baseline；不运行本地测试 | `feature_status=implemented`（domain source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；MemoryRecord 生命周期、processing grant/retention/revocation、ContextPlan/index generation/dirfd freshness、semantic recall 和 durable Memory scope 仍需 CM-02+、CAP/PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 66 | `CM-02` MemoryRecord 生命周期与 legacy import | `MemoryRecord` 增加 kind/purpose/sensitivity/validity/retention/dependencies/import_mode，严格校验 Candidate→Draft、Qualified→Active、Ephemeral→Active、Rejected→Rejected 组合及 qualified review/evidence/provenance；daemon v1 JSONL 走显式 `legacy_import`，强制 Unknown/Candidate/Draft/unsearchable/unverified，native writer/review 补齐元数据；新增 domain/daemon fixtures、CI workflow 与 memory-lifecycle baseline；不运行本地测试 | `feature_status=implemented`（domain/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；Memory mutation/CAS/index visibility/processing grant/retention/revocation/delete/cross-project scope、durable recovery 和 semantic recall 仍需 CM-03+、CM-04/05、PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 67 | `CM-03` 服务端派生 Memory read/write scope | `MemoryScope::intersect` 按 principal/project/session/purpose 强制同一身份、collections 取更窄交集、allow_write 只 AND；ControlPlane 对 memory.search/write 按 RoleSpec/显式 collection 派生 ExecutionScope.memory_scopes，缺 write collection/未知或越权 collection 拒绝；daemon handler 从 request ExecutionScope 重建 DomainMemoryScope 并复核 collection；新增 domain/core scope fixtures、CI workflow 与 memory-scope baseline；不运行本地测试 | `feature_status=implemented`（domain/core/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；processing grant/purpose/retention/revocation ledger、durable Memory scope/CAS/index generation、跨项目 user-private 与 semantic recall 仍需 CM-04+、PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 68 | `CM-04` 统一 Memory mutation 与幂等键 | 新增 domain `MemoryMutation`/`MemoryMutationTarget`/`MemoryMutationReceipt` 与七种 operation，绑定 server actor、MemoryScope、SourceRef evidence、policy/data epoch、payload digest、idempotency key；纯 `MemoryMutationLedger` 先预检全部 target，重复 key 返回原 receipt，payload 漂移和 stale revision fail-closed；daemon memory.write/review 在 JSONL append 前生成并应用 mutation contract，稳定 key/record identity 支持兼容 replay；新增 domain ledger fixtures、daemon source guard、CI workflow 与 mutation baseline；不运行本地测试 | `feature_status=implemented`（domain/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；CM-05 才把 Memory facts/body refs/review consumption/projection cursor 纳入 EventStore 唯一提交点，proposal 混合批次、跨进程 receipt/recovery、processing grant/retention/revocation/delete propagation 和 semantic recall 仍需 CM-05+、PD/SC；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 69 | `EXT-01` 稳定扩展领域合同 | 新增 `ExtensionId`/`ComponentId`/`SnapshotId`/`HookRunId`，以及版本化 `SkillDescriptor`、`HookDescriptor`/`HookDecision`、`PluginLifecycle`、`ExtensionSnapshot`、`ExtensionError`；校验 digest、generation/revision、重复 identity、bounded 字段和 unknown-field fail-closed，protocol 只 re-export 可序列化 contract；新增 domain/protocol fixtures、CI workflow 与 extension baseline；不运行本地测试 | `feature_status=implemented`（domain/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；EXT-02 才接 SourceResolver/ProjectTrust/path root，EXT-03 parser、EXT-04 catalog/generation、Hook process/effect、签名/持久恢复仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 70 | `EXT-02` SourceResolver、ProjectTrust 与路径根 | 新增 `kiana-skills::SourceResolver`，统一 user/KIANA_HOME/project/bundled/signed-package/plugin/external root kind、precedence、trust decision、digest-derived summary 和 resource containment；重复 canonical root、绝对/`..`/反斜杠/控制字符、root/resource symlink、root 外路径 fail-closed；loader 接入 resolver 并拒绝 symlink skill root/resource，protocol summary 不泄露绝对路径；新增 resolver filesystem fixtures、CI workflow 与 source baseline；不运行本地测试 | `feature_status=implemented`（skills/domain/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；EXT-03 严格 SKILL/manifest parser、EXT-04 catalog/generation、签名/包验证、Hook process/effect、跨进程 snapshot/失效仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 71 | `EXT-03` 严格 Skill/Plugin/Hook 解析器与兼容层 | `kiana-skills` Skill frontmatter 使用 deny-unknown-fields、严格类型/大小/metadata/path/context/tool 校验并规范化目录 slug；Plugin/Hook manifest 通过 bounded duplicate-key JSON、strict schema、唯一 component/hook id、entry/phase 检查和显式 legacy adapter；新增 parser fixtures、CI workflow 与 strict parser baseline；不运行本地测试 | `feature_status=implemented`（skills/domain/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；EXT-04 catalog/priority/generation、EXT-05 snapshot invalidation、package/signature/trust、resource existence、Hook process/effect 和 capability re-authorization 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 72 | `EXT-04` Catalog、优先级、重复和可解释性 | 新增 domain `ExtensionCatalog`/`CatalogCandidate` 与 deterministic winner/shadowed 诊断，按 kind/namespace/name/version/precedence/source/hash 固定排序；新增 `HookOrderCandidate` specificity/tie-break 排序并拒绝重复 hook id；`kiana-skills::build_skill_catalog` 将 legacy Command 汇总接入 catalog，避免 filesystem/HashMap 顺序；新增 domain/skills fixtures、CI workflow 与 catalog baseline；不运行本地测试 | `feature_status=implemented`（domain/skills source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；EXT-05 snapshot generation/invalidation、SourceResolver/签名/包验证、Hook match/process/effect、capability re-authorization 和跨进程恢复仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 73 | `EXT-05` 快照与失效 | 新增 `SnapshotCacheKey`/`ExtensionSnapshotCache`，绑定 cwd/trust/source-roots/package-generation/config/schema，重复 key 不覆盖、invalidate 保留诊断并递增 generation；SourceResolver 对 bounded root content fingerprint，动态/条件 skill/clear 触发全局 generation，legacy skill registry 使用 key+generation 避免跨代复用；新增 skills cache/resolver fixtures、CI workflow 与 snapshot baseline；不运行本地测试 | `feature_status=implemented`（skills/domain/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；package signature/manifest、跨进程 snapshot/CAS、Hook activation/process、capability re-authorization、EventLog cursor 和 durable recovery 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 74 | `UI-01` versioned UI protocol DTO | protocol 新增 `UiSnapshotV1`、`UiFeedEnvelope`、`UiActionV1`/`UiActionResult`、`UiCapability`、`UiError`、Session/Run/Item、HumanActionCard、ReceiptRef、ArtifactSummary、UiNotice、EvidenceLimitation 与 cursor/retry/disposition enums；校验 instance/authority epoch、cursor/sequence/revision、digest、大小、duplicate ID 和 unknown-field，旧 UiSnapshot/UiAction/RunStream wire 兼容；新增 protocol fixtures、CI workflow 与 UI baseline；不运行本地测试 | `feature_status=implemented`（protocol/domain source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；UI-02 handshake/error capability、UI-03 transport/instance、UI-04 action journal/CAS、snapshot projector/reconnect/durable recovery 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 75 | `UI-02` 统一错误、能力和 surface handshake | protocol 新增 UiHandshakeRequest/Response、UiHealth、UiSurface、requested capability 与 principal×surface 交集；现有 CapabilityErrorCode/ExecutionStatus 映射到封闭 UiError/StableError 和安全 retry disposition，Unknown 只能 QueryOriginal，错误使用固定文案不泄漏路径/token；kiana-client 增加 typed initialize/health 并在传输前后验证；新增 protocol/client fixtures、CI workflow 与 handshake baseline；不运行本地测试 | `feature_status=implemented`（protocol/client/domain source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；UI-03 instance/transport、UI-04 action journal/CAS、daemon handshake route/projector、cross-surface durable capability 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 76 | `UI-03` 本地实例身份、发现和 transport | protocol 新增 UiTransportKind/UiInstanceRecord，绑定 instance/authority epoch、protocol/workspace/endpoint digest、PID、ready 和 record digest；daemon `InstanceLease` 以 create-new lock/0600 record 保证单实例，discover 要求单一 ready record、严格权限和 peer workspace/protocol/epoch 校验，DaemonHost 暴露显式 acquire_instance；新增 daemon instance fixtures、CI workflow 与 instance baseline；不运行本地测试 | `feature_status=implemented`（daemon/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；UI-04 action journal/CAS、真实 Unix socket/named pipe listener、PID/restart epoch durable proof、snapshot projector/reconnect/cross-surface handshake 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 77 | `CO-02` 稳定组织、业务项目与工作区绑定 | 新增 WorkspaceId 与 OrganizationBinding/WorkspaceBinding/ProjectBinding/CompanyScope/CompanyScopeRegistry，canonical root 仅生成 workspace digest，组织/项目 ID 分离；同 workspace 可绑定多个 project 但 scope digest/authority 隔离，membership/归属/foreign project/legacy stream ambiguity/非 canonical root fail-closed；core Company context 拒绝 relative/`..` workspace root；新增 domain fixtures、core guard、CI workflow 与 company scope baseline；不运行本地测试 | `feature_status=implemented`（domain/core/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；CO-03 assignment/expiry/revocation/server identity、CompanyState legacy String→typed upcast、durable binding/CAS/recovery 和完整业务权限仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 78 | `CO-03` 角色任命、有效期与撤销接入服务端身份 | 新增 AssignmentId/ProjectAssignmentId、RoleAssignment/ProjectAssignment/ResolvedAssignment 与 server-owned AssignmentDirectory；端口提供 AssignmentDirectoryPort，daemon 以 authenticated principal 与 root-derived ProjectIdentity 解析并绑定 context，core 在 Company 边界重验证 actor/role/department、有效期、principal expiry 和 authority epoch；撤销级联项目范围，unknown/duplicate/window/digest/impersonation/expiry fail-closed；新增 domain/core fixtures、CI workflow 与 assignment baseline；不运行本地测试 | `feature_status=implemented`（domain/ports/core/daemon/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；当前 adapter 是 in-process snapshot，Assignment facts/membership/epoch 尚未 durable 落盘或 upcast 到 CompanyEvent，旧入口仍需后续逐条接入，完整 Company capability/approval/effect revalidation 继续由 CO-05+ 收口；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 79 | `CO-04` 五部门与专业岗位成为版本化目录 | RoleSpec/DepartmentSpec 增加 schema/version、input/output schema、prompt hash 和固定 model profile；RoleCatalog/DepartmentCatalog 提供 deterministic digest，新增 Analyst、QA、Librarian role packs；PromptBundle、ModelAssignment、run.authorized/session/receipt provenance 携带版本与 profile，未知角色/工具/profile/metadata drift fail-closed；daemon/provider 继续复用既有 Harness/ProviderGateway，ProjectTrust 控制 skills；新增 domain/core fixtures、CI workflow 与 role catalog baseline；不运行本地测试 | `feature_status=implemented`（domain/core/provider/daemon/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；目录是 built-in snapshot，尚无 durable/signature/hot-update catalog 或真实多模型 live 证据，assignment/Company command 权限仍由 CO-03/CO-05+ 收口；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 80 | `CO-05` 业务命令权限、责任和人工决定合同 | 新增 `CompanyCommandPolicy`、`DecisionPurpose`、`DecisionActorKind`、`HumanTask`/`HumanDecision` 与版本化 schema；每个 CompanyCommand 显式角色/actor kind/purpose/evidence/assignment 元数据，未知变体默认空矩阵拒绝；ControlPlane 在 CompanyState 读取前执行 policy，生成绑定 command target revision/digest、scope、decider、选项、期限和 epoch 的 HumanDecision，agent/SponsorProxy/Builder 自批拒绝；CompanyProof 同事件记录决定，旧 replay 缺失字段保持兼容；新增 domain/core fixtures、CI workflow 与 policy baseline；不运行本地测试 | `feature_status=implemented`（domain/core/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；HumanTask/inbox 尚未独立 durable，actor kind 仍由 server RequestContext cell/service 标记推导，完整 object-state/assignment/artifact/evidence/receipt 约束继续由 CO-06+ 收口；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 82 | `CO-07` 版本化业务事实与稳定命令回执 | 新增 CompanyCommandReceipt/DispatchIntent，稳定 logical command ID、payload/authority digest、expected/committed revision、event ID、Committed/Replayed/ResultUnknown 状态；Company command replay 在原 Company stream 上核对 request/actor/role/session 并返回 typed receipt，StartRun 与受限 delivery/cancel effect 写入 prepared intent，复用 protected command + EventStore TransitionBatch/read_command CAS；旧 event/response 字段保留；新增 domain/core fixtures、CI workflow 与 receipt baseline；不运行本地测试 | `feature_status=implemented`（domain/core/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；receipt/intent 尚未独立 durable query/consumer、dispatch success/unknown recovery 和完整 state migration/upcast，不能由 receipt 推断现实副作用；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 83 | `CO-08` 业务状态机、历史重放与兼容迁移 | 新增 CompanyReplayReducer，检查 company aggregate/root/owner、stream_version 连续性、event kind/idempotency、expected revision、重复/回退/gap/unknown schema 和 pure CompanyState transition；显式支持 `kiana.company-event.v0`→v1 标签迁移，core load_company 统一复用 reducer；新增 domain/core replay fixtures、CI workflow 与 replay baseline；不运行本地测试 | `feature_status=implemented`（domain/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；尚无独立 durable snapshot/projector/cursor migration journal，v0 仅支持当前字段 shape，typed refs/多 aggregate/business state upcast 和后续 object-level acceptance 留待 CO-09+；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 84 | `P0-A-01b` schema 注册表与 unknown field/migration 规则 | 复核并 CI-wiring 现有 `SCHEMA_CONTRACTS`/`SchemaLayer`/`SchemaVersion` 与 `EVENT_KIND_SPECS`/`event_migration`；wire additive minor、domain/runtime unknown field、unknown schema/major/required event 和未登记 migration fail-closed；新增 domain/core fixtures、CI workflow 与 schema baseline；不运行本地测试 | `feature_status=implemented`（domain/protocol/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；registry 不等于所有 legacy payload upcaster 或 durable migration runner，未知原始事实保留/隔离、全量 event/projector coverage 留待 ER/PD/DEP；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 85 | `P0-A-02` 稳定错误码枚举 | 复核并 CI-wiring 现有 CapabilityErrorCode/CapabilityErrorPolicy、CapabilityResult::failure_code 与 protocol ResponseEnvelope::failure_policy；固定 CLI exit、HTTP status、retry/new authorization/compensation/reconciliation，path_escape 与 result_unknown 负向路径和 unknown reason 保守归类；新增 domain/core fixtures、CI workflow 与 error-codes baseline；不运行本地测试 | `feature_status=implemented`（domain/protocol/core/entrypoints source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；错误 policy 不授权重试或证明真实 effect/reconciliation，新增错误码和所有入口映射仍需后续维护；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 86 | `P4-J7-06` Provider 中立内容、调用身份、错误与模型端口 | 复核并 CI-wiring typed ModelContent/ProviderContinuation、ModelCallSpec/PreparedModelCall、ModelFinish/Outcome/Error/Usage 与唯一 ports ModelClient；legacy text/tool_calls 与 typed block 冲突拒绝，unknown contract major、unsupported/cross-provider content fail-closed，Runner 仅 re-export、ProviderGateway 只消费 prepared/admitted contract；新增 domain/core fixtures、CI workflow 与 provider-model baseline；不运行本地测试 | `feature_status=implemented`（domain/ports/runner/provider/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；P4-J7-07 provider facade/migration、P4-J7-12 全协议编译、P4-J7-14 accumulator、跨 provider live/physical 与 provider receipt 仍未完成；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 87 | `P4-J7-07` 提取 kiana-provider 并迁移装配 | 独立 kiana-provider crate 公开唯一 ProviderGateway→ports ModelClient，manifest/source 不依赖 services/core/entrypoints/runner；daemon 生产分支只构造 ProviderGateway，kiana-services provider 仅保留 cfg(test) legacy fixtures，Runner 不依赖 provider；新增 extraction source guard、CI workflow 与 provider-extraction baseline；不运行本地测试 | `feature_status=implemented`（provider/daemon/ports/runner source + remote guard wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；kiana-services 公共兼容 facade 尚未逐协议委托/移除，profile/credential/config、全协议编译、stream accumulator、provider receipt/live 仍由 P4-J7-08+ 收口；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 88 | `P4-J7-08` 连接、profile 和配置快照 | 新增 ProviderProfileSnapshot/ProviderConfigSnapshot，分离 provider/protocol/connection/model/profile/profile_version/credential_ref/source；ProviderGateway catalog 暴露 secret-free configuration digest，route configuration_revision 对配置变化敏感；daemon `KIANA_MODEL_MODE` 明确 live/cassette 冲突与缺 cassette，invalid streaming 先拒绝，provider unknown profile/inherit/key/capability/concurrency 校验保持 fail-closed；新增 provider/core fixtures、CI workflow 与 config baseline；不运行本地测试 | `feature_status=implemented`（domain/provider/daemon/core/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；ConfigSnapshot 尚无 durable store/hot-update CAS/跨进程 epoch，活动 Run route/freshness、SecretStore/rotation/live proof 留待 P4-J7-09+；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 89 | `P4-J7-09` 凭据管理与 HTTP 目标校验 | provider credential 只经 explicit/env reference，snapshot/catalog/Debug 仅保存 digest；坏 header、缺 key、userinfo/query/fragment、非 TLS/非 loopback、invalid concurrency/streaming fail-closed，reqwest redirect/proxy disabled，credential revision 纳入 route revision，项目/role text 不能改 endpoint；新增 provider/core fixtures、CI workflow 与 credentials baseline；不运行本地测试 | `feature_status=implemented`（provider/daemon/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；SecretStore/OAuth rotation/revocation/DNS rebinding/真实 TLS/live 仍未证明，capability/role permit 由后续 P4-J7-10/11 收口；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 90 | `P4-J7-10` 模型能力目录、unknown capability 与显式 discovery | 新增 typed ModelCatalog/ModelCatalogEntry，绑定 provider/connection/model 原样 ID、Supported/Unsupported/Unknown capability、source/expiry/catalog revision/digest；Gateway 投影 configured catalog，same-name 模型无 connection_id 时 ambiguous，slash model ID 保留，catalog 不能授权 tools/route；新增 provider/core fixtures、CI workflow 与 model catalog baseline；不运行本地测试 | `feature_status=implemented`（domain/provider/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；Cache/LiveDiscovery 尚无 durable refresh/signature/expiry store，活动 PreparedModelCall route、role/permit/capability enforcement 与真实 transport 仍待 P4-J7-11+；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 91 | `P0-B-01` 正式状态机转移表 | 复核并 CI-wiring domain Approval/CapabilityExecution/RunCancellation/WorkPacket/Execution/Company transition matrices 与 terminal predicates；illegal transition、terminal reopen、result_unknown→success/auto-retry fail-closed，compressed event 只走 transition_via；core lifecycle/dispatch 与 Runner state driver 消费同一契约；新增 domain/core fixtures、CI workflow 与 state baseline；不运行本地测试 | `feature_status=implemented`（domain/core/runner source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；逐个 Company object edge/legacy projector、跨入口 UAT 和 durable transition proof 仍需 CO/ER/PD 后续步骤；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 92 | `P0-K1-01` 服务端身份与 authority epoch | DaemonHost 以 authenticated principal 覆盖 wire actor，ProjectTrustAuthority/canonical root/device/inode 派生 ProjectIdentity；首次 effectful session 以 CAS 固化 typed SessionAssignment，后续 role/department 变更 fail-closed；authority stream 的单调版本写入 assignment 与 `run.authorized.authority_epoch`，继续复用 ControlPlane owner/approval/Broker 主路径；新增 ingress fixture、core source guard、CI workflow 与 identity-authority baseline；不运行本地测试 | `feature_status=implemented`（domain/core/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；principal 仍为固定 local-user 兼容身份，OS/OAuth/tenant provider、durable assignment/Membership/Grant ledger、跨进程 epoch recovery 和完整 effect-time revalidation 仍由 CI/CP/SC/PD 后续步骤负责；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 93 | `P1-C-01` 组织与 Cell 契约 | `AgentTemplate`、`CellSpec`、`SpawnPlan`、`BudgetLease`、`CapabilityGrant`、`SupervisionLease` 六类合同统一由 domain 暴露，DTO 拒绝 unknown fields；模板 version/id 与 Cell 绑定，默认不委派，child grant 的 capability/operation/resource/path/expiry/delegation 只能是 parent 子集；CellRegistry 在 reserve 与 snapshot 恢复重复检查并 fail-closed；新增 domain fixture、core source guard、CI workflow 与 cell-contract baseline；不运行本地测试 | `feature_status=implemented`（domain/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；MemoryCellRegistry 仍是进程内 adapter，durable Cell/Grant/Budget/Lease projector、跨进程恢复和完整 scheduler/Swarm 生命周期留待 P1-C-02/SW/AUT/ER/PD；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 94 | `P1-C-03` 五部门角色目录与 model_profile 接线 | RoleCatalog/DepartmentCatalog 固定五部门九岗位，RoleSpec 校验 department/profile/prompt/I/O/tool metadata；ControlPlane 将 server-owned ModelAssignment.profile 写入每次 run，ProviderGateway 只按该 assignment 选择 configured connection，planning/executing/quality 可落到不同模型；新增独立 provider route fixture、core source guard、CI workflow 与 role-model-routing baseline；不运行本地测试 | `feature_status=implemented`（domain/core/provider source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；catalog 仍是 built-in snapshot，assignment/durable catalog/permit/真实 provider transport 与 live 多模型证据留待 CO/CI/P4-J7-11+；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 95 | `P1-D-01` WorkPacket 单一 ready 谓词 | `kiana_domain::ready_packets(graph, now)` 统一执行 identity/DAG、依赖状态、deadline 和 claim lease 判断；CompanyState/ControlPlane 直接消费，legacy `kiana-tasks` 仅委托 wrapper，不复制算法；新增跨 crate readiness fixture、core source guard、CI workflow 与 ready-predicate baseline；不运行本地测试 | `feature_status=implemented`（domain/core/tasks source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；readiness 仍为只读投影，依赖成环 admission、claim reclaim、durable queue/scheduler 和旧 board completion gate 仍由 P1-D-02/03、AUT/PD 后续步骤负责；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 96 | `P1-D-02` 依赖缺失 / 成环 fail-closed | `validate_dependency_dag` 拒绝 identity 错误、重复/缺失依赖和超大图，并将环规范化为稳定 cycle；CompanyState `ApprovePacket` 在追加候选前把完整项目图过 DAG 校验，PlanProject/Claim/dispatch 继续复用同一 helper；新增 domain deterministic graph fixtures、core source guard、CI workflow 与 dependency-graph baseline；不运行本地测试 | `feature_status=implemented`（domain/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；依赖图仍是结构校验，自动状态派生、claim reclaim、durable queue/scheduler 和完整跨入口行为留待 P1-D-03/AUT/ER/PD；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 97 | `P1-D-03` claim / lease 心跳回收 | `PacketClaim` 绑定 owner/session/heartbeat/expiry，ClaimPacket/StartRun/turn guard 只由当前 owner 在 live lease 内续租；ControlPlane 有界扫描过期 claims 并生成幂等 `ReclaimPacketClaim`，无 run 的 stale claim 才回到 ready，已 dispatch 必须 terminal observation，ResultUnknown 不自动 retry；新增 domain lease fixtures、core source guard、CI workflow 与 lease-recovery baseline；不运行本地测试 | `feature_status=implemented`（domain/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；claim/Company state 仍主要由现有 adapter 提供，跨进程 worker death、durable lease projector、queue fairness/backoff、scheduler heartbeat 和完整对账仍留待 AUT/SW/ER/PD；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 98 | `P1-E-01` 通信与问责分层 | 新增七类 `CommunicationMessageKind`（Chat/Command/Handoff/Decision/StatusReport/Evidence/Incident）与 digest/unknown-field 校验；Chat 禁止 action/ACK 且 `grants_authority=false`，`communication.send` 由 ControlPlane 以 server sender 验证后记录 formal EventLog fact；Handoff 要求定向 recipient 和 ACK，既有 PacketHandoff 复用目标 role/session/expiry 校验；新增 domain/core fixtures、ports/core source guard、CI workflow 与 communication baseline；不运行本地测试 | `feature_status=implemented`（domain/ports/core/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；消息投影、通知 outbox/delivery/read-state、跨进程送达和外部 channel 仍由 NM/ER/PD/INT 后续步骤负责；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 99 | `P1-H-01` ToolSpec registry | 新增 domain `ToolSpec`/`TOOL_SPECS`，固定五个模型工具的 canonical name、alias、capability、operation、risk、side_effecting、schema；`model_tool_name` 与 Runner canonical mapping 统一消费 registry，DaemonHost 组合时校验 surface/alias/schema drift；新增 domain/core fixtures、CI workflow 与 tool-authority baseline；不运行本地测试 | `feature_status=implemented`（domain/runner/core/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；registry 仍不授予 capability/path/approval，operator-only action、containment、Broker handler 和完整跨入口 UAT 留待 P1-H-02/03、CAP/CP；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 100 | `P1-H-03` 路径 containment 共享实现 | domain 新增 `enforce_path_containment`/`enforce_root_containment`，统一 normalize/allow-list/root lexical checks；apply_patch、shell workdir/patch、package、checkpoint、execution workspace、Cell grant snapshot 与 event scope 均消费 helper，MCP/Memory 保留 project-root/scope guard；新增 domain/core negative fixture/source guard、CI workflow 与 path-containment baseline；不运行本地测试 | `feature_status=implemented`（domain/core/daemon source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；helper 不等价于 fd-relative/no-follow、symlink/hardlink/rename/effect-time TOCTOU，完整 filesystem/secret/network containment 留待 CAP/SC/ER/PD；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 101 | `P3-I-01` Company 业务对象契约 | domain 固定 Objective、Initiative、Project、Milestone、Acceptance、Delivery、Outcome、ChangeRequest、Risk、Incident 十类对象及状态宏；补 Acceptance/CriteriaSnapshot/CompanyReview/MetricObservation 的 deny-unknown-field 与 identity/version/criteria/measurement/evidence validate，CompanyState transition 仍是唯一状态入口；新增 domain object fixture、core source guard、CI workflow 与 company-object baseline；不运行本地测试 | `feature_status=implemented`（domain/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；command/event freeze、durable replay/projector、完整业务闭环和现实 outcome 仍留待 P3-I-02+、CO/ER/PD；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 102 | `CI-02` Domain identity/config/authority contracts | 新增稳定 PrincipalId/ProviderAccountId/ServiceIdentityId 与 Principal/Membership/SecretRef/ProviderAccount/ServiceIdentity/ConfigSnapshot/AuthoritySnapshot typed DTO；全部 strict/digest/expiry/epoch 校验，ConfigSnapshot 拒绝 raw token/bearer/key，AuthoritySnapshot 拒绝 rollback/stale epoch；协议重导出并新增 domain/core CI-only fixtures/source guard；不运行本地测试 | `feature_status=implemented`（domain/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；SecretStore/CredentialLease/OAuth、durable identity/assignment/revoke projector、跨进程 authority recovery 和 provider effect boundary 仍由 CI-03..12、CP/SC/PD 负责；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 103 | `CI-03` Identity/config/credential ports | `kiana-ports` 新增 `IdentityResolver`、`CredentialResolver`、`ConfigSnapshotStore`、`CredentialRotationPort`/`RotationRevokePort`，返回 Principal/AuthoritySnapshot、immutable ConfigSnapshot 和仅含 SecretRef/status/expiry/digest 的 CredentialResolution；generation CAS/错误 resolver/版本边界 fail-closed；新增 ports fixture、core source guard、CI workflow 与 ports-identity baseline；不运行本地测试 | `feature_status=implemented`（ports/domain source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；无生产 IdentityResolver/ConfigSnapshotStore/CredentialStore adapter，SecretStore/lease/OAuth、durable identity/revoke、provider effect boundary 仍由 CI-04+、CP/SC/PD 负责；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 104 | `CI-04` 受保护 Daemon ingress 与本地主体迁移 | RequestMetadata 增加可选 instance/origin/host/opaque credential_ref/identity_mode，DaemonHost 在任何 core 处理前执行 loopback、bounded、SecretRef 和 protected credential presence 校验；server principal/ProjectTrust/SessionAssignment 仍覆盖 wire actor/role/trust，IdentityMigration 明确记录 legacy local-user 而不授予 authority；新增 domain/protocol/daemon fixtures、core source guard、CI workflow 与 daemon-ingress baseline；不运行本地测试 | `feature_status=implemented`（domain/protocol/daemon/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；真实 bearer/Unix peer credential、OS/keyring/OAuth/tenant auth、instance ownership、durable migration 和跨进程 session recovery 仍由 CI-05+、CP/SC/PD 负责；下一步领取总 roadmap 中下一个无前置且未完成 step |

| 当前 105 | `CI-05` durable authority ledger | 新增 PolicyProfile/DataBoundary/SharingGrant typed contracts 与 IDs，复用 Membership/Role/ProjectAssignment；`AuthorityLedger::rebuild/apply_event` 从 authority EventStore facts 重建并拒绝版本 gap/unknown/epoch rollback/stale，sharing_operations 只返回当前 epoch、未撤销未过期的跨项目操作；core authority_epoch/ledger 读取复用 reducer；新增 domain/core fixtures、CI workflow 与 authority-ledger baseline；不运行本地测试 | `feature_status=implemented`（domain/core/protocol source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；现有生产 authority stream 尚未写入全部 assignment/policy/share facts，durable projector、revoke propagation、Grant/Approval/Cell 全维度交集和跨进程 recovery 仍留待 CI-06+、CP/SC/PD；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 106 | `SW-01` typed Swarm IDs、schema 与 lineage | 新增 `SwarmPlanId`/`PartitionId`/`ChildCellId`/`AttemptId`/`DispatchIntentId`/`QueueEntryId`/`MergeDecisionId`，strict `SwarmLineage` 绑定 parent/root/workflow/correlation/causation、authority epoch/revision 与 canonical digest；protocol/ports 导出 typed boundary，保留现有 `commit_swarm`/Company EventLog 唯一执行路径；新增 domain/core fixtures、CI workflow 与 swarm-lineage baseline；不运行本地测试 | `feature_status=implemented`（domain/protocol/ports/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；lineage 尚未写入现有 Partition/Attempt/DispatchIntent durable facts，WorkGraph validator、queue/claim、child execution、replay/recovery 与 scheduler 仍由 SW-02+ 负责；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 107 | `SW-02` typed Partition/WorkGraph validator | 新增 strict `Partition`/`SwarmWorkGraph` 与 `PartitionProjection`，输入 digest、output contract/version、typed IDs、path/data scope disjoint、共享 `packet_graph` 拓扑校验、cycle/missing/duplicate/fingerprint、`first_success` 和 count/depth/concurrency/spawn-rate/TTL/budget 限额拒绝；`SwarmPlan.work_graph` 存在时在 Create 写入前校验，保留旧计划兼容路径；新增 domain/core fixtures、CI workflow 与 swarm-work-graph baseline；不运行本地测试 | `feature_status=implemented`（domain/protocol/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；optional graph 尚未覆盖所有旧 Swarm 计划，DispatchIntent/QueueEntry/claim/scheduler/child lifecycle/replay/recovery/effect-time fencing 仍由 SW-03+ 负责；下一步领取总 roadmap 中下一个无前置且未完成 step |
| 当前 108 | `SW-03` typed Swarm/Partition/Attempt reducer | 新增 strict `SwarmTransitionEvent` 与 `SwarmTransitionEntity`，`SwarmTransitionReducer` 统一 live/replay，绑定 `delegation.*` kind、revision/authority epoch、correlation/causation/digest；terminal reopen、Unknown→success、非法边、merge 无 review、版本 gap/epoch 回退 fail-closed；legacy `SwarmEvent.transitions` 可选批次由 core load 校验，不新增执行循环；新增 domain/core fixtures、CI workflow 与 swarm-reducer baseline；不运行本地测试 | `feature_status=implemented`（domain/protocol/core source + remote fixture wiring）、`proof_level=source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 已触发且未等待；旧 SwarmState 命令尚未自动生成完整 typed Partition/Attempt facts，dispatch/claim/resource/child/recovery projector 仍留待 SW-04+；下一步领取总 roadmap 中下一个无前置且未完成 step |

**当前切片的验收断言（CAP-00；仅由 GitHub CI 执行运行时测试）**

| 顺序 | 测试名 | 必须观察到的断言 |
|---|---|---|
| 先拒绝 | `malicious_unknown_tool_is_denied_before_the_broker` | 未知模型工具在 Broker 前被拒绝，handler 调用计数为零 |
| 先拒绝 | `incomplete_cell_capability_scope_is_rejected_before_broker` | 缺 mandatory scope 的请求不会通过宽权限默认值进入执行器 |
| 先拒绝 | `static_registration_rejects_duplicate_handler_keys` | 重复 capability handler key 在注册阶段 fail-closed |
| 先拒绝 | `malicious_apply_patch_parent_escape_is_denied_by_path_allowlist` | patch 路径越界在 adapter 执行前拒绝 |
| 先拒绝 | `oversized_mcp_inputs_fail_closed` | MCP 输入/帧超过边界时拒绝，不把截断数据送到远端 |
| 再成功 / 回归 | `model_written_memory_stays_unsearchable_until_approved` | 模型写入固定为 candidate/draft，审批前检索不可见；CI 提供运行时回执 |

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
| 2026-09-14 | `P0-J1-05a` 回归：model-config daemon 复用完整 `RuntimeConfig`，wall-time 非法值与耗尽路径 fail-closed 验收已入库；不运行本地测试，静态检查通过，CI 已触发但未等待 | `dd6a8d5` |
| 2026-09-14 | `P0-J1-05b` 收口：角色/环境 max_steps 经 DaemonHost 产品链生效，Receipt 记录授权上限，Start 命令与 Continue/run 隔离行为断言入库；不运行本地测试，静态检查通过，CI 已触发但未等待 | `bd9dea0` |
| 2026-09-14 | `P1-J3-01` 收口：模型记忆写入服务端固定为 `origin=model` + `candidate/draft`，默认检索排除；补 v1 存量兼容和 operator `memory.review` 晋升链；不运行本地测试，静态检查通过，CI 已触发但未等待 | `758ffbe` |
| 2026-09-14 | `CP-00` 收口：固定 DaemonHost/ControlPlane/Runner/Broker/Approval/Hook 入口矩阵、失败分类、源码测试索引和 CP-04/05 handoff；不运行本地测试，静态检查通过，CI 已触发但未等待 | `f366436` |
| 2026-09-14 | `ER-00` 收口：固定 Event/Receipt/Recovery 事实边界、EventStore capabilities、最小 ID 链、缓存与空/失败读取区分、结果未知分类和 source-only 验收索引；不运行本地测试，静态检查通过，CI 已触发但未等待 | `0a29de5` |
| 2026-09-14 | `CAP-00` 收口：固定 Capability registry、scope、cancel、patch、MCP、memory 六条调用链，五工具与 operator-only 边界、timeout/失败口径、CP/H handoff 和 source-indexed CI 验收索引；不运行本地测试，静态检查通过，CI 已触发但未等待 | `19b6fe6` |
| 2026-09-14 | CI 基线修复：kiana-query 六处陈旧 hash 长度断言对齐 `sha256:<hex>`、kiana-provider 补 `repository.workspace`（连续 8 个 CI run 红的三个 preflight 阻塞之二）；rustfmt 漂移与 unused import 清理 | `6b18a49` + `8c8f3d7` |
| 2026-09-14 | `H01` 收口：六段调用链对账（CLI/Host/Core/Runner/Broker/Provider-Receipt）落 `roadmap/harness-baseline.md`；runner/daemon 验收骨架入库（`8709b71`）；修正 roundtrip 断言为 `capability.completed`（成功路径不产 `run.tool_result`，该 kind 仅取消路径）；静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-14 | `P4-J7-04` 收口：固定 Provider 产品路径与 `#[cfg(test)] legacy_fixtures` 分界（model_client.rs 产品代码仅 1–39 行，kiana-provider 是唯一网络化产品路径且零测试）；调研 §2 十项缺口逐条复核，7 项已被 kiana-provider 修复、3 项部分成立；五个验收测试的代码归属索引入库；不运行本地测试，静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-14 | `CM-00` 收口：`kiana-query/tests/context_memory_baseline.rs` 落地两个验收测试（12 入口文件 SHA-256 快照 + 72 目录 reference inventory，漂移即红）；29-agent survey + 24 项对抗复核确认 §24.2 缺口全部仍成立，含蒸馏零测试、ONNX 未实现、JSONL 双提交点等；基线文档 `roadmap/context-memory-baseline.md`；不运行本地测试，静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-14 | `EXT-00` 收口：固定 skills/plugins/hooks 基线（11 文件 hash + 研究文档 hash），三列 gap matrix 入库——`activate_conditional_skills_for_paths` 与 `register_extension_static` 均零调用者、plugins.rs 明文 manifest 与 ExtensionRegistry 签名验证两套并行、daemon/query 两侧 UpdateInput 语义分裂、domain extensions.rs 339 行零测试；三项架构阻断检查（无第二 runner loop、入口不判权、Prompt 非授权来源）全部通过；不运行本地测试，静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-14 | `UI-00` 收口：四入口（CLI/Web/Workbench/Desktop）→ DaemonHost 调用图表 + 8 文件 hash 固定；7 项拒绝需求对抗复核：3 已覆盖（untrusted write、Host/Origin、unknown command），4 项 RED 全部有实现锚点但零测试（stale cursor、foreign-session cancel、响应丢失 result_unknown 呈现、旧 epoch 409）；`tui` 维持 parked；不运行本地测试，静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-14 | `CO-01` 收口：CompanyOS 交接基线（13 文件 hash）——46 个 CompanyCommand 变体全部接线（事件溯源聚合 + revision CAS + 幂等重放 + spawn_from_packet）但经命令路径零测试；卡上两个验收测试均为 docs-only 目标；CO-27 等待环触发条件定位（RequestAcceptance 全完成检查 + milestone 依赖 Accepted 检查即环本身）；五部门六角色验证通过；不运行本地测试，静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `P0-G-04` 事件重建投影源码收口：Invocation 请求/审批/派发/终态折叠、惰性缓存失效、冲突终态 fail-closed、预准备拒绝事实和审批恢复绑定已接入；新增重启/冲突验收测试；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | `38f23bc` |
| 2026-09-15 | `CI-01` 配置/凭据/身份基线收口：ProviderConfig Debug 脱敏、env/profile precedence 与 unknown-field 护栏、七类输出通道 sentinel fixture、legacy parser/local-user/config migration 边界、GitHub Actions 专用测试 job；不运行本地测试，静态检查通过，CI 已触发但未等待 | `1be7326` |
| 2026-09-15 | `SW-00` Swarm 现状 reconciliation：固定 10 个入口源码 hash；确认 protocol→ControlPlane→EventLog→Company StartRun→packet admission 已接线，同时记录 MemoryCellRegistry 进程内边界、event-before-dispatch 窗口、fresh child/queue/attempt/recovery 缺口；新增 GitHub Actions source-only guard；不运行本地测试，静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-00` Observability / Audit 现状 inventory：固定 19 个事实与投影入口源码 hash；登记 RuntimeEvent/EventStore/CommandReceipt/Receipt/RunStream/GoldenTrace/usage/Incident signal matrix、代码 owner、proof ceiling 与 OA-01+ 迁移清单；确认 golden trace ≠ telemetry trace、Receipt ≠ business Outcome、UI/transcript/cache/metric ≠ authority；新增 GitHub Actions source-only guard；不运行本地测试，静态检查通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-01` Domain schema 注册：`kiana-domain` 新增 ObservabilityRecord/AuditRecord/MetricCatalog/MetricPoint/TraceSummary 合同，注册四个 versioned schema，统一 unknown-field、minor compatibility、source cursor、bounded attributes、canonical digest 与 metric catalog membership 的 fail-closed 规则；新增远端 `oa01_contracts` 测试工作流；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-02` CorrelationContext/TraceRef/SpanRef：注册 correlation schema，严格解析 W3C traceparent 并将外部 parent 降级为 link；服务端从 RequestContext/scope/epoch 派生 root，run→turn→invocation→attempt command 绑定和跨 project/session、actor/epoch 错配均拒绝；异步恢复使用新 span + FollowsFrom，新增 domain/ports 远端护栏；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-03` RedactionProfile/bounded signal encoder：注册 profile schema，按 log/metric/trace/audit/export 绑定 DataClass、字节/深度和 digest；复用 redact_value/redact_text，残余 secret sentinel、NUL、深度、编码或大小错误全部 fail-closed 且不回退原文；新增远端跨信号夹具；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-04` Audit taxonomy / reducer：固定 command/authorization/approval/capability/credential/recovery/query/export action/decision 映射；仅从带 EventCursor、source binding、epoch 和唯一 source ID 的 committed RuntimeEvent 派生服务端 actor、redacted action/reason digest 与 AuditRecord；自报 `audit.*`、伪造 actor/record、缺 source/epoch、重复或矛盾 decision fail-closed；新增 domain/core 远端测试工作流；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-05` Observability/Audit ports and fakes：在 `kiana-ports` 固定封闭 signal union、sink/query/health traits、capability negotiation、append/flush ack、bounded audit query page；新增 versioned `HealthSnapshot` 与 Memory/JSONL non-durable fake，支持失败注入、容量/取消拒绝和 health/query 夹具；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-06` EventStore commit observer：在 `kiana-ports` 固定 `CommittedTransition`/receipt-cursor-source binding 与 observer failure contract；新增 `kiana-eventlog::StreamEventStore`，仅 fresh `Committed` 顺序通知，`Replayed`/CAS conflict/Unknown 不通知，observer failure 不改写已提交 outcome；新增远端 commit/replay/conflict/unknown/伪造 receipt 夹具；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-07` Run/Turn/Invocation span 生命周期：新增 `kiana.span-lifecycle.v1` domain contract 与稳定 ID/边界校验；`kiana-core::span_projection` 从 EventLog 派生可重建的 start/pause/resume/checkpoint/end 记录，绑定 source cursor/event，拒绝 terminal 冲突、忽略迟到/重复/旧 attempt，并提供只读 ControlPlane bridge；远端 CI 专用生命周期、重放与 stale-event 夹具；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-08` Provider/model/stream/usage instrumentation：新增 `kiana.model-attempt.v1` 与 committed `run.model_turn` reducer，安全摘要只保留 route/prompt hash、provider/model、stream/latency/stop/usage/retry/cache；provider/daemon 在 prepared boundary 复用 allow-list，malformed/truncated/timeout/retry/missing usage 均不标 `ok`，prompt、header、raw response 不进入 projection；新增 core/provider 远端夹具与 CI job；不运行本地测试，格式与 workspace 编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-09` Broker/approval/effect/stop instrumentation：新增 `kiana.capability-attempt.v1` 与 `CapabilityAttemptRecord`，从 committed request/decision/approval/permit/dispatch/execution/result/cancel facts 派生 admission、approval、effect、stop、fencing、zero-effect 和 bounded error；ControlPlane 在 handler 前 CAS 提交 `invocation.executing`，daemon shell/patch 输出补充有界 effect/stop metadata；policy/hook deny、expired approval、TOCTOU/lease mismatch、cancel 未确认均不伪造成功；新增远端 core 夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-10` EventLog/projector/Receipt/Artifact/Recovery metrics：新增 `kiana.metric-snapshot.v1`、`MetricSnapshot` 和只读 `project_operational_metrics`/`ControlPlane::operational_metrics`；以 committed EventLog 计算 durable/projector cursor、lag、commit/append/flush/rebuild/query latency、orphan/unknown、artifact bytes、last-error presence，空源和 cursor gap fail-closed，未知/孤儿/读取失败只返回 degraded + bounded limitations；新增远端 metrics 夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-11` Health snapshot/readiness/liveness/component capability：扩展 `HealthSnapshot` 的 probe kind、component state/version/last-success/limitation；core 只读聚合 EventLog operational metrics 与 EventStore capability，空源、cursor gap、projector lag/inferred checkpoint、Unknown/orphan、存储能力不足均不伪造 ready；DaemonHost 复用 ControlPlane health bridge；新增远端 health 夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-12` Metric catalog/reducer/cardinality guard：扩展 `MetricCatalog`/`MetricPoint` 的 counter/gauge/histogram 单位与 measured/estimated quality；新增 `MetricCardinalityGuard`，拒绝 raw ID/path/prompt/secret 标签和值并限制 series/label value cardinality，overflow 不静默；新增 `MetricReducer` 让 replay/live 共用 catalog、cursor、counter 单调性和 catalog digest 校验；新增远端治理夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-13` bounded observability queue/backpressure：新增 `kiana-ports::ObservabilityQueue`，区分 Event/Audit/Approval/Recovery/terminal critical 与 log/trace/metric best-effort；队列满时只驱逐 best-effort，critical 立即返回 `critical_queue_full` 且不阻塞已提交事实；记录 drop/reject reason/counter，支持 dequeue、flush/cancellable flush、shutdown/reopen；DaemonHost 暴露同一队列并让 drop/reject 降级 telemetry health；新增远端队列夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-14` Trace exporter/W3C context adapter：新增 `kiana.trace-export-span.v1` 与受限 `TraceExportSpan`，外部 traceparent 只作为 `ForeignParent` link；core 本地 bounded/no-op exporter 校验 trace/span、parent、低基数属性、source cursor/event、采样和容量，提供 JSONL、flush/shutdown/reopen；invalid parent、sampled=false、exporter closed/capacity overflow 不影响 EventLog/Receipt/授权；新增远端夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-15` AuditProjection checkpoint/rebuild：新增有界 `AuditProjectionSnapshot`/`AuditProjectionCheckpoint` schema 与 core `rebuild_audit_projection`/`AuditProjection`；rebuild/append/restore 绑定 source cursor/event、record IDs、records/checkpoint/projection digest，unknown audit schema、decision conflict、duplicate source、cursor gap/regression 和坏 checkpoint 均 fail-closed，不删除或覆写 EventLog 原事实；新增远端重建/增量/恢复/serde 夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-16` Audit query command/wire DTO：新增 `kiana-protocol` `AuditQueryRequest`/`AuditQueryResponse` 与 `RequestBody::AuditQuery`，`kiana-client` 复用 `KianaClient::audit_query`，DaemonHost 认证 actor、bounded limit/cursor 后调用 ControlPlane；core 只读重建 OA-15 projection，按 server-derived actor/session/project/approval lineage 过滤并返回 schema/source cursor/projection version/limitations，不暴露 raw EventLog/owner scope 字段；新增远端 wire deny/round-trip/raw-field 夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-17` query cursor/snapshot/paging：新增 `kiana.audit-query-cursor.v1`、`AuditQueryCursor` 与 filter digest/epoch/projection-version 绑定；AuditQueryRequest/Response 支持 cursor，ControlPlane 校验 source/after bounds、当前 projection epoch/filter digest、返回 bounded next cursor，stale/ahead/mismatched cursor fail-closed；新增远端 cursor serde/boundary 夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-18` audit export/manifest/delivery：新增 `kiana.audit-export.v1`/`kiana.audit-delivery-receipt.v1`、受限 `AuditExportManifest`/`AuditDeliveryReceipt`，protocol/client/DaemonHost 复用 query 路径；core 只导出已验证 AuditRecord 的 redacted JSONL/JSON/CSV，manifest 绑定 query/source/projection/artifact hashes，缺 purpose/recipient/retention、Safe、invalid scope/secret/oversize 均拒绝，delivery 无 server confirmation 保持 Unknown；新增远端 domain/wire 夹具与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-19` observability alert/incident/recovery association：新增有界 `ObservabilityAlert`/`ObservabilityIncident`/`ObservabilityIncidentSnapshot` 与 schema registry；core 从 OA-10 metrics 和 committed redaction/queue/journal/audit/failure facts 生成 stable fingerprint 去重告警/事故，绑定 source cursor/events 与固定 recovery plan；projector lag、audit/artifact loss、redaction、queue overflow、journal corruption、effect unknown、orphan dispatch 均可定位，unknown/reconciliation-required 不可直接 verified/closed，模型/UI 自报不触发；新增远端 CI 夹具；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-20` data governance/retention/deletion propagation：增强 `DataPolicy` schema/version/revision/data_epoch/digest 与 Purpose/Retention/Grant 校验，新增 `DataGovernanceSnapshot`/`DataRetentionObservation`；core 根据 committed data revocation/governance facts 计算 payload Available/Expired/Revoked/Unknown 与 audit metadata 分离，并统一传播到 receipt/audit/artifact/memory/index/cache/export；pending revocation fence 为 Unknown，daemon policy 读取做 integrity 校验并输出 data_epoch/propagation；新增远端 domain/core fixtures 与 CI job；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-21` replay/reconciliation diagnostics：新增有界 `ReplayDiagnostic`/`ReplayDiagnosticSnapshot` schema 与 core `diagnose_replay`/`ControlPlane::replay_diagnostics`，复用 committed Invocation/Run/Metric/Audit/Health/Span 投影，输出 source cursor/event、projection digest 和首个 divergence 的 invocation/attempt/input digest/status/error code；duplicate/gap/schema/terminal/projection mismatch、effect unknown 保持 Unknown，model/UI self-report 不触发；新增远端 CI fixtures；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-22` crash/fault injection matrix：新增 deterministic `FaultCase`/`FaultMatrix`（`kiana.fault-case.v1`/`kiana.fault-matrix.v1`）与 core replay-only simulator，覆盖 prepare/commit/dispatch/result/flush/projector/export/shutdown；固定 seed/source refs、rejected/unknown/observed、effect/fencing、duplicate/false-success 不变量，unknown/rejected fail-closed；新增远端 CI fixture；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-23` provider-independent eval suite：新增 `kiana.eval-case.v1`/`eval-result.v1`/`eval-suite.v1` 与 core `evaluate_provider_independent`，复用 committed normalized events、Audit/Metric/Span/Run receipt/Replay projection，要求证据缺失、secret/forbidden effect、status/replay divergence、measured cost/latency mismatch 均 Fail/Blocked，只有全 Pass 才 promote；新增远端 success/secret/forbidden/replay CI fixture；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-24` 四入口审计/健康/Receipt parity：新增 `kiana.entrypoint-parity.v1`，ControlPlane 从 owner-scoped committed EventLog 统一派生 source cursor、status、Receipt/Audit/Health digests、retention/unknown limitations；CLI `parity`、Workbench `/parity`、Web `/api/parity` 与 Desktop Web 壳复用同一 protocol/DaemonHost，健康端点改为真实 liveness projection；新增远端 core/protocol parity fixtures；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-25` 容量/性能/迁移演练：新增 `PerformanceBaseline`、六类 benchmark percentile、硬容量与安全护栏、rotation/archive/upgrade/downgrade `MigrationObservation`；core 提供 checked percentile/baseline reducer，远端 fixture 验证 journal writer unknown version、oversize/high-cardinality/backpressure guards 与 bounded summaries；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-26` Local durable observability gate：新增 CI-only durable gate 脚本/workflow，GitHub runner 验证 JSONL close/reopen、OA-24 parity/Unknown、critical queue preservation、source/release/artifact hash 与 secret scan；非 CI 调用 fail-closed；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-27` Cross-entry/company governance gate：新增 `kiana.company-governance.v1` 与 CompanyOS runtime→review→acceptance→delivery→close 只读投影；Completed 不自动变 Outcome，Closed 强制独立 Review、Acceptance、Confirmed Delivery、Closer、runtime evidence 和 ClosingReceipt；CLI/Workbench/Web/Desktop 通过同一 protocol/DaemonHost，远端 domain/core/protocol fixtures 覆盖越权与链缺失；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-15 | `OA-28` Physical/live handoff：新增 `kiana.live-handoff.v1`/`LiveHandoffManifest` 与 provider/connector/OTLP/OS target matrix；CI-only preflight 要求显式 opt-in、隔离环境、非秘密 `secret-ref`、operator approval、独立 receipt/reconcile、retention/cleanup，默认 fail-closed；新增远端 manifest/secret/unknown fixture 与 runbook；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `AUT-01` automation baseline：新增 `docs/roadmap/automation-baseline.md` 与 GitHub Actions source-only guard；盘点 `plan_command`/ControlPlane/DaemonHost 唯一路径、旧 `watch_scheduled_tasks` 兼容边界、缺失 scheduler/ClockPort/queue/claim/recovery，并冻结 AUT-02..24 fixture catalog；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `NM-00` notifications/messaging baseline：新增 `docs/roadmap/notifications-baseline.md` 与 GitHub Actions source-only guard；盘点 committed EventLog→HumanInbox/RunStream/SSE/transcript 的 owner、recipient、channel 与事实边界，确认无 durable NotificationStore/read-state/outbox/DeliveryWorker/外部通道，保留旧 watcher 兼容隔离并冻结 NM-01..22 fixture catalog；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `EQ-00` evaluation/quality baseline：新增 `docs/roadmap/evaluation-baseline.md` 与 GitHub Actions source-only guard；盘点旧 `EvalCommand` caller-fixture/report 面、OA-23 committed-fact core reducer、CLI/release smoke 边界，确认现状为 partial/source，冻结 Quality DTO/EvalStore/TraceNormalizer/isolated runner/finding/gate/CI fixture catalog 与迁移规则；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `PD-00` persistence/data-layer baseline：新增 `docs/roadmap/persistence-data-layer-baseline.md` 与 GitHub Actions source-only guard；盘点 EventStore/JSONL/Memory/Approval/Artifact/Memory/Query/Receipt/Projection 分层、cursor/capability/limits/owner scope，记录逻辑 StorageRoot/backup/migration/retention 目标与不把 Memory/cache/CI 写成 durable 的护栏；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `INT-00` integrations/connectors baseline：新增 `docs/roadmap/integrations-baseline.md` 与 GitHub Actions source-only guard；固定 Provider/Connector/MCP/A2A/Notification 术语、local_fixture/stdio 现状、server-owned binding/scope/idempotency/ProviderReceipt/reconcile 与 external/live 缺口，冻结 INT-01..33 fixture catalog 和迁移护栏；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CP-01` 服务端主体与项目身份：新增 typed `AuthenticatedPrincipalRef`/`ProjectIdentity`/`SessionAssignment` 合同与 schema registry；DaemonHost 服务端派生 local principal、canonical project identity/device-inode/trust digest，effectful 请求先解析 project identity，session assignment 继续 CAS 并重建校验；wire actor/role/trust 不可扩权；新增远端 identity/metadata guard；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CP-02` Run/Turn/Invocation/Execution：新增 typed `TurnIdentity`/`InvocationIdentity` 与 schema registry；`run.turn.v2` 显式创建新 Turn/Run 并记录 predecessor，legacy Continue 保留 `LegacyContinue`，Resume 继续复用同一 Run；请求事实记录 server-derived turn/invocation/attempt，Broker execution permit 阶段绑定真实 ExecutionId，call_id 只作关联值；新增远端 domain/core fixtures、CI workflow 与执行身份基线；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `SC-00` 安全与合规现状基线：新增 `security-compliance-baseline.md` 与 CI-only source guard；固定安全资产/入口/信任边界、SEC-01..12 当前 feature/proof 双维度、T01..T12 威胁和 SC-01..43 交接矩阵，明确 EventLog/ControlPlane/Broker 事实链与所有 partial/target/not_supported 限制；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P0-G-04` 事件重建投影与重启恢复收口：现有 Run/Invocation event projection、lazy cache invalidation、pending approval recovery/recheck 和 terminal conflict 绑定 EventLog；新增独立 source guard 与 CI-only focused workflow，覆盖新进程 Run/Invocation、pending approval gate recheck 和矛盾终态 fail-closed；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CP-03` action catalog/PreparedAction：新增 action-catalog schema registry 与 `validate_action_catalog`，固定每个 operation 的 capability/risk/schema/resource/effect/cancel/reconcile/idempotency/binding 元数据；prepare 阶段 bounded JSON/schema、identity/path/sandbox/defaults/hook 后归一化并冻结 catalog/action digest，Broker permit 继续核对同一 digest；新增 ReadOnly 风险伪造、duplicate key/numeric、catalog drift domain fixtures、core source guard 与 CI-only workflow；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CP-04` ScopeSet/单调授权：新增 `kiana.scope-set.v1` 与 NotApplicable/Restricted、path-aware intersection、budget/depth min、subset/digest/unknown-field guards；ControlPlane prepare 求 action/context path scope 交集并拒绝空集，DefaultPolicy hard deny 与 Gate/Hook Deny/Ask 合并保持单调，Cell parent Grant 不得被子级扩张；新增 domain/core remote fixtures、CI workflow 与 scope baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CP-05` 三条 capability 入口一致性：确认 direct、Harness tool、approval continuation 均沿 `prepare → authorize → stage/dispatch → finalize`，审批继续始终用当前 decision context 重验，统一 result mismatch/cancel/Unknown/permit/EventLog 边界；新增 source guard、跨路径远程 fixtures、CI workflow 与入口基线；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CP-06` 原子 TransitionBatch/command 幂等：注册 journal header/frame schema，统一 EventStore CAS/read-set、command digest、Committed/Replayed/Conflict/Unknown 和 all-or-none 语义；Memory/JSONL 复用同一 planner，observer 只接受 fresh commit；新增 stale conflict、same-command payload conflict、stale allow recompute eventlog fixtures、core guard、CI workflow 与事务基线；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `ER-01` 事件 schema/kind registry：新增 `EventKindSpec` machine-readable registry、required IDs/allowed fields/aggregate/terminal/secret/migration metadata 和 `kiana.runtime-event.v1`，unknown required kind/schema downgrade/payload unknown field fail-closed，opaque unknown 只读保留；旧 RuntimeEvent/JSONL decode 兼容，新增 domain/core remote fixtures、CI workflow 与 schema baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `ER-02` 统一身份/关联/顺序：RuntimeEvent 新增可选 command/correlation/causation/parent links 并默认 correlation=request_id，CorrelationContext/AttemptRef/CausationRef 与 EventStore/projectors 继续按稳定 ID/CAS 关联；event_id 重用、command digest 漂移、跨 run sequence 错配和 self-link fail-closed，legacy links/metadata 缺失保持 query-only；新增 domain/eventlog/core fixtures、CI workflow 与 identity baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `ER-03` 事件边界脱敏/Artifact：EventStore append/terminal 统一 recursive redaction、payload depth/size/NUL/secret stability checks，记录 redaction profile、non-resumable、data epoch 与 protected artifact refs；domain RedactionProfile/StreamingRedactor、Receipt/Artifact boundary 与 legacy compatibility 明确，新增远程 secret/oversize/deep/non-resumable fixtures、source guard、CI workflow 和 redaction baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `ER-04` CommandReceipt/read-set：新增 `CommandReceipt::validate_against` 与 schema registry，固化 batch command/digest、cursor/event IDs、expected aggregate versions、Committed/Replayed/Conflict/Unknown 和 `read_command` confirmation；缺 read-set、stale CAS、same-command payload drift、Unknown 均 fail-closed 且不追加/不派发；新增远程 eventlog fixtures、core guard、CI workflow 与 receipt baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CAP-01` capability authority/binding：以 `ACTION_OPERATIONS`/descriptor/PreparedAction 单一目录校验每个 operation 的 schema/risk/resource/effect/cancel/reconcile/idempotency/binding；DaemonHost 装配后 Broker catalog seal，duplicate/alias/kind/version/unknown fallback fail-closed；Runner 五工具与 operator-only 分离；新增 domain/Broker/core remote fixtures、CI workflow 与 authority baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CAP-02` input/schema/digest：统一 bounded JSON/schema/argv/patch/MCP/Memory 输入归一化、reserved authority fields、alias/NUL/path/numeric/timeout 限制，新增 `canonical_action_input_digest` 与 PreparedAction digest 校验；等价 JSON 摘要一致、执行影响字段必变，输入失败不进 Broker；新增远程 domain/core fixtures、source guard、CI workflow 与 input baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CAP-03` ExecutionScope：新增 typed `kiana.execution-scope.v1`，ControlPlane 从 server context/ScopeSet/action 派生 immutable scope 并写入 CapabilityRequest，Broker handler 前校验 scope/digest/resource/run/turn；direct command 不虚构 Harness，empty scope、caller scope、cross project/session/Cell、epoch/deadline/fence drift fail-closed；新增 domain/core fixtures、CI workflow 与 scope baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CAP-04` typed state/outcome：`CapabilityExecutionState` 集中 queued/阶段取消、启动失败与恢复 Unknown 的事件转移；新增 `CapabilityResultDimensions` process/stop/effect/exit/failure code，归一化器把非零 exit 保留为结构化 `execution_failed`，Protocol/CLI/HTTP/retry/reconcile 共享错误码；Invocation/attempt projection 拒绝终态复活、Unknown→Cancelled 和 foreign attempt，事件控制逻辑移除 result_unknown 文本猜测；新增 domain/core fixtures、source guard、CI workflow 与 state baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `H02` Harness identity/lifecycle：新增 `StepId`/`ModelAttemptId`、`StepIdentity`/`ModelAttemptIdentity` 与 ID/schema registry；Start wire、ControlPlane native path、Daemon skill adapter、Harness ActiveRun/checkpoint 和 `run.model_turn` facts 绑定 server Turn/Step/attempt，ModelAttemptRecord 保留 typed identity；旧 Start/legacy Continue reader 兼容，closed-run/foreign-late-result/identity tamper 夹具加入 GitHub CI；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `H03` Harness state driver：新增无 I/O `RunDriver`/`RunFrame`/`TurnFrame` 与 bounded mailbox/driver ownership/phase intents；`model_step` 从递归改为显式迭代，ActiveRun/checkpoint/step/tool/result/cancel/recovery 边界沿同一 reducer，未知 effect 停在 RecoveryRequired；新增 runner 纯状态 fixtures、CI workflow 与 baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `H04` Model content/provider boundary：`ModelMessage`/`ModelOutput` 保留 legacy text/tool_calls cassette，新增 typed `ModelContent` 与 reference-only `ProviderContinuation`；history 校验绑定 tool result，Provider compiler 在 wire 前按 route/provider/protocol/digest 拒绝跨 provider opaque、unsupported attachment/continuation，不携带 raw bytes；新增 domain/provider fixtures、source guard、CI workflow 与 baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `H05` Stop/error/retry classification：新增 `ModelStopReason`/`ModelOutcome` 与 `ModelError.side_effect_state`，legacy 缺 stop 补 end_turn/tool_use，Harness 完成/工具派发前拒绝 length/refusal/pause/incomplete/unknown，Provider parser/`run.model_turn` 保留 typed stop/retry/phase 及安全 detail；新增 domain/runner fixtures、CI workflow 与 baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P4-J7-05` 严格非流式工具响应：kiana-services OpenAI-compatible/Ollama response/request 解析要求 exactly-one/terminal/原生 ID/name/JSON object/duplicate ID，保留 Ollama 无原生 ID 的 ordinal 特例；daemon legacy adapter 去除默认 shell/tool/{} 修复并拒绝 malformed/missing/duplicate/empty-object/tool-result identity；新增远程 lib fixtures、CI workflow 与 provider baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CM-01` Context/Memory source-scope：新增 server-owned `SourceRef`/`SourceSnapshot`/`MemoryScope`/`Freshness`/`EvidenceStatus` 与 schema registry，绑定 principal/project/session/collection/purpose、digest/cursor/freshness，缺 identity/篡改/unknown field fail-closed；新增 domain source fixture、CI workflow 与 context-scope baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CM-02` MemoryRecord 生命周期/legacy import：补 kind/purpose/sensitivity/validity/retention/dependencies/import_mode 与 admission/review/state/provenance 组合校验；daemon v1 JSONL 显式转 unverifiable Unknown/Candidate/Draft legacy import，不默认 searchable/verified；native writer/review 补元数据，新增 domain/daemon fixtures、CI workflow 与 memory baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CM-03` Memory scope：`MemoryScope::intersect` 按 server principal/project/session/purpose 求 collection 交集与 allow_write AND；ControlPlane 将 role-granted memory collections 写入 ExecutionScope，daemon 从该 scope 重建并校验显式 collection/write，模型文本不授予权限；新增 domain/core scope fixtures、CI workflow 与 memory-scope baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CM-04` Memory mutation：新增七种 operation 的 server-owned `MemoryMutation`/receipt/纯 CAS ledger，绑定 actor/scope/evidence/policy-data epoch/payload digest/idempotency key；全部 targets 预检，重复 key 返回原 receipt，stale revision 与 payload 漂移拒绝；daemon write/review 在 JSONL append 前接线，新增 domain ledger fixtures、daemon source guard、CI workflow 与 mutation baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `EXT-01` 扩展合同：新增稳定 Extension/Component/Snapshot/HookRun IDs、Skill/Hook/Plugin/Snapshot/ExtensionError versioned DTO，digest/generation/revision/重复 identity/unknown-field fail-closed 校验与 protocol re-export；新增 domain/protocol fixtures、CI workflow 与 extension baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `EXT-02` SourceResolver：统一标准/显式 extension roots 与 precedence，先 canonicalize 实际 root 再应用 ProjectTrust；重复 root、绝对/`..`/symlink/越界 resource、未信任项目源 fail-closed，summary 使用 digest-derived source identity 且不泄露绝对路径；loader 复用 resolver，新增 resolver fixtures、CI workflow 与 source baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `EXT-03` 严格 parser：Skill frontmatter deny-unknown-fields/类型/大小/name 规范化，Plugin/Hook manifest bounded duplicate-key JSON、strict schema、唯一 ID/entry/phase 校验与显式 legacy adapter；新增 parser fixtures、CI workflow 与 strict parser baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `EXT-04` Catalog：domain `ExtensionCatalog` 按 kind/namespace/name/version/precedence/source/hash deterministic select winner，并保留 shadowed reason；Hook matcher 按 precedence/specificity/tie-break 稳定排序，重复 id 拒绝；legacy Command adapter 接入 catalog；新增 domain/skills fixtures、CI workflow 与 catalog baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `EXT-05` Snapshot：`SnapshotCacheKey` 绑定 cwd/trust/source-roots/package-generation/config/schema，cache entry 不覆盖、显式 invalidate 保留诊断并递增 generation；resolver root fingerprint 感知内容变化，dynamic/clear 触发全局 generation，skill registry 使用 key+generation；新增 cache/resolver fixtures、CI workflow 与 snapshot baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `UI-01` versioned UI protocol：新增 UiSnapshotV1/feed/action/result/capability/error/session/run/action-card/receipt/artifact/notice/limitation DTO 与 cursor/retry/disposition enums，校验 epoch/sequence/revision/digest/size/duplicate/unknown-field，保留旧 UiSnapshot/UiAction/RunStream wire；新增 protocol fixtures、CI workflow 与 UI baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `UI-02` handshake：新增 UiHandshakeRequest/Response、UiHealth、UiSurface、principal×surface capability intersection；CapabilityErrorCode/ExecutionStatus 映射到固定文案与安全 retry disposition，Unknown 只 QueryOriginal；kiana-client typed initialize/health 传输前后校验；新增 protocol/client fixtures、CI workflow 与 handshake baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `UI-03` 本地实例：新增 UiTransportKind/UiInstanceRecord 和 daemon InstanceLease/discover/peer 校验，workspace `.kiana/instances` create-new lock、0600 ready record、protocol/workspace/epoch/digest/duplicate/symlink fail-closed；DaemonHost 暴露 acquire_instance；新增 instance fixtures、CI workflow 与 baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CO-02` Company scope：新增 WorkspaceId、Organization/Workspace/Project bindings、CompanyScopeRegistry 与 legacy stream 显式导入/歧义拒绝；同 workspace 多项目 scope 隔离，canonical root/foreign project/组织 membership fail-closed；core Company root guard 与 domain fixtures/CI/baseline 接入；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CO-03` Server-owned assignments：新增 RoleAssignment/ProjectAssignment/ResolvedAssignment、有效窗口、revision/authority epoch、撤销级联和 AssignmentDirectoryPort；daemon 仅用 authenticated principal 与 root-derived project identity 构造 assignment context，core Company guard 在边界重验证 actor/role/department/expiry/epoch；domain/core fixtures、CI workflow 与 assignment baseline 接入；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CO-04` Versioned role catalog：五部门目录扩展为九岗位，新增 Analyst/QA/Librarian role packs；RoleSpec/DepartmentSpec、Role/Department Catalog、PromptBundle/ModelAssignment 与 run/session/receipt provenance 固定 schema/version、prompt hash、input/output schema、model profile；未知角色/工具/profile/metadata drift fail-closed，既有 Harness/ProviderGateway 路由保持单一主路径；新增 domain/core fixtures、CI workflow 与 role catalog baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CO-05` Company policy：新增 CompanyCommandPolicy、DecisionPurpose/ActorKind、HumanTask/HumanDecision；所有现有命令有显式角色/actor/purpose 矩阵，未知变体空集合拒绝；ControlPlane 在 Company admission 前拒绝 agent/SponsorProxy/Builder 自批，并将 decider、purpose、option、target revision/digest、scope、expiry/epoch 写入同一 CompanyProof；复用现有 ApprovalStore/Harness/EventLog，不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CO-06` Immutable evidence：新增 ArtifactVersion/ArtifactRef/EvidenceRef/Criterion 与 stable Artifact/Evidence/Criterion IDs、scope/provenance/content hash；ArtifactContentPort 只按版本读取并拒绝 missing/hash drift，core confined artifact path 生成 typed CompanyProof，CriteriaSnapshot/CompanyArtifact 保留兼容 typed refs；新增 domain/ports/core fixtures、CI workflow 与 artifact baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CO-07` Company receipts：新增 CompanyCommandReceipt/DispatchIntent，稳定 logical command ID、payload/authority digest、revision/event/replay/unknown 状态；Company core 在同一 EventStore CAS 事实前后返回 typed receipt，StartRun/受限 effect 写入 prepared intent，重复 key 原 receipt、不重复 dispatch；新增 domain/core fixtures、CI workflow 与 receipts baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CO-08` Company replay：新增 CompanyReplayReducer 与唯一 v0→v1 schema adapter；core load_company 校验 aggregate/root/owner/kind/idempotency、stream_version gap/regression、expected revision、重复命令、unknown major 和纯 CompanyState transition，历史重放结果稳定且不执行副作用；新增 domain/core fixtures、CI workflow 与 replay baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P0-A-01b` Schema registry gate：复核并专用 CI-wiring 现有 SCHEMA_CONTRACTS/SchemaLayer/SchemaVersion 与 EVENT_KIND_SPECS/event_migration；wire additive minor、domain/runtime unknown field、unknown schema/major/required event 和未登记 migration 均 fail-closed；新增 domain/core fixtures 与 schema baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P0-A-02` Stable errors：复核并专用 CI-wiring 现有 CapabilityErrorCode/CapabilityErrorPolicy、CapabilityResult::failure_code 与 protocol/CLI/HTTP mapping；path_escape 固定 403/exit3，新授权，ResultUnknown 固定 reconciliation/不可 retry，unknown reason 保守归类；新增 domain/core fixtures 与 error-codes baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P4-J7-06` Provider-neutral model contract：复核并 CI-wiring typed ModelContent/ProviderContinuation、PreparedModelCall/ModelFinish/Outcome/Error/Usage 与唯一 ports ModelClient；legacy 与 typed content 冲突、unknown major/cross-provider unsupported fail-closed，Runner 仅 re-export；新增 domain/core fixtures 与 provider-model baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P4-J7-07` Provider extraction：确认独立 kiana-provider crate 的 Gateway/codec/transport 不依赖 services/core/entrypoints/runner，daemon 生产路径唯一注入 Gateway，legacy kiana-services provider 仅 cfg(test)；新增 extraction source guard、CI workflow 与 provider-extraction baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P4-J7-08` Provider config snapshot：新增 ProviderProfileSnapshot/ProviderConfigSnapshot 和 Gateway secret-free configuration digest；profile/source/route revision 固定，unknown profile/inherit/key/capability/concurrency/streaming 拒绝，daemon explicit live/cassette mode conflict/missing cassette fail-closed；新增 provider/core fixtures、CI workflow 与 config baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P4-J7-09` Provider credentials/endpoints：复核并 CI-wiring credential reference/digest、HeaderValue/TLS/loopback/userinfo/query/fragment/redirect/proxy/concurrency/streaming guards；missing/invalid secret 和未授权公网 HTTP fail-closed，项目文本不能覆盖 endpoint；新增 provider/core fixtures 与 credentials baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P4-J7-10` Model catalog：新增 typed ModelCatalog/Entry，绑定 source/expiry/revision、Supported/Unsupported/Unknown capability 与 deterministic digest；Gateway 投影 configured entries，same-name model 要求 connection、slash ID 保持，discovery/list 不授予 tools 或改变 active route；新增 provider/core fixtures 与 catalog baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P0-B-01` State transition matrix：复核 domain Approval/Capability/RunCancellation/WorkPacket/Execution/Company 状态图与 terminal predicates，新增 illegal/terminal/result_unknown fixtures 和 core/runner source guard；不改变既有状态语义，CI-only 验证，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P0-K1-01` Server identity/authority epoch：DaemonHost 覆盖 wire actor，按 ProjectTrust 与 canonical root/device/inode 派生 ProjectIdentity；SessionAssignment 经 CAS 固化角色/部门，authority stream 单调版本写入 assignment 与 `run.authorized` epoch；新增 ingress fixture、core source guard、CI workflow 与 identity-authority baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-C-01` Organization/Cell contracts：统一 AgentTemplate、CellSpec、SpawnPlan、BudgetLease、CapabilityGrant、SupervisionLease 的 domain 合同，补 unknown-field fence、模板版本绑定、默认不可委派和 parent grant 子集校验；新增 domain fixture、core source guard、CI workflow 与 cell-contract baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-C-03` Role model routing：固定五部门九岗位 `RoleSpec.model_profile`，ControlPlane 将 server-owned profile 写入 `ModelAssignment`，ProviderGateway 只按 assignment 路由 planning/executing/quality 到不同 configured models；新增独立 provider fixture、core source guard、CI workflow 与 role-model-routing baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-D-01` Single WorkPacket readiness：确认 domain `ready_packets(graph, now)` 是唯一状态/依赖/deadline/claim 谓词，Company core/state 直接消费，legacy `kiana-tasks` 只委托该实现；新增跨 crate fixture、core source guard、CI workflow 与 ready-predicate baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-D-02` Dependency graph：`validate_dependency_dag` 拒绝缺失/重复边并输出确定性规范化 cycle；CompanyState `ApprovePacket` 追加前验证候选项目图；新增 domain/core fixtures、CI workflow 与 dependency-graph baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-D-03` Claim/lease recovery：PacketClaim owner/heartbeat/expiry 续租与过期扫描通过 CompanyCommand 回收，已 dispatch 的过期 claim 要求 terminal observation，Unknown 不自动重试；新增 domain/core fixtures、CI workflow 与 lease-recovery baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-E-01` Communication/accountability：新增七类 typed CommunicationMessage、Chat 无 authority/action/ACK、Handoff 定向 ACK、CommunicationPort 与 ControlPlane formal event 命令；新增 domain/core fixtures、CI workflow 与 communication baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-H-01` Tool authority：新增 domain `ToolSpec`/`TOOL_SPECS` 五工具单一 registry，`model_tool_name`/Runner canonical mapping 和 DaemonHost composition validation 统一消费，注册 schema/alias drift guard；新增 domain/core fixtures、CI workflow 与 tool-authority baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P1-H-03` Shared path containment：新增 domain lexical path/root containment helper，迁移 patch/shell/package/checkpoint/Cell/event scope 边界，保留 filesystem no-follow/TOCTOU 二次校验；新增 domain/core fixtures、CI workflow 与 path-containment baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `P3-I-01` Company object contracts：冻结十类 Company domain 对象/状态与 validate 不变量，补 Acceptance/CriteriaSnapshot/CompanyReview/MetricObservation strict fields/criteria/measurement checks；新增 domain/core fixtures、CI workflow 与 company-object baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CI-02` Identity contracts：新增 Principal/Membership/SecretRef/ProviderAccount/ServiceIdentity/ConfigSnapshot/AuthoritySnapshot 与稳定 ID，strict/digest/expiry/epoch/raw-secret 校验并协议导出；新增 domain/core fixtures、CI workflow 与 identity-contracts baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CI-03` Ports layering：新增 IdentityResolver、CredentialResolver、ConfigSnapshotStore、CredentialRotationPort/RotationRevokePort 与 secret-free CredentialResolution，明确 generation CAS 和 adapter fail-closed 边界；新增 ports/core fixtures、CI workflow 与 ports-identity baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CI-04` Protected daemon ingress：RequestMetadata 支持可选 loopback instance/origin/host/SecretRef metadata，DaemonHost 先行校验并保持 server principal/ProjectTrust/session ownership；新增显式 IdentityMigration 兼容事实、domain/protocol/daemon/core fixtures、CI workflow 与 daemon-ingress baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `CI-05` Authority ledger：新增 PolicyProfile/DataBoundary/SharingGrant typed contracts 与 stable IDs，AuthorityLedger 从 authority EventStore facts deterministic rebuild，拒绝 unknown/version gap/epoch rollback/stale，sharing scope 取当前 epoch 交集；core epoch 读取复用 reducer，新增 domain/core fixtures、CI workflow 与 authority-ledger baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `SW-01` Swarm lineage：新增七类 typed Swarm IDs 与 strict `SwarmLineage`，绑定 parent/root/workflow/correlation/causation、authority epoch/revision，拒绝 unknown schema/field、nil ID、cross-swarm、self-parent、revision/epoch 回退，digest 使用 canonical JSON；新增 protocol/ports 导出、domain/core fixtures、CI workflow 与 swarm-lineage baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `SW-02` WorkGraph：新增 typed Partition/SwarmWorkGraph，复用 packet_graph 确定性拓扑，拒绝空输入、path/data scope overlap、cycle/missing/duplicate/fingerprint、`first_success`、count/depth/concurrency/spawn-rate/TTL/budget 超限；提供稳定 ready/blocked/failed projection，并在 SwarmPlan typed graph 存在时于 Create 前校验；新增 domain/core fixtures、CI workflow 与 swarm-work-graph baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-16 | `SW-03` 状态 reducer：新增 strict Swarm/Partition/Attempt typed transition event 与纯 reducer/replay，delegation kind、revision/epoch/correlation/causation/digest 进入事实；非法边、terminal reopen、Unknown→success、merge 缺 review、版本 gap/epoch 回退拒绝；legacy SwarmEvent 可选 transitions 由 core load 校验；新增 domain/core fixtures、CI workflow 与 swarm-reducer baseline；不运行本地测试，格式与 workspace 静态编译通过，CI 已触发但未等待 | 待本提交 |
| 2026-09-10 | 记忆架构设计 spec + J3-01/J3-02 实施计划入库；roadmap 新增 `P1-J3-03`/`P1-J3-04`/`P4-J3-05` | `0bb624e` + `28fe392` + `a1fb227` |
| 2026-09-12 | 按 `db77c24` 核对当前窗口：`05b` 已有提交但真实链路未证明；重开 `05a` 的 wall-time 回归和 `G-04` 未交付范围，补齐审批/记忆依赖；历史证据不删除 | 文档修订未提交；证据块「Roadmap source reconciliation evidence (2026-09-12)」；无新增 CI |
| 2026-09-13 | 追加配置、凭据与身份专项设计：三域事实模型、SecretRef/Lease、assignment/authority epoch、OAuth/工作负载身份、deny-first 验收与 CI-01..CI-12 实施批次 | 文档规划未提交；基于 reference 与当前源码调研；无源码状态变更 |
| 2026-09-13 | 追加多 Agent 协调 / Swarm 专项：有界 fan-out/fan-in、持久 DispatchIntent、fresh child、监督/取消/恢复、确定性 reduce、独立合并及 SW-00..SW-18 执行批次 | 文档规划未提交；基于 reference 全目录盘点、CompanyOS 规范、LangGraph/OpenAI Agents/Agent Framework/Kubernetes/MCP 一手资料；无源码状态变更 |
| 2026-09-13 | 追加可观测性与审计专项设计：EventLog/Receipt/Log/Metric/Trace/Audit/Health 边界、提交后投影、脱敏/Unknown/背压/恢复、OA-00..OA-28 实施批次与验收矩阵 | 文档规划未提交；基于 reference、CompanyOS 规范、EventLog 源码和 OpenTelemetry/W3C 公开规范调研；无源码状态变更 |
| 2026-09-13 | 追加调度、工作流与触发器专项：durable queue/claim、timer/event occurrence、纯 replay planner、effect reservation、cancel/retry/Unknown/recovery 与 AUT-01..AUT-24 实施批次 | 文档规划未提交；基于 reference 与当前源码调研；无源码状态变更 |
| 2026-09-14 | 追加持久化与数据层专项：事实/投影/Artifact/Memory/Index 分层、cursor/generation、备份/恢复、迁移、保留和 `PD-00`–`PD-35` 实施批次 | 文档规划未提交；独立设计见 `roadmap/persistence-data-layer.md`；无源码状态变更 |
| 2026-09-14 | 补全通知与消息专项：消息/通知/投递/实时流分层、ControlPlane 处理流、`NM-00`–`NM-22` 实施步骤和拒绝优先验收矩阵 | 文档规划未提交；无源码状态变更 |
| 2026-09-14 | 追加集成与连接器专项：Provider/Connector/MCP/A2A 边界、账号绑定、SecretRef/Lease、幂等/回执/Unknown 对账、Webhook 入站和 INT-00..INT-33 实施批次 | 文档规划未提交；基于 reference 全目录、审计材料和当前 connector WIP 调研；无源码状态变更 |
| 2026-09-14 | 追加评测与质量专项：EvalCase/GoldenTrace、隔离执行、finding/评分、baseline、quality gate、反馈/漂移和 `EQ-00`–`EQ-51` 实施批次 | 文档规划未提交；无源码状态变更 |
| 2026-09-14 | 追加部署、运维与迁移专项：部署 profile、ReleaseManifest、健康/ready/drain、单 writer lease/fence、备份/恢复、forward migration、回滚、滚动发布和 `DEP-00`–`DEP-41` 实施批次 | 文档规划未提交；基于 reference 全目录、Temporal/SQLite/Kubernetes/Flyway/12-factor 调研；无源码状态变更 |
| 2026-09-14 | 追加计费、配额与成本专项：UsageVector/RateCard/CostLedger、层级预算与 ProviderBudget、原子预留/结算、Unknown/对账、背压和 `BQ-00`–`BQ-30` 实施批次 | 文档规划未提交；基于 reference 全目录、Provider/ControlPlane/CompanyOS 规范和相关项目源码调研；无源码状态变更 |
| 2026-09-14 | 追加安全与合规专项：身份、授权、Secret、数据治理、供应链、审计、事故响应和 `SC-00`–`SC-43` 实施批次 | 文档规划未提交；独立设计见 `roadmap/security-compliance.md`；无源码状态变更 |
| 2026-09-14 | 重排模块地图与专项导航：补产品平面/事实源矩阵、稳定锚点、独立文件入口，并将追加章节标为 `34-A`/`34-B` | 文档规划未提交；无源码状态变更 |
| 2026-09-14 | 将 11 个新增专项的 346 张 Step 并入 §1.1 总队列；与原 403 张卡合并为 749 张，展开前置、补详细卡锚点并按 W0–W11 拓扑串行化 | 文档规划未提交；无源码状态变更 |

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

### P0-A-01b schema 注册表与 unknown field/migration 规则　✅

- **现状**：ID 契约已登记（`P0-A-01a`）；canonical domain/wire/runtime schema、unknown field/event 和 migration registry 已在现有 contracts/event registry 中实现并由本步专用 CI 夹具核对。
- **做什么**：维护单一 schema registry，区分 domain schema 与 wire schema；固定 unknown field / unknown event 处理规则与显式 migration 规则；当前 evidence 见 [`schema-registry-baseline.md`](roadmap/schema-registry-baseline.md)。
- **风险**：兼容字段增加可升 minor，破坏性变化必须升 major 并提供 upcaster/迁移；未知 major 必须 fail-closed。
- **验收**：`unknown_major_version_fails_closed`
- **依赖 / 边界**：依赖 `P0-A-01a`；兼容边界以 `company-os-spec-index.md` §6.2 为准。
- **依据**：`company-os-implementation-outline.md` §Slice A、`company-os-spec-index.md` §6.2







<a id="step-p0-a-02"></a>

### P0-A-02 稳定错误码枚举　✅

- **现状**：`CapabilityErrorCode`/`CapabilityErrorPolicy` 与 `CapabilityResult::failure_code()` 已在现有 domain/protocol 路径实现，本步补专用 CI-only acceptance/source guard。
- **做什么**：维护 append-only `CapabilityErrorCode` enum + `CapabilityResult::failure_code()`；每个码固定 CLI exit、HTTP status、是否可重试、是否需新授权或补偿；当前 evidence 见 [`error-codes-baseline.md`](roadmap/error-codes-baseline.md)。
- **风险**：错误码一旦进入 wire 就只能加新码，不能改语义。
- **验收**：`path_escape_uses_stable_error_code`
- **依赖 / 边界**：依赖 `P0-A-01a`；`result_unknown` 必须进对账队列，不能自动 retry。
- **依据**：`company-os-implementation-outline.md` §Slice B







<a id="step-p0-b-01"></a>

### P0-B-01 正式状态机转移表　✅

- **现状**：domain 已有 Approval/Capability/RunCancellation/WorkPacket/Execution/Company transition matrices，本步补专用 CI fixture/source guard 冻结消费边界。
- **做什么**：维护 domain 状态转移表，冻结终态不可回、expired approval 不得执行、`result_unknown` 不得自动变成功；当前 evidence 见 [`state-machine-baseline.md`](roadmap/state-machine-baseline.md)。
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

### P0-G-04 事件重建投影　✅

- **现状**：源码提交 `38f23bc` 已把 Invocation 折叠、惰性缓存、终态冲突拒绝和审批恢复接入 `kiana-core`；历史账本只证明 Run 只读投影，远程 CI 验收仍未回执。
- **做什么**：新增 RunProjection / InvocationProjection，用 `read_stream("run", run_id)` 与 `read_all` 折叠 `run.*`/`capability.*`/`approval.*`；首次按 run_id/session 访问时惰性重建。
- **风险**：折叠遇矛盾终态必须保持 `run_terminal_conflict`/`result_unknown` fail-closed，不能猜。
- **验收**：新增 `new_process_rebuilds_invocation_state_from_events_alone`、冲突终态 fixture、重启 pending approval 的 authorization re-check；由 `.github/workflows/p0-g04-projection.yml` 远程执行，结果按用户要求不等待。
- **依赖 / 边界**：依赖 `P0-G-01`；内存 map 降级为写穿缓存。
- **依据**：`company-os-implementation-outline.md` §Slice G（A-2）｜`38f23bc`；Run 子集历史证据 `3a319be` + CI `34500579350`；本次证据块「P0-G-04 projection and restart recovery closure evidence (2026-09-16)」。







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

### P0-J1-05a 重复调用检测与 wall-time 预算接线　✅

- **现状**：重复调用与 wall-time 有 `d973ff6` / `409cfc7` / `747ff8b` 历史证据；`dd6a8d5` 修复了 model-config 构造路径丢失 wall-time 的回归，并覆盖非法配置拒绝。
- **做什么**：复现并修复 model-config 路径的 wall-time 接线，保留其他构造路径行为；角色步数由 `05b` 单独验收。
- **风险**：非法 wall-time 值也会被该路径忽略；必须在请求模型前拒绝，不能只验证配置 helper。
- **验收**：保留 `run_wall_time_budget_fails_closed`；新增 `model_config_rejects_invalid_wall_time_budget`、`model_config_wall_time_budget_fails_closed`（见 §2）。
- **依赖 / 边界**：无依赖；不同参数不算重复调用，不得误伤。
- **依据**：`company-os-implementation-outline.md` §Slice J1｜`409cfc7` + `747ff8b` + 历史 CI `34389804309`；账本「Run-level wall-time budget evidence (2026-09-10)」及「Roadmap source reconciliation evidence (2026-09-12)」。







<a id="step-p0-j1-05b"></a>

### P0-J1-05b 按角色的 max_steps　✅

- **现状**：`bd9dea0` 已将角色快照经 ControlPlane 的 Start 命令传入 harness；每个 run 保存 `min(command, harness)` 上限，Continue 只重置该 run 的步数与时钟，不共享其他 run 的预算。
- **做什么**：保持角色上限、显式环境覆盖优先、无角色默认 32，以及构造上限与 Start 命令的交集语义。
- **风险**：角色/环境/Continue 的行为依赖模型调用计数与 Receipt；测试只在 GitHub CI 执行，本地仅做静态编译。
- **验收**：`role_max_steps_reaches_the_harness`、`start_command_max_steps_limits_that_run`、`environment_max_steps_overrides_role_in_product_run`、`role_step_limits_are_isolated_across_runs_and_continue`。
- **依赖 / 边界**：依赖 `P0-J1-05a`；不改 `RoleSpec` 现有字段语义。
- **依据**：`company-os-implementation-outline.md` §Slice J1｜`bd9dea0`；本次证据块「P0-J1-05b role max-steps product-path evidence (2026-09-14)」。







<a id="step-p0-j7-01"></a>

### P0-J7-01 流式基线收尾　✅

- **现状**：已落地——增量按轮次聚合落账（`e9df8b4`）、不完整流 fail-closed（`3b65af2`）、命令行默认流式 + `--no-stream`（`e2b15c1`/`d704add`）、断线发 `stream_gap`（`8b2aecb`）。
- **做什么**：保持现状；后续协议扩展见 `P4-J7-02`/`-03`。
- **风险**：无已知风险；回归由 `run_streams_each_delta_by_default_before_terminal_receipt` 覆盖。
- **验收**：`run_streams_each_delta_by_default_before_terminal_receipt`
- **依赖 / 边界**：无依赖；Web 仍不声称 token streaming 之外的能力。
- **依据**：`company-os-implementation-outline.md` §Slice J7｜证据：CI `34376675138`







<a id="step-p0-k1-01"></a>

### P0-K1-01 服务端身份与 authority epoch　✅

- **现状**：`DaemonHost` 已持有 server-owned principal；ProjectIdentity、SessionAssignment 和 authority stream 均在 effectful 入口完成服务端解析/绑定。
- **做什么**：由受保护入口解析身份，服务端从不可变 assignment 派生 role/department/authority epoch；本步接通 authority stream version 到 session/run fence。
- **风险**：wire 上的 actor/trust/profile 若被当作授权来源，就是越权入口。
- **验收**：`wire_actor_cannot_grant_role_or_department`
- **依赖 / 边界**：依赖 `P0-A-01a`；不改现有 ProjectTrust 派生逻辑的语义。企业/OAuth 身份与 durable assignment store 仍是后续 CI/CP/SC/PD 范围。
- **依据**：`company-os-implementation-outline.md` §Slice K1；证据见 [`identity-authority-baseline.md`](roadmap/identity-authority-baseline.md)







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

### P1-C-01 组织与 Cell 契约　✅

- **现状**：`kiana-domain` 已提供六类 versioned Cell/组织合同，`MemoryCellRegistry` 在 reserve/snapshot 阶段重复校验模板、grant、budget、supervision 与 parent scope。
- **做什么**：补齐六类 DTO 的 unknown-field fence，固定模板版本绑定、子 grant 只能收窄、默认不可再委派，并接入专用 CI 验收。
- **风险**：模板版本若不固定，历史 Cell 无法复现。
- **验收**：`child_grant_cannot_exceed_parent_grant`
- **依赖 / 边界**：依赖 `P0-A-01a`；优先在 `kiana-domain` 定义契约。Cell 状态持久化、调度和跨进程恢复留在 P1-C-02/SW/AUT/ER/PD。
- **依据**：`company-os-implementation-outline.md` §Slice C；证据见 [`cell-contract-baseline.md`](roadmap/cell-contract-baseline.md)







<a id="step-p1-c-02"></a>

### P1-C-02 Cell 生命周期与 retire　⏳

- **现状**：registry/budget 仍是进程内状态，尚无 durable Cell projector 和跨进程恢复。
- **做什么**：打通 reserve→commit→terminal→retire 全链；retire 撤销 grant、释放锁和预算。
- **风险**：spawn 预留预算、锁和 grant 必须具有原子语义，否则会出现预算泄漏。
- **验收**：`retire_revokes_grants_and_releases_budget`
- **依赖 / 边界**：依赖 `P1-C-01`；`kiana-core` 执行授权和生命周期，`kiana-daemon` 提供目录与调度。
- **依据**：`company-os-implementation-outline.md` §Slice C







<a id="step-p1-c-03"></a>

### P1-C-03 五部门角色目录与 model_profile 接线　✅

- **现状**：RoleCatalog/DepartmentCatalog 已固定五部门九岗位，ControlPlane 为每次 run 安装带 role/profile/revision 的 ModelAssignment，ProviderGateway 按 assignment profile 选择连接。
- **做什么**：补齐 P1 基础卡的独立 model-profile route 验收，固定 planning/executing/quality 的不同模型选择与 drift 拒绝边界。
- **风险**：角色目录若散落各 crate 会形成第二真相；「规划用强模型、执行用便宜模型」必须可在收据里复现。
- **验收**：`planning_and_execution_roles_can_use_different_models`
- **依赖 / 边界**：依赖 `P1-C-01`；不新增模型可见工具，Provider 不能成为身份或权限事实源。
- **依据**：`COMPANY.md` §3、§4；证据见 [`role-model-routing-baseline.md`](roadmap/role-model-routing-baseline.md)







<a id="step-p1-d-01"></a>

### P1-D-01 WorkPacket 单一 ready 谓词　✅

- **现状**：`kiana-domain::ready_packets` 已统一校验 packet identity、DAG、依赖状态、deadline 和 claim lease；Company core/state 直接调用，legacy `kiana-tasks` 仅提供委托 wrapper。
- **做什么**：补齐跨 crate 的单一 ready predicate 验收，禁止 legacy board 复制 WorkPacket readiness 逻辑。
- **风险**：三处各写一份会让「可派发」的定义漂移。
- **验收**：`single_ready_predicate_agrees_across_three_callers`
- **依赖 / 边界**：依赖 `P0-A-01a`；就绪 = 状态可派发 + 依赖全成功 + 无未过期 lease 冲突。DAG admission 与 lease reclaim 仍由 P1-D-02/03 负责。
- **依据**：`company-os-implementation-outline.md` §Slice D；证据见 [`ready-predicate-baseline.md`](roadmap/ready-predicate-baseline.md)







<a id="step-p1-d-02"></a>

### P1-D-02 依赖缺失 / 成环 fail-closed　✅

- **现状**：`WorkPacket.dependencies` 是显式字段，domain DAG helper 已拒绝缺失/重复边并规范化环；CompanyState 的 ApprovePacket 现在在追加候选前验证完整项目图。
- **做什么**：补批准前候选图校验与确定性缺失/成环 CI 夹具，确保结构错误不会写入事实账本。
- **风险**：从 packet 文本解析依赖会引入不确定性和注入面。
- **验收**：`dependency_cycle_is_rejected_deterministically`
- **依赖 / 边界**：依赖 `P1-D-01`；父 packet blocked 时子 packet 派生 blocked，自动 reclaim 留在 P1-D-03。
- **依据**：`company-os-implementation-outline.md` §Slice D；证据见 [`dependency-graph-baseline.md`](roadmap/dependency-graph-baseline.md)







<a id="step-p1-d-03"></a>

### P1-D-03 claim / lease 心跳回收　✅

- **现状**：PacketClaim 已绑定 owner/session/heartbeat/expiry；runtime guard 在 turn 边界续租，ControlPlane 提供有界过期扫描并通过 CompanyCommand 回收。
- **做什么**：补过期 claim 回收、owner/heartbeat 负向验收和 no-double-dispatch 证明，保持 ready 查询与执行权分离。
- **风险**：ready 只是查询，执行许可仍必须由 policy/gates/approval 产生。
- **验收**：`expired_lease_is_reclaimed_without_double_dispatch`
- **依赖 / 边界**：依赖 `P1-D-01`；worker 死亡/过期只允许在有 terminal observation 时回收 in-flight claim，Unknown 不自动 retry。
- **依据**：`company-os-implementation-outline.md` §Slice D；证据见 [`lease-recovery-baseline.md`](roadmap/lease-recovery-baseline.md)







<a id="step-p1-e-01"></a>

### P1-E-01 通信与问责分层　✅

- **现状**：domain 已提供七类 `CommunicationMessageKind` 与 bounded/digest 合同，现有 PacketHandoff 继续绑定目标 role/session/expiry ACK。
- **做什么**：将通信消息经 `CommunicationPort`/ControlPlane `communication.send` 记录为正式事件；Chat 禁止 action/ACK/authority 字段，sender 必须来自 server context。
- **风险**：自由聊天一旦产生授权，问责链就断了。
- **验收**：`free_chat_never_grants_authority`
- **依赖 / 边界**：依赖 `P0-B-01`；消息只产生 EventLog fact，任何业务/能力授权仍回到 ControlPlane policy/gate/approval。
- **依据**：`company-os-implementation-outline.md` §Slice E；证据见 [`communication-baseline.md`](roadmap/communication-baseline.md)







<a id="step-p1-e-02"></a>

### P1-E-02 Symposium 会议对象契约化　⏳

- **现状**：`Symposium`/投票/黑板/`DecisionRecord` 已实现（`kiana-domain/src/symposiums.rs`），`convene_symposium` 有 chair 必须为 PM、`can_convene` 与 workspace-write 校验（`kiana-core/src/collaboration.rs:955-1044`）——对应 `COMPANY.md` §5.4 的 v0.3/v0.4，但从未进验收追踪。
- **做什么**：给现有会议路径补验收测试（chair 校验、投票、决议产出）；会议决定事件 durable 可重放。
- **风险**：会议代码已存在却不在 §1 表里，回归不可见；决定事件若不 durable，重启后决议丢失。
- **验收**：`symposium_decision_is_durable_and_replayable`
- **依赖 / 边界**：依赖 `P1-E-01`；不改会议参会边界（Builder 不进规划/监控会，冻结项）。
- **依据**：`COMPANY.md` §5.1–5.4







<a id="step-p1-h-01"></a>

### P1-H-01 `ToolSpec` registry　✅

- **现状**：五个模型可见工具的 JSON schema 已在 domain catalog；本步新增 typed `ToolSpec` authority，Runner alias 归一化和 DaemonHost 组合启动均消费该 registry。
- **做什么**：固定 `ToolSpec{name, aliases, capability, operation, risk_policy, side_effecting, schema}`/`TOOL_SPECS`，拒绝 surface/schema/alias drift，不新增模型可见工具。
- **风险**：registry 若成为第二套 authority 而不被 policy 消费，就是摆设。
- **验收**：`tool_authority_covers_every_model_visible_tool`
- **依赖 / 边界**：依赖 `P0-A-01a`；**不新增模型可见工具**，保持 5 个。registry 只描述入口，policy/gate/approval/Broker 仍拥有执行授权。
- **依据**：`company-os-implementation-outline.md` §Slice H；证据见 [`tool-authority-baseline.md`](roadmap/tool-authority-baseline.md)







<a id="step-p1-h-02"></a>

### P1-H-02 参数 schema 校验　✅

- **现状**：`9095ea7` 已加 `validate_tool_arguments`，在映射到 capability 前按现有 schema 表校验。
- **做什么**：保持现状；校验覆盖 `type` / `required` / `minimum` / `enum` 子集。
- **风险**：`additionalProperties` 不拒绝（老 cassette 不被误拒）；`schema_name_for_tool` 与 `tools.rs:234` 的别名表重复，`P1-H-01` 必须收敛。
- **验收**：`malformed_arguments_are_rejected_before_capability_mapping`
- **依赖 / 边界**：无硬依赖（`P1-H-01` 落地后只需替换数据源）；不改变已有工具的接受集。
- **依据**：`company-os-implementation-outline.md` §Slice H｜`9095ea7` + CI `34380323510` + 证据块「Tool argument validation at capability mapping evidence (2026-09-10)」（`e144d30`）







<a id="step-p1-h-03"></a>

### P1-H-03 路径 containment 共享实现　✅

- **现状**：domain 已提供 shared `enforce_path_containment`/`enforce_root_containment`；patch/shell/package/checkpoint/Cell/event scope 边界均已迁移，MCP/Memory 继续使用 root/scope resolver。
- **做什么**：固定 lexical containment API 与拒绝 reason，确保所有副作用边界不再新增独立 allow-list/root 检查。
- **风险**：只对部分工具生效会留下绕过面（symlink / hardlink / rename）。
- **验收**：`path_containment_is_shared_by_every_side_effecting_tool`
- **依赖 / 边界**：依赖 `P1-H-01`；lexical helper 不替代 filesystem no-follow/symlink/hardlink/TOCTOU fencing。
- **依据**：`company-os-implementation-outline.md` §Slice H；证据见 [`path-containment-baseline.md`](roadmap/path-containment-baseline.md)







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

### P1-J3-01 Memory 写入候选制　✅

- **现状**：`memory.write` 已由 broker 生成 v2 记录；持久层 candidate 默认不满足 `searchable()`，旧 v1 记录缺字段时的兼容语义此前未显式恢复。
- **做什么**：保持模型写入的服务端 `origin=model`、持久层 `candidate + draft` 和 scratch 例外；补 v1 存量可检索但 provenance 不可验证的解析边界，并用现有 `memory.review` 审批链完成晋升。
- **风险**：模型不能自批或伪造 origin/admission/state；instance-scratch 层保持默认可见，持久层必须先审批。
- **验收**：`model_written_memory_stays_unsearchable_until_approved`
- **依赖 / 边界**：依赖 `P0-A-01a`；持久层 candidate 默认不可检索。
- **依据**：`company-os-implementation-outline.md` §Slice J3；源码提交 `758ffbe`；本次证据块「P1-J3-01 memory candidate admission evidence (2026-09-14)」。







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

### P3-I-01 Company 业务对象契约　✅

- **现状**：Company domain 已提供十类对象、显式状态宏和 CompanyState transition；本步补齐 Acceptance/Review/MetricObservation 严格字段与 criteria/measurement invariants。
- **做什么**：冻结十类业务对象的 domain DTO/validate 边界，保持 core 只能通过 CompanyState/ControlPlane 推进状态。
- **风险**：把业务对象实现成 prompt 里的名词，而不是 domain 类型。
- **验收**：`company_objects_expose_invariants`
- **依赖 / 边界**：依赖 `P0-A-01a`；字段与状态机以 `company-os-domain-contracts.md` 为准。命令/event wire freeze、durable replay 和业务闭环留在 P3-I-02+、CO/ER/PD。
- **依据**：`company-os-implementation-outline.md` §Slice I；证据见 [`company-object-baseline.md`](roadmap/company-object-baseline.md)







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

> §1.1 是 749 张 Step 的唯一执行队列；以下导航只帮助定位设计说明、代码归属和验收背景。专项章节/文件不另设执行顺序，状态仍以本页总图与 `CURRENT_STATUS.md` 的证据块为准。

| 专项 | 细化卡 | 位置 |
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
| 配置 / 凭据 / 身份 | CI-01–CI-12（12） | [§29](#config-credentials-identity-plan) |
| Swarm | SW-00–SW-18（19） | [§30](#swarm-coordination-design) |
| 可观测性 / 审计 | OA-00–OA-28（29） | [§31](#observability-audit-plan) |
| 调度 / Workflow / Trigger | AUT-01–AUT-24（24） | [§32](#scheduling-workflow-trigger-plan) |
| Persistence / Data Layer | PD-00–PD-35（36） | [独立设计](roadmap/persistence-data-layer.md) · [§33](#persistence-data-layer-plan) |
| Notifications / Messaging | NM-00–NM-22（23） | [通知与消息：事实投影、订阅、投递、Human Inbox 与实时桥](#notification-messaging-design) |
| Integrations / Connectors | INT-00–INT-33（34） | [独立设计](roadmap/integrations-connectors.md) · [§34-A](#integrations-connectors-plan) |
| Evaluation / Quality | EQ-00–EQ-51（52） | [§34-B](#quality-evaluation-design) |
| Billing / Quota / Cost | BQ-00–BQ-30（31） | [计费、配额与成本：预留、用量、价格、容量、对账与发布门](#billing-quota-cost-plan) |
| Security / Compliance | SC-00–SC-43（44） | [安全与合规：身份、授权、隔离、秘密、数据治理、供应链、审计与证明门](#security-compliance-plan) |
| Deployment / Operations / Migration | DEP-00–DEP-41（42） | [部署、运维与迁移：发布、健康、备份、恢复、迁移、回滚与运维工具](#deployment-operations-migration-design) |

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

<a id="config-credentials-identity-plan"></a>

## 29. 配置、凭据与身份：实际设计、处理流程与详细实施步骤（2026-09-13 追加）

> 本专项补全 [module-map.md](module-map.md) 的“配置、凭据与身份”模块。它是实现路线，不把规范目标或参考项目能力写成当前实现。当前源码仍以 `CURRENT_STATUS.md` 为准：Provider 配置、固定 `local-user`、ProjectTrust 和角色目录已有局部实现，但 durable principal、assignment、SecretRef、OAuth 生命周期和 secret 全链路隔离尚未完成。
>
> 参考调研范围包括仓库内 `reference/` 的 Codex workload identity、OpenCode provider/credential/policy、OpenHands credential probe、Strix OAuth/PKCE、Cline/Roo/Crush/Goose 等项目，以及本仓库的 CompanyOS 治理与安全宪法。参考项目只提供行为启发，不引入其源码、凭据格式或第二执行循环。

### 29.1 设计结论

配置、凭据和身份是三个相互关联但不能互相替代的事实域：

| 事实域 | 回答的问题 | 权威对象 | 不得承担的职责 |
|---|---|---|---|
| Configuration | 这次运行采用了哪些非秘密参数、来自哪里、版本是什么 | `ConfigSnapshot`（canonical bytes + `config_revision`） | 不保存 secret 原值；不决定用户是否有权执行 |
| Credential | 出站目标当前是否有可用的 API key/OAuth/工作负载令牌 | `CredentialBinding`、`SecretRef`、`CredentialLease` | 不代表 Kiana 用户、组织、项目或角色身份 |
| Identity / Authority | 谁发起、属于哪个组织/项目、拥有哪些有效 assignment | `Principal`、`Membership`、`RoleAssignment`、`ProjectAssignment`、`authority_epoch` | 不从 wire body、模型文本、provider account 或 API key 推断权限 |

必须保持以下关系：

```text
protected ingress
  → authenticate server/client boundary
  → resolve Principal + Organization/Membership
  → resolve ProjectTrust + ProjectAssignment + PolicyProfile
  → build immutable AuthoritySnapshot(authority_epoch)
  → resolve ConfigSnapshot(non-secret, trusted sources)
  → evaluate provider.use / capability policy
  → bind ProviderAccount + SecretRef revision
  → ControlPlane admission (grant/approval/budget)
  → one invocation CredentialLease (opaque, short TTL, one-shot)
  → Provider transport injects secret at the last possible boundary
  → append redacted facts and receipt metadata
```

配置正确、凭据存在、Provider 可用和动作获授权是四个独立判断；任一失败都必须在 Broker/Provider 产生副作用前 fail-closed。Provider account ID、OAuth subject、ChatGPT account header、API key possession 和 local bearer 都不是 Kiana `PrincipalId`。

### 29.1.1 参考项目模式与 Kiana 落点

| 参考实现 | 可复用的行为模式 | Kiana 的落点 | 明确不照搬的部分 |
|---|---|---|---|
| OpenCode | credential tagged union、同 integration 单活跃凭据、OAuth 提前刷新；provider config、credential、provider.use policy 三分离；权限请求有 once/always/reject 决策 | `CredentialKind`/`CredentialLease`、Provider catalog→config→policy、ControlPlane approval 与审计 | 不把 key 存入配置/Connection，不让插件改写 policy，不提供 secret read-back |
| Codex workload identity | assertion 每次重读；16 KiB/UTF-8/NUL 校验；token URL/redirect/timeout/body 上限；single-flight、generation CAS、transient/permanent 错误分类 | workload `SecretRef` adapter、OAuth/工作负载刷新器、`CI-09` 并发与轮换测试 | 不把 access token 暴露给 Core/Runner，不用“刷新成功”推断 Kiana 授权 |
| Strix | PKCE S256 + state；跨进程 refresh lock + 锁内重读；旧 refresh 保留；0600/O_EXCL/atomic replace；安装身份与 provider token 分开 | OAuth account store、LocalInstance identity、daemon 本地 SecretStore | JWT claim 只作为外部 account metadata，不能本地当作 assignment 证据 |
| OpenHands | credential validity 与 permission validity 分离；只读 whoami/list probe；`missing_scope`、invalid、unknown 分级；presence-only UI | Connector `CredentialProbe`、结构化错误码和诊断投影 | 不执行 destructive probe，不因未知/非 JSON 错误删除凭据 |
| Continue / Cline / Roo / Crush / Goose | provider-specific auth 注入、API key/OAuth/AWS 等 credential chain、取消与 refresh 生命周期；覆盖默认 auth header 防重复 | Provider adapter 的最后边界注入、route/credential revision fencing | 不引入第二套 provider registry/执行循环，不允许 credential chain 绕过 ControlPlane |
| ADK / MetaGPT / gpt-pilot / Claude Code | OAuth/JWKS/API key 的认证、scope、expiry 分层；配置来源和 provider registry 的组织方式 | 作为 DTO、错误分类、配置迁移的对照测试 | 其 raw-key 配置、客户端自选身份和宽松 fallback 不作为 Kiana 规范 |
| CompanyOS 规范与本地实现 | Principal/Organization/Assignment/DataBoundary、ProjectTrust、authority epoch、EventLog/Receipt 事实链 | `CI-02` 至 `CI-12` 的领域合同、fencing、恢复和证据块 | 规范目标不当作已实现能力；当前状态仍以源码、测试和 `CURRENT_STATUS.md` 为准 |

### 29.2 当前代码基线与必须消除的分叉

1. `kiana-provider/src/config.rs` 已有 profile、endpoint、TLS/loopback、capability 和 concurrency 校验，但 `ProviderConfig.api_key` 与 `Connection.credential` 仍允许 raw secret 长驻内存；`configuration_revision` 只能记录 secret hash，不能替代 SecretRef/lease。
2. `kiana-daemon/src/model_client.rs` 仍有第二份 profile/env 解析（`profile_routes`/legacy provider path），会造成 precedence、缺失凭据和默认 profile 语义分叉；最终只能保留一个 ConfigResolver。
3. `DaemonHost` 当前固定 `AuthenticatedPrincipal { actor_id: "local-user", allowed_roles }`，`RequestMetadata` 仍可携带 actor/role/department/trust 声明；服务端必须继续覆盖这些声明，并逐步迁移到 durable LocalHumanPrincipal、SessionOwnership 和 assignment 投影。
4. `RequestContext`、`RoleSpec`、`AccountBinding` 尚缺稳定 `PrincipalId`、`AssignmentId`、`SecretRef`、`ProviderAccountId`、`authority_epoch`、data boundary 和 credential generation。
5. `docs/schemas/kiana-app-server-config-resolved.v1.schema.json` 与 secrets schema 的 `additionalProperties` 占位不能作为运行时校验；schema、迁移和 unknown-field 行为必须进入可执行测试。

### 29.3 目标领域合同（先落在 domain，再接 ports）

以下对象只允许序列化非秘密元数据。`Debug`、`Display`、`Serialize`、错误和事件投影必须输出 opaque ref、digest、状态和时间，不得输出 secret 原值。

```text
PrincipalId / OrganizationId / MembershipId / AssignmentId
ProjectId / SessionId / ServiceIdentityId / ProviderAccountId
SecretRef { store, key, purpose, audience, generation }
ConfigRevision / AuthorityEpoch / CredentialGeneration

Principal { id, kind: human|agent|service|mcp|provider, status, created_at }
Membership { id, principal_id, organization_id, status, valid_from, valid_until, authority_epoch }
RoleAssignment { id, principal_id, organization_id, role_id, department_id,
                 project_scope, capability_scopes, policy_profile, status,
                 valid_from, valid_until, authority_epoch }
ProjectAssignment { id, role_assignment_id, project_id, scope, status, authority_epoch }
ServiceIdentity { id, principal_id, credential_ref, capability_scopes, rotation_policy }
ProviderAccount { id, provider_id, external_subject?, tenant?, data_boundary, status }
CredentialBinding { id, provider_account_id, kind: key|oauth|workload,
                    secret_ref, scopes, endpoint_binding, status, generation, expires_at }
ConfigSource { kind: cli|workspace|user|env|builtin, trust, path?, precedence }
ConfigSnapshot { schema, source_refs, effective_non_secret_config,
                 config_revision, project_trust_revision }
AuthoritySnapshot { principal_id, organization_id, project_id, session_owner,
                    role_id, department_id, policy_profile, data_boundary,
                    authority_epoch, assignment_ids }
CredentialLease { lease_id, secret_ref, provider_account_id, purpose,
                  audience, endpoint_digest, issued_at, expires_at, one_shot }
SharingGrant { source_project, target_project, scope, purpose, operations, expires_at }
```

状态规则沿用 CompanyOS 目标合同：Principal `Proposed → Active → Suspended → Revoked → Archived`；Membership `Invited → Accepted → Active → Suspended/Expired → Revoked`；RoleAssignment `Requested → Approved → Active → Reduced/Suspended → Revoked`。撤销、降权、项目解绑和 credential rotation 都必须递增相应 fencing version；旧 Run、Grant、Approval、Scheduler trigger、CredentialLease 不能继续使用旧版本。

### 29.4 配置解析与快照流程

配置解析必须是纯的、可重放的、带来源的，且不让项目配置在未通过 ProjectTrust 前进入有效集合。推荐有效优先级：

```text
explicit non-secret CLI
  > trusted workspace/project config
  > user config
  > environment variables
  > built-in defaults
```

secret 不在此优先级中合并原值；配置只声明 `SecretRef` 或受限的环境变量名称。实现顺序：读取并限制文件大小 → 解析版本化 schema → 拒绝未知 major/字段 → canonicalize path/URL/JSON → 校验 ProjectTrust/source trust → 合并非秘密 patch → 解析 provider/model/profile → 生成不含 secret 原值的 `ConfigSnapshot` → 计算 `config_revision` → 原子发布新快照。旧 revision 在已有 admission 中保持只读，reload 不能隐式改写运行中的 route。

配置拒绝至少包括：未知字段、未知 schema major、重复 profile、空模型、非法 provider/protocol、带 userinfo/query/fragment 的 URL、非 HTTPS 的非 loopback endpoint、重定向、超限 body/frame、未信任项目引用本地 config/skill/plugin、显式 profile 缺失和无效 secret reference。provider policy 在完整 catalog/route 组装后单独评估 `provider.use`，不能藏在 provider 配置或插件里。

### 29.5 凭据解析、注入与生命周期流程

```text
SecretRef
  → CredentialStore resolve (env/keyring/file/OS/workload source)
  → validate purpose/audience/provider/account/endpoint/scope/expiry
  → issue CredentialLease (short TTL, one-shot, non-delegable)
  → provider transport inject header/body/file handle
  → redact all observations and dispose lease
```

只有 CredentialStore/Broker/Provider transport 可以接触解析后的 secret；ControlPlane、Runner、EventLog、Receipt、Memory、UI、子 Cell 和普通 adapter 只接触 `SecretRef`、presence、generation、expiry 和 digest。环境变量是输入来源，不是把 raw value 传播到 `Connection`、配置快照或事件的许可。文件凭据使用大小/NUL/UTF-8 校验、`0600`、临时文件 `O_EXCL` + 原子替换；endpoint 禁止 redirect、代理泄漏和非绑定目标。

OAuth/工作负载身份统一使用 typed lifecycle：PKCE S256 + 随机 state + callback anti-CSRF；access/refresh token、scope、external account subject、issued/expiry、refresh generation 分离保存；提前 skew 刷新；同一 account 使用 single-flight；`refresh(observed_generation)` 和 `invalidate_if_current` 做 compare-and-swap，旧 refresh 结果不得清掉新 token。瞬态 408/429/5xx/网络错误且旧 token 仍有效时可以保留旧 token 并延迟重试；永久错误、scope 不足、state/redirect 不匹配和 revoked 立即 fail-closed 并转 `reauth_required`/`revoked`。

连接测试只允许只读 `whoami`/`list`/advertised probe，并区分 `credential_invalid`、`scope_insufficient`、`endpoint_unreachable`、`provider_error`；不能因为非 JSON 错误或一次 probe 失败就删除凭据，也不能把“已配置”显示成“已登录”。

### 29.6 身份、assignment 与一次运行的绑定

受保护入口先认证本地实例/客户端，再从存储的 Principal、Membership、RoleAssignment、ProjectAssignment 和 ProjectTrust 派生 `AuthoritySnapshot`。wire 的 actor、role、department、agent、trust、model profile 只作为请求意图或诊断字段；不一致、过期、撤销、跨项目且无 SharingGrant、session owner 不匹配时拒绝，Broker 调用数必须为零。

`AuthoritySnapshot` 在 Run/Invocation admission 时固化：`principal_id + assignment_ids + authority_epoch + project_id + role_id + policy/config revision + data boundary`。每次 `dispatch`、`continue`、`approval consume` 和 effect-time 再检查 epoch/状态；子 Cell 权限严格为 `parent ∩ template ∩ department ∩ project ∩ packet ∩ approval`。`model_profile` 只能从服务端 RoleAssignment/RoleSpec 派生，模型文本不能切换 provider、account、scope 或 endpoint。

事件和 Receipt 只记录主体/assignment/organization/project/provider-account ID、revision、generation、source kind、digest、拒绝原因和状态转移。需要新增 `identity.authenticated`、`assignment.bound`、`config.snapshot_published`、`credential.lease_issued`（无值）、`credential.rotated`、`credential.revoked`、`oauth.refresh_succeeded/failed` 等事实事件；任何 secret sentinel 出现在 prompt、transcript、event、receipt、stdout/stderr、argv、env、cache、error 或 provider echo 都是发布阻断。

### 29.7 端到端处理与故障路径

```text
Ingress
  → protected transport / instance auth
  → server-derived AuthoritySnapshot
  → trusted ConfigSnapshot
  → provider catalog + provider.use policy
  → ControlPlane grant/approval/budget admission
  → CredentialLease resolve at effect boundary
  → route/authority/credential revision re-check
  → provider request (single attempt or classified bounded retry)
  → normalize response
  → append redacted event + usage/receipt metadata
  → dispose lease; continue/paused/failed/result_unknown
```

故障必须按边界处理：认证失败、assignment 过期、ProjectTrust 失败、policy deny、config invalid、credential missing/expired/revoked、scope insufficient、route drift 和 lease mismatch 都是 `failed`/`paused` 且零 effect；请求已经发出但结果不可确认时只能是 `result_unknown`，不能用 credential retry 或模型重试掩盖；rotation/revoke 后旧 invocation 不能因为缓存命中继续发送。daemon 重启后 lease 默认失效，恢复必须重新做 authority/config/credential admission；未知 schema major 或事件矛盾同样暂停并 fail-closed。

### 29.8 详细实施步骤

每一步都遵循“先拒绝、再成功、最后回归”的顺序；每个完成单元必须有源码快照、精确命令、退出码、测试 fixture/cassette、feature_status、proof_level 和 limitations 证据块。编号是本专项局部编号，不替代 P0–P6。

| Step | 目标与代码归属 | 依赖 | 先拒绝的验收 | 成功/回归验收 |
|---|---|---|---|---|
| <a id="step-ci-01"></a>`CI-01` | 基线盘点与迁移护栏；`docs/schemas`、`kiana-daemon/model_client.rs`、`kiana-provider/config.rs` | — | raw secret sentinel 在 debug/error/event/receipt/argv/env/cache 任一通道出现即失败；重复 profile parser 行为差异被测试捕获 | 固化当前 env/profile precedence、旧 `local-user` 兼容事件和 config migration fixture |
| <a id="step-ci-02"></a>`CI-02` | Domain 稳定 ID、Principal/Assignment/ProviderAccount/SecretRef/ConfigSnapshot/AuthoritySnapshot 合同；`kiana-domain` | CI-01 | 空/非法/跨类型 ID、过期状态转移、`Debug`/serde 泄漏 secret、authority epoch 回退拒绝 | round-trip schema、状态机合法转移、只输出 ref/hash 的 Receipt projection |
| <a id="step-ci-03"></a>`CI-03` | Ports 分层；`IdentityResolver`、`CredentialResolver`、`ConfigSnapshotStore`、`Rotation/Revoke`；`kiana-ports` | CI-02 | port 不能返回 raw secret 到 core/runner；错误 resolver、取消、版本不匹配 fail-closed | fake stores 支持 deterministic snapshot、lease 生命周期和故障注入 |
| <a id="step-ci-04"></a>`CI-04` | 受保护 Daemon ingress 与本地主体迁移；`kiana-daemon`、`kiana-client`、`kiana-protocol` | CI-02, CI-03 | 无/错 bearer 或 Unix credential、伪造 actor/role/trust、错误 Origin/Host、他人 session/project 全部拒绝且 0 broker calls | `LocalInstance → LocalHumanPrincipal → ProjectTrust → SessionOwnership` 可从事件重建；旧 local-user 只通过显式 migration event 兼容 |
| <a id="step-ci-05"></a>`CI-05` | Durable Membership/RoleAssignment/ProjectAssignment/PolicyProfile/DataBoundary/SharingGrant 与 authority epoch；`kiana-core`、`kiana-domain` | CI-02, CI-04 | revoked/expired assignment、跨项目无 grant、scope 超集、旧 epoch 的 run/grant/approval/scheduler dispatch 拒绝 | effective capability 正确取交集；角色、部门、项目、packet 和 approval 全链路绑定 |
| <a id="step-ci-06"></a>`CI-06` | 单一配置解析器与 schema/migration；新增 `ConfigResolver`，删除 daemon legacy parser；`kiana-provider`/`kiana-daemon` | CI-01, CI-03, CI-04 | unknown field/major、未信任 workspace config、非法 URL/redirect、profile 缺失、配置超限拒绝且不请求 provider | precedence、canonical snapshot、原子 reload、revision fencing；同一输入跨入口得到同一 snapshot |
| <a id="step-ci-07"></a>`CI-07` | SecretStore 与 CredentialLease；env/keyring/file/OS backend 的窄适配器；`kiana-provider`/`kiana-capability-broker` | CI-02, CI-03, CI-06 | missing/expired/revoked/wrong-purpose/wrong-endpoint SecretRef、one-shot 重放、lease 超时、secret sentinel 泄漏均 0 effect | fake secret store 注入成功，Broker 只在 effect boundary 解析并在完成后销毁 lease |
| <a id="step-ci-08"></a>`CI-08` | ProviderGateway 接线与 route admission；`kiana-provider`、`kiana-daemon`、`kiana-core` | CI-05, CI-06, CI-07 | route/config/authority/credential revision 在 admission 后漂移、provider 试图反推 actor、redirect/proxy/host 变更拒绝 | `Connection` 不保存 raw credential；fake provider 收到正确 opaque account binding 和一次注入，usage/receipt 不含 secret |
| <a id="step-ci-09"></a>`CI-09` | OAuth/工作负载身份生命周期；PKCE、state、callback、refresh single-flight、generation CAS、0600 atomic file | CI-07, CI-08 | state/redirect mismatch、坏/超限 token response、scope 不足、旧 refresh 覆盖新 token、并发 refresh 重复发请求 | 提前 skew refresh、瞬态错误保留有效 token、永久错误 reauth/revoked、rotation/revoke 立即 fencing |
| <a id="step-ci-10"></a>`CI-10` | Provider policy、只读 credential probe 与 UI/诊断边界；`kiana-policy`、`kiana-entrypoints`、protocol DTO | CI-06, CI-08, CI-09 | denied provider 被插件/默认 route 重新启用；probe 把 `missing_scope` 当 invalid credential；诊断/HTTP/Receipt 返回 secret | provider.use 按明确 precedence/last-match 评估；用户看到 configured/expired/reauth/scope 状态而不是凭据值 |
| <a id="step-ci-11"></a>`CI-11` | 审计、redaction、rotation/revoke、recovery projection；`kiana-core`、`kiana-eventlog`、`kiana-daemon` | CI-04..CI-10 | event replay 遇 unknown schema、stale epoch/revision、lease 缺失、credential refresh failure 不得自动 resume | 重启默认暂停；回放 identity/config/assignment/credential binding 后显式重新 admission；审计可按 ref/generation 查询 |
| <a id="step-ci-12"></a>`CI-12` | 产品链 deny-first/UAT 与发布证据收口；CLI/Web/Workbench/Desktop、fake provider、live opt-in | CI-01..CI-11 | 缺认证、未信任项目、越权/跨项目、过期审批、secret 泄漏、TOCTOU、重放、result_unknown 全矩阵 | fake provider 完整黄金链；按连接/协议的 live 只在显式 opt-in、有脱敏 fixture 和独立 evidence block 时宣称 `live` |

### 29.9 依赖批次与现有 roadmap 对接

```text
Wave A: CI-01 → CI-02 → CI-03
Wave B: CI-04 ∥ CI-06
Wave C: CI-05 → CI-07 → CI-08
Wave D: CI-09 ∥ CI-10
Wave E: CI-11 → CI-12
```

与现有专项的接点：`P0-K1-01`/`CP-01`/`CP-08`/`CP-09` 提供身份与 authority epoch；`P4-J7-08`/`P4-J7-09` 接入 ConfigSnapshot、SecretRef 和 ProviderGateway；`CP-13`/`CP-14` 负责 grant/lease 消费，`CP-18`/`CP-19` 负责恢复 fencing；`P1-C-03` 使用 assignment-derived `model_profile`；`CAP-25`/`CAP-28` 负责最小凭据注入和 egress；Connector 线必须复用 `CI-05` 的 ProviderAccount/DataBoundary，不得自建账号或 authn。

实施时允许拆分 `CI-07`、`CI-09`、`CI-10` 为更小的 agent 任务，但不得改变依赖顺序或把 raw secret 传入 Core/Runner。任何需要新增网络、远端租户、支付或第二执行循环的提案都必须另行登记，不能借此专项默认开启。

### 29.10 验收矩阵与证据口径

最低负向矩阵：缺认证、伪造/不一致 actor、错误 session/project、未信任项目、过期/撤销 assignment、跨项目无 SharingGrant、unknown config field/major、非法 endpoint、缺失/过期/错误用途 SecretRef、lease 重放、rotation/revoke race、OAuth PKCE/state/redirect 失败、scope 不足、provider policy deny、route/config/authority revision drift、TOCTOU、provider redirect、crash/replay、`result_unknown`、以及全输出通道 secret sentinel 扫描。

成功矩阵：本地 fake key、fake OAuth refresh、fake workload identity、多个 profile/connection、只读 probe、批准后的 local write、取消、重启后显式恢复、Receipt/审计查询和跨 CLI/Web/Workbench 的同一 snapshot。先使用 deterministic fake transport/store；真实服务商只按协议、连接和 credential kind 分别登记 fixture、环境、退出码和限制。

本专项的完成不以“结构体存在”或“配置文件能解析”为准。只有拒绝路径证明无 Broker effect，成功路径证明 route/authority/credential 三个 revision 一致，且 `CURRENT_STATUS.md` 证据块明确 `feature_status` 与 `proof_level`，才能推进对应 Step；任何 secret 泄漏、权限并集、旧 epoch 复用或恢复自动放行都会阻断发布。

---

<a id="swarm-coordination-design"></a>

## 30. 多 Agent 协调 / Swarm：实际代码设计、处理流程与详细实施步骤（2026-09-13 追加）

> 本专项补全 [module-map.md](module-map.md) 的「多 Agent 协调 / Swarm」模块。它描述目标设计、源码缺口和可执行步骤，不把当前 WIP 或 reference 能力写成已交付能力。当前状态仍以 `CURRENT_STATUS.md` 为准；本节新增的 `SW-*` 均从 ⏳ 开始，完成后才回填对应的 `P4-J6-01`、`P1-C-02`、`P1-D-01/02/03`、`P1-E-01`、`CO-18..23`、`CO-42..44`。

### 30.1 结论先行：Swarm 的边界和目标

Swarm 是 Workflow 中一种**有界的 fan-out/fan-in 策略**，不是第二个 Agent runtime、自由消息总线或新的权限中心。允许的首版拓扑只有：

```text
Supervisor → Workers
Planner → Builders → Reviewer
Parallel Map → Deterministic Reduce
Specialists → Synthesizer
```

所有子任务仍由同一个产品脊柱驱动：

```text
Swarm command / Workflow node
  → DaemonHost
  → ControlPlane admission
  → CellRegistry reserve / claim / lease
  → fresh child Session + Run
  → KianaHarness model loop
  → Capability Broker / sandbox
  → EventLog facts
  → typed result / review / merge decision
  → deterministic reducer
  → release and retire
```

明确禁止：自由 `SendMessage` 或全员广播、共享父 transcript、模型文本直接创建子 Cell、模型可见的第六个 delegation 工具、无界递归、`first_success` 竞速取消、子任务自行重试未知副作用、Integrator 直接运行另一套 Agent loop、以及把“所有 child completed”当作业务 Acceptance。规划/监控 symposium 仍不得让 Builder 列席；若要讨论，只能使用有限议程、轮数、消息数、token、wall-time 和 stall 上限。

### 30.2 参考调研与采用取舍

本轮先完整盘点 `reference/` 的 73 个目录，再精读与编排、任务、恢复、取消和权限直接相关的实现；`docs/reference-agent-audit/00-unified-agent-flow.md`、`99-kiana-mapping.md` 和 `company-os-organization-business-survey-2026-09-12.md` 是汇总入口。下表记录采用的行为模式和不照搬的保证：

| 来源 | 可采用的机制 | Kiana 的落点 | 不可直接推导的保证 |
|---|---|---|---|
| ChatDev DAG、OpenSpec、Task Master | 拓扑分层、依赖闭环、规范化 ID、确定性 ready/join | `WorkPacket`/`packet_graph`、`PartitionGraph`、SW-02 | 层内并发本身不证明写集安全或持久恢复 |
| Archon-Knowledge | 显式 `fan_out.items`、`max_parallel`、输入 key collision 拒绝；workflow retry 与 resume 分离 | `Partition`、`QueueEntry`、SW-02/SW-06/SW-11 | 不采用 `first_success`；未知副作用不能靠 retry 参数掩盖 |
| Gastown | 持久 convoy 与短命 worker 分离；调度器只注入容量、ready 查询和执行回调 | `SwarmPlan`/`WorkPacket` 持久，Cell/worker 可回收；SW-06 | 调度器成功不代表 worker 的外部效果成功 |
| Beads | CAS claim、同 holder 幂等、冲突可诊断；ready 查询不授予执行权 | `PacketClaim`/`PacketAttempt`/lease fence；SW-05/SW-06 | caller-asserted actor 不是认证身份 |
| LangGraph | 每个 superstep checkpoint；pending writes 保留已成功 sibling，恢复不重跑 | `SwarmCheckpoint`、per-partition completion；SW-11 | checkpoint 不是外部副作用 exactly-once；serializer 必须 allowlist/strict |
| AutoGen Magentic-One | progress ledger、RequestToSpeak、max turns/stalls、manager state | `ProgressLedger`、定向 `StatusReport`、SW-09/SW-16 | runtime subscription 和取消一致性不能当作 Kiana 授权证据 |
| Agency Swarm、MetaGPT | handoff 与 delegated work 分层；按 recipient/address 投递结构化工件 | `Handoff`/`DelegationPacket`/`TypedChildResult`；SW-07/SW-16 | flat callback history、自由 SendMessage、模型 JSON command 都不是事实或权限边界 |
| DeepSeek Harness、Cline、OpenCode、Crush、Goose | fresh child context、RunID/sequence、terminal must-deliver、effect-before-publication、取消排空 | child lineage、EventLog CAS、SW-08/SW-10/SW-17 | 参考实现中的内存 session、吞错 projector、弱 sandbox 不作为 durable 证明 |
| Pydantic AI、OpenAI Agents、Agent Framework | validate-before-defer、typed approval、call ID 绑定、独立 retry budget、可序列化 RunState | child proposal/approval/review identity；SW-12/SW-14 | SDK 的 approval callback 不替代 ControlPlane grant；handoff 不改变 Kiana authority |
| Temporal | workflow 与 activity/I/O 分离、heartbeat、attempt、取消通知与停止确认分离 | scheduler/worker/side-effect 边界、SW-09/SW-10/SW-11 | activity retry 不提供外部系统 exactly-once |
| Kubernetes Lease | holder identity、renew time、lease transition 和 fencing 语义 | `SupervisionLease`/worker epoch；SW-06/SW-09/SW-10 | lease 过期不会自动杀死旧进程，必须在 effect-time 再验 fence |
| MCP Tasks | 长任务有 receiver-owned task ID、状态机、轮询和取消终态 | 仅作为 Connector/MCP 适配参考；SW-17 | 当前 Kiana 继续关闭 HTTP MCP；MCP task 状态不能成为 Swarm 事实源 |

官方一手资料也只用于约束行为：[LangGraph 的 durable execution/checkpoint 文档](https://langchain-ai.github.io/langgraph/concepts/durable_execution/)说明 queue/worker/checkpoint 分离和 pending writes，已成功 sibling 不应被恢复重跑；[OpenAI Agents 的 HITL 文档](https://openai.github.io/openai-agents-python/human_in_the_loop/)要求以原始 call identity 序列化审批并由原 run 恢复；[Microsoft Agent Framework 的 orchestration 文档](https://learn.microsoft.com/en-us/agent-framework/workflows/orchestrations/)将 sequential/concurrent/handoff/group-chat/magentic 作为不同编排模式，支持把人审作为 workflow 节点；[Kubernetes Lease 文档](https://kubernetes.io/docs/concepts/architecture/leases/)强调 `holderIdentity`/`renewTime` 只是协调信号，effect-time 仍需 fencing；[MCP Tasks 规范](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/tasks)明确任务取消后必须保持 `cancelled`，即使底层执行继续。这些资料支持本节的边界，但不提升 Kiana 的 proof level。

### 30.3 当前源码基线与明确缺口

当前工作树已经有一个有价值的 deny-first 骨架，但它仍不能整体称为 durable/live：

| 位置 | 已有能力 | 必须补齐的细节 |
|---|---|---|
| `kiana-domain/src/swarm.rs` | `SwarmPlan`、Controller、Child、Failure、Merge、命令和状态；Create 限 packet/concurrency/depth/TTL/预算/路径/fingerprint；Start/Reconcile/Merge/Cancel 有拒绝条件 | 显式 `PartitionId`、`ChildCellId`、`AttemptId`、`DispatchIntentId`、`workflow_instance_id`、partition strategy、结构化 merge policy、lineage 和 per-child idempotency |
| `kiana-core/src/swarm.rs` | EventLog stream CAS、idempotency、公司 `StartRun` 接线、reconcile/cancel/retire | `StartChild` 仍直接触发 Company command；需持久 DispatchIntent/QueueEntry、capacity/fair ordering、typed delegation events、每 child supervision/checkpoint 和 result reconciliation |
| `kiana-core/src/cell_registry.rs` | reserve/commit、grant/lease 校验、并发、checkpoint/restore、root/child/path/budget 限制、retire | 跨进程资源事实、per-attempt 状态、旧 epoch 结果拒绝、partial reserve 回滚和完成结果核销要与 EventLog 原子边界明确 |
| `kiana-domain/src/packet_graph.rs` | DAG/cycle/missing dependency/ready/claim TTL 基础 | 将“ready 只读”与“claim/dispatch 执行权”分离到统一服务；失败传播、success sibling 保留和 successor attempt 规则需固定 |
| `kiana-domain/src/work_packets.rs` / Company | WorkPacket、Handoff、claim、Review、fresh Builder StartRun 路径 | packet 与 worker 生命周期解耦；同 packet 的 rework 产生新 attempt/successor，旧 Unknown 不得复用 |
| `kiana-core/src/events.rs` / projection | 运行事件、receipt、部分 Swarm command 事件 | `delegation_started/completed/failed/reconciled`、progress/heartbeat/checkpoint、causation/correlation/sequence/epoch 和 terminal replay |

现有 Controller grant 将 packet 写路径做 union，且当前模板允许 delegation；首版应把 controller 视为无模型执行的 admission/supervision 资源，child grant 只携带对应 partition 的路径、数据范围、预算和 approval 交集。`max_depth=1` 可以保留，但必须记录为显式 non-recursive policy，而不是遗漏字段。

### 30.4 目标对象和状态合同

持久事实与短命执行状态分开。以下对象先在 `kiana-domain` 固化，再通过 `kiana-ports` 接入 Core；所有 schema 必须 canonical serialize、拒绝未知 major/字段，并把 digest/version 写进事件。

```text
SwarmPlan
  { swarm_id, project_id, workflow_instance_id?, parent_cell_id?, owner,
    partition_strategy, partition_ids, child_count_limit, max_depth,
    max_concurrency, spawn_rate_limit, ttl, max_turns/messages/model_calls,
    max_tokens/effects/wall_time, merge_policy/version, approval_ref,
    idempotency_key, policy/config/authority revisions }

Partition
  { partition_id, swarm_id, ordinal, input_refs, data_scope, owned_paths,
    output_contract/version, dependency_partition_ids, work_fingerprint,
    status, child_cell_id?, active_attempt_id? }

ChildAttempt
  { attempt_id, partition_id, child_cell_id, parent/root/correlation IDs,
    session_id, run_id?, dispatch_intent_id, authority/grant/budget/lease epochs,
    input_digest, policy/config/template revisions, status, retry_of? }

DispatchIntent / QueueEntry
  { intent_id, swarm_id, partition_id, attempt_id, expected_revision,
    priority, ready_at, enqueue_key, capacity_class, claim_owner?, lease_epoch,
    expires_at, state, retry_after?, idempotency_key }

ProgressLedger / SupervisionLease
  { attempt_id, heartbeat_seq, last_progress_at, stall_count, max_stalls,
    checkpoint_ref, current_phase, next_expected_event, holder_identity,
    renew_at, expires_at, fence_epoch }

TypedChildResult / ChildFailureReport
  { attempt/partition/cell/run IDs, producer identity, output/evidence refs,
    usage, path/data manifest, status, error_class/reason_code,
    partial_output_refs, result_unknown, retryable, policy snapshot }

MergeDecision / MergeReceipt
  { decision_id, swarm/workflow/partition refs, strategy/version,
    accepted/rejected refs, conflict_refs, reviewer/acceptor identity,
    review/evidence/policy refs, canonical child order, created_at }
```

首版状态转移应固定为：

```text
Swarm:       proposed → validated → reserved → ready → running
             → ready_to_merge → merged/completed
             → failed | cancel_requested → cancelled | result_unknown
             → retired
Partition:   planned → ready → queued → claimed → running → waiting
             → succeeded | failed | cancelled | result_unknown
Attempt:     admitted → dispatched → active → paused | stopping
             → completed | failed | cancelled | result_unknown | fenced
```

终态不可回滚。Retry 只创建新 `ChildAttempt`/`DispatchIntent`，且只对已确认无副作用的 typed failure 开放；Resume 复用同一 attempt 的 checkpoint，不能借 resume 复活终态 Run。`result_unknown` 具有最高保护优先级：不允许 merge、自动 retry、资源释放或声称成功，必须进入 Incident/reconciliation。

### 30.5 端到端处理流程

```text
1. Plan
   PM/Sponsor/Workflow 产生 SwarmPlan proposal；Planner 只提交结构化分区建议。
2. Validate
   Core 校验 project/packet approved、DAG/inputs、partition 写集与 data scope 互斥、
   WorkFingerprint、merge policy、count/depth/spawn-rate/TTL/budget/authority。
3. Admit atomically
   一次受保护提交预留 project/runtime budget、父 Cell、child grant 上限、path/data locks、
   PacketClaim、SupervisionLease 和 DispatchIntent；任何一项失败都不能留下部分许可。
4. Queue
   durable QueueEntry 按 priority/deadline/aging/partition ordinal 的 canonical key 排序；
   ready 查询只读，worker claim 使用 CAS + lease epoch + capacity，迟到 worker 被 fence。
5. Materialize
   为每个 Partition 创建 fresh child Cell/Session/Run/Attempt，输入只来自冻结 refs 和授权
   context；不复制父私有 transcript、secret 或未批准 tool state；先写 delegation_started。
6. Execute
   child 经同一 DaemonHost→ControlPlane→Broker→Harness 执行；工具结果、usage、phase 和
   checkpoint 先落 EventLog，再发布 UI/stream；模型不能扩大角色、路径、provider 或预算。
7. Supervise
   worker 按 lease heartbeat；每个 model turn、tool batch 和 side-effect boundary 做 checkpoint；
   progress ledger 记录停滞、stall、重试和 next action；耗尽任何上限就 typed failure。
8. Reconcile
   Parent 只折叠持久 Run/Invocation/Attempt 事实；缺 terminal、事件断裂、旧 epoch 或无法确认
   外部效果进入 result_unknown + Incident；已完成 sibling 的 checkpoint 不重跑。
9. Review
   独立 Reviewer 对每个 typed result 按冻结 criteria/evidence 验证；reviewer/acceptor 不能是
   child author；不接受 transcript 自述或“命令退出 0”替代证据。
10. Reduce
    仅按固定 partition ordinal 运行 versioned deterministic reducer；检查 output schema、
    path/data/output conflict 和完整覆盖；v1 只支持 all-settled/all-success，partial accept
    必须是显式 policy，result_unknown 永远拒绝。
11. Release
    只有所有 child terminal 且无 unknown，才按 attempt→partition→controller 顺序 exactly-once
    释放 grant/lease/claim/locks/unused budget；Packet、Attempt、Evidence、Receipt 继续保留。
12. Surface
    UI/CLI 只投影 parent-child tree、queue、heartbeat、budget、failure、review、merge 和 terminal；
    重连先 hydration 再合并 cursor 后事件，迟到订阅者仍能得到 terminal。
```

### 30.6 详细实施步骤（SW-00–SW-18）

每一步都执行“先拒绝、再成功、最后回归”；测试名是待新增或需加强的验收目标，不表示当前已通过。

#### 波次 A：基线、契约与确定性计划

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功与退出条件 |
|---|---|---|---|---|
| <a id="step-sw-00"></a>`SW-00` | 现状 reconciliation；`CURRENT_STATUS.md`、`kiana-domain/{swarm,packet_graph,work_packets}.rs`、`kiana-core/{swarm,cell_registry,collaboration}.rs`、`kiana-ports`、daemon tests | — | `swarm_reconciliation_does_not_claim_durable_from_in_memory_cas`；记录当前状态、WIP hash、已有测试和未证明窗口 | 生成“已接线/仅类型/缺测试/未实现”表；把 `P4-J6-01`、CO-43/44 与 SW 步骤一一映射，不重建已有 Cell/Packet 类型 |
| <a id="step-sw-01"></a>`SW-01` | 稳定 ID、schema 和 lineage；domain + protocol + ports | SW-00 | unknown major/field、空 ID、cross-swarm partition、revision/epoch 回退、canonical bytes 不稳定均拒绝 | `SwarmPlanId/PartitionId/ChildCellId/AttemptId/DispatchIntentId/QueueEntryId/MergeDecisionId` round-trip；parent/root/workflow/correlation/causation 可定位 |
| <a id="step-sw-02"></a>`SW-02` | 显式 Partition/WorkGraph validator；复用 `packet_graph` | SW-01 | `swarm_plan_rejects_partition_overlap_and_unbound_input`、cycle/missing ref、重复 key/fingerprint、first_success、超 count/depth/concurrency/spawn-rate/TTL/budget 均拒绝 | ready/blocked/failed 输出稳定排序；每 partition 有 disjoint path/data scope、input digest、output contract 和 canonical fingerprint |
| <a id="step-sw-03"></a>`SW-03` | Swarm/Partition/Attempt 状态 reducer 与 typed transition events；core/domain events | SW-02 | 非法回退、重复 terminal、unknown→success、merge 前缺 review、terminal 后继续 dispatch 均拒绝 | 每次转移产生一条 `delegation_*`/state event；replay 与 live reducer 结果一致，单一 terminal 可核验 |

#### 波次 B：授权交集与原子准入

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功与退出条件 |
|---|---|---|---|---|
| <a id="step-sw-04"></a>`SW-04` | 从 parent/template/department/project/packet/approval 派生 child grant；`kiana-core` authority/capabilities + `cell_registry` | SW-03；CP-04/08/13、P1-C-02 | `swarm_rejects_grant_superset_and_controller_delegation`；伪造 role/path/provider/secret、旧 authority epoch、跨项目无 SharingGrant 均 0 broker effect | controller 是无模型 admission/supervision Cell；child 仅有 partition-specific 交集，reviewer/acceptor 身份与 author 分离 |
| <a id="step-sw-05"></a>`SW-05` | 统一原子 admission；budget/path/data lock/claim/grant/supervision/intent 同一 CAS 边界 | SW-04；CO-19/20 | `swarm_spawn_reservation_is_atomic_and_idempotent`；任一资源失败无 partial reservation；重复 key 同 payload 返回原 receipt，改 payload/authority 冲突 | 一次 admission 绑定 authority/config/policy revisions；WorkFingerprint 活跃复用被拒，已终态结果按显式复用策略只读引用 |

#### 波次 C：持久调度与 fresh child

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功与退出条件 |
|---|---|---|---|---|
| <a id="step-sw-06"></a>`SW-06` | durable `DispatchIntent`/`QueueEntry`、容量、公平顺序、claim lease/fence/backoff；`kiana-core`/`kiana-eventlog`/`kiana-ports` | SW-05；P1-D-01/02/03 | `swarm_queue_claim_fences_late_worker`；ready 查询不得取得执行权，容量超售、过期 lease、旧 worker result、无界 retry 均拒绝 | 同一 snapshot 在多入口得到同一 queue order；仅 confirmed stop 后 requeue；失败 attempt 与 successor attempt 可区分 |
| <a id="step-sw-07"></a>`SW-07` | fresh child Cell/Session/Run/Attempt 和 DelegationPacket；daemon/core/runner | SW-06；CO-21/23 | `swarm_child_uses_fresh_session_without_parent_private_history`；父 session 重用、输入/secret 漏传、child scope 超集、duplicate materialization 均拒绝 | 每 Partition 恰好一个 active attempt；冻结 input refs、authorized retrieval、lineage、template/policy/config/authority snapshot 进入 receipt |
| <a id="step-sw-08"></a>`SW-08` | 统一执行路由和 child correlation；复用 `DaemonHost→ControlPlane→Broker→KianaHarness` | SW-07；H01、CP-23/24 | `swarm_no_second_runtime_or_delegate_tool_reaches_harness`；模型生成 delegate/free message、直接 shell/provider、未授权 child run 均 0 effect | fake model 的 child tool call 与 parent/child/partition/attempt/correlation 事件可对齐；五个模型工具集合保持不变 |

#### 波次 D：监督、取消与恢复

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功与退出条件 |
|---|---|---|---|---|
| <a id="step-sw-09"></a>`SW-09` | heartbeat、checkpoint、ProgressLedger、stall/escalation 与分层预算 | SW-08；P0-J1、P2-K4、P4-J7 | `swarm_progress_ledger_escalates_stall_within_budget`；缺 heartbeat、超 turns/messages/tokens/effects/wall/TTL/stalls、checkpoint 越权均 typed failure | model turn/tool batch/side-effect boundary 可恢复；进度报告定向到 parent，重试预算与运行预算分离 |
| <a id="step-sw-10"></a>`SW-10` | parent→child cancel、drain、fence、Incident/Unknown；core/daemon/process supervisor | SW-09；P0-J1、CP-15/16/20 | `swarm_cancel_drains_started_children_and_marks_unknown`；取消后新 dispatch 被挡，已启动 effect 排空，未启动项有 synthetic terminal，旧 epoch 结果不能完成/释放 | 区分 cancelled/stopping/result_unknown；无法确认的现实效果永不自动 retry 或 merge |
| <a id="step-sw-11"></a>`SW-11` | EventLog replay、pending writes、崩溃恢复与显式 re-admission | SW-10；P0-G-04、CP-18/19 | `swarm_replay_rejects_gap_unknown_schema_and_terminal_conflict`；崩溃窗口、缺 checkpoint、审批/authority/lease 失效均暂停且 0 effect | `swarm_recovery_preserves_successful_siblings`；重启重建 queue/attempt/child tree，成功 sibling 不重跑，恢复必须重新过 grant/approval/fence |

#### 波次 E：结果、reduce 与独立合并

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功与退出条件 |
|---|---|---|---|---|
| <a id="step-sw-12"></a>`SW-12` | TypedChildResult/ChildFailureReport 与 delegation facts；domain/core/event projection | SW-11 | `swarm_child_failure_is_typed_and_persisted`；伪造 producer、缺 output/evidence/attempt、transcript 自述、secret sentinel、错误 status 组合均拒绝 | `delegation_started/completed/failed/reconciled` 带 parent/child/run/partition/attempt/causation/correlation/epoch；结果只引用 immutable artifact/evidence |
| <a id="step-sw-13"></a>`SW-13` | versioned deterministic reducer、冲突和完整覆盖检查 | SW-12；CO-43/44 | `swarm_merge_is_canonical_and_rejects_result_unknown`；unordered input、path/data/output conflict、missing partition、first_success 或 unknown result 均拒绝 | canonical partition order + strategy/version；v1 all-success/all-settled 可重放，partial acceptance 必须显式 policy 和证据 |
| <a id="step-sw-14"></a>`SW-14` | Independent Review、MergeDecision、MergeReceipt；`company` review + core swarm | SW-13；P3-I-04、CO-44 | `swarm_merge_covers_each_partition_once`；reviewer=author、旧 review/criteria、duplicate decision、未验证 output、reviewer 自己修改事实均拒绝 | 一 partition 一 immutable decision；receipt 绑定 reviewer/acceptor/policy/evidence/conflict refs；Swarm merge 不等于 Project Acceptance |
| <a id="step-sw-15"></a>`SW-15` | exactly-once release/retire、残余预算与事实保留 | SW-14 | `swarm_retire_releases_only_owned_resources_once`；unknown/in-flight/foreign Cell 不能释放，重复 retire 不退还未知成本 | child→controller 级联回收 grant/lease/claim/locks/unused budget；Packet/Attempt/Event/Evidence/Receipt 保留可查，controller 最后 retire |

#### 波次 F：通信、投影和总验收

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功与退出条件 |
|---|---|---|---|---|
| <a id="step-sw-16"></a>`SW-16` | 定向 WorkPacket/Handoff ACK/StatusReport/Evidence/Incident；可选 bounded Symposium | SW-15；P1-E-01/02、P4-E-03 | `swarm_no_free_message_or_broadcast_reaches_harness`；共享 transcript、未认证 recipient、超 round/message/token/stall、Builder 列席规划/监控均拒绝 | parent 只向具名 recipient 发结构化输入；ACK/negative ACK、progress、failure、replan 都可回放 |
| <a id="step-sw-17"></a>`SW-17` | parent-child UI/event projection、sequence/epoch/cursor、terminal replay/hydration | SW-16；P4-J7-02/03、P2-M5 | `swarm_late_subscriber_gets_terminal_without_duplicate_effect`；乱序/陈旧 event、断线 hydration 覆盖新事件、UI action 越权均拒绝 | CLI/Web/Workbench/desktop 展示同一树和 next action；UI 只读投影，terminal must-deliver，重连不重做 dispatch |
| <a id="step-sw-18"></a>`SW-18` | deny-first 全矩阵、fake golden、崩溃/竞态/replay 和发布证据 | SW-17；P4-J6-01、CO-42/43/44 | 覆盖未信任、越权、cycle/overlap/dup fingerprint、budget/concurrency/TTL/depth/spawn-rate、stale claim/heartbeat、cancel race、unknown、merge conflict、schema gap、crash-after-reserve/dispatch/effect、secret injection | `swarm_full_fake_model_golden_flow_is_deterministic`：两 disjoint partitions → fresh builders → reconcile → independent review/merge → release/retire；跨进程重启每个边界均不重复副作用 |

### 30.7 依赖波次、现有 roadmap 接点与实现顺序

```text
Wave A: SW-00 → SW-01 → SW-02 → SW-03
Wave B: SW-04 → SW-05                    (P1-C-02, CO-19/20)
Wave C: SW-06 → SW-07 → SW-08             (P1-D-01/02/03, CO-21/23)
Wave D: SW-09 → SW-10 → SW-11             (P0-J1, P0-G-04, CP-15/18/19)
Wave E: SW-12 → SW-13 → SW-14 → SW-15     (CO-43/44, P3-I-04)
Wave F: SW-16 → SW-17 → SW-18             (P1-E, P4-J7, UI projection)
```

`P4-J6-01` 是本专项的汇总验收，不另建 Swarm runtime。`CO-18..23` 提供 claim、Cell、ProcessManager 和 fresh Builder 路径；`P1-D-*` 提供 DAG/ready/lease；`CP-04/08/13/15/18/19/23/24` 提供 authority、grant、取消、恢复和统一命令提交；`P2-K4/K6` 提供 checkpoint/Incident；`CO-43/44` 提供并行 Builder、Integrator 和 MergeReceipt 业务接线。若某个既有单元已经有真实证据，SW 步骤只补其缺口，不重复建设 EventStore、Workflow engine、Broker 或 Harness loop。

实现顺序中的一个硬规则是：**先把拒绝路径做成可观察的零 effect，再做成功路径**。尤其先复现并固定三类反例：未批准项目仍可 dispatch、ready 查询被误当执行权、child/parent 共享 session 或 transcript；以及三类恢复反例：reserve 后崩溃、effect 后未知、成功 sibling 被重复执行。

### 30.8 验收矩阵与证据规则

最低负向矩阵应包含：

- 身份/权限：未认证、伪造 parent/role/department、跨 project、旧 authority epoch、controller delegation、grant 超集、secret/provider scope 注入；
- 计划/资源：未知 schema、缺 input、DAG cycle、path/data overlap、duplicate fingerprint/idempotency、超 count/depth/concurrency/spawn-rate/TTL/turn/token/effect/wall budget、容量超售；
- 调度/运行：ready 冒充 claim、重复 dispatch、过期 lease/heartbeat、迟到 worker、stale result、父取消竞态、进程组未停止、未知外部效果；
- 恢复/事实：事件 gap、未知 major、terminal conflict、坏 checkpoint、pending writes 丢失、旧 approval/lease 自动复活、成功 sibling 重跑；
- 合并/沟通：缺 partition decision、重复 decision、author 自审、旧 criteria、unknown 接受、conflict 隐藏、free broadcast/shared transcript、Builder 进入冻结 symposium；
- 泄漏/入口：prompt/transcript/EventLog/Receipt/UI/stdout/stderr/argv/env/cache/provider echo 出现 sentinel secret，或 CLI/Web/Workbench/desktop 产生不一致动作。

成功矩阵使用 deterministic fake model、fake Broker、真实 `DaemonHost`、真实 EventLog 和 CellRegistry，至少跑两 disjoint partitions；逐边界注入 crash、CAS contention、provider truncation、cancel、lease expiry、review reject 和 result_unknown。真实 provider 或远程连接只在显式 opt-in 下单独登记，不把 fake 或 source-only 证据升级成 `live`。

每个 SW 完成单元必须在 `CURRENT_STATUS.md` 追加：

```text
step: SW-xx / P4-J6-01 or linked unit
source_snapshot: HEAD + touched-file hashes
worktree_status: exact relevant files and dirty WIP
command_argv: exact focused/replay/release commands
cwd / environment: repo, toolchain, feature flags, no secret values
fixture / cassette: plan/partitions, fault point, input hash
exit_code / matched_tests: actual results only
artifacts: DispatchIntent, RuntimeEvent, checkpoint, Evidence, MergeReceipt refs
status change: feature_status before → after
proof-level change: source / local_behavior / durable / live (no inflation)
limitations: external effect, cross-process, provider, timing and untested gaps
reviewer: actual independent review or explicitly none
```

本专项完成的必要条件是：没有第二执行循环、没有自由消息总线、没有权限并集；每个 child 有 fresh context、attempt、lease/fence 和 typed result；每个不确定效果保留为 `result_unknown`；合并顺序和证据可从 EventLog 重放；所有入口只投影同一事实。结构体存在、单测通过、一次本机成功或“所有 child completed”均不足以提升 `P4-J6-01` 的状态。

---

<a id="observability-audit-plan"></a>

## 31. 可观测性与审计专项：实际代码设计、处理流程与详细实施步骤（2026-09-13 追加）

> 本专项补全 [module-map.md](module-map.md) 的“可观测性与审计”模块，和 `P1-J8-01`、`ER-30` 对接。它是给实施 agent 的代码设计与验收合同，不是当前能力声明；真实状态仍以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准。
>
> 本次任务说明优先于 roadmap 中较早的限制性文字；但既有安全宪法、ControlPlane 单一执行脊柱、EventLog 事实源、五个模型可见工具和 deny-first 证据纪律仍然是本专项的边界。专项局部编号 `OA-*` 不替代 P0–P6，也不把已有 `ER-*` 或 `P1-J8-01` 自动标成完成。
>
> 调研覆盖 `reference/` 的 DeepSeek Harness、Codex、OpenCode、Cline、Goose、Crush、Letta Code、OpenHands、Agno、OpenAI Agents、Pydantic AI、CrewAI、A2A、Archon-Knowledge、ECC、Graphiti、Temporal、LangGraph、Promptfoo 等，以及 `docs/reference-agent-audit/`、CompanyOS 平台/运营/质量规范和本地 EventLog journal。外部标准采用 [OpenTelemetry signals](https://opentelemetry.io/docs/concepts/signals/)、[OpenTelemetry trace API](https://opentelemetry.io/docs/specs/otel/trace/api/)、[W3C Trace Context](https://www.w3.org/TR/trace-context/) 和 [OpenTelemetry semantic conventions](https://opentelemetry.io/docs/specs/semconv/)。参考项目只提供机制启发，不证明 Kiana 的实现，也不复制其源码或凭据格式。

### 31.1 设计结论：五类对象，四条权威边界

可观测性不能被实现成“给所有函数加日志”。Kiana 需要把事实、诊断、统计、因果和审计拆成不同的数据合同：

| 对象/信号 | 权威问题 | 写入方式 | 失败语义 | 不得承担的职责 |
|---|---|---|---|---|
| `RuntimeEvent` / `TransitionBatch` | 发生了什么、按什么顺序、哪个命令被接受 | ControlPlane 构造；EventStore CAS/幂等提交 | 提交未知时不得 dispatch；损坏或矛盾 fail-closed | 不因 telemetry exporter 成功而变成成功；不保存 secret 原值 |
| `CommandReceipt` / `ExecutionReceipt` / `RunReceipt` | 哪个事实提交了、某次 attempt 的 effect 是否已确认、一次 Run 的可重算结果 | 从 EventLog + Artifact refs 纯投影 | 源 cursor、artifact 或 projector 不可读时返回错误/Unknown，不造空 Receipt | 不代表业务 Outcome、现实世界交付或 trace 已完整 |
| `OperationalLog` | 组件为何慢、失败、重试或降级 | 由 `ObservabilityPort` 结构化发出；可异步导出 | 可丢失但必须计数并提升 Health；不能阻塞 EventStore commit | 不改变 policy、grant、state 或 receipt |
| `MetricPoint` / `MetricSnapshot` | 一段时间的数量、延迟、容量和质量趋势 | 从 committed event reducer 与受控 runtime gauges 产生 | 丢失/聚合落后只影响统计；不得把估算当 measured | 不证明某个 effect 成功，不使用高基数秘密/原文标签 |
| `Trace` / `Span` | 一条命令跨入口、Run、Provider、Broker、EventStore 的因果与耗时 | `trace_id` 根 span，`span_id` 子操作；事件保存关联 ref | 采样或 exporter 失败不改变事实；Span `unknown` 不能投影为 Ok | 不作为授权、审批或恢复依据；不取代事件顺序 |
| `AuditRecord` / `AuditQuery` | 谁以什么权限对什么对象作了什么决定，证据在哪里 | 从安全相关 committed events 派生；必要时写 `audit.*` 事实 | 审计事实提交失败时高影响动作不得执行；查询失败不能返回“无记录” | 不暴露 prompt、tool args、token、密钥或未授权项目数据 |
| `HealthSnapshot` / `Incident` | 哪个组件是否可用、落后、隔离或需要人工处理 | 只读探针 + projector/queue/exporter 观测；Incident 通过 ControlPlane 事件 | stale/unknown 是 degraded 或 unavailable，不是 healthy | 不自动批准、重试未知副作用或关闭 Incident |

四条必须写进代码 review 和测试的边界：

1. **事实先于观察**：副作用前先提交 `TransitionBatch`（含 authority、approval、budget、lease、action digest）；提交后才能派发。提交后的事件再被 reducer 同步/异步转换为 logs、metrics、traces 和 audit projection。
2. **审计先于高影响效果，telemetry 可降级**：高影响动作的 `audit.required` 元数据必须和授权/permit 同一事实提交；OTLP/JSONL exporter、debug log、trace sampler 故障不能让动作获得额外权限，也不能阻塞已提交事实。队列满时只允许丢弃明确标记为 `best_effort` 的诊断信号，不能丢 Audit、Approval、Recovery、terminal 或 EventLog cursor。
3. **显示不是证据**：CLI/Web/Workbench/Desktop 只读 Receipt/Audit/Health DTO；SSE、transcript、缓存和 exporter 都是派生视图。任何“成功”文案必须能定位到 `source_cursor`、`source_event_ids` 和 effect/stop 证据。
4. **未知必须保留未知**：provider timeout、handler stop 未确认、result delivery 丢失、projector gap、artifact 不可读或 audit 事实提交不确定，都要保留 `unknown`/`degraded`，创建 Incident/Recovery 索引，禁止用 span end、metric increment 或模型文本覆盖。

### 31.2 参考项目吸收矩阵

| 来源 | 已观察到的可复用机制 | Kiana 的落点 | 明确不照搬 |
|---|---|---|---|
| DeepSeek Harness | step/request/tool/result event、flush 后结束、usage 和 malformed stream 记录、observer 监听 committed events | `run/turn/invocation` 生命周期、EventStore commit observer、provider normalized metadata | 不把 observer/console JSONL 当作唯一事实；不把 write-behind ack 当作 durable commit |
| Codex | Thread/Turn/rollout identity、interrupt/abort 事件、persist/flush/shutdown ack、每 step context capture | `command_id`/`run_id`/`turn_id`/`correlation_id` 关联、flush evidence、cancel/unknown 记录 | 不把 thread id 当权限边界；不复制大型 runtime |
| OpenCode | event projector、session hydration、aggregate/sequence/schema、SSE 同步 | Audit/Health/Receipt projector、cursor query、rebuild checkpoint | UI Bus 不成为第二数据库；旧事件不直接改业务表 |
| Cline / Crush / Letta Code | Local Runtime Host、RunID/OTID/seq、terminal must-deliver、gap/reconnect、approval audit | `trace/span` 与 `logical_cursor` 关联、terminal/audit 永不因普通 delta 丢失 | 不把断线重连当作成功；不把 UI timeline 作为审计事实 |
| Goose / Temporal / LangGraph | effect-before-event、step checkpoint、pending writes、历史 replay 与 workflow/activity 分离 | effect/stop evidence、Invocation attempt、replay-only projector、RecoveryCase | 不重跑模型、shell、MCP 或外部 effect；不引入完整 Temporal 服务 |
| Agno / OpenAI Agents / Pydantic AI | 可序列化 run state、usage/retry/trace、HITL requirements、独立 limit/retry 层 | `UsageRecord`、span names、approval/action digest、bounded retry metrics | 不把 callback、trace 或 cost estimate 当授权；不把 price unknown 当 cost limit 已强制 |
| OpenHands / Archon-Knowledge / ECC | 健康/活动事件、匿名 usage、security scan、workflow/audit 视图 | opt-in 诊断、presence-only health、配置/插件审计 DTO | 默认不向外发送遥测；不收集 prompt/response/路径/PII；不把扫描结果自动改 policy |
| A2A | 事件顺序、terminal stream、cursor、webhook ack、认证失败/授权拒绝审计 | 多入口 cursor、terminal envelope、未来外部订阅的 delivery evidence | 当前不打开远程/HTTP MCP 或外部 webhook 副作用 |
| Graphiti / Promptfoo | provenance/freshness、按版本分桶的 eval 和 replay divergence | `source_event_ids`、version dimensions、Eval evidence | 检索、eval 或 trace 不授予权限；不把质量分数抵消安全失败 |
| OpenTelemetry / W3C | logs、metrics、traces 是不同 signal；SpanContext/traceparent 传播；低基数和采样约束 | 本地语义约定、trace/span correlation、可选 OTLP adapter | 外部 trace context 只用于关联，不用于认证、assignment 或 authority |

`reference/` 的其余目录按审计相关性归入以下四组，作为 OA-00 的盘点边界：

- **运行时与入口**：`codex`、`deepseek-harness`、`opencode`、`cline`、`crush`、`goose`、`letta-code`、`pi`、`roo-code`、`claude-code-rust`、`mini-swe-agent`、`aider`、`continue`、`OpenHands`、`agent-framework`。
- **状态、工作流与恢复**：`temporal-sdk-python`、`langgraph`、`adk-python`、`pydantic-ai`、`crewAI`、`autogen`、`agency-swarm`、`MetaGPT`、`ChatDev`、`grok-build`、`architect-loop`、`12-factor-agents`、`container-use`、`beads`、`gastown`。
- **记忆、溯源与知识**：`MemPalace`、`memorix`、`mem0`、`letta`、`letta-oss`、`claude-memory`、`claude-mem-candidate`、`graphiti`、`graphify`、`llama-index`、`GitNexus`。
- **治理、扩展与评测**：`Archon`、`Archon-Knowledge`、`ECC`、`everything-claude-code`、`get-shit-done`、`gsd-core`、`gstack`、`OpenSpec`、`promptfoo-full`、`a2a`、`mcp-servers`、`ruflo`、`skills`、`awesome-agent-skills`、`superpowers`、`pm-skills`、`strix`、`orca`、`emdash`、`herdr`、`ai-coding-guide`、`claude-task-master`、`planning-with-files`。

这些目录只用于发现事件、游标、健康、审计、溯源、重放和评测模式；没有直接阅读或与当前模块无关的目录仍必须在 OA-00 的 inventory 中标记为 `reviewed/no-relevant-signal`，不能默认为已实现。

统一吸收规则：使用 `correlation_id`/`causation_id`/parent refs 建立因果图；使用 `stream_version`/`logical_cursor` 建立事实顺序；使用 `trace_id`/`span_id` 表达耗时和跨边界关联；使用 `source_event_ids`/artifact hash 表达审计证据。四套 ID 可以同时存在，不能把其中一套当成其他三套的别名。

### 31.3 目标代码分层与模块落点

```text
Ingress / Client
  → Authenticated RequestContext
  → CorrelationContext { trace_id, span_id, correlation_id, causation_id }
  → ControlPlane admission
       ├─ policy / gate / approval / budget / lease
       ├─ TransitionBatch { audit metadata + action digest }
       └─ EventStore commit (cursor + command receipt)
              ├─ Projectors: Run / Invocation / Receipt / Audit / Health / Incident
              ├─ MetricsReducer (committed facts + bounded runtime gauges)
              ├─ TraceBridge (spans, links, sampled export)
              └─ OperationalLogSink (redacted, bounded, best effort)
  → Capability Broker / Provider / Artifact store
  → result/effect/usage commit
  → Receipt + AuditQuery + Health/Incident projections
  → CLI / Workbench / Web / Desktop DTOs
```

建议代码边界：

| 层 | 主要文件/新接口 | 约束 |
|---|---|---|
| Domain | `kiana-domain/src/observability.rs`、`audit.rs`、`usage.rs`、`governance.rs`、`contracts.rs` | 只放稳定 ID、枚举、schema、digest、DataClass、Retention、TraceRef、MetricPoint、AuditRecord；`Debug`/serde 不输出 secret |
| Ports | `kiana-ports/src/lib.rs` 或独立 `observability` module | `ObservabilityPort`、`TraceSink`、`MetricSink`、`AuditQueryPort`、`HealthProbePort`；明确 durable/best-effort、flush ack、capabilities，不把 exporter 当 EventStore |
| Core | `kiana-core/src/observability.rs`、`events.rs`、`receipts.rs`、`projection.rs`、`recovery.rs` | 只消费已提交事实；建立/结束 span 不能推进状态；审计 query 必须重新做 principal/data-boundary 校验 |
| EventLog | `kiana-eventlog/src/*`、新增 audit/metric projection checkpoint | cursor、frame checksum、command dedup、rebuild、projector lag；commit observer 只在成功 commit 后通知 |
| Daemon | `kiana-daemon/src/health.rs`、`run_stream.rs`、`lib.rs` | 组合根注入 sink/projectors；SSE 只展示；健康探针无副作用；慢消费者不阻塞事实提交 |
| Protocol/Client | `kiana-protocol/src/lib.rs`、`kiana-client` | versioned `AuditQueryRequest/Response`、`HealthSnapshot`、`TraceSummary`、`MetricSnapshot`；禁止暴露原始 EventLog 写接口 |
| Entrypoints | `kiana-entrypoints` CLI/Web/Workbench/Desktop | 统一 query/action 路由；服务端补 owner/scope；导出、reconcile、incident action 回 ControlPlane |
| Scripts/CI | `scripts/`、`CURRENT_STATUS.md` | secret scan、cardinality test、fault injection、rebuild diff、evidence block；不把历史 CI 结果当当前证明 |

### 31.4 观测合同与字段白名单

#### 31.4.1 关联上下文

所有高价值信号都使用服务端构造的不可变 `CorrelationContext`：

```text
CorrelationContext {
  trace_id                  # 仅 trace 关联；入口无可信 parent 时新建
  span_id                   # 当前操作；每个 span 只能结束一次
  parent_span_id?
  correlation_id            # 一次命令/业务链的稳定关联
  causation_id?             # 直接触发它的事件/命令/attempt
  command_id?
  request_id
  organization_id?
  project_id?
  session_id?
  run_id?
  turn_id?
  invocation_id?
  execution_id?
  attempt?
  authority_epoch
  data_epoch
  source_cursor?
}
```

入口可以接收 W3C `traceparent` 作为关联输入，但服务端必须验证格式、生成自己的 authenticated `CorrelationContext`，不能从 `traceparent`、HTTP body、模型文本、MCP 返回值或 UI 字段推导 actor、role、project、grant 或 policy。跨进程/异步队列恢复时，使用新 child span 加 `SpanLink` 指向上游，而不是伪造旧 span owner。

#### 31.4.2 Span 白名单

固定 span 名称先采用：`kiana.command`、`kiana.run`、`kiana.turn`、`kiana.context.assemble`、`kiana.provider.request`、`kiana.capability.admission`、`kiana.approval.wait`、`kiana.broker.execute`、`kiana.eventlog.commit`、`kiana.projector.apply`、`kiana.receipt.project`、`kiana.recovery`、`kiana.audit.query`、`kiana.audit.export`。每个 span 至少有 `start/end`、`status`（`ok|error|unknown`）、`error_code?`、`source_cursor?`、`duration_ms` 和对应稳定 ID。

允许的低基数 attributes：`provider_id`、`model_id`、`capability_id`、`operation`、`sandbox_profile`、`environment`、`component`、`schema_version`、`outcome`、`reason_class`、`retryable`、`approval_required`、`effect_known`、`stop_confirmed`。禁止 attributes：prompt/response 原文、tool arguments、shell command、路径全文、环境变量值、Authorization/header、token、cookie、个人姓名、邮件、自由错误堆栈和高基数 raw IDs。需要定位请求时放到事件/审计的受权限 digest/ref，或在本地 debug fixture 中使用测试专用 allowlist。

#### 31.4.3 Metric 目录

Metric 必须声明 `name`、`kind`（counter/gauge/histogram）、`unit`、`description`、`stability`、`allowed_labels`、`privacy_class` 和 `source`（`event_reducer|runtime_gauge|derived`）。首批 canonical 名称：

```text
kiana.commands.accepted_total
kiana.commands.denied_total
kiana.runs.started_total
kiana.runs.completed_total
kiana.runs.failed_total
kiana.runs.cancelled_total
kiana.runs.result_unknown_total
kiana.runs.duration_ms
kiana.invocations.attempt_total
kiana.invocations.effect_unknown_total
kiana.approvals.requested_total
kiana.approvals.decided_total
kiana.eventlog.commit_total
kiana.eventlog.commit_failure_total
kiana.eventlog.append_latency_ms
kiana.eventlog.durable_cursor
kiana.projector.cursor
kiana.projector.lag_events
kiana.projector.rebuild_total
kiana.observability.queue_depth
kiana.observability.dropped_best_effort_total
kiana.observability.export_failure_total
kiana.provider.request_total
kiana.provider.request_latency_ms
kiana.provider.retry_total
kiana.provider.malformed_stream_total
kiana.provider.input_tokens_total
kiana.provider.output_tokens_total
kiana.provider.estimated_cost_total
kiana.provider.measured_cost_total
kiana.context.compaction_total
kiana.context.cache_hit_total
kiana.memory.retrieval_hit_total
kiana.memory.stale_hit_total
kiana.security.redaction_total
kiana.security.redaction_failure_total
kiana.audit.query_total
kiana.audit.query_denied_total
kiana.health.degraded_total
kiana.incidents.open_total
```

指标标签只允许经过注册表校验的低基数集合，例如 `outcome`、`reason_class`、`provider_id`、`model_id`、`capability_id`、`operation`、`sandbox_profile`、`component`、`schema_major`、`environment`。`run_id`、`session_id`、`request_id`、路径、prompt hash、tool args hash 和用户标识只能作为事件/Receipt/Audit 的查询字段，不能进入常规 metric label。估算成本和实测成本分开；没有 `provider_receipt_ref` 的值不能进入 `measured_cost_total`。

#### 31.4.4 AuditRecord

```text
AuditRecord {
  audit_id
  schema / record_version
  action_kind                 # command|authorization|approval|capability|credential|recovery|query|export
  decision                    # accepted|denied|staged|approved|consumed|failed|unknown|queried|exported
  actor_ref                   # server-derived Principal/Service ref
  organization_ref?
  project_ref?
  session_ref? / run_ref? / turn_ref?
  invocation_ref? / execution_ref? / attempt?
  target_ref / target_kind
  command_id? / request_id
  correlation_id / causation_id?
  authority_epoch / data_epoch
  policy_ref? / gate_ref? / approval_ref?
  action_digest? / input_digest?
  source_cursor / source_event_ids[]
  effect_known? / stop_confirmed?
  artifact_refs[] / provider_receipt_ref?
  reason_code? / error_class?
  redaction_profile / data_class / retention_class
  occurred_at / observed_at
}
```

`AuditRecord` 只能由服务端从已提交事件派生；模型、插件、MCP server、UI 和普通 exporter 不能自行提交“已批准/已完成”审计记录。原始参数只保留规范化 digest；若因调查必须读取受控 Artifact，需要单独 `audit.read_sensitive_evidence` 授权和查询审计。更正只能追加 `audit.correction`，保留原 record 和原因。

### 31.5 端到端处理流程

#### 31.5.1 正常命令与能力调用

```text
Ingress
  → authenticate + derive RequestContext/AuthoritySnapshot
  → create CorrelationContext + root command span
  → validate schema/trust/ownership/policy/gate/approval/budget
  → normalize action and compute action_digest
  → commit TransitionBatch:
       request.accepted / run.authorized / audit.required / reservation facts
  → after Committed/Replayed:
       start run/turn/model/capability spans
       dispatch one fenced Broker attempt
  → normalize provider/tool result and usage
  → commit effect/stop/result/usage/audit facts atomically where possible
  → projector folds Receipt + Audit + Metrics + Health cursor
  → deliver committed result to Harness once
  → end spans, publish best-effort logs/metrics/traces
  → expose redacted Receipt/Audit/Health DTO
```

`Replayed` 可以返回原 `CommandReceipt`，但必须再确认调用账本和 effect receipt；不能因为命令已提交就重复执行 handler。`OperationalLog`、trace exporter 和 live metric sink 都只能观察 committed facts，不可回写 Run 状态。

#### 31.5.2 拒绝、脱敏和审计失败

```text
untrusted/unauthenticated/owner mismatch
  → append denial fact when the caller is authorized to see it
  → redacted denial DTO + audit query visibility rules
  → zero Broker/provider calls

redaction/classification failure
  → do not publish the candidate log/span/audit projection
  → append security/observability failure metadata without secret value
  → high-impact action remains blocked or becomes Unknown according to effect boundary
  → increment redaction_failure and open Incident if repeated

required audit fact cannot commit
  → no permit/dispatch
  → return persistence/audit_unavailable
  → Health degraded; recovery is explicit
```

拒绝事件也要避免信息泄露：未认证调用者只能收到稳定错误码；已认证且有项目范围的操作者才可查询该 scope 内的拒绝原因、policy/gate ref 和 digest。任何 secret sentinel 出现在 log、span attribute/event、metric exemplar、audit DTO、Receipt、stdout/stderr、argv、env、cache、export 或错误字符串都阻断该 Step。

#### 31.5.3 Unknown、取消、重启和对账

```text
provider/handler started
  → timeout/cancel/process loss/result delivery loss
  → record execution attempt with effect_known=false or stop_confirmed=false
  → Run/Invocation = result_unknown / unknown
  → fence grant, lease, worker and resource
  → append incident.opened + recovery.proposed
  → recovery query shows last_durable_cursor and safe/forbidden actions
  → explicit reconcile / retry_without_effect / compensate / restore
  → new authority + approval + idempotency check
  → append observation/correction facts; never rewrite Unknown
```

重启流程先校验 journal header/frame/checksum、读取最后可信 cursor、重建 Audit/Receipt/Health/Incident projector，再将 orphan dispatch 标为 Unknown。重启、trace exporter 恢复或“看到了 terminal event”都不能自动 resume；只有显式恢复命令再次通过当前 authority/policy/gate/approval 才能继续。

#### 31.5.4 审计查询与导出

```text
AuditQueryRequest (server-authenticated principal)
  → validate bounded filters + snapshot/cursor
  → enforce organization/project/session ownership + DataBoundary
  → read AuditProjection at a committed source cursor
  → apply field-level redaction and retention policy
  → return page { records, next_cursor, source_cursor, projection_version, limitations }

AuditExportRequest
  → separate audit.export capability + purpose/recipient/retention
  → freeze source cursor and query digest
  → materialize redacted export artifact + manifest/hash
  → append audit.exported + DeliveryReceipt (if delivered)
  → export failure is visible; no claim of delivery
```

查询必须区分“无匹配记录”和“EventLog/Projection 不可用”；不允许以当前活动 session、UI 选中项或客户端传入的 owner/scope 作为授权。分页 cursor 绑定 `epoch + projection_version + source_cursor + filter_digest`，过滤条件变化、epoch 变化或 retention revoke 时要求重新 hydrate。

### 31.6 详细实施步骤（OA-00–OA-28）

每张卡都按“先拒绝、再成功、最后回归”执行。完成时必须在 `CURRENT_STATUS.md` 写证据块：`source_snapshot / worktree_status / command_argv / cwd·environment / fixture·cassette / exit_code / status change / proof-level change / limitations / reviewer`。`best_effort` telemetry 的本地证据不能提升 durable；Audit/EventLog 的 durable 证据不能自动提升 live 或 physical。

| Step | 代码归属与交付物 | 依赖 | 先拒绝的验收 | 成功与回归验收 |
|---|---|---|---|---|
| <a id="step-oa-00"></a>`OA-00` | 基线与信号 inventory；`module-map.md`、`CURRENT_STATUS.md`、`kiana-core/events.rs`、`kiana-eventlog/*`、现有 `ER-30`/`P1-J8-01` | — | 找出任何把 UI/transcript/cache/metric 当事实、把 trace 当授权、把 receipt 当 Outcome 的路径；重复/私自命名的观测字段列为 issue | 生成 signal matrix、代码 owner、当前 proof ceiling 和迁移清单；不改历史证据 |
| <a id="step-oa-01"></a>`OA-01` | Domain schema 注册；新增 `observability.v1`、`audit-record.v1`、`metric-catalog.v1`、`trace-summary.v1` 合同 | OA-00 | unknown major/required field、非法 status、空 cursor、错误 digest、未注册 metric 或过长 attribute fail-closed | serde round-trip、canonical bytes、minor additive compatibility、schema owner/版本测试 |
| <a id="step-oa-02"></a>`OA-02` | `CorrelationContext`、TraceRef、SpanRef、causation/parent link；`kiana-domain`/`kiana-ports` | OA-01 | 伪造 actor/authority、无效 traceparent、跨 project/session 关联、attempt/command 错配不得改变授权或查询范围 | ingress→run→turn→invocation→provider→broker→eventlog 的关联稳定；异步恢复使用 link 和新 span |
| <a id="step-oa-03"></a>`OA-03` | 统一 `RedactionProfile`、classification、bounded value encoder；复用 `redact_event_value` 并补 span/log/metric/audit/export 边界 | OA-01 | secret sentinel、prompt/tool args/path/env/header/token 进入任一信号或错误通道时阻断发布；redactor error 不回退原文 | split chunk、nested JSON、UTF-8/NUL、oversize、provider echo、artifact/export 全通道扫描；profile hash 可定位 |
| <a id="step-oa-04"></a>`OA-04` | Audit taxonomy 与 `AuditRecord` reducer；`kiana-domain`/`kiana-core` | OA-01, OA-03 | 模型/UI/plugin 伪造批准、完成、导出；原 record 被覆盖；未绑定 source event 的审计行不可见 | command/deny/approval/capability/credential/recovery/query/export 均有 stable action/decision/digest/source cursor |
| <a id="step-oa-05"></a>`OA-05` | `ObservabilityPort`/`TraceSink`/`MetricSink`/`AuditQueryPort`/`HealthProbePort`；Memory/JSONL fake adapters | OA-01..OA-04 | exporter 报错不得授予权限；不支持 durable/flush/capability 的 adapter 被依赖时 fail-closed；sink 不能调用 Broker | fake sink 可记录、注入失败、flush ack、取消和容量；接口依赖方向通过 boundary test |
| <a id="step-oa-06"></a>`OA-06` | EventStore commit observer；`kiana-eventlog`、`StreamEventStore`、`TransitionBatch` | OA-05 | pre-commit 观测不能让未提交事件出现在 Receipt/Audit/metric；重放不重复发通知/副作用 | 仅 `Committed` 发布一次；`Replayed` 可关联原 receipt；cursor/commit boundary、CAS conflict、Unknown evidence 可重建 |
| <a id="step-oa-07"></a>`OA-07` | Run/Turn/Invocation span 生命周期；`kiana-core` projection/runner bridge | OA-02, OA-06 | span end 不能制造 terminal；late delta、duplicate end、stale run/attempt 不覆盖事实 | 每次合法状态转移都有 start/end/status/error/unknown；取消、pause、compact、retry、resume、terminal 一一可定位 |
| <a id="step-oa-08"></a>`OA-08` | Provider/model/stream/usage instrumentation；`kiana-provider`、`kiana-daemon/model_client.rs` | OA-03, OA-07 | malformed/truncated/timeout/retry 不标 `ok`；secret header、prompt、raw response 不进 telemetry | provider/model/route/prompt version、latency、stop reason、usage、retry class、cache usage 可在 Receipt/Audit/metrics 对齐 |
| <a id="step-oa-09"></a>`OA-09` | Broker/approval/effect/stop instrumentation；`kiana-core/capabilities.rs`、`kiana-daemon/harness_capabilities.rs` | OA-04, OA-07 | policy deny/hook block/expired approval/lease mismatch/TOCTOU 均 zero effect 且可查询；cancel 不等于 stop confirmed | admission→permit→dispatch→execution→result/stop 的 attempt 证据完整；effect unknown 保留 fencing |
| <a id="step-oa-10"></a>`OA-10` | EventLog/projector/Receipt/Artifact/Recovery metrics；`kiana-eventlog`、`projection.rs`、`receipts.rs`、`recovery.rs` | OA-06..OA-09 | projector 读空、artifact 读错、cursor gap 不被计为 healthy/zero work；Receipt 不用 metric 补事实 | append/flush/rebuild/query latency、durable cursor、projector lag、orphan/unknown、artifact bytes、last error 可解释 |
| <a id="step-oa-11"></a>`OA-11` | Health snapshot、readiness/liveness、component capability；`kiana-daemon`/`kiana-core` | OA-10 | stale heartbeat、unknown exporter、cursor divergence、corrupt journal、missing audit projection 不返回 healthy/ready | DaemonHost/ControlPlane/EventStore/Projector/Provider/Broker/Artifact/Telemetry 各有 bounded probe、version、state、last success、limitation |
| <a id="step-oa-12"></a>`OA-12` | Metric catalog/reducer/cardinality guard；`kiana-core`/`kiana-eventlog` | OA-10 | raw IDs/path/prompt/secret 作为 label、cardinality overflow、estimated-as-measured、counter reset 伪造成功均拒绝 | event replay 与 live reducer 对同一 cursor 一致；counter/gauge/histogram 单位、label allowlist、overflow 计数和版本稳定 |
| <a id="step-oa-13"></a>`OA-13` | 异步队列、背压和丢弃策略；`kiana-daemon`/`kiana-eventlog` | OA-05, OA-10 | queue full 不丢 Event/Audit/Approval/Recovery/terminal，不阻塞 commit 到死；slow consumer 不能改事实 | best-effort log/trace 丢弃有 reason/counter；flush/shutdown ack、bounded memory、reopen spool、Health degraded 可观察 |
| <a id="step-oa-14"></a>`OA-14` | Trace exporter 与 W3C context adapter；可选 `kiana-observability` crate 或 daemon module | OA-02, OA-07, OA-13 | invalid parent、exporter failure、sampled=false、foreign baggage 不影响 authority；关闭 exporter 不改变 receipt | local no-op/JSONL exporter、可选 OTLP mapping、span links、sampling decision、shutdown flush；不声明外部 backend durable |
| <a id="step-oa-15"></a>`OA-15` | AuditProjection checkpoint/rebuild；`kiana-eventlog`/`kiana-core` | OA-04, OA-06, OA-10 | unknown audit schema、矛盾 decision、source cursor 回退、projection checksum 错误 fail-closed；不得删除原事实 | 新进程从 EventLog + Artifact refs 产生稳定 AuditRecord；同 cursor/version/hash 输出一致，gap 可重放 |
| <a id="step-oa-16"></a>`OA-16` | Audit query command/wire DTO；`kiana-protocol`、`kiana-client`、`DaemonHost` | OA-15, `CP-21/22` | 未认证、跨组织/project/session、客户端 owner/scope 覆盖、无限 limit、raw event endpoint 全部拒绝且 zero effect | CLI/Web/Workbench/Desktop 调用同一只读 query；字段 redaction、limitations、source cursor、projection version 一致 |
| <a id="step-oa-17"></a>`OA-17` | Query cursor、snapshot、分页和慢查询；`kiana-eventlog` cursor API、protocol | OA-15, OA-16 | cursor epoch/filter digest/version 不符、越界 page、projection lag/retention revoke 继续返回旧数据均拒绝 | stable next cursor、bounded page、empty-vs-unavailable 区分、replay during query、multi-tab/reconnect 一致 |
| <a id="step-oa-18"></a>`OA-18` | 审计导出、manifest、delivery evidence；`kiana-entrypoints`/ArtifactStore/ControlPlane | OA-03, OA-16, OA-17 | 无 `audit.export`/purpose/recipient/retention、scope 超集、未完成 artifact hash、delivery unknown 不能声称 exported/delivered | redacted JSONL/CSV/JSON export 绑定 query digest + source cursor + manifest hash + schema + DeliveryReceipt |
| <a id="step-oa-19"></a>`OA-19` | Alert/Incident 规则、去重和 Recovery 关联；`kiana-core/recovery.rs`、daemon health | OA-10, OA-11, OA-15 | alert 不能自动 approve/retry/close；同一 incident 重复风暴、模型自报恢复、unknown 被关闭均拒绝 | projector lag、audit loss、redaction failure、queue overflow、journal corruption、effect unknown 产生可重放 Incident/RecoveryPlan |
| <a id="step-oa-20"></a>`OA-20` | DataClass/Purpose/Retention/Deletion propagation；`data_governance.rs`、Memory/Artifact/Query/Telemetry stores | OA-03, OA-15..OA-19 | 删除只删 UI/cache、retention 绕过 scope、secret/PII 留在 trace/export/spool、audit metadata 与 payload ref 混淆均 fail | payload expiry 与 audit metadata 分离；revoke/deletion/data epoch 传播到 Receipt/Audit/Artifact/Memory/Index/Cache/export，重启后仍一致 |
| <a id="step-oa-21"></a>`OA-21` | Replay/reconciliation diagnostics；新增只读 `replay`/audit consistency fixture | OA-15, OA-19, OA-20 | replay 调模型/Provider/Broker、unknown schema 猜测、divergence 被吞、metric/trace 反写事实均拒绝 | `(invocation_id, attempt, input_digest, status, error_code)` 首个 divergence 可定位；Audit/Receipt/Health projector 与 baseline 一致 |
| <a id="step-oa-22"></a>`OA-22` | Crash/fault injection；EventStore、Broker、Provider、projector、export、shutdown | OA-06..OA-21 | 每个注入点证明 no duplicate effect、no false success、unknown queryable、resource fenced；固定 sleep 不算证据 | prepare/commit/dispatch/result/flush/projector/export 各 fault seed 可重放并生成同分类、cursor 和 limitations |
| <a id="step-oa-23"></a>`OA-23` | Provider-independent eval suite；fake model/provider/broker、GoldenTrace、Promptfoo 风格断言 | OA-08, OA-09, OA-21, `P1-L1-01` | secret、forbidden effect、policy safety、evidence completeness、replay divergence 任一失败阻断 Promote | expected normalized events、audit rows、metrics、spans、receipt assertions、final status、cost/latency bucket 可重现 |
| <a id="step-oa-24"></a>`OA-24` | 四入口审计/健康/Receipt parity；CLI/Web/Workbench/Desktop | OA-16..OA-23, `P2-M2..M5` | 入口自拼字段、自读 EventLog、自行判断成功/健康、自行触发恢复均拒绝 | 同一 run/cursor/query 在四入口的 owner filtering、status、limitations、source refs、unknown、retention 结果一致 |
| <a id="step-oa-25"></a>`OA-25` | 容量、性能和迁移演练；journal/projector/query/export benchmark | OA-12, OA-13, OA-17, OA-20 | 高基数/大 artifact/大 page/慢 exporter 导致事实丢失或无界内存、迁移 unknown version 静默通过均拒绝 | p50/p95/p99 append/flush/project/rebuild/query/export 基线；quota/backpressure/rotation/archive/upgrade/downgrade read-only 有证据 |
| <a id="step-oa-26"></a>`OA-26` | Local durable observability gate；release/smoke/CURRENT_STATUS | OA-22..OA-25, `ER-34` | 历史 CI、内存 sink、单测 mock、无 artifact hash 不能声称 durable；缺限制/审计 scope 测试不放行 | deny→commit→effect→receipt/audit→restart→unknown/reconcile；journal/artifact/export hashes、process/queue/cursor/secret scan 全部记录 |
| <a id="step-oa-27"></a>`OA-27` | Cross-entry/company governance gate；`ER-35`、CompanyOS Review/Delivery/Close、Cost/Memory/Data governance | OA-24..OA-26 | Runtime completed 不能直接变 Delivery/Outcome；audit query/export 不能跨 DataBoundary；Review 不能改 Builder facts | Objective→Project→Packet→Run→Review→Acceptance→Delivery→ClosingReceipt 的 trace/audit/evidence refs 可重建 |
| <a id="step-oa-28"></a>`OA-28` | Physical/live handoff；目标 OS、OTLP backend、隔离 provider/connector 和 operator runbook | OA-26, OA-27, `ER-36` | 无目标 backend/credential/provider receipt/独立账户时只能证明 deny/local；不能把 OTLP export 或 mock receipt 写成 live/physical | 每个 provider/backend/connector 组合单独记录环境、版本、权限、采样、retention、incident、cleanup 和失败矩阵 |

### 31.7 执行波次与依赖

```text
Wave A 事实与边界：OA-00 → OA-01 → OA-02 → OA-03 → OA-04
Wave B 接线与基础信号：OA-05 → OA-06 → OA-07 → OA-08 → OA-09
Wave C 统计与健康：OA-10 → OA-11 → OA-12 → OA-13
Wave D Trace 与审计查询：OA-14 ∥ OA-15 → OA-16 → OA-17 → OA-18
Wave E 运营治理：OA-19 → OA-20 → OA-21
Wave F 可靠性与入口：OA-22 → OA-23 → OA-24 → OA-25
Wave G 交付门：OA-26 → OA-27 → OA-28
```

与已有路线的接点：

- `P0-G-02a/b`、`P0-G-04`、`ER-01..ER-08` 提供事件落账、CAS、cursor、projection 和恢复事实；OA 不重建第二个 EventLog。
- `P0-J7-01`、`P4-J7-02/03`、Provider 专项提供 normalized stream、usage、terminal、sequence/epoch；OA 只增加 trace/metric/audit 投影。
- `P0-K1-01`、`CP-01/08/09/21/22` 提供 authenticated principal、authority epoch、query ownership 和 ControlPlane command；AuditQuery 不自行认证或授权。
- `P1-K5-01`、`P2-K6-01`、`P2-K7-01` 提供 usage/cost、Incident/Recovery、retention/deletion；OA 将它们纳入可定位的 metric/audit/health 证据，不把估算成本当账单。
- `P2-M2..M5`、`P4-L3-01`、`ER-26/27/29/30` 提供入口 projection、版本分桶、cursor、data governance 和高层 health/trace 卡；OA 细化实际字段、查询和测试，不改变既有 owner。
- `CI-01..CI-12` 的 identity/config/credential/SecretRef revision 必须进入 `CorrelationContext`、AuditRecord 和 Receipt metadata；raw secret 永远不进入 observability plane。

### 31.8 最低验收矩阵

每个场景必须同时检查 EventLog、Projection/Receipt/Audit、Telemetry、实际 effect/文件/进程四个观察面；只看返回 JSON 不算通过。

| 场景 | 必须证明 | 关联步骤 |
|---|---|---|
| 未认证、伪造 actor/role、跨项目 query | 稳定 deny；zero Broker/provider calls；不泄漏存在性或 secret；denial audit 只对授权 scope 可见 | OA-02/04/16 |
| policy/gate/hook deny、过期 approval、lease/epoch drift | 无 effect；AuditRecord 有 reason/digest；trace status=error；metrics 不计成功 | OA-04/07/09/12 |
| 正常 provider/tool run | commit cursor、command receipt、attempt、usage、cost type、span、AuditRecord、RunReceipt 可交叉定位 | OA-06..OA-10 |
| provider malformed/truncated/timeout | 不标 Ok；retry class 正确；已发送业务请求不自动重试；Unknown/Incident 可查询 | OA-08/09/19/21 |
| cancel 与 stop race | `cancel_requested`、stop confirmation、late result、terminal event 分离；不能把取消当 stopped | OA-07/09/19/22 |
| exporter/queue 背压 | Event/Audit/terminal 不丢；best-effort 丢弃有计数/原因；Health degraded；shutdown flush 有 ack | OA-05/13/14 |
| projector crash/gap/rebuild | 原事件保留；Receipt/Audit/Health 不造空；cursor gap 可重放；同 cursor 输出稳定 | OA-06/10/15/17/21/22 |
| secret/PII/oversize/redaction failure | log/span/metric/audit/receipt/artifact/export/stdout/stderr/argv/env/cache 全通道无 sentinel；失败不回退原文 | OA-03/08/18/20 |
| retention/revoke/delete | payload/ref 与 audit metadata 分离；epoch 传播；撤销后旧 projection/cache/export 不返回；法律留存由 policy 明确 | OA-18/20 |
| audit query/export | owner/DataBoundary/field redaction/cursor/filter digest/manifest hash/DeliveryReceipt 一致；delivery unknown 不宣称 delivered | OA-16..OA-18/24 |
| crash at every effect boundary | 无重复 effect、无假成功、Unknown 可对账、资源 fenced、recovery action 新授权 | OA-06/09/19/22 |
| 四入口和 CompanyOS closeout | CLI/Web/Workbench/Desktop 同一 query/projection；RunReceipt 不被 Delivery/Outcome 越权改写 | OA-24/27 |

### 31.9 交付、状态和限制

专项完成不是“接入 OpenTelemetry crate”或“看到一条 trace”。必须同时满足：

1. EventLog commit、AuditRecord、Receipt、MetricReducer、TraceBridge、Health/Incident 和四入口 query 都沿同一 `source_cursor`/关联 ID 工作；
2. deny、unknown、redaction、retention、ownership、backpressure、rebuild 和 crash 场景都有负向证据，Broker effect 数量符合预期；
3. 运行时 telemetry 丢失不会改变授权/状态，审计事实丢失不会被隐藏；`CURRENT_STATUS.md` 明确每项是 `source`、`local_behavior`、`durable`、`live` 还是 `physical`；
4. metric labels 通过 allowlist/cardinality gate，trace sampling 不影响审计完整性，导出只包含受控 DTO/Artifact manifest；
5. 每个 external exporter、OTLP backend、真实 provider/connector 都有独立 opt-in、环境、凭据、版本、退出码和限制，不能以本地 fake sink 或历史 CI 代替。

明确限制：本专项不打开第二条执行循环、不把 EventLog 变成远程 telemetry 服务、不默认向第三方发送本地数据、不实现企业 SIEM 合规认证、不以 trace/metric 推断现实业务 Outcome，也不因 observability 设计存在就提升当前产品的 live、enterprise 或 physical 证明等级。

---

<a id="scheduling-workflow-trigger-plan"></a>

## 32. 调度、工作流与触发器：实际代码设计、处理流程与详细实施步骤（2026-09-13 追加）

> 本专项补全 [module-map.md](module-map.md) 的“调度、工作流与触发器”模块。它是实现合同和拆分后的路线图，不是当前完成声明。当前实现事实仍以源码、精确测试回执和 `CURRENT_STATUS.md` 为准；本节不会因为类型、参考项目或单次本地测试通过而提高 `feature_status` 或 `proof_level`。
>
> 调研采用两条线：一是对仓库内 `reference/` 的整目录盘点后，定向阅读 Temporal Python、LangGraph、Codex、Grok workflow journal、Crush、ChatDev、ADK、OpenAI Agents、OpenHands、Goose、Letta 等与持久状态、队列、审批、重试和恢复有关的源码；二是对照 [Temporal application model](https://docs.temporal.io/)、[Inngest durable functions](https://www.inngest.com/docs/learn/how-functions-are-executed)、[Inngest concurrency](https://www.inngest.com/docs/guides/concurrency)、[Trigger.dev idempotency](https://trigger.dev/docs/idempotency)、[Restate durable steps](https://docs.restate.dev/develop/ts/durable-steps) 和 [DBOS steps](https://docs.dbos.dev/python/tutorials/step-tutorial) 的公开机制。参考资料只用于机制比较，不引入其源码、服务端、协议或第二执行循环。

### 32.1 设计目标和不变量

调度器负责发现“现在可以做什么”和取得一个有期限的 claim；工作流负责根据不可变定义和事实历史计算下一步；触发器负责把可信的时间/事件转换成幂等的 Workflow/Run 命令。只有 ControlPlane 能授权和提交副作用，Broker/Runner 只消费已经提交的 permit。三者共享一个事实源和一条执行脊柱：

```text
trusted ingress / clock
        │
        ▼
Trigger matcher ──(dedupe + occurrence)──► Scheduler queue
                                             │
                                             ▼
                                  atomic claim + fencing lease
                                             │
                                             ▼
Workflow planner (pure replay, no I/O)
        │                                  │
        └─ waiting/approval/signal ◄────────┘
        │
        └─ effect reservation (CAS) ─► ControlPlane admission
                                         │
                                         ▼
                               CapabilityBroker / Harness
                                         │
                                         ▼
                         observation / receipt / reconciliation
```

必须长期保持以下不变量：

1. **单一事实源**：定义、触发器、occurrence、队列 claim、node execution、approval、effect reservation、observation 和 recovery 全部由 EventLog 事实重建；内存 map、UI timeline、模型文本和缓存不是权威。
2. **纯规划**：`kiana-workflow` 的 replay/planner 只读取快照、历史、当前受控时间和命令，输出 `NextState + Intent`；不能调用模型、网络、文件、Broker 或随机数。
3. **先事实后副作用**：每次 dispatch 必须先提交带 `action_digest`、authority/data/policy revision、预算和 fencing token 的 reservation；没有 committed reservation 不能执行。
4. **一次 claim、一次终态**：多个 scheduler/worker 可以竞争，但一个 queue item 只有一个有效 lease/fence；过期 lease 只能在确认没有进行中的 effect 后回收。完成、失败、取消和 Unknown 都是幂等终态。
5. **Unknown 不等于失败**：提交超时、进程崩溃、provider receipt 缺失或 stop 未确认时保持 `ResultUnknown`/`Unknown`，走 reconcile 或人工恢复；不能自动 retry 或直接标为 succeeded。
6. **权限取交集**：触发器、工作流、角色、项目、WorkPacket、审批、预算、路径和当前 authority epoch 的交集决定有效范围；子工作流和子 packet 只能缩减父权限。
7. **没有入口专属循环**：CLI、Workbench、Web、Desktop、MCP、scheduler 和 worker 都调用同一个 `DaemonHost`/`ControlPlane`，不得自行启动模型循环、直接执行 capability 或另建权限判断。

### 32.2 参考机制与 Kiana 的取舍

| 参考 | 观察到的机制 | Kiana 采用 | 不采用的部分 |
|---|---|---|---|
| Temporal Python / Temporal docs | workflow 由历史驱动重放；timer、signal、child workflow 是历史命令；Activity 在 worker 上执行并有 heartbeat、取消和 retry | 将 workflow planner 与外部 effect 分层；把 timer/signal/child/failure 写入事实；每个 effect 有 execution/attempt/receipt | 不引入 Temporal server、task queue 协议或任意 Python workflow 代码执行 |
| LangGraph persistence | thread checkpoint 与跨 thread store 分离；super-step 边界持久化；pending writes 让已成功节点不被重跑 | `WorkflowSnapshot` 只保存可序列化运行态；节点结果和待投递结果单独落账；恢复以 cursor/definition digest 校验 | 不把 checkpoint 当 Memory，不让图节点绕过 ControlPlane 写文件或工具 |
| Inngest durable functions | 每个 step 有稳定 identity；事件触发、重试和并发 key 是服务端事实；同一个 step 重放已保存结果 | occurrence key、step/effect identity、并发策略和 bounded retry 持久化 | 不依赖 hosted event bus；Inngest 的自动 retry 规则不能覆盖 Kiana 的 Unknown 约束 |
| Trigger.dev idempotency | task run 用显式 idempotency key 防止重复启动，key 有 scope/TTL | `command_id + idempotency_key + action_digest + occurrence_key` 四元去重；TTL 与 retention 都可审计 | 不把幂等 key 当授权，不允许相同 key 携带不同 payload 或 authority |
| Restate / DBOS | 非确定 I/O 包在 durable step 中，结果 replay；retry 按次数、时间和错误类型分类 | 每个 Broker/Provider/AgentTask invocation 都成为可观察的 durable attempt；已知无副作用错误才可按策略重试 | 不把不可验证的外部副作用声称 exactly-once；没有 provider receipt 时进入 Unknown |
| 本地 Codex / Grok journal | writer flush/shutdown ack、request hash、dense sequence、replay divergence、journal full fail-closed | EventStore commit/flush ack、command digest、连续 cursor、容量上限和 divergence incident | 不复制参考项目的执行器、凭据格式或 UI event bus |
| 本地 Crush / Goose / OpenHands / ADK / ChatDev | 队列 drain、approval pause、checkpoint、human-in-loop 和图式依赖有可复用交互 | 只吸收“暂停/恢复需事实和审批”“取消要 stop evidence”“依赖图先校验” | 宿主权限 shell、宽松 session fallback、内存队列、自动重试和直接 agent-to-agent 总线均不作为权限依据 |

### 32.3 当前源码基线与需要补齐的缺口

| 位置 | 已有事实 | 实际缺口/风险 | 路线图处理 |
|---|---|---|---|
| `kiana-domain/src/automation.rs` | 已有 `WorkflowDefinition`、DAG 节点类型、实例/节点状态、`TriggerConcurrency`、`MissedSchedulePolicy`、Manual/Event/Interval schedule、`Tick`/`Fire` command | `DurableTrigger` 把 occurrence 压在 `pending_keys`/`fired` 聚合字段，缺少独立 occurrence、queue item、lease、fence、clock trust 和 authority revision；没有 calendar/cron 的确定性时区合同 | `AUT-02`、`AUT-04`、`AUT-07`、`AUT-10`、`AUT-12` 先补合同和有界投影；calendar 作为后续可选扩展，不先引入不确定解析器 |
| `kiana-workflow/src/durable.rs` | 纯 `plan_command`/replay、定义校验、DAG、retry/cancel/compensation、trigger interval catch-up 和并发分支 | planner 尚未输出独立 reservation/queue intent；`Tick` catch-up 需要固定上限、cursor 语义和原子 claim；FanOut/FanIn/SubWorkflow 的实际执行与父子预算、深度和 fence 仍需闭环 | `AUT-06`、`AUT-08`、`AUT-13`、`AUT-16`、`AUT-19`、`AUT-20` |
| `kiana-core/src/automation.rs` | 已做 command schema/role/trust/idempotency/revision 检查；commit 后可 dispatch AgentTask/Capability；有 `RecordObservation`、cancel effect、result unknown 路径 | dispatch 在同一 handler 内同步执行；缺少可重取的 reservation/worker claim；proof 和 cancel 辅助路径使用 `read_all`；`guard_workflow_capability` 只在匹配到已知 execution 时拒绝，未知 reservation 需要 fail-closed | `AUT-05`、`AUT-14`、`AUT-15`、`AUT-17`、`AUT-21`；以索引/cursor 查询替换全量扫描 |
| `kiana-daemon` | `DaemonHost` 是产品组合根，已有 execution control、fencing、上下文和 I/O adapter | 尚未发现产品路径中的持久 scheduler/worker；`sdk.rs::watch_scheduled_tasks` 是兼容面，不能成为第二调度入口 | `AUT-09` 建立 daemon 内 Tokio scheduler/worker，但它只提交 ControlPlane command，不直接执行 capability |
| `kiana-domain/src/packet_graph.rs`、`work_packets.rs` | 有确定性 ready view、DAG admission、budget lease 和 claim 语义 | 尚未与 workflow queue/trigger occurrence 共享 fence、父子 scope 和恢复索引 | `AUT-07`、`AUT-19` 统一 claim contract，不复制另一套 packet scheduler |
| `kiana-eventlog`、`kiana-ports` | 有 JSONL/memory event store、CAS、journal 和部分 projection/recovery | scheduler 需要按 aggregate/cursor/due_at/lease 查询和原子 batch；时钟、flush ack、cursor 读取尚未形成 port 合同 | `AUT-03`、`AUT-08`、`AUT-09`、`AUT-21` |

因此，当前已有的 `Tick`/`Fire` 只能视为“受控命令的纯规划入口”，不能写成“已经有 durable scheduler”。当前 `handle_workflow_command` 的同步 dispatch 也只能视为过渡实现；拆分 reservation 与 worker 时仍须复用 `DaemonHost` 和 `ControlPlane`，不能新增第二条执行脊柱。

### 32.4 领域对象和持久合同

优先扩展现有类型，只有在无法保持兼容和不变量时才新增对象。所有对象都使用稳定 ID、`deny_unknown_fields` 或显式 migration；`Debug`、事件、错误和 Receipt 只输出 ref/digest/status，不输出 secret、完整 prompt 或未治理 payload。

```text
WorkflowDefinitionVersion {
  definition_id, version, definition_digest, input/output_schema,
  nodes, max_steps, max_duration, retry_policy, compensation_policy,
  allowed_roles, project_scope, created_by, created_at
}

TriggerDefinition {
  trigger_id, definition_id/version, owner, role, project_scope,
  schedule, event_filter, concurrency, missed_schedule,
  max_firings, expires_at, approval_ref, authority_epoch, policy_revision
}

TriggerOccurrence {
  occurrence_id, trigger_id, occurrence_key, source_event_ref?,
  due_at, observed_at, payload_digest, status, attempt, command_id?,
  coalesce_count, source_cursor, authority_epoch
}

WorkflowQueueEntry {
  queue_id, target(instance|occurrence|node), priority, ready_at,
  definition_digest, input_digest, action_digest, expected_revision,
  status(queued|claimed|blocked|completed|unknown|cancelled),
  claim_owner?, lease_expires_at?, fence?, enqueue_cursor
}

WorkflowNodeExecution {
  execution_id, instance_id, node_id, attempt, input_digest,
  reservation_id, status, session_id, authority_epoch, policy_revision,
  started_at?, lease_expires_at?, ended_at?, effect_known?, stop_confirmed?,
  output_ref?, error_code?, evidence_refs[]
}

EffectReservation {
  reservation_id, execution_id, invocation_id, action_digest,
  permit_ref, owner, scope_digest, budget_lease, path_lock,
  authority_epoch, config_revision, policy_revision, fence,
  expires_at, status(prepared|claimed|settled|unknown|released)
}

SchedulerLease { lease_id, resource_key, owner, fence, expires_at, heartbeat_at }
ClockObservation { source, now_ms, monotonic_sample, trust, observed_at }
WorkflowObservation { execution_id, attempt, response_digest, effect_known,
  stop_confirmed, output_ref, evidence_refs[], observed_at }
RecoveryCase { target_ref, unknown_reason, last_cursor, safe_actions[],
  forbidden_actions[], authority_epoch, status, evidence_refs[] }
```

关键索引至少包括：`definition_id/version`、`trigger_id + occurrence_key`、`ready_at + priority`、`lease_expires_at`、`execution_id + attempt`、`action_digest`、`authority_epoch` 和 `source_cursor`。索引只是可重建加速层；索引丢失时必须从 EventLog 恢复，不能把空索引当作“没有任务”。

### 32.5 端到端处理流程

#### 32.5.1 注册定义和触发器

1. 受保护入口取得服务端 `Principal`、项目身份、`ProjectTrust`、角色 assignment、当前 authority epoch、policy/config revision 和可信时钟观察值。
2. ControlPlane 校验 definition schema、节点 ID、DAG 无环性、输入/输出 schema、最大 steps/duration、角色和项目边界；WorkflowDefinition 一经使用不可原地修改，变更必须注册新版本和新 digest。
3. 注册 TriggerDefinition 时再次检查定义版本、owner/role、approval proof、expires/max_firings、schedule 最小间隔、event filter schema、concurrency/missed policy 和 authority/policy revision。注册只提交 `definition/trigger registered` 事实，不创建 Run，不执行 capability。
4. 注册结果通过 command idempotency key 重放时返回原 commit；相同 key 携带不同 definition digest、actor、project、authority epoch 或 approval 必须拒绝。

#### 32.5.2 事件/Webhook 触发

```text
ingress authentication + source allowlist
  -> normalize {event_id, kind, source, observed_at, payload_digest}
  -> size/schema/signature/project-trust checks
  -> append trigger.event_observed (dedupe on source,event_id)
  -> match enabled trigger definitions deterministically
  -> create TriggerOccurrence (occurrence_key = trigger_id + source event id)
  -> apply concurrency policy and enqueue Fire command
  -> worker claims queue item and asks ControlPlane to start workflow
```

事件 payload 不得直接成为 capability 参数；只能作为已校验的输入 artifact/ref，经 definition input schema 和 policy 重新映射。重复 event、未知 kind、错误签名、过期 trigger、跨项目 source 和 payload 超限都必须在 Broker 之前拒绝且不创建实例。

#### 32.5.3 Interval 调度、错过时间和多 scheduler

1. Daemon scheduler 通过 `ClockPort` 取得 wall-clock 和 monotonic sample；wall-clock 用于 durable `due_at`，monotonic 只用于本进程 lease/timeout。时钟回退、来源不可信或跨重启无法证明连续性时，暂停受影响 trigger 并打开 incident。
2. scheduler 按 `next_at <= now` 的索引读取候选 trigger；每个 occurrence 使用确定性 key（例如 `trigger_id:scheduled_at`），在 CAS 中同时推进 cursor、写 occurrence 和入队，两个 scheduler 只有一个能成功。
3. `Skip` 只产生当前窗口允许的一次 occurrence；`FireOnce` 把错过窗口折叠为一个 occurrence 并记录 missed count；`CatchUp` 按固定 `catch_up_limit` 分批，超过上限转为 blocked/incident，不能在一个 tick 无限循环。
4. 进程重启先从 cursor/occurrence facts 恢复，再执行一次受上限约束的 reconciliation；不能以“启动时 now 已到期”为理由重放已经 committed 的 occurrence。没有 committed claim 的旧 queue item 才能重新入队。

#### 32.5.4 队列 claim、工作流推进和 effect

```text
queued item
  -> CAS queued→claimed(owner, fence, lease_expiry)
  -> load workflow history + immutable definition digest
  -> pure planner computes ready nodes / wait / terminal / effect intent
  -> commit node reservation + budget/path/child scope in one batch
  -> worker verifies fence and calls ControlPlane admission
  -> ControlPlane consumes approval/grant and emits prepared permit
  -> Broker/Runner performs exactly one attempt
  -> commit observation + usage + lease settlement atomically
  -> planner folds observation and enqueues next ready nodes or terminal receipt
```

一个 planner command 最多产生一个可执行 effect intent；并行节点通过多个独立 queue entries 执行，受 definition、project、packet、budget、worker capacity 和 path lock 的交集限制。`Advance`、`RecordObservation`、`Reconcile` 必须带 expected revision、execution/attempt identity 和 source cursor，旧 worker 的结果不能覆盖新 attempt。

#### 32.5.5 Approval、signal、cancel、retry 和 compensation

- **Approval**：node 进入 `WaitingApproval` 后提交 request digest、action digest、scope、nonce、expiry、approver role、authority/policy revision 和 invocation identity。Approve/Deny 以 CAS 单次消费；参数、路径、项目、epoch 或 definition digest 变化即拒绝并要求重新申请。
- **Signal**：外部 signal 必须有已认证 source、signal ID、payload schema、expected revision 和 occurrence dedupe。planner 一次消费一个 signal；重复 signal 返回原结果，不能推进两次。
- **Cancel**：`cancel_requested` 是意图。ControlPlane 先 fence 未开始 queue item，再向已开始 handler 请求 stop；只有 `StopReport`/effect receipt 确认后才可标记 cancelled。无法确认的 attempt 保持 Unknown，资源 lease 继续 fenced。
- **Retry**：纯 Literal/CopyInput/Gate 或 descriptor 明确证明“未 dispatch/幂等”的失败可生成新 attempt；每次 retry 使用新的 execution/command/idempotency key，并重新检查 budget、approval、scope、epoch 和 deadline。任何 effect 可能已发生而无 receipt 时禁止自动 retry。
- **Compensation**：补偿是新的 workflow instance/command，引用原失败实例和 evidence；重新走授权、预算、路径和审批。不得修改原实例的历史，也不得把 compensation 当作原实例成功。

#### 32.5.6 重启、未知结果和恢复

```text
open EventLog -> validate frames/schema/cursor -> rebuild projections/indexes
  -> fence stale scheduler/worker leases
  -> list queued/claimed/reserved/unknown/recovery cases
  -> default affected workflows to Paused/NeedsRecovery
  -> reconcile provider/runner/stop evidence by explicit command
  -> re-authorize current epoch/policy/approval
  -> commit resume/retry_without_effect/reconcile decision
  -> enqueue only newly committed work
```

恢复重放只折叠历史，不重新请求模型、执行 shell/MCP、发 webhook 或消耗审批。`result_unknown` 必须保留最后可信 cursor、attempt、action digest、effect/stop evidence 和安全/禁止动作；人工选择 `reconcile`、`retry_without_effect`、`abandon` 或 `compensate` 后才改变状态。Receipt 的 `Completed` 只证明本机事实链完成，不证明外部业务 outcome。

### 32.6 状态和并发策略

| 对象 | 允许状态 | 关键转移条件 |
|---|---|---|
| Trigger | `disabled → enabled → expired/disabled` | enable/register 需 authority + approval；过期不可自动延长；disable 清理未启动 occurrence 但保留事实 |
| Occurrence | `observed → queued → claimed → fired / coalesced / rejected / expired` | source/event dedupe、max_firings、concurrency 和 trigger expiry 在同一 CAS 判断 |
| Queue item | `queued → claimed → completed / blocked / cancelled / unknown` | claim 必须带 lease/fence；lease 过期先 reconcile，不能直接重跑可能有副作用的 item |
| Node execution | `pending → reserved → running → waiting_approval/waiting_signal / succeeded/failed/cancelled/result_unknown` | reservation 先于 effect；terminal observation 只接受相同 execution/attempt/fence |
| Workflow | `ready → running → waiting/paused/retrying/compensating → succeeded/failed/cancelled/result_unknown` | 由 planner 根据完整历史计算；终态不可被旧 late result resurrect |
| Trigger concurrency | `Reject`、`Queue`、`Replace`、`Coalesce` | Reject 返回稳定拒绝；Queue 保留每个 occurrence；Replace 先 cancel/fence 旧实例且等待 stop/reconcile；Coalesce 只保留一个 pending occurrence 并记录 count/digest |

`Replace` 不得把“发出 cancel”当作旧实例已停止；`Coalesce` 不得丢弃输入差异，至少保留 occurrence IDs、count、payload digests 和合并规则。所有队列、occurrence、attempt 和 child 数量都有硬上限，达到上限返回结构化错误并记录 incident。

### 32.7 代码落点和接口边界

| 层 | 目标改动 | 约束 |
|---|---|---|
| `kiana-domain` | 扩展 automation contracts、occurrence/queue/lease/fence/clock/recovery 对象；定义稳定错误码和状态转移 | 只放值对象、纯状态和 serde；不依赖 Tokio、provider、filesystem 或网络 |
| `kiana-protocol` | 增加 versioned scheduler/workflow commands、snapshot/query DTO、event envelope、occurrence/receipt/recovery payload | wire DTO 不接受 actor/role/trust 自声明作为权威；internal observation 不可由外部调用 |
| `kiana-ports` | `ClockPort`、`WorkflowQueueStore`、`LeaseStore`、cursor/index query、`EffectDispatcher`/`Reconciler` 窄接口 | port 只返回结构化错误和 opaque refs；不得把 Broker/runner 依赖下沉到 domain |
| `kiana-workflow` | 把 trigger tick/fire、ready node、reservation、retry/cancel/compensation/fan-in/out 规划做成纯函数；定义 planner intent | 无副作用；同一 state/history/command/clock snapshot 必须得到字节等价的 next state/intent |
| `kiana-core` | 统一 authority/policy/approval/budget/path lock；CAS 提交 reservation；执行结果、stop report、reconcile 原子回写；未知 reservation fail-closed | 不在 core 之外另做权限判断；拒绝路径必须证明 0 broker calls |
| `kiana-eventlog` | 原子 batch、command dedup、cursor/aggregate query、flush ack、容量和恢复索引 | append 成功不等于 effect 成功；索引可重建，事实不可改写 |
| `kiana-daemon` | 新增受 `DaemonHost` 管理的 Tokio scheduler/worker service，注入 `ClockPort`，有界 channel、heartbeat、shutdown/restart fencing | service 只提交 ControlPlane command/claim，不直接调用 capability handler；单 worktree/单组合根 |
| `kiana-capability-broker` / `kiana-runner` | 消费 prepared permit，回传 execution/stop/evidence；所有 attempt 使用 reservation 的 scope/epoch/fence | 不接受未提交 reservation、旧 fence、模型自造 request 或 wire caller 直接 execution |
| `kiana-query` / UI adapters | 读取可重建的 queue/trigger/workflow/incident/receipt projections；显示 next action、原因、证据和 limitation | 查询和解释不消费 lease/approval，不生成事实，不阻塞 EventStore commit |

### 32.8 详细实施步骤（AUT-01..AUT-24）

下表是可直接分派给 agent 的最小可验证切片。每个 Step 完成前先跑“先拒绝”验收，再跑成功/回归验收；Step 不改变现有 P0–P6 编号，只补充本模块的实施顺序。

| Step | 目标与代码归属 | 依赖 | 先拒绝的验收 | 成功/回归验收 |
|---|---|---|---|---|
| <a id="step-aut-01"></a>`AUT-01` | 基线与迁移护栏；盘点 `module-map`、现有 automation tests、`CURRENT_STATUS`，记录旧 `watch_scheduled_tasks` 兼容边界 | — | 发现第二 scheduler、直接 capability 入口或旧事件无法区分时阻断 | 形成 source snapshot、缺口清单、fixture 命名和不提高 proof 的迁移说明 |
| <a id="step-aut-02"></a>`AUT-02` | `ClockPort`、wall/monotonic、clock trust/rollback；`kiana-ports`、`kiana-domain` | AUT-01 | 回退、零/溢出、未可信 clock 不得延长 lease、approval、trigger expiry | fake clock 可重复驱动 timer、deadline、catch-up；跨重启保存 `ClockObservation` |
| <a id="step-aut-03"></a>`AUT-03` | definition/version/digest、DAG/schema/role/project validation；`kiana-domain`、`kiana-workflow` | AUT-01 | cycle、missing dependency、unknown field、超步数/超时、未授权 role/project、原地覆盖已用版本全拒绝且 0 effect | 同输入产生稳定 digest；旧实例固定使用原 definition version；migration 有显式事件 |
| <a id="step-aut-04"></a>`AUT-04` | TriggerDefinition、event envelope、occurrence key、approval/authority/policy 绑定；`kiana-domain`、`kiana-protocol` | AUT-02, AUT-03 | 伪造 source/event kind、重复 key、过期/超额/跨项目 trigger、event payload 超限均不创建 instance | Manual/Event/Interval 注册和 schema round-trip；event_ref 与证据一一对应 |
| <a id="step-aut-05"></a>`AUT-05` | automation event envelope、aggregate stream、command dedup、CAS/cursor query；`kiana-eventlog`、`kiana-ports` | AUT-03, AUT-04 | 同 key 不同 digest/actor/epoch、revision race、torn tail、flush 未确认不得返回 committed | 多 scheduler 对同一 aggregate 只有一个 commit；分页/重放与全量结果一致 |
| <a id="step-aut-06"></a>`AUT-06` | 纯 planner intent：ready nodes、wait、terminal、reservation、next queue item；`kiana-workflow` | AUT-03, AUT-05 | planner 触碰 I/O、随机、当前系统时间或产生越权 node/empty dependency 不通过 | 相同 history/clock snapshot 字节等价；planner replay 不重复已 committed effect |
| <a id="step-aut-07"></a>`AUT-07` | WorkPacket 与 workflow queue 共用 claim/scope/budget/path-lock contract；`kiana-domain`、`kiana-core` | AUT-05, AUT-06 | 父 scope/预算缺失、并行超限、循环依赖、重复 claim、过期 packet 全拒绝 | `ready_packets`、queue ready view、parent-child intersection 一致；一个 item 只有一个有效 claim |
| <a id="step-aut-08"></a>`AUT-08` | `WorkflowQueueStore`、lease/heartbeat/fence/reclaim；`kiana-eventlog`、`kiana-ports`、`kiana-core` | AUT-05, AUT-07 | 旧 fence/错误 owner/lease 回退、可能执行中的 lease 直接重领、heartbeat 越权均 0 dispatch | 两个 worker 竞争只一胜；安全过期 item 可重领；未知 effect 进入 recovery |
| <a id="step-aut-09"></a>`AUT-09` | `DaemonHost` 内 Tokio scheduler/worker service、bounded channel、shutdown；`kiana-daemon` | AUT-02, AUT-08 | service 绕过 ControlPlane、无限 channel、入口自建模型 loop、关闭时丢 claim ack 均阻断 | fake clock/queue 驱动 tick→claim→command；优雅关闭能 flush/fence，重启不重复执行 |
| <a id="step-aut-10"></a>`AUT-10` | Interval due/cursor/missed policy；`kiana-workflow`、`kiana-core` | AUT-02, AUT-04, AUT-09 | Skip/FireOnce/CatchUp 产生重复或无限 catch-up；`next_at` 与 occurrence 不一致拒绝 | 同一 `scheduled_at` 只一次；CatchUp 有固定批量和 incident；重启后 cursor 继续 |
| <a id="step-aut-11"></a>`AUT-11` | Event/Webhook ingress、签名/source allowlist、dedupe、filter；`kiana-daemon`、`kiana-protocol` | AUT-04, AUT-05, AUT-09 | 未认证、错误签名、未知 schema、重放 event、跨项目 source、payload 注入均 0 broker calls | event→occurrence→Fire 可重放；payload 只按 input schema 映射，不直接成为 capability request |
| <a id="step-aut-12"></a>`AUT-12` | Reject/Queue/Replace/Coalesce 精确定义与有界 pending index；`kiana-workflow` | AUT-07, AUT-10, AUT-11 | Replace 未 stop 就启动 successor、Coalesce 丢 occurrence digest、Queue 超上限无稳定错误均失败 | concurrency matrix 覆盖 active/terminal/race/restart；firing budget 精确结算 |
| <a id="step-aut-13"></a>`AUT-13` | `Advance` ready-node、dependency、FanOut/FanIn/SubWorkflow planner；`kiana-workflow` | AUT-06, AUT-07 | fan-out 越过深度/子 scope/预算、fan-in 缺结果、子定义版本漂移不得 dispatch | 并行节点各有 execution/attempt；fan-in 输入顺序稳定；子实例可恢复 |
| <a id="step-aut-14"></a>`AUT-14` | effect reservation、permit、action digest、authority/config/policy revision；`kiana-core`、`kiana-eventlog` | AUT-05, AUT-08, AUT-13 | 未 reservation、旧 epoch、scope/approval/budget/path drift、未知 execution ID 均 0 broker calls | reservation CAS 后可安全重取；一 reservation 只产生一个有效 attempt |
| <a id="step-aut-15"></a>`AUT-15` | worker dispatch/observation 分离；把当前 inline dispatch 改成可重取 command；`kiana-core`、`kiana-daemon` | AUT-09, AUT-14 | commit 后崩溃、重复 worker、observation revision race 不得重复 effect 或伪造 success | execution/result facts 可重放；已知失败反馈结构化结果，提交不确定返回 Unknown |
| <a id="step-aut-16"></a>`AUT-16` | retry classifier、attempt/lease/deadline、idempotent descriptor；`kiana-workflow`、`kiana-core` | AUT-14, AUT-15 | Unknown、effect receipt 缺失、审批过期、预算不足、非幂等 capability 禁止自动 retry | pure/no-dispatch 和明确幂等失败按 bounded policy 新建 attempt；每次 attempt 可审计 |
| <a id="step-aut-17"></a>`AUT-17` | cancel generation、stop report、late result fence、result_unknown；`kiana-core`、`kiana-daemon` | AUT-08, AUT-15 | queued/claimed/running/unknown 各阶段取消不一致、late result resurrect、stop 未确认却标 cancelled 全失败 | 未启动项 `not_executed`；已启动项有 stop/effect evidence；Unknown 进入 recovery |
| <a id="step-aut-18"></a>`AUT-18` | approval/signal pause-resume、single consume、checkpoint metadata；`kiana-core`、`kiana-runner` | AUT-14, AUT-17 | approver 越权、参数/path/epoch 变化、重复 signal/approval、UI 自带 context 全拒绝且不 dispatch | 重启后 owner/definition/action digest 匹配才恢复；signal 顺序和 dedupe 稳定 |
| <a id="step-aut-19"></a>`AUT-19` | parent-child workflow/WorkPacket fan-out、depth/concurrency/TTL、fail-fast/fan-in；`kiana-workflow`、`kiana-core` | AUT-07, AUT-13, AUT-17 | 子 scope/预算/role 超父级、循环 parent、失败后未启动 child 仍执行全拒绝 | child result/evidence 按稳定顺序合并；parent 状态由事实唯一推导 |
| <a id="step-aut-20"></a>`AUT-20` | compensation workflow、原实例引用和新授权；`kiana-workflow`、`kiana-core` | AUT-16, AUT-19 | 成功/Unknown 原实例被错误补偿、补偿复用旧 approval/lease、补偿越权全拒绝 | failed/cancelled 实例可显式选择 compensation；补偿独立 receipt 和 audit trail |
| <a id="step-aut-21"></a>`AUT-21` | boot recovery、projection/index rebuild、reconcile case；`kiana-daemon`、`kiana-eventlog`、`kiana-core` | AUT-15, AUT-17, AUT-18 | journal/schema/cursor 损坏、stale lease、Unknown、旧 epoch 不得自动 resume/dispatch | 重启默认 Paused/NeedsRecovery；显式 reconcile/resume 后只执行新 commit |
| <a id="step-aut-22"></a>`AUT-22` | scheduler/workflow/trigger snapshot、Receipt、incident query/UI adapter；`kiana-query`、`kiana-protocol` | AUT-05, AUT-21 | 查询消费 claim/approval、投影缺 source cursor、Receipt 将 business outcome 写成 runtime success 全失败 | 同 cursor/version 重算稳定；展示 due/blocked/unknown/reason/evidence/limitations |
| <a id="step-aut-23"></a>`AUT-23` | 跨 CLI/Web/Workbench/Desktop/MCP 的同一 DaemonHost 端到端 UAT；entrypoints、daemon、core | AUT-09, AUT-11, AUT-18, AUT-22 | 入口分叉 loop、直接 broker、伪造 actor、跨项目 trigger、权限并集、TOCTOU 全矩阵 | manual/event/interval→workflow→effect→receipt；deny path 记录 0 broker calls |
| <a id="step-aut-24"></a>`AUT-24` | durable/live 证据与发布门；scripts、fixtures、`CURRENT_STATUS.md` | AUT-01..AUT-23 | 只有类型/单测、内存 queue、未 flush journal、未对账 Unknown 不得宣称 durable/live | 每个切片有 source_snapshot/worktree/argv/env/fixture/exit/status/proof/limitations/reviewer；真实 provider 仅显式 opt-in |

### 32.9 依赖批次和现有 roadmap 对接

```text
Wave A: AUT-01 → AUT-02 → AUT-03 → AUT-04 → AUT-05
Wave B: AUT-06 ∥ AUT-07 → AUT-08
Wave C: AUT-09 → AUT-10 ∥ AUT-11 → AUT-12
Wave D: AUT-13 → AUT-14 → AUT-15
Wave E: AUT-16 ∥ AUT-17 → AUT-18
Wave F: AUT-19 → AUT-20 → AUT-21
Wave G: AUT-22 → AUT-23 → AUT-24
```

与已有卡片的接点如下：`P1-D-01/02/03` 提供 WorkPacket ready/DAG/lease reclaim；`P2-J5-01` 提供 workflow definition/replay 的契约；`P4-K2-01` 提供 trigger 只能创建 Workflow/Run 的边界；`CP-24` 提供 ready_packets、claim、fan-out/fan-in、trigger/Workflow replay 和 parent-child scope 的控制面验收；`CP-27` 提供非阻塞存储、ClockPort、容量和故障注入要求；Event/Receipt/Recovery 专项提供 `Unknown`、reconcile、receipt 和重启 fencing 语义。若已有卡片与本节的 reservation/lease/fence 细节冲突，采用能证明“先事实后副作用、Unknown 不自动重试、权限取交集”的更严格合同，并在实施卡中记录冲突。

### 32.10 最低验收矩阵和证据口径

**先拒绝矩阵**至少覆盖：未信任项目、伪造 actor/role/source、错误 session/project、definition cycle/unknown field、过期/超额/禁用 trigger、重复或错误签名 event、clock rollback、同 key 不同 digest、revision/authority/policy drift、重复 claim/旧 fence、队列/occurrence/child 超限、scope/预算/path lock 超集、过期 approval、重复 signal、Unknown 自动 retry、cancel 未 stop 却 terminal、late result resurrect、fan-in 缺输入、损坏 journal、projection/index miss 和入口绕过 `DaemonHost`。每个拒绝都要证明没有 Broker/handler effect，并有稳定错误码和事件。

**成功和恢复矩阵**至少覆盖：manual/event/interval 三种触发、Skip/FireOnce/CatchUp、四种 concurrency、两个 scheduler 竞争、多个 worker heartbeat/reclaim、DAG/fan-out/fan-in/sub-workflow、approval pause/resume、signal、known failure retry、cancel/stop、compensation、进程崩溃、journal flush、provider receipt 对账、Receipt 重算、CLI/Web/Workbench 共用同一 snapshot。没有 provider 可验证 receipt 的真实 effect 只登记 `local_behavior` 或 `durable` 的事实链，不宣称现实业务 outcome。

每个 `AUT-*` 完成时都必须写入 `CURRENT_STATUS.md` 规定的证据块：

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

`feature_status` 和 `proof_level` 分开填写；“有 `WorkflowDefinition`”“有 Tick 测试”“Receipt 存在”都不能单独把状态写成 implemented/durable/live。若发生 schema migration、事件兼容、索引重建或 recovery limitation，必须在 Receipt 和证据块中留下可查询的 limitation。此专项只新增设计与实施步骤，不改变当前实现状态账本。

---

<a id="persistence-data-layer-plan"></a>

## 33. 持久化与数据层专项：实际设计、处理流程与详细实施步骤（2026-09-14 追加）

> 本专项补全 [`module-map.md`](module-map.md) 第 8 模块，详细设计、参考调研、代码落点、拒绝优先验收和 `PD-00`–`PD-35` 实施卡见[独立专项文档](roadmap/persistence-data-layer.md)。本节只提供总路线图入口和执行摘要，不把当前 WIP 或参考项目能力写成已实现。

### 33.1 设计结论

持久化层采用单一事实源和可重建派生层：EventStore 保存 Runtime/Company facts 与 command receipt；Projector 生成 Run、Approval、Cell、Budget、Memory 和查询读模型；ArtifactStore 保存不可变字节和 manifest；Index/Cache 只提供可重建加速；Backup、Migration、Retention 和 Recovery 都围绕 source cursor、generation、schema/store format、authority epoch 和 data epoch 协作。现阶段保留 JSONL v2 作为事实账本，SQLite/WAL 仅作为投影或索引适配器，避免双写两套事实。

### 33.2 统一处理顺序

```text
resolve StorageRoot/trust
  → lock and capability/migration preflight
  → scan facts and classify integrity state
  → commit canonical transition (CAS/dedup/cursor)
  → dispatch only after Committed/Replayed
  → append result/artifact/effect settlement
  → project with checkpoint and source_cursor
  → query with freshness/generation/provenance
  → backup/restore/migrate/retain through manifests and epochs
```

任何 `Unknown`、损坏中间帧、过期审批、旧 epoch、hash/identity 不匹配或恢复材料缺失都必须保持 fail-closed；Receipt、UI timeline、Memory、Index 和 cache 不能成为第二事实源。

### 33.3 执行波次

| 波次 | 步骤 | 结果 |
|---|---|---|
| A Contracts | `PD-00 → PD-01 ∥ PD-02 → PD-03 → PD-04` | StorageRoot、schema、错误/健康、公共端口 |
| B Facts | `PD-05 → PD-06 → PD-07 → PD-08 → PD-09` | EventStore conformance、JSONL durability、cursor、完整性、projector |
| C Authority | `PD-10 → PD-11 → PD-12 → PD-13` | Receipt、Cell/Budget/Lease、Approval、Pending/Checkpoint |
| D Bytes/Query | `PD-14 → PD-15 → PD-16 ∥ PD-17 → PD-18 → PD-19 → PD-20 → PD-21` | Artifact、workspace、evidence、Memory、Index、cache |
| E Operations | `PD-22 → PD-23 → PD-24 → PD-25 → PD-26` | backup/restore、migration、retention、删除传播 |
| F Hardening | `PD-27 ∥ PD-28 → PD-29 → PD-30 → PD-31 → PD-32 → PD-33 → PD-34 → PD-35` | 并发、安全、health、故障注入、adapter、平台、UAT、发布证据 |

这些编号是追加专项的局部索引，不重排既有 P0–P6 和历史全量 step 编号。每个 PD 完成前先覆盖 deny、越权、损坏、重放、TOCTOU、恢复和 `result_unknown`，再验证成功路径，并在 `CURRENT_STATUS.md` 写入 source snapshot、命令、fixture、退出码、状态/证明等级和限制。

---

<a id="notification-messaging-design"></a>

## 34. 通知与消息：实际代码设计、处理流程与详细实施步骤（2026-09-14 追加）

> 本专项补全 [`module-map.md`](module-map.md) 的第 14 模块。它把“通信与问责”（`P1-E-01`/`P1-E-02`）和“通知投递、人类收件箱、实时事件桥”（`P2-K3-01`、`P2-M2-01`、`P2-M3-01`、`P4-E-03`、`P4-M6-01`）接成一条可实现的代码路线。新增 `NM-*` 是本专项局部编号，不改变既有 P0–P6 或历史 step 编号，也不把当前 WIP 或参考项目能力写成已交付能力。
>
> 当前事实仍以 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 和源码为准：`kiana-daemon/src/run_stream.rs` 已有按 run 的进程内 broadcast、epoch/sequence、有限 terminal replay 和 committed-event 投影；`kiana-protocol` 已有 `RunStreamEnvelope`/`RunStreamEvent`；`kiana-core` 已能把 Approval、Company 和 Recovery 事实拼成 `HumanInboxItem`。这些都是展示或局部投影，尚无独立的 durable 通知收件箱、订阅存储、投递 outbox、已读状态和外部通道证明。

### 34.1 设计结论：四条链必须分开

“消息”“通知”“实时流”“业务动作”不能共用一个模糊对象。它们的事实来源、可靠性和权限不同：

| 对象 | 回答的问题 | 权威来源 | 可靠性/权限边界 |
|---|---|---|---|
| `ConversationMessage` | 这次模型请求看到了什么上下文 | RunSnapshot/历史折叠 | 只服务 Harness；不构成跨部门通信，也不能授予权限 |
| `CommunicationMessage` | 哪个主体向哪个责任主体提交了什么 Chat/Command/Handoff/Decision/StatusReport/Evidence/Incident | ControlPlane 提交的 `RuntimeEvent` | 是业务通信事实；必须有 sender/recipient/scope/causation；普通 Chat 永不变成授权 |
| `Notification` | 某个主体应该看到哪一条已提交事实、严重程度和下一步动作 | EventLog 事件经 `NotificationProjector` 的投影 | 是可重建的投递视图；客户端不能通过它改写业务状态 |
| `DeliveryAttempt` | 某个 channel 是否接受、送达或确认了通知 | Notification outbox/投递事实 | 是副作用账本；ACK 只证明 channel/客户端收到，不证明业务动作已应用 |
| `RunStream` | 当前 run 的增量和终态如何低延迟显示 | committed event + runner delta | delta 可丢；terminal、approval、incident 和 receipt 必须能查询补回 |

通知实现不应建设一个可以独立发布任意消息的第二事件总线。正确边界是：EventLog 是唯一事实源，通知存储是带 `source_cursor` 的可重建投影，outbox 只负责投递生命周期，UI 只提交带版本和幂等键的 ControlPlane 命令。

### 34.2 消息类型与责任状态

`CommunicationMessage` 的最小合同如下；正文应优先使用 Artifact/引用，界面预览只保留经过脱敏和长度限制的摘要：

```text
CommunicationMessage {
  message_id
  message_type          // chat | command | handoff | decision |
                        // status_report | evidence | incident
  sender_principal_id
  recipient_principal_id?   // handoff 必须恰好一个；禁止隐式广播
  organization_id
  project_id?
  work_packet_id?
  cell_id?
  run_id?
  reply_to_message_id?
  causation_event_id?
  body_ref?             // Artifact/内容引用
  redacted_preview?
  schema_version
  authority_epoch
  data_classification
  idempotency_key
  created_at
  expires_at?
}
```

消息类型的语义固定如下：

| 类型 | 允许做什么 | 不允许推断什么 |
|---|---|---|
| `Chat` | 讨论、澄清、建议；可落账供审计 | 不产生 Grant、Approval、状态转移或责任转移 |
| `Command` | 作为 ControlPlane 命令的输入意图 | 消息文本不是已执行命令；必须重新验权、验版本、验幂等键 |
| `Handoff` | 向具名 recipient 交接 frozen WorkPacket/责任 | 发出不等于接收；recipient ACK 前原 owner 仍负责，拒绝 ACK 不得启动 |
| `Decision` | 引用具名 authority、criteria 和 evidence 的决定 | 普通 agent 文本、投票或“大家同意”不能改变业务状态 |
| `StatusReport` | 更新进度、阻塞、预算和下一步 | 进度百分比、沉默或一条普通消息不等于完成/验收 |
| `Evidence` | 提交可验证的 artifact、receipt、测试和 revision 引用 | 模型自报、stdout 文字或空测试结果不能成为证据 |
| `Incident` | 触发隔离、升级、Recovery/Reconciliation | 事故通知本身不自动批准、重试、关闭或清除 Unknown |

Handoff 的责任状态由 ControlPlane 维护：`proposed → submitted → acknowledged / rejected / expired`；ACK 之后才允许接收方进入执行。通知的状态（sent/read/acked）不能替代该状态。所有消息正文进入模型上下文时必须经过 target run 的 scope、purpose 和预算检查，不能共享父 transcript 或建立自由 `SendMessage` 总线。

### 34.3 通知合同、Human Inbox 和 action 引用

```text
Notification {
  notification_id
  source_event_id
  source_cursor
  aggregate_type / aggregate_id?
  organization_id / project_id? / work_packet_id? / cell_id? / run_id?
  recipient_principal_id
  category       // approval | task | handoff | meeting | decision | status |
                 // evidence | incident | reminder | reconciliation
  severity       // critical | high | medium | low
  title
  redacted_summary
  payload_digest
  action_refs[]  // server-generated command/action IDs, not executable code
  due_at?
  expires_at?
  dedup_key
  template_version
  subscription_revision
  data_classification
  state          // unread | read | expired | withdrawn | superseded
  created_at
}

NotificationSubscription {
  subscription_id
  owner_principal_id
  scope                  // principal/project/session/run, server-derived
  category_filter[]
  minimum_severity
  channel_bindings[]
  digest_policy
  quiet_hours?
  revision
  authority_epoch
  expires_at?
}

DeliveryAttempt {
  attempt_id
  notification_id
  channel
  attempt_no
  delivery_key
  lease_id / fence
  status       // pending | claimed | submitted | delivered | acknowledged |
               // failed | expired | result_unknown | dead_letter
  provider_receipt?
  error_code?
  next_attempt_at?
  source_cursor
}
```

`HumanTask` 继续是 Approval、Review、Acceptance、Escalation 和 Reconciliation 的投影视图，不能另造一套审批状态。通知 action 只携带 `action_ref`、目标版本、payload digest、允许的命令名和所需字段；用户点击后回到原 ControlPlane 命令，重新检查 actor、project/session ownership、authority epoch、policy/gate、criteria revision、approval expiry 和 idempotency key。`read`、`acknowledge`、`snooze`、`escalate`、`delegate`、`withdraw` 只改变收件箱投影或追加治理事实，永远不直接批准副作用。

`HumanTask` 的动作状态与通知投递状态必须同时展示但不能合并：

```text
notification: unread → read → acknowledged (可选)
delivery:     pending → claimed → submitted → delivered/acknowledged
action:       offered → submitted → applied / failed / result_unknown
```

仅 `action.applied` 才表示原命令已被 ControlPlane 接受并完成相应事实转移；channel ACK、浏览器已读、桌面 toast 展示和消息回复都不具备该含义。

### 34.4 从事实到通知的处理流程

事件捕获、分类、投递和用户动作按下面顺序运行。提交顺序很重要：先保证事实和投影存在，再允许任何客户端看到“需要行动”。

```text
ControlPlane command / Runner result / Company transition
  → validate typed message or terminal fact
  → append RuntimeEvent with CAS, authority, causation and idempotency
  → EventStore returns Committed(source_cursor)
  → NotificationProjector consumes committed cursor (observer only wakes it)
  → classify event → recipient/scope/severity/template/action refs
  → persist Notification + outbox row + projector checkpoint atomically
  → DeliveryWorker claims outbox with lease/fence
  → channel adapter sends bounded redacted envelope with delivery_key
  → record submitted/delivered/acknowledged or result_unknown
  → emit committed delivery fact / update derived delivery projection
  → UI hydrates snapshot then applies cursor-ordered events
  → user action returns to ControlPlane with expected revision + idempotency key
  → append action result, wake original wait key, derive follow-up notification
```

`NotificationProjector` 必须同时支持扫描和提交后唤醒：提交 observer 丢失、daemon 重启或 projector 暂停时，从最后一个 durable `source_cursor` 继续扫描；不能依赖内存 callback 保证不丢。每个事件只按 `(source_event_id, recipient_principal_id, channel_policy_revision, template_version)` 生成一次 intent。若同一 dedup key 的 payload digest 不同，返回 `notification_dedup_conflict` 并暂停该 intent，不能静默覆盖旧通知。

通知规则首批至少覆盖：

| 来源事实 | 默认类别/严重度 | 默认收件人和动作 |
|---|---|---|
| `approval.requested` | `approval/critical` | server-derived approver；approve/deny/cancel action |
| `run.failed`、`run.result_unknown`、`capability.result_unknown` | `incident` 或 `reconciliation/high` | owner/Closer/Sponsor；inspect/reconcile/recover |
| `run.finished`、`run.completed` | `task/medium` | run owner；open receipt/review |
| `handoff.submitted`、`handoff.acknowledged/rejected` | `handoff/high` | 具名 recipient/original owner；ACK/inspect |
| Symposium/Meeting `symposium.invited`、`agenda.changed`、`meeting.started` | `meeting/high` | server-derived attendee；open agenda/参加/回执；不得让 Builder 越过参会边界 |
| `status_report.recorded`、`evidence.ready` | `status/evidence/medium` | packet owner/Reviewer；open evidence/review |
| Symposium/Company `decision.recorded` | `decision/high` | 受影响 owner/下一责任人；open decision/continue |
| lease expiry、workflow wait、deadline reminder | `reminder/low..high` | 当前责任人；renew/escalate/inspect |

Critical（审批、Unknown、安全事故、发布阻断）必须保留在 durable Human Inbox，并提供查询兜底；Medium/Low 可以按订阅的 digest/quiet-hours 合并，但合并必须保留全部 `source_event_id[]`、最早 due time 和每个 action ref。不能对所有 RuntimeEvent 无差别 fan-out。

### 34.5 实时、订阅和恢复语义

实时传输分为 durable inbox 与 ephemeral run stream 两条 lane：

1. 页面打开先以认证主体调用 snapshot/tail 查询，拿到 `source_cursor`、`projection_version`、`epoch` 和最近 N 条通知；随后再挂载订阅，或先挂载订阅再加载 snapshot 并合并 hydration 期间事件。实现必须保证两者之间的窗口不会丢事件。
2. 增量使用服务端 `source_cursor`/`event_id`/`sequence`，不能以客户端 timestamp 排序。相同事件按 event ID 去重；重放只更新投影，不重复创建通知、未读计数、toast 或外部发送副作用。
3. 订阅 envelope 至少带 `schema`、`epoch`、`source_cursor`、`notification_id`、`event_id`、`scope` 和 redacted payload。`run_stream` 的 delta 仍允许丢失；Terminal、ApprovalRequested、Incident 和 `run.finished` 必须由查询或回放补回。
4. 出现 cursor gap、broadcast lag、projection lag、epoch 变化、旧 socket 的迟到事件或 projection checksum 不一致时，客户端丢弃不完整增量，标记 `syncing/stale`，重新 hydrate；不能把 gap 当成“没有通知”。
5. SSE/WS/本地订阅状态区分 `connecting → syncing → live → stale → offline → incompatible`；断线重连使用 1 秒起、30 秒封顶、带抖动的退避，旧连接关闭不能覆盖新连接。断线不等于 cancel，窗口关闭也不隐式 resume/cancel。
6. `run.finished` 是唯一终局投影，至少含 `run_id/status/error/cancelled/final_assistant_text/receipt_ref`，由 ControlPlane 在持久终态事实之后生成且每个 run 恰好一次。现有 `RunStreamEvent::Terminal` 作为兼容 wire 投影，三种界面都以匹配 `run_id` 的终局信封对账结束。

### 34.6 投递、失败、取消和安全边界

默认投递语义是 at-least-once + receiver idempotency。每次发送前先持久化 `claimed`，发送使用稳定 `delivery_key`；得到明确的 channel receipt 后再记录 `delivered/acknowledged`。发送超时、断线、provider 返回不完整或“已发出但 delivery fact 写入失败”都只能标记 `result_unknown`，冻结该 attempt 并进入 reconciliation，不能盲目重试造成重复通知或重复动作。只有在确认尚未交给 channel 的本地失败（例如连接尚未建立、校验在发送前失败）时才允许按 bounded policy 重试。

取消和撤销也不能伪造成功：取消订阅只阻止后续投递，不撤销已经提交的业务事实；正在发送的通知无法确认是否到达时保留 `result_unknown`；撤销 assignment/authority epoch 后，未发送 outbox 失效，已发送记录仍可审计。过期通知追加 `notification.expired`，不删除原事件；撤回或 supersede 追加新事实，不能覆盖历史。

所有通道都复用现有 redaction、DataBoundary、ProjectTrust、Principal/assignment 和 authority epoch：

- in-app/Human Inbox 只查询服务端根据 principal 派生的范围；客户端传入的 owner/project/recipient/filter 只能收窄，不能扩大；
- 通知正文限制大小、字段和嵌套深度，secret、token、环境变量、原始 stdout/stderr、未脱敏 prompt 和外部账户凭据只允许 opaque ref/digest；
- `action_refs` 不能包含可执行脚本、任意 URL 或隐式 capability；点击 action 必须走已注册的 `CommandRequest`/HumanTask action；
- CLI/TTY、Web、Desktop 先实现本地/in-process、SSE 或既有 run stream；Email、Slack、HTTP webhook、A2A push 等外部投递只能作为 Connector 适配器的后续目标，在认证、allowlist、签名、nonce、超时、幂等和对账完成前保持 `not_supported`；
- webhook 若未来启用，配置必须绑定 `ProviderAccount/ConnectorBinding` 和 project scope，使用 URL allowlist/SSRF 防护、HMAC/JWT/mTLS、timestamp+nonce、防重放、2xx ACK、退避、dead-letter 和删除幂等，绝不从通知正文推断权限。

### 34.7 参考项目取舍与 Kiana 映射

| 参考来源 | 吸收的设计 | Kiana 适配/限制 |
|---|---|---|
| [统一 Agent 流程审计](reference-agent-audit/00-unified-agent-flow.md)、[DeepSeek Harness](reference-agent-audit/02-deepseek-harness.md)、[ADK Python](reference-agent-audit/16-adk-python.md)、[Goose](reference-agent-audit/07-goose.md) | append/commit 后才通知；non-partial 事件先持久化；inbox claim/ACK 可恢复；effect-before-event | `NotificationProjector` 只消费 committed cursor；partial delta 不改变 durable inbox；未知状态先对账 |
| [OpenCode](reference-agent-audit/08-opencode.md) | event projector 同时驱动持久化和 SSE；eager subscribe、heartbeat、disposed；per-session 串行 runner | projector 是事实投影而非 authority；订阅丢失可从 cursor 扫描，不能由 SSE 代替 EventLog |
| [OpenHands](reference-agent-audit/06-openhands.md) | REST 历史 + since 增量、事件 ID 去重、1–30 秒退避、旧 socket 隔离 | 使用 server sequence/cursor/epoch，不采用其 timestamp 排序或前端 metadata 作为权限 |
| [Crush](reference-agent-audit/11-crush.md) | RunID/terminal RunComplete、服务器先订阅再 POST、Flush 后发 terminal、queued/active cancel 区分 | terminal/approval/incident 必达并可 query；协作式 cancel 不等于 effect 已停止 |
| [Codex](reference-agent-audit/01-codex.md)、[Agent Framework](reference-agent-audit/15-agent-framework.md)、[Agno](reference-agent-audit/21-agno.md)、[Letta](reference-agent-audit/25-letta-code.md) | ServerRequest/notification 区分；可序列化 approval/requirement；OTID/run/seq、过期审批协调、重连恢复 | Approval/HumanTask 状态仍由 ControlPlane；不采用进程内 approval/snapshot 作为 durable 证据 |
| [Cline](reference-agent-audit/05-cline.md)、[Roo Code](reference-agent-audit/10-roo-code.md)、[Pi](reference-agent-audit/12-pi.md) | seq/epoch、UI/model history 分离、悬空工具补齐、流式与完整状态双通道 | 通知 action 不写入模型 transcript；完整状态从 EventLog/Receipt 重建 |
| [A2A](../reference/a2a/docs/topics/life-of-a-task.md)（仅字段/状态形状） | Message 与 Task 分离、snapshot-first、cursor stream、多订阅、at-least-once webhook、ACK/receipt 区分 | 不启用远程 transport、push notification 或 streaming；关键结果使用 Receipt/Artifact 查询兜底 |
| [ECC unified-notifications-ops](../reference/ECC/skills/unified-notifications-ops/SKILL.md)（目录级技能参考） | Capture→Classify→Route→Collapse→Attach action；severity、owner、primary/fallback、digest-first | 规则由服务端版本化；不得对所有事件广播；critical/Unknown 不能被 digest 隐藏 |
| [CompanyOS 设计](company-os-design.md)、[运行治理](company-os-operations-governance.md) | HumanTask 只做 Approval/Review/Acceptance/Incident 投影；owner、due/expiry、authority epoch、reconciliation 和 escalation 有明确状态机 | 通知只呈现治理事实；ACK/read/snooze 不改变 Approval authority；过期、撤销和 Unknown 追加事实并可对账 |
| MetaGPT、ChatDev、12-factor-agents | 结构化 sender/recipient、human input、stream end marker 的形状 | 不采用内存 Map、未认证 webhook、共享 ChatChain、无 cursor 重放或模型文本授权 |
| AutoGen、CrewAI、Agency Swarm | 编排/事件/流状态分离、checkpoint 和流 reconciliation | runtime subscription/checkpoint 失败不能吞掉；不引入自由消息总线或 SDK 外部权限中心 |

### 34.8 代码落点与依赖关系

| 层 | 代码职责 | 计划落点 |
|---|---|---|
| Domain | `CommunicationMessage`、`Notification`、`NotificationSubscription`、`DeliveryAttempt`、severity/category/state、dedup/TTL/value bounds | `kiana-domain/src/notifications.rs`、`messages.rs`、`contracts.rs` |
| Ports | Event cursor reader、NotificationStore、SubscriptionStore、DeliveryChannel、Clock、Receipt/Reconciliation 接口 | `kiana-ports/src/notifications.rs`（或等价 module） |
| Core | typed message command、recipient/scope resolver、event classifier、action authority、HumanTask/notification projection、epoch/revocation | `kiana-core/src/notifications.rs`、`platform.rs`、`events.rs`、`recovery.rs` |
| EventLog/Data | source cursor/checkpoint、notification projection/outbox 原子写、attempt lease/fence、rebuild、retention | `kiana-eventlog/src/notifications.rs`，复用 PD-05..PD-13 的 store/transaction 原语 |
| Daemon | committed-event wake adapter、DeliveryWorker、in-process channel、run stream bridge、shutdown/drain | `kiana-daemon/src/notification_projector.rs`、`notification_worker.rs`、`run_stream.rs` |
| Protocol/Client | snapshot/page/subscription envelope、action/read/ack DTO、epoch/cursor/gap/error codes | `kiana-protocol/src/lib.rs`、`kiana-client/src/lib.rs` |
| Entrypoints | CLI/TTY inbox 与 ACK、Web REST+SSE hydrate/reconnect、Desktop OS notification permission | `kiana-entrypoints/src/cli.rs`、`workbench_chat.rs`、`web.rs`、`contrib/desktop/` |
| Connector（后续） | webhook/email/chat/外部推送认证、allowlist、provider receipt/reconcile | 复用 `kiana-daemon/src/connectors.rs`；未满足安全合同前保持 `not_supported` |

端口应把“读投影”“写治理动作”“产生外部投递”拆成不同接口，避免一个宽泛的 `send_notification` 同时拥有查询、授权和副作用权限。实现可以按下面的 Rust 形状落地；具体存储类型由 `PD-*` 决定：

```rust
trait NotificationStore {
    async fn page(
        &self,
        principal: PrincipalId,
        scope: NotificationScope,
        after: SourceCursor,
        limit: NonZeroUsize,
    ) -> Result<NotificationPage, NotificationReadError>;

    async fn apply_read_state(
        &self,
        principal: PrincipalId,
        notification_id: NotificationId,
        expected_revision: ProjectionRevision,
        action: ReadStateAction,
        idempotency_key: IdempotencyKey,
    ) -> Result<ReadStateReceipt, NotificationWriteError>;
}

trait NotificationProjector {
    async fn project_committed(
        &self,
        event: CommittedRuntimeEvent,
    ) -> Result<ProjectedNotifications, ProjectionError>;
}

trait DeliveryChannel {
    async fn submit(
        &self,
        envelope: RedactedNotificationEnvelope,
        delivery_key: DeliveryKey,
    ) -> Result<ChannelReceipt, ChannelError>;
}
```

`NotificationProjector::project_committed` 必须在同一投影事务中写入通知、outbox intent 和 checkpoint；`DeliveryChannel::submit` 只接受已 claim 的 intent，不能调用 `ControlPlane` 或 Broker。所有入口的 `page` 与 `apply_read_state` 都由 DaemonHost 注入主体和服务端 scope，不能让客户端直接构造 `PrincipalId`、recipient 或 channel。

### 34.9 详细实施步骤（NM-00–NM-22）

每一步都执行“先拒绝、再成功、最后回归”。下表的测试名是验收目标，不表示当前已经存在或通过；完成后必须在 `CURRENT_STATUS.md` 写 `source_snapshot / worktree_status / command_argv / cwd·environment / fixture·cassette / exit_code / status change / proof-level change / limitations / reviewer`。

| Step | 代码与交付物 | 依赖 | 先拒绝的验收 | 成功与回归验收 |
|---|---|---|---|---|
| <a id="step-nm-00"></a>`NM-00` | 现状/事件种类/入口 inventory；`run_stream.rs`、`platform.rs`、`web.rs`、`workbench_chat.rs`、`CURRENT_STATUS.md` | — | 证明当前没有 durable notification bus/read state；列出任何把 transcript/UI event 当事实的路径 | 产出 event→recipient→channel matrix、owner、proof ceiling 和迁移清单，不改历史证据 |
| <a id="step-nm-01"></a>`NM-01` | Domain contracts 与 schema registry；Message/Notification/Subscription/Attempt/ActionRef/DeliveryReceipt | NM-00 | unknown kind/schema、空 recipient、scope 超集、超长正文、错误 TTL、secret in `Debug/Serialize` fail-closed | serde/canonical bytes、状态转移、兼容 upcast、bounded value 和 digest 测试 |
| <a id="step-nm-02"></a>`NM-02` | 七类 `CommunicationMessage` 命令与生命周期；`kiana-domain`/`kiana-core` | NM-01、P1-E-01 | Chat/StatusReport/Evidence 文本不能授予 Grant；Handoff 无 recipient、ACK 前 dispatch、Command 伪造 actor/role 全拒绝 | 定向 Handoff ACK/reject、Decision authority/evidence、Incident escalation 均写 committed facts；不进入共享 transcript |
| <a id="step-nm-03"></a>`NM-03` | Event kind registry 与分类规则；`kiana-domain/contracts.rs`、`kiana-core/events.rs` | NM-01、ER-01 | 未注册事件、伪造 source event、模型/UI 自报 approval/completion、无 owner 的 critical event 拒绝投影 | approval/run terminal/unknown/handoff/status/evidence/incident/reminder 映射确定且可版本化 |
| <a id="step-nm-04"></a>`NM-04` | `NotificationProjector` + source cursor/checkpoint；`kiana-eventlog`/`kiana-core` | NM-03、PD-05..PD-09 | projector 读空、cursor 回退、gap、checksum 错误、pre-commit observer 不能产生通知 | committed-only、分页扫描、重启从 cursor 补齐、同一事件重放字节等价；observer 仅作 wake hint |
| <a id="step-nm-05"></a>`NM-05` | recipient/scope/subscription resolver；Principal/Assignment/ProjectTrust/authority epoch | NM-01、NM-04、CI-05 | 客户端 owner/project/recipient/filter 扩权、跨项目、revoked/expired epoch、未信任项目、role 自报全拒绝且无投递 | 同一主体三入口得到相同可见集合；订阅只能收窄服务端最大 scope，revision/expiry 可重建 |
| <a id="step-nm-06"></a>`NM-06` | Notification materializer 与 HumanTask bridge；`kiana-core/platform.rs`、`kiana-domain/platform.rs` | NM-04、NM-05、P2-K3-01 | approval/review/acceptance/incident/reconciliation 缺 source/event/evidence/due 或错误 decider 不可见 | 每个可行动事实有 redacted summary、action refs、due/expiry、source cursor；HumanTask 权威状态仍来自原对象 |
| <a id="step-nm-07"></a>`NM-07` | dedup/idempotency/OCC；dedup key、content hash、subscription revision | NM-04、NM-05 | 同 key 不同 digest、重复 claim、旧 revision、重放/多进程竞争不能多发或覆盖 | at-least-once 输入折叠为一次 intent；重复请求返回原 notification/receipt；并发 CAS 只有一个赢家 |
| <a id="step-nm-08"></a>`NM-08` | durable outbox + DeliveryWorker；attempt lease/fence、shutdown drain | NM-06、NM-07、PD-10..PD-13 | outbox claim 后 crash、lease 过期、revoked subscription、队列超限不能丢 critical/terminal/approval，也不能重复动作 | pending→claimed→submitted→ack/failed/unknown 可重建；重启继续安全处理，best-effort 丢弃有理由和计数 |
| <a id="step-nm-09"></a>`NM-09` | in-process/in-app channel；NotificationStore query/page | NM-06、NM-08 | 越权查询、无限 page、projection unavailable 当空、已读当批准、删除历史事实全拒绝 | `list/mark_read/ack` 只写投影/治理事实；empty 与 unavailable 区分；同 cursor 重算一致 |
| <a id="step-nm-10"></a>`NM-10` | action refs 与 HumanTask action command；审批、ACK、review、reconcile、snooze/escalate/delegate/withdraw | NM-06、NM-09、CP-18/19 | stale target、wrong decider、payload/criteria/epoch drift、double consume、read/ACK 伪造 approval、未知 action 全拒绝且 zero broker effect | action 提交回原 ControlPlane；一次决定唤醒一个 wait key；Applied/Failed/Unknown 与 delivery ACK 分开 |
| <a id="step-nm-11"></a>`NM-11` | unread/read/ack/snooze/digest 投影和排序；server time、due/urgency | NM-09、NM-10 | 客户端时间/预选按钮/沉默改变优先级或批准；snooze 隐藏 critical/Unknown；排序泄露他人范围 | urgency→due→source sequence 确定排序；已读跨重启保留；关键项始终可查询 |
| <a id="step-nm-12"></a>`NM-12` | durable inbox rebuild、retention/withdraw/supersede；`NotificationProjector`/PD-24..PD-26 | NM-04、NM-09、P2-K7-01 | 删除只删 UI/cache、retention 越权、withdraw 覆盖历史、payload ref 与 audit metadata 混淆 | source cursor 重建同一未读/状态；过期/撤回只追加事实；数据 epoch 传播到摘要、artifact、index、cache |
| <a id="step-nm-13"></a>`NM-13` | run stream/notification bridge；snapshot-first、after cursor、epoch、gap、heartbeat/disposed | NM-04、NM-08、UI-17/18 | lagged/old epoch/foreign run/late socket、delta gap 被当 completed、terminal 丢失无告警 | 先订阅再 hydrate（或保留合并窗口）；按 ID 去重；`run.finished`/approval/incident 可 query 回放；delta 仍是易失展示 |
| <a id="step-nm-14"></a>`NM-14` | CLI/TTY inbox 与运行状态；`cli.rs`、`workbench_chat.rs` | NM-09、NM-10、NM-13、UI-12/15 | `/approve` 伪造 scope、读已读即消费、错 session action、连接断开隐式 cancel/resume 全拒绝 | `/inbox`、`/approvals`、ACK/decision、retry/reconcile 共用协议；多会话徽标按服务端 projection 渲染 |
| <a id="step-nm-15"></a>`NM-15` | Web REST snapshot/page + SSE；`web.rs`/`kiana-client` | NM-09、NM-10、NM-13、UI-16/17/18/19/21 | wrong Host/Origin/bearer、cross-session/project、旧 cursor、无限 body、旧 socket 覆盖新状态全拒绝 | loopback-only；snapshot + after cursor hydrate、1–30 秒退避、connection/auth/conversation 错误分层；跨 tab action CAS 一致 |
| <a id="step-nm-16"></a>`NM-16` | Desktop local notification adapter；OS permission、tray、close/detach | NM-15、UI-24/25/26/27 | OS toast 泄露 private payload、托盘越权 command、close 隐式 resume/cancel、无权限仍声称 delivered | 仅 redacted title/summary；系统拒绝权限不影响 durable inbox；reopen 后从 cursor/receipt 恢复 |
| <a id="step-nm-17"></a>`NM-17` | severity/digest/reminder/escalation；primary/fallback、quiet hours、deadline | NM-06、NM-11、K3/CO-39 | digest 吞 critical/Unknown、模型自报恢复、重复提醒风暴、自动 approve/retry/close 全拒绝 | high/critical 立即/同日提醒，medium digest，low 可抑制；每次折叠带 source IDs、owner、next action 和 escalation 事实 |
| <a id="step-nm-18"></a>`NM-18` | cancellation/revocation/expiry/reconciliation；`recovery.rs`、`connectors.rs` | NM-08、NM-10、P2-K6-01 | cancel_requested 写成 delivered、in-flight 到达未知却重发、authority revoke 后继续发送、Unknown 自动 retry 全拒绝 | known pre-send failure bounded retry；send/ack 不确定进入 dead-letter/reconcile；新授权和新 delivery key 才能重试 |
| <a id="step-nm-19"></a>`NM-19` | 外部 Connector/webhook contract（默认关闭）；签名、allowlist、nonce、provider receipt | NM-08、NM-18、P4-K8-01 | HTTP/A2A push、任意 URL、SSRF、无 auth/重放/超时/幂等、外部 ACK 伪造应用全拒绝 | fake connector 只验证 envelope/receipt/reconcile；真实外部通道仅显式 opt-in，未验收保持 `not_supported` |
| <a id="step-nm-20"></a>`NM-20` | fault injection 与容量/安全测试；EventLog/Projector/Worker/Channel/UI 全链 | NM-04、NM-08、NM-13、NM-18 | duplicate/out-of-order/gap、crash windows、slow consumer、queue full、disk full、secret sentinel、projection loss 不得丢关键事实或产生 effect | bounded queue/backpressure、checkpoint/rebuild、redaction、lease reclaim、multi-subscriber isolation、critical delivery query fallback |
| <a id="step-nm-21"></a>`NM-21` | 跨入口 E2E 与消息/通知/动作对账；CLI/TTY/Web/Desktop、fake model/provider | NM-14、NM-15、NM-16、NM-20、CO-39/40/41 | 不同入口各自 bus/loop、一个入口已读影响另一个权限、消息文本改变 run、terminal/approval/incident 丢失全阻断 | 同一 EventLog/source cursor 在四入口产生同一 inbox/action/result；fresh process 重建未读、pending、delivery 和 terminal |
| <a id="step-nm-22"></a>`NM-22` | 发布门、证据和 `CURRENT_STATUS` 回填；feature/proof 分离 | NM-01..NM-21、ER/PD/UI 相关门 | 只有类型/单测/内存 broadcast/一次 toast 不能宣称 durable/live；unknown、audit 写失败或 scope 不确定阻断发布 | 每步有拒绝+成功+恢复证据；in-app/run stream 可按实际 proof 标记，外部 push 只有独立 live 证据才能提升 |

### 34.10 依赖批次

```text
Wave A — Contracts:     NM-00 → NM-01 → NM-02 → NM-03
Wave B — Projection:     NM-04 → NM-05 → NM-06 → NM-07
Wave C — Delivery:       NM-08 → NM-09 → NM-10 → NM-11
Wave D — Durability:     NM-12 ∥ NM-13 → NM-17 → NM-18
Wave E — Surfaces:       NM-14 ∥ NM-15 → NM-16
Wave F — External/UAT:   NM-19 → NM-20 → NM-21 → NM-22
```

与已有路线的接点：`P1-E-01` 提供七类通信语义；`P1-E-02`/`P4-E-03` 提供 Symposium/Decision 事实；`P0-F-02`、`P2-K3-01` 和 `CO-39` 提供 HumanTask/Approval 投影；`ER-06..ER-18` 提供 commit observer、terminal 和 Unknown 语义；`PD-05..PD-13` 提供 cursor、checkpoint、lease 和原子投影；`UI-17..UI-21` 提供 Web hydration/SSE/Human Inbox；`UI-26` 提供 Desktop 仅展示的 OS notification；`P4-K8-01` 之后才允许评估外部 Connector。任何步骤都不得把 `RunStreamBus` 升格为事实源，也不得把通知 worker 变成第二 Agent loop 或权限中心。

### 34.11 最低验收矩阵

| ID | 场景 | 必须断言 |
|---|---|---|
| `MSG-01` | 七类消息边界 | Chat/StatusReport/Evidence/Incident 不授予权限；Command/Handoff/Decision 必须回 ControlPlane；Handoff 未 ACK 不派发 |
| `NTF-01` | 事件到通知 | 只有 committed EventLog 产生通知；pre-commit/未注册/伪造 source 不可见；projector 可从 cursor 重建 |
| `NTF-02` | scope/recipient | 错 principal、project、session、role、epoch、subscription scope 超集全部拒绝，Broker/handler effect 为零 |
| `NTF-03` | dedup/replay | 相同 event/recipient/policy 只产生一个 intent；同 key 不同 digest 报 conflict；乱序重放不重复 toast/unread/delivery |
| `DEL-01` | outbox/lease | claim、crash、lease reclaim、shutdown、queue full 不丢 critical/terminal/approval；只有一个有效 sender |
| `DEL-02` | delivery Unknown | timeout、late ACK、写 receipt 失败、断线不能标 delivered；进入 `result_unknown`/reconcile，禁止盲重试 |
| `INBOX-01` | HumanTask/read state | read/ACK/snooze/delegate 不等于 approve；stale target、wrong decider、double consume 被拒；重启可重建 pending/read |
| `ACT-01` | action command | action ref 过期、digest/criteria/authority drift、未知命令、客户端伪造 owner 全拒绝且无 effect |
| `STREAM-01` | 实时桥 | snapshot+cursor hydrate、gap/lag/epoch、旧 socket、heartbeat/disposed、重连退避均可证明；delta 丢失不伪造完成 |
| `TERM-01` | 终局信封 | 每个 run 恰好一个匹配 `run.finished`/Terminal；最终文本与 Receipt 对账；缺终态返回 unavailable/Unknown，不自动完成 |
| `PRIV-01` | 脱敏/保留 | secret/prompt/raw output/foreign project 不进入通知、digest、toast、SSE、webhook、error；withdraw/retention 不删除原事实 |
| `EXT-01` | 外部通道 | 未认证、非 allowlist、重放、非 2xx、SSRF、provider receipt 缺失保持 `not_supported`/Unknown；无外部副作用 |
| `E2E-01` | 四入口一致性 | CLI/TTY/Web/Desktop 只能投影同一 EventLog/Notification snapshot；入口断开、重启、多 tab 不产生第二 bus 或权限并集 |

### 34.12 当前状态与限制声明

本专项追加的是设计和实施顺序，所有 `NM-*` 初始为待实施/待核验。当前可声称的只有：进程内 `RunStreamBus` 的局部增量展示和有限 terminal replay、`HumanInboxItem` 的局部查询投影、以及既有 Approval/Company/Recovery 事实的展示路径。当前不能声称 durable NotificationStore、跨进程未读/订阅恢复、可靠 outbox/DeliveryReceipt、OS 通知送达、Email/Slack/Webhook/A2A push 或现实业务消息已送达。

完成任何步骤时，必须分别更新对应的 `P1/P2/P4` 原单元、`CURRENT_STATUS.md` 证据块和本专项的状态；`Notification` 结构体存在、一个 SSE 客户端收到了 toast、客户端显示已读、或 channel 返回 2xx，都不足以提升 `feature_status` 或 `proof_level`。只有事实可重放、权限可重验、关键通知可查询补回、Unknown 可对账、所有入口共用同一 DaemonHost/ControlPlane，才允许推进相应退出条件。

---

<a id="integrations-connectors-plan"></a>

## 34-A. 集成与连接器专项：实际设计、处理流程与详细实施步骤（2026-09-14 追加）

> 本专项补全 [`module-map.md`](module-map.md) 第 13 模块和 `P4-K8-01`。完整调研、代码落点、拒绝优先验收和 `INT-00`–`INT-33` 实施卡见[独立专项文档](roadmap/integrations-connectors.md)。本节保留可扫描的架构结论、现状边界和执行索引，不把参考项目能力或当前 WIP 写成已交付。

### 34-A.1 现状和边界

当前源码只证明本地 `local_fixture` connector：`connector.manage`/`connector.invoke` 经过 `DaemonHost → ControlPlane → Policy/Gate/Approval → Capability Broker → EventStore`，binding、operation、scope、idempotency、rate limit、`ProviderReceipt` 和 `reconcile` 有局部代码；结果明确标记 `external_effect_performed: false`。HTTP MCP、真实 GitHub/Jira/Slack/Notion、支付/退款、远程账户、企业租户和 physical effect 仍是 `not_supported` 或后续目标。

本专项必须保持的分类：

| 对象 | 事实 | 不能替代 |
|---|---|---|
| Provider | 模型 endpoint、模型凭据、流式和用量 | Kiana Principal、Connector account 或项目授权 |
| Connector | 外部业务系统的 definition、account binding、operation、scope 和 effect receipt | MCP transport、Provider policy 或 Company Acceptance |
| MCP | 工具/资源的协议和 transport；当前仅 stdio 已支持 | connector scope、审批、idempotency 和外部 outcome |
| A2A | 异步 Agent Task、Artifact、status 和 push notification | 本地 capability permit 或 Kiana approval |
| Webhook/Trigger | 认证事件入口和 occurrence | 直接执行模型、Broker 或外部副作用 |

参考结论来自 Codex connector directory/account-scoped cache、OpenCode integration/credential/policy、Cline OAuth/MCP、OpenHands read-only credential probe/redaction、MCP capability handshake、A2A task/webhook/auth semantics、12-Factor pause/resume、Goose/Cline/Roo/Crush/OpenCode 生命周期审计和 Temporal/LangGraph 的 deterministic effect boundary。详细源码路径和取舍见独立文档 §2。

### 34-A.2 目标主链和数据合同

```text
operator / Harness / Workflow / verified webhook
  → versioned CommandIntent
  → DaemonHost authenticated context
  → ControlPlane resolves definition + binding + operation
  → canonical payload/schema/data boundary validation
  → policy + scope + budget + rate/concurrency + approval
  → commit Invocation reservation + action digest + idempotency key
  → issue one-shot CredentialLease at effect boundary
  → re-check authority/config/binding/credential/data epoch
  → prepared permit → Capability Broker → ConnectorAdapter
  → ProviderReceipt / EffectObservation / StopReport
  → append connector facts before result delivery
  → Receipt projection / Human Inbox / reconciliation
```

最低对象集合：`ConnectorDefinition`、`ConnectorOperation`、`AccountBinding`、`ProviderAccount`、`SecretRef`、`CredentialLease`、`ConnectorInvocation`、`Attempt`、`ProviderReceipt`、`EffectObservation`、`ReconciliationCase`。这些对象必须拥有稳定 ID、schema/version、owner project、scope、data class、purpose、revision/generation 和 source cursor；不可序列化 secret 原值。

风险由服务端 operation contract 派生：R0 list/inspect/health，R1 只读查询，R2 跨边界读取/导出，R3 create/update/send，R4 delete/payment/production。R3 必须对最终 payload 做一次确认，R4 默认拒绝；调用者不能通过参数把写操作降级为只读。

### 34-A.3 关键处理规则

1. **注册/绑定**：definition 版本不可原地覆盖；签名/hash、schema、adapter 能力、scope 和 data boundary 验证通过后才可进入 registry。Binding 绑定 `connector_id + version + provider_account + project + scope + endpoint digest`，撤销/轮换递增 revision/generation。
2. **凭据**：Core、Runner、EventLog、Receipt、Memory、UI 和子 Cell 只看 `SecretRef`、presence、generation、expiry 和 digest；只有 CredentialStore/Broker/transport 在最后边界解析 raw secret。OAuth 使用 PKCE/state/callback anti-CSRF、提前刷新、single-flight 和 generation CAS。
3. **调用**：payload 先 canonicalize 再算 digest；idempotency key 绑定 project、binding、operation 和 payload。先提交 reservation/permit，再允许 adapter；任何 revision/epoch drift、过期 approval/lease 或 scope 超集都必须 0 effect。
4. **回执**：已知成功/失败和 `result_unknown` 分开。没有 provider receipt/query/idempotency 的写操作不能声称 no-effect；Unknown 进入 RecoveryCase，人工或 provider observation 只能追加 `connector.reconciled`，不能改写原事件。
5. **入站**：Webhook/A2A 先验证认证、来源、时间戳、签名、nonce、版本和 payload schema，再写 `connector.event_received`/trigger occurrence；payload 只能成为 typed input artifact，不能直接成为未经 schema/policy 检查的 capability 参数。
6. **取消/恢复**：取消是意图；已启动 adapter 需要 StopReport，未确认的 effect 保持 Unknown 并继续 fencing。daemon 重启默认 Paused/NeedsRecovery，显式 reconcile/resume/retry_without_effect 才能重新授权。
7. **数据治理**：结果带 DataClass/Purpose/owner/source/retention；跨项目必须有 SharingGrant。撤销/删除先写 tombstone、提升 data epoch，再失效 Memory/Index/Cache/Artifact。

### 34-A.4 代码接线

| 层 | 主要落点 | 接线要求 |
|---|---|---|
| Domain | `kiana-domain/src/connectors.rs`、`capabilities.rs`、`governance.rs` | 纯合同、状态、digest 和错误；不访问网络/文件/Tokio |
| Protocol | `kiana-protocol/src/lib.rs` + schema fixtures | `connector.manage/invoke/health/reconcile` versioned DTO；服务端覆盖 caller authority |
| Ports | `kiana-ports/src/lib.rs` | `ConnectorAdapter`、`CredentialProbe`、`EffectObserver`、`WebhookVerifier` 窄接口 |
| Core | `kiana-core/src/connectors.rs`、`approvals.rs`、`recovery.rs` | normalize、risk/policy/approval、reservation/CAS、fence、receipt/reconcile |
| Broker | `kiana-capability-broker/src/lib.rs` | 只消费 prepared permit；不接受公开 authorization 字符串或模型自造 binding |
| Daemon | `kiana-daemon/src/connectors.rs`、`mcp_stdio.rs`、`execution_control.rs` | fixture/MCP/未来 HTTP adapter、credential lease、bounded worker 和 shutdown |
| Event/Query | `kiana-eventlog`、`kiana-query` | facts、dedup、projection、health、Receipt、可重建 index/cache |
| Entrypoints/UI | CLI/Web/Workbench/MCP adapters | 只转发命令、显示状态和 Human Inbox；不运行第二 loop 或直接调用 adapter |

### 34-A.5 详细步骤索引（完整验收见独立文档 §7）

| 波次 | Steps | 交付目标 |
|---|---|---|
| A 契约 | `INT-00 → INT-04` | 基线、边界、typed IDs、operation schema、registry CAS |
| B 绑定 | `INT-05 → INT-09` | account/project scope、SecretRef/Lease、adapter ports、fixture、只读 probe |
| C 传输 | `INT-10 ∥ INT-11 → INT-13` | stdio MCP、HTTPS/SSRF 边界、OAuth PKCE、全通道脱敏 |
| D 授权 | `INT-14 → INT-18` | protocol normalize、risk/approval、reservation、配额和 effect-time fencing |
| E 执行 | `INT-19 → INT-23` | Broker dispatch、ProviderReceipt、retry、Unknown/reconcile、cancel/stop |
| F 入站 | `INT-24 → INT-28` | Webhook/A2A、mapping/provenance、data governance、通知和四入口查询 |
| G 收口 | `INT-29 → INT-33` | restart/recovery、conformance、只读 pilot、受控写 pilot、发布证据 |

步骤执行顺序固定为“先拒绝、再成功、最后回归”。最小拒绝集包括：伪造 actor/role/risk/binding、未 trust、跨项目无 grant、unknown schema、scope 超集、raw secret 泄漏、OAuth state 错误、HTTP MCP、SSRF/redirect、重复 key 冲突、超额/旧 fence、过期 approval/lease、Unknown retry、cancel 未 stop、late result、损坏 journal、撤销后 cache 命中和 webhook 重放；每项都必须证明没有 Broker/adapter effect。

成功集包括 local fixture、stdio MCP、read-only probe、idempotent replay、R3 final payload、provider receipt/query reconcile、webhook occurrence、取消/停止、重启显式恢复、数据撤销和 CLI/Web/Workbench/MCP 同一 projection。真实服务按 connector/operation/account 单独 opt-in。

### 34-A.6 发布门和状态回填

`P4-K8-01` 只有在 `INT-00..33` 的相关步骤具备 `CURRENT_STATUS.md` 证据块后才能提升状态。证据必须包含：

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

`feature_status` 与 `proof_level` 分开记录；fixture receipt 只能证明 `source`/`local_behavior` 的事实链，不能证明 live/physical 或外部业务 outcome。真实 OAuth、HTTP、支付、退款、生产发布和跨系统回滚若要开启，必须另有环境、账号隔离、provider receipt、对账、清理和回滚证据；本专项不改变冻结项，也不自动开放这些能力。

<a id="quality-evaluation-design"></a>

## 34-B. 评测与质量专项：实际代码设计、处理流程与详细实施步骤（2026-09-14 追加）

> 本节补全 [`module-map.md`](module-map.md) 第 17 模块，并展开既有 `P1-L1-01 EvalSuite 与 GoldenTrace`。它不改变 P0–P6 的 canonical 编号，也不把现有 `kiana eval` 的局部能力写成完整质量平台。本文档前面的旧限制性文字若与本次追加设计冲突，以当前任务和本节为准。

### 34-B.1 当前代码基线与问题分类

当前可确认的源码事实如下：

| 位置 | 已有实现 | 本节要解决的缺口 |
|---|---|---|
| `kiana-commands/src/eval.rs` | `kiana.eval-suite.v1`、`kiana.eval-report.v1`、`kiana.eval-baseline.v1`；支持 `runtime_event_replay` fixture、事件/工具/usage/final 状态断言、baseline 阈值和 `--fail-on-failure` | Eval schema 仍是命令层私有 DTO；没有 `EvalCase` 与 Run/Receipt/Artifact 的统一关联，也没有 GoldenTrace、候选版本或质量门裁决 |
| `kiana-commands/tests/eval_command.rs`、`kiana-entrypoints/tests/cli_eval.rs` | 已覆盖正常、失败、baseline 回归、非法 baseline 和真实 binary 路由 | 没覆盖副作用隔离、权限拒绝、Unknown/取消/恢复、事件序列差异、敏感数据泄露和跨 provider 重放 |
| `scripts/release-smoke.sh` | 已有离线 eval smoke，创建临时 suite/baseline/JSONL fixture 并校验 report schema/status | smoke 是发布脚本中的单个检查函数，不产生可持久化的 EvalRun、证据包或 Promote 阻断原因 |
| `kiana-core/src/events.rs`、`receipts.rs`、`projection.rs`、`recovery.rs` | EventLog、Receipt、投影、恢复材料和 `redact_event_value` 已有局部路径 | 没有统一 TraceNormalizer；RuntimeEvent、Artifact、Receipt、EvalResult 之间没有稳定 correlation/causation 索引 |
| `kiana-daemon` 的 fake/cassette/model adapter | 有 fake model、cassette 和多个 smoke fixture | fixture 不能统一描述 initial state、expected events、forbidden effects、artifact assertions、版本快照和故障注入 |
| `.github/workflows/`、`scripts/` | 有 workspace、focused test、golden、workbench、release smoke | 没有 PR 快速层、nightly 深层、release Promote 层的明确分工，也没有失败分类、flake quarantine 和证据归档合同 |

因此，`P1-L1-01` 应拆成“质量内核 + 执行适配 + 证据/CI”三层。评测本身是只读消费者；只有现有 `ControlPlane` 才能启动一个被评测的运行，质量门只能提交 `quality.*` 事实，不能直接改变 Grant、Policy、Approval、EventLog 历史或外部系统状态。

### 34-B.2 参考项目调研与采用取舍

| 参考 | 观察到的机制 | Kiana 采用 | 明确不复制 |
|---|---|---|---|
| OpenAI Evals | registry 中的数据集、YAML 参数、basic/model-graded eval、可复现实验 | 用版本化 suite/case registry；允许纯代码 evaluator 和受控的可选 judge；每次运行固定 model/prompt/tool 版本 | 不把在线 API key 或第三方 dashboard 作为本地质量门依赖；LLM judge 不负责安全授权 |
| LangSmith | Dataset/Example、target function、Evaluator、Experiment 分离；离线有 reference output，在线以 run/thread 为对象 | 将 `EvalDataset`、`EvalTarget`、`Evaluator`、`EvalExperiment` 分离；同一 suite 可比较多个 candidate；结果按 case/run 保留 | 不引入 hosted tracing、跨租户数据上传或把 UI 实验表当事实源 |
| Promptfoo | provider matrix、声明式 assertions、红队/安全测试、阈值和失败报告 | 支持 provider/profile matrix、结构/正则/脚本断言、forbidden effect 和 red-team case；断言失败输出稳定 issue code | 不让 prompt 声明、`allowed-tools` 或 judge 输出直接扩大能力；不把单一文本分数当质量总判定 |
| Beads Oracle A | reference/candidate 双 binary、真实进程场景、环境清理、JSON-aware diff、volatile normalization、curated/deep 双层、golden provenance | 引入 reference/candidate replay、显式 env allowlist、规范化规则、in-scope predicate、快速 curated 与 nightly deep tier | 不把所有输出都强行 byte-equal；不忽略未覆盖范围，report 必须列出 out-of-scope 与 no-golden |
| DeepSeek Harness | 生产入口上的 deterministic snapshot、独立 benchmark worker、合成历史、replay 模式、计时与平台预算 | fixture 使用真实 DaemonHost/ControlPlane seam；性能工作在隔离 worker；snapshot 与 replay 明确分开 | 不复制产品算法或为 benchmark 暴露生产专用 export；性能完成不替代语义验收 |
| Graphify | 数据集、judge/grading、fairness rules、成本统计、可复现命令和结果报告 | 每个 suite 带 dataset provenance、judge 版本、抽样/公平性规则、成本/延迟桶和 reproduction command | 不以未经审查的外部数据或黑盒 judge 作为唯一 Promote 依据 |
| Aider | repo map、任务 benchmark、历史 replay、结果随时间绘图 | 将代码知识 snapshot、任务输入和历史 baseline 作为 EvalCase 输入；记录时间序列趋势 | repo map 仍是上下文数据，不是权限或事实账本 |
| ECC evaluator/RAG prototype | trace/report/candidate-playbook/verifier 五件产物；retrieve 与 action 分离；候选必须被 verifier 接受 | 采用 `EvalTrace`、`EvalReport`、`QualityCandidate`、`QualityVerdict` 的分离和只读诊断→候选→验证→晋级流程 | 不在 evaluator 中 merge、publish、修改配置或执行外部副作用 |
| OpenCode / OpenTelemetry | durable aggregate sequence、replay-and-tail cursor、ephemeral delta 不进 durable cursor；统一 span/metric 语义 | Eval 只消费 durable event cursor；stream chunk 默认聚合成 turn/span；指标字段按稳定命名和白名单记录 | 不把 token delta 或 UI timeline 当可重放事实；不记录 secret、完整 prompt 或隐藏推理 |
| Pydantic AI / Temporal / LangGraph | graph/state replay、checkpoint、activity/step 边界、reference output 和恢复测试 | 对 Workflow/Swarm 使用已存在的事件和 snapshot，评测按 node/attempt/cursor 比较；故障注入验证 Unknown 和恢复 | 不引入 hosted workflow server、任意 Python workflow 或自动 retry 覆盖 Kiana 的 Unknown 规则 |

### 34-B.3 质量系统的边界与不变量

质量循环固定为：

```text
observe durable facts
  → select version-pinned dataset/case
  → admit read-only eval experiment
  → build isolated target context
  → run fake/replay/shadow target through DaemonHost
  → normalize trace and collect artifacts/receipt
  → deterministic safety + contract evaluators
  → optional semantic evaluator (never an authority source)
  → aggregate scores and classify findings
  → compare baseline and detect divergence
  → QualityGate pass/reject/needs_shadow/rollback
  → append verdict/evidence; only an authorized promotion command changes route
  → monitor drift and generate candidate feedback
```

必须长期保持以下不变量：

1. **评测只读**：Eval target 可以调用 fake broker、临时 workspace 和录制 provider；任何真实网络、支付、消息发送、发布、设备操作和未批准写盘都必须被替换为 deny adapter。
2. **安全优先**：`forbidden_effect`、policy bypass、secret leak、evidence missing、replay divergence、fixture/schema integrity failure 任一发生，直接 `reject`；不能用文本质量、低成本或低延迟抵消。
3. **事实可追溯**：每个 case 结果都绑定 `suite_digest`、`case_digest`、target 版本、source snapshot、fixture digest、event cursor、artifact hash、receipt hash 和 evaluator/gate 版本。
4. **版本不可漂移**：model、prompt、tool catalog、memory snapshot、workflow、policy epoch 和 evaluator 都是显式输入；缺失版本或 unknown schema 只能 fail-closed。
5. **baseline 不等于真理**：baseline 是被接受的比较对象，不是授权依据；baseline 本身必须有 provenance、维护者和过期/重建策略。
6. **精确比较有边界**：稳定 ID、状态、错误码、工具名、事件类型和 digest 可 exact；时间、随机 ID、路径临时目录、host 信息只能按明确规则 normalize；未声明的差异不自动容忍。
7. **Judge 不掌权**：LLM-as-judge 只能产生 `semantic_score` 和解释引用，不能批准 capability、跳过 approval、改变 Acceptance criteria 或修改历史。
8. **失败可分类**：`functional_failure`、`safety_violation`、`evidence_gap`、`replay_divergence`、`infra_failure`、`flake`、`out_of_scope` 必须分开；infra/flake 不能伪装成 pass。
9. **候选变更最小化**：一次 Candidate 只改变一个主维度（model/prompt/tool_catalog/memory_index/workflow/route），关联的被动版本更新必须显式列出。
10. **质量不是第二执行脊柱**：`kiana-quality` 只做纯计算和端口调用；入口仍是 `DaemonHost → ControlPlane`，不能由 eval runner 另起模型循环或权限判断。

### 34-B.4 目标代码分层与模块落点

不建议继续把所有逻辑堆进 `kiana-commands/src/eval.rs`。新增能力按以下边界落位：

| 层 | 建议落点 | 责任 |
|---|---|---|
| 稳定对象与 schema | `kiana-domain/src/quality.rs`（必要时拆 `quality/`） | `EvalSuite`、`EvalCase`、`EvalDataset`、`GoldenTrace`、`EvalExperiment`、`EvalResult`、`QualityCandidate`、`QualityGate`、`Feedback`、`DriftAlert`；只含值对象、校验和状态转移 |
| wire 命令/事件 | `kiana-protocol` | `eval.run`、`eval.capture`、`eval.compare`、`quality.feedback`、`quality.promote`；`quality.*` RuntimeEvent 版本化、可重放、拒绝未知字段 |
| 端口 | `kiana-ports` | `EvalStore`、`FixtureStore`、`TraceSource`、`ArtifactReader`、`Judge`、`MetricsSink`、`Clock`；端口不暴露文件系统或 provider 私有类型 |
| 纯质量内核 | 新增 `kiana-quality` crate | fixture/schema 校验、TraceNormalizer、TraceDiff、断言 DSL、评分、baseline 比较、flake 分类、gate 计算；默认无 Tokio、网络和副作用依赖 |
| 权威编排 | `kiana-core/src/quality.rs` | 检查 Principal/ProjectTrust/role、实验 admission、版本快照、预算、只读 profile、质量门提交和 Promote/rollback 授权；复用 `ControlPlane` |
| 运行适配 | `kiana-daemon/src/eval_runtime.rs` | 临时 workspace、fake provider/broker、受限 EventStore/ArtifactStore、故障注入、进程回收、外部 effect deny；不得绕过 core |
| CLI 适配 | `kiana-commands/src/eval.rs`、`kiana-entrypoints` | 保留现有 `eval run` 兼容参数；改为调用 `kiana-quality`/`ControlPlane`，只负责参数解析和 human/JSON 输出 |
| fixture 与 CI | `tests/eval/`、`scripts/eval/`、`.github/workflows/` | curated/deep 数据集、golden capture、差分报告、JUnit/JSON evidence pack、PR/nightly/release 门 |

依赖方向必须是 `domain → protocol/ports → quality → core → daemon → commands/entrypoints`；`kiana-quality` 不得依赖 `kiana-query`、旧 `kiana-tools` 或直接打开网络。

### 34-B.5 领域对象和持久合同

下列是目标合同的最小字段。字段可在实现时拆成 Rust struct，但 schema 名称、状态和 digest 语义必须保持稳定。所有持久对象使用 `deny_unknown_fields` 或显式 migration。

```text
EvalDataset {
  dataset_id, version, purpose, owner, provenance, privacy_class,
  cases[], split(train|validation|regression|red_team|performance),
  created_at, expires_at?, digest
}

EvalSuite {
  suite_id, version, dataset_ref, workload_class,
  target_kind(replay|fake_model|daemon|shadow|workflow),
  case_refs[], evaluator_refs[], scoring_policy_ref,
  safety_policy_ref, budget_policy_ref, baseline_ref?,
  required_fixture_schema, owner, status(draft|active|deprecated), digest
}

EvalCase {
  case_id, suite_id, version, input_fixture_ref, initial_state_fixture_ref?,
  target_config, model_profile_ref?, prompt_bundle_ref?,
  tool_catalog_ref, memory_snapshot_ref?, workflow_ref?,
  expected_events[], expected_state, expected_artifacts[],
  expected_receipt_assertions[], forbidden_effects[],
  assertions[], fault_plan?, tags[], privacy_class, digest
}

GoldenTrace {
  trace_id, suite_id, case_id, source_run_id?, source_snapshot,
  input_hash, target_versions, event_cursor_range,
  normalized_events[], artifact_hashes[], receipt_hash?,
  normalization_version, human_acceptance?, quality_score?,
  created_at, expires_at?, provenance_ref, digest
}

EvalExperiment {
  experiment_id, suite_ref, candidate_ref, baseline_ref?,
  target_snapshot, execution_profile, random_seed, clock_mode,
  started_at, ended_at?, status(admitted|running|completed|failed|cancelled),
  case_results[], evidence_pack_ref, digest
}

EvalResult {
  result_id, experiment_id, case_id, status(pass|fail|blocked|infra_error|flake|out_of_scope),
  findings[], dimensions{functional,safety,evidence,recovery,context,cost,latency,replay},
  metrics, normalized_trace_ref, artifact_refs[], receipt_ref?,
  baseline_comparison?, evaluator_versions, created_at, digest
}

QualityCandidate {
  candidate_id, baseline_id, changed_dimension,
  changed_version_ref, suite_version, experiment_ref,
  status(draft|offline_evaluated|shadowed|approved|rejected|rolled_back|deprecated),
  owner, created_at, supersedes?
}

QualityGateDecision {
  decision_id, gate_id, gate_version, suite_version,
  candidate_id, baseline_id, thresholds, blocking_rules[],
  verdict(pass|reject|needs_shadow|rollback), score_delta,
  blocking_findings[], known_regressions[], approver?, evidence_ref,
  decided_at, source_cursor, digest
}

Feedback {
  feedback_id, principal_id, target_type(run|turn|tool_call|memory|workflow|artifact|receipt),
  target_ref, label, comment_ref?, correction_ref?, scope, privacy_policy,
  created_at, provenance_ref
}
```

`EvalResult` 是一次实验对一个 case 的事实结果，`QualityGateDecision` 是对候选版本的裁决；两者不能合并。`GoldenTrace` 是只读派生基线，不能被 `eval run` 原地覆盖；刷新必须通过 `eval capture` 产生新版本和新 provenance。

### 34-B.6 Fixture、Trace 与断言设计

#### 34.6.1 Fixture 分层

每个 case 的 fixture 由四部分组成：

```text
input fixture       = prompt/command + structured inputs (redacted)
initial state       = project trust, role, policy epoch, files, memory, workflow snapshot
provider fixture    = normalized model replies or deterministic fake stream
oracle              = expected events/state/artifacts/receipt + forbidden effects
```

fixture 目录建议为 `tests/eval/<suite>/<case>/`，文件名固定为 `case.json`、`initial-state.json`、`provider.jsonl`、`oracle.json`。入口通过 `FixtureStore` 读取，禁止从当前工作树隐式读取未声明文件；fixture 解析失败、路径逃逸、大小超限、重复 case id 和未知 schema 直接拒绝。

#### 34.6.2 Trace normalization

`TraceNormalizer` 按 `normalization_version` 执行以下顺序：

1. 只读取 durable RuntimeEvent、已完成的 Invocation/Artifact/Receipt 引用；丢弃未持久化的 stream delta、UI event 和日志文本。
2. 验证事件 sequence、aggregate/session/run 关联、parent/causation、terminal 唯一性和 schema version。
3. 对 prompt、tool argument、tool result、artifact bytes 应用已有 redaction；原文只保留在受保护 fixture，不进入 report。
4. 把稳定字段排序为 canonical JSON；数组只在 case 声明 `ordered=false` 时按 multiset 比较。
5. 对显式允许的 volatile 值替换为 `<TS>`、`<UUID>`、`<TEMP_PATH>`、`<ACTOR>` 等 token，同时记录替换计数；未声明的 volatile 字段不应静默归一化。
6. 为每个 normalized event 计算 `event_digest`，为整条 trace 计算 `trace_digest`；保留首个 divergence 的 cursor、event index 和字段路径。

#### 34.6.3 断言类型

第一版只实现可解释、可确定的断言：

| 类别 | 断言例子 | 失败代码 |
|---|---|---|
| 结构 | schema、required field、event count、terminal exactly-one | `fixture_schema_invalid`、`terminal_state_invalid` |
| 序列 | event type/order、call/result correlation、attempt monotonicity | `event_sequence_mismatch`、`call_result_unmatched` |
| 状态 | final status、approval/cancel/unknown/recovery transition | `state_expectation_mismatch` |
| 能力 | tool name、argument digest、policy verdict、grant scope | `capability_expectation_mismatch`、`policy_verdict_mismatch` |
| 产物 | file set、artifact digest、Receipt assertion、evidence ref | `artifact_expectation_mismatch`、`evidence_missing` |
| 资源 | input/output token、duration、tool calls、cost bucket | `budget_threshold_exceeded`、`latency_threshold_exceeded` |
| 安全 | forbidden effect、secret pattern、unredacted payload、网络/进程越界 | `forbidden_effect`、`secret_leak`、`sandbox_violation` |
| 语义（可选） | reference answer 对齐、rubric 分项、人工标签一致性 | `semantic_score_below_threshold`、`judge_unavailable` |

语义 judge 的输入只能是已脱敏的 case/reference/normalized output，必须记录 judge provider、model、prompt、temperature、版本和 judge trace digest；judge 不可用时按 suite policy 记 `infra_error` 或 `blocked`，不能降级为 pass。

### 34-B.7 端到端处理流程

#### 34.7.1 Admission 与拒绝路径

```text
CLI/Web/API request
  → DaemonHost resolves Principal + ProjectTrust + role + policy/config epoch
  → ControlPlane validates suite/case/fixture/schema/dataset privacy
  → reject unknown version, untrusted project, external-effect target,
     missing baseline, expired approval, invalid path, or budget overflow
  → append eval.admission_rejected with stable reason code
```

拒绝发生在 provider/broker 之前；不创建可执行 Run，不写真实 workspace，不调用 judge，不产生外部 effect。失败回执至少包含 `reason_code`、`suite_ref`、`case_ref`、actor、authority epoch 和 source cursor。

#### 34.7.2 Replay/Fake target 路径

```text
admitted EvalExperiment
  → materialize isolated temp workspace and initial-state snapshot
  → install fake provider + deny-by-default broker + bounded EventStore
  → invoke the same DaemonHost/ControlPlane command used by product path
  → record every request, policy verdict, capability attempt, artifact and receipt
  → crash/cancel/Unknown fault plan (if declared)
  → flush durable events before target returns
  → normalize trace
  → run deterministic evaluators
  → persist EvalResult and evidence pack
```

无副作用的 replay 不应重跑真实模型或真实工具。需要验证 provider parser 时使用录制的 normalized stream；需要验证完整 agent loop 时使用 fake model，所有 capability 都由 fake/deny adapter 受控。`result_unknown`、stop 未确认、EventStore flush 失败和进程崩溃都保留 Unknown，不得自动改成失败或成功。

#### 34.7.3 Baseline compare 与 divergence

比较顺序固定为：

1. suite/case/target/evaluator/schema digest 必须兼容；不兼容先报 `comparison_not_comparable`。
2. 比较 normalized event 序列的 `(aggregate, sequence, invocation_id, attempt, event_type, input_digest, state, error_code)`。
3. 再比较 artifact set/digest、Receipt assertions、metrics 和 final state。
4. 记录首个差异点；后续差异作为同一 root finding 的附加项，不重复计数。
5. baseline 缺 case、no-golden、out-of-scope、normalization replacement 超限都不能计入 pass。

#### 34.7.4 Gate 与 Promote

```text
EvalReport + baseline diff + evidence pack
  → QualityGate evaluates blocking rules first
  → reject on safety/evidence/replay/fixture integrity failure
  → pass only when all required dimensions and regression budgets pass
  → needs_shadow for allowed non-blocking regression or new candidate
  → append quality.gate_decided
  → authorized operator issues quality.promote / quality.rollback
  → ControlPlane rechecks authority, policy, candidate digest and route scope
  → append route promotion or rollback fact
```

`QualityGate` 只裁决“候选是否满足质量条件”。真正的默认路由变更必须再次经过 `ControlPlane` 的授权与审批；一个通过的离线 eval 不自动接通 live provider。

### 34-B.8 评分、门槛与回归政策

建议输出两类结果：维度分数和硬性阻断。维度分数用于趋势和诊断，硬性规则用于安全与发布。

```text
QualityScore = weighted_mean(
  functional_correctness,
  recovery_correctness,
  context_relevance,
  cost_efficiency,
  latency,
  semantic_quality
)

blocking =
  policy_safety == pass
  && evidence_completeness == pass
  && replay_correctness == pass
  && forbidden_effects == empty
  && fixture_integrity == pass
  && no_unclassified_infra_failure
```

默认策略：

- 安全、权限、secret redaction、terminal/recovery 合同为硬门，不参与平均；
- replay divergence 首项即阻断 Promote；
- functional/semantic 允许在 suite 中设置绝对阈值、相对 baseline 的最大下降和最小样本数；
- latency/cost 只在指定 performance suite 中作为门，不能覆盖安全失败；
- `flake` 需要按固定 seed 重跑一次。第二次相同失败为真实失败；两次不同且无安全问题才进入 quarantine，并让 suite 处于 `needs_review`，不能 pass；
- 多 case 聚合使用 Wilson/bootstrap 置信区间或明确的最小样本规则；样本不足标为 `insufficient_evidence`；
- 失败报告必须列出 `new_failures`、`fixed_failures`、`known_regressions`、`out_of_scope`、`no_golden` 和 `infra_failures`。

### 34-B.9 Candidate、Feedback 与 Drift

#### Candidate 状态机

```text
Draft → OfflineEvaluated → Shadowed → Approved → Deprecated
OfflineEvaluated → Rejected
Shadowed → RolledBack
```

进入 `Shadowed` 需要固定流量/样本上限、持续时间、回滚版本和 owner。任何 `route`、`model`、`prompt`、`tool_catalog`、`memory_index` 或 `workflow` 变更都必须创建新 Candidate；不允许直接编辑已批准版本。

#### Feedback

Feedback 只能指向 canonical `run/turn/tool_call/memory/workflow/artifact/receipt`，由服务端派生 provenance 和 privacy scope。`feedback → diagnosed pattern → candidate patch → eval suite → gate` 是唯一学习路径。反馈不得直接改 Policy、Grant、Approval、Acceptance criteria、Receipt 或 Memory ACL。

#### Drift

按版本和 workload 分桶观测：tool selection、policy deny、approval wait、Unknown/cancel、rework/acceptance、memory source、prompt cache、provider error、token/cost、latency、replay divergence、MCP/Skill schema。漂移只触发告警、shadow、降级或 rollback；自动修复必须产生 Candidate 和新的 EvalExperiment。

### 34-B.10 执行层级、CI 门与产物

| 层级 | 触发 | 内容 | 目标时长/范围 | 失败动作 |
|---|---|---|---|---|
| PR curated | 每次相关 Rust/fixture/schema/quality 变更 | 8–20 个最高风险 case；拒绝、工具生命周期、Receipt、replay、redaction、无副作用 | 快速、串行 daemon/core | 阻断 PR；输出 JSON/JUnit 和首个 divergence |
| PR package | 相关 crate 改动 | `kiana-quality` 单测、commands/entrypoints eval、workspace check/fmt/clippy | 受影响 package + 邻接 contract | 阻断 PR |
| Nightly deep | 定时或手动 | 全量 regression catalog、provider matrix、fault plan、性能 smoke、memory/context/workflow 分桶 | 可并行 case，但单 case 内串行；固定 worker 数 | 标记 regression/flake，生成趋势 artifact |
| Release candidate | 发布候选 tag/手动批准 | curated + deep + release-smoke + evidence pack + supply-chain/fixture provenance | 稳定快照、完整证据 | 不允许 Promote；需要人工处理 blocker |
| Shadow/live（解冻范围内） | 显式 opt-in | 脱敏采样、online evaluator、drift、rollback rehearsal | 受 budget/retention 限制 | 只告警/rollback，不改权限 |

建议新增脚本入口：

```text
scripts/eval-curated.sh
scripts/eval-deep.sh
scripts/eval-capture-golden.sh
scripts/eval-compare.sh
scripts/eval-package-evidence.sh
```

所有脚本使用显式环境白名单和临时 `KIANA_HOME`，不继承 provider key、代理、MCP 配置或用户的 `.kiana`。输出至少包括 `report.json`、`evidence-manifest.json`、可选 `junit.xml`、`trace-diff.json` 和 `reproduction.sh`；产物 manifest 绑定 source snapshot、命令 argv、cwd/environment 摘要、fixture digest、exit code 和限制。

### 34-B.11 详细实施步骤（EQ-00–EQ-51）

以下步骤是本专项的执行索引。每一步都遵循“先拒绝/越权/损坏/重放/Unknown，再成功路径”，步骤编号是局部编号，不重排既有 roadmap。

#### 波次 A：基线、schema 与边界（先建立可审计合同）

| Step | 目标与代码落点 | 最小验收 |
|---|---|---|
| <a id="step-eq-00"></a>`EQ-00` | 固定当前源码快照、工作树状态和现有 `eval` 行为；在 `docs/roadmap.md`/`CURRENT_STATUS.md` 建立本专项证据模板 | `eval_baseline_inventory_is_reproducible`；报告现有能力为 partial/local_behavior |
| <a id="step-eq-01"></a>`EQ-01` | 从 `kiana-commands/src/eval.rs` 提取 schema 常量、错误码和 JSON 兼容测试清单，禁止无记录的字段删除 | `legacy_eval_v1_contract_is_pinned` |
| <a id="step-eq-02"></a>`EQ-02` | 在 `kiana-domain/src/quality.rs` 加稳定 ID、digest、状态枚举和 `deny_unknown_fields` DTO | `quality_ids_and_state_transitions_are_validated` |
| <a id="step-eq-03"></a>`EQ-03` | 定义 `EvalDataset`/`EvalSuite`/`EvalCase`/`GoldenTrace` schema、版本和 provenance | `unknown_quality_schema_is_rejected` |
| <a id="step-eq-04"></a>`EQ-04` | 定义 case split、privacy class、owner、expires_at、minimum sample 和 workload tags | `expired_or_unowned_dataset_is_not_admitted` |
| <a id="step-eq-05"></a>`EQ-05` | 把 `kiana.eval-suite.v1`/baseline/report 与新 domain DTO 做显式 adapter，保留旧 CLI 输出字段 | `legacy_eval_cli_round_trips_through_quality_dto` |
| <a id="step-eq-06"></a>`EQ-06` | 在 `kiana-protocol` 登记 `eval.run/capture/compare` 和 `quality.feedback/promote/rollback` 命令/事件 | `quality_protocol_has_no_unversioned_events` |
| <a id="step-eq-07"></a>`EQ-07` | 在 `kiana-ports` 增加 `EvalStore`、`FixtureStore`、`TraceSource`、`ArtifactReader`、`Judge`、`MetricsSink` | `quality_ports_are_free_of_daemon_or_provider_types` |

#### 波次 B：fixture、隔离执行与事实采集

| Step | 目标与代码落点 | 最小验收 |
|---|---|---|
| <a id="step-eq-08"></a>`EQ-08` | 建立 `tests/eval/` 目录、case manifest、fixture size/path/schema 限制和 deterministic loader | `fixture_path_escape_and_unknown_fields_fail_closed` |
| <a id="step-eq-09"></a>`EQ-09` | 在 `kiana-daemon/src/eval_runtime.rs` 实现临时 workspace、临时 `KIANA_HOME`、固定 clock/random seed | `eval_target_isolated_from_operator_home` |
| <a id="step-eq-10"></a>`EQ-10` | 实现 fake provider adapter，支持完整 reply、分块 stream、tool call、malformed stream、provider error | `fake_provider_replays_normalized_stream_without_network` |
| <a id="step-eq-11"></a>`EQ-11` | 实现 deny-by-default broker；将真实 network/secret/MCP/payment/publish/desktop effect 映射为稳定拒绝 | `forbidden_capability_never_reaches_real_executor` |
| <a id="step-eq-12"></a>`EQ-12` | 通过 `DaemonHost`/`ControlPlane` 启动 target，禁止 quality crate 自行创建 runner loop | `eval_uses_the_same_daemonhost_spine` |
| <a id="step-eq-13"></a>`EQ-13` | 将 initial state、policy snapshot、role assignment、memory/workflow/artifact fixture 装入受控 store | `initial_state_digest_is_bound_to_experiment` |
| <a id="step-eq-14"></a>`EQ-14` | 采集 RuntimeEvent、Invocation、Artifact、Receipt 引用和 command receipt；flush 失败产生 infra/Unknown | `event_flush_failure_never_returns_eval_pass` |
| <a id="step-eq-15"></a>`EQ-15` | 增加 fault plan：approval deny/expire、cancel race、crash after effect、restart、stale lease、result unknown | `fault_plan_preserves_unknown_and_stop_evidence` |
| <a id="step-eq-16"></a>`EQ-16` | 对进程树、文件 diff、网络 syscall、secret pattern 做 eval-only evidence capture | `eval_evidence_has_no_unredacted_secret` |

#### 波次 C：TraceNormalizer、GoldenTrace 与差分

| Step | 目标与代码落点 | 最小验收 |
|---|---|---|
| <a id="step-eq-17"></a>`EQ-17` | 在 `kiana-quality/src/normalize.rs` 实现 durable event 选择和 sequence/terminal/correlation 校验 | `invalid_event_cursor_or_multiple_terminal_is_rejected` |
| <a id="step-eq-18"></a>`EQ-18` | 实现 canonical JSON、稳定数组策略、字段白名单和 redaction 复用 | `normalization_is_stable_and_redacts_payloads` |
| <a id="step-eq-19"></a>`EQ-19` | 实现受控 volatile normalization（timestamp/UUID/temp path/actor）并记录替换计数 | `undeclared_volatile_field_is_not_silently_normalized` |
| <a id="step-eq-20"></a>`EQ-20` | 计算 event/trace/artifact/receipt digest，绑定 `normalization_version` | `same_fixture_has_same_trace_digest` |
| <a id="step-eq-21"></a>`EQ-21` | 实现 `TraceDiff`：首个 divergence、字段路径、cursor、expected/actual 摘要和分类 | `trace_diff_reports_first_divergence_deterministically` |
| <a id="step-eq-22"></a>`EQ-22` | 支持 exact、ordered、multiset、numeric tolerance、regex/contains 等声明式 assertion | `assertion_modes_do_not_change_unrelated_fields` |
| <a id="step-eq-23"></a>`EQ-23` | 添加 `eval capture`：只从明确 source run/fixture 生成新 GoldenTrace，原文件不可覆盖 | `golden_capture_requires_source_and_writes_new_version` |
| <a id="step-eq-24"></a>`EQ-24` | 添加 Beads 风格 reference/candidate scenario runner、环境清理、in-scope/out-of-scope predicate | `candidate_diff_uses_scrubbed_environment_and_scope_report` |
| <a id="step-eq-25"></a>`EQ-25` | 支持 curated/deep catalog、no-golden、skip reason、scenario dedupe 和 stable ordering | `deep_catalog_is_opt_in_and_no_golden_is_visible` |
| <a id="step-eq-26"></a>`EQ-26` | 为 Runtime、Approval、Hook、Memory、Workflow、Swarm 各补 provider-independent trace fixture | `core_negative_fixture_matrix_is_complete` |

#### 波次 D：Evaluator、评分与统计

| Step | 目标与代码落点 | 最小验收 |
|---|---|---|
| <a id="step-eq-27"></a>`EQ-27` | 实现 deterministic evaluator trait 和 finding schema（code/expected/actual/message/evidence_ref） | `every_finding_has_stable_code_and_evidence` |
| <a id="step-eq-28"></a>`EQ-28` | 实现 runtime correctness evaluator：事件顺序、调用关联、terminal、retry、approval、cancel、Unknown | `runtime_evaluator_catches_unmatched_and_duplicate_terminal` |
| <a id="step-eq-29"></a>`EQ-29` | 实现 capability/safety evaluator：schema、grant scope、policy verdict、hook、network/process/file effect | `safety_failure_blocks_even_with_good_final_text` |
| <a id="step-eq-30"></a>`EQ-30` | 实现 evidence/receipt evaluator：artifact hash、receipt assertions、redaction、provenance、source cursor | `missing_evidence_is_blocking` |
| <a id="step-eq-31"></a>`EQ-31` | 实现 recovery/replay evaluator：crash/restart、fence、result_unknown、logic version、divergence | `replay_divergence_and_unknown_are_distinct_findings` |
| <a id="step-eq-32"></a>`EQ-32` | 实现 context/memory evaluator：ACL-before-ranking、provenance、freshness、compaction、budget | `unauthorized_memory_hit_is_blocking` |
| <a id="step-eq-33"></a>`EQ-33` | 实现 workflow/swarm evaluator：DAG、attempt、fan-in/out、child scope、merge、compensation | `child_scope_expansion_fails_quality_gate` |
| <a id="step-eq-34"></a>`EQ-34` | 实现 performance/cost metrics evaluator：duration、tokens、tool calls、cache、cost buckets | `budget_and_latency_thresholds_are_explicit` |
| <a id="step-eq-35"></a>`EQ-35` | 实现可选 semantic judge port；固定 judge prompt/model/version，judge 不可用不降级为 pass | `judge_unavailable_is_not_a_success` |
| <a id="step-eq-36"></a>`EQ-36` | 实现维度聚合、absolute/relative threshold、minimum sample 和置信区间策略 | `insufficient_sample_is_blocked_or_needs_review` |
| <a id="step-eq-37"></a>`EQ-37` | 实现 retry-once flake classifier、quarantine 记录和 infra failure 分类 | `flake_is_never_counted_as_pass` |

#### 波次 E：Baseline、Candidate 与 QualityGate

| Step | 目标与代码落点 | 最小验收 |
|---|---|---|
| <a id="step-eq-38"></a>`EQ-38` | 定义 `EvalExperiment` admission/terminal 状态和 case result 索引 | `experiment_state_is_replayable_from_events` |
| <a id="step-eq-39"></a>`EQ-39` | 实现 baseline registry：suite/case/target/evaluator digest、owner、expiry、refresh provenance | `stale_or_incompatible_baseline_cannot_compare` |
| <a id="step-eq-40"></a>`EQ-40` | 实现 `QualityCandidate` 单主变更维度和版本快照绑定 | `candidate_cannot_hide_model_or_prompt_drift` |
| <a id="step-eq-41"></a>`EQ-41` | 实现 `QualityGate` 配置与 `QualityGateDecision` 裁决分离、不可改写 | `gate_config_update_does_not_mutate_old_decision` |
| <a id="step-eq-42"></a>`EQ-42` | 实现 blocking rules：safety/evidence/replay/forbidden effect/fixture integrity/infra | `blocking_rule_precedes_weighted_score` |
| <a id="step-eq-43"></a>`EQ-43` | 实现 `quality.promote`/`quality.rollback` 的 ControlPlane 二次授权、审批、scope 和 epoch 重查 | `passing_eval_cannot_promote_without_authority` |
| <a id="step-eq-44"></a>`EQ-44` | 实现 shadow admission、sample/TTL/rollback route 和自动回滚证据 | `shadow_regression_rolls_back_without_grant_change` |

#### 波次 F：Feedback、Drift、CLI 与 CI

| Step | 目标与代码落点 | 最小验收 |
|---|---|---|
| <a id="step-eq-45"></a>`EQ-45` | 实现 `quality.feedback`，只引用 canonical target，服务端派生 provenance/privacy scope | `feedback_cannot_edit_policy_or_receipt` |
| <a id="step-eq-46"></a>`EQ-46` | 实现版本分桶 drift metrics、告警和 `drift.alerted` 事件 | `drift_alert_does_not_change_route_or_grant` |
| <a id="step-eq-47"></a>`EQ-47` | 扩展 `kiana eval`：`run/capture/compare/explain/list`，旧 `run --suite` 参数保持兼容 | `cli_eval_commands_route_through_control_plane` |
| <a id="step-eq-48"></a>`EQ-48` | 输出 JSON report、JUnit、human summary、evidence manifest、reproduction command；路径和 secret 脱敏 | `report_is_machine_readable_and_redacted` |
| <a id="step-eq-49"></a>`EQ-49` | 新增 `scripts/eval-curated.sh`、`eval-deep.sh`、`eval-capture-golden.sh`、`eval-compare.sh` | `scripts_use_explicit_environment_allowlist` |
| <a id="step-eq-50"></a>`EQ-50` | 接入 PR curated/package、nightly deep、release candidate workflow，daemon/core 测试串行 | `ci_lanes_have_distinct_scope_and_fail_closed` |
| <a id="step-eq-51"></a>`EQ-51` | 归档 report/trace-diff/evidence/reproduction，生成 `CURRENT_STATUS.md` 证据块 | `quality_evidence_block_is_complete` |

> `EQ-45`–`EQ-51` 是本专项的补充步骤，不要求把旧有 `P1-L1-01` 或其他历史 step 改名；若某个实现已经由当前 agent 完成，应将其验收绑定到对应 EQ step，并补充证据，而不是把未验证能力标成完成。

### 34-B.12 依赖波次与建议执行顺序

```text
A contracts: EQ-00 → EQ-01 → EQ-02 ∥ EQ-03 → EQ-04 → EQ-05 → EQ-06 → EQ-07
      ↓
B isolated target: EQ-08 → EQ-09 → EQ-10 ∥ EQ-11 → EQ-12 → EQ-13 → EQ-14 → EQ-15 → EQ-16
      ↓
C trace: EQ-17 → EQ-18 → EQ-19 → EQ-20 → EQ-21 ∥ EQ-22 → EQ-23 → EQ-24 → EQ-25 → EQ-26
      ↓
D evaluators: EQ-27 → EQ-28 ∥ EQ-29 ∥ EQ-30 → EQ-31 → EQ-32 ∥ EQ-33 ∥ EQ-34 → EQ-35 → EQ-36 → EQ-37
      ↓
E gate: EQ-38 → EQ-39 → EQ-40 → EQ-41 → EQ-42 → EQ-43 → EQ-44
      ↓
F operations: EQ-45 → EQ-46 ∥ EQ-47 → EQ-48 → EQ-49 → EQ-50 → EQ-51
```

可并行的步骤只共享只读 fixture 或独立模块；修改 `Cargo.toml`、`Cargo.lock`、protocol registry、CI workflow 和公共 schema 的集成由一个负责人串行合并。任何依赖步骤的拒绝、schema 不兼容或安全失败，都取消其后续 Promote 相关步骤，保留可独立完成的诊断和证据工作。

### 34-B.13 最低验收矩阵

| 证据面 | 必须先证明的拒绝路径 | 成功路径 | 证明上限 |
|---|---|---|---|
| Fixture/admission | untrusted project、unknown schema、path escape、expired dataset、budget overflow | 合法 suite/case 被 admit | `local_behavior` |
| Side effect | network/secret/MCP/payment/publish/write effect 被 deny | fake broker 完成只读或临时 workspace 任务 | `local_behavior` |
| Runtime | malformed stream、unmatched result、duplicate terminal、stale epoch、Unknown | normalized event/Receipt 完整 | `local_behavior`，有持久 EventStore 时再申请 `durable` |
| Trace/diff | cursor gap、redaction failure、undeclared volatile、首个 divergence | exact/multiset/tolerance 按声明比较 | `local_behavior` |
| Safety | policy bypass、scope expansion、hook failure、secret leak | 全部安全 evaluator pass | `local_behavior` |
| Recovery | crash、cancel race、stop 未确认、flush 失败、replay unknown version | 恢复到明确状态或保留 Unknown | `local_behavior`/`durable` 取决于证据 |
| Scoring | missing evidence、judge unavailable、insufficient sample、flake | 维度分数和 finding 可解释 | `local_behavior` |
| Promote | gate pass 但无 authority/approval、candidate digest 漂移 | 二次授权后追加 route fact | `local_behavior`，不能自动声称 live |
| CI/release | curated fail、deep no-golden、out-of-scope 被隐藏、artifact 缺失 | 分层门、报告、重现命令和证据归档 | `local_behavior` |

### 34-B.14 与现有路线图的接点

| 既有单元 | 关系 |
|---|---|
| `P0-A-01b` | 注册 Eval/Quality schema、unknown field/migration 规则 |
| `P0-G-02a/b`、`P0-G-04` | 提供 durable event、model-visible history、replay source；`EQ-17` 依赖其事件合同 |
| `P0-J1-*`、`P0-F-*` | cancellation、approval、Unknown 和恢复 fault plan 的被测对象 |
| `P1-J2/J3` | context/memory evaluator、prompt/tool budget、provenance 和 freshness 分桶 |
| `P1-J4`、`P1-H-*` | tool catalog、MCP schema、argument/path/safety evaluator |
| `P1-J8-01` | observability 字段和 trace/receipt 关联；`EQ-34` 消费其 usage/cost |
| `P1-K5-01` | runtime/project budget 与 cost evaluator，不能把 cost ledger 当 quality gate 事实源 |
| `P1-L1-01` | 本专项的原始单元；由 `EQ-17..26`、`EQ-38..44` 具体化 |
| `P1-L4-01` | RepositorySnapshot/RepoMap 作为 case 输入和 freshness evaluator |
| `P2-K6/K7` | 对账、删除传播、retention 对 EvalStore/evidence 的生命周期约束 |
| `P2-M*` | CLI/Web 的 eval report、gate 状态和人工反馈只做投影 |
| `P3-I-04/I-06` | Acceptance、Review 和 fake-model coding golden 闭环接入 business outcome evaluator |
| `P4-J3-05/J6/J7`、`P4-L3/L5/L6` | Swarm、stream、版本 drift、扩展供应链的 deep/release eval |

### 34-B.15 交付与状态口径

本专项在 `EQ-00..26` 完成前只能声明“有统一 fixture/trace 的局部基础”；在 `EQ-27..44` 完成前不能声明“有质量评分或 Promote gate”；在 `EQ-45..51` 完成前不能声明“有持续反馈、漂移监控或发布质量平台”。每次状态更新必须在 `CURRENT_STATUS.md` 写证据块：

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

当前已存在的 `kiana eval run`、release smoke 和 crate focused tests 只能作为 `EQ-00`/`EQ-01`/部分 `EQ-05` 的基线证据；它们没有证明真实模型质量、跨重启 durable EvalStore、在线 drift、外部 side effect 安全或 live Promote。评测报告、GoldenTrace、Baseline、QualityGate 和 Feedback 都必须服从同一事实源和授权链，不能成为第二个执行循环。

<a id="billing-quota-cost-plan"></a>

## 35. 计费、配额与成本：实际代码设计、处理流程与详细实施步骤（2026-09-14 追加）

> 本专项补全 [`module-map.md`](module-map.md) 第 18 模块，并把现有 `P1-K5-01`、`CP-11`、`P4-J7-24/25`、`ER-12`、`CO-45`、`OA-08` 的交叉要求收敛成一个可执行设计。旧路线图中“只做简单 usage”“不接账单”的限制不阻塞本专项设计；当前状态仍由 [`CURRENT_STATUS.md`](../CURRENT_STATUS.md) 决定，本节所有条目初始为 `feature_status=target`、`proof_level=source`。
>
> 本次调研先对 `reference/` 的项目目录和 `docs/reference-agent-audit/` 做全量关键词盘点，再对代表性实现做源码级核对。参考项目只提供行为对照，不复制其源码、凭据格式、许可证、遥测目的或第二条执行循环。第一阶段只实现本地事实、预算和对账边界，不实现支付、发票开具或企业多租户结算。

### 35.1 设计结论和范围

成本系统要回答四个不同问题：这次运行消耗了什么、当前还允许消耗多少、应把消耗归属给谁、外部 Provider 最终收了多少钱。四个问题必须由不同的对象和状态回答：

| 问题 | 权威对象 | 可以做的决定 | 不能推导的结论 |
|---|---|---|---|
| 本次实际消耗 | `UsageRecord`（每个 model/effect attempt 一条） | 生成 Receipt、运行指标和重放输入 | 缺失 usage 不是零成本；模型文本不是事实 |
| 当前运行能否继续 | `RuntimeBudget`、`BudgetLease`、`QuotaReservation` | 在副作用前允许、排队或拒绝 | 预算剩余不代表项目达成目标，也不代表已付款 |
| 成本如何归属 | `CostLedger`、`CostAllocation`、`RateCard` | 按 org/project/workflow/cell/run/model/provider 聚合 | 聚合视图不能复制扣费；计划成本不是实际成本 |
| 外部账单是否结清 | `MeasuredCost` + `ProviderReceipt` + `CostCorrection` | 对账、差异告警、导出给财务系统 | 没有 Provider receipt 不能声称已结算，FinancialBudget 首发不实现 |

预算层次固定为：

```text
FinancialBudget       组织/合同/付款承诺（首发只保留接口，不授权执行）
  └── ProjectBudget   项目周期成本、容量和时间基线
        └── RuntimeBudget   一次 Run 的硬上限
              └── BudgetLease   Cell/子任务从父预算派生的更窄租约

ProviderBudget        Provider/credential/model 的速率、并发和容量限制（横切层）
```

以下关系是硬不变量：

```text
BudgetLease consumed  ≠ RuntimeBudget achieved
RuntimeBudget spent  ≠ ProjectBudget settled
estimated cost       ≠ measured provider bill
provider bill        ≠ FinancialBudget authorization
```

本专项覆盖模型 token、缓存/推理 token、模型请求次数、工具调用、外部 effect 次数、并发槽、墙钟、输出/日志/Artifact 字节、Provider RPM/TPM 和可选成本上限。工具或本地模型没有外部价格时，仍记录资源用量；`cost=0` 只在 RateCard 明确给出零价且来源已固定时成立。

### 35.2 Reference 调研归纳

| 参考项目/材料 | 源码中观察到的模式 | Kiana 采用方式 | 明确不照搬 |
|---|---|---|---|
| DeepSeek Harness | provider stream 在终态才确认 usage；坏流、截断流和失败 attempt 保留错误与 usage；fixture 能断言请求数 | `NormalizedUsage` 按 attempt 提交；终态/EOF/取消分开；Fake Provider 断言 reservation 与真实请求数 | 不把 stdout JSONL 或 observer 当作账本；不让 SDK 自己重试 |
| Codex | thread usage 同时区分 `net_new_input_tokens`、cached、input、output、total 和 estimated USD；缺失字段保留 `null`；analytics 可选择是否发送 usage | UsageVector 允许未知；cache 与 total 的包含关系由协议 schema 声明；敏感数据默认不外发 | 不把 telemetry 或 workload token 当 Kiana 授权；不采用远程账号作为 Principal |
| Pydantic AI | `RequestUsage`/`RunUsage` 按字段可加；requests、input/output/tool calls、每请求 input 与累计 limit 分开；cost limit 对未知价格保持不可强制；显式 0 不被当作 unset | 采用 typed usage vector、per-attempt 与 cumulative 两层；0 与 unknown 分离；限制失败返回稳定 reason | 不直接把异常/回调当持久事实；不让任意 provider 字段进入核心 schema |
| OpenAI Agents | retry attempt 的 usage 会进入 run usage；trace 与 usage 可关闭敏感数据；turn/tool/output limit 分层 | retry 产生新 attempt，usage 保留；Trace/Metric 与 EventLog 分开；每类限制独立计数 | 不把 tracing callback 作为预算闸门；不让 trace 数据扩大 DataBoundary |
| Agno / MetaGPT / AutoGen / Agency Swarm / ChatDev | 有 run/team round、token 或 cost budget，常以进程内汇总和最大轮数终止 | 只吸收维度分桶、round/retry 成本可见性和 team 汇总 | 不采用进程内全局计数；不把 round 结束等同 durable settlement |
| Aider / Continue / Roo / Cline | 上下文 token 估算、切换模型保留历史/成本、abort 和 context exhaustion；Roo 维护 token 统计与 provider rate limit | 估算值带 `basis`；Continue/restart 不重置 Run 链累计；Provider capacity 独立于 Run budget | 不把字节估算说成 tokenizer 精确值；不由 UI 历史计算权威费用 |
| Goose / Crush / OpenCode | accepted run reservation、dispatch/cancel race、RunID 终态、projector/hydration、terminal event 保障并发安全 | 采用 reservation→dispatch→settlement、单终态和重启恢复；查询读模型可重建 | 不保留双 Agent loop；不以 bus publish 代替 commit |
| Mini-SWE-agent | 每次调用检查 `n_calls`、cost 和 wall clock；format error/retry 有上限，但存在 off-by-one 风险 | 把预算检查放在原子 reservation；用 property test 覆盖边界和 0/1 限额 | 不把本地 cost float 直接当账务金额；不接受隐式重试 |
| 12-factor agents | 清晰展示事件累积和人类审批，但没有持久预算、取消、重试或并发版本 | 只采用“控制流由服务端掌握”的原则，补齐事实与限制 | 不把内存 Map、类型断言或模型意图当授权/账本 |
| LangGraph / Temporal SDK | checkpoint/history replay、super-step 或 activity/effect 边界；重试与恢复围绕持久事件历史 | 让 reservation、provider attempt 和 reconciliation 成为可重放边界；每个 effect 绑定 attempt/fence | 不把 workflow checkpoint 当 usage ledger；不把 activity retry 当 exactly-once 外部扣费 |
| Beads / OpenSpec / Archon | 有序事件 journal、迁移/保留 watermark、artifact/plan 版本和 schema 校验 | 采用稠密 cursor、迁移前置检查、版本化 RateCard/CostCorrection 和 artifact evidence | 不引入其项目级 CLI 或另一路执行器 |
| MemPalace / claude-memory / Graphiti | 原始会话、抽取结果、provenance、validity window 和索引分层；检索是派生数据 | 将 usage/cost 原始事实与 rollup、cache、memory retrieval cost 分离；删除/撤销可传播 | 不用向量索引或 memory summary 作为成本事实，也不把命中次数当质量结果 |
| OpenTelemetry（参考规范） | logs、metrics、traces 分离，指标标签需控制基数；观测失败不能改变业务状态 | 保留 `UsageRecord`/EventLog 为事实，cost/queue/retry 为低基数 metrics，Trace 只存关联 ref | 不把 exporter、span 或 callback 当授权、审批或结算证据 |
| CompanyOS 规范、现有 Provider/CP/ER 设计 | 明确 `UsageRecord`、`CostLedger`、`RateCard`、`CostCorrection`、`Backpressure`；EventLog 是事实源，Receipt 是投影 | 将已有合同细化为字段、事件、CAS、对账和迁移；复用现有 `UsageRecord`/`CostLedger`，不建第二套账 | 规范目标不当作已实现；`estimated_cost` 不用于 FinancialBudget 结算 |

全量盘点的结论是：参考项目普遍实现了运行时 usage 或 cost 显示，却很少同时具备 durable reservation、价格版本、未知结果和外部账单对账。Kiana 的差异点必须放在“副作用前预留、事件后结算、未知不归零、修正可追溯”四个边界，而不是再增加一个 UI cost counter。

可复核的调研材料包括 [`docs/reference-agent-audit/01-codex.md`](reference-agent-audit/01-codex.md)、[`02-deepseek-harness.md`](reference-agent-audit/02-deepseek-harness.md)、[`07-goose.md`](reference-agent-audit/07-goose.md)、[`08-opencode.md`](reference-agent-audit/08-opencode.md)、[`09-continue.md`](reference-agent-audit/09-continue.md)、[`10-roo-code.md`](reference-agent-audit/10-roo-code.md)、[`11-crush.md`](reference-agent-audit/11-crush.md)、[`13-mini-swe-agent.md`](reference-agent-audit/13-mini-swe-agent.md)、[`17-openai-agents-python.md`](reference-agent-audit/17-openai-agents-python.md)、[`18-pydantic-ai.md`](reference-agent-audit/18-pydantic-ai.md)、[`21-agno.md`](reference-agent-audit/21-agno.md)、[`24-metagpt.md`](reference-agent-audit/24-metagpt.md)、[`26-12-factor-agents.md`](reference-agent-audit/26-12-factor-agents.md)、[`99-kiana-mapping.md`](reference-agent-audit/99-kiana-mapping.md)，以及本仓库的 [`provider-design-research.md`](provider-design-research.md)、[`company-os-operations-governance.md`](company-os-operations-governance.md) 和 [`company-os-spec-index.md`](company-os-spec-index.md)。补充核对了 `reference/langgraph/`、`reference/temporal-sdk-python/`、`reference/beads/`、`reference/MemPalace/`、`reference/claude-memory/`、`reference/graphiti/` 中的 checkpoint、journal、retention 和 provenance 实现。这些报告记录了源码路径和限制；未列出的 `reference/` 项目已参与关键词/入口盘点，但不被描述为逐行审计或已运行。

### 35.3 目标领域合同

以下类型优先落在 `kiana-domain`，使用整数和 checked 运算；不能使用浮点数表达钱或配额。`Debug`、`Display`、serde、错误文本和事件投影只能输出 opaque ID、digest、状态和范围，不输出 credential、完整 prompt 或原始 provider response。

```text
UsageId / AttemptId / InvocationId / ReservationId / LedgerEntryId
OrganizationId / ProjectId / WorkflowInstanceId / CellId / RunId
ProviderId / ModelId / CredentialGroupId / RateCardId / ProviderReceiptRef

UsageVector {
  input_tokens: Option<u64>,
  output_tokens: Option<u64>,
  cache_read_tokens: Option<u64>,
  cache_write_tokens: Option<u64>,
  reasoning_output_tokens: Option<u64>,
  audio_input_tokens: Option<u64>,
  audio_output_tokens: Option<u64>,
  tool_calls: u64,
  effect_count: u64,
  wall_time_ms: u64,
  output_bytes: u64,
  artifact_bytes: u64,
  storage_bytes: u64,
}

NormalizedUsage {
  usage_id, attempt_id, invocation_id?, run_id, cell_id?, project_id?, organization_id?,
  provider_id, requested_model_id, served_model_id?, route_id, retry_ordinal,
  vector: UsageVector, source: provider|local_executor|derived,
  observation: snapshot|delta|final, sequence?, observed_at,
  confidence: known|partial|unknown, basis, raw_digest,
  rate_card_ref?, estimated_cost?, measured_cost?, provider_receipt_ref?
}

RateCard {
  rate_card_id, provider_id, model_selector, currency, unit_scale,
  input_price_per_unit?, output_price_per_unit?, cache_read_price_per_unit?,
  cache_write_price_per_unit?, reasoning_price_per_unit?, audio_price_per_unit?,
  request_price?, tool_price?, effective_from, effective_to?, version, source, digest
}

Money { currency, micros: i128 }

RuntimeBudget { max_model_calls, max_tokens, max_tool_calls, max_effects,
                max_wall_time_ms, max_output_bytes, max_storage_bytes }
ProjectBudget { project_id, period_start, period_end, max_runs?, max_tokens?,
                max_estimated_cost?, max_measured_cost?, max_concurrency }
BudgetLease { lease_id, parent_ref, owner_cell, limits, reserved, consumed,
              authority_epoch, budget_epoch, expires_at, state }
ProviderBudget { quota_group, provider_id, model_selector?, window,
                 max_requests, max_input_tokens?, max_output_tokens?, max_cost?,
                 max_concurrency, queue_limit, retry_after_policy }
QuotaReservation { reservation_id, dimensions, limits_snapshot, amounts,
                   owner_run, owner_attempt, authority_epoch, config_revision,
                   expires_at, state: reserved|settled|released|unknown|expired }

CostLedgerEntry { entry_id, kind: reservation|consumption|release|correction,
                  usage_ref?, reservation_ref?, allocation_ref?, amount,
                  estimated_cost?, measured_cost?, currency, rate_card_ref?,
                  provider_receipt_ref?, created_at, source_event, digest }
CostCorrection { correction_id, target_entry_ref, delta_estimated?, delta_measured?,
                 reason, evidence_refs, requested_by, approval_ref, created_at }
```

字段规则：

1. `Option<u64>` 表示未知，不表示零；只有 provider 明确报告零才写 `Some(0)`。同理，`estimated_cost=None` 代表没有可用价格或 usage 不完整。
2. `UsageVector` 是叶子事实。一条 attempt 只生成一条逻辑 usage；按 project、role、workflow 等维度聚合时只生成 allocation 引用，不能把同一消费复制到多个账本再相加。
3. 流式 provider 的 `snapshot` 取同一 attempt 的最后一个单调有效快照；`delta` 按 sequence 去重后相加；重复 sequence + 不同 digest、计数回退、溢出和字段包含关系矛盾都拒绝结算。
4. `total_tokens` 不作为独立可加字段存储；由协议映射表明确 `input + output` 是否包含 cache/reasoning。无法证明包含关系时保留 raw 字段摘要并把 billable 部分标为 unknown。
5. 价格使用货币最小单位整数和 `checked_mul/checked_add`；RateCard 变更只能新增版本。历史记录永远引用旧版本，不能按当前价格重算历史账单。
6. `measured_cost` 只有在 provider receipt、发票导入或受信本地计量带有稳定 ref 时才能写入；估算成本可用于 admission 预留、告警和背压，不能清算 FinancialBudget。
7. correction 只能追加事件，不能 UPDATE/DELETE 原始记录；正负调整都必须有原因、证据、审批和目标 digest。修正后的查询视图是原始 entries 加 corrections 的确定性折叠。

### 35.4 统一处理流程

```text
入口请求
  → DaemonHost/ControlPlane 认证并固定 AuthoritySnapshot
  → 解析 ConfigSnapshot、Provider route、DataBoundary 和 RateCard revision
  → 编译最终 model/effect 请求，计算可解释的估算上界
  → 检查 RuntimeBudget、BudgetLease、ProjectBudget、ProviderBudget、容量和期限
  → 原子提交 QuotaReservation + BudgetLease reserved + command receipt
       ├─ Denied / Queued / Delayed：不产生 provider 或 handler effect
       ├─ Replayed：校验相同 digest，复用原 settlement/receipt
       └─ Committed：签发一次性 permit，进入 provider/broker
  → 创建 attempt；发送前再次校验 epoch、route、rate-card、reservation 和 cancel
  → provider/handler 执行；采集 normalized result、usage、receipt、stop report
  → 原子追加 attempt terminal + usage observed + settlement/release + artifact refs
       ├─ Known：结算已知消耗，释放未使用预留
       ├─ Partial：结算已知部分，剩余保持 reserved/unknown
       └─ Unknown：保留保守预留，进入 Incident/Reconciliation，不自动重试
  → projector 按 source_cursor 更新 run/project/provider usage rollup
  → Receipt 显示 usage、estimated/measured/unknown、reservation、retry、queue wait 和限制
  → 外部账单导入按 ProviderReceipt 对账，差异只产生 CostCorrection
```

模型请求和工具 effect 共用这条流程，但计量维度不同：模型请求至少计 requests、input/output/cache/reasoning tokens 和 provider latency；工具 effect 至少计 tool calls、effect count、wall time、output/artifact/storage bytes。一个模型产生多个工具声明时，模型 token 只在 model attempt 结算，工具 effect 只有在 Broker 真正启动后才计为 effect；被拒绝或未执行的声明不能伪造工具消费。

### 35.5 预算、配额和背压语义

**Admission。** `ControlPlane` 在发送 provider 请求或调用 Broker 前计算 `ReservationPlan`。输入 token 若没有可信 tokenizer，只能用 `basis=bytes_upper_bound` 或配置上界，并在 Receipt 标记估算；输出必须把 provider 能强制的 `max_output` 纳入上界。若 provider 没有可强制的输出限额或可靠价格，硬门只使用 requests/tokens/capacity，`cost_hard_limit_enforced=false`，不能声称费用不会超。

**层级取交集。** 可用额度是 `parent lease ∩ project policy ∩ role/template ∩ provider budget ∩ current authority`。child 预留从 parent 的 `remaining_reserved` 中扣除；兄弟总预留不能超过 parent。Continue、新 turn、重启和模型切换都不能清掉同一 Run 链的已消费量或 in-flight unknown reservation。

**窗口。** Provider RPM/TPM 和组织周期配额使用带时区的 UTC 窗口和注入时钟；窗口滚动只影响新 reservation，旧窗口中已接受的 attempt 仍按原 limits snapshot 结算。时钟回退、窗口溢出或 epoch 不一致时拒绝新 reservation，不提前释放旧 reservation。

**背压。** 所有队列有明确的 item、字节和等待时限上限，顺序为 `Accept → Queue → Delay → Coalesce（只用于声明幂等且保存 occurrence/digest）→ Reject(retry_after) → Escalate`。等待审批、退避和排队不占 active provider/concurrency slot；取消必须从队列移除并追加事实。退避不消耗新的 token/cost reservation，重新发送必须创建新的 attempt 并重新准入。

**释放和未知。** 预留未执行部分可释放；已知 usage 不退款；模型在请求已发出后失联、进程停止但 effect 未确认、provider receipt 缺失时状态是 `unknown`，保持保守占用并创建 reconciliation case。只有明确的 provider receipt、只读查询或人工批准的 reconcile 才能把 unknown 转为 settled/released/abandoned。

**重试和 fallback。** 只有发送前可证明未被 provider 接受的 transient error 才允许在同一逻辑命令中排队重试；每次重试是新 attempt、独立 usage 和独立 reservation。发送后 EOF、连接断开、取消或未知 HTTP 结果一律不自动重发。fallback 必须重新校验 capability、context/data boundary、credential、RateCard、ProviderBudget 和审批，不能把两个 provider 的部分结果拼成一个 usage。

### 35.6 代码落点和接口边界

| 层 | 目标改动 | 约束和现有锚点 |
|---|---|---|
| `kiana-domain` | 扩展 `usage.rs` 为 `UsageVector`、`NormalizedUsage`、`Money`、`RateCard`、`CostLedgerEntry`、`CostCorrection`、`ReservationPlan`、typed unknown/reason；补 ID、schema 和状态转移 | 只放纯值对象和 checked arithmetic；复用现有 `UsageRecord`/`CostLedger` 名称，提供旧字段 upcaster；不依赖 provider、Tokio 或文件系统 |
| `kiana-protocol` | versioned usage/quota/cost query、reservation、settlement、correction、backpressure 和 receipt DTO | wire 不接受 caller 自报的 project/role/budget；金额用整数 micros + currency；未知和估算必须可序列化 |
| `kiana-ports` | `UsageLedgerPort`、`QuotaReservationPort`、`RateCardPort`、`CostReconciliationPort`、`CapacityPort`、`ClockPort` | port 返回结构化错误和 opaque refs；reserve/settle/release/correct 都幂等并支持 expected versions |
| `kiana-core` | `BudgetAdmission`、`QuotaService`、层级 reservation/CAS、ProviderBudget window、backpressure、retry/fallback 再准入、correction approval | 复用 `model_budget.rs`、`cell_registry.rs`、`company.rs`；模型/工具都从 ControlPlane 进入；不在 daemon 或 UI 复制预算判断 |
| `kiana-provider` / `kiana-daemon` | provider usage normalization、stream snapshot/delta、RateCard route、credential quota group、capacity queue、ProviderReceipt adapter | `Connection` 不持久化 raw secret；每个 attempt 绑定 route/config/authority/rate-card revision；synthetic stream 不标 native |
| `kiana-eventlog` | `usage.reserved`、`usage.observed`、`usage.settled`、`usage.released`、`usage.unknown`、`cost.corrected` facts；CAS/dedup/projector checkpoint | EventLog 是唯一事实源；append 未确认不得 dispatch；same attempt 只能 settle 一次；旧事件通过显式 upcaster 读取 |
| `kiana-query` | org/project/run/provider/model/workflow rollup、daily/window query、unknown/reconciliation、cost-per-accepted-delivery 计算 | 查询来自可重建 projection，带 `source_cursor`、`projection_version`、`data_epoch`；不能从 UI cache 反推成本 |
| `kiana-entrypoints` / UI | 预算摘要、reservation/queue 状态、estimated/measured/unknown、rate-card version、reconcile/correction inbox | 只读投影；审批和 correction 走 versioned command；不显示“已付款”或把估算渲染成实测 |
| `scripts` / tests / `CURRENT_STATUS.md` | fake provider/broker、invoice cassette、concurrency/fault/replay fixtures、evidence block | 每步先 deny 再 success；真实 provider 仅显式 opt-in；状态和证明等级分开记录 |

现有代码的迁移重点：`kiana-domain/src/usage.rs` 当前 `CostLedger.cost_micros` 永远为 `None`，`Quota` 只有 scope/model_calls/tokens/concurrency；`kiana-core/src/model_budget.rs` 当前按 `text_bytes_plus_output_limit` 做模型预留，尚未覆盖 provider 价格、工具 effect 和 project rollup；`kiana-core/src/receipts.rs::cost_ledger_from_events` 只从 `run.model_turn` 聚合 input/output。实施时先扩展这些路径和事件，再删除/隔离任何旧的进程内计数器，避免出现第二成本账本。

### 35.7 详细实施步骤（BQ-00–BQ-30）

每张卡都是一个最小可验证切片。先跑拒绝验收，确认 provider/handler dispatch 数为零，再跑成功和恢复验收。编号是本专项局部索引，不重排既有 P0–P6、P1-K5-01 或 P4-J7 编号。

| Step | 目标与代码归属 | 依赖 | 先拒绝验收 | 成功/回归验收 |
|---|---|---|---|---|
| <a id="step-bq-00"></a>`BQ-00` | 基线、快照和冲突清单；盘点 `usage.rs`、`model_budget.rs`、`receipts.rs`、Provider response、CellRegistry、现有事件和测试 | — | 复现 `CostLedger`/`Quota`/预算类型混用、缺 usage 不得写 0；记录当前 RED，不篡改历史证据 | 形成 source snapshot、WIP 边界、事件/字段迁移表和 fixture 命名 |
| <a id="step-bq-01"></a>`BQ-01` | 稳定 ID、schema major、unknown/reason、状态枚举与错误码；`kiana-domain`/contracts | BQ-00 | 未知 major、未知枚举、重复 ID、跨 run/cell/project 绑定、负无符号金额全拒绝 | 所有 DTO round-trip；错误码稳定且不含 secret |
| <a id="step-bq-02"></a>`BQ-02` | `UsageVector` 和 `NormalizedUsage`；区分 absent/zero/partial、source/basis/sequence | BQ-01 | 缺失 usage 被当零、负数/溢出、unknown source 进入 measured、跨 attempt 混用全拒绝 | provider/local/fake 三类 usage 可序列化，字段含义表固定 |
| <a id="step-bq-03"></a>`BQ-03` | snapshot/delta/final stream 累计器；按 sequence 去重、单调性和包含关系校验 | BQ-02 | 重复 sequence 不同 digest、回退、截断、重复 cumulative 相加、终态后 delta 全拒绝 | cumulative 取最后快照、delta 恰好相加；usage-only chunk 保留 |
| <a id="step-bq-04"></a>`BQ-04` | `Money`、整数 micros、checked pricing arithmetic；`RateCard` 版本和有效时间 | BQ-01,BQ-02 | 浮点/负 unsigned、乘加溢出、重叠有效期、缺 currency/source、旧记录被当前价格重算全拒绝 | 相同 rate card 得到稳定 estimate；价格变更新增 version |
| <a id="step-bq-05"></a>`BQ-05` | `RateCardStore` 与模型/provider/缓存/音频/工具单价映射 | BQ-04 | unknown model、过期卡、cache/reasoning 重复计价、估算引用缺 version 全拒绝 | 受支持模型按 pinned card 生成可解释 breakdown；unknown price 保持 null |
| <a id="step-bq-06"></a>`BQ-06` | 五类预算合同和交集算法；`RuntimeBudget`、`BudgetLease`、`ProjectBudget`、`ProviderBudget` | BQ-01,BQ-04 | child union/提升、Continue 重置累计、FinancialBudget 误授予执行权、project/runtime 互换全拒绝 | 交集和 remaining 计算纯函数稳定；保留 `runtime_and_project_budgets_are_not_interchangeable` |
| <a id="step-bq-07"></a>`BQ-07` | Quota dimension/window、UTC/clock、quota group（alias/credential/model） | BQ-06 | 时钟回退、窗口错位、同 credential 别名绕过、空 scope/超限全拒绝 | 固定时钟下窗口滚动、quota group 聚合和 retry-after 可复现 |
| <a id="step-bq-08"></a>`BQ-08` | durable `QuotaReservation`、lease/fence/authority/config revision、CAS/dedup | BQ-06,BQ-07 | 并发 reserve 超卖、旧 epoch/fence、digest 冲突、重复 command、过期 permit 全拒绝且 0 dispatch | 两进程竞争只有一个 commit；replay 返回原 receipt；兄弟总预留不超 parent |
| <a id="step-bq-09"></a>`BQ-09` | admission estimator：最终 wire 请求、输出上限、retry allowance、tool/effect/storage 预算 | BQ-02,BQ-05,BQ-08 | 估算遗漏 schema/缓存/重试、unknown tokenizer 宣称 exact、价格未知仍硬性声称费用上限全拒绝 | estimate 带 basis/upper bound；fake provider 请求前能看到正确 reservation |
| <a id="step-bq-10"></a>`BQ-10` | Provider normalized usage adapters；Anthropic/OpenAI/Ollama/Gemini/Fake 字段包含关系 | BQ-02,BQ-03,BQ-05,BQ-09 | malformed usage、requested/served model 混淆、reported total 矛盾、provider 自报 token 直接授权全拒绝 | 每协议 fixture 映射到同一中立 vector；缺字段保留 unknown |
| <a id="step-bq-11"></a>`BQ-11` | Model attempt 生命周期和事件：prepared/dispatching/observed/settled/unknown | BQ-08,BQ-10 | 未 prepared、无 permit、同 attempt 多次 settle、append 未 flush 却 dispatch 全拒绝 | `run→turn→attempt→usage→receipt` 关联完整；失败 attempt 也留 usage/error |
| <a id="step-bq-12"></a>`BQ-12` | settlement/release/unknown fold；已知消费、未用预留和 result_unknown 分离 | BQ-11 | cancel/timeout/EOF 自动归零、unknown 自动释放、已知 usage 退款、重复 settle 双扣全拒绝 | known/partial/unknown 三路得到确定性账本；reconcile 前保守占用 |
| <a id="step-bq-13"></a>`BQ-13` | estimated/measured cost 计算和 Receipt breakdown；`receipts.rs`、query projector | BQ-05,BQ-12 | 无 RateCard/不完整 usage 写 measured、估算进入 FinancialBudget、cost=0 伪造全拒绝 | estimate 引用 rate-card version；measured 只带 provider receipt；unknown 显示原因 |
| <a id="step-bq-14"></a>`BQ-14` | append-only `CostLedgerEntry` 和 `CostCorrection` command/approval | BQ-13,CP-11,ER-12 | 原地 UPDATE/DELETE、模型文本改账、无 evidence/approval、target digest 不匹配全拒绝 | correction 只追加且可重放；原始/修正值和 approver 可追溯 |
| <a id="step-bq-15"></a>`BQ-15` | Tool/effect/resource usage；Broker invocation、shell/MCP、Artifact/log/storage 计量 | BQ-06,BQ-08,BQ-11 | 被拒绝声明计 effect、未启动工具计成功、路径/owner/lease 漂移、输出洪泛绕过 quota 全拒绝 | model/tool/effect 账目分层；实际启动数、字节和 wall time 与 Receipt 对齐 |
| <a id="step-bq-16"></a>`BQ-16` | Provider capacity、RPM/TPM、semaphore、bounded fair queue、backpressure | BQ-07,BQ-08,BQ-15 | 无界队列、取消后仍发送、退避占槽、别名绕过 quota、释放非 owner 许可全拒绝 | Accept/Queue/Delay/Reject 可观测；公平性、queue wait、RAII release 有测试 |
| <a id="step-bq-17"></a>`BQ-17` | Retry classifier、attempt reservation、Retry-After 和 cancellation | BQ-11,BQ-12,BQ-16,P4-J7-23 | post-send unknown 自动 retry、TLS/auth 永久错误重试、retry 超过 budget、cancel race 双终态全拒绝 | pre-send 429/408 按 bounded policy 新 attempt；实际请求数与账目一致 |
| <a id="step-bq-18"></a>`BQ-18` | 白名单 fallback 与 route/authority/data/price 重新准入 | BQ-05,BQ-08,BQ-10,BQ-17 | fallback 降低 capability、跨 data boundary、无 credential/预算或复用旧 permit 全拒绝 | 每个 fallback attempt 有独立 route、usage、rate-card 和 receipt |
| <a id="step-bq-19"></a>`BQ-19` | project/org/workflow/cell/run allocation；避免父子/多维重复相加 | BQ-13,BQ-14,BQ-15 | 同一 usage 多次扣费、跨项目无 SharingGrant、归属字段由 wire 自报全拒绝 | leaf usage 唯一；各维度 rollup 与总账一致；CO-45 portfolio 视图可重算 |
| <a id="step-bq-20"></a>`BQ-20` | EventLog ledger projector、source cursor、projection version、daily/window rollups | BQ-11,BQ-14,BQ-19,PD-05 | 先推进 cursor 再写 projection、跳过坏事件、projection 反写事实、stale 被标 fresh 全拒绝 | 新进程从 EventLog 重建同一汇总；失败 cursor/quarantine 可查询 |
| <a id="step-bq-21"></a>`BQ-21` | Restart/continue/replay/recovery；in-flight reservation、unknown attempt 和 fencing | BQ-08,BQ-12,BQ-20,ER-21 | 重启重发已完成 attempt、Continue 重置累计、旧 lease/epoch 继续结算、unknown 自动成功全拒绝 | 恢复默认 paused/needs-reconciliation；显式 reconcile 后只结算一次 |
| <a id="step-bq-22"></a>`BQ-22` | Provider invoice/receipt import、差异检测、correction workflow | BQ-05,BQ-14,BQ-20 | 未认证 receipt、重复 invoice、period/model/usage 不匹配、差异静默覆盖全拒绝 | 账单导入生成 measured/correction；差异、未知和待人工项可追踪 |
| <a id="step-bq-23"></a>`BQ-23` | query/protocol API：预算摘要、usage breakdown、cost export、reconciliation inbox | BQ-20,BQ-22 | 查询跨 DataBoundary、查询消费 reservation/approval、分页跳过 cursor、估算标实测全拒绝 | CLI/Web/Workbench 返回同一 projection、freshness、unknown 和 provenance |
| <a id="step-bq-24"></a>`BQ-24` | UI/入口展示与命令；只读预算卡、队列、超额原因、correction approval | BQ-23,UI-00 | UI 本地计算剩余额度、乐观扣费未 commit、隐藏 unknown、输入 actor/project 覆盖服务端全拒绝 | 同一 Run 在四入口显示一致；审批/重试/对账走 versioned command |
| <a id="step-bq-25"></a>`BQ-25` | Redaction、DataClass、telemetry separation；Event/Log/Metric/Trace/Receipt 安全 | BQ-11,BQ-13,BQ-23,OA-08 | API key、prompt、raw response、invoice secret、路径/高基数 ID 进入 metric 全拒绝 | digest/ref/低基数标签可关联；secret scan 和 redaction fixture 全绿 |
| <a id="step-bq-26"></a>`BQ-26` | 并发、崩溃、磁盘满、网络 EOF、provider 429/5xx、clock fault 注入 | BQ-08,BQ-12,BQ-16,BQ-20 | CAS race、partial frame、flush 失败、settlement 丢失、未知自动重试、超额继续 dispatch 全拒绝 | 故障后事实可重放，reservation/lease/queue 无泄漏；incident 有 action |
| <a id="step-bq-27"></a>`BQ-27` | Legacy usage/cassette/config upcaster 和迁移；旧 `CostLedger`/`Quota` 字段 | BQ-01,BQ-02,BQ-04,BQ-20 | unknown major 静默接受、旧 `cost_micros=0` 被当 measured、重复迁移覆盖历史全拒绝 | 旧 cassette 可读并标 `legacy/unknown`；迁移有版本、digest、rollback/read-only 说明 |
| <a id="step-bq-28"></a>`BQ-28` | 性能、容量、retention 和归档；usage/index/receipt 保留边界 | BQ-20,BQ-25,BQ-26 | rollup 无界内存、高基数标签、删除 retained evidence、archive 后仍可结算旧 lease 全拒绝 | 固定 fixture 测 p50/p95、磁盘/queue 上限；保留/归档不改账本事实 |
| <a id="step-bq-29"></a>`BQ-29` | GoldenTrace 与跨入口端到端：model→tool→event→receipt→invoice correction | BQ-21,BQ-22,BQ-23,BQ-24,BQ-26,BQ-27 | 任一入口绕过 DaemonHost、provider/handler 调用数与 reservation 不符、Receipt 把 runtime success 写 business outcome 全拒绝 | Fake provider、local executor、synthetic stream、known failure、unknown、retry、fallback、correction 全链可重放 |
| <a id="step-bq-30"></a>`BQ-30` | 发布门、状态账本和逐连接 live 证据；更新 `CURRENT_STATUS.md`/`module-map.md` | BQ-00..BQ-29 | 只有类型/单测/估算不能宣称 billing live；无 provider receipt 不能宣称 measured；限制未记录阻断 Promote | 每步有 evidence block；offline durable 与 opt-in live 分开；P1-K5/CP-11/P4-J7-24/ER-12/OA-08/CO-45 接点全部回填 |

### 35.8 执行波次和现有 roadmap 接点

```text
Wave A  Contracts:  BQ-00 → BQ-01 → BQ-02 → BQ-03 ∥ BQ-04 → BQ-05
Wave B  Budgets:    BQ-06 → BQ-07 → BQ-08 → BQ-09
Wave C  Attempts:   BQ-10 → BQ-11 → BQ-12 → BQ-13 → BQ-14
Wave D  Capacity:   BQ-15 → BQ-16 → BQ-17 ∥ BQ-18
Wave E  Rollups:    BQ-19 → BQ-20 → BQ-21 → BQ-22
Wave F  Surfaces:   BQ-23 → BQ-24 → BQ-25 → BQ-26
Wave G  Migration:  BQ-27 → BQ-28 → BQ-29 → BQ-30
```

与已有专项的边界固定如下：

| 已有卡片/专项 | 本节承接 | 不重复建设 |
|---|---|---|
| `P1-K5-01` | CostLedger、Quota、Runtime/Project 分层总入口 | 不再另建一套 usage ledger |
| `CP-11` / `CP-13/14` | budget reserve、effect admission、settlement 和 fencing | 不在 Provider/daemon 做独立授权 |
| `P4-J7-24/25` | 协议 usage 归一化、价格快照、Provider capacity/fallback | 不把 provider-specific usage 直接当 project cost |
| `ER-12` / `ER-21` | Receipt 聚合、Unknown、reconcile、replay | 不把 Receipt 当第二事实源 |
| `CO-45` | Project/Portfolio capacity 和成本视图 | 不把 runtime token 当业务收益或 FinancialBudget |
| `OA-08` / `OA-23/25` | cost/cache/retry metrics、eval、容量和迁移证据 | 不用 trace/metric 覆盖事实或授权 |
| `PD-05/10/11/20/24` | EventStore、projector、Budget/Lease、backup、retention | 不把 SQLite/index/cache 写成第二账本 |

若既有卡片与本节在 reservation、Unknown 或 measured/estimated 上有冲突，采用更严格的规则：**先提交事实和预留，后产生副作用；Unknown 保守挂账；只有带 receipt 的 measured 才能对账；所有 correction 追加且需授权。**


### 35.9 最低验收矩阵、指标和证据口径

**拒绝矩阵。** 至少覆盖：未信任项目、伪造 actor/project/role、budget 类型互换、空或未知 scope、窗口回退、并发超卖、旧 epoch/fence、重复 command/attempt/sequence、缺失 usage 被归零、累计 usage 重复相加、价格缺失却标 measured、RateCard 重叠/改写、post-send unknown 自动 retry、取消后仍发送、退避占槽、fallback 跨 DataBoundary、被拒工具计 effect、project/cell 多重扣费、账单 receipt 重放、correction 无审批、projection cursor 跳过、查询越权、secret/高基数泄漏和任一入口绕过 `DaemonHost`。每个拒绝都要有稳定 error/reason、事件或 incident，并证明 provider/broker handler effect 数为零。

**成功和恢复矩阵。** 至少覆盖：known usage、显式 zero、partial/unknown usage、cumulative/delta stream、缓存和 reasoning 字段、多个并发 reservation、queue fairness、cancel/timeout、pre-send retry、白名单 fallback、model+tool+artifact 账目、跨 Run/Project/Organization rollup、重启与 Continue、EventLog 重建、invoice measured、CostCorrection、retention/archive、CLI/TTY/Web/Desktop 同一 Receipt。没有 provider receipt 的真实调用最多提升到 `durable` 的本地事实链，不能提升为外部 bill settled 或业务 outcome。

**Canonical 指标。** 只注册低基数标签（`provider_id`、`model_id`、`route_id`、`reason_class`、`outcome`、`sandbox_profile`、`window`、`environment`）；`run_id`、`attempt_id`、prompt/tool hash、路径和主体 ID 只进事件/Receipt 查询。至少提供：

```text
kiana.usage.input_tokens / output_tokens / cache_read_tokens / cache_write_tokens
kiana.usage.requests / tool_calls / effects / wall_time_ms / bytes
kiana.cost.estimated_total / measured_total / unknown_total
kiana.quota.reserved / consumed / released / unknown / rejected
kiana.capacity.queue_depth / queue_wait_ms / active / retry_after
kiana.cost_per_accepted_delivery
kiana.workflow_retry_cost / swarm_duplicate_work_cost / budget_burn_rate
```

`estimated_total` 和 `measured_total` 永远分开；没有 `provider_receipt_ref` 的金额不能进入 measured。`cost_per_accepted_delivery` 只在 accepted delivery 非零且 project/data boundary 一致时计算，否则返回 unknown；平均 token 不是质量指标。

**证据块。** 每张 `BQ-*` 完成时在 `CURRENT_STATUS.md` 写入：

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

`feature_status` 和 `proof_level` 分开填写。`UsageRecord` 类型、一次本地单测、Receipt 字段、RateCard 文件或 Provider 返回的 token 都不能单独提升状态。发生 unknown、迁移、projection 落后、invoice 缺失、价格未覆盖、连接未实跑或 retention 限制时，Receipt、query 和证据块都必须保留可定位的 limitation；本专项只补设计和执行步骤，不改变现有实现状态。

<a id="deployment-operations-migration-design"></a>

## 36. 部署、运维与迁移专项：实际代码设计、处理流程与详细实施步骤（2026-09-14 追加）

> 本节补全 [`module-map.md`](module-map.md) 第 19 模块。它把 `kiana-daemon`、`kiana-core`、`kiana-eventlog`、`kiana-query`、配置/身份、调度、可观测性和发布脚本串成一条可执行的部署生命周期。这里的 `DEP-*` 是本专项的局部实施索引，不改变 P0–P6 或其他专项的 canonical step，也不把当前脚本、类型或单机 smoke 误写成已经具备 durable/live 的云部署能力。

### 36.1 调研结论：可借鉴的机制与明确边界

这次调研先看完整的 `reference/` 和 `docs/reference-agent-audit/`，再对照 Temporal、SQLite、Kubernetes、Flyway 和 12-factor agents 的公开机制。参考项目只提供形状和失败模式；Kiana 仍以自己的 `DaemonHost → ControlPlane → Broker → EventStore → Receipt` 为唯一事实与副作用路径。

| 来源 | 观察到的机制 | Kiana 采用 | 不直接照搬的部分 |
|---|---|---|---|
| [`00-unified-agent-flow`](reference-agent-audit/00-unified-agent-flow.md)、[`01-codex`](reference-agent-audit/01-codex.md)、[`02-deepseek-harness`](reference-agent-audit/02-deepseek-harness.md) | 事件优先、flush/shutdown、torn-tail 修复、恢复前重新授权、取消后排空已启动工作 | 把 deployment operation、drain、backup、migration 也写成可审计的事实和 Receipt；恢复默认暂停并保留 `Unknown` | 不把 transcript、后台 writer 或内存 snapshot 当作权威；不假设静态审计已经证明真实部署时序 |
| [`07-goose`](reference-agent-audit/07-goose.md)、[`08-opencode`](reference-agent-audit/08-opencode.md)、[`24-metagpt`](reference-agent-audit/24-metagpt.md)、[`11-crush`](reference-agent-audit/11-crush.md) | 持久 session/state、projector 与 source 分离、文件系统恢复和 terminal 事实 | 使用 source cursor、generation、projection rebuild、quarantine root 和显式 reconcile | 不引入第二个 session store、UI 状态源或自动重试副作用 |
| [`12-factor-agents`](../reference/12-factor-agents/README.md) | 确定性控制流、部署动作由工具/人工批准驱动、先检查环境和版本 | 把发布、迁移、回滚拆成可审批的 operation plan；计划由代码生成，执行仍回到 ControlPlane | 不让模型决定版本、环境、权限或发布顺序；不把示例 deploy tool 当作 Kiana 的连接器实现 |
| [`container-use`](../reference/container-use/docs/README.md) | environment 持有状态，副作用经过受控 environment，发布有明确 release/checksum 步骤 | 以 `StorageRoot`、`EnvironmentProfile`、`ReleaseManifest`、artifact digest 作为部署边界 | 不把容器本身当作安全边界；容器内仍须有 lease、sandbox、policy 和审计 |
| [`temporal-sdk-python`](../reference/temporal-sdk-python/README.md) 与 [worker versioning](https://github.com/temporalio/documentation/blob/main/docs/production-deployment/worker-deployments/worker-versioning.mdx) | deterministic workflow replay、worker build ID、pinned/auto-upgrade、旧 worker drain | 对 WorkflowDefinition、Provider、Skill/Plugin 保存 digest 和兼容窗口；运行实例固定版本，旧 revision drain 后才退休 | 不引入 Temporal server、外部 exactly-once 或任意 replay-safe 的假设 |
| [SQLite backup API](https://www.sqlite.org/backup.html)、[WAL](https://www.sqlite.org/wal.html)、[atomic commit](https://www.sqlite.org/atomiccommit.html) | 备份必须形成一致快照；WAL、主文件和 checkpoint 需要共同处理；单写者与锁语义影响恢复 | JSONL/WAL/SQLite adapter 都输出统一 `BackupManifest`、cursor、epoch、hash 和 restore verification | 不把 SQLite/ WAL 作为第二事实账本；当前适配器没有因此自动获得生产 durability |
| [Kubernetes probes](https://kubernetes.io/docs/concepts/workloads/pods/probes/)、[rolling update](https://kubernetes.io/docs/tasks/run-application/update-deployment/)、[health checks](https://kubernetes.io/docs/reference/using-api/health-checks/) | startup、readiness、liveness 语义不同；readiness 可摘流；滚动更新要有进度、暂停和回滚 | 定义 `startupz/readyz/livez/drainz` 等同一 `HealthSnapshot` 的投影，并将 rollout 分成 preflight、drain、serve、rollback | 不宣称当前已有 Kubernetes controller；orchestrated profile 只是目标适配器，单写者和数据根 fencing 先于副本数 |
| [Flyway validate](https://documentation.red-gate.com/flyway/reference/commands/validate) | migration checksum、版本顺序和 drift 检测是发布门 | MigrationRegistry 保存 checksum、前置版本、owner、兼容窗口和 `MigrationRecord` | 不采用可随意编辑的 SQL 目录；事实数据迁移默认 forward-only，降级走 restore/兼容二进制 |
| `beads`、`a2a`、`graphiti/mem0` 等审计材料 | claim/lease、TaskState、artifact 引用、时间/操作生命周期 | 复用 stable ID、lease/fence、artifact ref、source cursor 和 retention 术语 | 不把外部 TaskState、claim API 或 memory store 变成 Kiana 的权限事实源 |

从这些机制可归纳出六条工程结论：

1. 部署、迁移、备份、恢复和运维命令都是 ControlPlane command；运维适配器只能提交 intent、读取 snapshot 或执行已经授权的 permit。
2. 版本必须拆成 application build、protocol/domain schema、store format、projection、workflow/provider/extension、config、authority epoch 和 data epoch；单个 `version` 字段不能覆盖兼容性。
3. 任何会改变事实或可能产生外部副作用的操作，都先写 admission/preflight，再写 committed operation，最后才执行；`result_unknown` 不得被当作失败自动重试。
4. 一个 `StorageRoot` 同时只允许一个有效 writer/lease。新进程、新容器或 restore root 必须用 fencing token 和更高 epoch 使旧实例失效。
5. readiness 只表示当前 revision 可以接收新的受控命令，不表示所有历史业务已经完成；projector 落后、migration 未完成、存在未对账 Unknown 或没有有效 lease 时必须保持未 ready。
6. 备份和回滚证明的是本机事实链可以恢复；外部 provider、webhook、支付或用户业务 outcome 需要独立的 effect receipt/reconcile 证据。

### 36.2 总体边界、部署形态与权威对象

#### 部署形态

四种 profile 共享同一个 `DaemonHost` 和 ControlPlane，只替换宿主生命周期与存储适配器：

| Profile | 目标场景 | 进程/存储约束 | 当前口径 |
|---|---|---|---|
| `embedded-local` | CLI、Workbench、Desktop 的本地运行 | 一个 `DaemonHost`、一个 `StorageRoot`、本地 JSONL/文件 artifact；退出前 drain + flush | 当前路径的主要兼容目标；需用 `DEP-00` 基线确认实际证明等级 |
| `managed-local` | systemd、launchd、Windows service 或桌面后台服务 | supervisor 只重启进程，不保存业务状态；数据根、配置根、secret ref 分离 | 目标适配器；不得在 supervisor 中复制执行循环 |
| `container` | OCI image、单机 compose、带持久卷的任务 | image immutable；`StorageRoot` 挂载；单 writer lease；健康探针和优雅终止可观察 | 目标适配器；容器编排不自动提供数据一致性 |
| `orchestrated` | Kubernetes 或其他编排器 | replica、rollout、PDB、探针、卷和身份均由 adapter 映射，仍由 Kiana lease/fence 决定 active writer | deferred/target；在 durable store、identity、backup 和 migration gate 完成前不得宣称可用 |

部署 profile 不能新增一条模型循环或权限判断。Supervisor 可以发送 `SIGTERM`、停止容器或摘除服务，但“是否允许停止/迁移/回滚”由 operation admission 决定；宿主动作的结果通过 `OperationObservation` 回写。

#### 权威对象

| 对象 | 必填字段 | 权威位置与用途 |
|---|---|---|
| `ReleaseManifest` | `release_id`、git/source digest、target、build/toolchain、Cargo.lock digest、protocol/schema/store/projection version、minimum/maximum compatible versions、artifact digests、signature refs | 发布 artifact 与 CI 产物；不可由运行时自述覆盖 |
| `EnvironmentProfile` | environment ID、StorageRoot ref、config revision、secret refs、allowed capabilities、platform/filesystem、maintenance policy | `kiana-daemon` 解析后的不可变快照；secret value 不进入事件或诊断 |
| `DeploymentRevision` | revision/build ID、release ID、profile、instance ID、parent revision、phase、health、start/drain/retire timestamps | EventStore 的部署生命周期事实；旧 revision 可查询但不可接收新工作 |
| `OperationLease` | operation ID、owner/instance ID、StorageRoot、fence token、authority/data epoch、expires/heartbeat、scope | 单 writer、migration、backup、restore、rollout 的互斥和过期判断 |
| `HealthSnapshot` | startup/live/ready/draining、source cursor、projection cursor、migration/backup/lease 状态、capacity、reason codes | 由 daemon/core 聚合，CLI/Web/Desktop 只投影；不能由 HTTP 200 自行推断 ready |
| `BackupManifest` | backup ID、root/store identity、source cursor、generation、epochs、file/chunk hashes、artifact refs、config revision、encryption/key ref、created/verified | 恢复和审计的不可变材料；不含 secret 明文 |
| `MigrationPlan` / `MigrationRecord` | from/to schema/store/projection、ordered steps、checksums、preflight result、backup ID、owner、started/completed/failed、resume token | EventStore migration registry；一个版本只允许一次兼容记录，checksum drift fail-closed |
| `RunbookEvidence` | operation ID、source snapshot、commands、fixture/cassette、exit code、status/proof level、limitations、reviewer | `CURRENT_STATUS.md` 与 artifact manifest 的证据投影，不把文字说明当事实源 |

#### 版本轴与兼容矩阵

所有启动、迁移、恢复和 rollout 都计算下面的 tuple，并把结果写入 preflight receipt：

```text
(app_build, protocol_version, domain_schema, store_format, projection_version,
 workflow_definition_digest, provider_route_digest, extension_digest,
 config_revision, authority_epoch, data_epoch, generation)
```

`app_build` 可用于替换进程，但不能单独证明数据兼容；`store_format` 与 `domain_schema` 的 major 不兼容时不得启动；`projection_version` 落后可以停在 `rebuilding`，不能接受新业务命令；workflow/provider/extension 版本必须通过 run pin 或兼容窗口；`authority_epoch`、`data_epoch` 或 `generation` 回退视为 stale root。

### 36.3 生命周期状态机与统一处理流

#### 状态机

```text
discovered
  -> preflight
  -> quiescing -> draining -> backed_up
  -> migrated -> starting -> ready -> serving
  -> maintenance/draining -> stopped

任何阶段 --拒绝/失败--> needs_recovery 或 degraded
serving --外部结果不确定--> result_unknown（保持事实，不自动 retry）
restore --验证前--> quarantine；验证通过后才可 activate
```

`ready`、`serving`、`degraded`、`needs_recovery`、`result_unknown` 是不同维度：健康状态不能覆盖 operation 状态，operation 状态不能覆盖外部 effect 状态。终态只由带相同 `operation_id`、`revision`、`fence token` 的事实推进；late observation 不得 resurrect 已停止的 revision。

#### 安装与启动

```text
resolve executable + EnvironmentProfile + secret refs
  -> verify ReleaseManifest/signature/digest/toolchain
  -> resolve StorageRoot and ProjectTrust
  -> acquire OperationLease and fence stale instance
  -> read store header, scan frames/WAL/artifacts, classify integrity
  -> migration/compatibility/capacity/clock/filesystem preflight
  -> rebuild or catch up projector/indexes
  -> publish startup HealthSnapshot
  -> ready only when cursor, epoch, policy and drain state are valid
```

启动失败要返回稳定错误码和 remediation（例如 `store_major_unsupported`、`lease_conflict`、`migration_required`、`backup_missing`、`projection_lag`、`trust_denied`），不能只返回泛化 I/O 错误。

#### 正常运行、维护与关闭

```text
ready -> accept command -> ControlPlane admission -> canonical commit
      -> dispatch prepared permit -> append observation/effect receipt
      -> project -> metrics/logs/receipt

maintenance requested -> reject new work -> stop scheduler intake
  -> drain started work -> reconcile Unknown -> flush EventStore/artifacts
  -> write stopped/drained fact -> release lease -> supervisor may stop
```

关闭超时不得伪造 `stopped`；只能记录 `drain_timeout`，保留 active/unknown execution 和 lease evidence，下一次启动进入 `needs_recovery`。

#### 迁移与发布

```text
candidate ReleaseManifest
  -> read-only preflight + compatibility matrix
  -> acquire migration/operation lease
  -> create and verify backup
  -> quiesce/drain old revision
  -> apply ordered idempotent migration steps
  -> rebuild projector/index and verify source cursor/generation
  -> start new revision with pinned workflow/provider routes
  -> readiness gate + canary/blue-green observation
  -> promote, or fence/drain/restore previous revision
```

事实数据迁移默认 forward-only；需要回退时优先恢复已验证的旧 root 或启动仍兼容的旧二进制。没有 verified backup、旧 revision 仍可能写入、checksum drift、未知 migration 或未对账 effect 时，rollback/promotion 都拒绝。

### 36.4 实际代码设计与接口边界

#### crate 落点

| 层 | 代码设计 | 不能做什么 |
|---|---|---|
| `kiana-domain` | `ReleaseId`、`DeploymentRevision`、`OperationLease`、`FenceToken`、`HealthSnapshot`、`BackupManifest`、`MigrationRecord`、状态转移和稳定错误码 | 不读取文件、网络、进程或 secret；不持有 Tokio runtime |
| `kiana-protocol` | `kiana.protocol.v1` 下的 ops command/query/event DTO、operation receipt、health/backup/migration payload、unknown/error envelope | 不接受 caller 自报的 actor/role/epoch 作为权威；不直接调用 handler |
| `kiana-ports` | `StorageRootResolver`、`ClockPort`、`LeaseStore`、`HealthPort`、`BackupPort`、`MigrationPort`、`SupervisorPort`、`ConfigSource` 的窄接口 | 不把 daemon、provider、shell 或编排器依赖下沉到 domain |
| `kiana-core` | operation admission、ProjectTrust、policy/gate/approval、epoch/fence、maintenance/drain、migration decision、restore activation、reconcile | 不在 core 之外做第二次权限判断；不把“请求 supervisor”当作已完成 |
| `kiana-eventlog` | operation facts、flush ack、cursor/generation、journal/WAL/JSONL scan、CAS/dedup、snapshot manifest、migration registry | append 成功不等于外部 effect 成功；事实不可由 projector/cache 改写 |
| `kiana-daemon` | `DaemonHost` 内的 lifecycle coordinator、bounded channels、probe aggregator、supervisor adapter、config/root resolution、startup/shutdown | 不另起模型循环、scheduler 或 capability dispatch；不将宿主 pid 当唯一事实 |
| `kiana-query` | deployment/health/backup/migration/incident projections，按 source cursor 和 generation 查询 | 查询不取得 lease、approval 或 capability；projection 可删除并重建 |
| `kiana-workflow` / `kiana-runner` / `kiana-provider` | workflow/build pin、replay compatibility、drain/cancel/stop observation、provider route digest | 旧 run 不可静默切到新 definition/provider；未知 effect 不自动 retry |
| `scripts/`、`.github/workflows/`、`contrib/desktop/` | artifact build/sign/checksum、release smoke、安装/卸载/探针/服务适配、runbook command | 脚本不能成为事实源或绕过 ControlPlane；CI green 不等于 live deployment |

#### 领域/端口草案

下面的形状用于指导实现，字段名可在 protocol review 时调整，但语义不可省略：

```rust
struct DeploymentRevision {
    revision_id: RevisionId,
    release_id: ReleaseId,
    profile: DeploymentProfile,
    instance_id: InstanceId,
    storage_root: StorageRootId,
    build_id: BuildId,
    data_epoch: DataEpoch,
    authority_epoch: AuthorityEpoch,
    phase: DeploymentPhase,
}

struct OperationLease {
    operation_id: OperationId,
    owner: InstanceId,
    fence: FenceToken,
    storage_root: StorageRootId,
    expires_at: Timestamp,
    heartbeat_seq: u64,
}

trait DeploymentPort {
    async fn preflight(&self, request: PreflightRequest) -> Result<PreflightReport, DeployError>;
    async fn observe(&self, operation: OperationId) -> Result<OperationObservation, DeployError>;
    async fn drain(&self, lease: OperationLease) -> Result<DrainReport, DeployError>;
}
```

`DeploymentPort` 只描述宿主动作和观察结果；它不能直接获得 `CapabilityRequest`，也不能把 `PreflightReport` 伪造成 `Committed`。`BackupPort`、`MigrationPort`、`HealthPort` 同样返回结构化结果、source cursor、fence 和 evidence refs。

### 36.5 健康、配置、凭据与运维命令

#### 健康语义

| 探针/状态 | 判断内容 | 失败动作 |
|---|---|---|
| `startupz` | executable、config、trust、store header、lease、migration preflight、projector bootstrap 是否完成 | 不进入 ready；返回 reason code 和 operation id |
| `livez` | 进程事件循环、ControlPlane、EventStore 写入/读取线程是否仍能响应；不把 provider 可用性混入 | supervisor 可重启；重启前仍执行受控 drain |
| `readyz` | 当前 revision 是否可接收新命令：lease/fence、policy epoch、projection lag、capacity、migration、Unknown gate 均合格 | 摘流/拒绝新 admission；不自动杀进程 |
| `drainz` | 是否已停止新 intake、started work 是否排空、flush ack 是否完成、lease 是否释放 | 未完成时保留 `draining`，超时转 `needs_recovery` |
| `maintenance` | backup、migration、restore、repair、retention 的 operation phase、owner、deadline 和 next action | 维持受控拒绝；不得由 UI 直接继续 |

`HealthSnapshot` 必须携带 `observed_at`、monotonic sequence、source/projection cursor、revision/build、storage root、epoch、reason codes、redacted diagnostics 和 `proof_level`。`readyz=200` 只代表 admission ready，不代表业务 outcome 或外部连接器健康。

#### 配置与凭据

配置解析顺序固定为：compiled defaults → user/KIANA_HOME file → project file（先 ProjectTrust）→ explicit environment allowlist → command-line override（仅本次 operation）。解析后生成 immutable `ConfigSnapshot { config_revision, source_refs, redacted_digest, capabilities }`；运行过程中不原地修改。secret 只以 `SecretRef` 和 provider handle 存在，manifest、EventLog、health、backup、doctor 输出都只能出现 redacted ref/digest。

配置变更先生成 diff 和影响面（是否需要 restart、migration、lease 或重新审批），再由 ControlPlane 提交 `ConfigRevisionCommitted`；旧 run 使用创建时的 config revision，不能被隐式改写。

#### 建议的运维命令面

这些命令是 versioned protocol command 的 CLI 投影，Workbench/Web/Desktop 复用相同 DTO；命令本身不直接执行副作用：

```text
kiana ops status [--json]
kiana ops doctor [--json] [--include-evidence]
kiana ops preflight --release <id> [--profile <id>]
kiana ops drain --reason <reason> [--deadline <duration>]
kiana ops backup create|list|verify [--scope <root>]
kiana ops restore verify|plan|activate --backup <id>
kiana ops migrate plan|preflight|apply|resume|verify --to <version>
kiana ops projector status|rebuild|verify --from <cursor>
kiana ops reconcile list|show|commit --operation <id>
kiana ops rollout status|pause|resume|promote|rollback --revision <id>
kiana ops maintenance open|close --window <id>
```

`plan/preflight/status/verify` 是只读或生成待授权计划；`apply/activate/promote/rollback/reconcile/maintenance` 必须带 actor、scope、authority/approval、当前 epoch 和 operation id，并在提交前再次 CAS 检查。`doctor` 可暴露 remediation，但不能替用户自动修复。

### 36.6 备份、恢复、迁移与回滚的实际处理规则

#### 备份

1. `backup create` 先检查 trust、容量、retention/legal hold、operation scope 和 active lease；没有稳定 source cursor 或存在未处理的 torn tail 时拒绝。
2. 进入 `quiescing`，停止新的 scheduler intake，等待已开始的 append/flush/observation；不需要停止纯查询，但要记录 query cursor 和 projector lag。
3. 对 EventLog/JSONL、SQLite/WAL adapter、ArtifactStore、projection checkpoint、config revision 和 migration registry 生成 manifest。每个文件/块有大小、mtime（仅诊断）、SHA-256、logical cursor、generation 和相对路径；路径必须经过 StorageRoot resolver，禁止 symlink/hardlink escape。
4. manifest 写入临时文件后 fsync，再原子 rename；备份对象自身另有 digest/signature/encryption key ref。secret 不复制到 manifest，外部 secret 通过 ref 在恢复时重新绑定。
5. 备份完成后写 `BackupCreated` 与 `BackupVerified`；若任何对象只达到 local_behavior，manifest 必须标出限制，不能写 `durable`。

增量备份以 source cursor/generation 和 artifact chunk digest 为边界；不能只依赖 mtime。retention 由 backup ID、保留窗口、legal hold、依赖链和删除传播共同决定；删除一个父备份前必须证明所有增量仍可恢复或被另一个完整备份覆盖。

#### 恢复

```text
select backup -> verify signature/hash/manifest schema
  -> restore into new quarantine StorageRoot
  -> scan facts/WAL/artifacts and rebuild projections/indexes
  -> verify cursor/generation/epochs/receipt invariants
  -> reconcile in-flight/Unknown external effects
  -> acquire new lease and fence old root/revision
  -> explicit activate -> ready gate
```

恢复不能覆盖当前活动 root；必须先使用新 root 完成验证。`authority_epoch`、`data_epoch` 或 generation 比当前活动 root 小时拒绝激活；相同 identity 但不同 hash 进入 quarantine；丢失 artifact、cursor gap、migration checksum drift、未解析 Unknown 或 secrets 无法重新绑定都保持 `needs_recovery`。激活后旧 root 只读保留至 retention 到期，不能立即删除。

#### 迁移

MigrationRegistry 每条记录包含 `migration_id`、from/to tuple、checksum、ordered step list、precondition、reversible classification、owner、required backup、compatibility window 和 source cursor bounds。runner 算法：

```text
read-only preflight
  -> verify release/store/schema/projection compatibility
  -> verify required backup and free space
  -> acquire migration lease/fence
  -> record MigrationStarted
  -> run ordered, bounded, idempotent steps
  -> checkpoint after each step (cursor + checksum + resume token)
  -> rebuild/verify projections and indexes
  -> record MigrationCompleted and new data_epoch
```

迁移脚本只能通过 `MigrationPort` 读写受控 storage，不可以调用 provider、shell、MCP、webhook 或模型。推荐 expand → backfill → verify → switch → contract；compatibility window 内旧/新 revision 的 wire/schema 交集必须明确。遇到 checksum 改变、未知版本、并发 runner、空间不足、step partial、downgrade 请求或 fence 过期，runner 停止并记录 `MigrationBlocked`，可从 checkpoint resume，不得跳过失败 step。

#### 回滚和 result reconciliation

rollback 分为三种：

| 类型 | 允许条件 | 处理 |
|---|---|---|
| binary rollback | store/schema 向后兼容，或新版本只完成 expand | fence 新 revision、恢复旧 build、保持旧 workflow/provider pin，重新过 ready gate |
| data rollback | 有 verified backup 且已停止所有 writer | 新 root restore + verify + explicit activate；旧 root 保留审计 |
| effect reconciliation | provider/webhook 已发出但结果未知 | 不回滚本机事实来掩盖外部动作；进入 reconcile case，先查 external idempotency key/receipt，再 commit `reconcile`、`retry_without_effect`、`abandon` 或 `compensate` |

没有可验证 backup 或存在可能仍写入的旧 revision 时，`rollback` 只生成拒绝 receipt。任何自动化 retry 都要重新检查 authority、approval、budget、path lock、config revision、workflow digest 和 idempotency descriptor。

### 36.7 观测、告警、容量与事故处理

部署运维指标与产品 EventLog 分开建模，但都关联 `operation_id`、`revision_id`、`storage_root`、`source_cursor` 和 `trace_id`：

| 面 | 最低字段/指标 | 告警或动作 |
|---|---|---|
| 生命周期 | startup duration、ready transitions、drain duration、restart count、lease heartbeat age/fence conflicts | startup timeout、频繁重启、lease split-brain 进入 degraded/needs_recovery |
| 存储 | append/fsync latency、queue depth、disk bytes/inodes、torn-tail count、cursor gap、WAL/checkpoint size | 容量阈值触发 backpressure；完整性异常停止 admission |
| 投影 | source cursor、projection cursor、lag、rebuild duration、failed projection count | lag 超阈值未 ready；projector 失败只重建，不改事实 |
| 备份/恢复 | last verified backup、age、bytes、hash failures、restore verification duration、RPO/RTO | 过期/未验证备份阻断 migration/rollout；restore fail 保持 quarantine |
| 迁移/发布 | current/target version、step/row progress、checksum、phase duration、canary sample、rollback count | 进度 deadline、checksum drift、回滚阈值触发 pause |
| 安全/合规 | trust denial、secret redaction failure、signature failure、policy denial、audit append failure、retention/legal hold errors | 任一红线错误 fail-closed，并生成 incident evidence |

容量策略必须有硬上限和明确 backpressure：EventLog append、artifact bytes、pending operations、backup size、migration batch、diagnostic output、log rate 都不能无限增长。达到阈值时新 admission 返回结构化 `capacity_exceeded`，已开始的 append/flush 仍完成或转 `Unknown`；不通过删除事实来恢复空间。

事故状态建议使用 `observed → triaged → contained → recovering → verified → closed`，每次转移引用 operation/evidence。runbook 至少包含：症状、影响范围、只读诊断、停止/摘流条件、恢复/迁移/回滚决策树、所需审批、验证命令、RPO/RTO、复盘和证据归档。自动告警不能直接扩大 capability scope 或执行补偿。

### 36.8 详细实施步骤（`DEP-00`–`DEP-41`）

每个步骤先证明拒绝路径，再证明成功路径；依赖项只表示实现顺序，不表示已完成。步骤中的 “代码落点” 是建议归属，集成负责人最终按现有 crate 边界调整。

#### 波次 A：合同、身份与发布输入

| Step | 代码落点与目标 | 依赖 | 先拒绝的验收 | 成功路径与证据 |
|---|---|---|---|---|
| <a id="step-dep-00"></a>`DEP-00` | 盘点 `module-map`、`CURRENT_STATUS`、release scripts、DaemonHost、EventLog、现有 schema/migration/WIP；建立 source snapshot 与缺口分类 | — | 发现第二 execution loop、直接 supervisor/capability 入口或状态账本冲突即阻断 | 形成代码/脚本/平台矩阵、fixture 名称、proof ceiling 和限制清单 |
| <a id="step-dep-01"></a>`DEP-01` | 在 `kiana-domain` 定义 `DeploymentProfile`、`EnvironmentProfile`、`StorageRootId`、`InstanceId`、`DeploymentRevision` | DEP-00 | 空 root、跨 project root、未信任 project、未知 profile、路径 escape 全拒绝且无写入 | 同一输入得到稳定 profile digest；local/container/orchestrated profile round-trip |
| <a id="step-dep-02"></a>`DEP-02` | 定义 `ReleaseManifest`、artifact digest/signature、build/toolchain/Cargo.lock/source provenance | DEP-00 | digest/signature/toolchain/target 不匹配、manifest 缺字段、secret 明文均拒绝 | release manifest 可由 CI 生成、验证、redact 并绑定 artifact |
| <a id="step-dep-03"></a>`DEP-03` | 定义 app/protocol/domain/store/projection/workflow/provider/extension/config/authority/data 兼容矩阵 | DEP-01, DEP-02 | unknown major、downgrade、schema/store mismatch、workflow digest 漂移不得启动或接新工作 | 兼容/不兼容结果有稳定 reason code 和可重放 fixture |
| <a id="step-dep-04"></a>`DEP-04` | 在 `kiana-protocol` 注册 `ops.*` commands、query、events、error/unknown envelope、idempotency key | DEP-01 | caller 伪造 actor/epoch、重复 operation 不同 digest、未知 command 或超 scope 0 broker calls | CLI/Web/Workbench/Desktop 都能 round-trip 同一 DTO |
| <a id="step-dep-05"></a>`DEP-05` | 定义 lifecycle/operation 状态机、OperationJournal、phase deadlines、terminal/unknown 语义 | DEP-04 | 旧 revision late event、跳过 preflight、drain timeout 写 stopped、重复 terminal 均失败 | replay 可重建 operation；每次状态有 source cursor/reason/evidence |
| <a id="step-dep-06"></a>`DEP-06` | 实现 `OperationLease`、heartbeat、fence token、authority/data epoch 与单 writer CAS | DEP-01, DEP-05 | stale owner、过期 lease、错误 fence、epoch 回退、双 writer 均 0 dispatch | 两个进程竞争只有一个 active；安全失效后可重新 acquire |
| <a id="step-dep-07"></a>`DEP-07` | 实现 StorageRoot resolver、ProjectTrust、symlink/hardlink/path/capability/filesystem preflight | DEP-01, DEP-06 | 未信任路径、root escape、远程/不支持 FS、权限/空间/inode 不足均 fail-closed | root identity、mount/capability 和 remediation 可查询 |

#### 波次 B：配置、启动、健康与关闭

| Step | 代码落点与目标 | 依赖 | 先拒绝的验收 | 成功路径与证据 |
|---|---|---|---|---|
| <a id="step-dep-08"></a>`DEP-08` | 实现 ConfigSource 优先级、immutable `ConfigSnapshot`、config revision、SecretRef/redaction | DEP-01, DEP-02, DEP-07 | 未允许 env、project config 未过 trust、secret 出现在 log/manifest、运行中隐式改配置均拒绝 | 配置 diff、影响面、redacted digest 和 restart/migration requirement 稳定 |
| <a id="step-dep-09"></a>`DEP-09` | 定义 `SupervisorPort`，接入 systemd/launchd/Windows/container stop/start/restart 的窄 adapter | DEP-04, DEP-05, DEP-06 | adapter 直接调用模型/Capability、pid 代替 lease、强杀未生成 observation 均失败 | fake supervisor 可验证 signal、timeout、observation 和重启 fencing |
| <a id="step-dep-10"></a>`DEP-10` | 在 `DaemonHost` 内实现 startup coordinator：manifest/root/trust/lease/store/migration/projector/capacity 顺序 | DEP-03, DEP-06, DEP-07, DEP-08 | 任一 preflight 未完成仍发布 ready、旧 epoch 自动 resume、坏 journal 自动覆盖均失败 | 启动失败 reason 可重放；成功启动得到 `startupz` evidence |
| <a id="step-dep-11"></a>`DEP-11` | 实现 `HealthSnapshot` aggregator 与 startup/live/ready/drain/maintenance probe DTO | DEP-05, DEP-10 | provider 健康伪造 ready、projection lag/Unknown/lease conflict 被隐藏、HTTP 200 代替事实均失败 | fake clock/fixture 驱动 probe transitions；CLI/UI 只读 projection |
| <a id="step-dep-12"></a>`DEP-12` | 实现 ready admission、maintenance window、pause intake、drain deadline 与摘流语义 | DEP-05, DEP-06, DEP-11 | maintenance 中仍接新工作、过期 window 自动延长、ready 与 migration/backup 并存均拒绝 | 新命令被结构化拒绝；已开始工作可继续到 drain/reconcile |
| <a id="step-dep-13"></a>`DEP-13` | 把 cancellation、scheduler stop、runner/tool drain、EventStore/artifact flush ack 接入统一 shutdown | DEP-09, DEP-12 | flush 未确认写 stopped、started work 未排空、强杀丢 ack、late result resurrect 均失败 | 正常/超时关闭分别得到 stopped 或 needs_recovery，重启不重复 effect |
| <a id="step-dep-14"></a>`DEP-14` | 接入 lifecycle/operation metrics、structured logs、trace/evidence refs 和 audit event schema | DEP-04, DEP-05, DEP-11 | secret/path 泄漏、无 operation/revision/cursor 关联、audit append 失败被吞掉均阻断 | status/health/receipt/metrics 可用同一 operation 查询 |
| <a id="step-dep-15"></a>`DEP-15` | 实现 `ops status/doctor/preflight`，输出 redacted diagnostics、remediation 和 reproduction command | DEP-08, DEP-11, DEP-14 | doctor 修改事实、显示 secret、将未知/缺证据写成 healthy 均失败 | JSON/human 输出稳定，路径和限制脱敏，exit code 可用于 CI |
| <a id="step-dep-16"></a>`DEP-16` | 实现 projector/index/queue/lease repair 与 `ops reconcile` 只读检查/显式提交 | DEP-05, DEP-06, DEP-10, DEP-14 | repair 改 EventLog、自动重跑 Unknown、跳过 fence/approval、修复越权 cursor 均拒绝 | repair 产出新 projection generation；reconcile 决策有 actor/evidence |
| <a id="step-dep-17"></a>`DEP-17` | 实现 append/artifact/operation/log/diagnostic/migration capacity、backpressure 和 shutdown limits | DEP-10, DEP-13, DEP-14 | 无界 channel、磁盘满继续写、超限静默丢事件、关闭 deadline 被忽略均失败 | capacity threshold 可测试；拒绝带 stable code，不破坏已提交事实 |
| <a id="step-dep-18"></a>`DEP-18` | 建立 incident schema、runbook refs、phase deadline/alert routing 和 `RunbookEvidence` | DEP-14, DEP-15, DEP-16 | 告警直接扩大权限/自动补偿、incident 无 operation/evidence、关闭前未验证均失败 | observed→triaged→contained→recovering→verified→closed 可重放 |

#### 波次 C：备份、恢复与灾难演练

| Step | 代码落点与目标 | 依赖 | 先拒绝的验收 | 成功路径与证据 |
|---|---|---|---|---|
| <a id="step-dep-19"></a>`DEP-19` | 在 `kiana-eventlog`/`kiana-ports` 定义 `BackupManifest`、hash/chunk、cursor/generation/epoch、artifact/config refs | DEP-02, DEP-07, DEP-14 | manifest 缺 source cursor、hash mismatch、绝对路径/secret、torn tail 或 unknown integrity 均拒绝 | full backup manifest 可验证、可重放、可 redacted 导出 |
| <a id="step-dep-20"></a>`DEP-20` | 实现 quiesce snapshot：EventLog JSONL/WAL、ArtifactStore、projection checkpoint、migration registry 的一致快照 | DEP-12, DEP-13, DEP-19 | active writer、未 flush、WAL/主文件不一致、projector cursor 越过 source、并发 backup 均拒绝 | fake store 验证 quiesce→manifest→fsync→verify→resume 顺序 |
| <a id="step-dep-21"></a>`DEP-21` | 实现 incremental backup、retention、legal hold、archive、encryption/key ref 和删除依赖图 | DEP-19, DEP-20 | 只按 mtime 增量、删除仍被依赖的父备份、key ref 缺失、hold 被忽略均失败 | 增量链可还原；保留/归档/删除事件和容量可查询 |
| <a id="step-dep-22"></a>`DEP-22` | 实现 restore quarantine root、manifest/signature/hash/schema/cursor/epoch 校验和 projector/index rebuild | DEP-19, DEP-20, DEP-21 | 覆盖 active root、hash/identity/generation/epoch 回退、缺 artifact、unknown/migration 未处理均拒绝 | 新 root 在 quarantine 完成完整 scan/rebuild/verification |
| <a id="step-dep-23"></a>`DEP-23` | 实现 restore activation、new lease/fence、old root read-only、activation readiness gate | DEP-06, DEP-11, DEP-22 | 旧 writer 未 fence、相同 identity 不同 hash、未有 explicit activate、ready 前接新命令均失败 | activate 后只有新 root ready；旧 root 可审计且不能写入 |
| <a id="step-dep-24"></a>`DEP-24` | 建立 crash/restore/backup fault fixtures，测量 RPO/RTO 并生成演练证据 | DEP-20, DEP-22, DEP-23 | backup partial、restore 中断、损坏 frame、磁盘满、时钟回退、恢复后误 dispatch 均失败 | 定期演练输出 source/command/fixture/exit/RPO/RTO/limitations |
| <a id="step-dep-25"></a>`DEP-25` | 实现 external effect receipt lookup、idempotency key、Unknown reconciliation 与 compensation gate | DEP-13, DEP-16, DEP-23 | result_unknown 自动 retry、没有 external ref 伪造 success、旧 approval/epoch 复用均拒绝 | reconcile、retry_without_effect、abandon、compensate 各有独立 receipt |
| <a id="step-dep-26"></a>`DEP-26` | 把 retention/deletion/revocation 与 PD-25/PD-26、audit/backup/artifact/memory 生命周期接通 | DEP-18, DEP-21, DEP-25 | legal hold 下删除、只删 projection 不留事实、artifact/backup 引用悬空、revocation 不传播均失败 | deletion plan 有依赖、dry-run、commit receipt 和重建验证 |

#### 波次 D：迁移、兼容与回滚

| Step | 代码落点与目标 | 依赖 | 先拒绝的验收 | 成功路径与证据 |
|---|---|---|---|---|
| <a id="step-dep-27"></a>`DEP-27` | 定义 MigrationRegistry、checksum、ordered steps、precondition、owner、backup requirement、compatibility window | DEP-03, DEP-19 | unknown migration、checksum drift、重复版本、无 owner/backup requirement、伪造 down migration 均失败 | registry 可排序、签名/校验并绑定 ReleaseManifest |
| <a id="step-dep-28"></a>`DEP-28` | 实现 read-only migration preflight：store/schema/projection/workflow/provider/config/space/clock/lease 矩阵 | DEP-07, DEP-08, DEP-27 | major mismatch、downgrade、并发 runner、空间不足、active Unknown/old writer、未 verified backup 均阻断 | preflight report 含每一轴结果、remediation 和不变事实证明 |
| <a id="step-dep-29"></a>`DEP-29` | 实现 expand/backfill/verify/switch/contract 的 idempotent bounded migration primitives | DEP-27, DEP-28 | step 触碰 provider/shell/MCP、无 bounded batch、partial 后跳步、旧 reader 读到不可兼容字段均失败 | fixture migration 可重跑、checkpoint 后 resume，source cursor 不回退 |
| <a id="step-dep-30"></a>`DEP-30` | 实现 migration runner lock/fence、`MigrationStarted/Step/Blocked/Completed`、resume token 和 failure quarantine | DEP-06, DEP-27, DEP-28, DEP-29 | 第二 runner、lease 过期、checksum 改变、错误 resume token、失败后继续写均拒绝 | 崩溃后从最后 verified step resume；失败可查询且不伪造 completed |
| <a id="step-dep-31"></a>`DEP-31` | 实现 post-migration projector/index rebuild、source/projection cursor/generation/receipt invariant 验证 | DEP-16, DEP-22, DEP-30 | projection 越过 facts、旧 generation 伪 ready、index 不可重建、receipt 关联丢失均失败 | rebuild 后 query/receipt 与 source replay 一致，ready gate 只在 verify 后开放 |
| <a id="step-dep-32"></a>`DEP-32` | 实现 binary/data rollback decision gate、restore fallback、old root retention 和 rollback receipt | DEP-23, DEP-28, DEP-30, DEP-31 | destructive migration 后无 backup、writer 未停、旧 build 不兼容、未知 effect 未对账均拒绝 | compatible binary rollback 或 verified restore 均有可审计 phase 和验证命令 |
| <a id="step-dep-33"></a>`DEP-33` | 在 workflow/runner/provider/skills/plugins 中实现 build/digest pin、replay compatibility 和 old revision drain | DEP-03, DEP-28, DEP-31 | running workflow 静默换 definition/provider/extension、未知 replay version、project skill 未重新 trust 均拒绝 | 新 run 用新 digest，旧 run 保持 pin；旧 revision drain/retire 有证据 |

#### 波次 E：发布、滚动升级与供应链

| Step | 代码落点与目标 | 依赖 | 先拒绝的验收 | 成功路径与证据 |
|---|---|---|---|---|
| <a id="step-dep-34"></a>`DEP-34` | 扩展 release preflight：reproducible build、Cargo.lock、target matrix、SBOM/checksum/signature、migration/backup gate | DEP-02, DEP-03, DEP-18, DEP-27, DEP-28 | artifact 缺失/未签名、source drift、测试/manifest/migration gate 失败、秘密进入包均 fail-closed | `release-smoke.sh` 与 CI 生成可重现 manifest/evidence |
| <a id="step-dep-35"></a>`DEP-35` | 实现 managed-local/embedded-local 单机 rollout：plan→preflight→backup→drain→replace→ready→promote | DEP-10, DEP-13, DEP-20, DEP-28, DEP-34 | 新进程抢 lease、旧进程未 drain、ready 前接命令、失败自动重试副作用均拒绝 | 同一 root 的升级/重启不丢事实、不重复 effect；旧 revision 可查询 |
| <a id="step-dep-36"></a>`DEP-36` | 实现 container adapter：immutable image、volume/root identity、env allowlist、SIGTERM、startup/readiness/liveness probe | DEP-09, DEP-11, DEP-17, DEP-35 | image digest 漂移、volume 未验证、多个 writer、探针语义混淆、强杀无 evidence 均失败 | fake/container harness 验证启动、摘流、drain、restart、fence |
| <a id="step-dep-37"></a>`DEP-37` | 设计 orchestrated canary/blue-green/rainbow rollout 与 worker build routing；标记真实编排器为 target | DEP-33, DEP-35, DEP-36 | 未 pinned workflow、无 progress deadline、旧 worker 仍写、canary 失败却 promote 均拒绝 | adapter contract 能模拟 pause/resume/rollback；未接真实集群也不升 proof level |
| <a id="step-dep-38"></a>`DEP-38` | 实现 rollout pause/resume/promote/rollback、old revision retirement、retention 和 post-deploy verification | DEP-32, DEP-35, DEP-37 | 未授权 promote、指标/health 未达门槛、rollback 后旧 writer 存活、过早删除旧 root 均失败 | rollout state、decision、health window、retire evidence 完整可查 |
| <a id="step-dep-39"></a>`DEP-39` | 接入供应链、签名、SBOM、依赖许可、desktop package/checksum、secret scanning/compliance gate | DEP-02, DEP-34, DEP-38 | signature/SBOM/license/secret scan failure 被忽略、unsigned desktop binary 发布均阻断 | CI artifact、package、checksum、reviewer 和 policy decision 可关联 |
| <a id="step-dep-40"></a>`DEP-40` | 编写 release/upgrade/rollback/backup/restore/migration/health 的跨 CLI/Web/Workbench/Desktop E2E/UAT | DEP-15, DEP-24, DEP-31, DEP-36, DEP-38, DEP-39 | 任一入口分叉 DaemonHost、直接 broker、权限并集、Unknown 自动 retry、证据缺项均失败 | fake provider/effect + local durable fixture 覆盖 deny/success/restart/replay；真实 provider 仅显式 opt-in |
| <a id="step-dep-41"></a>`DEP-41` | 维护 runbook、operator reference、CURRENT_STATUS 证据块、release gate 和 capability/proof matrix | DEP-00..DEP-40 | 文档把 target 写 implemented/durable/live、遗漏限制、没有 reviewer/命令/fixture 均不能关闭 | 每个切片有 source snapshot、worktree、argv/env、fixture、exit、status/proof、limitations、reviewer |

### 36.9 依赖波次与现有路线图接点

```text
A contracts: DEP-00 → DEP-01 → DEP-02 ∥ DEP-03 → DEP-04 → DEP-05 → DEP-06 → DEP-07
      ↓
B lifecycle: DEP-08 → DEP-09 → DEP-10 → DEP-11 → DEP-12 → DEP-13
             ∥ DEP-14 → DEP-15 → DEP-16 → DEP-17 → DEP-18
      ↓
C backup/DR: DEP-19 → DEP-20 → DEP-21 → DEP-22 → DEP-23 → DEP-24
                                  ∥ DEP-25 → DEP-26
      ↓
D migration: DEP-27 → DEP-28 → DEP-29 → DEP-30 → DEP-31 → DEP-32 ∥ DEP-33
      ↓
E release:   DEP-34 → DEP-35 → DEP-36 → DEP-37 → DEP-38 → DEP-39 → DEP-40 → DEP-41
```

与已有 roadmap 的关系：

| 既有单元 | 在本专项中的接点 |
|---|---|
| `PD-00..PD-35` | StorageRoot、EventStore、cursor/generation、backup/restore、migration、retention、capacity、health、fault injection 和 UAT 的数据层基础；`DEP-19..32` 不能另造事实源 |
| `OA-01..OA-28` | operation/revision/trace/evidence/metrics/audit 字段；`DEP-14..18` 消费并补充 deployment 生命周期，不把 EventLog 直接当指标系统 |
| `AUT-05/AUT-08/AUT-09/AUT-15/AUT-17/AUT-21` | queue/lease/fence、DaemonHost service、stop/drain、Unknown、boot recovery；`DEP-06/10/13/16/25/33` 必须保持同一 epoch 语义 |
| `CI-01..CI-12` | config revision、secret ref、identity、rotation、environment allowlist；`DEP-01/08/34/39` 只引用其 contract，不把 secret 写入 deployment facts |
| `P0-A-01b`、`P0-G-02a/b`、`P0-J1-*` | versioned protocol、durable event、approval、cancel、recovery 的公共合同；部署 command 不能绕过它们 |
| `P1-J8-01`、`P1-K5-01` | usage/cost、trace、budget 与容量/发布门；成本指标不能替代 health 或 quality gate |
| `P2-K6/K7`、`P2-M*` | deletion/retention/CLI/Web projection；运维查询只能读 projection，删除需保留 audit/hold 语义 |
| `P4-J3-05/J6/J7`、`P4-L3/L5/L6` | swarm、stream、version drift、extension supply chain；`DEP-33/37/39` 约束旧 revision 和扩展 digest |
| `scripts/release-smoke.sh`、`.github/workflows/release.yml` | `DEP-34/39/40` 的执行入口；现有通过仅是基线，不能直接提高 proof level |

同一 integration owner 串行修改 `Cargo.toml`、`Cargo.lock`、protocol registry、CI workflow、migration registry 和公共 schema；独立 fixture、文档、诊断 projection 可以并行。任一 gate 拒绝时，取消依赖该 gate 的 promote/activate/retire，保留只读诊断、备份验证和证据归档。

### 36.10 最低验收矩阵与状态口径

| 证据面 | 必须先证明的拒绝路径 | 成功/恢复路径 | 证明上限 |
|---|---|---|---|
| Release/provenance | source/artifact/signature/lock/toolchain/target mismatch、secret leak、unknown major | manifest 可验证、artifact 可重现、CI/package evidence 可关联 | `local_behavior` |
| Root/trust/lease | untrusted root、path escape、symlink/hardlink、remote FS unsupported、stale owner/fence/epoch、双 writer | acquire/heartbeat/fence/release、重启后唯一 active writer | `local_behavior`；有跨重启 durable 证据才可升级 |
| Startup/health | migration 未完成、projector lag、Unknown、容量不足、provider 自报 ready、drain 中接新命令 | startup→ready、live/readiness 摘流、maintenance、timeout recovery | `local_behavior` |
| Shutdown | flush 未确认、started work 未排空、强杀、late result、重复 terminal | graceful drain、stop evidence、重启恢复/不重复 effect | `local_behavior`/`durable` 取决于 EventStore 证据 |
| Backup/restore | torn tail、cursor gap、hash/signature mismatch、WAL 不一致、覆盖 active root、epoch/generation 回退 | full/incremental backup、quarantine restore、rebuild、explicit activate、RPO/RTO 演练 | `durable` 只有跨重启/介质证据支持时成立 |
| Migration | checksum drift、unknown/downgrade、无 backup、并发 runner、partial step、space/clock failure | preflight、ordered idempotent resume、expand/contract、post-verify、rollback gate | `local_behavior`/`durable` 取决于 manifest 和恢复证据 |
| Effect reconciliation | `result_unknown` 自动 retry、无 external ref 伪 success、旧 approval/epoch 复用 | reconcile/retry_without_effect/abandon/compensate 各有 receipt | 不能据此声称外部业务 outcome |
| Rollout | old writer 存活、workflow/provider drift、canary 未达标、无 progress deadline、未授权 promote | local/container rollout、pause/resume、blue-green target adapter、rollback/retire | `local_behavior`；真实 orchestrated/live 需环境证据 |
| Operations/security | doctor 泄密、audit append 丢失、告警扩大权限、legal hold 绕过、capacity 静默丢事实 | status/doctor/runbook、redacted evidence、incident lifecycle、retention/deletion | `local_behavior` |

每个 `DEP-*` 完成时必须在 `CURRENT_STATUS.md` 产生以下证据块，且把 `feature_status` 与 `proof_level` 分开：

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

“有 `ReleaseManifest`”“有健康端点”“迁移测试通过”“备份文件存在”“CI green”都不能单独证明 deployed、durable、live 或 physical。当前 release smoke、单 crate 测试和本地 JSONL 只能作为基线；只有完成相应跨重启、恢复介质、真实 supervisor/编排器或显式 provider cassette 的证据，才能提高证明等级。任何未完成的云、多副本、自动备份、自动回滚、外部 effect exactly-once 和合规认证都必须保留 `target`、`partial` 或 `deferred` 口径。
<a id="security-compliance-plan"></a>

## 37. 安全与合规专项：实际代码设计、处理流程与详细实施步骤（2026-09-14 追加）

> 本节补全 [module-map.md](module-map.md) 第 20 模块。完整的威胁模型、reference 调研、领域对象、端口、数据生命周期、端到端流程、SEC 映射和 SC-00–SC-43 详细卡片见 [安全与合规专项](roadmap/security-compliance.md)。本节只建立总路线图中的架构结论、依赖和发布门，不改变 CURRENT_STATUS.md 的现状。

### 37.1 设计结论

安全与合规是横切的控制链，不是新的执行器或第二个模型循环。唯一有效的路径仍是：

~~~text
entrypoint -> versioned protocol -> DaemonHost
  -> ControlPlane admission/policy/gate/approval
  -> Capability Broker + OS sandbox + SecretStore
  -> handler/provider/connector effect
  -> EventLog fact -> Receipt/Audit/Query projection
~~~

实现时固定以下规则：

1. Principal、ProjectTrust、RoleAssignment、PolicyRevision、AuthorityEpoch 和 DataEpoch 由服务端解析和绑定；入口自报的 actor、project、role、trust、budget 和 endpoint 不能授予权限。
2. Grant 只做父级、模板、部门、项目、WorkPacket、Cell 和 approval 的交集；读、写、删除、网络、进程、secret、外部账户和高风险 effect 是独立 scope。
3. 先做 deny-first admission，再以 CAS 追加 AdmissionCommitted、预留预算并签发短 permit；Broker 在 effect 前重验 payload/target digest、path、endpoint、lease、fence、epoch 和 idempotency。
4. 模型、项目资源、skill/plugin/hook、MCP 描述、provider response、Webhook、环境变量、旧事件和恢复请求都是不可信输入；它们可以触发 approval 或 quarantine，不能直接扩大能力。
5. Secret 只以 opaque SecretRef 进入协议；SecretStore 的短期、单次 lease 只在 Broker 受控内存中解析，EventLog、Receipt、log、metric、trace、backup 和错误只保留 ref/digest。
6. EventLog 是产品事实源；Audit、Receipt、Trace、Metric、Transcript、Memory、Index、UI 和通知是不同投影，不能互相取代。result_unknown、取消、重放和删除传播都保留原事实。
7. 每个决定、effect、恢复、删除、供应链验证和事故都关联 operation/run/attempt、revision、source cursor、epoch、digest 和 evidence ref；没有这些字段不能提高 proof level。

### 37.2 威胁与控制面

| 威胁 | 首先拒绝或隔离的条件 | 主要 SC |
|---|---|---|
| 提示/间接注入、过度代理 | 低信任内容试图改变 policy、role、scope 或调用隐藏工具 | SC-21、SC-24、SC-26、SC-39 |
| confused deputy、权限升级 | audience、主体、account、purpose、父 Grant 或 approval 不匹配 | SC-04、SC-07、SC-09、SC-10、SC-17 |
| Secret 泄露 | 未分类、无目的、无法全链路 redaction 或超出 Broker | SC-18–SC-20、SC-39 |
| TOCTOU、重放、重复 effect | generation/fence/digest/sequence/epoch/idempotency 不匹配 | SC-12–SC-15 |
| SSRF、网络越界 | endpoint 不在 allowlist、解析后地址改变、token audience 错误 | SC-14、SC-17 |
| 资源耗尽 | quota、并发、bytes、wall time、retention 空间无余量 | SC-16、SC-22、SC-40 |
| 事实篡改、删除违规 | cursor/hash/source、purpose、hold、tombstone 不一致 | SC-23、SC-31、SC-32 |
| 供应链投毒 | lockfile、digest、license、SBOM、provenance、signature 缺失 | SC-25–SC-30 |

### 37.3 代码设计和数据生命周期

| 层 | 目标代码合同 | 边界 |
|---|---|---|
| kiana-domain | Principal、SecurityContext、Grant、Approval、SecretRef、DataClass、Purpose、Incident、EvidenceManifest、稳定 reason code | 只承载值和不变量，不读文件、网络、进程或 secret |
| kiana-protocol | versioned security context、command intent、permit、approval、audit、unknown、delete/export、evidence DTO | 不接受 caller 自报身份/epoch，不直接执行 |
| kiana-ports | IdentityResolver、PolicyEvaluator、SecretStore、DataGovernancePort、SupplyChainVerifier、SandboxPort、AuditSink、ClockPort | 窄接口、结构化错误、可替换 fixture |
| kiana-core | admission、grant intersection、approval digest、budget/path/endpoint gate、epoch/fence、cancel/unknown、incident/reconcile | 唯一授权和生命周期决策点 |
| kiana-capability-broker | permit 消费、effect-time recheck、sandbox、egress、secret lease、进程树和输出边界 | 不能接收模型/插件自行扩大的 profile |
| kiana-eventlog | append-only decision/effect/observation、CAS、redaction、source cursor、tombstone、retention | projection/cache/transcript 不能反写事实 |
| kiana-daemon | DaemonHost 组装、trust/config/secret root、connector/MCP ingress、bounded channels、health | 不另起 runner、scheduler、connector 执行循环 |
| kiana-query 和 UI | redacted audit/receipt/retention/incident projection、purpose/scope 查询 | 只读或提交 versioned command，不能本地 allow/删除 |

数据按 Public、Internal、Confidential、Restricted、Secret、Regulated 分类；未知分类取更严格级别。每次读取、provider 请求、telemetry、索引、memory 和 export 都带 Purpose，目的变化必须重新授权。Secret 不写 EventLog；不可改写的事实通过追加 DeletionTombstone 和 data epoch 表示，projection/index/cache/export/backup adapter 返回独立 receipt 或 unknown。删除证明只覆盖 Kiana 管辖的数据根，不代表外部系统已物理删除。

### 37.4 统一处理流

~~~text
startup/config:
  ReleaseManifest + lockfile/SBOM/signature/provenance
  -> ProjectTrust before project resources
  -> immutable ConfigSnapshot + SecretRef
  -> acquire lease/epoch and scan store

command/effect:
  versioned envelope
  -> server Principal/ProjectTrust/Role
  -> classify DataClass/Purpose/Risk
  -> deny-first policy/gate/approval/budget/path/endpoint
  -> CAS AdmissionCommitted + reservation + short permit
  -> Broker recheck + sandbox/SecretStore
  -> Started/Observed/Succeeded/Failed/Unknown
  -> flush EventLog -> Receipt/Audit/redacted projections

cancel/recovery:
  cancel/timeout/crash
  -> reject new intake and fence permit
  -> observe started work
  -> Stopped or result_unknown
  -> freeze related budget/data/export
  -> reconcile external receipt/idempotency
  -> explicit retry_without_effect/compensate/abandon/close
~~~

MCP HTTP 必须验证 resource/audience、PKCE、state、redirect 和 scope，并禁止 token passthrough；stdio MCP 仍须记录 binary/path、manifest digest、环境 allowlist 和 capability catalog。Webhook 先验证签名、nonce、时间窗、account、schema 和 idempotency，再转成 typed intent 回到 ControlPlane。

### 37.5 详细实施波次（SC-00–SC-43）

| 波次 | 卡片 | 交付重点 |
|---|---|---|
| A 合同与基线 | SC-00–SC-05 | 资产/威胁登记、版本化安全对象、稳定错误、server-owned context、deny-first policy |
| B 身份与审批 | SC-06–SC-11 | Principal/session、ProjectTrust/Role、epoch/fence、Grant 交集、精确 approval、四入口一致 |
| C Effect 与隔离 | SC-12–SC-17 | PendingInvocation/permit、CAS/idempotency、TOCTOU、网络/沙箱、cancel/Unknown、MCP/Webhook |
| D Secret 与数据治理 | SC-18–SC-24 | SecretRef/lease/redaction、DataClass/Purpose、retention/legal hold、tombstone/delete、memory/index/export |
| E 扩展与供应链 | SC-25–SC-30 | ProjectTrust、extension manifest、sandbox lifecycle、SBOM/license/advisory、release provenance/signature、route attestation |
| F 审计与事故 | SC-31–SC-36 | Audit schema/projector、Incident/Reconcile、SEC crosswalk、EvidenceManifest、CLI/TTY/Web/Desktop parity |
| G 验证与发布门 | SC-37–SC-43 | negative/property/fuzz/red-team/capacity、CI gate、恢复/保留演练、CURRENT_STATUS 回填 |

每张卡都先覆盖 deny、越权、过期 approval、取消、unknown、重放、TOCTOU、注入和恢复，再覆盖成功路径。完整代码目标、依赖、拒绝断言、成功证据和测试边界见 [SC 详细卡片](roadmap/security-compliance.md#security-compliance-steps)。

### 37.6 与现有专项的接点

| 既有专项 | 安全承接 | 不重复建设 |
|---|---|---|
| CP-*、P0-A/B/F/G/J1 | identity、admission、approval、budget、cancel、fence、Unknown | 不在 provider/UI/workflow/connector 建第二授权 |
| CAP-*、P4-J7-* | permit、sandbox、MCP/provider endpoint、effect/usage 证明 | provider response 和 capability catalog 不能成为安全事实 |
| ER-*、PD-* | append/CAS、Receipt、replay、backup、migration、retention、delete | 不建立第二 EventLog，不用 projection 覆盖事实 |
| CM-*、EXT-* | memory purpose/candidate、ProjectTrust、skill/plugin/hook 生命周期 | allowed-tools、memory origin、manifest 不能扩大 Grant |
| INT-*、NM-*、UI-* | connector/webhook、通知、入口一致、redacted projection | 实时流、通知、UI 不构成授权或送达事实 |
| OA-*、EQ-*、DEP-*、BQ-* | telemetry privacy、security eval、发布/恢复、quota/resource gate | metric、评测、脚本、成本账本不能替代 EventLog/ControlPlane |

### 37.7 SEC 映射、验收门和证据

| 条款 | 主要 SC | 最低结果 |
|---|---|---|
| SEC-01/02 | SC-04、SC-06–SC-11、SC-26 | server-owned identity、ProjectTrust、Grant 只交集、approval 精确绑定 |
| SEC-03/12 | SC-09、SC-16、SC-40 | Cell/run/project/provider 的预算、并发、输出、wall time、bytes 有上限 |
| SEC-04/07/08/09 | SC-12–SC-17、SC-37、SC-42 | exact permit、TOCTOU/cancel fencing、Unknown 不成功/不盲重试 |
| SEC-05/06 | SC-04、SC-18–SC-20、SC-36、SC-39 | loopback 不是认证，secret 全出口 redaction，四入口同一决定 |
| SEC-10/11 | SC-17、SC-21、SC-24–SC-36 | 不可信输入分层、EventLog append-only、审计可重放/可重建 |

| Gate | 条件 | 允许证明 |
|---|---|---|
| S0 contract | SC-00–05 的 schema、reason、fixture、威胁登记 | source |
| S1 deny path | SC-06–24、SC-37–39 的越权、泄露、重放、TOCTOU、删除、入口绕过均 zero effect | 局部 local_behavior |
| S2 durable | EventLog/CAS/replay/recovery/delete/incident manifest 可跨进程重建 | 覆盖对象 durable；外部 effect 仍可能 unknown |
| S3 opt-in live | 真实身份、密钥、网络、provider/connector、人工 approval 和外部回执均有逐连接证据 | 只提升对应连接到 live |
| S4 physical | 独立 safety controller、现场确认和物理回执 | 当前保持 not_supported |

每张 SC 卡完成时在 CURRENT_STATUS.md 写入：

~~~text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
~~~

本节只新增设计，不改变现状。authenticated principal、完整 Secret redaction/TOCTOU、durable audit projector、跨进程恢复、供应链验证、删除传播和真实连接器回执仍需逐卡验收。外部调研依据包括 [MCP Authorization](https://modelcontextprotocol.io/specification/2025-06-18/basic/authorization)、[NIST AI RMF](https://www.nist.gov/itl/ai-risk-management-framework)、[OWASP GenAI Top 10](https://genai.owasp.org/llm-top-10/)、[W3C Trace Context](https://www.w3.org/TR/trace-context/)、[OpenTelemetry sensitive data guidance](https://opentelemetry.io/docs/security/handling-sensitive-data/)、[SLSA](https://slsa.dev/spec/v1.2/) 和 [Sigstore security model](https://docs.sigstore.dev/about/security/)。
