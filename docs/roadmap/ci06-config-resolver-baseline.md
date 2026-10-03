# CI-06 单一配置解析器基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

> 2026-10-04 更新：下方新增的 overlay/reload 切片取代旧的纯 parser 限制；仍未接入
> DaemonHost 多项目配置来源。当前验证命令只在 GitHub CI 运行。

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

当前工作流以对应 target 的 `--no-run` 编译和现有 provider 回归替代上方历史全 workspace
编译命令。本地只读取和编辑源码、格式化及查看 Git 差异；不执行测试、构建、检查或验证
脚本，推送后不等待 GitHub CI。

## 限制与交接

- `Connection` 已使用 opaque SecretRef 和 provider-only SecretStore；显式
  `ProviderConfig.api_key` 仍由兼容 inline adapter 持有，不进入配置快照。
- 当前 DaemonHost provider 组合仍由启动环境构造，按项目配置来源与 reload、跨进程
  ConfigSnapshotStore、远端/非本地配置源和 durable fencing 留后续。
- 不宣称认证主体、OAuth、外部/live/physical provider effect 或生产 KMS/secret 生命周期。

## 2026-10-04 trusted overlay and atomic reload

`ProviderGateway::from_workspace` now resolves validated project text through the same resolver
and connection builder used by `from_env`. Explicit API configuration takes precedence over
environment, workspace defaults and builtins; explicit environment profiles override project
profiles with the same name. Trust facts come from `WorkspaceConfigTrust`, with matching admitted
and current server revisions, and are checked before parsing. Snapshots contain no secret values.

`reload_workspace` constructs and validates the whole candidate before publication, then compares
the expected current snapshot again while holding the write lock. Invalid, stale or busy reloads
preserve the previous snapshot. Every route binds the whole candidate, trust and a monotonic
process-local generation, so changing another profile or reverting to earlier content cannot
revive an old prepared call. An admitted attempt pins its snapshot through budget consumption,
capacity wait and transport; a reload during that interval returns `config_reload_busy`.

Existing quota windows, semaphores and circuit state survive a reload or scope removal/re-addition;
retained scopes are bounded, and numerical capacity policy changes are rejected. CI fixtures cover
these behaviors with held admission/transport futures and loopback fake providers, alongside the
existing configuration, credential, route-admission and provider-contract regressions.

Changed-source GitHub validation is pending at integration. `CI-06` remains partial/source:
the daemon has no provider workspace config path resolver, project-scoped gateway integration or
authoritative current trust revision API. A single gateway cannot install one project's overlay
as the default for other projects. Cross-process snapshots/generations and cancellation after a
remote request was sent remain separate requirements.
