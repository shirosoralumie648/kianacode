# Coding 黄金旅程账本

**Created:** 2026-07-26 | **覆盖需求：** COD-01..16 + NFR-04 | **消费方：** features/MILESTONES/verifier。
**权威声明：** Claude Code 公开对齐（parity）的唯一权威是 `docs/agent-program/kiana-completion/governance/`（15 journeys / 49 capabilities，含 `target_phase`、`required_proof_level`、`coverage_state`）。本文件不复制其内容，只提供**阶段反查索引**和 **Kiana 超集旅程**（parity 之外的差异化承诺）。COD-01 已完成（账本已冻结）。

## A. CC parity 账本的阶段反查索引

| Phase | parity capabilities（`cc.*`） | 备注 |
|-------|-------------------------------|------|
| 05 | auth.oauth-api-cloud | 凭据/认证归 policy phase |
| 07 | models.selection-fallback / provider-routing / effort-context | provider 协商 |
| 09 | agents.subagent-lifecycle / teams-messaging / background-worktrees；extensions.plugin-lifecycle / skills-commands / hook-lifecycle / trust-policy | 多 Agent 与扩展 |
| 10 | memory.project-user-rules / auto-memory / import-scope；checkpoint.turn-snapshot / diff-undo / user-change-protection | Coding 公开基线 |
| 11 | chrome.browser-integration / remote-session-files / permission-degradation；git.status-diff-commit / worktree-pr-review；ci.github-gitlab-headless | 生态与远端回执 |
| 16 | cli.print-stream-json / permission-sandbox-flags / model-context-flags / structured-output-schema / forward-subagent-text(blocked)；commands.session-lifecycle / mode-permission / status-diagnostics / background-remote；mcp.transport-config / tools-resources-prompts / auth-trust；sdk.session-events / structured-output-tools / mcp-headless-approval | Terminal/Headless/MCP 闭环 |
| 17 | ide.vscode-jetbrains / diff-context-diagnostics / session-resume | IDE（现 missing） |
| 18 | desktop.workspace-conversations；web.remote-sessions | Desktop/Web |
| 19 | install.native-platforms / update-doctor；surfaces.shared-session-state；voice.dictation-lifecycle / permission-retention / text-fallback-approval | 平台交付与 voice |

**划分说明：** parity 把 Git/PR/CI 放 Phase 11（生态回执），而 COD-07 traceability 在 Phase 10。约定：Phase 10 覆盖 COD-07 的本地 Git 安全（status/diff/commit/worktree/冲突检测，local_behavior），Phase 11 覆盖 PR/CI/远端回执（target_environment）。voice 同理：COD-15 本地行为在 Phase 11，跨入口完整性由 parity（phase 19）与 `surfaces.*` 收口。

## B. Kiana 超集旅程（差异化承诺）

### coding.repo-onboarding — 仓库接入与项目自动化发现
- 覆盖需求: COD-02
- 必备能力点: init/发现项目说明、agents、rules、prompts、checks、memory；作用域与优先级可见；未信任不加载
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: trust gate 与项目资源过滤已实现；init 生成与优先级展示未验收

### coding.context-composition — 上下文组合与预算
- 覆盖需求: COD-03/04
- 必备能力点: 目录/Read/Grep/Glob/symbol/repo map/impact/URL/图片组合；editable/read-only 固定；context 用量与 compact 语义；遗漏可见
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: context index/search/pack/repo-map 与文件集管理已有；预算显示、图片输入与 compact 恢复语义未验收

### coding.safe-edit-undo — 安全编辑与撤销
- 覆盖需求: COD-05
- 必备能力点: 精确 patch/AI diff 预览/changed-files/mtime 冲突保护/checkpoint/undo；不覆盖用户后续修改
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: Write/Edit/Delete 守卫、changed_files、checkpoint restore 已实现；per-turn checkpoint 与 undo 旅程未闭合

### coding.shell-execution — 受控 Shell 执行
- 覆盖需求: COD-06
- 必备能力点: cwd/env/streaming/timeout/cancel/background/truncation/结构化退出；危险命令策略拒绝或审批
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: exec-policy 与 sandbox 姿态已有；background/daemon 生命周期与结构化退出合同未验收

### coding.git-safety — 本地 Git 安全
- 覆盖需求: COD-07（本地面；远端回执面在 coding.headless-ci 与 parity phase 11）
- 必备能力点: status/diff/log/branch/commit 辅助；worktree 隔离；冲突检测；不动用户未授权 dirty state
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: diff/checkpoint/swarm worktree 已有；commit 辅助与 dirty-state 授权边界未验收

### coding.verify-loop — 自动质量门与修复循环
- 覆盖需求: COD-08
- 必备能力点: 自动发现并运行 format/lint/typecheck/build/test；诊断与受限 repair loop；debug 支持；最大重试
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: `kiana checks`/`review` 隔离执行已有；多语言自动发现与 repair loop 缺失

### coding.interaction-modes — 交互模式与一致性
- 覆盖需求: COD-09
- 必备能力点: Ask/Plan/Edit-Execute 模式、slash command、快捷键、output style、permission prompt 一致；切换不丢 session/policy
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: REPL 分类器/TUI 面板已有；Plan/Ask 模式语义未实现

### coding.review-findings — 结构化审查产出
- 覆盖需求: COD-16
- 必备能力点: code/dependency/security/coverage/release review 输出文件/行/严重度/置信度/复现与验证证据
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: review run/dry-run 合同已有；findings 结构化与验证绑定未实现

### coding.import-migration — 竞品配置迁移
- 覆盖需求: NFR-04
- 必备能力点: Claude Code 配置/session/memory/MCP 可审查导入；预览+回滚；不静默覆盖
- Required proof: local_behavior | 归属阶段: 10
- 当前 gap: `mcp add-from-claude-desktop` 已有雏形；session/memory/配置导入缺失

### coding.ecosystem-automation — 扩展生态与 workbench
- 覆盖需求: COD-10/12
- 必备能力点: MCP 全传输 tools/resources/templates/prompts 使用与调试；skills/plugins/hooks 安装验证禁用；Browser/Chrome/computer-use/screen capture/Notebook/LSP 按平台探测与降级
- Required proof: target_environment | 归属阶段: 11
- 当前 gap: MCP/插件/hook 本地面广泛实现；Chrome/computer-use 平台探测与审计降级未验收

### coding.multi-agent-teams — 多 Agent 编码
- 覆盖需求: COD-11
- 必备能力点: 显式或授权启动 subagent/team；隔离 scope/消息/预算/取消/handoff/集成 gate
- Required proof: local_behavior | 归属阶段: 11
- 当前 gap: bounded swarm/team 基础已实现；用户级启动与 handoff 旅程未闭合

### coding.headless-ci — Headless 与 CI 回执
- 覆盖需求: COD-13（+COD-07 远端面）
- 必备能力点: print/exec/stream-json/SDK/RPC/MCP 在 CI 创建/恢复 session、订阅 events、响应 approval、取消并取 typed result；GitHub Actions 受限凭据与 PR 回执
- Required proof: target_environment | 归属阶段: 11
- 当前 gap: stream-json/SDK 本地面已有；真实 CI 与 PR 回执未验证

### coding.background-remote — 后台与远程延续
- 覆盖需求: COD-14
- 必备能力点: 本地任务转 background/remote worker；四端同一进度/diff/日志/approval；重连不重复副作用
- Required proof: target_environment | 归属阶段: 11
- 当前 gap: remote session/bridge/OAuth 刷新已有；转移语义与幂等重连未验收

### coding.voice-input — 语音输入（本地行为面）
- 覆盖需求: COD-15
- 必备能力点: 开始/暂停/编辑确认/提交；权限与转写来源可见；文本回退；不绕过审批
- Required proof: local_behavior（跨入口 user_value 由 parity phase 19 收口） | 归属阶段: 11
- 当前 gap: 未实现（parity voice 域 missing）
