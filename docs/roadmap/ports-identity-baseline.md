# CI-03 identity, credential and configuration ports baseline

> 快照日期：2026-09-16。本页记录 `kiana-ports` 的 IdentityResolver、CredentialResolver、ConfigSnapshotStore 与 Rotation/Revoke 分层；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CI-03`](../roadmap.md#step-ci-03) |
| feature_status | `implemented`（ports contracts + secret-free resolution boundary source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | protected ingress → IdentityResolver → AuthoritySnapshot; ConfigSnapshotStore → non-secret config; CredentialResolver/RotationRevoke → opaque SecretRef/metadata at effect boundary |
| this step does | identity/authority resolution port、credential status/ref port、config snapshot CAS port、credential rotation/revoke generation port；所有端口不返回 raw secret |
| this step does not | 不实现 production adapter、SecretStore、OAuth/PKCE、durable identity DB、provider transport 或完整 cross-process recovery；CI-04+ 负责 |

## 2. Port rules

- `IdentityResolver` 接受已认证 principal 与 daemon-derived project，不信任 wire actor/role/trust；返回 Principal/AuthoritySnapshot typed metadata。
- `CredentialResolver` 只返回 `CredentialResolution { SecretRef, state, expiry, resolved_digest }`；Core/Runner 看不到原始 token/key，Missing/Expired/Revoked/Unknown 必须保守处理。
- `ConfigSnapshotStore` 读写 immutable non-secret `ConfigSnapshot`，发布带 expected revision；配置快照不承担用户授权或 secret 生命周期。
- `CredentialRotationPort`（别名 `RotationRevokePort`）使用 observed generation CAS，返回新 SecretRef，不允许 stale rotate/revoke 覆盖更新代次。
- Port trait 是依赖反转边界，不是权限授予；ControlPlane、Broker、Provider transport 仍需在 effect boundary 重验 authority/config/credential revision。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `ports_never_return_raw_secret_to_core` | CredentialResolution 只序列化 SecretRef/status/expiry/digest，不含 raw secret |
| `ports_keep_identity_config_credential_and_rotation_boundaries_separate` | 四类 port、错误/secret-free metadata 和 CI-02 domain contracts 均有 source guard |

`.github/workflows/ci03-ports.yml` 在 GitHub runner 执行 ports fixture、core source guard、fmt 和 domain/ports/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前没有生产 IdentityResolver/ConfigSnapshotStore/CredentialResolver adapter；CI fake 仅验证接口形状和 deny-first metadata，不证明 OS/keyring/file/OAuth 实际可用。
- `CredentialResolution.state=Available` 仍不等于凭据有效或动作获授权；SecretStore/lease/transport 只能在后续 CI-06/07/08 通过 Broker 接入。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
