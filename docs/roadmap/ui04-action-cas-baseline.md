# UI-04 Action CAS / idempotency / response-loss baseline

> 快照日期：2026-09-24。UI-04 的实现与拒绝路径由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-04`](ui-entrypoints.md#step-ui-04) |
| source snapshot | `a2aeae081d27`（P4-J7-20 合并后的 master；2026-10-17 completion slice） |
| feature_status | `implemented`（domain journal + ControlPlane facade + daemon surface adapter；源码与 CI 夹具接线完成，未等待 CI 回执） |
| proof_level | `source`（本步新增夹具依赖 GitHub Actions 编译/运行；历史 2026-10-06/10-08 的 in-memory domain/guard 回执为 `local_behavior`，不覆盖本步结论） |
| canonical path | UI command → UiActionCommand validation → ControlPlane `admit_ui_action` → EventStore CAS → effect receipt → `apply_ui_action` / reject / Unknown → `query_original_ui_action_for`（owner/scope 授权） |

UI-04 固定 command ID、idempotency key、target、owner/scope、authority epoch、cursor、target revision、deadline、payload digest 和 command digest。Applied record 必须携带合法 receipt digest，Accepted/Rejected/Unknown 不得携带 receipt；取消中的 action 或缺失 permit 在 ControlPlane admission 处拒绝。2026-10-17 completion slice 把 `UiActionAuthoritySnapshot` 移入 Domain 合同并随记录持久化：epoch/cursor/revision/owner/scope/permit 基础进 `record_digest`，permit 有意不进 `command_digest`（重发可换新 permit 而不改变命令身份）。`UiActionJournal` 只做确定性状态折叠，不执行副作用；ControlPlane action facade 是唯一持久化入口。

Authority snapshot digests now use the same strict `sha256:` plus 64 hexadecimal character
shape as the command/record contracts. A malformed permit digest is rejected before admission;
the permit basis is persisted into the record and covered by the record digest (2026-10-17
completion slice), while its deliberate exclusion from the command digest keeps a re-issued
permit on the same command identity for retries.

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `action_journal_replays_exact_key_without_second_effect` | 相同 key+digest 返回原记录；重放不增加 `effect_count`，Applied 只计一次 |
| `action_journal_rejects_changed_digest_owner_scope_and_cas` | payload/digest、owner、epoch、cursor、revision 不匹配拒绝 |
| `unknown_requires_original_key_and_never_becomes_applied` | ACK 丢失/Unknown 保持 Unknown；不得用新 ID 自动重做；只能 query original |
| `action_record_receipt_digest_is_bound_to_applied_state` | 缺失、非法或出现在非 Applied 状态的 receipt digest 拒绝 |
| `new_admission_requires_permit_and_live_target`（domain） | 缺 permit / 目标取消中 / 已过 deadline 的新命令拒绝且不产生记录 |
| `accepted_record_binds_the_admitting_authority_and_permit`（domain） | epoch/cursor/revision/owner/scope/permit 进记录且 record digest 覆盖；permit 不进 command digest |
| `resubmit_policy_never_authorizes_a_new_command_id`（domain） | Accepted/Unknown → QueryOriginal，Applied/Rejected → DoNotRetry；无换新 ID 自动重做档 |
| `duplicate_submission_returns_the_original_record_without_a_second_effect`（core） | 重复请求返回原记录、零追加 accepted 事实、Applied 只记一次 effect |
| `changed_payload_under_the_same_key_is_refused_before_any_event_is_written`（core） | 同 key 换 payload 在写事件前拒绝 |
| `reusing_a_committed_command_id_under_a_new_key_is_refused`（core） | 已提交 command ID 换新 key 复用 → `ui_action_command_id_reused` |
| `lost_ack_commit_is_unknown_and_only_the_original_key_converges`（core） | 提交已落盘但只丢 ACK → `result_unknown:ui_action_accept_unconfirmed`；原键查询收敛，重发零追加事实 |
| `concurrent_duplicate_admissions_commit_exactly_one_accepted_fact`（core） | 并发两个 command ID 抢同一 key：恰一条 accepted 事实，败者结构化拒绝 |
| `missing_permit_and_mismatched_authority_are_refused_without_events`（core） | 缺 permit / 取消中 / 越权 owner 或 scope 全部写前拒绝 |
| `original_query_hides_records_from_other_owners_and_scopes`（core） | 授权查询拒绝他 owner/scope（`ui_action_query_*`），空 owner 拒绝，缺记录返回 `None` |
| `restart_reloads_the_applied_record_and_refuses_a_second_effect`（core） | JSONL 重开日志模拟进程重启：同记录同一 digest，重放不追加 accepted/applied |
| `adapter_binds_server_owned_owner_scope_permit_and_recomputes_digests`（daemon） | 服务端持有 owner/scope/permit；自报 `submitted_by` 不一致拒绝 |
| `adapter_refuses_missing_permit_foreign_owner_missing_deadline_and_forged_digest`（daemon） | 缺 permit / 外来 owner / 缺 deadline / 伪造 payload digest 全部拒绝 |
| `daemon_host_admits_applies_and_reconciles_the_original_record`（daemon） | 组合根 admit→重复→apply→授权 query 闭环，跨 scope 查询拒绝 |
| `ui04_action_journal_guard` | ControlPlane 使用 EventStore `commit_transition`、CAS 和结构化 Unknown，daemon 不创建第二执行循环 |

## 3. Durable boundary and limitations

每个 Accepted/Applied/Rejected/Unknown 状态都作为 `ui_action` aggregate 的追加事实写入 EventStore，带 stream version、event idempotency key 和原始 record digest。CAS 冲突拒绝；EventStore 返回 Unknown 时不升格 Applied，调用方必须按原 idempotency key 查询。

2026-10-17 completion slice 之后，admission 快照随记录持久化，查询是 owner/scope 授权的（`query_original_ui_action_for`），并且同一个 command ID 不能跨 idempotency key 复用。当前证据仍是源码 + CI 夹具；GitHub workflow 未等待结果，本卡 proof level 维持 `source`。UI-04 不实现跨进程 socket、snapshot projector、feed replay、通知送达、外部 effect receipt 或人工身份认证；这些留给 UI-05+、NM、PD、ER、SC。旧 `RunStreamBus::claim_ui_action` 保留为轻量显示预条件，不能替代 ControlPlane journal。真实 UI effect receipt、多进程并发与 live 界面路径仍开放。

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

## 2026-10-17 completion slice (CI-only)

本切片把卡片的四类验收补齐到源码 + CI 夹具层面：

1. **授权基础随记录持久化**。`UiActionAuthoritySnapshot` 从 Core 移入 `kiana-domain/src/ui_action.rs`（Core 保持 `kiana_core::UiActionAuthoritySnapshot` re-export）；`accept` 只接受可信快照，`UiActionRecord` 增 `authority_epoch`/`authority_cursor`/`authority_revision`/`permit_digest` 并纳入 `record_digest`，新准入要求 permit、取消中或越权（owner/scope）写前拒绝。
2. **command ID 复用围栏**。`admit_ui_action` 先读 command receipt：同一 command ID 已绑定到另一个 idempotency key 时以 `ui_action_command_id_reused` 拒绝，而不是让旧命令在新 key 下二次准入。
3. **owner/scope 授权查询**。未授权的 `read_original_record` 保持私有；对外 `query_original_ui_action_for(key, owner, scope)` 校验空 owner 与 scope 不匹配（`ui_action_query_owner_invalid`/`ui_action_query_scope_invalid`/`ui_action_query_scope_mismatch`），DaemonHost 用自己的已认证主体调用。
4. **显式重投策略**。`UiActionResubmitPolicy::{QueryOriginal, DoNotRetry}`：Accepted/Unknown 只能按原键查询，Applied/Rejected 不再驱动；代码中不存在「换新 ID 自动重做」的分支。UI effect 计数沿用 `effect_count <= 1` 与 Applied 只写一次。

```text
source_snapshot: 分支 step/ui-04-20261017-1（基线 master a2aeae081d27，P4-J7-20 合并点）；kiana-domain/src/ui_action.rs、kiana-domain/tests/ui04_action_journal.rs、kiana-core/src/ui_actions.rs、kiana-core/src/lib.rs、kiana-core/tests/ui04_action_cas_behavior.rs、kiana-daemon/src/ui_action.rs、kiana-daemon/src/lib.rs、kiana-daemon/tests/ui04_action_adapter.rs、scripts/ci/test-shards.json、docs/roadmap.md §1.1 行 601 与 §3 变更日志、docs/roadmap/ui-entrypoints.md 卡、本文件
worktree_status: 行 601 🔄 → ✅（CI pending）、卡标题 🔄 → ✅；只改本 Step 相关源码/夹具/分片并单点回填共享文档；`UiActionV1 → UiActionCommand` 适配器与授权查询都在既有 DaemonHost→ControlPlane→EventStore 脊柱内，未新增第二执行循环、未改五个模型可见工具面、未触碰冻结项、无新依赖、未改 Cargo.toml/Cargo.lock/package.json
command_argv: CI-only；本地未运行 cargo build/test/check/clippy/run、make、npm、smoke 或验收脚本；只运行 `cargo fmt --all`（允许项，无基线遗留文件被带入）；静态核对用 rg/cat/sed/ls/git status/git diff；用 Python 复刻 scripts/ci/validate-workflows.sh 的分片覆盖规则（`kiana-core` 全部 tests/*.rs 恰被一个分片认领，`kiana-daemon` 为 targets=null 整 crate 分片）；`python3 scripts/roadmap-goal.py` 只读派生索引；CI 接线：统一 ci.yml 的 `kiana-core-s6/6`（新增 `ui04_action_cas_behavior`）、`kiana-domain-s4/4`（`ui04_action_journal`，既有）、`kiana-daemon` 整 crate 分片（新 target 自动发现）、`kiana-core-s6/6` 的 `ui04_action_journal_guard`
cwd·environment: worktree /workspace/kianacode-worktrees/UI-04，分支 step/ui-04-20261017-1；Linux x86_64；GitHub Actions ubuntu-latest 为测试权威（回执未等待）；本地只做静态核对
fixture·cassette: 先拒绝（core）：`changed_payload_under_the_same_key_is_refused_before_any_event_is_written`（写前拒绝、流仍只有 1 条 accepted）、`reusing_a_committed_command_id_under_a_new_key_is_refused`（`ui_action_command_id_reused`，新 key 流为空）、`missing_permit_and_mismatched_authority_are_refused_without_events`（缺 permit/取消中/外来 owner/外来 scope 四类，零事件）、`original_query_hides_records_from_other_owners_and_scopes`（他 owner/scope 拒绝、空 owner 拒绝、缺记录 None）；daemon：`adapter_refuses_missing_permit_foreign_owner_missing_deadline_and_forged_digest`（`ui_action_permit_required`/`ui_action_owner_mismatch`/`ui_action_deadline_required`/`ui_action_payload_digest_mismatch`）与 `adapter_binds_server_owned_owner_scope_permit_and_recomputes_digests` 中的 `ui_action_submitter_mismatch`；domain：`new_admission_requires_permit_and_live_target`、`resubmit_policy_never_authorizes_a_new_command_id`（Unknown 之后 reject 返回 `ui_action_terminal_conflict`）、`unknown_requires_original_key_and_never_becomes_applied`。再成功/回归：`duplicate_submission_returns_the_original_record_without_a_second_effect`（accepted/applied 各 1 条、effect_count 1，重放同 digest）、`lost_ack_commit_is_unknown_and_only_the_original_key_converges`（`LostAckStore` 提交已落盘只丢一次 ACK → `result_unknown:ui_action_accept_unconfirmed`，原键查询得 Accepted + QueryOriginal，重发零追加）、`concurrent_duplicate_admissions_commit_exactly_one_accepted_fact`（tokio::join! 两个 command ID；恰一个成功、恰 1 条 accepted、effect_count 0）、`restart_reloads_the_applied_record_and_refuses_a_second_effect`（JsonlEventLog 重开模拟进程重启：同 record_digest，重放不追加）、`daemon_host_admits_applies_and_reconciles_the_original_record`（组合根 admit→duplicate→apply→授权 query 闭环，跨 scope 查询拒绝）
exit_code: 本地未运行；未编译/未执行，退出码未知；CI 回执未等待
status_change: 卡片「步骤/先拒绝/成功回归」在源码层全部覆盖（授权快照绑定、CAS、幂等、ACK 丢失、进程重启、重复请求、并发 CAS、新旧 payload、原键查询收敛、禁止换 ID 重做）；行 601 由 🔄 提升 ✅（CI pending）/ feature_status=implemented
proof-level_change: 维持 source；历史 2026-10-06/10-08 回执只覆盖当时的 domain journal 与守卫（local_behavior），本切片未本地编译/运行，不提升 durable/live/physical
limitations: 本地未编译未运行，编译与运行正确性依赖 GitHub Actions；`LostAckStore`/`JsonlEventLog` 夹具模拟的是进程内与单进程重启的 ACK 丢失，不是真实多进程 socket 竞争；`query_original_ui_action_for` 的 owner/scope 由调用方（组合根）提供，身份仍来自本地 `AuthenticatedPrincipal::local()`，未证明远程/多用户身份；真实 UI effect receipt、跨进程并发、artifact/feed 投影与 live 界面路径仍属 UI-05+ 与其他专项
reviewer: root integration agent；domain 记录/摘要与重投策略、Core 准入/围栏/授权查询、daemon 适配器与组合根、CI 分片接线及 roadmap/baseline/CURRENT_STATUS 回填的静态复核
```
