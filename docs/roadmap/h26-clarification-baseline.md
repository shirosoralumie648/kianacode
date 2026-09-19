# H26 澄清请求与权限审批分离基线

> 快照日期：2026-09-19。本页记录 H26 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、协议、Runner 和 Core 夹具。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H26`](harness.md#step-h26) |
| feature_status | `implemented`（domain/core/runner/protocol/UI projection source；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | 澄清由 InteractionId + RunId + TurnId + StepId 绑定；Core 校验并产出原 step continuation，CapabilityApproval 仍只由既有 approval binding 消费 |
| this step does | 新增 question/answer/resolution/wait versioned contracts、角色/选项/必答/过期/取消/重复/foreign fence；Runner 的 AwaitingInput 只接受同一 InteractionId 一次；统一 Human Inbox 投影和 `run.clarification.answer` regular-input command；checkpoint 同时保留 driver 与 pending clarification，因此等待身份随原 run 材料恢复 |
| this step does not | 本切片不宣称 live provider、跨进程 EventLog projector 或真实 UI 状态已运行；模型侧“按角色注册澄清能力”的具体 provider wire 适配、持久 question event hydration、`waiting_for_input` 全局 TurnOutcome 映射留给后续 H27/PD/入口接线 |

## 1. Contract

`ClarificationRequest` 是待回答问题，不复用 `HumanTask`/`ApprovalBinding`。它包含目标
turn、可选 step、问题文本、可选项、required、绝对过期时间、cancel policy 和 responder
roles，并以 immutable request digest 作为 answer binding。`ClarificationAnswer` 只携带
InteractionId、run/turn/step、文字或已注册 option、回答者/角色/入口和 request digest；它
没有 approval ID、grant、capability request 或 tool arguments。

Core 的 commit boundary 先验证 answer，再返回 `ClarificationResolution`，明确
`resume_original_step=true` 且 `waiting_for_input=false`。普通回答因此不会形成 capability
permit，也不会改变模型工具目录。Runner state driver 在等待期间只记录一个
`pending_interaction_id`；foreign 或 duplicate answer 不能触发第二次 resume。

TTY 与 Web 继续通过 `human.inbox`/`human.resolve` 公共投影显示 question kind，CLI/非交互
调用可使用同一 `run.clarification.answer` command 和 actionable interaction ID；选项没有
默认答案，required question 必须等回答、cancel 或 expiry。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `question_answer_cannot_consume_tool_approval` | domain/core/protocol contracts have no approval or capability authority fields and the Core path never calls approval/broker |
| `duplicate_or_foreign_answer_does_not_resume_twice` | immutable answer binding and Runner pending interaction fence reject foreign/duplicate answers |
| `answer_after_restart_reaches_original_model_step` | runner checkpoint retains the driver/inbox material and resolution preserves the original run/turn/step target |
| `tty_and_web_use_the_same_human_inbox_projection` | both surfaces continue to query the shared `human.inbox` projection and render `question` |

## 3. Proof ceiling and handoff

H26 proof ceiling is `source`: typed contracts, deny-first transition helpers, protocol command
shape, Runner state fence and shared UI projection are established. Durable answer/question facts,
cross-process hydration, provider-native clarification registration, global `waiting_for_input`
TurnOutcome and live human identity remain open for H27, PD/ER and later entrypoint/provider work.
