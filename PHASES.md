# Kiana 阶段剧本：每一期做什么、改什么、读什么

Date: 2026-08-22
Status: 执行剧本（比 DESIGN.md / PROCESS.md 更细）
Companion: [COMPANY.md](COMPANY.md)（公司编排）、[DESIGN.md](DESIGN.md)、[PROCESS.md](PROCESS.md)

怎么用这份文件：

1. 只打开 **当前版本当前 Phase**。后面的章节是地图，不是待办。
2. 每个 Phase 都跑 PROCESS.md 的内环：classify → discuss → spec → plan → 落盘 → execute → verify。
3. 读 `reference/` 是读 **形状、协议、站序、失败案例**，不是把源码搬进 Kiana。
4. `reference/claude-code-rev-main` 与 `reference/claude-code-main (2)` 无 git，只许公开行为对照。
5. 本里程碑证明上限：`local_behavior`。
6. 北星是 Company OS，但 **v0.2 仍然只打开 Builder 工人**。不准用招角色代替写盘。

路径约定：`reference/...` 相对仓库根 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode`。

---

## 0. 每个 Phase 共用的内环（先做这些，再写代码）

| 步骤 | 做什么 | 参考 |
|---|---|---|
| Classify | spike / bounded / phase。Phase 1 是 phase，不是 spike | `reference/superpowers/skills/brainstorming/SKILL.md`（三路路由） |
| Discuss | 只锁本表「未决决策」；代码能回答的不要问 | `reference/gsd-core/skills/gsd-discuss-phase/SKILL.md`、`reference/OpenSpec/skills/openspec-explore/SKILL.md` |
| Spec | WHEN/THEN 场景，写死证明级别 | `reference/OpenSpec/skills/openspec-new-change/SKILL.md`、`reference/OpenSpec/skills/openspec-propose/SKILL.md` |
| Plan | 任务能装进新上下文；列出冻结项 | `reference/superpowers/skills/writing-plans/SKILL.md`、`reference/gsd-core/skills/gsd-plan-phase/SKILL.md` |
| 落盘 | `task_plan.md` / `progress.md` / `findings.md` | `reference/planning-with-files/templates/` |
| Execute | 原子提交；只改本 Phase 文件 | `reference/superpowers/skills/executing-plans/SKILL.md`、`reference/gsd-core/skills/gsd-execute-phase/SKILL.md` |
| Verify | 对着成功标准，不是 checkbox | `reference/gsd-core/skills/gsd-verify-work/SKILL.md`、`reference/superpowers/skills/verification-before-completion/SKILL.md` |
| Review | 有 diff 再做 | `reference/superpowers/skills/requesting-code-review/SKILL.md` |

运行时不变量（全程强制）：`reference/12-factor-agents/content/factor-01-natural-language-to-tool-calls.md` 到 `factor-12-stateless-reducer.md`。

---

## v0.2 Phase 1 — CLI 黄金路径（当前唯一可执行）

**打开条件：** 现在。  
**用户可见完成：** 在受信仓库里，真模型（或录制自真模型的回放）经 `DaemonHost` 用 `shell` / `apply_patch` 改出文件。未信任、无模型、空 prompt 失败可见。

```bash
kiana trust .
kiana run --sandbox workspace-write -- "create a file named GOLDEN_PATH.txt containing hello"
test -f GOLDEN_PATH.txt
```

**需求：** PATH-01..04, TRUST-01..02  
**git 为什么现在做：** aider 第 1–4 天 chat→edit；Codex 开源当天 CLI+shell；deepseek 第二天 `echo-agent`；cline 第一周就 abort/工具，但 Kiana 选的是 CLI 第一入口。

### 未决决策（discuss 只问这些）

1. **非交互写盘授权：** `kiana-policy` 对 `LocalWrite` 现在是 `Ask`。`kiana run` 无 TTY 时：  
   - A（推荐）：trusted + `--sandbox workspace-write` → 自动 Allow 工作区内写；工作区外仍 Deny。  
   - B：必须 `--yolo` / 预置 approval token。  
   参考 Codex `reference/codex/docs/sandbox.md`、`reference/codex/codex-rs/execpolicy`，以及 cline `reference/cline/sdk/packages/core/src/runtime/tools/tool-approval.ts`。
2. **Provider 证明：** CI 用 cassette/录制；本机可选 live。不要把 `KIANA_HARNESS_SCRIPT` 当 PATH 完成。
3. **成功形态：** 文件由 `apply_patch` 或 `shell` 创建均可，但必须发生在 project root 内，且 JSON 标明 harness。
4. **角色入口：** 产品北星是五部门公司（`COMPANY.md`）。v0.2 是否把 CLI 改成 `kiana company run`，或保持 `kiana run` 但收据强制 `role=builder`、`department=executing` + RoleSpec 类型先落地？（推荐后者。Symposium / 六层 RAG / 五部门行为 v0.3–v0.5 再做。）

### 工作包

**WP1 — 确认入口已经打在 harness 上（审计，几乎不写功能）**

- 读：`kiana-entrypoints/src/harness_run.rs`、`cli.rs` 里 `run`/`print` 分发、`kiana-entrypoints/src/sdk.rs` 的 `execute_owned_harness_turn`。
- 现有测试只证明 fake script：`kiana-entrypoints/tests/cli_run.rs`、`cli_print.rs`。
- 产出：一张「哪些子命令仍进 `runner.rs`」清单，print/run 必须不在其中。

**WP2 — 受信 + sandbox 档位打通写盘**

- `kiana-commands/src/trust.rs` + `kiana-types` trust 记录。
- `kiana-entrypoints/src/harness_run.rs` 的 `sandbox_policy_from_options`。
- `kiana-daemon/src/harness_sandbox.rs`（已注明派生自 Codex `linux-sandbox` + `core/spawn.rs`）。
- `kiana-daemon/src/harness_capabilities.rs`、`apply_patch.rs`。
- `kiana-policy/src/lib.rs`：按 discuss 结果改 trusted+workspace-write 的 LocalWrite。
- 断言：未 trust → 任何 `shell.exec`/`apply_patch` deny；trust + read-only → 不能写出文件；trust + workspace-write → 能写出，且写到 root 外失败。

**WP3 — 真模型回路（或录制回放）**

- `kiana-daemon/src/model_client.rs`
- `kiana-runner/src/model.rs`、`harness.rs`、`tools.rs`（模型只见 `shell`/`apply_patch`）
- `kiana-services/src/api/provider.rs`、`streaming.rs`、`retry.rs`
- 新增：不依赖 `KIANA_HARNESS_SCRIPT` 的测试。优先 cassette（提交一组 tool_call 序列）；live 走 `scripts/provider-live-smoke.sh` 但标 `live`，失败不升级证明级别。

**WP4 — Fail-closed 用户可见**

- 无模型、空 prompt、project_trust 读失败、workdir 逃逸：text + `--json` 都有稳定错误码。
- 参考 12-factor #9：`reference/12-factor-agents/content/factor-09-compact-errors.md`。

**WP5 — 黄金路径集成测试**

- fixture 仓（临时目录，先 `kiana trust`）。
- 跑 demo 命令或等价 cassette。
- 断言：`GOLDEN_PATH.txt`（或 patch 目标）存在；stdout JSON `harness=kiana-harness`；没有 `kiana-tools` 注册表工具名。

### 改哪些 Kiana 文件（允许集）

| 文件 | 为什么 |
|---|---|
| `kiana-entrypoints/src/harness_run.rs`、`cli.rs`（只动 run/print 分发） | 入口 |
| `kiana-entrypoints/tests/cli_run.rs`、`cli_print.rs` + 新 fixture 测试 | 证明 |
| `kiana-daemon/src/{lib,harness_capabilities,harness_sandbox,apply_patch,model_client}.rs` | 执行与隔离 |
| `kiana-runner/src/{harness,tools,model}.rs` | 模型环 |
| `kiana-core/src/lib.rs` | `start_run` / 授权 |
| `kiana-policy/src/lib.rs` | trusted 写盘决策 |
| `kiana-services/src/api/provider.rs` | 真/录制 provider |
| `kiana-commands/src/trust.rs` | 仅当 trust CLI 行为缺 |

**禁止碰：** `kiana-tools/**` 新工具、`runner.rs` 扩工具循环、`kiana-tui/**`、MCP、chrome、computer-use。

### 对照 reference（读什么、学什么、别学什么）

| 参考 | 路径 | 学 | 别学 |
|---|---|---|---|
| Codex（主对照，Apache-2.0，且 harness 已声明派生） | `reference/codex/codex-rs/cli`、`exec`、`apply-patch`、`core/src/apply_patch.rs`、`linux-sandbox`、`sandboxing`、`execpolicy`、`docs/sandbox.md`、`docs/execpolicy.md` | 两工具模型面；bwrap 档位；safe command 策略；workdir 相对路径 | 不要把 MCP/app-server/cloud-tasks 一并搬来 |
| deepseek-harness | `reference/deepseek-harness/packages/core/README.md`、`packages/core/agent-loop`、`packages/examples/agent-spine-demo`、`packages/examples/acp-demo`、`packages/shell`、`packages/sandbox` | 先可跑的 loop demo；shell 与 sandbox 分 crate | 不要抄它后期 web UI / MCP 插件矩阵 |
| aider | `reference/aider/aider/coders/base_coder.py`、`editblock_coder.py`、`diffs.py`、`commands.py`、`repo.py` | 「先 chat 通，再让 edit 真的落地」；git 感知 | 不要改用 BEFORE/AFTER 文本协议替代 apply_patch |
| 12-factor | `factor-01`、`factor-04`、`factor-08` | NL→tool call；工具是结构化输出；控制流归产品 | 不要做成 bag-of-tools |
| grok-build | `reference/grok-build/README.md`、`crates/common/xai-tool-protocol`、`xai-tool-runtime` | 工具协议与 runtime 分离（Rust 同语言） | 它是 monorepo 快照，没有从 0 历史；不要学 computer-hub |
| pi | `reference/pi/packages/agent`、`packages/coding-agent/src/cli.ts`、`packages/coding-agent/src/core` | 小 CLI 把 agent 跑起来 | TUI 渲染细节留到 Phase 4 |
| OpenHands（历史课，当前树偏前端） | 早期 commit：control loop + `docker/`；`reference/OpenHands/docker/` | 沙箱是第一周的事 | 不要把 Desktop/xterm 当 Phase 1 |
| cline | `reference/cline/sdk/packages/core/src/ClineCore.ts`、`runtime/tools/subprocess-sandbox.ts`、`tool-approval.ts` | 批准与 sandbox 生命周期 | 不要把 VSCode webview 当第一入口 |
| superpowers / OpenSpec / GSD / planning-with-files | 见 §0 | 内环 | 不是产品功能 |

### 成功标准（verify 只看这些）

1. 受信 fixture + workspace-write：文件出现，内容符合 prompt。
2. 未信任 / 无模型 / 空 prompt：非 0 退出，JSON 有稳定错误码，不写文件。
3. `--json` 含 `harness: kiana-harness`；无 Read/Edit/Grep 等 legacy 工具调用。
4. 证明级别仍是 `local_behavior`。`KIANA_HARNESS_SCRIPT` 测试降为回归。

### 明确不做

MCP、TUI、session resume、磁盘 eventlog、拆 `cli.rs`、50-tool 对等、live 环境证明、五部门编制、Symposium、六层 RAG。

---

## v0.2 Phase 2 — Session continue / cancel / 可见失败

**打开条件：** Phase 1 成功标准全绿。  
**用户可见完成：** 同一 harness 会话能继续；取消会停掉进行中的 shell/patch；provider/policy 错误在 text 和 JSON 都看得见，没有假 `completed`。

**需求：** SESS-01..03, TRUST-03  
**git：** cline 第一周 `abortTask`；Codex 第 2 天 command history；pi 2025-10 session storage，11-12 `--resume`；deepseek 先有 SessionId，后有 scripted permission 与 stable snapshots。

### 未决决策

1. Session 存储位置：进程内 + 磁盘指针（Phase 3 再把 event 做真），还是 Phase 2 就写 JSONL？推荐 **Phase 2 最小：run id 可查找 + 内存 harness run；磁盘完整收据留给 Phase 3**。对照 `reference/deepseek-harness/packages/session`（`session-persistence-jsonl` / `sqlite`）、`reference/pi/packages/session-backends`、`reference/codex/codex-rs/rollout`。
2. `kiana resume` 现有测试 `kiana-entrypoints/tests/cli_resume.rs` 是否走 legacy `events.jsonl`。必须改成 harness session 或显式标注 legacy 并让新命令用新 id。

### 工作包

**WP1 — Run/session 标识稳定**

- `kiana-runner/src/harness.rs` 的 `ActiveRun` 今天在 `Mutex<HashMap>`，进程一死就没了（Phase 3 才持久化事件，Phase 2 至少 id 可 round-trip 到 CLI）。
- CLI：`kiana run` 打印 `session_id`/`run_id`；`kiana run --continue` / `resume` 吃这个 id。
- 参考 12-factor #5 `#6` `#12`：`factor-05-unify-execution-state.md`、`factor-06-launch-pause-resume.md`、`factor-12-stateless-reducer.md`。

**WP2 — Continue 把对话接回去**

- `harness_run.rs` 的 `prompt_from_session_messages` 已有雏形。
- 不要把 continue 接到 `runner.rs`。
- 对照 pi `packages/coding-agent` 的 `--resume`（git: 2025-11-12）、deepseek `packages/session/session-projection`。

**WP3 — Cancel 真停**

- 取消必须打断 `shell.exec`（bwrap `--die-with-parent` 已在 sandbox 计划里）和未完成的 apply_patch。
- 对照 cline abort：`ClineCore.ts` + runtime orchestration；Codex exec-server 取消语义；Kiana `kiana-tui/src/cancellation` 只许当参考，不把 TUI 当完成。

**WP4 — 可见失败**

- provider 4xx/timeout、policy deny、sandbox 不可用（无 bwrap 且 fail-closed）：`status != completed`，错误码稳定。
- 参考 12-factor #9、Codex 早期「gracefully handle invalid commands」「request error details」。

### 改哪些文件

`kiana-entrypoints/src/{harness_run.rs,cli.rs,sdk.rs}`、`tests/cli_resume.rs`（重写或并列新测试）、`kiana-runner/src/{harness.rs,inbox.rs}`、`kiana-core/src/lib.rs`、`kiana-daemon/src/lib.rs`、`kiana-protocol`（若缺 cancel/resume 字段）。

### 对照 reference

| 参考 | 路径 | 学 |
|---|---|---|
| pi | `packages/coding-agent/src/cli.ts`、`packages/session-backends`、`packages/agent` | `--resume` 选择器；session 与 agent 分 package |
| deepseek | `packages/session/*`、`packages/core/session`、`scripts/session-fixture-layout.ts` | event-sourced session；fixture 可测 |
| Codex | `codex-rs/rollout`、`core/src` 里 thread/compact | rollout 是后期完整形态；Phase 2 先学 id + 续跑 |
| cline | `sdk/packages/core/src/runtime/orchestration`、`turn-queue`、`ClineCore.ts` | abort 与 turn 队列 |
| 12-factor | factor 5/6/7/9/12 | 状态统一、pause/resume、人的批准也是 tool、错误可见 |

### 成功标准

1. 第一次 run 的 id，第二次 `--continue` 能接到同一 harness run，而不是静默新开。
2. cancel 后无新的文件写入；进行中的 shell 退出。
3. 无模型/deny/超时：JSON `status` 不是 `completed`。
4. 找不到 session：失败可见（SESS-03）。

### 明确不做

完整磁盘 eventlog（Phase 3）、TUI cancel UX（Phase 4）、MCP elicitation。

---

## v0.2 Phase 3 — 磁盘收据

**打开条件：** Phase 2 绿。  
**用户可见完成：** 杀进程再开，仍能列出上次工具调用和改过的文件；第二次 run 不覆盖第一次。

**需求：** EVD-01..03  
**git：** aider 2023-05-12 transcript；Codex 2025-07-24 git metadata → rollout；deepseek 2026-08 JSONL/SQLite persistence。Kiana 现在是 `kiana-eventlog::MemoryEventLog`。

### 未决决策

1. 存储引擎：JSONL（先做，对照 deepseek `session-persistence-jsonl` + Codex rollout）vs SQLite（deepseek 后期）。**推荐 JSONL append-only。**
2. 收据 CLI 形态：`kiana run --json` 内嵌 vs `kiana evidence <id>`。现有 `kiana-commands` 若有 evidence 命令，只许接 harness 收据，不许复活旧 schema 手册。

### 工作包

**WP1 — EventStore 落地磁盘**

- 替换 `kiana-eventlog/src/lib.rs` 的纯内存实现；保留 trait。
- daemon 组装时注入磁盘 store，路径在 project 或 `~/.kiana/sessions/`。
- 参考 12-factor #5/#12；deepseek `packages/session/session-persistence-jsonl`；Codex `codex-rs/rollout`、`rollout-trace`。

**WP2 — 写入哪些事实**

- `request.accepted` / `capability.decision` / `shell.exec` / `apply_patch` 结果 / 变更文件列表 / 终态。
- `kiana-core` 已有 `append_event` 调用点，接到真 store。

**WP3 — 用户可读收据**

- `--json` 含 `files_changed`、`capabilities`、`run_id`。
- 重启后 `kiana run --resume <id> --json` 或 `evidence` 能读到同一批事实。

**WP4 — 不覆盖**

- 每次 run 新文件或只 append；禁止 truncate 上一次。
- 测试：run A 写 `A.txt`，run B 写 `B.txt`，杀进程后 A 的收据仍在。

### 改哪些文件

`kiana-eventlog/**`、`kiana-daemon/src/lib.rs`（注入）、`kiana-core/src/lib.rs`、`kiana-ports`（若 trait 不够）、`kiana-entrypoints` 输出、可选 `kiana-commands` 只读命令、新集成测试。

### 对照 reference

| 参考 | 路径 | 学 |
|---|---|---|
| deepseek | `packages/session/session-persistence-jsonl`、`session-persistence-sqlite`、`session-checkpoint-policy`、`session-projection` | 事实源 vs 投影分离 |
| Codex | `codex-rs/rollout`、`rollout-trace` | 会话可重放；git metadata 可后加 |
| aider | `aider/history.py`、早期 transcript | 人能读的记录先于完美 schema |
| 12-factor | factor 5、12 | 重放还原，不改内存对象当真相 |
| planning-with-files | `templates/progress.md`、`findings.md` | 「计划活过 /clear」的产品类比是收据活过重启 |

### 成功标准

EVD-01..03 全绿。Memory-only 不得再被 `DaemonHost::local()` 用于产品路径（测试可保留内存假 store）。

---

## v0.2 Phase 4 — TUI 接到同一 spine，或 park

**打开条件：** Phase 1 绿。可与 Phase 3 并行决策，但 **不得** 在 Phase 1 未绿时开工。  
**用户可见完成：** 要么 `kiana tui` 的黄金路径走 `DaemonHost`，要么文档+测试明确 TUI 非产品路径。

**需求：** SURF-01  
**git：** Codex TUI 开源后 9 天（`codex-rs/tui`），仍是同一 CLI 产品；pi 第二天就打 TUI 渲染质量；grok-build 开源即 TUI+harness。Kiana 现状：`kiana-entrypoints/src/tui.rs`（~4.5k）+ `kiana-tui/` 走 legacy stream。

### 未决决策（二选一，必须书面）

- **接上：** TUI 只消费 `KianaClient` / `DaemonHost` 事件，删除对 `unstable_v2_prompt_streaming` 黄金路径依赖。
- **park：** README + DESIGN 写明 `kiana tui` 非 v0.2；测试锁定「TUI 不作为 PATH 证明」。禁止再往 `kiana-tui` 堆功能冒充进度。

### 若选择接上：工作包

**WP1 — 事件流对齐**  
TUI 渲染 `RunnerEvent` / protocol 事件，不解析 legacy SDK 消息。对照 `reference/codex/codex-rs/tui`、`reference/pi/packages/tui`、`reference/grok-build/crates/codegen/xai-grok-pager*`。

**WP2 — 输入映射到同一 run API**  
用户回车 = `start_run`；Ctrl+C = Phase 2 cancel。

**WP3 — 黄金路径手测/自动**  
TUI 里触发创建文件，收据与 CLI 同一 run id。

### 对照 reference

| 参考 | 路径 | 学 |
|---|---|---|
| Codex | `codex-rs/tui`、`codex-rs/cli` | TUI 是 CLI 的全屏模式 |
| pi | `packages/tui`、`tui-plan.md`、coding-agent 的 TUI 集成 | 渲染 crate 与 agent crate 分开 |
| grok-build | `crates/codegen/xai-grok-pager-bin`、`xai-grok-pager-render` | Rust TUI 分层；同语言 |
| orca / emdash | 只当「桌面第一入口」反例 | Kiana 不在 v0.2 做 Desktop |

### 成功标准

二选一必须落地成可验证声明。不允许「TUI 很大所以算完成」。

---

## v0.3 — Trusted Workbench + 公司内核（打开前再切 GSD 计数）

**打开条件：** v0.2 四期成功标准全绿（TUI park 也算绿）。  
**目标：** 第一入口好用、可回归、可安装。**并且** 规划部 + 执行部 能交接：一次有界 Symposium（PM+Architect）产出一个 WorkPacket，独立上下文的 Builder 消费它。工具面仍是 harness，不复活 50-tool，不用 TeamCreate 当公司微信。

组织模型见 `COMPANY.md` §3–§5。v0.3 **只打开两个部门**，不是五部门。

公司工作包（与 eval/安装并行，但写盘工人必须已绿）：

**ORCH-WP1 RoleSpec + DepartmentSpec 成为派工单位**  
类型进 `kiana-domain`：`department_id`、prompt_hash、tools、sandbox、path_allow、knowledge_grants、can_convene。v0.3 至少 `planning/pm`、`planning/architect`、`executing/builder`。  
参考：`COMPANY.md` §3–§4；`reference/architect-loop/DESIGN.md` 角色表；`reference/MetaGPT/metagpt/roles/`（只学切分）；`reference/12-factor-agents/content/factor-10-small-focused-agents.md`；`reference/agency-swarm/docs/core-framework/agencies/communication-flows.mdx`（定向流，不学自由 SendMessage）。

**ORCH-WP2 独立上下文 spawn**  
派 Builder = 新 session，不复制编排器 transcript。work packet 是唯一输入。节点默认 fresh（对照 coleam00/Archon `context: fresh`）。  
参考：`reference/deepseek-harness/packages/subagent/README.md`（spawn 而非默认 fork）；`reference/pm-skills/agents/pm-workflow-orchestrator.md`（DELEGATE，无 Agent 套娃）；`reference/Archon-Knowledge/.archon/workflows/defaults/archon-idea-to-pr.yaml`。

**ORCH-WP3 PM / Architect 无写 src**  
PM 工具白名单不含 `apply_patch`；只能写 `charter/plan/packet`。Architect 只读 + 可选冲击半径工具。Policy 看 `role_id` + `department_id`。

**ORCH-WP4 冻结自由群聊总线**  
禁止把 `kiana-coordinator` 的 TeamCreate/SendMessage 接成产品。跨部门通信 = packet + eventlog。部门内讨论走 Symposium，不走自由 IM。

**ORCH-WP5 Symposium（v0.3 只做一场规划会）**  
对象：`Symposium` + 黑板（claims/votes/draft）+ `DecisionRecord`。每个发言者私有 session；Chair 发 `RequestToSpeak`；硬顶 `max_rounds`（建议 3–8）。产出必须是一个 WorkPacket，聊天不是产品。Builder 默认不列席。anti-meeting check：无权衡则跳过开会、直接异步包。  
参考：`reference/autogen/python/samples/core_distributed-group-chat/_agents.py`；`reference/pm-skills/skills/foundation-meeting-{agenda,brief,recap,synthesize}/`；`reference/pm-skills/docs/reference/skill-families/meeting-skills-contract.md`；Magentic-One 只学 progress ledger / max_stalls，不学融合成员历史。  
CrewAI：学 Crew vs Flow 分开——Symposium 是 Crew 形，部门交接是 Flow 形。ChatDev 1.0：学研讨会隐喻，不学共享 ChatChain。

### 建议切分的 Phase

**v0.3.1 Eval 对齐黄金路径**

- 做什么：fixture 仓 + cassette 回合 +「文件出现且收据可指」；可选 SWE 风格以后再说。
- 改：`kiana-entrypoints/tests/`、`scripts/` 新 `harness-golden-smoke.sh`；把 `scripts/provider-live-smoke.sh` 接到 harness。
- 参考：
  - aider `reference/aider/scripts/` 里 benchmark（git: 2023-06-23 `scripts/benchmark.py`）
  - OpenHands 早期 SWE-Bench+Docker（研究型，Kiana 不要一上来就上）
  - deepseek `packages/test-support`、session fixtures
  - pi `packages/evals`
  - GSD `reference/gsd-core/skills/gsd-add-tests/SKILL.md`

**v0.3.2 安装 / 升级 / 回滚**

- 做什么：`install.sh`、`scripts/release-smoke.sh`、`scripts/package-lifecycle-smoke.sh` 必须跑 v0.2 demo，不再验证已删 schema 手册。
- 参考：Codex 第 1 天 release script；pi 开源当天修 npm global bin；emdash 一周内 DMG（Desktop，只学「安装是功能」）；grok-build README 的 install.sh。
- 改：`install.sh`、`scripts/install-release-binary.sh`、`scripts/package-release.sh`、README。

**v0.3.3 第一入口打磨（CLI 优先）**

- 流式输出、中断提示、sandbox 档位在 help 里写清。
- 若 Phase 4 接了 TUI：只打磨黄金路径 UX（对照 pi TUI differential rendering、Codex tui mousewheel），不加 MCP 面板。
- 参考：`reference/aider/aider/io.py`、`mdstream.py`；Codex `codex-rs/tui`。

**v0.3.4 工作台最小加宽（可选，可推迟到 v0.4）**

- 仅当黄金路径反复需要：例如模型用 `shell` 做 grep 太脆，才在 **harness 工具面** 加只读 `grep`/`glob`。仍走 daemon broker。
- 参考：continue `reference/continue/core/tools/`；cline runtime tools；**不要**打开 `kiana-tools/src/registry.rs` 当完成。

### v0.3 冻结

MCP 市场、IDE、Desktop、企业、`kiana-tools` 新家族、computer-use、立项/监控/收尾三部门全编制、六层 RAG、跨部门联席会。

---

## v0.4 — Coding pack 基线（公开行为审计，不是克隆）

**打开条件：** v0.3 eval+安装绿；有一份签字的公开行为矩阵草稿。  
**目标：** 对 Claude Code **公开**核心路径做审计并实现缺口；MCP/skills 挂在 daemon 上。

### 建议切分

**v0.4.1 行为矩阵（先文档后代码）** — **已签字**

- 做什么：表列「公开行为 → Kiana 现状 → owner → 测试 → 许可证 → 是否本期」。
- 对照源（只行为，不抄源码）：
  - `reference/claude-code-rev-main/README.md`、`src/` 的 **模块名与用户可见命令**（dump，无 git）
  - `reference/claude-code-main (2)/` 同上
  - `reference/ai-coding-guide/claude-code/`、`codex/`（教程视角的公开用法）
- 反面教材：`reference/claude-code-rust`（第 1 天 v1.0.0）。
- 产出：`docs/coding-pack-matrix.md`（已落盘，不要恢复旧 104 req 语料）。验证：`.planning/phases/10-VERIFICATION.md`。

**v0.4.2 MCP client 经 daemon** — **已本地绿**

- 改：新 broker 路径或现有 crate，**禁止** `runner.rs` 接 MCP。验证：`.planning/phases/11-VERIFICATION.md`。stdio only；HTTP fail-closed。
- 参考：
  - Codex `reference/codex/codex-rs/codex-mcp`、`scripts/mcp_conformance/`（2025-05-02 才有 mcp-types）
  - deepseek `packages/mcp`（2026-07-07，loop 之后约 4 周）
  - cline `sdk/packages/core/src/extensions/mcp/`
  - continue `core/tools/mcpToolName.ts`
  - orca MCP inspector（2026-05-15，产品能跑终端之后）
- 学：发现、调用、错误、权限。别学：MCP 市场、远程 MCP SSO。

**v0.4.3 Skills / hooks**

- Kiana 已有 `kiana-skills/`。接到 harness 可见性与 trust。
- 参考：`reference/skills/`（Agent Skills 规范用法）、`reference/awesome-agent-skills`（目录不是运行时）、deepseek `packages/skill`、`packages/hooks`、pi `packages/coding-agent/src/extensions`、gstack `SKILL.md`。
- ECC / everything-claude-code：配置收藏，不是引擎。

**v0.4.4 Provider 矩阵显式降级**

- `kiana-services/src/api/provider.rs`：Anthropic / OpenAI-compatible / Ollama 能力差必须报告。
- 参考：OpenHands 后期 `feat(settings): select supported LLM providers`；pi 早期 reasoning token 跨 provider；aider `models.py`、`llm.py`。
- 12-factor #2/#3：自有 prompt 与 context，不要假装模型等价。

**v0.4.5 只读工作台工具（矩阵要才做）**

- Read/Grep/Glob 若矩阵标记核心：做成 harness 结构化工具，broker 执行，复用 sandbox。
- 对照 continue `core/tools/implementations`、aider `repomap.py`（上下文，不是 50 tool）。

### v0.4 冻结

Desktop/Web 产品循环、企业 RBAC、computer-use、chrome MCP、把 `kiana-tools` 全表接上。

---

## v0.5 — 五部门落盘 + 六层 RAG

**打开条件：** v0.4 核心路径（含 MCP 或明确延期 MCP）可用；用户已经能在 v0.2 意义上完成任务；v0.3 编排器能派独立 Builder 且规划会能出包。  
**目标：** 五个部门都是控制面对象（不是 prompt 章节）；Company/Department/Role/Project/User/Instance 六层记忆带 ACL；作者 ≠ 评审；长任务可停可续可证。组织细节见 `COMPANY.md` §3 与 §7。

**DEPT-WP1 五部门对象**  
`DepartmentSpec`：mission、default_roles、artifacts glob、symposium_policy、gates。立项/规划/执行/监控/收尾同时存在，不是线性五步。跨部门只交 WorkPacket；联席会是显式 `Symposium.joint`。  
参考：`COMPANY.md` §3；pm-skills `_workflows/`；coleam00/Archon DAG（过程软件化，不当编制）。

**MEM-WP1 六层集合**  
- Company：剧本、组织图、过程模板  
- Department：该部门决议与教训  
- Role：工艺（pm vs builder vs reviewer）  
- Project：repo map（先接 `kiana-query`）、事件、ADR、任务  
- User：偏好、跨项目约束（MemPalace 形）  
- Instance scratch：随 session 死，默认不晋升  
禁止混库。聊天不得自动入库；晋升必须显式 `memory.write`。  
参考：`reference/memorix/README.md`；`reference/MemPalace/README.md`；后期 graphify / GitNexus；coleam00/Archon `archive/v1-task-management-rag` 只作知识引擎形状。`reference/letta` 当前是落地页，勿当源码。

**MEM-WP2 检索是工具**  
`memory.search` / `memory.write` 走 daemon broker，请求带 `role_id` + `department_id`。命中写入收据。无来源不得当 Reviewer 的已验证结论。  
12-factor #4/#7。不要静默灌进系统提示。

**MEM-WP3 ACL**  
密级：public / company / department:* / role:* / project / packet / user-private / scratch。Builder 默认无 user-private、无 planning 未发布辩论。PM 可读 user:prefs。Librarian 是默认跨层写入者。  
Policy 扩 `RequestContext.{role_id,department_id}`。

**PMP-WP1 部门工件**  
立项 charter → 规划 packets → 执行 builder → 监控 gates/reviewer → 收尾 lessons 入库。小任务允许软件跳过开会，但留下短 charter。  
参考：`reference/pm-skills/_workflows/`、`deliver-*`、`iterate-lessons-log`；OpenSpec/GSD/planning-with-files 落盘形状。

**SYMP-WP1 五部门可开会**  
v0.3 的单场规划会推广到各部门；跨部门联席必须点名、限时、有 anti-meeting check。决议进**该部门 RAG**。  
参考：meeting family；autogen RequestToSpeak；Agency Swarm 定向流。

**REV-WP1 独立评审**  
Reviewer 新 session、只读、不是作者。architect-loop cohesion review。Schr0d/Archon `diff` 可作为监控部确定性门。

### 建议切分

**v0.5.1 Compact 与上下文所有权**

- `kiana-runner/src/compact.rs` 做真；预算可见。
- 参考：12-factor #3；Codex `core/src/compact.rs` 一族；deepseek `packages/compaction`、`packages/context`；grok `xai-grok-compaction`；continue `core/context`。

**v0.5.2 工作流 / 计划落盘（产品功能）**

- 让 `kiana-workflow` 不再是空名：用户可见的计划、关卡、恢复。
- 参考（方法 → 产品）：
  - OpenSpec 全套 `skills/openspec-*`
  - GSD `skills/gsd-plan-phase`、`gsd-execute-phase`、`gsd-verify-work`
  - planning-with-files 三文件
  - architect-loop `DESIGN.md` + `skills/`（orchestrator 持证据，builder 一次一个 issue）
  - pm-skills（产品决策与建造分离）
  - deepseek `packages/plan`、`packages/todo`、`packages/workflow`、`packages/goal`
- **不要**恢复 24-phase / Project OS 语料。用现在的 harness 收据做事实源。

**v0.5.3 六层记忆 / 仓库图**

- Company/Department/Role 集合先于向量炫技：先文件/JSONL 分区，图数据库后期。
- 仅当上下文反复炸：MemPalace / graphify / GitNexus / memorix。
- `reference/MemPalace`、`graphify`、`GitNexus`、`memorix` 都是后期乘法。浅克隆的不要编造历史。
- Schr0d/Archon 冲击半径 JSON 写入 Project RAG，不是另起记忆产品。
- Kiana 已有 `kiana-query`：先接 MemoryBroker，再考虑新图数据库。

**v0.5.4 子 agent / 并行（谨慎）**

- 12-factor #10 小而专注。architect-loop、deepseek `packages/subagent`、cline hub 后期。
- 没有路径锁和收据之前不要 swarm。旧 `kiana-tasks/swarm` 不是完成证明。

---

## v0.6 — 多入口同一核

**打开条件：** v0.5 resume 真能用。  
**顺序（git 共识 + Kiana 已选 CLI）：** SDK/print 彻底同一核 → IDE → Desktop/Web。

### 建议切分

**v0.6.1 Headless SDK / RPC**

- print/SDK/app-server 全部 `DaemonHost`。`runner.rs` 不再被产品路径调用。
- 参考：12-factor #11；Codex 2025-09-30 拆 `mcp-server` 与 `app-server`（`codex-rs/app-server*`）；deepseek `packages/sdk`、`packages/acp`、`apps/cli`；pi `packages/protocol`、`packages/server`；continue `core/protocol`。

**v0.6.2 IDE**

- 新入口只做 client。对照 continue `core/` + 扩展；cline / Roo-Code（Roo 与 cline 同源 IDE 路线）。
- 不要复制 runner。

**v0.6.3 Desktop / Web**

- 对照 OpenHands（当前树偏 Electron/前端）、orca（终端工作区）、emdash（Electron + worktree + 包一层 Codex CLI）。
- emdash 的教训：它可以是 **壳**，内核仍是 agent CLI。Kiana Desktop 若做，必须包 `DaemonHost`，不是再写一个循环。
- herdr：多 agent 终端复用，是工作区产品，不是 Kiana 内核。

**v0.6.4 执行部并行 swarm（可与 6.1 并行决策，但路径锁必须先绿）**

- 做什么：多个 Builder，一人一包一 worktree；冲突路径串行；编排器是软件。
- 主教材：`reference/ruflo/plugins/ruflo-swarm/README.md`（hierarchical、max 6–8、specialized、worktree、Monitor）。
- 实现对照：`reference/ruflo/v3/@claude-flow/swarm/src/message-bus.ts`（有界 retry）、`src/workers/worker-dispatch.ts`、`src/topology-manager.ts`。
- 本仓形状：`kiana-tasks/src/swarm.rs` 的 WorkPacket / path_locks / `.kiana/swarm-worktrees/{dispatch}/{task}`。把它做真接到 `KianaHarness`，不要当已完成。
- 也对照：architect-loop 一人一 issue 一 worktree；coleam00/Archon 每 run 一个 worktree。
- **不要：** ruflo Queen 当 LLM 编排器；Raft/BFT/Gossip；TeamCreate/SendMessage；默认 15–100 agent；共享 swarm memory 打穿 ACL。
- 需求：现有 SURF2 之外加并行执行，打开时再赋 ID，不进 v0.2。

**v0.6.5 远程执行雏形**

- 已有 `kiana-remote` / `kiana-bridge`。只在 SDK 同核之后接。
- Codex `cloud-tasks*` 是 v1.x 级，不要提前。

---

## v1.0 — 完整个人产品

**打开条件：** v0.6 至少 SDK 同核；安装升级过关；Coding pack 核心路径（不是工具数 100%）有矩阵证据。

### 必须做的事

| 工作 | Kiana 落点 | 参考 |
|---|---|---|
| 安装/升级/回滚/恢复 | `install.sh`、`scripts/package-*`、`scripts/release-*` | Codex release、pi npm、grok install、OpenHands docker |
| 文档与支持 | README、真用户手册（不要 104 req） | aider 文档演进、deepseek 双语文档、ai-coding-guide |
| 安全默认 | policy + sandbox fail-closed | Codex sandbox.md、12-factor #7 |
| Coding pack 核心路径 | v0.4 矩阵的 P0 全绿 | 公开行为，不是 dump 源码 |
| Daily/Research | **仅当**同一核已有一条黄金路径 | 否则保持草案；不要为 pack 复制 runner |
| 许可与供应链 | `deny.toml`、NOTICE | grok `THIRD-PARTY-NOTICES`、deepseek `THIRD_PARTY_NOTICES.md` |

v1.0 **不是** 企业、不是 38-reference 打勾、不是 TUI 像素对等 Claude Code。

---

## v1.x — 团队 / 云 / 企业（最后）

**打开条件：** v1.0 个人产品已发布。  
**git：** Codex `codex apply` 远程 2025-07-11；cline hub drain/upgrade 2026-08；OpenHands 云/helm 更晚。

### 做什么

- 租户、RBAC、审计、数据保留隔离（`kiana-capability-governance-supervisor` 旧治理代码可作原料，**不是**完成定义）。
- 官方云 / 自托管远程执行。
- SSO、托管策略：环境变量骨架已有 `KIANA_MANAGED_*`，产品化在这一站。
- 对照 Codex `cloud-config`、`cloud-tasks*`；cline `sdk/packages/core/src/hub`；OpenHands `helm/`。

个人开源版不掺企业契约。

---

## 附录 A — 按主题找 reference

| 主题 | 先读 |
|---|---|
| Agent loop | deepseek `packages/core/agent-loop`、Codex `codex-rs/core`、pi `packages/agent`、12-factor 1/4/8 |
| Shell + patch | Codex `apply-patch`、`exec`、`core/src/apply_patch.rs`；Kiana `kiana-daemon/apply_patch.rs`、`kiana-runner/tools.rs` |
| Sandbox | Codex `linux-sandbox`、`sandboxing`、`execpolicy`、`docs/sandbox.md`；deepseek `packages/sandbox`、`packages/shell`；Kiana `harness_sandbox.rs` |
| Trust / 批准 | Kiana `kiana-commands/src/trust.rs`、`kiana-policy`；cline `tool-approval.ts`；12-factor #7 |
| Session / resume | deepseek `packages/session/*`；pi `session-backends`；Codex `rollout`；12-factor 5/6/12 |
| 收据 / 重放 | Codex `rollout`；deepseek session-persistence；aider `history.py` |
| TUI | Codex `tui`；pi `packages/tui`；grok `xai-grok-pager*` |
| Eval | aider `scripts/benchmark.py`；pi `packages/evals`；GSD add-tests |
| MCP | Codex `codex-mcp` + mcp_conformance；deepseek `packages/mcp`；cline `extensions/mcp` |
| Skills | `reference/skills`、deepseek `packages/skill`、pi extensions |
| 安装发布 | Codex 早期 release script；pi package.json bin；`install.sh` |
| IDE | continue `core/`；cline `ClineCore.ts`；Roo-Code |
| Desktop 壳 | orca、emdash、OpenHands electron |
| 长任务方法 | GSD、OpenSpec、superpowers、planning-with-files、architect-loop、pm-skills |
| 过程引擎 | coleam00/Archon（`reference/Archon-Knowledge`）YAML + fresh_context + 人门 |
| 并行 swarm | ruflo-swarm：worktree、层次拓扑、6–8 上限、MessageBus 有界 retry；本仓 swarm.rs 包形状 |
| 冲击半径 | Schr0d/Archon（`reference/Archon`）`analyze --format agent` / `diff` |
| 组织/会议 | Agency Swarm communication_flows；ChatDev 1.0 公司隐喻；pm-skills meeting family；autogen RequestToSpeak |
| 分层记忆 | MemPalace wings/rooms；memorix Git Memory；kiana-query |
| 反模式 | claude-code-rust；langchain/autogen 无界群聊当骨架；ChatDev 共享 ChatChain；MetaGPT 全员广播；本仓已删 24-phase；letta 落地页当源码 |

## 附录 B — 现在立刻不要打开的目录

`kiana-tools/src/` 新工具、`kiana-entrypoints/src/runner.rs` 扩循环、`kiana-chrome-mcp`、`kiana-computer-*`、`kiana-url-handler`、`reference/claude-code-rev-main/src` 当实现源、v0.5+ 需求 ID。v0.4.3 只打开 CODE-03。

## 附录 C — 下一动作

当前计数：v0.4 Phase 3 / CODE-02 stdio MCP 已本地绿。  
下一命令：打开 **v0.4.3 CODE-03**（skills/hooks 接到 harness）。对照矩阵 §3 / §6。不要同时开五部门、TUI、Read/Grep/Glob、provider live 矩阵。
