#[test]
fn company_commands_are_frozen_and_versioned() {
    let domain = include_str!("../../kiana-domain/src/company.rs");
    let replay = include_str!("../../kiana-domain/src/company_replay.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let core = include_str!("../src/company.rs");
    let policy = include_str!("../../kiana-domain/src/company_policy.rs");
    let baseline = include_str!("../../docs/roadmap/p3-i02-company-commands-baseline.md");

    for marker in [
        "COMPANY_COMMAND_SCHEMA",
        "COMPANY_EVENT_SCHEMA",
        "CompanyCommandRequest",
        "CompanyCommand",
        "CompanyEvent",
        "deny_unknown_fields",
        "event_name",
        "ProposeObjective",
        "ObjectiveProposed",
        "ApproveProject",
        "ProjectApproved",
        "CreateMilestone",
        "MilestoneCreated",
        "ApprovePacket",
        "PacketApproved",
        "StartRun",
        "RunStartRequested",
        "RecordRunStarted",
        "RunStarted",
        "RequestAcceptance",
        "AcceptanceRequested",
        "DecideAcceptance",
        "AcceptanceDecided",
        "CloseProject",
        "ProjectClosed",
        "RecordOutcome",
        "OutcomeRecorded",
        "company_replay_gap",
        "company_replay_duplicate_command",
        "company_event_schema_unsupported",
        "company_revision_conflict",
        "company_idempotency_conflict",
        "commit_company",
        "with_stream_metadata",
        "with_idempotency_key",
        "CompanyReplayReducer",
        "migrate_company_event",
        "company.command_rejected",
        "authorize_context",
        "CompanyCommandPolicy",
    ] {
        assert!(
            domain.contains(marker)
                || replay.contains(marker)
                || protocol.contains(marker)
                || core.contains(marker)
                || policy.contains(marker)
                || baseline.contains(marker),
            "company command/event marker missing: {marker}"
        );
    }

    for (command, event) in [
        ("ProposeObjective", "ObjectiveProposed"),
        ("ApproveProject", "ProjectApproved"),
        ("CreateMilestone", "MilestoneCreated"),
        ("ApprovePacket", "PacketApproved"),
        ("StartRun", "RunStartRequested"),
        ("RequestAcceptance", "AcceptanceRequested"),
        ("DecideAcceptance", "AcceptanceDecided"),
        ("CloseProject", "ProjectClosed"),
        ("RecordOutcome", "OutcomeRecorded"),
    ] {
        assert!(
            domain.contains(command),
            "frozen command missing: {command}"
        );
        assert!(domain.contains(event), "frozen event missing: {event}");
    }
    assert!(domain.contains("COMPANY_COMMAND_SCHEMA: &str = \"kiana.company-command.v1\""));
    assert!(domain.contains("COMPANY_EVENT_SCHEMA: &str = \"kiana.company-event.v1\""));
    assert!(core.contains("format!(\"company.{}\", event.request.command.event_name())"));
    assert!(replay.contains(
        "runtime_event.kind != format!(\"company.{}\", record.request.command.event_name())"
    ));
    assert!(!core.contains("CapabilityBroker"));
    assert!(!core.contains("ModelClient"));
}
