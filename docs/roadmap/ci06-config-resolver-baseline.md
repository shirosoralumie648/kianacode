# CI-06 单一配置解析器基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

> 2026-10-04 更新：下方新增的 overlay/reload 切片取代旧的纯 parser 限制；仍未接入
> DaemonHost 多项目配置来源。当前验证命令只在 GitHub CI 运行。

## Resolver boundary

生产 `ProviderGateway::from_env` 只经 `kiana-provider::ConfigResolver::resolve` 构造连接、
profile 和 `ProviderConfigSnapshot`；底层 `config::connections` 是 resolver 的唯一实现，
不会再由 gateway/entrypoint 另行解析。解析顺序保持显式 `ProviderConfig` → trusted
workspace/project → provider 环境变量 → builtin，`KIANA_MODEL_PROFILES_JSON` 仍 strict
`deny_unknown_fields`、bounded profile
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
trusted workspace defaults, environment and builtins; a trusted workspace profile takes
precedence over an environment profile with the same name. Trust facts come from
`WorkspaceConfigTrust`, with matching admitted
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

Changed-source GitHub validation is pending at integration. `CI-06` remains partial with selected
resolver behavior locally verified:
the daemon has no provider workspace config path resolver, project-scoped gateway integration or
authoritative current trust revision API. A single gateway cannot install one project's overlay
as the default for other projects. Cross-process snapshots/generations and cancellation after a
remote request was sent remain separate requirements.

## 2026-10-05 focused local receipt

The former CI-only statement above is historical for the original baseline. Current-head bounded
selectors have now been run locally; they validate the resolver and source boundaries only and do
not close the missing DaemonHost product-chain integration.

```text
source_snapshot: current dirty worktree; kiana-provider resolver, kiana-daemon/core CI-06 guards and fixtures
worktree_status: roadmap row 166 remains 🔄; no project-scoped gateway, trust-revision or snapshot-store integration added
command_argv: bounded serial cargo test -p kiana-provider --test ci06_config_resolver --locked --offline -- --test-threads=1; cargo test -p kiana-daemon --test ci06_config_resolver --locked --offline -- --test-threads=1; cargo test -p kiana-core --test ci06_config_resolver --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; CARGO_BUILD_JOBS=1, RUST_TEST_THREADS=1, RUST_MIN_STACK=33554432, stripped non-incremental tests; provider max RSS 85452 KiB, daemon 91080 KiB, Core 85644 KiB, swaps 0
fixture·cassette: provider 9/9; daemon source guard 1/1; Core source guard 1/1; strict schema, trust-before-parse, secret-free snapshot, precedence, reload fencing and busy/stale denial
exit_code: all commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/ci06-{provider,daemon,core}.{log,resources}`
status_change: selected CI-06 resolver behavior is local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: selected selectors source → local_behavior; no durable/live/physical promotion
limitations: DaemonHost still constructs `ProviderGateway::from_env`; project config path/trust revision/reload wiring, production ConfigSnapshotStore, cross-process generation durability and cancellation after remote send remain open
reviewer: integration agent with CI-06 read-only audit; no human release reviewer
```

## 2026-10-07 duplicate-key local receipt

```text
source_snapshot: current dirty worktree; kiana-provider/src/resolver.rs; kiana-provider/src/config.rs; CI-06 provider/daemon/Core fixtures
worktree_status: roadmap row 166 remains 🔄; workspace/env profile JSON now rejects duplicate object keys before typed BTreeMap conversion; no DaemonHost project-scoped route, manifest/lockfile or second execution path change
command_argv: RED `cargo test -p kiana-provider --test ci06_config_resolver config_resolver_rejects_duplicate --locked --offline -- --test-threads=1`; GREEN provider `cargo test -p kiana-provider --test ci06_config_resolver --locked --offline -- --test-threads=1`; daemon guard `cargo test -p kiana-daemon --test ci06_config_resolver --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test ci06_config_resolver --locked --offline -- --test-threads=1`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread; `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; provider GREEN max RSS 423888 KiB (~414.0 MiB), daemon guard 130136 KiB (~127.1 MiB), Core guard 130112 KiB (~127.1 MiB); CPU 59%/54%/67%; all observed swap counters 0
fixture·cassette: RED workspace duplicate and environment duplicate both accepted last value and failed expected deny; GREEN provider 11/11, daemon guard 1/1, Core guard 1/1; stable errors `config_workspace_invalid` and `model_profile_config_invalid`; existing trust/schema/precedence/reload fixtures remain green
exit_code: RED 101 expected; GREEN provider/daemon/Core/format/diff 0; first RED compile-only source guard mismatch was corrected to require `parse_bounded_json` + `serde_json::from_value` and is not counted as behavior evidence; logs/resources `/tmp/kiana-roadmap-20261007-ci06/{red2,provider,daemon2,core}.{log,resources}`
status_change: selected duplicate-key and resolver boundary behavior promoted from source to local_behavior; whole CI-06 remains `feature_status=partial`, roadmap 🔄
proof-level_change: duplicate profile rejection source → local_behavior; no durable/live/physical promotion
limitations: DaemonHost still constructs `ProviderGateway::from_env`; no project config path/trust revision/current authority integration, durable cross-process ConfigSnapshotStore/generation, migration/upcaster registry, loopback redirect fixture or cancellation-after-remote-send proof
reviewer: integration agent with CI-06/SC-07 read-only audit; no human release reviewer
```

## 2026-10-08 trusted workspace precedence local receipt

The generic roadmap precedence target is now enforced by the production resolver. This corrects
the older 2026-10-04 baseline wording that documented environment profiles winning over a project
profile: explicit `ProviderConfig` still wins, then trusted workspace/project values win over
provider environment variables, and environment profiles only fill profile names absent from the
workspace. Workspace-declared `api_key_env` is selected before a provider's default environment
variable, so a host-level key cannot silently replace the project's opaque reference.

```text
source_snapshot: current dirty worktree; kiana-provider/src/config.rs; kiana-provider/tests/ci06_config_resolver.rs; kiana-daemon/tests/ci06_config_resolver.rs; kiana-core/tests/ci06_config_resolver.rs
worktree_status: roadmap row 166 remains 🔄 / feature_status=partial / proof_level=source; selected provider precedence and profile merge behavior now local_behavior; no DaemonHost project-scoped gateway, manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-provider --test ci06_config_resolver trusted_workspace_precedes_provider_environment_for_default_route --locked --offline -- --test-threads=1`; GREEN provider `cargo test -p kiana-provider --test ci06_config_resolver --locked --offline -- --test-threads=1`; daemon guard `cargo test -p kiana-daemon --test ci06_config_resolver --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test ci06_config_resolver --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental tests; RED max RSS 360496 KiB / 52% CPU; provider GREEN 429124 KiB / 78% CPU; daemon guard 948188 KiB / 88% CPU; Core guard 129760 KiB / 53% CPU; all measured runs reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261008-ci06-{red,provider,daemon,core}.{log,resources}`
fixture·cassette: RED old resolver selected an invalid environment endpoint and returned `model_endpoint_credentials_or_query_denied`; GREEN provider 12/12, daemon guard 1/1 and Core guard 1/1; fixture also asserts workspace provider/model and `CI06_PROJECT_KEY` credential digest beat `KIANA_PROVIDER`, `KIANA_OLLAMA_*` and `ANTHROPIC_API_KEY`
exit_code: RED 101 expected; GREEN provider/daemon/Core 0
status_change: selected CI-06 source selectors move source → local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: provider/workspace precedence and same-name profile merge source → local_behavior; no durable/live/physical promotion
limitations: DaemonHost still constructs `ProviderGateway::from_env`; project config path/trust revision wiring, durable cross-process ConfigSnapshotStore/generation, migration/upcaster registry, redirect fixture, lease lifecycle and cancellation-after-remote-send remain open
reviewer: root integration agent with CI-06 read-only audit; no human release reviewer
```

## 2026-10-08 provider/model control-character denial

The shared domain helper `contains_forbidden_provider_config_control` recognizes NUL, CR and LF.
Workspace top-level fields and workspace profiles are rejected by the resolver; explicit,
environment and environment-profile routes are rejected by connection construction before a route
can reach request compilation. Tests use embedded line breaks because the existing environment
reader intentionally trims surrounding whitespace.

```text
source_snapshot: current dirty worktree; kiana-domain/src/provider_config.rs; kiana-provider/src/resolver.rs; kiana-provider/src/config.rs; kiana-provider/tests/ci06_config_resolver.rs; kiana-domain/tests/ci06_config_values.rs; kiana-core/tests/ci06_config_resolver.rs; kiana-daemon/tests/ci06_config_resolver.rs
worktree_status: roadmap row 166 remains 🔄 / feature_status=partial / proof_level=source; selected provider/model lexical denial is locally verified; no DaemonHost project route, durable snapshot store or second execution path
command_argv: RED `cargo test -p kiana-provider --test ci06_config_resolver config_resolver_rejects_provider_model_control_characters_from_every_source --locked --offline -- --exact --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test ci06_config_values --locked --offline -- --test-threads=1`; provider `cargo test -p kiana-provider --test ci06_config_resolver --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test ci06_config_resolver --locked --offline -- --test-threads=1`; daemon guard `cargo test -p kiana-daemon --test ci06_config_resolver --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; RED max RSS 428324 KiB / 55% CPU; domain GREEN max RSS 4044540 KiB / 86% CPU; final provider GREEN 359144 KiB / 74% CPU; stable Core guard 128596 KiB / 65% CPU; stable daemon guard 943628 KiB / 89% CPU; all measured runs reported 0 swaps
fixture·cassette: RED old resolver returned `Ok(WorkspaceConfig)` for trusted workspace provider `ollama\n`; GREEN domain 1/1 (NUL/CR/LF helper), provider 13/13, Core guard 1/1 and daemon guard 1/1; assertions cover workspace top-level/profile fields, explicit provider/model, environment provider/model and environment profile provider/model with stable source-specific errors
exit_code: RED 101 expected; domain/provider/Core/daemon/fmt/diff 0
status_change: selected CI-06 provider/model control-character denial moves source → local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: lexical input denial before provider route construction source → local_behavior; no durable/live/physical promotion
limitations: no DaemonHost project config path/current trust revision wiring, cross-process snapshot/generation durability, migration/upcaster registry, redirect fixture, lease lifecycle or cancellation-after-remote-send proof; error reporting does not claim all possible Unicode/control policy
reviewer: root integration agent with CI-06 read-only config audit; focused provider route reviewer
```

## 2026-10-08 configuration-source provenance denial

`ProviderConfigSnapshot` no longer guesses the source of a default profile from its name. Each
`Connection` carries the source assigned at the resolver boundary, so an explicit API
`ProviderConfig`, trusted workspace default, route environment override and builtin route are
distinguishable in the canonical snapshot. Named environment/workspace profiles retain the
`Profile` source label. This closes the selected CI-06 provenance bug while keeping the broader
project-scoped source registry and durable snapshot work open.

```text
source_snapshot: current dirty worktree; kiana-provider/src/config.rs; kiana-provider/tests/ci06_config_resolver.rs; kiana-core/tests/ci06_config_resolver.rs
worktree_status: roadmap row 166 remains 🔄 / feature_status=partial / proof_level=source; snapshot source is carried by Connection and read directly by config::snapshot
command_argv: RED/ GREEN explicit-default selector; provider `ci06_config_resolver` target; provider `p4_j7_08_config` regression; Core `ci06_config_resolver` guard; final format/diff/roadmap/workspace checks
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped non-incremental profile; RED max RSS 359696 KiB / 53% CPU; GREEN selector 429660 KiB / 83% CPU; provider 360092 KiB / 73% CPU; P4-J7-08 312260 KiB / 83% CPU; Core guard 129916 KiB / 55% CPU; all sampled swap counters 0
fixture·cassette: explicit default route, builtin route with fixture credential, `KIANA_PROVIDER=ollama` route override and named profile; provider CI-06 15/15, P4-J7-08 3/3, Core guard 1/1; old code labeled explicit default `BuiltinDefault`, new code returns `Explicit`
exit_code: RED 101 expected; all GREEN/regression/guard/static checks 0
status_change: selected canonical source labeling moves source → local_behavior; whole CI-06 remains partial/source
proof-level_change: resolver snapshot provenance source → local_behavior; no DaemonHost project-scoped source registry, durable cross-process generation, migration, lease, live or physical proof
limitations: this slice classifies resolver inputs only; workspace and environment-profile origin remain represented by the existing Explicit/Profile labels, and no provider network request or external effect was exercised
reviewer: root integration agent with CI-06 read-only configuration audit; focused provenance reviewer
```

## 20. Environment profile secret-reference bound (2026-10-13)

Environment profile parsing now applies the same 128-byte `api_key_env` bound already enforced
for trusted workspace profiles. This prevents a 129-byte to 1024-byte environment variable name
from passing the provider profile parser and reaching `SecretRef`, whose lower-level key shape
bound is intentionally broader.

```text
source_snapshot: current dirty worktree; kiana-provider/src/config.rs; kiana-provider/tests/ci06_config_resolver.rs
worktree_status: roadmap row 166 remains 🔄 / feature_status=partial; selected environment-profile secret-reference bound is local_behavior; no DaemonHost project route, manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-provider --test ci06_config_resolver config_resolver_rejects_oversized_environment_profile_secret_ref --locked --offline -- --exact --test-threads=1`; GREEN same selector; provider `ci06_config_resolver`; daemon and Core CI-06 guards; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo/compiler job and one Rust test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental locked/offline profile; RED max RSS 368392 KiB / 70% CPU, GREEN 437704 KiB / 76% CPU, provider 85500 KiB / 47% CPU, daemon 4075148 KiB / 87% CPU, Core 130440 KiB / 49% CPU, all observed swap counters 0; receipts under `/tmp/kiana-roadmap-20261010-evidence/ci06-{oversized-env-red,oversized-env-green,provider-16,daemon-guard,core-guard-final}.*`
fixture·cassette: dynamic 129-byte `[A-Z]` environment variable name is bound to a sentinel; old parser returned `Ok` and RED failed expected denial; GREEN returns `model_credential_reference_invalid`; provider 16/16 and daemon guard 4/4 remain green
exit_code: RED 101 expected; exact GREEN/provider/daemon/Core guard 0; `cargo fmt --all --check`, `git diff --check` and workflow validator 0
status_change: selected environment-profile `api_key_env` lexical bound moves source → local_behavior; whole CI-06 remains feature_status=partial, proof_level=source
proof-level_change: environment-profile schema/resource bound source → local_behavior; no durable/live/physical promotion
limitations: DaemonHost project config path/trust revision/reload wiring, production ConfigSnapshotStore, cross-process generation durability, migration/upcaster registry, lease lifecycle and cancellation-after-remote-send remain open
reviewer: root integration agent with CI-06 read-only resolver audit; focused environment-profile boundary reviewer
```
