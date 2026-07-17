# Kiana Completion Agent Program

本目录是 `docs/superpowers/specs/2026-07-13-kiana-hundred-agent-completion-program-design.md` 的版本化执行目录。

它保存 170 张 Task Card、18 个领域定义、30 个 ownership package、依赖门禁和验收配置。Task Card 是规划对象，不是可直接派发的 WorkPacket。W0 Baseline Intake 完成后，Program Compiler 才能把 `ready` Task Card 与精确 `base_commit`、contract hashes、lease、fencing epoch 和预算绑定为不可变 WorkPacket。

## Layout

```text
program.json
task-card.schema.json
domains.json
ownership.json
references.json
tasks/
  E.json
  D01.json ... D18.json
dags/
  gates.json
acceptance/
  verification-gates.json
```

## Status Rules

- 初始状态只有 `E01` 为 `ready`。
- 其他 Task Card 均为 `blocked`，并给出具体依赖原因。
- 上游 Agent 自报完成不会解除阻塞；依赖提交必须进入新的 canonical baseline。
- `blocked_reason` 只描述当前门禁，不替代 `depends_on`。
- Task Card revision 可以更新规划字段；已派发 WorkPacket 不可原地修改。

## Execution Rule

- 全项目不采用 TDD。Builder 先实现批准的 Task Card/WorkPacket 合同，再补充并运行 focused、adversarial、integration 和 review 验证。
- `verification.expected_initial` 只描述实现或验证尚未完成的初始状态，不要求 pre-implementation failure run。
- `acceptance/verification-gates.json` 只接受实现后 focused evidence；缺失、失败、跳过或与 Task Card 无关的验证均 fail closed。

## Identity Rules

- 全局前置任务使用 `E01` 至 `E24`。
- 领域任务使用 `D01-01` 至 `D18-10`。
- ownership package 使用 `P01` 至 `P30`。
- Task Card ID、domain 配额和总数由机器校验，不按文件行数推断。

## Validation

```bash
jq empty docs/agent-program/kiana-completion/*.json \
  docs/agent-program/kiana-completion/tasks/*.json \
  docs/agent-program/kiana-completion/dags/*.json \
  docs/agent-program/kiana-completion/acceptance/*.json

jq -s '[.[].tasks[]] | length' \
  docs/agent-program/kiana-completion/tasks/*.json
```

期望任务总数为 `170`。领域包总数为 `146`，全局前置包总数为 `24`。

## Execution Boundary

当前目录只定义计划。Builder 不得从这里直接修改 canonical branch。执行必须经过独立 worktree、immutable WorkPacket、review、fresh verification 和 integration queue。
