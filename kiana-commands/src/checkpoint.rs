use crate::local_state::kiana_home_dir;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CheckpointCommand;

#[async_trait]
impl Command for CheckpointCommand {
    fn name(&self) -> &str {
        "checkpoint"
    }

    fn description(&self) -> &str {
        "Create a local git safety checkpoint"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let cwd = context_cwd(&context);
        let args = parse_checkpoint_args(&context.args)?;
        if args.help {
            return Ok(CommandResult::text(usage()));
        }
        if args.undo {
            if !args.last_assistant {
                return Err(anyhow!(usage()));
            }
            let report = undo_last_assistant_checkpoint(&cwd)?;
            if args.json_output {
                return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
            }
            return Ok(CommandResult::text(format_undo_report(&report)));
        }
        if let Some(target) = args.restore_target {
            let report = restore_checkpoint(&cwd, Path::new(&target))?;
            if args.json_output {
                return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
            }
            return Ok(CommandResult::text(format_restore_report(&report)));
        }

        let report = create_checkpoint(&cwd, &args)?;

        if args.json_output {
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }

        Ok(CommandResult::text(format!(
            "Checkpoint created\nid: {}\ndir: {}\ndirty: {}\nfiles: {}",
            report.id,
            report.checkpoint_dir,
            if report.dirty { "yes" } else { "no" },
            report.files.len()
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana checkpoint [--json]\n       kiana checkpoint [--assistant-turn --session-id <id> --turn-id <id>] [--json]\n       kiana checkpoint restore <checkpoint-dir-or-manifest> [--json]\n       kiana checkpoint undo --last-assistant [--json]"
}

fn parse_checkpoint_args(raw: &str) -> Result<CheckpointArgs> {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Ok(CheckpointArgs::default());
    }

    let mut args = CheckpointArgs::default();
    let mut index = 0;
    if matches!(tokens[index], "help" | "--help" | "-h") {
        args.help = true;
        return Ok(args);
    }
    if tokens[index] == "restore" {
        index += 1;
        let Some(target) = tokens.get(index) else {
            return Err(anyhow!(usage()));
        };
        if matches!(*target, "help" | "--help" | "-h") {
            args.help = true;
            return Ok(args);
        }
        args.restore_target = Some((*target).to_string());
        index += 1;
    } else if tokens[index] == "undo" {
        args.undo = true;
        index += 1;
    }

    while let Some(token) = tokens.get(index) {
        match *token {
            "--json" | "json" => args.json_output = true,
            "--assistant-turn" | "assistant-turn" => args.assistant_turn = true,
            "--last-assistant" | "last-assistant" => args.last_assistant = true,
            "--session-id" | "session-id" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(anyhow!(usage()));
                };
                args.session_id = Some((*value).to_string());
            }
            token if token.starts_with("--session-id=") => {
                let value = token.strip_prefix("--session-id=").unwrap_or_default();
                if value.is_empty() {
                    return Err(anyhow!(usage()));
                }
                args.session_id = Some(value.to_string());
            }
            "--turn-id" | "turn-id" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(anyhow!(usage()));
                };
                args.turn_id = Some((*value).to_string());
            }
            token if token.starts_with("--turn-id=") => {
                let value = token.strip_prefix("--turn-id=").unwrap_or_default();
                if value.is_empty() {
                    return Err(anyhow!(usage()));
                }
                args.turn_id = Some(value.to_string());
            }
            "help" | "--help" | "-h" => args.help = true,
            _ => return Err(anyhow!(usage())),
        }
        index += 1;
    }
    Ok(args)
}

fn create_checkpoint(cwd: &Path, args: &CheckpointArgs) -> Result<CheckpointReport> {
    let git_root = git_output(cwd, &["rev-parse", "--show-toplevel"])
        .ok_or_else(|| anyhow!("not inside a git repository"))?;
    let git_root = PathBuf::from(git_root.trim());
    let status = git_output(&git_root, &["status", "--short", "--untracked-files=all"])
        .context("failed to inspect git status")?;
    let files = parse_status_files(&status);
    let untracked_files = git_output_bytes(
        &git_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .context("failed to list untracked files")?;
    let untracked_files = parse_nul_paths(&untracked_files);
    let kind = if args.assistant_turn {
        if args.session_id.is_none() || args.turn_id.is_none() {
            return Err(anyhow!(
                "assistant-turn checkpoints require --session-id and --turn-id"
            ));
        }
        CheckpointKind::AssistantTurn
    } else {
        CheckpointKind::Manual
    };
    let created_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let id = checkpoint_id();
    let checkpoint_dir = checkpoint_repo_dir(&git_root).join(&id);
    fs::create_dir_all(&checkpoint_dir)
        .with_context(|| format!("failed to create {}", checkpoint_dir.display()))?;

    let staged_patch = write_patch(
        &git_root,
        &checkpoint_dir,
        "staged.diff",
        &["diff", "--cached", "--binary"],
    )?;
    let unstaged_patch = write_patch(
        &git_root,
        &checkpoint_dir,
        "unstaged.diff",
        &["diff", "--binary"],
    )?;
    copy_untracked_files(&git_root, &checkpoint_dir, &untracked_files)?;

    let manifest_path = checkpoint_dir.join("manifest.json");
    let report = CheckpointReport {
        id,
        root: cwd.to_string_lossy().to_string(),
        git_root: git_root.to_string_lossy().to_string(),
        checkpoint_dir: checkpoint_dir.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        inside_git_repo: true,
        dirty: !files.is_empty(),
        head: git_output(&git_root, &["rev-parse", "HEAD"]).map(|value| value.trim().to_string()),
        branch: git_output(&git_root, &["branch", "--show-current"])
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        kind,
        session_id: args.session_id.clone(),
        turn_id: args.turn_id.clone(),
        created_at_unix_ms,
        files,
        staged_patch,
        unstaged_patch,
        untracked_files,
        assistant_final_state: None,
    };
    fs::write(&manifest_path, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok(report)
}

pub fn create_assistant_turn_checkpoint(
    cwd: &Path,
    session_id: &str,
    turn_id: &str,
) -> Result<PathBuf> {
    let report = create_checkpoint(
        cwd,
        &CheckpointArgs {
            assistant_turn: true,
            session_id: Some(session_id.to_string()),
            turn_id: Some(turn_id.to_string()),
            ..CheckpointArgs::default()
        },
    )?;
    Ok(PathBuf::from(report.manifest_path))
}

pub fn record_assistant_turn_final_state(cwd: &Path, target: &Path) -> Result<()> {
    let manifest_path = checkpoint_manifest_path(target);
    let manifest = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let mut checkpoint: CheckpointReport = serde_json::from_str(&manifest)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    if checkpoint.kind != CheckpointKind::AssistantTurn {
        return Ok(());
    }
    let git_root = PathBuf::from(&checkpoint.git_root);
    let current_root = git_output(cwd, &["rev-parse", "--show-toplevel"])
        .ok_or_else(|| anyhow!("not inside a git repository"))?;
    if PathBuf::from(current_root.trim()) != git_root {
        return Err(anyhow!(
            "checkpoint belongs to {}, but current repository is {}",
            git_root.display(),
            current_root.trim()
        ));
    }

    let baseline = materialize_checkpoint_baseline(&checkpoint, &manifest_path)?;
    let checkpoint_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("checkpoint manifest has no parent directory"))?;
    let final_dir = checkpoint_dir.join("assistant-final");
    let _ = fs::remove_dir_all(&final_dir);
    fs::create_dir_all(&final_dir)
        .with_context(|| format!("failed to create {}", final_dir.display()))?;

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
    for path in paths {
        let Some(relative) = safe_relative_path(&path) else {
            continue;
        };
        let baseline_path = baseline.path.join(relative);
        let current_path = git_root.join(relative);
        if current_path.is_file() {
            if baseline_path.is_file() && files_equal(&baseline_path, &current_path) {
                continue;
            }
            let target = final_dir.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(&current_path, &target).with_context(|| {
                format!(
                    "failed to copy assistant final file {} to {}",
                    current_path.display(),
                    target.display()
                )
            })?;
            files.push(AssistantFinalFile { path, exists: true });
        } else if baseline_path.is_file() {
            files.push(AssistantFinalFile {
                path,
                exists: false,
            });
        }
    }

    checkpoint.assistant_final_state = Some(AssistantFinalState {
        created_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        files,
    });
    fs::write(&manifest_path, serde_json::to_string_pretty(&checkpoint)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok(())
}

fn undo_last_assistant_checkpoint(cwd: &Path) -> Result<CheckpointUndoReport> {
    let manifest_path = latest_assistant_checkpoint_manifest(cwd)?;
    undo_checkpoint_to_baseline(cwd, &manifest_path, "last_assistant")
}

fn undo_checkpoint_to_baseline(
    cwd: &Path,
    target: &Path,
    mode: &'static str,
) -> Result<CheckpointUndoReport> {
    let manifest_path = checkpoint_manifest_path(target);
    let manifest = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let checkpoint: CheckpointReport = serde_json::from_str(&manifest)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    let git_root = PathBuf::from(&checkpoint.git_root);
    let current_root = git_output(cwd, &["rev-parse", "--show-toplevel"])
        .ok_or_else(|| anyhow!("not inside a git repository"))?;
    if PathBuf::from(current_root.trim()) != git_root {
        return Err(anyhow!(
            "checkpoint belongs to {}, but current repository is {}",
            git_root.display(),
            current_root.trim()
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

    let mut report = CheckpointUndoReport {
        schema: "kiana.checkpoint.undo.v1",
        mode,
        root: cwd.to_string_lossy().to_string(),
        git_root: git_root.to_string_lossy().to_string(),
        checkpoint: UndoCheckpoint {
            id: checkpoint.id.clone(),
            manifest_path: manifest_path.to_string_lossy().to_string(),
            head: checkpoint.head.clone(),
            kind: checkpoint.kind,
            session_id: checkpoint.session_id.clone(),
            turn_id: checkpoint.turn_id.clone(),
            created_at_unix_ms: checkpoint.created_at_unix_ms,
        },
        changed: false,
        undone_files: Vec::new(),
        skipped: Vec::new(),
        conflicts: Vec::new(),
    };

    for path in paths {
        let Some(relative) = safe_relative_path(&path) else {
            report.skipped.push(UndoSkipped {
                path,
                reason: "unsafe_path".to_string(),
            });
            continue;
        };
        let baseline_path = baseline.path.join(relative);
        let current_path = git_root.join(relative);
        if let Some(reason) = assistant_final_conflict_reason(
            &checkpoint,
            &manifest_path,
            &path,
            &baseline_path,
            &current_path,
        )? {
            report.conflicts.push(UndoConflict { path, reason });
            continue;
        }
        undo_path_to_baseline(&path, &baseline_path, &current_path, &mut report)?;
    }

    report.changed = !report.undone_files.is_empty();
    Ok(report)
}

fn assistant_final_conflict_reason(
    checkpoint: &CheckpointReport,
    manifest_path: &Path,
    path: &str,
    baseline_path: &Path,
    current_path: &Path,
) -> Result<Option<String>> {
    let Some(final_state) = checkpoint.assistant_final_state.as_ref() else {
        return Ok(None);
    };
    let Some(final_file) = final_state.files.iter().find(|file| file.path == path) else {
        return Ok(None);
    };
    let Some(relative) = safe_relative_path(path) else {
        return Ok(None);
    };
    let checkpoint_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("checkpoint manifest has no parent directory"))?;
    let final_path = checkpoint_dir.join("assistant-final").join(relative);

    if final_file.exists {
        if current_path.is_file() && files_equal(&final_path, current_path) {
            return Ok(None);
        }
        if !baseline_path.exists() && !current_path.exists() {
            return Ok(None);
        }
        return Ok(Some("changed_after_assistant_turn".to_string()));
    }

    if current_path.exists() {
        return Ok(Some("changed_after_assistant_turn".to_string()));
    }
    Ok(None)
}

fn restore_checkpoint(cwd: &Path, target: &Path) -> Result<CheckpointRestoreReport> {
    let manifest_path = checkpoint_manifest_path(target);
    let manifest = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let checkpoint: CheckpointReport = serde_json::from_str(&manifest)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
    let checkpoint_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("checkpoint manifest has no parent directory"))?
        .to_path_buf();
    let git_root = PathBuf::from(&checkpoint.git_root);
    if !git_root.exists() {
        return Err(anyhow!(
            "checkpoint git root does not exist: {}",
            git_root.display()
        ));
    }
    let current_root = git_output(cwd, &["rev-parse", "--show-toplevel"])
        .ok_or_else(|| anyhow!("not inside a git repository"))?;
    if PathBuf::from(current_root.trim()) != git_root {
        return Err(anyhow!(
            "checkpoint belongs to {}, but current repository is {}",
            git_root.display(),
            current_root.trim()
        ));
    }

    let mut report = CheckpointRestoreReport {
        id: checkpoint.id.clone(),
        checkpoint_dir: checkpoint_dir.to_string_lossy().to_string(),
        git_root: git_root.to_string_lossy().to_string(),
        applied_patches: Vec::new(),
        restored_untracked: Vec::new(),
        skipped: Vec::new(),
        conflicts: Vec::new(),
    };

    restore_patch(
        &git_root,
        &checkpoint_dir.join("staged.diff"),
        "staged.diff",
        &mut report,
    )?;
    restore_patch(
        &git_root,
        &checkpoint_dir.join("unstaged.diff"),
        "unstaged.diff",
        &mut report,
    )?;
    restore_untracked_files(
        &git_root,
        &checkpoint_dir,
        &checkpoint.untracked_files,
        &mut report,
    )?;

    Ok(report)
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

fn latest_assistant_checkpoint_manifest(cwd: &Path) -> Result<PathBuf> {
    let git_root = git_output(cwd, &["rev-parse", "--show-toplevel"])
        .ok_or_else(|| anyhow!("not inside a git repository"))?;
    let git_root = PathBuf::from(git_root.trim());
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
        let Ok(checkpoint) = serde_json::from_str::<CheckpointReport>(&manifest_text) else {
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

pub(crate) fn checkpoint_repo_dir(git_root: &Path) -> PathBuf {
    kiana_home_dir()
        .join("checkpoints")
        .join(stable_path_key(git_root))
}

fn restore_patch(
    git_root: &Path,
    patch_path: &Path,
    patch_name: &str,
    report: &mut CheckpointRestoreReport,
) -> Result<()> {
    if !patch_path.is_file() {
        report.skipped.push(RestoreSkipped {
            path: patch_name.to_string(),
            reason: "missing_patch".to_string(),
        });
        return Ok(());
    }
    if fs::metadata(patch_path)
        .with_context(|| format!("failed to inspect {}", patch_path.display()))?
        .len()
        == 0
    {
        report.skipped.push(RestoreSkipped {
            path: patch_name.to_string(),
            reason: "empty_patch".to_string(),
        });
        return Ok(());
    }

    let check = run_git(
        git_root,
        &["apply", "--check", "--binary"],
        Some(patch_path),
    )?;
    if !check.status.success() {
        report.conflicts.push(RestoreConflict {
            path: patch_name.to_string(),
            reason: git_failure_reason(&check),
        });
        return Ok(());
    }
    let apply = run_git(git_root, &["apply", "--binary"], Some(patch_path))?;
    if !apply.status.success() {
        report.conflicts.push(RestoreConflict {
            path: patch_name.to_string(),
            reason: git_failure_reason(&apply),
        });
        return Ok(());
    }
    report.applied_patches.push(patch_name.to_string());
    Ok(())
}

fn restore_untracked_files(
    git_root: &Path,
    checkpoint_dir: &Path,
    paths: &[String],
    report: &mut CheckpointRestoreReport,
) -> Result<()> {
    for path in paths {
        let Some(relative) = safe_relative_path(path) else {
            report.skipped.push(RestoreSkipped {
                path: path.clone(),
                reason: "unsafe_path".to_string(),
            });
            continue;
        };
        let source = checkpoint_dir.join("untracked").join(relative);
        if !source.is_file() {
            report.skipped.push(RestoreSkipped {
                path: path.clone(),
                reason: "missing_artifact".to_string(),
            });
            continue;
        }
        let target = git_root.join(relative);
        if target.exists() {
            let source_bytes = fs::read(&source)
                .with_context(|| format!("failed to read {}", source.display()))?;
            let target_bytes = fs::read(&target)
                .with_context(|| format!("failed to read {}", target.display()))?;
            if source_bytes == target_bytes {
                report.skipped.push(RestoreSkipped {
                    path: path.clone(),
                    reason: "already_present".to_string(),
                });
            } else {
                report.conflicts.push(RestoreConflict {
                    path: path.clone(),
                    reason: "target_exists_with_different_content".to_string(),
                });
            }
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::copy(&source, &target).with_context(|| {
            format!(
                "failed to restore {} from {}",
                target.display(),
                source.display()
            )
        })?;
        report.restored_untracked.push(path.clone());
    }
    Ok(())
}

fn undo_path_to_baseline(
    path: &str,
    baseline_path: &Path,
    current_path: &Path,
    report: &mut CheckpointUndoReport,
) -> Result<()> {
    let baseline_exists = baseline_path.is_file();
    let current_exists = current_path.exists();
    if baseline_exists && current_exists && files_equal(baseline_path, current_path) {
        report.skipped.push(UndoSkipped {
            path: path.to_string(),
            reason: "unchanged".to_string(),
        });
        return Ok(());
    }

    if baseline_exists {
        if current_path.is_dir() {
            report.conflicts.push(UndoConflict {
                path: path.to_string(),
                reason: "current_path_is_directory".to_string(),
            });
            return Ok(());
        }
        if let Some(parent) = current_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::copy(baseline_path, current_path).with_context(|| {
            format!(
                "failed to restore {} from {}",
                current_path.display(),
                baseline_path.display()
            )
        })?;
        report.undone_files.push(UndoFile {
            path: path.to_string(),
            action: "restored".to_string(),
        });
        return Ok(());
    }

    if current_exists {
        if current_path.is_file() {
            fs::remove_file(current_path)
                .with_context(|| format!("failed to remove {}", current_path.display()))?;
            report.undone_files.push(UndoFile {
                path: path.to_string(),
                action: "removed".to_string(),
            });
        } else {
            report.conflicts.push(UndoConflict {
                path: path.to_string(),
                reason: "current_path_is_not_file".to_string(),
            });
        }
        return Ok(());
    }

    report.skipped.push(UndoSkipped {
        path: path.to_string(),
        reason: "already_absent".to_string(),
    });
    Ok(())
}

fn format_restore_report(report: &CheckpointRestoreReport) -> String {
    let mut lines = vec![
        format!("Checkpoint restored\nid: {}", report.id),
        format!("dir: {}", report.checkpoint_dir),
        format!("applied_patches: {}", report.applied_patches.len()),
        format!("restored_untracked: {}", report.restored_untracked.len()),
        format!("conflicts: {}", report.conflicts.len()),
    ];
    if !report.conflicts.is_empty() {
        lines.push("conflicted paths:".to_string());
        lines.extend(
            report
                .conflicts
                .iter()
                .map(|conflict| format!("- {}: {}", conflict.path, conflict.reason)),
        );
    }
    lines.join("\n")
}

fn format_undo_report(report: &CheckpointUndoReport) -> String {
    let mut lines = vec![
        format!("Checkpoint undo\nid: {}", report.checkpoint.id),
        format!("mode: {}", report.mode),
        format!("undone_files: {}", report.undone_files.len()),
        format!("conflicts: {}", report.conflicts.len()),
    ];
    if !report.conflicts.is_empty() {
        lines.push("conflicted paths:".to_string());
        lines.extend(
            report
                .conflicts
                .iter()
                .map(|conflict| format!("- {}: {}", conflict.path, conflict.reason)),
        );
    }
    lines.join("\n")
}

fn write_patch(
    git_root: &Path,
    checkpoint_dir: &Path,
    filename: &str,
    args: &[&str],
) -> Result<PatchArtifact> {
    let bytes = git_output_bytes(git_root, args).with_context(|| format!("git {:?}", args))?;
    let path = checkpoint_dir.join(filename);
    fs::write(&path, &bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(PatchArtifact {
        changed: !bytes.is_empty(),
        path: path.to_string_lossy().to_string(),
        bytes: bytes.len() as u64,
    })
}

fn copy_untracked_files(git_root: &Path, checkpoint_dir: &Path, paths: &[String]) -> Result<()> {
    let untracked_dir = checkpoint_dir.join("untracked");
    for path in paths {
        let Some(relative) = safe_relative_path(path) else {
            continue;
        };
        let source = git_root.join(relative);
        if !source.is_file() {
            continue;
        }
        let target = untracked_dir.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::copy(&source, &target).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let bytes = git_output_bytes(cwd, args)?;
    Some(String::from_utf8_lossy(&bytes).to_string())
}

fn git_output_bytes(cwd: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = run_git(cwd, args, None).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(output.stdout)
}

fn run_git(cwd: &Path, args: &[&str], input_file: Option<&Path>) -> Result<std::process::Output> {
    let mut command = ProcessCommand::new("git");
    command.current_dir(cwd).args(args);
    if let Some(path) = input_file {
        let file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        command.stdin(file);
    }
    command
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))
}

fn git_failure_reason(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        "git apply failed".to_string()
    } else {
        stdout
    }
}

fn materialize_checkpoint_baseline(
    checkpoint: &CheckpointReport,
    manifest_path: &Path,
) -> Result<CheckpointBaselineWorktree> {
    let head = checkpoint
        .head
        .as_deref()
        .ok_or_else(|| anyhow!("checkpoint has no HEAD; cannot build an undo baseline"))?;
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

    let baseline = CheckpointBaselineWorktree {
        path: baseline_path,
        git_root,
    };
    let checkpoint_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("checkpoint manifest has no parent directory"))?;
    apply_checkpoint_patch(&baseline.path, &checkpoint_dir.join("staged.diff"))?;
    apply_checkpoint_patch(&baseline.path, &checkpoint_dir.join("unstaged.diff"))?;
    copy_checkpoint_untracked_to_baseline(
        &baseline.path,
        checkpoint_dir,
        &checkpoint.untracked_files,
    )?;
    Ok(baseline)
}

fn temp_baseline_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "kiana-checkpoint-baseline-{}-{}-{}",
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

fn copy_checkpoint_untracked_to_baseline(
    baseline: &Path,
    checkpoint_dir: &Path,
    paths: &[String],
) -> Result<()> {
    for path in paths {
        let Some(relative) = safe_relative_path(path) else {
            continue;
        };
        let source = checkpoint_dir.join("untracked").join(relative);
        if !source.is_file() {
            continue;
        }
        let target = baseline.join(relative);
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
    let output = run_git(cwd, args, input_file)?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
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

fn checkpoint_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "cp-{}-{}-{}",
        now.as_secs(),
        now.subsec_nanos(),
        std::process::id()
    )
}

fn stable_path_key(path: &Path) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn parse_status_files(status: &str) -> Vec<CheckpointFile> {
    status
        .lines()
        .filter_map(parse_status_line)
        .collect::<Vec<_>>()
}

fn parse_status_line(line: &str) -> Option<CheckpointFile> {
    let mut chars = line.chars();
    let index = chars.next()?;
    let worktree = chars.next()?;
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(CheckpointFile {
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

fn parse_nul_paths(output: &[u8]) -> Vec<String> {
    output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect()
}

fn safe_relative_path(path: &str) -> Option<&Path> {
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
    Some(path)
}

#[derive(Debug, Default)]
struct CheckpointArgs {
    help: bool,
    json_output: bool,
    restore_target: Option<String>,
    undo: bool,
    last_assistant: bool,
    assistant_turn: bool,
    session_id: Option<String>,
    turn_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckpointKind {
    #[default]
    Manual,
    AssistantTurn,
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckpointReport {
    id: String,
    root: String,
    git_root: String,
    checkpoint_dir: String,
    manifest_path: String,
    inside_git_repo: bool,
    dirty: bool,
    head: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    kind: CheckpointKind,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    turn_id: Option<String>,
    #[serde(default)]
    created_at_unix_ms: u64,
    files: Vec<CheckpointFile>,
    staged_patch: PatchArtifact,
    unstaged_patch: PatchArtifact,
    untracked_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    assistant_final_state: Option<AssistantFinalState>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckpointFile {
    path: String,
    index: String,
    worktree: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PatchArtifact {
    changed: bool,
    path: String,
    bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AssistantFinalState {
    created_at_unix_ms: u64,
    files: Vec<AssistantFinalFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AssistantFinalFile {
    path: String,
    exists: bool,
}

#[derive(Debug, Serialize)]
struct CheckpointRestoreReport {
    id: String,
    checkpoint_dir: String,
    git_root: String,
    applied_patches: Vec<String>,
    restored_untracked: Vec<String>,
    skipped: Vec<RestoreSkipped>,
    conflicts: Vec<RestoreConflict>,
}

#[derive(Debug, Serialize)]
struct CheckpointUndoReport {
    schema: &'static str,
    mode: &'static str,
    root: String,
    git_root: String,
    checkpoint: UndoCheckpoint,
    changed: bool,
    undone_files: Vec<UndoFile>,
    skipped: Vec<UndoSkipped>,
    conflicts: Vec<UndoConflict>,
}

#[derive(Debug, Serialize)]
struct UndoCheckpoint {
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
struct UndoFile {
    path: String,
    action: String,
}

#[derive(Debug, Serialize)]
struct UndoSkipped {
    path: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct UndoConflict {
    path: String,
    reason: String,
}

struct CheckpointBaselineWorktree {
    path: PathBuf,
    git_root: PathBuf,
}

impl Drop for CheckpointBaselineWorktree {
    fn drop(&mut self) {
        let _ = ProcessCommand::new("git")
            .current_dir(&self.git_root)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Serialize)]
struct RestoreSkipped {
    path: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct RestoreConflict {
    path: String,
    reason: String,
}
