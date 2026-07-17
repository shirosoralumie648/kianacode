use crate::tasks::{inspect_swarm_recovery_integrity, load_tasks, task_list_id};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_tasks::{
    build_project_board_at_root, inspect_workflow_integrity, list_verification_packets,
    list_workflow_runs, read_evidence_events, read_workflow_state, resume_workflow_run,
    summarize_evidence, unresolved_blocking_evidence, validate_verification_packet_completion,
    validate_verification_packet_integrity, EvidenceKind, ProjectBoardProjection,
    ProjectBoardStatus, VerificationPacket, WorkflowError, WorkflowResumeReport,
    WorkflowResumeStatus, WorkflowRunSummary,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ReportCommand;

#[async_trait]
impl Command for ReportCommand {
    fn name(&self) -> &str {
        "report"
    }

    fn description(&self) -> &str {
        "Build evidence-backed project progress reports"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let (subcommand, rest) = split_word(context.args.trim());
        match subcommand.unwrap_or("progress") {
            "progress" => report_progress(&context, rest),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown report command '{other}'\n\n{}", usage())),
        }
    }
}

#[derive(Debug, Default)]
struct ProgressArgs {
    json_output: bool,
    workflow_id: Option<String>,
    task_list_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ProgressBlocker {
    code: String,
    severity: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProgressReport {
    schema: &'static str,
    workflow: WorkflowRunSummary,
    board: ProjectBoardProjection,
    evidence: ProgressEvidenceReport,
    latest_verification: Option<VerificationPacket>,
    blockers: Vec<ProgressBlocker>,
    next_action: String,
}

#[derive(Debug, Serialize)]
struct ProgressEvidenceReport {
    status: &'static str,
    event_count: usize,
    latest_sequence: u64,
    counts_by_status: BTreeMap<String, usize>,
    counts_by_kind: BTreeMap<String, usize>,
}

impl ProgressEvidenceReport {
    fn ready(events: &[kiana_tasks::EvidenceEvent]) -> Self {
        let summary = summarize_evidence(events);
        Self {
            status: "ready",
            event_count: summary.event_count,
            latest_sequence: summary.latest_sequence,
            counts_by_status: summary.counts_by_status,
            counts_by_kind: summary.counts_by_kind,
        }
    }

    fn blocked() -> Self {
        Self {
            status: "blocked",
            event_count: 0,
            latest_sequence: 0,
            counts_by_status: BTreeMap::new(),
            counts_by_kind: BTreeMap::new(),
        }
    }
}

fn report_progress(context: &CommandContext, raw: &str) -> Result<CommandResult> {
    let args = parse_progress_args(raw)?;
    let root = context_cwd(context);
    let workflow = resolve_workflow(&root, args.workflow_id.as_deref())?;
    let mut blockers = Vec::new();
    let workflow_integrity_ready = match inspect_workflow_integrity(&workflow.artifact_dir) {
        Ok(_) => true,
        Err(error) => {
            blockers.push(ProgressBlocker {
                code: "workflow_integrity_invalid".to_string(),
                severity: "blocking".to_string(),
                message: error.to_string(),
                task_id: None,
            });
            false
        }
    };
    if let Err(error) = inspect_swarm_recovery_integrity(&workflow.artifact_dir) {
        blockers.push(ProgressBlocker {
            code: "workflow_recovery_integrity".to_string(),
            severity: "blocking".to_string(),
            message: error.to_string(),
            task_id: None,
        });
    }
    let resume = match resume_workflow_run(&root, Some(&workflow.run_id)) {
        Ok(report) => Some(report),
        Err(error) => {
            blockers.push(ProgressBlocker {
                code: "workflow_resume_failed".to_string(),
                severity: "blocking".to_string(),
                message: error.to_string(),
                task_id: None,
            });
            None
        }
    };
    let explicit_task_list = args.task_list_id.as_deref();
    let tasks = load_tasks(context, explicit_task_list)?;
    let board =
        build_project_board_at_root(&root, task_list_id(context, explicit_task_list), &tasks)?;
    append_board_blockers(&mut blockers, resume.as_ref(), &board);
    let (evidence, evidence_events) = match read_evidence_events(&workflow.artifact_dir) {
        Ok(events) => (ProgressEvidenceReport::ready(&events), Some(events)),
        Err(error) => {
            blockers.push(ProgressBlocker {
                code: "evidence_ledger_unreadable".to_string(),
                severity: "blocking".to_string(),
                message: error.to_string(),
                task_id: None,
            });
            (ProgressEvidenceReport::blocked(), None)
        }
    };
    if let Some(events) = evidence_events.as_deref() {
        blockers.extend(
            unresolved_blocking_evidence(events, &workflow.workflow_id, &workflow.run_id)
                .into_iter()
                .map(|event| ProgressBlocker {
                    code: match event.kind {
                        EvidenceKind::ReviewFinding => "evidence_review_unresolved".to_string(),
                        _ => "evidence_blocker_unresolved".to_string(),
                    },
                    severity: "blocking".to_string(),
                    message: event.summary,
                    task_id: event.task_id,
                }),
        );
    }
    let latest_verification = match list_verification_packets(&workflow.artifact_dir) {
        Ok(packets) => {
            let mut latest = None;
            for (path, packet) in packets.into_iter().rev() {
                let Some(events) = evidence_events.as_deref() else {
                    break;
                };
                if let Err(error) = validate_verification_packet_integrity(&packet, events) {
                    blockers.push(ProgressBlocker {
                        code: "verification_packet_invalid".to_string(),
                        severity: "blocking".to_string(),
                        message: format!("{}: {error}", path.display()),
                        task_id: packet.task_id.clone(),
                    });
                    continue;
                }
                if let Err(error) =
                    validate_verification_packet_completion(&workflow.artifact_dir, &path, &packet)
                {
                    blockers.push(ProgressBlocker {
                        code: "verification_packet_orphaned".to_string(),
                        severity: "blocking".to_string(),
                        message: format!("{}: {error}", path.display()),
                        task_id: packet.task_id.clone(),
                    });
                    continue;
                }
                if workflow_integrity_ready {
                    latest = Some(packet);
                }
                break;
            }
            latest
        }
        Err(error) => {
            blockers.push(ProgressBlocker {
                code: "verification_packets_unreadable".to_string(),
                severity: "blocking".to_string(),
                message: error.to_string(),
                task_id: None,
            });
            None
        }
    };
    let resume_action = resume
        .as_ref()
        .map(|report| report.recommended_action.as_str())
        .unwrap_or("repair:reconcile-eventlog");
    let next_action = select_next_action(
        resume_action,
        &board,
        latest_verification.as_ref(),
        &blockers,
    );
    let report = ProgressReport {
        schema: "kiana.progress-report.v1",
        workflow,
        board,
        evidence,
        latest_verification,
        blockers,
        next_action,
    };

    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format_progress_report(&report)))
}

fn append_board_blockers(
    blockers: &mut Vec<ProgressBlocker>,
    resume: Option<&WorkflowResumeReport>,
    board: &ProjectBoardProjection,
) {
    if resume.is_some_and(|report| report.resume_status == WorkflowResumeStatus::Blocked) {
        let report = resume.expect("blocked resume report must exist");
        blockers.push(ProgressBlocker {
            code: "workflow_resume_blocked".to_string(),
            severity: "blocking".to_string(),
            message: report
                .blocker
                .clone()
                .unwrap_or_else(|| "workflow state cannot be resumed safely".to_string()),
            task_id: None,
        });
    }

    if let Some(column) = board
        .columns
        .iter()
        .find(|column| column.status == ProjectBoardStatus::Blocked)
    {
        blockers.extend(column.tasks.iter().map(|task| ProgressBlocker {
            code: "task_blocked".to_string(),
            severity: "blocking".to_string(),
            message:
                task.blocker_reason.clone().unwrap_or_else(|| {
                    "task is blocked by project policy or dependency".to_string()
                }),
            task_id: Some(task.task_id.clone()),
        }));
    }

    blockers.extend(board.policy_findings.iter().map(|finding| ProgressBlocker {
        code: finding.code.clone(),
        severity: finding.severity.clone(),
        message: finding.message.clone(),
        task_id: Some(finding.task_id.clone()),
    }));
}

fn select_next_action(
    resume_action: &str,
    board: &ProjectBoardProjection,
    latest_verification: Option<&VerificationPacket>,
    blockers: &[ProgressBlocker],
) -> String {
    if !blockers.is_empty() {
        return if resume_action.trim().is_empty() {
            "resolve_blockers".to_string()
        } else {
            resume_action.to_string()
        };
    }
    if let Some(task_id) = board.ready_task_ids.first() {
        return format!("execute_task:{task_id}");
    }
    if let Some(packet) = latest_verification {
        if !packet.next_action.trim().is_empty() {
            return packet.next_action.clone();
        }
    }
    if !resume_action.trim().is_empty() {
        return resume_action.to_string();
    }
    "select_next_goal".to_string()
}

fn parse_progress_args(raw: &str) -> Result<ProgressArgs> {
    let mut args = ProgressArgs::default();
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            "--json" | "-j" => args.json_output = true,
            "--workflow" => {
                index += 1;
                set_progress_option_once(
                    &mut args.workflow_id,
                    required_value(&tokens, index, "--workflow")?,
                    "--workflow",
                )?;
            }
            "--task-list" => {
                index += 1;
                set_progress_option_once(
                    &mut args.task_list_id,
                    required_value(&tokens, index, "--task-list")?,
                    "--task-list",
                )?;
            }
            token if token.starts_with("--workflow=") => {
                set_progress_option_once(
                    &mut args.workflow_id,
                    value_after_equals(token, "--workflow")?,
                    "--workflow",
                )?;
            }
            token if token.starts_with("--task-list=") => {
                set_progress_option_once(
                    &mut args.task_list_id,
                    value_after_equals(token, "--task-list")?,
                    "--task-list",
                )?;
            }
            _ => return Err(anyhow!(usage())),
        }
        index += 1;
    }
    Ok(args)
}

fn set_progress_option_once(slot: &mut Option<String>, value: String, option: &str) -> Result<()> {
    if slot.is_some() {
        return Err(anyhow!("duplicate option {option}\n\n{}", usage()));
    }
    *slot = Some(value);
    Ok(())
}

fn required_value(tokens: &[&str], index: usize, option: &str) -> Result<String> {
    tokens
        .get(index)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && !value.starts_with("--"))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("missing value for {option}\n\n{}", usage()))
}

fn value_after_equals(token: &str, option: &str) -> Result<String> {
    token
        .split_once('=')
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("missing value for {option}\n\n{}", usage()))
}

fn resolve_workflow(root: &Path, requested_run_id: Option<&str>) -> Result<WorkflowRunSummary> {
    let runs = match list_workflow_runs(root) {
        Ok(runs) => runs,
        Err(_) => report_fallback_workflow_runs(root)?,
    };
    if let Some(run_id) = requested_run_id {
        return runs
            .into_iter()
            .find(|run| run.run_id == run_id)
            .ok_or_else(|| anyhow!("workflow_not_found: workflow run '{run_id}' was not found"));
    }
    runs.into_iter().next().ok_or_else(|| {
        anyhow!(
            "workflow_not_found: initialize a workflow first with 'kiana tasks workflow init <request>'"
        )
    })
}

fn report_fallback_workflow_runs(root: &Path) -> Result<Vec<WorkflowRunSummary>> {
    let workflows_dir = root.join(".kiana/workflows");
    if !workflows_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(workflows_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let artifact_dir = entry.path();
        let directory_run_id = entry.file_name().to_string_lossy().to_string();
        let state = read_workflow_state(&artifact_dir).map_err(map_workflow_error)?;
        runs.push(WorkflowRunSummary {
            schema: "kiana.workflow-run-summary.v1".to_string(),
            workflow_id: state.workflow_id,
            run_id: directory_run_id,
            status: state.status,
            current_node: state.current_node,
            request: state.request,
            input_kind: state.input_kind,
            profile: state.profile,
            approval_required: state.approval_required,
            artifact_dir,
            created_at_ms: state.created_at_ms,
            updated_at_ms: state.updated_at_ms,
        });
    }
    runs.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| right.run_id.cmp(&left.run_id))
    });
    Ok(runs)
}

fn map_workflow_error(error: WorkflowError) -> anyhow::Error {
    match error {
        WorkflowError::NoRuns | WorkflowError::RunNotFound(_) => anyhow!(
            "workflow_not_found: initialize a workflow first with 'kiana tasks workflow init <request>'"
        ),
        other => anyhow!("workflow_state_unreadable: {other}"),
    }
}

fn format_progress_report(report: &ProgressReport) -> String {
    format!(
        "项目进度报告\n工作流：{}\n运行：{}\n任务总数：{}\n已完成：{}\n阻塞项：{}\n证据事件：{}\n最新验证包：{}\n下一步：{}",
        report.workflow.workflow_id,
        report.workflow.run_id,
        report.board.counts.values().sum::<usize>(),
        report.board.counts.get("done").copied().unwrap_or(0),
        report.blockers.len(),
        report.evidence.event_count,
        report
            .latest_verification
            .as_ref()
            .map(|packet| packet.verification_id.as_str())
            .unwrap_or("-"),
        report.next_action
    )
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

fn context_cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf()))
}

fn usage() -> &'static str {
    "Usage: kiana report progress [--json] [--workflow <run_id>] [--task-list <id>]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress_args_rejects_duplicate_workflow() {
        let error = parse_progress_args("--workflow run-a --workflow run-b")
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate option --workflow"));
    }

    #[test]
    fn parse_progress_args_rejects_duplicate_task_list() {
        let error = parse_progress_args("--task-list first --task-list second")
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate option --task-list"));
    }
}
