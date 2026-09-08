//! 面向文件夹的 Workbench 入口。
//!
//! 该模块负责把命令行参数和终端交互转换为同一个 [`DaemonHost`] 上的 run、continue、
//! cancel 与 receipt 请求。它不在入口层重建模型循环、权限判断或工具执行：无论是一次性
//! 提示还是交互回合，都会复用 `harness_run` 的协议适配路径。`kiana tui` 仍是历史 SDK
//! 流，不应被当作本模块的等价执行脊柱。
//!
//! Workbench 只维护进程内的 session/run 游标以便继续会话；持久事实和是否真正完成仍由
//! EventLog 与响应回执决定。终端显示到的文本、文件列表和“已完成”状态均是回执投影，
//! 不能单独证明外部副作用正确。

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

/// Workbench 子命令的人工可读用法文本。
///
/// 该文本是交互帮助，不参与参数解析和授权决策；实际可接受的档位仍由
/// `normalize_sandbox` 与控制平面共同约束。
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
    /// 用户显式选择的工作目录；缺失时稍后解析为当前目录。
    pub workdir: Option<PathBuf>,
    /// 请求使用的 runner 沙箱名称，默认是项目内可写档位。
    pub sandbox: String,
    /// 可选的角色标识，作为运行选项传给统一 harness 路径。
    pub role: Option<String>,
    /// 非空时以一次性方式提交给 daemon 的原始提示。
    pub prompt: Option<String>,
    /// 是否将响应以 JSON 输出，而不是终端友好文本。
    pub json: bool,
    /// 是否在启动前要求图形或外部目录选择器选择工作目录。
    pub pick_folder: bool,
    /// 是否只打印帮助后返回。
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

/// 判断参数序列是否应由 Workbench 路由处理。
///
/// 这是入口分流的语法判断，不会访问 daemon；路径形态的参数还会确认它当前解析为目录，
/// 以避免把普通提示词误判为工作目录。
pub fn is_workbench_invocation(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("workbench" | "wb" | "gui" | "--pick-folder") => true,
        Some(value) if value.starts_with("--workdir=") || value.starts_with("--pick-folder=") => {
            true
        }
        Some("--workdir") => true,
        Some(value) if looks_like_workdir_path(value) => resolve_existing_dir(value).is_ok(),
        _ => false,
    }
}

/// 判断一个词元是否具有目录路径的明显形态。
///
/// 结果仅是启发式，不能代替 `resolve_existing_dir` 的规范化和目录检查。它特意拒绝
/// 空字符串与选项形式，避免参数解析把未知 flag 误收进工作目录。
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

/// 解析 Workbench 参数并进入相应的一次性或交互运行模式。
pub async fn main_from_args(args: &[String]) -> Result<()> {
    let launch = parse_workbench_args(args)?;
    run_workbench(launch).await
}

/// 以当前目录和默认设置启动交互 Workbench。
///
/// 此便捷入口仍会经过 [`run_workbench`]，因此不会跳过工作目录信任检查或统一 harness。
pub async fn run_cwd_interactive() -> Result<()> {
    run_workbench(WorkbenchLaunch::default()).await
}

/// 从 CLI 词元解析 Workbench 启动配置。
///
/// `--` 之后的所有词元都属于提示词；此前的路径形态词元只在尚未指定工作目录时被视为
/// 目录，其余普通词元会拼接为提示。未知选项直接失败，避免拼写错误静默改变模型输入。
/// 该函数只完成语法解析，目录存在性、信任状态和沙箱执行由后续路径处理。
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
                    args.get(index).ok_or_else(|| anyhow!("workdir_required"))?,
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

/// 运行 Workbench 的完整入口流程。
///
/// 函数先处理帮助和目录选择，再规范化并切换到工作目录；随后构造统一 harness 选项并创建
/// 一个本地 [`DaemonHost`]。交互终端使用 `workbench_chat`，非终端或 JSON 模式则按回合
/// 调用 `run_envelope_on_host` / `continue_envelope_on_host`。工作目录未受信任时，写入型
/// 请求仍由下游 fail-closed 策略拒绝，本函数不会因为 UI 提示而自行授予信任。
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
            // 一次性请求也必须走相同 harness，因此它获得的拒绝码与
            // `kiana run --sandbox workspace-write` 一致，而不会因入口不同
            // 意外放宽为可写执行。
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
        crate::harness_run::continue_envelope_on_host(host, session_id, prompt, run_id, options)
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
            Err(
                rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
            ) => {
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

/// 通过受控顺序调用目录选择器，并返回已经存在的规范化目录。
///
/// 优先使用用户显式配置的 `KIANA_FOLDER_PICKER_CMD`，其次只在图形会话中尝试常见选择器。
/// 选择器输出最终仍交由 `resolve_existing_dir` 检查，因此空输出、失败状态和文件路径
/// 都会被拒绝。该函数只选择本地目录，不会自动写入 trust 配置。
pub fn pick_folder() -> Result<PathBuf> {
    if let Ok(command) = std::env::var("KIANA_FOLDER_PICKER_CMD") {
        let trimmed = command.trim();
        if !trimmed.is_empty() {
            return run_picker_command(trimmed);
        }
    }
    let has_display =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if has_display {
        if which("zenity") {
            let output = Command::new("zenity")
                .args([
                    "--file-selection",
                    "--directory",
                    "--title=Kiana: choose folder",
                ])
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
        assert!(is_workbench_invocation(&[
            "--workdir".to_owned(),
            "/tmp".to_owned()
        ]));
    }
}
