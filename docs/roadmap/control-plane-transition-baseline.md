# CP-06 atomic transition and command idempotency baseline

> 快照日期：2026-09-16。本文记录 CP-06 的 TransitionBatch/read-set/CommitOutcome 和 EventStore
> 原子合同；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-06`](control-plane.md#step-cp-06) |
| source snapshot | `b744469`（CP-05 入口一致性后的干净基线） |
| feature_status | `implemented`（domain/ports/eventlog/core atomic transition source） |
| proof_level | `source`；格式与 test-target 静态编译不提升为 local_behavior/durable/live/physical |
| canonical path | ControlPlane read-set → `TransitionBatch` validate → EventStore CAS/command digest → all-or-none frame/`CommandReceipt` → projection/observer |
| this step does | 固定 command_id+digest 幂等、expected aggregate versions、batch/frame bounds、Committed/Replayed/Conflict/Unknown 分类和无部分状态语义 |
| this step does not | 不把单事件 append 当多聚合事务，不把 Unknown 当拒绝/成功，不自动补偿外部 effect，不声称掉电/跨机器 exactly-once |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain journal contract | `kiana-domain/src/journal.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48`, `6ba5d3fef8d874ed05f68cb62c593306220442d39e5fb37f6fc59922e6387415`, `4d77c0c495963746f22150fc1f74f4cdf7a2629537f318fc91b12647a81e3999` |
| Ports/eventlog adapters | `kiana-ports/src/lib.rs`, `kiana-eventlog/src/event_store_core.rs`, `kiana-eventlog/src/journal_core.rs`, `kiana-eventlog/src/memory.rs`, `kiana-eventlog/src/jsonl.rs`, `kiana-eventlog/src/stream.rs` | `a539b96c0813c0f08c043dd89b4f2bcbe9fdb4d6d9f93923443821b54679e6d0`, `506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e`, `84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56`, `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9`, `f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f`, `bc955e9c583466a97c1fb02fda4bff8af076dd04ee8df342e21777345b9c033a` |
| Core commit boundary | `kiana-core/src/dispatch.rs`, `kiana-core/src/events.rs`, `kiana-core/src/approvals.rs` | `464b68ec143743580a201b01d26e7ca8d4f1dc542acb4116edb228a885b65cd0`, `5a0348f5e1940363119d920244724428af1e1373f692f6424ccfd3b8a6dfe26e`, `439a602ee0463d1be33ce144d787e5b7b6cbdb19db7277de2da248aa0cf28b1e` |
| Fixtures/guard/workflow | `kiana-eventlog/tests/cp06_atomic_transitions.rs`, `kiana-core/tests/cp06_transition_guard.rs`, `.github/workflows/cp06-atomic-transitions.yml` | `30e797bb3944408dd8f34eccb813eeeb35ad7330ae8191a337ae67cae206ba15`, `f33f81432b42cf8d26b4351d97328854479e879b6bbfe88aedb8334ee22cf29e`, `bf35229b42916c226f37d179cb8bcb2a0340895566060d3117899806648320d5` |

这些 hash 只作静态源码漂移锚点；提交/回执/锁的存在不等于跨进程 durable 或外部 exactly-once。

## 2. Contract matrix

| 对象 | 不变量 | 失败边界 |
|---|---|---|
| `TransitionBatch` | command_id、bare SHA-256 digest、非空 bounded events、唯一 read-set、每个 event 写入 read-set aggregate | 缺字段、重复 read-set、event aggregate/version 不匹配或 frame oversize 拒绝 |
| `AggregateVersion` | aggregate type/id 有界；expected version 是该 command 的 CAS 读取快照 | 实际版本不同返回 `CommitOutcome::Conflict`，不写任何 event |
| `CommandReceipt` | command/digest/commit id/cursor/event IDs/最终 versions 一致，observer 只在 committed 后收到 | forged cursor/event/version 被拒，不能覆盖事实 |
| `CommitOutcome::Committed` | 一个批次全部写入并返回 receipt | 只有确认写入后才可让后续授权/observer继续 |
| `CommitOutcome::Replayed` | 同 command_id + 同 digest 返回原 receipt，不追加事件 | 同 command_id + 不同 digest 是结构化 conflict |
| `CommitOutcome::Conflict` | CAS/read-set 冲突只报告 changed versions | 不重算为 Allow，不写部分 aggregate |
| `CommitOutcome::Unknown` | commit 结果不确定时必须 read_command 确认 | 未确认前不发布 permit/dispatch authority |
| `JournalFrame` | header/writer/body length/body sha256/version 校验；Transition frame all-or-none | unknown writer/corruption/oversize/torn frame fail-closed |

## 3. Adapter boundary

`EventStorePort::commit_transition` 是 CP/approval/budget/lease/permit 共用的原子入口；适配器通过 `EventStoreCapabilities` 声明 atomic transition、command receipt、cursor、writer version 和 bounds。`MemoryEventLog` 与 `JsonlEventLog` 复用 `JournalState::plan_transition`，只在存储锁/持久同步上有差异；旧 adapter 缺少事务能力时返回 explicit unsupported，而不是降级为 `append`。

`append_expected` / `append_idempotent_expected` 保留兼容单事件语义，调用方必须明确它们不等价多聚合事务。`StreamEventStore` 的 commit observer 只在 `Committed` 后收到 transition；Replay、Conflict、Unknown 不产生成功通知，observer failure 作为诊断保留，不改写已提交事实。

## 4. Failure-first fixtures

| Fixture | 断言 |
|---|---|
| `cp_transition_conflict_changes_no_aggregate` | stale expected version 返回 Conflict，aggregate/event 数量不变 |
| `cp_same_command_different_payload_is_conflict` | 同 command_id 的不同 digest 拒绝且不追加第二事件 |
| `cp_stale_allow_is_recomputed_after_cas_conflict` | CAS conflict 报告实际版本，调用者必须重新计算决定 |
| `only_fresh_committed_transitions_notify_and_replay_links_original_receipt` | committed 通知一次，replay 链接原 receipt，冲突不通知 |
| `observer_failure_is_diagnostic_and_never_rewrites_a_committed_outcome` | observer 故障不把已提交事实改成失败 |
| `conflict_and_unknown_outcomes_never_publish_observer_notifications` | Conflict/Unknown 不泄露可执行 authority |

## 5. ControlPlane use

`commit_confirmed` 在 dispatch/permit 等高后果路径要求 `supports_atomic_transitions()`，将 command digest canonicalize 后提交；Committed/Replayed 才继续，Conflict 返回结构化冲突，Unknown 转为 `result_unknown` 并要求 `read_command`/reconciliation。普通 `append_event` 仍用于兼容展示或已明确单事件的事实，不能被用来绕过共享 read-set。

授权/审批/预算/租约的共同转移必须把所有影响 aggregate 放进同一 expected_versions；CAS 失败不能靠在旧 payload 后追加事件“消除冲突”。Receipt/Projection/Observer 只能从已提交批次派生，不能反写或覆盖 EventLog。

## 6. 限制与交接

- JSONL 事务帧和锁是本地文件系统能力；本步骤不证明断电、磁盘损坏恢复、网络文件系统或跨主机 exactly-once。
- EventStore 的 Unknown 只说明提交回执不确定，不说明 effect 未发生；Broker/Provider/Connector 仍需独立 execution receipt/reconcile。
- 现有兼容 append/API 仍可写 legacy 事件；CP-07/ER-01+ 负责统一磁盘 reader、schema migration 和 legacy upcast，不能把 compatibility append 当新授权路径。
- command receipt、aggregate CAS、approval/budget/lease 同批提交的完整组合仍由 CP-08..14、ER/PD/SC 后续步骤覆盖；本步不新增第二事实源。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；GitHub CI 结果按用户要求不等待，不提升 durable/live/physical。
