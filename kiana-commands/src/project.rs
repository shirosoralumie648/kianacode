use crate::tasks::{load_tasks, task_list_id};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_tasks::{
    build_project_board_at_root, select_next_project_task, ProjectBoardProjection,
    ProjectBoardStatus,
};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ProjectCommand;

#[async_trait]
impl Command for ProjectCommand {
    fn name(&self) -> &str {
        "project"
    }

    fn description(&self) -> &str {
        "Inspect the project board and select the next ready task"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("board") {
            "board" => project_board(&context, rest),
            "next" => project_next(&context, rest),
            "import-release-actions" => project_import_release_actions(&context, rest),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!(
                "unknown project command '{}'\n\n{}",
                other,
                usage()
            )),
        }
    }
}

fn project_board(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (json_output, task_list) = parse_project_args(rest)?;
    let task_list_id = task_list_id(context, task_list.as_deref());
    let tasks = load_tasks(context, task_list.as_deref())?;
    let board = build_project_board_at_root(project_root(context), task_list_id, &tasks)?;
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&board)?));
    }
    Ok(CommandResult::text(format_board(&board)))
}

fn project_next(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let (json_output, task_list) = parse_project_args(rest)?;
    let task_list_id = task_list_id(context, task_list.as_deref());
    let tasks = load_tasks(context, task_list.as_deref())?;
    let board = build_project_board_at_root(project_root(context), task_list_id, &tasks)?;
    let report = select_next_project_task(&board);
    if json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }

    let text = if let Some(task) = report.selected_task {
        format!(
            "Project next\ntask_list_id: {}\nselected_task: {}\ntitle: {}\nwhy: {}\nalternatives: {}",
            report.task_list_id,
            task.task_id,
            task.title,
            report.why.join(", "),
            report.alternatives.len()
        )
    } else {
        let primary_blocker = report
            .primary_blockers
            .first()
            .map(|blocker| {
                format!(
                    "{} count={} tasks={}",
                    blocker.code,
                    blocker.count,
                    blocker.task_ids.join(",")
                )
            })
            .unwrap_or_else(|| "-".to_string());
        format!(
            "Project next\ntask_list_id: {}\nselected_task: -\nreason: no ready tasks\nblocked: {}\nbacklog: {}\nprimary_blocker: {}",
            report.task_list_id,
            report.blocked_summary.get("blocked").copied().unwrap_or(0),
            report.blocked_summary.get("backlog").copied().unwrap_or(0),
            primary_blocker
        )
    };
    Ok(CommandResult::text(text))
}

#[derive(Debug, Default)]
struct ImportReleaseActionsArgs {
    json_output: bool,
    from: Option<PathBuf>,
    task_list: Option<String>,
}

fn project_import_release_actions(context: &CommandContext, rest: &str) -> Result<CommandResult> {
    let args = parse_import_release_actions_args(rest)?;
    let task_list_id = task_list_id(
        context,
        args.task_list.as_deref().or(Some("commercial-release")),
    );
    let report = read_release_action_plan_report(context, args.from.as_ref())?;
    let action_plan = report
        .get("action_plan")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("release blockers report is missing action_plan"))?;
    if action_plan.get("schema").and_then(Value::as_str)
        != Some("kiana.commercial-release-action-plan.v1")
    {
        return Err(anyhow!("release action_plan schema mismatch"));
    }

    let task_dir = project_tasks_dir(context, &task_list_id);
    fs::create_dir_all(&task_dir)
        .with_context(|| format!("failed to create {}", task_dir.display()))?;

    let mut imported = Vec::new();
    for action in release_actions(action_plan)? {
        let task = release_action_task(&action)?;
        let task_id = task
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("generated release action task is missing id"))?;
        let path = task_dir.join(format!("{}.json", sanitize_task_file_component(task_id)));
        write_json_atomic(&path, &task)?;
        imported.push(json!({
            "id": task_id,
            "path": path.strip_prefix(project_root(context)).unwrap_or(path.as_path()).to_string_lossy(),
            "owner": task.get("owner").cloned().unwrap_or(Value::Null),
            "resolution_scope": task.pointer("/metadata/resolution_scope").cloned().unwrap_or(Value::Null),
            "external": task.pointer("/metadata/external").cloned().unwrap_or(Value::Null)
        }));
    }

    let result = json!({
        "schema": "kiana.project-release-action-import.v1",
        "task_list_id": task_list_id,
        "tasks_dir": task_dir.to_string_lossy(),
        "source_schema": report.get("schema").cloned().unwrap_or(Value::Null),
        "action_plan_schema": action_plan.get("schema").cloned().unwrap_or(Value::Null),
        "imported": imported.len(),
        "tasks": imported
    });
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&result)?));
    }
    Ok(CommandResult::text(format!(
        "Project release action import\ntask_list_id: {}\nimported: {}\ntasks_dir: {}\nusage: kiana project board {}",
        result["task_list_id"].as_str().unwrap_or("commercial-release"),
        result["imported"].as_u64().unwrap_or(0),
        task_dir.display(),
        result["task_list_id"].as_str().unwrap_or("commercial-release")
    )))
}

fn parse_import_release_actions_args(rest: &str) -> Result<ImportReleaseActionsArgs> {
    let mut parsed = ImportReleaseActionsArgs::default();
    let mut parts = rest.split_whitespace().peekable();
    while let Some(part) = parts.next() {
        match part {
            "--json" | "-j" => parsed.json_output = true,
            "--from" => {
                let value = parts.next().ok_or_else(|| {
                    anyhow!("project import-release-actions --from requires a path")
                })?;
                parsed.from = Some(PathBuf::from(value));
            }
            value if value.starts_with("--from=") => {
                parsed.from = Some(PathBuf::from(value.trim_start_matches("--from=")));
            }
            value if value.starts_with('-') => {
                return Err(anyhow!(
                    "unknown project import-release-actions option '{}'\n\n{}",
                    value,
                    usage()
                ));
            }
            value if parsed.task_list.is_none() => parsed.task_list = Some(value.to_string()),
            _ => return Err(anyhow!("{}", usage())),
        }
    }
    Ok(parsed)
}

fn read_release_action_plan_report(
    context: &CommandContext,
    from: Option<&PathBuf>,
) -> Result<Value> {
    let text = if let Some(path) = from {
        let path = resolve_project_path(context, path);
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        let output = std::process::Command::new("bash")
            .arg("scripts/commercial-release-blockers-report.sh")
            .arg("--json")
            .current_dir(project_root(context))
            .output()
            .context("failed to run commercial release blockers report")?;
        if !output.status.success() {
            return Err(anyhow!(
                "commercial release blockers report failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        String::from_utf8(output.stdout)
            .context("commercial release blockers report was not valid UTF-8")?
    };
    serde_json::from_str(&text).context("release blockers report was not valid JSON")
}

fn release_actions(action_plan: &serde_json::Map<String, Value>) -> Result<Vec<Value>> {
    let mut actions = Vec::new();
    for key in ["local_actions", "external_actions"] {
        let Some(items) = action_plan.get(key).and_then(Value::as_array) else {
            return Err(anyhow!("release action_plan is missing {key}"));
        };
        actions.extend(items.iter().cloned());
    }
    Ok(actions)
}

fn release_action_task(action: &Value) -> Result<Value> {
    let action_id = required_action_string(action, "id")?;
    let title = required_action_string(action, "title")?;
    let owner = required_action_string(action, "owner")?;
    let resolution_scope = required_action_string(action, "resolution_scope")?;
    let external = action
        .get("external")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let required_action = required_action_string(action, "required_action")?;
    let verification_commands = string_array(action, "verification_commands")?;
    let acceptance_artifacts = string_array(action, "acceptance_artifacts")?;
    let paths = string_array(action, "paths")?;
    let env = string_array(action, "env")?;
    let handoff_notes = string_array(action, "handoff_notes")?;
    let task_id = format!("release-{}", sanitize_task_file_component(&action_id));
    Ok(json!({
        "id": task_id,
        "title": format!("Release blocker: {title}"),
        "status": "pending",
        "owner": owner,
        "board_status": if external { "review" } else { "spec" },
        "verification_commands": verification_commands,
        "allowed_paths": paths,
        "approval_required": external,
        "approval_status": if external { "required" } else { "local" },
        "blocks": [],
        "blockedBy": [],
        "created_at": 0,
        "updated_at": 0,
        "metadata": {
            "source": "release_action_plan",
            "source_action_id": action_id,
            "task_type": "commercial_release_blocker",
            "priority": if external { 80 } else { 100 },
            "resolution_scope": resolution_scope,
            "external": external,
            "required_action": required_action,
            "acceptance_artifacts": acceptance_artifacts,
            "env": env,
            "handoff_notes": handoff_notes,
            "release_blocker_reason": if external { "external release evidence required" } else { "local release automation blocker" },
            "unblock_condition": "complete required action and rerun kiana release blockers --json"
        }
    }))
}

fn required_action_string(action: &Value, key: &'static str) -> Result<String> {
    action
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("release action is missing {key}"))
}

fn string_array(action: &Value, key: &'static str) -> Result<Vec<String>> {
    let Some(items) = action.get(key).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!("release action {key} contains a non-string item"))
        })
        .collect()
}

fn project_tasks_dir(context: &CommandContext, task_list_id: &str) -> PathBuf {
    context
        .app_state
        .get("tasks_root")
        .or_else(|| context.app_state.get("tasksRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| resolve_project_path(context, &path))
        .or_else(|| std::env::var_os("KIANA_TASKS_ROOT").map(PathBuf::from))
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                project_root(context).join(path)
            }
        })
        .unwrap_or_else(|| project_root(context).join(".kiana").join("tasks"))
        .join(sanitize_task_file_component(task_list_id))
}

fn resolve_project_path(context: &CommandContext, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root(context).join(path)
    }
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("json.{}.{unique}.tmp", std::process::id()));
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(&temporary, bytes)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| {
        format!(
            "failed to replace task file {} from {}",
            path.display(),
            temporary.display()
        )
    })
}

fn parse_project_args(rest: &str) -> Result<(bool, Option<String>)> {
    let mut json_output = false;
    let mut task_list = None;
    for part in rest.split_whitespace() {
        match part {
            "--json" | "-j" => json_output = true,
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown project option '{}'\n\n{}", value, usage()));
            }
            value if task_list.is_none() => task_list = Some(value.to_string()),
            _ => return Err(anyhow!("{}", usage())),
        }
    }
    Ok((json_output, task_list))
}

fn sanitize_task_file_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "task".to_string()
    } else {
        sanitized
    }
}

fn format_board(board: &ProjectBoardProjection) -> String {
    let counts = ProjectBoardStatus::all_for_display()
        .into_iter()
        .map(|status| {
            let key = status.display_key();
            format!("{}={}", key, board.counts.get(key).copied().unwrap_or(0))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let ready = board
        .columns
        .iter()
        .find(|column| column.status == ProjectBoardStatus::Ready)
        .map(|column| format_tasks(&column.tasks, false))
        .unwrap_or_else(|| "-".to_string());
    let blocked = board
        .columns
        .iter()
        .find(|column| column.status == ProjectBoardStatus::Blocked)
        .map(|column| format_tasks(&column.tasks, true))
        .unwrap_or_else(|| "-".to_string());
    format!(
        "Project board\ntask_list_id: {}\n{}\nready:\n{}\nblocked:\n{}\npolicy_findings: {}\nusage: kiana project next [--json] [task_list_id]",
        board.task_list_id,
        counts,
        ready,
        blocked,
        board.policy_findings.len()
    )
}

fn format_tasks(tasks: &[kiana_tasks::ProjectTaskCard], include_blocker: bool) -> String {
    if tasks.is_empty() {
        return "-".to_string();
    }
    tasks
        .iter()
        .take(5)
        .map(|task| {
            if include_blocker {
                format!(
                    "- {} {} blocker={}",
                    task.task_id,
                    task.title,
                    task.blocker_reason.as_deref().unwrap_or("-")
                )
            } else {
                format!("- {} {}", task.task_id, task.title)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_word(input: &str) -> (Option<&str>, &str) {
    let input = input.trim();
    if input.is_empty() {
        return (None, "");
    }
    match input.find(char::is_whitespace) {
        Some(index) => (Some(&input[..index]), input[index..].trim()),
        None => (Some(input), ""),
    }
}

fn project_root(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf()))
}

fn usage() -> &'static str {
    "Usage:\n  kiana project board [--json] [task_list_id]\n  kiana project next [--json] [task_list_id]\n  kiana project import-release-actions [--json] [--from <report.json>] [task_list_id]"
}

#[cfg(test)]
mod tests {
    use super::ProjectCommand;
    use crate::local_state::env_lock;
    use crate::types::{Command, CommandContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-project-command-{}-{unique}",
            std::process::id()
        ))
    }

    fn context(args: &str, root: &Path) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                json!(root.to_string_lossy().to_string()),
            )]),
        }
    }

    fn write_task(root: &Path, id: &str, value: Value) {
        let dir = root.join(".kiana").join("tasks").join("default");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }

    fn write_passing_packet(root: &Path, task_id: &str) -> String {
        let run = kiana_tasks::initialize_workflow_run(
            root,
            kiana_tasks::WorkflowInit {
                request: "project command fixture".to_string(),
                input_kind: kiana_tasks::WorkflowInputKind::Qa,
                profile: kiana_tasks::WorkflowProfile::Gated,
                approval_required: false,
            },
        )
        .unwrap();
        let event = kiana_tasks::append_evidence_event(
            &run.artifact_dir,
            kiana_tasks::EvidenceEventDraft {
                event_id: None,
                workflow_id: run.workflow_id.clone(),
                run_id: run.run_id.clone(),
                task_id: Some(task_id.to_string()),
                workpacket_id: None,
                recorded_at_ms: None,
                kind: kiana_tasks::EvidenceKind::TestResult,
                status: kiana_tasks::EvidenceStatus::Pass,
                summary: "fixture passed".to_string(),
                source: kiana_tasks::EvidenceSource {
                    source_type: "local_command".to_string(),
                    name: "fixture".to_string(),
                    actor: Some("coordinator".to_string()),
                },
                payload: json!({"exit_code": 0}),
                changed_files: vec![],
                confidence: Some(1.0),
                severity: None,
                next_action: Some("review".to_string()),
                supersedes_event_id: None,
            },
        )
        .unwrap();
        let events = kiana_tasks::read_evidence_events(&run.artifact_dir).unwrap();
        let packet = kiana_tasks::build_verification_packet(
            &run.workflow_id,
            &run.run_id,
            Some(task_id.to_string()),
            "project_p0",
            vec![kiana_tasks::VerificationCheck {
                check_id: "fixture".to_string(),
                description: "fixture".to_string(),
                required: true,
                status: kiana_tasks::EvidenceStatus::Pass,
                evidence_id: Some(event.event_id),
                command: Some("cargo test".to_string()),
                reason: None,
            }],
            &events,
        )
        .unwrap();
        let path = kiana_tasks::write_verification_packet(&run.artifact_dir, &packet).unwrap();
        let relative = path.strip_prefix(root).unwrap().to_string_lossy();
        format!("verification:{relative}#{}", packet.verification_id)
    }

    fn column<'a>(board: &'a Value, status: &str) -> &'a Value {
        board["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|column| column["status"] == status)
            .unwrap_or_else(|| panic!("missing column {status}"))
    }

    #[tokio::test]
    async fn project_board_projects_existing_task_files_as_json() {
        let _env_guard = env_lock().lock().unwrap();
        let root = temp_root();
        let done_evidence = write_passing_packet(&root, "done");
        write_task(
            &root,
            "dependency",
            json!({
                "id": "dependency",
                "title": "Dependency",
                "status": "pending",
                "blocks": ["dependent"],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 10,
                "metadata": {"verification_commands": ["cargo test -p dependency"]}
            }),
        );
        write_task(
            &root,
            "dependent",
            json!({
                "id": "dependent",
                "title": "Dependent",
                "status": "pending",
                "blocks": [],
                "blockedBy": ["dependency"],
                "created_at": 1,
                "updated_at": 20,
                "metadata": {"verification_commands": ["cargo test -p dependent"]}
            }),
        );
        write_task(
            &root,
            "missing-evidence",
            json!({
                "id": "missing-evidence",
                "title": "Missing evidence",
                "status": "completed",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 30,
                "metadata": {}
            }),
        );
        write_task(
            &root,
            "done",
            json!({
                "id": "done",
                "title": "Done",
                "status": "completed",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 40,
                "metadata": {"evidence": [done_evidence]}
            }),
        );

        let result = ProjectCommand
            .execute(context("board --json", &root))
            .await
            .unwrap();
        let board: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(board["schema"], "kiana.project-board.v1");
        assert_eq!(board["task_list_id"], "default");
        assert_eq!(board["columns"].as_array().unwrap().len(), 8);
        assert_eq!(column(&board, "ready")["tasks"][0]["task_id"], "dependency");
        assert!(column(&board, "blocked")["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["task_id"] == "dependent"));
        assert!(column(&board, "blocked")["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["task_id"] == "missing-evidence"));
        assert_eq!(column(&board, "done")["tasks"][0]["task_id"], "done");

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn project_next_selects_highest_ranked_ready_task_as_json() {
        let root = temp_root();
        for (id, priority, blocks, updated_at) in [
            ("old", 10, json!(["one"]), 10),
            ("fanout", 10, json!(["one", "two"]), 20),
            ("lower", 9, json!(["one", "two", "three"]), 1),
        ] {
            write_task(
                &root,
                id,
                json!({
                    "id": id,
                    "title": id,
                    "status": "pending",
                    "blocks": blocks,
                    "blockedBy": [],
                    "created_at": 1,
                    "updated_at": updated_at,
                    "metadata": {
                        "priority": priority,
                        "verification_commands": [format!("cargo test -p {id}")]
                    }
                }),
            );
        }

        let result = ProjectCommand
            .execute(context("next --json", &root))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.project-next.v1");
        assert_eq!(report["selected_task"]["task_id"], "fanout");
        assert!(report["why"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "unblocks=2"));
        assert_eq!(report["alternatives"].as_array().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn project_import_release_actions_writes_project_board_tasks() {
        let root = temp_root();
        std::fs::create_dir_all(&root).unwrap();
        let report_path = root.join("commercial-blockers.json");
        std::fs::write(
            &report_path,
            serde_json::to_vec_pretty(&json!({
                "schema": "kiana.commercial-release-blockers.v1",
                "version": "0.1.0",
                "action_plan": {
                    "schema": "kiana.commercial-release-action-plan.v1",
                    "status": "blocked",
                    "total_actions": 2,
                    "local_actions": [
                        {
                            "id": "workflow.recovery-integrity",
                            "title": "Workflow recovery integrity proof is verified",
                            "owner": "local-release-automation",
                            "owner_status": "local-owner",
                            "resolution_scope": "local-automation",
                            "external": false,
                            "required_action": "Generate the release WorkflowRun proof.",
                            "acceptance_artifacts": ["dist/proofs/workflow/recovery-integrity.json"],
                            "verification_commands": ["kiana release workflow-proof --json --run-id <run_id>"],
                            "env": ["KIANA_RELEASE_WORKFLOW_RUN_ID"],
                            "paths": ["dist/proofs/workflow/recovery-integrity.json"],
                            "handoff_notes": ["Resolve locally before release owner review."]
                        }
                    ],
                    "external_actions": [
                        {
                            "id": "signing.release-artifacts",
                            "title": "Release artifacts are signed",
                            "owner": "release-security",
                            "owner_status": "role-owner-required",
                            "resolution_scope": "release-security",
                            "external": true,
                            "required_action": "Attach accepted signing proof.",
                            "acceptance_artifacts": ["dist/proofs/signing/release-signature.json"],
                            "verification_commands": ["bash scripts/sign-release-artifacts.sh"],
                            "env": ["KIANA_SIGNING_COMMAND"],
                            "paths": ["dist/proofs/signing/release-signature.json"],
                            "handoff_notes": ["Assign to release-security."]
                        }
                    ],
                    "actions_by_resolution_scope": {},
                    "next_verification": ["kiana release blockers --json"]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let result = ProjectCommand
            .execute(context(
                &format!(
                    "import-release-actions --json --from {} release-board",
                    report_path.display()
                ),
                &root,
            ))
            .await
            .unwrap();
        let import: Value = serde_json::from_str(&result.value).unwrap();
        assert_eq!(import["schema"], "kiana.project-release-action-import.v1");
        assert_eq!(import["task_list_id"], "release-board");
        assert_eq!(import["imported"], 2);

        let board = ProjectCommand
            .execute(context("board --json release-board", &root))
            .await
            .unwrap();
        let board: Value = serde_json::from_str(&board.value).unwrap();
        assert_eq!(board["task_list_id"], "release-board");
        let spec_tasks = column(&board, "spec")["tasks"].as_array().unwrap();
        assert!(spec_tasks
            .iter()
            .any(|task| task["task_id"] == "release-workflow-recovery-integrity"));
        let review_tasks = column(&board, "review")["tasks"].as_array().unwrap();
        assert!(review_tasks
            .iter()
            .any(|task| task["task_id"] == "release-signing-release-artifacts"));

        let task_path =
            root.join(".kiana/tasks/release-board/release-workflow-recovery-integrity.json");
        let task: Value = serde_json::from_slice(&std::fs::read(task_path).unwrap()).unwrap();
        assert_eq!(task["metadata"]["source"], "release_action_plan");
        assert_eq!(
            task["verification_commands"][0],
            "kiana release workflow-proof --json --run-id <run_id>"
        );
        assert_eq!(
            task["allowed_paths"][0],
            "dist/proofs/workflow/recovery-integrity.json"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn project_board_text_reports_counts_and_blockers() {
        let root = temp_root();
        write_task(
            &root,
            "ready",
            json!({
                "id": "ready",
                "title": "Ready task",
                "status": "pending",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1,
                "metadata": {"verification_commands": ["cargo test -p ready"]}
            }),
        );
        write_task(
            &root,
            "blocked",
            json!({
                "id": "blocked",
                "title": "Blocked task",
                "status": "failed",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 2,
                "metadata": {"blocker_reason": "targeted test failed"}
            }),
        );

        let result = ProjectCommand
            .execute(context("board", &root))
            .await
            .unwrap();

        assert!(result.value.contains("ready=1"));
        assert!(result.value.contains("blocked=1"));
        assert!(result.value.contains("- ready Ready task"));
        assert!(result
            .value
            .contains("- blocked Blocked task blocker=targeted test failed"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn project_next_text_reports_when_no_task_is_ready() {
        let root = temp_root();
        write_task(
            &root,
            "backlog",
            json!({
                "id": "backlog",
                "title": "Backlog task",
                "status": "pending",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1,
                "metadata": {}
            }),
        );
        write_task(
            &root,
            "failed",
            json!({
                "id": "failed",
                "title": "Failed task",
                "status": "failed",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 2,
                "metadata": {"blocker_reason": "verification failed"}
            }),
        );

        let result = ProjectCommand
            .execute(context("next", &root))
            .await
            .unwrap();

        assert!(result.value.contains("selected_task: -"));
        assert!(result.value.contains("reason: no ready tasks"));
        assert!(result.value.contains("blocked: 1"));
        assert!(result.value.contains("backlog: 1"));
        assert!(result
            .value
            .contains("primary_blocker: explicit_blocker count=1 tasks=failed"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn project_uses_canonical_task_list_id_and_rejects_extra_args() {
        let root = temp_root();
        let result = ProjectCommand
            .execute(context("board --json review/team", &root))
            .await
            .unwrap();
        let board: Value = serde_json::from_str(&result.value).unwrap();
        assert_eq!(board["task_list_id"], "review-team");

        for args in ["board one two", "next --unknown"] {
            let error = ProjectCommand
                .execute(context(args, &root))
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("Usage:"), "{args}: {error}");
        }

        let _ = std::fs::remove_dir_all(root);
    }
}
