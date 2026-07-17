use crate::checks::{run_checks_report, CheckResult};
use crate::tasks::{
    acquire_task_evidence_lease, append_task_evidence_reference, validate_task_evidence_target,
};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_tasks::{
    append_evidence_event, build_verification_packet, read_evidence_events,
    redact_and_bound_evidence_text, resume_workflow_run, write_verification_packet,
    EvidenceEventDraft, EvidenceKind, EvidenceSource, EvidenceStatus, VerificationCheck,
    VerificationStatus, WorkflowError,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const SUPPORTED_PROFILE: &str = "project_p0";
const OUTPUT_TAIL_BYTES: usize = 8 * 1024;

pub struct ValidateCommand;

#[async_trait]
impl Command for ValidateCommand {
    fn name(&self) -> &str {
        "validate"
    }

    fn description(&self) -> &str {
        "Run a verification profile and persist evidence"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let args = parse_validate_args(&context.args)?;
        if args.show_help {
            return Ok(CommandResult::text(usage()));
        }
        if args.profile != SUPPORTED_PROFILE {
            return Err(anyhow!(
                "profile_unsupported: profile '{}' is not executable yet; supported profile: {}",
                args.profile,
                SUPPORTED_PROFILE
            ));
        }

        let root = context_cwd(&context);
        let resume = resume_workflow_run(&root, args.workflow_id.as_deref()).map_err(|error| {
            match error {
                WorkflowError::NoRuns | WorkflowError::RunNotFound(_) => anyhow!(
                    "workflow_not_found: initialize a workflow first with 'kiana tasks workflow init <request>'"
                ),
                other => anyhow!("workflow_inconsistent: {other}"),
            }
        })?;
        if !resume.resume_status.can_continue() || !resume.eventlog_consistent {
            return Err(anyhow!(
                "workflow_inconsistent: {}; suggested_next={}",
                resume
                    .blocker
                    .as_deref()
                    .unwrap_or("workflow state and eventlog are inconsistent"),
                resume.recommended_action
            ));
        }
        let task_lease = args
            .task_id
            .as_deref()
            .map(|task_id| {
                acquire_task_evidence_lease(&context, Some(args.task_list_id.as_str()), task_id)
            })
            .transpose()?;

        let checks = run_checks_report(&root)?;
        let mut verification_checks = Vec::with_capacity(checks.results.len());
        for result in &checks.results {
            let status = evidence_status(result.status);
            let reason = check_reason(result, status);
            let stdout = redact_and_bound_evidence_text(&result.stdout, OUTPUT_TAIL_BYTES);
            let stderr = redact_and_bound_evidence_text(&result.stderr, OUTPUT_TAIL_BYTES);
            let event = append_evidence_event(
                &resume.run.artifact_dir,
                EvidenceEventDraft {
                    event_id: None,
                    workflow_id: resume.run.workflow_id.clone(),
                    run_id: resume.run.run_id.clone(),
                    task_id: args.task_id.clone(),
                    workpacket_id: None,
                    recorded_at_ms: None,
                    kind: evidence_kind(result),
                    status,
                    summary: format!("{}: {}", result.description, result.status),
                    source: EvidenceSource {
                        source_type: "local_command".to_string(),
                        name: "kiana validate".to_string(),
                        actor: Some("coordinator".to_string()),
                    },
                    payload: json!({
                        "check_id": result.id,
                        "command": result.command,
                        "cwd": checks.root.as_str(),
                        "exit_code": result.exit_code,
                        "stdout": stdout,
                        "stderr": stderr,
                        "error": result.error.as_deref(),
                        "reason": reason.clone(),
                        "isolation": checks.execution.isolation,
                        "applied_current_changes": checks.execution.applied_current_changes,
                    }),
                    changed_files: Vec::new(),
                    confidence: Some(1.0),
                    severity: if status == EvidenceStatus::Fail {
                        Some("blocker".to_string())
                    } else {
                        None
                    },
                    next_action: Some(
                        match status {
                            EvidenceStatus::Pass => "review",
                            EvidenceStatus::Fail => "fix_failed_check",
                            EvidenceStatus::Blocked => "resolve_blocker",
                            EvidenceStatus::Skipped | EvidenceStatus::Unknown => "configure_check",
                        }
                        .to_string(),
                    ),
                    supersedes_event_id: None,
                },
            )?;
            verification_checks.push(VerificationCheck {
                check_id: result.id.to_string(),
                description: result.description.to_string(),
                required: true,
                status,
                evidence_id: Some(event.event_id),
                command: Some(result.command.to_string()),
                reason,
            });
        }

        let ledger = read_evidence_events(&resume.run.artifact_dir)?;
        let packet = build_verification_packet(
            &resume.run.workflow_id,
            &resume.run.run_id,
            args.task_id.clone(),
            &args.profile,
            verification_checks,
            &ledger,
        )?;
        if let Some(task_id) = args.task_id.as_deref() {
            validate_task_evidence_target(&context, Some(args.task_list_id.as_str()), task_id)?;
        }
        let packet_path = write_verification_packet(&resume.run.artifact_dir, &packet)?;
        if let Some(task_id) = args.task_id.as_deref() {
            let relative_packet = packet_path
                .strip_prefix(&root)
                .unwrap_or(packet_path.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            let evidence_reference = format!(
                "verification:{}#{}",
                relative_packet, packet.verification_id
            );
            append_task_evidence_reference(
                &context,
                Some(&args.task_list_id),
                task_id,
                &evidence_reference,
                task_lease
                    .as_ref()
                    .expect("task lease exists when task_id is present"),
            )?;
        }
        let report = ValidationRunReport {
            schema: "kiana.validation-run.v1",
            workflow_id: resume.run.workflow_id,
            run_id: resume.run.run_id,
            task_id: args.task_id,
            task_list_id: args.task_list_id,
            profile: args.profile,
            final_status: packet.final_status,
            verification_id: packet.verification_id,
            packet_path,
            evidence_ids: packet.evidence_ids,
            summary: ValidationSummary {
                total: packet.checks.len(),
                passed: packet.pass_count,
                failed: packet.fail_count,
                blocked: packet.blocked_count,
                skipped: packet.skipped_count,
                unknown: packet.unknown_count,
            },
            isolation: checks.execution.isolation,
            applied_current_changes: checks.execution.applied_current_changes,
            next_action: packet.next_action,
        };

        if args.json_output {
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }
        Ok(CommandResult::text(format_validation_report(&report)))
    }
}

fn parse_validate_args(raw: &str) -> Result<ValidateArgs> {
    let mut args = ValidateArgs {
        profile: SUPPORTED_PROFILE.to_string(),
        task_list_id: "default".to_string(),
        ..ValidateArgs::default()
    };
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        match token {
            "--json" | "json" => args.json_output = true,
            "help" | "--help" | "-h" => args.show_help = true,
            "--workflow" => {
                index += 1;
                args.workflow_id = Some(required_value(&tokens, index, "--workflow")?);
            }
            "--task" => {
                index += 1;
                args.task_id = Some(required_value(&tokens, index, "--task")?);
            }
            "--task-list" => {
                index += 1;
                args.task_list_id = required_value(&tokens, index, "--task-list")?;
            }
            "--profile" => {
                index += 1;
                args.profile = required_value(&tokens, index, "--profile")?;
            }
            _ if token.starts_with("--workflow=") => {
                args.workflow_id = Some(value_after_equals(token, "--workflow")?);
            }
            _ if token.starts_with("--task=") => {
                args.task_id = Some(value_after_equals(token, "--task")?);
            }
            _ if token.starts_with("--task-list=") => {
                args.task_list_id = value_after_equals(token, "--task-list")?;
            }
            _ if token.starts_with("--profile=") => {
                args.profile = value_after_equals(token, "--profile")?;
            }
            _ => return Err(anyhow!(usage())),
        }
        index += 1;
    }
    Ok(args)
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

fn evidence_status(status: &str) -> EvidenceStatus {
    match status {
        "passed" => EvidenceStatus::Pass,
        "failed" => EvidenceStatus::Fail,
        "skipped" => EvidenceStatus::Skipped,
        "blocked" => EvidenceStatus::Blocked,
        _ => EvidenceStatus::Unknown,
    }
}

fn evidence_kind(result: &CheckResult) -> EvidenceKind {
    match result.id {
        "rustfmt" => EvidenceKind::LintResult,
        "cargo_check" => EvidenceKind::BuildResult,
        "cargo_test" | "release_smoke" => EvidenceKind::TestResult,
        _ => EvidenceKind::CommandResult,
    }
}

fn check_reason(result: &CheckResult, status: EvidenceStatus) -> Option<String> {
    if status == EvidenceStatus::Skipped {
        return Some(
            result
                .error
                .clone()
                .unwrap_or_else(|| "check was skipped".to_string()),
        );
    }
    result.error.clone()
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
    "Usage: kiana validate [--json] [--workflow <run_id>] [--task <task_id>] [--task-list <id>] [--profile project_p0]"
}

fn format_validation_report(report: &ValidationRunReport) -> String {
    format!(
        "验证完成\nworkflow: {}\nrun: {}\nprofile: {}\nstatus: {}\npassed: {}\nfailed: {}\nblocked: {}\npacket: {}\nnext: {}",
        report.workflow_id,
        report.run_id,
        report.profile,
        verification_status_text(report.final_status),
        report.summary.passed,
        report.summary.failed,
        report.summary.blocked,
        report.packet_path.display(),
        report.next_action
    )
}

fn verification_status_text(status: VerificationStatus) -> &'static str {
    match status {
        VerificationStatus::Pass => "pass",
        VerificationStatus::Fail => "fail",
        VerificationStatus::Blocked => "blocked",
        VerificationStatus::Inconclusive => "inconclusive",
    }
}

#[derive(Debug, Default)]
struct ValidateArgs {
    json_output: bool,
    show_help: bool,
    workflow_id: Option<String>,
    task_id: Option<String>,
    task_list_id: String,
    profile: String,
}

#[derive(Debug, Serialize)]
struct ValidationRunReport {
    schema: &'static str,
    workflow_id: String,
    run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    task_list_id: String,
    profile: String,
    final_status: VerificationStatus,
    verification_id: String,
    packet_path: PathBuf,
    evidence_ids: Vec<String>,
    summary: ValidationSummary,
    isolation: &'static str,
    applied_current_changes: bool,
    next_action: String,
}

#[derive(Debug, Serialize)]
struct ValidationSummary {
    total: usize,
    passed: usize,
    failed: usize,
    blocked: usize,
    skipped: usize,
    unknown: usize,
}
