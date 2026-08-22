# Coding pack 公开行为矩阵（CODE-01）

Date: 2026-08-23
Status: **签字草稿**（v0.4 Phase 2）+ CODE-02 stdio **已绿** + CODE-03 context/PreToolUse **已绿**
Proof: `local_behavior`
Companion: `.planning/phases/10-CONTEXT.md`、`.planning/phases/11-VERIFICATION.md`、`.planning/phases/12-VERIFICATION.md`、`PHASES.md` v0.4.3、`DESIGN.md` §4

本文件是 v1.0 `REL-03` 的审计底表。没有本表条目、owner、测试和证据的能力，不得计入 Coding pack 完成。

这不是 Claude Code 克隆清单，也不是把
`reference/claude-code-rev-main/src/tools/` 的目录名搬进 `kiana-tools`。

---

## 0. 怎么读

### 0.1 列

| 列 | 含义 |
|---|---|
| 公开行为 | 用户能看见或依赖的产品行为，用动词短语，不写内部类型名 |
| 公开来源 | 只读公开文档 / 教程 / 模块名；禁止当实现源 |
| Kiana 现状 | owned harness 真相。legacy `kiana-tools` / `runner.rs` 不算产品路径 |
| owner | 产品路径 crate。空 = 尚未有产品 owner |
| 测试 / 证据 | 已有测试或「缺」 |
| 许可证 | 可借鉴形状 / 只许行为对照 / 禁止 |
| 优先级 | P0 = v1.0 `REL-03` 核心路径；P1 = 工作台加宽；P2 = 可延期 |
| 是否本期 | 见下表 |

### 0.2 「是否本期」标记

| 标记 | 含义 |
|---|---|
| **已绿** | v0.2 / v0.3 / v0.4 Phase 1–4 已在 owned harness 上证明 |
| **文档本期** | 本 Phase 只进矩阵，不写代码 |
| **矩阵后开** | 本草稿签字后才允许开实现；默认下一刀是 CODE-04 |
| **冻结** | 直到声明的版本，或永远不做产品总线 |

v0.4 Phase 4 把 P0-SKILL / P0-HOOK / CODE-03 打成已绿（诚实：Skill 是 System 上下文，不是 skill 工具；hook 是 PreToolUse，不是全套）。HTTP MCP、provider 仍不是代码完成。

### 0.3 对照源（只行为）

| 源 | 能当什么 | 不能当什么 |
|---|---|---|
| `reference/ai-coding-guide/claude-code/` | 教程视角的公开用法：5 类工具、权限、MCP、skills、hooks、slash、checkpoints、SDK | 不是官方源码 |
| `reference/ai-coding-guide/codex/` | Codex 形：sandbox 档位、审批、AGENTS.md、MCP/skills | 不是把 Kiana 换成 Codex |
| `reference/claude-code-rev-main/README.md` + `src/` **目录名** | 用户可见工具/命令名的存在证明 | dump，无 git；禁止搬源码 |
| `reference/claude-code-main (2)/claude-code-main/` | 官方公开仓库的安装/插件叙事 | `LICENSE.md` = Anthropic 专有；无完整运行时源码 |
| `reference/claude-code-rust` | 反面教材：第 1 天 v1.0.0 | 禁止当站序或完成定义 |
| Codex / deepseek / pi / cline git（`PROCESS.md`） | MCP/skills **何时**进产品 | 不把 git 日期当完成 |

两个 dump 无 git，不能当演进证据。Kiana 工人形状跟 Codex：模型只见 `shell` + `apply_patch` + `mcp`，副作用由 daemon broker。

### 0.4 本草稿锁死的产品决策

1. owned harness 保持 broker 工具面：`shell` + `apply_patch` + `mcp`。不把 `kiana-tools` 50+ 接上当完成。
2. P0 ≠ dump 工具数。Read/Grep/Glob 不是 P0；今天用 `shell` 搜。
3. MCP 必须经 daemon broker 进 harness，禁止 `runner.rs` 当产品 MCP。
4. Skills / hooks 必须经 harness + trust，加载器存在 ≠ 接到工人。
5. TeamCreate / SendMessage 永远不是产品总线。跨部门走 WorkPacket；部门内走有界 Symposium。
6. 证明上限仍 `local_behavior`。live provider / `~/.local/bin` / `scripts/release-smoke.sh` 不是本表完成。

---

## 1. P0 — v1.0 Coding pack 核心路径

这些必须最终全绿，才允许宣称 `REL-03`。其中已绿项是回归基线；未绿项按 §6 打开，不得一次做完。

| ID | 公开行为 | 公开来源 | Kiana 现状 | owner | 测试 / 证据 | 许可证 | 优先级 | 是否本期 |
|---|---|---|---|---|---|---|---|---|
| P0-LOOP | 用户用自然语言派活，工人自己「想→做→看」直到写出结果或失败可见 | 教程 `03-how-it-works.md`；Codex `02-core-concepts.md` | `kiana run` → DaemonHost → KianaHarness。cassette / fake-script 可跑；live 不是完成 | `kiana-entrypoints` `harness_run.rs`；`kiana-daemon`；`kiana-runner` | `cli_run` golden cassette；`scripts/harness-golden-smoke.sh` | Codex Apache-2.0 形状可借鉴 | P0 | 已绿 |
| P0-WRITE | 在受信工作区改/建文件 | 教程「文件操作」；Codex apply_patch | 模型工具 `apply_patch`（及 `shell` 写盘）。默认 Builder | `kiana-runner/src/tools.rs`；`kiana-daemon/src/apply_patch.rs` | `trusted_workspace_write_cassette_creates_golden_path_file` | Codex Apache-2.0 | P0 | 已绿 |
| P0-SHELL | 在沙箱里跑命令（测试、git、构建） | 教程「执行」；Codex sandbox.md | 模型工具 `shell` → `shell.exec`；Linux bubblewrap | `kiana-daemon/src/harness_sandbox.rs`；`harness_capabilities.rs` | cli_run / daemon_host 写盘与 deny | Codex Apache-2.0 | P0 | 已绿 |
| P0-TRUST | 未信任项目任何副作用 deny | 教程 `20-permissions.md` `21-security.md`；Codex 审批 | 未 trust → `project_untrusted`；文件不出现 | `kiana-commands` trust；`kiana-types` trust；`kiana-policy` | `untrusted_workspace_write_does_not_create_file` | 行为对照 + 本仓已有 | P0 | 已绿 |
| P0-SANDBOX | 默认只读；写盘必须显式工作区可写；工作区外 fail-closed | Codex `read-only` / `workspace-write` / `danger-full-access` | CLI `--sandbox workspace-write`；默认 read-only；role 可再收紧 | `kiana-entrypoints` harness_run；`kiana-policy`；RoleSpec | v0.2/v0.3 CLI 测试；reviewer read-only | Codex Apache-2.0 | P0 | 已绿 |
| P0-FAIL | 无模型、空 prompt、未知 session、未知 role 失败可见，禁止假成功 | 教程打断/权限；12-factor #7 | 专用错误码：`prompt_required`、`project_untrusted`、`role_unknown` 等 | `kiana-core`；`kiana-daemon`；CLI | `run_without_model_fails_closed` 等 | 本仓 | P0 | 已绿 |
| P0-CONT | 同一 harness session 可 continue | 教程 `/resume`；Codex rollout；deepseek session | **同一 DaemonHost 生命周期** 可 continue；跨进程新 host fail-closed（除收据/review 按持久 session_id 查） | `kiana-core`；`kiana-daemon`；CLI `--continue` | cli_run / daemon_host continue 测试 | 行为对照 | P0 | 已绿（同核） |
| P0-CANCEL | 用户能停掉进行中的能力 | 教程 Esc 急停 | `kiana run --cancel <session>` | `kiana-core`；CLI | `cancel_unknown_session_fails_closed` + daemon cancel | 行为对照 | P0 | 已绿 |
| P0-RCPT | 工具调用与改过的文件可列出；进程重启后第一次 run 不被第二次覆盖 | Codex rollout；aider history | 产品 host 用 `JsonlEventLog`；`kiana run --receipt`；`run.receipt` 含 `session_id` / `role_id` / `files_changed` | `kiana-eventlog`；`kiana-core` | daemon `disk_receipts_survive_restart_and_do_not_overwrite_the_first_run` | 本仓 | P0 | 已绿 |
| P0-ROLE | 每次 run 带角色与部门；默认执行部 Builder；规划角色不能写 src | COMPANY.md；本仓 v0.3 | 目录 `planning/pm` `planning/architect` `executing/builder` `monitoring/reviewer` | `kiana-domain` RoleSpec | Phase 1–3 CLI 测试 | 本仓 | P0 | 已绿 |
| P0-REV | 作者 ≠ 评审；评审新 session；确定性门 | 教程 `/review` 只作行为名；COMPANY 监控部 | `kiana run --review <author_session_id>` 写 `gate/REVIEW.json`；不跑模型；不复制 transcript | `kiana-core`；`kiana-domain` | `.planning/phases/9-VERIFICATION.md` | 本仓 | P0 | 已绿 |
| P0-ORCH | 独立上下文工人；packet 是唯一输入；规划会有界 | COMPANY.md；architect-loop；OpenSpec | `--packet` 新 session；`--symposium` PM+Architect 硬顶轮次；anti-meeting 可跳过 | `kiana-core`；`kiana-domain` | `.planning/phases/6,7,8-VERIFICATION.md` | 本仓 + 方法对照 | P0 | 已绿 |
| P0-MCP | 作为 MCP **客户端** 发现/调用外部 server（stdio / HTTP）；工具经同一审批/沙箱 | 教程 `22-mcp.md`；Codex mcp-types 2025-05-02；deepseek `packages/mcp` 2026-07-07 | **stdio 已绿。** 模型工具 `mcp` → broker `mcp.call`。`KIANA_MCP_SERVERS_JSON`。HTTP → `mcp_transport_unsupported`。legacy `kiana-tools/mcp_tool.rs` 与 `kiana-entrypoints/mcp.rs` **仍不算产品路径** | `kiana-daemon` `harness_mcp.rs`；`kiana-runner` `tools.rs`；`kiana-policy` | `.planning/phases/11-VERIFICATION.md`；`trusted_builder_stdio_mcp_echoes_through_daemon` | MCP 规范公开；实现对照 Codex/deepseek Apache/MIT，不对照 dump | P0 | 已绿（stdio）；HTTP 仍后开 |
| P0-SKILL | 项目/用户 Skill 对工人可见；trust 决定项目 Skill；按需加载 | 教程 `26-agent-skills.md`；`reference/skills` | **context 已绿。** Daemon `SkillAwareRunner` 把 skill pack 注入 harness 第一条 System 消息。项目 `.claude/skills` / `.kiana/skills` 未信任 withhold。不是模型工具 `skill`，不是 dump SkillTool | `kiana-daemon` `harness_skills.rs`；`kiana-skills`；`kiana-runner` | `.planning/phases/12-VERIFICATION.md`；`trusted_project_skill_appears_in_harness_system_message` | Agent Skills 公开用法可借鉴；dump SkillTool 禁止搬 | P0 | 已绿（context） |
| P0-HOOK | pre/post tool、stop、session 钩子能拦或续；失败可见 | 教程 `33-hooks.md`；deepseek `packages/hooks` | **PreToolUse 已绿。** policy Allow 之后、broker execute 之前可 `hook_blocked`。Ask fail-closed。post/stop/session 仍不是产品完成 | `kiana-core` `pre_tool_hook_block`；`kiana-query` stop_hooks | `.planning/phases/12-VERIFICATION.md`；`pre_tool_use_hook_blocks_apply_patch_before_broker_execute` | 行为对照 | P0 | 已绿（PreToolUse） |
| P0-PROV | 多供应商能力差必须显式报告/降级，禁止静默装等价 | 教程 `05-third-party-models.md`；OpenHands provider settings；12-factor #2/#3 | `kiana-services` 有 Anthropic / OpenAI-compatible / Ollama 与 `UnsupportedCapability`。**`kiana run` 未向用户证明**「该供应商缺 tools/streaming」 | `kiana-services`；`kiana-daemon/src/model_client.rs` | provider 单测有 unsupported_tools；无 CLI 产品证据 | 本仓 + 行为对照 | P0 | 矩阵后开（CODE-04） |

P0 搜索策略（锁死）：Claude Code 教程把「按文件名 / 正则搜」列为 5 类内置工具之一。Kiana 选择 Codex 形——**用 `shell` 跑 `rg`/`ls`/`cat`**，不在 P0 再做一个 Read/Grep/Glob 注册表。若 v0.4.2–0.4.3 之后仍无法在沙箱里可靠搜索，再开 P1-READ。

---

## 2. P1 — 工作台加宽（矩阵要才做，不是 dump）

| ID | 公开行为 | 公开来源 | Kiana 现状 | owner | 测试 / 证据 | 许可证 | 优先级 | 是否本期 |
|---|---|---|---|---|---|---|---|---|
| P1-READ | 结构化 Read / Grep / Glob（模型可见，sandbox 复用） | 教程 5 类工具「搜索」「文件操作」；dump `FileReadTool` `GrepTool` `GlobTool` 仅作名字 | harness 无这些工具。`kiana-query` repo_map / search 是上下文能力，不是模型工具。legacy `kiana-tools` 有对应实现，**不是产品路径** | 若打开：daemon broker 新能力，不扩 `kiana-tools` 产品面 | 缺 | 行为对照；形状可看 continue `core/tools`、aider `repomap.py` | P1 | 矩阵后开（v0.4.5，且仅当 shell 搜索不够） |
| P1-INSTR | 项目说明书每次会话注入（AGENTS.md / CLAUDE.md 类） | Codex `11-agents-md.md`；教程 `18-claude-md-guide.md` | 仓库有 `AGENTS.md` 给人看。harness **未**把项目说明书当工人输入证明 | `kiana-runner` context assembly | 缺 | 行为对照 | P1 | 文档本期 |
| P1-COMPACT | 长对话可压缩后续聊，用户可感知 | 教程 `/compact` `19-context-management.md` | `kiana-query` token budget / 状态机存在；用户不可感知 compact | `kiana-query`；harness | query 单测 | 行为对照 | P1 | 文档本期（产品化跟 v0.5 LONG） |
| P1-PLAN | 只读规划模式，出方案不改 src | 教程 Plan mode；dump `EnterPlanModeTool` | 规划角色 `pm`/`architect` 已不能写 src，可写 `plan/` `packet/`。没有 Claude 式 Shift+Tab plan mode | RoleSpec | v0.3 CLI | 本仓已有角色替代 | P1 | 文档本期（不另做 plan mode CLI，除非矩阵复审） |
| P1-REWIND | 编辑工具改动可回滚；bash/外部副作用不在内 | 教程 `37-checkpoints.md` `/rewind` | 无产品 checkpoint。收据列出 `files_changed`，不是游戏存档 | — | 缺 | 行为对照 | P1 | 文档本期 |
| P1-WEB | 搜网页 / 抓文档 | 教程「网络」；dump `WebFetchTool` `WebSearchTool` | legacy 有 web 工具；harness 无。优先 MCP 接，而不是再做一套内置浏览器 | MCP（CODE-02）或日后 `kiana-network` broker | 缺 | 行为对照 | P1 | 矩阵后开（建议经 MCP，不经 dump WebBrowser） |
| P1-SLASH | 会话内斜杠元操作：help / 清上下文 / 切模型 / 权限 | 教程 `36-slash-commands.md`（`/help` `/clear` `/model` `/mcp` `/permissions`…） | CLI 是 `kiana run` 开关，不是 REPL slash。TUI park。不要把 dump `src/commands/` 百个目录当 P0 | 若做：harness REPL 或命令层 | 缺 | 行为对照；禁止搬 dump 命令实现 | P1 | 文档本期 |
| P1-SUB | 专项子工人独立上下文，父级持有证据 | 教程 `23-subagents.md`；dump `Task*` | Kiana 用 RoleSpec + packet spawn，**不是** Claude Task 工具。禁止 TeamCreate | `kiana-core` packet/symposium | v0.3 已绿 | 本仓 | P1 | 已绿（公司形，不是 Task 工具形） |
| P1-GIT | 常用 git 工作流（status/diff/commit）在沙箱内 | 教程 `43-git-workflow.md` | 经 `shell` 即可，无单独 Git 工具。这是 Codex 形的正确选择 | `shell.exec` | 无专门 git 测试 | 行为对照 | P1 | 文档本期（不新工具） |
| P1-LSP | 转到定义 / 诊断 | 教程「代码智能」明确是**插件**；dump `LSPTool` | 非开箱核心。crush/continue 可作后期形状 | — | 缺 | 行为对照 | P2 | 冻结到 v0.6+ 或 skills/MCP |
| P1-NB | Notebook 编辑 | dump `NotebookEditTool` | 非核心路径 | — | 缺 | 行为对照 | P2 | 冻结 |
| P1-CRON | 定时任务 / 监控 | dump `ScheduleCronTool` `MonitorTool` | legacy 有 cron 形；不是 harness | — | 缺 | 行为对照 | P2 | 冻结到 v1.x 自动化 |

---

## 3. 扩展层（P0 缺口的实现约束）

这些是 Coding pack 审计项，不是「把 dump 工具抄过来」。

| ID | 公开行为 | 实现约束（签字） | 参考（实现形状，不抄 dump） | 是否本期 |
|---|---|---|---|---|
| CODE-02 | MCP client | 发现、列出工具、调用、错误、权限全走 daemon broker。stdio 默认本地进程；HTTP 远程。SSE 不作为新默认。第一次调用要审批。失败码可见。**禁止** `kiana-entrypoints/src/runner.rs` 接 MCP。**禁止**把 `kiana-tools/mcp_tool.rs` 标成完成 | Codex `codex-rs/codex-mcp` + `scripts/mcp_conformance/`；deepseek `packages/mcp`；cline `extensions/mcp`；continue `mcpToolName.ts`；orca inspector | 已绿（stdio）；HTTP 仍 `mcp_transport_unsupported` |
| CODE-03 | Skills / hooks | Skill 可见性跟 ProjectTrust；用户/bundled 在未信任时仍可；项目 `.claude/skills` 或 Kiana 等价目录未信任 withhold。接到 harness **prompt 装配**（System），不是工具面。Hooks fail-closed。ECC / awesome-agent-skills 是目录不是运行时 | `reference/skills`；deepseek `packages/skill` `packages/hooks`；pi extensions；gstack `SKILL.md` | 已绿（context + PreToolUse） |
| CODE-04 | Provider 显式降级 | Anthropic / OpenAI-compatible / Ollama：缺 tools 或 streaming 必须在 `kiana run --json` 里出现机器可读错误，不能空转成功 | 本仓 `ProviderError::UnsupportedCapability`；OpenHands provider settings；pi reasoning-token；aider `models.py` | 矩阵后开（可与 MCP 后并行） |

已有但**不算完成**的代码：

- `kiana-entrypoints/src/mcp.rs` — Kiana 作为 MCP **server** 暴露 legacy 工具，给别人调 Kiana。这是反方向。
- `kiana-services/src/mcp.rs` + `kiana-tools/src/mcp_tool.rs` — client 在 legacy 注册表。
- `kiana-skills/` — 加载器仍在；产品路径是 daemon 把它装配进 harness System，不是 SkillTool。
- `kiana-query/src/stop_hooks.rs` — PreToolUse 已接到 harness 黄金路径；post/stop/session 仍未当产品完成。

---

## 4. 入口 / 表面

| ID | 公开行为 | 公开来源 | Kiana 现状 | 是否本期 |
|---|---|---|---|---|
| SURF-CLI | 终端非交互 / 可脚本入口 | 官方 README `claude`；Codex CLI | `kiana run` / print 走 harness。**产品第一入口** | 已绿 |
| SURF-TUI | 全屏终端同一内核 | Codex TUI 开源后 9 天仍是同一产品 | `kiana tui` **park**：legacy SDK/stream，不是 DaemonHost | 冻结（保持 park，直到书面再迁；v0.6 才考虑） |
| SURF-SDK | Headless / print / RPC 同一核 | 教程 `45-agent-sdk.md` | print 经 `execute_owned_harness_turn`；`kiana --resume` / `cli_resume.rs` 仍是 legacy | 文档本期；彻底离开 `runner.rs` = v0.6 SURF2-01 |
| SURF-IDE | 编辑器扩展 | 教程 `08-vscode.md` `09-jetbrains.md`；continue/cline git | 无产品 IDE。git 共识：先做好一个入口 | 冻结到 v0.6 |
| SURF-DESK | 桌面工作区 | 教程 `10-desktop.md`；orca/emdash | 无。Desktop 若做必须包 DaemonHost | 冻结到 v0.6 末 / v1.0 以后 |
| SURF-WEB | Web / 云会话 | 教程 `11-web-and-cloud.md` | 无 | 冻结到 v1.x |
| SURF-CHROME | 浏览器自动化 | 教程 `40-chrome.md`；dump Chrome MCP | `kiana-chrome-mcp` 存在，非 harness | 冻结 |
| SURF-CU | Computer use / 键鼠截屏 | Codex `17-computer-use.md` | `kiana-computer-*` feature-gated，非产品路径 | 冻结 |

---

## 5. 冻结（本表明确说「不要做」）

| ID | 行为 / 目录 | 为什么冻 | 直到 |
|---|---|---|---|
| FZ-TEAM | dump `TeamCreateTool` `SendMessageTool`；本仓 `kiana-coordinator` | 自由群聊总线。跨部门只交 packet | **永远**不当产品总线 |
| FZ-TOOLS | 把 `kiana-tools` 50+ 接到 harness | 完成定义会变成工具数。owned harness 是 Codex 形 broker 工具面（`shell` + `apply_patch` + `mcp`） | 永远不按「接上」完成；单点能力走 broker |
| FZ-DUMP | `reference/claude-code-rev-main/src/**` 当实现 | 无 git 的还原树；许可证不清 | 永远只许模块名对照 |
| FZ-CCMAIN | `reference/claude-code-main (2)` 源码复用 | Anthropic Commercial ToS | 永远只许公开 README/插件叙事 |
| FZ-CCRUST | `reference/claude-code-rust` 站序 | 第 1 天发布 v1.0.0 再修编译再补 MCP | 永远当反面教材 |
| FZ-CLI | 拆 `cli.rs`（~25k / 934KB）当目标 | 只在挡住 spine 时拆 | 非本里程碑 |
| FZ-DEPT | 五部门 + 六层 RAG + JointSymposium | 打开条件是 v0.4 核心路径（含 MCP **或**本表明确延期 MCP） | v0.5 |
| FZ-SWARM | ruflo Queen / 15–100 agent / 共享 swarm memory | v0.6.4 才谈并行 Builder + path lock | v0.6 |
| FZ-ENT | 租户 / SSO / 托管策略 / 官方云 | git 里没有「先企业再写盘」 | v1.x |
| FZ-104 | 恢复旧 104 req 语料 | 已删；本矩阵替代 | 永远不 |

延期 MCP 的合法写法：若 CODE-02 打开前发现许可证或安全块，必须在本文件改「是否本期」为 **冻结到 v0.5 之后** 并说明原因。未改表就把 MCP 标完成，无效。

---

## 6. 本草稿签字后的打开顺序

只打开一行。做完再动下一行。

| 顺序 | 站 | 需求 | 用户可见完成 | 不做什么 |
|---|---|---|---|---|
| 已完成 | v0.4.2 | CODE-02 | `kiana run` 的工人能经 daemon 调一个本地 stdio MCP；未信任 deny；JSON 标明 harness + mcp | 市场、SSO、把 legacy mcp_tool 打勾 |
| 已完成 | v0.4.3 | CODE-03 | 受信项目的一条 Skill 出现在 harness System 上下文；未信任项目 Skill 不出现；PreToolUse 可拦 apply_patch | 把 awesome-agent-skills 当运行时；SkillTool |
| 下一刀 | v0.4.4 | CODE-04 | 不支持 tools 的 provider profile → 机器可读失败，不写盘、不假成功 | live 矩阵当完成 |
| 仅当需要 | v0.4.5 | P1-READ | 结构化 Grep/Glob/Read 走 broker + 同一 sandbox | 50-tool 注册表 |
| 再然后 | v0.5 | LONG/DEPT/MEM | 五部门对象 + 六层 RAG ACL | 用 MCP 代替部门 |
| 再然后 | v0.6 | SURF2 | SDK/print 100% harness；下一入口 IDE 优先于 Desktop | 为 Desktop 复制 runner |
| 再然后 | v1.0 | REL | 安装升级回滚 + 文档 + **本表 P0 全绿** | 工具数 100%、企业、TUI 像素对等 |

`PHASES.md` v0.4 打开条件「有一份签字的公开行为矩阵草稿」= **本文件**。v0.4.3 CODE-03 context + PreToolUse 已绿。下一独立 Phase 是 v0.4.4 CODE-04。

---

## 7. Dump 工具目录名（存在证明，全部不是 P0）

来源：`reference/claude-code-rev-main/src/tools/` 目录名。只证明「还原树里有这个名字」。Kiana 不按此表实现。

| 目录名 | 映射到本矩阵 | 处理 |
|---|---|---|
| BashTool PowerShellTool | P0-SHELL | 已绿（`shell`） |
| FileReadTool FileEditTool FileWriteTool | P0-WRITE / P1-READ | 写 = apply_patch；读 = shell 或日后 P1-READ |
| GlobTool GrepTool | P1-READ | 矩阵后开，非 P0 |
| WebFetchTool WebSearchTool WebBrowserTool | P1-WEB | 经 MCP 或冻 |
| MCPTool ListMcpResourcesTool ReadMcpResourceTool McpAuthTool | P0-MCP / CODE-02 | stdio 已绿；HTTP/auth 仍后开 |
| SkillTool DiscoverSkillsTool | P0-SKILL / CODE-03 | context 已绿；**不是** SkillTool |
| EnterPlanModeTool ExitPlanModeTool VerifyPlanExecutionTool | P1-PLAN | 用角色替代 |
| TaskCreateTool TaskGetTool TaskListTool TaskOutputTool TaskStopTool TaskUpdateTool | P1-SUB | 用 packet/role，不抄 Task 工具 |
| TeamCreateTool TeamDeleteTool SendMessageTool | FZ-TEAM | 永远不当总线 |
| AskUserQuestionTool | 审批/人门 | v0.5 HITL；现 fail-closed 非交互 |
| TodoWriteTool | 工作记忆 | 文档本期；不 P0 |
| LSPTool REPLTool NotebookEditTool | P1-LSP / P1-NB | 冻结 |
| ScheduleCronTool MonitorTool RemoteTriggerTool | P1-CRON / SURF | 冻结 |
| EnterWorktreeTool ExitWorktreeTool | v0.6 swarm/worktree | 冻结 |
| WorkflowTool ReviewArtifactTool | 本仓 symposium/review | 已有公司形，不抄 |
| ConfigTool BriefTool SleepTool SnipTool ToolSearchTool TerminalCaptureTool SendUserFileTool OverflowTestTool SyntheticOutputTool TungstenTool | 内部/测试/专有 | 忽略 |

Slash 命令 dump（`src/commands/`）同样只作名字存在证明：`/mcp` `/skills` `/resume` `/review` `/compact` `/init` `/permissions` `/hooks` `/plugin` `/chrome` `/desktop`…。Kiana 用 `kiana run` 开关表达已绿项（`--review` `--continue` `--receipt` `--sandbox` `--role` `--packet` `--symposium`）。不要为对齐 slash 菜单而开 TUI。

---

## 8. 反面教材

`reference/claude-code-rust`：第 1 天提交「完整功能 + 发布 v1.0.0」，随后修编译，再补 MCP。这正好是本矩阵禁止的完成定义——功能名打勾、无 owner/测试/许可证列、无黄金路径证据。

Kiana 反例已发生过一次：24-phase Project OS 跳过「能改文件」。本表存在就是为了不再跳站。

---

## 9. 许可证边界（逐源）

| 源 | 许可 | Kiana 规则 |
|---|---|---|
| Codex (`reference/codex`) | Apache-2.0 | shell / apply_patch / sandbox / MCP client 形状可借鉴；保留来源注释 |
| deepseek-harness / pi / aider / OpenHands | 各开源许可，以目录 LICENSE 为准 | 可借鉴分层与站序 |
| `reference/claude-code-main (2)/claude-code-main/LICENSE.md` | © Anthropic PBC，Commercial ToS | 只读 README/插件/公开文档叙事 |
| `reference/claude-code-rev-main` | 还原树，非上游；许可不明 | 只许模块名与用户可见命令对照 |
| `reference/ai-coding-guide` | 教程 | 公开用法对照 |
| `reference/claude-code-rust` | 自称 MIT 的再实现 | 不抄；当失败案例 |
| MCP 规范 | 公开标准 | 可实现 client；不抄 dump 的 MCPTool 源码 |

开源源码仅在许可证兼容时复用。专有产品只做 clean-room 行为层。

---

## 10. v1.0 `REL-03` 将检查什么

宣称个人完整产品的 Coding pack 时，检查员只看：

1. 本表 **P0-LOOP … P0-ORCH 仍绿**（回归）。
2. **P0-MCP、P0-SKILL、P0-HOOK、P0-PROV 变绿**，或本表已改为延期并写明用户能在无 MCP 时完成 v0.2 意义的写盘任务。
3. 每条绿行有 owner crate + 测试名 + `local_behavior` 证据。
4. dump 工具数、`kiana-tools` 文件数、reference 打勾、crate 数，全部无效。

本 Phase 不把 2 变成绿。本 Phase 只把表造出来。
