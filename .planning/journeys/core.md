# Core 黄金旅程账本

**Created:** 2026-07-26 | **覆盖需求：** CORE-01..16 与横切 DIF-01..09 | **消费方：** `.planning/features/` 的 `领域旅程` 字段、MILESTONES 退出门禁、phase verifier。
**规则：** 旅程是具名验收目标：其 required proof 证据到位前，引用它的 FEAT/需求不得标 complete。内部域账本无外部源冻结机制；接近验收需绑定不可变 evidence 时按 governance 四 family 纪律升格为 JSON revision。

### core.walking-skeleton — 最薄纵向链路（M0）
- 覆盖需求: CORE-01/02/04/05/06/07 的最薄子集（M0 里程碑载体）
- 必备能力点: 真实请求 → typed RuntimeEvent → session 持久化 → PolicyDecision → 单工具执行 → typed result；全程 EventLog 可重放；CLI 单入口演示
- Required proof: local_behavior
- 归属阶段: 3-6（横切，各 phase 的 `[M0]` FEAT 子集）
- 当前 gap: 各环节孤立实现大多存在（runner/session/trust/policy/tool），未作为单一贯通旅程验收

### core.event-replay — 事件权威与重放
- 覆盖需求: CORE-01
- 必备能力点: turn/stream/tool/permission/error/usage/result 全部版本化 typed event；每入口可序列化并重放同一序列；schema 兼容策略
- Required proof: local_behavior
- 归属阶段: 3
- 当前 gap: `kiana-runtime-event.v1` 已 pinned 且多适配器覆盖；跨入口重放一致性与兼容 adapter 未验收

### core.registry-discovery — 统一 registry 与并发语义
- 覆盖需求: CORE-04
- 必备能力点: command/tool/MCP workbench/connector 唯一 registry；schema/版本/权限/生命周期/结构化错误；读并发写串行可观察
- Required proof: local_behavior
- 归属阶段: 3
- 当前 gap: ToolRegistry 与 doctor tool_parity 已有；registry 统一暴露与并发决策事件化未验收

### core.session-lifecycle — Session 全生命周期
- 覆盖需求: CORE-02
- 必备能力点: create/list/resume/fork/compact/import/export/delete；父子 turn/stop reason/tool lifecycle/附件/pack 选择重启一致；旧 session 迁移
- Required proof: local_behavior
- 归属阶段: 4
- 当前 gap: SDK session store/JSONL tree/compact/import 已实现；重启一致性矩阵与迁移回滚未验收

### core.memory-provenance — 记忆与来源优先
- 覆盖需求: CORE-08
- 必备能力点: 分类型持久化（Observation/Reasoning/Decision/Git/Research evidence）；source/confidence/scope/retention/stale；搜索/编辑/导出/删除；live evidence 优先
- Required proof: local_behavior
- 归属阶段: 4
- 当前 gap: `kiana.memory-record.v1` JSONL/status/search 已有；类型分级、stale 边界与 live 优先未实现

### core.policy-decision — 统一策略决策与硬边界
- 覆盖需求: CORE-05/06 + DIF-09
- 必备能力点: 七层配置确定性优先级；凭据仅 Keychain/vault；三档自主权按项目覆盖；一致 PolicyDecision，deny 优先；硬策略不可绕过
- Required proof: local_behavior（凭据存储 target_environment）
- 归属阶段: 5
- 当前 gap: 信任根/managed policy/config resolved 已 fail-closed；档位选择、Keychain 集成与全入口一致决策未实现

### core.context-pack — 可追溯上下文构建
- 覆盖需求: CORE-07
- 必备能力点: 预算内 ContextPack；repo map/symbol/impact/可编辑集/artifact graph 带 source/time/hash/truncation；冻结快照不静默变更
- Required proof: local_behavior
- 归属阶段: 6
- 当前 gap: context index/artifacts/graph/pack CLI 与 app 端点已有；预算、truncation reason 与快照冻结语义未验收

### core.provider-negotiation — Provider 能力协商
- 覆盖需求: CORE-03/14 + DIF-07
- 必备能力点: 六类 provider 注册/认证/选择/健康检查；能力显式路由/降级/拒绝；usage/cost/延迟/retry/健康可查询；遥测默认本地
- Required proof: target_environment（live provider）
- 归属阶段: 7
- 当前 gap: Anthropic/OpenAI-compatible/Ollama/fake 适配与 capability profile 已有；Gemini/OpenRouter、协商事件与成本账本缺失

### core.workflow-evidence — 证据驱动完成
- 覆盖需求: CORE-09/10 + DIF-01
- 必备能力点: Intent Router 输出 pack/风险/权限/副作用/验收理由；持久 WorkflowRun DAG；Evidence Ledger 五态 verifier；模型自述不能 complete
- Required proof: local_behavior
- 归属阶段: 8
- 当前 gap: WorkflowRun/EventLog/VerificationPacket/五态已实现且有商业门禁；Intent Router 语义与验收定义前置未实现

### core.recovery — 可恢复与 result_unknown
- 覆盖需求: CORE-11 + DIF-02/03
- 必备能力点: crash/interrupt/超时/断流/worker 失败可恢复到最后可信节点；EventLog 重建 state；幂等键；未知外部结果先查询/人工核对
- Required proof: local_behavior（故障注入 target_environment 于 M5/M6）
- 归属阶段: 8
- 当前 gap: crash-window 修复/recovery journal/HMAC 链已实现；provider 断流恢复与 result_unknown 外部核对流程未实现

### core.multi-agent-packet — 受限多 Agent
- 覆盖需求: CORE-12 + DIF-04
- 必备能力点: typed WorkPacket（目标/输入/路径/工具/预算/依赖/验收/返回 schema）；路径锁或 worktree 隔离；late result 不自动集成；整体 gate 重跑
- Required proof: local_behavior
- 归属阶段: 9
- 当前 gap: bounded swarm 生命周期/集成/回滚/重试已实现（Linux）；packet 验收 schema 与跨平台身份后端缺失

### core.extension-lifecycle — 扩展生命周期与信任
- 覆盖需求: CORE-13
- 必备能力点: skills/plugins/hooks/MCP/connectors/packs 的来源/license/版本/完整性/权限 manifest/receipt/回滚/冲突诊断；未信任默认不加载
- Required proof: local_behavior
- 归属阶段: 9
- 当前 gap: plugin receipts/managed policy/trust gate/组件 preflight 已实现；connector 与 domain-pack 合同未统一

### core.local-first-data — 本地优先数据主权
- 覆盖需求: CORE-15 + DIF-05
- 必备能力点: 无账户运行三 pack；数据默认在设备；备份/迁移/导出/彻底删除；sync/telemetry 明示 opt-in
- Required proof: target_environment
- 归属阶段: 19
- 当前 gap: 本地运行与 TELEMETRY.md 承诺已有；备份/迁移/删除的用户旅程未实现

### core.release-lifecycle — 发布生命周期与透明就绪
- 覆盖需求: CORE-16 + DIF-12
- 必备能力点: 安装/升级/回滚/卸载全平台；签名/checksum/SBOM/license 扫描/release notes/doctor/支持包；blockers 透明可导出
- Required proof: target_environment
- 归属阶段: 2（DIF-12 机制）/ 24（CORE-16 全量证明）
- 当前 gap: 本地机制与合同基本完备（commercial-release-readiness）；外部证据（真实 remote/签名/渠道/验收）全部未闭合
