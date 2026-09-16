# CI-04 protected daemon ingress baseline

> 快照日期：2026-09-16。本页记录 loopback/protected ingress metadata、server-owned local principal 覆盖、ProjectTrust/session owner boundary 与历史 local-user migration；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CI-04`](../roadmap.md#step-ci-04) |
| feature_status | `implemented`（daemon ingress validation + legacy migration contract source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | RequestMetadata protected fields → DaemonHost ingress validation → authenticated principal/ProjectTrust → SessionAssignment/ControlPlane |
| this step does | 可选 instance/origin/host/opaque credential-ref 校验、protected_local credential presence gate、legacy local-user migration typed fact；旧 wire actor/role/trust 仍不可扩权 |
| this step does not | 不声称已实现 OS credential/keyring/bearer verifier、Unix socket peer credentials、Origin/Host TLS listener、企业 OAuth/tenant auth 或跨进程 identity recovery |

## 2. Ingress rules

- `validate_protected_ingress` 在 DaemonHost `handle` 最前执行；Origin/Host 若存在必须指向 loopback，拒绝 userinfo/non-loopback，instance ID bounded，credential_ref 先过 SecretRef 校验。
- `identity_mode=protected_local` 必须同时携带 instance_id 与 opaque credential_ref；`legacy_local_user` 仅是显式兼容模式，不把 actor_id 变成认证证据。
- 后续 DaemonHost 仍以 server-owned principal 覆盖 actor，读取 ProjectTrust/canonical ProjectIdentity，绑定 session assignment；查询不 mint assignment，跨 project/session owner 继续由 core 拒绝。
- `IdentityMigration` 只记录旧 principal 到新 principal 的迁移原因/时间/digest；它不携带 secret，也不授予角色、项目或 capability。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `protected_ingress_rejects_bad_origin_host_and_missing_credential` | 非 loopback Origin/Host 和 protected mode 缺 credential 在 core 前拒绝 |
| `protected_loopback_metadata_accepts_opaque_credential_reference` | 合法 loopback metadata 仅携带 SecretRef，不含 raw bearer/key |
| `local_user_migration_is_an_explicit_non_authorizing_fact` | local-user migration strict/digest 事实可读但不产生 authority |
| `daemon_ingress_is_checked_before_core_and_legacy_identity_is_explicit` | DaemonHost ingress guard、protocol optional fields 和 migration source boundary 存在 |

`.github/workflows/ci04-ingress.yml` 在 GitHub runner 执行 domain/daemon ingress fixtures、core source guard、fmt 和 domain/protocol/daemon/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 protected fields 是协议 metadata 校验，不是密码学认证；真正 bearer/Unix peer/keyring/OS credential resolver、instance ownership 和 cross-process migration 仍需后续 adapter/CI/SC/PD。
- `SecretRef` 与 `IdentityMigration` 只传 opaque ref/digest；Available credential、Principal 或 ProjectTrust 都不自动授权 capability，必须重新走 ControlPlane。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
