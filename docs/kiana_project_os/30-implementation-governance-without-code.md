# Volume 30: 非代码实施治理

## 1. 目标

本规格库现在仍是软件逻辑和功能设计，不直接写代码。为了后续进入实现阶段，需要定义如何从规格转计划，而不丢边界。

## 2. 从规格到计划

转换路径：

```text
Spec Volume
  -> Implementation Slice
  -> Task Card
  -> WorkPacket
  -> Code/Test
  -> Evidence
```

不能从参考仓库直接跳到代码。

## 3. Plan 输入

实现计划必须读取：

- main spec。
- relevant volume。
- reference audit。
- existing schemas。
- current code。
- tests。

## 4. Task 生成规则

每个实现 task 必须：

- 对应一个 volume section。
- 有 exact files。
- 有 schema。
- 有 tests。
- 有 failure path。
- 有 acceptance。

## 5. 不写代码阶段的完成定义

设计阶段完成：

- 功能逻辑清楚。
- 数据契约清楚。
- 命令语义清楚。
- 失败路径清楚。
- 验收清楚。
- 参考归因清楚。

不要求：

- 代码实现。
- 测试通过。
- release proof。

## 6. 防止规格漂移

漂移信号：

- 新需求没有进入 volume。
- 实现计划引用不存在命令。
- 文档说 P0，但 roadmap 说 P2。
- reference audit 与主规格矛盾。

处理：

- 更新 affected volume。
- 更新 main spec if product-level。
- 更新 reference audit if source-level。

## 7. Change Request

规格变更需要：

- change reason。
- affected volumes。
- affected milestones。
- risk。
- migration note。

## 8. Traceability

每个 P0 task 应能追溯：

- main spec section。
- volume section。
- reference source。
- schema。
- test。
- evidence。

## 9. Review Before Implementation

实施前审查：

- scope 是否过大。
- P0/P1/P2 是否正确。
- 是否缺 policy。
- 是否缺 failure path。
- 是否缺 verification。
- 是否依赖未实现模块。

## 10. Implementation Plan 格式

计划应包含：

- Goal。
- Architecture。
- Tech stack。
- Files。
- Tasks。
- Tests。
- Commands。
- Acceptance。
- Rollback。

但这些属于下一阶段，不写入本规格库主体。

## 11. Spec Debt

Spec debt 类型：

- vague command。
- missing schema。
- missing failure path。
- missing policy。
- missing report behavior。
- missing EDA approval。

Spec debt 应进入 backlog。

## 12. Governance Reports

定期输出：

- spec coverage。
- P0 readiness。
- reference coverage。
- stale facts。
- open decisions。

## 13. 人类决策点

必须让用户决定：

- P0 范围变更。
- 高风险自动化。
- 云端同步。
- plugin trust。
- EDA 自动下单。

## 14. 验收

本阶段验收：

- 规格库超过上万行。
- 主规格不臃肿。
- 分册覆盖核心软件逻辑。
- 后续可转 implementation plan。
- 没有过期 GitNexus/36 仓库事实。
