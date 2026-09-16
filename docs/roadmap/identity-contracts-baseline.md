# CI-02 identity and authority contract baseline

> 快照日期：2026-09-16。本页记录 Principal/Membership/Assignment、SecretRef、ProviderAccount、ConfigSnapshot 和 AuthoritySnapshot 的 domain contracts；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CI-02`](../roadmap.md#step-ci-02) |
| feature_status | `implemented`（typed identity/config/authority source contracts） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | Principal/Membership/Assignment → AuthoritySnapshot → ControlPlane admission；ConfigSnapshot → Provider route；SecretRef → CredentialStore/Broker effect boundary |
| this step does | 稳定 identity IDs、Principal/Membership 状态、digest/expiry/epoch、无秘密 SecretRef、ProviderAccount、无秘密 ConfigSnapshot 和 AuthoritySnapshot contracts；unknown-field/raw-secret/epoch regression fail-closed |
| this step does not | 不实现 SecretStore/credential lease、OAuth/PKCE、durable identity DB、assignment revoke projection 或 provider live transport；CI-03/04/05/06/07+ 负责 |

## 2. Contract rules

- `Principal` 绑定 typed PrincipalId、kind/status、authenticated metadata 和生命周期；`AuthenticatedPrincipalRef` 只含 opaque ID/method/issuer/generation/expiry/digest。
- `Membership` 绑定 PrincipalId/OrganizationId、有效窗口和 authority_epoch；RoleAssignment/ProjectAssignment 继续复用 CO-03 typed contracts。
- `SecretRef` 只包含 store/key/purpose/audience/generation/reference_digest，不存在 raw secret 字段；Debug/serde 只能看到 reference metadata，CredentialStore/Broker/Provider transport 才能在 effect boundary 解析值。
- `ProviderAccount` 是外部 provider/account metadata，不是 Kiana principal 或项目授权；`ConfigSnapshot` 只保存受信 source refs、非秘密 effective config、config/trust revision，检测 raw token/bearer/key 字段。
- `AuthoritySnapshot` 绑定 principal/org/project/session owner/role/department/policy/data boundary、assignment IDs 和 monotonic authority_epoch；`validate_current_epoch` 拒绝 rollback/stale，snapshot 本身不替代每次 dispatch/approval/effect revalidation。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `ci02_domain_contracts_are_versioned_and_secret_free` | Principal/Membership/SecretRef/ProviderAccount/ConfigSnapshot/AuthoritySnapshot round-trip/validate，serde 不泄露 raw secret，authority epoch drift 拒绝 |
| `ci02_snapshot_contracts_reject_raw_secret_fields_and_epoch_drift` | ConfigSnapshot 中 raw token/bearer 载荷 fail-closed |
| `identity_contracts_keep_secret_and_authority_metadata_typed` | 稳定 ID、schema、strict DTO、protocol export 和 authority/secret guard 存在 |

`.github/workflows/ci02-identity-contracts.yml` 在 GitHub runner 执行 domain fixtures、core source guard、fmt 和 domain/core/protocol test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 这些是 source-level value objects；没有 durable Principal/Membership/Authority store、跨进程 recovery、rotation/revoke 或完整 assignment ledger。
- `SecretRef` 的存在不证明凭据存在或 provider 可用；`ProviderAccount` 不授予 project/role/capability，`AuthoritySnapshot` 仍需在 CP/Broker 边界重验。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
