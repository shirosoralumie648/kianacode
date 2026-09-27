use kiana_domain::{
    OperationId, QuiesceComponent, QuiesceObservation, QuiesceReport, QuiesceRequest,
    QuiesceStatus, QuiesceWriteState, QUIESCE_VERSION,
};
use uuid::Uuid;

const TARGET: u64 = 1_000;

fn operation() -> OperationId {
    OperationId::from_uuid(Uuid::from_u128(11))
}

fn observation(
    component: QuiesceComponent,
    write_state: QuiesceWriteState,
    durable_cursor: u64,
) -> QuiesceObservation {
    let wal_consistent = (component == QuiesceComponent::EventLogWal).then_some(true);
    QuiesceObservation::new(
        component,
        write_state,
        durable_cursor,
        durable_cursor,
        wal_consistent,
    )
    .expect("observation")
}

/// A fully quiesced store: every component idle and durable to the target cursor.
fn coherent() -> Vec<QuiesceObservation> {
    QuiesceComponent::ALL
        .iter()
        .map(|component| observation(*component, QuiesceWriteState::Idle, TARGET))
        .collect()
}

fn request(observations: Vec<QuiesceObservation>) -> QuiesceRequest {
    QuiesceRequest::new(operation(), "operator", TARGET, 2, 3, 900, observations).expect("request")
}

#[test]
fn a_coherent_store_is_quiesced_at_the_target_cursor() {
    let value = request(coherent());
    let report = QuiesceReport::evaluate(&value).expect("report");
    assert_eq!(report.status, QuiesceStatus::Quiesced);
    assert_eq!(report.snapshot_cursor, TARGET);
    assert_eq!(report.reason, "quiesce_snapshot_admitted");
    assert_eq!(report.remediation, "none");
}

#[test]
fn an_active_writer_or_unflushed_remainder_does_not_admit_a_snapshot() {
    for (state, reason) in [
        (QuiesceWriteState::ActiveWriter, "quiesce_active_writer"),
        (QuiesceWriteState::Draining, "quiesce_unflushed_remainder"),
        (QuiesceWriteState::Unknown, "quiesce_state_unknown"),
    ] {
        let mut observations = coherent();
        observations[0] = observation(QuiesceComponent::EventLogMain, state, TARGET);
        let report = QuiesceReport::evaluate(&request(observations)).expect("report");
        assert_ne!(report.status, QuiesceStatus::Quiesced, "state={state:?}");
        assert_eq!(report.reason, reason);
        // A refused decision must not hand back a cursor a caller could snapshot at.
        assert_eq!(report.snapshot_cursor, 0);
    }
}

#[test]
fn a_component_not_durable_to_the_target_is_inconsistent() {
    let mut observations = coherent();
    // The artifact store has only flushed up to an earlier cursor than the requested target.
    observations[2] = observation(
        QuiesceComponent::ArtifactStore,
        QuiesceWriteState::Idle,
        400,
    );
    let report = QuiesceReport::evaluate(&request(observations)).expect("report");
    assert_eq!(report.status, QuiesceStatus::Inconsistent);
    assert_eq!(report.reason, "quiesce_target_not_durable");
    assert_eq!(report.snapshot_cursor, 0);
}

#[test]
fn a_wal_that_disagrees_with_the_main_file_blocks_the_snapshot() {
    let mut observations = coherent();
    observations[1] = QuiesceObservation::new(
        QuiesceComponent::EventLogWal,
        QuiesceWriteState::Idle,
        TARGET,
        TARGET,
        Some(false),
    )
    .expect("observation");
    let report = QuiesceReport::evaluate(&request(observations)).expect("report");
    assert_eq!(report.status, QuiesceStatus::Inconsistent);
    assert_eq!(report.reason, "quiesce_wal_main_divergence");
}

#[test]
fn a_projector_cursor_ahead_of_the_source_is_rejected_before_any_decision() {
    // The request itself is invalid, so it never reaches the reducer.
    let error = QuiesceRequest::new(
        operation(),
        "operator",
        TARGET,
        2,
        3,
        TARGET + 1,
        coherent(),
    )
    .expect_err("projector ahead of source");
    assert_eq!(error, "quiesce_projector_cursor_ahead_of_source");
}

#[test]
fn every_component_must_be_observed_exactly_once() {
    let mut missing = coherent();
    missing.pop();
    let error = QuiesceRequest::new(operation(), "operator", TARGET, 2, 3, 900, missing)
        .expect_err("missing component");
    assert_eq!(error, "quiesce_request_missing_migration_registry");

    let duplicated = QuiesceRequest::new(
        operation(),
        "operator",
        TARGET,
        2,
        3,
        900,
        vec![
            observation(
                QuiesceComponent::EventLogMain,
                QuiesceWriteState::Idle,
                TARGET
            );
            5
        ],
    )
    .expect_err("duplicate components");
    assert_eq!(duplicated, "quiesce_request_observation_duplicate");
}

#[test]
fn a_wal_observation_must_state_its_consistency() {
    // Silence is not consistency: an observation that does not say whether the WAL agrees with
    // the main file cannot support a snapshot decision.
    let silent = QuiesceObservation::new(
        QuiesceComponent::EventLogWal,
        QuiesceWriteState::Idle,
        TARGET,
        TARGET,
        None,
    )
    .expect_err("wal consistency missing");
    assert_eq!(silent, "quiesce_observation_wal_consistency_missing");

    // A non-WAL component has no WAL relationship to report.
    let unexpected = QuiesceObservation::new(
        QuiesceComponent::ArtifactStore,
        QuiesceWriteState::Idle,
        TARGET,
        TARGET,
        Some(true),
    )
    .expect_err("wal consistency on a non-wal component");
    assert_eq!(unexpected, "quiesce_observation_wal_consistency_unexpected");
}

#[test]
fn a_regression_between_visible_and_durable_cursor_is_rejected() {
    let error = QuiesceObservation::new(
        QuiesceComponent::EventLogMain,
        QuiesceWriteState::Idle,
        TARGET,
        TARGET - 1,
        None,
    )
    .expect_err("durable ahead of visible");
    assert_eq!(error, "quiesce_observation_cursor_regression");
}

#[test]
fn a_forged_report_or_request_digest_fails_closed() {
    let value = request(coherent());
    let mut report = QuiesceReport::evaluate(&value).expect("report");

    // Admitting a cursor the store never reached must not survive revalidation.
    report.snapshot_cursor = TARGET + 5_000;
    report.report_digest = report.digest();
    let error = report.validate_against(&value).expect_err("forged cursor");
    assert_eq!(error, "quiesce_report_binding_invalid");

    // A stale digest must not authenticate a rewritten reason.
    let mut tampered = QuiesceReport::evaluate(&value).expect("report");
    tampered.reason = "quiesce_snapshot_admitted".to_owned();
    tampered.status = QuiesceStatus::Quiesced;
    let error = tampered.validate_against(&value).expect_err("stale digest");
    assert_eq!(error, "quiesce_report_digest_mismatch");

    assert_eq!(value.schema, "kiana.quiesce-request.v1");
    assert_eq!(report.schema, "kiana.quiesce-report.v1");
    assert_eq!(value.version, QUIESCE_VERSION);
}

#[test]
fn the_decision_is_deterministic_for_the_same_facts() {
    let value = request(coherent());
    let first = QuiesceReport::evaluate(&value).expect("first");
    let second = QuiesceReport::evaluate(&value).expect("second");
    assert_eq!(first, second);

    // A request whose facts are all refused must always report the same first violated rule,
    // regardless of the order the components were observed in.
    let mut observations = coherent();
    observations[0] = observation(
        QuiesceComponent::EventLogMain,
        QuiesceWriteState::Unknown,
        1,
    );
    observations[1] = observation(
        QuiesceComponent::EventLogWal,
        QuiesceWriteState::ActiveWriter,
        1,
    );
    let a = QuiesceReport::evaluate(&request(observations.clone())).expect("a");
    observations.reverse();
    let b = QuiesceReport::evaluate(&request(observations)).expect("b");
    assert_eq!(a.reason, b.reason);
    assert_eq!(a.reason, "quiesce_state_unknown");
}
