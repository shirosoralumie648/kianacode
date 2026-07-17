# Volume 11: 数据契约与事件分类

## 1. 设计目标

Kiana 的核心不是某一个 UI 或某一套 prompt，而是一组稳定的数据契约。只要这些契约稳定，CLI、TUI、Web、云端 worker、企业离线、插件生态都可以共享同一套运行事实。

本卷定义：

- 数据对象边界。
- 事件分类。
- packet 生命周期。
- schema 兼容策略。
- projection 规则。
- 错误和降级语义。

## 2. 契约分层

Kiana 数据契约分为五层：

| 层 | 名称 | 作用 |
| --- | --- | --- |
| L1 | Runtime Contract | session、turn、event、tool call、tool result |
| L2 | Workflow Contract | WorkflowRun、state、DAG、gate、node |
| L3 | Work Contract | TaskCard、WorkPacket、ResultPacket |
| L4 | Evidence Contract | EvidenceEvent、VerificationPacket、ReviewPacket、Report |
| L5 | Trust Contract | PolicyDecision、Approval、PluginReceipt、MCP provenance |

设计原则：

- 下层不依赖上层。
- 上层可以引用下层 id。
- 所有对象必须有 schema_version。
- 所有持久对象必须有 created_at 或 timestamp。
- 所有状态变化必须有 event。

## 3. ID 规范

ID 使用带前缀的稳定字符串：

| 对象 | 前缀 | 示例 |
| --- | --- | --- |
| WorkflowRun | `wf_` | `wf_20260709_001` |
| TaskCard | `task_` | `task_001` |
| WorkPacket | `wp_` | `wp_001` |
| ResultPacket | `rp_` | `rp_001` |
| VerificationPacket | `vp_` | `vp_001` |
| ReviewPacket | `rv_` | `rv_001` |
| EvidenceEvent | `evt_` | `evt_0012` |
| PolicyDecision | `pol_` | `pol_001` |
| Approval | `ap_` | `ap_001` |
| Memory | `mem_` | `mem_001` |

ID 要求：

- 在 workflow 内唯一。
- 可读。
- 不依赖数据库自增。
- 可在文件名中使用。

## 4. Runtime Contract

### 4.1 Session

Session 表示用户与 Kiana 的长期交互入口。

字段：

- session_id。
- user_id optional。
- workspace_root。
- created_at。
- current_workflow_id。
- active_mode。
- model_profile。
- permission_profile。

Session 不等于 WorkflowRun。一个 Session 可以跨多个 WorkflowRun。

### 4.2 Turn

Turn 表示一轮模型请求和工具循环。

字段：

- turn_id。
- session_id。
- workflow_id optional。
- input。
- selected_mode。
- route_decision_id。
- started_at。
- completed_at。
- stop_reason。
- tool_calls。
- token_usage optional。

Turn 可被压缩，但关键事件必须进入 eventlog。

### 4.3 RuntimeEvent

RuntimeEvent 是 UI 和 SDK 投影的基础。

类型：

- assistant_delta。
- tool_call_started。
- tool_call_completed。
- command_output。
- file_changed。
- route_decision。
- policy_decision。
- turn_completed。

RuntimeEvent 可以是 transient，也可以被重要事件转写进 Evidence Ledger。

## 5. Workflow Contract

### 5.1 WorkflowRun

必需字段：

- workflow_id。
- schema_version。
- goal。
- mode。
- status。
- artifact_dir。
- created_at。
- current_node。
- verification_profile。
- policy_profile。

可选字段：

- parent_workflow_id。
- fork_reason。
- owner。
- tags。
- domain。

### 5.2 WorkflowState

WorkflowState 是 eventlog 的投影。

字段：

- workflow_id。
- status。
- current_node。
- kanban。
- dirty_state。
- memory_state。
- project_fingerprint。
- last_event_id。

规则：

- 可被重建。
- 不做唯一事实源。
- 写入必须记录 event id。

### 5.3 WorkflowDAG

DAG 描述节点和依赖。

节点类型：

- capture。
- context。
- research。
- design。
- plan。
- confirm。
- workpacket。
- route。
- execute。
- verify。
- review。
- ship。
- learn。

边类型：

- normal。
- retry。
- fallback。
- approval。
- blocked。
- loop。

## 6. Work Contract

### 6.1 TaskCard

TaskCard 是最小业务任务。

字段分类：

- identity。
- objective。
- scope。
- dependencies。
- acceptance。
- verification。
- risk。
- evidence。
- status。

TaskCard 不应包含：

- 大段 prompt。
- worker 私有思考。
- 未验证 memory。

### 6.2 WorkPacket

WorkPacket 是 worker 执行输入。

原则：

- immutable。
- scope-bounded。
- evidence-aware。
- retryable。

WorkPacket 一旦派发，不应原地修改；需要新版本时创建 `wp_001_r2` 或 revision event。

### 6.3 ResultPacket

ResultPacket 是执行事实。

包含：

- changed_files。
- commands。
- output summaries。
- errors。
- deviations。
- suggested_next。

它不包含成功判断，成功判断属于 VerificationPacket。

## 7. Evidence Contract

### 7.1 EvidenceEvent

EvidenceEvent 是最小证据单元。

kind：

- command_result。
- test_result。
- build_result。
- diff_summary。
- file_check。
- review_finding。
- approval。
- blocker。
- eda_artifact_check。

status：

- pass。
- fail。
- blocked。
- skipped。
- unknown。

### 7.2 VerificationPacket

VerificationPacket 汇总一组 evidence。

字段：

- verification_id。
- profile。
- checks。
- final_status。
- required_checks。
- optional_checks。
- skipped_checks。
- evidence_ids。

### 7.3 ReviewPacket

ReviewPacket 表示审查结果。

字段：

- reviewer_type。
- scope。
- findings。
- blocking_count。
- recommendation。
- evidence_ids。

## 8. Trust Contract

### 8.1 PolicyDecision

PolicyDecision 对所有敏感动作统一建模。

字段：

- subject。
- capability。
- target。
- action。
- decision。
- reason。
- policy_source。
- requires_approval。

### 8.2 ApprovalRecord

ApprovalRecord 保存用户批准。

字段：

- approval_id。
- requested_action。
- exact_operation。
- risk_summary。
- approved_by。
- approved_at。
- scope。
- expires。

Approval 必须绑定 exact operation。命令改变后需要重新 approval。

## 9. Event Taxonomy

### 9.1 Lifecycle Events

- workflow_created。
- workflow_resumed。
- workflow_forked。
- workflow_completed。
- workflow_cancelled。
- workflow_blocked。

### 9.2 Planning Events

- capture_completed。
- context_pack_built。
- findings_recorded。
- design_candidate_created。
- decision_recorded。
- plan_created。
- plan_revised。
- task_created。
- task_split。

### 9.3 Execution Events

- workpacket_created。
- worker_assigned。
- worker_started。
- command_started。
- command_completed。
- file_changed。
- resultpacket_created。

### 9.4 Verification Events

- verification_started。
- check_completed。
- verification_completed。
- acceptance_checked。
- not_building_checked。

### 9.5 Review Events

- review_scope_created。
- review_started。
- finding_created。
- finding_triaged。
- review_completed。

### 9.6 Trust Events

- policy_checked。
- approval_requested。
- approval_granted。
- approval_rejected。
- plugin_enabled。
- mcp_server_started。
- hook_registered。

### 9.7 Learning Events

- learning_recorded。
- memory_proposed。
- memory_accepted。
- memory_rejected。
- memory_marked_stale。

## 10. Packet 生命周期

### 10.1 TaskCard 生命周期

```text
draft -> ready -> doing -> review -> done
                    |        |
                    v        v
                 blocked   fixing
```

### 10.2 WorkPacket 生命周期

```text
created -> assigned -> running -> reported -> integrated
                          |          |
                          v          v
                       failed      rejected
```

### 10.3 VerificationPacket 生命周期

```text
created -> running -> pass
                  -> fail
                  -> blocked
                  -> inconclusive
```

## 11. 兼容策略

Schema 版本规则：

- patch 增加可选字段。
- minor 增加对象类型。
- major 改变语义。

读取旧 schema：

- 尽量 migrate。
- migration 写 event。
- 无法 migrate 则 block。

写入新 schema：

- 必须带 schema_version。
- 必须通过 schema validation。
- 必须保持 unknown fields 不破坏旧读取器。

## 12. Projection 规则

Projection 是从 eventlog/state 生成视图。

视图：

- board。
- report。
- timeline。
- dashboard。
- worker status。
- evidence summary。

规则：

- projection 可重建。
- projection 不应成为唯一事实源。
- projection 错误不应破坏 eventlog。

## 13. 错误语义

错误分类：

- validation_error。
- policy_denied。
- approval_required。
- missing_artifact。
- stale_context。
- dirty_state_conflict。
- worker_failed。
- verification_failed。
- review_blocked。

错误必须携带：

- code。
- message。
- evidence。
- suggested_next。
- recoverable。

## 14. 数据契约验收

P0 验收：

- 能创建 workflow。
- 能写 eventlog。
- 能重建 state。
- 能创建 task card。
- 能创建 evidence event。
- 能校验 schema。
- 能报告旧 schema 不兼容。
