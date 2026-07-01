use crate::checks::{run_checks_report, CheckResult, ChecksRunReport};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

pub struct ReviewCommand;

#[async_trait]
impl Command for ReviewCommand {
    fn name(&self) -> &str {
        "review"
    }

    fn description(&self) -> &str {
        "Prepare a deterministic local code review"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = parse_review_args(&context.args)?;
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

        let report = build_run_report(&cwd)?;
        Ok(CommandResult::text(serde_json::to_string_pretty(&report)?))
    }
}

fn usage() -> &'static str {
    "Usage: kiana review --json\n       kiana review --dry-run --json"
}

fn parse_review_args(raw: &str) -> Result<ReviewArgs> {
    let mut args = ReviewArgs::default();
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

fn build_dry_run_report(cwd: &Path) -> ReviewDryRunReport {
    let Some(git_root) = git_output(cwd, &["rev-parse", "--show-toplevel"]) else {
        return ReviewDryRunReport::not_git(cwd);
    };
    let git_root = PathBuf::from(git_root.trim());
    let status =
        git_output(&git_root, &["status", "--short", "--untracked-files=all"]).unwrap_or_default();
    let staged = git_output(&git_root, &["diff", "--cached", "--binary"]).unwrap_or_default();
    let unstaged = git_output(&git_root, &["diff", "--binary"]).unwrap_or_default();
    let mut files = parse_status_files(&status);
    files.sort_by(|left, right| left.path.cmp(&right.path));

    ReviewDryRunReport {
        schema: "kiana.review.dry_run.v1",
        root: cwd.to_string_lossy().to_string(),
        git_root: Some(git_root.to_string_lossy().to_string()),
        inside_git_repo: true,
        dry_run: true,
        dirty: !files.is_empty(),
        head: git_output(&git_root, &["rev-parse", "HEAD"]).map(|value| value.trim().to_string()),
        branch: git_output(&git_root, &["branch", "--show-current"])
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        planned_steps: vec![
            "create_isolated_worktree",
            "apply_current_patch",
            "run_configured_checks",
            "produce_review_findings",
            "discard_isolated_worktree",
        ],
        files,
        patches: ReviewPatches {
            staged: PatchPreview::from_text(staged),
            unstaged: PatchPreview::from_text(unstaged),
        },
    }
}

fn build_run_report(cwd: &Path) -> Result<ReviewRunReport> {
    let dry_run = build_dry_run_report(cwd);
    let checks = run_checks_report(cwd)?;
    let findings = findings_from_checks(&checks.results);

    Ok(ReviewRunReport {
        schema: "kiana.review.run.v1",
        root: dry_run.root,
        git_root: dry_run.git_root,
        inside_git_repo: dry_run.inside_git_repo,
        dry_run: false,
        dirty: dry_run.dirty,
        head: dry_run.head,
        branch: dry_run.branch,
        files: dry_run.files,
        patches: dry_run.patches,
        checks,
        findings,
    })
}

fn findings_from_checks(results: &[CheckResult]) -> Vec<ReviewFinding> {
    results
        .iter()
        .filter(|result| result.status == "failed" || result.status == "skipped")
        .map(|result| ReviewFinding {
            kind: if result.status == "failed" {
                "check_failed"
            } else {
                "check_skipped"
            },
            severity: if result.status == "failed" {
                "error"
            } else {
                "warning"
            },
            check_id: result.id,
            message: format!("{}: {}", result.description, result.command),
        })
        .collect()
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

fn context_cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn parse_status_files(status: &str) -> Vec<ReviewFile> {
    status
        .lines()
        .filter_map(parse_status_line)
        .collect::<Vec<_>>()
}

fn parse_status_line(line: &str) -> Option<ReviewFile> {
    let mut chars = line.chars();
    let index = chars.next()?;
    let worktree = chars.next()?;
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(ReviewFile {
        path: parse_status_path(path),
        index: index.to_string(),
        worktree: worktree.to_string(),
    })
}

fn parse_status_path(path: &str) -> String {
    path.rsplit_once(" -> ")
        .map(|(_, new_path)| new_path)
        .unwrap_or(path)
        .trim_matches('"')
        .to_string()
}

#[derive(Debug, Default)]
struct ReviewArgs {
    dry_run: bool,
    json_output: bool,
    show_help: bool,
}

#[derive(Debug, Serialize)]
struct ReviewDryRunReport {
    schema: &'static str,
    root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_root: Option<String>,
    inside_git_repo: bool,
    dry_run: bool,
    dirty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    planned_steps: Vec<&'static str>,
    files: Vec<ReviewFile>,
    patches: ReviewPatches,
}

#[derive(Debug, Serialize)]
struct ReviewRunReport {
    schema: &'static str,
    root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_root: Option<String>,
    inside_git_repo: bool,
    dry_run: bool,
    dirty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    files: Vec<ReviewFile>,
    patches: ReviewPatches,
    checks: ChecksRunReport,
    findings: Vec<ReviewFinding>,
}

#[derive(Debug, Serialize)]
struct ReviewFinding {
    kind: &'static str,
    severity: &'static str,
    check_id: &'static str,
    message: String,
}

impl ReviewDryRunReport {
    fn not_git(cwd: &Path) -> Self {
        Self {
            schema: "kiana.review.dry_run.v1",
            root: cwd.to_string_lossy().to_string(),
            git_root: None,
            inside_git_repo: false,
            dry_run: true,
            dirty: false,
            head: None,
            branch: None,
            planned_steps: Vec::new(),
            files: Vec::new(),
            patches: ReviewPatches::default(),
        }
    }
}

#[derive(Debug, Serialize)]
struct ReviewFile {
    path: String,
    index: String,
    worktree: String,
}

#[derive(Debug, Default, Serialize)]
struct ReviewPatches {
    staged: PatchPreview,
    unstaged: PatchPreview,
}

#[derive(Debug, Default, Serialize)]
struct PatchPreview {
    changed: bool,
    bytes: u64,
    text: String,
}

impl PatchPreview {
    fn from_text(text: String) -> Self {
        Self {
            changed: !text.is_empty(),
            bytes: text.len() as u64,
            text,
        }
    }
}
