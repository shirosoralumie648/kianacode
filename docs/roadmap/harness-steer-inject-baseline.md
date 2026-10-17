# H19 Continue / Steer / Inject 产品接线基线

> 快照日期：2026-09-18；2026-10-17 补充三入口产品接线（CLI/TTY/Web，本地不运行测试，CI-only）。本页记录 additive protocol、统一 client/host 路由和下一安全边界输入；运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H19`](harness.md#step-h19) |
| feature_status | `implemented`（protocol/client/daemon/core/runner source；CI-only fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责运行时夹具 |
| focused CI | 手动 workflow [`h19-steer-inject-diagnostic.yml`](../../.github/workflows/h19-steer-inject-diagnostic.yml) 并行运行 protocol wire、runner deferred-steer、Core route guard 与入口接线（`kiana-entrypoints` target `h19_steer_inject_wiring`）；不把未等待的远程结果提升为更高 proof |
| authority | ControlPlane owns run/input identity, current-turn fence and accepted receipt; Runner only queues input at a safe boundary |
| this step does | additive `SteerRequest`/`InjectRequest` and Runner command; core validates source/target/expected turn, records `run.input.accepted`, returns bounded ACK, records a rejected claim when Runner refuses delivery, and queues through the same KianaClient→DaemonHost→ControlPlane spine; in-flight runs use a deferred mailbox; CLI（`--steer`/`--inject`）、Web（`/api/steer`、`/api/inject`）与 TTY Workbench（运行中继续输入 → `next-turn` 注入）都经共享 helper 真正发起该请求 |
| this step does not | Continue v1 public return values are unchanged; Steer cannot alter sandbox/model/active tool parameters and never cancels work; Inject does not wake an idle turn; Web 页面 JS 尚未加入 steer/inject 交互元素；CLI/TTY 依赖会话内可见的 run id（TTY 无 turn-id 投影，因此走 `next-turn`）；durable cross-process Inbox/projector and live provider proof remain open |

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

Product surfaces call the shared helpers (`steer_envelope*` / `inject_envelope*` in
`harness_run.rs`) instead of ad-hoc wiring: CLI exposes `--steer <run_id> --turn <turn_id>` and
`--inject <run_id> [--turn] [--target] [--source]`; Web exposes token-guarded `POST /api/steer`
and `POST /api/inject` whose request bodies reject unknown fields and whose server fixes the
`web` source label; the TTY Workbench queues plain text typed during a running turn as a
`next-turn` inject from source `tty` and renders the ACK without ending its local running state.
Steer/inject ACKs never carry sandbox or model inputs, and malformed Web requests are rejected
before they can consume a replay key.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `steer_cannot_change_sandbox_or_model_profile` | steer protocol/core path has no sandbox/model mutation input and only forwards a bounded inbox message |
| `stale_turn_steer_does_not_target_next_turn` | expected turn mismatch returns `stale_turn_steer`; steer target remains next-step |
| `steer_during_stream_is_seen_by_next_step_without_duplicate_prompt` | an input arriving while the first model call is in flight appears exactly once in the next model request and does not create a duplicate prompt |
| `entrypoints_route_steer_and_inject_through_the_shared_spine` | CLI flags、Web 路由与 TTY 注入都调用共享 helper，且 TTY 运行中输入走 `next-turn`，不另起 run |
| `steer_inject_wiring_carries_no_permission_mutation_and_rejects_before_claiming` | Web 请求体拒绝未知权限字段；畸形请求在占用幂等槽之前被拒；排队回执不结束本地 running 状态 |
| `cli_steer_with_stale_turn_is_rejected_without_accepted_fact` | 过期 turn 的 steer 返回 `stale_turn_steer` 且不新增 `run.input.accepted`、不落任何含该文本的事实 |
| `cli_steer_without_turn_flag_fails_before_any_daemon_request` | 缺少 `--turn` 在入口参数层失败（`steer_turn_required`），不产生事件 |
| `cli_target_or_source_without_inject_is_rejected` | 单独出现 `--target`/`--source` 被拒（`inject_options_require_inject`），不静默忽略 |
| `cli_steer_on_current_turn_is_queued_and_records_accepted_fact` | 当前 turn 的 steer 经共享脊柱返回 `queued` ACK，并写入 `source=steer` 的 `run.input.accepted` |
| `cli_inject_queues_source_labelled_input_for_next_turn` | 显式 `--target next-turn --source cli-check` 的 inject 返回 `queued` ACK 并按该来源/目标写入 accepted 事实 |

## 3. Proof ceiling and handoff

H19 proof ceiling is `source`: additive wire types, shared routing, current-turn fencing, the
in-flight deferred queue and the three entrypoint wirings are established. Durable accepted/claimed
atomicity across process death, Artifact-backed oversized input, cross-process recovery,
provider-native stream guarantees and live/physical proof remain for H20+, PD/ER and provider work.
