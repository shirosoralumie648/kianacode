//! 文件夹 Workbench 的终端会话视图与输入编排。
//!
//! 此模块管理 ratatui 的短生命周期界面状态、键盘输入和 slash 命令，并把实际运行请求
//! 交给同一个 [`DaemonHost`]。它不会直接调用模型或工具；提交、继续、取消和查看回执均
//! 通过 `harness_run` 回到 daemon，因此 UI 不能绕过授权、信任和沙箱边界。
//!
//! “running/idle” 是当前进程观察到的视图状态而非持久执行事实。界面订阅
//! [`DaemonHost::subscribe_run`] 的 additive run-stream 只用于增量展示；完成、取消和
//! 结果未知仍以终态响应/回执为准。要审计执行结果仍应读取正式 receipt。

use anyhow::{anyhow, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures_util::StreamExt;
use kiana_daemon::DaemonHost;
use kiana_protocol::{ExecutionStatus, ResponseEnvelope, RunId, RunStreamEvent};
use kiana_types::{write_project_trust, ProjectTrust};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Terminal;
use serde_json::Value;
use std::collections::HashMap;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::harness_run;
use crate::workbench::WORKBENCH_USAGE;

const SLASH_HELP: &str =
    "Slash: /trust  /sandbox read-only|workspace-write  /receipt  /cancel  /quit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatRole {
    /// 用户输入的提示词。
    User,
    /// 回执中可展示的助手文本。
    Assistant,
    /// 本地 UI 产生的帮助、错误或状态提示。
    System,
    /// 回执声明的文件变更摘要。
    Changed,
}

/// 终端时间线的一条不可持久化展示消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// 决定消息在界面中的标签和颜色。
    pub role: ChatRole,
    /// 显示给用户的文本；它不是 EventLog 的替代品。
    pub text: String,
}

/// 当前 turn 的增量展示状态。
///
/// 该状态只服务于 ratatui 投影，不是 EventLog 或 Receipt 的替代品。`text` 是已经收到
/// 的 delta 拼接结果，`message_index` 指向时间线中唯一一条仍在增长的助手消息。
#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamTurn {
    run_id: RunId,
    text: String,
    message_index: Option<usize>,
}

/// 对一行用户输入解析得到的本地 UI 动作。
///
/// 某些动作会进一步发起 daemon 请求，另一些只改变显示状态。枚举本身不代表动作已经
/// 执行、获批或成功。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatAction {
    /// 空白输入，不产生任何变化。
    None,
    /// 将给定提示词提交给统一 harness。
    Submit(String),
    /// 请求取消当前运行。
    Cancel,
    /// 退出终端循环。
    Quit,
    /// 将当前目录标记为受信任，实际写入由调用路径执行。
    Trust,
    /// 切换为已规范化的沙箱名称。
    SetSandbox(String),
    /// 在时间线中显示当前沙箱设置。
    ShowSandbox,
    /// 请求读取当前 run 的 receipt。
    Receipt,
    /// 显示帮助内容。
    Help,
    /// 显示解析或校验错误，而不提交模型请求。
    Error(String),
}

/// Workbench 当前可渲染的进程内状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchView {
    /// 已规范化的工作目录。
    pub folder: PathBuf,
    /// 当前读取到的项目信任状态；可能因外部修改而过时。
    pub trusted: bool,
    /// 当前回合将作为选项传给 daemon 的沙箱档位。
    pub sandbox: String,
    /// 本次界面会话使用的 session 标识。
    pub session_id: String,
    /// 是否已有一个请求在本 UI 中等待响应。
    pub running: bool,
    /// 尚未提交的编辑缓冲区。
    pub input: String,
    /// 用于绘制对话尾部的展示消息集合。
    pub messages: Vec<ChatMessage>,
    /// 当前正在增量展示的 turn；终态到达后清空。
    stream_turn: Option<StreamTurn>,
    /// 最近一次已应用的终态，用于忽略同一响应的重复投递。
    last_terminal: Option<(RunId, ExecutionStatus)>,
}

impl WorkbenchView {
    /// 创建空对话视图，并加入不会影响授权的本地帮助提示。
    pub fn new(folder: PathBuf, trusted: bool, sandbox: String, session_id: String) -> Self {
        let mut view = Self {
            folder,
            trusted,
            sandbox,
            session_id,
            running: false,
            input: String::new(),
            messages: Vec::new(),
            stream_turn: None,
            last_terminal: None,
        };
        view.push_system(format!(
            "Conversation surface on DaemonHost. {SLASH_HELP}. Esc/Ctrl-C cancels a running turn."
        ));
        view
    }

    /// 生成可扫描的状态栏文本。
    ///
    /// 为避免终端宽度被 UUID 撑开，session ID 只显示前八个字节；完整 ID 始终保留在
    /// [`session_id`](Self::session_id) 中供 daemon 调用。
    pub fn status_line(&self) -> String {
        format!(
            "kiana  {}  trusted:{}  sandbox:{}  {}  session:{}",
            self.folder.display(),
            if self.trusted { "yes" } else { "no" },
            self.sandbox,
            if self.running { "running" } else { "idle" },
            short_id(&self.session_id)
        )
    }

    /// 将输入行解释为 slash 命令或普通提示词。
    ///
    /// 解析不会执行动作，且未知 slash 命令回退到帮助，而不是误发送给模型。普通提示词
    /// 会去除两端空白后再提交，以保证空白行不会创建无意义运行。
    pub fn interpret_line(&self, raw: &str) -> ChatAction {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return ChatAction::None;
        }
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, args) = split_slash(rest);
            return match name {
                "quit" | "exit" | "q" => ChatAction::Quit,
                "help" | "h" => ChatAction::Help,
                "trust" => ChatAction::Trust,
                "sandbox" if args.is_empty() => ChatAction::ShowSandbox,
                "sandbox" => interpret_sandbox(args),
                "receipt" => ChatAction::Receipt,
                "cancel" => ChatAction::Cancel,
                _ => ChatAction::Help,
            };
        }
        ChatAction::Submit(trimmed.to_owned())
    }

    /// 追加一条本地系统提示，不与 daemon 通信。
    pub fn push_system(&mut self, text: impl Into<String>) {
        self.messages.push(ChatMessage {
            role: ChatRole::System,
            text: text.into(),
        });
    }

    /// 追加一条已经准备提交或刚提交的用户消息。
    pub fn push_user(&mut self, text: impl Into<String>) {
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            text: text.into(),
        });
    }

    /// 为即将启动的 turn 建立增量展示状态。
    ///
    /// 调用方必须已经完成 [`DaemonHost::subscribe_run`] 订阅；该方法本身不发起 daemon
    /// 请求，也不会把“开始展示”当成运行已经开始。
    fn begin_stream(&mut self, run_id: RunId) {
        self.stream_turn = Some(StreamTurn {
            run_id,
            text: String::new(),
            message_index: None,
        });
        self.last_terminal = None;
    }

    /// 应用一条 additive run-stream 事件。
    ///
    /// `Delta` 只更新展示消息；`Terminal` 只把最终响应交还给调用方，必须由调用方继续走
    /// [`apply_response`](Self::apply_response) 才可能结束本地 running 状态。没有活跃
    /// turn 时忽略迟到事件，避免旧订阅把新 turn 的展示状态污染。
    fn apply_stream_event(&mut self, event: RunStreamEvent) -> Result<Option<ResponseEnvelope>> {
        let Some(active_run_id) = self.stream_turn.as_ref().map(|stream| stream.run_id) else {
            return Ok(None);
        };
        match event {
            RunStreamEvent::Delta { run_id, text } => {
                if run_id != active_run_id {
                    return Err(anyhow!("stream_run_id_mismatch"));
                }
                self.push_stream_delta(&text);
                Ok(None)
            }
            RunStreamEvent::Terminal { run_id, response } => {
                if run_id != active_run_id {
                    return Err(anyhow!("stream_run_id_mismatch"));
                }
                Ok(Some(response))
            }
            RunStreamEvent::Unknown => Ok(None),
        }
    }

    fn push_stream_delta(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(stream) = self.stream_turn.as_mut() else {
            return;
        };
        stream.text.push_str(text);
        let streamed_text = stream.text.clone();
        if let Some(index) = stream.message_index {
            self.messages[index].text = streamed_text;
        } else {
            let index = self.messages.len();
            self.messages.push(ChatMessage {
                role: ChatRole::Assistant,
                text: streamed_text,
            });
            if let Some(stream) = self.stream_turn.as_mut() {
                stream.message_index = Some(index);
            }
        }
    }

    /// 将 daemon 响应投影到时间线，并结束本地等待状态。
    ///
    /// 非 `Completed` 响应只显示结构化错误文本，不把其输出伪装为助手成功回复；成功响应
    /// 仅提取约定 JSON 字段。已有增量文本时，`Completed` 用最终回执文本替换增量投影；
    /// `Cancelled` / `ResultUnknown` 保留已显示文本并明确标注中断，不伪装成完成。
    pub fn apply_response(&mut self, response: &ResponseEnvelope) {
        let response_run_id = run_id_from(response);
        if response.status.is_terminal() {
            if let Some(run_id) = response_run_id {
                if self.last_terminal == Some((run_id, response.status)) {
                    return;
                }
                self.last_terminal = Some((run_id, response.status));
            }
        }
        if response.status != ExecutionStatus::Completed {
            self.finish_non_completed_response(response);
            return;
        }
        self.finish_completed_response(response);
    }

    fn finish_non_completed_response(&mut self, response: &ResponseEnvelope) {
        self.stream_turn = None;
        match response.status {
            ExecutionStatus::Cancelled => self.push_system("已中断"),
            ExecutionStatus::ResultUnknown => self.push_system(format!(
                "已中断（result_unknown）：{}",
                response.error.as_deref().unwrap_or("result_unknown")
            )),
            _ => self.push_system(format!(
                "blocked: {}",
                response.error.as_deref().unwrap_or("kiana_harness_failed")
            )),
        }
        self.running = false;
    }

    fn finish_completed_response(&mut self, response: &ResponseEnvelope) {
        let final_text = response.output["output"]["text"]
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned);
        let streamed_index = self
            .stream_turn
            .take()
            .and_then(|stream| stream.message_index);
        if let Some(text) = final_text {
            if let Some(index) = streamed_index {
                self.messages[index].text = text;
            } else {
                self.messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    text,
                });
            }
        }
        if let Some(files) = response.output["files_changed"].as_array() {
            let changed: Vec<String> = files
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if !changed.is_empty() {
                self.messages.push(ChatMessage {
                    role: ChatRole::Changed,
                    text: changed.join(", "),
                });
            }
        }
        self.running = false;
    }
}

fn interpret_sandbox(args: &str) -> ChatAction {
    match normalize_sandbox(args) {
        Ok(sandbox) => ChatAction::SetSandbox(sandbox),
        Err(error) => ChatAction::Error(error.to_string()),
    }
}

/// 将用户友好的沙箱别名规范化为 runner 协议使用的名称。
///
/// 只接受 `read-only` 和 `workspace-write` 两个档位；明确拒绝
/// `danger-full-access`，未知值同样失败。该函数的成功结果仍只是请求参数，后续策略、
/// 项目 trust 和实际 sandbox backend 仍可能拒绝执行。
pub fn normalize_sandbox(value: &str) -> Result<String> {
    match value.trim().replace('_', "-").to_ascii_lowercase().as_str() {
        "read-only" | "readonly" => Ok("read-only".to_owned()),
        "workspace-write" | "workspacewrite" | "workspace" => Ok("workspace-write".to_owned()),
        "danger-full-access" | "dangerfullaccess" | "full" => {
            Err(anyhow!("danger_full_access_rejected"))
        }
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

fn split_slash(rest: &str) -> (&str, &str) {
    match rest.split_once(char::is_whitespace) {
        Some((name, args)) => (name, args.trim()),
        None => (rest, ""),
    }
}

fn short_id(value: &str) -> &str {
    if value.len() > 8 {
        &value[..8]
    } else {
        value
    }
}

/// 将一个键盘事件转换为终端编辑命令。
///
/// 释放事件被忽略以避免重复输入；Esc/Ctrl-C 仅在本地确认为运行中时请求取消，空闲时的
/// Ctrl-C/Ctrl-D 则退出。返回命令仍须由事件循环处理，不能等同于远端操作已发生。
pub fn action_from_key(key: KeyEvent, running: bool) -> Option<KeyCommand> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') if running => Some(KeyCommand::Cancel),
            KeyCode::Char('c') | KeyCode::Char('d') => Some(KeyCommand::Quit),
            KeyCode::Char('j') => Some(KeyCommand::Newline),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc if running => Some(KeyCommand::Cancel),
        KeyCode::Esc => None,
        KeyCode::Enter => Some(KeyCommand::Submit),
        KeyCode::Backspace => Some(KeyCommand::Backspace),
        KeyCode::Char(ch) => Some(KeyCommand::Insert(ch)),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCommand {
    /// 向输入缓冲区插入一个字符。
    Insert(char),
    /// 删除输入缓冲区最后一个 Unicode 标量值。
    Backspace,
    /// 插入多行输入所需的换行符。
    Newline,
    /// 解释并处理当前输入缓冲区。
    Submit,
    /// 请求取消当前 daemon run。
    Cancel,
    /// 退出终端循环。
    Quit,
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}

/// 运行 ratatui Workbench 事件循环。
///
/// 函数在进入备用屏幕和 raw mode 后创建 UI 状态，并将耗时的 daemon 调用放入 Tokio
/// 任务，经 channel 回到主循环，避免阻塞键盘与绘制。`TerminalGuard` 负责异常退出时
/// 尽力恢复终端；取消只是发送统一 cancel 请求，实际结果仍以随后 receipt/响应为准。
pub async fn run(
    host: Arc<DaemonHost>,
    session_id: String,
    workdir: PathBuf,
    sandbox: String,
    mut options: HashMap<String, Value>,
    initial_prompt: Option<String>,
) -> Result<()> {
    let trusted = harness_run::project_trusted(&workdir.to_string_lossy())?;
    let mut view = WorkbenchView::new(workdir.clone(), trusted, sandbox, session_id.clone());
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut events = EventStream::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<Result<kiana_protocol::ResponseEnvelope>>();
    let (stream_tx, mut stream_rx) =
        mpsc::unbounded_channel::<std::result::Result<RunStreamEvent, String>>();
    let mut started = false;
    let mut last_run_id: Option<RunId> = None;

    if let Some(prompt) = initial_prompt {
        submit_turn(
            &mut view,
            &host,
            &session_id,
            &options,
            started,
            last_run_id.clone(),
            prompt,
            &tx,
            &stream_tx,
        )?;
        started = true;
    }

    loop {
        terminal.draw(|frame| draw(frame, &view))?;
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Some(Ok(response)) => {
                        last_run_id = run_id_from(&response).or(last_run_id);
                        started = true;
                        view.apply_response(&response);
                    }
                    Some(Err(error)) => {
                        view.running = false;
                        view.push_system(error.to_string());
                    }
                    None => view.running = false,
                }
            }
            stream_event = stream_rx.recv() => {
                match stream_event {
                    Some(Ok(event)) => {
                        match view.apply_stream_event(event) {
                            Ok(Some(response)) => {
                                last_run_id = run_id_from(&response).or(last_run_id);
                                started = true;
                                view.apply_response(&response);
                            }
                            Ok(None) => {}
                            Err(error) => view.push_system(error.to_string()),
                        }
                    }
                    Some(Err(error)) => {
                        // 流通道失败不能结束 running；必须继续等终态响应/回执对账。
                        view.push_system(format!("stream interrupted; awaiting receipt: {error}"));
                    }
                    None => {}
                }
            }
            event = events.next() => {
                let Some(Ok(Event::Key(key))) = event else { continue };
                match action_from_key(key, view.running) {
                    Some(KeyCommand::Quit) => break,
                    Some(KeyCommand::Cancel) => {
                        spawn_cancel(&host, &session_id, last_run_id.clone(), &options, &tx);
                    }
                    Some(KeyCommand::Submit) => {
                        let action = view.interpret_line(&view.input.clone());
                        view.input.clear();
                        match handle_action(
                            action,
                            &mut view,
                            &host,
                            &session_id,
                            &mut options,
                            started,
                            last_run_id.clone(),
                            &tx,
                            &stream_tx,
                        )
                        .await?
                        {
                            LoopControl::Quit => break,
                            LoopControl::Submitted => started = true,
                            LoopControl::Continue => {}
                        }
                    }
                    Some(KeyCommand::Backspace) => {
                        view.input.pop();
                    }
                    Some(KeyCommand::Newline) => view.input.push('\n'),
                    Some(KeyCommand::Insert(ch)) => view.input.push(ch),
                    None => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(80)) => {}
        }
    }
    Ok(())
}

enum LoopControl {
    Continue,
    Submitted,
    Quit,
}

async fn handle_action(
    action: ChatAction,
    view: &mut WorkbenchView,
    host: &Arc<DaemonHost>,
    session_id: &str,
    options: &mut HashMap<String, Value>,
    started: bool,
    last_run_id: Option<RunId>,
    tx: &mpsc::UnboundedSender<Result<kiana_protocol::ResponseEnvelope>>,
    stream_tx: &mpsc::UnboundedSender<std::result::Result<RunStreamEvent, String>>,
) -> Result<LoopControl> {
    match action {
        ChatAction::None => Ok(LoopControl::Continue),
        ChatAction::Quit => Ok(LoopControl::Quit),
        ChatAction::Help => {
            view.push_system(format!("{WORKBENCH_USAGE}\n{SLASH_HELP}"));
            Ok(LoopControl::Continue)
        }
        ChatAction::Error(error) => {
            view.push_system(error);
            Ok(LoopControl::Continue)
        }
        ChatAction::ShowSandbox => {
            view.push_system(format!("sandbox: {}", view.sandbox));
            Ok(LoopControl::Continue)
        }
        ChatAction::Trust => {
            write_project_trust(&view.folder, ProjectTrust::Trusted).map_err(anyhow::Error::msg)?;
            view.trusted = true;
            view.push_system(format!("trusted {}", view.folder.display()));
            Ok(LoopControl::Continue)
        }
        ChatAction::SetSandbox(sandbox) => {
            view.sandbox = sandbox.clone();
            options.insert("sandbox".to_string(), Value::String(sandbox.clone()));
            view.push_system(format!("sandbox: {sandbox}"));
            Ok(LoopControl::Continue)
        }
        ChatAction::Receipt => {
            let response = harness_run::receipt_envelope_on_host(
                Arc::clone(host),
                session_id.to_owned(),
                last_run_id,
                options,
            )
            .await?;
            view.apply_response(&response);
            Ok(LoopControl::Continue)
        }
        ChatAction::Cancel => {
            if !view.running {
                view.push_system("nothing to cancel");
                return Ok(LoopControl::Continue);
            }
            spawn_cancel(host, session_id, last_run_id, options, tx);
            Ok(LoopControl::Continue)
        }
        ChatAction::Submit(prompt) => {
            if view.running {
                view.push_system("turn is running; Esc or /cancel first");
                return Ok(LoopControl::Continue);
            }
            submit_turn(
                view,
                host,
                session_id,
                options,
                started,
                last_run_id,
                prompt,
                tx,
                stream_tx,
            )?;
            Ok(LoopControl::Submitted)
        }
    }
}

fn spawn_cancel(
    host: &Arc<DaemonHost>,
    session_id: &str,
    last_run_id: Option<RunId>,
    options: &HashMap<String, Value>,
    tx: &mpsc::UnboundedSender<Result<kiana_protocol::ResponseEnvelope>>,
) {
    // 将取消也交给与 run 相同的 daemon host，确保请求附带当前 session、run 和选项，
    // 而不是让 UI 直接终止底层进程。
    let host = Arc::clone(host);
    let session_id = session_id.to_owned();
    let options = options.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let result =
            harness_run::cancel_envelope_on_host(host, session_id, last_run_id, "user", &options)
                .await;
        let _ = tx.send(result);
    });
}

fn submit_turn(
    view: &mut WorkbenchView,
    host: &Arc<DaemonHost>,
    session_id: &str,
    options: &HashMap<String, Value>,
    started: bool,
    last_run_id: Option<RunId>,
    prompt: String,
    tx: &mpsc::UnboundedSender<Result<kiana_protocol::ResponseEnvelope>>,
    stream_tx: &mpsc::UnboundedSender<std::result::Result<RunStreamEvent, String>>,
) -> Result<()> {
    // 启动/继续的选择只取决于本会话是否已拥有 run；真正的生命周期合法性仍由 daemon
    // 验证。结果通过 channel 串回 UI，避免并发任务直接改动视图状态。
    let stream_run_id = if started {
        last_run_id.or_else(|| RunId::parse_str(session_id))
    } else {
        RunId::parse_str(session_id)
    }
    .ok_or_else(|| anyhow!("workbench_stream_run_id_required"))?;
    subscribe_run_stream(host, stream_run_id, stream_tx);
    view.begin_stream(stream_run_id);
    view.push_user(prompt.clone());
    view.running = true;
    let host = Arc::clone(host);
    let session_id = session_id.to_owned();
    let options = options.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = if started {
            harness_run::continue_envelope_on_host(host, session_id, prompt, last_run_id, &options)
                .await
        } else {
            harness_run::run_envelope_on_host(host, session_id, prompt, &options).await
        };
        let _ = tx.send(result);
    });
    Ok(())
}

fn subscribe_run_stream(
    host: &Arc<DaemonHost>,
    run_id: RunId,
    stream_tx: &mpsc::UnboundedSender<std::result::Result<RunStreamEvent, String>>,
) {
    // 必须在 run 任务启动前创建订阅；否则会错过已经发出的早期 delta。订阅只转发展示
    // 事件，终态仍由 run/cancel 响应与 receipt 对账。
    let mut subscription = host.subscribe_run(run_id);
    let stream_tx = stream_tx.clone();
    tokio::spawn(async move {
        loop {
            match subscription.recv().await {
                Ok(envelope) => {
                    let terminal = matches!(envelope.event, RunStreamEvent::Terminal { .. });
                    if stream_tx.send(Ok(envelope.event)).is_err() {
                        break;
                    }
                    if terminal {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let _ = stream_tx.send(Err(format!("stream_subscription_lagged:{skipped}")));
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    let _ = stream_tx.send(Err("stream_closed_before_terminal".to_owned()));
                    break;
                }
            }
        }
    });
}

fn run_id_from(response: &kiana_protocol::ResponseEnvelope) -> Option<RunId> {
    response
        .output
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(RunId::parse_str)
}

fn visible_messages(messages: &[ChatMessage], height: usize) -> &[ChatMessage] {
    let height = height.max(1);
    if messages.len() <= height {
        messages
    } else {
        &messages[messages.len() - height..]
    }
}

fn draw(frame: &mut ratatui::Frame, view: &WorkbenchView) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let list_height = chunks[0].height.saturating_sub(2) as usize;
    let items: Vec<ListItem> = visible_messages(&view.messages, list_height)
        .iter()
        .map(|message| {
            let (label, color) = match message.role {
                ChatRole::User => ("You", Color::Cyan),
                ChatRole::Assistant => ("Kiana", Color::Green),
                ChatRole::System => ("system", Color::DarkGray),
                ChatRole::Changed => ("changed", Color::Yellow),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{label}: "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(message.text.clone()),
            ]))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("conversation")),
        chunks[0],
    );

    let input = Paragraph::new(view.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("input"));
    frame.render_widget(input, chunks[1]);

    let status = Paragraph::new(view.status_line()).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_protocol::{ExecutionStatus, RequestId, ResponseEnvelope, PROTOCOL_SCHEMA};

    fn completed_response(output: Value) -> ResponseEnvelope {
        ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: RequestId::new(),
            status: ExecutionStatus::Completed,
            output,
            error: None,
        }
    }

    fn running_view() -> WorkbenchView {
        let mut view = WorkbenchView::new(
            PathBuf::from("/tmp/demo"),
            true,
            "workspace-write".to_owned(),
            RunId::new().to_string(),
        );
        view.running = true;
        view
    }

    fn delta_event(run_id: RunId, text: &str) -> RunStreamEvent {
        RunStreamEvent::Delta {
            run_id,
            text: text.to_owned(),
        }
    }

    fn response_with_run_id(
        status: ExecutionStatus,
        run_id: RunId,
        output: Value,
    ) -> ResponseEnvelope {
        let mut output = output;
        output["run_id"] = Value::String(run_id.to_string());
        ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: RequestId::new(),
            status,
            output,
            error: None,
        }
    }

    #[test]
    fn status_line_names_folder_trust_and_sandbox() {
        let mut view = WorkbenchView::new(
            PathBuf::from("/tmp/demo"),
            true,
            "workspace-write".to_owned(),
            "abcdef12-session".to_owned(),
        );
        let status = view.status_line();
        assert!(status.contains("/tmp/demo"), "{status}");
        assert!(status.contains("trusted:yes"), "{status}");
        assert!(status.contains("sandbox:workspace-write"), "{status}");
        assert!(status.contains("idle"), "{status}");
        view.running = true;
        let running = view.status_line();
        assert!(running.contains("running"), "{running}");
    }

    #[test]
    fn sandbox_slash_actually_switches() {
        let view = WorkbenchView::new(
            PathBuf::from("/tmp/demo"),
            true,
            "workspace-write".to_owned(),
            "s".to_owned(),
        );
        assert_eq!(
            view.interpret_line("/sandbox read-only"),
            ChatAction::SetSandbox("read-only".to_owned())
        );
        assert_eq!(
            view.interpret_line("/sandbox workspace-write"),
            ChatAction::SetSandbox("workspace-write".to_owned())
        );
        assert_eq!(view.interpret_line("/sandbox"), ChatAction::ShowSandbox);
        assert_eq!(
            view.interpret_line("/sandbox danger-full-access"),
            ChatAction::Error("danger_full_access_rejected".to_owned())
        );
        assert_eq!(view.interpret_line("/quit"), ChatAction::Quit);
        assert_eq!(
            view.interpret_line("write GOLDEN_PATH.txt"),
            ChatAction::Submit("write GOLDEN_PATH.txt".to_owned())
        );
    }

    #[test]
    fn esc_cancels_only_while_running() {
        let running = KeyEvent::from(KeyCode::Esc);
        assert_eq!(action_from_key(running, true), Some(KeyCommand::Cancel));
        assert_eq!(action_from_key(running, false), None);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(action_from_key(ctrl_c, true), Some(KeyCommand::Cancel));
        assert_eq!(action_from_key(ctrl_c, false), Some(KeyCommand::Quit));
    }

    #[test]
    fn apply_response_appends_assistant_and_files() {
        let mut view = WorkbenchView::new(
            PathBuf::from("/tmp/demo"),
            true,
            "workspace-write".to_owned(),
            "s".to_owned(),
        );
        view.running = true;
        let response = completed_response(serde_json::json!({
            "output": {"text": "created GOLDEN_PATH.txt"},
            "files_changed": ["GOLDEN_PATH.txt"]
        }));
        view.apply_response(&response);
        assert!(!view.running);
        assert!(view
            .messages
            .iter()
            .any(|message| message.role == ChatRole::Assistant
                && message.text.contains("created GOLDEN_PATH.txt")));
        assert!(view
            .messages
            .iter()
            .any(|message| message.role == ChatRole::Changed
                && message.text.contains("GOLDEN_PATH.txt")));
    }

    #[test]
    fn stream_deltas_update_one_assistant_message_without_completing_the_turn() {
        let mut view = running_view();
        let run_id = RunId::new();
        view.begin_stream(run_id);

        assert!(view
            .apply_stream_event(delta_event(run_id, "alpha"))
            .unwrap()
            .is_none());
        assert!(view
            .apply_stream_event(delta_event(run_id, " beta"))
            .unwrap()
            .is_none());

        let assistant: Vec<_> = view
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::Assistant)
            .collect();
        assert_eq!(assistant.len(), 1);
        assert_eq!(assistant[0].text, "alpha beta");
        assert!(view.running, "delta alone must not end the local turn");
        assert!(!view
            .messages
            .iter()
            .any(|message| message.text.contains("completed")));
    }

    #[test]
    fn terminal_event_waits_for_the_terminal_response_to_finish_the_turn() {
        let mut view = running_view();
        let run_id = RunId::new();
        view.begin_stream(run_id);
        view.apply_stream_event(delta_event(run_id, "partial"))
            .unwrap();
        let response = response_with_run_id(
            ExecutionStatus::Completed,
            run_id,
            serde_json::json!({"output": {"text": "partial final"}}),
        );

        let returned = view
            .apply_stream_event(RunStreamEvent::Terminal {
                run_id,
                response: response.clone(),
            })
            .unwrap();

        assert_eq!(returned, Some(response));
        assert!(view.running, "terminal event alone must not end the turn");
        assert_eq!(
            view.messages
                .iter()
                .find(|message| message.role == ChatRole::Assistant)
                .unwrap()
                .text,
            "partial"
        );
    }

    #[test]
    fn completed_terminal_replaces_streamed_text_without_duplicating_messages() {
        let mut view = running_view();
        let run_id = RunId::new();
        view.begin_stream(run_id);
        view.apply_stream_event(delta_event(run_id, "alpha"))
            .unwrap();
        view.apply_stream_event(delta_event(run_id, " beta"))
            .unwrap();
        let response = response_with_run_id(
            ExecutionStatus::Completed,
            run_id,
            serde_json::json!({
                "output": {"text": "alpha beta gamma"},
                "files_changed": ["GOLDEN_PATH.txt"]
            }),
        );

        view.apply_response(&response);
        view.apply_response(&response);

        let assistant: Vec<_> = view
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::Assistant)
            .collect();
        assert_eq!(assistant.len(), 1);
        assert_eq!(assistant[0].text, "alpha beta gamma");
        assert_eq!(
            view.messages
                .iter()
                .filter(|message| message.role == ChatRole::Changed)
                .count(),
            1
        );
        assert!(!view.running);
    }

    #[test]
    fn cancelled_terminal_preserves_streamed_text_and_marks_interrupted() {
        let mut view = running_view();
        let run_id = RunId::new();
        view.begin_stream(run_id);
        view.apply_stream_event(delta_event(run_id, "half"))
            .unwrap();
        let mut response = response_with_run_id(
            ExecutionStatus::Cancelled,
            run_id,
            serde_json::json!({"output": {}}),
        );
        response.error = Some("cancelled:user".to_owned());

        view.apply_response(&response);

        assert_eq!(
            view.messages
                .iter()
                .find(|message| message.role == ChatRole::Assistant)
                .unwrap()
                .text,
            "half"
        );
        assert!(view
            .messages
            .iter()
            .any(|message| message.role == ChatRole::System && message.text == "已中断"));
        assert!(!view.running);
    }

    #[test]
    fn result_unknown_terminal_preserves_text_and_marks_interrupted() {
        let mut view = running_view();
        let run_id = RunId::new();
        view.begin_stream(run_id);
        view.apply_stream_event(delta_event(run_id, "half"))
            .unwrap();
        let mut response = response_with_run_id(
            ExecutionStatus::ResultUnknown,
            run_id,
            serde_json::json!({"output": {}}),
        );
        response.error = Some("result_unknown:provider_timeout".to_owned());

        view.apply_response(&response);

        assert_eq!(
            view.messages
                .iter()
                .find(|message| message.role == ChatRole::Assistant)
                .unwrap()
                .text,
            "half"
        );
        assert!(view.messages.iter().any(|message| {
            message.role == ChatRole::System
                && message.text.contains("已中断")
                && message.text.contains("result_unknown")
        }));
        assert!(!view.running);
    }

    #[test]
    fn stream_delta_for_another_run_is_rejected_without_mutating_text() {
        let mut view = running_view();
        let run_id = RunId::new();
        view.begin_stream(run_id);

        let error = view
            .apply_stream_event(delta_event(RunId::new(), "wrong"))
            .unwrap_err();

        assert_eq!(error.to_string(), "stream_run_id_mismatch");
        assert!(!view
            .messages
            .iter()
            .any(|message| message.role == ChatRole::Assistant));
    }

    #[test]
    fn danger_sandbox_is_rejected() {
        let error = normalize_sandbox("danger-full-access")
            .unwrap_err()
            .to_string();
        assert_eq!(error, "danger_full_access_rejected");
        assert_eq!(
            normalize_sandbox("read_only").unwrap(),
            "read-only".to_owned()
        );
    }

    #[test]
    fn visible_messages_keep_the_tail() {
        let messages: Vec<ChatMessage> = (0..5)
            .map(|index| ChatMessage {
                role: ChatRole::System,
                text: index.to_string(),
            })
            .collect();
        let visible = visible_messages(&messages, 2);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].text, "3");
        assert_eq!(visible[1].text, "4");
    }
}
