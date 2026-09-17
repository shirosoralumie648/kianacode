# CP-08 Grant authority 与撤销 epoch 基线

> 快照日期：2026-09-17。本页记录 domain grant envelope/reducer 与现有 core authority stream 的
> source/CI 证据；不把内存 ledger 或 fixture 写成完整 durable authorization。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-08`](control-plane.md#step-cp-08) |
| feature_status | `implemented`（root/child GrantAuthorityEnvelope、GrantLedger、revocation fence） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog authority facts + ControlPlane；GrantLedger 是纯 reducer contract |
| this step does | principal/project/parent/issuer/scope/expiry/epoch/revision binding, root bounded envelope, child subset, append sequence, ancestor revoke fencing, snapshot restore |
| this step does not | 不接 durable GrantStore/permit consumption、完整 approval/policy intersection、SecretStore、跨进程 recovery 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/grant_authority.rs` 新增 strict `GrantAuthorityEnvelope`、
`GrantLedgerSnapshot` 和纯 `GrantLedger`。root run 必须显式限制 operations/paths，child 必须
绑定已存在 parent、同一 principal/project/issuer/authority epoch、较小 scope/expiry；重复 ID、
sequence gap/regression、跨主体/项目 parent、scope widening、unknown/invalid digest 全部
拒绝。`revoke` 只产生新 revision/epoch 的 revoked envelope；`active_grant` 沿祖先链复核，因此
祖先撤销后后代立即被 fence。Snapshot restore 按 parent availability 重建，保留 revoked ancestor
的历史关系，不把旧状态恢复成 active。

现有 `AuthorityLedger` 仍是 EventLog facts 的 authority reducer；CP-08 新 ledger 只提供可复用
的 grant 值对象边界，后续 CP-13/CP-17 才把它与 permit/Cell 生命周期做原子消费与释放。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `root_and_child_grants_are_strictly_bounded_and_snapshot_replays` | root/child subset、sequence、snapshot/restore 稳定 |
| `grant_ledger_rejects_widening_duplicate_gap_and_foreign_parent` | widening、duplicate、sequence gap、foreign parent fail-closed |
| `revoking_an_ancestor_fences_descendants_and_preserves_unknown_state` | revoke epoch 使 descendants inactive，历史 revoked snapshot 可恢复 |
| `cp08_grant_authority_is_domain_reducer_and_does_not_bypass_eventlog_control` | source guard 固定 domain reducer/authority stream/无执行绕过 |

## 3. Proof ceiling and handoff

CP-08 proof ceiling 为 `source`：root/child/revoke/restore 不变量已固定，现有 authority stream
仍需 durable CAS、完整 assignment/trust/policy/data revisions、Permit/Approval/Cell 原子边界。
CP-09/10 将消费 exact approval material，CP-11 budget、CP-12 lease/fence、CP-13 permit 将消费
grant envelope；cross-process crash/recovery、Secret/redaction、external/live/physical proof 未
宣称。
