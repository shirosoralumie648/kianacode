# CO-03 server-owned assignment baseline

> 快照日期：2026-09-16。本文记录角色任命、项目范围、有效期、撤销和服务端身份重验证的 source slice；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CO-03`](companyos.md#step-co-03) |
| source snapshot | `9b75d21` parent plus this step's uncommitted source; final commit is recorded in git history |
| feature_status | `implemented`（domain assignment directory + ports + daemon/core guard source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| authority path | authenticated principal → server-owned AssignmentDirectoryPort → typed ResolvedAssignment → daemon context binding → core Company assignment revalidation → existing ControlPlane/EventLog path |
| this step does | 分离 RoleSpec 模板与 RoleAssignment 实例；ProjectAssignment 绑定组织/项目和角色任命；有效窗口、revision、authority epoch、digest、撤销级联和 ambiguity fail-closed；daemon 从自身 principal 与 root-derived ProjectIdentity 解析，core 提供 Company 边界重验证 |
| this step does not | 不把客户端 role/actor/organization 字段变成授权；不新增第二执行循环或 EventStore；当前 `InMemoryAssignmentDirectory` 只是显式非 durable adapter，尚未接入持久身份数据库、EventLog assignment facts 或跨进程恢复 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain assignment contracts | `kiana-domain/src/assignment.rs` | `7875396fb93fbd2f0c246d5ad7364fad3c850485507b425335ddb67f6d0df1a7` |
| Stable IDs/schema/export | `kiana-domain/src/ids.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `80575707b8b3bbb04b692073705b3d19723bc64d6f1a44883c367c5a1ea9eabb`, `5c99ae3078959115bc004209dab5753d065fde027c546e77aa571003d79a4d25`, `bb7fdab9d44b304a8a6778849878b17ae4897e4135d0a09b0bfa7dca266545e1` |
| Identity port | `kiana-ports/src/lib.rs` | `a5edf3ea8c9e2efe90c9ce0a78e8e5aeea1d575d80e509fe7913620ca24f1ba9` |
| Core Company guard | `kiana-core/src/company.rs`, `kiana-core/src/lib.rs` | `31dc904dfc0c8bebb26649c6c95d682c3ff435d98037631daca051dc5383ec2a`, `fe9dbca50064dc5a5558c99cba958850aae02464143a9b35802ab5f9f32d285b` |
| Daemon context path | `kiana-daemon/src/lib.rs` | `2cb13d2eb49b628a2a3f72114207800481cd5fca30549d23556e4664e46298be` |
| Protocol surface | `kiana-protocol/src/lib.rs` | `89108ec93ab521b6db16235d4100f1f887e43aefd03e9410593c499aacae4557` |
| Fixtures and CI | `kiana-domain/tests/co03_assignment.rs`, `kiana-core/tests/co03_assignment.rs`, `kiana-core/tests/co03_assignment_guard.rs`, `.github/workflows/co03-assignments.yml` | `d656efdd59e36c1a2fed23aa191f4b14d44e0aaa56b8380674b3bfc406cf79d6`, `4cdcb844c25ea9745129104acd3efc7853c39bc57579173bcb2d06f603f94a7d`, `7dd82091aa676fe68f3916bbfc40362bf63b199c404c91894e96fcc0e2c0ed4d`, `352425a18c48e7e5e68074d41cbee5bd8c45faa406007639d3ef2994f99d3deb` |

## 2. Assignment contract

`RoleAssignment` 是不可变的、带 revision/authority epoch/digest 的任命快照，角色模板仍由 `RoleSpec` 查找，部门不允许由调用方独立改写。`ProjectAssignment` 必须匹配 principal、organization、项目清单和角色窗口；重复 identity、越界窗口、unknown fields、digest 漂移和非 canonical project list 直接拒绝。

`AssignmentDirectory::resolve` 只接受 server-authenticated principal、绑定 organization/project 和当前时间，要求恰好一个同时有效的角色/项目 assignment；过期、撤销、缺失或多重命中 fail-closed，客户端请求不同角色返回 `assignment_role_mismatch`。`revoke_role` 递增角色 epoch/revision 并级联撤销项目 assignment，旧 snapshot 只能在 core revalidation 前被视为展示材料。

`ResolvedAssignment` 带有效窗口、assignment/project IDs、actor/organization/project/role/department、revision/authority epoch 和 resolution digest。它是短期解析结果，不是可跨请求转借的 approval 或 capability grant。

## 3. Server identity path

`kiana-ports::AssignmentDirectoryPort` 是身份适配边界，生产实现可替换当前内存 adapter。`DaemonHost::resolve_assignment_for_project` 先用 daemon-owned `ProjectTrustAuthority`/filesystem identity 得到 ProjectId，再用 daemon-owned principal 解析 assignment；wire role 只作为被校验的查询键。`context_from_assignment` 拒绝 actor/role impersonation，写入 server principal、role 和 department 后调用 `kiana-core::validate_company_assignment`。

core helper 在 Company read/write、Continue、审批消费或效果派发前可复用：先跑既有 workspace/trust/role guard，再核对 principal、role/department、assignment validity window、principal expiry 和 resolution snapshot freshness。它不把 assignment snapshot 当作事实源，调用方仍必须从 AssignmentDirectoryPort 重新解析。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `revoked_assignment_blocks_company_command_and_approved_continuation` | role revoke 级联 project revoke；resolve 不再返回 authority |
| `client_role_name_cannot_impersonate_sponsor` | 同一 principal 不能用 wire role 将 builder assignment 冒充 sponsor |
| `active_assignment_survives_reopen_with_the_same_scope` | typed resolved assignment 可序列化重开且 scope/department 保持不变 |
| `assignment_expiry_and_unknown_fields_fail_closed` | 到期时间和未知字段拒绝；source guard 验证 ports/daemon/core 边界存在 |
| `company_assignment_guard_rejects_expiry_and_actor_or_role_drift` | core Company guard 在写入/继续前拒绝 assignment expiry、actor drift 和 role/department drift |

`.github/workflows/co03-assignments.yml` 在 GitHub runner 执行 domain fixtures、core guard、fmt 和 ports/core/daemon test-target compile；本地只做格式、静态编译和 diff 检查。

## 5. 限制与交接

- assignment directory 目前是进程内 snapshot；无 durable membership/assignment store、CAS journal、跨进程 recovery 或 authority epoch persistence。
- 现有 `RequestContext`/CompanyEvent 仍保留兼容的 actor/role 字段和 String project stream；本步没有批量 upcast 历史 CompanyState，也没有把 assignment facts 伪装成已持久化事实。
- daemon assignment API 尚未改变所有旧入口的 context 构造；接入每个 dispatch/Continue/approval/effect path 需要后续 CO-05/CO-07 与身份持久化切片继续收口。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。

## 6. Current-head focused local assignment lane (2026-10-05)

本地聚焦回归修正了 source guard 中引用旧方法名的测试标记，并验证现有
assignment deny-first 行为；这只提升所选 fixtures 的证明等级，不改变整张卡的
durability 或入口接线结论。

```text
source_snapshot: current dirty worktree; kiana-domain assignment directory; kiana-core Company assignment guard; CO-03 source guard
worktree_status: corrected the guard marker from stale `resolve_assignment` to the declared `pub fn resolve`; no assignment behavior, manifest or lockfile change
command_argv: bounded serial `cargo test -p kiana-domain --test co03_assignment --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test co03_assignment --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test co03_assignment_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; max RSS 131372 KiB on the largest selector, 0 swaps
fixture·cassette: domain 6/6; core assignment guard 1/1; core source guard 1/1; expiry, revocation cascade, actor/role impersonation, digest/window drift and assignment boundary markers
exit_code: all three selectors 0
status_change: selected CO-03 contracts and deny-first behavior recorded as `local_behavior`; roadmap row 062 remains `✅`, whole card feature_status=`implemented`, proof_level=`source`
proof-level_change: selected fixtures `source` → `local_behavior`; no durable/live/physical promotion
limitations: in-process assignment snapshot, durable membership/epoch facts, all DaemonHost dispatch/Continue/approval/effect paths, cross-process recovery and external/live/physical identity proof remain open
reviewer: integration agent; focused local assignment reviewer
```

## 7. 2026-10-08 nil typed identity denial

The shared assignment contracts now reject nil IDs on `RoleAssignment`, `ProjectAssignment` and
`ResolvedAssignment`. The deny fixtures recompute their digests after mutation and cover the IDs
that define role, organization, project and assignment identity. SC-07 authority snapshot coverage
confirms the resolved-assignment rule is enforced at the Core join as well.

```text
source_snapshot: current dirty worktree; kiana-domain/src/assignment.rs; domain CO-03 assignment fixtures; Core SC-07 authority snapshot fixture
worktree_status: selected contract hardening only; CO-03 row remains unchanged and the SC-07 whole card remains partial/source
command_argv: RED exact `co03_assignment` selector; GREEN domain `co03_assignment` + `sc07_trust_snapshots`; GREEN Core `co03_assignment` + `sc07_authority_guard` + `sc07_authority_snapshot`
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB test stack, stripped/non-incremental profile, locked offline dependencies; peak RSS 4055948 KiB RED, 3944396 KiB domain GREEN, 330588 KiB Core GREEN; 0 swaps
fixture·cassette: RED expected failure; domain 7/7 and 3/3; Core 1/1, 1/1 and 3/3; digest-correct nil identity mutations rejected
exit_code: RED 101 expected; all GREEN 0
status_change: selected nil identity contract behavior recorded as local_behavior; no roadmap card promoted
proof-level_change: focused contract denial source → local_behavior; no durable/live/physical promotion
limitations: in-process assignment directory, mandatory DaemonHost request-path enforcement, durable membership/epoch facts, cross-process recovery and external/live/physical identity proof remain open
reviewer: root integration agent with read-only SC-07 assignment audit
```

## 8. 2026-10-13 resolved assignment zero valid-from denial

`ResolvedAssignment::validate` 现在把 `valid_from_unix_ms == 0` 与其它无效窗口一起拒绝，避免一个
带有有效 digest 的 forged snapshot 以零起始时间进入 Company assignment guard。该修复只收紧 typed
assignment 边界，没有把 snapshot 误写成 durable authority fact。

```text
source_snapshot: current dirty worktree; kiana-domain/src/assignment.rs; kiana-domain/tests/co03_assignment.rs; kiana-core/tests/co03_assignment_guard.rs
worktree_status: CO-03 row 062 remains ✅ / implemented/source; SC-07 row 118 remains 🔄 / partial/source; selected zero-valid-from denial is local_behavior
command_argv: RED exact `resolved_assignment_rejects_zero_valid_from_time`; GREEN same selector; full domain `co03_assignment`; Core `co03_assignment`; Core source guard `co03_assignment_guard`; all locked/offline through bounded serial launcher
cwd·environment: repository root Linux/bash; one Cargo/compiler worker and one Rust test thread per command, `RUST_MIN_STACK=33554432`, stripped/non-incremental profile; RED max RSS 248856 KiB / CPU 36%; GREEN max RSS 4079808 KiB / CPU 92%; domain rerun 82932 KiB / 65%; Core exact deny 327476 KiB / 90%; Core target rerun 1214132 KiB / 94%; guard 1217484 KiB / 92%; all observed swap counters 0
fixture·cassette: forged resolved assignment keeps a recomputed digest while changing only valid_from_unix_ms to zero; RED expected 101, exact GREEN 0; domain 8/8, Core 2/2 (expiry/actor/role plus zero-valid-from), source guard 1/1
exit_code: RED 101 expected; exact GREEN/domain/Core/guard 0
status_change: zero valid-from snapshots now fail closed at the domain validator; selected assignment behavior source → local_behavior; no roadmap card promotion
proof-level_change: focused in-process denial source → local_behavior; no durable/live/physical promotion
limitations: AssignmentDirectory remains in-process; DaemonHost compatibility dispatch does not yet consume `context_from_assignment`; org membership provisioning, default-unassigned and multi-role policy, durable authority/CAS/recovery, external/live/physical identity proof remain open
reviewer: root integration agent with read-only SC-07 audit; focused CO-03 assignment reviewer
```
