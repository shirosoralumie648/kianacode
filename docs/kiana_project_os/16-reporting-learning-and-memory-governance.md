# Volume 16: Reporting、Learning 与 Memory Governance

## 1. 目标

Kiana 的报告和学习系统要让项目长期推进，而不是每次重新解释。报告必须基于证据，学习必须可治理。

## 2. 报告类型

| 类型 | 用途 |
| --- | --- |
| progress | 当前进展 |
| blocker | 当前阻塞 |
| handoff | 交接 |
| teacher | 给老师/导师汇报 |
| manager | 项目管理汇报 |
| audit | 审计结果 |
| release | 发布准备 |
| eda | 硬件审查 |

## 3. Progress Report

结构：

- 背景。
- 当前目标。
- 已完成。
- 正在做。
- 阻塞。
- 风险。
- 下一步。
- 需要用户决策。

要求：

- 每个完成项有 evidence。
- 未验证项标未知。
- memory-derived 事实标注可能过期。

## 4. Handoff Report

用于：

- 新 session。
- worker handoff。
- 人类接手。

内容：

- objective。
- current state。
- decisions。
- files touched。
- commands run。
- blockers。
- next task。
- caution。

## 5. Teacher Report

面向不熟代码细节的人。

要求：

- 中文。
- 背景清楚。
- 少代码细节。
- 讲清进度和问题。
- 能直接口述。

## 6. Audit Report

结构：

- scope。
- methodology。
- findings by severity。
- evidence。
- blockers。
- recommended P0 fixes。
- unknowns。

## 7. Learning Loop

Learn 阶段输入：

- eventlog。
- ResultPacket。
- VerificationPacket。
- ReviewPacket。
- user decisions。

输出：

- learnings.md。
- memory proposals。
- follow-up tasks。
- stale memory updates。

## 8. Memory Proposal

Memory 不应自动写入最终库。

Proposal 字段：

- proposed_text。
- type。
- source evidence。
- confidence。
- related files。
- suggested retention。
- risk。

决策：

- accept。
- reject。
- revise。
- defer。

## 9. Memory Governance

规则：

- 低置信不注入。
- stale 降权。
- 用户偏好高优先。
- release/commercial 状态必须 live verify。
- 安全相关 memory 必须 evidence。

## 10. Stale 管控

Stale 原因：

- file changed。
- command changed。
- schema changed。
- time elapsed。
- user contradicted。
- tests contradicted。

处理：

- mark stale。
- request refresh。
- do not use for claim。
- keep as historical context。

## 11. Report Source Labels

报告中事实标签：

- `live`。
- `verified`。
- `memory-derived`。
- `historical`。
- `unknown`。
- `blocked`。

## 12. Follow-up Tasks

Learn 阶段可以创建 follow-up，但必须：

- 标明来源。
- 标明优先级。
- 标明是否 blocking。
- 不自动塞入 Done。

## 13. Metrics

Workflow metrics：

- time to first action。
- tasks completed。
- blockers count。
- verification pass rate。
- retry count。
- worker conflict count。
- stale memory hits。

这些用于改进，不作为虚假 KPI。

## 14. 验收

P0 验收：

- `/report progress` 基于 evidence。
- handoff 可让新 session 继续。
- learnings.md 写入。
- memory proposal 有 source/confidence。
- stale memory 不用于完成声明。
