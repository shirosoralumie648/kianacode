use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_tasks::{
    inspect_workflow_integrity, list_workflow_runs, read_evidence_events, read_verification_packet,
    resume_workflow_run, validate_verification_packet_completion,
    validate_verification_packet_integrity, EvidenceEvent, VerificationPacket, WorkflowError,
    WorkflowRunSummary,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub struct EvidenceCommand;

#[async_trait]
impl Command for EvidenceCommand {
    fn name(&self) -> &str {
        "evidence"
    }

    fn description(&self) -> &str {
        "Inspect and verify persisted workflow evidence"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let (subcommand, rest) = split_word(context.args.trim());
        match subcommand.unwrap_or("list") {
            "list" => evidence_list(&context, rest),
            "show" => evidence_show(&context, rest),
            "verify" => evidence_verify(&context, rest),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown evidence command '{other}'\n\n{}", usage())),
        }
    }
}

#[derive(Debug, Default)]
struct EvidenceArgs {
    json_output: bool,
    workflow_id: Option<String>,
    task_id: Option<String>,
    positional: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EvidenceListReport {
    schema: &'static str,
    workflow_id: String,
    run_id: String,
    task_id: Option<String>,
    count: usize,
    events: Vec<EvidenceEvent>,
}

#[derive(Debug, Serialize)]
struct EvidenceShowReport {
    schema: &'static str,
    workflow_id: String,
    run_id: String,
    event: EvidenceEvent,
}

#[derive(Debug, Clone, Serialize)]
struct IntegrityFinding {
    code: String,
    severity: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct EvidenceIntegrityReport {
    schema: &'static str,
    workflow_id: String,
    run_id: String,
    status: &'static str,
    event_count: usize,
    packet_count: usize,
    findings: Vec<IntegrityFinding>,
    next_action: &'static str,
}

fn evidence_list(context: &CommandContext, raw: &str) -> Result<CommandResult> {
    let args = parse_args(raw)?;
    if !args.positional.is_empty() {
        return Err(anyhow!(usage()));
    }
    let root = context_cwd(context);
    let run = resolve_run(&root, args.workflow_id.as_deref())?;
    let mut events = read_evidence_events(&run.artifact_dir)?;
    if let Some(task_id) = args.task_id.as_deref() {
        events.retain(|event| event.task_id.as_deref() == Some(task_id));
    }
    let report = EvidenceListReport {
        schema: "kiana.evidence-list.v1",
        workflow_id: run.workflow_id,
        run_id: run.run_id,
        task_id: args.task_id,
        count: events.len(),
        events,
    };
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format!(
        "Evidence list\nworkflow: {}\nrun: {}\ncount: {}",
        report.workflow_id, report.run_id, report.count
    )))
}

fn evidence_show(context: &CommandContext, raw: &str) -> Result<CommandResult> {
    let args = parse_args(raw)?;
    if args.task_id.is_some() || args.positional.len() != 1 {
        return Err(anyhow!(usage()));
    }
    let workflow_id = args
        .workflow_id
        .as_deref()
        .ok_or_else(|| anyhow!("workflow_required: evidence show requires --workflow <run_id>"))?;
    let root = context_cwd(context);
    let run = resolve_run(&root, Some(workflow_id))?;
    let event_id = &args.positional[0];
    let event = read_evidence_events(&run.artifact_dir)?
        .into_iter()
        .find(|event| event.event_id == *event_id)
        .ok_or_else(|| anyhow!("evidence_not_found: event '{event_id}' was not found"))?;
    let report = EvidenceShowReport {
        schema: "kiana.evidence-show.v1",
        workflow_id: run.workflow_id,
        run_id: run.run_id,
        event,
    };
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format!(
        "Evidence event\nid: {}\nkind: {:?}\nstatus: {:?}\nsummary: {}",
        report.event.event_id, report.event.kind, report.event.status, report.event.summary
    )))
}

fn evidence_verify(context: &CommandContext, raw: &str) -> Result<CommandResult> {
    let args = parse_args(raw)?;
    if args.task_id.is_some() || !args.positional.is_empty() {
        return Err(anyhow!(usage()));
    }
    let workflow_id = args.workflow_id.as_deref().ok_or_else(|| {
        anyhow!("workflow_required: evidence verify requires --workflow <run_id>")
    })?;
    let root = context_cwd(context);
    let resume = resume_workflow_run(&root, Some(workflow_id)).map_err(map_workflow_error)?;
    let run = resume.run.clone();
    let mut findings = Vec::new();

    if let Err(error) = inspect_workflow_integrity(&run.artifact_dir) {
        findings.push(finding(
            "workflow_integrity_invalid",
            error.to_string(),
            None,
            None,
            Some(run.artifact_dir.clone()),
        ));
    }

    if !resume.resume_status.can_continue() || !resume.eventlog_consistent {
        findings.push(finding(
            "workflow_state_mismatch",
            resume
                .blocker
                .unwrap_or_else(|| "workflow state and EventLog disagree".to_string()),
            None,
            None,
            Some(run.artifact_dir.join("state.json")),
        ));
    }

    let events = match read_evidence_events(&run.artifact_dir) {
        Ok(events) => events,
        Err(error) => {
            findings.push(finding(
                "evidence_ledger_invalid",
                error.to_string(),
                None,
                None,
                Some(run.artifact_dir.join("eventlog.jsonl")),
            ));
            Vec::new()
        }
    };
    let event_ids = events
        .iter()
        .map(|event| event.event_id.as_str())
        .collect::<BTreeSet<_>>();
    let (task_counts, task_references, task_findings) = scan_tasks(&root);
    findings.extend(task_findings);
    let packets = read_packets(&run.artifact_dir, &mut findings);

    for event in &events {
        if let Some(task_id) = event.task_id.as_deref() {
            validate_task_identity(task_id, None, &task_counts, &mut findings);
        }
    }
    for (path, packet) in &packets {
        if let Err(error) = validate_verification_packet_integrity(packet, &events) {
            findings.push(finding(
                "verification_packet_invalid",
                error.to_string(),
                packet.task_id.clone(),
                Some(packet.verification_id.clone()),
                Some(path.clone()),
            ));
        }
        if let Err(error) = validate_verification_packet_completion(&run.artifact_dir, path, packet)
        {
            findings.push(finding(
                "verification_packet_completion_missing",
                error.to_string(),
                packet.task_id.clone(),
                Some(packet.verification_id.clone()),
                Some(path.clone()),
            ));
        }
        if packet.workflow_id != run.workflow_id || packet.run_id != run.run_id {
            findings.push(finding(
                "packet_run_mismatch",
                format!(
                    "packet {} belongs to workflow {}/{}",
                    packet.verification_id, packet.workflow_id, packet.run_id
                ),
                packet.task_id.clone(),
                Some(packet.verification_id.clone()),
                Some(path.clone()),
            ));
        }
        for evidence_id in &packet.evidence_ids {
            if !event_ids.contains(evidence_id.as_str()) {
                findings.push(finding(
                    "missing_evidence_reference",
                    format!(
                        "packet {} references missing evidence {}",
                        packet.verification_id, evidence_id
                    ),
                    packet.task_id.clone(),
                    Some(packet.verification_id.clone()),
                    Some(path.clone()),
                ));
            }
        }
        for check in &packet.checks {
            match check.evidence_id.as_deref() {
                Some(evidence_id) if !event_ids.contains(evidence_id) => findings.push(finding(
                    "missing_check_evidence_reference",
                    format!(
                        "packet {} check {} references missing evidence {}",
                        packet.verification_id, check.check_id, evidence_id
                    ),
                    packet.task_id.clone(),
                    Some(packet.verification_id.clone()),
                    Some(path.clone()),
                )),
                None if check.required => findings.push(finding(
                    "missing_check_evidence_reference",
                    format!(
                        "packet {} required check {} has no evidence reference",
                        packet.verification_id, check.check_id
                    ),
                    packet.task_id.clone(),
                    Some(packet.verification_id.clone()),
                    Some(path.clone()),
                )),
                _ => {}
            }
        }
        if let Some(task_id) = packet.task_id.as_deref() {
            validate_task_identity(
                task_id,
                Some(&packet.verification_id),
                &task_counts,
                &mut findings,
            );
            let expected_reference = (
                path.strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                packet.verification_id.clone(),
            );
            if task_counts.get(task_id).copied() == Some(1)
                && !task_references
                    .get(task_id)
                    .is_some_and(|references| references.contains(&expected_reference))
            {
                findings.push(finding(
                    "missing_packet_task_link",
                    format!(
                        "task {task_id} does not reference packet {}",
                        packet.verification_id
                    ),
                    Some(task_id.to_string()),
                    Some(packet.verification_id.clone()),
                    Some(path.clone()),
                ));
            }
        }
    }

    findings.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.task_id.cmp(&right.task_id))
            .then_with(|| left.verification_id.cmp(&right.verification_id))
            .then_with(|| left.path.cmp(&right.path))
    });
    let status = if findings.is_empty() {
        "pass"
    } else {
        "blocked"
    };
    let report = EvidenceIntegrityReport {
        schema: "kiana.evidence-integrity.v1",
        workflow_id: run.workflow_id,
        run_id: run.run_id,
        status,
        event_count: events.len(),
        packet_count: packets.len(),
        findings,
        next_action: if status == "pass" {
            "continue"
        } else {
            "repair_evidence_integrity"
        },
    };
    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format!(
        "Evidence integrity\nworkflow: {}\nrun: {}\nstatus: {}\nevents: {}\npackets: {}\nfindings: {}\nnext: {}",
        report.workflow_id,
        report.run_id,
        report.status,
        report.event_count,
        report.packet_count,
        report.findings.len(),
        report.next_action
    )))
}

fn resolve_run(root: &Path, requested_run_id: Option<&str>) -> Result<WorkflowRunSummary> {
    let runs = list_workflow_runs(root).map_err(map_workflow_error)?;
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

fn map_workflow_error(error: WorkflowError) -> anyhow::Error {
    match error {
        WorkflowError::NoRuns | WorkflowError::RunNotFound(_) => anyhow!(
            "workflow_not_found: initialize a workflow first with 'kiana tasks workflow init <request>'"
        ),
        other => anyhow!("workflow_inconsistent: {other}"),
    }
}

fn read_packets(
    artifact_dir: &Path,
    findings: &mut Vec<IntegrityFinding>,
) -> Vec<(PathBuf, VerificationPacket)> {
    let dir = artifact_dir.join("verification");
    let mut paths = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>(),
        Err(error) => {
            findings.push(finding(
                "verification_directory_unreadable",
                error.to_string(),
                None,
                None,
                Some(dir),
            ));
            return Vec::new();
        }
    };
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| match read_verification_packet(&path) {
            Ok(packet) => Some((path, packet)),
            Err(error) => {
                findings.push(finding(
                    "verification_packet_invalid",
                    error.to_string(),
                    None,
                    None,
                    Some(path),
                ));
                None
            }
        })
        .collect()
}

fn scan_tasks(
    root: &Path,
) -> (
    BTreeMap<String, usize>,
    BTreeMap<String, BTreeSet<(String, String)>>,
    Vec<IntegrityFinding>,
) {
    let tasks_root = root.join(".kiana/tasks");
    let mut paths = Vec::new();
    if let Ok(lists) = std::fs::read_dir(&tasks_root) {
        for list in lists.flatten() {
            let list_path = list.path();
            if !list_path.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&list_path) {
                paths.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
                    path.extension().and_then(|value| value.to_str()) == Some("json")
                }));
            }
        }
    }
    paths.sort();
    let mut counts = BTreeMap::new();
    let mut references = BTreeMap::<String, BTreeSet<(String, String)>>::new();
    let mut findings = Vec::new();
    for path in paths {
        let value: Value = match std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))
            .and_then(|contents| {
                serde_json::from_str(&contents)
                    .with_context(|| format!("failed to parse {}", path.display()))
            }) {
            Ok(value) => value,
            Err(error) => {
                findings.push(finding(
                    "task_file_invalid",
                    error.to_string(),
                    None,
                    None,
                    Some(path),
                ));
                continue;
            }
        };
        let Some(task_id) = value
            .get("id")
            .or_else(|| value.get("task_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            findings.push(finding(
                "task_identity_missing",
                "task file is missing id/task_id".to_string(),
                None,
                None,
                Some(path),
            ));
            continue;
        };
        *counts.entry(task_id.to_string()).or_insert(0) += 1;
        if let Some(evidence) = value
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| metadata.get("evidence"))
            .and_then(Value::as_array)
        {
            for reference in evidence.iter().filter_map(Value::as_str) {
                if let Some(verification_reference) = verification_reference(reference) {
                    references
                        .entry(task_id.to_string())
                        .or_default()
                        .insert(verification_reference);
                }
            }
        }
    }
    (counts, references, findings)
}

fn verification_reference(reference: &str) -> Option<(String, String)> {
    let (path, verification_id) = reference.strip_prefix("verification:")?.rsplit_once('#')?;
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || verification_id.trim().is_empty()
    {
        return None;
    }
    Some((
        path.to_string_lossy().replace('\\', "/"),
        verification_id.to_string(),
    ))
}

fn validate_task_identity(
    task_id: &str,
    verification_id: Option<&str>,
    task_counts: &BTreeMap<String, usize>,
    findings: &mut Vec<IntegrityFinding>,
) {
    match task_counts.get(task_id).copied().unwrap_or(0) {
        0 => findings.push(finding(
            "missing_task_reference",
            format!("evidence references missing task {task_id}"),
            Some(task_id.to_string()),
            verification_id.map(str::to_string),
            None,
        )),
        count if count > 1 => findings.push(finding(
            "duplicate_task_reference",
            format!("task {task_id} matches {count} files"),
            Some(task_id.to_string()),
            verification_id.map(str::to_string),
            None,
        )),
        _ => {}
    }
}

fn finding(
    code: impl Into<String>,
    message: impl Into<String>,
    task_id: Option<String>,
    verification_id: Option<String>,
    path: Option<PathBuf>,
) -> IntegrityFinding {
    IntegrityFinding {
        code: code.into(),
        severity: "blocker".to_string(),
        message: message.into(),
        task_id,
        verification_id,
        path: path.map(|path| path.to_string_lossy().to_string()),
    }
}

fn parse_args(raw: &str) -> Result<EvidenceArgs> {
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut args = EvidenceArgs::default();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        match token {
            "--json" | "json" => args.json_output = true,
            "--workflow" => {
                index += 1;
                args.workflow_id = Some(required_value(&tokens, index, "--workflow")?);
            }
            "--task" => {
                index += 1;
                args.task_id = Some(required_value(&tokens, index, "--task")?);
            }
            _ if token.starts_with("--workflow=") => {
                args.workflow_id = Some(value_after_equals(token, "--workflow")?);
            }
            _ if token.starts_with("--task=") => {
                args.task_id = Some(value_after_equals(token, "--task")?);
            }
            _ if token.starts_with('-') => return Err(anyhow!(usage())),
            _ => args.positional.push(token.to_string()),
        }
        index += 1;
    }
    Ok(args)
}

fn required_value(tokens: &[&str], index: usize, option: &str) -> Result<String> {
    tokens
        .get(index)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && !value.starts_with('-'))
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

fn context_cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf()))
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

fn usage() -> &'static str {
    "Usage:\n  kiana evidence list [--json] [--workflow <run_id>] [--task <task_id>]\n  kiana evidence show [--json] --workflow <run_id> <event_id>\n  kiana evidence verify [--json] --workflow <run_id>"
}
