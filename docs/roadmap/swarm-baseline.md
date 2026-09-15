# SW-00 Swarm 现状 reconciliation

> 快照日期：2026-09-15。本文是 `SW-00` 的 source-only 交接基线，不是运行时验收，也不把
> 现有类型、事件结构或静态检查升级成 `local_behavior`、`durable`、`live` 或 `physical`。
> 本轮不在本地运行测试；`swarm_baseline` 由 GitHub Actions 执行。

## 1. 快照与范围

| 项目 | 记录 |
|---|---|
| source snapshot | `1be732612bf727d21a4d7dea54ad4aff5454f15b`（CI-01 已推送的干净基线） |
| worktree 基线 | `master`，与 `origin/master` 同步；本文件、测试和 workflow 是 SW-00 变更 |
| proof-level | `source-only`；本轮不声明 `local_behavior`、`durable`、`live` 或 `physical` |
| 范围 | Swarm domain/state、packet graph/work packet、ControlPlane 路由与 replay、CellRegistry 端口与默认实现、packet child admission、现有测试边界 |
| 本轮不做 | 不新增 scheduler、queue、worker loop、自由消息总线、第二执行路径或稳定 ID 契约；这些属于 SW-01+ |

源码快照 hash：

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Swarm domain/state | `kiana-domain/src/swarm.rs` | `697c11d7c0f0be37adc5aeac6617e0920190b9c895574449994c4aec3c26ba0d` |
| Packet graph | `kiana-domain/src/packet_graph.rs` | `8f56ce5fb8ba80f50aabff3e6fb6221718289437b1a511123821c0d98509c8a0` |
| Work packet | `kiana-domain/src/work_packets.rs` | `2cc8b27f3710512b50bef970715dade5d76f27d010c77e14e3c22823d2b8b1e6` |
| Swarm ControlPlane | `kiana-core/src/swarm.rs` | `71a3b50e7971cb3c3eac4fece08a8e94f4a4ca83ec3c0a6a7c1d7407460fc9bb` |
| Cell registry | `kiana-core/src/cell_registry.rs` | `218ae2f9efec2c173881cd5d33d41d414c7e76f9db9e130d6518bfde602d9663` |
| Packet collaboration | `kiana-core/src/collaboration.rs` | `49e61cf89add855ca4fc3687104d666388477848d63397b9ea50b0404021dabf` |
| Port contract | `kiana-ports/src/lib.rs` | `88ff290c7a848e45af34985c42b0f7c0fb5719f8ae42708076e73c3290706ab1` |
| Protocol route | `kiana-protocol/src/lib.rs` | `19b06c1e828bef31cf6fdbd52283e0bf2ed54b69ca4d1c6fed94dc385e376ace` |
| Command route | `kiana-core/src/commands.rs` | `305d94e31ef5fceb494284dde0fa0b0bdfa0a31cda8d9609e890e7ca593063d8` |
| Daemon composition | `kiana-daemon/src/lib.rs` | `f262e808ab239eed0e01c1e28aef75cb5d78b819d809c1d07592ab34d367d281` |

## 2. 当前事实矩阵

| 分类 | 当前事实 | 证明等级 / 边界 |
|---|---|---|
| 已接线 | protocol 的 `swarm.command.v1` / snapshot 进入 `ControlPlane`；`SwarmState::transition` 覆盖 Create、StartChild、Reconcile、Merge、Cancel；`load_swarms` 折叠 `swarm.command_applied` 并检查 schema、revision、stream version、principal key、幂等键 | `source`；不是运行时 deny/success 证明 |
| 已接线 | `StartChild` 在现有 Company `StartRun` 路径继续执行；packet child 经过 `swarm_parent_for_packet`、`SWARM_CHILD_TEMPLATE` 和 `CellRegistryPort` admission；packet graph 的 DAG/cycle/ready helper 被 Company 路径复用 | `source` / process-local；不等于 durable queue 或真实 worker |
| 已接线但有窗口 | Swarm 事实先提交，再执行 controller admission、Company `StartRun` 或 reconcile；缺失 run 会被映射为 `ResultUnknown`/reconciliation-required | `source`；event 与副作用不是同一 durable 原子边界 |
| 仅类型/局部 | `SwarmPlan`、`SwarmChild`、`ChildMergeDecision`、`SwarmProof` 和现有 Cell/Packet/Grant/Lease 类型存在；`SwarmState` 有 bounded transition 规则（最多 8 个 packet/concurrency、depth=1、5 分钟 TTL、approval/budget/path/fingerprint/review 检查） | `source`；稳定 `PartitionId`、`AttemptId`、`DispatchIntentId`、lineage 和 typed delegation facts 尚不存在 |
| 仅类型/局部 | `MemoryCellRegistry` 将记录、预算、路径锁和 capability lease 放在 `Mutex<RegistryState>`；`CellRegistryPort` 明确允许进程内实现且不承诺 durable recovery | `source`；不能把 checkpoint/restore 或 event replay 写成 Cell durable ledger |
| 缺测试 | 新 CompanyOS Swarm 路径没有在 `kiana-core/tests`、`kiana-daemon/tests` 或 domain tests 中找到专项测试；旧 `kiana-commands/tests/swarm_command.rs` 是兼容执行面，不计入本产品路径证据 | `source`；deny、expiry、idempotency、replay、TOCTOU、unknown、restart、merge、retire 均未由本步运行时验证 |
| 未实现 | durable `DispatchIntent`/`QueueEntry`/claim lease/fence、独立 scheduler/worker、跨进程 Cell ledger、fresh child session/attempt、typed delegation/heartbeat/checkpoint/result facts、独立 Integrator/MergeReceipt、Swarm UI projection | `target`；分别落在 SW-01..SW-18 |

## 3. 复用边界与未证明窗口

1. `ControlPlane` 的默认组合使用 `MemoryCellRegistry`；daemon 的本地 JSONL EventLog 只能为事件事实提供持久介质，不能让内存中的 Cell reservation 自动变成 durable。
2. `StartChild` 已接受的 child 事实先写入 Swarm stream，之后才调用 Company `StartRun`。进程在这段窗口退出时，当前实现只有后续 reconcile/`ResultUnknown` 语义，没有 durable dispatch intent、attempt 或 claim fence。
3. `StartChild` 的 child `session_id` 来自当前 authority；核心 dispatch 也复制该 session 到嵌套 Company context。fresh child session、私有历史隔离和 successor attempt 仍未证明。
4. controller admission 的注释明确它没有 model session 或副作用；它是资源 admission/supervision Cell，不是另一个模型执行循环。
5. `packet_graph::ready_packets` 是查询 helper，不是 claim/dispatch authority；不能用 ready 结果推断已经取得执行权。

## 4. Roadmap 对照

| 目标 | 当前 source 对账 | 后续切片 |
|---|---|---|
| `P4-J6-01` 有界 Swarm | bounded domain transition、ControlPlane route、Company StartRun 和 packet admission 已有局部接线；完整 fan-out/fan-in、持久调度、恢复、reduce、release 未完成 | `SW-01`–`SW-18`，按 roadmap Wave A–F 顺序 |
| `CO-43` 并行 Builder | Create/StartChild/Reconcile 与现有 packet Cell 路由可达；没有 durable queue/attempt/fence，也没有跨进程 worker 恢复 | `SW-02`–`SW-12` |
| `CO-44` Integrator / MergeReceipt | domain 有 `ChildMergeDecision` 和 reviewer/session/coverage 检查；现有 generic review/receipt 不能等同于 Swarm integrator 的独立、不可变 merge receipt | `SW-13`–`SW-15` |

## 5. SW-00 护栏与退出条件

GitHub Actions 的 `swarm_baseline` 测试固定本节源码 hash，并检查以下语义仍被明确记录：

- `MemoryCellRegistry` 是 process-local，`CellRegistryPort` 不承诺 durable recovery；
- Swarm product ingress 和 event replay 已接线，但 `StartChild` 的 event-before-dispatch 窗口仍存在；
- 现状分类保持 `已接线 / 仅类型或局部 / 缺测试 / 未实现`，不得把 source 证据升级为 durable/live。

SW-00 完成只表示 reconciliation 和迁移护栏入库。任何运行时 deny-first、跨进程恢复、fresh child、queue fencing 或 deterministic golden 由后续 SW 卡和 GitHub CI 证明。
