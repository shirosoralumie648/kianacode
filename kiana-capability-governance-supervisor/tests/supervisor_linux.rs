#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use kiana_capability_governance_supervisor::{
    run_public, CleanupState, FailureCode, LinuxProcessLauncher, Slice, SupervisorConfig,
    WorkerExit, SUPERVISOR_WORKER_EXIT_CODE,
};

static TEST_LOCK: Mutex<()> = Mutex::new(());
static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn supervisor_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kiana-capability-governance-supervisor"))
}

fn run_binary(slice: &str) -> Output {
    Command::new(supervisor_bin())
        .arg(slice)
        .current_dir(repo_root())
        .env_clear()
        .output()
        .unwrap()
}

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn linux_public_slices_succeed_after_prebuilt_binary() {
    let _guard = test_lock();
    for slice in [
        "schemas",
        "fixture-shapes",
        "public-baseline",
        "reference-governance",
        "semantic-negative",
        "drift-refresh",
        "legacy-authority",
        "generated-views",
        "production",
    ] {
        let output = run_binary(slice);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout.matches("offline=true").count(), 1);
        assert!(stdout.contains(&format!("OK: slice={slice} ")));
        assert!(!stdout.contains("socket("));
        assert!(!stdout.contains("INJECTED"));
        assert!(!stdout.contains("kiana-capability-governance-"));
    }
}

#[test]
fn linux_public_invocation_ignores_caller_controlled_cwd() {
    let _guard = test_lock();
    let caller_cwd = test_root("caller-cwd");
    let output = Command::new(supervisor_bin())
        .arg("fixture-shapes")
        .current_dir(&caller_cwd)
        .env_clear()
        .output()
        .unwrap();
    fs::remove_dir_all(caller_cwd).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .matches("offline=true")
            .count(),
        1
    );
}

#[test]
fn linux_public_usage_and_direct_internal_modes_fail_closed() {
    let _guard = test_lock();
    for args in [vec![], vec!["unknown"]] {
        let output = Command::new(supervisor_bin())
            .args(args)
            .current_dir(repo_root())
            .env_clear()
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("offline=true"));
    }
    let output = Command::new(supervisor_bin())
        .args([
            "__worker-launcher",
            "--runner",
            "/tmp/forged",
            "--slice",
            "schemas",
        ])
        .current_dir(repo_root())
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("offline=true"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("/tmp/forged"));
}

#[test]
fn linux_runtime_environment_shims_do_not_replace_fixed_launchers() {
    let _guard = test_lock();
    let output = Command::new(supervisor_bin())
        .arg("fixture-shapes")
        .current_dir(repo_root())
        .env_clear()
        .env("PATH", "/definitely/not/a/launcher/path")
        .env("BASH_ENV", "/definitely/not/a/bash/env")
        .env("ENV", "/definitely/not/a/shell/env")
        .env("PYTHONPATH", "/definitely/not/python")
        .env("PYTHONHOME", "/definitely/not/python")
        .env("TMPDIR", "/definitely/not/tmp")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .matches("offline=true")
            .count(),
        1
    );
}

fn test_root(label: &str) -> PathBuf {
    let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = PathBuf::from(format!(
        "/var/tmp/kiana-supervisor-test-{}-{suffix}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(&root).unwrap();
    fs::create_dir(root.join("scripts")).unwrap();
    root
}

fn create_custom_runner(body: &str, label: &str) -> (PathBuf, PathBuf) {
    let root = test_root(label);
    let runner = root.join("scripts/capability-governance-smoke.sh");
    let script = format!(
        r#"#!/usr/bin/bash
set -euo pipefail
[[ "$#" == 2 && "$1" == "--internal-worker" ]]
slice="$2"
[[ -z "${{KIANA_GOVERNANCE_LAUNCH_FD:-}}" ]]
[[ -z "${{KIANA_GOVERNANCE_COMPLETION_FD:-}}" ]]
python="${{KIANA_GOVERNANCE_PYTHON}}"
set +e
"$python" -I - <<'PY'
import errno
import socket
try:
    socket.socket(socket.AF_INET, socket.SOCK_STREAM)
except OSError as exc:
    raise SystemExit(80 if exc.errno == errno.EPERM else 81)
raise SystemExit(82)
PY
canary=$?
set -e
[[ "$canary" == 80 ]]
{body}
exit 80
"#
    );
    fs::write(&runner, script).unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let runner = fs::canonicalize(runner).unwrap();
    (root, runner)
}

fn custom_runner(
    body: &str,
    deadline: Duration,
) -> kiana_capability_governance_supervisor::SupervisorOutcome {
    let (root, runner) = create_custom_runner(body, "runner");
    let mut config = SupervisorConfig::new(root.clone(), runner, supervisor_bin());
    config.deadline = deadline;
    config.term_grace = Duration::from_millis(100);
    let outcome = run_public(&config, Slice::FixtureShapes, &LinuxProcessLauncher);
    fs::remove_dir_all(root).unwrap();
    outcome
}

#[test]
fn linux_extra_socket_and_io_uring_attempts_fail_closed() {
    let _guard = test_lock();
    let socket = custom_runner(
        r#"set +e
"$python" -I - <<'PY'
import errno
import socket
try:
    socket.socket(socket.AF_INET, socket.SOCK_STREAM)
except OSError as exc:
    raise SystemExit(90 if exc.errno == errno.EPERM else 91)
raise SystemExit(92)
PY
probe=$?
set -e
[[ "$probe" == 90 ]]"#,
        Duration::from_secs(5),
    );
    assert_eq!(socket.exit_code, 1);
    assert!(matches!(
        socket.verdict.failure_code(),
        Some(FailureCode::NetworkAttempt { .. })
    ));
    assert!(!String::from_utf8_lossy(&socket.public_stdout).contains("offline=true"));

    let io_uring = custom_runner(
        &format!(
            r#"set +e
"$python" -I - <<'PY'
import ctypes
import errno
libc = ctypes.CDLL(None, use_errno=True)
result = libc.syscall({}, 1, 0)
raise SystemExit(90 if result == -1 and ctypes.get_errno() == errno.EPERM else 91)
PY
probe=$?
set -e
[[ "$probe" == 90 ]]"#,
            libc::SYS_io_uring_setup
        ),
        Duration::from_secs(5),
    );
    assert_eq!(io_uring.exit_code, 1);
    assert!(matches!(
        io_uring.verdict.failure_code(),
        Some(FailureCode::NetworkAttempt { .. })
    ));
    assert!(!String::from_utf8_lossy(&io_uring.public_stdout).contains("offline=true"));
}

#[test]
fn linux_worker_marker_exit_zero_and_deadline_cannot_authorize_success() {
    let _guard = test_lock();
    let marker = custom_runner("printf 'forged offline=true\\n'", Duration::from_secs(5));
    assert_eq!(marker.exit_code, 1);
    assert!(marker.verdict.worker_stdout_has_final_marker);
    assert!(marker.public_stdout.is_empty());

    let exit_zero = custom_runner("exit 0", Duration::from_secs(5));
    assert_eq!(exit_zero.exit_code, 1);
    assert_ne!(
        exit_zero.verdict.worker_exit,
        WorkerExit::Code(SUPERVISOR_WORKER_EXIT_CODE)
    );
    assert!(exit_zero.public_stdout.is_empty());

    let deadline = custom_runner("sleep 3600 & wait", Duration::from_secs(1));
    assert_eq!(deadline.exit_code, 1);
    assert_eq!(
        deadline.verdict.failure_code(),
        Some(FailureCode::DeadlineExceeded)
    );
    assert_eq!(deadline.verdict.runtime_cleanup, CleanupState::Removed);
    assert!(deadline.public_stdout.is_empty());
}

#[test]
fn linux_output_overflow_fails_closed() {
    let _guard = test_lock();
    let overflow = custom_runner(
        r#""$python" -I - <<'PY'
import sys
sys.stdout.write("x" * 1048577)
PY"#,
        Duration::from_secs(5),
    );
    assert_eq!(overflow.exit_code, 1);
    assert_eq!(
        overflow.verdict.failure_code(),
        Some(FailureCode::OutputLimitExceeded)
    );
    assert!(overflow.public_stdout.is_empty());
    assert_eq!(overflow.verdict.runtime_cleanup, CleanupState::Removed);

    let stderr_overflow = custom_runner(
        r#""$python" -I - <<'PY'
import sys
sys.stderr.write("x" * 1048577)
PY"#,
        Duration::from_secs(5),
    );
    assert_eq!(stderr_overflow.exit_code, 1);
    assert_eq!(
        stderr_overflow.verdict.failure_code(),
        Some(FailureCode::OutputLimitExceeded)
    );
    assert_eq!(
        stderr_overflow.verdict.runtime_cleanup,
        CleanupState::Removed
    );
}

#[test]
fn linux_worker_cannot_reopen_or_write_unexpected_descriptors() {
    let _guard = test_lock();
    let inherited = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
        .unwrap();
    let inherited_fd = inherited.as_raw_fd();
    let fd_flags = unsafe { libc::fcntl(inherited_fd, libc::F_GETFD) };
    assert!(fd_flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(inherited_fd, libc::F_SETFD, fd_flags & !libc::FD_CLOEXEC) },
        0
    );
    let outcome = custom_runner(
        r#"for fd in $(seq 3 256); do
  if (printf 'forged' >&"$fd") 2>/dev/null; then exit 61; fi
done"#,
        Duration::from_secs(5),
    );
    assert_eq!(
        outcome.exit_code,
        0,
        "{}",
        String::from_utf8_lossy(&outcome.public_stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&outcome.public_stdout)
            .matches("offline=true")
            .count(),
        1
    );
    drop(inherited);
}

fn runtime_roots() -> BTreeSet<PathBuf> {
    fs::read_dir("/tmp")
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("kiana-capability-governance-"))
        })
        .collect()
}

#[test]
fn linux_sigint_and_sigterm_return_shell_codes_without_residue() {
    let _guard = test_lock();
    for (signal, expected) in [(libc::SIGINT, 130), (libc::SIGTERM, 143)] {
        let before = runtime_roots();
        let (root, _) = create_custom_runner("sleep 3600 & wait", "signal");
        let child = Command::new(supervisor_bin())
            .arg("fixture-shapes")
            .current_dir(&root)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        while runtime_roots().difference(&before).next().is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "supervisor did not create its runtime root"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(expected));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("offline=true"));
        assert_eq!(runtime_roots(), before);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn linux_term_resistant_descendants_are_killed_without_residue() {
    let _guard = test_lock();
    let before = runtime_roots();
    let outcome = custom_runner(
        "trap '' TERM; (trap '' TERM; sleep 3600) & wait",
        Duration::from_millis(100),
    );
    assert_eq!(outcome.exit_code, 1);
    assert_eq!(
        outcome.verdict.failure_code(),
        Some(FailureCode::DeadlineExceeded)
    );
    assert_eq!(outcome.verdict.runtime_cleanup, CleanupState::Removed);
    assert_eq!(runtime_roots(), before);
}
