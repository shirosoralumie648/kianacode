# UI-05 原子 snapshot projector 与分页基线

> 快照日期：2026-09-24；2026-10-17 完成切片（CI-only）。UI-05 的源码和拒绝夹具由 GitHub Actions 执行；本轮本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-05`](ui-entrypoints.md#step-ui-05) |
| source snapshot | `1668101c8b6a`（UI-05 完成切片起点）+ 本分支源码；历史切片起点 `312e00b6` |
| feature_status | `implemented`（domain projector + daemon query/health/rebuild facade + health/rebuild 合同 + remote fixture wiring） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | DaemonHost `persisted_events` 单次读取 → owner/session filter → source/projection cursor/retention gate → deterministic entries → page cursor → `UiSnapshotPage`；同一读取可生成 `UiSnapshotProjectorHealth`，并可由 `rebuild_ui_snapshot_projection` 显式重建 |

`project_ui_snapshot` 只接受一个账本快照，要求 query 的 `source_cursor == events.len()` 且 projection cursor 已追平；lag/unknown 返回结构化拒绝，不伪装空列表。条目按 session/run/action/artifact/receipt kind、ID、revision 和 source cursor 稳定排序。

完成切片补上的三件事：

1. **limitations 真实化**：`UiSnapshotPage.limitations` 不再恒为空——`next_page` 存在时带 `ui_snapshot_page_partial`，`retention_floor > 0` 时带 `ui_snapshot_retention_floor_applied`，按字典序去重排列并进入页面摘要。
2. **projector health/lag**：新增 `UiSnapshotProjectorStatus`（UpToDate/Lagging/Unknown）、`UiSnapshotProjectorHealthQuery`、`UiSnapshotProjectorHealth` 与 `project_ui_snapshot_health()`。落后给出显式 `lag` 数值，未知进度是独立状态；两者都带稳定 limitation，绝不渲染成空列表。谎报 projection cursor 超过 source（`ui_snapshot_projection_cursor_ahead`）或 source 与切片长度不符仍拒绝。
3. **read-model rebuild 命令**：新增 `UiSnapshotRebuild` 与 `rebuild_ui_snapshot_projection()`，DaemonHost 侧为 `rebuild_ui_snapshot_projection(query)`：一次 `persisted_events()` 同时重建页面与 health，页面走全部页/retention/cursor 校验，health 必须 `UpToDate`；故障时不返回半新半旧的组合，重放同一账本得到同一 `rebuild_digest`。

三条读路径都只读：不追加事实、不构造 capability broker、不打开第二条执行循环；DaemonHost 侧分别暴露为 `ui_snapshot_page`、`ui_snapshot_projector_health`、`rebuild_ui_snapshot_projection`。

## 2. Failure-first fixture matrix

Domain（`kiana-domain/tests/ui05_snapshot_projector.rs`）：

| Fixture | 断言 |
|---|---|
| `snapshot_is_atomic_stable_and_page_cursor_bound` | 单一 cursor 投影、稳定排序、分页边界、next cursor 绑定 epoch/source/generation，跨 cursor 或篡改拒绝 |
| `lag_retention_and_owner_boundaries_fail_closed` | projection lag/unknown、retention floor 下 pending action、foreign session owner 均拒绝 |
| `empty_slice_projects_an_empty_page_without_a_next_cursor` | 合法空切片返回可渲染空页与自洽摘要；声明 source cursor 却没有事件按拼接拒绝 |
| `repeat_reads_and_cursor_replay_return_the_same_page_without_changing_facts` | 重复读取与游标回放摘要一致，事实切片不因投影改变 |
| `page_limitations_expose_partial_pages_and_the_retention_floor` | partial/retention limitation 的出现条件与排序，末页不再带 partial |
| `pagination_boundaries_and_illegal_cursors_fail_closed` | page size 0/257、空 after、非 JSON、超大游标、越界 offset、跨 source/generation/epoch 游标全部拒绝；offset == 长度是合法空尾页 |
| `projector_health_reports_lag_and_unknown_instead_of_an_empty_list` | lag/unknown/up-to-date 三态、cursor-ahead 与 source mismatch 拒绝、retention limitation、状态不自洽拒绝 |
| `rebuild_matches_the_page_and_recovers_after_projection_faults` | rebuild 与页面投影一致、lag/unknown 失败、故障后同一事实重建出同一摘要、篡改 page/health 子摘要被拒 |

Daemon（`kiana-daemon/tests/ui05_snapshot_page.rs`，真实 `DaemonHost` + EventStore）：

| Fixture | 断言 |
|---|---|
| `ui_snapshot_page_projects_one_owner_scoped_slice_and_paginates_with_limitations` | 单次读取、owner/session 过滤、稳定排序、分页与 partial limitation、重复读取与游标回放、事实未被修改 |
| `ui_snapshot_page_refuses_lag_foreign_session_retention_and_stitched_cursors` | lag/unknown、跨 generation/source/epoch 拼接游标、foreign session owner、retention pending 拒绝；终态后同一游标恢复且 retention limitation 可见 |
| `ui_snapshot_page_fails_closed_when_the_projection_source_cannot_be_read` | `read_all` 不支持时 page/health/rebuild 都返回 `ui_snapshot_projection_unknown`，而真实空账本返回合法空页 |
| `projector_health_reports_lag_and_rebuild_matches_the_page_for_the_same_cursor` | health 三态与 lag 数值、rebuild 页面一致、故障失败、恢复后同一 `rebuild_digest` |
| `rebuild_from_a_restarted_host_over_the_same_ledger_returns_the_same_page` | JsonlEventLog 上释放文件锁后由新宿主实例重开同一账本，页面与 rebuild 摘要一致 |

Core 守卫（`kiana-core/tests/ui05_snapshot_projector_guard.rs`）：

| Guard | 断言 |
|---|---|
| `snapshot_projector_keeps_cursor_retention_lag_and_owner_boundaries` | domain projector、DaemonHost 三条读路径、versioned protocol DTO、health/rebuild/limitation markers 保持；三条读路径都不含 `CapabilityBroker`、`.append(`、`commit_transition` |
| `ui05_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards` | 上述 8 个 domain 与 5 个 daemon 夹具名绑定 baseline、手动 lane 保持 dispatch-only/`contents: read` 且执行三个 target、自动分片册仍覆盖 domain/core 两个 target、domain projector 无文件/网络/进程/`unsafe`/EventStore 写路径、夹具无 `#[ignore` |

## 3. Lane wiring

- 手动 lane：`.github/workflows/ui05-snapshot-projector.yml`（`workflow_dispatch`-only、`permissions: contents: read`、`cargo fetch --locked`、fmt 门、domain/daemon/core 三个 target 串行、`cargo test -p kiana-domain -p kiana-daemon -p kiana-core --no-run --locked`）。
- 自动覆盖：统一 `ci.yml` 的 `kiana-domain-s3/4`（已列 `ui05_snapshot_projector`）与 `kiana-core-s5/6`（已列 `ui05_snapshot_projector_guard`）；`kiana-daemon` 是整 crate 分片（`targets: null`），会自动发现 `ui05_snapshot_page`。

## 4. Durable boundary and limitations

页游标包含 schema、authority epoch、source cursor、offset、projection generation 和 digest；服务端不会使用 timestamp 拼页。retention floor 只允许保留待办 action，无法证明待办已终态时返回 `ui_snapshot_retention_protected_pending`。snapshot 是 EventLog 的可重建视图，不向账本写回事实；health/rebuild 也不写入或缓存 read-model 进度，调用方给出的 cursor 标签就是唯一进度输入。

当前证据只覆盖源码与 GitHub CI wiring，CI 结果未等待；proof level 上限 `source`。UI-05 不实现 feed/replay/backpressure、跨进程 projector worker、通知送达、artifact 内容下载或 live/physical proof；这些留给 UI-06+、NM、PD、ER、SC。`rebuild_from_a_restarted_host_over_the_same_ledger_returns_the_same_page` 用新宿主实例重开同一 JSONL 账本，仍是同进程内的 source/local 夹具，不等于真实进程重启或 durable 跨进程恢复。旧 `DaemonHost::ui_snapshot`、`ui_snapshot_page` 保留兼容，新增 `ui_snapshot_projector_health` / `rebuild_ui_snapshot_projection` 为受控只读 query facade。

## 5. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

`ci.yml` 的 shards 运行 `cargo fmt --all --check` 与该 Step 的三个 target；手动 lane 另列出同样三条命令与 workspace 编译。No local tests/build/check/clippy/smoke are run; CI result is not awaited.
