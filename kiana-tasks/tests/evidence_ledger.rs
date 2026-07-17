use kiana_tasks::{
    append_evidence_event, build_verification_packet, read_evidence_events,
    redact_and_bound_evidence_text, unresolved_blocking_evidence,
    validate_verification_packet_integrity, write_verification_packet, EvidenceEventDraft,
    EvidenceKind, EvidenceSource, EvidenceStatus, VerificationCheck, VerificationStatus,
    WorkflowInit, WorkflowInputKind, WorkflowProfile,
};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-evidence-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn workflow_root(label: &str) -> (PathBuf, kiana_tasks::WorkflowRun) {
    let root = temp_root(label);
    std::fs::create_dir_all(&root).unwrap();
    let run = kiana_tasks::initialize_workflow_run(
        &root,
        WorkflowInit {
            request: format!("Evidence test {label}"),
            input_kind: WorkflowInputKind::Qa,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    (root, run)
}

fn source() -> EvidenceSource {
    EvidenceSource {
        source_type: "local_command".to_string(),
        name: "kiana validate".to_string(),
        actor: Some("coordinator".to_string()),
    }
}

fn draft(
    run: &kiana_tasks::WorkflowRun,
    event_id: &str,
    status: EvidenceStatus,
    exit_code: Option<i32>,
) -> EvidenceEventDraft {
    EvidenceEventDraft {
        event_id: Some(event_id.to_string()),
        workflow_id: run.workflow_id.clone(),
        run_id: run.run_id.clone(),
        task_id: Some("task_test".to_string()),
        workpacket_id: None,
        recorded_at_ms: Some(100),
        kind: EvidenceKind::TestResult,
        status,
        summary: "cargo test completed".to_string(),
        source: source(),
        payload: json!({
            "check_id": "cargo_test",
            "command": "cargo test --workspace",
            "exit_code": exit_code,
            "stdout_tail": "test result",
            "stderr_tail": ""
        }),
        changed_files: Vec::new(),
        confidence: Some(1.0),
        severity: None,
        next_action: Some("review".to_string()),
        supersedes_event_id: None,
    }
}

fn check(
    check_id: &str,
    required: bool,
    status: EvidenceStatus,
    evidence_id: Option<&str>,
    reason: Option<&str>,
) -> VerificationCheck {
    VerificationCheck {
        check_id: check_id.to_string(),
        description: format!("check {check_id}"),
        required,
        status,
        evidence_id: evidence_id.map(str::to_string),
        command: Some(format!("run {check_id}")),
        reason: reason.map(str::to_string),
    }
}

#[test]
fn evidence_event_validation_rejects_invalid_identity_source_and_status_payloads() {
    let (root, run) = workflow_root("invalid");

    let mut invalid = draft(&run, "evt_empty_workflow", EvidenceStatus::Pass, Some(0));
    invalid.workflow_id.clear();
    assert!(append_evidence_event(&run.artifact_dir, invalid)
        .unwrap_err()
        .to_string()
        .contains("workflow_id"));

    let mut invalid = draft(&run, "evt_empty_source", EvidenceStatus::Pass, Some(0));
    invalid.source.name.clear();
    assert!(append_evidence_event(&run.artifact_dir, invalid)
        .unwrap_err()
        .to_string()
        .contains("source.name"));

    let mut invalid = draft(&run, "evt_confidence", EvidenceStatus::Pass, Some(0));
    invalid.confidence = Some(1.5);
    assert!(append_evidence_event(&run.artifact_dir, invalid)
        .unwrap_err()
        .to_string()
        .contains("confidence"));

    let mut invalid = draft(&run, "evt_skipped", EvidenceStatus::Skipped, None);
    invalid.payload = json!({"check_id": "cargo_test"});
    assert!(append_evidence_event(&run.artifact_dir, invalid)
        .unwrap_err()
        .to_string()
        .contains("reason"));

    let invalid = draft(&run, "evt_missing_exit", EvidenceStatus::Pass, None);
    assert!(append_evidence_event(&run.artifact_dir, invalid)
        .unwrap_err()
        .to_string()
        .contains("exit_code"));

    assert_eq!(read_evidence_events(&run.artifact_dir).unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ledger_projection_is_monotonic_restart_safe_and_detects_duplicates_and_corrupt_tail() {
    let (root, run) = workflow_root("append");

    let first = append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_first", EvidenceStatus::Pass, Some(0)),
    )
    .unwrap();
    let second = append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_second", EvidenceStatus::Fail, Some(1)),
    )
    .unwrap();
    assert_eq!(second.sequence, first.sequence + 1);

    let restored = read_evidence_events(&run.artifact_dir).unwrap();
    assert_eq!(restored.len(), 2);
    assert_eq!(restored[0].event_id, "evt_first");
    assert_eq!(restored[1].event_id, "evt_second");

    assert!(append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_second", EvidenceStatus::Fail, Some(1))
    )
    .unwrap_err()
    .to_string()
    .contains("duplicate"));

    let eventlog_path = run.artifact_dir.join("eventlog.jsonl");
    let mut eventlog = OpenOptions::new()
        .append(true)
        .open(&eventlog_path)
        .unwrap();
    eventlog.write_all(b"{not-json}\n").unwrap();
    eventlog.flush().unwrap();

    let error = read_evidence_events(&run.artifact_dir)
        .unwrap_err()
        .to_string();
    assert!(error.contains("line 5"), "unexpected error: {error}");
    assert!(error.contains("eventlog"), "unexpected error: {error}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn verification_packet_applies_required_check_precedence() {
    let (root, run) = workflow_root("verification");

    let pass = append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_pass", EvidenceStatus::Pass, Some(0)),
    )
    .unwrap();
    let fail = append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_fail", EvidenceStatus::Fail, Some(1)),
    )
    .unwrap();
    let blocked = append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_blocked", EvidenceStatus::Blocked, None),
    )
    .unwrap();
    let mut skipped_draft = draft(
        &run,
        "evt_skipped_with_reason",
        EvidenceStatus::Skipped,
        None,
    );
    skipped_draft.payload["reason"] = json!("tool unavailable");
    let skipped = append_evidence_event(&run.artifact_dir, skipped_draft).unwrap();
    let events = vec![pass, fail, blocked, skipped];

    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        Some("task_test".to_string()),
        "project_p0",
        vec![check(
            "required_pass",
            true,
            EvidenceStatus::Pass,
            Some("evt_pass"),
            None,
        )],
        &events,
    )
    .unwrap();
    assert_eq!(packet.final_status, VerificationStatus::Pass);

    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![
            check(
                "required_pass",
                true,
                EvidenceStatus::Pass,
                Some("evt_pass"),
                None,
            ),
            check(
                "required_fail",
                true,
                EvidenceStatus::Fail,
                Some("evt_fail"),
                None,
            ),
        ],
        &events,
    )
    .unwrap();
    assert_eq!(packet.final_status, VerificationStatus::Fail);

    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![check(
            "required_blocked",
            true,
            EvidenceStatus::Blocked,
            Some("evt_blocked"),
            Some("dependency unavailable"),
        )],
        &events,
    )
    .unwrap();
    assert_eq!(packet.final_status, VerificationStatus::Blocked);

    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![check(
            "required_skipped",
            true,
            EvidenceStatus::Skipped,
            Some("evt_skipped_with_reason"),
            Some("tool unavailable"),
        )],
        &events,
    )
    .unwrap();
    assert_eq!(packet.final_status, VerificationStatus::Inconclusive);

    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![
            check(
                "required_pass",
                true,
                EvidenceStatus::Pass,
                Some("evt_pass"),
                None,
            ),
            check(
                "optional_fail",
                false,
                EvidenceStatus::Fail,
                Some("evt_fail"),
                None,
            ),
        ],
        &events,
    )
    .unwrap();
    assert_eq!(packet.final_status, VerificationStatus::Pass);

    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![check(
            "optional_only",
            false,
            EvidenceStatus::Pass,
            Some("evt_pass"),
            None,
        )],
        &events,
    )
    .unwrap();
    assert_eq!(packet.final_status, VerificationStatus::Blocked);
    assert_eq!(packet.next_action, "add_required_checks");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evidence_output_is_bounded_and_redacts_common_secret_shapes() {
    let input = "Authorization: Bearer top-secret\nAPI_KEY=abc123\nvisible-line\n";
    let value = redact_and_bound_evidence_text(input, 48);

    assert!(value.redacted);
    assert!(value.truncated);
    assert_eq!(value.original_bytes, input.len());
    assert!(!value.text.contains("top-secret"));
    assert!(!value.text.contains("abc123"));
    assert!(value.text.contains("[REDACTED]"));
}

#[test]
fn evidence_output_keeps_the_utf8_safe_tail_with_the_failure_summary() {
    let input = format!(
        "{}Authorization: Bearer top-secret\n测试失败: assertion failed\n",
        "compile output\n".repeat(64)
    );

    let value = redact_and_bound_evidence_text(&input, 96);

    assert!(value.truncated);
    assert!(value.redacted);
    assert!(value.text.len() <= 96);
    assert!(value.text.is_char_boundary(value.text.len()));
    assert!(!value.text.contains("top-secret"));
    assert!(value.text.contains("测试失败: assertion failed"));
    assert!(value.text.ends_with("测试失败: assertion failed\n"));
}

#[test]
fn verification_packet_integrity_rejects_forged_pass_and_ledger_mismatches() {
    let (root, run) = workflow_root("packet-integrity");
    let event = append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_integrity", EvidenceStatus::Pass, Some(0)),
    )
    .unwrap();
    let events = read_evidence_events(&run.artifact_dir).unwrap();
    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        Some("task_test".to_string()),
        "project_p0",
        vec![check(
            "required_pass",
            true,
            EvidenceStatus::Pass,
            Some(&event.event_id),
            None,
        )],
        &events,
    )
    .unwrap();

    validate_verification_packet_integrity(&packet, &events).unwrap();

    let mut forged = packet.clone();
    forged.checks.clear();
    assert!(validate_verification_packet_integrity(&forged, &events)
        .unwrap_err()
        .to_string()
        .contains("required check"));

    let mut forged = packet.clone();
    forged.pass_count += 1;
    assert!(validate_verification_packet_integrity(&forged, &events)
        .unwrap_err()
        .to_string()
        .contains("pass_count"));

    let mut forged = packet.clone();
    forged.checks[0].evidence_id = Some("missing-event".to_string());
    assert!(validate_verification_packet_integrity(&forged, &events)
        .unwrap_err()
        .to_string()
        .contains("missing evidence"));

    let mut forged = packet.clone();
    forged.task_id = Some("another-task".to_string());
    assert!(validate_verification_packet_integrity(&forged, &events)
        .unwrap_err()
        .to_string()
        .contains("task"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn verification_packet_writer_rejects_unsafe_ids_and_preserves_immutable_artifacts() {
    let (root, run) = workflow_root("packet-writer");
    let event = append_evidence_event(
        &run.artifact_dir,
        draft(&run, "evt_packet_writer", EvidenceStatus::Pass, Some(0)),
    )
    .unwrap();
    let events = read_evidence_events(&run.artifact_dir).unwrap();
    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        Some("task_test".to_string()),
        "project_p0",
        vec![check(
            "required_pass",
            true,
            EvidenceStatus::Pass,
            Some(&event.event_id),
            None,
        )],
        &events,
    )
    .unwrap();

    for unsafe_id in [
        "../state",
        "/absolute",
        "nested/path",
        r"nested\path",
        ".",
        "vp-é",
    ] {
        let mut unsafe_packet = packet.clone();
        unsafe_packet.verification_id = unsafe_id.to_string();
        let error = write_verification_packet(&run.artifact_dir, &unsafe_packet)
            .unwrap_err()
            .to_string();
        assert!(error.contains("verification_id"), "{unsafe_id}: {error}");
    }
    assert!(run.artifact_dir.join("state.json").is_file());

    let mut immutable = packet.clone();
    immutable.verification_id = "vp_immutable".to_string();
    let path = write_verification_packet(&run.artifact_dir, &immutable).unwrap();
    let original = std::fs::read(&path).unwrap();

    let mut replacement = immutable.clone();
    replacement.created_at_ms += 1;
    let error = write_verification_packet(&run.artifact_dir, &replacement)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already exists"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), original);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unresolved_blocking_evidence_tracks_only_active_supersedes_chain_tails() {
    let (root, run) = workflow_root("active-blockers");
    let blocker = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_blocker_old".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: Some("task_test".to_string()),
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::BlockerRecord,
            status: EvidenceStatus::Blocked,
            summary: "dependency unavailable".to_string(),
            source: source(),
            payload: json!({"reason": "dependency unavailable"}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: Some("block".to_string()),
            next_action: Some("restore_dependency".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_review_active".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: None,
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::ReviewFinding,
            status: EvidenceStatus::Unknown,
            summary: "security review blocks release".to_string(),
            source: source(),
            payload: json!({}),
            changed_files: vec![],
            confidence: Some(0.9),
            severity: Some("block".to_string()),
            next_action: Some("fix_security_review".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_blocker_resolved".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: Some("task_test".to_string()),
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::BlockerRecord,
            status: EvidenceStatus::Pass,
            summary: "dependency restored".to_string(),
            source: source(),
            payload: json!({}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: None,
            next_action: Some("continue".to_string()),
            supersedes_event_id: Some(blocker.event_id),
        },
    )
    .unwrap();

    let events = read_evidence_events(&run.artifact_dir).unwrap();
    let unresolved = unresolved_blocking_evidence(&events, &run.workflow_id, &run.run_id);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].event_id, "evt_review_active");

    let _ = std::fs::remove_dir_all(root);
}
