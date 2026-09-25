//! Pure Workbench controller, keymap and command-palette contracts.
//!
//! This module owns only user-intent translation and disposable controller state.  It never
//! calls a daemon, starts a task, cancels a run, resumes a session, or executes a capability.
//! The returned [`WorkbenchUiAction`] is an input to the typed UI client; its protocol digest is
//! supplied by that client boundary before [`kiana_protocol::UiActionV1::validate`] is called.

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
pub struct Keymap {
    bindings: BTreeMap<KeyChord, WorkbenchCommand>,
}

impl Keymap {
    pub fn new(bindings: impl IntoIterator<Item = KeyBinding>) -> Result<Self, String> {
        let mut map = BTreeMap::new();
        for binding in bindings {
            if map.insert(binding.chord, binding.command).is_some() {
                return Err(format!("duplicate_shortcut:{}", binding.chord.label()));
            }
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
    /// Convert the controller intent into the versioned wire action after the client has computed
    /// the canonical payload digest.  The controller cannot authorize or execute this action.
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
pub enum ConnectionStatus {
    Attached,
    Degraded,
    Closed,
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
    submission: SubmissionStatus,
    run: RunStatus,
    epoch: String,
    cursor: u64,
    revision: Option<u64>,
    draft: String,
    draft_revision: u64,
    sequence: u64,
    in_flight: Option<WorkbenchUiAction>,
    audit: Vec<ActionAuditEntry>,
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

    pub fn set_draft(&mut self, draft: impl Into<String>) -> Result<(), String> {
        if self.connection == ConnectionStatus::Closed {
            return Err("workbench_controller_closed".to_owned());
        }
        let draft = draft.into();
        if draft.len() > 64 * 1024 || draft.contains('\0') {
            return Err("workbench_draft_invalid".to_owned());
        }
        self.draft = draft;
        self.draft_revision = self.draft_revision.saturating_add(1);
        Ok(())
    }

    pub fn set_connection(&mut self, connection: ConnectionStatus) {
        self.connection = connection;
    }

    pub fn switch_session(&mut self, session_id: &str) -> Result<ControllerDecision, String> {
        if self.connection == ConnectionStatus::Closed {
            return Err("workbench_controller_closed".to_owned());
        }
        self.switcher.select(session_id)?;
        self.draft.clear();
        self.draft_revision = self.draft_revision.saturating_add(1);
        self.submission = SubmissionStatus::Idle;
        Ok(ControllerDecision::SessionChanged {
            session_id: session_id.to_owned(),
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
        let command = intent
            .command()
            .ok_or_else(|| "workbench_intent_invalid".to_owned())?;
        self.require_capability(command)?;
        if command.is_mutating() && self.submission.blocks_mutation() {
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
            self.in_flight = Some(action.clone());
        }
        self.record_audit(&action, AuditDisposition::Prepared);
        Ok(ControllerDecision::Action(action))
    }

    pub fn record_action_result(
        &mut self,
        command_id: RequestId,
        disposition: UiActionDisposition,
    ) -> Result<(), String> {
        let Some(action) = self.in_flight.clone() else {
            return Err("action_result_without_submission".to_owned());
        };
        if action.command_id != command_id {
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
        self.record_audit(&action, audit);
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

    pub fn observe_run_status(&mut self, status: ExecutionStatus) {
        self.run = RunStatus::from_execution(status);
        if matches!(
            self.run,
            RunStatus::Completed | RunStatus::Cancelled | RunStatus::Failed
        ) {
            self.submission = SubmissionStatus::Idle;
            self.in_flight = None;
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
                validate_text(workspace, "workspace", 256)?;
                Ok((workspace.clone(), json!({"workspace": workspace}), true))
            }
            WorkbenchIntent::Attach { session_id } => {
                validate_text(session_id, "session_id", 256)?;
                Ok((session_id.clone(), json!({"session_id": session_id}), true))
            }
            WorkbenchIntent::New { title } => {
                if let Some(title) = title {
                    validate_text(title, "title", 512)?;
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
        let entry = ActionAuditEntry {
            sequence: self.sequence,
            command: action.command,
            command_id: action.command_id,
            idempotency_key: action.idempotency_key.clone(),
            disposition,
        };
        self.audit.push(entry);
        if self.audit.len() > MAX_CONTROLLER_AUDIT_ENTRIES {
            let overflow = self.audit.len() - MAX_CONTROLLER_AUDIT_ENTRIES;
            self.audit.drain(0..overflow);
        }
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
    validate_text(prompt, "prompt", 64 * 1024)
}
