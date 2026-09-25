# UI-04 Action CAS / idempotency / response-loss baseline

> 快照日期：2026-09-24。UI-04 的实现与拒绝路径由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-04`](ui-entrypoints.md#step-ui-04) |
| source snapshot | `561066fa`（P4-J7-21 合并后的 master） |
| feature_status | `implemented`（domain journal + ControlPlane facade + daemon adapter source） |
| proof_level | `source`；GitHub-only fixture wiring，未提升为 local_behavior/durable/live/physical |
| canonical path | UI command → UiActionCommand validation → ControlPlane `admit_ui_action` → EventStore CAS → effect receipt → `apply_ui_action` / reject / Unknown → `query_original_ui_action` |

UI-04 固定 command ID、idempotency key、target、owner/scope、authority epoch、cursor、target revision、deadline、permit、payload digest 和 command digest。Applied record 必须携带合法 receipt digest，Accepted/Rejected/Unknown 不得携带 receipt；取消中的 action 或无 permit 在 ControlPlane admission 处拒绝。`UiActionJournal` 只做确定性状态折叠，不执行副作用；ControlPlane action facade 是唯一持久化入口。

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `action_journal_replays_exact_key_without_second_effect` | 相同 key+digest 返回原记录；重放不增加 `effect_count`，Applied 只计一次 |
| `action_journal_rejects_changed_digest_owner_scope_and_cas` | payload/digest、owner、epoch、cursor、revision 不匹配拒绝 |
| `unknown_requires_original_key_and_never_becomes_applied` | ACK 丢失/Unknown 保持 Unknown；不得用新 ID 自动重做；只能 query original |
| `action_record_receipt_digest_is_bound_to_applied_state` | 缺失、非法或出现在非 Applied 状态的 receipt digest 拒绝 |
| `ui04_action_journal_guard` | ControlPlane 使用 EventStore `commit_transition`、CAS 和结构化 Unknown，daemon 不创建第二执行循环 |

## 3. Durable boundary and limitations

每个 Accepted/Applied/Rejected/Unknown 状态都作为 `ui_action` aggregate 的追加事实写入 EventStore，带 stream version、event idempotency key 和原始 record digest。CAS 冲突拒绝；EventStore 返回 Unknown 时不升格 Applied，调用方必须按原 idempotency key 查询。

当前证据只覆盖源码和 CI 夹具；GitHub workflow 未等待结果。UI-04 不实现跨进程 socket、snapshot projector、feed replay、通知送达、外部 effect receipt 或人工身份认证；这些留给 UI-05+、NM、PD、ER、SC。旧 `RunStreamBus::claim_ui_action` 保留为轻量显示预条件，不能替代 ControlPlane journal。

## 4. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

CI runs `cargo fmt --all --check`, domain UI-04 fixtures, core source guard and workspace test-target compile. No local tests/build/check/clippy/smoke are run; CI result is not awaited.
