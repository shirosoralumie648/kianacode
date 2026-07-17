use kiana_tasks::{
    append_evidence_event, initialize_local_hmac_key, initialize_workflow_run,
    inspect_workflow_integrity, list_review_packets, read_review_packet, read_workflow_events,
    write_review_packet, EvidenceEventDraft, EvidenceKind, EvidenceSource, EvidenceStatus,
    ReviewFinding, ReviewPacket, ReviewSeverity, WorkflowEventKind, WorkflowInit,
    WorkflowInputKind, WorkflowProfile,
};
use serde_json::json;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

static INTEGRITY_ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    INTEGRITY_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct IntegrityEnvGuard {
    previous_home: Option<OsString>,
    previous_key_file: Option<OsString>,
}

impl IntegrityEnvGuard {
    fn isolated(home: &Path) -> Self {
        let previous_home = std::env::var_os("KIANA_HOME");
        let previous_key_file = std::env::var_os("KIANA_WORKFLOW_INTEGRITY_KEY_FILE");
        std::env::set_var("KIANA_HOME", home);
        std::env::remove_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE");
        Self {
            previous_home,
            previous_key_file,
        }
    }
}

impl Drop for IntegrityEnvGuard {
    fn drop(&mut self) {
        match self.previous_home.take() {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        match self.previous_key_file.take() {
            Some(value) => std::env::set_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", value),
            None => std::env::remove_var("KIANA_WORKFLOW_INTEGRITY_KEY_FILE"),
        }
    }
}

fn root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "kiana-review-packet-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn review_packet_round_trips_and_records_workflow_event() {
    let _lock = env_lock();
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Strict audit".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let evidence = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_review_finding".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: None,
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::ReviewFinding,
            status: EvidenceStatus::Unknown,
            summary: "Production placeholder".to_string(),
            source: EvidenceSource {
                source_type: "local_audit".to_string(),
                name: "fixture".to_string(),
                actor: Some("reviewer".to_string()),
            },
            payload: json!({"file": "src/main.rs", "line": 7}),
            changed_files: vec![],
            confidence: Some(0.95),
            severity: Some("high".to_string()),
            next_action: Some("triage_finding".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    let packet = ReviewPacket {
        schema: "kiana.review-packet.v1".to_string(),
        id: "rv_test".to_string(),
        workflow_id: run.workflow_id.clone(),
        run_id: run.run_id.clone(),
        mode: "strict".to_string(),
        reviewer_type: "commercial_readiness".to_string(),
        scope: vec!["repository".to_string()],
        findings: vec![ReviewFinding {
            severity: ReviewSeverity::High,
            confidence: 0.95,
            category: "placeholder".to_string(),
            title: "Production placeholder".to_string(),
            file: Some("src/main.rs".to_string()),
            line: Some(7),
            evidence: "TODO replace placeholder".to_string(),
            risk: "Incomplete production behavior".to_string(),
            recommendation: "Implement or classify the marker".to_string(),
            owner: "engineering".to_string(),
            decision: "review".to_string(),
            next_action: "triage_finding".to_string(),
        }],
        score: 80.0,
        blocking_count: 0,
        recommendation: "Review marker before release".to_string(),
        decision: "review_required".to_string(),
        evidence_refs: vec![evidence.event_id],
        created_at_ms: 1,
        next_action: "triage_findings".to_string(),
    };

    let path = write_review_packet(&run.artifact_dir, &packet).unwrap();
    assert_eq!(read_review_packet(&path).unwrap().id, packet.id);
    let packets = list_review_packets(&run.artifact_dir).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].1.decision, "review_required");
    let events = read_workflow_events(&run.artifact_dir).unwrap();
    assert_eq!(
        events.last().unwrap().kind,
        WorkflowEventKind::ReviewCompleted
    );
    let integrity = inspect_workflow_integrity(&run.artifact_dir).unwrap();
    assert_eq!(integrity.verified_artifact_count, 1);
    assert_eq!(integrity.orphan_artifact_count, 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn review_packet_rejects_untrusted_refs_unsafe_ids_and_duplicate_writes() {
    let _lock = env_lock();
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnvGuard::isolated(&root.join("home"));
    initialize_local_hmac_key().unwrap();
    let run = initialize_workflow_run(
        &root,
        WorkflowInit {
            request: "Review packet trust boundary".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap();
    let evidence = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_review_valid".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: None,
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::ReviewFinding,
            status: EvidenceStatus::Unknown,
            summary: "valid review evidence".to_string(),
            source: EvidenceSource {
                source_type: "local_audit".to_string(),
                name: "fixture".to_string(),
                actor: Some("reviewer".to_string()),
            },
            payload: json!({}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: Some("high".to_string()),
            next_action: Some("review".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    let finding = ReviewFinding {
        severity: ReviewSeverity::High,
        confidence: 1.0,
        category: "trust".to_string(),
        title: "Trust boundary".to_string(),
        file: None,
        line: None,
        evidence: evidence.event_id.clone(),
        risk: "invalid evidence linkage".to_string(),
        recommendation: "repair evidence linkage".to_string(),
        owner: "engineering".to_string(),
        decision: "review".to_string(),
        next_action: "review".to_string(),
    };
    let packet = ReviewPacket {
        schema: "kiana.review-packet.v1".to_string(),
        id: "rv_trusted".to_string(),
        workflow_id: run.workflow_id.clone(),
        run_id: run.run_id.clone(),
        mode: "strict".to_string(),
        reviewer_type: "commercial_readiness".to_string(),
        scope: vec!["repository".to_string()],
        findings: vec![finding],
        score: 90.0,
        blocking_count: 0,
        recommendation: "review".to_string(),
        decision: "review_required".to_string(),
        evidence_refs: vec![evidence.event_id],
        created_at_ms: 1,
        next_action: "review".to_string(),
    };

    let mut missing = packet.clone();
    missing.id = "rv_missing".to_string();
    missing.evidence_refs[0] = "evt_missing".to_string();
    assert!(write_review_packet(&run.artifact_dir, &missing)
        .unwrap_err()
        .to_string()
        .contains("missing"));

    let foreign = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_foreign".to_string()),
            workflow_id: "wf_foreign".to_string(),
            run_id: "run_foreign".to_string(),
            task_id: None,
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::ReviewFinding,
            status: EvidenceStatus::Unknown,
            summary: "foreign evidence".to_string(),
            source: EvidenceSource {
                source_type: "local_audit".to_string(),
                name: "fixture".to_string(),
                actor: Some("reviewer".to_string()),
            },
            payload: json!({}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: Some("high".to_string()),
            next_action: Some("review".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();
    let mut cross_run = packet.clone();
    cross_run.id = "rv_cross_run".to_string();
    cross_run.evidence_refs[0] = foreign.event_id;
    assert!(write_review_packet(&run.artifact_dir, &cross_run)
        .unwrap_err()
        .to_string()
        .contains("different workflow run"));

    let mut unsafe_packet = packet.clone();
    unsafe_packet.id = "../state".to_string();
    assert!(write_review_packet(&run.artifact_dir, &unsafe_packet)
        .unwrap_err()
        .to_string()
        .contains("review_id"));

    let path = write_review_packet(&run.artifact_dir, &packet).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut replacement = packet.clone();
    replacement.created_at_ms += 1;
    assert!(write_review_packet(&run.artifact_dir, &replacement)
        .unwrap_err()
        .to_string()
        .contains("already exists"));
    assert_eq!(std::fs::read(path).unwrap(), original);

    let _ = std::fs::remove_dir_all(root);
}
