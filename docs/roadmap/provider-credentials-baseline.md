# P4-J7-09 provider credentials and outbound endpoint baseline

> 快照日期：2026-09-16。本文记录 SecretRef-like credential handling、auth header/endpoint validation、redirect/proxy/TLS boundary and connection revision isolation；本地不运行测试，运行时/guard 夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P4-J7-09`](provider.md#step-p4-j7-09) |
| source snapshot | `6fb453e`（P4-J7-08 parent）加本步 fixtures/source guard；最终 commit 记录在 git history |
| feature_status | `implemented`（provider credential/endpoint validation source + CI fixtures） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | operator credential reference/config → ProviderGateway connection validation → frozen ModelRoute/credential revision → no-redirect HTTP client; project text/trust cannot override endpoint |
| this step does | credential 只从 explicit config/env reference 解析并在内存使用；坏 HeaderValue、缺 key、userinfo/query/fragment、非 TLS/非 loopback endpoint、invalid concurrency/streaming fail-closed；HTTP client 禁 redirect/proxy，连接 revision 包含 credential digest |
| this step does not | 不记录 raw key/body/header，不关闭 TLS certificate validation，不自动跟随 redirect，不让项目/role text 改 endpoint；SecretStore/OAuth rotation、DNS pinning and live network evidence remain later scope |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Provider credential/config | `kiana-provider/src/config.rs` | `8194ee74bd1e7a237a9f2d553b143e7851583f93c773019f05ec1dca3ef1fbae` |
| HTTP transport/gateway | `kiana-provider/src/transport.rs`, `kiana-provider/src/lib.rs` | `56d1fd235705792b9fa64a22e5cc480e75cfa9d69de6c8d98f2698c06dde9392`, `eaa616c19849dac01fdcef0a8e25becb6ed8f776985f0ff076b0d0333ebec45f` |
| Daemon assembly guard | `kiana-daemon/src/model_client.rs` | `b75295fd9a804e1a63f88fd1c891c8491a36447227d7be5ef67e133fc844def1` |
| Fixtures and CI | `kiana-provider/tests/p4_j7_09_credentials.rs`, `kiana-core/tests/p4_j7_09_credentials_guard.rs`, `.github/workflows/p4-j7-09-credentials.yml` | `a39b29d33e602a37fd5641f87bb625419dcce331f759dfa45867c8bfbaf5ac01`, `541d90f805a909d4fee22fceaee7a9803a0c61d94201280e187cf303fb35fd6a`, `55fdbf15b619464bec808edd9b68011eb54b2b35ad7e2c295fc99e3df3bd323d` |

## 2. Credential and endpoint contract

Provider config accepts `api_key_env`/explicit API key only at the configuration boundary; `ProviderConfig` Debug redacts it, configuration snapshot stores only `credential_ref=sha256:*`, and route revision includes a digest of the credential. Missing non-Ollama credentials, malformed header values and invalid references return structured `ModelError` before network send.

Endpoint construction rejects URL userinfo, query and fragment, requires HTTPS except explicitly local loopback HTTP, pins `localhost` to `127.0.0.1`, and builds a reqwest client with redirects disabled and ambient proxies disabled. Base URL is an operator/provider setting; project root/trust/role prompt is never consulted to widen it. Transport bounds/timeouts remain in the connection snapshot and errors preserve request-sent classification.

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `invalid_auth_header_never_panics_and_missing_secret_fails_closed` | newline header 与缺 Anthropic key 返回结构化错误，不 panic/不发送 |
| `endpoint_userinfo_query_fragment_and_non_tls_are_rejected_but_loopback_is_allowed` | userinfo/query/fragment/public HTTP 拒绝，registered loopback HTTP 可配置 |
| `provider_credentials_and_endpoint_boundaries_are_server_owned` | HeaderValue、credential revision、Policy::none、Gateway/project authority 代码边界存在 |

`.github/workflows/p4-j7-09-credentials.yml` 在 GitHub runner 执行 provider credential/endpoint fixtures、core source guard、fmt 和 provider/daemon/core test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前 credential 生命周期是进程内 connection snapshot；无 SecretStore、rotation/revocation fence、OAuth refresh merge 或跨进程 lease。
- endpoint 防重定向和 TLS policy 已在 source/fixture 层固定，但未做真实 DNS rebinding、代理链或外部 provider live 证明；loopback fixture 不等于 live success。
- P4-J7-10/11 继续分离 capability discovery、role route、permit/预算与 network authority，不能把 credential 可用性当 capability grant。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
