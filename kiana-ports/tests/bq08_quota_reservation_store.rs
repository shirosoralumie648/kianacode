use kiana_domain::{
    AttemptId, ClockObservation, QuotaGroupKey, QuotaReservation, QuotaReservationId,
    QuotaReservationState, QuotaWindow, RunId,
};
use kiana_ports::{InMemoryQuotaReservationStore, QuotaReservationPort};

fn reservation() -> QuotaReservation {
    let clock = ClockObservation::observe("fixture", 10_000, 10_000, None, 1).unwrap();
    QuotaReservation::new(
        QuotaReservationId::new(),
        QuotaGroupKey::new("provider", "credential", Some("model".to_owned()), None).unwrap(),
        QuotaWindow::from_clock(&clock, 60_000).unwrap(),
        RunId::new(),
        AttemptId::new(),
        4,
        "config:v1",
        1,
        10,
        1,
        "idem:one",
        70_000,
    )
    .unwrap()
}

#[tokio::test]
async fn reservation_store_replays_idempotently_and_fences_transitions() {
    let store = InMemoryQuotaReservationStore::new();
    let reservation = reservation();
    let id = reservation.reservation_id;
    let stored = store
        .reserve_quota(reservation.clone(), None)
        .await
        .unwrap();
    assert_eq!(stored, reservation);
    assert_eq!(
        store
            .reserve_quota(reservation.clone(), None)
            .await
            .unwrap(),
        stored
    );
    let transitioned = store
        .transition_quota(id, 1, 4, "config:v1", QuotaReservationState::Settled)
        .await
        .unwrap();
    assert_eq!(transitioned.revision, 2);
    assert_eq!(transitioned.state, QuotaReservationState::Settled);
    assert_eq!(
        store
            .transition_quota(id, 1, 4, "config:v1", QuotaReservationState::Released)
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:quota_reservation_revision_stale"
    );
    assert_eq!(
        store
            .transition_quota(id, 2, 5, "config:v1", QuotaReservationState::Released)
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:quota_reservation_fence_stale"
    );
}

#[tokio::test]
async fn reservation_store_rejects_digest_conflict_and_missing_expected() {
    let store = InMemoryQuotaReservationStore::new();
    let reservation = reservation();
    let id = reservation.reservation_id;
    store
        .reserve_quota(reservation.clone(), None)
        .await
        .unwrap();
    let mut conflict = reservation;
    conflict.requested_tokens = 11;
    conflict.reservation_digest = conflict.digest();
    assert_eq!(
        store
            .reserve_quota(conflict, None)
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:quota_reservation_digest_or_revision_conflict"
    );
    assert_eq!(
        store
            .reserve_quota(
                QuotaReservation::new(
                    id,
                    kiana_domain::QuotaGroupKey::new("provider", "credential", None, None).unwrap(),
                    kiana_domain::QuotaWindow::from_clock(
                        &kiana_domain::ClockObservation::observe(
                            "fixture", 10_000, 10_000, None, 1,
                        )
                        .unwrap(),
                        60_000,
                    )
                    .unwrap(),
                    RunId::new(),
                    AttemptId::new(),
                    4,
                    "config:v1",
                    1,
                    10,
                    1,
                    "idem:two",
                    70_000,
                )
                .unwrap(),
                Some(1),
            )
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:quota_reservation_digest_or_revision_conflict"
    );
}
