# P4-J7-08 provider configuration snapshot baseline

> 快照日期：2026-09-16。本文记录 provider/connection/profile/model/credential source snapshot、cassette/live mode 和 streaming policy 边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P4-J7-08`](provider.md#step-p4-j7-08) |
| source snapshot | `bd67223`（P4-J7-07 parent）加本步 source/CI；最终 commit 记录在 git history |
| feature_status | `implemented`（typed provider config snapshot + daemon selection guards source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | operator config/env → ProviderGateway connection/profile snapshot (secret-free) → ModelAssignment.profile → PreparedModelCall route/configuration_revision; cassette mode remains explicit daemon selection |
| this step does | 新增 ProviderProfileSnapshot/ProviderConfigSnapshot，分离 provider/protocol/connection/model/profile/profile_version/credential_ref/source；Gateway catalog 暴露 digest-only configuration；daemon 先校验 `KIANA_MODEL_MODE` 与 `KIANA_STREAMING`，cassette/live 冲突和 missing cassette fail-closed；profile unknown/inherit conflict/key/streaming/concurrency 规则由 provider config 固定 |
| this step does not | 不把 credential 原值放入 snapshot/catalog/receipt；不热更新活动 Run 的 route；不让 cassette 覆盖显式 live mode；不宣称配置快照 durable 或真实 provider live 成功 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain snapshots | `kiana-domain/src/provider_config.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs`, `kiana-domain/src/model.rs` | `9f126160b2287278db8bb58ab72b8fe0e42972d3a3d3c5944f5be463a31cf004`, `282972d0d05b07f65f56b64040561193da4b01f77a92663d8715fcf6aff146bf`, `ab3bb341c5abe8b23cd59ede3e4d604002c11bebef711b981d3985d5a6c43aa9`, `95bc42f1a1a01d196bb4d10c701b16b22ddd0e5f44a5c120ea6492b1068729e9` |
| Provider config/gateway | `kiana-provider/src/config.rs`, `kiana-provider/src/lib.rs` | `8194ee74bd1e7a237a9f2d553b143e7851583f93c773019f05ec1dca3ef1fbae`, `eaa616c19849dac01fdcef0a8e25becb6ed8f776985f0ff076b0d0333ebec45f` |
| Daemon selection guard | `kiana-daemon/src/model_client.rs` | `b75295fd9a804e1a63f88fd1c891c8491a36447227d7be5ef67e133fc844def1` |
| Fixtures and CI | `kiana-provider/tests/p4_j7_08_config.rs`, `kiana-core/tests/p4_j7_08_config_guard.rs`, `.github/workflows/p4-j7-08-provider-config.yml` | `0697873f1411b3021c6e61172437c877bbc7863d2fe1c81beac3d40e34ced867`, `ebe8917a2512f07e1751c71c6075c66024b801211ba66c2cbc0980677a57d3dc`, `ed38d667e8492dac5cdfe693bff10c320be40d67b0fc30238fd5cd7590fc68ae` |

## 2. Configuration contract

`ProviderConfigSnapshot` 固定 selection mode、version、profiles 和 snapshot digest；每个 `ProviderProfileSnapshot` 绑定 route/capabilities、profile_version、secret-free `credential_ref=sha256:*`、source 和 digest。原始 API key 只在 connection transport 内存中使用，不进入 catalog 或日志。

`ProviderGateway::configuration_snapshot` 从已解析 connections 生成 deterministic projection；`catalog()` 保留旧 connections/capabilities 字段并添加 configuration snapshot。每个 route 的 `configuration_revision` 对 provider/protocol/model/origin/credential revision/declared capabilities/streaming 变化敏感，PreparedModelCall 继续冻结并核对 route。

`KIANA_MODEL_PROFILES_JSON` 显式 map 只接受 RoleCatalog 中的 profile；未知 profile、unknown field、inherit_default 与其它字段并用、credential env 非法/缺失、capability/endpoint/concurrency/streaming 非法均拒绝。未配置 profile 时可使用 default connection，但不改变 server ModelAssignment 的 profile identity。

daemon 的 `KIANA_MODEL_MODE` 只接受 `live` 或 `cassette`：script cassette 与 explicit live 同时出现、explicit cassette 没有 script 都返回 unavailable；未设置 mode 时保留 legacy cassette 优先。无论 mode，invalid `KIANA_STREAMING` 先 fail-closed。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `profile_snapshot_is_secret_free_and_changes_revision_on_route_change` | snapshot 不含 API key，profile source/route digest 稳定，model route 改变生成新 snapshot digest |
| `unknown_profile_and_invalid_streaming_policy_fail_closed` | unknown profile 与 invalid streaming policy 不回退到其它 profile/default |
| `provider_configuration_is_snapshotted_and_cassette_mode_is_explicit` | domain/provider/daemon 源码存在 snapshot、selection conflict、streaming guard，single Gateway 继续使用 frozen route |

`.github/workflows/p4-j7-08-provider-config.yml` 在 GitHub runner 执行 provider config fixtures、core source guard、fmt 和 domain/provider/daemon/core test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 snapshot 在 Gateway 进程内生成，尚无 durable ConfigSnapshotStore、热更新 CAS、跨进程 epoch 或活动 Run 重启恢复。
- default connection fallback 是未配置 profile 的兼容连接选择，不代表未知 role/model profile 被接受；显式 profile map 缺项仍由 `explicit_profiles` 让 Gateway 拒绝。
- credential source 只以 digest/reference 形式出现；SecretStore/rotation/revocation、TLS/live request 和跨入口设置 DTO 由 CI/P4-J7-09+ 处理。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
