use super::*;
use serde_json::Value;

#[test]
fn approval_state_machine_is_single_use_and_terminal() {
    assert_eq!(
        ApprovalState::Staged
            .transition(ApprovalState::Active)
            .unwrap(),
        ApprovalState::Active
    );
    assert_eq!(
        ApprovalState::Active
            .transition(ApprovalState::Approved)
            .unwrap()
            .transition(ApprovalState::Consumed)
            .unwrap(),
        ApprovalState::Consumed
    );
    assert!(ApprovalState::Consumed.is_terminal());
    assert_eq!(
        ApprovalState::Consumed
            .transition(ApprovalState::Active)
            .unwrap_err(),
        DomainError::InvalidStateTransition {
            aggregate: "approval",
            from: "consumed",
            to: "active",
        }
    );
}

#[test]
fn capability_execution_cannot_skip_authorization_or_recover_unknown() {
    assert!(
        !CapabilityExecutionState::Requested.can_transition_to(CapabilityExecutionState::Executing)
    );
    assert!(CapabilityExecutionState::PolicyChecked
        .can_transition_to(CapabilityExecutionState::AwaitingApproval));
    assert!(
        CapabilityExecutionState::Executing.can_transition_to(CapabilityExecutionState::Unknown)
    );
    assert!(CapabilityExecutionState::Unknown.is_terminal());
    assert!(
        !CapabilityExecutionState::Unknown.can_transition_to(CapabilityExecutionState::Succeeded)
    );
}

#[test]
fn cancelled_execution_is_terminal_and_serializes_distinctly() {
    assert!(ExecutionStatus::Running.can_transition_to(ExecutionStatus::Cancelled));
    assert!(ExecutionStatus::AwaitingApproval.can_transition_to(ExecutionStatus::Cancelled));
    assert!(ExecutionStatus::Cancelled.is_terminal());
    assert!(!ExecutionStatus::Cancelled.can_transition_to(ExecutionStatus::Running));
    let encoded = serde_json::to_string(&ExecutionStatus::Cancelled).unwrap();
    assert_eq!(encoded, "\"cancelled\"");
    assert_eq!(ExecutionStatus::Cancelled.as_str(), "cancelled");
}

#[test]
fn work_packet_and_cell_terminals_cannot_return_to_running() {
    assert!(WorkPacketStatus::Closed.is_terminal());
    assert!(!WorkPacketStatus::Closed.can_transition_to(WorkPacketStatus::Running));
    assert!(WorkPacketStatus::Blocked.can_transition_to(WorkPacketStatus::Running));
    assert!(WorkPacketStatus::AwaitingApproval.can_transition_to(WorkPacketStatus::Running));
    assert!(CellLifecycle::Retired.is_terminal());
    assert!(!CellLifecycle::Retired.can_transition_to(CellLifecycle::Running));
    assert!(!CellLifecycle::Failed.is_terminal());
    assert!(CellLifecycle::CancelRequested.can_transition_to(CellLifecycle::Cancelled));
}

#[test]
fn stable_ids_are_distinct_serializable_contract_types() {
    let turn = TurnId::new();
    let cell = CellId::new();
    assert_ne!(turn.to_string(), cell.to_string());
    let encoded = serde_json::to_string(&turn).unwrap();
    assert_eq!(serde_json::from_str::<TurnId>(&encoded).unwrap(), turn);
}

#[test]
fn company_os_contracts_validate_and_child_grants_only_shrink() {
    let role = RoleSpec::builder();
    let template = AgentTemplate::for_role(&role, "1.0.0");
    assert!(template.validate().is_ok());

    let mut parent = CapabilityGrant {
        schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Filesystem,
        operation: "apply_patch".to_owned(),
        resources: vec!["workspace".to_owned()],
        paths: vec!["src".to_owned()],
        expires_at_unix_ms: 200,
        approval_id: None,
        delegation_allowed: true,
    };
    let child = CapabilityGrant {
        schema: CAPABILITY_GRANT_SCHEMA.to_owned(),
        grant_id: CapabilityGrantId::new(),
        capability: CapabilityKind::Filesystem,
        operation: "apply_patch".to_owned(),
        resources: vec!["workspace".to_owned()],
        paths: vec!["src/lib.rs".to_owned()],
        expires_at_unix_ms: 100,
        approval_id: None,
        delegation_allowed: false,
    };
    assert!(parent.validate().is_ok());
    assert!(child.validate().is_ok());
    assert!(parent.contains(&child));
    parent.delegation_allowed = false;
    let mut delegated_child = child.clone();
    delegated_child.delegation_allowed = true;
    assert!(!parent.contains(&delegated_child));
}

#[test]
fn budget_lease_rejects_overconsumption_without_mutating_usage() {
    let lease = BudgetLease::new(2, 100, 1_000, 1, 1);
    assert!(lease.can_consume(2, 100, 1));
    assert!(!lease.can_consume(3, 100, 1));
    assert_eq!(lease.tool_calls_used, 0);
    assert_eq!(lease.effects_used, 0);
}

#[test]
fn work_packet_legacy_json_defaults_to_draft_and_round_trips_new_fields() {
    let packet: WorkPacket = serde_json::from_value(serde_json::json!({
        "schema": WORK_PACKET_SCHEMA,
        "id": "wp-1",
        "goal": "ship a change"
    }))
    .unwrap();
    assert_eq!(packet.status, WorkPacketStatus::Draft);
    assert!(packet.project_id.is_none());

    let mut packet = packet;
    packet.project_id = Some(ProjectId::new());
    packet.acceptance_tests = vec!["cargo test".to_owned()];
    packet
        .transition_status(WorkPacketStatus::Approved)
        .unwrap();
    let encoded = serde_json::to_value(&packet).unwrap();
    assert_eq!(encoded["status"], "approved");
    assert_eq!(encoded["acceptance_tests"][0], "cargo test");

    packet.status = WorkPacketStatus::Closed;
    assert_eq!(packet.validate(), Err("packet_status_terminal"));
}

#[test]
fn runtime_event_supports_optional_company_os_stream_metadata() {
    let request_id = RequestId::new();
    let event = RuntimeEvent::new(request_id, 2, "run.completed", Value::Null)
        .unwrap()
        .with_stream_metadata("request", request_id.to_string(), 2)
        .with_idempotency_key("run-2-completed");

    assert_eq!(event.aggregate_type.as_deref(), Some("request"));
    assert_eq!(
        event.aggregate_id.as_deref().unwrap(),
        request_id.to_string()
    );
    assert_eq!(event.stream_version, Some(2));
    assert_eq!(event.idempotency_key.as_deref(), Some("run-2-completed"));

    let legacy: RuntimeEvent = serde_json::from_value(serde_json::json!({
        "event_id": event.event_id,
        "request_id": request_id,
        "sequence": 1,
        "kind": "run.accepted",
        "data": null
    }))
    .unwrap();
    assert!(legacy.aggregate_type.is_none());
    assert!(legacy.idempotency_key.is_none());
}

#[test]
fn request_context_defaults_to_untrusted() {
    let context = RequestContext::local("session-1", "/repo");
    assert!(!context.project_trusted);
    assert_eq!(context.permission_profile, PermissionProfile::Safe);
}

#[test]
fn v0_2_worker_is_executing_builder() {
    let role = RoleSpec::builder();
    let department = DepartmentSpec::executing();
    assert_eq!(role.role_id, ROLE_BUILDER);
    assert_eq!(role.department_id, DEPARTMENT_EXECUTING);
    assert_eq!(
        role.tools,
        [
            "shell",
            "apply_patch",
            "mcp",
            "memory.search",
            "memory.write"
        ]
    );
    assert_eq!(role.sandbox, ROLE_SANDBOX_WORKSPACE_WRITE);
    assert_eq!(role.path_allow, ["."]);
    assert!(!role.prompt_hash.is_empty());
    assert_eq!(department.department_id, DEPARTMENT_EXECUTING);
    assert_eq!(department.roles, [ROLE_BUILDER]);
    assert!(department.can_convene);
    assert!(role.can_convene);
}

#[test]
fn v0_3_catalog_has_planning_and_executing_roles() {
    let planning = DepartmentSpec::planning();
    assert_eq!(planning.department_id, DEPARTMENT_PLANNING);
    assert_eq!(planning.roles, [ROLE_PM, ROLE_ARCHITECT]);
    assert!(planning.can_convene);

    let pm = RoleSpec::pm();
    assert_eq!(pm.department_id, DEPARTMENT_PLANNING);
    assert_eq!(pm.tools, ["apply_patch", "memory.search", "memory.write"]);
    assert!(pm.can_convene);
    assert!(pm.allows_path("plan/WORK.md"));
    assert!(pm.allows_path("charter/GOAL.md"));
    assert!(pm.allows_path("packet/task.json"));
    assert!(!pm.allows_path("GOLDEN_PATH.txt"));
    assert!(!pm.allows_path("src/lib.rs"));
    assert!(!pm.allows_tool("shell"));

    let architect = RoleSpec::architect();
    assert_eq!(architect.department_id, DEPARTMENT_PLANNING);
    assert_eq!(architect.tools, ["memory.search"]);
    assert!(!architect.workspace_write_allowed());
    assert!(!architect.allows_path("plan/WORK.md"));
    assert!(!architect.can_convene);
    assert!(architect.can_vote);

    assert_eq!(RoleSpec::lookup("pm").unwrap().role_id, ROLE_PM);
    assert_eq!(RoleSpec::lookup("").unwrap().role_id, ROLE_BUILDER);
    assert!(RoleSpec::lookup("ceo").is_none());

    let monitoring = DepartmentSpec::monitoring();
    assert_eq!(monitoring.department_id, DEPARTMENT_MONITORING);
    assert_eq!(monitoring.roles, [ROLE_REVIEWER]);
    assert!(monitoring.can_convene);
    let reviewer = RoleSpec::reviewer();
    assert!(reviewer.can_convene);
    assert_eq!(reviewer.department_id, DEPARTMENT_MONITORING);
    assert_eq!(reviewer.tools, ["memory.search"]);
    assert!(!reviewer.workspace_write_allowed());
    assert!(!reviewer.allows_tool("apply_patch"));
    assert!(!reviewer.allows_path("GOLDEN_PATH.txt"));
    assert_eq!(RoleSpec::lookup("reviewer").unwrap().role_id, ROLE_REVIEWER);
    assert_eq!(
        DepartmentSpec::lookup("planning").unwrap().department_id,
        DEPARTMENT_PLANNING
    );
}

#[test]
fn v0_5_catalog_has_five_departments_without_changing_default_worker() {
    let ids: Vec<_> = DepartmentSpec::catalog()
        .into_iter()
        .map(|department| department.department_id)
        .collect();
    assert_eq!(
        ids,
        [
            DEPARTMENT_INITIATING,
            DEPARTMENT_PLANNING,
            DEPARTMENT_EXECUTING,
            DEPARTMENT_MONITORING,
            DEPARTMENT_CLOSING,
        ]
    );

    let initiating = DepartmentSpec::initiating();
    assert_eq!(initiating.pmp_group, DEPARTMENT_INITIATING);
    assert_eq!(initiating.roles, [ROLE_SPONSOR]);
    assert_eq!(initiating.artifacts, [PLANNING_PATH_CHARTER]);
    assert_eq!(initiating.rag_collection, "department:initiating");
    assert!(initiating.can_convene);
    assert!(!initiating.mission.is_empty());
    assert!(!initiating.gates.is_empty());

    let sponsor = RoleSpec::sponsor();
    assert_eq!(sponsor.department_id, DEPARTMENT_INITIATING);
    assert!(sponsor.allows_path("charter/GOAL.md"));
    assert!(!sponsor.allows_path("GOLDEN_PATH.txt"));
    assert!(!sponsor.allows_path("src/lib.rs"));
    assert!(!sponsor.allows_tool("shell"));
    assert_eq!(RoleSpec::lookup("sponsor").unwrap().role_id, ROLE_SPONSOR);

    let closing = DepartmentSpec::closing();
    assert_eq!(closing.roles, [ROLE_CLOSER]);
    assert_eq!(closing.artifacts, [CLOSING_PATH_LESSONS]);
    assert!(closing.can_convene);

    let closer = RoleSpec::closer();
    assert!(closer.can_convene);
    assert_eq!(closer.department_id, DEPARTMENT_CLOSING);
    assert!(closer.allows_path("lessons/LEARNED.md"));
    assert!(!closer.allows_path("GOLDEN_PATH.txt"));
    assert!(!closer.allows_tool("shell"));
    assert_eq!(RoleSpec::lookup("closer").unwrap().role_id, ROLE_CLOSER);

    assert_eq!(RoleSpec::lookup("").unwrap().role_id, ROLE_BUILDER);
    assert_eq!(
        DepartmentSpec::lookup("").unwrap().department_id,
        DEPARTMENT_EXECUTING
    );
    assert!(RoleSpec::lookup("ceo").is_none());
}

#[test]
fn v0_5_memory_grants_keep_builder_off_private_and_unreleased() {
    assert_eq!(MEMORY_LAYERS.len(), 6);
    let builder = RoleSpec::builder();
    assert!(builder.allows_knowledge(MEMORY_LAYER_COMPANY));
    assert!(builder.allows_knowledge(MEMORY_LAYER_PROJECT));
    assert!(builder.allows_knowledge("project:code"));
    assert!(builder.allows_knowledge("role:builder"));
    assert!(builder.allows_knowledge(MEMORY_LAYER_INSTANCE_SCRATCH));
    assert!(!builder.allows_knowledge(MEMORY_COLLECTION_USER_PRIVATE));
    assert!(!builder.allows_knowledge(MEMORY_COLLECTION_USER_PREFS));
    assert!(!builder.allows_knowledge(MEMORY_COLLECTION_PLANNING_UNRELEASED));
    assert!(!builder.allows_knowledge("project:events"));
    assert!(builder.allows_memory_write(MEMORY_LAYER_INSTANCE_SCRATCH));
    assert!(!builder.allows_memory_write(MEMORY_LAYER_PROJECT));
    assert!(!builder.allows_memory_write(MEMORY_COLLECTION_USER_PRIVATE));

    let pm = RoleSpec::pm();
    assert!(pm.allows_knowledge(MEMORY_COLLECTION_USER_PREFS));
    assert!(pm.allows_knowledge(MEMORY_COLLECTION_PLANNING_UNRELEASED));
    assert!(!pm.allows_knowledge(MEMORY_COLLECTION_USER_PRIVATE));
    assert!(pm.allows_memory_write("department:planning"));
    assert!(!pm.allows_memory_write(MEMORY_LAYER_INSTANCE_SCRATCH));

    let reviewer = RoleSpec::reviewer();
    assert!(reviewer.allows_knowledge("project:events"));
    assert!(!reviewer.allows_knowledge("project:code"));
    assert!(!reviewer.allows_memory_write(MEMORY_LAYER_INSTANCE_SCRATCH));
    assert!(MemoryCollection::parse("not-a-layer").is_none());
}

#[test]
fn request_context_defaults_to_executing_builder() {
    let context = RequestContext::local("session-1", "/repo");
    assert_eq!(context.role_id, ROLE_BUILDER);
    assert_eq!(context.department_id, DEPARTMENT_EXECUTING);
    assert_eq!(context.work_packet_id, None);
    assert!(context.path_allow.is_empty());
}

#[test]
fn builder_lock_paths_exclusive_when_empty_and_overlap_on_prefix() {
    assert_eq!(builder_lock_paths(&[]), vec![EXCLUSIVE_PATH_LOCK]);
    assert_eq!(
        builder_lock_paths(&["ALPHA.txt".to_owned(), "ALPHA.txt".to_owned()]),
        vec!["ALPHA.txt"]
    );
    assert!(path_locks_conflict("ALPHA.txt", "ALPHA.txt"));
    assert!(path_locks_conflict("src", "src/lib.rs"));
    assert!(!path_locks_conflict("ALPHA.txt", "BRAVO.txt"));
    assert!(path_locks_conflict(EXCLUSIVE_PATH_LOCK, "ALPHA.txt"));
    assert!(allow_list_covers(&["ALPHA.txt".to_owned()], "ALPHA.txt"));
    assert!(!allow_list_covers(
        &["ALPHA.txt".to_owned()],
        "GOLDEN_PATH.txt"
    ));
}

#[test]
fn work_packet_prompt_is_only_packet_fields() {
    let mut packet = WorkPacket::builder_task("wp-1", "create GOLDEN_PATH.txt containing hello");
    packet.path_allow = vec!["GOLDEN_PATH.txt".to_owned()];
    packet.acceptance = vec!["file exists".to_owned()];
    packet.validate().unwrap();
    let prompt = packet.as_prompt();
    assert!(prompt.contains("Work packet wp-1"));
    assert!(prompt.contains("Goal: create GOLDEN_PATH.txt containing hello"));
    assert!(prompt.contains("Path allow: GOLDEN_PATH.txt"));
    assert!(!prompt.contains("PLANNER_SECRET_TOKEN"));
    let mut architect = packet.clone();
    architect.assignee_role = ROLE_ARCHITECT.to_owned();
    assert_eq!(architect.validate(), Err("packet_role_must_be_builder"));
    let allowed =
        WorkPacket::builder_task("wp-2", "create ALPHA.txt").with_path_allow(["ALPHA.txt"]);
    assert_eq!(allowed.path_allow, ["ALPHA.txt"]);
}

#[test]
fn planning_symposium_excludes_builder_and_prompt_is_blackboard_only() {
    let mut meeting = Symposium::planning("sym-1", "one vertical slice vs two packets", 2);
    meeting
        .blackboard
        .claims
        .push(SymposiumClaim::new(ROLE_PM, "choose the vertical slice"));
    assert_eq!(meeting.chair, ROLE_PM);
    assert_eq!(meeting.attendees, [ROLE_PM, ROLE_ARCHITECT]);
    assert!(!meeting.attendees.iter().any(|role| role == ROLE_BUILDER));
    let prompt = meeting.speaker_prompt(ROLE_ARCHITECT);
    assert!(prompt.contains("Speak as architect"));
    assert!(prompt.contains("Blackboard claims:"));
    assert!(prompt.contains("choose the vertical slice"));
    assert!(!prompt.contains("PLANNER_SECRET_TOKEN"));
    assert_eq!(
        Symposium::validate_max_rounds(0),
        Err("symposium_max_rounds_invalid")
    );
    meeting.validate().unwrap();
    let (decision, packet) = meeting.close(false).unwrap();
    let packet = packet.expect("planning packet");
    assert_eq!(decision.schema, DECISION_RECORD_SCHEMA);
    assert_eq!(packet.assignee_role, ROLE_BUILDER);
    assert!(!decision.skipped_meeting);
    assert_eq!(meeting.status, SYMPOSIUM_STATUS_CLOSED);
    assert_eq!(packet.goal, "one vertical slice vs two packets");

    let mut skipped = Symposium::planning("sym-2", "create GOLDEN_PATH.txt containing hello", 4);
    let (decision, packet) = skipped.close(true).unwrap();
    let packet = packet.expect("skipped planning packet");
    assert!(decision.skipped_meeting);
    assert_eq!(skipped.status, SYMPOSIUM_STATUS_SKIPPED);
    assert_eq!(packet.goal, "create GOLDEN_PATH.txt containing hello");

    let mut with_builder =
        Symposium::planning("sym-3", "create GOLDEN_PATH.txt containing hello", 4);
    with_builder.attendees.push(ROLE_BUILDER.to_owned());
    assert_eq!(
        with_builder.validate(),
        Err("symposium_builder_not_attendee")
    );
}

#[test]
fn each_department_can_convene_without_a_joint_meeting() {
    let expected = [
        (
            DEPARTMENT_INITIATING,
            ROLE_SPONSOR,
            INITIATING_DECISION_PATH,
            false,
        ),
        (DEPARTMENT_PLANNING, ROLE_PM, DECISION_RECORD_PATH, true),
        (
            DEPARTMENT_EXECUTING,
            ROLE_BUILDER,
            EXECUTING_DECISION_PATH,
            false,
        ),
        (
            DEPARTMENT_MONITORING,
            ROLE_REVIEWER,
            MONITORING_DECISION_PATH,
            false,
        ),
        (
            DEPARTMENT_CLOSING,
            ROLE_CLOSER,
            CLOSING_DECISION_PATH,
            false,
        ),
    ];
    for (department_id, chair, path, emits_packet) in expected {
        let mut meeting =
            Symposium::department(department_id, format!("sym-{department_id}"), "decide", 2)
                .unwrap();
        assert_eq!(meeting.chair, chair);
        assert_eq!(meeting.decision_path(), path);
        assert_eq!(
            meeting.builder_present(),
            department_id == DEPARTMENT_EXECUTING
        );
        meeting.validate().unwrap();
        let (decision, packet) = meeting.close(true).unwrap();
        assert!(decision.skipped_meeting);
        assert_eq!(packet.is_some(), emits_packet);
        if department_id != DEPARTMENT_EXECUTING {
            assert!(!meeting.attendees.iter().any(|role| role == ROLE_BUILDER));
        }
    }

    let mut architect_chair =
        Symposium::planning("sym-arch", "one vertical slice vs two packets", 2);
    architect_chair.chair = ROLE_ARCHITECT.to_owned();
    assert_eq!(
        architect_chair.validate(),
        Err("symposium_chair_must_be_pm")
    );

    let mut joint = Symposium::planning("sym-joint", "one vertical slice vs two packets", 2);
    joint.attendees = vec![ROLE_PM.to_owned(), ROLE_REVIEWER.to_owned()];
    assert_eq!(joint.validate(), Err("joint_symposium_frozen"));
}

#[test]
fn capability_request_serialization_contains_reference_not_secret_value() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Secret,
        "resolve",
        serde_json::json!({ "secret_ref": "provider/anthropic" }),
    );
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("secret_ref"));
    assert!(!json.contains("secret_value"));
}

#[test]
fn legacy_approval_challenge_defaults_risk_to_read_only() {
    let challenge: ApprovalChallenge = serde_json::from_value(serde_json::json!({
        "schema": APPROVAL_CHALLENGE_SCHEMA,
        "approval_id": ApprovalId::new(),
        "request_id": RequestId::new(),
        "request_hash": "sha256:legacy",
        "expires_at_unix_ms": 1,
        "reason": "approval_required",
        "nonce": "nonce",
        "policy_version": "kiana.policy.v1",
    }))
    .unwrap();

    assert_eq!(challenge.risk, RiskLevel::ReadOnly);
}

#[test]
fn authorization_and_event_invariants_fail_closed() {
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Query,
        "search",
        Value::Null,
    );
    assert_eq!(
        AuthorizedCapabilityRequest::new("", request).unwrap_err(),
        DomainError::EmptyAuthorizationId
    );
    assert_eq!(
        RuntimeEvent::new(RequestId::new(), 0, "invalid", Value::Null).unwrap_err(),
        DomainError::InvalidEventSequence
    );
}

#[test]
fn review_packet_rejects_author_session() {
    let packet = ReviewPacket::closed(
        "rv-1",
        "builder-1",
        ROLE_BUILDER,
        "builder-1",
        "pass",
        "same session",
        vec!["GOLDEN_PATH.txt".to_owned()],
    );
    assert_eq!(packet.validate(), Err("review_author_session_denied"));
}

#[test]
fn review_packet_requires_builder_author() {
    let packet = ReviewPacket::closed(
        "rv-1",
        "builder-1",
        ROLE_PM,
        "reviewer-1",
        "pass",
        "pm authored",
        Vec::new(),
    );
    assert_eq!(packet.validate(), Err("review_author_must_be_builder"));
}

#[test]
fn role_paths_are_normalized_and_traversal_is_rejected() {
    // 路径规范化必须消除无害的分隔符差异，同时拒绝绝对路径和目录穿越。
    assert_eq!(
        normalize_role_path(" ./src\\lib.rs "),
        Some("src/lib.rs".to_owned())
    );
    assert_eq!(normalize_role_path("."), Some(".".to_owned()));
    assert_eq!(normalize_role_path("/etc/passwd"), None);
    assert_eq!(normalize_role_path("../outside.txt"), None);
    assert_eq!(normalize_role_path("src/../../outside.txt"), None);
    assert_eq!(normalize_role_path(""), None);
}

#[test]
fn work_fingerprint_is_order_independent_and_requires_inputs() {
    // 指纹输入中的引用顺序不应影响结果；关键字段缺失时必须拒绝生成指纹。
    let first = WorkFingerprint::from_parts(
        " ship change ",
        &[" b ".to_owned(), "a".to_owned(), String::new()],
        " packet-1 ",
        " output-v1 ",
        " policy-v1 ",
    )
    .unwrap();
    let second = WorkFingerprint::from_parts(
        "ship change",
        &["a".to_owned(), "b".to_owned()],
        "packet-1",
        "output-v1",
        "policy-v1",
    )
    .unwrap();
    assert_eq!(first, second);
    assert!(first.as_str().starts_with("fnv1a64:"));

    // 四个契约字段任意一个为空都不能生成可审计的工作指纹。
    for (objective, partition, output, policy) in [
        ("", "packet", "output", "policy"),
        ("objective", "", "output", "policy"),
        ("objective", "packet", "", "policy"),
        ("objective", "packet", "output", ""),
    ] {
        assert_eq!(
            WorkFingerprint::from_parts(objective, &[], partition, output, policy),
            Err("work_fingerprint_input_required")
        );
    }
}
