# CO-01 CompanyOS 交接基线

> 快照日期：2026-09-14。本文是 `CO-01` 的 source-only 交接基线，不改变产品状态。
> 运行时验收由 GitHub CI 负责；本文把「已接线 / 仅类型 / 缺测试 / 未实现」四分类固定下来。

## 1. 快照

| 项目 | 记录 |
|---|---|
| source snapshot | `edf82307ccaf2cb7089de890434878b1dba68c00`（UI-00 收口提交） |
| worktree 基线 | `master`，工作树干净 |

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Company 聚合（domain） | `kiana-domain/src/company.rs` | `2a62850326a81300f366731bf49653e37e13019a14510d19d709531cfb7549ad` |
| 业务对象（domain） | `kiana-domain/src/company_business.rs` | `0190af710f60fb71768afaad0961beda4e0803d3ac21f6258e54843f7f05141e` |
| 收尾（domain） | `kiana-domain/src/company_closeout.rs` | `564c3ba0be17df570690628dbc567a61139484be5ec306c278ce3e9c766af74a` |
| 角色目录 | `kiana-domain/src/roles.rs` | `a2beb2104d751ee6c9e6b9ef510f85e65ec3da727973e110f44209b4c402a5c6` |
| 工单 | `kiana-domain/src/work_packets.rs` | `2cc8b27f3710512b50bef970715dade5d76f27d010c77e14e3c22823d2b8b1e6` |
| 会议 | `kiana-domain/src/symposiums.rs` | `4312fc403e7233a7e2c9e828a68ba8418cdadca0a79dba1ad8d191046b26e0ed` |
| 工单图 | `kiana-domain/src/packet_graph.rs` | `8f56ce5fb8ba80f50aabff3e6fb6221718289437b1a511123821c0d98509c8a0` |
| Swarm（domain） | `kiana-domain/src/swarm.rs` | `697c11d7c0f0be37adc5aeac6617e0920190b9c895574449994c4aec3c26ba0d` |
| Company 执行（core） | `kiana-core/src/company.rs` | `ac0fc31a32b5eec4f94692aeaf9431ef958b8677fd735d51dd690597d7f638ee` |
| 业务执行（core） | `kiana-core/src/company_business.rs` | `bf9b3f82227987b892f542d0ca328255c0d3da7fe11d450acd667530af9f0d5a` |
| 协作 | `kiana-core/src/collaboration.rs` | `49e61cf89add855ca4fc3687104d666388477848d63397b9ea50b0404021dabf` |
| 自动化 | `kiana-core/src/automation.rs` | `806831b7077e11d918aa78de5e73ff453535a143fe917d63e1be522dde9bd853` |
| Swarm（core） | `kiana-core/src/swarm.rs` | `71a3b50e7971cb3c3eac4fece08a8e94f4a4ca83ec3c0a6a7c1d7407460fc9bb` |

## 2. 已接线 / 仅类型 / 缺测试 / 未实现

### 已接线（产品路径真实存在）

- **Company 聚合全量接线**：`CompanyState`（domain company.rs:866-892）为事件溯源聚合根，17 张状态表 + `revision`；`load_company`（core company.rs:691-724）从 `company` 聚合流重放，`commit_company`（:726-760）写 `company.<event>` 事件。
- **46 个 CompanyCommand 全部实现**（domain company.rs:549-741）：Objective/Initiative/Project 全生命周期（Propose→Chartering→Approve→Plan→Active→Acceptance→Close→Archive）、Milestone、Packet（Approve/Rework/Claim/Renew/Reclaim）、Run（StartRun/Reconcile/RecordStarted）、Acceptance、Review、Delivery（含 `MarkDeliveryUnknown`/`ReconcileDelivery`）、Outcome、Change、Risk、Incident、Pause/Resume/Cancel/Fail/Archive。
- **命令执行链**：`handle_company_command`（core company.rs:230-427）按序执行 company_context guard → 字节上限 → schema/idempotency 校验 → 幂等重放 → revision CAS → swarm-parent 检查 → proof → `state.transition` → `commit_company` → Business 副作用 / `StartRun`→`spawn_from_packet` / `RequestCancelProject` 取消清扫。
- **wire 入口**：CLI `kiana command <name>`（product_command.rs）→ `RequestEnvelope::command` → `ControlPlane::handle_command`（commands.rs:240）→ company handlers；生产调用者 automation.rs:196、platform.rs:901-922、swarm.rs:132。
- **五部门六角色完整**（✅ 唯一验证通过的 key claim）：`RoleSpec::catalog()` 六角色（roles.rs:295-303，编译期定数）+ `DepartmentSpec::catalog()` 五部门（:597-604，测试断言确数）。
- **依赖环检测存在**：`validate_dependency_dag`（packet_graph.rs:62-130，`packet_dependency_cycle`）在 PlanProject/ClaimPacket/StartRun 强制；业务侧 `business_milestone_cycle_or_missing`/`business_packet_cycle_or_missing`（company_business.rs:793/808）。
- **workflow durable**：kiana-workflow crate（lib.rs + durable.rs 1016 行）基于现有 EventStore 的持久 workflow/trigger。

### 缺测试（已接线但零覆盖）——本基线最重要的 RED 清单

1. **`company_unapproved_project_cannot_dispatch` 不存在**（docs-only 目标，companyos.md:76）：无任何测试断言未批准项目派发时 broker/model 调用为零。`company_approved_packet_required` guard（core company.rs:486/535/591）未测。最接近的零断言测试是 `spawn_packet_path_allow_fails_closed_outside_the_packet`（core control_plane.rs:3973）。
2. **`company_approved_packet_reaches_existing_harness` 不存在**（companyos.md:77）：`StartRun → spawn_from_packet` 接线（core company.rs:358-402）未测。
3. **46/46 CompanyCommand 变体经命令路径零测试**：`CompanyCommandRequest`/`handle_company_command` 无任何集成或单元测试（daemon/entrypoints 测试零 company 引用）。
4. **CO-27 等待环仍是登记在案的开缺陷**：触发条件 = `RequestAcceptance` 要求全项目 run/packet 完成（domain company.rs:1846/1859 `project_other_runs_incomplete`/`project_packets_incomplete`）+ milestone 启动要求依赖 Accepted（:1705-1713）——这些检查**就是**环本身，无任何机制检测或打破它。DAG 无环≠无等待环（无环图照样死锁此语义）。CO-27 两个目标测试均不存在。
5. 领域单测仅有 8 个（kiana-domain/src/tests.rs:82-560）：合同校验、budget lease、WorkPacket legacy/roundtrip/prompt、symposium 排除 Builder、五部门开会、ReviewPacket 双断言。milestone 验收门、acceptance/delivery 流、claim/renew/reclaim lease、company_snapshot 投影全部无测试。
6. company 相关 core/daemon 源文件（company.rs 1242 行、company_business.rs 718 行、swarm/collaboration/automation/platform、durable.rs 1016 行）**全部无 cfg(test)**。

### 仅类型 / 未实现

- 未见「仅类型未接线」的 Company 主对象——46 命令均接线。差距不在类型，在验证。
- 旧文档「业务对象不存在」「无类型」表述与现状不符（六角色五部门 + 46 命令均已存在）；本基线按现状记录，旧表述不作为能力证明也不作为上限。

## 3. CO 步骤映射（复用范围）

| CO 步骤 | 现有实现锚点 | 复用判定 |
|---|---|---|
| CO-02–05（组织/立项分层） | CompanyState + Project/Objective/Initiative 状态机全量 | 直接补测试，不重建 |
| CO-13/16-17（工单合同） | WorkPacket + PacketClaim + company_packet_graph_invalid | 复用；通用部门工单是扩展 |
| CO-24–36（验收/交付/收尾链） | Acceptance/Delivery/Outcome/Change/Risk/Incident 机器 + closeout.rs | 复用 + 测试补齐 |
| CO-26–28（Milestone 独立验收） | 等待环触发条件已定位（§2.4） | CO-27 需要设计变更，不只是测试 |
| CO-07–08/23/32/42（恢复） | automation DispatchIntent + durable.rs | 复用现有 EventStore 底座 |
| CO-14–16/22-23/38-41（ProcessManager） | Business side effect + swarm 路由 | 复用路由，补合同 |

## 4. 交接说明

- CO-01 的两个验收测试是「复现」语义：实现已存在、测试待写，写时应断言 broker/model 计数为零（先拒绝）和 spawn/receipt 全链（再成功），落到 kiana-core/tests 或 daemon 集成测试。
- 46 命令零覆盖是 CO-02+ 的最大测试债；建议按状态机家族分批补（project 生命周期 → packet/claim → acceptance/delivery → change/risk/incident）。
- CO-27 是设计缺陷不只是测试缺口，等待 CO-26–28 专门处理。
