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
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: String,
    pub prompt: String,
    pub cwd: String,
    pub status: BackgroundTaskStatus,
    pub pid: Option<u32>,
    pub session_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidentDaemonStatus {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResidentDaemonState {
    pub root: String,
    pub status: ResidentDaemonStatus,
    pub pid: Option<u32>,
    pub started_at: u64,
    pub updated_at: u64,
    pub last_error: Option<String>,
}

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

pub fn create_task(prompt: String, cwd: PathBuf) -> Result<BackgroundTask> {
    create_task_at(&default_root(), prompt, cwd)
}

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

pub fn spawn_task(task_id: &str) -> Result<BackgroundTask> {
    let root = default_root();
    spawn_task_at(&root, task_id)
}

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

pub fn resident_daemon_status() -> Result<Option<ResidentDaemonState>> {
    resident_daemon_status_at(&default_root())
}

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

pub fn start_resident_daemon() -> Result<ResidentDaemonState> {
    start_resident_daemon_at(&default_root())
}

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

pub fn stop_resident_daemon() -> Result<ResidentDaemonState> {
    stop_resident_daemon_at(&default_root())
}

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

pub fn list_tasks() -> Result<Vec<BackgroundTask>> {
    list_tasks_at(&default_root())
}

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

pub fn read_task(root: &Path, task_id: &str) -> Result<BackgroundTask> {
    validate_task_id(task_id)?;
    let path = task_file(root, task_id);
    let contents =
        fs::read_to_string(&path).with_context(|| format!("task '{}' not found", task_id))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse task file {}", path.display()))
}

pub fn read_logs(task_id: &str) -> Result<String> {
    read_logs_at(&default_root(), task_id)
}

pub fn read_logs_at(root: &Path, task_id: &str) -> Result<String> {
    validate_task_id(task_id)?;
    let path = log_file(root, task_id);
    fs::read_to_string(&path).with_context(|| format!("task '{}' has no log file", task_id))
}

pub fn kill_task(task_id: &str) -> Result<BackgroundTask> {
    let root = default_root();
    kill_task_at(&root, task_id)
}

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
