//! NM-20 failure-first fixtures: one named test per item in the card's rejected-first and
//! success columns, driven through the real seams of the notification chain.
//!
//! Card rejected-first column (all must be refused or fail closed):
//! `duplicate/out-of-order/gap`, `crash windows`, `slow consumer`, `queue full`, `disk full`,
//! `secret sentinel`, `projection loss`.
//!
//! Card success column (all must be provable from the driven result):
//! `bounded queue/backpressure`, `checkpoint/rebuild`, `redaction`, `lease reclaim`,
//! `multi-subscriber isolation`, `critical delivery query fallback`.
//!
//! Every test drives a real seam through `support::nm20_chain_seam` and compares against what
//! that seam actually returned. None of them asserts against a table that restates a policy.
//! The two channel/UI cases (`slow_subscriber`, `multi_subscriber_isolation`) are driven by the
//! daemon fixture target, which is the only place `NotificationStreamBridge` is reachable; here
//! they are reported as missing rather than faked.

mod support;

use kiana_core::{
    Nm20FaultCase, Nm20FaultFamily, Nm20FaultHarnessRun, Nm20FaultObservation, Nm20FaultSeam,
    Nm20LibrarySeam, NM20_FAULT_HARNESS_SCHEMA,
};
use serde_json::Value;

fn source_ids() -> Vec<String> {
    vec![
        "sha256:".to_owned() + &"1".repeat(64),
        "sha256:".to_owned() + &"2".repeat(64),
    ]
}

/// The full report this target can produce: ten cases through real seams (eight library, two
/// EventLog adapter), with the two daemon channel cases honestly missing.
fn run() -> Nm20FaultHarnessRun {
    support::nm20_chain_seam::run_all(20, 3, source_ids())
}

fn observation(case: Nm20FaultCase) -> Nm20FaultObservation {
    run()
        .observation(case)
        .unwrap_or_else(|| panic!("nm20 case {} not driven", case.as_str()))
        .clone()
}

/// The run always carries the safety invariants, whatever the case.
fn assert_invariants(observation: &Nm20FaultObservation) {
    assert!(
        !observation.effect_started,
        "an injected fault started an effect"
    );
    assert!(observation.secret_free, "a secret reached the report");
    assert!(observation.critical_preserved, "a critical fact was lost");
    observation.validate().expect("observation valid");
}

#[test]
fn duplicate_replay_materializes_one_row_and_a_stable_checkpoint() {
    let observation = observation(Nm20FaultCase::DuplicateReplay);
    assert_eq!(observation.family, Nm20FaultFamily::Duplicate);
    assert!(observation.refusal_fired);
    assert!(observation.duplicate_suppressed);
    assert!(
        observation.returned_ok,
        "an exact replay is idempotent, not an error"
    );
    assert_eq!(
        observation.refusal_code,
        "notification_projection_duplicate_suppressed"
    );
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("second notification row for one committed event")
    );
    assert_invariants(&observation);
}

#[test]
fn out_of_order_cursor_regression_is_refused_without_mutating_the_projection() {
    let observation = observation(Nm20FaultCase::CursorRegression);
    assert_eq!(observation.family, Nm20FaultFamily::OutOfOrder);
    assert!(!observation.returned_ok);
    assert_eq!(
        observation.refusal_code, "projection_checkpoint_cursor_not_advanced",
        "the checkpoint must refuse a stale tail"
    );
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("projection advanced by a stale cursor")
    );
    assert_invariants(&observation);
}

#[test]
fn cursor_gap_requires_snapshot_and_is_never_reported_as_completed() {
    let observation = observation(Nm20FaultCase::CursorGap);
    assert_eq!(observation.family, Nm20FaultFamily::CursorGap);
    assert!(observation.snapshot_required);
    assert!(!observation.returned_ok);
    assert_eq!(observation.refusal_code, "projection_checkpoint_cursor_gap");
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("gap reported as a completed position")
    );
    assert_invariants(&observation);
}

#[test]
fn crash_window_lease_reclaim_fences_the_stale_lease_and_submits_once() {
    let observation = observation(Nm20FaultCase::LeaseReclaimFence);
    assert_eq!(observation.family, Nm20FaultFamily::CrashWindow);
    assert!(observation.reconcile_required);
    assert_eq!(
        observation.refusal_code, "notification_outbox_lease_fence_mismatch",
        "the reclaimed record must reject the crashed worker's lease"
    );
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("stale lease submitting a reclaimed record")
    );
    assert!(!observation.returned_ok);
    assert_invariants(&observation);
}

#[test]
fn durable_crash_after_claim_reclaims_without_a_second_delivery() {
    let observation = observation(Nm20FaultCase::CrashAfterClaim);
    assert_eq!(observation.family, Nm20FaultFamily::CrashWindow);
    assert!(observation.refusal_fired);
    assert!(observation.duplicate_suppressed);
    assert_eq!(
        observation.refusal_code, "notification_outbox_lease_fence_mismatch",
        "the durable store must fence the crashed worker's lease"
    );
    assert_invariants(&observation);
}

#[test]
fn queue_full_sheds_best_effort_and_never_a_protected_record() {
    let observation = observation(Nm20FaultCase::BoundedQueueShed);
    assert_eq!(observation.family, Nm20FaultFamily::QueueFull);
    assert_eq!(
        observation.refusal_code,
        "notification_outbox_queue_full_shed_counted"
    );
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("critical/actionable notification shed by queue pressure")
    );
    assert!(
        observation.reconcile_required || observation.query_fallback_available,
        "a shed best-effort intent must still be accounted for"
    );
    assert_invariants(&observation);
}

#[test]
fn disk_full_write_is_classified_and_can_only_reconcile_unknown() {
    let observation = observation(Nm20FaultCase::DiskFullWriteRefusal);
    assert_eq!(observation.family, Nm20FaultFamily::DiskFull);
    assert!(observation.reconcile_required);
    assert_eq!(observation.refusal_code, "platform_disk_full");
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("disk-full write silently retried or marked delivered")
    );
    assert!(!observation.returned_ok);
    assert_invariants(&observation);
}

#[test]
fn secret_sentinel_is_quarantined_and_the_redacted_form_passes() {
    let observation = observation(Nm20FaultCase::SecretSentinel);
    assert_eq!(observation.family, Nm20FaultFamily::SecretSentinel);
    assert_eq!(
        observation.refusal_code, "secret_sentinel_quarantined",
        "the sentinel must be refused before it reaches a digest or toast"
    );
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("secret sentinel rendered, digested or persisted")
    );
    assert_invariants(&observation);
}

#[test]
fn projection_loss_rebuilds_byte_equivalently_from_committed_input() {
    let observation = observation(Nm20FaultCase::ProjectionLoss);
    assert_eq!(observation.family, Nm20FaultFamily::ProjectionLoss);
    assert!(observation.snapshot_required);
    assert!(observation.query_fallback_available);
    assert_eq!(
        observation.refusal_code,
        "notification_projection_rebuild_equivalent"
    );
    assert_invariants(&observation);
}

#[test]
fn critical_query_fallback_serves_the_committed_row_while_the_stream_is_gone() {
    let observation = observation(Nm20FaultCase::CriticalQueryFallback);
    assert!(observation.query_fallback_available);
    assert!(observation.snapshot_required);
    assert_eq!(
        observation.refusal_code, "notification_query_fallback_served",
        "the committed inbox must answer when the volatile stream cannot"
    );
    assert_eq!(
        observation.forbidden_value.as_deref(),
        Some("critical fact hidden behind a lagged volatile stream")
    );
    assert_invariants(&observation);
}

#[test]
fn library_seam_refuses_the_adapter_cases_instead_of_faking_them() {
    let mut seam = Nm20LibrarySeam;
    for case in Nm20FaultCase::ADAPTER_CASES {
        let refusal = seam
            .inject(case)
            .expect_err("the library seam must refuse an adapter-backed case");
        assert_eq!(
            refusal,
            format!("nm20_adapter_seam_required:{}", case.as_str())
        );
    }
    // The library-only run reports exactly the cases it did not drive.
    let library_run =
        Nm20FaultHarnessRun::evaluate_with_default(20, 3, source_ids()).expect("library-only run");
    assert_eq!(
        library_run.missing_cases(),
        Nm20FaultCase::ADAPTER_CASES.to_vec()
    );
    library_run.validate().expect("library-only run valid");
}

#[test]
fn report_shape_is_deterministic_and_replayable() {
    let first = run();
    let second = run();
    assert_eq!(first, second);
    assert_eq!(first.schema, NM20_FAULT_HARNESS_SCHEMA);
    assert_eq!(first.cases.len(), 10);
    assert_eq!(
        first.missing_cases(),
        vec![
            Nm20FaultCase::SlowSubscriber,
            Nm20FaultCase::MultiSubscriberIsolation
        ],
        "the daemon channel cases are driven by the daemon fixture target"
    );
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    first.validate().expect("run valid");

    // Every family named by the card appears at least once the two runs are combined; this run
    // covers everything except the slow-consumer family that the daemon target drives.
    let families: Vec<Nm20FaultFamily> = first
        .cases
        .iter()
        .map(|observation| observation.family)
        .collect();
    for family in [
        Nm20FaultFamily::Duplicate,
        Nm20FaultFamily::OutOfOrder,
        Nm20FaultFamily::CursorGap,
        Nm20FaultFamily::CrashWindow,
        Nm20FaultFamily::QueueFull,
        Nm20FaultFamily::DiskFull,
        Nm20FaultFamily::SecretSentinel,
        Nm20FaultFamily::ProjectionLoss,
    ] {
        assert!(
            families.contains(&family),
            "family missing: {}",
            family.as_str()
        );
    }
    assert!(
        !families.contains(&Nm20FaultFamily::SlowConsumer),
        "slow consumer is the daemon target's case"
    );
}

#[test]
fn fixture_captures_bounded_notification_fault_harness_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/nm20-notification-faults.json"))
            .expect("valid NM-20 fixture");
    for case in Nm20FaultCase::ALL {
        assert!(
            fixture["harness_cases"]
                .as_array()
                .expect("harness_cases")
                .iter()
                .any(|value| value == case.as_str()),
            "fixture missing harness case {}",
            case.as_str()
        );
    }
    for family in [
        "duplicate",
        "out_of_order",
        "cursor_gap",
        "crash_window",
        "slow_consumer",
        "queue_full",
        "disk_full",
        "secret_sentinel",
        "projection_loss",
    ] {
        assert!(
            fixture["harness_families"]
                .as_array()
                .expect("harness_families")
                .iter()
                .any(|value| value == family),
            "fixture missing family {family}"
        );
    }
    assert!(fixture["seam_split"]
        .as_array()
        .expect("seam_split")
        .iter()
        .any(|value| value == "kiana-core tests: ten cases (library + EventLog adapter)"));
    assert!(fixture["seam_split"]
        .as_array()
        .expect("seam_split")
        .iter()
        .any(|value| value == "kiana-daemon tests: ten cases (library + channel/UI bridge)"));
    assert_eq!(fixture["proof_ceiling"], "source");
}
