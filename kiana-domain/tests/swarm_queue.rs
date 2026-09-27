use kiana_domain::{
    DispatchIntentId, QueueEntryId, SwarmQueueEntry, SwarmQueueEntryState, SwarmQueueLedger,
    SWARM_QUEUE_ENTRY_SCHEMA,
};
use serde_json::json;

const FINGERPRINT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn entry(sequence: u64, session: &str) -> SwarmQueueEntry {
    let mut value = SwarmQueueEntry {
        schema: SWARM_QUEUE_ENTRY_SCHEMA.to_owned(),
        entry_id: QueueEntryId::new(),
        dispatch_intent_id: DispatchIntentId::new(),
        work_fingerprint: FINGERPRINT.to_owned(),
        session_key: session.to_owned(),
        sequence,
        attempt: 0,
        max_attempts: 2,
        not_before_unix_ms: 1,
        fence_epoch: 7,
        state: SwarmQueueEntryState::Ready,
        entry_digest: String::new(),
    };
    value.entry_digest = value.canonical_digest();
    value
}

#[test]
fn queue_claim_is_capacity_bound_and_fairly_ordered() {
    let mut ledger = SwarmQueueLedger::new(1, 7).unwrap();
    let first = entry(2, "session-b");
    let second = entry(1, "session-a");
    let second_id = second.entry_id;
    ledger.enqueue(first).unwrap();
    ledger.enqueue(second).unwrap();
    let claimed = ledger.claim(10).unwrap();
    assert_eq!(claimed.entry_id, second_id);
    assert_eq!(claimed.state, SwarmQueueEntryState::Claimed);
    assert_eq!(ledger.claim(10), Err("swarm_queue_capacity_exceeded"));
}

#[test]
fn stale_fence_and_expired_delay_are_rejected() {
    let mut ledger = SwarmQueueLedger::new(1, 7).unwrap();
    let mut stale = entry(1, "session-a");
    stale.fence_epoch = 6;
    stale.entry_digest = stale.canonical_digest();
    assert_eq!(ledger.enqueue(stale), Err("swarm_queue_stale_fence"));

    let mut delayed = entry(1, "session-a");
    delayed.not_before_unix_ms = 100;
    delayed.entry_digest = delayed.canonical_digest();
    let id = delayed.entry_id;
    ledger.enqueue(delayed).unwrap();
    assert_eq!(ledger.claim(99), Err("swarm_queue_no_ready_entry"));
    let claimed = ledger.claim(100).unwrap();
    assert_eq!(claimed.entry_id, id);
}

#[test]
fn retry_backoff_and_max_attempt_fence_are_bounded() {
    let mut ledger = SwarmQueueLedger::new(1, 7).unwrap();
    let value = entry(1, "session-a");
    let id = value.entry_id;
    ledger.enqueue(value).unwrap();
    ledger.claim(1).unwrap();
    assert_eq!(
        ledger.retry(id, 7, 1, 0),
        Err("swarm_queue_backoff_invalid")
    );
    ledger.retry(id, 7, 1, 10).unwrap();
    ledger.claim(11).unwrap();
    ledger.retry(id, 7, 11, 10).unwrap();
    assert_eq!(ledger.claim(21), Err("swarm_queue_no_ready_entry"));
}

#[test]
fn unknown_fields_and_digest_drift_fail_closed() {
    let mut value = serde_json::to_value(entry(1, "session-a")).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmQueueEntry>(value).is_err());

    let mut invalid = entry(1, "session-a");
    invalid.entry_digest = FINGERPRINT.to_owned();
    assert_eq!(invalid.validate(), Err("swarm_queue_entry_invalid"));
}
