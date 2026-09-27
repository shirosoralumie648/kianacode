//! PD-27 source guard: the writer queue is a decision over reported facts, not a second writer.
//!
//! PD-06/ER-06 already put a bounded writer queue and observable flush/close in `jsonl.rs`. This
//! guard pins the two rules PD-27 adds on top of them, and pins that the new module reaches
//! neither the filesystem nor the port layer: a queue contract that opened a file, took an OS
//! lock or appended an event would be a second fact source, and its decision could not be
//! re-derived from the evidence alone.

/// Search `source` from `anchor` onward, so an ordering assertion is not fooled by a marker that
/// also appears in a module doc comment above the reducer.
fn after(source: &str, anchor: &str, marker: &str) -> usize {
    let start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("PD-27 anchor missing: {anchor}"));
    source[start..]
        .find(marker)
        .unwrap_or_else(|| panic!("PD-27 marker missing after {anchor}: {marker}"))
        + start
}

#[test]
fn pd27_a_full_queue_refuses_observably_and_a_late_append_is_refused() {
    let queue = include_str!("../src/writer_queue.rs");
    for marker in [
        "WriterQueuePolicy",
        "WriterQueueState",
        "WriterRegistry",
        "WriterTakeover",
        "WriterTakeoverReport",
        "WriterAdmission",
        "WriterAdmissionReport",
        "WriterShutdown",
        "WriterShutdownMode",
        "WriterAdmissionStatus",
        "WRITER_QUEUE_POLICY_SCHEMA",
        "WRITER_REGISTRY_SCHEMA",
        "WRITER_TAKEOVER_SCHEMA",
        "WRITER_TAKEOVER_REPORT_SCHEMA",
        "WRITER_ADMISSION_SCHEMA",
        "WRITER_ADMISSION_REPORT_SCHEMA",
        "WRITER_SHUTDOWN_SCHEMA",
        "WRITER_QUEUE_VERSION",
        "MAX_QUEUE_WRITERS",
        "MAX_OUTSTANDING_LEASES",
        "writer_queue_full",
        "writer_queue_bytes_exceeded",
        "writer_per_writer_limit_exceeded",
        "writer_frame_oversize",
        "writer_batch_oversize",
        "writer_queue_closed",
        "writer_queue_draining",
        "writer_queue_state_unknown",
        "writer_instance_not_registry_holder",
        "writer_epoch_stale",
        "writer_append_admitted",
    ] {
        assert!(queue.contains(marker), "PD-27 marker missing: {marker}");
    }
    // A silent drop would report nothing, so nothing in this module may be named as one.
    for forbidden in ["silently_dropped", "drop_silently", "best_effort_append"] {
        assert!(
            !queue.contains(forbidden),
            "PD-27 introduced a silent-drop path: {forbidden}"
        );
    }
    assert!(
        !queue.contains("eventlog_worker_queue_full"),
        "PD-27 must not redefine the existing PD-06 saturation code"
    );
}

#[test]
fn pd27_a_refused_admission_can_never_carry_admitted_depth_or_bytes() {
    let queue = include_str!("../src/writer_queue.rs");
    assert!(
        queue.contains(
            "if self.status == WriterAdmissionStatus::Rejected\n            && (self.accepted_depth_after != 0 || self.accepted_bytes_after != 0)"
        ),
        "PD-27 must reject a refusal that still reports admitted depth or bytes"
    );
    assert!(
        queue.contains("writer_report_admitted_depth_leak"),
        "PD-27 must name the admitted-depth leak refusal"
    );
    // The reducer keeps every refusal's admitted values at zero, not only the revalidator.
    let refused = after(queue, "fn derive_admission", "let refused = |reason:");
    assert!(
        refused > 0,
        "PD-27 admission reducer must hold one zero-valued refusal shape"
    );
}

#[test]
fn pd27_an_append_after_shutdown_returned_and_an_unestablished_state_are_both_refused() {
    let queue = include_str!("../src/writer_queue.rs");
    // Unknown is checked before Closed and before the capacity rules: an unestablished state
    // cannot be diagnosed, and must never be reported as a specific refusal.
    let unknown = after(queue, "fn derive_admission", "writer_queue_state_unknown");
    let closed = after(queue, "fn derive_admission", "writer_queue_closed");
    let full = after(queue, "fn derive_admission", "writer_queue_full");
    assert!(
        unknown < closed && closed < full,
        "PD-27 admission order changed: unknown {unknown}, closed {closed}, full {full}"
    );
    // Only an open queue may append; draining and closed both refuse.
    assert!(
        queue.contains("matches!(self, Self::Open)"),
        "PD-27 must admit appends only on an open queue"
    );
}

#[test]
fn pd27_a_cancelled_writer_may_not_leave_a_lock_or_a_lease_behind() {
    let queue = include_str!("../src/writer_queue.rs");
    for marker in [
        "writer_shutdown_outstanding_writer",
        "writer_shutdown_pending_remainder",
        "writer_shutdown_lease_residue",
        "writer_shutdown_lock_not_released",
        "writer_shutdown_unflushed_remainder",
        "writer_shutdown_durable_cursor_regression",
        "writer_shutdown_state_unknown",
        "writer_shutdown_already_closed",
        "writer_shutdown_dead_writer_released_lock",
        "writer_shutdown_dead_writer_cannot_flush",
        "hard_kill",
    ] {
        assert!(queue.contains(marker), "PD-27 marker missing: {marker}");
    }
    // A kill-9 runs no code, so a dead writer can claim neither a release nor new durability.
    assert!(
        queue.contains("matches!(self, Self::Graceful | Self::Cancelled)"),
        "PD-27 must scope post-kill effect claims to the modes that can run code"
    );
    // Shutdown must not return while a writer is still appending behind it. The anchor is the
    // `impl WriterShutdown` block, not `pub fn validate(&self)`, which every validator in this
    // module shares -- anchoring on the generic signature would silently measure another type.
    let outstanding = after(
        queue,
        "impl WriterShutdown {",
        "writer_shutdown_outstanding_writer",
    );
    let remainder = after(
        queue,
        "impl WriterShutdown {",
        "writer_shutdown_pending_remainder",
    );
    let released = after(
        queue,
        "impl WriterShutdown {",
        "writer_shutdown_lock_not_released",
    );
    assert!(
        remainder < outstanding && outstanding < released,
        "PD-27 shutdown order changed: remainder {remainder}, outstanding {outstanding}, released {released}"
    );
}

#[test]
fn pd27_a_hard_killed_holder_is_replaced_only_by_an_advanced_epoch_and_a_fresh_fence() {
    let queue = include_str!("../src/writer_queue.rs");
    for marker in [
        "writer_takeover_live_holder",
        "writer_takeover_epoch_not_advanced",
        "writer_takeover_fence_token_reused",
        "writer_takeover_registry_digest_mismatch",
        "writer_takeover_lock_mismatch",
        "writer_takeover_accepted",
    ] {
        assert!(queue.contains(marker), "PD-27 marker missing: {marker}");
    }
    // Identity binding first (the digest on disk is the state being reasoned about), then
    // liveness, then monotonic authority.
    let digest = after(
        queue,
        "fn derive_takeover",
        "writer_takeover_registry_digest_mismatch",
    );
    let live = after(queue, "fn derive_takeover", "writer_takeover_live_holder");
    let epoch = after(
        queue,
        "fn derive_takeover",
        "writer_takeover_epoch_not_advanced",
    );
    let fence = after(
        queue,
        "fn derive_takeover",
        "writer_takeover_fence_token_reused",
    );
    assert!(
        digest < live && live < epoch && epoch < fence,
        "PD-27 takeover order changed: digest {digest}, live {live}, epoch {epoch}, fence {fence}"
    );
    // A lease that expires at or before it was acquired never bounded anything.
    assert!(
        queue.contains("if self.expires_at_unix_ms <= self.acquired_at_unix_ms"),
        "PD-27 must reject an empty lease window"
    );
}

#[test]
fn pd27_the_queue_reuses_the_registered_capacity_vocabulary_and_decides_nothing_else() {
    let queue = include_str!("../src/writer_queue.rs");
    // Reuse, not a second vocabulary: the bounds come from the envelope PD-17 registered.
    assert!(
        queue.contains("pub fn from_capacity_envelope("),
        "PD-27 must derive its bounds from the registered capacity envelope"
    );
    assert!(
        queue.contains("envelope.observability_queue_capacity"),
        "PD-27 must read the queue depth from the registered capacity envelope"
    );
    assert!(
        queue.contains("envelope.max_batch_events"),
        "PD-27 must read the per-writer cap from the registered capacity envelope"
    );
    assert!(
        queue.contains("envelope.max_event_bytes"),
        "PD-27 must read the frame cap from the registered capacity envelope"
    );
    // A per-writer cap above the queue cap is a cap that can never bind.
    assert!(
        queue.contains("if self.max_pending_per_writer > self.max_queue_depth"),
        "PD-27 must reject a per-writer cap the queue cap would always preempt"
    );

    // The module decides; it must not perform the effects it decides about.
    for forbidden in [
        "std::fs",
        "std::io",
        "std::process",
        "tokio::",
        "flock",
        "File::",
        "spawn_blocking",
        "Semaphore",
        "EventStorePort",
        "ArtifactStorePort",
        "sync_all",
        "set_len",
    ] {
        assert!(
            !queue.contains(forbidden),
            "PD-27 writer queue crossed effect boundary: {forbidden}"
        );
    }
}

#[test]
fn pd27_jsonl_still_refuses_observably_rather_than_waiting_or_dropping() {
    // The pre-existing bounded queue this contract describes. PD-27 adds the decision contract
    // above it; it must not have been quietly replaced by an unbounded wait or a silent drop.
    let jsonl = include_str!("../src/jsonl.rs");
    for marker in [
        "MAX_STORAGE_WORKERS",
        "try_acquire_owned",
        "PortError::Unavailable(\"eventlog_worker_queue_full\".into())",
        "failed(\"eventlog_closed\")",
        "eventlog_close_in_progress",
        "compare_exchange",
        "flush_store",
    ] {
        assert!(
            jsonl.contains(marker),
            "PD-27 JSONL marker missing: {marker}"
        );
    }
    for forbidden in [
        "eventlog_silent_drop",
        "eventlog_event_dropped",
        "unwrap_or_default()",
    ] {
        assert!(
            !jsonl.contains(forbidden),
            "PD-27 JSONL queue introduced a silent-drop path: {forbidden}"
        );
    }
}
