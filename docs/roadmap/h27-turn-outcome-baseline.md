# H27 结构化输出与准确 TurnOutcome 基线

> 快照日期：2026-09-19。本页记录 H27 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、协议和 Core 夹具。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H27`](harness.md#step-h27) |
| feature_status | `implemented`（domain/core/protocol/receipt source；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | Core 依据 pending invocation/approval/clarification/steering/background 状态和可选 OutputContract 生成唯一 TurnOutcome；模型文本不能自报 completed |
| this step does | 新增 versioned OutputContract、确定性 JSON object/type/required 校验与 digest；新增 Answered/Completed/AwaitingInput/AwaitingApproval/Blocked/Cancelled/Failed/ResultUnknown outcome reducer；完成事件由 Core 注入 server-derived `turn_outcome` 后才形成 `run.completed` receipt |
| this step does not | 本切片不宣称所有 RoleSpec/WorkPacket 输出 schema 已迁移为具体 JSON schema、不做有界模型格式修复、不改变 CompanyOS acceptance、全局 `ExecutionStatus::AwaitingInput` wire enum 或 provider-native structured-output live 证据 |

## 1. Contract

`OutputContract` 只描述 bounded object 的 required fields、字段类型和是否允许额外字段；
它不是权限或工单验收契约。`TurnOutcome::decide` 固定拒绝顺序：未知副作用、取消、已知失败、
澄清等待、审批等待、未结 invocation/background/steering，最后才验证输出并允许
`answered`/`completed`。Invalid schema 形成 `failed`，不会形成 completed。

Core 在已有 Runner `Completed` 事实进入 receipt 前调用同一 reducer，并把不可由模型覆盖的
digest-bound outcome 附在结构化结果中；pending clarification 仍沿 H26 的 waiting path，
approval/Company acceptance 不会被 Harness 结果文字替代。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `valid_contract_is_the_only_path_to_completed` | only a valid structured result without outstanding work can be Completed |
| `invalid_final_schema_never_reports_completed` | missing/wrong fields become Failed and retain a deterministic reason |
| `pending_work_and_waiting_state_take_precedence_over_text_done` | question, approval, invocation, background job and steering blockers cannot be hidden by text |
| `unknown_effect_and_cancel_are_never_success` | cancellation and Unknown never map to success |
| `outcome_contract_and_terminal_gate_are_core_owned` | lifecycle/receipt path annotates and records the server-derived outcome before terminal receipt |

## 3. Proof ceiling and handoff

H27 proof ceiling is `source`: typed contract validation, explicit precedence, digest-bound outcome
and Core terminal gate are established. Role/WorkPacket schema resolution, provider-native response
formats, bounded output-repair ModelClient calls, durable cross-process terminal CAS and live/physical
external verification remain open for later H/PD/provider work.
