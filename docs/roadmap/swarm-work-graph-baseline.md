# SW-02 typed Swarm WorkGraph baseline

> 快照日期：2026-09-16。本页记录显式 Partition/WorkGraph 规划合同；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`SW-02`](../roadmap.md#step-sw-02) |
| feature_status | `implemented`（typed Partition/WorkGraph validator + deterministic projection） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | typed WorkGraph → shared `packet_graph::validate_dependency_graph` → existing ControlPlane Swarm/EventLog path |
| this step does | Partition input/data/path/output/fingerprint 合同、依赖 cycle/missing/duplicate、scope/path overlap、count/depth/concurrency/spawn-rate/TTL/budget 上限、`first_success` 拒绝、稳定 ready/blocked/failed projection |
| this step does not | 不实现 queue/claim/scheduler、child materialization、attempt/dispatch durable facts、merge/replay/recovery 或 effect-time fencing；SW-03+ 负责 |

## 2. Validator rules

`Partition` 使用 domain-owned `PartitionId`/`SwarmPlanId`，规范化并锁定 input refs、data scope、owned paths、output contract/version、预算和 `WorkFingerprint`。空输入、重复输入、nil ID、digest/fingerprint drift、未知字段和错误 schema fail-closed。

`SwarmWorkGraph` 在新 SwarmPlan 的 `work_graph` 可选字段上接入：存在 typed graph 时，`SwarmState::Create` 在任何状态写入前调用 `validate`，并要求 graph swarm ID 与 packet plan 对齐；旧字符串 packet plan 在迁移期继续可读。路径使用 shared lexical containment/conflict helper，data scope 采用确定性前缀/通配冲突规则。

依赖边先转换为字符串键，再复用 `packet_graph::validate_dependency_graph`，因此 cycle 规范化、missing ref 和 duplicate edge 与 WorkPacket 图保持同一实现。投影只读地按同一拓扑输出排序后的 ready/blocked/failed；它不会创建 claim 或执行 Broker。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `swarm_plan_rejects_partition_overlap_and_unbound_input` | 重叠 owned path、空 input refs 均在构造/图校验前拒绝 |
| `work_graph_rejects_cycle_missing_duplicate_and_first_success` | cycle/missing dependency、重复 fingerprint 与 `first_success` fail-closed |
| `work_graph_rejects_limits_and_preserves_stable_projection` | count/depth/concurrency/spawn-rate/TTL/token/model-call budget 上限拒绝，成功依赖后的 ready 投影可读，unknown fields 拒绝 |
| `work_graph_rejects_duplicate_partition_keys_and_ordinals` | 重绑 graph digest 后，duplicate PartitionId key 与 ordinal 分别命中稳定拒绝码；run `36993318357` domain-s4/4 目标 6/6 通过 |
| `work_graph_projection_reports_failure_causes_and_stably_sorts_ready_items` | Failed/Cancelled/ResultUnknown、失败依赖传播、运行中/未完成依赖阻塞、成功依赖放行及多项 ready/failed 的稳定排序；run `36993318357` domain-s4/4 目标 6/6 通过 |
| `swarm_work_graph_uses_shared_packet_graph_and_keeps_execution_in_control_plane` | domain 复用 shared packet graph；Swarm 仍通过现有 ControlPlane/EventLog 唯一路径 |

上述 domain fixtures 和 core source guard 现由 `.github/workflows/ci.yml` 的 `kiana-domain-s4/4` 与 `kiana-core-s6/6` 分片执行；目标清单在 `scripts/ci/test-shards.json`。格式、构建、静态检查和测试均交给 GitHub runner；本地只做 `git diff --check`。

## 4. 限制与交接

- `SwarmPlan.work_graph` 目前是迁移期 optional；没有 graph 的旧计划仍按旧 packet checks 运行，不能声称所有 Swarm 已使用 typed graph。
- `PartitionProjection` 是 read-only planning view，ready 不产生执行权；DispatchIntent/QueueEntry、capacity/fair ordering、claim lease 和 retry 由 SW-05/06 负责。
- WorkFingerprint 是现有 domain 的确定性 FNV 兼容指纹，不是密码学签名；真实跨进程唯一性、持久 CAS 和 worker 效果仍需 EventLog/PD/SC 证据。

## 5. 2026-10-02 canonical data scope follow-up

`Partition::new` 会 trim data scope，但 wire/direct facts 原先只检查非空、长度、NUL 和排序。带首尾空白的 `" project/a/item"` 可通过该检查，并在 graph digest 重新绑定后避开 `scope_conflict("project/a", " project/a/item")` 的原字符串前缀比较。`Partition::validate` 现在先拒绝非 canonical scope，再进入图的重叠检查与 readiness projection；构造器仍接受可规范化输入。

`work_graph_rejects_noncanonical_wire_data_scope` 覆盖 direct facts、重绑 digest 的 JSON round-trip 和 projection 拒绝；它同时验证构造器 trim 后拒绝真实重叠，并保留 disjoint canonical graph 的 wire/projection 成功路径。既有拒绝断言保持不变。

```text
source_snapshot: 64ae7860 + SW-02 canonical data scope working-tree slice
worktree_status: isolated step/sw02-work-graph-20261002; only swarm_graph.rs, sw02_work_graph.rs and this baseline changed
command_argv:
  gh run view 36677090825 --json status,conclusion,headSha,jobs
  gh run view 36677090825 --log-failed
  git diff --check
cwd/environment: /home/shirosora/kiana-wt/sw02-work-graph-20261002; Linux; no local cargo/fmt/build/check/clippy/test/smoke commands
fixture/cassette: work_graph_rejects_noncanonical_wire_data_scope in existing CI-sharded sw02_work_graph target; no provider or cassette
exit_code: 0 for completed CI metadata/log reads and git diff --check; new fixture has not run locally or remotely at this snapshot
status_change: canonical data scope validation gap closed at source; no overall SW-02 status promotion
proof-level_change: remains source
limitations: CI 36677090825 ran c221c211 and passed the three pre-existing SW-02 domain fixtures plus the core source guard, but the overall CI failed elsewhere; the new fixture requires a later GitHub CI receipt; optional graph migration, durable dispatch and effect-time fencing remain outside this slice
reviewer: Codex SW-02 isolated implementation source review; integration review and GitHub CI pending
```

## 6. 2026-10-02 Create-order source guard

The existing core guard checked that the WorkGraph validator and ControlPlane/EventLog markers
were present, but did not pin the required ordering. The guard now asserts that the typed graph
validation and swarm-plan identity check both occur inside the `SwarmCommand::Create` branch before
`next.swarms.insert` can write the new state. This is a source-only regression fence; it does not
execute a swarm, mint a permit, or add another execution path.

```text
source_snapshot: 8348e059 + SW-02 Create-order guard slice
worktree_status: isolated step/sw02-audit-20261002; only kiana-core/tests/sw02_work_graph_guard.rs and this baseline changed
command_argv:
  rg -n 'SwarmCommand::Create|graph.validate\(\)|graph.swarm_plan_id.to_string\(\)|next.swarms.insert' kiana-domain/src/swarm.rs kiana-core/tests/sw02_work_graph_guard.rs
  git diff --check
cwd/environment: /tmp/kiana-sw02-audit-20261002; Linux; no local cargo/fmt/build/check/clippy/test/smoke commands
fixture/cassette: swarm_work_graph_is_validated_before_create_state_write in kiana-core/tests/sw02_work_graph_guard.rs; no provider or cassette
exit_code: 0 for source inspection and git diff --check; GitHub CI receipt pending/unobserved
status_change: SW-02 remains 🔄; added a source-order fence for pre-write WorkGraph validation
proof-level_change: remains source
limitations: source ordering does not prove runtime or durable behavior; optional graph migration, typed dispatch/queue/claim, child lifecycle, replay/recovery and effect-time fencing remain outside SW-02
reviewer: Codex SW-02 isolated source audit; no local runtime test reviewer
```

## 7. Identity and projection fixture follow-up (2026-10-02)

The duplicate-key fixture mutates a valid graph's second `PartitionId`, then recomputes the graph
digest so validation reaches the duplicate-key check. A separate graph mutation duplicates only
the ordinal and likewise recomputes the digest, pinning both structured rejection codes without
changing production behavior.

The projection fixture includes direct Failed, Cancelled and ResultUnknown partitions, a pending
partition whose dependency failed, a Running partition, a pending dependency chain, and a
successful dependency. It checks exact failed IDs and block reasons, then reverses the graph's
partition order and requires an identical projection with ready and failed IDs sorted by typed ID.

```text
source_snapshot: source commit `68752712` based on `3859cea9`; integrated on master as `abce1f4f`; formatting correction applied after the remote gate report
worktree_status: duplicate-identity and projection fixtures plus evidence only; no production semantics, manifest or lockfile changes
command_argv:
  `git diff --check`
  source review of `kiana-domain/tests/sw02_work_graph.rs`, `CURRENT_STATUS.md` and this baseline
  `cargo fmt --all` (formatting only, in response to run `36993318357`)
cwd·environment: source review in `/tmp/kiana-sw02-fixture-evidence-20261002`; formatting in repository root; no local test/build/check/clippy/smoke commands
fixture·cassette: run `36994107681` / head `41b4d135` / domain-s4/4 job `110797094394` reported `sw02_work_graph` 6/6 passing, including both fixtures in this section
exit_code: source review and `git diff --check` 0; formatter-only `cargo fmt --all` 0; exact domain target passed 6/6
existing_remote_receipt: run `36994107681` / core-s6/6 job `110797094448` reported both core guard fixtures passing (2/2), including `swarm_work_graph_uses_shared_packet_graph_and_keeps_execution_in_control_plane`; the overall domain/core shards failed on unrelated sibling targets. Run `36677090825`, head `c221c211`, covered the original SW-02 fixtures.
status_change: none; roadmap row 090 remains `🔄`
proof-level change: none; remains `source`
limitations: run `36994107681` was cancelled by a subsequent push and its shards failed on unrelated sibling tests despite the exact SW-02 domain/core targets passing. Workflow-reference and rustfmt gate corrections are recorded in `CURRENT_STATUS.md`; optional graph migration and durable dispatch/effect fencing remain outside SW-02.
reviewer: source-level review of rejection setup, projection expectations and unified CI shard mapping; no runtime test reviewer
```

## 8. 2026-10-03 model-call budget rejection fixture

The existing limit fixture now covers both model-call budget branches. A graph above
`MAX_SWARM_MODEL_CALLS` is re-digested before validation and must return the hard-cap error with no
partition ID. A valid two-partition graph whose aggregate model-call budgets exceed its declared
graph limit is also re-digested and must return the same error bound to the partition that crosses
the limit. No validator behavior or shared packet binding contract changed.

The additions run in the existing `sw02_work_graph` target on `kiana-domain-s4/4`; the core source
guard remains on `kiana-core-s6/6`, and no shard/workflow change is needed. Run `37056167237`, head
`8affa112`, domain-s4 job `111001574290` passed the complete `sw02_work_graph` target 6/6,
including both new model-call budget branches; the enclosing domain shard failed on sibling targets.
SW-02 remains `partial/source`: exact packet-set binding, optional legacy graph migration, durable
dispatch and effect-time fencing remain open.
