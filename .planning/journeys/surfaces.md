# Product Surfaces 黄金旅程账本

**Created:** 2026-07-26 | **覆盖需求：** SURF-01..10 + NFR-03/05/06 | **消费方：** features/MILESTONES/verifier。
**纪律：** 所有入口只扩展 shared Core contract，禁止第二套 session/permission/event/completion 模型（AF-04）。

### surfaces.cli-tui — CLI/REPL/TUI 产品闭环
- 覆盖需求: SURF-01
- 必备能力点: 三 pack 的 prompt/commands/history-resume/diff/tool cards/approval/background-workflow monitor/settings-doctor/附件；文本与 JSON/stream 输出不混淆
- Required proof: local_behavior | 归属阶段: 16
- 当前 gap: TUI 审批/diff/resume/history/settings 与 REPL 分类器已实现；三 pack 视图与 workflow monitor 未闭合；parity phase-16 的 cli/commands 域 15 项待 proof

### surfaces.headless-sdk — Headless SDK/RPC
- 覆盖需求: SURF-02
- 必备能力点: 版本化 API/typed events/cancel/backpressure/reconnect/approval callback/idempotency/auth/language-neutral examples；breaking change 迁移路径
- Required proof: local_behavior | 归属阶段: 16
- 当前 gap: SDK session store/stream-json/runtime-event schema 已 pinned；backpressure/idempotency/迁移策略未定义

### surfaces.mcp-interop — MCP server/client 互操作
- 覆盖需求: SURF-03
- 必备能力点: stdio/HTTP/SSE/WS 上 tools/resources/templates/prompts/auth/capability discovery/errors 完整互操作；policy 与本地工具一致
- Required proof: local_behavior | 归属阶段: 16
- 当前 gap: 四传输与 prompts 生命周期本地面已实现；server 侧完整互操作矩阵未验收

### surfaces.readiness-diagnostics — 就绪与诊断
- 覆盖需求: SURF-10
- 必备能力点: capability/readiness、provider/connector/plugin health、policy、data location、version/update、release blockers、脱敏支持诊断可查看导出
- Required proof: local_behavior | 归属阶段: 16
- 当前 gap: doctor/reference_capabilities/release blockers/app 端点已广泛实现；跨入口一致呈现未验收

### surfaces.ide — IDE 客户端
- 覆盖需求: SURF-04
- 必备能力点: VS Code 与 JetBrains 的项目上下文/chat/inline-patch diff/diagnostics/review/task-workflow state/approval/resume；无独立 session/permission model
- Required proof: user_value | 归属阶段: 17
- 当前 gap: 未交付（parity ide 域 missing）；app-server 合同是底座

### surfaces.desktop — Desktop 工作区
- 覆盖需求: SURF-05
- 必备能力点: conversation/workspace/files-sources/artifacts/connector center/models-settings/computer-use/approval inbox/history-search/三 pack views；无账户本地可用
- Required proof: user_value | 归属阶段: 18
- 当前 gap: 未交付（parity desktop 域 missing）；app-server 端点覆盖广

### surfaces.web-appserver — Web/App Server
- 覆盖需求: SURF-06
- 必备能力点: conversations/workflows/events/files/artifacts/team/admin/release operations；实时 reconnect/RBAC/pagination/bounded history；local 与 hosted 同一 contract
- Required proof: local_behavior | 归属阶段: 18
- 当前 gap: `/app` 合同大量实现（conversations/events/release/context 族）；workflows/team/admin 操作面与 RBAC 缺失

### surfaces.state-consistency — 状态一致与打磨质量门
- 覆盖需求: SURF-08 + NFR-06
- 必备能力点: loading/empty/error/offline/degraded/permission-denied/result-unknown 全入口一致；错误信息含原因/影响/下一步；帮助从错误现场可达
- Required proof: local_behavior | 归属阶段: 18
- 当前 gap: 状态语义分散在各端点；一致性契约与错误信息标准未定义

### surfaces.i18n-a11y — 国际化与可访问性
- 覆盖需求: NFR-03 + NFR-05
- 必备能力点: 字符串外化；中英双语；CJK/长文本/窄终端布局；键盘完整导航/screen reader/reduced motion/对比度检查清单
- Required proof: local_behavior | 归属阶段: 18
- 当前 gap: 未实现（当前混合中英硬编码文案）

### surfaces.cross-surface-continuity — 跨入口连续性
- 覆盖需求: SURF-07
- 必备能力点: CLI 创建→IDE 看 diff→Desktop 审批→Web 观察；相同 IDs/events/state/evidence；离线冲突明确解决
- Required proof: target_environment | 归属阶段: 19
- 当前 gap: 事件与 session 合同已统一；四端旅程与冲突解决未实现

### surfaces.platform-matrix — 平台交付矩阵
- 覆盖需求: SURF-09
- 必备能力点: Linux/macOS 原生与 Windows/WSL 的 path/terminal/sandbox/Keychain/browser-native-host 差异端到端验证；不支持能力明示
- Required proof: target_environment | 归属阶段: 19
- 当前 gap: 平台边界诊断（process identity/platform-security proof）已 fail-closed；目标环境矩阵证据缺失
