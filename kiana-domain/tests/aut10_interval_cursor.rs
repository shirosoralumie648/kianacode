use kiana_domain::{plan_interval_due, MissedSchedulePolicy, MAX_INTERVAL_CATCH_UP};

#[test]
fn not_due_keeps_cursor_without_creating_occurrence() {
    let batch = plan_interval_due(2_000, 1_000, 1_999, MissedSchedulePolicy::CatchUp)
        .expect("valid future cursor");
    assert!(batch.occurrence_keys.is_empty());
    assert_eq!(batch.next_at, 2_000);
}

#[test]
fn skip_and_fire_once_advance_past_all_missed_slots() {
    let skipped =
        plan_interval_due(1_000, 1_000, 5_500, MissedSchedulePolicy::Skip).expect("skip batch");
    assert!(skipped.occurrence_keys.is_empty());
    assert_eq!(skipped.skipped, 5);
    assert_eq!(skipped.next_at, 6_000);

    let once = plan_interval_due(1_000, 1_000, 5_500, MissedSchedulePolicy::FireOnce)
        .expect("fire once batch");
    assert_eq!(once.occurrence_keys, vec!["schedule:1000"]);
    assert_eq!(once.skipped, 4);
    assert_eq!(once.next_at, 6_000);
}

#[test]
fn catch_up_is_bounded_and_preserves_the_remaining_cursor() {
    let batch = plan_interval_due(1_000, 1_000, 50_000, MissedSchedulePolicy::CatchUp)
        .expect("catch up batch");
    assert_eq!(batch.emitted, MAX_INTERVAL_CATCH_UP);
    assert_eq!(
        batch.occurrence_keys.first().map(String::as_str),
        Some("schedule:1000")
    );
    assert_eq!(
        batch.occurrence_keys.last().map(String::as_str),
        Some("schedule:32000")
    );
    assert_eq!(batch.backlog_remaining, 18);
    assert!(batch.catch_up_limited);
    assert_eq!(batch.next_at, 33_000);

    let remainder = plan_interval_due(batch.next_at, 1_000, 50_000, MissedSchedulePolicy::CatchUp)
        .expect("bounded remainder");
    assert_eq!(remainder.emitted, 18);
    assert_eq!(remainder.next_at, 51_000);
}

#[test]
fn cursor_overflow_and_invalid_clock_fail_closed() {
    assert_eq!(
        plan_interval_due(
            u64::MAX - 500,
            1_000,
            u64::MAX,
            MissedSchedulePolicy::FireOnce
        )
        .expect_err("overflow"),
        "trigger_interval_cursor_overflow"
    );
    assert_eq!(
        plan_interval_due(1_000, 1_000, 0, MissedSchedulePolicy::Skip).expect_err("zero clock"),
        "trigger_interval_cursor_invalid"
    );
}
