# 第二阶段 Batch 3 - Core dispatch/approvals + Runner 模块注释工程进度

**启动时间**：Batch 2 完成后  
**预算**：18-24 工时  
**覆盖范围**：kiana-runner 干净文件 + kiana-core dispatch/approvals（需先清理）

## Batch 3 执行计划

### Phase 1：kiana-runner 干净文件（进行中）

8 个干净源文件，共 2,215 行，3 个 Agent 并行处理：

| # | 文件 | 行数 | 状态 | 职责 |
|---|------|------|------|------|
| 1 | lib.rs | 163 | 🔄 Agent | 模块入口、trait 定义 |
| 2 | protocol_runner.rs | 86 | 🔄 Agent | 协议定义、版本控制 |
| 3 | state_driver.rs | 520 | 🔄 Agent | 状态机驱动、执行流程 |
| 4 | inbox.rs | 340 | 🔄 Agent | 消息接收、优先级队列 |
| 5 | budget.rs | 354 | 🔄 Agent | Token 预算、实时监控 |
| 6 | retry.rs | 145 | 🔄 Agent | 重试策略、指数退避 |
| 7 | model.rs | 212 | 🔄 Agent | Model 交互、流式响应 |
| 8 | stream_normalizer.rs | 295 | 🔄 Agent | 流规范化、协议转换 |
| 9 | progress.rs | 10 | 🔄 Agent | 进度跟踪 |
| **小计** | | **2,215** | | |

### Phase 2：kiana-core dispatch/approvals（待后续）

这些文件在工作区有改动，需先处理：
- dispatch.rs（1,166 行）
- approvals.rs（1,159 行）
- dispatch_delivery_tests.rs（994 行）
- automation_dispatch.rs（12 行）

**建议处理策略**：
- 若改动为临时工作，可 `git stash` 清理
- 若改动为重要未提交工作，建议 `git commit` 后再处理注释

**预计工作量**：~3,331 行源码，15-18 工时

---

## 覆盖的核心概念

### Runner 执行引擎
- Agent 完整生命周期（创建→运行→完成）
- 状态机驱动与转移约束
- Token 预算与实时监控
- 重试和故障恢复
- Model 交互与流处理

### Core dispatch/approvals
- 多聚合分发（dispatch）
- 审批生命周期（approvals）
- 效果递交与确认
- 与 Runner 的交互

---

**Status**: Batch 3 Phase 1 进行中（3 个 Agent 并行处理 kiana-runner）
**Last Updated**: 当前会话（Agent 后台处理中）
