use crate::tasks::{inspect_swarm_recovery_integrity, load_tasks, task_list_id};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_tasks::{
    append_evidence_event, build_project_board_at_root, inspect_workflow_integrity,
    list_verification_packets, list_workflow_runs, read_evidence_events, read_workflow_state,
    redact_and_bound_evidence_text, unresolved_blocking_evidence,
    validate_verification_packet_completion, validate_verification_packet_integrity,
    write_review_packet, EvidenceEvent, EvidenceEventDraft, EvidenceKind, EvidenceSource,
    EvidenceStatus, IntegrityError, ProjectBoardProjection, ReviewFinding, ReviewPacket,
    ReviewSeverity, WorkflowError, WorkflowRunSummary, REVIEW_PACKET_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct AuditCommand;

#[async_trait]
impl Command for AuditCommand {
    fn name(&self) -> &str {
        "audit"
    }

    fn description(&self) -> &str {
        "Run an evidence-backed strict commercial readiness audit"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let (subcommand, rest) = split_word(context.args.trim());
        match subcommand.unwrap_or("strict") {
            "strict" => audit_strict(&context, rest),
            "help" | "--help" | "-h" => Ok(CommandResult::text(usage())),
            other => Err(anyhow!("unknown audit command '{other}'\n\n{}", usage())),
        }
    }
}

#[derive(Debug, Default)]
struct AuditArgs {
    json_output: bool,
    workflow_id: Option<String>,
    task_list_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CommercialSummary {
    schema: String,
    status: String,
    blocking: usize,
    external_blocking: usize,
    local_blocking: usize,
    satisfied: usize,
    total_checks: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommercialReportContract {
    schema: String,
    version: String,
    generated_at: String,
    status: String,
    release_tag: String,
    summary: CommercialReportSummaryContract,
    checks: Vec<CommercialCheckContract>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommercialReportSummaryContract {
    total_checks: usize,
    satisfied: usize,
    blocking: usize,
    external_blocking: usize,
    local_blocking: usize,
    blocking_by_resolution_scope: CommercialScopeCountsContract,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommercialScopeCountsContract {
    #[serde(rename = "local-automation")]
    local_automation: usize,
    #[serde(rename = "release-owner")]
    release_owner: usize,
    #[serde(rename = "release-security")]
    release_security: usize,
    #[serde(rename = "release-environment")]
    release_environment: usize,
    #[serde(rename = "final-artifact-derived")]
    final_artifact_derived: usize,
    #[serde(rename = "live-service")]
    live_service: usize,
    #[serde(rename = "acceptance-owner")]
    acceptance_owner: usize,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommercialCheckContract {
    id: String,
    category: String,
    severity: String,
    status: String,
    external: bool,
    title: String,
    gate: String,
    evidence: String,
    required_action: String,
    paths: Vec<String>,
    commands: Vec<String>,
    env: Vec<String>,
    owner: String,
    owner_status: String,
    resolution_scope: String,
    acceptance_artifacts: Vec<String>,
    verification_commands: Vec<String>,
    handoff_notes: Vec<String>,
}

struct BoundedProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    timed_out: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AuditSurface {
    id: &'static str,
    applicable: bool,
    status: &'static str,
    evidence: Vec<String>,
    missing: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StrictAuditReport {
    schema: &'static str,
    workflow_id: String,
    run_id: String,
    task_list_id: String,
    status: String,
    decision: String,
    score: f32,
    blocking_count: usize,
    finding_count: usize,
    findings: Vec<ReviewFinding>,
    surfaces: Vec<AuditSurface>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commercial: Option<CommercialSummary>,
    review_id: String,
    persistence_status: String,
    packet_path: Option<String>,
    evidence_ids: Vec<String>,
    next_action: String,
}

fn audit_strict(context: &CommandContext, raw: &str) -> Result<CommandResult> {
    let args = parse_audit_args(raw)?;
    let root = context_cwd(context);
    let workflow = resolve_workflow(&root, args.workflow_id.as_deref())?;
    let explicit_task_list = args.task_list_id.as_deref();
    let selected_task_list = task_list_id(context, explicit_task_list);
    let tasks = load_tasks(context, explicit_task_list)?;
    let board = build_project_board_at_root(&root, selected_task_list.clone(), &tasks)?;
    let mut findings = Vec::new();
    append_workflow_integrity_findings(&workflow, &mut findings);

    let (events, ledger_readable) = match read_evidence_events(&workflow.artifact_dir) {
        Ok(events) => (events, true),
        Err(error) => {
            findings.push(block_finding(
                "evidence_integrity",
                "Evidence Ledger 无法读取",
                None,
                None,
                error.to_string(),
                "修复或恢复 eventlog.jsonl 后重新执行严格审计",
                "workflow-owner",
            ));
            (Vec::new(), false)
        }
    };
    append_board_findings(&board, &mut findings);
    append_verification_findings(&root, &workflow, &events, &mut findings);
    append_unresolved_evidence_findings(&workflow, &events, &mut findings);
    append_stale_artifact_findings(&root, &workflow, &mut findings);
    let surfaces = detect_surfaces(&root);
    append_surface_findings(&surfaces, &mut findings);
    findings.extend(scan_suspicious_markers(&root));
    let (commercial, mut commercial_findings) = read_commercial_blockers(&root);
    findings.append(&mut commercial_findings);
    sort_findings(&mut findings);

    let blocking_count = findings
        .iter()
        .filter(|finding| finding.severity == ReviewSeverity::Block)
        .count();
    let status = if blocking_count > 0 {
        "blocked"
    } else if findings.is_empty() {
        "pass"
    } else {
        "review_required"
    };
    let next_action = match status {
        "blocked" => "resolve_blocking_findings",
        "review_required" => "triage_findings",
        _ => "continue:ship",
    };
    let score = audit_score(&findings);
    let review_id = format!("rv_strict_{}_{}", now_ms(), std::process::id());
    let (evidence_ids, packet_path, persistence_status) = if ledger_readable {
        let evidence_ids = persist_finding_evidence(&workflow, &findings, &events)?;
        let packet = ReviewPacket {
            schema: REVIEW_PACKET_SCHEMA.to_string(),
            id: review_id.clone(),
            workflow_id: workflow.workflow_id.clone(),
            run_id: workflow.run_id.clone(),
            mode: "strict".to_string(),
            reviewer_type: "commercial_readiness".to_string(),
            scope: vec!["repository".to_string(), selected_task_list.clone()],
            findings: findings.clone(),
            score,
            blocking_count,
            recommendation: audit_recommendation(status).to_string(),
            decision: status.to_string(),
            evidence_refs: evidence_ids.clone(),
            created_at_ms: now_ms(),
            next_action: next_action.to_string(),
        };
        let packet_path = write_review_packet(&workflow.artifact_dir, &packet)?;
        (
            evidence_ids,
            Some(project_relative_path(&root, &packet_path)),
            "persisted".to_string(),
        )
    } else {
        (Vec::new(), None, "blocked".to_string())
    };
    let report = StrictAuditReport {
        schema: "kiana.strict-audit.v1",
        workflow_id: workflow.workflow_id,
        run_id: workflow.run_id,
        task_list_id: selected_task_list,
        status: status.to_string(),
        decision: status.to_string(),
        score,
        blocking_count,
        finding_count: findings.len(),
        findings,
        surfaces,
        commercial,
        review_id,
        persistence_status,
        packet_path,
        evidence_ids,
        next_action: next_action.to_string(),
    };

    if args.json_output {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format_audit_report(&report)))
}

fn append_workflow_integrity_findings(
    workflow: &WorkflowRunSummary,
    findings: &mut Vec<ReviewFinding>,
) {
    match inspect_workflow_integrity(&workflow.artifact_dir) {
        Ok(_) => {
            if let Err(error) = inspect_swarm_recovery_integrity(&workflow.artifact_dir) {
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Block,
                    confidence: 1.0,
                    category: "workflow_recovery_integrity".to_string(),
                    title: "Swarm 恢复日志完整性验证失败".to_string(),
                    file: Some(".kiana/workflows".to_string()),
                    line: None,
                    evidence: error.to_string(),
                    risk: "恢复状态可能被篡改、降级、回放或脱离认证归档链".to_string(),
                    recommendation: "恢复可信密钥和已认证 journal，或重新执行受控恢复".to_string(),
                    owner: "workflow-owner".to_string(),
                    decision: "fix".to_string(),
                    next_action: "resume_or_restore_authenticated_recovery_journal".to_string(),
                });
            }
        }
        Err(WorkflowError::Integrity(IntegrityError::ArtifactMismatch(path))) => {
            findings.push(ReviewFinding {
                severity: ReviewSeverity::Block,
                confidence: 1.0,
                category: "workflow_integrity_artifact_mismatch".to_string(),
                title: "已认证 Workflow artifact 与 descriptor 不一致".to_string(),
                file: Some(path.clone()),
                line: None,
                evidence: format!("integrity_artifact_mismatch: {path}"),
                risk: "事实产物可能被修改、删除或绕过认证写入".to_string(),
                recommendation: "恢复已认证内容或重新生成并提交事实产物".to_string(),
                owner: "workflow-owner".to_string(),
                decision: "fix".to_string(),
                next_action: "restore_or_regenerate_authenticated_artifact".to_string(),
            });
        }
        Err(error) => findings.push(ReviewFinding {
            severity: ReviewSeverity::Block,
            confidence: 1.0,
            category: "workflow_integrity".to_string(),
            title: "Workflow 完整性验证失败".to_string(),
            file: Some(".kiana/workflows".to_string()),
            line: None,
            evidence: error.to_string(),
            risk: "Workflow 历史、状态或认证链不可作为发布证据".to_string(),
            recommendation: "恢复可信 EventLog 和完整性密钥后重新执行严格审计".to_string(),
            owner: "workflow-owner".to_string(),
            decision: "fix".to_string(),
            next_action: "restore_workflow_integrity".to_string(),
        }),
    }
}

fn append_board_findings(board: &ProjectBoardProjection, findings: &mut Vec<ReviewFinding>) {
    for finding in &board.policy_findings {
        let severity = if finding.severity == "blocker" {
            ReviewSeverity::Block
        } else {
            ReviewSeverity::Medium
        };
        findings.push(ReviewFinding {
            severity,
            confidence: 1.0,
            category: finding.code.clone(),
            title: format!("Task {} 未满足项目门禁", finding.task_id),
            file: Some(format!(
                ".kiana/tasks/{}/{}.json",
                board.task_list_id, finding.task_id
            )),
            line: None,
            evidence: finding.message.clone(),
            risk: "任务状态可能与可验证完成事实不一致".to_string(),
            recommendation: "修复任务依赖、验证引用或显式 blocker 后重新审计".to_string(),
            owner: "task-owner".to_string(),
            decision: if severity == ReviewSeverity::Block {
                "fix".to_string()
            } else {
                "review".to_string()
            },
            next_action: "repair_task_gate".to_string(),
        });
    }
}

fn append_verification_findings(
    root: &Path,
    workflow: &WorkflowRunSummary,
    events: &[EvidenceEvent],
    findings: &mut Vec<ReviewFinding>,
) {
    let packets = match list_verification_packets(&workflow.artifact_dir) {
        Ok(packets) => packets,
        Err(error) => {
            findings.push(block_finding(
                "verification_integrity",
                "VerificationPacket 无法读取",
                None,
                None,
                error.to_string(),
                "修复 verification 目录中的损坏或不兼容文件",
                "verification-owner",
            ));
            return;
        }
    };
    for (path, packet) in packets {
        if let Err(error) = validate_verification_packet_integrity(&packet, events) {
            findings.push(block_finding(
                "verification_integrity",
                "VerificationPacket 完整性校验失败",
                Some(project_relative_path(root, &path)),
                None,
                error.to_string(),
                "重新运行对应验证并替换损坏的验证包",
                "verification-owner",
            ));
            continue;
        }
        if let Err(error) =
            validate_verification_packet_completion(&workflow.artifact_dir, &path, &packet)
        {
            findings.push(block_finding(
                "verification_completion_missing",
                "VerificationPacket 缺少完成事件",
                Some(project_relative_path(root, &path)),
                None,
                error.to_string(),
                "重新运行对应验证，生成与 EventLog 一致的验证包",
                "verification-owner",
            ));
            continue;
        }
        for check in packet.checks.iter().filter(|check| check.required) {
            if matches!(
                check.status,
                EvidenceStatus::Skipped | EvidenceStatus::Unknown
            ) {
                findings.push(block_finding(
                    "required_check_incomplete",
                    &format!("Required check {} 未完成", check.check_id),
                    Some(project_relative_path(root, &path)),
                    None,
                    check
                        .reason
                        .clone()
                        .unwrap_or_else(|| "required check is skipped or unknown".to_string()),
                    "执行 required check 或记录可审计的阻塞原因",
                    "verification-owner",
                ));
            }
        }
    }
}

fn persist_finding_evidence(
    workflow: &WorkflowRunSummary,
    findings: &[ReviewFinding],
    existing_events: &[EvidenceEvent],
) -> Result<Vec<String>> {
    let existing_ids = existing_events
        .iter()
        .filter(|event| {
            event.workflow_id == workflow.workflow_id && event.run_id == workflow.run_id
        })
        .map(|event| event.event_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut evidence_ids = Vec::new();
    for finding in findings {
        if finding.category == "unresolved_evidence"
            && existing_ids.contains(finding.evidence.as_str())
        {
            evidence_ids.push(finding.evidence.clone());
            continue;
        }
        let blocking = finding.severity == ReviewSeverity::Block;
        let event = append_evidence_event(
            &workflow.artifact_dir,
            EvidenceEventDraft {
                event_id: None,
                workflow_id: workflow.workflow_id.clone(),
                run_id: workflow.run_id.clone(),
                task_id: None,
                workpacket_id: None,
                recorded_at_ms: None,
                kind: if blocking {
                    EvidenceKind::BlockerRecord
                } else {
                    EvidenceKind::ReviewFinding
                },
                status: if blocking {
                    EvidenceStatus::Blocked
                } else {
                    EvidenceStatus::Unknown
                },
                summary: finding.title.clone(),
                source: EvidenceSource {
                    source_type: "local_audit".to_string(),
                    name: "kiana audit strict".to_string(),
                    actor: Some("coordinator".to_string()),
                },
                payload: serde_json::to_value(finding)?,
                changed_files: Vec::new(),
                confidence: Some(finding.confidence as f64),
                severity: Some(format!("{:?}", finding.severity).to_ascii_lowercase()),
                next_action: Some(finding.next_action.clone()),
                supersedes_event_id: None,
            },
        )?;
        evidence_ids.push(event.event_id);
    }
    Ok(evidence_ids)
}

fn append_unresolved_evidence_findings(
    workflow: &WorkflowRunSummary,
    events: &[EvidenceEvent],
    findings: &mut Vec<ReviewFinding>,
) {
    findings.extend(
        unresolved_blocking_evidence(events, &workflow.workflow_id, &workflow.run_id)
            .into_iter()
            .map(|event| ReviewFinding {
                severity: ReviewSeverity::Block,
                confidence: event.confidence.unwrap_or(1.0) as f32,
                category: "unresolved_evidence".to_string(),
                title: event.summary,
                file: None,
                line: None,
                evidence: event.event_id,
                risk: "Evidence Ledger 中仍存在未关闭的阻塞事实".to_string(),
                recommendation: event
                    .next_action
                    .clone()
                    .unwrap_or_else(|| "追加 supersedes 事件明确关闭阻塞".to_string()),
                owner: event
                    .source
                    .actor
                    .unwrap_or_else(|| "workflow-owner".to_string()),
                decision: "fix".to_string(),
                next_action: event
                    .next_action
                    .unwrap_or_else(|| "resolve_unresolved_evidence".to_string()),
            }),
    );
}

fn append_stale_artifact_findings(
    root: &Path,
    workflow: &WorkflowRunSummary,
    findings: &mut Vec<ReviewFinding>,
) {
    for directory in ["verification", "review"] {
        let artifact_directory = workflow.artifact_dir.join(directory);
        let Ok(entries) = fs::read_dir(&artifact_directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("tmp") {
                continue;
            }
            findings.push(block_finding(
                "stale_artifact_temp",
                "发现未完成的临时审计产物",
                Some(project_relative_path(root, &path)),
                None,
                format!("temporary {directory} artifact remains after an interrupted write"),
                "确认没有活跃 writer 后删除临时文件，并重新运行对应验证或审查",
                "workflow-owner",
            ));
        }
    }
}

fn detect_surfaces(root: &Path) -> Vec<AuditSurface> {
    let cargo = root.join("Cargo.toml").is_file();
    let package_json = root.join("package.json").is_file();
    let python = root.join("pyproject.toml").is_file() || root.join("setup.py").is_file();
    let test_applicable = cargo || package_json || python;
    let mut test_evidence = Vec::new();
    if workspace_has_test_surface(root) {
        test_evidence.push("workspace tests or inline test declarations".to_string());
    }
    if package_json && package_has_test_script(&root.join("package.json")) {
        test_evidence.push("package.json scripts.test".to_string());
    }
    let test_missing = if test_applicable && test_evidence.is_empty() {
        vec!["未发现 tests/、Rust #[test] 或有效 package test script".to_string()]
    } else {
        Vec::new()
    };

    let release_hints = [
        "VERSION",
        "scripts/release-preflight.sh",
        "scripts/package-release.sh",
        ".github/workflows/release.yml",
        ".github/workflows/release.yaml",
    ];
    let release_applicable = release_hints.iter().any(|path| root.join(path).exists());
    let mut release_evidence = Vec::new();
    let has_version = root.join("VERSION").is_file()
        || cargo
        || package_json
        || root.join("pyproject.toml").is_file();
    if has_version {
        release_evidence.push("version contract".to_string());
    }
    let release_entrypoints = [
        "scripts/release-preflight.sh",
        "scripts/package-release.sh",
        ".github/workflows/release.yml",
        ".github/workflows/release.yaml",
    ];
    let has_release_entrypoint = release_entrypoints
        .iter()
        .any(|path| root.join(path).is_file());
    if has_release_entrypoint {
        release_evidence.push("release entrypoint".to_string());
    }
    let mut release_missing = Vec::new();
    if release_applicable && !has_version {
        release_missing.push("缺少版本契约".to_string());
    }
    if release_applicable && !has_release_entrypoint {
        release_missing.push("缺少 release script 或 release workflow".to_string());
    }

    let capability_hints = [
        "plugins",
        ".mcp.json",
        "mcp",
        "hooks",
        "kiana-tools",
        "kiana-commands/src/mcp.rs",
        "kiana-commands/src/plugin.rs",
        "kiana-commands/src/permissions.rs",
    ];
    let security_applicable = capability_hints.iter().any(|path| root.join(path).exists());
    let security_contracts = [
        "kiana-commands/src/permissions.rs",
        "kiana-types/src/trust.rs",
        "scripts/platform-security-proof-report.sh",
        "docs/schemas/kiana-platform-security-proof.v1.schema.json",
    ];
    let security_evidence = existing_paths(root, &security_contracts);
    let security_missing = if security_applicable && security_evidence.is_empty() {
        vec!["能力包含插件、MCP、hooks 或执行权限，但未发现安全控制契约".to_string()]
    } else {
        Vec::new()
    };

    let policy_applicable = capability_hints.iter().any(|path| root.join(path).exists());
    let policy_contracts = [
        "docs/schemas/kiana-managed-plugin-policy.v1.schema.json",
        "kiana-commands/src/permissions.rs",
        "kiana-types/src/trust.rs",
        ".kiana/policy.json",
    ];
    let policy_evidence = existing_paths(root, &policy_contracts);
    let policy_missing = if policy_applicable && policy_evidence.is_empty() {
        vec!["能力包含插件、MCP、hooks 或权限入口，但未发现 trust/policy 契约".to_string()]
    } else {
        Vec::new()
    };

    vec![
        surface("test", test_applicable, test_evidence, test_missing),
        surface(
            "release",
            release_applicable,
            release_evidence,
            release_missing,
        ),
        surface(
            "security",
            security_applicable,
            security_evidence,
            security_missing,
        ),
        surface("policy", policy_applicable, policy_evidence, policy_missing),
    ]
}

fn surface(
    id: &'static str,
    applicable: bool,
    evidence: Vec<String>,
    missing: Vec<String>,
) -> AuditSurface {
    let status = if !applicable {
        "not_applicable"
    } else if missing.is_empty() {
        "ready"
    } else {
        "missing"
    };
    AuditSurface {
        id,
        applicable,
        status,
        evidence,
        missing,
    }
}

fn append_surface_findings(surfaces: &[AuditSurface], findings: &mut Vec<ReviewFinding>) {
    for surface in surfaces
        .iter()
        .filter(|surface| surface.status == "missing")
    {
        findings.push(block_finding(
            &format!("surface:{}", surface.id),
            &format!("缺少适用的 {} surface", surface.id),
            None,
            None,
            surface.missing.join("; "),
            "补齐可执行入口、策略契约与可验证证据，或记录该 surface 不适用的明确理由",
            "project-owner",
        ));
    }
}

fn existing_paths(root: &Path, candidates: &[&str]) -> Vec<String> {
    candidates
        .iter()
        .filter(|path| root.join(path).is_file())
        .map(|path| (*path).to_string())
        .collect()
}

fn directory_has_test_file(root: &Path) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(file_type) if file_type.is_file() && is_test_source_file(&entry.path()) => {
                    return true;
                }
                Ok(file_type) if file_type.is_dir() => stack.push(entry.path()),
                _ => {}
            }
        }
    }
    false
}

fn is_test_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some(
            "rs" | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "py"
                | "go"
                | "java"
                | "kt"
                | "kts"
                | "c"
                | "cc"
                | "cpp"
                | "cs"
                | "rb"
                | "php"
                | "swift"
        )
    )
}

fn rust_source_has_test_attribute(contents: &str) -> bool {
    let mut block_comment_depth = 0usize;
    for line in contents.lines() {
        let bytes = line.as_bytes();
        let mut visible = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if block_comment_depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    block_comment_depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    block_comment_depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }
            if bytes[index..].starts_with(b"//") {
                break;
            }
            if bytes[index..].starts_with(b"/*") {
                block_comment_depth += 1;
                index += 2;
                continue;
            }
            visible.push(bytes[index]);
            index += 1;
        }
        if std::str::from_utf8(&visible).is_ok_and(|line| line.trim_start().starts_with("#[test]"))
        {
            return true;
        }
    }
    false
}

fn workspace_has_test_surface(root: &Path) -> bool {
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if surface_scan_excluded(root, &path) {
                continue;
            }
            match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => {
                    if path.file_name().and_then(|value| value.to_str()) == Some("tests")
                        && directory_has_test_file(&path)
                    {
                        return true;
                    }
                    stack.push(path);
                }
                Ok(file_type) if file_type.is_file() => {
                    if path.extension().and_then(|value| value.to_str()) != Some("rs") {
                        continue;
                    }
                    let Ok(metadata) = entry.metadata() else {
                        continue;
                    };
                    if metadata.len() > 1024 * 1024 {
                        continue;
                    }
                    if fs::read_to_string(path)
                        .is_ok_and(|contents| rust_source_has_test_attribute(&contents))
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

fn surface_scan_excluded(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .any(|component| {
            let Component::Normal(value) = component else {
                return false;
            };
            matches!(
                value.to_string_lossy().as_ref(),
                ".git"
                    | ".kiana"
                    | "target"
                    | "reference"
                    | "node_modules"
                    | "vendor"
                    | "generated"
                    | "dist"
                    | "build"
                    | "docs"
                    | "testdata"
                    | "fixtures"
            )
        })
}

fn package_has_test_script(path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&contents) else {
        return false;
    };
    value
        .get("scripts")
        .and_then(|scripts| scripts.get("test"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|script| {
            !script.is_empty()
                && !script.contains("no test specified")
                && !script.eq_ignore_ascii_case("true")
        })
}

fn scan_suspicious_markers(root: &Path) -> Vec<ReviewFinding> {
    const MAX_FILE_BYTES: u64 = 1024 * 1024;
    const MAX_FINDINGS: usize = 200;
    const MAX_MARKER_FINDINGS: usize = MAX_FINDINGS - 1;
    const MARKERS: [&str; 9] = [
        "todo",
        "fixme",
        "todo!",
        "unimplemented!",
        "placeholder",
        "replace-me",
        "stub",
        "fake implementation",
        "hardcoded",
    ];
    let mut findings = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if excluded_path(root, &path) {
                continue;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() || !auditable_extension(&path) {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) if metadata.len() <= MAX_FILE_BYTES => metadata,
                _ => continue,
            };
            if metadata.len() == 0 {
                continue;
            }
            let contents = match fs::read(&path) {
                Ok(bytes) if !bytes.contains(&0) => String::from_utf8_lossy(&bytes).into_owned(),
                _ => continue,
            };
            for (index, line) in contents.lines().enumerate() {
                let lower = line.to_ascii_lowercase();
                let Some(marker) = MARKERS.iter().find(|marker| lower.contains(**marker)) else {
                    continue;
                };
                if findings.len() >= MAX_MARKER_FINDINGS {
                    findings.push(ReviewFinding {
                        severity: ReviewSeverity::Medium,
                        confidence: 1.0,
                        category: "marker_scan_truncated".to_string(),
                        title: "代码标记扫描达到结果上限".to_string(),
                        file: None,
                        line: None,
                        evidence: format!(
                            "marker findings were capped at {MAX_MARKER_FINDINGS}; additional matches were omitted"
                        ),
                        risk: "审计结果只覆盖部分命中，剩余标记尚未完成分类".to_string(),
                        recommendation: "缩小扫描范围或先处理已报告标记后重新执行严格审计"
                            .to_string(),
                        owner: "engineering".to_string(),
                        decision: "review".to_string(),
                        next_action: "continue_marker_audit".to_string(),
                    });
                    return findings;
                }
                let evidence = redact_and_bound_evidence_text(line, 240);
                findings.push(ReviewFinding {
                    severity: ReviewSeverity::Medium,
                    confidence: 0.75,
                    category: "suspicious_marker".to_string(),
                    title: format!("发现需要分类的代码标记：{marker}"),
                    file: Some(project_relative_path(root, &path)),
                    line: Some((index + 1) as u32),
                    evidence: evidence.text.trim().to_string(),
                    risk: "标记可能代表未完成实现、测试替身或发布前占位内容".to_string(),
                    recommendation:
                        "人工确认该标记是否位于生产路径，并选择 FIX、SKIP 或记录接受理由"
                            .to_string(),
                    owner: "engineering".to_string(),
                    decision: "review".to_string(),
                    next_action: "triage_marker".to_string(),
                });
            }
        }
    }
    findings
}

fn excluded_path(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative == Path::new("kiana-commands/src/audit.rs") {
        return true;
    }
    relative.components().any(|component| {
        let Component::Normal(value) = component else {
            return false;
        };
        matches!(
            value.to_string_lossy().as_ref(),
            ".git"
                | ".kiana"
                | "target"
                | "reference"
                | "node_modules"
                | "vendor"
                | "generated"
                | "dist"
                | "build"
                | "docs"
                | "tests"
                | "testdata"
                | "fixtures"
        )
    })
}

fn auditable_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some(
            "rs" | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "py"
                | "sh"
                | "bash"
                | "toml"
                | "yaml"
                | "yml"
                | "json"
        )
    )
}

fn read_commercial_blockers(root: &Path) -> (Option<CommercialSummary>, Vec<ReviewFinding>) {
    const TIMEOUT: Duration = Duration::from_secs(30);
    const OUTPUT_LIMIT: usize = 1024 * 1024;
    let script = root.join("scripts/commercial-release-blockers-report.sh");
    if !script.is_file() {
        return (None, Vec::new());
    }
    let mut command = ProcessCommand::new("bash");
    command.arg(&script).arg("--json").current_dir(root);
    let output = match run_bounded_process(&mut command, TIMEOUT, OUTPUT_LIMIT) {
        Ok(output) => output,
        Err(error) => {
            return (
                None,
                vec![block_finding(
                    "commercial_report_failed",
                    "商业 blocker 报告无法启动",
                    Some("scripts/commercial-release-blockers-report.sh".to_string()),
                    None,
                    error.to_string(),
                    "修复报告脚本运行环境后重新审计",
                    "release-owner",
                )],
            );
        }
    };
    if output.timed_out {
        return (
            None,
            vec![block_finding(
                "commercial_report_timeout",
                "商业 blocker 报告执行超时",
                Some("scripts/commercial-release-blockers-report.sh".to_string()),
                None,
                format!("report exceeded {} seconds", TIMEOUT.as_secs()),
                "缩短报告执行时间或拆分重量级检查后重新审计",
                "release-owner",
            )],
        );
    }
    if output.stdout_truncated || output.stderr_truncated {
        return (
            None,
            vec![block_finding(
                "commercial_report_output_limit",
                "商业 blocker 报告输出超过上限",
                Some("scripts/commercial-release-blockers-report.sh".to_string()),
                None,
                format!("stdout/stderr exceeded {OUTPUT_LIMIT} bytes"),
                "减少报告输出并保留机器可读 JSON 后重新审计",
                "release-owner",
            )],
        );
    }
    if !output.status.success() {
        return (
            None,
            vec![block_finding(
                "commercial_report_failed",
                "商业 blocker 报告执行失败",
                Some("scripts/commercial-release-blockers-report.sh".to_string()),
                None,
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
                "修复脚本错误后重新审计",
                "release-owner",
            )],
        );
    }
    let report: CommercialReportContract = match serde_json::from_slice(&output.stdout) {
        Ok(report) => report,
        Err(error) => {
            return (
                None,
                vec![block_finding(
                    "commercial_report_invalid",
                    "商业 blocker 报告不是有效 JSON",
                    Some("scripts/commercial-release-blockers-report.sh".to_string()),
                    None,
                    error.to_string(),
                    "修复脚本输出契约后重新审计",
                    "release-owner",
                )],
            );
        }
    };
    let summary = match validate_commercial_report(&report) {
        Ok(summary) => summary,
        Err(error) => {
            return (
                None,
                vec![block_finding(
                    "commercial_report_invalid",
                    "商业 blocker 报告违反数据契约",
                    Some("scripts/commercial-release-blockers-report.sh".to_string()),
                    None,
                    error,
                    "修复 schema、枚举值和 summary/checks 一致性后重新审计",
                    "release-owner",
                )],
            );
        }
    };
    let findings = report
        .checks
        .iter()
        .filter(|check| check.status == "blocking")
        .map(commercial_finding)
        .collect();
    (Some(summary), findings)
}

fn validate_commercial_report(
    report: &CommercialReportContract,
) -> Result<CommercialSummary, String> {
    const SCOPES: [&str; 7] = [
        "local-automation",
        "release-owner",
        "release-security",
        "release-environment",
        "final-artifact-derived",
        "live-service",
        "acceptance-owner",
    ];
    if report.schema != "kiana.commercial-release-blockers.v1" {
        return Err(format!("unexpected schema {}", report.schema));
    }
    for (field, value) in [
        ("version", report.version.as_str()),
        ("generated_at", report.generated_at.as_str()),
        ("release_tag", report.release_tag.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{field} must not be empty"));
        }
    }
    if !matches!(report.status.as_str(), "ready" | "blocked") {
        return Err(format!("invalid status {}", report.status));
    }
    let version_parts = report
        .release_tag
        .strip_prefix('v')
        .map(|value| value.split('.').take(3).collect::<Vec<_>>())
        .unwrap_or_default();
    if version_parts.len() != 3
        || version_parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|value| value.is_ascii_digit()))
    {
        return Err(format!("invalid release_tag {}", report.release_tag));
    }

    let mut blocking = 0usize;
    let mut external_blocking = 0usize;
    let mut scope_counts = std::collections::BTreeMap::<&str, usize>::new();
    for check in &report.checks {
        let valid_id = check
            .id
            .bytes()
            .next()
            .is_some_and(|value| value.is_ascii_lowercase() || value.is_ascii_digit())
            && check.id.bytes().all(|value| {
                value.is_ascii_lowercase() || value.is_ascii_digit() || matches!(value, b'.' | b'-')
            });
        if !valid_id {
            return Err(format!("invalid check id {}", check.id));
        }
        if !matches!(
            check.category.as_str(),
            "source-control"
                | "build-test"
                | "signing"
                | "distribution"
                | "live-service"
                | "acceptance"
        ) {
            return Err(format!("invalid category for {}", check.id));
        }
        if !matches!(check.severity.as_str(), "blocker" | "warning" | "info") {
            return Err(format!("invalid severity for {}", check.id));
        }
        if !matches!(check.status.as_str(), "satisfied" | "blocking") {
            return Err(format!("invalid status for {}", check.id));
        }
        if !matches!(
            check.owner_status.as_str(),
            "local-owner" | "role-owner-required" | "specific-owner-assigned"
        ) {
            return Err(format!("invalid owner_status for {}", check.id));
        }
        if !SCOPES.contains(&check.resolution_scope.as_str()) {
            return Err(format!("invalid resolution_scope for {}", check.id));
        }
        for (field, value) in [
            ("title", check.title.as_str()),
            ("gate", check.gate.as_str()),
            ("required_action", check.required_action.as_str()),
            ("owner", check.owner.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{field} must not be empty for {}", check.id));
            }
        }
        if check.status == "blocking" {
            blocking += 1;
            external_blocking += usize::from(check.external);
            *scope_counts
                .entry(check.resolution_scope.as_str())
                .or_insert(0) += 1;
        }
    }
    let total_checks = report.checks.len();
    let satisfied = total_checks.saturating_sub(blocking);
    let local_blocking = blocking.saturating_sub(external_blocking);
    let expected_status = if blocking == 0 { "ready" } else { "blocked" };
    let summary = &report.summary;
    if summary.total_checks != total_checks
        || summary.satisfied != satisfied
        || summary.blocking != blocking
        || summary.external_blocking != external_blocking
        || summary.local_blocking != local_blocking
        || report.status != expected_status
    {
        return Err("summary counts or report status do not match checks".to_string());
    }
    let reported_scopes = [
        summary.blocking_by_resolution_scope.local_automation,
        summary.blocking_by_resolution_scope.release_owner,
        summary.blocking_by_resolution_scope.release_security,
        summary.blocking_by_resolution_scope.release_environment,
        summary.blocking_by_resolution_scope.final_artifact_derived,
        summary.blocking_by_resolution_scope.live_service,
        summary.blocking_by_resolution_scope.acceptance_owner,
    ];
    if SCOPES
        .iter()
        .zip(reported_scopes)
        .any(|(scope, reported)| scope_counts.get(scope).copied().unwrap_or(0) != reported)
    {
        return Err("blocking_by_resolution_scope does not match checks".to_string());
    }
    Ok(CommercialSummary {
        schema: report.schema.clone(),
        status: report.status.clone(),
        blocking,
        external_blocking,
        local_blocking,
        satisfied,
        total_checks,
    })
}

fn commercial_finding(check: &CommercialCheckContract) -> ReviewFinding {
    ReviewFinding {
        severity: ReviewSeverity::Block,
        confidence: 1.0,
        category: "commercial_release".to_string(),
        title: check.title.clone(),
        file: check.paths.first().cloned(),
        line: None,
        evidence: check.evidence.clone(),
        risk: if check.external {
            "外部发布责任方或真实环境证据尚未闭环".to_string()
        } else {
            "本地可解决的商业发布门禁尚未闭环".to_string()
        },
        recommendation: check.required_action.clone(),
        owner: check.owner.clone(),
        decision: "block".to_string(),
        next_action: format!("resolve_commercial_blocker:{}", check.id),
    }
}

fn run_bounded_process(
    command: &mut ProcessCommand,
    timeout: Duration,
    output_limit: usize,
) -> io::Result<BoundedProcessOutput> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("child stdout pipe unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("child stderr pipe unavailable"))?;
    let stdout_reader = thread::spawn(move || read_bounded_stream(stdout, output_limit));
    let stderr_reader = thread::spawn(move || read_bounded_stream(stderr, output_limit));
    let started = Instant::now();
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(20));
    };
    let (stdout, stdout_truncated) = join_bounded_reader(stdout_reader)?;
    let (stderr, stderr_truncated) = join_bounded_reader(stderr_reader)?;
    Ok(BoundedProcessOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        timed_out,
    })
}

fn read_bounded_stream(mut reader: impl Read, output_limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::with_capacity(output_limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = output_limit.saturating_sub(output.len());
        let retained = read.min(remaining);
        output.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    Ok((output, truncated))
}

fn join_bounded_reader(
    reader: thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
) -> io::Result<(Vec<u8>, bool)> {
    reader
        .join()
        .map_err(|_| io::Error::other("bounded output reader panicked"))?
}

fn block_finding(
    category: &str,
    title: &str,
    file: Option<String>,
    line: Option<u32>,
    evidence: String,
    recommendation: &str,
    owner: &str,
) -> ReviewFinding {
    ReviewFinding {
        severity: ReviewSeverity::Block,
        confidence: 1.0,
        category: category.to_string(),
        title: title.to_string(),
        file,
        line,
        evidence,
        risk: "审计事实不完整或商业发布门禁未满足".to_string(),
        recommendation: recommendation.to_string(),
        owner: owner.to_string(),
        decision: "fix".to_string(),
        next_action: format!("resolve_{category}"),
    }
}

fn sort_findings(findings: &mut [ReviewFinding]) {
    findings.sort_by(|left, right| {
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then_with(|| left.category.cmp(&right.category))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.title.cmp(&right.title))
    });
}

fn severity_rank(severity: ReviewSeverity) -> u8 {
    match severity {
        ReviewSeverity::Block => 0,
        ReviewSeverity::High => 1,
        ReviewSeverity::Medium => 2,
        ReviewSeverity::Low => 3,
        ReviewSeverity::Note => 4,
    }
}

fn audit_score(findings: &[ReviewFinding]) -> f32 {
    let penalty = findings
        .iter()
        .map(|finding| match finding.severity {
            ReviewSeverity::Block => 20.0,
            ReviewSeverity::High => 10.0,
            ReviewSeverity::Medium => 4.0,
            ReviewSeverity::Low => 1.0,
            ReviewSeverity::Note => 0.0,
        })
        .sum::<f32>();
    (100.0 - penalty).max(0.0)
}

fn audit_recommendation(status: &str) -> &'static str {
    match status {
        "blocked" => "先关闭 blocking findings，再重新执行严格审计",
        "review_required" => "逐项完成 FIX、SKIP 或接受风险决策",
        _ => "当前严格审计未发现阻塞项，可继续下一交付门禁",
    }
}

fn parse_audit_args(raw: &str) -> Result<AuditArgs> {
    let mut args = AuditArgs::default();
    let tokens = raw.split_whitespace().collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index] {
            "--json" | "-j" => args.json_output = true,
            "--workflow" => {
                index += 1;
                set_audit_option_once(
                    &mut args.workflow_id,
                    required_value(&tokens, index, "--workflow")?,
                    "--workflow",
                )?;
            }
            "--task-list" => {
                index += 1;
                set_audit_option_once(
                    &mut args.task_list_id,
                    required_value(&tokens, index, "--task-list")?,
                    "--task-list",
                )?;
            }
            token if token.starts_with("--workflow=") => {
                set_audit_option_once(
                    &mut args.workflow_id,
                    value_after_equals(token, "--workflow")?,
                    "--workflow",
                )?;
            }
            token if token.starts_with("--task-list=") => {
                set_audit_option_once(
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

fn set_audit_option_once(slot: &mut Option<String>, value: String, option: &str) -> Result<()> {
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
        Err(_) => audit_fallback_workflow_runs(root)?,
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

fn audit_fallback_workflow_runs(root: &Path) -> Result<Vec<WorkflowRunSummary>> {
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

fn format_audit_report(report: &StrictAuditReport) -> String {
    format!(
        "严格审计报告\n工作流：{}\n运行：{}\n状态：{}\n评分：{:.1}\n阻塞项：{}\n发现项：{}\nReviewPacket：{}\n下一步：{}",
        report.workflow_id,
        report.run_id,
        report.status,
        report.score,
        report.blocking_count,
        report.finding_count,
        report.packet_path.as_deref().unwrap_or("未持久化"),
        report.next_action
    )
}

fn project_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
    "Usage: kiana audit strict [--json] [--workflow <run_id>] [--task-list <id>]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_process_enforces_timeout() {
        let mut command = ProcessCommand::new("bash");
        command.arg("-c").arg("sleep 5");
        let started = Instant::now();
        let output = run_bounded_process(&mut command, Duration::from_millis(50), 64).unwrap();

        assert!(output.timed_out);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn bounded_process_enforces_output_limit() {
        let mut command = ProcessCommand::new("bash");
        command.arg("-c").arg("printf '%01024d' 0");
        let output = run_bounded_process(&mut command, Duration::from_secs(2), 64).unwrap();

        assert!(!output.timed_out);
        assert!(output.status.success());
        assert!(output.stdout_truncated);
        assert_eq!(output.stdout.len(), 64);
    }

    #[test]
    fn parse_audit_args_rejects_duplicate_workflow() {
        let error = parse_audit_args("--workflow run-a --workflow run-b")
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate option --workflow"));
    }

    #[test]
    fn parse_audit_args_rejects_duplicate_task_list() {
        let error = parse_audit_args("--task-list first --task-list second")
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate option --task-list"));
    }
}
