use kiana_domain::{InputDisposition, RunId};
use kiana_runner::{Inbox, InboxMessage, InboxTarget};

#[test]
fn duplicate_input_is_consumed_once() {
    let run_id = RunId::new();
    let mut inbox = Inbox::default();
    let message = InboxMessage::user("same");
    let input_id = message.input_id;
    let first = inbox
        .insert_for_run(run_id, InboxTarget::NextStep, message.clone())
        .unwrap();
    assert_eq!(first.disposition, InputDisposition::Accepted);
    let duplicate = inbox
        .insert_for_run(run_id, InboxTarget::NextStep, message)
        .unwrap();
    assert_eq!(duplicate.disposition, InputDisposition::Duplicate);
    assert_eq!(inbox.claim(InboxTarget::NextStep).len(), 1);
    assert!(inbox.claimed(input_id));
    let replay = inbox
        .insert_for_run(
            run_id,
            InboxTarget::NextStep,
            InboxMessage {
                input_id,
                ..InboxMessage::user("same")
            },
        )
        .unwrap();
    assert_eq!(replay.disposition, InputDisposition::Claimed);
    assert!(!inbox.has_pending());
}

#[test]
fn checkpoint_round_trip_preserves_accept_order_and_target() {
    let run_id = RunId::new();
    let mut inbox = Inbox::default();
    let mut first = InboxMessage::from_source("web", "first");
    first.target_turn_id = Some(kiana_domain::TurnId::new());
    inbox
        .insert_for_run(run_id, InboxTarget::NextTurn, first)
        .unwrap();
    inbox
        .insert_for_run(run_id, InboxTarget::NextStep, InboxMessage::user("second"))
        .unwrap();
    let encoded = serde_json::to_value(&inbox).unwrap();
    let restored: Inbox = serde_json::from_value(encoded).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.next_turn[0].source, "web");
    assert_eq!(restored.next_turn[0].target, InboxTarget::NextTurn);
    assert_eq!(restored.next_step[0].text, "second");
    assert_eq!(restored.next_sequence(), 2);
}

#[test]
fn cross_turn_input_is_rejected_at_claim_boundary() {
    let run_id = RunId::new();
    let current = kiana_domain::TurnId::new();
    let foreign = kiana_domain::TurnId::new();
    let mut message = InboxMessage::user("stale steer");
    message.target_turn_id = Some(foreign);
    let mut inbox = Inbox::default();
    inbox
        .insert_for_run(run_id, InboxTarget::NextStep, message)
        .unwrap();
    let claimed = inbox.claim(InboxTarget::NextStep);
    assert_eq!(claimed.len(), 1);
    assert_ne!(claimed[0].target_turn_id, Some(current));
}
