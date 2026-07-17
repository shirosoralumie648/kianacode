# Volume 24: 命令目录与输出契约

## 1. 目标

命令是用户触发 Kiana 的主要方式。每个命令必须有稳定语义、输入、输出、错误和 evidence 行为。

## 2. 命令契约字段

每个命令定义：

- name。
- mode。
- purpose。
- input。
- options。
- output human。
- output json。
- artifacts written。
- evidence behavior。
- policy checks。
- errors。

## 3. `/project plan`

输入：

- goal。
- optional source path。
- optional constraints。

输出：

- workflow id。
- goal summary。
- created task count。
- first next task。
- artifact path。

错误：

- unclear goal。
- cannot write artifacts。
- unsafe request。

## 4. `/project status`

输出：

- workflow id。
- status。
- current node。
- dirty state。
- blockers。
- last evidence。

JSON 必须包含：

- `workflow_id`。
- `status`。
- `current_node`。
- `last_event_id`。

## 5. `/project board`

输出：

- columns。
- task cards。
- blockers。
- evidence status。

错误：

- missing workflow。
- corrupt state。

## 6. `/project next`

输出：

- selected task。
- reasons。
- skipped tasks。
- next command。

JSON：

- selected。
- reasons。
- skipped。
- requires_approval。

## 7. `/context packet`

输出：

- context path。
- included sources。
- stale warnings。
- missing facts。

Evidence：

- context_pack_built event。

## 8. `/swarm dispatch`

输入：

- max workers。
- optional task ids。

输出：

- dispatched。
- skipped。
- path locks。
- worker ids。

错误：

- no ready tasks。
- path conflict。
- policy denied。

## 9. `/audit strict`

输出：

- findings。
- severity counts。
- blockers。
- report path。

JSON：

- findings array。
- evidence ids。
- final_status。

## 10. `/report progress`

输出：

- human report。
- source labels。
- evidence references。

错误：

- no workflow。
- no evidence。
- stale only。

## 11. `/eda review`

输入：

- artifact paths。
- constraints。

输出：

- eda review path。
- risk summary。
- missing artifacts。
- approval needs。

## 12. `/policy status`

输出：

- active policy。
- denied capabilities。
- ask capabilities。
- approvals。
- risk flags。

## 13. `/plugin status`

输出：

- installed plugins。
- enabled/disabled。
- capabilities。
- receipt status。

## 14. `/mcp status`

输出：

- servers。
- startup status。
- tools visible。
- required/optional。
- failures。

## 15. Human Output Style

要求：

- 中文优先。
- 简洁。
- 结果先行。
- evidence 明确。
- blocker 明确。

避免：

- 内部对象堆满屏。
- 没有下一步。
- 模糊成功。

## 16. JSON Output Style

要求：

- schema_version。
- command。
- status。
- data。
- errors。
- evidence。

失败也输出 JSON。

## 17. Exit Codes

建议：

- 0 success。
- 1 command failure。
- 2 validation failure。
- 3 policy denied。
- 4 approval required。
- 5 blocked。
- 6 internal error。

## 18. 验收

P0 验收：

- 每个核心命令有 JSON 输出。
- failure 输出可解析。
- human 输出能读。
- evidence path 存在。
- policy denial 不伪装成功。
