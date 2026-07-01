use anyhow::Result;
use colored::*;
use kiana_commands::{create_default_command_registry, CommandContext, CommandType};
use rustyline::DefaultEditor;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub async fn run_repl() -> Result<()> {
    let command_registry = create_default_command_registry();
    let mut editor = DefaultEditor::new()?;
    let cwd = std::env::current_dir()?;
    let mut app_state = HashMap::new();
    let mut session_id = create_repl_session(&cwd).await?;

    println!("{}", "━".repeat(60).bright_black());
    println!("{}", "Kiana REPL".bright_cyan().bold());
    println!("{} Session: {}", "ℹ".bright_blue(), session_id);
    println!("{} Press Ctrl+C to exit", "ℹ".bright_blue());
    println!("{}", "━".repeat(60).bright_black());
    println!();

    loop {
        let mut input = match editor.readline(&format!("{} ", "You:".bright_green().bold())) {
            Ok(line) => line,
            Err(_) => break,
        };

        if input.trim().is_empty() {
            continue;
        }

        editor.add_history_entry(&input)?;

        if let Some(command_line) = input.trim().strip_prefix('/') {
            let mut parts = command_line.splitn(2, char::is_whitespace);
            let name = parts.next().unwrap_or_default();
            let args = parts.next().unwrap_or_default().trim().to_string();

            if let Some(command) = command_registry.get(name) {
                let command_state = command_app_state(&app_state, &session_id, &cwd);
                let should_clear_session = is_clear_session_command(name, &args);
                let result = command
                    .execute(CommandContext {
                        args,
                        app_state: command_state,
                    })
                    .await?;

                if result.output_type == "exit" {
                    if !result.value.is_empty() {
                        println!("{} {}", "Kiana:".bright_cyan().bold(), result.value);
                    }
                    break;
                }

                match command.command_type() {
                    CommandType::Prompt => {
                        input = result.value;
                    }
                    CommandType::Local | CommandType::LocalJsx => {
                        if should_clear_session {
                            session_id = create_repl_session(&cwd).await?;
                            app_state.clear();
                            println!(
                                "{} New session: {}",
                                "Kiana:".bright_cyan().bold(),
                                session_id
                            );
                        } else if !result.value.is_empty() {
                            println!("{} {}", "Kiana:".bright_cyan().bold(), result.value);
                        }
                        println!();
                        continue;
                    }
                }
            } else {
                println!(
                    "{} Unknown command: /{}. Try /help.",
                    "Kiana:".bright_cyan().bold(),
                    name
                );
                println!();
                continue;
            }
        }

        println!("{} Running...", "Kiana:".bright_cyan().bold());
        match run_repl_prompt(&session_id, &cwd, input).await {
            Ok(text) if text.trim().is_empty() => {
                println!(
                    "{} Completed with no assistant text.",
                    "Kiana:".bright_cyan().bold()
                );
            }
            Ok(text) => {
                println!("{} {}", "Assistant:".bright_cyan().bold(), text);
            }
            Err(error) => {
                eprintln!("{} {}", "✗".red().bold(), format!("Error: {}", error).red());
            }
        }
        println!();
    }

    Ok(())
}

async fn create_repl_session(cwd: &Path) -> Result<String> {
    let mut options = HashMap::new();
    options.insert(
        "title".to_string(),
        Value::String(repl_session_title(cwd).to_string()),
    );
    options.insert("tag".to_string(), Value::String("repl".to_string()));
    let session = crate::sdk::unstable_v2_create_session(options).await?;
    Ok(session.session_id)
}

async fn run_repl_prompt(session_id: &str, cwd: &Path, input: String) -> Result<String> {
    let mut options = repl_prompt_options(session_id, cwd);
    options.insert("execute".to_string(), Value::Bool(true));
    let result = crate::sdk::unstable_v2_prompt(input, options).await?;
    Ok(assistant_text_from_result(&result))
}

fn repl_prompt_options(session_id: &str, cwd: &Path) -> HashMap<String, Value> {
    HashMap::from([
        (
            "session_id".to_string(),
            Value::String(session_id.to_string()),
        ),
        (
            "cwd".to_string(),
            Value::String(cwd.to_string_lossy().to_string()),
        ),
    ])
}

fn repl_session_title(cwd: &Path) -> String {
    let name = cwd
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(".");
    format!("REPL {}", name)
}

fn assistant_text_from_result(result: &Value) -> String {
    result
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn command_app_state(
    app_state: &HashMap<String, Value>,
    session_id: &str,
    cwd: &Path,
) -> HashMap<String, Value> {
    let mut state = app_state.clone();
    state.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    state.insert(
        "cwd".to_string(),
        Value::String(cwd.to_string_lossy().to_string()),
    );
    state
}

fn is_clear_session_command(name: &str, args: &str) -> bool {
    name == "clear" && args.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repl_prompt_options_include_session_and_cwd() {
        let options = repl_prompt_options("session-1", Path::new("/tmp/work"));

        assert_eq!(options["session_id"], json!("session-1"));
        assert_eq!(options["cwd"], json!("/tmp/work"));
        assert!(options.get("execute").is_none());
    }

    #[test]
    fn repl_session_title_uses_cwd_name() {
        assert_eq!(
            repl_session_title(Path::new("/tmp/kianacode")),
            "REPL kianacode"
        );
        assert_eq!(repl_session_title(Path::new("/")), "REPL .");
    }

    #[test]
    fn assistant_text_defaults_to_empty_string() {
        assert_eq!(
            assistant_text_from_result(&json!({"assistant_text": "done"})),
            "done"
        );
        assert_eq!(assistant_text_from_result(&json!({})), "");
    }

    #[test]
    fn command_app_state_includes_current_session_context() {
        let state = command_app_state(&HashMap::new(), "session-1", Path::new("/tmp/work"));

        assert_eq!(state["session_id"], json!("session-1"));
        assert_eq!(state["cwd"], json!("/tmp/work"));
    }

    #[test]
    fn clear_session_command_requires_no_args() {
        assert!(is_clear_session_command("clear", ""));
        assert!(is_clear_session_command("clear", "   "));
        assert!(!is_clear_session_command("clear", "status"));
        assert!(!is_clear_session_command("clear", "--help"));
        assert!(!is_clear_session_command("status", ""));
    }
}
