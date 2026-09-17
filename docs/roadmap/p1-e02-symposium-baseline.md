# P1-E-02 Symposium 会议对象契约化基线

> 快照日期：2026-09-18。本页回填 Symposium/DecisionRecord 与 durable 决议事件；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-E-02`](../roadmap.md#step-p1-e-02) |
| feature_status | `implemented`（domain/core/daemon/protocol source；CI-only symposium fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | ControlPlane/EventLog owns `symposium.closed` and DecisionRecord facts; speaker transcript/blackboard is input evidence only |
| this step does | Symposium validates department/chair/attendee boundaries and bounded rounds, runs existing Harness speakers or explicit anti-meeting, closes a typed DecisionRecord/optional WorkPacket, writes department artifact and durable `symposium.closed` event; replay reads the event stream rather than trusting UI text |
| this step does not | 不让 Builder 参加规划/监控会议、不把投票等同于 approval、不把 artifact 文件或 blackboard 作为第二事实源；部门 RAG and cross-process projector remain P4-E-03/PD/ER |

## 1. Contract

Planning meetings require the PM chair and workspace-write scope; executing/monitoring/closing
meetings keep their department role/attendee contracts. Each speaker gets a bounded private session
through the existing Runner path. Closing preserves claims, votes, draft alternatives and evidence
in a `DecisionRecord`, then appends `symposium.closed` under the request's EventLog stream and writes
the redacted decision artifact. A replay consumer can reconstruct the decision from the event.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `symposium_decision_is_durable_and_replayable` | typed Symposium/DecisionRecord close path appends `symposium.closed` and leaves an event-stream replay source |
| `symposium_rejects_invalid_chair_and_preserves_decision_alternatives` | chair/attendee boundaries reject invalid meetings while blackboard alternatives remain part of the decision contract |
| `convene_two_rounds_uses_private_speaker_sessions` | existing runtime regression keeps speaker sessions isolated and deterministic |
| `each_department_anti_meeting_writes_its_own_artifact` | existing runtime regression writes department-specific decision artifacts through the same core path |

## 3. Proof ceiling and handoff

P1-E-02 proof ceiling is `source` plus existing CI runtime fixtures: Symposium/DecisionRecord
contracts, boundary checks, durable close event and artifact projection are explicit. Durable
cross-process decision projector, department memory/RAG admission, external human decisions and
full replay recovery remain P4-E-03/PD/ER/SC work.
