# Roadmap 专项文档

`docs/roadmap.md` 是路线图的总入口；[§1.1 全量 Step 总图](../roadmap.md#all-step-index) 汇总 749 张可逐条领取的卡，并按依赖拓扑给出唯一执行顺序。下面只用于定位专项设计说明和代码归属，不另设执行队列。所有状态仍以 `CURRENT_STATUS.md` 的证据块为准。

| 专项 | 局部索引 | 位置 |
|---|---:|---|
| ControlPlane | CP-00–CP-30（31） | [CP-01 身份基线](control-plane-identity-baseline.md) · [CP-02 执行身份基线](control-plane-execution-identity-baseline.md) · [CP-03 action 基线](control-plane-action-baseline.md) · [CP-04 ScopeSet 基线](control-plane-scope-baseline.md) · [CP-05 入口一致性](control-plane-entry-path-baseline.md) · [CP-06 原子转移基线](control-plane-transition-baseline.md) · [授权、状态转移与恢复设计](control-plane.md) |
| Harness | H01–H36（36） | [H02 身份基线](harness-identity-baseline.md) · [H03 状态驱动器](harness-state-driver-baseline.md) · [H04 模型内容基线](harness-model-content-baseline.md) · [Agent 运行时设计与实施步骤](harness.md) |
| Provider | P4-J7-04–P4-J7-31（28） | [Provider 协议与 streaming 设计](provider.md) |
| CompanyOS | CO-01–CO-48（48） | [组织、业务流程与交付闭环](companyos.md) |
| Capability | CAP-00–CAP-34（35） | [CAP-01 authority 基线](capability-authority-baseline.md) · [CAP-02 input/digest 基线](capability-input-baseline.md) · [CAP-03 ExecutionScope](capability-execution-scope-baseline.md) · [CAP-04 状态/outcome 基线](capability-state-baseline.md) · [能力目录、Broker 与执行边界](capability.md) |
| Event / Receipt / Recovery | ER-00–ER-36（37） | [ER-01 schema/kind 基线](event-receipt-schema-baseline.md) · [ER-02 关联/顺序基线](event-receipt-identity-baseline.md) · [ER-03 脱敏/Artifact 基线](event-receipt-redaction-baseline.md) · [ER-04 CommandReceipt 基线](event-receipt-command-receipt-baseline.md) · [事实账本、收据与恢复](event-receipt-recovery.md) |
| Context / Memory | CM-00–CM-39（40） | [上下文、检索、记忆与治理](context-memory.md) |
| Skills / Plugins / Hooks | EXT-00–EXT-31（32） | [扩展来源、信任与生命周期](skills-plugins-hooks.md) |
| UI / Entrypoints | UI-00–UI-41（42） | [CLI、Workbench、Web、Desktop 与入口一致性](ui-entrypoints.md) |
| 配置 / 凭据 / 身份 | CI-01–CI-12（12） | [roadmap §29](../roadmap.md#config-credentials-identity-plan) |
| Swarm | SW-00–SW-18（19） | [roadmap §30](../roadmap.md#swarm-coordination-design) |
| 可观测性 / 审计 | OA-00–OA-28（29） | [roadmap §31](../roadmap.md#observability-audit-plan) |
| 调度 / Workflow / Trigger | AUT-01–AUT-24（24） | [roadmap §32](../roadmap.md#scheduling-workflow-trigger-plan) |
| 持久化 / 数据层 | PD-00–PD-35（36） | [PD-00 基线](persistence-data-layer-baseline.md) · [独立设计](persistence-data-layer.md) · [roadmap §33](../roadmap.md#persistence-data-layer-plan) |
| 通知 / 消息 | NM-00–NM-22（23） | [NM-00 基线](notifications-baseline.md) · [roadmap §34](../roadmap.md#notification-messaging-design) |
| 集成 / 连接器 | INT-00–INT-33（34） | [INT-00 基线](integrations-baseline.md) · [独立设计](integrations-connectors.md) · [roadmap §34-A](../roadmap.md#integrations-connectors-plan) |
| 评测 / 质量 | EQ-00–EQ-51（52） | [EQ-00 基线](evaluation-baseline.md) · [roadmap §34-B](../roadmap.md#quality-evaluation-design) |
| 计费 / 配额 / 成本 | BQ-00–BQ-30（31） | [roadmap §35](../roadmap.md#billing-quota-cost-plan) |
| 部署 / 运维 / 迁移 | DEP-00–DEP-41（42） | [roadmap §36](../roadmap.md#deployment-operations-migration-design) |
| 安全 / 合规 | SC-00–SC-43（44） | [SC-00 基线](security-compliance-baseline.md) · [独立设计](security-compliance.md) · [roadmap §37](../roadmap.md#security-compliance-plan) |

返回：[路线图总入口](../roadmap.md#appendix-navigation) · [文档总入口](../README.md) · [当前状态账本](../../CURRENT_STATUS.md)
