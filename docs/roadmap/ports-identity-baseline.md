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
| `credential_resolution_metadata_is_strict_and_fail_closed` | 完整 resolution metadata 可 round-trip；raw secret unknown field、过期和 digest drift 均拒绝 |
| `ports_keep_identity_config_credential_and_rotation_boundaries_separate` | 四类 port、错误/secret-free metadata 和 CI-02 domain contracts 均有 source guard |

统一 `.github/workflows/ci.yml` 的 test shard 在 GitHub runner 执行 ports fixture、core source guard、fmt 和 domain/ports/core test-target 编译；旧的独立 CI-03 workflow 已合并删除。本地不运行测试。

## 4. 限制与交接

- 当前没有生产 IdentityResolver/ConfigSnapshotStore/CredentialResolver adapter；CI fake 仅验证接口形状和 deny-first metadata，不证明 OS/keyring/file/OAuth 实际可用。
- `CredentialResolution.state=Available` 仍不等于凭据有效或动作获授权；SecretStore/lease/transport 只能在后续 CI-06/07/08 通过 Broker 接入。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。

## 5. Credential resolution digest correction (2026-10-02)

Source review found that `CredentialResolution::validate` checked the `sha256:` prefix and
total string length, but did not require the 64-byte suffix to be hexadecimal. A same-length
value such as `sha256:gggg...` therefore passed the claimed digest-shape guard. Validation now
requires exactly 64 ASCII hexadecimal bytes, with a CI fixture for the equal-length invalid case.
The valid digest and existing malformed-prefix cases remain covered.

```text
source_snapshot: d4a85ebd0862380549115e91324a9887cd0ded3d + CI-03 digest-shape correction
worktree_status: isolated /tmp/kiana-ci03-audit on audit/ci03-ports-d4a85ebd; only ports validator, CI fixture and this baseline changed
command_argv: gh run view 36897771406 --job 110489410130; git diff --check
cwd/environment: /tmp/kiana-ci03-audit; Linux; local test/build/check/clippy/fmt/smoke commands not run
fixture or cassette: kiana-ports/tests/ci03_ports.rs::credential_resolution_metadata_is_strict_and_fail_closed; GitHub CI ports job
exit_code: 0 for source/diff checks; GitHub run 36897771406 ports job was in progress when inspected, so the pre-fix remote result is unobserved
status_change: none; CI-03 source status remains implemented with its existing limitations
proof-level_change: none; source only
limitations: the corrected fixture has not run; latest master CI run is pending/unobserved for kiana-ports; no production resolver, store, rotation or revoke adapter is implemented here
reviewer: CI-03 implementation agent source review; no runtime test reviewer
```

## 6. Latest unified CI receipt (2026-10-02)

The current unified workflow reached the CI-03 ports fixture on run `36916662965`.
Both CI-03 tests passed. The `kiana-ports` shard was still red because unrelated
EQ-07, PD-30/32/34 and BQ-05 fixtures failed in the same shard; this receipt does
not promote the CI-03 roadmap row or claim a green workspace run.

```text
source_snapshot: `12fbcd21aaa677ed07f28106b02ab1be89b2d927`; `kiana-ports/src/lib.rs`; `kiana-domain/src/identity_contracts.rs`; `kiana-ports/tests/ci03_ports.rs`; `kiana-core/tests/ci03_ports_guard.rs`; `docs/roadmap/ports-identity-baseline.md`
worktree_status: isolated `/tmp/kiana-ci03-audit-20261002` on `step/ci03-audit-20261002`; docs-only receipt; no port, domain, test, manifest or lockfile implementation changed
command_argv: `gh run view 36916662965 --job 110509907468 --log`; filtered the CI log for `tests/ci03_ports.rs`; `git diff --check`; `git show --check HEAD`
cwd/environment: `/tmp/kiana-ci03-audit-20261002`; Linux; GitHub Actions is the only test executor; no local cargo test/build/check/fmt/clippy/smoke command was run
fixture or cassette: `kiana-ports/tests/ci03_ports.rs::credential_resolution_metadata_is_strict_and_fail_closed` passed; `kiana-ports/tests/ci03_ports.rs::ports_never_return_raw_secret_to_core` passed; job `110509907468` also reported unrelated BQ-05/EQ-07/PD-30/32/34 failures
exit_code: CI-03 fixture assertions exited 0 in the GitHub log; the containing `kiana-ports` job concluded failure for unrelated targets; no full-workflow green result was observed or awaited
status_change: none; CI-03 remains 🔄 with the existing production-adapter and recovery limitations
proof-level_change: none; `feature_status=partial`, `proof_level=source`; no local_behavior, durable, live or physical promotion
limitations: this receipt proves only the two CI-03 ports fixtures on the observed runner; it does not prove production IdentityResolver/ConfigSnapshotStore/CredentialResolver adapters, SecretStore/lease/OAuth behavior, rotation/revoke durability, or a green workspace CI run
reviewer: isolated CI-03 ports audit; no local runtime test reviewer
```
