# AGENTS.md — Kiana 工作约定

本文件是 coding agent 在本仓库工作的常驻约束。**先读完再动手。**

Kiana 是一个本地优先的受控 coding-agent 运行时（Rust workspace）。它的核心主张不是
"工具多"，而是：每次执行都经过授权、每个动作都留下可核验的记录、拿不准就拒绝。
在这个仓库里工作，必须遵守同一套纪律。

新人先读 [`docs/company-os-overview.md`](docs/company-os-overview.md)（白话总览：
心智模型、任务走读、术语词典、FAQ）。

---

## 1. 事实权威顺序（冲突时用它裁决）

```text
1. 源码 + 精确测试回执 + CURRENT_STATUS.md
2. README.md / USER.md（当前用户面）
3. docs/company-os-security-constitution.md（安全宪法）
4. CompanyOS 领域与平台规范（docs/company-os-*.md）
5. 实施大纲 / PHASES.md / PROCESS.md / DESIGN.md
6. reference/ 与 docs/reference-agent-audit/（只是审计材料）
```

**关键区分**：`docs/` 下除 `company-os-overview.md` 外，几乎全是"规范目标
（Normative Target）"——写的是系统**应该成为什么**，不是**现在是什么**。
不要因为规范里定义了某个对象，就认为它已实现或已强制。

"现在真的做到哪了"只有一个来源：[`CURRENT_STATUS.md`](CURRENT_STATUS.md)。

发现文档与代码冲突时：**报告冲突，不要改规范去迁就实现，也不要放宽测试消除冲突。**

---

## 2. 真实命令

根 `package.json` 有依赖但**没有 scripts 段**。`npm run build` / `npm test`
不是本仓库的命令，任何提到它们的说明都已过时。

```bash
# 构建与静态检查
cargo build --bin kiana
cargo check --workspace --locked --offline
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline

# 测试
cargo test --workspace --locked --offline --no-fail-fast
cargo test -p <crate> --test <target> --locked --offline -- --test-threads=1
# daemon / control-plane 相关测试必须串行；并行会产生假失败

# 发布门与冒烟
bash scripts/release-smoke.sh
bash scripts/harness-golden-smoke.sh
bash scripts/v10-workbench-smoke.sh
bash scripts/v10-p0-closeout-smoke.sh

# 可选 Electron 壳
npm install && node --test contrib/desktop/tests/*.js
```

无网络环境优先加 `--locked --offline`。Make 目标见 `Makefile`；示例以
`USER.md` 和 `scripts/` 为准，`run.sh` 可能已过时。

---

## 3. 产品主路径（唯一执行脊柱）

```text
kiana-entrypoints
  → kiana-client / kiana-protocol（versioned wire envelope）
  → kiana-daemon::DaemonHost（唯一组合根）
  → kiana-core::ControlPlane（授权与生命周期权威）
  → kiana-policy + kiana-gates + approval
  → kiana-capability-broker / kiana-runner::KianaHarness
  → handlers → EventStore → Receipt
```

**不得新增第二条执行循环。** CLI、Workbench、Web、Desktop 都必须复用同一个
`DaemonHost`；不要在 entrypoint、adapter、workflow 或 UI 里另起模型循环或权限判断。

模型可见工具面**锁死为五个**：`shell`、`apply_patch`、`mcp`、`memory.search`、
`memory.write`（见 `kiana-runner/src/tools.rs`）。搜索文件用 `shell` 跑 `rg`/`ls`/`cat`，
不要新增 Read/Grep/Glob 类工具（`coding-pack-matrix.md` 的 P1-READ 已明确跳过）。
**工具调用不直接执行**：它转成 CapabilityRequest 回到 ControlPlane 过审，由 Broker 执行。

---

## 4. Workspace 布局

### 产品路径 crate（新能力放这里）

| crate | 职责 |
|---|---|
| `kiana-domain` | 稳定 ID、值对象、角色/部门、WorkPacket、状态与不变量 |
| `kiana-protocol` | versioned wire DTO、命令、错误、事件载荷（`kiana.protocol.v1`） |
| `kiana-ports` | core 与 adapter 之间的接口 |
| `kiana-core` | ControlPlane：authority chain、policy/gate/approval、生命周期、fencing、事件 |
| `kiana-daemon` | 组合根 DaemonHost：目录、调度、投影、本地 adapter |
| `kiana-runner` | 规范 Agent loop（KianaHarness）、inbox/continue、compaction、工具 schema |
| `kiana-capability-broker` | capability descriptor、dispatch、结果边界 |
| `kiana-policy` / `kiana-gates` | 策略矩阵与关卡/审批映射 |
| `kiana-eventlog` | 事件事实、CAS、恢复接口、Receipt 投影 |
| `kiana-query` | 上下文索引、repo map、搜索、Memory、context pack |
| `kiana-workflow` | 确定性流程定义（无绕过 ControlPlane 的执行权） |
| `kiana-entrypoints` | CLI、workbench、web、architecture、MCP 等入口路由 |
| `kiana-client` | 本地进程内协议传输 |

### 兼容边界（**不要**把新能力放进去）

`kiana-tools`、`kiana-commands`、`kiana-tasks`、`kiana-types`、`kiana-services`、
`kiana-bridge`、`kiana-remote`、旧 SDK/runner。

它们是历史遗留的第二套执行面，只保留兼容性。**旧 AGENTS.md 说的"新命令注册到
`kiana-commands/src/registry.rs`、新工具注册到 `kiana-tools/src/registry.rs`"
对新 CompanyOS 能力已不适用**——那条路会绕过 ControlPlane。

### 其余

`kiana-chrome-mcp`、`kiana-computer-mcp`：feature-gated，非产品路径。
`legacy/`：已从 workspace 移除的旧 crate（当前：`kiana-computer-input`、`kiana-coordinator`、
`kiana-ink`、`kiana-color-diff`、`kiana-screen-capture`、`kiana-tui`、`kiana-components`）；
不参与编译，只保留历史。
`kiana-screens`/`kiana-modifiers`：TUI 相关，已 park。
`contrib/desktop/`：Electron 壳（Node）。`reference/`：只读审计输入，非 workspace 成员。

---

## 5. 架构约束

- **依赖方向**：`kiana-domain` / `kiana-protocol` / `kiana-ports` 在最底层，不得依赖
  上层。`kiana-core` 不得依赖 `kiana-query` / `kiana-types`（已通过
  `kiana-core/tests/dependency_boundaries.rs` 强制）。共享契约先下沉再暴露。
- **Trust 边界**：项目 `.claude/skills`、`.kiana/plugins` 等项目本地资源必须先过
  `ProjectTrust` 检查再加载；用户级与 `KIANA_HOME` 级是不同的信任范围。
- **文件系统权威**：写集由 WorkPacket、PathLock 和 sandbox profile 决定；
  不要假设本地 `.kiana` 路径，用现有的路径解析器。
- **异步**：模型、MCP、网络、长 I/O 走 Tokio；纯状态/文件 helper 保持同步且可确定性测试。
- **错误处理**：入口/命令层用 `anyhow::Result` + `Context`；可复用库暴露结构化错误和
  稳定错误码。策略拒绝必须返回结构化原因，不要塌缩成通用 I/O 错误。
- **全局状态**：优先显式上下文或不可变快照，不要新增可变全局。

---

## 6. 反模式

- **绕过 ControlPlane**：从 entrypoint、UI、MCP、workflow 或 artifact 直接执行副作用。
- **第二个事实源**：把 transcript、UI timeline、缓存或模型自述当作状态权威。
  Transcript 是可丢弃的展示视图，EventLog 才是事实。
- **未过 trust 就加载项目资源**：项目本地 skill/plugin/hook/MCP 配置可以注入指令和能力。
- **发明入口专属事件形状**：要扩展就扩 `RuntimeEvent` 并同步所有 adapter。
- **把状态写成比证据更强**：`target` 写成 `implemented`、一次本地通过说成 `durable`/`live`。
- **靠放宽断言让 CI 变绿**：删测试、`#[ignore]`、改期望值。
- **权限并集**：子 Cell 权限只能是父级、模板、部门、项目、packet、approval 的**交集**。

---

## 7. 冻结项与禁止事项

冻结清单以 [`docs/coding-pack-matrix.md`](docs/coding-pack-matrix.md) §5 为准，常踩的有：

- **不许拆 `kiana-entrypoints/src/cli.rs`**（约 25k 行）——FZ-CLI 明确冻结，
  只在它真的挡住主路径时才拆，且需要先说明。
- **不许把 `kiana-tools` 的 50+ 工具接到 harness**（FZ-TOOLS）。
- **不许实现 TeamCreate / SendMessage 自由消息总线**（FZ-TEAM，永久）。
- **不许打开** HTTP MCP（当前返回 `mcp_transport_unsupported`）、live provider、
  token streaming、支付/外卖/打车/订票/IoT、企业租户与远程执行。
- **不许让 Builder 列席规划会或监控会**；这两条 attendee 边界已冻结。
- **不许把 `reference/claude-code-rev-main`、`claude-code-main` 的源码复制进来**——
  前者许可证不明，后者是 Anthropic 专有；只允许模块名与公开行为对照。
- **不许自动 commit / push / merge / rebase / 删除 worktree**，除非用户明确授权。
- **不许改 `.gitignore` 去提交** `.codex/`、`.mcp.json`、`.env` 或任何密钥。

---

## 8. 工作方式与证据纪律

**一次只推进一个最小可验证切片。** 顺序固定：

```text
复现 → 分类（基线遗留 / 本次相关）→ 最小修复 → 聚焦测试 → 回归 → 证据
```

**先证明拒绝路径，再证明成功路径。** 任何新能力都要先覆盖 deny、越权、过期审批、
取消、`result_unknown`、重放、TOCTOU、注入和恢复，再谈 happy path。

每次改变状态声明，必须产出这个证据块（格式见 `CURRENT_STATUS.md` §6）：

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations（必须写）/ reviewer
```

两个维度不能混：

```text
feature_status: implemented | partial | target | deferred | not_supported
proof_level:    source | local_behavior | durable | live | physical
```

永远不成立的推断：有类型 ≠ 已强制；单测通过 ≠ local_behavior；
local_behavior ≠ durable；Receipt 存在 ≠ 现实结果正确；参考项目有实现 ≠ Kiana 已实现。

---

## 9. 并发与授权

- 一个 worktree 只许一个写者；只读研究 agent 可共享 checkout，写者不可。
- 子任务只能缩减能力，不能新增工具、服务器、命名空间、网络、支出、并发或委派深度。
- 只有集成负责人修改共享 manifest 和 lockfile（`Cargo.toml`、`Cargo.lock`、`package.json`）。
- 策略拒绝或依赖失败时，取消依赖它的以及尚未开始的兄弟任务。
- 每个有后果的动作都要产生决策回执；破坏性、支出和发布动作需要人工批准。

---

## 10. 卡住时怎么办

遇到以下任一情况：需要架构决策、需要放宽某条安全约束、需要触碰第 7 节的冻结项、
或修复方案会引入第二条执行路径——**停下来**，写清失败现象、根因、两三个候选方案
及取舍，等用户决定。不要自己选一条绕过约束的路。

---

## 相关文档

- [`CLAUDE.md`](CLAUDE.md) — 架构约束与命令（与本文件同源，更详细）
- [`CURRENT_STATUS.md`](CURRENT_STATUS.md) — 当前状态账本（唯一现状来源）
- [`USER.md`](USER.md) — 可运行命令
- [`docs/company-os-overview.md`](docs/company-os-overview.md) — 白话总览与术语词典
- [`docs/README.md`](docs/README.md) — 文档地图与阅读路径
- [`docs/company-os-implementation-outline.md`](docs/company-os-implementation-outline.md) — 工程切片与 Gate 0
