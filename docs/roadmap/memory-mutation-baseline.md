# CM-04 unified Memory mutation and idempotency baseline

> 快照日期：2026-09-16。本文记录 CM-04 的 MemoryMutation/CAS/幂等合同与 handler 预检；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-04`](context-memory.md#step-cm-04) |
| source snapshot | `0fe99c4`（CM-03 server-derived scope 提交后的干净基线） |
| feature_status | `implemented`（domain mutation contract + deterministic CAS/idempotency guard + daemon normalization source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | ControlPlane `ExecutionScope` → daemon server `MemoryScope` → typed `MemoryMutation` → all-target preflight/CAS + idempotent receipt → handler persistence |
| this step does | 定义 ADD/UPDATE/DELETE/APPROVE/PUBLISH/EXPIRE/REVOKE、target expected revision、scope/evidence、actor、policy/data epoch、payload digest 和 idempotency key；纯 ledger 在任何 revision 写入前完成全目标预检，重复 key 返回原回执，payload 漂移/过期 revision fail-closed；memory.write/review 生成并校验该合同 |
| this step does not | 不把 handler JSONL append 伪称 EventStore 原子提交；正文引用、Memory facts、review consumption、projection cursor 的唯一提交点留给 CM-05；proposal 批量物化、processing grant/retention/revocation ledger 和跨进程恢复仍未完成 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain mutation/CAS contract | `kiana-domain/src/memory_mutation.rs` | `7abfc4bf7899569f9d20e86a9aecdf2bc0e04e5fdfa26fc1f96f938342da7e97` |
| Domain exports/schema registry | `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs` | `eacb8e9415167e32a7365372b61eaf21aa288d57f7e1182e3dea4e0e99059b8c`, `3ef30b315e1aa0c61b441340f87ab3b6037473145024b14286645276905b0efc` |
| Daemon write/review normalization | `kiana-daemon/src/harness_memory.rs` | `d06ada5cf3f8c1ba0a17670ec8873ec68c68d1e34a1c777fb86fbe5d8178bbad` |
| Fixtures/workflow | `kiana-domain/tests/cm04_memory_mutation.rs`, `kiana-daemon/tests/cm04_memory_mutation_guard.rs`, `.github/workflows/cm04-memory-mutation.yml` | `e7d37b4e9e885293cd9ec5bdf08dd533b3688b4cc216e221f025a34b88b22c34`, `67e315ddb20c21d27e5c24cb319d73609e57bda6d4a9f79839c42a329e3ede5f`, `c72b471a140936f16506666bcd1d62e5fd559af276c33ee2ff08e57b2652af14` |

hash 只用于 CM-04 源码漂移复核，不构成 Memory durable、外部效果或业务结果证明。

## 2. Mutation contract

`MemoryMutation` 只接受七个显式 operation，携带服务端 actor 与完整 `MemoryScope`。actor 必须等于 scope principal；scope 必须 `allow_write`，每个 target collection 必须被 scope 覆盖。每个 target 都有确切 `expected_revision`：ADD 只能从 0 创建，其余操作必须提供已有正 revision。evidence 使用已校验的 `SourceRef`，policy/data epoch 与 protected payload digest 进入 mutation digest；raw 正文不进入 authority intent。

`MemoryMutationReceipt` 绑定 mutation digest、scope digest、actor、epoch、target successor revisions 和 idempotency key。Receipt 的 `validate_against` 拒绝 identity/epoch/digest 漂移，避免把同一 key 当作不同事实。

## 3. CAS and replay boundary

`MemoryMutationLedger::preflight` 先读取全部 target 的当前 revision，任何一个不匹配或溢出都会返回冲突且不推进其他 target。`apply` 先按 idempotency key 查询：相同 digest 返回 `Replayed { original }`，不同 digest 返回 `memory_mutation_idempotency_conflict`；只有全体预检完成后才更新 revision map 并保存新 receipt。因此 stale request 不能变成 last-write-wins，批量 mutation 不会部分推进。

daemon `memory.write` 将稳定 idempotency key 映射到记录 ID，生成 server-owned Add mutation；相同 key/记录内容返回同一 mutation receipt，内容漂移拒绝。operator `memory.review` 将 promote/reject 映射为 APPROVE/REVOKE mutation，在 record append 前按 exact revision 和 evidence 预检。scope、actor、policy/data epoch 均来自授权请求，而不是模型文本。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `duplicate_memory_mutation_returns_original_receipt` | 同一 key + 同一 mutation digest 第二次返回原始 receipt，revision 不重复推进 |
| `stale_revision_never_last_write_wins` | 旧 expected revision 被拒绝，当前 revision 保持最新值 |
| `batch_preflight_rejects_one_stale_target_without_advancing_the_other` | 多目标中一个 stale 时整批拒绝，其他 target 不被部分更新 |
| `memory_handlers_use_server_mutation_contract` | daemon write/review 从 server scope 构造 mutation，ledger/CAS 在 append 前执行，未从模型 actor/scope 授权 |

`.github/workflows/cm04-memory-mutation.yml` 在 GitHub runner 执行 domain ledger fixtures、daemon source guard 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 ledger 是可复用的纯 CAS/idempotency 规则模型；daemon 仍有 JSONL projection，CM-05 才把 Memory fact、正文引用、review/approval consumption 和 EventStore `TransitionBatch` 接到同一提交点。
- handler 侧通过稳定 key/record identity 做兼容 replay，但跨进程 receipt journal、崩溃前后对账和 projection cursor 尚未声明 durable；未知提交点仍需 CM-05 的 `result_unknown`/reconciliation 语义。
- `accept_proposal` 的混合 ADD/UPDATE/DELETE 批量 materialization 仍沿现有审批链，完整跨 target 原子物化与 source dependency graph 留给 CM-05/06；本步不建立第二事实源。
- policy/data epoch 当前复用授权 ExecutionScope 的 server epoch；完整 processing grant、retention、revocation/delete 传播仍需 PD/SC/CM-06。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
