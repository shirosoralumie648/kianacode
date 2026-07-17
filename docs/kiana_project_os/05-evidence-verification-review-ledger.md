# Volume 05: Evidence、Verification、Review 与 Ledger

## 1. Evidence Ledger 定义

Evidence Ledger 是 Kiana 的完成证明系统。它回答：

- 做了什么。
- 改了哪里。
- 跑了什么命令。
- 结果是什么。
- 谁审查了。
- 哪些风险还在。
- 为什么可以说 Done。

没有 evidence，Kiana 不能宣称完成。

## 2. Evidence 类型

| 类型 | 说明 |
| --- | --- |
| command_result | 命令执行结果 |
| diff_summary | 文件修改摘要 |
| test_result | 测试结果 |
| build_result | 构建结果 |
| lint_result | lint/format/typecheck |
| security_result | 安全扫描/审查 |
| review_finding | 审查发现 |
| approval_record | 用户批准 |
| blocker_record | 阻塞原因 |
| artifact_check | 文件/产物完整性检查 |
| eda_check | 硬件资料审查 |
| memory_decision | memory 写入/拒绝原因 |

## 3. Evidence Event 字段

必需字段：

- event_id。
- workflow_id。
- timestamp。
- kind。
- status。
- summary。
- source。
- evidence payload。

可选字段：

- task_id。
- workpacket_id。
- command。
- changed_files。
- reviewer。
- confidence。
- severity。
- next_action。

## 4. Verification Gate

Verification Gate 是自动/半自动验证层。

检查类型：

- format。
- lint。
- type。
- build。
- unit tests。
- integration tests。
- smoke tests。
- security scan。
- dependency/license。
- acceptance criteria。

### 4.1 Profile

不同任务使用不同 profile：

| Profile | 用途 |
| --- | --- |
| quick | L1 小任务 |
| task_default | L2 单任务 |
| project_p0 | P0 项目功能 |
| release | 发布前 |
| audit_strict | 严格审计 |
| eda_review | 硬件审查 |

### 4.2 Pass 条件

Verification pass 需要：

- 必需命令成功。
- acceptance criteria 满足。
- NOT_BUILDING 未违反。
- 无阻断错误。
- evidence 写入。

### 4.3 Fail 条件

Verification fail：

- 命令失败。
- 测试失败。
- build 失败。
- 缺少必要验证。
- acceptance 未满足。
- 发现 scope violation。

fail 后进入：

- Fix Loop。
- Add Tests。
- Re-plan。
- Block。

## 5. Review Gate

Review Gate 是判断“验证通过是否足够”的层。

Review 类型：

- Product Review。
- Architecture Review。
- Code Review。
- Security Review。
- QA Review。
- Docs/DX Review。
- Error Handling Review。
- Red Team Review。
- EDA Review。

### 5.1 Finding 字段

- finding_id。
- severity。
- confidence。
- component。
- evidence。
- recommendation。
- decision。

Severity：

- blocker。
- high。
- medium。
- low。
- note。

Decision：

- fix。
- skip。
- block。
- ask_user。

### 5.2 Blocking Finding

blocking finding 不能自动跳过。

允许路径：

- fix。
- block。
- ask user。
- 用户明确接受风险。

必须记录：

- reason。
- evidence。
- owner。
- next action。

## 6. ResultPacket

ResultPacket 是执行结果，不是完成证明。

字段：

- workpacket_id。
- task_id。
- changed_files。
- commands_run。
- errors。
- notes。
- scope_deviations。
- suggested_next。

ResultPacket 后必须经过 Verification。

## 7. VerificationPacket

字段：

- verification_id。
- task_id。
- profile。
- checks。
- pass_count。
- fail_count。
- skipped_count。
- blocked_count。
- final_status。
- evidence_events。

Skipped 必须有 reason。

### 7.1 VerificationPacket 完成事实

VerificationPacket 自身不是完成事实。Board、report、audit 只有同时满足以下条件，才承认一次 verification：

1. `verification_id` 只包含 ASCII 字母、数字、`-`、`_`，长度不超过 128。
2. packet 路径位于当前 run 的 `verification/` 内，不能通过 `..` 或 symlink 逃逸。
3. EventLog 存在 `VerificationCompleted`。
4. 事件中的 `verification_id`、`workflow_id`、`run_id`、`final_status`、`packet_path` 与 packet 完全一致。
5. required check 的 evidence event 真实存在，并属于相同 WorkflowRun 和 Task。
6. Task metadata 中的 verification 引用与最终 packet 路径和 ID 完全一致。

只有 packet、完成事件或 Task 引用中的任意一个都不够。孤儿 packet、伪造 pass JSON、跨 run 引用和路径穿越必须投影为 blocker。

### 7.2 不可覆盖发布

VerificationPacket 使用临时文件写入后不可覆盖发布。相同 `verification_id` 已存在时返回冲突，不能用新内容替换旧证明。`verification/` 中残留 `.tmp` 文件表示上一次发布可能在中途崩溃，strict audit 必须阻塞发布，直到人工或恢复流程完成 reconcile。

## 8. ReviewPacket

字段：

- review_id。
- reviewer_type。
- input_scope。
- findings。
- score。
- blocking_count。
- recommendation。

ReviewPacket 不等于测试。

### 8.1 ReviewPacket 信任边界

ReviewPacket 的可信条件：

- `review_id` 使用与 artifact 文件名兼容的安全 ID。
- packet 路径必须保留在当前 run 的 `review/` 目录。
- `workflow_id/run_id` 必须与被审计运行一致。
- `evidence_refs` 中每个 ID 都必须在当前 EventLog 中存在。
- 每个 finding 必须恰好绑定一个唯一 evidence ref。
- evidence ref 不能跨 workflow、跨 run 或重复支撑多个 finding。
- `ReviewCompleted` 事件必须记录 packet 路径和所属 workflow/run。
- packet 必须不可覆盖发布。

EventLog 已损坏时，review 仍可在内存中形成 blocked 报告，但不得声称 packet 已持久化；输出必须设置 `persistence_status: blocked` 和空 `packet_path`。

### 8.2 Blocker 生命周期

BlockerRecord 和 blocking ReviewFinding 使用 append-only supersession 表达状态变化：

```text
blocker-A (active)
  <- superseded by blocker-A-resolved
```

投影层从 `supersedes_event_id` 链接中选择尚未被后续事件覆盖的链尾。strict audit 再次发现同一 unresolved blocker 时复用原 EvidenceEvent ID，不重复写入等价历史；解除 blocker 时追加 resolution/superseding event，不修改或删除原事件。

## 9. Consolidated Review

Review synthesis 做：

- deduplicate findings。
- severity normalize。
- confidence normalize。
- assign owner。
- decide fix/skip/block/ask。
- produce consolidated-review.md。

禁止：

- 用平均分掩盖 blocker。
- 合并后丢 evidence。

## 10. Strict Audit

`/audit strict` 检查：

- fake implementation。
- stub。
- placeholder。
- hardcoded path。
- missing tests。
- schema drift。
- unverified release claim。
- policy gap。
- plugin trust gap。
- MCP exposure gap。
- hook fail-open。
- memory stale。
- TODO in production path。

每项 finding 需要：

- file/path。
- evidence。
- risk。
- suggested fix。
- priority。

## 11. Report Progress

`/report progress` 从 ledger 生成：

- 项目背景。
- 当前阶段。
- 已完成。
- 未完成。
- 阻塞。
- 风险。
- 下一步。

报告语气：

- 中文。
- 事实优先。
- 可直接汇报。
- 不夸大。

## 12. Evidence Retention

保留策略：

- eventlog 永久保留。
- command stdout 可截断。
- 大文件输出保存 tail 和摘要。
- 关键 proof 保存完整路径。
- secret 必须 redaction。
- marker finding 写入前必须 redaction 和长度约束。
- 单次 marker 审计设置明确上限；超过上限写 `marker_scan_truncated` blocker，不能静默丢弃。
- VerificationPacket、ReviewPacket 和完成事件永久保留且不可覆盖。
- `.tmp` 只表示未完成发布，不能参与 Done/Pass 投影。

## 13. “完成”的定义

Task Done：

- ResultPacket exists。
- VerificationPacket pass。
- ReviewGate no blocker。
- Evidence Ledger complete。

Workflow Done：

- 所有 required tasks done。
- blockers cleared or accepted。
- report generated。
- learnings written。

Commercial Ready：

- release proof。
- security proof。
- license/support proof。
- package lifecycle proof。
- policy/trust proof。
