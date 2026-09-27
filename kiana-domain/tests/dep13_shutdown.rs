use kiana_domain::{
    schema_contract, ShutdownAckStatus, ShutdownInput, ShutdownPhase, ShutdownPhaseEvidence,
    ShutdownReport, ShutdownStatus, SHUTDOWN_INPUT_SCHEMA, SHUTDOWN_VERSION,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn phases(status: ShutdownAckStatus) -> Vec<ShutdownPhaseEvidence> {
    ShutdownPhase::ALL
        .into_iter()
        .map(|phase| ShutdownPhaseEvidence::new(phase, status, 4, D, "ok").unwrap())
        .collect()
}

fn input() -> ShutdownInput {
    let mut input = ShutdownInput {
        schema: SHUTDOWN_INPUT_SCHEMA.to_owned(),
        version: SHUTDOWN_VERSION,
        now_unix_ms: 1_000,
        deadline_unix_ms: 2_000,
        cancellation_requested: true,
        intake_paused: true,
        scheduler_fenced: true,
        runner_drained: true,
        tools_drained: true,
        event_store_flushed: true,
        artifact_flushed: true,
        event_store_closed: true,
        active_work_count: 0,
        unknown_effect_count: 0,
        late_result_count: 0,
        phases: phases(ShutdownAckStatus::Confirmed),
        input_digest: String::new(),
    };
    input.input_digest = input.digest();
    input.validate().unwrap();
    input
}

#[test]
fn shutdown_schemas_and_normal_ack_produce_stopped() {
    for schema in [
        "kiana.shutdown-phase-evidence.v1",
        SHUTDOWN_INPUT_SCHEMA,
        "kiana.shutdown-report.v1",
    ] {
        let contract = schema_contract(schema).unwrap();
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert!(!contract.allow_unknown_fields);
    }
    let input = input();
    let report = ShutdownReport::evaluate(&input).unwrap();
    assert_eq!(report.status, ShutdownStatus::Stopped);
    report.validate_against(&input).unwrap();
}

#[test]
fn shutdown_never_claims_stopped_without_drain_or_flush_ack() {
    let mut active = input();
    active.active_work_count = 1;
    active.input_digest = active.digest();
    let report = ShutdownReport::evaluate(&active).unwrap();
    assert_eq!(report.status, ShutdownStatus::NeedsRecovery);

    let mut flush = input();
    flush.event_store_flushed = false;
    flush.input_digest = flush.digest();
    let report = ShutdownReport::evaluate(&flush).unwrap();
    assert_eq!(report.status, ShutdownStatus::NeedsRecovery);

    let mut late = input();
    late.late_result_count = 1;
    late.input_digest = late.digest();
    let report = ShutdownReport::evaluate(&late).unwrap();
    assert_eq!(report.status, ShutdownStatus::NeedsRecovery);

    let mut deadline = input();
    deadline.now_unix_ms = 2_000;
    deadline.input_digest = deadline.digest();
    let report = ShutdownReport::evaluate(&deadline).unwrap();
    assert_eq!(report.status, ShutdownStatus::NeedsRecovery);
}

#[test]
fn shutdown_unknown_phase_and_missing_ack_are_explicit() {
    let mut unknown = input();
    unknown.phases[3] = ShutdownPhaseEvidence::new(
        ShutdownPhase::SchedulerFence,
        ShutdownAckStatus::Unknown,
        4,
        D,
        "fence_unknown",
    )
    .unwrap();
    unknown.input_digest = unknown.digest();
    let report = ShutdownReport::evaluate(&unknown).unwrap();
    assert_eq!(report.status, ShutdownStatus::Unknown);

    let mut missing = input();
    missing.phases.pop();
    missing.input_digest = missing.digest();
    let report = ShutdownReport::evaluate(&missing).unwrap();
    assert_eq!(report.status, ShutdownStatus::NeedsRecovery);
}

#[test]
fn shutdown_rejects_tampered_report_and_out_of_order_phases() {
    let input = input();
    let mut report = ShutdownReport::evaluate(&input).unwrap();
    report.reason = "forged".to_owned();
    report.report_digest = report.digest();
    assert_eq!(
        report.validate_against(&input).unwrap_err(),
        "shutdown_report_binding_mismatch"
    );

    let mut malformed = input();
    malformed.phases.reverse();
    malformed.input_digest = malformed.digest();
    assert_eq!(
        malformed.validate().unwrap_err(),
        "shutdown_input_header_invalid"
    );
}
