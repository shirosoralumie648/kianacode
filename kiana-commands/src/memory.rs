use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use std::io::Write;

pub struct MemoryCommand;

#[async_trait]
impl Command for MemoryCommand {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Manage memory"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = context.args.trim();
        let path = kiana_home_dir().join("memory.md");
        let (command, rest) = split_command(args);
        match command {
            None | Some("status") => {
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "unknown memory status command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                Ok(CommandResult::text(format!(
                    "Memory\nfile: {}\nbytes: {}\nusage: kiana memory show|append <text>|clear",
                    path.display(),
                    bytes
                )))
            }
            Some("help") | Some("--help") | Some("-h") => {
                if rest.is_empty() {
                    Ok(CommandResult::text(usage()))
                } else {
                    Err(anyhow!(
                        "unknown memory help command '{}'\n\n{}",
                        rest,
                        usage()
                    ))
                }
            }
            Some("path") => {
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "unknown memory path command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                Ok(CommandResult::text(path.display().to_string()))
            }
            Some("show") => {
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "unknown memory show command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                Ok(CommandResult::text(text))
            }
            Some("append") => {
                let text = rest;
                if text.is_empty() {
                    return Err(anyhow!("memory append requires text"));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)?;
                writeln!(file, "{}", text)?;
                Ok(CommandResult::text(format!(
                    "Memory updated\nfile: {}",
                    path.display()
                )))
            }
            Some("clear") => {
                if matches!(rest, "help" | "--help" | "-h") {
                    return Ok(CommandResult::text(usage()));
                }
                if !rest.is_empty() {
                    return Err(anyhow!(
                        "unknown memory clear command '{}'\n\n{}",
                        rest,
                        usage()
                    ));
                }
                if path.exists() {
                    std::fs::remove_file(&path)?;
                }
                Ok(CommandResult::text(format!(
                    "Memory cleared\nfile: {}",
                    path.display()
                )))
            }
            Some(other) => Err(anyhow!(
                "unknown memory command '{}'; expected status, path, show, append, or clear",
                other
            )),
        }
    }
}

fn split_command(value: &str) -> (Option<&str>, &str) {
    let mut parts = value.splitn(2, char::is_whitespace);
    let command = parts.next().filter(|value| !value.is_empty());
    let rest = parts.next().unwrap_or_default().trim();
    (command, rest)
}

fn usage() -> &'static str {
    "Usage: kiana memory [status]\n       kiana memory path\n       kiana memory show\n       kiana memory append <text>\n       kiana memory clear"
}

#[cfg(test)]
mod tests {
    use super::MemoryCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{MutexGuard, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-memory-command-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_memory(root: &std::path::Path) -> std::path::PathBuf {
        std::env::set_var("KIANA_HOME", root);
        fs::create_dir_all(root).unwrap();
        let path = root.join("memory.md");
        fs::write(&path, "important memory\n").unwrap();
        path
    }

    #[tokio::test]
    async fn memory_clear_rejects_extra_args_without_deleting_file() {
        let _guard = lock_env();
        let root = temp_home();
        let path = write_memory(&root);

        let error = MemoryCommand
            .execute(context("clear status"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown memory clear command"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("important memory"));

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn memory_clear_help_reports_usage_without_deleting_file() {
        let _guard = lock_env();
        let root = temp_home();
        let path = write_memory(&root);

        let result = MemoryCommand
            .execute(context("clear --help"))
            .await
            .unwrap();

        assert!(result.value.contains("Usage: kiana memory"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("important memory"));

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }
}
