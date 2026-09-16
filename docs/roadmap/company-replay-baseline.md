# CO-08 Company state replay and migration baseline

> 快照日期：2026-09-16。本文记录 CompanyState 的确定性 reducer、stream gap/duplicate/terminal/schema 拒绝及唯一 legacy v0→v1 adapter；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CO-08`](companyos.md#step-co-08) |
| source snapshot | `bfd7084`（CO-07 parent）加本步源码；最终 commit 记录在 git history |
| feature_status | `implemented`（domain replay reducer + core load wiring + legacy schema adapter source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | EventStore company stream metadata → CompanyReplayReducer gap/identity/schema checks → pure CompanyState::transition → rebuilt state/history |
| this step does | 新增 CompanyReplayReducer，固定 aggregate/root/owner、stream_version 连续性、event kind/idempotency、command expected revision 和 pure state transition；未知 major 拒绝；仅支持显式 `kiana.company-event.v0` 字段形状迁移到 v1，旧历史可重建 |
| this step does not | reducer 不执行 Runner/provider/effect、不改写历史 bytes、不把当前 role policy 重新应用到旧事实；完整业务对象 state migration/upcast、跨 aggregate transaction 和 durable projector 仍需后续 PD/ER/CO steps |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain reducer/migration | `kiana-domain/src/company_replay.rs`, `kiana-domain/src/company.rs`, `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs` | `781bb9542ab04140e9c4206c05d4bcb265c7a8f1333c83ff58fa297754200d0f`, `4cf29654e4f6e3729b47c5f65f1beab9f7daea3e244d643cc8562273d147767b`, `bb71a2feb0f3de2595d7e39db0be897561f0377280cc3f5db733b22d1449ad65`, `aa796d275675e113d0a5fa338664fae0d4234357ee833de19acb384e72370403` |
| Core replay wiring | `kiana-core/src/company.rs` | `67ae4acefedb2ae8b6392ccbb425f1ca773c123dfcfdfe079118457775af4615` |
| Fixtures and CI | `kiana-domain/tests/co08_replay.rs`, `kiana-core/tests/co08_replay_guard.rs`, `.github/workflows/co08-company-replay.yml` | `9544dc667642598ef189b29262ad65ad2c182c4f84f5934f00e646accfd0c377`, `a6101f2a35f167b2c617ebfb4090e2f10ab727a0f0a6d64670f835750d659551`, `c00100ce08bf4c369605eb992c76fc36991b0724a970993b281425894ddfe9a5` |

## 2. Reducer invariants

`CompanyReplayReducer::apply` 要求 runtime event 属于唯一 company aggregate，stream_version 必须等于 `state.revision+1`，event kind 必须和 command event_name 对齐，runtime idempotency key 必须是 `company:{aggregate}:{logical_key}`，record owner/authority/root/request expected_revision 必须与 reducer snapshot 一致，logical key 不能重复。Transition 失败会返回 state transition error，reducer 保持原 state 不变。

`migrate_company_event` 只把已知 `kiana.company-event.v0` 的 schema 标签升级为 v1；unknown/missing/未来 major 不猜测、不丢字段、不执行。CompanyState 仍由纯 `transition` 产生，reducer 不调用任何 capability/Runner/Provider。

## 3. Core integration and terminal safety

`kiana-core::load_company` 读取并按 stream_version 排序后交给 reducer，保留 typed CompanyEvent history。Replay gap、regressed version、duplicate idempotency、aggregate/root/owner/kind mismatch 和 unknown schema 在任何新 command 之前 fail-closed；同一历史在 policy/catalog 更新后仍按记录中的 command/authority/proof transition 重建，不被今天的角色声明重写。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `company_v1_history_rebuilds_identically_after_replay` | 同一 v1 Company event 序列两次 reducer 得到相同 state/revision/history |
| `replay_rejects_gaps_duplicates_and_unknown_schema_without_state_change` | gap、重复 logical key、unknown major 都拒绝且不推进 state |
| `legacy_v0_event_migrates_only_when_the_shape_is_currently_parseable` | 已知 v0 schema 显式迁移，其他 schema 不隐式升级 |
| `company_load_path_uses_deterministic_reducer_and_explicit_migration` | core load_company 只走 reducer，CompanyState transition 仍是唯一纯应用路径 |

`.github/workflows/co08-company-replay.yml` 在 GitHub runner 执行 domain replay/migration fixtures、core source guard、fmt 和 domain/core/daemon test-target compile；本地只做格式、静态编译和 diff 检查。

## 5. 限制与交接

- reducer 当前接收已从 EventStore 读出的 runtime event；尚无独立 durable snapshot/projector、cursor checkpoint 或跨进程 migration journal。
- v0 adapter 只验证 schema 标签和现有 v1 字段 shape，不提供任意旧 payload 字段重命名；未知/缺失字段必须人工编写新迁移版本。
- 业务 state 中 String IDs、typed Artifact/Assignment refs 和多 aggregate facts 尚未全面 upcast；CO-09+ 在 reducer 之上补业务命令状态语义与 object-level acceptance。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
