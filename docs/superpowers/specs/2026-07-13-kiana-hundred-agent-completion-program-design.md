# Kiana 百 Agent 完整项目推进计划设计

日期：2026-07-13

状态：已逐段确认

适用仓库：`kianacode`

目标执行环境：Codex 多会话 / 多 CLI 实例

执行规则：全项目不采用 TDD。每个 WorkPacket 先按批准的合同完成实现，再补充并运行 focused、adversarial、integration、review 验证；不要求 pre-implementation failure run 或 RED→GREEN evidence。

## 1. 目标

本设计定义如何使用 100 至 150 个可调度 Codex Agent，完成 Kiana 的全部产品目标，并把经过实战验证的多 Agent 协作能力迁入 Kiana 本身。

完成标准不是代码量、任务数量或文档数量，而是：

1. 对 38 个参考项目的能力进行 100% 盘点。
2. 每项能力必须归入 `implement`、`equivalent`、`alternative` 或 `excluded`。
3. 所有适用能力必须有生产代码、失败路径、测试、用户入口和 fresh evidence。
4. 18 个 Kiana 能力域全部达到 Domain Done。
5. Linux、macOS、Windows 产品与安全验证通过。
6. provider、remote、MCP、Web/IDE 等真实端到端路径通过。
7. 商业发布 blocker 严格模式归零。

本设计采用混合分层方案：完整能力账本和 170 个 WorkPacket 一次建立，但按基线、契约、热点拆分、领域实现、跨域集成和商用验收分波执行。

## 2. 设计原则

- 100 至 150 是 Agent 注册池，不是同时写代码的进程数。
- 默认最多 32 个活跃 Agent，其中最多 16 个 Builder。
- 一个 Agent 同时只持有一个 WorkPacket。
- 一个 WorkPacket 只允许一个 Implementer 写代码。
- canonical branch 始终只有一个写者。
- Task Card 可变；派发后的 WorkPacket 不可变。
- Worker 输出是不可信输入，必须经过独立 review 和 fresh verification。
- 上游 Agent 自报完成不等于依赖完成；依赖必须已进入当前 packet 的 base commit。
- 物理路径不重叠不代表语义无冲突；公共契约使用 semantic lock。
- 当前脏工作树是有效成果，不丢弃、不 reset、不自动 stash。
- 外部 Codex 控制面先实战，稳定后 Kiana 内建同一协议。

## 3. 两层控制面

### 3.1 版本化计划层

```text
Capability Ledger
  -> 18 Capability Domains
  -> 30 Ownership Packages
  -> 170 Task Cards
  -> Immutable WorkPackets
```

版本化计划层回答：

- 参考项目有哪些能力。
- 每项能力在 Kiana 中如何处理。
- 哪个领域和 ownership package 负责。
- 任务依赖、风险和验收是什么。
- 哪些任务可以进入 Ready。

### 3.2 运行状态层

```text
WorkPacket Attempt
  -> ResultPacket
  -> ReviewPacket
  -> VerificationPacket
  -> Integration Train
  -> Accepted Commit
  -> Commercial Evidence
```

运行状态层回答：

- 谁领取了任务。
- lease 是否仍有效。
- Agent 实际修改了什么。
- review、verification 和 integration 是否通过。
- 失败是否需要重试、拆包或人工决策。

### 3.3 存储布局

```text
docs/agent-program/kiana-completion/
  README.md
  program.json
  capability-ledger.jsonl
  domains/
  ownership/
  tasks/
  dags/
  acceptance/

.kiana/programs/kiana-completion/
  program.db
  artifacts/
  worktrees/
  prompts/
  logs/

docs/schemas/
  kiana-agent-program-*.schema.json
```

`docs/agent-program/` 保存需要 Git 版本化的计划事实。`.kiana/programs/` 保存高频运行状态，不进入功能事实提交。`docs/schemas/` 保存正式数据契约。

运行状态使用 SQLite WAL 和事务，不使用无锁 JSON read-modify-write。大日志、patch 和 packet artifact 存在内容寻址文件系统中，SQLite 只保存路径、SHA-256 和状态索引。

## 4. 核心对象

### 4.1 Capability Ledger

Capability Ledger 是参考覆盖的唯一权威。每条记录必须包含：

```text
capability_id
reference_project
reference_revision
source_paths
source_lines
behavior_summary
constraints
failure_behavior
target_domain
target_ownership_package
target_task_ids
disposition
acceptance
evidence
status
freshness
```

### 4.2 Ownership Package

Ownership Package 定义稳定的修改边界：

- 负责路径。
- 对外公共接口。
- 上游依赖和下游消费者。
- physical path locks。
- semantic locks。
- 当前与拆分后的并发容量。
- 唯一 Domain Integrator。

### 4.3 Task Card

Task Card 是可变项目管理对象：

```text
task_id
domain_id
ownership_package
outcome
priority
risk
status
dependencies
blockers
acceptance
candidate_paths
owner_role
reference_capabilities
required_artifacts
required_approvals
```

### 4.4 WorkPacket

WorkPacket 是 Agent 的唯一执行输入。WorkPacket 一旦进入 `leased` 就不可修改，任何范围变化都创建 revision 或 child packet。

必需字段分组：

| 分组 | 字段 |
| --- | --- |
| Identity | `packet_id`、`task_id`、`domain_id`、`ownership_package`、`wave_id`、`attempt`、`role` |
| Baseline | `base_commit`、`base_tree`、dirty manifest hash、`Cargo.lock` hash、toolchain、target、contract hashes |
| Scope | `goal`、`non_goals`、`read_paths`、`write_paths`、`exclusive_paths`、`forbidden_paths`、`generated_paths` |
| Dependencies | required packet/commit/artifact hashes、`produces`、`invalidates` |
| Lease | `lease_id`、fencing epoch、heartbeat interval、expires_at |
| Execution | Codex prompt、allowed commands、forbidden commands、network/secrets policy |
| Budget | wall time、tool calls、changed files、LOC、output bytes、compile budget、retry count |
| Verification | post-implementation focused command、adversarial cases、domain gate、acceptance criteria |
| Review | review focus、required reviewer roles、risk flags |
| Integration | queue class、commit/patch contract、rollback strategy |

### 4.5 Attempt 与结果包

每次执行生成独立 Attempt。Attempt 必须绑定：

- 新 worktree。
- 新临时分支。
- 新 lease 和 fencing token。
- 精确 base commit。
- 独立日志和 artifact directory。

允许的 terminal result：

- `completed`
- `blocked`
- `partial`
- `failed`
- `scope_violation`
- `stale_base`
- `lease_expired`

旧 Attempt 永久保留审计，但失效 token 不能再次进入集成队列。

## 5. 120 Agent 标准编制

注册池按 120 个 Agent 设计，可按相同比例扩展到 150。

| 角色 | 数量 | 职责 |
| --- | ---: | --- |
| Control Plane | 5 | Program Director、Scheduler、Lease、Evidence、Incident/Capacity |
| Domain Lead | 18 | 每个能力域一个 owner，维护接口、依赖和状态 |
| Builder/Researcher | 60 | 每次执行一个 WorkPacket |
| Independent Reviewer | 20 | 独立审查范围、实现、测试和安全性 |
| Fresh Verifier | 12 | 在新 worktree 中重放和验证 |
| Integrator/Release | 5 | 领域集成、全局 merge train、release evidence |

### 5.1 默认活跃槽位

```text
16 Builder
 6 Reviewer
 4 Verifier
 3 Domain Integrator
 1 Scheduler / Lease Controller
 1 Contract / Security Controller
 1 Global Merge Controller
-----------------------------
32 active slots
```

### 5.2 资源槽位

- Cargo compile/test semaphore：2 至 4。
- workspace test：1。
- release/security gate：1。
- read-only reference/research/doc audit：最多 32。
- canonical integration writer：1。

实际并发：

```text
C_active = min(
  32,
  ready_packets,
  conflict_independent_packets,
  host_capacity,
  reviewer_capacity,
  verifier_capacity,
  policy_capacity
)
```

主机容量必须使用最近 20 个 packet 的 p95 RSS、CPU、构建时长和文件描述符数据动态估计，不能仅按逻辑 CPU 数固定配置。

## 6. 30 个 Ownership Packages

| ID | Package | 主要范围 | 当前并发 -> 拆分后并发 |
| --- | --- | --- | ---: |
| P01 | Core contracts | `kiana-types` runtime/permission/trust/plugin/hook/tool DTO | 1 -> 2 |
| P02 | Constants/bootstrap | `kiana-constants`、`kiana-bootstrap` | 2 -> 2 |
| P03 | Coordinator profiles | `kiana-coordinator` | 1 -> 1 |
| P04 | Desktop/UI primitives | color/components/ink/modifiers/input/capture crates | 3 -> 5 |
| P05 | Browser/native integration | chrome/computer/url-handler crates | 3 -> 3 |
| P06 | Provider/model/auth services | `kiana-services/src/api`、auth、OAuth | 1 -> 3 |
| P07 | Integration services | MCP、LSP、network policy、analytics | 2 -> 4 |
| P08 | Repo intelligence/RAG | query index、repo map、search、vector、pack | 1 -> 4 |
| P09 | Query loop/hook policy | query config、transition、budget、stop hooks | 2 -> 3 |
| P10 | Skills/extensions | `kiana-skills` loaders、cache、plugin/MCP skill | 2 -> 3 |
| P11 | Workflow/evidence/integrity | `kiana-tasks` workflow/evidence/integrity | 1 -> 4 |
| P12 | Board/swarm model | project board、swarm、task types | 2 -> 3 |
| P13 | Tool kernel/filesystem | Tool、ToolContext、registry、permissions、file tools | 1 -> 3 |
| P14 | Shell/sandbox/process | Bash、PowerShell、exec policy、monitor | 2 -> 3 |
| P15 | External workbenches | MCP/LSP/Web/Notebook/remote tools | 2 -> 5 |
| P16 | Agent/team/task tools | agent/team/task/cron/worktree/workflow tools | 1 -> 6 |
| P17 | Command framework/catalog | Command、registry、local state、基础命令 | 1 -> 3 |
| P18 | Config/model/auth/trust commands | config/model/auth/license/trust/cost/stats | 2 -> 4 |
| P19 | Plugin/MCP/hooks commands | marketplace、MCP config、hooks、skills | 1 -> 5 |
| P20 | Coding/session/context commands | checkpoint/diff/review/checks/context/session/memory | 3 -> 6 |
| P21 | Project/workflow/swarm commands | tasks/project/evidence/audit/report/validate/swarm | 1 -> 7 |
| P22 | Release/eval/EDA/doctor commands | release/doctor/eval/EDA/advisor | 3 -> 5 |
| P23 | Remote runtime | `kiana-remote` | 3 -> 5 |
| P24 | Bridge runtime | `kiana-bridge` | 1 -> 4 |
| P25 | Assistant runner/SDK/session | entrypoint runner、SDK、session persistence | 1 -> 5 |
| P26 | CLI/print/app server | CLI router、stream-json、direct-connect、`/app` | 1 -> 8 |
| P27 | Interactive/background/MCP surfaces | TUI、REPL、bg、MCP、init、screens | 3 -> 6 |
| P28 | Schema governance | `docs/schemas`、schema contract gate | 1 -> 6 |
| P29 | Release automation/CI/proof | scripts、GitHub workflows、release evidence | 1 -> 6 |
| P30 | Product/reference/design docs | Project OS、reference audit、roadmap、matrix | 4 -> 6 |

P01、P17、P26、P28、P29 是汇聚包，默认单写者。`Cargo.toml`、`Cargo.lock` 和 workspace dependency 变更由 Dependency Integrator 串行处理。

## 7. 170 个 WorkPacket 配额

### 7.1 24 个全局前置包

| ID 范围 | 数量 | 内容 |
| --- | ---: | --- |
| E01-E04 | 4 | dirty intake、baseline commits、reference ledger、ownership/DAG |
| E05-E18 | 14 | 巨型文件、registry、release scripts 的行为不变拆分 |
| E19-E22 | 4 | runtime/session、tool/policy、workflow/evidence、app/release contracts |
| E23-E24 | 2 | SQLite lease/fencing、integration queue/build semaphore |

14 个热点拆分包覆盖：

1. CLI parse/route/print。
2. Direct-connect app-server routes。
3. Runner event/provider turn。
4. Runner tool/team loops。
5. Project/workflow/swarm command file。
6. Bridge work loop。
7. Plugin marketplace/install/policy/receipt。
8. Agent/team/task tool store。
9. TUI controller/reducer/view。
10. MCP service/tool transport。
11. Query index/search/vector/pack。
12. Command/tool registry decentralization。
13. Release/preflight/smoke shell libraries。
14. Product/reference documentation indexes。

拆分包只能做行为不变的 move、module extraction 和 re-export。语义修复必须进入后续独立 packet。

### 7.2 146 个领域包

| ID | 能力域 | Ownership | 包数 |
| --- | --- | --- | ---: |
| D01 | Runtime、EventLog、Recovery | P01、P11 | 6 |
| D02 | Session、SDK、Transcript | P20、P25 | 9 |
| D03 | Tool Kernel、Permission、Trust、Sandbox | P01、P13、P14 | 9 |
| D04 | Local Coding、Git、Review、Checks | P13、P20 | 7 |
| D05 | Workflow、Project OS、Router | P11、P21 | 10 |
| D06 | Task Board、Team、Bounded Swarm | P12、P16、P21 | 11 |
| D07 | Skills、Plugins、Hooks | P10、P19 | 7 |
| D08 | MCP、LSP、External Tools | P07、P15、P19 | 8 |
| D09 | Provider、Model、Auth | P06、P18、P25 | 9 |
| D10 | Remote、Bridge、Cloud Session | P23、P24、P26 | 7 |
| D11 | CLI、REPL、TUI | P04、P27 | 7 |
| D12 | Web、IDE、Browser Client | P26、新客户端 workspace | 9 |
| D13 | Repo Intelligence、RAG、Code Graph | P08、P20 | 9 |
| D14 | Memory、Consolidation、Staleness | P08、P09、P20 | 6 |
| D15 | Notebook、Data Interpreter、EDA | P05、P15、P22 | 8 |
| D16 | Eval、Audit、Evidence、Reporting | P11、P21、P22、P28 | 7 |
| D17 | Observability、Usage、Cost | P02、P06、P07、P18、P25、P27 | 7 |
| D18 | Cross-platform Release、Enterprise | P05、P28、P29 | 10 |
|  | 合计 |  | 146 |

总任务库为 `24 + 146 = 170` 个初始 WorkPacket。

每个领域必须覆盖 reference mapping、公共契约、正常行为、错误恢复、安全权限、实现后 focused/adversarial 验证、跨模块集成和产品验收。若 packet 允许修改超过 8 个路径、包含多个目标、验证命令不相关，或同时触及 schema/runtime/UI，则必须拆成 child packets。

## 8. Dependency DAG 与调度

### 8.1 两张调度图

```text
Dependency DAG
  决定是否 Ready

Path + Semantic Conflict Graph
  决定 Ready 集合中谁可以并行
```

DAG 节点包括 Task、Contract、Artifact、Verification 和 Approval。硬边包括 `requires`、`blocks`、`produces`、`verifies`；`related` 不影响 Ready。

### 8.2 Ready 条件

- 所有上游提交已进入 packet base commit。
- required artifact hash 匹配。
- required decision/approval 已完成。
- scope 与 verification 完整。
- 无冲突 lease。
- reviewer、verifier 和资源槽位可用。
- policy 允许派发。

### 8.3 Semantic Locks

最低集合：

```text
contract:RuntimeEvent
contract:Session
contract:Tool
contract:Workflow
registry:commands
registry:tools
global:Cargo.lock
global:schema-index
global:release-workflow
policy:commercial-security
```

Scheduler 按规范排序后一次性申请全部 physical 和 semantic locks。运行中禁止扩大 scope。第一版 lease 合同固定为 heartbeat 30 秒、TTL 120 秒、grace 60 秒；每个资源维护单调 fencing epoch。后续参数调整必须作为版本化 policy 变更进入独立 WorkPacket。

## 9. 执行波次

| Wave | 内容 | Builder 上限 | 晋级条件 |
| --- | --- | ---: | --- |
| W0 | 117 条变更审查、拆分提交、BaselineManifest | 0 | 新 baseline commit 与 ownership 明确 |
| W1 | hang 诊断、P0 panic/竞态、公共契约冻结 | 4-8 | post-implementation focused verification、contract review |
| W2 | 14 个热点拆分、registry 分域 | 8-12 | golden/route/schema 行为不变 |
| W3 | Runtime、Session、Tool、Workflow、Task/Swarm | 16 | domain、stress、recovery tests |
| W4 | Provider、MCP、Remote、RAG、Memory、Notebook、clients | 16 | 每域端到端验收 |
| W5 | 跨域 wiring、UI、observability、跨平台安全 | 8-12 | integration train 与 fresh verification |
| W6 | 三平台、live service、signing、channels、customer acceptance | 2-4 | commercial blockers 为 0 |

每个 Wave 使用 conflict graph coloring 形成 micro-wave。同色任务才可同时执行。

接口变更严格 contract-first：

```text
Contract Owner 修改并集成公共接口
  -> Consumers 基于新 commit 并行
  -> Commands/Entrypoints wiring
  -> Cross-domain verification
```

## 10. Codex 执行合同

每个 Codex Agent 获得：

```text
独立 worktree
单独临时分支
workpacket.json
prompt.md
bounded context pack
只读 reference notes
```

Builder 必须：

1. 校验 base commit、contract hashes 和 lease。
2. 声明已理解 scope。
3. 按批准合同实现最小改动。
4. 补充正常、异常、边界和回归验证场景。
5. 跑 focused、adversarial 和适用的 integration gate。
6. 审计 actual changed set。
7. 在临时分支创建一个 packet commit。
8. 写不可变 ResultPacket。
9. 不 push、不 merge、不修改 canonical branch。

Builder 禁止：

- 修改 forbidden paths。
- 降低默认权限或 release gate。
- 用超时增长或 `#[ignore]` 掩盖 hang。
- 伪造 provider、remote、signing 或 customer proof。
- 提交凭据。
- 顺手重构无关模块。

### 10.1 状态流

```text
draft -> planned -> blocked/ready
ready -> leased -> running
running -> result_submitted
result_submitted -> review_passed/rework
review_passed -> verification_passed/rework
verification_passed -> integration_queued
integration_queued -> accepted/stale/conflict
accepted -> done
```

## 11. Review 与 Verification

### 11.1 Independent Review

Reviewer 必须与 Implementer 不同，检查：

- touch set 和 lease 范围。
- 目标、非目标和 acceptance。
- 隐藏兼容性、并发和安全问题。
- 实现后验证是否覆盖原问题、边界和回归风险。
- 是否削弱 gate 或伪造 evidence。
- reference capability 映射是否准确。

### 11.2 Fresh Verification

Verifier 在新 worktree 中重放 packet commit：

| Gate | 内容 |
| --- | --- |
| G0 Packet | post-implementation focused/adversarial verification、format |
| G1 Domain | crate、contract、领域集成测试 |
| G2 Stress | concurrency、recovery、repeat、serial/default parallel |
| G3 Repository | workspace check/test、schema、release local-RC |
| G4 Commercial | 三平台、live service、signing、channels、acceptance |

G0/G1 可并行；G3 单槽；G4 仅由 Release Controller 执行。

## 12. Integration Queue

Queue entry 绑定：

```text
packet hash
result hash
review hash
verification hash
base commit
fencing epoch
touch set
dependency commits
risk
queue class
```

状态：

```text
enqueued
  -> stale_check
  -> reviewed
  -> targeted_verified
  -> domain_staged
  -> train_verified
  -> canonical_accepted
```

异常状态为 `stale`、`rework`、`quarantined`、`conflict`、`blocked`。

普通 merge train 包含 4 至 8 个互不冲突 packet。公共 contract/schema、`Cargo.lock`、auth/security/policy、release workflow、migration 和跨平台 sandbox 必须单包 train。

Domain Integrator 可并行维护 staging branches。Global Merge Controller 串行构建 train，全绿后只允许 fast-forward canonical branch。

## 13. 失败与恢复

| 情况 | 处理 |
| --- | --- |
| 临时网络、429、进程启动失败 | 同 packet 最多自动重试 2 次 |
| stale base / lease expired | 废弃旧 token，创建新 Attempt |
| path/scope/secret/policy violation | quarantine，禁止自动重试 |
| semantic/schema conflict | Contract Owner 创建 synthesis packet |
| flaky test | 固定种子重复 3 次，仍不稳则 blocker |
| merge train failure | staging 中二分最小失败集，健康包重新排队 |
| 连续两次相同失败 | stop-the-line，Incident Review |
| Worker 异常退出 | 保留日志和 Partial Result，回收 worktree |

不能仅根据 exit code 判断 terminal success。terminal 状态必须由原子状态记录和不可变 ResultPacket 共同证明。

## 14. Reference Capability Ledger

### 14.1 Disposition

每项参考能力必须选择：

- `implement`：实现相同行为。
- `equivalent`：已有等价行为并补齐证据。
- `alternative`：采用更适合 Kiana/Rust 的替代实现。
- `excluded`：不属于产品目标。

`excluded` 必须包含理由、影响分析和 Product Owner 批准。Agent 不能自行排除。

### 14.2 Capability 状态

```text
discovered
  -> classified
  -> planned
  -> implemented
  -> behavior_verified
  -> product_accepted
  -> commercial_proven
```

API、schema、测试名或文档存在均不能单独证明 `implemented`。至少需要生产路径、正常与失败测试、消费者或用户入口、fresh verification 和差异说明。

## 15. 完成定义

### 15.1 WorkPacket Done

- Result、Review、Verification Packet 均有效。
- packet commit 已进入 canonical branch。
- Capability Ledger 已绑定 evidence。
- 无未解释 scope deviation。

### 15.2 Domain Done

- 领域内所有适用能力达到 `behavior_verified`。
- 跨模块、恢复、安全和压力测试通过。
- 用户入口和错误处理完整。
- 无 P0/P1 blocker 和 stale evidence。

### 15.3 Program Done

- 38 个参考项目能力 100% 分类。
- 所有适用能力达到 `product_accepted`。
- 所有排除项获明确批准。
- 18 个领域全部 Done。
- Linux、macOS、Windows 验证通过。
- 真实 provider、remote、MCP、Web/IDE 路径通过。
- 签名、公证、渠道、entitlement、运营和客户验收 evidence 有效。
- `commercial-release-blockers-report --fail-on-blockers` 为 0。

## 16. 外部 Codex 控制面

外部控制面位于 `tools/agent-program/`，不依赖尚未稳定的 Kiana runtime。

```text
Program Compiler
  -> Scheduler / Lease Manager
  -> Worktree Manager
  -> Codex Launcher
  -> Result Collector
  -> Review / Verify Dispatcher
  -> Integration Controller
  -> Evidence Reporter
```

职责：

- 编译 capability ledger、domains、tasks 和 DAG。
- 事务性领取、续租、释放 WorkPacket。
- 创建和回收 Codex worktree。
- 生成 prompt 和 launch manifest。
- 收集结果并校验 touch set。
- 调度 Reviewer、Verifier 和 integration train。
- 输出 coverage、throughput、rework、stale、cost 和 blocker 报告。

## 17. Kiana 内建迁移

| 外部组件 | Kiana 落点 |
| --- | --- |
| Program/Task/Packet types | `kiana-types` |
| DAG、Scheduler、Lease、Integration Queue | `kiana-tasks` |
| Worktree、Codex process adapter | `kiana-tools` |
| `program plan/dispatch/status/integrate` | `kiana-commands` |
| CLI、TUI、app-server projection | `kiana-entrypoints` |
| Evidence、release gates | `kiana-tasks` + release scripts |

迁移使用 shadow mode：

1. 外部控制面完成至少 3 个完整 Wave、50 个 accepted WorkPacket。
2. Kiana 只读同一 schema/数据库，生成但不执行调度决策。
3. 对比 Ready、lease、retry 和 integration decisions。
4. 连续 20 个 packet 决策一致后，Kiana 接管新 dispatch。
5. 外部控制面降为恢复和兼容工具。
6. 同一时刻只有一个 active scheduler epoch。

内建能力验收：

- 100 个以上 WorkPacket 实战执行无丢失。
- lease 过期、进程退出和 stale base 可恢复。
- scope violation 必定阻止集成。
- canonical branch 始终单写者。
- 外部与内建结果能追溯到同一 capability、packet 和 evidence。
- 重启后不重复派发、不丢 terminal result。

## 18. 当前基线处理

设计输入快照为 `HEAD cbef0efd34e9a227d10b27b67bb48563126fa0ab`，工作树审计时包含 117 条状态。用户已确认这些变更全部是有效工作，需要保留并纳入新基线。该 commit 和状态数是审计输入锚点，不是 W0 的最终执行基线；本规格提交后，Baseline Curator 必须重新记录实际 HEAD 和完整工作树 manifest。

W0 使用唯一 Baseline Curator：

1. 记录 HEAD、tracked diff hash、untracked manifest、每个文件 mode/size/SHA-256。
2. 按功能、风险、ownership package 和完成度分类。
3. 检查 secret、generated output、reference drift 和 scope。
4. 按可审查功能切片拆分本地提交。
5. 每个提交运行对应 focused verification。
6. 生成 reviewed BaselineManifest。
7. 用户确认后冻结新 base commit。

W0 完成前：

- Builder concurrency 为 0。
- 当前 root 只允许 intake、review 和 coordination。
- 不允许新 Agent 猜测未提交文件归属。
- 不使用 reset、checkout、自动 stash 或批量丢弃。

## 19. Stop-The-Line 条件

任一条件发生时停止新增 dispatch：

- baseline drift。
- 未知文件 ownership。
- lease 或 fencing violation。
- contract hash 漂移。
- evidence 损坏或缺失。
- secret 泄漏。
- canonical branch 出现非 Merge Controller 写入。
- verification backlog 超过两个 micro-wave。
- 同一失败连续发生两次。
- commercial proof 来源无法认证。

恢复派发前必须创建 Incident Record、明确 root cause 或外部 blocker，并更新 DAG、packet 或 policy。

## 20. 设计验收标准

本设计通过的条件：

1. 38-reference 能力闭环有唯一账本。
2. 18 个领域与 30 个 ownership package 有明确映射。
3. 初始任务库精确为 170 个 WorkPacket。
4. 120 Agent 编制与 32 active cap 一致。
5. 当前 117 条变更有不丢失的 baseline intake 流程。
6. WorkPacket、lease、review、verification 和 integration contract 完整。
7. 失败、冲突、stale、retry 和 recovery 行为明确。
8. 外部 Codex 控制面和 Kiana 内建迁移复用同一协议。
9. Program Done 绑定真实产品与商用 evidence。

本规格批准后，后续实施计划必须按独立子项目生成，不能把 170 个 packet 塞入一个无法执行的单体计划。优先子项目顺序为：

1. W0 Baseline Intake。
2. Agent Program schemas 与 SQLite control plane。
3. W1 Reliability gates。
4. W2 Hotspot decomposition。
5. D01-D18 domain plans。
6. Cross-domain integration 与 commercial acceptance。
