# CI-06 单一配置解析器基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Resolver boundary

生产 `ProviderGateway::from_env` 只经 `kiana-provider::ConfigResolver::resolve` 构造连接、
profile 和 `ProviderConfigSnapshot`；底层 `config::connections` 是 resolver 的唯一实现，
不会再由 gateway/entrypoint 另行解析。解析顺序保持显式 `ProviderConfig` → provider 环境
变量 → builtin，`KIANA_MODEL_PROFILES_JSON` 仍 strict `deny_unknown_fields`、bounded profile
和 capability/endpoint 校验。

`ConfigResolver::parse_workspace` 在解析任何 workspace JSON 前要求调用方已证明
`project_trusted=true`，然后校验 `kiana.provider-config.v1`/version、大小、provider/model、
profile allowlist、环境变量名称和 HTTPS/loopback URL；userinfo、query、fragment、非 TLS
远端、未知字段/major、继承冲突和越界输入 fail-closed。workspace overlay 可生成 domain
`ConfigSnapshot`：只含 non-secret config、source ref、config revision 和 trust revision，
不会返回或序列化 raw API key。它不做网络请求、不从配置文本推断身份或权限。

daemon 中的 `legacy_fixtures` profile parser 仍被 `#[cfg(test)]` 隔离，作为兼容回归材料；
它不参与产品组合根，CI source guard 固定这一边界。配置 snapshot/revision 变化只能使后续
admission 重新核验，不隐式切换已运行的 route。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-provider --test ci06_config_resolver --locked -- --test-threads=1
cargo test -p kiana-daemon --test ci06_config_resolver --locked -- --test-threads=1
cargo test -p kiana-core --test ci06_config_resolver --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- `ProviderConfig`/`Connection` 仍由 provider adapter 在最后边界持有 raw credential；CI-07
  才接入 SecretStore/CredentialLease，CI-08/10 再补 route admission/policy。
- Workspace parser 是可复用纯 API；当前 DaemonHost provider 组合仍由启动环境构造，按项目
  动态 reload、跨进程 ConfigSnapshotStore、远端/非本地配置源和 durable fencing 留后续。
- 不宣称认证主体、OAuth、外部/live/physical provider effect 或生产 KMS/secret 生命周期。
