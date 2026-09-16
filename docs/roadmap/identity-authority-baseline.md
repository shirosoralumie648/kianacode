# P0-K1-01 server identity and authority epoch baseline

> 快照日期：2026-09-16。本页记录受保护入口的本地主体、项目身份、assignment 绑定与 authority epoch fencing；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-K1-01`](../roadmap.md#step-p0-k1-01) |
| feature_status | `implemented`（受保护入口 source slice） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | RequestMetadata → DaemonHost authenticated principal/project resolver → authority revision/epoch → SessionAssignment CAS → ControlPlane |
| this step does | 认证主体由 daemon 持有；ProjectTrust 与 canonical root/device/inode 派生 ProjectIdentity；角色和部门只能来自服务端允许的 assignment；session/run 绑定 authority epoch |
| this step does not | 不声称企业租户、OAuth/OS credential provider、跨进程身份恢复或 durable assignment store 已完成 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Authority/session/run fencing | `kiana-core/src/authority.rs`, `kiana-core/src/sessions.rs`, `kiana-core/src/lifecycle.rs` | `da1b0c435a9bbead6dc73dd0b9ac034ca7bfe596782f0d43d9226ff6e2367480`, `f0d751864ca31bd16ecc272a381c0ce77005e645f73cfe5161b164390f4090a2`, `782ba89002efb69afc9768d6950cf497b693c5095511e8dd44da9130ecc0659e` |
| Protected daemon ingress | `kiana-daemon/src/lib.rs` | `8cf74ce42364295a64c1871da8f0290deaf8d34ed80bd8b826e7433f21540483` |
| Identity/assignment contracts | `kiana-domain/src/identity.rs`, `kiana-domain/src/assignment.rs` | `689bcb7cc16951e82b7689c5132c3dfeba12a52a622acfcd509725f1ba0e36b3`, `7875396fb93fbd2f0c246d5ad7364fad3c850485507b425335ddb67f6d0df1a7` |
| Fixtures, guard and CI | `kiana-daemon/tests/p0_k1_identity.rs`, `kiana-core/tests/p0_k1_identity_guard.rs`, `.github/workflows/p0-k1-identity.yml` | `bbae9c739984566b02bf21bfeda0aeefab64e97573980a13c929f3f52bb4cf07`, `ba4bc417e94b359331ad4405e35d5aeed6566fc5390664dc1b26296443e3f222`, `f90b2d9521b72e2e6e416c8f16e5aebb05322b645733bf1d15681c90301d458f` |

Hashes are source anchors for this static slice; they are not authenticated identity material and do not promote proof beyond `source`.

## 2. 受保护入口规则

1. `actor_id`、`role_id`、`department_id` 和 `project_trusted` 是 wire 声明，不是授权事实。DaemonHost 将 actor 覆盖为 authenticated principal，并通过 ProjectTrustAuthority 重新读取项目可信度。
2. 项目路径先 canonicalize，再记录稳定 ProjectId、Unix device/inode 和 trust digest；不可解析、非目录或 trust lookup 失败时拒绝。
3. 首次 effectful session 由服务端角色 allowlist 选择 RoleSpec，并以 CAS 写入 typed `SessionAssignment`。同一 session/project 的后续请求必须与既有 assignment 字节一致；wire 角色或部门变更不能改写 assignment。
4. `synchronize_authority` 将配置/项目身份的 revision 写入 `authority` stream；其单调 `stream_version` 是数值 `authority_epoch`。SessionAssignment 与 `run.authorized` 同时记录该 epoch，revision digest 仍单独保留。
5. 查询、健康、收据和 parity 读取不刷新 authority 或创建 assignment；continue/cancel/approval/receipt 继续复用 core 的 owner 检查。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `wire_actor_cannot_grant_role_or_department` | 已建立的 server assignment 不可被 wire actor/role/department 覆盖，拒绝发生在 ControlPlane/Broker 之前 |
| `protected_ingress_uses_server_identity_and_authority_epoch` | DaemonHost 身份覆盖、ProjectTrust、assignment CAS 和 authority epoch source guard 均存在 |
| `principal_and_project_identity_are_server_shaped_and_deterministic` | CP-01 typed principal/project digest 和 canonical identity 继续由 GitHub CI 覆盖 |

`.github/workflows/p0-k1-identity.yml` 在 GitHub runner 执行 ingress fixture、source guard、fmt 和 daemon/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 principal 仍是固定 `local-user`/`local_os` 兼容身份；`KIANA_LOCAL_ALLOWED_ROLES` 是本地主体的服务端 allowlist，不是企业 RBAC 或密码学认证。
- authority epoch 来自进程内/当前 EventStore 的 authority stream version；assignment directory、Membership、Grant/Approval 全维度 revoke、跨进程 durable recovery 和 OS/OAuth provider 交给 CI/CP/SC/PD 后续步骤。
- 远程 CI 结果按用户要求不等待；没有本地 test/smoke 运行，proof level 保持 `source`。
