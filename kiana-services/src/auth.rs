use serde::{Deserialize, Serialize};

use crate::api::provider::{
    ANTHROPIC_PROVIDER_ID, FAKE_MODEL_ID, FAKE_PROVIDER_ID, OLLAMA_DEFAULT_MODEL_ID,
    OLLAMA_PROVIDER_ID, OPENAI_COMPATIBLE_DEFAULT_MODEL_ID, OPENAI_COMPATIBLE_PROVIDER_ID,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key: String,
    pub source: KeySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeySource {
    Environment,
    Config,
    Helper,
}

pub fn get_api_key() -> Option<ApiKey> {
    let env_key = std::env::var("ANTHROPIC_API_KEY").ok().and_then(non_empty);
    let key = kiana_bootstrap::config::get_api_key().and_then(non_empty)?;
    let source = if env_key.as_deref() == Some(key.as_str()) {
        KeySource::Environment
    } else {
        KeySource::Config
    };
    Some(ApiKey { key, source })
}

pub fn check_oauth_tokens() -> bool {
    crate::oauth::inspect_oauth_tokens(crate::oauth::DEFAULT_OAUTH_EXPIRY_SKEW)
        .has_usable_access_token()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAuthStatus {
    pub provider_id: String,
    pub status: String,
    pub auth: String,
    pub auth_source: String,
    pub model_id: String,
    pub base_url: Option<String>,
    pub key_preview: Option<String>,
    pub issues: Vec<String>,
}

pub fn provider_auth_statuses(config: &kiana_bootstrap::config::Config) -> Vec<ProviderAuthStatus> {
    vec![
        anthropic_provider_auth_status(config),
        openai_provider_auth_status(),
        ollama_provider_auth_status(),
        fake_provider_auth_status(),
    ]
}

fn anthropic_provider_auth_status(config: &kiana_bootstrap::config::Config) -> ProviderAuthStatus {
    let api_key = get_api_key();
    let source = api_key
        .as_ref()
        .map(|api_key| match api_key.source {
            KeySource::Environment => "ANTHROPIC_API_KEY",
            KeySource::Config => "config",
            KeySource::Helper => "helper",
        })
        .unwrap_or("none");
    let key_preview = api_key
        .as_ref()
        .map(|api_key| redacted_preview(&api_key.key));
    let base_url = std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .and_then(non_empty)
        .or_else(|| config.base_url.clone().and_then(non_empty))
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());
    let model_id = std::env::var("ANTHROPIC_MODEL")
        .ok()
        .and_then(non_empty)
        .unwrap_or_else(|| config.model.clone());
    let mut issues = Vec::new();
    if api_key.is_none() {
        issues.push("set ANTHROPIC_API_KEY or configure api_key".to_string());
    }
    ProviderAuthStatus {
        provider_id: ANTHROPIC_PROVIDER_ID.to_string(),
        status: if issues.is_empty() {
            "configured".to_string()
        } else {
            "missing".to_string()
        },
        auth: "api_key".to_string(),
        auth_source: source.to_string(),
        model_id,
        base_url: Some(base_url),
        key_preview,
        issues,
    }
}

fn openai_provider_auth_status() -> ProviderAuthStatus {
    let api_key = first_env_value(&["KIANA_OPENAI_API_KEY", "OPENAI_API_KEY"]);
    let source = api_key
        .as_ref()
        .map(|(source, _)| source.to_string())
        .unwrap_or_else(|| "none".to_string());
    let key_preview = api_key
        .as_ref()
        .map(|(_, api_key)| redacted_preview(api_key));
    let base_url = first_env_value(&["KIANA_OPENAI_BASE_URL", "OPENAI_BASE_URL"])
        .map(|(_, value)| value)
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model_id = first_env_value(&["KIANA_OPENAI_MODEL", "OPENAI_MODEL"])
        .map(|(_, value)| value)
        .unwrap_or_else(|| OPENAI_COMPATIBLE_DEFAULT_MODEL_ID.to_string());
    let mut issues = Vec::new();
    if api_key.is_none() {
        issues.push("set KIANA_OPENAI_API_KEY or OPENAI_API_KEY".to_string());
    }
    ProviderAuthStatus {
        provider_id: OPENAI_COMPATIBLE_PROVIDER_ID.to_string(),
        status: if issues.is_empty() {
            "configured".to_string()
        } else {
            "missing".to_string()
        },
        auth: "api_key".to_string(),
        auth_source: source,
        model_id,
        base_url: Some(base_url),
        key_preview,
        issues,
    }
}

fn ollama_provider_auth_status() -> ProviderAuthStatus {
    let base_url = first_env_value(&["KIANA_OLLAMA_BASE_URL", "OLLAMA_BASE_URL"])
        .map(|(_, value)| value)
        .unwrap_or_else(|| "http://localhost:11434".to_string());
    let model_id = first_env_value(&["KIANA_OLLAMA_MODEL", "OLLAMA_MODEL"])
        .map(|(_, value)| value)
        .unwrap_or_else(|| OLLAMA_DEFAULT_MODEL_ID.to_string());
    ProviderAuthStatus {
        provider_id: OLLAMA_PROVIDER_ID.to_string(),
        status: "configured".to_string(),
        auth: "not_required".to_string(),
        auth_source: "none".to_string(),
        model_id,
        base_url: Some(base_url),
        key_preview: None,
        issues: vec![
            "daemon reachability is not checked by auth status; run model smoke --live".to_string(),
        ],
    }
}

fn fake_provider_auth_status() -> ProviderAuthStatus {
    ProviderAuthStatus {
        provider_id: FAKE_PROVIDER_ID.to_string(),
        status: "configured".to_string(),
        auth: "not_required".to_string(),
        auth_source: "none".to_string(),
        model_id: FAKE_MODEL_ID.to_string(),
        base_url: None,
        key_preview: None,
        issues: Vec::new(),
    }
}

fn first_env_value(keys: &[&'static str]) -> Option<(&'static str, String)> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .and_then(non_empty)
            .map(|value| (*key, value))
    })
}

fn redacted_preview(secret: &str) -> String {
    let suffix: String = secret
        .trim()
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if suffix.is_empty() {
        "redacted".to_string()
    } else {
        format!("redacted-{}", suffix)
    }
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        check_oauth_tokens, get_api_key, provider_auth_statuses, KeySource, OLLAMA_PROVIDER_ID,
        OPENAI_COMPATIBLE_PROVIDER_ID,
    };
    use chrono::{Duration as ChronoDuration, Utc};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        crate::env_test_lock()
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn temp_path(name: &str, extension: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-service-auth-{name}-{}-{unique}.{extension}",
            std::process::id()
        ))
    }

    fn clear_auth_env() {
        for key in [
            "KIANA_CONFIG_FILE",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_MANAGED_SETTINGS_FILE",
            "KIANA_MANAGED_POLICY_FILE",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OPENAI_BASE_URL",
            "OPENAI_BASE_URL",
            "KIANA_OPENAI_MODEL",
            "OPENAI_MODEL",
            "KIANA_OLLAMA_BASE_URL",
            "OLLAMA_BASE_URL",
            "KIANA_OLLAMA_MODEL",
            "OLLAMA_MODEL",
            "KIANA_OAUTH_TOKENS_FILE",
            "CLAUDE_CODE_OAUTH_TOKENS_FILE",
            "KIANA_HOME",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn api_key_resolves_from_config_file_when_env_missing() {
        let _guard = lock_env();
        clear_auth_env();
        let path = temp_path("config", "toml");
        fs::write(&path, "api_key = \"config-key\"\n").unwrap();
        std::env::set_var("KIANA_CONFIG_FILE", &path);

        let api_key = get_api_key().expect("config key should resolve");

        assert_eq!(api_key.key, "config-key");
        assert_eq!(api_key.source, KeySource::Config);

        let _ = fs::remove_file(path);
        clear_auth_env();
    }

    #[test]
    fn provider_auth_statuses_report_openai_key_without_leaking_secret() {
        let _guard = lock_env();
        clear_auth_env();
        std::env::set_var("KIANA_OPENAI_API_KEY", "openai-secret-1234");
        std::env::set_var("KIANA_OPENAI_BASE_URL", "https://gateway.example/v1");
        std::env::set_var("KIANA_OPENAI_MODEL", "gpt-test");

        let statuses = provider_auth_statuses(&kiana_bootstrap::config::Config::default());
        let openai = statuses
            .iter()
            .find(|status| status.provider_id == OPENAI_COMPATIBLE_PROVIDER_ID)
            .unwrap();

        assert_eq!(openai.status, "configured");
        assert_eq!(openai.auth, "api_key");
        assert_eq!(openai.auth_source, "KIANA_OPENAI_API_KEY");
        assert_eq!(openai.key_preview.as_deref(), Some("redacted-1234"));
        assert_eq!(
            openai.base_url.as_deref(),
            Some("https://gateway.example/v1")
        );
        assert_eq!(openai.model_id, "gpt-test");
        let json = serde_json::to_string(openai).unwrap();
        assert!(!json.contains("openai-secret"));

        clear_auth_env();
    }

    #[test]
    fn provider_auth_statuses_report_ollama_as_no_auth_with_endpoint() {
        let _guard = lock_env();
        clear_auth_env();
        std::env::set_var("OLLAMA_BASE_URL", "http://127.0.0.1:11434");
        std::env::set_var("OLLAMA_MODEL", "llama-test");

        let statuses = provider_auth_statuses(&kiana_bootstrap::config::Config::default());
        let ollama = statuses
            .iter()
            .find(|status| status.provider_id == OLLAMA_PROVIDER_ID)
            .unwrap();

        assert_eq!(ollama.status, "configured");
        assert_eq!(ollama.auth, "not_required");
        assert_eq!(ollama.auth_source, "none");
        assert_eq!(ollama.base_url.as_deref(), Some("http://127.0.0.1:11434"));
        assert_eq!(ollama.model_id, "llama-test");
        assert!(ollama.issues[0].contains("model smoke --live"));

        clear_auth_env();
    }

    #[test]
    fn api_key_reports_environment_when_env_is_effective_key() {
        let _guard = lock_env();
        clear_auth_env();
        std::env::set_var("ANTHROPIC_API_KEY", "env-key");

        let api_key = get_api_key().expect("env key should resolve");

        assert_eq!(api_key.key, "env-key");
        assert_eq!(api_key.source, KeySource::Environment);

        clear_auth_env();
    }

    #[test]
    fn api_key_uses_managed_config_overlay_after_env() {
        let _guard = lock_env();
        clear_auth_env();
        let managed = temp_path("managed", "json");
        fs::write(&managed, r#"{"apiKey":"managed-key"}"#).unwrap();
        std::env::set_var("ANTHROPIC_API_KEY", "env-key");
        std::env::set_var("KIANA_MANAGED_SETTINGS_FILE", &managed);

        let api_key = get_api_key().expect("managed key should resolve");

        assert_eq!(api_key.key, "managed-key");
        assert_eq!(api_key.source, KeySource::Config);

        let _ = fs::remove_file(managed);
        clear_auth_env();
    }

    #[test]
    fn api_key_ignores_empty_effective_key() {
        let _guard = lock_env();
        clear_auth_env();
        std::env::set_var("ANTHROPIC_API_KEY", "   ");

        assert!(get_api_key().is_none());

        clear_auth_env();
    }

    #[test]
    fn oauth_token_check_accepts_unexpired_token_file() {
        let _guard = lock_env();
        clear_auth_env();
        let path = temp_path("oauth-valid", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &path);
        crate::oauth::save_oauth_tokens(&crate::oauth::OAuthTokens {
            access_token: "oauth-access".to_string(),
            refresh_token: Some("oauth-refresh".to_string()),
            expires_at: Some(Utc::now() + ChronoDuration::hours(1)),
        })
        .unwrap();

        assert!(check_oauth_tokens());

        let _ = fs::remove_file(path);
        clear_auth_env();
    }

    #[test]
    fn oauth_token_check_rejects_expired_token_file() {
        let _guard = lock_env();
        clear_auth_env();
        let path = temp_path("oauth-expired", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &path);
        crate::oauth::save_oauth_tokens(&crate::oauth::OAuthTokens {
            access_token: "oauth-access".to_string(),
            refresh_token: Some("oauth-refresh".to_string()),
            expires_at: Some(Utc::now() - ChronoDuration::minutes(1)),
        })
        .unwrap();

        assert!(!check_oauth_tokens());

        let _ = fs::remove_file(path);
        clear_auth_env();
    }
}
