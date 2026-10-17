use kiana_domain::*;
use std::collections::{BTreeMap, BTreeSet};

fn sponsor_context() -> RequestContext {
    let mut context = RequestContext::local("cp23-session", "/tmp/cp23-company");
    context.actor_id = Some("human-sponsor".to_owned());
    context.assign_role(&RoleSpec::sponsor());
    context
}

fn approval_command() -> CompanyCommand {
    CompanyCommand::ApproveProject {
        project_id: "project-1".to_owned(),
        decision_ref: "decision-1".to_owned(),
    }
}

fn decision_fixture() -> (
    CompanyCommandPolicy,
    RequestContext,
    CompanyCommand,
    HumanDecision,
) {
    let context = sponsor_context();
    let command = approval_command();
    let policy = command.policy();
    let decision = policy
        .decision(&context, &command, 7, 1_000)
        .expect("policy should create a server decision")
        .expect("project approval requires a human decision");
    (policy, context, command, decision)
}

#[test]
fn cp23_accepts_a_server_decision_bound_to_the_current_command_and_scope() {
    let (policy, context, command, decision) = decision_fixture();

    assert_eq!(decision.actor_kind, DecisionActorKind::Human);
    assert_eq!(decision.target_revision, 7);
    assert!(policy
        .validate_decision_for(Some(&decision), &context, &command, 7, 1_001)
        .is_ok());
}

#[test]
fn cp23_rejects_actor_role_and_session_binding_mismatches() {
    let (policy, context, command, decision) = decision_fixture();

    let mut other_actor = context.clone();
    other_actor.actor_id = Some("different-sponsor".to_owned());
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &other_actor, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    let mut other_role = context.clone();
    other_role.assign_role(&RoleSpec::pm());
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &other_role, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    let mut other_session = context.clone();
    other_session.session_id = SessionId::new("different-session");
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &other_session, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );
}

#[test]
fn cp23_rejects_command_revision_and_scope_digest_mismatches() {
    let (policy, context, command, decision) = decision_fixture();

    let mut changed_command = command.clone();
    if let CompanyCommand::ApproveProject { decision_ref, .. } = &mut changed_command {
        *decision_ref = "different-decision".to_owned();
    }
    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &context, &changed_command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    assert_eq!(
        policy
            .validate_decision_for(Some(&decision), &context, &command, 8, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );

    let mut changed_scope = decision.clone();
    changed_scope.scope_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        policy
            .validate_decision_for(Some(&changed_scope), &context, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );
}

#[test]
fn cp23_rejects_expired_or_missing_decisions_and_unexpected_decisions() {
    let (policy, context, command, decision) = decision_fixture();

    assert_eq!(
        policy
            .validate_decision_for(
                Some(&decision),
                &context,
                &command,
                7,
                decision.expires_at_unix_ms,
            )
            .unwrap_err(),
        "company_human_decision_binding_mismatch"
    );
    assert_eq!(
        policy
            .validate_decision_for(None, &context, &command, 7, 1_001)
            .unwrap_err(),
        "company_human_decision_missing"
    );

    let non_human_command = CompanyCommand::RegisterArtifact {
        artifact_id: "artifact-1".to_owned(),
        relative_path: "OUTPUT.txt".to_owned(),
    };
    assert_eq!(
        non_human_command
            .policy()
            .validate_decision_for(Some(&decision), &context, &non_human_command, 7, 1_001,)
            .unwrap_err(),
        "company_unexpected_human_decision"
    );
}

fn builder_context() -> RequestContext {
    let mut context = RequestContext::local("cp23-builder-session", "/tmp/cp23-company");
    context.actor_id = Some("human-builder".to_owned());
    context.assign_role(&RoleSpec::builder());
    context
}

fn reviewer_context() -> RequestContext {
    let mut context = RequestContext::local("cp23-reviewer-session", "/tmp/cp23-company");
    context.actor_id = Some("human-reviewer".to_owned());
    context.assign_role(&RoleSpec::reviewer());
    context
}

fn objective() -> Objective {
    Objective {
        objective_id: "objective-1".to_owned(),
        organization_id: "org-1".to_owned(),
        title: "Ship a governed output".to_owned(),
        problem: "the project needs a reviewable artifact".to_owned(),
        metric: "delivery_rate".to_owned(),
        baseline: 0.0,
        target: 1.0,
        unit: "ratio".to_owned(),
        measurement_method: Some("fixture acceptance ratio".to_owned()),
        direction: MetricDirection::AtLeast,
        period_start: 1,
        period_end: u64::MAX - 1,
        owner_principal_id: "human-sponsor".to_owned(),
        status: ObjectiveStatus::Proposed,
        version: 1,
    }
}

fn budget_policy() -> CompanyBudgetPolicy {
    CompanyBudgetPolicy {
        project: ProjectBudget {
            project_id: ProjectId::new(),
            max_runs: 4,
            max_tokens: 100_000,
        },
        runtime: RuntimeBudget {
            max_model_calls: 8,
            max_tokens: 100_000,
            max_wall_time_ms: 60_000,
        },
        quota: Quota {
            scope: "project-1".to_owned(),
            model_calls: 8,
            tokens: 100_000,
            concurrency: 1,
        },
    }
}

fn incident() -> Incident {
    Incident {
        incident_id: "incident-1".to_owned(),
        project_id: Some("project-1".to_owned()),
        run_id: None,
        risk_id: None,
        delivery_id: None,
        severity: "error".to_owned(),
        detected_at: 1,
        impact: "unresolved effect".to_owned(),
        timeline: vec!["opened".to_owned()],
        owner_id: "human-sponsor".to_owned(),
        response_actions: Vec::new(),
        evidence_refs: vec!["event:incident-1".to_owned()],
        escalation_target: None,
        status: IncidentStatus::Open,
    }
}

/// Representative command per registered class. Drift between the class entry and the
/// per-command policy is a registration bug and must fail `authorize_policy`.
fn representative_commands() -> Vec<CompanyCommand> {
    vec![
        CompanyCommand::Business {
            schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
            action: Box::new(CompanyBusinessAction::CreateOrganization {
                organization_id: "00000000-0000-0000-0000-000000000001".to_owned(),
                name: "Fixture Org".to_owned(),
            }),
        },
        CompanyCommand::Business {
            schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
            action: Box::new(CompanyBusinessAction::Pause {
                project_id: "project-1".to_owned(),
            }),
        },
        CompanyCommand::ClaimPacket {
            packet_id: "packet-1".to_owned(),
        },
        CompanyCommand::RenewPacketClaim {
            packet_id: "packet-1".to_owned(),
        },
        CompanyCommand::ReclaimPacketClaim {
            packet_id: "packet-1".to_owned(),
        },
        CompanyCommand::ApprovePacket {
            project_id: "project-1".to_owned(),
            milestone_id: "milestone-1".to_owned(),
            packet: WorkPacket::builder_task("packet-1", "write OUTPUT.txt"),
        },
        CompanyCommand::RecordReview {
            review_id: "review-1".to_owned(),
            acceptance_id: "acceptance-1".to_owned(),
            criterion_results: BTreeMap::new(),
            evidence_refs: Vec::new(),
        },
        CompanyCommand::RequestAcceptance {
            acceptance_id: "acceptance-1".to_owned(),
            project_id: "project-1".to_owned(),
            packet_id: "packet-1".to_owned(),
            evidence_refs: Vec::new(),
        },
        CompanyCommand::DecideAcceptance {
            acceptance_id: "acceptance-1".to_owned(),
            review_id: "review-1".to_owned(),
            decision: AcceptanceDecision::Accept,
            reasons: Vec::new(),
            waiver_ref: None,
        },
        CompanyCommand::PrepareDelivery {
            delivery: Delivery {
                delivery_id: "delivery-1".to_owned(),
                project_id: "project-1".to_owned(),
                acceptance_id: "acceptance-1".to_owned(),
                artifact_refs: vec!["artifact:output".to_owned()],
                recipient_ref: "human-sponsor".to_owned(),
                handoff_receipt_ref: None,
                delivered_at: None,
                incident_id: None,
                status: DeliveryStatus::Prepared,
                version: 1,
            },
        },
        CompanyCommand::ConfirmDelivery {
            delivery_id: "delivery-1".to_owned(),
            handoff_receipt_ref: "event:handoff".to_owned(),
        },
        CompanyCommand::CloseProject {
            project_id: "project-1".to_owned(),
            delivery_id: "delivery-1".to_owned(),
            receipt_id: "closing-1".to_owned(),
            evidence_refs: vec!["event:close".to_owned()],
        },
        CompanyCommand::ProposeObjective {
            objective: objective(),
        },
        CompanyCommand::DecideObjective {
            objective_id: "objective-1".to_owned(),
            approve: true,
        },
        CompanyCommand::ProposeProject {
            project: Project {
                project_id: "project-1".to_owned(),
                organization_id: "org-1".to_owned(),
                objective_refs: vec!["objective-1".to_owned()],
                sponsor_id: "human-sponsor".to_owned(),
                charter_ref: "artifact:charter".to_owned(),
                scope_baseline: "coding-v1".to_owned(),
                success_criteria: vec!["output exists".to_owned()],
                non_goals: Vec::new(),
                project_budget_ref: "budget:project-1".to_owned(),
                risk_summary: "bounded fixture".to_owned(),
                decision_ref: None,
                milestone_refs: Vec::new(),
                incident_id: None,
                acceptance_id: None,
                closing_receipt_id: None,
                status: ProjectStatus::Proposed,
                version: 1,
            },
        },
        CompanyCommand::ConfigureBudget {
            project_id: "project-1".to_owned(),
            policy: budget_policy(),
        },
        CompanyCommand::StartRun {
            project_id: "project-1".to_owned(),
            packet_id: "packet-1".to_owned(),
            sandbox: Some("workspace-write".to_owned()),
        },
        CompanyCommand::RegisterArtifact {
            artifact_id: "artifact-1".to_owned(),
            relative_path: "OUTPUT.txt".to_owned(),
        },
        CompanyCommand::RecordOutcome {
            outcome_id: "outcome-1".to_owned(),
            objective_id: "objective-1".to_owned(),
            project_id: "project-1".to_owned(),
            observation: MetricObservation {
                value: 1.0,
                observed_at: 1,
                evidence_refs: vec!["event:outcome".to_owned()],
            },
        },
        CompanyCommand::AcknowledgeHandoff {
            handoff_id: "packet:packet-1".to_owned(),
            accept: true,
            reason: "accepted".to_owned(),
        },
        CompanyCommand::OpenIncident {
            incident: incident(),
        },
        CompanyCommand::DecideChange {
            change_id: "change-1".to_owned(),
            approve: true,
            decision_ref: "artifact:decision".to_owned(),
        },
    ]
}

#[test]
fn company_mutation_permissions_register_every_protected_class_once() {
    let table = company_mutation_permissions();
    assert_eq!(table.len(), CompanyMutationClass::ALL.len());
    let mut classes = BTreeSet::new();
    let mut digests = BTreeSet::new();
    for permission in &table {
        permission
            .validate()
            .expect("every registered permission must be self-consistent");
        assert!(
            classes.insert(permission.class),
            "mutation class registered twice: {:?}",
            permission.class
        );
        assert!(
            digests.insert(permission.permission_digest.clone()),
            "permission digest reused: {:?}",
            permission.class
        );
        assert_eq!(permission.permission_digest, permission.digest());
        assert_eq!(permission.surface, permission.class.surface());
    }
    for class in CompanyMutationClass::ALL {
        assert!(
            classes.contains(&class),
            "class missing from the permission table: {class:?}"
        );
    }

    // The classes CP-23 names must register explicit subjects, preconditions and evidence.
    for class in [
        CompanyMutationClass::Assignment,
        CompanyMutationClass::PacketClaim,
        CompanyMutationClass::PacketChange,
        CompanyMutationClass::Review,
        CompanyMutationClass::Acceptance,
        CompanyMutationClass::Delivery,
        CompanyMutationClass::Closeout,
    ] {
        let permission = CompanyMutationPermission::for_class(class);
        assert!(
            !permission.allowed_roles.is_empty(),
            "protected class must register subjects: {class:?}"
        );
        assert!(permission.assignment_required);
        assert!(permission.evidence_required);
        assert!(!permission.preconditions.is_empty());
        assert!(!permission.operator_only);
        assert_eq!(permission.subject_denied_code, "company_role_denied");
    }

    // Operator surfaces are registered once, operator-only, with a stable denial code.
    for (operation, action, class, code) in [
        (
            "memory.review",
            "accept_proposal",
            CompanyMutationClass::MemoryPromote,
            "memory_review_operator_required",
        ),
        (
            "data.governance",
            "delete",
            CompanyMutationClass::DataRevoke,
            "governance_operator_required",
        ),
        (
            "data.governance",
            "revoke",
            CompanyMutationClass::DataRevoke,
            "governance_operator_required",
        ),
        (
            "data.governance",
            "expire",
            CompanyMutationClass::DataRevoke,
            "governance_operator_required",
        ),
    ] {
        let permission =
            operator_mutation_permission(operation, action).expect("registered operator surface");
        permission.validate().expect("operator permission is valid");
        assert_eq!(permission.class, class);
        assert_eq!(permission.surface, operation);
        assert_eq!(permission.subject_denied_code, code);
        assert!(permission.operator_only);
        assert!(permission.allowed_roles.is_empty());
        assert!(permission.actions.contains(&action.to_owned()));
        assert!(!permission.actor_kinds.contains(&DecisionActorKind::Agent));
        assert!(!permission.human_decision_required);
    }
    for (operation, action) in [
        ("memory.review", "list"),
        ("memory.review", "reject_proposal"),
        ("memory.write", "accept_proposal"),
        ("data.governance", "read"),
        ("data.governance", "restore"),
    ] {
        assert!(
            operator_mutation_permission(operation, action).is_none(),
            "unregistered operator surface must stay outside the table: {operation}/{action}"
        );
    }
}

#[test]
fn registered_permissions_never_drift_from_the_command_policy() {
    let mut classes = BTreeSet::new();
    for command in representative_commands() {
        let policy = command.policy();
        let permission = command.mutation_permission();
        assert_eq!(permission.class, command.mutation_class());
        permission
            .authorize_policy(&policy)
            .unwrap_or_else(|reason| panic!("{reason}: {:?}", command.event_name()));
        for role in &policy.allowed_roles {
            assert!(
                permission.allows_role(role),
                "class {:?} does not register policy role {role}",
                permission.class
            );
        }
        classes.insert(permission.class);
    }
    for class in [
        CompanyMutationClass::Assignment,
        CompanyMutationClass::PacketClaim,
        CompanyMutationClass::PacketChange,
        CompanyMutationClass::Review,
        CompanyMutationClass::Acceptance,
        CompanyMutationClass::Delivery,
        CompanyMutationClass::Closeout,
        CompanyMutationClass::Objective,
        CompanyMutationClass::Project,
        CompanyMutationClass::Budget,
        CompanyMutationClass::Change,
        CompanyMutationClass::Run,
        CompanyMutationClass::Artifact,
        CompanyMutationClass::Outcome,
        CompanyMutationClass::Handoff,
        CompanyMutationClass::Incident,
    ] {
        assert!(
            classes.contains(&class),
            "representative commands do not cover class: {class:?}"
        );
    }
}

#[test]
fn cp_builder_cannot_self_accept_or_promote_authority() {
    let builder = builder_context();

    // A Builder cannot approve projects, configure budgets, decide objectives or accept work.
    for command in [
        CompanyCommand::ApproveProject {
            project_id: "project-1".to_owned(),
            decision_ref: "artifact:decision".to_owned(),
        },
        CompanyCommand::ConfigureBudget {
            project_id: "project-1".to_owned(),
            policy: budget_policy(),
        },
        CompanyCommand::DecideObjective {
            objective_id: "objective-1".to_owned(),
            approve: true,
        },
        CompanyCommand::DecideAcceptance {
            acceptance_id: "acceptance-1".to_owned(),
            review_id: "review-1".to_owned(),
            decision: AcceptanceDecision::Accept,
            reasons: Vec::new(),
            waiver_ref: None,
        },
        CompanyCommand::Business {
            schema: COMPANY_BUSINESS_SCHEMA.to_owned(),
            action: Box::new(CompanyBusinessAction::Appoint {
                assignment: BusinessAssignment {
                    assignment_id: "assignment-1".to_owned(),
                    organization_id: "org-1".to_owned(),
                    owner_principal_id: "human-builder".to_owned(),
                    role_id: "sponsor".to_owned(),
                    actor_kind: BusinessActorKind::Agent,
                    project_ids: vec!["project-1".to_owned()],
                    expires_at: u64::MAX - 1,
                    revoked: false,
                    version: 1,
                },
            }),
        },
    ] {
        assert_eq!(
            command.policy().authorize_context(&builder).unwrap_err(),
            "company_role_denied",
            "builder must not submit {:?}",
            command.event_name()
        );
    }

    // A controlled model cell (Agent) is denied the human acceptance decision even when the role
    // is the reviewer role; the same command admits the human reviewer subject.
    let accept = CompanyCommand::DecideAcceptance {
        acceptance_id: "acceptance-1".to_owned(),
        review_id: "review-1".to_owned(),
        decision: AcceptanceDecision::Accept,
        reasons: Vec::new(),
        waiver_ref: None,
    };
    let mut agent_reviewer = reviewer_context();
    agent_reviewer.cell_id = Some(CellId::new());
    assert_eq!(
        accept
            .policy()
            .authorize_context(&agent_reviewer)
            .unwrap_err(),
        "company_human_decision_required"
    );
    assert!(accept
        .policy()
        .authorize_context(&reviewer_context())
        .is_ok());
    assert_eq!(
        accept
            .policy()
            .authorize_context(&builder_context())
            .unwrap_err(),
        "company_role_denied"
    );
    // A controlled Builder cell cannot accept either: the role is denied before the actor kind.
    let mut agent_builder = builder_context();
    agent_builder.cell_id = Some(CellId::new());
    assert_eq!(
        accept
            .policy()
            .authorize_context(&agent_builder)
            .unwrap_err(),
        "company_role_denied"
    );

    // The authority-promotion classes register the sponsor subject and nothing else.
    let assignment = CompanyMutationPermission::for_class(CompanyMutationClass::Assignment);
    assert!(assignment.allows_role("sponsor"));
    for role in ["builder", "reviewer", "architect", "closer", "pm"] {
        assert!(
            !assignment.allows_role(role),
            "assignment promotion must not admit {role}"
        );
    }
    let acceptance = CompanyMutationPermission::for_class(CompanyMutationClass::Acceptance);
    assert!(acceptance.allows_role("reviewer"));
    assert!(acceptance.allows_role("sponsor"));
    assert!(
        !acceptance.allows_role("architect"),
        "acceptance must not admit an unregistered subject"
    );
}

#[test]
fn packet_claim_and_reclaim_subjects_are_registered_and_deny_first() {
    let claim = CompanyCommand::ClaimPacket {
        packet_id: "packet-1".to_owned(),
    };
    assert!(claim.policy().authorize_context(&builder_context()).is_ok());
    assert_eq!(
        claim
            .policy()
            .authorize_context(&reviewer_context())
            .unwrap_err(),
        "company_role_denied"
    );

    // Before CP-23 the reclaim mutation had no explicit entry, so the empty-role fallback denied
    // every subject and the documented expired-lease scan could never commit.
    let reclaim = CompanyCommand::ReclaimPacketClaim {
        packet_id: "packet-1".to_owned(),
    };
    let policy = reclaim.policy();
    assert_eq!(
        policy.allowed_roles,
        vec!["builder".to_owned(), "closer".to_owned()]
    );
    assert!(policy.authorize_context(&builder_context()).is_ok());
    let mut closer = RequestContext::local("cp23-closer-session", "/tmp/cp23-company");
    closer.actor_id = Some("human-closer".to_owned());
    closer.assign_role(&RoleSpec::closer());
    assert!(policy.authorize_context(&closer).is_ok());
    assert_eq!(
        policy.authorize_context(&reviewer_context()).unwrap_err(),
        "company_role_denied"
    );
    let permission = reclaim.mutation_permission();
    assert_eq!(permission.class, CompanyMutationClass::PacketClaim);
    permission
        .authorize_policy(&policy)
        .expect("reclaim policy stays inside the packet-claim class entry");
    for code in [
        "company_packet_not_claimed",
        "company_claim_not_expired",
        "company_claim_worker_not_stopped",
    ] {
        assert!(
            permission
                .preconditions
                .iter()
                .any(|entry| entry.as_str() == code),
            "packet claim precondition missing: {code}"
        );
    }
}

#[test]
fn operator_mutation_surfaces_deny_cells_and_unregistered_subjects() {
    let memory = operator_mutation_permission("memory.review", "accept_proposal")
        .expect("memory promotion is registered");
    let revoke = operator_mutation_permission("data.governance", "revoke")
        .expect("data revocation is registered");

    // A controlled model cell is never an operator subject, even with an authenticated actor.
    let mut cell_operator = RequestContext::local("cp23-cell", "/tmp/cp23-company");
    cell_operator.cell_id = Some(CellId::new());
    assert_eq!(
        memory
            .authorize_operator_context(&cell_operator)
            .unwrap_err(),
        "memory_review_operator_required"
    );
    assert_eq!(
        revoke
            .authorize_operator_request(
                cell_operator.cell_id.as_ref(),
                cell_operator.actor_id.as_deref()
            )
            .unwrap_err(),
        "governance_operator_required"
    );

    // Missing identity fails closed on both the context and the dispatch-level request.
    let mut anonymous = RequestContext::local("cp23-anonymous", "/tmp/cp23-company");
    anonymous.actor_id = None;
    assert_eq!(
        memory.authorize_operator_context(&anonymous).unwrap_err(),
        "memory_review_operator_required"
    );
    assert_eq!(
        memory.authorize_operator_request(None, None).unwrap_err(),
        "memory_review_operator_required"
    );
    assert_eq!(
        revoke
            .authorize_operator_request(None, Some("   "))
            .unwrap_err(),
        "governance_operator_required"
    );

    // The authenticated human operator and the service principal are the registered subjects.
    let mut operator = RequestContext::local("cp23-operator", "/tmp/cp23-company");
    operator.actor_id = Some("local-user".to_owned());
    assert!(memory.authorize_operator_context(&operator).is_ok());
    assert!(revoke
        .authorize_operator_request(None, Some("local-user"))
        .is_ok());
    assert!(revoke
        .authorize_operator_request(None, Some("service:retention"))
        .is_ok());

    // An unregistered operation/action pair cannot borrow the operator subject.
    assert!(operator_mutation_permission("data.governance", "delete_forever").is_none());
    assert!(operator_mutation_permission("memory.review", "promote").is_none());
}
