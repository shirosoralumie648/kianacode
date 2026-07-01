use crate::local_state::{config_path, load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;

pub struct LogoutCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogoutTarget {
    All,
    ApiKeyOnly,
    OAuthOnly,
}

#[async_trait]
impl Command for LogoutCommand {
    fn name(&self) -> &str {
        "logout"
    }

    fn description(&self) -> &str {
        "Logout from account"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let arg = context.args.trim();
        match arg {
            "" | "--all" => logout(LogoutTarget::All),
            "--api-key-only" => logout(LogoutTarget::ApiKeyOnly),
            "--oauth-only" => logout(LogoutTarget::OAuthOnly),
            "status" => Ok(CommandResult::text(logout_status())),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown logout command '{}'\n\n{}", other, usage())),
        }
    }
}

fn logout(target: LogoutTarget) -> anyhow::Result<CommandResult> {
    let mut config = load_user_config();
    let had_file_key = config
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let path = config_path();
    let removed_config_key = if target != LogoutTarget::OAuthOnly {
        config.api_key = None;
        save_user_config(&config)?;
        if had_file_key {
            "yes"
        } else {
            "no"
        }
    } else {
        "skipped"
    };
    let oauth_deletion = if target != LogoutTarget::ApiKeyOnly {
        let deletion = kiana_services::oauth::delete_oauth_tokens()?;
        Some(deletion)
    } else {
        None
    };
    let env_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let (removed_oauth_tokens, oauth_file) = oauth_deletion
        .as_ref()
        .map(|deletion| {
            (
                if deletion.removed { "yes" } else { "no" },
                deletion.file.as_str(),
            )
        })
        .unwrap_or(("skipped", "skipped"));
    Ok(CommandResult::text(format!(
        "Logout complete\nremoved_config_key: {}\nfile: {}\nremoved_oauth_tokens: {}\noauth_file: {}\nenv_key_still_set: {}",
        removed_config_key,
        path.display(),
        removed_oauth_tokens,
        oauth_file,
        if env_key { "yes" } else { "no" }
    )))
}

fn logout_status() -> String {
    let config = load_user_config();
    let file_key = config
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let env_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let oauth = kiana_services::oauth::inspect_oauth_tokens(
        kiana_services::oauth::DEFAULT_OAUTH_EXPIRY_SKEW,
    );
    format!(
        "Logout status\nfile_api_key: {}\nenv_api_key: {}\nconfig_file: {}\noauth_status: {}\noauth_file: {}\nusage: kiana logout [--all|--api-key-only|--oauth-only]",
        if file_key { "set" } else { "missing" },
        if env_key { "set" } else { "missing" },
        config_path().display(),
        oauth.status.as_str(),
        oauth.file
    )
}

fn usage() -> &'static str {
    "Usage: kiana logout [--all|--api-key-only|--oauth-only]\n       kiana logout status"
}

#[cfg(test)]
mod tests {
    use super::LogoutCommand;
    use crate::local_state::{env_lock, save_user_config};
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-logout-command-{}-{unique}.toml",
            std::process::id()
        ))
    }

    fn temp_oauth_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-logout-oauth-{}-{unique}.json",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    fn write_config_with_api_key(path: &std::path::Path) {
        std::env::set_var("KIANA_CONFIG_FILE", path);
        std::env::remove_var("ANTHROPIC_API_KEY");
        let mut config = kiana_bootstrap::config::Config::default();
        config.api_key = Some("sk-ant-logout-test-key".to_string());
        save_user_config(&config).unwrap();
    }

    fn write_oauth_tokens(path: &std::path::Path) {
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", path);
        kiana_services::oauth::save_oauth_tokens(&kiana_services::oauth::OAuthTokens {
            access_token: "oauth-access-token".to_string(),
            refresh_token: Some("oauth-refresh-token".to_string()),
            expires_at: None,
        })
        .unwrap();
    }

    fn clear_logout_env() {
        std::env::remove_var("KIANA_CONFIG_FILE");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_OAUTH_TOKENS_FILE");
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKENS_FILE");
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn logout_removes_config_key_and_oauth_tokens_by_default() {
        let _guard = env_lock().lock().unwrap();
        clear_logout_env();
        let config_path = temp_config_path();
        let oauth_path = temp_oauth_path();
        write_config_with_api_key(&config_path);
        write_oauth_tokens(&oauth_path);

        let result = LogoutCommand.execute(context("")).await.unwrap();

        assert!(result.value.contains("Logout complete"));
        assert!(result.value.contains("removed_config_key: yes"));
        assert!(result.value.contains("removed_oauth_tokens: yes"));
        assert!(!fs::read_to_string(&config_path)
            .unwrap()
            .contains("sk-ant-logout-test-key"));
        assert!(!oauth_path.exists());

        let _ = fs::remove_file(config_path);
        clear_logout_env();
    }

    #[tokio::test]
    async fn logout_oauth_only_preserves_config_key() {
        let _guard = env_lock().lock().unwrap();
        clear_logout_env();
        let config_path = temp_config_path();
        let oauth_path = temp_oauth_path();
        write_config_with_api_key(&config_path);
        write_oauth_tokens(&oauth_path);

        let result = LogoutCommand
            .execute(context("--oauth-only"))
            .await
            .unwrap();

        assert!(result.value.contains("removed_config_key: skipped"));
        assert!(result.value.contains("removed_oauth_tokens: yes"));
        assert!(fs::read_to_string(&config_path)
            .unwrap()
            .contains("sk-ant-logout-test-key"));
        assert!(!oauth_path.exists());

        let _ = fs::remove_file(config_path);
        clear_logout_env();
    }

    #[tokio::test]
    async fn logout_status_reports_without_removing_config_key() {
        let _guard = env_lock().lock().unwrap();
        clear_logout_env();
        let path = temp_config_path();
        write_config_with_api_key(&path);

        let result = LogoutCommand.execute(context("status")).await.unwrap();

        assert!(result.value.contains("Logout status"));
        assert!(result.value.contains("file_api_key: set"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("sk-ant-logout-test-key"));

        let _ = fs::remove_file(&path);
        clear_logout_env();
    }

    #[tokio::test]
    async fn logout_rejects_unknown_args_without_removing_config_key() {
        let _guard = env_lock().lock().unwrap();
        clear_logout_env();
        let path = temp_config_path();
        write_config_with_api_key(&path);

        let error = LogoutCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown logout command"));
        assert!(error.contains("Usage: kiana logout"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("sk-ant-logout-test-key"));

        let _ = fs::remove_file(&path);
        clear_logout_env();
    }
}
