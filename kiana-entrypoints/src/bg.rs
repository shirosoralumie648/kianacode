//! 历史后台任务与常驻 daemon worker 兼容入口。
//!
//! 本模块把任务元数据和日志保存到本地目录，并通过当前可执行文件派生 worker 子进程。
//! 它保留的是旧后台任务面，不是 CompanyOS 产品脊柱的第二个授权根；尤其是
//! `run_worker_at` 的 `record_session` 路径仍调用旧 SDK prompt 接口，不能当作新的
//! `DaemonHost`/ControlPlane 执行证据。新能力应沿 `kiana-entrypoints -> kiana-daemon`
//! 主路径实现，调用这些兼容 API 时必须把它们的本地状态与 EventLog 分开看待。
//!
//! 任务文件、daemon 状态文件和日志都属于可恢复的本地展示/调度记录。写入成功不等于
//! 子进程已经完成，发送终止信号也不等于目标进程已经停止；调用方应再次读取状态并结合
//! 真实 receipt 验证结果。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskStatus {
    /// 已创建但尚未派生 worker。
    Pending,
    /// 已记录 worker PID，等待 worker 更新结果。
    Running,
    /// worker 返回了本模块认为的成功状态。
    Completed,
    /// worker 捕获到错误并写入了错误文本。
    Failed,
    /// 用户请求发送终止信号后记录的本地状态；不保证进程已退出。
    Killed,
}

/// 后台任务的持久化元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    /// 任务 UUID，同时作为任务和日志文件名的一部分。
    pub id: String,
    /// 提交给 worker 的提示词原文（去除首尾空白后保存）。
    pub prompt: String,
    /// worker 应切换到的工作目录文字；启动前由操作系统检查是否可用。
    pub cwd: String,
    /// 本地记录的任务生命周期状态。
    pub status: BackgroundTaskStatus,
    /// 最近一次 worker 的进程 ID；任务结束或被标记终止后清空。
    pub pid: Option<u32>,
    /// 旧 SDK 会话返回的 ID；没有执行会话时保持为空。
    pub session_id: Option<String>,
    /// 创建时间的 Unix 秒数。
    pub created_at: u64,
    /// 最近一次元数据更新的 Unix 秒数。
    pub updated_at: u64,
    /// worker 退出码的本地摘要，不等于完整进程等待证据。
    pub exit_code: Option<i32>,
    /// 最近一次失败的可读错误；成功任务应清空该字段。
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidentDaemonStatus {
    /// 常驻 worker 被认为正在运行。
    Running,
    /// 没有活跃 PID，或最近一次探测已确认 PID 消失。
    Stopped,
}

/// 常驻后台 supervisor 的本地状态快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResidentDaemonState {
    /// 保存状态和任务文件的根目录文字。
    pub root: String,
    /// supervisor 最近记录的运行状态。
    pub status: ResidentDaemonStatus,
    /// 常驻 worker 的进程 ID；停止状态下为空。
    pub pid: Option<u32>,
    /// supervisor 启动时间的 Unix 秒数。
    pub started_at: u64,
    /// 状态文件最近更新时间的 Unix 秒数。
    pub updated_at: u64,
    /// supervisor 最近一次轮询或派生失败的文字摘要。
    pub last_error: Option<String>,
}

/// 解析后台任务根目录。
///
/// 优先读取 `KIANA_BG_DIR`，其次使用 `KIANA_HOME/bg-tasks`，再退回 `$HOME/.kiana/bg-tasks`
/// 或当前目录下的 `.kiana/bg-tasks`。这里只返回路径，不创建目录，也不验证它是否可信。
pub fn default_root() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_BG_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path).join("bg-tasks");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kiana").join("bg-tasks");
    }
    PathBuf::from(".kiana").join("bg-tasks")
}

/// 在默认根目录创建一个待处理后台任务。
pub fn create_task(prompt: String, cwd: PathBuf) -> Result<BackgroundTask> {
    create_task_at(&default_root(), prompt, cwd)
}

/// 在指定根目录创建并持久化一个待处理任务。
///
/// 空提示会被拒绝；任务 ID 由 UUID 生成，创建记录和初始日志都写入成功后才返回。该函数
/// 不会启动子进程，也不检查工作目录是否能被未来 worker 使用。
pub fn create_task_at(root: &Path, prompt: String, cwd: PathBuf) -> Result<BackgroundTask> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow!("background prompt cannot be empty"));
    }

    ensure_root(root)?;
    let now = now_unix_seconds();
    let task = BackgroundTask {
        id: Uuid::new_v4().to_string(),
        prompt,
        cwd: cwd.to_string_lossy().to_string(),
        status: BackgroundTaskStatus::Pending,
        pid: None,
        session_id: None,
        created_at: now,
        updated_at: now,
        exit_code: None,
        error: None,
    };
    write_task(root, &task)?;
    append_log(root, &task.id, &format!("created task {}\n", task.id))?;
    Ok(task)
}

/// 在默认根目录为任务派生一个 detached worker。
pub fn spawn_task(task_id: &str) -> Result<BackgroundTask> {
    let root = default_root();
    spawn_task_at(&root, task_id)
}

/// 从指定根目录读取任务并派生 worker。
///
/// 派生参数会把根目录通过 `KIANA_BG_DIR` 传给子进程，并把标准输入/输出/错误全部断开。
/// 元数据在 `spawn` 成功后才更新为 `Running`；如果随后写文件失败，子进程可能已经存在，
/// 因此调用方应通过日志和 PID 重新核对，而不能把错误当作“未启动”。
pub fn spawn_task_at(root: &Path, task_id: &str) -> Result<BackgroundTask> {
    let mut task = read_task(root, task_id)?;
    let exe = std::env::current_exe().context("failed to locate current executable")?;
    let child = Command::new(exe)
        .arg("--daemon-worker")
        .arg("bg-task")
        .arg(task_id)
        .env("KIANA_BG_DIR", root)
        .current_dir(&task.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn background worker")?;

    task.status = BackgroundTaskStatus::Running;
    task.pid = Some(child.id());
    task.updated_at = now_unix_seconds();
    write_task(root, &task)?;
    append_log(
        root,
        task_id,
        &format!("spawned worker pid={}\n", child.id()),
    )?;
    Ok(task)
}

/// 处理 `--daemon-worker` 的兼容 worker 分派。
///
/// 仅接受 `bg-task` 和 `resident` 两种 kind；未知 kind 或缺少参数立即失败。该入口本身
/// 不做权限提升，后台任务是否应被允许执行仍取决于调用它的外层策略。
pub async fn run_worker(kind: Option<&str>, args: &[String]) -> Result<()> {
    match kind {
        Some("bg-task") => {
            let task_id = args
                .first()
                .ok_or_else(|| anyhow!("usage: kiana --daemon-worker bg-task <task_id>"))?;
            run_worker_at(&default_root(), task_id, true).await
        }
        Some("resident") => run_resident_daemon_at(&default_root()).await,
        Some(other) => Err(anyhow!("unknown daemon worker kind '{}'", other)),
        None => Err(anyhow!("usage: kiana --daemon-worker <kind>")),
    }
}

/// 读取默认根目录的常驻 daemon 状态，并按 PID 探测修正陈旧的 Running 标记。
pub fn resident_daemon_status() -> Result<Option<ResidentDaemonState>> {
    resident_daemon_status_at(&default_root())
}

/// 读取指定根目录的常驻 daemon 状态。
///
/// 若状态文件声称运行中但 PID 已不存在，会写回 `Stopped` 和诊断错误；PID 探测只是
/// 活跃性线索，不能证明 PID 没有被系统复用或 worker 已完成清理。
pub fn resident_daemon_status_at(root: &Path) -> Result<Option<ResidentDaemonState>> {
    resident_daemon_status_at_with(root, process_is_running)
}

fn resident_daemon_status_at_with<F>(
    root: &Path,
    is_running: F,
) -> Result<Option<ResidentDaemonState>>
where
    F: Fn(u32) -> bool,
{
    let path = daemon_file(root);
    if !path.exists() {
        return Ok(None);
    }

    let mut state: ResidentDaemonState = serde_json::from_str(&fs::read_to_string(&path)?)?;
    if state.status == ResidentDaemonStatus::Running && !state.pid.is_some_and(is_running) {
        state.status = ResidentDaemonStatus::Stopped;
        state.pid = None;
        state.updated_at = now_unix_seconds();
        state.last_error = Some("resident daemon pid is no longer running".to_string());
        write_daemon_state(root, &state)?;
    }
    Ok(Some(state))
}

/// 在默认根目录启动常驻 supervisor（若已有活动状态则直接返回该状态）。
pub fn start_resident_daemon() -> Result<ResidentDaemonState> {
    start_resident_daemon_at(&default_root())
}

/// 在指定根目录启动常驻 supervisor 并写入状态文件。
///
/// 该操作只派生当前 executable 的 `resident` worker，并以 PID/时间戳更新本地状态；它不
/// 取得 ControlPlane lease，也不保证 worker 能持续运行，因此消费者应继续轮询状态和日志。
pub fn start_resident_daemon_at(root: &Path) -> Result<ResidentDaemonState> {
    ensure_root(root)?;
    if let Some(state) = resident_daemon_status_at(root)? {
        if state.status == ResidentDaemonStatus::Running {
            return Ok(state);
        }
    }

    let exe = std::env::current_exe().context("failed to locate current executable")?;
    let child = Command::new(exe)
        .arg("--daemon-worker")
        .arg("resident")
        .env("KIANA_BG_DIR", root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn resident daemon")?;

    let now = now_unix_seconds();
    let state = ResidentDaemonState {
        root: root.to_string_lossy().to_string(),
        status: ResidentDaemonStatus::Running,
        pid: Some(child.id()),
        started_at: now,
        updated_at: now,
        last_error: None,
    };
    write_daemon_state(root, &state)?;
    append_daemon_log(
        root,
        &format!("started resident daemon pid={}\n", child.id()),
    )?;
    Ok(state)
}

/// 停止默认根目录记录的常驻 supervisor。
pub fn stop_resident_daemon() -> Result<ResidentDaemonState> {
    stop_resident_daemon_at(&default_root())
}

/// 向指定根目录记录的常驻 supervisor 发送终止信号并标记为停止。
///
/// 这里的 `Stopped` 是本地状态变更；系统调用成功仅说明信号已发送，不能证明目标进程
/// 已退出或其正在运行的任务已经回滚。
pub fn stop_resident_daemon_at(root: &Path) -> Result<ResidentDaemonState> {
    ensure_root(root)?;
    let mut state = resident_daemon_status_at(root)?.unwrap_or_else(|| {
        let now = now_unix_seconds();
        ResidentDaemonState {
            root: root.to_string_lossy().to_string(),
            status: ResidentDaemonStatus::Stopped,
            pid: None,
            started_at: now,
            updated_at: now,
            last_error: None,
        }
    });

    if let Some(pid) = state.pid {
        if process_is_running(pid) {
            terminate_process(pid)?;
            append_daemon_log(root, &format!("sent termination signal to pid={pid}\n"))?;
        }
    }
    state.status = ResidentDaemonStatus::Stopped;
    state.pid = None;
    state.updated_at = now_unix_seconds();
    write_daemon_state(root, &state)?;
    Ok(state)
}

/// 运行常驻 supervisor 的无限轮询循环。
///
/// 每轮扫描 Pending 任务并尝试派生 worker，错误写入 daemon 日志和状态文件后继续轮询。
/// 该循环没有内置退出条件，必须由外部进程终止；它是兼容后台面，不应被复用为新的
/// CompanyOS 执行循环。
pub async fn run_resident_daemon_at(root: &Path) -> Result<()> {
    ensure_root(root)?;
    let now = now_unix_seconds();
    write_daemon_state(
        root,
        &ResidentDaemonState {
            root: root.to_string_lossy().to_string(),
            status: ResidentDaemonStatus::Running,
            pid: Some(std::process::id()),
            started_at: now,
            updated_at: now,
            last_error: None,
        },
    )?;
    append_daemon_log(root, "resident daemon worker started\n")?;

    loop {
        if let Err(error) = spawn_pending_tasks_once_at(root) {
            append_daemon_log(root, &format!("supervisor error: {error}\n"))?;
            if let Some(mut state) = resident_daemon_status_at(root)? {
                state.updated_at = now_unix_seconds();
                state.last_error = Some(error.to_string());
                write_daemon_state(root, &state)?;
            }
        }
        sleep(Duration::from_secs(resident_poll_secs())).await;
    }
}

/// 扫描一次指定根目录并为所有 Pending 任务尝试派生 worker。
///
/// 返回的是成功更新到 Running 的任务列表；遇到单个任务错误会立即返回错误，已派生的
/// 兄弟任务不会自动撤销。
pub fn spawn_pending_tasks_once_at(root: &Path) -> Result<Vec<BackgroundTask>> {
    spawn_pending_tasks_once_with(root, spawn_task_at)
}

fn spawn_pending_tasks_once_with<F>(root: &Path, mut spawn: F) -> Result<Vec<BackgroundTask>>
where
    F: FnMut(&Path, &str) -> Result<BackgroundTask>,
{
    let mut spawned = Vec::new();
    for task in list_tasks_at(root)? {
        if task.status == BackgroundTaskStatus::Pending {
            spawned.push(spawn(root, &task.id)?);
        }
    }
    Ok(spawned)
}

/// 在指定根目录执行一个任务 worker，并把最终状态写回任务文件。
///
/// `record_session = true` 时沿用旧 SDK prompt 接口并保存其 session ID；这条兼容路径不
/// 等于统一 `DaemonHost` 主路径。函数会把本地状态更新为 Completed/Failed，但不会声明
/// 外部文件或网络副作用已经正确完成。
pub async fn run_worker_at(root: &Path, task_id: &str, record_session: bool) -> Result<()> {
    let mut task = read_task(root, task_id)?;
    task.status = BackgroundTaskStatus::Running;
    task.pid = Some(std::process::id());
    task.updated_at = now_unix_seconds();
    task.error = None;
    write_task(root, &task)?;
    append_log(root, task_id, "worker started\n")?;

    let result = async {
        if record_session {
            let mut options = serde_json::Map::new();
            options.insert("execute".to_string(), Value::Bool(true));
            let result =
                crate::sdk::unstable_v2_prompt(task.prompt.clone(), options.into_iter().collect())
                    .await
                    .context("failed to run background prompt in SDK session")?;
            task.session_id = result
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            append_log(
                root,
                task_id,
                &format!(
                    "SDK prompt {} in session {} (execution={})\n",
                    result
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown"),
                    task.session_id.as_deref().unwrap_or("<unknown>"),
                    result
                        .get("execution")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                ),
            )?;
        } else {
            append_log(root, task_id, "session recording skipped by test runner\n")?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    match result {
        Ok(()) => {
            task.status = BackgroundTaskStatus::Completed;
            task.exit_code = Some(0);
            task.error = None;
            append_log(root, task_id, "worker completed\n")?;
        }
        Err(e) => {
            task.status = BackgroundTaskStatus::Failed;
            task.exit_code = Some(1);
            task.error = Some(e.to_string());
            append_log(root, task_id, &format!("worker failed: {}\n", e))?;
        }
    }
    task.pid = None;
    task.updated_at = now_unix_seconds();
    write_task(root, &task)?;
    Ok(())
}

/// 列出默认根目录下的任务，并按更新时间倒序排列。
pub fn list_tasks() -> Result<Vec<BackgroundTask>> {
    list_tasks_at(&default_root())
}

/// 列出指定根目录下可解析的 JSON 任务文件。
///
/// 文件名扩展名不是 `.json` 的条目会忽略；任何 JSON 解析失败都会使整个列表失败，避免
/// 把损坏的状态静默当作不存在。
pub fn list_tasks_at(root: &Path) -> Result<Vec<BackgroundTask>> {
    ensure_root(root)?;
    let mut tasks = Vec::new();
    for entry in fs::read_dir(tasks_dir(root))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let task: BackgroundTask = serde_json::from_str(&fs::read_to_string(&path)?)?;
        tasks.push(task);
    }
    tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(tasks)
}

/// 按任务 ID 读取并解析指定根目录的任务文件。
pub fn read_task(root: &Path, task_id: &str) -> Result<BackgroundTask> {
    validate_task_id(task_id)?;
    let path = task_file(root, task_id);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("task '{}' not found", task_id))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse task file {}", path.display()))
}

/// 读取默认根目录的任务日志。
pub fn read_logs(task_id: &str) -> Result<String> {
    read_logs_at(&default_root(), task_id)
}

/// 读取指定根目录的任务日志；不存在时返回明确错误。
pub fn read_logs_at(root: &Path, task_id: &str) -> Result<String> {
    validate_task_id(task_id)?;
    let path = log_file(root, task_id);
    fs::read_to_string(&path).with_context(|| format!("task '{}' has no log file", task_id))
}

/// 向默认根目录任务的活动 PID 发送终止信号并标记任务为 Killed。
pub fn kill_task(task_id: &str) -> Result<BackgroundTask> {
    let root = default_root();
    kill_task_at(&root, task_id)
}

/// 向指定根目录任务发送终止信号并写回 Killed 状态。
///
/// 任务没有 PID 时仍会记录“无活动 PID”并更新状态；这种状态更新是用户意图记录，不是
/// 进程终止证明。PID 和任务 ID 都会先经过受限解析，避免路径穿越。
pub fn kill_task_at(root: &Path, task_id: &str) -> Result<BackgroundTask> {
    let mut task = read_task(root, task_id)?;
    if let Some(pid) = task.pid {
        terminate_process(pid)?;
        append_log(
            root,
            task_id,
            &format!("sent termination signal to pid={}\n", pid),
        )?;
    } else {
        append_log(root, task_id, "task had no active pid\n")?;
    }
    task.status = BackgroundTaskStatus::Killed;
    task.pid = None;
    task.updated_at = now_unix_seconds();
    write_task(root, &task)?;
    Ok(task)
}

/// 校验任务 ID 后，以 pretty JSON 覆盖写入任务元数据。
///
/// 写入不是 CAS，也没有跨进程锁；并发 worker 可能造成最后写入者覆盖。因此该文件只能
/// 作为兼容状态快照，不能冒充 EventLog 的不可变事实。
pub fn write_task(root: &Path, task: &BackgroundTask) -> Result<()> {
    validate_task_id(&task.id)?;
    ensure_root(root)?;
    fs::write(
        task_file(root, &task.id),
        serde_json::to_string_pretty(task)?,
    )?;
    Ok(())
}

fn ensure_root(root: &Path) -> Result<()> {
    fs::create_dir_all(tasks_dir(root))?;
    fs::create_dir_all(logs_dir(root))?;
    Ok(())
}

fn append_log(root: &Path, task_id: &str, line: &str) -> Result<()> {
    validate_task_id(task_id)?;
    ensure_root(root)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file(root, task_id))?;
    file.write_all(format!("[{}] {}", now_unix_seconds(), line).as_bytes())?;
    Ok(())
}

fn validate_task_id(task_id: &str) -> Result<()> {
    if task_id.trim().is_empty()
        || task_id.contains('/')
        || task_id.contains('\\')
        || task_id.contains("..")
    {
        return Err(anyhow!("invalid task id"));
    }
    Ok(())
}

fn tasks_dir(root: &Path) -> PathBuf {
    root.join("tasks")
}

fn logs_dir(root: &Path) -> PathBuf {
    root.join("logs")
}

fn daemon_file(root: &Path) -> PathBuf {
    root.join("daemon.json")
}

fn daemon_log_file(root: &Path) -> PathBuf {
    logs_dir(root).join("daemon.log")
}

fn task_file(root: &Path, task_id: &str) -> PathBuf {
    tasks_dir(root).join(format!("{}.json", task_id))
}

fn log_file(root: &Path, task_id: &str) -> PathBuf {
    logs_dir(root).join(format!("{}.log", task_id))
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn write_daemon_state(root: &Path, state: &ResidentDaemonState) -> Result<()> {
    ensure_root(root)?;
    fs::write(daemon_file(root), serde_json::to_string_pretty(state)?)?;
    Ok(())
}

fn append_daemon_log(root: &Path, line: &str) -> Result<()> {
    ensure_root(root)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(daemon_log_file(root))?;
    file.write_all(format!("[{}] {}", now_unix_seconds(), line).as_bytes())?;
    Ok(())
}

fn resident_poll_secs() -> u64 {
    std::env::var("KIANA_DAEMON_POLL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(2)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessTerminationCommand {
    program: &'static str,
    args: Vec<String>,
}

#[cfg(any(unix, test))]
fn unix_termination_command(pid: u32) -> ProcessTerminationCommand {
    ProcessTerminationCommand {
        program: "kill",
        args: vec!["-TERM".to_string(), pid.to_string()],
    }
}

#[cfg(any(windows, test))]
fn windows_termination_command(pid: u32) -> ProcessTerminationCommand {
    ProcessTerminationCommand {
        program: "taskkill",
        args: vec![
            "/PID".to_string(),
            pid.to_string(),
            "/T".to_string(),
            "/F".to_string(),
        ],
    }
}

#[cfg(unix)]
fn termination_command(pid: u32) -> ProcessTerminationCommand {
    unix_termination_command(pid)
}

#[cfg(windows)]
fn termination_command(pid: u32) -> ProcessTerminationCommand {
    windows_termination_command(pid)
}

#[cfg(any(unix, windows))]
fn terminate_process(pid: u32) -> Result<()> {
    let command = termination_command(pid);
    let status = Command::new(command.program)
        .args(&command.args)
        .status()
        .with_context(|| format!("failed to invoke {}", command.program))?;
    if !status.success() {
        return Err(anyhow!("{} returned status {}", command.program, status));
    }
    Ok(())
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(any(windows, test))]
fn tasklist_output_contains_pid(output: &[u8], pid: u32) -> bool {
    let needle = format!("\",\"{pid}\",\"");
    String::from_utf8_lossy(output)
        .lines()
        .any(|line| line.contains(&needle))
}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    Command::new("tasklist")
        .arg("/FI")
        .arg(filter)
        .arg("/FO")
        .arg("CSV")
        .arg("/NH")
        .output()
        .map(|output| output.status.success() && tasklist_output_contains_pid(&output.stdout, pid))
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn terminate_process(_pid: u32) -> Result<()> {
    Err(anyhow!(
        "background task termination is not supported on this platform yet"
    ))
}

#[cfg(not(any(unix, windows)))]
fn process_is_running(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        create_task_at, kill_task_at, list_tasks_at, read_logs_at, read_task,
        resident_daemon_status_at, spawn_pending_tasks_once_with, stop_resident_daemon_at,
        write_daemon_state, BackgroundTaskStatus, ResidentDaemonState, ResidentDaemonStatus,
    };
    use std::fs;
    use uuid::Uuid;

    fn test_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("kiana-bg-{}-{}", name, Uuid::new_v4()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[tokio::test]
    async fn background_worker_updates_task_and_logs() {
        let root = test_root("worker");
        let task = create_task_at(
            &root,
            "inspect background runner".to_string(),
            std::env::current_dir().unwrap(),
        )
        .unwrap();

        super::run_worker_at(&root, &task.id, false).await.unwrap();

        let updated = read_task(&root, &task.id).unwrap();
        assert_eq!(updated.status, BackgroundTaskStatus::Completed);
        let logs = read_logs_at(&root, &task.id).unwrap();
        assert!(logs.contains("worker started"));
        assert!(logs.contains("worker completed"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn background_task_store_lists_and_kills_without_pid() {
        let root = test_root("store");
        let task = create_task_at(
            &root,
            "store prompt".to_string(),
            std::env::current_dir().unwrap(),
        )
        .unwrap();
        let listed = list_tasks_at(&root).unwrap();
        assert_eq!(listed.len(), 1);

        let killed = kill_task_at(&root, &task.id).unwrap();
        assert_eq!(killed.status, BackgroundTaskStatus::Killed);
        let logs = read_logs_at(&root, &task.id).unwrap();
        assert!(logs.contains("task had no active pid"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resident_daemon_state_marks_dead_pid_as_stopped() {
        let root = test_root("daemon-state");
        let now = super::now_unix_seconds();
        write_daemon_state(
            &root,
            &ResidentDaemonState {
                root: root.to_string_lossy().to_string(),
                status: ResidentDaemonStatus::Running,
                pid: Some(u32::MAX),
                started_at: now,
                updated_at: now,
                last_error: None,
            },
        )
        .unwrap();

        let state = super::resident_daemon_status_at_with(&root, |_| false)
            .unwrap()
            .unwrap();
        assert_eq!(state.status, ResidentDaemonStatus::Stopped);
        assert!(state.last_error.unwrap().contains("pid"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn process_termination_commands_cover_unix_and_windows() {
        let unix = super::unix_termination_command(12345);
        assert_eq!(unix.program, "kill");
        assert_eq!(unix.args, vec!["-TERM", "12345"]);

        let windows = super::windows_termination_command(12345);
        assert_eq!(windows.program, "taskkill");
        assert_eq!(windows.args, vec!["/PID", "12345", "/T", "/F"]);
    }

    #[test]
    fn tasklist_output_matches_exact_pid_field() {
        let output = br#""cmd.exe","12345","Console","1","4,096 K"
"other.exe","912345","Console","1","4,096 K"
"#;
        assert!(super::tasklist_output_contains_pid(output, 12345));
        assert!(!super::tasklist_output_contains_pid(output, 2345));
    }

    #[test]
    fn resident_supervisor_spawns_pending_tasks_only() {
        let root = test_root("daemon-pending");
        let pending = create_task_at(
            &root,
            "pending prompt".to_string(),
            std::env::current_dir().unwrap(),
        )
        .unwrap();
        let completed = create_task_at(
            &root,
            "completed prompt".to_string(),
            std::env::current_dir().unwrap(),
        )
        .unwrap();
        let mut completed_task = read_task(&root, &completed.id).unwrap();
        completed_task.status = BackgroundTaskStatus::Completed;
        super::write_task(&root, &completed_task).unwrap();

        let spawned = spawn_pending_tasks_once_with(&root, |root, task_id| {
            let mut task = read_task(root, task_id)?;
            task.status = BackgroundTaskStatus::Running;
            super::write_task(root, &task)?;
            Ok(task)
        })
        .unwrap();

        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0].id, pending.id);
        assert_eq!(
            read_task(&root, &pending.id).unwrap().status,
            BackgroundTaskStatus::Running
        );
        assert_eq!(
            read_task(&root, &completed.id).unwrap().status,
            BackgroundTaskStatus::Completed
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stop_resident_daemon_without_existing_state_writes_stopped_state() {
        let root = test_root("daemon-stop");

        let state = stop_resident_daemon_at(&root).unwrap();

        assert_eq!(state.status, ResidentDaemonStatus::Stopped);
        assert_eq!(
            resident_daemon_status_at(&root).unwrap().unwrap().status,
            ResidentDaemonStatus::Stopped
        );

        let _ = fs::remove_dir_all(root);
    }
}
