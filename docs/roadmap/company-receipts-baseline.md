# CO-07 versioned Company facts and stable receipt baseline

> 快照日期：2026-09-16。本文记录 Company logical command receipt、payload/authority digest、DispatchIntent 与已有 EventStore CAS 的接线；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CO-07`](companyos.md#step-co-07) |
| source snapshot | `4f72c36`（CO-06 parent）加本步源码；最终 commit 记录在 git history |
| feature_status | `implemented`（typed Company receipt/intent + existing atomic EventStore path source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | CompanyCommand logical key → deterministic command/payload/authority digests → existing `commit_protected_event`/EventStore CAS → CompanyCommandReceipt + optional DispatchIntent |
| this step does | 新增 versioned CompanyCommandReceipt/DispatchIntent，固定 logical command ID、idempotency key、payload/authority digest、expected/committed revision、event ID、replay/unknown 状态；StartRun/selected delivery effects 写入 prepared intent，replay 返回原 receipt，不重复派发 |
| this step does not | 不新增第二 EventStore 或 dispatcher；receipt 不把 Runner/Provider/外部交付结果伪装成成功；旧 CompanyEvent 仍按现有 replay reducer 读取，完整状态迁移留给 CO-08 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain receipt contract | `kiana-domain/src/company_receipts.rs`, `kiana-domain/src/company.rs`, `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs` | `4a5517b10472490986e36f797b42573b9ea75c7acb0c7950cdff50ce31fa951b`, `4cf29654e4f6e3729b47c5f65f1beab9f7daea3e244d643cc8562273d147767b`, `9045c4fa7793e5994721010a799a89e7245f39a7b8bded440aba527840e8c2d6`, `aa796d275675e113d0a5fa338664fae0d4234357ee833de19acb384e72370403` |
| Core Company CAS/response | `kiana-core/src/company.rs`, `kiana-core/src/authority.rs` | `7e938f5b91c45c65f276dd8ee70690b989ccc3fa5604734921637b35975df809`, `115fd5b4eb01266269b8c7d9564aba5118c49d396fd0dbac67a16f88ba015c3a` |
| Protocol/export | `kiana-protocol/src/lib.rs` | `a5efad3bbff0cd67f82ad79b4d137dfec3424cc1c6f3969ba9eaacc356cc260c` |
| Fixtures and CI | `kiana-domain/tests/co07_receipt.rs`, `kiana-core/tests/co07_receipt_guard.rs`, `.github/workflows/co07-company-receipts.yml` | `2271616509f57e2a2cf1cc408236b4573e0269fe0b494b7dfbd610f40827cde9`, `1bcf042be53144dd2ef62b2271d024fa5d6bd5611f3b630a83fde62759da6d37`, `521694c1e1e23dba7b4df9e4dc048235ac2e27acbd84fab69ce9dc2b96822a99` |

## 2. Receipt contract

`CompanyCommandReceipt` 以 `CompanyCommandReceipt::command_id(aggregate,idempotency)` 生成稳定 ID，payload digest 只覆盖不可变 command，authority digest 绑定 actor/session/role/department/root。Committed 必须有 committed revision 与 event ID；Replayed 使用同一 command/payload/authority identity；ResultUnknown 不能声称已经提交。所有 digest、revision、key 和 intent binding 都 fail-closed。

`DispatchIntent` 是 Company fact 与后续 effect 的显式交界，绑定 command ID、effect kind、target/scope digest、attempt 和状态。`Prepared` 只表示意图随业务事实提交，不表示 effect 已执行；消费/未知/取消状态要由后续事实或对账动作更新。

## 3. Core integration

`handle_company_command` 在 CompanyState transition 前计算稳定 command identity。重复 idempotency key 先核对完整 request、actor/role/session 边界，再从原 Company stream 读取原 event 并返回 typed replay receipt；payload 或 authority drift 返回 `company_idempotency_conflict`。新事实继续经过 `commit_protected_event`，该路径使用已有 `TransitionBatch`、authority read-set、EventStore CAS 和 `read_command` receipt 能力。

StartRun 与受限 delivery/cancel reconciliation 命令在同一 CompanyProof 写入 prepared DispatchIntent；之后仍调用现有 Harness/Capability path，若结果未知只提升 response/intent 状态，不重放副作用。旧 response 的 `event_id`/state 字段保留，typed receipt 作为稳定新字段。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `company_receipt_is_stable_for_retries_and_binds_dispatch_intent` | 相同 aggregate/key 得到相同 command ID，payload drift 可识别，intent 绑定 receipt |
| `unknown_company_receipt_cannot_claim_a_commit` | ResultUnknown receipt 没有 commit revision/event ID，仍可校验并等待对账 |
| `company_command_receipt_and_dispatch_intent_use_existing_event_store_cas` | Company path 经过 protected command/commit_transition/read_command，未新增 dispatcher/事实源 |

`.github/workflows/co07-company-receipts.yml` 在 GitHub runner 执行 domain receipt fixtures、core source guard、fmt 和 domain/ports/core/daemon test-target compile；本地只做格式、静态编译和 diff 检查。

## 5. 限制与交接

- CompanyCommandReceipt 目前是 response/proof projection，完整 durable query/read-by-key API、cross-session authorization projection 和 receipt retention 仍依赖后续 PD/ER/CO-08+。
- DispatchIntent 只在 CompanyProof 随事实提交 prepared 状态；现有 StartRun/业务 effect 的消费事实和跨进程 recovery 需要后续切片显式实现，不能根据 receipt 推断现实副作用。
- CompanyEvent/CompanyState 仍兼容 String project/artifact references；payload schema migration、unknown event/state transition 与历史 upcast 由 CO-08 处理。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
