use crate::checkpoint::{checkpoint_repo_dir, CheckpointKind};
use crate::local_state::sdk_sessions_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DiffCommand;

#[async_trait]
impl Command for DiffCommand {
    fn name(&self) -> &str {
        "diff"
    }

    fn description(&self) -> &str {
        "Show diff"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = parse_diff_args(&context.args)?;
        if args.help {
            return Ok(CommandResult::text(usage()));
        }
        let cwd = context_cwd(&context);

        if let Some(session_id) = args.session_changes.as_deref() {
            if !args.json_output || args.from_checkpoint.is_some() || args.last_assistant {
                return Err(anyhow!(usage()));
            }
            let report = session_changes_report(session_id)?;
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }

        if args.last_assistant {
            if !args.json_output || args.from_checkpoint.is_some() {
                return Err(anyhow!(usage()));
            }
            let checkpoint = latest_assistant_checkpoint_manifest(&cwd)?;
            let report = diff_from_checkpoint(&cwd, &checkpoint)?;
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }

        if let Some(checkpoint) = args.from_checkpoint {
            if !args.json_output {
                return Err(anyhow!(usage()));
            }
            let report = diff_from_checkpoint(&cwd, Path::new(&checkpoint))?;
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }

        if !git_success(&cwd, &["rev-parse", "--is-inside-work-tree"]) {
            if args.json_output {
                return Ok(CommandResult::text(serde_json::to_string_pretty(
                    &DiffReport::not_git(cwd),
                )?));
            }
            return Ok(CommandResult::text("Not inside a git repository."));
        }

        let status = git_output(&cwd, &["status", "--short"]).unwrap_or_default();
        let stat = git_output(&cwd, &["diff", "--stat"]).unwrap_or_default();
        let staged = git_output(&cwd, &["diff", "--cached", "--stat"]).unwrap_or_default();
        if args.json_output {
            return Ok(CommandResult::text(serde_json::to_string_pretty(
                &DiffReport::from_git(cwd, &status, &staged, &stat),
            )?));
        }

        let mut lines = Vec::new();
        if status.trim().is_empty() {
            lines.push("Working tree clean.".to_string());
        } else {
            lines.push("Changed files:".to_string());
            lines.extend(status.lines().map(str::to_string));
        }

        if !staged.trim().is_empty() {
            lines.push(String::new());
            lines.push("Staged diff stat:".to_string());
            lines.push(staged.trim_end().to_string());
        }

        if !stat.trim().is_empty() {
            lines.push(String::new());
            lines.push("Unstaged diff stat:".to_string());
            lines.push(stat.trim_end().to_string());
        }

        Ok(CommandResult::text(lines.join("\n")))
    }
}

fn usage() -> &'static str {
    "Usage: kiana diff [--json]\n       kiana diff --from-checkpoint <checkpoint-dir-or-manifest> --json\n       kiana diff --last-assistant --json\n       kiana diff --session-changes <session-id> --json"
}

fn parse_diff_args(raw: &str) -> Result<DiffArgs> {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut args = DiffArgs::default();
    let mut index = 0;

    while let Some(token) = tokens.get(index) {
        match *token {
            "" => {}
            "help" | "--help" | "-h" => args.help = true,
            "--json" | "json" => args.json_output = true,
            "--last-assistant" | "last-assistant" => args.last_assistant = true,
            "--session-changes" | "session-changes" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(anyhow!(usage()));
                };
                validate_session_id(value)?;
                args.session_changes = Some((*value).to_string());
            }
            token if token.starts_with("--session-changes=") => {
                let value = token.strip_prefix("--session-changes=").unwrap_or_default();
                if value.is_empty() {
                    return Err(anyhow!(usage()));
                }
                validate_session_id(value)?;
                args.session_changes = Some(value.to_string());
            }
            "--from-checkpoint" | "from-checkpoint" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(anyhow!(usage()));
                };
                args.from_checkpoint = Some((*value).to_string());
            }
            token if token.starts_with("--from-checkpoint=") => {
                let value = token.strip_prefix("--from-checkpoint=").unwrap_or_default();
                if value.is_empty() {
                    return Err(anyhow!(usage()));
                }
                args.from_checkpoint = Some(value.to_string());
            }
            _ => return Err(anyhow!(usage())),
        }
        index += 1;
    }

    Ok(args)
}

fn session_changes_report(session_id: &str) -> Result<SessionChangesReport> {
    validate_session_id(session_id)?;
    let path = sdk_sessions_dir().join(session_id).join("events.jsonl");
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("session '{}' was not found", session_id))?;
    let mut files = BTreeMap::<String, SessionChangedFile>::new();
    let mut change_count = 0usize;

    for (line_index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: kiana_types::RuntimeEvent = serde_json::from_str(line).with_context(|| {
            format!(
                "failed to parse runtime event {} in {}",
                line_index + 1,
                path.display()
            )
        })?;
        if event.session_id != session_id {
            return Err(anyhow!(
                "runtime event {} in {} belongs to session '{}'",
                line_index + 1,
                path.display(),
                event.session_id
            ));
        }

        let kiana_types::RuntimeEventPayload::ToolResult(tool_result) = event.payload else {
            continue;
        };
        let Some(changed_files) = tool_result.changed_files else {
            continue;
        };
        collect_changed_files(&mut files, &mut change_count, &changed_files);
    }

    Ok(SessionChangesReport {
        schema: "kiana.diff.session_changes.v1",
        session_id: session_id.to_string(),
        changed: !files.is_empty(),
        file_count: files.len(),
        change_count,
        files: files.into_values().collect(),
    })
}

fn collect_changed_files(
    files: &mut BTreeMap<String, SessionChangedFile>,
    change_count: &mut usize,
    changed_files: &Value,
) {
    let Some(items) = changed_files.as_array() else {
        return;
    };

    for item in items {
        let Some(path) = item.get("path").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if path.is_empty() || safe_relative_path(path).is_none() {
            continue;
        }

        *change_count += 1;
        let entry = files
            .entry(path.to_string())
            .or_insert_with(|| SessionChangedFile {
                path: path.to_string(),
                operations: BTreeSet::new(),
                sources: BTreeSet::new(),
            });

        if let Some(operation) = item
            .get("operation")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            entry.operations.insert(operation.to_string());
        }
        if let Some(source) = item
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            entry.sources.insert(source.to_string());
        }
    }
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.trim().is_empty()
        || session_id.chars().any(char::is_whitespace)
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
    {
        return Err(anyhow!("session id contains invalid path characters"));
    }
    Ok(())
}

fn git_success(cwd: &Path, args: &[&str]) -> bool {
    ProcessCommand::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
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

fn diff_from_checkpoint(cwd: &Path, target: &Path) -> Result<DiffFromCheckpointReport> {
    let manifest_path = checkpoint_manifest_path(target);
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let checkpoint: CheckpointManifest = serde_json::from_str(&manifest_text)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    let git_root = current_git_root(cwd)?;
    if git_root != PathBuf::from(&checkpoint.git_root) {
        return Err(anyhow!(
            "checkpoint belongs to {}, but current repository is {}",
            checkpoint.git_root,
            git_root.display()
        ));
    }

    let baseline = materialize_checkpoint_baseline(&checkpoint, &manifest_path)?;
    let current_status =
        git_output(&git_root, &["status", "--short", "--untracked-files=all"]).unwrap_or_default();
    let mut paths = BTreeSet::new();
    paths.extend(checkpoint.files.iter().map(|file| file.path.clone()));
    paths.extend(checkpoint.untracked_files.iter().cloned());
    paths.extend(
        parse_status_files(&current_status)
            .into_iter()
            .map(|file| file.path),
    );

    let mut files = Vec::new();
    let mut patches = Vec::new();
    for path in paths {
        let Some(relative) = safe_relative_path(&path) else {
            continue;
        };
        let baseline_path = baseline.path.join(&relative);
        let current_path = git_root.join(&relative);
        let baseline_exists = baseline_path.exists();
        let current_exists = current_path.exists();
        if !baseline_exists && !current_exists {
            continue;
        }
        if files_equal(&baseline_path, &current_path) {
            continue;
        }
        let patch = no_index_diff_for_path(
            &path,
            &baseline_path,
            baseline_exists,
            &current_path,
            current_exists,
        )?;
        if patch.trim().is_empty() {
            continue;
        }
        files.push(DiffFromCheckpointFile {
            path: path.clone(),
            baseline_exists,
            current_exists,
        });
        patches.push(patch);
    }

    Ok(DiffFromCheckpointReport {
        schema: "kiana.diff.from_checkpoint.v1",
        root: cwd.to_string_lossy().to_string(),
        git_root: git_root.to_string_lossy().to_string(),
        inside_git_repo: true,
        checkpoint: DiffCheckpoint {
            id: checkpoint.id,
            manifest_path: manifest_path.to_string_lossy().to_string(),
            head: checkpoint.head,
            kind: checkpoint.kind,
            session_id: checkpoint.session_id,
            turn_id: checkpoint.turn_id,
            created_at_unix_ms: checkpoint.created_at_unix_ms,
        },
        changed: !files.is_empty(),
        files,
        patch: patches.join("\n"),
    })
}

fn latest_assistant_checkpoint_manifest(cwd: &Path) -> Result<PathBuf> {
    let git_root = current_git_root(cwd)?;
    let repo_dir = checkpoint_repo_dir(&git_root);
    let entries = match fs::read_dir(&repo_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(anyhow!(
                "no assistant-turn checkpoint found for {}",
                git_root.display()
            ));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", repo_dir.display()));
        }
    };

    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let Ok(manifest_text) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(checkpoint) = serde_json::from_str::<CheckpointManifest>(&manifest_text) else {
            continue;
        };
        if PathBuf::from(&checkpoint.git_root) != git_root {
            continue;
        }
        if checkpoint.kind != CheckpointKind::AssistantTurn {
            continue;
        }
        candidates.push((
            checkpoint.created_at_unix_ms,
            checkpoint.id.clone(),
            manifest_path,
        ));
    }

    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    candidates
        .pop()
        .map(|(_, _, manifest)| manifest)
        .ok_or_else(|| {
            anyhow!(
                "no assistant-turn checkpoint found for {}",
                git_root.display()
            )
        })
}

fn current_git_root(cwd: &Path) -> Result<PathBuf> {
    let git_root = git_output(cwd, &["rev-parse", "--show-toplevel"])
        .ok_or_else(|| anyhow!("not inside a git repository"))?;
    Ok(PathBuf::from(git_root.trim()))
}

fn checkpoint_manifest_path(target: &Path) -> PathBuf {
    if target
        .file_name()
        .is_some_and(|name| name == "manifest.json")
    {
        target.to_path_buf()
    } else {
        target.join("manifest.json")
    }
}

fn materialize_checkpoint_baseline(
    checkpoint: &CheckpointManifest,
    manifest_path: &Path,
) -> Result<BaselineWorktree> {
    let head = checkpoint
        .head
        .as_deref()
        .ok_or_else(|| anyhow!("checkpoint has no HEAD; cannot build a baseline diff"))?;
    let git_root = PathBuf::from(&checkpoint.git_root);
    let baseline_path = temp_baseline_path();
    let _ = fs::remove_dir_all(&baseline_path);
    let worktree = run_git_checked(
        &git_root,
        &[
            "worktree",
            "add",
            "--detach",
            "--force",
            "--quiet",
            baseline_path
                .to_str()
                .ok_or_else(|| anyhow!("baseline path is not valid UTF-8"))?,
            head,
        ],
        None,
    );
    if let Err(error) = worktree {
        let _ = fs::remove_dir_all(&baseline_path);
        return Err(error);
    }

    let baseline = BaselineWorktree {
        path: baseline_path,
        git_root,
    };
    let checkpoint_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("checkpoint manifest has no parent directory"))?;
    apply_checkpoint_patch(&baseline.path, &checkpoint_dir.join("staged.diff"))?;
    apply_checkpoint_patch(&baseline.path, &checkpoint_dir.join("unstaged.diff"))?;
    copy_checkpoint_untracked(&baseline.path, checkpoint_dir, &checkpoint.untracked_files)?;
    Ok(baseline)
}

fn temp_baseline_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "kiana-diff-baseline-{}-{}-{}",
        std::process::id(),
        now.as_secs(),
        now.subsec_nanos()
    ))
}

fn apply_checkpoint_patch(worktree: &Path, patch_path: &Path) -> Result<()> {
    if !patch_path.is_file() {
        return Ok(());
    }
    if fs::metadata(patch_path)
        .with_context(|| format!("failed to inspect {}", patch_path.display()))?
        .len()
        == 0
    {
        return Ok(());
    }
    run_git_checked(worktree, &["apply", "--binary"], Some(patch_path))?;
    Ok(())
}

fn copy_checkpoint_untracked(
    baseline: &Path,
    checkpoint_dir: &Path,
    paths: &[String],
) -> Result<()> {
    for path in paths {
        let Some(relative) = safe_relative_path(path) else {
            continue;
        };
        let source = checkpoint_dir.join("untracked").join(&relative);
        if !source.is_file() {
            continue;
        }
        let target = baseline.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::copy(&source, &target).with_context(|| {
            format!(
                "failed to copy checkpoint artifact {} to {}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

pub(crate) fn no_index_diff_for_path(
    relative_path: &str,
    baseline_path: &Path,
    baseline_exists: bool,
    current_path: &Path,
    current_exists: bool,
) -> Result<String> {
    let left = if baseline_exists {
        baseline_path
    } else {
        Path::new("/dev/null")
    };
    let right = if current_exists {
        current_path
    } else {
        Path::new("/dev/null")
    };
    let output = ProcessCommand::new("git")
        .args(["diff", "--no-index", "--binary", "--no-color", "--"])
        .arg(left)
        .arg(right)
        .output()
        .with_context(|| format!("failed to diff {}", relative_path))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(anyhow!(
            "git diff --no-index failed for {}: {}",
            relative_path,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let patch = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(normalize_no_index_patch(
        &patch,
        relative_path,
        baseline_path,
        baseline_exists,
        current_path,
        current_exists,
    ))
}

fn normalize_no_index_patch(
    patch: &str,
    relative_path: &str,
    baseline_path: &Path,
    baseline_exists: bool,
    current_path: &Path,
    current_exists: bool,
) -> String {
    let baseline_display = baseline_path.to_string_lossy();
    let current_display = current_path.to_string_lossy();
    patch
        .lines()
        .map(|line| {
            if line.starts_with("diff --git ") {
                format!("diff --git a/{relative_path} b/{relative_path}")
            } else if line == format!("--- {baseline_display}") {
                if baseline_exists {
                    format!("--- a/{relative_path}")
                } else {
                    "--- /dev/null".to_string()
                }
            } else if line == format!("+++ {current_display}") {
                if current_exists {
                    format!("+++ b/{relative_path}")
                } else {
                    "+++ /dev/null".to_string()
                }
            } else if line == "--- /dev/null" || line == "+++ /dev/null" {
                line.to_string()
            } else {
                line.replace(baseline_display.as_ref(), &format!("a/{relative_path}"))
                    .replace(current_display.as_ref(), &format!("b/{relative_path}"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn files_equal(left: &Path, right: &Path) -> bool {
    if !left.is_file() || !right.is_file() {
        return false;
    }
    match (fs::read(left), fs::read(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn run_git_checked(cwd: &Path, args: &[&str], input_file: Option<&Path>) -> Result<()> {
    let mut command = ProcessCommand::new("git");
    command.current_dir(cwd).args(args);
    if let Some(path) = input_file {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        command.stdin(file);
    }
    let output = command
        .output()
        .with_context(|| format!("failed to run git {:?}", args))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

fn safe_relative_path(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        return None;
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return None;
    }
    Some(path.to_path_buf())
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
struct DiffArgs {
    json_output: bool,
    help: bool,
    from_checkpoint: Option<String>,
    last_assistant: bool,
    session_changes: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiffReport {
    root: String,
    inside_git_repo: bool,
    dirty: bool,
    files: Vec<DiffFile>,
    staged: DiffSection,
    unstaged: DiffSection,
}

impl DiffReport {
    fn not_git(root: PathBuf) -> Self {
        Self {
            root: root.to_string_lossy().to_string(),
            inside_git_repo: false,
            dirty: false,
            files: Vec::new(),
            staged: DiffSection::default(),
            unstaged: DiffSection::default(),
        }
    }

    fn from_git(root: PathBuf, status: &str, staged_stat: &str, unstaged_stat: &str) -> Self {
        let files = parse_status_files(status);
        Self {
            root: root.to_string_lossy().to_string(),
            inside_git_repo: true,
            dirty: !files.is_empty(),
            files,
            staged: DiffSection::from_stat(staged_stat),
            unstaged: DiffSection::from_stat(unstaged_stat),
        }
    }
}

#[derive(Debug, Serialize)]
struct DiffFile {
    path: String,
    index: String,
    worktree: String,
}

#[derive(Debug, Default, Serialize)]
struct DiffSection {
    changed: bool,
    stat: String,
}

#[derive(Debug, Serialize)]
struct DiffFromCheckpointReport {
    schema: &'static str,
    root: String,
    git_root: String,
    inside_git_repo: bool,
    checkpoint: DiffCheckpoint,
    changed: bool,
    files: Vec<DiffFromCheckpointFile>,
    patch: String,
}

#[derive(Debug, Serialize)]
struct DiffCheckpoint {
    id: String,
    manifest_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<String>,
    kind: CheckpointKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    created_at_unix_ms: u64,
}

#[derive(Debug, Serialize)]
struct DiffFromCheckpointFile {
    path: String,
    baseline_exists: bool,
    current_exists: bool,
}

#[derive(Debug, Serialize)]
struct SessionChangesReport {
    schema: &'static str,
    session_id: String,
    changed: bool,
    file_count: usize,
    change_count: usize,
    files: Vec<SessionChangedFile>,
}

#[derive(Debug, Serialize)]
struct SessionChangedFile {
    path: String,
    operations: BTreeSet<String>,
    sources: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct CheckpointManifest {
    id: String,
    git_root: String,
    head: Option<String>,
    #[serde(default)]
    kind: CheckpointKind,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    created_at_unix_ms: u64,
    #[serde(default)]
    files: Vec<CheckpointManifestFile>,
    #[serde(default)]
    untracked_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CheckpointManifestFile {
    path: String,
}

struct BaselineWorktree {
    path: PathBuf,
    git_root: PathBuf,
}

impl Drop for BaselineWorktree {
    fn drop(&mut self) {
        let _ = ProcessCommand::new("git")
            .current_dir(&self.git_root)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl DiffSection {
    fn from_stat(stat: &str) -> Self {
        let stat = stat.trim_end().to_string();
        Self {
            changed: !stat.trim().is_empty(),
            stat,
        }
    }
}

fn parse_status_files(status: &str) -> Vec<DiffFile> {
    status
        .lines()
        .filter_map(parse_status_line)
        .collect::<Vec<_>>()
}

fn parse_status_line(line: &str) -> Option<DiffFile> {
    let mut chars = line.chars();
    let index = chars.next()?;
    let worktree = chars.next()?;
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(DiffFile {
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

#[cfg(test)]
mod tests {
    use super::DiffCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command as ProcessCommand;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn diff_rejects_extra_args_instead_of_ignoring_them() {
        let result = DiffCommand
            .execute(CommandContext {
                args: "README.md".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn diff_json_reports_dirty_state_from_command_cwd() {
        let _guard = env_lock().lock().unwrap();
        let root = fixture_root("dirty");
        run_git(&root, &["init"]);
        run_git(&root, &["config", "core.autocrlf", "false"]);
        run_git(&root, &["config", "core.eol", "lf"]);
        run_git(
            &root,
            &["config", "user.email", "kiana-tests@example.invalid"],
        );
        run_git(&root, &["config", "user.name", "Kiana Tests"]);
        fs::write(root.join("baseline.txt"), "baseline\n").unwrap();
        run_git(&root, &["add", "baseline.txt"]);
        run_git(&root, &["commit", "-m", "baseline"]);
        fs::write(root.join("staged.txt"), "staged\n").unwrap();
        run_git(&root, &["add", "staged.txt"]);
        fs::write(root.join("worktree.txt"), "worktree\n").unwrap();

        let result = DiffCommand
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["inside_git_repo"], true);
        assert_eq!(value["dirty"], true);
        assert_eq!(value["staged"]["changed"], true);
        assert_eq!(value["unstaged"]["changed"], false);
        let files = value["files"].as_array().unwrap();
        assert!(files.iter().any(|file| {
            file["path"] == "staged.txt" && file["index"] == "A" && file["worktree"] == " "
        }));
        assert!(files.iter().any(|file| {
            file["path"] == "worktree.txt" && file["index"] == "?" && file["worktree"] == "?"
        }));

        let root = value["root"].as_str().unwrap().to_string();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn diff_session_changes_json_aggregates_tool_result_changed_files() {
        let _guard = env_lock().lock().unwrap();
        let root = fixture_root("session-changes");
        let sessions_dir = root.join("sdk-sessions");
        let session_dir = sessions_dir.join("session-changes");
        fs::create_dir_all(&session_dir).unwrap();
        unsafe {
            std::env::set_var("KIANA_SDK_SESSIONS_DIR", &sessions_dir);
        }
        let first = kiana_types::RuntimeEvent::new(
            "event-1",
            "session-changes",
            "turn-1",
            None,
            1,
            "2026-07-05T00:00:01Z",
            kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                tool_call_id: "tool-1".to_string(),
                name: Some("Write".to_string()),
                workbench: None,
                is_error: false,
                content: json!({"ok": true}),
                error: None,
                changed_files: Some(json!([
                    {"path": "src/lib.rs", "operation": "create", "source": "Write"},
                    {"path": "README.md", "operation": "update", "source": "Edit"}
                ])),
            }),
        );
        let second = kiana_types::RuntimeEvent::new(
            "event-2",
            "session-changes",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-07-05T00:00:02Z",
            kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                tool_call_id: "tool-2".to_string(),
                name: Some("Edit".to_string()),
                workbench: None,
                is_error: false,
                content: json!({"ok": true}),
                error: None,
                changed_files: Some(json!([
                    {"path": "src/lib.rs", "operation": "update", "source": "Edit"}
                ])),
            }),
        );
        fs::write(
            session_dir.join("events.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::to_string(&first).unwrap(),
                serde_json::to_string(&second).unwrap()
            ),
        )
        .unwrap();

        let result = DiffCommand
            .execute(CommandContext {
                args: "--session-changes session-changes --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.diff.session_changes.v1");
        assert_eq!(value["session_id"], "session-changes");
        assert_eq!(value["changed"], true);
        assert_eq!(value["file_count"], 2);
        assert_eq!(value["change_count"], 3);
        assert_eq!(value["files"][0]["path"], "README.md");
        assert_eq!(value["files"][0]["operations"], json!(["update"]));
        assert_eq!(value["files"][1]["path"], "src/lib.rs");
        assert_eq!(value["files"][1]["operations"], json!(["create", "update"]));
        assert_eq!(value["files"][1]["sources"], json!(["Edit", "Write"]));

        unsafe {
            std::env::remove_var("KIANA_SDK_SESSIONS_DIR");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn diff_session_changes_json_returns_empty_report_for_legacy_events() {
        let _guard = env_lock().lock().unwrap();
        let root = fixture_root("session-changes-empty");
        let sessions_dir = root.join("sdk-sessions");
        let session_dir = sessions_dir.join("legacy-session");
        fs::create_dir_all(&session_dir).unwrap();
        unsafe {
            std::env::set_var("KIANA_SDK_SESSIONS_DIR", &sessions_dir);
        }
        let event = kiana_types::RuntimeEvent::new(
            "event-legacy",
            "legacy-session",
            "turn-1",
            None,
            1,
            "2026-07-05T00:00:03Z",
            kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                tool_call_id: "tool-legacy".to_string(),
                name: Some("Read".to_string()),
                workbench: None,
                is_error: false,
                content: json!("read-only result"),
                error: None,
                changed_files: None,
            }),
        );
        fs::write(
            session_dir.join("events.jsonl"),
            format!("{}\n", serde_json::to_string(&event).unwrap()),
        )
        .unwrap();

        let result = DiffCommand
            .execute(CommandContext {
                args: "--session-changes legacy-session --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.diff.session_changes.v1");
        assert_eq!(value["session_id"], "legacy-session");
        assert_eq!(value["changed"], false);
        assert_eq!(value["file_count"], 0);
        assert_eq!(value["change_count"], 0);
        assert_eq!(value["files"], json!([]));

        unsafe {
            std::env::remove_var("KIANA_SDK_SESSIONS_DIR");
        }
        let _ = fs::remove_dir_all(root);
    }

    fn fixture_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("kiana-diff-{name}-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout={}\nstderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
