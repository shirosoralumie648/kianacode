//! Pure Workbench controller, keymap and command-palette contracts.
//!
//! This module owns only user-intent translation and disposable controller state.  It never
//! calls a daemon, starts a task, cancels a run, resumes a session, or executes a capability.
//! The returned [`WorkbenchUiAction`] is an input to the typed UI client; its protocol digest is
//! supplied by that client boundary before [`kiana_protocol::UiActionV1::validate`] is called.

//! Workbench 的**纯控制器**：把「用户想干什么」翻译成「协议上该发什么」。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 键盘 / 鼠标 / 命令面板
//!    ↓  Keymap / CommandPalette：按键 → WorkbenchIntent
//! 【本文件】纯翻译：intent → WorkbenchUiAction → UiActionV1
//!    ↓  （交给 client 去算 digest、提交）
//! kiana-client 的 typed client
//!    ↓
//! DaemonHost → ControlPlane   ← 授权在那边，不在这里
//! ```
//!
//! # 「纯」是这里最重要的性质
//!
//! 文件头那句「never calls a daemon, starts a task, cancels a run, resumes a session,
//! or executes a capability」不是客套。整层控制器**只做翻译与状态**：
//! 不发请求、不改 run、不动能力、不碰 EventLog。
//!
//! 这样做换来三件事：
//! 1. 它可以被**整个读完再测**——没有 I/O、没有并发、没有时序；
//! 2. 「界面上有个 Cancel 按钮」与「真的取消了某个 run」之间隔着一整条授权链，
//!    按钮只能产生**意图**，取消必须由控制面在拿到命令后决定；
//! 3. 想新增一种界面行为时，改的是这一层的一个 `match`，而不是在四个入口里各写一遍副作用。
//!
//! # 与本仓库其它入口层的对照
//!
//! `command_dispatch.rs` 会真的把请求发给 `DaemonHost`；`harness_run.rs` 会真的发起一次 run。
//! 本文件两者都不做——它是这三个文件里**唯一**可以脱离 daemon 单独运行的一层。
//!
//! # 断线、响应丢失与重启
//!
//! 「纯」不代表可以假装一切顺利。交付结果**可能永远回不来**：网络断了、进程被 SIGTERM 打死、
//! 机器重启。这三种情况下控制器能做的只有一件事——**记住自己发过什么，然后什么都不做**：
//!
//! - [`WorkbenchController::set_connection`] 在离开 `Attached` 时不重试、不取消、不恢复，
//!   只把在途提交标记为 [`SubmissionStatus::Unknown`]，并把它原本的
//!   [`SubmissionIdentity`]（command id + idempotency key）交回调用方；
//! - [`WorkbenchController::note_termination_signal`] 明确 SIGINT/SIGTERM **不是**窗口关闭，
//!   不产生 cancel/resume；
//! - [`WorkbenchController::recover`] 允许新进程只带走**服务端观测到的坐标**与那个未对账的
//!   身份，并且必须延续 idempotency 序号——这样新动作的 key 绝不会撞上已死进程用过的 key；
//! - 在对账完成之前，新的变更意图一律 [`Err`]（`duplicate_submission`）。宁可让用户看到一个
//!   「还在对账」的界面，也不能让一次响应丢失变成第二次副作用。
use kiana_protocol::{
    ExecutionStatus, RequestId, RunId, SessionSummary, UiActionDisposition, UiActionV1,
    UiCapability, UI_ACTION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKBENCH_CONTROLLER_SCHEMA: &str = "kiana.workbench-controller.v1";
pub const MAX_WORKBENCH_SESSIONS: usize = 256;
pub const MAX_COMMAND_PALETTE_ENTRIES: usize = 32;
pub const MAX_CONTROLLER_AUDIT_ENTRIES: usize = 128;
/// 草稿与 prompt 共用的文本上限：64 KiB。
pub const MAX_CONTROLLER_TEXT_BYTES: usize = 64 * 1024;
/// `UiActionV1.target_id` 的线上上限；`workbench.open` 的 workspace 与 attach 的 session id
/// 必须与它对齐，否则一个本地「准备成功」的动作会在 client 校验时才失败。
pub const MAX_WORKBENCH_TARGET_BYTES: usize = 256;
/// 会话标题上限（`ui_session_title` 的线上上限是 512）。
pub const MAX_WORKBENCH_TITLE_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Workbench 能发出的**全部**命令。
///
/// 【为什么要是一个封闭枚举，而不是字符串】
/// 因为它是「这个界面到底能做什么」的完整声明。命令面板里能出现什么、快捷键能绑到什么、
/// 哪些动作在协议上合法——三处都从这一个枚举派生。加一个字符串就等于加一个没人审计过的新能力。
///
/// 它同时也是**能力边界的文档**：读一遍这个枚举，就知道 Workbench 在设计上能做什么、
/// 以及刻意不做什么（比如没有「删除会话」）。
pub enum WorkbenchCommand {
    Open,
    Attach,
    New,
    Continue,
    Status,
    Run,
    Cancel,
    Resume,
    Receipt,
    WindowClose,
}

impl WorkbenchCommand {
    pub const ALL: [Self; 9] = [
        Self::Open,
        Self::Attach,
        Self::New,
        Self::Continue,
        Self::Status,
        Self::Run,
        Self::Cancel,
        Self::Resume,
        Self::Receipt,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "workbench.open",
            Self::Attach => "workbench.attach",
            Self::New => "workbench.new",
            Self::Continue => "workbench.continue",
            Self::Status => "workbench.status",
            Self::Run => "workbench.run",
            Self::Cancel => "workbench.cancel",
            Self::Resume => "workbench.resume",
            Self::Receipt => "workbench.receipt",
            Self::WindowClose => "workbench.window_close",
        }
    }

    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::Open
                | Self::Attach
                | Self::New
                | Self::Continue
                | Self::Run
                | Self::Cancel
                | Self::Resume
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct KeyChord {
    pub key: char,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyChord {
    pub const fn new(key: char, ctrl: bool, alt: bool, shift: bool) -> Self {
        Self {
            key,
            ctrl,
            alt,
            shift,
        }
    }

    pub fn label(self) -> String {
        let mut parts = Vec::with_capacity(4);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        let key = if self.key == ' ' {
            "Space".to_owned()
        } else {
            self.key.to_string()
        };
        if parts.is_empty() {
            key
        } else {
            format!("{}+{key}", parts.join("+"))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyBinding {
    pub chord: KeyChord,
    pub command: WorkbenchCommand,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 按键绑定表。
///
/// 【为什么按键映射要是一等公民，而不是散在事件处理里】
/// 因为「这个键对应哪个命令」是**用户能读懂**的东西：它应该能被列出来、被导出、被改。
/// 埋在 `match key { .. }` 里就只剩写代码的人知道了。
///
/// 也因为它让「同一套命令，既能来自按键、也能来自命令面板」成为可能——
/// 两条入口最后都收敛到同一个 `WorkbenchCommand`。
pub struct Keymap {
    bindings: BTreeMap<KeyChord, WorkbenchCommand>,
}

impl Keymap {
    pub fn new(bindings: impl IntoIterator<Item = KeyBinding>) -> Result<Self, String> {
        let mut map = BTreeMap::new();
        for binding in bindings {
            // 一个键位最多一个命令：否则同一个按键会派发两条命令。
            if map.contains_key(&binding.chord) {
                return Err(format!("duplicate_shortcut:{}", binding.chord.label()));
            }
            // 一个命令最多一个键位：否则「同一次意图」会有两条不同的快捷键路径，
            // 帮助文本也会互相矛盾。
            if map.values().any(|command| *command == binding.command) {
                return Err(format!(
                    "duplicate_command_binding:{}",
                    binding.command.as_str()
                ));
            }
            map.insert(binding.chord, binding.command);
        }
        Ok(Self { bindings: map })
    }

    pub fn default_workbench() -> Self {
        Self::new([
            KeyBinding {
                chord: KeyChord::new('o', true, false, false),
                command: WorkbenchCommand::Open,
            },
            KeyBinding {
                chord: KeyChord::new('a', true, false, false),
                command: WorkbenchCommand::Attach,
            },
            KeyBinding {
                chord: KeyChord::new('n', true, false, false),
                command: WorkbenchCommand::New,
            },
            KeyBinding {
                chord: KeyChord::new('k', true, false, false),
                command: WorkbenchCommand::Status,
            },
            KeyBinding {
                chord: KeyChord::new('r', true, false, false),
                command: WorkbenchCommand::Run,
            },
            KeyBinding {
                chord: KeyChord::new('c', true, false, false),
                command: WorkbenchCommand::Cancel,
            },
            KeyBinding {
                chord: KeyChord::new('u', true, false, false),
                command: WorkbenchCommand::Resume,
            },
            KeyBinding {
                chord: KeyChord::new('e', true, false, false),
                command: WorkbenchCommand::Receipt,
            },
        ])
        .expect("default Workbench keymap has unique shortcuts")
    }

    pub fn resolve(&self, chord: KeyChord) -> Option<WorkbenchCommand> {
        self.bindings.get(&chord).copied()
    }

    pub fn bindings(&self) -> impl Iterator<Item = KeyBinding> + '_ {
        self.bindings.iter().map(|(chord, command)| KeyBinding {
            chord: *chord,
            command: *command,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommandPaletteEntry {
    pub command: WorkbenchCommand,
    pub label: String,
    pub capability_id: String,
    #[serde(default)]
    pub shortcut: Option<KeyChord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 命令面板：按名字检索可用命令。
///
/// 【与 Keymap 的关系】
/// 两者是**同一套命令的两种入口**。面板按名字找，键位按物理键找。
/// 把它们分开，是因为它们的可发现性来源不同：面板要能被搜索，键位要能被打印成帮助。
pub struct CommandPalette {
    entries: Vec<CommandPaletteEntry>,
}

impl CommandPalette {
    pub fn new(entries: Vec<CommandPaletteEntry>) -> Result<Self, String> {
        if entries.is_empty() || entries.len() > MAX_COMMAND_PALETTE_ENTRIES {
            return Err("command_palette_size_invalid".to_owned());
        }
        let mut commands = BTreeSet::new();
        let mut shortcuts = BTreeSet::new();
        for entry in &entries {
            if entry.label.trim().is_empty() || entry.label.len() > 128 {
                return Err("command_palette_label_invalid".to_owned());
            }
            if entry.capability_id.trim().is_empty() || entry.capability_id.len() > 128 {
                return Err("command_palette_capability_invalid".to_owned());
            }
            if !commands.insert(entry.command) {
                return Err(format!(
                    "command_palette_duplicate_command:{}",
                    entry.command.as_str()
                ));
            }
            if let Some(shortcut) = entry.shortcut {
                if !shortcuts.insert(shortcut) {
                    return Err(format!("duplicate_shortcut:{}", shortcut.label()));
                }
            }
        }
        Ok(Self { entries })
    }

    pub fn default_workbench() -> Self {
        let keymap = Keymap::default_workbench();
        let entries = WorkbenchCommand::ALL
            .into_iter()
            .map(|command| CommandPaletteEntry {
                command,
                label: command.as_str().replace("workbench.", ""),
                capability_id: command.as_str().to_owned(),
                shortcut: keymap
                    .bindings()
                    .find(|binding| binding.command == command)
                    .map(|binding| binding.chord),
            })
            .collect();
        Self::new(entries).expect("default Workbench palette is valid")
    }

    pub fn entries(&self) -> &[CommandPaletteEntry] {
        &self.entries
    }

    /// Return only commands explicitly advertised by an enabled capability.
    pub fn visible(&self, capabilities: &[UiCapability]) -> Vec<CommandPaletteEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                capabilities.iter().any(|capability| {
                    capability.enabled
                        && (capability.capability_id == entry.capability_id
                            || capability
                                .actions
                                .iter()
                                .any(|action| action.as_str() == entry.command.as_str()))
                })
            })
            .cloned()
            .collect()
    }

    pub fn find_visible(
        &self,
        command: WorkbenchCommand,
        capabilities: &[UiCapability],
    ) -> Option<CommandPaletteEntry> {
        self.visible(capabilities)
            .into_iter()
            .find(|entry| entry.command == command)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 会话切换器。
///
/// 【它属于「纯」的一层意味着什么】
/// 切换会话只改变「当前看哪个会话」这个本地状态。它**不会**取消或恢复任何 run——
/// 那些都要发命令、都要控制面点头。会话切换器只负责「我现在在看谁」。
pub struct SessionSwitcher {
    sessions: Vec<SessionSummary>,
    active_session_id: Option<String>,
}

impl SessionSwitcher {
    pub fn new(
        sessions: Vec<SessionSummary>,
        active_session_id: Option<String>,
    ) -> Result<Self, String> {
        if sessions.len() > MAX_WORKBENCH_SESSIONS {
            return Err("session_switcher_limit".to_owned());
        }
        let mut ids = BTreeSet::new();
        for session in &sessions {
            if !ids.insert(session.session_id.to_string()) {
                return Err("session_switcher_duplicate_session".to_owned());
            }
        }
        if active_session_id
            .as_deref()
            .is_some_and(|id| !ids.contains(id))
        {
            return Err("session_switcher_active_missing".to_owned());
        }
        Ok(Self {
            sessions,
            active_session_id,
        })
    }

    pub fn sessions(&self) -> &[SessionSummary] {
        &self.sessions
    }

    pub fn active_session_id(&self) -> Option<&str> {
        self.active_session_id.as_deref()
    }

    pub fn select(&mut self, session_id: &str) -> Result<bool, String> {
        if !self
            .sessions
            .iter()
            .any(|session| session.session_id.to_string() == session_id)
        {
            return Err("session_switcher_session_missing".to_owned());
        }
        let changed = self.active_session_id.as_deref() != Some(session_id);
        self.active_session_id = Some(session_id.to_owned());
        Ok(changed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 用户意图：比命令**更宽**的一层。
///
/// 【为什么要分 intent 和 command 两层】
/// 因为用户表达意图的方式比协议命令丰富：`Attach` 带着目标、`Run` 带着 prompt、
/// `Continue` 带着要续的 run。这些参数属于**界面**。
/// 而协议命令只需要一个名字加一个 payload。
///
/// 分成两层之后，界面参数不会渗进协议，而协议命令也不会因为界面多一种表达方式就变多。
///
/// `WindowClose` 刻意没有对应命令——它不是一次操作系统的命令，只是一个「离开」信号。
pub enum WorkbenchIntent {
    Open {
        workspace: String,
    },
    Attach {
        session_id: String,
    },
    New {
        title: Option<String>,
    },
    Continue {
        run_id: Option<RunId>,
        prompt: String,
        draft_revision: u64,
    },
    Status,
    Run {
        prompt: String,
        draft_revision: u64,
    },
    Cancel {
        run_id: Option<RunId>,
    },
    Resume {
        run_id: Option<RunId>,
    },
    Receipt {
        run_id: Option<RunId>,
    },
    WindowClose,
}

impl WorkbenchIntent {
    /// 把意图**收窄**成命令名。
    ///
    /// 【这个方法为什么重要】
    /// 它是意图与协议之间唯一的收敛点。收窄之后，参数被放进 payload，名字被固定，
    /// 后面无论走哪条路（按键、面板、脚本），拿到的都是同一个 `WorkbenchCommand`。
    ///
    /// 【返回 `Option` 而不是 `enum`】
    /// 因为确实存在「有意图但没有命令」的情况（`WindowClose`）。
    /// 用 `Option` 表达「这一条走不通」比造一个假的命令名更诚实。
    fn command(&self) -> Option<WorkbenchCommand> {
        match self {
            Self::Open { .. } => Some(WorkbenchCommand::Open),
            Self::Attach { .. } => Some(WorkbenchCommand::Attach),
            Self::New { .. } => Some(WorkbenchCommand::New),
            Self::Continue { .. } => Some(WorkbenchCommand::Continue),
            Self::Status => Some(WorkbenchCommand::Status),
            Self::Run { .. } => Some(WorkbenchCommand::Run),
            Self::Cancel { .. } => Some(WorkbenchCommand::Cancel),
            Self::Resume { .. } => Some(WorkbenchCommand::Resume),
            Self::Receipt { .. } => Some(WorkbenchCommand::Receipt),
            Self::WindowClose => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 交给 client 的动作，尚未成为协议动作。
///
/// 【`expected_*` 三个字段是这一层最要紧的设计】
/// `expected_epoch` / `expected_cursor` / `expected_revision` 表达的是
/// **「我以为现在是这样」**。它们是乐观并发：提交时控制面会比对，不一致就拒绝。
///
/// 为什么界面要主动报出自己看到的位置？因为人和机器都会慢半拍——用户盯着一个
/// 已经变了的界面点「取消」，如果系统照做，取消的就是另一个 run。把这三个数报出来，
/// 就能把这种时差变成一次明确的拒绝，而不是一次执行在错误对象上的取消。
///
/// 【`idempotency_key`】
/// 同一次点击被重发（网络重试、用户双击）不能变成两次操作。
pub struct WorkbenchUiAction {
    pub command: WorkbenchCommand,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub target_id: String,
    pub expected_epoch: String,
    pub expected_cursor: u64,
    pub expected_revision: Option<u64>,
    pub payload: Value,
    pub submitted_by: String,
}

impl WorkbenchUiAction {
    /// 这次提交的稳定身份。响应丢失之后，能交回调用方的就是它：command id 用于对账，
    /// idempotency key 用于让服务端认出「这是同一次点击」。
    pub fn identity(&self) -> SubmissionIdentity {
        SubmissionIdentity {
            command: self.command,
            command_id: self.command_id,
            idempotency_key: self.idempotency_key.clone(),
        }
    }

    /// Convert the controller intent into the versioned wire action after the client has computed
    /// the canonical payload digest.  The controller cannot authorize or execute this action.
    /// 转成协议动作。
    ///
    /// 【⚠ 为什么 `payload_digest` 是参数而不是在这里算】
    /// 这是整个模块最容易被「顺手优化掉」的地方。digest 只能由**能看到规范化字节**
    /// 的那一层计算，而本层不是那一层：它既不决定最终发出去的字节，也不该假装自己知道。
    /// 于是它把 digest 收下，转手填进协议动作。
    ///
    /// 如果改成在这里自己算，就会出现两个问题：一是可能算出与真正发出去的字节不一致的
    /// digest（于是控制面永远拒绝，表现为「这个界面坏了」）；二是控制面将无法再用
    /// 「你报的 digest 与你发的内容是否一致」来发现伪造。所以这一步的注释写在这里，
    /// 而不是留给下一个读代码的人去发现。
    ///
    /// 【`deadline_unix_ms` 为什么写死为 `None`】
    /// 超时是控制面的策略，不是界面的决定。界面无权给自己发的动作设一个期限。
    pub fn into_protocol(self, payload_digest: impl Into<String>) -> UiActionV1 {
        UiActionV1 {
            schema: UI_ACTION_SCHEMA.to_owned(),
            command_id: self.command_id,
            idempotency_key: self.idempotency_key,
            target_id: self.target_id,
            expected_epoch: self.expected_epoch,
            expected_cursor: self.expected_cursor,
            expected_revision: self.expected_revision,
            payload: self.payload,
            payload_digest: payload_digest.into(),
            submitted_by: self.submitted_by,
            deadline_unix_ms: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 控制器与 daemon 之间的连接边界。
///
/// 【为什么 `Degraded` 也让本地拒绝新提交】
/// 因为控制器没有发送队列。允许「先准备、稍后发」等于把重试循环藏进界面里——
/// 那正是不能出现的东西（重试必须由控制面按原 idempotency key 决定）。
/// 所以除了 `Attached`，任何状态都只能观察与编辑草稿，不能准备动作。
pub enum ConnectionStatus {
    Attached,
    Degraded,
    Closed,
}

impl ConnectionStatus {
    /// 只有 `Attached` 才允许准备面向服务端的意图。
    pub const fn accepts_submissions(self) -> bool {
        matches!(self, Self::Attached)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 终止信号。
///
/// 【为什么它要做成一等公民】
/// 因为「进程收到 SIGTERM」最容易被顺手实现成「退出前先 cancel 一下」。
/// 那是界面替控制面做的决定，也是本项目明确拒绝的行为。把信号写进状态，
/// 就能断言：信号只让在途提交进入「结果未知」，**不产生任何动作**。
pub enum TerminationSignal {
    Interrupt,
    Terminate,
}

impl TerminationSignal {
    pub const ALL: [Self; 2] = [Self::Interrupt, Self::Terminate];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interrupt => "sigint",
            Self::Terminate => "sigterm",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
/// 一次提交的**稳定身份**：command id + idempotency key。
///
/// 【为什么它要和 payload 分开】
/// 因为对账只需要身份。响应丢失之后，控制器能交给下一个进程的只有「我发过哪个命令」，
/// 而不是「我当时打算干什么」——后者一旦能带走，就等于允许新进程重放旧副作用。
pub struct SubmissionIdentity {
    pub command: WorkbenchCommand,
    pub command_id: RequestId,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 离开 `Attached` 时，在途提交的去向。
///
/// `Unknown(identity)` 是**唯一**的合法结果：既不能假装成功（`Applied`），
/// 也不能假装失败后重发（新 command id）。
pub enum InFlightDisposition {
    None,
    Unknown(SubmissionIdentity),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionTransition {
    pub previous: ConnectionStatus,
    pub current: ConnectionStatus,
    pub in_flight: InFlightDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    Idle,
    Preparing,
    Accepted,
    Applied,
    Rejected,
    Unknown,
}

impl SubmissionStatus {
    fn blocks_mutation(self) -> bool {
        matches!(self, Self::Preparing | Self::Accepted | Self::Unknown)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Idle,
    Running,
    Cancelling,
    AwaitingApproval,
    Completed,
    Cancelled,
    Failed,
    ResultUnknown,
}

impl RunStatus {
    fn from_execution(status: ExecutionStatus) -> Self {
        match status {
            ExecutionStatus::AwaitingApproval => Self::AwaitingApproval,
            ExecutionStatus::Running | ExecutionStatus::Accepted | ExecutionStatus::Queued => {
                Self::Running
            }
            ExecutionStatus::Cancelling => Self::Cancelling,
            ExecutionStatus::Completed => Self::Completed,
            ExecutionStatus::Cancelled => Self::Cancelled,
            ExecutionStatus::ResultUnknown => Self::ResultUnknown,
            ExecutionStatus::Failed | ExecutionStatus::Denied | ExecutionStatus::Blocked => {
                Self::Failed
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerState {
    pub connection: ConnectionStatus,
    pub termination: Option<TerminationSignal>,
    pub submission: SubmissionStatus,
    pub run: RunStatus,
    pub active_session_id: Option<String>,
    pub draft_revision: u64,
    pub draft_bytes: usize,
    pub epoch: String,
    pub cursor: u64,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerDecision {
    Action(WorkbenchUiAction),
    WindowClosed { run_pending: bool },
    SessionChanged { session_id: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDisposition {
    Prepared,
    Accepted,
    Applied,
    Rejected,
    Unknown,
    /// 身份跨进程边界被带走、结局仍未对账（见 [`WorkbenchController::recover`]）。
    Recovered,
    WindowClosed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionAuditEntry {
    pub sequence: u64,
    pub command: WorkbenchCommand,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub disposition: AuditDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkbenchController {
    workspace: String,
    principal: String,
    keymap: Keymap,
    palette: CommandPalette,
    switcher: SessionSwitcher,
    capabilities: Vec<UiCapability>,
    connection: ConnectionStatus,
    termination: Option<TerminationSignal>,
    submission: SubmissionStatus,
    run: RunStatus,
    epoch: String,
    cursor: u64,
    revision: Option<u64>,
    draft: String,
    draft_revision: u64,
    sequence: u64,
    /// 未结算提交的**身份**。
    ///
    /// 【为什么这里只有 identity、没有 payload】
    /// 控制器故意不保存动作内容：把它交给调用方之后，手里只剩下「我发过哪个命令」，
    /// 结构上就没有可重放的东西。跨进程恢复能带走的也只有这一份身份。
    in_flight: Option<SubmissionIdentity>,
    audit: Vec<ActionAuditEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 重启恢复的**输入**：只允许服务端观测到的坐标与集合。
///
/// 【为什么它是一个独立结构而不是一串参数】
/// 因为它同时是一张白名单：这里没有 `draft`、没有 `submission`、没有 in-flight action 的
/// 位置。调用方**无法**通过恢复把上一个进程的本地草稿或在途副作用带进来——编译器替我们
/// 守着这条边界。
pub struct RecoveryObservation {
    pub workspace: String,
    pub principal: String,
    pub epoch: String,
    pub cursor: u64,
    pub revision: Option<u64>,
    pub connection: ConnectionStatus,
    pub sessions: Vec<SessionSummary>,
    pub active_session_id: Option<String>,
    pub capabilities: Vec<UiCapability>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// 重启恢复的**结果**：新控制器 + 仍需对账的原始身份（没有就为 `None`）。
pub struct WorkbenchRecovery {
    pub controller: WorkbenchController,
    pub reconcile: Option<SubmissionIdentity>,
}

impl WorkbenchController {
    pub fn new(
        workspace: impl Into<String>,
        principal: impl Into<String>,
        epoch: impl Into<String>,
        cursor: u64,
        revision: Option<u64>,
        sessions: Vec<SessionSummary>,
        active_session_id: Option<String>,
        capabilities: Vec<UiCapability>,
    ) -> Result<Self, String> {
        let workspace = workspace.into();
        let principal = principal.into();
        let epoch = epoch.into();
        if workspace.trim().is_empty() || principal.trim().is_empty() || epoch.trim().is_empty() {
            return Err("workbench_controller_scope_invalid".to_owned());
        }
        if cursor == 0 || revision == Some(0) {
            return Err("workbench_controller_cursor_invalid".to_owned());
        }
        for capability in &capabilities {
            capability.validate()?;
        }
        let switcher = SessionSwitcher::new(sessions, active_session_id)?;
        Ok(Self {
            workspace,
            principal,
            keymap: Keymap::default_workbench(),
            palette: CommandPalette::default_workbench(),
            switcher,
            capabilities,
            connection: ConnectionStatus::Attached,
            termination: None,
            submission: SubmissionStatus::Idle,
            run: RunStatus::Idle,
            epoch,
            cursor,
            revision,
            draft: String::new(),
            draft_revision: 1,
            sequence: 0,
            in_flight: None,
            audit: Vec::new(),
        })
    }

    pub fn state(&self) -> ControllerState {
        ControllerState {
            connection: self.connection,
            termination: self.termination,
            submission: self.submission,
            run: self.run,
            active_session_id: self.switcher.active_session_id().map(str::to_owned),
            draft_revision: self.draft_revision,
            draft_bytes: self.draft.len(),
            epoch: self.epoch.clone(),
            cursor: self.cursor,
            revision: self.revision,
        }
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn draft_revision(&self) -> u64 {
        self.draft_revision
    }

    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    pub fn command_palette(&self) -> &CommandPalette {
        &self.palette
    }

    pub fn visible_commands(&self) -> Vec<CommandPaletteEntry> {
        self.palette.visible(&self.capabilities)
    }

    pub fn session_switcher(&self) -> &SessionSwitcher {
        &self.switcher
    }

    pub fn audit(&self) -> &[ActionAuditEntry] {
        &self.audit
    }

    /// 仍未对账的提交身份。
    ///
    /// 只要它是 `Some`，新的变更意图就会被 [`WorkbenchController::dispatch`] 拒绝：
    /// 客户端必须先用**同一个** command id / idempotency key 查询服务端，把结果交回
    /// [`WorkbenchController::record_action_result`]。
    pub fn pending_identity(&self) -> Option<SubmissionIdentity> {
        self.in_flight.clone()
    }

    pub fn set_draft(&mut self, draft: impl Into<String>) -> Result<(), String> {
        if self.connection == ConnectionStatus::Closed {
            return Err("workbench_controller_closed".to_owned());
        }
        // 断线时草稿仍然可以编辑（纯本地状态），但进程已在终止流程里就不要再接受输入。
        if self.termination.is_some() {
            return Err("workbench_controller_terminating".to_owned());
        }
        let draft = draft.into();
        if draft.len() > MAX_CONTROLLER_TEXT_BYTES || draft.contains('\0') {
            return Err("workbench_draft_invalid".to_owned());
        }
        self.draft = draft;
        self.draft_revision = self.draft_revision.saturating_add(1);
        Ok(())
    }

    /// 移动连接边界。
    ///
    /// 【离开 `Attached` 时唯一允许做的事】
    /// 把在途提交标记为 [`SubmissionStatus::Unknown`]（结局未知），并把它的原始身份交回
    /// 调用方。不重试、不发新命令、不 cancel、不 resume，也不改写 run 维度。
    /// 重新回到 `Attached` 也不会「自动补发」——未对账的提交仍然挡着新的变更。
    pub fn set_connection(&mut self, connection: ConnectionStatus) -> ConnectionTransition {
        let previous = self.connection;
        self.connection = connection;
        let in_flight = if connection.accepts_submissions() {
            InFlightDisposition::None
        } else {
            self.mark_delivery_unknown()
        };
        ConnectionTransition {
            previous,
            current: connection,
            in_flight,
        }
    }

    /// SIGINT / SIGTERM：进程级终止信号。
    ///
    /// 【它为什么不是 `WindowClose`】
    /// 关窗是用户动作，信号不是。两者都**不**产生 cancel/resume，但信号还必须额外表达
    /// 「本进程不会再发任何东西」：因此控制器进入终止态，除关窗外的意图一律拒绝。
    pub fn note_termination_signal(&mut self, signal: TerminationSignal) -> InFlightDisposition {
        self.termination.get_or_insert(signal);
        self.mark_delivery_unknown()
    }

    pub fn switch_session(&mut self, session_id: &str) -> Result<ControllerDecision, String> {
        if self.connection == ConnectionStatus::Closed {
            return Err("workbench_controller_closed".to_owned());
        }
        if self.termination.is_some() {
            return Err("workbench_controller_terminating".to_owned());
        }
        self.switcher.select(session_id)?;
        self.draft.clear();
        self.draft_revision = self.draft_revision.saturating_add(1);
        // 会话切换只清理**已结算**的提交显示：未对账的提交身份仍然挡着新的变更，
        // 否则「切个会话」就成了绕过响应丢失对账的捷径。
        if self.in_flight.is_none() {
            self.submission = SubmissionStatus::Idle;
        }
        Ok(ControllerDecision::SessionChanged {
            session_id: session_id.to_owned(),
        })
    }

    /// 用上一个进程里**可安全带走**的部分重建控制器。
    ///
    /// 可以带走的只有服务端观测到的坐标/集合，以及那个未对账的提交身份；带不走草稿、
    /// 提交状态断言或待发动作。几条硬规则：
    ///
    /// 1. 作用域（workspace/principal/epoch）必须不变——换了世界就该用 [`Self::new`]；
    /// 2. 坐标（cursor/revision）只能前进：重启不能让本地时间倒流；
    /// 3. 活动会话必须与旧控制器一致——换会话是用户动作（`switch_session`），不是恢复副作用；
    /// 4. idempotency 序号延续：新动作的 key 不会与已死进程用过的 key 相同；
    /// 5. 有未对账身份时，恢复出来的控制器直接停在「等待对账」，而不是 `Idle`。
    pub fn recover(&self, observation: RecoveryObservation) -> Result<WorkbenchRecovery, String> {
        let RecoveryObservation {
            workspace,
            principal,
            epoch,
            cursor,
            revision,
            connection,
            sessions,
            active_session_id,
            capabilities,
        } = observation;
        if !connection.accepts_submissions() {
            return Err("workbench_recovery_offline".to_owned());
        }
        if workspace.trim().is_empty() || principal.trim().is_empty() || epoch.trim().is_empty() {
            return Err("workbench_controller_scope_invalid".to_owned());
        }
        if cursor == 0 || revision == Some(0) {
            return Err("workbench_controller_cursor_invalid".to_owned());
        }
        if workspace != self.workspace || principal != self.principal {
            return Err("workbench_recovery_scope_changed".to_owned());
        }
        if epoch != self.epoch {
            return Err("workbench_recovery_epoch_mismatch".to_owned());
        }
        if cursor < self.cursor {
            return Err("workbench_recovery_cursor_stale".to_owned());
        }
        if matches!((revision, self.revision), (Some(observed), Some(known)) if observed < known) {
            return Err("workbench_recovery_revision_stale".to_owned());
        }
        if self.switcher.active_session_id().is_some()
            && active_session_id.as_deref() != self.switcher.active_session_id()
        {
            return Err("workbench_recovery_session_changed".to_owned());
        }
        for capability in &capabilities {
            capability.validate()?;
        }
        let switcher = SessionSwitcher::new(sessions, active_session_id)?;
        let reconcile = self.pending_identity();
        let mut controller = Self {
            workspace,
            principal,
            keymap: Keymap::default_workbench(),
            palette: CommandPalette::default_workbench(),
            switcher,
            capabilities,
            connection: ConnectionStatus::Attached,
            termination: None,
            submission: SubmissionStatus::Idle,
            run: RunStatus::Idle,
            epoch,
            cursor,
            revision,
            draft: String::new(),
            // 草稿内容不带走，但版本边界必须前进：旧进程构建的意图一律 stale_draft。
            draft_revision: self.draft_revision.saturating_add(1),
            // 序号延续，否则新动作的 idempotency key 可能与已死进程的重名。
            sequence: self.sequence,
            in_flight: None,
            audit: Vec::new(),
        };
        if let Some(identity) = reconcile.clone() {
            controller.submission = SubmissionStatus::Unknown;
            controller.in_flight = Some(identity.clone());
            controller.record_identity(&identity, AuditDisposition::Recovered);
        }
        Ok(WorkbenchRecovery {
            controller,
            reconcile,
        })
    }

    pub fn dispatch(&mut self, intent: WorkbenchIntent) -> Result<ControllerDecision, String> {
        if matches!(&intent, WorkbenchIntent::WindowClose) {
            self.connection = ConnectionStatus::Closed;
            let run_pending = !matches!(self.run, RunStatus::Idle | RunStatus::Completed);
            self.record_window_close(run_pending);
            return Ok(ControllerDecision::WindowClosed { run_pending });
        }
        if self.connection == ConnectionStatus::Closed {
            return Err("workbench_controller_closed".to_owned());
        }
        // 终止流程中的进程不再准备任何新意图（关窗已在上面处理）。
        if self.termination.is_some() {
            return Err("workbench_controller_terminating".to_owned());
        }
        // 断线期间不能「先准备、稍后发」：控制器没有发送队列，重试属于控制面。
        if !self.connection.accepts_submissions() {
            return Err("workbench_controller_offline".to_owned());
        }
        let command = intent
            .command()
            .ok_or_else(|| "workbench_intent_invalid".to_owned())?;
        self.require_capability(command)?;
        // 同一时刻只允许一个未结算的提交。`submission` 是展示口径，`in_flight` 是事实口径：
        // 只要还留着身份（哪怕显示已被会话切换清成 Idle），就不能准备第二条变更。
        if command.is_mutating() && (self.submission.blocks_mutation() || self.in_flight.is_some())
        {
            return Err("duplicate_submission".to_owned());
        }
        let (target_id, payload, clears_draft) = self.intent_payload(&intent, command)?;
        if matches!(&intent, WorkbenchIntent::Run { draft_revision, .. } if *draft_revision != self.draft_revision)
            || matches!(&intent, WorkbenchIntent::Continue { draft_revision, .. } if *draft_revision != self.draft_revision)
        {
            return Err("stale_draft".to_owned());
        }
        if clears_draft {
            self.draft.clear();
            self.draft_revision = self.draft_revision.saturating_add(1);
        }
        self.sequence = self.sequence.saturating_add(1);
        let action = WorkbenchUiAction {
            command,
            command_id: RequestId::new(),
            idempotency_key: format!("workbench:{}:{}", command.as_str(), self.sequence),
            target_id,
            expected_epoch: self.epoch.clone(),
            expected_cursor: self.cursor,
            expected_revision: self.revision,
            payload,
            submitted_by: self.principal.clone(),
        };
        if command.is_mutating()
            || matches!(
                command,
                WorkbenchCommand::Status | WorkbenchCommand::Receipt
            )
        {
            self.submission = SubmissionStatus::Preparing;
            self.in_flight = Some(action.identity());
        }
        self.record_audit(&action, AuditDisposition::Prepared);
        Ok(ControllerDecision::Action(action))
    }

    /// 回填一次提交的结果。
    ///
    /// 【为什么 command id 必须对得上】
    /// 一张「卡片」代表一次点击。结果里的 command id 不匹配，说明它属于别的点击（stale card）：
    /// 既不能拿它结算当前提交，也不能把它当成成功。于是这里只做两件事——结算或者拒绝。
    pub fn record_action_result(
        &mut self,
        command_id: RequestId,
        disposition: UiActionDisposition,
    ) -> Result<(), String> {
        let Some(pending) = self.in_flight.clone() else {
            return Err("action_result_without_submission".to_owned());
        };
        if pending.command_id != command_id {
            return Err("action_result_command_mismatch".to_owned());
        }
        let (submission, audit) = match disposition {
            UiActionDisposition::Accepted => {
                (SubmissionStatus::Accepted, AuditDisposition::Accepted)
            }
            UiActionDisposition::Applied => (SubmissionStatus::Applied, AuditDisposition::Applied),
            UiActionDisposition::Rejected => {
                (SubmissionStatus::Rejected, AuditDisposition::Rejected)
            }
            UiActionDisposition::Unknown => (SubmissionStatus::Unknown, AuditDisposition::Unknown),
        };
        self.submission = submission;
        self.record_identity(&pending, audit);
        if matches!(
            disposition,
            UiActionDisposition::Applied | UiActionDisposition::Rejected
        ) {
            self.in_flight = None;
            if self.submission == SubmissionStatus::Rejected {
                self.submission = SubmissionStatus::Idle;
            }
        }
        Ok(())
    }

    /// 观察服务端投影出来的 run 状态。
    ///
    /// run 维度与 submission 维度**互相独立**：run 到达终态并不能证明某次提交的结局已知
    /// （例如响应在网络上丢了，或者丢的是另一条命令的结果）。所以这里只推进 run 维度；
    /// 未对账的提交身份必须由 [`Self::record_action_result`] 显式结算后才能放行新的变更。
    pub fn observe_run_status(&mut self, status: ExecutionStatus) {
        self.run = RunStatus::from_execution(status);
        if matches!(
            self.run,
            RunStatus::Completed | RunStatus::Cancelled | RunStatus::Failed
        ) && self.in_flight.is_none()
        {
            self.submission = SubmissionStatus::Idle;
        }
    }

    fn require_capability(&self, command: WorkbenchCommand) -> Result<(), String> {
        if self
            .palette
            .find_visible(command, &self.capabilities)
            .is_some()
        {
            Ok(())
        } else {
            Err(format!("capability_unavailable:{}", command.as_str()))
        }
    }

    fn intent_payload(
        &self,
        intent: &WorkbenchIntent,
        command: WorkbenchCommand,
    ) -> Result<(String, Value, bool), String> {
        let active = self
            .switcher
            .active_session_id()
            .map(str::to_owned)
            .unwrap_or_else(|| self.workspace.clone());
        match intent {
            WorkbenchIntent::Open { workspace } => {
                // UiActionV1.target_id is bounded to 256 bytes; keep the controller and wire
                // contract aligned so an action cannot be prepared only to fail at the client.
                validate_text(workspace, "workspace", MAX_WORKBENCH_TARGET_BYTES)?;
                Ok((workspace.clone(), json!({"workspace": workspace}), true))
            }
            WorkbenchIntent::Attach { session_id } => {
                validate_text(session_id, "session_id", MAX_WORKBENCH_TARGET_BYTES)?;
                Ok((session_id.clone(), json!({"session_id": session_id}), true))
            }
            WorkbenchIntent::New { title } => {
                if let Some(title) = title {
                    validate_text(title, "title", MAX_WORKBENCH_TITLE_BYTES)?;
                }
                Ok((self.workspace.clone(), json!({"title": title}), true))
            }
            WorkbenchIntent::Continue { run_id, prompt, .. } => {
                validate_prompt(prompt)?;
                Ok((active, json!({"run_id": run_id, "prompt": prompt}), false))
            }
            WorkbenchIntent::Status => Ok((active.clone(), json!({"session_id": active}), false)),
            WorkbenchIntent::Run { prompt, .. } => {
                validate_prompt(prompt)?;
                Ok((active, json!({"prompt": prompt}), false))
            }
            WorkbenchIntent::Cancel { run_id }
            | WorkbenchIntent::Resume { run_id }
            | WorkbenchIntent::Receipt { run_id } => {
                if command == WorkbenchCommand::Cancel
                    && run_id.is_none()
                    && self.run == RunStatus::Idle
                {
                    return Err("run_required".to_owned());
                }
                Ok((active, json!({"run_id": run_id}), false))
            }
            WorkbenchIntent::WindowClose => Err("workbench_intent_invalid".to_owned()),
        }
    }

    fn record_audit(&mut self, action: &WorkbenchUiAction, disposition: AuditDisposition) {
        self.sequence = self.sequence.max(1);
        self.record_identity(&action.identity(), disposition);
    }

    /// 按**身份**记一条审计。身份是跨进程也可以安全带走的东西，所以断线、信号和恢复
    /// 走的都是这一条路径，而不是各自发明一套记录。
    fn record_identity(&mut self, identity: &SubmissionIdentity, disposition: AuditDisposition) {
        self.sequence = self.sequence.max(1);
        self.audit.push(ActionAuditEntry {
            sequence: self.sequence,
            command: identity.command,
            command_id: identity.command_id,
            idempotency_key: identity.idempotency_key.clone(),
            disposition,
        });
        self.trim_audit();
    }

    /// 离开 `Attached`（或进程收到终止信号）时，把在途提交从「可能成功」改写成「结局未知」，
    /// 并把身份交回调用方。已经是对账过的提交不重复记录。
    fn mark_delivery_unknown(&mut self) -> InFlightDisposition {
        let Some(pending) = self.in_flight.clone() else {
            return InFlightDisposition::None;
        };
        if self.submission != SubmissionStatus::Unknown {
            self.submission = SubmissionStatus::Unknown;
            self.record_identity(&pending, AuditDisposition::Unknown);
        }
        InFlightDisposition::Unknown(pending)
    }

    fn record_window_close(&mut self, _run_pending: bool) {
        self.sequence = self.sequence.saturating_add(1);
        self.audit.push(ActionAuditEntry {
            sequence: self.sequence,
            command: WorkbenchCommand::WindowClose,
            command_id: RequestId::new(),
            idempotency_key: format!("workbench:window-close:{}", self.sequence),
            disposition: AuditDisposition::WindowClosed,
        });
        self.trim_audit();
    }

    fn trim_audit(&mut self) {
        if self.audit.len() > MAX_CONTROLLER_AUDIT_ENTRIES {
            let overflow = self.audit.len() - MAX_CONTROLLER_AUDIT_ENTRIES;
            self.audit.drain(0..overflow);
        }
    }
}

fn validate_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("workbench_{field}_invalid"));
    }
    Ok(())
}

fn validate_prompt(prompt: &str) -> Result<(), String> {
    validate_text(prompt, "prompt", MAX_CONTROLLER_TEXT_BYTES)
}
