# CP-01 ControlPlane identity and project scope baseline

> 快照日期：2026-09-16。本文记录 CP-01 的 server-owned identity slice，不是企业认证、租户隔离或
> live credential provider 的完成声明。本轮不在本地运行测试；domain/runtime fixtures 只由 GitHub CI 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-01`](control-plane.md#step-cp-01) |
| source snapshot | `b923553`（INT-00 已推送的干净基线） |
| feature status | `implemented`（typed identity/project/assignment source） |
| proof ceiling | `source`；静态编译和源码 guard 不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | wire RequestMetadata → DaemonHost authenticated principal/project resolver → RequestContext → ControlPlane/session assignment CAS → policy/gate/approval/Broker |
| this step does | typed principal/project/assignment snapshots, canonical root identity, server role allowlist, CAS assignment and owner checks |
| this step does not | 不实现 OS credential daemon、OAuth/tenant auth、外部 identity provider、Grant epoch ledger 或第二执行路径 |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Identity domain contracts | `kiana-domain/src/identity.rs` | `689bcb7cc16951e82b7689c5132c3dfeba12a52a622acfcd509725f1ba0e36b3` |
| Identity schema/exports | `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `7461cbb138e3d0a5c36a40d3ef9efaaeb80f72f096785cc68c7b075d2b7e37cb`, `5db7842856e64100c5a1f02519905850133f57c1fc6206d932f34b40f970ee7b` |
| Daemon principal/project resolver | `kiana-daemon/src/lib.rs` | `fabcc0eefb41852dc7dfd4ad7cf511bcfae02c13f2ec92ceed8f95fdfae04839` |
| Session assignment CAS/rebuild | `kiana-core/src/sessions.rs` | `f30f7dfb28af81765f4f1bc82fda77f95e40e2ed44876be57f8c3adc598cb4a1` |
| Protocol metadata | `kiana-protocol/src/lib.rs` | `1a55d24975f7092d7baf8c959fc4d3f390b75564b1ebeb75f6b697acf9a6e755` |
| CP-01 domain fixtures/guard/workflow | `kiana-domain/tests/cp01_identity.rs`, `kiana-core/tests/cp01_identity_guard.rs`, `.github/workflows/cp01-identity.yml` | `68baeac5ba3d850bc23e0858cfc8bbfa1fbaebd1aed10110b228c76daba648fc`, `47338f4bdd46ed017b7676f2fafac23a74b531e0e212023fbaac839a3c3d15df`, `07d0752aed22cb45fb52603e35f92550af18ec0ec0cc92e00d933f94ad39e8b0` |

Later CP/CI/SC steps touching these files must refresh hashes in the same commit. Hashes are source
anchors, not authenticated principal or cross-process durability evidence.

## 3. Identity and scope matrix

| Input/record | Server-owned behavior | Failure boundary |
|---|---|---|
| wire `actor_id` | DaemonHost compares it with its authenticated principal and then overwrites context actor | absent/foreign actor rejected before any capability path; wire value never grants identity |
| wire role/department | DaemonHost looks up role and intersects with principal allowed_roles; department must match role | unknown, disallowed or mismatched role rejected; model/prompt cannot select a wider role |
| project root/trust | injected `ProjectTrustAuthority` reads stored trust; `project_identity` canonicalizes directory and records device/inode (Unix) plus trust digest | unavailable/non-directory/untrusted project fails closed; root aliases share deterministic ProjectId |
| session assignment | ControlPlane writes `session.assigned` with `SessionAssignment` typed snapshot using CAS `append_expected(..., Some(0))`; existing assignment must byte-match | contention/mismatch/invalid typed snapshot returns conflict; read-only queries do not create assignment |
| continue/cancel/approval/receipt | existing session binding and persisted run identity compare actor, project, role and department | foreign session/run/role/project returns owner mismatch; no Broker effect |
| configuration/authority | effectful requests call `synchronize_authority` after server project identity resolution; assignment is persisted before dispatch | authority/project identity drift blocks admission; read-only projections do not refresh authority |

## 4. Typed contracts

`AuthenticatedPrincipalRef` exposes only principal ID, authentication method, issuer, credential
generation, expiry and a digest; no secret value is serializable. `ProjectIdentity` derives a stable
UUID-shaped `ProjectId` from canonical root bytes, retains display root separately, and binds optional
device/inode and trust revision. `SessionAssignment` binds principal/project/session/role/department,
assignment revision and authority epoch under a canonical digest; all three use `deny_unknown_fields`.

The local resolver currently creates `local-user`/`local_os` with an effectively non-expiring local
credential generation and role allowlist from `KIANA_LOCAL_ALLOWED_ROLES` (default: catalog roles).
This is an explicit local compatibility identity, not enterprise authentication. Future authenticated
providers must replace/inject the resolver without accepting caller actor/role/project/trust fields.

## 5. Migration and deny-first guard

The old `RequestMetadata` wire shape remains readable for compatibility, but missing role/department
fields use builder/executing defaults only; defaults are not authorization. A legacy session assignment
without typed identity remains readable for compatibility, while a present typed assignment is fully
validated; migration must add a new assignment fact, never rewrite old events. No query, health, receipt,
or parity request may mint an assignment or call Broker.

The CP-01 guard blocks removal of server principal overwrite, ProjectTrust injection, canonical project
identity, role allowlist, assignment CAS or typed assignment validation. It does not claim that the local
principal is externally authenticated, that role authority is durable across machines, or that a
ProjectId alone is a tenant boundary.

## 6. Fixtures and handoff

| Fixture | Purpose | Owner step |
|---|---|---|
| `principal_and_project_identity_are_server_shaped_and_deterministic` | typed contract/deterministic canonical root | CP-01 |
| `assignment_binds_principal_project_role_and_department` | assignment digest/unknown-field/tamper guard | CP-01 |
| `cp_forged_wire_actor_or_role_never_reaches_broker` | daemon overwrite/role allowlist | CP-01/04 |
| `cp_first_assignment_requires_role_authority` | assignment CAS and role authority | CP-01/08 |
| `cp_cross_project_same_session_is_rejected` | project identity/session owner fence | CP-01/08/19 |
| `cp_query_does_not_create_assignment` | read-only no-write boundary | CP-01/21 |

All runtime behavior fixtures run in GitHub Actions only. CP-01 closes the typed source boundary and
hands OS/auth provider, authority epoch, Grant intersection, approval and cross-process recovery to
CP-02+ / CI / SC steps.
