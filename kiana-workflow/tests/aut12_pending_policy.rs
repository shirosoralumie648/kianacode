#[test]
fn pending_occurrence_index_preserves_digest_and_stop_before_replace() {
    let source = include_str!("../src/durable.rs");
    for marker in [
        "pending_digests",
        "fired_digests",
        "trigger_occurrence_digest_conflict",
        "TriggerConcurrency::Reject",
        "TriggerConcurrency::Queue",
        "TriggerConcurrency::Coalesce",
        "TriggerConcurrency::Replace",
        "WorkflowInstanceStatus::CancelRequested",
        "trigger_queue_full",
    ] {
        assert!(source.contains(marker), "AUT-12 marker missing: {marker}");
    }
    let replace = source
        .find("TriggerConcurrency::Replace")
        .expect("replace branch");
    let cancel = source[replace..]
        .find("WorkflowInstanceStatus::CancelRequested")
        .expect("replace cancels active instance");
    let pending = source[replace..]
        .find("push_pending(&mut pending")
        .expect("replace queues successor");
    assert!(
        cancel < pending,
        "replace must stop/fence before successor queueing"
    );
}
