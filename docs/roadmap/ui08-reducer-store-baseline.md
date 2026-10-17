# UI-08 共享 reducer/entity store baseline

> 快照日期：2026-10-17（completion slice，CI-only）。UI-08 的共享实体 reducer、边界 guard、
> 声明式 fixture 与 reducer trace 由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-08`](ui-entrypoints.md#step-ui-08) |
| source snapshot | 分支 `step/ui-08-20261017-1`（基线 master `2812372bcb3f`，UI-07 合并点）；提交后绑定本提交 |
| feature_status | `implemented`（纯 reducer、scope/revision/事件 ID fence、optimistic rollback、gap/epoch/conflict 围栏、bounded hydrate/dehydrate） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | typed client snapshot/feed/action → `kiana-client::UiEntityStore` → presenter projection；store 不授权、不执行 capability、不写 EventStore |

UI-08 在 `kiana-client` 提供一个按 scope 绑定的共享实体 store。Run、Connection、Submission、
Inbox 和 Artifact 使用同一套 key/revision/lifecycle 规则；Draft 命名空间属于本地输入层
（UI-12/UI-14），store 只拒绝它、不存储它。`reduce` 是纯函数，`apply` 只提交 reducer 输出，
presenter 不再自建事实状态。

2026-10-17 收口切片在此之上补了四类真实缺口（全部先拒绝优先）：

1. **真实 feed 起不来**：daemon 的 live 订阅第一帧是 `feed_sequence = 0` 的 boundary，旧实现把它
   当重复帧拒绝。现在「尚未应用任何帧」的 boundary 是合法起点（`initial_boundary`）。
2. **缺 entity 命名空间的 server 帧会被猜成 run**：`entity_kind` 缺失时旧实现默认 `run`。现在
   `ui_feed_entity_kind_missing` 直接拒绝，raw run-stream 帧（`{"Delta": ...}`）不可能被投影成实体。
3. **draft 命名空间可被服务端/伪造快照写入**：feed 里 `entity_kind = draft` →
   `ui_feed_draft_kind_forbidden`；hydrate 里出现 draft 实体 → `ui_store_snapshot_draft_entity`
   （在摘要校验之前拒绝）。
4. **乐观值可以掩盖服务端新 revision**：pending/unknown 实体遇到更高 revision 的权威 delta 时旧实现
   报 `StaleRevision` 并保留本地值。现在返回 `SnapshotRequired` + `UiStoreGapReason::PendingConflict`，
   帧不被记为已应用、位置不回退，并保留本地值等待显式 hydrate。

另外两处收口：`Rejected` 结算不再删除「自己从未 stage 过」的权威实体（只回收 accepted 产生的
pending 占位）；protocol snapshot 合并到 protected 实体时刷新 rollback baseline，让回滚回到服务端
最新已知值，而不是复活更旧的本地值。

## 2. Failure-first fixture matrix

`kiana-client/tests/ui08_reducer_store.rs`（17 条命名夹具，先拒绝在前）：

| Fixture | 断言 |
|---|---|
| `reducer_rejects_old_revision_and_keeps_one_entity` | 旧 revision 不能覆盖新值，实体 key 保持唯一 |
| `reducer_deduplicates_events_and_requires_snapshot_after_gap` | 重复 event/frame 被拒绝，sequence gap 进入 snapshot-required 状态 |
| `optimistic_state_rolls_back_and_protected_entries_block_eviction` | pending 条目保护 cache，拒绝结果恢复 previous 或移除 optimistic 条目 |
| `accepted_keeps_optimistic_record_until_settled_or_expired` | Accepted 不升级为事实，settle/expiry 才结束 optimistic |
| `unknown_and_pending_survive_hydrate_and_dehydrate` | `Unknown` 与 pending 状态经过 digest/schema 校验后保留 |
| `hydrate_rejects_optimistic_previous_from_a_foreign_scope` | rollback previous 跨 workspace/session/tab/epoch 时拒绝 hydrate |
| `tab_scope_isolation_and_reducer_purity_are_explicit` | 跨 tab 输入被拒绝，纯 reducer 不修改原 store |
| `live_feed_starts_at_the_initial_boundary_and_markers_never_regress` | 位置 0 的 boundary 可采纳；heartbeat 是 marker；落后位置与已见事件 ID 被拒绝 |
| `replayed_terminal_after_the_boundary_is_not_a_second_item` | 终态 replay 只落一次；重复 ID 不产生第二条 item |
| `epoch_reset_drops_authoritative_state_and_keeps_local_pending` | 新 authority epoch 的 boundary 丢旧权威实体、保留本地 pending、要求 snapshot |
| `generic_feed_requires_an_explicit_kind_and_refuses_the_draft_namespace` | 缺 kind / draft / 未知 kind / 缺 id / revision 0 / 跨 session 全部拒绝且零副作用 |
| `pending_conflict_never_lets_an_optimistic_value_hide_a_newer_revision` | pending 冲突 → `PendingConflict` snapshot-required，本地值不被覆盖，帧不被记为已应用 |
| `snapshot_merge_refreshes_the_rollback_baseline_instead_of_a_stale_value` | 拒绝后回滚到服务端最新 revision，而不是更旧的本地值 |
| `hydrate_refuses_a_forged_draft_entity` | 伪造的 draft 实体在摘要校验前被拒绝 |
| `a_stale_rejection_never_deletes_an_authoritative_entity` | 陈旧 `Rejected` 不删权威实体；accepted 占位仍可回收 |
| `kind_scoped_views_keep_the_six_namespaces_separate` | 五个服务端命名空间 + submission 各自可数；draft 视图恒空 |
| `ui08_reducer_plan_and_trace_replay_deterministically` | 声明式 plan/trace 与源码常量、夹具名绑定，并两次重放到同一摘要 |

真实组合根夹具 `kiana-daemon/tests/ui08_entity_store.rs`（3 条，`DaemonHost` + `KianaHarness`）：

| Fixture | 断言 |
|---|---|
| `raw_run_stream_frames_are_refused_without_an_entity_envelope` | 真实 run-stream 帧先证明没有 entity 命名空间，再证明 store 一律 `ui_feed_entity_kind_missing` 拒绝；只有 boundary marker 被采纳且不造实体；事实数与模型调用次数不变 |
| `store_projection_is_scope_bound_and_never_writes_facts` | 同一帧进入 tab-1 成功、进入 tab-2 `ScopeMismatch`；pending 只属于本 tab；投影不追加事实、不重跑模型 |
| `real_gap_fences_later_deltas_until_a_snapshot_and_writes_no_facts` | 真实旧 epoch cursor 只能拿到 gap → `SnapshotRequired`；boundary 只报位置、不解除围栏；后续 delta `GapRequired`；heartbeat 仍可作 marker；事实不变 |

因为 daemon 目前只发布 run-stream 帧，daemon 夹具用显式 adapter 形状（`entity_kind`/`entity_id`/
`revision`）补齐命名空间——这正是「谁提供身份谁负责」：store 不猜、不写事实。

## 3. State and recovery contract

```text
new(scope) -- boundary@0 (initial) --> position adopted (SnapshotHydrated)
authoritative -- dense entity delta --> revision-checked entity update
optimistic --> pending (protected, expires/rolls back)
pending -- Applied --> authoritative
pending -- Rejected/expiry --> previous baseline (snapshot-refreshed, never stale)
pending -- Unknown --> unknown (protected, original idempotency key retained)
duplicate event id --> DuplicateEvent (never a second item)
position behind --> DuplicateFrame (never applied late)
gap / sequence gap / pending conflict / new authority epoch --> snapshot required
  (position not advanced on a gap; boundary markers still record the server position)
marker (boundary/heartbeat/unknown) --> position only, never an entity
```

The store binds every entity and an optimistic update's rollback `previous` entity to the same
workspace/session/tab and authority epoch. Feed sequence and event IDs reject replay and sequence
gaps; a gap must be repaired by a snapshot before later deltas are accepted. Cache eviction only
removes authoritative entries, never pending or unknown entries. The snapshot digest covers schema,
scope, epoch, entities, optimistic records and replay bookkeeping.

## 4. Presenter boundary

* Presenters read the store's namespace-scoped views (`entities_of_kind` / `count_of_kind`); they do
  not keep their own run/connection/submission/inbox/artifact maps.
* Drafts stay in the input layer (`kiana-entrypoints/src/tty_input.rs`,
  `kiana-entrypoints/src/workbench_controller.rs`); the store refuses any server or snapshot value
  in that namespace.
* `kiana-entrypoints/tests/ui08_store_boundary_guard.rs` fails if an entrypoint re-declares the store
  (`struct UiEntityStore`, `enum UiStoreEvent`, `fn ensure_capacity`, …), and
  `kiana-core/tests/ui08_reducer_store_guard.rs` binds the plan, the trace, the fixture names, the
  lane and the automatic shards.
* No second execution path: the store contains no `CapabilityBroker`, `ControlPlane::new`,
  `DaemonHost`, `KianaHarness`, `EventStorePort`, `ModelClient`, `std::fs::` or `.append(`.

## 5. Declarative plan and reducer trace

`kiana-client/tests/fixtures/ui08-reducer-store.json`（`kiana.ui08-reducer-store-fixture.v1`）固定：

* 六个 entity kind、三个 lifecycle 名、五个 gap reason（含 `pending_conflict`）；
* 预算 `4096 / 8192 / 512`（与 `UI_ENTITY_STORE_MAX_ENTITIES`/`_EVENTS`/`_UNKNOWN` 逐字相等）；
* 17 个客户端夹具名、拒绝清单与 limitations；
* 31 步 reducer trace：先证明未 hydrate 不能吃 delta、gap 不能靠 boundary 解除，再证明 stale
  revision、重复 event ID、落后位置、缺 kind、draft 命名空间、revision 0 与 pending 冲突全部拒绝，
  最后证明 revision 单调、cache eviction 只淘汰权威条目、optimistic 可过期回滚；trace 重放两次得到
  同一 `snapshot_digest`。

## 6. CI wiring

手动 lane [`.github/workflows/ui08-reducer-store.yml`](../../.github/workflows/ui08-reducer-store.yml)
（`workflow_dispatch`-only、`permissions: contents: read`）执行 `cargo fetch --locked`、fmt 门、
`kiana-client --test ui08_reducer_store`、`kiana-core --test ui08_reducer_store_guard`、
`kiana-entrypoints --test ui08_store_boundary_guard`、`kiana-daemon --test ui08_entity_store`
四条串行 target，并以 `cargo test -p kiana-client -p kiana-core -p kiana-entrypoints -p kiana-daemon
--no-run --locked` 收口。自动覆盖仍是统一 `ci.yml` 的 `kiana-core-s6/6`（已列
`ui08_reducer_store_guard`）与 `kiana-client`/`kiana-daemon`/`kiana-entrypoints` 整 crate 分片
（`targets: null`，自动发现）。无 Cargo.toml/Cargo.lock/package.json 变化，无新依赖，
`scripts/ci/test-shards.json` 未改。

## 7. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the four UI-08 targets above
and the touched-crate compile step. Local tests, builds, checks, clippy and smoke are deliberately
not run; CI result is not awaited.

## 8. Limitations

* 进程内一次性展示投影：不落 EventLog、不授权、不执行 capability，也不持久化 store 状态。
* revision 与 feed sequence 只在本 store 内单调；与服务端其它坐标（UI-05 页 cursor、账本 sequence）
  不做跨坐标对账。protocol snapshot hydrate 会把本地位置重置为 0，之后第一帧无条件接受。
* gap 只能由显式 snapshot hydrate 解除；本轮仍没有把 UI-05 页投影成协议 `UiSnapshotV1` 的生产者，
  所以真实 surface 在 gap 之后仍要求后续步骤补齐 hydrate 通道。
* draft 属于本地输入层（UI-12/UI-14），store 只拒绝它、不存储它；跨进程/跨 tab 草稿恢复不在本卡。
* 「terminal 之后的迟到 delta」由 daemon feed 的 `terminal_delta_rejections` 围栏；store 只把重复终态
  当 duplicate 拒绝，不额外记录终态封闭集合（快照 schema 由 UI-09 锁定，不新增字段）。
* 真实 daemon 帧的 entity 命名空间由调用方补齐；未证明跨进程 socket/SSE、durable 订阅、
  presenter 迁移或 live/physical 证据。
* proof_level 上限为 source：本地未编译未运行，编译与行为正确性依赖 GitHub Actions。
