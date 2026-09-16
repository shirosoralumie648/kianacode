# P4-J7-07 provider extraction baseline

> 快照日期：2026-09-16。本文记录独立 `kiana-provider` crate、daemon 装配和 legacy `kiana-services` test-only facade 边界；本地不运行测试，运行时/guard 夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P4-J7-07`](provider.md#step-p4-j7-07) |
| source snapshot | `7753930`（P4-J7-06 parent）加本步 source guard/CI evidence；最终 commit 记录在 git history |
| feature_status | `implemented`（独立 provider crate + daemon production wiring source） |
| proof_level | `source`；静态编译与远程 guard 不提升为 local_behavior/durable/live/physical |
| authority path | DaemonHost → `kiana-daemon::model_client::from_config` → `kiana_provider::ProviderGateway` → ports ModelClient; Runner 只依赖 ports |
| this step does | 确认 provider 编解码/transport/gateway 已从 services 迁至 `kiana-provider`；provider manifest/source 不依赖 services/core/entrypoints/runner；daemon 生产路径使用 Gateway，旧 services provider 仅保留在 `#[cfg(test)] legacy_fixtures` 兼容夹具 |
| this step does not | 不删除旧 `kiana-services::api::provider` 公共兼容模块、不双发真实请求、不让 Runner 依赖 provider 实现；旧 facade 的逐协议 DTO 差异和移除计划留给后续 provider cards |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Provider crate | `kiana-provider/Cargo.toml`, `kiana-provider/src/lib.rs` | `d28e132ba131e8fce452e8dcece77c1d3129b35c19e18563505ae2cbfb5ecb79`, `f8b04b4b2749dd872a88aaf2025ea2ca3b69894e861fb5e5c4c2d79424908ed` |
| Daemon/runner/core dependency boundary | `kiana-daemon/src/model_client.rs`, `kiana-runner/Cargo.toml`, `kiana-core/Cargo.toml` | `32aec09c6a47cb9f090fd3749164c693b9687c24d7efb0b9d0c9db709cbaec53`, `12545ad290c34b8fc0898aeb06b43273e84b3746c55a976c996f76a8f175f113`, `b6fa02ec15c6ad44d19b06555d0fd8e0a040fc1554fb6e2ad36877ab8c76de91` |
| Extraction guard and CI | `kiana-core/tests/p4_j7_07_extraction_guard.rs`, `.github/workflows/p4-j7-07-provider-extraction.yml` | `5d3f768fba9ed20680f9d1798f789034897ee38ad7e1ee8e3c6cf052819d22ab`, `9683a33f309ed12d22e13c96b61aaf1754a255c39ef3652ba1e90aabeee8b507` |

## 2. Extraction contract

`kiana-provider` 只依赖 domain/ports 与网络编解码库，公开 `ProviderGateway` 实现唯一 ports `ModelClient`；它接收 server-installed `ModelAssignment`、PreparedModelCall 和 budget permit，不能创建 Runner loop、执行工具或读取 core/entrypoint 状态。`kiana-runner` 的 ModelClient re-export 保持低层依赖方向。

daemon 顶层生产 `from_config` 处理 cassette/fake 后，其在线分支只构造 `kiana_provider::ProviderGateway::from_env` 并包一层 metadata guard。历史 `kiana-services` provider 类型仅位于 `#[cfg(test)] legacy_fixtures`，为离线兼容测试保留；这不是生产模型路径或第二事实源。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `provider_extraction_keeps_one_production_gateway_and_legacy_services_test_only` | provider manifest/source 无 legacy runtime dependency，daemon Gateway 生产装配存在，services occurrence 全在 cfg(test) 后 |
| `missing_model_still_fails_closed_after_extraction` | provider gateway/config 缺 credential/route 时返回结构化 unavailable，而不调用 Runner/工具（由现有 provider CI fixture 复用） |
| `daemon_uses_one_provider_gateway_after_migration` | daemon 只通过一个 ProviderGateway→ModelClient 端口进入既有 Harness（source guard + compile） |

`.github/workflows/p4-j7-07-provider-extraction.yml` 在 GitHub runner 执行 extraction source guard、fmt 和 provider/daemon test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- `kiana-services::api::provider` 仍是兼容公共 surface，尚未逐协议委托/删除；后续 P4-J7-12/14+ 负责 codec/accumulator 收口和差异 fixture。
- provider crate 的 HTTP/live 请求、凭据/route/profile、capacity、usage/receipt 证明分别由 P4-J7-08+ 与 INT/ER/DEP 提供；本步不声称 live。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
