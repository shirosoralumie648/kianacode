use crate::credentials::{EnvSecretStore, InlineSecretStore, SecretStore};
use kiana_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, fmt, time::Duration};

#[derive(Clone, Default)]
pub struct ProviderConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

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
    pub capacity: std::sync::Arc<tokio::sync::Semaphore>,
}
#[derive(Clone)]
pub(crate) struct TransportLimits {
    pub headers: Duration,
    pub first_event: Duration,
    pub idle: Duration,
    pub total: Duration,
    pub max_body: usize,
    pub max_frame: usize,
}
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
fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}
fn support(value: bool) -> CapabilitySupport {
    if value {
        CapabilitySupport::Supported
    } else {
        CapabilitySupport::Unsupported
    }
}

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

pub(crate) fn connections(
    config: ProviderConfig,
) -> Result<(BTreeMap<String, Connection>, bool), ModelError> {
    let configured = env("KIANA_MODEL_PROFILES_JSON");
    let default = connection("default", config, None);
    if configured.is_none() {
        return default
            .map(|connection| (BTreeMap::from([("default".to_owned(), connection)]), false));
    }
    let mut result = BTreeMap::new();
    if let Ok(connection) = default {
        result.insert("default".to_owned(), connection);
    }
    let raw = configured.unwrap();
    if raw.len() > 64 * 1024 {
        return Err(ModelError::invalid("model_profile_config_too_large"));
    }
    let values: BTreeMap<String, ProfileConfig> = serde_json::from_str(&raw)
        .map_err(|_| ModelError::invalid("model_profile_config_invalid"))?;
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
            )?
        };
        item.route.profile = profile.clone();
        result.insert(profile, item);
    }
    if result.is_empty() {
        return Err(ModelError::invalid("model_profiles_empty"));
    }
    // Connection aliases that point at the same credential and origin share one
    // semaphore. A profile name is a routing label, not a way to bypass the
    // provider's connection-level capacity limit.
    let mut capacities: BTreeMap<String, std::sync::Arc<tokio::sync::Semaphore>> = BTreeMap::new();
    for connection in result.values_mut() {
        let scope = json_digest(&json!({
            "provider":connection.route.provider_id,
            "origin":connection.endpoint.as_str(),
            "credential":connection.credential_ref.as_ref().map(|reference|reference.reference_digest.clone()),
        }));
        let capacity = capacities
            .entry(scope)
            .or_insert_with(|| connection.capacity.clone())
            .clone();
        connection.capacity = capacity;
    }
    Ok((result, true))
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
fn connection(
    name: &str,
    config: ProviderConfig,
    declared: Option<DeclaredCapabilities>,
) -> Result<Connection, ModelError> {
    connection_with_credential_env(name, config, declared, None, None)
}

fn connection_with_credential_env(
    name: &str,
    config: ProviderConfig,
    declared: Option<DeclaredCapabilities>,
    credential_env_override: Option<String>,
    ollama_load_timeout: Option<Duration>,
) -> Result<Connection, ModelError> {
    let provider = config
        .provider
        .or_else(|| env("KIANA_PROVIDER"))
        .unwrap_or_else(|| "anthropic".to_owned());
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
        .unwrap_or_else(|| default_model.to_owned());
    if model.trim().is_empty() || model.len() > 256 {
        return Err(ModelError::invalid("model_id_invalid"));
    }
    let base = config
        .base_url
        .or_else(|| env(base_env))
        .unwrap_or_else(|| default_base.to_owned());
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
    // Pin localhost to a literal address; redirects are disabled and ambient proxies ignored.
    if endpoint.host_str() == Some("localhost") {
        endpoint
            .set_host(Some("127.0.0.1"))
            .map_err(|_| ModelError::invalid("model_endpoint_invalid"))?;
    }
    let configured_env =
        credential_env_override.or_else(|| (!key_env.is_empty()).then(|| key_env.to_owned()));
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
    let streaming_override = match env("KIANA_STREAMING").as_deref() {
        None | Some("auto" | "on" | "1" | "true" | "yes") => true,
        Some("off" | "0" | "false" | "no") => false,
        _ => return Err(ModelError::invalid("model_streaming_policy_invalid")),
    };
    let streaming = protocol == ModelProtocol::OllamaChat || streaming_override;
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
    let (tools, images, structured, context_window, max_output, source) =
        if let Some(cap) = &declared {
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
        provider_id: provider,
        protocol,
        connection_id: name.to_owned(),
        model_id: model,
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
        revision,
    };
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
        capacity: std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrency)),
    })
}

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
