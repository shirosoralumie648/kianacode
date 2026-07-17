# Volume 22: 错误、失败与 Blocker 分类

## 1. 目标

Kiana 必须把失败当成一等状态，而不是把失败隐藏在聊天文本里。错误分类清楚，恢复路径才清楚。

本卷定义：

- error taxonomy。
- failure packet。
- blocker 类型。
- retry 策略。
- escalation 策略。

## 2. Error 与 Blocker 区别

Error：

- 某个动作失败。
- 可能可恢复。
- 需要记录原因和输出。

Blocker：

- 当前流程不能继续。
- 需要用户、外部状态、设计调整或安全决策。

一个 error 可能变成 blocker，但不是所有 error 都是 blocker。

## 3. Error 分类

| Error | 说明 |
| --- | --- |
| validation_error | 数据不符合 schema |
| command_failed | 命令退出非 0 |
| command_timeout | 命令超时 |
| policy_denied | policy 拒绝 |
| approval_required | 需要用户批准 |
| missing_artifact | 必要文件不存在 |
| stale_context | 上下文过期 |
| dirty_state_conflict | 工作区状态冲突 |
| worker_failed | worker 失败 |
| verification_failed | 验证失败 |
| review_blocked | 审查阻断 |
| mcp_unavailable | MCP 不可用 |
| plugin_disabled | 插件禁用 |
| network_denied | 网络未授权 |
| eda_artifact_invalid | 硬件资料无效 |

## 4. FailurePacket

字段：

- failure_id。
- workflow_id。
- task_id optional。
- node。
- error_code。
- severity。
- recoverable。
- evidence。
- suggested_next。
- retry_count。

## 5. Severity

| Severity | 行为 |
| --- | --- |
| critical | stop/block |
| high | fix before continue |
| medium | fix or record risk |
| low | record follow-up |
| info | report only |

## 6. Recoverability

| 类型 | 行为 |
| --- | --- |
| retryable | 可按 retry policy 重试 |
| user_decision | 需要用户 |
| replan_required | 回到 Plan |
| context_refresh | 刷新 Context |
| policy_approval | Approval Gate |
| external_wait | Block |
| unrecoverable | Cancel/Block |

## 7. Blocker 分类

| Blocker | 说明 |
| --- | --- |
| missing_user_decision | 缺用户方向 |
| missing_approval | 缺高风险批准 |
| missing_artifact | 缺输入文件 |
| external_service | 外部服务不可用 |
| test_failure_unknown | 测试失败原因不明 |
| architecture_conflict | 架构冲突 |
| security_risk | 安全风险 |
| dirty_git | 工作区冲突 |
| scope_unclear | 范围不清 |
| impossible_goal | 目标不可实现 |

## 8. Retry Policy

Retry policy 字段：

- max_attempts。
- backoff。
- retryable_errors。
- stop_on_scope_change。
- stop_on_policy_denied。

默认：

- 同一失败最多重试 2 次。
- policy_denied 不重试。
- approval_required 不重试。
- scope violation 不重试。

## 9. Fix Loop 失败

Fix Loop 自身失败时：

1. 记录 fix attempt。
2. 比较错误是否变化。
3. 如果同一错误重复，停止盲目修。
4. 升级到 Research/Plan/Ask。

禁止：

- 无限改。
- 隐藏失败。
- 用 IFERROR 风格掩盖。

## 10. Command Failure

记录：

- command。
- cwd。
- exit code。
- stdout tail。
- stderr tail。
- duration。
- environment summary。

分类：

- expected red test。
- unexpected fail。
- missing command。
- timeout。
- permission denied。

## 11. Verification Failure

Verification fail 不等于 workflow fail。

路径：

- Fix Loop。
- Add Tests。
- Re-plan。
- Block。

必须保存：

- failed check。
- expected。
- actual。
- related files。

## 12. Review Blocker

Review blocker 处理：

- fix。
- user accepts risk。
- block。

Skip blocker 需要：

- explicit user acceptance。
- reason。
- evidence。

## 13. Dirty Git Blocker

分类：

- user_dirty。
- agent_dirty。
- mixed_dirty。
- unknown_dirty。

行为：

- user_dirty：保护用户修改。
- agent_dirty：恢复或回滚。
- mixed_dirty：隔离或问用户。
- unknown_dirty：停止写入。

## 14. EDA Blocker

类型：

- missing BOM。
- missing Gerber。
- schematic unreadable。
- high voltage risk。
- battery safety risk。
- order approval missing。
- inconsistent designator。

EDA blocker 默认不能自动忽略。

## 15. Block Report

Block Report 包含：

- blocked node。
- reason。
- evidence。
- user needed。
- options。
- recommended default。
- safe next action。

## 16. 验收

P0 验收：

- 每个失败都有 FailurePacket。
- Blocked 有 blocker reason。
- retry 次数有限。
- policy denied 不重试。
- dirty git 不覆盖用户修改。
- report 能展示 blocker。
