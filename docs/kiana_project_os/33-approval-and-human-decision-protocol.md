# Volume 33: Approval 与人工决策协议

## 1. 目标

Kiana 需要同时支持自动推进和人工控制。Approval 与 Decision Protocol 的目标不是让用户频繁点确认，而是在方向改变、高风险、不可逆、成本敏感或证据不足时，把人类决策变成可追踪状态。

它要解决：

- 什么时候必须问用户。
- 问什么才是有效问题。
- 用户回答如何进入状态机。
- 拒绝后如何继续。
- 旧决策何时失效。

## 2. Approval 与 Decision 的区别

Decision 是方向选择。

例如：

- P0 先做 Project OS 还是 EDA。
- 并行策略保守还是激进。
- 插件生态先支持本地还是远程 marketplace。

Approval 是风险授权。

例如：

- 是否 push。
- 是否 merge。
- 是否 deploy。
- 是否运行网络命令。
- 是否安装插件。
- 是否自动下单 PCB。

Decision 可以改变计划。Approval 只授权某个操作。

## 3. Decision 对象

```json
{
  "schema_version": "kiana.decision.v1",
  "decision_id": "dec_parallel_strategy",
  "workflow_id": "wf_20260709_design",
  "question": "Kiana P0 的并行策略采用哪一种？",
  "options": [
    {
      "id": "conservative",
      "label": "保守",
      "effect": "默认串行，只对文档和审计并行"
    },
    {
      "id": "bounded",
      "label": "有界并行",
      "effect": "WorkPacket + path lock + 主 agent 集成"
    }
  ],
  "selected": "bounded",
  "decided_by": "user",
  "decided_at": "2026-07-09T14:00:00+08:00",
  "applies_to": ["swarm", "scheduler", "workpacket"],
  "expires_when": ["policy_profile_changes", "major_architecture_change"]
}
```

## 4. Approval 对象

```json
{
  "schema_version": "kiana.approval.v1",
  "approval_id": "ap_push_release_branch",
  "operation": "git_push",
  "risk_level": "high",
  "requested_by": "workflow:wf_release",
  "reason": "需要推送 release candidate 分支",
  "scope": {
    "branch": "release/v0.1",
    "remote": "origin"
  },
  "status": "requested",
  "expires_at": "2026-07-09T23:59:00+08:00"
}
```

Approval 必须绑定具体 operation 和 scope，不能是泛化授权。

## 5. 必问场景

必须问用户：

- 目标不清且无法安全默认。
- 多个路线会改变架构。
- 操作不可逆或难回滚。
- 可能产生费用。
- 访问外部网络或第三方账号。
- 修改安全/权限策略。
- 安装或启用插件。
- 执行 push/merge/deploy。
- EDA 下单、BOM 替代、生产文件导出。
- 用户明确要求先确认。

## 6. 不该问的场景

不应打断用户：

- 低风险文档补全。
- 已有明确默认策略。
- 用户已经给出“继续/ok/按这个来”。
- 查询当前状态。
- 运行只读检查。
- 修复明显拼写/过期事实。
- 在当前规格边界内扩展细节。

过度提问会破坏 Project OS 的推进感。

## 7. 问题质量

好问题：

- 一次只问一个决策。
- 给出默认推荐。
- 说明每个选项的后果。
- 可被记录为 Decision。

坏问题：

- “你想怎么做？”
- “要不要完善？”
- “A/B/C/D 都可以，你选。”
- 一次问多个互相独立的问题。

## 8. Decision Gate

Decision Gate 输入：

- unresolved decisions。
- risk level。
- default availability。
- user preference。
- time sensitivity。

输出：

- ask_user。
- use_default。
- block。
- proceed_with_caution。
- replan。

use_default 必须记录原因。

## 9. Approval Gate

Approval Gate 输入：

- operation。
- policy profile。
- operation risk。
- current scope。
- prior approvals。
- expiration。

输出：

- approved。
- requested。
- rejected。
- expired。
- not_required。

Approval Gate 不负责执行操作，只负责授权判断。

## 10. 用户回答解析

短回答映射：

| 用户输入 | 语义 |
| --- | --- |
| ok | 接受当前推荐 |
| 继续 | 按当前计划推进 |
| a/b/c | 选择对应选项 |
| 都要 | 选择组合方案，但需要拆分范围 |
| 先别 | 暂停当前操作 |
| 不要 | reject |

如果回答含糊但低风险，可以采用推荐默认。高风险必须复问。

## 11. Decision Log

Decision Log 是 append-only。

事件：

- decision_requested。
- decision_defaulted。
- decision_selected。
- decision_superseded。
- decision_expired。

不能直接覆盖旧决策。新决策 supersede 旧决策。

## 12. 失效规则

Decision 失效条件：

- 用户改变目标。
- P0/P1/P2 范围改变。
- 关键参考事实变化。
- 项目结构大改。
- policy profile 改变。
- 时间超过有效期。

失效后不能继续引用为当前依据。

## 13. Approval 有效期

Approval 必须有有效期。

默认：

- shell/network：当前 workflow。
- push/merge/deploy：一次 operation。
- plugin install：一次 install。
- EDA irreversible action：一次 artifact/version。

禁止永久授权高风险操作。

## 14. 灰区处理

灰区是影响方向但不一定高风险的选择。

例子：

- 文档要“主规格厚”还是“分册库厚”。
- 并行 worker 更激进还是更稳。
- `/eda` 先做审查还是自动生成。

灰区处理：

- 给出 2-3 方案。
- 推荐默认。
- 记录选择。
- 如果用户多次选同类偏好，形成 preference memory。

## 15. 拒绝后行为

Approval rejected 后：

- 不执行操作。
- 记录 rejection。
- 查找替代路径。
- 如果无替代，block。
- 如果可降级，继续低风险方案。

例如：

- 不允许 push -> 生成 PR draft 文案。
- 不允许网络 -> 使用本地资料。
- 不允许自动下单 -> 生成人工检查清单。

## 16. Fork 决策

用户可能要求两个方向都做。

处理：

- 创建两个 subgoal。
- 共享上层 thesis。
- 分离 Workstream。
- 分离 acceptance。
- 如果资源冲突，由 Scheduler 排序。

例如：

- 高并发并行干活。
- 电路电子设计方向。

二者共享 Project OS，但落入不同 workstream。

## 17. 决策与 Memory

Memory 可以记住偏好，但不能自动授权。

可记：

- 用户喜欢中文文档。
- 用户偏好“软件逻辑和功能设计，不直接写代码”。
- 用户偏好 Project OS/PMP 方向。

不可记为永久授权：

- deploy。
- push。
- 安装插件。
- 网络访问。
- 花费。
- EDA 下单。

## 18. 决策可解释性

每次自动默认要解释：

```json
{
  "decision_id": "dec_doc_volume_strategy",
  "selected": "multi_volume_spec_library",
  "decision_mode": "defaulted",
  "reason": "user requested large design volume and Chinese docs; multi-volume keeps main spec readable"
}
```

## 19. 人类汇报

在 `/report` 中，Decision 部分应显示：

- 已做关键决策。
- 当前未决决策。
- 需要用户批准的高风险操作。
- 默认决策及原因。
- 已拒绝操作及替代方案。

不要把所有 event dump 给用户。

## 20. 验收

Approval/Decision Protocol 完成标准：

- 区分 Decision 与 Approval。
- 定义必问/不问场景。
- 定义对象 schema。
- 定义失效和有效期。
- 定义拒绝后的替代行为。
- 支持用户短回答。
- 支持两个方向都做时的 fork。
