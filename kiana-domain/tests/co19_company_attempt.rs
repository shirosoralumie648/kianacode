use kiana_domain::*;

fn claim(
    ledger: &mut PacketAttemptLedger,
    packet: &str,
    session: &str,
    request: RequestId,
    now: u64,
    expiry: u64,
) -> PacketAttempt {
    ledger
        .claim(
            packet,
            1,
            CellId::new(),
            SessionId::new(session),
            request,
            now,
            expiry,
        )
        .expect("claim")
}

#[test]
fn two_claimers_cannot_start_two_runs_for_one_packet() {
    let mut ledger = PacketAttemptLedger::default();
    let request = RequestId::new();
    let first = claim(&mut ledger, "packet-1", "worker-1", request, 10, 100);
    let replay = claim(&mut ledger, "packet-1", "worker-1", request, 20, 100);
    assert_eq!(replay.attempt_id, first.attempt_id);
    assert_eq!(replay.epoch, first.epoch);
    assert_eq!(
        ledger
            .claim(
                "packet-1",
                1,
                CellId::new(),
                SessionId::new("worker-2"),
                RequestId::new(),
                20,
                100,
            )
            .unwrap_err(),
        "packet_attempt_already_claimed"
    );
}

#[test]
fn confirmed_stopped_attempt_can_be_reclaimed_with_full_history() {
    let mut ledger = PacketAttemptLedger::default();
    let request = RequestId::new();
    let first = claim(&mut ledger, "packet-1", "worker-1", request, 10, 100);
    ledger
        .confirm_stopped("packet-1", first.attempt_id, first.epoch, request, 50)
        .expect("stop");
    assert_eq!(
        ledger
            .fence_expired("packet-1", first.attempt_id, first.epoch, 100)
            .expect("fence"),
        PacketAttemptStatus::Fenced
    );
    let second = claim(
        &mut ledger,
        "packet-1",
        "worker-2",
        RequestId::new(),
        200,
        300,
    );
    assert_eq!(second.epoch, first.epoch + 1);
    assert_eq!(ledger.history("packet-1").len(), 2);
    assert_eq!(
        ledger
            .complete(
                "packet-1",
                first.attempt_id,
                first.epoch,
                request,
                PacketAttemptStatus::Succeeded,
                None,
                210,
            )
            .unwrap_err(),
        "packet_attempt_fence_mismatch"
    );
}

#[test]
fn expired_claim_with_unconfirmed_effect_cannot_be_reassigned() {
    let mut ledger = PacketAttemptLedger::default();
    let request = RequestId::new();
    let attempt = claim(&mut ledger, "packet-unknown", "worker", request, 10, 100);
    ledger
        .mark_started(
            "packet-unknown",
            attempt.attempt_id,
            attempt.epoch,
            request,
            20,
        )
        .expect("started");
    assert_eq!(
        ledger
            .fence_expired("packet-unknown", attempt.attempt_id, attempt.epoch, 100)
            .expect("fence"),
        PacketAttemptStatus::ResultUnknown
    );
    assert_eq!(
        ledger
            .claim(
                "packet-unknown",
                1,
                CellId::new(),
                SessionId::new("retry"),
                RequestId::new(),
                200,
                300,
            )
            .unwrap_err(),
        "packet_attempt_result_unknown_requires_reconcile"
    );
}

#[test]
fn terminal_attempts_are_serializable_and_keep_epoch_identity() {
    let mut ledger = PacketAttemptLedger::default();
    let request = RequestId::new();
    let attempt = claim(&mut ledger, "packet-done", "worker", request, 10, 100);
    ledger
        .complete(
            "packet-done",
            attempt.attempt_id,
            attempt.epoch,
            request,
            PacketAttemptStatus::Succeeded,
            Some(format!("sha256:{}", "a".repeat(64))),
            20,
        )
        .expect("complete");
    let reopened: PacketAttemptLedger =
        serde_json::from_value(serde_json::to_value(&ledger).expect("serialize")).expect("reopen");
    assert_eq!(reopened, ledger);
    assert_eq!(reopened.history("packet-done")[0].epoch, 1);
    assert!(reopened.validate().is_ok());
}
