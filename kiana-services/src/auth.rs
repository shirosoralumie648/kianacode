use serde::{Deserialize, Serialize};

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
    use super::{check_oauth_tokens, get_api_key, KeySource};
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
