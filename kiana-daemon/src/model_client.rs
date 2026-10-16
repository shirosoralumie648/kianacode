//! Daemon 为 Harness 选择并包装 `ModelClient` 的适配层。
//!
//! `DaemonHost::local_with_model_config` 通过 [`from_config`] 注入调用方选项，默认 Host 装配则通过
//! [`from_env`] 读取环境配置。生产选择结果是脚本模型、显式 fake、`kiana-provider::ProviderGateway`
//! 或 `UnavailableModel`，再经 `InstrumentedModelClient` 暴露给 Runner。
//!
//! 生产代码到 `InstrumentedModelClient` 为止；它在模型端口边界检查 metadata 后委托给实际实现。
//! 文件后半段的 `legacy_fixtures` 位于 `#[cfg(test)]`，只为测试保留旧的 provider 适配和 fake，
//! 不属于产品构造路径。provider 请求编译与协议转换的产品实现应继续读 `kiana-provider`。
use crate::LocalModelConfig;
use async_trait::async_trait;
use kiana_ports::ModelClient;
use kiana_runner::{ScriptedModel, UnavailableModel};
use std::sync::Arc;
const ENV_MODEL_MODE: &str = "KIANA_MODEL_MODE";
const ENV_STREAMING: &str = "KIANA_STREAMING";

/// 用默认 `LocalModelConfig` 创建模型端口；DaemonHost 的环境配置装配路径会调用它。
pub(crate) fn from_env() -> Arc<dyn ModelClient> {
    from_config(LocalModelConfig::default())
}

/// 按优先级选择当前 Host 使用的模型实现，并用审计边界 wrapper 包装。
///
/// 选择失败不会在 Host 构造阶段丢失模型端口，而是返回会在调用时报告错误的
/// `UnavailableModel`；`Arc` 共享具体实现的所有权，`dyn ModelClient` 则让 Runner 不必知道其具体类型。
pub(crate) fn from_config(config: LocalModelConfig) -> Arc<dyn ModelClient> {
    // 非空脚本路径声明了 cassette 来源；若同时要求 live，下面会把冲突明确转成 unavailable。
    let cassette = std::env::var("KIANA_HARNESS_SCRIPT")
        .ok()
        .is_some_and(|path| !path.trim().is_empty());
    // 先验证明确的模式声明；无效值包装成 UnavailableModel，避免静默改用另一 provider。
    let mode = match selection_mode_from_env() {
        Ok(mode) => mode,
        Err(error) => {
            return Arc::new(InstrumentedModelClient {
                inner: Arc::new(UnavailableModel::new(error)),
            })
        }
    };
    if cassette && mode == Some("live") {
        // 同时声明 cassette 路径和 live 模式的配置互相矛盾，不猜测调用方希望哪一个优先。
        return Arc::new(InstrumentedModelClient {
            inner: Arc::new(UnavailableModel::new("model_selection_conflict")),
        });
    }
    if !cassette && mode == Some("cassette") {
        // 显式要求 cassette 却没给脚本文件时，不能回退到可能联网的 provider。
        return Arc::new(InstrumentedModelClient {
            inner: Arc::new(UnavailableModel::new("cassette_required")),
        });
    }
    // 即使选中的模型支持流式，也先拒绝拼写不受支持的流式策略；开关的实际语义由下游 client 处理。
    if let Err(error) = validate_streaming_environment() {
        return Arc::new(InstrumentedModelClient {
            inner: Arc::new(UnavailableModel::new(error)),
        });
    }
    // 拒绝条件通过后才选实现：脚本文件优先，其次精确的 fake provider，其余交给 ProviderGateway。
    // KIANA_MODEL_MODE=live 本身不会强制选 ProviderGateway；此处它只参与前面的值校验和 cassette 冲突检查。
    let client: Arc<dyn ModelClient> = match std::env::var("KIANA_HARNESS_SCRIPT") {
        // 脚本加载失败同样保留为统一 ModelClient 错误实现，不在 Host 装配处 panic。
        Ok(path) if !path.trim().is_empty() => match ScriptedModel::from_json_path(path.trim()) {
            Ok(model) => Arc::new(model),
            Err(error) => Arc::new(UnavailableModel::new(error)),
        },
        // config.provider 优先于 KIANA_PROVIDER；只有精确值 `fake` 才选择内建确定性模型。
        _ if config
            .provider
            .as_deref()
            .or(std::env::var("KIANA_PROVIDER").ok().as_deref())
            == Some("fake") =>
        {
            // `fake-text-only` 是 provider 注册表中的明确能力降级：它不能接收工具，
            // 因而必须在模型端口边界报告稳定的 `unsupported_tools`，不能用普通文本
            // cassette 冒充可执行工具模型。其余 fake model 仍走确定性的本地响应队列。
            if std::env::var("KIANA_FAKE_MODEL")
                .ok()
                .is_some_and(|model| model.trim().eq_ignore_ascii_case("fake-text-only"))
            {
                Arc::new(UnavailableModel::new("unsupported_tools"))
            } else {
                Arc::new(ScriptedModel::new(
                    (0..128)
                        .map(|_| {
                            kiana_domain::ModelOutput::text("Deterministic fake provider response")
                        })
                        .collect(),
                ))
            }
        }
        _ => match kiana_provider::ProviderGateway::from_env(kiana_provider::ProviderConfig {
            provider: config.provider,
            model: config.model,
            base_url: config.base_url,
            api_key: config.api_key,
        }) {
            Ok(gateway) => Arc::new(gateway),
            // 配置/凭据/adapter 初始化失败转成端口内的 unavailable 状态，实际请求时再呈现错误。
            Err(error) => Arc::new(UnavailableModel::new(format!("model_unavailable:{error}"))),
        },
    };
    // 所有分支都收敛到 instrumented wrapper，保持 ModelClient 端口形状一致。
    Arc::new(InstrumentedModelClient { inner: client })
}

/// 解析可选的模型模式声明；只接受 live 与 cassette 两种显式值。
fn selection_mode_from_env() -> Result<Option<&'static str>, String> {
    // 未设置或空白值表示“采用普通自动选择”；ASCII 大小写写法等价。
    match std::env::var(ENV_MODEL_MODE)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .as_deref()
    {
        None => Ok(None),
        Some("live") => Ok(Some("live")),
        Some("cassette") => Ok(Some("cassette")),
        Some(_) => Err("model_mode_invalid".to_owned()),
    }
}

/// 校验 `KIANA_STREAMING` 的词法取值；实际 Auto/On/Off 语义由 provider client 再解释。
fn validate_streaming_environment() -> Result<(), String> {
    // 未设置允许采用 Auto；变量若被设置，则必须匹配明确白名单，空字符串也会被拒绝。
    match std::env::var(ENV_STREAMING)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("auto" | "on" | "1" | "true" | "yes" | "off" | "0" | "false" | "no") => Ok(()),
        Some(_) => Err("model_streaming_policy_invalid".to_owned()),
    }
}

/// Runner 共用的模型端口包装器：在调用内层实现前校验可观测的 model-attempt metadata。
///
/// 它不会写事件、调用 Broker 或替代 ControlPlane 的事件 reducer；`inner` 用 trait object 隐去
/// ScriptedModel、ProviderGateway 等具体类型，并通过 `Arc` 共享所有权。
struct InstrumentedModelClient {
    /// 实际模型实现；wrapper 只加 metadata 检查，其余模型行为交回此对象。
    inner: Arc<dyn ModelClient>,
}

// 与 ModelClient trait 上的 async_trait 宏保持一致，生成兼容的异步方法签名。
#[async_trait]
impl ModelClient for InstrumentedModelClient {
    fn prepare_call(
        &self,
        request: kiana_domain::ModelRequest,
        spec: kiana_domain::ModelCallSpec,
    ) -> Result<kiana_domain::PreparedModelCall, kiana_domain::ModelError> {
        // 先让具体实现构造 provider 调用，再校验将随调用暴露的可观测 metadata。
        let prepared = self.inner.prepare_call(request, spec)?;
        kiana_provider::safe_prepared_metadata(&prepared)?;
        Ok(prepared)
    }

    fn request_context(
        &self,
        request: &kiana_domain::ModelRequest,
    ) -> kiana_domain::ModelRequestContext {
        // 请求上下文语义由实际模型实现提供，wrapper 不在此处重写。
        self.inner.request_context(request)
    }

    async fn complete(
        &self,
        request: kiana_domain::ModelRequest,
    ) -> Result<kiana_domain::ModelOutput, String> {
        // 非流式入口只委托模型端口；错误类型仍沿原有 Result 返回。
        self.inner.complete(request).await
    }

    async fn complete_streaming(
        &self,
        request: kiana_domain::ModelRequest,
        on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<kiana_domain::ModelOutput, String> {
        // 将 Runner 提供的 delta 回调直接借给内层模型，让增量沿同一回调路径返回。
        self.inner.complete_streaming(request, on_delta).await
    }

    async fn complete_prepared(
        &self,
        prepared: kiana_domain::PreparedModelCall,
        on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<kiana_domain::ModelReply, kiana_domain::ModelError> {
        // Prepared call 可在 prepare 后单独传入，因此在真正发送前再次检查其 metadata。
        kiana_provider::safe_prepared_metadata(&prepared)?;
        self.inner.complete_prepared(prepared, on_delta).await
    }

    async fn complete_admitted(
        &self,
        prepared: kiana_domain::PreparedModelCall,
        permit: kiana_domain::ModelCallPermit,
        admission: &dyn kiana_ports::ModelBudgetPort,
        on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<kiana_domain::ModelReply, kiana_domain::ModelError> {
        // 保持 permit、预算 admission 端口和 delta 回调原样传递；此 wrapper 不自行签发预算许可。
        kiana_provider::safe_prepared_metadata(&prepared)?;
        self.inner
            .complete_admitted(prepared, permit, admission, on_delta)
            .await
    }
}

// 仅为离线测试保留的旧模型装配实现；cfg(test) 使它不会进入产品构建。
#[cfg(test)]
mod legacy_fixtures {
    //! 测试专用的历史模型适配层，用于在离线测试中驱动旧 Provider、重试和流式转换实现。
    //!
    //! 它不属于产品的 `ModelClient` 装配路径：产品前段调用 `kiana-provider::ProviderGateway`；
    //! 这里为了覆盖兼容夹具而直接依赖 `kiana-services`。脚本模型优先，没有可用凭据时 fail closed。

    use async_trait::async_trait;
    use kiana_domain::{ModelContent, RoleSpec};
    use kiana_runner::{
        ModelClient, ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelRequestContext,
        ModelRole, ModelToolCall, ModelUsage, ScriptedModel, UnavailableModel,
    };
    use kiana_services::api::errors::ApiErrorKind;
    use kiana_services::api::messages::{Message, MessagesRequest, MessagesResponse};
    use kiana_services::api::provider::{
        next_provider_stream_event, provider_registry_entry, AnthropicProvider, FakeProvider,
        OllamaProvider, OpenAiCompatibleProvider, Provider, ProviderError, ProviderStream,
        ANTHROPIC_PROVIDER_ID, FAKE_PROVIDER_ID, OLLAMA_PROVIDER_ID, OPENAI_COMPATIBLE_PROVIDER_ID,
    };
    use kiana_services::api::retry::{with_retry, RetryConfig};
    use kiana_services::api::streaming::{
        ContentBlock as StreamContentBlock, Delta as StreamDelta, StreamEvent,
    };
    use kiana_services::errors::ServiceError;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::LocalModelConfig;

    // 测试夹具仍读取与产品一致的脚本变量，方便用同一个 cassette 检验选择优先级。
    const ENV_HARNESS_SCRIPT: &str = "KIANA_HARNESS_SCRIPT";
    const ENV_PROVIDER: &str = "KIANA_PROVIDER";
    const ENV_STREAMING: &str = "KIANA_STREAMING";
    // 下面的超时和重试参数只服务于旧离线夹具，不是生产 ProviderGateway 的运行配置。
    const DEFAULT_MAX_TOKENS: u32 = 4096;
    const PROVIDER_TIMEOUT: Duration = Duration::from_secs(60);
    const MODEL_MAX_RETRIES: u32 = 2;
    const MODEL_RETRY_BASE_DELAY: Duration = Duration::from_millis(100);
    // 无凭据时附带可操作的本地运行提示，避免测试失败只暴露内部 provider 错误。
    const MODEL_UNAVAILABLE_HINT: &str = "没有可用的模型。请设置 ANTHROPIC_API_KEY；或设置 KIANA_PROVIDER=ollama（可用 KIANA_OLLAMA_BASE_URL 指定地址，默认 http://localhost:11434）；或设置 KIANA_HARNESS_SCRIPT=/path/to/cassette.json 运行本地 cassette。";

    /// 用默认模型配置创建测试夹具的模型端口。
    pub(crate) fn from_env() -> Arc<dyn ModelClient> {
        from_config(LocalModelConfig::default())
    }

    /// 先尝试 cassette；没有脚本时再建 provider client，并附加测试用 profile router。
    ///
    /// 初始化失败被包装成 `UnavailableModel`，使测试能观察 fail-closed 行为而不是在装配时 panic。
    pub(crate) fn from_config(config: LocalModelConfig) -> Arc<dyn ModelClient> {
        match std::env::var(ENV_HARNESS_SCRIPT) {
            // 脚本路径非空时优先回放固定轮次；JSON 文件读取/解析失败转换为可调用的错误模型。
            Ok(path) if !path.trim().is_empty() => match ScriptedModel::from_json_path(path.trim())
            {
                Ok(model) => Arc::new(model),
                Err(error) => Arc::new(unavailable_model(Some(&error))),
            },
            _ => {
                // 无脚本时由环境和 LocalModelConfig 构造默认 provider；缺失/失败都成为 unavailable。
                let default = match provider_from_env(config) {
                    Ok(Some(model)) => model,
                    Ok(None) => Arc::new(unavailable_model(None)),
                    Err(error) => Arc::new(unavailable_model(Some(&error))),
                };
                // 仅测试夹具支持按 prompt profile 选择额外 provider；配置无效时整体 fail closed。
                profile_routes(default)
                    .unwrap_or_else(|error| Arc::new(unavailable_model(Some(&error))))
            }
        }
    }

    /// 一条由运行者提供的模型 profile 配置。
    ///
    /// profile 配置只在夹具装配时从环境变量读取一次；模型提示中的内容只能选择
    /// `RoleSpec` 已知的 profile 名称，不能自行指定 provider、模型地址或密钥。
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ConfiguredProfile {
        /// Provider 注册表中的适配器 ID，由 `provider_from_env` 解析。
        provider: String,
        /// 交给所选 provider 的模型 ID。
        model: String,
        /// 可选服务地址；是否采用及其格式由具体 provider 决定。
        #[serde(default)]
        base_url: Option<String>,
        /// 可选的密钥环境变量名；配置保存变量名，不把密钥本身写入 JSON。
        #[serde(default)]
        api_key_env: Option<String>,
    }

    /// 测试夹具中的 profile 分发器。
    ///
    /// `default` 保留普通 provider，`profiles` 保存按角色 profile 名称装配的替代客户端。
    /// 两者用 `Arc` 共享，允许路由器在一次请求期间借用客户端而不转移其所有权。
    struct ProfileRouter {
        /// 没有请求专用配置，或请求的 profile 未被配置时使用的客户端。
        default: Arc<dyn ModelClient>,
        /// 仅包含通过角色目录白名单校验并成功装配的客户端。
        profiles: BTreeMap<String, Arc<dyn ModelClient>>,
    }

    /// 从环境变量建立测试用的角色模型路由。
    ///
    /// `KIANA_MODEL_PROFILES_JSON` 为空时直接保留默认客户端；否则先限制配置大小、解析
    /// 严格 JSON，再校验名称来自 `RoleSpec` 目录，并逐项创建 provider。任一配置无效都会
    /// 返回错误，由调用者把整套路由转换成 `UnavailableModel`，避免静默采用部分配置。
    fn profile_routes(default: Arc<dyn ModelClient>) -> Result<Arc<dyn ModelClient>, String> {
        // 未配置 profile 时不额外包一层路由器，维持默认客户端原有行为。
        let Some(raw) = std::env::var("KIANA_MODEL_PROFILES_JSON")
            .ok()
            .filter(|v| !v.trim().is_empty())
        else {
            return Ok(default);
        };
        // 环境变量也可能过大；在 JSON 解析和分配嵌套对象前设定输入上限。
        if raw.len() > 64 * 1024 {
            return Err("model_profile_config_too_large".to_owned());
        }
        // BTreeMap 同时限定顶层配置形状，并以稳定顺序遍历 profile，便于测试结果复现。
        let configs: BTreeMap<String, ConfiguredProfile> =
            serde_json::from_str(&raw).map_err(|_| "model_profile_config_invalid".to_owned())?;
        // 模型请求只应引用程序已定义的角色 profile，不能借提示内容创建任意路由名。
        let allowed = RoleSpec::catalog()
            .into_iter()
            .map(|r| r.model_profile)
            .collect::<std::collections::BTreeSet<_>>();
        let mut profiles = BTreeMap::new();
        for (name, config) in configs {
            // provider 和模型名不能为空；profile 名称必须来自上面的角色目录。
            if !allowed.contains(&name)
                || config.provider.trim().is_empty()
                || config.model.trim().is_empty()
            {
                return Err("model_profile_config_invalid".to_owned());
            }
            // 只允许环境变量标识符字符，避免把任意字符串当作变量引用；密钥值仍从进程环境读取。
            let api_key = match config.api_key_env {
                Some(key) => {
                    if key.is_empty()
                        || !key
                            .bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                    {
                        return Err("model_profile_key_reference_invalid".to_owned());
                    }
                    Some(
                        std::env::var(key)
                            .ok()
                            .filter(|v| !v.trim().is_empty())
                            .ok_or_else(|| "model_profile_key_unavailable".to_owned())?,
                    )
                }
                None => None,
            };
            // 复用夹具的 provider 装配逻辑。返回 None 表示该 provider 缺少运行条件，不能加入路由表。
            let model = provider_from_env(LocalModelConfig {
                provider: Some(config.provider),
                model: Some(config.model),
                base_url: config.base_url,
                api_key,
            })?
            .ok_or_else(|| format!("model_profile_unavailable:{name}"))?;
            profiles.insert(name, model);
        }
        // 所有 profile 都成功装配后才发布路由器；前面的任何错误都会拒绝整份配置。
        Ok(Arc::new(ProfileRouter { default, profiles }))
    }

    /// 从模型请求的系统消息中提取第一个可解码 PromptBundle 的 profile 名称。
    ///
    /// 普通系统消息或解码失败的消息会被跳过；这里只读取已组装提示中的选择结果，
    /// 不从用户消息中接受路由指令。
    fn request_profile(request: &ModelRequest) -> Option<String> {
        request
            .messages
            .iter()
            .filter(|message| message.role == ModelRole::System)
            .find_map(|message| {
                // find_map 让格式不匹配的系统消息继续向后搜索，首个有效 bundle 决定 profile。
                kiana_domain::PromptBundle::decode(&message.text)
                    .ok()
                    .map(|bundle| bundle.model_profile)
            })
    }

    // 路由方法返回借用的 trait object：客户端由 ProfileRouter 中的 Arc 持有，调用期间无需克隆 Arc。
    impl ProfileRouter {
        /// 选择已配置的专用客户端；未携带 profile 或未配置该名称时回退到默认客户端。
        fn select(&self, request: &ModelRequest) -> &dyn ModelClient {
            request_profile(request)
                .and_then(|name| self.profiles.get(&name))
                .unwrap_or(&self.default)
                .as_ref()
        }
    }

    // 该实现只存在于测试夹具中；三个入口统一按同一条 profile 规则转发给实际客户端。
    #[async_trait]
    impl ModelClient for ProfileRouter {
        /// 先向选中的客户端询问请求上下文，再附加此次 profile 路由结果供测试诊断。
        fn request_context(&self, request: &ModelRequest) -> ModelRequestContext {
            let mut context = self.select(request).request_context(request);
            if let Some(profile) = request_profile(request) {
                // 即使名称没有专用配置，也记录回退事实，避免把 default 路径误读为 profile 命中。
                context.sources.push(format!(
                    "model_profile:{profile}:{}",
                    if self.profiles.contains_key(&profile) {
                        "configured"
                    } else {
                        "default"
                    }
                ));
            }
            context
        }

        /// 将完整请求交给已选客户端；`async_trait` 把异步 trait 方法转换为可通过 `dyn` 调用的形式。
        async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
            self.select(&request).complete(request).await
        }

        /// 将请求和增量回调一起转发，流式事件仍由实际 provider 客户端产生。
        async fn complete_streaming(
            &self,
            request: ModelRequest,
            on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
        ) -> Result<ModelOutput, String> {
            self.select(&request)
                .complete_streaming(request, on_delta)
                .await
        }
    }

    /// 把装配失败转成可注入的模型端口，延迟到调用模型时再向上层报告原因。
    ///
    /// 空白原因被忽略，避免错误文本出现只有分隔符的情况；构造本身不会触发网络请求。
    fn unavailable_model(reason: Option<&str>) -> UnavailableModel {
        let message = match reason.map(str::trim).filter(|reason| !reason.is_empty()) {
            Some(reason) => format!("model_unavailable: {reason}. {MODEL_UNAVAILABLE_HINT}"),
            None => format!("model_unavailable: {MODEL_UNAVAILABLE_HINT}"),
        };
        UnavailableModel::new(message)
    }

    /// 按配置、环境变量和 provider 注册表构造旧测试夹具使用的模型客户端。
    ///
    /// Provider ID 先取 `LocalModelConfig`，再取 `KIANA_PROVIDER`，最后默认 Anthropic；模型 ID
    /// 同样按显式配置、注册表声明的环境变量、注册表默认值回退。需要密钥的远端 provider
    /// 缺少密钥时返回 `Ok(None)`，与配置错误区分开来，由调用者决定如何 fail closed。
    /// 该函数只创建适配器，不会在装配时发送模型请求。
    fn provider_from_env(config: LocalModelConfig) -> Result<Option<Arc<dyn ModelClient>>, String> {
        // 规范化 ID 供注册表和下方 match 共用；空字符串视为未配置而不是未知 provider。
        let provider_id = config
            .provider
            .or_else(|| std::env::var(ENV_PROVIDER).ok())
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| ANTHROPIC_PROVIDER_ID.to_owned());
        // 注册表提供环境变量名、缺省模型和默认地址；未注册 ID 直接报出稳定的定位信息。
        let Some(entry) = provider_registry_entry(&provider_id) else {
            return Err(format!("unknown_provider:{provider_id}"));
        };
        // 明确配置的非空模型名优先，否则依次检查注册表列出的环境变量，最后使用其默认模型。
        let model_id = config
            .model
            .filter(|value| !value.trim().is_empty())
            .or_else(|| first_env(&entry.model_env_vars))
            .unwrap_or(entry.default_model_id);
        // trait object 统一四种 provider 的具体类型；Box 让单个分支可返回不同实现，随后由 Arc 共享客户端。
        let provider: Box<dyn Provider> = match provider_id.as_str() {
            // FakeProvider 读取脚本值生成确定性响应，不需要凭据或真实网络端点。
            FAKE_PROVIDER_ID => Box::new(
                FakeProvider::from_script_value(model_id.clone(), None)
                    .map_err(|error| error.to_string())?,
            ),
            id if id == ANTHROPIC_PROVIDER_ID => {
                // Anthropic 需要 API key；缺少 key 是“当前不可用”，不是 provider 配置解析异常。
                let Some(api_key) = config
                    .api_key
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| first_env(&entry.api_key_env_vars))
                else {
                    return Ok(None);
                };
                // URL 按显式选项、注册表环境变量、注册表缺省地址依次选择。
                let base_url = config
                    .base_url
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| first_env(&entry.base_url_env_vars))
                    .or(entry.default_base_url)
                    .unwrap_or_default();
                Box::new(AnthropicProvider::new(api_key, base_url, PROVIDER_TIMEOUT))
            }
            id if id == OPENAI_COMPATIBLE_PROVIDER_ID => {
                // OpenAI-compatible 适配同样需要 key；无 key 时不构造一个之后必然失败的 client。
                let Some(api_key) = config
                    .api_key
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| first_env(&entry.api_key_env_vars))
                else {
                    return Ok(None);
                };
                // 兼容接口允许自定义 base URL，最终 URL 仍按配置、环境变量、注册表默认值取值。
                let base_url = config
                    .base_url
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| first_env(&entry.base_url_env_vars))
                    .or(entry.default_base_url)
                    .unwrap_or_default();
                Box::new(
                    OpenAiCompatibleProvider::new(api_key, base_url, PROVIDER_TIMEOUT)
                        .map_err(|error| error.to_string())?,
                )
            }
            id if id == OLLAMA_PROVIDER_ID => {
                // 本地 Ollama 适配不使用 API key，只需解析服务地址。
                let base_url = config
                    .base_url
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| first_env(&entry.base_url_env_vars))
                    .or(entry.default_base_url)
                    .unwrap_or_default();
                Box::new(
                    OllamaProvider::new(base_url, PROVIDER_TIMEOUT)
                        .map_err(|error| error.to_string())?,
                )
            }
            // 注册表存在并不代表此测试夹具实现了对应适配器，避免悄悄落到别的协议。
            other => return Err(format!("unknown_provider:{other}")),
        };
        // provider 和测试用流式策略绑定为 ModelClient；Arc 供上层的 dyn ModelClient 端口共享。
        Ok(Some(Arc::new(ProviderModelClient {
            provider,
            model: model_id,
            streaming_policy: streaming_enabled_from_env(),
        })))
    }

    /// 旧测试夹具对流式请求的策略开关；最终能否原生流式还要结合 provider 的模型能力判断。
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum StreamingPolicy {
        /// 完全关闭 provider 原生流式，改用一次性完成后发出单个增量。
        Off,
        /// provider 支持时使用原生流式，不支持时回退到单增量。
        Auto,
        /// 要求原生流式，不支持时返回明确错误。
        On,
    }

    /// 将环境变量解析为测试夹具的流式策略；未识别值保守地按 `Auto` 处理。
    fn streaming_enabled_from_env() -> StreamingPolicy {
        match std::env::var(ENV_STREAMING)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("off" | "0" | "false" | "no") => StreamingPolicy::Off,
            Some("on" | "1" | "true" | "yes") => StreamingPolicy::On,
            Some("auto") | None | Some(_) => StreamingPolicy::Auto,
        }
    }

    /// 为旧 provider client 生成系统提示：先用 Builder 的角色提示，再追加运行者环境指令。
    ///
    /// 这里只负责合并文本；环境指令依旧受请求经过的产品策略和角色约束，不在此处授予工具权限。
    fn provider_system_prompt() -> String {
        let mut prompt = kiana_domain::PromptBundle::for_role(&RoleSpec::builder()).system_prompt();
        for name in ["KIANA_SYSTEM_PROMPT", "KIANA_APPEND_SYSTEM_PROMPT"] {
            // 两个兼容变量按固定顺序追加；空白值不改变默认提示。
            if let Ok(extra) = std::env::var(name) {
                if !extra.trim().is_empty() {
                    prompt.push_str(
                    "\n\nOperator instructions (subject to product policy and assigned role):\n",
                );
                    prompt.push_str(extra.trim());
                }
            }
        }
        prompt
    }

    /// 按 provider 注册表给出的优先顺序返回首个非空环境变量值。
    fn first_env(keys: &[String]) -> Option<String> {
        keys.iter().find_map(|key| {
            // 缺失、不可读或只有空白的变量都会继续尝试下一个候选名。
            std::env::var(key)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    }

    /// 将具体 Provider 与模型、流式策略组合成旧 `ModelClient` 适配器。
    ///
    /// `provider` 使用 `Box<dyn Provider>` 擦除 Anthropic/OpenAI/Ollama 等具体类型；
    /// `ModelClient` 的异步方法再通过 `async_trait` 暴露给 Runner 端口。
    struct ProviderModelClient {
        /// 接收已转换消息并执行实际 provider 请求的协议适配器。
        provider: Box<dyn Provider>,
        /// Provider 请求中使用的模型标识。
        model: String,
        /// 控制 `complete_streaming` 是否请求原生流式。
        streaming_policy: StreamingPolicy,
    }

    #[async_trait]
    impl ModelClient for ProviderModelClient {
        /// 为 Runner 预先生成此 provider 请求的预算与来源上下文。
        ///
        /// 使用 provider 的模型资料填充窗口上限，并把内部消息/工具转成旧服务 DTO 后序列化计字节；
        /// 转换或序列化失败记为 `usize::MAX`，使后续预算校验明确拒绝，而不是低估请求大小。
        fn request_context(&self, request: &ModelRequest) -> ModelRequestContext {
            // `for_request` 先读取请求自带的 prompt 与 token 预算；只有 prompt 为空才补旧 provider 默认值。
            let mut context = ModelRequestContext::for_request(request);
            if context.system_prompt.trim().is_empty() {
                context.system_prompt = provider_system_prompt();
                context
                    .sources
                    .push("builtin:provider_fallback_with_operator_environment".to_owned());
            }
            // 将客户端最终采用的模型身份和注册表窗口写回上下文，供预算检查与上层记录使用。
            context.provider_id = Some(self.provider.provider_id().to_owned());
            context.model_id = Some(self.model.clone());
            context.context_window =
                u64::from(self.provider.model_profile(&self.model).context_window);
            context.reserved_output_tokens = u64::from(DEFAULT_MAX_TOKENS);
            // 预算口径按将要发送的 provider DTO 估算，而不是原始 Rust 结构体的内存占用。
            context.message_bytes = map_messages(&request.messages)
                .ok()
                .and_then(|m| serde_json::to_vec(&m).ok())
                .map(|b| b.len())
                .unwrap_or(usize::MAX);
            // 工具 schema 也计入上下文大小；无法准确编码时采用最大值以触发 fail closed。
            context.tool_schema_bytes = map_tools(&request.tools)
                .ok()
                .and_then(|tools| serde_json::to_vec(&tools).ok())
                .map(|b| b.len())
                .unwrap_or(usize::MAX);
            context
        }

        /// 将内部请求转换为服务 API DTO，验证预算后调用 provider 并转换响应。
        ///
        /// `with_retry` 的闭包每次尝试都克隆已准备好的 owned 请求字段，因为底层异步 API
        /// 消耗 `MessagesRequest`；模型请求失败或预算不合格会以字符串错误返回给调用方。
        async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
            // 先做领域模型到兼容服务 DTO 的转换，无法表示的消息或工具在发网络请求前失败。
            let messages = map_messages(&request.messages)?;
            let tools = map_tools(&request.tools)?;
            // 预算只在完整 provider 形状组装后校验，system prompt、工具和预留输出都能计入。
            let context = self.request_context(&request);
            context.budget().validate().map_err(str::to_owned)?;
            let system = Some(json!(context.system_prompt));
            // with_retry 会重新调用 FnMut 闭包；每轮克隆数据，使 async move 拥有该轮请求的值。
            let response = with_retry(
                || {
                    let messages = messages.clone();
                    let tools = tools.clone();
                    let system = system.clone();
                    async move {
                        // 空工具列表映射为 None，避免给 provider 发送一个语义上“已启用但为空”的 tools 字段。
                        self.provider
                            .create_message(MessagesRequest {
                                model: self.model.clone(),
                                messages,
                                max_tokens: DEFAULT_MAX_TOKENS,
                                system,
                                temperature: None,
                                tools: if tools.is_empty() { None } else { Some(tools) },
                                thinking: None,
                                stream: Some(false),
                            })
                            .await
                            .map_err(provider_error_to_service_error)
                    }
                },
                // 此旧夹具的重试次数和基础间隔集中在 model_retry_config 中。
                model_retry_config(),
            )
            .await
            // 将服务层错误分类重新映射为此旧 ModelClient 接口使用的字符串错误。
            .map_err(model_error_from_service_error)?;
            // 服务响应中包含的文本、工具调用和 usage 在这里还原成 Runner 使用的 ModelOutput。
            output_from_response(&response)
        }

        /// 按配置尝试原生 provider 流式，再把事件聚合为完整输出并逐段回调给 Runner。
        ///
        /// `Off` 总是退回单增量；`Auto` 仅在模型支持原生流式时开流；`On` 遇到不支持的模型立即报错。
        async fn complete_streaming(
            &self,
            request: ModelRequest,
            on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
        ) -> Result<ModelOutput, String> {
            // 能力来自当前 provider/model 的 registry profile，不能只根据环境开关推断支持情况。
            let profile = self.provider.model_profile(&self.model);
            match self.streaming_policy {
                StreamingPolicy::Off => {
                    // 兼容调用方仍可收到增量接口结果，但底层先完成整次非流式请求。
                    return self.complete_as_single_delta(request, on_delta).await;
                }
                StreamingPolicy::Auto if !profile.native_streaming => {
                    // 自动模式优先保证任务可运行；不支持原生流式时降级为一个完整文本增量。
                    return self.complete_as_single_delta(request, on_delta).await;
                }
                StreamingPolicy::On if !profile.native_streaming => {
                    // 显式 On 是调用约束，不能悄悄回退，否则 UI 观察到的流式语义会与配置不符。
                    return Err(unsupported_streaming_error(
                        self.provider.provider_id(),
                        &self.model,
                    ));
                }
                StreamingPolicy::Auto | StreamingPolicy::On => {}
            }

            // 原生流式与普通调用使用相同 DTO 转换及预算预检，避免只在某一入口绕过限制。
            let messages = map_messages(&request.messages)?;
            let tools = map_tools(&request.tools)?;
            let context = self.request_context(&request);
            context.budget().validate().map_err(str::to_owned)?;
            let system = Some(json!(context.system_prompt));
            // 每次重试都重新建立拥有数据的请求；拿到 ProviderStream 后才开始消费事件。
            let stream = with_retry(
                || {
                    let messages = messages.clone();
                    let tools = tools.clone();
                    let system = system.clone();
                    async move {
                        // `stream: Some(true)` 选择 provider 的事件流 API，而不是返回一份普通完整响应。
                        self.provider
                            .stream_message(MessagesRequest {
                                model: self.model.clone(),
                                messages,
                                max_tokens: DEFAULT_MAX_TOKENS,
                                system,
                                temperature: None,
                                tools: if tools.is_empty() { None } else { Some(tools) },
                                thinking: None,
                                stream: Some(true),
                            })
                            .await
                            .map_err(provider_error_to_service_error)
                    }
                },
                model_retry_config(),
            )
            .await
            .map_err(model_error_from_service_error)?;

            // 聚合器负责维护 block 索引、工具 JSON 和 usage，同时通过借用的回调向 Runner 发增量。
            aggregate_provider_stream(self.provider.provider_id(), &self.model, stream, on_delta)
                .await
        }
    }

    /// 构造测试 fixture 的统一重试参数，避免每次调用复制策略常量。
    fn model_retry_config() -> RetryConfig {
        RetryConfig {
            max_retries: MODEL_MAX_RETRIES,
            base_delay: MODEL_RETRY_BASE_DELAY,
        }
    }

    /// 把 provider 不支持原生流式转换为与普通 provider 错误相同的公开错误文本。
    fn unsupported_streaming_error(provider_id: &str, model_id: &str) -> String {
        model_error_from_service_error(provider_error_to_service_error(
            ProviderError::UnsupportedCapability {
                provider_id: provider_id.to_owned(),
                model_id: model_id.to_owned(),
                capability: "streaming".to_owned(),
                message: format!(
                    "model {provider_id}/{model_id} does not support native streaming"
                ),
            },
        ))
    }

    impl ProviderModelClient {
        /// 用普通完成请求模拟流式接口，并在文本非空时只发送一个完整文本 delta。
        async fn complete_as_single_delta(
            &self,
            request: ModelRequest,
            on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
        ) -> Result<ModelOutput, String> {
            // 复用 complete 的 DTO 映射、预算检查和重试语义，避免回退路径另造一套请求逻辑。
            let output = self.complete(request).await?;
            if !output.text.is_empty() {
                // 不发送空增量；回调错误按 `?` 立即返回并中断本次回退。
                on_delta(ModelDelta::Text {
                    text: output.text.clone(),
                })?;
            }
            Ok(output)
        }
    }

    /// 聚合 provider 事件时维护的单个内容块状态。
    enum StreamBlock {
        /// 已输出给 Runner 的文本块累计值，用于最终 `ModelOutput`。
        Text(String),
        /// 工具调用可能分多段到达，需保留调用身份和输入 JSON 缓冲区直到 block 完成。
        ToolUse {
            /// Provider 生成的调用 ID，之后随完整工具调用交给 Runner。
            id: String,
            /// Provider 返回的工具名称。
            name: String,
            /// block 开始事件携带的初始 JSON 值。
            initial_input: Value,
            /// 后续输入增量拼接形成的原始 JSON 文本。
            input_json: String,
        },
    }

    /// 消费 provider 的逐事件流，边向 Runner 发增量，边保存完整 block 供最终结果使用。
    ///
    /// `ProviderStream` 按序产生消息开始、内容块、增量和终止事件；block index 用来把交错到达的
    /// 文本与工具输入归回各自内容块。provider 错误、协议顺序错误、回调失败或不完整终止都会中止聚合。
    async fn aggregate_provider_stream(
        provider_id: &str,
        fallback_model_id: &str,
        mut stream: ProviderStream,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        // 用 provider 给出的 block index 保存正在组装的块；BTreeMap 使最终遍历按 index 稳定排序。
        let mut blocks = BTreeMap::<usize, StreamBlock>::new();
        // usage 和停止原因由不同事件补齐；若开流中途断掉，这些值也用于判断是否存在终态证据。
        let mut input_tokens = 0_u32;
        let mut output_tokens = 0_u32;
        let mut stop_reason = None;
        let mut message_stopped = false;
        // 某些 provider 未发送 MessageStart 时仍保留调用者选定的模型 ID 作为结果回退值。
        let mut model_id = Some(fallback_model_id.to_owned());

        // 每次 await 等待下一条异步事件；流结束由 None 表示，事件解码/传输错误则是 Err。
        while let Some(event) = next_provider_stream_event(&mut stream).await {
            // 先把 provider 层错误归一成 service/model 错误；`?` 会立刻结束本次聚合。
            match event.map_err(|error| {
                model_error_from_service_error(provider_error_to_service_error(error))
            })? {
                StreamEvent::MessageStart { message } => {
                    // MessageStart 提供本轮实际输入用量和 provider 确认的模型 ID。
                    input_tokens = message.usage.input_tokens;
                    model_id = Some(message.model);
                }
                StreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => match content_block {
                    StreamContentBlock::Text { text } => {
                        // 起始事件有时已带初始文本；先立即推给订阅者，再保留副本用于完整输出。
                        if !text.is_empty() {
                            on_delta(ModelDelta::Text { text: text.clone() })?;
                        }
                        blocks.insert(index, StreamBlock::Text(text));
                    }
                    StreamContentBlock::ToolUse(tool) => {
                        // 工具调用的参数可能后续以 InputJsonDelta 分片到达，因此此处只建状态槽。
                        blocks.insert(
                            index,
                            StreamBlock::ToolUse {
                                id: tool.id,
                                name: tool.name,
                                initial_input: tool.input,
                                input_json: String::new(),
                            },
                        );
                    }
                    // 旧 ModelOutput 不暴露思考块；保留接口兼容性但不把这些内容回灌给 Runner。
                    StreamContentBlock::Thinking { .. }
                    | StreamContentBlock::RedactedThinking { .. } => {}
                },
                StreamEvent::ContentBlockDelta { index, delta } => match delta {
                    StreamDelta::TextDelta { text } => {
                        // delta 必须对应已开启的文本 block；错序意味着流不完整或 provider 协议不匹配。
                        let Some(StreamBlock::Text(current)) = blocks.get_mut(&index) else {
                            return Err(format!("stream_text_delta_without_content_block:{index}"));
                        };
                        // 一份副本累积成最终输出，原始片段则马上通知 Runner；空片段不产生无意义事件。
                        current.push_str(&text);
                        if !text.is_empty() {
                            on_delta(ModelDelta::Text { text })?;
                        }
                    }
                    StreamDelta::InputJsonDelta { partial_json } => {
                        // 工具参数以文本片段累积，等 block 完成后再一次性解析 JSON。
                        let Some(StreamBlock::ToolUse { input_json, .. }) = blocks.get_mut(&index)
                        else {
                            return Err(format!("stream_tool_delta_without_content_block:{index}"));
                        };
                        input_json.push_str(&partial_json);
                    }
                    // 当前结果类型不承载思考正文或签名，因此不会把它们伪装成普通文本。
                    StreamDelta::ThinkingDelta { .. } | StreamDelta::SignatureDelta { .. } => {}
                },
                // block stop 不额外改变聚合数据；Ping 只是保活事件。
                StreamEvent::ContentBlockStop { .. } | StreamEvent::Ping => {}
                // MessageStop 与 MessageDelta 是不同事件：一个证明流结束，一个携带 token/stop 元数据。
                StreamEvent::MessageStop => message_stopped = true,
                StreamEvent::MessageDelta { delta, usage } => {
                    // 记录最新尾部用量和停止原因，供退出循环后的完整性判断与结果填充使用。
                    output_tokens = usage.output_tokens;
                    stop_reason = delta.stop_reason;
                }
                StreamEvent::Error { error } => {
                    // provider 的 Error 事件不是传输层 Err；从 JSON 载荷抽取稳定 code/message 并纳入统一错误映射。
                    let code = error
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("provider_stream_error");
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("provider stream error");
                    return Err(model_error_from_service_error(
                        provider_error_to_service_error(ProviderError::Provider {
                            provider_id: provider_id.to_owned(),
                            code: code.to_owned(),
                            message: message.to_owned(),
                        }),
                    ));
                }
            }
        }

        // 流正常结束时必须带终止信号（`message_stop`，或至少一个 `stop_reason`）。
        // 否则就是非 SSE 正文或被截断的流：宁可 fail-closed，也不能把它当成"成功且空白"。
        if !message_stopped && stop_reason.is_none() {
            return Err(model_error_from_service_error(ServiceError::Unknown(
                "provider_stream_incomplete".to_owned(),
            )));
        }

        // 终止证据齐全后才把 block 状态折叠成一次 ModelOutput；此时不再向 on_delta 发事件。
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        for (index, block) in blocks {
            match block {
                // 文本已经按增量发出，这里只做最终完整文本的拼接。
                StreamBlock::Text(chunk) => text.push_str(&chunk),
                StreamBlock::ToolUse {
                    id,
                    name,
                    initial_input,
                    input_json,
                } => {
                    // 没有参数增量时使用 block 起始事件的 JSON；否则必须把完整缓冲解析成合法 JSON。
                    let arguments = if input_json.trim().is_empty() {
                        initial_input
                    } else {
                        serde_json::from_str(&input_json)
                            .map_err(|error| format!("invalid_stream_tool_input:{index}:{error}"))?
                    };
                    // 内部工具名可能含点号，回到 Runner 前通过同一 wire 名称解码规则还原。
                    tool_calls.push(ModelToolCall {
                        id,
                        name: tool_name_from_wire(&name),
                        arguments,
                    });
                }
            }
        }

        // usage、stop_reason 和实际模型 ID 都来自流中的终态信息；content 由旧接口暂不承载，保持空数组。
        Ok(ModelOutput {
            text,
            tool_calls,
            usage: Some(ModelUsage {
                input_tokens: u64::from(input_tokens),
                output_tokens: u64::from(output_tokens),
            }),
            stop_reason,
            model_id,
            content: Vec::new(),
            continuation: None,
            structured: None,
        })
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ProviderRetryClass {
        /// 当前错误不应由旧 retry helper 自动重试。
        None,
        /// 请求超时，可在相同请求上重试。
        Timeout,
        /// 连接建立或传输失败，可在相同请求上重试。
        Connection,
        /// 服务商限流，等待重试间隔后再发送。
        RateLimit,
        /// 服务暂时过载/不可用，映射为 503 类错误后重试。
        ServerUnavailable,
    }

    /// 把 provider 领域错误转换为旧服务层的稳定错误类型，供 `with_retry` 和最终 ModelClient 错误共同使用。
    fn provider_error_to_service_error(error: ProviderError) -> ServiceError {
        // 先分类再保存原始文本，分类决定 retry helper 是否会再次执行请求。
        let retry_class = provider_error_retry_class(&error);
        let original = error.to_string();
        match retry_class {
            // 限流保留 RateLimit 语义，让上层能够区分配额问题与普通网络失败。
            ProviderRetryClass::RateLimit => ServiceError::RateLimit(original),
            // 旧 provider 的 overload 统一成 503，避免不同 provider code 直接泄漏到调用方。
            ProviderRetryClass::ServerUnavailable => ServiceError::Http {
                status: 503,
                body: original,
            },
            // timeout/connection 都属于可重试的连接类故障。
            ProviderRetryClass::Timeout | ProviderRetryClass::Connection => {
                ServiceError::Connection(original)
            }
            ProviderRetryClass::None => match error {
                // 能力不支持不是临时故障，保留明确 code 且不将其误判为网络重试。
                ProviderError::UnsupportedCapability { .. } => {
                    ServiceError::Unknown(error.code().to_owned())
                }
                _ => ServiceError::Unknown(original),
            },
        }
    }

    /// 根据结构化 API 错误或 provider 自定义 code/message 判断重试类别。
    ///
    /// 自定义 provider 可能只提供字符串，因此这里同时匹配规范 code、HTTP 数字和常见消息片段；
    /// 未命中的错误保持 `None`，避免把认证、参数和权限错误重复发送。
    fn provider_error_retry_class(error: &ProviderError) -> ProviderRetryClass {
        match error {
            // 旧服务 API 已有枚举，直接将各类映射到 retry policy。
            ProviderError::Api(error) => match error.kind {
                ApiErrorKind::RateLimit => ProviderRetryClass::RateLimit,
                ApiErrorKind::ApiTimeout => ProviderRetryClass::Timeout,
                ApiErrorKind::Repeated529 | ApiErrorKind::ServerOverload => {
                    ProviderRetryClass::ServerUnavailable
                }
                ApiErrorKind::ConnectionError | ApiErrorKind::SslCertError => {
                    ProviderRetryClass::Connection
                }
                _ => ProviderRetryClass::None,
            },
            ProviderError::Provider { code, message, .. } => {
                // 归一化大小写后按优先级匹配：限流/过载优先于 timeout/connection。
                let code = code.to_ascii_lowercase();
                let message = message.to_ascii_lowercase();
                if code.contains("rate_limit")
                    || code == "429"
                    || code.contains("too_many_requests")
                    || message.contains("429")
                    || message.contains("rate limit")
                    || message.contains("too many requests")
                {
                    ProviderRetryClass::RateLimit
                } else if code.contains("server_overload")
                    || code.contains("overloaded")
                    || code == "503"
                    || code.contains("503")
                    || message.contains("503")
                    || message.contains("service unavailable")
                    || message.contains("overloaded")
                {
                    ProviderRetryClass::ServerUnavailable
                } else if code.contains("timeout")
                    || code == "408"
                    || message.contains("timeout")
                    || message.contains("timed out")
                {
                    ProviderRetryClass::Timeout
                } else if code.contains("connection")
                    || code == "request_failed"
                    || message.contains("connection")
                    || message.contains("connect error")
                {
                    ProviderRetryClass::Connection
                } else {
                    ProviderRetryClass::None
                }
            }
            _ => ProviderRetryClass::None,
        }
    }

    /// 将服务层错误压平为旧 `ModelClient` 需要的字符串；可保留的分类只用于内容选择，不改变业务控制流。
    fn model_error_from_service_error(error: ServiceError) -> String {
        match error {
            // 这些变体已经携带适合展示和测试断言的原始 message。
            ServiceError::Unknown(message)
            | ServiceError::Auth(message)
            | ServiceError::RateLimit(message)
            | ServiceError::Connection(message) => message,
            ServiceError::Http { body, .. } => body,
            // 其他服务错误没有专门字段时使用其 Display 文本，避免丢失错误上下文。
            other => other.to_string(),
        }
    }

    /// 服务商的工具名只允许 `[A-Za-z0-9_-]`；Kiana 内部名里的点号在线协议上换成下划线。
    /// 内部名始终是唯一真源，映射只发生在这条边界上。
    const WIRE_TOOL_NAME_MAP: &[(&str, &str)] = &[
        ("memory.search", "memory_search"),
        ("memory.write", "memory_write"),
    ];

    /// 把内部工具名转换为 provider 接受的 wire 名称；未列出的合法名称保持原样。
    fn tool_name_to_wire(name: &str) -> String {
        // 映射表是双向边界的唯一来源，避免在多个转换点分别写替换规则。
        WIRE_TOOL_NAME_MAP
            .iter()
            .find(|(internal, _)| *internal == name)
            .map(|(_, wire)| (*wire).to_owned())
            .unwrap_or_else(|| name.to_owned())
    }

    /// 将 provider 返回的工具名还原成 Runner 使用的内部 canonical 名称。
    fn tool_name_from_wire(name: &str) -> String {
        WIRE_TOOL_NAME_MAP
            .iter()
            .find(|(_, wire)| *wire == name)
            .map(|(internal, _)| (*internal).to_owned())
            .unwrap_or_else(|| name.to_owned())
    }

    /// 将内部工具 schema 转成旧 provider 的工具 DTO，并在发送前校验名称存在。
    ///
    /// 同时兼容 `input_schema` 和旧 `parameters` 字段；两者都缺失时使用空 object schema，
    /// 这样 provider 能收到结构合法的定义，而不会因为可选描述字段缺失而 panic。
    fn map_tools(tools: &[Value]) -> Result<Vec<Value>, String> {
        tools
            .iter()
            .map(|tool| {
                // 工具名是 provider 调用关联的关键字段，缺失或空白时拒绝整个请求。
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| "model_tool_schema_name_missing".to_owned())?;
                // 描述是可选展示字段，不影响 schema 语义；缺失时发送空字符串。
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                // 新旧 schema 字段兼容，最终统一输出 provider 需要的 input_schema。
                let parameters = tool
                    .get("input_schema")
                    .or_else(|| tool.get("parameters"))
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                Ok(json!({
                    "name": tool_name_to_wire(name),
                    "description": description,
                    "input_schema": parameters,
                }))
            })
            .collect()
    }

    /// 将四种内部消息角色映射到 provider 消息协议。
    ///
    /// system 消息由请求上下文单独发送；user/assistant 保留文本与工具块，tool 结果按 Anthropic
    /// 兼容格式作为 user 消息回灌。无法跨 provider 表达的附件、opaque block 或角色错配会在网络请求前拒绝。
    fn map_messages(messages: &[ModelMessage]) -> Result<Vec<Message>, String> {
        let mut mapped = Vec::new();
        for message in messages {
            // content_blocks 在这里统一验证消息内部 block 形状，再按角色解释其语义。
            let blocks = message
                .content_blocks()
                .map_err(|error| error.to_string())?;
            match message.role {
                // system prompt 已由 ModelRequestContext 单独填入 MessagesRequest.system。
                ModelRole::System => {}
                ModelRole::User => {
                    let mut text = String::new();
                    for block in blocks {
                        match block {
                            // 多个文本块按原顺序合并为旧 provider 的单一 user content。
                            ModelContent::Text { text: value } => text.push_str(&value),
                            // 旧 provider 夹具不能表达附件引用，必须在发送前显式失败。
                            ModelContent::AttachmentRef { .. } => {
                                return Err(
                                    "unsupported_content_block_fails_before_request".to_owned()
                                )
                            }
                            // opaque 数据只允许留在 provider 专用边界，不能跨协议猜测其结构。
                            ModelContent::ProviderOpaque { .. } => {
                                return Err("opaque_item_cannot_cross_provider".to_owned())
                            }
                            // user 角色出现 tool/text 之外的 block，说明上游消息状态非法。
                            _ => return Err("model_content_role_invalid".to_owned()),
                        }
                    }
                    mapped.push(Message {
                        role: "user".to_owned(),
                        content: json!(text),
                    });
                }
                ModelRole::Assistant => {
                    let mut content = Vec::new();
                    for block in blocks {
                        match block {
                            // assistant 文本使用 provider 的 content block 数组表达。
                            ModelContent::Text { text } => {
                                content.push(json!({ "type": "text", "text": text }));
                            }
                            // 工具调用名在此跨 wire 边界转换，参数保持 JSON object。
                            ModelContent::ToolCall { call } => {
                                content.push(json!({
                                    "type": "tool_use",
                                    "id": call.id,
                                    "name": tool_name_to_wire(&call.name),
                                    "input": call.arguments,
                                }));
                            }
                            // 这些 block 没有旧协议的安全表达，不能静默丢弃。
                            ModelContent::AttachmentRef { .. } => {
                                return Err(
                                    "unsupported_content_block_fails_before_request".to_owned()
                                )
                            }
                            ModelContent::ProviderOpaque { .. } => {
                                return Err("opaque_item_cannot_cross_provider".to_owned())
                            }
                            ModelContent::ToolResult { .. } => {
                                return Err("model_content_role_invalid".to_owned())
                            }
                        }
                    }
                    if content.is_empty() {
                        // provider 要求 assistant content 非空时补空文本 block，保持消息角色仍可见。
                        content.push(json!({ "type": "text", "text": "" }));
                    }
                    mapped.push(Message {
                        role: "assistant".to_owned(),
                        content: Value::Array(content),
                    });
                }
                ModelRole::Tool => {
                    // tool 消息必须正好由一个 ToolResult block 构成，并带回原始 call_id。
                    let ModelContent::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } = blocks
                        .into_iter()
                        .next()
                        .ok_or_else(|| "tool_result_missing_call_id".to_owned())?
                    else {
                        return Err("tool_result_missing_call_id".to_owned());
                    };
                    // 兼容 provider 把工具结果放在 user 消息的 tool_result content 中。
                    mapped.push(Message {
                        role: "user".to_owned(),
                        content: json!([{
                            "type": "tool_result",
                            "tool_use_id": call_id,
                            "content": content,
                            "is_error": is_error,
                        }]),
                    });
                }
            }
        }
        Ok(mapped)
    }

    /// 把 provider 完整响应解析为 Runner 的文本、工具调用、usage 和终止信息。
    ///
    /// 工具块必须有唯一 ID、非空名称和 object 参数；未知内容块被拒绝，防止响应中的未支持语义
    /// 被错误地当作成功结果继续执行。
    fn output_from_response(response: &MessagesResponse) -> Result<ModelOutput, String> {
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        let mut seen_tool_ids = std::collections::HashSet::new();
        for block in &response.content {
            // provider 返回的 content block 由 type 分派到文本或工具解析器。
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    // 缺少 text 字段的文本块不增加输出，但仍属于可识别 block。
                    if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                        text.push_str(chunk);
                    }
                }
                Some("tool_use") => {
                    // ID/name 是后续 capability 请求关联的身份；任一缺失都不能安全执行工具。
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.trim().is_empty())
                        .ok_or_else(|| "missing_native_tool_identity".to_owned())?
                        .to_owned();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|name| !name.trim().is_empty())
                        .ok_or_else(|| "provider_tool_name_missing".to_owned())?
                        .to_owned();
                    // 参数必须是 object，避免把标量/数组直接当作工具参数透传。
                    let arguments = block
                        .get("input")
                        .filter(|input| input.is_object())
                        .cloned()
                        .ok_or_else(|| "provider_tool_arguments_invalid".to_owned())?;
                    // 同一响应中重复 ID 会破坏调用关联，即使 payload 相同也拒绝。
                    if !seen_tool_ids.insert(id.clone()) {
                        return Err("duplicate_tool_id_with_different_payload".to_owned());
                    }
                    tool_calls.push(ModelToolCall {
                        id,
                        name: tool_name_from_wire(&name),
                        arguments,
                    });
                }
                // provider 扩展 block 尚未纳入该夹具协议，保持 fail closed。
                _ => return Err("provider_content_block_unsupported".to_owned()),
            }
        }
        // usage/stop/model 由 provider 的响应元数据直接携带；兼容 ModelOutput 的其它容器保持空值。
        Ok(ModelOutput {
            text,
            tool_calls,
            usage: Some(ModelUsage {
                input_tokens: u64::from(response.usage.input_tokens),
                output_tokens: u64::from(response.usage.output_tokens),
            }),
            stop_reason: response.stop_reason.clone(),
            model_id: Some(response.model.clone()),
            content: Vec::new(),
            continuation: None,
            structured: None,
        })
    }

    #[cfg(test)]
    mod tests {
        //! 旧模型适配器的离线行为测试。
        //!
        //! 测试通过环境变量 guard、内存 provider 和本地临时 HTTP server 构造 cassette/provider
        //! 场景，不依赖真实模型凭据。每个测试先锁住进程环境，再在 guard drop 时恢复变量。
        use super::*;
        use async_trait::async_trait;
        use kiana_services::api::messages::{MessagesResponse, Usage};
        use kiana_services::api::provider::{
            provider_stream_from_events, FakeProviderStep, ModelProfile, ProviderResult,
            StreamingMode, FAKE_TEXT_ONLY_MODEL_ID,
        };
        use std::ffi::{OsStr, OsString};
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::Mutex;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Rust 测试线程共享进程环境；全局锁避免并行测试互相覆盖 KIANA_* 变量。
        static ENV_LOCK: Mutex<()> = Mutex::new(());

        /// 获取环境变量测试锁；毒化锁时仍取回内部 guard，避免一次 panic 让后续测试全部失效。
        fn env_lock() -> std::sync::MutexGuard<'static, ()> {
            ENV_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        /// 保存单个环境变量的旧值，并在离开测试作用域时恢复它。
        struct EnvGuard {
            /// 要恢复的固定环境变量名。
            key: &'static str,
            /// 变量原值；None 表示测试前不存在该变量。
            previous: Option<OsString>,
        }

        impl EnvGuard {
            /// 设置变量并记录旧值；返回的 guard 必须保持存活到测试结束。
            fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
                let previous = std::env::var_os(key);
                std::env::set_var(key, value);
                Self { key, previous }
            }

            /// 删除变量并记录旧值，用于验证缺省/不可用路径。
            fn remove(key: &'static str) -> Self {
                let previous = std::env::var_os(key);
                std::env::remove_var(key);
                Self { key, previous }
            }
        }

        impl Drop for EnvGuard {
            /// 按测试前的存在性恢复变量；drop 本身不传播环境操作错误。
            fn drop(&mut self) {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }

        /// 为脚本模型测试创建并自动清理的临时 JSON 文件。
        struct TempScript {
            /// 文件系统中的 cassette 路径。
            path: PathBuf,
        }

        impl TempScript {
            /// 使用进程 ID 与单调递增序号生成近似唯一文件名，再写入脚本内容。
            fn new(contents: &str) -> Self {
                static NEXT_ID: AtomicU64 = AtomicU64::new(0);
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "kiana-model-client-test-{}-{id}.json",
                    std::process::id()
                ));
                std::fs::write(&path, contents).expect("write temporary harness script");
                Self { path }
            }

            /// 返回 cassette 的借用路径，供环境变量使用。
            fn path(&self) -> &Path {
                &self.path
            }
        }

        impl Drop for TempScript {
            /// 测试结束后尽力删除临时文件；删除失败不掩盖主测试结果。
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.path);
            }
        }

        /// 记录是否调用过原生流式 API 的最小 provider mock。
        struct TrackingProvider {
            /// 控制模型资料是否声明 native streaming。
            native_streaming: bool,
            /// 流式入口被调用时置为 true；回退测试还会让该入口 panic。
            stream_called: Arc<AtomicBool>,
        }

        #[async_trait]
        impl Provider for TrackingProvider {
            /// mock provider 的稳定身份。
            fn provider_id(&self) -> &str {
                "tracking"
            }

            /// 返回带有可切换流式能力的固定模型资料。
            fn model_profile(&self, model_id: &str) -> ModelProfile {
                ModelProfile {
                    provider_id: "tracking".to_owned(),
                    provider_display_name: "Tracking".to_owned(),
                    model_id: model_id.to_owned(),
                    supports_tools: true,
                    supports_streaming: true,
                    streaming_mode: if self.native_streaming {
                        StreamingMode::Native
                    } else {
                        StreamingMode::Synthetic
                    },
                    native_streaming: self.native_streaming,
                    supports_vision: false,
                    supports_structured_output: false,
                    context_window: 8_192,
                }
            }

            /// 返回固定文本，供非流式回退断言。
            async fn create_message(
                &self,
                request: MessagesRequest,
            ) -> ProviderResult<MessagesResponse> {
                Ok(MessagesResponse {
                    id: "tracking-msg".to_owned(),
                    model: request.model,
                    role: "assistant".to_owned(),
                    content: vec![json!({
                        "type": "text",
                        "text": "fallback text",
                    })],
                    stop_reason: Some("end_turn".to_owned()),
                    usage: Usage {
                        input_tokens: 4,
                        output_tokens: 2,
                    },
                })
            }

            /// 回退路径不应触达该方法；调用它说明流式能力判断失效。
            async fn stream_message(
                &self,
                _request: MessagesRequest,
            ) -> ProviderResult<ProviderStream> {
                self.stream_called.store(true, Ordering::SeqCst);
                panic!("stream_message must not be called when native streaming is unavailable");
            }
        }

        /// 把预制 `StreamEvent` 序列包装为 provider stream 的 mock。
        struct EventProvider {
            /// 测试希望聚合器按顺序消费的事件。
            events: Vec<StreamEvent>,
        }

        #[async_trait]
        impl Provider for EventProvider {
            /// mock provider 的稳定身份。
            fn provider_id(&self) -> &str {
                "event-provider"
            }

            /// 始终声明可原生流式，确保测试进入聚合器。
            fn model_profile(&self, model_id: &str) -> ModelProfile {
                ModelProfile {
                    provider_id: "event-provider".to_owned(),
                    provider_display_name: "Event Provider".to_owned(),
                    model_id: model_id.to_owned(),
                    supports_tools: true,
                    supports_streaming: true,
                    streaming_mode: StreamingMode::Native,
                    native_streaming: true,
                    supports_vision: false,
                    supports_structured_output: false,
                    context_window: 8_192,
                }
            }

            /// 非流式入口不应使用；panic 能让测试明确发现错误路径。
            async fn create_message(
                &self,
                _request: MessagesRequest,
            ) -> ProviderResult<MessagesResponse> {
                panic!("event provider must only be used through the streaming path");
            }

            /// 将预制事件转换成异步 provider stream。
            async fn stream_message(
                &self,
                _request: MessagesRequest,
            ) -> ProviderResult<ProviderStream> {
                Ok(provider_stream_from_events(self.events.clone()))
            }
        }

        /// 第一次返回临时错误、之后返回事件流的 provider，用于验证重试发生在首个 delta 之前。
        struct RetryingStreamProvider {
            /// 已调用次数；原子计数保证 async 重试闭包可共享。
            attempts: Arc<AtomicU64>,
            /// 第二次尝试返回的事件序列。
            events: Vec<StreamEvent>,
        }

        #[async_trait]
        impl Provider for RetryingStreamProvider {
            /// mock provider 的稳定身份。
            fn provider_id(&self) -> &str {
                "retrying-stream-provider"
            }

            /// 声明原生流式能力，使测试走 stream_message。
            fn model_profile(&self, model_id: &str) -> ModelProfile {
                ModelProfile {
                    provider_id: "retrying-stream-provider".to_owned(),
                    provider_display_name: "Retrying Stream Provider".to_owned(),
                    model_id: model_id.to_owned(),
                    supports_tools: true,
                    supports_streaming: true,
                    streaming_mode: StreamingMode::Native,
                    native_streaming: true,
                    supports_vision: false,
                    supports_structured_output: false,
                    context_window: 8_192,
                }
            }

            /// 此 mock 专用于流式测试，不允许走普通完成路径。
            async fn create_message(
                &self,
                _request: MessagesRequest,
            ) -> ProviderResult<MessagesResponse> {
                panic!("retrying stream provider must not use the non-streaming fallback");
            }

            /// 首次调用产生可重试 overload，随后返回同一事件流。
            async fn stream_message(
                &self,
                _request: MessagesRequest,
            ) -> ProviderResult<ProviderStream> {
                let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return Err(ProviderError::Provider {
                        provider_id: self.provider_id().to_owned(),
                        code: "server_overload".to_owned(),
                        message: "retry before first delta".to_owned(),
                    });
                }
                Ok(provider_stream_from_events(self.events.clone()))
            }
        }

        /// 用指定流式能力和策略组装 `ProviderModelClient`，并返回调用观测标记。
        fn tracking_client(
            native_streaming: bool,
            streaming_policy: StreamingPolicy,
        ) -> (ProviderModelClient, Arc<AtomicBool>) {
            let stream_called = Arc::new(AtomicBool::new(false));
            let client = ProviderModelClient {
                provider: Box::new(TrackingProvider {
                    native_streaming,
                    stream_called: Arc::clone(&stream_called),
                }),
                model: "tracking-model".to_owned(),
                streaming_policy,
            };
            (client, stream_called)
        }

        #[test]
        /// 验证 Anthropic 没有密钥时返回 None，补充密钥后才构造 provider client。
        fn provider_env_anthropic_with_key_selects_provider_client() {
            let _lock = env_lock();
            let _script = EnvGuard::remove(ENV_HARNESS_SCRIPT);
            let _provider = EnvGuard::set(ENV_PROVIDER, ANTHROPIC_PROVIDER_ID);

            let without_key = provider_from_env(LocalModelConfig::default())
                .expect("anthropic provider configuration should be valid");
            assert!(
                without_key.is_none(),
                "anthropic without ANTHROPIC_API_KEY should not select a provider client"
            );

            let _key = EnvGuard::set("ANTHROPIC_API_KEY", "test-key");
            let with_key = provider_from_env(LocalModelConfig::default())
                .expect("anthropic provider configuration should be valid");
            assert!(
                with_key.is_some(),
                "KIANA_PROVIDER=anthropic with ANTHROPIC_API_KEY should select a provider client"
            );
        }

        #[test]
        /// 验证流式环境值支持大小写不敏感的 off/auto/on 别名。
        fn streaming_env_parses_off_auto_and_on_case_insensitively() {
            let _lock = env_lock();
            let _streaming = EnvGuard::remove(ENV_STREAMING);
            assert_eq!(streaming_enabled_from_env(), StreamingPolicy::Auto);

            for value in ["off", "0", "FALSE", "No"] {
                let _streaming = EnvGuard::set(ENV_STREAMING, value);
                assert_eq!(
                    streaming_enabled_from_env(),
                    StreamingPolicy::Off,
                    "KIANA_STREAMING={value}"
                );
            }

            for value in ["on", "1", "TRUE", "Yes"] {
                let _streaming = EnvGuard::set(ENV_STREAMING, value);
                assert_eq!(
                    streaming_enabled_from_env(),
                    StreamingPolicy::On,
                    "KIANA_STREAMING={value}"
                );
            }

            for value in ["auto", "", "unexpected"] {
                let _streaming = EnvGuard::set(ENV_STREAMING, value);
                assert_eq!(
                    streaming_enabled_from_env(),
                    StreamingPolicy::Auto,
                    "KIANA_STREAMING={value}"
                );
            }
        }

        #[tokio::test]
        /// 未注册 provider 名称必须 fail closed，而不是回退到默认 provider。
        async fn provider_env_unknown_name_fails_closed() {
            let _lock = env_lock();
            let _script = EnvGuard::remove(ENV_HARNESS_SCRIPT);
            let _provider = EnvGuard::set(ENV_PROVIDER, "definitely-unknown");

            let error = provider_from_env(LocalModelConfig::default())
                .err()
                .expect("unknown provider should return an error");
            assert_eq!(error, "unknown_provider:definitely-unknown");

            let client = from_config(LocalModelConfig::default());
            let error = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                })
                .await
                .unwrap_err();
            assert!(
                error.starts_with("model_unavailable: unknown_provider:definitely-unknown"),
                "{error}"
            );
        }

        #[tokio::test]
        /// cassette 脚本存在时优先于 provider 环境配置，并能在离线条件下完成一次请求。
        async fn harness_script_takes_precedence_over_provider_env() {
            let _lock = env_lock();
            let script = TempScript::new(r#"[{"text":"from cassette"}]"#);
            let _script = EnvGuard::set(ENV_HARNESS_SCRIPT, script.path().as_os_str());
            let _provider = EnvGuard::set(ENV_PROVIDER, "definitely-unknown");

            let client = from_config(LocalModelConfig::default());
            let output = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                })
                .await
                .expect("KIANA_HARNESS_SCRIPT should be selected before provider validation");

            assert_eq!(output.text, "from cassette");
            assert!(output.tool_calls.is_empty());
        }

        #[tokio::test]
        /// Off 策略通过普通完成接口生成单个文本增量。
        async fn streaming_switch_off_uses_non_streaming_fallback() {
            let (client, stream_called) = tracking_client(true, StreamingPolicy::Off);
            let mut deltas = Vec::new();

            let output = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("hello")],
                        tools: Vec::new(),
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |delta| {
                        deltas.push(delta);
                        Ok(())
                    },
                )
                .await
                .expect("switch off should use the non-streaming path");

            assert_eq!(output.text, "fallback text");
            assert_eq!(
                deltas,
                vec![ModelDelta::Text {
                    text: "fallback text".to_owned()
                }]
            );
            assert!(
                !stream_called.load(Ordering::SeqCst),
                "native stream_message must not be called when KIANA_STREAMING is off"
            );
        }

        #[tokio::test]
        /// Auto 策略遇到不支持原生流式的模型时回退，不调用 stream_message。
        async fn streaming_auto_uses_non_streaming_fallback_for_non_native_provider() {
            let (client, stream_called) = tracking_client(false, StreamingPolicy::Auto);
            let mut deltas = Vec::new();

            let output = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("hello")],
                        tools: Vec::new(),
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |delta| {
                        deltas.push(delta);
                        Ok(())
                    },
                )
                .await
                .expect("synthetic-only providers should use the non-streaming fallback");

            assert_eq!(output.text, "fallback text");
            assert_eq!(
                deltas,
                vec![ModelDelta::Text {
                    text: "fallback text".to_owned()
                }]
            );
            assert!(
                !stream_called.load(Ordering::SeqCst),
                "synthetic providers must not be reported as native streaming"
            );
        }

        #[tokio::test]
        /// On 策略遇到不支持原生流式的模型时返回能力错误而不是静默回退。
        async fn streaming_on_fails_closed_for_non_native_provider() {
            let (client, stream_called) = tracking_client(false, StreamingPolicy::On);

            let error = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("hello")],
                        tools: Vec::new(),
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |_| Ok(()),
                )
                .await
                .expect_err("forced streaming must not silently fall back");

            assert_eq!(error, "unsupported_streaming");
            assert!(
                !stream_called.load(Ordering::SeqCst),
                "non-native providers must be rejected before stream_message is called"
            );
        }

        #[tokio::test]
        /// 环境要求 On 时，fake provider 不具备原生流式能力；Auto 则可回退完成。
        async fn streaming_env_on_rejects_fake_provider_and_auto_falls_back() {
            let _lock = env_lock();
            let _script = EnvGuard::remove(ENV_HARNESS_SCRIPT);
            let _provider = EnvGuard::set(ENV_PROVIDER, FAKE_PROVIDER_ID);
            let _fake_script = EnvGuard::set(
                "KIANA_FAKE_PROVIDER_SCRIPT",
                r#"[{"type":"assistant_text","text":"fallback text"}]"#,
            );
            let config = LocalModelConfig {
                model: Some(FAKE_TEXT_ONLY_MODEL_ID.to_owned()),
                ..LocalModelConfig::default()
            };

            let _streaming = EnvGuard::set(ENV_STREAMING, "on");
            let on_client = from_config(config.clone());
            let error = on_client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("hello")],
                        tools: Vec::new(),
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |_| Ok(()),
                )
                .await
                .expect_err("KIANA_STREAMING=on must not silently fall back");
            assert_eq!(error, "unsupported_streaming");

            let _streaming = EnvGuard::set(ENV_STREAMING, "auto");
            let auto_client = from_config(config);
            let mut deltas = Vec::new();
            let output = auto_client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("hello")],
                        tools: Vec::new(),
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |delta| {
                        deltas.push(delta);
                        Ok(())
                    },
                )
                .await
                .expect("auto must fall back to the complete response");
            assert_eq!(output.text, "fallback text");
            assert_eq!(
                deltas,
                vec![ModelDelta::Text {
                    text: "fallback text".to_owned(),
                }]
            );
        }

        #[tokio::test]
        /// 临时 provider 错误应在首个 delta 之前重试，成功后只向回调发送一次事件序列。
        async fn streaming_retries_transient_connection_before_first_delta() {
            let event = |value: Value| serde_json::from_value::<StreamEvent>(value).unwrap();
            let attempts = Arc::new(AtomicU64::new(0));
            let client = ProviderModelClient {
                provider: Box::new(RetryingStreamProvider {
                    attempts: Arc::clone(&attempts),
                    events: vec![
                        event(json!({
                            "type": "message_start",
                            "message": {
                                "id": "msg_retry",
                                "model": "retry-model",
                                "role": "assistant",
                                "usage": {"input_tokens": 4, "output_tokens": 0}
                            }
                        })),
                        event(json!({
                            "type": "content_block_start",
                            "index": 0,
                            "content_block": {"type": "text", "text": ""}
                        })),
                        event(json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": {"type": "text_delta", "text": "retried stream"}
                        })),
                        event(json!({
                            "type": "message_delta",
                            "delta": {"stop_reason": "end_turn"},
                            "usage": {"input_tokens": 0, "output_tokens": 2}
                        })),
                        event(json!({"type": "message_stop"})),
                    ],
                }),
                model: "retry-model".to_owned(),
                streaming_policy: StreamingPolicy::Auto,
            };
            let mut deltas = Vec::new();

            let output = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("hello")],
                        tools: Vec::new(),
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |delta| {
                        deltas.push(delta);
                        Ok(())
                    },
                )
                .await
                .expect("transient stream connection failures should be retried");

            assert_eq!(attempts.load(Ordering::SeqCst), 2);
            assert_eq!(output.text, "retried stream");
            assert_eq!(
                deltas,
                vec![ModelDelta::Text {
                    text: "retried stream".to_owned()
                }]
            );
        }

        #[tokio::test]
        /// 验证原生流式中的工具输入 JSON 分片可以聚合为一个工具调用。
        async fn native_streaming_aggregates_input_json_delta_without_network() {
            let event = |value: Value| serde_json::from_value::<StreamEvent>(value).unwrap();
            let events = vec![
                event(json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_event",
                        "model": "event-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 9, "output_tokens": 0}
                    }
                })),
                event(json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text", "text": ""}
                })),
                event(json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": "streamed"}
                })),
                event(json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_event",
                        "name": "shell",
                        "input": {}
                    }
                })),
                event(json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {"type": "input_json_delta", "partial_json": "{\"command\":"}
                })),
                event(json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {"type": "input_json_delta", "partial_json": "\"pwd\"}"}
                })),
                event(json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"input_tokens": 0, "output_tokens": 5}
                })),
                event(json!({"type": "message_stop"})),
            ];
            let client = ProviderModelClient {
                provider: Box::new(EventProvider { events }),
                model: "event-model".to_owned(),
                streaming_policy: StreamingPolicy::On,
            };
            let mut deltas = Vec::new();

            let output = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("run pwd")],
                        tools: vec![json!({"name": "shell"})],
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |delta| {
                        deltas.push(delta);
                        Ok(())
                    },
                )
                .await
                .expect("decoded native events should aggregate");

            assert_eq!(output.text, "streamed");
            assert_eq!(
                output.tool_calls,
                vec![ModelToolCall {
                    id: "toolu_event".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({"command": "pwd"}),
                }]
            );
            assert_eq!(
                output.usage,
                Some(ModelUsage {
                    input_tokens: 9,
                    output_tokens: 5,
                })
            );
            assert_eq!(output.stop_reason.as_deref(), Some("tool_use"));
            assert_eq!(output.model_id.as_deref(), Some("event-model"));
            assert_eq!(
                deltas,
                vec![ModelDelta::Text {
                    text: "streamed".to_owned()
                }]
            );
        }

        #[tokio::test]
        /// 缺少 MessageStop/stop_reason 的截断流必须被拒绝。
        async fn native_streaming_without_a_terminal_event_fails_closed() {
            let event = |value: Value| serde_json::from_value::<StreamEvent>(value).unwrap();
            // 端点回 200 但正文不是 SSE，或流在中途被截断：解析结果只有开头，没有 message_stop。
            // 这种"成功但空白"的答案必须变成机器可读的错误。
            let client = ProviderModelClient {
                provider: Box::new(EventProvider {
                    events: vec![event(json!({
                        "type": "message_start",
                        "message": {
                            "id": "msg_truncated",
                            "model": "event-model",
                            "role": "assistant",
                            "usage": {"input_tokens": 9, "output_tokens": 0}
                        }
                    }))],
                }),
                model: "event-model".to_owned(),
                streaming_policy: StreamingPolicy::On,
            };

            let error = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("run pwd")],
                        tools: vec![json!({"name": "shell"})],
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |_| Ok(()),
                )
                .await
                .expect_err("a stream without a terminal event must fail closed");

            assert_eq!(error, "provider_stream_incomplete");
        }

        #[tokio::test]
        /// 工具输入分片不是合法 JSON 时，聚合器返回解析错误而不执行工具。
        async fn native_streaming_invalid_tool_json_fails_closed() {
            let event = |value: Value| serde_json::from_value::<StreamEvent>(value).unwrap();
            let events = vec![
                event(json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_bad_tool",
                        "model": "event-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 3, "output_tokens": 0}
                    }
                })),
                event(json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_bad",
                        "name": "shell",
                        "input": {}
                    }
                })),
                event(json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": "{\"command\":"}
                })),
                event(json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"input_tokens": 0, "output_tokens": 2}
                })),
            ];
            let client = ProviderModelClient {
                provider: Box::new(EventProvider { events }),
                model: "event-model".to_owned(),
                streaming_policy: StreamingPolicy::On,
            };

            let error = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("run shell")],
                        tools: vec![json!({"name": "shell"})],
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |_| Ok(()),
                )
                .await
                .expect_err("invalid streamed tool JSON must fail closed");

            assert!(error.starts_with("invalid_stream_tool_input:0:"), "{error}");
        }

        #[tokio::test]
        /// 验证普通 provider 响应中的工具调用与最终文本能映射成 ModelOutput。
        async fn provider_wrapper_maps_tool_calls_and_final_text() {
            let provider = FakeProvider::new(
                "fake-model".to_owned(),
                vec![
                    FakeProviderStep::ToolCall {
                        id: Some("c1".to_owned()),
                        name: "shell".to_owned(),
                        input: json!({ "command": "ls" }),
                        text: Some("running ls".to_owned()),
                    },
                    FakeProviderStep::FinalAnswer {
                        text: "architecture mapped".to_owned(),
                    },
                ],
            );
            let client = ProviderModelClient {
                provider: Box::new(provider),
                model: "fake-model".to_owned(),
                streaming_policy: StreamingPolicy::Off,
            };
            let first = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user("map it")],
                    tools: vec![json!({"name": "shell", "parameters": {"type": "object"}})],
                    sandbox: "read-only".to_owned(),
                })
                .await
                .unwrap();
            assert_eq!(first.text, "running ls");
            assert_eq!(first.tool_calls[0].id, "c1");
            assert_eq!(first.tool_calls[0].name, "shell");
            assert_eq!(first.stop_reason.as_deref(), Some("tool_use"));
            assert_eq!(first.model_id.as_deref(), Some("fake-model"));
            assert_eq!(
                first.usage,
                Some(ModelUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                })
            );

            let second = client
                .complete(ModelRequest {
                    messages: vec![
                        ModelMessage::user("map it"),
                        ModelMessage::assistant_with_tools("running ls", first.tool_calls.clone()),
                        ModelMessage::tool("c1", r#"{"stdout":"listed"}"#),
                    ],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                })
                .await
                .unwrap();
            assert_eq!(second.text, "architecture mapped");
            assert!(second.tool_calls.is_empty());
            assert_eq!(second.stop_reason.as_deref(), Some("end_turn"));
        }

        #[test]
        fn legacy_tool_result_mapping_preserves_identity_and_refuses_missing_call_id() {
            let message = ModelMessage::tool("c1", r#"{"stdout":"listed"}"#);
            let mapped = map_messages(std::slice::from_ref(&message)).unwrap();
            assert_eq!(mapped.len(), 1);
            assert_eq!(mapped[0].role, "user");
            assert_eq!(
                mapped[0].content,
                json!([{
                    "type": "tool_result",
                    "tool_use_id": "c1",
                    "content": r#"{"stdout":"listed"}"#,
                    "is_error": false,
                }])
            );
            let mut missing_identity = message;
            missing_identity.tool_call_id = None;
            assert_eq!(
                map_messages(&[missing_identity]).unwrap_err(),
                "model_history_orphan_tool_result"
            );
        }

        #[tokio::test]
        /// 使用本地事件流验证 Anthropic 原生流式文本、工具和 usage 聚合，无需网络。
        async fn anthropic_native_streaming_aggregates_output_without_network() {
            let stream_body = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_stream","model":"claude-stream-test","role":"assistant","usage":{"input_tokens":12,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello "}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"shell","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"ls\"}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":1}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":0,"output_tokens":7}}

event: message_stop
data: {"type":"message_stop"}

"#;
            let (base_url, mut request_rx, server) =
                start_mock_anthropic_stream_server(stream_body.to_owned()).await;
            let provider =
                AnthropicProvider::new("test-key".to_owned(), base_url, Duration::from_secs(5));
            let client = ProviderModelClient {
                provider: Box::new(provider),
                model: "claude-stream-test".to_owned(),
                streaming_policy: StreamingPolicy::On,
            };
            let mut deltas = Vec::new();

            let output = client
                .complete_streaming(
                    ModelRequest {
                        messages: vec![ModelMessage::user("say hello")],
                        tools: vec![json!({
                            "name": "shell",
                            "input_schema": {"type": "object"}
                        })],
                        sandbox: "read-only".to_owned(),
                    },
                    &mut |delta| {
                        deltas.push(delta);
                        Ok(())
                    },
                )
                .await
                .expect("native Anthropic SSE should aggregate");

            assert_eq!(output.text, "Hello world");
            assert_eq!(
                output.tool_calls,
                vec![ModelToolCall {
                    id: "toolu_1".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({"command": "ls"}),
                }]
            );
            assert_eq!(
                output.usage,
                Some(ModelUsage {
                    input_tokens: 12,
                    output_tokens: 7,
                })
            );
            assert_eq!(output.stop_reason.as_deref(), Some("tool_use"));
            assert_eq!(output.model_id.as_deref(), Some("claude-stream-test"));
            assert_eq!(
                deltas,
                vec![
                    ModelDelta::Text {
                        text: "Hello ".to_owned()
                    },
                    ModelDelta::Text {
                        text: "world".to_owned()
                    }
                ]
            );

            let request = request_rx
                .recv()
                .await
                .expect("mock server should receive the streaming request");
            assert!(request.starts_with("POST /v1/messages "));
            assert!(request.contains("\"stream\":true"));
            assert!(request.contains("\"name\":\"shell\""));
            server.await.unwrap();
        }

        #[tokio::test]
        /// 文本模型收到工具请求时返回 provider 的 unsupported-tools 错误。
        async fn text_only_provider_maps_unsupported_tools_code() {
            let provider = FakeProvider::new(
                FAKE_TEXT_ONLY_MODEL_ID.to_owned(),
                vec![FakeProviderStep::AssistantText {
                    text: "should not be consumed".to_owned(),
                }],
            );
            let client = ProviderModelClient {
                provider: Box::new(provider),
                model: FAKE_TEXT_ONLY_MODEL_ID.to_owned(),
                streaming_policy: StreamingPolicy::Off,
            };
            let error = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user(
                        "create a file named GOLDEN_PATH.txt containing hello",
                    )],
                    tools: vec![json!({
                        "name": "apply_patch",
                        "parameters": {"type": "object"}
                    })],
                    sandbox: "workspace-write".to_owned(),
                })
                .await
                .unwrap_err();
            assert_eq!(error, "unsupported_tools");
        }

        #[tokio::test]
        /// 临时连接类 provider 错误应按 retry 配置重试并最终成功。
        async fn provider_retries_transient_error_without_network() {
            let provider = FakeProvider::new(
                "fake-model".to_owned(),
                vec![
                    FakeProviderStep::ProviderError {
                        code: Some("rate_limit".to_owned()),
                        message: "slow down".to_owned(),
                    },
                    FakeProviderStep::FinalAnswer {
                        text: "recovered".to_owned(),
                    },
                ],
            );
            let client = ProviderModelClient {
                provider: Box::new(provider),
                model: "fake-model".to_owned(),
                streaming_policy: StreamingPolicy::Off,
            };

            let output = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                })
                .await
                .expect("transient provider errors should be retried");

            assert_eq!(output.text, "recovered");
        }

        #[tokio::test]
        /// 非临时参数/认证类错误不能被无意义地重试。
        async fn provider_does_not_retry_non_transient_error_without_network() {
            let provider = FakeProvider::new(
                "fake-model".to_owned(),
                vec![
                    FakeProviderStep::ProviderError {
                        code: Some("invalid_model".to_owned()),
                        message: "model does not exist".to_owned(),
                    },
                    FakeProviderStep::FinalAnswer {
                        text: "should not be consumed".to_owned(),
                    },
                ],
            );
            let client = ProviderModelClient {
                provider: Box::new(provider),
                model: "fake-model".to_owned(),
                streaming_policy: StreamingPolicy::Off,
            };

            let error = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                })
                .await
                .expect_err("non-transient provider errors must not be retried");

            assert!(error.contains("invalid_model"), "{error}");
        }

        #[tokio::test]
        /// 限流错误经过等待后可以重试，成功响应只返回一次。
        async fn provider_retries_rate_limit_then_succeeds() {
            let (base_url, mut request_rx, server) = start_mock_openai_compatible_server(vec![
                (
                    429,
                    json!({
                        "error": {
                            "code": "rate_limit",
                            "message": "slow down"
                        }
                    }),
                ),
                (
                    200,
                    json!({
                        "id": "chatcmpl_retry",
                        "model": "gpt-test",
                        "choices": [{
                            "message": {
                                "role": "assistant",
                                "content": "recovered"
                            },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 11,
                            "completion_tokens": 3
                        }
                    }),
                ),
            ])
            .await;
            let provider = OpenAiCompatibleProvider::new(
                "test-key".to_owned(),
                base_url,
                Duration::from_secs(5),
            )
            .unwrap();
            let client = ProviderModelClient {
                provider: Box::new(provider),
                model: "gpt-test".to_owned(),
                streaming_policy: StreamingPolicy::Off,
            };

            let output = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                })
                .await
                .expect("429 should be retried");

            assert_eq!(output.text, "recovered");
            assert_eq!(output.model_id.as_deref(), Some("gpt-test"));
            assert_eq!(output.stop_reason.as_deref(), Some("stop"));
            assert_eq!(
                output.usage,
                Some(ModelUsage {
                    input_tokens: 11,
                    output_tokens: 3,
                })
            );
            assert!(request_rx.recv().await.is_some());
            assert!(request_rx.recv().await.is_some());
            assert!(
                request_rx.try_recv().is_err(),
                "429 followed by success must use exactly two attempts"
            );
            server.await.unwrap();
        }

        #[tokio::test]
        /// 认证错误属于不可重试类，第一次失败即返回。
        async fn provider_does_not_retry_auth_errors() {
            let (base_url, mut request_rx, server) = start_mock_openai_compatible_server(vec![(
                401,
                json!({
                    "error": {
                        "code": "invalid_api_key",
                        "message": "bad key"
                    }
                }),
            )])
            .await;
            let provider = OpenAiCompatibleProvider::new(
                "bad-key".to_owned(),
                base_url,
                Duration::from_secs(5),
            )
            .unwrap();
            let client = ProviderModelClient {
                provider: Box::new(provider),
                model: "gpt-test".to_owned(),
                streaming_policy: StreamingPolicy::Off,
            };

            let error = client
                .complete(ModelRequest {
                    messages: vec![ModelMessage::user("hello")],
                    tools: Vec::new(),
                    sandbox: "read-only".to_owned(),
                })
                .await
                .expect_err("401 must fail immediately");

            assert!(
                error.contains("provider authentication error"),
                "unexpected auth error: {error}"
            );
            assert!(error.contains("bad key"), "unexpected auth error: {error}");
            assert!(request_rx.recv().await.is_some());
            assert!(request_rx.try_recv().is_err(), "401 must not be retried");
            server.await.unwrap();
        }

        /// 启动一次或多次响应的本地 HTTP server，并把收到的原始请求通过 channel 暴露给测试。
        ///
        /// 每个 response 对应一次 TCP accept，正好模拟重试产生的多次独立请求；server task
        /// 在所有预置响应发送完后结束，调用者负责 await 它确认任务没有 panic。
        async fn start_mock_openai_compatible_server(
            responses: Vec<(u16, Value)>,
        ) -> (
            String,
            tokio::sync::mpsc::UnboundedReceiver<String>,
            tokio::task::JoinHandle<()>,
        ) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
            let server = tokio::spawn(async move {
                for (status, response) in responses {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let request = read_http_request(&mut socket).await;
                    request_tx.send(request).unwrap();
                    let body = serde_json::to_string(&response).unwrap();
                    let reason = match status {
                        200 => "OK",
                        401 => "Unauthorized",
                        429 => "Too Many Requests",
                        503 => "Service Unavailable",
                        _ => "Error",
                    };
                    let raw = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                    socket.write_all(raw.as_bytes()).await.unwrap();
                }
            });
            (format!("http://{addr}"), request_rx, server)
        }

        /// 启动一次返回 SSE body 的本地 HTTP server，用来验证 Anthropic 流式协议适配。
        async fn start_mock_anthropic_stream_server(
            stream_body: String,
        ) -> (
            String,
            tokio::sync::mpsc::UnboundedReceiver<String>,
            tokio::task::JoinHandle<()>,
        ) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut socket).await;
                request_tx.send(request).unwrap();
                let raw = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                stream_body.len(),
                stream_body
            );
                socket.write_all(raw.as_bytes()).await.unwrap();
            });
            (format!("http://{addr}"), request_rx, server)
        }

        /// 从 socket 读取完整 HTTP 请求，依据 header 中的 content-length 判断 body 已收齐。
        async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                // 分块读取避免假设单次 read 能拿到完整请求；连接关闭时退出作为兜底。
                let read = socket.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
                // 只有 header 和声明长度的 body 都到齐才停止，防止把重试请求截断后误判。
                if let Some(expected_len) = expected_http_request_len(&buffer) {
                    if buffer.len() >= expected_len {
                        break;
                    }
                }
            }
            String::from_utf8(buffer).unwrap()
        }

        /// 根据 HTTP header 计算请求总长度；未找到完整 header 时返回 None 继续读取。
        fn expected_http_request_len(buffer: &[u8]) -> Option<usize> {
            let header_end = find_header_end(buffer)?;
            let headers = std::str::from_utf8(&buffer[..header_end]).ok()?;
            // 只读取 content-length；测试 server 不处理 chunked encoding。
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            Some(header_end + 4 + content_length)
        }

        /// 找到 HTTP header 与 body 之间的 CRLF 分隔符位置。
        fn find_header_end(buffer: &[u8]) -> Option<usize> {
            buffer.windows(4).position(|window| window == b"\r\n\r\n")
        }
    }
}
