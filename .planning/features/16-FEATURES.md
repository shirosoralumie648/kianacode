# Phase 16 特性账本 — Terminal、Headless 与 MCP 产品闭环

**Created:** 2026-07-26 | **父需求：** SURF-01, SURF-02, SURF-03, SURF-10, NFR-01, NFR-02 | **主旅程：** surfaces.cli-tui, surfaces.headless-sdk, surfaces.mcp-interop, surfaces.readiness-diagnostics
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。parity Phase 16 计 15 项：cc.cli.*×5、cc.commands.*×4、cc.mcp.*×3、cc.sdk.*×3。

### FEAT-16-01 — CLI/TUI 三 pack 视图与 workflow monitor
- 父需求: SURF-01
- 领域旅程: surfaces.cli-tui
- 描述: CLI/REPL/TUI 支持三 pack 的 prompt/commands/history-resume/diff/tool cards/approval/background-workflow monitor/settings-doctor/附件；文本与 JSON/stream 输出互不混淆。
- 验收:
  - [local_behavior] 三 pack slash commands 与工具卡在 TUI 可达；workflow monitor 显示 DAG 进度；文本/JSON/stream 不混输出 fixture
- Verifier: CLI-TUI 旅程测试
- 当前基线: partial — TUI 审批/diff/settings/history 与 REPL 分类器已实现；三 pack 视图与 workflow monitor 缺失
- 设计引用: DESIGN-INDEX Phase 16 行（project_os 12/24）
- 依赖: FEAT-10-08, FEAT-12-01, FEAT-14-01
- 状态: pending

### FEAT-16-02 — CLI flags parity（cc.cli 域 5 项）
- 父需求: SURF-01
- 领域旅程: surfaces.cli-tui（parity: cc.cli.print-stream-json/permission-sandbox-flags/model-context-flags/structured-output-schema；blocked: cc.cli.forward-subagent-text）
- 描述: 四条 parity CLI flag 能力取得 local_behavior proof；`forward-subagent-text` 版本冲突 blocker 解除并验证或记录显式差异决策。
- 验收:
  - [local_behavior] 四条 flag 旅程测试通过；`forward-subagent-text` blocker 解除或差异决策记录
- Verifier: parity evidence 更新
- 当前基线: partial — print-stream-json/structured-output-schema 为 implemented；permission-sandbox-flags/model-context-flags 为 partial；forward-subagent-text 为 blocked
- 设计引用: governance parity cc.cli 域
- 依赖: FEAT-16-01
- 状态: pending

### FEAT-16-03 — Interactive commands parity（cc.commands 域 4 项）
- 父需求: SURF-01
- 领域旅程: surfaces.cli-tui（parity: cc.commands.session-lifecycle/mode-permission/status-diagnostics/background-remote）
- 描述: 四条 commands parity 能力取得 local_behavior proof。
- 验收:
  - [local_behavior] 四条 commands 旅程测试；session-lifecycle/status-diagnostics（implemented 基线）回归；mode-permission/background-remote（partial）补齐
- Verifier: parity evidence 更新
- 当前基线: partial — session-lifecycle/status-diagnostics 为 implemented；mode-permission/background-remote 为 partial
- 设计引用: governance parity cc.commands 域
- 依赖: FEAT-16-01
- 状态: pending

### FEAT-16-04 — Headless SDK/RPC 版本化 API
- 父需求: SURF-02
- 领域旅程: surfaces.headless-sdk（parity: cc.sdk.session-events/structured-output-tools/mcp-headless-approval）
- 描述: 版本化 API、typed events、cancel/backpressure/reconnect、approval callback、idempotency、auth 和 language-neutral examples；breaking change 迁移路径；三条 SDK parity 取证。
- 验收:
  - [local_behavior] SDK 合同文档（版本、breaking-change 策略）；backpressure/idempotency fixture；三 parity SDK 旅程测试
- Verifier: SDK 合同测试 + parity evidence
- 当前基线: partial — SDK JSONL/runtime-event schema/stream-json 合同 pinned；backpressure/迁移策略未定义
- 设计引用: sdk-runtime-events；governance parity cc.sdk 域
- 依赖: FEAT-03-03
- 状态: pending

### FEAT-16-05 — MCP 全传输互操作 parity（cc.mcp 域 3 项）
- 父需求: SURF-03
- 领域旅程: surfaces.mcp-interop（parity: cc.mcp.transport-config/tools-resources-prompts/auth-trust）
- 描述: stdio/HTTP/SSE/WS 上四资源类完整互操作；policy 与本地工具一致；三条 MCP parity 取证。
- 验收:
  - [local_behavior] 四传输 × 四资源类互操作矩阵（transport-config/tools-resources-prompts 为 implemented，回归）；auth-trust（partial）补齐
- Verifier: MCP 互操作矩阵 + parity evidence
- 当前基线: partial — 四传输与 prompts 已实现（transport-config/tools-resources-prompts implemented）；auth-trust partial
- 设计引用: governance parity cc.mcp 域
- 依赖: FEAT-11-01
- 状态: pending

### FEAT-16-06 — 就绪诊断与健康统一视图
- 父需求: SURF-10
- 领域旅程: surfaces.readiness-diagnostics
- 描述: 每个入口可查看 capability/readiness、provider/connector/plugin health、policy、data location、version/update、release blockers 和脱敏诊断；输出脱敏且可导出。
- 验收:
  - [local_behavior] 六类就绪维度跨 CLI/TUI/App 一致；导出脱敏诊断包
- Verifier: 就绪一致性测试
- 当前基线: partial — doctor/reference_capabilities/blockers 已广泛实现；跨入口一致性矩阵缺失
- 设计引用: project_os 15/36
- 依赖: FEAT-07-02
- 状态: pending

### FEAT-16-07 — 性能预算与冷启动门禁（NFR-01）
- 父需求: NFR-01
- 领域旅程: surfaces.cli-tui
- 描述: 冷启动 < 25 s 回归门禁纳入 release smoke；大仓 repo-map/索引与长会话内存有可测门槛。
- 验收:
  - [local_behavior] fresh-process 冷启动在 CI release smoke 门禁中（双样本 < 25 s）；大仓 fixture 索引耗时有阈值
- Verifier: release smoke + 性能回归
- 当前基线: partial — Phase 1 已证双样本 < 25 s；未纳入 release smoke 持续门禁；大仓无测
- 设计引用: FEAT-01-03（01-14 gap closure 依据）
- 依赖: None
- 状态: pending

### FEAT-16-08 — 可观测性基线（NFR-02）
- 父需求: NFR-02
- 领域旅程: surfaces.readiness-diagnostics
- 描述: 结构化日志/trace/doctor 诊断在 CLI/TUI/Headless/Desktop/Web 一致可用且默认仅本地；脱敏诊断包可导出。
- 验收:
  - [local_behavior] 五入口诊断格式与脱敏一致性矩阵；doctor --json 在所有发布 binary 可用
- Verifier: 诊断一致性测试
- 当前基线: partial — doctor/config-resolved/release-blockers 本地面完整；Desktop/Web 侧与统一格式缺失
- 设计引用: project_os 15
- 依赖: FEAT-16-06
- 状态: pending
