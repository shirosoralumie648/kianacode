//! PD-27 failure-first fixtures for the bounded writer queue contract.
//!
//! One test per "先拒绝的" column of the card, in the card's own order:
//! silent drop after the queue fills, a write after shutdown returned, and a cancelled writer
//! leaving a lock or lease behind. Each test names the stable refusal code it expects, so a
//! regression that turned a refusal into a silent success fails here first.

use kiana_domain::{
    CapacityEnvelope, EventStoreHealth, FenceTokenId, InstanceId, RunId, StorageLockId,
    StorageRootId,
};
use kiana_eventlog::{
    WriterAdmission, WriterAdmissionReport, WriterAdmissionStatus, WriterQueuePolicy,
    WriterQueueState, WriterRegistry, WriterShutdown, WriterShutdownMode, WriterTakeover,
    WriterTakeoverReport, WRITER_QUEUE_VERSION,
};

const EPOCH: u64 = 4;
const NEXT_EPOCH: u64 = 5;
const ACQUIRED: u64 = 1_000;
const EXPIRES: u64 = 2_000;

fn root() -> StorageRootId {
    StorageRootId::parse_str("00000000-0000-5000-8000-000000000000").expect("root")
}
fn lock() -> StorageLockId {
    StorageLockId::parse_str("00000000-0000-5000-8000-00000000000b").expect("lock")
}
fn holder() -> InstanceId {
    InstanceId::parse_str("00000000-0000-5000-8000-00000000000d").expect("holder")
}
fn successor() -> InstanceId {
    InstanceId::parse_str("00000000-0000-5000-8000-000000000011").expect("successor")
}
fn fence() -> FenceTokenId {
    FenceTokenId::parse_str("00000000-0000-5000-8000-000000000013").expect("fence")
}
fn next_fence() -> FenceTokenId {
    FenceTokenId::parse_str("00000000-0000-5000-8000-000000000017").expect("next fence")
}

/// A bounded policy: 8 frames, 4096 buffered bytes, 4 writers, 2 pending per writer.
fn policy() -> WriterQueuePolicy {
    WriterQueuePolicy::new(8, 4_096, 4, 2, 1_024, 5_000).expect("policy")
}

/// The writer lease as the holder instance sees it.
fn registry() -> WriterRegistry {
    WriterRegistry::new(
        root(),
        lock(),
        holder(),
        "daemon-primary",
        EPOCH,
        fence(),
        ACQUIRED,
        EXPIRES,
    )
    .expect("registry")
}

/// An append attempt by the holder against an open queue with the given occupancy.
fn admission(
    state: WriterQueueState,
    depth: u64,
    bytes: u64,
    writer_pending: u64,
    count: u64,
    frame_bytes: u64,
) -> WriterAdmission {
    WriterAdmission::new(
        holder(),
        RunId::parse_str("00000000-0000-5000-8000-00000000001d").expect("run"),
        "daemon-primary",
        EPOCH,
        state,
        depth,
        bytes,
        writer_pending,
        count,
        frame_bytes,
    )
    .expect("admission")
}

fn decide(admission: WriterAdmission) -> Result<WriterAdmissionReport, String> {
    WriterAdmissionReport::evaluate(&policy(), &registry(), &admission)
}

#[allow(clippy::too_many_arguments)]
fn shutdown(
    mode: WriterShutdownMode,
    pending_depth: u64,
    pending_bytes: u64,
    durable_before: u64,
    durable_after: u64,
    admitted: u64,
    writers: u32,
    leases: Vec<&str>,
    released: bool,
) -> Result<WriterShutdown, String> {
    WriterShutdown::new(
        lock(),
        holder(),
        EPOCH,
        mode,
        WriterQueueState::Draining,
        pending_depth,
        pending_bytes,
        durable_before,
        durable_after,
        admitted,
        writers,
        leases.into_iter().map(str::to_owned).collect(),
        released,
    )
}

/// A shutdown that would be accepted: drained, nothing outstanding, lock handed back.
fn clean(mode: WriterShutdownMode) -> Result<WriterShutdown, String> {
    shutdown(
        mode,
        0,
        0,
        10,
        12,
        12,
        0,
        Vec::new(),
        mode != WriterShutdownMode::HardKill,
    )
}

#[test]
fn an_open_queue_below_every_bound_admits_the_append() {
    let report = decide(admission(WriterQueueState::Open, 3, 1_000, 1, 1, 512)).expect("report");
    assert_eq!(report.status, WriterAdmissionStatus::Accepted);
    assert_eq!(report.reason, "writer_append_admitted");
    assert_eq!(report.remediation, "none");
    // An accepted append reports the depth and bytes it actually added, not a refusal.
    assert_eq!(report.accepted_depth_after, 4);
    assert_eq!(report.accepted_bytes_after, 1_512);
}

#[test]
fn a_full_queue_refuses_observably_and_admits_nothing() {
    // The card's first refusal: 8 queued frames plus one more exceeds the declared depth.
    let report = decide(admission(WriterQueueState::Open, 8, 1_000, 0, 1, 512)).expect("report");
    assert_eq!(report.status, WriterAdmissionStatus::Rejected);
    assert_eq!(report.reason, "writer_queue_full");
    // The refusal must not be readable as a success: zero admitted depth and zero admitted
    // bytes. A silent drop would have looked exactly like this append landing somewhere.
    assert_eq!(report.accepted_depth_after, 0);
    assert_eq!(report.accepted_bytes_after, 0);
}

#[test]
fn a_forged_admission_that_reports_admitted_depth_on_a_refusal_fails_closed() {
    let mut report =
        decide(admission(WriterQueueState::Open, 8, 1_000, 0, 1, 512)).expect("report");
    // Re-sealing a refusal so it claims it queued the frame anyway must not survive revalidation.
    report.accepted_depth_after = 9;
    report.report_digest = report.digest();
    let error = report
        .validate_against(
            &policy(),
            &registry(),
            &admission(WriterQueueState::Open, 8, 1_000, 0, 1, 512),
        )
        .expect_err("forged admitted depth");
    assert_eq!(error, "writer_report_admitted_depth_leak");
}

#[test]
fn a_queue_over_its_byte_bound_refuses_before_it_is_depth_full() {
    // 4096 buffered bytes is the cap; the byte rule is a separate bound, not a restatement of
    // the depth rule, so it must report its own code even though depth still has room.
    let report = decide(admission(WriterQueueState::Open, 1, 4_096, 0, 1, 512)).expect("report");
    assert_eq!(report.status, WriterAdmissionStatus::Rejected);
    assert_eq!(report.reason, "writer_queue_bytes_exceeded");
    assert_eq!(report.accepted_depth_after, 0);
}

#[test]
fn a_per_writer_pending_cap_refuses_a_second_writer_burst() {
    // max_pending_per_writer is 2, so a writer already holding 2 cannot add another frame even
    // though the queue itself is nearly empty.
    let report = decide(admission(WriterQueueState::Open, 1, 512, 2, 1, 256)).expect("report");
    assert_eq!(report.status, WriterAdmissionStatus::Rejected);
    assert_eq!(report.reason, "writer_per_writer_limit_exceeded");
}

#[test]
fn a_batch_or_frame_above_its_own_cap_is_refused() {
    let oversize_frame =
        decide(admission(WriterQueueState::Open, 0, 0, 0, 1, 2_048)).expect("frame");
    assert_eq!(oversize_frame.reason, "writer_frame_oversize");

    // A three-frame batch exceeds the per-writer pending cap of 2.
    let oversize_batch = decide(admission(WriterQueueState::Open, 0, 0, 0, 3, 128)).expect("batch");
    assert_eq!(oversize_batch.reason, "writer_batch_oversize");
}

#[test]
fn an_append_after_shutdown_returned_is_refused_not_parked_in_a_finished_drain() {
    // The card's second refusal: once close has returned, a late append must be refused.
    for (state, reason) in [
        (WriterQueueState::Closed, "writer_queue_closed"),
        (WriterQueueState::Draining, "writer_queue_draining"),
        (WriterQueueState::Unknown, "writer_queue_state_unknown"),
    ] {
        let report = decide(admission(state, 0, 0, 0, 1, 128)).expect("report");
        assert_eq!(
            report.status,
            WriterAdmissionStatus::Rejected,
            "state={state:?}"
        );
        assert_eq!(report.reason, reason);
        assert_eq!(report.accepted_depth_after, 0);
    }
}

#[test]
fn an_unestablished_state_outranks_a_closed_queue() {
    // Both refuse, but an unestablished state must not be reported as a specific refusal: it
    // reports `unknown` first so the operator investigates rather than trusting the reason.
    let report =
        decide(admission(WriterQueueState::Unknown, 99_999, 0, 0, 1, 128)).expect("report");
    assert_eq!(report.reason, "writer_queue_state_unknown");
}

#[test]
fn a_second_instance_is_refused_the_holders_writer_lease() {
    // Multi-process writer: only the registry holder may append. A second instance must take the
    // lease over explicitly, so the two writers cannot interleave appends unnoticed.
    let foreign = WriterAdmission::new(
        successor(),
        RunId::parse_str("00000000-0000-5000-8000-00000000001f").expect("foreign run"),
        "daemon-secondary",
        EPOCH,
        WriterQueueState::Open,
        0,
        0,
        0,
        1,
        128,
    )
    .expect("admission");
    let report = WriterAdmissionReport::evaluate(&policy(), &registry(), &foreign).expect("report");
    assert_eq!(report.status, WriterAdmissionStatus::Rejected);
    assert_eq!(report.reason, "writer_instance_not_registry_holder");
    assert_eq!(report.accepted_bytes_after, 0);
}

#[test]
fn a_stale_data_epoch_is_refused_before_any_capacity_rule() {
    let stale = WriterAdmission::new(
        holder(),
        RunId::parse_str("00000000-0000-5000-8000-00000000001d").expect("run"),
        "daemon-primary",
        EPOCH - 1,
        WriterQueueState::Open,
        0,
        0,
        0,
        1,
        128,
    )
    .expect("admission");
    let report = WriterAdmissionReport::evaluate(&policy(), &registry(), &stale).expect("report");
    assert_eq!(report.reason, "writer_epoch_stale");
}

#[test]
fn a_cancelled_writer_that_leaves_a_lease_behind_is_refused() {
    // The card's third refusal: cancellation must hand back the lock AND the lease. A lease left
    // behind is ownership nobody can revoke, and the next process inherits the ambiguity.
    let residue = shutdown(
        WriterShutdownMode::Cancelled,
        0,
        0,
        10,
        12,
        12,
        0,
        vec!["memory/project-a"],
        true,
    )
    .expect_err("lease residue");
    assert_eq!(residue, "writer_shutdown_lease_residue");

    // The same writer, having released everything, is accepted.
    let cancelled = clean(WriterShutdownMode::Cancelled).expect("cancelled");
    assert!(cancelled.validate().is_ok());
}

#[test]
fn a_cancelled_writer_that_does_not_release_the_lock_is_refused() {
    let held = shutdown(
        WriterShutdownMode::Cancelled,
        0,
        0,
        10,
        12,
        12,
        0,
        Vec::new(),
        false,
    )
    .expect_err("lock not released");
    assert_eq!(held, "writer_shutdown_lock_not_released");
}

#[test]
fn a_shutdown_that_returns_with_a_writer_still_appending_is_refused() {
    // The card's second refusal, from the other side: shutdown must not return while a writer is
    // still appending behind it.
    for (writers, reason) in [
        (1u32, "writer_shutdown_outstanding_writer"),
        (0, "writer_shutdown_pending_remainder"),
    ] {
        let error = shutdown(
            WriterShutdownMode::Graceful,
            0,
            if reason == "writer_shutdown_pending_remainder" {
                4
            } else {
                0
            },
            10,
            12,
            12,
            writers,
            Vec::new(),
            true,
        )
        .expect_err("outstanding writer");
        assert_eq!(error, reason);
    }

    // Bytes still buffered are the same failure seen from the byte counter.
    let buffered = shutdown(
        WriterShutdownMode::Graceful,
        0,
        8,
        10,
        12,
        12,
        0,
        Vec::new(),
        true,
    )
    .expect_err("buffered bytes");
    assert_eq!(buffered, "writer_shutdown_pending_remainder");
}

#[test]
fn a_shutdown_that_claims_more_durable_than_it_flushed_is_refused() {
    let unflushed = shutdown(
        WriterShutdownMode::Graceful,
        0,
        0,
        10,
        12,
        20,
        0,
        Vec::new(),
        true,
    )
    .expect_err("unflushed remainder");
    assert_eq!(unflushed, "writer_shutdown_unflushed_remainder");

    let regressed = shutdown(
        WriterShutdownMode::Graceful,
        0,
        0,
        12,
        10,
        10,
        0,
        Vec::new(),
        true,
    )
    .expect_err("cursor regression");
    assert_eq!(regressed, "writer_shutdown_durable_cursor_regression");
}

#[test]
fn a_hard_killed_writer_can_claim_no_lock_release_and_no_new_durability() {
    // A kill-9 runs no code after the kill, so a record claiming it released the lock or made
    // more bytes durable is describing something that did not happen.
    let released = shutdown(
        WriterShutdownMode::HardKill,
        0,
        0,
        10,
        10,
        10,
        0,
        Vec::new(),
        true,
    )
    .expect_err("dead writer released the lock");
    assert_eq!(released, "writer_shutdown_dead_writer_released_lock");

    let flushed = shutdown(
        WriterShutdownMode::HardKill,
        0,
        0,
        10,
        12,
        10,
        0,
        Vec::new(),
        false,
    )
    .expect_err("dead writer flushed");
    assert_eq!(flushed, "writer_shutdown_dead_writer_cannot_flush");

    // The honest kill-9 record: nothing pending was durable, nothing was released, and the queue
    // is handed over rather than shut down. It is accepted, which is what lets the next process
    // take over.
    let killed = clean(WriterShutdownMode::HardKill).expect("hard kill");
    assert_eq!(killed.durable_cursor_after, killed.durable_cursor_before);
    assert!(!killed.lock_released);
}

#[test]
fn a_shutdown_record_for_an_unknown_or_already_closed_queue_is_refused() {
    let unknown = WriterShutdown::new(
        lock(),
        holder(),
        EPOCH,
        WriterShutdownMode::Graceful,
        WriterQueueState::Unknown,
        0,
        0,
        10,
        12,
        12,
        0,
        Vec::new(),
        true,
    )
    .expect_err("unknown state");
    assert_eq!(unknown, "writer_shutdown_state_unknown");

    let closed = WriterShutdown::new(
        lock(),
        holder(),
        EPOCH,
        WriterShutdownMode::Graceful,
        WriterQueueState::Closed,
        0,
        0,
        10,
        12,
        12,
        0,
        Vec::new(),
        true,
    )
    .expect_err("already closed");
    assert_eq!(closed, "writer_shutdown_already_closed");
}

#[test]
fn a_live_holder_may_not_be_displaced() {
    let previous = registry();
    let takeover = WriterTakeover::new(
        lock(),
        previous.holder_instance,
        previous.registry_digest.clone(),
        successor(),
        "daemon-secondary",
        NEXT_EPOCH,
        next_fence(),
        false,
    )
    .expect("takeover");
    let report = WriterTakeoverReport::evaluate(&previous, &takeover).expect("report");
    assert_eq!(report.status, WriterAdmissionStatus::Rejected);
    assert_eq!(report.reason, "writer_takeover_live_holder");
}

#[test]
fn a_takeover_of_an_expired_lease_advances_the_epoch_and_the_fence_token() {
    // This is the card's success path: a hard-killed holder is replaced, the epoch moves forward
    // so the dead writer is fenced, and the successor may write.
    let previous = registry();
    let takeover = WriterTakeover::new(
        lock(),
        previous.holder_instance,
        previous.registry_digest.clone(),
        successor(),
        "daemon-secondary",
        NEXT_EPOCH,
        next_fence(),
        true,
    )
    .expect("takeover");
    let report = WriterTakeoverReport::evaluate(&previous, &takeover).expect("report");
    assert_eq!(report.status, WriterAdmissionStatus::Accepted);
    assert_eq!(report.reason, "writer_takeover_accepted");

    // The successor is now the holder and may append under the new epoch.
    let successor_registry = WriterRegistry::new(
        root(),
        lock(),
        successor(),
        "daemon-secondary",
        NEXT_EPOCH,
        next_fence(),
        ACQUIRED,
        EXPIRES,
    )
    .expect("successor registry");
    let append = WriterAdmission::new(
        successor(),
        RunId::parse_str("00000000-0000-5000-8000-000000000025").expect("successor run"),
        "daemon-secondary",
        NEXT_EPOCH,
        WriterQueueState::Open,
        0,
        0,
        0,
        1,
        128,
    )
    .expect("admission");
    let admitted =
        WriterAdmissionReport::evaluate(&policy(), &successor_registry, &append).expect("report");
    assert_eq!(admitted.status, WriterAdmissionStatus::Accepted);
}

#[test]
fn a_takeover_that_reuses_the_epoch_or_the_fence_token_is_refused() {
    let previous = registry();
    for (epoch, token, reason) in [
        (EPOCH, next_fence(), "writer_takeover_epoch_not_advanced"),
        (NEXT_EPOCH, fence(), "writer_takeover_fence_token_reused"),
    ] {
        let takeover = WriterTakeover::new(
            lock(),
            previous.holder_instance,
            previous.registry_digest.clone(),
            successor(),
            "daemon-secondary",
            epoch,
            token,
            true,
        )
        .expect("takeover");
        let report = WriterTakeoverReport::evaluate(&previous, &takeover).expect("report");
        assert_eq!(
            report.status,
            WriterAdmissionStatus::Rejected,
            "epoch={epoch}"
        );
        assert_eq!(report.reason, reason);
    }
}

#[test]
fn a_takeover_built_from_another_registry_is_refused() {
    let previous = registry();
    // A takeover bound to a digest that is not the registry on disk is not reasoning about the
    // right state at all, so the digest binding is checked before liveness.
    let takeover = WriterTakeover::new(
        lock(),
        previous.holder_instance,
        format!("sha256:{}", "0".repeat(64)),
        successor(),
        "daemon-secondary",
        NEXT_EPOCH,
        next_fence(),
        false,
    )
    .expect("takeover");
    let report = WriterTakeoverReport::evaluate(&previous, &takeover).expect("report");
    assert_eq!(report.reason, "writer_takeover_registry_digest_mismatch");
}

#[test]
fn a_policy_without_bounds_or_with_a_cap_that_cannot_bind_is_refused() {
    let zero = WriterQueuePolicy::new(0, 4_096, 4, 2, 1_024, 5_000).expect_err("no depth bound");
    assert_eq!(zero, "writer_queue_policy_bounds_missing");

    let unbounded =
        WriterQueuePolicy::new(8, 4_096, 0, 2, 1_024, 5_000).expect_err("no writer cap");
    assert_eq!(unbounded, "writer_queue_policy_writer_count_invalid");

    // A per-writer cap above the queue cap can never bind, so the per-writer resource limit
    // would look enforced while it is not.
    let inert = WriterQueuePolicy::new(8, 4_096, 4, 99, 1_024, 5_000).expect_err("inert cap");
    assert_eq!(inert, "writer_queue_policy_pending_exceeds_queue");
}

#[test]
fn the_queue_bounds_are_derived_from_the_registered_capacity_envelope() {
    // The card depends on PD-06/09 and DEP-17's capacity vocabulary. This slice reuses those
    // numbers rather than declaring a second set that could drift away from them.
    let envelope = CapacityEnvelope {
        journal_max_bytes: 1_000_000,
        journal_max_events: 10_000,
        max_frame_bytes: 4_096,
        max_event_bytes: 512,
        max_batch_events: 8,
        max_page_events: 64,
        max_export_records: 1_000,
        observability_queue_capacity: 64,
        max_artifact_bytes: 2_000_000,
        high_cardinality_rejected: true,
        oversize_rejected: true,
        backpressure_preserves_facts: true,
    };
    let derived = WriterQueuePolicy::from_capacity_envelope(&envelope, 8_192, 4, 5_000)
        .expect("derived policy");
    assert_eq!(
        derived.max_queue_depth,
        envelope.observability_queue_capacity
    );
    assert_eq!(derived.max_pending_per_writer, envelope.max_batch_events);
    assert_eq!(derived.max_frame_bytes, envelope.max_event_bytes);
    assert!(derived.validate().is_ok());

    // An envelope that dropped its backpressure-preserves-facts guard is refused before it can
    // become a writer queue policy.
    let unguarded = CapacityEnvelope {
        backpressure_preserves_facts: false,
        ..envelope
    };
    let error = WriterQueuePolicy::from_capacity_envelope(&unguarded, 8_192, 4, 5_000)
        .expect_err("no backpressure guard");
    assert_eq!(error, "performance_safety_guard_missing");
}

#[test]
fn a_lease_that_expires_before_it_was_acquired_is_refused() {
    let window = WriterRegistry::new(
        root(),
        lock(),
        holder(),
        "daemon-primary",
        EPOCH,
        fence(),
        ACQUIRED,
        ACQUIRED,
    )
    .expect_err("empty lease window");
    assert_eq!(window, "writer_registry_lease_window_invalid");
}

#[test]
fn an_unredacted_or_oversize_writer_label_is_refused() {
    // A writer label travels into logs, receipts and the takeover report, so it crosses the same
    // receipt-channel boundary as any other stored text. A credential-shaped label must be
    // refused by name, not merely by length.
    let leaky = WriterRegistry::new(
        root(),
        lock(),
        holder(),
        "daemon api_key=sk-live-0123456789abcdef",
        EPOCH,
        fence(),
        ACQUIRED,
        EXPIRES,
    )
    .expect_err("credential-shaped writer label");
    assert_eq!(leaky, "writer_registry_holder_secret_detected");

    // `redact_text` would rewrite this one, so it is refused as not-yet-redacted rather than
    // silently stored with the marker intact.
    let unmarked = WriterRegistry::new(
        root(),
        lock(),
        holder(),
        "daemon token=abcdef",
        EPOCH,
        fence(),
        ACQUIRED,
        EXPIRES,
    );
    assert!(
        unmarked.is_err(),
        "an unredacted writer label must be refused"
    );

    let long = WriterRegistry::new(
        root(),
        lock(),
        holder(),
        "w".repeat(kiana_eventlog::MAX_WRITER_LABEL + 1),
        EPOCH,
        fence(),
        ACQUIRED,
        EXPIRES,
    )
    .expect_err("oversize writer label");
    assert_eq!(long, "writer_registry_holder_invalid");
}

#[test]
fn a_forged_report_or_digest_fails_closed_and_the_decision_is_deterministic() {
    let policy = policy();
    let registry = registry();
    let admission = admission(WriterQueueState::Open, 8, 1_000, 0, 1, 512);

    let report = WriterAdmissionReport::evaluate(&policy, &registry, &admission).expect("report");
    // Admitting a frame the queue refused must not survive revalidation.
    let mut forged = report.clone();
    forged.status = WriterAdmissionStatus::Accepted;
    forged.reason = "writer_append_admitted".to_owned();
    forged.report_digest = forged.digest();
    let error = forged
        .validate_against(&policy, &registry, &admission)
        .expect_err("forged acceptance");
    assert_eq!(error, "writer_admission_report_binding_invalid");

    // A stale digest must not authenticate a rewritten reason either.
    let mut tampered = report.clone();
    tampered.reason = "writer_queue_closed".to_owned();
    let error = tampered
        .validate_against(&policy, &registry, &admission)
        .expect_err("stale digest");
    assert_eq!(error, "writer_admission_report_binding_invalid");

    // The same facts always yield the same first violated rule.
    let first = WriterAdmissionReport::evaluate(&policy, &registry, &admission).expect("first");
    let second = WriterAdmissionReport::evaluate(&policy, &registry, &admission).expect("second");
    assert_eq!(first, second);
    assert_eq!(first.reason, "writer_queue_full");
}

#[test]
fn the_schemas_are_versioned_and_a_health_ack_still_cannot_promote_the_proof_level() {
    let value = registry();
    assert_eq!(value.schema, "kiana.writer-registry.v1");
    assert_eq!(value.version, WRITER_QUEUE_VERSION);
    assert_eq!(policy().schema, "kiana.writer-queue-policy.v1");
    assert_eq!(
        admission(WriterQueueState::Open, 0, 0, 0, 1, 128).schema,
        "kiana.writer-admission.v1"
    );
    assert_eq!(
        WriterAdmissionReport::evaluate(
            &policy(),
            &value,
            &admission(WriterQueueState::Open, 0, 0, 0, 1, 128)
        )
        .expect("report")
        .schema,
        "kiana.writer-admission-report.v1"
    );
    assert_eq!(
        clean(WriterShutdownMode::Graceful)
            .expect("shutdown")
            .schema,
        "kiana.writer-shutdown.v1"
    );

    // The adapter's flush/close acknowledgement remains an acknowledgement. Reading one does not
    // establish that the queue is bounded, only that a flush or close call returned.
    let health = EventStoreHealth::new(3, true, true, 12, true);
    assert!(health.validate().is_ok());
    assert!(health.closed);
    assert_eq!(health.last_durable_cursor, 12);
}
