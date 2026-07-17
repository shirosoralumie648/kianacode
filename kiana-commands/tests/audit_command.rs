use kiana_commands::audit::AuditCommand;
use kiana_commands::{Command, CommandContext};
use kiana_tasks::{
    append_evidence_event, build_verification_packet, initialize_workflow_run, list_review_packets,
    read_evidence_events, write_verification_packet, EvidenceEventDraft, EvidenceKind,
    EvidenceSource, EvidenceStatus, VerificationCheck, WorkflowInit, WorkflowInputKind,
    WorkflowProfile,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]),
    }
}

fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "kiana-audit-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        ROOT_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn init(root: &Path) -> kiana_tasks::WorkflowRun {
    initialize_workflow_run(
        root,
        WorkflowInit {
            request: "Strict commercial audit".to_string(),
            input_kind: WorkflowInputKind::Review,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap()
}

#[tokio::test]
async fn audit_strict_requires_workflow() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let error = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("workflow_not_found"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_records_marker_finding_with_file_and_line() {
    let root = root();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "// TODO API_KEY=top-secret-value\nfn main() {}\n",
    )
    .unwrap();
    let run = init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["schema"], "kiana.strict-audit.v1");
    assert_eq!(report["status"], "review_required");
    assert_eq!(report["blocking_count"], 0);
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| {
            finding["category"] == "suspicious_marker"
                && finding["file"] == "src/main.rs"
                && finding["line"] == 1
                && !finding["evidence"]
                    .as_str()
                    .unwrap()
                    .contains("top-secret-value")
        }));
    assert!(!result.value.contains("top-secret-value"));
    assert!(root.join(report["packet_path"].as_str().unwrap()).is_file());
    assert_eq!(list_review_packets(&run.artifact_dir).unwrap().len(), 1);
    let events = read_evidence_events(&run.artifact_dir).unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == EvidenceKind::ReviewFinding));
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("top-secret-value"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_reports_marker_scan_truncation() {
    let root = root();
    std::fs::create_dir_all(root.join("src")).unwrap();
    for index in 0..205 {
        std::fs::write(
            root.join("src").join(format!("marker_{index:03}.rs")),
            format!("// TODO marker {index}\n"),
        )
        .unwrap();
    }
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["category"] == "marker_scan_truncated"));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_blocks_stale_packet_temporary_files() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = init(&root);
    std::fs::write(
        run.artifact_dir.join("verification/.vp_crashed.tmp"),
        b"partial verification packet",
    )
    .unwrap();
    std::fs::write(
        run.artifact_dir.join("review/.rv_crashed.tmp"),
        b"partial review packet",
    )
    .unwrap();

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert_eq!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["category"] == "stale_artifact_temp")
            .count(),
        2
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_preserves_commercial_local_and_external_blockers() {
    let root = root();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    let payload = json!({
        "schema": "kiana.commercial-release-blockers.v1",
        "version": "0.1.0",
        "generated_at": "2026-07-10T00:00:00Z",
        "status": "blocked",
        "release_tag": "v0.1.0",
        "summary": {
            "blocking": 2,
            "external_blocking": 1,
            "local_blocking": 1,
            "satisfied": 0,
            "total_checks": 2,
            "blocking_by_resolution_scope": {
                "local-automation": 1,
                "release-owner": 0,
                "release-security": 1,
                "release-environment": 0,
                "final-artifact-derived": 0,
                "live-service": 0,
                "acceptance-owner": 0
            }
        },
        "checks": [
            {
                "id": "local.test",
                "category": "build-test",
                "title": "Local test gate",
                "gate": "local tests pass",
                "status": "blocking",
                "severity": "blocker",
                "external": false,
                "owner": "engineering",
                "owner_status": "local-owner",
                "resolution_scope": "local-automation",
                "evidence": "tests missing",
                "required_action": "add tests",
                "paths": ["tests"],
                "commands": ["cargo test"],
                "env": [],
                "acceptance_artifacts": [],
                "verification_commands": ["cargo test"],
                "handoff_notes": []
            },
            {
                "id": "external.signing",
                "category": "signing",
                "title": "External signing gate",
                "gate": "signed release artifact",
                "status": "blocking",
                "severity": "blocker",
                "external": true,
                "owner": "release-security",
                "owner_status": "role-owner-required",
                "resolution_scope": "release-security",
                "evidence": "signature absent",
                "required_action": "provide signing proof",
                "paths": [],
                "commands": [],
                "env": [],
                "acceptance_artifacts": ["signature"],
                "verification_commands": [],
                "handoff_notes": []
            }
        ]
    });
    std::fs::write(
        root.join("scripts/commercial-release-blockers-report.sh"),
        format!("#!/usr/bin/env bash\ncat <<'JSON'\n{payload}\nJSON\n"),
    )
    .unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert_eq!(report["blocking_count"], 2);
    assert_eq!(report["commercial"]["local_blocking"], 1);
    assert_eq!(report["commercial"]["external_blocking"], 1);
    assert_eq!(
        report["commercial"]["schema"],
        "kiana.commercial-release-blockers.v1"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_blocks_invalid_commercial_report_contract() {
    for (label, payload) in [
        ("empty", "{}"),
        (
            "mismatched-counts",
            r#"{"schema":"kiana.commercial-release-blockers.v1","status":"blocked","summary":{"blocking":0,"external_blocking":0,"local_blocking":0,"satisfied":0,"total_checks":1},"checks":[{"id":"local.test","category":"test","title":"Local test gate","status":"blocking","severity":"blocker","external":false,"owner":"engineering","evidence":"tests missing","required_action":"add tests","paths":["tests"]}]}"#,
        ),
    ] {
        let root = root();
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(
            root.join("scripts/commercial-release-blockers-report.sh"),
            format!("#!/usr/bin/env bash\nprintf '%s\\n' '{}'\n", payload),
        )
        .unwrap();
        init(&root);

        let result = AuditCommand
            .execute(context("strict --json", &root))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();
        assert_eq!(report["status"], "blocked", "{label}");
        assert!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| finding["category"] == "commercial_report_invalid"),
            "{label}: {}",
            serde_json::to_string_pretty(&report).unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[tokio::test]
async fn audit_strict_reports_corrupt_ledger_without_claiming_persistence() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = init(&root);
    use std::io::Write;
    let mut eventlog = std::fs::OpenOptions::new()
        .append(true)
        .open(run.artifact_dir.join("eventlog.jsonl"))
        .unwrap();
    writeln!(eventlog, "{{not-json}}").unwrap();

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert_eq!(report["persistence_status"], "blocked");
    assert!(report["packet_path"].is_null());
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["category"] == "evidence_integrity"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_blocks_completed_task_without_verification_reference() {
    let root = root();
    std::fs::create_dir_all(root.join(".kiana/tasks/default")).unwrap();
    std::fs::write(
        root.join(".kiana/tasks/default/task-1.json"),
        serde_json::to_vec_pretty(&json!({
            "id": "task-1", "title": "Unverified done", "status": "completed",
            "blockedBy": [], "blocks": [], "created_at": 1, "updated_at": 2,
            "metadata": {"verification_commands": ["cargo test"]}
        }))
        .unwrap(),
    )
    .unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| { finding["file"] == ".kiana/tasks/default/task-1.json" }));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_blocks_required_skipped_check() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = init(&root);
    let packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![VerificationCheck {
            check_id: "required-live-gate".to_string(),
            description: "Required live gate".to_string(),
            required: true,
            status: EvidenceStatus::Skipped,
            evidence_id: None,
            command: Some("live-check".to_string()),
            reason: Some("credential unavailable".to_string()),
        }],
        &[],
    )
    .unwrap();
    write_verification_packet(&run.artifact_dir, &packet).unwrap();

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["category"] == "required_check_incomplete"),
        "{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_blocks_orphan_verification_packet() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = init(&root);
    let mut packet = build_verification_packet(
        &run.workflow_id,
        &run.run_id,
        None,
        "project_p0",
        vec![VerificationCheck {
            check_id: "external".to_string(),
            description: "external".to_string(),
            required: true,
            status: EvidenceStatus::Skipped,
            evidence_id: None,
            command: Some("external-check".to_string()),
            reason: Some("credential unavailable".to_string()),
        }],
        &[],
    )
    .unwrap();
    packet.verification_id = "vp_orphan_audit".to_string();
    std::fs::write(
        run.artifact_dir
            .join("verification")
            .join("vp_orphan_audit.json"),
        serde_json::to_vec_pretty(&packet).unwrap(),
    )
    .unwrap();

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["category"] == "verification_completion_missing"));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_reuses_unresolved_blocker_evidence_without_duplication() {
    let root = root();
    std::fs::create_dir_all(&root).unwrap();
    let run = init(&root);
    let blocker = append_evidence_event(
        &run.artifact_dir,
        EvidenceEventDraft {
            event_id: Some("evt_audit_blocker".to_string()),
            workflow_id: run.workflow_id.clone(),
            run_id: run.run_id.clone(),
            task_id: None,
            workpacket_id: None,
            recorded_at_ms: None,
            kind: EvidenceKind::BlockerRecord,
            status: EvidenceStatus::Blocked,
            summary: "customer acceptance missing".to_string(),
            source: EvidenceSource {
                source_type: "manual".to_string(),
                name: "fixture".to_string(),
                actor: Some("reviewer".to_string()),
            },
            payload: json!({"reason": "acceptance missing"}),
            changed_files: vec![],
            confidence: Some(1.0),
            severity: Some("block".to_string()),
            next_action: Some("collect_acceptance".to_string()),
            supersedes_event_id: None,
        },
    )
    .unwrap();

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    assert_eq!(report["status"], "blocked");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["category"] == "unresolved_evidence"));
    assert!(report["evidence_ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id == &Value::String(blocker.event_id.clone())));
    assert_eq!(
        read_evidence_events(&run.artifact_dir)
            .unwrap()
            .iter()
            .filter(|event| event.kind == EvidenceKind::BlockerRecord)
            .count(),
        1
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_blocks_applicable_project_without_test_surface() {
    let root = root();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"surface-fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let test_surface = report["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|surface| surface["id"] == "test")
        .unwrap();
    assert_eq!(test_surface["applicable"], true);
    assert_eq!(test_surface["status"], "missing");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["category"] == "surface:test"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_blocks_plugin_surface_without_policy_contract() {
    let root = root();
    std::fs::create_dir_all(root.join("plugins/example")).unwrap();
    std::fs::write(
        root.join("plugins/example/plugin.json"),
        "{\"name\":\"example\",\"version\":\"1.0.0\"}\n",
    )
    .unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let policy_surface = report["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|surface| surface["id"] == "policy")
        .unwrap();
    assert_eq!(policy_surface["applicable"], true);
    assert_eq!(policy_surface["status"], "missing");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["category"] == "surface:policy"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_detects_workspace_member_test_surface() {
    let root = root();
    std::fs::create_dir_all(root.join("crate-a/tests")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crate-a\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("crate-a/Cargo.toml"),
        "[package]\nname = \"crate-a\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("crate-a/tests/smoke.rs"),
        "#[test]\nfn smoke() {}\n",
    )
    .unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let test_surface = report["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|surface| surface["id"] == "test")
        .unwrap();
    assert_eq!(test_surface["status"], "ready");
    assert!(!report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["category"] == "surface:test"));
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_does_not_accept_tests_readme_as_test_surface() {
    let root = root();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"readme-only-tests\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n").unwrap();
    std::fs::write(root.join("tests/README.md"), "Tests will be added later.\n").unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let test_surface = report["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|surface| surface["id"] == "test")
        .unwrap();
    assert_eq!(test_surface["status"], "missing");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_ignores_commented_rust_test_attribute() {
    let root = root();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"commented-test\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        "// #[test]\n// fn fake_test() {}\npub fn value() -> u8 { 1 }\n",
    )
    .unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    let test_surface = report["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|surface| surface["id"] == "test")
        .unwrap();
    assert_eq!(test_surface["status"], "missing");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn audit_strict_does_not_accept_directories_as_security_policy_contracts() {
    let root = root();
    std::fs::create_dir_all(root.join("plugins/example")).unwrap();
    std::fs::write(
        root.join("plugins/example/plugin.json"),
        "{\"name\":\"example\",\"version\":\"1.0.0\"}\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("kiana-commands/src/permissions.rs")).unwrap();
    std::fs::create_dir_all(root.join("kiana-types/src/trust.rs")).unwrap();
    init(&root);

    let result = AuditCommand
        .execute(context("strict --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&result.value).unwrap();
    for surface_id in ["security", "policy"] {
        let surface = report["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|surface| surface["id"] == surface_id)
            .unwrap();
        assert_eq!(surface["status"], "missing", "surface={surface_id}");
    }
    let _ = std::fs::remove_dir_all(root);
}
