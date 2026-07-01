use crate::local_state::{config_path, load_user_config};
use crate::login::LoginCommand;
use crate::logout::LogoutCommand;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::json;

pub struct AuthCommand;

#[async_trait]
impl Command for AuthCommand {
    fn name(&self) -> &str {
        "auth"
    }

    fn description(&self) -> &str {
        "Manage authentication"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("status") {
            "" | "status" => auth_status(rest),
            "login" => {
                LoginCommand
                    .execute(CommandContext {
                        args: rest.to_string(),
                        app_state: context.app_state,
                    })
                    .await
            }
            "logout" => {
                LogoutCommand
                    .execute(CommandContext {
                        args: rest.to_string(),
                        app_state: context.app_state,
                    })
                    .await
            }
            "help" | "--help" | "-h" if rest.is_empty() => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown auth command '{}'\n\n{}", other, usage())),
        }
    }
}

fn auth_status(rest: &str) -> anyhow::Result<CommandResult> {
    match rest.trim() {
        "" | "--text" => Ok(CommandResult::text(auth_status_text())),
        "--json" => Ok(CommandResult::text(auth_status_json()?)),
        "help" | "--help" | "-h" => Ok(CommandResult::text(status_usage())),
        other => Err(anyhow!(
            "unknown auth status option '{}'\n\n{}",
            other,
            status_usage()
        )),
    }
}

fn auth_status_text() -> String {
    let state = auth_state();
    format!(
        "Auth status\napi_key: {}\nsource: {}\nconfig_file: {}\noauth_status: {}\noauth_access_token: {}\noauth_refresh_token: {}\noauth_expires_at: {}\noauth_expired: {}\noauth_expiring: {}\noauth_refreshable: {}\noauth_store: {}\noauth_file: {}\n{}usage: kiana auth status [--json|--text]",
        state.api_key,
        state.source,
        state.config_file,
        state.oauth_status,
        state.oauth_access_token,
        state.oauth_refresh_token,
        state.oauth_expires_at.as_deref().unwrap_or("none"),
        yes_no(state.oauth_expired),
        yes_no(state.oauth_expiring),
        yes_no(state.oauth_refreshable),
        state.oauth_store,
        state.oauth_file,
        state
            .oauth_error
            .as_ref()
            .map(|error| format!("oauth_error: {error}\n"))
            .unwrap_or_default()
    )
}

fn auth_status_json() -> anyhow::Result<String> {
    let state = auth_state();
    Ok(serde_json::to_string_pretty(&json!({
        "api_key": state.api_key,
        "source": state.source,
        "config_file": state.config_file,
        "oauth": {
            "status": state.oauth_status,
            "access_token": state.oauth_access_token,
            "refresh_token": state.oauth_refresh_token,
            "file": state.oauth_file,
            "store": state.oauth_store,
            "expires_at": state.oauth_expires_at,
            "expired": state.oauth_expired,
            "expiring": state.oauth_expiring,
            "refreshable": state.oauth_refreshable,
            "error": state.oauth_error,
        }
    }))?)
}

fn auth_state() -> AuthState {
    let env_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let file_key = load_user_config()
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let oauth = oauth_state();
    AuthState {
        api_key: if env_key || file_key {
            "set".to_string()
        } else {
            "missing".to_string()
        },
        source: if env_key {
            "ANTHROPIC_API_KEY".to_string()
        } else if file_key {
            "config".to_string()
        } else {
            "none".to_string()
        },
        config_file: config_path().display().to_string(),
        oauth_status: oauth.status,
        oauth_access_token: oauth.access_token,
        oauth_refresh_token: oauth.refresh_token,
        oauth_file: oauth.file,
        oauth_store: oauth.store,
        oauth_expires_at: oauth.expires_at,
        oauth_expired: oauth.expired,
        oauth_expiring: oauth.expiring,
        oauth_refreshable: oauth.refreshable,
        oauth_error: oauth.error,
    }
}

struct AuthState {
    api_key: String,
    source: String,
    config_file: String,
    oauth_status: String,
    oauth_access_token: String,
    oauth_refresh_token: String,
    oauth_file: String,
    oauth_store: String,
    oauth_expires_at: Option<String>,
    oauth_expired: bool,
    oauth_expiring: bool,
    oauth_refreshable: bool,
    oauth_error: Option<String>,
}

fn oauth_state() -> OAuthAuthState {
    let inspection = kiana_services::oauth::inspect_oauth_tokens(
        kiana_services::oauth::DEFAULT_OAUTH_EXPIRY_SKEW,
    );
    let (access_token, refresh_token) = match inspection.status {
        kiana_services::oauth::OAuthTokenFileStatus::Valid => (
            presence(inspection.access_token),
            presence(inspection.refresh_token),
        ),
        kiana_services::oauth::OAuthTokenFileStatus::Missing => {
            ("missing".to_string(), "missing".to_string())
        }
        kiana_services::oauth::OAuthTokenFileStatus::Invalid => {
            ("invalid".to_string(), "invalid".to_string())
        }
    };
    OAuthAuthState {
        status: inspection.status.as_str().to_string(),
        access_token,
        refresh_token,
        file: inspection.file,
        store: inspection.store,
        expires_at: inspection.expires_at.map(|value| value.to_rfc3339()),
        expired: inspection.expired,
        expiring: inspection.expiring,
        refreshable: inspection.refreshable,
        error: inspection.error,
    }
}

struct OAuthAuthState {
    status: String,
    access_token: String,
    refresh_token: String,
    file: String,
    store: String,
    expires_at: Option<String>,
    expired: bool,
    expiring: bool,
    refreshable: bool,
    error: Option<String>,
}

fn presence(present: bool) -> String {
    if present {
        "set".to_string()
    } else {
        "missing".to_string()
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn usage() -> &'static str {
    "Usage: kiana auth [status|login|logout]\n       kiana auth status [--json|--text]\n       kiana auth login <api-key>\n       kiana auth logout"
}

fn status_usage() -> &'static str {
    "Usage: kiana auth status [--json|--text]"
}

fn split_word(input: &str) -> (Option<&str>, &str) {
    let input = input.trim();
    if input.is_empty() {
        return (None, "");
    }
    match input.find(char::is_whitespace) {
        Some(index) => (Some(&input[..index]), input[index..].trim()),
        None => (Some(input), ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_state::env_lock;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct EnvGuard {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn set(values: &[(&'static str, Option<&str>)]) -> Self {
            let previous = values
                .iter()
                .map(|(key, _)| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for (key, value) in values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { values: previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kiana-auth-{name}-{}-{unique}", std::process::id()))
    }

    #[test]
    fn auth_status_json_reports_oauth_token_file_without_leaking_tokens() {
        let _lock = env_lock().lock().unwrap();
        let token_path = temp_path("oauth.json");
        let config_path = temp_path("config.toml");
        let token_path_str = token_path.to_string_lossy().to_string();
        let config_path_str = config_path.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("ANTHROPIC_API_KEY", None),
            ("KIANA_CONFIG_FILE", Some(&config_path_str)),
            ("KIANA_OAUTH_TOKENS_FILE", Some(&token_path_str)),
        ]);
        kiana_services::oauth::save_oauth_tokens(&kiana_services::oauth::OAuthTokens {
            access_token: "oauth-access-secret".to_string(),
            refresh_token: Some("oauth-refresh-secret".to_string()),
            expires_at: None,
        })
        .unwrap();

        let output = auth_status_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(value["api_key"], "missing");
        assert_eq!(value["oauth"]["status"], "valid");
        assert_eq!(value["oauth"]["access_token"], "set");
        assert_eq!(value["oauth"]["refresh_token"], "set");
        assert_eq!(value["oauth"]["file"], token_path_str);
        assert_eq!(value["oauth"]["store"], "file");
        assert_eq!(value["oauth"]["expires_at"], serde_json::Value::Null);
        assert_eq!(value["oauth"]["expired"], false);
        assert_eq!(value["oauth"]["expiring"], false);
        assert_eq!(value["oauth"]["refreshable"], true);
        assert!(!output.contains("oauth-access-secret"));
        assert!(!output.contains("oauth-refresh-secret"));

        let _ = fs::remove_file(token_path);
        let _ = fs::remove_file(config_path);
    }
}
