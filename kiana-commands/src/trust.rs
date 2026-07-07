use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_types::{
    find_project_trust_file, project_trust_file_path, project_trust_from_app_state,
    read_project_trust, remove_project_trust, write_project_trust, ProjectTrust,
};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct TrustCommand;

#[async_trait]
impl Command for TrustCommand {
    fn name(&self) -> &str {
        "trust"
    }

    fn description(&self) -> &str {
        "Manage project trust"
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
                reject_unexpected_rest("trust status", rest)?;
                status(&context)
            }
            "path" => {
                reject_unexpected_rest("trust path", rest)?;
                Ok(CommandResult::text(
                    effective_file_path(&cwd(&context)).display().to_string(),
                ))
            }
            "json" => {
                reject_unexpected_rest("trust json", rest)?;
                status_json(&context)
            }
            "trust" | "trusted" | "allow" | "enable" => {
                reject_unexpected_rest("trust trust", rest)?;
                set_trust(&context, ProjectTrust::Trusted)
            }
            "untrust" | "untrusted" | "deny" | "disable" => {
                reject_unexpected_rest("trust untrust", rest)?;
                set_trust(&context, ProjectTrust::Untrusted)
            }
            "reset" | "clear" => {
                reject_unexpected_rest("trust reset", rest)?;
                reset_trust(&context)
            }
            "help" | "--help" | "-h" => {
                reject_unexpected_rest("trust help", rest)?;
                Ok(CommandResult::text(usage()))
            }
            other => Err(anyhow!("unknown trust command '{}'\n\n{}", other, usage())),
        }
    }
}

fn status(context: &CommandContext) -> Result<CommandResult> {
    let cwd = cwd(context);
    let mut state = context.app_state.clone();
    state.entry("cwd".to_string()).or_insert_with(|| json!(cwd));
    let trust = project_trust_from_app_state(&state);
    let file_path = effective_file_path(&cwd);
    let file_status = if file_path.is_file() {
        "found"
    } else {
        "missing"
    };
    let source = trust_source(context, &cwd);
    Ok(CommandResult::text(
        [
            "Project trust status".to_string(),
            format!("project_trust: {}", trust.as_str()),
            format!("source: {source}"),
            format!("file: {} ({file_status})", file_path.display()),
            "usage: kiana trust trust | untrust | reset | status".to_string(),
        ]
        .join("\n"),
    ))
}

fn status_json(context: &CommandContext) -> Result<CommandResult> {
    let cwd = cwd(context);
    let mut state = context.app_state.clone();
    state.entry("cwd".to_string()).or_insert_with(|| json!(cwd));
    let trust = project_trust_from_app_state(&state);
    let file_path = effective_file_path(&cwd);
    Ok(CommandResult::text(serde_json::to_string_pretty(&json!({
        "project_trust": trust.as_str(),
        "project_trusted": trust.as_bool(),
        "source": trust_source(context, &cwd),
        "file": file_path,
        "file_exists": file_path.is_file()
    }))?))
}

fn set_trust(context: &CommandContext, project_trust: ProjectTrust) -> Result<CommandResult> {
    let cwd = cwd(context);
    let path = write_project_trust(&cwd, project_trust).map_err(anyhow::Error::msg)?;
    Ok(CommandResult::text(format!(
        "Project trust updated\nproject_trust: {}\nfile: {}",
        project_trust.as_str(),
        path.display()
    )))
}

fn reset_trust(context: &CommandContext) -> Result<CommandResult> {
    let cwd = cwd(context);
    let path = remove_project_trust(&cwd).map_err(anyhow::Error::msg)?;
    let file = path.unwrap_or_else(|| project_trust_file_path(cwd));
    Ok(CommandResult::text(format!(
        "Project trust reset\nproject_trust: {}\nfile: {}",
        ProjectTrust::Trusted.as_str(),
        file.display()
    )))
}

fn trust_source(context: &CommandContext, cwd: &std::path::Path) -> &'static str {
    if has_app_state_project_trust(&context.app_state) {
        "session"
    } else if read_project_trust(cwd).ok().flatten().is_some() {
        "file"
    } else {
        "default"
    }
}

fn has_app_state_project_trust(app_state: &std::collections::HashMap<String, Value>) -> bool {
    app_state.contains_key("project_trusted")
        || app_state.contains_key("projectTrusted")
        || app_state
            .get("trust")
            .and_then(Value::as_object)
            .is_some_and(|trust| trust.contains_key("project"))
        || app_state
            .get("project")
            .and_then(Value::as_object)
            .is_some_and(|project| project.contains_key("trusted"))
}

fn effective_file_path(cwd: &std::path::Path) -> PathBuf {
    find_project_trust_file(cwd).unwrap_or_else(|| project_trust_file_path(cwd))
}

fn cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn split_word(value: &str) -> (Option<&str>, &str) {
    let value = value.trim_start();
    if value.is_empty() {
        return (None, "");
    }
    if let Some(index) = value.find(char::is_whitespace) {
        (Some(&value[..index]), value[index..].trim_start())
    } else {
        (Some(value), "")
    }
}

fn reject_unexpected_rest(command: &str, rest: &str) -> Result<()> {
    if rest.trim().is_empty() {
        return Ok(());
    }
    Err(anyhow!("usage: kiana {command}\n\n{}", usage()))
}

fn usage() -> &'static str {
    "Usage:\n  kiana trust [status|list]\n  kiana trust trust\n  kiana trust untrust\n  kiana trust reset\n  kiana trust path\n  kiana trust json"
}

#[cfg(test)]
mod tests {
    use super::TrustCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use kiana_types::{project_trust_from_app_state, ProjectTrust};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn context(args: &str, cwd: &std::path::Path) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
        }
    }

    #[tokio::test]
    async fn trust_untrust_persists_project_trust_for_app_state_contract() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-trust-command-{}-{unique}",
            std::process::id()
        ));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();

        let result = TrustCommand
            .execute(context("untrust", &project))
            .await
            .unwrap();
        assert!(result.value.contains("project_trust: untrusted"));
        let app_state = HashMap::from([("cwd".to_string(), json!(project.clone()))]);
        assert_eq!(
            project_trust_from_app_state(&app_state),
            ProjectTrust::Untrusted
        );

        let status = TrustCommand
            .execute(context("status", &project))
            .await
            .unwrap();
        assert!(status.value.contains("project_trust: untrusted"));
        assert!(status.value.contains("source: file"));

        let reset = TrustCommand
            .execute(context("reset", &project))
            .await
            .unwrap();
        assert!(reset.value.contains("project_trust: trusted"));
        let app_state = HashMap::from([("cwd".to_string(), json!(project))]);
        assert_eq!(
            project_trust_from_app_state(&app_state),
            ProjectTrust::Trusted
        );

        let _ = fs::remove_dir_all(root);
    }
}
