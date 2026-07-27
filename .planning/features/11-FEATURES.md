# Phase 11 特性账本 — Coding 生态、自动化与远程闭环

**Created:** 2026-07-26 | **父需求：** COD-10..COD-15 | **主旅程：** coding.ecosystem-automation, coding.multi-agent-teams, coding.headless-ci, coding.background-remote, coding.voice-input
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。parity: cc.chrome.*×3、cc.git.*×2、cc.ci.*×1（target_phase 11）。M2 前半的核心账本。

### FEAT-11-01 — MCP 完整互操作与调试旅程
- 父需求: COD-10
- 领域旅程: coding.ecosystem-automation
- 描述: 通过完整 MCP transports 和 tools/resources/templates/prompts 使用、验证和调试 MCP、skills、plugins、hooks、commands；项目资源仍受 trust gate。
- 验收:
  - [local_behavior] 四传输 × 四资源类的使用与调试旅程；错误诊断可操作
- Verifier: MCP 旅程测试
- 当前基线: partial — 四传输/prompts 生命周期/scoped config 已实现；调试旅程与 resources/templates 完整面缺失
- 设计引用: DESIGN-INDEX Phase 11 行（project_os 14）
- 依赖: FEAT-09-05
- 状态: pending

### FEAT-11-02 — Browser/Chrome workbench（parity chrome 域）
- 父需求: COD-12
- 领域旅程: coding.ecosystem-automation（parity: cc.chrome.browser-integration / remote-session-files / permission-degradation）
- 描述: Browser/Chrome 集成按平台探测、权限启用、暂停和审计；远程 session 文件延续；不可用时明确降级。
- 验收:
  - [target_environment] 真实浏览器环境三条 parity 能力取证；降级路径显式
- Verifier: chrome workbench 测试 + parity evidence
- 当前基线: partial — kiana-chrome-mcp crate 与远程 session 基础存在；平台探测/审计/降级未验收
- 设计引用: governance parity chrome 域
- 依赖: FEAT-11-01
- 状态: pending

### FEAT-11-03 — Computer-use 与桌面 workbench
- 父需求: COD-12
- 领域旅程: coding.ecosystem-automation
- 描述: computer-use、screen capture、clipboard、URL handler、Notebook、LSP 等 workbench 按平台探测、权限启用、暂停和审计；不可用明确降级。
- 验收:
  - [local_behavior] 六类 workbench 的探测/权限/降级矩阵（Linux 优先，平台边界显式）
- Verifier: workbench 矩阵测试
- 当前基线: partial — computer-mcp/computer-input/screen-capture/url-handler crate 存在（test_init 有平台边界）；权限/审计/降级面缺失
- 设计引用: functional-design §15/F11
- 依赖: FEAT-05-05
- 状态: pending

### FEAT-11-04 — 用户级 subagent/team 启动与 handoff
- 父需求: COD-11
- 领域旅程: coding.multi-agent-teams
- 描述: subagent、agent teams 和并行任务由用户显式启动或 router 授权启动；每个 worker 有隔离 scope、消息、预算、状态、取消、handoff 和集成 gate。
- 验收:
  - [local_behavior] 用户启动/观察/取消/handoff 全旅程；router 授权启动接入 Intent Router
- Verifier: 多 Agent 用户旅程测试
- 当前基线: partial — swarm/team 机制完整（FEAT-09 系列）；用户旅程与 router 启动缺失
- 设计引用: functional-design §11/F07
- 依赖: FEAT-09-04, FEAT-08-01
- 状态: pending

### FEAT-11-05 — 本地 Git 远端回执（parity git 域）
- 父需求: COD-13
- 领域旅程: coding.headless-ci（parity: cc.git.status-diff-commit / worktree-pr-review）
- 描述: PR 创建/检查/review 与远端回执保留预览、审批和回执记录（COD-07 远端面并入）。
- 验收:
  - [target_environment] 真实 forge（GitHub）PR 旅程带回执；两条 parity git 能力取证
- Verifier: PR 回执测试
- 当前基线: partial — 本地 git 面已有（FEAT-10-06）；PR/远端回执缺失
- 设计引用: governance parity git 域
- 依赖: FEAT-10-06
- 状态: pending

### FEAT-11-06 — Headless/CI 执行（parity ci 域）
- 父需求: COD-13
- 领域旅程: coding.headless-ci（parity: cc.ci.github-gitlab-headless）
- 描述: 非交互 print/exec、stream-json、SDK/RPC/MCP 在 CI 和第三方应用中创建/恢复 session、订阅 events、响应 approval、取消任务并取得 typed result；GitHub Actions 有受限凭据和 PR 回执。
- 验收:
  - [target_environment] 真实 GitHub Actions 全旅程（创建→approval→result→回执）；受限凭据验证
- Verifier: CI 端到端测试
- 当前基线: partial — stream-json/SDK/print 本地面完整；真实 CI 旅程缺失
- 设计引用: governance parity ci 域；project_os 24
- 依赖: FEAT-11-05
- 状态: pending

### FEAT-11-07 — 后台/远程任务转移与幂等重连
- 父需求: COD-14
- 领域旅程: coding.background-remote
- 描述: 本地任务转为 background 或 remote worker，离线后继续；CLI/IDE/Desktop/Web 观察同一进度、diff、日志、approval；重连不重复副作用。
- 验收:
  - [target_environment] 转移→断连→重连旅程；幂等副作用验证；多入口观察一致
- Verifier: 远程延续测试
- 当前基线: partial — remote session/bridge/OAuth 刷新/重试已实现；转移语义与幂等重连缺失
- 设计引用: functional-design §11/F07
- 依赖: FEAT-08-06
- 状态: pending

### FEAT-11-08 — Voice 输入本地行为面
- 父需求: COD-15
- 领域旅程: coding.voice-input
- 描述: voice 输入可开始、暂停、编辑确认并提交 prompt；音频权限、转写来源和保留策略可见；不可用回退文本；不绕过审批。
- 验收:
  - [local_behavior] 完整 dictation 生命周期与文本回退；审批保持（跨入口 user_value 在 Phase 19 parity 收口）
- Verifier: voice 生命周期测试
- 当前基线: none — parity voice 域 missing
- 设计引用: governance parity voice 域（target_phase 19，本地面前置于此）
- 依赖: FEAT-10-08
- 状态: pending
