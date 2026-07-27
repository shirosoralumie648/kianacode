# Phase 10 特性账本 — Coding 公开基线与仓库闭环

**Created:** 2026-07-26 | **父需求：** COD-02..COD-09, COD-16, NFR-04 | **主旅程：** coding.repo-onboarding 等 §B 全部（Phase 10 域）
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。COD-07 只覆盖本地 Git 安全面；PR/CI 远端回执在 Phase 11（见 coding.md 划分说明）。M1 Coding Alpha 的核心账本。

### FEAT-10-01 — 仓库接入与项目资源发现
- 父需求: COD-02
- 领域旅程: coding.repo-onboarding
- 描述: init/打开仓库后发现有作用域和优先级的项目说明、agents、rules、prompts、checks、memory；未信任项目不加载自动化资源。
- 验收:
  - [local_behavior] init 生成与发现旅程完整；作用域/优先级展示；trust gate 回归
- Verifier: onboarding 旅程测试
- 当前基线: partial — trust 过滤/agents 发现/skills 加载已实现；init 生成与优先级展示缺失
- 设计引用: DESIGN-INDEX Phase 10 行（functional-design §5/F01、project_os 04）
- 依赖: FEAT-09-06
- 状态: pending

### FEAT-10-02 — 上下文组合与来源预算
- 父需求: COD-03
- 领域旅程: coding.context-composition
- 描述: 目录浏览、Read/Grep/Glob、symbol/reference、repo map、dependency/impact/trace、图片/截图输入可组合；结果显示遗漏、预算和来源。
- 验收:
  - [local_behavior] 全部来源类型组合可用；图片输入进入 context；遗漏与预算显示
- Verifier: 组合旅程测试
- 当前基线: partial — Read/Grep/Glob/repo-map/search/graph 已实现；symbol/reference、impact/trace、图片输入缺失
- 设计引用: functional-design §8/F04；project_os 06
- 依赖: FEAT-06-03
- 状态: pending

### FEAT-10-03 — 上下文固定、用量与 compact 语义
- 父需求: COD-04
- 领域旅程: coding.context-composition
- 描述: 固定 editable/read-only 文件、引用文件/目录/URL/图片、查看 context 使用量、compact、恢复 compact 后语义，知道哪些内容被省略。
- 验收:
  - [local_behavior] 文件集固定与引用旅程；用量显示；compact 前后语义一致且省略项可见
- Verifier: context 生命周期测试
- 当前基线: partial — editable/read-only 文件集与 compact 已实现；用量显示与省略可见性缺失
- 设计引用: functional-design §8/F04
- 依赖: FEAT-10-02
- 状态: pending

### FEAT-10-04 — 安全编辑、checkpoint 与 undo（parity checkpoint 域）
- 父需求: COD-05
- 领域旅程: coding.safe-edit-undo（parity: cc.checkpoint.turn-snapshot / diff-undo / user-change-protection）
- 描述: Write/Edit/Delete/NotebookEdit 提供精确 patch、AI diff 预览、changed-files、mtime 冲突保护；每个 assistant turn 可 checkpoint、diff、undo；不覆盖用户后续修改。
- 验收:
  - [local_behavior] per-turn checkpoint/undo 旅程闭合；三条 parity checkpoint 能力取得 local_behavior proof；NotebookEdit 补齐
- Verifier: 编辑安全套件 + parity evidence 更新
- 当前基线: implemented(大部) — 编辑守卫/changed_files/checkpoint restore/late-edit 保护已实现；per-turn 自动 checkpoint 与 NotebookEdit 缺失
- 设计引用: functional-design §9/F05；project_os 05
- 依赖: FEAT-04-01
- 状态: pending

### FEAT-10-05 — 受控 Shell 与危险命令策略
- 父需求: COD-06
- 领域旅程: coding.shell-execution
- 描述: shell/PowerShell 任务支持 cwd/env、streaming、timeout、cancel、background/daemon、output truncation、sandbox/network 状态和结构化退出；危险命令执行前被策略拒绝或审批。
- 验收:
  - [local_behavior] 全参数面与结构化退出合同；危险命令 fixture 全部拦截或审批
- Verifier: shell 合同测试
- 当前基线: partial — exec-policy/sandbox 姿态/bash 工具已实现；background/daemon 与结构化退出合同缺失
- 设计引用: project_os 24
- 依赖: FEAT-05-05
- 状态: pending

### FEAT-10-06 — 本地 Git 安全（COD-07 本地面）
- 父需求: COD-07
- 领域旅程: coding.git-safety
- 描述: Git status/diff/log/branch/commit 辅助、worktree 隔离、冲突检测保留预览与审批；不自动处理用户未授权 dirty state（AF-16）。
- 验收:
  - [local_behavior] commit 辅助带预览审批；dirty-state 授权边界 fixture；worktree 冲突检测
- Verifier: git 安全套件
- 当前基线: partial — diff/checkpoint/swarm worktree/dirty 报告已实现；commit 辅助与授权边界缺失
- 设计引用: functional-design §9/F05
- 依赖: FEAT-10-04
- 状态: pending

### FEAT-10-07 — 自动质量门与受限修复循环
- 父需求: COD-08
- 领域旅程: coding.verify-loop
- 描述: 自动发现并运行 format/lint/typecheck/build/test；失败形成诊断和受限 repair loop；debug 支持日志、LSP diagnostics、复现步骤和最大重试；通过后再 review。
- 验收:
  - [local_behavior] Rust/JS 两类样例仓自动发现并运行工具链；repair loop 重试有界且不覆盖用户修改；LSP diagnostics 进入 debug 流
- Verifier: verify-loop 旅程测试
- 当前基线: partial — checks/review 隔离执行（rustfmt/cargo）已实现；多语言发现、repair loop、LSP 集成缺失
- 设计引用: functional-design §12/F08；project_os 25
- 依赖: FEAT-10-05
- 状态: pending

### FEAT-10-08 — 交互模式与一致性（Ask/Plan/Execute）
- 父需求: COD-09
- 领域旅程: coding.interaction-modes
- 描述: Ask、Plan、Edit/Execute 模式及 slash command、快捷键、output style、status line、interactive question、permission prompt 一致；切换模式不丢 session 或绕过 policy。
- 验收:
  - [local_behavior] 三模式语义与切换保持 session/policy；命令/快捷键/输出风格一致性矩阵
- Verifier: 模式切换测试
- 当前基线: partial — REPL 分类器/TUI 审批与设置面已实现；Plan/Ask 模式语义缺失
- 设计引用: project_os 12
- 依赖: FEAT-05-06
- 状态: pending

### FEAT-10-09 — Memory 与 rules 的 Coding 旅程（parity memory 域）
- 父需求: COD-02
- 领域旅程: coding.repo-onboarding（parity: cc.memory.project-user-rules / auto-memory / import-scope）
- 描述: user/project/local rules 作用域与优先级、自动 memory 形成与 stale 边界、memory import/export 达到 parity local_behavior proof。
- 验收:
  - [local_behavior] 三条 parity memory 能力逐项取证并更新 evidence
- Verifier: parity evidence 更新
- 当前基线: partial — rules 加载与 memory JSONL 已实现（cc.memory.project-user-rules 为 implemented）；auto-memory 与 import-scope 为 partial
- 设计引用: governance parity；project_os 28
- 依赖: FEAT-04-05, FEAT-10-01
- 状态: pending

### FEAT-10-10 — 结构化 review findings
- 父需求: COD-16
- 领域旅程: coding.review-findings
- 描述: code review、dependency/security review、test coverage 和 release review 产出带文件/行/严重度/置信度/复现与验证证据的 findings；未验证安全问题不得包装成事实。
- 验收:
  - [local_behavior] findings schema（file/line/severity/confidence/repro/verification）；未验证项显式标注
- Verifier: findings 合同测试
- 当前基线: partial — review run 合同已有；findings 结构化与验证绑定缺失
- 设计引用: project_os 05/25
- 依赖: FEAT-10-07
- 状态: pending

### FEAT-10-11 — 竞品配置迁移导入
- 父需求: NFR-04
- 领域旅程: coding.import-migration
- 描述: 从 Claude Code 导入配置、session、memory、MCP 配置：预览、可回滚、不静默覆盖。
- 验收:
  - [local_behavior] 四类导入各有预览+回滚；冲突显式提示
- Verifier: 导入旅程测试
- 当前基线: partial — `mcp add-from-claude-desktop` 已有；其余三类缺失
- 设计引用: AF-18 正向面
- 依赖: FEAT-10-01
- 状态: pending
