//! Single provider configuration resolver.
//!
//! This module owns the product configuration boundary. It resolves explicit/env profiles for
//! `ProviderGateway` and parses optional workspace overlays into strict, secret-free snapshots.
//! Workspace text is never accepted before the caller proves ProjectTrust; the resolver does not
//! perform network I/O or return raw credentials to callers.
//!
//! ## 这个文件在系统里的位置
//!
//! ```text
//!   kiana-daemon/src/model_client.rs::from_config()
//!        |  组装 ProviderConfig { provider, model, base_url, api_key }
//!        v
//!   kiana-provider/src/lib.rs::ProviderGateway::from_env()
//!        |
//!        v
//!   【本文件 ConfigResolver::resolve()】 <- 唯一生产配置解析入口
//!        |    |- config::connections()  显式/环境变量 profile -> Connection
//!        |    `- config::snapshot()     Connection -> ProviderConfigSnapshot（无密钥）
//!        v
//!   Resolution { connections, explicit_profiles, snapshot }
//! ```
//!
//! 本文件还有第二条入口：**工作区覆盖配置（workspace overlay）**。
//!
//! ```text
//!   某项目目录里的 .kiana 配置文本
//!        |  注意：必须先由调用方证明 ProjectTrust，本文件不自己判断信任
//!        v
//!   【本文件 ConfigResolver::parse_workspace()】 -> WorkspaceConfig（结构化、已校验）
//!        v
//!   【本文件 ConfigResolver::workspace_snapshot()】 -> ConfigSnapshot（带摘要，用于版本围栏）
//! ```
//!
//! ## 两个入口的信任模型差异（关键）
//!
//! | | `resolve()` | `parse_workspace()` |
//! |---|---|---|
//! | 配置来源 | 进程环境变量 / 显式参数 | **项目目录里的文件** |
//! | 信任级别 | 与用户本人同权 | 项目本地资源，**必须先过 ProjectTrust** |
//! | 是否查网 | 否 | 否 |
//!
//! 项目本地文件是**不可信输入**：一个克隆下来的仓库完全可能自带一份把 base_url
//! 指向攻击者服务器的配置。所以 `parse_workspace` 的第一个判断就是
//! `if !project_trusted { 拒绝 }`，发生在解析任何内容之前。
//! `workspace_snapshot` 的 `project_trust_revision` 也必须由权威边界传入——
//! **本文件绝不会从配置文本里推断信任**。

use crate::config::{self, Connection};
use crate::ProviderConfig;
use kiana_domain::{json_digest, ConfigSnapshot, ProviderConfigSnapshot, RoleSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// 工作区覆盖配置的 schema 标识。
///
/// 【作用】 解析时必须逐字匹配，否则一律拒绝。
/// 这是防止“拿旧版配置喂新版解析器”的第一道闸门。

pub const WORKSPACE_CONFIG_SCHEMA: &str = "kiana.provider-config.v1";
/// 工作区配置的结构版本号。当前只有 1。
///
/// 【设计意图】 将来结构变了就 +1；老版本走**拒绝**，而不是猜测迁移。
/// 配置错了要用户改，猜错了会让用户拿到一个他自己没配过的行为。

pub const WORKSPACE_CONFIG_VERSION: u64 = 1;
/// 工作区配置文本的字节上限：64 KiB。
///
/// 【为什么需要上限】 配置是项目里的一个文件，理论上可以任意大。
/// 不限大小等于允许一个恶意仓库塞进 1 GB 的 JSON 把进程内存吃光。
/// 64 KiB 远大于任何真实配置的合理体积。
///
/// 【⚠ 检查的是字节数不是字符数】 `raw.len()` 返回字节数。含中文的配置会“看起来更小”，
/// 这是有意的保守方向（宁可多拒一次，不可放行过大输入）。

pub const MAX_WORKSPACE_CONFIG_BYTES: usize = 64 * 1024;

/// 无状态配置解析器。
///
/// 【为什么是一个空结构体】
/// 它没有任何字段，也不持有连接池——全部状态都在返回的 `Resolution` 里。
/// 这是刻意的：解析器可以被随意复制/新建，不存在“半初始化状态”，也不可能被误复用。
///
/// 【调用者】 `kiana-provider/src/lib.rs::ProviderGateway::from_env`。

#[derive(Clone, Debug, Default)]
pub struct ConfigResolver;

#[derive(Clone)]
/// 一次配置解析的完整产物。
///
/// 【字段】
/// - `connections`：profile 名 -> 连接。**含密钥句柄与 HTTP client**，是内部结构。
/// - `explicit_profiles`：是否来自 `KIANA_MODEL_PROFILES_JSON` 显式配置。
///   这个布尔值很关键：见下面 `ProviderGateway::connection()` 的逻辑——
///   没有显式配置时，未知 profile 会**回落到 default**；有显式配置时则**直接报错**。
///   否则用户拼错一个 profile 名就会静默地用上默认服务商。
/// - `snapshot`：无密钥的对外快照，用于展示与版本围栏。

pub(crate) struct Resolution {
    pub connections: BTreeMap<String, Connection>,
    pub explicit_profiles: bool,
    pub snapshot: ProviderConfigSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 工作区覆盖配置的**结构化**形态（项目里的那份 JSON 文本解析后的结果）。
///
/// 【信任前提】 只有 `parse_workspace`（且 `project_trusted == true`）才会构造它。
///
/// 【字段】
/// - `schema` / `version`：必须等于 `WORKSPACE_CONFIG_SCHEMA` / `..._VERSION`，否则拒绝。
/// - `provider` / `model` / `base_url`：整体覆盖默认值。`Option` 的 `None` 表示“不覆盖，继承”。
/// - `api_key_env`：**环境变量名**，不是密钥本身。注意这里存的是名字——
///   密钥值永远不进这个结构体。
/// - `profiles`：按 profile 名分组的局部覆盖。
///
/// 【serde 注解】 `deny_unknown_fields` 表示多写一个字段就整体解析失败。
/// 代价是老配置升级后可能报错，收益是**拼错的字段不会被静默忽略**——
/// 后者危险得多（用户以为自己配了 base_url，其实没生效）。

pub struct WorkspaceConfig {
    pub schema: String,
    pub version: u64,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, WorkspaceProfile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 单个 profile 的覆盖项。
///
/// 【与 `WorkspaceConfig` 的关键差别】
/// 这里的 `provider` 和 `model` 是**必填**（不是 `Option`），
/// 含义是“这个 profile 必须明确指定服务商和模型，不允许隐式继承”。
/// 顶层配置可以只写一半往下传，profile 不行——profile 是一个明确的路由目标，
/// 半定义的路由目标没有意义。
///
/// 【inherit_default】 为 `true` 时表示“整体继承 default 连接”。
/// 此时若又写了 `base_url` 或 `api_key_env`，`validate_workspace` 会报
/// `config_profile_inheritance_conflict`：既说继承又说要改，语义自相矛盾，
/// 必须在配置阶段拒绝，而不是猜一个。

pub struct WorkspaceProfile {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub inherit_default: bool,
}

impl ConfigResolver {
    /// Resolve the only production provider connection/profile parser.
    /// 解析生产环境唯一的 provider 连接/配置。
    ///
    /// 【作用】 组装 `Resolution`：连接表 + 是否有显式 profile + 无密钥快照。
    ///
    /// 【调用者】 `ProviderGateway::from_env`（`kiana-provider/src/lib.rs`），
    /// 再往上追就是 `kiana-daemon/src/model_client.rs::from_config`。
    ///
    /// 【输入】 `config`：由 daemon 组装的原始配置（provider / model / base_url / api_key）。
    ///   每一项都是 `Option`——`None` 表示“去环境变量找，再找不到用内置默认”。
    ///
    /// 【输出】 `Resolution`。
    ///
    /// 【副作用】
    /// 无网络 I/O、无环境变量**写入**。它会**读取**环境变量（通过 `config::connections`），
    /// 并构造 `reqwest::Client`。构造 client 会创建连接池，但不建立连接（reqwest 是惰性的）。
    ///
    /// 【失败情况】 任何 profile / provider / endpoint 校验失败都会向上传播为 `ModelError`。
    ///   注意这里是**启动即失败**，不是运行时失败——配置错了就该立刻知道。

    pub(crate) fn resolve(config: ProviderConfig) -> Result<Resolution, kiana_domain::ModelError> {
        let (connections, explicit_profiles) = config::connections(config)?;
        let snapshot = config::snapshot(&connections)?;
        Ok(Resolution {
            connections,
            explicit_profiles,
            snapshot,
        })
    }

    /// Parse a strict workspace overlay after the caller has established ProjectTrust.
    /// 在**已建立 ProjectTrust 的前提下**解析工作区覆盖配置。
    ///
    /// 【作用】 把项目里的 JSON 文本变成经过校验的 `WorkspaceConfig`。
    ///
    /// 【调用者】
    /// 仓库中未在 daemon/core 找到直接调用点；`workspace_snapshot` 内部会调用它。
    /// 它是 `pub` 的，供未来接入工作区配置面时使用。
    ///
    /// 【输入】
    /// - `raw`：配置文件文本。
    /// - `project_trusted`：**由调用方提供的信任结论**。本函数不自己判断。
    ///
    /// 【输出】 校验通过的 `WorkspaceConfig`。
    ///
    /// 【副作用】 无。不读文件、不写文件、不查网。
    ///
    /// 【核心流程】 三步，顺序不可换：
    /// ```text
    ///   1. project_trusted == false ?  -> 立即拒绝   ← 必须在最前面
    ///   2. 长度 / NUL 字节检查        -> 拒绝
    ///   3. JSON 解析 + validate_workspace() -> 拒绝或返回
    /// ```
    ///
    /// 【⚠ 为什么信任检查必须在解析之前】
    /// 如果先解析再检查信任，一个恶意仓库可以用一个“超大 / 结构诡异”的 JSON
    /// 触发解析器里的 CPU/内存消耗——**信任检查晚一步，攻击面就大一步**。
    /// 先问“你是谁”，再问“你说了什么”。
    ///
    /// 【⚠ 为什么拒绝 NUL 字节】 NUL 会截断后续传递给 C 层（libc / 某些系统调用）的字符串，
    /// 是典型的路径穿越前奏。

    pub fn parse_workspace(
        raw: &str,
        project_trusted: bool,
    ) -> Result<WorkspaceConfig, kiana_domain::ModelError> {
        if !project_trusted {
            return Err(kiana_domain::ModelError::invalid(
                "config_workspace_untrusted",
            ));
        }
        if raw.len() > MAX_WORKSPACE_CONFIG_BYTES || raw.contains('\0') {
            return Err(kiana_domain::ModelError::invalid(
                "config_workspace_too_large",
            ));
        }
        let config: WorkspaceConfig = serde_json::from_str(raw)
            .map_err(|_| kiana_domain::ModelError::invalid("config_workspace_invalid"))?;
        validate_workspace(&config)?;
        Ok(config)
    }

    /// Parse a trusted overlay and produce the canonical secret-free domain snapshot used for
    /// revision fencing. `project_trust_revision` must already be a digest from the authority
    /// boundary; this function does not infer trust from the config text.
    /// 解析可信覆盖配置，并产出用于版本围栏（revision fencing）的**无密钥**规范快照。
    ///
    /// 【作用】 `parse_workspace` 的加强版：除了解析，还把结果转成
    /// `ConfigSnapshot`（领域类型），附带内容摘要与信任版本号。
    ///
    /// 【调用者】 同上，未在 daemon/core 找到直接调用点。
    ///
    /// 【输入】
    /// - `raw` / `project_trusted`：同 `parse_workspace`。
    /// - `project_trust_revision`：**必须已经是权威边界算出的摘要**。
    ///   本函数原样塞进快照，不做二次解释、不从配置文本推断。
    ///
    /// 【输出】 `ConfigSnapshot`，包含：
    /// - 来源标签 `"workspace:provider-config"`（用于溯源：这份配置从哪来）。
    /// - `effective`：解析后的配置重新序列化成的规范 JSON。
    /// - 摘要 `json_digest(&effective)`：内容变了摘要就变。
    /// - `project_trust_revision`：信任侧版本。
    ///
    /// 【副作用】 无。
    ///
    /// 【⚠ 为什么先 parse 再 to_value 再 hash，而不是直接 hash 原始文本】
    /// 因为 `raw` 里字段顺序、空白、是否省略默认值都可能不同，但**语义相同**。
    /// 先规范化再哈希，得到的是“语义摘要”：只改格式不会让摘要变，
    /// 改真实内容才会。这正是版本围栏需要的语义。

    pub fn workspace_snapshot(
        raw: &str,
        project_trusted: bool,
        project_trust_revision: &str,
    ) -> Result<ConfigSnapshot, kiana_domain::ModelError> {
        let config = Self::parse_workspace(raw, project_trusted)?;
        let effective = serde_json::to_value(&config)
            .map_err(|_| kiana_domain::ModelError::invalid("config_workspace_encode_failed"))?;
        ConfigSnapshot::new(
            vec!["workspace:provider-config".to_owned()],
            effective.clone(),
            json_digest(&effective),
            project_trust_revision.to_owned(),
        )
        .map_err(kiana_domain::ModelError::invalid)
    }
}

/// 校验一份已解析的工作区配置是否合法。
///
/// 【作用】 所有**语义**层面的检查集中在这里（长度、枚举合法性、端点安全、继承冲突）。
/// 纯函数，不读环境变量、不查网。
///
/// 【核心流程】
/// ```text
///   1. schema / version 逐字匹配           -> config_workspace_schema_unsupported
///   2. provider / model 长度与空串         -> config_provider_invalid / config_model_invalid
///   3. base_url 走 validate_endpoint()     -> TLS / 无凭据 / 无 query 检查
///   4. api_key_env 走 validate_env_ref()   -> 只允许 [A-Z0-9_]
///   5. profiles 数量 <= 64                 -> config_profile_limit_exceeded
///   6. 逐个 profile：名字必须在 RoleSpec 目录里 + inherit 不与 base_url/api_key_env 共存
/// ```
///
/// 【⚠ 第 6 步“名字必须在 RoleSpec::catalog() 里”是什么意思】
/// profile 名不是随便起的字符串，它必须对应一个**真实存在的角色**。
/// 因为路由时是“角色 -> profile”，如果配置里写了一个没有任何角色引用的 profile，
/// 它就是一个永远不会被用到、也没人审查过的死配置。直接拒绝更安全。

fn validate_workspace(config: &WorkspaceConfig) -> Result<(), kiana_domain::ModelError> {
    if config.schema != WORKSPACE_CONFIG_SCHEMA || config.version != WORKSPACE_CONFIG_VERSION {
        return Err(kiana_domain::ModelError::invalid(
            "config_workspace_schema_unsupported",
        ));
    }
    for (value, code, max) in [
        (config.provider.as_deref(), "config_provider_invalid", 128),
        (config.model.as_deref(), "config_model_invalid", 256),
    ] {
        if value.is_some_and(|value| value.trim().is_empty() || value.len() > max) {
            return Err(kiana_domain::ModelError::invalid(code));
        }
    }
    if let Some(base_url) = &config.base_url {
        validate_endpoint(base_url)?;
    }
    validate_env_ref(config.api_key_env.as_deref())?;
    if config.profiles.len() > 64 {
        return Err(kiana_domain::ModelError::invalid(
            "config_profile_limit_exceeded",
        ));
    }
    let allowed = RoleSpec::catalog()
        .into_iter()
        .map(|role| role.model_profile)
        .collect::<std::collections::BTreeSet<_>>();
    for (name, profile) in &config.profiles {
        if !allowed.contains(name)
            || profile.provider.trim().is_empty()
            || profile.provider.len() > 128
            || profile.model.trim().is_empty()
            || profile.model.len() > 256
        {
            return Err(kiana_domain::ModelError::invalid("config_profile_invalid"));
        }
        if let Some(base_url) = &profile.base_url {
            validate_endpoint(base_url)?;
        }
        validate_env_ref(profile.api_key_env.as_deref())?;
        if profile.inherit_default && (profile.base_url.is_some() || profile.api_key_env.is_some())
        {
            return Err(kiana_domain::ModelError::invalid(
                "config_profile_inheritance_conflict",
            ));
        }
    }
    Ok(())
}

/// 校验“密钥环境变量名”。
///
/// 【作用】 只校验**名字**的形状，永远不读取那个环境变量的值。
///
/// 【为什么只允许 `[A-Z0-9_]`】
/// - 大写 + 下划线是环境变量名的通用惯例，便于识别。
/// - 禁止点号、斜杠、空格等，可以防止“用奇怪的名字读到别的东西”。
/// - 最关键：这条白名单让**路径穿越式取值**不可能发生——名字里根本没有 `/`。
///
/// 【⚠ 这不是密钥安全检查】 它保证的是“名字长得像环境变量名”，
/// 至于那个变量里装的是什么、是不是真密钥，由 `credentials.rs` 在使用时校验。

fn validate_env_ref(value: Option<&str>) -> Result<(), kiana_domain::ModelError> {
    if value.is_some_and(|value| {
        value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    }) {
        return Err(kiana_domain::ModelError::invalid(
            "config_secret_ref_invalid",
        ));
    }
    Ok(())
}

/// 校验一个 provider 端点 URL 是否可以安全使用。
///
/// 【作用】 这是**凭据外泄的最后一道闸**：密钥即将被发往这个 URL，所以 URL 必须无毒。
///
/// 【检查项，逐条都有存在理由】
/// ```text
///   1. 能被 Url::parse 解析                -> 否则连格式都不对
///   2. 不能带 username / password         -> https://user:pass@host
///        理由：URL 里的凭据会进日志、进错误信息，等于泄漏。
///   3. 不能带 query                       -> ?api_key=xxx
///        理由：同上；且 query 常常会被中间层记录。
///   4. 不能带 fragment (#xxx)             -> 同上
///   5. scheme 必须是 https，
///        或者 scheme == http 且 host 是 loopback -> 否则拒绝
///        理由：明文 HTTP 会把 API key 直接暴露在网络上。
///              例外是本地回环（Ollama 跑在 127.0.0.1），那里根本没有网络可偷。
/// ```
///
/// 【⚠ 这里只管“形状”，不管“归属”】
/// 一个 `https://evil.example.com` 完全能通过这个检查。
/// 判断“这个域名是否是我们允许的上游”属于信任/策略问题，不属于 URL 语法问题。
/// 本函数只负责排除**明显有毒**的写法。

fn validate_endpoint(value: &str) -> Result<(), kiana_domain::ModelError> {
    let endpoint = reqwest::Url::parse(value.trim())
        .map_err(|_| kiana_domain::ModelError::invalid("config_endpoint_invalid"))?;
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(kiana_domain::ModelError::invalid(
            "config_endpoint_credentials_or_query_denied",
        ));
    }
    let local = endpoint.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && local) {
        return Err(kiana_domain::ModelError::invalid(
            "config_endpoint_requires_tls_or_loopback",
        ));
    }
    Ok(())
}

/// Keep the resolver's effective config shape inspectable without exposing credentials.
/// 把工作区配置转成可检查的 JSON 值，**不包含任何密钥**。
///
/// 【作用】 供诊断/测试查看“最终生效的配置长什么样”。
///
/// 【⚠ 为什么函数名叫 redacted 却没有任何脱敏代码】
/// 因为它**结构上就不可能**带上密钥：`WorkspaceConfig` 里存的是
/// `api_key_env`（环境变量**名**），不是密钥值本身。
/// 脱敏不是“事后擦掉”，而是“从来就没放进来”——这比事后脱敏可靠得多。
///
/// 【副作用】 无。

pub fn redacted_workspace_value(
    config: &WorkspaceConfig,
) -> Result<Value, kiana_domain::ModelError> {
    serde_json::to_value(config)
        .map_err(|_| kiana_domain::ModelError::invalid("config_workspace_encode_failed"))
}
