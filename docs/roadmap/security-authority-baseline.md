# SC-07 ProjectTrust、RoleAssignment 与 DepartmentSnapshot 基线

> 快照日期：2026-10-02。本页记录 server-owned source contracts 和 GitHub CI-only fixtures；不
> 把内存目录或一次 CI 通过写成 durable authentication 或完整权限 enforcement。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-07`](security-compliance.md#step-sc-07) |
| feature_status | `partial`（typed trust/department/authority contracts；显式 assignment 尚未进入 daemon 主请求路径） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责运行 fixtures |
| authority | Daemon-owned ProjectIdentity/ProjectTrustAuthority、AssignmentDirectory 与 ControlPlane SecurityContext；handle 主路径仍使用本地 role allowlist 兼容解析 |
| this step does | strict ProjectTrustSnapshot/DepartmentSnapshot、typed authority join、显式 validator principal/project/role/department/epoch checks、untrusted effect gate |
| this step does not | 不实现外部 authn、durable role/department store、policy Grant intersection、SecretStore、跨进程 revocation 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/trust_snapshots.rs` 新增 `ProjectTrustSnapshot` 与 `DepartmentSnapshot`：前者
绑定 project ID、canonical-root/trust revision digest、server source、revision 和 trusted 状态；
后者绑定 department、canonical role IDs、revision 和 authority epoch。未知字段、foreign/unknown
role、duplicate/noncanonical role list、digest drift 和无效 epoch 全部拒绝。

`kiana-core/src/security_authority.rs` 新增 `SecurityAuthoritySnapshot`，把 daemon-owned
principal、project trust、现有 `ResolvedAssignment` 和 DepartmentSnapshot 连接到同一
SecurityContextId/authority epoch。它要求 assignment principal/project/role/department/epoch 与
trust/department snapshot 完全一致。`validate_request` 还要求调用方提供 daemon 已解析的
`ProjectIdentity`，并校验 project ID、canonical-root digest、trust revision 和请求 root 与该身份
一致；它只做值校验，不执行或发放 capability。

`RequestContext.project_trusted` remains a caller assertion: a false value cannot upgrade or replace
the server snapshot, and a true value against an untrusted server snapshot is rejected. The typed
snapshot's effect gate reads only `SecurityAuthoritySnapshot.project_trust.trusted` through
`require_trusted_for_effect`, so the deny fixture constructs an untrusted server snapshot rather than
mutating only the request hint. The live `DaemonHost::handle` preflight currently uses the separate
`SecurityContext` path for project/trust checks.

`DaemonHost::context_from_assignment` 先以 server principal、project trust、assignment 和
department snapshot 生成 authority snapshot，再调用既有 company assignment guard。该 helper
目前没有调用者。`DaemonHost::handle` 实际通过 `ControlPlane::resolve_security_context` 做前置校验，
角色来自本地 `RoleSpec` 与 `principal.allowed_roles`，没有解析 `AssignmentDirectory` 或构造
`SecurityAuthoritySnapshot`。因此 typed assignment 不是当前所有请求的强制 authority source；
wire actor/role/trust 仍不能突破现有本地兼容检查。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `project_trust_snapshot_is_server_scoped_and_strict` | project/trust source/revision/digest 绑定且 raw token/foreign ID 拒绝 |
| `department_snapshot_is_canonical_and_role_bound` | department role list canonical、unknown/foreign role 拒绝 |
| `assignment_directory_resolution_binds_principal_project_role_and_epoch` | server AssignmentDirectory 解析绑定 principal/project/role/epoch |
| `authority_snapshot_round_trips_and_validates_scope` | authority join strict serde/digest/request scope validation |
| `authority_snapshot_rejects_foreign_role_project_and_untrusted_effect` | role/project tamper、foreign request root/identity、server-owned untrusted effect 与 wire trust escalation fail-closed |
| `authority_snapshot_and_daemon_assignment_paths_are_server_owned` | source guard 固定 project identity/root binding、daemon helper 校验与无 Broker/执行依赖 |

## 3. Proof ceiling and handoff

SC-07 当前为 `partial/source`：strict snapshot/join validator 已绑定 exact project identity/root，
但 `DaemonHost::handle` 尚未消费 `AssignmentDirectory`；默认 host 的目录为空，而且请求路径没有
server-owned `OrganizationId` 来源。要启用强制 explicit assignment，必须先定义 assignment provisioning
和默认 local compatibility 的迁移语义；静默 fallback 会继续允许未指派请求，不能算完成 SC-07。
AssignmentDirectory 仍为内存适配器，ProjectTrustAuthority 仍读取本地 trust，role/department
尚无 durable CAS/revoke projector；local-user 也不是外部 authenticated principal。SC-08 负责
authority/session epoch fence 与 refresh，SC-09 负责 GrantScope intersection；Policy/Approval/
Secret/redaction/TOCTOU、跨进程恢复和真实 external/live/physical effect 仍未证明。
