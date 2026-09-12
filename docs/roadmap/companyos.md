# Roadmap 专项：CompanyOS 组织与业务专项

> 返回 [Kiana 执行路线图](../roadmap.md) 的总图与当前窗口。本文保留原专项编号、状态、依赖、验收口径和证据限制；专项步骤完成不会自动改变 P 阶段状态。

<a id="companyos-business-design"></a>

## 17. CompanyOS 组织与业务补全（2026-09-12 追加设计）

本节回应「完善 module-map 中 CompanyOS 的实际代码设计」：补齐五部门、角色任命、立项、规划、任务交接、评审验收、交付收尾及成果测量。**本次只补设计与步骤，所有新增步骤均为待实施/待核验；不表示产品已经完成。**

配套材料：

- [组织与业务代码设计](../company-os-organization-business-design.md)：对象、模块、状态、命令、会议、调度、恢复及用户流程。
- [参考调研记录](../reference-agent-audit/company-os-organization-business-survey-2026-09-12.md)：72 个 reference 目录清单、重点机制核对、5 个目录外相关项目及取舍。
- [模块地图](../module-map.md)：本节聚焦其中「1. CompanyOS：组织与业务」，复用其他模块的执行能力。

### 17.1 与正在执行的 roadmap 如何衔接

`CO-01`–`CO-48` 是本文既有 P0–P4 单元的**细化步骤标识**，不是第二套阶段编号。每张卡列出归属、直接依赖和具体退出条件；原单元只有在其完整范围有证据后才更新状态。另一个 agent 已实现同一部分时，先跑对应验收、复用实现，不再写一套。

当前 §2 窗口继续由现有实现 agent 推进。本清单在交接核对后按依赖取最小切片，不要求先做完所有 P0–P6 才开始组织与业务工作；身份、审批、事件、恢复等依赖必须在产品链实际成立。仅有 WIP 类型或旧 CI 不能视为满足依赖。

本轮期间末尾已追加 ControlPlane 专项（§14，CP-00–CP-30）。通用身份、事务、审批、派发、取消和恢复以其对应实现为共用底座；CO 补的是组织/业务对象与这些能力的接线，不能再建 EventStore、workflow 引擎或效果循环。追加前 WIP 已有 workflow/trigger、工单认领、交接与 swarm 路由；CO-01 先核验，CO-18–CO-23、CO-43–CO-45 在原路由上补合同与验证。

按用户最新要求，旧版「只允许六行卡」「九个业务命令」「冻结项一概不得讨论」等限制若妨碍本次补全，以本次任务和 §17–§19 为准。这里确定的是目标设计及实施顺序，源码仍须版本化迁移；不授权自动提交、推送、发布或外部交付。

### 17.2 本次最关键的设计补充

| 问题 | 本次设计 | 落在步骤 |
|---|---|---|
| Company 被混同于一组角色 prompt | Organization/Department/RoleAssignment 与 Project/Packet/Attempt/Cell 分层；业务状态由 ControlPlane 掌握 | CO-02–CO-05、CO-13、CO-21–CO-23 |
| 旧卡仍写业务对象不存在 | WIP 已有五部门、六角色；命令初读 40 个、追加前增至 46 个，并新增 workflow/调度接线；重新核对源码和验收 | CO-01 |
| WorkPacket 只能指派 Builder | v1 保留 Builder 合同，增加版本化通用部门工单及结果合同 | CO-13、CO-16–CO-17 |
| 分阶段验收可能死锁 | Packet、Milestone、Project 分别验收；M1 可以先 Accepted，再启动依赖它的 M2 | CO-26–CO-28 |
| 文本去重当成标准覆盖 | Criterion ID + refines/covers/verifies 关系 + 逐条证据；同文不同目标不合并 | CO-06、CO-11、CO-25 |
| 返工/变更覆盖历史 | 新 attempt 或 successor packet；变更原子发布新 baseline、使旧批准失效 | CO-19、CO-29–CO-30 |
| 命令成功或模型完成被当成交付成功 | Run、Review、Acceptance、Delivery、ClosingReceipt、Outcome 分别记录和确认 | CO-24–CO-36 |
| 公司有 API，但用户仍需手工串所有命令 | 同一 Harness 的角色输出进入 Proposal；确定性 ProcessManager 生成下一动作与 HumanTask | CO-14–CO-16、CO-22–CO-23、CO-38–CO-41 |
| 有事件还不能证明恢复可用 | DispatchIntent 与消费回执持久化；旧事实按原版本重放；Unknown 必须对账 | CO-07–CO-08、CO-23、CO-32、CO-42 |

### 17.3 推荐实施批次

| 批次 | 步骤 | 可交付的检查点 |
|---|---|---|
| A 组织与事实底座 | CO-01–CO-08 | 身份、业务权限、引用和版本可验证；同一命令不能重复产生效果 |
| B 从需求到正式派工 | CO-09–CO-17 | Charter/Plan/会议提案产生通用工单，责任交接有 ACK |
| C 可监督的实际执行 | CO-18–CO-24 | 唯一 ready、claim/fence、Cell 资源、fresh Run、过程推进、证据入账 |
| D 逐层验收与异常闭环 | CO-25–CO-32 | 分阶段交付无等待环；拒绝、返工、变更、暂停、取消、Unknown 都有出口 |
| E 交付与可用入口 | CO-33–CO-42 | 本地交付/确认/收尾/Outcome、记忆、任务视图、人工决定与重启恢复 |
| F 并行、复用与总验收 | CO-43–CO-48 | 有界并行与集成、多项目容量、过程模板、完整 fake/live 证据 |

优先检查当前最容易形成假闭环的三个反例：**未批准项目也能写、依赖里程碑无法先验收、换 session 的作者给自己评审**。这些在相应步骤开始时先复现，不等到最后一轮才补。

---

<a id="companyos-business-steps"></a>

## 18. CompanyOS 详细步骤（CO-01–CO-48）

所有测试名均为**待新增或需加强的验收目标**，不是已存在/已通过声明。代码路径带「拟新增」的要由执行者创建；其他路径是当前核对入口。每步先覆盖拒绝，再覆盖真实产品调用；共同验证命令、证据要求及跨步骤场景见 §19。

### 批次 A：组织与事实底座

<a id="co-01"></a>

<a id="step-co-01"></a>



#### CO-01 · 锁定当前实现与计划的交接基线　⏳

- **归属**：`P3-I-01`、`P1-C-01`；本步骤先核对，不重建已有类型。
- **依赖**：无；与当前 §2 窗口做源码快照交接。
- **代码与产物**：domain/core 的 `company.rs`、`roles.rs`、`work_packets.rs`、`symposiums.rs`、`packet_graph.rs`/`handoff.rs`、core `collaboration.rs`/`automation.rs`/`swarm.rs`、workflow `durable.rs`、`CURRENT_STATUS.md`；形成“已接线/仅类型/缺测试/未实现”清单。
- **实现顺序**：①记录 HEAD、相关 WIP 文件 hash 与现有测试目标；②逐项追踪 CompanyCommand→DaemonHost→ControlPlane→事件/运行；③把下列 CO 步骤映射到现有实现，明确复用范围。
- **先拒绝**：复现 `company_unapproved_project_cannot_dispatch`，实际协议请求必须被拒绝，broker/model 调用次数为零；同时登记 CO-27 等待环的源码触发条件。
- **再成功 / 退出**：`company_approved_packet_reaches_existing_harness` 在明确的 fixture 中经过现有 spawn/receipt；没有运行依据的 WIP 保持待验收。文档中的旧“无类型”表述与证据分别记录。

<a id="co-02"></a>

<a id="step-co-02"></a>



#### CO-02 · 稳定组织、业务项目与工作区绑定　⏳

- **归属**：`P1-C-01`、`P3-I-01`、`P0-A-01a`。
- **依赖**：CO-01。
- **代码与产物**：`kiana-domain` ID/contracts；拟新增 `organization.rs`；core Company scope resolver、daemon 现有路径解析器。
- **实现顺序**：①补 OrganizationId、Membership/WorkspaceBinding 引用；②ProjectId 与 canonical filesystem path 分离；③定义旧 principal+path stream 到稳定 scope 的显式迁移/只读映射，禁止同一历史流被双重导入。
- **先拒绝**：`company_scope_rejects_foreign_project_and_ambiguous_legacy_root` 拒绝跨组织引用、错误项目根、歧义迁移；调用方填写 organization_id 不能扩权。
- **再成功 / 退出**：`two_business_projects_share_a_workspace_without_sharing_authority` 可明确区分同目录中的两个项目；工作区迁移后的历史查询仍定位原 ID；standalone Run 不成为项目证据。

<a id="co-03"></a>

<a id="step-co-03"></a>



#### CO-03 · 角色任命、有效期与撤销接入服务端身份　⏳

- **归属**：`P0-K1-01`、`P1-C-01`。
- **依赖**：CO-02；`P0-K1-01` 的本地 authenticated principal 前置须可用。
- **代码与产物**：domain RoleAssignment/ProjectAssignment、`kiana-ports` 身份接口、daemon context 构造、core capability/approval/company guards。
- **实现顺序**：①角色模板与任命分离；②从持久 assignment 解析角色、部门、项目范围、有效期；③撤销/收窄递增 authority epoch，在派发、Continue、审批消费和效果前重新核验。
- **先拒绝**：`revoked_assignment_blocks_company_command_and_approved_continuation`、`client_role_name_cannot_impersonate_sponsor`；拒绝后无新效果，旧批准不能被消费执行。
- **再成功 / 退出**：`active_assignment_survives_reopen_with_the_same_scope`；角色实例、session 和 actor 在 Receipt 中可定位，原始 owner 不由角色切换改写。

<a id="co-04"></a>

<a id="step-co-04"></a>



#### CO-04 · 五部门与专业岗位成为版本化目录　⏳

- **归属**：`P1-C-03`、`P1-J2-03`、`P1-J2-04`。
- **依赖**：CO-03；复用现有角色包与 prompt provenance 工作。
- **代码与产物**：`kiana-domain/src/roles.rs`、`role-packs/`、daemon 模型/提示词注入。
- **实现顺序**：①登记五部门的 mission、角色、工件和 gate；②补首个流程需要的 Analyst/QA/Librarian 等岗位，其他岗位可按需启用；③固定 RoleSpec、prompt、model profile、输入输出 schema 版本并写进 Run 证据。
- **先拒绝**：`role_pack_cannot_grant_capabilities_or_load_before_project_trust`；未知岗位/模型配置不静默回退为 PM、Builder 或权限更大的角色。
- **再成功 / 退出**：`planning_and_execution_role_profiles_reach_distinct_model_requests` 由同一 host 的真实 fake-provider 请求观察角色差异；同一版本可复现，创建目录不自动启动所有角色。

<a id="co-05"></a>

<a id="step-co-05"></a>



#### CO-05 · 业务命令权限、责任和人工决定合同　⏳

- **归属**：`P3-I-02`、`P2-K3-01`、`P0-F-01`。
- **依赖**：CO-03、CO-04。
- **代码与产物**：CompanyCommand policy、domain HumanTask/DecisionPurpose、`kiana-protocol` 请求/响应；复用审批权威。
- **实现顺序**：①逐命令列出 actor kind、assignment、目标状态、批准目的与证据；②区分 Sponsor 与模型 SponsorProxy，负责人/执行者/评审/接收者分别绑定；③人工决定固定目标版本、digest、允许选项、有效期和待恢复动作。
- **先拒绝**：`sponsor_proxy_and_builder_cannot_approve_their_own_business_request`；审批过期、scope 不同、目标版本不同均被拒；工具批准不代替 Charter/Acceptance/Delivery 决定。
- **再成功 / 退出**：`business_decision_records_the_authorized_decider_and_purpose`；所有可写命令有明确矩阵条目，新增 variant 默认拒绝，后台服务身份不能伪造人类决定。

<a id="co-06"></a>

<a id="step-co-06"></a>



#### CO-06 · 不可变工件、Evidence 与 Criterion 引用合同　⏳

- **归属**：`P2-K4-01`、`P3-I-01`、`P3-I-04`。
- **依赖**：CO-02、CO-05。
- **代码与产物**：domain ArtifactRef/ArtifactVersion/EvidenceRef/Criterion、core `artifacts.rs` 与 CompanyProof、ports 只读内容接口。
- **实现顺序**：①每个引用包含稳定 ID、版本、hash、schema、scope、provenance；②内容持久化后才能提交引用，历史内容与工作区当前内容分开；③为相同文字的标准分配不同 Criterion ID，检查字段大小和密级。
- **先拒绝**：`company_evidence_rejects_foreign_run_changed_content_and_missing_blob`；断链、hash 错误、symlink/路径逃逸、伪造命令退出码不能形成验收证据。
- **再成功 / 退出**：`artifact_version_remains_reviewable_after_workspace_file_changes`；能复查批准时的原件，并明确显示当前工作区已变化；blob/事件写入间故障不产出虚假成功。

<a id="co-07"></a>

<a id="step-co-07"></a>



#### CO-07 · 版本化业务事实与稳定命令回执　⏳

- **归属**：`P3-I-02`、`P0-A-01b`、`P0-G-04`。
- **依赖**：CO-05、CO-06。
- **代码与产物**：domain CompanyCommand/Event/CommandReceipt、core `company.rs`、protocol、EventStore CAS；DispatchIntent 与消费事实。
- **实现顺序**：①固定 logical command ID、payload digest 和预期 revision；②同一事务边界提交业务事实和后续效果意图；③重复命令返回原事件/结果引用，投影新状态另列；跨 session 查询 receipt 重新验证主体但不重复执行。
- **先拒绝**：`company_duplicate_key_with_changed_payload_or_authority_is_rejected`；并发相同 expected revision 只有一方提交；错误响应不得掩盖已提交事实。
- **再成功 / 退出**：`company_command_retry_after_reopen_returns_original_receipt_without_dispatch`；原 key/事件保持稳定，dispatch 次数为一或仍处待对账，绝不盲目重复。

<a id="co-08"></a>

<a id="step-co-08"></a>



#### CO-08 · 业务状态机、历史重放与兼容迁移　⏳

- **归属**：`P0-B-01`、`P0-A-01b`、`P3-I-03`。
- **依赖**：CO-07。
- **代码与产物**：domain `company.rs`/`states.rs`、拟新增 Company event reducer、旧 v1 replay adapter、协议兼容 fixture。
- **实现顺序**：①为每类业务对象登记允许边、决定者、所需证据和失败出口；②分离状态 revision 与 scope/criteria baseline version；③新事实用 apply(event) 重建，旧命令事件固定原 reducer 语义，未知 major 拒绝执行但可保留原始数据。
- **先拒绝**：`company_replay_rejects_gaps_conflicting_terminals_and_unknown_execution_schema`；过往事实不能因今天角色 policy 改了而被重写。
- **再成功 / 退出**：`company_v1_history_rebuilds_identically_after_policy_and_schema_upgrade`；每个可执行状态都有明确暂停/取消/失败去向，旧 v1 wire 测试保留，迁移本身幂等。

### 批次 B：从需求到正式派工

<a id="co-09"></a>

<a id="step-co-09"></a>



#### CO-09 · Objective 与 Initiative 受理和取舍　⏳

- **归属**：`P3-I-01`、`P3-I-02`。
- **依赖**：CO-08。
- **代码与产物**：domain Objective/Initiative、core Company handlers、受理提案 schema。
- **实现顺序**：①录入问题、目标 owner、metric/baseline/target/单位/方向/窗口；②Initiative 补 hypothesis、价值/成本/风险、资料缺口和取舍决定；③批准后转换为 Project 引用，重复转换返回原结果，拒绝/撤回保留历史。
- **先拒绝**：`initiative_cannot_convert_without_approved_objective_and_sponsor_decision`；NaN、时间窗倒置、错误 owner/organization 和缺测量方法均不能批准。
- **再成功 / 退出**：`approved_initiative_converts_once_and_keeps_objective_ancestry`；允许直接立项但需记录跳过 Initiative 的明确入口，不能丢失目标关系。

<a id="co-10"></a>

<a id="step-co-10"></a>



#### CO-10 · Charter 与项目 go/no-go　⏳

- **归属**：`P3-I-01`、`P3-I-02`。
- **依赖**：CO-09、CO-06。
- **代码与产物**：Project、版本化 Charter、ProjectBudget 引用；Company approve/reject handlers。
- **实现顺序**：①立项角色生成范围、非目标、干系人、成功标准、风险和预算的草案；②Sponsor 批准精确 Charter 版本与决议；③冻结项目 baseline，未经批准只允许受理/分析，不能进入实现派发。
- **先拒绝**：`project_approval_rejects_missing_budget_changed_charter_and_inactive_objective`；填写一个非空 budget_ref 不等于已存在有效预算。
- **再成功 / 退出**：`project_charter_approval_freezes_scope_criteria_and_budget`；后续状态事件不递增 baseline；拒绝项目保持可查询且不能写源码。

<a id="co-11"></a>

<a id="step-co-11"></a>



#### CO-11 · 建立标准覆盖图，替换文本集合推断　⏳

- **归属**：`P3-I-04`、`P1-D-01`。
- **依赖**：CO-06、CO-10。
- **代码与产物**：拟新增 `kiana-domain/src/criteria.rs`、Plan coverage、core 冻结校验。
- **实现顺序**：①标准用 ID 和 origin baseline 定位；②Project→Milestone→Packet 的 covers/refines 关系显式批准；③每个必需上层标准都有覆盖安排，机器验证引用/完整性，语义充分性由独立规划或评审记录支撑。
- **先拒绝**：`criteria_coverage_rejects_missing_parent_duplicate_identity_and_silent_weakening`；相同文字不同来源不得合并，一条通用“测试通过”不能覆盖所有标准。
- **再成功 / 退出**：`criterion_trace_links_project_requirement_to_specific_test_evidence`；能从项目目标追到具体验证，又能说明尚未覆盖的部分。

<a id="co-12"></a>

<a id="step-co-12"></a>



#### CO-12 · Milestone、Plan 与依赖图提案　⏳

- **归属**：`P3-I-01`、`P1-D-02`。
- **依赖**：CO-10、CO-11。
- **代码与产物**：Milestone、版本化 Plan/WorkGraph、domain DAG helper、core Plan validation。
- **实现顺序**：①把目标拆成可独立验收的里程碑与输出；②分清 parent/child、阻塞依赖、资料引用及信息关联；③批量验证候选图，冻结依赖版本，提供稳定拓扑顺序、missing refs 和 cycle path。
- **先拒绝**：`plan_rejects_missing_dependencies_cycles_and_cross_project_edges`；空必需里程碑、孤立标准和读取范围不足都返回具体原因。
- **再成功 / 退出**：`two_milestone_plan_preserves_dependency_and_acceptance_boundaries`；先允许草稿批量创建，再在批准事务核验完整图，避免只能逐个插入造成顺序限制。

<a id="co-13"></a>

<a id="step-co-13"></a>



#### CO-13 · WorkPacket 扩为通用部门工作合同　⏳

- **归属**：`P1-D-01`、`P1-E-01`、`P1-C-01`。
- **依赖**：CO-04、CO-06、CO-12。
- **代码与产物**：`work_packets.rs`、schema registry、domain ResultContract/CompanyProposal；现有 Builder packet adapter。
- **实现顺序**：①新增受版本控制的工作种类、输入/输出 schema、责任方和目标角色任命；②输入依据区分 Intake/Initiative/Charter/Plan，分析/规划包可在相应批准阶段运行、只写受控提案，实现包必须绑定批准 Plan 与写集；③业务 packet 不携带可自行生效的 Grant/Lease，运行权限仍由 core 派生。
- **先拒绝**：`department_packet_cannot_smuggle_builder_write_scope_or_runtime_grants`；旧 v1 的 `packet_role_must_be_builder` 测试保持原语义，新 schema 单独验证岗位/任务种类。
- **再成功 / 退出**：`analysis_and_implementation_packets_validate_through_versioned_contracts`；可表达立项调查、规划、实现、验证、交付准备和知识整理；未生成 Plan 时分析包可在受理授权下工作，实现包仍拒绝，消除“先有计划才能生成计划”的循环。

<a id="co-14"></a>

<a id="step-co-14"></a>



#### CO-14 · 会议、黑板和决议对象补齐　⏳

- **归属**：`P1-E-02`、`P4-E-03`。
- **依赖**：CO-04、CO-06、CO-13。
- **代码与产物**：`symposiums.rs`、DecisionProposal/DecisionRecord、domain 会议状态和公开/私有上下文视图。
- **实现顺序**：①绑定 Project/baseline、议程、具名 assignment、主席、限额与输出合同；②Claim/Vote/Draft 带来源、版本和证据；③声明会议/异步处理选择、分歧、未决项和明确决定者，私有 brief 单独保存。
- **先拒绝**：`symposium_rejects_uninvited_stale_and_duplicate_contributions`；投票数不等于批准权，未点名角色不能读主席准备或伪造票。
- **再成功 / 退出**：`symposium_decision_preserves_alternatives_dissent_and_evidence`；五部门可使用相同合同；联席会议明确列出各 assignment 的可见与决策范围。

<a id="co-15"></a>

<a id="step-co-15"></a>



#### CO-15 · 用现有 Harness 驱动有界角色会议　⏳

- **归属**：`P1-E-02`、`P4-E-03`、`P1-J2-03`。
- **依赖**：CO-14；现有角色 max_steps、wall-time 和取消路径已验收。
- **代码与产物**：core `convene_symposium`、daemon 角色模型路由、Runner ResultContract 校验。
- **实现顺序**：①主持人按确定性调度点名，复用每位成员独立 session；②模型仅返回结构化贡献，core 核验后发布 blackboard；③达到轮次、消息、token、时间或 stall 上限就关闭/升级，形成可复查 DecisionProposal。
- **先拒绝**：`symposium_budget_exhaustion_or_cancel_never_emits_an_approved_decision`；不得广播私有 scratch，成员失效不静默换人或扩权。
- **再成功 / 退出**：`symposium_and_async_proposal_share_the_same_decision_gate`；开会和 anti-meeting 两条路都产同一合同、经同一决策入口且可重放，所有模型调用来自已有 Harness。

<a id="co-16"></a>

<a id="step-co-16"></a>



#### CO-16 · 角色提案进入业务命令，原子批准计划　⏳

- **归属**：`P3-I-02`、`P1-E-02`、`P1-D-02`。
- **依赖**：CO-07、CO-12、CO-13、CO-15。
- **代码与产物**：CompanyProposal intake、PlanApproved 事实、core Company handlers；protocol 计划预览与批准 DTO。
- **实现顺序**：①只接收授权角色任务的明确结构化结果；②校验输入版本、图、范围、预算与标准覆盖，生成待批准的完整 Plan 预览；③一次 CAS 固定 Plan/Milestone/Packet 引用集合，产生后续 Handoff 意图。
- **先拒绝**：`proposal_text_cannot_execute_commands_and_invalid_plan_commits_nothing`；README、stdout 或 assistant 文本里的命令 JSON 不触发副作用；第 N 个 packet 校验失败不留下半套批准计划。
- **再成功 / 退出**：`approved_plan_materializes_the_exact_reviewed_packet_graph_once`；用户看到并批准的计划 hash 与生效图一致，重复批准不重复创建包。

<a id="co-17"></a>

<a id="step-co-17"></a>



#### CO-17 · 跨部门交接与 ACK 责任转移　⏳

- **归属**：`P1-E-01`、`P1-D-03`。
- **依赖**：CO-13、CO-16。
- **代码与产物**：domain Handoff、owner/assignee/acceptor 绑定、core handoff commands；状态投影显示待接单方。
- **实现顺序**：①发送方提交精确 packet revision、输入/输出和目标 assignment；②接收方验证范围、预算和资料后 ACK 或拒收；③ACK 前原 owner 负责，拒收/超时返回原 owner 并提供升级对象；重发不重转责任。
- **先拒绝**：`handoff_ack_rejects_wrong_recipient_revision_and_expired_assignment`；沉默不算接单，StatusReport/Chat 不产生责任转移。
- **再成功 / 退出**：`handoff_changes_accountability_once_and_survives_reopen`；同一交接在各部门视图中一致，ACK 记录不自动领取运行 lease。

### 批次 C：可监督的实际执行

<a id="co-18"></a>

<a id="step-co-18"></a>



#### CO-18 · 唯一 ready 谓词与可解释阻塞原因　⏳

- **归属**：`P1-D-01`、`P1-D-02`。
- **依赖**：CO-08、CO-12、CO-17。
- **代码与产物**：domain `ready_packets`/依赖判定、core spawn admission、daemon query、入口 next/看板。
- **实现顺序**：①给定图快照和显式 now 计算就绪；②检查批准/接单、依赖满足类型、输入版本、暂停/变更、有效 lease；③返回 Ready 或结构化 blockers，所有消费者调用同一实现。
- **先拒绝**：`ready_packet_requires_the_declared_dependency_outcome`；RequiresAcceptance 不被 Run Completed 满足，Unknown/失败依赖、缺件、有效旧 claim 都不能派发。
- **再成功 / 退出**：`ready_predicate_agrees_between_scheduler_spawn_and_project_view`；同一输入的排序/原因一致；查询本身不取得 claim、预算或写权限。

<a id="co-19"></a>

<a id="step-co-19"></a>



#### CO-19 · 原子 claim、租约 fencing 与执行尝试　⏳

- **归属**：`P1-D-03`、`P0-J1-01`。
- **依赖**：CO-07、CO-18。
- **代码与产物**：PacketClaim/PacketAttempt、core claim/renew/reclaim commands、EventStore CAS、可控时钟 fixture。
- **实现顺序**：①分离长期 packet 身份与短期 attempt/lease；②原子认领记录 worker assignment、epoch、expires_at、execution request；③过期先 fence 旧实例，确认未执行/已停止后才允许新 attempt；重试不能复活旧终态。
- **先拒绝**：`expired_claim_with_unconfirmed_effect_cannot_be_reassigned`、`two_claimers_cannot_start_two_runs_for_one_packet`；迟到心跳/旧 epoch 结果不能释放或覆盖新 lease。
- **再成功 / 退出**：`confirmed_stopped_attempt_can_be_reclaimed_with_full_history`；所有尝试可查询，预算累计保留；同一 claimant 的幂等重试不把 Reviewing 降回 Running。

<a id="co-20"></a>

<a id="step-co-20"></a>



#### CO-20 · Cell 资源预留、提交与回收闭环　⏳

- **归属**：`P1-C-02`、`P1-K5-01`。
- **依赖**：CO-03、CO-19。
- **代码与产物**：core `cell_registry.rs`、CellRegistryPort、SpawnPlan、BudgetLease、PathLock、CapabilityGrant、SupervisionLease。
- **实现顺序**：①在派发前预留预算、路径、模板与监督资源；②成功写入 admission 决定后才启动 Cell；③partial failure 撤销未生效资源，terminal/retire 幂等撤权，仍不确定的实际费用/副作用保留待对账责任。
- **先拒绝**：`cell_admission_failure_cannot_leak_budget_paths_or_live_grants`；父权限不足、路径重叠、模板过期、监督无效和预算耗尽都不能触发模型/工具调用。
- **再成功 / 退出**：`cell_retirement_releases_only_its_own_resources_once`；两个实例可独立完成，无交叉释放；重启后能定位预留记录，不把进程内 mutex 当 durable 证明。

<a id="co-21"></a>

<a id="step-co-21"></a>



#### CO-21 · 角色任务接入 fresh Run 与明确 Company scope　⏳

- **归属**：`P3-I-02`、`P1-C-03`、`P1-J2-01`。
- **依赖**：CO-04、CO-13、CO-20。
- **代码与产物**：core `spawn_from_packet`/Company StartRun、daemon RequestContext 和 context pack、Runner Start/Continue。
- **实现顺序**：①服务端从 assignment/packet/attempt 派生角色、工作区、路径、预算，并按工作种类校验 Intake/Charter/Plan 前置；②每个新任务用 fresh session，输入仅为冻结产物与授权检索；③Company/standalone mode 显式绑定，不按“目录里碰巧有 Project”推断全部请求的归属。
- **先拒绝**：`company_run_rejects_forged_scope_and_private_planning_context`；Builder 不能通过 direct run、Continue 或 approval 绕过 packet 授权；新角色任务不能继承父模型私有历史。
- **再成功 / 退出**：`department_tasks_share_the_existing_harness_with_isolated_contexts`；分析、实现、验证任务均经过同一 ControlPlane；Receipt 关联全部业务和执行 ID。

<a id="co-22"></a>

<a id="step-co-22"></a>



#### CO-22 · 确定性 Company ProcessManager　⏳

- **归属**：`P2-J5-01`、`P3-I-03`。
- **依赖**：CO-08、CO-16、CO-17、CO-21。
- **代码与产物**：复用 `kiana-workflow/src/durable.rs` 的 plan_command 和 core `automation.rs`；拟新增 `company_process.rs` 作为业务适配层，ProcessState 关联既有 WorkflowInstance。
- **实现顺序**：①核对已有节点，补角色任务、业务验证、人工等待、交接与收尾的 Company 合同；②模板固定版本/hash，在同一纯 planner 上用已记录事件/时间计算下一动作；③每个 activation 生成稳定 intent ID，业务 stream 原子记录意图、workflow stream 幂等消费，不假定跨流事务。
- **先拒绝**：`company_process_cannot_execute_tools_or_complete_on_model_text`；未通过 gate、模板漂移、未知节点/事件不能推进，重放不产生真实模型请求。
- **再成功 / 退出**：`same_business_events_produce_the_same_process_intents`；Planner/Builder/Reviewer/Closer 通过各自任命的受限子实例/Run 切换，调度者不能覆写自身 role 来冒充；Sponsor 节点进入 HumanTask，编排器没有第二模型循环。

<a id="co-23"></a>

<a id="step-co-23"></a>



#### CO-23 · 持久唤醒队列与意图消费　⏳

- **归属**：`P2-J5-01`、`P4-K2-01`、`P2-K6-01`。
- **依赖**：CO-07、CO-19、CO-22。
- **代码与产物**：既有 core `automation.rs` 的 trigger/tick/advance、拟新增 daemon `company_dispatch.rs` 的唤醒 adapter、wake/dispatch 事实与消费 receipt；复用现有队列/触发合同并补事件自动唤醒。
- **实现顺序**：①从已提交事件创建/重建等待队列；②按 intent ID 合并重复通知、claim 后回到 core 重新验权；③通知丢失/daemon 重启可扫描补齐，消费结果与 cursor 持久化，先恢复后新派发。
- **先拒绝**：`duplicate_wakeup_and_crash_after_intent_cannot_duplicate_effects`；无法判断是否已派发时记录 Unknown/reconciliation，不用重试吞掉窗口。
- **再成功 / 退出**：`committed_human_decision_wakes_exactly_one_next_activation_after_restart`；wait key 关联原任务，不重复问已决定的问题；模型调用计数和队列事实可核对。

<a id="co-24"></a>

<a id="step-co-24"></a>



#### CO-24 · 执行结果归集为不可变 EvidenceBundle　⏳

- **归属**：`P3-I-03`、`P2-K4-01`、`P1-J8-01`。
- **依赖**：CO-06、CO-21、CO-23。
- **代码与产物**：core receipt/result ingestion、Artifact manifest、domain EvidenceBundle、Run/Invocation 投影。
- **实现顺序**：①从实际 invocation/receipt 收集命令、退出码、输入/workspace revision、修改文件、产物 hash 与测试结果；②校验 output schema；③发布 EvidenceReady 事实并通知评审，不能仅据 Run Completed 完成 packet。
- **先拒绝**：`evidence_bundle_rejects_model_claims_zero_test_matches_and_foreign_artifacts`；缺退出码、错误源码版本、测试零命中、ResultUnknown 都不能作为通过证据。
- **再成功 / 退出**：`builder_evidence_links_each_output_to_the_actual_run_and_invocation`；可从文件变更追到具体授权与执行回执，输出登记失败保持待补证/对账。

### 批次 D：逐层验收与异常闭环

<a id="co-25"></a>

<a id="step-co-25"></a>



#### CO-25 · 独立 Reviewer 与逐条证据结论　⏳

- **归属**：`P3-I-04`、`P1-E-01`。
- **依赖**：CO-03、CO-11、CO-24。
- **代码与产物**：core `review_author_run`、Company RecordReview、domain ReviewResult/ReviewerAssignment、QA 验证 packet。
- **实现顺序**：①从全部受评输出提取作者 principal/role instance/session 集合；②分配独立 Reviewer，提供冻结标准、diff 与证据；③每个 Criterion 给 Pass/Fail/InsufficientEvidence/NotApplicable、引用与理由，QA 的执行证据独立登记。
- **先拒绝**：`review_denies_every_author_even_after_session_rotation`；多作者中任一作者、标准/证据被替换、只提交总括 LGTM 均不满足；Reviewer 不可修改 Builder 原件。
- **再成功 / 退出**：`independent_review_records_per_criterion_evidence_and_missing_coverage`；review 事实关联精确 target/baseline/evidence revision；不同模型不被误当成身份独立证明。

<a id="co-26"></a>

<a id="step-co-26"></a>



#### CO-26 · Packet 级验收与输出接收　⏳

- **归属**：`P3-I-04`、`P1-D-01`。
- **依赖**：CO-24、CO-25。
- **代码与产物**：domain AcceptanceTarget::Packet、core Request/DecideAcceptance、packet completion 与依赖投影。
- **实现顺序**：①申请时固定 packet revision、有效 attempt、作者集合与 criteria/evidence snapshot；②等待独立 review 与具名 acceptor 决定；③只有所有必需标准通过才形成 packet 验收事实并满足相应依赖。
- **先拒绝**：`packet_acceptance_rejects_run_success_without_review_or_complete_evidence`；NotApplicable/waiver 不静默算通过；接单 ACK 不能冒充工作成果被接受。
- **再成功 / 退出**：`accepted_packet_unlocks_only_its_declared_dependents`；不等其他无关 packet，全程可见尚未满足的标准及接受决定者。

<a id="co-27"></a>

<a id="step-co-27"></a>



#### CO-27 · Milestone 独立验收，消除阶段依赖等待环　⏳

- **归属**：`P3-I-01`、`P3-I-04`。
- **依赖**：CO-12、CO-18、CO-26。
- **代码与产物**：Milestone lifecycle、AcceptanceTarget::Milestone、core Company acceptance 和 ready 查询。
- **实现顺序**：①建立 M2 依赖 M1 Accepted 的两阶段 fixture；②Milestone 申请验收仅消费本阶段必需 packet 与里程碑标准；③通过后记录当前 milestone Accepted，再按统一 ready 解锁 M2，Project 仍可 Active。
- **先拒绝**：`milestone_acceptance_cannot_use_other_milestone_evidence_or_skip_required_packets`；先记录旧 RequestAcceptance 全项目前置导致的等待问题，禁止以删除 M2 依赖“解决”。
- **再成功 / 退出**：`first_milestone_can_be_accepted_before_dependent_milestone_starts`；M1 验收成功时 M2 尚无 Run，之后 M2 才可启动；不遍历改写所有里程碑状态。

<a id="co-28"></a>

<a id="step-co-28"></a>



#### CO-28 · Project 验收、拒绝与显式豁免　⏳

- **归属**：`P3-I-04`、`P2-K3-01`。
- **依赖**：CO-05、CO-25、CO-27。
- **代码与产物**：AcceptanceTarget::Project、完整作者/评审集合、project gate、持久 HumanTask 决定。
- **实现顺序**：①冻结项目级标准、所有必需 milestone acceptance 与整体验证；②独立 Reviewer/授权决策者做接受或拒绝；③豁免只允许授权人类，写具体标准、原因、范围、残余义务与后续处置。
- **先拒绝**：`project_acceptance_rejects_stale_baseline_missing_milestone_and_self_review`；部分交付不自动关闭项目，模型不能批准豁免。
- **再成功 / 退出**：`project_acceptance_aggregates_all_required_milestones_without_overwriting_them`；Accepted、Rejected、Waived 可区分，决定重放稳定，旧评审仍可查看。

<a id="co-29"></a>

<a id="step-co-29"></a>



#### CO-29 · 有界返工与 successor/attempt 历史　⏳

- **归属**：`P3-I-04`、`P1-D-03`、`P3-I-06`。
- **依赖**：CO-19、CO-26、CO-27、CO-28。
- **代码与产物**：PacketAttempt、ReworkPacket、supersedes/rework provenance、拒绝标准关联和依赖解析。
- **实现顺序**：①区分原任务重试与新 successor packet；②记录 rework cause、原拒绝、有效 baseline、最大次数和剩余预算；③重验受影响输出，依赖显式指向有效交付版本，原失败/拒绝记录不删除。
- **先拒绝**：`rework_cannot_change_acceptance_reset_budget_or_revive_a_terminal_attempt`；重复返工、替代链成环、Unknown 未对账和越过尝试上限被拒。
- **再成功 / 退出**：`rejected_packet_can_pass_a_new_review_while_preserving_prior_failures`；多里程碑返工只影响其依赖闭包；旧失败记录不会永久阻塞已明确替代的新版本。

<a id="co-30"></a>

<a id="step-co-30"></a>



#### CO-30 · ChangeRequest 实际应用到整组基线　⏳

- **归属**：`P3-I-01`、`P3-I-04`、`P4-L3-01`。
- **依赖**：CO-07、CO-11、CO-16、CO-28、CO-29。
- **代码与产物**：ChangeImpact、Charter/Plan/Packet baseline publication、core Decide/Implement/VerifyChange、审批和调度 fencing。
- **实现顺序**：①基于旧 baseline 列出范围、标准、预算、依赖、活跃 Run、Review 和 Delivery 的影响；②批准与应用分开，CAS 发布完整新引用；③使受影响旧授权/评审失效，验证新计划；未受影响证据按明确规则复用。
- **先拒绝**：`approved_change_cannot_leave_old_packets_or_approvals_authoritative`；两个变更竞态、部分写入、批准后输入漂移、正在执行但未被 fence 都不能默默应用。
- **再成功 / 退出**：`baseline_change_updates_the_affected_graph_and_preserves_historical_decisions`；不能只修改 Project.scope_baseline/success_criteria 字符串而留下旧 Milestone/Packet 生效。

<a id="co-31"></a>

<a id="step-co-31"></a>



#### CO-31 · 项目暂停、恢复、取消与部门传播　⏳

- **归属**：`P0-J1-01`–`P0-J1-04`、`P3-I-02`、`P2-K6-01`。
- **依赖**：CO-03、CO-19、CO-23、CO-30；现有进程停止确认能力须可用。
- **代码与产物**：Company pause/resume/cancel、Run cancellation、DispatchIntent fence、Cell retire；覆盖 Proposed/Approved/Planned/Active/Review/Delivery 等阶段。
- **实现顺序**：①区分停止新派发与停止已运行工作；②先持久化请求，取消未开始动作并等待已启动效果确认；③Resume 重验 baseline、epoch、预算和 approval，并恢复原等待/健康状态，不能一律写 Active。
- **先拒绝**：`project_cancel_with_unconfirmed_child_never_reports_cancelled_or_closed`；取消后的迟到结果不推进验收；已撤权 session 不得恢复。
- **再成功 / 退出**：`project_pause_and_cancel_propagate_without_affecting_unrelated_projects`；所有受影响任务有确定取消或 Unknown 回执，重复操作幂等，无孤儿 claim。

<a id="co-32"></a>

<a id="step-co-32"></a>



#### CO-32 · 风险、事故、Unknown 与对账工作流　⏳

- **归属**：`P2-K6-01`、`P3-I-01`。
- **依赖**：CO-24、CO-30、CO-31。
- **代码与产物**：Risk/Incident、RecoveryPlan/Reconciliation、run/delivery observation adapter、supervision/status projection。
- **实现顺序**：①Risk trigger 与实际 Incident 分开；②MissingResult、worker lost、证据损坏、预算不确定等生成具名事故和处理期限；③只读检查效果、提出补偿/重试决定，记录后续 reconciliation fact，不修改原 Unknown 终态。
- **先拒绝**：`unknown_effect_cannot_be_retried_or_closed_by_status_report`；错误对象/账户/范围证据、模型自报、未确认停止不算对账完成。
- **再成功 / 退出**：`reconciliation_resolves_the_business_blocker_without_rewriting_original_run`；恢复或失败结果可核验；失败依赖停止后续工作，成功 sibling 证据保留。

### 批次 E：交付与可用入口

<a id="co-33"></a>

<a id="step-co-33"></a>



#### CO-33 · 版本化 DeliveryManifest 与本地交付包　⏳

- **归属**：`P3-I-05`、`P2-K4-01`。
- **依赖**：CO-06、CO-28、CO-32。
- **代码与产物**：Delivery/DeliveryManifest、Artifact content store、core prepare delivery；本地 package handler 通过 Broker 接入。
- **实现顺序**：①引用验收的精确产物集合、版本和 hash；②列出接收者、位置/渠道、确认方式、基线及残余事项；③生成可检查的本地交付目录/归档和 manifest，把 package 完成与真实交接分开。
- **先拒绝**：`delivery_preparation_rejects_unaccepted_changed_or_foreign_artifacts`；空交付、缺件、跨项目、symlink/路径逃逸和陈旧 acceptance 被拒。
- **再成功 / 退出**：`local_delivery_package_matches_the_accepted_manifest`；检查得到的每件产物都能追到验收与原始 Run，channel 明确标 local_package，不声称外部发送。

<a id="co-34"></a>

<a id="step-co-34"></a>



#### CO-34 · 交付授权、效果记录与接收确认　⏳

- **归属**：`P3-I-05`、`P4-K8-01` 的本地交接合同。
- **依赖**：CO-05、CO-07、CO-32、CO-33。
- **代码与产物**：core approve/dispatch/confirm/reconcile delivery、DeliveryConfirmation HumanTask、Broker 本地交付回执。
- **实现顺序**：①批准精确 manifest、destination 和 recipient；②先记 DispatchRequested 后执行，实际效果证据才产生 Delivered；③接收方确认同一版本才 Confirmed，部分/未知交付进入对账，重试查询原决定。
- **先拒绝**：`delivery_cannot_confirm_from_sender_claim_or_repeat_after_unknown_dispatch`；错接收者、过期决定、内容/目标变化、响应丢失不能重发或冒充确认。
- **再成功 / 退出**：`authorized_recipient_confirms_the_exact_delivery_once`；记录本地实际收到的包；外部发布/发送 adapter 如后续加入，必须满足同一合同并另附实跑证据。

<a id="co-35"></a>

<a id="step-co-35"></a>



#### CO-35 · 成功、失败、取消和豁免的 ClosingReceipt　⏳

- **归属**：`P3-I-05`、`P3-I-06`。
- **依赖**：CO-28、CO-31、CO-32、CO-34。
- **代码与产物**：CompanyClosingReceipt、core close/archive、旧 `close_author_run` 适配；项目级多作者/多验收汇总。
- **实现顺序**：①根据 close_kind 汇总目标、计划、packet/attempt、验收、交付、预算、角色与异常；②成功关闭验证所有必需验收和交付确认，失败/取消验证停止与剩余义务；③豁免具名记录，archive 是后续保留动作，不是“结果未知已解决”。
- **先拒绝**：`closing_rejects_missing_delivery_unresolved_effects_and_incomplete_author_set`；Closer 不能自评替代独立证据，失败/取消不得标成功。
- **再成功 / 退出**：`each_close_kind_produces_an_honest_queryable_closing_receipt`；失败、取消与豁免也能合理收尾；原失败事实保留，旧单 Run 收尾不冒充整个项目关闭。

<a id="co-36"></a>

<a id="step-co-36"></a>



#### CO-36 · Outcome 测量与目标实现判定　⏳

- **归属**：`P3-I-05`、`P1-L1-01`。
- **依赖**：CO-09、CO-10、CO-35。
- **代码与产物**：OutcomePlan/MetricObservation/OutcomeAssessment、Company record/assess/achieve commands、可控时钟/数据集 fixture。
- **实现顺序**：①立项时固定测量方法、窗口、采样/聚合、数据来源和 owner；②窗口内追加观测，去重并保留 observed_at/recorded_at；③窗口结束后按规则计算结果，迟到观测形成新 assessment；Sponsor 独立决定 Objective Achieved。
- **先拒绝**：`objective_cannot_be_achieved_from_delivery_tests_or_cherry_picked_observation`；单位错误、过期来源、窗口外数据、NaN、缺样本/缺来源均不宣称实现。
- **再成功 / 退出**：`outcome_assessment_replays_from_frozen_measurement_rules_and_observations`；区分 Realized、PartiallyRealized、NotRealized 和缺数据；fixture 与真实观测明确标记。

<a id="co-37"></a>

<a id="step-co-37"></a>



#### CO-37 · 部门决议、收尾经验与 Memory 候选晋升　⏳

- **归属**：`P4-E-03`、`P4-J3-05`、`P1-J3-03`。
- **依赖**：CO-15、CO-35；`P1-J3-01`–`P1-J3-03` 的候选写入、ACL 和审批已可用。
- **代码与产物**：Decision/Lesson proposal、core memory proposals、daemon distillation、department/role/project collection。
- **实现顺序**：①决议和 ClosingReceipt 的事件触发一次提取；②产出带原始证据、适用范围、密级/期限、相似记录的 candidate；③通过既有准入通道晋升；后续检索带来源和新鲜度，不因索引失败阻断已确认业务交付。
- **先拒绝**：`closing_lesson_cannot_publish_private_scratch_or_change_role_policy`；重复蒸馏、自批、跨项目可见性泄漏、过期决议回灌被拒。
- **再成功 / 退出**：`approved_department_lesson_is_retrievable_with_its_original_decision_evidence`；事实账本、知识候选与批准知识三种状态清楚分开。

<a id="co-38"></a>

<a id="step-co-38"></a>



#### CO-38 · 组织与项目的可重建读模型　⏳

- **归属**：`P2-M2-01`、`P2-M4-01`、`P3-I-03`。
- **依赖**：CO-18、CO-22、CO-28、CO-35、CO-36。
- **代码与产物**：Company snapshot/query DTO、core projection、daemon 查询端口；OrganizationSummary/ProjectDetail/WorkGraph/DeliveryDetail。
- **实现顺序**：①按授权范围投影目标、部门队列、里程碑、包、实例和成果；②每个 blocker 返回原因/责任人/允许动作，每个结果提供证据链接；③分页、cursor、revision、epoch 一致，过期 UI 不能覆盖新事实。
- **先拒绝**：`company_projection_never_leaks_foreign_project_or_advances_from_chat`；同 ID 不同 scope、旧 cursor、伪造进度、自报百分比不能变成权威完成态。
- **再成功 / 退出**：`company_view_rebuilds_with_the_same_blockers_actions_and_evidence_links`；清除缓存后重建等价，部门“工作量”和已验收业务成果分别显示。

<a id="co-39"></a>

<a id="step-co-39"></a>



#### CO-39 · 统一 Human Inbox 与有后续动作的决定卡　⏳

- **归属**：`P2-K3-01`、`P2-M3-01`。
- **依赖**：CO-05、CO-23、CO-28、CO-34、CO-38。
- **代码与产物**：HumanTask 查询/决定协议、持久 task 状态、UI action DTO；Charter/Change/RuntimeApproval/Acceptance/Delivery/Incident 六类动作。
- **实现顺序**：①统一收件箱、目标摘要、精确版本、可选决定与期限；②用户回复回到原命令权威，保存决定并唤醒原 wait key；③按 urgency/等待时长排序，过期和改版卡明确失效并显示替代任务。
- **先拒绝**：`human_inbox_rejects_stale_target_wrong_decider_and_double_consumption`；仅 elapsed time、被预选的按钮或通知已读都不等于批准。
- **再成功 / 退出**：`one_human_decision_is_visible_and_consumed_consistently_across_clients`；重启后待办和既有决定都可查，已回答的问题不再重复询问。

<a id="co-40"></a>

<a id="step-co-40"></a>



#### CO-40 · CLI 与 Workbench 的 Company 用户流程　⏳

- **归属**：`P0-M1-01`、`P2-M3-01`、`P3-I-06`。
- **依赖**：CO-38、CO-39。
- **代码与产物**：entrypoint 路由、`product_command.rs`/workbench、client；拟提供 Company create/inspect/next/decide/delivery/close 用户动作。
- **实现顺序**：①用户输入目标后展示可审查 Charter/Plan 草案；②用服务器 permitted actions 推进，查看状态/证据和处理必要决定；③文本/JSON 输出共用响应，手动和自动过程可切换但不更改权限。
- **先拒绝**：`company_cli_and_workbench_cannot_bypass_missing_business_gate`；自由文本“批准全部”不扩成未展示的未来授权，命令参数不伪造角色。
- **再成功 / 退出**：`user_can_complete_a_local_company_project_without_handwriting_command_envelopes`；可完成立项、计划、执行观察、验收、交付确认与关闭，错误提示说明下一步和责任人。

<a id="co-41"></a>

<a id="step-co-41"></a>



#### CO-41 · Web 与 Desktop 复用同一 Company 状态　⏳

- **归属**：`P2-M2-01`、`P2-M3-01`、`P2-M4-01`、`P2-M5-01`、`P4-M6-01`。
- **依赖**：CO-38、CO-39、CO-40；复用既有 stream cursor/epoch 接线。
- **代码与产物**：`web.rs`/`web_page.html`、workbench 共用 DTO、`contrib/desktop`；项目总览、阶段图、Inbox、证据和交付页。
- **实现顺序**：①订阅 snapshot+events 渲染同一业务对象；②会议讨论/执行进度与权威决定分开显示；③重连补 cursor，按钮由 server actions 驱动，键盘/窄屏/文本回退可用。
- **先拒绝**：`stale_web_action_and_reconnected_delta_cannot_repeat_company_transition`；不可信 Origin/身份、旧审批卡、过期表单都不能触发业务操作。
- **再成功 / 退出**：`cli_workbench_web_and_desktop_show_the_same_company_terminal_and_next_action`；同一项目在四入口对账一致，Desktop 不另存业务事实或另起 Agent 循环。

<a id="co-42"></a>

<a id="step-co-42"></a>



#### CO-42 · 全业务链跨进程恢复与 schema 升级演练　⏳

- **归属**：`P3-I-03`、`P0-F-03`、`P2-J5-01`、`P2-K6-01`。
- **依赖**：CO-08、CO-23、CO-29、CO-30、CO-32、CO-35、CO-39。
- **代码与产物**：Company/Process/HumanTask 投影与恢复消费、磁盘 EventStore/Artifact/Approval fixture、独立进程恢复测试。
- **实现顺序**：①在 intent 提交、派发、模型结果、工具效果、评审、人工决定、交付、收尾边界注入崩溃；②销毁原进程和缓存后重建；③先只读核对，再显式恢复/对账，验证升级后的旧记录读取与待决流程版本固定。
- **先拒绝**：`company_restart_with_missing_result_or_corrupt_evidence_never_reexecutes_blindly`；无授权/续接材料、断裂 stream、旧 assignment、未知 schema 都保持可诊断阻塞。
- **再成功 / 退出**：`new_process_rebuilds_and_resumes_the_company_chain_from_durable_facts`；业务图、责任、预算、等待任务与原 receipt 一致。仅重复构造同进程对象不足以声称跨进程 durable。

### 批次 F：并行、复用与总验收

<a id="co-43"></a>

<a id="step-co-43"></a>



#### CO-43 · 有界多角色/多 Builder 并行　⏳

- **归属**：`P4-J6-01`、`P1-C-02`。
- **依赖**：CO-18、CO-19、CO-20、CO-21、CO-29、CO-42。
- **代码与产物**：复用 domain/core `swarm.rs`、SwarmPlan/Partition/WorkFingerprint/DelegationPacket 与现有 child admission；补 daemon 调度接线与隔离 workspace/worktree adapter。
- **实现顺序**：①固定 parent/root、分区、输入版本、输出合同与 merge owner；②按授权上限预留并发、深度、TTL、预算，写者工作区隔离；③失败按计划传播，成功 sibling 保留证据，未启动依赖取消，全部 child 最终 retire。
- **先拒绝**：`swarm_rejects_overlapping_writers_duplicate_fingerprints_and_excess_delegation`；子权限不超过父/模板/部门/项目/packet/approval 交集，旧 epoch child 不得写回。
- **再成功 / 退出**：`bounded_swarm_completes_independent_packets_with_a_full_responsibility_chain`；观察真实两任务并发、停机与预算释放，不用虚构消息数量或仅校验 SwarmPlan 类型充当证明。

<a id="co-44"></a>

<a id="step-co-44"></a>



#### CO-44 · Integrator、冲突处理与 MergeReceipt　⏳

- **归属**：`P4-J6-01`、`P2-K4-01`、`P3-I-04`。
- **依赖**：CO-25、CO-26、CO-43。
- **代码与产物**：Integrator RoleAssignment、IntegrationPlan/MergeDecision/MergeReceipt、core merge 路径、Artifact/Git adapter。
- **实现顺序**：①基于固定 base 和 child 输出版本生成合并候选；②冲突成为具名集成任务，重新验证组合后行为；③在相应授权下写入目标工作区并保存准确 merge receipt，再进入上层验收。
- **先拒绝**：`integration_rejects_stale_base_unreviewed_outputs_and_hidden_conflict_resolution`；并行单测各自绿不证明合并结果绿，自动 push/发布不能由 MergeReceipt 推出。
- **再成功 / 退出**：`integrated_output_is_revalidated_and_traced_to_all_child_artifacts`；可定位每个 child、集成人、冲突决定、结果版本与测试，merge 成功仍不直接接受项目。

<a id="co-45"></a>

<a id="step-co-45"></a>



#### CO-45 · 多项目优先级、容量与组织成本账　⏳

- **归属**：`P1-K5-01`、`P4-K2-01`；关联 `P3-I-01` 的 Portfolio/Program 领域扩展。
- **依赖**：CO-02、CO-20、CO-23、CO-36、CO-38、CO-43。
- **代码与产物**：ProjectBudget/UsageRecord/Quota、Organization summary、Portfolio/Program 引用、调度优先级/容量策略。
- **实现顺序**：①区分 reserved/spent/released/unknown costs，按 project/role/run/model 归集；②Portfolio 记录目标投资优先级，Program 记录跨项目依赖/风险责任；③同一授权范围内按容量、截止期和 aging 调度，跨项目共享证据需要显式 grant。
- **先拒绝**：`organization_budget_cannot_be_bypassed_by_new_attempt_project_or_role`；预算引用不存在、重复使用费、跨项目权限并集和容量超售被拒。
- **再成功 / 退出**：`two_projects_share_capacity_fairly_with_independent_evidence_and_budgets`；预算耗尽一方不吞掉另一方可用配额，计划成本和实际模型成本不混写为业务收益。

<a id="co-46"></a>

<a id="step-co-46"></a>



#### CO-46 · 版本化流程模板、组织配置升级与第二种业务样例　⏳

- **归属**：`P2-J5-01`、`P4-L3-01`、`P4-L5-01`。
- **依赖**：CO-04、CO-13、CO-22、CO-37、CO-42、CO-45。
- **代码与产物**：ProcessTemplate/RolePack/PolicyProfile version registry、配置提案与回滚记录、coding 与 research-report 模板。
- **实现顺序**：①把角色、过程图、必需产物和 gate 固定为模板版本；②安装/升级只产生配置提案，新流程使用新版本，活跃流程固定旧版本或显式迁移；③跑“只读调研→独立核对→本地报告交付”验证业务通用性。
- **先拒绝**：`template_upgrade_cannot_change_active_process_authority_or_skip_acceptance`；提示词中的 allowed-tools、未受信包、隐含任意脚本/新工具不能形成执行授权。
- **再成功 / 退出**：`coding_and_research_projects_share_governance_with_distinct_output_contracts`；第二模板不需要 Builder 写源码，回滚模板不删除既有业务事实。

<a id="co-47"></a>

<a id="step-co-47"></a>



#### CO-47 · fake-model Company 黄金闭环与故障矩阵　⏳

- **归属**：`P3-I-06`、`P1-L1-01`。
- **依赖**：CO-35、CO-36、CO-37、CO-40、CO-41、CO-42、CO-44、CO-46。
- **代码与产物**：拟新增 `kiana-daemon/tests/company_lifecycle.rs`、core/domain 聚焦 suite、`scripts/company-os-business-smoke.sh`、可重放 cassette 与 expected trace。
- **实现顺序**：①一个目标、两依赖里程碑、三 packet，先拒绝再返工通过；②穿插人工决定、取消/Unknown、重启、并行/集成和本地交付确认；③比较每个命令回执、真实文件、EventLog 和四入口终态，不只检查最后 JSON 的 status。
- **先拒绝**：§19.2 全矩阵场景均有实际断言和非零命中；篡改标准、缺证据、重复交付、错误 owner、故障注入均不能 Closed(success)。
- **再成功 / 退出**：`fake_model_company_project_produces_complete_closing_receipt`；同时覆盖拒绝关闭/取消关闭/豁免/成果未实现，完整 stdout、fixture hash 和源码快照形成证据包。

<a id="co-48"></a>

<a id="step-co-48"></a>



#### CO-48 · 真实模型闭环验证、文档回填与交接　⏳

- **归属**：`P3-I-06`、`P1-L1-01`、`P0-J7-01` 的业务接线验证。
- **依赖**：CO-47；复用已配置且获授权的 live provider 与有界预算。
- **代码与产物**：真实模型运行记录/Receipt、角色 model route 证据、`USER.md`/`module-map.md`/状态账本及本 roadmap 回填。
- **实现顺序**：①在临时受控项目按确定预算跑实际 Planner/Builder/Reviewer/Closer；②对生成代码运行独立验证，人工审查并接收本地交付；③记录 provider/model/版本、角色差异、失败和费用，按证据分别更新功能状态与证明等级。
- **先拒绝**：`live_company_run_stops_on_invalid_proposal_budget_limit_and_unapproved_action` 的实跑场景必须留原始结果；真实模型失败不临时换 cassette 仍标 live。
- **再成功 / 退出**：在有完整证据的有限 provider/model/任务范围内证明 live 业务闭环；缺凭据或未获调用授权时准确记录未完成 live gate，不能用已有单 Run streaming 证据代替。发布、推送和外部交付按当次明确授权另行执行。

---

## 19. 本追加范围的验证、证据与执行交接

### 19.1 每张卡的共同完成要求

1. 先核对当前文件和依赖单元；已有实现通过本卡验收则复用，发现回归先分类，不重复建模块。
2. 新增行为至少有对应拒绝/异常与成功断言；跨模块行为必须通过 `DaemonHost::handle` 或其真实协议入口，不能只调用配置 helper。
3. 每次提交业务事实都检查 target、scope、baseline、authority、幂等和事件关联；测试必须核对副作用计数/实际文件/receipt，而非只断言返回文本。
4. 聚焦测试后运行受影响回归；daemon/control-plane 相关测试串行。CI/提交由当前明确授权决定；无本次 CI 就不填写历史 CI 为当前证明。
5. 保留 v1 合同、迁移 fixture 和历史证据；若本次采用新语义，新版本有独立测试，不能通过改宽旧断言完成升级。
6. 更新本卡、归属单元和 `CURRENT_STATUS.md`，把实现状态与证明等级分开；跨进程持久恢复、真实模型、真实交付各自需要对应证据。

### 19.2 必须覆盖的跨步骤场景

| 场景 | 必须观察的结果 | 主要步骤 |
|---|---|---|
| 未信任项目、伪造 Sponsor、跨项目工件 | 结构化拒绝，无模型/工具效果，owner 不漂移 | CO-02–CO-07、CO-21 |
| 权限撤销发生于批准后/效果前 | 新 epoch 拒绝旧 Grant/Approval/claim，停机可核对 | CO-03、CO-19–CO-21、CO-31 |
| Plan 缺依赖、成环、批次最后一个包非法 | 不留部分批准图，错误给出稳定引用/环路径 | CO-12、CO-16、CO-18 |
| 两人认领、租约过期但旧 worker 存活 | 只一个有效 claim；旧效果未确认时不能重派 | CO-19–CO-20、CO-43 |
| 会议无决议、模型输出注入命令、预算耗尽 | 可诊断终态/升级，未生成未经授权的批准 | CO-14–CO-16 |
| M2 依赖 M1 验收 | M1 可先验收，M2 后开工，Project 仍未完成 | CO-27 |
| 同文不同 Criterion、多作者、零命中测试 | 每条标准与每个作者仍被检查，缺证据拒绝 | CO-11、CO-24–CO-28 |
| 评审拒绝后返工一次 | 新 attempt/review 可通过，旧拒绝和预算消耗保留 | CO-29 |
| 批准后改 Charter/Plan/Packet 或交付内容 | 原审批/验收不匹配新版本，重新进入决策 | CO-30、CO-33–CO-34 |
| 取消与完成竞态、停止无法确认 | 不出现两个矛盾终态，不误报 Cancelled/Closed(success) | CO-31–CO-32 |
| 真实效果后结果落账失败 | Unknown + Incident；只对账，不盲目重复 | CO-23–CO-24、CO-32、CO-34、CO-42 |
| 接收确认错人、重复交付、交付确认缺失 | 拒绝成功关闭；同一逻辑交付不重复执行 | CO-34–CO-35 |
| 缺 Outcome 数据、仅一条挑选的有利样本 | 继续待测/未实现；不自动 Objective Achieved | CO-36 |
| 重启发生于人工决定与下一节点之间 | 原决定只消费一次，补同一 intent，不再询问 | CO-23、CO-39、CO-42 |
| 部门经验检索、权限撤销/数据过期 | 只见授权有效记录，仍能定位原始证据；无私有草稿泄漏 | CO-37 |
| 多项目/多写者/整合冲突 | 工作区、预算与责任独立；合并结果重新验证 | CO-43–CO-45 |
| 新模板/新版程序读取旧事件 | 原历史等价、待决流程版本固定；未知执行 schema 拒绝 | CO-08、CO-42、CO-46 |

### 19.3 实施时使用的验证命令

以下是**后续实现时的命令模板，本次文档调研没有运行 Rust 测试**。先跑本卡新增测试，确认匹配数量大于零；不要把不存在的 test target 当成已有命令。`company_lifecycle` 和 business smoke 由 CO-47 新建。

```bash
# 按当前卡选择相关测试；daemon/control-plane 始终串行
cargo test -p kiana-domain --lib --locked --offline -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-workflow --locked --offline -- --test-threads=1

# 有对应新增目标后执行
cargo test -p kiana-daemon --test company_lifecycle --locked --offline -- --test-threads=1
bash scripts/company-os-business-smoke.sh

# 集成检查与仓库发布门
cargo check --workspace --locked --offline
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
bash scripts/release-smoke.sh
```

每轮记录 fixture、source/workspace revision、命令 argv、命中数、exit code、事件/Artifact/Receipt 引用和限制。失败时区分基线、当前改动、变化中的共享 WIP 与环境问题，不改测试期望来消除冲突。

### 19.4 证据块与本次调研边界

实现 agent 使用下列结构回填；没有实际运行的字段明确写未执行：

```text
step: CO-xx / 对应 P 单元
source_snapshot: HEAD + 相关 WIP 文件 hash
worktree_status: 精确相关文件清单
command_argv: 实际执行的 argv
cwd / environment: 项目根、系统、工具链、必要配置（不含密钥）
fixture / cassette: 输入、hash、时间和故障注入点
exit_code / matched_tests: 每条实际结果
artifacts: RuntimeEvent / EvidenceBundle / Receipt / blob hash
status change: 原 feature_status → 新 feature_status
proof-level change: source / local_behavior / durable / live 的实际范围
limitations: 未验证的路径、依赖、外部效果与恢复限制
reviewer: 实际复核者或未独立复核
```

本次研究基于 `db77c2485bcafecbb1da17ec57ee509ad2ee32b4` 与当日 WIP，只产生研究记录、目标设计和本追加清单；未运行 CompanyOS 代码、未调用 live provider、未运行 CI、未改变任何完成态。新增文档的链接、步骤唯一性与依赖图另做静态检查，其通过不升级运行证明。

下一实施动作是 **CO-01**：与现有 agent 的最新代码做基线交接，然后按依赖核验/推进。首先把 CO-27 的阶段验收等待环列入聚焦场景；不要把本节当成从零重写所有已经存在的 CompanyOS 类型。

---

返回：[路线图总图与当前窗口](../roadmap.md#appendix-navigation) · [文档总入口](../README.md)
