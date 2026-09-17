# CI-07 SecretStore 与 CredentialLease 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Effect boundary

`SecretRef` 只携带 schema、store、key、purpose、audience、generation 和 digest。新增的
`CredentialLease` 只携带 provider/account、用途、受众、endpoint digest、时间窗口和一次性
消费状态；它没有 secret value 字段，并以 `deny_unknown_fields` 拒绝未知扩展。

`kiana-provider::Connection` 现在保存 `credential_ref` 与 `SecretStore` handle，不保存
`credential: Option<String>`。`EnvSecretStore` 在请求即将发送、并已取得 connection capacity
之后读取环境值，`InlineSecretStore` 仅为既有显式 `ProviderConfig.api_key` 提供兼容路径；两者
都创建绑定 provider/purpose/audience/endpoint 的短 lease。transport 在构造认证 header 前
重新核对绑定并 consume one-shot lease，`SecretMaterial::Drop` 清理临时 buffer；snapshot、
catalog、EventLog、错误和 Broker 只看 ref/digest/lease metadata。

Broker 暴露 `consume_credential_lease` 作为 effect-boundary metadata guard：它不解析、不返回
raw secret，只拒绝过期、重放、provider/purpose/audience/endpoint 漂移的 lease。缺失 env、
非法 HeaderValue、错误 SecretRef 和不支持的 protected backend 均 fail-closed。

## Backend scope

本步提供窄适配器名称和显式拒绝边界：env 可用，inline 仅兼容旧 API；keyring、file、OS
适配器返回 `credential_backend_unsupported:*`，不会悄悄 fallback 到环境变量。真实 keyring/
0600 文件权限、OS credential API、跨进程 rotation/revocation 与 OAuth 生命周期留给 CI-09/11
及对应平台切片；本步不声称 HSM、durable 或 live provider 证明。

## CI-only evidence

`.github/workflows/ci07-secret-store.yml` 执行 domain lease、provider store/projection、Broker
binding、core source guard 和 workspace test-target compile。负向夹具覆盖 missing/expired/
wrong-purpose/wrong-endpoint/replay、未知字段和 sentinel 不进入 snapshot/catalog；成功夹具
使用 inline fake store issue/consume 一次性 lease。工作流会由本提交触发；本地只做格式、静态
编译和 diff 检查，不等待 CI 结果。

## Limitations

- explicit `ProviderConfig.api_key` 仍会被兼容性 inline adapter 在进程内持有；新配置应使用
  `SecretRef`/env 或后续受保护 backend。
- 当前 lease 是进程内短期 metadata，重启、跨进程 revoke、generation CAS、OAuth refresh
  single-flight 和真实 provider effect 仍未完成。
- header/client 内部可能复制认证值；清理 `SecretMaterial` 是 best-effort，不是物理内存擦除
  或外部系统删除证明。
