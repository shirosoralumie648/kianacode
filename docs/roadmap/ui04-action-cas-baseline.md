# UI-04 Action CAS / idempotency / response-loss baseline

> 快照日期：2026-09-24。UI-04 的实现与拒绝路径由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-04`](ui-entrypoints.md#step-ui-04) |
| source snapshot | `561066fa`（P4-J7-21 合并后的 master） |
| feature_status | `partial`（domain journal + ControlPlane facade source; authority/permit binding and product-chain behavior remain open） |
| proof_level | `local_behavior` for the focused in-memory domain journal fixtures; no Core/EventStore, durable/live/physical proof |
| canonical path | UI command → UiActionCommand validation → ControlPlane `admit_ui_action` → EventStore CAS → effect receipt → `apply_ui_action` / reject / Unknown → `query_original_ui_action` |

UI-04 固定 command ID、idempotency key、target、owner/scope、authority epoch、cursor、target revision、deadline、payload digest 和 command digest。Applied record 必须携带合法 receipt digest，Accepted/Rejected/Unknown 不得携带 receipt；取消中的 action 或缺失 permit 在 ControlPlane admission 处拒绝。当前 `UiActionAuthoritySnapshot.permit_digest` 只检查存在且格式有效，没有绑定进 `UiActionCommand`/journal digest；这项 roadmap 要求仍未完成。`UiActionJournal` 只做确定性状态折叠，不执行副作用；ControlPlane action facade 是唯一持久化入口。

Authority snapshot digests now use the same strict `sha256:` plus 64 hexadecimal character
shape as the command/record contracts. A malformed permit digest is rejected before admission;
this lexical fence does not bind the permit value into the command or journal digest.

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

CI runs `cargo fmt --all --check`, domain UI-04 fixtures, core source guard and workspace test-target compile. Current-head local domain and Core guard receipts are recorded below; remote CI result is not awaited.

## 2026-10-06 focused local receipt

The stale Core guard was aligned with the baseline's `permit` marker and its no-second-loop check
was scoped to the actual UI action bridge wrappers, avoiding unrelated `CapabilityBroker::new`
composition elsewhere in `DaemonHost`.

```text
source_snapshot: current dirty worktree; domain UI action journal, Core facade/source guard and DaemonHost UI bridge
worktree_status: UI-04 remains 🔄 / partial; guard-only scope correction, no permit digest binding or Core/EventStore behavior implementation
command_argv: bounded serial cargo test -p kiana-domain --test ui04_action_journal --locked --offline -- --test-threads=1; cargo test -p kiana-core --test ui04_action_journal_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB stack, stripped/non-incremental profile; domain max RSS 83188 KiB, Core guard 133552 KiB, swaps 0
fixture·cassette: domain 4/4 in-memory journal state/receipt/idempotency fixtures; Core source guard 1/1
exit_code: both commands 0; logs/resources `/tmp/kiana-roadmap-20261006-ui04-domain-final.{log,resources}` and `/tmp/kiana-roadmap-20261006-ui04-core-final.{log,resources}`
status_change: selected domain/guard behavior is local_behavior; whole UI-04 card remains partial/source
proof-level_change: selected fixtures/guard source → local_behavior; no durable/live/physical promotion
limitations: action permit digest is not bound into command/journal digest; no Core/EventStore CAS, response-loss restart or external effect test; original-action query lacks owner/scope arguments and must remain internal until authorized; durable/cross-process and human identity proof remain open
reviewer: integration agent with UI-04 read-only audit; no human release reviewer
```

## 2026-10-08 authority digest lexical fence

```text
source_snapshot: current dirty worktree; kiana-core/src/ui_actions.rs; kiana-core/tests/ui04_action_authority.rs; kiana-core/tests/ui04_action_journal_guard.rs
worktree_status: UI-04 remains 🔄 / partial; authority `scope_digest` and `permit_digest` now reject non-hex payloads before admission, while permit binding and Core/EventStore response-loss behavior remain open
command_argv: RED `cargo test -p kiana-core --test ui04_action_authority authority_rejects_non_hex_permit_digest_before_admission --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-core --test ui04_action_authority --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test ui04_action_journal_guard --locked --offline -- --test-threads=1`; existing domain `cargo test -p kiana-domain --test ui04_action_journal --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB stack, stripped/non-incremental profile; RED max RSS 318568 KiB, GREEN 1193608 KiB, Core guard expected 132136 KiB, all observed swap counters 0
fixture·cassette: forged permit digest `sha256:` plus 64 `g` characters; RED old validator returned `Ok(())`; GREEN returns `ui_action_permit_digest_invalid`; source guard requires `valid_digest` and `is_ascii_hexdigit`
exit_code: RED 101 expected; final GREEN/guard/domain commands 0 after baseline marker alignment
status_change: authority digest lexical denial is local_behavior; UI-04 row/card remains 🔄 / partial
proof-level_change: selected validator fixture/guard source → local_behavior; no durable/live/physical promotion
limitations: lexical validation does not bind permit digest into `UiActionCommand`/journal digest; no Core/EventStore CAS, response-loss restart, cross-process owner/scope query authorization or external effect receipt
reviewer: root integration agent with UI-04 read-only audit; no human release reviewer
```
