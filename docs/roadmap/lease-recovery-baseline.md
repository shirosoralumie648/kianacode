# P1-D-03 packet claim and lease recovery baseline

> 快照日期：2026-09-16。本页记录 WorkPacket claim 的 owner/heartbeat/expiry、受控续租和过期扫描回收；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-D-03`](../roadmap.md#step-p1-d-03) |
| feature_status | `implemented`（domain claim + Company scan/recovery source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | ClaimPacket/StartRun → PacketClaim → `renew_company_claim` → `reclaim_packet_leases` → ReclaimPacketClaim → Company facts |
| this step does | owner-bound lease creation/renewal, heartbeat expiry detection, bounded deterministic scan, stale claim removal and no-double-dispatch guard |
| this step does not | 不把 readiness 查询当执行权，不自动重试未知 in-flight effect，不实现 durable scheduler/worker process recovery；AUT/SW/ER/PD 后续步骤负责 |

## 2. Lease rules

- `PacketClaim` 绑定 CellId、owner SessionId、heartbeat_at 和 lease_expires_at；renew 需要当前 owner、未过期 lease、单调 heartbeat，并受 packet deadline 限制。
- `CompanyState::ClaimPacket` 先调用 canonical `ready_packets` 且要求无现有 claim；StartRun/每个 runtime turn 使用 `renew_company_claim`，只在 heartbeat 超过阈值时追加 renewal fact。
- `ControlPlane::reclaim_packet_leases` 只扫描过期 claim（最多 128 个），为每项生成带 packet/owner/expiry 的幂等 command；`ReclaimPacketClaim` 在事实转移前再次确认 expiry。无 run 的 stale claim 才恢复 ready；已 dispatch 的 claim 必须提供同一 execution_request 的 terminal observation，ResultUnknown 进入 incident，不能重复派发。
- 回收不删除历史事实，claim clear 与 reclaim receipt 仍由 Company EventLog/Company command CAS 记录；ready 只是投影，不能绕过 policy/gate/approval/Broker。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `expired_lease_is_reclaimed_without_double_dispatch` | 过期 claim 可清除并回到 ready；重复 reclaim 被拒且不会创建第二次 dispatch |
| `heartbeat_renewal_requires_the_current_owner_and_live_lease` | foreign owner、过期 lease 或非单调 heartbeat fail-closed 且不改写 claim |
| `packet_lease_scan_renews_and_reclaims_through_company_commands` | core scan/renew/reclaim 都回到 CompanyCommand，不在查询或 scheduler 中直接执行副作用 |

`.github/workflows/p1-d03-lease.yml` 在 GitHub runner 执行 domain lease fixtures、core source guard、fmt 和 domain/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 claim/Company state 与 EventStore 使用仍是进程内或现有 adapter 语义；跨进程 worker death、durable lease projector、queue fairness/backoff 和 scheduler heartbeat 仍未完成。
- 过期 in-flight run 不会因 claim 回收自动重试；必须先有 terminal observation，对账/Unknown 继续由 ER/CP/SW/PD 收口。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
