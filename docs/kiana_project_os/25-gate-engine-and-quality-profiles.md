# Volume 25: Gate 引擎与 Quality Profile

## 1. 目标

Gate Engine 是 Kiana 的质量控制层。它负责把“能不能继续”变成明确的、可记录的决策。

## 2. GateResult

字段：

- gate_name。
- status。
- reasons。
- evidence。
- next_node。
- required_actions。

status：

- pass。
- caution。
- fail。
- blocked。
- needs_user。

## 3. Gate 类型

- Capture Gate。
- Context Gate。
- Research Gate。
- Design Gate。
- Plan Gate。
- Plan Confirmation Gate。
- WorkPacket Gate。
- Routing Gate。
- Execution Gate。
- Quality Gate。
- Verify Gate。
- Review Gate。
- Ship Gate。
- Learn Gate。

## 4. Quality Profile

Profile 决定检查组合。

| Profile | 检查 |
| --- | --- |
| quick | syntax/basic |
| task | format/lint/test |
| project | format/lint/type/build/test |
| audit_strict | plus security/release/policy |
| release | full release proof |
| eda | artifact/BOM/DFM/bring-up |

## 5. Capture Gate

Pass：

- goal clear。
- success criteria exists。
- constraints known。
- NOT_BUILDING captured。

Fail：

- goal vague。
- unsafe。
- impossible。

## 6. Context Gate

Pass：

- live state known。
- memory freshness known。
- stack/scripts known。
- dirty state classified。

Fail：

- missing files。
- context stale。
- dirty conflict。

## 7. Plan Gate

Pass：

- tasks executable。
- dependencies clear。
- verification defined。
- rollback defined。

Fail：

- vague tasks。
- no tests。
- no scope。

## 8. WorkPacket Gate

Pass：

- allowed files present。
- forbidden files present。
- commands present。
- review focus present。

Fail：

- missing bounds。
- risky without approval。
- dependency invalid。

## 9. Quality Gate

Checks：

- format。
- lint。
- type。
- build。
- security。
- dependency。

Skipped 必须有 reason。

## 10. Verify Gate

Pass：

- targeted tests pass。
- acceptance met。
- NOT_BUILDING not violated。

Fail：

- tests fail。
- acceptance missing。
- cannot verify。

## 11. Review Gate

Pass：

- no blockers。
- high findings fixed or accepted。

Fail：

- blocker。
- security issue。
- insufficient tests。

## 12. Ship Gate

Report only：

- no approval needed。

Approval required：

- push。
- merge。
- deploy。
- release。

## 13. Learn Gate

Pass：

- eventlog updated。
- progress updated。
- learnings written。

Fail：

- cannot write artifacts。
- memory proposal invalid。

## 14. Gate Composition

一个节点可以有多个 gate。

规则：

- fail stops。
- blocked stops。
- caution continues with evidence。
- needs_user asks。

## 15. 验收

P0 验收：

- gate result 可序列化。
- failed gate 有 reasons。
- skipped check 有 reason。
- gate 决策写 eventlog。
- report 能显示 gate 状态。
