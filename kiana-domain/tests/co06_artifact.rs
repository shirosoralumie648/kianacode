use kiana_domain::{
    json_digest, ArtifactOrphanRetention, ArtifactOrphanRetentionDecision, ArtifactProvenance,
    ArtifactPublicationIntent, ArtifactPublicationReconciliation,
    ArtifactPublicationRetentionPlan, ArtifactPublicationRetentionPolicy, ArtifactPublicationState,
    ArtifactPublicationVerdict, ArtifactVersion, ArtifactWorkspaceDelta,
    ArtifactWorkspaceDeltaState, Criterion, CriterionId, EventId, EvidenceId, EvidenceRef,
    InvocationId, ProjectId, RunId, MAX_ARTIFACT_ORPHAN_COUNT,
};
use uuid::Uuid;

fn scope() -> String {
    json_digest(&serde_json::json!({
        "organization": "org-1",
        "project": ProjectId::new(),
        "principal": "local-user",
    }))
}

fn provenance() -> ArtifactProvenance {
    ArtifactProvenance {
        producer_kind: "run".to_owned(),
        producer_id: "run-1".to_owned(),
        source_event_id: Some(EventId::new()),
        source_run_id: Some(RunId::new()),
        recorded_by: "local-user".to_owned(),
    }
}

#[test]
fn artifact_version_reference_and_evidence_bind_immutable_content() {
    let bytes = b"historical evidence";
    let scope = scope();
    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-evidence.v1",
        bytes,
        scope.clone(),
        provenance(),
        100,
    )
    .unwrap();
    version.validate().unwrap();
    let reference = version.as_ref();
    reference.validate().unwrap();
    let evidence = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::new(),
        Some(InvocationId::new()),
        reference.clone(),
        scope,
        provenance(),
        100,
    )
    .unwrap();
    evidence.validate().unwrap();

    let mut forged = reference;
    forged.content_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    forged.validate().unwrap();
    assert_ne!(forged.content_hash, version.content_hash);
}

#[test]
fn criterion_ids_keep_same_text_distinct_and_scope_mismatch_fails() {
    let scope = scope();
    let first = Criterion::new(
        CriterionId::new(),
        "charter:v1",
        1,
        "the service responds",
        "isolated_http_fixture",
        vec!["verification_log".to_owned()],
        true,
        scope.clone(),
    )
    .unwrap();
    let second = Criterion::new(
        CriterionId::new(),
        "charter:v1",
        1,
        "the service responds",
        "isolated_http_fixture",
        vec!["verification_log".to_owned()],
        true,
        scope.clone(),
    )
    .unwrap();
    assert_ne!(first.criterion_id, second.criterion_id);
    assert_ne!(first.criterion_digest, second.criterion_digest);

    let version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-evidence.v1",
        b"evidence",
        scope.clone(),
        provenance(),
        100,
    )
    .unwrap();
    let mismatched = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::new(),
        None,
        version.as_ref(),
        json_digest(&serde_json::json!({"other": true})),
        provenance(),
        100,
    );
    assert_eq!(mismatched.unwrap_err(), "evidence_scope_mismatch");

    let mut unknown = serde_json::to_value(first).unwrap();
    unknown["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Criterion>(unknown).is_err());
}

#[test]
fn artifact_and_evidence_identity_ids_must_be_non_nil() {
    let scope = scope();
    let nil_artifact = kiana_domain::ArtifactId::from_uuid(Uuid::nil());
    let version = ArtifactVersion::new(
        nil_artifact,
        1,
        "kiana.test-evidence.v1",
        b"evidence",
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(version.unwrap_err(), "artifact_version_identity_invalid");

    let valid_version = ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-evidence.v1",
        b"evidence",
        scope.clone(),
        provenance(),
        100,
    )
    .unwrap();
    let mut reference = valid_version.as_ref();
    reference.artifact_id = nil_artifact;
    assert_eq!(
        reference.validate().unwrap_err(),
        "artifact_ref_identity_invalid"
    );

    let nil_evidence = EvidenceRef::new(
        kiana_domain::EvidenceId::from_uuid(Uuid::nil()),
        "verification_log",
        RunId::new(),
        Some(InvocationId::new()),
        valid_version.as_ref(),
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(nil_evidence.unwrap_err(), "evidence_ref_identity_invalid");

    let nil_run = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::from_uuid(Uuid::nil()),
        Some(InvocationId::new()),
        valid_version.as_ref(),
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(nil_run.unwrap_err(), "evidence_ref_identity_invalid");

    let nil_invocation = EvidenceRef::new(
        EvidenceId::new(),
        "verification_log",
        RunId::new(),
        Some(InvocationId::from_uuid(Uuid::nil())),
        valid_version.as_ref(),
        scope.clone(),
        provenance(),
        100,
    );
    assert_eq!(nil_invocation.unwrap_err(), "evidence_ref_identity_invalid");

    let criterion = Criterion::new(
        CriterionId::from_uuid(Uuid::nil()),
        "charter:v1",
        1,
        "the service responds",
        "isolated_http_fixture",
        vec!["verification_log".to_owned()],
        true,
        scope,
    );
    assert_eq!(criterion.unwrap_err(), "criterion_identity_invalid");
}

#[test]
fn artifact_provenance_source_ids_must_be_non_nil() {
    let scope = scope();
    for (source_event_id, source_run_id) in [
        (Some(EventId::from_uuid(Uuid::nil())), Some(RunId::new())),
        (Some(EventId::new()), Some(RunId::from_uuid(Uuid::nil()))),
    ] {
        let mut invalid = provenance();
        invalid.source_event_id = source_event_id;
        invalid.source_run_id = source_run_id;
        assert_eq!(
            ArtifactVersion::new(
                kiana_domain::ArtifactId::new(),
                1,
                "kiana.test-evidence.v1",
                b"evidence",
                scope.clone(),
                invalid,
                100,
            )
            .expect_err("nil provenance source identity must fail closed"),
            "artifact_provenance_identity_invalid"
        );
    }
}

fn evidence_reference(bytes: &[u8]) -> kiana_domain::ArtifactRef {
    ArtifactVersion::new(
        kiana_domain::ArtifactId::new(),
        1,
        "kiana.test-evidence.v1",
        bytes,
        scope(),
        provenance(),
        100,
    )
    .unwrap()
    .as_ref()
}

#[test]
fn artifact_publication_intent_binds_reference_and_terminal_state() {
    let reference = evidence_reference(b"historical evidence");
    let intent = ArtifactPublicationIntent::new(
        "command-1",
        "idempotency-1",
        "aggregate-1",
        reference.clone(),
        100,
    )
    .unwrap();
    assert!(intent.is_pending());
    assert_eq!(
        intent.manifest_digest,
        json_digest(&serde_json::json!(&reference))
    );
    assert_eq!(intent.artifact, reference);

    // A pending intent may not carry a resolution marker.
    assert!(intent
        .resolved(ArtifactPublicationState::Pending, None, 200)
        .is_err());
    // Committed requires the fact's event id; Abandoned must not carry one.
    assert!(intent
        .resolved(ArtifactPublicationState::Committed, None, 200)
        .is_err());
    assert!(intent
        .resolved(
            ArtifactPublicationState::Abandoned,
            Some(EventId::new()),
            200
        )
        .is_err());
    // A resolution cannot precede the recorded publication window.
    assert!(intent
        .resolved(ArtifactPublicationState::Abandoned, None, 99)
        .is_err());

    let event_id = EventId::new();
    let committed = intent
        .resolved(ArtifactPublicationState::Committed, Some(event_id), 200)
        .unwrap();
    assert_eq!(committed.state, ArtifactPublicationState::Committed);
    assert_eq!(committed.resolved_event_id, Some(event_id));
    assert_eq!(committed.artifact, reference);
    // A terminal intent is not re-resolvable.
    assert!(committed
        .resolved(ArtifactPublicationState::Abandoned, None, 300)
        .is_err());
    let abandoned = intent
        .resolved(ArtifactPublicationState::Abandoned, None, 150)
        .unwrap();
    assert_eq!(abandoned.state, ArtifactPublicationState::Abandoned);
    assert_eq!(abandoned.resolved_event_id, None);

    // Manifest drift, unknown fields and empty identity fail closed.
    let mut tampered = intent.clone();
    tampered.manifest_digest = json_digest(&serde_json::json!({"drifted": true}));
    assert_eq!(
        tampered.validate().unwrap_err(),
        "artifact_publication_manifest_mismatch"
    );
    let mut unknown = serde_json::to_value(&intent).unwrap();
    unknown["unknown_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ArtifactPublicationIntent>(unknown).is_err());
    assert!(
        ArtifactPublicationIntent::new("", "idempotency-1", "aggregate-1", reference, 100).is_err()
    );
}

#[test]
fn artifact_publication_reconciliation_classifies_orphans_and_stays_conservative() {
    let reference = evidence_reference(b"orphan bytes");
    let published = ArtifactPublicationReconciliation::new(
        "command-1",
        reference.clone(),
        true,
        true,
        Some(EventId::new()),
        "fact_committed_and_blob_verified",
    )
    .unwrap();
    assert_eq!(published.verdict, ArtifactPublicationVerdict::Published);
    assert_eq!(
        published.terminal_state(),
        Some(ArtifactPublicationState::Committed)
    );

    let orphan = ArtifactPublicationReconciliation::new(
        "command-2",
        reference.clone(),
        false,
        true,
        None,
        "orphan_committed_blob_without_fact",
    )
    .unwrap();
    assert_eq!(orphan.verdict, ArtifactPublicationVerdict::Unpublished);
    assert_eq!(
        orphan.terminal_state(),
        Some(ArtifactPublicationState::Abandoned)
    );

    let fact_without_blob = ArtifactPublicationReconciliation::new(
        "command-3",
        reference.clone(),
        true,
        false,
        Some(EventId::new()),
        "fact_committed_without_readable_blob",
    )
    .unwrap();
    assert_eq!(
        fact_without_blob.verdict,
        ArtifactPublicationVerdict::Unverifiable
    );
    assert!(fact_without_blob.terminal_state().is_none());

    let unconfirmed = ArtifactPublicationReconciliation::new(
        "command-4",
        reference.clone(),
        false,
        false,
        None,
        "fact_and_blob_unconfirmed",
    )
    .unwrap();
    assert_eq!(
        unconfirmed.verdict,
        ArtifactPublicationVerdict::Unverifiable
    );
    assert!(unconfirmed.terminal_state().is_none());

    // A reconciliation cannot claim an event id without a committed fact.
    let dropped = ArtifactPublicationReconciliation::new(
        "command-5",
        reference.clone(),
        false,
        true,
        Some(EventId::new()),
        "orphan_committed_blob_without_fact",
    )
    .unwrap();
    assert_eq!(dropped.event_id, None);
    let mut forged = orphan;
    forged.verdict = ArtifactPublicationVerdict::Published;
    assert_eq!(
        forged.validate().unwrap_err(),
        "artifact_publication_verdict_mismatch"
    );
}

#[test]
fn artifact_workspace_delta_denies_inconsistent_or_escaping_states() {
    let reference = evidence_reference(b"one\ntwo\n");
    let unchanged = ArtifactWorkspaceDelta::new(
        reference.clone(),
        "src/evidence.txt",
        8,
        ArtifactWorkspaceDeltaState::Unchanged,
        Some(reference.content_hash.clone()),
        Some(8),
        0,
        0,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        unchanged.current_state,
        ArtifactWorkspaceDeltaState::Unchanged
    );

    // Unchanged must carry the committed digest, Changed must carry a current one, and an absent
    // source must not smuggle content or a line delta.
    assert!(ArtifactWorkspaceDelta::new(
        reference.clone(),
        "src/evidence.txt",
        8,
        ArtifactWorkspaceDeltaState::Unchanged,
        Some(json_digest(&serde_json::json!({"drifted": true}))),
        Some(8),
        0,
        0,
        Vec::new(),
    )
    .is_err());
    assert!(ArtifactWorkspaceDelta::new(
        reference.clone(),
        "src/evidence.txt",
        8,
        ArtifactWorkspaceDeltaState::Changed,
        None,
        None,
        1,
        0,
        Vec::new(),
    )
    .is_err());
    assert!(ArtifactWorkspaceDelta::new(
        reference.clone(),
        "src/evidence.txt",
        8,
        ArtifactWorkspaceDeltaState::Missing,
        Some(reference.content_hash.clone()),
        None,
        0,
        0,
        Vec::new(),
    )
    .is_err());

    // A path that escapes the project root, or an oversized snapshot, is never a valid input.
    assert!(ArtifactWorkspaceDelta::new(
        reference.clone(),
        "../outside.txt",
        8,
        ArtifactWorkspaceDeltaState::Missing,
        None,
        None,
        0,
        0,
        Vec::new(),
    )
    .is_err());
    assert!(ArtifactWorkspaceDelta::new(
        reference.clone(),
        "/etc/passwd",
        8,
        ArtifactWorkspaceDeltaState::Missing,
        None,
        None,
        0,
        0,
        Vec::new(),
    )
    .is_err());
    assert!(ArtifactWorkspaceDelta::new(
        reference,
        "src/evidence.txt",
        (kiana_domain::MAX_ARTIFACT_BYTES + 1) as u64,
        ArtifactWorkspaceDeltaState::Missing,
        None,
        None,
        0,
        0,
        Vec::new(),
    )
    .is_err());
}

#[test]
fn artifact_orphan_retention_needs_a_bounded_policy_and_an_abandoned_window() {
    // Retention is opt-in and bounded: a zero or unbounded window/cap, or an empty reason, is not
    // a declared policy and must never authorize reclamation.
    assert!(ArtifactPublicationRetentionPolicy::new(0, 4, false, "co06-retention").is_err());
    assert!(ArtifactPublicationRetentionPolicy::new(
        kiana_domain::MAX_ARTIFACT_ORPHAN_AGE_MS + 1,
        4,
        false,
        "co06-retention",
    )
    .is_err());
    assert!(ArtifactPublicationRetentionPolicy::new(1, 0, false, "co06-retention").is_err());
    assert!(ArtifactPublicationRetentionPolicy::new(
        1,
        MAX_ARTIFACT_ORPHAN_COUNT + 1,
        false,
        "co06-retention",
    )
    .is_err());
    assert!(ArtifactPublicationRetentionPolicy::new(1, 4, false, "").is_err());
    let plan_only =
        ArtifactPublicationRetentionPolicy::new(1_000, 4, false, "co06-retention").unwrap();
    let permitting =
        ArtifactPublicationRetentionPolicy::new(1_000, 4, true, "co06-retention").unwrap();
    let mut unknown = serde_json::to_value(&plan_only).unwrap();
    unknown["unknown_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ArtifactPublicationRetentionPolicy>(unknown).is_err());

    let reference = evidence_reference(b"orphan bytes");
    // Only a window reconciliation already proved fact-free may be reclaimed, whatever a caller
    // claims about it.
    assert_eq!(
        ArtifactOrphanRetention::new(
            "command-pending",
            reference.clone(),
            ArtifactPublicationState::Pending,
            10_000,
            ArtifactOrphanRetentionDecision::Removed,
            "orphan_blob_reclaimed",
        )
        .unwrap_err(),
        "artifact_orphan_retention_not_abandoned"
    );
    assert_eq!(
        ArtifactOrphanRetention::new(
            "command-committed",
            reference.clone(),
            ArtifactPublicationState::Committed,
            10_000,
            ArtifactOrphanRetentionDecision::Eligible,
            "orphan_eligible_for_retention",
        )
        .unwrap_err(),
        "artifact_orphan_retention_not_abandoned"
    );

    let abandoned = ArtifactOrphanRetention::new(
        "command-1",
        reference.clone(),
        ArtifactPublicationState::Abandoned,
        10_000,
        ArtifactOrphanRetentionDecision::Removed,
        "orphan_blob_reclaimed",
    )
    .expect("abandoned window");
    // A plan may not report removal under a policy that never permitted it.
    assert_eq!(
        ArtifactPublicationRetentionPlan::new(
            plan_only.clone(),
            true,
            0,
            vec![abandoned.clone()],
            20_000,
        )
        .unwrap_err(),
        "artifact_orphan_retention_removal_not_permitted"
    );
    // Nor may it select a window that has not aged past the declared retention window.
    let too_young = ArtifactOrphanRetention::new(
        "command-1",
        reference.clone(),
        ArtifactPublicationState::Abandoned,
        10,
        ArtifactOrphanRetentionDecision::Eligible,
        "orphan_eligible_for_retention",
    )
    .unwrap();
    assert_eq!(
        ArtifactPublicationRetentionPlan::new(plan_only.clone(), true, 0, vec![too_young], 20_000)
            .unwrap_err(),
        "artifact_orphan_retention_window_not_elapsed"
    );
    // The same window cannot be counted twice, and a per-run cap cannot be widened by listing
    // more reclaimable windows than the policy allows.
    let eligible = |command_id: &str| {
        ArtifactOrphanRetention::new(
            command_id,
            evidence_reference(b"orphan bytes"),
            ArtifactPublicationState::Abandoned,
            10_000,
            ArtifactOrphanRetentionDecision::Eligible,
            "orphan_eligible_for_retention",
        )
        .unwrap()
    };
    assert_eq!(
        ArtifactPublicationRetentionPlan::new(
            plan_only.clone(),
            true,
            0,
            vec![eligible("command-2"), eligible("command-2")],
            20_000,
        )
        .unwrap_err(),
        "artifact_orphan_retention_duplicate"
    );
    let capped =
        ArtifactPublicationRetentionPolicy::new(1_000, 1, true, "co06-retention").unwrap();
    assert_eq!(
        ArtifactPublicationRetentionPlan::new(
            capped,
            true,
            0,
            vec![eligible("command-2"), eligible("command-3")],
            20_000,
        )
        .unwrap_err(),
        "artifact_orphan_retention_capacity_exceeded"
    );

    let mixed = ArtifactPublicationRetentionPlan::new(
        permitting.clone(),
        true,
        2,
        vec![
            abandoned,
            ArtifactOrphanRetention::new(
                "command-retained",
                reference.clone(),
                ArtifactPublicationState::Abandoned,
                10,
                ArtifactOrphanRetentionDecision::Retained,
                "orphan_age_below_retention_window",
            )
            .unwrap(),
            ArtifactOrphanRetention::new(
                "command-denied",
                reference.clone(),
                ArtifactPublicationState::Abandoned,
                10_000,
                ArtifactOrphanRetentionDecision::Denied,
                "orphan_retention_capacity_exceeded",
            )
            .unwrap(),
        ],
        20_000,
    )
    .expect("mixed retention plan");
    assert_eq!(
        (
            mixed.eligible_count,
            mixed.removed_count,
            mixed.retained_count,
            mixed.denied_count,
        ),
        (0, 1, 1, 1)
    );
    assert_eq!(mixed.pending_count, 2);
    assert!(!mixed.is_plan_only());

    // A plan that misreports its own counts, or that carries orphans without a journal to back
    // them, is rejected by re-verification rather than trusted.
    let mut tampered = mixed.clone();
    tampered.removed_count = 0;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "artifact_orphan_retention_counts_mismatch"
    );
    assert_eq!(
        ArtifactPublicationRetentionPlan::new(
            permitting.clone(),
            false,
            0,
            vec![eligible("command-2")],
            20_000,
        )
        .unwrap_err(),
        "artifact_orphan_retention_plan_unbacked"
    );
    assert_eq!(
        ArtifactPublicationRetentionPlan::new(
            permitting.clone(),
            false,
            0,
            Vec::new(),
            0,
        )
        .unwrap_err(),
        "artifact_orphan_retention_plan_invalid"
    );
    let plan_only_plan = ArtifactPublicationRetentionPlan::new(
        plan_only,
        true,
        1,
        vec![eligible("command-2")],
        20_000,
    )
    .expect("plan-only retention plan");
    assert!(plan_only_plan.is_plan_only());
    assert_eq!(plan_only_plan.eligible_count, 1);
    let mut unknown = serde_json::to_value(&plan_only_plan).unwrap();
    unknown["unknown_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ArtifactPublicationRetentionPlan>(unknown).is_err());
}
