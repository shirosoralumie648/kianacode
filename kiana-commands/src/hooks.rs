use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_types::hooks::{parse_hook_commands, parse_hook_config, HookConfig};
use serde_json::Value;
use std::path::PathBuf;

pub struct HooksCommand;

#[async_trait]
impl Command for HooksCommand {
    fn name(&self) -> &str {
        "hooks"
    }

    fn description(&self) -> &str {
        "Manage hooks"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = context.args.trim();
        let (command, rest) = split_word(args);
        match command.unwrap_or("status") {
            "" | "status" | "list" => {
                reject_unexpected_rest("hooks status", rest)?;
                Ok(CommandResult::text(hooks_status()))
            }
            "path" => {
                reject_unexpected_rest("hooks path", rest)?;
                Ok(CommandResult::text(hooks_file_path().display().to_string()))
            }
            "json" => {
                reject_unexpected_rest("hooks json", rest)?;
                Ok(CommandResult::text(serde_json::to_string_pretty(
                    &read_hooks_file()?,
                )?))
            }
            "add" => add_hook(rest),
            "remove" | "rm" => remove_hook(rest),
            "clear" => clear_hooks(rest),
            "help" | "--help" | "-h" => {
                reject_unexpected_rest("hooks help", rest)?;
                Ok(CommandResult::text(usage()))
            }
            other => Err(anyhow!("unknown hooks command '{}'\n\n{}", other, usage())),
        }
    }
}

fn reject_unexpected_rest(command: &str, rest: &str) -> Result<()> {
    if rest.trim().is_empty() {
        return Ok(());
    }
    Err(anyhow!("usage: kiana {command}\n\n{}", usage()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookSlot {
    Stop,
    PreToolUse,
    PostToolUse,
    SessionStart,
    UserPromptSubmit,
    TaskCompleted,
    TeammateIdle,
    Fallback,
}

const HOOK_SLOTS: [HookSlot; 8] = [
    HookSlot::Stop,
    HookSlot::PreToolUse,
    HookSlot::PostToolUse,
    HookSlot::SessionStart,
    HookSlot::UserPromptSubmit,
    HookSlot::TaskCompleted,
    HookSlot::TeammateIdle,
    HookSlot::Fallback,
];

type HooksFile = HookConfig;

impl HookSlot {
    fn key(self) -> &'static str {
        match self {
            HookSlot::Stop => "Stop",
            HookSlot::PreToolUse => "PreToolUse",
            HookSlot::PostToolUse => "PostToolUse",
            HookSlot::SessionStart => "SessionStart",
            HookSlot::UserPromptSubmit => "UserPromptSubmit",
            HookSlot::TaskCompleted => "TaskCompleted",
            HookSlot::TeammateIdle => "TeammateIdle",
            HookSlot::Fallback => "hooks",
        }
    }

    fn label(self) -> &'static str {
        match self {
            HookSlot::Stop => "stop",
            HookSlot::PreToolUse => "pre_tool_use",
            HookSlot::PostToolUse => "post_tool_use",
            HookSlot::SessionStart => "session_start",
            HookSlot::UserPromptSubmit => "user_prompt_submit",
            HookSlot::TaskCompleted => "task_completed",
            HookSlot::TeammateIdle => "teammate_idle",
            HookSlot::Fallback => "fallback",
        }
    }

    fn env_name(self) -> &'static str {
        match self {
            HookSlot::Stop => "KIANA_STOP_HOOKS",
            HookSlot::PreToolUse => "KIANA_PRE_TOOL_USE_HOOKS",
            HookSlot::PostToolUse => "KIANA_POST_TOOL_USE_HOOKS",
            HookSlot::SessionStart => "KIANA_SESSION_START_HOOKS",
            HookSlot::UserPromptSubmit => "KIANA_USER_PROMPT_SUBMIT_HOOKS",
            HookSlot::TaskCompleted => "KIANA_TASK_COMPLETED_HOOKS",
            HookSlot::TeammateIdle => "KIANA_TEAMMATE_IDLE_HOOKS",
            HookSlot::Fallback => "KIANA_HOOKS",
        }
    }
}

fn hooks_status() -> String {
    let path = hooks_file_path();
    let (file, file_error) = match read_hooks_file() {
        Ok(file) => (file, None),
        Err(error) => (HooksFile::new(), Some(error.to_string())),
    };

    let mut lines = vec![
        "Hooks status".to_string(),
        format!(
            "file: {} ({})",
            path.display(),
            if path.is_file() { "found" } else { "missing" }
        ),
    ];

    for slot in HOOK_SLOTS {
        lines.push(format!(
            "effective_{}_hooks: {}",
            slot.label(),
            effective_hook_count(slot, &file)
        ));
        lines.push(format!(
            "file_{}_hooks: {}",
            slot.label(),
            file_hook_count(slot, &file)
        ));
        lines.push(format!(
            "env_{}_hooks: {}",
            slot.label(),
            hook_count_from_env(slot.env_name()).unwrap_or(0)
        ));
    }

    if let Some(error) = file_error {
        lines.push(format!("file_error: {error}"));
    }
    lines.push(
        "usage: kiana hooks add <stop|pre-tool-use|post-tool-use|session-start|user-prompt-submit|task-completed|teammate-idle|fallback> <command>".to_string(),
    );
    lines.join("\n")
}

fn add_hook(rest: &str) -> Result<CommandResult> {
    let (event, command) = parse_event_and_tail(rest, "hooks add")?;
    let command = command.trim();
    if command.is_empty() {
        return Err(anyhow!("hooks add requires a command\n\n{}", usage()));
    }

    let mut file = read_hooks_file()?;
    let count = {
        let commands = file.entry(event.key().to_string()).or_default();
        commands.push(command.to_string());
        commands.len()
    };
    let path = write_hooks_file(&file)?;
    Ok(CommandResult::text(format!(
        "Hook added\nevent: {}\ncount: {}\nfile: {}",
        event.label(),
        count,
        path.display()
    )))
}

fn remove_hook(rest: &str) -> Result<CommandResult> {
    let (event, selector) = parse_event_and_tail(rest, "hooks remove")?;
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(anyhow!(
            "hooks remove requires a 1-based index or exact command\n\n{}",
            usage()
        ));
    }

    let mut file = read_hooks_file()?;
    let commands = file.entry(event.key().to_string()).or_default();
    let removed = if let Ok(index) = selector.parse::<usize>() {
        if index == 0 || index > commands.len() {
            return Err(anyhow!(
                "hook index {} is out of range for {} (count={})",
                index,
                event.label(),
                commands.len()
            ));
        }
        commands.remove(index - 1)
    } else {
        let Some(position) = commands.iter().position(|command| command == selector) else {
            return Err(anyhow!("hook command not found for {}", event.label()));
        };
        commands.remove(position)
    };

    prune_empty_events(&mut file);
    let path = write_hooks_file(&file)?;
    Ok(CommandResult::text(format!(
        "Hook removed\nevent: {}\nremoved: {}\nremaining: {}\nfile: {}",
        event.label(),
        removed,
        file_hook_count(event, &file),
        path.display()
    )))
}

fn clear_hooks(rest: &str) -> Result<CommandResult> {
    let mut file = read_hooks_file()?;
    let trimmed = rest.trim();
    let cleared = if trimmed.is_empty() || matches!(trimmed, "all" | "*") {
        let count = file.values().map(Vec::len).sum::<usize>();
        file.clear();
        count
    } else {
        let event = parse_slot(trimmed)?;
        file.remove(event.key())
            .map(|commands| commands.len())
            .unwrap_or(0)
    };

    let path = write_hooks_file(&file)?;
    Ok(CommandResult::text(format!(
        "Hooks cleared\nremoved: {}\nfile: {}",
        cleared,
        path.display()
    )))
}

fn split_word(value: &str) -> (Option<&str>, &str) {
    let value = value.trim_start();
    if value.is_empty() {
        return (None, "");
    }
    let Some((word, rest)) = value.split_once(char::is_whitespace) else {
        return (Some(value), "");
    };
    (Some(word), rest.trim_start())
}

fn parse_event_and_tail<'a>(value: &'a str, usage_name: &str) -> Result<(HookSlot, &'a str)> {
    let (event, rest) = split_word(value);
    let event = event
        .ok_or_else(|| anyhow!("{usage_name} requires an event\n\n{}", usage()))
        .and_then(parse_slot)?;
    Ok((event, rest))
}

fn parse_slot(value: &str) -> Result<HookSlot> {
    match value {
        "Stop" | "stop" | "stop-hooks" | "stop_hooks" | "KIANA_STOP_HOOKS" => Ok(HookSlot::Stop),
        "PreToolUse"
        | "preToolUse"
        | "pre-tool-use"
        | "pre_tool_use"
        | "pre-tool-use-hooks"
        | "pre_tool_use_hooks"
        | "KIANA_PRE_TOOL_USE_HOOKS" => Ok(HookSlot::PreToolUse),
        "PostToolUse"
        | "postToolUse"
        | "post-tool-use"
        | "post_tool_use"
        | "post-tool-use-hooks"
        | "post_tool_use_hooks"
        | "KIANA_POST_TOOL_USE_HOOKS" => Ok(HookSlot::PostToolUse),
        "SessionStart"
        | "sessionStart"
        | "session-start"
        | "session_start"
        | "session-start-hooks"
        | "session_start_hooks"
        | "KIANA_SESSION_START_HOOKS" => Ok(HookSlot::SessionStart),
        "UserPromptSubmit"
        | "userPromptSubmit"
        | "user-prompt-submit"
        | "user_prompt_submit"
        | "user-prompt-submit-hooks"
        | "user_prompt_submit_hooks"
        | "KIANA_USER_PROMPT_SUBMIT_HOOKS" => Ok(HookSlot::UserPromptSubmit),
        "TaskCompleted"
        | "taskCompleted"
        | "task-completed"
        | "task_completed"
        | "completed"
        | "KIANA_TASK_COMPLETED_HOOKS" => Ok(HookSlot::TaskCompleted),
        "TeammateIdle"
        | "teammateIdle"
        | "teammate-idle"
        | "teammate_idle"
        | "idle"
        | "KIANA_TEAMMATE_IDLE_HOOKS" => Ok(HookSlot::TeammateIdle),
        "hooks" | "fallback" | "all-hooks" | "all_hooks" | "KIANA_HOOKS" => Ok(HookSlot::Fallback),
        _ => Err(anyhow!(
            "unknown hook event '{}'; expected stop, pre-tool-use, post-tool-use, session-start, user-prompt-submit, task-completed, teammate-idle, or fallback",
            value
        )),
    }
}

fn hooks_file_path() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_HOOKS_FILE") {
        return PathBuf::from(path);
    }
    kiana_home_dir().join("hooks.json")
}

fn read_hooks_file() -> Result<HooksFile> {
    let path = hooks_file_path();
    if !path.is_file() {
        return Ok(HooksFile::new());
    }
    let contents = std::fs::read_to_string(&path)?;
    let value = serde_json::from_str::<Value>(&contents)
        .map_err(|error| anyhow!("failed to parse {}: {}", path.display(), error))?;
    parse_hook_config(value).map_err(|error| {
        anyhow!(
            "invalid hook schema in {} at {}: {}",
            path.display(),
            error.location,
            error.message
        )
    })
}

fn write_hooks_file(file: &HooksFile) -> Result<PathBuf> {
    let path = hooks_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(file)?))?;
    Ok(path)
}

fn effective_hook_count(slot: HookSlot, file: &HooksFile) -> usize {
    if let Some(count) = hook_count_from_env(slot.env_name()) {
        return count;
    }
    if slot != HookSlot::Fallback {
        if let Some(count) = hook_count_from_env(HookSlot::Fallback.env_name()) {
            return count;
        }
    }
    file_commands_for(slot, file).map(Vec::len).unwrap_or(0)
}

fn file_hook_count(slot: HookSlot, file: &HooksFile) -> usize {
    file.get(slot.key()).map(Vec::len).unwrap_or(0)
}

fn file_commands_for<'a>(slot: HookSlot, file: &'a HooksFile) -> Option<&'a Vec<String>> {
    if let Some(commands) = file.get(slot.key()) {
        return Some(commands);
    }
    if slot != HookSlot::Fallback {
        return file.get(HookSlot::Fallback.key());
    }
    None
}

fn hook_count_from_env(env_name: &str) -> Option<usize> {
    std::env::var(env_name)
        .ok()
        .and_then(|value| parse_hook_commands(&value).ok())
        .map(|commands| commands.len())
}

fn prune_empty_events(file: &mut HooksFile) {
    file.retain(|_, commands| !commands.is_empty());
}

fn usage() -> &'static str {
    "Usage:\n  kiana hooks [status|list]\n  kiana hooks path\n  kiana hooks json\n  kiana hooks add <stop|pre-tool-use|post-tool-use|session-start|user-prompt-submit|task-completed|teammate-idle|fallback> <command>\n  kiana hooks remove <event> <index|exact-command>\n  kiana hooks clear [event|all]"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_state::env_lock;
    use crate::Command;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_hooks_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-hooks-command-{}-{unique}.json",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::new(),
        }
    }

    fn clear_hook_env() {
        for key in [
            "KIANA_HOOKS",
            "KIANA_STOP_HOOKS",
            "KIANA_PRE_TOOL_USE_HOOKS",
            "KIANA_POST_TOOL_USE_HOOKS",
            "KIANA_SESSION_START_HOOKS",
            "KIANA_USER_PROMPT_SUBMIT_HOOKS",
            "KIANA_TASK_COMPLETED_HOOKS",
            "KIANA_TEAMMATE_IDLE_HOOKS",
            "KIANA_HOOKS_FILE",
        ] {
            std::env::remove_var(key);
        }
    }

    #[tokio::test]
    async fn hooks_reports_configured_stop_hooks() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        std::env::set_var(
            "KIANA_STOP_HOOKS",
            serde_json::to_string(&vec!["echo one", "echo two"]).unwrap(),
        );

        let result = HooksCommand.execute(context("status")).await.unwrap();

        assert!(result.value.contains("effective_stop_hooks: 2"));
        assert!(result.value.contains("env_stop_hooks: 2"));

        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_add_remove_and_clear_persist_file() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let added = HooksCommand
            .execute(context("add stop echo from file"))
            .await
            .unwrap();
        assert!(added.value.contains("event: stop"));
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("echo from file"));

        let status = HooksCommand.execute(context("status")).await.unwrap();
        assert!(status.value.contains("effective_stop_hooks: 1"));
        assert!(status.value.contains("file_stop_hooks: 1"));

        let removed = HooksCommand
            .execute(context("remove stop 1"))
            .await
            .unwrap();
        assert!(removed.value.contains("remaining: 0"));

        HooksCommand
            .execute(context("add fallback echo fallback"))
            .await
            .unwrap();
        let cleared = HooksCommand.execute(context("clear all")).await.unwrap();
        assert!(cleared.value.contains("removed: 1"));
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "{}");

        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_adds_pre_tool_use_hook_to_persistent_file() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let added = HooksCommand
            .execute(context("add pre-tool-use echo guard"))
            .await
            .unwrap();

        assert!(added.value.contains("event: pre_tool_use"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"PreToolUse\""));
        assert!(saved.contains("echo guard"));
        let status = HooksCommand.execute(context("status")).await.unwrap();
        assert!(status.value.contains("effective_pre_tool_use_hooks: 1"));
        assert!(status.value.contains("file_pre_tool_use_hooks: 1"));

        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_adds_post_tool_use_hook_to_persistent_file() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let added = HooksCommand
            .execute(context("add post-tool-use echo audit"))
            .await
            .unwrap();

        assert!(added.value.contains("event: post_tool_use"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"PostToolUse\""));
        assert!(saved.contains("echo audit"));
        let status = HooksCommand.execute(context("status")).await.unwrap();
        assert!(status.value.contains("effective_post_tool_use_hooks: 1"));
        assert!(status.value.contains("file_post_tool_use_hooks: 1"));

        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_adds_session_start_hook_to_persistent_file() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let added = HooksCommand
            .execute(context("add session-start echo context"))
            .await
            .unwrap();

        assert!(added.value.contains("event: session_start"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"SessionStart\""));
        assert!(saved.contains("echo context"));
        let status = HooksCommand.execute(context("status")).await.unwrap();
        assert!(status.value.contains("effective_session_start_hooks: 1"));
        assert!(status.value.contains("file_session_start_hooks: 1"));

        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_adds_user_prompt_submit_hook_to_persistent_file() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let added = HooksCommand
            .execute(context("add user-prompt-submit echo inspect"))
            .await
            .unwrap();

        assert!(added.value.contains("event: user_prompt_submit"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"UserPromptSubmit\""));
        assert!(saved.contains("echo inspect"));
        let status = HooksCommand.execute(context("status")).await.unwrap();
        assert!(status
            .value
            .contains("effective_user_prompt_submit_hooks: 1"));
        assert!(status.value.contains("file_user_prompt_submit_hooks: 1"));

        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_json_normalizes_legacy_array_to_fallback() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::fs::write(&path, r#"["echo legacy"]"#).unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let json = HooksCommand.execute(context("json")).await.unwrap();

        assert!(json.value.contains("\"hooks\""));
        assert!(json.value.contains("echo legacy"));

        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_json_rejects_malformed_hook_schema() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::fs::write(&path, r#"{"Stop":[{"command":"echo nested"}]}"#).unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        let error = HooksCommand
            .execute(context("json"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("invalid hook schema"), "{error}");
        assert!(error.contains("Stop[0]"), "{error}");
        assert!(error.contains(path.to_string_lossy().as_ref()), "{error}");
        let _ = std::fs::remove_file(path);
        clear_hook_env();
    }

    #[tokio::test]
    async fn hooks_read_only_subcommands_reject_extra_args() {
        let _guard = env_lock().lock().unwrap();
        clear_hook_env();
        let path = temp_hooks_path();
        std::env::set_var("KIANA_HOOKS_FILE", &path);

        for args in [
            "status now",
            "list now",
            "path extra",
            "json stop",
            "help now",
        ] {
            let error = HooksCommand
                .execute(context(args))
                .await
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("usage: kiana hooks"),
                "{args} returned wrong error: {error}"
            );
        }
        assert!(!path.exists());

        clear_hook_env();
    }
}
