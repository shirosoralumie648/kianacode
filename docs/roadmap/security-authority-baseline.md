# SC-07 ProjectTrust、RoleAssignment 与 DepartmentSnapshot 基线

> 快照日期：2026-09-17。本页记录 server-owned source contracts 和 GitHub CI-only fixtures；不
> 把内存目录或一次 CI 通过写成 durable authentication 或完整权限 enforcement。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-07`](security-compliance.md#step-sc-07) |
| feature_status | `implemented`（trust/department snapshots、authority join、daemon assignment wiring） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责运行 fixtures |
| authority | Daemon-owned ProjectIdentity/ProjectTrustAuthority、AssignmentDirectory 与 ControlPlane SecurityContext |
| this step does | strict ProjectTrustSnapshot/DepartmentSnapshot、typed authority join、principal/project/role/epoch checks、untrusted effect gate |
| this step does not | 不实现外部 authn、durable role/department store、policy Grant intersection、SecretStore、跨进程 revocation 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/trust_snapshots.rs` 新增 `ProjectTrustSnapshot` 与 `DepartmentSnapshot`：前者
绑定 project ID、canonical-root/trust revision digest、server source、revision 和 trusted 状态；
后者绑定 department、canonical role IDs、revision 和 authority epoch。未知字段、foreign/unknown
role、duplicate/noncanonical role list、digest drift 和无效 epoch 全部拒绝。

`kiana-core/src/security_authority.rs` 新增 `SecurityAuthoritySnapshot`，把 daemon-owned
principal、project trust、现有 `ResolvedAssignment` 和 DepartmentSnapshot 连接到同一
SecurityContextId/authority epoch。它要求 assignment principal/project/role/department/epoch 与
trust/department snapshot 完全一致，提供 `validate_request` 和 `require_trusted_for_effect`；它
只做值校验，不执行或发放 capability。

`DaemonHost::context_from_assignment` 先以 server principal、project trust、assignment 和
department snapshot 生成 authority snapshot，再调用既有 company assignment guard。`project_trust_snapshot`
提供同一 daemon-owned ProjectTrustAuthority 的 typed projection；wire actor/role/trust 仍不是
authority source。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `project_trust_snapshot_is_server_scoped_and_strict` | project/trust source/revision/digest 绑定且 raw token/foreign ID 拒绝 |
| `department_snapshot_is_canonical_and_role_bound` | department role list canonical、unknown/foreign role 拒绝 |
| `assignment_directory_resolution_binds_principal_project_role_and_epoch` | server AssignmentDirectory 解析绑定 principal/project/role/epoch |
| `authority_snapshot_round_trips_and_validates_scope` | authority join strict serde/digest/request scope validation |
| `authority_snapshot_rejects_foreign_role_project_and_untrusted_effect` | role/project tamper 与 untrusted effect fail-closed |
| `authority_snapshot_and_daemon_assignment_paths_are_server_owned` | source guard 固定 daemon-owned assignment/trust、无 Broker/执行依赖 |

## 3. Proof ceiling and handoff

SC-07 证明上限为 `source`：snapshot 和 assignment join 的边界已固定，但 AssignmentDirectory
仍为内存适配器，ProjectTrustAuthority 仍读取本地 trust，role/department 尚无 durable CAS/revoke
projector；local-user 也不是外部 authenticated principal。SC-08 负责 authority/session epoch
fence 与 refresh，SC-09 负责 GrantScope intersection；Policy/Approval/Secret/redaction/TOCTOU、
跨进程恢复和真实 external/live/physical effect 仍未证明。
