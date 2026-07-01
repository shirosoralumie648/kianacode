use crate::local_state::sdk_sessions_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct SessionCommand;

#[async_trait]
impl Command for SessionCommand {
    fn name(&self) -> &str {
        "session"
    }

    fn description(&self) -> &str {
        "Manage sessions"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("list") {
            "" | "list" | "status" => {
                reject_unexpected_rest("session list", rest)?;
                list_sessions(current_session_id(&context.app_state))
            }
            "path" => {
                reject_unexpected_rest("session path", rest)?;
                Ok(CommandResult::text(
                    sdk_sessions_dir().display().to_string(),
                ))
            }
            "current" => {
                reject_unexpected_rest("session current", rest)?;
                show_current_session(&context.app_state)
            }
            "show" | "json" => show_session(rest, &context.app_state),
            "rename" => rename_session(rest, &context.app_state),
            "tag" => tag_session(rest, &context.app_state),
            "files" | "file-set" | "file-sets" => session_files(rest, &context.app_state),
            "fork" => fork_session(rest, &context.app_state),
            "delete" | "rm" => delete_session(rest, &context.app_state),
            "reply" => reply_session(rest, &context.app_state),
            "import" => import_session(rest),
            "export" => export_session(rest, &context.app_state).await,
            "compact" => compact_session(rest, &context.app_state).await,
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!(
                "unknown session command '{}'\n\n{}",
                other,
                usage()
            )),
        }
    }
}

fn reject_unexpected_rest(command: &str, rest: &str) -> Result<()> {
    if rest.trim().is_empty() {
        return Ok(());
    }
    Err(anyhow!("usage: kiana {command}\n\n{}", usage()))
}

fn list_sessions(current_session_id: Option<String>) -> Result<CommandResult> {
    let dir = sdk_sessions_dir();
    let sessions = read_all_sessions(&dir)?;
    if sessions.is_empty() {
        return Ok(CommandResult::text(format!(
            "No SDK sessions found.\nsessions_dir: {}",
            dir.display()
        )));
    }

    let mut lines = vec![format!("{} SDK session(s):", sessions.len())];
    for session in sessions.iter().take(20) {
        let id = session_id(session).unwrap_or("<unknown>");
        let marker = if current_session_id.as_deref() == Some(id) {
            "*"
        } else {
            "-"
        };
        let title = session
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("(untitled)");
        let tag = session.get("tag").and_then(Value::as_str).unwrap_or("-");
        let message_count = message_count(session);
        let updated_at = session
            .get("updated_at")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        lines.push(format!(
            "{} {}\t{}\ttag={}\tmessages={}\tupdated={}",
            marker, id, title, tag, message_count, updated_at
        ));
    }
    if sessions.len() > 20 {
        lines.push(format!("... {} more", sessions.len() - 20));
    }
    lines.push("usage: kiana session show [id] | reply [id] --record-only <msg> | rename [id] <title> | tag [id] [tag] | files [id] | fork [id] | import <path> | export [id] | compact [id] | delete [id]".to_string());

    Ok(CommandResult::text(lines.join("\n")))
}

fn show_current_session(
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let session_id = require_current_session_id(app_state)?;
    let session = read_session(&session_id)?;
    Ok(CommandResult::text(format!(
        "Current session\nid: {}\ntitle: {}\ntag: {}\nmessages: {}\nupdated: {}",
        session_id,
        session
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("(untitled)"),
        session.get("tag").and_then(Value::as_str).unwrap_or("-"),
        message_count(&session),
        session
            .get("updated_at")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    )))
}

fn show_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let session_id = session_id_arg(rest, app_state)?;
    let session = read_session(&session_id)?;
    Ok(CommandResult::text(serde_json::to_string_pretty(&session)?))
}

fn rename_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let (session_id, title) = session_id_and_tail(rest, app_state, "session rename")?;
    let title = title.trim();
    if title.is_empty() {
        return Err(anyhow!("session rename requires a title\n\n{}", usage()));
    }
    let mut session = read_session(&session_id)?;
    session["title"] = json!(title);
    session["updated_at"] = json!(now_unix_seconds());
    write_session(&session_id, &session)?;
    Ok(CommandResult::text(format!(
        "Session renamed\nid: {}\ntitle: {}",
        session_id, title
    )))
}

fn tag_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let (session_id, tag) = session_id_and_tail(rest, app_state, "session tag")?;
    let mut session = read_session(&session_id)?;
    let tag = tag.trim();
    session["tag"] = if tag.is_empty() {
        Value::Null
    } else {
        json!(tag)
    };
    session["updated_at"] = json!(now_unix_seconds());
    write_session(&session_id, &session)?;
    Ok(CommandResult::text(format!(
        "Session tagged\nid: {}\ntag: {}",
        session_id,
        if tag.is_empty() { "-" } else { tag }
    )))
}

#[derive(Debug, PartialEq, Eq)]
struct SessionFilesArgs {
    session_id: String,
    editable_files: Vec<String>,
    read_only_files: Vec<String>,
    clear_editable: bool,
    clear_read_only: bool,
    show_help: bool,
}

fn session_files(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let args = parse_session_files_args(rest, app_state)?;
    if args.show_help {
        return Ok(CommandResult::text(session_files_usage()));
    }

    let mut session = read_session(&args.session_id)?;
    let mut changed = false;
    if args.clear_editable {
        remove_session_field(&mut session, "editable_files");
        changed = true;
    }
    if args.clear_read_only {
        remove_session_field(&mut session, "read_only_files");
        changed = true;
    }
    if !args.editable_files.is_empty() {
        session["editable_files"] = json!(args.editable_files);
        changed = true;
    }
    if !args.read_only_files.is_empty() {
        session["read_only_files"] = json!(args.read_only_files);
        changed = true;
    }
    if changed {
        session["updated_at"] = json!(now_unix_seconds());
        write_session(&args.session_id, &session)?;
    }

    let editable = session_file_list(&session, "editable_files");
    let read_only = session_file_list(&session, "read_only_files");
    let heading = if changed {
        "Session file sets updated"
    } else {
        "Session file sets"
    };
    Ok(CommandResult::text(format!(
        "{heading}\nid: {}\neditable_files: {}\nread_only_files: {}",
        args.session_id,
        format_file_set(&editable),
        format_file_set(&read_only)
    )))
}

fn parse_session_files_args(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<SessionFilesArgs> {
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut session_id = None;
    let mut editable_files = Vec::new();
    let mut read_only_files = Vec::new();
    let mut clear_editable = false;
    let mut clear_read_only = false;
    let mut show_help = false;
    let mut index = 0;

    while let Some(arg) = tokens.get(index).copied() {
        match arg {
            "help" | "--help" | "-h" => show_help = true,
            "--clear" => {
                clear_editable = true;
                clear_read_only = true;
            }
            "--clear-editable" => clear_editable = true,
            "--clear-read-only" | "--clear-readonly" => clear_read_only = true,
            "--editable" | "--editable-file" | "--editableFile" => {
                let value = tokens
                    .get(index + 1)
                    .copied()
                    .filter(|value| !value.starts_with('-'))
                    .ok_or_else(|| {
                        anyhow!("{arg} requires a file path\n\n{}", session_files_usage())
                    })?;
                editable_files.push(nonempty_file_set_value(arg, value)?);
                index += 1;
            }
            "--read-only" | "--read-only-file" | "--readOnlyFile" | "--readonly"
            | "--readonly-file" | "--readonlyFile" => {
                let value = tokens
                    .get(index + 1)
                    .copied()
                    .filter(|value| !value.starts_with('-'))
                    .ok_or_else(|| {
                        anyhow!("{arg} requires a file path\n\n{}", session_files_usage())
                    })?;
                read_only_files.push(nonempty_file_set_value(arg, value)?);
                index += 1;
            }
            other if other.starts_with('-') => {
                return Err(anyhow!(
                    "unknown session files option '{}'\n\n{}",
                    other,
                    session_files_usage()
                ));
            }
            value => {
                if session_id.is_some() {
                    return Err(anyhow!(
                        "session files accepts at most one session id\n\n{}",
                        session_files_usage()
                    ));
                }
                session_id = Some(resolve_session_id_token(value, app_state)?);
            }
        }
        index += 1;
    }

    let session_id = match session_id {
        Some(session_id) => session_id,
        None if show_help => String::new(),
        None => require_current_session_id(app_state)?,
    };

    Ok(SessionFilesArgs {
        session_id,
        editable_files,
        read_only_files,
        clear_editable,
        clear_read_only,
        show_help,
    })
}

fn resolve_session_id_token(
    value: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<String> {
    if value == "current" {
        require_current_session_id(app_state)
    } else {
        validate_session_id(value)?;
        Ok(value.to_string())
    }
}

fn nonempty_file_set_value(flag: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(anyhow!("{flag} requires a non-empty file path"))
    } else {
        Ok(value.to_string())
    }
}

fn session_file_list(session: &Value, key: &str) -> Vec<String> {
    session
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn format_file_set(files: &[String]) -> String {
    if files.is_empty() {
        "<none>".to_string()
    } else {
        files.join(", ")
    }
}

fn remove_session_field(session: &mut Value, key: &str) {
    if let Some(object) = session.as_object_mut() {
        object.remove(key);
    }
}

fn fork_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let session_id = session_id_arg(rest, app_state)?;
    let source = read_session(&session_id)?;
    let forked_id = unique_session_id();
    let now = now_unix_seconds();
    let mut forked = source.clone();
    forked["session_id"] = json!(forked_id);
    forked["parent_session_id"] = json!(session_id);
    forked["created_at"] = json!(now);
    forked["updated_at"] = json!(now);
    write_session(
        forked
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        &forked,
    )?;
    let mut result = CommandResult::text(format!(
        "Session forked\nsource: {}\nforked: {}",
        session_id,
        forked["session_id"].as_str().unwrap_or_default()
    ));
    result.metadata = Some(
        [
            ("session_switch_to".to_string(), forked_id),
            ("session_source_id".to_string(), session_id),
            ("session_switch_reason".to_string(), "fork".to_string()),
        ]
        .into(),
    );
    Ok(result)
}

fn delete_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let session_id = session_id_arg(rest, app_state)?;
    if current_session_id(app_state).as_deref() == Some(session_id.as_str()) {
        return Err(anyhow!(
            "refusing to delete the active session from inside the REPL"
        ));
    }
    let path = session_file(&session_id)?;
    if !path.is_file() {
        return Err(anyhow!("session '{}' was not found", session_id));
    }
    fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(CommandResult::text(format!(
        "Session deleted\nid: {session_id}"
    )))
}

#[derive(Debug, PartialEq, Eq)]
struct ReplyArgs {
    session_id: String,
    message: String,
}

fn reply_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let args = parse_reply_args(rest, app_state)?;
    let mut session = read_session(&args.session_id)?;
    let messages = session
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("session '{}' has invalid messages", args.session_id))?;
    let now = now_unix_seconds();
    messages.push(json!({
        "role": "user",
        "content": args.message,
        "created_at": now,
    }));
    let message_count = messages.len();
    session["updated_at"] = json!(now);
    write_session(&args.session_id, &session)?;
    let mut result = CommandResult::text(format!(
        "Session reply recorded\nid: {}\nmessages: {}\nexecution: record_only",
        args.session_id, message_count
    ));
    result.metadata = Some(
        [
            ("session_id".to_string(), args.session_id),
            ("session_transcript_mutated".to_string(), "true".to_string()),
        ]
        .into(),
    );
    Ok(result)
}

fn parse_reply_args(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<ReplyArgs> {
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut record_only = false;
    let mut index = 0;
    parse_reply_flags(&tokens, &mut index, &mut record_only)?;
    let session_id = tokens.get(index).ok_or_else(|| {
        anyhow!(
            "session reply requires a session id and message\n\n{}",
            usage()
        )
    })?;
    let session_id = if *session_id == "current" {
        require_current_session_id(app_state)?
    } else {
        validate_session_id(session_id)?;
        (*session_id).to_string()
    };
    index += 1;
    parse_reply_flags(&tokens, &mut index, &mut record_only)?;
    if tokens.get(index) == Some(&"--") {
        index += 1;
    }
    if !record_only {
        return Err(anyhow!(
            "session reply in local commands only supports --record-only; use `kiana session reply <id> <message>` from the CLI to run the model"
        ));
    }
    let message = tokens.get(index..).unwrap_or_default().join(" ");
    if message.trim().is_empty() {
        return Err(anyhow!("session reply requires a message\n\n{}", usage()));
    }
    Ok(ReplyArgs {
        session_id,
        message,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct ImportArgs {
    input_path: PathBuf,
    force: bool,
    show_help: bool,
}

fn import_session(rest: &str) -> Result<CommandResult> {
    let args = parse_import_args(rest)?;
    if args.show_help {
        return Ok(CommandResult::text(import_usage()));
    }
    let contents = fs::read_to_string(&args.input_path)
        .with_context(|| format!("failed to read {}", args.input_path.display()))?;
    let session: Value = serde_json::from_str(&contents).with_context(|| {
        format!(
            "failed to parse session import {}",
            args.input_path.display()
        )
    })?;
    let session_id = imported_session_id(&session)?;
    let message_count = imported_message_count(&session)?;
    let dir = sdk_sessions_dir();
    let path = session_file(&session_id)?;
    let events_path = session_events_file(&dir, &session_id)?;
    let event_dir = events_path
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve event directory for '{}'", session_id))?;

    if !args.force && (path.exists() || events_path.exists()) {
        return Err(anyhow!(
            "session '{}' already exists; pass --force to replace it",
            session_id
        ));
    }
    if args.force && event_dir.exists() {
        if event_dir.is_dir() {
            fs::remove_dir_all(event_dir)
                .with_context(|| format!("failed to remove {}", event_dir.display()))?;
        } else {
            fs::remove_file(event_dir)
                .with_context(|| format!("failed to remove {}", event_dir.display()))?;
        }
    }

    write_session(&session_id, &session)?;

    let mut result = CommandResult::text(format!(
        "Session imported\nid: {}\nmessages: {}\nfile: {}",
        session_id,
        message_count,
        path.display()
    ));
    result.metadata = Some(
        [
            ("session_id".to_string(), session_id),
            ("session_imported".to_string(), "true".to_string()),
        ]
        .into(),
    );
    Ok(result)
}

fn parse_import_args(rest: &str) -> Result<ImportArgs> {
    let tokens = rest.split_whitespace().collect::<Vec<_>>();
    let mut input_path = None;
    let mut force = false;
    let mut show_help = false;
    let mut index = 0;

    while let Some(arg) = tokens.get(index).copied() {
        match arg {
            "" => {}
            "help" | "--help" | "-h" => show_help = true,
            "--force" | "-f" => force = true,
            other if other.starts_with('-') => {
                return Err(anyhow!(
                    "unknown session import option '{}'\n\n{}",
                    other,
                    import_usage()
                ));
            }
            value => {
                if input_path.is_some() {
                    return Err(anyhow!(
                        "session import accepts one input path\n\n{}",
                        import_usage()
                    ));
                }
                input_path = Some(PathBuf::from(value));
            }
        }
        index += 1;
    }

    let input_path = match input_path {
        Some(path) => path,
        None if show_help => PathBuf::new(),
        None => {
            return Err(anyhow!(
                "session import requires a path\n\n{}",
                import_usage()
            ))
        }
    };

    Ok(ImportArgs {
        input_path,
        force,
        show_help,
    })
}

fn imported_session_id(session: &Value) -> Result<String> {
    let session_id = session
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("imported session is missing session_id"))?;
    validate_session_id(session_id)?;
    Ok(session_id.to_string())
}

fn imported_message_count(session: &Value) -> Result<usize> {
    session
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| anyhow!("imported session is missing messages array"))
}

fn parse_reply_flags(tokens: &[&str], index: &mut usize, record_only: &mut bool) -> Result<()> {
    while let Some(arg) = tokens.get(*index).copied() {
        match arg {
            "--record-only" => *record_only = true,
            "--execute" => {
                return Err(anyhow!(
                    "session reply in local commands only supports --record-only"
                ))
            }
            _ if arg.starts_with('-') => {
                return Err(anyhow!("unknown session reply option '{}'", arg));
            }
            _ => break,
        }
        *index += 1;
    }
    Ok(())
}

async fn export_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let args = session_export_args(rest, app_state)?;
    let command = crate::export::ExportCommand;
    command
        .execute(CommandContext {
            args,
            app_state: app_state.clone(),
        })
        .await
}

async fn compact_session(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<CommandResult> {
    let command = crate::compact::CompactCommand;
    command
        .execute(CommandContext {
            args: rest.trim().to_string(),
            app_state: app_state.clone(),
        })
        .await
}

fn session_export_args(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<String> {
    let rest = rest.trim();
    let (first, tail) = split_word(rest);
    let Some(first) = first else {
        let session_id = require_current_session_id(app_state)?;
        return Ok(format!("--session {session_id}"));
    };
    if matches!(first, "help" | "--help" | "-h" | "json" | "text") || first.starts_with('-') {
        return Ok(rest.to_string());
    }
    let session_id = if first == "current" {
        require_current_session_id(app_state)?
    } else {
        validate_session_id(first)?;
        first.to_string()
    };
    if tail.is_empty() {
        Ok(format!("--session {session_id}"))
    } else {
        Ok(format!("--session {session_id} {tail}"))
    }
}

fn read_all_sessions(dir: &Path) -> Result<Vec<Value>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let mut seen_session_ids = BTreeSet::new();
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        if let Some(session_id) = session_id(&value) {
            seen_session_ids.insert(session_id.to_string());
            sessions.push(value);
        }
    }
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(session_id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if seen_session_ids.contains(session_id) || !session_events_file(dir, session_id)?.exists()
        {
            continue;
        }
        let session = read_session_from_event_tree(dir, session_id)?;
        seen_session_ids.insert(session_id.to_string());
        sessions.push(session);
    }

    sessions.sort_by(|a, b| {
        let a_time = a.get("updated_at").and_then(Value::as_u64).unwrap_or(0);
        let b_time = b.get("updated_at").and_then(Value::as_u64).unwrap_or(0);
        b_time.cmp(&a_time)
    });
    Ok(sessions)
}

fn read_session(session_id: &str) -> Result<Value> {
    validate_session_id(session_id)?;
    let path = session_file(session_id)?;
    if path.exists() {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("session '{}' was not found", session_id))?;
        return serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse session file {}", path.display()));
    }
    read_session_from_event_tree(&sdk_sessions_dir(), session_id)
}

fn write_session(session_id: &str, session: &Value) -> Result<()> {
    validate_session_id(session_id)?;
    let dir = sdk_sessions_dir();
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = session_file(session_id)?;
    let tmp = dir.join(format!(".{}.{}.tmp", session_id, std::process::id()));
    fs::write(&tmp, serde_json::to_string_pretty(session)?)
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("failed to replace {}", path.display()))?;
    append_runtime_events_for_missing_messages(&dir, session_id, session)?;
    Ok(())
}

fn session_file(session_id: &str) -> Result<PathBuf> {
    validate_session_id(session_id)?;
    Ok(sdk_sessions_dir().join(format!("{session_id}.json")))
}

fn session_events_file(root: &Path, session_id: &str) -> Result<PathBuf> {
    validate_session_id(session_id)?;
    Ok(root.join(session_id).join("events.jsonl"))
}

fn read_session_from_event_tree(root: &Path, session_id: &str) -> Result<Value> {
    validate_session_id(session_id)?;
    let path = session_events_file(root, session_id)?;
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("session '{}' was not found", session_id))?;
    let mut messages = Vec::new();
    let mut created_at = None;
    let mut updated_at = None;

    for (line_index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: kiana_types::RuntimeEvent = serde_json::from_str(line).with_context(|| {
            format!(
                "failed to parse runtime event {} in {}",
                line_index + 1,
                path.display()
            )
        })?;
        if event.session_id != session_id {
            return Err(anyhow!(
                "runtime event {} in {} belongs to session '{}'",
                line_index + 1,
                path.display(),
                event.session_id
            ));
        }
        let event_time = parse_runtime_event_timestamp(&event.timestamp);
        created_at = Some(created_at.unwrap_or(event_time));
        updated_at = Some(event_time);
        match event.payload {
            kiana_types::RuntimeEventPayload::UserMessage(message)
            | kiana_types::RuntimeEventPayload::AssistantMessage(message) => {
                messages.push(message.message);
            }
            _ => {}
        }
    }

    let now = now_unix_seconds();
    let title = infer_session_title_from_messages(&messages).unwrap_or_else(|| session_id.into());
    Ok(json!({
        "session_id": session_id,
        "title": title,
        "tag": null,
        "parent_session_id": null,
        "created_at": created_at.unwrap_or(now),
        "updated_at": updated_at.unwrap_or_else(|| created_at.unwrap_or(now)),
        "messages": messages
    }))
}

fn append_runtime_events_for_missing_messages(
    root: &Path,
    session_id: &str,
    session: &Value,
) -> Result<()> {
    let Some(messages) = session.get("messages").and_then(Value::as_array) else {
        return Ok(());
    };
    if messages.is_empty() {
        return Ok(());
    }

    let path = session_events_file(root, session_id)?;
    let existing_count = read_existing_runtime_event_count(&path)?;
    if existing_count >= messages.len() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    for (index, message) in messages.iter().enumerate().skip(existing_count) {
        let turn_id = format!("turn-{index}");
        let parent_turn_id = index.checked_sub(1).map(|parent| format!("turn-{parent}"));
        let timestamp = runtime_timestamp_from_message(message);
        let event = kiana_types::sdk_message_to_runtime_event(
            session_id,
            &turn_id,
            parent_turn_id,
            index as u64,
            &timestamp,
            message.clone(),
        );
        writeln!(file, "{}", serde_json::to_string(&event)?)
            .with_context(|| format!("failed to append {}", path.display()))?;
    }

    Ok(())
}

fn read_existing_runtime_event_count(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut count = 0;
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        serde_json::from_str::<Value>(line).with_context(|| {
            format!(
                "failed to parse runtime event {} in {}",
                index + 1,
                path.display()
            )
        })?;
        count += 1;
    }
    Ok(count)
}

fn runtime_timestamp_from_message(message: &Value) -> String {
    match message.get("created_at") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| now_unix_seconds().to_string())
        }
        None => now_unix_seconds().to_string(),
    }
}

fn parse_runtime_event_timestamp(timestamp: &str) -> u64 {
    timestamp.trim().parse::<u64>().unwrap_or_default()
}

fn infer_session_title_from_messages(messages: &[Value]) -> Option<String> {
    messages.iter().find_map(|message| {
        (message.get("role").and_then(Value::as_str) == Some("user"))
            .then(|| message_content_as_text(message.get("content")?))
            .flatten()
            .map(|text| infer_title(&text))
    })
}

fn message_content_as_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
    .map(|text| text.trim().to_string())
    .filter(|text| !text.is_empty())
}

fn infer_title(prompt: &str) -> String {
    let title = prompt.lines().next().unwrap_or(prompt).trim();
    let mut chars = title.chars();
    let shortened: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        format!("{}...", shortened)
    } else {
        shortened.to_string()
    }
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.trim().is_empty()
        || session_id.chars().any(char::is_whitespace)
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
    {
        return Err(anyhow!("session id contains invalid path characters"));
    }
    Ok(())
}

fn session_id(value: &Value) -> Option<&str> {
    value.get("session_id").and_then(Value::as_str)
}

fn message_count(value: &Value) -> usize {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0)
}

fn current_session_id(app_state: &std::collections::HashMap<String, Value>) -> Option<String> {
    app_state
        .get("session_id")
        .or_else(|| app_state.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn require_current_session_id(
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<String> {
    current_session_id(app_state)
        .ok_or_else(|| anyhow!("no current session in command context; pass a session id"))
}

fn session_id_arg(
    rest: &str,
    app_state: &std::collections::HashMap<String, Value>,
) -> Result<String> {
    let rest = rest.trim();
    if rest.is_empty() || rest == "current" {
        return require_current_session_id(app_state);
    }
    validate_session_id(rest)?;
    Ok(rest.to_string())
}

fn session_id_and_tail<'a>(
    rest: &'a str,
    app_state: &std::collections::HashMap<String, Value>,
    usage_name: &str,
) -> Result<(String, &'a str)> {
    let (first, tail) = split_word(rest);
    let Some(first) = first else {
        return Err(anyhow!("{usage_name} requires arguments\n\n{}", usage()));
    };
    if first == "current" || session_file(first).is_ok_and(|path| path.is_file()) {
        return Ok((session_id_arg(first, app_state)?, tail));
    }
    Ok((require_current_session_id(app_state)?, rest))
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

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn unique_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("session-{nanos}")
}

fn usage() -> &'static str {
    "Usage:\n  kiana session [list|status]\n  kiana session path\n  kiana session current\n  kiana session show [session_id|current]\n  kiana session reply [session_id|current] --record-only <message>\n  kiana session rename [session_id|current] <title>\n  kiana session tag [session_id|current] [tag]\n  kiana session files [session_id|current] [--editable <path>] [--read-only <path>] [--clear]\n  kiana session fork [session_id|current]\n  kiana session delete <session_id>\n  kiana session import <session.json> [--force]\n  kiana session export [session_id|current] [--format text|json] [output_path]\n  kiana session compact [session_id|current] [--dry-run]"
}

fn session_files_usage() -> &'static str {
    "Usage:\n  kiana session files [session_id|current] [--editable <path>] [--read-only <path>] [--clear]\n  kiana session files [session_id|current] --clear-editable\n  kiana session files [session_id|current] --clear-read-only"
}

fn import_usage() -> &'static str {
    "Usage:\n  kiana session import <session.json> [--force]\n\nImports an exported local SDK session JSON file and rebuilds its runtime event tree."
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_state::env_lock;
    use crate::Command;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{MutexGuard, PoisonError};

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-session-command-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, app_state: HashMap<String, Value>) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state,
        }
    }

    fn current_context(session_id: &str) -> HashMap<String, Value> {
        HashMap::from([("session_id".to_string(), json!(session_id))])
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_test_session(root: &Path, session_id: &str) {
        let dir = root.join("sdk-sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{session_id}.json")),
            serde_json::to_string_pretty(&json!({
                "session_id": session_id,
                "title": "Original",
                "tag": null,
                "parent_session_id": null,
                "created_at": 1,
                "updated_at": 2,
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn session_lists_and_marks_current_session() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-1");

        let result = SessionCommand
            .execute(context("list", current_context("session-1")))
            .await
            .unwrap();

        assert!(result.value.contains("1 SDK session"));
        assert!(result.value.contains("* session-1"));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn session_no_arg_subcommands_reject_extra_args() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-extra");
        let app_state = current_context("session-extra");

        for args in ["list extra", "status extra", "path extra", "current extra"] {
            let error = SessionCommand
                .execute(context(args, app_state.clone()))
                .await
                .unwrap_err()
                .to_string();

            assert!(
                error.contains("usage: kiana session"),
                "{args} returned unexpected error: {error}"
            );
        }

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn session_renames_tags_and_shows_current_session() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-2");
        let app_state = current_context("session-2");

        SessionCommand
            .execute(context("rename New Title", app_state.clone()))
            .await
            .unwrap();
        SessionCommand
            .execute(context("tag current important", app_state.clone()))
            .await
            .unwrap();
        let show = SessionCommand
            .execute(context("show", app_state))
            .await
            .unwrap();

        assert!(show.value.contains("\"title\": \"New Title\""));
        assert!(show.value.contains("\"tag\": \"important\""));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn session_files_updates_and_shows_durable_file_sets() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-files");

        let result = SessionCommand
            .execute(context(
                "files session-files --editable src/lib.rs --editable tests/foo.rs --read-only README.md --readonly docs/plan.md",
                HashMap::new(),
            ))
            .await
            .unwrap();
        let session = read_session("session-files").unwrap();

        assert!(result.value.contains("Session file sets updated"));
        assert_eq!(
            session["editable_files"],
            serde_json::json!(["src/lib.rs", "tests/foo.rs"])
        );
        assert_eq!(
            session["read_only_files"],
            serde_json::json!(["README.md", "docs/plan.md"])
        );

        let shown = SessionCommand
            .execute(context("files session-files", HashMap::new()))
            .await
            .unwrap();
        assert!(shown
            .value
            .contains("editable_files: src/lib.rs, tests/foo.rs"));
        assert!(shown
            .value
            .contains("read_only_files: README.md, docs/plan.md"));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn session_forks_and_refuses_to_delete_current_session() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-3");
        let app_state = current_context("session-3");

        let forked = SessionCommand
            .execute(context("fork", app_state.clone()))
            .await
            .unwrap();
        assert!(forked.value.contains("source: session-3"));
        let sessions = read_all_sessions(&root.join("sdk-sessions")).unwrap();
        assert_eq!(sessions.len(), 2);

        let error = SessionCommand
            .execute(context("delete", app_state))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("refusing to delete the active session"));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn session_reply_record_only_appends_user_message() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-reply");

        let result = SessionCommand
            .execute(context(
                "reply session-reply --record-only follow up",
                HashMap::new(),
            ))
            .await
            .unwrap();
        let session = read_session("session-reply").unwrap();
        let messages = session["messages"].as_array().unwrap();

        assert!(result.value.contains("Session reply recorded"));
        assert!(result.value.contains("id: session-reply"));
        assert!(result.value.contains("messages: 2"));
        let metadata = result.metadata.as_ref().expect("metadata");
        assert_eq!(
            metadata
                .get("session_transcript_mutated")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            metadata.get("session_id").map(String::as_str),
            Some("session-reply")
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "follow up");

        let event_file = root
            .join("sdk-sessions")
            .join("session-reply")
            .join("events.jsonl");
        let events = std::fs::read_to_string(&event_file)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", event_file.display()))
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "user_message");
        assert_eq!(events[0]["sequence"], 0);
        assert_eq!(events[0]["turn_id"], "turn-0");
        assert_eq!(events[0]["message"]["content"], "hello");
        assert_eq!(events[1]["type"], "user_message");
        assert_eq!(events[1]["sequence"], 1);
        assert_eq!(events[1]["turn_id"], "turn-1");
        assert_eq!(events[1]["parent_turn_id"], "turn-0");
        assert_eq!(events[1]["message"]["content"], "follow up");

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn session_import_force_replaces_existing_json_and_event_tree() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);
        write_test_session(&root, "session-import");
        let import_path = root.join("imported-session.json");
        std::fs::write(
            &import_path,
            serde_json::to_string_pretty(&json!({
                "session_id": "session-import",
                "title": "Imported",
                "tag": null,
                "parent_session_id": null,
                "created_at": 10,
                "updated_at": 11,
                "messages": [
                    {"role": "user", "content": "new prompt", "created_at": 10},
                    {"role": "assistant", "content": [{"type": "text", "text": "new reply"}], "created_at": 11}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let stale_event_dir = root.join("sdk-sessions").join("session-import");
        std::fs::create_dir_all(&stale_event_dir).unwrap();
        let stale_event = kiana_types::sdk_message_to_runtime_event(
            "session-import",
            "turn-0",
            None,
            0,
            "1",
            json!({"role": "user", "content": "stale prompt"}),
        );
        std::fs::write(
            stale_event_dir.join("events.jsonl"),
            format!("{}\n", serde_json::to_string(&stale_event).unwrap()),
        )
        .unwrap();

        let result = SessionCommand
            .execute(context(
                &format!("import --force {}", import_path.display()),
                HashMap::new(),
            ))
            .await
            .unwrap();
        let session = read_session("session-import").unwrap();
        let events = std::fs::read_to_string(stale_event_dir.join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert!(result.value.contains("Session imported"));
        assert_eq!(session["title"], "Imported");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["message"]["content"], "new prompt");
        assert_eq!(events[1]["message"]["content"][0]["text"], "new reply");

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }

    #[test]
    fn session_id_validation_rejects_whitespace() {
        let error = validate_session_id("session with spaces")
            .unwrap_err()
            .to_string();

        assert!(error.contains("session id contains invalid path characters"));
    }

    #[tokio::test]
    async fn session_show_rejects_whitespace_id_before_file_lookup() {
        let _guard = lock_env();
        let root = temp_root();
        std::env::set_var("KIANA_HOME", &root);

        let error = SessionCommand
            .execute(context("show missing session", HashMap::new()))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("session id contains invalid path characters"));
        assert!(!error.contains("was not found"));

        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("KIANA_HOME");
    }
}
