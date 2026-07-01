use crate::local_state::app_state_array_len;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_tools::permissions::{
    effective_tool_permissions, read_tool_permissions_file, write_tool_permissions_file,
};

pub struct PermissionsCommand;

#[async_trait]
impl Command for PermissionsCommand {
    fn name(&self) -> &str {
        "permissions"
    }

    fn description(&self) -> &str {
        "Manage permissions"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let raw_args = context.args.trim().to_string();
        let (command, rest) = split_command(&raw_args);
        match command {
            "" | "status" if rest.is_empty() => permissions_status(context),
            "status" => Err(anyhow!(
                "unknown permissions command '{}'\n\n{}",
                raw_args,
                usage()
            )),
            "allow" => update_rule(rest, RuleUpdate::Allow),
            "deny" => update_rule(rest, RuleUpdate::Deny),
            "remove" => update_rule(rest, RuleUpdate::Remove),
            "profile" => set_profile(rest),
            "mode" => set_mode(rest),
            "help" | "--help" | "-h" if rest.is_empty() => Ok(CommandResult::text(usage())),
            "help" | "--help" | "-h" => Err(anyhow!(
                "unknown permissions command '{}'\n\n{}",
                raw_args,
                usage()
            )),
            other => Err(anyhow!(
                "unknown permissions command '{}'\n\n{}",
                other,
                usage()
            )),
        }
    }
}

enum RuleUpdate {
    Allow,
    Deny,
    Remove,
}

fn split_command(args: &str) -> (&str, &str) {
    let args = args.trim();
    if args.is_empty() {
        return ("", "");
    }
    if let Some((command, rest)) = args.split_once(char::is_whitespace) {
        (command, rest.trim())
    } else {
        (args, "")
    }
}

fn permissions_status(context: CommandContext) -> Result<CommandResult> {
    let permissions = effective_tool_permissions(&context.app_state);
    let mut lines = vec![
        "Permissions status".to_string(),
        format!("profile: {}", permissions.profile),
        format!("mode: {}", permissions.mode),
        "tool_default: allow unless a deny rule or restrictive mode blocks it".to_string(),
        format!("file: {}", permissions.file_path.display()),
        format!(
            "file_status: {}",
            if permissions.file_loaded {
                "loaded"
            } else {
                "missing"
            }
        ),
        format!(
            "managed_policy_file: {}",
            permissions
                .managed_policy_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "managed_policy_status: {}",
            if permissions.managed_policy_loaded {
                "loaded"
            } else if permissions.managed_policy_path.is_some() {
                "error"
            } else {
                "none"
            }
        ),
        format!(
            "sources: {}",
            if permissions.sources.is_empty() {
                "defaults".to_string()
            } else {
                permissions.sources.join(",")
            }
        ),
        format!(
            "allowed_tools: {}",
            format_rules(&permissions.allowed_tools)
        ),
        format!(
            "disallowed_tools: {}",
            format_rules(&permissions.disallowed_tools)
        ),
        format!(
            "managed_allowed_tools: {}",
            format_rules(&permissions.managed_allowed_tools)
        ),
        format!(
            "managed_disallowed_tools: {}",
            format_rules(&permissions.managed_disallowed_tools)
        ),
        format!(
            "managed_ask_tools: {}",
            format_rules(&permissions.managed_ask_tools)
        ),
        format!(
            "session_tasks_seen_by_permission_layer: {}",
            app_state_array_len(&context.app_state, "tasks")
        ),
        "interactive_prompts: ask mode prompts in interactive terminals; non-interactive callers need matching allow rules".to_string(),
    ];
    if let Some(error) = permissions.file_error {
        lines.push(format!("file_error: {}", error));
    }
    if let Some(error) = permissions.managed_policy_error {
        lines.push(format!("managed_policy_error: {}", error));
    }
    Ok(CommandResult::text(lines.join("\n")))
}

fn update_rule(rule: &str, update: RuleUpdate) -> Result<CommandResult> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Err(anyhow!("missing tool rule\n\n{}", usage()));
    }
    validate_rule(rule)?;
    let mut file = read_tool_permissions_file().map_err(anyhow::Error::msg)?;

    match update {
        RuleUpdate::Allow => {
            remove_rule(&mut file.disallowed_tools, rule);
            add_rule(&mut file.allowed_tools, rule);
        }
        RuleUpdate::Deny => {
            remove_rule(&mut file.allowed_tools, rule);
            add_rule(&mut file.disallowed_tools, rule);
        }
        RuleUpdate::Remove => {
            remove_rule(&mut file.allowed_tools, rule);
            remove_rule(&mut file.disallowed_tools, rule);
        }
    }

    let path = write_tool_permissions_file(&file).map_err(anyhow::Error::msg)?;
    Ok(CommandResult::text(format!(
        "Permissions updated\nfile: {}\nallowed_tools: {}\ndisallowed_tools: {}",
        path.display(),
        format_rules(&file.allowed_tools),
        format_rules(&file.disallowed_tools)
    )))
}

fn validate_rule(rule: &str) -> Result<()> {
    if rule.chars().any(char::is_whitespace) && !is_parenthesized_tool_rule(rule) {
        return Err(anyhow!(
            "invalid permission rule '{}'; rules with spaces must use Tool(pattern) syntax\n\n{}",
            rule,
            usage()
        ));
    }
    Ok(())
}

fn is_parenthesized_tool_rule(rule: &str) -> bool {
    let Some(open) = rule.find('(') else {
        return false;
    };
    if !rule.ends_with(')') || open >= rule.len() - 1 {
        return false;
    }
    let tool = &rule[..open];
    !tool.is_empty() && !tool.chars().any(char::is_whitespace)
}

fn set_mode(mode: &str) -> Result<CommandResult> {
    let raw_mode = mode.trim();
    let mut parts = raw_mode.split_whitespace();
    let mode = parts
        .next()
        .ok_or_else(|| anyhow!("missing permission mode\n\n{}", usage()))?;
    if parts.next().is_some() {
        return Err(anyhow!(
            "unknown permissions mode arguments '{}'; expected exactly one mode\n\n{}",
            raw_mode,
            usage()
        ));
    }
    if !matches!(
        mode,
        "default" | "ask" | "plan" | "auto" | "acceptEdits" | "bypassPermissions" | "dontAsk"
    ) {
        return Err(anyhow!(
            "invalid permission mode '{}'; expected default, ask, plan, auto, acceptEdits, bypassPermissions, or dontAsk",
            mode
        ));
    }

    let mut file = read_tool_permissions_file().map_err(anyhow::Error::msg)?;
    file.mode = Some(mode.to_string());
    let path = write_tool_permissions_file(&file).map_err(anyhow::Error::msg)?;
    Ok(CommandResult::text(format!(
        "Permissions mode updated\nfile: {}\nmode: {}",
        path.display(),
        mode
    )))
}

fn set_profile(profile: &str) -> Result<CommandResult> {
    let raw_profile = profile.trim();
    let mut parts = raw_profile.split_whitespace();
    let profile = parts
        .next()
        .ok_or_else(|| anyhow!("missing permission profile\n\n{}", usage()))?;
    if parts.next().is_some() {
        return Err(anyhow!(
            "unknown permissions profile arguments '{}'; expected exactly one profile\n\n{}",
            raw_profile,
            usage()
        ));
    }
    if !matches!(
        profile,
        "read-only"
            | "readonly"
            | "read_only"
            | "workspace"
            | "commercial"
            | "commercial-security"
            | "commercial_security"
            | "full"
            | "ask"
            | "plan"
    ) {
        return Err(anyhow!(
            "invalid permission profile '{}'; expected read-only, workspace, commercial, full, ask, or plan",
            profile
        ));
    }

    let profile = normalize_profile(profile);
    let mut file = read_tool_permissions_file().map_err(anyhow::Error::msg)?;
    file.profile = Some(profile.clone());
    file.mode = None;
    let path = write_tool_permissions_file(&file).map_err(anyhow::Error::msg)?;
    Ok(CommandResult::text(format!(
        "Permissions profile updated\nfile: {}\nprofile: {}\nmode: {}",
        path.display(),
        profile,
        profile_default_mode(&profile)
    )))
}

fn add_rule(rules: &mut Vec<String>, rule: &str) {
    if !rules
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(rule))
    {
        rules.push(rule.to_string());
    }
}

fn remove_rule(rules: &mut Vec<String>, rule: &str) {
    rules.retain(|existing| !existing.eq_ignore_ascii_case(rule));
}

fn format_rules(rules: &[String]) -> String {
    if rules.is_empty() {
        "none".to_string()
    } else {
        rules.join(",")
    }
}

fn usage() -> &'static str {
    "Usage:\n  kiana permissions [status]\n  kiana permissions allow <ToolName|Tool(pattern)>\n  kiana permissions deny <ToolName|Tool(pattern)>\n  kiana permissions remove <ToolName|Tool(pattern)>\n  kiana permissions profile <read-only|workspace|commercial|full|ask|plan>\n  kiana permissions mode <default|ask|plan|auto|acceptEdits|bypassPermissions|dontAsk>"
}

fn normalize_profile(profile: &str) -> String {
    match profile.trim() {
        "read-only" | "readonly" | "read_only" => "read-only".to_string(),
        "workspace" | "default" => "workspace".to_string(),
        "commercial" | "commercial-security" | "commercial_security" => "commercial".to_string(),
        "full" => "full".to_string(),
        "ask" => "ask".to_string(),
        "plan" => "plan".to_string(),
        other => other.to_string(),
    }
}

fn profile_default_mode(profile: &str) -> &'static str {
    match profile {
        "read-only" | "plan" => "plan",
        "full" => "bypassPermissions",
        "commercial" => "ask",
        "ask" => "ask",
        _ => "default",
    }
}

#[cfg(test)]
mod tests {
    use super::PermissionsCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{MutexGuard, PoisonError};

    const PERMISSION_ENV_VARS: &[&str] = &[
        "KIANA_PERMISSIONS_FILE",
        "KIANA_PERMISSION_PROFILE",
        "KIANA_PERMISSION_MODE",
        "KIANA_ALLOWED_TOOLS",
        "KIANA_DISALLOWED_TOOLS",
        "KIANA_ASK_TOOLS",
        "KIANA_MANAGED_POLICY_FILE",
        "KIANA_MANAGED_PERMISSIONS_FILE",
    ];

    fn temp_permissions_path() -> std::path::PathBuf {
        let id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(format!("kiana-permissions-{id}.json"))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::new(),
        }
    }

    fn clear_permission_env() {
        for name in PERMISSION_ENV_VARS {
            std::env::remove_var(name);
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[tokio::test]
    async fn status_reports_session_permission_rules() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);

        let mut app_state = HashMap::new();
        app_state.insert("permission_mode".to_string(), json!("ask"));
        app_state.insert("allowed_tools".to_string(), json!(["Read"]));
        app_state.insert("disallowed_tools".to_string(), json!(["Bash"]));

        let result = PermissionsCommand
            .execute(CommandContext {
                args: String::new(),
                app_state,
            })
            .await
            .unwrap();

        assert!(result.value.contains("mode: ask"));
        assert!(result.value.contains("allowed_tools: Read"));
        assert!(result.value.contains("disallowed_tools: Bash"));

        let _ = std::fs::remove_file(&path);
        clear_permission_env();
    }

    #[tokio::test]
    async fn status_reports_managed_policy_rules() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        let managed = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);
        std::env::set_var("KIANA_MANAGED_POLICY_FILE", &managed);
        std::fs::write(
            &managed,
            r#"{"permissions":{"profile":"read-only","disallowedTools":["Bash"],"askTools":["Write"]}}"#,
        )
        .unwrap();

        let status = PermissionsCommand.execute(context("status")).await.unwrap();

        assert!(status.value.contains("profile: read-only"));
        assert!(status.value.contains("managed_policy_status: loaded"));
        assert!(status.value.contains("managed_disallowed_tools: Bash"));
        assert!(status.value.contains("managed_ask_tools: Write"));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&managed);
        clear_permission_env();
    }

    #[tokio::test]
    async fn allow_and_mode_commands_persist_permissions_file() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);

        let allow = PermissionsCommand
            .execute(context("allow Bash"))
            .await
            .unwrap();
        assert!(allow.value.contains("allowed_tools: Bash"));

        let mode = PermissionsCommand
            .execute(context("mode ask"))
            .await
            .unwrap();
        assert!(mode.value.contains("mode: ask"));

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["mode"], "ask");
        assert_eq!(saved["allowed_tools"][0], "Bash");

        let _ = std::fs::remove_file(&path);
        clear_permission_env();
    }

    #[tokio::test]
    async fn profile_command_persists_permission_profile_and_status_reports_it() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);

        let profile = PermissionsCommand
            .execute(context("profile read-only"))
            .await
            .unwrap();
        assert!(profile.value.contains("profile: read-only"));

        let status = PermissionsCommand.execute(context("status")).await.unwrap();
        assert!(status.value.contains("profile: read-only"));
        assert!(status.value.contains("mode: plan"));

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["profile"], "read-only");

        let _ = std::fs::remove_file(&path);
        clear_permission_env();
    }

    #[tokio::test]
    async fn commercial_profile_defaults_to_ask_mode() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);

        let profile = PermissionsCommand
            .execute(context("profile commercial"))
            .await
            .unwrap();
        assert!(profile.value.contains("profile: commercial"));
        assert!(profile.value.contains("mode: ask"));

        let status = PermissionsCommand.execute(context("status")).await.unwrap();
        assert!(status.value.contains("profile: commercial"));
        assert!(status.value.contains("mode: ask"));

        let _ = std::fs::remove_file(&path);
        clear_permission_env();
    }

    #[tokio::test]
    async fn mode_rejects_extra_words_without_writing_permissions_file() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);

        let error = PermissionsCommand
            .execute(context("mode ask please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown permissions mode arguments"));
        assert!(!path.exists());

        clear_permission_env();
    }

    #[tokio::test]
    async fn allow_preserves_parenthesized_rule_with_spaces() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);

        let result = PermissionsCommand
            .execute(context("allow Bash(git status)"))
            .await
            .unwrap();

        assert!(result.value.contains("allowed_tools: Bash(git status)"));
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["allowed_tools"][0], "Bash(git status)");

        let _ = std::fs::remove_file(&path);
        clear_permission_env();
    }

    #[tokio::test]
    async fn allow_rejects_unparenthesized_extra_words_without_writing_permissions_file() {
        let _guard = lock_env();
        clear_permission_env();
        let path = temp_permissions_path();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);

        let error = PermissionsCommand
            .execute(context("allow Bash extra"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("invalid permission rule"));
        assert!(!path.exists());

        clear_permission_env();
    }
}
