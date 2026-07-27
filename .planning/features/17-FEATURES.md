# Phase 17 特性账本 — IDE 客户端

**Created:** 2026-07-26 | **父需求：** SURF-04 | **主旅程：** surfaces.ide
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。不得建立独立 session/permission 模型（AF-04）。parity: cc.ide.*×3（全部 missing 基线）。

### FEAT-17-01 — VS Code 扩展（项目上下文/chat/diff/diagnostics）
- 父需求: SURF-04
- 领域旅程: surfaces.ide（parity: cc.ide.diff-context-diagnostics）
- 描述: VS Code 扩展提供项目上下文、chat、inline/patch diff、diagnostics、review，使用 app-server 合同，不保存独立事实。
- 验收:
  - [user_value] 真实 VS Code 用户旅程：打开仓库→chat→inline diff→diagnostics→review；无独立 session 存储
- Verifier: VS Code 端到端旅程
- 当前基线: none — app-server `/app` 合同大量实现是底座
- 设计引用: DESIGN-INDEX Phase 17 行（governance parity cc.ide 域；SURF-04）
- 依赖: FEAT-16-04, FEAT-16-05
- 状态: pending

### FEAT-17-02 — VS Code 任务/workflow state/approval/resume
- 父需求: SURF-04
- 领域旅程: surfaces.ide（parity: cc.ide.session-resume）
- 描述: VS Code 中 task/workflow 状态可见、approval 可响应、session 可 resume，状态与其他入口一致。
- 验收:
  - [user_value] workflow 状态/approval/resume 旅程；切换入口后状态等价
- Verifier: 跨入口状态一致性测试
- 当前基线: none
- 设计引用: governance parity cc.ide.session-resume
- 依赖: FEAT-17-01
- 状态: pending

### FEAT-17-03 — JetBrains 扩展
- 父需求: SURF-04
- 领域旅程: surfaces.ide（parity: cc.ide.vscode-jetbrains）
- 描述: JetBrains 插件提供与 VS Code 等价的核心用户旅程，不建立独立持久化或权限模型。
- 验收:
  - [user_value] JetBrains 中相同 Coding 旅程通过；版本不兼容显式显示
- Verifier: JetBrains 端到端旅程
- 当前基线: none
- 设计引用: governance parity cc.ide.vscode-jetbrains
- 依赖: FEAT-17-02
- 状态: pending

### FEAT-17-04 — IDE 不兼容/未信任状态 fail-safe
- 父需求: SURF-04
- 领域旅程: surfaces.ide
- 描述: workspace 未信任、Core/client 版本不兼容或能力不支持时，IDE 明确显示原因和恢复路径并 fail-safe。
- 验收:
  - [local_behavior] 三类 fail-safe 场景各有 fixture；无静默降质路径
- Verifier: fail-safe 负面测试
- 当前基线: none — trust gate 与 capability profile 底座已有
- 设计引用: 产品总纲 §6
- 依赖: FEAT-05-06, FEAT-17-01
- 状态: pending
