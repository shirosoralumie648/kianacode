# SC-08 authority epoch、session fence 与 policy refresh 基线

> 快照日期：2026-09-17。本页记录 typed fence 与 ControlPlane read/recheck helpers 的 source/CI
> 证据，不把内存/JSONL 读取写成跨进程 durable fencing。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-08`](security-compliance.md#step-sc-08) |
| feature_status | `implemented`（AuthorityFence、ControlPlane issue/recheck、deny-first fixtures） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog authority stream / ControlPlane epoch；fence 只是 immutable observation |
| this step does | authority/session/policy/config fence fields, monotonic parent digest/sequence, current validation, stale/rollback/expiry reasons, read-only refresh helpers |
| this step does not | 不实现 durable permit ledger、跨进程 session store、自动撤销/重试、完整 approval/Grant binding 或外部 effect |

## 1. Contract

`kiana-domain/src/fencing.rs` 新增 `AuthorityFence`，绑定 scope/session、authority epoch、
session generation、policy/config revision digest、sequence、parent digest、短期有效窗口和
fence digest。Genesis/继承链必须连续；epoch/generation 只能前进；`validate_current` 对过期、
authority rollback/stale、session generation、policy/config drift 返回稳定 `AUTH_*`/
`POLICY_*`/`UNKNOWN_*`/`FACT_*` reason，不把旧 fence 当作当前许可。

`kiana-core/src/security_fence.rs` 新增 `ControlPlane::issue_authority_fence`、
`validate_authority_fence` 和 `authority_fence_snapshot`。它们只读取现有 authority stream，将
server-owned revision 归一化为 digest，并在调用者进入已有 admission/permit 路径前重验 scope、
session、epoch、policy/config 和时间；没有新 Broker/handler/执行循环。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `authority_fence_is_versioned_digest_linked_and_round_trips` | fence genesis/successor parent digest、sequence 和 serde 稳定 |
| `authority_fence_rejects_epoch_session_policy_config_and_time_drift` | stale/rollback/expiry 各边界返回稳定 reason |
| `authority_fence_rejects_invalid_parent_and_successor_rollback` | 缺 parent、epoch/session 回退 fail-closed |
| `control_plane_fence_helpers_read_authority_and_never_revive_stale_work` | source guard 固定 EventLog read/recheck、无 effect/旧状态复活 |

## 3. Proof ceiling and handoff

SC-08 的 proof ceiling 为 `source`：typed fence 与 core helper 已写入，但当前 session/assignment
及 authority facts 仍部分进程内/JSONL，issue 本身不是 durable permit 或撤销事件；late observations、
approval consumption、Grant/Scope 与 Broker effect-time fence 需后续 SC-09/10/12。SC-09 将以此
epoch/fence 作为 GrantScope intersection 输入，SC-12 将把 fence 绑定到 PendingInvocation/Permit
CAS；未确认 external effect 仍必须保持 Unknown。
