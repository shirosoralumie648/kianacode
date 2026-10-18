use kiana_domain::{
    connector_not_executed_stop_report, json_digest, ConnectorCancellationRecord,
    ConnectorCancellationSettlement, ConnectorCancellationState, ConnectorDispatchLifecycle,
    ConnectorDispatchStage, ConnectorLateResult, ConnectorLateResultDisposition,
    ConnectorLeaseSettlement, InvocationId, ProcessGroupState, StopMethod, StopReport,
};
use serde_json::json;

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn prepared() -> ConnectorDispatchLifecycle {
    ConnectorDispatchLifecycle::prepared(
        InvocationId::new(),
        1,
        digest("command"),
        digest("binding"),
        digest("payload"),
        digest("idempotency"),
    )
    .expect("prepared")
}

fn dispatching() -> ConnectorDispatchLifecycle {
    prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching")
}

fn confirmed_kill(lifecycle: &ConnectorDispatchLifecycle) -> StopReport {
    StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Kill,
        true,
        true,
        true,
        ProcessGroupState::Empty,
        true,
        25,
    )
    .expect("confirmed kill")
}

/// A "confirmed" report that never recorded a stop signal: incomplete evidence about started work.
fn signal_less_confirmed(lifecycle: &ConnectorDispatchLifecycle) -> StopReport {
    StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::None,
        false,
        false,
        true,
        ProcessGroupState::Empty,
        true,
        25,
    )
    .expect("signal-less confirmed report")
}

fn unconfirmed(lifecycle: &ConnectorDispatchLifecycle) -> StopReport {
    StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Term,
        true,
        false,
        false,
        ProcessGroupState::Present,
        false,
        25,
    )
    .expect("unconfirmed stop")
}

/// A hand-written settlement with recomputed digests.  It is internally consistent — which is the
/// point: the refusal must come from re-deriving the outcome from server-owned evidence.
fn forged_settlement(
    base: &ConnectorCancellationSettlement,
    state: ConnectorCancellationState,
) -> ConnectorCancellationSettlement {
    let mut forged = ConnectorCancellationSettlement {
        state,
        lease_settlement: state.lease_settlement(),
        settlement_digest: String::new(),
        ..base.clone()
    };
    forged.settlement_digest = forged.digest();
    forged
}

/// A hand-written record with recomputed digests around an already forged settlement.
fn forged_record(
    base: &ConnectorCancellationRecord,
    settlement: ConnectorCancellationSettlement,
) -> ConnectorCancellationRecord {
    let mut forged = ConnectorCancellationRecord {
        settlement,
        record_digest: String::new(),
        ..base.clone()
    };
    forged.record_digest = forged.digest();
    forged
}

#[test]
fn confirmed_stop_before_dispatch_releases_lease_as_not_executed() {
    let lifecycle = prepared();
    let report = connector_not_executed_stop_report(lifecycle.invocation_id).expect("stop");
    let settlement = ConnectorCancellationSettlement::from_stop_report(
        &lifecycle,
        digest("permit"),
        digest("lease"),
        &report,
        1_000,
        1_001,
    )
    .expect("settlement");
    assert_eq!(settlement.state, ConnectorCancellationState::NotExecuted);
    assert_eq!(
        settlement.lease_settlement,
        ConnectorLeaseSettlement::Released
    );
    assert_eq!(
        settlement.reject_late_result(&digest("late")).unwrap_err(),
        "connector_cancellation_late_result_fenced"
    );
}

#[test]
fn started_effect_keeps_lease_held_and_unconfirmed_stop_is_unknown() {
    let lifecycle = prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching");
    let confirmed = StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Kill,
        true,
        true,
        true,
        ProcessGroupState::Empty,
        true,
        25,
    )
    .expect("confirmed stop");
    let stopped = ConnectorCancellationSettlement::from_stop_report(
        &lifecycle,
        digest("permit"),
        digest("lease"),
        &confirmed,
        1_000,
        1_025,
    )
    .expect("stopped");
    assert_eq!(stopped.state, ConnectorCancellationState::StopConfirmed);
    assert_eq!(
        stopped.lease_settlement,
        ConnectorLeaseSettlement::HeldForReconciliation
    );

    let unknown_report = StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Term,
        true,
        false,
        false,
        ProcessGroupState::Unknown,
        false,
        25,
    )
    .expect("unknown stop");
    let unknown = ConnectorCancellationSettlement::from_stop_report(
        &lifecycle,
        digest("permit"),
        digest("lease"),
        &unknown_report,
        1_000,
        1_025,
    )
    .expect("unknown");
    assert_eq!(unknown.state, ConnectorCancellationState::Unknown);
    assert_eq!(
        unknown.lease_settlement,
        ConnectorLeaseSettlement::HeldForReconciliation
    );
}

#[test]
fn observed_or_malformed_late_material_cannot_be_cancelled_as_not_executed() {
    let observed = prepared()
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("dispatching")
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    let report = connector_not_executed_stop_report(observed.invocation_id).expect("stop");
    assert_eq!(
        ConnectorCancellationSettlement::from_stop_report(
            &observed,
            digest("permit"),
            digest("lease"),
            &report,
            1,
            2,
        )
        .unwrap_err(),
        "connector_cancellation_effect_already_observed"
    );

    let prepared = prepared();
    let mut value = serde_json::to_value(
        ConnectorCancellationSettlement::from_stop_report(
            &prepared,
            digest("permit"),
            digest("lease"),
            &connector_not_executed_stop_report(prepared.invocation_id).unwrap(),
            1,
            2,
        )
        .unwrap(),
    )
    .expect("encode");
    value["late_provider_result"] = json!(true);
    assert!(serde_json::from_value::<ConnectorCancellationSettlement>(value).is_err());
}

#[test]
fn a_cancellation_without_confirmed_stop_evidence_never_reports_not_executed() {
    let dispatching = dispatching();

    // A "confirmed" stop that never sent a signal is incomplete evidence about started work: it
    // cannot be recorded as a stopped effect, let alone as never executed.
    let signal_less = signal_less_confirmed(&dispatching);
    assert_eq!(
        ConnectorCancellationSettlement::from_stop_report(
            &dispatching,
            digest("permit"),
            digest("lease"),
            &signal_less,
            1_000,
            1_025,
        )
        .unwrap_err(),
        "connector_cancellation_stop_attempt_missing"
    );
    assert_eq!(
        ConnectorCancellationRecord::from_stop_report(
            &dispatching,
            digest("permit"),
            digest("lease"),
            &signal_less,
            1_000,
            1_025,
        )
        .unwrap_err(),
        "connector_cancellation_stop_attempt_missing"
    );

    // An unconfirmed stop is not a cancellation: it settles as Unknown with the lease held.
    let unconfirmed_report = unconfirmed(&dispatching);
    let mut record = ConnectorCancellationRecord::from_stop_report(
        &dispatching,
        digest("permit"),
        digest("lease"),
        &unconfirmed_report,
        1_000,
        1_025,
    )
    .expect("unknown settlement");
    assert_eq!(
        record.reconstruct_outcome(&dispatching).unwrap().0,
        ConnectorCancellationState::Unknown
    );
    assert_eq!(
        record.reconstruct_outcome(&dispatching).unwrap().1,
        ConnectorLeaseSettlement::HeldForReconciliation
    );
    assert!(!record.settlement.lease_settlement.releases_lease());

    // Even a hand-written settlement with recomputed digests cannot turn an unconfirmed report
    // into `not_executed`: the outcome is re-derived from the report, not read from the record.
    let forged_release = forged_record(
        &record,
        forged_settlement(&record.settlement, ConnectorCancellationState::NotExecuted),
    );
    forged_release
        .validate()
        .expect("the forged record is self-consistent");
    assert_eq!(
        forged_release
            .reconstruct_outcome(&dispatching)
            .unwrap_err(),
        "connector_cancellation_started_effect_cannot_release_lease"
    );

    // Claiming a confirmed stop that the report never confirmed is refused by the derivation
    // itself, before any lease rule can apply.
    let forged_stop = forged_record(
        &record,
        forged_settlement(
            &record.settlement,
            ConnectorCancellationState::StopConfirmed,
        ),
    );
    forged_stop
        .validate()
        .expect("the forged record is self-consistent");
    assert_eq!(
        forged_stop.reconstruct_outcome(&dispatching).unwrap_err(),
        "connector_cancellation_settlement_mismatch"
    );
    assert_eq!(
        forged_stop
            .settlement
            .validate_for_stop_report(&dispatching, &unconfirmed_report)
            .unwrap_err(),
        "connector_cancellation_settlement_mismatch"
    );

    // A settlement carrying a state/lease pair that the state itself does not imply is refused
    // before any reconstruction can trust it.
    let mut inconsistent = record.settlement.clone();
    inconsistent.lease_settlement = ConnectorLeaseSettlement::Released;
    inconsistent.settlement_digest = inconsistent.digest();
    assert_eq!(
        inconsistent.validate().unwrap_err(),
        "connector_cancellation_lease_settlement_invalid"
    );
}

#[test]
fn a_started_effect_cannot_release_its_lease_or_be_recorded_as_not_executed() {
    let dispatching = dispatching();
    let confirmed = confirmed_kill(&dispatching);
    let record = ConnectorCancellationRecord::from_stop_report(
        &dispatching,
        digest("permit"),
        digest("lease"),
        &confirmed,
        1_000,
        1_025,
    )
    .expect("stop confirmed");
    assert_eq!(
        record.reconstruct_outcome(&dispatching).unwrap(),
        (
            ConnectorCancellationState::StopConfirmed,
            ConnectorLeaseSettlement::HeldForReconciliation
        )
    );
    assert!(!record.settlement.lease_settlement.releases_lease());

    // A forged settlement that claims the started effect never ran (and released the lease) is
    // self-consistent but fails every reconstruction against the dispatching lifecycle.
    let forged = forged_record(
        &record,
        forged_settlement(&record.settlement, ConnectorCancellationState::NotExecuted),
    );
    forged.validate().expect("self-consistent forgery");
    assert_eq!(
        forged.reconstruct_outcome(&dispatching).unwrap_err(),
        "connector_cancellation_started_effect_cannot_release_lease"
    );
    assert_eq!(
        forged
            .settlement
            .validate_for_lifecycle(&dispatching)
            .unwrap_err(),
        "connector_cancellation_started_effect_cannot_release_lease"
    );

    // A record built before dispatch cannot be replayed against the started attempt: the lease
    // release would apply to an effect that may already exist.
    let prepared = prepared();
    let pre_dispatch = ConnectorCancellationRecord::from_stop_report(
        &prepared,
        digest("permit"),
        digest("lease"),
        &connector_not_executed_stop_report(prepared.invocation_id).expect("stop"),
        1_000,
        1_001,
    )
    .expect("not executed");
    assert_eq!(
        pre_dispatch
            .validate_for_lifecycle(&dispatching)
            .unwrap_err(),
        "connector_cancellation_started_effect_cannot_release_lease"
    );
    assert_eq!(
        pre_dispatch.reconstruct_outcome(&dispatching).unwrap_err(),
        "connector_cancellation_started_effect_cannot_release_lease"
    );

    // A settlement that drifted from the attempt it claims to settle is refused, not repaired.
    let mut drifted = record.clone();
    drifted.settlement.attempt = 2;
    drifted.settlement.settlement_digest = drifted.settlement.digest();
    drifted.record_digest = drifted.digest();
    assert_eq!(
        drifted.reconstruct_outcome(&dispatching).unwrap_err(),
        "connector_cancellation_lifecycle_mismatch"
    );
}

#[test]
fn a_late_result_cannot_resurrect_a_settled_attempt() {
    let dispatching = dispatching();
    let confirmed = confirmed_kill(&dispatching);
    let mut record = ConnectorCancellationRecord::from_stop_report(
        &dispatching,
        digest("permit"),
        digest("lease"),
        &confirmed,
        1_000,
        1_025,
    )
    .expect("stop confirmed");
    let outcome = record.reconstruct_outcome(&dispatching).expect("outcome");
    let settlement_digest = record.settlement.settlement_digest.clone();

    // A result addressed to another invocation or attempt, or one that was already known when the
    // attempt settled, is not late evidence at all.
    let foreign = ConnectorLateResult::new(InvocationId::new(), 1, digest("late"), 2_000)
        .expect("late result");
    assert_eq!(
        record.record_late_result(foreign).unwrap_err(),
        "connector_cancellation_late_result_invocation_mismatch"
    );
    let next_attempt =
        ConnectorLateResult::new(dispatching.invocation_id, 2, digest("late"), 2_000)
            .expect("late result");
    assert_eq!(
        record.record_late_result(next_attempt).unwrap_err(),
        "connector_cancellation_late_result_attempt_mismatch"
    );
    let not_late = ConnectorLateResult::new(dispatching.invocation_id, 1, digest("late"), 1_024)
        .expect("late result");
    assert_eq!(
        record.record_late_result(not_late).unwrap_err(),
        "connector_cancellation_late_result_not_late"
    );
    assert!(record.fenced_late_results.is_empty());

    // A properly addressed late result is fenced: it may be kept as evidence and nothing else.
    let late = ConnectorLateResult::new(dispatching.invocation_id, 1, digest("late"), 2_000)
        .expect("late result");
    assert_eq!(
        record.settlement.fence_late_result(&late).unwrap_err(),
        "connector_cancellation_late_result_fenced"
    );
    assert_eq!(
        record.record_late_result(late).expect("fenced evidence"),
        ConnectorLateResultDisposition::Fenced
    );
    assert_eq!(record.reconstruct_outcome(&dispatching).unwrap(), outcome);
    assert_eq!(record.settlement.settlement_digest, settlement_digest);
    assert_eq!(record.fenced_late_results.len(), 1);

    // The serialized replay reconstructs the same terminal outcome: recording the evidence moved
    // nothing, and no field of the envelope can turn it back into a live attempt.
    let encoded = serde_json::to_value(&record).expect("encode");
    let replayed: ConnectorCancellationRecord = serde_json::from_value(encoded).expect("decode");
    assert_eq!(replayed, record);
    assert_eq!(replayed.reconstruct_outcome(&dispatching).unwrap(), outcome);
    assert_eq!(replayed.fenced_late_results.len(), 1);

    // A settlement can never declare that a late result was accepted, and late evidence that
    // arrived before the settlement cannot be re-dated into lateness.
    let mut unfenced = record.settlement.clone();
    unfenced.late_result_fenced = false;
    unfenced.settlement_digest = unfenced.digest();
    assert_eq!(
        unfenced.validate().unwrap_err(),
        "connector_cancellation_settlement_invalid"
    );
    let mut re_dated = record.clone();
    re_dated.fenced_late_results[0].observed_at_unix_ms = 1_000;
    re_dated.record_digest = re_dated.digest();
    assert_eq!(
        re_dated.validate().unwrap_err(),
        "connector_cancellation_late_result_not_late"
    );

    // A record whose digest does not cover its evidence is refused before reconstruction.
    let mut drifted = record.clone();
    drifted.record_digest = digest("drift");
    assert_eq!(
        drifted.validate().unwrap_err(),
        "connector_cancellation_record_digest_mismatch"
    );
    let mut wrong_schema = record.clone();
    wrong_schema.schema = "kiana.connector-cancellation-record.v2".to_owned();
    assert_eq!(
        wrong_schema.validate().unwrap_err(),
        "connector_cancellation_record_invalid"
    );

    // Rewriting the forked settlement to claim the late result resurrected the attempt still
    // fails: the durable replay of the forged record is refused, not trusted.
    let forged = forged_record(
        &record,
        forged_settlement(&record.settlement, ConnectorCancellationState::NotExecuted),
    );
    forged.validate().expect("self-consistent forgery");
    let forged_replay: ConnectorCancellationRecord =
        serde_json::from_value(serde_json::to_value(&forged).expect("encode")).expect("decode");
    assert_eq!(
        forged_replay.reconstruct_outcome(&dispatching).unwrap_err(),
        "connector_cancellation_started_effect_cannot_release_lease"
    );
}

#[test]
fn the_three_cancellation_outcomes_reconstruct_from_stop_evidence() {
    // not_executed: nothing was ever started, so the confirmed stop gives the lease back.
    let prepared = prepared();
    let not_executed = ConnectorCancellationRecord::from_stop_report(
        &prepared,
        digest("permit"),
        digest("lease"),
        &connector_not_executed_stop_report(prepared.invocation_id).expect("stop"),
        1_000,
        1_001,
    )
    .expect("not executed record");

    // stop_confirmed: a started effect keeps its lease held for reconciliation.
    let dispatching = dispatching();
    let stop_confirmed = ConnectorCancellationRecord::from_stop_report(
        &dispatching,
        digest("permit"),
        digest("lease"),
        &confirmed_kill(&dispatching),
        1_000,
        1_025,
    )
    .expect("stop confirmed record");

    // unknown: an unconfirmed stop is not a cancellation, in either direction.
    let unknown = ConnectorCancellationRecord::from_stop_report(
        &dispatching,
        digest("permit"),
        digest("lease"),
        &unconfirmed(&dispatching),
        1_000,
        1_025,
    )
    .expect("unknown record");

    for (record, lifecycle, expected) in [
        (
            &not_executed,
            &prepared,
            (
                ConnectorCancellationState::NotExecuted,
                ConnectorLeaseSettlement::Released,
            ),
        ),
        (
            &stop_confirmed,
            &dispatching,
            (
                ConnectorCancellationState::StopConfirmed,
                ConnectorLeaseSettlement::HeldForReconciliation,
            ),
        ),
        (
            &unknown,
            &dispatching,
            (
                ConnectorCancellationState::Unknown,
                ConnectorLeaseSettlement::HeldForReconciliation,
            ),
        ),
    ] {
        record.validate().expect("record validates");
        record
            .validate_for_lifecycle(lifecycle)
            .expect("record binds its lifecycle");
        assert_eq!(record.reconstruct_outcome(lifecycle).unwrap(), expected);
        assert_eq!(record.settlement.state.lease_settlement(), expected.1);
        assert_eq!(record.settlement.lease_settlement, expected.1);
        assert!(record.settlement.late_result_fenced);
        assert!(record.fenced_late_results.is_empty());

        // The durable envelope round-trips and reconstructs the same outcome.
        let replayed: ConnectorCancellationRecord =
            serde_json::from_value(serde_json::to_value(record).expect("encode")).expect("decode");
        assert_eq!(replayed, *record);
        assert_eq!(replayed.reconstruct_outcome(lifecycle).unwrap(), expected);
    }

    // Only `not_executed` gives the lease back; a consumed lease is a committed dispatch result
    // and is never a cancellation outcome.
    assert!(ConnectorLeaseSettlement::Released.releases_lease());
    assert!(!ConnectorLeaseSettlement::HeldForReconciliation.releases_lease());
    assert!(!ConnectorLeaseSettlement::Consumed.releases_lease());

    // An attempt that already observed an effect can never be settled as cancelled, however the
    // stop evidence looks.
    let observed = dispatching
        .advance(
            ConnectorDispatchStage::Observed,
            Some(digest("receipt")),
            Some(digest("observation")),
            None,
            None,
        )
        .expect("observed");
    assert_eq!(
        ConnectorCancellationRecord::from_stop_report(
            &observed,
            digest("permit"),
            digest("lease"),
            &confirmed_kill(&observed),
            1_000,
            1_025,
        )
        .unwrap_err(),
        "connector_cancellation_effect_already_observed"
    );
}
