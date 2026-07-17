# Volume 32: WorkPacket Schema 深潜

## 1. 目标

WorkPacket 是 Kiana 并行执行和任务边界控制的核心对象。它把一个 Task Card 转换成 worker 可以执行、主 agent 可以审查、系统可以恢复的工作包。

WorkPacket 必须解决四个问题：

- 做什么。
- 不做什么。
- 能改哪里。
- 怎么证明完成。

如果 WorkPacket 边界不清，并行只会制造冲突。

## 2. WorkPacket 与 Task Card 的区别

Task Card 面向项目管理：

- 目标。
- 状态。
- 优先级。
- 依赖。
- owner。

WorkPacket 面向执行：

- 输入证据。
- allowed files。
- forbidden files。
- required commands。
- expected outputs。
- rollback。
- review focus。

一个 Task Card 可以拆成多个 WorkPacket。

## 3. 最小结构

```json
{
  "schema_version": "kiana.workpacket.v1",
  "workpacket_id": "wp_policy_profile_001",
  "task_id": "task_policy_profile",
  "workflow_id": "wf_20260709_policy",
  "title": "Define P0 policy profile contract",
  "objective": "Define the behavior and schema for personal_local policy profile",
  "in_scope": [
    "policy profile fields",
    "approval required actions",
    "default deny behavior"
  ],
  "not_building": [
    "enterprise SSO",
    "cloud policy sync",
    "automatic credential rotation"
  ],
  "allowed_paths": [
    "docs/kiana_project_os/08-trust-policy-plugin-mcp-hooks.md",
    "docs/kiana_project_os/33-approval-and-human-decision-protocol.md"
  ],
  "forbidden_paths": [
    "src/**",
    "Cargo.toml"
  ],
  "verification": [
    "rg -n \"approval_required|default deny\" docs/kiana_project_os"
  ],
  "review_focus": [
    "permission boundaries",
    "high-risk operation coverage"
  ],
  "risk_level": "high",
  "requires_approval": false
}
```

## 4. 字段分组

字段分为八组：

| 组 | 字段 |
| --- | --- |
| Identity | workpacket_id/task_id/workflow_id/title |
| Scope | objective/in_scope/not_building |
| Inputs | context_pack/findings/decisions/artifacts |
| File Boundary | allowed_paths/forbidden_paths/path_locks |
| Execution | mode/worker_type/steps/budget |
| Verification | commands/acceptance/evidence_required |
| Review | review_focus/risk_flags/escalation |
| Recovery | retry_policy/rollback/partial_result |

字段缺失时不能进入 dispatch。

## 5. Identity 规则

WorkPacket ID 必须稳定：

- 不依赖当前时间秒级随机值。
- 可从 task slug 派生。
- 重试时保留原 ID，attempt 单独递增。
- fork 时生成新 ID 并记录 parent_workpacket_id。

示例：

```json
{
  "workpacket_id": "wp_board_query_001",
  "attempt": 2,
  "parent_workpacket_id": null
}
```

## 6. Scope 规则

`objective` 必须是一句话。

`in_scope` 必须是可检查条目。

`not_building` 必须防止范围膨胀。

坏例子：

- “完善项目系统”。
- “优化体验”。
- “顺便整理代码”。

好例子：

- “定义 Project Board 的 next_task 查询输出契约”。
- “不修改 runtime 执行逻辑”。
- “不引入新数据库依赖”。

## 7. Inputs

输入对象：

- ContextPack。
- Findings。
- DecisionLog。
- Reference notes。
- Existing files。
- Test outputs。
- User constraints。

WorkPacket 必须记录输入来源，不能只写“参考上下文”。

```json
{
  "inputs": [
    {
      "type": "context_pack",
      "path": ".kiana/runs/wf_001/context_pack.md",
      "freshness": "fresh"
    },
    {
      "type": "decision",
      "id": "dec_parallel_bounded",
      "summary": "bounded swarm only"
    }
  ]
}
```

## 8. File Boundary

allowed_paths 不是建议，是硬边界。

规则：

- worker 只能读取更宽范围，但只能修改 allowed_paths。
- forbidden_paths 优先级高于 allowed_paths。
- high-risk paths 需要主 agent 或 approval。
- generated files 必须声明。
- lock files 默认禁止并行改。

路径匹配必须支持：

- exact file。
- directory prefix。
- glob。
- generated artifact label。

## 9. Path Lock

WorkPacket dispatch 前申请 lock：

```json
{
  "lock_id": "lock_docs_policy",
  "workpacket_id": "wp_policy_profile_001",
  "paths": [
    "docs/kiana_project_os/08-trust-policy-plugin-mcp-hooks.md"
  ],
  "mode": "write",
  "expires_at": "2026-07-09T19:00:00+08:00"
}
```

Lock 过期不能自动释放为可写。系统要先检查 worker 是否仍活跃。

## 10. Execution Mode

执行模式：

| mode | 用途 |
| --- | --- |
| inline | 主 agent 直接执行 |
| worker | 单个 worker 执行 |
| parallel | 多 worker 并行 |
| review_only | 只审查不改动 |
| design_only | 只写规格不写代码 |

当前用户要求属于 design_only。

## 11. Step 语义

WorkPacket 可以包含步骤，但步骤不是自然语言愿望。

每个 step 至少有：

- step_id。
- action。
- expected_output。
- evidence_type。
- failure_behavior。

示例：

```json
{
  "step_id": "s1",
  "action": "extend_project_board_spec",
  "expected_output": "board query contract documented",
  "evidence_type": "file_diff",
  "failure_behavior": "return partial result and mark review_needed"
}
```

## 12. Verification

Verification 分三层：

- structural：文件、schema、标题、引用存在。
- semantic：要求被覆盖，无矛盾。
- behavioral：命令/测试/审查通过。

设计文档阶段主要用 structural 和 semantic。

代码阶段必须补 behavioral。

## 13. Acceptance

Acceptance 必须可判定：

```json
{
  "acceptance": [
    {
      "criterion": "WorkPacket includes allowed_paths and forbidden_paths",
      "evidence": "schema section exists",
      "required": true
    },
    {
      "criterion": "parallel dispatch conflict rules are explicit",
      "evidence": "path lock section exists",
      "required": true
    }
  ]
}
```

不接受：

- “写得足够完整”。
- “感觉可实现”。
- “后续补充”。

## 14. Review Focus

review_focus 告诉审查者看什么：

- scope creep。
- missing evidence。
- unsafe permission。
- unstated dependency。
- schema mismatch。
- conflict risk。
- rollback missing。
- stale input。

Review 不应重新审所有东西，而是聚焦 WorkPacket 风险。

## 15. ResultPacket

WorkPacket 产出 ResultPacket：

```json
{
  "schema_version": "kiana.result_packet.v1",
  "workpacket_id": "wp_board_query_001",
  "status": "done",
  "changed_paths": [
    "docs/kiana_project_os/31-project-board-data-model-and-queries.md"
  ],
  "commands_run": [
    "wc -l docs/kiana_project_os/*.md"
  ],
  "evidence": [
    "ev_file_created",
    "ev_line_count"
  ],
  "scope_deviations": [],
  "notes": [
    "design only; no code changed"
  ]
}
```

## 16. Partial Result

如果 worker 没完成，不能丢失产出。

Partial Result 包含：

- done steps。
- failed step。
- changed files。
- command outputs。
- unresolved questions。
- rollback suggestion。

主 agent 根据 partial result 决定：

- retry。
- split。
- merge partial。
- discard。
- ask user。

## 17. Split 规则

WorkPacket 过大时拆分。

拆分信号：

- allowed_paths 超过 8 个。
- objective 包含多个动词。
- verification 命令不相关。
- review_focus 超过 6 项。
- worker 预计超过预算。
- 同时触及 schema、runtime、UI、docs。

拆分后必须保留 parent-child 关系。

## 18. Merge 规则

小 WorkPacket 可以合并，但只能在以下情况：

- 同一 task。
- 同一 allowed path。
- 同一 verification。
- 风险等级一致。
- 没有并行价值。

合并不能掩盖 blocked reason。

## 19. Retry Policy

Retry 字段：

```json
{
  "retry_policy": {
    "max_attempts": 2,
    "retry_on": ["transient_command_failure", "merge_conflict_resolved"],
    "do_not_retry_on": ["approval_rejected", "scope_invalid"]
  }
}
```

连续失败后必须升级，不允许无限循环。

## 20. Rollback

Rollback 不一定是执行 git reset。

设计阶段 rollback：

- 删除新增文档。
- 回退某个 section。
- 标记 superseded。

代码阶段 rollback：

- revert patch。
- disable feature flag。
- restore config。
- regenerate artifact。

Rollback 计划必须写明会影响哪些文件。

## 21. Worker Handoff

Worker 收到的内容必须足够独立：

- 目标。
- 上下文。
- allowed/forbidden。
- expected evidence。
- failure behavior。
- reporting format。

不要让 worker 去猜整个项目战略。

## 22. 主 Agent 集成

主 agent 负责：

- dispatch。
- path lock。
- result review。
- conflict resolution。
- final verification。
- user reporting。

worker 不负责最终 ship。

## 23. WorkPacket 反例

反例：

```json
{
  "title": "完善 Kiana",
  "allowed_paths": ["**/*"],
  "verification": [],
  "not_building": []
}
```

问题：

- 目标不可验收。
- 允许修改范围无限。
- 没有验证。
- 没有边界。
- 不能安全并行。

## 24. 验收

WorkPacket 设计完成标准：

- schema 字段覆盖 identity/scope/input/file/execution/verification/review/recovery。
- 明确 Task Card 与 WorkPacket 区别。
- 明确 split/merge/retry/rollback。
- 明确 ResultPacket 和 Partial Result。
- 明确 worker 与主 agent 职责边界。
- 能支撑 bounded swarm。
