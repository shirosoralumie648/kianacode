# Kiana

## What This Is

Kiana 是一个面向高级个人用户、公开发行用户、团队和企业的完整 AI Agent 产品。它以同一个本地优先的 `Kiana Core` 支撑 Coding、Academic Research 和 Daily Work 三个能力包，并通过 CLI/TUI、Headless SDK/RPC/MCP、IDE、Desktop、Web/App Server 提供一致的任务执行体验。

**当前执行不是这个完整 1.0 画面。** 当前要把已经接线的本地 CLI/TUI agent 收成一条能跑通的黄金路径，再逐步扩展成“什么都能干”的助手。完整产品定义仍是北星，见 Current Milestone。

产品以 Claude Code 的公开功能覆盖作为 Coding 基线，以 Claude Desktop 的桌面工作区和连接器体验作为桌面基线；同时逐项审计 `reference/` 中的项目，形成经过许可证、安全和产品治理的能力超集。个人核心采用 Open Core 模式开源，官方云与企业自托管版提供同步、远程执行、团队协作和治理能力。

## Core Value

Kiana 必须在覆盖 Claude Code 公开核心能力的基础上，更可靠地完成真实长任务，并用可验证证据和可恢复状态证明任务确实完成。

## Current Milestone

**v0.2 Runnable Local Agent MVP**（2026-08-22 重切）

先做一个能跑通的本地 coding agent，再逐渐做成更通用的 agents 助手。剩余工作改为纵向切片，不再继续执行原 Phase 3–24 横切地基（契约 → 状态 → 策略 → RuntimeHost → 三包 → 全入口 → 云/企业）。

本里程碑成功标准：用户在已信任仓库上用 `kiana -p` / `kiana run` / `kiana tui`、一个真实 provider、read/edit/shell 完成任务；trust/policy fail-closed；失败可见；跑完能指出工具与文件改动证据。只声称 `local_behavior`。

明确移出本里程碑：IDE/Desktop/Web 产品闭环、Research/Daily pack、官方云、企业自托管、38-reference 产品完成、签名 `dist/`、1.0 措辞。旧设计文档已删除，不再作为执行依据。

Phase 1 保持已完成。Phase 2 保持人审门禁，不把 local verification 升级为关闭，也不用它挡住 Phase 3。

## Business Context

- **Customer**: 高级个人开发者与研究者、公开发行用户、协作团队，以及需要私有部署和集中治理的企业
- **Revenue model**: 本地个人核心开源；官方云按同步、远程执行和团队能力收费；企业版通过自托管许可、治理能力、支持和服务商业化
- **Success metric (current)**: 一条本地黄金路径在 CLI/TUI 上对真实仓库 + 真实 provider 可跑通，失败可见，并留下工具/改动证据
- **Success metric (north star)**: Coding、Academic Research、Daily Work、全部产品入口、官方云和企业自托管版共同通过 1.0 发布门禁；1.0 不是 v0.2 的完成标准
- **Strategy notes**: 当前执行见 `docs/planning-current.md` 和 `.planning/ROADMAP.md`。Phase 1 治理账本仍在 `docs/agent-program/` 与 `docs/reference-feature-matrix.md`。

## Requirements

### Validated

- ✓ Rust 2021 多 crate 工作区与 `kiana` 主二进制已经建立 — existing
- ✓ CLI、REPL、TUI、SDK、MCP、remote/bridge 和 native integration 入口已经存在 — existing
- ✓ `CommandRegistry`、`ToolRegistry`、模型工具循环和统一 `RuntimeEvent` 契约已经形成 — existing
- ✓ Anthropic、OpenAI-compatible、Ollama 和测试 Provider 的基础适配已经存在 — existing
- ✓ 任务板、持久化 workflow DAG、追加式 EventLog、evidence/integrity 与 bounded swarm 基础已经存在 — existing
- ✓ 项目信任、权限策略、skills、plugins、hooks 和 MCP 的基础加载边界已经存在 — existing
- ✓ schema contract、release smoke、package lifecycle 和商业发布 blocker 报告基础已经存在 — existing
- ✓ Chrome、computer-use、screen capture、URL handler 和远程会话等集成基础已经存在 — existing

### Active

本里程碑（v0.2）只把这些当作正在建造的假设，直到有 local_behavior 验证：

- [ ] 用户能用 `kiana -p` / `kiana run` 在已信任仓库上跑通一次真实 provider + read/edit/shell 任务
- [ ] 用户能用 `kiana tui` 跑通同一条黄金路径
- [ ] 缺 provider/auth/trust 时失败可见，不假装成功
- [ ] 用户能恢复/继续最近会话，并能取消进行中的 run
- [ ] 未信任项目在写/执行前 fail-closed；permission profile 拒绝可见
- [ ] 跑完后能检查工具调用、文件改动，并在重启后再次打开收据

### North Star (later milestones, not current active)

- [ ] Coding Pack 覆盖 Claude Code 的公开核心功能，但允许 Kiana 保留自己的命令、配置和交互设计
- [ ] Kiana Core 为所有入口和能力包提供统一 runtime、session、provider、tool、policy、memory、workflow、evidence 与 recovery 契约
- [ ] 首个完整版本同时支持 Anthropic、OpenAI、Gemini、OpenRouter、OpenAI-compatible 和本地模型，并对 Provider 能力差异进行显式降级
- [ ] CLI/TUI、Headless SDK/RPC/MCP、IDE、Desktop 和 Web/App Server 使用同一事件流与持久化任务状态
- [ ] Coding Pack 完成代码库理解、编辑、Git 安全、diff/undo、测试修复、MCP、扩展、多 Agent、后台与远程编码工作流
- [ ] Academic Research Pack 完成文献、引用、证据图谱、研究计划、实验、统计、论文、复现包和领域研究扩展
- [ ] Daily Work Pack 完成个人效率、办公协作、浏览器/桌面自动化、项目工作流、多 Agent、审批、审计、报告和团队协作
- [ ] 所有长任务通过持久化 workflow、证据账本、验收 verifier、故障恢复和结果未知状态可靠推进
- [ ] 用户首次启动选择安全、平衡或自治档位，并可按项目覆盖；任何档位都不能绕过高风险硬策略
- [ ] 本地个人版无需账户即可完整使用，代码、会话、记忆和凭据默认留在设备；云同步和远程执行显式启用并加密
- [ ] Linux 和 macOS 提供原生交付，Windows 首个完整版本通过 WSL 支持
- [ ] 38 个 `reference/` 目录的能力逐项进入 `Adopt / Adapt / Reject` 矩阵，并绑定许可证、实现、测试、风险和发布证据
- [ ] 开源个人版、官方云和企业自托管版共同达到稳定安装、升级、回滚、恢复、安全、运维和支持门禁
- [ ] Alpha/Beta 持续按垂直能力发布并收集反馈，只有全部 1.0 门禁通过后才使用“完整”或“生产就绪”表述

### Out of Scope

- 继续把原 Phase 3–24 横切地基当作当前执行主线 — 2026-08-22 起剩余工作改为纵向 MVP
- 在 v0.2 声称 IDE/Desktop/Web、云、企业、Research/Daily 或 1.0 完成 — 那些是后续列车
- 复制 Claude Code、Claude Desktop 或其他专有产品的非公开源码、品牌和受保护素材 — 只复刻公开可观察行为并独立实现
- 为追求“全抄”而机械移植重复、废弃、不安全、许可证不兼容或仅为 stub/mock 的参考实现 — 每项拒绝必须在能力矩阵中说明理由
- 以命令、页面、类型、测试桩或文档存在作为功能完成证据 — 完成必须绑定真实行为、验证和发布证据
- 在首个完整版本中交付原生 Windows 客户端 — 首版通过 WSL 支持，原生 Windows 在后续版本补齐
- 让本地个人核心依赖强制账户、官方云或企业控制面才能工作 — Local-first 是不可退让的产品边界
- 让模型编造论文来源、实验数据、外部回执或任务完成状态 — 无证据时必须返工、阻塞或等待人工确认
- 允许自治模式绕过删除、发布、付款、凭据、消息发送和其他高风险外部写入审批 — 硬策略对全部自主权档位生效

## Context

- 当前仓库是已有大量实现和文档的 brownfield Rust monorepo，而不是从零创建的新项目。
- `.planning/codebase/` 已在 2026-07-13 完成七份代码库映射；该映射是现有能力和架构边界的初始事实来源。
- 当前架构已经具备入口层、命令层、Assistant runtime、tool/workbench、workflow/project OS、context/hooks、extension/trust 等分层，应在这些边界上演进而不是另起一套运行时。
- `reference/` 当前包含 38 个项目目录，覆盖 coding agents、agent runtimes、planning/workflow、memory/knowledge、multi-agent、IDE、app-server、research 和扩展生态。
- 2026-08-22：用户明确要求先有能跑通的 MVP，再扩展成通用 agents 助手。原 24 阶段设计/规划文档已删除；Phase 1/2 执行记录保留。
- 当前产品已经能编译并接线 REPL / `kiana -p` / `kiana run` / `kiana tui`、多 provider 和工具循环；缺口是把它们收成可靠黄金路径，而不是再铺一层未使用的横切抽象。
- `kiana architecture status --json` 仍报告 `legacy_edges_remaining: 9`。这是架构事实，不是 v0.2 完成障碍的替代指标。
- v0.2 把已接线的本地 agent 收成可演示黄金路径，而不是先做完 schema/RuntimeHost 再谈用户任务。
- 完整 1.0 仍覆盖 Coding、Research、Daily、Cloud 和 Enterprise；那是后续里程碑，不是本里程碑范围。
- Coding 仍是最高投入的能力包；v0.2 只取 coding 黄金路径，不取完整公开基线。
- 用户改用 Kiana 的首要理由不是界面克隆，而是长任务、多 Agent、证据链、验证和恢复带来的更可靠任务完成能力。
- 现有工作区含大量未提交实现与设计成果；规划提交必须保持原子性，不能将无关修改混入 GSD 文档提交。

## Constraints

- **Tech stack**: 延续 Rust 2021、Tokio、Cargo workspace 和既有 crate 分层；共享契约先进入 `kiana-types` 或对应低层 crate
- **Architecture**: 所有产品入口必须消费同一 runtime、session、tool、policy 和 event contracts，禁止为 Desktop、Web 或能力包复制核心逻辑
- **Providers**: 首版即多供应商；能力不一致时必须显式报告、路由或降级，不能静默假装等价
- **Platforms**: Linux/macOS 原生，Windows/WSL；平台沙箱、路径、终端和安装差异必须进入测试矩阵
- **Data**: Local-first、无需账户、凭据进入系统 Keychain 或服务端 secret vault；同步和遥测均为显式启用
- **Safety**: ProjectTrust、权限档位、网络策略、沙箱和外部副作用审批必须 fail closed
- **Research integrity**: 所有结论、引用、数据、实验和图表保留 provenance；无法验证的内容不得进入最终结论
- **Licensing**: 开源源码仅在许可证兼容时复用；专有产品只进行公开行为层的 clean-room 式独立实现
- **Release quality**: 安装、升级、回滚、恢复、安全、供应链、性能、文档和支持都是功能完成的一部分
- **Commercial delivery**: 个人开源版、官方云和企业自托管版共享核心契约，但租户、RBAC、审计和数据保留边界必须隔离
- **Scope control**: `reference/` 覆盖通过能力矩阵治理；没有矩阵条目、owner、测试和证据的能力不能进入完成统计

## Planning Execution Rule

- **全项目不采用 TDD**：任何新计划、重规划或实现任务不得要求测试先于实现、RED→GREEN 循环、失败测试驱动开发，或把 `tdd=true` 作为任务前置条件。
- 实施顺序统一为：先按已批准的接口、状态机和安全合同完成最小实现，再运行 focused tests、adversarial checks、integration gates 和 review；验证可以覆盖正常、异常、边界和回归场景，但不改变实现先行的顺序。
- 历史计划、总结和研究文档中的 TDD/RED/GREEN 文字仅保留为历史证据，不得作为当前执行要求；后续维护这些文档时不得新增 TDD 依赖。
- 当前 GSD 配置中的 `tdd_mode: false` 是本项目配置依据。任何技能或模板默认要求 TDD 时，以本项目规则和用户指令为准。

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| 全项目规划与实现不采用 TDD，统一采用合同实现后验证 | 用户明确要求取消测试先行，同时保留 focused、adversarial、integration 和 review 的质量门禁 | — Locked |
| 剩余工作改为纵向可跑通 MVP（v0.2 Phase 3–6），而不是继续原 Phase 3–24 横切地基 | 用户要求先有能跑通的本地 agent，再扩展成通用助手；横切会把第一条黄金路径推到 Phase 10 | — Locked 2026-08-22 |
| 采用“能力与体验对齐 + 最大化合法移植”的组合策略 | 既覆盖目标能力，又避免专有源码复制和许可证风险 | — Pending |
| Coding 以 Claude Code 公开功能覆盖为基线，但不追求命令或配置的直接兼容 | 用户要的是完整能力，Kiana 需要保留可演进的独立产品设计 | — Pending |
| “更可靠地完成任务”是相对 Claude Code 的首要差异化 | 长任务、验证、证据和恢复比外观克隆更能形成长期价值 | — Pending |
| 采用稳定 Kiana Core、三个能力包和多端外壳 | 与现有 Rust 分层一致，并允许并行开发和统一验证 | — Pending |
| 首版即采用多供应商和本地模型架构 | 避免单一厂商绑定，并支撑本地优先和成本控制 | — Pending |
| Coding、Research、Daily 并行，Coding 投入最高，三者共同构成 1.0 | 保留 Coding 优先级，同时不把其他产品线降为长期实验功能 | — Pending |
| 首个完整版本包含全部产品入口 | 仍是 1.0 北星；v0.2 只交付 CLI print/run + `kiana tui` | ⚠️ Revisit — deferred out of current milestone 2026-08-22 |
| 采用 Local-first 与首次启动自主权档位选择 | 同时满足个人可控性、易用性和高级自治需求 | — Pending |
| 采用 Open Core，1.0 同时包含官方云与企业自托管 | 个人采用、商业收入和企业治理形成完整产品闭环 | — Pending |
| 使用持续 Alpha/Beta，但严格保留 1.0 命名门禁 | 允许早期反馈，同时避免未完成产品被包装成正式版本 | — Pending |
| Reference 逐项采用 `Adopt / Adapt / Reject` 治理 | “全部参考”转化为可审计、可拒绝、可验证的明确合同 | — Pending |
| 1.0 的最高硬门禁是公开发行质量 | 功能数量不能替代稳定性、安全、恢复和长期维护能力 | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `$gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `$gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-08-22 after v0.2 MVP milestone recut*
