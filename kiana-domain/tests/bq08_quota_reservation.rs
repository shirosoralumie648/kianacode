use kiana_domain::{
    AttemptId, ClockObservation, QuotaGroupKey, QuotaReservation, QuotaReservationId,
    QuotaReservationState, QuotaWindow, RunId,
};

fn reservation() -> QuotaReservation {
    let clock = ClockObservation::observe("fixture", 10_000, 10_000, None, 1).unwrap();
    let window = QuotaWindow::from_clock(&clock, 60_000).unwrap();
    QuotaReservation::new(
        QuotaReservationId::new(),
        QuotaGroupKey::new("provider", "credential", Some("model".to_owned()), None).unwrap(),
        window,
        RunId::new(),
        AttemptId::new(),
        3,
        "config:v1",
        1,
        10,
        1,
        "idempotency:one",
        70_000,
    )
    .unwrap()
}

#[test]
fn reservation_digest_and_state_fence_are_strict() {
    let mut reservation = reservation();
    reservation.validate().unwrap();
    assert_eq!(reservation.revision, 1);
    reservation
        .transition(QuotaReservationState::Unknown)
        .unwrap();
    assert_eq!(reservation.revision, 2);
    assert_eq!(reservation.state, QuotaReservationState::Unknown);
    reservation
        .transition(QuotaReservationState::Settled)
        .unwrap();
    assert_eq!(reservation.state, QuotaReservationState::Settled);
    assert!(reservation
        .transition(QuotaReservationState::Released)
        .is_err());
}

#[test]
fn reservation_rejects_digest_expiry_and_identity_drift() {
    let mut invalid = reservation();
    invalid.reservation_digest = "sha256:tampered".to_owned();
    assert_eq!(invalid.validate().unwrap_err(), "quota_reservation_invalid");
    let mut expired = reservation();
    expired.expires_at_unix_ms = expired.window.start_unix_ms;
    expired.reservation_digest = expired.digest();
    assert_eq!(expired.validate().unwrap_err(), "quota_reservation_invalid");
}
