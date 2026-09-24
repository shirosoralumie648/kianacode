# UI-05 原子 snapshot projector 与分页基线

> 快照日期：2026-09-24。UI-05 的源码和拒绝夹具由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-05`](ui-entrypoints.md#step-ui-05) |
| source snapshot | `312e00b6`（EXT-27 动态可见性源码合并后的 master；EQ-27 evaluator compile fix 与 UI-04 durable action CAS 已在其祖先） |
| feature_status | `implemented`（domain projector + daemon EventStore query facade + remote fixture wiring） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | DaemonHost `persisted_events` 单次读取 → owner/session filter → source/projection cursor/retention gate → deterministic entries → signed page cursor → UiSnapshotPage |

`project_ui_snapshot` 只接受一个账本快照，要求 query 的 `source_cursor == events.len()` 且 projection cursor 已追平；lag/unknown 返回结构化拒绝，不伪装空列表。条目按 session/run/action/artifact/receipt kind、ID、revision 和 source cursor 稳定排序。

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `snapshot_is_atomic_stable_and_page_cursor_bound` | 单一 cursor 投影、稳定排序、分页边界、next cursor 绑定 epoch/source/generation，跨 cursor 或篡改拒绝 |
| `lag_retention_and_owner_boundaries_fail_closed` | projection lag/unknown、retention floor 下 pending action、foreign session owner 均拒绝 |
| `ui05_snapshot_projector_guard` | domain projector、DaemonHost facade、versioned protocol DTO、retention/lag/owner markers 保持，入口不创建第二执行循环 |

## 3. Durable boundary and limitations

页游标包含 schema、authority epoch、source cursor、offset、projection generation 和 digest；服务端不会使用 timestamp 拼页。retention floor 只允许保留待办 action，无法证明待办已终态时返回 `ui_snapshot_retention_protected_pending`。snapshot 是 EventLog 的可重建视图，不向账本写回事实。

当前证据只覆盖源码与 GitHub CI wiring，CI 结果未等待。UI-05 不实现 feed/replay/backpressure、跨进程 projector worker、通知送达、artifact 内容下载或 live/physical proof；这些留给 UI-06+、NM、PD、ER、SC。旧 `DaemonHost::ui_snapshot` 保留兼容，新增 `ui_snapshot_page` 为受控 query facade。

## 4. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

CI runs `cargo fmt --all --check`, domain UI-05 fixtures, core source guard and workspace test-target compile. No local tests/build/check/clippy/smoke are run; CI result is not awaited.
