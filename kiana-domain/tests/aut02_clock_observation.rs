use kiana_domain::{ClockObservation, ClockTrust};

#[test]
fn trusted_clock_observation_is_digest_bound_and_rollbacks_are_untrusted() {
    let first = ClockObservation::observe("fake", 1_000, 10, None, 1).unwrap();
    assert_eq!(first.trust, ClockTrust::Trusted);
    assert!(first.allows_before(2_000).unwrap());
    assert_eq!(first.clamp_deadline(1_900, 2_000).unwrap(), 1_900);

    let rollback = ClockObservation::observe("fake", 900, 9, Some(&first), 2).unwrap();
    assert_eq!(rollback.trust, ClockTrust::Rollback);
    assert_eq!(
        rollback.allows_before(2_000).unwrap_err(),
        "clock_untrusted"
    );
    assert_eq!(rollback.validate_transition_from(&first).unwrap(), ());
}

#[test]
fn zero_overflow_revision_and_digest_tamper_fail_closed() {
    assert_eq!(
        ClockObservation::observe("fake", 0, 1, None, 1).unwrap_err(),
        "clock_zero_sample"
    );
    assert_eq!(
        ClockObservation::observe("fake", u128::from(u64::MAX) + 1, 1, None, 1).unwrap_err(),
        "clock_wall_overflow"
    );
    let first = ClockObservation::observe("fake", 1_000, 10, None, 1).unwrap();
    assert_eq!(
        ClockObservation::observe("fake", 1_001, 11, Some(&first), 1).unwrap_err(),
        "clock_revision_invalid"
    );
    let mut tampered = first;
    tampered.wall_now_unix_ms = 1_001;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "clock_observation_invalid"
    );
}
