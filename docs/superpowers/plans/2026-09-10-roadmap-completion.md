# roadmap.md 补全实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `docs/roadmap.md` 从"审计 backlog 的附属清单"补全为覆盖 canonical 阶段 P0–P6 的执行骨架：P0–P4 每个实施单元一张六行卡，P5/P6 只登记边界。

**Architecture:** 单文件 Markdown。总图按 canonical 阶段 P0–P6 组织，单元编号统一为 `P<阶段>-<切片>-<序号>`。roadmap 只引用 canonical 文档、不定义新规范；每个单元的证据仍以 `CURRENT_STATUS.md` 为准。

**Tech Stack:** Markdown；验证靠 `grep` / `awk` 计数与断言，无需编译。

**Spec:** `docs/superpowers/specs/2026-09-10-roadmap-completion-design.md`

## Global Constraints

- **禁止自动提交**：所有 `git commit` 步骤必须先取得用户显式授权（`AGENTS.md`、`CLAUDE.md` 硬规则）。未授权时执行到提交步骤即停，标记为"待授权"。
- roadmap **不定义新规范**：每张卡必带「依据」，指向 canonical 文档章节。
- 每张卡**最多 8 行**（含标题行）。
- 「验收」必须是可写成测试名的断言。
- 完成态中**不得出现旧编号** `0.x`–`8.x`。
- **只保留一套 P 编号**：`company-os-spec-index.md` §7 的 P0–P6。
- 不修改任何 `docs/company-os-*.md`、`CURRENT_STATUS.md` 的既有证据块。

## 覆盖清单（38 个实施单元 → 阶段）

| 阶段 | 覆盖的切片/子切片 |
|---|---|
| P0 | A、B、F、G、J1、J7、K1、M1 |
| P1 | C、D、E、H、J2、J3、J4、J8、K5、L1、L4 |
| P2 | J5、K3、K4、K6、K7、L2、M2、M3、M4、M5、M7 |
| P3 | I |
| P4 | J6、J7、K2、K8、L3、L5、L6、M6 |

合计覆盖全部 38 个切片/子切片（上表因 J7 跨阶段而出现 39 次）。**J7 跨阶段**：`P0-J7-01` 是已落地的流式基线，`P4-J7-02`/`-03` 是尚未开始的协议单元——切片跨阶段合法，编号不重复。

---

## Task 1: §0 规则与章节骨架

**Files:**
- Modify: `docs/roadmap.md:1-33`（文件头与 §0）

**Interfaces:**
- Produces: §0 的编号规则与状态口径；后续所有任务依赖它决定编号与状态写法。

- [ ] **Step 1: 改写文件头**

把前 6 行替换为：

```markdown
# Kiana 执行路线图（P0–P6 执行骨架 + 进度）

> **一屏看进度** → §1 总图。**看某步具体做什么** → §4 起的详细卡。**你想加东西** → §11 追加区。
> 当前事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文只排顺序、记进度、写验收口径，**不定义新规范**。
> 阶段编号以 [`company-os-spec-index.md`](company-os-spec-index.md) §7 的 P0–P6 为唯一 canonical。
> 单元清单来源：`company-os-implementation-outline.md` §3 的切片 A–M 与子切片 J1–M7（共 38 个）。
```

- [ ] **Step 2: 在 §0 追加编号规则小节**

在「状态图例」表格之后插入：

```markdown
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
```

- [ ] **Step 3: 验证编号规则存在**

Run: `grep -c 'P<阶段>-<切片>-<序号>' docs/roadmap.md`
Expected: `1`

- [ ] **Step 4: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): establish P0-P6 numbering rules and section skeleton"
```

---

## Task 2: P0 总图行与详细卡

**Files:**
- Modify: `docs/roadmap.md`（§1 总图、新增 §4）

**Interfaces:**
- Consumes: Task 1 的编号规则。
- Produces: `P0-*` 共 18 个单元的编号与验收测试名；后续任务沿用同一表格格式。

- [ ] **Step 1: 在 §1 写入 P0 的 18 行**

表格列固定为 `编号 | 阶段 | 切片 | 依赖 | 退出条件 | 状态`：

```markdown
| 编号 | 阶段 | 切片 | 依赖 | 退出条件 | 状态 |
|---|---|---|---|---|---|
| `P0-A-01` | P0 | A 契约注册表 | — | 每个公开类型有唯一 owner、版本与转换测试 | ⏳ |
| `P0-A-02` | P0 | A 契约注册表 | `P0-A-01` | `CapabilityErrorCode` + `failure_code()`，每码有 CLI exit / HTTP status / 可重试映射 | ⏳ |
| `P0-B-01` | P0 | B 正式状态机 | `P0-A-01` | Cell/WorkPacket/CapabilityExecution/Approval 四张转移表；非法转移与重复请求有断言 | ⏳ |
| `P0-F-01` | P0 | F Approval | `P0-B-01` | TTY/Web/一次性 CLI 三处可列举同一 pending 并回复 | ⏳ |
| `P0-F-02` | P0 | F Approval | `P0-F-01` | 每次批/拒都有 durable 记录；重复消费与过期被拒 | ⏳ |
| `P0-F-03` | P0 | F Approval | `P0-G-02` | 重启后可续跑同一 Runner；缺材料返回 `approval_continuation_unavailable` | ⏳ |
| `P0-G-01` | P0 | G 事实源与恢复 | — | 内存未命中时只读回读重建；账本无记录仍 fail-closed | 🔄 |
| `P0-G-02` | P0 | G 事实源与恢复 | `P0-G-01` | 新增 `run.prompt`/`run.tool_call`/`run.tool_result`，全过 `redact_event_value` | ⏳ |
| `P0-G-03` | P0 | G 事实源与恢复 | `P0-G-02` | additive `ResumeRequest`，`PROTOCOL_SCHEMA` 不动，复用同一 `drive_run` | ⏳ |
| `P0-G-04` | P0 | G 事实源与恢复 | `P0-G-01` | 新进程仅凭事件重建 Run/Invocation；矛盾终态 fail-closed | ⏳ |
| `P0-J1-01` | P0 | J1 Runtime | `P0-B-01` | `RunCancellationState` + 转移表；`ExecutionStatus` 补 `Queued`/`Cancelling`；每 run 恰好一条终态 | ⏳ |
| `P0-J1-02` | P0 | J1 Runtime | `P0-J1-01` | queued tool calls 排空并合成 replay-safe 结果 | ⏳ |
| `P0-J1-03` | P0 | J1 Runtime | `P0-J1-01` | 取消路径确认进程组停止；无法确认进 `result_unknown` | ⏳ |
| `P0-J1-04` | P0 | J1 Runtime | `P0-J1-01`–`03` | 保留 `cancelling_mid_stream_never_completes_or_emits_a_late_delta` 语义 | ⏳ |
| `P0-J1-05` | P0 | J1 Runtime | — | 重复工具调用 / `max_steps` 按角色 / wall-time 预算均 fail-closed，阈值进 `RuntimeConfig` | 🔄 |
| `P0-J7-01` | P0 | J7 Provider/Output | — | 账本粒度、不完整流 fail-closed、默认开启均已落地并有证据块 | ✅ |
| `P0-K1-01` | P0 | K1 Identity | `P0-A-01` | 由受保护入口解析身份；服务端从不可变 assignment 派生 role/department | ⏳ |
| `P0-M1-01` | P0 | M1 Workbench | — | CLI/TTY/Web/Desktop 对同一 run 的 terminal state 一致 | ⏳ |
```

- [ ] **Step 2: 在 §4 写入 P0 的 18 张卡**

每张卡严格用六行模板。**字段映射规则**（所有卡都按这张表提取，不得自由发挥）：

| 卡片字段 | 来源 | 写法 |
|---|---|---|
| 标题行 | 总图「编号」+ 单元标题 | `### <编号> <标题>　<状态>` |
| 现状 | 该 Slice 的「实现位置」或当前代码位置 | 必须带 `file:line` 或 canonical 文档章节 |
| 做什么 | 总图「退出条件」展开为一句动作 | 一句话，动词开头 |
| 风险 | 该 Slice 的「当前状态」与账本 limitations | 一句话；无风险则写"无已知风险" |
| 验收 | 总图「验收测试名」 | 反引号包裹的测试名 |
| 依赖 / 边界 | 总图「依赖」+ 该 Slice 的「不做」条款 | 一句 |
| 依据 | `company-os-implementation-outline.md` 的对应 `§Slice X` | 章节名 |

示例：

```markdown
### P0-G-02 账本记录可重放 history　⏳

- **现状**：账本不记 user prompt，也不记 provider 的 `tool_call_id`（`kiana-core/src/lifecycle.rs:40-54`）。
- **做什么**：新增 `run.prompt` / `run.tool_call` / `run.tool_result` 事件 kind，全部过 `redact_event_value`。
- **风险**：新事件增加 append 次数，必须沿用轮次聚合的合并语义。
- **验收**：`resume_rebuilds_model_visible_history_from_ledger`
- **依赖 / 边界**：依赖 `P0-G-01`；不新增模型可见工具，不动 `PROTOCOL_SCHEMA`。
- **依据**：`company-os-implementation-outline.md` §Slice G
```

- [ ] **Step 3: 验证 P0 行数与卡片数**

Run: `grep -cE '^\| \`P0-' docs/roadmap.md && grep -cE '^### P0-' docs/roadmap.md`
Expected: `18` 与 `18`

- [ ] **Step 4: 验证卡片行数上限**

Run:
```bash
awk '/^### P[0-6]-/{if(h!=""&&n>8)print "TOO LONG ("n"): "h; h=$0; n=0; next}
     /^## /{if(h!=""&&n>8)print "TOO LONG ("n"): "h; h=""; n=0; next}
     h!=""&&NF{n++}
     END{if(h!=""&&n>8)print "TOO LONG ("n"): "h}' docs/roadmap.md
```
Expected: 无输出

- [ ] **Step 5: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): add P0 master-table rows and detail cards"
```

---

## Task 3: P1 总图行与详细卡

**Files:**
- Modify: `docs/roadmap.md`（§1 总图、新增 §5）

**Interfaces:**
- Consumes: Task 2 的表格与卡片格式。
- Produces: `P1-*` 共 18 个单元。

- [ ] **Step 1: 写入 P1 的 18 行**

| 编号 | 切片 | 退出条件 |
|---|---|---|
| `P1-C-01` | C 组织与 Cell | `AgentTemplate`/`CellSpec`/`SpawnPlan`/`BudgetLease`/`CapabilityGrant`/`SupervisionLease` 定义；子权限只减不增 |
| `P1-C-02` | C 组织与 Cell | reserve→commit→terminal→retire 全链；retire 撤销 grant、释放锁与预算 |
| `P1-D-01` | D WorkPacket | `ready_packets(graph, now)` 单实现；ControlPlane / `kiana project next` / 看板三处结果一致 |
| `P1-D-02` | D WorkPacket | `validate_dependency_dag` 输出确定性规范化环；缺依赖不推进状态 |
| `P1-D-03` | D WorkPacket | 过期 lease 退回 ready 并记事件；worker 死亡后可回收且不重复派发 |
| `P1-E-01` | E 通信与问责 | Chat/Command/Handoff/Decision/StatusReport/Evidence/Incident 分离；Handoff 必须 ACK |
| `P1-H-01` | H Capability/Broker | 工具权威单一真源；不新增模型可见工具，保持 5 个 |
| `P1-H-02` | H Capability/Broker | 映射期拒绝非法参数；`additionalProperties` 不默认禁止 |
| `P1-H-03` | H Capability/Broker | 所有副作用工具共用同一 containment |
| `P1-J2-01` | J2 Context/Cache | `PromptSection{name, order, text}` + `render_prompt()` + provenance |
| `P1-J2-02` | J2 Context/Cache | `TokenBudget` 计入 tool schemas 与 system prompt；越界 fail-closed |
| `P1-J2-03` | J2 Context/Cache | `RoleSpec.prompt` 进入 provider 的 system message |
| `P1-J3-01` | J3 Memory | 模型写入一律 candidate+draft；`origin` 服务端派生；默认检索排除 |
| `P1-J4-01` | J4 Capability/MCP | MCP server/tool schema、health、trust、version、result validation 可追踪 |
| `P1-J8-01` | J8 Observability | provider/model、policy verdict、tool args hash、usage、retry/cancel reason 可追溯且不泄密 |
| `P1-K5-01` | K5 Cost/capacity | `UsageRecord`/`CostLedger`/`Quota`；`RuntimeBudget` 与 `ProjectBudget` 不混用 |
| `P1-L1-01` | L1 Eval | GoldenTrace 绑定源码快照/输入 hash/版本/Receipt；replay 无真实副作用 |
| `P1-L4-01` | L4 Code intelligence | 结果带 snapshot、来源与 freshness |

**验收测试名**（逐行填入表格最后一列，与单元一一对应）：

```text
P1-C-01  child_grant_cannot_exceed_parent_grant
P1-C-02  retire_revokes_grants_and_releases_budget
P1-D-01  single_ready_predicate_agrees_across_three_callers
P1-D-02  dependency_cycle_is_rejected_deterministically
P1-D-03  expired_lease_is_reclaimed_without_double_dispatch
P1-E-01  free_chat_never_grants_authority
P1-H-01  tool_authority_covers_every_model_visible_tool
P1-H-02  malformed_arguments_are_rejected_before_capability_mapping
P1-H-03  path_containment_is_shared_by_every_side_effecting_tool
P1-J2-01 context_sections_render_with_provenance
P1-J2-02 tool_schemas_count_toward_the_budget
P1-J2-03 role_prompt_reaches_the_provider_system_message
P1-J3-01 model_written_memory_stays_unsearchable_until_approved
P1-J4-01 mcp_tool_schema_and_health_are_traceable
P1-J8-01 observability_record_is_redacted_and_traceable
P1-K5-01 runtime_and_project_budgets_are_not_interchangeable
P1-L1-01 golden_trace_replay_has_no_external_side_effects
P1-L4-01 code_intelligence_results_carry_freshness
```

- [ ] **Step 2: 在 §5 写入 P1 的 18 张卡**

沿用 Task 2 Step 2 的六行模板，「依据」指向 `company-os-implementation-outline.md` 的 `§Slice C`/`§Slice D`/… 对应小节。

- [ ] **Step 3: 验证行数与卡片数**

Run: `grep -cE '^\| \`P1-' docs/roadmap.md && grep -cE '^### P1-' docs/roadmap.md`
Expected: `18` 与 `18`

- [ ] **Step 4: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): add P1 master-table rows and detail cards"
```

---

## Task 4: P2 总图行与详细卡

**Files:**
- Modify: `docs/roadmap.md`（§1 总图、新增 §6）

**Interfaces:**
- Consumes: Task 2 的格式。
- Produces: `P2-*` 共 12 个单元。

- [ ] **Step 1: 写入 P2 的 12 行**

| 编号 | 切片 | 退出条件 | 验收测试名 |
|---|---|---|---|
| `P2-J5-01` | J5 Workflow | 版本固定；重试/取消/审批/补偿可重放 | `workflow_definition_replays_after_restart` |
| `P2-K3-01` | K3 Human control | Approval/Review/Acceptance/Incident 进入同一 Inbox | `human_inbox_collects_all_decision_types` |
| `P2-K4-01` | K4 Artifact | CheckpointService 绑定 transcript offset + workspace revision + invocation；恢复后旧 approval 作废 | `restore_invalidates_stale_approval` |
| `P2-K6-01` | K6 Reliability | crash/timeout/cancel/disk full/MCP failure/Provider Unknown 各有 Incident/Recovery | `every_failure_class_has_an_incident_and_recovery` |
| `P2-K7-01` | K7 Data governance | 删除/过期/撤销传播到 Memory、Artifact、Index、Compaction、cache policy | `deletion_propagates_to_memory_and_index` |
| `P2-L2-01` | L2 Feedback | Feedback 只产生候选，不能直接改 Role/Grant/Policy/历史事实 | `feedback_cannot_mutate_policy_or_history` |
| `P2-M2-01` | M2 UI projection | `UiSnapshot`/`UiAction`/cursor/epoch；乐观更新不覆盖更新事件 | `stale_ui_action_is_rejected_by_epoch` |
| `P2-M3-01` | M3 Human actions | Approval/Review/Acceptance/Incident 动作卡三处复用 | `action_card_is_shared_by_all_surfaces` |
| `P2-M4-01` | M4 Run/Artifact detail | Run timeline/Invocation/Diff/Evidence/Receipt 可相互定位 | `receipt_artifact_and_evidence_cross_locate` |
| `P2-M5-01` | M5 Web sync | snapshot hydration + 事件订阅 + 重连不重放 delta | `web_sse_reconnect_emits_stream_gap_without_replaying_delta_items` |
| `P2-M5-02` | M5 Web sync | 只读列出持久会话 | `web_lists_persisted_sessions_after_restart` |
| `P2-M7-01` | M7 accessible fallback | 键盘、窄屏、文本状态、aria/高对比 | `status_is_reachable_without_color` |

- [ ] **Step 2: 在 §6 写入 P2 的 12 张卡**

- [ ] **Step 3: 验证行数与卡片数**

Run: `grep -cE '^\| \`P2-' docs/roadmap.md && grep -cE '^### P2-' docs/roadmap.md`
Expected: `12` 与 `12`

- [ ] **Step 4: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): add P2 master-table rows and detail cards"
```

---

## Task 5: P3 总图行与详细卡

**Files:**
- Modify: `docs/roadmap.md`（§1 总图、新增 §7）

- [ ] **Step 1: 写入 P3 的 6 行**

| 编号 | 切片 | 退出条件 | 验收测试名 |
|---|---|---|---|
| `P3-I-01` | I Company 生命周期 | Objective/Initiative/Project/Milestone/Acceptance/Delivery/Outcome/ChangeRequest/Risk/Incident 定义与不变量 | `company_objects_expose_invariants` |
| `P3-I-02` | I Company 生命周期 | 九个命令/事件冻结（`propose_objective`…`record_outcome`） | `company_commands_are_frozen_and_versioned` |
| `P3-I-03` | I Company 生命周期 | 新进程可从事件与 Artifact 引用重建全链 | `new_process_rebuilds_the_company_chain` |
| `P3-I-04` | I Company 生命周期 | criteria snapshot 冻结；Reviewer 不改写 Builder 原始事实 | `reviewer_cannot_rewrite_builder_facts` |
| `P3-I-05` | I Company 生命周期 | Project 关闭需 Acceptance+Delivery+ClosingReceipt 或显式豁免；Outcome 不自动夸大 | `outcome_cannot_be_claimed_without_measurement` |
| `P3-I-06` | I Company 生命周期 | 端到端产出完整 ClosingReceipt | `fake_model_coding_project_produces_closing_receipt` |

- [ ] **Step 2: 在 §7 写入 P3 的 6 张卡**

「依据」统一指向 `company-os-implementation-outline.md` §Slice I 与 `company-os-domain-contracts.md`。

- [ ] **Step 3: 验证**

Run: `grep -cE '^\| \`P3-' docs/roadmap.md && grep -cE '^### P3-' docs/roadmap.md`
Expected: `6` 与 `6`

- [ ] **Step 4: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): add P3 master-table rows and detail cards"
```

---

## Task 6: P4 总图行与详细卡

**Files:**
- Modify: `docs/roadmap.md`（§1 总图、新增 §8）

- [ ] **Step 1: 写入 P4 的 9 行**

| 编号 | 切片 | 退出条件 | 验收测试名 |
|---|---|---|---|
| `P4-J6-01` | J6 Swarm | fan-out 有 parent/partition/预算/并发/TTL/WorkFingerprint/MergeDecision | `swarm_fanout_is_bounded_and_merges_deterministically` |
| `P4-J7-02` | J7 Provider/Output | additive `sequence`/`epoch`；`PROTOCOL_SCHEMA` 不动 | `run_stream_sequence_is_monotonic` |
| `P4-J7-03` | J7 Provider/Output | Usage/ToolCall/ApprovalRequested/Error 投影；terminal 重放给迟到订阅者 | `terminal_is_replayed_to_late_subscriber` |
| `P4-K2-01` | K2 Trigger | Trigger 只能创建 Workflow/Run，不能直接执行 Capability | `trigger_cannot_execute_a_capability_directly` |
| `P4-K8-01` | K8 Connector | 不绕过 ControlPlane/Approval/Idempotency/Receipt/reconciliation | `connector_cannot_bypass_the_control_plane` |
| `P4-L3-01` | L3 Version governance | ModelProfile/PromptBundle/RouteDecision/DriftReport 按版本分桶 | `drift_report_is_bucketed_by_version` |
| `P4-L5-01` | L5 Extension | skill `allowed-tools` 不进 policy；read-only 扩展写操作在 broker 拒绝 | `skill_allowed_tools_cannot_grant_shell` |
| `P4-L6-01` | L6 Supply chain | content hash/license/signature/capability diff/rollback 可审计 | `extension_signature_is_verified_before_install` |
| `P4-M6-01` | M6 Desktop shell | workspace onboarding/health/tray/background/safe close | `desktop_safe_close_leaves_no_orphan_process` |

- [ ] **Step 2: 在 §8 写入 P4 的 9 张卡**

- [ ] **Step 3: 验证**

Run: `grep -cE '^\| \`P4-' docs/roadmap.md && grep -cE '^### P4-' docs/roadmap.md`
Expected: `9` 与 `9`

- [ ] **Step 4: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): add P4 master-table rows and detail cards"
```

---

## Task 7: §9 P5/P6 边界登记、§10 冻结项、§13 文档维护项

**Files:**
- Modify: `docs/roadmap.md`（新增 §9、§10、§13）

- [ ] **Step 1: 写 §9 P5/P6 边界登记**

```markdown
## 9. P5 / P6 边界登记（不写卡）

本阶段**只登记边界与"为什么不现在写"**，不写六行卡。

| 阶段 | 范围 | 为什么现在不写 | 依据 |
|---|---|---|---|
| P5 | Office / Work | 发送、提交和外部资料修改属 R3，需最终 payload 单次确认 | `company-os-implementation-outline.md` §4 |
| P5 | Search / Recommendation | 先做只读搜索、来源、时间和新鲜度 | 同上 |
| P5 | Commerce / Food | 购物车/下单/支付/退款属 R4，必须绑定商户、商品、数量、总成本等 digest | 同上 |
| P5 | Mobility / Travel | 出票、打车、订房、改签需单次确认与 Unknown 对账 | 同上 |
| P5 | Home / IoT | 门锁、摄像、燃气、固件属 R5，默认禁止自治 | 同上 |
| P6 | Team / Remote / Enterprise | 需要 durable principal、跨机身份与供应链证据，仓库当前证明上限为 `local_behavior` | `company-os-spec-index.md` §7 |

进入条件：P0–P4 的对应能力（尤其 `P0-K1-01` 身份、`P1-J4-01` MCP、`P4-K8-01` Connector）先落地。
```

- [ ] **Step 2: 写 §10 冻结与不做项**

```markdown
## 10. 冻结与不做项

| 项 | 状态 | 说明 | 依据 |
|---|---|---|---|
| `kiana-entrypoints/src/runner.rs` 的第二条执行循环 | ❓ 待拍板 | 直连 `execute_tool_calls_with_permission_handler`，绕过 broker 与 ControlPlane；两条路：(a) 迁到 `DaemonHost` 脊柱，(b) 标记 legacy 并冻结且不进产品帮助文案 | 旧 `7.1` |
| 新增模型可见工具 | 🚫 冻结 | 保持 5 个 | `AGENTS.md` §7 |
| Builder 参与规划/监控 symposium | 🚫 冻结 | 参会边界冻结 | `CLAUDE.md` |
| HTTP MCP | 🚫 冻结 | 只支持 stdio | `CLAUDE.md` |
| 第二套执行循环或控制面 | 🚫 冻结 | 产品脊柱只有 `DaemonHost` | `AGENTS.md` §8 |

**决定前不动代码**：❓ 项需要先产出书面决定 + 文档更新。
```

- [ ] **Step 3: 写 §13 文档维护项**

```markdown
## 13. 文档维护项（不属于产品单元，不进总图）

- [ ] 重审 `codex` / `deepseek-harness` / `goose` 三份审计（旧 `8.1`）
- [ ] 新增 `grok-build` 审计 + 补审计 9 项（旧 `8.2`）
```

- [ ] **Step 4: 验证三节存在**

Run: `grep -cE '^## (9|10|13)\.' docs/roadmap.md`
Expected: `3`

- [ ] **Step 5: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): register P5/P6 boundaries, frozen items, and doc maintenance"
```

---

## Task 8: §2 当前窗口回填与 §3 变更日志

**Files:**
- Modify: `docs/roadmap.md`（§2、§3）

- [ ] **Step 1: 回填 §2 当前窗口**

先取真实状态，再写入：

```bash
git log --oneline -5
gh run list --limit 6
git status --short
```

§2 必须反映：当前 HEAD、CI 最新 run 的结论（`success`/`failure`/`cancelled` 三态要区分）、工作树未提交的改动属于哪个单元。

- [ ] **Step 2: 在 §3 追加变更日志行**

格式沿用现有四列 `日期 | 做了什么 | 提交`，为本次补全的每个提交各加一行。

- [ ] **Step 3: 验证变更日志有本次条目**

Run: `grep -c 'roadmap' docs/roadmap.md`
Expected: ≥ 1（§3 新增行）

- [ ] **Step 4: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): refresh in-flight status and changelog"
```

---

## Task 9: 迁移清理（删除旧编号与旧小节）

**Files:**
- Modify: `docs/roadmap.md`

**Interfaces:**
- Consumes: Task 2–8 的全部新内容。
- Produces: 完成态文档，无旧编号残留。

- [ ] **Step 1: 列出旧编号残留**

Run: `grep -nE '(^\| \*\*[0-8]\.[0-9]|^### [0-8]\.[0-9])' docs/roadmap.md`
Expected: 列出所有待删的旧小节与旧表行

- [ ] **Step 2: 逐条删除**

删除旧 §4–§10 的 P0–P7 小节标题与其下已被新卡覆盖的内容。保留：§0 用法、§3 变更日志、§11 追加区、§12 完成定义。

- [ ] **Step 3: 验证旧编号清零**

Run: `grep -cE '(^\| \*\*[0-8]\.[0-9]|^### [0-8]\.[0-9])' docs/roadmap.md`
Expected: `0`

- [ ] **Step 4: 验证只剩一套 P 编号**

Run: `grep -oE 'P[0-9]' docs/roadmap.md | sort -u | tr '\n' ' '`
Expected: 只出现 `P0 P1 P2 P3 P4 P5 P6`

- [ ] **Step 5: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): remove legacy 0.x-8.x numbering and superseded sections"
```

---

## Task 10: 最终验收

**Files:**
- Modify: 无（只读校验）；如失败则回到对应任务修复

- [ ] **Step 1: 覆盖 38 个实施单元**

Run:
```bash
grep -oE '^\| \`P[0-6]-(A|B|C|D|E|F|G|H|I|J[1-8]|K[1-8]|L[1-6]|M[1-7])-' docs/roadmap.md \
  | sed -E 's/^\| `P[0-6]-//; s/-$//' | sort -u | wc -l
```
Expected: `38`

- [ ] **Step 2: 卡片行数上限**

Run: 同 Task 2 Step 4 的 awk 命令
Expected: 无输出

- [ ] **Step 3: 每张卡都有「依据」**

Run: `[ "$(grep -cE '^### P[0-6]-' docs/roadmap.md)" = "$(grep -c '^- \*\*依据\*\*' docs/roadmap.md)" ] && echo OK`
Expected: `OK`

- [ ] **Step 4: 同一编号不重复出现**

Run:
```bash
grep -oE '^\| \`P[0-6]-[A-Z0-9]+-[0-9]+' docs/roadmap.md | sort | uniq -d
```
Expected: 无输出（每个编号只出现一次，即只属一个阶段）

- [ ] **Step 5: spec §7 五条逐条对照**

对照 `docs/superpowers/specs/2026-09-10-roadmap-completion-design.md` §7，逐条记录 pass/fail。

- [ ] **Step 6: 提交（需用户显式授权）**

```bash
git add docs/roadmap.md
git commit -m "docs(roadmap): complete P0-P6 execution skeleton"
```
