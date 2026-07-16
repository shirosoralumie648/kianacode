use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const EMBEDDED_PYTHON: &str = env!("KIANA_GOVERNANCE_PYTHON");
pub const EMBEDDED_PYTHON_UID: &str = env!("KIANA_GOVERNANCE_PYTHON_UID");
pub const EMBEDDED_PYTHON_GID: &str = env!("KIANA_GOVERNANCE_PYTHON_GID");
pub const SUPERVISOR_WORKER_EXIT_CODE: i32 = 80;
pub const MAX_STDOUT_BYTES: usize = 1 << 20;
pub const MAX_STDERR_BYTES: usize = 1 << 20;
pub const MAX_TRACE_BYTES: usize = 64 << 10;
pub const MAX_RECEIPT_BYTES: usize = 256;
pub const SLICE_DEADLINE: Duration = Duration::from_secs(30);
pub const TERM_GRACE: Duration = Duration::from_secs(2);
const RECEIPT_EARLY_CONFIRMATION: Duration = Duration::from_millis(10);
const PYTHON_PREFLIGHT_DEADLINE: Duration = Duration::from_secs(5);
pub const DEFAULT_RUNNER: &str = "scripts/capability-governance-smoke.sh";
const FINAL_MARKER: &str = "offline=true";
const SANDBOX_SUPERVISOR: &str = "/tmp/kiana-supervisor";
const SANDBOX_WORKER_TMP: &str = "/tmp/kiana-worker";
const TRACE_LOGGER_COMMAND: &str = "/tmp/kiana-supervisor __trace-logger";
const FIXED_PATH: &str = "/usr/bin:/bin";
const PYTHON_CANARY: &str = r#"
import jsonschema
schema = {"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "integer"}
validator = jsonschema.Draft202012Validator(schema)
validator.validate(1)
try:
    validator.validate("invalid")
except jsonschema.ValidationError:
    pass
else:
    raise SystemExit(1)
"#;

static INTERRUPTED_SIGNAL: AtomicI32 = AtomicI32::new(0);
static SIGNAL_HANDLER_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slice {
    Schemas,
    FixtureShapes,
}

impl Slice {
    pub fn parse_public(value: &str) -> Result<Self, UsageError> {
        match value {
            "schemas" => Ok(Self::Schemas),
            "fixture-shapes" => Ok(Self::FixtureShapes),
            _ => Err(UsageError),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Schemas => "schemas",
            Self::FixtureShapes => "fixture-shapes",
        }
    }

    pub const fn worker_arg(self) -> &'static str {
        self.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchToken([u8; 32]);

impl LaunchToken {
    pub fn generate() -> io::Result<Self> {
        let mut bytes = [0_u8; 32];
        let mut offset = 0;
        while offset < bytes.len() {
            let result = unsafe {
                libc::syscall(
                    libc::SYS_getrandom,
                    bytes[offset..].as_mut_ptr(),
                    bytes.len() - offset,
                    0,
                )
            };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if result == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "getrandom returned zero bytes",
                ));
            }
            offset += result as usize;
        }
        Ok(Self(bytes))
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn as_hex(&self) -> String {
        let mut result = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            let _ = write!(result, "{byte:02x}");
        }
        result
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeadlineState {
    Pending,
    Completed,
    Exceeded,
    Interrupted(i32),
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SandboxState {
    NotStarted,
    Started,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerExit {
    Code(i32),
    Signaled(i32),
    Missing,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceChannelState {
    ParentOwnedAndWorkerInaccessible,
    WorkerVisible,
    Tampered,
    Missing,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraceVerdict {
    ExactStartupCanaryOnly,
    NetworkAttempt { syscall: String },
    Invalid,
    Missing,
    Oversized,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptVerdict {
    ExactAndEof,
    Missing,
    Early,
    Duplicate,
    WrongToken,
    WrongSlice,
    TrailingBytes,
    Oversized,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureVerdict {
    CompleteBounded,
    LimitExceeded,
    Truncated,
    ReadFailed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessTreeState {
    FullyReaped,
    DescendantsRemain,
    GroupKillFailed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerTmpState {
    Empty,
    NonEmpty,
    Missing,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupState {
    Removed,
    Residue,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationState {
    Unverified,
    Pending,
    Attested,
    Complete,
    Failed,
    Unknown,
}

impl VerificationState {
    pub const fn is_verified(self) -> bool {
        matches!(self, Self::Complete)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FailureCode {
    SupervisorUnavailable,
    LauncherUntrusted,
    SandboxStartFailed,
    SandboxAttestationFailed,
    NetworkAttempt { syscall: String },
    DeadlineExceeded,
    WorkerFailed,
    ReceiptInvalid,
    OutputLimitExceeded,
    ProcessCleanupFailed,
    RuntimeCleanupFailed,
}

impl FailureCode {
    pub fn render_diagnostic(&self) -> String {
        match self {
            Self::SupervisorUnavailable => "supervisor_unavailable".to_owned(),
            Self::LauncherUntrusted => "launcher_untrusted".to_owned(),
            Self::SandboxStartFailed => "sandbox_start_failed".to_owned(),
            Self::SandboxAttestationFailed => "sandbox_attestation_failed".to_owned(),
            Self::NetworkAttempt { syscall } => {
                let safe = if is_safe_syscall_name(syscall) {
                    syscall.as_str()
                } else {
                    "unknown"
                };
                format!("network_attempt: blocked syscall={safe}")
            }
            Self::DeadlineExceeded => "deadline_exceeded".to_owned(),
            Self::WorkerFailed => "worker_failed".to_owned(),
            Self::ReceiptInvalid => "receipt_invalid".to_owned(),
            Self::OutputLimitExceeded => "output_limit_exceeded".to_owned(),
            Self::ProcessCleanupFailed => "process_cleanup_failed".to_owned(),
            Self::RuntimeCleanupFailed => "runtime_cleanup_failed".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsageError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisorError {
    pub code: FailureCode,
    pub context: &'static str,
}

impl SupervisorError {
    pub const fn new(code: FailureCode, context: &'static str) -> Self {
        Self { code, context }
    }

    pub fn diagnostic(&self) -> String {
        self.code.render_diagnostic()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verdict {
    pub launchers_trusted: bool,
    pub sandbox: SandboxState,
    pub deadline: DeadlineState,
    pub worker_exit: WorkerExit,
    pub trace_channel: TraceChannelState,
    pub trace: TraceVerdict,
    pub receipt: ReceiptVerdict,
    pub stdout: CaptureVerdict,
    pub stderr: CaptureVerdict,
    pub stderr_empty: bool,
    pub worker_stdout_has_final_marker: bool,
    pub process_tree: ProcessTreeState,
    pub worker_tmp: WorkerTmpState,
    pub runtime_cleanup: CleanupState,
}

impl Verdict {
    pub fn is_authorized_success(&self) -> bool {
        self.launchers_trusted
            && self.sandbox == SandboxState::Started
            && self.deadline == DeadlineState::Completed
            && self.worker_exit == WorkerExit::Code(SUPERVISOR_WORKER_EXIT_CODE)
            && self.trace_channel == TraceChannelState::ParentOwnedAndWorkerInaccessible
            && self.trace == TraceVerdict::ExactStartupCanaryOnly
            && self.receipt == ReceiptVerdict::ExactAndEof
            && self.stdout == CaptureVerdict::CompleteBounded
            && self.stderr == CaptureVerdict::CompleteBounded
            && self.stderr_empty
            && !self.worker_stdout_has_final_marker
            && self.process_tree == ProcessTreeState::FullyReaped
            && self.worker_tmp == WorkerTmpState::Empty
            && self.runtime_cleanup == CleanupState::Removed
    }

    pub fn failure_code(&self) -> Option<FailureCode> {
        if self.is_authorized_success() {
            return None;
        }
        if !self.launchers_trusted {
            return Some(FailureCode::LauncherUntrusted);
        }
        if self.sandbox != SandboxState::Started {
            return Some(FailureCode::SandboxStartFailed);
        }
        if self.deadline == DeadlineState::Exceeded {
            return Some(FailureCode::DeadlineExceeded);
        }
        if let TraceVerdict::NetworkAttempt { syscall } = &self.trace {
            return Some(FailureCode::NetworkAttempt {
                syscall: syscall.clone(),
            });
        }
        if self.trace_channel != TraceChannelState::ParentOwnedAndWorkerInaccessible
            || self.trace != TraceVerdict::ExactStartupCanaryOnly
        {
            return Some(FailureCode::SandboxAttestationFailed);
        }
        if self.stdout == CaptureVerdict::LimitExceeded
            || self.stderr == CaptureVerdict::LimitExceeded
        {
            return Some(FailureCode::OutputLimitExceeded);
        }
        if self.stdout != CaptureVerdict::CompleteBounded
            || self.stderr != CaptureVerdict::CompleteBounded
        {
            return Some(FailureCode::WorkerFailed);
        }
        if self.receipt != ReceiptVerdict::ExactAndEof {
            return Some(FailureCode::ReceiptInvalid);
        }
        if self.worker_exit != WorkerExit::Code(SUPERVISOR_WORKER_EXIT_CODE)
            || !self.stderr_empty
            || self.worker_stdout_has_final_marker
        {
            return Some(FailureCode::WorkerFailed);
        }
        if self.process_tree != ProcessTreeState::FullyReaped {
            return Some(FailureCode::ProcessCleanupFailed);
        }
        if self.worker_tmp != WorkerTmpState::Empty {
            return Some(FailureCode::RuntimeCleanupFailed);
        }
        if self.runtime_cleanup != CleanupState::Removed {
            return Some(FailureCode::RuntimeCleanupFailed);
        }
        Some(FailureCode::WorkerFailed)
    }
}

pub fn parse_receipt(
    bytes: &[u8],
    token: &LaunchToken,
    slice: Slice,
    observed_before_worker_exit: bool,
) -> ReceiptVerdict {
    if bytes.len() > MAX_RECEIPT_BYTES {
        return ReceiptVerdict::Oversized;
    }
    if observed_before_worker_exit {
        return ReceiptVerdict::Early;
    }
    if bytes.is_empty() {
        return ReceiptVerdict::Missing;
    }
    let expected_prefix = format!("complete:{}:", token.as_hex());
    if !bytes.ends_with(b"\n") {
        return ReceiptVerdict::TrailingBytes;
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return ReceiptVerdict::TrailingBytes,
    };
    if text.matches("complete:").count() > 1 {
        return ReceiptVerdict::Duplicate;
    }
    let expected = format!("{}{slice}\n", expected_prefix, slice = slice.as_str());
    if text == expected {
        return ReceiptVerdict::ExactAndEof;
    }
    if !text.starts_with("complete:") {
        return ReceiptVerdict::Missing;
    }
    if !text.starts_with(&expected_prefix) {
        return ReceiptVerdict::WrongToken;
    }
    let rest = &text[expected_prefix.len()..text.len() - 1];
    if rest != slice.as_str() {
        return ReceiptVerdict::WrongSlice;
    }
    ReceiptVerdict::TrailingBytes
}

pub fn parse_trace(bytes: &[u8]) -> TraceVerdict {
    if bytes.is_empty() {
        return TraceVerdict::Missing;
    }
    if bytes.len() > MAX_TRACE_BYTES {
        return TraceVerdict::Oversized;
    }
    if !bytes.is_ascii() {
        return TraceVerdict::Invalid;
    }
    let mut records = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let text = match std::str::from_utf8(line) {
            Ok(text) => text,
            Err(_) => return TraceVerdict::Invalid,
        };
        let name = match syscall_name(text) {
            Some(name) => name,
            None => return TraceVerdict::Invalid,
        };
        records.push((name, text));
    }
    if records.is_empty() {
        return TraceVerdict::Missing;
    }
    if records.len() > 1 {
        if let Some((name, _)) = records
            .iter()
            .skip(1)
            .find(|(name, _)| is_network_syscall(name))
        {
            return TraceVerdict::NetworkAttempt {
                syscall: (*name).to_owned(),
            };
        }
        return TraceVerdict::Invalid;
    }
    let (name, line) = records[0];
    if name == "socket" && line.contains("= -1 EPERM") && line.contains("(INJECTED)") {
        TraceVerdict::ExactStartupCanaryOnly
    } else if is_network_syscall(name) {
        TraceVerdict::NetworkAttempt {
            syscall: name.to_owned(),
        }
    } else {
        TraceVerdict::Invalid
    }
}

fn syscall_name(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let line = if let Some(rest) = line.strip_prefix("[pid ") {
        let end = rest.find(']')?;
        rest.get(end + 1..)?.trim_start()
    } else {
        let digits = line
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        if digits > 0 {
            line.get(digits..)?.trim_start()
        } else {
            line
        }
    };
    let end = line.find('(')?;
    let name = &line[..end];
    if name.is_empty()
        || !name.chars().enumerate().all(|(index, character)| {
            (index == 0 && (character == '_' || character.is_ascii_alphabetic()))
                || (index > 0 && (character == '_' || character.is_ascii_alphanumeric()))
        })
    {
        return None;
    }
    Some(name)
}

fn is_network_syscall(name: &str) -> bool {
    matches!(
        name,
        "socket"
            | "socketpair"
            | "connect"
            | "bind"
            | "listen"
            | "accept"
            | "accept4"
            | "sendto"
            | "sendmsg"
            | "sendmmsg"
            | "recvfrom"
            | "recvmsg"
            | "recvmmsg"
            | "getsockname"
            | "getpeername"
            | "setsockopt"
            | "getsockopt"
            | "shutdown"
            | "io_uring_setup"
            | "io_uring_enter"
            | "io_uring_register"
    )
}

fn is_safe_syscall_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub enum Invocation {
    Public(Slice),
    TraceLogger,
    WorkerLauncher { runner: PathBuf, slice: Slice },
}

pub fn parse_invocation(args: &[OsString]) -> Result<Invocation, UsageError> {
    if args.len() == 2 {
        if let Some(value) = args[1].to_str() {
            if value == "__trace-logger" {
                return Ok(Invocation::TraceLogger);
            }
            if let Ok(slice) = Slice::parse_public(value) {
                return Ok(Invocation::Public(slice));
            }
        }
        return Err(UsageError);
    }
    if args.len() == 6
        && args[1] == OsStr::new("__worker-launcher")
        && args[2] == OsStr::new("--runner")
        && args[4] == OsStr::new("--slice")
    {
        let runner = PathBuf::from(&args[3]);
        let value = args[5].to_str().ok_or(UsageError)?;
        return Ok(Invocation::WorkerLauncher {
            runner,
            slice: Slice::parse_public(value)?,
        });
    }
    Err(UsageError)
}

#[derive(Clone, Debug)]
pub struct SupervisorConfig {
    pub repo_root: PathBuf,
    pub runner: PathBuf,
    pub supervisor_bin: PathBuf,
    pub deadline: Duration,
    pub term_grace: Duration,
    pub limits: CaptureLimits,
}

impl SupervisorConfig {
    pub fn new(repo_root: PathBuf, runner: PathBuf, supervisor_bin: PathBuf) -> Self {
        Self {
            repo_root,
            runner,
            supervisor_bin,
            deadline: SLICE_DEADLINE,
            term_grace: TERM_GRACE,
            limits: CaptureLimits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LauncherKind {
    Bwrap,
    Strace,
    Bash,
    Shell,
    Python,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedLauncherPaths {
    pub bwrap: PathBuf,
    pub strace: PathBuf,
    pub bash: PathBuf,
    pub shell: PathBuf,
}

impl Default for FixedLauncherPaths {
    fn default() -> Self {
        Self {
            bwrap: PathBuf::from("/usr/bin/bwrap"),
            strace: PathBuf::from("/usr/bin/strace"),
            bash: PathBuf::from("/usr/bin/bash"),
            shell: PathBuf::from("/bin/sh"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedExecutable {
    pub path: PathBuf,
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedLaunchers {
    pub bwrap: TrustedExecutable,
    pub strace: TrustedExecutable,
    pub bash: TrustedExecutable,
    pub shell: TrustedExecutable,
    pub python: TrustedExecutable,
}

fn validate_executable(
    path: &Path,
    require_root: bool,
) -> Result<TrustedExecutable, SupervisorError> {
    let canonical = fs::canonicalize(path).map_err(|_| {
        SupervisorError::new(FailureCode::LauncherUntrusted, "canonicalize launcher")
    })?;
    let metadata = fs::metadata(&canonical)
        .map_err(|_| SupervisorError::new(FailureCode::LauncherUntrusted, "stat launcher"))?;
    let mode = metadata.mode();
    let uid = metadata.uid() as u32;
    let gid = metadata.gid() as u32;
    let current_uid = unsafe { libc::getuid() } as u32;
    let current_gid = unsafe { libc::getgid() } as u32;
    if !metadata.is_file()
        || mode & 0o100 == 0
        || mode & 0o002 != 0
        || (require_root && (uid != 0 || gid != 0 || mode & 0o020 != 0))
        || (!require_root && mode & 0o020 != 0 && gid != current_gid)
        || (!require_root && uid != 0 && uid != current_uid)
    {
        return Err(SupervisorError::new(
            FailureCode::LauncherUntrusted,
            "launcher metadata",
        ));
    }
    Ok(TrustedExecutable {
        path: canonical,
        uid,
        gid,
        mode,
    })
}

pub fn validate_fixed_launchers(
    paths: &FixedLauncherPaths,
) -> Result<TrustedLaunchers, SupervisorError> {
    for (path, expected) in [
        (&paths.bwrap, "/usr/bin/bwrap"),
        (&paths.strace, "/usr/bin/strace"),
        (&paths.bash, "/usr/bin/bash"),
    ] {
        let canonical_matches = fs::canonicalize(path)
            .map(|canonical| canonical == Path::new(expected))
            .unwrap_or(false);
        if path != Path::new(expected)
            || fs::symlink_metadata(path)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(true)
            || !canonical_matches
        {
            return Err(SupervisorError::new(
                FailureCode::LauncherUntrusted,
                "fixed launcher path",
            ));
        }
    }
    if paths.shell != Path::new("/bin/sh") {
        return Err(SupervisorError::new(
            FailureCode::LauncherUntrusted,
            "fixed shell path",
        ));
    }
    let bwrap = validate_executable(&paths.bwrap, true)?;
    let strace = validate_executable(&paths.strace, true)?;
    let bash = validate_executable(&paths.bash, true)?;
    if bwrap.path != paths.bwrap || strace.path != paths.strace || bash.path != paths.bash {
        return Err(SupervisorError::new(
            FailureCode::LauncherUntrusted,
            "canonical launcher path",
        ));
    }
    let shell = validate_executable(&paths.shell, true)?;
    let python = embedded_python_path()?;
    Ok(TrustedLaunchers {
        bwrap,
        strace,
        bash,
        shell,
        python,
    })
}

pub fn embedded_python_path() -> Result<TrustedExecutable, SupervisorError> {
    let executable = validate_executable(Path::new(EMBEDDED_PYTHON), false)?;
    let build_uid = EMBEDDED_PYTHON_UID
        .parse::<u32>()
        .map_err(|_| SupervisorError::new(FailureCode::LauncherUntrusted, "python build uid"))?;
    let build_gid = EMBEDDED_PYTHON_GID
        .parse::<u32>()
        .map_err(|_| SupervisorError::new(FailureCode::LauncherUntrusted, "python build gid"))?;
    if executable.uid != build_uid || executable.gid != build_gid {
        return Err(SupervisorError::new(
            FailureCode::LauncherUntrusted,
            "python build identity",
        ));
    }
    let mut child = Command::new(&executable.path)
        .arg("-I")
        .arg("-c")
        .arg(PYTHON_CANARY)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| SupervisorError::new(FailureCode::LauncherUntrusted, "python preflight"))?;
    let process_group = child.id() as libc::pid_t;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None)
                if started.elapsed() < PYTHON_PREFLIGHT_DEADLINE
                    && current_interrupt().is_none() =>
            {
                thread::sleep(Duration::from_millis(5));
            }
            Ok(None) | Err(_) => {
                unsafe {
                    libc::kill(-process_group, libc::SIGKILL);
                }
                break child.wait().ok();
            }
        }
    };
    if !status.is_some_and(|status| status.success())
        || started.elapsed() >= PYTHON_PREFLIGHT_DEADLINE
        || current_interrupt().is_some()
    {
        return Err(SupervisorError::new(
            FailureCode::LauncherUntrusted,
            "python preflight",
        ));
    }
    Ok(executable)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureLimits {
    pub stdout: usize,
    pub stderr: usize,
    pub trace: usize,
    pub receipt: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            stdout: MAX_STDOUT_BYTES,
            stderr: MAX_STDERR_BYTES,
            trace: MAX_TRACE_BYTES,
            receipt: MAX_RECEIPT_BYTES,
        }
    }
}

#[derive(Debug)]
pub struct RuntimeRoot {
    path: PathBuf,
    worker_tmp: PathBuf,
    path_identity: RuntimeDirectoryIdentity,
    worker_tmp_identity: RuntimeDirectoryIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeDirectoryIdentity {
    device: u64,
    inode: u64,
    uid: u32,
    gid: u32,
}

impl RuntimeRoot {
    pub fn create() -> Result<Self, SupervisorError> {
        for _ in 0..8 {
            let token = LaunchToken::generate().map_err(|_| {
                SupervisorError::new(FailureCode::RuntimeCleanupFailed, "runtime token")
            })?;
            let path = PathBuf::from("/tmp")
                .join(format!("kiana-capability-governance-{}", token.as_hex()));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    if !runtime_directory_is_trusted(&path) {
                        let _ = fs::remove_dir_all(&path);
                        return Err(SupervisorError::new(
                            FailureCode::RuntimeCleanupFailed,
                            "runtime ownership",
                        ));
                    }
                    let worker_tmp = path.join("worker");
                    let mut worker_builder = fs::DirBuilder::new();
                    worker_builder.mode(0o700);
                    if worker_builder.create(&worker_tmp).is_err()
                        || !runtime_directory_is_trusted(&worker_tmp)
                    {
                        let _ = fs::remove_dir_all(&path);
                        return Err(SupervisorError::new(
                            FailureCode::RuntimeCleanupFailed,
                            "worker tmp",
                        ));
                    }
                    let Some(path_identity) = runtime_directory_identity(&path) else {
                        let _ = fs::remove_dir_all(&path);
                        return Err(SupervisorError::new(
                            FailureCode::RuntimeCleanupFailed,
                            "runtime identity",
                        ));
                    };
                    let Some(worker_tmp_identity) = runtime_directory_identity(&worker_tmp) else {
                        let _ = fs::remove_dir_all(&path);
                        return Err(SupervisorError::new(
                            FailureCode::RuntimeCleanupFailed,
                            "worker tmp identity",
                        ));
                    };
                    return Ok(Self {
                        path,
                        worker_tmp,
                        path_identity,
                        worker_tmp_identity,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        Err(SupervisorError::new(
            FailureCode::RuntimeCleanupFailed,
            "runtime root",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn worker_tmp(&self) -> &Path {
        &self.worker_tmp
    }

    pub fn inspect_worker_tmp(&self) -> WorkerTmpState {
        if !runtime_directory_matches(&self.path, self.path_identity)
            || !runtime_directory_matches(&self.worker_tmp, self.worker_tmp_identity)
        {
            return WorkerTmpState::Unknown;
        }
        let entries = match fs::read_dir(&self.worker_tmp) {
            Ok(entries) => entries,
            Err(_) => return WorkerTmpState::Missing,
        };
        if entries.into_iter().next().is_some() {
            WorkerTmpState::NonEmpty
        } else {
            WorkerTmpState::Empty
        }
    }

    pub fn cleanup(self) -> CleanupState {
        if runtime_directory_identity(&self.path) != Some(self.path_identity)
            || runtime_directory_identity(&self.worker_tmp) != Some(self.worker_tmp_identity)
        {
            return CleanupState::Failed;
        }
        for path in [&self.path, &self.worker_tmp] {
            if fs::set_permissions(path, fs::Permissions::from_mode(0o700)).is_err() {
                return CleanupState::Failed;
            }
        }
        if !runtime_directory_matches(&self.path, self.path_identity)
            || !runtime_directory_matches(&self.worker_tmp, self.worker_tmp_identity)
        {
            return CleanupState::Failed;
        }
        match fs::remove_dir_all(&self.path) {
            Ok(())
                if matches!(
                    fs::symlink_metadata(&self.path),
                    Err(error) if error.kind() == io::ErrorKind::NotFound
                ) =>
            {
                CleanupState::Removed
            }
            Ok(()) => CleanupState::Residue,
            Err(_) => CleanupState::Failed,
        }
    }
}

fn runtime_directory_is_trusted(path: &Path) -> bool {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    metadata.file_type().is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.mode() & 0o7777 == 0o700
        && metadata.uid() as u32 == unsafe { libc::getuid() } as u32
        && metadata.gid() as u32 == unsafe { libc::getgid() } as u32
}

fn runtime_directory_identity(path: &Path) -> Option<RuntimeDirectoryIdentity> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    Some(RuntimeDirectoryIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        uid: metadata.uid() as u32,
        gid: metadata.gid() as u32,
    })
}

fn runtime_directory_matches(path: &Path, expected: RuntimeDirectoryIdentity) -> bool {
    runtime_directory_is_trusted(path) && runtime_directory_identity(path) == Some(expected)
}

#[derive(Debug)]
pub struct ParentChannels {
    pub launch_write: OwnedFd,
    pub completion_read: OwnedFd,
    pub worker_stdout_read: OwnedFd,
    pub worker_stderr_read: OwnedFd,
    pub guard_stderr_read: OwnedFd,
    pub trace_read: OwnedFd,
}

#[derive(Debug)]
pub struct ChildChannels {
    pub launch_read: OwnedFd,
    pub completion_write: OwnedFd,
    pub worker_stdout_write: OwnedFd,
    pub worker_stderr_write: OwnedFd,
    pub guard_stderr_write: OwnedFd,
    pub trace_write: OwnedFd,
}

#[derive(Debug)]
pub struct ChannelSet {
    pub parent: ParentChannels,
    pub child: ChildChannels,
}

fn pipe_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut descriptors = [0; 2];
    let result = unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let read = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
    let write = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
    Ok((read, write))
}

impl ChannelSet {
    pub fn create() -> Result<Self, SupervisorError> {
        let (launch_read, launch_write) = pipe_pair()
            .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "launch pipe"))?;
        let (completion_read, completion_write) = pipe_pair().map_err(|_| {
            SupervisorError::new(FailureCode::SandboxStartFailed, "completion pipe")
        })?;
        let (worker_stdout_read, worker_stdout_write) = pipe_pair()
            .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "stdout pipe"))?;
        let (worker_stderr_read, worker_stderr_write) = pipe_pair()
            .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "stderr pipe"))?;
        let (guard_stderr_read, guard_stderr_write) = pipe_pair()
            .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "guard pipe"))?;
        let (trace_read, trace_write) = pipe_pair()
            .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "trace pipe"))?;
        Ok(Self {
            parent: ParentChannels {
                launch_write,
                completion_read,
                worker_stdout_read,
                worker_stderr_read,
                guard_stderr_read,
                trace_read,
            },
            child: ChildChannels {
                launch_read,
                completion_write,
                worker_stdout_write,
                worker_stderr_write,
                guard_stderr_write,
                trace_write,
            },
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct BoundedCapture {
    pub bytes: Vec<u8>,
    pub verdict: CaptureVerdict,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ReceiptCapture {
    pub capture: BoundedCapture,
    pub observed_before_worker_exit: bool,
}

#[derive(Debug, Default)]
struct ReceiptTiming {
    child_exited: AtomicBool,
    first_byte_seen_at: Mutex<Option<Instant>>,
    observed_before_worker_exit: AtomicBool,
}

impl ReceiptTiming {
    fn record_first_byte(&self) {
        if self.child_exited.load(Ordering::SeqCst) {
            return;
        }
        match self.first_byte_seen_at.lock() {
            Ok(mut observed_at) => {
                observed_at.get_or_insert_with(Instant::now);
            }
            Err(_) => self
                .observed_before_worker_exit
                .store(true, Ordering::SeqCst),
        }
    }

    fn confirm_early_while_child_is_running(&self) {
        let confirmed = match self.first_byte_seen_at.lock() {
            Ok(observed_at) => observed_at
                .as_ref()
                .is_some_and(|instant| instant.elapsed() >= RECEIPT_EARLY_CONFIRMATION),
            Err(_) => true,
        };
        if confirmed {
            self.observed_before_worker_exit
                .store(true, Ordering::SeqCst);
        }
    }
}

pub fn read_bounded<R: Read + Send + 'static>(
    reader: R,
    limit: usize,
) -> JoinHandle<BoundedCapture> {
    read_bounded_monitored(reader, limit, Arc::new(AtomicBool::new(false)))
}

fn read_bounded_monitored<R: Read + Send + 'static>(
    reader: R,
    limit: usize,
    capture_failed: Arc<AtomicBool>,
) -> JoinHandle<BoundedCapture> {
    thread::spawn(move || {
        let mut reader = reader;
        let mut bytes = Vec::with_capacity(limit.min(4096));
        let mut buffer = [0_u8; 4096];
        loop {
            let remaining = limit.saturating_sub(bytes.len());
            if remaining == 0 {
                let mut sentinel = [0_u8; 1];
                return match reader.read(&mut sentinel) {
                    Ok(0) => BoundedCapture {
                        bytes,
                        verdict: CaptureVerdict::CompleteBounded,
                    },
                    Ok(_) => {
                        capture_failed.store(true, Ordering::SeqCst);
                        BoundedCapture {
                            bytes,
                            verdict: CaptureVerdict::LimitExceeded,
                        }
                    }
                    Err(_) => {
                        capture_failed.store(true, Ordering::SeqCst);
                        BoundedCapture {
                            bytes,
                            verdict: CaptureVerdict::ReadFailed,
                        }
                    }
                };
            }
            let size = remaining.min(buffer.len());
            match reader.read(&mut buffer[..size]) {
                Ok(0) => {
                    return BoundedCapture {
                        bytes,
                        verdict: CaptureVerdict::CompleteBounded,
                    }
                }
                Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    capture_failed.store(true, Ordering::SeqCst);
                    return BoundedCapture {
                        bytes,
                        verdict: CaptureVerdict::ReadFailed,
                    };
                }
            }
        }
    })
}

fn read_receipt_bounded<R: Read + Send + 'static>(
    reader: R,
    limit: usize,
    timing: Arc<ReceiptTiming>,
    capture_failed: Arc<AtomicBool>,
) -> JoinHandle<ReceiptCapture> {
    thread::spawn(move || {
        let mut reader = reader;
        let mut bytes = Vec::with_capacity(limit.min(256));
        let mut first_byte_seen = false;
        let mut buffer = [0_u8; 256];
        loop {
            let remaining = limit.saturating_sub(bytes.len());
            if remaining == 0 {
                let mut sentinel = [0_u8; 1];
                let verdict = match reader.read(&mut sentinel) {
                    Ok(0) => CaptureVerdict::CompleteBounded,
                    Ok(_) => {
                        capture_failed.store(true, Ordering::SeqCst);
                        CaptureVerdict::LimitExceeded
                    }
                    Err(_) => {
                        capture_failed.store(true, Ordering::SeqCst);
                        CaptureVerdict::ReadFailed
                    }
                };
                return ReceiptCapture {
                    capture: BoundedCapture { bytes, verdict },
                    observed_before_worker_exit: timing
                        .observed_before_worker_exit
                        .load(Ordering::SeqCst),
                };
            }
            let size = remaining.min(buffer.len());
            match reader.read(&mut buffer[..size]) {
                Ok(0) => {
                    return ReceiptCapture {
                        capture: BoundedCapture {
                            bytes,
                            verdict: CaptureVerdict::CompleteBounded,
                        },
                        observed_before_worker_exit: timing
                            .observed_before_worker_exit
                            .load(Ordering::SeqCst),
                    };
                }
                Ok(count) => {
                    if !first_byte_seen {
                        first_byte_seen = true;
                        timing.record_first_byte();
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    capture_failed.store(true, Ordering::SeqCst);
                    return ReceiptCapture {
                        capture: BoundedCapture {
                            bytes,
                            verdict: CaptureVerdict::ReadFailed,
                        },
                        observed_before_worker_exit: timing
                            .observed_before_worker_exit
                            .load(Ordering::SeqCst),
                    };
                }
            }
        }
    })
}

pub fn close_unexpected_fds(keep: &[RawFd]) -> io::Result<()> {
    let mut keep = keep
        .iter()
        .copied()
        .filter(|fd| *fd >= 3)
        .collect::<Vec<_>>();
    keep.sort_unstable();
    keep.dedup();

    let mut first = 3_u32;
    let mut close_range_supported = true;
    for fd in keep.iter().copied() {
        let fd = fd as u32;
        if first < fd && close_fd_range(first, fd - 1).is_err() {
            close_range_supported = false;
            break;
        }
        first = fd.saturating_add(1);
    }
    if close_range_supported && close_fd_range(first, u32::MAX).is_ok() {
        return Ok(());
    }

    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let upper = limit.rlim_cur.min(RawFd::MAX as libc::rlim_t).max(3) as RawFd;
    for fd in 3..upper {
        if !keep.contains(&fd) {
            unsafe {
                libc::close(fd);
            }
        }
    }
    Ok(())
}

fn close_fd_range(first: u32, last: u32) -> io::Result<()> {
    if first > last {
        return Ok(());
    }
    let result = unsafe { libc::syscall(libc::SYS_close_range, first, last, 0_u32) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Debug)]
pub struct SandboxLaunchSpec {
    pub bwrap_argv: Vec<OsString>,
    pub strace_argv: Vec<OsString>,
    pub worker_argv: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub runtime_root: PathBuf,
    pub worker_tmp: PathBuf,
}

pub trait RunningProcess {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
    fn terminate_group(&self, signal: i32) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<ExitStatus>;
    fn process_group(&self) -> libc::pid_t;
}

pub trait ProcessLauncher {
    fn spawn(
        &self,
        spec: &SandboxLaunchSpec,
        channels: ChildChannels,
    ) -> Result<Box<dyn RunningProcess>, SupervisorError>;
}

pub struct LinuxProcessLauncher;

pub fn build_sandbox_launch_spec(
    config: &SupervisorConfig,
    runtime: &RuntimeRoot,
    channels: &ChildChannels,
    _token: &LaunchToken,
    slice: Slice,
) -> Result<SandboxLaunchSpec, SupervisorError> {
    let repo_root = fs::canonicalize(&config.repo_root)
        .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "repo root"))?;
    let runner = fs::canonicalize(&config.runner)
        .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "runner"))?;
    let supervisor_bin = fs::canonicalize(&config.supervisor_bin)
        .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "supervisor binary"))?;
    if !repo_root.is_dir()
        || !runner.is_file()
        || !runner.starts_with(&repo_root)
        || !supervisor_bin.is_file()
        || !runtime_directory_is_trusted(runtime.path())
        || !runtime_directory_is_trusted(runtime.worker_tmp())
    {
        return Err(SupervisorError::new(
            FailureCode::SandboxStartFailed,
            "sandbox inputs",
        ));
    }

    let launch_fd = channels.launch_read.as_raw_fd().to_string();
    let completion_fd = channels.completion_write.as_raw_fd().to_string();
    let worker_stdout_fd = channels.worker_stdout_write.as_raw_fd().to_string();
    let worker_stderr_fd = channels.worker_stderr_write.as_raw_fd().to_string();
    let env = vec![
        (OsString::from("PATH"), OsString::from(FIXED_PATH)),
        (OsString::from("LC_ALL"), OsString::from("C")),
        (OsString::from("HOME"), OsString::from("/tmp/kiana-home")),
        (OsString::from("TMPDIR"), OsString::from(SANDBOX_WORKER_TMP)),
        (OsString::from("USER"), OsString::from("kiana")),
        (OsString::from("LOGNAME"), OsString::from("kiana")),
        (OsString::from("SHELL"), OsString::from("/usr/bin/bash")),
        (
            OsString::from("KIANA_GOVERNANCE_PYTHON"),
            OsString::from(EMBEDDED_PYTHON),
        ),
        (
            OsString::from("KIANA_GOVERNANCE_LAUNCH_FD"),
            OsString::from(&launch_fd),
        ),
        (
            OsString::from("KIANA_GOVERNANCE_COMPLETION_FD"),
            OsString::from(&completion_fd),
        ),
        (
            OsString::from("KIANA_GOVERNANCE_WORKER_STDOUT_FD"),
            OsString::from(&worker_stdout_fd),
        ),
        (
            OsString::from("KIANA_GOVERNANCE_WORKER_STDERR_FD"),
            OsString::from(&worker_stderr_fd),
        ),
    ];

    let worker_argv = vec![
        OsString::from(SANDBOX_SUPERVISOR),
        OsString::from("__worker-launcher"),
        OsString::from("--runner"),
        runner.as_os_str().to_owned(),
        OsString::from("--slice"),
        OsString::from(slice.as_str()),
    ];
    let mut strace_argv = vec![
        OsString::from("-f"),
        OsString::from("-qq"),
        OsString::from("-e"),
        OsString::from("trace=%network,io_uring_setup,io_uring_enter,io_uring_register"),
        OsString::from("-e"),
        OsString::from("signal=none"),
        OsString::from("-e"),
        OsString::from("inject=%network:error=EPERM"),
        OsString::from("-e"),
        OsString::from("inject=io_uring_setup:error=EPERM"),
        OsString::from("-e"),
        OsString::from("inject=io_uring_enter:error=EPERM"),
        OsString::from("-e"),
        OsString::from("inject=io_uring_register:error=EPERM"),
        OsString::from("-o"),
        OsString::from(format!("|{TRACE_LOGGER_COMMAND}")),
        OsString::from("--"),
    ];
    strace_argv.extend(worker_argv.iter().cloned());

    let mut bwrap_argv = vec![
        OsString::from("--die-with-parent"),
        OsString::from("--unshare-all"),
        OsString::from("--clearenv"),
        OsString::from("--ro-bind"),
        OsString::from("/"),
        OsString::from("/"),
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--tmpfs"),
        OsString::from("/proc"),
        OsString::from("--remount-ro"),
        OsString::from("/proc"),
        OsString::from("--tmpfs"),
        OsString::from("/tmp"),
        OsString::from("--dir"),
        OsString::from("/tmp/kiana-home"),
        OsString::from("--ro-bind"),
        supervisor_bin.as_os_str().to_owned(),
        OsString::from(SANDBOX_SUPERVISOR),
        OsString::from("--bind"),
        runtime.worker_tmp().as_os_str().to_owned(),
        OsString::from(SANDBOX_WORKER_TMP),
        OsString::from("--chdir"),
        repo_root.as_os_str().to_owned(),
    ];
    for (name, value) in &env {
        bwrap_argv.push(OsString::from("--setenv"));
        bwrap_argv.push(name.clone());
        bwrap_argv.push(value.clone());
    }
    bwrap_argv.push(OsString::from("--"));
    bwrap_argv.push(OsString::from("/usr/bin/strace"));
    bwrap_argv.extend(strace_argv.iter().cloned());

    Ok(SandboxLaunchSpec {
        bwrap_argv,
        strace_argv,
        worker_argv,
        env,
        runtime_root: runtime.path().to_path_buf(),
        worker_tmp: runtime.worker_tmp().to_path_buf(),
    })
}

struct LinuxRunningProcess {
    child: Child,
    process_group: libc::pid_t,
}

impl RunningProcess for LinuxRunningProcess {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    fn terminate_group(&self, signal: i32) -> io::Result<()> {
        let result = unsafe { libc::kill(-self.process_group, signal) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    fn process_group(&self) -> libc::pid_t {
        self.process_group
    }
}

impl ProcessLauncher for LinuxProcessLauncher {
    fn spawn(
        &self,
        spec: &SandboxLaunchSpec,
        channels: ChildChannels,
    ) -> Result<Box<dyn RunningProcess>, SupervisorError> {
        let ChildChannels {
            launch_read,
            completion_write,
            worker_stdout_write,
            worker_stderr_write,
            guard_stderr_write,
            trace_write,
        } = channels;
        let inherited = [
            launch_read.as_raw_fd(),
            completion_write.as_raw_fd(),
            worker_stdout_write.as_raw_fd(),
            worker_stderr_write.as_raw_fd(),
        ];

        let mut command = Command::new("/usr/bin/bwrap");
        command
            .args(&spec.bwrap_argv)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::from(File::from(trace_write)))
            .stderr(Stdio::from(File::from(guard_stderr_write)))
            .process_group(0);
        unsafe {
            command.pre_exec(move || {
                for fd in inherited {
                    clear_cloexec(fd)?;
                }
                close_unexpected_fds_after_fork(inherited)
            });
        }
        let child = command
            .spawn()
            .map_err(|_| SupervisorError::new(FailureCode::SandboxStartFailed, "spawn bwrap"))?;
        let process_group = child.id() as libc::pid_t;
        drop(launch_read);
        drop(completion_write);
        drop(worker_stdout_write);
        drop(worker_stderr_write);
        Ok(Box::new(LinuxRunningProcess {
            child,
            process_group,
        }))
    }
}

fn clear_cloexec(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn close_unexpected_fds_after_fork(mut keep: [RawFd; 4]) -> io::Result<()> {
    for index in 1..keep.len() {
        let mut cursor = index;
        while cursor > 0 && keep[cursor - 1] > keep[cursor] {
            keep.swap(cursor - 1, cursor);
            cursor -= 1;
        }
    }
    let mut first = 3_u32;
    let mut close_range_supported = true;
    for fd in keep {
        let fd = fd as u32;
        if first < fd && close_fd_range(first, fd - 1).is_err() {
            close_range_supported = false;
            break;
        }
        first = fd.saturating_add(1);
    }
    if close_range_supported && close_fd_range(first, u32::MAX).is_ok() {
        return Ok(());
    }

    let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let upper = limit.rlim_cur.min(RawFd::MAX as libc::rlim_t).max(3) as RawFd;
    for fd in 3..upper {
        if !keep.contains(&fd) {
            unsafe {
                libc::close(fd);
            }
        }
    }
    Ok(())
}

pub fn trace_logger_main() -> Result<(), SupervisorError> {
    close_unexpected_fds(&[]).map_err(|_| {
        SupervisorError::new(FailureCode::SandboxAttestationFailed, "trace logger fds")
    })?;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    io::copy(&mut input, &mut output).map_err(|_| {
        SupervisorError::new(FailureCode::SandboxAttestationFailed, "trace logger copy")
    })?;
    output.flush().map_err(|_| {
        SupervisorError::new(FailureCode::SandboxAttestationFailed, "trace logger flush")
    })?;
    Ok(())
}

fn parse_inherited_fd(name: &'static str) -> Result<RawFd, SupervisorError> {
    let value = std::env::var(name)
        .map_err(|_| SupervisorError::new(FailureCode::SandboxAttestationFailed, "worker fd"))?;
    let fd = value
        .parse::<RawFd>()
        .map_err(|_| SupervisorError::new(FailureCode::SandboxAttestationFailed, "worker fd"))?;
    if fd < 3 {
        return Err(SupervisorError::new(
            FailureCode::SandboxAttestationFailed,
            "worker fd",
        ));
    }
    Ok(fd)
}

#[repr(C)]
struct CapabilityHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CapabilityData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn clear_process_capabilities() -> io::Result<()> {
    let mut header = CapabilityHeader {
        version: 0x2008_0522,
        pid: 0,
    };
    let mut data = [CapabilityData {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    let result = unsafe {
        libc::syscall(
            libc::SYS_capset,
            &mut header as *mut CapabilityHeader,
            data.as_mut_ptr(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn worker_launcher_main(runner: &Path, slice: Slice) -> Result<(), SupervisorError> {
    let canonical_runner = fs::canonicalize(runner)
        .map_err(|_| SupervisorError::new(FailureCode::SandboxAttestationFailed, "runner"))?;
    if !runner.is_absolute()
        || canonical_runner != runner
        || !canonical_runner.is_file()
        || canonical_runner.file_name() != Some(OsStr::new("capability-governance-smoke.sh"))
    {
        return Err(SupervisorError::new(
            FailureCode::SandboxAttestationFailed,
            "runner",
        ));
    }
    if std::env::var_os("KIANA_GOVERNANCE_PYTHON").as_deref() != Some(OsStr::new(EMBEDDED_PYTHON)) {
        return Err(SupervisorError::new(
            FailureCode::SandboxAttestationFailed,
            "python binding",
        ));
    }
    let launch_fd = parse_inherited_fd("KIANA_GOVERNANCE_LAUNCH_FD")?;
    let completion_fd = parse_inherited_fd("KIANA_GOVERNANCE_COMPLETION_FD")?;
    let stdout_fd = parse_inherited_fd("KIANA_GOVERNANCE_WORKER_STDOUT_FD")?;
    let stderr_fd = parse_inherited_fd("KIANA_GOVERNANCE_WORKER_STDERR_FD")?;
    let descriptors = [launch_fd, completion_fd, stdout_fd, stderr_fd];
    for (index, fd) in descriptors.iter().enumerate() {
        if descriptors[..index].contains(fd) {
            return Err(SupervisorError::new(
                FailureCode::SandboxAttestationFailed,
                "duplicate worker fd",
            ));
        }
    }

    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(SupervisorError::new(
            FailureCode::SandboxAttestationFailed,
            "no new privileges",
        ));
    }
    clear_process_capabilities().map_err(|_| {
        SupervisorError::new(FailureCode::SandboxAttestationFailed, "capability clear")
    })?;
    let dev_null = open_dev_null()
        .map_err(|_| SupervisorError::new(FailureCode::SandboxAttestationFailed, "worker stdin"))?;
    for (source, target) in [(dev_null.as_raw_fd(), 0), (stdout_fd, 1), (stderr_fd, 2)] {
        if unsafe { libc::dup2(source, target) } < 0 {
            return Err(SupervisorError::new(
                FailureCode::SandboxAttestationFailed,
                "worker stdio",
            ));
        }
    }
    clear_cloexec(launch_fd)
        .map_err(|_| SupervisorError::new(FailureCode::SandboxAttestationFailed, "launch fd"))?;
    clear_cloexec(completion_fd).map_err(|_| {
        SupervisorError::new(FailureCode::SandboxAttestationFailed, "completion fd")
    })?;
    close_unexpected_fds(&[launch_fd, completion_fd]).map_err(|_| {
        SupervisorError::new(FailureCode::SandboxAttestationFailed, "worker fd close")
    })?;

    let mut command = Command::new("/usr/bin/bash");
    command
        .arg(&canonical_runner)
        .arg("--internal-worker")
        .arg(slice.as_str())
        .env_clear()
        .env("PATH", FIXED_PATH)
        .env("LC_ALL", "C")
        .env("HOME", "/tmp/kiana-home")
        .env("TMPDIR", SANDBOX_WORKER_TMP)
        .env("USER", "kiana")
        .env("LOGNAME", "kiana")
        .env("SHELL", "/usr/bin/bash")
        .env("KIANA_GOVERNANCE_PYTHON", EMBEDDED_PYTHON)
        .env("KIANA_GOVERNANCE_LAUNCH_FD", launch_fd.to_string())
        .env("KIANA_GOVERNANCE_COMPLETION_FD", completion_fd.to_string());
    let error = command.exec();
    Err(SupervisorError::new(
        FailureCode::SandboxStartFailed,
        if error.kind() == io::ErrorKind::NotFound {
            "worker bash missing"
        } else {
            "worker exec"
        },
    ))
}

pub struct SupervisorOutcome {
    pub exit_code: i32,
    pub public_stdout: Vec<u8>,
    pub public_stderr: Vec<u8>,
    pub verdict: Verdict,
}

fn unavailable_verdict() -> Verdict {
    Verdict {
        launchers_trusted: false,
        sandbox: SandboxState::NotStarted,
        deadline: DeadlineState::Unknown,
        worker_exit: WorkerExit::Unknown,
        trace_channel: TraceChannelState::Unknown,
        trace: TraceVerdict::Unknown,
        receipt: ReceiptVerdict::Unknown,
        stdout: CaptureVerdict::Unknown,
        stderr: CaptureVerdict::Unknown,
        stderr_empty: false,
        worker_stdout_has_final_marker: false,
        process_tree: ProcessTreeState::Unknown,
        worker_tmp: WorkerTmpState::Unknown,
        runtime_cleanup: CleanupState::Unknown,
    }
}

pub fn run_public(
    config: &SupervisorConfig,
    slice: Slice,
    launcher: &dyn ProcessLauncher,
) -> SupervisorOutcome {
    let _signal_lock = match SIGNAL_HANDLER_LOCK.lock() {
        Ok(lock) => lock,
        Err(_) => {
            return failure_outcome(unavailable_verdict(), FailureCode::SupervisorUnavailable, 1)
        }
    };
    INTERRUPTED_SIGNAL.store(0, Ordering::SeqCst);
    let mut signal_handlers = match SignalHandlerGuard::install() {
        Ok(handlers) => handlers,
        Err(_) => {
            return failure_outcome(unavailable_verdict(), FailureCode::SupervisorUnavailable, 1)
        }
    };
    let mut outcome = run_public_with_signal_handlers(config, slice, launcher);
    let late_interrupt = current_interrupt();
    if signal_handlers.restore().is_err() {
        return failure_outcome(outcome.verdict, FailureCode::SupervisorUnavailable, 1);
    }
    if let Some((signal, exit_code)) = late_interrupt {
        outcome.verdict.deadline = DeadlineState::Interrupted(signal);
        return failure_outcome(outcome.verdict, FailureCode::WorkerFailed, exit_code);
    }
    outcome
}

fn run_public_with_signal_handlers(
    config: &SupervisorConfig,
    slice: Slice,
    launcher: &dyn ProcessLauncher,
) -> SupervisorOutcome {
    let mut verdict = unavailable_verdict();
    if let Err(error) = validate_fixed_launchers(&FixedLauncherPaths::default()) {
        return failure_outcome(verdict, error.code, 1);
    }
    verdict.launchers_trusted = true;
    if let Some((signal, exit_code)) = current_interrupt() {
        verdict.deadline = DeadlineState::Interrupted(signal);
        return failure_outcome(verdict, FailureCode::WorkerFailed, exit_code);
    }

    let runtime = match RuntimeRoot::create() {
        Ok(runtime) => runtime,
        Err(error) => return failure_outcome(verdict, error.code, 1),
    };
    let token = match LaunchToken::generate() {
        Ok(token) => token,
        Err(_) => {
            verdict.runtime_cleanup = runtime.cleanup();
            return failure_outcome(verdict, FailureCode::SandboxStartFailed, 1);
        }
    };
    let channels = match ChannelSet::create() {
        Ok(channels) => channels,
        Err(error) => {
            verdict.runtime_cleanup = runtime.cleanup();
            return failure_outcome(verdict, error.code, 1);
        }
    };
    let spec = match build_sandbox_launch_spec(config, &runtime, &channels.child, &token, slice) {
        Ok(spec) => spec,
        Err(error) => {
            verdict.runtime_cleanup = runtime.cleanup();
            return failure_outcome(verdict, error.code, 1);
        }
    };
    if let Some((signal, exit_code)) = current_interrupt() {
        verdict.deadline = DeadlineState::Interrupted(signal);
        verdict.runtime_cleanup = runtime.cleanup();
        return failure_outcome(verdict, FailureCode::WorkerFailed, exit_code);
    }
    let ChannelSet { parent, child } = channels;
    let mut process = match launcher.spawn(&spec, child) {
        Ok(process) => process,
        Err(error) => {
            verdict.sandbox = SandboxState::Failed;
            verdict.runtime_cleanup = runtime.cleanup();
            return failure_outcome(verdict, error.code, 1);
        }
    };
    verdict.sandbox = SandboxState::Started;

    let ParentChannels {
        launch_write,
        completion_read,
        worker_stdout_read,
        worker_stderr_read,
        guard_stderr_read,
        trace_read,
    } = parent;
    let capture_failed = Arc::new(AtomicBool::new(false));
    let stdout_reader = read_bounded_monitored(
        File::from(worker_stdout_read),
        config.limits.stdout,
        Arc::clone(&capture_failed),
    );
    let stderr_reader = read_bounded_monitored(
        File::from(worker_stderr_read),
        config.limits.stderr,
        Arc::clone(&capture_failed),
    );
    let guard_reader = read_bounded_monitored(
        File::from(guard_stderr_read),
        config.limits.stderr,
        Arc::clone(&capture_failed),
    );
    let trace_reader = read_bounded_monitored(
        File::from(trace_read),
        config.limits.trace,
        Arc::clone(&capture_failed),
    );
    let receipt_timing = Arc::new(ReceiptTiming::default());
    let receipt_reader = read_receipt_bounded(
        File::from(completion_read),
        config.limits.receipt,
        Arc::clone(&receipt_timing),
        Arc::clone(&capture_failed),
    );

    let started = Instant::now();
    let launch_ready = write_launch_token(launch_write, &token).is_ok();
    let (status, deadline, process_wait_ok) = if launch_ready {
        wait_for_process(
            process.as_mut(),
            config.deadline,
            config.term_grace,
            &receipt_timing,
            &capture_failed,
        )
    } else {
        let (status, stopped) = stop_process(process.as_mut(), config.term_grace);
        (status, DeadlineState::Unknown, stopped)
    };
    receipt_timing.child_exited.store(true, Ordering::SeqCst);
    verdict.deadline = deadline;
    verdict.worker_exit = status
        .as_ref()
        .map(status_to_worker_exit)
        .unwrap_or(WorkerExit::Unknown);

    let process_group = process.process_group();
    let process_tree_clean = wait_for_group_exit(process_group, Duration::from_millis(50));
    verdict.process_tree = if process_tree_clean && process_wait_ok {
        ProcessTreeState::FullyReaped
    } else {
        let _ = process.terminate_group(libc::SIGKILL);
        let _ = wait_for_group_exit(process_group, config.term_grace);
        ProcessTreeState::DescendantsRemain
    };

    let stdout = join_capture(stdout_reader);
    let stderr = join_capture(stderr_reader);
    let guard = join_capture(guard_reader);
    let trace = join_capture(trace_reader);
    let receipt = join_receipt_capture(receipt_reader);
    verdict.stdout = stdout.verdict;
    verdict.stderr = stderr.verdict;
    verdict.stderr_empty = stderr.bytes.is_empty()
        && guard.bytes.is_empty()
        && guard.verdict == CaptureVerdict::CompleteBounded;
    verdict.worker_stdout_has_final_marker = contains_final_marker(&stdout.bytes);
    verdict.trace_channel = if trace.verdict == CaptureVerdict::CompleteBounded
        && guard.verdict == CaptureVerdict::CompleteBounded
    {
        TraceChannelState::ParentOwnedAndWorkerInaccessible
    } else {
        TraceChannelState::Unknown
    };
    verdict.trace = match trace.verdict {
        CaptureVerdict::CompleteBounded => parse_trace(&trace.bytes),
        CaptureVerdict::LimitExceeded => TraceVerdict::Oversized,
        _ => TraceVerdict::Unknown,
    };
    verdict.receipt = match receipt.capture.verdict {
        CaptureVerdict::CompleteBounded => parse_receipt(
            &receipt.capture.bytes,
            &token,
            slice,
            receipt_timing
                .observed_before_worker_exit
                .load(Ordering::SeqCst),
        ),
        CaptureVerdict::LimitExceeded => ReceiptVerdict::Oversized,
        _ => ReceiptVerdict::Unknown,
    };
    verdict.worker_tmp = runtime.inspect_worker_tmp();
    verdict.runtime_cleanup = runtime.cleanup();

    let interrupted = match verdict.deadline {
        DeadlineState::Interrupted(signal) => Some(signal),
        _ => None,
    };
    if verdict.is_authorized_success() {
        let mut public_stdout = stdout.bytes;
        if !public_stdout.is_empty() && !public_stdout.ends_with(b"\n") {
            public_stdout.push(b'\n');
        }
        public_stdout.extend_from_slice(
            format!(
                "OK: slice={} elapsed_seconds={:.3} offline=true\n",
                slice.as_str(),
                started.elapsed().as_secs_f64()
            )
            .as_bytes(),
        );
        return SupervisorOutcome {
            exit_code: 0,
            public_stdout,
            public_stderr: Vec::new(),
            verdict,
        };
    }

    let exit_code = match interrupted {
        Some(libc::SIGINT) => 130,
        Some(libc::SIGTERM) => 143,
        _ => 1,
    };
    let code = verdict.failure_code().unwrap_or(FailureCode::WorkerFailed);
    failure_outcome(verdict, code, exit_code)
}

fn failure_outcome(verdict: Verdict, code: FailureCode, exit_code: i32) -> SupervisorOutcome {
    SupervisorOutcome {
        exit_code,
        public_stdout: Vec::new(),
        public_stderr: format!("{}\n", code.render_diagnostic()).into_bytes(),
        verdict,
    }
}

fn current_interrupt() -> Option<(i32, i32)> {
    match INTERRUPTED_SIGNAL.load(Ordering::SeqCst) {
        libc::SIGINT => Some((libc::SIGINT, 130)),
        libc::SIGTERM => Some((libc::SIGTERM, 143)),
        _ => None,
    }
}

fn write_launch_token(fd: OwnedFd, token: &LaunchToken) -> io::Result<()> {
    let mut writer = File::from(fd);
    writer.write_all(token.as_hex().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

extern "C" fn record_signal(signal: i32) {
    INTERRUPTED_SIGNAL.store(signal, Ordering::SeqCst);
}

struct SignalHandlerGuard {
    previous_int: libc::sigaction,
    previous_term: libc::sigaction,
    active: bool,
}

impl SignalHandlerGuard {
    fn install() -> io::Result<Self> {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = record_signal as *const () as usize;
        action.sa_flags = 0;
        if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut previous_int: libc::sigaction = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous_int) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut previous_term: libc::sigaction = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigaction(libc::SIGTERM, &action, &mut previous_term) } != 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::sigaction(libc::SIGINT, &previous_int, std::ptr::null_mut());
            }
            return Err(error);
        }
        Ok(Self {
            previous_int,
            previous_term,
            active: true,
        })
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        let term_result =
            unsafe { libc::sigaction(libc::SIGTERM, &self.previous_term, std::ptr::null_mut()) };
        let term_error = (term_result != 0).then(io::Error::last_os_error);
        let int_result =
            unsafe { libc::sigaction(libc::SIGINT, &self.previous_int, std::ptr::null_mut()) };
        let int_error = (int_result != 0).then(io::Error::last_os_error);
        self.active = false;
        INTERRUPTED_SIGNAL.store(0, Ordering::SeqCst);
        match (term_error, int_error) {
            (None, None) => Ok(()),
            (Some(error), _) | (None, Some(error)) => Err(error),
        }
    }
}

impl Drop for SignalHandlerGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn wait_for_process(
    process: &mut dyn RunningProcess,
    deadline: Duration,
    term_grace: Duration,
    receipt_timing: &ReceiptTiming,
    capture_failed: &AtomicBool,
) -> (Option<ExitStatus>, DeadlineState, bool) {
    let started = Instant::now();
    loop {
        match process.try_wait() {
            Ok(Some(status)) => {
                receipt_timing.child_exited.store(true, Ordering::SeqCst);
                let signal = INTERRUPTED_SIGNAL.load(Ordering::SeqCst);
                let state = if signal == libc::SIGINT || signal == libc::SIGTERM {
                    DeadlineState::Interrupted(signal)
                } else {
                    DeadlineState::Completed
                };
                return (Some(status), state, true);
            }
            Ok(None) => {
                receipt_timing.confirm_early_while_child_is_running();
            }
            Err(_) => {
                let (status, clean) = stop_process(process, term_grace);
                receipt_timing.child_exited.store(true, Ordering::SeqCst);
                return (status, DeadlineState::Unknown, clean);
            }
        }
        let signal = INTERRUPTED_SIGNAL.load(Ordering::SeqCst);
        if signal == libc::SIGINT || signal == libc::SIGTERM {
            let (status, clean) = stop_process(process, term_grace);
            receipt_timing.child_exited.store(true, Ordering::SeqCst);
            return (status, DeadlineState::Interrupted(signal), clean);
        }
        if capture_failed.load(Ordering::SeqCst) {
            let (status, clean) = stop_process(process, term_grace);
            receipt_timing.child_exited.store(true, Ordering::SeqCst);
            return (status, DeadlineState::Unknown, clean);
        }
        if started.elapsed() >= deadline {
            let (status, clean) = stop_process(process, term_grace);
            receipt_timing.child_exited.store(true, Ordering::SeqCst);
            return (status, DeadlineState::Exceeded, clean);
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn stop_process(
    process: &mut dyn RunningProcess,
    term_grace: Duration,
) -> (Option<ExitStatus>, bool) {
    let mut clean = process.terminate_group(libc::SIGTERM).is_ok();
    let started = Instant::now();
    while started.elapsed() < term_grace {
        match process.try_wait() {
            Ok(Some(status)) => return (Some(status), clean),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                clean = false;
                break;
            }
        }
    }
    clean &= process.terminate_group(libc::SIGKILL).is_ok();
    match process.wait() {
        Ok(status) => (Some(status), clean),
        Err(_) => (None, false),
    }
}

fn group_exists(process_group: libc::pid_t) -> bool {
    let result = unsafe { libc::kill(-process_group, 0) };
    if result == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn wait_for_group_exit(process_group: libc::pid_t, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !group_exists(process_group) {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    !group_exists(process_group)
}

fn status_to_worker_exit(status: &ExitStatus) -> WorkerExit {
    match status.code() {
        Some(code) => WorkerExit::Code(code),
        None => status
            .signal()
            .map(WorkerExit::Signaled)
            .unwrap_or(WorkerExit::Unknown),
    }
}

fn join_capture(handle: JoinHandle<BoundedCapture>) -> BoundedCapture {
    handle.join().unwrap_or(BoundedCapture {
        bytes: Vec::new(),
        verdict: CaptureVerdict::ReadFailed,
    })
}

fn join_receipt_capture(handle: JoinHandle<ReceiptCapture>) -> ReceiptCapture {
    handle.join().unwrap_or(ReceiptCapture {
        capture: BoundedCapture {
            bytes: Vec::new(),
            verdict: CaptureVerdict::ReadFailed,
        },
        observed_before_worker_exit: false,
    })
}

pub fn contains_final_marker(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).contains(FINAL_MARKER)
}

pub fn validate_semantic_diagnostic(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_STDERR_BYTES || contains_final_marker(bytes) {
        return false;
    }
    let text = String::from_utf8_lossy(bytes);
    !text.contains("/tmp/")
        && !text.contains("/home/")
        && !text.contains("/var/")
        && !text
            .split_whitespace()
            .any(|word| word.len() == 64 && word.chars().all(|c| c.is_ascii_hexdigit()))
}

#[allow(dead_code)]
fn child_status(child: &mut Child) -> WorkerExit {
    match child.try_wait() {
        Ok(Some(status)) => status
            .code()
            .map(WorkerExit::Code)
            .unwrap_or(WorkerExit::Signaled(-1)),
        Ok(None) => WorkerExit::Missing,
        Err(_) => WorkerExit::Unknown,
    }
}

#[allow(dead_code)]
fn open_dev_null() -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open("/dev/null")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn success_verdict() -> Verdict {
        Verdict {
            launchers_trusted: true,
            sandbox: SandboxState::Started,
            deadline: DeadlineState::Completed,
            worker_exit: WorkerExit::Code(SUPERVISOR_WORKER_EXIT_CODE),
            trace_channel: TraceChannelState::ParentOwnedAndWorkerInaccessible,
            trace: TraceVerdict::ExactStartupCanaryOnly,
            receipt: ReceiptVerdict::ExactAndEof,
            stdout: CaptureVerdict::CompleteBounded,
            stderr: CaptureVerdict::CompleteBounded,
            stderr_empty: true,
            worker_stdout_has_final_marker: false,
            process_tree: ProcessTreeState::FullyReaped,
            worker_tmp: WorkerTmpState::Empty,
            runtime_cleanup: CleanupState::Removed,
        }
    }

    #[test]
    fn verdict_only_all_true_authorizes_success() {
        assert!(success_verdict().is_authorized_success());
    }

    #[test]
    fn verdict_worker_exit_zero_fails_closed() {
        let mut verdict = success_verdict();
        verdict.worker_exit = WorkerExit::Code(0);
        assert!(!verdict.is_authorized_success());
    }

    #[test]
    fn verdict_each_required_component_failure_blocks_success() {
        let mut variants = Vec::new();

        let mut verdict = success_verdict();
        verdict.launchers_trusted = false;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.sandbox = SandboxState::Failed;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.deadline = DeadlineState::Exceeded;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.worker_exit = WorkerExit::Code(0);
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.trace_channel = TraceChannelState::WorkerVisible;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.trace = TraceVerdict::Invalid;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.receipt = ReceiptVerdict::Early;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.stdout = CaptureVerdict::LimitExceeded;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.stderr = CaptureVerdict::LimitExceeded;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.stderr_empty = false;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.worker_stdout_has_final_marker = true;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.process_tree = ProcessTreeState::DescendantsRemain;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.worker_tmp = WorkerTmpState::NonEmpty;
        variants.push(verdict);
        let mut verdict = success_verdict();
        verdict.runtime_cleanup = CleanupState::Residue;
        variants.push(verdict);

        assert_eq!(variants.len(), 14);
        assert!(variants
            .iter()
            .all(|verdict| !verdict.is_authorized_success()));
    }

    #[test]
    fn verdict_unknown_states_fail_closed() {
        let mut verdict = success_verdict();
        verdict.deadline = DeadlineState::Unknown;
        assert!(!verdict.is_authorized_success());
        verdict.deadline = DeadlineState::Completed;
        verdict.trace = TraceVerdict::Unknown;
        assert!(!verdict.is_authorized_success());
    }

    #[test]
    fn parse_receipt_accepts_exact_token_slice_eof() {
        let token = LaunchToken::from_bytes([0xabu8; 32]);
        let bytes = format!("complete:{}:schemas\n", token.as_hex()).into_bytes();
        assert_eq!(
            parse_receipt(&bytes, &token, Slice::Schemas, false),
            ReceiptVerdict::ExactAndEof
        );
        assert_eq!(
            parse_receipt(&bytes, &token, Slice::Schemas, true),
            ReceiptVerdict::Early
        );
    }

    #[test]
    fn parse_receipt_rejects_wrong_token_slice_and_trailing() {
        let token = LaunchToken::from_bytes([0xabu8; 32]);
        let wrong = LaunchToken::from_bytes([0xcdu8; 32]);
        let wrong_token = format!("complete:{}:schemas\n", wrong.as_hex());
        assert_eq!(
            parse_receipt(wrong_token.as_bytes(), &token, Slice::Schemas, false),
            ReceiptVerdict::WrongToken
        );
        let wrong_slice = format!("complete:{}:fixture-shapes\n", token.as_hex());
        assert_eq!(
            parse_receipt(wrong_slice.as_bytes(), &token, Slice::Schemas, false),
            ReceiptVerdict::WrongSlice
        );
        let trailing = format!("complete:{}:schemas\nextra", token.as_hex());
        assert_eq!(
            parse_receipt(trailing.as_bytes(), &token, Slice::Schemas, false),
            ReceiptVerdict::TrailingBytes
        );
        assert_eq!(
            parse_receipt(b"", &token, Slice::Schemas, false),
            ReceiptVerdict::Missing
        );
        let duplicate = format!(
            "complete:{}:schemas\ncomplete:{}:schemas\n",
            token.as_hex(),
            token.as_hex()
        );
        assert_eq!(
            parse_receipt(duplicate.as_bytes(), &token, Slice::Schemas, false),
            ReceiptVerdict::Duplicate
        );
        assert_eq!(
            parse_receipt(
                &vec![b'x'; MAX_RECEIPT_BYTES + 1],
                &token,
                Slice::Schemas,
                false,
            ),
            ReceiptVerdict::Oversized
        );
    }

    #[test]
    fn parse_trace_accepts_exact_startup_canary_only() {
        let trace = b"[pid 1] socket(AF_UNIX, SOCK_STREAM, 0) = -1 EPERM (Operation not permitted) (INJECTED)\n";
        assert_eq!(parse_trace(trace), TraceVerdict::ExactStartupCanaryOnly);
    }

    #[test]
    fn parse_trace_rejects_extra_network_and_io_uring_attempts() {
        let trace = b"socket(AF_UNIX, SOCK_STREAM, 0) = -1 EPERM (INJECTED)\nconnect(3, 0x1234, 16) = -1 EPERM (INJECTED)\n";
        assert_eq!(
            parse_trace(trace),
            TraceVerdict::NetworkAttempt {
                syscall: "connect".to_owned()
            }
        );
        let trace = b"io_uring_setup(1, 0x1234) = -1 EPERM (INJECTED)\n";
        assert_eq!(
            parse_trace(trace),
            TraceVerdict::NetworkAttempt {
                syscall: "io_uring_setup".to_owned()
            }
        );
        assert_eq!(parse_trace(b"\xffsocket()\n"), TraceVerdict::Invalid);
        assert_eq!(
            FailureCode::NetworkAttempt {
                syscall: "socket;secret=/home/user".to_owned()
            }
            .render_diagnostic(),
            "network_attempt: blocked syscall=unknown"
        );
    }

    #[test]
    fn bounded_capture_accepts_limit_and_rejects_limit_plus_one() {
        let exact = read_bounded(Cursor::new(b"abc".to_vec()), 3)
            .join()
            .unwrap();
        assert_eq!(exact.verdict, CaptureVerdict::CompleteBounded);
        let overflow = read_bounded(Cursor::new(b"abcd".to_vec()), 3)
            .join()
            .unwrap();
        assert_eq!(overflow.verdict, CaptureVerdict::LimitExceeded);
    }

    #[test]
    fn invocation_parser_accepts_public_and_internal_shapes() {
        let args = vec![OsString::from("supervisor"), OsString::from("schemas")];
        assert!(matches!(
            parse_invocation(&args),
            Ok(Invocation::Public(Slice::Schemas))
        ));
        let args = vec![
            OsString::from("supervisor"),
            OsString::from("__trace-logger"),
        ];
        assert!(matches!(
            parse_invocation(&args),
            Ok(Invocation::TraceLogger)
        ));
    }

    #[test]
    fn receipt_timing_confirms_first_byte_while_child_remains_running() {
        let timing = ReceiptTiming::default();
        timing.record_first_byte();
        thread::sleep(RECEIPT_EARLY_CONFIRMATION + Duration::from_millis(2));
        timing.confirm_early_while_child_is_running();
        assert!(timing.observed_before_worker_exit.load(Ordering::SeqCst));
    }

    #[test]
    fn runtime_root_revalidates_metadata_before_cleanup() {
        let runtime = RuntimeRoot::create().unwrap();
        let root = runtime.path().to_path_buf();
        assert!(root.starts_with("/tmp"));
        assert_eq!(runtime.inspect_worker_tmp(), WorkerTmpState::Empty);
        fs::set_permissions(runtime.worker_tmp(), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(runtime.inspect_worker_tmp(), WorkerTmpState::Unknown);
        assert_eq!(runtime.cleanup(), CleanupState::Removed);
        assert!(!root.exists());
    }

    #[test]
    fn embedded_python_rechecks_build_uid_gid_and_ignores_runtime_path() {
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("PATH", "/definitely/not/python");
        let executable = embedded_python_path().unwrap();
        match previous_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        assert_eq!(executable.path, PathBuf::from(EMBEDDED_PYTHON));
        assert_eq!(executable.uid.to_string(), EMBEDDED_PYTHON_UID);
        assert_eq!(executable.gid.to_string(), EMBEDDED_PYTHON_GID);
    }

    #[test]
    fn signal_handler_guard_restores_previous_handlers() {
        fn action(signal: i32) -> libc::sigaction {
            let mut current: libc::sigaction = unsafe { std::mem::zeroed() };
            assert_eq!(
                unsafe { libc::sigaction(signal, std::ptr::null(), &mut current) },
                0
            );
            current
        }

        let _lock = SIGNAL_HANDLER_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before_int = action(libc::SIGINT);
        let before_term = action(libc::SIGTERM);
        let mut guard = SignalHandlerGuard::install().unwrap();
        assert_eq!(
            action(libc::SIGINT).sa_sigaction,
            record_signal as *const () as usize
        );
        assert_eq!(
            action(libc::SIGTERM).sa_sigaction,
            record_signal as *const () as usize
        );
        guard.restore().unwrap();
        let after_int = action(libc::SIGINT);
        let after_term = action(libc::SIGTERM);
        assert_eq!(after_int.sa_sigaction, before_int.sa_sigaction);
        assert_eq!(after_term.sa_sigaction, before_term.sa_sigaction);
    }
}
