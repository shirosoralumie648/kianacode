//! Folder workbench: Codex / pi / dsh-style "open a directory and work".
//!
//! Product spine is the same `DaemonHost` used by `kiana run`.
//! `kiana tui` stays parked on the legacy SDK stream.

use anyhow::{anyhow, Context, Result};
use kiana_daemon::DaemonHost;
use kiana_protocol::{ExecutionStatus, RunId};
use kiana_types::{write_project_trust, ProjectTrust};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub const WORKBENCH_USAGE: &str = "\
Usage: kiana workbench [--json] [--sandbox read-only|workspace-write] [--role builder|pm|architect|reviewer] [--workdir DIR | --pick-folder] [--] [<prompt>]
       kiana --workdir DIR [--sandbox workspace-write] [--] [<prompt>]
       kiana --pick-folder
       kiana gui

Open a folder (cwd, --workdir, file-manager path, or GUI picker) and work through DaemonHost.
TTY conversation: transcript + input + status. Esc/Ctrl-C cancels a running turn. /sandbox actually switches.
--json and KIANA_WORKBENCH_PLAIN=1 stay one-shot / rustyline. Token streaming is not claimed.
Interactive mode needs a terminal. Scripts pass a prompt after --.
kiana tui stays parked.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchLaunch {
    pub workdir: Option<PathBuf>,
    pub sandbox: String,
    pub role: Option<String>,
    pub prompt: Option<String>,
    pub json: bool,
    pub pick_folder: bool,
    pub help: bool,
}

impl Default for WorkbenchLaunch {
    fn default() -> Self {
        Self {
            workdir: None,
            sandbox: "workspace-write".to_owned(),
            role: None,
            prompt: None,
            json: false,
            pick_folder: false,
            help: false,
        }
    }
}

pub fn is_workbench_invocation(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("workbench" | "wb" | "gui" | "--pick-folder") => true,
        Some(value) if value.starts_with("--workdir=") || value.starts_with("--pick-folder=") => {
            true
        }
        Some("--workdir") => true,
        Some(value) if looks_like_workdir_path(value) => {
            resolve_existing_dir(value).is_ok()
        }
        _ => false,
    }
}

pub fn looks_like_workdir_path(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return false;
    }
    trimmed == "."
        || trimmed == ".."
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('~')
        || trimmed.starts_with('/')
        || trimmed.contains('/')
        || trimmed.contains('\\')
}

pub async fn main_from_args(args: &[String]) -> Result<()> {
    let launch = parse_workbench_args(args)?;
    run_workbench(launch).await
}

pub async fn run_cwd_interactive() -> Result<()> {
    run_workbench(WorkbenchLaunch::default()).await
}

pub fn parse_workbench_args(args: &[String]) -> Result<WorkbenchLaunch> {
    let mut launch = WorkbenchLaunch::default();
    let mut index = 0;
    if matches!(
        args.first().map(String::as_str),
        Some("workbench" | "wb" | "gui")
    ) {
        if args.first().map(String::as_str) == Some("gui") {
            launch.pick_folder = true;
        }
        index = 1;
    }
    let mut prompt_parts = Vec::new();
    while index < args.len() {
        let argument = &args[index];
        match argument.as_str() {
            "help" | "--help" | "-h" => launch.help = true,
            "--json" => launch.json = true,
            "--pick-folder" | "--pick" | "gui" => launch.pick_folder = true,
            "--workdir" => {
                index += 1;
                launch.workdir = Some(PathBuf::from(
                    args.get(index)
                        .ok_or_else(|| anyhow!("workdir_required"))?,
                ));
            }
            value if value.starts_with("--workdir=") => {
                launch.workdir = Some(PathBuf::from(value.trim_start_matches("--workdir=")));
            }
            "--sandbox" => {
                index += 1;
                launch.sandbox = args
                    .get(index)
                    .ok_or_else(|| anyhow!("run_sandbox_required"))?
                    .clone();
            }
            value if value.starts_with("--sandbox=") => {
                launch.sandbox = value.trim_start_matches("--sandbox=").to_owned();
            }
            "--role" => {
                index += 1;
                launch.role = Some(
                    args.get(index)
                        .ok_or_else(|| anyhow!("run_role_required"))?
                        .clone(),
                );
            }
            value if value.starts_with("--role=") => {
                launch.role = Some(value.trim_start_matches("--role=").to_owned());
            }
            "--" => {
                prompt_parts.extend(args[index + 1..].iter().cloned());
                break;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown workbench option: {value}"));
            }
            value => {
                if launch.workdir.is_none() && looks_like_workdir_path(value) {
                    launch.workdir = Some(PathBuf::from(value));
                } else {
                    prompt_parts.push(value.to_owned());
                }
            }
        }
        index += 1;
    }
    let prompt = prompt_parts.join(" ");
    if !prompt.trim().is_empty() {
        launch.prompt = Some(prompt);
    }
    Ok(launch)
}

pub async fn run_workbench(mut launch: WorkbenchLaunch) -> Result<()> {
    if launch.help {
        print!("{WORKBENCH_USAGE}");
        return Ok(());
    }
    if launch.pick_folder {
        launch.workdir = Some(pick_folder()?);
    }
    let workdir = resolve_workdir(launch.workdir.as_deref())?;
    std::env::set_current_dir(&workdir)
        .with_context(|| format!("failed to enter {}", workdir.display()))?;

    let mut options = HashMap::new();
    options.insert(
        "cwd".to_string(),
        Value::String(workdir.to_string_lossy().into_owned()),
    );
    options.insert("sandbox".to_string(), Value::String(launch.sandbox.clone()));
    if let Some(role) = launch.role.clone() {
        options.insert("role".to_string(), Value::String(role));
    }

    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    if !crate::harness_run::project_trusted(&workdir.to_string_lossy())? {
        if interactive {
            if ask_trust(&workdir)? {
                write_project_trust(&workdir, ProjectTrust::Trusted).map_err(anyhow::Error::msg)?;
            } else {
                eprintln!(
                    "folder is untrusted; writes will fail closed. Type /trust or run: kiana trust ."
                );
            }
        } else if launch.prompt.is_some() {
            // One-shot still goes through the harness so the fail-closed code is
            // the same `workspace_write_requires_trusted_non_safe_profile`
            // users already get from `kiana run --sandbox workspace-write`.
        }
    }

    let host = Arc::new(DaemonHost::local().map_err(anyhow::Error::msg)?);
    let session_id = uuid::Uuid::new_v4().to_string();
    let plain = launch.json || std::env::var_os("KIANA_WORKBENCH_PLAIN").is_some();
    if interactive && !plain {
        return crate::workbench_chat::run(
            host,
            session_id,
            workdir,
            launch.sandbox,
            options,
            launch.prompt,
        )
        .await;
    }

    let mut started = false;
    let mut last_run_id: Option<RunId> = None;

    if let Some(prompt) = launch.prompt.clone() {
        let response = dispatch_turn(
            Arc::clone(&host),
            &session_id,
            &prompt,
            started,
            last_run_id.clone(),
            &options,
        )
        .await?;
        started = true;
        last_run_id = run_id_from(&response);
        print_turn(&response, launch.json)?;
        if !interactive {
            return finish_status(&response);
        }
    } else if !interactive {
        return Err(anyhow!(
            "workbench_requires_terminal_or_prompt. {WORKBENCH_USAGE}"
        ));
    }

    if !interactive {
        return Ok(());
    }

    print_banner(&workdir, &session_id, &launch.sandbox)?;
    let mut editor = rustyline::DefaultEditor::new().ok();
    loop {
        let line = match read_line(&mut editor, "You> ") {
            Ok(line) => line,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed {
            "/quit" | "/exit" | "/q" => break,
            "/help" => {
                println!("{WORKBENCH_USAGE}");
                println!("Slash: /trust  /sandbox  /receipt  /quit");
                continue;
            }
            "/trust" => {
                write_project_trust(&workdir, ProjectTrust::Trusted).map_err(anyhow::Error::msg)?;
                println!("trusted {}", workdir.display());
                continue;
            }
            trimmed if trimmed == "/sandbox" || trimmed.starts_with("/sandbox ") => {
                let rest = trimmed.strip_prefix("/sandbox").unwrap_or("").trim();
                if rest.is_empty() {
                    println!("sandbox: {}", launch.sandbox);
                } else {
                    match crate::workbench_chat::normalize_sandbox(rest) {
                        Ok(sandbox) => {
                            launch.sandbox = sandbox.clone();
                            options.insert("sandbox".to_string(), Value::String(sandbox.clone()));
                            println!("sandbox: {sandbox}");
                        }
                        Err(error) => eprintln!("{error}"),
                    }
                }
                continue;
            }
            "/receipt" => {
                let response = crate::harness_run::receipt_envelope_on_host(
                    Arc::clone(&host),
                    session_id.clone(),
                    last_run_id.clone(),
                    &options,
                )
                .await?;
                print_turn(&response, false)?;
                continue;
            }
            _ => {}
        }
        let response = dispatch_turn(
            Arc::clone(&host),
            &session_id,
            trimmed,
            started,
            last_run_id.clone(),
            &options,
        )
        .await?;
        started = true;
        last_run_id = run_id_from(&response).or(last_run_id);
        if let Err(error) = print_turn(&response, false) {
            eprintln!("{error}");
        }
    }
    Ok(())
}

async fn dispatch_turn(
    host: Arc<DaemonHost>,
    session_id: &str,
    prompt: &str,
    started: bool,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<kiana_protocol::ResponseEnvelope> {
    if started {
        crate::harness_run::continue_envelope_on_host(
            host,
            session_id,
            prompt,
            run_id,
            options,
        )
        .await
    } else {
        crate::harness_run::run_envelope_on_host(host, session_id, prompt, options).await
    }
}

fn finish_status(response: &kiana_protocol::ResponseEnvelope) -> Result<()> {
    if response.status != ExecutionStatus::Completed {
        return Err(anyhow!(
            "{}",
            response.error.as_deref().unwrap_or("kiana_harness_failed")
        ));
    }
    Ok(())
}

fn print_turn(response: &kiana_protocol::ResponseEnvelope, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(response)?);
        return finish_status(response);
    }
    if response.status != ExecutionStatus::Completed {
        eprintln!(
            "blocked: {}",
            response.error.as_deref().unwrap_or("unknown")
        );
        return finish_status(response);
    }
    if let Some(text) = response.output["output"]["text"].as_str() {
        if !text.trim().is_empty() {
            println!("{text}");
        }
    }
    if let Some(session_id) = response.output["session_id"].as_str() {
        println!("session_id: {session_id}");
    }
    if let Some(files) = response.output["files_changed"].as_array() {
        for file in files {
            if let Some(path) = file.as_str() {
                println!("changed: {path}");
            }
        }
    }
    Ok(())
}

fn print_banner(workdir: &Path, session_id: &str, sandbox: &str) -> Result<()> {
    let trusted = crate::harness_run::project_trusted(&workdir.to_string_lossy())?;
    println!("Kiana workbench");
    println!("folder: {}", workdir.display());
    println!("trusted: {}", if trusted { "yes" } else { "no" });
    println!("sandbox: {sandbox}");
    println!("session: {session_id}");
    println!("role: builder / executing");
    println!("Type a request. /help  /trust  /quit");
    Ok(())
}

fn ask_trust(workdir: &Path) -> Result<bool> {
    print!(
        "Trust this folder so Kiana can work here?\n  {}\n[y/N] ",
        workdir.display()
    );
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "YES"))
}

fn read_line(editor: &mut Option<rustyline::DefaultEditor>, prompt: &str) -> Result<String> {
    if let Some(editor) = editor.as_mut() {
        match editor.readline(prompt) {
            Ok(line) => {
                let _ = editor.add_history_entry(&line);
                return Ok(line);
            }
            Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => {
                return Err(anyhow!("eof"));
            }
            Err(_) => {}
        }
    }
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line)? == 0 {
        return Err(anyhow!("eof"));
    }
    Ok(line)
}

fn run_id_from(response: &kiana_protocol::ResponseEnvelope) -> Option<RunId> {
    response
        .output
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(RunId::parse_str)
}

fn resolve_workdir(explicit: Option<&Path>) -> Result<PathBuf> {
    match explicit {
        Some(path) => resolve_existing_dir(path),
        None => std::env::current_dir().context("failed to resolve current directory"),
    }
}

fn resolve_existing_dir(path: impl AsRef<Path>) -> Result<PathBuf> {
    let expanded = expand_user(path.as_ref());
    let canonical = if expanded.exists() {
        expanded
            .canonicalize()
            .with_context(|| format!("workdir_not_found: {}", expanded.display()))?
    } else {
        return Err(anyhow!("workdir_not_found: {}", expanded.display()));
    };
    if !canonical.is_dir() {
        return Err(anyhow!("workdir_not_found: {}", canonical.display()));
    }
    Ok(canonical)
}

fn expand_user(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

pub fn pick_folder() -> Result<PathBuf> {
    if let Ok(command) = std::env::var("KIANA_FOLDER_PICKER_CMD") {
        let trimmed = command.trim();
        if !trimmed.is_empty() {
            return run_picker_command(trimmed);
        }
    }
    let has_display = std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if has_display {
        if which("zenity") {
            let output = Command::new("zenity")
                .args(["--file-selection", "--directory", "--title=Kiana: choose folder"])
                .output()
                .context("pick_folder_unavailable")?;
            return dir_from_command_output("zenity", output);
        }
        if which("kdialog") {
            let start = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            let output = Command::new("kdialog")
                .args(["--getexistingdirectory", &start.to_string_lossy()])
                .output()
                .context("pick_folder_unavailable")?;
            return dir_from_command_output("kdialog", output);
        }
        if which("python3") {
            let output = Command::new("python3")
                .args([
                    "-c",
                    "import os,sys\ntry:\n import tkinter as tk\n from tkinter import filedialog\n root=tk.Tk(); root.withdraw()\n path=filedialog.askdirectory(title='Kiana: choose folder')\nexcept Exception:\n sys.exit(2)\n if not path:\n  sys.exit(1)\n print(path)\n",
                ])
                .output()
                .context("pick_folder_unavailable")?;
            if output.status.success() {
                return dir_from_command_output("tkinter", output);
            }
        }
    }
    Err(anyhow!(
        "pick_folder_unavailable: set KIANA_FOLDER_PICKER_CMD, install zenity/kdialog, or pass --workdir"
    ))
}

fn run_picker_command(command: &str) -> Result<PathBuf> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .context("pick_folder_unavailable")?;
    dir_from_command_output("KIANA_FOLDER_PICKER_CMD", output)
}

fn dir_from_command_output(name: &str, output: std::process::Output) -> Result<PathBuf> {
    if !output.status.success() {
        return Err(anyhow!("pick_folder_unavailable:{name}"));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() {
        return Err(anyhow!("pick_folder_unavailable:{name}"));
    }
    resolve_existing_dir(path)
}

fn which(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_like_args_are_workdirs() {
        assert!(looks_like_workdir_path("."));
        assert!(looks_like_workdir_path("./src"));
        assert!(looks_like_workdir_path("/tmp/project"));
        assert!(!looks_like_workdir_path("trust"));
        assert!(!looks_like_workdir_path("run"));
        assert!(!looks_like_workdir_path("--help"));
    }

    #[test]
    fn parse_workdir_and_prompt() {
        let launch = parse_workbench_args(&[
            "workbench".to_owned(),
            "--workdir".to_owned(),
            "/tmp/demo".to_owned(),
            "--".to_owned(),
            "create GOLDEN_PATH.txt".to_owned(),
        ])
        .unwrap();
        assert_eq!(launch.workdir.as_deref(), Some(Path::new("/tmp/demo")));
        assert_eq!(launch.prompt.as_deref(), Some("create GOLDEN_PATH.txt"));
        assert_eq!(launch.sandbox, "workspace-write");
        assert!(!launch.pick_folder);
    }

    #[test]
    fn gui_selects_picker() {
        let launch = parse_workbench_args(&["gui".to_owned()]).unwrap();
        assert!(launch.pick_folder);
        assert!(is_workbench_invocation(&["--pick-folder".to_owned()]));
        assert!(is_workbench_invocation(&["--workdir".to_owned(), "/tmp".to_owned()]));
    }
}
