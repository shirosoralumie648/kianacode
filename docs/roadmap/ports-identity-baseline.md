# CI-03 identity, credential and configuration ports baseline

> 快照日期：2026-10-02。本页记录 `kiana-ports` 的 IdentityResolver、CredentialResolver、ConfigSnapshotStore 与 Rotation/Revoke 分层；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CI-03`](../roadmap.md#step-ci-03) |
| feature_status | `partial`（ports contracts + secret-free resolution boundary source；adapter and recovery limits remain open） |
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
| `config_snapshot_store_revision_cas_is_deterministic` | 测试内存 fake 返回稳定快照；当前 config revision 可 CAS 更新；旧 revision 被拒绝且已发布快照保持不变 |
| `credential_rotation_port_generation_cas_rejects_stale_without_mutation` | 测试内存 fake 对 rotate/revoke 均按 observed generation CAS；旧 generation 拒绝且 SecretRef 不变，当前 generation 成功递增 |
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
status_change: none; CI-03 remains in progress with `feature_status=partial` and its existing limitations
proof-level_change: none; source only
limitations: run `36897771406` was still in progress at this source capture; the later receipt in §6 records the corrected fixture passing in run `36916662965`, while unrelated fixtures kept that ports job red. No production resolver, store, rotation or revoke adapter is implemented here.
reviewer: CI-03 implementation agent source review; no runtime test reviewer
```

## 6. Last exact unified CI receipt (2026-10-02)

The current unified workflow reached the CI-03 ports fixture on run `36916662965`.
Both CI-03 tests passed. The `kiana-ports` shard was still red because unrelated
EQ-07, PD-30/32/34 and BQ-05 fixtures failed in the same shard; this receipt does
not promote the CI-03 roadmap row or claim a green workspace run.

```text
source_snapshot: `12fbcd21aaa677ed07f28106b02ab1be89b2d927`; `kiana-ports/src/lib.rs`; `kiana-domain/src/identity_contracts.rs`; `kiana-ports/tests/ci03_ports.rs`; `kiana-core/tests/ci03_ports_guard.rs`; `docs/roadmap/ports-identity-baseline.md`
worktree_status: isolated `/tmp/kiana-ci03-audit-20261002` on `step/ci03-audit-20261002`; docs-only receipt; no port, domain, test, manifest or lockfile implementation changed
command_argv: `gh run view 36916662965 --job 110509907468 --log`; filtered the CI log for the text tests/ci03_ports.rs; `git diff --check`; `git show --check HEAD`
cwd/environment: `/tmp/kiana-ci03-audit-20261002`; Linux; GitHub Actions is the only test executor; no local cargo test/build/check/fmt/clippy/smoke command was run
fixture or cassette: `kiana-ports/tests/ci03_ports.rs::credential_resolution_metadata_is_strict_and_fail_closed` passed; `kiana-ports/tests/ci03_ports.rs::ports_never_return_raw_secret_to_core` passed; job `110509907468` also reported unrelated BQ-05/EQ-07/PD-30/32/34 failures
exit_code: CI-03 fixture assertions exited 0 in the GitHub log; the containing `kiana-ports` job concluded failure for unrelated targets; no full-workflow green result was observed or awaited
status_change: none; CI-03 remains 🔄 with the existing production-adapter and recovery limitations
proof-level_change: none; `feature_status=partial`, `proof_level=source`; no local_behavior, durable, live or physical promotion
limitations: this receipt proves only the two CI-03 ports fixtures on the observed runner; it does not prove production IdentityResolver/ConfigSnapshotStore/CredentialResolver adapters, SecretStore/lease/OAuth behavior, rotation/revoke durability, or a green workspace CI run
reviewer: isolated CI-03 ports audit; no local runtime test reviewer
```

## 7. ConfigSnapshotStore explicit-revision fixture (2026-10-02)

The CI-03 ports fixture now uses test-local in-memory fakes to pin stable configuration reads,
explicit-revision compare-and-swap, and credential rotation/revoke generation compare-and-swap.
Stale configuration and credential revisions return conflicts without changing the current
snapshot or SecretRef; matching credential generations advance the reference. The fixtures do
not cover `None`/initial-create semantics, durable stores, lease lifecycle, or cancellation after
an effect.

The original ConfigSnapshotStore fixture commit `4455675553ece5437e7b615654f18cd2aa437c62` was
integrated by `dc1df0e8496966071aef6a0be001a6ab6d9430f4`, which is an ancestor of pushed master
`db8a46067c1fd33b07f8e5a515d793e729aa1779`. The fixture is mapped to the `kiana-ports` test
shard in the unified `.github/workflows/ci.yml`; it has no CI receipt because run `36986316487`
cancelled that shard at `Run shard` before fixture output. Run `36993318357` later marked the
ports shard failed on an earlier source head, but its logs were unavailable at observation and
no ConfigSnapshotStore fixture result is inferred. The older receipt in §6 predates this
ConfigSnapshotStore test and does not prove it.

```text
source_snapshot: base `db8a46067c1fd33b07f8e5a515d793e729aa1779` plus this isolated CI-03 slice; prior ConfigSnapshotStore source `4455675553ece5437e7b615654f18cd2aa437c62` integrated by `dc1df0e8496966071aef6a0be001a6ab6d9430f4`; `kiana-ports/tests/ci03_ports.rs`; `kiana-ports/src/lib.rs`; `docs/roadmap/ports-identity-baseline.md`
worktree_status: isolated `/tmp/kiana-ci03-generation-cas-20261002` based on pushed master; test-local ConfigSnapshotStore and CredentialRotationPort fakes cover explicit revision/generation CAS; no production adapter, authority behavior, manifest or lockfile changed
command_argv: source review; `git diff --check`; no local test/build/check/fmt/clippy/smoke command was run
cwd·environment: `/tmp/kiana-ci03-generation-cas-20261002`; Linux; GitHub Actions is the only test executor
fixture·cassette: `config_snapshot_store_revision_cas_is_deterministic` has no receipt: run `36986316487` cancelled `Tests (kiana-ports)` at `Run shard` before fixture output. New `credential_rotation_port_generation_cas_rejects_stale_without_mutation` is wired through the same unified `kiana-ports` shard and has not run remotely. Run `36993318357` targeted an earlier source head and its full logs were unavailable; no fixture result is inferred.
exit_code: source review and `git diff --check` only; fixtures not run locally; no new GitHub receipt observed or awaited
status_change: none; CI-03 remains 🔄 with `feature_status=partial` and `proof_level=source`
limitations: test-local in-memory fakes only; initial config publish, durable persistence, production adapters, lease lifecycle, cancellation/recovery and credential rotation/revoke durability remain unproven
reviewer: manual source review; no runtime test reviewer
```
