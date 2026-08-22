# Phase 03 特性账本 — 契约与 Schema 基线

> **PARKED 2026-08-22.** This ledger is the old v1.0 Phase 3 (schema/contract baseline). Current Phase 3 is the runnable golden path in `.planning/ROADMAP.md`. Do not execute this file as `$gsd-discuss-phase 3`.


**Created:** 2026-07-26 | **父需求：** CORE-01, CORE-04 | **主旅程：** core.event-replay, core.registry-discovery, core.walking-skeleton
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。`[M0]` 标记 walking-skeleton 最薄子集。

### FEAT-03-01 — [M0] RuntimeEvent 全变体与单入口重放
- 父需求: CORE-01
- 领域旅程: core.event-replay
- 描述: 一次真实请求的 turn/stream/tool/permission/error/usage/result 全部落为版本化 typed event，可从 EventLog 逐字节重放出一致投影。
- 验收:
  - [local_behavior] 全事件变体（含 usage、terminal result + stop_reason）经 v1 schema 校验；CLI 单入口重放一致
- Verifier: schema-contract-smoke + 重放 fixture 测试
- 当前基线: implemented — `kiana-runtime-event.v1` pinned，多适配器与 TUI/export 全 fixture 已覆盖；usage 事件与重放一致性断言待收口
- 设计引用: DESIGN-INDEX Phase 3 行（sdk-runtime-events、schemas/、control-plane 架构）
- 依赖: None
- 状态: pending

### FEAT-03-02 — 事件 schema 版本化与兼容 adapter
- 父需求: CORE-01
- 领域旅程: core.event-replay
- 描述: 旧客户端/旧数据经兼容 adapter 迁移；无法兼容的输入被明确拒绝并给出诊断，不静默丢字段。
- 验收:
  - [local_behavior] v1→未来版本的迁移合同成文；不兼容 fixture 得到 typed 拒绝诊断
  - [local_behavior] legacy session（JSON 与 JSONL）读取路径回归通过
- Verifier: 兼容矩阵测试
- 当前基线: partial — legacy session 兼容已实现；版本升级 adapter 合同未定义
- 设计引用: 同上；project_os 11（数据契约）
- 依赖: FEAT-03-01
- 状态: pending

### FEAT-03-03 — 跨入口重放等价矩阵
- 父需求: CORE-01
- 领域旅程: core.event-replay
- 描述: CLI、SDK/RPC、MCP、remote/bridge、App Server 对同一事件序列的序列化与重放语义一致。
- 验收:
  - [local_behavior] 五入口消费同一 fixture 序列，输出投影逐字段等价；差异即失败
- Verifier: 跨适配器等价测试
- 当前基线: partial — 各适配器独立覆盖已有（runner/remote/bridge/app events-view）；无统一等价矩阵
- 设计引用: control-plane 架构设计
- 依赖: FEAT-03-01
- 状态: pending

### FEAT-03-04 — [M0] 统一 registry 只读发现
- 父需求: CORE-04
- 领域旅程: core.registry-discovery
- 描述: 用户从任一入口发现 command/tool/MCP workbench/connector 时，看到同一 registry 的 schema、版本、权限与生命周期。
- 验收:
  - [local_behavior] 单一 registry 源支撑 CLI `--help`/TUI/App `/app/commands`/doctor tool_parity 的一致清单
- Verifier: 发现面一致性测试
- 当前基线: partial — ToolRegistry/tool_parity/app commands 已有；connector 域与统一暴露缺失
- 设计引用: project_os 24（命令目录）；control-plane 架构
- 依赖: None
- 状态: pending

### FEAT-03-05 — Registry 生命周期与结构化错误合同
- 父需求: CORE-04
- 领域旅程: core.registry-discovery
- 描述: registry 条目携带版本、权限、生命周期（注册/启用/禁用/废弃）与结构化错误语义。
- 验收:
  - [local_behavior] 生命周期状态变更事件化；错误按 pinned 错误税则输出
- Verifier: 生命周期测试 + 错误 schema 校验
- 当前基线: partial — plugin enable/disable 与 tool 错误元数据已有；统一生命周期/错误税则未定义
- 设计引用: project_os 22（错误税则）/11
- 依赖: FEAT-03-04
- 状态: pending

### FEAT-03-06 — 读并发/写串行的 registry 决策
- 父需求: CORE-04
- 领域旅程: core.registry-discovery
- 描述: 安全读操作可并行，写操作被串行化或隔离，结果在事件流中可观察。
- 验收:
  - [local_behavior] 并发调用 fixture：读并行、写串行/隔离，事件流含决策记录
- Verifier: 并发语义测试
- 当前基线: partial — 工具 read-only/concurrency-safe 标记已有；串行化决策事件缺失
- 设计引用: project_os 21（调度）
- 依赖: FEAT-03-04
- 状态: pending

### FEAT-03-07 — 不兼容输入显式拒绝
- 父需求: CORE-01
- 领域旅程: core.event-replay
- 描述: 无法兼容的事件/会话输入被明确拒绝并产出可操作诊断。
- 验收:
  - [local_behavior] 损坏/未知 schema/越界 fixture 全部 typed 拒绝，无静默丢弃路径
- Verifier: 负面 fixture 套件
- 当前基线: partial — stream-json 输入错误合同与 eval 严格解析已示范；事件/会话域负面套件缺失
- 设计引用: sdk-runtime-events
- 依赖: FEAT-03-02
- 状态: pending