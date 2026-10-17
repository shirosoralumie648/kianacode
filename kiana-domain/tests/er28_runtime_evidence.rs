use kiana_domain::*;
use std::collections::BTreeMap;

fn refs() -> Vec<String> {
    vec!["event:runtime-1".to_owned(), "event:runtime-2".to_owned()]
}

#[test]
fn runtime_receipt_requires_terminal_status_and_bound_event_refs() {
    let request_id = RequestId::new();
    let receipt = RuntimeReceiptRef::new(request_id, ExecutionStatus::Completed, refs()).unwrap();
    assert_eq!(receipt.request_id, request_id);
    assert_eq!(receipt.status, ExecutionStatus::Completed);
    assert!(receipt.validate().is_ok());

    let mut tampered = receipt.clone();
    tampered.status = ExecutionStatus::Running;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "runtime_receipt_terminal_status_required"
    );

    assert!(RuntimeReceiptRef::new(request_id, ExecutionStatus::Running, refs()).is_err());
    let mut digest_tampered = receipt;
    digest_tampered.event_refs.push("event:extra".to_owned());
    assert_eq!(
        digest_tampered.validate().unwrap_err(),
        "runtime_receipt_digest_mismatch"
    );
}

#[test]
fn evidence_bundle_and_unknown_incident_are_explicitly_bound() {
    let request_id = RequestId::new();
    let receipt =
        RuntimeReceiptRef::new(request_id, ExecutionStatus::ResultUnknown, refs()).unwrap();
    let bundle =
        RuntimeEvidenceBundle::new(receipt.clone(), vec!["artifact:build-report".to_owned()])
            .unwrap();
    assert!(bundle.validate().is_ok());

    let incident = WorkflowIncident::new(
        "workflow-unknown:test",
        "instance-1",
        "node-1",
        request_id,
        "workflow_runtime_result_unknown",
        receipt.event_refs.clone(),
    )
    .unwrap();
    assert!(incident.validate().is_ok());

    let mut mismatch = incident;
    mismatch.request_id = RequestId::new();
    assert_eq!(
        mismatch.validate().unwrap_err(),
        "workflow_incident_digest_mismatch"
    );
}

#[test]
fn non_event_and_non_artifact_refs_fail_closed() {
    let request_id = RequestId::new();
    assert_eq!(
        RuntimeReceiptRef::new(
            request_id,
            ExecutionStatus::Completed,
            vec!["artifact:wrong-boundary".to_owned()],
        )
        .unwrap_err(),
        "runtime_event_reference_invalid"
    );
    let receipt = RuntimeReceiptRef::new(
        request_id,
        ExecutionStatus::Completed,
        vec!["event:runtime-1".to_owned()],
    )
    .unwrap();
    assert_eq!(
        RuntimeEvidenceBundle::new(receipt, vec!["event:not-artifact".to_owned()]).unwrap_err(),
        "runtime_artifact_reference_invalid"
    );
}

// ---------------------------------------------------------------------------
// ER-28 business binding: a runtime terminal is evidence, never a business outcome.
// The fixture below rebuilds the committed Builder -> Review -> Acceptance ->
// Delivery -> Close edges as durable references so each one can be checked
// against events and registered artifacts.
// ---------------------------------------------------------------------------

const ER28_PROJECT: &str = "project-er28";
const ER28_PACKET: &str = "packet-er28";
const ER28_BUNDLE: &str = "bundle-er28";
const ER28_PACKET_ACCEPTANCE: &str = "acceptance-er28-packet";
const ER28_PROJECT_ACCEPTANCE: &str = "acceptance-er28-project";
const ER28_DELIVERY: &str = "delivery-er28";
const ER28_ARTIFACT: &str = "artifact-er28";
const ER28_RECEIPT: &str = "receipt-er28";
const ER28_CLOSE_EVENT: &str = "event:er28-close";

fn er28_event_refs() -> Vec<String> {
    vec!["event:er28-run-1".to_owned(), "event:er28-run-2".to_owned()]
}

fn er28_artifact_refs() -> Vec<String> {
    vec![format!("artifact:{ER28_ARTIFACT}")]
}

fn er28_builder_author(run_id: RunId) -> BusinessAuthor {
    BusinessAuthor {
        principal_id: "builder-cell-er28".to_owned(),
        human_owner: "local-user".to_owned(),
        role_instance_id: "cell-er28-builder".to_owned(),
        session_id: SessionId::new("builder-er28-session"),
        run_id,
        role_id: "builder".to_owned(),
    }
}

struct Er28Fixture {
    state: CompanyState,
    proof: CompanyProof,
    authority: CompanyAuthority,
    run_id: RunId,
    receipt: RuntimeReceiptRef,
}

fn er28_close_command(kind: BusinessCloseKind, evidence_refs: Vec<String>) -> CompanyCommand {
    CompanyCommand::Business {
        schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
        action: Box::new(CompanyBusinessAction::Closeout {
            schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
            action: Box::new(BusinessCloseoutAction::Close {
                project_id: ER28_PROJECT.to_owned(),
                receipt_id: ER28_RECEIPT.to_owned(),
                close_kind: kind,
                reason: "explicit business close decision".to_owned(),
                evidence_refs,
                residual_obligations: Vec::new(),
            }),
        }),
    }
}

fn er28_fixture() -> Er28Fixture {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let event_refs = er28_event_refs();
    let artifact_refs = er28_artifact_refs();
    let receipt =
        RuntimeReceiptRef::new(request_id, ExecutionStatus::Completed, event_refs.clone()).unwrap();
    let runtime_evidence =
        RuntimeEvidenceBundle::new(receipt.clone(), artifact_refs.clone()).unwrap();
    let builder = er28_builder_author(run_id);
    let reviewer = BusinessAuthor {
        principal_id: "reviewer-er28".to_owned(),
        human_owner: "local-user".to_owned(),
        role_instance_id: "assignment-er28-reviewer".to_owned(),
        session_id: SessionId::new("reviewer-er28-session"),
        run_id: RunId::new(),
        role_id: "reviewer".to_owned(),
    };
    let criterion_packet = BusinessCriterion {
        criterion_id: "criterion-packet-er28".to_owned(),
        target: AcceptanceTarget::Packet(ER28_PACKET.to_owned()),
        description: "builder output satisfies the frozen packet criterion".to_owned(),
        required: true,
        refines: vec!["criterion-project-er28".to_owned()],
    };
    let criterion_project = BusinessCriterion {
        criterion_id: "criterion-project-er28".to_owned(),
        target: AcceptanceTarget::Project(ER28_PROJECT.to_owned()),
        description: "project outcome satisfies the frozen charter criterion".to_owned(),
        required: true,
        refines: Vec::new(),
    };
    let baseline = BusinessBaseline {
        project_id: ER28_PROJECT.to_owned(),
        organization_id: "org-er28".to_owned(),
        workspace: "/er28-project".to_owned(),
        version: 1,
        charter_ref: "artifact:charter-er28".to_owned(),
        scope: "coding-v1".to_owned(),
        criteria: BTreeMap::from([
            (
                criterion_packet.criterion_id.clone(),
                criterion_packet.clone(),
            ),
            (
                criterion_project.criterion_id.clone(),
                criterion_project.clone(),
            ),
        ]),
        stakeholders: vec![BusinessStakeholder {
            principal_id: "local-user".to_owned(),
            responsibility: "sponsor".to_owned(),
            delivery_recipient: true,
        }],
        approved_by: "local-user".to_owned(),
        approved_at: 10,
        plan_version: 1,
        active_packets: vec![ER28_PACKET.to_owned()],
        superseded_packets: BTreeMap::new(),
    };
    let run = CompanyRun {
        packet_id: ER28_PACKET.to_owned(),
        project_id: ER28_PROJECT.to_owned(),
        execution_request_id: request_id,
        author_session_id: SessionId::new("builder-er28-session"),
        run_id: Some(run_id),
        status: ExecutionStatus::Completed,
        evidence_refs: event_refs.clone(),
        runtime_receipt: Some(receipt.clone()),
        incident_id: None,
    };
    let bundle = EvidenceBundle {
        bundle_id: ER28_BUNDLE.to_owned(),
        project_id: ER28_PROJECT.to_owned(),
        packet_id: ER28_PACKET.to_owned(),
        baseline_version: 1,
        packet_version: 1,
        run_id,
        authors: vec![builder.clone()],
        artifact_refs: artifact_refs.clone(),
        event_refs: event_refs.clone(),
        runtime_receipt: Some(receipt.clone()),
        runtime_evidence: Some(runtime_evidence),
        digest: "bundle-digest-er28".to_owned(),
        created_at: 40,
    };
    let review = BusinessReview {
        review_id: "review-er28".to_owned(),
        assignment_id: "assignment-er28-reviewer".to_owned(),
        reviewer: reviewer.clone(),
        results: BTreeMap::from([(
            criterion_packet.criterion_id.clone(),
            CriterionConclusion {
                verdict: CriterionVerdict::Pass,
                evidence_refs: event_refs.clone(),
                reason: "independent review of the builder evidence".to_owned(),
            },
        )]),
        evidence_digest: "evidence-digest-er28".to_owned(),
        recorded_at: 50,
    };
    let project_review = BusinessReview {
        review_id: "review-er28-project".to_owned(),
        assignment_id: "assignment-er28-reviewer".to_owned(),
        reviewer,
        results: BTreeMap::from([(
            criterion_project.criterion_id.clone(),
            CriterionConclusion {
                verdict: CriterionVerdict::Pass,
                evidence_refs: event_refs.clone(),
                reason: "independent review of the project evidence".to_owned(),
            },
        )]),
        evidence_digest: "evidence-digest-er28".to_owned(),
        recorded_at: 50,
    };
    let packet_acceptance = BusinessAcceptance {
        acceptance_id: ER28_PACKET_ACCEPTANCE.to_owned(),
        project_id: ER28_PROJECT.to_owned(),
        target: AcceptanceTarget::Packet(ER28_PACKET.to_owned()),
        baseline_version: 1,
        criteria: BTreeMap::from([(criterion_packet.criterion_id.clone(), criterion_packet)]),
        bundle_ids: vec![ER28_BUNDLE.to_owned()],
        authors: vec![builder.clone()],
        evidence_digest: "evidence-digest-er28".to_owned(),
        review: Some(review),
        status: AcceptanceStatus::Accepted,
        decision_maker: Some("local-user".to_owned()),
        reasons: Vec::new(),
        waiver_refs: Vec::new(),
        created_at: 60,
        decided_at: Some(70),
    };
    let project_acceptance = BusinessAcceptance {
        acceptance_id: ER28_PROJECT_ACCEPTANCE.to_owned(),
        project_id: ER28_PROJECT.to_owned(),
        target: AcceptanceTarget::Project(ER28_PROJECT.to_owned()),
        baseline_version: 1,
        criteria: BTreeMap::from([(criterion_project.criterion_id.clone(), criterion_project)]),
        bundle_ids: vec![ER28_BUNDLE.to_owned()],
        authors: vec![builder.clone()],
        evidence_digest: "evidence-digest-er28".to_owned(),
        review: Some(project_review),
        status: AcceptanceStatus::Accepted,
        decision_maker: Some("local-user".to_owned()),
        reasons: Vec::new(),
        waiver_refs: Vec::new(),
        created_at: 60,
        decided_at: Some(70),
    };
    let delivery = BusinessDelivery {
        manifest: BusinessDeliveryManifest {
            schema: "kiana.company-delivery-manifest.v2".to_owned(),
            delivery_id: ER28_DELIVERY.to_owned(),
            project_id: ER28_PROJECT.to_owned(),
            baseline_version: 1,
            acceptance_id: ER28_PROJECT_ACCEPTANCE.to_owned(),
            acceptance_digest: "evidence-digest-er28".to_owned(),
            channel: "local_package".to_owned(),
            destination: "deliveries/er28".to_owned(),
            recipient: "local-user".to_owned(),
            residual_obligations: Vec::new(),
            entries: Vec::new(),
        },
        digest: "delivery-digest-er28".to_owned(),
        status: BusinessDeliveryStatus::Confirmed,
        approval: Some(BusinessDeliveryApproval {
            manifest_digest: "delivery-digest-er28".to_owned(),
            approved_by: "local-user".to_owned(),
            approved_at: 20,
            expires_at: 10_000,
        }),
        dispatch_request_id: Some(RequestId::new()),
        sender_session: Some(SessionId::new("closer-er28-session")),
        package_receipt: None,
        confirmation: Some(BusinessDeliveryConfirmation {
            recipient: "local-user".to_owned(),
            manifest_digest: "delivery-digest-er28".to_owned(),
            package_sha256: "package-sha256-er28".to_owned(),
            received_at: 30,
            decision_request_id: RequestId::new(),
        }),
        created_at: 20,
    };
    let project = Project {
        project_id: ER28_PROJECT.to_owned(),
        organization_id: "org-er28".to_owned(),
        objective_refs: vec!["objective-er28".to_owned()],
        sponsor_id: "local-user".to_owned(),
        charter_ref: "artifact:charter-er28".to_owned(),
        scope_baseline: "coding-v1".to_owned(),
        success_criteria: vec!["builder output exists".to_owned()],
        non_goals: vec!["no external publish".to_owned()],
        project_budget_ref: format!("budget:{ER28_PROJECT}"),
        risk_summary: "bounded local fixture".to_owned(),
        decision_ref: Some("event:er28-decision".to_owned()),
        milestone_refs: Vec::new(),
        incident_id: None,
        acceptance_id: Some(ER28_PROJECT_ACCEPTANCE.to_owned()),
        closing_receipt_id: None,
        status: ProjectStatus::Accepted,
        version: 3,
    };
    let state = CompanyState {
        business: CompanyBusinessState {
            clock_ms: 0,
            assignments: BTreeMap::from([(
                "assignment-er28-closer".to_owned(),
                BusinessAssignment {
                    assignment_id: "assignment-er28-closer".to_owned(),
                    organization_id: "org-er28".to_owned(),
                    owner_principal_id: "closer-1".to_owned(),
                    role_id: "closer".to_owned(),
                    actor_kind: BusinessActorKind::Human,
                    project_ids: vec![ER28_PROJECT.to_owned()],
                    expires_at: 10_000,
                    version: 1,
                    revoked: false,
                },
            )]),
            baselines: BTreeMap::from([(ER28_PROJECT.to_owned(), baseline)]),
            artifacts: BTreeMap::from([(
                ER28_ARTIFACT.to_owned(),
                BusinessArtifact {
                    artifact_id: ER28_ARTIFACT.to_owned(),
                    project_id: ER28_PROJECT.to_owned(),
                    baseline_version: 1,
                    content_hash: "artifact-hash-er28".to_owned(),
                    schema: "kiana.artifact-output.v1".to_owned(),
                    producer_runs: vec![run_id],
                    registered_at: 35,
                },
            )]),
            bundles: BTreeMap::from([(ER28_BUNDLE.to_owned(), bundle.clone())]),
            acceptances: BTreeMap::from([
                (ER28_PACKET_ACCEPTANCE.to_owned(), packet_acceptance),
                (ER28_PROJECT_ACCEPTANCE.to_owned(), project_acceptance),
            ]),
            closeout: CompanyCloseoutState {
                deliveries: BTreeMap::from([(ER28_DELIVERY.to_owned(), delivery)]),
                ..CompanyCloseoutState::default()
            },
            ..CompanyBusinessState::default()
        },
        revision: 7,
        projects: BTreeMap::from([(ER28_PROJECT.to_owned(), project)]),
        packets: BTreeMap::from([(
            ER28_PACKET.to_owned(),
            CompanyPacket {
                project_id: ER28_PROJECT.to_owned(),
                milestone_id: "milestone-er28".to_owned(),
                packet: WorkPacket::builder_task(ER28_PACKET, "write the ER-28 output")
                    .with_path_allow(["OUTPUT.txt"])
                    .with_acceptance_tests(["builder output exists"]),
                version: 1,
            },
        )]),
        runs: BTreeMap::from([(ER28_PACKET.to_owned(), run.clone())]),
        ..CompanyState::default()
    };
    let proof = CompanyProof {
        business: BusinessProof {
            workspace: "/er28-project".to_owned(),
            actor_is_human: true,
            authors: BTreeMap::from([(run_id.to_string(), builder)]),
            runs: BTreeMap::from([(ER28_PACKET.to_owned(), run)]),
            reviewer: None,
            verified_events: event_refs.clone(),
            review_results: BTreeMap::new(),
        },
        events: event_refs
            .iter()
            .cloned()
            .chain([ER28_CLOSE_EVENT.to_owned()])
            .collect(),
        closeout: CompanyCloseoutProof {
            all_runs_stopped: true,
            run_evidence: vec![event_refs[0].clone()],
            usage: BusinessUsageSummary {
                run_count: 1,
                model_calls: 2,
                evidence_refs: event_refs.clone(),
                ..BusinessUsageSummary::default()
            },
            ..CompanyCloseoutProof::default()
        },
        ..CompanyProof::default()
    };
    let authority = CompanyAuthority {
        actor_id: "closer-1".to_owned(),
        role_id: "closer".to_owned(),
        session_id: SessionId::new("closer-er28-session"),
        now_ms: 100,
        execution_request_id: RequestId::new(),
        execution_cell_id: None,
    };
    Er28Fixture {
        state,
        proof,
        authority,
        run_id,
        receipt,
    }
}

#[test]
fn company_success_cannot_be_inferred_from_run_completed() {
    let mut fixture = er28_fixture();
    // The runtime terminal is real: Completed, bound to events and artifact evidence.
    assert_eq!(fixture.receipt.status, ExecutionStatus::Completed);
    assert!(fixture.receipt.validate().is_ok());
    assert_eq!(
        fixture.state.runs[ER28_PACKET]
            .runtime_receipt
            .as_ref()
            .expect("observed runtime receipt"),
        &fixture.receipt
    );

    // The same completed run still does not establish business success.
    fixture.state.business.acceptances.clear();
    fixture.state.projects.get_mut(ER28_PROJECT).unwrap().status = ProjectStatus::Active;
    assert!(!fixture
        .state
        .business_accepted(&AcceptanceTarget::Project(ER28_PROJECT.to_owned()), 1));
    assert_eq!(
        fixture
            .state
            .transition(
                &er28_close_command(
                    BusinessCloseKind::Success,
                    vec![ER28_CLOSE_EVENT.to_owned()]
                ),
                &fixture.authority,
                &fixture.proof,
            )
            .unwrap_err(),
        "business_closing_required_acceptance_missing"
    );
    assert!(fixture.state.business.closeout.receipts.is_empty());

    // The completed runtime may still be closed as an explicit human failure decision.
    let failed = fixture
        .state
        .transition(
            &er28_close_command(
                BusinessCloseKind::Failure,
                vec![ER28_CLOSE_EVENT.to_owned()],
            ),
            &fixture.authority,
            &fixture.proof,
        )
        .expect("explicit failure close");
    assert_eq!(
        failed.business.closeout.receipts[ER28_RECEIPT].close_kind,
        BusinessCloseKind::Failure
    );
    assert_eq!(failed.projects[ER28_PROJECT].status, ProjectStatus::Failed);
}

#[test]
fn closing_without_evidence_is_denied() {
    // Legacy bundles without the ControlPlane-derived runtime receipt/evidence fail closed.
    let mut legacy = er28_fixture();
    {
        let bundle = legacy
            .state
            .business
            .bundles
            .get_mut(ER28_BUNDLE)
            .expect("fixture bundle");
        bundle.runtime_receipt = None;
        bundle.runtime_evidence = None;
    }
    assert_eq!(
        legacy
            .state
            .transition(
                &er28_close_command(
                    BusinessCloseKind::Success,
                    vec![ER28_CLOSE_EVENT.to_owned()]
                ),
                &legacy.authority,
                &legacy.proof,
            )
            .unwrap_err(),
        "business_closing_runtime_evidence_missing"
    );

    // A close decision must cite recorded events; empty or unknown refs are denied.
    let fixture = er28_fixture();
    for evidence_refs in [Vec::new(), vec!["event:not-recorded".to_owned()]] {
        assert_eq!(
            fixture
                .state
                .transition(
                    &er28_close_command(BusinessCloseKind::Success, evidence_refs),
                    &fixture.authority,
                    &fixture.proof,
                )
                .unwrap_err(),
            "business_evidence_unverified"
        );
    }

    // An unresolved runtime keeps the close denied until reconciliation.
    let mut unknown = er28_fixture();
    unknown.proof.closeout.all_runs_stopped = false;
    assert_eq!(
        unknown
            .state
            .transition(
                &er28_close_command(
                    BusinessCloseKind::Success,
                    vec![ER28_CLOSE_EVENT.to_owned()]
                ),
                &unknown.authority,
                &unknown.proof,
            )
            .unwrap_err(),
        "business_closing_unresolved_effects"
    );
}

#[test]
fn er28_builder_review_acceptance_delivery_close_refs_rebuild_from_events_and_artifacts() {
    let fixture = er28_fixture();
    let closed = fixture
        .state
        .transition(
            &er28_close_command(
                BusinessCloseKind::Success,
                vec![ER28_CLOSE_EVENT.to_owned()],
            ),
            &fixture.authority,
            &fixture.proof,
        )
        .expect("business close with bound runtime evidence");

    // Builder -> evidence bundle: every event and artifact reference resolves.
    let bundle = &closed.business.bundles[ER28_BUNDLE];
    for reference in &bundle.event_refs {
        assert!(
            fixture.proof.events.contains(reference),
            "unbound event reference: {reference}"
        );
    }
    for reference in &bundle.artifact_refs {
        let id = reference
            .strip_prefix("artifact:")
            .expect("artifact reference");
        assert!(
            closed.business.artifacts.contains_key(id),
            "unbound artifact reference: {reference}"
        );
    }
    assert_eq!(
        bundle
            .runtime_receipt
            .as_ref()
            .expect("bound runtime receipt"),
        &fixture.receipt
    );
    assert_eq!(
        bundle
            .runtime_evidence
            .as_ref()
            .expect("bound runtime evidence")
            .artifact_refs,
        bundle.artifact_refs
    );

    // Review -> acceptance: an independent review is bound to the same evidence digest.
    for acceptance in closed.business.acceptances.values() {
        assert_eq!(acceptance.status, AcceptanceStatus::Accepted);
        for bundle_id in &acceptance.bundle_ids {
            assert!(closed.business.bundles.contains_key(bundle_id));
        }
        let review = acceptance.review.as_ref().expect("independent review");
        assert_eq!(review.evidence_digest, acceptance.evidence_digest);
        assert_ne!(review.reviewer.run_id, bundle.run_id);
        for conclusion in review.results.values() {
            for reference in &conclusion.evidence_refs {
                assert!(fixture.proof.events.contains(reference));
            }
        }
    }

    // Acceptance -> delivery -> closing receipt: the accepted target and evidence stay linked.
    let delivery = &closed.business.closeout.deliveries[ER28_DELIVERY];
    assert_eq!(delivery.status, BusinessDeliveryStatus::Confirmed);
    assert_eq!(
        delivery.manifest.acceptance_id,
        ER28_PROJECT_ACCEPTANCE.to_owned()
    );
    assert!(closed
        .business
        .acceptances
        .contains_key(&delivery.manifest.acceptance_id));
    let receipt = &closed.business.closeout.receipts[ER28_RECEIPT];
    assert_eq!(receipt.close_kind, BusinessCloseKind::Success);
    assert_eq!(receipt.run_ids, vec![fixture.run_id]);
    assert!(receipt
        .acceptance_ids
        .contains(&ER28_PACKET_ACCEPTANCE.to_owned()));
    assert!(receipt
        .acceptance_ids
        .contains(&ER28_PROJECT_ACCEPTANCE.to_owned()));
    assert_eq!(receipt.delivery_ids, vec![ER28_DELIVERY.to_owned()]);
    assert!(receipt.evidence_refs.contains(&ER28_CLOSE_EVENT.to_owned()));
    for reference in &receipt.evidence_refs {
        assert!(fixture.proof.events.contains(reference));
    }
    for run_id in &receipt.run_ids {
        assert!(closed.runs.values().any(|run| run.run_id == Some(*run_id)));
    }
    assert_eq!(closed.projects[ER28_PROJECT].status, ProjectStatus::Closed);
    assert_eq!(
        closed.projects[ER28_PROJECT].closing_receipt_id.as_deref(),
        Some(ER28_RECEIPT)
    );
}
