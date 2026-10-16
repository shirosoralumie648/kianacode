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

## 4. 2026-10-04 assignment project binding guard

`DaemonHost::context_from_assignment` now rejects a server-resolved assignment whose project ID
does not match the daemon-derived `ProjectIdentity` before trust, department, or authority snapshot
assembly. This keeps a foreign project assignment from reaching later authority joins and returns
the structured `assignment_project_mismatch` failure. The SC-07 source guard pins this ordering.

```text
source_snapshot: parent `b3a21616` plus `038071b5`; kiana-daemon/src/lib.rs; kiana-core/tests/sc07_authority_guard.rs
worktree_status: project identity is checked before trust/department/authority assembly; DaemonHost::handle still uses the compatibility role path and is intentionally not silently rewired
command_argv: isolated rustfmt --edition 2021 --check; isolated git diff --check; staged diff check; root cherry-pick `073b6ad2`; push-triggered `.github/workflows/ci.yml` includes `sc07_authority_guard`; no local test/build/check/clippy/smoke
cwd·environment: repository root and isolated worktree `/tmp/kiana-sc07-authority-assignment`; Linux/bash; GitHub Actions is the runtime test authority
fixture·cassette: SC-07 authority guard source marker; changed-source CI pending
exit_code: isolated format/diff checks 0; changed-source CI pending; no local runtime exit code
status_change: SC-07 roadmap row 118 remains `🔄`; feature_status=partial; proof_level=source
proof-level_change: none; no local_behavior, durable, live or physical promotion
limitations: `DaemonHost::handle` still lacks server-owned OrganizationId/assignment provisioning, explicit assignment enforcement and durable revoke/CAS; this slice only hardens the existing helper
reviewer: source review of project identity derivation and assignment mismatch ordering; no local runtime reviewer
```

## 5. Current-head focused local authority lane (2026-10-05)

```text
source_snapshot: current dirty worktree; kiana-domain trust/assignment snapshots; kiana-core security authority/guards; kiana-daemon assignment helper
worktree_status: SC-07 remains roadmap row 118 🔄; typed assignment remains disconnected from DaemonHost::handle; no automatic commit/push/merge/rebase
command_argv: serial bounded `cargo test -p kiana-domain --test sc07_trust_snapshots --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test sc07_authority_snapshot --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test sc07_authority_guard --locked --offline -- --test-threads=1`; `cargo test -p kiana-daemon --test control_plane --no-run --locked --offline`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, 32 MiB test stack, stripped/non-incremental profile; one Cargo/test worker; 5.02s elapsed, 3.16s user, 1.13s system, max RSS 700204 KiB (~683.8 MiB), 0 swaps
fixture·cassette: domain trust/department/assignment 3/3; Core authority snapshot 2/2; Core source guard 1/1; daemon control-plane target compile; foreign project/root/role/trust revision, wire trust escalation and untrusted effect denial
exit_code: all four commands 0
status_change: focused SC-07 contract/deny behavior recorded as `local_behavior`; whole card remains `feature_status=partial`, `proof_level=source`
proof-level_change: focused snapshot and guard selectors `source` → `local_behavior`; no whole-card durable/live/physical promotion
limitations: DaemonHost::handle does not consume AssignmentDirectory/context_from_assignment; server OrganizationId/provisioning, default unassigned and multi-role semantics, durable CAS/revoke/refresh, cross-process recovery and external/live/physical auth proof remain open
reviewer: integration agent with read-only SC-07 audit; focused local authority reviewer
```

## 6. 2026-10-08 nil typed assignment identity denial

`RoleAssignment`, `ProjectAssignment` and `ResolvedAssignment` now reject nil typed IDs with
structured identity errors. Domain fixtures recompute each assignment digest after mutating IDs,
so the assertions exercise the identity guard rather than failing earlier on digest mismatch.
`SecurityAuthoritySnapshot::from_parts` also rejects a digest-correct resolved assignment with a nil
project ID, demonstrating that the authority join inherits the domain validator.

```text
source_snapshot: current dirty worktree; kiana-domain assignment contracts; kiana-core authority snapshot and source guard
worktree_status: selected nil-identity contract is locally verified; whole SC-07 remains partial/source and roadmap row 118 🔄
command_argv: RED exact domain selector; GREEN domain `co03_assignment` and `sc07_trust_snapshots`; GREEN Core `sc07_authority_snapshot`, `sc07_authority_guard` and `co03_assignment`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo build job/test thread, 32 MiB test stack, stripped/non-incremental test profile, locked offline dependencies; maximum observed RSS 4055948 KiB on RED and 3944396 KiB on domain GREEN; Core GREEN 330588 KiB; CPU 88%/85%/81%; 0 swaps
fixture·cassette: RED 101 expected; domain GREEN 7/7 + 3/3; Core GREEN 1/1 + 1/1 + 3/3; role/project/resolved nil identities and digest-correct Core authority snapshot
exit_code: RED 101 expected; all GREEN and static checks 0
status_change: selected identity denial promoted to local_behavior; whole card remains partial/source
proof-level_change: focused nil-identity rejection source → local_behavior; no durable/live/physical promotion
limitations: DaemonHost::handle still lacks mandatory server-owned assignment; OrganizationId provisioning, default-unassigned/multi-role migration, durable CAS/revocation/refresh, cross-process recovery and external/live/physical proof remain open
reviewer: root integration agent; focused domain/Core reviewer
```
