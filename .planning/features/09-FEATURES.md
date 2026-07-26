# Phase 09 特性账本 — 受限多 Agent、扩展与 Pack 契约

**Created:** 2026-07-26 | **父需求：** CORE-12, CORE-13, DIF-04 | **主旅程：** core.multi-agent-packet, core.extension-lifecycle
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。

### FEAT-09-01 — Typed WorkPacket 与验收 schema
- 父需求: CORE-12
- 领域旅程: core.multi-agent-packet（parity: cc.agents.subagent-lifecycle）
- 描述: worker 只接收 typed WorkPacket：目标、输入、allowed/forbidden paths、tools、预算、依赖、验收和返回 schema。
- 验收:
  - [local_behavior] packet schema 含验收与返回 schema 字段；越界路径/工具/预算全部 fail closed
- Verifier: packet 合同测试
- 当前基线: partial — swarm WorkPacket/scope/预算/结果包已实现；验收与返回 schema 字段缺失
- 设计引用: DESIGN-INDEX Phase 9 行（project_os 07/32）
- 依赖: FEAT-08-02
- 状态: pending

### FEAT-09-02 — 隔离执行与冲突阻塞
- 父需求: CORE-12
- 领域旅程: core.multi-agent-packet（parity: cc.agents.background-worktrees）
- 描述: 并发写入使用路径锁或 worktree 隔离；越界或跨 worker 冲突被阻塞；worker 状态/取消/失败可查看。
- 验收:
  - [local_behavior] 路径冲突/scope 偏离 fixture 阻塞；worker 健康/预算/身份可查询（现有 v2 合同回归）
- Verifier: swarm 冲突测试
- 当前基线: implemented — Linux worktree/快照隔离、进程身份、健康报告、冲突检测已实现；跨平台身份后端缺失
- 设计引用: project_os 07/27
- 依赖: FEAT-09-01
- 状态: pending

### FEAT-09-03 — Late result 拒绝与整体 gate 重跑（DIF-04）
- 父需求: DIF-04
- 领域旅程: core.multi-agent-packet
- 描述: 失联或过期 worker 的 late result 不自动集成；父 workflow 只在重新验证 packet 和整体 gate 后完成。
- 验收:
  - [local_behavior] late/lost worker fixture：结果隔离不集成；集成后整体 gate 重跑记录在案
- Verifier: 集成 gate 测试
- 当前基线: partial — integrate plan/apply 已验证 baseline/hash/verification；late-result 拒绝语义未显式化
- 设计引用: project_os 27
- 依赖: FEAT-09-02
- 状态: pending

### FEAT-09-04 — 用户级多 Agent 启动与观察（parity teams）
- 父需求: CORE-12
- 领域旅程: core.multi-agent-packet（parity: cc.agents.teams-messaging）
- 描述: 用户可查看每个 worker 的 packet、路径、tools、预算、依赖、lease、状态、取消、结果和失败。
- 验收:
  - [local_behavior] team/任务板/mailbox 的用户操作面完整；worker 全字段可见
- Verifier: team 操作测试
- 当前基线: partial — team plan/status 合同与 swarm monitor 已实现；用户级 mailbox/handoff 面缺失
- 设计引用: project_os 04/31
- 依赖: FEAT-09-02
- 状态: pending

### FEAT-09-05 — 扩展来源与完整性合同（parity extensions）
- 父需求: CORE-13
- 领域旅程: core.extension-lifecycle（parity: cc.extensions.plugin-lifecycle / skills-commands / hook-lifecycle）
- 描述: skills/plugins/hooks/MCP/connectors/packs 显示来源、license、版本、完整性、权限 manifest 和 receipt，支持 enable/disable/update/rollback 与冲突诊断。
- 验收:
  - [local_behavior] 六类扩展统一生命周期面；update/rollback 与冲突诊断补齐；receipt 完整性回归
- Verifier: 扩展生命周期测试
- 当前基线: implemented(plugins/skills/hooks/MCP) — receipts/managed policy/preflight/审计已实现；connectors 与 packs 未纳入、update/rollback 缺失
- 设计引用: project_os 13/14
- 依赖: None
- 状态: pending

### FEAT-09-06 — Trust gate 全域回归（parity trust-policy）
- 父需求: CORE-13
- 领域旅程: core.extension-lifecycle（parity: cc.extensions.trust-policy）
- 描述: 未信任项目资源默认不加载；信任建立展示将启用能力与风险；记录在项目外用户存储。
- 验收:
  - [local_behavior] parity 该项 required local_behavior proof 取得（现有 fail-closed 体系验收化）
- Verifier: trust 边界套件（已有测试聚合）
- 当前基线: implemented — 外部信任根/三态/资源过滤/运行时传播已完整；proof 未记录到 parity evidence
- 设计引用: governance parity
- 依赖: None
- 状态: pending

### FEAT-09-07 — Domain-pack 注册合同
- 父需求: CORE-13
- 领域旅程: core.extension-lifecycle
- 描述: 能力包通过稳定 domain pack contract 注册领域 tools/workflow 模板/对象 schema/评测集/呈现扩展；不得建立第二套 session/policy/event/completion 模型（AF-04）。
- 验收:
  - [local_behavior] pack 注册接口 + 一个示例 pack（EDA 现有能力迁移为首个注册者）；反向测试证明 pack 无法绕过核心模型
- Verifier: pack 合同测试
- 当前基线: none — pack 概念未合同化；EDA 以内建命令存在
- 设计引用: 产品总纲 §3.2；project_os 34
- 依赖: FEAT-09-05
- 状态: pending
