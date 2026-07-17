# Volume 02: Workflow Runtime 与状态机

## 1. WorkflowRun 定义

WorkflowRun 是 Kiana 中复杂任务的持久运行单元。它不是一次聊天，也不是一个模型 turn，也不是一个 git branch。它是一个可恢复、可审计、可分解、可验证的项目推进容器。

WorkflowRun 负责承载：

- 目标。
- 约束。
- 运行状态。
- DAG 节点。
- 任务卡。
- 事件日志。
- 执行结果。
- 验证结果。
- 审查结果。
- 学习记录。

## 2. WorkflowRun 与其他对象的关系

```text
Session
  has many WorkflowRun

WorkflowRun
  has many TaskCard
  has many Event
  has many WorkPacket
  has many ResultPacket
  has many VerificationPacket
  has many ReviewPacket

TaskCard
  may have one or more WorkPacket

WorkPacket
  produces ResultPacket

ResultPacket
  feeds VerificationPacket

VerificationPacket
  feeds ReviewPacket

ReviewPacket
  feeds Ship or Fix Loop
```

## 3. 目录结构

```text
.kiana/workflows/<workflow_id>/
  workflow.yaml
  state.json
  eventlog.jsonl
  context_pack.md
  findings.md
  task_plan.md
  plan-confirmation.md
  decision-log.md
  workpackets/
  resultpackets/
  verification/
  review/
  reports/
  pr/
  learnings.md
```

### 3.1 `workflow.yaml`

职责：

- 保存运行元数据。
- 保存目标和模式。
- 保存 approval 策略。
- 保存 verification profile。
- 保存 artifact 目录。

它是相对稳定的配置文件，运行中不应频繁改写。

允许变更：

- 通过 decision event 修改 goal。
- 通过 approval event 修改允许操作。
- 通过 plan revision 修改 DAG template。

禁止：

- 工具执行时直接改 goal。
- worker 自己改 workflow policy。
- 手写覆盖导致 eventlog 无法解释。

### 3.2 `state.json`

职责：

- 保存当前节点。
- 保存 Kanban 状态。
- 保存 project fingerprint。
- 保存 memory stale 状态。
- 保存 last_event_id。

`state.json` 是 eventlog 的 materialized view。

要求：

- 可以通过 eventlog 重建。
- 每次写入带 last_event_id。
- crash 后可以校验是否落后。

### 3.3 `eventlog.jsonl`

职责：

- append-only。
- 记录所有状态变化。
- 记录所有 gate 决策。
- 记录所有 command result。
- 记录所有 approval。

事件类型：

- `workflow_created`
- `capture_completed`
- `context_pack_built`
- `task_created`
- `task_status_changed`
- `workpacket_created`
- `worker_started`
- `command_started`
- `command_completed`
- `result_packet_created`
- `verification_completed`
- `review_completed`
- `approval_requested`
- `approval_granted`
- `approval_rejected`
- `blocked`
- `resumed`
- `forked`
- `learned`

## 4. Workflow 节点

### 4.1 Runtime Init

输入：

- user request。
- cwd。
- current git state。
- selected mode。

动作：

1. 生成 workflow_id。
2. 创建 artifact directory。
3. 写 workflow.yaml。
4. 写 initial state.json。
5. 写 workflow_created event。

失败：

- artifact directory 已存在：生成新 id 或 resume。
- 无写权限：Block。
- cwd 非项目目录：Ask/Block。

### 4.2 Capture

提取：

- goal。
- success criteria。
- constraints。
- risk。
- approval needs。
- NOT_BUILDING。
- input type。

Gate：

- clear -> Context Intake。
- unclear -> Clarify。
- too large -> Split。
- high risk -> Approval Required。
- unsafe -> Cancel/Block。

### 4.3 Context Intake

动作：

1. Restore previous state if resuming。
2. Load memory summary。
3. Check project fingerprint。
4. Targeted retrieval。
5. Detect stack。
6. Detect scripts/tests/CI。
7. Build ContextPack。

Gate：

- fresh -> Research。
- partial/stale -> Refresh。
- major project change -> Rebuild memory。
- too large -> Compact。
- dirty git -> Isolation Decision。

### 4.4 Research

动作：

- search code/docs/logs/issues。
- search existing patterns。
- trace affected symbols。
- record findings。

Gate：

- enough -> Design。
- missing facts -> more research。
- conflict -> resolve。
- critical unknown -> ask or mark unknown。

### 4.5 Design Candidate

动作：

- generate design candidate。
- map affected modules。
- estimate risk surface。
- define interfaces。
- define test surface。
- define compatibility impact。
- define NOT_BUILDING。

Gate：

- accepted -> Plan。
- tradeoff -> Discuss。
- mismatch -> Capture。
- high architecture risk -> Alternative。
- requires approval -> Approval。

### 4.6 Plan

动作：

- create/update task_plan.md。
- decompose tasks。
- define dependencies。
- define verification strategy。
- define rollback。
- define review strategy。
- define DAG node dependencies。

Gate：

- executable -> Plan Confirmation。
- too vague -> Re-plan。
- missing tests -> Add test strategy。
- missing verification -> Add commands。
- missing scope limits -> Add NOT_BUILDING。

### 4.7 Plan Confirmation

动作：

- verify referenced files exist。
- verify target patterns still exist。
- verify commands available。
- verify branch/worktree state。
- verify scope limits captured。
- verify acceptance criteria testable。

Gate：

- continue -> WorkPacket。
- repo changed -> Context Intake。
- needs user decision -> Discuss。
- invalid -> Plan。

### 4.8 WorkPacket

动作：

- define task goal。
- define in-scope。
- define out-of-scope。
- define allowed/forbidden files。
- attach tests。
- attach verification commands。
- attach rollback。
- attach risk flags。
- attach review focus。
- attach retry policy。

Gate：

- valid -> Router。
- missing bounds -> Plan。
- risky -> Approval。
- invalid dependency -> Plan。

### 4.9 Router

动作：

- classify task type。
- select commands/skills。
- select specialist agents。
- select rules pack。
- select verification profile。
- select execution mode。

Gate：

- single -> Execute。
- parallel -> Dispatch Bounded Swarm。
- missing rule pack -> Generic Safe Rules。
- needs isolation -> Worktree。

### 4.10 Execute

动作：

- read WorkPacket。
- lock scope。
- write failing test if required。
- minimal implementation。
- bounded edit。
- targeted check after file change。
- collect changed files。
- detect out-of-scope diff。

Result：

- step done -> Quality Gate。
- failed -> Fix Loop。
- requirement problem -> Plan。
- architecture problem -> Discuss。
- risk discovered -> Approval。

### 4.11 Quality Gate

检查：

- format。
- lint。
- type。
- build。
- static security。
- dependency/license。

结果：

- pass -> Behavior Verify。
- fail -> Fix Loop。
- security finding -> Security Fix/Block。

### 4.12 Behavior Verify

检查：

- targeted tests。
- regression tests。
- acceptance criteria。
- user goal satisfaction。
- NOT_BUILDING not violated。

结果：

- pass -> Review Scope。
- test failed -> Fix Loop。
- tests missing -> Add Tests。
- cannot verify -> Block。

### 4.13 Review Scope

动作：

- collect changed files。
- collect diff summary。
- collect rules。
- copy NOT_BUILDING。
- add review focus。
- add verification evidence。
- write review/scope.md。

### 4.14 Multi Review

审查角色：

- Product Review。
- Architecture Review。
- Code Review。
- Security Review。
- QA/Test Coverage Review。
- Docs/DX Review。
- Error Handling Review。
- Red Team Review。

ReviewPacket 字段：

- reviewer。
- focus。
- findings。
- severity。
- confidence。
- evidence。
- recommendation。

### 4.15 Review Triage

finding 决策：

- FIX。
- SKIP with reason。
- BLOCKED。
- ASK USER。

禁止：

- blocking finding 直接跳过。
- 没有 skip reason。
- 把 review 文本当 verification。

### 4.16 Fix Loop

动作：

1. read failure/finding packet。
2. classify root cause。
3. apply minimal fix。
4. record fix attempt。
5. check scope did not expand。

Gate：

- fixed -> Quality Gate。
- retryable -> retry。
- need decision -> Ask。
- need re-plan -> Plan。
- budget exceeded -> Block。

### 4.17 Ship

动作：

- prepare delivery summary。
- summarize changed files。
- summarize tests/checks。
- summarize review findings。
- summarize risks/follow-ups。

Gate：

- report only -> Learn。
- create PR draft -> PR artifact。
- push/merge/deploy -> Approval。
- rejected -> Block。

### 4.18 Learn

动作：

- append eventlog。
- update progress。
- write learnings。
- propose memory updates。
- update project fingerprint。
- update workflow metrics。

Gate：

- next WorkPacket -> Loop Controller。
- next Goal -> Loop Controller。
- done -> Completed。

## 5. 状态集合

### 5.1 WorkflowRun 状态

| 状态 | 含义 |
| --- | --- |
| `created` | artifact directory 已创建 |
| `capturing` | 正在抽取目标和约束 |
| `contextualizing` | 正在构建上下文 |
| `researching` | 正在查证 |
| `designing` | 正在形成设计候选 |
| `planning` | 正在拆任务 |
| `confirming_plan` | 正在确认计划可执行 |
| `ready` | 有 Ready task |
| `executing` | 正在执行 |
| `verifying` | 正在验证 |
| `reviewing` | 正在审查 |
| `fixing` | 正在修复 |
| `blocked` | 需要用户或外部状态 |
| `completed` | 完成 |
| `cancelled` | 取消 |

### 5.2 Task 状态

| 状态 | 含义 |
| --- | --- |
| `draft` | 未确认 |
| `ready` | 依赖满足，可执行 |
| `doing` | 正在执行 |
| `review` | 等待审查 |
| `blocked` | 被阻塞 |
| `done` | 完成 |
| `cancelled` | 取消 |

### 5.3 Worker 状态

| 状态 | 含义 |
| --- | --- |
| `assigned` | 已收到 WorkPacket |
| `running` | 正在执行 |
| `waiting` | 等待命令或资源 |
| `reporting` | 正在提交 ResultPacket |
| `failed` | 执行失败 |
| `completed` | 执行完成 |
| `reaped` | 被回收 |

## 6. 允许迁移

WorkflowRun 允许迁移：

```text
created -> capturing
capturing -> contextualizing
contextualizing -> researching
researching -> designing
designing -> planning
planning -> confirming_plan
confirming_plan -> ready
ready -> executing
executing -> verifying
verifying -> reviewing
reviewing -> fixing
fixing -> verifying
reviewing -> completed
any -> blocked
blocked -> contextualizing
blocked -> planning
blocked -> ready
any -> cancelled
```

禁止迁移：

- `created -> completed`
- `executing -> completed` without verification。
- `verifying -> completed` without review decision。
- `blocked -> done` without unblock evidence。
- `worker failed -> integrated` without triage。

## 7. Crash Recovery

恢复流程：

1. 读取 workflow.yaml。
2. 读取 state.json。
3. 读取 eventlog.jsonl。
4. 校验 `state.last_event_id` 是否存在。
5. 从 eventlog 重建 state。
6. 比对重建 state 和 state.json。
7. 如果不一致，以 eventlog 为准并写 `state_repaired` event。
8. 检查 git fingerprint。
9. 若 dirty state 与记录不同，进入 Dirty State Gate。
10. 选择 next node。

### 7.1 恢复结果不是布尔值

恢复必须产生结构化结果：

| 结果 | 含义 | 允许行为 |
| --- | --- | --- |
| `resumed` | EventLog、state、artifact 和 git fingerprint 一致 | 继续 next node |
| `repaired` | state 可由完整 EventLog 确定性重建 | 追加 repair event 后继续 |
| `blocked` | EventLog 损坏、packet 半发布或 dirty ownership 不明确 | 停止执行并给出修复动作 |
| `fork_required` | 用户修改与 agent 未完成修改无法安全合并 | 创建隔离 run/worktree |
| `cancelled` | 用户明确终止 | 只允许写终止与学习事件 |

禁止在恢复失败时回退到“看起来合理”的默认节点。`state.json` 缺失可以重建，EventLog 中间损坏不能靠跳过坏行继续；后者必须进入 reconcile/block 流程。

### 7.2 Writer Lease 与并发一致性

同一 WorkflowRun 的 EventLog 同一时刻只允许一个 writer。运行目录使用 `.eventlog.lock` 表示 writer lease，lease 至少记录持有者 PID 和创建时间。

回收规则：

1. Linux 上若 PID 可检查且进程仍存在，返回 `WriterBusy`。
2. Linux 上若 PID 已不存在，锁可判定为 stale 并回收。
3. 无法可靠判断 PID 存活时，只有超过配置 TTL 才能回收。
4. 回收、唯一性检查、event append 和 state 投影必须在同一临界区完成。
5. 进程退出前释放 lease；异常退出由下一次恢复按上述规则处理。

显式 event ID 或关键 data key 的唯一性不能采用“先无锁查询、后加锁追加”。必须在持有 writer lease 时执行 `check + append`，否则两个 worker 可能同时通过预检查并写入重复完成事实。

### 7.3 Artifact 两阶段发布

VerificationPacket、ReviewPacket 等完成证据采用两阶段发布：

1. 在目标目录创建同文件系统临时文件。
2. 写入完整 JSON、flush，并完成 schema/引用校验。
3. 以不可覆盖方式发布为最终文件名。
4. 最终文件已存在时返回冲突，不得覆盖。
5. 崩溃遗留的 `.tmp` 文件不参与完成投影，并由 strict audit 记录为 blocker。

这样可以区分“文件正在生成”“文件已经完整发布”和“同 ID 重复发布”三种状态，避免普通 rename 覆盖旧证据。

## 8. Dirty State Gate

分类：

- `clean`：可继续。
- `agent_dirty`：Kiana 上次修改未完成，可恢复或回滚。
- `user_dirty`：用户中途修改，必须保护。
- `mixed_dirty`：需要隔离或确认。

行为：

| 分类 | 行为 |
| --- | --- |
| clean | continue |
| agent_dirty | resume/fix/rollback |
| user_dirty | rebuild ContextPack，避免覆盖 |
| mixed_dirty | ask 或 create worktree |

## 9. Fork Model

fork 用于：

- 尝试替代方案。
- 从 blocked 状态拆出探索。
- 并行实验。

规则：

- fork 创建新 workflow_id。
- parent workflow 不共享 mutable state。
- fork 复制 goal/context snapshot。
- fork 写 parent_workflow_id。
- merge/final adoption 需要 decision event。

## 10. Event Sourcing 原则

Kiana 不需要一开始实现完整事件溯源框架，但必须遵守：

- 关键状态变化写 event。
- event append-only。
- state 可重建。
- packet 可追溯。
- report 可追溯到 evidence。
- 唯一性检查与 append 在同一 writer lease 内。
- packet 发布不可覆盖，临时文件不可视为完成事实。
- blocker 解除通过 superseding event 表达，不删除历史事件。

这样后续 dashboard、cloud workspace、team runtime 都能复用同一条事件流。
