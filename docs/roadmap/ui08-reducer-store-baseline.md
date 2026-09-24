# UI-08 共享 reducer/entity store baseline

> 快照日期：2026-09-24。UI-08 的共享实体 reducer、边界 guard 和 fixtures 由 GitHub
> Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-08`](ui-entrypoints.md#step-ui-08) |
| source snapshot | `319df52a`（UI-08 分支基线，提交后绑定本提交） |
| feature_status | `implemented`（纯 reducer、scope/revision fence、optimistic rollback、bounded hydrate/dehydrate） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | typed client snapshot/feed/action → `kiana-client::UiEntityStore` → presenter projection；store 不授权、不执行 capability |

UI-08 在 `kiana-client` 提供一个按 scope 绑定的共享实体 store。Run、Connection、Submission、
Draft、Inbox 和 Artifact 使用同一套 key/revision/lifecycle 规则；presenter 不再创建第二份事实
状态。`reduce` 是纯函数，`apply` 只提交 reducer 输出。

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `reducer_rejects_old_revision_and_keeps_one_entity` | 旧 revision 不能覆盖新值，实体 key 保持唯一 |
| `reducer_deduplicates_events_and_requires_snapshot_after_gap` | 重复 event/frame 被拒绝，sequence gap 进入 snapshot-required 状态 |
| `optimistic_state_rolls_back_and_protected_entries_block_eviction` | pending 条目保护 cache，拒绝结果恢复 previous 或移除 optimistic 条目 |
| `unknown_and_pending_survive_hydrate_and_dehydrate` | `Unknown` 与 pending 状态经过 digest/schema 校验后保留 |
| `tab_scope_isolation_and_reducer_purity_are_explicit` | 跨 tab 输入被拒绝，纯 reducer 不修改原 store |
| `ui08_store_is_a_pure_bounded_projection` | source guard 拒绝 Broker、ControlPlane、DaemonHost、模型循环和文件副作用 |

## 3. State and recovery contract

```text
new(scope) -- snapshot --> authoritative entities
authoritative -- feed delta --> revision-checked entity update
optimistic --> pending (protected, expires/rolls back)
pending -- Applied --> authoritative
pending -- Rejected/expiry --> previous value or removal
pending -- Unknown --> unknown (protected, original idempotency key retained)
gap/epoch mismatch --> snapshot required; local pending/unknown state is retained by policy
```

The store binds every entity to workspace/session/tab and authority epoch. Feed sequence and event
IDs reject replay and sequence gaps; a gap must be repaired by a snapshot before later deltas are
accepted. Cache eviction only removes authoritative entries, never pending or unknown entries. The
snapshot digest covers schema, scope, epoch, entities, optimistic records and replay bookkeeping.

## 4. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the client reducer fixtures,
core/entrypoint source guards and `cargo check --workspace --tests --locked`. Local tests, builds,
checks, clippy and smoke are deliberately not run; CI result is not awaited.

Limitations: this is an in-process bounded presentation projection; no durable store, cross-process
replay, websocket/SSE delivery, presenter migration, provider/live timing or physical proof is
claimed. Snapshot/action DTOs remain supplied by the UI-05–07 contracts, and reconnect/crash
recovery remain UI-18/UI-33 work.
