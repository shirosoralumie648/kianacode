use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ChecksCommand;

#[async_trait]
impl Command for ChecksCommand {
    fn name(&self) -> &str {
        "checks"
    }

    fn description(&self) -> &str {
        "Show deterministic local quality gates"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = parse_checks_args(&context.args)?;
        if args.show_help {
            return Ok(CommandResult::text(usage()));
        }
        if !args.json_output {
            return Err(anyhow!(usage()));
        }

        let cwd = context_cwd(&context);
        if args.dry_run {
            let report = build_dry_run_report(&cwd);
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }

        let report = run_checks_report(&cwd)?;
        Ok(CommandResult::text(serde_json::to_string_pretty(&report)?))
    }
}

fn usage() -> &'static str {
    "Usage: kiana checks --json\n       kiana checks --dry-run --json"
}

fn parse_checks_args(raw: &str) -> Result<ChecksArgs> {
    let mut args = ChecksArgs::default();
    for token in raw.split_whitespace() {
        match token {
            "" => {}
            "help" | "--help" | "-h" => args.show_help = true,
            "--dry-run" | "dry-run" => args.dry_run = true,
            "--json" | "json" => args.json_output = true,
            _ => return Err(anyhow!(usage())),
        }
    }
    Ok(args)
}

fn build_dry_run_report(cwd: &Path) -> ChecksDryRunReport {
    let git_root =
        git_output(cwd, &["rev-parse", "--show-toplevel"]).map(|value| PathBuf::from(value.trim()));
    let root = git_root.as_deref().unwrap_or(cwd);
    let checks = discover_checks(root);

    ChecksDryRunReport {
        schema: "kiana.checks.dry_run.v1",
        root: cwd.to_string_lossy().to_string(),
        git_root: git_root.map(|path| path.to_string_lossy().to_string()),
        inside_git_repo: git_output(cwd, &["rev-parse", "--is-inside-work-tree"])
            .map(|value| value.trim() == "true")
            .unwrap_or(false),
        dry_run: true,
        checks,
    }
}

fn discover_checks(root: &Path) -> Vec<CheckPlan> {
    let mut checks = Vec::new();
    if root.join("Cargo.toml").is_file() {
        checks.push(CheckPlan::new(
            "rustfmt",
            "Rust formatting",
            "cargo fmt --all --check",
        ));
        checks.push(CheckPlan::new(
            "cargo_check",
            "Rust workspace compilation",
            "cargo check --workspace",
        ));
        checks.push(CheckPlan::new(
            "cargo_test",
            "Rust workspace tests",
            "cargo test --workspace --no-fail-fast",
        ));
    }
    if root.join("scripts").join("release-smoke.sh").is_file() {
        checks.push(CheckPlan::new(
            "release_smoke",
            "Release smoke gate",
            "bash scripts/release-smoke.sh",
        ));
    }
    checks
}

pub(crate) fn run_checks_report(cwd: &Path) -> Result<ChecksRunReport> {
    let git_root =
        git_output(cwd, &["rev-parse", "--show-toplevel"]).map(|value| PathBuf::from(value.trim()));
    let root = git_root.as_deref().unwrap_or(cwd);
    let checks = discover_checks(root);
    let isolation = if let Some(root) = git_root.as_deref() {
        create_isolated_worktree(root).transpose()?
    } else {
        None
    };
    let (run_root, execution) = if let Some(isolation) = isolation.as_ref() {
        (
            isolation.path.as_path(),
            CheckExecution {
                isolation: "git_worktree",
                applied_current_changes: isolation.applied_current_changes,
            },
        )
    } else {
        (
            root,
            CheckExecution {
                isolation: "none",
                applied_current_changes: false,
            },
        )
    };
    let results = checks
        .into_iter()
        .map(|check| run_check(run_root, check))
        .collect::<Vec<_>>();
    let summary = CheckSummary::from_results(&results);

    Ok(ChecksRunReport {
        schema: "kiana.checks.run.v1",
        root: cwd.to_string_lossy().to_string(),
        git_root: git_root.map(|path| path.to_string_lossy().to_string()),
        inside_git_repo: git_output(cwd, &["rev-parse", "--is-inside-work-tree"])
            .map(|value| value.trim() == "true")
            .unwrap_or(false),
        dry_run: false,
        execution,
        summary,
        results,
    })
}

fn run_check(root: &Path, check: CheckPlan) -> CheckResult {
    let parts = check.command.split_whitespace().collect::<Vec<_>>();
    let Some((program, args)) = parts.split_first() else {
        return CheckResult::skipped(check, "empty command");
    };
    let output = ProcessCommand::new(resolve_check_program(program))
        .current_dir(root)
        .args(args)
        .output();
    match output {
        Ok(output) => CheckResult {
            id: check.id,
            description: check.description,
            command: check.command,
            status: if output.status.success() {
                "passed"
            } else {
                "failed"
            },
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            error: None,
        },
        Err(error) => CheckResult {
            id: check.id,
            description: check.description,
            command: check.command,
            status: "failed",
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error.to_string()),
        },
    }
}

fn resolve_check_program(program: &str) -> String {
    if program != "bash" {
        return program.to_string();
    }
    preferred_bash_program()
}

#[cfg(windows)]
fn preferred_bash_program() -> String {
    for candidate in [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
    ] {
        if Path::new(candidate).is_file() {
            return candidate.to_string();
        }
    }
    "bash".to_string()
}

#[cfg(not(windows))]
fn preferred_bash_program() -> String {
    "bash".to_string()
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_output_bytes(cwd: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = ProcessCommand::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(output.stdout)
}

fn create_isolated_worktree(git_root: &Path) -> Option<Result<IsolatedChecksWorktree>> {
    let head = git_output(git_root, &["rev-parse", "HEAD"])?;
    Some(materialize_isolated_worktree(git_root, head.trim()))
}

fn materialize_isolated_worktree(git_root: &Path, head: &str) -> Result<IsolatedChecksWorktree> {
    let worktree_path = temp_worktree_path();
    let _ = fs::remove_dir_all(&worktree_path);
    run_git_checked(
        git_root,
        &[
            "worktree",
            "add",
            "--detach",
            "--force",
            "--quiet",
            path_as_str(&worktree_path)?,
            head,
        ],
        None,
    )?;
    let mut worktree = IsolatedChecksWorktree {
        path: worktree_path,
        git_root: git_root.to_path_buf(),
        applied_current_changes: false,
    };
    worktree.applied_current_changes = apply_current_changes(git_root, &worktree.path)?;
    Ok(worktree)
}

fn apply_current_changes(git_root: &Path, worktree: &Path) -> Result<bool> {
    let mut applied = false;
    for args in [
        &["diff", "--cached", "--binary"][..],
        &["diff", "--binary"][..],
    ] {
        let patch = git_output_bytes(git_root, args).unwrap_or_default();
        if patch.is_empty() {
            continue;
        }
        run_git_checked(worktree, &["apply", "--binary"], Some(&patch))?;
        applied = true;
    }

    for relative in untracked_files(git_root) {
        let Some(relative) = safe_relative_path(&relative) else {
            continue;
        };
        let source = git_root.join(&relative);
        if !source.is_file() {
            continue;
        }
        let target = worktree.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::copy(&source, &target).with_context(|| {
            format!(
                "failed to copy untracked file {} to isolated worktree {}",
                source.display(),
                target.display()
            )
        })?;
        applied = true;
    }

    Ok(applied)
}

fn untracked_files(git_root: &Path) -> Vec<PathBuf> {
    git_output_bytes(
        git_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .unwrap_or_default()
    .split(|byte| *byte == 0)
    .filter(|path| !path.is_empty())
    .map(|path| PathBuf::from(String::from_utf8_lossy(path).to_string()))
    .collect()
}

fn safe_relative_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => clean.push(part),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn temp_worktree_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "kiana-checks-worktree-{}-{}-{}",
        std::process::id(),
        now.as_secs(),
        now.subsec_nanos()
    ))
}

fn run_git_checked(cwd: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<()> {
    let mut command = ProcessCommand::new("git");
    command.current_dir(cwd).args(args);
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run git {:?}", args))?;
    if let Some(input) = stdin {
        let mut child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open git stdin"))?;
        child_stdin
            .write_all(input)
            .with_context(|| format!("failed to write git {:?} stdin", args))?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for git {:?}", args))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn path_as_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}

struct IsolatedChecksWorktree {
    path: PathBuf,
    git_root: PathBuf,
    applied_current_changes: bool,
}

impl Drop for IsolatedChecksWorktree {
    fn drop(&mut self) {
        if let Some(path) = self.path.to_str() {
            let _ = ProcessCommand::new("git")
                .current_dir(&self.git_root)
                .args(["worktree", "remove", "--force", path])
                .output();
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn context_cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Default)]
struct ChecksArgs {
    dry_run: bool,
    json_output: bool,
    show_help: bool,
}

#[derive(Debug, Serialize)]
struct ChecksDryRunReport {
    schema: &'static str,
    root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_root: Option<String>,
    inside_git_repo: bool,
    dry_run: bool,
    checks: Vec<CheckPlan>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChecksRunReport {
    pub(crate) schema: &'static str,
    pub(crate) root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) git_root: Option<String>,
    pub(crate) inside_git_repo: bool,
    pub(crate) dry_run: bool,
    pub(crate) execution: CheckExecution,
    pub(crate) summary: CheckSummary,
    pub(crate) results: Vec<CheckResult>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CheckExecution {
    pub(crate) isolation: &'static str,
    pub(crate) applied_current_changes: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct CheckSummary {
    pub(crate) total: usize,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) skipped: usize,
}

impl CheckSummary {
    fn from_results(results: &[CheckResult]) -> Self {
        Self {
            total: results.len(),
            passed: results
                .iter()
                .filter(|result| result.status == "passed")
                .count(),
            failed: results
                .iter()
                .filter(|result| result.status == "failed")
                .count(),
            skipped: results
                .iter()
                .filter(|result| result.status == "skipped")
                .count(),
        }
    }
}

#[derive(Debug, Serialize)]
struct CheckPlan {
    id: &'static str,
    description: &'static str,
    command: &'static str,
}

impl CheckPlan {
    fn new(id: &'static str, description: &'static str, command: &'static str) -> Self {
        Self {
            id,
            description,
            command,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CheckResult {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) command: &'static str,
    pub(crate) status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

impl CheckResult {
    fn skipped(check: CheckPlan, reason: &str) -> Self {
        Self {
            id: check.id,
            description: check.description,
            command: check.command,
            status: "skipped",
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(reason.to_string()),
        }
    }
}
