//! Provider 连接与环境配置的解析、组装与快照。
//!
//! ## 这个文件在系统里的位置
//!
//! ```text
//!   环境变量 / KIANA_MODEL_PROFILES_JSON / 显式 ProviderConfig
//!        |
//!        v
//!   【本文件 connections()】—— 逐个 profile 构造 Connection（路由 + 凭据 + 限额 + 闸门）
//!        |
//!        v
//!   【本文件 snapshot()】—— Connection -> 无密钥的 ProviderConfigSnapshot
//!        |
//!        v
//!   resolver.rs::Resolution -> ProviderGateway
//! ```
//!
//! ## 一个 `Connection` 是什么
//!
//! 它是“**一个可用的上游通道**”的全部运行时状态：往哪个 URL 发、用哪个模型、
//! 用哪份凭据、允许多少并发、配额窗口、熔断器、以及一个已经配好策略的 `reqwest::Client`。
//! 一个 profile 名对应一个 Connection。
//!
//! ## 配置快照与重载
//!
//! 连接、凭据、限额、熔断器在 resolver 中作为完整候选构造，Gateway 原子替换快照。
//! 已准入的调用持有旧快照直到 effect 结束；同一上游的容量计数和熔断状态跨重载保留。
//!
//! ## 上游 / 下游
//!
//! - 上游：`resolver.rs::ConfigResolver::resolve` 调用本文件的 `connections()` 与 `snapshot()`。
//! - 下游：产出的 `Connection` 被 `request.rs`（组请求体）、`transport.rs`（发请求、
//!   用限额/熔断/信号量）、`lib.rs`（列目录）消费。
//!
//! ## ⚠ 本文件绝不返回裸凭据
//!
//! 配置里读到的 `api_key` 只被用来（a）算一个 `credential_revision` 摘要、
//! (b) 塞进 `InlineSecretStore`。真正取值是发请求那一刻由 `credentials.rs` 做的。
//! 快照 `snapshot()` 里只有 `credential_ref.reference_digest`，没有密钥本身。
//!
//! ## 与 env 的关系
//!
//! 本文件会**读取**环境变量（`env()` 辅助函数），但从不写入。
//! 读取的键包括：`KIANA_PROVIDER`、`ANTHROPIC_*` / `OPENAI_*` / `GEMINI_*` /
//! `KIANA_OLLAMA_*`、`KIANA_MODEL_PROFILES_JSON`、`KIANA_STREAMING`、
//! `KIANA_MODEL_MAX_CONCURRENCY`、`KIANA_MODEL_QUEUE_LIMIT`。

use crate::credentials::{EnvSecretStore, InlineSecretStore, SecretStore};
use kiana_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, fmt, time::Duration};

/// 传给 `ProviderGateway::from_env` 的**原始**配置。
///
/// 【它是什么】 一个“可能有也可能没有”的四元组。每一项都是 `Option`，
/// 含义统一是“**没指定，去环境变量找，再找不到用内置默认**”。
///
/// 【谁构造它】 `kiana-daemon/src/model_client.rs::from_config`，
/// 来自 `LocalModelConfig` 与 `KIANA_PROVIDER` 等环境变量。
///
/// 【字段】
/// - `provider`：服务商名（`anthropic` / `openai` / `ollama` / `gemini` …）。
///   实际支持哪些见 `connection_with_credential_env` 里的那个 match。
/// - `model`：模型 id。`None` 时取服务商默认模型。
/// - `base_url`：自定义端点。`None` 时取服务商公网地址（Ollama 默认本地 11434）。
/// - `api_key`：**明文密钥**。这是本文件里唯一接触明文的地方，
///   它会立刻被转成摘要 + `InlineSecretStore`，之后不再以明文形式存在本结构体里。
///
/// 【⚠ 这个结构体自己实现了 `Debug`，把 `api_key` 打成 `[REDACTED]`】
/// 见下方 `impl fmt::Debug`。理由：默认的 derive 会把密钥原样打进日志，
/// 一次 `{:?}` 就泄漏。任何持有密钥的结构体都必须手写 `Debug`。

#[derive(Clone, Default)]
pub struct ProviderConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

/// 手写 `Debug`：把 `api_key` 替换成固定的 `[REDACTED]` 占位符。
///
/// 【⚠ 为什么这是安全性的一部分，而不是调试便利】
/// Rust 的 `#[derive(Debug)]` 会把**所有**字段原样格式化。
/// 只要有人写了 `tracing::info!(?config, "provider ready")`，密钥就进了日志文件，
/// 而日志文件通常会被收集、备份、同步到其它地方。
/// 手写 `Debug` 是把“泄漏”变成“结构上不可能”，比事后加脱敏中间件可靠得多。
///
/// 【注意 `.map(|_| "[REDACTED]")` 的写法】
/// 它保留了 `Option` 的形状（`Some("[REDACTED]")` vs `None`），
/// 这样调试时仍能看出“到底配没配密钥”，但看不到值。

impl fmt::Debug for ProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}
/// `KIANA_MODEL_PROFILES_JSON` 里单个 profile 的**配置文本**形态。
///
/// 【serde 注解 `deny_unknown_fields`】 多写一个字段就整体解析失败。
/// 代价是加字段要同步改这里，收益是**拼错的字段不会被静默忽略**——
/// 用户以为配了 `base_ur1`，其实没生效，这种错误极难排查。
///
/// 【字段】
/// - `provider` / `model`：必填。
/// - `base_url`：可选自定义端点。
/// - `api_key_env`：**环境变量名**，不是密钥。
/// - `capabilities`：手工声明的能力（见 `DeclaredCapabilities`）。
/// - `ollama_load_timeout_ms`：仅 Ollama 可用的加载超时。
/// - `inherit_default`：为 `true` 表示整体继承 `default` 连接。
///
/// 【⚠ `inherit_default` 与其它字段互斥】
/// 既写 `inherit_default: true` 又写 `provider`，在 `connections()` 里会报
/// `model_profile_inheritance_conflict`。语义自相矛盾，必须在配置阶段拒绝。

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProfileConfig {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub capabilities: Option<DeclaredCapabilities>,
    #[serde(default)]
    pub ollama_load_timeout_ms: Option<u64>,
    #[serde(default)]
    pub inherit_default: bool,
}
/// 手工声明的能力集。
///
/// 【为什么需要手工声明】
/// Kiana 不能靠“发个探测请求”去问上游“你支持不支持图片”——那要花钱、要时间、
/// 而且探测本身也是一次未授权的调用。所以能力要么来自内置目录，要么由运维显式声明。
///
/// 【字段】
/// - `tools` / `images` / `structured_output`：三个开关，映射成
///   `CapabilitySupport::Supported` / `Unsupported`（见 `support()`）。
/// - `context_window`：上下文窗口大小。0 或过大都非法。
/// - `max_output`：单次最大输出。必须 `0 < max_output < context_window`
///   ——输出窗口不可能大于总窗口。

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeclaredCapabilities {
    pub tools: bool,
    #[serde(default)]
    pub images: bool,
    #[serde(default)]
    pub structured_output: bool,
    pub context_window: u64,
    pub max_output: u64,
}
/// 一个 profile 对应的完整上游连接。**运行期只读。**
///
/// 【字段分组】
/// ```text
///   身份     route(ModelRoute), provider_account, endpoint, max_output
///   凭据     credential_ref, credential_revision, credential_store
///   传输     client(reqwest::Client), limits(TransportLimits)
///   闸门     capacity_policy, capacity_window, capacity, queue_slots, circuit
/// ```
///
/// 【关键字段的业务含义】
/// - `route`：**路由**，即“用哪个服务商 / 哪个协议 / 哪个模型 / 哪个 profile”。
///   它的 `configuration_revision` 是整条路由配置的摘要，准入时会拿它做漂移检测
///   （见 `lib.rs::complete_admitted` 里的 `model_route_changed_after_admission`）。
/// - `provider_account`：**账号的摘要 id**（`json_digest({provider, connection})`），
///   不是账号名。用于审计时区分“哪个连接发的”，且不泄漏身份信息。
/// - `credential_ref`：**不透明引用**（例如 `env` + 变量名）。永远不是密钥本身。
/// - `credential_revision`：**凭据内容的摘要**。作用是“准入之后凭据有没有被换过”。
///   `lib.rs` 会在发请求前重新向 `credential_store` 查一次当前摘要并比对。
/// - `client`：**策略已锁死**的 HTTP 客户端。关闭重定向、忽略代理、禁用 reqwest 自带重试
///   （重试必须由上层决策，否则预算账本会对不上）。详见 `connection_with_credential_env`。
/// - `limits`：分阶段超时与字节上限。
/// - `capacity` / `queue_slots`：两个信号量，分别限制“同时执行数”与“排队数”。
/// - `circuit`：熔断器。连续失败到阈值就停止放行，冷却后放一个探测请求试探恢复。
///
/// 【⚠ 别名共享】 `connections()` 会把 provider+endpoint+凭据三者都相同的多个 profile
/// 合并成同一份闸门（policy/window/semaphore/circuit）。改 profile 名字骗不过配额。

#[derive(Clone)]
pub(crate) struct Connection {
    pub route: ModelRoute,
    pub capabilities: ModelCapabilities,
    pub endpoint: reqwest::Url,
    pub provider_account: String,
    pub credential_ref: Option<SecretRef>,
    pub credential_revision: String,
    pub credential_store: std::sync::Arc<dyn SecretStore>,
    pub client: reqwest::Client,
    pub limits: TransportLimits,
    pub max_output: u64,
    /// Immutable server-owned RPM/TPM/concurrency identity. Profile aliases share this policy
    /// when provider/origin/credential scope is identical; a profile name cannot widen quota.
    pub capacity_policy: std::sync::Arc<ProviderCapacityPolicy>,
    pub capacity_window: std::sync::Arc<crate::capacity::CapacityWindow>,
    pub capacity: std::sync::Arc<tokio::sync::Semaphore>,
    pub queue_slots: std::sync::Arc<tokio::sync::Semaphore>,
    pub circuit: std::sync::Arc<std::sync::Mutex<ProviderCircuitBreaker>>,
}
/// 传输层的**分阶段**超时与字节上限。
///
/// 【⚠ 这里最容易误解的一点：五个超时是“同时生效的上界”，不是“各管一段”】
///
/// ```text
///   headers   发出请求 -> 收到响应头       （不含读 body）
///   first_event 流式：发出 -> 第一个**语义**事件（含大模型冷启动时间）
///   idle      每个相邻 chunk 之间允许的最大空档
///   total     整个 attempt 的总墙钟上限
/// ```
///
/// 真正决定一次尝试能活多久的是 `total`（`transport.rs::send` 用
/// `min(deadline - now, limits.total)` 包住整个内层调用）。
/// 另外三个的作用是**在总时限内给出更早、更有信息量的失败**：
/// 如果 `total` 是 180 秒而 `headers` 是 30 秒，那么“连响应头都收不到”会在 30 秒就
/// 以 `provider_headers_timeout` 失败，而不是干等 3 分钟才报一个笼统的总超时。
///
/// 【字节上限】
/// - `max_body`：整个响应体的累计字节上限（流式与非流式都用）。
/// - `max_frame`：单个 SSE/NDJSON 帧的字节上限，防止一条畸形帧撑爆内存。

#[derive(Clone)]
pub(crate) struct TransportLimits {
    pub headers: Duration,
    pub first_event: Duration,
    pub idle: Duration,
    pub total: Duration,
    pub max_body: usize,
    pub max_frame: usize,
}
/// 默认限额。**这些数字是工程判断，不是从任何规范推导出来的。**
///
/// 逐项说明取值理由：
/// ```text
///   headers:  30s   发出到响应头。模型服务端一般几百 ms 就回头；
///                    30s 已经很宽松，再大只会让失败来得更晚。
///   first_event: 60s 大模型冷启动（加载权重）可能几十秒，Ollama 首次拉模型更久。
///                    Ollama 场景会被 ollama_load_timeout 覆盖成更长的值。
///   idle:     45s   相邻 chunk 的空档。流式输出中模型"正在想"的时间。
///   total:   180s   整个 attempt 的总墙钟上限。3 分钟对一次模型调用是合理上界。
///   max_body: 8 MiB 足够容纳一次长文本回复；再大就该分页而不是一次性拉。
///   max_frame: 256 KiB 单帧上限。正常的增量只有几十到几 KB；
///                    256 KiB 足以容纳一个超长 tool_call 参数，同时挡住畸形帧。
/// ```
///
/// 【⚠ 这些值可被部分覆盖】 `KIANA_MODEL_MAX_CONCURRENCY` / `KIANA_MODEL_QUEUE_LIMIT`
/// 覆盖并发相关项；`ollama_load_timeout` 只覆盖 `first_event`。
/// 覆盖项同样有上界校验（见 `connection_with_credential_env`）。

impl Default for TransportLimits {
    fn default() -> Self {
        Self {
            headers: Duration::from_secs(30),
            first_event: Duration::from_secs(60),
            idle: Duration::from_secs(45),
            total: Duration::from_secs(180),
            max_body: 8 * 1024 * 1024,
            max_frame: 256 * 1024,
        }
    }
}
/// 读一个环境变量，**trim 后非空**才返回。
///
/// 【为什么 trim】 shell 里 `export FOO=` 会得到一个空串，用户以为“设了”，其实等于没设。
/// 统一按“没设”处理，比在后面每个使用点都判一次空串更可靠。
///
/// 【⚠ 它只读不写，且调用点在 `connections()` 里】 也就是说环境变量只在**启动解析**时读一次，
/// 之后配置被冻结。运行期改环境变量不会影响已建立的连接。

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}
/// 把 `bool` 翻译成三态的 `CapabilitySupport`。
///
/// 【⚠ 为什么需要三态而不是 bool】
/// `true` 有两种完全不同的含义：
/// - “我们**确认**支持”（有内置目录、或运维显式声明）
/// - “我们**猜**支持”（模型名在白名单里，但没实测）
///
/// 这两者的决策权重不同。`request.rs` 里有这样的判断：
/// 只有 `CapabilitySupport::Supported` 才允许带工具清单的请求；
/// `Unknown` 会被拒绝。把二者压成一个 bool，就等于把“猜”当成“确认”。

fn support(value: bool) -> CapabilitySupport {
    if value {
        CapabilitySupport::Supported
    } else {
        CapabilitySupport::Unsupported
    }
}

/// 解析 Ollama 专属的“模型加载超时”。
///
/// 【背景】 Ollama 首次调用某个模型时，如果权重不在本地，需要先下载/加载，
/// 这段时间服务端不返回任何字节。普通 HTTP 客户端会在 `headers` 超时处就报错，
/// 所以 Ollama 需要一个**更长的首事件超时**。
///
/// 【三条规则，逐条有理由】
/// ```text
///   ("ollama", Some(ms)) 且 1_000 <= ms <= 150_000  -> 接受
///   ("ollama", None)                                -> 不覆盖，用默认值
///   ("ollama", Some(_)) 超出范围                     -> ollama_load_timeout_invalid
///   (非 ollama, Some(_))                             -> ollama_load_timeout_provider_mismatch
///   (非 ollama, None)                                -> 不适用，放行
/// ```
///
/// 【⚠ 为什么非 Ollama 传了这个参数要报错，而不是静默忽略】
/// 静默忽略会让用户以为“超时已经调大了”，实际完全没生效——这正是
/// Kiana 反对的“把状态写成比证据更强”。给别的 provider 配 Ollama 专属参数，
/// 几乎一定是配错了 profile。
///
/// 【为什么下界是 1000ms】 小于 1 秒的加载超时在任何机器上都没有意义。
/// 【为什么上界是 150 秒】 超过 150 秒的加载，用户早就该去看日志了；
/// 而且无上限的超时会让失败请求永远占着并发槽位。

fn parse_ollama_load_timeout(
    provider: &str,
    timeout_ms: Option<u64>,
) -> Result<Option<Duration>, ModelError> {
    match (provider, timeout_ms) {
        ("ollama", Some(timeout_ms)) if (1_000..=150_000).contains(&timeout_ms) => {
            Ok(Some(Duration::from_millis(timeout_ms)))
        }
        ("ollama", None) => Ok(None),
        ("ollama", Some(_)) => Err(ModelError::invalid("ollama_load_timeout_invalid")),
        (_, Some(_)) => Err(ModelError::invalid("ollama_load_timeout_provider_mismatch")),
        (_, None) => Ok(None),
    }
}

/// 把「环境变量 + 显式配置 + `KIANA_MODEL_PROFILES_JSON`」解析成一组 `Connection`。
///
/// 【作用】 provider 启动时唯一的主入口，产出 `BTreeMap<profile 名, Connection>`。
///
/// 【调用者】 `resolver.rs::ConfigResolver::resolve`。
///
/// 【输入】 `config`：daemon 组装的原始配置（各项可为 `None`）。
///
/// 【输出】 `(BTreeMap<String, Connection>, bool)`。
///   第二个值 `explicit_profiles` 表示“是否来自 `KIANA_MODEL_PROFILES_JSON`”。
///   **它会改变未知 profile 的行为**（见 `ProviderGateway::connection`）：
///   - 显式配置存在时，未知 profile 直接报 `model_profile_unconfigured`；
///   - 没有显式配置时，未知 profile 静默回落到 `default`。
///   没有这个布尔值的话，用户把 profile 名拼错后可能悄悄用上了默认服务商。
///
/// 【副作用】
/// - **读取**环境变量（`env()`）；从不写入。
/// - 构造多个 `reqwest::Client`（只建连接池，不建立实际连接）。
/// - 解析并校验可能来自 `KIANA_MODEL_PROFILES_JSON` 的 JSON 文本。
///
/// 【失败情况】（全部是启动期失败）
/// | 错误码 | 含义 |
/// |---|---|
/// | `model_profile_config_too_large` | profile JSON 超过 64 KiB |
/// | `model_profile_config_invalid` | JSON 解析失败或含未知字段 |
/// | `model_profile_unknown` | profile 名不对应任何真实角色 |
/// | `model_profile_inheritance_conflict` | 同时写了 `inherit_default` 和具体字段 |
/// | `model_default_route_unavailable` | 要求继承 default，但 default 建不出来 |
/// | `model_profile_credential_reference_required` | 非 Ollama profile 没给 `api_key_env` |
/// | `model_profile_credential_reference_invalid` | 环境变量名不是 `[A-Z0-9_]` |
/// | `model_credential_unavailable` | 环境变量没设或为空 |
/// | `model_profiles_empty` | 一个连接都没建成 |

pub(crate) fn connections(
    config: ProviderConfig,
) -> Result<(BTreeMap<String, Connection>, bool), ModelError> {
    connections_with_workspace(config, None)
}

pub(crate) fn connections_with_workspace(
    config: ProviderConfig,
    workspace: Option<&crate::resolver::WorkspaceConfig>,
) -> Result<(BTreeMap<String, Connection>, bool), ModelError> {
    let configured = env("KIANA_MODEL_PROFILES_JSON");
    let default = connection_with_credential_env("default", config, None, None, None, workspace);
    if workspace.is_some_and(|workspace| {
        workspace.provider.is_some()
            || workspace.model.is_some()
            || workspace.base_url.is_some()
            || workspace.api_key_env.is_some()
    }) && default.is_err()
    {
        // An explicitly declared workspace default is part of the candidate. It must not be
        // silently dropped merely because another profile happens to be usable.
        return default
            .map(|connection| (BTreeMap::from([("default".to_owned(), connection)]), false));
    }
    let mut values = workspace
        .map(|workspace| {
            workspace
                .profiles
                .iter()
                .map(|(name, profile)| {
                    (
                        name.clone(),
                        ProfileConfig {
                            provider: profile.provider.clone(),
                            model: profile.model.clone(),
                            base_url: profile.base_url.clone(),
                            api_key_env: profile.api_key_env.clone(),
                            capabilities: None,
                            ollama_load_timeout_ms: None,
                            inherit_default: profile.inherit_default,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let explicit_profiles = configured.is_some() || !values.is_empty();
    if !explicit_profiles {
        return default
            .map(|connection| (BTreeMap::from([("default".to_owned(), connection)]), false));
    }
    let mut result = BTreeMap::new();
    if let Ok(connection) = default {
        result.insert("default".to_owned(), connection);
    }
    // `KIANA_MODEL_PROFILES_JSON` 是运维写的一整段 JSON，形如
    //   {"planner": {...}, "builder": {...}}
    // 64 KiB 上限防止一个超大配置把启动内存吃光。
    //
    // 解析成 BTreeMap 而不是 Vec：profile 名天然是键，且 BTreeMap 的迭代顺序确定，
    // 让“连接建立的顺序”是可复现的（便于测试与对账）。

    if let Some(raw) = configured {
        if raw.len() > 64 * 1024 {
            return Err(ModelError::invalid("model_profile_config_too_large"));
        }
        let environment: BTreeMap<String, ProfileConfig> = serde_json::from_str(&raw)
            .map_err(|_| ModelError::invalid("model_profile_config_invalid"))?;
        // Explicit environment profiles override a project profile with the same name.
        values.extend(environment);
    }
    if values.len() > 64 {
        return Err(ModelError::invalid("model_profile_limit_exceeded"));
    }
    for (profile, value) in values {
        if !RoleSpec::catalog()
            .iter()
            .any(|role| role.model_profile == profile)
        {
            return Err(ModelError::invalid("model_profile_unknown"));
        }
        let mut item = if value.inherit_default {
            if !value.provider.is_empty()
                || !value.model.is_empty()
                || value.base_url.is_some()
                || value.api_key_env.is_some()
                || value.capabilities.is_some()
                || value.ollama_load_timeout_ms.is_some()
            {
                return Err(ModelError::invalid("model_profile_inheritance_conflict"));
            }
            result
                .get("default")
                .ok_or_else(|| ModelError::invalid("model_default_route_unavailable"))?
                .clone()
        } else {
            let ollama_load_timeout =
                parse_ollama_load_timeout(&value.provider, value.ollama_load_timeout_ms)?;
            let key_env = if let Some(name) = value.api_key_env {
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                {
                    return Err(ModelError::invalid("model_credential_reference_invalid"));
                }
                Some(name)
            } else if value.provider == "ollama" {
                None
            } else {
                return Err(ModelError::invalid(
                    "model_profile_credential_reference_required",
                ));
            };
            connection_with_credential_env(
                &profile,
                ProviderConfig {
                    provider: Some(value.provider),
                    model: Some(value.model),
                    base_url: value.base_url,
                    api_key: None,
                },
                value.capabilities,
                key_env,
                ollama_load_timeout,
                None,
            )?
        };
        item.route.profile = profile.clone();
        result.insert(profile, item);
    }
    // 一个连接都没建成 => 启动失败。
    // 继续跑下去的话，第一次真正调模型时才会暴露问题，而那时用户可能已经等了很久。
    // 配置错误应该在启动时就炸。

    if result.is_empty() {
        return Err(ModelError::invalid("model_profiles_empty"));
    }
    // Connection aliases that point at the same credential and origin share one
    // semaphore. A profile name is a routing label, not a way to bypass the
    // provider's connection-level capacity limit.
    // ========== 别名去重：把闸门按“真实上游身份”归并 ==========
    //
    // 场景：运维配了 3 个 profile，全都是同一个 OpenAI endpoint + 同一把 key，
    // 只是模型名不同（planner 用 gpt-4o、builder 用 gpt-4o-mini）。
    // 如果每个 profile 各有一套信号量和配额窗口，那么实际上并发能力被放大了 3 倍——
    // 用户以为限了 2 并发，实际能跑 6。
    //
    // 解法：算一个“上游身份 scope”摘要，把 scope 相同的 profile 指向**同一份**闸门。

    let mut capacities: BTreeMap<
        String,
        (
            std::sync::Arc<ProviderCapacityPolicy>,
            std::sync::Arc<crate::capacity::CapacityWindow>,
            std::sync::Arc<tokio::sync::Semaphore>,
            std::sync::Arc<tokio::sync::Semaphore>,
            std::sync::Arc<std::sync::Mutex<ProviderCircuitBreaker>>,
        ),
    > = BTreeMap::new();
    for connection in result.values_mut() {
        // scope = (provider, endpoint 明文, 凭据引用摘要) 三元组的摘要。
        //
        // 【⚠ 三个字段缺一不可】
        //   少了 provider    : 不同服务商的配额会被错误合并
        //   少了 endpoint    : 同一服务商的不同部署（国内/海外节点）会被错误合并
        //   少了 credential  : 不同账号的配额会被错误合并 —— 这是最危险的一种，
        //                      因为不同账号在服务端是不同的限流桶
        //
        // 【为什么 model 不在 scope 里】
        // 配额是按账号+部署算的，与本次用哪个模型无关。
        // 把 model 算进去会导致“换模型 = 换配额桶”，那等于给了用户一个绕过限流的办法。

        let scope = capacity_scope(connection);
        // `or_insert_with` 只在这个 scope 第一次出现时创建闸门，之后全部复用。
        // 于是三个 profile 共享同一个 Semaphore / CapacityWindow / CircuitBreaker。
        //
        // ⚠ 注意 `circuit`（熔断器）也在归并范围内：这是对的。
        //    上游挂了就是挂了，换个 profile 名并不能让同一个 endpoint 变健康。

        let (capacity_policy, capacity_window, capacity, queue_slots, circuit) = capacities
            .entry(scope)
            .or_insert_with(|| {
                (
                    connection.capacity_policy.clone(),
                    connection.capacity_window.clone(),
                    connection.capacity.clone(),
                    connection.queue_slots.clone(),
                    connection.circuit.clone(),
                )
            })
            .clone();
        connection.capacity = capacity;
        connection.capacity_policy = capacity_policy;
        connection.capacity_window = capacity_window;
        connection.queue_slots = queue_slots;
        connection.circuit = circuit;
    }
    Ok((result, explicit_profiles))
}

pub(crate) fn capacity_scope(connection: &Connection) -> String {
    json_digest(&json!({
        "provider": connection.route.provider_id,
        "origin": connection.endpoint.as_str(),
        "credential": connection.credential_ref.as_ref().map(|reference| &reference.reference_digest),
    }))
}

#[cfg(test)]
mod ollama_timeout_tests {
    use super::*;

    #[test]
    fn ollama_load_timeout_is_bounded_by_transport_total() {
        assert_eq!(
            parse_ollama_load_timeout("ollama", Some(1_000)).unwrap(),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            parse_ollama_load_timeout("ollama", Some(150_000)).unwrap(),
            Some(Duration::from_secs(150))
        );
        assert_eq!(
            parse_ollama_load_timeout("ollama", Some(999))
                .unwrap_err()
                .code,
            "ollama_load_timeout_invalid"
        );
        assert_eq!(
            parse_ollama_load_timeout("ollama", Some(150_001))
                .unwrap_err()
                .code,
            "ollama_load_timeout_invalid"
        );
        assert_eq!(
            parse_ollama_load_timeout("openai", Some(1_000))
                .unwrap_err()
                .code,
            "ollama_load_timeout_provider_mismatch"
        );
    }
}
/// 构造一个连接的**唯一真正实现**。`connections()` 的所有路径最终都汇到这里。
///
/// 【作用】 把“一个 profile 的全部配置”变成一个可运行的 `Connection`：
/// 协议选择 → 端点拼装与安全校验 → 凭据接入 → 能力判定 → 客户端与闸门构造。
///
/// 【输入】
/// - `name`：profile 名（`"default"` 或某个具名 profile）。
/// - `config`：provider / model / base_url / api_key（都可为 `None`）。
/// - `declared`：运维显式声明的能力；`None` 表示走内置目录。
/// - `credential_env_override`：显式指定用哪个环境变量取密钥（profile 场景）。
/// - `ollama_load_timeout`：Ollama 专属的首事件超时覆盖。
///
/// 【输出】 校验完成的 `Connection`。
///
/// 【副作用】 读环境变量；创建一个策略锁死的 `reqwest::Client`（惰性，不建连）。
///   **不发起任何网络请求。**
///
/// 【失败情况】 见下方各阶段注释；全部是启动期 `ModelError`。

fn connection_with_credential_env(
    name: &str,
    config: ProviderConfig,
    declared: Option<DeclaredCapabilities>,
    credential_env_override: Option<String>,
    ollama_load_timeout: Option<Duration>,
    workspace: Option<&crate::resolver::WorkspaceConfig>,
) -> Result<Connection, ModelError> {
    let provider = config
        .provider
        .or_else(|| env("KIANA_PROVIDER"))
        .or_else(|| workspace.and_then(|workspace| workspace.provider.clone()))
        .unwrap_or_else(|| "anthropic".to_owned());
    // ========== 阶段 1：provider 名 -> 协议 + 默认值表 ==========
    //
    // 这张表是**穷举 match**：认识 5 类 provider（以及 openai 的几种拼写别名），
    // 其它一律 `model_provider_unsupported`。
    //
    // 每个 provider 给出 7 个值：
    //   protocol       线上协议方言（决定 request.rs 怎么组包、response.rs 怎么解包）
    //   default_model  该 provider 的默认模型
    //   default_base   该 provider 的默认根地址
    //   key_env        密钥默认从哪个环境变量读（ollama 为空串 = 不需要密钥）
    //   model_env      模型名可被哪个环境变量覆盖
    //   base_env       端点可被哪个环境变量覆盖
    //   path           要拼在根地址后面的 API 路径
    //
    // 【⚠ 别名的存在是为了兼容写法，不是为了扩展能力】
    // `openai` / `openai-compatible` / `openai_compatible` 三者等价。
    // 新增别名很容易，但每加一个就多一份文档负担，所以这里只保留实际出现过的写法。

    let (protocol, default_model, default_base, key_env, model_env, base_env, path) =
        match provider.as_str() {
            "anthropic" => (
                ModelProtocol::AnthropicMessages,
                "claude-sonnet-4-6",
                "https://api.anthropic.com",
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_MODEL",
                "ANTHROPIC_BASE_URL",
                "v1/messages",
            ),
            "openai" | "openai-compatible" | "openai_compatible" => (
                ModelProtocol::OpenAiChat,
                "gpt-4o-mini",
                "https://api.openai.com/v1",
                "OPENAI_API_KEY",
                "OPENAI_MODEL",
                "OPENAI_BASE_URL",
                "chat/completions",
            ),
            "openai-responses" | "openai_responses" => (
                ModelProtocol::OpenAiResponses,
                "gpt-4.1",
                "https://api.openai.com/v1",
                "OPENAI_API_KEY",
                "OPENAI_MODEL",
                "OPENAI_BASE_URL",
                "responses",
            ),
            "ollama" => (
                ModelProtocol::OllamaChat,
                "qwen2.5-coder:7b",
                "http://localhost:11434",
                "",
                "KIANA_OLLAMA_MODEL",
                "KIANA_OLLAMA_BASE_URL",
                "api/chat",
            ),
            "gemini" | "gemini-interactions" => (
                ModelProtocol::GeminiInteractions,
                "gemini-2.5-flash",
                "https://generativelanguage.googleapis.com/v1beta",
                "GEMINI_API_KEY",
                "GEMINI_MODEL",
                "GEMINI_BASE_URL",
                "interactions",
            ),
            _ => return Err(ModelError::invalid("model_provider_unsupported")),
        };
    let model = config
        .model
        .or_else(|| env(model_env))
        .or_else(|| workspace.and_then(|workspace| workspace.model.clone()))
        .unwrap_or_else(|| default_model.to_owned());
    if model.trim().is_empty() || model.len() > 256 {
        return Err(ModelError::invalid("model_id_invalid"));
    }
    let base = config
        .base_url
        .or_else(|| env(base_env))
        .or_else(|| workspace.and_then(|workspace| workspace.base_url.clone()))
        .unwrap_or_else(|| default_base.to_owned());
    // ========== 阶段 2：端点拼装与安全校验 ==========
    //
    // 先 `trim_end_matches('/')` 再拼 path，避免出现 `https://api.x.com//v1/messages`
    // 这种双斜杠（部分服务端会 404，部分会正常，属于运气问题，不可取）。
    //
    // 接下来的四道检查，任何一道不过就拒绝启动。逐条理由写在 resolver.rs 的
    // `validate_endpoint` 注释里（同一套规则的另一处实现），这里只强调：
    // **这是密钥离开进程前最后一次检查目标地址的机会。**

    let mut endpoint = reqwest::Url::parse(&format!("{}/{}", base.trim_end_matches('/'), path))
        .map_err(|_| ModelError::invalid("model_endpoint_invalid"))?;
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
        || endpoint.query().is_some()
    {
        return Err(ModelError::invalid(
            "model_endpoint_credentials_or_query_denied",
        ));
    }
    // loopback 判定：字面量 `"localhost"` 或任何能解析成回环地址的 IP（127.0.0.0/8、::1）。
    //
    // 只有 loopback 才允许明文 HTTP——因为回环流量根本不上网卡，
    // 同一台机器上的其它进程理论上能看，但那是本机信任边界内的事。
    // 任何**非**回环的 http:// 都会被下一行拒绝。

    let local = endpoint.host_str().is_some_and(|h| {
        h == "localhost"
            || h.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && local) {
        return Err(ModelError::invalid(
            "model_endpoint_requires_tls_or_loopback",
        ));
    }
    // 把 `localhost` 钉成字面量 `127.0.0.1`。
    //
    // 【为什么】 `localhost` 可能被 /etc/hosts 解析到 `::1`，而很多服务端只监听 IPv4。
    // 更糟的是 DNS rebinding：一个恶意 hosts 条目能把 localhost 指向公网地址，
    // 于是明文 HTTP + 密钥就发到了公网。钉成字面量就切断了这条路径。
    //
    // 配合下面 client 的三条设置，这条链路才完整：
    //   redirect: none  -> 对方不能把我们的请求 302 到别处
    //   no_proxy        -> 不继承 HTTP_PROXY 等环境变量（否则密钥会经过第三方代理）
    //   retry: never    -> reqwest 自带重试被禁用（重试决策必须由上层做，见下）

    // Pin localhost to a literal address; redirects are disabled and ambient proxies ignored.
    if endpoint.host_str() == Some("localhost") {
        endpoint
            .set_host(Some("127.0.0.1"))
            .map_err(|_| ModelError::invalid("model_endpoint_invalid"))?;
    }
    // ========== 阶段 3：凭据接入（三选一） ==========
    //
    //   A. config.api_key 有值  -> InlineSecretStore（明文来自显式配置）
    //   B. 否则有环境变量名      -> EnvSecretStore（真正的常规路径）
    //   C. 否则                  -> 只有 Ollama 允许（本地服务不需要密钥）
    //
    // 【三条路径都只产出 SecretRef + 摘要 + store 句柄，从不把明文放进 Connection】

    let configured_env = credential_env_override
        .or_else(|| (!key_env.is_empty() && env(key_env).is_some()).then(|| key_env.to_owned()))
        .or_else(|| workspace.and_then(|workspace| workspace.api_key_env.clone()))
        .or_else(|| (!key_env.is_empty()).then(|| key_env.to_owned()));
    let (credential_ref, credential_store, credential_revision) = if let Some(value) =
        config.api_key
    {
        let revision = json_digest(&json!(&value));
        let reference = SecretRef::new(
            "inline",
            format!("config:{name}"),
            "provider.request",
            provider.clone(),
            1,
        )
        .map_err(|_| ModelError::invalid("credential_secret_ref_invalid"))?;
        let store = InlineSecretStore::new(value)?;
        (
            Some(reference),
            std::sync::Arc::new(store) as std::sync::Arc<dyn SecretStore>,
            revision,
        )
    } else if let Some(env_name) = configured_env {
        let value =
            env(&env_name).ok_or_else(|| ModelError::invalid("model_credential_unavailable"))?;
        if reqwest::header::HeaderValue::from_str(&value).is_err() {
            return Err(ModelError::invalid("model_credential_header_invalid"));
        }
        let revision = json_digest(&json!(&value));
        let reference = SecretRef::new("env", env_name, "provider.request", provider.clone(), 1)
            .map_err(|_| ModelError::invalid("credential_secret_ref_invalid"))?;
        (
            Some(reference),
            std::sync::Arc::new(EnvSecretStore) as std::sync::Arc<dyn SecretStore>,
            revision,
        )
    } else {
        if protocol != ModelProtocol::OllamaChat {
            return Err(ModelError::invalid("model_credential_unavailable"));
        }
        (
            None,
            std::sync::Arc::new(EnvSecretStore) as std::sync::Arc<dyn SecretStore>,
            "none".to_owned(),
        )
    };
    // 流式开关。`KIANA_STREAMING` 支持两套写法（`on/1/true/yes` 与 `off/0/false/no`），
    // 非法值直接报错而不是当默认值——配置写错要立刻知道。
    //
    // ⚠ 注意下一行：`streaming = protocol == OllamaChat || streaming_override`。
    //    也就是说 **Ollama 永远走流式，用户关不掉**。原因是 Ollama 的非流式端点
    //    在长回复上体验很差，且当前实现只验证过流式路径。

    let streaming_override = match env("KIANA_STREAMING").as_deref() {
        None | Some("auto" | "on" | "1" | "true" | "yes") => true,
        Some("off" | "0" | "false" | "no") => false,
        _ => return Err(ModelError::invalid("model_streaming_policy_invalid")),
    };
    let streaming = protocol == ModelProtocol::OllamaChat || streaming_override;
    // 内置能力目录：判断“当前模型名是否在已知白名单里”。
    //
    // 【⚠ 这是一个会过时的设计，读者必须知道】
    // 白名单写死在代码里。厂商发布新模型名后，这里不会自动更新，
    // 结果是新模型被判为 `CapabilitySupport::Unknown`，
    // 于是**带工具清单的请求会被拒绝**（`model_tool_capability_unknown_or_unsupported`）。
    //
    // 这不是 bug 而是刻意的 fail-closed：宁可让用户显式声明能力，
    // 也不要因为“猜对了”而放行一个我们没验证过的模型。
    // 运维可以用 `capabilities` 字段显式声明来绕过这个限制。

    let known = match protocol {
        ModelProtocol::AnthropicMessages => matches!(
            model.as_str(),
            "claude-sonnet-4-6" | "claude-opus-4-1" | "claude-haiku-4-5"
        ),
        ModelProtocol::OpenAiChat | ModelProtocol::OpenAiResponses => matches!(
            model.as_str(),
            "gpt-4o-mini" | "gpt-4o" | "gpt-4.1" | "gpt-4.1-mini" | "deepseek-chat"
        ),
        ModelProtocol::OllamaChat => false,
        ModelProtocol::GeminiInteractions => {
            matches!(model.as_str(), "gemini-2.5-flash" | "gemini-2.5-pro")
        }
        _ => false,
    };
    // 能力与窗口的最终取值。两个分支：
    //
    //   有显式声明 -> 完全采用声明值，source = "operator_config.v1"
    //   无声明     -> 走内置目录：
    //                 模型在白名单里 -> Supported，否则 Unknown
    //                 context_window 按协议给（Anthropic 200_000，其余 128_000）
    //                 max_output      固定 4096
    //                 source          = "builtin_catalog.2026-09-12"（带日期，便于判断是否过期）
    //
    // 【⚠ `source` 带日期这一点很重要】
    // 它让运维一眼能看出“这份能力信息有多旧”。哪天厂商改了模型，
    // 快照里的日期就会提醒人去核对。

    let (tools, images, structured, context_window, max_output, source) =
        if let Some(cap) = &declared {
            // 显式声明的能力也要校验，否则用户可以声明
            // `context_window = 0` 或 `max_output >= context_window` 制造 nonsense 配置。
            // 16_000_000 的上限防止有人声明一个荒谬的窗口把预算系统撑爆。

            if cap.context_window == 0
                || cap.max_output == 0
                || cap.max_output >= cap.context_window
                || cap.context_window > 16_000_000
            {
                return Err(ModelError::invalid("model_capabilities_invalid"));
            }
            (
                support(cap.tools),
                support(cap.images),
                support(cap.structured_output),
                cap.context_window,
                cap.max_output,
                "operator_config.v1",
            )
        } else {
            (
                if known {
                    CapabilitySupport::Supported
                } else {
                    CapabilitySupport::Unknown
                },
                CapabilitySupport::Unsupported,
                if known {
                    CapabilitySupport::Supported
                } else {
                    CapabilitySupport::Unknown
                },
                if protocol == ModelProtocol::AnthropicMessages {
                    200_000
                } else {
                    128_000
                },
                4096,
                "builtin_catalog.2026-09-12",
            )
        };
    let revision = json_digest(
        &json!({"provider":provider,"protocol":protocol,"model":model,"origin":endpoint.as_str(),
        "credential_revision":credential_revision,"declared":declared,"streaming":streaming,
        "ollama_load_timeout_ms":ollama_load_timeout.map(|value| value.as_millis())}),
    );
    let provider_account = json_digest(&json!({
        "provider": provider,
        "connection": name,
    }));
    let route = ModelRoute {
        provider_id: provider.clone(),
        protocol,
        connection_id: name.to_owned(),
        model_id: model.clone(),
        profile: name.to_owned(),
        configuration_revision: revision.clone(),
        streaming,
    };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15))
        .pool_idle_timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| ModelError::invalid("model_http_client_unavailable"))?;
    let capabilities = ModelCapabilities {
        tools,
        streaming: CapabilitySupport::Supported,
        structured_output: structured,
        images,
        reasoning_replay: CapabilitySupport::Unsupported,
        context_window,
        max_output,
        source: source.to_owned(),
        revision: revision.clone(),
    };
    // 并发闸门。默认 2，上限 128。
    //
    // 【为什么默认这么低】 2 是个保守值：既能让常见的“规划 + 执行”并行跑起来，
    // 又不会在服务商侧触发限流。默认调高很容易，调低很难受，所以默认保守。
    //
    // 上限 128 防止有人设成 10000 把自己的连接池和上游都压垮。

    let max_concurrency = env("KIANA_MODEL_MAX_CONCURRENCY")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| ModelError::invalid("model_concurrency_invalid"))
        })
        .transpose()?
        .unwrap_or(2);
    if max_concurrency == 0 || max_concurrency > 128 {
        return Err(ModelError::invalid("model_concurrency_invalid"));
    }
    // 排队上限。默认 128，上限 1024。
    //
    // 【为什么要有排队上限，而不是无限等】
    // 无限排队意味着：早到的请求会在信号量里排到 deadline 过期，
    // 白等一场然后失败。设上限让超出的请求**立刻**拿到
    // `provider_capacity_queue_full`，用户马上知道“系统满了”，而不是等 3 分钟。

    let queue_limit = env("KIANA_MODEL_QUEUE_LIMIT")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| ModelError::invalid("model_queue_limit_invalid"))
        })
        .transpose()?
        .unwrap_or(128);
    if queue_limit == 0 || queue_limit > 1_024 {
        return Err(ModelError::invalid("model_queue_limit_invalid"));
    }
    let quota_group = QuotaGroupKey::new(
        provider.clone(),
        credential_revision.clone(),
        Some(model.clone()),
        Some(name.to_owned()),
    )
    .map_err(ModelError::invalid)?;
    // 构造容量策略。注意最后两个参数是**写死的**魔数：
    //
    //   600         每分钟请求数上限（RPM）
    //   1_000_000   每分钟 token 数上限（TPM）
    //
    // 【⚠ 这两个数字在仓库里没有可发现的推导依据】 它们是工程默认值，
    // 不是从任何服务商文档抄来的。不同服务商的真实限额差异极大
    //（免费档可能只有 20 RPM）。如果用户撞到 429，那多半是这两个值比他的账号档位高。

    let capacity_policy = ProviderCapacityPolicy::new(
        quota_group,
        max_concurrency as u32,
        queue_limit as u32,
        600,
        1_000_000,
        30_000,
        revision.clone(),
    )
    .map_err(ModelError::invalid)?;
    // 熔断器：阈值 3 次连续失败，冷却 30_000 ms（30 秒）。
    //
    // 【它解决什么问题】
    // 上游挂掉时，如果没有熔断器，每个新请求都要等满 `headers` 超时（30 秒）才失败。
    // 3 次之后本文件就会直接把后续请求全部拒掉（`provider_circuit_open`），
    // 直到冷却期结束放一个探测请求过去看看恢复没有。
    //
    // 【为什么只有部分错误会触发熔断】 见 transport.rs 的 `trips_circuit`：
    // 只有「请求已发出 + 传输层错误 + 具体几种 code」才算。
    // 例如 401（key 无效）不触发熔断——那不是上游挂了，重试一万次也没用。

    let circuit =
        ProviderCircuitBreaker::new(revision.clone(), 3, 30_000).map_err(ModelError::invalid)?;
    let mut limits = TransportLimits::default();
    if protocol == ModelProtocol::OllamaChat {
        if let Some(timeout) = ollama_load_timeout {
            limits.first_event = timeout;
        }
    }
    Ok(Connection {
        route,
        capabilities,
        endpoint,
        provider_account,
        credential_ref,
        credential_revision,
        credential_store,
        client,
        limits,
        max_output,
        capacity_policy: std::sync::Arc::new(capacity_policy),
        capacity_window: std::sync::Arc::new(crate::capacity::CapacityWindow::default()),
        capacity: std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrency)),
        queue_slots: std::sync::Arc::new(tokio::sync::Semaphore::new(queue_limit)),
        circuit: std::sync::Arc::new(std::sync::Mutex::new(circuit)),
    })
}

/// 把一组 `Connection` 汇总成**无密钥**的对外配置快照。
///
/// 【作用】 给 `ProviderGateway::configuration_snapshot()` / `catalog()` 提供展示用数据。
///
/// 【⚠ 这个快照绝不包含密钥】
/// 每个 profile 只带 `credential_ref.reference_digest`（摘要），
/// 不带 `credential_ref.key` 指向的环境变量名，更不带密钥值。
///
/// 【⚠ `ProviderSelectionMode::Live` 是写死的】
/// 注意最后一行固定传 `Live`。这不代表“已经验证过 live provider”——
/// 当前版本的证明上限仍是 `local_behavior`（见 README / USER.md）。
/// 这个字段描述的是“这次快照的来源是运行时配置解析”，不是“已经跑通过线上模型”。

pub(crate) fn snapshot(
    connections: &BTreeMap<String, Connection>,
) -> Result<ProviderConfigSnapshot, ModelError> {
    let profiles = connections
        .values()
        .map(|connection| {
            ProviderProfileSnapshot::new(
                connection.route.clone(),
                connection.capabilities.clone(),
                connection
                    .credential_ref
                    .as_ref()
                    .map(|reference| reference.reference_digest.clone()),
                if connection.route.profile == "default" {
                    ProviderConfigSource::BuiltinDefault
                } else {
                    ProviderConfigSource::Profile
                },
            )
            .map_err(ModelError::invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    ProviderConfigSnapshot::new(ProviderSelectionMode::Live, profiles).map_err(ModelError::invalid)
}
