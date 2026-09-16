# SW-03 Swarm status reducer baseline

> 快照日期：2026-09-16。本页记录 Swarm/Partition/Attempt 的 typed transition/replay 合同；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`SW-03`](../roadmap.md#step-sw-03) |
| feature_status | `implemented`（typed transition event + pure live/replay reducer） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | SwarmTransitionEvent → SwarmTransitionReducer → legacy SwarmEvent validation → existing ControlPlane/EventLog CAS |
| this step does | strict transition schema、Swarm/Partition/Attempt 合同、revision/epoch fencing、terminal/Unknown/merge-review 拒绝、同一 reducer 的 replay/live 入口 |
| this step does not | 不自动创建 dispatch/claim、执行 Broker、替换旧命令 reducer、实现 child materialization、queue/recovery projector；SW-04+ 负责 |

## 2. State and event rules

`SwarmTransitionEntity` 以 tagged union 区分 Swarm、Partition 和 Attempt；每个 `SwarmTransitionEvent` 带 server-side typed IDs、`delegation.*` event kind、连续 revision、不可回退 authority epoch、correlation/causation、可选 merge-review 完成标记和 canonical digest。unknown schema/field、nil ID、kind/entity 不一致和 digest drift fail-closed。

`SwarmTransitionReducer` 从 `Reserved`/`Pending`/`Proposed` 初态按一张纯状态表接受合法边；terminal 状态不可重开，`ResultUnknown` 不得转成功，`ReadyToMerge → Completed` 必须带 `review_complete=true`，版本 gap/重复/回退和旧 authority epoch 拒绝。`replay` 与 `apply` 使用同一实现。

现有 `SwarmEvent` 增加兼容性的可选 `transitions` 批次，`validate_transitions` 在 core `load_swarms` 反序列化时调用；空批次仍可读取历史命令事件。该字段不授权执行，真实 commit 仍只经 `commit_swarm`/EventLog。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `reducer_rejects_illegal_terminal_unknown_and_merge_without_review` | 非法边、terminal reopen、Unknown→success 和无 review merge 均拒绝 |
| `reducer_replay_matches_live_and_rejects_revision_epoch_regressions` | replay/live 结果一致，版本 gap/epoch 回退/unknown field 均拒绝 |
| `swarm_status_reducer_is_typed_and_replay_uses_the_existing_event_path` | domain reducer 与 core load/commit 路径保持单一执行脊柱 |

`.github/workflows/sw03-reducer.yml` 在 GitHub runner 执行 domain reducer fixtures、core source guard、fmt 和 domain/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 现有 legacy SwarmState command transitions 尚未为每个命令自动生成完整 typed Partition/Attempt facts；`transitions` 批次可选是迁移边界，不代表旧事件已有完整 lineage。
- reducer 只验证事实和状态，不持有资源锁、预算、审批或 worker lease；effect-time authorization、dispatch CAS、child scope、cancel/Unknown recovery 和 durable projector 仍需 SW-04+、ER/PD/SC。
- `SwarmTransitionEvent` 的 digest 是 canonical integrity fingerprint，不是签名或跨进程身份认证；CI 结果故意不等待。
