use kiana_domain::{
    MaintenanceCandidate, MaintenanceDecision, MaintenanceObjectKind, MemoryMaintenancePlan,
    RetentionDisposition,
};
use std::fs;

const SCAN: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn candidate(
    id: &str,
    kind: MaintenanceObjectKind,
    bytes: u64,
    retention: RetentionDisposition,
    legal_hold: bool,
    event_reference: Option<&str>,
    receipt_reference: Option<&str>,
    orphan_detected: bool,
    recoverable_orphan: bool,
    decision: MaintenanceDecision,
) -> MaintenanceCandidate {
    let (generation, expiry, tombstone) = match kind {
        MaintenanceObjectKind::IndexGeneration => (1, None, None),
        MaintenanceObjectKind::SummaryArtifact
        | MaintenanceObjectKind::ResultArtifact
        | MaintenanceObjectKind::CacheEntry => (0, Some(900), None),
        MaintenanceObjectKind::Tombstone => (0, None, Some(4)),
    };
    MaintenanceCandidate::new(
        id,
        kind,
        generation,
        3,
        bytes,
        1_000,
        expiry,
        tombstone,
        5,
        retention,
        legal_hold,
        event_reference.map(str::to_owned),
        receipt_reference.map(str::to_owned),
        orphan_detected,
        recoverable_orphan,
        decision,
    )
    .expect("maintenance candidate")
}

fn plan() -> MemoryMaintenancePlan {
    MemoryMaintenancePlan::new(
        "maintenance-37",
        SCAN,
        20,
        4,
        9,
        1_000,
        1_000,
        1_200,
        vec![
            candidate(
                "index:generation-1",
                MaintenanceObjectKind::IndexGeneration,
                100,
                RetentionDisposition::Eligible,
                false,
                None,
                None,
                false,
                false,
                MaintenanceDecision::DeleteEligible,
            ),
            candidate(
                "artifact:summary-expired",
                MaintenanceObjectKind::SummaryArtifact,
                150,
                RetentionDisposition::Eligible,
                false,
                None,
                None,
                false,
                false,
                MaintenanceDecision::DeleteEligible,
            ),
            candidate(
                "artifact:retained-receipt",
                MaintenanceObjectKind::ResultArtifact,
                80,
                RetentionDisposition::Eligible,
                false,
                None,
                Some("receipt:run-1"),
                false,
                false,
                MaintenanceDecision::Retain,
            ),
            candidate(
                "cache:orphan",
                MaintenanceObjectKind::CacheEntry,
                70,
                RetentionDisposition::Eligible,
                false,
                None,
                None,
                true,
                true,
                MaintenanceDecision::ReportOrphan,
            ),
            candidate(
                "tombstone:4",
                MaintenanceObjectKind::Tombstone,
                50,
                RetentionDisposition::Eligible,
                false,
                None,
                None,
                false,
                false,
                MaintenanceDecision::DeleteEligible,
            ),
            candidate(
                "artifact:legal-hold",
                MaintenanceObjectKind::SummaryArtifact,
                60,
                RetentionDisposition::Held,
                true,
                Some("event:hold-1"),
                None,
                false,
                false,
                MaintenanceDecision::Retain,
            ),
        ],
    )
    .expect("maintenance plan")
}

#[test]
fn maintenance_plan_keeps_retained_evidence_and_reports_orphans() {
    let plan = plan();
    plan.validate().expect("valid maintenance plan");
    assert_eq!(
        plan.delete_candidates().expect("delete candidates"),
        vec![
            "index:generation-1".to_owned(),
            "artifact:summary-expired".to_owned(),
            "tombstone:4".to_owned()
        ]
    );
    assert_eq!(
        plan.orphan_candidates().expect("orphan candidates"),
        vec!["cache:orphan".to_owned()]
    );
    assert_eq!(plan.planned_reclaim_bytes, 300);
    assert!(plan.quota_satisfied_after_plan().expect("quota result"));
    assert!(plan.canonical_bytes().expect("canonical plan").len() > 512);
}

#[test]
fn maintenance_never_deletes_receipt_referenced_or_current_generation_material() {
    let referenced = MaintenanceCandidate::new(
        "artifact:referenced",
        MaintenanceObjectKind::ResultArtifact,
        0,
        3,
        10,
        1_000,
        Some(900),
        None,
        5,
        RetentionDisposition::Eligible,
        false,
        None,
        Some("receipt:kept".to_owned()),
        false,
        false,
        MaintenanceDecision::DeleteEligible,
    );
    assert_eq!(
        referenced.unwrap_err(),
        "memory_maintenance_delete_reference_or_retention_gate"
    );

    let current = MaintenanceCandidate::new(
        "index:generation-3",
        MaintenanceObjectKind::IndexGeneration,
        3,
        3,
        10,
        1_000,
        None,
        None,
        5,
        RetentionDisposition::Eligible,
        false,
        None,
        None,
        false,
        false,
        MaintenanceDecision::DeleteEligible,
    );
    assert_eq!(
        current.unwrap_err(),
        "memory_maintenance_delete_expiry_or_generation_gate"
    );
}

#[test]
fn orphan_report_requires_recoverable_no_reference_evidence() {
    let result = MaintenanceCandidate::new(
        "cache:unrecoverable-orphan",
        MaintenanceObjectKind::CacheEntry,
        0,
        3,
        10,
        1_000,
        Some(900),
        None,
        5,
        RetentionDisposition::Eligible,
        false,
        None,
        None,
        true,
        false,
        MaintenanceDecision::ReportOrphan,
    );
    assert_eq!(
        result.unwrap_err(),
        "memory_maintenance_orphan_report_invalid"
    );
}

#[test]
fn fixture_is_a_plan_only_and_contains_no_delete_instruction_or_path() {
    let raw = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cm37-memory-maintenance.json"
    ))
    .expect("maintenance fixture");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("fixture json");
    assert_eq!(value["schema"], "kiana.memory-maintenance-fixture.v1");
    assert_eq!(value["effect_authority"], "none");
    assert_eq!(value["orphan_action"], "report_and_recover");
    assert!(!raw.contains("delete_file") && !raw.contains("remove_file"));
    assert!(!raw.contains("/home/") && !raw.contains("/tmp/"));
}
