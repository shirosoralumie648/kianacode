# H19 Continue / Steer / Inject 产品接线基线

> 快照日期：2026-09-18。本页记录 additive protocol、统一 client/host 路由和下一安全边界输入；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H19`](harness.md#step-h19) |
| feature_status | `implemented`（protocol/client/daemon/core/runner source；CI-only fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责运行时夹具 |
| authority | ControlPlane owns run/input identity, current-turn fence and accepted receipt; Runner only queues input at a safe boundary |
| this step does | additive `SteerRequest`/`InjectRequest` and Runner command; core validates source/target/expected turn, records `run.input.accepted`, returns bounded ACK, records a rejected claim when Runner refuses delivery, and queues through the same KianaClient→DaemonHost→ControlPlane spine; in-flight runs use a deferred mailbox |
| this step does not | Continue v1 public return values are unchanged; Steer cannot alter sandbox/model/active tool parameters and never cancels work; Inject does not wake an idle turn; durable cross-process Inbox/projector and live provider proof remain open |

## 1. Contract

Continue remains the existing compatibility command and explicit new-turn/resume semantics are not
changed. Steer carries an `expected_turn_id` and is normalized to `next-step`; a stale turn is
rejected instead of silently creating or targeting a later turn. Inject carries a source and an
explicit `next-step`/`next-turn` target, is bounded before admission, and is queued without waking an
idle run. Neither request carries sandbox or model-profile mutation fields.

All surfaces use the versioned protocol and the same client/daemon/core route. ControlPlane records
the accepted input before forwarding the Runner command; a Runner rejection is recorded as a
rejected claim. A run temporarily removed from the active map remains addressable through the
in-flight registry and receives the message at the next model-step boundary. The queue is not a
second execution loop and cannot authorize a capability.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `steer_cannot_change_sandbox_or_model_profile` | steer protocol/core path has no sandbox/model mutation input and only forwards a bounded inbox message |
| `stale_turn_steer_does_not_target_next_turn` | expected turn mismatch returns `stale_turn_steer`; steer target remains next-step |
| `steer_during_stream_is_seen_by_next_step_without_duplicate_prompt` | an input arriving while the first model call is in flight appears exactly once in the next model request and does not create a duplicate prompt |

## 3. Proof ceiling and handoff

H19 proof ceiling is `source`: additive wire types, shared routing, current-turn fencing and the
in-flight deferred queue are established. Durable accepted/claimed atomicity across process death,
Artifact-backed oversized input, cross-process recovery, provider-native stream guarantees and
live/physical proof remain for H20+, PD/ER and provider work.
