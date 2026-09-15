# CI-01 配置、凭据与身份基线

> 快照日期：2026-09-15。本文是 `CI-01` 的 source-only 基线和迁移护栏，不是
> `ConfigResolver`、`SecretStore` 或认证系统的交付声明。运行时测试只在 GitHub
> Actions 执行；本轮按工作约定不在本地运行测试，也不等待远端结果。

## 1. 范围和事实边界

| 项目 | 记录 |
|---|---|
| roadmap 卡 | [`CI-01`](../roadmap.md#step-ci-01) |
| source snapshot | `310bcec`（CI-01 开始前的干净基线；本步提交包含实现、fixture、文档和 CI） |
| feature_status | `partial`：基线、脱敏护栏和迁移记录已实现；配置/凭据/身份目标仍未完成 |
| proof_level | `source`：格式和 workspace 编译只作静态证据；运行时回执由 GitHub CI 产生 |
| 产品入口 | `DaemonHost -> kiana-daemon::model_client::from_config -> kiana-provider::ProviderGateway` |
| 本步不做 | `ConfigResolver`、`SecretStore`、`CredentialLease`、认证 ingress、身份事件、旧 parser 删除 |

当前生产 parser 是 [`kiana-provider/src/config.rs`](../../kiana-provider/src/config.rs)。
`kiana-daemon/src/model_client.rs` 的 `legacy_fixtures` 从 `#[cfg(test)]` 起才编译，
它保留旧 provider/profile 行为供兼容测试使用，不是第二条生产执行路径。CI 脚本会
检查这条边界，避免测试 fixture 无声变成生产 parser。

## 2. 当前解析优先级

### 2.1 模型入口

1. `KIANA_HARNESS_SCRIPT` 非空时，DaemonHost 注入 `ScriptedModel`。
2. 否则 `KIANA_PROVIDER=fake` 使用固定的离线模型。
3. 其他 provider 由 `ProviderGateway::from_env` 解析。

### 2.2 ProviderGateway

| 值 | 优先级（高到低） |
|---|---|
| provider | `ProviderConfig.provider` → `KIANA_PROVIDER` → `anthropic` |
| model/base URL | 显式字段 → provider 专属环境变量 → builtin |
| api key | `ProviderConfig.api_key` → provider 专属环境变量 → 缺失即拒绝（Ollama 除外） |
| profile route | profile 字段 → explicit default connection → provider 环境变量 → builtin |
| profile config | `KIANA_MODEL_PROFILES_JSON`，上限 64 KiB；未知角色、未知字段、非法引用 fail-closed |

生产 OpenAI parser 只读取 `OPENAI_API_KEY`、`OPENAI_MODEL` 和 `OPENAI_BASE_URL`。
`KIANA_OPENAI_*` 仍出现在 `kiana-services` 兼容诊断和 README/USER 旧说明中；它们
在本步明确登记为 compatibility-only，不偷偷加入产品 parser。旧 fixture parser
还不支持 `inherit_default`/`capabilities`，并使用不同的环境变量别名；两套行为
不能被称为等价。

Bootstrap 的另一条配置 overlay（仅供现状对账）顺序是：

```text
base config < remote settings < settings file < settings JSON
  < ANTHROPIC_* environment < managed settings
```

这条 overlay 仍由旧 bootstrap/auth 兼容面持有；CI-06 才会把配置解析收敛到单一、
版本化且可重载的 `ConfigSnapshot`。

## 3. 凭据和脱敏边界

CI-01 的 fixture 使用专用 sentinel，扫描以下可见通道：`debug`、`error`、`event`、
`receipt`、`argv`、`env`、`cache`。任一 raw sentinel 穿透投影都必须使 CI 失败；
失败输出只包含通道和 fixture 名，不打印 sentinel 内容。

| 通道/对象 | 当前事实 | CI-01 护栏 | 后续归属 |
|---|---|---|---|
| `ProviderConfig` Debug | 本步改为 `[REDACTED]`，不显示 `api_key` | Rust integration test | CI-02/CI-07 扩大所有 domain Debug 合同 |
| provider catalog/route | 只包含 provider、model、endpoint、revision 和 capabilities | Rust integration test 确认 catalog 不含 key | CI-08 opaque account binding |
| event/receipt/error | `kiana-domain`/core 已有分散 redaction | fixture 走 `redact_value`/`redact_text` | CI-11 统一审计 projection |
| argv/env/cache | sandbox 已清理环境；完整跨边界扫描尚未统一 | fixture 覆盖投影形状 | CI-07 lease 注入与 CI-11 全链扫描 |
| bootstrap `Config` / daemon `LocalModelConfig` | 仍持有 raw `api_key` 且部分类型派生 Debug | 本步只记录风险，不扩大修复面 | CI-02、CI-06/07 |
| `kiana-services::auth::ApiKey` / legacy client | 兼容路径仍持有 raw key，client 还有旧 unwrap | 本步标记 legacy 可达面 | CI-07/CI-10 迁移或隔离 |

`secret_ref`（例如 `vault://ci01/reference-only`）不是 secret 值，不应被 redaction
误删；测试同时断言引用保留、值被清除。`Connection.credential` 当前仍是 raw
字符串，因此不能把本步描述为 SecretStore 已接线。

## 4. 身份和迁移边界

当前 `kiana-protocol` 默认 actor 与 `DaemonHost` principal 都是固定的
`local-user`。仓库没有可供 CI-01 伪造的 `identity.authenticated` 或 migration
event kind。`legacy-local-user-event.json` 因此只表达：

- 旧 actor 可作为 compatibility-only 输入读取；
- 身份升级必须有未来显式 migration event；
- 不允许通过隐式字段覆盖、caller metadata 或 fixture 自创 event kind 升级主体。

正式的本地 ingress、Unix credential/bearer、ProjectTrust、SessionOwnership 和
身份事件属于 CI-04；稳定 ID、Principal、Assignment、SecretRef 和 authority
epoch 属于 CI-02/CI-03/CI-05。

## 5. 配置迁移边界

`docs/schemas/kiana-app-server-config-resolved.v1.schema.json` 与
`kiana-app-server-secrets.v1.schema.json` 当前是只含 schema id 的 envelope shell，
并允许 `additionalProperties`。它们是 source-only 占位，不是完整运行时 schema。

`config-migration-v0.json` 固定以下迁移口径：unknown major/field、非法 endpoint、
重复 profile 都要在版本化迁移规则存在前拒绝；不得隐式升级或把旧 JSON 当作当前
snapshot。单一 parser、canonical snapshot、trust 检查、原子 reload 和 revision
fencing 由 CI-06 实现。

## 6. GitHub CI 验收入口

| 文件 | 用途 |
|---|---|
| `scripts/ci-01-baseline.py` | 校验 fixture、schema 占位和 product/test-only parser 边界；不打印 secret 值 |
| `scripts/fixtures/config-credentials-identity/*.json` | env/profile precedence、local-user 兼容、migration deferred、secret channel fixture |
| `kiana-provider/tests/ci01_baseline.rs` | 在隔离环境锁内验证显式配置优先级、profile unknown-field 拒绝和七类输出脱敏 |
| `.github/workflows/ci01-baseline.yml` | GitHub Actions 专用 job：先跑 source guard，再跑 provider integration test |

测试顺序仍是“先拒绝、再成功/回归”：unknown profile field 和 raw sentinel 先拒绝；
显式 provider/profile precedence 以及 secret reference 保留随后回归。Rust 测试使用
环境变量锁并恢复原值，避免并行 job 内互相污染；本地不执行这些测试。

## 7. 限制和交接

- 本步没有把 raw credential 从 `Connection`、bootstrap 或 legacy services 移除。
- 本步没有实现认证主体、角色 assignment、SecretRef 解析、lease、rotation 或 revoke。
- 本地没有运行 `cargo test`、smoke 或 provider 网络调用；只在 GitHub CI 执行测试。
- 远端 CI 结果按用户要求不等待，因此本步最高只能声明 `source` proof；不能声明
  `local_behavior`、`durable` 或 `live`。
- CI-02 应先把稳定 ID/Principal/SecretRef/ConfigSnapshot 合同落到 domain；CI-06/07
  再删除或隔离旧 parser/raw credential，CI-04 再处理 local-user 的显式迁移事件。
